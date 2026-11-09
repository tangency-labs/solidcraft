use serde_json::{Value, json};

use super::{ExportParams, PlayParams, StudyExported, StudyPlayed};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

/// A door, `Door`, on a revolute joint `Hinge` at the origin, a `Block` beside it, and a motion
/// study `Swing` that turns the hinge from 0 to 90° over 10 steps.
fn door(level: ParamCheck) -> Session {
    let mut s = Session::default();
    for (name, corner, size) in [("Door", [0, 0, 0], [100, 4, 50]), ("Block", [60, 30, 0], [20, 20, 50])] {
        for (id, p) in [
            ("component.activate", json!({"component": "root"})),
            ("component.create", json!({"name": name, "grounded": false})),
            ("solid.box", json!({"length": size[0], "width": size[1], "height": size[2], "corner": corner})),
        ] {
            s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
        }
    }
    let hinge = json!({"type": "revolute", "a": {"occurrence": 0, "point": [0, 0, 0]}, "b": {"occurrence": "Door", "point": [0, 0, 0]}, "flip": true, "name": "Hinge"});
    s.execute("joint.create", &hinge).unwrap_or_else(|e| panic!("{e}"));
    let swing = json!({"name": "Swing", "steps": 10, "keys": [{"joint": "Hinge", "points": [[0, 0], [10, 90]]}]});
    s.execute("motion.study", &swing).unwrap_or_else(|e| panic!("{e}"));
    s.param_check = level;
    s
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<PlayParams>(), &["quiet", "step", "study"]);
    assert_keys(input_schema_for::<ExportParams>(), &["path", "samples", "study"]);
}

/// #12668: calls that fail today get the same message in strict, on the door with its study.
#[test]
fn strict_keeps_todays_errors() {
    for (id, p) in [
        ("motion.play", json!({"step": 5})),
        ("motion.play", json!({"study": null, "step": 5})),
        ("motion.play", json!({"study": true, "step": 5})),
        ("motion.play", json!({"study": "Nope", "step": 5})),
        ("motion.play", json!({"study": "Swing"})),
        ("motion.play", json!({"study": "Swing", "step": null})),
        ("motion.play", json!({"study": "Swing", "step": "5"})),
        ("motion.export", json!({})),
        ("motion.export", json!({"study": [1]})),
        ("motion.export", json!({"study": "Nope"})),
        ("motion.export", json!({"study": "Swing", "samples": 1})),
        ("motion.export", json!({"study": "Swing", "samples": 10_001})),
        ("motion.export", json!({"study": "Swing", "samples": null})),
        ("motion.export", json!({"study": "Swing", "samples": "10"})),
    ] {
        let off = door(ParamCheck::Off).execute(id, &p).err().map(|e| e.to_string());
        assert!(off.is_some(), "{id} {p} should fail today");
        assert_eq!(door(ParamCheck::Strict).execute(id, &p).err().map(|e| e.to_string()), off, "{id} {p}");
    }
}

/// #12668: keys of the wrong type that the handlers ignore today (a `quiet` that isn't true or
/// false is read as false; a `path` that isn't text as no path) are refused by the contracts: in
/// Warn the call still works, with a warning naming the key.
#[test]
fn the_contracts_refuse_what_the_handlers_ignore() {
    for (id, p, key) in [
        ("motion.play", json!({"study": "Swing", "step": 5, "quiet": "yes"}), "quiet"),
        ("motion.export", json!({"study": "Swing", "samples": 2, "path": 5}), "path"),
    ] {
        assert!(door(ParamCheck::Off).execute(id, &p).is_ok(), "{id} {p} works today");
        let warned = door(ParamCheck::Warn).execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert!(warned["param_warnings"].to_string().contains(key), "{warned}");
        let error = door(ParamCheck::Strict).execute(id, &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(error.contains(&format!("`{key}`")), "{id} {p}: {error}");
    }
}

/// #12668: the schemas take what the handlers take. Each call is checked against both the schema
/// and the handler, with the check off, so the two can't drift apart.
#[test]
fn the_schemas_take_what_the_handlers_take() {
    let play = input_schema_for::<PlayParams>();
    let export = input_schema_for::<ExportParams>();
    let study_id = door(ParamCheck::Off).doc.assembly.studies.first().map(|m| m.id).unwrap_or_default();
    for (id, schema, p) in [
        ("motion.play", &play, json!({"study": "Swing", "step": 5})),
        ("motion.play", &play, json!({"study": study_id, "step": 2.5, "quiet": true})),
        ("motion.play", &play, json!({"study": "Swing", "step": -3})),
        ("motion.play", &play, json!({"study": "Swing", "step": 1e6})),
        ("motion.play", &play, json!({"study": "Swing", "step": "5"})),
        ("motion.play", &play, json!({"study": "Swing"})),
        ("motion.play", &play, json!({"step": 5})),
        ("motion.export", &export, json!({"study": "Swing"})),
        ("motion.export", &export, json!({"study": study_id, "samples": 2})),
        ("motion.export", &export, json!({"study": "Swing", "samples": 3.7})),
        ("motion.export", &export, json!({"study": "Swing", "samples": 1})),
        ("motion.export", &export, json!({"study": "Swing", "samples": 2, "path": ""})),
        ("motion.export", &export, json!({"samples": 2})),
    ] {
        let takes = door(ParamCheck::Off).execute(id, &p).is_ok();
        assert_eq!(conforms(schema.as_value(), schema.as_value(), &p), takes, "{id} {p}");
    }
}

/// #12668: Play and Export reply as their output schemas say, in Warn without warnings, and the
/// replies are the JSON the handlers gave before: the hinge turned to 45° with every occurrence
/// listed, then only the one that moved; the samples; and the same samples written to a JSON
/// file and to a CSV file. A misspelt key is ignored today: Warn says so, and strict refuses it.
#[test]
fn play_and_export_reply_with_their_schemas() {
    let played = output_schema_for::<StudyPlayed>();
    let played = played.as_value();
    let exported = output_schema_for::<StudyExported>();
    let exported = exported.as_value();
    let mut s = door(ParamCheck::Warn);
    let mut run = |id: &str, p: Value, schema: &Value| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{r}");
        r
    };
    let r = run("motion.play", json!({"study": "Swing", "step": 5}), played);
    assert_eq!(r["moved"], true, "{r}");
    assert_eq!(r["positions"].as_array().map(Vec::len), Some(2), "every occurrence: {r}");
    let hinge = &r["values"][0]["values"][0];
    assert_eq!((hinge["unit"].as_str(), hinge["value"].as_f64().map(f64::round)), (Some("deg"), Some(45.0)), "{r}");
    assert_eq!((&r["stopped_by_contact"], &r["stopped_by_loop"], &r["unmoved"]), (&json!(null), &json!(null), &json!([])), "{r}");
    let r = run("motion.play", json!({"study": "Swing", "step": 10, "quiet": true}), played);
    assert_eq!(r["positions"].as_array().map(|p| p.iter().map(|x| x["occurrence"].clone()).collect::<Vec<_>>()), Some(vec![json!("Door:1")]), "{r}");
    let samples = run("motion.export", json!({"study": "Swing", "samples": 3}), exported);
    assert_eq!((&samples["study"], &samples["steps"]), (&json!("Swing"), &json!(10)), "{samples}");
    assert_eq!(
        samples["samples"].as_array().map(|x| x.iter().map(|r| r["step"].clone()).collect::<Vec<_>>()),
        Some(vec![json!(0.0), json!(5.0), json!(10.0)])
    );
    let dir = solidcraft_io::vfs::temp_dir().join(format!("solidcraft-motion-export-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("swing.json").to_string_lossy().into_owned();
    let r = run("motion.export", json!({"study": "Swing", "samples": 3, "path": path}), exported);
    assert_eq!(r, json!({"path": path, "samples": 3}));
    let file: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_default()).unwrap_or_default();
    assert_eq!(file, samples, "the file holds the samples the reply gives");
    let path = dir.join("swing.csv").to_string_lossy().into_owned();
    run("motion.export", json!({"study": "Swing", "samples": 3, "path": path}), exported);
    let csv = std::fs::read_to_string(&path).unwrap_or_default();
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines.first().copied(), Some("step,occurrence,x,y,z,xx,xy,xz,yx,yy,yz,zx,zy,zz"));
    assert_eq!(lines.len(), 1 + 3 * 2, "a row per occurrence per sample: {csv}");
    assert!(lines.get(1).is_some_and(|l| l.starts_with("0.0,")), "the step as JSON writes it: {csv}");
    let _ = std::fs::remove_dir_all(&dir);
    // A misspelt key.
    let p = json!({"study": "Swing", "step": 5, "quite": true});
    let r = s.execute("motion.play", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
    assert!(r["param_warnings"].to_string().contains("quite"), "{r}");
    s.param_check = ParamCheck::Strict;
    let error = s.execute("motion.play", &p).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains("`quite`"), "{error}");
}
