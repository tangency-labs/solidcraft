use serde_json::{Value, json};
use solidcraft_doc::drawing_view::ViewSettings;

use super::{BaseParams, CopyParams, EditParams, InspectParams, MoveParams, ProjectedParams, SnapshotParams, ViewSet};
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    let keys = [
        "chord_mm",
        "decimals",
        "display_style",
        "hide",
        "max_error_mm",
        "measure",
        "position_mm",
        "reference_hides_model",
        "reference_shown_percent",
        "reference_style",
        "scale",
        "scene",
        "sheet",
        "source",
        "tangent_edges",
        "units",
        "view",
    ];
    assert_keys(input_schema_for::<SnapshotParams>(), &keys);
    let base = [
        "display_style",
        "position_mm",
        "reference_hides_model",
        "reference_shown_percent",
        "reference_style",
        "scale",
        "sheet",
        "tangent_edges",
        "view",
    ];
    assert_keys(input_schema_for::<BaseParams>(), &base);
    assert_keys(input_schema_for::<InspectParams>(), &["view"]);
    assert_keys(input_schema_for::<EditParams>(), &["set", "view"]);
    assert_keys(input_schema_for::<MoveParams>(), &["break_alignment", "position_mm", "sheet", "view"]);
    assert_keys(input_schema_for::<CopyParams>(), &["position_mm", "sheet", "view", "with_linked"]);
    assert_keys(input_schema_for::<ProjectedParams>(), &["parent", "place", "side"]);
}

/// `drawing.view.edit` changes what a saved view sets itself, field for field, and its direction,
/// place and link, so the edit and the record keep one list of a view's settings.
#[test]
fn the_edit_contract_lists_a_saved_views_settings() {
    let all = solidcraft_doc::drawing_view::ViewOverrides {
        scale: "1:2".parse().ok(),
        display_style: Some(Default::default()),
        reference_style: Some(Default::default()),
        reference_hides_model: Some(false),
        tangent_edges: Some(Default::default()),
    };
    let own = serde_json::to_value(all).unwrap_or_default();
    let mut theirs: Vec<String> = own.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
    theirs.extend(["linked", "place", "reference_shown_percent", "view"].map(String::from));
    theirs.sort();
    let schema = input_schema_for::<ViewSet>();
    let mut ours: Vec<String> = schema.as_value()["properties"].as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
    ours.sort();
    assert_eq!(ours, theirs);
}

/// The contract takes the view's settings field for field, plus the sheet and the position, so
/// the snapshot and a saved view (W12) keep one parameter set.
#[test]
fn the_contract_lists_the_view_settings() {
    // With a scale: none is left out when written.
    let settings = serde_json::to_value(ViewSettings { scale: "1:2".parse().ok(), ..ViewSettings::default() }).unwrap_or_default();
    let mut theirs: Vec<String> = settings.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
    // Beside a view's settings, the request's own: where it goes, its page, its lines' chord and
    // accuracy check, whether it wants the paper scene, what it draws, and what it reads.
    theirs.extend(["position_mm", "sheet", "chord_mm", "max_error_mm", "scene", "source", "hide", "measure", "units", "decimals"].map(String::from));
    theirs.sort();
    let schema = input_schema_for::<SnapshotParams>();
    let mut ours: Vec<String> = schema.as_value()["properties"].as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
    ours.sort();
    assert_eq!(ours, theirs);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("drawing.view.snapshot", json!({"scale": "0:1"})),
        ("drawing.view.snapshot", json!({"view": "Front"})),
        ("drawing.view.snapshot", json!({"display_style": "shaded_visible"})),
        ("drawing.view.snapshot", json!({"sheet": {"dpi": 1e9}})),
    ]);
}

#[test]
fn exact_sheet_segments_do_not_broaden_projected_view_paths() {
    use crate::schema::drawing_scene::SheetPathSegResult;
    use solidcraft_geom::{MAX_BEZIER_CTRL, Seg2, Vec2};
    let projected = output_schema_for::<super::PathSegResult>();
    let sheet = output_schema_for::<SheetPathSegResult>();
    let accepts = |schema: &schemars::Schema, value: &Value| conforms(schema.as_value(), schema.as_value(), value);
    for segment in [Seg2::Line { a: Vec2::ZERO, b: Vec2::X }, Seg2::Arc { center: Vec2::ZERO, radius: 5.0, start: 0.0, sweep: 1.0 }] {
        let value = serde_json::to_value(segment).unwrap();
        assert!(accepts(&projected, &value) && accepts(&sheet, &value), "{value}");
    }
    let mut controls = [Vec2::ZERO; MAX_BEZIER_CTRL];
    for (i, point) in controls.iter_mut().take(6).enumerate() {
        *point = Vec2::new(i as f64, (i % 2) as f64);
    }
    for segment in [
        Seg2::Cubic { p0: Vec2::ZERO, p1: Vec2::Y, p2: Vec2::new(1.0, 1.0), p3: Vec2::X },
        Seg2::Conic { a: Vec2::ZERO, apex: Vec2::Y, b: Vec2::X, w: 2.0 / 3.0 },
        Seg2::Bezier { n: 5, p: controls },
    ] {
        let value = serde_json::to_value(segment).unwrap();
        assert!(accepts(&sheet, &value), "{value}");
        assert!(!accepts(&projected, &value), "projected-view line/arc contract broadened: {value}");
        let mut extra = value.clone();
        extra["unexpected"] = json!(true);
        assert!(!accepts(&sheet, &extra), "{extra}");
    }
    let mut bezier = serde_json::to_value(Seg2::Bezier { n: 5, p: controls }).unwrap();
    for degree in [3, 10] {
        bezier["n"] = json!(degree);
        assert!(!accepts(&sheet, &bezier), "{bezier}");
    }
    bezier["n"] = json!(5);
    bezier["p"].as_array_mut().unwrap().pop();
    assert!(!accepts(&sheet, &bezier), "all serialized control slots are required: {bezier}");
    assert!(!accepts(&sheet, &json!({"type": "cubic", "p0": [0,0], "p1": [0,1], "p2": [1,1]})));
    assert!(!accepts(&sheet, &json!({"type": "conic", "a": [0,0], "apex": [0,1], "b": [1,0], "w": 0})));
}
