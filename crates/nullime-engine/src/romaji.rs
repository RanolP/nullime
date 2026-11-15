//! Romaji (Hepburn + Kunrei common table) to kana, plus the Japanese
//! interpreter that adds kanji from a `Dictionary`.

use crate::kanji::Dictionary;
use crate::{Candidate, Interpreter, Lang, Stroke};

const TABLE: &[(&str, &str)] = &[
    ("a", "あ"), ("i", "い"), ("u", "う"), ("e", "え"), ("o", "お"),
    ("ka", "か"), ("ki", "き"), ("ku", "く"), ("ke", "け"), ("ko", "こ"),
    ("kya", "きゃ"), ("kyu", "きゅ"), ("kyo", "きょ"),
    ("sa", "さ"), ("si", "し"), ("shi", "し"), ("su", "す"), ("se", "せ"), ("so", "そ"),
    ("sha", "しゃ"), ("shu", "しゅ"), ("sho", "しょ"), ("sya", "しゃ"), ("syu", "しゅ"), ("syo", "しょ"),
    ("ta", "た"), ("ti", "ち"), ("chi", "ち"), ("tu", "つ"), ("tsu", "つ"), ("te", "て"), ("to", "と"),
    ("cha", "ちゃ"), ("chu", "ちゅ"), ("cho", "ちょ"), ("tya", "ちゃ"), ("tyu", "ちゅ"), ("tyo", "ちょ"),
    ("na", "な"), ("ni", "に"), ("nu", "ぬ"), ("ne", "ね"), ("no", "の"),
    ("nya", "にゃ"), ("nyu", "にゅ"), ("nyo", "にょ"),
    ("ha", "は"), ("hi", "ひ"), ("hu", "ふ"), ("fu", "ふ"), ("he", "へ"), ("ho", "ほ"),
    ("hya", "ひゃ"), ("hyu", "ひゅ"), ("hyo", "ひょ"),
    ("fa", "ふぁ"), ("fi", "ふぃ"), ("fe", "ふぇ"), ("fo", "ふぉ"),
    ("ma", "ま"), ("mi", "み"), ("mu", "む"), ("me", "め"), ("mo", "も"),
    ("mya", "みゃ"), ("myu", "みゅ"), ("myo", "みょ"),
    ("ya", "や"), ("yu", "ゆ"), ("yo", "よ"),
    ("ra", "ら"), ("ri", "り"), ("ru", "る"), ("re", "れ"), ("ro", "ろ"),
    ("rya", "りゃ"), ("ryu", "りゅ"), ("ryo", "りょ"),
    ("wa", "わ"), ("wo", "を"),
    ("ga", "が"), ("gi", "ぎ"), ("gu", "ぐ"), ("ge", "げ"), ("go", "ご"),
    ("gya", "ぎゃ"), ("gyu", "ぎゅ"), ("gyo", "ぎょ"),
    ("za", "ざ"), ("zi", "じ"), ("ji", "じ"), ("zu", "ず"), ("ze", "ぜ"), ("zo", "ぞ"),
    ("ja", "じゃ"), ("ju", "じゅ"), ("jo", "じょ"), ("zya", "じゃ"), ("zyu", "じゅ"), ("zyo", "じょ"),
    ("jya", "じゃ"), ("jyu", "じゅ"), ("jyo", "じょ"),
    ("da", "だ"), ("di", "ぢ"), ("du", "づ"), ("de", "で"), ("do", "ど"),
    ("ba", "ば"), ("bi", "び"), ("bu", "ぶ"), ("be", "べ"), ("bo", "ぼ"),
    ("bya", "びゃ"), ("byu", "びゅ"), ("byo", "びょ"),
    ("pa", "ぱ"), ("pi", "ぴ"), ("pu", "ぷ"), ("pe", "ぺ"), ("po", "ぽ"),
    ("pya", "ぴゃ"), ("pyu", "ぴゅ"), ("pyo", "ぴょ"),
    ("xa", "ぁ"), ("xi", "ぃ"), ("xu", "ぅ"), ("xe", "ぇ"), ("xo", "ぉ"),
    ("xya", "ゃ"), ("xyu", "ゅ"), ("xyo", "ょ"), ("xtu", "っ"), ("xtsu", "っ"),
    ("nn", "ん"),
];

/// Upper bound on distinct parses kept per span; ambiguity only comes from `n`.
const MAX_PARSES: usize = 8;

fn is_vowel(b: u8) -> bool {
    matches!(b, b'a' | b'i' | b'u' | b'e' | b'o')
}

/// Every way to read `s[i..]` as one token: (consumed bytes, kana).
fn tokens_at(s: &[u8], i: usize) -> Vec<(usize, &'static str)> {
    let rest = &s[i..];
    let mut out: Vec<(usize, &'static str)> = TABLE
        .iter()
        .filter(|(r, _)| rest.starts_with(r.as_bytes()))
        .map(|&(r, k)| (r.len(), k))
        .collect();
    let c = rest[0];
    if !c.is_ascii_lowercase() {
        out.push((1, literal(c)));
        return out;
    }
    let next = rest.get(1).copied();
    // Single n is ん before a non-vowel non-y, or at the end of the span.
    if c == b'n' && next.is_none_or(|n| !is_vowel(n) && n != b'y') {
        out.push((1, "ん"));
    }
    // Doubled consonant (and t before ch) is っ.
    if c != b'n' && !is_vowel(c) && (next == Some(c) || (c == b't' && rest.starts_with(b"tch"))) {
        out.push((1, "っ"));
    }
    out
}

fn literal(c: u8) -> &'static str {
    const ASCII: &str = "!\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~";
    let i = (c - b'!') as usize;
    &ASCII[i..i + 1]
}

/// All full readings of `s` as hiragana, best first. Empty when `s` cannot
/// be consumed completely.
pub fn to_hiragana(s: &str) -> Vec<String> {
    let s = s.as_bytes();
    let n = s.len();
    if n == 0 {
        return Vec::new();
    }
    // completes[i]: s[i..] has at least one full parse. Prunes the search so
    // long runs of `n` cannot blow up.
    let mut completes = vec![false; n + 1];
    completes[n] = true;
    for i in (0..n).rev() {
        completes[i] = tokens_at(s, i).iter().any(|&(len, _)| completes[i + len]);
    }
    if !completes[0] {
        return Vec::new();
    }
    let mut parses = Vec::new();
    collect(s, 0, &completes, &mut Vec::new(), &mut parses);
    // Fewer tokens first (ほん over ほんん), then fewer ん+vowel (こんにちは over
    // こんいちは, since nn+vowel is usually a typed n before na/ni/...).
    parses.sort_by_key(|tokens: &Vec<&str>| {
        let text: String = tokens.concat();
        (tokens.len(), awkward_n(&text))
    });
    let mut out: Vec<String> = Vec::new();
    for tokens in parses {
        let text = tokens.concat();
        if !out.contains(&text) {
            out.push(text);
        }
    }
    out
}

fn collect(
    s: &[u8],
    i: usize,
    completes: &[bool],
    path: &mut Vec<&'static str>,
    out: &mut Vec<Vec<&'static str>>,
) {
    if out.len() >= MAX_PARSES {
        return;
    }
    if i == s.len() {
        out.push(path.clone());
        return;
    }
    for (len, kana) in tokens_at(s, i) {
        if completes[i + len] {
            path.push(kana);
            collect(s, i + len, completes, path, out);
            path.pop();
        }
    }
}

fn awkward_n(text: &str) -> usize {
    let chars: Vec<char> = text.chars().collect();
    chars
        .windows(2)
        .filter(|w| w[0] == 'ん' && "あいうえおやゆよ".contains(w[1]))
        .count()
}

pub fn to_katakana(hiragana: &str) -> String {
    hiragana
        .chars()
        .map(|c| match c {
            '\u{3041}'..='\u{3096}' => char::from_u32(c as u32 + 0x60).unwrap(),
            _ => c,
        })
        .collect()
}

pub struct JapaneseInterpreter<D> {
    pub dictionary: D,
}

impl<D: Dictionary> Interpreter for JapaneseInterpreter<D> {
    /// Hiragana readings first, then dictionary kanji for each reading, then
    /// katakana.
    fn interpret(&self, strokes: &[Stroke]) -> Vec<Candidate> {
        let romaji: String = strokes.iter().map(|s| s.ch as char).collect();
        let readings = to_hiragana(&romaji);
        let ja = |text: String| Candidate { text, lang: Lang::Ja };
        let mut out: Vec<Candidate> = readings.iter().cloned().map(ja).collect();
        for reading in &readings {
            out.extend(self.dictionary.lookup(reading).into_iter().map(|k| ja(k.to_string())));
        }
        out.extend(readings.iter().map(|r| ja(to_katakana(r))));
        out.dedup();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Catches the basic table or full-consumption rule breaking.
    #[test]
    fn basic_readings() {
        assert_eq!(to_hiragana("sake")[0], "さけ");
        assert_eq!(to_hiragana("shitsu")[0], "しつ");
        assert_eq!(to_hiragana("situ")[0], "しつ");
        assert!(to_hiragana("hello").is_empty());
        assert!(to_hiragana("k").is_empty());
    }

    // Catches doubled-consonant っ handling.
    #[test]
    fn small_tsu() {
        assert_eq!(to_hiragana("kitte")[0], "きって");
        assert_eq!(to_hiragana("matcha")[0], "まっちゃ");
    }

    // Catches ん rules: nn, n before consonant, trailing n, and the nn+vowel preference.
    #[test]
    fn syllabic_n() {
        assert_eq!(to_hiragana("konnichiha")[0], "こんにちは");
        assert_eq!(to_hiragana("hon")[0], "ほん");
        assert_eq!(to_hiragana("honn")[0], "ほん");
        assert_eq!(to_hiragana("kanji")[0], "かんじ");
        assert_eq!(to_hiragana("konna")[0], "こんな");
        assert_eq!(to_hiragana("kyou")[0], "きょう");
    }

    // Catches katakana offset errors.
    #[test]
    fn katakana() {
        assert_eq!(to_katakana("きって"), "キッテ");
    }

    // Catches D13 regressions: digits/punctuation pass through romaji literally.
    #[test]
    fn literals_pass_through() {
        assert_eq!(to_hiragana("ka1")[0], "か1");
    }
}
