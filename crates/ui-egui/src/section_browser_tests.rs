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

#[test]
fn contribution_quarter_section_hide_show_edit_and_delete_retain_both_planes() {
    let mut h = harness();
    h.app.run("inspect.section", json!({"plane":"YZ","at":4,"second":{"plane":"XZ","at":6,"flip":true}})).unwrap();
    let cut = (h.app.session.section, h.app.session.section_second);
    crate::browser::set_section_visible(&mut h.app, false);
    assert!(h.app.session.section.is_none() && h.app.session.section_second.is_none());
    crate::browser::set_section_visible(&mut h.app, true);
    assert_eq!((h.app.session.section, h.app.session.section_second), cut);
    crate::dialogs::edit_section(&mut h.app);
    let cmds = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    let mut scratch = h.app.session.scratch();
    scratch.execute(&cmds[0].0, &cmds[0].1).unwrap();
    assert_eq!((scratch.section, scratch.section_second), cut);
    crate::dialogs::cancel(&mut h.app);
    assert_eq!((h.app.session.section, h.app.session.section_second), cut);
    crate::browser::set_section_visible(&mut h.app, false);
    menu(&mut h, "Delete");
    assert!(h.app.session.section.is_none() && h.app.session.section_second.is_none());
    assert!(h.app.tree.hidden_section.is_none() && h.app.tree.hidden_section_second.is_none());
}

#[test]
fn edit_restores_both_authored_groups_but_toolbar_starts_a_new_analysis() {
    use crate::dialogs::Kind;
    use solidcraft_engine::Sel;
    let mut h = harness();
    h.app.session.selection = vec![Sel::Plane { name: "XY".into() }];
    h.app.start("inspect.section");
    h.frames(2);
    click(&mut h, "dialog:Quarter section");
    let d = h.app.dialog.as_mut().unwrap();
    d.inputs[1].items = vec![Sel::Plane { name: "YZ".into() }];
    if let Kind::Section { offset, flip, second_offset, second_flip, .. } = &mut d.kind {
        *offset = "2 mm".into();
        *flip = true;
        *second_offset = "3 mm".into();
        *second_flip = true;
    }
    ok(&mut h);
    let cut = (h.app.session.section, h.app.session.section_second);
    h.app.session.selection.clear();
    h.app.start("inspect.section");
    let d = h.app.dialog.as_ref().unwrap();
    assert_eq!(d.inputs.len(), 1);
    assert!(d.inputs[0].items.is_empty());
    assert!(matches!(&d.kind, Kind::Section { offset, quarter: false, .. } if offset == "0 mm"));
    crate::dialogs::cancel(&mut h.app);
    crate::browser::set_section_visible(&mut h.app, false);
    crate::dialogs::edit_section(&mut h.app);
    let d = h.app.dialog.as_ref().unwrap();
    assert_eq!(d.inputs.len(), 2);
    assert_eq!(d.inputs[0].items, [Sel::Plane { name: "XY".into() }]);
    assert_eq!(d.inputs[1].items, [Sel::Plane { name: "YZ".into() }]);
    assert!(
        matches!(&d.kind, Kind::Section { offset, flip: true, quarter: true, second_offset, second_flip: true } if offset == "2 mm" && second_offset == "3 mm")
    );
    let commands = crate::dialogs::apply_commands(&h.app, d).unwrap();
    let mut scratch = h.app.session.scratch();
    scratch.execute(&commands[0].0, &commands[0].1).unwrap();
    assert_eq!((scratch.section, scratch.section_second), cut);
    crate::dialogs::cancel(&mut h.app);
    crate::browser::set_section_visible(&mut h.app, true);
    assert_eq!((h.app.session.section, h.app.session.section_second), cut);
}

#[test]
fn command_quarter_edit_preserves_cut_despite_preselected_plane() {
    use solidcraft_engine::Sel;
    for name in ["XY", "XZ"] {
        let mut h = harness();
        h.app.run("inspect.section", json!({"plane":"YZ","at":4,"second":{"plane":"XZ","at":6,"flip":true}})).unwrap();
        let cut = (h.app.session.section, h.app.session.section_second);
        h.app.session.selection = vec![Sel::Plane { name: name.into() }];
        crate::dialogs::edit_section(&mut h.app);
        let d = h.app.dialog.as_ref().unwrap();
        assert!(d.inputs.iter().all(|input| input.items.is_empty()), "Edit must use both effective planes, not toolbar preselection {name}");
        let commands = crate::dialogs::apply_commands(&h.app, d).unwrap();
        let mut scratch = h.app.session.scratch();
        scratch.execute(&commands[0].0, &commands[0].1).unwrap();
        assert_eq!((scratch.section, scratch.section_second), cut);
        crate::dialogs::cancel(&mut h.app);
        assert_eq!((h.app.session.section, h.app.session.section_second), cut);
    }
}

#[test]
fn contribution_cpu_quarter_section_caps_both_exposed_planes() {
    use solidcraft_engine::{Session, geom::Vec3};
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
    s.execute("inspect.section", &json!({"plane":"YZ","at":5,"second":{"plane":"XZ","at":5,"flip":true}})).unwrap();
    let cam = solidcraft_engine::render::Camera { target: Vec3::new(5.0, 5.0, 5.0), distance: 30.0, ..Default::default() };
    let scene = solidcraft_engine::view::scene(&s, &cam);
    let image = solidcraft_engine::render::render(&scene, &cam, 200, 200);
    let vp = cam.view_proj(1.0, scene.radius);
    for point in [Vec3::new(5.0, 2.5, 5.0), Vec3::new(7.5, 5.0, 5.0)] {
        let q = vp.apply(point);
        let x = ((q[0] / q[3] + 1.0) * 100.0) as usize;
        let y = ((1.0 - q[1] / q[3]) * 100.0) as usize;
        let i = (y * 200 + x) * 4;
        let rgb = &image.rgba[i..i + 3];
        assert!(i < image.rgba.len() && i16::from(rgb[0]) > i16::from(rgb[2]) + 10, "{point:?}: {rgb:?}");
    }
}

#[test]
fn contribution_quarter_section_respects_both_browser_eyes() {
    let mut h = harness();
    h.app.run("inspect.section", json!({"plane":"YZ","at":4,"second":{"plane":"XZ","at":6,"flip":true}})).unwrap();
    h.frames(3);
    let cut = (h.app.session.section, h.app.session.section_second);
    click(&mut h, "eye:Analysis");
    assert!(h.app.session.section.is_none() && h.app.session.section_second.is_none());
    assert_eq!((h.app.tree.hidden_section, h.app.tree.hidden_section_second), cut);
    click(&mut h, "eye:Section Analysis");
    click(&mut h, "eye:Analysis");
    assert!(h.app.session.section.is_none() && h.app.session.section_second.is_none());
    click(&mut h, "eye:Section Analysis");
    assert_eq!((h.app.session.section, h.app.session.section_second), cut);
    assert!(h.app.tree.hidden_section.is_none() && h.app.tree.hidden_section_second.is_none());
    menu(&mut h, "Delete");
    assert!(h.app.session.section.is_none() && h.app.session.section_second.is_none());
}

#[test]
fn contribution_quarter_second_picker_uses_the_same_browser_selection_as_first() {
    use solidcraft_engine::Sel;
    let mut h = harness();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
    h.app.tree.expanded.insert("origin".into());
    crate::dialogs::edit_section(&mut h.app);
    h.frames(3);
    click(&mut h, "row:XY");
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items, [Sel::Plane { name: "XY".into() }]);
    click(&mut h, "dialog:Quarter section");
    assert_eq!(h.app.dialog.as_ref().unwrap().active, 1);
    click(&mut h, "row:YZ");
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[1].items, [Sel::Plane { name: "YZ".into() }]);
    click(&mut h, "dialog:Plane");
    click(&mut h, "row:XZ");
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items, [Sel::Plane { name: "XZ".into() }]);
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[1].items, [Sel::Plane { name: "YZ".into() }]);
    click(&mut h, "dialog:Second offset");
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"5 mm"})).unwrap();
    click(&mut h, "dialog:Second flip");
    let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(commands[0].1["plane"], "XZ");
    assert_eq!(commands[0].1["second"]["plane"], "YZ");
    assert_eq!(commands[0].1["second"]["offset"], "5 mm");
    assert_eq!(commands[0].1["second"]["flip"], true);
    let mut scratch = h.app.session.scratch();
    scratch.execute(&commands[0].0, &commands[0].1).unwrap();
    crate::dialogs::cancel(&mut h.app);
    assert!(h.app.session.section.is_none() && h.app.session.section_second.is_none());
}

#[test]
fn contribution_quarter_saved_pickers_survive_half_quarter_and_remove() {
    let mut h = harness();
    h.app.run("inspect.section", json!({"plane":"YZ","at":4,"second":{"plane":"XZ","at":6,"flip":true}})).unwrap();
    let cut = (h.app.session.section, h.app.session.section_second);
    crate::dialogs::edit_section(&mut h.app);
    h.frames(3);
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs.len(), 2);
    click(&mut h, "dialog:Quarter section");
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs.len(), 1);
    let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert!(commands[0].1.get("second").is_none());
    click(&mut h, "dialog:Quarter section");
    let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    let mut scratch = h.app.session.scratch();
    scratch.execute(&commands[0].0, &commands[0].1).unwrap();
    assert_eq!((scratch.section, scratch.section_second), cut);
    crate::dialogs::cancel(&mut h.app);
    assert_eq!((h.app.session.section, h.app.session.section_second), cut);
    crate::dialogs::edit_section(&mut h.app);
    h.frames(3);
    click(&mut h, "dialog:Remove Section");
    assert!(h.app.dialog.is_none());
    assert!(h.app.session.section.is_none() && h.app.session.section_second.is_none());
}

#[test]
fn contribution_quarter_both_pickers_refuse_nonplanar_faces_and_unknown_planes() {
    use solidcraft_engine::{Sel, geom::Vec3};
    let mut h = harness();
    h.app.run("solid.cylinder", json!({"diameter":10,"height":20,"body_name":"Shaft"})).unwrap();
    h.app.run("inspect.section", json!({"plane":"XY","second":{"plane":"YZ"}})).unwrap();
    let world = h.app.session.world_state();
    let body = world.body("Shaft").unwrap();
    let count = body.mesh().tri_face.iter().max().copied().unwrap() as usize + 1;
    let curved = (0..count).find(|index| crate::dialogs::planar_face(&h.app.session, "Shaft", *index).is_none()).unwrap();
    for input in [0, 1] {
        crate::dialogs::edit_section(&mut h.app);
        let mut dialog = h.app.dialog.take().unwrap();
        dialog.active = input;
        let hit = crate::viewport::Hit::Face { body: "Shaft".into(), index: curved, point: Vec3::ZERO };
        assert!(dialog.candidate_on(&h.app.session, &hit, None).is_none());
        dialog.inputs[input].items = vec![Sel::Face { body: "Shaft".into(), index: curved, point: Vec3::ZERO }];
        assert!(crate::dialogs::apply_commands(&h.app, &dialog).unwrap_err().contains("planar"));
        dialog.inputs[input].items = vec![Sel::Plane { name: "Missing plane".into() }];
        assert!(crate::dialogs::apply_commands(&h.app, &dialog).unwrap_err().contains("unknown plane"));
    }
}
#[test]
fn quarter_fields_follow_their_plane_and_canvas_changes_only_the_active_distance() {
    use crate::dialogs::Kind;
    use solidcraft_engine::geom::Plane;
    let mut h = harness();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
    h.app.tree.expanded.insert("origin".into());
    crate::dialogs::edit_section(&mut h.app);
    h.frames(3);
    click(&mut h, "row:XY");
    click(&mut h, "dialog:Quarter section");
    click(&mut h, "row:YZ");
    let y = |name: &str| crate::scenario::handle_at(name).unwrap().y;
    assert!(y("dialog:Plane") < y("dialog:Distance"));
    assert!(y("dialog:Flip") < y("dialog:Quarter section"));
    assert!(y("dialog:Quarter section") < y("dialog:Second plane"));
    assert!(y("dialog:Second plane") < y("dialog:Second offset"));
    assert!(y("dialog:Second flip") < y("dialog:Remove Section"));
    for (index, name, handle) in [(1, "YZ", "dialog:Second plane"), (0, "XY", "dialog:Plane")] {
        click(&mut h, handle);
        let d = h.app.dialog.as_ref().unwrap();
        assert_eq!(d.active, index);
        let (_, Some(normal)) = crate::canvas::anchor(&h.app, d).unwrap() else { panic!("missing direction") };
        assert!((normal - Plane::named(name).unwrap().normal()).len() < 1e-9);
        let arrow = crate::scenario::handle_at("arrow").unwrap();
        h.step(&json!({"drag":[[arrow.x,arrow.y],[arrow.x+70.0,arrow.y-50.0]],"steps":15})).unwrap();
        h.frames(3);
        let Kind::Section { offset, second_offset, .. } = &h.app.dialog.as_ref().unwrap().kind else { panic!() };
        if index == 1 {
            assert_eq!(offset, "0 mm");
            assert_ne!(second_offset, "0 mm");
        } else {
            assert_ne!(offset, "0 mm");
        }
    }
}

#[test]
fn reopened_section_keeps_both_stored_reference_arrows() {
    let mut h = harness();
    h.app.run("inspect.section", json!({"plane":"YZ","at":4,"flip":true,"second":{"plane":"XZ","at":6,"flip":false}})).unwrap();
    let before = (h.app.session.section, h.app.session.section_second);
    crate::dialogs::edit_section(&mut h.app);
    h.frames(3);
    for index in [0, 1] {
        h.app.dialog.as_mut().unwrap().active = index;
        let d = h.app.dialog.as_ref().unwrap();
        assert!(d.inputs[index].items.is_empty());
        let (_, Some(normal)) = crate::canvas::anchor(&h.app, d).unwrap() else { panic!() };
        let reference = if index == 0 { before.0.unwrap() } else { before.1.unwrap() };
        assert!((normal - reference.1).len() < 1e-9);
    }
    crate::dialogs::cancel(&mut h.app);
    assert_eq!((h.app.session.section, h.app.session.section_second), before);
}
#[test]
fn quarter_flip_pairs_remove_both_external_halfspaces_and_keep_volume() {
    use solidcraft_engine::{Session, geom::Vec3};
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
    let before = s.execute("inspect.measure", &json!({})).unwrap();
    for first_flip in [false, true] {
        for second_flip in [false, true] {
            s.execute("inspect.section", &json!({"plane":"YZ","at":5,"flip":first_flip,"second":{"plane":"XZ","at":5,"flip":second_flip}})).unwrap();
            let cam = solidcraft_engine::render::Camera::default();
            let scene = solidcraft_engine::view::scene(&s, &cam);
            let mut removed = 0;
            for x in [2.5, 7.5] {
                for y in [2.5, 7.5] {
                    let p = Vec3::new(x, y, 5.0);
                    let first = s.section.unwrap();
                    let second = s.section_second.unwrap();
                    let expected = first.1.dot(p - first.0) > 0.0 || second.1.dot(p - second.0) > 0.0;
                    let clipped = solidcraft_engine::render::section_clipped(p, scene.clip, scene.clip_second);
                    assert_eq!(clipped, expected, "{first_flip}/{second_flip}: {p:?}");
                    removed += usize::from(clipped);
                }
            }
            assert_eq!(removed, 3, "a quarter cut keeps one quadrant");
            let after = s.execute("inspect.measure", &json!({})).unwrap();
            assert_eq!(before["total"]["volume_mm3"], after["total"]["volume_mm3"]);
        }
    }
}
#[test]
fn quarter_toggle_keeps_both_authored_offsets_and_flip_states() {
    use crate::dialogs::Kind;
    let mut h = harness();
    h.app.tree.expanded.insert("origin".into());
    crate::dialogs::edit_section(&mut h.app);
    h.frames(3);
    click(&mut h, "row:XY");
    click(&mut h, "dialog:Quarter section");
    click(&mut h, "row:YZ");
    if let Kind::Section { offset, second_offset, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
        *offset = "2 mm".into();
        *second_offset = "3 mm".into();
    }
    click(&mut h, "dialog:Flip");
    assert_eq!(h.app.dialog.as_ref().unwrap().active, 0);
    click(&mut h, "dialog:Second flip");
    assert_eq!(h.app.dialog.as_ref().unwrap().active, 1);
    let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    let mut scratch = h.app.session.scratch();
    scratch.execute(&commands[0].0, &commands[0].1).unwrap();
    let first = scratch.section;
    click(&mut h, "dialog:Quarter section");
    let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    scratch.execute(&commands[0].0, &commands[0].1).unwrap();
    assert_eq!(scratch.section, first, "switching to half preserves the first plane");
    assert!(scratch.section_second.is_none());
    click(&mut h, "dialog:Quarter section");
    let d = h.app.dialog.as_ref().unwrap();
    let Kind::Section { offset, flip, second_offset, second_flip, .. } = &d.kind else { panic!() };
    assert_eq!((offset.as_str(), second_offset.as_str()), ("2 mm", "3 mm"));
    assert!(*flip && *second_flip);
    assert_eq!(d.inputs[1].items, [solidcraft_engine::Sel::Plane { name: "YZ".into() }]);
    let commands = crate::dialogs::apply_commands(&h.app, d).unwrap();
    scratch.execute(&commands[0].0, &commands[0].1).unwrap();
    assert_eq!(scratch.section, first, "enabling quarter preserves the first plane");
}
