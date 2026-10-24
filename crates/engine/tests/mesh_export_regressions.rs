//! Mesh export regressions: a closed body exports as a closed mesh.

use std::collections::HashMap;

use serde_json::{Value, json};
use solidcraft_engine::Session;
use solidcraft_io::{Format, export, read_3mf, read_stl};

fn run(s: &mut Session, script: Value) {
    for c in script.as_array().into_iter().flatten() {
        let (id, p) = (c["command"].as_str().unwrap_or_default(), &c["params"]);
        if let Err(e) = s.execute(id, p) {
            panic!("{id} {p}: {e}");
        }
    }
}

/// The edges (by vertex index) that are not used by exactly two triangles.
fn open_edges(triangles: &[[u32; 3]]) -> Vec<((u32, u32), u32)> {
    let mut uses: HashMap<(u32, u32), u32> = HashMap::new();
    for t in triangles {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            *uses.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    uses.into_iter().filter(|(_, n)| *n != 2).collect()
}

/// A bowl section (lines and arcs) revolved about Z, its rim cut by a large cylinder across it.
/// The edge where the cylinder meets the inner cone is an intersection curve; one of its samples
/// lay a few ten-thousandths of a millimetre off both surfaces, each face put the point on its
/// own surface, and the exported mesh had a crack there (four edges used once).
#[test]
fn a_rim_cut_by_a_cylinder_exports_closed() {
    let mut s = Session::default();
    run(
        &mut s,
        json!([
            {"command": "sketch.create", "params": {"plane": "XZ", "name": "Section"}},
            {"command": "sketch.line", "params": {"points": [[0, 0], [48.8, 0], [50.0240096077, -1.2]]}},
            {"command": "sketch.arc.three_point", "params": {"start": [50.0240096077, -1.2], "end": [58.7867965644, -21.2132034356], "through": [52.5188160894, -12.0326443841]}},
            {"command": "sketch.line", "params": {"points": [[58.7867965644, -21.2132034356], [85.5735931288, -48], [82.1794805791, -48], [57.0897402896, -22.9102597104]]}},
            {"command": "sketch.arc.three_point", "params": {"start": [57.0897402896, -22.9102597104], "end": [48.3846976331, -7.0875], "through": [51.6125175266, -15.6176451113]}},
            {"command": "sketch.arc.three_point", "params": {"start": [48.3846976331, -7.0875], "end": [42.5300120096, -2.4], "through": [46.2800120096, -3.7162515012]}},
            {"command": "sketch.line", "params": {"points": [[42.5300120096, -2.4], [0, -2.4], [0, 0]]}},
            {"command": "sketch.finish", "params": {}},
            {"command": "solid.revolve", "params": {"sketch": "Section", "axis": "Z"}},
            {"command": "sketch.create", "params": {"plane": "YZ", "name": "RimCutter"}},
            {"command": "sketch.arc.three_point", "params": {"start": [-100, 11.6204810674], "end": [100, 46.4045454912], "through": [-52, 8]}},
            {"command": "sketch.line", "params": {"points": [[100, 46.4045454912], [100, 70], [-100, 70], [-100, 11.6204810674]]}},
            {"command": "sketch.finish", "params": {}},
            {"command": "solid.extrude", "params": {"sketch": "RimCutter", "distance": 200, "direction": "symmetric", "operation": "cut"}}
        ]),
    );
    let st = s.model.state();
    assert_eq!(st.bodies.len(), 1);

    let (objects, _) = read_3mf(&export(&st, &[], Format::ThreeMf, "cove").unwrap()).unwrap();
    assert_eq!(objects.len(), 1);
    let open = open_edges(&objects[0].triangles);
    assert!(open.is_empty(), "3MF: {} edges not used twice, e.g. {:?}", open.len(), open.first());

    // STL has no indices: vertices are the same where their coordinates are.
    let mut index: HashMap<[u32; 3], u32> = HashMap::new();
    let triangles: Vec<[u32; 3]> = read_stl(&export(&st, &[], Format::StlBinary, "cove").unwrap())
        .unwrap()
        .iter()
        .map(|t| {
            t.map(|p| {
                let n = index.len() as u32;
                *index.entry(p.map(f32::to_bits)).or_insert(n)
            })
        })
        .collect();
    let open = open_edges(&triangles);
    assert!(open.is_empty(), "STL: {} edges not used twice, e.g. {:?}", open.len(), open.first());
}

/// The triangles of an STL export of `s` with these command options (`file.save_mesh`), read back.
fn saved_stl(s: &mut Session, label: &str, options: Value) -> Vec<[[f32; 3]; 3]> {
    let path = std::env::temp_dir().join(format!("solidcraft-mesh-options-{label}-{}.stl", std::process::id()));
    let mut p = options;
    p["path"] = json!(path.to_string_lossy());
    s.execute("file.save_mesh", &p).unwrap_or_else(|e| panic!("{label}: {e}"));
    let bytes = std::fs::read(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    read_stl(&bytes).unwrap()
}

/// How far, at most, the facets of `tris` sag inside a cylinder of radius `r` about the Z axis
/// through `(cx, cy)`: the depth of the middle of every edge of a side facet (one facing
/// sideways, not a cap) whose ends both lie on the cylinder.
fn sag(tris: &[[[f32; 3]; 3]], (cx, cy): (f64, f64), r: f64) -> f64 {
    let rho = |p: [f64; 3]| ((p[0] - cx).powi(2) + (p[1] - cy).powi(2)).sqrt();
    let mut worst: f64 = 0.0;
    for t in tris {
        let v = t.map(|p| p.map(f64::from));
        let (e1, e2) = ([v[1][0] - v[0][0], v[1][1] - v[0][1], v[1][2] - v[0][2]], [v[2][0] - v[0][0], v[2][1] - v[0][1], v[2][2] - v[0][2]]);
        let n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if !(len > 0.0) || (n[2] / len).abs() > 0.5 {
            continue;
        }
        for (a, b) in [(v[0], v[1]), (v[1], v[2]), (v[2], v[0])] {
            if (rho(a) - r).abs() < 1e-4 && (rho(b) - r).abs() < 1e-4 {
                let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, (a[2] + b[2]) / 2.0];
                worst = worst.max(r - rho(mid));
            }
        }
    }
    worst
}

/// The distinct vertices of `tris` on the circle of radius `r` about `(cx, cy)` at height `z`.
fn sides(tris: &[[[f32; 3]; 3]], (cx, cy): (f64, f64), r: f64, z: f64) -> usize {
    let mut seen = std::collections::HashSet::new();
    for p in tris.iter().flatten() {
        let (x, y, pz) = (f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
        if (pz - z).abs() < 1e-4 && (((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - r).abs() < 1e-4 {
            seen.insert(p.map(f32::to_bits));
        }
    }
    seen.len()
}

/// Without options a mesh export is what it was before they existed (#2846).
#[test]
fn mesh_export_without_options_is_unchanged() {
    let mut s = Session::default();
    run(&mut s, json!([{"command": "solid.cylinder", "params": {"diameter": 20, "height": 20}}]));
    let st = s.model.state();
    let meta = solidcraft_doc::Metadata::default();
    for format in [Format::StlBinary, Format::Obj, Format::ThreeMf] {
        let before = export(&st, &[], format, "c").unwrap();
        let after = solidcraft_io::export_with_options(&st, &[], format, "c", &meta, solidcraft_io::MeshOptions::default()).unwrap();
        assert!(before == after, "{format:?}");
    }
}

/// `tolerance` sets how far facets may sag from a curved face, coarser or finer than the default,
/// and the export keeps to it (#2846).
#[test]
fn mesh_export_tolerance_bounds_the_sag() {
    let mut s = Session::default();
    run(&mut s, json!([{"command": "solid.cylinder", "params": {"diameter": 20, "height": 20}}]));
    let coarse = saved_stl(&mut s, "coarse", json!({"tolerance": 0.1}));
    let default = saved_stl(&mut s, "default", json!({}));
    let fine = saved_stl(&mut s, "fine", json!({"tolerance": 0.001}));
    eprintln!("Ø20 × 20 cylinder: tolerance 0.1 → {} triangles, default → {}, 0.001 → {}", coarse.len(), default.len(), fine.len());
    assert!(coarse.len() < default.len() && default.len() < fine.len(), "{} {} {}", coarse.len(), default.len(), fine.len());
    for (tris, tol) in [(&coarse, 0.1), (&fine, 0.001)] {
        let sag = sag(tris, (0.0, 0.0), 10.0);
        eprintln!("  tolerance {tol}: facets sag at most {sag:.5} mm");
        assert!(sag <= tol + 1e-5 && sag > tol / 10.0, "tolerance {tol}: facets sag {sag}");
    }
}

/// A small hole in a large plate: the default tolerance follows the plate's size and leaves the
/// hole a polygon of a few sides; an angle gives it as many as a large hole (#2846).
#[test]
fn mesh_export_angle_rounds_small_holes_on_large_parts() {
    let mut s = Session::default();
    run(
        &mut s,
        json!([
            {"command": "solid.box", "params": {"length": 400, "width": 400, "height": 10, "corner": [-200, -200, 0]}},
            {"command": "solid.cylinder", "params": {"diameter": 2, "height": 10, "operation": "cut"}}
        ]),
    );
    let default = sides(&saved_stl(&mut s, "plate-default", json!({})), (0.0, 0.0), 1.0, 10.0);
    let angled = sides(&saved_stl(&mut s, "plate-angle", json!({"angle": 10})), (0.0, 0.0), 1.0, 10.0);
    eprintln!("Ø2 hole in a 400 × 400 plate: {default} sides by default, {angled} with angle 10");
    assert!(default < 12, "default: {default} sides");
    assert!(angled >= 36, "angle 10: {angled} sides");
}

/// Out-of-range or non-numeric options, and options for a format that isn't a mesh, are refused
/// and nothing is written.
#[test]
fn mesh_export_options_are_checked() {
    let mut s = Session::default();
    run(&mut s, json!([{"command": "solid.box", "params": {"length": 10, "width": 10, "height": 10}}]));
    let path = std::env::temp_dir().join(format!("solidcraft-mesh-options-bad-{}.stl", std::process::id()));
    let step = std::env::temp_dir().join(format!("solidcraft-mesh-options-bad-{}.step", std::process::id()));
    let p = path.to_string_lossy().to_string();
    let bad = [
        ("file.save_mesh", json!({"path": p, "tolerance": 0})),
        ("file.save_mesh", json!({"path": p, "tolerance": -1})),
        ("file.save_mesh", json!({"path": p, "tolerance": 1e308})),
        ("file.save_mesh", json!({"path": p, "tolerance": 11})),
        ("file.save_mesh", json!({"path": p, "tolerance": "fine"})),
        ("file.save_mesh", json!({"path": p, "angle": 0})),
        ("file.save_mesh", json!({"path": p, "angle": 0.5})),
        ("file.save_mesh", json!({"path": p, "angle": 360})),
        ("file.export", json!({"path": p, "angle": "round"})),
        ("file.export", json!({"path": step.to_string_lossy(), "tolerance": 0.1})),
    ];
    for (cmd, params) in bad {
        let err = s.execute(cmd, &params).map(|_| ()).unwrap_err().to_string();
        assert!(err.contains("tolerance") || err.contains("angle"), "{cmd} {params}: {err}");
        assert!(!path.exists() && !step.exists(), "{cmd} {params} wrote a file");
    }
    // In range, both are taken (the file is written).
    s.execute("file.save_mesh", &json!({"path": p, "tolerance": 0.05, "angle": 15})).unwrap();
    assert!(path.exists());
    let _ = std::fs::remove_file(&path);
}
