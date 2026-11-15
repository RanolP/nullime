//! Lattice decoding: split the keystrokes into spans and pick the best-scoring
//! path, where every span is read in whichever language scores best.

use crate::english::EnglishInterpreter;
use crate::scorer::Scorer;
use crate::{Candidate, Interpreter, Lang, Stroke};

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
    /// Every language's candidates for this span: the path's reading first, the rest best first; never empty.
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

/// Best path ending at one stroke position whose last span reads as `lang`.
struct Node {
    best: f32,
    /// Start of the last span on that path, and that span's candidates with
    /// the one this path reads first.
    start: usize,
    candidates: Vec<Candidate>,
    /// Language of the last span; `None` only for the empty path at 0.
    lang: Option<Lang>,
    /// `lang` of the node at `start` this path came from.
    prev: Option<Lang>,
}

/// Incremental DP over a growing stroke sequence. The best path to position
/// `j` depends only on `strokes[..j]`, so appending a stroke computes one new
/// position (at most `MAX_SPAN` spans) and Backspace just drops the last one.
/// Each position keeps the best path per last-span language, because the
/// cost of the next span depends on the language it switches from.
pub struct Lattice {
    /// `nodes[j]` holds the best paths covering `strokes[..j]`, at most one
    /// per language; `nodes[0]` is the empty path.
    nodes: Vec<Vec<Node>>,
}

impl Default for Lattice {
    fn default() -> Self {
        let empty = Node { best: 0.0, start: 0, candidates: Vec::new(), lang: None, prev: None };
        Lattice { nodes: vec![vec![empty]] }
    }
}

/// True when a span starting at `at` would cut a run of letters, i.e. split
/// what is typed as one word.
fn mid_word(strokes: &[Stroke], at: usize) -> bool {
    at > 0 && strokes[at - 1].ch.is_ascii_lowercase() && strokes[at].ch.is_ascii_lowercase()
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
            let mut best: Vec<Node> = Vec::new();
            for start in end.saturating_sub(MAX_SPAN)..end {
                let cands = candidates(&strokes[start..end], interpreters, scorer);
                let mid = mid_word(strokes, start);
                for (i, (cand, score)) in cands.iter().enumerate() {
                    // Only each language's best candidate can start a best path.
                    if cands[..i].iter().any(|(c, _)| c.lang == cand.lang) {
                        continue;
                    }
                    for prev in &self.nodes[start] {
                        let switch = match prev.lang {
                            Some(p) if mid => scorer.mid_word_switch_penalty(p, cand.lang),
                            _ => 0.0,
                        };
                        let total = prev.best + score - scorer.segment_penalty() - switch;
                        let slot = best.iter().position(|n| n.lang == Some(cand.lang));
                        if slot.is_some_and(|k| total <= best[k].best) {
                            continue;
                        }
                        // The path's reading goes first; the rest keep score order.
                        let mut ordered: Vec<Candidate> = cands.iter().map(|(c, _)| c.clone()).collect();
                        ordered[..=i].rotate_right(1);
                        let node = Node {
                            best: total,
                            start,
                            candidates: ordered,
                            lang: Some(cand.lang),
                            prev: prev.lang,
                        };
                        match slot {
                            Some(k) => best[k] = node,
                            None => best.push(node),
                        }
                    }
                }
            }
            assert!(!best.is_empty(), "end > start and candidates are never empty");
            self.nodes.push(best);
        }
    }

    /// The best path's spans, in order.
    pub fn spans(&self) -> Vec<Span> {
        let mut spans = Vec::new();
        let mut end = self.len();
        // First of equals, so ties keep the order nodes were found in.
        let Some(mut node) =
            self.nodes[end].iter().reduce(|a, b| if b.best > a.best { b } else { a })
        else {
            return spans;
        };
        while end > 0 {
            spans.push(Span { start: node.start, end, candidates: node.candidates.clone() });
            end = node.start;
            let prev = node.prev;
            node = self.nodes[end].iter().find(|n| n.lang == prev).expect("path's previous node");
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
