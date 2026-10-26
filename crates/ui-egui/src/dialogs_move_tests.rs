//! The Move window's main value: the one the canvas box shows and the keyboard types into.
//! It follows the move type, because each type uses a different value.
use serde_json::json;
use solidcraft_engine::Session;

use super::*;
use crate::Services;
use crate::dialogs::Dialog;

fn move_dialog(mode: usize) -> Dialog {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 20, "width": 20, "height": 20})).unwrap();
    let mut app = SolveApp::new(s, Services::default());
    app.start("solid.move");
    let mut d = app.dialog.clone().unwrap();
    d.kind = kind(mode, &app.session.doc);
    d.inputs = inputs(mode);
    if let Some(inp) = d.inputs.first_mut() {
        inp.items = vec![Sel::Body { name: "Body1".into() }];
    }
    d
}

#[test]
fn the_main_value_follows_the_move_type() {
    for mode in [FREE, TRANSLATE, POINT_TO_POSITION] {
        let mut d = move_dialog(mode);
        let got = d.primary().map(|p| (p.0, p.1));
        assert_eq!(got, Some(("X", ValueKind::Length)), "{} types a distance", TYPES[mode]);
    }

    let mut d = move_dialog(ROTATE);
    let got = d.primary().map(|p| (p.0, p.1));
    assert_eq!(got, Some(("Angle", ValueKind::Angle)), "Rotate types an angle, not an X distance");

    let mut d = move_dialog(POINT_TO_POINT);
    assert!(d.primary().is_none(), "Point to Point is two picks with nothing to type");
}

#[test]
fn the_main_value_writes_through_to_the_command() {
    let mut d = move_dialog(ROTATE);
    if let Some((_, _, value)) = d.primary() {
        *value = "90 deg".into();
    }
    let Kind::Move { angle, .. } = &d.kind else { panic!("not a move") };
    assert_eq!(angle, "90 deg", "the box edits the angle itself, not a copy of it");

    let mut d = move_dialog(TRANSLATE);
    if let Some((_, _, value)) = d.primary() {
        *value = "30 mm".into();
    }
    let Kind::Move { x, y, z, .. } = &d.kind else { panic!("not a move") };
    assert_eq!((x.as_str(), y.as_str(), z.as_str()), ("30 mm", "0 mm", "0 mm"), "the box edits X alone");
}

/// Free Move and Translate carry the triad. The other three have no manipulator yet, so their
/// value box is the only way to drive them: it must still have somewhere to sit.
#[test]
fn every_move_type_has_somewhere_to_put_its_value_box() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 20, "width": 20, "height": 20})).unwrap();
    let app = SolveApp::new(s, Services::default());
    for mode in [FREE, TRANSLATE, ROTATE, POINT_TO_POINT, POINT_TO_POSITION] {
        let d = move_dialog(mode);
        let has_triad = triad(&app, &d).is_some();
        assert_eq!(has_triad, matches!(mode, FREE | TRANSLATE), "{} triad", TYPES[mode]);
        let at = triad(&app, &d).map(|t| t.center).or_else(|| crate::canvas::anchor(&app, &d).map(|(base, _)| base));
        assert!(at.is_some(), "{} has nowhere to put its value box", TYPES[mode]);
    }
}

/// Point to Point takes the origin point as From or To (#3696): a click on the origin dot picks
/// it, and the body moves from its corner onto the origin.
#[test]
fn point_to_point_takes_the_origin_point() {
    let mut h = crate::scenario::Harness::new();
    h.app.set_origin_shown(true);
    h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20})).unwrap();
    h.app.run("solid.move", json!({"bodies": ["Body1"], "translate": ["30 mm", "20 mm", "0 mm"]})).unwrap();
    h.app.cam.distance = 200.0;
    h.settle();
    let hi = h.app.session.model.state().body("Body1").unwrap().mesh().bounds().max;
    h.app.start("solid.move");
    {
        let d = h.app.dialog.as_mut().unwrap();
        d.kind = kind(POINT_TO_POINT, &h.app.session.doc);
        d.inputs = inputs(POINT_TO_POINT);
        d.inputs[0].items = vec![Sel::Body { name: "Body1".into() }];
        d.active = 1;
    }
    let corner = h.call("ui.at", json!({ "world": [hi.x, hi.y, hi.z] }))["result"].clone();
    h.step(&json!({ "click": corner })).unwrap();
    h.app.dialog.as_mut().unwrap().active = 2;
    let origin = h.call("ui.at", json!({ "world": [0, 0, 0] }))["result"].clone();
    h.step(&json!({ "click": origin })).unwrap();
    let d = h.app.dialog.clone().unwrap();
    assert_eq!(d.inputs[2].items, vec![Sel::Vertex { body: String::new(), point: Vec3::ZERO }], "the origin is the To point");
    let p = params(&h.app, &d).unwrap();
    assert_eq!(p["translate"], json!([mm(-hi.x), mm(-hi.y), mm(-hi.z)]), "the corner goes onto the origin");
}

/// #4776: the box shows the value of the handle dragged: the distance along the Y arrow after
/// dragging it, the angle of a ring after turning it; typing then edits that value.
#[test]
fn the_box_shows_the_dragged_axis() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 20, "width": 20, "height": 20})).unwrap();
    let app = SolveApp::new(s, Services::default());
    let mut d = move_dialog(FREE);
    let g = |handle, change| crate::gizmo::Drag { handle, change, started: true, done: true };
    drag(&app, &mut d, g(crate::gizmo::Handle::Axis(1), Change::Translate(Vec3::new(0.0, 5.0, 0.0))));
    let (label, kind, value) = d.primary().unwrap();
    assert_eq!((label, kind, value.as_str()), ("Y", ValueKind::Length, "5 mm"));
    *value = "7 mm".into();
    let Kind::Move { x, y, .. } = &d.kind else { panic!("not a move") };
    assert_eq!((x.as_str(), y.as_str()), ("0 mm", "7 mm"), "typing edits Y");
    drag(&app, &mut d, g(crate::gizmo::Handle::Ring(2), Change::Rotate { axis: 2, angle: 0.5 }));
    assert_eq!(d.primary().map(|p| (p.0, p.1)), Some(("Z Angle", ValueKind::Angle)));
    // Translate has no rings: a ring index past its values leaves the box on a distance.
    let mut t = move_dialog(TRANSLATE);
    drag(&app, &mut t, g(crate::gizmo::Handle::Axis(2), Change::Translate(Vec3::new(0.0, 0.0, 3.0))));
    assert_eq!(t.primary().map(|p| p.0), Some("Z"));
}

/// #4776: Tab in the box goes to the next value (X, Y, Z, then the angles in Free Move) and
/// Shift+Tab back, the keyboard staying in the box; Enter moves the body by what was typed.
#[test]
fn tab_in_the_box_goes_to_the_next_axis() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20})).unwrap();
    h.settle();
    h.app.run("select.set", json!({"items": [Sel::Body { name: "Body1".into() }]})).unwrap();
    h.app.start("solid.move");
    h.frames(3);
    assert_eq!(h.app.dialog.as_mut().and_then(|d| d.primary()).map(|p| p.0), Some("X"));
    h.step(&json!({"key": "Tab"})).unwrap();
    h.frames(2);
    assert_eq!(h.app.dialog.as_mut().and_then(|d| d.primary()).map(|p| p.0), Some("Y"));
    h.step(&json!({"key": "Tab"})).unwrap();
    h.step(&json!({"key": "Tab", "shift": true})).unwrap();
    h.frames(2);
    assert_eq!(h.app.dialog.as_mut().and_then(|d| d.primary()).map(|p| p.0), Some("Y"), "Shift+Tab goes back");
    h.step(&json!({"text": "12"})).unwrap();
    h.frames(1);
    h.step(&json!({"key": "Enter"})).unwrap();
    h.settle();
    let b = h.app.session.model.state().body("Body1").unwrap().mesh().bounds();
    assert!((b.min.y - 12.0).abs() < 1e-6 && b.min.x.abs() < 1e-6, "moved along Y: {b:?}");
}

fn component_selection_fixture() -> crate::scenario::Harness {
    let mut h = crate::scenario::Harness::new();
    for (command, params) in [
        ("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [-30, 0, 0], "body_name": "Outside"})),
        ("component.create", json!({"name": "Part"})),
        ("solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Inside"})),
        ("component.create", json!({"name": "Child"})),
        ("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [30, 0, 0], "body_name": "Nested"})),
        ("component.activate", json!({"component": "root"})),
        ("component.create", json!({"name": "Sibling"})),
        ("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [60, 0, 0], "body_name": "SiblingBody"})),
        ("component.activate", json!({"component": "Part"})),
    ] {
        h.app.run(command, params).unwrap();
    }
    h.app.ui.show_browser = false;
    h.app.ui.show_timeline = false;
    h.app.set_origin_shown(false);
    h.step(&json!({"call": "ui.view", "params": {"view": "top"}})).unwrap();
    // Keep the selection rectangle to the right of the Move dialog.
    h.step(&json!({"call": "ui.camera", "params": {"target": [0, 0, 0], "distance": 250}})).unwrap();
    h
}

fn select_component_boxes(h: &mut crate::scenario::Harness, crossing: bool, add: bool) {
    let ends = if crossing { [[75, -5, 0], [-35, 15, 0]] } else { [[-35, -5, 0], [75, 15, 0]] };
    h.step(&json!({"drag": [{"world": ends[0]}, {"world": ends[1]}], "shift": add})).unwrap();
}

#[test]
fn move_box_selection_stays_in_the_active_component_and_its_children() {
    for crossing in [false, true] {
        let mut h = component_selection_fixture();
        h.app.start("solid.move");
        h.settle();
        for add in [false, true] {
            select_component_boxes(&mut h, crossing, add);
            let names = h.app.dialog.as_ref().unwrap().inputs[0].items.body_names();
            assert_eq!(names, ["Inside", "Nested"], "crossing={crossing}, add={add}");
        }
    }
}

#[test]
fn body_box_preselection_uses_component_scope_and_root_keeps_all_bodies() {
    let mut h = component_selection_fixture();
    select_component_boxes(&mut h, false, false);
    assert_eq!(h.app.session.selection.body_names(), ["Inside", "Nested"]);
    h.app.start("solid.move");
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items.body_names(), ["Inside", "Nested"]);
    h.step(&json!({"key": "Escape"})).unwrap();
    h.app.run("component.activate", json!({"component": "root"})).unwrap();
    h.app.run("select.clear", json!({})).unwrap();
    h.app.start("solid.move");
    h.settle();
    select_component_boxes(&mut h, true, false);
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items.body_names(), ["Outside", "Inside", "SiblingBody", "Nested"]);
}

#[test]
fn move_box_selection_uses_the_active_occurrence_path() {
    let mut h = component_selection_fixture();
    let part = h.app.session.active_component;
    let second = h.app.run("occurrence.copy", json!({"component": part, "translate": [0, 30, 0]})).unwrap()["occurrence"].as_u64().unwrap();
    h.app.run("component.activate", json!({"component": part, "occurrence": second, "path": [second]})).unwrap();
    let mut expected = h.app.session.occurrence_bodies(&[second], true).unwrap();
    expected.sort();
    h.app.start("solid.move");
    h.settle();
    h.step(&json!({"drag": [{"world": [-35, -5, 0]}, {"world": [75, 45, 0]}]})).unwrap();
    let mut names = h.app.dialog.as_ref().unwrap().inputs[0].items.body_names();
    names.sort();
    assert_eq!(names, expected, "only the active placement and its nested child are picked");
}

#[test]
fn move_reference_box_selection_can_still_use_an_inactive_body() {
    let mut h = component_selection_fixture();
    h.app.start("solid.move");
    {
        let d = h.app.dialog.as_mut().unwrap();
        d.kind = kind(ROTATE, &h.app.session.doc);
        d.inputs = inputs(ROTATE);
        d.inputs[0].items = vec![Sel::Body { name: "Inside".into() }];
        d.active = 1;
    }
    h.settle();
    h.step(&json!({"drag": [{"world": [-35, -5, 0]}, {"world": [-15, 15, 0]}]})).unwrap();
    let axis = &h.app.dialog.as_ref().unwrap().inputs[1].items;
    assert!(matches!(axis.as_slice(), [Sel::Edge { body, .. }] if body == "Outside"), "{axis:?}");
}

/// The triad and the command must agree even when Free Move precomputes a pivoted rotation.
#[test]
fn inch_move_lengths_agree_in_the_triad_and_every_numeric_move_route() {
    for mode in [FREE, TRANSLATE, POINT_TO_POSITION] {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length":20,"width":20,"height":20})).unwrap();
        s.execute("document.units", &json!({"units":"in"})).unwrap();
        let mut app = SolveApp::new(s, Services::default());
        let mut d = move_dialog(mode);
        d.kind = kind(mode, &app.session.doc);
        if let Kind::Move { x, y, z, rz, .. } = &mut d.kind {
            *x = "0.25".into();
            *y = "2 mm".into();
            *z = "0".into();
            if mode == FREE {
                *rz = "90 deg".into();
            }
        }
        if mode == POINT_TO_POSITION {
            d.inputs[1].items = vec![Sel::Vertex { body: "Body1".into(), point: Vec3::new(10.0, 10.0, 10.0) }];
        } else {
            let center = triad(&app, &d).unwrap().center;
            assert!(center.dist(Vec3::new(16.35, 12.0, 10.0)) < 1e-9, "{center:?}");
        }
        for (id, p) in crate::dialogs::apply_commands(&app, &d).unwrap() {
            app.run(&id, p).unwrap();
        }
        let measured = app.session.execute("inspect.measure", &json!({})).unwrap();
        let center: Vec<f64> = measured["bodies"][0]["center_of_mass"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
        let expected = if mode == POINT_TO_POSITION { [6.35, 2.0, 0.0] } else { [16.35, 12.0, 10.0] };
        for (got, expected) in center.into_iter().zip(expected) {
            assert!((got - expected).abs() < 1e-6, "mode {mode}: {got}, expected {expected}");
        }
    }
}

/// A 60 × 40 × 10 plate with a Ø8 hole at (45, 20), Point to Point picking From.
fn holed_plate_picking_from() -> crate::scenario::Harness {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"length": 60, "width": 40, "height": 10})).unwrap();
    h.app.run("solid.cylinder", json!({"radius": 4, "height": 12, "base": [45, 20, -1], "operation": "cut"})).unwrap();
    h.app.cam.distance = 160.0;
    h.settle();
    h.app.start("solid.move");
    let d = h.app.dialog.as_mut().unwrap();
    d.kind = kind(POINT_TO_POINT, &h.app.session.doc);
    d.inputs = inputs(POINT_TO_POINT);
    d.inputs[0].items = vec![Sel::Body { name: "Body1".into() }];
    d.active = 1;
    h
}

/// Hovering a hole's rim wakes its centre (#5912): a click there, in the hole's empty middle,
/// picks the centre exactly.
#[test]
fn hovering_a_rim_wakes_its_centre_for_a_click() {
    let mut h = holed_plate_picking_from();
    h.step(&json!({ "move": { "world": [49, 20, 10] } })).unwrap();
    let centre = Vec3::new(45.0, 20.0, 10.0);
    assert!(h.app.viewport.woken.points.iter().any(|p| p.at.dist(centre) < 1e-6), "{:?}", h.app.viewport.woken);
    h.step(&json!({ "click": { "world": [45, 20, 10] } })).unwrap();
    let from = &h.app.dialog.as_ref().unwrap().inputs[1].items;
    assert!(matches!(from.as_slice(), [Sel::Vertex { body, point }] if body == "Body1" && point.dist(centre) < 1e-6), "{from:?}");
}

/// Hovering another edge puts the woken points to sleep and wakes its own: a straight edge's
/// middle is picked from further away than the edge itself is.
#[test]
fn another_edge_wakes_its_own_points_and_the_rest_sleep() {
    let mut h = holed_plate_picking_from();
    h.step(&json!({ "move": { "world": [49, 20, 10] } })).unwrap();
    h.step(&json!({ "move": { "world": [20, 0, 10] } })).unwrap();
    let woken: Vec<Vec3> = h.app.viewport.woken.points.iter().map(|p| p.at).collect();
    assert!(!woken.iter().any(|p| p.dist(Vec3::new(45.0, 20.0, 10.0)) < 1e-6), "the rim's centre sleeps: {woken:?}");
    let middle = Vec3::new(30.0, 0.0, 10.0);
    assert!(woken.iter().any(|p| p.dist(middle) < 1e-6), "{woken:?}");
    // 12 points below the middle: past an edge's reach, within a woken point's.
    let at = h.call("ui.at", json!({ "world": [30, 0, 10] }))["result"].clone();
    let below = json!([at[0].as_f64().unwrap(), at[1].as_f64().unwrap() + 12.0]);
    h.step(&json!({ "click": below })).unwrap();
    let from = &h.app.dialog.as_ref().unwrap().inputs[1].items;
    assert!(matches!(from.as_slice(), [Sel::Vertex { point, .. }] if point.dist(middle) < 1e-6), "{from:?}");
}

/// An input that doesn't take hover pick points (Rotate's axis) wakes none.
#[test]
fn an_axis_input_wakes_no_points() {
    let mut h = holed_plate_picking_from();
    let d = h.app.dialog.as_mut().unwrap();
    d.kind = kind(ROTATE, &h.app.session.doc);
    d.inputs = inputs(ROTATE);
    d.inputs[0].items = vec![Sel::Body { name: "Body1".into() }];
    d.active = 1;
    h.step(&json!({ "move": { "world": [49, 20, 10] } })).unwrap();
    assert!(h.app.viewport.woken.points.is_empty(), "{:?}", h.app.viewport.woken);
}
