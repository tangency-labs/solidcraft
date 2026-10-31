use serde_json::json;
use solidcraft_doc::drawing_view::ViewSettings;

use super::{BaseParams, CopyParams, EditParams, InspectParams, MoveParams, ProjectedParams, SnapshotParams, ViewSet};
use crate::schema::input_schema_for;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors};

#[test]
fn schemas_and_the_key_check_agree() {
    let keys = [
        "chord_mm",
        "display_style",
        "hide",
        "max_error_mm",
        "position_mm",
        "reference_hides_model",
        "reference_margin_mm",
        "reference_style",
        "scale",
        "scene",
        "sheet",
        "source",
        "tangent_edges",
        "view",
    ];
    assert_keys(input_schema_for::<SnapshotParams>(), &keys);
    let base = [
        "display_style",
        "position_mm",
        "reference_hides_model",
        "reference_margin_mm",
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
    theirs.extend(["linked", "place", "reference_margin_mm", "view"].map(String::from));
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
    // accuracy check, whether it wants the paper scene, and what it draws.
    theirs.extend(["position_mm", "sheet", "chord_mm", "max_error_mm", "scene", "source", "hide"].map(String::from));
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
        ("drawing.view.snapshot", json!({"display_style": "shaded"})),
        ("drawing.view.snapshot", json!({"sheet": {"dpi": 1e9}})),
    ]);
}
