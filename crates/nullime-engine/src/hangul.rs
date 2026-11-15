//! 두벌식 automaton: QWERTY keystrokes to Hangul syllables.

use crate::{Candidate, Interpreter, Lang, Stroke};

const CHOSEONG: &[char] = &[
    'ㄱ', 'ㄲ', 'ㄴ', 'ㄷ', 'ㄸ', 'ㄹ', 'ㅁ', 'ㅂ', 'ㅃ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅉ', 'ㅊ', 'ㅋ', 'ㅌ',
    'ㅍ', 'ㅎ',
];
// Index 0 is "no final".
const JONGSEONG: &[char] = &[
    '\0', 'ㄱ', 'ㄲ', 'ㄳ', 'ㄴ', 'ㄵ', 'ㄶ', 'ㄷ', 'ㄹ', 'ㄺ', 'ㄻ', 'ㄼ', 'ㄽ', 'ㄾ', 'ㄿ', 'ㅀ', 'ㅁ',
    'ㅂ', 'ㅄ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅊ', 'ㅋ', 'ㅌ', 'ㅍ', 'ㅎ',
];
const COMPOUND_VOWELS: &[(char, char, char)] = &[
    ('ㅗ', 'ㅏ', 'ㅘ'),
    ('ㅗ', 'ㅐ', 'ㅙ'),
    ('ㅗ', 'ㅣ', 'ㅚ'),
    ('ㅜ', 'ㅓ', 'ㅝ'),
    ('ㅜ', 'ㅔ', 'ㅞ'),
    ('ㅜ', 'ㅣ', 'ㅟ'),
    ('ㅡ', 'ㅣ', 'ㅢ'),
];
const COMPOUND_FINALS: &[(char, char, char)] = &[
    ('ㄱ', 'ㅅ', 'ㄳ'),
    ('ㄴ', 'ㅈ', 'ㄵ'),
    ('ㄴ', 'ㅎ', 'ㄶ'),
    ('ㄹ', 'ㄱ', 'ㄺ'),
    ('ㄹ', 'ㅁ', 'ㄻ'),
    ('ㄹ', 'ㅂ', 'ㄼ'),
    ('ㄹ', 'ㅅ', 'ㄽ'),
    ('ㄹ', 'ㅌ', 'ㄾ'),
    ('ㄹ', 'ㅍ', 'ㄿ'),
    ('ㄹ', 'ㅎ', 'ㅀ'),
    ('ㅂ', 'ㅅ', 'ㅄ'),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Jamo {
    Consonant(char),
    Vowel(char),
}

fn jamo_for(stroke: Stroke) -> Option<Jamo> {
    use Jamo::{Consonant as C, Vowel as V};
    // Only these seven keys have a distinct shifted jamo; other shifted
    // letters type the unshifted jamo.
    if stroke.shift {
        match stroke.ch {
            b'q' => return Some(C('ㅃ')),
            b'w' => return Some(C('ㅉ')),
            b'e' => return Some(C('ㄸ')),
            b'r' => return Some(C('ㄲ')),
            b't' => return Some(C('ㅆ')),
            b'o' => return Some(V('ㅒ')),
            b'p' => return Some(V('ㅖ')),
            _ => {}
        }
    }
    let jamo = match stroke.ch {
        b'q' => C('ㅂ'),
        b'w' => C('ㅈ'),
        b'e' => C('ㄷ'),
        b'r' => C('ㄱ'),
        b't' => C('ㅅ'),
        b'y' => V('ㅛ'),
        b'u' => V('ㅕ'),
        b'i' => V('ㅑ'),
        b'o' => V('ㅐ'),
        b'p' => V('ㅔ'),
        b'a' => C('ㅁ'),
        b's' => C('ㄴ'),
        b'd' => C('ㅇ'),
        b'f' => C('ㄹ'),
        b'g' => C('ㅎ'),
        b'h' => V('ㅗ'),
        b'j' => V('ㅓ'),
        b'k' => V('ㅏ'),
        b'l' => V('ㅣ'),
        b'z' => C('ㅋ'),
        b'x' => C('ㅌ'),
        b'c' => C('ㅊ'),
        b'v' => C('ㅍ'),
        b'b' => V('ㅠ'),
        b'n' => V('ㅜ'),
        b'm' => V('ㅡ'),
        _ => return None,
    };
    Some(jamo)
}

fn combine(table: &[(char, char, char)], a: char, b: char) -> Option<char> {
    table.iter().find(|&&(x, y, _)| x == a && y == b).map(|&(_, _, c)| c)
}

fn split_final(c: char) -> Option<(char, char)> {
    COMPOUND_FINALS.iter().find(|&&(_, _, f)| f == c).map(|&(a, b, _)| (a, b))
}

#[derive(Default)]
struct Syllable {
    initial: Option<char>,
    vowel: Option<char>,
    last: Option<char>,
}

impl Syllable {
    fn flush(&mut self, out: &mut String) {
        match (self.initial, self.vowel) {
            (Some(i), Some(v)) => {
                let i = CHOSEONG.iter().position(|&c| c == i).unwrap() as u32;
                let v = v as u32 - 'ㅏ' as u32;
                let f = self
                    .last
                    .map_or(0, |f| JONGSEONG.iter().position(|&c| c == f).unwrap() as u32);
                out.push(char::from_u32(0xAC00 + (i * 21 + v) * 28 + f).unwrap());
            }
            (Some(i), None) => out.push(i),
            (None, Some(v)) => out.push(v),
            (None, None) => {}
        }
        *self = Syllable::default();
    }
}

/// Composes keystrokes into Hangul. Jamo that cannot join a syllable are
/// emitted as lone compatibility jamo (U+3131..U+3163).
pub fn compose(strokes: &[Stroke]) -> String {
    let mut out = String::new();
    let mut cur = Syllable::default();
    for &stroke in strokes {
        match jamo_for(stroke) {
            None => {
                cur.flush(&mut out);
                out.push(stroke.ch as char);
            }
            Some(Jamo::Consonant(c)) => {
                if cur.initial.is_some() && cur.vowel.is_some() {
                    match cur.last {
                        None if JONGSEONG.contains(&c) => cur.last = Some(c),
                        Some(f) if combine(COMPOUND_FINALS, f, c).is_some() => {
                            cur.last = combine(COMPOUND_FINALS, f, c);
                        }
                        _ => {
                            cur.flush(&mut out);
                            cur.initial = Some(c);
                        }
                    }
                } else {
                    cur.flush(&mut out);
                    cur.initial = Some(c);
                }
            }
            Some(Jamo::Vowel(v)) => {
                if let Some(f) = cur.last {
                    // The final consonant (or the second half of a compound
                    // final) becomes the initial of the next syllable.
                    let (keep, moved) = match split_final(f) {
                        Some((a, b)) => (Some(a), b),
                        None => (None, f),
                    };
                    cur.last = keep;
                    cur.flush(&mut out);
                    cur.initial = Some(moved);
                    cur.vowel = Some(v);
                } else if let Some(prev) = cur.vowel {
                    match combine(COMPOUND_VOWELS, prev, v) {
                        Some(cv) => cur.vowel = Some(cv),
                        None => {
                            cur.flush(&mut out);
                            cur.vowel = Some(v);
                        }
                    }
                } else {
                    cur.vowel = Some(v);
                }
            }
        }
    }
    cur.flush(&mut out);
    out
}

/// Stand-in Korean word list for the baseline scorer; the real language
/// model is undecided (D7).
pub const SAMPLE_WORDS: &[&str] = &[
    "안녕", "안녕하세요", "정도", "사람", "한국", "한국어", "일본", "일본어", "영어", "사랑", "감사",
    "감사합니다", "오늘", "내일", "어제", "시간", "생각", "문제", "사용", "입력", "입력기", "우리",
    "나라", "학교", "친구", "회사", "컴퓨터", "프로그램", "자판", "한글", "가나", "간", "글자", "말",
    "물", "밥", "집", "네", "아니요", "그리고", "하지만", "그래서", "지금", "여기", "저기", "무엇",
    "누구", "어디", "언제", "왜", "어떻게", "있다", "없다", "하다", "되다", "같다", "많이", "조금",
    "정말", "진짜", "좋아", "좋다", "세상", "공부", "대학교", "선생님", "학생", "가족", "음식",
];

pub fn is_lone_jamo(c: char) -> bool {
    ('\u{3131}'..='\u{3163}').contains(&c)
}

pub struct HangulInterpreter;

impl Interpreter for HangulInterpreter {
    fn interpret(&self, strokes: &[Stroke]) -> Vec<Candidate> {
        vec![Candidate { text: compose(strokes), lang: Lang::Ko }]
    }
}

#[cfg(test)]
pub(crate) fn strokes(s: &str) -> Vec<Stroke> {
    s.bytes()
        .map(|b| Stroke { ch: b.to_ascii_lowercase(), shift: b.is_ascii_uppercase() })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Catches broken basic syllable assembly and final-consonant handling.
    #[test]
    fn composes_basic_syllables() {
        assert_eq!(compose(&strokes("dkssud")), "안녕");
        assert_eq!(compose(&strokes("rks")), "간");
    }

    // Catches the final consonant not moving to the next syllable before a vowel.
    #[test]
    fn final_moves_to_next_syllable() {
        assert_eq!(compose(&strokes("rksk")), "가나");
        // Compound final splits: 닭 + ㅏ -> 달가
        assert_eq!(compose(&strokes("ekfrk")), "달가");
    }

    // Catches missing compound vowels / finals.
    #[test]
    fn compound_jamo() {
        assert_eq!(compose(&strokes("dhk")), "와");
        assert_eq!(compose(&strokes("dml")), "의");
        assert_eq!(compose(&strokes("ekfr")), "닭");
        assert_eq!(compose(&strokes("djqt")), "없");
    }

    // Catches wrong Shift mapping: QWERT/OP get 쌍자음/ㅒㅖ, others stay unshifted.
    #[test]
    fn shift_jamo() {
        assert_eq!(compose(&strokes("Qk")), "빠");
        assert_eq!(compose(&strokes("Tk")), "싸");
        assert_eq!(compose(&strokes("dP")), "예");
        assert_eq!(compose(&strokes("gK")), "하");
        // ㄸ cannot be a final, so it starts a new syllable.
        assert_eq!(compose(&strokes("rkE")), "가ㄸ");
    }

    // Catches lone jamo being dropped or merged incorrectly.
    #[test]
    fn lone_jamo() {
        assert_eq!(compose(&strokes("r")), "ㄱ");
        assert_eq!(compose(&strokes("k")), "ㅏ");
        assert_eq!(compose(&strokes("rr")), "ㄱㄱ");
    }

    // Catches D13 regressions: digits/punctuation must pass through and end the syllable.
    #[test]
    fn non_letters_pass_through() {
        assert_eq!(compose(&strokes("0.1wjdeh")), "0.1정도");
        assert_eq!(compose(&strokes("rk1k")), "가1ㅏ");
    }
}
