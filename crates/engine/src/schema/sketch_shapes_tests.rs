use serde_json::{Value, json};

use super::{
    ArcCenterPointParams, ArcThreePointParams, CircleCenterParams, CircleThreePointParams, CircleTwoPointParams, DrawResult, LineParams, PointParams,
    PointResult,
};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, issues, output_schema_for};

/// A session in a new sketch that holds one line, `l1`, from [0, 0] to [10, 0].
fn session(level: ParamCheck) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]], "ids": ["l1"]}));
    s
}

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<LineParams>(), &["closed", "connect", "construction", "ids", "infer", "points", "sketch"]);
    assert_keys(input_schema_for::<PointParams>(), &["connect", "id", "point", "sketch"]);
    assert_keys(input_schema_for::<CircleCenterParams>(), &["center", "connect", "construction", "diameter", "id", "radius", "sketch", "through"]);
    assert_keys(input_schema_for::<CircleTwoPointParams>(), &["connect", "construction", "p0", "p1", "sketch"]);
    assert_keys(input_schema_for::<CircleThreePointParams>(), &["connect", "construction", "p0", "p1", "p2", "sketch"]);
    assert_keys(input_schema_for::<ArcThreePointParams>(), &["connect", "construction", "end", "id", "sketch", "start", "through"]);
    assert_keys(
        input_schema_for::<ArcCenterPointParams>(),
        &["center", "clockwise", "connect", "construction", "end", "id", "sketch", "start", "sweep"],
    );
}

/// Calls the handler refuses fail in strict with the same message. (The shared helper finishes
/// its sketch first; these need one being edited.)
#[test]
fn strict_keeps_todays_errors() {
    let cases = [
        ("sketch.line", json!({"points": 5})),
        ("sketch.line", json!({"points": [[0, 0]]})),
        ("sketch.line", json!({"points": [[0, 0], "zz"]})),
        ("sketch.line", json!({"points": [[0, 0], [1]]})),
        ("sketch.line", json!({"points": [[0, 0], [true, "a"]]})),
        ("sketch.line", json!({"points": [[0, 0], ["", 1]]})),
        ("sketch.line", json!({"points": [[0, 0], [5, 5]], "ids": 5})),
        ("sketch.line", json!({"points": [[0, 0], [5, 5]], "ids": [1]})),
        ("sketch.line", json!({"points": [[0, 0], [5, 5]], "ids": ["a|b"]})),
        ("sketch.line", json!({"points": [[0, 0], [5, 5]], "ids": "a,b"})),
        ("sketch.line", json!({"points": [[0, 0], {"vertex": "x"}]})),
        ("sketch.point", json!({"point": "l1.end"})),
        ("sketch.point", json!({"point": [1]})),
        ("sketch.point", json!({"point": ["", 2]})),
        ("sketch.point", json!({"point": [1, true]})),
        ("sketch.point", json!({"point": "mid:nope"})),
        ("sketch.point", json!({"point": {"vertex": "x"}})),
        ("sketch.point", json!({"point": "on:l1:x"})),
        ("sketch.point", json!({"point": "vertex:1,2"})),
        ("sketch.point", json!({"point": [1, 2], "id": "a|b"})),
        ("sketch.circle.center", json!({"center": [0, 0]})),
        ("sketch.circle.center", json!({"center": [0, 0], "radius": true})),
        ("sketch.circle.center", json!({"center": [0, 0], "radius": " "})),
        ("sketch.circle.center", json!({"center": [0, 0], "radius": 1e12})),
        ("sketch.circle.center", json!({"center": [0, 0], "radius": 0})),
        ("sketch.circle.center", json!({"center": [0, 0], "diameter": [1]})),
        ("sketch.circle.center", json!({"center": 5, "radius": 1})),
        ("sketch.circle.center", json!({"center": [0, 0], "radius": 1, "through": null})),
        ("sketch.circle.center", json!({"center": [0, 0], "radius": 1, "id": "a]"})),
        ("sketch.circle.two_point", json!({"p0": "l1.end", "p1": [1, 1]})),
        ("sketch.circle.two_point", json!({"p0": [0, 0], "p1": ["1", 1]})),
        ("sketch.circle.two_point", json!({"p0": [0, 0], "p1": [0, 0]})),
        ("sketch.circle.three_point", json!({"p0": [0, 0], "p1": [1, 0], "p2": [2, 0]})),
        ("sketch.circle.three_point", json!({"p0": [0, 0], "p1": 5, "p2": [2, 3]})),
        ("sketch.arc.three_point", json!({"start": [0, 0], "end": [10, 0], "through": [5, 0]})),
        ("sketch.arc.three_point", json!({"start": [0, 0], "end": [10, 0], "through": "zz"})),
        ("sketch.arc.three_point", json!({"start": [0, 0], "end": [10, 0], "through": [5, 5], "id": "a|b"})),
        ("sketch.arc.center_point", json!({"center": [0, 0], "start": [5, 0], "sweep": 0})),
        ("sketch.arc.center_point", json!({"center": [0, 0], "start": [5, 0], "sweep": 400})),
        ("sketch.arc.center_point", json!({"center": [0, 0], "start": [5, 0], "end": true})),
        ("sketch.arc.center_point", json!({"center": [0, 0], "start": [5, 0], "end": null})),
        ("sketch.arc.center_point", json!({"center": [0, 0], "start": [5, 0]})),
    ];
    for (id, p) in &cases {
        let error = |level| session(level).execute(id, p).err().map(|e| e.to_string());
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{id} {p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{id} {p}");
    }
}

/// Every form each command takes is accepted by the contract: run in warn, the reply has no
/// warnings and matches the output schema.
#[test]
fn replies_match_the_output_schema_and_forms_are_accepted() {
    let draw = output_schema_for::<DrawResult>();
    let draw = draw.as_value();
    let point = output_schema_for::<PointResult>();
    let point = point.as_value();
    let calls = [
        ("sketch.line", json!({"points": [[0, 5], [10, 5]]})),
        ("sketch.line", json!({"points": ["l1.end", [20, 0]], "infer": true})),
        ("sketch.line", json!({"points": [{"ref": "l1.start"}, {"at": [1, 2]}], "construction": true})),
        ("sketch.line", json!({"points": [[30, 0], ["5 mm", 3], {"x": 40, "y": 9}], "closed": true, "ids": "solo", "connect": false})),
        ("sketch.line", json!({"points": [[50, 0], [60, 0]], "ids": ["a", "b"], "sketch": null})),
        ("sketch.circle.center", json!({"center": [50, 50], "radius": 2})),
        ("sketch.circle.center", json!({"center": [50, 60], "diameter": 4, "id": "ring", "construction": true})),
        ("sketch.circle.center", json!({"center": [50, 70], "radius": "3 mm", "connect": false})),
        ("sketch.circle.center", json!({"center": ["1 mm", 80], "diameter": "6 mm", "through": "l1.end"})),
        ("sketch.circle.two_point", json!({"p0": [70, 0], "p1": [80, 0]})),
        ("sketch.circle.two_point", json!({"p0": {"x": 70, "y": 10}, "p1": {"x": 80, "y": 10}, "construction": false})),
        ("sketch.circle.three_point", json!({"p0": [90, 0], "p1": [100, 10], "p2": [110, 0]})),
        ("sketch.circle.three_point", json!({"p0": "l1.start", "p1": {"at": [0, -10]}, "p2": [-10, 0]})),
        ("sketch.arc.three_point", json!({"start": [0, 20], "end": [10, 20], "through": [5, 25], "id": "arc_one"})),
        ("sketch.arc.three_point", json!({"start": [0, 30], "end": [10, 30], "through": [5, 35], "construction": true})),
        ("sketch.arc.center_point", json!({"center": [0, 40], "start": [5, 40], "end": [0, 45]})),
        ("sketch.arc.center_point", json!({"center": [0, 50], "start": [5, 50], "end": [0, 55], "clockwise": true, "id": "arc_two"})),
        ("sketch.arc.center_point", json!({"center": [0, 60], "start": [5, 60], "sweep": 90})),
        ("sketch.arc.center_point", json!({"center": [0, 70], "start": [5, 70], "sweep": -45, "end": [0, 75]})),
    ];
    let point_calls = [
        json!({"point": [30, 30]}),
        json!({"point": {"x": 31, "y": 31}, "id": "pt"}),
        json!({"point": ["10 mm", 40], "connect": false}),
        json!({"point": "mid:l1"}),
        json!({"point": "on:l1:5,0"}),
    ];
    let mut s = session(ParamCheck::Warn);
    for (id, p) in &calls {
        let r = run(&mut s, id, p.clone());
        assert!(conforms(draw, draw, &r), "{id} {p}: {r} doesn't match {draw}");
        assert!(r.get("param_warnings").is_none(), "{id} {p}: {r}");
        assert!(r["created"]["curves"].is_array(), "{r}");
    }
    for p in &point_calls {
        let r = run(&mut s, "sketch.point", p.clone());
        assert!(conforms(point, point, &r), "{p}: {r} doesn't match {point}");
        assert!(r.get("param_warnings").is_none(), "{p}: {r}");
    }
    // A misspelt key still runs in warn and is named.
    let r = run(&mut s, "sketch.line", json!({"points": [[0, 90], [10, 90]], "constructon": true}));
    assert!(conforms(draw, draw, &r), "{r} doesn't match {draw}");
    assert!(r["param_warnings"].as_array().is_some_and(|w| w.len() == 1), "{r}");
    let r = run(&mut s, "sketch.point", json!({"point": [3, 95], "constructon": true}));
    assert!(conforms(point, point, &r), "{r} doesn't match {point}");
    assert!(r["param_warnings"].as_array().is_some_and(|w| w.len() == 1), "{r}");
}

/// The same calls give the same JSON with the check off and in strict.
#[test]
fn the_check_does_not_change_the_reply() {
    let calls = [
        ("sketch.line", json!({"points": [[0, 5], [10, 5], [10, 15]], "closed": true, "ids": ["a", "b", "c"]})),
        ("sketch.point", json!({"point": [3, 3]})),
        ("sketch.point", json!({"point": "mid:l1"})),
        ("sketch.circle.center", json!({"center": [50, 50], "radius": 2, "id": "c1"})),
        ("sketch.circle.two_point", json!({"p0": [70, 0], "p1": [80, 0]})),
        ("sketch.circle.three_point", json!({"p0": [90, 0], "p1": [100, 10], "p2": [110, 0]})),
        ("sketch.arc.three_point", json!({"start": [0, 20], "end": [10, 20], "through": [5, 25]})),
        ("sketch.arc.center_point", json!({"center": [0, 40], "start": [5, 40], "sweep": 90})),
    ];
    let (mut off, mut strict) = (session(ParamCheck::Off), session(ParamCheck::Strict));
    for (id, p) in &calls {
        let (a, b) = (run(&mut off, id, p.clone()), run(&mut strict, id, p.clone()));
        assert_eq!(a, b, "{id} {p}");
        assert!(a["created"]["points"].is_array() && a["sketch"]["dof"].is_number(), "{a}");
    }
    // Key order and spelling of a reply, as the handlers wrote it before the types.
    let r = run(&mut off, "sketch.point", json!({"point": [4, 4]}));
    assert_eq!(
        r.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()),
        Some(vec!["constraints".into(), "created".into(), "point".into(), "sketch".into()])
    );
}

/// Values of the wrong type that a handler ignores or reads as left out are refused by the
/// contract: that is what warn reports and strict refuses.
#[test]
fn wrong_typed_keys_are_reported() {
    let base = [
        ("sketch.line", json!({"points": [[0, 0], [5, 5]]})),
        ("sketch.point", json!({"point": [1, 1]})),
        ("sketch.circle.center", json!({"center": [0, 0], "radius": 1})),
        ("sketch.circle.two_point", json!({"p0": [0, 0], "p1": [5, 5]})),
        ("sketch.circle.three_point", json!({"p0": [0, 0], "p1": [5, 5], "p2": [10, 0]})),
        ("sketch.arc.three_point", json!({"start": [0, 0], "end": [10, 0], "through": [5, 5]})),
        ("sketch.arc.center_point", json!({"center": [0, 0], "start": [5, 0], "sweep": 90})),
    ];
    for (id, p) in &base {
        assert!(issues(id, p).is_empty(), "{id} {p}");
        for key in ["connect", "construction", "closed", "infer", "clockwise"] {
            let mut bad = p.clone();
            bad[key] = json!("yes");
            if input_schema_has(id, key) {
                assert_eq!(issues(id, &bad).len(), 1, "{id} {key}: {:?}", issues(id, &bad));
            }
        }
        let mut bad = p.clone();
        bad["sketch"] = json!(true);
        assert_eq!(issues(id, &bad)[0].message, "`sketch` must be an id or a name");
    }
    let message = |id: &str, p: Value| issues(id, &p).first().map(|i| i.message.clone());
    assert_eq!(message("sketch.point", json!({"point": [1, 1], "id": 5})).as_deref(), Some("`id` must be text"));
    assert_eq!(
        message("sketch.arc.center_point", json!({"center": [0, 0], "start": [5, 0], "sweep": "90"})).as_deref(),
        Some("`sweep` must be a number of degrees")
    );
    assert_eq!(message("sketch.circle.center", json!({"center": [0, 0], "radius": 1, "through": {"vertex": [1, 2, 3]}})), None);
    assert_eq!(message("sketch.point", json!({"point": {"on_edge": [1, 2, 3]}})), None);
    assert_eq!(message("sketch.point", json!({"point": "vertex:1,2,3"})), None);
    assert_eq!(message("sketch.line", json!({"points": [{"vertex": [1, 2, 3]}, {"on_edge": [1, 2, 3]}]})), None);
}

fn input_schema_has(id: &str, key: &str) -> bool {
    crate::cmd::find_command(id).and_then(|c| c.contract).is_some_and(|c| c.allowed_keys().contains(key))
}
