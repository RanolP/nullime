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
//!
//! Mixed-language cases (`tools/nullime-dict/mixed.tsv`): unlike the two sets
//! above, these go through the whole `Engine`, so they measure which language
//! each span gets. Columns: keys to type (a space is the Space key, which
//! commits), then one or more accepted results. The result is the text an app
//! would hold afterwards. `## name` lines start a group.

use std::path::Path;

use nullime_engine::ja_asset::JaAsset;
use nullime_engine::ja_convert::convert;
use nullime_engine::{Engine, Key, KeyEvent};

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

pub fn run(asset: &Path, tsv: Option<&Path>, ajimee: Option<&Path>, mixed: Option<&Path>) -> Result<(), String> {
    let name = asset.display().to_string();
    let asset = crate::load(asset)?;
    println!("asset\t{name}\t{} entries", asset.entry_count());
    if let Some(tsv) = tsv {
        mozc(&asset, tsv)?;
    }
    if let Some(ajimee) = ajimee {
        ajimee_bench(&asset, ajimee)?;
    }
    if let Some(mixed) = mixed {
        mixed_bench(&asset, mixed)?;
    }
    Ok(())
}

/// Types `keys` into a fresh engine. Returns the text the app ends up with and
/// the segments shown before each commit, for miss reports.
fn type_line(asset: &JaAsset, keys: &str) -> (String, String) {
    let mut engine = Engine::with_ja_dictionary(asset.clone());
    let (mut text, mut shown, mut last) = (String::new(), String::new(), String::new());
    for b in keys.bytes() {
        let key = if b == b' ' { Key::Space } else { Key::Char(b) };
        let r = engine.key(KeyEvent::new(key));
        text += &r.commit;
        if r.preedit.is_some() {
            last = crate::bench::show(&r);
        }
        if key == Key::Space {
            shown += &std::mem::take(&mut last);
            shown.push(' ');
            if !r.handled {
                text.push(' ');
            }
        }
    }
    text += &engine.reset().commit;
    shown += &last;
    (text, shown)
}

fn mixed_bench(asset: &JaAsset, path: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    // (group, cases, hits) in file order.
    let mut groups: Vec<(String, usize, usize)> = Vec::new();
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("## ") {
            groups.push((name.to_string(), 0, 0));
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut f = line.split('\t');
        let keys = f.next().unwrap_or_default();
        let accepted: Vec<&str> = f.collect();
        if accepted.is_empty() {
            return Err(format!("{}: line without an expected result: {line}", path.display()));
        }
        if groups.is_empty() {
            groups.push((String::new(), 0, 0));
        }
        let group = groups.last_mut().expect("pushed above");
        group.1 += 1;
        let (got, shown) = type_line(asset, keys);
        if accepted.contains(&got.as_str()) {
            group.2 += 1;
        } else {
            println!("miss\tmixed\t{keys}\texpected {}\tgot {shown}", accepted[0]);
        }
    }
    for (name, cases, hits) in &groups {
        println!("mixed\t{name}\t{hits}/{cases} = {:.1}%", pct(*hits, *cases));
    }
    let (cases, hits) = groups.iter().fold((0, 0), |(c, h), g| (c + g.1, h + g.2));
    println!("mixed\t{}\tacc@1 {hits}/{cases} = {:.1}%", path.display(), pct(hits, cases));
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
