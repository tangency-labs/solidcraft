use serde_json::{Value, json};

use super::{
    BreakParams, BreakResult, ChamferDistanceAngleParams, ChamferEqualParams, ChamferResult, ChamferTwoParams, CircularPatternParams, ExtendParams,
    ExtendResult, FilletParams, FilletResult, MirrorParams, MirrorResult, MoveParams, MoveResult, OffsetBreakParams, OffsetBreakResult, OffsetParams,
    OffsetResult, OffsetSide, PatternResult, RectangularPatternParams, ScaleParams, ScaleResult, TransformParams, TransformResult, TrimOutcome,
    TrimParams, TrimPathParams, TrimPathResult, TrimResult,
};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, issues, output_schema_for};

/// A session with a sketch being edited that holds a square (10 mm either side of the origin) and
/// the ids of its four sides.
fn editing(level: ParamCheck) -> (Session, Vec<String>) {
    let mut s = Session { param_check: level, ..Session::default() };
    s.execute("sketch.create", &json!({"plane": "XY"})).unwrap_or_else(|e| panic!("{e}"));
    let r = s.execute("sketch.rectangle.center", &json!({"center": [0, 0], "corner": [10, 10]})).unwrap_or_else(|e| panic!("{e}"));
    let sides = r["created"]["curves"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
    (s, sides)
}

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn id_of(r: &Value, list: &str) -> String {
    r["created"][list][0].as_str().unwrap_or_else(|| panic!("{r}")).to_string()
}

fn error(level: ParamCheck, id: &str, p: &Value) -> Option<String> {
    let (mut s, _) = editing(level);
    s.execute(id, p).err().map(|e| e.to_string())
}

/// A call the handler already refuses gets the same message in strict, with a sketch being edited.
/// (`assert_strict_keeps_todays_errors` runs after the sketch is finished, so a refusal that comes
/// from the design's side would read "no sketch is being edited" there.)
fn assert_strict_keeps_todays_errors(cases: &[(&str, Value)]) {
    for (id, p) in cases {
        let off = error(ParamCheck::Off, id, p);
        assert!(off.is_some(), "{id} {p} should fail today");
        assert_eq!(error(ParamCheck::Strict, id, p), off, "{id} {p}");
    }
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<TrimParams>(), &["at", "curve", "sketch"]);
    assert_keys(input_schema_for::<TrimPathParams>(), &["dry_run", "points", "sketch"]);
    assert_keys(input_schema_for::<BreakParams>(), &["at", "curve", "points", "sketch"]);
    assert_keys(input_schema_for::<ExtendParams>(), &["at", "curve", "curves", "sketch"]);
    assert_keys(input_schema_for::<FilletParams>(), &["a", "b", "point", "points", "radius", "sketch"]);
    assert_keys(input_schema_for::<ChamferEqualParams>(), &["a", "b", "distance", "point", "points", "sketch"]);
    assert_keys(input_schema_for::<ChamferTwoParams>(), &["a", "b", "distance", "distance2", "point", "points", "sketch"]);
    assert_keys(input_schema_for::<ChamferDistanceAngleParams>(), &["a", "angle", "b", "distance", "point", "points", "sketch"]);
    assert_keys(
        input_schema_for::<OffsetParams>(),
        &["chain", "curves", "direction", "distance", "distance2", "flip", "instances", "side", "sketch", "symmetric"],
    );
    assert_keys(input_schema_for::<OffsetBreakParams>(), &["curve", "sketch"]);
    assert_keys(input_schema_for::<MirrorParams>(), &["entities", "line", "sketch"]);
    assert_keys(input_schema_for::<MoveParams>(), &["angle", "center", "copy", "entities", "rotate_constraints", "sketch", "translate"]);
    assert_keys(input_schema_for::<ScaleParams>(), &["base", "entities", "factor", "sketch"]);
    assert_keys(input_schema_for::<TransformParams>(), &["angle", "center", "entities", "factor", "sketch", "translate"]);
    assert_keys(input_schema_for::<CircularPatternParams>(), &["alignment", "angle", "center", "count", "entities", "pattern", "reversed", "sketch"]);
    assert_keys(
        input_schema_for::<RectangularPatternParams>(),
        &[
            "alignment",
            "alignment2",
            "count",
            "count2",
            "dir",
            "dir2",
            "direction",
            "direction2",
            "entities",
            "pattern",
            "reversed",
            "reversed2",
            "sketch",
            "spacing",
            "spacing2",
        ],
    );
}

#[test]
fn strict_keeps_todays_errors() {
    let many = |n: usize, item: Value| Value::Array(vec![item; n]);
    assert_strict_keeps_todays_errors(&[
        ("sketch.trim", json!({})),
        ("sketch.trim", json!({"curve": "l1"})),
        ("sketch.trim", json!({"curve": "l1", "at": null})),
        ("sketch.trim", json!({"curve": "l1", "at": [1]})),
        ("sketch.trim", json!({"curve": "l1", "at": "x"})),
        ("sketch.trim", json!({"at": [0, 0]})),
        ("sketch.trim", json!({"at": [0, 0], "curve": 5})),
        ("sketch.trim", json!({"at": [0, 0], "curve": null})),
        ("sketch.trim", json!({"at": [0, 0], "curve": "l1", "sketch": -1})),
        ("sketch.trim", json!({"at": [0, 0], "curve": "l1", "sketch": 1.5})),
        ("sketch.trim", json!({"at": [0, 0], "curve": "l1", "sketch": true})),
        ("sketch.trim.path", json!({})),
        ("sketch.trim.path", json!({"points": 5})),
        ("sketch.trim.path", json!({"points": null})),
        ("sketch.trim.path", json!({"points": [[0, 0]]})),
        ("sketch.trim.path", json!({"points": [[0, 0], "x"]})),
        ("sketch.trim.path", json!({"points": many(2001, json!([0, 0]))})),
        ("sketch.break", json!({})),
        ("sketch.break", json!({"curve": "l1"})),
        ("sketch.break", json!({"curve": "l1", "at": [1, 2, 3]})),
        ("sketch.break", json!({"curve": 7, "at": [0, 0]})),
        ("sketch.break", json!({"curve": "l1", "points": "x"})),
        ("sketch.break", json!({"curve": "l1", "points": null})),
        ("sketch.break", json!({"curve": "l1", "points": [[1, 2], "x"]})),
        ("sketch.break", json!({"curve": "l1", "points": many(1001, json!([0, 0]))})),
        ("sketch.extend", json!({})),
        ("sketch.extend", json!({"curve": "l1"})),
        ("sketch.extend", json!({"at": [0, 0]})),
        ("sketch.extend", json!({"curve": 3, "at": [0, 0]})),
        ("sketch.extend", json!({"curves": "x"})),
        ("sketch.extend", json!({"curves": null})),
        ("sketch.extend", json!({"curves": ["l1"]})),
        ("sketch.extend", json!({"curves": ["l1", 2]})),
        ("sketch.extend", json!({"curves": ["l1", "l2"], "curve": "l1"})),
        ("sketch.extend", json!({"curves": ["l1", "l2"], "curve": null})),
        ("sketch.extend", json!({"curves": ["l1", "l2"], "at": [0, 0]})),
        ("sketch.extend", json!({"curves": ["l1", "l2"], "at": [[0, 0]]})),
        ("sketch.extend", json!({"curves": ["l1", "l2"], "at": null})),
        ("sketch.fillet", json!({"point": "l1.end"})),
        ("sketch.fillet", json!({"point": "l1.end", "radius": true})),
        ("sketch.fillet", json!({"point": "l1.end", "radius": "  "})),
        ("sketch.fillet", json!({"point": "l1.end", "radius": 1e9})),
        ("sketch.fillet", json!({"radius": 1})),
        ("sketch.fillet", json!({"radius": 1, "point": 5})),
        ("sketch.fillet", json!({"radius": 1, "a": "l1"})),
        ("sketch.fillet", json!({"radius": 1, "a": "l1", "b": 2})),
        ("sketch.fillet", json!({"radius": 1, "a": 1, "b": "l2"})),
        ("sketch.fillet", json!({"radius": 1, "points": []})),
        ("sketch.fillet", json!({"radius": 1, "points": many(501, json!("l1.end"))})),
        ("sketch.fillet", json!({"radius": 1, "points": [5]})),
        ("sketch.fillet", json!({"radius": 1, "points": [{"a": "l1"}]})),
        ("sketch.fillet", json!({"radius": 1, "points": [{}]})),
        ("sketch.chamfer.equal_distance", json!({"point": "l1.end"})),
        ("sketch.chamfer.equal_distance", json!({"point": "l1.end", "distance": "2"})),
        ("sketch.chamfer.equal_distance", json!({"point": "l1.end", "distance": null})),
        ("sketch.chamfer.equal_distance", json!({"point": "l1.end", "distance": 1e9})),
        ("sketch.chamfer.equal_distance", json!({"distance": 2})),
        ("sketch.chamfer.equal_distance", json!({"distance": 2, "points": []})),
        ("sketch.chamfer.equal_distance", json!({"distance": 2, "points": [5]})),
        ("sketch.chamfer.two_distance", json!({"point": "l1.end", "distance2": 2})),
        ("sketch.chamfer.two_distance", json!({"point": "l1.end", "distance": 2})),
        ("sketch.chamfer.two_distance", json!({"point": "l1.end", "distance": 2, "distance2": "x"})),
        ("sketch.chamfer.two_distance", json!({"distance": 2, "distance2": 3, "a": "l1"})),
        ("sketch.chamfer.distance_angle", json!({"point": "l1.end", "distance": 2})),
        ("sketch.chamfer.distance_angle", json!({"point": "l1.end", "distance": 2, "angle": "45"})),
        ("sketch.chamfer.distance_angle", json!({"point": "l1.end", "angle": 45})),
        ("sketch.chamfer.distance_angle", json!({"distance": 2, "angle": 45})),
        ("sketch.offset", json!({})),
        ("sketch.offset", json!({"curves": []})),
        ("sketch.offset", json!({"curves": null, "distance": 1})),
        ("sketch.offset", json!({"curves": 5, "distance": 1})),
        ("sketch.offset", json!({"curves": [5], "distance": 1})),
        ("sketch.offset", json!({"curves": many(501, json!("l1")), "distance": 1})),
        ("sketch.offset", json!({"curves": ["l1"]})),
        ("sketch.offset", json!({"curves": ["l1"], "distance": true})),
        ("sketch.offset", json!({"curves": ["l1"], "distance": ""})),
        ("sketch.offset", json!({"curves": ["l1"], "distance": 1e9})),
        ("sketch.offset", json!({"curves": ["l1"], "distance": 1, "direction": "negative"})),
        ("sketch.offset", json!({"curves": ["l1"], "distance": 1, "direction": "Reverse"})),
        ("sketch.offset", json!({"curves": ["l1"], "distance": 1, "direction": "up"})),
        ("sketch.offset", json!({"curves": ["l1"], "distance": 1, "distance2": null})),
        ("sketch.offset", json!({"curves": ["l1"], "distance": 1, "distance2": true})),
        ("sketch.offset", json!({"curves": ["l1"], "distance": 1, "instances": 0})),
        ("sketch.offset", json!({"curves": ["l1"], "distance": 1, "instances": 101})),
        ("sketch.offset", json!({"curves": ["l1"], "distance": 1, "instances": 2.5})),
        ("sketch.offset", json!({"curves": ["l1"], "distance": 1, "instances": true})),
        ("sketch.offset", json!({"curves": ["l1"], "distance": 1, "instances": ["2"]})),
        ("sketch.offset", json!({"curves": ["l1"], "distance": 1, "instances": " "})),
        ("sketch.offset", json!({"curves": ["l1"], "distance": 1, "sketch": []})),
        ("sketch.offset.break", json!({})),
        ("sketch.offset.break", json!({"curve": 5})),
        ("sketch.offset.break", json!({"curve": null})),
        ("sketch.offset.break", json!({"curve": "l1", "sketch": -3})),
        ("sketch.mirror", json!({})),
        ("sketch.mirror", json!({"entities": ["l1"]})),
        ("sketch.mirror", json!({"entities": ["l1"], "line": 5})),
        ("sketch.mirror", json!({"entities": ["l1"], "line": null})),
        ("sketch.mirror", json!({"entities": ["l1"], "line": ["l2"]})),
        ("sketch.mirror", json!({"entities": 5, "line": "l2"})),
        ("sketch.mirror", json!({"entities": [5], "line": "l2"})),
        ("sketch.mirror", json!({"entities": ["l1"], "line": "l2", "sketch": 1.5})),
        ("sketch.move", json!({"entities": ["l1"], "translate": 5})),
        ("sketch.move", json!({"entities": ["l1"], "translate": null})),
        ("sketch.move", json!({"entities": ["l1"], "translate": [1]})),
        ("sketch.move", json!({"entities": ["l1"], "angle": "30"})),
        ("sketch.move", json!({"entities": ["l1"], "angle": null})),
        ("sketch.move", json!({"entities": ["l1"], "angle": 1e9})),
        ("sketch.move", json!({"entities": null})),
        ("sketch.move", json!({"entities": [true]})),
        ("sketch.move", json!({"entities": ["l1"], "center": true})),
        ("sketch.move", json!({"entities": ["l1"], "center": null})),
        ("sketch.move", json!({"entities": ["l1"], "center": [1]})),
        ("sketch.scale", json!({"entities": ["l1"]})),
        ("sketch.scale", json!({"entities": ["l1"], "factor": 0})),
        ("sketch.scale", json!({"entities": ["l1"], "factor": "2"})),
        ("sketch.scale", json!({"entities": ["l1"], "factor": 1e6})),
        ("sketch.scale", json!({"entities": ["l1"], "factor": 2, "base": true})),
        ("sketch.scale", json!({"entities": ["l1"], "factor": 2, "base": [1]})),
        ("sketch.scale", json!({"factor": 2})),
        ("sketch.scale", json!({"entities": "l1", "factor": 2, "sketch": false})),
        ("sketch.transform", json!({"entities": ["l1"], "factor": 0})),
        ("sketch.transform", json!({"entities": ["l1"], "factor": "2"})),
        ("sketch.transform", json!({"entities": ["l1"], "factor": 1e6})),
        ("sketch.transform", json!({"entities": ["l1"], "angle": "30"})),
        ("sketch.transform", json!({"entities": ["l1"], "angle": null})),
        ("sketch.transform", json!({"entities": ["l1"], "angle": true})),
        ("sketch.transform", json!({"entities": ["l1"], "translate": 5})),
        ("sketch.transform", json!({"entities": ["l1"], "translate": null})),
        ("sketch.transform", json!({"entities": ["l1"], "translate": [1]})),
        ("sketch.transform", json!({"entities": ["l1"], "center": true})),
        ("sketch.transform", json!({"entities": ["l1"], "center": null})),
        ("sketch.transform", json!({"entities": ["l1"], "center": [1]})),
        ("sketch.transform", json!({"entities": null})),
        ("sketch.transform", json!({"entities": [true]})),
        ("sketch.transform", json!({"factor": 2})),
        ("sketch.transform", json!({"entities": "l1", "translate": [1, 2], "sketch": false})),
        ("sketch.pattern.circular", json!({})),
        ("sketch.pattern.circular", json!({"entities": ["l1"], "center": [20, 0]})),
        ("sketch.pattern.circular", json!({"entities": ["l1"], "center": [20, 0], "count": true})),
        ("sketch.pattern.circular", json!({"entities": ["l1"], "center": [20, 0], "count": null})),
        ("sketch.pattern.circular", json!({"entities": ["l1"], "center": [20, 0], "count": 0})),
        ("sketch.pattern.circular", json!({"entities": ["l1"], "center": [20, 0], "count": 2.5})),
        ("sketch.pattern.circular", json!({"entities": ["l1"], "center": [20, 0], "count": 501})),
        ("sketch.pattern.circular", json!({"entities": ["l1"], "center": [20, 0], "count": 1e9})),
        ("sketch.pattern.circular", json!({"entities": ["l1"], "center": 5, "count": 3})),
        ("sketch.pattern.circular", json!({"entities": ["l1"], "center": null, "count": 3})),
        ("sketch.pattern.circular", json!({"entities": ["l1"], "count": 3})),
        ("sketch.pattern.circular", json!({"entities": ["l1"], "center": [20, 0], "count": 3, "angle": true})),
        ("sketch.pattern.circular", json!({"entities": ["l1"], "center": [20, 0], "count": 3, "angle": 0})),
        ("sketch.pattern.circular", json!({"entities": ["l1"], "center": [20, 0], "count": 3, "angle": 361})),
        ("sketch.pattern.circular", json!({"entities": ["l1"], "center": [20, 0], "count": 3, "alignment": "middle"})),
        ("sketch.pattern.circular", json!({"entities": ["l1"], "center": [20, 0], "count": 3, "alignment": null})),
        ("sketch.pattern.circular", json!({"entities": ["l1"], "center": [20, 0], "count": 3, "reversed": "yes"})),
        ("sketch.pattern.circular", json!({"entities": ["l1"], "center": [20, 0], "count": 3, "reversed": null})),
        ("sketch.pattern.circular", json!({"center": [20, 0], "count": 3})),
        ("sketch.pattern.circular", json!({"entities": 5, "center": [20, 0], "count": 3})),
        ("sketch.pattern.rectangular", json!({})),
        ("sketch.pattern.rectangular", json!({"entities": ["l1"], "count": 3})),
        ("sketch.pattern.rectangular", json!({"entities": ["l1"], "count": 3, "spacing": true})),
        ("sketch.pattern.rectangular", json!({"entities": ["l1"], "count": 3, "spacing": null})),
        ("sketch.pattern.rectangular", json!({"entities": ["l1"], "count": 3, "spacing": 1e9})),
        ("sketch.pattern.rectangular", json!({"entities": ["l1"], "spacing": 5})),
        ("sketch.pattern.rectangular", json!({"entities": ["l1"], "count": 0, "spacing": 5})),
        ("sketch.pattern.rectangular", json!({"entities": ["l1"], "count": 3, "spacing": 5, "count2": false})),
        ("sketch.pattern.rectangular", json!({"entities": ["l1"], "count": 3, "spacing": 5, "count2": 501})),
        ("sketch.pattern.rectangular", json!({"entities": ["l1"], "count": 3, "spacing": 5, "spacing2": [1]})),
        ("sketch.pattern.rectangular", json!({"entities": ["l1"], "count": 3, "spacing": 5, "dir": [0, 0]})),
        ("sketch.pattern.rectangular", json!({"entities": ["l1"], "count": 3, "spacing": 5, "dir": null})),
        ("sketch.pattern.rectangular", json!({"entities": ["l1"], "count": 3, "spacing": 5, "dir": "x"})),
        ("sketch.pattern.rectangular", json!({"entities": ["l1"], "count": 3, "spacing": 5, "dir2": [0, 0]})),
        ("sketch.pattern.rectangular", json!({"entities": ["l1"], "count": 3, "spacing": 5, "direction": 5})),
        ("sketch.pattern.rectangular", json!({"entities": ["l1"], "count": 3, "spacing": 5, "direction2": true})),
        ("sketch.pattern.rectangular", json!({"entities": ["l1"], "count": 3, "spacing": 5, "alignment": "up"})),
        ("sketch.pattern.rectangular", json!({"entities": ["l1"], "count": 3, "spacing": 5, "alignment2": 1})),
        ("sketch.pattern.rectangular", json!({"entities": ["l1"], "count": 3, "spacing": 5, "reversed": 1})),
        ("sketch.pattern.rectangular", json!({"entities": ["l1"], "count": 3, "spacing": 5, "reversed2": "no"})),
        ("sketch.pattern.rectangular", json!({"count": 3, "spacing": 5})),
    ]);
}

/// What the check says about a call, one message per problem.
fn warnings(id: &str, p: Value) -> Vec<String> {
    issues(id, &p).into_iter().map(|i| i.message).collect()
}

/// The keys a handler reads only in some cases are read only in those cases, so a call that works
/// today gets no warning for what the handler never looks at, and a call the handler refuses is
/// refused for its key with the handler's message.
#[test]
fn the_types_read_each_key_as_the_handler_does() {
    let none = Vec::<String>::new();
    // Break: `points` when the key is there, otherwise `at`.
    assert_eq!(warnings("sketch.break", json!({"curve": "l1", "points": [[0, 1]]})), none);
    assert_eq!(warnings("sketch.break", json!({"curve": "l1", "points": [[0, 1]], "at": 5})), none);
    assert_eq!(warnings("sketch.break", json!({"curve": "l1", "at": [0, 1]})), none);
    assert_eq!(warnings("sketch.break", json!({"curve": "l1", "at": 5})), ["`at` must be a point [x, y]"]);
    assert_eq!(warnings("sketch.break", json!({"curve": "l1", "points": null, "at": [0, 1]})), ["`points` must be a list of points"]);
    assert_eq!(warnings("sketch.break", json!({"curve": "l1", "points": [5]})), ["`points` must contain [x, y] points"]);
    // Extend: the two-curve form when `curves` is there.
    assert_eq!(warnings("sketch.extend", json!({"curve": "l1", "at": [0, 1]})), none);
    assert_eq!(warnings("sketch.extend", json!({"curves": ["l1", "l2"]})), none);
    assert_eq!(warnings("sketch.extend", json!({"curves": ["l1", "l2"], "at": [[0, 1], {"x": 1, "y": 2}]})), none);
    assert_eq!(
        warnings("sketch.extend", json!({"curves": ["l1", "l2"], "curve": "l1"})),
        ["give `curve` with `at` (one curve) or `curves` (two curves), not both"]
    );
    assert_eq!(
        warnings("sketch.extend", json!({"curves": ["l1", "l2"], "at": [0, 1]})),
        ["with `curves`, `at` is one point per curve: [[x,y],[x,y]]"]
    );
    assert_eq!(warnings("sketch.extend", json!({"curve": "l1", "at": [[0, 1], [1, 2]]})), ["`at` must be a point [x, y]"]);
    // Fillet and chamfers: `points` when it is a list; otherwise `point`, or `a` and `b` when
    // `point` is not text.
    assert_eq!(warnings("sketch.fillet", json!({"radius": 2, "point": "l1.end"})), none);
    assert_eq!(warnings("sketch.fillet", json!({"radius": "r / 2", "a": "l1", "b": "l2"})), none);
    assert_eq!(warnings("sketch.fillet", json!({"radius": 2, "point": "l1.end", "a": 5, "b": null})), none);
    assert_eq!(warnings("sketch.fillet", json!({"radius": 2, "point": 7, "a": "l1", "b": "l2"})), none);
    assert_eq!(warnings("sketch.fillet", json!({"radius": 2, "points": ["l1.end", {"a": "l1", "b": "l2"}, {"point": "l3.end", "a": 4}]})), none);
    assert_eq!(warnings("sketch.fillet", json!({"radius": 2, "points": ["l1.end"], "point": 3, "a": 4, "b": 5})), none);
    assert_eq!(warnings("sketch.fillet", json!({"radius": 2, "points": "l1.end", "point": "l2.end"})), none);
    assert_eq!(warnings("sketch.fillet", json!({"radius": 2, "point": null, "a": "l1", "b": "l2"})), none);
    assert_eq!(warnings("sketch.fillet", json!({"radius": 2, "a": "l1"})), ["`b` must be a line"]);
    assert_eq!(warnings("sketch.fillet", json!({"radius": 2, "points": [{"b": "l1"}]})), ["`a` must be a line"]);
    assert_eq!(warnings("sketch.fillet", json!({"radius": 2, "points": [3]})), ["`a` must be a line"]);
    assert_eq!(warnings("sketch.fillet", json!({"radius": 2, "points": []})), ["`points` must list 1…500 corners"]);
    assert_eq!(warnings("sketch.fillet", json!({"radius": [1], "point": "l1.end"})), ["`radius` must be a number or an expression"]);
    assert_eq!(warnings("sketch.fillet", json!({"radius": 1e10, "point": "l1.end"})), ["`radius` must be a number between -1e9 and 1e9"]);
    assert_eq!(warnings("sketch.chamfer.equal_distance", json!({"distance": 2, "points": ["l1.end"], "a": 1})), none);
    assert_eq!(warnings("sketch.chamfer.two_distance", json!({"distance": 2, "distance2": -1, "a": "l1", "b": "l2"})), none);
    assert_eq!(warnings("sketch.chamfer.distance_angle", json!({"distance": 2, "angle": 30, "point": "l1.end"})), none);
    assert_eq!(
        warnings("sketch.chamfer.distance_angle", json!({"distance": 2, "angle": "30", "point": "l1.end"})),
        ["`angle` must be a number (degrees)"]
    );
    // Offset: `symmetric` is read unless `direction` is `symmetric`; `direction` that is not text
    // is not read at all.
    let offset = |extra: Value| {
        let mut p = json!({"curves": ["l1"], "distance": 2});
        for (k, v) in extra.as_object().into_iter().flatten() {
            p[k] = v.clone();
        }
        warnings("sketch.offset", p)
    };
    assert_eq!(offset(json!({"direction": "Symmetric", "symmetric": "yes"})), none);
    assert_eq!(offset(json!({"direction": "one_side", "symmetric": true})), none);
    assert_eq!(offset(json!({"direction": 5, "symmetric": true})), none);
    assert_eq!(offset(json!({"direction": null, "side": [1, 2], "chain": false, "flip": true})), none);
    assert_eq!(offset(json!({"symmetric": "yes"})), ["`symmetric` must be true or false"]);
    assert_eq!(offset(json!({"direction": "positive", "symmetric": 1})), ["`symmetric` must be true or false"]);
    assert_eq!(offset(json!({"distance2": "d / 2", "instances": "n"})), none);
    assert_eq!(offset(json!({"instances": 100, "side": {"x": 1, "y": 2}})), none);
    assert_eq!(offset(json!({"instances": 2.0})), none);
    assert_eq!(offset(json!({"instances": 0})), ["`instances` must be a whole number from 1 to 100, or an expression for one"]);
    // A `side` that is not a point is read as left out, as the handler does.
    assert_eq!(offset(json!({"side": "left"})), none);
    assert_eq!(offset(json!({"flip": 1})), ["`flip` must be true or false"]);
    assert_eq!(offset(json!({"direction": "down"})), ["unknown direction `down` (one_side, symmetric)"]);
    assert_eq!(
        offset(json!({"direction": "FLIP"})),
        ["an offset has no negative direction: give a negative `distance`, or a `side` to offset toward"]
    );
    assert_eq!(offset(json!({"distance2": null})), ["`distance2` must be a length expression"]);
    assert_eq!(warnings("sketch.offset", json!({"curves": "l1", "distance": "2 mm"})), none);
    // Move and scale: a point or a point's id.
    assert_eq!(warnings("sketch.move", json!({"entities": "l1", "translate": {"x": 1, "y": 2}, "angle": 90, "center": "p1", "copy": true})), none);
    assert_eq!(warnings("sketch.move", json!({"entities": ["l1"], "center": [1, 2], "rotate_constraints": false})), none);
    assert_eq!(warnings("sketch.move", json!({"entities": ["l1"], "center": [1]})), ["`center` must be [x, y] or a point id"]);
    assert_eq!(warnings("sketch.move", json!({"entities": ["l1"], "copy": "yes"})), ["`copy` must be true or false"]);
    assert_eq!(warnings("sketch.scale", json!({"entities": ["l1"], "factor": 2, "base": "p1"})), none);
    assert_eq!(warnings("sketch.scale", json!({"entities": ["l1"], "factor": 2, "base": [1, 2]})), none);
    assert_eq!(warnings("sketch.scale", json!({"entities": ["l1"], "factor": 0})), ["`factor` must be a positive number"]);
    assert_eq!(warnings("sketch.scale", json!({"entities": ["l1"], "factor": 2, "base": 7})), ["`base` must be [x, y] or a point id"]);
    // Transform: every key is read only when it is there, because each one has a default.
    assert_eq!(warnings("sketch.transform", json!({"entities": "l1"})), none);
    assert_eq!(warnings("sketch.transform", json!({"entities": ["l1"], "factor": 2, "angle": -90, "center": "p1", "translate": [1, 2]})), none);
    assert_eq!(warnings("sketch.transform", json!({"entities": ["l1"], "center": [1, 2]})), none);
    assert_eq!(warnings("sketch.transform", json!({"entities": ["l1"], "factor": 0})), ["`factor` must be a positive number"]);
    assert_eq!(warnings("sketch.transform", json!({"entities": ["l1"], "factor": 1e6})), ["`factor` must be a positive number"]);
    assert_eq!(warnings("sketch.transform", json!({"entities": ["l1"], "angle": "90"})), ["`angle` must be a number (degrees)"]);
    // The transform reads its angle itself, so at any finite size; the move reads its angle
    // through `params::num`, which caps the size. Each contract says what its handler says.
    assert_eq!(warnings("sketch.transform", json!({"entities": ["l1"], "angle": 1e12})), none);
    assert_eq!(warnings("sketch.move", json!({"entities": ["l1"], "angle": 1e12})), ["`angle` must be finite"]);
    assert_eq!(warnings("sketch.transform", json!({"entities": ["l1"], "translate": [1]})), ["`translate` must be a point [x, y]"]);
    assert_eq!(warnings("sketch.transform", json!({"entities": ["l1"], "center": 7})), ["`center` must be [x, y] or a point id"]);
    assert_eq!(warnings("sketch.transform", json!({"entities": ["l1"], "sketch": false})), ["`sketch` must be an id or a name"]);
    // Mirror: a curve id, an axis, or an origin axis as an object.
    assert_eq!(warnings("sketch.mirror", json!({"entities": ["l1", "p2"], "line": "y"})), none);
    assert_eq!(warnings("sketch.mirror", json!({"entities": ["l1"], "line": {"axis": "Y"}})), none);
    assert_eq!(warnings("sketch.mirror", json!({"entities": ["l1"], "line": 3})), ["`line` must be a curve id, or x / y for a sketch axis"]);
    // Patterns: a pattern being edited needs only what changes.
    assert_eq!(warnings("sketch.pattern.circular", json!({"pattern": "pat1"})), none);
    assert_eq!(warnings("sketch.pattern.circular", json!({"pattern": "pat1", "count": "n + 1", "angle": "90 deg", "reversed": true})), none);
    assert_eq!(
        warnings("sketch.pattern.circular", json!({"entities": ["c1"], "center": "p3", "count": 3, "angle": -90, "alignment": "center"})),
        none
    );
    assert_eq!(warnings("sketch.pattern.circular", json!({"pattern": 4, "count": 3})), none);
    assert_eq!(warnings("sketch.pattern.rectangular", json!({"pattern": "pat1"})), none);
    assert_eq!(warnings("sketch.pattern.rectangular", json!({"pattern": "pat1", "spacing": "w * 2", "count2": "m"})), none);
    assert_eq!(
        warnings(
            "sketch.pattern.rectangular",
            json!({"entities": ["c1"], "count": 3, "spacing": -5, "count2": 2, "spacing2": 4, "dir": [1, 1], "dir2": null, "direction": "l1",
                   "direction2": null, "alignment": "left", "alignment2": "center", "reversed": true, "reversed2": false})
        ),
        none
    );
    assert_eq!(
        warnings("sketch.pattern.rectangular", json!({"entities": ["c1"], "count": 3, "spacing": 5, "dir2": [0, 0]})),
        ["`dir2` must be a finite nonzero vector"]
    );
    assert_eq!(warnings("sketch.pattern.circular", json!({"pattern": "pat1", "angle": 400})), ["`angle` must be nonzero and at most 360 degrees"]);
    assert_eq!(warnings("sketch.pattern.circular", json!({"pattern": "pat1", "center": [1]})), ["`center` must be [x,y] or a point ref"]);
    // A key the command doesn't read is unknown to it, and says what it would have been.
    assert_eq!(
        warnings("sketch.pattern.circular", json!({"entities": ["c1"], "center": [1, 2], "count": 3, "spacing": 4})),
        ["unknown key `spacing`"]
    );
    assert_eq!(warnings("sketch.trim", json!({"curve": "l1", "at": [1, 2], "curv": "l2"})), ["unknown key `curv`; did you mean `curve`?"]);
}

/// The schemas take the calls the handlers take, and say which keys a handler cannot do without.
#[test]
fn the_schemas_take_what_the_handlers_read() {
    let takes = |schema: schemars::Schema, cases: &[(Value, bool)]| {
        let schema = schema.as_value();
        for (p, takes) in cases {
            assert_eq!(conforms(schema, schema, p), *takes, "{p}");
        }
    };
    takes(
        input_schema_for::<TrimParams>(),
        &[
            (json!({"curve": "l1", "at": [1, 2]}), true),
            (json!({"curve": "l1"}), false),
            (json!({"at": [1, 2]}), false),
            (json!({"curve": "l1", "at": 3}), false),
        ],
    );
    takes(
        input_schema_for::<TrimPathParams>(),
        &[
            (json!({"points": [[0, 0], [1, 1]]}), true),
            (json!({"points": [[0, 0], [1, 1]], "dry_run": true}), true),
            (json!({"points": [[0, 0]]}), false),
            (json!({}), false),
        ],
    );
    takes(
        input_schema_for::<BreakParams>(),
        &[
            (json!({"curve": "l1", "at": [1, 2]}), true),
            (json!({"curve": "l1", "points": [[1, 2]], "sketch": 4}), true),
            (json!({"curve": "l1"}), false),
            (json!({"at": [1, 2]}), false),
        ],
    );
    takes(
        input_schema_for::<ExtendParams>(),
        &[
            (json!({"curve": "l1", "at": [1, 2]}), true),
            (json!({"curves": ["l1", "l2"]}), true),
            (json!({"curves": ["l1", "l2"], "at": [[1, 2], [3, 4]]}), true),
            (json!({"curves": ["l1", "l2"], "curve": "l1"}), false),
            (json!({"curves": ["l1"]}), false),
            (json!({"curve": "l1"}), false),
        ],
    );
    takes(
        input_schema_for::<FilletParams>(),
        &[
            (json!({"radius": 2, "point": "l1.end"}), true),
            (json!({"radius": "r", "a": "l1", "b": "l2"}), true),
            (json!({"radius": 2, "points": ["l1.end", {"a": "l1", "b": "l2"}]}), true),
            (json!({"point": "l1.end"}), false),
            (json!({"radius": 2}), false),
            (json!({"radius": 2, "a": "l1"}), false),
        ],
    );
    takes(
        input_schema_for::<ChamferTwoParams>(),
        &[(json!({"distance": 1, "distance2": 2, "point": "l1.end"}), true), (json!({"distance": 1, "point": "l1.end"}), false)],
    );
    takes(
        input_schema_for::<OffsetParams>(),
        &[
            (json!({"curves": ["l1"], "distance": 2}), true),
            (json!({"curves": "l1", "distance": "w / 2", "direction": "symmetric", "instances": 3, "side": [1, 2]}), true),
            (json!({"curves": ["l1"]}), false),
            (json!({"distance": 2}), false),
            (json!({"curves": ["l1"], "distance": 2, "instances": 101}), false),
        ],
    );
    takes(
        input_schema_for::<ScaleParams>(),
        &[
            (json!({"entities": ["l1"], "factor": 2, "base": "p1"}), true),
            (json!({"entities": ["l1"], "factor": 0}), false),
            (json!({"entities": ["l1"]}), false),
        ],
    );
    takes(
        input_schema_for::<TransformParams>(),
        &[
            (json!({"entities": ["l1"]}), true),
            (json!({"entities": "l1", "factor": 2, "angle": 90, "center": "p1", "translate": [1, 2]}), true),
            (json!({"entities": ["l1"], "center": [1, 2]}), true),
            (json!({"entities": ["l1"], "angle": 1e12}), true),
            (json!({"entities": ["l1"], "factor": 0}), false),
            (json!({"entities": ["l1"], "angle": "90"}), false),
            (json!({"entities": ["l1"], "center": [1]}), false),
            (json!({"entities": ["l1"], "translate": [1, 2, 3]}), false),
            (json!({"factor": 2}), false),
        ],
    );
    takes(
        input_schema_for::<CircularPatternParams>(),
        &[
            (json!({"entities": ["c1"], "center": [1, 2], "count": 4}), true),
            (json!({"entities": ["c1"], "center": "p1", "count": "n", "angle": "90 deg", "alignment": "center"}), true),
            (json!({"pattern": "pat1", "count": 5}), true),
            (json!({"entities": ["c1"], "count": 4}), false),
            (json!({"entities": ["c1"], "center": [1, 2], "count": 4, "alignment": "middle"}), false),
        ],
    );
    takes(
        input_schema_for::<RectangularPatternParams>(),
        &[
            (json!({"entities": ["c1"], "count": 3, "spacing": 5, "dir": [1, 0], "dir2": [0, 1], "count2": 2}), true),
            (json!({"pattern": "pat1"}), true),
            (json!({"entities": ["c1"], "count": 3}), false),
        ],
    );
}

fn matches(schema: schemars::Schema, what: &str, reply: &Value) {
    let schema = schema.as_value();
    assert!(conforms(schema, schema, reply), "{what}: {reply} doesn't match {schema}");
    assert!(reply.get("param_warnings").is_none(), "{what}: {reply}");
}

/// A line of the square's sketch that crosses its left and right sides.
fn crossing_line(s: &mut Session) -> String {
    id_of(&run(s, "sketch.line", json!({"points": [[-20, 3], [20, 3]], "infer": false})), "curves")
}

#[test]
fn trim_and_break_replies_match_the_output_schema() {
    // Trim: a piece goes (the curve keeps its id), a curve nothing crosses goes whole, and a
    // spline is replaced by pieces or goes.
    let (mut s, _) = editing(ParamCheck::Warn);
    let line = crossing_line(&mut s);
    let far = id_of(&run(&mut s, "sketch.line", json!({"points": [[40, 40], [50, 40]], "infer": false})), "curves");
    let spline = id_of(&run(&mut s, "sketch.spline.fit_point", json!({"points": [[-5, 0], [0, 20], [5, 0]]})), "curves");
    let lone = id_of(&run(&mut s, "sketch.spline.fit_point", json!({"points": [[60, 60], [70, 65], [80, 60]]})), "curves");
    let trimmed = run(&mut s, "sketch.trim", json!({"curve": line, "at": [15, 3]}));
    assert_eq!(trimmed["result"], json!({"trimmed": line}), "{trimmed}");
    matches(output_schema_for::<TrimResult>(), "sketch.trim", &trimmed);
    let sketch = s.active_sketch;
    let deleted = run(&mut s, "sketch.trim", json!({"curve": far, "at": [45, 40], "sketch": sketch}));
    assert_eq!(deleted["result"], json!({"deleted": far}), "{deleted}");
    matches(output_schema_for::<TrimResult>(), "sketch.trim", &deleted);
    let split = run(&mut s, "sketch.trim", json!({"curve": spline, "at": [0, 20]}));
    assert_eq!(split["result"]["trimmed"], json!(spline), "{split}");
    assert!(split["result"]["pieces"].is_array(), "{split}");
    matches(output_schema_for::<TrimResult>(), "sketch.trim", &split);
    let whole = run(&mut s, "sketch.trim", json!({"curve": lone, "at": [70, 65]}));
    assert_eq!(whole["result"], json!({"trimmed": lone, "pieces": null}), "{whole}");
    matches(output_schema_for::<TrimResult>(), "sketch.trim", &whole);

    // A line that is a single-line offset's source releases its linked extents.
    let (mut s, sides) = editing(ParamCheck::Warn);
    let made = run(&mut s, "sketch.offset", json!({"curves": [sides[0]], "distance": 2}));
    assert!(made["curves"].is_array(), "{made}");
    let cutter = id_of(&run(&mut s, "sketch.line", json!({"points": [[0, -30], [0, 30]], "infer": false})), "curves");
    let released = run(&mut s, "sketch.trim", json!({"curve": sides[0], "at": [5, -10]}));
    matches(output_schema_for::<TrimResult>(), "sketch.trim", &released);
    let _ = cutter;

    // Trim along a path, and its dry run.
    let (mut s, _) = editing(ParamCheck::Warn);
    let path = json!({"points": [[-20, 3], [20, 3]]});
    let dry = run(&mut s, "sketch.trim.path", json!({"points": path["points"], "dry_run": true}));
    assert_eq!(dry["dry_run"], json!(true), "{dry}");
    let schema = output_schema_for::<TrimPathResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &dry), "{dry}");
    assert!(dry.get("param_warnings").is_none(), "{dry}");
    let done = run(&mut s, "sketch.trim.path", path);
    assert!(done.get("dry_run").is_none() && done["trimmed"].as_array().is_some_and(|t| !t.is_empty()), "{done}");
    matches(output_schema_for::<TrimPathResult>(), "sketch.trim.path", &done);

    // Break, at the crossings and at given positions.
    let (mut s, _) = editing(ParamCheck::Warn);
    let line = crossing_line(&mut s);
    let broken = run(&mut s, "sketch.break", json!({"curve": line, "at": [0, 3]}));
    assert!(broken["curves"].as_array().is_some_and(|c| c.len() == 3), "{broken}");
    matches(output_schema_for::<BreakResult>(), "sketch.break", &broken);
    let (mut s, _) = editing(ParamCheck::Warn);
    let line = id_of(&run(&mut s, "sketch.line", json!({"points": [[-5, 30], [5, 30]], "infer": false})), "curves");
    let broken = run(&mut s, "sketch.break", json!({"curve": line, "points": [[-2, 30], [2, 31], [2, 30]]}));
    assert!(broken["curves"].as_array().is_some_and(|c| c.len() == 3), "{broken}");
    matches(output_schema_for::<BreakResult>(), "sketch.break", &broken);
}

#[test]
fn extend_replies_match_the_output_schema() {
    let (mut s, _) = editing(ParamCheck::Warn);
    let short = id_of(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [5, 0]], "infer": false})), "curves");
    let extended = run(&mut s, "sketch.extend", json!({"curve": short, "at": [5, 0]}));
    matches(output_schema_for::<ExtendResult>(), "sketch.extend", &extended);
    let a = id_of(&run(&mut s, "sketch.line", json!({"points": [[30, 0], [30, 5]], "infer": false})), "curves");
    let b = id_of(&run(&mut s, "sketch.line", json!({"points": [[40, 20], [45, 20]], "infer": false})), "curves");
    let met = run(&mut s, "sketch.extend", json!({"curves": [a, b]}));
    matches(output_schema_for::<ExtendResult>(), "sketch.extend", &met);
    let c = id_of(&run(&mut s, "sketch.line", json!({"points": [[60, 0], [60, 5]], "infer": false})), "curves");
    let d = id_of(&run(&mut s, "sketch.line", json!({"points": [[70, 20], [75, 20]], "infer": false})), "curves");
    let met = run(&mut s, "sketch.extend", json!({"curves": [c, d], "at": [[60, 5], [70, 20]]}));
    matches(output_schema_for::<ExtendResult>(), "sketch.extend", &met);
}

#[test]
fn fillet_and_chamfer_replies_match_the_output_schema() {
    let fillet = output_schema_for::<FilletResult>();
    let chamfer = output_schema_for::<ChamferResult>();
    for p in [
        json!({"point": "l2.end", "radius": 2}),
        json!({"a": "l1", "b": "l2", "radius": "1 + 1"}),
        json!({"points": ["l1.end", {"a": "l2", "b": "l3"}, {"point": "l3.end"}], "radius": 2}),
        json!({"points": ["l1.end"], "point": 3, "a": 1, "radius": 2}),
    ] {
        let (mut s, _) = editing(ParamCheck::Warn);
        let r = run(&mut s, "sketch.fillet", p.clone());
        assert!(r["param"].is_string() && r["curves"].as_array().is_some_and(|c| !c.is_empty()), "{r}");
        matches(fillet.clone(), &format!("sketch.fillet {p}"), &r);
    }
    for (id, p) in [
        ("sketch.chamfer.equal_distance", json!({"point": "l2.end", "distance": 2})),
        ("sketch.chamfer.equal_distance", json!({"points": ["l1.end", {"a": "l2", "b": "l3"}], "distance": 2})),
        ("sketch.chamfer.two_distance", json!({"a": "l1", "b": "l2", "distance": 2, "distance2": 3})),
        ("sketch.chamfer.distance_angle", json!({"point": "l2.end", "distance": 2, "angle": 40})),
    ] {
        let (mut s, _) = editing(ParamCheck::Warn);
        let r = run(&mut s, id, p.clone());
        assert!(r["curves"].as_array().is_some_and(|c| !c.is_empty()), "{r}");
        matches(chamfer.clone(), &format!("{id} {p}"), &r);
    }
}

#[test]
fn offset_replies_match_the_output_schema() {
    let schema = output_schema_for::<OffsetResult>();
    for p in [
        json!({"curves": ["l1", "l2", "l3", "l4"], "distance": 2}),
        json!({"curves": ["l1", "l2", "l3", "l4"], "distance": "2 mm", "flip": true, "chain": false}),
        json!({"curves": ["l1"], "distance": 2, "side": [0, 30]}),
        json!({"curves": "l1", "distance": 2, "direction": "symmetric"}),
        json!({"curves": ["l1"], "distance": 2, "symmetric": true}),
        json!({"curves": ["l1"], "distance": 2, "distance2": 3, "side": [0, 30]}),
        json!({"curves": ["l1"], "distance": 2, "instances": 3, "side": [0, 30]}),
        json!({"curves": ["l1"], "distance": 2, "direction": "one_side", "instances": "1 + 1", "side": [0, 30]}),
    ] {
        let (mut s, _) = editing(ParamCheck::Warn);
        let r = run(&mut s, "sketch.offset", p.clone());
        assert!(r["sides"].as_array().is_some_and(|c| !c.is_empty()), "{r}");
        matches(schema.clone(), &format!("sketch.offset {p}"), &r);
    }
    // Break the association of what an offset made, a single curve's and a closed boundary's.
    for sources in [json!(["l1"]), json!(["l1", "l2", "l3", "l4"])] {
        let (mut s, _) = editing(ParamCheck::Warn);
        let made = run(&mut s, "sketch.offset", json!({"curves": sources, "distance": 2, "side": [0, 30]}));
        let output = made["curves"][0].as_str().unwrap_or_default().to_string();
        let r = run(&mut s, "sketch.offset.break", json!({"curve": output}));
        assert_eq!(r["curve"], json!(output), "{r}");
        matches(output_schema_for::<OffsetBreakResult>(), "sketch.offset.break", &r);
    }
}

#[test]
fn mirror_move_scale_and_pattern_replies_match_the_output_schema() {
    let mirror = output_schema_for::<MirrorResult>();
    for line in [json!("l3"), json!("y"), json!({"axis": "Y"})] {
        let (mut s, _) = editing(ParamCheck::Warn);
        let r = run(&mut s, "sketch.mirror", json!({"entities": ["l1"], "line": line}));
        assert!(r["curves"].as_array().is_some_and(|c| !c.is_empty()), "{r}");
        matches(mirror.clone(), &format!("sketch.mirror {line}"), &r);
    }
    let moved = output_schema_for::<MoveResult>();
    let scaled = output_schema_for::<ScaleResult>();
    let (mut s, _) = editing(ParamCheck::Warn);
    let c = id_of(&run(&mut s, "sketch.circle.center", json!({"center": [30, 0], "radius": 3})), "curves");
    let r = run(&mut s, "sketch.move", json!({"entities": [c], "translate": [1, 2], "angle": 15, "center": [0, 0]}));
    matches(moved.clone(), "sketch.move", &r);
    let r = run(&mut s, "sketch.move", json!({"entities": c, "translate": {"x": 5, "y": 0}, "copy": true, "rotate_constraints": false}));
    assert_eq!(r["curves"].as_array().map(Vec::len), Some(1), "{r}");
    matches(moved, "sketch.move", &r);
    let r = run(&mut s, "sketch.scale", json!({"entities": [c], "factor": 2, "base": [0, 0]}));
    matches(scaled.clone(), "sketch.scale", &r);
    let r = run(&mut s, "sketch.scale", json!({"entities": [c], "factor": 0.5}));
    matches(scaled, "sketch.scale", &r);

    let pattern = output_schema_for::<PatternResult>();
    let (mut s, _) = editing(ParamCheck::Warn);
    let c = id_of(&run(&mut s, "sketch.circle.center", json!({"center": [30, 0], "radius": 2})), "curves");
    let made = run(&mut s, "sketch.pattern.circular", json!({"entities": [c], "center": [0, 0], "count": 4}));
    assert!(made["center"].is_string(), "{made}");
    matches(pattern.clone(), "sketch.pattern.circular", &made);
    let edited = run(&mut s, "sketch.pattern.circular", json!({"pattern": made["pattern"], "count": 5, "angle": 180, "alignment": "center"}));
    assert_eq!(edited["pattern"], made["pattern"], "{edited}");
    matches(pattern.clone(), "sketch.pattern.circular", &edited);
    let (mut s, _) = editing(ParamCheck::Warn);
    let c = id_of(&run(&mut s, "sketch.circle.center", json!({"center": [30, 30], "radius": 2})), "curves");
    let made =
        run(&mut s, "sketch.pattern.rectangular", json!({"entities": [c], "count": 3, "spacing": 8, "count2": 2, "spacing2": "6 mm", "dir": [1, 0]}));
    assert!(made["center"].is_null(), "{made}");
    matches(pattern.clone(), "sketch.pattern.rectangular", &made);
    let edited =
        run(&mut s, "sketch.pattern.rectangular", json!({"pattern": made["pattern"], "spacing": 9, "alignment": "center", "reversed2": true}));
    matches(pattern, "sketch.pattern.rectangular", &edited);
}

/// `sketch.transform` answers with the object its output schema describes: the curves it moved,
/// with an ellipse's axes, and the sketch's solve.
#[test]
fn a_transform_answers_with_the_curves_it_moved_and_the_solve() {
    let schema = output_schema_for::<TransformResult>();
    let (mut s, _) = editing(ParamCheck::Warn);
    let line = id_of(&run(&mut s, "sketch.line", json!({"points": [[-5, 20], [5, 20]], "infer": false})), "curves");
    let r = run(&mut s, "sketch.transform", json!({"entities": [line], "factor": 2, "angle": 90, "center": [0, 0], "translate": [1, 1]}));
    assert_eq!(r["curves"], json!([line]), "{r}");
    assert!(r["sketch"]["solved"].is_boolean() && r["sketch"]["dof"].is_number(), "{r}");
    matches(schema.clone(), "sketch.transform", &r);
    // An ellipse takes its axis lines along, and the reply lists all three.
    let ellipse = id_of(&run(&mut s, "sketch.ellipse", json!({"center": [0, 30], "major": [10, 30], "minor": [0, 36]})), "curves");
    let r = run(&mut s, "sketch.transform", json!({"entities": [ellipse], "factor": 2}));
    let moved = r["curves"].as_array().unwrap_or_else(|| panic!("{r}"));
    assert!(moved.contains(&json!(ellipse)) && moved.len() == 3, "{r}");
    matches(schema, "sketch.transform", &r);
}

/// A misspelt key still runs in Warn, and says so, with a reply that matches.
#[test]
fn a_misspelt_key_is_reported_in_warn() {
    let line = |s: &mut Session| id_of(&run(s, "sketch.line", json!({"points": [[0, 0], [5, 0]], "infer": false})), "curves");
    let calls: Vec<(&str, fn(&mut Session) -> Value)> = vec![
        ("sketch.trim", |s| json!({"curve": crossing_line(s), "at": [15, 3], "atx": [1, 2]})),
        ("sketch.trim.path", |_| json!({"points": [[-20, 3], [20, 3]], "point": [1, 1]})),
        ("sketch.break", |s| json!({"curve": crossing_line(s), "at": [0, 3], "curv": "l1"})),
        (
            "sketch.extend",
            |s| json!({"curve": id_of(&run(s, "sketch.line", json!({"points": [[0, 0], [5, 0]], "infer": false})), "curves"), "at": [5, 0], "att": 1}),
        ),
        ("sketch.fillet", |_| json!({"point": "l2.end", "radius": 2, "radious": 3})),
        ("sketch.chamfer.equal_distance", |_| json!({"point": "l2.end", "distance": 2, "distence": 3})),
        ("sketch.chamfer.two_distance", |_| json!({"point": "l2.end", "distance": 2, "distance2": 1, "distence": 3})),
        ("sketch.chamfer.distance_angle", |_| json!({"point": "l2.end", "distance": 2, "angle": 40, "angel": 3})),
        ("sketch.offset", |_| json!({"curves": ["l1"], "distance": 2, "side": [0, 30], "distence": 3})),
        ("sketch.mirror", |_| json!({"entities": ["l1"], "line": "l3", "lines": 1})),
        ("sketch.move", |_| json!({"entities": ["l1"], "translate": [1, 2], "translat": 1})),
        ("sketch.scale", |_| json!({"entities": ["l1"], "factor": 2, "facter": 1})),
        ("sketch.transform", |_| json!({"entities": ["l1"], "factor": 2, "facter": 1})),
    ];
    for (id, call) in calls {
        let (mut s, _) = editing(ParamCheck::Warn);
        let p = call(&mut s);
        let r = run(&mut s, id, p.clone());
        let warned = r["param_warnings"].as_array().unwrap_or_else(|| panic!("{id} {p}: {r}"));
        assert_eq!(warned.len(), 1, "{id}: {r}");
        assert!(warned[0].as_str().is_some_and(|w| w.starts_with("unknown key `")), "{id}: {r}");
    }
    // The ones that need a shape of their own.
    let (mut s, _) = editing(ParamCheck::Warn);
    let a = line(&mut s);
    let c = id_of(&run(&mut s, "sketch.circle.center", json!({"center": [30, 0], "radius": 2})), "curves");
    for (id, p) in [
        ("sketch.offset.break", None),
        ("sketch.pattern.circular", Some(json!({"entities": [c], "center": [0, 0], "count": 3, "cnt": 1}))),
        ("sketch.pattern.rectangular", Some(json!({"entities": [c], "count": 3, "spacing": 8, "spaceing": 1}))),
    ] {
        let Some(p) = p else { continue };
        let r = run(&mut s, id, p.clone());
        let warned = r["param_warnings"].as_array().unwrap_or_else(|| panic!("{id} {p}: {r}"));
        assert_eq!(warned.len(), 1, "{id}: {r}");
    }
    let made = run(&mut s, "sketch.offset", json!({"curves": [a], "distance": 1, "side": [0, 30]}));
    let output = made["curves"][0].as_str().unwrap_or_default().to_string();
    let r = run(&mut s, "sketch.offset.break", json!({"curve": output, "sketches": 2}));
    assert!(r["param_warnings"].as_array().is_some_and(|w| w.len() == 1), "{r}");
    let schema = output_schema_for::<OffsetBreakResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &r), "{r}");
}

/// The reply types serialize to the JSON the handlers built before: a sketch command's reply
/// also carries the `created` the session adds, and an absent `param2` or `association` is left
/// out.
#[test]
fn replies_keep_their_shape() {
    let keys = |v: &Value| v.as_object().map(|o| o.keys().cloned().collect::<std::collections::BTreeSet<_>>()).unwrap_or_default();
    let set = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<std::collections::BTreeSet<_>>();
    let (mut s, _) = editing(ParamCheck::Off);
    let r = run(&mut s, "sketch.offset", json!({"curves": ["l1", "l2", "l3", "l4"], "distance": 2}));
    assert_eq!(keys(&r), set(&["constraints", "created", "curves", "instances", "param", "sides", "sketch"]), "{r}");
    assert!(r["sketch"]["solved"].is_boolean() && r["sketch"]["dof"].is_number(), "{r}");
    assert_eq!(keys(&r["sides"][0]), set(&["curves", "distance", "instance", "param"]), "{r}");
    let (mut s, _) = editing(ParamCheck::Off);
    let r = run(&mut s, "sketch.offset", json!({"curves": ["l1"], "distance": 2, "distance2": 1, "side": [0, 30]}));
    assert!(r.get("association").is_none() && r["param2"].is_string(), "{r}");
    let r = run(&mut s, "sketch.fillet", json!({"point": "l2.end", "radius": 2}));
    assert_eq!(keys(&r), set(&["constraints", "created", "curves", "moved_to_virtual_corner", "param", "sketch", "virtual_corners"]), "{r}");
    let side = OffsetSide { distance: 1.5, curves: vec!["l1".into()], param: None, instance: 1, association: Some("o1".into()) };
    assert_eq!(
        serde_json::to_value(&side).unwrap_or_default(),
        json!({"distance": 1.5, "curves": ["l1"], "param": null, "instance": 1, "association": "o1"})
    );
    let trimmed = TrimOutcome::Trimmed { trimmed: "l1".into(), offset_released: Vec::new() }.released(vec!["l9".into()]);
    assert_eq!(serde_json::to_value(&trimmed).unwrap_or_default(), json!({"trimmed": "l1", "offset_released": ["l9"]}));
}
