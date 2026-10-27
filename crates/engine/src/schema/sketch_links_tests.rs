use serde_json::{Value, json};

use super::{
    FreezeLinkResult, LinkVisibilityParams, LinkVisibilityResult, LinksParams, ReferencesParams, ReferencesResult, RemoveLinkResult,
    RepairLinkParams, RepairLinkResult, ResumeLinkResult, UpdateReferencesParams, UpdateReferencesResult,
};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<ReferencesParams>(), &["sketch"]);
    assert_keys(input_schema_for::<UpdateReferencesParams>(), &["capture", "files", "sketch"]);
    assert_keys(input_schema_for::<LinksParams>(), &["entities", "link", "links", "sketch"]);
    assert_keys(input_schema_for::<LinkVisibilityParams>(), &["entities", "link", "links", "sketch", "visible"]);
    assert_keys(input_schema_for::<RepairLinkParams>(), &["body", "link", "ref"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("sketch.references", json!({"sketch": true})),
        ("sketch.references", json!({"sketch": null})),
        ("sketch.update_references", json!({"sketch": [1]})),
        ("sketch.freeze_link", json!({"sketch": 1, "links": "a"})),
        ("sketch.resume_link", json!({"sketch": 1, "entities": [1]})),
        ("sketch.remove_link", json!({"sketch": 1, "links": null})),
        ("sketch.link_visibility", json!({"sketch": 1, "entities": "c1"})),
    ]);
}

/// A 40 × 30 × 10 box and a sketch on its top face's plane with the face projected, in Warn.
/// Returns the session and the link.
fn projected() -> (Session, String) {
    let mut s = Session::default();
    let run = |s: &mut Session, id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "body_name": "B"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 10}));
    let r = run(&mut s, "sketch.project", json!({"refs": [{"body": "B", "face": [5, 5, 10]}]}));
    s.param_check = ParamCheck::Warn;
    (s, r["links"][0]["link"].as_str().unwrap_or_default().to_string())
}

#[test]
fn replies_match_the_output_schema() {
    let (mut s, link) = projected();
    let check = |s: &mut Session, id: &str, p: Value, schema: schemars::Schema| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
        let schema = schema.as_value();
        assert!(r.get("param_warnings").is_none(), "{id} {p}: {r}");
        assert!(conforms(schema, schema, &r), "{id}: {r} doesn't match {schema}");
    };
    check(&mut s, "sketch.references", json!({}), output_schema_for::<ReferencesResult>());
    check(&mut s, "sketch.freeze_link", json!({"link": link}), output_schema_for::<FreezeLinkResult>());
    check(&mut s, "sketch.resume_link", json!({"links": [link]}), output_schema_for::<ResumeLinkResult>());
    check(&mut s, "sketch.link_visibility", json!({"link": link, "visible": false}), output_schema_for::<LinkVisibilityResult>());
    check(&mut s, "sketch.link_visibility", json!({"link": link}), output_schema_for::<LinkVisibilityResult>());
    check(&mut s, "sketch.repair_link", json!({"link": link, "ref": {"body": "B", "face": [5, 5, 10]}}), output_schema_for::<RepairLinkResult>());
    check(&mut s, "sketch.update_references", json!({"capture": true, "files": true}), output_schema_for::<UpdateReferencesResult>());
    check(&mut s, "sketch.finish", json!({}), schemars::json_schema!(true));
    check(&mut s, "sketch.references", json!({"sketch": 2}), output_schema_for::<ReferencesResult>());
    check(&mut s, "sketch.update_references", json!({"sketch": 2}), output_schema_for::<UpdateReferencesResult>());
    check(&mut s, "sketch.remove_link", json!({"sketch": 2, "link": link}), output_schema_for::<RemoveLinkResult>());
    // A misspelt key is reported, and the reply with its warnings still matches.
    let (mut s, link) = projected();
    let r = s.execute("sketch.freeze_link", &json!({"link": link, "lnks": []})).unwrap_or_else(|e| panic!("{e}"));
    let schema = output_schema_for::<FreezeLinkResult>();
    assert!(r["param_warnings"].as_array().is_some_and(|w| !w.is_empty()), "{r}");
    assert!(conforms(schema.as_value(), schema.as_value(), &r), "{r}");
}
