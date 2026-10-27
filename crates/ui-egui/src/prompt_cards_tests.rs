//! The confirmation and notice prompts share the kit's card: header (icon, title, close), body
//! and pinned footer. Each keeps its answers (buttons, close, Esc, Enter).
use serde_json::json;
use solidcraft_engine::Sel;

use crate::scenario::{self, Harness};

fn press(h: &mut Harness, widget: &str) {
    h.frames(3);
    h.step(&json!({"widget": widget})).unwrap_or_else(|e| panic!("{widget}: {e}"));
    h.frames(2);
}

fn card_is_drawn(h: &mut Harness) {
    h.frames(3);
    for key in ["dialog:header", "dialog:footer"] {
        let rect = scenario::rect_of(key).unwrap_or_else(|| panic!("missing {key}"));
        assert!(rect.is_finite() && rect.is_positive(), "{key}: {rect:?}");
    }
    assert!(scenario::handle_at("dialog:close").is_some() && scenario::handle_at("dialog:cancel").is_some());
}

/// A sketch with an extrude built on it: deleting the sketch takes another feature along.
fn with_dependent_feature() -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane": "XY", "name": "Base"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("solid.extrude", json!({"profile": {"sketch": "Base", "index": 0}, "distance": 5})).unwrap();
    let sketch = h.app.session.doc.features.iter().find(|f| f.name == "Base").unwrap().id;
    h.app.run("select.set", json!({"items": [Sel::Feature { id: sketch }]})).unwrap();
    crate::delete::delete_selection(&mut h.app);
    assert!(h.app.menu.confirm.is_some(), "deleting the sketch asks first");
    h
}

#[test]
fn delete_confirmation_deletes_on_the_primary_action_and_keeps_everything_otherwise() {
    let mut h = with_dependent_feature();
    let features = h.app.session.doc.features.len();
    card_is_drawn(&mut h);
    assert!(scenario::handle_at("dialog:Delete").is_some(), "the primary action says what it does");
    press(&mut h, "Cancel");
    assert!(h.app.menu.confirm.is_none() && h.app.session.doc.features.len() == features);

    let mut h = with_dependent_feature();
    h.frames(3);
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    assert!(h.app.menu.confirm.is_none() && h.app.session.doc.features.len() == features, "Esc keeps everything");

    let mut h = with_dependent_feature();
    h.frames(3);
    h.step(&json!({"click": {"handle": "dialog:close"}})).unwrap();
    h.frames(2);
    assert!(h.app.menu.confirm.is_none() && h.app.session.doc.features.len() == features, "the close button keeps everything");

    let mut h = with_dependent_feature();
    h.frames(3);
    h.step(&json!({"click": {"handle": "dialog:Delete"}})).unwrap();
    h.frames(2);
    assert!(h.app.menu.confirm.is_none() && h.app.session.doc.features.len() < features);
}

#[test]
fn legacy_delete_dialog_is_a_confirm_whose_primary_action_is_delete() {
    let mut h = with_dependent_feature();
    h.app.menu.confirm = None;
    h.app.dialog = Some(crate::dialogs::Dialog::confirm_delete(1, vec!["Extrude1".into()], vec!["Hole1".into()]));
    card_is_drawn(&mut h);
    assert!(scenario::handle_at("dialog:Delete").is_some() && scenario::rect_of("dialog:note:confirm_detail").is_some());
    press(&mut h, "Cancel");
    assert!(h.app.dialog.is_none());
}

#[test]
fn measure_card_only_closes_and_rename_card_takes_a_name() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.step(&json!({"start": "inspect.measure"})).unwrap();
    card_is_drawn(&mut h);
    assert!(scenario::handle_at("dialog:ok").is_none(), "Measure has nothing to confirm");
    assert!(scenario::rect_of("dialog:hint:measure").is_some(), "it says what to pick");
    h.step(&json!({"key": "Enter"})).unwrap();
    h.frames(2);
    assert!(h.app.dialog.is_some(), "Enter does not close Measure");
    press(&mut h, "Close");
    assert!(h.app.dialog.is_none());

    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
    let id = h.app.session.doc.features.last().unwrap().id;
    h.app.dialog = Some(crate::dialogs::Dialog::rename(id, "Block"));
    card_is_drawn(&mut h);
    assert!(scenario::handle_at("dialog:Name").is_some());
    h.step(&json!({"key": "A", "cmd": true})).unwrap();
    h.step(&json!({"text": "Housing"})).unwrap();
    h.step(&json!({"key": "Enter"})).unwrap();
    h.frames(3);
    assert!(h.app.dialog.is_none(), "Enter accepts the name");
    assert!(h.app.session.doc.features.iter().any(|f| f.name == "Housing"));
}

#[test]
fn section_card_keeps_its_distance_and_flip() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    h.app.session.selection = vec![Sel::Plane { name: "XZ".into() }];
    h.step(&json!({"start": "inspect.section"})).unwrap();
    card_is_drawn(&mut h);
    assert!(scenario::handle_at("dialog:Distance").is_some());
    h.step(&json!({"widget": "Flip"})).unwrap();
    h.frames(2);
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(crate::dialogs::Kind::Section { flip: true, .. })));
}

#[test]
fn save_prompt_moves_between_its_actions_with_the_arrow_keys() {
    use crate::documents::Answer;
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
    h.app.docs.closing = Some(h.app.docs.active);
    h.frames(3);
    card_is_drawn(&mut h);
    for label in ["Save", "Don't Save", "Cancel"] {
        assert!(scenario::handle_at(&format!("dialog:{label}")).is_some(), "{label}");
    }
    assert_eq!(h.app.docs.choice, Answer::Save);
    // The ghost action is at the left: Don't Save, Cancel, Save. Arrows follow that order.
    h.step(&json!({"key": "ArrowLeft"})).unwrap();
    h.frames(2);
    assert_eq!(h.app.docs.choice, Answer::Cancel);
    h.step(&json!({"key": "ArrowRight"})).unwrap();
    h.frames(2);
    assert_eq!(h.app.docs.choice, Answer::Save);
    h.step(&json!({"key": "ArrowLeft"})).unwrap();
    h.step(&json!({"key": "ArrowLeft"})).unwrap();
    h.frames(2);
    assert_eq!(h.app.docs.choice, Answer::DontSave);
    h.step(&json!({"key": "ArrowRight"})).unwrap();
    h.frames(2);
    assert_eq!(h.app.docs.choice, Answer::Cancel);
    h.step(&json!({"key": "Enter"})).unwrap();
    h.frames(2);
    assert!(h.app.docs.closing.is_none(), "Enter gave the picked Cancel");
    assert!(h.app.session.is_dirty(), "nothing was discarded");
}

#[test]
fn capture_question_offers_capture_revert_and_cancel() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
    let body = h.app.session.model.state().bodies[0].name.clone();
    h.app.run("component.from_bodies", json!({"bodies": [body]})).unwrap();
    let occ = h.app.session.doc.occurrences[0].id;
    h.app.run("occurrence.ground", json!({"occurrence": occ, "grounded": false})).unwrap();
    h.app.run("occurrence.move", json!({"occurrence": occ, "translate": [10, 0, 0], "capture": false})).unwrap();
    h.app.start("sketch.create");
    card_is_drawn(&mut h);
    press(&mut h, "Cancel");
    assert!(h.app.tree.capture_prompt.is_none() && !h.app.session.pending_moves.is_empty());
    h.app.start("sketch.create");
    press(&mut h, "Revert");
    assert!(h.app.session.pending_moves.is_empty() && h.app.dialog.is_some());
}

/// Type `text` into the card's X distance field.
fn type_distance(h: &mut Harness, text: &str) {
    h.step(&json!({"click": {"handle": "occurrence-move:distance:0"}, "double": true})).unwrap();
    h.frames(2);
    h.step(&json!({"key": "A", "cmd": true})).unwrap();
    h.step(&json!({"text": text})).unwrap();
    h.step(&json!({"key": "Enter"})).unwrap();
    h.frames(3);
}

#[test]
fn occurrence_move_card_edits_distances_and_captures_or_reverts() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
    let body = h.app.session.model.state().bodies[0].name.clone();
    h.app.run("component.from_bodies", json!({"bodies": [body]})).unwrap();
    let occ = h.app.session.doc.occurrences[0].id;
    h.app.run("occurrence.ground", json!({"occurrence": occ, "grounded": false})).unwrap();
    h.app.tree.occurrence_move = Some(crate::browser::OccMove::new(occ));
    card_is_drawn(&mut h);
    for k in 0..3 {
        assert!(scenario::handle_at(&format!("occurrence-move:distance:{k}")).is_some());
        assert!(scenario::handle_at(&format!("occurrence-move:angle:{k}")).is_some());
    }
    type_distance(&mut h, "5");
    assert!(!h.app.session.pending_moves.is_empty(), "the move is pending");
    press(&mut h, "Cancel");
    assert!(h.app.tree.occurrence_move.is_none() && h.app.session.pending_moves.is_empty(), "Cancel puts the move back");

    h.app.tree.occurrence_move = Some(crate::browser::OccMove::new(occ));
    h.frames(3);
    type_distance(&mut h, "5");
    assert!(!h.app.session.pending_moves.is_empty(), "the move is pending");
    press(&mut h, "Capture All Positions");
    assert!(h.app.tree.occurrence_move.is_none() && h.app.session.pending_moves.is_empty());
}

#[test]
fn settings_reset_notice_dismisses_or_opens_the_graphics_page() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.settings_reset = vec![crate::prefs::SettingReset::Msaa { requested: 8, reset: 4, supported: vec![1, 4] }];
    card_is_drawn(&mut h);
    press(&mut h, "OK");
    assert!(h.app.settings_reset.is_empty() && !h.app.prefs_window.open);
    h.app.settings_reset = vec![crate::prefs::SettingReset::Msaa { requested: 8, reset: 4, supported: vec![1, 4] }];
    h.frames(3);
    press(&mut h, "Open Preferences");
    assert!(h.app.settings_reset.is_empty() && h.app.prefs_window.open);
    assert_eq!(h.app.prefs_window.section, crate::prefs::PAGE_GRAPHICS);
}

/// Graphics settings on trial (#4015): the question's Keep and Go back, and an unanswered one
/// giving up after its time: it closes for a restart and leaves the trial for the next start to
/// drop.
#[test]
fn graphics_trial_question_keeps_goes_back_or_gives_up() {
    use crate::prefs::{GraphicsChoice, GraphicsTrial};
    let dx12 = GraphicsChoice { api: "dx12".into(), msaa: 4 };
    let on_trial = |h: &mut Harness| {
        h.app.quit_requested = false;
        h.app.graphics.restart = false;
        h.app.graphics.saved = dx12.clone();
        h.app.graphics.trial = Some(GraphicsTrial { previous: dx12.clone(), shown: 0.0, last_frame: None });
        (h.app.preferences.graphics_api, h.app.preferences.msaa) = ("vulkan".into(), 8);
    };
    let mut h = Harness::new();
    h.app.home.open = false;
    on_trial(&mut h);
    card_is_drawn(&mut h);
    assert!(h.app.graphics.trial.as_ref().and_then(|t| t.last_frame).is_some(), "the countdown starts when the question shows");
    press(&mut h, "Keep");
    assert_eq!(h.app.graphics.saved, GraphicsChoice { api: "vulkan".into(), msaa: 8 });
    assert!(h.app.graphics.trial.is_none() && !h.app.graphics.restart && !h.app.quit_requested);

    on_trial(&mut h);
    press(&mut h, "Go back");
    assert_eq!((h.app.preferences.graphics_api.as_str(), h.app.preferences.msaa), ("dx12", 4));
    assert!(h.app.graphics.trial.is_none() && h.app.graphics.restart && h.app.quit_requested);

    // A window that drew nothing for a long while (hidden behind others) doesn't give up the
    // moment it draws again.
    on_trial(&mut h);
    h.frames(2);
    if let Some(trial) = &mut h.app.graphics.trial {
        trial.last_frame = trial.last_frame.map(|t| t - 1000.0);
    }
    h.frames(1);
    let shown = h.app.graphics.trial.as_ref().map(|t| t.shown);
    assert!(shown.is_some_and(|s| s < 2.0) && !h.app.quit_requested, "still asking: {shown:?}");

    if let Some(trial) = &mut h.app.graphics.trial {
        trial.shown = crate::prefs_notice::TRIAL_SECONDS;
    }
    h.frames(1);
    assert!(h.app.graphics.trial.is_none() && h.app.graphics.restart && h.app.quit_requested, "gave up: closes for a restart");
    assert_eq!(h.app.preferences.graphics_api, "vulkan", "nothing saved: the next start drops the trial");

    // A start from the desktop opens on Home: the question shows there too.
    h.app.home.open = true;
    on_trial(&mut h);
    card_is_drawn(&mut h);
    press(&mut h, "Keep");
    assert!(h.app.graphics.trial.is_none() && !h.app.graphics.restart);
}

#[test]
fn object_properties_card_only_closes() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("solid.box", json!({"length": 10, "width": 20, "height": 30})).unwrap();
    let v = h.app.run("inspect.measure", json!({})).unwrap();
    h.app.menu.props = Some(("Body1".into(), v));
    card_is_drawn(&mut h);
    assert!(scenario::handle_at("dialog:ok").is_none());
    assert!(scenario::rect_of("dialog:row:Mass").is_some() && scenario::rect_of("dialog:row:Volume").is_some());
    press(&mut h, "Close");
    assert!(h.app.menu.props.is_none());
}

#[test]
fn delete_confirm_resumes_after_the_issue_draft_closes() {
    let mut h = with_dependent_feature();
    h.frames(3);
    let features = h.app.session.doc.features.len();
    h.app.issue_report.draft = Some(crate::issue_report::draft_for(&h.app, None, None));
    // As in the report window's screenshot/layout tests, let the bounded review settle.
    h.frames(20);
    let [x, y] = h.widget("Close draft").unwrap();
    assert!(scenario::rect_of("dialog:cancel").unwrap().contains(egui::pos2(x as f32, y as f32)));
    h.step(&json!({"widget":"Close draft"})).unwrap();
    h.frames(3);
    assert!(h.app.issue_report.draft.is_none(), "the report can close while the confirmation is suspended");
    assert!(h.app.menu.confirm.is_some() && h.app.session.doc.features.len() == features);
    press(&mut h, "Cancel");
    assert!(h.app.menu.confirm.is_none() && h.app.session.doc.features.len() == features);
}
