use serde_json::{Value, json};

use super::{
    AssistantParams, CheckpointParams, CheckpointResult, RedoParams, RedoResult, RepeatParams, RestoreCheckpointParams, SelectModeParams,
    SelectModeResult, UndoParams, UndoResult,
};
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};
use crate::{PickMode, Session};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<RepeatParams>(), &[]);
    assert_keys(input_schema_for::<AssistantParams>(), &[]);
    assert_keys(input_schema_for::<UndoParams>(), &[]);
    assert_keys(input_schema_for::<RedoParams>(), &[]);
    assert_keys(input_schema_for::<CheckpointParams>(), &[]);
    assert_keys(input_schema_for::<RestoreCheckpointParams>(), &["checkpoint", "forget"]);
    assert_keys(input_schema_for::<SelectModeParams>(), &["mode"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("edit.restore_checkpoint", json!({"checkpoint": "first"})),
        ("edit.restore_checkpoint", json!({"checkpoint": -1})),
        ("select.mode", json!({"mode": "edges"})),
        ("select.mode", json!({"mode": "Bodies"})),
        ("select.mode", json!({"mode": 1})),
        ("select.mode", json!({"mode": null})),
        ("select.mode", json!({"mode": ["faces"]})),
    ]);
}

#[test]
fn select_mode_replies_match_its_output_schema() {
    let schema = output_schema_for::<SelectModeResult>();
    let schema = schema.as_value();
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    for mode in PickMode::ALL {
        let r = s.execute("select.mode", &json!({"mode": mode})).unwrap_or_else(|e| panic!("{e}"));
        assert!(r.get("param_warnings").is_none(), "{r}");
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    }
    let r = s.execute("select.mode", &json!({"mode": "parts", "modes": "bodies"})).unwrap_or_else(|e| panic!("{e}"));
    assert!(r.get("param_warnings").is_some(), "{r}");
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    for bad in [json!({"mode": "edges"}), json!({"mode": 1}), json!({})] {
        assert!(!conforms(schema, schema, &bad), "{bad}");
    }
}

/// Undo, redo, a checkpoint, going back to it and forgetting one, in Warn: each reply matches
/// its output schema and carries no warning.
#[test]
fn replies_match_the_output_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    let matches = |schema: schemars::Schema, id: &str, reply: &Value| {
        let schema = schema.as_value();
        assert!(reply.get("param_warnings").is_none() && conforms(schema, schema, reply), "{id}: {reply} doesn't match {schema}");
    };
    run("solid.box", json!({"length": 10, "width": 10, "height": 10}));
    matches(output_schema_for::<UndoResult>(), "edit.undo", &run("edit.undo", json!({})));
    matches(output_schema_for::<RedoResult>(), "edit.redo", &run("edit.redo", json!({})));
    let taken = run("edit.checkpoint", json!({}));
    matches(output_schema_for::<CheckpointResult>(), "edit.checkpoint", &taken);
    run("solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [20, 0, 0]}));
    let back = run("edit.restore_checkpoint", json!({"checkpoint": taken["checkpoint"]}));
    assert_eq!(back["restored"], true, "{back}");
    matches(output_schema_for::<CheckpointResult>(), "edit.restore_checkpoint", &back);
    let again = run("edit.checkpoint", json!({}));
    let dropped = run("edit.restore_checkpoint", json!({"checkpoint": again["checkpoint"], "forget": true}));
    assert_eq!(dropped["restored"], false, "{dropped}");
    matches(output_schema_for::<CheckpointResult>(), "edit.restore_checkpoint", &dropped);
}
