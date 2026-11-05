use std::time::Duration;

use serde_json::{Value, json};

use super::{
    Export3mfParams, ExportMeshParams, ExportStepParams, ExportUrdfParams, NewerFileReport, OpenElsewhere, OpenParams, OpenResult, OpenedDesign,
    RecoverParams, RecoverResult, RecoveryDiscardParams, RecoveryDiscardResult, RecoveryListParams, RecoveryListResult,
};
use crate::Session;
use crate::recovery::Autosaver;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<ExportStepParams>(), &["bodies", "path"]);
    assert_keys(input_schema_for::<ExportMeshParams>(), &["bodies", "path", "quality"]);
    assert_keys(input_schema_for::<Export3mfParams>(), &["bodies", "objects", "path", "quality"]);
    assert_keys(input_schema_for::<ExportUrdfParams>(), &["effort", "mesh_prefix", "path", "velocity"]);
    assert_keys(input_schema_for::<OpenParams>(), &["max_points", "names", "path", "read_only", "units", "up"]);
    assert_keys(input_schema_for::<RecoveryListParams>(), &["dir"]);
    assert_keys(input_schema_for::<RecoverParams>(), &["dir", "discard", "id"]);
    assert_keys(input_schema_for::<RecoveryDiscardParams>(), &["all", "dir", "id"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
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
