//! glTF files import as mesh bodies (#1505): in a component named after the file, the node tree
//! as nested components held as one rigid group, a mesh placed again another occurrence of one
//! component, metres as millimetres, Y up turned to Z up (unless `up: z`), and the materials as
//! the bodies' appearances. Every file here is built by the test.

use serde_json::{Value, json};

use super::run;
use crate::*;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("solidcraft-gltf-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A glTF document with one buffer holding a cube of side 0.01 m (10 mm) at the origin.
struct Cube {
    bin: Vec<u8>,
    json: Value,
}

fn cube_file() -> Cube {
    let s = 0.01f32;
    let p: Vec<[f32; 3]> = (0..8).map(|i| [(i & 1) as f32 * s, ((i >> 1) & 1) as f32 * s, ((i >> 2) & 1) as f32 * s]).collect();
    let t: [u16; 36] = [0, 2, 1, 1, 2, 3, 4, 5, 6, 5, 7, 6, 0, 1, 4, 1, 5, 4, 2, 6, 3, 3, 6, 7, 0, 4, 2, 2, 4, 6, 1, 3, 5, 3, 7, 5];
    let mut bin: Vec<u8> = p.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
    bin.extend(t.iter().flat_map(|v| v.to_le_bytes()));
    let json = json!({
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": []}],
        "nodes": [],
        "meshes": [
            {"name": "Block", "primitives": [{"attributes": {"POSITION": 0}, "indices": 1, "material": 0}]},
            {"name": "Peg", "primitives": [{"attributes": {"POSITION": 0}, "indices": 1, "material": 1}]},
        ],
        "materials": [
            {"name": "Brass", "pbrMetallicRoughness": {"baseColorFactor": [0.8, 0.5, 0.1, 1.0], "metallicFactor": 1.0, "roughnessFactor": 0.3}},
            {"name": "Blue", "pbrMetallicRoughness": {"baseColorFactor": [0.0, 0.0, 1.0, 1.0], "metallicFactor": 0.0, "roughnessFactor": 0.8}},
        ],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": 8, "type": "VEC3", "min": [0.0, 0.0, 0.0], "max": [s, s, s]},
            {"bufferView": 1, "componentType": 5123, "count": 36, "type": "SCALAR"},
        ],
        "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": 96}, {"buffer": 0, "byteOffset": 96, "byteLength": 72}],
        "buffers": [{"byteLength": bin.len()}],
    });
    Cube { bin, json }
}

impl Cube {
    fn glb(&self, dir: &std::path::Path, name: &str) -> String {
        let path = dir.join(name);
        std::fs::write(&path, solidcraft_io::glb_container(self.json.to_string().as_bytes(), &self.bin)).unwrap();
        path.to_string_lossy().to_string()
    }
}

/// World bounding boxes of every shown body, rounded to 0.001 mm, sorted.
fn shown_boxes(s: &Session) -> Vec<[i64; 6]> {
    let mut out: Vec<[i64; 6]> = s
        .world_state()
        .unwrap()
        .bodies
        .iter()
        .map(|b| {
            let bb = solidcraft_kernel::measure(&b.body).unwrap().bbox;
            [bb.min.x, bb.min.y, bb.min.z, bb.max.x, bb.max.y, bb.max.z].map(|v| (v * 1000.0).round() as i64)
        })
        .collect();
    out.sort();
    out
}

fn mm(b: [f64; 6]) -> [i64; 6] {
    b.map(|v| (v * 1000.0).round() as i64)
}

fn components(s: &mut Session) -> Vec<Value> {
    run(s, "component.list", json!({}))["components"].as_array().unwrap().clone()
}

/// `robot.glb`: a `Base` (Block, at x = 0.1 m) with an `Arm` (Peg, 0.05 m up Y, a quarter turn
/// about Y), and three `Bolt` nodes all placing the Peg mesh (0.03 m apart along X).
fn robot(dir: &std::path::Path, up: Option<&str>) -> (Session, Value, String) {
    let mut c = cube_file();
    c.json["nodes"] = json!([
        {"name": "Base", "mesh": 0, "translation": [0.1, 0.0, 0.0], "children": [1]},
        {"name": "Arm", "mesh": 1, "translation": [0.0, 0.05, 0.0], "rotation": [0.0, 0.70710677, 0.0, 0.70710677]},
        {"name": "Bolt", "mesh": 1, "translation": [0.0, 0.0, 0.0]},
        {"name": "Bolt", "mesh": 1, "translation": [0.03, 0.0, 0.0]},
        {"name": "Bolt", "mesh": 1, "translation": [0.06, 0.0, 0.0]},
    ]);
    c.json["scenes"][0]["nodes"] = json!([0, 2, 3, 4]);
    let path = c.glb(dir, "robot.glb");
    let mut s = Session::default();
    let mut p = json!({"path": path});
    if let Some(up) = up {
        p["up"] = json!(up);
    }
    let r = run(&mut s, "file.insert_mesh", p);
    (s, r, path)
}

#[test]
fn a_gltf_node_tree_comes_in_as_components_with_instances() {
    let dir = temp_dir("tree");
    let (mut s, r, path) = robot(&dir, Some("z"));
    assert!(r.get("stand_up").is_none(), "{r}");
    let list = components(&mut s);
    let names: Vec<&str> = list.iter().filter_map(|c| c["name"].as_str()).collect();
    // The arm and the bolts place the same mesh the same way inside: one component, named
    // after the mesh.
    for n in ["robot", "Base", "Peg"] {
        assert!(names.contains(&n), "{n} in {names:?}");
    }
    assert_eq!(r["component_name"], "robot");
    assert_eq!(r["subcomponents"], 2, "{r}");
    let id = |n: &str| list.iter().find(|c| c["name"] == n).unwrap()["id"].as_u64().unwrap();
    // Placed four times; its mesh is built once.
    assert_eq!(s.doc.occurrences.iter().filter(|o| o.component == id("Peg")).count(), 4);
    assert_eq!(s.model.state().bodies.len(), 2, "Block and Peg, once each");
    let peg_parent = list.iter().find(|c| c["name"] == "Peg").unwrap()["parent"].as_u64().unwrap();
    assert_eq!(peg_parent, id("Base"), "made where it is first placed");
    // Metres are millimetres, and each body is where the file puts it. The arm is turned a
    // quarter about Y: its X becomes -Z.
    let want = vec![
        mm([0.0, 0.0, 0.0, 10.0, 10.0, 10.0]),
        mm([30.0, 0.0, 0.0, 40.0, 10.0, 10.0]),
        mm([60.0, 0.0, 0.0, 70.0, 10.0, 10.0]),
        mm([100.0, 0.0, 0.0, 110.0, 10.0, 10.0]),
        mm([100.0, 50.0, -10.0, 110.0, 60.0, 0.0]),
    ];
    assert_eq!(shown_boxes(&s), want);
    // Materials are the bodies' looks, with their metal and roughness.
    let brass = s.doc.appearances.bodies.get("Block").expect("the block's look");
    assert_eq!(brass.name, "Brass");
    assert_eq!(brass.color, [231, 188, 89]);
    let pbr = brass.pbr.expect("PBR");
    assert!((pbr.metallic() - 1.0).abs() < 1e-9 && (pbr.roughness() - 0.3).abs() < 1e-6);
    assert_eq!(s.doc.appearances.bodies.values().filter(|l| l.name == "Blue").count(), 1);

    // Rigid until unlocked.
    let robot = id("robot");
    assert_eq!(s.contents_joints(robot).len(), 5, "one for each placement");
    let r = run(&mut s, "component.unlock", json!({"component": "robot"}));
    assert_eq!(r["joints"].as_array().unwrap().len(), 5, "{r}");
    assert!(s.contents_joints(robot).is_empty());
    run(&mut s, "edit.undo", json!({}));

    // Saved and opened again, it is the same (nothing new is saved for glTF).
    let design = dir.join("robot.solidcraft").to_string_lossy().to_string();
    run(&mut s, "file.save_as", json!({"path": design}));
    let mut again = Session::default();
    run(&mut again, "doc.open", json!({"path": design}));
    assert_eq!(shown_boxes(&again), want);
    assert_eq!(again.doc.appearances.bodies.get("Block").map(|l| l.name.as_str()), Some("Brass"));

    // Opened by itself, the file is a new design of that component.
    let mut opened = Session::default();
    let r = run(&mut opened, "doc.open", json!({"path": path, "up": "z"}));
    assert_eq!(r["component_name"], "robot", "{r}");
    assert_eq!(shown_boxes(&opened), want);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn gltf_y_up_stands_up_by_default() {
    let dir = temp_dir("up");
    let (s, r, _) = robot(&dir, None);
    assert!(r["stand_up"].as_u64().is_some(), "{r}");
    // Y up becomes Z up: a body at file y in 50..60 mm is at z 50..60.
    let boxes = shown_boxes(&s);
    assert!(boxes.contains(&mm([100.0, 0.0, 50.0, 110.0, 10.0, 60.0])), "{boxes:?}");
    assert!(boxes.iter().all(|b| b[2] >= -1), "nothing below the ground: {boxes:?}");
    let e = Session::default().execute("file.insert_mesh", &json!({"path": "x.stl", "up": "y"})).unwrap_err().to_string();
    assert!(e.contains("glTF"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_gltf_with_its_bin_next_to_it_opens_and_broken_files_say_why() {
    let dir = temp_dir("bin");
    let mut c = cube_file();
    c.json["nodes"] = json!([{"name": "Only", "mesh": 0}]);
    c.json["scenes"][0]["nodes"] = json!([0]);
    c.json["buffers"][0]["uri"] = json!("cube.bin");
    std::fs::write(dir.join("cube.bin"), &c.bin).unwrap();
    let path = dir.join("part.gltf").to_string_lossy().to_string();
    std::fs::write(&path, c.json.to_string()).unwrap();
    let mut s = Session::default();
    let r = run(&mut s, "doc.open", json!({"path": path}));
    assert_eq!(r["bodies"], json!(["Block"]), "{r}");
    assert_eq!(s.doc.name, "part");
    assert_eq!(shown_boxes(&s), vec![mm([0.0, -10.0, 0.0, 10.0, 0.0, 10.0])], "turned Z up about X");

    // The .bin missing, the file truncated: errors that say so, and the design is untouched.
    std::fs::remove_file(dir.join("cube.bin")).unwrap();
    let before = s.doc.to_json();
    let e = s.execute("file.insert_mesh", &json!({"path": path})).unwrap_err().to_string();
    assert!(e.contains("cube.bin"), "{e}");
    let glb = dir.join("cut.glb");
    let whole = solidcraft_io::glb_container(c.json.to_string().as_bytes(), &c.bin);
    std::fs::write(&glb, &whole[..whole.len() / 2]).unwrap();
    let e = s.execute("file.insert_mesh", &json!({"path": glb.to_string_lossy()})).unwrap_err().to_string();
    assert!(e.contains("truncated") || e.contains("GLB"), "{e}");
    assert_eq!(s.doc.to_json(), before);
    let _ = std::fs::remove_dir_all(&dir);
}
