//! Imports every `.stp`/`.step` file under a directory and prints one line per file:
//! bodies, skipped bodies with reasons, and topology counts.
//!
//! `cargo run -p step-import --example survey -- corpus/steptools`

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            files(&p, out);
        } else if p
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("stp") || e.eq_ignore_ascii_case("step"))
        {
            out.push(p);
        }
    }
}

fn main() {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "corpus/steptools".into());
    let mut list = Vec::new();
    files(Path::new(&dir), &mut list);
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    for p in list {
        let bytes = std::fs::read(&p).unwrap_or_default();
        let name = p.strip_prefix(&dir).unwrap_or(&p).display().to_string();
        match step_import::import(&bytes, &step_import::Limits::default()) {
            Err(e) => println!("{name}: PARSE ERROR {e}"),
            Ok(m) => {
                let (f, e, v): (usize, usize, usize) = m.bodies.iter().fold((0, 0, 0), |a, b| {
                    (
                        a.0 + b.faces.len(),
                        a.1 + b.edges.len(),
                        a.2 + b.vertices.len(),
                    )
                });
                println!(
                    "{name}: bodies={} skipped={} faces={f} edges={e} vertices={v} instances={}",
                    m.bodies.len(),
                    m.skipped.len(),
                    m.assembly_instances
                );
                for s in &m.skipped {
                    println!("    #{} {}: {}", s.step_id, s.kind, s.reason);
                    *reasons.entry(s.reason.clone()).or_default() += 1;
                }
            }
        }
    }
    println!("-- skip reasons");
    for (r, n) in reasons {
        println!("{n:6} {r}");
    }
}
