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

/// #2846: `file.save_mesh`'s `deviation` is an absolute chord tolerance in millimetres: a finer
/// one tessellates a curved face more densely, and anything that is not a positive number is
/// refused.
#[test]
fn a_finer_surface_deviation_exports_a_denser_mesh() {
    let mut s = Session::default();
    run(&mut s, json!([{"command": "solid.cylinder", "params": {"diameter": 40, "height": 30}}]));
    let dir = std::env::temp_dir().join(format!("solidcraft-deviation-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let export = |s: &mut Session, name: &str, extra: Value| -> usize {
        let path = dir.join(name);
        let mut p = json!({"path": path.to_string_lossy()});
        for (k, v) in extra.as_object().unwrap() {
            p.as_object_mut().unwrap().insert(k.clone(), v.clone());
        }
        s.execute("file.save_mesh", &p).unwrap();
        read_stl(&std::fs::read(&path).unwrap()).unwrap().len()
    };
    let standard = export(&mut s, "standard.stl", json!({}));
    let fine = export(&mut s, "fine.stl", json!({"deviation": 0.001}));
    assert!(fine > standard, "deviation 0.001 mm on a 40 mm cylinder: {fine} triangles vs {standard}");
    // A deviation overrides the quality preset rather than combining with it.
    let coarse = export(&mut s, "coarse.stl", json!({"deviation": 0.5, "quality": "finest"}));
    assert!(coarse < standard, "0.5 mm beats even finest: {coarse} triangles vs {standard}");
    for bad in [json!(0), json!(-1), json!("wide"), json!(true)] {
        let r = s.execute("file.save_mesh", &json!({"path": dir.join("bad.stl").to_string_lossy(), "deviation": bad}));
        assert!(r.is_err(), "{bad} is not a positive number of millimetres");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
