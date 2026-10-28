//! Joints in the browser (#1160): an eye per joint and on the Joints folder hides and shows
//! joint glyphs; Show/Hide in a joint's menu and the V key do the same. Hiding only hides the
//! drawing: the joint still constrains and drives. A hidden joint picked in the browser draws
//! highlighted until the pick goes away.

use egui::pos2;
use serde_json::json;

use crate::context_menu::{self, Target};
use crate::scenario::Harness;
use crate::test_support::click;

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
    // The Joints folder starts closed.
    h.app.tree.expanded.insert("c0/joints".into());
    h.frames(3);
    h
}

fn id(h: &Harness, name: &str) -> u64 {
    h.app.session.doc.assembly.joints.iter().find(|j| j.name == name).unwrap().id
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
        h.app.tree.expanded.insert("c0/joints".into());
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
