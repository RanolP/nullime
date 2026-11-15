//! Candidate scoring. Pluggable (D7): the decoder only sees `Scorer`.

use std::collections::HashSet;

use crate::{english, hangul, Candidate, Lang, Stroke};

/// Higher is better; scores of the segments on a path are summed.
pub trait Scorer {
    /// `rank` is the candidate's position in its interpreter's own output.
    fn score(&self, strokes: &[Stroke], candidate: &Candidate, rank: usize) -> f32;
    /// Subtracted once per segment, so fewer segments win ties.
    fn segment_penalty(&self) -> f32;
}

/// STAND-IN heuristic until a real probability model is chosen (D7):
/// word-list hits, Hangul well-formedness, and a per-segment penalty.
/// Romaji that does not fully consume its span never reaches the scorer,
/// because the Japanese interpreter emits nothing for it.
pub struct BaselineScorer {
    en_words: HashSet<&'static str>,
    ko_words: HashSet<&'static str>,
}

const PER_LETTER: f32 = 0.3;
const LONE_JAMO: f32 = -1.5;
const EN_WORD_PER_LETTER: f32 = 0.5;
const EN_NON_WORD_PER_LETTER: f32 = -1.0;
const WORD_BONUS: f32 = 1.0;
const JA_RANK_STEP: f32 = 0.05;
// Must outweigh short high-scoring pieces (the one-letter word "i"), or
// `konnichiha` splits into こん|i|초|i|は.
const SEGMENT_PENALTY: f32 = 1.5;

impl Default for BaselineScorer {
    fn default() -> Self {
        BaselineScorer {
            en_words: english::SAMPLE_WORDS.iter().copied().collect(),
            ko_words: hangul::SAMPLE_WORDS.iter().copied().collect(),
        }
    }
}

impl BaselineScorer {
    fn english(&self, text: &str) -> f32 {
        // Digits and punctuation are literal in every language, so they
        // score 0 and only letter runs decide.
        text.split(|c: char| !c.is_ascii_alphabetic())
            .filter(|w| !w.is_empty())
            .map(|w| {
                let len = w.len() as f32;
                if self.en_words.contains(w.to_ascii_lowercase().as_str()) {
                    EN_WORD_PER_LETTER * len + WORD_BONUS
                } else {
                    EN_NON_WORD_PER_LETTER * len
                }
            })
            .sum()
    }

    fn korean(&self, text: &str, letters: f32) -> f32 {
        let lone = text.chars().filter(|&c| hangul::is_lone_jamo(c)).count() as f32;
        let words = text
            .split(|c: char| !('가'..='힣').contains(&c))
            .filter(|w| self.ko_words.contains(w))
            .count() as f32;
        PER_LETTER * letters + LONE_JAMO * lone + WORD_BONUS * words
    }
}

impl Scorer for BaselineScorer {
    fn score(&self, strokes: &[Stroke], candidate: &Candidate, rank: usize) -> f32 {
        let letters = strokes.iter().filter(|s| s.ch.is_ascii_lowercase()).count() as f32;
        match candidate.lang {
            Lang::En => self.english(&candidate.text),
            Lang::Ko => self.korean(&candidate.text, letters),
            Lang::Ja => PER_LETTER * letters - JA_RANK_STEP * rank as f32,
        }
    }

    fn segment_penalty(&self) -> f32 {
        SEGMENT_PENALTY
    }
}
