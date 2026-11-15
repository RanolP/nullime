//! Candidate scoring. Pluggable (D7): the decoder only sees `Scorer`.

use std::collections::HashSet;

use crate::{english, hangul, lm, romaji, Candidate, Lang, Stroke};

/// Higher is better; scores of the segments on a path are summed.
pub trait Scorer {
    /// `rank` is the candidate's position in its interpreter's own output.
    fn score(&self, strokes: &[Stroke], candidate: &Candidate, rank: usize) -> f32;
    /// Subtracted once per segment, so fewer segments win ties.
    fn segment_penalty(&self) -> f32;
    /// Subtracted when a span boundary falls between two letters, i.e. inside
    /// what was typed as one word, and the path reads `prev` then `next`.
    /// May be negative: it can re-score `next` under a model for what follows
    /// a word (a particle) in place of its `score`. `strokes` is `next`'s span.
    fn mid_word_switch_penalty(&self, prev: Lang, strokes: &[Stroke], next: &Candidate) -> f32;
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
// Japanese: the kana bigram's ln P of the best reading, like the other two
// languages, plus dictionary evidence: a path cheaper than JA_COST_PER_LETTER
// per typed letter earns (that much - cost) / JA_COST_SCALE, a dearer path
// earns nothing: one with an unknown-word node, but also a short word in the
// sample dictionary (の is 4300 for two letters). JA_PRIOR is what a Japanese
// span pays to exist, and is what keeps a plausible non-word (`tokyo` ->
// ときょ) English. Tuned on the 194-word labelled list against both
// dictionaries (docs/architecture.md "점수").
const JA_PRIOR: f32 = -6.0;
const JA_COST_PER_LETTER: f32 = 1800.0;
const JA_COST_SCALE: f32 = 500.0;
// After English inside one run of letters a Japanese span is read as a
// particle, like Korean: this share is any Japanese, the rest is uniform over
// JA_PARTICLES. A particle's score is then that model's alone, without
// JA_PRIOR or dictionary evidence, so it does not hang on what the loaded
// dictionary charges for の (sample 4300, Mozc 1601). The switch pays
// MID_WORD_PARTICLE on top: below 4.9 `node` -> no|で, above 5.4
// `GitHubni` stays English.
const JA_NON_PARTICLE: f32 = 0.001;
const MID_WORD_PARTICLE: f32 = 5.0;
const JA_PARTICLES: &[&str] = &["は", "が", "を", "に", "へ", "と", "で", "も", "の", "から", "まで", "より", "や", "か"];
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

    fn japanese(&self, strokes: &[Stroke], cost: i32, letters: f32) -> f32 {
        let romaji: String = strokes.iter().map(|s| s.ch as char).collect();
        let reading = romaji::to_hiragana(&romaji).iter().map(|r| lm::ja_reading(r)).fold(f32::MIN, f32::max);
        reading + RANDOM_KEY * letters + JA_PRIOR + (JA_COST_PER_LETTER * letters - cost as f32).max(0.0) / JA_COST_SCALE
    }

    /// `korean_after_word` for Japanese: the span's `score` minus its score
    /// as a particle. Particles are hiragana, so the candidate's text is its
    /// reading.
    fn japanese_after_word(&self, strokes: &[Stroke], next: &Candidate) -> f32 {
        if !JA_PARTICLES.contains(&next.text.as_str()) {
            return MID_WORD_PARTICLE - JA_NON_PARTICLE.ln();
        }
        let general = lm::ja_reading(&next.text);
        let particle = (1.0 - JA_NON_PARTICLE) / JA_PARTICLES.len() as f32;
        let after = (particle + JA_NON_PARTICLE * general.exp()).ln() + RANDOM_KEY * letters(strokes);
        self.score(strokes, next, 0) - after + MID_WORD_PARTICLE
    }
}

fn letters(strokes: &[Stroke]) -> f32 {
    strokes.iter().filter(|s| s.ch.is_ascii_lowercase()).count() as f32
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
        let letters = letters(strokes);
        match candidate.lang {
            Lang::En => self.english(&candidate.text),
            Lang::Ko => self.korean(&candidate.text, letters),
            Lang::Ja => match candidate.cost {
                Some(cost) => self.japanese(strokes, cost, letters),
                None => PER_LETTER * letters - JA_RANK_STEP * rank as f32,
            },
        }
    }

    fn segment_penalty(&self) -> f32 {
        SEGMENT_PENALTY
    }

    fn mid_word_switch_penalty(&self, prev: Lang, strokes: &[Stroke], next: &Candidate) -> f32 {
        match (prev, next.lang) {
            (Lang::En, Lang::Ko) => self.korean_after_word(&next.text),
            (Lang::En, Lang::Ja) => self.japanese_after_word(strokes, next),
            (Lang::Ko, Lang::Ko) | (Lang::Ja, Lang::Ja) => 0.0,
            _ => MID_WORD_SWITCH,
        }
    }
}
