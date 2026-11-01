//! The file-pcurve gate: every body of a STEP corpus built without and with
//! [`BuildOptions::file_pcurves`], compared.
//!
//! Ignored by default; the corpus is not in the repository. Run it with the directory
//! of `.stp` / `.step` files (read in place, recursively) in `SOLIDCORE_STEP_CORPUS`:
//!
//! ```sh
//! SOLIDCORE_STEP_CORPUS=/path/to/corpus cargo test --release -p step-brep \
//!     --test file_pcurves_corpus -- --ignored --nocapture
//! ```
//!
//! Each body prints one line per build: topology counts, manifold solid, display
//! validity, Level-2 check failures, area and volume, and the pcurve statistics. The
//! test fails when the pcurve build is worse than the plain one in topology, manifold,
//! display validity or Level-2 failures, or fails where the plain build succeeds. Area
//! and volume differences above 1e-9 relative are listed for review, not failed.

use std::path::{Path, PathBuf};

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::Kernel;
use solidcore::mass::integrator::properties::{brep_area, brep_volume, PropertyAttributes};
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::Brep;
use step_brep::{
    build_with_file_pcurves, validate_for_display, BuildMode, BuildOptions, PcurveStats,
};

/// Stitched builds only (the file's pcurves are used on the stitched build; the
/// topology-first build would hide them), with or without the file's pcurves.
fn stitched(file_pcurves: bool) -> BuildOptions {
    BuildOptions {
        file_pcurves,
        mode: BuildMode::StitchOnly,
        ..BuildOptions::default()
    }
}

/// What one build of one body gave.
#[derive(Debug, Clone, PartialEq)]
struct Outcome {
    counts: (usize, usize, usize),
    manifold: Option<bool>,
    display: bool,
    level2_failures: Option<usize>,
    area: Option<f64>,
    volume: Option<f64>,
}

fn measure(brep: &Brep) -> Outcome {
    let kernel = Kernel::default();
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let mut copy = brep.clone();
    let area = brep_area(&kernel, &mut copy, 1e-9).ok();
    let mut copy = brep.clone();
    let volume = brep_volume(&kernel, &mut copy, 1e-9, &attrs).ok();
    let mut copy = brep.clone();
    let mut check_kernel = Kernel::default();
    let level2_failures = check_brep(&mut check_kernel, &mut copy, CheckLevel::Level2)
        .ok()
        .map(|check| check.failures().count());
    Outcome {
        counts: (
            brep.face_list().len(),
            brep.edge_list().len(),
            brep.vertex_list().len(),
        ),
        manifold: is_manifold_solid(brep).ok(),
        display: validate_for_display(brep).is_ok(),
        level2_failures,
        area,
        volume,
    }
}

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths = Vec::new();
    for entry in entries.flatten() {
        paths.push(entry.path());
    }
    paths.sort();
    for path in paths {
        if path.is_dir() {
            files(&path, out);
        } else if path
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "stp" | "step"))
        {
            out.push(path);
        }
    }
}

fn relative(a: f64, b: f64) -> f64 {
    (a - b).abs() / a.abs().max(b.abs()).max(f64::MIN_POSITIVE)
}

/// Why `on` is worse than `off`, if it is.
fn regressions(off: &Outcome, on: &Outcome) -> Vec<String> {
    let mut out = Vec::new();
    if on.counts != off.counts {
        out.push(format!("topology {:?} -> {:?}", off.counts, on.counts));
    }
    if off.manifold == Some(true) && on.manifold != Some(true) {
        out.push(format!("manifold {:?} -> {:?}", off.manifold, on.manifold));
    }
    if off.display && !on.display {
        out.push("display valid -> invalid".into());
    }
    match (off.level2_failures, on.level2_failures) {
        (Some(a), Some(b)) if b > a => out.push(format!("Level-2 failures {a} -> {b}")),
        (Some(a), None) => out.push(format!("Level-2 failures {a} -> check error")),
        _ => {}
    }
    out
}

#[test]
#[ignore = "needs SOLIDCORE_STEP_CORPUS"]
fn file_pcurves_do_not_regress_the_corpus() {
    let Some(dir) = std::env::var_os("SOLIDCORE_STEP_CORPUS") else {
        eprintln!("SOLIDCORE_STEP_CORPUS is not set");
        return;
    };
    let mut paths = Vec::new();
    files(Path::new(&dir), &mut paths);
    assert!(!paths.is_empty(), "no STEP files under {dir:?}");
    let mut failures = Vec::new();
    let mut total = PcurveStats::default();
    // (plane or B-spline surface faces, curved faces)
    let mut total_faces = (0usize, 0usize);
    for path in &paths {
        let name = path
            .strip_prefix(&dir)
            .unwrap_or(path)
            .display()
            .to_string();
        let Ok(bytes) = std::fs::read(path) else {
            println!("{name}: unreadable");
            continue;
        };
        let model = match step_import::import(&bytes, &step_import::Limits::default()) {
            Ok(model) => model,
            Err(error) => {
                println!("{name}: parse error {error}");
                continue;
            }
        };
        let mapped: usize = model.file_pcurves.edges.values().map(Vec::len).sum();
        let faces: usize = model.bodies.iter().map(|body| body.faces.len()).sum();
        let flat: usize = model
            .bodies
            .iter()
            .flat_map(|body| &body.faces)
            .filter(|face| {
                matches!(
                    face.surface,
                    step_import::model::Surface::Plane { .. }
                        | step_import::model::Surface::BSpline(_)
                )
            })
            .count();
        println!(
            "{name}: {} bodies, {mapped} pcurves mapped, {} not mapped; FACES {flat} plane or B-spline, {} curved",
            model.bodies.len(),
            model.file_pcurves.diagnostics.len(),
            faces - flat
        );
        total_faces.0 += flat;
        total_faces.1 += faces - flat;
        for (index, body) in model.bodies.iter().enumerate() {
            let off = build_with_file_pcurves(body, &model.file_pcurves, stitched(false));
            let on = build_with_file_pcurves(body, &model.file_pcurves, stitched(true));
            match (off, on) {
                (Ok((off, _)), Ok((on, stats))) => {
                    let (off, on) = (measure(&off), measure(&on));
                    println!("  body {index} off {off:?}");
                    println!("  body {index} on  {on:?} {stats:?}");
                    total.add(&stats);
                    for (what, a, b) in [
                        ("area", off.area, on.area),
                        ("volume", off.volume, on.volume),
                    ] {
                        if let (Some(a), Some(b)) = (a, b) {
                            if relative(a, b) > 1e-9 {
                                println!(
                                    "  CHANGED {name} body {index} {what} {a:.12e} -> {b:.12e} ({:.3e} relative)",
                                    relative(a, b)
                                );
                            }
                        } else if a.is_some() != b.is_some() {
                            println!("  CHANGED {name} body {index} {what} {a:?} -> {b:?}");
                        }
                    }
                    for regression in regressions(&off, &on) {
                        failures.push(format!("{name} body {index}: {regression}"));
                    }
                }
                (Ok(_), Err(error)) => {
                    failures.push(format!(
                        "{name} body {index}: build fails with pcurves: {error}"
                    ));
                }
                (Err(error), Ok(_)) => {
                    println!("  body {index} off fails ({error}), on builds");
                }
                (Err(off), Err(on)) => {
                    println!("  body {index} fails both ways: {off} / {on}");
                }
            }
        }
    }
    println!(
        "total faces: {} plane or B-spline, {} curved",
        total_faces.0, total_faces.1
    );
    println!("total {total:?}");
    assert!(failures.is_empty(), "regressions:\n{}", failures.join("\n"));
}

/// One body of `SOLIDCORE_STEP_FILE` (`SOLIDCORE_STEP_BODY`, default 0) built without
/// and with file pcurves: per face its surface kind and area both ways, and the Level-2
/// check lines only one build has.
#[test]
#[ignore = "needs SOLIDCORE_STEP_FILE"]
fn one_body_face_by_face() {
    let Some(path) = std::env::var_os("SOLIDCORE_STEP_FILE") else {
        eprintln!("SOLIDCORE_STEP_FILE is not set");
        return;
    };
    let index: usize = std::env::var("SOLIDCORE_STEP_BODY")
        .ok()
        .and_then(|b| b.parse().ok())
        .unwrap_or(0);
    let bytes = std::fs::read(&path).unwrap();
    let model = step_import::import(&bytes, &step_import::Limits::default()).unwrap();
    let body = &model.bodies[index];
    let build =
        |on: bool| build_with_file_pcurves(body, &model.file_pcurves, stitched(on)).unwrap();
    let (off, _) = build(false);
    let (on, stats) = build(true);
    println!("{stats:?}");
    let kernel = Kernel::default();
    let face_areas = |brep: &Brep| -> Vec<(String, Option<f64>)> {
        let mut copy = brep.clone();
        brep.face_list()
            .iter()
            .map(|&f| {
                let kind = solidcore::topology::face::p2::surface(brep, f)
                    .ok()
                    .flatten()
                    .map_or_else(
                        || "none".to_string(),
                        |s| format!("{:?}", s.kind()).chars().take(12).collect(),
                    );
                let area =
                    solidcore::mass::integrator::properties::face_area(&kernel, &mut copy, f, 1e-9)
                        .ok();
                (kind, area)
            })
            .collect()
    };
    let (a, b) = (face_areas(&off), face_areas(&on));
    for (i, ((kind, x), (_, y))) in a.iter().zip(&b).enumerate() {
        let mark = match (x, y) {
            (Some(x), Some(y)) if relative(*x, *y) > 1e-9 => " CHANGED",
            (Some(_), Some(_)) => "",
            _ => " MISSING",
        };
        println!("face {i} {kind}: {x:?} -> {y:?}{mark}");
    }
    let lines = |brep: &Brep| {
        let mut copy = brep.clone();
        let mut k = Kernel::default();
        check_brep(&mut k, &mut copy, CheckLevel::Level2)
            .map(|check| check.lines())
            .unwrap_or_default()
    };
    let (l0, l1) = (lines(&off), lines(&on));
    for line in l1.iter().filter(|l| !l0.contains(l)) {
        println!("ONLY WITH PCURVES: {line}");
    }
    for line in l0.iter().filter(|l| !l1.contains(l)) {
        println!("ONLY WITHOUT: {line}");
    }
}
