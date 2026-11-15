//! Reader for `nullime-ja.dict`, the external Japanese conversion asset
//! (D15). `tools/nullime-dict` writes it; the layout is in
//! docs/architecture.md "일본어 사전 애셋". Every multi-byte field is
//! little-endian and read with `from_le_bytes`, so the bytes need no alignment
//! and can come straight from a memory map or a JS `Uint8Array`.

use std::fmt;
use std::ops::{Deref, Range};
use std::sync::Arc;

use crate::kanji::{Connection, Dictionary, Entry, Surface};

pub const MAGIC: &[u8; 8] = b"NULLIMEJ";
pub const VERSION: u32 = 1;

/// Section order in the header's section table.
pub const SECTIONS: [&str; 8] = ["license", "charmap", "trie", "groups", "entries", "pos", "surfaces", "connection"];
pub const LICENSE: usize = 0;
/// Sorted u32 code points of up to 254 frequent reading characters. A trie
/// key spells a listed character as its 1-based index (one byte instead of
/// three for kana) and any other as `CHAR_ESCAPE` plus its UTF-8 bytes.
pub const CHARMAP: usize = 1;
pub const CHAR_ESCAPE: u8 = 0xFF;
pub const TRIE: usize = 2;
pub const GROUPS: usize = 3;
pub const ENTRIES: usize = 4;
pub const POS: usize = 5;
pub const SURFACES: usize = 6;
pub const CONNECTION: usize = 7;

/// magic(8) + version(4) + unknown_id(2) + reserved(2) + 8 × (offset u32, len u32).
pub const HEADER_LEN: usize = 16 + SECTIONS.len() * 8;
pub const ENTRY_LEN: usize = 8;

/// Top two bits of an entry's `surface_ref`; the low 30 bits are a byte
/// offset into the surface blob when the kind is `SURFACE_TEXT`.
pub const SURFACE_TEXT: u32 = 0;
pub const SURFACE_READING: u32 = 1;
pub const SURFACE_KATAKANA: u32 = 2;
pub const SURFACE_OFFSET_MASK: u32 = (1 << 30) - 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetError {
    TooShort,
    BadMagic,
    UnsupportedVersion(u32),
    SectionOutOfBounds(&'static str),
    Trie(String),
    Corrupt(&'static str),
}

impl fmt::Display for AssetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AssetError::TooShort => write!(f, "file shorter than the header"),
            AssetError::BadMagic => write!(f, "not a nullime-ja.dict file (bad magic)"),
            AssetError::UnsupportedVersion(v) => write!(f, "unsupported asset version {v}, expected {VERSION}"),
            AssetError::SectionOutOfBounds(s) => write!(f, "section `{s}` lies outside the file"),
            AssetError::Trie(e) => write!(f, "reading index: {e}"),
            AssetError::Corrupt(what) => write!(f, "corrupt asset: {what}"),
        }
    }
}

impl std::error::Error for AssetError {}

/// Bytes that stay the same for the owner's whole life.
///
/// # Safety
///
/// Every `as_ref()` call must return the same slice (same address, length
/// and contents). The yada trie reads its units with `get_unchecked` after
/// one validation at load, so bytes that change afterwards are out-of-bounds
/// reads. A memory map qualifies only while nothing rewrites the file; the
/// implementer of that wrapper takes on that promise.
pub unsafe trait StableBytes: AsRef<[u8]> + Send + Sync + 'static {}

// SAFETY: owned buffers only change through `&mut`, which the asset never
// hands out once it has taken them.
unsafe impl StableBytes for Vec<u8> {}
unsafe impl StableBytes for Box<[u8]> {}
unsafe impl StableBytes for Arc<[u8]> {}
unsafe impl StableBytes for &'static [u8] {}

/// Most entries one reading contributes per lookup; groups are sorted by
/// cost, so a cut keeps the best. Mozc OSS's largest group has 286.
const MAX_ENTRIES_PER_READING: usize = 512;

type Owner = Arc<dyn AsRef<[u8]> + Send + Sync>;

/// A byte range of the shared owner, so the trie can hold its section
/// without borrowing from `JaAsset`.
#[derive(Clone)]
struct Section {
    owner: Owner,
    range: Range<usize>,
}

impl Deref for Section {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &(*self.owner).as_ref()[self.range.clone()]
    }
}

/// Byte offsets of the connection section's parts, relative to the file.
#[derive(Clone)]
struct ConnLayout {
    n: usize,
    /// u64 words per bitmap row.
    words: usize,
    defaults: usize,
    exc_start: usize,
    bitmap: usize,
    rank: usize,
    exceptions: Range<usize>,
}

/// The loaded asset. Owns (shares) its bytes, so the engine can keep it for
/// its whole life whether they came from a `Vec<u8>` or a memory map.
#[derive(Clone)]
pub struct JaAsset {
    owner: Owner,
    unknown_id: u16,
    sections: [Range<usize>; 8],
    trie: yada::DoubleArray<Section>,
    groups: usize,
    entries: usize,
    conn: ConnLayout,
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

impl JaAsset {
    /// Copies `bytes`; use `new` to keep a memory map or an owned buffer
    /// without copying.
    pub fn from_bytes(bytes: &[u8]) -> Result<JaAsset, AssetError> {
        JaAsset::new(bytes.to_vec())
    }

    pub fn new(bytes: impl StableBytes) -> Result<JaAsset, AssetError> {
        let owner: Owner = Arc::new(bytes);
        let b = (*owner).as_ref();
        if b.len() < HEADER_LEN {
            return Err(AssetError::TooShort);
        }
        if &b[..8] != MAGIC {
            return Err(AssetError::BadMagic);
        }
        let version = u32_at(b, 8).unwrap_or(0);
        if version != VERSION {
            return Err(AssetError::UnsupportedVersion(version));
        }
        let unknown_id = u16_at(b, 12).unwrap_or(0);
        let mut sections: [Range<usize>; 8] = Default::default();
        for (i, name) in SECTIONS.iter().enumerate() {
            let off = u32_at(b, 16 + i * 8).unwrap_or(0) as usize;
            let len = u32_at(b, 20 + i * 8).unwrap_or(0) as usize;
            let end = off.checked_add(len).filter(|&e| e <= b.len() && off >= HEADER_LEN);
            sections[i] = off..end.ok_or(AssetError::SectionOutOfBounds(name))?;
        }
        let trie_section = Section { owner: owner.clone(), range: sections[TRIE].clone() };
        let trie = yada::DoubleArray::new(trie_section).map_err(|e| AssetError::Trie(e.to_string()))?;
        // yada's validation skips leaf units, yet every search starts by
        // following unit 0's offset unchecked; a leaf root would read out of
        // bounds.
        if u32_at(b, sections[TRIE].start).is_none_or(|root| root >> 31 == 1) {
            return Err(AssetError::Corrupt("trie root is a leaf"));
        }

        let groups = sections[GROUPS].len() / 4;
        let entries = sections[ENTRIES].len() / ENTRY_LEN;
        let charmap = sections[CHARMAP].len();
        if groups == 0
            || !charmap.is_multiple_of(4)
            || charmap / 4 >= CHAR_ESCAPE as usize
            || !sections[ENTRIES].len().is_multiple_of(ENTRY_LEN)
            || !sections[POS].len().is_multiple_of(4)
        {
            return Err(AssetError::Corrupt("table sizes"));
        }
        let conn = Self::conn_layout(b, sections[CONNECTION].clone())?;
        Ok(JaAsset { owner, unknown_id, sections, trie, groups: groups - 1, entries, conn })
    }

    fn conn_layout(b: &[u8], s: Range<usize>) -> Result<ConnLayout, AssetError> {
        let bad = AssetError::Corrupt("connection section");
        let n = u32_at(b, s.start).ok_or(bad.clone())? as usize;
        if n == 0 || n > u16::MAX as usize + 1 {
            return Err(bad);
        }
        let words = n.div_ceil(64);
        let defaults = s.start + 4;
        let exc_start = defaults + n * 2;
        let bitmap = exc_start + n * 4;
        let rank = bitmap + n * words * 8;
        let exceptions = rank + n * words * 2..s.end;
        if exceptions.start > s.end || exceptions.len() % 2 != 0 {
            return Err(bad);
        }
        Ok(ConnLayout { n, words, defaults, exc_start, bitmap, rank, exceptions })
    }

    fn bytes(&self) -> &[u8] {
        (*self.owner).as_ref()
    }

    fn section(&self, i: usize) -> &[u8] {
        self.bytes().get(self.sections[i].clone()).unwrap_or(&[])
    }

    /// The license notices the converter embedded (Mozc / IPAdic / ICOT).
    pub fn license(&self) -> &str {
        std::str::from_utf8(self.section(LICENSE)).unwrap_or("")
    }

    pub fn entry_count(&self) -> usize {
        self.entries
    }

    pub fn pos_count(&self) -> usize {
        self.conn.n
    }

    /// One-byte trie code for `c`, or None when it is spelled escaped.
    fn code(&self, c: char) -> Option<u8> {
        let map = self.section(CHARMAP);
        let (mut lo, mut hi) = (0, map.len() / 4);
        while lo < hi {
            let mid = (lo + hi) / 2;
            match u32_at(map, mid * 4)?.cmp(&(c as u32)) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => return Some(mid as u8 + 1),
            }
        }
        None
    }

    fn entry(&self, i: usize) -> Option<Entry<'_>> {
        let e = self.section(ENTRIES);
        let at = i * ENTRY_LEN;
        let surface_ref = u32_at(e, at)?;
        let pos = u16_at(e, at + 4)? as usize;
        let cost = u16_at(e, at + 6)? as i16;
        let pos_table = self.section(POS);
        let lid = u16_at(pos_table, pos * 4)?;
        let rid = u16_at(pos_table, pos * 4 + 2)?;
        let surface = match surface_ref >> 30 {
            SURFACE_READING => Surface::Reading,
            SURFACE_KATAKANA => Surface::Katakana,
            SURFACE_TEXT => {
                let blob = self.section(SURFACES);
                let off = (surface_ref & SURFACE_OFFSET_MASK) as usize;
                let len = *blob.get(off)? as usize;
                Surface::Text(std::str::from_utf8(blob.get(off + 1..off + 1 + len)?).ok()?)
            }
            _ => return None,
        };
        Some(Entry { surface, lid, rid, cost })
    }
}

impl Dictionary for JaAsset {
    fn common_prefix<'a>(&'a self, text: &str, f: &mut dyn FnMut(usize, Entry<'a>)) {
        let groups = self.section(GROUPS);
        // `ends[k]` maps key length k back to the byte length of `text` it
        // covers; matches only end on whole encoded characters.
        let mut key = Vec::with_capacity(text.len());
        let mut ends = vec![usize::MAX; text.len() * 4 + 1];
        for (i, c) in text.char_indices() {
            match self.code(c) {
                Some(code) => key.push(code),
                None => {
                    key.push(CHAR_ESCAPE);
                    key.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
                }
            }
            ends[key.len()] = i + c.len_utf8();
        }
        for (group, key_len) in self.trie.common_prefix_search(&key[..]) {
            let Some(&len) = ends.get(key_len).filter(|&&l| l != usize::MAX) else { continue };
            let group = group as usize;
            if group >= self.groups {
                continue;
            }
            let (Some(first), Some(end)) = (u32_at(groups, group * 4), u32_at(groups, group * 4 + 4)) else {
                continue;
            };
            let first = first as usize;
            let end = (end as usize).min(self.entries).min(first.saturating_add(MAX_ENTRIES_PER_READING));
            for i in first..end {
                if let Some(entry) = self.entry(i) {
                    f(len, entry);
                }
            }
        }
    }

    fn unknown_id(&self) -> u16 {
        self.unknown_id
    }
}

impl Connection for JaAsset {
    /// Row `rid`, column `lid`: the row's default unless the bitmap marks an
    /// exception, whose index is the row's exception start plus the set bits
    /// before it.
    fn cost(&self, rid: u16, lid: u16) -> i32 {
        const UNREACHABLE: i32 = i16::MAX as i32;
        let c = &self.conn;
        let (r, l) = (rid as usize, lid as usize);
        if r >= c.n || l >= c.n {
            return UNREACHABLE;
        }
        let b = self.bytes();
        let (word, bit) = (l / 64, l % 64);
        let Some(bits) = u64_at(b, c.bitmap + (r * c.words + word) * 8) else {
            return UNREACHABLE;
        };
        if bits >> bit & 1 == 0 {
            return u16_at(b, c.defaults + r * 2).map_or(UNREACHABLE, |v| v as i16 as i32);
        }
        let before = (bits & ((1u64 << bit) - 1)).count_ones() as usize;
        // Checked: the indices come from the file and usize is 32-bit on wasm.
        let index = u32_at(b, c.exc_start + r * 4)
            .zip(u16_at(b, c.rank + (r * c.words + word) * 2))
            .and_then(|(s, rank)| (s as usize).checked_add(rank as usize)?.checked_add(before));
        index
            .and_then(|i| c.exceptions.start.checked_add(i.checked_mul(2)?))
            .filter(|&at| at < c.exceptions.end && c.exceptions.end - at >= 2)
            .and_then(|at| u16_at(b, at))
            .map_or(UNREACHABLE, |v| v as i16 as i32)
    }
}
