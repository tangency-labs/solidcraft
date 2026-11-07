use serde_json::{Value, json};

use super::{
    AutoConstrainDatumParams, AutoConstrainParams, AutoConstrainResult, ConstrainerParams, ConstrainerResult, ConstraintAdded, CurvatureParams,
    FinishAutoConstrainParams, GlyphsParams, GlyphsResult, PolygonParams, PolygonResult, SnapParams, SnapResult, ToggleDiameterParams,
    ToggleDiameterResult, ToggleDrivenParams, ToggleDrivenResult,
};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

const AUTO_KEYS: [&str; 5] = ["angle_tolerance", "datum", "dimensions", "sketch", "tolerance"];

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<ToggleDiameterParams>(), &["constraint", "diameter", "sketch"]);
    assert_keys(input_schema_for::<CurvatureParams>(), &["a", "b", "if_redundant", "sketch"]);
    assert_keys(input_schema_for::<PolygonParams>(), &["lines", "sketch"]);
    assert_keys(input_schema_for::<ConstrainerParams>(), &["entities", "if_redundant", "sketch"]);
    assert_keys(input_schema_for::<AutoConstrainParams>(), &AUTO_KEYS);
    assert_keys(input_schema_for::<AutoConstrainDatumParams>(), &AUTO_KEYS);
    assert_keys(input_schema_for::<FinishAutoConstrainParams>(), &AUTO_KEYS);
    assert_keys(input_schema_for::<ToggleDrivenParams>(), &["constraint", "driven", "sketch"]);
    // Snap Inference works in the sketch being edited: it reads no `sketch`.
    assert_keys(input_schema_for::<GlyphsParams>(), &["sketch"]);
    assert_keys(input_schema_for::<SnapParams>(), &["at", "from", "radius"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("sketch.toggle_diameter", json!({"constraint": 5})),
        ("sketch.toggle_diameter", json!({"constraint": null})),
        ("sketch.toggle_diameter", json!({"constraint": "c1"})),
        ("sketch.toggle_diameter", json!({"constraint": "c1", "diameter": true})),
        // The keys these handlers read before they need a sketch.
        ("sketch.constrainer", json!({"entities": 5})),
        ("sketch.constrainer", json!({"entities": [1]})),
        ("sketch.constrainer", json!({"entities": ["a", 2]})),
        ("sketch.constrainer", json!({})),
        ("sketch.constrainer", json!({"entities": null})),
        ("sketch.constraint.polygon", json!({"lines": true})),
        ("sketch.constraint.polygon", json!({"lines": [{}]})),
        ("sketch.constraint.polygon", json!({})),
        ("sketch.constraint.polygon", json!({"lines": ["l1", "l2"]})),
        ("sketch.toggle_driven", json!({})),
        ("sketch.toggle_driven", json!({"constraint": 5})),
        ("sketch.toggle_driven", json!({"constraint": null})),
        ("sketch.toggle_driven", json!({"constraint": ["k1"]})),
        ("sketch.snap", json!({})),
        ("sketch.snap", json!({"at": 5})),
        ("sketch.snap", json!({"at": [1]})),
        ("sketch.snap", json!({"at": null})),
        ("sketch.snap", json!({"at": [1, "2"]})),
        ("sketch.snap", json!({"at": [1, 2]})),
        ("sketch.glyphs", json!({})),
        ("sketch.auto_constrain", json!({})),
    ]);
}

/// A session that is editing a sketch with a line and a circle in it.
fn editing(level: ParamCheck) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    for (id, p) in [
        ("sketch.create", json!({"plane": "XY"})),
        ("sketch.line", json!({"points": [[0, 0], [10, 0.5]]})),
        ("sketch.circle.center", json!({"center": [50, 0], "radius": 5})),
    ] {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    }
    s
}

/// The same refusals in a sketch being edited, where the handlers go on to read the sketch, the
/// curves and the points: strict says what the handler says.
#[test]
fn strict_keeps_todays_errors_in_a_sketch() {
    let cases = [
        ("sketch.constraint.curvature", json!({})),
        ("sketch.constraint.curvature", json!({"a": 5, "b": "c1"})),
        ("sketch.constraint.curvature", json!({"a": "c1"})),
        ("sketch.constraint.curvature", json!({"a": "c1", "b": null})),
        ("sketch.constraint.curvature", json!({"a": "c1", "b": ["c2"]})),
        ("sketch.constraint.curvature", json!({"a": "c1", "b": "c2", "sketch": true})),
        ("sketch.constraint.curvature", json!({"a": "c1", "b": "c2", "sketch": -1})),
        ("sketch.constraint.curvature", json!({"a": "c1", "b": "c2", "sketch": 1.5})),
        ("sketch.constraint.curvature", json!({"a": "c1", "b": "c2", "sketch": [1]})),
        ("sketch.constrainer", json!({"entities": ["c1"], "sketch": {}})),
        ("sketch.constraint.polygon", json!({"lines": ["c1", "c2", "c3"], "sketch": false})),
        ("sketch.toggle_driven", json!({"constraint": "k1", "sketch": -3})),
        ("sketch.glyphs", json!({"sketch": true})),
        ("sketch.glyphs", json!({"sketch": 2.5})),
        ("sketch.auto_constrain", json!({"sketch": ["a"]})),
        ("sketch.auto_constrain_from_datum", json!({"sketch": -1})),
        ("sketch.finish_auto_constrain", json!({"sketch": {}})),
    ];
    for (index, (id, p)) in cases.iter().enumerate() {
        let error = |level: ParamCheck| editing(level).execute(id, p).err().map(|e| e.to_string());
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{index} {id} {p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{index} {id} {p}");
    }
}

/// What strict adds on purpose: a value of the wrong type the handlers read as if the key were
/// left out (a wrong-typed option or `from`).
#[test]
fn strict_refuses_options_the_handlers_ignore() {
    let mut s = editing(ParamCheck::Strict);
    let line = s.execute("sketch.constrainer", &json!({"entities": ["c1"], "if_redundant": 5}));
    assert!(line.is_err_and(|e| e.to_string().contains("`if_redundant` must be text")));
    for (id, p) in [
        ("sketch.auto_constrain", json!({"tolerance": "0.1"})),
        ("sketch.auto_constrain", json!({"angle_tolerance": true})),
        ("sketch.auto_constrain", json!({"dimensions": "no"})),
        ("sketch.auto_constrain", json!({"datum": 3})),
        ("sketch.snap", json!({"at": [1, 1], "from": "x"})),
        ("sketch.snap", json!({"at": [1, 1], "radius": "1"})),
    ] {
        let e = s.execute(id, &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains("invalid parameters"), "{id} {p}: {e}");
        s.param_check = ParamCheck::Off;
        assert!(s.execute(id, &p).is_ok(), "{id} {p} works with the check off");
        s.param_check = ParamCheck::Strict;
    }
}

/// The schemas take what the handlers read: the keys they refuse when left out are required.
#[test]
fn the_schemas_take_what_the_handlers_read() {
    let check = |schema: schemars::Schema, cases: &[(Value, bool)]| {
        let schema = schema.as_value().clone();
        for (p, takes) in cases {
            assert_eq!(conforms(&schema, &schema, p), *takes, "{p}");
        }
    };
    check(
        input_schema_for::<CurvatureParams>(),
        &[
            (json!({"a": "c1", "b": "c2"}), true),
            (json!({"a": "c1", "b": "c2", "sketch": 3, "if_redundant": "refuse"}), true),
            (json!({"a": "c1"}), false),
            (json!({"b": "c1"}), false),
            (json!({"a": 1, "b": "c2"}), false),
            (json!({"a": "c1", "b": "c2", "sketch": -1}), false),
            (json!({"a": "c1", "b": "c2", "from": 1}), false),
        ],
    );
    check(
        input_schema_for::<PolygonParams>(),
        &[(json!({"lines": ["l1", "l2", "l3"]}), true), (json!({"lines": "l1", "sketch": "Sketch1"}), true), (json!({}), false)],
    );
    check(
        input_schema_for::<ConstrainerParams>(),
        &[(json!({"entities": ["l1"]}), true), (json!({"entities": "p1"}), true), (json!({}), false), (json!({"entities": [1]}), false)],
    );
    check(
        input_schema_for::<ToggleDrivenParams>(),
        &[(json!({"constraint": "k1"}), true), (json!({"constraint": "k1", "driven": false}), true), (json!({"driven": true}), false)],
    );
    check(
        input_schema_for::<SnapParams>(),
        &[
            (json!({"at": [1, 2]}), true),
            (json!({"at": {"x": 1, "y": 2}, "from": [0, 0], "radius": 2}), true),
            (json!({}), false),
            (json!({"at": [1, 2, 3]}), false),
            (json!({"at": [1, 2], "sketch": 1}), false),
        ],
    );
    for schema in
        [input_schema_for::<AutoConstrainParams>(), input_schema_for::<AutoConstrainDatumParams>(), input_schema_for::<FinishAutoConstrainParams>()]
    {
        check(
            schema,
            &[
                (json!({}), true),
                (json!({"tolerance": 0.1, "angle_tolerance": 2, "dimensions": false, "datum": "p1", "sketch": 4}), true),
                (json!({"tolerance": "x"}), false),
                (json!({"dimension": true}), false),
            ],
        );
    }
    check(input_schema_for::<GlyphsParams>(), &[(json!({}), true), (json!({"sketch": "S"}), true), (json!({"curve": 1}), false)]);
}

/// Each key reads as its handler reads it: `null` is left out, a number is a whole sketch id,
/// and every key the handler takes in only some cases (`if_redundant`, `datum`) is still read.
#[test]
fn the_types_read_each_key_like_the_handlers() {
    fn parses<T: serde::de::DeserializeOwned>(v: Value) -> bool {
        serde_json::from_value::<T>(v).is_ok()
    }
    // `null` is a key left out, as every reader reads it.
    assert!(parses::<CurvatureParams>(json!({"a": null, "b": null, "sketch": null, "if_redundant": null})));
    assert!(parses::<PolygonParams>(json!({"lines": null, "sketch": null})));
    assert!(parses::<ConstrainerParams>(json!({"entities": null})));
    assert!(parses::<ToggleDrivenParams>(json!({"constraint": null, "driven": null})));
    assert!(parses::<SnapParams>(json!({"at": null, "from": null, "radius": null})));
    assert!(parses::<GlyphsParams>(json!({"sketch": null})));
    // A key left out is the handler's to refuse, with its own message.
    assert!(parses::<CurvatureParams>(json!({})) && parses::<PolygonParams>(json!({})) && parses::<SnapParams>(json!({})));
    // `sketch`: a whole number or text.
    for ok in [json!(3), json!("Sketch1"), json!("7")] {
        assert!(parses::<GlyphsParams>(json!({"sketch": ok})), "{ok}");
    }
    for bad in [json!(-1), json!(2.5), json!(true), json!([1]), json!({})] {
        assert!(!parses::<GlyphsParams>(json!({"sketch": bad})), "{bad}");
    }
    // Lists of names, as `string_list` reads them.
    assert!(parses::<PolygonParams>(json!({"lines": "l1"})) && parses::<PolygonParams>(json!({"lines": ["l1", "l2"]})));
    assert!(!parses::<PolygonParams>(json!({"lines": [1]})) && !parses::<ConstrainerParams>(json!({"entities": 5})));
    // Points: `[x, y]` or `{x, y}`.
    assert!(parses::<SnapParams>(json!({"at": [1, 2]})) && parses::<SnapParams>(json!({"at": {"x": 1, "y": 2}, "from": {"x": 0, "y": 0}})));
    assert!(!parses::<SnapParams>(json!({"at": [1, 2, 3]})) && !parses::<SnapParams>(json!({"at": [1, 1], "from": [1]})));
    // Numbers the handlers read as the default (zero, negative) are still numbers.
    assert!(
        parses::<SnapParams>(json!({"at": [1, 2], "radius": 0})) && parses::<AutoConstrainParams>(json!({"tolerance": -1, "angle_tolerance": 0}))
    );
    // `datum` and the tolerances are read by all three Auto Constrain commands.
    let p = json!({"datum": "p1", "tolerance": 1, "angle_tolerance": 2, "dimensions": false});
    assert!(parses::<AutoConstrainParams>(p.clone()) && parses::<AutoConstrainDatumParams>(p.clone()) && parses::<FinishAutoConstrainParams>(p));
}

#[test]
fn toggle_diameter_reply_matches_its_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let run = |s: &mut Session, id: &str, p: serde_json::Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let c = run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10}));
    let circle = c["curves"][0].as_str().unwrap_or_else(|| panic!("{c}")).to_string();
    let d = run(&mut s, "sketch.dimension", json!({"entities": [circle]}));
    let g = run(&mut s, "sketch.glyphs", json!({}));
    let did = g["glyphs"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|x| x["param"] == d["param"])
        .and_then(|x| x["id"].as_str())
        .unwrap_or_else(|| panic!("{g}"))
        .to_string();
    let schema = output_schema_for::<ToggleDiameterResult>();
    let schema = schema.as_value();
    let r = run(&mut s, "sketch.toggle_diameter", json!({"constraint": did}));
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    assert_eq!(r["diameter"], false, "{r}");
    let r = run(&mut s, "sketch.toggle_diameter", json!({"constraint": did, "unknown": 1}));
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    assert!(r["param_warnings"].as_array().is_some_and(|w| !w.is_empty()), "{r}");
}

fn matches(schema: schemars::Schema, r: &Value) {
    let schema = schema.as_value();
    assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
}

fn keys(r: &Value) -> Vec<&str> {
    r.as_object().map(|o| o.keys().map(String::as_str).collect()).unwrap_or_default()
}

fn ids(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_string)).collect()
}

/// Every command, in every form its handler takes, replies as its output schema says in Warn
/// without a warning; a misspelt key still gets its warning and the reply still matches, and
/// strict refuses it.
#[test]
fn replies_match_the_output_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let run = |s: &mut Session, id: &str, p: Value| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert!(r.get("param_warnings").is_none(), "{id} {p}: {r}");
        r
    };
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let sketch = s.active_sketch.unwrap_or_default();
    let a = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0.5]]}))["curves"])[0].clone();
    let b = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 5], [10, 6]]}))["curves"])[0].clone();

    // The constrainer: one line; two lines; the same again (redundant), by `sketch` as an id.
    let r = run(&mut s, "sketch.constrainer", json!({"entities": [a]}));
    matches(output_schema_for::<ConstrainerResult>(), &r);
    assert_eq!((r["type"].as_str(), keys(&r)), (Some("Horizontal"), vec!["constraint", "created", "sketch", "type"]), "{r}");
    assert!(r["sketch"]["solved"] == true && r["sketch"]["dof"].is_u64(), "{r}");
    let r = run(&mut s, "sketch.constrainer", json!({"entities": [a, b]}));
    assert_eq!(r["type"], "Parallel", "{r}");
    let r = run(&mut s, "sketch.constrainer", json!({"entities": a, "sketch": sketch}));
    matches(output_schema_for::<ConstrainerResult>(), &r);
    assert_eq!((r["redundant"].clone(), r["determined_by"].is_array()), (json!(true), true), "{r}");
    assert_eq!(keys(&r), vec!["constraint", "created", "determined_by", "redundant", "sketch", "type"], "{r}");
    let e = s.execute("sketch.constrainer", &json!({"entities": [a], "if_redundant": "Refuse"})).err().map(|e| e.to_string());
    assert!(e.is_some_and(|e| e.contains("over-constrain")));

    // Curvature continuity, and polygon.
    let l = ids(&run(&mut s, "sketch.line", json!({"points": [[-10, 0], [0, 0]], "infer": true}))["curves"])[0].clone();
    let sp =
        ids(&run(&mut s, "sketch.spline.control_point", json!({"points": [format!("{l}.end"), [5, 2], [10, 6], [15, 15]]}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.constraint.curvature", json!({"a": l, "b": sp, "if_redundant": "keep"}));
    matches(output_schema_for::<ConstraintAdded>(), &r);
    assert_eq!(keys(&r), vec!["constraint", "created", "sketch"], "{r}");
    let r = run(&mut s, "sketch.constraint.curvature", json!({"a": l, "b": sp}));
    matches(output_schema_for::<ConstraintAdded>(), &r);
    assert_eq!(r["redundant"], true, "{r}");
    let quad = ids(&run(&mut s, "sketch.line", json!({"points": [[100, 0], [111, 1], [110, 9], [99, 10]], "closed": true}))["curves"]);
    let r = run(&mut s, "sketch.constraint.polygon", json!({"lines": quad}));
    matches(output_schema_for::<PolygonResult>(), &r);
    assert_eq!((r["constraints"].as_array().map(Vec::len), keys(&r)), (Some(7), vec!["constraints", "created", "sketch"]), "{r}");

    // Glyphs, snaps.
    let g = run(&mut s, "sketch.glyphs", json!({}));
    matches(output_schema_for::<GlyphsResult>(), &g);
    assert!(g["glyphs"].as_array().is_some_and(|l| l.len() > 5) && g["glyphs"][0]["at"].is_array(), "{g}");
    assert_eq!(keys(&g["glyphs"][0]), vec!["at", "dimension", "driven", "failing", "id", "name", "param", "redundant", "value"], "{g}");
    assert_eq!(keys(&g), vec!["conflicts", "dof", "fully_constrained", "glyphs", "sketch"], "{g}");
    let g = run(&mut s, "sketch.glyphs", json!({"sketch": sketch}));
    matches(output_schema_for::<GlyphsResult>(), &g);
    for p in [
        json!({"at": [0.2, 0.1]}),
        json!({"at": {"x": 5, "y": 0.3}, "radius": 1}),
        json!({"at": [15.5, 6], "from": [0, 6.2], "radius": 2}),
        json!({"at": [3.2, 5], "from": [3, 20], "radius": 1.5}),
        json!({"at": [3, 3], "from": null}),
    ] {
        let r = run(&mut s, "sketch.snap", p.clone());
        matches(output_schema_for::<SnapResult>(), &r);
        assert_eq!(keys(&r), vec!["snaps"], "{r}");
    }
    let snaps: Vec<String> = [
        json!({"at": [0.2, 0.1]}),
        json!({"at": [5.1, 0.3]}),
        json!({"at": [3, 5.2]}),
        json!({"at": [3.4, 50], "from": [3, 51], "radius": 1}),
        json!({"at": [3.4, 50], "from": [3.9, 53], "radius": 1}),
        json!({"at": [40, 10.4], "from": [30, 10], "radius": 1}),
    ]
    .into_iter()
    .flat_map(|p| {
        let r = run(&mut s, "sketch.snap", p);
        matches(output_schema_for::<SnapResult>(), &r);
        r["snaps"].as_array().cloned().unwrap_or_default().into_iter().filter_map(|x| x["type"].as_str().map(str::to_string)).collect::<Vec<_>>()
    })
    .collect();
    for kind in ["coincident", "midpoint", "on_curve", "horizontal", "vertical"] {
        assert!(snaps.iter().any(|k| k == kind), "no {kind} in {snaps:?}");
    }
    let r = run(&mut s, "sketch.snap", json!({"at": [5.1, 0.3]}));
    assert!(r["snaps"][0]["curve"].is_string() && r["snaps"][0]["at"].is_array(), "{r}");
    // The end of `a` (made horizontal above) alone: the coincident snap names that point.
    let r = run(&mut s, "sketch.snap", json!({"at": [10.2, 0.1], "radius": 0.5}));
    let first = &r["snaps"][0];
    assert_eq!((first["type"].as_str(), first["point"].as_str()), (Some("coincident"), Some(format!("{a}.end").as_str())), "{r}");

    // A dimension, driven and driving again.
    let c = ids(&run(&mut s, "sketch.circle.center", json!({"center": [50, 0], "radius": 5}))["curves"])[0].clone();
    let d = run(&mut s, "sketch.dimension", json!({"entities": [c]}));
    let g = run(&mut s, "sketch.glyphs", json!({}));
    let did = g["glyphs"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|x| x["param"] == d["param"])
        .and_then(|x| x["id"].as_str())
        .unwrap_or_default()
        .to_string();
    assert!(!did.is_empty(), "{g}");
    let dimension = g["glyphs"].as_array().into_iter().flatten().find(|x| x["id"] == json!(did)).cloned().unwrap_or_default();
    assert!(dimension["value"].is_number() && dimension["dimension"] == true, "{dimension}");
    for p in [json!({"constraint": did}), json!({"constraint": did, "driven": false}), json!({"constraint": did, "driven": true, "sketch": null})] {
        let r = run(&mut s, "sketch.toggle_driven", p);
        matches(output_schema_for::<ToggleDrivenResult>(), &r);
        assert_eq!(keys(&r), vec!["constraint", "created", "driven", "sketch"], "{r}");
    }
    let r = s.execute("sketch.toggle_driven", &json!({"constraint": did, "drivn": true})).unwrap_or_else(|e| panic!("{e}"));
    assert!(r["param_warnings"].as_array().is_some_and(|w| w.len() == 1), "{r}");
    matches(output_schema_for::<ToggleDrivenResult>(), &r);

    // Auto Constrain: with the default options, with every option, from a datum, and finishing.
    run(&mut s, "sketch.point", json!({"point": [5, 5], "id": "d"}));
    for (id, p) in [
        ("sketch.auto_constrain", json!({})),
        ("sketch.auto_constrain", json!({"tolerance": 0.05, "angle_tolerance": 2, "dimensions": false, "datum": "origin", "sketch": null})),
        ("sketch.auto_constrain_from_datum", json!({"datum": "d"})),
        ("sketch.auto_constrain_from_datum", json!({})),
    ] {
        let r = run(&mut s, id, p);
        matches(output_schema_for::<AutoConstrainResult>(), &r);
        assert_eq!(keys(&r), vec!["added", "created", "dof", "sketch"], "{r}");
    }
    let r = run(&mut s, "sketch.finish_auto_constrain", json!({"datum": "d", "tolerance": 0.02}));
    matches(output_schema_for::<AutoConstrainResult>(), &r);
    assert!(s.active_sketch.is_none() && r["dof"] == 0, "{r}");

    // More forms, still without a warning: a sketch by name, `from` as an object, `if_redundant`
    // on a constraint that is not redundant, Finish with every option.
    let mut t = editing(ParamCheck::Warn);
    let tname = t.doc.features.first().map(|f| f.name.clone()).unwrap_or_default();
    let tline = ids(&run(&mut t, "sketch.line", json!({"points": [[0, 30], [10, 30.5]]}))["curves"])[0].clone();
    run(&mut t, "sketch.glyphs", json!({"sketch": tname}));
    run(&mut t, "sketch.snap", json!({"at": [1, 1], "from": {"x": 0, "y": 1}}));
    run(&mut t, "sketch.constrainer", json!({"entities": [tline], "if_redundant": "refuse", "sketch": tname}));
    let r = run(&mut t, "sketch.finish_auto_constrain", json!({"sketch": tname, "dimensions": false, "angle_tolerance": 3}));
    matches(output_schema_for::<AutoConstrainResult>(), &r);

    // A misspelt key runs in warn and says so; the reply still matches. Strict refuses it.
    let mut s = editing(ParamCheck::Warn);
    let p = json!({"tolerence": 0.1});
    let r = s.execute("sketch.auto_constrain", &p).unwrap_or_else(|e| panic!("{e}"));
    assert!(
        r["param_warnings"].as_array().is_some_and(|w| w.len() == 1 && w[0].as_str().is_some_and(|w| w.contains("did you mean `tolerance`"))),
        "{r}"
    );
    matches(output_schema_for::<AutoConstrainResult>(), &r);
    // A new line the constrainer can still act on.
    let line = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 20], [10, 20.5]]}))["curves"])[0].clone();
    for (id, p, reply) in [
        ("sketch.snap", json!({"at": [1, 1], "radious": 2}), output_schema_for::<SnapResult>()),
        ("sketch.glyphs", json!({"sketch_id": 1}), output_schema_for::<GlyphsResult>()),
        ("sketch.constrainer", json!({"entities": [line], "if_redundent": "refuse"}), output_schema_for::<ConstrainerResult>()),
    ] {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert!(r["param_warnings"].as_array().is_some_and(|w| !w.is_empty()), "{id}: {r}");
        matches(reply, &r);
    }
    s.param_check = ParamCheck::Strict;
    let e = s.execute("sketch.snap", &json!({"at": [1, 1], "radious": 2})).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(e.contains("`radious`") && e.contains("`radius`"), "{e}");
}
