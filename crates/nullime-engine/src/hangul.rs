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

/// True for a precomposed syllable outside the 2350 of KS X 1001, the set
/// everyday Korean text stays within. 두벌식 readings of English words land
/// outside it (`authorize` -> 며쇄걐ㄷ).
pub fn is_rare_syllable(c: char) -> bool {
    let Some(i) = (c as u32).checked_sub(0xAC00).filter(|&i| i < 11172) else {
        return false;
    };
    KS_X_1001[i as usize / 64] & (1 << (i % 64)) == 0
}

/// Bit `i` set when U+AC00+i is in KS X 1001. Generated with Python's
/// `euc_kr` codec: a syllable is in the set when it encodes to 2 bytes.
const KS_X_1001: [u64; 175] = [
    0x1303b0113eff0793, 0x0593000011102801, 0x3b019703b0111e7b, 0x306b959300a01112,
    0x113032011102b051, 0xb879300a011102b0, 0x0080001030011306, 0x93000011100b0113,
    0x0593000000102b03, 0x3b011323b051746b, 0x7000000000001030, 0x111029001303b011,
    0xb015300000012180, 0x020000303001030e, 0x1300000010230111, 0x0113030010106b81,
    0x0000010030111013, 0x3000000022b85530, 0x113afb079702b011, 0x00000021011303b0,
    0x03b011383b0d1b00, 0x1300000111330113, 0x00000100111c2b05, 0x2a011300b0111000,
    0x1010000102b01930, 0x1030030111000000, 0x0011146b07130230, 0x8fb8f9742b051300,
    0x00000000103b0113, 0x01134ab0d9700000, 0x000011030011103b, 0x100001112ab15930,
    0x00100b0111010000, 0x0000102b01130000, 0x02a0111020000101, 0x0102b05930210111,
    0x011307b019300000, 0x00000003b011383b, 0x383b0d1300000000, 0x000010000103b011,
    0x0010102001130000, 0x0000011000000100, 0x0002181130000000, 0x0111000000100000,
    0x0b01930000000023, 0x302b011100301110, 0x01303b0113c7b011, 0xb011300000000280,
    0x03b011302b011383, 0x1102b011300a0011, 0x0111010000002000, 0x2b011302a011102b,
    0x3000000101000010, 0x11302b0113029011, 0xb0113000000066b0, 0x07b0113a6b07d302,
    0x1300000000200103, 0x011303b011386b05, 0x2b051b00000010b8, 0x1000000003000110,
    0x79700a011102a011, 0x0000100a0111a2b0, 0x0090111000011100, 0x9300000000090111,
    0x011322b0f9f2bb05, 0x000000002001323b, 0x303b019306b05930, 0x117000001123a011,
    0x00001010001102b0, 0x0000011003011301, 0x01010010162b0793, 0x0111020011300000,
    0x00000000b0113029, 0x383b05130eb05130, 0x000001000303b011, 0x0000103901930000,
    0x000000003b000302, 0x0000000000230113, 0x0001000000100000, 0x0000000290113020,
    0x1000000000000000, 0x0000030111020000, 0xb079b02b01130000, 0x02b011303b011323,
    0x1343b0d9f0210111, 0x011103b011303b01, 0x20011322b0517020, 0x300b011101901110,
    0x0016ab019302b011, 0xb011302101130100, 0x02b0313029010302, 0x1b42b81930000000,
    0x0000033011383301, 0x3305130000000020, 0x0000000000001110, 0x0130230593000001,
    0x3011101000010100, 0x0230113000000100, 0x1100000010100001, 0x8513020000000000,
    0x2b01130010111003, 0x303b011363b87730, 0x7b30020111a2b091, 0xf0d1702b011357f0,
    0x0ab971301b0111e3, 0x13029001303b0113, 0x071302b011302b01, 0x230113033011302b,
    0x30ab011302b01130, 0x7130090111feb411, 0xb011307b05d347b8, 0x0000111021015303,
    0x1102b011306b0513, 0x0513000000103301, 0x30000102a01038eb, 0x3020001302b01110,
    0x001010000102b071, 0x1011100b01130000, 0x000000002b011300, 0x1303b095366b0593,
    0x0000020001103b01, 0x20000103b0113000, 0x3000000001000010, 0x00101001030ab011,
    0x0000000301110100, 0x0300001023011302, 0x0100000010000000, 0x0000029000100000,
    0x7b01538630113000, 0x0021015103b01130, 0x11303b0113000000, 0x00011010001102b0,
    0x020011102b011302, 0x0102b01110000000, 0x000102b011300100, 0x2b01110000011010,
    0x002b011302101110, 0x11302b0393000000, 0x0000303b011302b0, 0x03b0193000000002,
    0x0103b011102b0113, 0x011302b011300000, 0x0001010200001021, 0x102b011300000010,
    0x1130200001020011, 0x30113001011102b0, 0x02b0113000000002, 0x0103b011303b0313,
    0x0513000000002000, 0x10001102b011303b, 0x142b011300000110, 0x0110000001000001,
    0xb011300000010280, 0x0000001010000102, 0x9302101110230113, 0x0113003011100b05,
    0x3b011323b051702b, 0x3000000000000030, 0x11102b011303b011, 0xb011300a01010330,
    0x0000000020000102, 0x9300a01110000011, 0x0000020000102b05, 0x2901110090111000,
    0x3000000000b01110, 0x11302b211302b011, 0x00000020000103b0, 0x02b011302b051300,
    0x13002011103b0113, 0x0013028011322b21, 0x0a011102a0113028, 0x3021011102921130,
    0x11302b0113020011, 0x3011122b03d30290, 0x000000002b011302,
];

pub struct HangulInterpreter;

impl Interpreter for HangulInterpreter {
    fn interpret(&self, strokes: &[Stroke]) -> Vec<Candidate> {
        vec![Candidate { text: compose(strokes), lang: Lang::Ko, cost: None }]
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
