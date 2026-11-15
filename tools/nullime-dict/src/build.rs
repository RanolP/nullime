//! Mozc `dictionary_oss` text files → `nullime-ja.dict`.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::time::Instant;

use nullime_engine::ja_asset::{self, JaAsset, ENTRY_LEN, HEADER_LEN, MAGIC, SECTIONS, VERSION};
use nullime_engine::kanji::{Connection, Dictionary, Surface};
use nullime_engine::romaji::to_katakana;

use crate::MOZC_COMMIT;

/// POS of the unknown-word fallback nodes, looked up by name in `id.def`.
const UNKNOWN_POS: &str = "名詞,一般,*,*,*,*,*";

pub struct Word {
    pub reading: String,
    pub lid: u16,
    pub rid: u16,
    pub cost: i32,
    pub surface: String,
}

/// Entries left out of a trimmed asset (the web demo's small default, D18).
#[derive(Default, Clone, Copy)]
pub struct Trim {
    /// Drop words whose Mozc cost is above this (rarer words cost more).
    pub max_cost: Option<i32>,
}

#[derive(Default)]
pub struct Stats {
    pub words: usize,
    pub trimmed: usize,
    pub readings: usize,
    pub reading_chars: usize,
    pub escaped_chars: usize,
    pub entries: usize,
    pub duplicates: usize,
    pub cost_clamped: usize,
    pub long_surfaces_skipped: usize,
    pub conn_clamped: usize,
    pub conn_exceptions: usize,
    pub surfaces_reading: usize,
    pub surfaces_katakana: usize,
    pub blob_bytes: usize,
    pub sections: Vec<(&'static str, usize)>,
}

impl Stats {
    fn report(&self) {
        eprintln!(
            "words {} (trimmed {}, duplicate surface/POS {}), readings {} over {} characters ({} escaped), entries {} \
             (surface = reading {}, = katakana {}, long skipped {}), surface blob {} B, \
             cost clamped {}, connection exceptions {} (clamped {})",
            self.words, self.trimmed, self.duplicates, self.readings, self.reading_chars, self.escaped_chars, self.entries,
            self.surfaces_reading, self.surfaces_katakana, self.long_surfaces_skipped, self.blob_bytes,
            self.cost_clamped, self.conn_exceptions, self.conn_clamped
        );
        for (name, len) in &self.sections {
            eprintln!("  section {name:<10} {len:>10} B");
        }
    }
}

pub fn run(mozc: &Path, out: &Path, trim: Trim) -> Result<(), String> {
    let started = Instant::now();
    let read = |name: &str| std::fs::read_to_string(mozc.join(name)).map_err(|e| format!("{}: {e}", mozc.join(name).display()));
    let mut words = Vec::new();
    for name in (0..10).map(|i| format!("dictionary{i:02}.txt")).chain(["suffix.txt".to_string()]) {
        parse_words(&read(&name)?, &name, &mut words)?;
    }
    let all = words.len();
    words.retain(|w| trim.max_cost.is_none_or(|max| w.cost <= max));
    let trimmed = all - words.len();
    let unknown_id = parse_unknown_id(&read("id.def")?)?;
    let (n, matrix) = parse_connection(&read("connection_single_column.txt")?)?;
    let license = format!(
        "nullime-ja.dict is built from the Mozc OSS dictionary (https://github.com/google/mozc, \
         commit {MOZC_COMMIT}, src/data/dictionary_oss). Its notices follow verbatim.\n\n{}\n\n{}",
        read("README.txt")?,
        read("LICENSE")?
    );
    let (bytes, mut stats) = write_asset(&words, unknown_id, n, &matrix, &license)?;
    stats.trimmed = trimmed;
    std::fs::write(out, &bytes).map_err(|e| format!("{}: {e}", out.display()))?;
    verify(&bytes, &words, n, &matrix)?;
    stats.report();
    eprintln!(
        "wrote {} ({} bytes, {:.1} MiB) in {:.1}s; verified every connection cost and every entry",
        out.display(),
        bytes.len(),
        bytes.len() as f64 / (1024.0 * 1024.0),
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

pub fn parse_words(text: &str, name: &str, out: &mut Vec<Word>) -> Result<(), String> {
    for (line_no, line) in text.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let bad = || format!("{name}:{}: expected reading\\tlid\\trid\\tcost\\tsurface: {line:?}", line_no + 1);
        let mut f = line.split('\t');
        let (Some(reading), Some(lid), Some(rid), Some(cost), Some(surface)) = (f.next(), f.next(), f.next(), f.next(), f.next())
        else {
            return Err(bad());
        };
        out.push(Word {
            reading: reading.to_string(),
            lid: lid.parse().map_err(|_| bad())?,
            rid: rid.parse().map_err(|_| bad())?,
            cost: cost.parse().map_err(|_| bad())?,
            surface: surface.to_string(),
        });
    }
    Ok(())
}

pub fn parse_unknown_id(id_def: &str) -> Result<u16, String> {
    id_def
        .lines()
        .find_map(|l| {
            let (id, pos) = l.split_once(' ')?;
            (pos == UNKNOWN_POS).then(|| id.parse().ok()).flatten()
        })
        .ok_or_else(|| format!("id.def has no `{UNKNOWN_POS}` line"))
}

pub fn parse_connection(text: &str) -> Result<(usize, Vec<i32>), String> {
    let mut lines = text.lines();
    let n: usize = lines.next().and_then(|l| l.trim().parse().ok()).ok_or("connection: first line must be N")?;
    let matrix: Vec<i32> = lines
        .filter(|l| !l.is_empty())
        .map(|l| l.trim().parse().map_err(|_| format!("connection: bad cost {l:?}")))
        .collect::<Result<_, _>>()?;
    if matrix.len() != n * n {
        return Err(format!("connection: expected {} costs, found {}", n * n, matrix.len()));
    }
    Ok((n, matrix))
}

fn clamp(v: i32, counter: &mut usize) -> i16 {
    let c = v.clamp(i16::MIN as i32, i16::MAX as i32);
    if c != v {
        *counter += 1;
    }
    c as i16
}

fn put_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

pub fn write_asset(words: &[Word], unknown_id: u16, n: usize, matrix: &[i32], license: &str) -> Result<(Vec<u8>, Stats), String> {
    let mut stats = Stats { words: words.len(), ..Stats::default() };
    if n > u16::MAX as usize + 1 || words.iter().any(|w| w.lid as usize >= n || w.rid as usize >= n) {
        return Err("POS id outside the connection matrix".into());
    }

    // Bytewise-sorted readings (the trie's requirement) → (surface, lid, rid) → lowest cost.
    let mut by_reading: BTreeMap<&str, HashMap<(&str, u16, u16), i32>> = BTreeMap::new();
    for w in words {
        if w.reading.is_empty() || w.reading.contains('\0') {
            continue;
        }
        let slot = by_reading.entry(&w.reading).or_default().entry((&w.surface, w.lid, w.rid)).or_insert(i32::MAX);
        if *slot != i32::MAX {
            stats.duplicates += 1;
        }
        *slot = (*slot).min(w.cost);
    }

    let mut pos_ids: HashMap<(u16, u16), u16> = HashMap::new();
    let mut pos_table = Vec::new();
    let mut blob = Vec::new();
    let mut blob_index: HashMap<&str, u32> = HashMap::new();
    let mut entries = Vec::new();
    let mut groups = Vec::new();
    // The most frequent characters get one-byte codes; rarer ones (user
    // dictionaries can carry any) are spelled escaped.
    let mut freq: HashMap<char, usize> = HashMap::new();
    for c in by_reading.keys().flat_map(|r| r.chars()) {
        *freq.entry(c).or_default() += 1;
    }
    let mut ranked: Vec<(usize, char)> = freq.into_iter().map(|(c, n)| (n, c)).collect();
    ranked.sort_by(|a, b| b.cmp(a));
    let mut chars: Vec<char> = ranked.iter().take(ja_asset::CHAR_ESCAPE as usize - 1).map(|&(_, c)| c).collect();
    chars.sort();
    stats.escaped_chars = ranked.len() - chars.len();
    let mut charmap = Vec::new();
    for &c in &chars {
        put_u32(&mut charmap, c as u32);
    }
    let encode = |reading: &str| -> Vec<u8> {
        let mut key = Vec::with_capacity(reading.len());
        for c in reading.chars() {
            match chars.binary_search(&c) {
                Ok(i) => key.push(i as u8 + 1),
                Err(_) => {
                    key.push(ja_asset::CHAR_ESCAPE);
                    key.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
                }
            }
        }
        key
    };
    let mut keys: Vec<(Vec<u8>, u32)> = Vec::new();
    for (reading, set) in &by_reading {
        let mut list: Vec<_> = set.iter().map(|(&(s, l, r), &c)| (c, s, l, r)).collect();
        list.sort();
        let first = (entries.len() / ENTRY_LEN) as u32;
        let katakana = to_katakana(reading);
        for (cost, surface, lid, rid) in list {
            let surface_ref = if surface == *reading {
                stats.surfaces_reading += 1;
                ja_asset::SURFACE_READING << 30
            } else if surface == katakana {
                stats.surfaces_katakana += 1;
                ja_asset::SURFACE_KATAKANA << 30
            } else if surface.len() > u8::MAX as usize {
                stats.long_surfaces_skipped += 1;
                continue;
            } else {
                let off = *blob_index.entry(surface).or_insert_with(|| {
                    let off = blob.len() as u32;
                    blob.push(surface.len() as u8);
                    blob.extend_from_slice(surface.as_bytes());
                    off
                });
                if off > ja_asset::SURFACE_OFFSET_MASK {
                    return Err("surface blob exceeds 1 GiB".into());
                }
                off
            };
            let next = pos_ids.len();
            let pos = *pos_ids.entry((lid, rid)).or_insert_with(|| {
                put_u16(&mut pos_table, lid);
                put_u16(&mut pos_table, rid);
                next as u16
            });
            if pos_ids.len() > u16::MAX as usize {
                return Err("more than 65535 (lid, rid) pairs".into());
            }
            put_u32(&mut entries, surface_ref);
            put_u16(&mut entries, pos);
            put_u16(&mut entries, clamp(cost, &mut stats.cost_clamped) as u16);
        }
        let end = (entries.len() / ENTRY_LEN) as u32;
        if end > first {
            keys.push((encode(reading), (groups.len() / 4) as u32));
            put_u32(&mut groups, first);
        }
    }
    put_u32(&mut groups, (entries.len() / ENTRY_LEN) as u32);
    stats.readings = keys.len();
    stats.entries = entries.len() / ENTRY_LEN;
    stats.blob_bytes = blob.len();
    stats.reading_chars = ranked.len();
    // Code order differs from UTF-8 order; the trie needs its keys sorted.
    keys.sort();
    let trie = yada::builder::DoubleArrayBuilder::build(&keys).map_err(|e| format!("trie: {e}"))?;

    let connection = write_connection(n, matrix, &mut stats);

    let sections: [&[u8]; 8] = [license.as_bytes(), &charmap, &trie, &groups, &entries, &pos_table, &blob, &connection];
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    put_u32(&mut out, VERSION);
    put_u16(&mut out, unknown_id);
    put_u16(&mut out, 0);
    let table_at = out.len();
    out.resize(HEADER_LEN, 0);
    for (i, s) in sections.iter().enumerate() {
        while out.len() % 8 != 0 {
            out.push(0);
        }
        let off = out.len() as u32;
        out[table_at + i * 8..table_at + i * 8 + 4].copy_from_slice(&off.to_le_bytes());
        out[table_at + i * 8 + 4..table_at + i * 8 + 8].copy_from_slice(&(s.len() as u32).to_le_bytes());
        out.extend_from_slice(s);
        stats.sections.push((SECTIONS[i], s.len()));
    }
    Ok((out, stats))
}

/// Per row: the most frequent cost as the default, a bitmap of columns that
/// differ, a running popcount per bitmap word, and the differing costs.
fn write_connection(n: usize, matrix: &[i32], stats: &mut Stats) -> Vec<u8> {
    let words = n.div_ceil(64);
    let mut defaults = Vec::new();
    let mut exc_start = Vec::new();
    let mut bitmap = Vec::new();
    let mut rank = Vec::new();
    let mut exceptions = Vec::new();
    let mut exception_count = 0u32;
    for row in matrix.chunks(n) {
        let row: Vec<i16> = row.iter().map(|&v| clamp(v, &mut stats.conn_clamped)).collect();
        let mut freq: HashMap<i16, usize> = HashMap::new();
        for &v in &row {
            *freq.entry(v).or_default() += 1;
        }
        let default = freq.into_iter().max_by_key(|&(v, c)| (c, std::cmp::Reverse(v))).map_or(0, |(v, _)| v);
        put_u16(&mut defaults, default as u16);
        put_u32(&mut exc_start, exception_count);
        let mut in_row = 0u16;
        for w in 0..words {
            put_u16(&mut rank, in_row);
            let mut bits = 0u64;
            for b in 0..64 {
                let col = w * 64 + b;
                if col < n && row[col] != default {
                    bits |= 1 << b;
                    put_u16(&mut exceptions, row[col] as u16);
                    in_row += 1;
                }
            }
            bitmap.extend_from_slice(&bits.to_le_bytes());
        }
        exception_count += in_row as u32;
    }
    stats.conn_exceptions = exception_count as usize;
    let mut out = Vec::new();
    put_u32(&mut out, n as u32);
    for part in [defaults, exc_start, bitmap, rank, exceptions] {
        out.extend_from_slice(&part);
    }
    out
}

/// Reads the written bytes back through the engine's own reader: every
/// connection cost, and every word reachable under its reading.
fn verify(bytes: &[u8], words: &[Word], n: usize, matrix: &[i32]) -> Result<(), String> {
    let asset = JaAsset::from_bytes(bytes).map_err(|e| format!("verify: {e}"))?;
    for r in 0..n {
        for l in 0..n {
            let want = matrix[r * n + l].clamp(i16::MIN as i32, i16::MAX as i32);
            let got = asset.cost(r as u16, l as u16);
            if got != want {
                return Err(format!("verify: connection ({r},{l}) = {got}, expected {want}"));
            }
        }
    }
    let mut by_reading: HashMap<&str, Vec<&Word>> = HashMap::new();
    for w in words.iter().filter(|w| w.surface.len() <= u8::MAX as usize) {
        by_reading.entry(&w.reading).or_default().push(w);
    }
    for (reading, ws) in by_reading {
        let mut found: Vec<(String, u16, u16)> = Vec::new();
        asset.common_prefix(reading, &mut |len, e| {
            if len == reading.len() {
                let s = match e.surface {
                    Surface::Text(s) => s.to_string(),
                    Surface::Reading => reading.to_string(),
                    Surface::Katakana => to_katakana(reading),
                };
                found.push((s, e.lid, e.rid));
            }
        });
        for w in ws {
            if !found.iter().any(|(s, l, r)| *s == w.surface && *l == w.lid && *r == w.rid) {
                return Err(format!("verify: {} → {} missing", w.reading, w.surface));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nullime_engine::ja_convert::convert;

    // Catches the writer and the engine's reader disagreeing on the layout
    // (offsets, flags, POS table, connection bitmap/rank), using rows in
    // Mozc's own text format.
    #[test]
    fn written_asset_reads_back_and_converts() {
        let dict = "わたし\t1\t1\t2000\t私\nわたし\t1\t1\t5000\tわたし\nは\t2\t2\t1000\tは\nてすと\t1\t1\t3000\tテスト\n";
        let mut words = Vec::new();
        parse_words(dict, "test", &mut words).unwrap();
        let (n, matrix) = parse_connection("3\n0\n100\n900\n100\n800\n50\n100\n60\n700\n").unwrap();
        let (bytes, _) = write_asset(&words, 1, n, &matrix, "LICENSE TEXT").unwrap();
        verify(&bytes, &words, n, &matrix).unwrap();
        let asset = JaAsset::from_bytes(&bytes).unwrap();
        assert_eq!(asset.license(), "LICENSE TEXT");
        assert_eq!(convert(&asset, "わたしはてすと", 1)[0].surface, "私はテスト");
    }

    // Catches a file whose trie root is a leaf unit passing yada's validation
    // and then reading out of bounds on the first Japanese keystroke.
    #[test]
    fn leaf_trie_root_is_rejected() {
        let mut words = Vec::new();
        parse_words("あ\t1\t1\t1000\t亜\n", "test", &mut words).unwrap();
        let (n, matrix) = parse_connection("2\n0\n0\n0\n0\n").unwrap();
        let (mut bytes, _) = write_asset(&words, 1, n, &matrix, "").unwrap();
        let header = 16 + ja_asset::TRIE * 8;
        let root = u32::from_le_bytes(bytes[header..header + 4].try_into().unwrap()) as usize;
        bytes[root + 3] |= 0x80;
        assert_eq!(JaAsset::new(bytes).err(), Some(ja_asset::AssetError::Corrupt("trie root is a leaf")));
    }

    // Catches readings with characters beyond the 254 one-byte codes (a
    // user-built dictionary, D17) becoming unfindable or rejected.
    #[test]
    fn rare_reading_characters_are_escaped_and_found() {
        let mut dict = String::new();
        for i in 0..300u32 {
            let c = char::from_u32(0x4E00 + i).unwrap();
            // Frequent characters repeat so the 254 cut is deterministic.
            let reading = if i < 254 { format!("{c}{c}") } else { c.to_string() };
            dict.push_str(&format!("{reading}\t1\t1\t1000\tW{i}\n"));
        }
        let mut words = Vec::new();
        parse_words(&dict, "test", &mut words).unwrap();
        let (n, matrix) = parse_connection("2\n0\n0\n0\n0\n").unwrap();
        let (bytes, stats) = write_asset(&words, 1, n, &matrix, "").unwrap();
        assert_eq!(stats.escaped_chars, 46);
        verify(&bytes, &words, n, &matrix).unwrap();
        let asset = JaAsset::from_bytes(&bytes).unwrap();
        let rare = char::from_u32(0x4E00 + 299).unwrap().to_string();
        assert_eq!(convert(&asset, &rare, 1)[0].surface, "W299");
    }
}
