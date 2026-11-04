use serde_json::{Value, json};

use super::{
    ParamAddParams, ParamAdded, ParamCommentParams, ParamCommented, ParamDeleted, ParamFavoriteParams, ParamFavorited, ParamMoveParams, ParamMoved,
    ParamNameParams, ParamRenameParams, ParamRenamed,
};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<ParamAddParams>(), &["comment", "expression", "favorite", "name", "unit", "value"]);
    assert_keys(input_schema_for::<ParamRenameParams>(), &["name", "new_name"]);
    assert_keys(input_schema_for::<ParamNameParams>(), &["name"]);
    assert_keys(input_schema_for::<ParamCommentParams>(), &["comment", "name"]);
    assert_keys(input_schema_for::<ParamFavoriteParams>(), &["favorite", "name"]);
    assert_keys(input_schema_for::<ParamMoveParams>(), &["after", "before", "name"]);
}

/// The calls fail on a design without parameters, with the handlers' messages or the
/// document's.
#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("parameters.add", json!({})),
        ("parameters.add", json!({"name": "  ", "expression": 1})),
        ("parameters.add", json!({"name": 5, "expression": 1})),
        ("parameters.add", json!({"name": "W"})),
        ("parameters.add", json!({"name": "W", "expression": true})),
        ("parameters.add", json!({"name": "W", "expression": null})),
        ("parameters.add", json!({"name": "W", "value": [1]})),
        ("parameters.add", json!({"name": "1W", "expression": 1})),
        ("parameters.add", json!({"name": "W", "expression": "1 +"})),
        ("parameters.add", json!({"name": "W", "expression": 1, "unit": "furlong"})),
        ("parameters.rename", json!({"new_name": "X"})),
        ("parameters.rename", json!({"name": "Nope", "new_name": "X"})),
        ("parameters.rename", json!({"name": "W"})),
        ("parameters.rename", json!({"name": "W", "new_name": 5})),
        ("parameters.delete", json!({})),
        ("parameters.delete", json!({"name": "Nope"})),
        ("parameters.comment", json!({"name": "Nope", "comment": "x"})),
        ("parameters.comment", json!({"name": "W"})),
        ("parameters.comment", json!({"name": "W", "comment": null})),
        ("parameters.comment", json!({"name": "W", "comment": 5})),
        ("parameters.favorite", json!({"name": []})),
        ("parameters.favorite", json!({"name": "Nope"})),
        ("parameters.move", json!({"name": "W"})),
        ("parameters.move", json!({"name": "W", "before": "A", "after": "B"})),
        ("parameters.move", json!({"name": "W", "before": 5})),
        ("parameters.move", json!({"name": "Nope", "before": "A"})),
    ]);
}

/// The schemas take what the handlers read: `expression` or `value` as a number or text (or
/// `null`, read as left out), and a non-blank name.
#[test]
fn the_schemas_take_what_the_handlers_read() {
    let add = input_schema_for::<ParamAddParams>();
    let add = add.as_value();
    for (p, takes) in [
        (json!({"name": "W", "expression": "10 mm"}), true),
        (json!({"name": "W", "expression": 10, "unit": "mm", "comment": "Wall", "favorite": true}), true),
        (json!({"name": "W", "value": 10}), true),
        (json!({"name": "W", "expression": true}), false),
        (json!({"name": "", "expression": 10}), false),
    ] {
        assert_eq!(conforms(add, add, &p), takes, "{p}");
    }
    // `conforms` doesn't read the `anyOf` of `required` lists, so it's pinned here.
    assert_eq!(add["anyOf"], json!([{"required": ["expression"]}, {"required": ["value"]}]), "{add}");
    let comment = input_schema_for::<ParamCommentParams>();
    let comment = comment.as_value();
    assert!(!conforms(comment, comment, &json!({"name": "W"})), "{comment}");
    assert!(!conforms(comment, comment, &json!({"name": "W", "comment": null})), "{comment}");
}

/// #11941: adding a number and a text parameter, then commenting, favouriting, moving,
/// renaming and deleting, reply as the output schemas say, in Warn without warnings. A
/// misspelt key gets `param_warnings` in Warn, since the handlers ignore unknown keys, and
/// strict refuses it.
#[test]
fn parameter_edits_reply_with_their_schemas() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let mut run = |id: &str, p: Value, schema: schemars::Schema| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        let schema = schema.as_value();
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{r}");
        r
    };
    let r = run("parameters.add", json!({"name": "W", "expression": "10 mm", "comment": "Wall"}), output_schema_for::<ParamAdded>());
    assert_eq!((r["unit"].as_str(), r["value"].as_f64(), r["display_value"].as_f64()), (Some("mm"), Some(10.0), Some(10.0)), "{r}");
    let r = run("parameters.add", json!({"name": "Label", "expression": "Rev A", "unit": "text"}), output_schema_for::<ParamAdded>());
    assert!(r["text"] == "Rev A" && r["value"].is_null() && r.get("display_value").is_none(), "{r}");
    run("parameters.add", json!({"name": "H", "value": 5, "unit": "mm", "favorite": true}), output_schema_for::<ParamAdded>());
    run("parameters.comment", json!({"name": "H", "comment": "Height"}), output_schema_for::<ParamCommented>());
    let r = run("parameters.favorite", json!({"name": "H", "favorite": false}), output_schema_for::<ParamFavorited>());
    assert_eq!(r["favorite"], false, "{r}");
    let r = run("parameters.move", json!({"name": "H", "before": "W"}), output_schema_for::<ParamMoved>());
    let at = |name: &str| r["order"].as_array().and_then(|order| order.iter().position(|n| n == name));
    assert!(at("H").is_some() && at("H") < at("W"), "{r}");
    let r = run("parameters.rename", json!({"name": "W", "new_name": " Width "}), output_schema_for::<ParamRenamed>());
    assert_eq!((r["from"].as_str(), r["to"].as_str()), (Some("W"), Some("Width")), "{r}");
    run("parameters.delete", json!({"name": "Label"}), output_schema_for::<ParamDeleted>());
    let p = json!({"name": "Width", "favourite": true});
    let mut warned = Session { param_check: ParamCheck::Warn, ..Session::default() };
    warned.execute("parameters.add", &json!({"name": "Width", "expression": 3})).unwrap_or_else(|e| panic!("{e}"));
    let r = warned.execute("parameters.favorite", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
    assert!(r["param_warnings"].to_string().contains("favourite"), "{r}");
    warned.param_check = ParamCheck::Strict;
    let error = warned.execute("parameters.favorite", &p).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains("`favourite`"), "{error}");
}
