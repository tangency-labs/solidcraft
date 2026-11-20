//! OpenUSD through the commands: `file.export` writes `.usda`, `.usdc`, `.usd` and `.usdz`, and
//! `file.insert_mesh` and `doc.open` read them back as mesh bodies in a component named after the
//! file, the prim tree as nested components. Expected boxes are those the design's own
//! dimensions give; nothing is taken from SolidCraft's output.

use serde_json::json;

use super::run;
use crate::*;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("solidcraft-usd-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// World bounding boxes of every shown body, rounded to 0.01 mm (USD mesh points are single
/// precision), sorted.
fn shown_boxes(s: &Session) -> Vec<[i64; 6]> {
    let mut out: Vec<[i64; 6]> = s
        .world_state()
        .unwrap()
        .bodies
        .iter()
        .map(|b| {
            let bb = solidcraft_kernel::measure(&b.body).unwrap().bbox;
            [bb.min.x, bb.min.y, bb.min.z, bb.max.x, bb.max.y, bb.max.z].map(|v| (v * 100.0).round() as i64)
        })
        .collect();
    out.sort();
    out
}

fn mm(b: [f64; 6]) -> [i64; 6] {
    b.map(|v| (v * 100.0).round() as i64)
}

/// A loose 10 mm cube, and a `Bracket` component (a 20 × 20 × 10 mm base and a 10 × 10 × 10 mm
/// block on it) placed twice, the second copy 40 mm along Y.
fn bracket() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [-30, 0, 0], "body_name": "Loose"}));
    run(&mut s, "component.create", json!({"name": "Bracket"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "body_name": "Base"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [5, 5, 10], "operation": "new", "body_name": "Block"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "occurrence.copy", json!({"component": "Bracket", "translate": [0, 40, 0]}));
    s
}

fn bracket_boxes() -> Vec<[i64; 6]> {
    let mut want = vec![
        mm([-30.0, 0.0, 0.0, -20.0, 10.0, 10.0]),
        mm([0.0, 0.0, 0.0, 20.0, 20.0, 10.0]),
        mm([0.0, 40.0, 0.0, 20.0, 60.0, 10.0]),
        mm([5.0, 5.0, 10.0, 15.0, 15.0, 20.0]),
        mm([5.0, 45.0, 10.0, 15.0, 55.0, 20.0]),
    ];
    want.sort();
    want
}

/// Each encoding exports and reads back with every body where the design places it; the bracket
/// placed twice comes back as one component placed twice, its bodies built once.
#[test]
fn an_assembly_exports_to_openusd_and_reads_back_in_place() {
    let dir = temp_dir("round");
    let mut s = bracket();
    assert_eq!(shown_boxes(&s), bracket_boxes());
    for file in ["bracket.usda", "bracket.usdc", "bracket.usd", "bracket.usdz"] {
        let path = dir.join(file).to_string_lossy().to_string();
        let out = run(&mut s, "file.export", json!({"path": path}));
        assert_eq!(out["format"], "Usd", "{out}");
        assert!(out["bytes"].as_u64().unwrap() > 0);
        assert!(out.get("warnings").is_none_or(|w| w.as_array().is_some_and(Vec::is_empty)), "{file}: {out}");

        let mut back = Session::default();
        let r = run(&mut back, "file.insert_mesh", json!({"path": path}));
        assert_eq!(r["component_name"], "bracket", "{file}: {r}");
        // The file says Z is up, so nothing is turned.
        assert!(r.get("stand_up").is_none(), "{file}: {r}");
        assert_eq!(shown_boxes(&back), bracket_boxes(), "{file}");
        let bracket = back.doc.components.iter().find(|c| c.name.starts_with("Bracket")).map(|c| c.id);
        let bracket =
            bracket.unwrap_or_else(|| panic!("{file}: no Bracket in {:?}", back.doc.components.iter().map(|c| &c.name).collect::<Vec<_>>()));
        assert_eq!(back.doc.occurrences.iter().filter(|o| o.component == bracket).count(), 2, "{file}");
        assert_eq!(back.model.state().bodies.len(), 3, "{file}: Loose, Base and Block, each built once");

        // Opened by itself, the file is a new design holding that component.
        let mut opened = Session::default();
        let r = run(&mut opened, "doc.open", json!({"path": path}));
        assert_eq!(r["component_name"], "bracket", "{file}: {r}");
        assert_eq!(shown_boxes(&opened), bracket_boxes(), "{file}");
    }
    // `format` may say OpenUSD, or the encoding the extension picks, but not another encoding.
    let path = dir.join("named.usdc").to_string_lossy().to_string();
    for f in ["usd", "usdc", "USDC"] {
        assert_eq!(run(&mut s, "file.export", json!({"path": path, "format": f}))["format"], "Usd", "{f}");
    }
    let e = s.execute("file.export", &json!({"path": path, "format": "usda"})).unwrap_err().to_string();
    assert!(e.contains("disagrees with the path's extension"), "{e}");
    // Chosen bodies only.
    let path = dir.join("loose.usda").to_string_lossy().to_string();
    run(&mut s, "file.export", json!({"path": path, "bodies": ["Loose"]}));
    let mut back = Session::default();
    run(&mut back, "file.insert_mesh", json!({"path": path}));
    assert_eq!(shown_boxes(&back), vec![mm([-30.0, 0.0, 0.0, -20.0, 10.0, 10.0])]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A layer that says Y is up stands up by default, `up: z` keeps its axes, and a unit is refused
/// (the file states its own).
#[test]
fn a_y_up_layer_stands_up_unless_told_otherwise() {
    let dir = temp_dir("up");
    let path = dir.join("tet.usda");
    std::fs::write(
        &path,
        "#usda 1.0\n(\n    metersPerUnit = 0.001\n    upAxis = \"Y\"\n)\ndef Mesh \"Tet\"\n{\n    int[] faceVertexCounts = [3, 3, 3, 3]\n    \
         int[] faceVertexIndices = [0, 2, 1, 0, 1, 3, 1, 2, 3, 2, 0, 3]\n    point3f[] points = [(0, 0, 0), (10, 0, 0), (0, 20, 0), (0, 0, 30)]\n}\n",
    )
    .unwrap();
    let path = path.to_string_lossy().to_string();
    let mut s = Session::default();
    let r = run(&mut s, "file.insert_mesh", json!({"path": path}));
    assert!(r["stand_up"].as_u64().is_some(), "{r}");
    // A quarter turn about X: file Y (0..20) is up, file Z (0..30) is toward -Y.
    assert_eq!(shown_boxes(&s), vec![mm([0.0, -30.0, 0.0, 10.0, 0.0, 20.0])]);
    let mut kept = Session::default();
    let r = run(&mut kept, "file.insert_mesh", json!({"path": path, "up": "z"}));
    assert!(r.get("stand_up").is_none(), "{r}");
    assert_eq!(shown_boxes(&kept), vec![mm([0.0, 0.0, 0.0, 10.0, 20.0, 30.0])]);
    let e = Session::default().execute("file.insert_mesh", &json!({"path": path, "units": "cm"})).unwrap_err().to_string();
    assert!(e.contains("OpenUSD file states its own"), "{e}");
    let broken = dir.join("broken.usdz").to_string_lossy().to_string();
    std::fs::write(&broken, b"PK\x03\x04 not a package").unwrap();
    let e = Session::default().execute("file.insert_mesh", &json!({"path": broken})).unwrap_err().to_string();
    assert!(e.contains("broken.usdz"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Every shown body's volume, rounded to 0.001 mm³, sorted, and whether any is a mesh.
fn volumes(s: &Session) -> (Vec<i64>, bool) {
    let state = s.world_state().unwrap();
    let mut v: Vec<i64> = state.bodies.iter().map(|b| (solidcraft_kernel::measure(&b.body).unwrap().volume * 1000.0).round() as i64).collect();
    v.sort();
    (v, state.bodies.iter().any(|b| b.body.is_mesh()))
}

/// With `geometry: "exact"`, the bracket comes back from each encoding as B-rep bodies, not
/// meshes: each where the design places it, with the volumes the design's dimensions give (the
/// loose cube 1000 mm³, the base 4000 and the block 1000 at each of the bracket's two
/// placements), the bracket one component placed twice. Without it, the same file still comes in
/// as meshes.
#[test]
fn exact_geometry_reads_an_assembly_back_as_brep() {
    let dir = temp_dir("exact");
    let mut s = bracket();
    for file in ["bracket.usda", "bracket.usdz"] {
        let path = dir.join(file).to_string_lossy().to_string();
        run(&mut s, "file.export", json!({"path": path}));

        let mut back = Session::default();
        let r = run(&mut back, "file.insert_mesh", json!({"path": path, "geometry": "exact"}));
        assert_eq!(r["geometry"], "exact", "{file}: {r}");
        assert_eq!(r["component_name"], "bracket", "{file}: {r}");
        assert!(r.get("stand_up").is_none(), "{file}: the file says Z is up: {r}");
        assert_eq!(shown_boxes(&back), bracket_boxes(), "{file}");
        let (v, mesh) = volumes(&back);
        assert!(!mesh, "{file}: every body is a B-rep");
        assert_eq!(v, vec![1_000_000, 1_000_000, 1_000_000, 4_000_000, 4_000_000], "{file}");
        let feature = back.doc.features.iter().find(|f| f.id == r["feature"].as_u64().unwrap()).unwrap();
        assert!(matches!(&feature.kind, solidcraft_doc::FeatureKind::Import { records: Some(_), .. }), "{file}");
        let bracket = back.doc.components.iter().find(|c| c.name.starts_with("Bracket")).map(|c| c.id);
        let bracket =
            bracket.unwrap_or_else(|| panic!("{file}: no Bracket in {:?}", back.doc.components.iter().map(|c| &c.name).collect::<Vec<_>>()));
        assert_eq!(back.doc.occurrences.iter().filter(|o| o.component == bracket).count(), 2, "{file}");

        let mut opened = Session::default();
        let r = run(&mut opened, "doc.open", json!({"path": path, "geometry": "EXACT"}));
        assert_eq!(r["component_name"], "bracket", "{file}: {r}");
        assert_eq!(shown_boxes(&opened), bracket_boxes(), "{file}");
        assert!(!volumes(&opened).1, "{file}");

        // The default is the mesh import, as before.
        let mut meshes = Session::default();
        let r = run(&mut meshes, "file.insert_mesh", json!({"path": path, "geometry": "meshes"}));
        assert!(r.get("geometry").is_none(), "{file}: {r}");
        assert!(volumes(&meshes).1, "{file}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A stage that says Y is up stands up by default with `geometry: "exact"` too, and `up: z` keeps
/// its axes: the 10 × 20 × 30 mm box, turned a quarter about X, has its file Y (0..20) up and its
/// file Z (0..30) toward -Y. `geometry` is refused for another kind of file, an unknown choice is
/// refused, and so is a crease angle with exact geometry. `units` is refused for an OpenUSD file on
/// either route, `doc.open`'s exact one too, as the stage states its own.
#[test]
fn exact_geometry_stands_a_y_up_stage_up() {
    let dir = temp_dir("exact-up");
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 20, "height": 30}));
    let z_up = dir.join("box.usda").to_string_lossy().to_string();
    run(&mut s, "file.export", json!({"path": z_up}));
    let text = std::fs::read_to_string(&z_up).unwrap();
    assert!(text.contains("upAxis = \"Z\""));
    let path = dir.join("y_up.usda").to_string_lossy().to_string();
    std::fs::write(&path, text.replace("upAxis = \"Z\"", "upAxis = \"Y\"")).unwrap();

    let mut stood = Session::default();
    let r = run(&mut stood, "file.insert_mesh", json!({"path": path, "geometry": "exact"}));
    assert!(r["stand_up"].as_u64().is_some(), "{r}");
    assert_eq!(shown_boxes(&stood), vec![mm([0.0, -30.0, 0.0, 10.0, 0.0, 20.0])]);
    assert_eq!(volumes(&stood), (vec![6_000_000], false));
    let mut kept = Session::default();
    let r = run(&mut kept, "file.insert_mesh", json!({"path": path, "geometry": "exact", "up": "z"}));
    assert!(r.get("stand_up").is_none(), "{r}");
    assert_eq!(shown_boxes(&kept), vec![mm([0.0, 0.0, 0.0, 10.0, 20.0, 30.0])]);

    let mut opened = Session::default();
    let r = run(&mut opened, "doc.open", json!({"path": path, "geometry": "exact"}));
    assert!(r["stand_up"].as_u64().is_some(), "{r}");
    assert_eq!(shown_boxes(&opened), vec![mm([0.0, -30.0, 0.0, 10.0, 0.0, 20.0])]);

    let stl = dir.join("part.stl").to_string_lossy().to_string();
    run(&mut s, "file.export", json!({"path": stl}));
    for (cmd, p, why) in [
        ("file.insert_mesh", json!({"path": stl, "geometry": "exact"}), "OpenUSD files only"),
        ("doc.open", json!({"path": stl, "geometry": "exact"}), "OpenUSD files only"),
        ("file.insert_mesh", json!({"path": path, "geometry": "nurbs"}), "`geometry` must be"),
        ("file.insert_mesh", json!({"path": path, "geometry": "exact", "crease_angle_degrees": 30}), "crease_angle_degrees"),
        ("file.insert_mesh", json!({"path": path, "geometry": "exact", "units": "cm"}), "states its own"),
        ("doc.open", json!({"path": path, "geometry": "exact", "units": "cm"}), "states its own"),
        ("doc.open", json!({"path": path, "units": "cm"}), "states its own"),
    ] {
        let e = Session::default().execute(cmd, &p).unwrap_err().to_string();
        assert!(e.contains(why), "{cmd} {p}: {e}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
