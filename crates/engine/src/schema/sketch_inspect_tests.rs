use serde_json::{Value, json};

use super::{CenterOfMassParams, CentersOfMass, CurvatureCombParams, CurvatureCombs, IsocurveParams, Isocurves, MinimumRadii, MinimumRadiusParams};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<CurvatureCombParams>(), &["count", "curves", "density", "edges", "faces", "scale", "sketch"]);
    assert_keys(input_schema_for::<MinimumRadiusParams>(), &["curves", "edges", "faces", "sketch"]);
    assert_keys(input_schema_for::<IsocurveParams>(), &["count", "faces"]);
    assert_keys(input_schema_for::<CenterOfMassParams>(), &["bodies"]);
}

/// Each point list keeps its own command's messages: a bad point, an empty list, `null`.
#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("inspect.curvature_comb", json!({})),
        ("inspect.curvature_comb", json!({"faces": []})),
        ("inspect.curvature_comb", json!({"faces": null})),
        ("inspect.curvature_comb", json!({"faces": "x"})),
        ("inspect.curvature_comb", json!({"faces": [[1, 2]]})),
        ("inspect.curvature_comb", json!({"faces": [[0, 0, 0]], "count": "x"})),
        ("inspect.curvature_comb", json!({"faces": [[0, 0, 0]], "count": 1e9})),
        ("inspect.curvature_comb", json!({"edges": [[1, 2]]})),
        ("inspect.curvature_comb", json!({"edges": [[0, 0, 1e9]]})),
        ("inspect.minimum_radius", json!({"edges": ["x"]})),
        ("inspect.minimum_radius", json!({"faces": [[1, 2]]})),
        ("inspect.minimum_radius", json!({"faces": [[0, 0, 500]]})),
        ("inspect.isocurve", json!({"faces": []})),
        ("inspect.isocurve", json!({"faces": null})),
        ("inspect.isocurve", json!({"faces": 5})),
        ("inspect.isocurve", json!({"faces": [[1, 2]]})),
        ("inspect.isocurve", json!({"faces": [{"x": 0, "y": 0, "z": 500}]})),
        ("inspect.center_of_mass", json!({})),
        ("inspect.center_of_mass", json!({"bodies": ["nope"]})),
    ]);
}

/// #10949: the four analysis commands reply as their output schemas say, in Warn without
/// warnings for good calls and with them for a misspelt key, on sketch curves, a cylinder's
/// edge and face, and every face. A `density` that isn't a number, which the handler reads as
/// none, gets a warning, and strict refuses it.
#[test]
fn analysis_commands_reply_with_their_schemas() {
    let (combs, radii, isocurves, centers) = (
        output_schema_for::<CurvatureCombs>(),
        output_schema_for::<MinimumRadii>(),
        output_schema_for::<Isocurves>(),
        output_schema_for::<CentersOfMass>(),
    );
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let run = |s: &mut Session, id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    let matches = |schema: &schemars::Schema, r: &Value, warned: bool| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
        assert_eq!(r.get("param_warnings").is_some(), warned, "{r}");
    };
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Curves"}));
    let circle = run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 5}))["curves"][0].clone();
    let line = run(&mut s, "sketch.line", json!({"points": [[0, 20], [10, 20]]}))["curves"][0].clone();
    let r = run(&mut s, "inspect.curvature_comb", json!({"curves": [circle, line], "density": 20}));
    matches(&combs, &r, false);
    assert!(r["combs"][1]["min_radius"].is_null(), "a line is straight: {r}");
    let r = run(&mut s, "inspect.minimum_radius", json!({"curves": circle, "sketch": "Curves"}));
    matches(&radii, &r, false);
    assert!(r["items"][0]["at"].is_array() && r["at"].is_array(), "{r}");
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.cylinder", json!({"base": [50, 0, 0], "radius": 8, "height": 10, "body_name": "Can"}));

    matches(&combs, &run(&mut s, "inspect.curvature_comb", json!({"edges": [[58, 0, 10]]})), false);
    let r = run(&mut s, "inspect.curvature_comb", json!({"faces": [{"x": 58, "y": 0, "z": 5}], "count": 3, "scale": 0.5}));
    matches(&combs, &r, false);
    assert_eq!(r["scale"], 0.5, "{r}");
    matches(&radii, &run(&mut s, "inspect.minimum_radius", json!({"faces": [[58, 0, 5]], "edges": [[58, 0, 10]]})), false);
    // Every face: the flat ends have no radius, and so no `at`.
    let r = run(&mut s, "inspect.minimum_radius", json!({}));
    matches(&radii, &r, false);
    assert!(r["items"].as_array().into_iter().flatten().any(|i| i["min_radius"].is_null() && i.get("at").is_none()), "{r}");
    matches(&isocurves, &run(&mut s, "inspect.isocurve", json!({"faces": [[58, 0, 5]], "count": 4})), false);
    matches(&centers, &run(&mut s, "inspect.center_of_mass", json!({})), false);
    let r = run(&mut s, "inspect.center_of_mass", json!({"bodies": "Can"}));
    matches(&centers, &r, false);
    assert_eq!(r["bodies"][0]["body"], "Can", "{r}");

    matches(&combs, &run(&mut s, "inspect.curvature_comb", json!({"edges": [[58, 0, 10]], "dencity": 20})), true);
    matches(&combs, &run(&mut s, "inspect.curvature_comb", json!({"edges": [[58, 0, 10]], "density": "20"})), true);
    matches(&radii, &run(&mut s, "inspect.minimum_radius", json!({"face": [[58, 0, 5]]})), true);
    matches(&isocurves, &run(&mut s, "inspect.isocurve", json!({"faces": [[58, 0, 5]], "cnt": 2})), true);
    matches(&centers, &run(&mut s, "inspect.center_of_mass", json!({"body": "Can"})), true);
    s.param_check = ParamCheck::Strict;
    let error = s.execute("inspect.curvature_comb", &json!({"edges": [[58, 0, 10]], "density": "20"})).err().map(|e| e.to_string());
    assert!(error.as_deref().is_some_and(|e| e.contains("`density` must be a number")), "{error:?}");
}
