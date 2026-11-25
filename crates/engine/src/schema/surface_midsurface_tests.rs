use serde_json::{Value, json};

use super::{MidsurfaceAnalysis, MidsurfaceAnalyzeParams, MidsurfaceParams};
use crate::Session;
use crate::schema::features::FeatureResult;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<MidsurfaceParams>(),
        &[
            "body",
            "body_name",
            "body_names",
            "carry_holes",
            "exclude",
            "include_review_pairs",
            "keep_original",
            "max_thickness",
            "min_aspect",
            "min_thickness",
            "name",
            "pairs",
            "participants",
            "stitch",
            "suppress_holes_below",
        ],
    );
    assert_keys(input_schema_for::<MidsurfaceAnalyzeParams>(), &["body", "max_thickness", "min_aspect", "min_thickness"]);
}

/// A plate 40 x 30 x 2 named `Plate`, at `level`.
fn plate(level: ParamCheck) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 2, "body_name": "Plate"})).unwrap_or_else(|e| panic!("{e}"));
    s
}

/// Calls that fail today give the same message in strict: a missing or misnamed body, a body the
/// design lacks, and limits that don't evaluate or have the wrong unit.
#[test]
fn strict_keeps_todays_errors() {
    for cmd in ["surface.midsurface", "surface.midsurface_analyze"] {
        for p in [
            json!({}),
            json!({"body": 5}),
            json!({"body": null}),
            json!({"body": "Nope"}),
            json!({"body": "Plate", "max_thickness": "nonsense"}),
            json!({"body": "Plate", "max_thickness": "30 deg"}),
            json!({"body": "Plate", "min_thickness": "nonsense"}),
        ] {
            let error = |level: ParamCheck| plate(level).execute(cmd, &p).err().map(|e| e.to_string());
            let off = error(ParamCheck::Off);
            assert!(off.is_some(), "{cmd} {p} should fail today");
            assert_eq!(error(ParamCheck::Strict), off, "{cmd} {p}");
        }
    }
}

/// A limit given that is not a number or an expression is refused, not read as absent and
/// defaulted; an empty `pairs` list chooses nothing and is refused, not read as "the confident
/// ones" (which would build them, and with `keep_original: false` remove the solid). Even with
/// the parameter check off, and with nothing added to the design.
#[test]
fn unreadable_limits_and_an_empty_pair_list_are_refused_not_defaulted() {
    for p in [
        json!({"body": "Plate", "max_thickness": ""}),
        json!({"body": "Plate", "max_thickness": "5 mm", "min_thickness": " "}),
        json!({"body": "Plate", "max_thickness": "5 mm", "suppress_holes_below": ""}),
        json!({"body": "Plate", "max_thickness": "5 mm", "suppress_holes_below": true}),
        json!({"body": "Plate", "max_thickness": "5 mm", "pairs": [], "keep_original": false}),
        // A pair of three points, or of one, is refused before its points are read.
        json!({"body": "Plate", "max_thickness": "5 mm", "pairs": [[[0, 0, 0], [0, 0, 1], [0, 0, 2]]]}),
        json!({"body": "Plate", "max_thickness": "5 mm", "pairs": [[[0, 0, 0]]]}),
        // A true/false option given as anything but true or false is refused, not read as absent:
        // "false" would otherwise build one sewn body, or (`keep_original`) keep the solid.
        json!({"body": "Plate", "max_thickness": "5 mm", "stitch": "false"}),
        json!({"body": "Plate", "max_thickness": "5 mm", "carry_holes": 0}),
        json!({"body": "Plate", "max_thickness": "5 mm", "keep_original": "false"}),
        json!({"body": "Plate", "max_thickness": "5 mm", "include_review_pairs": "yes"}),
    ] {
        let mut s = plate(ParamCheck::Off);
        let before = s.doc.features.len();
        assert!(s.execute("surface.midsurface", &p).is_err(), "{p}");
        assert_eq!(s.doc.features.len(), before, "{p}");
        assert!(s.model.state().body("Plate").is_some(), "{p}");
    }
    // The pair list is read before the body is scanned: an empty one is refused as such even
    // where the body could not be analysed.
    let mut s = plate(ParamCheck::Off);
    let error = s.execute("surface.midsurface", &json!({"body": "Nope", "pairs": []})).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains("`pairs` is empty"), "{error}");
    // So are the true/false options.
    let mut s = plate(ParamCheck::Off);
    let error = s.execute("surface.midsurface", &json!({"body": "Nope", "stitch": "false"})).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains("`stitch` must be true or false"), "{error}");
    // Left out or null, the defaults stand: the plate's one pair is built, one surface body, and
    // the plate is kept.
    let mut s = plate(ParamCheck::Off);
    let reply = s
        .execute(
            "surface.midsurface",
            &json!({"body": "Plate", "max_thickness": "5 mm", "min_thickness": null, "pairs": null, "stitch": null, "keep_original": null}),
        )
        .unwrap_or_else(|e| panic!("{e}"));
    let created: Vec<&str> =
        reply.get("created_bodies").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    assert_eq!(created.len(), 1, "{reply}");
    assert!(created.iter().all(|n| s.model.state().body(n).is_some_and(|b| b.body.is_surface())), "{reply}");
    assert!(s.model.state().body("Plate").is_some(), "{reply}");
}

/// A plate 40 x 30 x 10 spans sqrt(1200) / 10 = 3.46 times its thickness: below the default 5
/// it needs a decision, but with `min_aspect: 0` it is sure and built. The feature keeps the 0,
/// so its rebuild judges by the same limit, and the warning asks for no decision.
#[test]
fn the_aspect_limit_asked_for_is_stored_and_the_warning_asks_no_decision() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 10, "body_name": "Plate"})).unwrap_or_else(|e| panic!("{e}"));
    let reply =
        s.execute("surface.midsurface", &json!({"body": "Plate", "max_thickness": "11 mm", "min_aspect": 0})).unwrap_or_else(|e| panic!("{e}"));
    let warning = reply.get("warning").and_then(Value::as_str).unwrap_or("");
    assert!(!warning.contains("need a decision"), "{reply}");
    let stored = s.doc.features.iter().rev().find_map(|f| match &f.kind {
        solidcraft_doc::FeatureKind::SurfaceMidsurface { min_aspect, .. } => Some(*min_aspect),
        _ => None,
    });
    assert_eq!(stored, Some(Some(0.0)), "{reply}");
}

#[test]
fn replies_match_the_output_schemas() {
    let check = |schema: schemars::Schema, reply: &Value| {
        let schema = schema.as_value().clone();
        assert!(conforms(&schema, &schema, reply), "{reply}");
        assert!(reply.get("param_warnings").is_none(), "{reply}");
    };
    let mut s = plate(ParamCheck::Warn);
    let (before, undo) = (s.doc.clone(), s.undo.len());
    let analysis = s.execute("surface.midsurface_analyze", &json!({"body": "Plate", "max_thickness": 5})).unwrap_or_else(|e| panic!("{e}"));
    // Read-only: the analysis adds no feature and no undo step.
    assert_eq!(s.doc, before, "{analysis}");
    assert_eq!(s.undo.len(), undo, "{analysis}");
    // So a caller with read authority alone may run it; building stays an edit.
    let effect = |id: &str| crate::cmd::find_command(id).map(|c| c.effect);
    assert_eq!(effect("surface.midsurface_analyze"), Some(craft_plugin::Effect::Read));
    assert_eq!(effect("surface.midsurface"), Some(craft_plugin::Effect::Edit));
    check(output_schema_for::<MidsurfaceAnalysis>(), &analysis);
    let built = s
        .execute(
            "surface.midsurface",
            &json!({"body": "Plate", "max_thickness": "5 mm", "carry_holes": true, "stitch": false, "keep_original": true}),
        )
        .unwrap_or_else(|e| panic!("{e}"));
    check(output_schema_for::<FeatureResult>(), &built);
    let points = analysis.get("pairs").and_then(|p| p.get(0)).and_then(|p| p.get("points")).cloned().unwrap_or(Value::Null);
    let again = s
        .execute("surface.midsurface", &json!({"body": "Plate", "pairs": [points], "exclude": [], "min_aspect": 3}))
        .unwrap_or_else(|e| panic!("{e}"));
    check(output_schema_for::<FeatureResult>(), &again);
}
