//! Section Analysis in the browser (#822): an Analysis folder lists each plane of it with an eye
//! to hide and show it, and its menu edits or deletes it. In the viewport, a planar face's menu
//! starts a section on it (or adds its plane), and a cut face's menu edits or turns off its plane.

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

/// Run an item of a section plane's row menu.
fn menu(h: &mut Harness, plane: u64, label: &str) {
    let items = context_menu::items(&h.app, &Target::Section { plane });
    let item = items.iter().find(|i| i.label == label).unwrap_or_else(|| panic!("no {label} in {items:?}")).clone();
    context_menu::run_item(&mut h.app, &item, pos2(0.0, 0.0));
    h.frames(3);
}

#[test]
fn no_analysis_folder_without_a_section() {
    let h = harness();
    assert!(crate::scenario::handle_at("row:Analysis").is_none());
    assert!(crate::scenario::handle_at("row:Section 1").is_none());
    drop(h);
}

#[test]
fn the_eye_hides_and_shows_the_section_and_delete_removes_it() {
    let mut h = harness();
    h.app.run("inspect.section", json!({"plane": "XZ", "offset": 15})).unwrap();
    h.frames(3);
    let section = only_cut(&h);
    assert!(crate::scenario::handle_at("row:Section 1").is_some(), "the section's plane is listed under Analysis");

    click(&mut h, "eye:Section 1");
    assert!(h.app.session.section.is_empty() && h.app.session.section_hidden.len() == 1, "the eye hides the plane");
    assert!(crate::scenario::handle_at("row:Section 1").is_some(), "a hidden plane stays listed");

    click(&mut h, "eye:Section 1");
    let back = only_cut(&h);
    assert!((back.0 - section.0).len() < 1e-9 && (back.1 - section.1).len() < 1e-9, "the same plane: {back:?} vs {section:?}");

    // The folder's eye is its own: hiding it hides the plane without changing the plane's eye,
    // and hiding the plane meanwhile keeps it hidden when the folder shows again.
    click(&mut h, "eye:Analysis");
    assert!(h.app.session.section.is_empty() && h.app.session.section_folder.is_some());
    menu(&mut h, 1, "Show/Hide");
    assert!(h.app.session.section.is_empty(), "the menu hides the plane itself");
    assert!(h.app.session.section_folder.is_some(), "the plane's eye doesn't change the folder's");
    click(&mut h, "eye:Analysis");
    assert!(h.app.session.section.is_empty(), "the plane was hidden on its own meanwhile");
    assert!(!h.app.session.section_folder.is_some());
    menu(&mut h, 1, "Show/Hide");
    assert!(!h.app.session.section.is_empty(), "Show/Hide in the menu brings it back");

    menu(&mut h, 1, "Delete");
    assert!(h.app.session.section.is_empty() && h.app.session.section_hidden.is_empty());
    assert!(crate::scenario::handle_at("row:Section 1").is_none(), "a deleted section leaves the browser");
}

fn ids(planes: &[solidcraft_engine::SectionPlane]) -> Vec<u64> {
    planes.iter().map(|c| c.id).collect()
}

/// The Analysis folder's eye belongs to its design: a section in another tab leaves it alone,
/// and coming back the folder's eye still brings its planes back.
#[test]
fn the_folder_eye_stays_with_its_design() {
    let mut h = box_in_view();
    corner(&mut h);
    click(&mut h, "eye:Analysis");
    assert_eq!(h.app.session.section_folder, Some(vec![1, 2]));
    crate::documents::open_tab(&mut h.app, solidcraft_engine::Session::default()).unwrap();
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
    h.app.run("inspect.section", json!({"plane": "XY", "at": 5})).unwrap();
    h.frames(3);
    assert_eq!(h.app.session.section_folder, None);
    crate::documents::switch(&mut h.app, 0);
    h.frames(3);
    assert_eq!((ids(&h.app.session.section), h.app.session.section_folder.clone()), (vec![], Some(vec![1, 2])));
    click(&mut h, "eye:Analysis");
    assert_eq!(ids(&h.app.session.section), [1, 2]);
}

/// A plane added while the Analysis folder is hidden shows the folder again, and the planes it
/// hid come back with it.
#[test]
fn a_plane_added_while_the_folder_is_hidden_brings_its_planes_back() {
    let mut h = box_in_view();
    corner(&mut h);
    click(&mut h, "eye:Analysis");
    assert!(h.app.session.section.is_empty());
    menu_at(&mut h, [20.0, 15.0, 20.0]);
    pick(&mut h, "Section View");
    ok(&mut h);
    assert_eq!((ids(&h.app.session.section), h.app.session.section_folder.clone()), (vec![1, 2, 3], None));
    assert!(
        context_menu::items(&h.app, &Target::Section { plane: 1 })
            .iter()
            .any(|i| i.label == "Show/Hide" && i.params.as_ref().is_some_and(|p| p["visible"] == false))
    );
}

#[test]
fn deleting_a_hidden_section_forgets_it() {
    let mut h = harness();
    h.app.run("inspect.section", json!({"plane": "XY", "offset": 2})).unwrap();
    h.frames(3);
    click(&mut h, "eye:Section 1");
    menu(&mut h, 1, "Delete");
    assert!(h.app.session.section.is_empty() && h.app.session.section_hidden.is_empty());
    assert!(crate::scenario::handle_at("row:Analysis").is_none());
}

#[test]
fn edit_opens_the_section_dialog() {
    let mut h = harness();
    h.app.run("inspect.section", json!({"plane": "YZ"})).unwrap();
    h.frames(3);
    menu(&mut h, 1, "Edit Section Analysis");
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
    assert!((cut.0.z + 12.0).abs() < 1e-9, "12 mm against XY's normal: {cut:?}");

    // The toolbar starts a new section (in place of this one), not an edit of it.
    h.app.session.selection.clear();
    h.app.start("inspect.section");
    let (offset, _, plane) = section_dialog(&h);
    assert_eq!(offset, "0 mm", "a new section starts at its plane");
    assert!(plane.is_empty(), "a new section picks its own plane: {plane:?}");
    crate::dialogs::cancel(&mut h.app);
    h.frames(2);
    assert!((only_cut(&h).0.z + 12.0).abs() < 1e-9, "cancelling keeps the section");

    menu(&mut h, 1, "Edit Section Analysis");
    let (offset, flip, plane) = section_dialog(&h);
    assert_eq!((offset.as_str(), flip), ("12 mm", false));
    assert_eq!(plane, vec![solidcraft_engine::Sel::Plane { name: "XY".into() }]);
    crate::dialogs::cancel(&mut h.app);
    h.frames(2);

    // Hidden and shown again, it is still the section the dialog made.
    click(&mut h, "eye:Section 1");
    click(&mut h, "eye:Section 1");
    menu(&mut h, 1, "Edit Section Analysis");
    assert_eq!(section_dialog(&h).0, "12 mm");
    crate::dialogs::cancel(&mut h.app);
    h.frames(2);

    // A section made some other way (here a command) is not the dialog's: Edit starts afresh.
    h.app.run("inspect.section", json!({"plane": "XZ", "offset": 5})).unwrap();
    h.frames(3);
    menu(&mut h, 1, "Edit Section Analysis");
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

    click(&mut h, "eye:Section 1");
    click(&mut h, "eye:Section 1");
    assert!(!h.app.session.section.is_empty() && !h.app.session.section_hatch, "shown again, still plain");
    menu(&mut h, 1, "Edit Section Analysis");
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
    assert!(crate::scenario::handle_at("row:Section 1").is_none(), "the browser no longer lists it");

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
/// its outward normal: at distance 0 it cuts nothing, and a positive distance cuts into the part,
/// where the arrow points: dragged in, the distance grows.
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
    // The arrow dragged down into the box: a positive distance, the cut where it was let go.
    h.step(&json!({"drag": [{"handle": "arrow"}, {"world": [20, 15, 12]}]})).unwrap();
    h.frames(2);
    let (offset, ..) = section_dialog(&h);
    let dragged = crate::length_input::eval(&h.app.session.doc, &offset, solidcraft_engine::doc::expr::Kind::Length).unwrap();
    assert!((dragged - 8.0).abs() < 1.0, "dragged 8 mm in: {offset}");
    let [cut] = crate::viewport::section_planes(&h.app)[..] else { panic!("one plane") };
    assert!((cut.origin.z - (20.0 - dragged)).abs() < 1e-9, "{cut:?}");
    set_offset(&mut h, "5 mm");
    h.frames(2);
    let tip = crate::scenario::handle_at("arrow").expect("the arrow");
    let at = h.at(&json!({"world": [20, 15, 15]})).unwrap();
    assert!((tip.x - at[0] as f32).abs() < 2.0 && (tip.y - at[1] as f32).abs() < 2.0, "the arrow's tip sits on the cut: {tip:?} {at:?}");
    ok(&mut h);
    let (origin, normal) = only_cut(&h);
    assert!((normal - solidcraft_engine::geom::Vec3::Z).len() < 1e-9 && (origin.z - 15.0).abs() < 1e-9, "{origin:?} {normal:?}");
}

/// With a section on, Section View on another face adds that face's plane to it: the dialog
/// says so, opens unflipped as the first plane did, the preview keeps the planes on show, and OK
/// leaves both. At 0 the new plane hides nothing more; moved in, the section hides what lies
/// beyond both planes.
#[test]
fn section_view_adds_a_face_plane_to_the_section_on_show() {
    let mut h = box_in_view();
    h.app.run("inspect.section", json!({"plane": {"origin": [0, 15, 0], "normal": [0, -1, 0]}, "hatch": false})).unwrap();
    h.frames(3);
    let labels = menu_at(&mut h, [40.0, 22.0, 10.0]);
    assert!(labels.iter().any(|l| l == "Section View"), "{labels:?}");
    pick(&mut h, "Section View");
    let kind = h.app.dialog.as_ref().map(|d| d.kind.clone());
    assert!(matches!(kind, Some(crate::dialogs::Kind::Section { add: true, flip: false, hatch: false, .. })), "{kind:?}");
    h.frames(2);
    assert!(crate::scenario::handle_at("dialog:hint:section_add").is_some(), "the dialog says the plane joins the section");
    let preview = crate::viewport::section_planes(&h.app);
    assert_eq!(preview.len(), 2, "the preview keeps the section's plane");
    assert!(!hidden(&preview, [39.0, 5.0, 10.0]), "at the end face, the new plane hides nothing more: {preview:?}");
    assert!(!crate::viewport::section_hatch(&h.app), "the preview keeps the section plain");
    set_offset(&mut h, "20 mm");
    h.frames(2);
    let preview = crate::viewport::section_planes(&h.app);
    assert!(hidden(&preview, [30.0, 5.0, 10.0]) && !hidden(&preview, [10.0, 5.0, 10.0]) && !hidden(&preview, [30.0, 20.0, 10.0]), "{preview:?}");
    ok(&mut h);
    let planes: Vec<_> = h.app.session.section.iter().map(|c| (c.id, c.normal)).collect();
    assert_eq!(planes.len(), 2, "{planes:?}");
    assert!((planes[1].1 - solidcraft_engine::geom::Vec3::X).len() < 1e-9 && (h.app.session.section[1].origin.x - 20.0).abs() < 1e-9, "{planes:?}");
    assert!(!h.app.session.section_hatch, "a plain section stays plain");
    assert!(hidden(&h.app.session.section, [30.0, 5.0, 10.0]) && !hidden(&h.app.session.section, [10.0, 5.0, 10.0]));
}

/// Whether a section of `planes` hides the world point `p`.
fn hidden(planes: &[solidcraft_engine::SectionPlane], p: [f64; 3]) -> bool {
    let p = solidcraft_engine::geom::Vec3::new(p[0], p[1], p[2]);
    !planes.is_empty() && !planes.iter().any(|c| c.keeps(p, 0.0))
}

/// Two planes by Section View (#8717): from the long front face 10 mm in, then from the end
/// face 18 mm in.
fn corner(h: &mut Harness) {
    menu_at(h, [20.0, 0.0, 10.0]);
    pick(h, "Section View");
    set_offset(h, "10 mm");
    ok(h);
    menu_at(h, [40.0, 22.0, 10.0]);
    pick(h, "Section View");
    set_offset(h, "18 mm");
    ok(h);
}

/// The two planes cut away the corner they both hide, and each flips on its own: the second's
/// Flip leaves the first as it was.
#[test]
fn each_plane_flips_on_its_own() {
    let mut h = box_in_view();
    corner(&mut h);
    let planes = h.app.session.section.clone();
    assert_eq!(planes.len(), 2);
    assert!(hidden(&planes, [30.0, 5.0, 10.0]), "the corner is cut: {planes:?}");
    assert!(!hidden(&planes, [10.0, 5.0, 10.0]) && !hidden(&planes, [30.0, 20.0, 10.0]), "only the corner: {planes:?}");
    assert!((planes[0].origin.y - 10.0).abs() < 1e-9 && (planes[1].origin.x - 22.0).abs() < 1e-9, "{planes:?}");

    // Edit the second plane and flip it: the preview turns it alone round, and so does OK.
    menu(&mut h, 2, "Edit Section Analysis");
    let kind = h.app.dialog.as_ref().map(|d| d.kind.clone());
    assert!(matches!(kind, Some(crate::dialogs::Kind::Section { flip: false, edit: Some(2), .. })), "{kind:?}");
    if let Some(crate::dialogs::Kind::Section { flip, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        *flip = true;
    }
    h.frames(2);
    let preview = crate::viewport::section_planes(&h.app);
    assert_eq!(preview[0], planes[0], "the first plane keeps its side");
    assert!((preview[1].normal + solidcraft_engine::geom::Vec3::X).len() < 1e-9 && (preview[1].origin.x - 22.0).abs() < 1e-9, "{preview:?}");
    ok(&mut h);
    let after = h.app.session.section.clone();
    assert!(after[0] == planes[0] && (after[1].normal + solidcraft_engine::geom::Vec3::X).len() < 1e-9, "{after:?}");
    assert!(hidden(&after, [10.0, 5.0, 10.0]) && !hidden(&after, [30.0, 5.0, 10.0]));
    // Editing the first plane opens it unflipped, as it was left.
    menu(&mut h, 1, "Edit Section Analysis");
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(crate::dialogs::Kind::Section { flip: false, edit: Some(1), .. })));
}

/// Each plane has its row under Analysis, with its own eye, double-click and menu.
#[test]
fn each_plane_has_its_own_row() {
    let mut h = box_in_view();
    corner(&mut h);
    assert!(crate::scenario::handle_at("row:Section 1").is_some() && crate::scenario::handle_at("row:Section 2").is_some());
    assert!(crate::scenario::handle_at("row:Section Analysis").is_none(), "the planes are the rows");
    let double = |h: &mut Harness, row: &str| {
        // Clear of the clicks before, so egui reads two clicks, not three.
        h.frames(60);
        h.step(&json!({"click": {"handle": row}, "double": true})).unwrap();
        h.frames(3);
        let kind = h.app.dialog.as_ref().map(|d| d.kind.clone());
        crate::dialogs::cancel(&mut h.app);
        h.frames(2);
        kind
    };
    let kind = double(&mut h, "row:Section 1");
    assert!(matches!(&kind, Some(crate::dialogs::Kind::Section { edit: Some(1), offset, .. }) if offset == "10 mm"), "{kind:?}");
    let kind = double(&mut h, "row:Section 2");
    assert!(matches!(&kind, Some(crate::dialogs::Kind::Section { edit: Some(2), offset, .. }) if offset == "18 mm"), "{kind:?}");

    // One eye hides its plane alone.
    click(&mut h, "eye:Section 2");
    let ids = |planes: &[solidcraft_engine::SectionPlane]| planes.iter().map(|c| c.id).collect::<Vec<_>>();
    assert_eq!((ids(&h.app.session.section), ids(&h.app.session.section_hidden)), (vec![1], vec![2]));
    assert!(crate::scenario::handle_at("row:Section 2").is_some(), "a hidden plane keeps its row");
    click(&mut h, "eye:Section 2");
    assert_eq!(ids(&h.app.session.section), [1, 2]);

    // Delete takes out the row's plane; the last one turns the section off.
    menu(&mut h, 1, "Delete");
    assert_eq!(ids(&h.app.session.section), [2]);
    assert!(crate::scenario::handle_at("row:Section 1").is_none() && crate::scenario::handle_at("row:Section 2").is_some());
    menu(&mut h, 2, "Delete");
    assert!(h.app.session.section.is_empty() && crate::scenario::handle_at("row:Analysis").is_none());
}

/// A cut face's menu offers Edit Section View right above Turn Off Section View: it opens the
/// dialog on that face's plane, where it was left.
#[test]
fn edit_section_view_edits_the_plane_clicked() {
    let mut h = box_in_view();
    corner(&mut h);
    for (at, id, offset) in [([22.0, 5.0, 10.0], 2, "18 mm"), ([30.0, 10.0, 10.0], 1, "10 mm")] {
        let labels = menu_at(&mut h, at);
        let edit = labels.iter().position(|l| l == "Edit Section View").unwrap_or_else(|| panic!("{labels:?}"));
        assert_eq!(labels.get(edit + 1).map(String::as_str), Some("Turn Off Section View"), "{labels:?}");
        pick(&mut h, "Edit Section View");
        let kind = h.app.dialog.as_ref().map(|d| d.kind.clone());
        assert!(matches!(&kind, Some(crate::dialogs::Kind::Section { edit: Some(e), offset: o, .. }) if *e == id && o == offset), "{at:?}: {kind:?}");
        crate::dialogs::cancel(&mut h.app);
        h.frames(2);
    }
}

/// Starting, adding, moving, flipping, hiding and removing section planes are undo steps, one
/// per OK or menu action; the dialog's preview takes none. Undo brings each section back.
#[test]
fn section_changes_are_undone_in_the_app() {
    let mut h = box_in_view();
    h.app.session.mark_saved();
    let start = h.app.session.undo.len();
    let mut states = vec![h.app.session.section_state()];
    menu_at(&mut h, [20.0, 0.0, 10.0]);
    pick(&mut h, "Section View");
    for d in ["4 mm", "8 mm", "10 mm"] {
        set_offset(&mut h, d);
        h.frames(2);
    }
    assert_eq!(h.app.session.undo.len(), start, "a preview takes no undo step");
    ok(&mut h);
    states.push(h.app.session.section_state());
    menu_at(&mut h, [40.0, 22.0, 10.0]);
    pick(&mut h, "Section View");
    set_offset(&mut h, "18 mm");
    ok(&mut h);
    states.push(h.app.session.section_state());
    menu(&mut h, 2, "Edit Section Analysis");
    if let Some(crate::dialogs::Kind::Section { flip, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        *flip = true;
    }
    ok(&mut h);
    states.push(h.app.session.section_state());
    click(&mut h, "eye:Section 1");
    states.push(h.app.session.section_state());
    menu(&mut h, 2, "Delete");
    states.push(h.app.session.section_state());
    assert_eq!(h.app.session.undo.len(), start + 5, "one step each");
    for want in states.iter().rev().skip(1) {
        h.app.run("edit.undo", json!({})).unwrap();
        h.frames(2);
        assert_eq!(&h.app.session.section_state(), want);
    }
    assert!(crate::scenario::handle_at("row:Analysis").is_none(), "undone to no section");
    h.app.run("edit.redo", json!({})).unwrap();
    h.app.run("edit.redo", json!({})).unwrap();
    h.frames(3);
    assert_eq!(h.app.session.section_state(), states[2]);
    assert!(crate::scenario::handle_at("row:Section 2").is_some(), "redone, the rows are back");
    assert!(!h.app.session.is_dirty(), "a section is not part of the design");
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

/// Edit on a section of several planes edits the plane in place: the preview keeps the others,
/// the dialog says so, OK with no plane picked keeps the section, and a plane Section View added
/// reopens where it was left, its OK moving that plane alone.
#[test]
fn edit_moves_one_plane_of_several() {
    let mut h = box_in_view();
    h.app.run("inspect.section", json!({"plane": {"origin": [0, 15, 0], "normal": [0, -1, 0]}})).unwrap();
    h.frames(3);
    menu_at(&mut h, [40.0, 22.0, 10.0]);
    pick(&mut h, "Section View");
    set_offset(&mut h, "20 mm");
    ok(&mut h);
    let before = h.app.session.section.clone();
    assert_eq!(before.len(), 2);

    menu(&mut h, before[1].id, "Edit Section Analysis");
    let kind = h.app.dialog.as_ref().map(|d| d.kind.clone());
    let Some(crate::dialogs::Kind::Section { offset, flip: false, edit: Some(id), .. }) = kind else { panic!("{kind:?}") };
    assert_eq!((offset.as_str(), id), ("20 mm", before[1].id), "the plane Section View added, where it was left");
    h.frames(2);
    assert!(crate::scenario::handle_at("dialog:hint:section_edit").is_some(), "the dialog says it edits one plane");
    assert!(crate::scenario::handle_at("dialog:hint:section_off").is_none());
    assert_eq!(crate::viewport::section_planes(&h.app), before, "the preview is the section as it is");
    set_offset(&mut h, "30 mm");
    h.frames(2);
    let preview = crate::viewport::section_planes(&h.app);
    assert!(preview.len() == 2 && preview[0] == before[0] && (preview[1].origin.x - 10.0).abs() < 1e-9, "{preview:?}");
    ok(&mut h);
    let after = h.app.session.section.clone();
    assert!(after.len() == 2 && after[0] == before[0] && after[1].id == before[1].id && (after[1].origin.x - 10.0).abs() < 1e-9, "{after:?}");

    // A section of several planes made some other way: Edit moves the plane, and OK with none
    // picked asks for one instead of turning the section off.
    h.app.run("inspect.section", json!({"plane": "XZ", "offset": 15})).unwrap();
    h.app.run("inspect.section", json!({"plane": "YZ", "offset": 20, "add": true})).unwrap();
    h.frames(3);
    let before = h.app.session.section.clone();
    menu(&mut h, 2, "Edit Section Analysis");
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

    // Hidden, the plane stays hidden while the dialog is open; OK shows it where it is moved.
    click(&mut h, "eye:Section 2");
    menu(&mut h, 2, "Edit Section Analysis");
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(crate::dialogs::Kind::Section { edit: Some(2), .. })));
    assert_eq!((h.app.session.section.len(), h.app.session.section_hidden.len()), (1, 1), "opening the dialog changes nothing");
    if let Some(d) = h.app.dialog.as_mut() {
        d.inputs[0].items = vec![solidcraft_engine::Sel::Plane { name: "YZ".into() }];
    }
    set_offset(&mut h, "-25 mm");
    ok(&mut h);
    let ids: Vec<u64> = h.app.session.section.iter().map(|c| c.id).collect();
    assert!(
        ids == [1, 2] && h.app.session.section_hidden.is_empty() && (h.app.session.section[1].origin.x - 25.0).abs() < 1e-9,
        "{:?}",
        h.app.session.section
    );
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
