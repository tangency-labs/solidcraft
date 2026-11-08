use serde_json::{Value, json};

use super::{DimensionResult, SketchDimensionParams};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, issues, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<SketchDimensionParams>(),
        &["driven", "entities", "half_axes", "if_redundant", "orientation", "previous_axis", "scale_sketch", "sketch", "text_at", "type", "value"],
    );
    // Only `entities` is required, and the type leaves it to the handler's message.
    let schema = input_schema_for::<SketchDimensionParams>();
    assert_eq!(schema.as_value()["required"], json!(["entities"]));
    assert!(serde_json::from_value::<SketchDimensionParams>(json!({})).is_ok());
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("sketch.dimension", json!({})),
        ("sketch.dimension", json!({"entities": null})),
        ("sketch.dimension", json!({"entities": []})),
        ("sketch.dimension", json!({"entities": ["l1", "l2", "l3"]})),
        ("sketch.dimension", json!({"entities": 5})),
        ("sketch.dimension", json!({"entities": true})),
        ("sketch.dimension", json!({"entities": {"edge": [0, 0, 0]}})),
        ("sketch.dimension", json!({"entities": [1]})),
        ("sketch.dimension", json!({"entities": ["l1", null]})),
        ("sketch.dimension", json!({"entities": [{"foo": 1}]})),
        ("sketch.dimension", json!({"entities": [["l1"]]})),
        ("sketch.dimension", json!({"entities": vec!["l1"; 1001]})),
        ("sketch.dimension", json!({"entities": [{"edge": 5}]})),
        ("sketch.dimension", json!({"entities": [{"vertex": [1, 2]}]})),
        // The handler looks the sketch up before it reads a model reference's form.
        ("sketch.dimension", json!({"entities": [{"edge": 5}], "sketch": true})),
        ("sketch.dimension", json!({"entities": ["l1"], "half_axes": "no"})),
        ("sketch.dimension", json!({"entities": ["l1"], "half_axes": 1})),
        ("sketch.dimension", json!({"entities": ["l1"], "half_axes": []})),
        ("sketch.dimension", json!({"entities": ["l1"], "sketch": true})),
        ("sketch.dimension", json!({"entities": ["l1"], "sketch": -1})),
        ("sketch.dimension", json!({"entities": ["l1"], "sketch": 2.5})),
        ("sketch.dimension", json!({"entities": ["l1"], "sketch": [1]})),
    ]);
}

/// A session that is editing a sketch "Plan" with two lines, a circle and two more lines (l3, l4).
fn editing(level: ParamCheck) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    for (id, p) in [
        ("sketch.create", json!({"plane": "XY", "name": "Plan"})),
        ("sketch.line", json!({"points": [[0, 0], [10, 0.5]]})),
        ("sketch.line", json!({"points": [[0, 20], [10, 30]]})),
        ("sketch.circle.center", json!({"center": [50, 0], "radius": 5})),
        ("sketch.line", json!({"points": [[0, 40], [10, 40]]})),
        ("sketch.line", json!({"points": [[0, 40], [5, 50]]})),
    ] {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    }
    s
}

/// The same refusals in a sketch being edited, where the handler goes on to read the sketch and
/// its entities, and the ones the design settles: strict says what the handler says.
#[test]
fn strict_keeps_todays_errors_in_a_sketch() {
    let cases = [
        json!({}),
        json!({"entities": null}),
        json!({"entities": []}),
        json!({"entities": ["l1", "l2", "c1"]}),
        json!({"entities": 5}),
        json!({"entities": [1]}),
        json!({"entities": ["l1", {"x": 1}]}),
        json!({"entities": vec!["l1"; 1001]}),
        json!({"entities": [{"edge": [1, 2]}]}),
        json!({"entities": [{"edge": 5}], "sketch": "Plan"}),
        json!({"entities": ["l1"], "half_axes": "no"}),
        json!({"entities": ["l1"], "half_axes": 0, "sketch": true}),
        json!({"entities": ["l1"], "sketch": true}),
        json!({"entities": ["l1"], "sketch": -1}),
        json!({"entities": ["l1"], "sketch": 1.5}),
        // What the design settles: the contract passes these on.
        json!({"entities": ["nope"]}),
        json!({"entities": ["l1"], "sketch": "Missing"}),
        json!({"entities": ["l1", "c1", "l2"]}),
        json!({"entities": ["l1"], "value": "w ="}),
        json!({"entities": ["l1"], "value": "w = 5", "driven": true}),
        json!({"entities": ["l1"], "value": "w = 5 +"}),
        json!({"entities": ["c1", "c1"]}),
    ];
    for (index, p) in cases.iter().enumerate() {
        let error = |level: ParamCheck| editing(level).execute("sketch.dimension", p).err().map(|e| e.to_string());
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{index} {p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{index} {p}");
    }
}

/// What strict refuses on purpose: a value of the wrong type that the handler reads as if the key
/// were left out. Warn names each one and the call still runs.
#[test]
fn strict_refuses_what_the_handler_ignores() {
    let cases = [
        (json!({"entities": ["l1"], "type": 5}), "`type` must be text"),
        (json!({"entities": ["l1"], "orientation": true}), "`orientation` must be text"),
        (json!({"entities": ["l1"], "value": true}), "`value` must be a number or an expression"),
        (json!({"entities": ["l1"], "value": ""}), "`value` must be a number or an expression"),
        (json!({"entities": ["l1"], "value": 1e12}), "`value` must be a number between -1e9 and 1e9"),
        (json!({"entities": ["l1"], "text_at": [1]}), "`text_at` must be a point [x, y] of numbers"),
        (json!({"entities": ["l1"], "driven": "yes"}), "`driven` must be true or false"),
        (json!({"entities": ["l1"], "scale_sketch": 1}), "`scale_sketch` must be true or false"),
        (json!({"entities": ["l1"], "previous_axis": 1}), "`previous_axis` must be text"),
        (json!({"entities": ["l1"], "if_redundant": 5}), "`if_redundant` must be text"),
        (json!({"entities": ["l1"], "drivn": true}), "unknown key `drivn`; did you mean `driven`?"),
    ];
    for (p, message) in cases {
        let found = issues("sketch.dimension", &p).into_iter().map(|i| i.message).collect::<Vec<_>>();
        assert_eq!(found, [message], "{p}");
        let strict = editing(ParamCheck::Strict).execute("sketch.dimension", &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(strict.contains(message), "{p}: {strict}");
        let off = editing(ParamCheck::Off).execute("sketch.dimension", &p);
        assert!(off.is_ok(), "{p} works with the check off: {off:?}");
    }
}

fn parses(v: Value) -> bool {
    serde_json::from_value::<SketchDimensionParams>(v).is_ok()
}

/// A driven dimension has no value, no scale and nothing it could be redundant with: those keys
/// are not read for it (the handler never looks at them), and are for a driving one. The other
/// keys are read either way.
#[test]
fn a_driven_dimension_reads_only_what_the_handler_reads() {
    for key in [("value", json!(true)), ("scale_sketch", json!("yes")), ("if_redundant", json!(7)), ("value", json!(""))] {
        let (k, v) = key;
        let driving = json!({"entities": ["l1"], k: v.clone()});
        let driven = json!({"entities": ["l1"], "driven": true, k: v.clone()});
        assert!(!parses(driving), "{k} {v} is read for a driving dimension");
        assert!(parses(driven), "{k} {v} is not read for a driven one");
        assert!(issues("sketch.dimension", &json!({"entities": ["l1"], "driven": true, k: v})).is_empty());
    }
    // `driven: false` and `null` are driving.
    for driven in [json!(false), json!(null)] {
        assert!(!parses(json!({"entities": ["l1"], "driven": driven, "value": true})));
    }
    // Read in both: the type, where the text goes, the ellipse keys and the sketch.
    for key in [
        ("type", json!(1)),
        ("orientation", json!(1)),
        ("text_at", json!("x")),
        ("previous_axis", json!([])),
        ("half_axes", json!(1)),
        ("sketch", json!(true)),
    ] {
        let (k, v) = key;
        assert!(!parses(json!({"entities": ["l1"], "driven": true, k: v.clone()})), "{k} {v} when driven");
        assert!(!parses(json!({"entities": ["l1"], k: v.clone()})), "{k} {v}");
    }
    // The handler's own refusal of a definition on a driven dimension is left to it.
    assert!(parses(json!({"entities": ["l1"], "driven": true, "value": "w = 5"})));
}

/// Each key reads as the handler reads it: `null` is left out, entities are names or model
/// references, a value is a number or an expression, a sketch an id or a name.
#[test]
fn the_types_read_each_key_like_the_handler() {
    assert!(parses(json!({"entities": null, "sketch": null, "type": null, "orientation": null, "value": null, "text_at": null})));
    assert!(parses(json!({"driven": null, "scale_sketch": null, "previous_axis": null, "half_axes": null, "if_redundant": null})));
    for entities in [
        json!("l1"),
        json!(["l1"]),
        json!(["l1", "p2"]),
        json!(["l1.start", "mid:l2"]),
        json!(["x", "c1.center"]),
        json!(["p1", {"edge": [1, 2, 3], "body": "Flange"}]),
        json!([{"vertex": [1, 2, 3]}, {"edge": {"x": 1, "y": 2, "z": 3}}]),
        // Forms the handler passes on to the design: a body, a sketch or a plane in place of a point.
        json!([{"edge": 5, "body": "B"}]),
    ] {
        assert!(parses(json!({"entities": entities})), "{entities}");
    }
    for entities in [json!(5), json!([5]), json!([null]), json!([{"face": [1, 2, 3]}]), json!([{}]), json!({"edge": [1, 2, 3]}), json!([["a"]])] {
        assert!(!parses(json!({"entities": entities})), "{entities}");
    }
    // The count is the handler's: one or two.
    assert!(!parses(json!({"entities": []})) && !parses(json!({"entities": ["a", "b", "c"]})));
    // A value: a number, or an expression of up to 4096 characters, including `name = expression`.
    for value in [json!(2), json!(-1.5), json!("width / 2"), json!("50 mm"), json!("w = 40"), json!("#w = 4 mm")] {
        assert!(parses(json!({"entities": ["l1"], "value": value})), "{value}");
    }
    for value in [json!(true), json!([1]), json!({}), json!(""), json!("   "), json!("x".repeat(4097))] {
        assert!(!parses(json!({"entities": ["l1"], "value": value})), "{value}");
    }
    // Points as `params::vec2` reads them.
    assert!(parses(json!({"entities": ["l1"], "text_at": [1, 2]})) && parses(json!({"entities": ["l1"], "text_at": {"x": 1, "y": 2}})));
    assert!(!parses(json!({"entities": ["l1"], "text_at": [1, 2, 3]})) && !parses(json!({"entities": ["l1"], "text_at": [1, "2"]})));
    // The type is text of any content: an unknown word is never refused (the handler measures the
    // entities' default for it).
    assert!(parses(json!({"entities": ["l1"], "type": "ARC_LENGTH", "orientation": "Horizontal", "previous_axis": "major"})));
    assert!(parses(json!({"entities": ["l1"], "type": "no such type", "if_redundant": "refuse"})));
    // A sketch: a whole number or text.
    for sketch in [json!(3), json!("Plan"), json!("7")] {
        assert!(parses(json!({"entities": ["l1"], "sketch": sketch})), "{sketch}");
    }
    for sketch in [json!(-1), json!(1.5), json!(false), json!([1])] {
        assert!(!parses(json!({"entities": ["l1"], "sketch": sketch})), "{sketch}");
    }
}

/// The schema says what the handler takes: `entities` is required, and a driven dimension may
/// carry the keys that only a driving one reads.
#[test]
fn the_schema_takes_what_the_handler_reads() {
    let schema = input_schema_for::<SketchDimensionParams>();
    let schema = schema.as_value();
    for (p, takes) in [
        (json!({"entities": ["l1"]}), true),
        (json!({"entities": "l1", "type": "radius", "value": "r = 5", "text_at": [1, 2], "sketch": 3}), true),
        (json!({"entities": ["a", {"edge": [1, 2, 3], "body": "B"}], "driven": true, "if_redundant": "driven", "scale_sketch": true}), true),
        (json!({"entities": ["e"], "half_axes": false, "previous_axis": "minor", "orientation": "vertical"}), true),
        (json!({}), false),
        (json!({"value": 3}), false),
        (json!({"entities": []}), false),
        (json!({"entities": ["a", "b", "c"]}), false),
        (json!({"entities": ["a"], "value": true}), false),
        (json!({"entities": ["a"], "driven": 1}), false),
        (json!({"entities": ["a"], "colour": 1}), false),
    ] {
        assert_eq!(conforms(schema, schema, &p), takes, "{p}");
    }
}

fn matches(r: &Value) {
    let schema = output_schema_for::<DimensionResult>();
    let schema = schema.as_value();
    assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
    assert!(r.get("param_warnings").is_none(), "{r}");
}

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// Each form of the command, run in Warn: the reply has no warning and matches its output schema.
#[test]
fn replies_match_the_output_schema() {
    let mut s = editing(ParamCheck::Warn);
    let dim = |s: &mut Session, p: Value| {
        let r = run(s, "sketch.dimension", p);
        matches(&r);
        r
    };
    // A length, with a value, and a distance chosen by `type` and by `orientation`.
    let r = dim(&mut s, json!({"entities": ["l1"], "value": 12}));
    assert!(r["type"] == "LinearDimension" && r["unit"] == "mm" && r["expression"] == "12" && r.get("driven").is_none(), "{r}");
    assert!(r["param"].is_string() && r["sketch"]["solved"] == true, "{r}");
    let r = dim(&mut s, json!({"entities": ["l2"], "type": "horizontal", "value": "8 mm", "text_at": [5, 35]}));
    assert!((r["measured"].as_f64().unwrap_or_default() - 8.0).abs() < 1e-6, "{r}");
    dim(&mut s, json!({"entities": ["l3.start", "l3.end"], "orientation": "horizontal"}));
    dim(&mut s, json!({"entities": ["l4.start", "l4.end"], "type": "vertical", "value": null, "sketch": "Plan"}));
    // A circle's diameter, and a definition of a parameter by the value.
    let r = dim(&mut s, json!({"entities": ["c1"], "value": "d = 12"}));
    assert!(r["type"] == "DiameterDimension" && r["defined"] == "d" && r["expression"] == "d", "{r}");
    // An angle between two lines, in degrees.
    let r = dim(&mut s, json!({"entities": ["l3", "l4"], "type": "angle", "value": "45 deg"}));
    assert!(r["type"] == "AngularDimension" && r["unit"] == "deg", "{r}");
    // A driven dimension: no parameter, the keys only a driving one reads are not read.
    let r = dim(&mut s, json!({"entities": ["l2"], "driven": true, "value": true, "scale_sketch": "yes", "if_redundant": 3, "type": "length"}));
    assert!(r["param"].is_null() && r["driven"] == true && r.get("expression").is_none(), "{r}");
    // One the sketch already determines, placed as driven instead.
    let r = dim(&mut s, json!({"entities": ["l1.start", "l1.end"], "if_redundant": "driven"}));
    assert!(r["redundant"] == true && r["driven"] == true && r["determined_by"].as_array().is_some_and(|a| !a.is_empty()), "{r}");
    let r = s.execute("sketch.dimension", &json!({"entities": ["l1"], "value": 13}));
    assert!(r.as_ref().is_err_and(|e| e.to_string().contains("over-constrain")), "{r:?}");
    // The midpoints of two lines.
    let mut s = editing(ParamCheck::Warn);
    dim(&mut s, json!({"entities": ["mid:l1", "mid:l2"], "text_at": [30, 15], "type": "auto"}));
    // Scaling a sketch's first dimension.
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 5}));
    let r = dim(&mut s, json!({"entities": ["c1"], "value": "30 mm", "scale_sketch": true}));
    assert!(r["scaled"].as_f64().is_some_and(|f| f > 1.0), "{r}");
    // An ellipse's half-axis.
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let e = run(&mut s, "sketch.ellipse", json!({"center": [0, 0], "major": [20, 0], "minor_radius": 8}));
    let e = e["curves"][0].as_str().unwrap_or_default().to_string();
    let r = dim(&mut s, json!({"entities": [e], "text_at": [3, 16], "previous_axis": "Minor", "half_axes": true}));
    assert!(r["axis"] == "major", "{r}");
    let r = dim(&mut s, json!({"entities": [e], "driven": true, "half_axes": false}));
    assert!(r["driven"] == true && r.get("axis").is_none(), "{r}");
}

/// A dimension that measures a model edge: the edge is projected first, and the call is read in
/// Warn without a warning.
#[test]
fn a_model_edge_is_an_entity() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 50]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"sketch": "Base", "body_name": "Flange"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Near"}));
    let point = run(&mut s, "sketch.point", json!({"point": [50, 25]}))["point"].as_str().unwrap_or_default().to_string();
    let r = run(&mut s, "sketch.dimension", json!({"entities": [point, {"edge": [50, 0, 0], "body": "Flange"}], "sketch": "Near"}));
    matches(&r);
    assert!((r["measured"].as_f64().unwrap_or_default() - 25.0).abs() < 1e-6, "{r}");
    // A model reference that is no form of one is refused with the handler's words, in strict too.
    let mut error = |level: ParamCheck| {
        s.param_check = level;
        s.execute("sketch.dimension", &json!({"entities": [point, {"edge": 5}]})).err().map(|e| e.to_string())
    };
    let off = error(ParamCheck::Off);
    assert!(off.as_deref().is_some_and(|e| e.contains("cannot project")), "{off:?}");
    assert_eq!(error(ParamCheck::Strict), off);
}

/// A misspelt key still runs in Warn, and says so, with a reply that still matches its schema.
#[test]
fn a_misspelt_key_is_reported_in_warn() {
    let mut s = editing(ParamCheck::Warn);
    let r = run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "valeu": 12}));
    let warnings = r["param_warnings"].as_array().cloned().unwrap_or_default();
    assert_eq!(warnings, [json!("unknown key `valeu`")], "{r}");
    let schema = output_schema_for::<DimensionResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &r), "{r}");
    let mut s = editing(ParamCheck::Strict);
    let e = s.execute("sketch.dimension", &json!({"entities": ["l1"], "text_t": [1, 1]})).err().map(|e| e.to_string());
    assert!(e.is_some_and(|e| e.contains("unknown key `text_t`")));
}

/// The reply serializes as the handler's `json!` literal did: what is not there is left out.
#[test]
fn the_reply_leaves_out_what_is_absent() {
    let mut s = editing(ParamCheck::Off);
    let keys = |r: &Value| {
        let mut k: Vec<&str> = r.as_object().map(|o| o.keys().map(String::as_str).collect()).unwrap_or_default();
        k.sort_unstable();
        k.join(",")
    };
    let driving = run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": 12}));
    assert_eq!(keys(&driving), "created,expression,measured,param,sketch,type,unit", "{driving}");
    let driven = run(&mut s, "sketch.dimension", json!({"entities": ["l2"], "driven": true}));
    assert_eq!(keys(&driven), "created,driven,measured,param,sketch,type,unit", "{driven}");
    let defined = run(&mut s, "sketch.dimension", json!({"entities": ["c1"], "value": "d = 12"}));
    assert_eq!(keys(&defined), "created,defined,expression,measured,param,sketch,type,unit", "{defined}");
}
