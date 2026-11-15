//! Embedded language models that make Korean and English spans comparable:
//! a Hangul syllable unigram, a Korean particle unigram and an English letter
//! trigram, all natural-log probabilities. Tables come from `tools/nullime-dict/gen_lm.py`; sources and
//! licenses are in docs/architecture.md "언어 모델" and NOTICE.

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
