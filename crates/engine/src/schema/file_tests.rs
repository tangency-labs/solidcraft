use serde_json::{Value, json};

use super::{Export3mfParams, ExportMeshParams, ExportParams, ExportStepParams, ExportUrdfParams, FileExportResult};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<ExportStepParams>(), &["bodies", "path"]);
    assert_keys(input_schema_for::<ExportMeshParams>(), &["bodies", "path", "quality"]);
    assert_keys(input_schema_for::<Export3mfParams>(), &["bodies", "objects", "path", "quality"]);
    assert_keys(input_schema_for::<ExportUrdfParams>(), &["effort", "mesh_prefix", "path", "velocity"]);
    assert_keys(input_schema_for::<ExportParams>(), &["bodies", "effort", "format", "mesh_prefix", "objects", "path", "quality", "velocity"]);
}

#[test]
fn bad_calls_say_what_is_wrong_and_write_nothing() {
    let dir = std::env::temp_dir().join(format!("solidcraft-export-schemas-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap_or_else(|e| panic!("{e}"));
    for cmd in ["file.export_step", "file.export_stl", "file.export_3mf", "file.export_obj", "file.export_urdf"] {
        let e = s.execute(cmd, &json!({})).unwrap_err().to_string();
        assert!(e.contains(cmd) && e.contains("path"), "{cmd}: {e}");
    }
    let at = dir.join("a.stl").to_string_lossy().to_string();
    let e = s.execute("file.export_stl", &json!({"path": at, "quality": "rough"})).unwrap_err().to_string();
    assert!(e.contains("quality"), "{e}");
    s.param_check = ParamCheck::Strict;
    let e = s.execute("file.export_stl", &json!({"path": at, "objects": "component"})).unwrap_err().to_string();
    assert!(e.contains("objects"), "objects is for 3MF only: {e}");
    s.param_check = ParamCheck::Off;
    let e = s.execute("file.export_urdf", &json!({"path": at, "effort": -1})).unwrap_err().to_string();
    assert!(e.contains("effort"), "{e}");
    assert_eq!(std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0), 0, "nothing written");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A fresh folder for one test's files.
fn folder(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("solidcraft-export-schema-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    dir
}

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// 10 mm cubes in components A (grounded) and B, so a URDF and a 3MF with parts can be written.
fn two_cubes() -> Session {
    let mut s = Session::default();
    for (name, x) in [("A", 0), ("B", 20)] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
        run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2, "corner": [x, 0, 10], "operation": "new"}));
    }
    let a = s.doc.find_component("A").and_then(|c| s.doc.occurrence_of(c)).map(|o| o.id).unwrap_or(0);
    run(&mut s, "occurrence.ground", json!({"occurrence": a, "grounded": true}));
    s
}

#[test]
fn strict_keeps_todays_errors() {
    let dir = folder("strict");
    let at = |name: &str| dir.join(name).to_string_lossy().to_string();
    assert_strict_keeps_todays_errors(&[
        ("file.export", json!({})),
        ("file.export", json!({"path": ""})),
        ("file.export", json!({"path": "  "})),
        ("file.export", json!({"path": 5})),
        ("file.export", json!({"path": null})),
        ("file.export", json!({"path": "x".repeat(4096)})),
        ("file.export", json!({"path": at("a.xyz")})),
        ("file.export", json!({"path": at("a.stl"), "format": "xyz"})),
        ("file.export", json!({"path": at("a.xyz"), "quality": "rough"})),
        ("file.export", json!({"path": at("a.stl"), "quality": "rough"})),
        ("file.export", json!({"path": at("a.stl"), "quality": 3})),
        ("file.export", json!({"path": at("a.stl"), "objects": "component"})),
        ("file.export", json!({"path": at("a.3mf"), "objects": "parts"})),
        ("file.export", json!({"path": at("a.stl"), "bodies": [1]})),
        ("file.export", json!({"path": at("a.stl"), "bodies": 1})),
        ("file.export", json!({"path": at("a.json"), "bodies": ["Body1"]})),
        ("file.export", json!({"path": at("a.urdf"), "bodies": ["Body1"]})),
        ("file.export", json!({"path": at("a.urdf"), "effort": -1})),
        ("file.export", json!({"path": at("a.urdf"), "effort": 0})),
        ("file.export", json!({"path": at("a.urdf"), "velocity": "fast"})),
        ("file.export", json!({"path": at("a"), "format": "urdf", "velocity": 1e9})),
    ]);
    assert_eq!(std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0), 0, "nothing written");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Each format's keys are read only for that format, as the handler reads them: a key another
/// format takes is not checked, and `format` picks the format before the path's extension does.
#[test]
fn the_export_type_reads_each_key_only_for_its_format() {
    let read = |p: Value| serde_json::from_value::<ExportParams>(p).map_err(|e| e.to_string());
    let ok = |p: Value| read(p.clone()).unwrap_or_else(|e| panic!("{p}: {e}"));
    let p = ok(json!({"path": "a.urdf", "quality": "rough", "objects": 3, "effort": 5}));
    assert!(p.quality.is_none() && p.objects.is_none() && p.effort == Some(5.0), "{p:?}");
    let p = ok(json!({"path": "a.stl", "effort": -1, "velocity": "fast", "mesh_prefix": 3, "quality": " Fine "}));
    assert!(p.effort.is_none() && p.velocity.is_none() && p.mesh_prefix.is_none() && p.quality.is_some(), "{p:?}");
    assert!(ok(json!({"path": "a.stl", "format": "3mf", "objects": "component"})).objects.is_some());
    assert!(read(json!({"path": "a.3mf", "format": "stl", "objects": "component"})).is_err());
    assert!(read(json!({"path": "a", "format": "urdf", "effort": 0})).is_err());
    // An unknown format, or no path to write to, is the handler's to refuse: nothing else is read.
    assert!(ok(json!({"path": "a.xyz", "quality": "rough", "effort": -1})).quality.is_none());
    assert!(ok(json!({"quality": "rough"})).path.is_none());
    assert!(ok(json!({"path": "a.stl", "bodies": null, "format": null, "quality": null, "objects": null})).bodies.is_none());
}

/// (`conforms` does not read `oneOf`, so `quality` and `objects` values are checked by the type,
/// in `the_export_type_reads_each_key_only_for_its_format`.)
#[test]
fn the_export_schema_takes_what_the_handler_reads() {
    let schema = input_schema_for::<ExportParams>();
    let schema = schema.as_value();
    for (p, takes) in [
        (json!({"path": "a.stl"}), true),
        (json!({"path": "a.3mf", "format": "3mf-tagged", "bodies": ["A", "B"], "quality": "fine", "objects": "component"}), true),
        (json!({"path": "robot.urdf", "mesh_prefix": "package://robot/meshes/", "effort": 50, "velocity": 2.5}), true),
        (json!({"path": "a.step", "bodies": "Body1"}), true),
        (json!({}), false),
        (json!({"path": "a.stl", "bodies": [1]}), false),
        (json!({"path": "a.stl", "colour": true}), false),
    ] {
        assert_eq!(conforms(schema, schema, &p), takes, "{p}");
    }
}

/// Real exports of every format in warn: no `param_warnings`, and each reply matches the output
/// schema. The same calls go through in strict.
#[test]
fn export_replies_match_the_output_schema() {
    let dir = folder("replies");
    let at = |name: &str| dir.join(name).to_string_lossy().to_string();
    let schema = output_schema_for::<FileExportResult>();
    let schema = schema.as_value();
    let calls = [
        json!({"path": at("a.stl")}),
        json!({"path": at("b.stl"), "format": "stla", "quality": "finest"}),
        json!({"path": at("c.obj"), "quality": "fine"}),
        json!({"path": at("d.step")}),
        json!({"path": at("e.3mf"), "objects": "component"}),
        json!({"path": at("f.bin"), "format": "3mf-tagged"}),
        json!({"path": at("g.json")}),
        json!({"path": at("h.urdf"), "mesh_prefix": "package://robot/meshes/", "effort": 50, "velocity": 2}),
    ];
    for level in [ParamCheck::Warn, ParamCheck::Strict] {
        let mut s = two_cubes();
        s.param_check = level;
        for p in &calls {
            let reply = run(&mut s, "file.export", p.clone());
            assert!(reply.get("param_warnings").is_none(), "{p}: {reply}");
            assert!(conforms(schema, schema, &reply), "{p}: {reply} doesn't match {schema}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A misspelt key is reported in warn and refused in strict, before anything is written; a key of
/// the wrong type the handler reads as left out is reported too.
#[test]
fn a_misspelt_or_wrong_typed_export_key_is_reported() {
    let dir = folder("misspelt");
    let at = |name: &str| dir.join(name).to_string_lossy().to_string();
    let mut s = two_cubes();
    s.param_check = ParamCheck::Warn;
    let reply = run(&mut s, "file.export", json!({"path": at("a.stl"), "qualty": "fine"}));
    let warnings = reply["param_warnings"].to_string();
    assert!(warnings.contains("qualty"), "{reply}");
    let reply = run(&mut s, "file.export", json!({"path": at("b.stl"), "format": 3}));
    assert!(reply["param_warnings"].to_string().contains("format"), "{reply}");
    s.param_check = ParamCheck::Strict;
    let e = s.execute("file.export", &json!({"path": at("c.stl"), "qualty": "fine"})).unwrap_err().to_string();
    assert!(e.contains("qualty"), "{e}");
    assert!(!dir.join("c.stl").exists(), "nothing written");
    let _ = std::fs::remove_dir_all(&dir);
}
