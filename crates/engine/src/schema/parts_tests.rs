use serde_json::{Value, json};

use super::{InsertedPart, PartsInsertParams, PartsLibrary, PartsLibraryParams, PartsResizeParams, ResizedPart};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A 40 x 30 x 10 plate with two 6.4 mm holes through its top face, as in the command's tests:
/// a point on each hole's rim.
fn plate(s: &mut Session) -> [[f64; 3]; 2] {
    run(s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "body_name": "Plate"}));
    run(s, "solid.hole", json!({"position": [10, 15, 10], "diameter": 6.4}));
    run(s, "solid.hole", json!({"position": [30, 15, 10], "diameter": 6.4}));
    [[13.2, 15.0, 10.0], [33.2, 15.0, 10.0]]
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<PartsLibraryParams>(), &["family"]);
    assert_keys(input_schema_for::<PartsInsertParams>(), &["at", "direction", "family", "length", "name", "point", "size"]);
    assert_keys(input_schema_for::<PartsResizeParams>(), &["component", "length", "size"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("parts.library", json!({"family": "nope"})),
        ("parts.resize", json!({"component": true})),
        ("parts.resize", json!({"component": "Nope"})),
    ]);
}

/// The insert names itself `parts.insert (Insert Fastener)` in its messages, under either id,
/// where strict names the id called; the message after the name is the same.
#[test]
fn the_insert_keeps_its_messages_in_strict() {
    let many = vec![[0, 0, 0]; 1001];
    let message = |s: &mut Session, id: &str, p: &Value| s.execute(id, p).err().map(|e| e.to_string().split_once("`: ").map(|m| m.1.to_string()));
    for p in [
        json!({"family": 5, "size": "M6"}),
        json!({"family": "washer"}),
        json!({"family": "washer", "size": 6}),
        json!({"family": "nope", "size": "M6"}),
        json!({"family": "socket_head_cap_screw", "size": "M6", "length": 0}),
        json!({"family": "socket_head_cap_screw", "size": "M6", "length": "x"}),
        json!({"family": "socket_head_cap_screw", "size": "M6", "length": 3000}),
        json!({"family": "washer", "size": "M6", "at": "x"}),
        json!({"family": "washer", "size": "M6", "at": [[1, 2]]}),
        json!({"family": "washer", "size": "M6", "at": many}),
    ] {
        for id in ["parts.insert", "parts.fastener"] {
            let off = message(&mut Session::default(), id, &p);
            assert!(off.as_ref().is_some_and(Option::is_some), "{id} {p} should fail today");
            let strict = message(&mut Session { param_check: ParamCheck::Strict, ..Session::default() }, id, &p);
            assert_eq!(strict, off, "{id} {p}");
        }
    }
    // A resize's length is read once the part is found.
    let error = |level: ParamCheck| {
        let mut s = Session { param_check: level, ..Session::default() };
        let c = run(&mut s, "parts.insert", json!({"family": "washer", "size": "M6"}))["component"].clone();
        s.execute("parts.resize", &json!({"component": c, "length": 0})).err().map(|e| e.to_string())
    };
    let off = error(ParamCheck::Off);
    assert!(off.is_some());
    assert_eq!(error(ParamCheck::Strict), off);
}

/// #11003: the library, an insert and a resize reply as their output schemas say, in Warn
/// without warnings for good calls and with them for a misspelt key: every family and one, a
/// part at a point turned to a direction, one seated in two holes under the dialog's id, and a
/// resize.
#[test]
fn part_commands_reply_with_their_schemas() {
    let (library, inserted, resized) = (output_schema_for::<PartsLibrary>(), output_schema_for::<InsertedPart>(), output_schema_for::<ResizedPart>());
    let matches = |schema: &schemars::Schema, r: &Value, warned: bool| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
        assert_eq!(r.get("param_warnings").is_some(), warned, "{r}");
    };
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let r = run(&mut s, "parts.library", json!({}));
    matches(&library, &r, false);
    assert!(r["families"].as_array().is_some_and(|f| f.len() > 5) && r.get("family").is_none(), "{r}");
    let r = run(&mut s, "parts.library", json!({"family": "socket_head_cap_screw"}));
    matches(&library, &r, false);
    assert!(r["family"] == "socket_head_cap_screw" && r["lengths"].is_array() && r.get("families").is_none(), "{r}");
    matches(&library, &run(&mut s, "parts.library", json!({"family": "bearing"})), false);

    let holes = plate(&mut s);
    let r = run(
        &mut s,
        "parts.insert",
        json!({"family": "hex_nut", "size": "M6", "point": [60, 0, 0], "direction": {"x": 1, "y": 0, "z": 0}, "name": "Nut"}),
    );
    matches(&inserted, &r, false);
    assert!(r.get("joint").is_none() && r["name"] == "Nut", "{r}");
    let r = run(&mut s, "parts.fastener", json!({"family": "socket_head_cap_screw", "size": "M6", "length": 20, "at": holes}));
    matches(&inserted, &r, false);
    assert_eq!((r["occurrences"].as_array().map(Vec::len), r["joints"].as_array().map(Vec::len)), (Some(2), Some(2)), "{r}");
    let screw = r["component"].clone();
    let r = run(&mut s, "parts.resize", json!({"component": screw, "size": "M8", "length": 25}));
    matches(&resized, &r, false);
    assert_eq!((r["size"].as_str(), r["length"].as_f64()), (Some("M8"), Some(25.0)), "{r}");

    matches(&library, &run(&mut s, "parts.library", json!({"familly": "washer"})), true);
    matches(&inserted, &run(&mut s, "parts.insert", json!({"family": "washer", "size": "M6", "lenght": 3})), true);
    matches(&resized, &run(&mut s, "parts.resize", json!({"component": screw, "sise": "M6"})), true);
}

/// A list of hole picks starts with a point (a list of three numbers), not an object and not a
/// shorter list; a single point or a list of points is fine.
#[test]
fn hole_picks_schema_requires_a_first_point() {
    let root = serde_json::to_value(input_schema_for::<PartsInsertParams>()).unwrap();
    let at = &root["properties"]["at"];
    let ok = |v: Value| conforms(&root, at, &v);
    assert!(ok(json!([[1, 2, 3]])));
    assert!(ok(json!([1, 2, 3])));
    assert!(ok(json!([[1, 2, 3], [4, 5, 6]])));
    assert!(!ok(json!([[1, 2]])));
    assert!(!ok(json!([[1, 2, 3], [1, 2]])));
    assert!(!ok(json!([{"x": 1}])));
}
