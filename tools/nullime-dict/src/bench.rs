//! Per-keystroke engine latency with the full asset, plus what the engine
//! shows for a few mixed-language probes (for scorer calibration).

use std::path::Path;
use std::time::Instant;

use nullime_engine::{Engine, Key, KeyEvent, Response};

const SENTENCES: &[&str] = &[
    "kyouhaiitenkidesune",    // 19 strokes: 今日はいい天気ですね
    "watashihagakuseidesu",   // 20 strokes: 私は学生です
    "watashihanihonjindesu",  // 21 strokes
    "kinouhatomodachitoeigawomimashita", // 33 strokes: over MAX_SPAN
];

const PROBES: &[&str] = &[
    "dkssud", "hello", "0.1mmwjdeh", "konnichiha", "sake", "arigatou", "toukyou", "gksrmf", "thank",
    "watashihanihonjindesu", "dkssudgktpdy", "nihongo", "kanji", "rust", "ok", "anime",
];

fn type_keys(engine: &mut Engine, s: &str) -> (Vec<f64>, Response) {
    let mut times = Vec::new();
    let mut last = None;
    for b in s.bytes() {
        let t = Instant::now();
        let r = engine.key(KeyEvent::new(Key::Char(b)));
        times.push(t.elapsed().as_secs_f64() * 1e6);
        last = Some(r);
    }
    (times, last.expect("non-empty input"))
}

pub fn show(r: &Response) -> String {
    let Some(p) = &r.preedit else { return String::new() };
    p.segments.iter().map(|s| format!("[{} {:?}]", s.text, s.lang)).collect::<Vec<_>>().join("")
}

pub fn run(asset: &Path) -> Result<(), String> {
    let t = Instant::now();
    let asset = crate::load(asset)?;
    println!("load\t{:.1} ms", t.elapsed().as_secs_f64() * 1e3);
    for s in SENTENCES {
        // Warm-up run, then the measured one.
        type_keys(&mut Engine::with_ja_dictionary(asset.clone()), s);
        let (times, r) = type_keys(&mut Engine::with_ja_dictionary(asset.clone()), s);
        let max = times.iter().cloned().fold(0.0, f64::max);
        let mean = times.iter().sum::<f64>() / times.len() as f64;
        println!("{s}\t{} strokes\tmean {mean:.0} us\tmax {max:.0} us\t{}", s.len(), show(&r));
        println!("  per-key us: {}", times.iter().map(|t| format!("{t:.0}")).collect::<Vec<_>>().join(" "));
    }
    for p in PROBES {
        let (_, r) = type_keys(&mut Engine::with_ja_dictionary(asset.clone()), p);
        println!("probe\t{p}\t{}", show(&r));
    }
    Ok(())
}
