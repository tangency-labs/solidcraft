use serde_json::json;

use super::AnalysisParams;
use crate::Session;
use crate::schema::input_schema_for;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<AnalysisParams>(), &["off"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[("inspect.analysis", json!({"off": "yes"})), ("inspect.analysis", json!({"off": 1}))]);
}

/// The view analyses are one group: turning one on turns the one on before off, and
/// `inspect.analysis` names the one on and turns it off.
#[test]
fn one_view_analysis_is_on_at_a_time() {
    let mut s = Session::default();
    s.execute("solid.cylinder", &json!({"radius": 5, "height": 10})).unwrap();
    let active = |s: &mut Session| s.execute("inspect.analysis", &json!({})).unwrap()["active"].clone();
    assert_eq!(active(&mut s), json!(null));
    s.execute("inspect.zebra", &json!({})).unwrap();
    assert_eq!(active(&mut s), json!("inspect.zebra"));
    s.execute("inspect.curvature_comb", &json!({"edges": [[0, 5, 10]]})).unwrap();
    assert_eq!(active(&mut s), json!("inspect.curvature_comb"));
    assert!(s.analysis.is_none(), "the zebra stripes went off");
    s.execute("inspect.isocurve", &json!({"faces": [[0, 5, 5]]})).unwrap();
    assert_eq!(active(&mut s), json!("inspect.isocurve"));
    assert!(s.comb.is_none(), "the combs went off");
    s.execute("inspect.continuity", &json!({})).unwrap();
    assert_eq!(active(&mut s), json!("inspect.continuity"));
    assert!(s.isocurves.is_empty(), "the isocurves went off");
    s.execute("inspect.draft", &json!({})).unwrap();
    assert_eq!(active(&mut s), json!("inspect.draft"));
    assert!(s.continuity.is_empty(), "the seams went off");
    // Turning on the comb with nothing picked yet.
    s.execute("inspect.curvature_comb", &json!({"add": true})).unwrap();
    assert_eq!(active(&mut s), json!("inspect.curvature_comb"));
    assert_eq!(s.execute("inspect.analysis", &json!({"off": true})).unwrap()["active"], json!(null));
    assert!(s.comb.is_none() && s.analysis.is_none());
}
