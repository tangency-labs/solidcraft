//! SolidCraft's STEP export, byte for byte, and its round trip.
//!
//! Every STEP file in the repository's STEP fixtures (`tests/step-conformance`,
//! `tests/step-regressions`, the import kernel cases and solidcore-step's fixtures) is read
//! and written back through `step_export_bodies`. The written text's digest must be the one in
//! [`DIGESTS`]. These digests were taken from the export as it was before the AP242 product layer
//! moved into solidcore-step, so a passing run shows the move kept every byte. A small assembly
//! with names and a colour goes through `step_export_products` the same way.
//!
//! The written file is then read again: the same bodies, each with the same face count and its
//! volume within 1e-6 (relative) of the first read.
//!
//! `STEP_EXPORT_BYTES_PRINT=1` prints the table instead of checking it.

use solidcore_record::Vec3;
use solidcraft_kernel::{ExportBody, ExportProduct, StepHeader, box_solid, cylinder, measure, step_export_bodies, step_export_products, step_import};

/// FNV-1a, 64 bits: a stable digest of the written text.
fn digest(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// The fixtures, relative to the repository root, in sorted order.
fn fixtures(root: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut walk = vec![
        root.join("tests/step-conformance"),
        root.join("tests/step-regressions"),
        root.join("tests/kernel-cases/import"),
        root.join("solidcore/crates/solidcore-step/tests/fixtures"),
    ];
    while let Some(dir) = walk.pop() {
        for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk.push(path);
            } else if path.extension().is_some_and(|x| x.eq_ignore_ascii_case("step") || x.eq_ignore_ascii_case("stp")) {
                out.push(path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out.sort();
    out
}

/// (fixture, digest of its export) as written before the move, plus new fixtures.
#[rustfmt::skip]
const DIGESTS: &[(&str, &str)] = &[
    ("solidcore/crates/solidcore-step/tests/fixtures/closed-spline-cylinder.step", "197c80fea864545e"),
    ("solidcore/crates/solidcore-step/tests/fixtures/discord-2026-10-30-fillet.step", "5545d4ee93431803"),
    ("solidcore/crates/solidcore-step/tests/fixtures/rod_doubled_edge.step", "8432fa140e74c146"),
    ("tests/kernel-cases/import/1721-closed-bspline-tube-face/input.step", "c5176ac4d067ab52"),
    ("tests/kernel-cases/import/2043-outer-bound-opposing-surface-sense/input.step", "d8c5f6b3ce4ea3cf"),
    ("tests/kernel-cases/import/258-cone-point-vertex/input.step", "be8c91bead2f5a20"),
    ("tests/kernel-cases/import/258-cone-to-point/input.step", "112fb1ed128b8863"),
    ("tests/kernel-cases/import/258-tiny-pin/input.step", "7b4b8437f83a83a3"),
    ("tests/kernel-cases/import/3098-bspline-edges-over-tiny-knot-span/input.step", "b570b62268740b6a"),
    ("tests/kernel-cases/import/3208-extruded-ellipse-face-over-parameter-seam/input.step", "ec8784dcbe789ac8"),
    ("tests/kernel-cases/import/3352-vertex-off-its-edge-curve/input.step", "5e6454bed8d85bc3"),
    ("tests/kernel-cases/import/3730-void-oriented-shell-false/input.step", "e461e3732a479928"),
    ("tests/kernel-cases/import/3880-cylinder-seam-edge/input.step", "183935caa3bdda17"),
    ("tests/kernel-cases/import/3880-nurbs-hemisphere-dome/input.step", "c072f4d08cfefb04"),
    ("tests/kernel-cases/import/3963-closed-spline-edge-vertex-past-start/input.step", "c0e524535a1b5a62"),
    ("tests/kernel-cases/import/4005-cylinder-written-as-near-zero-cone/input.step", "6f13fdd4ff99c151"),
    ("tests/kernel-cases/import/4006-plane-written-as-cone-of-nearly-right-angle/input.step", "05c07f99a56d82c1"),
    ("tests/step-conformance/cone_point.step", "f8011a27d4b1fa9f"),
    ("tests/step-conformance/cone_point_vertex.step", "bc4c5fab88085533"),
    ("tests/step-conformance/cylinder.step", "e4e777124e084560"),
    ("tests/step-conformance/cylinder_inch.step", "11564eb93051bc70"),
    ("tests/step-conformance/cylinder_nurbs.step", "d2fa55692f333099"),
    ("tests/step-conformance/cylinder_placed.step", "54916a649b258555"),
    ("tests/step-conformance/cylinder_seam.step", "fba8ee481853a674"),
    ("tests/step-conformance/cylinder_twice.step", "0d40e99e9e7e5d6d"),
    ("tests/step-conformance/dome.step", "054949758cf68925"),
    ("tests/step-conformance/dome_nurbs.step", "9fdf321f816644ca"),
    ("tests/step-conformance/far_cylinder.step", "dcaaaadcc9f0ef04"),
    ("tests/step-conformance/grooved_shaft.step", "83cb26c7776bcf55"),
    ("tests/step-conformance/plate_100_holes.step", "414bd143a34716e6"),
    ("tests/step-conformance/sheet_disc.step", "a88c4d5661255edc"),
    ("tests/step-conformance/tiny_pin.step", "e86130e9cc19deff"),
    ("tests/step-conformance/torus_ring_nurbs.step", "0c2c85a98751fe67"),
    ("tests/step-conformance/washer.step", "490ba83889e1002f"),
    ("tests/step-regressions/cylinder_rim_off_its_wall.step", "e07c2fdbef7cc0c6"),
    ("tests/step-regressions/cylinder_spline_rims.step", "609aa9a609b95308"),
    ("tests/step-regressions/rod_doubled_edge.step", "8432fa140e74c146"),
    ("tests/step-regressions/tiny_rounds_coarse_tolerance.step", "d2c476d6163c1863"),
    ("(assembly)", "45b6ef6cf64529e2"),
];

/// Each body's (face count, volume) as read.
fn measures(bodies: &[solidcraft_kernel::ImportedBody]) -> Vec<(usize, f64)> {
    bodies
        .iter()
        .map(|b| {
            let m = measure(&b.body).unwrap();
            (m.faces, m.volume)
        })
        .collect()
}

/// The export reads back as the same bodies: face counts equal, volumes within 1e-6.
fn round_trip(name: &str, first: &[(usize, f64)], text: &str) -> Result<(), String> {
    let again = step_import(text).map_err(|e| format!("{name}: the export does not read back: {e}"))?;
    let second = measures(&again.bodies);
    if first.len() != second.len() {
        return Err(format!("{name}: {} bodies read back as {}", first.len(), second.len()));
    }
    for (i, ((f0, v0), (f1, v1))) in first.iter().zip(&second).enumerate() {
        if f0 != f1 {
            return Err(format!("{name}: body {i}: {f0} faces read back as {f1}"));
        }
        if (v0 - v1).abs() > 1e-6 * v0.abs().max(v1.abs()) {
            return Err(format!("{name}: body {i}: volume {v0} read back as {v1}"));
        }
    }
    Ok(())
}

#[test]
fn step_export_keeps_its_bytes_and_reads_back() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let print = std::env::var_os("STEP_EXPORT_BYTES_PRINT").is_some();
    let mut table: Vec<(String, String)> = Vec::new();
    let mut failures = Vec::new();
    for name in fixtures(&root) {
        let text = std::fs::read_to_string(root.join(&name)).unwrap();
        let imp = step_import(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        let bodies: Vec<ExportBody> = imp.bodies.iter().map(|b| ExportBody { name: b.name.clone(), body: &b.body, color: b.color }).collect();
        let file_name = name.rsplit('/').next().unwrap_or(&name).to_string();
        let out = step_export_bodies(&bodies, &StepHeader { file_name, ..Default::default() }).unwrap_or_else(|e| panic!("{name}: {e}"));
        table.push((name.clone(), digest(&out)));
        if let Err(e) = round_trip(&name, &measures(&imp.bodies), &out) {
            failures.push(e);
        }
    }
    // An assembly: a part placed twice, names (one not ASCII) and a colour.
    let bx = box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 5.0)).unwrap();
    let pin = cylinder(Vec3::ZERO, Vec3::Z, 2.0, 8.0).unwrap();
    let rot90 = [[0.0, 1.0, 0.0, 0.0], [-1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [50.0, 0.0, 0.0, 1.0]];
    let shift = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 5.0, 1.0]];
    let products = vec![
        ExportProduct {
            name: "Root Ä".into(),
            bodies: vec![ExportBody { name: "Base".into(), body: &bx, color: Some([1.0, 0.0, 0.0]) }],
            children: vec![(1, shift, "Pin:1".into()), (1, rot90, String::new())],
        },
        ExportProduct { name: "Pin".into(), bodies: vec![ExportBody { name: "Shaft".into(), body: &pin, color: None }], children: vec![] },
    ];
    let header = StepHeader {
        file_name: "assembly.step".into(),
        time_stamp: "2026-11-12T00:00:00".into(),
        author: "a".into(),
        organization: "o".into(),
        description: "d".into(),
    };
    let out = step_export_products(&products, 0, &header).unwrap();
    table.push(("(assembly)".into(), digest(&out)));
    if print {
        for (name, d) in &table {
            println!("    (\"{name}\", \"{d}\"),");
        }
        return;
    }
    let expected: Vec<(String, String)> = DIGESTS.iter().map(|(n, d)| ((*n).to_string(), (*d).to_string())).collect();
    assert_eq!(table, expected, "the export's bytes changed");
    assert!(failures.is_empty(), "round trips: {failures:#?}");
}
