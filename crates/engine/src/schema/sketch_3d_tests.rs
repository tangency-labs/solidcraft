use serde_json::{Value, json};

use super::{Curves3dResult, Line3dParams, Move3dParams, Move3dResult, Point3dParams, Point3dResult, Spline3dParams};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<Line3dParams>(), &["points", "sketch"]);
    assert_keys(input_schema_for::<Spline3dParams>(), &["points", "sketch"]);
    assert_keys(input_schema_for::<Point3dParams>(), &["point", "sketch"]);
    assert_keys(input_schema_for::<Move3dParams>(), &["by", "index", "point", "sketch", "to", "wire"]);
}

#[test]
fn strict_keeps_todays_errors() {
    let many: Vec<Value> = (0..501).map(|i| json!([i, 0, 0])).collect();
    assert_strict_keeps_todays_errors(&[
        ("sketch.line3d", json!({})),
        ("sketch.line3d", json!({"points": null})),
        ("sketch.line3d", json!({"points": "x"})),
        ("sketch.line3d", json!({"points": {"0": [0, 0, 0]}})),
        ("sketch.line3d", json!({"points": []})),
        ("sketch.line3d", json!({"points": [[0, 0, 0]]})),
        ("sketch.line3d", json!({"points": many})),
        ("sketch.line3d", json!({"points": [[0, 0, 0], [1, 2]]})),
        ("sketch.line3d", json!({"points": [[0, 0, 0], "a"]})),
        ("sketch.line3d", json!({"points": [[0, 0, 0], [0, 0, 1]], "sketch": true})),
        ("sketch.line3d", json!({"points": [[0, 0, 0], [0, 0, 1]], "sketch": -1})),
        ("sketch.line3d", json!({"points": [[0, 0, 0], [0, 0, 1]], "sketch": 1.5})),
        ("sketch.line3d", json!({"points": [[0, 0, 0], [0, 0, 1]], "sketch": [1]})),
        ("sketch.spline3d", json!({})),
        ("sketch.spline3d", json!({"points": [[0, 0, 0]]})),
        ("sketch.spline3d", json!({"points": 5})),
        ("sketch.spline3d", json!({"points": [[0, 0, 0], [1, 1]]})),
        ("sketch.point3d", json!({})),
        ("sketch.point3d", json!({"point": null})),
        ("sketch.point3d", json!({"point": [1, 2]})),
        ("sketch.point3d", json!({"point": "1,2,3"})),
        ("sketch.point3d", json!({"point": [1, 2, 3], "sketch": {}})),
        ("sketch.move3d", json!({})),
        ("sketch.move3d", json!({"wire": 3, "to": [0, 0, 0]})),
        ("sketch.move3d", json!({"wire": null, "to": [0, 0, 0]})),
        ("sketch.move3d", json!({"wire": "w1", "index": -1, "to": [0, 0, 0]})),
        ("sketch.move3d", json!({"wire": "w1", "index": 500, "to": [0, 0, 0]})),
        ("sketch.move3d", json!({"wire": "w1", "index": 1.5, "to": [0, 0, 0]})),
        ("sketch.move3d", json!({"wire": "w1", "index": "1", "to": [0, 0, 0]})),
        ("sketch.move3d", json!({"wire": "w1", "index": null, "to": [0, 0, 0]})),
        ("sketch.move3d", json!({"wire": "w1", "to": [0, 0]})),
        ("sketch.move3d", json!({"wire": "w1", "to": null})),
        ("sketch.move3d", json!({"wire": "w1", "by": "up"})),
        ("sketch.move3d", json!({"wire": "w1", "by": null})),
        ("sketch.move3d", json!({"wire": "w1", "to": [0, 0, 0], "by": [1]})),
        ("sketch.move3d", json!({"wire": "w1", "to": [0, 0, 0], "sketch": true})),
    ]);
}

/// A session with a 3D sketch being edited, and the ids of a drawn line's curves.
fn three_d(level: ParamCheck) -> (Session, Vec<String>) {
    let mut s = Session { param_check: level, ..Session::default() };
    for (id, p) in [("sketch.create", json!({"plane": "XY", "name": "Path"})), ("sketch.options", json!({"sketch_3d": true}))] {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    }
    let drawn = s.execute("sketch.line3d", &json!({"points": [[0, 0, 0], [0, 0, 10], [10, 0, 10]]})).unwrap_or_else(|e| panic!("{e}"));
    let ids = drawn["curves"].as_array().into_iter().flatten().filter_map(|c| c.as_str().map(String::from)).collect();
    (s, ids)
}

/// Calls the handlers take today (every form of each key, `null` for `sketch`) get no issue from
/// the contracts; calls they refuse are refused by strict with the same message.
#[test]
fn the_types_read_each_key_as_the_handlers_do() {
    let (_, ids) = three_d(ParamCheck::Off);
    let wire = ids[0].clone();
    let cases = [
        ("sketch.line3d", json!({"points": [[0, 0, 0], [1, 1, 1]]})),
        ("sketch.line3d", json!({"points": [{"x": 0, "y": 0, "z": 0}, [1, 1, 1], [2, 2, 2]], "sketch": null})),
        ("sketch.line3d", json!({"points": [[0, 0, 0], [1, 1, 1]], "sketch": "Path"})),
        ("sketch.line3d", json!({"points": [[0, 0, 0], [1, 1, 1]], "sketch": 9999})),
        ("sketch.line3d", json!({"points": [[0, 0, 0], [1, 1, 1]], "sketch": "no such sketch"})),
        ("sketch.line3d", json!({"points": [[0, 0, 0], [1e9, 1, 1]]})),
        ("sketch.line3d", json!({"points": (0..500).map(|i| json!([i, 0, 0])).collect::<Vec<_>>()})),
        ("sketch.spline3d", json!({"points": [[0, 0, 0], [1, 1, 1], [2, 0, 3]]})),
        ("sketch.spline3d", json!({"points": [[0, 0, 0], [1, 1, 1]], "sketch": null})),
        ("sketch.point3d", json!({"point": [3, 4, 5]})),
        ("sketch.point3d", json!({"point": {"x": 3, "y": 4, "z": 5}, "sketch": "Path"})),
        ("sketch.move3d", json!({"wire": wire, "to": [1, 2, 3]})),
        ("sketch.move3d", json!({"wire": wire, "index": 1, "by": [1, 0, 0]})),
        ("sketch.move3d", json!({"wire": wire, "index": 0, "to": [1, 2, 3], "by": [1, 0, 0]})),
        ("sketch.move3d", json!({"wire": wire, "by": {"x": 1, "y": 0, "z": 0}, "sketch": null})),
        ("sketch.move3d", json!({"wire": wire, "by": [1, 0, 0], "to": null})),
        ("sketch.move3d", json!({"wire": wire})),
        ("sketch.move3d", json!({"wire": "no such wire", "to": [1, 2, 3]})),
        ("sketch.move3d", json!({"wire": wire, "index": 7, "to": [1, 2, 3]})),
    ];
    for (id, p) in cases {
        let (mut off, _) = three_d(ParamCheck::Off);
        let (mut strict, _) = three_d(ParamCheck::Strict);
        let (mut warn, _) = three_d(ParamCheck::Warn);
        let today = off.execute(id, &p).map_err(|e| e.to_string());
        let checked = strict.execute(id, &p).map_err(|e| e.to_string());
        let warned = warn.execute(id, &p);
        let issues: Vec<_> = crate::schema::issues(id, &p).into_iter().map(|i| i.message).collect();
        match &today {
            // A refusal the handler gives about the document (no such sketch, wire or point).
            Err(e) if issues.is_empty() => {
                let document = ["no sketch", "is not an editable drawn 3D curve", "has no drawn point", "give `to` or `by`"];
                assert!(document.iter().any(|m| e.contains(m)), "{id} {p}: refused for another reason: {e}");
                assert_eq!(checked.as_ref().err(), Some(e), "{id} {p}");
                assert!(warned.is_err(), "{id} {p}");
            }
            Err(e) => assert_eq!(checked.as_ref().err(), Some(e), "{id} {p}: {issues:?}"),
            Ok(reply) => {
                assert!(issues.is_empty(), "{id} {p}: {issues:?}");
                assert_eq!(checked.as_ref().ok(), Some(reply), "{id} {p}");
                assert_eq!(warned.ok().as_ref(), Some(reply), "{id} {p}");
            }
        }
    }
}

/// The schemas say what the handlers need: the keys they refuse without are required, `to` or
/// `by` is needed, and a point is `[x, y, z]` or `{x, y, z}`.
#[test]
fn the_schemas_take_what_the_handlers_read() {
    let takes = |schema: schemars::Schema, cases: &[(Value, bool)]| {
        let schema = schema.as_value();
        for (p, takes) in cases {
            assert_eq!(conforms(schema, schema, p), *takes, "{p}");
        }
    };
    let two = json!([[0, 0, 0], [1, 1, 1]]);
    for schema in [input_schema_for::<Line3dParams>(), input_schema_for::<Spline3dParams>()] {
        takes(
            schema,
            &[
                (json!({"points": two}), true),
                (json!({"points": [{"x": 0, "y": 0, "z": 0}, [1, 1, 1]], "sketch": "Path"}), true),
                (json!({"points": two, "sketch": 4}), true),
                (json!({}), false),
                (json!({"points": [[0, 0, 0]]}), false),
                (json!({"points": [[0, 0, 0], [1, 1]]}), false),
                (json!({"points": two, "sketch": true}), false),
            ],
        );
    }
    takes(
        input_schema_for::<Point3dParams>(),
        &[
            (json!({"point": [1, 2, 3]}), true),
            (json!({"point": {"x": 1, "y": 2, "z": 3}}), true),
            (json!({}), false),
            (json!({"point": [1, 2]}), false),
        ],
    );
    takes(
        input_schema_for::<Move3dParams>(),
        &[
            (json!({"wire": "w1", "to": [1, 2, 3]}), true),
            (json!({"wire": "w1", "by": [1, 2, 3], "index": 499}), true),
            (json!({"wire": "w1", "to": [1, 2, 3], "by": [1, 2, 3], "sketch": "Path"}), true),
            (json!({"to": [1, 2, 3]}), false),
            (json!({"wire": "w1"}), false),
            (json!({"wire": "w1", "to": [1, 2, 3], "index": 500}), false),
            (json!({"wire": "w1", "to": [1, 2, 3], "index": -1}), false),
            (json!({"wire": 3, "to": [1, 2, 3]}), false),
            (json!({"point": "l1.end", "by": [0, 0, 1]}), true),
            (json!({"point": "p3", "wire": "w1", "to": [1, 2, 3]}), false),
            (json!({"point": 3, "to": [1, 2, 3]}), false),
            (json!({"point": "p3"}), false),
        ],
    );
}

fn matches(schema: schemars::Schema, id: &str, reply: &Value) {
    let schema = schema.as_value();
    assert!(conforms(schema, schema, reply), "{id}: {reply} doesn't match {schema}");
    assert!(reply.get("param_warnings").is_none(), "{id}: {reply}");
}

/// Each command, in Warn: the real reply matches its output schema and carries no warning.
#[test]
fn replies_match_the_output_schema() {
    let (mut s, _) = three_d(ParamCheck::Warn);
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    let line = run("sketch.line3d", json!({"points": [[0, 5, 0], [0, 5, 10], [10, 5, 10]]}));
    assert_eq!(line["curves"].as_array().map(Vec::len), Some(2), "{line}");
    matches(output_schema_for::<Curves3dResult>(), "sketch.line3d", &line);
    let by_name = run("sketch.line3d", json!({"points": [[0, 6, 0], [0, 6, 10]], "sketch": "Path"}));
    matches(output_schema_for::<Curves3dResult>(), "sketch.line3d", &by_name);
    let spline = run("sketch.spline3d", json!({"points": [[0, 8, 0], [5, 9, 5], [10, 8, 10]]}));
    assert_eq!(spline["curves"].as_array().map(Vec::len), Some(1), "{spline}");
    matches(output_schema_for::<Curves3dResult>(), "sketch.spline3d", &spline);
    let point = run("sketch.point3d", json!({"point": {"x": 3, "y": 4, "z": 5}}));
    assert!(point["point"].is_string(), "{point}");
    matches(output_schema_for::<Point3dResult>(), "sketch.point3d", &point);
    let curve = line["curves"][0].clone();
    for p in [
        json!({"wire": curve, "index": 1, "by": [0, 0, 5]}),
        json!({"wire": curve, "to": [1, 5, 1]}),
        json!({"wire": point["point"], "to": [3, 4, 6], "by": [9, 9, 9]}),
    ] {
        let moved = run("sketch.move3d", p);
        assert!(moved["moved"].as_u64().is_some_and(|n| n >= 1), "{moved}");
        matches(output_schema_for::<Move3dResult>(), "sketch.move3d", &moved);
    }
}

/// A misspelt key still runs in Warn, and says so, with a reply that still matches.
#[test]
fn a_misspelt_key_is_reported_in_warn() {
    let (mut s, ids) = three_d(ParamCheck::Warn);
    let calls = [
        ("sketch.line3d", json!({"points": [[0, 5, 0], [0, 5, 10]], "skech": "Path"})),
        ("sketch.spline3d", json!({"points": [[0, 5, 0], [0, 5, 10]], "skech": "Path"})),
        ("sketch.point3d", json!({"point": [1, 2, 3], "pont": [1, 2, 3]})),
        ("sketch.move3d", json!({"wire": ids[0], "to": [1, 2, 3], "inex": 1})),
    ];
    for (id, p) in calls {
        let reply = s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
        let warnings = reply["param_warnings"].as_array().cloned().unwrap_or_default();
        assert!(warnings.len() == 1 && warnings[0].as_str().is_some_and(|w| w.contains("unknown key")), "{id}: {reply}");
        let schema = match id {
            "sketch.point3d" => output_schema_for::<Point3dResult>(),
            "sketch.move3d" => output_schema_for::<Move3dResult>(),
            _ => output_schema_for::<Curves3dResult>(),
        };
        assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{id}: {reply}");
    }
}

/// The warning for what a key gets wrong is the handler's own wording.
#[test]
fn the_check_names_what_is_wrong_with_a_key() {
    let issues = |id: &str, p: Value| crate::schema::issues(id, &p).into_iter().map(|i| i.message).collect::<Vec<_>>();
    assert_eq!(issues("sketch.line3d", json!({"points": [[0, 0, 0]]})), ["`points` needs 2 to 500 points"]);
    assert_eq!(issues("sketch.line3d", json!({"points": [[0, 0, 0], [1]]})), ["a point must be [x, y, z]"]);
    assert_eq!(issues("sketch.spline3d", json!({"points": 1})), ["`points` must list [x,y,z] points"]);
    assert_eq!(issues("sketch.point3d", json!({"point": [1]})), ["`point` must be a point [x, y, z]"]);
    assert_eq!(issues("sketch.move3d", json!({"wire": 1})), ["`wire` must be a 3D curve or point id"]);
    assert_eq!(issues("sketch.move3d", json!({"wire": "w", "index": 500})), ["`index` must be a fit point index below 500"]);
    assert_eq!(issues("sketch.move3d", json!({"wire": "w", "by": 1})), ["`by` must be [x,y,z]"]);
    assert_eq!(issues("sketch.line3d", json!({"points": [[0, 0, 0], [1, 1, 1]], "sketch": true})), ["`sketch` must be an id or a name"]);
}
