use std::sync::Arc;

use serde_json::json;
use solidcraft_engine::Sel;

use crate::test_support::{click, component_box as fixture};

#[test]
fn navigating_and_selecting_reuse_placements_but_rows_read_live_view_state() {
    let (mut h, component, occurrence) = fixture();
    let first = super::model_placement(&mut h.app, &[occurrence]).unwrap();
    h.app.cam.yaw += 0.25;
    h.app.run("select.set", json!({"items":[Sel::Body{name:"Body1".into()}]})).unwrap();
    h.app.session.visibility.hidden_bodies.push("Body1".into());
    h.app.ui.locked_bodies.push("Body1".into());
    let after = super::model_placement(&mut h.app, &[occurrence]).unwrap();
    assert!(Arc::ptr_eq(&first, &after), "view-only changes resolved the same placement again");
    let rows = super::entries(&h.app, component, "bodies");
    assert_eq!(rows.len(), 1);
    assert!(rows[0].selected && rows[0].locked && !rows[0].visible);
    h.frames(2);
    assert!(Arc::ptr_eq(&after, &super::model_placement(&mut h.app, &[occurrence]).unwrap()));
}

#[test]
fn pending_and_reverted_moves_replace_cached_transforms() {
    let (mut h, _, occurrence) = fixture();
    let first = super::model_placement(&mut h.app, &[occurrence]).unwrap();
    h.app.run("occurrence.move", json!({"occurrence":occurrence,"translate":[20,0,0],"capture":false})).unwrap();
    let moved = super::model_placement(&mut h.app, &[occurrence]).unwrap();
    assert!(!Arc::ptr_eq(&first, &moved));
    assert!((solidcraft_engine::doc::apply_point(&moved.world, solidcraft_engine::geom::Vec3::ZERO).x - 20.0).abs() < 1e-9);
    h.app.run("component.revert_position", json!({"occurrences":[occurrence]})).unwrap();
    let reverted = super::model_placement(&mut h.app, &[occurrence]).unwrap();
    assert!(!Arc::ptr_eq(&moved, &reverted));
    assert_eq!(first.world, reverted.world);
}

#[test]
fn feature_indices_follow_component_edits_and_undo() {
    let (mut h, component, _) = fixture();
    super::prepare_model(&mut h.app);
    assert!(super::model_feature_indices(&h.app, 0, true).is_empty());
    h.app.run("sketch.create", json!({"plane":"XY", "name":"Root sketch"})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    super::prepare_model(&mut h.app);
    assert_eq!(super::entries(&h.app, 0, "sketches")[0].label, "Root sketch");
    assert!(super::entries(&h.app, component, "sketches").is_empty());
    h.app.run("component.activate", json!({"component":component})).unwrap();
    h.app.run("sketch.create", json!({"plane":"YZ", "name":"Part sketch"})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    super::prepare_model(&mut h.app);
    assert_eq!(super::entries(&h.app, component, "sketches")[0].label, "Part sketch");
    assert_eq!(super::entries(&h.app, 0, "sketches").len(), 1);
    h.app.run("edit.undo", json!({})).unwrap();
    super::prepare_model(&mut h.app);
    assert!(super::entries(&h.app, component, "sketches").is_empty());
    assert_eq!(super::entries(&h.app, 0, "sketches").len(), 1);
}

#[test]
fn body_index_and_placements_follow_geometry_undo_and_document_identity() {
    let (mut h, component, occurrence) = fixture();
    let first = super::model_placement(&mut h.app, &[occurrence]).unwrap();
    let copy = h.app.run("occurrence.copy", json!({"occurrence":occurrence,"translate":[50,0,0]})).unwrap()["occurrence"].as_u64().unwrap();
    let second = super::model_placement(&mut h.app, &[copy]).unwrap();
    assert_ne!(first.path, second.path);
    assert_ne!(first.bodies[0].world, second.bodies[0].world);
    assert_eq!(super::model_children(&h.app, 0), [occurrence, copy]);
    h.app.run("component.activate", json!({"component":component})).unwrap();
    h.app.run("solid.box", json!({"length":2,"width":2,"height":2,"corner":[20,0,0]})).unwrap();
    let changed = super::model_placement(&mut h.app, &[occurrence]).unwrap();
    assert_eq!(changed.bodies.len(), 2);
    assert_eq!(super::entries(&h.app, component, "bodies").len(), 2);
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(super::model_placement(&mut h.app, &[occurrence]).unwrap().bodies.len(), 1);
    assert_eq!(super::entries(&h.app, component, "bodies").len(), 1);
    // A separate session can have the same revision and the same occurrence ids.
    h.app.session = solidcraft_engine::Session::default();
    assert!(super::model_placement(&mut h.app, &[occurrence]).is_none());
    assert!(super::model_children(&h.app, 0).is_empty());
    assert!(super::entries(&h.app, component, "bodies").is_empty());
}

/// #3005: a face picked in the viewport, on the second placement of a part whose rows are
/// closed: that placement opens down to its body's row (which the browser lights for the pick,
/// #3009); the first placement stays closed.
#[test]
fn a_face_picked_in_the_viewport_opens_the_browser_to_its_placements_body() {
    let (mut h, _, occurrence) = fixture();
    let copy = h.app.run("occurrence.copy", json!({"occurrence":occurrence,"translate":[50,0,0]})).unwrap()["occurrence"].as_u64().unwrap();
    let first = super::model_placement(&mut h.app, &[occurrence]).unwrap().bodies[0].world.clone();
    let second = super::model_placement(&mut h.app, &[copy]).unwrap().bodies[0].world.clone();
    h.app.ui.show_browser = true;
    let (one, two) = (super::occurrence_key(&[occurrence]), super::occurrence_key(&[copy]));
    h.app.tree.collapsed.extend([one.clone(), two.clone(), format!("{two}/bodies")]);
    h.frames(2);
    assert!(!h.app.tree.order.contains(&format!("b:{second}")), "the body's row is closed away");
    let face = Sel::Face { body: second.clone(), index: 0, point: solidcraft_engine::geom::Vec3::new(55.0, 5.0, 10.0) };
    crate::viewport::select(&mut h.app, Some(face), false, false);
    assert_eq!(h.app.tree.reveal.as_deref(), Some(second.as_str()));
    h.frames(2);
    assert!(h.app.tree.reveal.is_none(), "revealed");
    assert!(!h.app.tree.collapsed.contains(&two) && !h.app.tree.collapsed.contains(&format!("{two}/bodies")));
    assert!(h.app.tree.collapsed.contains(&one), "the other placement stays as it was");
    assert!(h.app.tree.order.contains(&format!("b:{second}")), "the row is drawn");
    assert!(!h.app.tree.order.contains(&format!("b:{first}")), "the other placement's row stays closed away");
}

fn body_names(app: &crate::SolveApp) -> Vec<String> {
    app.dialog
        .as_ref()
        .and_then(|d| d.inputs.first())
        .map(|i| {
            i.items
                .iter()
                .filter_map(|s| match s {
                    Sel::Body { name } => Some(name.clone()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn two_bodies(h: &mut crate::scenario::Harness) {
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Left"})).unwrap();
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [30, 0, 0], "body_name": "Right"})).unwrap();
}

fn open_move(h: &mut crate::scenario::Harness) {
    h.app.run("select.clear", json!({})).unwrap();
    h.app.tree.picked_components.clear();
    h.app.start("solid.move");
    assert!(h.app.dialog.is_some(), "Move/Copy opens");
    assert!(body_names(&h.app).is_empty(), "nothing was selected, so the body list starts empty");
}

/// #4771: with Move/Copy open, each body clicked in the browser joins the body list.
/// Ctrl-click toggles one body and leaves the others.
#[test]
fn browser_move_copy_takes_each_body_clicked_in_the_tree() {
    let mut h = crate::scenario::Harness::new();
    two_bodies(&mut h);
    open_move(&mut h);
    h.frames(2);
    click(&mut h, "row:Left");
    assert_eq!(body_names(&h.app), vec!["Left".to_string()]);
    h.step(&json!({"click": {"handle": "row:Right"}, "ctrl": true})).unwrap();
    h.frames(2);
    let names = body_names(&h.app);
    assert!(names.contains(&"Left".into()) && names.contains(&"Right".into()), "{names:?}");
    h.step(&json!({"click": {"handle": "row:Left"}, "ctrl": true})).unwrap();
    h.frames(2);
    assert_eq!(body_names(&h.app), vec!["Right".to_string()]);
    assert!(h.app.session.selection.is_empty(), "the click fills the dialog and does not replace the selection");
}

/// #4771: one click on a body group, or on the Bodies folder, selects that whole set.
/// The fold arrow still collapses the row.
#[test]
fn browser_move_copy_takes_a_group_or_the_bodies_folder_in_one_click() {
    let mut h = crate::scenario::Harness::new();
    two_bodies(&mut h);
    let group = h.app.run("browser.group", json!({"component": 0, "folder": "bodies", "name": "Pair", "items": ["Left", "Right"]})).unwrap();
    let group = group["group"].as_u64().unwrap();
    open_move(&mut h);
    h.frames(2);
    click(&mut h, "row:Pair (2)");
    let names = body_names(&h.app);
    assert!(names.contains(&"Left".into()) && names.contains(&"Right".into()), "{names:?}");
    assert!(!h.app.tree.collapsed.contains(&format!("c0/g{group}")), "the group label selects; it does not fold");
    click(&mut h, "row:Pair (2)");
    assert!(body_names(&h.app).is_empty(), "clicking the group again clears those bodies");
    click(&mut h, "fold:Pair (2)");
    assert!(h.app.tree.collapsed.contains(&format!("c0/g{group}")), "the arrow still folds the group");
    assert!(body_names(&h.app).is_empty());

    h.app.tree.collapsed.clear();
    h.frames(2);
    click(&mut h, "row:Bodies");
    let names = body_names(&h.app);
    assert!(names.contains(&"Left".into()) && names.contains(&"Right".into()), "{names:?}");
    assert!(!h.app.tree.collapsed.contains("c0/bodies"), "the Bodies label selects; it does not fold");
    click(&mut h, "fold:Bodies");
    assert!(h.app.tree.collapsed.contains("c0/bodies"));
}

/// #4771: one click on a component adds every body it owns. The design row does the same.
#[test]
fn browser_move_copy_takes_a_component_clicked_in_the_tree() {
    let (mut h, _, occurrence) = fixture();
    let owned = h.app.session.occurrence_bodies(&[occurrence], true).unwrap();
    assert!(!owned.is_empty(), "the component has a body to move");
    open_move(&mut h);
    h.frames(2);
    click(&mut h, &format!("row:occurrence:{occurrence}"));
    assert_eq!(body_names(&h.app), owned, "the component's bodies are the Move/Copy selection");
    assert!(h.app.tree.occurrence_move.is_none(), "the open body dialog stays; this click does not switch to component Move");

    h.app.dialog = None;
    h.app.run("select.clear", json!({})).unwrap();
    h.frames(2);
    click(&mut h, "row:Untitled");
    assert_eq!(h.app.tree.picked_components, vec![0]);
    h.app.start("solid.move");
    let names = body_names(&h.app);
    assert!(names.iter().any(|n| owned.contains(n)), "Move/Copy starts from the component picked in the tree: {names:?}");
}

/// M and the Move/Copy command both start from a body that is already selected, and from a
/// component picked in the tree. One selected component placement still moves as a component.
#[test]
fn browser_move_copy_starts_from_the_selected_body_or_component() {
    let mut h = crate::scenario::Harness::new();
    two_bodies(&mut h);
    let both = json!({"items": [Sel::Body { name: "Left".into() }, Sel::Body { name: "Right".into() }]});
    h.app.run("select.set", both.clone()).unwrap();
    crate::keymap::press(&mut h.app, "M");
    let names = body_names(&h.app);
    assert!(names.contains(&"Left".into()) && names.contains(&"Right".into()), "M uses the selected bodies: {names:?}");
    // Starting the command takes the selection into the dialog, so the toolbar is tried
    // with the bodies selected again.
    h.app.dialog = None;
    h.app.run("select.set", both).unwrap();
    h.app.start("solid.move");
    let names = body_names(&h.app);
    assert!(names.contains(&"Left".into()) && names.contains(&"Right".into()), "the Move/Copy button uses the same selection: {names:?}");

    h.app.dialog = None;
    h.frames(2);
    click(&mut h, "row:Untitled");
    crate::keymap::press(&mut h.app, "M");
    let names = body_names(&h.app);
    assert!(names.contains(&"Left".into()) && names.contains(&"Right".into()), "M uses the component picked in the tree: {names:?}");

    let (mut placed, _, occurrence) = fixture();
    placed.app.run("select.set", json!({"items": [{"type": "occurrence", "path": [occurrence]}]})).unwrap();
    crate::keymap::press(&mut placed.app, "M");
    assert!(placed.app.dialog.is_none(), "one selected component still moves as a component");
    assert!(placed.app.tree.occurrence_move.is_some());
}

/// Shift-click adds every body between the two rows.
#[test]
fn browser_shift_click_adds_the_bodies_in_between_to_move_copy() {
    let mut h = crate::scenario::Harness::new();
    two_bodies(&mut h);
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [60, 0, 0], "body_name": "Far"})).unwrap();
    open_move(&mut h);
    h.frames(2);
    h.step(&json!({"click": {"handle": "row:Left"}})).unwrap();
    h.frames(1);
    h.step(&json!({"click": {"handle": "row:Far"}, "shift": true})).unwrap();
    h.frames(1);
    let names = body_names(&h.app);
    assert!(names.contains(&"Left".into()) && names.contains(&"Right".into()) && names.contains(&"Far".into()), "{names:?}");
}

/// #4452: Combine's active input takes the body, then the next one does. A browser click is the
/// same pick as a click in the view, so the first body is the target and the next is a tool.
#[test]
fn combine_takes_bodies_clicked_in_the_browser() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"corner": [0, 0, 0], "length": 20, "width": 20, "height": 20, "body_name": "Block"})).unwrap();
    h.app.run("solid.box", json!({"corner": [10, 10, 10], "length": 20, "width": 20, "height": 20, "body_name": "Cutter"})).unwrap();
    h.app.session.selection.clear();
    h.app.start("solid.combine");
    h.frames(2);
    h.step(&json!({"click": {"browser": "Block"}})).unwrap();
    h.frames(1);
    h.step(&json!({"click": {"browser": "Cutter"}})).unwrap();
    h.frames(1);
    let d = h.app.dialog.as_ref().expect("Combine stays open");
    assert_eq!(d.inputs[0].items, vec![Sel::Body { name: "Block".into() }], "target: {:?}", d.inputs);
    assert_eq!(d.inputs[1].items, vec![Sel::Body { name: "Cutter".into() }], "tools: {:?}", d.inputs);
    assert!(h.app.session.selection.is_empty(), "the browser click does not replace the selection");
}

/// With Move/Copy closed, a body click still selects that body, and a group or folder click
/// still folds.
#[test]
fn browser_rows_still_fold_or_select_when_move_copy_is_not_open() {
    let mut h = crate::scenario::Harness::new();
    two_bodies(&mut h);
    let group = h.app.run("browser.group", json!({"component": 0, "folder": "bodies", "name": "Pair", "items": ["Left", "Right"]})).unwrap();
    let group = group["group"].as_u64().unwrap();
    h.app.run("select.clear", json!({})).unwrap();
    h.frames(2);
    click(&mut h, "row:Left");
    assert!(h.app.dialog.is_none());
    assert_eq!(h.app.session.selection, vec![Sel::Body { name: "Left".into() }]);
    click(&mut h, "row:Pair (2)");
    assert!(h.app.tree.collapsed.contains(&format!("c0/g{group}")), "a group click folds when no command wants bodies");
    click(&mut h, "row:Bodies");
    assert!(h.app.tree.collapsed.contains("c0/bodies"));
}
