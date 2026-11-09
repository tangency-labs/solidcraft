use serde_json::{Value, json};

use super::{
    CoincidentParams, CoincidentResult, CollinearParams, ConcentricParams, ConstraintResult, EqualParams, EqualResult, FixParams, FixResult,
    HorizontalVerticalParams, MidpointParams, Operand, ParallelParams, PerpendicularParams, PointOperand, SketchKey, SymmetryParams, SymmetryResult,
    TangentParams, TangentResult,
};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, issues, output_schema_for};

const IDS: [&str; 11] = [
    "sketch.constraint.coincident",
    "sketch.constraint.collinear",
    "sketch.constraint.concentric",
    "sketch.constraint.equal",
    "sketch.constraint.fix",
    "sketch.constraint.horizontal_vertical",
    "sketch.constraint.midpoint",
    "sketch.constraint.parallel",
    "sketch.constraint.perpendicular",
    "sketch.constraint.symmetry",
    "sketch.constraint.tangent",
];

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A sketch being edited, with lines `l1` to `l6`, circles `c1` to `c4` and points `p1` and `p2`.
fn scene(level: ParamCheck) -> Session {
    let mut s = Session { param_check: ParamCheck::Off, ..Session::default() };
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    for (id, a, b) in [
        ("l1", [0, 0], [20, 5]),
        ("l2", [30, 0], [50, 10]),
        ("l3", [0, 20], [20, 30]),
        ("l4", [30, 20], [50, 22]),
        ("l5", [0, 40], [20, 41]),
        ("l6", [30, 40], [49, 52]),
    ] {
        run(&mut s, "sketch.line", json!({"points": [a, b], "ids": [id]}));
    }
    for (id, c, r) in [("c1", [0, 70], 5), ("c2", [30, 70], 8), ("c3", [60, 70], 3), ("c4", [60, 80], 3)] {
        run(&mut s, "sketch.circle.center", json!({"center": c, "radius": r, "ids": [id]}));
    }
    run(&mut s, "sketch.point", json!({"point": [5, 100], "id": "p1"}));
    run(&mut s, "sketch.point", json!({"point": [15, 105], "id": "p2"}));
    s.param_check = level;
    s
}

fn output_conforms<T: schemars::JsonSchema>(r: &Value) {
    let schema = output_schema_for::<T>();
    let schema = schema.as_value();
    assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<CoincidentParams>(), &["a", "b", "if_redundant", "sketch"]);
    assert_keys(input_schema_for::<CollinearParams>(), &["a", "b", "if_redundant", "sketch"]);
    assert_keys(input_schema_for::<ConcentricParams>(), &["a", "b", "if_redundant", "sketch"]);
    assert_keys(input_schema_for::<EqualParams>(), &["a", "b", "curves", "if_redundant", "sketch"]);
    assert_keys(input_schema_for::<FixParams>(), &["entities", "entity", "fixed", "sketch"]);
    assert_keys(input_schema_for::<HorizontalVerticalParams>(), &["if_redundant", "line", "mode", "points", "sketch"]);
    assert_keys(input_schema_for::<MidpointParams>(), &["a", "b", "if_redundant", "line", "point", "sketch"]);
    assert_keys(input_schema_for::<ParallelParams>(), &["a", "b", "if_redundant", "sketch"]);
    assert_keys(input_schema_for::<PerpendicularParams>(), &["a", "b", "if_redundant", "sketch"]);
    assert_keys(input_schema_for::<SymmetryParams>(), &["a", "b", "line", "point", "sketch"]);
    assert_keys(input_schema_for::<TangentParams>(), &["a", "b", "if_redundant", "sketch"]);
}

#[test]
fn the_commands_publish_their_schemas() {
    for id in IDS {
        let spec = crate::cmd::find_command(id).unwrap_or_else(|| panic!("{id}"));
        assert!(spec.contract.is_some() && spec.output.is_some(), "{id}");
    }
}

/// Keys a handler refuses when they are left out are required in the schema (and optional in the
/// type, so that the handler's message is the one a caller sees).
#[test]
fn keys_the_handler_needs_are_required_in_the_schema() {
    let required = |schema: schemars::Schema| -> Vec<String> {
        schema.as_value()["required"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect()
    };
    assert_eq!(required(input_schema_for::<TangentParams>()), ["a", "b"]);
    assert_eq!(required(input_schema_for::<CoincidentParams>()), ["a", "b"]);
    assert_eq!(required(input_schema_for::<ConcentricParams>()), ["a", "b"]);
    assert_eq!(required(input_schema_for::<SymmetryParams>()), ["a", "b"]);
    // The forms that exclude each other are alternatives.
    let schema = input_schema_for::<EqualParams>();
    assert_eq!(schema.as_value()["anyOf"], json!([{"required": ["a", "b"]}, {"required": ["curves"]}]));
    let schema = input_schema_for::<MidpointParams>();
    assert_eq!(schema.as_value()["anyOf"], json!([{"required": ["a", "b"]}, {"required": ["point", "line"]}]));
    let schema = input_schema_for::<HorizontalVerticalParams>();
    assert_eq!(schema.as_value()["anyOf"], json!([{"required": ["line"]}, {"required": ["points"]}]));
    let schema = input_schema_for::<FixParams>();
    assert_eq!(schema.as_value()["anyOf"], json!([{"required": ["entity"]}, {"required": ["entities"]}]));
}

#[test]
fn strict_keeps_todays_errors() {
    // The sketch is named: after the scene the handler would otherwise stop at "no sketch is being
    // edited" before it looks at the rest.
    let on = |mut p: Value| {
        p["sketch"] = json!("Sketch1");
        p
    };
    assert_strict_keeps_todays_errors(&[
        ("sketch.constraint.tangent", on(json!({"a": 5, "b": "x"}))),
        ("sketch.constraint.tangent", on(json!({"a": "x", "b": ["y"]}))),
        ("sketch.constraint.tangent", on(json!({"a": null, "b": "x"}))),
        ("sketch.constraint.tangent", on(json!({"a": {"point": 1}, "b": "x"}))),
        ("sketch.constraint.tangent", on(json!({"b": "x"}))),
        ("sketch.constraint.parallel", on(json!({"a": true, "b": "x"}))),
        ("sketch.constraint.perpendicular", on(json!({"a": "x", "b": 3}))),
        ("sketch.constraint.collinear", on(json!({"a": [1], "b": "x"}))),
        ("sketch.constraint.equal", on(json!({"a": 5, "b": "l1"}))),
        ("sketch.constraint.equal", on(json!({"a": "l1", "b": null}))),
        ("sketch.constraint.equal", on(json!({"curves": ["a"]}))),
        ("sketch.constraint.equal", on(json!({"curves": null}))),
        ("sketch.constraint.equal", on(json!({"curves": 5}))),
        ("sketch.constraint.equal", on(json!({"curves": ["a", 5]}))),
        ("sketch.constraint.equal", on(json!({"curves": vec!["a"; 201]}))),
        ("sketch.constraint.concentric", on(json!({"a": 5, "b": "x"}))),
        ("sketch.constraint.concentric", on(json!({"a": "x", "b": 5}))),
        ("sketch.constraint.coincident", on(json!({"a": 5, "b": "x"}))),
        ("sketch.constraint.coincident", on(json!({"a": "x", "b": 5}))),
        ("sketch.constraint.coincident", on(json!({"a": "x"}))),
        ("sketch.constraint.coincident", on(json!({"a": 5}))),
        ("sketch.constraint.coincident", on(json!({"a": {"nothing": 1}}))),
        ("sketch.constraint.fix", on(json!({"fixed": "x", "entities": 5}))),
        ("sketch.constraint.coincident", on(json!({"a": {"midpoint": 5}, "b": "x"}))),
        ("sketch.constraint.coincident", on(json!({"a": {"nothing": 5}, "b": "x"}))),
        ("sketch.constraint.horizontal_vertical", on(json!({}))),
        ("sketch.constraint.horizontal_vertical", on(json!({"line": 5}))),
        ("sketch.constraint.horizontal_vertical", on(json!({"points": ["l1.start"]}))),
        ("sketch.constraint.horizontal_vertical", on(json!({"points": ["l1.start", 5]}))),
        ("sketch.constraint.horizontal_vertical", on(json!({"points": ["l1.start", {"midpoint": 5}]}))),
        ("sketch.constraint.horizontal_vertical", on(json!({"points": 5, "line": 5}))),
        ("sketch.constraint.midpoint", on(json!({"a": 5, "b": "l1"}))),
        ("sketch.constraint.midpoint", on(json!({"a": "l1", "b": null}))),
        ("sketch.constraint.midpoint", on(json!({"a": "l1"}))),
        ("sketch.constraint.midpoint", on(json!({"point": 5, "line": "l1"}))),
        ("sketch.constraint.midpoint", on(json!({"point": "p1", "line": 5}))),
        ("sketch.constraint.midpoint", on(json!({"point": "p1"}))),
        ("sketch.constraint.symmetry", on(json!({"a": 5, "b": "x", "line": "y"}))),
        ("sketch.constraint.symmetry", on(json!({"a": "x", "b": 5, "line": "y"}))),
        ("sketch.constraint.symmetry", on(json!({"a": "x", "b": "y", "line": 5}))),
        ("sketch.constraint.symmetry", on(json!({"a": "x", "b": "y", "point": 5}))),
        ("sketch.constraint.symmetry", on(json!({"a": "x", "b": "y", "point": null}))),
        ("sketch.constraint.fix", on(json!({}))),
        ("sketch.constraint.fix", on(json!({"entities": []}))),
        ("sketch.constraint.fix", on(json!({"entity": 5}))),
        ("sketch.constraint.fix", on(json!({"entities": 5}))),
        ("sketch.constraint.fix", on(json!({"entities": ["p1", 5]}))),
        ("sketch.constraint.fix", on(json!({"entity": "p1", "entities": {"x": 1}}))),
    ]);
    // `sketch` is read the way `sketch_store::target` reads it.
    let with_sketch = |sketch: Value| json!({"a": "l1", "b": "l2", "sketch": sketch});
    assert_strict_keeps_todays_errors(&[
        ("sketch.constraint.parallel", with_sketch(json!(true))),
        ("sketch.constraint.parallel", with_sketch(json!(-1))),
        ("sketch.constraint.parallel", with_sketch(json!(1.5))),
        ("sketch.constraint.parallel", with_sketch(json!(["Sketch1"]))),
        ("sketch.constraint.parallel", with_sketch(json!(999))),
        ("sketch.constraint.parallel", with_sketch(json!("nothing"))),
        ("sketch.constraint.fix", json!({"entity": "p1", "sketch": {}})),
    ]);
}

/// The types read each key as the handlers do: what a handler accepts, they accept (and give the
/// handler's message for what it refuses); keys a handler does not read in a case are not looked
/// at in that case.
#[test]
fn the_types_read_each_key_as_the_handlers_do() {
    fn parse<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, String> {
        serde_json::from_value(v).map_err(|e| e.to_string())
    }
    fn whole_message(e: String) -> String {
        e.trim_start_matches(['\u{1}', '\u{2}', '\u{3}']).split(" at line ").next().unwrap_or_default().to_string()
    }

    // `sketch`: an id, a name or null.
    let p: TangentParams = parse(json!({"a": "x", "b": "y", "sketch": 3})).unwrap();
    assert_eq!(p.sketch, Some(SketchKey::Id(3)));
    let p: TangentParams = parse(json!({"a": "x", "b": "y", "sketch": "S"})).unwrap();
    assert_eq!(p.sketch, Some(SketchKey::Name("S".into())));
    let p: TangentParams = parse(json!({"a": "x", "b": "y", "sketch": null})).unwrap();
    assert_eq!(p.sketch, None);
    for bad in [json!(-1), json!(1.5), json!(true), json!([1]), json!({})] {
        let e = parse::<TangentParams>(json!({"a": "x", "b": "y", "sketch": bad})).map(|_| ()).unwrap_err();
        assert_eq!(whole_message(e), "`sketch` must be an id or a name", "{bad}");
    }

    // Operands: an id, or a model edge or vertex (projected before the handler reads them).
    let p: ParallelParams = parse(json!({"a": "l1", "b": {"edge": [0, 0, 0], "body": "Body1"}})).unwrap();
    assert_eq!((p.a, matches!(p.b, Some(Operand::Model(_)))), (Some(Operand::Id("l1".into())), true));
    let p: ParallelParams = parse(json!({"a": {"vertex": [1, 2, 3]}, "b": "y"})).unwrap();
    assert!(matches!(p.a, Some(Operand::Model(_))));
    let p: ParallelParams = parse(json!({})).unwrap();
    assert_eq!((p.a, p.b), (None, None), "left out: the handler's to refuse");
    let e = parse::<ParallelParams>(json!({"a": 5, "b": "y"})).map(|_| ()).unwrap_err();
    assert_eq!(whole_message(e), "`a` must be a curve id, or x / y for a sketch axis");
    let e = parse::<ConcentricParams>(json!({"a": "c1", "b": {"midpoint": "l1"}})).map(|_| ()).unwrap_err();
    assert_eq!(whole_message(e), "`b` must be a curve id");

    // `if_redundant` and the plain flags and texts.
    let e = parse::<TangentParams>(json!({"a": "x", "b": "y", "if_redundant": 5})).map(|_| ()).unwrap_err();
    assert_eq!(whole_message(e), "`if_redundant` must be text");
    assert!(parse::<TangentParams>(json!({"a": "x", "b": "y", "if_redundant": null})).is_ok());
    let e = parse::<FixParams>(json!({"entity": "p1", "fixed": "yes"})).map(|_| ()).unwrap_err();
    assert_eq!(whole_message(e), "`fixed` must be true or false");

    // Equal: with `curves`, `a`, `b` and `if_redundant` are not read.
    let p: EqualParams = parse(json!({"curves": ["c1", "c2"], "a": 5, "b": null, "if_redundant": 5})).unwrap();
    assert_eq!((p.curves.map(|c| c.0), p.a, p.b, p.if_redundant), (Some(vec!["c1".into(), "c2".into()]), None, None, None));
    assert_eq!(
        whole_message(parse::<EqualParams>(json!({"curves": ["c1"]})).map(|_| ()).unwrap_err()),
        "`curves` must list 2 to 200 lines or circles/arcs"
    );
    assert_eq!(
        whole_message(parse::<EqualParams>(json!({"curves": null, "a": "l1", "b": "l2"})).map(|_| ()).unwrap_err()),
        "`curves` must list 2 to 200 lines or circles/arcs"
    );
    assert_eq!(
        whole_message(parse::<EqualParams>(json!({"curves": ["c1", 7]})).map(|_| ()).unwrap_err()),
        "`curves` must list names; the entry 2 is 7"
    );
    assert_eq!(whole_message(parse::<EqualParams>(json!({"curves": 7})).map(|_| ()).unwrap_err()), "`curves` must be a list of names; it is 7");
    assert!(parse::<EqualParams>(json!({"curves": vec!["c"; 200]})).is_ok());
    assert!(parse::<EqualParams>(json!({"curves": vec!["c"; 201]})).is_err());
    // Without `curves`, `a` and `b` are read.
    assert_eq!(whole_message(parse::<EqualParams>(json!({"a": 5, "b": "l2"})).map(|_| ()).unwrap_err()), "`a` must be a curve id");

    // Coincident: the middle of a line is a point.
    let p: CoincidentParams = parse(json!({"a": {"midpoint": "l1"}, "b": "p1"})).unwrap();
    assert_eq!(p.a, Some(PointOperand::Midpoint("l1".into())));
    let msg = |v: Value| whole_message(parse::<CoincidentParams>(v).map(|_| ()).unwrap_err());
    assert_eq!(msg(json!({"a": {"midpoint": 3}, "b": "p1"})), "`midpoint` must be a curve id");
    assert_eq!(msg(json!({"a": "p1", "b": 3})), "`b` must be a point or curve");
    assert_eq!(msg(json!({"a": 3, "b": "p1"})), "`a` must be a point reference");
    // The handler reads `b` before `a`.
    assert_eq!(msg(json!({"a": 3, "b": 3})), "`b` must be a point or curve");

    // Horizontal / Vertical: `points` when it is a list, else `line`.
    let p: HorizontalVerticalParams = parse(json!({"points": ["l1.start", {"midpoint": "l2"}], "line": 5, "mode": "VERTICAL"})).unwrap();
    assert!(p.line.is_none() && p.points.is_some(), "`line` is not read when `points` is a list");
    let p: HorizontalVerticalParams = parse(json!({"points": "x", "line": "l1"})).unwrap();
    assert_eq!((p.line.as_deref(), p.points.is_some()), (Some("l1"), false), "a `points` that is not a list is not read");
    let p: HorizontalVerticalParams = parse(json!({"points": null, "line": "l1", "mode": "bogus"})).unwrap();
    assert_eq!(p.line.as_deref(), Some("l1"));
    let msg = |v: Value| whole_message(parse::<HorizontalVerticalParams>(v).map(|_| ()).unwrap_err());
    assert_eq!(msg(json!({"line": 5})), "`line` must be a curve id");
    assert_eq!(msg(json!({"points": "x", "line": 5})), "`line` must be a curve id");
    assert_eq!(msg(json!({"points": ["a"]})), "`points` needs exactly two point references");
    assert_eq!(msg(json!({"points": ["a", "b", "c"]})), "`points` needs exactly two point references");
    assert_eq!(msg(json!({"points": ["a", 3]})), "`points` must be a point reference");
    assert_eq!(msg(json!({"points": ["a", {"midpoint": 3}]})), "`midpoint` must be a curve id");
    assert_eq!(msg(json!({"points": [{"edge": [0, 0, 0]}, "b"]})), "`points` must be a point reference");

    // Midpoint: `a` and `b` when either is given (`null` too), else `point` and `line`.
    let p: MidpointParams = parse(json!({"a": "l1", "b": "l2", "point": 5, "line": 6})).unwrap();
    assert!(p.point.is_none() && p.line.is_none() && p.a.is_some(), "`point` and `line` are not read with `a` and `b`");
    let p: MidpointParams = parse(json!({"point": "p1", "line": "l1"})).unwrap();
    assert!(p.point.is_some() && p.line.is_some());
    let msg = |v: Value| whole_message(parse::<MidpointParams>(v).map(|_| ()).unwrap_err());
    assert_eq!(msg(json!({"a": "l1", "b": null, "point": "p1", "line": "l1"})), "`b` must be a point or line id");
    assert_eq!(msg(json!({"a": 5, "b": "l2"})), "`a` must be a point or line id");
    assert_eq!(msg(json!({"b": 5})), "`b` must be a point or line id");
    assert_eq!(msg(json!({"point": 5, "line": "l1"})), "`point` must be a point reference");
    assert_eq!(msg(json!({"point": "p1", "line": 5})), "`line` must be a curve id");

    // Symmetry: `point` when given (`null` too) and `line` is not read; else `line`.
    let p: SymmetryParams = parse(json!({"a": "p1", "b": "p2", "point": "origin", "line": 5})).unwrap();
    assert!(p.point.is_some() && p.line.is_none());
    let p: SymmetryParams = parse(json!({"a": "p1", "b": "p2", "line": "y"})).unwrap();
    assert!(p.line.is_some() && p.point.is_none());
    let msg = |v: Value| whole_message(parse::<SymmetryParams>(v).map(|_| ()).unwrap_err());
    assert_eq!(msg(json!({"a": "p1", "b": "p2", "point": null, "line": "y"})), "`point` must be a point reference");
    assert_eq!(msg(json!({"a": "p1", "b": "p2", "line": 5})), "`line` must be a curve id, or x / y for a sketch axis");
    assert_eq!(msg(json!({"a": 1, "b": "p2", "line": "y"})), "`a` must be a point or curve");
    assert_eq!(msg(json!({"a": "p1", "b": 2, "line": "y"})), "`b` must be a point or curve");

    // Fix: `entity` or `entities`, one at least.
    let p: FixParams = parse(json!({"entity": "p1"})).unwrap();
    assert_eq!(p.entity.as_deref(), Some("p1"));
    let p: FixParams = parse(json!({"entities": "p1", "fixed": false})).unwrap();
    assert_eq!((p.entities.map(|e| e.0), p.fixed), (Some(vec!["p1".into()]), Some(false)));
    let p: FixParams = parse(json!({"entities": null, "entity": "p1"})).unwrap();
    assert!(p.entities.is_none());
    let msg = |v: Value| whole_message(parse::<FixParams>(v).map(|_| ()).unwrap_err());
    assert_eq!(msg(json!({})), "`entity` or `entities` must name sketch points or curves");
    assert_eq!(msg(json!({"entities": []})), "`entity` or `entities` must name sketch points or curves");
    assert_eq!(msg(json!({"entity": 5})), "`entity` or `entities` must name sketch points or curves");
    assert_eq!(msg(json!({"entities": 5})), "`entities` must be a list of names; it is 5");
    assert_eq!(msg(json!({"entities": ["a", true]})), "`entities` must list names; the entry 2 is true");
    // A `entity` that is not text is ignored by the handler when `entities` names something;
    // the type refuses it, which is what warn and strict report.
    assert_eq!(msg(json!({"entity": 5, "entities": ["p1"]})), "`entity` must be text");
}

fn assert_clean<T: schemars::JsonSchema>(s: &mut Session, id: &str, p: Value) -> Value {
    let r = run(s, id, p.clone());
    assert!(r.get("param_warnings").is_none(), "{id} {p} warned: {r}");
    output_conforms::<T>(&r);
    r
}

#[test]
fn replies_match_the_output_schemas_and_real_calls_get_no_warnings() {
    let new = || scene(ParamCheck::Warn);

    // Horizontal / Vertical: a line, a line with a mode, two points, a line's middle, and a
    // `line` that `points` makes unread.
    let mut s = new();
    assert_clean::<ConstraintResult>(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": "l1"}));
    assert_clean::<ConstraintResult>(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": "l2", "mode": "Vertical"}));
    assert_clean::<ConstraintResult>(
        &mut s,
        "sketch.constraint.horizontal_vertical",
        json!({"points": ["l3.start", "l3.end"], "mode": "horizontal"}),
    );
    assert_clean::<ConstraintResult>(&mut s, "sketch.constraint.horizontal_vertical", json!({"points": ["c1.center", {"midpoint": "l4"}]}));
    assert_clean::<ConstraintResult>(&mut s, "sketch.constraint.horizontal_vertical", json!({"points": ["l5.start", "l5.end"], "line": 7}));
    assert_clean::<ConstraintResult>(&mut s, "sketch.constraint.horizontal_vertical", json!({"points": "not a list", "line": "l6", "mode": "x"}));

    // Coincident: two points, a point on a curve, a point on an axis, a line's middle, two lines,
    // two circles, and a pair that is joined already.
    let mut s = new();
    let r = assert_clean::<CoincidentResult>(&mut s, "sketch.constraint.coincident", json!({"a": "l1.end", "b": "l2.start"}));
    assert!(r.get("existing").is_none() && r.get("constraints").is_none(), "{r}");
    let r = assert_clean::<CoincidentResult>(&mut s, "sketch.constraint.coincident", json!({"a": "l1.end", "b": "l2.start"}));
    assert_eq!(r["existing"], true, "{r}");
    assert_clean::<CoincidentResult>(&mut s, "sketch.constraint.coincident", json!({"a": "p1", "b": "c1"}));
    assert_clean::<CoincidentResult>(&mut s, "sketch.constraint.coincident", json!({"a": "x", "b": "p2"}));
    assert_clean::<CoincidentResult>(&mut s, "sketch.constraint.coincident", json!({"a": {"midpoint": "l3"}, "b": "l4.start"}));
    assert_clean::<CoincidentResult>(&mut s, "sketch.constraint.coincident", json!({"a": "l5", "b": "l6"}));
    let r = assert_clean::<CoincidentResult>(&mut s, "sketch.constraint.coincident", json!({"a": "c3", "b": "c4"}));
    assert!(r["constraints"].as_array().is_some_and(|c| c.len() == 2) && r.get("constraint").is_some(), "{r}");

    // Tangent: two curves, and a line's end point and a circle (#12092), which adds two
    // constraints.
    let mut s = new();
    let r = assert_clean::<TangentResult>(&mut s, "sketch.constraint.tangent", json!({"a": "l1", "b": "c1"}));
    assert!(r.get("constraints").is_none(), "{r}");
    let r = assert_clean::<TangentResult>(&mut s, "sketch.constraint.tangent", json!({"a": "l3.end", "b": "c2"}));
    assert!(r["constraints"].as_array().is_some_and(|c| c.len() == 2) && r.get("constraint").is_some(), "{r}");

    // Parallel, perpendicular, collinear.
    let mut s = new();
    assert_clean::<ConstraintResult>(&mut s, "sketch.constraint.parallel", json!({"a": "l2", "b": "l3"}));
    assert_clean::<ConstraintResult>(&mut s, "sketch.constraint.parallel", json!({"a": "l4", "b": "x"}));
    assert_clean::<ConstraintResult>(&mut s, "sketch.constraint.perpendicular", json!({"a": "l5", "b": "l6"}));
    assert_clean::<ConstraintResult>(&mut s, "sketch.constraint.perpendicular", json!({"a": "y", "b": "l1", "sketch": "Sketch1"}));
    let mut s = new();
    assert_clean::<ConstraintResult>(&mut s, "sketch.constraint.collinear", json!({"a": "l1", "b": "l2"}));
    assert_clean::<ConstraintResult>(&mut s, "sketch.constraint.collinear", json!({"a": "l3", "b": "l4", "if_redundant": "refuse"}));

    // Concentric: two circles, and a circle and a point.
    let mut s = new();
    assert_clean::<ConstraintResult>(&mut s, "sketch.constraint.concentric", json!({"a": "c1", "b": "c2"}));
    assert_clean::<ConstraintResult>(&mut s, "sketch.constraint.concentric", json!({"a": "p1", "b": "c3"}));

    // Equal: two curves, and a list, with the keys that list leaves unread.
    let mut s = new();
    let r = assert_clean::<EqualResult>(&mut s, "sketch.constraint.equal", json!({"a": "l1", "b": "l2"}));
    assert!(r.get("constraint").is_some() && r.get("constraints").is_none(), "{r}");
    let r = assert_clean::<EqualResult>(&mut s, "sketch.constraint.equal", json!({"curves": ["c1", "c2", "c3"], "a": 5, "if_redundant": 5}));
    assert!(r["constraints"].as_array().is_some_and(|c| c.len() == 2) && r.get("constraint").is_none(), "{r}");
    assert_clean::<EqualResult>(&mut s, "sketch.constraint.equal", json!({"curves": ["c3", "c4"]}));

    // Midpoint: a point and a line, two lines, and the `a` and `b` form with `point` and `line`
    // left unread.
    let mut s = new();
    assert_clean::<ConstraintResult>(&mut s, "sketch.constraint.midpoint", json!({"point": "p1", "line": "l1"}));
    assert_clean::<ConstraintResult>(&mut s, "sketch.constraint.midpoint", json!({"a": "l2", "b": "l3"}));
    assert_clean::<ConstraintResult>(&mut s, "sketch.constraint.midpoint", json!({"a": "p2", "b": "l4", "point": 1, "line": 2}));

    // Symmetry: about a sketch axis, a line, a point, two lines and two circles.
    let mut s = new();
    let r = assert_clean::<SymmetryResult>(&mut s, "sketch.constraint.symmetry", json!({"a": "l2.start", "b": "l2.end", "line": "y"}));
    assert_eq!(r["constraints"].as_array().map(Vec::len), Some(1), "{r}");
    assert_clean::<SymmetryResult>(&mut s, "sketch.constraint.symmetry", json!({"a": "p1", "b": "p2", "line": "l5"}));
    assert_clean::<SymmetryResult>(&mut s, "sketch.constraint.symmetry", json!({"a": "l3", "b": "l4", "point": "origin"}));
    assert_clean::<SymmetryResult>(&mut s, "sketch.constraint.symmetry", json!({"a": "c1", "b": "c2", "line": "y"}));
    assert_clean::<SymmetryResult>(&mut s, "sketch.constraint.symmetry", json!({"a": "l5", "b": "l6", "line": "l1"}));

    // Fix: one entity, a list, `fixed`, and a release.
    let mut s = new();
    let r = assert_clean::<FixResult>(&mut s, "sketch.constraint.fix", json!({"entity": "p1"}));
    assert_eq!(r["result"], json!({"fixed": true, "changed": 1}), "{r}");
    assert_clean::<FixResult>(&mut s, "sketch.constraint.fix", json!({"entities": ["l1", "c1"], "fixed": true}));
    assert_clean::<FixResult>(&mut s, "sketch.constraint.fix", json!({"entity": "p2", "entities": "l2"}));
    let r = assert_clean::<FixResult>(&mut s, "sketch.constraint.fix", json!({"entities": ["p1", "l1"], "fixed": false, "sketch": "Sketch1"}));
    assert_eq!(r["result"]["fixed"], false, "{r}");
}

/// A constraint the sketch already determines is kept as a redundant one, and the reply says what
/// determines it, unless `if_redundant` is `refuse`.
#[test]
fn a_redundant_constraint_is_reported() {
    let mut s = scene(ParamCheck::Warn);
    assert_clean::<ConstraintResult>(&mut s, "sketch.constraint.parallel", json!({"a": "l1", "b": "l2"}));
    let r = assert_clean::<ConstraintResult>(&mut s, "sketch.constraint.parallel", json!({"a": "l1", "b": "l2"}));
    assert_eq!(r["redundant"], true, "{r}");
    assert!(r["determined_by"].is_array(), "{r}");
    let e = s.execute("sketch.constraint.parallel", &json!({"a": "l1", "b": "l2", "if_redundant": "Refuse"})).unwrap_err().to_string();
    assert!(e.contains("is already determined by"), "{e}");
}

/// A key a call misspells is reported in warn, and the reply still matches its schema.
#[test]
fn a_misspelt_key_is_reported_in_warn() {
    let cases: [(&str, Value, &str); 11] = [
        ("sketch.constraint.coincident", json!({"a": "l1.end", "b": "l2.start", "if_redundent": "refuse"}), "if_redundant"),
        ("sketch.constraint.collinear", json!({"a": "l1", "b": "l2", "skech": "Sketch1"}), "sketch"),
        ("sketch.constraint.concentric", json!({"a": "c1", "b": "c2", "bb": "x"}), "b"),
        ("sketch.constraint.equal", json!({"curvs": ["c1", "c2"], "a": "l1", "b": "l2"}), "curves"),
        ("sketch.constraint.fix", json!({"entites": ["p1"], "entity": "p1"}), "entities"),
        ("sketch.constraint.horizontal_vertical", json!({"line": "l1", "mod": "vertical"}), "mode"),
        ("sketch.constraint.midpoint", json!({"point": "p1", "lines": "l1", "line": "l1"}), "line"),
        ("sketch.constraint.parallel", json!({"a": "l1", "b": "l2", "sketch_": "Sketch1"}), "sketch"),
        ("sketch.constraint.perpendicular", json!({"a": "l1", "b": "l2", "if_redundnt": "refuse"}), "if_redundant"),
        ("sketch.constraint.symmetry", json!({"a": "p1", "b": "p2", "lin": "y", "line": "y"}), "line"),
        ("sketch.constraint.tangent", json!({"a": "l1", "b": "c1", "if_redundant ": "refuse"}), "if_redundant"),
    ];
    for (id, p, nearest) in cases {
        let mut s = scene(ParamCheck::Warn);
        let r = run(&mut s, id, p.clone());
        let warnings = r["param_warnings"].as_array().unwrap_or_else(|| panic!("{id} {p}: no warning in {r}"));
        assert_eq!(warnings.len(), 1, "{id}: {warnings:?}");
        let w = warnings[0].as_str().unwrap_or_default();
        assert!(w.starts_with("unknown key `") && w.contains(&format!("did you mean `{nearest}`?")), "{id}: {w}");
        // And the check names it, without running anything.
        assert_eq!(issues(id, &p).len(), 1, "{id}");
        match id {
            "sketch.constraint.fix" => output_conforms::<FixResult>(&r),
            "sketch.constraint.symmetry" => output_conforms::<SymmetryResult>(&r),
            "sketch.constraint.coincident" => output_conforms::<CoincidentResult>(&r),
            "sketch.constraint.equal" => output_conforms::<EqualResult>(&r),
            _ => output_conforms::<ConstraintResult>(&r),
        }
        // In strict the call is refused, and the sketch stays as it was.
        let mut s = scene(ParamCheck::Strict);
        let before = s.doc.clone();
        let e = s.execute(id, &p).unwrap_err().to_string();
        assert!(e.contains("unknown key"), "{id}: {e}");
        assert!(std::sync::Arc::ptr_eq(&before, &s.doc) || *before == *s.doc, "{id}");
    }
}

/// With the check off, the replies are the ones the commands gave before: the same keys, and no
/// `param_warnings`.
#[test]
fn off_leaves_the_replies_as_they_were() {
    let mut s = scene(ParamCheck::Off);
    let r = run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": "l1", "if_redundnt": 1}));
    let mut keys: Vec<_> = r.as_object().into_iter().flat_map(|o| o.keys().cloned()).collect();
    keys.sort();
    assert_eq!(keys, ["constraint", "created", "sketch"], "{r}");
    let r = run(&mut s, "sketch.constraint.fix", json!({"entity": "p1"}));
    let mut keys: Vec<_> = r.as_object().into_iter().flat_map(|o| o.keys().cloned()).collect();
    keys.sort();
    assert_eq!(keys, ["created", "result", "sketch"], "{r}");
}

/// A model edge or vertex is an operand: projected into the sketch first, and the call gets no
/// warning.
#[test]
fn model_edges_and_vertices_are_operands() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 50]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Base", "distance": "20 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": "20 mm", "name": "Top"}));
    run(&mut s, "sketch.line", json!({"points": [[10, 10], [40, 25]], "ids": ["l1"]}));
    let r = assert_clean::<ConstraintResult>(&mut s, "sketch.constraint.parallel", json!({"a": "l1", "b": {"edge": [100, 25, 20], "body": "Body1"}}));
    assert_eq!(r["created"]["curves"].as_array().map(Vec::len), Some(1), "the projected edge is reported: {r}");
    assert_clean::<CoincidentResult>(&mut s, "sketch.constraint.coincident", json!({"a": "l1.end", "b": {"vertex": [100, 0, 20]}}));
}
