//! Lattice decoding: split the keystrokes into spans and pick the best-scoring
//! path, where every span is read in whichever language scores best.

use crate::english::EnglishInterpreter;
use crate::scorer::Scorer;
use crate::{Candidate, Interpreter, Stroke};

/// Longest span one segment may cover. Bounds decoding to O(n * MAX_SPAN)
/// spans; longer input simply becomes several segments.
pub const MAX_SPAN: usize = 32;

/// Score of the literal fallback per stroke, for spans no interpreter reads.
/// Low enough that any real reading wins.
const FALLBACK_PER_STROKE: f32 = -10.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    /// Stroke range relative to the slice given to `decode`.
    pub start: usize,
    pub end: usize,
    /// Every language's candidates for this span, best first; never empty.
    pub candidates: Vec<Candidate>,
}

/// All candidates for one span, scored, best first, duplicates (same text)
/// keeping the higher score. Never empty: when no interpreter reads the span,
/// the keys as typed come back as a low-scoring literal.
pub fn candidates(
    strokes: &[Stroke],
    interpreters: &[Box<dyn Interpreter>],
    scorer: &dyn Scorer,
) -> Vec<(Candidate, f32)> {
    let mut scored: Vec<(Candidate, f32)> = interpreters
        .iter()
        .flat_map(|interp| {
            interp
                .interpret(strokes)
                .into_iter()
                .enumerate()
                .map(|(rank, c)| {
                    let s = scorer.score(strokes, &c, rank);
                    (c, s)
                })
        })
        .collect();
    if scored.is_empty() {
        let literal = EnglishInterpreter.interpret(strokes).remove(0);
        return vec![(literal, FALLBACK_PER_STROKE * strokes.len() as f32)];
    }
    // Stable, so ties keep interpreter order.
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut out: Vec<(Candidate, f32)> = Vec::with_capacity(scored.len());
    for (c, s) in scored {
        if !out.iter().any(|(o, _)| o.text == c.text) {
            out.push((c, s));
        }
    }
    out
}

/// Best path ending at one stroke position.
struct Node {
    best: f32,
    /// Start of the last span on that path, and that span's candidates.
    start: usize,
    candidates: Vec<Candidate>,
}

/// Incremental DP over a growing stroke sequence. The best path to position
/// `j` depends only on `strokes[..j]`, so appending a stroke computes one new
/// node (at most `MAX_SPAN` spans) and Backspace just drops the last node.
pub struct Lattice {
    /// `nodes[j]` is the best path covering `strokes[..j]`; `nodes[0]` is empty.
    nodes: Vec<Node>,
}

impl Default for Lattice {
    fn default() -> Self {
        Lattice { nodes: vec![Node { best: 0.0, start: 0, candidates: Vec::new() }] }
    }
}

impl Lattice {
    /// Number of strokes covered.
    pub fn len(&self) -> usize {
        self.nodes.len() - 1
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn truncate(&mut self, len: usize) {
        self.nodes.truncate(len + 1);
    }

    /// Extends the lattice to cover all of `strokes`, whose first `self.len()`
    /// strokes must be the ones already covered.
    pub fn extend(
        &mut self,
        strokes: &[Stroke],
        interpreters: &[Box<dyn Interpreter>],
        scorer: &dyn Scorer,
    ) {
        for end in self.len() + 1..=strokes.len() {
            let mut best: Option<Node> = None;
            for start in end.saturating_sub(MAX_SPAN)..end {
                let cands = candidates(&strokes[start..end], interpreters, scorer);
                let total = self.nodes[start].best + cands[0].1 - scorer.segment_penalty();
                if best.as_ref().is_none_or(|b| total > b.best) {
                    best = Some(Node {
                        best: total,
                        start,
                        candidates: cands.into_iter().map(|(c, _)| c).collect(),
                    });
                }
            }
            self.nodes.push(best.expect("end > start, so at least one span"));
        }
    }

    /// The best path's spans, in order.
    pub fn spans(&self) -> Vec<Span> {
        let mut spans = Vec::new();
        let mut end = self.len();
        while end > 0 {
            let node = &self.nodes[end];
            spans.push(Span { start: node.start, end, candidates: node.candidates.clone() });
            end = node.start;
        }
        spans.reverse();
        spans
    }
}

/// One-shot decode of a whole stroke sequence.
pub fn decode(
    strokes: &[Stroke],
    interpreters: &[Box<dyn Interpreter>],
    scorer: &dyn Scorer,
) -> Vec<Span> {
    let mut lattice = Lattice::default();
    lattice.extend(strokes, interpreters, scorer);
    lattice.spans()
}
