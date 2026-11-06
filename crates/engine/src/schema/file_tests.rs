use std::time::Duration;

use serde_json::{Value, json};

use super::{
    Export3mfParams, ExportMeshParams, ExportParams, ExportStepParams, ExportStlParams, ExportUrdfParams, FileExportResult, NewerFileReport,
    OpenElsewhere, OpenParams, OpenResult, OpenedDesign, RecoverParams, RecoverResult, RecoveryDiscardParams, RecoveryDiscardResult,
    RecoveryListParams, RecoveryListResult,
};
use crate::Session;
use crate::recovery::Autosaver;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<ExportStepParams>(), &["bodies", "path"]);
    assert_keys(input_schema_for::<ExportMeshParams>(), &["bodies", "path", "quality"]);
    assert_keys(input_schema_for::<ExportStlParams>(), &["bodies", "lay_on_print_faces", "path", "quality"]);
    assert_keys(input_schema_for::<Export3mfParams>(), &["bodies", "lay_on_print_faces", "objects", "path", "quality"]);
    assert_keys(input_schema_for::<ExportUrdfParams>(), &["collision", "effort", "mesh_prefix", "mimic", "path", "velocity"]);
    assert_keys(
        input_schema_for::<ExportParams>(),
        &["bodies", "collision", "effort", "format", "lay_on_print_faces", "mesh_prefix", "mimic", "objects", "path", "quality", "velocity"],
    );
    assert_keys(
        input_schema_for::<OpenParams>(),
        &["folder", "history", "max_points", "names", "origin", "path", "read_only", "reuse_import", "units", "up", "verify"],
    );
    assert_keys(input_schema_for::<RecoveryListParams>(), &["dir"]);
    assert_keys(input_schema_for::<RecoverParams>(), &["dir", "discard", "id"]);
    assert_keys(input_schema_for::<RecoveryDiscardParams>(), &["all", "dir", "id"]);
}

/// The words the open contract takes are the ones the handler's readers take, and its limits
/// are the point cloud command's.
#[test]
fn open_values_are_read_as_the_handler_reads_them() {
    let ok = |p: Value| serde_json::from_value::<OpenParams>(p).is_ok();
    assert!(ok(json!({"path": "a.step", "up": " Y ", "names": "Product"})));
    assert!(ok(json!({"path": "a.step", "up": null, "names": null, "read_only": null})));
    assert!(ok(json!({"path": "a.ply", "units": "FT", "max_points": 20_000_000})));
    assert!(!ok(json!({"path": "a.ply", "units": "yd"})));
    assert!(!ok(json!({"path": "a.ply", "max_points": 0})));
    assert!(!ok(json!({"path": "a.ply", "max_points": 20_000_001})));
    assert!(!ok(json!({"path": "a.ply", "max_points": 1.5})));
    assert!(!ok(json!({"path": "x".repeat(4096)})));
    assert!(ok(json!({"path": "x".repeat(4095)})));
    let units: Vec<&str> = solidcraft_doc::point_cloud::UNITS.iter().map(|(u, _)| *u).collect();
    let schema = input_schema_for::<OpenParams>();
    assert_eq!(schema.as_value()["$defs"]["CloudUnits"]["enum"], json!(units));
    // The same words as pointcloud.insert says them.
    let mut s = Session::default();
    for (key, bad) in [("units", json!("yd")), ("max_points", json!(0))] {
        let said = s.execute("pointcloud.insert", &json!({"path": "a.ply", key: bad})).unwrap_err().to_string();
        let mine = serde_json::from_value::<OpenParams>(json!({"path": "a.ply", key: bad})).unwrap_err().to_string();
        let words = mine.split(" at line").next().unwrap_or_default();
        assert!(said.contains(words), "{said} / {words}");
    }
}

fn dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("solidcraft-open-schemas-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap_or_else(|e| panic!("{e}"));
    d
}

/// Every kind of file `doc.open` opens, and the recovery commands, in Warn: each reply matches its
/// output schema and carries no warning; a misspelt key's warning still matches.
#[test]
fn replies_match_the_output_schema() {
    let d = dir("replies");
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let run = |s: &mut Session, id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    let matches = |schema: schemars::Schema, id: &str, reply: &Value| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, reply), "{id}: {reply} doesn't match {schema}");
    };
    let opened = |reply: &Value| {
        assert!(reply.get("param_warnings").is_none(), "{reply}");
        matches(output_schema_for::<OpenResult>(), "doc.open", reply);
    };
    let at = |name: &str| d.join(name).to_string_lossy().to_string();

    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    // A design, plain and read-only.
    run(&mut s, "file.save_as", json!({"path": at("part.sc")}));
    let design = run(&mut s, "doc.open", json!({"path": at("part.sc")}));
    assert_eq!((design["features"].as_u64(), design.get("read_only")), (Some(1), None), "{design}");
    opened(&design);
    let read_only = run(&mut s, "doc.open", json!({"path": at("part.sc"), "read_only": true}));
    assert_eq!(read_only["read_only"], true, "{read_only}");
    opened(&read_only);
    // STEP, with its options, and meshes.
    run(&mut s, "file.export_step", json!({"path": at("part.step")}));
    for options in [json!({}), json!({"up": "y", "names": "Product"}), json!({"up": "Z", "names": null})] {
        let mut p = json!({"path": at("part.step")});
        if let Some(o) = p.as_object_mut() {
            o.extend(options.as_object().cloned().unwrap_or_default());
        }
        let step = run(&mut s, "doc.open", p);
        assert!(step["component"].is_u64() && step["bodies"].is_array(), "{step}");
        assert!(step.get("system").is_some() && step.get("occurrence").is_some() && step.get("feature").is_none(), "{step}");
        opened(&step);
    }
    let turned = run(&mut s, "doc.open", json!({"path": at("part.step"), "up": "y"}));
    assert!(turned["stand_up"].is_u64(), "{turned}");
    run(&mut s, "file.export_stl", json!({"path": at("part.stl")}));
    run(&mut s, "file.export_3mf", json!({"path": at("part.3mf")}));
    run(&mut s, "file.export_obj", json!({"path": at("part.obj")}));
    for name in ["part.stl", "part.3mf", "part.obj"] {
        let mesh = run(&mut s, "doc.open", json!({"path": at(name)}));
        assert!(mesh["component"].is_u64() && mesh.get("system").is_none(), "{name}: {mesh}");
        opened(&mesh);
    }
    // An STL file in another unit.
    let scaled = run(&mut s, "doc.open", json!({"path": at("part.stl"), "units": "cm"}));
    assert!(scaled["component"].is_u64(), "{scaled}");
    opened(&scaled);
    // A drawing, as DXF and as SVG.
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [30, 20]}));
    run(&mut s, "sketch.export_dxf", json!({"path": at("drawing.dxf")}));
    run(&mut s, "sketch.finish", json!({}));
    std::fs::write(
        at("drawing.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="30"><rect x="0" y="0" width="40" height="30"/></svg>"#,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    for name in ["drawing.dxf", "drawing.svg"] {
        let drawing = run(&mut s, "doc.open", json!({"path": at(name)}));
        assert!(drawing["sketch"].is_u64() && drawing.get("created_sketch").is_some(), "{name}: {drawing}");
        let mut keys: Vec<&str> = drawing.as_object().map(|o| o.keys().map(String::as_str).collect()).unwrap_or_default();
        keys.sort();
        assert_eq!(keys, ["created_sketch", "curves", "errors", "features", "info", "name", "path", "sketch", "texts"], "{drawing}");
        opened(&drawing);
    }
    // A point cloud.
    std::fs::write(at("cloud.xyz"), "0 0 0\n1 0 0\n0 1 0\n0 0 1\n").unwrap_or_else(|e| panic!("{e}"));
    let cloud = run(&mut s, "doc.open", json!({"path": at("cloud.xyz"), "units": "cm", "max_points": 100}));
    assert_eq!(cloud["points"], 4, "{cloud}");
    let mut keys: Vec<&str> = cloud.as_object().map(|o| o.keys().map(String::as_str).collect()).unwrap_or_default();
    keys.sort();
    assert_eq!(keys, ["bounds", "cloud", "errors", "features", "path", "points", "source_points", "warnings"], "{cloud}");
    opened(&cloud);
    // Another app's lock, and a misspelt key.
    let busy = OpenedDesign {
        path: "a.sc".into(),
        features: 0,
        errors: 0,
        name: "a".into(),
        read_only: None,
        open_elsewhere: Some(OpenElsewhere { host: "h".into(), user: "u".into(), app: "1".into(), opened: 7 }),
        newer_file: None,
        warnings: None,
        param_warnings: Vec::new(),
        signature: None,
    };
    matches(output_schema_for::<OpenResult>(), "doc.open", &serde_json::to_value(OpenResult::Design(busy)).unwrap_or_default());
    // A design a newer SolidCraft saved: the notice and what wasn't read.
    let newer = OpenedDesign {
        path: "a.sc".into(),
        features: 0,
        errors: 0,
        name: "a".into(),
        read_only: None,
        open_elsewhere: None,
        newer_file: Some(NewerFileReport {
            format: 99,
            reads_up_to: 50,
            unknown_changes: vec!["gyroid_lattice".into()],
            not_understood: vec!["feature 3".into()],
            notice: "Saved by a newer SolidCraft".into(),
        }),
        warnings: Some(vec!["Saved by a newer SolidCraft".into(), "Not read: feature 3. Saving keeps it.".into()]),
        param_warnings: Vec::new(),
        signature: None,
    };
    matches(output_schema_for::<OpenResult>(), "doc.open", &serde_json::to_value(OpenResult::Design(newer)).unwrap_or_default());
    let warned = run(&mut s, "doc.open", json!({"path": at("part.sc"), "pathh": 1}));
    assert!(warned["param_warnings"].as_array().is_some_and(|w| !w.is_empty()), "{warned}");
    matches(output_schema_for::<OpenResult>(), "doc.open", &warned);

    // The recovery commands, on a folder with two entries.
    let rec = at("recovery");
    let mut ids = Vec::new();
    for _ in 0..2 {
        let mut made = Session::default();
        made.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap_or_else(|e| panic!("{e}"));
        let mut saver = Autosaver::new(std::path::Path::new(&rec), Duration::from_secs(3600)).unwrap_or_else(|e| panic!("{e}"));
        assert!(saver.save(&made).unwrap_or(false));
        ids.push(saver.id.clone());
    }
    let list = run(&mut s, "doc.recovery_list", json!({"dir": rec}));
    assert!(list.get("param_warnings").is_none() && list["designs"].as_array().map(Vec::len) == Some(2), "{list}");
    matches(output_schema_for::<RecoveryListResult>(), "doc.recovery_list", &list);
    let empty = run(&mut s, "doc.recovery_list", json!({"dir": at("nothing")}));
    assert_eq!(empty["designs"], json!([]), "{empty}");
    matches(output_schema_for::<RecoveryListResult>(), "doc.recovery_list", &empty);
    let default = run(&mut s, "doc.recovery_list", json!({}));
    matches(output_schema_for::<RecoveryListResult>(), "doc.recovery_list", &default);
    let recovered = run(&mut s, "doc.recover", json!({"dir": rec, "id": ids[0]}));
    assert!(recovered.get("param_warnings").is_none() && recovered["features"] == 1, "{recovered}");
    matches(output_schema_for::<RecoverResult>(), "doc.recover", &recovered);
    let recovered = run(&mut s, "doc.recover", json!({"dir": rec, "id": ids[0], "discard": true}));
    matches(output_schema_for::<RecoverResult>(), "doc.recover", &recovered);
    let one = run(&mut s, "doc.recovery_discard", json!({"dir": rec, "id": ids[1]}));
    assert!(one.get("param_warnings").is_none() && one["discarded"] == json!([ids[1]]), "{one}");
    matches(output_schema_for::<RecoveryDiscardResult>(), "doc.recovery_discard", &one);
    let all = run(&mut s, "doc.recovery_discard", json!({"dir": rec, "all": true}));
    assert_eq!(all["discarded"], json!([]), "{all}");
    matches(output_schema_for::<RecoveryDiscardResult>(), "doc.recovery_discard", &all);
    let warned = run(&mut s, "doc.recovery_discard", json!({"dir": rec, "all": true, "al": true}));
    assert!(warned["param_warnings"].as_array().is_some_and(|w| !w.is_empty()), "{warned}");
    matches(output_schema_for::<RecoveryDiscardResult>(), "doc.recovery_discard", &warned);
    let _ = std::fs::remove_dir_all(&d);
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

/// Mark a print face on the first body of `s`, a box: its side at the lowest X.
fn mark_a_print_face(s: &mut Session) {
    let (name, bbox) = {
        let world = s.world_state().unwrap_or_else(|e| panic!("{e}"));
        let body = world.bodies.first().expect("a body");
        (body.name.clone(), solidcraft_kernel::measure_mass(&body.body).expect("measure").bbox)
    };
    let point = [bbox.min.x, (bbox.min.y + bbox.max.y) / 2.0, (bbox.min.z + bbox.max.z) / 2.0];
    run(s, "body.print_face", json!({"body": name, "point": point}));
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
        ("file.export", json!({"path": at("a.step"), "lay_on_print_faces": true})),
        ("file.export", json!({"path": at("a.obj"), "lay_on_print_faces": true})),
        ("file.export", json!({"path": at("a.stl"), "lay_on_print_faces": "yes"})),
        ("file.export", json!({"path": at("a.3mf"), "lay_on_print_faces": 1})),
        ("file.export_stl", json!({"path": at("a.stl"), "lay_on_print_faces": "yes"})),
        ("file.export_3mf", json!({"path": at("a.3mf"), "lay_on_print_faces": [true]})),
        ("doc.open", json!({"path": ""})),
        ("doc.open", json!({"path": "  "})),
        ("doc.open", json!({"path": 5})),
        ("doc.open", json!({"path": null})),
        ("doc.open", json!({"path": "part.step", "up": "x"})),
        ("doc.open", json!({"path": "part.step", "up": 5})),
        ("doc.open", json!({"path": "part.glb", "up": "sideways"})),
        ("doc.open", json!({"path": "part.step", "names": "x"})),
        ("doc.open", json!({"path": "part.step", "names": 5})),
        ("doc.recover", json!({"id": 5})),
        ("doc.recover", json!({"id": null})),
        ("doc.recover", json!({"id": "nothing-here"})),
        ("doc.recovery_discard", json!({})),
        ("doc.recovery_discard", json!({"all": false})),
        ("doc.recovery_discard", json!({"id": "nothing-here"})),
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
    // `lay_on_print_faces`: true for STL and 3MF only, false for any format, not read for URDF.
    for path in ["a.stl", "a.3mf"] {
        assert_eq!(ok(json!({"path": path, "lay_on_print_faces": true})).lay_on_print_faces, Some(true), "{path}");
    }
    assert_eq!(ok(json!({"path": "a", "format": "stla", "lay_on_print_faces": true})).lay_on_print_faces, Some(true));
    assert_eq!(ok(json!({"path": "a.step", "lay_on_print_faces": false})).lay_on_print_faces, Some(false));
    assert!(ok(json!({"path": "a.urdf", "lay_on_print_faces": "yes"})).lay_on_print_faces.is_none());
    for path in ["a.step", "a.obj", "a.glb", "a.json"] {
        let e = read(json!({"path": path, "lay_on_print_faces": true})).expect_err(path);
        assert!(e.contains("STL and 3MF"), "{path}: {e}");
    }
    let e = read(json!({"path": "a.stl", "lay_on_print_faces": "yes"})).expect_err("a string");
    assert!(e.contains("must be true or false"), "{e}");
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
        (json!({"path": "a.stl", "lay_on_print_faces": true}), true),
        (json!({}), false),
        (json!({"path": "a.stl", "lay_on_print_faces": "yes"}), false),
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
        // With a print face marked: `print_faces` in the first, and in the second a warning (the
        // body is a part of a component's object, so it stays as it lies).
        json!({"path": at("i.stl"), "lay_on_print_faces": true}),
        json!({"path": at("j.3mf"), "objects": "component", "lay_on_print_faces": true}),
    ];
    for level in [ParamCheck::Warn, ParamCheck::Strict] {
        let mut s = two_cubes();
        s.param_check = level;
        mark_a_print_face(&mut s);
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
