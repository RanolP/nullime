//! Kana-kanji conversion of one hiragana reading, Mozc style: a word lattice
//! from common-prefix lookups at every character position, unknown-word
//! fallback nodes so every reading has a path, Viterbi forward costs, then
//! exact A* backward search for the N best distinct surfaces.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};

use crate::kanji::{Connection, Dictionary, Surface};
use crate::romaji::to_katakana;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversion {
    pub surface: String,
    /// Word plus connection costs of the whole path, BOS to EOS; lower is better.
    pub cost: i32,
}

/// Unknown spans cost more than any normal word so the dictionary wins
/// wherever it has an answer; one longer unknown span beats a chain of
/// one-character unknowns, and hiragana beats katakana for them.
const UNKNOWN_BASE: i32 = 9000;
const UNKNOWN_PER_CHAR: i32 = 400;
const UNKNOWN_KATAKANA_EXTRA: i32 = 600;
const UNKNOWN_MAX_CHARS: usize = 8;
/// Bounds A* work on pathological lattices; N-best stops early instead.
const MAX_POPS: usize = 4096;

struct Node<'a> {
    start: usize,
    end: usize,
    lid: u16,
    rid: u16,
    cost: i32,
    surface: Surface<'a>,
}

impl Node<'_> {
    fn push_surface(&self, reading: &str, out: &mut String) {
        match self.surface {
            Surface::Text(s) => out.push_str(s),
            Surface::Reading => out.push_str(&reading[self.start..self.end]),
            Surface::Katakana => out.push_str(&to_katakana(&reading[self.start..self.end])),
        }
    }
}

/// The `n` lowest-cost distinct surfaces for `reading`, best first. Empty only
/// for an empty reading.
pub fn convert<D: Dictionary + Connection + ?Sized>(model: &D, reading: &str, n: usize) -> Vec<Conversion> {
    if reading.is_empty() || n == 0 {
        return Vec::new();
    }
    let len = reading.len();
    let edge = model.bos_eos();
    let mut nodes: Vec<Node> = vec![Node { start: 0, end: 0, lid: edge, rid: edge, cost: 0, surface: Surface::Reading }];
    // Node indices by start and by end byte position.
    let mut starting: Vec<Vec<usize>> = vec![Vec::new(); len + 1];
    let mut ending: Vec<Vec<usize>> = vec![Vec::new(); len + 1];
    ending[0].push(0);

    let unknown = model.unknown_id();
    let boundaries: Vec<usize> = reading.char_indices().map(|(i, _)| i).collect();
    for (k, &pos) in boundaries.iter().enumerate() {
        let rest = &reading[pos..];
        let before = nodes.len();
        model.common_prefix(rest, &mut |byte_len, e| {
            nodes.push(Node { start: pos, end: pos + byte_len, lid: e.lid, rid: e.rid, cost: e.cost as i32, surface: e.surface });
        });
        // Like MeCab: multi-character unknown words only where no dictionary
        // word starts, or one long unknown span would swallow known words.
        let max_chars = if nodes.len() > before { 1 } else { UNKNOWN_MAX_CHARS };
        for chars in 1..=max_chars.min(boundaries.len() - k) {
            let end = boundaries.get(k + chars).copied().unwrap_or(len);
            let cost = UNKNOWN_BASE + UNKNOWN_PER_CHAR * chars as i32;
            nodes.push(Node { start: pos, end, lid: unknown, rid: unknown, cost, surface: Surface::Reading });
            nodes.push(Node {
                start: pos,
                end,
                lid: unknown,
                rid: unknown,
                cost: cost + UNKNOWN_KATAKANA_EXTRA,
                surface: Surface::Katakana,
            });
        }
    }
    let eos = nodes.len();
    nodes.push(Node { start: len, end: len, lid: edge, rid: edge, cost: 0, surface: Surface::Reading });
    for (i, node) in nodes.iter().enumerate().skip(1) {
        starting[node.start].push(i);
        if i != eos {
            ending[node.end].push(i);
        }
    }

    // fwd[i]: lowest cost from BOS through node i, including its own cost.
    let mut fwd = vec![i32::MAX; nodes.len()];
    fwd[0] = 0;
    for pos in boundaries.iter().copied().chain(std::iter::once(len)) {
        for &i in &starting[pos] {
            let node = &nodes[i];
            let best = ending[pos]
                .iter()
                .filter(|&&p| fwd[p] != i32::MAX)
                .map(|&p| fwd[p] + model.cost(nodes[p].rid, node.lid))
                .min();
            if let Some(best) = best {
                fwd[i] = best + node.cost;
            }
        }
    }
    if fwd[eos] == i32::MAX {
        return Vec::new();
    }

    // A* from EOS back to BOS. `g` is the exact cost of the path after a
    // node; fwd is an exact heuristic, so complete paths pop in cost order.
    // Arena entries are (node, successor entry).
    let mut arena: Vec<(usize, usize)> = vec![(eos, usize::MAX)];
    let mut heap = BinaryHeap::new();
    heap.push(Reverse((fwd[eos], 0i32, 0usize)));
    let mut out: Vec<Conversion> = Vec::new();
    let mut seen = HashSet::new();
    let mut pops = 0;
    while let Some(Reverse((f, g, entry))) = heap.pop() {
        pops += 1;
        if pops > MAX_POPS {
            break;
        }
        let (i, _) = arena[entry];
        if i == 0 {
            let mut surface = String::new();
            let mut at = arena[entry].1;
            while at != usize::MAX && arena[at].0 != eos {
                nodes[arena[at].0].push_surface(reading, &mut surface);
                at = arena[at].1;
            }
            if seen.insert(surface.clone()) {
                out.push(Conversion { surface, cost: f });
                if out.len() >= n {
                    break;
                }
            }
            continue;
        }
        let node = &nodes[i];
        for &p in &ending[node.start] {
            if fwd[p] == i32::MAX {
                continue;
            }
            let g2 = g + node.cost + model.cost(nodes[p].rid, node.lid);
            arena.push((p, entry));
            heap.push(Reverse((fwd[p] + g2, g2, arena.len() - 1)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kanji::SampleDictionary;

    fn top(reading: &str) -> String {
        convert(&SampleDictionary, reading, 1).remove(0).surface
    }

    // Catches Viterbi choosing a locally cheap word over the cheaper whole path
    // (here 日本語 vs 日本+ご, and particles joining nouns into a sentence).
    #[test]
    fn multi_word_viterbi_picks_lower_cost_path() {
        assert_eq!(top("にほんご"), "日本語");
        assert_eq!(top("わたしはがくせいです"), "私は学生です");
    }

    // Catches readings with no dictionary entry producing no conversion at all.
    #[test]
    fn unknown_span_falls_back_to_kana() {
        let all = convert(&SampleDictionary, "ぴよ", 4);
        assert_eq!(all[0].surface, "ぴよ");
        assert!(all.iter().any(|c| c.surface == "ピヨ"));
        assert_eq!(top("ねこぴよ"), "猫ぴよ");
    }

    // Catches N-best returning duplicates or out-of-order costs.
    #[test]
    fn n_best_is_distinct_and_sorted() {
        let all = convert(&SampleDictionary, "さけ", 5);
        assert_eq!(all[0].surface, "酒");
        assert_eq!(all[1].surface, "鮭");
        assert!(all.windows(2).all(|w| w[0].cost <= w[1].cost));
        let mut texts: Vec<_> = all.iter().map(|c| &c.surface).collect();
        texts.dedup();
        assert_eq!(texts.len(), all.len());
    }
}
