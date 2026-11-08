use serde_json::{Value, json};

use super::{EditTraceParams, KeepTraceParams, MotionTraceEdited, MotionTraceKept, MotionTraced, TraceParams};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

/// A door, `Door`, on a revolute joint `Hinge` at the origin, and a motion study `Swing` that
/// turns the hinge from 0 to 90° over 10 steps.
fn door(level: ParamCheck) -> Session {
    let mut s = Session::default();
    for (id, p) in [
        ("component.activate", json!({"component": "root"})),
        ("component.create", json!({"name": "Door", "grounded": false})),
        ("solid.box", json!({"length": 100, "width": 4, "height": 50, "corner": [0, 0, 0]})),
    ] {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    }
    let hinge = json!({"type": "revolute", "a": {"occurrence": 0, "point": [0, 0, 0]}, "b": {"occurrence": "Door", "point": [0, 0, 0]}, "flip": true, "name": "Hinge"});
    s.execute("joint.create", &hinge).unwrap_or_else(|e| panic!("{e}"));
    let swing = json!({"name": "Swing", "steps": 10, "keys": [{"joint": "Hinge", "points": [[0, 0], [10, 90]]}]});
    s.execute("motion.study", &swing).unwrap_or_else(|e| panic!("{e}"));
    s.param_check = level;
    s
}

/// The door with the path of its far corner kept as an associative trace, `Corner`.
fn door_with_trace(level: ParamCheck) -> Session {
    let mut s = door(ParamCheck::Off);
    let keep = json!({"occurrence": "Door", "point": [100, 0, 0], "joint": "Hinge", "from": 0, "to": 90, "samples": 5, "name": "Corner"});
    s.execute("motion.trace.keep", &keep).unwrap_or_else(|e| panic!("{e}"));
    s.param_check = level;
    s
}

/// The far corner of the door as the hinge turns a quarter.
fn corner() -> Value {
    json!({"occurrence": "Door", "point": [100, 0, 0], "joint": "Hinge", "from": 0, "to": 90, "samples": 5})
}

/// `corner()` with `key` set to `value`.
fn corner_with(key: &str, value: Value) -> Value {
    let mut p = corner();
    p[key] = value;
    p
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<TraceParams>(), &["from", "index", "joint", "occurrence", "point", "samples", "study", "to"]);
    assert_keys(
        input_schema_for::<KeepTraceParams>(),
        &["auto_update", "from", "index", "joint", "name", "occurrence", "point", "point_ref", "samples", "study", "to"],
    );
    assert_keys(input_schema_for::<EditTraceParams>(), &["auto_update", "from", "index", "joint", "samples", "sketch", "study", "to"]);
}

/// Calls that fail today get the same message in strict, on the door (and its kept trace).
#[test]
fn strict_keeps_todays_errors() {
    let mut cases = Vec::new();
    for id in ["motion.trace", "motion.trace.keep"] {
        for (key, value) in [
            ("occurrence", json!(true)),
            ("occurrence", json!(null)),
            ("occurrence", json!([1.5])),
            ("occurrence", json!(vec![0; 65])),
            ("occurrence", json!("Nope")),
            ("point", json!("x")),
            ("point", json!(null)),
            ("point", json!([1, 2])),
            ("point", json!([1e8, 0, 0])),
            ("joint", json!(true)),
            ("joint", json!("Nope")),
            ("from", json!(true)),
            ("from", json!(null)),
            ("to", json!([90])),
            ("to", json!(1e10)),
            ("samples", json!(1)),
            ("samples", json!(501)),
            ("samples", json!("10")),
            ("samples", json!(null)),
        ] {
            cases.push((id, corner_with(key, value)));
        }
        let mut no_to = corner();
        if let Some(o) = no_to.as_object_mut() {
            o.remove("to");
        }
        cases.push((id, no_to));
        cases.push((id, json!({"occurrence": "Door", "point": [100, 0, 0], "study": true})));
        cases.push((id, json!({"occurrence": "Door", "point": [100, 0, 0], "study": "Nope"})));
        cases.push((id, json!({"occurrence": "Door", "point": [100, 0, 0], "study": "Swing", "joint": "Hinge"})));
    }
    for p in
        [json!({"sketch": true}), json!({"sketch": "Nope"}), json!({"sketch": "Corner", "samples": 1}), json!({"sketch": "Corner", "study": "Nope"})]
    {
        cases.push(("motion.trace.edit", p));
    }
    for (id, p) in cases {
        let off = door_with_trace(ParamCheck::Off).execute(id, &p).err().map(|e| e.to_string());
        assert!(off.is_some(), "{id} {p} should fail today");
        assert_eq!(door_with_trace(ParamCheck::Strict).execute(id, &p).err().map(|e| e.to_string()), off, "{id} {p}");
    }
}

/// Keys of the wrong type that the handlers ignore today (a `name` that isn't text is read as no
/// name; an `auto_update` that isn't true or false as true) are refused by the contracts: in Warn
/// the call still works, with a warning naming the key.
#[test]
fn the_contracts_refuse_what_the_handlers_ignore() {
    for (id, p, key) in [
        ("motion.trace.keep", corner_with("name", json!(5)), "name"),
        ("motion.trace.keep", corner_with("auto_update", json!("no")), "auto_update"),
        ("motion.trace.edit", json!({"sketch": "Corner", "auto_update": "no"}), "auto_update"),
    ] {
        assert!(door_with_trace(ParamCheck::Off).execute(id, &p).is_ok(), "{id} {p} works today");
        let warned = door_with_trace(ParamCheck::Warn).execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert!(warned["param_warnings"].to_string().contains(key), "{warned}");
        let error = door_with_trace(ParamCheck::Strict).execute(id, &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(error.contains(&format!("`{key}`")), "{id} {p}: {error}");
    }
}

/// The schemas take what the handlers take. Each call is checked against both the schema and the
/// handler, with the check off, so the two can't drift apart.
#[test]
fn the_schemas_take_what_the_handlers_take() {
    let trace = input_schema_for::<TraceParams>();
    let keep = input_schema_for::<KeepTraceParams>();
    let edit = input_schema_for::<EditTraceParams>();
    let s = door_with_trace(ParamCheck::Off);
    let door_id = s.doc.occurrences.iter().find(|o| o.name.starts_with("Door")).map(|o| o.id).unwrap_or_default();
    let study_id = s.doc.assembly.studies.first().map(|m| m.id).unwrap_or_default();
    let sketch_id = s.doc.assembly.traces.first().map(|t| t.sketch).unwrap_or_default();
    let joint_id = s.doc.assembly.joints.first().map(|j| j.id).unwrap_or_default();
    let mut calls = Vec::new();
    for (id, schema) in [("motion.trace", &trace), ("motion.trace.keep", &keep)] {
        for p in [
            corner(),
            json!({"occurrence": "Door", "point": [100, 0, 0], "study": "Swing"}),
            json!({"occurrence": door_id, "point": {"x": 100, "y": 0, "z": 0}, "study": study_id, "samples": 3.4}),
            json!({"occurrence": [door_id], "point": [100, 0, 0], "joint": joint_id, "from": "0 deg", "to": "45 deg", "index": 0}),
            corner_with("samples", json!(500)),
            json!({"point": [100, 0, 0], "study": "Swing"}),
            json!({"occurrence": "Door", "study": "Swing"}),
        ] {
            calls.push((id, schema, p));
        }
    }
    for p in [
        corner_with("name", json!("Path")),
        corner_with("name", json!(" ")),
        corner_with("auto_update", json!(false)),
        corner_with("point_ref", json!({"occurrence": "Door", "point": [100, 0, 0]})),
    ] {
        calls.push(("motion.trace.keep", &keep, p));
    }
    for p in [
        json!({"sketch": "Corner", "samples": 3}),
        json!({"sketch": sketch_id, "from": 10, "to": "80 deg"}),
        json!({"sketch": "Corner", "study": "Swing"}),
        json!({"sketch": "Corner", "joint": "Hinge", "from": 0, "to": 45, "index": 0}),
        json!({"sketch": "Corner", "auto_update": false}),
        json!({"sketch": "Corner", "auto_update": true}),
        json!({"samples": 3}),
    ] {
        calls.push(("motion.trace.edit", &edit, p));
    }
    for (id, schema, p) in calls {
        let takes = door_with_trace(ParamCheck::Off).execute(id, &p).is_ok();
        assert_eq!(conforms(schema.as_value(), schema.as_value(), &p), takes, "{id} {p}");
    }
}

/// The three commands reply as their output schemas say, in Warn without warnings: the corner's
/// path, as points and as a kept spline; an edit of the kept trace; and its detach. A misspelt key
/// is ignored today: Warn says so, and strict refuses it.
#[test]
fn trace_keep_and_edit_reply_with_their_schemas() {
    let traced = output_schema_for::<MotionTraced>();
    let kept = output_schema_for::<MotionTraceKept>();
    let edited = output_schema_for::<MotionTraceEdited>();
    let mut s = door(ParamCheck::Warn);
    let mut run = |id: &str, p: Value, schema: &Value| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{r}");
        r
    };
    let r = run("motion.trace", corner(), traced.as_value());
    assert_eq!((&r["occurrence"], &r["samples"], &r["stopped_at"]), (&json!("Door:1"), &json!(5), &json!(null)), "{r}");
    let last = r["points"].as_array().and_then(|p| p.last()).cloned().unwrap_or_default();
    let end = [last[0].as_f64(), last[1].as_f64()].map(|x| x.map(f64::round));
    assert!(end == [Some(0.0), Some(100.0)] || end == [Some(0.0), Some(-100.0)], "a quarter turn about Z: {r}");
    run("motion.trace", json!({"occurrence": "Door", "point": [100, 0, 0], "study": "Swing"}), traced.as_value());
    let r = run("motion.trace.keep", corner_with("name", json!("Corner")), kept.as_value());
    assert_eq!((&r["name"], &r["points"]), (&json!("Corner"), &json!(5)), "{r}");
    assert_eq!(r["curves"].as_array().map(Vec::len), Some(1), "{r}");
    let sketch = r["sketch"].clone();
    run("motion.trace.keep", corner_with("auto_update", json!(false)), kept.as_value());
    let r = run("motion.trace.edit", json!({"sketch": "Corner", "samples": 3}), edited.as_value());
    assert_eq!(r, json!({"sketch": sketch, "auto_update": true}));
    let r = run("motion.trace.edit", json!({"sketch": "Corner", "auto_update": false}), edited.as_value());
    assert_eq!(r, json!({"sketch": sketch, "auto_update": false}));
    // A misspelt key.
    let p = corner_with("sample", json!(3));
    let r = s.execute("motion.trace", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
    assert!(r["param_warnings"].to_string().contains("sample"), "{r}");
    s.param_check = ParamCheck::Strict;
    let error = s.execute("motion.trace", &p).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains("`sample`"), "{error}");
}
