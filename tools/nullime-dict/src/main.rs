//! Builds and checks `nullime-ja.dict`, the external Japanese conversion
//! asset (D15). Not part of the engine; see docs/architecture.md
//! "일본어 사전 애셋".

mod bench;
mod build;
mod eval;
mod fetch;

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;

/// The Mozc commit whose `src/data/dictionary_oss` the asset is built from.
pub const MOZC_COMMIT: &str = "a4541df29db443b49f81cf54665873b68fde9efa";

const USAGE: &str = "usage:
  nullime-dict fetch --out <dir>
  nullime-dict build --mozc <dir> --out <file> [--max-cost <n>]
  nullime-dict eval  --asset <file> [--tsv <evaluation.tsv>] [--ajimee <evaluation_items.json>] [--mixed <mixed.tsv>]
  nullime-dict bench --asset <file>";

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else { return usage() };
    let mut flags: HashMap<String, String> = HashMap::new();
    while let Some(flag) = args.next() {
        let Some(name) = flag.strip_prefix("--") else { return usage() };
        let Some(value) = args.next() else { return usage() };
        flags.insert(name.to_string(), value);
    }
    let path = |name: &str| flags.get(name).map(PathBuf::from);
    let result = match command.as_str() {
        "fetch" => match path("out") {
            Some(out) => fetch::run(&out),
            None => return usage(),
        },
        "build" => {
            let (Some(mozc), Some(out)) = (path("mozc"), path("out")) else { return usage() };
            let max_cost = match flags.get("max-cost").map(|v| v.parse::<i32>()) {
                Some(Ok(v)) => Some(v),
                Some(Err(_)) => return usage(),
                None => None,
            };
            build::run(&mozc, &out, build::Trim { max_cost })
        }
        "eval" => match path("asset") {
            Some(asset) if ["tsv", "ajimee", "mixed"].iter().any(|f| flags.contains_key(*f)) => {
                eval::run(&asset, path("tsv").as_deref(), path("ajimee").as_deref(), path("mixed").as_deref())
            }
            _ => return usage(),
        },
        "bench" => match path("asset") {
            Some(asset) => bench::run(&asset),
            None => return usage(),
        },
        _ => return usage(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

pub fn load(path: &std::path::Path) -> Result<nullime_engine::ja_asset::JaAsset, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    nullime_engine::ja_asset::JaAsset::new(bytes).map_err(|e| format!("{}: {e}", path.display()))
}
