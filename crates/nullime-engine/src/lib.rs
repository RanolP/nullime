//! NullIME engine: turns a stream of QWERTY keystrokes into the most probable
//! mix of English, Korean (두벌식) and Japanese (Romaji + kanji) text.
//! The contract with platform frontends is `docs/architecture.md`.

pub mod decode;
pub mod engine;
pub mod english;
pub mod hangul;
pub mod ja_asset;
pub mod ja_convert;
pub mod kanji;
pub mod romaji;
pub mod scorer;

pub use engine::Engine;

#[cfg(feature = "serde")]
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// Printable US-QWERTY ASCII `0x21..=0x7E`. Letters arrive lowercase with
    /// Shift in `KeyEvent::shift`; digits and punctuation arrive already
    /// shifted (Shift+1 is `b'!'`).
    Char(u8),
    Space,
    Enter,
    Backspace,
    Left,
    Right,
    Up,
    Down,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: Key,
    pub shift: bool,
    /// Ctrl, Alt or Cmd held.
    pub modifier: bool,
}

impl KeyEvent {
    pub fn new(key: Key) -> Self {
        KeyEvent { key, shift: false, modifier: false }
    }
}

/// One composing keystroke as the interpreters see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stroke {
    /// Lowercase for letters; any other printable ASCII is passed through
    /// literally by every interpreter (D13).
    pub ch: u8,
    pub shift: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub enum Lang {
    En,
    Ko,
    Ja,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Candidate {
    pub text: String,
    pub lang: Lang,
    /// Conversion path cost (lower is better) when the interpreter has one;
    /// only Japanese does today. The scorer maps it to a score.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub cost: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Segment {
    pub text: String,
    pub lang: Lang,
    pub candidates: Vec<Candidate>,
    pub selected: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Preedit {
    pub segments: Vec<Segment>,
    pub focused: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Response {
    pub handled: bool,
    pub commit: String,
    pub preedit: Option<Preedit>,
}

/// Produces candidates for one span of keystrokes in one language.
pub trait Interpreter {
    /// Candidates in this interpreter's own preference order; empty when the
    /// span is not a valid input in this language.
    fn interpret(&self, strokes: &[Stroke]) -> Vec<Candidate>;
}
