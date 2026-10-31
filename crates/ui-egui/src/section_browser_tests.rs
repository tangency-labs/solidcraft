//! Section Analyses in the browser (#822, #3977): the Analysis folder lists every section with an
//! eye each (one shown at a time) and an eye of its own; a section's menu edits, renames, shows
//! or hides, or deletes it, and a double-click edits it.

use egui::pos2;
use serde_json::json;

use crate::context_menu::{self, Target};
use crate::scenario::Harness;
use crate::test_support::click;

fn harness() -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.ui.show_browser = true;
    // The toolbar makes a new section here; its toggle (Viewport section status) is tested on its own.
    h.app.preferences.viewport_section_status = false;
    h.frames(3);
    h
}

fn menu(h: &mut Harness, id: u64, label: &str) {
    let items = context_menu::items(&h.app, &Target::Section { id });
    let item = items.iter().find(|i| i.label == label).unwrap_or_else(|| panic!("no {label} in {items:?}")).clone();
    context_menu::run_item(&mut h.app, &item, pos2(0.0, 0.0));
    h.frames(3);
}

fn make(h: &mut Harness, p: serde_json::Value) -> u64 {
    let id = h.app.run("inspect.section", p).unwrap()["id"].as_u64().unwrap();
    h.frames(3);
    id
}

fn shown(h: &Harness) -> Option<u64> {
    h.app.session.doc.section_analyses.shown
}

#[test]
fn no_analysis_folder_without_a_section() {
    let h = harness();
    assert!(crate::scenario::handle_at("row:Analysis").is_none());
    assert!(crate::scenario::handle_at("row:Section 1").is_none());
    drop(h);
}

/// Every section has its row and its eye; turning one on turns the one shown off (it stays
/// listed), and the folder's eye hides them all and brings back the one shown.
#[test]
fn the_eyes_show_one_section_at_a_time() {
    let mut h = harness();
    let a = make(&mut h, json!({"plane": "XZ", "offset": 15}));
    let b = make(&mut h, json!({"plane": "XY", "offset": 5}));
    for row in ["row:Analysis", "row:Section 1", "row:Section 2"] {
        assert!(crate::scenario::handle_at(row).is_some(), "{row} is listed");
    }
    assert_eq!(shown(&h), Some(b), "the new section is shown, the older one hidden");
    let cut_b = h.app.session.section.expect("a cut");
    assert!((cut_b.0.z - 5.0).abs() < 1e-9);

    click(&mut h, "eye:Section 1");
    assert_eq!(shown(&h), Some(a), "its eye shows it and hides the other");
    assert!((h.app.session.section.expect("a cut").0.y - 15.0).abs() < 1e-9);
    click(&mut h, "eye:Section 1");
    assert_eq!((shown(&h), h.app.session.section), (None, None), "its eye hides it again");
    assert!(crate::scenario::handle_at("row:Section 1").is_some(), "a hidden section stays listed");
    click(&mut h, "eye:Section 2");
    assert_eq!(h.app.session.section, Some(cut_b));

    // The folder's eye is its own: hiding it turns the cut off without changing the sections'
    // eyes, and showing it brings back the one shown.
    click(&mut h, "eye:Analysis");
    assert_eq!(h.app.session.section, None);
    assert!(h.app.session.doc.section_analyses.folder_hidden && shown(&h) == Some(b));
    menu(&mut h, b, "Show/Hide");
    assert!(h.app.session.section.is_none() && shown(&h).is_none(), "the menu hides the section itself");
    assert!(h.app.session.doc.section_analyses.folder_hidden, "a section's eye doesn't change the folder's");
    menu(&mut h, a, "Show/Hide");
    assert_eq!(shown(&h), Some(a));
    assert!(h.app.session.section.is_none(), "the folder still hides it");
    click(&mut h, "eye:Analysis");
    assert!((h.app.session.section.expect("the folder shows it").0.y - 15.0).abs() < 1e-9);
}

#[test]
fn delete_removes_one_section_and_the_folder_with_the_last() {
    let mut h = harness();
    let a = make(&mut h, json!({"plane": "XZ", "offset": 15}));
    let b = make(&mut h, json!({"plane": "XY", "offset": 2}));
    menu(&mut h, b, "Delete");
    assert!(h.app.session.section.is_none(), "the section shown is gone");
    assert!(crate::scenario::handle_at("row:Section 2").is_none(), "a deleted section leaves the browser");
    assert!(crate::scenario::handle_at("row:Section 1").is_some(), "the other stays");
    menu(&mut h, a, "Delete");
    assert!(h.app.session.doc.section_analyses.is_empty());
    assert!(crate::scenario::handle_at("row:Analysis").is_none(), "no sections, no folder");
}

#[test]
fn rename_names_the_section_in_the_browser() {
    let mut h = harness();
    let a = make(&mut h, json!({"plane": "YZ"}));
    menu(&mut h, a, "Rename");
    let mut rename = h.app.menu.rename.clone().expect("the rename box opens");
    assert_eq!(rename.text, "Section 1");
    rename.text = "Middle".into();
    context_menu::commit_rename(&mut h.app, &rename);
    h.app.menu.rename = None;
    h.frames(3);
    assert!(crate::scenario::handle_at("row:Middle").is_some());
    assert!(crate::scenario::handle_at("row:Section 1").is_none());
}

fn section_dialog(h: &Harness) -> (String, bool, bool, Option<u64>, Vec<solidcraft_engine::Sel>) {
    let d = h.app.dialog.as_ref().expect("the Section Analysis dialog");
    let crate::dialogs::Kind::Section { offset, flip, hatch, edit } = &d.kind else { panic!("not the section dialog: {:?}", d.kind) };
    (offset.clone(), *flip, *hatch, edit.as_ref().map(|e| e.id), d.inputs.first().map(|i| i.items.clone()).unwrap_or_default())
}

fn ok(h: &mut Harness) {
    h.frames(2);
    h.step(&json!({"widget":"OK"})).unwrap();
    h.frames(3);
    assert!(h.app.dialog.is_none(), "OK closes the dialog");
}

fn start_on_xy(h: &mut Harness, offset: &str) {
    h.app.session.selection = vec![solidcraft_engine::Sel::Plane { name: "XY".into() }];
    h.app.start("inspect.section");
    if let Some(crate::dialogs::Kind::Section { offset: o, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        *o = offset.into();
    }
}

/// The toolbar always makes a new section, from a fresh dialog; Edit (the menu or a
/// double-click) opens the dialog with that section's own plane, distance, flip and hatch, and
/// OK changes only that one; Cancel changes nothing.
#[test]
fn the_toolbar_makes_new_sections_and_edit_changes_one() {
    let mut h = harness();
    h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
    start_on_xy(&mut h, "12 mm");
    ok(&mut h);
    let first = shown(&h).expect("a section");
    assert_eq!(h.app.session.doc.section_analyses.get(first).map(|x| x.name.as_str()), Some("Section 1"));

    h.app.session.selection.clear();
    h.app.start("inspect.section");
    let (offset, _, _, edit, plane) = section_dialog(&h);
    assert_eq!((offset.as_str(), edit), ("0 mm", None), "a new section starts afresh");
    assert!(plane.is_empty(), "a new section picks its own plane: {plane:?}");
    crate::dialogs::cancel(&mut h.app);
    h.frames(2);
    assert_eq!(h.app.session.doc.section_analyses.items.len(), 1, "Cancel makes nothing");

    h.app.run("inspect.section", json!({"plane": "XZ", "offset": 5, "flip": true, "hatch": false})).unwrap();
    h.frames(3);
    let second = shown(&h).expect("the second section");
    assert_ne!(first, second);

    // Edit the first: its values, not the second's, and not the dialog's last ones.
    menu(&mut h, first, "Edit Section Analysis");
    let (offset, flip, hatch, edit, plane) = section_dialog(&h);
    assert_eq!((offset.as_str(), flip, hatch, edit), ("12 mm", false, true, Some(first)));
    assert_eq!(plane, vec![solidcraft_engine::Sel::Plane { name: "XY".into() }]);
    crate::dialogs::cancel(&mut h.app);
    h.frames(2);
    assert_eq!(shown(&h), Some(second), "Cancel leaves the sections as they were");
    assert_eq!(h.app.session.doc.section_analyses.get(first).map(|x| x.offset.as_str()), Some("12 mm"));

    // A double-click edits the second.
    // Well after the last click, so this is a double-click and not a triple one.
    h.frames(40);
    h.step(&json!({"click": {"handle": "row:Section 2"}, "double": true})).unwrap();
    h.frames(3);
    let (offset, flip, hatch, edit, plane) = section_dialog(&h);
    assert_eq!((offset.as_str(), flip, hatch, edit), ("5", true, false, Some(second)));
    assert_eq!(plane, vec![solidcraft_engine::Sel::Plane { name: "XZ".into() }]);
    crate::dialogs::cancel(&mut h.app);
    h.frames(2);

    // OK changes that one section and shows it.
    menu(&mut h, first, "Edit Section Analysis");
    if let Some(crate::dialogs::Kind::Section { offset, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        *offset = "8 mm".into();
    }
    ok(&mut h);
    assert_eq!(h.app.session.doc.section_analyses.items.len(), 2, "editing makes no new section");
    assert_eq!(shown(&h), Some(first));
    assert!((h.app.session.section.expect("a cut").0.z - 8.0).abs() < 1e-9);
    assert_eq!(h.app.session.doc.section_analyses.get(second).map(|x| x.offset.as_str()), Some("5"), "the other keeps its own");
}

/// The dialog's Hatch option: the preview follows it, OK makes the section with it, and the
/// section keeps it when hidden and shown again or edited.
#[test]
fn the_hatch_option_carries_through_the_section() {
    let mut h = harness();
    h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
    start_on_xy(&mut h, "0 mm");
    assert!(crate::viewport::section_hatch(&h.app), "a new section is hatched");
    if let Some(crate::dialogs::Kind::Section { hatch, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        *hatch = false;
    }
    h.frames(2);
    assert!(!crate::viewport::section_hatch(&h.app), "the preview follows the option");
    ok(&mut h);
    assert!(h.app.session.section.is_some() && !h.app.session.section_hatch);

    click(&mut h, "eye:Section 1");
    click(&mut h, "eye:Section 1");
    assert!(h.app.session.section.is_some() && !h.app.session.section_hatch, "shown again, still plain");
    let id = shown(&h).expect("shown");
    menu(&mut h, id, "Edit Section Analysis");
    assert!(!section_dialog(&h).2, "the dialog shows the section's own hatch");
}

/// Editing a section with no plane picked: the dialog says OK turns it off, Cancel keeps it and
/// OK hides it (it stays listed). A new section with no plane still asks for one (#2410, #2411).
#[test]
fn ok_with_no_plane_picked_hides_the_section_edited() {
    let mut h = harness();
    h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
    let id = make(&mut h, json!({"plane": "XZ", "offset": 10}));
    menu(&mut h, id, "Edit Section Analysis");
    if let Some(plane) = h.app.dialog.as_mut().and_then(|d| d.inputs.first_mut()) {
        plane.items.clear();
    }
    h.frames(3);
    assert!(crate::scenario::handle_at("dialog:hint:section_off").is_some(), "the dialog says OK turns the section off");
    h.step(&json!({"widget":"Cancel"})).unwrap();
    h.frames(3);
    assert!(h.app.dialog.is_none() && h.app.session.section.is_some(), "Cancel keeps the section");

    menu(&mut h, id, "Edit Section Analysis");
    if let Some(plane) = h.app.dialog.as_mut().and_then(|d| d.inputs.first_mut()) {
        plane.items.clear();
    }
    ok(&mut h);
    assert_eq!(h.app.session.section, None, "OK with no plane picked hides the section");
    assert!(crate::scenario::handle_at("row:Section 1").is_some(), "the browser still lists it");

    h.app.session.selection.clear();
    h.app.start("inspect.section");
    h.frames(3);
    assert!(crate::scenario::handle_at("dialog:hint:section_off").is_none(), "a new section turns nothing off");
    h.step(&json!({"widget":"OK"})).unwrap();
    h.frames(3);
    assert_eq!(h.app.dialog.as_ref().and_then(|d| d.error.as_deref()), Some("select a plane or planar face first"));
    assert_eq!(h.app.session.doc.section_analyses.items.len(), 1, "no plane, no new section");
}

/// A section on a face that is gone stays listed, marked, and cuts nothing.
#[test]
fn a_section_whose_face_is_gone_is_marked() {
    let mut h = harness();
    h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
    let id = make(&mut h, json!({"plane": {"face": [20, 15, 20]}}));
    assert!(h.app.session.section.is_some());
    let boxf = h.app.session.doc.features.last().map(|f| f.id).unwrap();
    h.app.run("timeline.suppress", json!({"feature": boxf, "suppressed": true})).unwrap();
    h.frames(3);
    assert!(h.app.session.section.is_none(), "it cuts nothing");
    assert!(crate::scenario::handle_at("row:Section 1").is_some(), "it stays listed");
    assert!(solidcraft_engine::cmd::section::status(&h.app.session, id).is_some_and(|r| r.is_err()));
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
        let shown = h.app.session.doc.section_analyses.shown.unwrap();
        crate::dialogs::edit_section(&mut h.app, shown);
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
        let shown = h.app.session.doc.section_analyses.shown.unwrap();
        crate::dialogs::edit_section(&mut h.app, shown);
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
    h.app.preferences.viewport_section_status = true;
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

/// The "Section Analysis replaces the cut" preference (#3977): off (the default), OK in the
/// toolbar's dialog adds a section and Remove Section hides it; on, OK changes the section shown
/// and Remove Section deletes it, as the one cut was before. It changes no section a design has.
#[test]
fn the_replace_preference_steers_the_toolbar_and_remove_section() {
    let mut h = harness();
    assert!(!h.app.preferences.section_replaces_cut, "adding is the default");
    h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
    let names = |h: &Harness| h.app.session.doc.section_analyses.items.iter().map(|x| x.name.clone()).collect::<Vec<_>>();
    let remove = |h: &mut Harness| {
        let items = context_menu::items(&h.app, &Target::Viewport);
        let item = items.iter().find(|i| i.label == "Remove Section").unwrap_or_else(|| panic!("no Remove Section in {items:?}")).clone();
        context_menu::run_item(&mut h.app, &item, pos2(0.0, 0.0));
        h.frames(3);
    };
    start_on_xy(&mut h, "4 mm");
    ok(&mut h);
    start_on_xy(&mut h, "6 mm");
    ok(&mut h);
    assert_eq!(names(&h), ["Section 1", "Section 2"], "each OK adds a section");
    remove(&mut h);
    assert!(h.app.session.section.is_none());
    assert_eq!(names(&h), ["Section 1", "Section 2"], "Remove Section hides, the section stays");

    crate::prefs::control(&mut h.app, &json!({"section_replaces_cut": true})).unwrap();
    assert!(h.app.preferences.section_replaces_cut);
    assert_eq!(names(&h), ["Section 1", "Section 2"], "turning it on changes no section");
    let first = h.app.session.doc.section_analyses.items[0].id;
    click(&mut h, "eye:Section 1");
    start_on_xy(&mut h, "9 mm");
    ok(&mut h);
    assert_eq!(names(&h), ["Section 1", "Section 2"], "OK replaces the cut shown");
    assert_eq!(shown(&h), Some(first));
    assert!((h.app.session.section.expect("a cut").0.z - 9.0).abs() < 1e-9);
    remove(&mut h);
    assert!(h.app.session.section.is_none());
    assert_eq!(names(&h), ["Section 2"], "Remove Section deletes the section shown");
    // None shown: OK makes one, named as a new section is.
    start_on_xy(&mut h, "3 mm");
    ok(&mut h);
    assert_eq!(names(&h), ["Section 2", "Section 3"]);
    assert!((h.app.session.section.expect("a cut").0.z - 3.0).abs() < 1e-9);
}
