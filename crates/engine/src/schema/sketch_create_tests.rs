use serde_json::{Value, json};

use super::{
    ArcSlotCenterParams, ArcSlotResult, ArcSlotThreePointParams, ArcTangentParams, BlendCurveParams, CenterlineParams, ConicParams,
    ControlPointSplineParams, DrawResult, EditTextParams, EditTextResult, EllipseParams, FitPointSplineParams, InfiniteLineParams, LineTypeResult,
    MidpointLineParams, SketchTextParams, SlotCenterPointParams, SplineRemovePointParams, SplineRemovePointResult, SplineReverseParams,
    SplineReverseResult, TextResult, ThreeTangentCircleParams, TwoTangentCircleParams,
};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<SplineReverseParams>(), &["curve", "sketch"]);
    assert_keys(input_schema_for::<SplineRemovePointParams>(), &["curve", "point", "sketch"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("sketch.spline.reverse", json!({"curve": 5})),
        ("sketch.spline.reverse", json!({"curve": null})),
        ("sketch.spline.reverse", json!({"curve": "nope"})),
        ("sketch.spline.reverse", json!({"curve": "l1"})),
        ("sketch.spline.remove_point", json!({"curve": 5, "point": "p1"})),
        ("sketch.spline.remove_point", json!({"curve": "nope", "point": "p1"})),
        ("sketch.spline.remove_point", json!({"curve": "l1", "point": 3})),
        ("sketch.spline.remove_point", json!({"curve": "l1"})),
    ]);
}

#[test]
fn spline_remove_point_reply_matches_its_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let run = |s: &mut Session, id: &str, p: serde_json::Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let c = run(&mut s, "sketch.spline.fit_point", json!({"points": [[5, 0], [10, 5], [20, 0], [30, 5], [40, 0]]}));
    let spline = c["curves"][0].as_str().unwrap_or_else(|| panic!("{c}")).to_string();
    let schema = output_schema_for::<SplineRemovePointResult>();
    let schema = schema.as_value();
    let r = run(&mut s, "sketch.spline.remove_point", json!({"curve": spline, "point": format!("{spline}.start")}));
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    assert!(r.get("param_warnings").is_none(), "{r}");
    let r = run(&mut s, "sketch.spline.remove_point", json!({"curve": spline, "point": format!("{spline}.end"), "unknown": 1}));
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    assert!(r["param_warnings"].as_array().is_some_and(|w| !w.is_empty()), "{r}");
}

#[test]
fn spline_reverse_reply_matches_its_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let run = |s: &mut Session, id: &str, p: serde_json::Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let c = run(&mut s, "sketch.spline.fit_point", json!({"points": [[0, 0], [10, 5], [20, 0]]}));
    let spline = c["curves"][0].as_str().unwrap_or_else(|| panic!("{c}")).to_string();
    let schema = output_schema_for::<SplineReverseResult>();
    let schema = schema.as_value();
    let r = run(&mut s, "sketch.spline.reverse", json!({"curve": spline}));
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    assert!(r.get("param_warnings").is_none(), "{r}");
    let r = run(&mut s, "sketch.spline.reverse", json!({"curve": spline, "unknown": 1}));
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    assert!(r["param_warnings"].as_array().is_some_and(|w| !w.is_empty()), "{r}");
}

#[test]
fn construction_line_contracts_and_replies_match() {
    use super::{ConstructionLineParams, ConstructionLineResult};
    assert_keys(
        input_schema_for::<ConstructionLineParams>(),
        &["alternate", "angle", "at", "construction", "curve", "curve2", "length", "mode", "near", "offset", "sketch", "solution", "through"],
    );
    let schema = output_schema_for::<ConstructionLineResult>();
    let schema = schema.as_value();
    let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
    s.execute("sketch.create", &json!({"plane":"XY"})).unwrap();
    for (id, p) in [
        ("sketch.construction_line", json!({"at":[0,0],"through":[5,5]})),
        ("sketch.construction_line.horizontal", json!({"at":[0,10]})),
        ("sketch.construction_line.angle", json!({"at":[10,10],"angle":30})),
        ("sketch.construction_line.vertical", json!({"at":[20,0],"length":"12 mm","near":[20,-5]})),
    ] {
        let r = s.execute(id, &p).unwrap();
        assert!(conforms(schema, schema, &r), "{r}");
    }
    assert!(s.execute("sketch.construction_line", &json!({"at":[0,0],"mode":"unknown"})).is_err());
}

// ---------------------------------------------------------------------------------------------
// The creation commands (#12550)

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A session at `level` editing a sketch with two lines meeting at (40, 0), a circle and a text.
fn drawing(level: ParamCheck) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0]]}));
    run(&mut s, "sketch.line", json!({"points": [[40, 0], [40, 30]]}));
    run(&mut s, "sketch.circle.center", json!({"center": [10, 10], "radius": 4}));
    run(&mut s, "sketch.text", json!({"text": "Hi", "at": [0, 40]}));
    s
}

/// The link of the text `drawing` makes.
fn text_link() -> String {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0]]}));
    run(&mut s, "sketch.line", json!({"points": [[40, 0], [40, 30]]}));
    run(&mut s, "sketch.circle.center", json!({"center": [10, 10], "radius": 4}));
    run(&mut s, "sketch.text", json!({"text": "Hi", "at": [0, 40]}))["link"].as_str().unwrap_or_default().to_string()
}

/// Like `assert_strict_keeps_todays_errors`, in a sketch being edited: the calls whose refusal
/// depends on the design (a point or curve that isn't there), or that the handler reaches only
/// after it has found its sketch.
fn assert_strict_keeps_todays_errors_in_a_sketch(cases: &[(&str, Value)]) {
    for (id, p) in cases {
        let error = |level: ParamCheck| drawing(level).execute(id, p).err().map(|e| e.to_string());
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{id} {p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{id} {p}");
    }
}

fn matches(schema: schemars::Schema, r: &Value) {
    let schema = schema.as_value();
    assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
}

fn issues(id: &str, p: Value) -> Vec<String> {
    crate::schema::issues(id, &p).into_iter().map(|i| i.message).collect()
}

#[test]
fn creation_schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<MidpointLineParams>(), &["construction", "end", "mid", "sketch"]);
    assert_keys(input_schema_for::<ArcTangentParams>(), &["curve", "end", "sketch", "start"]);
    assert_keys(input_schema_for::<TwoTangentCircleParams>(), &["curves", "diameter", "near", "radius", "sketch"]);
    assert_keys(input_schema_for::<ThreeTangentCircleParams>(), &["curves", "near", "sketch"]);
    assert_keys(input_schema_for::<SlotCenterPointParams>(), &["center", "connect", "end", "sketch", "width"]);
    assert_keys(input_schema_for::<ArcSlotThreePointParams>(), &["end", "sketch", "start", "through", "width"]);
    assert_keys(input_schema_for::<ArcSlotCenterParams>(), &["center", "end", "sketch", "start", "sweep", "width"]);
    assert_keys(input_schema_for::<EllipseParams>(), &["axes", "center", "construction", "major", "minor", "minor_radius", "sketch"]);
    assert_keys(input_schema_for::<FitPointSplineParams>(), &["construction", "periodic", "points", "sketch"]);
    assert_keys(input_schema_for::<ControlPointSplineParams>(), &["construction", "periodic", "points", "sketch"]);
    assert_keys(input_schema_for::<ConicParams>(), &["apex", "construction", "end", "rho", "sketch", "start"]);
    assert_keys(input_schema_for::<BlendCurveParams>(), &["a", "b", "continuity", "sketch"]);
    assert_keys(input_schema_for::<CenterlineParams>(), &["curves", "sketch", "value"]);
    assert_keys(input_schema_for::<InfiniteLineParams>(), &["curves", "sketch", "value"]);
    // The texts go into the sketch being edited; `sketch` only picks what `created` compares.
    assert_keys(
        input_schema_for::<SketchTextParams>(),
        &[
            "align",
            "angle",
            "at",
            "bold",
            "font",
            "height",
            "italic",
            "letter_spacing",
            "line_spacing",
            "path",
            "reverse",
            "sketch",
            "text",
            "text_expression",
        ],
    );
    assert_keys(
        input_schema_for::<EditTextParams>(),
        &[
            "align",
            "angle",
            "at",
            "bold",
            "edits",
            "entity",
            "font",
            "height",
            "italic",
            "letter_spacing",
            "line_spacing",
            "link",
            "live",
            "path",
            "reverse",
            "sketch",
            "text",
            "text_expression",
        ],
    );
    // Every command has a contract and an output schema, and the edit's items too.
    for id in [
        "sketch.arc.tangent",
        "sketch.blend_curve",
        "sketch.centerline",
        "sketch.circle.three_tangent",
        "sketch.circle.two_tangent",
        "sketch.conic",
        "sketch.edit_text",
        "sketch.ellipse",
        "sketch.infinite",
        "sketch.line.midpoint",
        "sketch.slot.arc_center",
        "sketch.slot.arc_three_point",
        "sketch.slot.center_point",
        "sketch.spline.control_point",
        "sketch.spline.control_point_5",
        "sketch.spline.fit_point",
        "sketch.text",
    ] {
        let c = crate::cmd::find_command(id).unwrap_or_else(|| panic!("{id}"));
        assert!(c.contract.is_some() && c.output.is_some(), "{id}");
    }
    assert_eq!(solidcraft_sketch::MAX_SPLINE_POINTS, 500, "the handler's message and the type's say 500");
}

/// The calls the handlers refuse before they look at a sketch: strict gives their messages.
#[test]
fn strict_keeps_todays_errors_before_the_sketch() {
    let long = "x".repeat(solidcraft_sketch::MAX_TEXT_CHARS + 1);
    let at = json!([0, 0]);
    assert_strict_keeps_todays_errors(&[
        ("sketch.line.midpoint", json!({})),
        ("sketch.line.midpoint", json!({"mid": [1, 1]})),
        ("sketch.line.midpoint", json!({"mid": [1, 1], "end": 5})),
        ("sketch.line.midpoint", json!({"mid": [1, 1], "end": null})),
        ("sketch.line.midpoint", json!({"mid": [1, 1], "end": [1]})),
        ("sketch.arc.tangent", json!({"start": "l1.end"})),
        ("sketch.arc.tangent", json!({"start": "l1.end", "end": true})),
        ("sketch.arc.tangent", json!({"start": "l1.end", "end": null})),
        ("sketch.circle.two_tangent", json!({})),
        ("sketch.circle.two_tangent", json!({"radius": true})),
        ("sketch.circle.two_tangent", json!({"radius": "  "})),
        ("sketch.circle.two_tangent", json!({"diameter": "5"})),
        ("sketch.circle.two_tangent", json!({"radius": 5})),
        ("sketch.circle.two_tangent", json!({"radius": 5, "near": "x"})),
        ("sketch.circle.two_tangent", json!({"diameter": 5, "near": null})),
        ("sketch.circle.three_tangent", json!({})),
        ("sketch.circle.three_tangent", json!({"near": [1]})),
        ("sketch.slot.center_point", json!({})),
        ("sketch.slot.center_point", json!({"center": [0, 0]})),
        ("sketch.slot.center_point", json!({"center": [0, 0], "end": [5, 0]})),
        ("sketch.slot.center_point", json!({"center": [0, 0], "end": [5, 0], "width": 0})),
        ("sketch.slot.center_point", json!({"center": [0, 0], "end": [5, 0], "width": -2})),
        ("sketch.slot.center_point", json!({"center": [0, 0], "end": [5, 0], "width": "2"})),
        ("sketch.slot.center_point", json!({"center": [0, 0], "end": [5, 0], "width": null})),
        ("sketch.slot.center_point", json!({"center": [0, 0], "end": [5], "width": 2})),
        ("sketch.slot.arc_three_point", json!({})),
        ("sketch.slot.arc_three_point", json!({"start": [0, 0], "through": [1, 1]})),
        ("sketch.slot.arc_three_point", json!({"start": [0, 0], "through": [1, 1], "end": [2, 0]})),
        ("sketch.slot.arc_three_point", json!({"start": [0, 0], "through": [1, 1], "end": [2, 0], "width": 0})),
        ("sketch.slot.arc_center", json!({})),
        ("sketch.slot.arc_center", json!({"center": [0, 0], "start": [5, 0]})),
        ("sketch.slot.arc_center", json!({"center": [0, 0], "start": [5, 0], "width": 1})),
        ("sketch.slot.arc_center", json!({"center": [0, 0], "start": [5, 0], "width": 1, "end": true})),
        ("sketch.slot.arc_center", json!({"center": [0, 0], "start": [5, 0], "width": 1, "sweep": "x"})),
        ("sketch.slot.arc_center", json!({"center": [0, 0], "start": [5, 0], "width": 1, "sweep": null})),
        ("sketch.slot.arc_center", json!({"center": [0, 0], "start": [5, 0], "width": 1, "sweep": 1e9})),
        ("sketch.ellipse", json!({})),
        ("sketch.ellipse", json!({"center": [0, 0]})),
        ("sketch.ellipse", json!({"center": [0, 0], "major": [10, 0]})),
        ("sketch.ellipse", json!({"center": [0, 0], "major": [10, 0], "minor": true})),
        ("sketch.ellipse", json!({"center": [0, 0], "major": [10, 0], "minor": [1]})),
        ("sketch.ellipse", json!({"center": [0, 0], "major": [10, 0], "minor_radius": 0})),
        ("sketch.ellipse", json!({"center": [0, 0], "major": [10, 0], "minor_radius": -3})),
        ("sketch.ellipse", json!({"center": [0, 0], "major": [10, 0], "minor_radius": 1e8})),
        ("sketch.ellipse", json!({"center": [0, 0], "major": [10, 0], "minor_radius": "3"})),
        ("sketch.ellipse", json!({"center": [0, 0], "major": [10, 0], "minor_radius": 3, "axes": "yes"})),
        ("sketch.ellipse", json!({"center": [0, 0], "major": [10, 0], "minor": [1, 1], "axes": 0})),
        ("sketch.conic", json!({"rho": 0})),
        ("sketch.conic", json!({"rho": 1})),
        ("sketch.conic", json!({"rho": -2})),
        ("sketch.conic", json!({"rho": 1e-7})),
        ("sketch.spline.fit_point", json!({"periodic": "yes"})),
        ("sketch.spline.fit_point", json!({"periodic": null})),
        ("sketch.spline.fit_point", json!({"periodic": 1, "points": [[0, 0], [1, 1]]})),
        ("sketch.spline.control_point", json!({"periodic": true, "points": [[0, 0], [1, 1]]})),
        ("sketch.spline.control_point", json!({"periodic": null})),
        ("sketch.spline.control_point_5", json!({"periodic": true})),
        ("sketch.blend_curve", json!({"continuity": "g9"})),
        ("sketch.blend_curve", json!({"continuity": "g0"})),
        ("sketch.blend_curve", json!({"continuity": 5})),
        ("sketch.blend_curve", json!({"continuity": true})),
        ("sketch.centerline", json!({})),
        ("sketch.centerline", json!({"curves": []})),
        ("sketch.centerline", json!({"curves": null})),
        ("sketch.centerline", json!({"curves": 5})),
        ("sketch.centerline", json!({"curves": [1]})),
        ("sketch.centerline", json!({"curves": ["l1", true]})),
        ("sketch.infinite", json!({})),
        ("sketch.infinite", json!({"curves": []})),
        ("sketch.infinite", json!({"curves": {"a": 1}})),
        ("sketch.text", json!({})),
        ("sketch.text", json!({"at": null})),
        ("sketch.text", json!({"at": [1], "text": "x"})),
        ("sketch.text", json!({"at": [1, 2, 3], "text": "x"})),
        ("sketch.text", json!({"at": "x", "text": "x"})),
        ("sketch.text", json!({"at": [1, true], "text": "x"})),
        ("sketch.text", json!({"at": ["", 1], "text": "x"})),
        ("sketch.text", json!({"at": {"x": "w", "y": 1}, "text": "x"})),
        ("sketch.text", json!({"at": ["no_such_parameter", 1], "text": "x"})),
        ("sketch.text", json!({"at": at})),
        ("sketch.text", json!({"at": at, "text": ""})),
        ("sketch.text", json!({"at": at, "text": "  \n "})),
        ("sketch.text", json!({"at": at, "text": 5})),
        ("sketch.text", json!({"at": at, "text": long})),
        ("sketch.text", json!({"at": at, "text_expression": 5})),
        ("sketch.text", json!({"at": at, "text": "x", "text_expression": "'y'"})),
        ("sketch.text", json!({"at": at, "text": 5, "text_expression": 6})),
        ("sketch.text", json!({"at": at, "text": "x", "height": 0})),
        ("sketch.text", json!({"at": at, "text": "x", "height": 1e5})),
        ("sketch.text", json!({"at": at, "text": "x", "height": true})),
        ("sketch.text", json!({"at": at, "text": "x", "height": " "})),
        ("sketch.text", json!({"at": at, "text": "x", "height": "no_such_parameter"})),
        ("sketch.text", json!({"at": at, "text": "x", "angle": []})),
        ("sketch.text", json!({"at": at, "text": "x", "angle": 1e9})),
        ("sketch.text", json!({"at": at, "text": "x", "font": 5})),
        ("sketch.text", json!({"at": at, "text": "x", "font": "No Such Font Family 12345"})),
        ("sketch.text", json!({"at": at, "text": "x", "bold": "yes"})),
        ("sketch.text", json!({"at": at, "text": "x", "italic": 1})),
        ("sketch.text", json!({"at": at, "text": "x", "reverse": []})),
        ("sketch.text", json!({"at": at, "text": "x", "align": "up"})),
        ("sketch.text", json!({"at": at, "text": "x", "align": 5})),
        ("sketch.text", json!({"at": at, "text": "x", "letter_spacing": "wide"})),
        ("sketch.text", json!({"at": at, "text": "x", "line_spacing": 0})),
        ("sketch.text", json!({"at": at, "text": "x", "line_spacing": "2"})),
        ("sketch.text", json!({"at": at, "text": "x", "path": 5})),
    ]);
}

/// The calls that need the design to be refused, or that the handlers refuse once they have
/// found their sketch: strict gives their messages.
#[test]
fn strict_keeps_todays_errors_in_a_sketch() {
    let at = json!([0, 0]);
    // The text `drawing` made.
    let t = text_link();
    assert_strict_keeps_todays_errors_in_a_sketch(&[
        ("sketch.line.midpoint", json!({"end": [1, 1]})),
        ("sketch.line.midpoint", json!({"end": [1, 1], "mid": null})),
        ("sketch.line.midpoint", json!({"end": [1, 1], "mid": 5})),
        ("sketch.line.midpoint", json!({"end": [1, 1], "mid": [1]})),
        ("sketch.line.midpoint", json!({"end": [1, 1], "mid": [1, true]})),
        ("sketch.line.midpoint", json!({"end": [1, 1], "mid": ["", 1]})),
        ("sketch.line.midpoint", json!({"end": [1, 1], "mid": ["no_such_parameter", 1]})),
        ("sketch.line.midpoint", json!({"end": [1, 1], "mid": {"ref": 5}})),
        ("sketch.line.midpoint", json!({"end": [1, 1], "mid": {"vertex": [1, 2]}})),
        ("sketch.line.midpoint", json!({"end": [1, 1], "mid": "no_such_point"})),
        ("sketch.line.midpoint", json!({"end": [1, 1], "mid": [1, 1]})),
        ("sketch.line.midpoint", json!({"end": [1, 1], "mid": [3, 3], "sketch": true})),
        ("sketch.line.midpoint", json!({"end": [1, 1], "mid": [3, 3], "sketch": -1})),
        ("sketch.line.midpoint", json!({"end": [1, 1], "mid": [3, 3], "sketch": 4096})),
        ("sketch.arc.tangent", json!({"end": [1, 1]})),
        ("sketch.arc.tangent", json!({"end": [1, 1], "start": null})),
        ("sketch.arc.tangent", json!({"end": [1, 1], "start": 5})),
        ("sketch.arc.tangent", json!({"end": [1, 1], "start": {"at": 5}})),
        ("sketch.arc.tangent", json!({"end": [1, 1], "start": "on:l1:5,0"})),
        ("sketch.arc.tangent", json!({"end": "no_such_point", "start": "l1.end"})),
        ("sketch.arc.tangent", json!({"end": [50, 20], "start": "l1.end", "curve": "no_such_curve"})),
        ("sketch.arc.tangent", json!({"end": [50, 0], "start": "l1.end"})),
        ("sketch.circle.two_tangent", json!({"radius": 5, "near": [20, 10]})),
        ("sketch.circle.two_tangent", json!({"radius": 5, "near": [20, 10], "curves": null})),
        ("sketch.circle.two_tangent", json!({"radius": 5, "near": [20, 10], "curves": ["l1"]})),
        ("sketch.circle.two_tangent", json!({"radius": 5, "near": [20, 10], "curves": 5})),
        ("sketch.circle.two_tangent", json!({"radius": 5, "near": [20, 10], "curves": [1, 2]})),
        ("sketch.circle.two_tangent", json!({"radius": 5, "near": [20, 10], "curves": ["l1", true]})),
        ("sketch.circle.two_tangent", json!({"radius": 5, "near": [20, 10], "curves": ["l1", "l1", "l2"]})),
        ("sketch.circle.two_tangent", json!({"radius": 5, "near": [20, 10], "curves": ["l1", "no_such_curve"]})),
        ("sketch.circle.two_tangent", json!({"radius": "no_such_parameter", "near": [20, 10], "curves": ["l1", "l2"]})),
        ("sketch.circle.two_tangent", json!({"radius": 0, "near": [20, 10], "curves": ["l1", "l2"]})),
        ("sketch.circle.three_tangent", json!({"near": [20, 10]})),
        ("sketch.circle.three_tangent", json!({"near": [20, 10], "curves": ["l1", "l2"]})),
        ("sketch.circle.three_tangent", json!({"near": [20, 10], "curves": {}})),
        ("sketch.circle.three_tangent", json!({"near": [20, 10], "curves": ["l1", "l2", "no_such_curve"]})),
        ("sketch.slot.center_point", json!({"center": [0, 0], "end": [0, 0], "width": 2})),
        ("sketch.slot.center_point", json!({"center": [0, 0], "end": [5, 0], "width": 2, "sketch": [1]})),
        ("sketch.slot.arc_three_point", json!({"start": [0, 0], "through": [1, 0], "end": [2, 0], "width": 1})),
        ("sketch.slot.arc_three_point", json!({"start": [0, 0], "through": [1, 1], "end": [2, 0], "width": 30})),
        ("sketch.slot.arc_center", json!({"center": [0, 0], "start": [0, 0], "width": 1, "sweep": 90})),
        ("sketch.slot.arc_center", json!({"center": [0, 0], "start": [5, 0], "width": 1, "sweep": 0})),
        ("sketch.slot.arc_center", json!({"center": [0, 0], "start": [5, 0], "width": 1})),
        ("sketch.slot.arc_center", json!({"center": [0, 0], "start": [5, 0], "width": 1, "end": [0, 0]})),
        ("sketch.ellipse", json!({"center": [0, 0], "major": [0, 0], "minor_radius": 3})),
        ("sketch.ellipse", json!({"center": [0, 0], "major": [10, 0], "minor": [20, 0]})),
        ("sketch.ellipse", json!({"center": [0, 0], "major": [10, 0], "minor_radius": 3, "sketch": 4096})),
        ("sketch.conic", json!({"end": [2, 0], "apex": [1, 1]})),
        ("sketch.conic", json!({"start": null, "end": [2, 0], "apex": [1, 1]})),
        ("sketch.conic", json!({"start": [0, 0], "end": 5, "apex": [1, 1]})),
        ("sketch.conic", json!({"start": [0, 0], "end": [2, 0]})),
        ("sketch.conic", json!({"start": "l1.start", "end": "l1.start", "apex": [1, 1]})),
        ("sketch.conic", json!({"start": [0, 0], "end": [2, 0], "apex": "no_such_point"})),
        ("sketch.conic", json!({"start": [0, 0], "end": [2, 0], "apex": "x:l1+l1:1,1"})),
        ("sketch.spline.fit_point", json!({})),
        ("sketch.spline.fit_point", json!({"points": 5})),
        ("sketch.spline.fit_point", json!({"points": null})),
        ("sketch.spline.fit_point", json!({"points": [[0, 0]]})),
        ("sketch.spline.fit_point", json!({"points": vec![json!([0, 1]); 501]})),
        ("sketch.spline.fit_point", json!({"points": [[0, 0], 5]})),
        ("sketch.spline.fit_point", json!({"points": [[0, 0], [1, 1], {"at": "x"}]})),
        ("sketch.spline.fit_point", json!({"points": [[0, 0], [0, 0]]})),
        ("sketch.spline.fit_point", json!({"points": [[0, 0], [1, 1]], "sketch": true})),
        ("sketch.spline.control_point", json!({"points": [[0, 0], "no_such_point"]})),
        ("sketch.spline.control_point_5", json!({"points": [[0, 0], [1, 1], ["", 3]]})),
        ("sketch.blend_curve", json!({"b": "l2.end"})),
        ("sketch.blend_curve", json!({"a": "l1.end"})),
        ("sketch.blend_curve", json!({"a": null, "b": "l2.end"})),
        ("sketch.blend_curve", json!({"a": "l1.end", "b": 5})),
        ("sketch.blend_curve", json!({"a": "l1.end", "b": "l1.end"})),
        ("sketch.blend_curve", json!({"a": [1, 1], "b": "l2.end"})),
        ("sketch.blend_curve", json!({"a": "no_such_point", "b": "l2.end"})),
        ("sketch.blend_curve", json!({"a": "l1.end", "b": "l2.end", "continuity": "g5"})),
        ("sketch.centerline", json!({"curves": ["no_such_curve"]})),
        ("sketch.centerline", json!({"curves": ["c1"]})),
        ("sketch.infinite", json!({"curves": ["l1", "c1"]})),
        ("sketch.infinite", json!({"curves": ["no_such_curve"]})),
        ("sketch.text", json!({"at": at, "text": "x", "path": "no_such_curve"})),
        ("sketch.text", json!({"at": at, "text": "x", "path": t})),
        ("sketch.edit_text", json!({})),
        ("sketch.edit_text", json!({"link": 5})),
        ("sketch.edit_text", json!({"link": null, "entity": null})),
        ("sketch.edit_text", json!({"link": "no_such_link"})),
        ("sketch.edit_text", json!({"entity": "no_such_curve"})),
        ("sketch.edit_text", json!({"entity": "l1"})),
        ("sketch.edit_text", json!({"link": t, "text": 5})),
        ("sketch.edit_text", json!({"link": t, "text": ""})),
        ("sketch.edit_text", json!({"link": t, "text": "x", "text_expression": "'y'"})),
        ("sketch.edit_text", json!({"link": t, "text_expression": 5})),
        ("sketch.edit_text", json!({"link": t, "at": [1]})),
        ("sketch.edit_text", json!({"link": t, "at": "x"})),
        ("sketch.edit_text", json!({"link": t, "height": -1})),
        ("sketch.edit_text", json!({"link": t, "height": []})),
        ("sketch.edit_text", json!({"link": t, "angle": {}})),
        ("sketch.edit_text", json!({"link": t, "font": false})),
        ("sketch.edit_text", json!({"link": t, "bold": "no"})),
        ("sketch.edit_text", json!({"link": t, "align": "middle"})),
        ("sketch.edit_text", json!({"link": t, "letter_spacing": true})),
        ("sketch.edit_text", json!({"link": t, "line_spacing": -1})),
        ("sketch.edit_text", json!({"link": t, "path": 3})),
        ("sketch.edit_text", json!({"edits": []})),
        ("sketch.edit_text", json!({"edits": "t1"})),
        ("sketch.edit_text", json!({"edits": {"link": t}})),
        ("sketch.edit_text", json!({"edits": [5]})),
        ("sketch.edit_text", json!({"edits": [{}]})),
        ("sketch.edit_text", json!({"edits": [{"link": t}, {"link": "no_such_link"}]})),
        ("sketch.edit_text", json!({"edits": [{"link": t, "text": 5}]})),
        ("sketch.edit_text", json!({"edits": [{"link": t, "at": [1, 2, 3]}]})),
        ("sketch.edit_text", json!({"edits": vec![json!({"link": t}); 1001]})),
        ("sketch.edit_text", json!({"edits": [{"link": t, "bold": 1}], "live": false})),
    ]);
}

/// A key the handler reads leniently (a wrong type read as left out) is a mistake the check
/// reports: the one place where strict refuses a call that runs today, as documented.
#[test]
fn wrong_types_the_handlers_ignore_are_reported() {
    for (id, p, message) in [
        ("sketch.conic", json!({"start": [0, 0], "end": [2, 0], "apex": [1, 1], "rho": "x"}), "`rho` must be a number between 0 and 1"),
        ("sketch.line.midpoint", json!({"mid": [0, 0], "end": [2, 0], "construction": "yes"}), "`construction` must be true or false"),
        ("sketch.slot.center_point", json!({"center": [0, 0], "end": [2, 0], "width": 1, "connect": 1}), "`connect` must be true or false"),
        ("sketch.arc.tangent", json!({"start": "l1.end", "end": [2, 0], "curve": 5}), "`curve` must be text"),
        ("sketch.centerline", json!({"curves": ["l1"], "value": 1}), "`value` must be true or false"),
        ("sketch.edit_text", json!({"link": "t1", "live": 1}), "`live` must be true or false"),
    ] {
        assert_eq!(issues(id, p), [message], "{id}");
    }
}

/// Keys a handler reads only in some cases are checked only in those cases: a call that works
/// never gets a warning for the rest.
#[test]
fn keys_read_only_in_some_cases_are_checked_only_then() {
    let ok = |id: &str, p: Value| assert_eq!(issues(id, p.clone()), Vec::<String>::new(), "{id} {p}");
    let near = json!([20, 10]);
    // radius wins over diameter, whatever the diameter is; without a radius, a diameter does.
    ok("sketch.circle.two_tangent", json!({"radius": 5, "diameter": "wide", "near": near, "curves": ["l1", "l2"]}));
    ok("sketch.circle.two_tangent", json!({"radius": "r / 2", "diameter": null, "near": near, "curves": ["l1", "l2"]}));
    ok("sketch.circle.two_tangent", json!({"radius": true, "diameter": 10, "near": near, "curves": ["l1", "l2"]}));
    ok("sketch.circle.two_tangent", json!({"diameter": 10, "near": near, "curves": ["l1", "l2"]}));
    assert_eq!(
        issues("sketch.circle.two_tangent", json!({"diameter": "wide", "near": near, "curves": ["l1", "l2"]})),
        ["needs `radius` or `diameter`"]
    );
    // sweep wins over end, however end is written; without a sweep, end is read.
    let slot = |extra: Value| {
        let mut p = json!({"center": [0, 0], "start": [5, 0], "width": 1});
        p.as_object_mut().into_iter().for_each(|o| o.extend(extra.as_object().cloned().unwrap_or_default()));
        p
    };
    ok("sketch.slot.arc_center", slot(json!({"sweep": 90, "end": "nowhere"})));
    ok("sketch.slot.arc_center", slot(json!({"sweep": 90, "end": null})));
    ok("sketch.slot.arc_center", slot(json!({"end": [0, 5]})));
    assert_eq!(issues("sketch.slot.arc_center", slot(json!({"end": "nowhere"}))), ["`end` must be a point [x, y]"]);
    assert_eq!(issues("sketch.slot.arc_center", slot(json!({"sweep": null, "end": [0, 5]}))), ["`sweep` must be finite degrees"]);
    // minor_radius wins over minor, and a minor_radius that is no number is not read when there is a minor.
    let ellipse = |extra: Value| {
        let mut p = json!({"center": [0, 0], "major": [10, 0]});
        p.as_object_mut().into_iter().for_each(|o| o.extend(extra.as_object().cloned().unwrap_or_default()));
        p
    };
    ok("sketch.ellipse", ellipse(json!({"minor_radius": 3, "minor": "nowhere"})));
    ok("sketch.ellipse", ellipse(json!({"minor_radius": "wide", "minor": [1, 1]})));
    ok("sketch.ellipse", ellipse(json!({"minor_radius": null, "minor": [1, 1]})));
    ok("sketch.ellipse", ellipse(json!({"minor": {"x": 1, "y": 1}})));
    assert_eq!(issues("sketch.ellipse", ellipse(json!({"minor_radius": "wide", "minor": "x"}))), ["needs `minor` [x,y] or `minor_radius`"]);
    // Outside `edits` a text edit reads its own keys; with `edits` it reads only `edits` (and `live`).
    ok("sketch.edit_text", json!({"edits": [{"link": "t1"}], "link": 5, "text": 7, "at": "no", "bold": "yes"}));
    ok("sketch.edit_text", json!({"edits": null, "link": "t1", "text": "x"}));
    assert_eq!(issues("sketch.edit_text", json!({"edits": [{"entity": "c1", "bold": "yes"}]})), ["`bold` must be true or false"]);
    // A link that isn't text is read as left out when there is an entity, and the entity as left out when there is a link.
    ok("sketch.edit_text", json!({"link": 5, "entity": "l1"}));
    ok("sketch.edit_text", json!({"link": "t1", "entity": 5}));
    assert_eq!(issues("sketch.edit_text", json!({"link": 5, "entity": 5})), ["give `link` or `entity`"]);
    // The texts of a single edit read their own `at` as null: kept.
    ok("sketch.edit_text", json!({"link": "t1", "at": null, "height": null, "text": null}));
    // A text on a path: `at` is checked all the same, as the handler does.
    assert_eq!(issues("sketch.text", json!({"text": "x", "at": null})), [crate::cmd::AT_MUST]);
    ok("sketch.text", json!({"text": "x", "at": [0, 0], "sketch": 4}));
    ok("sketch.edit_text", json!({"link": "t1", "sketch": "S"}));
}

/// Every form of every creation command runs in warn without warnings, replies as its output
/// schema says, and a misspelt key gets its warning while the reply still matches.
#[test]
fn creation_replies_match_the_output_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    run(&mut s, "parameters.add", json!({"name": "tall", "expression": "6 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    for points in [[[0, 0], [40, 0]], [[40, 0], [40, 30]], [[40, 30], [0, 30]]] {
        run(&mut s, "sketch.line", json!({"points": points}));
    }
    let run_ok = |s: &mut Session, id: &str, p: Value| {
        let r = run(s, id, p.clone());
        assert!(r.get("param_warnings").is_none(), "{id} {p}: {r}");
        r
    };
    let drawn = |s: &mut Session, id: &str, p: Value| {
        let r = run_ok(s, id, p);
        let slot = id == "sketch.slot.center_point";
        matches(if slot { output_schema_for::<crate::schema::sketch_shapes::SlotResult>() } else { output_schema_for::<DrawResult>() }, &r);
        let expected: std::collections::BTreeSet<String> = if slot {
            ["centre_line", "constraints", "created", "curves", "middle", "sketch"].map(String::from).into()
        } else {
            ["constraints", "created", "curves", "sketch"].map(String::from).into()
        };
        assert_eq!(r.as_object().map(|o| o.keys().cloned().collect::<std::collections::BTreeSet<_>>()), Some(expected), "{r}");
        r
    };
    let near = json!([30, 10]);
    for p in [
        json!({"mid": [100, 5], "end": [110, 5]}),
        json!({"mid": [100, 15], "end": [110, 15], "construction": true}),
        json!({"mid": "l1.start", "end": [10, 10], "construction": null, "sketch": null}),
        json!({"mid": {"ref": "l1.end"}, "end": {"x": 50, "y": 10}}),
        json!({"mid": {"at": [100, 25]}, "end": [110, 25]}),
        json!({"mid": ["101 mm", "26"], "end": [111, 26]}),
        json!({"mid": "mid:l1", "end": [20, 10]}),
        json!({"mid": "on:l3:10,30", "end": [10, 20]}),
    ] {
        drawn(&mut s, "sketch.line.midpoint", p);
    }
    drawn(&mut s, "sketch.arc.tangent", json!({"start": "l2.end", "end": [60, 40], "curve": "l2"}));
    drawn(&mut s, "sketch.circle.two_tangent", json!({"radius": 5, "near": near, "curves": ["l1", "l2"]}));
    drawn(&mut s, "sketch.circle.two_tangent", json!({"radius": "tall", "near": near, "curves": ["l1", "l2"]}));
    drawn(&mut s, "sketch.circle.two_tangent", json!({"diameter": 8, "near": near, "curves": ["l1", "l2"]}));
    drawn(&mut s, "sketch.circle.two_tangent", json!({"radius": 4, "diameter": "ignored", "near": near, "curves": ["l1", "l2"]}));
    drawn(&mut s, "sketch.circle.three_tangent", json!({"near": [25, 15], "curves": ["l1", "l2", "l3"]}));
    drawn(&mut s, "sketch.slot.center_point", json!({"center": [100, 100], "end": [110, 100], "width": 4}));
    drawn(&mut s, "sketch.slot.center_point", json!({"center": [100, 120], "end": [110, 120], "width": 4, "connect": false, "sketch": null}));
    for p in [
        json!({"start": [100, 0], "through": [110, 10], "end": [120, 0], "width": 2}),
        json!({"start": [130, 0], "through": [140, 10], "end": [150, 0], "width": 2, "sketch": null}),
    ] {
        let r = run_ok(&mut s, "sketch.slot.arc_three_point", p);
        matches(output_schema_for::<ArcSlotResult>(), &r);
        assert!(r["centre_arc"].is_string() && r["middle"].is_string(), "{r}");
    }
    for p in [
        json!({"center": [200, 0], "start": [210, 0], "end": [200, 10], "width": 2}),
        json!({"center": [200, 30], "start": [210, 30], "sweep": 90, "width": 2}),
        json!({"center": [200, 60], "start": [210, 60], "sweep": 90, "end": "nowhere", "width": 2}),
    ] {
        let r = run_ok(&mut s, "sketch.slot.arc_center", p);
        matches(output_schema_for::<ArcSlotResult>(), &r);
        assert!(r["centre_arc"].is_string() && r["middle"].is_string(), "{r}");
    }
    for p in [
        json!({"center": [300, 0], "major": [310, 0], "minor_radius": 4}),
        json!({"center": [300, 20], "major": [310, 20], "minor": [302, 24], "axes": false}),
        json!({"center": [300, 40], "major": [310, 40], "minor": [302, 44], "axes": null, "construction": true}),
        json!({"center": [300, 60], "major": [310, 60], "minor_radius": 4, "minor": "nowhere"}),
        json!({"center": [300, 80], "major": [310, 80], "minor_radius": "wide", "minor": {"x": 302, "y": 84}}),
    ] {
        drawn(&mut s, "sketch.ellipse", p);
    }
    drawn(&mut s, "sketch.spline.fit_point", json!({"points": [[0, 50], [10, 60], [20, 50]]}));
    drawn(&mut s, "sketch.spline.fit_point", json!({"points": [[30, 50], [40, 60], [50, 50], [45, 45]], "periodic": true, "construction": true}));
    drawn(&mut s, "sketch.spline.fit_point", json!({"points": ["l1.start", {"at": [10, 90]}, ["20 mm", 80]], "periodic": false}));
    drawn(&mut s, "sketch.spline.control_point", json!({"points": [[0, 110], [10, 120], [20, 110], [30, 120]], "periodic": false}));
    drawn(&mut s, "sketch.spline.control_point_5", json!({"points": [[0, 130], [10, 140], [20, 130], [30, 140], [40, 130], [50, 140]]}));
    drawn(&mut s, "sketch.conic", json!({"start": [0, 150], "end": [20, 150], "apex": [10, 160]}));
    drawn(&mut s, "sketch.conic", json!({"start": [30, 150], "end": [50, 150], "apex": [40, 160], "rho": 0.3, "construction": true}));
    drawn(&mut s, "sketch.blend_curve", json!({"a": "l1.start", "b": "l3.end"}));
    drawn(&mut s, "sketch.blend_curve", json!({"a": {"ref": "l1.end"}, "b": "l2.end", "continuity": " G2 "}));
    drawn(&mut s, "sketch.blend_curve", json!({"a": "l1.start", "b": "l2.end", "continuity": "", "sketch": null}));
    for (id, p) in [
        ("sketch.centerline", json!({"curves": ["l1"], "value": true})),
        ("sketch.centerline", json!({"curves": "l1", "value": null})),
        ("sketch.infinite", json!({"curves": ["l2", "l3"], "value": true, "sketch": null})),
        ("sketch.infinite", json!({"curves": "l2"})),
    ] {
        let r = run_ok(&mut s, id, p);
        matches(output_schema_for::<LineTypeResult>(), &r);
        assert_eq!(
            r.as_object().map(|o| o.keys().cloned().collect::<std::collections::BTreeSet<_>>()),
            Some(["changed", "created", "sketch"].map(String::from).into()),
            "{r}"
        );
    }
    // Sketch text: its forms, and its edits.
    let text = |s: &mut Session, p: Value| {
        let r = run_ok(s, "sketch.text", p);
        matches(output_schema_for::<TextResult>(), &r);
        r
    };
    let first = text(&mut s, json!({"text": "Hello\nthere", "at": [0, 200]}));
    assert!(first["guide"].is_null() && first.get("exprs").is_none(), "{first}");
    let link = first["link"].as_str().unwrap_or_default().to_string();
    text(
        &mut s,
        json!({"text": "Styled", "at": {"x": 50, "y": 200}, "height": 8, "angle": 10, "font": "", "bold": false, "italic": true, "reverse": false, "align": "Center", "letter_spacing": 0.5, "line_spacing": 1.2, "path": ""}),
    );
    text(&mut s, json!({"text": null, "text_expression": "'A' + 'B'", "at": [0, 220], "bold": null, "align": null, "path": null}));
    let expr = text(&mut s, json!({"text": "Param", "at": ["1 + 1", 230], "height": "tall", "angle": "30 deg"}));
    // An expression that names no parameter is the number it comes to.
    assert_eq!(expr["exprs"], json!({"height": "tall"}), "{expr}");
    let on_path = text(&mut s, json!({"text": "Curve", "at": [0, 0], "path": "l1", "align": " RIGHT ", "reverse": true}));
    assert_eq!(on_path["guide"], json!("l1"), "{on_path}");
    let other = text(&mut s, json!({"text": "Other", "at": [0, 260]}));
    let outline = other["curves"][0].clone();
    for p in [
        json!({"link": link, "text": "Changed"}),
        json!({"link": link, "at": [5, 205], "height": "tall", "angle": null, "font": null, "line_spacing": null}),
        json!({"entity": outline, "italic": true, "live": false}),
        json!({"link": link, "entity": 5, "text": "Same", "edits": null}),
        json!({"edits": [{"link": link, "text": "One"}, {"entity": outline, "height": 9}], "link": 5, "text": 7, "at": "ignored"}),
    ] {
        let r = run_ok(&mut s, "sketch.edit_text", p.clone());
        matches(output_schema_for::<EditTextResult>(), &r);
        assert!(r["edits"].is_array() == p.get("edits").is_some_and(|e| !e.is_null()), "{r}");
    }
    // A misspelt key runs in warn and says so; the reply still matches. Strict refuses it.
    let misspelt: [(&str, Value); 17] = [
        ("sketch.line.midpoint", json!({"mid": [100, 45], "end": [110, 45], "constrution": true})),
        ("sketch.arc.tangent", json!({"start": "l2.end", "end": [62, 42], "curv": "l2"})),
        ("sketch.circle.two_tangent", json!({"radious": 3, "diameter": 6, "near": near, "curves": ["l1", "l2"]})),
        ("sketch.circle.three_tangent", json!({"nere": 1, "near": [25, 15], "curves": ["l1", "l2", "l3"]})),
        ("sketch.slot.center_point", json!({"center": [100, 140], "end": [110, 140], "widht": 1, "width": 4})),
        ("sketch.slot.arc_three_point", json!({"start": [160, 0], "through": [170, 10], "end": [180, 0], "width": 2, "throgh": 1})),
        ("sketch.slot.arc_center", json!({"center": [200, 90], "start": [210, 90], "sweep": 90, "width": 2, "swep": 1})),
        ("sketch.ellipse", json!({"center": [300, 100], "major": [310, 100], "minor_radius": 4, "axs": false})),
        ("sketch.spline.fit_point", json!({"points": [[60, 50], [70, 60], [80, 50]], "periodc": true})),
        ("sketch.spline.control_point", json!({"points": [[60, 110], [70, 120], [80, 110]], "point": 1})),
        ("sketch.spline.control_point_5", json!({"points": [[60, 130], [70, 140], [80, 130], [90, 140], [100, 130], [110, 140]], "constructon": 1})),
        ("sketch.conic", json!({"start": [60, 150], "end": [80, 150], "apex": [70, 160], "rhoo": 0.4})),
        ("sketch.blend_curve", json!({"a": "l1.start", "b": "l3.end", "continuty": "g1"})),
        ("sketch.centerline", json!({"curves": ["l1"], "vlaue": true})),
        ("sketch.infinite", json!({"curves": ["l2"], "vlaue": true})),
        ("sketch.text", json!({"text": "Oops", "at": [0, 240], "hieght": 3})),
        ("sketch.edit_text", json!({"link": link, "txt": "x"})),
    ];
    for (id, p) in &misspelt {
        let r = s.execute(id, p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert!(
            r["param_warnings"].as_array().is_some_and(|w| w.len() == 1 && w[0].as_str().is_some_and(|m| m.contains("unknown key"))),
            "{id}: {r}"
        );
    }
    s.param_check = ParamCheck::Strict;
    for (id, p) in &misspelt {
        let error = s.execute(id, p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(error.contains("unknown key"), "{id}: {error}");
    }
}

/// Each command's schemas describe it: a summary, and a description on every key.
#[test]
fn creation_schemas_are_described() {
    for id in [
        "sketch.arc.tangent",
        "sketch.blend_curve",
        "sketch.centerline",
        "sketch.circle.three_tangent",
        "sketch.circle.two_tangent",
        "sketch.conic",
        "sketch.edit_text",
        "sketch.ellipse",
        "sketch.infinite",
        "sketch.line.midpoint",
        "sketch.slot.arc_center",
        "sketch.slot.arc_three_point",
        "sketch.slot.center_point",
        "sketch.spline.control_point",
        "sketch.spline.control_point_5",
        "sketch.spline.fit_point",
        "sketch.text",
    ] {
        let c = crate::cmd::find_command(id).unwrap_or_else(|| panic!("{id}"));
        let input = c.contract.map(|c| c.input_schema()).unwrap_or_else(|| panic!("{id}"));
        let input = input.as_value();
        assert!(input["description"].as_str().is_some_and(|d| d.len() > 20), "{id}: no summary");
        for (key, property) in input["properties"].as_object().into_iter().flatten() {
            assert!(property.get("description").is_some() || property.get("$ref").is_some(), "{id}: `{key}` has no description");
        }
        let output = (c.output.unwrap_or_else(|| panic!("{id}")))();
        assert!(
            output.as_value().get("properties").is_some() || output.as_value().get("oneOf").is_some() || output.as_value().get("anyOf").is_some(),
            "{id}"
        );
    }
}

/// The schemas ask for what the handlers can't do without, and take what they read.
#[test]
fn the_creation_schemas_take_what_the_handlers_read() {
    let takes = |schema: schemars::Schema, cases: &[(Value, bool)]| {
        let schema = schema.as_value();
        for (p, takes) in cases {
            assert_eq!(conforms(schema, schema, p), *takes, "{p}");
        }
    };
    takes(
        input_schema_for::<TwoTangentCircleParams>(),
        &[
            (json!({"radius": 5, "near": [1, 2], "curves": ["l1", "l2"]}), true),
            (json!({"diameter": 5, "near": {"x": 1, "y": 2}, "curves": ["l1", "l2"]}), true),
            (json!({"diameter": 5, "near": [1, 2], "curves": "l1"}), false),
            (json!({"radius": "r / 2", "near": [1, 2], "curves": ["l1", "l2"], "sketch": 3}), true),
            (json!({"near": [1, 2], "curves": ["l1", "l2"]}), false),
            (json!({"radius": 5, "curves": ["l1", "l2"]}), false),
            (json!({"radius": 5, "near": [1, 2]}), false),
            (json!({"radius": 5, "near": [1, 2], "curves": ["l1"]}), false),
            (json!({"radius": 5, "near": [1, 2], "curves": ["l1", "l2", "l3"]}), false),
        ],
    );
    takes(
        input_schema_for::<ArcSlotCenterParams>(),
        &[
            (json!({"center": [0, 0], "start": [5, 0], "width": 1, "sweep": 90}), true),
            (json!({"center": [0, 0], "start": [5, 0], "width": 1, "end": [0, 5]}), true),
            (json!({"center": [0, 0], "start": [5, 0], "width": 1}), false),
            (json!({"center": [0, 0], "start": [5, 0], "sweep": 90}), false),
            (json!({"center": [0, 0], "start": [5, 0], "width": 0, "sweep": 90}), false),
        ],
    );
    takes(
        input_schema_for::<EllipseParams>(),
        &[
            (json!({"center": [0, 0], "major": [5, 0], "minor_radius": 2}), true),
            (json!({"center": [0, 0], "major": [5, 0], "minor": [1, 1], "axes": false}), true),
            (json!({"center": [0, 0], "major": [5, 0]}), false),
            (json!({"center": [0, 0], "minor_radius": 2}), false),
            (json!({"center": [0, 0], "major": [5, 0], "minor_radius": 0}), false),
        ],
    );
    takes(
        input_schema_for::<ConicParams>(),
        &[
            (json!({"start": [0, 0], "end": "l1.end", "apex": {"vertex": [1, 2, 3]}, "rho": 0.2}), true),
            (json!({"start": ["w / 2", 0], "end": {"ref": "p1"}, "apex": "mid:l1"}), true),
            (json!({"start": [0, 0], "end": [1, 0]}), false),
            (json!({"start": [0, 0], "end": [1, 0], "apex": [0, 1], "rho": 1}), false),
        ],
    );
    takes(
        input_schema_for::<FitPointSplineParams>(),
        &[
            (json!({"points": [[0, 0], [1, 1]], "periodic": true}), true),
            (json!({"points": [[0, 0]]}), false),
            (json!({"points": vec![json!([0, 0]); 501]}), false),
            (json!({"periodic": true}), false),
        ],
    );
    takes(
        input_schema_for::<ControlPointSplineParams>(),
        &[(json!({"points": [[0, 0], [1, 1]], "periodic": false}), true), (json!({"points": [[0, 0]]}), false)],
    );
    takes(
        input_schema_for::<BlendCurveParams>(),
        &[
            (json!({"a": "l1.end", "b": "l2.start", "continuity": "G3"}), true),
            (json!({"a": "l1.end", "b": "l2.start", "continuity": ""}), true),
            (json!({"a": "l1.end"}), false),
        ],
    );
    takes(
        input_schema_for::<CenterlineParams>(),
        &[
            (json!({"curves": "l1"}), true),
            (json!({"curves": ["l1", "l2"], "value": false}), true),
            (json!({"curves": []}), false),
            (json!({}), false),
        ],
    );
    takes(
        input_schema_for::<SketchTextParams>(),
        &[
            (json!({"text": "x", "at": [0, 0]}), true),
            (json!({"text_expression": "'x'", "at": ["w / 2", "5"]}), true),
            (json!({"text": "x", "at": {"x": 1, "y": 2}, "height": "h", "angle": "30 deg", "align": "CENTRE", "path": ""}), true),
            (json!({"text": "x"}), false),
            (json!({"at": [0, 0]}), false),
            (json!({"text": "x", "at": [0, 0], "height": 0}), false),
        ],
    );
    takes(
        input_schema_for::<EditTextParams>(),
        &[
            (json!({"link": "j1", "text": "y"}), true),
            (json!({"entity": "n1", "at": [1, 2]}), true),
            (json!({"edits": [{"link": "j1"}, {"entity": "n1", "height": 4}], "live": false}), true),
            (json!({"edits": []}), false),
            (json!({"edits": [{"text": "y"}]}), false),
            (json!({"text": "y"}), false),
        ],
    );
}

/// The point forms the types take are the forms the handlers read: `sketch.line.midpoint` snaps
/// its points, `sketch.blend_curve` does not.
#[test]
fn point_forms_match_the_point_readers() {
    let refused =
        |e: &crate::EngineError| e.to_string().contains("a point must be [x, y]") || e.to_string().contains("a point's coordinates must be");
    let forms = [
        json!([1, 2]),
        json!({"x": 1, "y": 2}),
        json!([1e9, 2]),
        json!([1, "2"]),
        json!(["1 mm", "2"]),
        json!(["1", true]),
        json!([true, "2"]),
        json!(["", 2]),
        json!([" ", 2]),
        json!(["x".repeat(4097), 2]),
        json!([1e9, "2"]),
        json!([null, "2"]),
        json!([1, 2, 3]),
        json!([1]),
        json!([]),
        json!("l1.end"),
        json!(""),
        json!("mid:l1"),
        json!("vertex:1,2,3"),
        json!({"ref": "l1.end"}),
        json!({"ref": 5}),
        json!({"ref": 5, "at": [1, 2]}),
        json!({"ref": "l1.end", "at": 5}),
        json!({"at": [1, 2]}),
        json!({"at": {"x": 1, "y": 2}}),
        json!({"at": [1, "2"]}),
        json!({"at": 5}),
        json!({"vertex": [1, 2, 3]}),
        json!({"vertex": [1, 2]}),
        json!({"vertex": [1, 2], "at": [1, 2]}),
        json!({"on_edge": {"x": 1, "y": 2, "z": 3}}),
        json!({"on_edge": "x"}),
        json!({}),
        json!({"x": 1}),
        json!(5),
        json!(true),
        json!(null),
        json!([[1, 2]]),
    ];
    for v in forms {
        let mut s = drawing(ParamCheck::Off);
        let snapped = s.execute("sketch.line.midpoint", &json!({"mid": v, "end": [7, 7]}));
        assert_eq!(crate::cmd::point_arg_form(&v, true).is_ok(), !snapped.as_ref().err().is_some_and(refused), "mid {v}: {snapped:?}");
        let mut s = drawing(ParamCheck::Off);
        let plain = s.execute("sketch.blend_curve", &json!({"a": v, "b": "l2.end"}));
        assert_eq!(crate::cmd::point_arg_form(&v, false).is_ok(), !plain.as_ref().err().is_some_and(refused), "a {v}: {plain:?}");
        // The type says the same.
        assert_eq!(serde_json::from_value::<super::SnapPoint>(v.clone()).is_ok(), crate::cmd::point_arg_form(&v, true).is_ok(), "{v}");
        assert_eq!(serde_json::from_value::<super::SketchPoint>(v.clone()).is_ok(), crate::cmd::point_arg_form(&v, false).is_ok(), "{v}");
    }
}

/// A text edit made `live` goes to the live runner, which the check doesn't wrap yet: it still
/// replies as the output schema says, and says nothing about its keys.
#[test]
fn a_live_text_edit_replies_as_its_schema_says() {
    let mut s = drawing(ParamCheck::Warn);
    let link = text_link();
    let r = run(&mut s, "sketch.edit_text", json!({"link": link, "height": 7, "live": true, "unknown": 1}));
    matches(output_schema_for::<EditTextResult>(), &r);
    assert!(r.get("param_warnings").is_none() && r["link"] == json!(link), "{r}");
}

/// The patterns that spell what a handler reads ignoring case and blanks.
#[test]
fn the_case_blind_patterns_say_what_the_handlers_read() {
    assert_eq!(
        super::word_pattern(&["left", "right"]),
        format!("^[{0}]*([Ll][Ee][Ff][Tt]|[Rr][Ii][Gg][Hh][Tt])[{0}]*$", crate::schema::types::whitespace_class())
    );
}
