use serde_json::{Value, json};

use super::{AngleReadoutParams, TabStep, TabStepParams};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<AngleReadoutParams>(), &["enabled"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("view.angle_readout", json!({"enabled": 1})),
        ("view.angle_readout", json!({"enabled": "true"})),
        ("view.angle_readout", json!({"enabled": null})),
    ]);
}

#[test]
fn angle_readout_replies_match_the_output_schema() {
    let schema = output_schema_for::<super::AngleReadoutResult>();
    let schema = schema.as_value();
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    for enabled in [true, false] {
        let r = s.execute("view.angle_readout", &json!({"enabled": enabled})).unwrap_or_else(|e| panic!("{e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert_eq!(r.get("param_warnings"), None, "{r}");
        assert_eq!(r["enabled"], Value::Bool(enabled));
    }
    assert!(!conforms(schema, schema, &json!({"enabled": "yes"})));
    assert!(!conforms(schema, schema, &json!({})));
}

/// #8565: the next and previous tab take nothing, say which way the window goes, and leave the
/// design and its undo as they were.
#[test]
fn next_and_previous_tab_echo_their_step_without_editing_the_design() {
    assert_keys(input_schema_for::<TabStepParams>(), &[]);
    let schema = output_schema_for::<TabStep>();
    let schema = schema.as_value();
    let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap_or_else(|e| panic!("{e}"));
    let (before, undo) = (s.doc.to_json(), s.undo.len());
    for (id, step) in [("view.next_tab", 1), ("view.previous_tab", -1)] {
        let r = s.execute(id, &json!({})).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(r, json!({"step": step}), "{id}");
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(s.execute(id, &json!({"step": 2})).is_err(), "{id} takes no parameters (strict check)");
    }
    assert_eq!((s.doc.to_json(), s.undo.len()), (before, undo), "nothing in the design changes");
}
