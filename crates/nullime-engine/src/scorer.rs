//! Candidate scoring. Pluggable (D7): the decoder only sees `Scorer`.

use std::collections::HashSet;

use crate::{english, hangul, lm, Candidate, Lang, Stroke};

/// Higher is better; scores of the segments on a path are summed.
pub trait Scorer {
    /// `rank` is the candidate's position in its interpreter's own output.
    fn score(&self, strokes: &[Stroke], candidate: &Candidate, rank: usize) -> f32;
    /// Subtracted once per segment, so fewer segments win ties.
    fn segment_penalty(&self) -> f32;
    /// Subtracted when a span boundary falls between two letters, i.e. inside
    /// what was typed as one word, and the path reads `prev` then `next`.
    /// May be negative: it can re-score `next` under a model for what follows
    /// a word (a particle) in place of its `score`.
    fn mid_word_switch_penalty(&self, prev: Lang, next: &Candidate) -> f32;
}

/// Korean and English spans score as how much likelier their language model
/// makes the text than random keys would (docs/architecture.md "점수").
/// Romaji that does not fully consume its span never reaches the scorer,
/// because the Japanese interpreter emits nothing for it.
pub struct BaselineScorer {
    en_words: HashSet<&'static str>,
}

/// ln 26: what a letter costs if keys were random. Adding it per letter turns
/// a language model's ln P into a log-likelihood ratio, so a Korean and an
/// English reading of the same keys compare directly.
const RANDOM_KEY: f32 = 3.2580965;
/// ln P of a syllable outside KS X 1001's 2350, below every syllable in the
/// table (the rarest is about -14.7).
const KO_RARE_SYLLABLE: f32 = -15.0;
/// ln P of a lone jamo, a key that joined no syllable.
const KO_LONE_JAMO: f32 = -12.0;
const EN_WORD_BONUS: f32 = 3.0;
const PER_LETTER: f32 = 0.3;
const JA_RANK_STEP: f32 = 0.05;
// Japanese with a conversion cost: a per-letter gain minus the path cost.
// Linear in cost, so a sentence split in two pays the extra BOS/EOS
// connection costs and loses to one segment. Calibrated on the Mozc asset
// (docs/architecture.md "점수").
const JA_PER_LETTER: f32 = 1.0;
const JA_COST_SCALE: f32 = 2500.0;
// Must outweigh short high-scoring pieces (the one-letter word "i"), or
// `konnichiha` splits into こん|i|초|i|は.
const SEGMENT_PENALTY: f32 = 1.5;
// Charged when a span boundary falls inside one run of letters. The only
// common real switch there is English followed by a Korean or Japanese
// particle (`Rustfmf` -> Rust를); Hangul and kana also split freely because
// the split changes how syllables compose. Any other boundary carves a typed
// word into pieces that each score well alone: `Signing` -> 냐후|in|g,
// `sushi` -> s|us|hi, `Testing` -> Test|in|g.
const MID_WORD_SWITCH: f32 = 5.0;
// English followed by a Japanese particle still pays something, or an
// English-looking prefix peels off a Japanese word.
const MID_WORD_PARTICLE: f32 = 3.0;
// Korean after English inside one run of letters is read as a particle
// (Kotlin|를), so it is scored as a mixture: this share of such spans is any
// Korean text, the rest is a 조사 (lm::ko_particle). Without it a common
// syllable peels off a weak English word (`http` -> ht|세) or an
// English-looking prefix off a Korean word (`Ehaqkdrkrgk` -> Eha|방각하).
const KO_NON_PARTICLE: f32 = 0.001;

impl Default for BaselineScorer {
    fn default() -> Self {
        BaselineScorer { en_words: english::SAMPLE_WORDS.iter().copied().collect() }
    }
}

impl BaselineScorer {
    fn english(&self, text: &str) -> f32 {
        // Digits and punctuation are literal in every language, so they
        // score 0 and only letter runs decide.
        text.split(|c: char| !c.is_ascii_alphabetic())
            .filter(|w| !w.is_empty())
            .map(|w| {
                let bonus = self.en_words.contains(w.to_ascii_lowercase().as_str());
                lm::en_word(w) + RANDOM_KEY * w.len() as f32 + if bonus { EN_WORD_BONUS } else { 0.0 }
            })
            .sum()
    }

    fn korean(&self, text: &str, letters: f32) -> f32 {
        korean_logp(text) + RANDOM_KEY * letters
    }

    /// How much less likely `text` is right after English letters than
    /// `korean` scored it: ln P_general - ln P_after_word.
    fn korean_after_word(&self, text: &str) -> f32 {
        let general = korean_logp(text);
        let after = match lm::ko_particle(text.trim_end_matches(|c: char| c.is_ascii())) {
            Some(p) => ((1.0 - KO_NON_PARTICLE) * p.exp() + KO_NON_PARTICLE * general.exp()).ln(),
            None => KO_NON_PARTICLE.ln() + general,
        };
        general - after
    }
}

fn korean_logp(text: &str) -> f32 {
    text.chars()
        .map(|c| match c {
            '가'..='힣' => lm::ko_syllable(c).unwrap_or(KO_RARE_SYLLABLE),
            _ if hangul::is_lone_jamo(c) => KO_LONE_JAMO,
            _ => 0.0,
        })
        .sum()
}

impl Scorer for BaselineScorer {
    fn score(&self, strokes: &[Stroke], candidate: &Candidate, rank: usize) -> f32 {
        let letters = strokes.iter().filter(|s| s.ch.is_ascii_lowercase()).count() as f32;
        match candidate.lang {
            Lang::En => self.english(&candidate.text),
            Lang::Ko => self.korean(&candidate.text, letters),
            Lang::Ja => match candidate.cost {
                Some(cost) => JA_PER_LETTER * letters - cost as f32 / JA_COST_SCALE,
                None => PER_LETTER * letters - JA_RANK_STEP * rank as f32,
            },
        }
    }

    fn segment_penalty(&self) -> f32 {
        SEGMENT_PENALTY
    }

    fn mid_word_switch_penalty(&self, prev: Lang, next: &Candidate) -> f32 {
        match (prev, next.lang) {
            (Lang::En, Lang::Ko) => self.korean_after_word(&next.text),
            (Lang::En, Lang::Ja) => MID_WORD_PARTICLE,
            (Lang::Ko, Lang::Ko) | (Lang::Ja, Lang::Ja) => 0.0,
            _ => MID_WORD_SWITCH,
        }
    }
}
