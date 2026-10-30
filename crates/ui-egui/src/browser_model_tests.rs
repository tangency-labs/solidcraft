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
    h.app.session.visibility.locked_bodies.push("Body1".into());
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

/// #8768: by default the Browser's rows start open, as before: a placement lists its body at
/// once. Browser rows start closed (Preferences › Interface) closes the rows nobody opened, and
/// turning it off again opens them.
#[test]
fn browser_rows_start_open_by_default_and_close_only_with_the_preference() {
    let (mut h, _, occurrence) = fixture();
    assert!(!h.app.preferences.browser_rows_start_closed, "off by default");
    let body = super::model_placement(&mut h.app, &[occurrence]).unwrap().bodies[0].world.clone();
    let row = format!("b:{body}");
    h.app.ui.show_browser = true;
    h.frames(2);
    assert!(h.app.tree.order.contains(&row), "the placement starts open and lists its body");
    assert_eq!(h.call("ui.prefs", json!({"browser_rows_start_closed": true}))["ok"], true);
    h.frames(2);
    assert!(!h.app.tree.order.contains(&row), "with the preference on, the placement starts closed");
    assert_eq!(h.call("ui.prefs", json!({"browser_rows_start_closed": false}))["ok"], true);
    h.frames(2);
    assert!(h.app.tree.order.contains(&row), "off again, it is open again");
}

/// #8768: while the Browser's rows start closed (Preferences › Interface), a pick in the 3D view
/// opens, closes and scrolls nothing, and neither the pick nor a body under the pointer has its
/// placement looked up to mark the rows above it: on a large assembly that lookup names every
/// placement again after each edit. With the preference off again, the rows above the hovered
/// body are marked (#3009); a picked face marks its feature's row, not its body's (#9848).
#[test]
fn a_pick_in_the_viewport_leaves_the_browser_alone_while_its_rows_start_closed() {
    let (mut h, _, occurrence) = fixture();
    assert_eq!(h.call("ui.prefs", json!({"browser_rows_start_closed": true}))["ok"], true);
    let copy = h.app.run("occurrence.copy", json!({"occurrence":occurrence,"translate":[50,0,0]})).unwrap()["occurrence"].as_u64().unwrap();
    let second = super::model_placement(&mut h.app, &[copy]).unwrap().bodies[0].world.clone();
    h.app.ui.show_browser = true;
    // Both placements open by hand, their Bodies folders closed as they start.
    h.app.tree.expanded.extend([super::occurrence_key(&[occurrence]), super::occurrence_key(&[copy])]);
    h.frames(2);
    let (collapsed, expanded, order) = (h.app.tree.collapsed.clone(), h.app.tree.expanded.clone(), h.app.tree.order.clone());
    let point = solidcraft_engine::geom::Vec3::new(55.0, 5.0, 10.0);
    crate::viewport::select(&mut h.app, Some(Sel::Face { body: second.clone(), index: 0, point }), false, false);
    assert!(h.app.tree.reveal.is_none(), "nothing to scroll to");
    h.frames(3);
    assert!(!h.app.session.selection.is_empty(), "the face is picked");
    assert_eq!((&h.app.tree.collapsed, &h.app.tree.expanded), (&collapsed, &expanded), "every row stays open or closed as it was");
    assert_eq!(h.app.tree.order, order, "the same rows are drawn");
    h.app.viewport.hover = Some(crate::viewport::Hit::Face { body: second.clone(), index: 0, point });
    let lit = super::Lit::of(&mut h.app);
    assert!(lit.paths.is_empty(), "no row above the picked or hovered body is marked: {:?}", lit.paths);
    assert!(!h.app.tree.model.as_ref().unwrap().body_paths_walked(), "the placements were not walked for the pick or the pointer");
    assert_eq!(h.call("ui.prefs", json!({"browser_rows_start_closed": false}))["ok"], true);
    // The call's frames took the hover from the pointer, which is elsewhere.
    h.app.viewport.hover = Some(crate::viewport::Hit::Face { body: second.clone(), index: 0, point });
    let lit = super::Lit::of(&mut h.app);
    assert_eq!(lit.paths, [vec![copy]], "the placement above the hovered body; the picked face marks its feature");
}

/// #8768: while the Browser's rows start closed, a new design's Bodies and Sketches folders list
/// nothing until their arrow opens them, and Find in Browser opens the Bodies folder that holds
/// the body it finds.
#[test]
fn with_rows_starting_closed_a_designs_folders_open_with_their_arrow_or_find_in_browser() {
    let mut h = crate::scenario::Harness::new();
    assert_eq!(h.call("ui.prefs", json!({"browser_rows_start_closed": true}))["ok"], true);
    h.app.run("sketch.create", json!({"plane":"XY","name":"Sketch1"})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Block"})).unwrap();
    h.app.ui.show_browser = true;
    h.frames(2);
    let drawn = |label: &str| super::drawn_rows().iter().any(|row| row.0 == label);
    let body_row = "b:Block".to_string();
    assert!(drawn("Bodies") && drawn("Sketches"), "the folders are listed");
    assert!(!super::is_open(&h.app, "c0/bodies", false) && !super::is_open(&h.app, "c0/sketches", false));
    assert!(!h.app.tree.order.contains(&body_row), "the body's row is closed away");
    assert!(!drawn("Sketch1"), "so is the sketch's");
    click(&mut h, "fold:Sketches");
    h.frames(1);
    assert!(drawn("Sketch1"), "the arrow opens the Sketches folder");
    assert!(!h.app.tree.order.contains(&body_row), "the Bodies folder stays closed");

    crate::context_menu::run_item(
        &mut h.app,
        &crate::context_menu::Item::action("ui.findBrowser", "", "").with(json!({"body":"Block"})),
        egui::Pos2::ZERO,
    );
    h.frames(2);
    assert!(super::is_open(&h.app, "c0/bodies", false), "Find in Browser opens the Bodies folder");
    assert!(h.app.tree.order.contains(&body_row) && h.app.tree.reveal.is_none(), "and scrolls to the body's row");
}

/// Hovering a placement row lights its whole subtree: the subtree is walked once per model, not
/// on every frame the pointer stays on the row (#3892: a nested STEP assembly cost a frame per
/// thousand placements while the pointer was over the tree, even with the rows collapsed).
#[test]
fn hovering_a_collapsed_nested_placement_walks_its_subtree_once() {
    let mut h = crate::scenario::Harness::new();
    let sub = h.app.run("component.create", json!({"name":"Sub"})).unwrap();
    let (sub_component, sub_occurrence) = (sub["component"].as_u64().unwrap(), sub["occurrence"].as_u64().unwrap());
    let leaf = h.app.run("component.create", json!({"name":"Leaf"})).unwrap()["component"].as_u64().unwrap();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
    h.app.run("component.activate", json!({"component":sub_component})).unwrap();
    for i in 1..6 {
        h.app.run("occurrence.copy", json!({"component":leaf,"translate":[i as f64 * 20.0, 0, 0]})).unwrap();
    }
    h.app.run("component.activate", json!({"component":0})).unwrap();
    h.app.run("occurrence.copy", json!({"component":sub_component,"translate":[0, 40, 0]})).unwrap();
    let path = vec![sub_occurrence];
    h.app.tree.collapsed.insert(super::occurrence_key(&path));
    h.frames(3);
    let at = crate::scenario::handle_at(&format!("row:occurrence:{sub_occurrence}")).unwrap();
    h.call("ui.move", json!({"x":at.x,"y":at.y}));
    let walked = h.app.session.occurrence_bodies_shared(&path, true).unwrap();
    assert_eq!(walked.len(), 6, "{walked:?}");
    assert_eq!(h.app.viewport.hover_bodies, walked.to_vec(), "the row lights its placement's bodies and every one below it");
    h.frames(10);
    assert_eq!(h.app.viewport.hover_bodies, walked.to_vec());
    assert!(Arc::ptr_eq(&walked, &h.app.session.occurrence_bodies_shared(&path, true).unwrap()), "frames on the same row walk nothing again");
    // An edit is a new model: the row lights the subtree that is there now.
    h.app.run("component.activate", json!({"component":sub_component})).unwrap();
    h.app.run("occurrence.copy", json!({"component":leaf,"translate":[0, 80, 0]})).unwrap();
    h.app.run("component.activate", json!({"component":0})).unwrap();
    h.call("ui.move", json!({"x":at.x,"y":at.y + 1.0}));
    assert_eq!(h.app.viewport.hover_bodies.len(), 7);
}

/// An open curve turned round is a surface body and the Bodies folder shows it as one; a closed
/// profile turned all the way round, and a box, are solids.
#[test]
fn bodies_folder_tells_surfaces_from_solids() {
    let mut h = crate::scenario::Harness::new();
    let mut run = |id: &str, p: serde_json::Value| {
        h.app.run(id, p).unwrap();
    };
    run("sketch.create", json!({"plane":"XY", "name":"S"}));
    run("sketch.line", json!({"points":[[3, 0], [3, 4]], "ids":["side"]}));
    run("sketch.rectangle.two_point", json!({"p0":[5, 0], "p1":[7, 3]}));
    run("sketch.finish", json!({}));
    run("surface.revolve", json!({"sketch":"S", "curves":["side"], "axis":"y", "body_name":"Tube"}));
    run("surface.revolve", json!({"sketch":"S", "profiles":[0], "axis":"y", "body_name":"Ring"}));
    run("solid.box", json!({"length":2, "width":2, "height":2, "corner":[20, 0, 0]}));
    super::prepare_model(&mut h.app);
    let icons: Vec<(String, &str)> = super::entries(&h.app, 0, "bodies").into_iter().map(|e| (e.key, e.icon)).collect();
    assert_eq!(icons.len(), 3, "{icons:?}");
    assert_eq!(icons[..2], [("Tube".to_string(), "surface"), ("Ring".to_string(), "body")]);
    assert_eq!(icons[2].1, "body", "the box is a solid");
}

#[test]
fn construction_rows_include_points_and_axes() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("construct.point.vertex", json!({"point":{"point":[20,15,10]},"name":"Point1"})).unwrap();
    h.app.run("construct.axis.two_points", json!({"a":{"point":[0,0,0]},"b":{"point":[0,0,30]},"name":"Axis1"})).unwrap();
    for cached in [false, true] {
        if cached {
            super::prepare_model(&mut h.app);
        }
        let rows = super::entries(&h.app, 0, "construction");
        assert_eq!(rows.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(), ["Point1", "Axis1"]);
        for row in &rows {
            assert!(row.visible && row.sel.is_some() && row.target.is_some());
        }
    }
    let rows = super::entries(&h.app, 0, "construction");
    for row in &rows {
        super::pick_from_browser(&mut h.app, row.sel.clone().unwrap());
        assert!(super::entries(&h.app, 0, "construction").iter().find(|r| r.label == row.label).unwrap().selected);
        let items = crate::context_menu::items(&h.app, row.target.as_ref().unwrap());
        for id in ["ui.rename", "ui.delete", "ui.editFeature", "ui.originPart"] {
            assert!(items.iter().any(|i| i.id == id), "{} lacks {id}", row.label);
        }
        assert!(!items.iter().any(|i| i.id == "sketch.create"));
    }
    h.app.session.visibility.hidden_origin.push("Point1".into());
    assert!(!super::entries(&h.app, 0, "construction")[0].visible);
    h.app.run("selection.delete", json!({"items": rows.iter().map(|r| r.sel.clone().unwrap()).collect::<Vec<_>>()})).unwrap();
    super::prepare_model(&mut h.app);
    assert!(super::entries(&h.app, 0, "construction").is_empty());
}

#[test]
fn render_browser_swatch_resolves_face_body_component_then_physical_material() {
    let (mut h, component, _) = fixture();
    crate::render_workspace::enter(&mut h.app, true).unwrap();
    let swatch = |h: &crate::scenario::Harness| crate::render_workspace::scene::decoration(&h.app, &["Body1".into()]);
    h.app.run("material.assign", json!({"bodies":["Body1"],"material":"Brass"})).unwrap();
    assert_eq!(swatch(&h).name, "Brass");
    h.app.run("appearance.assign", json!({"components":[component.to_string()],"appearance":"ABS - Red"})).unwrap();
    assert_eq!(swatch(&h).name, "ABS - Red");
    h.app.run("appearance.assign", json!({"bodies":["Body1"],"appearance":"ABS - Blue"})).unwrap();
    assert_eq!(swatch(&h).name, "ABS - Blue");
    let point = solidcraft_engine::geom::Vec3::new(0.0, 5.0, 5.0);
    h.app.run("appearance.assign", json!({"body":"Body1","faces":[point],"appearance":"ABS - Red"})).unwrap();
    let state = h.app.session.world_state();
    let body = state.body("Body1").unwrap();
    let face = solidcraft_engine::doc::appearance::face_index_at(body, point).unwrap();
    h.app.run("select.set", json!({"items":[Sel::Face{body:"Body1".into(),index:face,point}]})).unwrap();
    let resolved = swatch(&h);
    assert_eq!(resolved.name, "ABS - Red");
    assert_eq!(resolved.swatch, Some(egui::Color32::from_rgb(196, 40, 40)));
    assert_eq!(resolved.count, "1 face");
    h.app.run("select.clear", json!({})).unwrap();
    assert_eq!(swatch(&h).name, "ABS - Blue");
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
    crate::keymap::press(&mut h.app, "M", false);
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
    crate::keymap::press(&mut h.app, "M", false);
    let names = body_names(&h.app);
    assert!(names.contains(&"Left".into()) && names.contains(&"Right".into()), "M uses the component picked in the tree: {names:?}");

    let (mut placed, _, occurrence) = fixture();
    placed.app.run("select.set", json!({"items": [{"type": "occurrence", "path": [occurrence]}]})).unwrap();
    crate::keymap::press(&mut placed.app, "M", false);
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

#[test]
fn feature_editor_browser_rows_follow_the_marker_and_return_after_cancel() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY","name":"Base"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[10,10]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("solid.extrude", json!({"sketch":"Base","distance":10,"name":"Depth"})).unwrap();
    let feature = h.app.session.doc.find_feature("Depth").unwrap().id;
    h.app.run("sketch.create", json!({"plane":"YZ","name":"Later sketch"})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("construct.plane.offset", json!({"base":"XY","offset":20,"name":"Later plane"})).unwrap();
    super::prepare_model(&mut h.app);
    assert_eq!(super::entries(&h.app, 0, "sketches").len(), 2);
    assert_eq!(super::entries(&h.app, 0, "construction").len(), 1);
    h.app.edit_feature(feature);
    // Both the fallback queries and the cached index must follow rollback.
    assert_eq!(super::entries(&h.app, 0, "sketches").iter().map(|e| e.label.as_str()).collect::<Vec<_>>(), ["Base"]);
    assert!(super::entries(&h.app, 0, "construction").is_empty());
    assert!(super::entries(&h.app, 0, "bodies").is_empty());
    super::prepare_model(&mut h.app);
    assert_eq!(super::entries(&h.app, 0, "sketches").len(), 1);
    assert!(super::entries(&h.app, 0, "construction").is_empty());
    crate::dialogs::cancel(&mut h.app);
    super::prepare_model(&mut h.app);
    assert_eq!(super::entries(&h.app, 0, "sketches").len(), 2);
    assert_eq!(super::entries(&h.app, 0, "construction").len(), 1);
    assert_eq!(super::entries(&h.app, 0, "bodies").len(), 1);
}

/// The sketch being edited is marked in the user's language, not with an English "(editing)",
/// and its default name reads as the browser shows it elsewhere.
#[test]
fn the_edited_sketch_is_marked_in_the_users_language() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY","name":"Sketch1"})).unwrap();
    let _japanese = crate::i18n::scope("ja");
    let rows = super::entries(&h.app, 0, "sketches");
    assert_eq!(rows.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(), ["スケッチ1  (編集中)"]);
}

/// A design saved before groups could nest or be ordered shows the browser it always did: the
/// groups in creation order, each with its items, above the loose items, which follow the saved
/// order and leave the ones it does not name to the end. That layout is spelled out here and
/// compared with the rows drawn now, including a saved order that still names a grouped item.
#[test]
fn a_design_saved_before_nested_groups_keeps_its_browser_order() {
    use super::cache::Row;
    const BODIES: [&str; 5] = ["Plate", "Washer", "Bolt", "Nut", "Cap"];
    fn drawn(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|r| match r {
                Row::Item(e) => e.key.clone(),
                Row::Group(g) => format!("{{{}: {}}}", g.label, drawn(&g.rows).join(",")),
            })
            .collect()
    }
    let cases: [(&[(&str, &[&str])], &[&str]); 3] = [
        (&[("Fasteners", &["Bolt", "Nut"])], &[]),
        (&[("Fasteners", &["Bolt", "Nut"])], &["Washer", "Bolt", "Cap"]),
        (&[("Zeta", &["Plate"]), ("Alpha", &["Cap", "Washer"])], &["Cap", "Nut"]),
    ];
    for (groups, saved) in cases {
        let mut h = crate::scenario::Harness::new();
        for (i, name) in BODIES.iter().enumerate() {
            h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"corner":[i as f64 * 20.0, 0, 0],"body_name":name})).unwrap();
        }
        for (name, items) in groups {
            h.app.run("browser.group", json!({"component":0,"folder":"bodies","name":name,"items":items})).unwrap();
        }
        if !saved.is_empty() {
            h.app.run("browser.order", json!({"folder":"bodies","order":saved})).unwrap();
        }
        // The old layout: groups in creation order, then the loose items by their saved place.
        let mut expected: Vec<String> = groups.iter().map(|(n, items)| format!("{{{n} ({}): {}}}", items.len(), items.join(","))).collect();
        let mut loose: Vec<&str> = BODIES.into_iter().filter(|b| !groups.iter().any(|(_, items)| items.contains(b))).collect();
        loose.sort_by_key(|b| saved.iter().position(|s| s == b).unwrap_or(usize::MAX));
        expected.extend(loose.into_iter().map(String::from));
        let rows = super::arrange(&h.app.session.doc, 0, "bodies", "c0", super::entries(&h.app, 0, "bodies"));
        assert_eq!(drawn(&rows), expected, "groups {groups:?}, saved order {saved:?}");
    }
}
