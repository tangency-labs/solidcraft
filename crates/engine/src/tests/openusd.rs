//! OpenUSD through the commands: `file.export` writes `.usda`, `.usdc`, `.usd` and `.usdz`, and
//! `file.insert_mesh` and `doc.open` read them back as mesh bodies in a component named after the
//! file, the prim tree as nested components. Expected boxes are those the design's own
//! dimensions give; nothing is taken from SolidCraft's output.

use serde_json::json;

use super::run;
use crate::*;

/// A test's temporary directory, removed when dropped, so a failed assertion cleans up too.
struct TempDir(std::path::PathBuf);

impl AsRef<std::path::Path> for TempDir {
    fn as_ref(&self) -> &std::path::Path {
        &self.0
    }
}

impl std::ops::Deref for TempDir {
    type Target = std::path::Path;

    fn deref(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp_dir(tag: &str) -> TempDir {
    let dir = std::env::temp_dir().join(format!("solidcraft-usd-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    TempDir(dir)
}

/// The members of a `.usdz` package, by name.
fn usdz_members(bytes: &[u8]) -> std::collections::BTreeMap<String, Vec<u8>> {
    use std::io::Read as _;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("a .usdz package is a ZIP archive");
    let mut members = std::collections::BTreeMap::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        let name = entry.name().unwrap().into_owned();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        members.insert(name, data);
    }
    members
}

/// Image `bytes` in a file of their own in the caller's directory, as `canvas.decal` and
/// `canvas.insert` take an image.
fn picture_file(dir: &std::path::Path, bytes: &[u8]) -> String {
    let path = dir.join("picture.png");
    std::fs::write(&path, bytes).unwrap();
    path.to_string_lossy().into_owned()
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
    // The grounded A roots the articulation of A and B, jointed by the hinge; C's joint has no
    // OpenUSD form, so C is in none.
    let a_prim = text.find("\"A_1\"").unwrap_or_else(|| panic!("no A_1 prim:\n{text}"));
    let a_head = &text[a_prim..a_prim + text[a_prim..].find('{').unwrap_or(0)];
    assert!(a_head.contains("PhysicsArticulationRootAPI"), "{text}");
    assert_eq!(text.matches("PhysicsArticulationRootAPI").count(), 1, "{text}");
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

/// With `physics`, gears and a rack couple their joints as PhysX does: G1 (radius 10) meshing G2
/// (radius 20) turns it half as far the other way, a PhysX gear joint of ratio 0.5 from G1's
/// hinge to G2's; a 10 mm pinion moves its rack 10 mm per radian, 0.1 rad/mm, which the stage
/// writes in degrees per metre. An internal gear turns its mate the same way, which PhysX's gear
/// joint does not: it is left out with a warning.
#[test]
fn physics_writes_gears_and_racks_as_couplings() {
    let dir = temp_dir("gears");
    let mut s = Session::default();
    let part = |s: &mut Session, name: &str, solid: &str, p: Value| {
        run(s, "component.activate", json!({"component": "root"}));
        run(s, "component.create", json!({"name": name, "grounded": false}));
        run(s, solid, p);
    };
    part(&mut s, "Base", "solid.box", json!({"length": 120, "width": 60, "height": 10, "corner": [-30, -30, -20]}));
    for (name, x, r) in [("G1", 0, 10), ("G2", 30, 20), ("G3", 65, 15)] {
        part(&mut s, name, "solid.cylinder", json!({"base": [x, 0, 0], "axis": [0, 0, 1], "radius": r, "height": 5}));
    }
    part(&mut s, "Rack", "solid.box", json!({"length": 40, "width": 4, "height": 4, "corner": [-20, -14, 0]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let base = s.doc.occurrence_of(s.doc.find_component("Base").unwrap()).unwrap().id;
    run(&mut s, "occurrence.ground", json!({"occurrence": base, "grounded": true}));
    let pin = |s: &mut Session, kind: &str, name: &str, b: &str, p: [f64; 3], z: [f64; 3]| {
        let at = |o: &str| json!({"occurrence": o, "point": p, "z": z});
        run(s, "joint.create", json!({"type": kind, "a": at("Base"), "b": at(b), "flip": true, "name": name}));
    };
    pin(&mut s, "revolute", "H1", "G1", [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    pin(&mut s, "revolute", "H2", "G2", [30.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    pin(&mut s, "revolute", "H3", "G3", [65.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    pin(&mut s, "slider", "Rail", "Rack", [0.0, -10.0, 0.0], [1.0, 0.0, 0.0]);
    run(&mut s, "joint.gear", json!({"a": "H1", "b": "H2", "ra": 10, "rb": 20}));
    run(&mut s, "joint.rack_pinion", json!({"a": "H1", "b": "Rail", "radius": 10}));
    run(&mut s, "joint.gear", json!({"a": "H2", "b": "H3", "ra": 20, "rb": 15, "internal": true}));
    let path = dir.join("gears.usda");
    let reply = run(&mut s, "file.export", json!({"path": path.to_string_lossy(), "physics": true}));
    let warnings: Vec<String> = serde_json::from_value(reply["warnings"].clone()).unwrap();
    assert!(warnings.iter().any(|w| w.starts_with("motion link 3 between `H2` and `H3`: its gears turn the same way")), "{warnings:?}");
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.matches("def PhysxPhysicsGearJoint").count(), 1, "{text}");
    assert_eq!(text.matches("def PhysxPhysicsRackAndPinionJoint").count(), 1, "{text}");
    // Each coupled joint: the revolute or prismatic joint whose prim the relationship names,
    // found by the part its B side moves.
    let target = |at: usize, rel: &str| -> String {
        let rest = &text[at..];
        let line = rest[rest.find(rel).unwrap_or_else(|| panic!("no {rel}:\n{text}"))..].lines().next().unwrap();
        line[line.find('<').unwrap() + 1..line.find('>').unwrap()].to_string()
    };
    let moves = |joint: &str| -> String {
        let name = joint.rsplit('/').next().unwrap();
        let at = text.find(&format!("\"{name}\"")).unwrap_or_else(|| panic!("no {joint}:\n{text}"));
        target(at, "rel physics:body1 = ")
    };
    // Each coupling is named after the joints it couples.
    assert!(text.contains("def PhysxPhysicsGearJoint \"H1_H2\""), "{text}");
    assert!(text.contains("def PhysxPhysicsRackAndPinionJoint \"H1_Rail\""), "{text}");
    let gear = text.find("def PhysxPhysicsGearJoint").unwrap();
    assert!((value_after(&text, gear, "float physics:gearRatio = ")[0] - 0.5).abs() < 1e-6, "{text}");
    assert!(moves(&target(gear, "rel physics:hinge0 = ")).ends_with("/G1_1"), "{text}");
    assert!(moves(&target(gear, "rel physics:hinge1 = ")).ends_with("/G2_1"), "{text}");
    let rack = text.find("def PhysxPhysicsRackAndPinionJoint").unwrap();
    let ratio = 0.1 * 1000.0 * 180.0 / std::f64::consts::PI;
    assert!((value_after(&text, rack, "float physics:ratio = ")[0] - ratio).abs() < 1e-3, "{text}");
    assert!(moves(&target(rack, "rel physics:hinge = ")).ends_with("/G1_1"), "{text}");
    assert!(moves(&target(rack, "rel physics:prismatic = ")).ends_with("/Rack_1"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A gear coupling joins the two gears, the parts its hinges turn, whichever side of each hinge
/// the grounded base is on: here B of one hinge and A of the other.
#[test]
fn a_gear_coupling_joins_the_gears_whichever_side_the_base_is_on() {
    let dir = temp_dir("gears-sides");
    let mut s = Session::default();
    let part = |s: &mut Session, name: &str, solid: &str, p: Value| {
        run(s, "component.activate", json!({"component": "root"}));
        run(s, "component.create", json!({"name": name, "grounded": false}));
        run(s, solid, p);
    };
    part(&mut s, "Base", "solid.box", json!({"length": 120, "width": 60, "height": 10, "corner": [-30, -30, -20]}));
    for (name, x, r) in [("G1", 0, 10), ("G2", 30, 20)] {
        part(&mut s, name, "solid.cylinder", json!({"base": [x, 0, 0], "axis": [0, 0, 1], "radius": r, "height": 5}));
    }
    run(&mut s, "component.activate", json!({"component": "root"}));
    let base = s.doc.occurrence_of(s.doc.find_component("Base").unwrap()).unwrap().id;
    run(&mut s, "occurrence.ground", json!({"occurrence": base, "grounded": true}));
    let at = |o: &str, p: [f64; 3]| json!({"occurrence": o, "point": p, "z": [0.0, 0.0, 1.0]});
    run(&mut s, "joint.create", json!({"type": "revolute", "a": at("Base", [0.0; 3]), "b": at("G1", [0.0; 3]), "flip": true, "name": "H1"}));
    let p2 = [30.0, 0.0, 0.0];
    run(&mut s, "joint.create", json!({"type": "revolute", "a": at("G2", p2), "b": at("Base", p2), "flip": true, "name": "H2"}));
    run(&mut s, "joint.gear", json!({"a": "H1", "b": "H2", "ra": 10, "rb": 20}));
    let path = dir.join("gears.usda");
    let reply = run(&mut s, "file.export", json!({"path": path.to_string_lossy(), "physics": true}));
    let text = std::fs::read_to_string(&path).unwrap();
    let at = text.find("def PhysxPhysicsGearJoint").unwrap_or_else(|| panic!("no gear joint: {reply}\n{text}"));
    let rest = &text[at + 1..];
    let gear = &rest[..rest.find("\n    def ").unwrap_or(rest.len())];
    let body = |rel: &str| -> String {
        let line = gear[gear.find(rel).unwrap_or_else(|| panic!("no {rel}:\n{gear}"))..].lines().next().unwrap();
        line[line.find('<').unwrap() + 1..line.find('>').unwrap()].to_string()
    };
    let bodies = [body("rel physics:body0 = "), body("rel physics:body1 = ")];
    assert!(bodies.iter().any(|b| b.ends_with("/G1_1")) && bodies.iter().any(|b| b.ends_with("/G2_1")), "{bodies:?}\n{gear}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A built-in surface bound in a render setup is written into an OpenUSD package with
/// `render_setup`: its four images are members and the body's mesh has coordinates.
#[cfg(feature = "bundled-render-library")]
#[test]
fn a_render_setups_builtin_surface_is_packaged() {
    let dir = temp_dir("surface");
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 2, "body_name": "Coupon"}));
    run(
        &mut s,
        "render.library.surface.bind",
        json!({"name": "Maps", "set": {"job": {"method": "gpu_path", "options": {}}}, "surface": {"id": "SURF-001", "body_index": 0, "body_name": "Coupon"}}),
    );
    let path = dir.join("coupon.usdz");
    let reply = run(&mut s, "file.export", json!({"path": path.to_string_lossy(), "render_setup": "Maps"}));
    // The reply leaves out an empty list of warnings.
    let warnings: Vec<String> = serde_json::from_value(reply["warnings"].clone()).unwrap_or_default();
    assert!(warnings.is_empty(), "{warnings:?}");
    // A package is a ZIP archive: its texture members are entries with image bytes.
    let bytes = std::fs::read(&path).unwrap();
    let members = usdz_members(&bytes);
    let textures: Vec<&String> = members.keys().filter(|name| name.starts_with("textures/")).collect();
    assert_eq!(textures.len(), 4, "{textures:?}");
    for name in textures {
        let image = &members[name];
        assert!(image.starts_with(b"\x89PNG") || image.starts_with(&[0xFF, 0xD8]), "{name}: no image bytes");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Without the built-in surfaces' images in the build, the body is written with its look, and a
/// warning says so; a setup that does not exist is refused.
#[test]
fn a_render_setup_without_its_images_warns_and_a_missing_setup_is_refused() {
    let dir = temp_dir("nosurface");
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 2, "body_name": "Coupon"}));
    let path = dir.join("coupon.usdz");
    let refused = s.execute("file.export", &json!({"path": path.to_string_lossy(), "render_setup": "Nowhere"}));
    assert!(refused.unwrap_err().to_string().contains("no render setup named `Nowhere`"));
    let refused = s.execute("file.export", &json!({"path": dir.join("c.stl").to_string_lossy(), "render_setup": "Maps"}));
    assert!(refused.unwrap_err().to_string().contains("`render_setup` is for OpenUSD"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The `points` of mesh prim `prim` in `text`, in millimetres.
fn decal_points(text: &str, prim: &str) -> Vec<[f64; 3]> {
    // The decal's material has its name too: the mesh prim is the one to read.
    let at = text.find(&format!("def Mesh \"{prim}\"")).unwrap_or_else(|| panic!("no {prim} mesh:\n{text}"));
    let rest = &text[at..];
    let start = rest.find("point3f[] points = [").unwrap() + "point3f[] points = [".len();
    let body = &rest[start..start + rest[start..].find(']').unwrap()];
    let values: Vec<f64> = body.split(['(', ')', ',']).filter_map(|v| v.trim().parse().ok()).collect();
    values.chunks(3).map(|p| [p[0] * 1e3, p[1] * 1e3, p[2] * 1e3]).collect()
}

/// With `images`, an OpenUSD package holds each shown picture as a two-sided quad with its image,
/// in its component, 0.2 mm off the face on each side; the image is a member of the package. A
/// hidden picture is not written, nor any picture without `images`, and a text layer warns.
#[test]
fn images_write_the_shown_pictures_into_a_package() {
    use image::ImageEncoder;
    let dir = temp_dir("decals");
    let mut s = Session::default();
    for (name, x) in [("A", 0), ("B", 20)] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
    }
    run(&mut s, "component.activate", json!({"component": "B"}));
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png).write_image(&[200u8; 4 * 8], 4, 2, image::ExtendedColorType::Rgba8).unwrap();
    let file = picture_file(&dir, &png);
    run(&mut s, "canvas.decal", json!({"path": file, "face": [25, 5, 10], "width": 8, "name": "Logo"}));
    run(&mut s, "canvas.insert", json!({"path": file, "plane": "XY", "width": 8, "name": "Hidden"}));
    run(&mut s, "canvas.edit", json!({"canvas": "Hidden", "visible": false}));

    let path = dir.join("decals.usdz");
    let reply = run(&mut s, "file.export", json!({"path": path.to_string_lossy(), "images": true}));
    let warnings: Vec<String> = serde_json::from_value(reply["warnings"].clone()).unwrap_or_default();
    assert!(warnings.is_empty(), "{warnings:?}");
    // A package is a ZIP archive: the layer is an entry, and the picture is a member with bytes.
    let bytes = std::fs::read(&path).unwrap();
    let members = usdz_members(&bytes);
    let text = String::from_utf8(members["root.usda"].clone()).unwrap();
    let image = format!("textures/{}.png", solidcraft_doc::render_resources::HdrId::of(&png));
    assert!(members.get(&image).is_some_and(|data| !data.is_empty()), "no {image} member");
    assert!(!text.contains("\"Hidden\""), "the hidden picture is written");
    // Worked out by hand: B is placed where it was built. The decal is 8 x 4 mm (the image is 2:1)
    // about (25, 5) on B's top face at z = 10, along the face's axes either way round; the front
    // quad 0.2 mm above the face, the back quad 0.2 mm below.
    let corners = decal_points(&text, "Logo");
    assert_eq!(corners.len(), 8, "{corners:?}");
    let plan = |c: &[[f64; 3]]| {
        let mut xy: Vec<(i64, i64)> = c.iter().map(|p| ((p[0] * 1e3).round() as i64, (p[1] * 1e3).round() as i64)).collect();
        xy.sort();
        xy
    };
    let along_x = [(21000, 3000), (21000, 7000), (29000, 3000), (29000, 7000)];
    let along_y = [(23000, 1000), (23000, 9000), (27000, 1000), (27000, 9000)];
    let front = plan(&corners[..4]);
    assert!(front == along_x || front == along_y, "{corners:?}");
    assert_eq!(plan(&corners[4..]), front);
    assert!(corners[..4].iter().all(|p| (p[2] - 10.2).abs() < 1e-3) && corners[4..].iter().all(|p| (p[2] - 9.8).abs() < 1e-3), "{corners:?}");

    let plain = dir.join("plain.usdz");
    run(&mut s, "file.export", json!({"path": plain.to_string_lossy()}));
    assert!(!String::from_utf8_lossy(&std::fs::read(&plain).unwrap()).contains("\"Logo\""));
    let layer = dir.join("decals.usda");
    let reply = run(&mut s, "file.export", json!({"path": layer.to_string_lossy(), "images": true}));
    let warnings: Vec<String> = serde_json::from_value(reply["warnings"].clone()).unwrap_or_default();
    assert!(warnings.iter().any(|w| w.contains("written only in a .usdz package")), "{warnings:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// With `render_setup`, a body the setup gives optics is written as glass: the preview surface's
/// index of refraction, and the MaterialX shader's full transmission, with the colour 1 mm of the
/// absorption leaves (e^(−a)) at a depth of 1 mm; a warning says a reader of the preview surface
/// alone shows it opaque.
#[test]
fn render_setup_optics_are_written_as_transmission() {
    let dir = temp_dir("optics");
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 3, "body_name": "Pane"}));
    run(
        &mut s,
        "render.settings.set",
        json!({"name": "Glass", "set": {"width": 4, "height": 4, "job": {"method": "cpu_raytrace", "options": {"lights": [],
               "environment": {"color": [1, 1, 1], "intensity": 1}, "progressive": {"samples_per_pixel": 1, "seed": 1}}}}}),
    );
    let revision = s.revision;
    run(
        &mut s,
        "render.optics.bind",
        json!({"name": "Glass", "body_index": 0, "body_name": "Pane", "expected_revision": revision,
               "optics": {"ior": 1.52, "absorption_per_mm": [0.1, 0.0, 2.0], "boundary": "closed_mesh"}}),
    );
    let path = dir.join("glass.usda");
    let reply = run(&mut s, "file.export", json!({"path": path.to_string_lossy(), "render_setup": "Glass"}));
    let warnings: Vec<String> = serde_json::from_value(reply["warnings"].clone()).unwrap_or_default();
    assert!(
        warnings.iter().any(|w| w.starts_with("Pane: the optics of render setup `Glass` are written as the MaterialX shader's transmission")),
        "{warnings:?}"
    );
    let text = std::fs::read_to_string(&path).unwrap();
    let preview = text.find("info:id = \"UsdPreviewSurface\"").unwrap_or_else(|| panic!("no preview surface:\n{text}"));
    assert!((value_after(&text, preview, "float inputs:ior = ")[0] - 1.52).abs() < 1e-6, "{text}");
    let pbr = text.find("info:id = \"ND_open_pbr_surface_surfaceshader\"").unwrap_or_else(|| panic!("no OpenPBR shader:\n{text}"));
    assert_eq!(value_after(&text, pbr, "float inputs:transmission_weight = "), [1.0]);
    let colour = value_after(&text, pbr, "color3f inputs:transmission_color = ");
    assert_eq!(colour.len(), 3, "{colour:?}");
    for (got, want) in colour.iter().zip([(-0.1f64).exp(), 1.0, (-2.0f64).exp()]) {
        assert!((got - want).abs() < 1e-6, "{colour:?}");
    }
    assert!((value_after(&text, pbr, "float inputs:transmission_depth = ")[0] - 0.001).abs() < 1e-9, "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A design with a picture on a 10 mm cube in component B, and a loose cube at the root.
fn pictured(dir: &std::path::Path) -> Session {
    use image::ImageEncoder;
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [-30, 0, 0], "body_name": "Loose"}));
    run(&mut s, "component.create", json!({"name": "B"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0]}));
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png).write_image(&[200u8; 4 * 8], 4, 2, image::ExtendedColorType::Rgba8).unwrap();
    run(&mut s, "canvas.decal", json!({"path": picture_file(dir, &png), "face": [25, 5, 10], "width": 8, "name": "Logo"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    s
}

/// `purpose: distribution` writes meshes only, with the pictures shown on the model;
/// `interchange` (the default) writes the exact shapes and no pictures; `exact_brep` and
/// `images` override the purpose; `purpose` is for OpenUSD only.
#[test]
fn a_distribution_export_is_meshes_with_pictures() {
    let dir = temp_dir("purpose");
    let mut s = pictured(&dir);
    let export = |s: &mut Session, name: &str, extra: Value| {
        let path = dir.join(name);
        let mut p = json!({"path": path.to_string_lossy()});
        for (k, v) in extra.as_object().unwrap() {
            p[k] = v.clone();
        }
        run(s, "file.export", p);
        String::from_utf8_lossy(&std::fs::read(&path).unwrap()).to_string()
    };
    let interchange = export(&mut s, "interchange.usdz", json!({}));
    assert!(interchange.contains("def BrepArray") && !interchange.contains("\"Logo\""), "{interchange}");
    let distribution = export(&mut s, "distribution.usdz", json!({"purpose": "distribution"}));
    assert!(!distribution.contains("BrepArray"), "{distribution}");
    assert_eq!(distribution.matches("def Mesh").count(), 3, "two cubes and the picture:\n{distribution}");
    assert!(distribution.contains("def Mesh \"Logo\""), "{distribution}");
    let exact = export(&mut s, "exact.usdz", json!({"purpose": "distribution", "exact_brep": true, "images": false}));
    assert!(exact.contains("def BrepArray") && !exact.contains("\"Logo\""), "{exact}");
    let stl = dir.join("x.stl");
    let refused = s.execute("file.export", &json!({"path": stl.to_string_lossy(), "purpose": "distribution"}));
    assert!(refused.unwrap_err().to_string().contains("`purpose` is for OpenUSD"));
    let refused = s.execute("file.export", &json!({"path": dir.join("y.usda").to_string_lossy(), "purpose": "print"}));
    assert!(refused.unwrap_err().to_string().contains("`purpose` must be interchange or distribution"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn openusd_exports_pass_openusd_validators() {
    let dir = temp_dir("validate");
    let mut s = pictured(&dir);
    let b = s.doc.occurrence_of(s.doc.find_component("B").unwrap()).unwrap().id;
    run(&mut s, "occurrence.ground", json!({"occurrence": b, "grounded": true}));
    run(&mut s, "browser.visibility", json!({"items": ["Loose"], "visible": false}));
    let mut files = Vec::new();
    for (name, options) in [
        ("interchange.usda", json!({})),
        ("interchange.usdc", json!({})),
        ("distribution.usdz", json!({"purpose": "distribution"})),
        ("physics.usda", json!({"physics": true})),
    ] {
        let path = dir.join(name);
        let mut p = options;
        p["path"] = json!(path.to_string_lossy());
        run(&mut s, "file.export", p);
        files.push(path);
    }
    // `SOLIDCRAFT_USD_PYTHON` names a Python that has the bindings: then nothing may skip.
    let required = std::env::var("SOLIDCRAFT_USD_PYTHON").ok();
    let python = required.clone().unwrap_or_else(|| "python3".into());
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/usd-validate.py");
    let out = std::process::Command::new(&python).arg(&script).args(["--ignore", "MissingShaderIdInRegistry"]).args(&files).output();
    match out {
        // Exit 3 is the script's "no bindings, nothing checked"; any other failure, a signal
        // included (no exit code), or a file that does not open (2) fails.
        Ok(out) if required.is_none() && out.status.code() == Some(3) => {
            eprintln!("skipped: {python} has no OpenUSD bindings: {}", String::from_utf8_lossy(&out.stderr));
        }
        Ok(out) => assert!(out.status.success(), "{}: {}{}", out.status, String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)),
        Err(e) => {
            assert!(required.is_none(), "{python} does not run: {e}");
            eprintln!("skipped: {python} does not run: {e}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
