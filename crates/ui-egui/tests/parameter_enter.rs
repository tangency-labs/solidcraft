//! Enter submits only the focused Parameters editor row.
use egui::{Event, Key, Modifiers};
use serde_json::json;
use solidcraft_ui_egui::scenario::Harness;

fn open() -> Harness {
    let mut h = Harness::new();
    // The Browser's Parameters row starts this command; the ribbon shows only the active tab, so no
    // Parameters button is on screen to click.
    h.call("ui.start", json!({"command":"parameters.change"}));
    h.frames(3);
    assert!(h.app.params.is_some());
    h.step(&json!({"click":{"handle":"dialog:table:user:add"}})).unwrap();
    h
}
fn fill(h: &mut Harness, field: &str, text: &str) {
    h.run(&json!([
        {"click":{"handle":format!("param:new:{field}")}},
        {"key":"A","cmd":true}, {"text":text}
    ]))
    .unwrap();
}
fn enter(h: &mut Harness, repeat: bool) {
    h.app.synthetic.push(Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat, modifiers: Modifiers::NONE });
    h.frame();
    h.app.synthetic.push(Event::Key { key: Key::Enter, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
    h.frames(3);
}
#[test]
fn parameter_enter_adds_once_and_focuses_next_name() {
    let mut h = open();
    fill(&mut h, "expression", "25 mm");
    fill(&mut h, "name", "width");
    let undo = h.app.session.undo.len();
    enter(&mut h, false);
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert!(h.app.params.as_ref().unwrap().new_name.is_empty());
    assert!(h.app.params.as_ref().unwrap().error.is_none());
    let saved = h.app.session.doc.to_json();
    enter(&mut h, true);
    assert_eq!(h.app.session.doc.to_json(), saved);
    assert_eq!(h.app.session.undo.len(), undo + 1);
    // Name has focus after successful creation; no mouse needed for the next name.
    h.run(&json!([{"text":"height"}])).unwrap();
    assert_eq!(h.app.params.as_ref().unwrap().new_name, "height");
    // The new row's expression starts blank: Enter after the name goes on to it, and Enter
    // there adds the parameter.
    enter(&mut h, false);
    assert_eq!(h.app.session.undo.len(), undo + 1, "nothing added without an expression");
    h.run(&json!([{"text":"10 mm"}])).unwrap();
    assert_eq!(h.app.params.as_ref().unwrap().new_expr, "10 mm");
    enter(&mut h, false);
    assert_eq!(h.app.session.undo.len(), undo + 2);
    let result = h.app.session.execute("parameters.list", &json!({})).unwrap();
    assert!(result["parameters"].as_array().unwrap().iter().any(|p| p["name"] == "height"));
}
#[test]
fn parameter_enter_invalid_preserves_draft_and_other_rows() {
    let mut h = open();
    h.app.session.execute("parameters.add", &json!({"name":"existing","expression":"10 mm"})).unwrap();
    h.frames(3);
    fill(&mut h, "expression", "missing + 1 mm");
    fill(&mut h, "name", "bad");
    let saved = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    enter(&mut h, false);
    assert_eq!(h.app.session.doc.to_json(), saved);
    assert_eq!(h.app.session.undo.len(), undo);
    let d = h.app.params.as_ref().unwrap();
    assert_eq!(d.new_name, "bad");
    assert_eq!(d.new_expr, "missing + 1 mm");
    assert!(d.error.is_some());
    fill(&mut h, "name", "existing");
    fill(&mut h, "expression", "15 mm");
    enter(&mut h, false);
    assert_eq!(h.app.session.doc.to_json(), saved);
    assert!(h.app.params.as_ref().unwrap().error.is_some());
}
#[test]
fn parameter_enter_edits_expression_without_adding_draft() {
    let mut h = open();
    h.app.session.execute("parameters.add", &json!({"name":"width","expression":"10 mm"})).unwrap();
    h.frames(3);
    // A named draft with an invalid expression stays pending on focus loss.
    fill(&mut h, "expression", "missing + 1 mm");
    fill(&mut h, "name", "unsubmitted");
    let undo = h.app.session.undo.len();
    h.run(&json!([{"click":{"handle":"param:width"}}, {"key":"A","cmd":true}, {"text":"20 mm"}])).unwrap();
    enter(&mut h, false);
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert_eq!(h.app.params.as_ref().unwrap().new_name, "unsubmitted");
    assert!(h.app.params.as_ref().unwrap().error.is_none(), "Enter in the existing row never attempts the invalid draft");
    let result = h.app.session.execute("parameters.list", &json!({})).unwrap();
    let ps = result["parameters"].as_array().unwrap();
    assert!(ps.iter().any(|p| p["name"] == "width" && p["expression"] == "20 mm"));
    assert!(!ps.iter().any(|p| p["name"] == "unsubmitted"));
    let saved = h.app.session.doc.to_json();
    h.run(&json!([{"click":{"handle":"param:width"}}, {"key":"A","cmd":true}, {"text":"missing + 2 mm"}])).unwrap();
    enter(&mut h, false);
    assert_eq!(h.app.session.doc.to_json(), saved);
    assert!(h.app.params.as_ref().unwrap().error.is_some());
    assert_eq!(h.app.params.as_ref().unwrap().editing.get("width").map(String::as_str), Some("missing + 2 mm"));
}

#[test]
fn parameter_enter_comment_uses_selected_unit_and_unrelated_focus_does_not_submit() {
    let mut h = open();
    h.run(&json!([{"click":{"handle":"param:new:unit"}}, {"widget":"cm"}])).unwrap();
    fill(&mut h, "expression", "2 cm");
    fill(&mut h, "comment", "Overall length");
    fill(&mut h, "expression", "missing + 1 mm");
    fill(&mut h, "name", "length");
    let undo = h.app.session.undo.len();
    h.run(&json!([{"click":{"handle":"params:title"}}])).unwrap();
    // Make the pending expression valid while unrelated focus is retained, without firing
    // the newly approved focus-loss commit route. Unrelated Enter must still do nothing.
    h.app.params.as_mut().unwrap().new_expr = "2 cm".into();
    enter(&mut h, false);
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(h.app.params.as_ref().unwrap().error.is_none(), "unrelated Enter never attempts the pending draft");
    h.run(&json!([{"click":{"handle":"param:new:comment"}}])).unwrap();
    enter(&mut h, false);
    let result = h.app.session.execute("parameters.list", &json!({})).unwrap();
    assert!(result["parameters"].as_array().unwrap().iter().any(|p| p["name"] == "length" && p["unit"] == "cm" && p["comment"] == "Overall length"));
    assert_eq!(h.app.session.undo.len(), undo + 1);
}
#[test]
fn parameter_rename_arrow_keys_move_cursor_and_keep_editing() {
    let mut h = open();
    h.app.session.execute("parameters.add", &json!({"name":"width","expression":"10 mm"})).unwrap();
    // Let the click that opened the dialog age, or egui counts the double-click as a triple.
    h.frames(30);
    h.run(&json!([{"click":{"handle":"param:name:width"},"double":true}])).unwrap();
    h.frames(2);
    let renaming = |h: &Harness| h.app.params.as_ref().unwrap().renaming.clone();
    assert_eq!(renaming(&h), Some(("width".to_string(), "width".to_string())));
    // Move the cursor to the start and type there; every arrow key keeps the rename open.
    h.run(&json!([{"key":"ArrowLeft"}, {"key":"ArrowUp"}, {"key":"ArrowDown"}, {"key":"ArrowRight"}, {"key":"Home"}, {"text":"plate_"}])).unwrap();
    assert_eq!(renaming(&h), Some(("width".to_string(), "plate_width".to_string())));
    enter(&mut h, false);
    assert_eq!(renaming(&h), None);
    let result = h.app.session.execute("parameters.list", &json!({})).unwrap();
    assert!(result["parameters"].as_array().unwrap().iter().any(|p| p["name"] == "plate_width"));
}
/// #3965: leaving an invalid name and clicking back into it must edit the name.
/// Refusing the add used to put focus back on the field being left, so the
/// correction was typed into the expression instead.
#[test]
fn invalid_parameter_name_stays_editable_after_leaving_it() {
    let mut h = open();
    fill(&mut h, "name", "bad name");
    h.run(&json!([
        {"click":{"handle":"param:new:expression"}},
        {"click":{"handle":"param:new:name"}},
        {"key":"A","cmd":true},
        {"text":"width"}
    ]))
    .unwrap();
    let d = h.app.params.as_ref().unwrap();
    assert_eq!(d.new_name, "width", "typing after returning to Name edits the name");
    assert_eq!(d.new_expr, "10 mm", "the expression keeps the draft");
    let result = h.app.session.execute("parameters.list", &json!({})).unwrap();
    assert!(!result["parameters"].as_array().unwrap().iter().any(|p| p["name"] == "bad name" || p["name"] == "width"));
}

#[test]
fn parameter_add_resets_expression_for_next_row() {
    let mut h = open();
    let initial = h.app.params.as_ref().unwrap().new_expr.clone();
    fill(&mut h, "expression", "25 mm");
    fill(&mut h, "name", "width");
    enter(&mut h, false);
    let result = h.app.session.execute("parameters.list", &json!({})).unwrap();
    assert!(result["parameters"].as_array().unwrap().iter().any(|p| p["name"] == "width" && p["expression"] == "25 mm"));
    let d = h.app.params.as_ref().unwrap();
    assert!(d.new_name.is_empty());
    assert_eq!(d.new_expr, initial);
}
