//! `bench-step`: a STEP assembly with the structure of a large printer assembly, generated here,
//! then opened as `bench-open` opens a file.
//!
//! The Prusa Core One assembly (#2814) is 61 MB, 189 distinct parts placed 561 times, most of
//! them small screws, nuts and brackets that appear many times. The file cannot be committed, so
//! the shape is rebuilt: distinct parts (a block with a row of through holes and a boss, every one a
//! different size), each placed several times by an assembly with rigid placements.
//!
//! ```text
//! solidcraft-cli bench-step [--parts N] [--instances K] [--holes H] [--out part.step] [--generate-only]
//! ```
//! writes the file (default: the temporary folder), then prints the phases `bench-open` prints
//! for it, with `--display normal`.

use std::time::Instant;

use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_engine::kernel::{Body, BoolOp, ExportBody, ExportProduct, StepHeader, boolean, box_solid, cylinder, step_export_products};

/// One part: a block with a hole through it and a boss on top, sized by its number so that no
/// two parts are the same item.
fn part(k: usize, holes: usize) -> Result<Body, String> {
    let f = k as f64;
    let (w, d, h) = (20.0 + f % 7.0 * 3.0, 14.0 + f % 5.0 * 2.0, 6.0 + f % 3.0 * 2.0);
    let block = box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(w, d, h)).map_err(|e| e.to_string())?;
    // A row of holes, as a bracket or a plate has them: many curved faces per part.
    let mut drilled = block;
    for i in 0..holes {
        let x = w * (i as f64 + 0.5) / holes as f64;
        let hole =
            cylinder(Vec3::new(x, d / 2.0, -1.0), Vec3::Z, (w / holes as f64 * 0.3).min(2.0 + f % 4.0 * 0.5), h + 2.0).map_err(|e| e.to_string())?;
        drilled = boolean(&drilled, &hole, BoolOp::Cut).map_err(|e| e.to_string())?.ok_or("a hole removed the block")?;
    }
    let boss = cylinder(Vec3::new(w / 4.0, d / 4.0, h), Vec3::Z, 1.5, 3.0 + f % 4.0).map_err(|e| e.to_string())?;
    boolean(&drilled, &boss, BoolOp::Union).map_err(|e| e.to_string())?.ok_or_else(|| "the boss joined to nothing".to_string())
}

/// Rigid placement, column-major 4x4: turned about Z by `turn`, moved to `at`.
fn placement(turn: f64, at: [f64; 3]) -> [[f64; 4]; 4] {
    let (s, c) = turn.sin_cos();
    [[c, s, 0.0, 0.0], [-s, c, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [at[0], at[1], at[2], 1.0]]
}

pub(crate) fn run(args: &[String]) -> Result<(), String> {
    let number = |name: &str, default: usize| -> Result<usize, String> {
        match args.iter().position(|a| a == name) {
            None => Ok(default),
            Some(i) => {
                args.get(i + 1).and_then(|v| v.parse().ok()).filter(|n| (1..=100_000).contains(n)).ok_or(format!("{name}: a number from 1 to 100000"))
            }
        }
    };
    let (parts, instances, holes) = (number("--parts", 189)?, number("--instances", 3)?, number("--holes", 4)?);
    let t = Instant::now();
    let bodies = (0..parts).map(|k| part(k, holes)).collect::<Result<Vec<_>, _>>()?;
    let mut products: Vec<ExportProduct> = bodies
        .iter()
        .enumerate()
        .map(|(k, body)| ExportProduct {
            name: format!("Part {k}"),
            bodies: vec![ExportBody { name: format!("Part {k}"), body, color: None }],
            children: Vec::new(),
        })
        .collect();
    let children = (0..parts * instances)
        .map(|n| {
            (n % parts, placement(n as f64 * 0.7, [(n % 40) as f64 * 60.0, (n / 40) as f64 * 60.0, 0.0]), format!("Part {}:{}", n % parts, n / parts))
        })
        .collect();
    products.push(ExportProduct { name: "Assembly".into(), bodies: Vec::new(), children });
    let header = StepHeader { file_name: "bench-step.step".into(), ..StepHeader::default() };
    let text = step_export_products(&products, parts, &header).map_err(|e| e.to_string())?;
    let path = match args.iter().position(|a| a == "--out").and_then(|i| args.get(i + 1)) {
        Some(p) => p.clone(),
        None => std::env::temp_dir().join(format!("solidcraft-bench-step-{}.step", std::process::id())).to_string_lossy().into_owned(),
    };
    std::fs::write(&path, &text).map_err(|e| format!("{path}: {e}"))?;
    println!(
        "{}",
        json!({"step": "generate", "ms": t.elapsed().as_secs_f64() * 1000.0, "parts": parts, "placements": parts * instances, "bytes": text.len(), "path": path})
    );
    if args.iter().any(|a| a == "--generate-only") {
        return Ok(());
    }
    let result = crate::cmd_bench_open(&[path.clone(), "--display".into(), "normal".into()]);
    if !args.iter().any(|a| a == "--out") {
        let _ = std::fs::remove_file(&path);
    }
    result
}
