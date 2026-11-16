//! `cargo run --example primitives`: builds a box, cylinder, cone, pyramid, sphere and
//! torus (with the sphere's no-pole and partial and the torus's partial variants) in pure
//! Rust and proves each is a closed manifold solid with the analytic Euler
//! characteristic, volume, area and bounding box. Exits non-zero if any check fails.
//!
//! `-- --json <path>` also writes the results as JSON (the progress site's Solids page;
//! CI uploads it as the `solids-report` artifact). Schema 1:
//!
//! ```text
//! { "schema": 1, "source": "crates/solidcore/examples/primitives", "rel_tol": 1e-9,
//!   "summary": { "total", "built", "passed" },
//!   "primitives": [ { "name", "built", "pass",
//!       "blocker": null | { "unit", "upstream", "stage": "build" | "measure" },
//!       "error": null | "<first error, Debug>",
//!       "analytic": { "volume", "area", "euler", "counts": null | [f, e, v],
//!                     "bbox": { "min": [x, y, z], "max": [x, y, z] } },
//!       "measured": null | { "faces", "edges", "vertices", "loops", "manifold_solid",
//!                            "volume", "area", "bbox" },     (each null if unmeasured)
//!       "checks": [ { "name", "pass", "expected", "actual", "rel_err", "message" } ] } ],
//!   "corpus": { "status": "pending", "note", "models": [
//!       { "name", "source", "built", "pass", "blocker", "error",
//!         "expected": { "volume", "area", "center_of_mass": [x, y, z] },
//!         "actual":   { "volume", "area", "center_of_mass": [x, y, z] },
//!         "rel_err":  { "volume", "area", "center_of_mass" } } ] } }
//! ```
//!
//! `corpus.models` stays empty until a corpus of real models is chosen.
//!
//! `-- --wire <path>` writes the wireframe of every primitive that builds, for drawing
//! pictures of the solids: each edge's 3D curve sampled at 48 parameters across its
//! interval (edges whose curve fails to evaluate are left out):
//!
//! ```text
//! [ { "name", "edges": [ [ [x, y, z], ... ], ... ] }, ... ]
//! ```
//!
//! No STL is written: upstream's open SMLib has no B-rep face tessellation to a mesh
//! (only `SmApiTessellateBoundaries`), so there is nothing faithful to export.

mod checks;

use checks::{Evaluation, Spec, Value};
use solidcore::math::Point3;
use std::fmt::Write as _;

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut json_path = None;
    let mut wire_path = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--json" => json_path = Some(args.next().ok_or("--json needs a path")?),
            "--wire" => wire_path = Some(args.next().ok_or("--wire needs a path")?),
            _ => {
                return Err(format!(
                    "unknown argument {a:?} (usage: [--json <path>] [--wire <path>])"
                ))
            }
        }
    }
    let mut failed = 0;
    let mut results = Vec::new();
    for spec in checks::specs().into_iter().chain(checks::curved_specs()) {
        println!("== {}", spec.name);
        println!(
            "   analytic: volume {:.12} area {:.12} bbox {:?}",
            spec.volume, spec.area, spec.bbox
        );
        let ev = checks::evaluate(&spec);
        match checks::report(&ev) {
            Ok(r) => {
                let (f, e, v, l) = r.counts;
                println!(
                    "   F {f} E {e} V {v} L {l}  V-E+F = {}",
                    v as i64 - e as i64 + f as i64
                );
                println!("   manifold solid: {}", r.manifold_solid);
                println!("   volume {:.12} area {:.12}", r.volume, r.area);
                println!("   bbox {:?}", r.bbox);
                println!("   OK");
            }
            Err(msg) => {
                failed += 1;
                println!("   FAIL: {msg}");
            }
        }
        results.push((spec, ev));
    }
    println!("STL: not written (no B-rep mesh tessellation in upstream SMLib)");
    if let Some(path) = json_path {
        write_file(&path, &to_json(&results))?;
        println!("JSON: {path}");
    }
    if let Some(path) = wire_path {
        let mut wires = Vec::new();
        for (spec, _) in &results {
            if let Some(w) = checks::wireframe(spec, WIRE_SAMPLES) {
                wires.push(wire_json(spec.name, &w));
            }
        }
        write_file(&path, &format!("[\n{}\n]\n", wires.join(",\n")))?;
        println!("wireframe: {path}");
    }
    if failed == 0 {
        Ok(())
    } else {
        Err(format!("{failed} primitive(s) failed"))
    }
}

/// Parameters sampled along each edge by `--wire`.
const WIRE_SAMPLES: usize = 48;

/// Writes `text` to `path`, creating its directory.
fn write_file(path: &str, text: &str) -> Result<(), String> {
    let p = std::path::Path::new(path);
    if let Some(dir) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(p, text).map_err(|e| format!("{path}: {e}"))
}

/// One primitive's `--wire` entry.
fn wire_json(name: &str, edges: &[Vec<Point3>]) -> String {
    let edges: Vec<String> = edges
        .iter()
        .map(|e| {
            let pts: Vec<String> = e.iter().map(|p| point(*p)).collect();
            format!("[{}]", pts.join(", "))
        })
        .collect();
    format!(
        "  {{\"name\": {}, \"edges\": [\n    {}\n  ]}}",
        string(name),
        edges.join(",\n    ")
    )
}

fn num(x: f64) -> String {
    if x.is_finite() {
        format!("{x:?}")
    } else {
        "null".to_string()
    }
}

fn string(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn point(p: Point3) -> String {
    format!("[{}, {}, {}]", num(p.x), num(p.y), num(p.z))
}

fn bbox(min: Point3, max: Point3) -> String {
    format!("{{\"min\": {}, \"max\": {}}}", point(min), point(max))
}

fn value(v: Value) -> String {
    match v {
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Num(x) => num(x),
        Value::Counts(f, e, v) => format!("[{f}, {e}, {v}]"),
        Value::BBox(a, b) => bbox(a, b),
    }
}

fn opt<T>(v: Option<T>, f: impl Fn(T) -> String) -> String {
    v.map_or_else(|| "null".to_string(), f)
}

fn primitive(spec: &Spec, ev: &Evaluation) -> String {
    let blocker = opt(ev.blocker(), |(unit, upstream)| {
        let stage = if ev.build_error.is_some() {
            "build"
        } else {
            "measure"
        };
        format!(
            "{{\"unit\": {}, \"upstream\": {}, \"stage\": \"{stage}\"}}",
            string(unit),
            string(upstream)
        )
    });
    let error = opt(ev.first_error(), |e| string(&format!("{e:?}")));
    let analytic = format!(
        "{{\"volume\": {}, \"area\": {}, \"euler\": {}, \"counts\": {}, \"bbox\": {}}}",
        num(spec.volume),
        num(spec.area),
        spec.euler,
        opt(spec.counts, |(f, e, v)| format!("[{f}, {e}, {v}]")),
        bbox(spec.bbox.0, spec.bbox.1)
    );
    let measured = opt(ev.measured.as_ref(), |m| {
        let c = m.counts.as_ref().ok();
        format!(
            "{{\"faces\": {}, \"edges\": {}, \"vertices\": {}, \"loops\": {}, \
             \"manifold_solid\": {}, \"volume\": {}, \"area\": {}, \"bbox\": {}}}",
            opt(c, |c| c.0.to_string()),
            opt(c, |c| c.1.to_string()),
            opt(c, |c| c.2.to_string()),
            opt(c, |c| c.3.to_string()),
            opt(m.manifold_solid.as_ref().ok(), bool::to_string),
            opt(m.volume.as_ref().ok(), |x| num(*x)),
            opt(m.area.as_ref().ok(), |x| num(*x)),
            opt(m.bbox.as_ref().ok(), |b| bbox(b.0, b.1))
        )
    });
    let checks: Vec<String> = ev
        .checks
        .iter()
        .map(|c| {
            format!(
                "{{\"name\": {}, \"pass\": {}, \"expected\": {}, \"actual\": {}, \
                 \"rel_err\": {}, \"message\": {}}}",
                string(c.name),
                c.pass,
                value(c.expected),
                opt(c.actual, value),
                opt(c.rel_err, num),
                string(&c.message)
            )
        })
        .collect();
    format!(
        "    {{\"name\": {}, \"built\": {}, \"pass\": {}, \"blocker\": {blocker}, \
         \"error\": {error},\n     \"analytic\": {analytic},\n     \"measured\": {measured},\n     \
         \"checks\": [{}]}}",
        string(spec.name),
        ev.build_error.is_none(),
        ev.pass(),
        checks.join(", ")
    )
}

fn to_json(results: &[(Spec, Evaluation)]) -> String {
    let built = results
        .iter()
        .filter(|(_, e)| e.build_error.is_none())
        .count();
    let passed = results.iter().filter(|(_, e)| e.pass()).count();
    let items: Vec<String> = results.iter().map(|(s, e)| primitive(s, e)).collect();
    format!(
        "{{\n  \"schema\": 1,\n  \"source\": \"crates/solidcore/examples/primitives\",\n  \
         \"rel_tol\": {},\n  \"summary\": {{\"total\": {}, \"built\": {built}, \"passed\": {passed}}},\n  \
         \"primitives\": [\n{}\n  ],\n  \
         \"corpus\": {{\"status\": \"pending\", \"note\": \"waiting on the choice of a corpus of real models\", \
         \"models\": []}}\n}}\n",
        num(checks::REL_TOL),
        results.len(),
        items.join(",\n")
    )
}
