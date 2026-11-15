//! Lattice decoding: split the keystrokes into spans and pick the best-scoring
//! path, where every span is read in whichever language scores best.

use crate::scorer::Scorer;
use crate::{Candidate, Interpreter, Stroke};

/// Longest span one segment may cover. Bounds decoding to O(n * MAX_SPAN)
/// spans; longer input simply becomes several segments.
pub const MAX_SPAN: usize = 32;

#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    /// Stroke range relative to the slice given to `decode`.
    pub start: usize,
    pub end: usize,
    /// Every language's candidates for this span, best first.
    pub candidates: Vec<Candidate>,
}

/// All candidates for one span, scored, best first, duplicates (same text)
/// keeping the higher score.
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

pub fn decode(
    strokes: &[Stroke],
    interpreters: &[Box<dyn Interpreter>],
    scorer: &dyn Scorer,
) -> Vec<Span> {
    let n = strokes.len();
    let mut best = vec![f32::NEG_INFINITY; n + 1];
    let mut back = vec![0usize; n + 1];
    best[0] = 0.0;
    for end in 1..=n {
        for start in end.saturating_sub(MAX_SPAN)..end {
            let Some((_, top)) = candidates(&strokes[start..end], interpreters, scorer)
                .into_iter()
                .next()
            else {
                continue;
            };
            let total = best[start] + top - scorer.segment_penalty();
            if total > best[end] {
                best[end] = total;
                back[end] = start;
            }
        }
    }
    let mut spans = Vec::new();
    let mut end = n;
    while end > 0 {
        let start = back[end];
        let candidates = candidates(&strokes[start..end], interpreters, scorer)
            .into_iter()
            .map(|(c, _)| c)
            .collect();
        spans.push(Span { start, end, candidates });
        end = start;
    }
    spans.reverse();
    spans
}
