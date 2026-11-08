use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::{InsertMeshParams, MeshInserted, StepNamesParam, UpAxis};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<InsertMeshParams>(), &["folder", "name", "names", "origin", "path", "units", "up"]);
}

#[test]
fn strict_keeps_todays_errors() {
    let long = "x".repeat(4096);
    assert_strict_keeps_todays_errors(&[
        ("file.insert_mesh", json!({})),
        ("file.insert_mesh", json!({"path": ""})),
        ("file.insert_mesh", json!({"path": "  "})),
        ("file.insert_mesh", json!({"path": 5})),
        ("file.insert_mesh", json!({"path": null})),
        ("file.insert_mesh", json!({"path": long})),
        ("file.insert_mesh", json!({"up": "q", "units": "yd"})),
        // The kind of file is refused before anything else is read.
        ("file.insert_mesh", json!({"path": "x.step"})),
        ("file.insert_mesh", json!({"path": "x.step", "up": "q", "units": 5, "origin": 1})),
        ("file.insert_mesh", json!({"path": "x.step", "name": 5})),
        // `up` is for a glTF file only, whatever it says.
        ("file.insert_mesh", json!({"path": "x.stl", "up": "y"})),
        ("file.insert_mesh", json!({"path": "x.stl", "up": "q"})),
        ("file.insert_mesh", json!({"path": "x.obj", "up": 5})),
        ("file.insert_mesh", json!({"path": "x.glb", "up": "q"})),
        ("file.insert_mesh", json!({"path": "x.glb", "up": 5})),
        ("file.insert_mesh", json!({"path": "x.glb", "up": "y", "names": "bogus"})),
        ("file.insert_mesh", json!({"path": "x.gltf", "up": "z", "names": 5})),
        ("file.insert_mesh", json!({"path": "x.gltf", "up": "z", "folder": "yes"})),
        ("file.insert_mesh", json!({"path": "x.stl", "units": "yd"})),
        ("file.insert_mesh", json!({"path": "x.stl", "units": 5})),
        ("file.insert_mesh", json!({"path": "x.stl", "units": true})),
        ("file.insert_mesh", json!({"path": "x.3mf", "units": "mm"})),
        ("file.insert_mesh", json!({"path": "x.glb", "units": "mm"})),
        ("file.insert_mesh", json!({"path": "x.stl", "origin": "yes"})),
        ("file.insert_mesh", json!({"path": "x.stl", "origin": 1})),
        // The kind of file is read in any case, and a PLY file is a mesh too.
        ("file.insert_mesh", json!({"path": "X.GLB", "up": "q"})),
        ("file.insert_mesh", json!({"path": "X.STL", "up": "y"})),
        ("file.insert_mesh", json!({"path": "X.3MF", "units": "mm"})),
        ("file.insert_mesh", json!({"path": "x.ply", "units": "yd"})),
        ("file.insert_mesh", json!({"path": "x.ply", "up": "y"})),
        // A file that isn't there.
        ("file.insert_mesh", json!({"path": "does-not-exist.stl"})),
        ("file.insert_mesh", json!({"path": "does-not-exist.glb", "up": "Z", "names": " Auto ", "folder": false})),
    ]);
}

fn parse(p: Value) -> Result<InsertMeshParams, String> {
    serde_json::from_value(p).map_err(|e| e.to_string())
}

/// The keys read as the handler reads them: the same words in any case, `null` as left out, and
/// the keys it doesn't read in a call left unread, so a bad one there is no problem.
#[test]
fn the_types_read_each_key_as_the_handler_does() {
    let keys = |p: Value| parse(p).unwrap_or_else(|e| panic!("{e}")).0;
    let k = keys(json!({"path": "a.glb", "name": "Mine", "up": " Y ", "names": " Solid ", "folder": true, "units": "IN", "origin": false}));
    assert_eq!(k.path.as_deref(), Some("a.glb"));
    assert_eq!(k.name.as_deref(), Some("Mine"));
    assert_eq!(k.up, Some(UpAxis::Y));
    assert_eq!(k.names, Some(StepNamesParam::Solid));
    assert_eq!(k.folder, Some(true));
    assert_eq!(k.units.map(|u| u.0), Some("in".to_string()));
    assert_eq!(k.origin, Some(false));
    assert_eq!(keys(json!({"path": "a.glb", "up": "Z"})).up, Some(UpAxis::Z));
    let none = keys(json!({"path": "a.stl", "name": null, "up": null, "names": null, "folder": null, "units": null, "origin": null}));
    assert!(none.name.is_none() && none.up.is_none() && none.units.is_none() && none.origin.is_none());

    // Not read: `names` and `folder` without an `up` for a glTF file, `up` for any other file,
    // everything but `path` for a file that isn't a mesh.
    for p in [
        json!({"path": "a.stl", "names": 5, "folder": "x"}),
        json!({"path": "a.glb", "names": 5, "folder": "x"}),
        json!({"path": "a.glb", "up": null, "names": 5, "folder": "x"}),
        json!({"path": "a.stl", "up": "sideways", "names": 5}),
        json!({"path": "a.step", "up": 1, "names": 5, "folder": 5, "units": 5, "origin": 5}),
    ] {
        parse(p.clone()).unwrap_or_else(|e| panic!("{p}: {e}"));
    }
    parse(json!({"path": "a.GLB", "up": "y", "names": "auto"})).unwrap_or_else(|e| panic!("{e}"));
    assert!(parse(json!({"path": "a.GLB", "up": "y", "names": "x"})).is_err());
    parse(json!({"path": "a.PLY", "up": 5, "names": 5})).unwrap_or_else(|e| panic!("{e}"));
    let k = keys(json!({"path": "a.glb", "names": 5}));
    assert!(k.names.is_none() && k.folder.is_none());

    // Read, and refused with the handler's message.
    let refused = |p: Value| crate::schema::issues("file.insert_mesh", &p).into_iter().next().map(|i| i.message).unwrap_or_default();
    for (p, message) in [
        (json!({"path": "a.glb", "up": "x"}), "`up` must be \"z\" (the file's axes are kept) or \"y\" (the file has Y up: its Y becomes Z)"),
        (json!({"path": "a.glb", "up": "y", "names": "x"}), "`names` must be \"auto\", \"solid\", \"product\" or \"instance\""),
        (json!({"path": "a.stl", "units": "yd"}), "`units` must be one of mm, cm, m, in, ft (not `yd`)"),
        (json!({"path": "a.stl", "units": 1}), "`units` must be one of mm, cm, m, in, ft"),
        (json!({"path": "a.3mf", "units": []}), "`units` must be one of mm, cm, m, in, ft"),
        (json!({"path": "a.stl", "origin": "yes"}), "`origin` must be true"),
        (json!({"path": "a.glb", "up": "z", "folder": 0}), "`folder` must be true"),
        (json!({}), "`path` is required"),
        (json!({"path": " "}), "`path` is required"),
        (json!({"up": "x", "path": 5}), "`path` is required"),
    ] {
        let e = refused(p.clone());
        assert!(e.starts_with(message), "{p}: {e}");
    }
}

/// The check names what is wrong, in the handler's words, and a key the handler would ignore is
/// still a known key.
#[test]
fn the_check_names_what_is_wrong() {
    let issues = |p: Value| crate::schema::issues("file.insert_mesh", &p).into_iter().map(|i| i.message).collect::<Vec<_>>();
    assert_eq!(issues(json!({"path": "a.stl", "unit": "in"})), ["unknown key `unit`; did you mean `units`?"]);
    assert_eq!(issues(json!({"path": "a.stl", "units": "yd"})), ["`units` must be one of mm, cm, m, in, ft (not `yd`)"]);
    assert_eq!(issues(json!({"path": "a.stl", "name": 5})), ["`name` must be text"]);
    assert_eq!(issues(json!({})), ["`path` is required"]);
    // `name` is read as text or not at all; a number is ignored by the handler but is a mistake.
    assert_eq!(issues(json!({"path": "a.stl", "name": null})), Vec::<String>::new());
    assert!(issues(json!({"path": "a.stl", "up": "y", "names": 5})).is_empty(), "an `up` for another file is the handler's to refuse");
    assert!(issues(json!({"path": "a.stl", "names": 5, "folder": 5})).is_empty());
}

/// The published schema: `path` is required and shaped as the handler reads it; `names` and
/// `folder` have their shapes only where they are read.
#[test]
fn the_schema_takes_what_the_handler_reads() {
    let schema = input_schema_for::<InsertMeshParams>();
    let schema = schema.as_value();
    for (p, takes) in [
        (json!({"path": "a.stl"}), true),
        (json!({"path": "a.glb", "up": "Y", "names": "Auto", "folder": true, "name": "N"}), true),
        (json!({"path": "a.stl", "units": "in", "origin": true}), true),
        (json!({"path": "a.stl", "units": null, "origin": null, "up": null}), true),
        // Left unread, so not shaped.
        (json!({"path": "a.stl", "names": 5, "folder": "x"}), true),
        (json!({"path": "a.glb", "up": null, "names": 5, "folder": "x"}), true),
        (json!({}), false),
        (json!({"path": 5}), false),
        (json!({"path": "a.stl", "origin": "yes"}), false),
        (json!({"path": "a.stl", "units": 5}), false),
        (json!({"path": "a.stl", "folder": "x", "up": "y"}), false),
        (json!({"path": "a.glb", "up": "y", "names": 5}), false),
        (json!({"path": "a.stl", "name": 5}), false),
    ] {
        assert_eq!(conforms(schema, schema, &p), takes, "{p}");
    }
}

/// `conforms` doesn't read patterns: pin the text of the ones the schema carries.
#[test]
fn the_patterns_are_pinned() {
    let schema = input_schema_for::<InsertMeshParams>();
    let schema = schema.as_value();
    assert_eq!(schema["properties"]["path"]["pattern"], json!(crate::schema::types::not_blank_pattern()));
    assert_eq!(schema["properties"]["path"]["maxLength"], json!(4095));
    let ws = crate::schema::types::whitespace_class();
    assert_eq!(schema["$defs"]["UpAxis"]["pattern"], json!(format!("^[{ws}]*([zZ]|[yY])[{ws}]*$")));
    assert_eq!(schema["$defs"]["MeshUnits"]["pattern"], json!("^([mM][mM]|[cC][mM]|[mM]|[iI][nN]|[fF][tT])$"));
    assert_eq!(
        schema["$defs"]["StepNamesParam"]["pattern"],
        json!(format!("^[{ws}]*([aA][uU][tT][oO]|[sS][oO][lL][iI][dD]|[pP][rR][oO][dD][uU][cC][tT]|[iI][nN][sS][tT][aA][nN][cC][eE])[{ws}]*$"))
    );
}

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("solidcraft-insert-mesh-schema-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    dir
}

/// A cube of side 10 mm, saved as STL, and the same as 3MF.
fn cube_files(dir: &Path) -> (String, String) {
    let mut part = Session::default();
    let (stl, threemf) = (dir.join("cube.stl").to_string_lossy().to_string(), dir.join("cube.3mf").to_string_lossy().to_string());
    for (id, p) in [
        ("solid.box", json!({"length": 10, "width": 10, "height": 10})),
        ("file.save_mesh", json!({"path": stl})),
        ("file.export_3mf", json!({"path": threemf})),
    ] {
        part.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    }
    (stl, threemf)
}

/// A glTF file (a glb): a cube of side 0.01 m placed by a `Base` node with an `Arm` node below
/// it, so the import has components of its own.
fn glb(dir: &Path) -> String {
    let s = 0.01f32;
    let p: Vec<[f32; 3]> = (0..8).map(|i| [(i & 1) as f32 * s, ((i >> 1) & 1) as f32 * s, ((i >> 2) & 1) as f32 * s]).collect();
    let t: [u16; 36] = [0, 2, 1, 1, 2, 3, 4, 5, 6, 5, 7, 6, 0, 1, 4, 1, 5, 4, 2, 6, 3, 3, 6, 7, 0, 4, 2, 2, 4, 6, 1, 3, 5, 3, 7, 5];
    let mut bin: Vec<u8> = p.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
    bin.extend(t.iter().flat_map(|v| v.to_le_bytes()));
    let json = json!({
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [
            {"name": "Base", "mesh": 0, "children": [1]},
            {"name": "Arm", "mesh": 1, "translation": [0.0, 0.05, 0.0]},
        ],
        "meshes": [
            {"name": "Block", "primitives": [{"attributes": {"POSITION": 0}, "indices": 1}]},
            {"name": "Peg", "primitives": [{"attributes": {"POSITION": 0}, "indices": 1}]},
        ],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": 8, "type": "VEC3", "min": [0.0, 0.0, 0.0], "max": [s, s, s]},
            {"bufferView": 1, "componentType": 5123, "count": 36, "type": "SCALAR"},
        ],
        "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": 96}, {"buffer": 0, "byteOffset": 96, "byteLength": 72}],
        "buffers": [{"byteLength": bin.len()}],
    });
    let path = dir.join("robot.glb");
    std::fs::write(&path, solidcraft_io::glb_container(json.to_string().as_bytes(), &bin)).unwrap_or_else(|e| panic!("{e}"));
    path.to_string_lossy().to_string()
}

fn matches(reply: &Value) {
    let schema = output_schema_for::<MeshInserted>();
    let schema = schema.as_value();
    assert!(conforms(schema, schema, reply), "{reply} doesn't match {schema}");
}

/// Every form the command takes, run in Warn: each reply matches its output schema and carries
/// no warning (Warn runs a call the contract refuses anyway, so this shows the contract takes
/// each call). The keys the handler leaves unread are given bad values on purpose.
#[test]
fn replies_match_the_output_schema() {
    let dir = folder("replies");
    let (stl, threemf) = cube_files(&dir);
    let robot = glb(&dir);
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let mut run = |p: Value| {
        let r = s.execute("file.insert_mesh", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(r.get("param_warnings").is_none(), "{p}: {r}");
        matches(&r);
        r
    };
    let plain = run(json!({"path": stl}));
    assert_eq!(plain["component_name"], "cube", "{plain}");
    assert!(plain.get("units").is_none() && plain.get("moved_to_origin").is_none() && plain.get("stand_up").is_none(), "{plain}");
    assert!(plain.get("subcomponents").is_none() && plain["bodies"] == plain["created_bodies"], "{plain}");
    assert!(plain["occurrence"].is_u64(), "{plain}");

    let named = run(json!({"path": stl, "name": "Block", "units": "IN", "origin": true}));
    assert_eq!(named["units"], "in", "{named}");
    assert_eq!(named["component_name"], "Block", "{named}");
    assert!(named["moved_to_origin"].as_array().is_some_and(|a| a.len() == 3), "{named}");
    run(json!({"path": stl, "units": "cm", "origin": false, "up": null, "name": null}));
    // Keys left unread, with values that would be refused if they were read.
    run(json!({"path": stl, "names": 5, "folder": "x"}));
    run(json!({"path": threemf, "names": [], "folder": {}}));
    run(json!({"path": robot, "names": 5, "folder": "x"}));

    let stood = run(json!({"path": robot}));
    assert!(stood["stand_up"].is_u64() && stood["subcomponents"].is_u64() && stood["rigid_joints"].is_u64(), "{stood}");
    let kept = run(json!({"path": robot, "up": "Z", "names": " Instance ", "folder": true, "origin": true}));
    assert!(kept.get("stand_up").is_none() && kept["subcomponents"].is_u64(), "{kept}");
    let turned = run(json!({"path": robot, "up": " y "}));
    assert!(turned["stand_up"].is_u64(), "{turned}");

    // A misspelt key still runs in Warn, says so, and the reply still matches.
    let warned = s.execute("file.insert_mesh", &json!({"path": stl, "unit": "in"})).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(warned["param_warnings"], json!(["unknown key `unit`; did you mean `units`?"]), "{warned}");
    matches(&warned);
    let _ = std::fs::remove_dir_all(&dir);
}

/// In Strict a call that works today is not refused, and one that fails fails with the
/// handler's message (see `strict_keeps_todays_errors`).
#[test]
fn strict_runs_the_calls_that_work() {
    let dir = folder("strict");
    let (stl, _) = cube_files(&dir);
    let robot = glb(&dir);
    let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
    for p in [json!({"path": stl, "units": "mm"}), json!({"path": robot, "up": "y", "names": "auto", "folder": false})] {
        s.execute("file.insert_mesh", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
    }
    let e = s.execute("file.insert_mesh", &json!({"path": stl, "unit": "in"})).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(e.contains("unknown key `unit`"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}
