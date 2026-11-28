//! Every body the naming corpus's designs build, through the exact exchange and back
//! (`solidcraft_kernel::exact`), against the same body through STEP text (solidcore's writer,
//! then its reader): how many come back with the faces, edges, volume and area they had, how
//! many each way refuses, and the time each takes.
//!
//! `cargo run --release -p solidcraft-cli --example exact_corpus` (the corpus of
//! `tests/naming_corpus.rs`, without the oracle recipes, which need the CLI binary).

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{Value, json};
use solidcraft_engine::Session;
use solidcraft_kernel::{Body, exact, measure_mass, step_export, step_import};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn designs() -> Vec<(String, PathBuf)> {
    let root = root();
    let mut out = Vec::new();
    let mut add = |prefix: &str, dir: PathBuf, ext: &str| {
        let Ok(rd) = std::fs::read_dir(&dir) else { return };
        let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == ext)).collect();
        v.sort();
        for p in v {
            let stem = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
            out.push((format!("{prefix}/{stem}"), p));
        }
    };
    add("corpus", root.join("apps/solidcraft-cli/tests/corpus"), "solidcraft");
    add("examples", root.join("examples"), "json");
    add("examples", root.join("examples/acceptance"), "json");
    add("regressions", root.join("examples/regressions"), "json");
    add("demo", root.join("examples/demo"), "json");
    add("typical", root.join("tests/action_matrix/designs"), "json");
    add("demo", root.join("tests/action_matrix/designs"), solidcraft_engine::io::DESIGN_EXT);
    add("fixtures", root.join("crates/engine/src/tests/fixtures"), solidcraft_engine::io::DESIGN_EXT);
    add("hole_matrix", root.join("tests/hole_matrix/old-0.9"), "json");
    add("taper", root.join("tests/taper/before-1014"), "json");
    add("projection", root.join("tests/projection/before-1508"), "json");
    out
}

fn load(path: &Path) -> Result<Session, String> {
    let mut s = Session::default();
    if solidcraft_engine::io::is_design_container_path(path) {
        s.execute("doc.open", &json!({"path": path.to_string_lossy()})).map_err(|e| e.to_string())?;
        return Ok(s);
    }
    let v: Value = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    if v.get("features").is_some() {
        s.execute("doc.open", &json!({"path": path.to_string_lossy()})).map_err(|e| e.to_string())?;
        return Ok(s);
    }
    let script = match v.get("steps").and_then(Value::as_array) {
        Some(steps) => json!({"commands": steps.iter().map(|st| json!({"command": st["command"], "params": st["params"]})).collect::<Vec<_>>()}),
        None => v,
    };
    s.run_script(&script).map_err(|e| e.to_string())?;
    if s.active_sketch.is_some() {
        let _ = s.execute("sketch.finish", &json!({}));
    }
    Ok(s)
}

/// Whether `back` is `body` again: its faces and edges, and its volume and area to 1e-9.
fn same(body: &Body, back: &Body) -> Result<(), String> {
    if (back.face_count(), back.edge_count()) != (body.face_count(), body.edge_count()) {
        return Err(format!("{} faces and {} edges back as {} and {}", body.face_count(), body.edge_count(), back.face_count(), back.edge_count()));
    }
    let (a, b) = (measure_mass(body).map_err(|e| e.to_string())?, measure_mass(back).map_err(|e| e.to_string())?);
    let close = |x: f64, y: f64| (x - y).abs() <= 1e-9 * x.abs().max(1.0);
    if !close(a.volume, b.volume) || !close(a.area, b.area) {
        return Err(format!("volume {} and area {} back as {} and {}", a.volume, a.area, b.volume, b.area));
    }
    Ok(())
}

#[derive(Default)]
struct Tally {
    same: usize,
    differ: Vec<String>,
    refused: Vec<String>,
    seconds: f64,
}

fn main() {
    let (mut exact_tally, mut step_tally) = (Tally::default(), Tally::default());
    let (mut designs_read, mut bodies) = (0, 0);
    for (name, path) in designs() {
        let session = match load(&path) {
            Ok(s) => s,
            Err(e) => {
                println!("{name}: does not load: {e}");
                continue;
            }
        };
        designs_read += 1;
        for (k, mb) in session.model.state().bodies.iter().enumerate() {
            let body = &mb.body;
            if body.is_mesh() || body.face_count() == 0 {
                continue;
            }
            bodies += 1;
            let what = format!("{name} body {k}");
            let start = Instant::now();
            let back = exact::exact_exported(body).map(|mut ex| {
                ex.history = None;
                ex
            });
            let back = back.and_then(|ex| exact::body_from_exact(ex, &[]));
            exact_tally.seconds += start.elapsed().as_secs_f64();
            match back {
                Ok(back) => match same(body, &back) {
                    Ok(()) => exact_tally.same += 1,
                    Err(e) => exact_tally.differ.push(format!("{what}: {e}")),
                },
                Err(e) => exact_tally.refused.push(format!("{what}: {e}")),
            }
            let start = Instant::now();
            let back = step_export(&[body], "exact corpus").and_then(|text| step_import(&text)).map_err(|e| e.to_string()).and_then(|i| {
                if i.bodies.len() == 1 {
                    i.bodies.into_iter().next().map(|b| b.body).ok_or_else(|| "no body".into())
                } else {
                    Err(format!("{} bodies", i.bodies.len()))
                }
            });
            step_tally.seconds += start.elapsed().as_secs_f64();
            match back {
                Ok(back) => match same(body, &back) {
                    Ok(()) => step_tally.same += 1,
                    Err(e) => step_tally.differ.push(format!("{what}: {e}")),
                },
                Err(e) => step_tally.refused.push(format!("{what}: {e}")),
            }
        }
    }
    println!("{designs_read} designs, {bodies} bodies");
    for (way, t) in [("exact", &exact_tally), ("STEP", &step_tally)] {
        println!("{way}: {} the same, {} differ, {} refused, {:.2} s", t.same, t.differ.len(), t.refused.len(), t.seconds);
        for d in &t.differ {
            println!("  {way} differs: {d}");
        }
        for r in &t.refused {
            println!("  {way} refused: {r}");
        }
    }
}
