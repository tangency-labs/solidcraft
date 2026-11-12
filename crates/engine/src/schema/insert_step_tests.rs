use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_kernel::{ExportBody, ExportProduct, StepHeader, box_solid, step_export_products};

use super::{InsertStepParams, StepInserted};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<InsertStepParams>(), &["folder", "matrix", "name", "names", "path", "tolerance", "up"]);
}

#[test]
fn strict_keeps_todays_errors() {
    let long = "x".repeat(4096);
    assert_strict_keeps_todays_errors(&[
        ("file.insert_step", json!({})),
        ("file.insert_step", json!({"path": ""})),
        ("file.insert_step", json!({"path": "  "})),
        ("file.insert_step", json!({"path": 5})),
        ("file.insert_step", json!({"path": null})),
        ("file.insert_step", json!({"path": long})),
        // A file of another kind is refused before any other key is read.
        ("file.insert_step", json!({"path": "x.stl"})),
        ("file.insert_step", json!({"path": "x.stl", "up": "q", "names": 5, "folder": "x", "matrix": 5, "tolerance": 5})),
        ("file.insert_step", json!({"path": "x.3mf", "name": 5})),
        ("file.insert_step", json!({"path": "x.txt", "up": "y"})),
        ("file.insert_step", json!({"path": "X.STL", "up": "y"})),
        // The keys of a CAD file, read in the handler's order.
        ("file.insert_step", json!({"path": "a.step", "up": "q"})),
        ("file.insert_step", json!({"path": "a.step", "up": 5})),
        ("file.insert_step", json!({"path": "a.stp", "up": "y", "names": "bogus"})),
        ("file.insert_step", json!({"path": "a.igs", "names": 5})),
        ("file.insert_step", json!({"path": "a.iges", "names": []})),
        ("file.insert_step", json!({"path": "a.fcstd", "folder": "yes"})),
        ("file.insert_step", json!({"path": "a.3dm", "folder": 1})),
        // `matrix` is the handler's to read; `tolerance` only with it.
        ("file.insert_step", json!({"path": "a.step", "matrix": "turn it"})),
        ("file.insert_step", json!({"path": "a.step", "matrix": [2, 0, 0, 0, 0, 2, 0, 0, 0, 0, 2, 0]})),
        ("file.insert_step", json!({"path": "a.step", "matrix": [1, 2, 3], "tolerance": "loose"})),
        ("file.insert_step", json!({"path": "a.step", "matrix": [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], "tolerance": 0})),
        ("file.insert_step", json!({"path": "a.step", "matrix": [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], "tolerance": 0.01})),
        // A file that isn't there.
        ("file.insert_step", json!({"path": "does-not-exist.step"})),
        ("file.insert_step", json!({"path": "does-not-exist.step", "up": "Z", "names": " Auto ", "folder": false})),
    ]);
}

fn parse(p: Value) -> Result<InsertStepParams, String> {
    serde_json::from_value(p).map_err(|e| e.to_string())
}

/// The keys read as the handler reads them: the same words in any case, `null` as left out, and
/// the keys it doesn't read in a call left unread, so a bad one there is no problem.
#[test]
fn the_types_read_each_key_as_the_handler_does() {
    let keys = |p: Value| parse(p).unwrap_or_else(|e| panic!("{e}")).0;
    let k = keys(json!({"path": "a.step", "name": "Mine", "up": " Y ", "names": " Solid ", "folder": true}));
    assert_eq!(k.path.as_deref(), Some("a.step"));
    assert_eq!(k.name.as_deref(), Some("Mine"));
    assert_eq!(k.up, Some(super::UpParam(true)));
    assert_eq!(k.names.map(|names| names.0), Some(solidcraft_io::StepNames::Solid));
    assert_eq!(k.folder, Some(true));
    assert_eq!(keys(json!({"path": "a.iges", "up": "Z"})).up, Some(super::UpParam(false)));
    assert_eq!(keys(json!({"path": "a.3dm", "names": " Instance "})).names.map(|names| names.0), Some(solidcraft_io::StepNames::Instance));
    assert!(keys(json!({"path": "a.step"})).folder.is_none());
    let none = keys(json!({"path": "a.step", "name": null, "up": null, "names": null, "folder": null, "matrix": null, "tolerance": null}));
    assert!(none.name.is_none() && none.up.is_none() && none.names.is_none() && none.folder.is_none() && none.matrix.is_none());
    for matrix in [json!([0, -1, 0, 100, 1, 0, 0, 0, 0, 0, 1, 0]), json!({"m": 1}), json!("turn")] {
        assert_eq!(keys(json!({"path": "a.step", "matrix": matrix.clone()})).matrix.map(|m| m.0), Some(matrix.clone()), "{matrix}");
    }
    assert_eq!(
        keys(json!({"path": "a.step", "matrix": [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], "tolerance": 0.001})).tolerance.map(|t| t.0),
        Some(json!(0.001))
    );

    // Not read: `tolerance` without a `matrix`, and everything but `path` for a file the command
    // refuses before it reads another key.
    for p in [
        json!({"path": "a.step", "tolerance": "loose"}),
        json!({"path": "a.step", "matrix": null, "tolerance": []}),
        json!({"path": "a.stl", "up": "sideways", "names": 5, "folder": 5, "matrix": 5, "tolerance": 5}),
        json!({"path": "x.txt", "name": 5, "up": 1, "names": 5, "folder": 5, "matrix": 5, "tolerance": 5}),
    ] {
        parse(p.clone()).unwrap_or_else(|e| panic!("{p}: {e}"));
    }
    parse(json!({"path": "A.STP", "up": "y", "names": "auto", "folder": false})).unwrap_or_else(|e| panic!("{e}"));
    assert!(parse(json!({"path": "A.STP", "up": "y", "names": "x"})).is_err());
    let k = keys(json!({"path": "a.step", "tolerance": 5}));
    assert!(k.tolerance.is_none() && k.matrix.is_none());

    // With `matrix` it is read as given, as `occurrence.move` reads it: the published schema
    // shapes it, and `matrix::tolerance` is the one that refuses a bad one, in its own words
    // (see `strict_keeps_todays_errors`).
    let with_matrix = json!({"path": "a.step", "matrix": [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], "tolerance": "loose"});
    assert_eq!(keys(with_matrix.clone()).tolerance.map(|t| t.0), Some(json!("loose")));
    assert!(crate::schema::issues("file.insert_step", &with_matrix).is_empty(), "the check leaves the refusal to the handler");

    // Read, and refused with the handler's message.
    let refused = |p: Value| crate::schema::issues("file.insert_step", &p).into_iter().next().map(|i| i.message).unwrap_or_default();
    for (p, message) in [
        (json!({"path": "a.step", "up": "x"}), "`up` must be \"z\" (the file's axes are kept) or \"y\" (the file has Y up: its Y becomes Z)"),
        (json!({"path": "a.step", "up": "y", "names": "x"}), "`names` must be \"auto\", \"solid\", \"product\" or \"instance\""),
        (
            json!({"path": "a.step", "folder": 0}),
            "`folder` must be true (an import that brings more than one item goes into a `Group #N` history folder) or false (no folder)",
        ),
        (json!({}), "`path` is required"),
        (json!({"path": " "}), "`path` is required"),
        (json!({"up": "x", "path": 5}), "`path` is required"),
        (json!({"up": "x", "names": 5, "folder": 5}), "`path` is required"),
    ] {
        let e = refused(p.clone());
        assert!(e.starts_with(message), "{p}: {e}");
    }
}

/// The check names what is wrong, in the handler's words, and a key the handler would ignore is
/// still a known key.
#[test]
fn the_check_names_what_is_wrong() {
    let issues = |p: Value| crate::schema::issues("file.insert_step", &p).into_iter().map(|i| i.message).collect::<Vec<_>>();
    assert_eq!(issues(json!({"path": "a.step", "toleranc": 1})), ["unknown key `toleranc`; did you mean `tolerance`?"]);
    assert_eq!(
        issues(json!({"path": "a.step", "up": "q"})),
        ["`up` must be \"z\" (the file's axes are kept) or \"y\" (the file has Y up: its Y becomes Z)"]
    );
    assert_eq!(issues(json!({"path": "a.step", "name": 5})), ["`name` must be text"]);
    assert_eq!(issues(json!({})), ["`path` is required"]);
    // `name` is read as text or not at all; a number is ignored by the handler but is a mistake.
    assert_eq!(issues(json!({"path": "a.step", "name": null})), Vec::<String>::new());
    // The kind of file, and whether a matrix is a rigid move, are the handler's to refuse.
    assert!(issues(json!({"path": "a.stl", "up": "y", "names": 5, "folder": "x", "matrix": 5})).is_empty());
    assert!(issues(json!({"path": "a.step", "matrix": "turn it", "tolerance": 5})).is_empty(), "no `matrix` given means no `tolerance`");
    assert!(issues(json!({"path": "a.step", "matrix": [2, 0, 0, 0, 0, 2, 0, 0, 0, 0, 2, 0]})).is_empty());
}

/// The published schema: `path` is required and shaped as the handler reads it; `tolerance` has
/// its shape only where it is read.
#[test]
fn the_schema_takes_what_the_handler_reads() {
    let schema = input_schema_for::<InsertStepParams>();
    let schema = schema.as_value();
    let matrix = json!([0, -1, 0, 100, 1, 0, 0, 0, 0, 0, 1, 0]);
    for (p, takes) in [
        (json!({"path": "a.step"}), true),
        (json!({"path": "a.STP", "name": "N", "up": "y", "names": "auto", "folder": true}), true),
        (json!({"path": "a.iges", "up": null, "names": null, "folder": null, "matrix": null, "tolerance": null}), true),
        (json!({"path": "a.fcstd", "matrix": matrix, "tolerance": 1e-9}), true),
        (json!({"path": "a.3dm", "matrix": [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]], "tolerance": 0.001}), true),
        // Which kinds of file the command takes is the handler's to refuse, so the schema asks
        // nothing of the extension; `tolerance` without `matrix` is the case it does describe.
        (json!({"path": "a.stl", "up": "y", "names": "auto", "folder": true}), true),
        (json!({"path": "a.step", "tolerance": "x"}), true),
        (json!({"path": "a.step", "matrix": null, "tolerance": []}), true),
        (json!({}), false),
        (json!({"path": 5}), false),
        (json!({"path": "a.step", "name": 5}), false),
        (json!({"path": "a.stl", "up": 5}), false),
        (json!({"path": "a.step", "up": 5}), false),
        (json!({"path": "a.step", "up": "q"}), false),
        (json!({"path": "a.step", "names": 5}), false),
        (json!({"path": "a.step", "up": "y", "names": "other"}), false),
        (json!({"path": "a.step", "folder": "yes"}), false),
        (json!({"path": "a.step", "folder": 1}), false),
        // A matrix is shaped as `occurrence.move` publishes it; `matrix::rigid` still decides
        // whether it is a rigid move.
        (json!({"path": "a.step", "matrix": "turn it"}), false),
        (json!({"path": "a.step", "matrix": [1, 2, 3]}), false),
        (json!({"path": "a.step", "matrix": matrix, "tolerance": "x"}), false),
        (json!({"path": "a.step", "matrix": matrix, "tolerance": 0}), false),
        (json!({"path": "a.step", "matrix": matrix, "tolerance": 0.01}), false),
    ] {
        assert_eq!(conforms(schema, schema, &p), takes, "{p}");
    }
}

/// `up`, `names`, `matrix` and `tolerance` are the shapes `doc.open` and `occurrence.move`
/// publish, not copies of them; `path` is shaped as the handler reads it.
#[test]
fn the_shared_shapes_are_the_ones_published() {
    let mine = input_schema_for::<InsertStepParams>();
    let mine = mine.as_value();
    let open = input_schema_for::<crate::schema::file::OpenParams>();
    let open = open.as_value();
    let move_ = input_schema_for::<crate::schema::component::MoveParams>();
    let move_ = move_.as_value();
    for name in ["UpParam", "NamesParam"] {
        let shape = &mine["$defs"][name];
        assert!(shape.is_object(), "{name} is in my schema");
        assert_eq!(shape, &open["$defs"][name], "{name} is doc.open's shape");
    }
    for name in ["MoveMatrix", "MatrixTolerance"] {
        let shape = &mine["$defs"][name];
        assert!(shape.is_object(), "{name} is in my schema");
        assert_eq!(shape, &move_["$defs"][name], "{name} is occurrence.move's shape");
    }
    assert_eq!(mine["properties"]["path"]["pattern"], json!(crate::schema::types::not_blank_pattern()));
    assert_eq!(mine["properties"]["path"]["maxLength"], json!(4095));
}

/// A fresh directory for one test's STEP files.
fn folder(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("solidcraft-insert-step-schema-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    dir
}

fn write_step(dir: &Path, name: &str, products: &[ExportProduct<'_>]) -> String {
    let text = step_export_products(products, 0, &StepHeader::default()).unwrap_or_else(|e| panic!("{e}"));
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap_or_else(|e| panic!("{e}"));
    path.to_string_lossy().to_string()
}

/// A STEP file holding one solid: a box 10 mm on a side.
fn block_file(dir: &Path) -> String {
    let block = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
    let part = |name: &str, label: &str, body| ExportProduct {
        name: name.into(),
        bodies: vec![ExportBody { name: label.into(), body, color: None }],
        children: Vec::new(),
    };
    write_step(dir, "block.step", &[part("Block", "Block", &block)])
}

/// A STEP file holding an assembly: a plate 40 wide, 4 high and 20 deep with two posts standing
/// on it, as a Y-up file has it, so the import makes components and rigid joints of its own.
fn frame_file(dir: &Path) -> String {
    let plate = box_solid(Vec3::ZERO, Vec3::new(40.0, 4.0, 20.0)).unwrap();
    let post = box_solid(Vec3::ZERO, Vec3::new(4.0, 30.0, 4.0)).unwrap();
    let at = |x: f64, y: f64, z: f64| [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [x, y, z, 1.0]];
    let part = |name: &str, label: &str, body| ExportProduct {
        name: name.into(),
        bodies: vec![ExportBody { name: label.into(), body, color: None }],
        children: Vec::new(),
    };
    let products = [
        ExportProduct {
            name: "Frame".into(),
            bodies: Vec::new(),
            children: vec![
                (1, at(0.0, 0.0, 0.0), "Plate-1".into()),
                (2, at(10.0, 4.0, 8.0), "Post-1".into()),
                (2, at(26.0, 4.0, 8.0), "Post-2".into()),
            ],
        },
        part("Plate", "Plate", &plate),
        part("Post", "Post", &post),
    ];
    write_step(dir, "frame.step", &products)
}

fn matches(reply: &Value) {
    let schema = output_schema_for::<StepInserted>();
    let schema = schema.as_value();
    assert!(conforms(schema, schema, reply), "{reply} doesn't match {schema}");
}

/// Every form the command takes, run in Warn: each reply matches its output schema and carries no
/// warning (Warn runs a call the contract refuses anyway, so this shows the contract takes each
/// call). The keys the handler leaves unread are given bad values on purpose.
#[test]
fn replies_match_the_output_schema() {
    let dir = folder("replies");
    let block = block_file(&dir);
    let frame = frame_file(&dir);
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let mut run = |p: Value| {
        let r = s.execute("file.insert_step", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(r.get("param_warnings").is_none(), "{p}: {r}");
        matches(&r);
        r
    };
    let plain = run(json!({"path": block}));
    assert_eq!(plain["component_name"], "block", "{plain}");
    assert_eq!(plain["name"], "Block", "{plain}");
    assert!(plain["bodies"] == plain["created_bodies"] && plain["occurrence"].is_u64(), "{plain}");
    assert!(plain.get("subcomponents").is_none() && plain.get("rigid_joints").is_none(), "{plain}");
    assert!(plain.get("stand_up").is_none() && plain.get("group").is_none() && plain.get("transform").is_none(), "{plain}");

    let named = run(json!({"path": block, "name": "Brake", "up": " Z ", "names": " Solid ", "folder": false}));
    assert_eq!(named["component_name"], "Brake", "{named}");
    assert_eq!(named["name"], "Brake", "{named}");
    run(json!({"path": block, "name": null, "up": null, "names": null, "folder": null, "matrix": null, "tolerance": null}));
    // Keys left unread, with values that would be refused if they were read.
    run(json!({"path": block, "tolerance": "x"}));
    run(json!({"path": block, "matrix": null, "tolerance": []}));

    let stood = run(json!({"path": frame}));
    assert!(stood["subcomponents"].is_u64() && stood["rigid_joints"].is_u64(), "{stood}");
    assert!(stood.get("stand_up").is_none(), "{stood}");
    let turned = run(json!({"path": frame, "up": " y "}));
    assert_eq!(turned["stand_up"], turned["occurrence"], "{turned}");
    let grouped = run(json!({"path": frame, "up": "z", "names": " Instance ", "folder": true}));
    assert!(grouped["group"].is_u64() && grouped.get("stand_up").is_none(), "{grouped}");
    let quarter = json!([0, -1, 0, 100, 1, 0, 0, 0, 0, 0, 1, 0]);
    let placed = run(json!({"path": frame, "up": "y", "matrix": quarter, "tolerance": 1e-9}));
    assert_eq!(placed["transform"][3][0], json!(100.0), "{placed}");

    // A misspelt key still runs in Warn, says so, and the reply still matches.
    let warned = s.execute("file.insert_step", &json!({"path": block, "toleranc": 1})).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(warned["param_warnings"], json!(["unknown key `toleranc`; did you mean `tolerance`?"]), "{warned}");
    matches(&warned);
}

/// In Strict a call that works today is not refused, and one that fails fails with the handler's
/// message (see `strict_keeps_todays_errors`).
#[test]
fn strict_runs_the_calls_that_work() {
    let dir = folder("strict");
    let block = block_file(&dir);
    let frame = frame_file(&dir);
    let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
    for p in [
        json!({"path": block}),
        json!({"path": block, "name": "Brake"}),
        json!({"path": block, "up": "z", "names": "auto", "folder": true}),
        json!({"path": block, "tolerance": "x"}),
        json!({"path": block, "matrix": null, "tolerance": []}),
        json!({"path": frame, "up": "y", "names": "instance", "folder": true}),
        json!({"path": frame, "matrix": [0, -1, 0, 100, 1, 0, 0, 0, 0, 0, 1, 0], "tolerance": 1e-9}),
    ] {
        s.execute("file.insert_step", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
    }
    let e = s.execute("file.insert_step", &json!({"path": block, "uints": 1})).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(e.contains("unknown key `uints`"), "{e}");
}
