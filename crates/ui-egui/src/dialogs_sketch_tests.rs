//! Every legacy sketch popup uses the same card as the feature dialogs.
use super::*;
use crate::scenario::{self, Harness};
use crate::sketch_modify_ui::Mod;

fn fixture() -> Harness {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[10,10],"p1":[40,30]})).unwrap();
    h.frames(3);
    h
}

fn picked(h: &Harness, command: &str) -> Dialog {
    let mut d = Dialog::for_command(&h.app, command).unwrap();
    let sk = h.app.session.doc.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    if let Kind::SketchModify(k) = &d.kind {
        d.inputs[0].items = vec![if matches!(k, Mod::Corner { .. }) {
            Sel::SketchPoint { id: sk.points.iter().find(|p| p.id != "origin").unwrap().id.clone() }
        } else {
            Sel::SketchCurve { id: sk.curves[0].id.clone() }
        }];
        if matches!(k, Mod::Mirror) {
            d.inputs[1].items = vec![Sel::SketchCurve { id: sk.curves[3].id.clone() }];
        }
        if matches!(k, Mod::PatternCirc { .. }) {
            d.inputs[1].items = vec![Sel::SketchPoint { id: "origin".into() }];
        }
    }
    if let Kind::SketchModify(Mod::Move { x, .. }) = &mut d.kind {
        *x = "2 mm".into();
    }
    d
}

fn card(h: &Harness) {
    let outer = scenario::rect_of("dialog:card").unwrap();
    for key in ["dialog:header", "dialog:hint:tool", "dialog:footer"] {
        assert!(outer.expand(1.0).contains_rect(scenario::rect_of(key).unwrap()), "{key}");
    }
    for key in ["dialog:close", "dialog:ok", "dialog:cancel"] {
        assert!(outer.contains(scenario::handle_at(key).unwrap()), "{key}");
    }
    if let Some(palette) = h.app.viewport.palette_rect {
        assert!(!palette.intersects(outer), "{outer:?} / {palette:?}");
    }
}

fn form(command: &str, labels: &[&str]) {
    let mut h = fixture();
    let d = picked(&h, command);
    let expected = apply_commands(&h.app, &d).unwrap();
    assert_eq!(expected[0].0, command);
    assert_eq!(super::command(&d.kind), Some(command));
    let before = h.app.session.doc.to_json();
    h.app.dialog = Some(d);
    h.frames(6);
    card(&h);
    let d = h.app.dialog.as_ref().unwrap();
    assert_eq!(apply_commands(&h.app, d).unwrap(), expected, "rendering retains all command parameters");
    assert_eq!(h.app.session.doc.to_json(), before, "preview never commits");
    for label in d.inputs.iter().map(|i| i.label).chain(labels.iter().copied()) {
        let rect = scenario::rect_of(&format!("dialog:row:{label}")).unwrap_or_else(|| panic!("{command}: missing {label}"));
        assert_eq!(rect.height(), Tokens::get().control_height);
    }
    let spec = solidcraft_engine::find_command(command).unwrap();
    assert!(h.widget(spec.label).is_some(), "header uses command label");
    assert!(!spec.icon.is_empty());
    h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
    assert!(h.app.dialog.is_none());
    assert_eq!(h.app.session.doc.to_json(), before);
}

macro_rules! form_case {
    ($name:ident, $id:literal, [$($label:literal),*]) => {
        #[test] fn $name() { form($id, &[$($label),*]); }
    };
}

form_case!(scale, "sketch.scale", ["Factor"]);
form_case!(offset, "sketch.offset", ["Direction", "Distance"]);
form_case!(mirror, "sketch.mirror", []);
form_case!(move_copy, "sketch.move", ["Position", "Angle"]);
form_case!(rectangular_pattern, "sketch.pattern.rectangular", ["Quantity", "Spacing", "Alignment", "Quantity 2", "Spacing 2", "Alignment 2"]);
form_case!(circular_pattern, "sketch.pattern.circular", ["Quantity", "Total angle", "Alignment"]);
form_case!(fillet, "sketch.fillet", ["Radius"]);
form_case!(chamfer_equal, "sketch.chamfer.equal_distance", ["Chamfer Type", "Distance"]);
form_case!(chamfer_two, "sketch.chamfer.two_distance", ["Chamfer Type", "Distance 1", "Distance 2"]);
form_case!(chamfer_angle, "sketch.chamfer.distance_angle", ["Chamfer Type", "Distance 1", "Angle"]);

#[test]
fn create_sketch_uses_card_and_still_starts_immediately_on_plane_pick() {
    let mut h = Harness::new();
    h.app.start("sketch.create");
    h.frames(4);
    card(&h);
    assert!(scenario::rect_of("dialog:field:Plane").is_some());
    assert_eq!(scenario::count_of("dialog:ok:enabled"), Some(0.0));
    h.step(&json!({"click":{"plane":"XY"}})).unwrap();
    assert!(h.app.dialog.is_none());
    assert!(h.app.session.active_sketch.is_some());
}

#[test]
fn apply_commits_once_keeps_values_and_waits_for_new_selection_ok_closes() {
    for command in ["sketch.offset", "sketch.mirror", "sketch.pattern.rectangular", "sketch.pattern.circular"] {
        let mut h = fixture();
        h.app.dialog = Some(picked(&h, command));
        h.settle();
        let before = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        assert_eq!(scenario::count_of("dialog:ok:enabled"), Some(1.0), "{command}");
        let undo = h.app.session.undo.len();
        h.step(&json!({"click":{"handle":"dialog:apply"}})).unwrap();
        h.settle();
        assert_eq!(h.app.session.undo.len(), undo + 1, "{command}: one Apply, one undo");
        let d = h.app.dialog.as_ref().unwrap();
        assert!(d.inputs.iter().all(|i| i.items.is_empty()));
        assert_eq!(d.active, 0);
        // Repick the original input; the numeric values are unchanged.
        let next = picked(&h, command);
        h.app.dialog.as_mut().unwrap().inputs = next.inputs;
        let after = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        assert_eq!(before, after);
        h.settle();
        h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none(), "{command}: OK closes");
    }
}

#[test]
fn picked_list_removes_one_item_and_field_still_activates_input() {
    let mut h = fixture();
    let mut d = picked(&h, "sketch.scale");
    let second = h.app.session.doc.sketch(1).unwrap().curves[1].id.clone();
    d.inputs[0].items.push(Sel::SketchCurve { id: second.clone() });
    h.app.dialog = Some(d);
    h.frames(4);
    h.step(&json!({"click":{"handle":"dialog:list:Entities:0:remove"}})).unwrap();
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items, vec![Sel::SketchCurve { id: second }]);
    h.step(&json!({"click":{"handle":"dialog:Base point"}})).unwrap();
    assert_eq!(h.app.dialog.as_ref().unwrap().active, 1);
}

#[test]
fn panel_dimension_preserves_expression_and_command_and_ok_closes() {
    let mut h = fixture();
    h.app.run("parameters.add", json!({"name":"span","expression":"3 mm"})).unwrap();
    h.app.dialog = Some(Dialog::edit_param(&h.app.session, "span"));
    h.frames(4);
    card(&h);
    assert!(h.ctx.memory(|m| m.focused().is_some()), "single-value popup takes keyboard focus on open");
    h.step(&json!({"click":{"handle":"dialog:span"}})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"2 * 5 mm"})).unwrap();
    let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(commands, vec![("parameters.change".into(), json!({"name":"span","expression":"2 * 5 mm"}))]);
    h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    assert!(h.app.dialog.is_none());
    assert_eq!(h.app.session.doc.param("span").unwrap().expr, "2 * 5 mm");
}

#[test]
fn text_and_text_move_cards_preserve_parameters_and_cancel_live_edits() {
    let mut h = fixture();
    let out = h.app.run("sketch.text", json!({"text":"II","at":[50,20],"height":8})).unwrap();
    h.app.session.selection = vec![Sel::SketchCurve { id: out["curves"][0].as_str().unwrap().into() }];
    h.app.start("sketch.edit_text");
    h.frames(4);
    card(&h);
    for label in ["Text", "Font", "Style", "Alignment", "Position", "Height", "Angle", "Letter Spacing", "Line Spacing", "Path"] {
        assert!(scenario::rect_of(&format!("dialog:row:{label}")).is_some(), "{label}");
    }
    let before = h.app.session.doc.to_json();
    h.step(&json!({"text":"Changed"})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), before);
    h.app.session.selection = vec![Sel::SketchCurve { id: out["curves"][0].as_str().unwrap().into() }];
    h.app.start("sketch.move");
    h.frames(4);
    card(&h);
    assert!(matches!(h.app.dialog.as_ref().unwrap().kind, Kind::TextMove(_)));
    assert!(scenario::handle_at("dialog:Distance X").is_some());
    assert!(scenario::handle_at("dialog:Distance Y").is_some());
    assert!(scenario::handle_at("dialog:Create Copy").is_some());
    h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    assert!(h.app.dialog.is_none());
}

#[test]
fn apply_keeps_the_panel_open_after_asynchronous_evaluation() {
    let mut h = fixture();
    let mut d = picked(&h, "sketch.pattern.rectangular");
    let cmds = apply_commands(&h.app, &d).unwrap();
    d.repeat_after_apply = true;
    h.app.dialog = Some(d);
    let (finish, done) = std::sync::mpsc::channel();
    h.app.applying = Some(Applying::computing(done, cmds));
    let undo = h.app.session.undo.len();
    h.frames(2);
    assert_eq!(h.app.session.undo.len(), undo);
    finish.send(None).unwrap();
    h.frames(3);
    assert!(h.app.applying.is_none());
    let d = h.app.dialog.as_ref().unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert!(d.inputs.iter().all(|i| i.items.is_empty()));
    assert!(!d.repeat_after_apply);
}

#[test]
fn circular_pattern_quantity_arrows_step_the_count_within_its_limits_and_keep_parameters() {
    let mut h = fixture();
    h.app.dialog = Some(picked(&h, "sketch.pattern.circular"));
    h.settle();
    let count = |h: &Harness| match &h.app.dialog.as_ref().unwrap().kind {
        Kind::SketchModify(Mod::PatternCirc { count, .. }) => count.clone(),
        other => panic!("not a sketch pattern window: {other:?}"),
    };
    let set = |h: &mut Harness, text: &str| {
        if let Some(Kind::SketchModify(Mod::PatternCirc { count, .. })) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            *count = text.into();
        }
        h.frames(2);
    };
    let sent = |h: &Harness| apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap()[0].1["count"].clone();
    let previewed = |h: &Harness| crate::dialogs::preview_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap()[0].1["count"].clone();
    assert_eq!(count(&h), "6");
    h.step(&json!({"click":{"handle":"dialog:Quantity up"}})).unwrap();
    assert_eq!(count(&h), "7");
    assert_eq!((sent(&h), previewed(&h)), (json!(7), json!(7)), "the preview follows the arrow as it follows typing");
    h.step(&json!({"click":{"handle":"dialog:Quantity down"}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:Quantity down"}})).unwrap();
    assert_eq!(count(&h), "5");
    // A circular pattern needs the original and one copy, and makes at most 500.
    set(&mut h, "2");
    h.step(&json!({"click":{"handle":"dialog:Quantity down"}})).unwrap();
    assert_eq!(count(&h), "2");
    set(&mut h, "500");
    h.step(&json!({"click":{"handle":"dialog:Quantity up"}})).unwrap();
    assert_eq!(count(&h), "500");
    // A count typed past the limit steps back inside it.
    set(&mut h, "900");
    h.step(&json!({"click":{"handle":"dialog:Quantity down"}})).unwrap();
    assert_eq!(count(&h), "500");
    // An expression keeps its parameter: the arrows adjust a trailing offset.
    h.app.run("parameters.add", json!({"name":"holes","expression":"4","unit":""})).unwrap();
    set(&mut h, "holes");
    h.step(&json!({"click":{"handle":"dialog:Quantity up"}})).unwrap();
    assert_eq!((count(&h), sent(&h)), ("holes + 1".into(), json!("holes + 1")), "the command keeps the parameter");
    for _ in 0..2 {
        h.step(&json!({"click":{"handle":"dialog:Quantity down"}})).unwrap();
    }
    assert_eq!((count(&h), sent(&h)), ("holes - 1".into(), json!("holes - 1")));
    // Text that is not a count leaves the arrows without effect.
    set(&mut h, "nothing");
    h.step(&json!({"click":{"handle":"dialog:Quantity up"}})).unwrap();
    assert_eq!(count(&h), "nothing");
}

#[test]
fn rectangular_pattern_quantity_arrows_keep_both_directions_within_the_copy_limit() {
    let mut h = fixture();
    h.app.dialog = Some(picked(&h, "sketch.pattern.rectangular"));
    h.settle();
    let counts = |h: &Harness| match &h.app.dialog.as_ref().unwrap().kind {
        Kind::SketchModify(Mod::PatternRect { count, count2, .. }) => (count.clone(), count2.clone()),
        other => panic!("not a sketch pattern window: {other:?}"),
    };
    let (one, _) = counts(&h);
    h.step(&json!({"click":{"handle":"dialog:Quantity 2 up"}})).unwrap();
    assert_eq!(counts(&h), (one.clone(), "2".into()));
    if let Some(Kind::SketchModify(Mod::PatternRect { count, count2, .. })) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        (*count, *count2) = ("250".into(), "2".into());
    }
    h.frames(2);
    // 250 × 2 is the 500 copies a sketch pattern makes at most.
    h.step(&json!({"click":{"handle":"dialog:Quantity up"}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:Quantity 2 up"}})).unwrap();
    assert_eq!(counts(&h), ("250".into(), "2".into()));
    h.step(&json!({"click":{"handle":"dialog:Quantity down"}})).unwrap();
    assert_eq!(counts(&h), ("249".into(), "2".into()));
}
