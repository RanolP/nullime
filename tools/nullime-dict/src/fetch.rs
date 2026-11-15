//! Downloads the pinned Mozc OSS dictionary files with the system `curl`
//! (shipped with Windows 10+, macOS and most Linux distributions), so the
//! tool needs no HTTP dependency.

use std::path::Path;
use std::process::Command;

use crate::MOZC_COMMIT;

pub const FILES: &[&str] = &[
    "dictionary00.txt",
    "dictionary01.txt",
    "dictionary02.txt",
    "dictionary03.txt",
    "dictionary04.txt",
    "dictionary05.txt",
    "dictionary06.txt",
    "dictionary07.txt",
    "dictionary08.txt",
    "dictionary09.txt",
    "suffix.txt",
    "connection_single_column.txt",
    "id.def",
    "README.txt",
    "evaluation.tsv",
];

/// Mozc's own BSD-3 license sits at the repository root, not in `dictionary_oss/`.
pub const ROOT_FILES: &[&str] = &["LICENSE"];

pub fn run(out: &Path) -> Result<(), String> {
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let paths = FILES.iter().map(|f| (*f, "src/data/dictionary_oss/")).chain(ROOT_FILES.iter().map(|f| (*f, "")));
    for (file, dir) in paths {
        let url = format!("https://raw.githubusercontent.com/google/mozc/{MOZC_COMMIT}/{dir}{file}");
        let dest = out.join(file);
        eprintln!("fetch {url}");
        let status = Command::new("curl")
            .args(["-sSfL", "--retry", "2", "-o"])
            .arg(&dest)
            .arg(&url)
            .status()
            .map_err(|e| format!("running curl: {e}"))?;
        if !status.success() {
            return Err(format!("curl failed ({status}) for {url}"));
        }
    }
    eprintln!("fetched {} files into {}", FILES.len() + ROOT_FILES.len(), out.display());
    Ok(())
}
