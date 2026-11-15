//! Conversion accuracy (D19): readings go straight into
//! `ja_convert::convert` and the top-1 surface is compared with the expected
//! one. Neither data set is in the repository; pass their paths.
//!
//! Mozc `src/data/dictionary_oss/evaluation.tsv`: columns status, input,
//! output, command, argument, version. Rows whose command is exactly
//! `Conversion Expected` mean "the top conversion of input is argument" and
//! give Acc@1; `Conversion Expected k` rows (argument within the top k) are
//! reported as hit@k.
//!
//! AJIMEE-Bench `JWTD_v2/v1/evaluation_items.json`: an array of
//! {input (katakana), expected_output [accepted surfaces], context_text, ...}.
//! A hit is a top-1 equal to any accepted surface. `context_text` (left
//! context, half of the items) is ignored: the engine has no context model yet.

use std::path::Path;

use nullime_engine::ja_asset::JaAsset;
use nullime_engine::ja_convert::convert;

fn top1(asset: &JaAsset, reading: &str) -> String {
    convert(asset, reading, 1).into_iter().next().map(|c| c.surface).unwrap_or_default()
}

fn pct(hits: usize, cases: usize) -> f64 {
    if cases == 0 {
        0.0
    } else {
        100.0 * hits as f64 / cases as f64
    }
}

fn katakana_to_hiragana(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{30A1}'..='\u{30F6}' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
            _ => c,
        })
        .collect()
}

pub fn run(asset: &Path, tsv: Option<&Path>, ajimee: Option<&Path>) -> Result<(), String> {
    let name = asset.display().to_string();
    let asset = crate::load(asset)?;
    println!("asset\t{name}\t{} entries", asset.entry_count());
    if let Some(tsv) = tsv {
        mozc(&asset, tsv)?;
    }
    if let Some(ajimee) = ajimee {
        ajimee_bench(&asset, ajimee)?;
    }
    Ok(())
}

fn mozc(asset: &JaAsset, tsv: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(tsv).map_err(|e| format!("{}: {e}", tsv.display()))?;
    let (mut cases, mut hits, mut k_cases, mut k_hits) = (0, 0, 0, 0);
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split('\t').collect();
        let [_, input, _, command, expected, ..] = f[..] else { continue };
        if command == "Conversion Expected" {
            cases += 1;
            let got = top1(asset, input);
            if got == expected {
                hits += 1;
            } else {
                println!("miss\tmozc\t{input}\texpected {expected}\tgot {got}");
            }
        } else if let Some(k) = command.strip_prefix("Conversion Expected ").and_then(|k| k.parse::<usize>().ok()) {
            k_cases += 1;
            if convert(asset, input, k).iter().any(|c| c.surface == expected) {
                k_hits += 1;
            }
        }
    }
    println!("mozc\t{}\tacc@1 {hits}/{cases} = {:.1}%", tsv.display(), pct(hits, cases));
    println!("mozc\t{}\thit@k {k_hits}/{k_cases} = {:.1}%", tsv.display(), pct(k_hits, k_cases));
    Ok(())
}

fn ajimee_bench(asset: &JaAsset, path: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let items: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let items = items.as_array().ok_or("AJIMEE: expected a JSON array")?;
    let (mut cases, mut hits) = (0, 0);
    for item in items {
        let (Some(input), Some(expected)) = (item["input"].as_str(), item["expected_output"].as_array()) else {
            return Err(format!("AJIMEE: item without input/expected_output: {item}"));
        };
        cases += 1;
        let got = top1(asset, &katakana_to_hiragana(input));
        if expected.iter().any(|e| e.as_str() == Some(got.as_str())) {
            hits += 1;
        } else {
            println!("miss\tajimee\t{input}\texpected {}\tgot {got}", expected[0]);
        }
    }
    println!("ajimee\t{}\tacc@1 {hits}/{cases} = {:.1}% (context ignored)", path.display(), pct(hits, cases));
    Ok(())
}
