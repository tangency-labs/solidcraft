use serde_json::{Value, json};

use super::{
    ArcCenterPointParams, ArcThreePointParams, CircleCenterParams, CircleThreePointParams, CircleTwoPointParams, DrawResult, LineParams, PointParams,
    PointResult, PolygonCircumscribedParams, PolygonEdgeParams, PolygonInscribedParams, PolygonResult, RectCenterParams, RectResult,
    RectRoundedParams, RectThreePointParams, RectTwoPointParams, RoundedRectResult, SlotCenterToCenterParams, SlotOverallParams,
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
    assert_keys(input_schema_for::<SlotCenterToCenterParams>(), &["connect", "p0", "p1", "sketch", "width"]);
    assert_keys(input_schema_for::<SlotOverallParams>(), &["connect", "p0", "p1", "sketch", "width"]);
    assert_keys(
        input_schema_for::<ArcCenterPointParams>(),
        &["center", "clockwise", "connect", "construction", "end", "id", "sketch", "start", "sweep"],
    );
    assert_keys(input_schema_for::<RectTwoPointParams>(), &["connect", "construction", "p0", "p1", "sketch"]);
    assert_keys(input_schema_for::<RectThreePointParams>(), &["connect", "construction", "p0", "p1", "p2", "sketch"]);
    assert_keys(input_schema_for::<RectCenterParams>(), &["center", "connect", "construction", "corner", "sketch"]);
    assert_keys(input_schema_for::<RectRoundedParams>(), &["connect", "construction", "p0", "p1", "radius", "sketch"]);
    // The polygons draw no construction geometry: their handlers never read `construction`.
    assert_keys(input_schema_for::<PolygonInscribedParams>(), &["angle", "center", "connect", "diameter", "radius", "sides", "sketch"]);
    assert_keys(input_schema_for::<PolygonCircumscribedParams>(), &["angle", "center", "connect", "diameter", "radius", "sides", "sketch"]);
    assert_keys(input_schema_for::<PolygonEdgeParams>(), &["connect", "p0", "p1", "sides", "sketch"]);
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
        ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [0, 5]})),
        ("sketch.rectangle.two_point", json!({"p0": "l1.end", "p1": [5, 5]})),
        ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [5, true]})),
        ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": ["", 5]})),
        ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": ["5", true]})),
        ("sketch.rectangle.center", json!({"center": [0, 0], "corner": [0, 5]})),
        ("sketch.rectangle.center", json!({"center": {"ref": "l1.end"}, "corner": [5, 5]})),
        ("sketch.rectangle.rounded", json!({"p0": [0, 0], "p1": [10, 10], "radius": 6})),
        ("sketch.rectangle.rounded", json!({"p0": [0, 0], "p1": [10, 10], "radius": 0})),
        ("sketch.rectangle.rounded", json!({"p0": [0, 0], "p1": [10, 10], "radius": true})),
        ("sketch.rectangle.rounded", json!({"p0": [0, 0], "p1": [1], "radius": 1})),
        ("sketch.rectangle.three_point", json!({"p0": [0, 0], "p1": [0, 0], "p2": [1, 1]})),
        ("sketch.rectangle.three_point", json!({"p0": [0, 0], "p1": [5, 0], "p2": [9, 0]})),
        ("sketch.rectangle.three_point", json!({"p0": "l1.end", "p1": [5, 0], "p2": [5, 5]})),
        ("sketch.polygon.inscribed", json!({"center": [0, 0]})),
        ("sketch.polygon.inscribed", json!({"center": [0, 0], "radius": 0})),
        ("sketch.polygon.inscribed", json!({"center": [0, 0], "radius": "5"})),
        ("sketch.polygon.inscribed", json!({"center": [0, 0], "radius": 5, "sides": 2})),
        ("sketch.polygon.inscribed", json!({"center": "l1.end", "radius": 5})),
        ("sketch.polygon.circumscribed", json!({"center": [0, 0], "diameter": "x"})),
        ("sketch.polygon.circumscribed", json!({"center": [0, 0], "radius": 5, "sides": 201})),
        ("sketch.polygon.edge", json!({"p0": [0, 0], "p1": [0, 0]})),
        ("sketch.polygon.edge", json!({"p0": [0, 0], "p1": [5, 0], "sides": 2.5})),
        ("sketch.polygon.edge", json!({"p0": [0, 0], "p1": ["5", 0]})),
        ("sketch.slot.center_to_center", json!({"p1": [10, 0], "width": 4})),
        ("sketch.slot.center_to_center", json!({"p0": "l1.end", "p1": [10, 0], "width": 4})),
        ("sketch.slot.center_to_center", json!({"p0": ["1", 0], "p1": [10, 0], "width": 4})),
        ("sketch.slot.center_to_center", json!({"p0": [0, 5], "p1": [10, 5]})),
        ("sketch.slot.center_to_center", json!({"p0": [0, 5], "p1": [10, 5], "width": 0})),
        ("sketch.slot.center_to_center", json!({"p0": [0, 5], "p1": [10, 5], "width": -2})),
        ("sketch.slot.center_to_center", json!({"p0": [0, 5], "p1": [10, 5], "width": "4"})),
        ("sketch.slot.center_to_center", json!({"p0": [0, 5], "p1": [10, 5], "width": 1e12})),
        ("sketch.slot.center_to_center", json!({"p0": [3, 5], "p1": [3, 5], "width": 4})),
        ("sketch.slot.overall", json!({"p0": [0, 5], "p1": [10, 5]})),
        ("sketch.slot.overall", json!({"p0": [0, 5], "p1": 7, "width": 4})),
        ("sketch.slot.overall", json!({"p0": [0, 5], "p1": [10, 5], "width": true})),
        ("sketch.slot.overall", json!({"p0": [3, 5], "p1": [3, 5], "width": 4})),
        ("sketch.slot.overall", json!({"p0": [0, 5], "p1": [4, 5], "width": 4})),
        ("sketch.slot.overall", json!({"p0": [0, 5], "p1": [3, 5], "width": 4})),
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
        ("sketch.slot.center_to_center", json!({"p0": [0, 100], "p1": [20, 100], "width": 6})),
        ("sketch.slot.center_to_center", json!({"p0": {"x": 0, "y": 120}, "p1": {"x": 20, "y": 120}, "width": 0.5, "connect": false})),
        ("sketch.slot.overall", json!({"p0": [0, 140], "p1": [20, 140], "width": 6})),
        ("sketch.slot.overall", json!({"p0": [0, 160], "p1": [20, 170], "width": 6, "connect": true, "sketch": null})),
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
        ("sketch.slot.center_to_center", json!({"p0": [0, 100], "p1": [20, 100], "width": 6})),
        ("sketch.slot.overall", json!({"p0": [0, 140], "p1": [20, 140], "width": 6})),
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
        ("sketch.slot.center_to_center", json!({"p0": [0, 5], "p1": [10, 5], "width": 4})),
        ("sketch.slot.overall", json!({"p0": [0, 5], "p1": [10, 5], "width": 4})),
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

/// The rectangles and polygons (#13196): every form each takes is accepted (run in warn, no
/// warnings), and each reply matches its own output schema.
#[test]
fn rectangles_and_polygons_reply_with_their_types() {
    let schemas = [
        ("rect", output_schema_for::<RectResult>()),
        ("draw", output_schema_for::<DrawResult>()),
        ("rounded", output_schema_for::<RoundedRectResult>()),
        ("polygon", output_schema_for::<PolygonResult>()),
    ];
    let calls = [
        ("rect", "sketch.rectangle.two_point", json!({"p0": [100, 0], "p1": [110, 10]})),
        ("rect", "sketch.rectangle.two_point", json!({"p0": {"x": 100, "y": 20}, "p1": ["110 mm", 30], "construction": true, "connect": false})),
        ("rect", "sketch.rectangle.two_point", json!({"p0": [100, 40], "p1": [110, 50], "sketch": null})),
        ("draw", "sketch.rectangle.three_point", json!({"p0": [120, 0], "p1": [130, 0], "p2": [125, 5]})),
        ("draw", "sketch.rectangle.three_point", json!({"p0": {"x": 120, "y": 20}, "p1": [130, 25], "p2": [120, 30], "construction": true})),
        ("rect", "sketch.rectangle.center", json!({"center": [150, 5], "corner": [155, 10]})),
        ("rect", "sketch.rectangle.center", json!({"center": ["150 mm", 25], "corner": [145, 20], "connect": false})),
        ("rounded", "sketch.rectangle.rounded", json!({"p0": [170, 0], "p1": [190, 10], "radius": 2})),
        ("rounded", "sketch.rectangle.rounded", json!({"p0": [170, 20], "p1": [190, 30], "radius": "1 mm", "construction": true})),
        ("polygon", "sketch.polygon.inscribed", json!({"center": [210, 5], "radius": 4})),
        ("polygon", "sketch.polygon.inscribed", json!({"center": {"x": 210, "y": 25}, "diameter": 8, "sides": 5, "angle": 0})),
        ("polygon", "sketch.polygon.circumscribed", json!({"center": [230, 5], "radius": 4, "sides": 8})),
        ("polygon", "sketch.polygon.circumscribed", json!({"center": [230, 25], "diameter": 8, "angle": 45, "connect": false})),
        ("polygon", "sketch.polygon.edge", json!({"p0": [250, 0], "p1": [255, 0]})),
        ("polygon", "sketch.polygon.edge", json!({"p0": [250, 20], "p1": [255, 20], "sides": 4})),
    ];
    let mut s = session(ParamCheck::Warn);
    for (kind, id, p) in &calls {
        let schema = schemas.iter().find(|(k, _)| k == kind).map(|(_, s)| s.as_value()).unwrap();
        let r = run(&mut s, id, p.clone());
        assert!(conforms(schema, schema, &r), "{id} {p}: {r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{id} {p}: {r}");
        assert!(r["created"]["curves"].is_array(), "{r}");
    }
    // The replies' keys, as the handlers wrote them before the types: a rounded rectangle names
    // its arcs and radius parameter, and no corners.
    let keys = |r: &Value| r.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default();
    let mut off = session(ParamCheck::Off);
    let r = run(&mut off, "sketch.rectangle.two_point", json!({"p0": [100, 0], "p1": [110, 10]}));
    assert_eq!(keys(&r), ["constraints", "corners", "created", "curves", "sides", "sketch"]);
    assert!(r["sides"]["bottom"].is_string() && r["corners"]["top_left"].is_string(), "{r}");
    let r = run(&mut off, "sketch.rectangle.rounded", json!({"p0": [170, 0], "p1": [190, 10], "radius": 2}));
    assert_eq!(keys(&r), ["arcs", "centre", "constraints", "created", "curves", "diagonal", "param", "sides", "sketch"]);
    assert_eq!((r["curves"].as_array().map(Vec::len), r["arcs"].as_array().map(Vec::len)), (Some(8), Some(4)), "{r}");
    assert!(r["param"].as_str().is_some_and(|name| !name.is_empty()), "{r}");
    let r = run(&mut off, "sketch.polygon.inscribed", json!({"center": [210, 5], "radius": 4}));
    assert_eq!(keys(&r), ["constraints", "created", "curves", "sketch", "support_circle"]);
    assert_eq!(r["curves"].as_array().map(Vec::len), Some(6), "six sides by default: {r}");
    // The same calls give the same JSON with the check off and in strict.
    let (mut off, mut strict) = (session(ParamCheck::Off), session(ParamCheck::Strict));
    for (_, id, p) in &calls {
        assert_eq!(run(&mut off, id, p.clone()), run(&mut strict, id, p.clone()), "{id} {p}");
    }
}

/// What the rectangles and polygons ignore or read as left out today is reported: a number typed
/// as text, a key a polygon never reads.
#[test]
fn rectangles_and_polygons_report_wrong_typed_keys() {
    let message = |id: &str, p: Value| issues(id, &p).first().map(|i| i.message.clone());
    let polygon = json!({"center": [0, 0], "radius": 5});
    for key in ["sides", "angle"] {
        let mut bad = polygon.clone();
        bad[key] = json!("6");
        assert_eq!(message("sketch.polygon.inscribed", bad), Some(format!("`{key}` must be a number")));
    }
    let mut bad = polygon.clone();
    bad["construction"] = json!(true);
    assert_eq!(issues("sketch.polygon.inscribed", &bad).len(), 1, "a polygon draws no construction geometry");
    assert_eq!(
        message("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [5, 5], "construction": "yes"})).as_deref(),
        Some("`construction` must be true or false")
    );
    assert_eq!(
        message("sketch.rectangle.center", json!({"center": [0, 0], "corner": [5, 5], "sketch": true})).as_deref(),
        Some("`sketch` must be an id or a name")
    );
    assert_eq!(message("sketch.rectangle.rounded", json!({"p0": ["w / 2", 0], "p1": [5, 5], "radius": "r"})), None);
    assert_eq!(message("sketch.polygon.edge", json!({"p0": [0, 0], "p1": [5, 0], "sides": 4})), None);
}

/// Slot replies expose the construction centre line and its constrained middle point.
#[test]
fn slot_replies_include_their_construction_middle() {
    let mut s = session(ParamCheck::Off);
    for (id, p) in [
        ("sketch.slot.center_to_center", json!({"p0": [0, 5], "p1": [10, 5], "width": 4})),
        ("sketch.slot.overall", json!({"p0": [0, 20], "p1": [10, 20], "width": 4})),
    ] {
        let r = run(&mut s, id, p);
        let o = r.as_object().unwrap_or_else(|| panic!("{id}: {r}"));
        let mut keys: Vec<&str> = o.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["centre_line", "constraints", "created", "curves", "middle", "sketch"], "{id}: {r}");
        for k in ["curves", "constraints"] {
            let list = o[k].as_array().unwrap_or_else(|| panic!("{id} {k}: {r}"));
            assert!(!list.is_empty() && list.iter().all(Value::is_string), "{id} {k}: {r}");
        }
        assert_eq!(o["curves"].as_array().map(Vec::len), Some(4), "{id}: {r}");
        assert_eq!(sorted_keys(&o["sketch"]), Some(vec!["dof", "solved"]), "{id}: {r}");
        assert_eq!(sorted_keys(&o["created"]), Some(vec!["constraints", "curves", "dimensions", "points"]), "{id}: {r}");
        assert!(o["centre_line"].is_string() && o["middle"].is_string(), "{id}: {r}");
        let mut expected = o["curves"].as_array().cloned().unwrap_or_default();
        expected.push(o["centre_line"].clone());
        assert_eq!(o["created"]["curves"], json!(expected), "{id}: {r}");
    }
}

fn sorted_keys(v: &Value) -> Option<Vec<&str>> {
    v.as_object().map(|m| m.keys().map(String::as_str).collect())
}
