//! `nullime-ja.dict` next to the DLL, memory-mapped once per process and
//! shared by every text service (docs/architecture.md "일본어 사전 애셋").

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use nullime_engine::ja_asset::JaAsset;
use nullime_engine::Engine;
use windows::Win32::Storage::FileSystem::FILE_SHARE_READ;

use crate::log;
use crate::registration::dll_path;

const FILE_NAME: &str = "nullime-ja.dict";

static ASSET: OnceLock<Option<JaAsset>> = OnceLock::new();

struct MappedFile(memmap2::Mmap);

impl AsRef<[u8]> for MappedFile {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

// SAFETY: a read-only map keeps its address and length for its life, and its
// contents cannot change: `load` opens the file sharing read access only, so
// Windows refuses every open for writing or deleting while the handle lives,
// and memmap2 keeps a duplicate of that handle for as long as the map.
unsafe impl nullime_engine::ja_asset::StableBytes for MappedFile {}

/// An engine using the mapped dictionary, or the built-in sample when it is
/// missing or invalid. Never fails; the reason is logged once.
pub fn engine() -> Engine {
    match ASSET.get_or_init(load) {
        Some(asset) => Engine::with_ja_dictionary(asset.clone()),
        None => Engine::new(),
    }
}

fn path() -> Option<PathBuf> {
    let dll = PathBuf::from(OsString::from_wide(&dll_path().ok()?));
    Some(dll.parent()?.join(FILE_NAME))
}

fn load() -> Option<JaAsset> {
    let Some(path) = path() else {
        log("dictionary: DLL path unknown; using the sample dictionary");
        return None;
    };
    let result = map(&path)
        .map_err(|e| e.to_string())
        .and_then(|map| JaAsset::new(map).map_err(|e| e.to_string()));
    match result {
        Ok(asset) => {
            log(&format!("dictionary: mapped {} ({} entries)", path.display(), asset.entry_count()));
            Some(asset)
        }
        Err(e) => {
            log(&format!("dictionary: {}: {e}; using the sample dictionary", path.display()));
            None
        }
    }
}

/// Maps `path` sharing read access only, which is what makes `MappedFile`'s
/// `StableBytes` promise hold. Fails if another process already holds write
/// access to the file.
fn map(path: &Path) -> std::io::Result<MappedFile> {
    let file = std::fs::OpenOptions::new().read(true).share_mode(FILE_SHARE_READ.0).open(path)?;
    // SAFETY: see `MappedFile`.
    Ok(MappedFile(unsafe { memmap2::Mmap::map(&file) }?))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Catches a regression to a plain `File::open`, which shares write access
    // and lets another process change the trie under yada's unchecked reads;
    // also checks that the lock outlives the `File`, which `map` drops.
    #[test]
    fn mapped_file_refuses_writers_until_unmapped() {
        const ERROR_SHARING_VIOLATION: i32 = 32;
        let path = std::env::temp_dir().join(format!("nullime-share-{}.bin", std::process::id()));
        std::fs::write(&path, [1u8; 64]).unwrap();
        let mapped = map(&path).unwrap();
        let writer = std::fs::OpenOptions::new().write(true).open(&path);
        assert_eq!(writer.err().and_then(|e| e.raw_os_error()), Some(ERROR_SHARING_VIOLATION));
        assert!(std::fs::remove_file(&path).is_err());
        drop(mapped);
        std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
    }
}
