//! Per-key state machine (docs/architecture.md "키별 동작").

use crate::decode::Lattice;
use crate::english::EnglishInterpreter;
use crate::hangul::HangulInterpreter;
use crate::ja_asset::JaAsset;
use crate::kanji::{Connection, Dictionary, SampleDictionary};
use crate::romaji::JapaneseInterpreter;
use crate::scorer::{BaselineScorer, Scorer};
use crate::{Interpreter, Key, KeyEvent, Preedit, Response, Segment, Stroke};

struct Seg {
    start: usize,
    end: usize,
    candidates: Vec<crate::Candidate>,
    selected: usize,
    /// Chosen with Up/Down; later keystrokes leave it alone.
    pinned: bool,
}

pub struct Engine {
    /// Every composing keystroke so far; Backspace pops one (D9).
    strokes: Vec<Stroke>,
    segs: Vec<Seg>,
    focused: usize,
    /// DP over `strokes[lattice_from..]`, the part after the last pinned
    /// segment; kept across keys so a keystroke only decodes the spans that
    /// end at it.
    lattice: Lattice,
    lattice_from: usize,
    interpreters: Vec<Box<dyn Interpreter>>,
    scorer: Box<dyn Scorer>,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    /// Japanese uses the built-in `SampleDictionary`; see `with_ja_dictionary`.
    pub fn new() -> Engine {
        Engine::with_ja_model(SampleDictionary)
    }

    /// Japanese conversion from a loaded `nullime-ja.dict` (D15). The engine
    /// keeps the asset, which shares its bytes (docs/architecture.md
    /// "일본어 사전 애셋").
    pub fn with_ja_dictionary(asset: JaAsset) -> Engine {
        Engine::with_ja_model(asset)
    }

    fn with_ja_model(model: impl Dictionary + Connection + 'static) -> Engine {
        Engine::with_parts(
            vec![
                Box::new(EnglishInterpreter),
                Box::new(HangulInterpreter),
                Box::new(JapaneseInterpreter { dictionary: model }),
            ],
            Box::new(BaselineScorer::default()),
        )
    }

    /// Swap in other interpreters or a different scorer (D7).
    pub fn with_parts(interpreters: Vec<Box<dyn Interpreter>>, scorer: Box<dyn Scorer>) -> Engine {
        Engine {
            strokes: Vec::new(),
            segs: Vec::new(),
            focused: 0,
            lattice: Lattice::default(),
            lattice_from: 0,
            interpreters,
            scorer,
        }
    }

    pub fn key(&mut self, ev: KeyEvent) -> Response {
        if ev.modifier {
            return self.finish();
        }
        let composing = !self.strokes.is_empty();
        match ev.key {
            Key::Char(ch) => match normalize(ch, ev.shift) {
                Some(stroke) => {
                    self.strokes.push(stroke);
                    self.redecode();
                    self.composing_response()
                }
                None => self.finish(),
            },
            Key::Space | Key::Enter | Key::Other => self.finish(),
            _ if !composing => pass(),
            Key::Backspace => {
                self.strokes.pop();
                let len = self.strokes.len();
                self.segs.retain(|s| s.end <= len);
                self.lattice.truncate(len.saturating_sub(self.lattice_from));
                if len == 0 {
                    self.focused = 0;
                    return Response { handled: true, commit: String::new(), preedit: None };
                }
                self.redecode();
                self.composing_response()
            }
            Key::Left => {
                self.focused = self.focused.saturating_sub(1);
                self.composing_response()
            }
            Key::Right => {
                self.focused = (self.focused + 1).min(self.segs.len() - 1);
                self.composing_response()
            }
            Key::Up | Key::Down => {
                let seg = &mut self.segs[self.focused];
                let n = seg.candidates.len();
                seg.selected = if ev.key == Key::Down {
                    (seg.selected + 1) % n
                } else {
                    (seg.selected + n - 1) % n
                };
                seg.pinned = true;
                self.composing_response()
            }
        }
    }

    /// Commits whatever is composing and closes the composition window.
    pub fn reset(&mut self) -> Response {
        self.finish()
    }

    fn finish(&mut self) -> Response {
        let commit = self.segments().iter().map(|s| s.text.as_str()).collect();
        self.strokes.clear();
        self.segs.clear();
        self.focused = 0;
        self.lattice = Lattice::default();
        self.lattice_from = 0;
        Response { handled: false, commit, preedit: None }
    }

    /// Re-reads everything after the last pinned segment.
    fn redecode(&mut self) {
        let keep = self.segs.iter().rposition(|s| s.pinned).map_or(0, |i| i + 1);
        self.segs.truncate(keep);
        let from = self.segs.last().map_or(0, |s| s.end);
        if from != self.lattice_from {
            self.lattice = Lattice::default();
            self.lattice_from = from;
        }
        let tail = &self.strokes[from..];
        self.lattice.truncate(self.lattice.len().min(tail.len()));
        self.lattice.extend(tail, &self.interpreters, self.scorer.as_ref());
        let spans = self.lattice.spans();
        self.segs.extend(spans.into_iter().map(|span| Seg {
            start: from + span.start,
            end: from + span.end,
            candidates: span.candidates,
            selected: 0,
            pinned: false,
        }));
        self.focused = self.segs.len().saturating_sub(1);
    }

    fn segments(&self) -> Vec<Segment> {
        self.segs
            .iter()
            .map(|s| {
                let top = &s.candidates[s.selected];
                Segment {
                    text: top.text.clone(),
                    lang: top.lang,
                    candidates: s.candidates.clone(),
                    selected: s.selected,
                }
            })
            .collect()
    }

    fn composing_response(&self) -> Response {
        debug_assert!(self.segs.iter().all(|s| s.start < s.end));
        Response {
            handled: true,
            commit: String::new(),
            preedit: Some(Preedit { segments: self.segments(), focused: self.focused }),
        }
    }
}

fn pass() -> Response {
    Response { handled: false, commit: String::new(), preedit: None }
}

/// Uppercase letters are folded to lowercase + Shift so frontends that send
/// either form behave the same.
fn normalize(ch: u8, shift: bool) -> Option<Stroke> {
    match ch {
        b'A'..=b'Z' => Some(Stroke { ch: ch.to_ascii_lowercase(), shift: true }),
        0x21..=0x7E => Some(Stroke { ch, shift }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Lang;

    fn press(e: &mut Engine, key: Key) -> Response {
        e.key(KeyEvent::new(key))
    }

    fn type_str(e: &mut Engine, s: &str) -> Response {
        let mut last = None;
        for b in s.bytes() {
            last = Some(match b {
                b' ' => press(e, Key::Space),
                b'A'..=b'Z' => e.key(KeyEvent {
                    key: Key::Char(b.to_ascii_lowercase()),
                    shift: true,
                    modifier: false,
                }),
                _ => press(e, Key::Char(b)),
            });
        }
        last.unwrap()
    }

    fn preedit_text(r: &Response) -> String {
        r.preedit.as_ref().unwrap().segments.iter().map(|s| s.text.as_str()).collect()
    }

    fn all_candidates(r: &Response) -> Vec<String> {
        let p = r.preedit.as_ref().unwrap();
        p.segments.iter().flat_map(|s| s.candidates.iter().map(|c| c.text.clone())).collect()
    }

    // Catches Korean losing to the literal English reading of 두벌식 keys.
    #[test]
    fn dkssud_reads_as_korean_with_english_candidate() {
        let r = type_str(&mut Engine::new(), "dkssud");
        let p = r.preedit.unwrap();
        assert_eq!(p.segments.len(), 1);
        assert_eq!(p.segments[0].text, "안녕");
        assert_eq!(p.segments[0].lang, Lang::Ko);
        assert!(p.segments[0].candidates.iter().any(|c| c.text == "dkssud" && c.lang == Lang::En));
    }

    // Catches English words being split or read as Hangul/Romaji.
    #[test]
    fn hello_reads_as_english() {
        let r = type_str(&mut Engine::new(), "hello");
        let p = r.preedit.unwrap();
        assert_eq!(p.segments[0].text, "hello");
        assert_eq!(p.segments[0].lang, Lang::En);
    }

    // Catches Japanese readings or sample-dictionary kanji missing from candidates.
    #[test]
    fn japanese_readings_reachable() {
        let c = all_candidates(&type_str(&mut Engine::new(), "sake"));
        assert!(c.contains(&"さけ".to_string()));
        assert!(c.contains(&"酒".to_string()));
        assert!(all_candidates(&type_str(&mut Engine::new(), "kitte")).contains(&"きって".into()));
        let c = all_candidates(&type_str(&mut Engine::new(), "konnichiha"));
        assert!(c.contains(&"こんにちは".to_string()));
    }

    // Catches D9 regressions: Backspace must undo one keystroke, not a whole syllable.
    #[test]
    fn backspace_undoes_last_keystroke() {
        let mut e = Engine::new();
        type_str(&mut e, "rksk");
        let r = press(&mut e, Key::Backspace);
        assert_eq!(preedit_text(&r), "간");
        assert!(r.handled);
    }

    // Catches Backspace on the last keystroke leaving an empty window open, or D10 being handled.
    #[test]
    fn backspace_to_empty_then_idle() {
        let mut e = Engine::new();
        type_str(&mut e, "r");
        let r = press(&mut e, Key::Backspace);
        assert!(r.handled && r.preedit.is_none());
        assert!(!press(&mut e, Key::Backspace).handled);
    }

    // Catches D5 regressions: Space commits the top reading and still reaches the app.
    #[test]
    fn space_commits_and_passes_through() {
        let mut e = Engine::new();
        type_str(&mut e, "dkssud");
        let r = press(&mut e, Key::Space);
        assert_eq!(r, Response { handled: false, commit: "안녕".into(), preedit: None });
        let r = press(&mut e, Key::Space);
        assert_eq!(r, Response { handled: false, commit: String::new(), preedit: None });
    }

    // Catches D13 regressions: digits/punctuation join the composition next to Hangul.
    #[test]
    fn digits_units_and_korean_commit_together() {
        let mut e = Engine::new();
        let r = type_str(&mut e, "0.1mmwjdeh");
        assert_eq!(preedit_text(&r), "0.1mm정도");
        let r = press(&mut e, Key::Space);
        assert_eq!(r.commit, "0.1mm정도");
        assert!(!r.handled);
    }

    // Catches D8 regressions: Up/Down cycles candidates of the focused segment.
    #[test]
    fn up_down_change_selected_candidate() {
        let mut e = Engine::new();
        type_str(&mut e, "dkssud");
        let r = press(&mut e, Key::Down);
        let seg = &r.preedit.as_ref().unwrap().segments[0];
        assert_eq!(seg.selected, 1);
        assert_eq!(seg.text, seg.candidates[1].text);
        let r = press(&mut e, Key::Up);
        assert_eq!(r.preedit.unwrap().segments[0].selected, 0);
    }

    // Catches D8 regressions: Left/Right moves focus across segments and Up/Down follows it.
    #[test]
    fn left_right_move_focus() {
        let mut e = Engine::new();
        let r = type_str(&mut e, "0.1mmwjdeh");
        assert!(r.preedit.as_ref().unwrap().segments.len() >= 2);
        assert_eq!(r.preedit.unwrap().focused, 1);
        let r = press(&mut e, Key::Left);
        assert_eq!(r.preedit.as_ref().unwrap().focused, 0);
        let r = press(&mut e, Key::Down);
        let p = r.preedit.unwrap();
        assert_eq!(p.segments[0].selected, 1);
        assert_eq!(p.segments[1].selected, 0);
        let r = press(&mut e, Key::Right);
        assert_eq!(r.preedit.unwrap().focused, 1);
    }

    // Catches a pinned segment being re-read by later keystrokes, or staying pinned after Backspace enters it.
    #[test]
    fn pinned_segment_survives_typing_until_backspaced() {
        let mut e = Engine::new();
        type_str(&mut e, "dkssud");
        let picked = press(&mut e, Key::Down).preedit.unwrap().segments[0].text.clone();
        let r = type_str(&mut e, "gk");
        let p = r.preedit.unwrap();
        assert_eq!(p.segments[0].text, picked);
        assert_eq!(p.segments.last().unwrap().text, "하");
        press(&mut e, Key::Backspace);
        press(&mut e, Key::Backspace);
        let r = press(&mut e, Key::Backspace);
        let p = r.preedit.unwrap();
        assert_eq!(p.segments[0].selected, 0);
        assert_eq!(preedit_text_of(&p), "안녀");
    }

    // Catches the incremental lattice drifting from a fresh decode after edits and Backspace.
    #[test]
    fn incremental_matches_full_decode() {
        let mut e = Engine::new();
        type_str(&mut e, "0.1mmwjdehkonnichiha");
        for _ in 0..4 {
            press(&mut e, Key::Backspace);
        }
        type_str(&mut e, "sakedkssud");
        press(&mut e, Key::Backspace);
        type_str(&mut e, "Qk");
        let full = crate::decode::decode(&e.strokes, &e.interpreters, e.scorer.as_ref());
        let got: Vec<_> = e.segs.iter().map(|s| (s.start, s.end, s.candidates.clone())).collect();
        let want: Vec<_> = full.into_iter().map(|s| (s.start, s.end, s.candidates)).collect();
        assert_eq!(got, want);
    }

    // Catches a panic (host app abort) when no interpreter reads a span.
    #[test]
    fn unreadable_span_falls_back_to_literal() {
        let mut e = Engine::with_parts(
            vec![Box::new(JapaneseInterpreter { dictionary: SampleDictionary })],
            Box::new(BaselineScorer::default()),
        );
        let r = press(&mut e, Key::Char(b'k'));
        assert_eq!(preedit_text(&r), "k");
        press(&mut e, Key::Down);
        let r = press(&mut e, Key::Up);
        assert_eq!(preedit_text(&r), "k");
    }

    fn preedit_text_of(p: &Preedit) -> String {
        p.segments.iter().map(|s| s.text.as_str()).collect()
    }

    // Catches modifier/Other keys not committing the composition before passing through.
    #[test]
    fn modifier_commits() {
        let mut e = Engine::new();
        type_str(&mut e, "hello");
        let r = e.key(KeyEvent { key: Key::Char(b'c'), shift: false, modifier: true });
        assert_eq!(r, Response { handled: false, commit: "hello".into(), preedit: None });
    }
}
