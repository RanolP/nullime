//! English: the keystrokes as typed.

use crate::{Candidate, Interpreter, Lang, Stroke};

pub struct EnglishInterpreter;

impl Interpreter for EnglishInterpreter {
    fn interpret(&self, strokes: &[Stroke]) -> Vec<Candidate> {
        let text = strokes
            .iter()
            .map(|s| if s.shift { s.ch.to_ascii_uppercase() } else { s.ch } as char)
            .collect();
        vec![Candidate { text, lang: Lang::En, cost: None }]
    }
}

/// STAND-IN word list for the baseline scorer (lowercase); the real language
/// model is undecided (D7). Includes common unit abbreviations so `0.1mm`
/// reads as English.
pub const SAMPLE_WORDS: &[&str] = &[
    "a", "about", "after", "again", "all", "also", "an", "and", "any", "are", "as", "at", "back",
    "be", "because", "been", "before", "being", "best", "but", "by", "call", "can", "code", "come",
    "could", "data", "day", "did", "do", "does", "down", "each", "even", "file", "find", "first",
    "for", "from", "get", "give", "go", "good", "great", "had", "has", "have", "he", "hello", "help",
    "her", "here", "hi", "him", "his", "how", "i", "if", "in", "input", "into", "is", "it", "its",
    "just", "key", "know", "language", "like", "line", "little", "long", "look", "make", "man",
    "many", "may", "me", "more", "most", "my", "name", "new", "no", "not", "now", "of", "off", "ok",
    "old", "on", "one", "only", "or", "other", "our", "out", "over", "people", "please", "rust",
    "sake", "said", "same", "say", "see", "she", "should", "so", "some", "start", "such", "take",
    "test", "text", "than", "thank", "thanks", "that", "the", "their", "them", "then", "there",
    "these", "they", "thing", "think", "this", "time", "to", "two", "type", "up", "us", "use",
    "very", "want", "was", "way", "we", "well", "were", "what", "when", "where", "which", "who",
    "why", "will", "with", "word", "work", "world", "would", "write", "year", "yes", "you", "your",
    "mm", "cm", "km", "kg", "mg", "ml", "px", "pt", "em", "ms", "kb", "mb", "gb",
];
