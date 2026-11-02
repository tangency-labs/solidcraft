use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::{
    ConvertParams, FixedPoint, FlangeEdges, FlangeParams, FlangePosition, FlangeTypeParam, FoldParams, FoldPoints, FoldPosition, HeightFromParam,
    HemParams, PerforateParams, PerforationPattern, ReliefShapeParam, SheetPoints, ThenSteps, UnfoldParams,
};
use crate::Session;
use crate::schema::features::FeatureResult;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

/// The built-in rule's name, which a design without rules uses.
const STEEL: &str = "Steel (mm)";

/// A command's own keys and `FeatureCommon`'s, sorted.
fn keys(own: &[&'static str]) -> Vec<&'static str> {
    let mut k = own.to_vec();
    k.extend(["body_name", "body_names", "name", "participants"]);
    k.sort_unstable();
    k
}

/// A 100 × 60 sheet of the default rule (2.5 thick) from sketch `Base`, and a 100 × 60 × 2.5
/// box `Plate` beside it.
fn sheet() -> Session {
    let mut s = Session::default();
    for (id, p) in [
        ("sketch.create", json!({"plane": "XY", "name": "Base"})),
        ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]})),
        ("sketch.finish", json!({})),
        ("sheet.flange", json!({"sketch": "Base"})),
        ("solid.box", json!({"length": 100, "width": 60, "height": 2.5, "corner": [0, 100, 0], "body_name": "Plate"})),
    ] {
        if let Err(e) = s.execute(id, &p) {
            panic!("{id}: {e}");
        }
    }
    s
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<FlangeParams>(),
        &keys(&[
            "angle",
            "body",
            "corner_relief",
            "corner_relief_shape",
            "curves",
            "distance",
            "edges",
            "flip",
            "height",
            "height_from",
            "position",
            "profiles",
            "radius",
            "reverse",
            "rule",
            "sketch",
            "then",
            "type",
        ]),
    );
    assert_keys(input_schema_for::<PerforateParams>(), &keys(&["body", "diameter", "margin", "pattern", "pitch"]));
    assert_keys(input_schema_for::<HemParams>(), &keys(&["body", "edges", "flip", "gap", "length"]));
    assert_keys(input_schema_for::<FoldParams>(), &keys(&["angle", "body", "curve", "fixed", "flip", "points", "position", "radius", "sketch"]));
    assert_keys(input_schema_for::<UnfoldParams>(), &keys(&["body"]));
    assert_keys(input_schema_for::<ConvertParams>(), &keys(&["bends", "body", "face", "rule"]));
}

fn parse<T: DeserializeOwned>(v: Value) -> Option<T> {
    serde_json::from_value(v).ok()
}

/// Whether `T` accepts `v` and its schema agrees.
fn takes<T: DeserializeOwned + schemars::JsonSchema>(v: &Value) -> bool {
    let schema = input_schema_for::<T>();
    let schema = schema.as_value();
    let accepted = parse::<T>(v.clone()).is_some();
    assert_eq!(conforms(schema, schema, v), accepted, "{v}: the schema and the reader disagree");
    accepted
}

/// The value types accept what the handlers' readers accept, and their schemas agree (apart
/// from the word types' patterns, which the test validator doesn't read: those are checked on
/// the readers alone).
#[test]
fn the_value_types_match_the_readers() {
    let many = |n: usize| Value::Array(vec![json!([1, 2, 3]); n]);
    for (v, ok) in [
        (json!([[1, 2, 3]]), true),
        (json!([{"x": 1, "y": 2, "z": 3}, {"point": [1, 2, 3]}, {"point": {"x": 1, "y": 2, "z": 3}, "body": "B"}]), true),
        (many(1000), true),
        (many(1001), false),
        (json!([]), false),
        (json!([[1, 2]]), false),
        (json!([{"point": [1, 2]}]), false),
        (json!([[1e10, 0, 0]]), false),
        (json!(5), false),
        (json!(null), false),
    ] {
        assert_eq!(takes::<SheetPoints>(&v), ok, "{v}");
        assert_eq!(parse::<SheetPoints>(v.clone()).map(|p| p.0), crate::cmd::sheet_points(Some(&v)).ok(), "{v}");
    }
    for (v, ok) in [
        (json!([[1, 2, 3], {"x": 1, "y": 2, "z": 3}]), true),
        (json!([{"point": [1, 2, 3]}, {"point": [1, 2, 3], "angle": "30 deg", "height": 5}, {"point": [1, 2, 3], "angle": null}]), true),
        (many(1000), true),
        (many(1001), false),
        (json!([{"x": 1, "y": 2, "z": 3, "point": [1, 2, 3]}]), false),
        (json!([{"x": 1, "y": 2, "z": 3, "angle": 30}]), false),
        (json!([{"angle": 30}]), false),
        (json!([{"point": [1, 2, 3], "depth": 1}]), false),
        (json!([{"point": [1, 2, 3], "angle": true}]), false),
        (json!([{"point": [1, 2]}]), false),
        (json!([[1, 2]]), false),
        (json!([]), false),
        (json!(null), false),
    ] {
        assert_eq!(takes::<FlangeEdges>(&v), ok, "{v}");
    }
    let bends = |n: usize| Value::Array(vec![json!({"height": 10}); n]);
    for (v, ok) in [
        (json!([{"height": 10}]), true),
        (json!([{"height": 10, "angle": "45 deg", "flip": true}, {"height": "5 mm", "angle": null, "flip": null}]), true),
        (json!([]), true),
        (bends(16), true),
        (bends(17), false),
        (json!([{"angle": 30}]), false),
        (json!([{"height": 10, "depth": 1}]), false),
        (json!([{"height": 10, "flip": "yes"}]), false),
        (json!([5]), false),
        (json!(5), false),
    ] {
        assert_eq!(takes::<ThenSteps>(&v), ok, "{v}");
        assert_eq!(parse::<ThenSteps>(v.clone()).map(|t| t.0), crate::cmd::then_list(Some(&v)).ok(), "{v}");
    }
    // The handler reads a bend's `angle` that is no expression as left out (90°); the type
    // refuses it, which is what warn and strict report.
    for angle in [json!(true), json!([45])] {
        let v = json!([{"height": 10, "angle": angle}]);
        assert!(!takes::<ThenSteps>(&v), "{v}");
        assert_eq!(crate::cmd::then_list(Some(&v)), crate::cmd::then_list(Some(&json!([{"height": 10}]))), "{v}");
    }
    for (v, ok) in [
        (json!([[0, 0, 0], [1, 1, 1]]), true),
        (json!([{"x": 0, "y": 0, "z": 0}, [1, 1, 1]]), true),
        (json!([[0, 0, 0]]), false),
        (json!([[0, 0, 0], [1, 1]]), false),
        (json!([[0, 0, 0], [1, 1, 1], [2, 2, 2]]), false),
        (json!(null), false),
    ] {
        assert_eq!(takes::<FoldPoints>(&v), ok, "{v}");
    }
    for (v, ok) in [(json!([1, 2, 3]), true), (json!({"x": 1, "y": 2, "z": 3}), true), (json!([1, 2]), false), (json!(null), false)] {
        assert_eq!(takes::<FixedPoint>(&v), ok, "{v}");
    }
    for (v, ok) in [
        (json!("staggered"), true),
        (json!("square"), true),
        (json!("Square"), false),
        (json!("hex"), false),
        (json!(5), false),
        (json!(null), false),
    ] {
        assert_eq!(takes::<PerforationPattern>(&v), ok, "{v}");
    }
    let words = |accepted: &[&str], refused: &[Value], read: &dyn Fn(Value) -> bool| {
        for w in accepted {
            assert!(read(json!(w)), "{w}");
        }
        for v in refused {
            assert!(!read(v.clone()), "{v}");
        }
    };
    words(&["base", "Edge", "CONTOUR"], &[json!("sideways"), json!(""), json!(5)], &|v| parse::<FlangeTypeParam>(v).is_some());
    words(
        &["inside", "Outside", "MIDDLE", "bend_outside", "Material_Outside", "TANGENT"],
        &[json!("mid"), json!(" inside"), json!("material outside"), json!(""), json!(3)],
        &|v| parse::<FlangePosition>(v).is_some(),
    );
    words(&["outer", " Inner ", "TANGENT"], &[json!("sharp"), json!(""), json!(5)], &|v| parse::<HeightFromParam>(v).is_some());
    words(&["centerline", "Start", "END", "mould", "Mold"], &[json!("center"), json!("side"), json!(null)], &|v| parse::<FoldPosition>(v).is_some());
    words(&["notch", " Round ", "rectangular", "TEAR"], &[json!("oval"), json!(""), json!(5)], &|v| parse::<ReliefShapeParam>(v).is_some());
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("sheet.flange", json!({"type": "sideways"})),
        ("sheet.flange", json!({"type": "EDGEWAYS", "edges": [[0, 0, 0]], "height": 10})),
        ("sheet.flange", json!({"edges": [], "height": 10})),
        ("sheet.flange", json!({"edges": 5, "height": 10})),
        ("sheet.flange", json!({"edges": [[0, 0]], "height": 10})),
        ("sheet.flange", json!({"edges": [[0, 0, 0], {"point": [0, 0, 0], "depth": 3}], "height": 10})),
        ("sheet.flange", json!({"edges": [{"point": [0, 0]}], "height": 10})),
        ("sheet.flange", json!({"edges": [{"point": [0, 0, 0], "angle": true}], "height": 10})),
        ("sheet.flange", json!({"edges": [{"point": [0, 0, 0], "height": "  "}], "height": 10})),
        ("sheet.flange", json!({"edges": [[0, 0, 0]], "height": 10, "position": "nowhere"})),
        ("sheet.flange", json!({"edges": [[0, 0, 0]], "height": 10, "height_from": "sharp"})),
        ("sheet.flange", json!({"edges": [[0, 0, 0]], "height": 10, "then": 5})),
        ("sheet.flange", json!({"edges": [[0, 0, 0]], "height": 10, "then": [{"angle": 30}]})),
        ("sheet.flange", json!({"edges": [[0, 0, 0]], "height": 10, "then": [{"height": 5, "turn": 1}]})),
        ("sheet.perforate", json!({"diameter": 2, "pitch": 5, "pattern": "hex"})),
        ("sheet.perforate", json!({"diameter": 2, "pitch": 5, "pattern": "Square"})),
        ("sheet.hem", json!({"edges": [], "length": 5})),
        ("sheet.hem", json!({"edges": 5, "length": 5})),
        ("sheet.hem", json!({"edges": null, "length": 5})),
        ("sheet.hem", json!({"edges": [[1, 2]], "length": 5})),
        ("sheet.fold", json!({"points": [[0, 0, 0]]})),
        ("sheet.fold", json!({"points": [[0, 0, 0], [1, 2]]})),
        ("sheet.fold", json!({"points": null})),
        ("sheet.fold", json!({"points": [[0, 0, 0], [10, 0, 0]], "position": "side"})),
        ("sheet.fold", json!({"points": [[0, 0, 0], [10, 0, 0]], "fixed": [1, 2]})),
        ("sheet.fold", json!({"points": [[0, 0, 0], [10, 0, 0]], "fixed": null})),
        ("sheet.fold", json!({"sketch": true, "curve": "l1"})),
        ("sheet.convert", json!({"body": "Nothing", "face": [0, 0, 0]})),
    ]);
    // Calls that get past the handler's first checks only on a sheet.
    for (id, p) in [
        ("sheet.flange", json!({"edges": [[50, 0, 2.5]], "height": 20, "corner_relief_shape": "oval"})),
        ("sheet.flange", json!({"edges": [[50, 0, 2.5]], "height": 20, "corner_relief_shape": 5})),
        ("sheet.convert", json!({"body": "Plate", "face": [50, 130, 2.5], "bends": []})),
        ("sheet.convert", json!({"body": "Plate", "face": [50, 130, 2.5], "bends": [[1, 2]]})),
        ("sheet.convert", json!({"body": "Plate", "face": ""})),
        ("sheet.convert", json!({"body": "Plate", "face": true})),
    ] {
        let error = |level: ParamCheck| {
            let mut s = sheet();
            s.param_check = level;
            s.execute(id, &p).err().map(|e| e.to_string())
        };
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{p}");
    }
}

/// A rule's value given to a flange or a bent conversion is refused either way: by the handler,
/// which points to `sheet.manage_rules`, and in strict as a key the command doesn't take.
#[test]
fn rule_values_are_refused_with_the_check_off_and_in_strict() {
    for (id, p, key) in [
        ("sheet.flange", json!({"sketch": "Base", "thickness": 2}), "thickness"),
        ("sheet.flange", json!({"edges": [[50, 0, 2.5]], "height": 20, "k_factor": 0.4}), "k_factor"),
        ("sheet.convert", json!({"body": "Plate", "face": [50, 130, 2.5], "bends": [[50, 100, 1.25]], "bend_radius": 2}), "bend_radius"),
    ] {
        let error = |level: ParamCheck| {
            let mut s = sheet();
            s.param_check = level;
            s.execute(id, &p).err().map(|e| e.to_string()).unwrap_or_default()
        };
        let off = error(ParamCheck::Off);
        assert!(off.contains(&format!("`{key}` is not a {id} parameter: the sheet metal rule sets it")), "{p}: {off}");
        assert!(error(ParamCheck::Strict).contains(&format!("unknown key `{key}`")), "{p}");
    }
}

/// A further bend's `angle` that is no expression: with the check off the flange bends it 90°,
/// as before; warn reports it and strict refuses it.
#[test]
fn a_bend_angle_that_is_no_expression_is_reported() {
    let p = json!({"edges": [[0, 30, 2.5]], "height": 12, "then": [{"height": 8, "angle": true}]});
    let want = "then[0]: `angle` must be a number or an expression";
    let run = |level: ParamCheck| {
        let mut s = sheet();
        s.param_check = level;
        s.execute("sheet.flange", &p)
    };
    let off = run(ParamCheck::Off).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(off.get("param_warnings"), None, "{off}");
    let warn = run(ParamCheck::Warn).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(warn["param_warnings"], json!([want]), "{warn}");
    let strict = run(ParamCheck::Strict).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(strict.ends_with(want), "{strict}");
}

#[test]
fn replies_match_the_output_schema() {
    let schema = output_schema_for::<FeatureResult>();
    let schema = schema.as_value();
    let check = |r: Value| assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    let warn = || {
        let mut s = sheet();
        s.param_check = ParamCheck::Warn;
        s
    };
    let ok = |s: &mut Session, id: &str, p: Value| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert_eq!(r.get("param_warnings"), None, "{id} {p}: {r}");
        r
    };
    // Each form each command takes, in Warn, without warnings.
    let mut s = warn();
    for (id, p) in [
        ("sheet.flange", json!({"edges": [[50, 0, 2.5]], "height": 20})),
        ("sheet.hem", json!({"edges": [[50, 0, 20]], "length": 8, "gap": 1, "flip": false})),
        (
            "sheet.flange",
            json!({"type": "Edge", "edges": [{"point": [100, 30, 2.5], "angle": "60 deg", "height": null}], "height": 15, "angle": 90,
                   "radius": 2, "position": "OUTSIDE", "flip": true, "corner_relief_shape": " round ", "corner_relief": 3, "name": "Side"}),
        ),
        ("sheet.perforate", json!({"diameter": 2, "pitch": 5, "margin": 3, "pattern": "square"})),
        (
            "sheet.flange",
            json!({"edges": [[0, 30, 2.5]], "height": 12, "position": "material_outside", "height_from": "inner",
                   "then": [{"height": 8}, {"height": 6, "angle": "45 deg", "flip": true}], "name": "Chain"}),
        ),
        ("sheet.unfold", json!({})),
        ("sheet.refold", json!({"name": "Back"})),
        ("sheet.flange", json!({"type": "BASE", "sketch": "Base", "profiles": "all", "rule": STEEL, "flip": true, "body_names": ["Second"]})),
        ("sheet.unfold", json!({"body": "Second"})),
        ("sheet.refold", json!({"body": "Second"})),
        ("sheet.convert", json!({"body": "Plate", "face": [50, 130, 2.5], "bends": null, "rule": STEEL, "body_name": "Plated"})),
    ] {
        check(ok(&mut s, id, p));
    }
    let mut s = warn();
    for (id, p) in [
        ("sheet.fold", json!({"points": [[60, 0, 2.5], [60, 60, 2.5]], "angle": "45 deg", "radius": 1, "position": "Mould", "fixed": [10, 30, 2.5]})),
        ("sheet.perforate", json!({"diameter": 2, "pitch": 6})),
    ] {
        check(ok(&mut s, id, p));
    }
    let mut s = warn();
    for (id, p) in [
        ("sketch.create", json!({"plane": "XY", "offset": 2.5, "name": "FoldLine"})),
        ("sketch.line", json!({"points": [[40, 0], [40, 60]], "ids": ["fl"]})),
        ("sketch.finish", json!({})),
    ] {
        ok(&mut s, id, p);
    }
    let body = s.model.state().sheets.first().map(|sh| sh.body.clone()).unwrap_or_default();
    check(ok(&mut s, "sheet.fold", json!({"sketch": "FoldLine", "curve": "fl", "position": "start", "flip": true, "body": body})));
    let mut s = warn();
    for (id, p) in [
        ("sketch.create", json!({"plane": {"origin": [0, 0, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, -1]}, "name": "Contour"})),
        ("sketch.line", json!({"points": [[0, 0], [40, 0], [40, -30], [70, -30]], "ids": ["l1", "l2", "l3"]})),
        ("sketch.finish", json!({})),
    ] {
        ok(&mut s, id, p);
    }
    check(ok(
        &mut s,
        "sheet.flange",
        json!({"sketch": "Contour", "curves": ["l1", "l2", "l3"], "distance": 50, "reverse": true, "flip": true, "rule": STEEL}),
    ));
    check(ok(&mut s, "sheet.flange", json!({"type": "contour", "sketch": "Contour", "curves": "l1", "distance": "20 mm", "body_name": "Strip"})));
    // A misspelt key: the call runs, and the reply carrying its warning still matches.
    let mut s = warn();
    let r = s.execute("sheet.unfold", &json!({"bodies": "x"})).unwrap_or_else(|e| panic!("{e}"));
    assert!(r.get("param_warnings").is_some(), "{r}");
    check(r);
}

#[test]
fn rule_commands_publish_contracts_and_refuse_unknown_keys() {
    for id in ["sheet.rules", "sheet.manage_rules"] {
        let spec = crate::command_specs().into_iter().find(|c| c.id == id).unwrap();
        assert!(spec.contract.is_some() && spec.output.is_some(), "{id}: input schema missing");
        let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
        let p = if id == "sheet.rules" { json!({"unknown_key": true}) } else { json!({"name": "Test", "thikness": 1}) };
        assert!(s.execute(id, &p).is_err(), "{id}: unknown key accepted");
        assert!(s.doc.sheet.rules.is_empty());
    }
}

#[test]
fn rule_contract_keys_and_readers_match() {
    use super::{AngleDisplayParam, ManageRulesParams, RuleName, RulesParams};
    assert_keys(input_schema_for::<RulesParams>(), &[]);
    assert_keys(
        input_schema_for::<ManageRulesParams>(),
        &[
            "active",
            "angle_display",
            "bend_radius",
            "corner_relief",
            "corner_relief_shape",
            "delete",
            "gap",
            "hem_gap",
            "k_factor",
            "name",
            "relief_depth",
            "relief_width",
            "thickness",
        ],
    );
    for (text, valid) in [("bend", true), (" INCLUDED ", true), ("\u{2003}BeNd\u{2003}", true), ("outside", false)] {
        assert_eq!(parse::<AngleDisplayParam>(json!(text)).is_some(), valid);
    }
    assert!(parse::<AngleDisplayParam>(json!(false)).is_none());
    for v in [json!(null), json!(false), json!(1), json!(""), json!(" \n "), json!("a".repeat(129)), json!("é".repeat(65))] {
        assert!(parse::<RuleName>(v).is_none());
    }
    assert_eq!(parse::<RuleName>(json!("  My rule  ")).unwrap().0, "My rule");
    assert!(parse::<RuleName>(json!("é".repeat(64))).is_some());
    let pattern = input_schema_for::<AngleDisplayParam>();
    assert!(pattern.as_value()["pattern"].as_str().unwrap().contains(&super::blanks()));
    assert_strict_keeps_todays_errors(&[
        ("sheet.manage_rules", json!({"name": null})),
        ("sheet.manage_rules", json!({"name": true})),
        ("sheet.manage_rules", json!({"name": "  "})),
        ("sheet.manage_rules", json!({"name": "a".repeat(129)})),
        ("sheet.manage_rules", json!({"name": "R", "angle_display": "outer"})),
        ("sheet.manage_rules", json!({"name": "R", "angle_display": 5})),
        ("sheet.manage_rules", json!({"name": "R", "corner_relief_shape": "oval"})),
        ("sheet.manage_rules", json!({"name": "R", "thickness": -1})),
        ("sheet.manage_rules", json!({"name": "R", "k_factor": 2})),
        ("sheet.manage_rules", json!({"name": "Missing", "delete": true})),
    ]);
}

/// Run all rule forms in Warn, compare with Off, and pin the successful reply shapes.
#[test]
fn rule_replies_match_the_output_schemas_without_warnings() {
    use super::{ManageRulesResult, RulesResult};
    let mut off = Session::default();
    let mut warn = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let calls = [
        ("sheet.rules", json!({})),
        (
            "sheet.manage_rules",
            json!({"name": "  Metric  ", "thickness": 2, "k_factor": "0.4", "bend_radius": "Thickness / 2",
            "relief_width": 1, "relief_depth": "Thickness", "corner_relief": 3, "corner_relief_shape": " Round ",
            "hem_gap": 0.1, "gap": "Thickness / 2", "angle_display": " INCLUDED ", "active": true}),
        ),
        (
            "sheet.manage_rules",
            json!({"name": "Metric", "thickness": "3 mm", "k_factor": 0.5, "bend_radius": 1,
            "relief_width": "1 mm", "relief_depth": 2, "corner_relief": "3 mm", "corner_relief_shape": "tear",
            "hem_gap": "0.2 mm", "gap": 2, "angle_display": "BeNd", "active": false, "delete": false}),
        ),
        ("sheet.manage_rules", json!({"name": "Other"})),
        (
            "sheet.manage_rules",
            json!({"name": "Other", "thickness": null, "k_factor": null, "bend_radius": null,
            "relief_width": null, "relief_depth": null, "corner_relief": null, "corner_relief_shape": null,
            "hem_gap": null, "gap": null, "angle_display": null, "active": null, "delete": null}),
        ),
        ("sheet.rules", json!({})),
        ("sheet.manage_rules", json!({"name": "Other", "active": true})),
        ("sheet.manage_rules", json!({"name": "Metric", "delete": true})),
        ("sheet.manage_rules", json!({"name": "Other", "delete": true})),
        ("sheet.rules", json!({})),
    ];
    for (id, p) in calls {
        let a = off.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        let b = warn.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert_eq!(a, b, "{id} {p}");
        assert!(b.get("param_warnings").is_none(), "{id} {p}: {b}");
        let schema = if id == "sheet.rules" { output_schema_for::<RulesResult>() } else { output_schema_for::<ManageRulesResult>() };
        assert!(conforms(schema.as_value(), schema.as_value(), &b), "{id}: {b}");
        if let Some(rule) = b.get("rule") {
            let saved = off.doc.sheet.rules.iter().find(|r| json!(r.name) == rule["name"]).unwrap();
            let mut old = json!(saved);
            old["angle_display"] = json!(saved.angle_display);
            assert_eq!(*rule, old);
            let (vals, _) = off.doc.param_values();
            assert_eq!(b["values"], json!(off.doc.rule_values(&vals, saved).unwrap()));
            assert_eq!(b["errors"], json!([]));
        }
    }
    // Saved rules whose expressions no longer evaluate are still listed, with null values.
    warn.doc_mut().sheet.rules.push(solidcraft_doc::sheet::SheetRule {
        name: "Invalid".into(),
        thickness: "Missing".into(),
        ..solidcraft_doc::sheet::SheetRule::steel()
    });
    let reply = warn.execute("sheet.rules", &json!({})).unwrap();
    assert_eq!(reply["rules"][0]["values"], Value::Null);
    let schema = output_schema_for::<RulesResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply));
}

#[test]
fn rule_warnings_match_the_output_schema_and_strict_does_not_mutate() {
    use super::{ManageRulesResult, RulesResult};
    for (id, p) in [("sheet.rules", json!({"naem": "R"})), ("sheet.manage_rules", json!({"name": "R", "thikness": 1}))] {
        let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
        assert!(s.execute(id, &p).is_err());
        assert!(s.doc.sheet.rules.is_empty());
        s.param_check = ParamCheck::Warn;
        let reply = s.execute(id, &p).unwrap();
        assert!(!reply["param_warnings"].as_array().unwrap().is_empty());
        let schema = if id == "sheet.rules" { output_schema_for::<RulesResult>() } else { output_schema_for::<ManageRulesResult>() };
        assert!(conforms(schema.as_value(), schema.as_value(), &reply));
    }
}
