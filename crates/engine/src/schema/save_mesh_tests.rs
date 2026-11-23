use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::{SaveMeshParams, SaveMeshResult};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("solidcraft-13308-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn with_box(level: ParamCheck) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    s.execute("solid.box", &json!({"length": 20, "width": 10, "height": 5, "body_name": "Block"})).unwrap();
    s.execute("solid.cylinder", &json!({"radius": 3, "height": 8, "body_name": "Pin", "operation": "new"})).unwrap();
    s
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<SaveMeshParams>(),
        &["ascii", "bodies", "format", "lay_on_print_faces", "objects", "path", "quality", "single_instance", "slicer"],
    );
    // `path` is the key the handler refuses when it is left out: required in the schema, optional in the type.
    let schema = input_schema_for::<SaveMeshParams>();
    assert_eq!(schema.as_value()["required"], json!(["path"]));
    assert!(serde_json::from_value::<SaveMeshParams>(json!({})).is_ok());
    // The patterns are pinned (`conforms` doesn't read them).
    let defs = &schema.as_value()["$defs"];
    assert_eq!(defs["MeshFormat"]["pattern"], "^([Ss][Tt][Ll]|3[Mm][Ff]|3[Mm][Ff]-[Tt][Aa][Gg][Gg][Ee][Dd])$");
    assert_eq!(defs["MeshPath"]["pattern"], json!(crate::schema::types::not_blank_pattern()));
    assert_eq!(defs["MeshPath"]["maxLength"], 4095);
    let quality = defs["MeshQualityParam"]["pattern"].as_str().unwrap();
    assert!(quality.starts_with("^[\\u0009-\\u000d\\u0020") && quality.contains("[Ff][Ii][Nn][Ee][Ss][Tt]"), "{quality}");
    assert_eq!(schema.as_value()["allOf"].as_array().map(Vec::len), Some(2));
}

/// Strict gives the handler's message for the calls it refuses for a key. Nothing is written: each
/// of these is refused before the file is.
#[test]
fn strict_keeps_todays_errors() {
    let p = |extra: Value| {
        let mut p = json!({"path": "/nonexistent-13308/out.stl"});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        p
    };
    let mut cases: Vec<(&str, Value)> = vec![
        ("file.save_mesh", json!({})),
        ("file.save_mesh", json!({"path": ""})),
        ("file.save_mesh", json!({"path": "   "})),
        ("file.save_mesh", json!({"path": 5})),
        ("file.save_mesh", json!({"path": null})),
        ("file.save_mesh", json!({"path": ["a.stl"]})),
        ("file.save_mesh", json!({"path": "x".repeat(4096)})),
    ];
    for extra in [
        json!({"format": "obj"}),
        json!({"format": "step"}),
        json!({"format": ""}),
        json!({"format": "3MF-Tagged "}),
        json!({"quality": "ultra"}),
        json!({"quality": 5}),
        json!({"quality": true}),
        json!({"quality": ""}),
        json!({"objects": "Body"}),
        json!({"objects": "parts"}),
        json!({"objects": 5}),
        json!({"objects": false}),
        json!({"bodies": 5}),
        json!({"bodies": [1, "a"]}),
        json!({"bodies": {"a": 1}}),
        json!({"bodies": true}),
        json!({"format": "stl", "bodies": ["a", null]}),
        json!({"lay_on_print_faces": "yes"}),
        json!({"lay_on_print_faces": 1}),
        json!({"format": "3mf", "lay_on_print_faces": {}}),
    ] {
        cases.push(("file.save_mesh", p(extra)));
    }
    assert_strict_keeps_todays_errors(&cases);
}

/// The refusals only the handler can make (they need the design or two keys together) are the same
/// in strict, and nothing is written.
#[test]
fn strict_keeps_the_handlers_own_refusals() {
    let dir = temp_dir("own");
    let at = |n: &str| dir.join(n).to_string_lossy().into_owned();
    for p in [
        json!({"path": at("a.stl"), "objects": "component"}),
        json!({"path": at("b.stl"), "bodies": ["Nope"]}),
        json!({"path": at("c.3mf"), "slicer": "no such slicer anywhere"}),
        json!({"path": at("d.stl"), "slicer": ""}),
    ] {
        let error = |level: ParamCheck| with_box(level).execute("file.save_mesh", &p).err().map(|e| e.to_string());
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{p}");
    }
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The types read each key as the handler does: keys it never reads in a call are not read by the
/// contract either, and the ones it reads fail with its words.
#[test]
fn the_types_read_like_the_handler() {
    let reads = |p: Value| serde_json::from_value::<SaveMeshParams>(p).is_ok();
    let issues = |p: Value| crate::schema::issues("file.save_mesh", &p).into_iter().map(|i| i.message).collect::<Vec<_>>();
    // `ascii` is read only for `format: stl`, in any case; `single_instance` only with a text `slicer`.
    assert!(reads(json!({"path": "a.stl", "ascii": "yes"})) && reads(json!({"path": "a.stl", "ascii": 1, "format": "3mf"})));
    assert!(reads(json!({"path": "a.stl", "ascii": {}, "format": null})) && reads(json!({"path": "a.stl", "ascii": [], "format": null})));
    assert!(reads(json!({"path": "a.stl", "single_instance": "no"})) && reads(json!({"path": "a.stl", "single_instance": 0, "slicer": null})));
    assert!(reads(json!({"path": "a.stl", "format": "STL", "ascii": true})) && reads(json!({"path": "a.stl", "format": "Stl", "ascii": null})));
    assert!(reads(json!({"path": "a.stl", "slicer": "prusa", "single_instance": false})));
    assert!(!reads(json!({"path": "a.stl", "format": "stl", "ascii": "yes"})) && !reads(json!({"path": "a.stl", "format": "STL", "ascii": 1})));
    assert!(!reads(json!({"path": "a.stl", "slicer": "prusa", "single_instance": "no"})));
    // `null` reads as left out where the handler does.
    assert!(reads(json!({"path": "a.stl", "format": null, "bodies": null, "slicer": null, "quality": null, "objects": null})));
    // Stricter than the handler, which reads a wrong type as left out.
    assert!(!reads(json!({"path": "a.stl", "format": 5})) && !reads(json!({"path": "a.stl", "slicer": 5})));
    assert!(!reads(json!({"path": "a.stl", "slicer": true})));
    // The words the handler takes: any case for the format and the quality (trimmed), lower case for `objects`.
    for format in ["stl", "3MF", "3mf-tagged", "3Mf-TaGGed"] {
        assert!(reads(json!({"path": "a", "format": format})), "{format}");
    }
    for quality in ["standard", "FINE", " Finest\n"] {
        assert!(reads(json!({"path": "a", "quality": quality})), "{quality:?}");
    }
    assert!(reads(json!({"path": "a", "objects": "body"})) && reads(json!({"path": "a", "objects": "component"})));
    assert!(reads(json!({"path": "a", "bodies": "A"})) && reads(json!({"path": "a", "bodies": ["A", "B"]})));
    assert!(reads(json!({"path": "a", "bodies": []})));
    // The handler's own messages.
    assert_eq!(issues(json!({"path": ""})), ["`path` is required"]);
    assert_eq!(issues(json!({"path": 5})), ["`path` is required"]);
    assert_eq!(issues(json!({"path": null})), ["`path` is required"]);
    assert_eq!(issues(json!({"path": "a", "format": "OBJ"})), ["unknown mesh format `obj` (stl, 3mf, 3mf-tagged)"]);
    assert_eq!(issues(json!({"path": "a", "quality": "ultra"})), ["`quality` must be standard, fine or finest, not \"ultra\""]);
    assert_eq!(issues(json!({"path": "a", "quality": 5})), ["`quality` must be standard, fine or finest, not 5"]);
    assert_eq!(issues(json!({"path": "a", "objects": "Body"})), ["`objects` must be body or component, not \"Body\""]);
    assert_eq!(issues(json!({"path": "a", "objects": null})), Vec::<String>::new());
    assert_eq!(issues(json!({"path": "a", "bodies": 5})), ["`bodies` must be a list of names; it is 5"]);
    assert_eq!(issues(json!({"path": "a", "pth": 1})), ["unknown key `pth`; did you mean `path`?"]);
    // A key the handler doesn't read in this call is not checked, so no warning for it.
    assert!(issues(json!({"path": "a", "format": "3mf", "ascii": "yes", "single_instance": 3})).is_empty());
    assert_eq!(issues(json!({"path": "a", "format": "stl", "ascii": "yes"})), ["`ascii` must be true or false"]);
}

/// What the schema takes: the same calls as the type, including the keys read only in some cases.
#[test]
fn the_schema_takes_what_the_handler_reads() {
    let schema = input_schema_for::<SaveMeshParams>();
    let schema = schema.as_value();
    for (p, takes) in [
        (json!({"path": "a.stl"}), true),
        (json!({"path": "a.3mf", "format": "3MF", "bodies": ["A"], "quality": "fine", "objects": "component"}), true),
        (json!({"path": "a", "format": "stl", "ascii": true}), true),
        (json!({"path": "a", "slicer": "prusa", "single_instance": false}), true),
        (json!({"path": "a", "bodies": "A"}), true),
        (json!({}), false),
        (json!({"path": 5}), false),
        (json!({"path": "a", "format": 5}), false),
        (json!({"path": "a", "objects": "x"}), false),
        (json!({"path": "a", "bodies": 5}), false),
        (json!({"path": "a", "wibble": 1}), false),
        // Read only in some cases: anything elsewhere, the shape where it is read.
        (json!({"path": "a", "ascii": "yes"}), true),
        (json!({"path": "a", "format": "3mf", "ascii": "yes"}), true),
        (json!({"path": "a", "format": "stl", "ascii": "yes"}), false),
        (json!({"path": "a", "single_instance": "no"}), true),
        (json!({"path": "a", "slicer": 5, "single_instance": "no"}), false),
        (json!({"path": "a", "slicer": "x", "single_instance": "no"}), false),
    ] {
        // `conforms` doesn't read `pattern`: the cases a pattern alone refuses are in the type tests.
        assert_eq!(conforms(schema, schema, &p), takes, "{p}");
    }
}

fn matches(r: &Value) {
    let schema = output_schema_for::<SaveMeshResult>();
    let schema = schema.as_value();
    assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
}

fn keys(r: &Value) -> BTreeSet<String> {
    r.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default()
}

/// Every form of the command replies as its output schema says, in warn and without warnings, with
/// the keys it always had; a misspelt key gets its warning and the reply still matches.
#[test]
fn replies_match_the_output_schema() {
    let dir = temp_dir("replies");
    let at = |n: &str| dir.join(n).to_string_lossy().into_owned();
    let mut s = with_box(ParamCheck::Warn);
    let run = |s: &mut Session, p: Value| {
        let r = s.execute("file.save_mesh", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(r.get("param_warnings").is_none(), "{p}: {r}");
        matches(&r);
        assert_eq!(keys(&r), ["bytes", "format", "path"].map(String::from).into(), "{p}: {r}");
        assert_eq!(r["bytes"].as_u64(), std::fs::metadata(r["path"].as_str().unwrap()).ok().map(|m| m.len()), "{p}: {r}");
        r
    };
    for (p, format) in [
        (json!({"path": at("a.stl")}), "StlBinary"),
        (json!({"path": at("b.3mf")}), "ThreeMf"),
        (json!({"path": at("c.dat")}), "StlBinary"),
        (json!({"path": at("d.dat"), "format": "STL"}), "StlBinary"),
        (json!({"path": at("e.stl"), "format": "stl", "ascii": true}), "StlAscii"),
        (json!({"path": at("f.stl"), "format": "Stl", "ascii": null}), "StlBinary"),
        (json!({"path": at("g.stl"), "format": "3mf"}), "ThreeMf"),
        (json!({"path": at("h.3mf"), "format": "3MF-tagged"}), "ThreeMfTagged"),
        (json!({"path": at("i.stl"), "bodies": "Block", "quality": "Fine"}), "StlBinary"),
        (json!({"path": at("j.3mf"), "bodies": ["Block", "Pin"], "quality": " finest ", "objects": "component"}), "ThreeMf"),
        (json!({"path": at("k.3mf"), "objects": "body", "bodies": []}), "ThreeMf"),
        (json!({"path": at("l.stl"), "format": null, "bodies": null, "quality": null, "objects": null, "slicer": null}), "StlBinary"),
        // Keys the handler doesn't read in these calls take anything, and get no warning.
        (json!({"path": at("m.3mf"), "ascii": "yes"}), "ThreeMf"),
        (json!({"path": at("n.stl"), "single_instance": [1], "ascii": {}}), "StlBinary"),
        (json!({"path": at("o.stl"), "format": "3mf", "ascii": 7, "single_instance": "no"}), "ThreeMf"),
    ] {
        let r = run(&mut s, p.clone());
        assert_eq!(r["format"], format, "{p}");
    }
    assert!(std::fs::read_to_string(at("e.stl")).unwrap().starts_with("solid"));
    assert!(std::fs::read(at("a.stl")).unwrap().len() > 84);
    // Laid on their print faces: the bodies laid are named, and the reply still matches.
    s.execute("body.print_face", &json!({"body": "Block", "point": [10, 5, 5]})).unwrap();
    for p in [json!({"path": at("r.stl"), "lay_on_print_faces": true}), json!({"path": at("s.3mf"), "lay_on_print_faces": true})] {
        let r = s.execute("file.save_mesh", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(r.get("param_warnings").is_none(), "{p}: {r}");
        matches(&r);
        assert_eq!(keys(&r), ["bytes", "format", "path", "print_faces"].map(String::from).into(), "{p}: {r}");
        assert_eq!(r["print_faces"], json!(["Block"]), "{p}");
    }
    s.execute("body.print_face", &json!({"body": "Block"})).unwrap();

    // A misspelt key runs in warn and says so; the reply still matches. Strict refuses it.
    let p = json!({"path": at("p.stl"), "qualty": "fine"});
    let r = s.execute("file.save_mesh", &p).unwrap();
    assert!(r["param_warnings"].as_array().is_some_and(|w| w.iter().any(|m| m.as_str().is_some_and(|m| m.contains("`qualty`")))), "{r}");
    matches(&r);
    assert_eq!(r["format"], "StlBinary");
    s.param_check = ParamCheck::Strict;
    let error = s.execute("file.save_mesh", &json!({"path": at("q.stl"), "qualty": "fine"})).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains("`qualty`") && error.contains("did you mean `quality`"), "{error}");
    assert!(!std::path::Path::new(&at("q.stl")).exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// With a `slicer`: the reply says where the file went; a body with a print face is laid on it and
/// listed. The program is a script that records what it is given.
#[cfg(unix)]
#[test]
fn replies_with_a_slicer_match_the_output_schema() {
    use std::os::unix::fs::PermissionsExt;
    let _turn = crate::tests::mesh_convert::CHOSEN_PROGRAMS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = temp_dir("slicer");
    let (script, seen) = (dir.join("fake-slicer"), dir.join("seen"));
    std::fs::write(&script, format!("#!/bin/sh\necho \"$@\" >> {}\n", seen.display())).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let spec = script.to_string_lossy().into_owned();
    crate::slicers::set_chosen_programs(std::slice::from_ref(&spec));
    let mut s = with_box(ParamCheck::Warn);
    s.execute("body.print_face", &json!({"body": "Block", "point": [10, 5, 5]})).unwrap();
    let out = dir.join("part.stl").to_string_lossy().into_owned();
    let r = s.execute("file.save_mesh", &json!({"path": out, "slicer": spec, "single_instance": false}));
    let again =
        s.execute("file.save_mesh", &json!({"path": dir.join("x.3mf"), "slicer": spec, "format": "3mf", "objects": "component", "bodies": "Block"}));
    crate::slicers::set_chosen_programs(&[]);
    let (r, again) = (r.unwrap(), again.unwrap());
    for r in [&r, &again] {
        assert!(r.get("param_warnings").is_none(), "{r}");
        matches(r);
        assert!(r["slicer"].is_string() && r.get("slicer_error").is_none(), "{r}");
    }
    assert_eq!(keys(&r), ["bytes", "format", "path", "print_faces", "slicer"].map(String::from).into(), "{r}");
    assert_eq!(r["print_faces"], json!(["Block"]));
    assert_eq!(r["path"], json!(out));
    assert_eq!(keys(&again), ["bytes", "format", "path", "print_faces", "slicer"].map(String::from).into(), "{again}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The typed reply serializes to the keys the handler's JSON had, and nothing for what is unset.
#[test]
fn the_reply_type_serializes_as_before() {
    use super::{SaveMeshResult, SlicerDelivery};
    let reply = |delivery| SaveMeshResult {
        path: "a.stl".into(),
        bytes: 5,
        format: "StlBinary".into(),
        print_faces: Vec::new(),
        warnings: Vec::new(),
        delivery,
        param_warnings: Vec::new(),
    };
    assert_eq!(serde_json::to_value(reply(SlicerDelivery::default())).unwrap(), json!({"path": "a.stl", "bytes": 5, "format": "StlBinary"}));
    // Print faces and warnings are there when there are any, as `Laid::report` put them.
    let laid = SaveMeshResult { print_faces: vec!["A".into()], warnings: vec!["w".into()], ..reply(SlicerDelivery::default()) };
    let r = serde_json::to_value(laid).unwrap();
    assert_eq!(r, json!({"path": "a.stl", "bytes": 5, "format": "StlBinary", "print_faces": ["A"], "warnings": ["w"]}));
    matches(&r);
    for (delivery, key) in [
        (SlicerDelivery { slicer: Some("P".into()), ..Default::default() }, "slicer"),
        (SlicerDelivery { slicer_error: Some("e".into()), ..Default::default() }, "slicer_error"),
        (SlicerDelivery { starting: Some("P".into()), ..Default::default() }, "starting"),
        (SlicerDelivery { slicer_timeout: Some("P".into()), ..Default::default() }, "slicer_timeout"),
    ] {
        let r = serde_json::to_value(reply(delivery)).unwrap();
        assert_eq!(keys(&r), ["bytes", "format", "path", key].map(String::from).into(), "{r}");
        matches(&r);
    }
}
