use serde_json::json;

use super::*;
use crate::schema::tests::{assert_keys, conforms, drawing_session};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    let create = ["font", "language", "line_group", "name", "projection", "template", "units"];
    assert_keys(input_schema_for::<DrawingCreateParams>(), &create);
    let set = ["font", "language", "line_group", "name", "projection", "revision_numbering", "units", "updates"];
    assert_keys(input_schema_for::<DrawingSetParams>(), &set);
    assert_keys(input_schema_for::<TemplateListParams>(), &[]);
    assert_keys(input_schema_for::<TemplateApplyParams>(), &["dry_run", "template"]);
    assert_keys(input_schema_for::<TemplateSaveParams>(), &["path"]);
}

/// A template saved as a file, then applied from it, matches the output schemas with no warnings
/// in warn mode; strict refuses a path that isn't a .json file as the command does.
#[test]
fn the_template_save_reply_matches_its_output_schema() {
    let (mut s, _) = drawing_session(ParamCheck::Warn);
    let folder = crate::tests::Folder::new("drawing-template-schema");
    let path = folder.path("shop.json");
    let saved = output_schema_for::<TemplateSaveResult>();
    let r = s.execute("drawing.template.save", &json!({"path": path})).unwrap();
    assert!(conforms(saved.as_value(), saved.as_value(), &r) && r.get("param_warnings").is_none(), "{r}");
    let applied = output_schema_for::<TemplateApplyResult>();
    let r = s.execute("drawing.template.apply", &json!({"template": path, "dry_run": true})).unwrap();
    assert!(conforms(applied.as_value(), applied.as_value(), &r) && r.get("param_warnings").is_none(), "{r}");
    let error = |level| drawing_session(level).0.execute("drawing.template.save", &json!({"path": "shop.txt"})).err().map(|e| e.to_string());
    assert_eq!(error(ParamCheck::Strict), error(ParamCheck::Off));
}

/// A call that already fails gets the same error in strict as with the check off.
#[test]
fn strict_keeps_todays_errors() {
    let calls = [
        ("drawing.template.apply", json!({"template": "iso-b2-part"})),
        ("drawing.template.apply", json!({"template": ""})),
        ("drawing.create", json!({"units": 3})),
        ("drawing.create", json!({"units": "parsec"})),
        ("drawing.create", json!({"name": 7})),
        ("drawing.create", json!({"projection": "second"})),
        ("drawing.create", json!({"line_group": "0.6"})),
        ("drawing.create", json!({"font": "a.ttf"})),
        ("drawing.create", json!({"template": "iso-b2-part"})),
        ("drawing.set", json!({"updates": "later"})),
        ("drawing.set", json!({"line_group": null})),
        ("drawing.set", json!({"name": null})),
        ("drawing.set", json!({"language": "e"})),
        ("drawing.set", json!({"revision_numbering": {"start": 2}})),
        ("drawing.set", json!({"revision_numbering": {"begin": 1}})),
    ];
    for (id, p) in calls {
        let error = |level| drawing_session(level).0.execute(id, &p).err().map(|e| e.to_string());
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{id} {p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{id} {p}");
    }
}

/// `updates` belongs to `drawing.set`: a new drawing takes its template's, so `drawing.create` refuses
/// the key at every check level, adding no drawing and no undo step.
#[test]
fn create_refuses_the_set_only_updates_key() {
    for level in [ParamCheck::Off, ParamCheck::Strict] {
        let (mut s, _) = drawing_session(level);
        let (drawings, steps) = (s.doc.drawings.len(), s.undo.len());
        let e = s.execute("drawing.create", &json!({"updates": "deferred"})).unwrap_err().to_string();
        // Strict's contract check speaks first ("unknown key"), the schema reader otherwise ("unknown field").
        assert!(e.contains("unknown") && e.contains("`updates`"), "{level:?}: {e}");
        assert_eq!((s.doc.drawings.len(), s.undo.len()), (drawings, steps), "{level:?}: nothing changed");
    }
}

/// Every key `drawing.create` and `drawing.set` take, in one call each: no warnings in warn mode, so
/// the contracts take what the commands do.
#[test]
fn the_drawing_settings_contracts_take_every_key() {
    let (mut s, _) = drawing_session(ParamCheck::Warn);
    let set = json!({"name": "Plate 2", "language": "en-GB", "units": "in", "projection": "first", "updates": "deferred", "line_group": "0.7", "font": "", "revision_numbering": {"start": 0}});
    assert!(s.execute("drawing.set", &set).unwrap().get("param_warnings").is_none());
    s.execute("drawing.close", &json!({})).unwrap();
    let create = json!({"name": "Plate 3", "template": "iso-a4-part", "language": "de", "units": "follow_model", "projection": "third", "line_group": "1", "font": ""});
    assert!(s.execute("drawing.create", &create).unwrap().get("param_warnings").is_none());
}

/// Applying a template, as a dry run and for real, matches the output schema with no warnings in
/// warn mode.
#[test]
fn the_template_apply_reply_matches_its_output_schema() {
    let (mut s, _) = drawing_session(ParamCheck::Warn);
    let schema = output_schema_for::<TemplateApplyResult>();
    let schema = schema.as_value();
    for p in [json!({"template": "iso-a2-part", "dry_run": true}), json!({"template": "iso-a2-part"}), json!({"template": "iso-a2-part"})] {
        let r = s.execute("drawing.template.apply", &p).unwrap();
        assert!(conforms(schema, schema, &r) && r.get("param_warnings").is_none(), "{p}: {r} doesn't match {schema}");
    }
}

/// The list matches its output schema, with no warnings in warn mode; strict refuses a key it
/// doesn't take.
#[test]
fn the_template_list_matches_its_output_schema() {
    let (mut s, _) = drawing_session(ParamCheck::Warn);
    let schema = output_schema_for::<TemplateListResult>();
    let schema = schema.as_value();
    let r = s.execute("drawing.template.list", &json!({})).unwrap();
    assert!(conforms(schema, schema, &r) && r.get("param_warnings").is_none(), "{r} doesn't match {schema}");
    assert_eq!(r["templates"].as_array().map(Vec::len), Some(10));
    s.param_check = ParamCheck::Strict;
    assert!(s.execute("drawing.template.list", &json!({"kind": "part"})).is_err());
}
