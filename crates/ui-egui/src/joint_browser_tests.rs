//! Joints in the browser (#1160): an eye per joint and on the Joints folder hides and shows
//! joint glyphs; Show/Hide in a joint's menu and the V key do the same. Hiding only hides the
//! drawing: the joint still constrains and drives. A hidden joint picked in the browser draws
//! highlighted until the pick goes away.

use egui::pos2;
use serde_json::json;

use crate::context_menu::{self, Target};
use crate::scenario::Harness;
use crate::test_support::click;

/// A picked joint lights the parts it joins and draws alone, until it is unpicked.
#[test]
fn a_picked_joint_lights_its_parts_and_fades_the_rest() {
    let mut h = harness();
    let [a, b, c] = ["A:1", "B:1", "C:1"].map(|name| occurrence(&h, name));
    let (hinge, glue) = (id(&h, "Hinge"), id(&h, "Glue"));
    assert_eq!(crate::viewport::solid_bodies(&h.app), None);

    crate::dialogs_assembly::pick_joint(&mut h.app, hinge, false);
    h.frames(2);
    let mut parts: Vec<_> = [a, b].iter().flat_map(|o| h.app.session.occurrence_bodies(&[*o], true).unwrap()).collect();
    parts.sort();
    assert_eq!(crate::viewport::solid_bodies(&h.app).as_ref(), Some(&parts));
    assert!(!parts.iter().any(|p| h.app.session.occurrence_bodies(&[c], true).unwrap().contains(p)));
    assert_eq!(drawn(&h), [(hinge, true)], "the other joints step back");

    // Ctrl adds a joint, and its part.
    crate::dialogs_assembly::pick_joint(&mut h.app, glue, true);
    h.frames(2);
    assert_eq!(crate::viewport::solid_bodies(&h.app).unwrap().len(), parts.len() + h.app.session.occurrence_bodies(&[c], true).unwrap().len());

    // The way a pick always ended: a click on nothing.
    crate::viewport::select(&mut h.app, None, false, false);
    h.frames(2);
    assert_eq!(crate::viewport::solid_bodies(&h.app), None);
    assert_eq!(drawn(&h), [(hinge, false), (glue, false)]);
}

/// The light is a glance: anything that shows the user has moved on ends it, though the joint
/// stays picked, and it doesn't come back by itself.
#[test]
fn anything_else_puts_out_a_picked_joints_light() {
    let mut h = harness();
    let (hinge, glue) = (id(&h, "Hinge"), id(&h, "Glue"));
    let solid = |h: &Harness| crate::viewport::solid_bodies(&h.app);
    let pick = |h: &mut Harness| {
        h.app.tree.picked_joints.clear();
        h.frames(2);
        crate::dialogs_assembly::pick_joint(&mut h.app, hinge, false);
        h.frames(2);
        assert!(solid(h).is_some());
    };

    // Moving the camera is looking.
    pick(&mut h);
    h.app.cam.pitch += 0.2;
    h.frames(2);
    assert!(solid(&h).is_some());

    // Showing or hiding something.
    h.app.run("browser.visibility", json!({"joints": [glue], "visible": false})).unwrap();
    h.frames(2);
    assert_eq!(solid(&h), None);
    assert_eq!(h.app.tree.picked_joints, [hinge]);
    h.app.run("browser.visibility", json!({"joints": [glue], "visible": true})).unwrap();
    h.frames(2);
    assert_eq!(solid(&h), None, "it doesn't come back with the state it left");

    // Selecting something beside the joint.
    pick(&mut h);
    let body = h.app.session.world_state().bodies.first().unwrap().name.clone();
    crate::viewport::select(&mut h.app, Some(solidcraft_engine::Sel::Body { name: body }), true, false);
    h.frames(2);
    assert_eq!(solid(&h), None);
    h.app.run("select.clear", json!({})).unwrap();

    // Editing the design.
    pick(&mut h);
    h.app.run("joint.drive", json!({"joint": "Hinge", "value": 45})).unwrap();
    h.frames(2);
    assert_eq!(solid(&h), None);

    // A dialog, and it stays out after the dialog closes.
    pick(&mut h);
    h.step(&json!({"click": {"handle": "row:Hinge"}, "double": true})).unwrap();
    h.frames(2);
    assert!(h.app.dialog.is_some());
    assert_eq!(solid(&h), None);
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    assert_eq!(solid(&h), None);
}

/// With its preference off a pick lights nothing and every joint keeps drawing, and turning it off
/// puts out a light that is on.
#[test]
fn highlight_joint_parts_off_keeps_the_view_as_it_was() {
    let mut h = harness();
    let (hinge, glue) = (id(&h, "Hinge"), id(&h, "Glue"));
    h.app.preferences.highlight_joint_parts = false;
    crate::dialogs_assembly::pick_joint(&mut h.app, hinge, false);
    h.frames(2);
    assert_eq!(crate::viewport::solid_bodies(&h.app), None);
    assert_eq!(drawn(&h), [(hinge, true), (glue, false)]);

    h.app.preferences.highlight_joint_parts = true;
    h.app.tree.picked_joints.clear();
    h.frames(2);
    crate::dialogs_assembly::pick_joint(&mut h.app, hinge, false);
    h.frames(2);
    assert!(crate::viewport::solid_bodies(&h.app).is_some());
    h.app.preferences.highlight_joint_parts = false;
    h.frames(2);
    assert_eq!(crate::viewport::solid_bodies(&h.app), None);
}

/// Every placement of a part in a shared subassembly lights up, as a joint names no path.
#[test]
fn a_joint_finds_every_placement_of_its_parts() {
    let (mut h, [_, axis, spindle, ..]) = subassembly();
    let again = h.app.run("occurrence.copy", json!({"occurrence": axis, "translate": [0, 200, 0]})).unwrap()["occurrence"].as_u64().unwrap();
    let mut paths = h.app.session.doc.occurrence_paths(spindle);
    paths.sort();
    let mut expected = vec![vec![axis, spindle], vec![again, spindle]];
    expected.sort();
    assert_eq!(paths, expected);
    assert_eq!(h.app.session.doc.occurrence_paths(axis), [vec![axis]]);
    assert!(h.app.session.doc.occurrence_paths(u64::MAX).is_empty());
}

/// Components A (grounded) and B joined by a revolute "Hinge", and C held by a rigid "Glue".
fn harness() -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.ui.show_browser = true;
    crate::test_support::grounded_boxes(&[("A", 0), ("B", 30), ("C", 60)], |id, p| h.app.run(id, p).unwrap());
    h.app
        .run("joint.create", json!({"type": "revolute", "name": "Hinge", "a": {"occurrence": "A:1", "face": [10, 10, 10]}, "b": {"occurrence": "B:1", "face": [40, 10, 0]}}))
        .unwrap();
    h.app
        .run("joint.create", json!({"type": "rigid", "name": "Glue", "a": {"occurrence": "A:1", "face": [20, 10, 5]}, "b": {"occurrence": "C:1", "face": [60, 10, 5]}}))
        .unwrap();
    h.frames(3);
    h
}

fn id(h: &Harness, name: &str) -> u64 {
    h.app.session.doc.assembly.joints.iter().find(|j| j.name == name).unwrap().id
}

fn occurrence(h: &Harness, name: &str) -> u64 {
    h.app.session.doc.occurrences.iter().find(|o| o.name == name).unwrap().id
}

fn drawn(h: &Harness) -> Vec<(u64, bool)> {
    crate::dialogs_assembly::drawn_joints(&h.app)
}

#[test]
fn eyes_hide_and_show_joints_and_the_folder_keeps_their_state() {
    let mut h = harness();
    let (hinge, glue) = (id(&h, "Hinge"), id(&h, "Glue"));
    assert_eq!(drawn(&h), [(hinge, false), (glue, false)], "every joint draws by default");

    click(&mut h, "eye:Hinge");
    assert!(!h.app.session.joint_visible(hinge) && h.app.session.joint_visible(glue));
    assert_eq!(drawn(&h), [(glue, false)], "a hidden joint isn't drawn");
    assert!(crate::scenario::handle_at("row:Hinge").is_some(), "a hidden joint stays listed");

    // The folder's eye is its own: hiding it hides every glyph without changing a joint's
    // eye, and showing it brings back what was shown before (Hinge stays hidden).
    click(&mut h, "eye:Joints");
    assert!(drawn(&h).is_empty());
    assert!(!h.app.session.joint_visible(hinge) && h.app.session.joint_visible(glue));
    click(&mut h, "eye:Hinge");
    assert!(drawn(&h).is_empty(), "a joint shown in a hidden folder stays undrawn");
    assert!(h.app.session.folder_hidden("c0/joints"), "a joint's eye doesn't change the folder's");
    click(&mut h, "eye:Hinge");
    click(&mut h, "eye:Joints");
    assert_eq!(drawn(&h), [(glue, false)]);
    // Hiding every joint leaves the folder shown.
    click(&mut h, "eye:Glue");
    assert!(!h.app.session.folder_hidden("c0/joints"));
}

#[test]
fn a_hidden_joint_still_drives_and_shows_while_picked() {
    let mut h = harness();
    let hinge = id(&h, "Hinge");
    click(&mut h, "eye:Hinge");
    let b = |h: &Harness| {
        let c = h.app.session.doc.find_component("B").unwrap();
        h.app.session.doc.occurrence_of(c).unwrap().transform
    };
    let before = b(&h);
    h.app.run("joint.drive", json!({"joint": "Hinge", "value": 90})).unwrap();
    h.frames(3);
    assert_ne!(before, b(&h), "the hidden hinge still turns B");
    assert!(!h.app.session.joint_visible(hinge));

    // Picked in the browser, the hidden joint draws highlighted, and only while it is picked.
    click(&mut h, "row:Hinge");
    assert_eq!(h.app.tree.picked_joints, [hinge]);
    assert!(drawn(&h).contains(&(hinge, true)), "{:?}", drawn(&h));
    assert!(!h.app.session.joint_visible(hinge), "picking doesn't show it");
    crate::viewport::select(&mut h.app, None, false, false);
    assert!(h.app.tree.picked_joints.is_empty());
    assert!(!drawn(&h).iter().any(|(j, _)| *j == hinge));
}

#[test]
fn the_menu_and_the_v_key_hide_and_show_picked_joints() {
    let mut h = harness();
    let (hinge, glue) = (id(&h, "Hinge"), id(&h, "Glue"));
    let items = context_menu::items(&h.app, &Target::Joint { id: hinge });
    let item = items.iter().find(|i| i.label == "Show/Hide").unwrap_or_else(|| panic!("no Show/Hide in {items:?}")).clone();
    assert_eq!(item.shortcut, "V");
    context_menu::run_item(&mut h.app, &item, pos2(0.0, 0.0));
    assert!(!h.app.session.joint_visible(hinge) && h.app.session.joint_visible(glue));
    let items = context_menu::items(&h.app, &Target::Joint { id: hinge });
    let item = items.iter().find(|i| i.label == "Show/Hide").unwrap().clone();
    context_menu::run_item(&mut h.app, &item, pos2(0.0, 0.0));
    assert!(h.app.session.joint_visible(hinge));

    // V on the joints picked in the browser.
    click(&mut h, "row:Hinge");
    h.app.tree.picked_joints.push(glue);
    context_menu::toggle_visibility(&mut h.app);
    assert!(!h.app.session.joint_visible(hinge) && !h.app.session.joint_visible(glue));
    context_menu::toggle_visibility(&mut h.app);
    assert!(h.app.session.joint_visible(hinge) && h.app.session.joint_visible(glue));
}

/// What an occurrence's Joints folder lists: (joint, the placement it joins this one to).
fn listed(h: &Harness, occurrence: u64) -> Vec<(String, String)> {
    let doc = &h.app.session.doc;
    let attached = doc.attached_joints().remove(&occurrence).unwrap_or_default();
    crate::dialogs_assembly::placement_joints(doc, &attached).into_iter().map(|j| (j.name, j.other)).collect()
}

/// Frame (grounded) and Axis, a subassembly holding Spindle, Nut and Carriage. Bearing and Rail
/// tie Axis to Frame from inside it; Screw and Mount stay inside. Returns the occurrences
/// Frame, Axis, Spindle, Nut and Carriage.
fn subassembly() -> (Harness, [u64; 5]) {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.ui.show_browser = true;
    for (name, x) in [("Frame", 0), ("Spindle", 30), ("Nut", 60), ("Carriage", 90)] {
        h.app.run("component.activate", json!({"component": "root"})).unwrap();
        h.app.run("component.create", json!({"name": name})).unwrap();
        h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [x, 0, 0]})).unwrap();
    }
    h.app.run("component.activate", json!({"component": "root"})).unwrap();
    h.app.run("occurrence.ground", json!({"occurrence": "Frame:1", "grounded": true})).unwrap();
    h.app.run("component.create", json!({"name": "Axis"})).unwrap();
    h.app.run("component.activate", json!({"component": "root"})).unwrap();
    for part in ["Spindle:1", "Nut:1", "Carriage:1"] {
        h.app.run("occurrence.reparent", json!({"occurrence": part, "parent": "Axis"})).unwrap();
    }
    let [frame, axis, spindle, nut, carriage] = ["Frame:1", "Axis:1", "Spindle:1", "Nut:1", "Carriage:1"].map(|name| occurrence(&h, name));
    for (name, a, b, at) in
        [("Bearing", frame, spindle, 30), ("Screw", spindle, nut, 60), ("Mount", nut, carriage, 90), ("Rail", frame, carriage, 100)]
    {
        h.app.run("joint.as_built", json!({"type": "rigid", "name": name, "a": a, "b": b, "at": {"point": [at, 5, 5]}})).unwrap();
    }
    h.frames(3);
    (h, [frame, axis, spindle, nut, carriage])
}

/// A subassembly lists the joints that cross its border; a part lists all of its own.
#[test]
fn a_subassembly_lists_only_the_joints_that_cross_its_border() {
    let (h, [frame, axis, spindle, nut, carriage]) = subassembly();
    let pair = |n: &str, o: &str| (n.to_string(), o.to_string());
    assert_eq!(listed(&h, axis), [pair("Bearing", "Frame:1"), pair("Rail", "Frame:1")]);
    assert_eq!(listed(&h, spindle), [pair("Bearing", "Frame:1"), pair("Screw", "Nut:1")]);
    assert_eq!(listed(&h, nut), [pair("Screw", "Spindle:1"), pair("Mount", "Carriage:1")]);
    assert_eq!(listed(&h, carriage), [pair("Mount", "Nut:1"), pair("Rail", "Frame:1")]);
    assert_eq!(listed(&h, frame), [pair("Bearing", "Spindle:1"), pair("Rail", "Carriage:1")]);
}

/// Only a joint's own ends are direct, and with detailed folders off a subassembly joined from
/// inside has no folder, as before they existed.
#[test]
fn plain_joint_folders_list_only_a_placements_own_joints() {
    let (mut h, [_, axis, spindle, ..]) = subassembly();
    let attached = h.app.session.doc.attached_joints();
    assert!(attached[&axis].iter().all(|link| !link.direct), "both cross from inside");
    assert!(attached[&spindle].iter().all(|link| link.direct));

    h.app.tree.expanded.insert(format!("o:{axis}"));
    h.app.tree.expanded.insert(format!("o:{axis}/joints"));
    h.frames(3);
    let rows = |label: &str| crate::browser::drawn_rows().into_iter().filter(|r| r.0 == label).count();
    assert_eq!(rows("Bearing"), 2, "the root's folder and Axis's");
    h.app.preferences.detailed_joint_folders = false;
    h.frames(3);
    assert_eq!(rows("Bearing"), 1, "the root's folder alone");
}

/// Show Joints draws one placement's joints, hidden ones too, and fades the bodies outside it. It
/// is a view, no undo step; Escape, a click on nothing or the menu leaves it.
#[test]
fn show_joints_draws_only_that_placements_joints() {
    let (mut h, [_, axis, spindle, nut, _]) = subassembly();
    let [bearing, screw, mount, rail] = ["Bearing", "Screw", "Mount", "Rail"].map(|name| id(&h, name));
    let drawn_ids = |h: &Harness| drawn(h).into_iter().map(|d| d.0).collect::<Vec<_>>();
    assert_eq!(drawn_ids(&h), [bearing, screw, mount, rail]);
    h.app.run("browser.visibility", json!({"joints": [mount], "visible": false})).unwrap();

    let path = vec![axis, nut];
    let item = |h: &Harness| {
        let items = context_menu::items(&h.app, &Target::Occurrence { path: path.clone() });
        items.iter().find(|i| i.id == "ui.showJoints").unwrap_or_else(|| panic!("no Show Joints in {items:?}")).clone()
    };
    let undo = h.app.session.undo.len();
    assert_eq!(item(&h).label, "Show Joints");
    let show = item(&h);
    context_menu::run_item(&mut h.app, &show, pos2(0.0, 0.0));
    h.frames(3);
    assert_eq!(drawn_ids(&h), [screw, mount], "the nut's joints only, the hidden one too");
    assert_eq!(h.app.session.undo.len(), undo, "a view is no undo step");
    assert!(!h.app.session.joint_visible(mount), "showing it here doesn't unhide it");

    let solid = h.app.session.occurrence_bodies(&path, true).unwrap();
    assert!(!solid.is_empty());
    let world = h.app.session.world_state();
    let kept = crate::viewport::solid_bodies(&h.app);
    assert_eq!(kept.as_ref(), Some(&solid));
    for b in &world.bodies {
        assert_eq!(crate::viewport::body_faded(&h.app, b, &kept), !solid.contains(&b.name), "{}", b.name);
    }

    // Picking a joint meanwhile keeps the view, and draws the joint with the others.
    h.app.tree.picked_joints.push(rail);
    assert_eq!(drawn_ids(&h), [screw, mount, rail]);
    h.app.tree.picked_joints.clear();

    // The menu's way back.
    assert_eq!(item(&h).label, "Stop Showing Joints");
    let show = item(&h);
    context_menu::run_item(&mut h.app, &show, pos2(0.0, 0.0));
    assert!(h.app.session.joint_focus().is_none());
    assert_eq!(item(&h).label, "Show Joints");

    // Escape, on another placement.
    h.app.run("joint.focus", json!({"path": [axis, spindle]})).unwrap();
    h.frames(3);
    assert_eq!(drawn_ids(&h), [bearing, screw]);
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(3);
    assert!(h.app.session.joint_focus().is_none(), "Escape leaves it");

    // A click on nothing, as it lets go of a selection.
    h.app.run("joint.focus", json!({"path": path})).unwrap();
    crate::viewport::select(&mut h.app, None, false, false);
    assert!(h.app.session.joint_focus().is_none());
    assert_eq!(drawn_ids(&h), [bearing, screw, rail], "the normal view is back, Mount still hidden");
}

/// A placement no joint attaches has nothing to show, and a placement that goes ends the view.
#[test]
fn show_joints_needs_a_placement_with_joints() {
    let mut h = harness();
    let [b, c] = ["B:1", "C:1"].map(|name| occurrence(&h, name));
    h.app.run("joint.focus", json!({"path": [b]})).unwrap();
    assert_eq!(h.app.session.joint_focus(), Some([b].as_slice()));
    h.app.run("joint.delete", json!({"joint": "Glue"})).unwrap();
    let items = context_menu::items(&h.app, &Target::Occurrence { path: vec![c] });
    assert!(!items.iter().find(|i| i.id == "ui.showJoints").unwrap().enabled);

    h.app.run("component.delete", json!({"component": "B"})).unwrap();
    assert!(h.app.session.joint_focus().is_none());
    assert!(h.app.run("joint.focus", json!({"path": [b]})).is_err(), "a placement that isn't there is refused");
}

/// Each placement has a Joints folder listing the joints that attach it, beside the placement
/// each joins it to (#9130). Its rows are the root folder's joints: a click picks, the eye
/// hides, a double-click edits and the menu acts on the same joint.
#[test]
fn each_placement_lists_the_joints_that_attach_it() {
    let mut h = harness();
    let hinge = id(&h, "Hinge");
    let occ = |h: &Harness, name: &str| h.app.session.doc.occurrences.iter().find(|o| o.name == name).unwrap().id;
    let (a, b, c) = (occ(&h, "A:1"), occ(&h, "B:1"), occ(&h, "C:1"));
    let pair = |n: &str, o: &str| (n.to_string(), o.to_string());
    assert_eq!(listed(&h, a), [pair("Hinge", "B:1"), pair("Glue", "C:1")]);
    assert_eq!(listed(&h, b), [pair("Hinge", "A:1")]);
    assert_eq!(listed(&h, c), [pair("Glue", "A:1")]);

    // The folders start closed: only the root's lists rows.
    let rows = |label: &str| crate::browser::drawn_rows().into_iter().filter(|r| r.0 == label).map(|r| r.1.center().y).collect::<Vec<_>>();
    assert_eq!(rows("Joints").len(), 4, "the root's Joints folder and one per placement");
    assert_eq!(rows("Hinge").len(), 1);

    // Opened, B's folder lists Hinge a second time, below the root's.
    h.app.tree.expanded.insert(format!("o:{b}/joints"));
    h.frames(3);
    let hinges = rows("Hinge");
    assert_eq!(hinges.len(), 2);
    assert_eq!(rows("Glue").len(), 1, "Glue doesn't attach B");
    let at = crate::scenario::handle_at("row:Hinge").unwrap();
    assert_eq!(at.y, hinges[1], "the row clicked below is B's");

    h.step(&json!({"click": {"handle": "row:Hinge"}, "double": true})).unwrap();
    h.frames(3);
    let editing = match h.app.dialog.as_ref().map(|d| &d.kind) {
        Some(crate::dialogs::Kind::Assembly(crate::dialogs_assembly::Asm::Joint(f))) => f.editing,
        _ => None,
    };
    assert_eq!(editing, Some(hinge), "a double click edits the joint");
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(3);
    assert!(h.app.dialog.is_none());

    h.app.tree.picked_joints.clear();
    click(&mut h, "row:Hinge");
    assert_eq!(h.app.tree.picked_joints, [hinge]);
    click(&mut h, "eye:Hinge");
    assert!(!h.app.session.joint_visible(hinge), "B's eye hides the joint itself");
    click(&mut h, "eye:Hinge");
    assert!(h.app.session.joint_visible(hinge));

    // Its menu is the joint's: Delete removes Hinge, and B, which nothing joins now, has no folder.
    h.step(&json!({"click": {"handle": "row:Hinge"}, "button": "right"})).unwrap();
    h.frames(3);
    h.step(&json!({"widget": "Delete"})).unwrap();
    h.frames(3);
    assert!(h.app.session.doc.assembly.joints.iter().all(|j| j.id != hinge));
    assert!(listed(&h, b).is_empty());
    assert_eq!(rows("Joints").len(), 3, "the root's, A's and C's");
}

mod delete {
    //! Joint history and Browser selection must be the target of the Delete key.
    use serde_json::{Value, json};

    use crate::scenario::Harness;

    fn assembly() -> Harness {
        let mut h = Harness::new();
        h.app.home.open = false;
        for (name, x) in [("A", 0), ("B", 30), ("C", 60)] {
            h.app.run("component.activate", json!({"component": "root"})).unwrap();
            h.app.run("component.create", json!({"name": name})).unwrap();
            h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [x, 0, 0]})).unwrap();
        }
        h.app.run("component.activate", json!({"component": "root"})).unwrap();
        h.app.run("occurrence.ground", json!({"occurrence": "A:1", "grounded": true})).unwrap();
        for (name, kind, other, face) in [("Hinge", "revolute", "B:1", [40, 10, 0]), ("Glue", "rigid", "C:1", [70, 10, 0])] {
            h.app
                .run(
                    "joint.create",
                    json!({"type": kind, "name": name,
                "a": {"occurrence": "A:1", "face": [10, 10, 10]}, "b": {"occurrence": other, "face": face}}),
                )
                .unwrap();
        }
        h.frames(3);
        h
    }

    fn joint(h: &Harness, name: &str) -> u64 {
        h.app.session.doc.assembly.joints.iter().find(|j| j.name == name).unwrap().id
    }

    fn click(h: &mut Harness, handle: &str) {
        h.step(&json!({"click": {"handle": handle}})).unwrap();
        h.frames(3);
    }

    fn features(h: &Harness) -> Value {
        serde_json::to_value(&h.app.session.doc.features).unwrap()
    }

    #[test]
    fn a_joint_history_click_replaces_the_stale_feature_before_delete_in_both_layouts() {
        for left in [false, true] {
            let mut h = assembly();
            h.app.ui.show_browser = false;
            h.app.preferences.timeline_left = left;
            h.frames(3);
            let hinge = joint(&h, "Hinge");
            let before = features(&h);
            let feature = h.app.session.doc.features.last().unwrap();
            let (id, name) = (feature.id, feature.name.clone());
            click(&mut h, &format!("timeline:{name}"));
            assert!(h.app.session.selection.iter().any(|s| matches!(s, solidcraft_engine::Sel::Feature { id: f } if *f == id)));
            click(&mut h, "timeline-joint:Hinge");
            assert_eq!(h.app.tree.picked_joints, [hinge], "the clicked joint is the selection; left={left}");
            assert!(h.app.session.selection.is_empty(), "the old feature must not remain a Delete target");
            h.step(&json!({"key": "Delete"})).unwrap();
            assert_eq!(features(&h), before, "Delete must leave every feature intact");
            assert!(h.app.session.doc.assembly.joints.iter().all(|j| j.id != hinge));
            assert_eq!(h.app.session.doc.assembly.joints.len(), 1);
            h.app.run("edit.undo", json!({})).unwrap();
            assert_eq!(h.app.session.doc.assembly.joints.len(), 2);
            assert_eq!(features(&h), before);
            h.app.run("edit.redo", json!({})).unwrap();
            assert_eq!(h.app.session.doc.assembly.joints.len(), 1);
            assert_eq!(features(&h), before);
        }
    }

    #[test]
    fn browser_joint_multiselection_deletes_in_one_undo_step() {
        let mut h = assembly();
        let before = features(&h);
        click(&mut h, "row:Hinge");
        h.step(&json!({"click": {"handle": "row:Glue"}, "ctrl": true})).unwrap();
        h.frames(3);
        assert_eq!(h.app.tree.picked_joints.len(), 2);
        h.step(&json!({"key": "Backspace"})).unwrap();
        assert!(h.app.session.doc.assembly.joints.is_empty());
        assert!(h.app.tree.picked_joints.is_empty());
        assert_eq!(features(&h), before);
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.assembly.joints.len(), 2);
        assert_eq!(features(&h), before);
    }

    #[test]
    fn picking_a_feature_after_a_joint_replaces_the_joint_selection() {
        for left in [false, true] {
            let mut h = assembly();
            h.app.ui.show_browser = false;
            h.app.preferences.timeline_left = left;
            h.frames(3);
            click(&mut h, "timeline-joint:Hinge");
            let name = h.app.session.doc.features.last().unwrap().name.clone();
            click(&mut h, &format!("timeline:{name}"));
            assert!(h.app.tree.picked_joints.is_empty(), "joint selection must follow the feature click; left={left}");
            assert!(!h.app.session.selection.is_empty());
        }
    }

    #[test]
    fn a_joint_menu_delete_uses_its_target_despite_a_stale_feature_selection() {
        let mut h = assembly();
        let before = features(&h);
        let feature = h.app.session.doc.features.last().unwrap().id;
        h.app.run("select.set", json!({"items": [{"type": "feature", "id": feature}]})).unwrap();
        let hinge = joint(&h, "Hinge");
        let items = crate::context_menu::items(&h.app, &crate::context_menu::Target::Joint { id: hinge });
        let item = items.iter().find(|i| i.label == "Delete").unwrap().clone();
        crate::context_menu::run_item(&mut h.app, &item, egui::pos2(0., 0.));
        assert_eq!(h.app.session.doc.assembly.joints.len(), 1);
        assert_eq!(features(&h), before);
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.assembly.joints.len(), 2);
    }

    #[test]
    fn history_joint_ctrl_click_toggles_and_the_context_menu_selects_its_target() {
        for left in [false, true] {
            let mut h = assembly();
            h.app.ui.show_browser = false;
            h.app.preferences.timeline_left = left;
            h.frames(3);
            let (hinge, glue) = (joint(&h, "Hinge"), joint(&h, "Glue"));
            let before = features(&h);
            click(&mut h, "timeline-joint:Hinge");
            h.step(&json!({"click": {"handle": "timeline-joint:Glue"}, "ctrl": true})).unwrap();
            h.frames(3);
            assert_eq!(h.app.tree.picked_joints, [hinge, glue]);
            h.step(&json!({"click": {"handle": "timeline-joint:Hinge"}, "ctrl": true})).unwrap();
            h.frames(3);
            assert_eq!(h.app.tree.picked_joints, [glue]);
            let name = h.app.session.doc.features.last().unwrap().name.clone();
            click(&mut h, &format!("timeline:{name}"));
            h.step(&json!({"click": {"handle": "timeline-joint:Hinge"}, "button": "right"})).unwrap();
            h.frames(3);
            assert_eq!(h.app.tree.picked_joints, [hinge]);
            assert!(h.app.session.selection.is_empty());
            h.step(&json!({"widget": "Delete"})).unwrap();
            assert!(h.app.session.doc.assembly.joints.iter().all(|j| j.id != hinge));
            assert!(h.app.session.doc.assembly.joints.iter().any(|j| j.id == glue));
            assert_eq!(features(&h), before);
        }
    }

    #[test]
    fn joint_delete_does_not_run_while_a_dialog_or_history_command_is_active() {
        let mut h = assembly();
        click(&mut h, "row:Hinge");
        let before = features(&h);
        h.step(&json!({"start": "solid.box"})).unwrap();
        let hinge = joint(&h, "Hinge");
        h.app.tree.picked_joints.clear();
        crate::dialogs_assembly::pick_joint(&mut h.app, hinge, false);
        assert!(h.app.dialog.is_some());
        assert_eq!(h.app.tree.picked_joints, [hinge], "the dialog guard must protect an actual joint selection");
        click(&mut h, "dialog:ui.length");
        h.step(&json!({"key": "Delete"})).unwrap();
        assert_eq!(h.app.session.doc.assembly.joints.len(), 2, "Delete belongs to the focused field");
        assert_eq!(h.app.tree.picked_joints, [hinge]);
        crate::delete::delete_selection(&mut h.app);
        assert_eq!(h.app.session.doc.assembly.joints.len(), 2, "a dialog must not delete a joint behind it");
        assert!(h.app.status.as_ref().is_some_and(|s| s.2 && s.0.contains("dialog")), "the refusal says why");
        h.step(&json!({"key": "Escape"})).unwrap();
        let hinge = joint(&h, "Hinge");
        h.app.tree.picked_joints.clear();
        crate::dialogs_assembly::pick_joint(&mut h.app, hinge, false);
        assert_eq!(h.app.tree.picked_joints, [hinge]);
        h.app.apply_off_thread = Some(std::time::Duration::ZERO);
        h.app.run_heavy("timeline.roll_to", json!({"position": 1})).unwrap();
        assert!(h.app.computing_since().is_some());
        crate::delete::delete_selection(&mut h.app);
        assert_eq!(h.app.session.doc.assembly.joints.len(), 2, "Delete must not race the pending history command");
        assert_eq!(features(&h), before);
    }

    #[test]
    fn delete_while_editing_that_joint_closes_edit_joint_and_deletes_it() {
        let mut h = assembly();
        let before = features(&h);
        let hinge = joint(&h, "Hinge");
        h.step(&json!({"click": {"handle": "row:Hinge"}, "double": true})).unwrap();
        h.frames(3);
        let editing = |h: &Harness| match h.app.dialog.as_ref().map(|d| &d.kind) {
            Some(crate::dialogs::Kind::Assembly(crate::dialogs_assembly::Asm::Joint(f))) => f.editing,
            _ => None,
        };
        assert_eq!(editing(&h), Some(hinge), "the double click opens Edit Joint");
        h.step(&json!({"key": "Delete"})).unwrap();
        h.frames(3);
        assert!(h.app.dialog.is_none(), "Delete closes the Edit Joint it deletes the joint of");
        assert!(h.app.session.doc.assembly.joints.iter().all(|j| j.id != hinge));
        assert_eq!(h.app.session.doc.assembly.joints.len(), 1);
        assert_eq!(features(&h), before);
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.assembly.joints.len(), 2);
        assert_eq!(features(&h), before);
        // The Browser menu's Delete on the same joint goes the same way.
        h.app.dialog = crate::dialogs_assembly::edit_joint(&h.app, hinge);
        let items = crate::context_menu::items(&h.app, &crate::context_menu::Target::Joint { id: hinge });
        let item = items.iter().find(|i| i.label == "Delete").unwrap().clone();
        crate::context_menu::run_item(&mut h.app, &item, egui::pos2(0., 0.));
        assert!(h.app.dialog.is_none());
        assert_eq!(h.app.session.doc.assembly.joints.len(), 1);
    }

    #[test]
    fn a_ctrl_joint_pick_does_not_leave_a_folder_as_the_delete_target() {
        let mut h = assembly();
        h.app.ui.show_browser = false;
        h.app.preferences.timeline_left = true;
        let feature = h.app.session.doc.features[0].id;
        let group = h.app.run("timeline.group", json!({"name": "History", "features": [feature]})).unwrap()["group"].as_u64().unwrap();
        h.app.tree.picked_timeline_groups.push(group);
        h.frames(3);
        let before = features(&h);
        let hinge = joint(&h, "Hinge");
        h.step(&json!({"click": {"handle": "timeline-joint:Hinge"}, "ctrl": true})).unwrap();
        h.frames(3);
        assert_eq!(h.app.tree.picked_joints, [hinge]);
        assert!(h.app.tree.picked_timeline_groups.is_empty());
        h.step(&json!({"key": "Delete"})).unwrap();
        assert!(h.app.session.doc.assembly.joints.iter().all(|joint| joint.id != hinge));
        assert!(h.app.session.doc.timeline_groups.iter().any(|folder| folder.id == group), "Delete must preserve the old folder");
        assert_eq!(features(&h), before);
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.assembly.joints.len(), 2);
    }
}
