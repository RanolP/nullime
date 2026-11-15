//! Embedded language models that make Korean, English and Japanese spans
//! comparable: a Hangul syllable unigram, a Korean particle unigram, an
//! English letter trigram and a kana bigram, all natural-log probabilities.
//! Tables come from `tools/nullime-dict/gen_lm.py`; sources and licenses are
//! in docs/architecture.md "언어 모델" and NOTICE.

use crate::hangul;

/// Table bytes are `round(-ln p * SCALE)`.
const SCALE: f32 = 8.0;

/// One byte per KS X 1001 syllable, in code point order.
static KO_SYLLABLE: &[u8; 2350] = include_bytes!("../data/ko_syllable.bin");
/// `EN_TRIGRAM[(a * 27 + b) * 27 + c]` is P(c | a b) over a-z and the word
/// boundary (26).
static EN_TRIGRAM: &[u8; 19683] = include_bytes!("../data/en_trigram.bin");

/// `particle\tcost` lines: the 조사 unigram over whole particle strings.
static KO_PARTICLE: &str = include_str!("../data/ko_particle.txt");

const BOUNDARY: usize = 26;

/// Kana symbols are ぁ..ん (U+3041..U+3093), then the word boundary.
const KANA_FIRST: u32 = 0x3041;
const KANA_BOUNDARY: usize = 0x3093 - 0x3041 + 1;
const KANA_SYMBOLS: usize = KANA_BOUNDARY + 1;
/// `JA_BIGRAM[a * KANA_SYMBOLS + b]` is P(b | a).
static JA_BIGRAM: &[u8; KANA_SYMBOLS * KANA_SYMBOLS] = include_bytes!("../data/ja_bigram.bin");

fn one_particle(s: &str) -> Option<f32> {
    KO_PARTICLE.lines().find_map(|l| {
        let (p, cost) = l.split_once('\t')?;
        (p == s).then(|| -cost.parse::<f32>().unwrap_or(255.0) / SCALE)
    })
}

/// ln P of `s` as a particle, or two particles in a row (에서+는); `None`
/// when it is neither.
pub fn ko_particle(s: &str) -> Option<f32> {
    let two = s.char_indices().skip(1).filter_map(|(i, _)| Some(one_particle(&s[..i])? + one_particle(&s[i..])?));
    one_particle(s).into_iter().chain(two).reduce(f32::max)
}

/// ln P of a Hangul syllable; `None` outside KS X 1001's 2350.
pub fn ko_syllable(c: char) -> Option<f32> {
    hangul::ks_x_1001_index(c).map(|i| -(KO_SYLLABLE[i] as f32) / SCALE)
}

/// ln P of one word of ASCII letters (case ignored), boundaries included.
pub fn en_word(word: &str) -> f32 {
    let (mut a, mut b) = (BOUNDARY, BOUNDARY);
    let mut cost = 0u32;
    for c in word.bytes().map(|c| (c.to_ascii_lowercase() - b'a') as usize).chain([BOUNDARY]) {
        cost += EN_TRIGRAM[(a * 27 + b) * 27 + c] as u32;
        (a, b) = (b, c);
    }
    -(cost as f32) / SCALE
}

/// ln P of a hiragana reading, boundaries included. Runs of other characters
/// (the digits and punctuation romaji passes through) split it into words
/// and score 0, as in `en_word`'s callers.
pub fn ja_reading(reading: &str) -> f32 {
    let mut cost = 0u32;
    let mut prev = None;
    for c in reading.chars().map(|c| (c as u32).checked_sub(KANA_FIRST).filter(|&i| (i as usize) < KANA_BOUNDARY)).chain([None]) {
        match (prev, c) {
            (None, None) => {}
            (p, c) => cost += JA_BIGRAM[p.map_or(KANA_BOUNDARY, |p| p as usize) * KANA_SYMBOLS + c.map_or(KANA_BOUNDARY, |c| c as usize)] as u32,
        }
        prev = c;
    }
    -(cost as f32) / SCALE
}
