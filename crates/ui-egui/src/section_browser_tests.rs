//! Section Analysis in the browser (#822): an Analysis folder lists it with an eye to hide and
//! show it, and its menu edits or deletes it.

use egui::pos2;
use serde_json::json;

use crate::context_menu::{self, Target};
use crate::scenario::Harness;
use crate::test_support::click;

fn harness() -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.ui.show_browser = true;
    h.frames(3);
    h
}

fn menu(h: &mut Harness, label: &str) {
    let items = context_menu::items(&h.app, &Target::Section);
    let item = items.iter().find(|i| i.label == label).unwrap_or_else(|| panic!("no {label} in {items:?}")).clone();
    context_menu::run_item(&mut h.app, &item, pos2(0.0, 0.0));
    h.frames(3);
}

#[test]
fn no_analysis_folder_without_a_section() {
    let h = harness();
    assert!(crate::scenario::handle_at("row:Analysis").is_none());
    assert!(crate::scenario::handle_at("row:Section Analysis").is_none());
    drop(h);
}

#[test]
fn the_eye_hides_and_shows_the_section_and_delete_removes_it() {
    let mut h = harness();
    h.app.run("inspect.section", json!({"plane": "XZ", "offset": 15})).unwrap();
    h.frames(3);
    let section = h.app.session.section.expect("a section");
    assert!(crate::scenario::handle_at("row:Section Analysis").is_some(), "the section is listed under Analysis");

    click(&mut h, "eye:Section Analysis");
    assert_eq!(h.app.session.section, None, "the eye turns the section off");
    assert!(crate::scenario::handle_at("row:Section Analysis").is_some(), "a hidden section stays listed");

    click(&mut h, "eye:Section Analysis");
    let back = h.app.session.section.expect("the eye turns it back on");
    assert!((back.0 - section.0).len() < 1e-9 && (back.1 - section.1).len() < 1e-9, "the same plane: {back:?} vs {section:?}");

    // The folder's eye is its own: hiding it turns the section off without changing the
    // section's eye, and hiding the section doesn't change the folder's.
    click(&mut h, "eye:Analysis");
    assert_eq!(h.app.session.section, None);
    assert!(h.app.tree.analysis_hidden && !h.app.tree.section_hidden);
    menu(&mut h, "Show/Hide");
    assert!(h.app.session.section.is_none() && h.app.tree.section_hidden, "the menu hides the section itself");
    assert!(h.app.tree.analysis_hidden, "the section's eye doesn't change the folder's");
    click(&mut h, "eye:Analysis");
    assert_eq!(h.app.session.section, None, "the section was hidden on its own meanwhile");
    assert!(!h.app.tree.analysis_hidden);
    menu(&mut h, "Show/Hide");
    assert!(h.app.session.section.is_some(), "Show/Hide in the menu brings it back");

    menu(&mut h, "Delete");
    assert_eq!(h.app.session.section, None);
    assert!(h.app.tree.hidden_section.is_none());
    assert!(crate::scenario::handle_at("row:Section Analysis").is_none(), "a deleted section leaves the browser");
}

#[test]
fn deleting_a_hidden_section_forgets_it() {
    let mut h = harness();
    h.app.run("inspect.section", json!({"plane": "XY", "offset": 2})).unwrap();
    h.frames(3);
    click(&mut h, "eye:Section Analysis");
    menu(&mut h, "Delete");
    assert!(h.app.session.section.is_none() && h.app.tree.hidden_section.is_none());
    assert!(crate::scenario::handle_at("row:Analysis").is_none());
}

#[test]
fn edit_opens_the_section_dialog() {
    let mut h = harness();
    h.app.run("inspect.section", json!({"plane": "YZ"})).unwrap();
    h.frames(3);
    menu(&mut h, "Edit Section Analysis");
    assert!(h.app.dialog.is_some(), "the Section Analysis dialog opens");
}

fn section_dialog(h: &Harness) -> (String, bool, Vec<solidcraft_engine::Sel>) {
    let d = h.app.dialog.as_ref().expect("the Section Analysis dialog");
    let crate::dialogs::Kind::Section { offset, flip, .. } = &d.kind else { panic!("not the section dialog: {:?}", d.kind) };
    (offset.clone(), *flip, d.inputs.first().map(|i| i.items.clone()).unwrap_or_default())
}

fn ok(h: &mut Harness) {
    h.frames(2);
    h.step(&json!({"widget":"OK"})).unwrap();
    h.frames(3);
    assert!(h.app.dialog.is_none(), "OK closes the dialog");
}

/// Editing a section reopens its dialog at the plane and distance it was left at, so the cut
/// moves on from there instead of jumping back to the plane.
#[test]
fn edit_starts_from_the_section_as_it_was_left() {
    let mut h = harness();
    h.app.preferences.viewport_section_status = false;
    h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
    h.app.session.selection = vec![solidcraft_engine::Sel::Plane { name: "XY".into() }];
    h.app.start("inspect.section");
    if let Some(crate::dialogs::Kind::Section { offset, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        *offset = "12 mm".into();
    }
    ok(&mut h);
    let cut = h.app.session.section.expect("a section");
    assert!((cut.0.z - 12.0).abs() < 1e-9, "{cut:?}");

    // The toolbar starts a new section (in place of this one), not an edit of it.
    h.app.session.selection.clear();
    h.app.start("inspect.section");
    let (offset, _, plane) = section_dialog(&h);
    assert_eq!(offset, "0 mm", "a new section starts at its plane");
    assert!(plane.is_empty(), "a new section picks its own plane: {plane:?}");
    crate::dialogs::cancel(&mut h.app);
    h.frames(2);
    assert!(h.app.session.section.is_some_and(|c| (c.0.z - 12.0).abs() < 1e-9), "cancelling keeps the section");

    menu(&mut h, "Edit Section Analysis");
    let (offset, flip, plane) = section_dialog(&h);
    assert_eq!((offset.as_str(), flip), ("12 mm", false));
    assert_eq!(plane, vec![solidcraft_engine::Sel::Plane { name: "XY".into() }]);
    crate::dialogs::cancel(&mut h.app);
    h.frames(2);

    // Hidden and shown again, it is still the section the dialog made.
    click(&mut h, "eye:Section Analysis");
    click(&mut h, "eye:Section Analysis");
    menu(&mut h, "Edit Section Analysis");
    assert_eq!(section_dialog(&h).0, "12 mm");
    crate::dialogs::cancel(&mut h.app);
    h.frames(2);

    // A section made some other way (here a command) is not the dialog's: Edit starts afresh.
    h.app.run("inspect.section", json!({"plane": "XZ", "offset": 5})).unwrap();
    h.frames(3);
    menu(&mut h, "Edit Section Analysis");
    let (offset, _, plane) = section_dialog(&h);
    assert_eq!(offset, "0 mm");
    assert!(plane.is_empty());
}

/// The dialog's Hatch option: the preview follows it, OK makes the section with it, and the
/// section keeps it when hidden and shown again or edited.
#[test]
fn the_hatch_option_carries_through_the_section() {
    let mut h = harness();
    h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
    h.app.session.selection = vec![solidcraft_engine::Sel::Plane { name: "XY".into() }];
    h.app.start("inspect.section");
    assert!(crate::viewport::section_hatch(&h.app), "a new section is hatched");
    if let Some(crate::dialogs::Kind::Section { hatch, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        *hatch = false;
    }
    h.frames(2);
    assert!(!crate::viewport::section_hatch(&h.app), "the preview follows the option");
    ok(&mut h);
    assert!(h.app.session.section.is_some() && !h.app.session.section_hatch);

    click(&mut h, "eye:Section Analysis");
    click(&mut h, "eye:Section Analysis");
    assert!(h.app.session.section.is_some() && !h.app.session.section_hatch, "shown again, still plain");
    menu(&mut h, "Edit Section Analysis");
    let d = h.app.dialog.as_ref().map(|d| &d.kind);
    assert!(matches!(d, Some(crate::dialogs::Kind::Section { hatch: false, .. })), "{d:?}");
}

/// With a section on and no plane picked, the dialog says OK turns the section off: Cancel keeps
/// it and OK removes it. With no section on, OK still asks for a plane (#2410).
#[test]
fn ok_with_no_plane_picked_turns_the_section_off() {
    let mut h = harness();
    h.app.preferences.viewport_section_status = false;
    h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
    h.app.run("inspect.section", json!({"plane": "XZ", "offset": 10})).unwrap();
    h.app.session.selection.clear();
    h.app.start("inspect.section");
    h.frames(3);
    assert!(section_dialog(&h).2.is_empty(), "no plane picked");
    assert!(crate::scenario::handle_at("dialog:hint:section_off").is_some(), "the dialog says OK turns the section off");
    h.step(&json!({"widget":"Cancel"})).unwrap();
    h.frames(3);
    assert!(h.app.dialog.is_none() && h.app.session.section.is_some(), "Cancel keeps the section");

    h.app.start("inspect.section");
    ok(&mut h);
    assert_eq!(h.app.session.section, None, "OK with no plane picked turns the section off");
    assert!(crate::scenario::handle_at("row:Section Analysis").is_none(), "the browser no longer lists it");

    h.app.start("inspect.section");
    h.frames(3);
    assert!(crate::scenario::handle_at("dialog:hint:section_off").is_none(), "no section to turn off");
    h.step(&json!({"widget":"OK"})).unwrap();
    h.frames(3);
    assert_eq!(h.app.dialog.as_ref().and_then(|d| d.error.as_deref()), Some("select a plane or planar face first"));
}

#[test]
fn editing_programmatic_sections_keeps_actual_plane_flip_hatch_and_cancel_then_moves_it() {
    for params in [
        json!({"plane":"XY","at":-25.4,"flip":true,"hatch":false}),
        json!({"plane":{"origin":[7,-12,4],"normal":[-3,-4,0]},"offset":5,"hatch":false}),
        json!({"plane":{"origin":[7,-12,4],"normal":[0,0,-1]},"offset":-5,"flip":true}),
    ] {
        let mut h = harness();
        h.app.run("inspect.section", params).unwrap();
        assert!(h.app.dialog.is_none(), "programmatic commands never open a form");
        let cut = h.app.session.section.unwrap();
        let hatch = h.app.session.section_hatch;
        crate::dialogs::edit_section(&mut h.app);
        let d = h.app.dialog.as_ref().unwrap();
        let cmds = crate::dialogs::apply_commands(&h.app, d).unwrap();
        let mut scratch = h.app.session.scratch();
        scratch.execute(&cmds[0].0, &cmds[0].1).unwrap();
        let preview = scratch.section.unwrap();
        assert!((preview.0 - cut.0).len() < 1e-9 && (preview.1 - cut.1).len() < 1e-9);
        assert_eq!(scratch.section_hatch, hatch);
        assert_eq!(h.app.session.section, Some(cut), "edit intent cannot toggle the active cut off");
        crate::dialogs::cancel(&mut h.app);
        assert_eq!(h.app.session.section, Some(cut));
        crate::dialogs::edit_section(&mut h.app);
        if let Some(crate::dialogs::Kind::Section { offset, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            *offset = "30 mm".into();
        }
        let cmds = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        h.app.run(&cmds[0].0, cmds[0].1.clone()).unwrap();
        assert_ne!(h.app.session.section.unwrap().0, cut.0);
        assert!(
            (h.app.session.section.unwrap().1 - cut.1).len() < 1e-12,
            "moving the plane retains its clipping direction within normalisation precision"
        );
    }
}

#[test]
fn section_ribbon_toggle_and_setting_edit_have_separate_intent_and_compatibility() {
    let mut h = harness();
    h.app.preferences.viewport_status_details = false;
    h.app.run("inspect.zebra", json!({})).unwrap();
    h.app.run("inspect.section", json!({"plane":"YZ","offset":3})).unwrap();
    h.frames(3);
    let effect = h.app.session.status_items().into_iter().find(|e| e.command == "inspect.section").unwrap();
    let item = crate::viewport_status::presentation(&h.app, &effect).unwrap();
    h.step(&json!({"click":{"handle":format!("viewport_status:{}:setting",item.id)}})).unwrap();
    assert!(h.app.dialog.is_some() && h.app.session.section.is_some());
    crate::dialogs::cancel(&mut h.app);
    h.app.start("inspect.section");
    assert!(h.app.dialog.is_none() && h.app.session.section.is_none());
    assert!(h.app.session.analysis.is_some());
    h.app.run("inspect.section", json!({"plane":"XY"})).unwrap();
    h.app.preferences.viewport_section_status = false;
    let cut = h.app.session.section;
    h.app.start("inspect.section");
    assert!(h.app.dialog.is_some());
    assert_eq!(h.app.session.section, cut);
}
