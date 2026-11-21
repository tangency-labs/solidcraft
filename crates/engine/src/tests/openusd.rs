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
    // A format SolidCraft writes itself, named, is written whatever the extension, as it always
    // was; another format cadmpeg writes is not written under an OpenUSD name.
    let stl = dir.join("named-stl.usda").to_string_lossy().to_string();
    assert_eq!(run(&mut s, "file.export", json!({"path": stl, "format": "stl"}))["format"], "StlBinary");
    assert!(!std::fs::read(&stl).unwrap().starts_with(b"#usda"));
    let rhino = dir.join("named-rhino.usda").to_string_lossy().to_string();
    let e = s.execute("file.export", &json!({"path": rhino, "format": "3dm"})).unwrap_err().to_string();
    assert!(e.contains("disagrees with the path's extension"), "{e}");
    assert!(!std::path::Path::new(&rhino).exists());
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

/// The points of every `Mesh` prim of a text layer.
fn mesh_points(text: &str) -> usize {
    // A long array goes on over several lines, up to its closing bracket.
    text.split("point3f[] points = [").skip(1).map(|rest| rest.split(']').next().unwrap_or("").matches('(').count()).sum()
}

/// `quality` is the export's Mesh quality, as for STL: a finer one writes more points for a
/// cylinder, whose round faces are what it refines.
#[test]
fn mesh_quality_changes_the_points_written() {
    let dir = temp_dir("quality");
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 20, "height": 10, "body_name": "Disc"}));
    let mut points = Vec::new();
    for quality in ["standard", "fine"] {
        let path = dir.join(format!("{quality}.usda")).to_string_lossy().to_string();
        let out = run(&mut s, "file.export", json!({"path": path, "quality": quality}));
        assert_eq!(out["format"], "Usd", "{out}");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("def BrepArray"), "the exact shape goes with the mesh:\n{text}");
        points.push(mesh_points(&text));
    }
    assert!(points[0] > 0 && points[1] > points[0], "fine writes more points than standard: {points:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A mesh body (an inserted STL) has no exact shape: it is written as its mesh alone, and reads
/// back as the same box.
#[test]
fn a_mesh_body_exports_as_a_mesh() {
    let dir = temp_dir("meshbody");
    let stl = dir.join("cube.stl").to_string_lossy().to_string();
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 20, "height": 30}));
    run(&mut s, "file.save_mesh", json!({"path": stl}));
    let mut s = Session::default();
    run(&mut s, "file.insert_mesh", json!({"path": stl}));
    assert!(s.world_state().unwrap().bodies.iter().all(|b| b.body.is_mesh()));
    let path = dir.join("cube.usda").to_string_lossy().to_string();
    let out = run(&mut s, "file.export", json!({"path": path}));
    assert!(out.get("warnings").is_none_or(|w| w.as_array().is_some_and(Vec::is_empty)), "{out}");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("def Mesh") && !text.contains("def BrepArray"), "{text}");
    let mut back = Session::default();
    run(&mut back, "file.insert_mesh", json!({"path": path}));
    assert_eq!(shown_boxes(&back), vec![mm([0.0, 0.0, 0.0, 10.0, 20.0, 30.0])]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A placement that mirrors (not a rigid one) is not written as one: every body is written where
/// the design shows it, with a warning naming the occurrence, and reads back there.
#[test]
fn a_mirroring_placement_writes_the_bodies_where_they_are_shown() {
    let dir = temp_dir("mirror");
    let mut s = bracket();
    let o = s.doc_mut().occurrences.iter_mut().find(|o| o.name.starts_with("Bracket")).unwrap();
    o.transform[0][0] = -1.0;
    let name = o.name.clone();
    let shown = shown_boxes(&s);
    assert_ne!(shown, bracket_boxes(), "the mirror moves the first bracket");
    let path = dir.join("mirror.usda").to_string_lossy().to_string();
    let out = run(&mut s, "file.export", json!({"path": path}));
    assert!(out["warnings"].to_string().contains(&name), "{out}");
    let mut back = Session::default();
    run(&mut back, "file.insert_mesh", json!({"path": path}));
    assert_eq!(shown_boxes(&back), shown);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A component moved and not captured yet is written where the design shows it, in the
/// assembly as when the bodies are chosen by name.
#[test]
fn an_uncaptured_move_is_written_where_it_is_shown() {
    let dir = temp_dir("moved");
    let mut s = bracket();
    let o = s.doc.occurrences.iter().find(|o| o.name.starts_with("Bracket")).unwrap();
    let mut moved = o.transform;
    moved[3][0] += 25.0;
    s.pending_moves.insert(o.id, moved);
    let shown = shown_boxes(&s);
    assert_ne!(shown, bracket_boxes(), "the move shows");
    let path = dir.join("moved.usda").to_string_lossy().to_string();
    let out = run(&mut s, "file.export", json!({"path": path}));
    assert!(out.get("warnings").is_none_or(|w| w.as_array().is_some_and(Vec::is_empty)), "{out}");
    let mut back = Session::default();
    run(&mut back, "file.insert_mesh", json!({"path": path}));
    assert_eq!(shown_boxes(&back), shown);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The text of the `Xform` prim named `name` in `text`: from its `def` to its closing brace.
fn prim<'a>(text: &'a str, name: &str) -> &'a str {
    let start = text.find(&format!("def Xform \"{name}\"")).unwrap_or_else(|| panic!("no {name} prim:\n{text}"));
    let mut depth = 0usize;
    for (i, c) in text[start..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' if depth == 1 => return &text[start..=start + i],
            '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    panic!("{name}'s prim does not close:\n{text}")
}

/// A body the view hides is written invisible. A body a render setup gives optics is written,
/// with a warning naming the body, the setup and what the file leaves out.
#[test]
fn hidden_bodies_are_invisible_and_render_setup_optics_warn() {
    let dir = temp_dir("view");
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 3, "body_name": "Glass"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 3, "corner": [20, 0, 0], "operation": "new", "body_name": "Shelf"}));
    run(
        &mut s,
        "render.settings.set",
        json!({"name": "Studio", "set": {"width": 4, "height": 4, "job": {"method": "cpu_raytrace", "options": {"lights": [],
               "environment": {"color": [1, 1, 1], "intensity": 1}, "progressive": {"samples_per_pixel": 1, "seed": 1}}}}}),
    );
    let revision = s.revision;
    run(
        &mut s,
        "render.optics.bind",
        json!({"name": "Studio", "body_index": 0, "body_name": "Glass", "expected_revision": revision,
               "optics": {"ior": 1.5, "absorption_per_mm": [0.0, 0.0, 0.0], "boundary": "closed_mesh"}}),
    );
    run(&mut s, "browser.visibility", json!({"items": ["Shelf"], "visible": false}));
    let path = dir.join("view.usda");
    let reply = run(&mut s, "file.export", json!({"path": path.to_string_lossy()}));
    let warnings: Vec<String> = serde_json::from_value(reply["warnings"].clone()).unwrap();
    assert!(
        warnings.iter().any(|w| w.starts_with("Glass: render setup `Studio` gives it optics, which OpenUSD export does not write yet")),
        "{warnings:?}"
    );
    let text = std::fs::read_to_string(&path).unwrap();
    let invisible = "token visibility = \"invisible\"";
    assert_eq!(text.matches(invisible).count(), 1, "{text}");
    assert!(prim(&text, "Shelf").contains(invisible), "the Shelf prim holds it:\n{text}");
    assert!(!prim(&text, "Glass").contains(invisible), "Glass stays shown:\n{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The value of the attribute that `line` (such as `float physics:mass =`) starts, in `text`
/// after `from`.
fn value_after(text: &str, from: usize, line: &str) -> Vec<f64> {
    let rest = &text[from..];
    let at = rest.find(line).unwrap_or_else(|| panic!("no `{line}` after {from}:\n{text}"));
    let value = rest[at + line.len()..].lines().next().unwrap_or_default();
    value.split(['(', ')', ',']).filter_map(|v| v.trim().parse().ok()).collect()
}

/// With `physics`, a jointed placement moved without capturing the move is written where its
/// joints place it, as URDF export writes it, with URDF's warning, so its joint's frames meet as a
/// simulator starts: the file is the one written before the move.
#[test]
fn a_physics_export_writes_jointed_parts_where_their_joints_place_them() {
    let dir = temp_dir("physics-moved");
    let mut s = Session::default();
    for (name, x) in [("A", 0), ("B", 20)] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
    }
    run(&mut s, "component.activate", json!({"component": "root"}));
    let a = s.doc.occurrence_of(s.doc.find_component("A").unwrap()).unwrap().id;
    run(&mut s, "occurrence.ground", json!({"occurrence": a, "grounded": true}));
    run(&mut s, "joint.create", json!({"type": "revolute", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "name": "Hinge"}));
    let before = dir.join("before.usda");
    run(&mut s, "file.export", json!({"path": before.to_string_lossy(), "physics": true}));
    let b = s.doc.occurrence_of(s.doc.find_component("B").unwrap()).unwrap();
    let (id, name) = (b.id, b.name.clone());
    let mut moved = b.transform;
    moved[3][0] += 20.0;
    s.pending_moves.insert(id, moved);
    let after = dir.join("after.usda");
    let reply = run(&mut s, "file.export", json!({"path": after.to_string_lossy(), "physics": true}));
    let warnings: Vec<String> = serde_json::from_value(reply["warnings"].clone()).unwrap_or_default();
    assert!(
        warnings.iter().any(|w| w == &format!("`{name}` was moved but the move is not captured: exported where its joints place it")),
        "{warnings:?}"
    );
    assert_eq!(std::fs::read_to_string(&after).unwrap(), std::fs::read_to_string(&before).unwrap());
    let _ = std::fs::remove_dir_all(&dir);
}

/// With `physics`, an export of chosen bodies writes no physics, so a jointed placement moved
/// without capturing the move is written where the design shows it, as without `physics`, and
/// nothing says it was exported where its joints place it.
#[test]
fn a_physics_export_of_chosen_bodies_writes_them_where_they_are_shown() {
    let dir = temp_dir("physics-chosen");
    let mut s = Session::default();
    for (name, x) in [("A", 0), ("B", 20)] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
    }
    run(&mut s, "component.activate", json!({"component": "root"}));
    let a = s.doc.occurrence_of(s.doc.find_component("A").unwrap()).unwrap().id;
    run(&mut s, "occurrence.ground", json!({"occurrence": a, "grounded": true}));
    run(&mut s, "joint.create", json!({"type": "revolute", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "name": "Hinge"}));
    let b = s.doc.occurrence_of(s.doc.find_component("B").unwrap()).unwrap();
    let id = b.id;
    let mut moved = b.transform;
    moved[3][0] += 20.0;
    s.pending_moves.insert(id, moved);
    let world = s.world_state().unwrap();
    let body = world.bodies.iter().find(|b| solidcraft_kernel::measure(&b.body).unwrap().bbox.min.x > 15.0).expect("the moved body").name.clone();
    let export = |s: &mut Session, name: &str, physics: bool| {
        let path = dir.join(name);
        let reply = run(s, "file.export", json!({"path": path.to_string_lossy(), "bodies": [body.clone()], "physics": physics}));
        let warnings: Vec<String> = serde_json::from_value(reply["warnings"].clone()).unwrap_or_default();
        (std::fs::read_to_string(&path).unwrap(), warnings)
    };
    let (with, warnings) = export(&mut s, "with.usda", true);
    assert!(!warnings.iter().any(|w| w.contains("exported where its joints place it")), "{warnings:?}");
    assert!(warnings.iter().any(|w| w.contains("no physics is written")), "{warnings:?}");
    let (without, _) = export(&mut s, "without.usda", false);
    assert_eq!(with, without);
    let _ = std::fs::remove_dir_all(&dir);
}

/// With `physics`, the joints, the grounded placement and each component's mass are written as
/// OpenUSD physics: a revolute joint with its limits in degrees, the grounded placement fixed to
/// the world, and a 10 mm cube's mass, centre and inertia from its density. A joint with no
/// OpenUSD form is left out with a warning. Without `physics`, none of it is written.
#[test]
fn physics_writes_joints_grounding_and_mass() {
    let dir = temp_dir("physics");
    let mut s = Session::default();
    for (name, x) in [("A", 0), ("B", 20), ("C", 40)] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
    }
    run(&mut s, "component.activate", json!({"component": "root"}));
    let a = s.doc.occurrence_of(s.doc.find_component("A").unwrap()).unwrap().id;
    run(&mut s, "occurrence.ground", json!({"occurrence": a, "grounded": true}));
    run(
        &mut s,
        "joint.create",
        json!({"type": "revolute", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "limits": [[-0.5, 1.0]], "name": "Hinge"}),
    );
    run(&mut s, "joint.create", json!({"type": "pin_slot", "a": {"face": [25, 5, 10]}, "b": {"face": [45, 5, 0]}, "name": "Slot"}));
    let path = dir.join("physics.usda");
    let reply = run(&mut s, "file.export", json!({"path": path.to_string_lossy(), "physics": true}));
    let warnings: Vec<String> = serde_json::from_value(reply["warnings"].clone()).unwrap();
    assert!(warnings.iter().any(|w| w.starts_with("joint `Slot`: a PinSlot joint has no OpenUSD physics form")), "{warnings:?}");
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.matches("def PhysicsRevoluteJoint").count(), 1, "{text}");
    assert_eq!(text.matches("def PhysicsFixedJoint").count(), 1, "the grounded A:\n{text}");
    let hinge = text.find("def PhysicsRevoluteJoint").unwrap();
    let lower = value_after(&text, hinge, "float physics:lowerLimit = ");
    let upper = value_after(&text, hinge, "float physics:upperLimit = ");
    // `joint.create` takes the limits in degrees, as written.
    let saved = s.doc.assembly.joints.iter().find(|j| j.name == "Hinge").unwrap().limits[0].unwrap();
    assert!((saved.0.to_degrees() + 0.5).abs() < 1e-9 && (saved.1.to_degrees() - 1.0).abs() < 1e-9, "{saved:?}");
    assert!((lower[0] + 0.5).abs() < 1e-4 && (upper[0] - 1.0).abs() < 1e-4, "{lower:?} {upper:?}");
    // B: a 10 mm cube (1 cm³) of its material's density (g/cm³), centred at (25, 5, 5) mm in its
    // frame; about its centre each moment is m a² / 6. The stage is in kg and metres.
    let body = s.model.state().bodies.iter().find(|b| s.doc.model_body_component(b) == s.doc.find_component("B").unwrap()).unwrap().name.clone();
    let mass = s.doc.density(&body) * 1e-3;
    let b = text.find("\"B").unwrap_or_else(|| panic!("no B prim:\n{text}"));
    let got = value_after(&text, b, "float physics:mass = ");
    assert!((got[0] - mass).abs() < 1e-6 * mass.max(1.0), "{got:?} is not {mass}");
    let com = value_after(&text, b, "point3f physics:centerOfMass = ");
    assert_eq!(com.len(), 3, "{com:?}");
    for (g, w) in com.iter().zip([0.025, 0.005, 0.005]) {
        assert!((g - w).abs() < 1e-6, "{com:?}");
    }
    let inertia = value_after(&text, b, "float3 physics:diagonalInertia = ");
    assert_eq!(inertia.len(), 3, "{inertia:?}");
    let moment = mass * 0.01 * 0.01 / 6.0;
    for g in &inertia {
        assert!((g - moment).abs() < 1e-3 * moment, "{inertia:?} is not {moment} each");
    }

    let plain = dir.join("plain.usda");
    run(&mut s, "file.export", json!({"path": plain.to_string_lossy()}));
    let text = std::fs::read_to_string(&plain).unwrap();
    assert!(!text.contains("Physics"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}
