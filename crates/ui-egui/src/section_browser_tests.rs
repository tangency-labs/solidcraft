//! Section Analysis in the browser (#822): an Analysis folder lists it with an eye to hide and
//! show it, and its menu edits or deletes it. In the viewport, a planar face's menu starts a
//! section on it (or adds its plane), and a cut face's menu turns its plane off.

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

/// The one plane of the section (origin, normal).
fn only_cut(h: &Harness) -> (solidcraft_engine::geom::Vec3, solidcraft_engine::geom::Vec3) {
    let [c] = h.app.session.section[..] else { panic!("one section plane: {:?}", h.app.session.section) };
    (c.origin, c.normal)
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
    let section = only_cut(&h);
    assert!(crate::scenario::handle_at("row:Section Analysis").is_some(), "the section is listed under Analysis");

    click(&mut h, "eye:Section Analysis");
    assert!(h.app.session.section.is_empty(), "the eye turns the section off");
    assert!(crate::scenario::handle_at("row:Section Analysis").is_some(), "a hidden section stays listed");

    click(&mut h, "eye:Section Analysis");
    let back = only_cut(&h);
    assert!((back.0 - section.0).len() < 1e-9 && (back.1 - section.1).len() < 1e-9, "the same plane: {back:?} vs {section:?}");

    // The folder's eye is its own: hiding it turns the section off without changing the
    // section's eye, and hiding the section doesn't change the folder's.
    click(&mut h, "eye:Analysis");
    assert!(h.app.session.section.is_empty());
    assert!(h.app.tree.analysis_hidden && !h.app.tree.section_hidden);
    menu(&mut h, "Show/Hide");
    assert!(h.app.session.section.is_empty() && h.app.tree.section_hidden, "the menu hides the section itself");
    assert!(h.app.tree.analysis_hidden, "the section's eye doesn't change the folder's");
    click(&mut h, "eye:Analysis");
    assert!(h.app.session.section.is_empty(), "the section was hidden on its own meanwhile");
    assert!(!h.app.tree.analysis_hidden);
    menu(&mut h, "Show/Hide");
    assert!(!h.app.session.section.is_empty(), "Show/Hide in the menu brings it back");

    menu(&mut h, "Delete");
    assert!(h.app.session.section.is_empty());
    assert!(h.app.tree.hidden_section.is_empty());
    assert!(crate::scenario::handle_at("row:Section Analysis").is_none(), "a deleted section leaves the browser");
}

#[test]
fn deleting_a_hidden_section_forgets_it() {
    let mut h = harness();
    h.app.run("inspect.section", json!({"plane": "XY", "offset": 2})).unwrap();
    h.frames(3);
    click(&mut h, "eye:Section Analysis");
    menu(&mut h, "Delete");
    assert!(h.app.session.section.is_empty() && h.app.tree.hidden_section.is_empty());
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
    let cut = only_cut(&h);
    assert!((cut.0.z - 12.0).abs() < 1e-9, "{cut:?}");

    // The toolbar starts a new section (in place of this one), not an edit of it.
    h.app.session.selection.clear();
    h.app.start("inspect.section");
    let (offset, _, plane) = section_dialog(&h);
    assert_eq!(offset, "0 mm", "a new section starts at its plane");
    assert!(plane.is_empty(), "a new section picks its own plane: {plane:?}");
    crate::dialogs::cancel(&mut h.app);
    h.frames(2);
    assert!((only_cut(&h).0.z - 12.0).abs() < 1e-9, "cancelling keeps the section");

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
    assert!(!h.app.session.section.is_empty() && !h.app.session.section_hatch);

    click(&mut h, "eye:Section Analysis");
    click(&mut h, "eye:Section Analysis");
    assert!(!h.app.session.section.is_empty() && !h.app.session.section_hatch, "shown again, still plain");
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
    assert!(h.app.dialog.is_none() && !h.app.session.section.is_empty(), "Cancel keeps the section");

    h.app.start("inspect.section");
    ok(&mut h);
    assert!(h.app.session.section.is_empty(), "OK with no plane picked turns the section off");
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
        let cut = only_cut(&h);
        let hatch = h.app.session.section_hatch;
        crate::dialogs::edit_section(&mut h.app);
        let d = h.app.dialog.as_ref().unwrap();
        let cmds = crate::dialogs::apply_commands(&h.app, d).unwrap();
        let mut scratch = h.app.session.scratch();
        scratch.execute(&cmds[0].0, &cmds[0].1).unwrap();
        let [preview] = scratch.section[..] else { panic!("one section plane: {:?}", scratch.section) };
        assert!((preview.origin - cut.0).len() < 1e-9 && (preview.normal - cut.1).len() < 1e-9);
        assert_eq!(scratch.section_hatch, hatch);
        assert_eq!(only_cut(&h), cut, "edit intent cannot toggle the active cut off");
        crate::dialogs::cancel(&mut h.app);
        assert_eq!(only_cut(&h), cut);
        crate::dialogs::edit_section(&mut h.app);
        if let Some(crate::dialogs::Kind::Section { offset, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            *offset = "30 mm".into();
        }
        let cmds = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        h.app.run(&cmds[0].0, cmds[0].1.clone()).unwrap();
        assert_ne!(only_cut(&h).0, cut.0);
        assert!((only_cut(&h).1 - cut.1).len() < 1e-12, "moving the plane retains its clipping direction within normalisation precision");
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
    assert!(h.app.dialog.is_some() && !h.app.session.section.is_empty());
    crate::dialogs::cancel(&mut h.app);
    h.app.start("inspect.section");
    assert!(h.app.dialog.is_none() && h.app.session.section.is_empty());
    assert!(h.app.session.analysis.is_some());
    h.app.run("inspect.section", json!({"plane":"XY"})).unwrap();
    h.app.preferences.viewport_section_status = false;
    let cut = h.app.session.section.clone();
    h.app.start("inspect.section");
    assert!(h.app.dialog.is_some());
    assert_eq!(h.app.session.section, cut);
}

/// A box seen from the home view, the window drawn.
fn box_in_view() -> Harness {
    let mut h = harness();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    h.app.cam = solidcraft_engine::view::home_camera(&h.app.session);
    h.frames(3);
    h
}

/// Right-click the world point `at` and read the menu's labels.
fn menu_at(h: &mut Harness, at: [f64; 3]) -> Vec<String> {
    crate::context_menu::close(&mut h.app);
    h.frames(1);
    let p = h.at(&json!({"world": at})).unwrap();
    h.call("ui.menu", json!({"x": p[0], "y": p[1]}));
    h.frames(2);
    let menu = h.call("ui.menu", json!({}))["result"].clone();
    menu["items"].as_array().map(|items| items.iter().filter_map(|i| i["label"].as_str().map(str::to_owned)).collect()).unwrap_or_default()
}

fn pick(h: &mut Harness, label: &str) {
    assert_eq!(h.call("ui.menuPick", json!({"item": label}))["ok"], true, "{label}");
    h.frames(3);
}

/// Sets the open Section Analysis dialog's distance.
fn set_offset(h: &mut Harness, value: &str) {
    if let Some(crate::dialogs::Kind::Section { offset, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        *offset = value.into();
    }
}

/// Section View on a planar face starts the Section Analysis dialog on the face's plane, with
/// its outward normal: at distance 0 it cuts nothing, and a distance into the part cuts it.
#[test]
fn section_view_on_a_face_starts_a_section_on_its_plane() {
    let mut h = box_in_view();
    let labels = menu_at(&mut h, [20.0, 15.0, 20.0]);
    assert!(labels.iter().any(|l| l == "Section View"), "{labels:?}");
    assert!(!labels.iter().any(|l| l == "Turn Off Section View"), "no section yet: {labels:?}");
    pick(&mut h, "Section View");
    let (offset, flip, plane) = section_dialog(&h);
    assert_eq!((offset.as_str(), flip), ("0 mm", false));
    assert!(matches!(plane[..], [solidcraft_engine::Sel::Face { .. }]), "{plane:?}");
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(crate::dialogs::Kind::Section { add: false, .. })));
    let preview = crate::viewport::section_planes(&h.app);
    let [cut] = preview[..] else { panic!("one plane: {preview:?}") };
    assert!(
        (cut.normal - solidcraft_engine::geom::Vec3::Z).len() < 1e-9 && (cut.origin.z - 20.0).abs() < 1e-9,
        "the top face's outward plane: {cut:?}"
    );
    set_offset(&mut h, "-5 mm");
    ok(&mut h);
    let (origin, normal) = only_cut(&h);
    assert!((normal - solidcraft_engine::geom::Vec3::Z).len() < 1e-9 && (origin.z - 15.0).abs() < 1e-9, "{origin:?} {normal:?}");
}

/// With a section on, Section View on another face adds that face's plane to it: the dialog
/// says so, the preview keeps the planes on show, and OK leaves both. The plane starts flipped,
/// hiding the part's side, so the cut stays as it was (an immediate OK changes nothing), and
/// moving it into the part brings back what lies before it.
#[test]
fn section_view_adds_a_face_plane_to_the_section_on_show() {
    let mut h = box_in_view();
    h.app.run("inspect.section", json!({"plane": {"origin": [0, 15, 0], "normal": [0, -1, 0]}, "hatch": false})).unwrap();
    h.frames(3);
    let hidden = |planes: &[solidcraft_engine::SectionPlane], p: [f64; 3]| {
        let p = solidcraft_engine::geom::Vec3::new(p[0], p[1], p[2]);
        !planes.iter().any(|c| c.keeps(p, 0.0))
    };
    let labels = menu_at(&mut h, [40.0, 22.0, 10.0]);
    assert!(labels.iter().any(|l| l == "Section View"), "{labels:?}");
    pick(&mut h, "Section View");
    let kind = h.app.dialog.as_ref().map(|d| d.kind.clone());
    assert!(matches!(kind, Some(crate::dialogs::Kind::Section { add: true, flip: true, hatch: false, .. })), "{kind:?}");
    h.frames(2);
    assert!(crate::scenario::handle_at("dialog:hint:section_add").is_some(), "the dialog says the plane joins the section");
    let preview = crate::viewport::section_planes(&h.app);
    assert_eq!(preview.len(), 2, "the preview keeps the section's plane");
    assert!(hidden(&preview, [20.0, 5.0, 10.0]) && hidden(&preview, [39.0, 5.0, 10.0]), "the cut is still on show: {preview:?}");
    assert!(!crate::viewport::section_hatch(&h.app), "the preview keeps the section plain");
    ok(&mut h);
    assert_eq!(h.app.session.section.len(), 2);
    assert!(hidden(&h.app.session.section, [20.0, 5.0, 10.0]) && !h.app.session.section_hatch, "OK at 0 keeps the cut, and plain");
    let labels = menu_at(&mut h, [20.0, 15.0, 10.0]);
    assert!(labels.iter().any(|l| l == "Turn Off Section View"), "the cut face is still there to turn off: {labels:?}");

    // Moved 20 mm into the part, it hides only what lies past x 20.
    h.app.run("inspect.section", json!({"plane": {"origin": [0, 15, 0], "normal": [0, -1, 0]}})).unwrap();
    h.frames(3);
    menu_at(&mut h, [40.0, 22.0, 10.0]);
    pick(&mut h, "Section View");
    set_offset(&mut h, "-20 mm");
    ok(&mut h);
    let planes: Vec<_> = h.app.session.section.iter().map(|c| (c.id, c.normal)).collect();
    assert_eq!(planes.len(), 2, "{planes:?}");
    assert!((planes[1].1 + solidcraft_engine::geom::Vec3::X).len() < 1e-9 && (h.app.session.section[1].origin.x - 20.0).abs() < 1e-9, "{planes:?}");
    assert!(h.app.session.section_hatch, "a hatched section stays hatched");
    assert!(hidden(&h.app.session.section, [10.0, 5.0, 10.0]) && !hidden(&h.app.session.section, [30.0, 5.0, 10.0]));
}

/// Section View cuts on the face right-clicked, even when the selection holds other faces before
/// it (a curved one, or another planar one).
#[test]
fn section_view_cuts_on_the_face_right_clicked() {
    let mut h = harness();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    h.app.run("solid.cylinder", json!({"base": [28, 15, 0], "radius": 5, "height": 20, "operation": "cut"})).unwrap();
    h.app.cam = solidcraft_engine::view::home_camera(&h.app.session);
    h.frames(3);
    let face_at = |h: &mut Harness, at: [f64; 3]| {
        menu_at(h, at);
        let face = h.app.session.selection.first().cloned().expect("a face under the cursor");
        crate::context_menu::close(&mut h.app);
        face
    };
    let (hole, top, front) = (face_at(&mut h, [24.47, 18.53, 18.0]), face_at(&mut h, [10.0, 8.0, 20.0]), face_at(&mut h, [12.0, 0.0, 5.0]));
    let section_face = |h: &mut Harness, selection: &[&solidcraft_engine::Sel], at: [f64; 3]| {
        h.app.session.selection = selection.iter().map(|s| (*s).clone()).collect();
        let labels = menu_at(h, at);
        assert_eq!(h.app.session.selection.len(), selection.len(), "the right-click keeps the selection");
        let items = context_menu::items(&h.app, &Target::Viewport);
        let item = items.iter().find(|i| i.id == "ui.sectionView");
        (labels, item.and_then(|i| i.params.as_ref().and_then(|p| serde_json::from_value::<solidcraft_engine::Sel>(p["face"].clone()).ok())))
    };
    let index = |s: &solidcraft_engine::Sel| match s {
        solidcraft_engine::Sel::Face { index, .. } => *index,
        _ => usize::MAX,
    };
    let (labels, face) = section_face(&mut h, &[&hole, &top], [10.0, 8.0, 20.0]);
    assert_eq!(face.as_ref().map(index), Some(index(&top)), "the top face, not the hole before it: {labels:?}");
    let (_, face) = section_face(&mut h, &[&front, &top], [10.0, 8.0, 20.0]);
    assert_eq!(face.as_ref().map(index), Some(index(&top)), "the face clicked, not the first selected");
    let (labels, face) = section_face(&mut h, &[&top, &hole], [24.47, 18.53, 18.0]);
    assert!(face.is_none() && !labels.iter().any(|l| l == "Section View"), "the hole clicked is curved: {labels:?}");
}

/// Edit on a section of several planes edits its last plane in place: the preview keeps the
/// others, the dialog says so, OK with no plane picked keeps the section, and a plane Section
/// View added reopens where it was left, its OK moving that plane alone.
#[test]
fn edit_moves_one_plane_of_several() {
    let mut h = box_in_view();
    h.app.run("inspect.section", json!({"plane": {"origin": [0, 15, 0], "normal": [0, -1, 0]}})).unwrap();
    h.frames(3);
    menu_at(&mut h, [40.0, 22.0, 10.0]);
    pick(&mut h, "Section View");
    set_offset(&mut h, "-20 mm");
    ok(&mut h);
    let before = h.app.session.section.clone();
    assert_eq!(before.len(), 2);

    menu(&mut h, "Edit Section Analysis");
    let kind = h.app.dialog.as_ref().map(|d| d.kind.clone());
    let Some(crate::dialogs::Kind::Section { offset, flip: true, edit: Some(id), .. }) = kind else { panic!("{kind:?}") };
    assert_eq!((offset.as_str(), id), ("-20 mm", before[1].id), "the plane Section View added, where it was left");
    h.frames(2);
    assert!(crate::scenario::handle_at("dialog:hint:section_edit").is_some(), "the dialog says it edits one plane");
    assert!(crate::scenario::handle_at("dialog:hint:section_off").is_none());
    assert_eq!(crate::viewport::section_planes(&h.app), before, "the preview is the section as it is");
    set_offset(&mut h, "-30 mm");
    h.frames(2);
    let preview = crate::viewport::section_planes(&h.app);
    assert!(preview.len() == 2 && preview[0] == before[0] && (preview[1].origin.x - 10.0).abs() < 1e-9, "{preview:?}");
    ok(&mut h);
    let after = h.app.session.section.clone();
    assert!(after.len() == 2 && after[0] == before[0] && after[1].id == before[1].id && (after[1].origin.x - 10.0).abs() < 1e-9, "{after:?}");

    // A section of several planes made some other way: Edit moves its last plane, and OK with
    // none picked asks for one instead of turning the section off.
    h.app.run("inspect.section", json!({"plane": "XZ", "offset": 15})).unwrap();
    h.app.run("inspect.section", json!({"plane": "YZ", "offset": 20, "add": true})).unwrap();
    h.frames(3);
    let before = h.app.session.section.clone();
    menu(&mut h, "Edit Section Analysis");
    let (offset, _, plane) = section_dialog(&h);
    assert!(offset == "0 mm" && plane.is_empty(), "{offset} {plane:?}");
    assert_eq!(crate::viewport::section_planes(&h.app), before, "the preview keeps every plane");
    h.frames(2);
    h.step(&json!({"widget":"OK"})).unwrap();
    h.frames(3);
    assert_eq!(h.app.session.section, before, "OK with no plane keeps the section");
    assert!(h.app.dialog.is_some(), "and asks for a plane");
    crate::dialogs::cancel(&mut h.app);
    h.frames(2);

    // Hidden, Edit shows it again to edit it.
    click(&mut h, "eye:Section Analysis");
    assert!(h.app.session.section.is_empty());
    menu(&mut h, "Edit Section Analysis");
    assert_eq!(h.app.session.section.len(), 2, "shown to edit");
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(crate::dialogs::Kind::Section { edit: Some(_), .. })));
}

/// A curved face has no plane: its menu offers no Section View.
#[test]
fn section_view_is_not_offered_on_a_curved_face() {
    let mut h = harness();
    h.app.run("solid.cylinder", json!({"base": [0, 0, 0], "radius": 10, "height": 20})).unwrap();
    h.app.cam = solidcraft_engine::view::home_camera(&h.app.session);
    h.frames(3);
    let side = menu_at(&mut h, [10.0 * 0.6, -10.0 * 0.8, 10.0]);
    assert!(side.iter().any(|l| l == "Look At") && !side.iter().any(|l| l == "Section View"), "{side:?}");
    let top = menu_at(&mut h, [0.0, 0.0, 20.0]);
    assert!(top.iter().any(|l| l == "Section View"), "the flat end is planar: {top:?}");
}

/// Turn Off Section View on a cut face: with one plane it turns the section off; with two it
/// takes out the plane whose cut face was clicked and leaves the other.
#[test]
fn turn_off_section_view_removes_the_clicked_plane() {
    let mut h = box_in_view();
    h.app.run("inspect.section", json!({"plane": {"origin": [0, 15, 0], "normal": [0, -1, 0]}})).unwrap();
    h.frames(3);
    let labels = menu_at(&mut h, [20.0, 15.0, 10.0]);
    assert!(labels.iter().any(|l| l == "Turn Off Section View"), "the cut face's menu: {labels:?}");
    pick(&mut h, "Turn Off Section View");
    assert!(h.app.session.section.is_empty(), "one plane: the section is off");

    // Two planes cut a quarter away (x > 20, y < 15): each cut face is its own plane's.
    let two = |h: &mut Harness| {
        h.app.run("inspect.section", json!({"plane": {"origin": [0, 15, 0], "normal": [0, -1, 0]}})).unwrap();
        h.app.run("inspect.section", json!({"plane": {"origin": [20, 0, 0], "normal": [1, 0, 0]}, "add": true})).unwrap();
        h.frames(3);
    };
    two(&mut h);
    let labels = menu_at(&mut h, [20.0, 7.0, 10.0]);
    assert!(labels.iter().any(|l| l == "Turn Off Section View"), "the second plane's cut face: {labels:?}");
    pick(&mut h, "Turn Off Section View");
    let left: Vec<u64> = h.app.session.section.iter().map(|c| c.id).collect();
    assert_eq!(left, [1], "the clicked plane goes, the other stays");
    two(&mut h);
    menu_at(&mut h, [30.0, 15.0, 10.0]);
    pick(&mut h, "Turn Off Section View");
    let left: Vec<(u64, f64)> = h.app.session.section.iter().map(|c| (c.id, c.normal.x)).collect();
    assert_eq!(left, [(2, 1.0)], "the first plane's cut face takes out the first plane");

    // On a face the section keeps, the menu has no such item.
    let labels = menu_at(&mut h, [10.0, 25.0, 20.0]);
    assert!(labels.iter().any(|l| l == "Section View") && !labels.iter().any(|l| l == "Turn Off Section View"), "{labels:?}");
}
