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

#[test]
fn editing_a_move_in_a_turned_component_keeps_world_distances() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("component.create", json!({"name": "Part", "grounded": false})).unwrap();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    h.app.run("parameters.add", json!({"name": "travel", "expression": "4 mm"})).unwrap();
    h.app.run("occurrence.move", json!({"occurrence": "Part:1", "axis": [1, 0, 0], "angle": "90 deg", "translate": [50, 10, 20]})).unwrap();
    h.app.run("component.activate", json!({"component": "root"})).unwrap();
    let moved = h.app.run("solid.move", json!({"bodies": ["Body1"], "translate": ["0 mm", "-travel", "0 mm"]})).unwrap()["feature"].as_u64().unwrap();
    let bounds = |h: &crate::scenario::Harness| crate::open_meshing::session_world(&h.app.session).body("Body1").unwrap().mesh().bounds();
    let before = bounds(&h);
    let mut d = crate::dialogs::for_feature(&h.app, moved, None).unwrap();
    let Kind::Move { x, y, z, .. } = &d.kind else { panic!("not a move") };
    let displayed = Vec3::new(len(&h.app, x).unwrap(), len(&h.app, y).unwrap(), len(&h.app, z).unwrap());
    assert!((displayed - Vec3::new(0.0, -4.0, 0.0)).len() < 1e-6, "{displayed:?}");
    for (command, params) in crate::dialogs::apply_commands(&h.app, &d).unwrap() {
        h.app.run(&command, params).unwrap();
    }
    let after = bounds(&h);
    assert!((after.min - before.min).len() < 1e-6 && (after.max - before.max).len() < 1e-6, "accepting the unchanged dialog must not move the body");
    h.app.run("parameters.change", json!({"name": "travel", "expression": "8 mm"})).unwrap();
    assert!((bounds(&h).min - before.min - Vec3::new(0.0, -4.0, 0.0)).len() < 1e-6);
    if let Kind::Move { y, .. } = &mut d.kind {
        *y = "-12 mm".into();
    }
    for (command, params) in crate::dialogs::apply_commands(&h.app, &d).unwrap() {
        h.app.run(&command, params).unwrap();
    }
    assert!((bounds(&h).min - before.min - Vec3::new(0.0, -8.0, 0.0)).len() < 1e-6, "edited distances follow world axes too");
}

/// #6822: Rotate must offer a draggable handle after the body and axis are picked.
#[test]
fn rotate_handle_updates_the_angle_without_moving_the_camera() {
    let mut h = crate::scenario::Harness::new();
    h.app.preferences.nav = "openscad".into();
    h.app.run("solid.box", json!({"length": 20, "width": 30, "height": 10})).unwrap();
    h.step(&json!({"call": "ui.view", "params": {"view": "top"}})).unwrap();
    h.app.cam.distance = 180.0;
    h.app.start("solid.move");
    {
        let d = h.app.dialog.as_mut().unwrap();
        d.kind = kind(ROTATE, &h.app.session.doc);
        d.inputs = inputs(ROTATE);
        d.inputs[0].items = vec![Sel::Body { name: "Body1".into() }];
        d.inputs[1].items = vec![Sel::Axis { name: "Z".into() }];
    }
    h.frames(5);
    let before = serde_json::to_value(&*h.app.session.doc).unwrap();
    let camera = h.app.cam;
    let start = h.at(&json!({"handle": "move_rotate"})).unwrap();
    let centre = h.at(&json!({"world": [0, 0, 5]})).unwrap();
    // A quarter turn clockwise in the top view is -90 degrees about +Z.
    let end = [centre[0] - (start[1] - centre[1]), centre[1] + (start[0] - centre[0])];
    h.step(&json!({"drag": [start, end], "steps": 12})).unwrap();
    let d = h.app.dialog.as_ref().unwrap();
    let p = params(&h.app, d).unwrap();
    assert_eq!(p["angle"], "-90 deg");
    assert_eq!(p["axis"], json!([0.0, 0.0, 1.0]));
    assert_eq!(h.app.cam, camera, "the handle owns the drag");
    assert_eq!(serde_json::to_value(&*h.app.session.doc).unwrap(), before, "a preview does not commit the move");
    h.step(&json!({"key": "Enter"})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none());
    let bounds = h.app.session.model.state().body("Body1").unwrap().mesh().bounds();
    assert!((bounds.min - Vec3::new(0.0, -20.0, 0.0)).len() < 1e-6, "{bounds:?}");
    assert!((bounds.max - Vec3::new(30.0, 0.0, 10.0)).len() < 1e-6, "{bounds:?}");
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(serde_json::to_value(&*h.app.session.doc).unwrap(), before);
}

#[test]
fn rotate_handle_follows_an_offset_axis_and_preserves_full_turns() {
    for (direction, initial, expected, free) in [(Vec3::new(1.0, 1.0, 1.0), "720 deg", 755.0, false), (-Vec3::Z, "", 37.3, true)] {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length": 20, "width": 30, "height": 10})).unwrap();
        let origin = Vec3::new(5.0, 7.0, 9.0);
        let end = origin + direction;
        h.app
            .run(
                "construct.axis.two_points",
                json!({"a": {"point": [origin.x, origin.y, origin.z]}, "b": {"point": [end.x, end.y, end.z]}, "name": "TurnAxis"}),
            )
            .unwrap();
        h.app.cam = h.app.cam.looking_from(direction);
        h.app.cam.distance = 180.0;
        h.app.start("solid.move");
        {
            let d = h.app.dialog.as_mut().unwrap();
            d.kind = kind(ROTATE, &h.app.session.doc);
            d.inputs = inputs(ROTATE);
            d.inputs[0].items = vec![Sel::Body { name: "Body1".into() }];
        }
        h.frames(3);
        assert!(crate::scenario::handle_at("move_rotate").is_none(), "an axis must be selected first");
        {
            let d = h.app.dialog.as_mut().unwrap();
            d.inputs[1].items = vec![Sel::Axis { name: "TurnAxis".into() }];
            if let Kind::Move { angle, .. } = &mut d.kind {
                *angle = initial.into();
            }
        }
        h.frames(3);
        let before = serde_json::to_value(&*h.app.session.doc).unwrap();
        let d = h.app.dialog.as_ref().unwrap();
        let (centre, axis) = rotation_axis(&h.app, d).unwrap();
        assert!((centre - origin).cross(direction).len() < 1e-9);
        assert!((axis - direction.normalized().unwrap()).len() < 1e-9);
        let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
        let radius = crate::gizmo::ring_radius(&proj, centre).unwrap();
        let e1 = axis.any_perp();
        let e2 = axis.cross(e1);
        let th = 37.3_f64.to_radians();
        let target = centre + (e1 * th.cos() + e2 * th.sin()) * radius;
        h.step(&json!({"drag": [{"handle": "move_rotate"}, {"world": [target.x, target.y, target.z]}], "ctrl": free, "steps": 12})).unwrap();
        let d = h.app.dialog.as_ref().unwrap();
        let p = params(&h.app, d).unwrap();
        let actual = ang(&h.app, p["angle"].as_str().unwrap()).unwrap();
        assert!((actual.to_degrees() - expected).abs() < 0.11, "{p}");
        let pivot: [String; 3] = serde_json::from_value(p["origin"].clone()).unwrap();
        let pivot = pivot.map(|v| len(&h.app, &v).unwrap());
        let pivot = Vec3::new(pivot[0], pivot[1], pivot[2]);
        let moved_origin = gizmo::apply(&gizmo::rotation(axis, actual), origin - pivot) + pivot;
        assert!((moved_origin - origin).len() < 1e-5, "the selected axis stays fixed");
        h.step(&json!({"widget": "Cancel"})).unwrap();
        assert_eq!(serde_json::to_value(&*h.app.session.doc).unwrap(), before);
    }
}

/// Free Move and Translate carry the triad. The other modes still need a place for their value
/// box before enough geometry is picked to draw a handle.
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

/// After a body is picked, Point to Point, Point to Position and Rotate go on to their empty
/// From, Point or Axis, so the next click picks that instead of another body (#6170). Free
/// Move stays on Bodies: its Pivot is optional.
#[test]
fn a_body_pick_goes_on_to_the_point_or_axis() {
    for (mode, next) in [(POINT_TO_POINT, Some(1)), (POINT_TO_POSITION, Some(1)), (ROTATE, Some(1)), (FREE, None)] {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length": 60, "width": 40, "height": 10})).unwrap();
        h.app.run("select.clear", json!({})).unwrap();
        h.app.cam.distance = 160.0;
        h.settle();
        h.app.start("solid.move");
        let d = h.app.dialog.as_mut().unwrap();
        d.kind = kind(mode, &h.app.session.doc);
        d.inputs = inputs(mode);
        d.active = 0;
        h.step(&json!({ "click": { "world": [30, 20, 10] } })).unwrap();
        let d = h.app.dialog.as_ref().unwrap();
        assert_eq!(d.inputs[0].items, vec![Sel::Body { name: "Body1".into() }], "{}", TYPES[mode]);
        assert_eq!(d.active, next.unwrap_or(0), "{} goes on after the body", TYPES[mode]);
    }
    // Point to Point: the click after the body is the From point, the body stays picked.
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.app.run("solid.box", json!({"length": 60, "width": 40, "height": 10})).unwrap();
    h.app.run("select.clear", json!({})).unwrap();
    h.app.cam.distance = 160.0;
    h.settle();
    h.app.start("solid.move");
    let d = h.app.dialog.as_mut().unwrap();
    d.kind = kind(POINT_TO_POINT, &h.app.session.doc);
    d.inputs = inputs(POINT_TO_POINT);
    d.active = 0;
    h.step(&json!({ "click": { "world": [30, 30, 10] } })).unwrap();
    h.step(&json!({ "click": { "world": [60, 0, 10] } })).unwrap();
    let d = h.app.dialog.as_ref().unwrap();
    assert_eq!(d.inputs[0].items.len(), 1, "the body is still picked");
    assert!(matches!(d.inputs[1].items.as_slice(), [Sel::Vertex { .. }]), "{:?}", d.inputs[1].items);
}

/// #14205: once Point to Point, Point to Position or Rotate has gone on from Bodies (#6170), a
/// Ctrl or Shift click on another body adds it to Bodies, and the window stays where it was, so
/// the next plain click still picks the From point, Point or Axis. A Ctrl/Shift click on a picked
/// body takes it out. Free Move never goes on, and plain clicks there add bodies as before.
#[test]
fn ctrl_or_shift_click_adds_bodies_after_move_has_gone_on() {
    for mode in [POINT_TO_POINT, POINT_TO_POSITION, ROTATE] {
        let mut h = three_bodies_in_move(mode);
        let body = |name: &str| Sel::Body { name: name.into() };
        let (a, b, c) = (body("A"), body("B"), body("C"));
        h.step(&json!({ "click": { "world": [10, 10, 10] } })).unwrap();
        assert_eq!(h.app.dialog.as_ref().unwrap().active, 1, "{}: a plain click goes on (#6170)", TYPES[mode]);
        h.step(&json!({ "click": { "world": [40, 10, 10] }, "ctrl": true })).unwrap();
        h.step(&json!({ "click": { "world": [70, 10, 10] }, "shift": true })).unwrap();
        let d = h.app.dialog.as_ref().unwrap();
        assert_eq!(d.inputs[0].items, vec![a.clone(), b.clone(), c.clone()], "{}: Ctrl and Shift clicks add bodies", TYPES[mode]);
        assert_eq!(d.active, 1, "{}: and stay on {}", TYPES[mode], d.inputs[1].label);
        assert!(d.inputs[1].items.is_empty(), "{}: {:?}", TYPES[mode], d.inputs[1].items);
        h.step(&json!({ "click": { "world": [40, 10, 10] }, "ctrl": true })).unwrap();
        assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items, vec![a.clone(), c.clone()], "{}: a picked body comes out", TYPES[mode]);
        // The next plain click is the From point, Point or Axis, not another body.
        let edge = if mode == ROTATE { [10, 0, 10] } else { [0, 0, 10] };
        h.step(&json!({ "click": { "world": edge } })).unwrap();
        let d = h.app.dialog.as_ref().unwrap();
        assert_eq!(d.inputs[0].items, vec![a, c], "{}: the plain click picked no body", TYPES[mode]);
        assert_eq!(d.inputs[1].items.len(), 1, "{}: it picked {}", TYPES[mode], d.inputs[1].label);
    }
}

/// Boxes A, B and C (20 wide, at x 0, 30 and 60) seen from the top, and Move open on `mode`
/// with Bodies active.
fn three_bodies_in_move(mode: usize) -> crate::scenario::Harness {
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    for (x, name) in [(0, "A"), (30, "B"), (60, "C")] {
        h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [x, 0, 0], "body_name": name})).unwrap();
    }
    h.app.run("select.clear", json!({})).unwrap();
    h.call("ui.view", json!({"view": "top"}));
    h.app.fit_view();
    h.settle();
    h.app.start("solid.move");
    let d = h.app.dialog.as_mut().unwrap();
    d.kind = kind(mode, &h.app.session.doc);
    d.inputs = inputs(mode);
    d.active = 0;
    h
}

/// #14205: with Shift adding only (Preferences), a Shift click on a body Move has picked already
/// keeps it in Bodies, while a Ctrl click still takes it out.
#[test]
fn an_add_only_shift_click_keeps_a_picked_body_in_move() {
    let mut h = three_bodies_in_move(POINT_TO_POINT);
    h.app.preferences.selection_shift_add_only = true;
    let body = |name: &str| Sel::Body { name: name.into() };
    h.step(&json!({ "click": { "world": [10, 10, 10] } })).unwrap();
    h.step(&json!({ "click": { "world": [40, 10, 10] }, "shift": true })).unwrap();
    h.step(&json!({ "click": { "world": [40, 10, 10] }, "shift": true })).unwrap();
    let d = h.app.dialog.as_ref().unwrap();
    assert_eq!(d.inputs[0].items, vec![body("A"), body("B")], "Shift adds only");
    assert_eq!(d.active, 1, "and Move stays on {}", d.inputs[1].label);
    h.step(&json!({ "click": { "world": [40, 10, 10] }, "ctrl": true })).unwrap();
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items, vec![body("A")], "Ctrl still takes it out");
}

/// #14205: while OK computes, a Ctrl or Shift click leaves Bodies as it is, as other picks do.
#[test]
fn a_ctrl_or_shift_click_holds_bodies_while_move_computes() {
    let mut h = three_bodies_in_move(POINT_TO_POINT);
    h.step(&json!({ "click": { "world": [10, 10, 10] } })).unwrap();
    let (_finish, done) = std::sync::mpsc::channel();
    h.app.applying = Some(crate::dialogs::Applying {
        done,
        ready: None,
        seen: true,
        started: crate::now_ms(),
        cmds: Vec::new(),
        progress: Default::default(),
        quality: Default::default(),
    });
    h.step(&json!({ "click": { "world": [40, 10, 10] }, "ctrl": true })).unwrap();
    h.step(&json!({ "click": { "world": [10, 10, 10] }, "shift": true })).unwrap();
    let d = h.app.dialog.as_ref().unwrap();
    assert_eq!(d.inputs[0].items, vec![Sel::Body { name: "A".into() }]);
    assert_eq!(d.active, 1);
}

/// #8265: input queued while the window is busy arrives together, so one frame can carry the
/// button let go and the mouse already moving on. The drag must end where the button was
/// released, not jump the value to where the mouse has since got to.
#[test]
fn triad_drag_ends_where_the_button_was_released() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"length": 20, "width": 30, "height": 10})).unwrap();
    h.app.start("solid.move");
    {
        let d = h.app.dialog.as_mut().unwrap();
        d.inputs[0].items = vec![Sel::Body { name: "Body1".into() }];
    }
    h.frames(3);
    let pos = |p: [f64; 2]| egui::pos2(p[0] as f32, p[1] as f32);
    let button = |p: [f64; 2], pressed: bool| egui::Event::PointerButton {
        pos: pos(p),
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    let grip = h.at(&json!({"gizmo": "x"})).unwrap();
    let released = h.at(&json!({"gizmo": "x", "by": [20.0, 0.0, 0.0]})).unwrap();
    let later = h.at(&json!({"gizmo": "x", "by": [30.0, 0.0, 0.0]})).unwrap();
    let value = |h: &crate::scenario::Harness| match &h.app.dialog.as_ref().unwrap().kind {
        Kind::Move { x, .. } => x.clone(),
        other => panic!("not a move: {other:?}"),
    };
    // Hover the arrow, press it, then drag 20 mm along it.
    h.frame_with(vec![egui::Event::PointerMoved(pos(grip))]);
    h.frame_with(vec![button(grip, true)]);
    h.frame_with(vec![egui::Event::PointerMoved(pos(released))]);
    assert_eq!(value(&h), "20 mm", "the drag follows the pointer");
    // The button is let go and the mouse already moves on: one frame carries both.
    h.frame_with(vec![button(released, false), egui::Event::PointerMoved(pos(later))]);
    assert_eq!(value(&h), "20 mm", "the drag ends where the button was released");
}

/// A Move dialog on the 20 × 30 × 10 box, translated 10 mm along X.
fn translated_box() -> crate::scenario::Harness {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"length": 20, "width": 30, "height": 10})).unwrap();
    h.app.start("solid.move");
    let d = h.app.dialog.as_mut().unwrap();
    d.kind = kind(TRANSLATE, &h.app.session.doc);
    d.inputs = inputs(TRANSLATE);
    d.inputs[0].items = vec![Sel::Body { name: "Body1".into() }];
    if let Kind::Move { x, .. } = &mut d.kind {
        *x = "10 mm".into();
    }
    h
}

/// #8746: changing the Move Type keeps the move made so far, and the new type moves the body
/// on from there; one solid.move holds the chain.
#[test]
fn changing_the_move_type_keeps_the_move_made_so_far() {
    let mut h = translated_box();
    let corners = |b: solidcraft_engine::geom::Aabb3| [b.min, b.max];
    let before = corners(crate::open_meshing::session_world(&h.app.session).body("Body1").unwrap().mesh().bounds());
    let mut d = h.app.dialog.take().unwrap();
    switch_type(&h.app, &mut d, ROTATE);
    // No axis yet: the translation alone is made.
    let p = params(&h.app, &d).unwrap();
    assert_eq!(p["translate"], json!(["10 mm", "0 mm", "0 mm"]), "{p}");
    assert!(p.get("angle").is_none(), "{p}");
    // A quarter turn about Z, after the translation.
    d.inputs[1].items = vec![Sel::Axis { name: "Z".into() }];
    if let Kind::Move { angle, .. } = &mut d.kind {
        *angle = "90 deg".into();
    }
    for (command, params) in crate::dialogs::apply_commands(&h.app, &d).unwrap() {
        h.app.run(&command, params).unwrap();
    }
    let after = crate::open_meshing::session_world(&h.app.session).body("Body1").unwrap().mesh().bounds();
    let turn = |p: Vec3| Vec3::new(-p.y, p.x + 10.0, p.z);
    let mut want = solidcraft_engine::geom::Aabb3::EMPTY;
    for c in before {
        want.add(turn(c));
    }
    assert!((after.min - want.min).len() < 1e-6 && (after.max - want.max).len() < 1e-6, "{after:?} != {want:?}");
}

/// A free turn about an off-origin pivot survives a switch to Translate: Translate saves no
/// `origin` of its own, yet the applied move keeps that turn and adds the later translation.
#[test]
fn changing_move_types_keeps_an_off_origin_rotation() {
    let mut h = translated_box();
    let mut d = h.app.dialog.take().unwrap();
    switch_type(&h.app, &mut d, FREE);
    d.extra.insert("pivot".into(), json!([20.0, 15.0, 5.0]));
    d.extra.insert("origin".into(), json!(["20 mm", "15 mm", "5 mm"]));
    if let Kind::Move { rz, .. } = &mut d.kind {
        *rz = "90 deg".into();
    }
    let p = params(&h.app, &d).unwrap();
    let m = transform(&h.app, &p).unwrap();
    assert!(apply_point(&m, Vec3::ZERO).dist(Vec3::new(35.0, 5.0, 0.0)) < 1e-6, "{p}");
    switch_type(&h.app, &mut d, TRANSLATE);
    assert!(!d.extra.contains_key("origin"));
    if let Kind::Move { z, .. } = &mut d.kind {
        *z = "7 mm".into();
    }
    for (command, params) in crate::dialogs::apply_commands(&h.app, &d).unwrap() {
        h.app.run(&command, params).unwrap();
    }
    let after = h.app.session.world_state().unwrap().body("Body1").unwrap().mesh().bounds();
    assert!(after.min.dist(Vec3::new(5.0, 5.0, 7.0)) < 1e-6 && after.max.dist(Vec3::new(35.0, 25.0, 17.0)) < 1e-6, "{after:?}");
}

/// #8746: a pick on the moving body is made where the preview draws it, and is kept where the
/// current type's move starts from.
#[test]
fn a_pick_on_the_moved_body_is_taken_back_by_the_current_move() {
    let h = translated_box();
    let shown = Sel::Vertex { body: "Body1".into(), point: Vec3::new(30.0, 30.0, 10.0) };
    let kept = from_shown(&h.app, shown);
    assert_eq!(kept, Sel::Vertex { body: "Body1".into(), point: Vec3::new(20.0, 30.0, 10.0) });
    assert_eq!(shown_point(&h.app, "Body1", Vec3::new(20.0, 30.0, 10.0)), Vec3::new(30.0, 30.0, 10.0));
    let elsewhere = Sel::Vertex { body: "Other".into(), point: Vec3::new(30.0, 30.0, 10.0) };
    assert_eq!(from_shown(&h.app, elsewhere.clone()), elsewhere, "a pick on another body stays where it was made");
}

/// A woken point of the moved body shows and is picked where the preview draws the body, not
/// where the design keeps it.
#[test]
fn woken_points_of_the_moved_body_follow_the_preview() {
    let mut h = translated_box();
    let kept = Vec3::new(20.0, 30.0, 10.0);
    h.app.viewport.woken.points =
        vec![solidcraft_engine::snap::SnapPoint { at: kept, kind: solidcraft_engine::snap::SnapKind::Middle, body: "Body1".into() }];
    let shown: Vec<Vec3> = crate::snap_wake::shown(&h.app).into_iter().map(|(at, _)| at).collect();
    assert_eq!(shown, vec![Vec3::new(30.0, 30.0, 10.0)]);
    h.app.viewport.woken.points[0].body = "Other".into();
    let shown: Vec<Vec3> = crate::snap_wake::shown(&h.app).into_iter().map(|(at, _)| at).collect();
    assert_eq!(shown, vec![kept], "another body's point stays put");
}

/// The prefilled Free Move angles must re-make the turn the feature stored, whatever the axis:
/// recomposing them the way `step` does (Z, then Y, then X) gives the same rotation matrix.
#[test]
fn the_prefilled_angles_remake_the_stored_turn() {
    let cases = [
        // #9696: the turn a Free Move of 10 deg on each axis stores.
        (Vec3::new(0.5407162030609368, 0.6444004775717719, 0.5407162030609368), 16.7865_f64.to_radians()),
        (Vec3::X, std::f64::consts::PI),
        (Vec3::Y, std::f64::consts::FRAC_PI_2),
        (Vec3::new(0.0, 0.0, -1.0), 0.5),
        (Vec3::new(1.0, 2.0, 3.0).normalized().unwrap(), 0.7),
        (Vec3::new(-0.3, 0.8, -0.5).normalized().unwrap(), -1.2),
    ];
    for (axis, th) in cases {
        let (rx, ry, rz) = euler_from_axis_angle(axis, th);
        let r = gizmo::mul(&gizmo::mul(&gizmo::rotation(Vec3::Z, rz), &gizmo::rotation(Vec3::Y, ry)), &gizmo::rotation(Vec3::X, rx));
        let o = gizmo::rotation(axis, th);
        for i in 0..3 {
            for j in 0..3 {
                assert!((r[i][j] - o[i][j]).abs() < 1e-9, "axis {axis:?} angle {th}: {r:?} != {o:?}");
            }
        }
    }
}

/// The #9696 repro: a Free Move of 10 mm and 10 deg on each axis stores one turn about a
/// slanted axis; editing shows the stored translation and angles that make that turn again,
/// re-saving unchanged does not move the body, and zeroing every value brings it back.
#[test]
fn editing_a_free_move_shows_the_values_it_was_made_with() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20})).unwrap();
    let id = h
        .app
        .run(
            "solid.move",
            json!({
                "bodies": ["Body1"],
                "translate": ["9.729024 mm", "9.95222 mm", "10.327918 mm"],
                "axis": [0.5407162030609368, 0.6444004775717719, 0.5407162030609368],
                "angle": "16.7865 deg",
            }),
        )
        .unwrap()["feature"]
        .as_u64()
        .unwrap();
    let bounds = |h: &crate::scenario::Harness| crate::open_meshing::session_world(&h.app.session).body("Body1").unwrap().mesh().bounds();
    let before = bounds(&h);
    let d = crate::dialogs::for_feature(&h.app, id, None).unwrap();
    let Kind::Move { x, y, z, rx, ry, rz, .. } = &d.kind else { panic!("not a move") };
    let shown = [len(&h.app, x).unwrap(), len(&h.app, y).unwrap(), len(&h.app, z).unwrap()];
    let want = [9.729024, 9.95222, 10.327918];
    assert!((Vec3::new(shown[0], shown[1], shown[2]) - Vec3::new(want[0], want[1], want[2])).len() < 1e-6, "translation {shown:?}");
    let angles = [ang(&h.app, rx).unwrap().to_degrees(), ang(&h.app, ry).unwrap().to_degrees(), ang(&h.app, rz).unwrap().to_degrees()];
    for a in angles {
        assert!((a - 10.0).abs() < 1e-3, "the stored turn shows as the angles it was made with, not {angles:?}");
    }
    assert!(d.extra.get("axis").is_none() && d.extra.get("angle").is_none(), "the turn is shown, not hidden in extra");
    // Re-saving the dialog unchanged must not move the body.
    for (command, params) in crate::dialogs::apply_commands(&h.app, &d).unwrap() {
        h.app.run(&command, params).unwrap();
    }
    let after = bounds(&h);
    assert!(
        (after.min - before.min).len() < 1e-4 && (after.max - before.max).len() < 1e-4,
        "re-saving unchanged must not move: {before:?} -> {after:?}"
    );
    // Zeroing every value brings the body back where it was before the move.
    let mut d = crate::dialogs::for_feature(&h.app, id, None).unwrap();
    if let Kind::Move { x, y, z, rx, ry, rz, .. } = &mut d.kind {
        *x = "0".into();
        *y = "0".into();
        *z = "0".into();
        *rx = "0 deg".into();
        *ry = "0 deg".into();
        *rz = "0 deg".into();
    }
    for (command, params) in crate::dialogs::apply_commands(&h.app, &d).unwrap() {
        h.app.run(&command, params).unwrap();
    }
    let back = bounds(&h);
    assert!(
        (back.min - Vec3::ZERO).len() < 1e-6 && (back.max - Vec3::new(20.0, 20.0, 20.0)).len() < 1e-6,
        "zeroed values must undo the move: {back:?}"
    );
}

/// A move about a world axis keeps its angle expression in that axis's field.
#[test]
fn editing_a_move_about_a_world_axis_keeps_the_angle_expression() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20})).unwrap();
    let id = h
        .app
        .run("solid.move", json!({"bodies": ["Body1"], "translate": ["0", "0", "5 mm"], "axis": [0.0, 0.0, 1.0], "angle": "45 deg"}))
        .unwrap()["feature"]
        .as_u64()
        .unwrap();
    let d = crate::dialogs::for_feature(&h.app, id, None).unwrap();
    let Kind::Move { rx, ry, rz, .. } = &d.kind else { panic!("not a move") };
    assert_eq!((rx.as_str(), ry.as_str(), rz.as_str()), ("0 deg", "0 deg", "45 deg"));
}

/// A design with a component `Part` holding a 20 mm cube at the origin, seen from above, with no
/// dialog open. Returns the harness and the part's placement.
fn part_harness() -> (crate::scenario::Harness, u64) {
    let mut h = crate::scenario::Harness::new();
    let made = h.app.run("component.create", json!({"name": "Part", "grounded": false})).unwrap();
    h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20, "body_name": "Peg"})).unwrap();
    h.app.run("component.activate", json!({"component": "root"})).unwrap();
    h.app.ui.show_browser = false;
    h.app.ui.show_timeline = false;
    h.app.set_origin_shown(false);
    h.step(&json!({"call": "ui.view", "params": {"view": "top"}})).unwrap();
    (h, made["occurrence"].as_u64().unwrap())
}

/// Opens Move/Copy with the viewport in component (`components`) or body pick mode, Create Copy
/// ticked or not, then clicks the top of the cube.
fn click_cube_in_move(components: bool, copy: bool) -> (crate::scenario::Harness, u64) {
    let (mut h, occurrence) = part_harness();
    h.app.start("solid.move");
    h.settle();
    h.app.ui.pick_components = components;
    h.app.ui.pick_bodies = !components;
    if copy {
        h.app.dialog.as_mut().unwrap().extra.insert("copy".into(), json!(true));
    }
    h.step(&json!({"click": {"world": [10, 10, 20]}})).unwrap();
    (h, occurrence)
}

#[test]
fn a_component_picked_with_create_copy_is_moved_as_a_component_copy() {
    let (h, occurrence) = click_cube_in_move(true, true);
    let d = h.app.dialog.as_ref().unwrap();
    assert_eq!(d.inputs[0].items, vec![Sel::Occurrence { path: vec![occurrence] }]);
    let p = crate::dialogs_move::params(&h.app, d).unwrap();
    assert_eq!(p["occurrences"], json!([[occurrence]]));
    assert_eq!(p["bodies"], json!([]));
    assert_eq!(p["copy"], json!(true));
}

#[test]
fn a_body_picked_with_create_copy_is_still_a_body_copy() {
    let (h, _) = click_cube_in_move(false, true);
    let d = h.app.dialog.as_ref().unwrap();
    assert!(matches!(d.inputs[0].items.as_slice(), [Sel::Body { .. }]), "{:?}", d.inputs[0].items);
    let p = crate::dialogs_move::params(&h.app, d).unwrap();
    assert_eq!(p["bodies"].as_array().map(Vec::len), Some(1));
    assert!(p.get("occurrences").is_none());
    assert_eq!(p["copy"], json!(true));
}

#[test]
fn a_component_picked_without_create_copy_moves_its_bodies_as_before() {
    let (h, occurrence) = click_cube_in_move(true, false);
    let d = h.app.dialog.as_ref().unwrap();
    assert_eq!(d.inputs[0].items, vec![Sel::Occurrence { path: vec![occurrence] }]);
    let p = crate::dialogs_move::params(&h.app, d).unwrap();
    assert_eq!(p["bodies"].as_array().map(Vec::len), Some(1), "the component's body moves");
    assert!(p.get("occurrences").is_none());
    assert!(p.get("copy").is_none());
}

#[test]
fn a_component_picked_before_create_copy_is_ticked_is_copied_as_a_component() {
    let (mut h, occurrence) = click_cube_in_move(true, false);
    h.app.dialog.as_mut().unwrap().extra.insert("copy".into(), json!(true));
    let p = crate::dialogs_move::params(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(p["occurrences"], json!([[occurrence]]));
    let placements = h.app.session.doc.occurrences.len();
    h.app.session.execute("solid.move", &p).unwrap();
    assert_eq!(h.app.session.doc.occurrences.len(), placements + 1, "OK copies the component");
}

#[test]
fn editing_a_move_never_takes_a_component_pick() {
    // An existing move of another body is edited, and the component is clicked in component mode.
    let (mut h, _) = part_harness();
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [100, 0, 0], "body_name": "Plate"})).unwrap();
    let moved = h.app.run("solid.move", json!({"bodies": ["Plate"], "translate": [0, 0, 5]})).unwrap()["feature"].as_u64().unwrap();
    h.app.dialog = Some(crate::dialogs::for_feature(&h.app, moved, None).unwrap());
    h.settle();
    h.app.ui.pick_components = true;
    h.app.ui.pick_bodies = false;
    h.step(&json!({"click": {"world": [10, 10, 20]}})).unwrap();
    let items = h.app.dialog.as_ref().unwrap().inputs[0].items.clone();
    assert!(items.iter().any(|s| matches!(s, Sel::Body { name } if name == "Peg")), "the click picks the body: {items:?}");
    assert!(!items.iter().any(|s| matches!(s, Sel::Occurrence { .. })), "an edited move's picks stay bodies: {items:?}");
}

#[test]
fn selected_components_are_copied_when_create_copy_is_ticked_after_them() {
    // One selected placement starts the Browser's own component move, so two are selected here.
    let (mut h, first) = part_harness();
    let second = h.app.run("occurrence.copy", json!({"occurrence": first, "translate": [50, 0, 0]})).unwrap()["occurrence"].as_u64().unwrap();
    h.app.session.selection = vec![Sel::Occurrence { path: vec![first] }, Sel::Occurrence { path: vec![second] }];
    h.app.start("solid.move");
    let picked = vec![Sel::Occurrence { path: vec![first] }, Sel::Occurrence { path: vec![second] }];
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items, picked);
    let p = crate::dialogs_move::params(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(p["bodies"].as_array().map(Vec::len), Some(2), "without a copy the components' bodies move");
    assert!(p.get("occurrences").is_none());
    h.app.dialog.as_mut().unwrap().extra.insert("copy".into(), json!(true));
    let p = crate::dialogs_move::params(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(p["occurrences"], json!([[first], [second]]));
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

/// A woken point hidden behind a face is not picked, like a vertex there: the click takes the
/// face in front of it.
#[test]
fn a_woken_point_behind_a_face_is_not_picked() {
    let mut h = holed_plate_picking_from();
    h.step(&json!({ "move": { "world": [49, 20, 10] } })).unwrap();
    let hidden = Vec3::new(20.0, 20.0, 0.0);
    h.app.viewport.woken.points.push(solidcraft_engine::snap::SnapPoint {
        at: hidden,
        kind: solidcraft_engine::snap::SnapKind::Middle,
        body: String::new(),
    });
    h.step(&json!({ "click": { "world": [20, 20, 0] } })).unwrap();
    let from = &h.app.dialog.as_ref().unwrap().inputs[1].items;
    assert!(!matches!(from.as_slice(), [Sel::Vertex { point, .. }] if point.dist(hidden) < 1e-6), "{from:?}");
}

/// A woken point of a body hidden since it woke is neither shown nor picked.
#[test]
fn woken_points_of_a_hidden_body_sleep() {
    let mut h = holed_plate_picking_from();
    h.step(&json!({ "move": { "world": [49, 20, 10] } })).unwrap();
    assert!(!crate::snap_wake::shown(&h.app).is_empty(), "{:?}", h.app.viewport.woken);
    h.app.session.visibility.hidden_bodies.push("Body1".into());
    assert!(crate::snap_wake::shown(&h.app).is_empty(), "{:?}", h.app.viewport.woken);
}

/// Move's points don't take a sketch line, but hovering one wakes its middle, which they take.
#[test]
fn a_sketch_line_wakes_its_middle_for_move() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 10})).unwrap();
    h.app.run("sketch.create", json!({"plane": "XY", "name": "S"})).unwrap();
    h.app.run("sketch.line", json!({"points": [[60, 0], [80, 0]], "ids": ["l"]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.cam.distance = 200.0;
    h.settle();
    h.app.start("solid.move");
    let d = h.app.dialog.as_mut().unwrap();
    d.kind = kind(POINT_TO_POINT, &h.app.session.doc);
    d.inputs = inputs(POINT_TO_POINT);
    d.inputs[0].items = vec![Sel::Body { name: "Body1".into() }];
    d.active = 1;
    h.step(&json!({ "move": { "world": [65, 0, 0] } })).unwrap();
    let middle = Vec3::new(70.0, 0.0, 0.0);
    assert!(
        h.app.viewport.woken.points.iter().any(|p| p.at.dist(middle) < 1e-6),
        "hover {:?} woken {:?}",
        h.app.viewport.hover,
        h.app.viewport.woken
    );
    h.step(&json!({ "click": { "world": [70, 0, 0] } })).unwrap();
    let from = &h.app.dialog.as_ref().unwrap().inputs[1].items;
    assert!(matches!(from.as_slice(), [Sel::Vertex { point, .. }] if point.dist(middle) < 1e-6), "{from:?}");
}

/// Crossing a face on the way from a rim to its centre keeps the centre awake: only another
/// edge or curve puts a rim's points to sleep.
#[test]
fn crossing_a_face_keeps_a_rims_centre_awake() {
    let mut h = holed_plate_picking_from();
    h.step(&json!({ "move": { "world": [49, 20, 10] } })).unwrap();
    h.step(&json!({ "move": { "world": [53, 24, 10] } })).unwrap();
    assert!(matches!(h.app.viewport.hover, Some(crate::viewport::Hit::Face { .. })), "{:?}", h.app.viewport.hover);
    let centre = Vec3::new(45.0, 20.0, 10.0);
    assert!(h.app.viewport.woken.points.iter().any(|p| p.at.dist(centre) < 1e-6), "{:?}", h.app.viewport.woken);
    h.step(&json!({ "click": { "world": [45, 20, 10] } })).unwrap();
    let from = &h.app.dialog.as_ref().unwrap().inputs[1].items;
    assert!(matches!(from.as_slice(), [Sel::Vertex { point, .. }] if point.dist(centre) < 1e-6), "{from:?}");
}

/// Crossing a second face of the rim's body replaces all the first face's points; the rim's
/// stay.
#[test]
fn a_second_crossed_face_replaces_the_firsts_points() {
    let mut h = holed_plate_picking_from();
    h.step(&json!({ "move": { "world": [49, 20, 10] } })).unwrap();
    assert!(matches!(h.app.viewport.woken.item, Some(Sel::Edge { .. })), "{:?}", h.app.viewport.woken);
    let rim_centre = Vec3::new(45.0, 20.0, 10.0);
    let st = h.app.session.world_state().unwrap();
    let b = st.body("Body1").unwrap();
    let tol = (b.body.size() * 2e-3).max(1e-3);
    let flat: Vec<_> = b.body.faces(tol).unwrap().into_iter().filter(|f| f.plane_normal.is_some()).collect();
    let top = flat.iter().find(|f| (f.centroid.z - 10.0).abs() < 1e-6).unwrap();
    let side = flat.iter().find(|f| f.centroid.x.abs() < 1e-6).unwrap();
    let has = |h: &crate::scenario::Harness, at: Vec3| h.app.viewport.woken.points.iter().any(|p| p.at.dist(at) < 1e-6);
    // A face's snap centre is its area centre from the snap's own tessellation, so it is only
    // near the centroid of `faces` (a holed face differs by a few thousandths).
    let has_centre = |h: &crate::scenario::Harness, at: Vec3| {
        h.app.viewport.woken.points.iter().any(|p| p.kind == solidcraft_engine::snap::SnapKind::FaceCentre && p.at.dist(at) < 0.05)
    };
    for (face, gone) in [(top, None), (side, Some(top.centroid))] {
        h.app.viewport.hover = Some(crate::viewport::Hit::Face { body: "Body1".into(), index: face.index, point: face.centroid });
        crate::snap_wake::update(&mut h.app, &[]);
        assert!(has(&h, rim_centre), "the rim's centre stays: {:?}", h.app.viewport.woken);
        assert!(has_centre(&h, face.centroid), "the face's centre wakes: {:?}", h.app.viewport.woken);
        if let Some(at) = gone {
            assert!(!has_centre(&h, at), "the first face's centre sleeps: {:?}", h.app.viewport.woken);
        }
    }
}

/// A face of another body is not on the way to the rim's centre: it sleeps the rim's points
/// and wakes its own centre.
#[test]
fn a_face_of_another_body_sleeps_a_rims_points() {
    let mut h = holed_plate_picking_from();
    h.app.run("solid.box", json!({"corner": [70, 0, 0], "length": 20, "width": 20, "height": 10})).unwrap();
    h.settle();
    h.step(&json!({ "move": { "world": [49, 20, 10] } })).unwrap();
    assert!(matches!(h.app.viewport.woken.item, Some(Sel::Edge { .. })), "{:?}", h.app.viewport.woken);
    h.step(&json!({ "move": { "world": [76, 6, 10] } })).unwrap();
    assert!(matches!(&h.app.viewport.woken.item, Some(Sel::Face { body, .. }) if body != "Body1"), "{:?}", h.app.viewport.woken);
    assert!(h.app.viewport.woken.points.iter().any(|p| p.at.dist(Vec3::new(80.0, 10.0, 10.0)) < 1e-6), "{:?}", h.app.viewport.woken);
}

/// A woken point of a sketch hidden since it woke is neither shown nor picked.
#[test]
fn woken_points_of_a_hidden_sketch_sleep() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 10})).unwrap();
    h.app.run("sketch.create", json!({"plane": "XY", "name": "S"})).unwrap();
    h.app.run("sketch.line", json!({"points": [[60, 0], [80, 0]], "ids": ["l"]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.cam.distance = 200.0;
    h.settle();
    h.app.start("solid.move");
    let d = h.app.dialog.as_mut().unwrap();
    d.kind = kind(POINT_TO_POINT, &h.app.session.doc);
    d.inputs = inputs(POINT_TO_POINT);
    d.inputs[0].items = vec![Sel::Body { name: "Body1".into() }];
    d.active = 1;
    h.step(&json!({ "move": { "world": [65, 0, 0] } })).unwrap();
    let Some(Sel::ExternalSketchCurve { sketch, .. }) = h.app.viewport.woken.item.clone() else { panic!("{:?}", h.app.viewport.woken) };
    assert!(!crate::snap_wake::shown(&h.app).is_empty());
    h.app.session.visibility.hidden_sketches.push(sketch);
    assert!(crate::snap_wake::shown(&h.app).is_empty(), "{:?}", h.app.viewport.woken);
}

/// An edit sleeps the woken points, even with nothing under the pointer: a point the design no
/// longer offers is not shown or picked.
#[test]
fn an_edit_sleeps_the_woken_points() {
    let mut h = holed_plate_picking_from();
    h.step(&json!({ "move": { "world": [49, 20, 10] } })).unwrap();
    assert!(!h.app.viewport.woken.points.is_empty(), "{:?}", h.app.viewport.woken);
    h.app.run("solid.box", json!({"corner": [200, 200, 0], "length": 5, "width": 5, "height": 5})).unwrap();
    // Nothing under the pointer now: the points woken before the edit sleep all the same.
    h.app.viewport.hover = None;
    crate::snap_wake::update(&mut h.app, &[]);
    assert!(h.app.viewport.woken.points.is_empty(), "{:?}", h.app.viewport.woken);
}

/// A move dialog with a sketch line off the box: its middle is woken by hovering the line.
fn box_and_sketch_line(point: Option<[f64; 2]>) -> crate::scenario::Harness {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 10})).unwrap();
    h.app.run("sketch.create", json!({"plane": "XY", "name": "S"})).unwrap();
    h.app.run("sketch.line", json!({"points": [[60, 0], [80, 0]], "ids": ["l"]})).unwrap();
    if let Some(p) = point {
        h.app.run("sketch.point", json!({"point": p, "id": "q"})).unwrap();
    }
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.cam.distance = 200.0;
    h.settle();
    h
}

/// Move's points don't take a sketch point: one beside a line's woken middle no longer hides
/// the middle, which a click there picks.
#[test]
fn a_point_the_input_doesnt_take_leaves_the_woken_middle() {
    let mut h = box_and_sketch_line(Some([70.3, 0.0]));
    h.app.start("solid.move");
    let d = h.app.dialog.as_mut().unwrap();
    d.kind = kind(POINT_TO_POINT, &h.app.session.doc);
    d.inputs = inputs(POINT_TO_POINT);
    d.inputs[0].items = vec![Sel::Body { name: "Body1".into() }];
    d.active = 1;
    h.step(&json!({ "move": { "world": [65, 0, 0] } })).unwrap();
    let middle = Vec3::new(70.0, 0.0, 0.0);
    assert!(h.app.viewport.woken.points.iter().any(|p| p.at.dist(middle) < 1e-6), "{:?}", h.app.viewport.woken);
    h.step(&json!({ "click": { "world": [70.2, 0, 0] } })).unwrap();
    let from = &h.app.dialog.as_ref().unwrap().inputs[1].items;
    assert!(matches!(from.as_slice(), [Sel::Vertex { point, .. }] if point.dist(middle) < 1e-6), "{from:?}");
}

/// Align takes sketch points: a sketch point on a line's woken middle stays the sketch point.
#[test]
fn a_sketch_point_on_a_woken_middle_stays_the_sketch_point() {
    let mut h = box_and_sketch_line(Some([70.0, 0.0]));
    h.app.start("solid.align");
    {
        let d = h.app.dialog.as_mut().unwrap();
        d.inputs[0].items = vec![Sel::Body { name: "Body1".into() }];
        d.active = 1;
    }
    h.step(&json!({ "move": { "world": [65, 0, 0] } })).unwrap();
    assert!(h.app.viewport.woken.points.iter().any(|p| p.at.dist(Vec3::new(70.0, 0.0, 0.0)) < 1e-6), "{:?}", h.app.viewport.woken);
    h.step(&json!({ "click": { "world": [70, 0, 0] } })).unwrap();
    let from = &h.app.dialog.as_ref().unwrap().inputs[1].items;
    assert!(matches!(from.as_slice(), [Sel::SketchPoint { .. }]), "{from:?}");
}

/// On a body the Move dialog has already moved (30 mm along X by Translate, then Point to Point),
/// the woken points show and are picked where the preview draws the body, not where the model
/// has it; a click on one is kept where Point to Point starts from, as a pick of the body is.
#[test]
fn a_moved_bodys_woken_points_show_where_it_is_drawn() {
    let mut h = holed_plate_picking_from();
    let mut d = h.app.dialog.take().unwrap();
    d.kind = kind(TRANSLATE, &h.app.session.doc);
    d.inputs = inputs(TRANSLATE);
    d.inputs[0].items = vec![Sel::Body { name: "Body1".into() }];
    if let Kind::Move { x, .. } = &mut d.kind {
        *x = "30 mm".into();
    }
    switch_type(&h.app, &mut d, POINT_TO_POINT);
    h.app.dialog = Some(d);
    let (model, drawn) = (Vec3::new(45.0, 20.0, 10.0), Vec3::new(75.0, 20.0, 10.0));
    h.step(&json!({ "move": { "world": [79, 20, 10] } })).unwrap();
    assert!(h.app.viewport.woken.points.iter().any(|p| p.at.dist(model) < 1e-6), "the moved rim wakes the centre: {:?}", h.app.viewport.woken);
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    let screen = |p: Vec3| proj.to_screen(p).unwrap();
    let hits = crate::snap_wake::hits(&h.app, &proj, screen(drawn));
    assert!(matches!(hits.first(), Some((_, crate::viewport::Hit::Vertex { point, .. }, _)) if point.dist(drawn) < 1e-6), "{hits:?}");
    assert!(crate::snap_wake::hits(&h.app, &proj, screen(model)).is_empty(), "nothing where the model has the plate");
    h.step(&json!({ "click": { "world": [75, 20, 10] } })).unwrap();
    let from = &h.app.dialog.as_ref().unwrap().inputs[1].items;
    assert!(matches!(from.as_slice(), [Sel::Vertex { body, point }] if body == "Body1" && point.dist(drawn) < 1e-6), "{from:?}");
}
/// A vertex of another body nearer the pointer than a woken point is the one hovered and picked,
/// even within the few pixels where a woken point wins over the edge it lies on.
#[test]
fn another_bodys_vertex_nearer_than_a_woken_point_wins() {
    let mut h = holed_plate_picking_from();
    h.step(&json!({ "move": { "world": [49, 20, 10] } })).unwrap();
    let centre = Vec3::new(45.0, 20.0, 10.0);
    assert!(woken_has(&h, centre), "the rim wakes its centre: {:?}", h.app.viewport.woken);
    // A small box in the hole, the corner of its top two pixels from the woken centre on screen.
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    let screen = |p: Vec3| proj.to_screen(p).unwrap();
    let per_mm = f64::from(screen(centre + Vec3::X).distance(screen(centre)));
    let corner = centre + Vec3::X * (2.0 / per_mm);
    assert!(f64::from(screen(corner).distance(screen(centre))) < f64::from(crate::snap_wake::ON), "within a woken point's own reach");
    h.app.run("solid.box", json!({"length": 1, "width": 1, "height": 1, "corner": [corner.x, corner.y, 9], "operation": "new"})).unwrap();
    h.step(&json!({ "move": { "world": [corner.x, corner.y, corner.z] } })).unwrap();
    assert!(
        matches!(&h.app.viewport.hover, Some(crate::viewport::Hit::Vertex { body, point }) if body == "Body2" && point.dist(corner) < 1e-6),
        "{:?}",
        h.app.viewport.hover
    );
    h.step(&json!({ "click": { "world": [corner.x, corner.y, corner.z] } })).unwrap();
    let from = &h.app.dialog.as_ref().unwrap().inputs[1].items;
    assert!(matches!(from.as_slice(), [Sel::Vertex { body, point }] if body == "Body2" && point.dist(corner) < 1e-6), "{from:?}");
}

/// Snap points are on by default; with the preference off, hovering a rim wakes nothing and a
/// click in the hole's middle picks nothing there, as before (#5912).
#[test]
fn snap_points_are_on_by_default_and_sleep_with_the_preference_off() {
    assert!(crate::prefs::Prefs::default().snap_points, "on by default");
    let mut h = holed_plate_picking_from();
    h.app.preferences.snap_points = false;
    h.step(&json!({ "move": { "world": [49, 20, 10] } })).unwrap();
    assert!(h.app.viewport.woken.points.is_empty(), "{:?}", h.app.viewport.woken);
    h.step(&json!({ "click": { "world": [45, 20, 10] } })).unwrap();
    let from = &h.app.dialog.as_ref().unwrap().inputs[1].items;
    let centre = Vec3::new(45.0, 20.0, 10.0);
    assert!(!from.iter().any(|s| matches!(s, Sel::Vertex { point, .. } if point.dist(centre) < 1e-6)), "{from:?}");
}

/// An input that doesn't take snap points (Rotate's axis) wakes none.
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

/// Construct Point takes snap points (#11228): a hole's centre, clicked in its empty middle,
/// is saved as the centre of the hole's rim, not as the nearest vertex.
#[test]
fn construct_point_saves_a_snapped_centre_as_a_reference() {
    let mut h = holed_plate_picking_from();
    h.app.dialog = None;
    h.app.start("construct.point.vertex");
    h.step(&json!({ "move": { "world": [49, 20, 10] } })).unwrap();
    h.step(&json!({ "click": { "world": [45, 20, 10] } })).unwrap();
    h.step(&json!({ "click": { "handle": "dialog:OK" } })).unwrap();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog.as_ref().and_then(|d| d.error.clone()));
    let f = h.app.session.doc.features.last().unwrap();
    let saved = serde_json::to_value(&f.kind).unwrap();
    assert_eq!(saved["def"]["point"]["type"], "snap", "{saved}");
    assert_eq!(saved["def"]["point"]["kind"], "centre", "{saved}");
    let at = solidcraft_engine::view::construction_points(&h.app.session).into_iter().map(|p| p.2).next().unwrap();
    assert!(at.dist(Vec3::new(45.0, 20.0, 10.0)) < 1e-6, "{at:?}");
}

/// Offset Plane's Through point takes an edge's middle (#11228); reopened, OK keeps the saved
/// reference, and with the point removed the plane goes back to its distance.
#[test]
fn offset_plane_through_a_snapped_middle_keeps_it_when_edited() {
    use solidcraft_engine::snap::{SnapKind, SnapPoint};
    let mut h = holed_plate_picking_from();
    h.app.dialog = None;
    h.app.start("construct.plane.offset");
    h.frames(1);
    let middle = Vec3::new(30.0, 0.0, 10.0);
    let edge = Sel::Edge { body: "Body1".into(), index: 0, point: Vec3::new(20.0, 0.0, 10.0) };
    h.app.viewport.snapped = vec![(edge, SnapPoint { at: middle, kind: SnapKind::Middle, body: "Body1".into() })];
    let d = h.app.dialog.as_mut().unwrap();
    d.inputs[0].items = vec![Sel::Plane { name: "XY".into() }];
    d.inputs[1].items = vec![Sel::Vertex { body: "Body1".into(), point: middle }];
    let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(commands[0].1["through"]["type"], "snap", "{:?}", commands);
    h.step(&json!({ "key": "Enter" })).unwrap();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog.as_ref().and_then(|d| d.error.clone()));
    h.frames(3);
    let (id, _, pl) = solidcraft_engine::view::construction_planes(&h.app.session).into_iter().next().expect("the plane");
    assert!((pl.origin.z - 10.0).abs() < 1e-6, "{pl:?}");
    let saved = serde_json::to_value(&h.app.session.doc.feature(id).unwrap().kind).unwrap();
    assert_eq!(saved["plane"]["point"]["kind"], "middle", "{saved}");
    // Reopened, nothing remembered: the saved reference is kept as it is.
    h.app.viewport.snapped.clear();
    let mut reopened = crate::dialogs::for_feature(&h.app, id, None).unwrap();
    let commands = crate::dialogs::apply_commands(&h.app, &reopened).unwrap();
    assert_eq!(commands[0].1["params"]["through"]["kind"], "middle", "{:?}", commands);
    reopened.inputs[1].items.clear();
    let commands = crate::dialogs::apply_commands(&h.app, &reopened).unwrap();
    assert!(commands[0].1["params"]["through"].is_null(), "{:?}", commands);
}

/// Whether a point is among the woken ones.
fn woken_has(h: &crate::scenario::Harness, p: Vec3) -> bool {
    h.app.viewport.woken.points.iter().any(|w| w.at.dist(p) < 1e-6)
}

/// The Move dialog on the 20 × 30 × 10 box with the triad drawn, and the screen point of a handle.
fn move_triad_fixture() -> crate::scenario::Harness {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"length": 20, "width": 30, "height": 10})).unwrap();
    h.app.start("solid.move");
    h.app.dialog.as_mut().unwrap().inputs[0].items = vec![Sel::Body { name: "Body1".into() }];
    h.frames(3);
    h
}

fn pointer_button(p: [f64; 2], pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: egui::pos2(p[0] as f32, p[1] as f32),
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    }
}

fn pointer_at(p: [f64; 2]) -> egui::Event {
    egui::Event::PointerMoved(egui::pos2(p[0] as f32, p[1] as f32))
}

fn box_label(h: &mut crate::scenario::Harness) -> Option<&'static str> {
    h.app.dialog.as_mut().and_then(|d| d.primary()).map(|p| p.0)
}

/// A press and release on an arrow, with no drag between, puts the canvas box on that arrow's value,
/// and moves nothing.
#[test]
fn a_click_on_an_arrow_puts_the_box_on_its_axis() {
    // The press and the release in frames of their own, and both in one frame (input queued while
    // the window is busy arrives together, #8265), there with the pointer then moved off the triad.
    for (handle, label, one_frame, off) in [
        ("y", "Y", false, false),
        ("z", "Z", false, false),
        ("x", "X", false, false),
        ("y", "Y", true, false),
        ("z", "Z", true, false),
        ("y", "Y", true, true),
    ] {
        let mut h = move_triad_fixture();
        assert_eq!(box_label(&mut h), Some("X"), "the box starts on X");
        let at = h.at(&json!({"gizmo": handle})).unwrap();
        h.frame_with(vec![pointer_at(at)]);
        if off {
            let away = h.at(&json!({"gizmo": "ball"})).map(|b| [b[0] - 3.0 * (at[0] - b[0]), b[1] - 3.0 * (at[1] - b[1])]).unwrap();
            h.frame_with(vec![pointer_button(at, true), pointer_button(at, false), pointer_at(away)]);
        } else if one_frame {
            h.frame_with(vec![pointer_button(at, true), pointer_button(at, false)]);
        } else {
            h.frame_with(vec![pointer_button(at, true)]);
            h.frame_with(vec![pointer_button(at, false)]);
        }
        h.frames(2);
        assert_eq!(box_label(&mut h), Some(label), "a click on the {label} arrow (one frame: {one_frame}, off: {off})");
        let Kind::Move { x, y, z, .. } = &h.app.dialog.as_ref().unwrap().kind else { panic!("not a move") };
        assert_eq!((x.as_str(), y.as_str(), z.as_str()), ("0 mm", "0 mm", "0 mm"), "a click moves nothing");
    }
}

/// Reopening a Move with a parametric origin keeps the origin's expressions and the angle.
#[test]
fn editing_a_move_keeps_its_parametric_origin() {
    for axis in [[0, 0, 1], [1, 1, 0]] {
        let mut s = Session::default();
        s.execute("parameters.change", &json!({"name": "pivot", "expression": "10 mm"})).unwrap();
        s.execute("solid.box", &json!({"length": 20, "width": 20, "height": 20, "body_name": "B"})).unwrap();
        let result = s.execute("solid.move", &json!({"bodies": ["B"], "origin": ["pivot", "10 mm", 0], "axis": axis, "angle": "90 deg"})).unwrap();
        let feature = s.doc.feature(result["feature"].as_u64().unwrap()).unwrap().kind.clone();
        let app = SolveApp::new(s, Services::default());
        let mut d = move_dialog(FREE);
        for_feature(&app.session.doc, &mut d, &feature, &solidcraft_engine::doc::IDENTITY);
        let p = params(&app, &d).unwrap();
        assert_eq!(p["origin"], json!(["pivot", "10 mm", "0"]));
        assert_eq!(p["angle"], "90 deg");
    }
}

/// Reopening and applying a Move of a body in a turned placement leaves the body where it was.
#[test]
fn editing_a_placed_body_move_preserves_its_world_transform() {
    let mut s = Session::default();
    let part = s.execute("component.create", &json!({"name": "Part", "grounded": false})).unwrap();
    s.execute("solid.box", &json!({"length": 20, "width": 10, "height": 8, "corner": [100,0,0], "body_name": "B"})).unwrap();
    s.execute("occurrence.move", &json!({"occurrence": part["occurrence"], "axis": [1,0,0], "angle": "90 deg", "translate": [10,20,30]})).unwrap();
    s.execute("component.activate", &json!({"component": 0})).unwrap();
    let result =
        s.execute("solid.move", &json!({"bodies": ["B"], "origin": ["120 mm", "16 mm", "35 mm"], "angle": "90 deg", "translate": [0,0,7]})).unwrap();
    let id = result["feature"].as_u64().unwrap();
    let before = s.world_state().unwrap().body("B").unwrap().mesh().bounds();
    let mut app = SolveApp::new(s, Services::default());
    let d = crate::dialogs::for_feature(&app, id, None).unwrap();
    for (command, params) in crate::dialogs::apply_commands(&app, &d).unwrap() {
        app.run(&command, params).unwrap();
    }
    let after = app.session.world_state().unwrap().body("B").unwrap().mesh().bounds();
    assert!(before.min.dist(after.min) < 1e-6 && before.max.dist(after.max) < 1e-6, "{before:?} -> {after:?}");
}

/// The rings stay on the world axes, so a turn dragged on one goes about the axis it shows, on
/// top of the turns already made. It went about that axis as the earlier turn had left it, when
/// the angles always applied about X, then Y, then Z. The body's centre stays where it is.
#[test]
fn a_ring_turns_about_the_axis_it_shows_after_another_turn() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"length": 20, "width": 30, "height": 10})).unwrap();
    let centre = Vec3::new(10.0, 15.0, 5.0);
    h.app.cam = h.app.cam.looking_from(Vec3::X);
    h.app.cam.target = centre;
    h.app.cam.distance = 180.0;
    h.app.run("select.set", json!({"items": [Sel::Body { name: "Body1".into() }]})).unwrap();
    h.app.start("solid.move");
    if let Some(Kind::Move { ry, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        *ry = "40 deg".into();
    }
    h.frames(3);
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    let radius = gizmo::ring_radius(&proj, centre).unwrap();
    // The scenario handle sits 60 degrees round the ring: turn it on by 35.
    let th = 95_f64.to_radians();
    let target = centre + (Vec3::Y * th.cos() + Vec3::Z * th.sin()) * radius;
    h.step(&json!({"drag": [{"handle": "move_rx"}, {"world": [target.x, target.y, target.z]}], "steps": 12})).unwrap();
    let want = gizmo::mul(&gizmo::rotation(Vec3::X, 35_f64.to_radians()), &gizmo::rotation(Vec3::Y, 40_f64.to_radians()));
    let mut d = h.app.dialog.clone().unwrap();
    assert_eq!(d.primary().map(|p| p.0), Some("X Angle"));
    let p = params(&h.app, &d).unwrap();
    let m = transform(&h.app, &p).unwrap();
    assert_turn(&m, &want);
    assert!((apply_point(&m, centre) - centre).len() < 1e-4, "{p}");
}

/// A Move dialog on the 20 × 30 × 10 box, seen from a corner so every ring has width on screen.
fn turning_box() -> (crate::scenario::Harness, Vec3) {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"length": 20, "width": 30, "height": 10})).unwrap();
    let centre = Vec3::new(10.0, 15.0, 5.0);
    h.app.cam = h.app.cam.looking_from(Vec3::new(1.0, 0.8, 0.6));
    h.app.cam.target = centre;
    h.app.cam.distance = 220.0;
    h.app.run("select.set", json!({"items": [Sel::Body { name: "Body1".into() }]})).unwrap();
    h.app.start("solid.move");
    h.frames(3);
    (h, centre)
}

/// Drag ring `ring` (0 to 2) of the triad on by `by` degrees.
fn turn_ring(h: &mut crate::scenario::Harness, ring: usize, by: f64) {
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    let t = triad(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    let grip = gizmo::ring_grip(&proj, &t, ring).unwrap();
    let [start, end] = [grip, grip + by].map(|a| gizmo::handle_point(["rx", "ry", "rz"][ring], Vec3::ZERO, a).unwrap());
    h.step(&json!({"drag": [{"world": [start.x, start.y, start.z]}, {"world": [end.x, end.y, end.z]}], "steps": 12})).unwrap();
    h.frames(2);
}

/// Type the three Free Move angles into the open dialog.
fn set_angles(h: &mut crate::scenario::Harness, a: [&str; 3]) {
    if let Some(Kind::Move { rx, ry, rz, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        [*rx, *ry, *rz] = a.map(String::from);
    }
    h.frames(3);
}

/// The three angles the open Move dialog shows.
fn shown_angles(h: &crate::scenario::Harness) -> [String; 3] {
    let Kind::Move { rx, ry, rz, .. } = &h.app.dialog.as_ref().unwrap().kind else { panic!("not a move") };
    [rx.clone(), ry.clone(), rz.clone()]
}

/// The whole move the dialog makes, in the world.
fn whole_move(h: &crate::scenario::Harness) -> Mat {
    let p = params(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    transform(&h.app, &p).unwrap()
}

/// Assert that `m` turns as `want` does (its rotation part only).
fn assert_turn(m: &Mat, want: &[[f64; 3]; 3]) {
    let rows = [0, 1, 2].map(|r| [m[0][r], m[1][r], m[2][r]]);
    for (got, want) in rows.iter().flatten().zip(want.iter().flatten()) {
        assert!((got - want).abs() < 1e-5, "{rows:?} != {want:?}");
    }
}

/// After a turn, the next ring's box reads what was dragged and the first angle stays. It read
/// "Y Angle 0 deg" while one set of angles in a fixed order described the combined turn: after a
/// quarter turn about Z, a turn about Y is all X in that form.
#[test]
fn the_second_angle_reads_what_was_dragged() {
    let rot = |a: Vec3, th: f64| gizmo::rotation(a, th.to_radians());
    let (mut h, centre) = turning_box();
    set_angles(&mut h, ["0 deg", "0 deg", "90 deg"]);
    turn_ring(&mut h, 1, 35.0);
    assert_eq!(shown_angles(&h), ["0 deg", "35 deg", "90 deg"]);
    let mut d = h.app.dialog.clone().unwrap();
    assert_eq!(d.primary().map(|p| (p.0, p.2.clone())), Some(("Y Angle", "35 deg".to_string())));
    let m = whole_move(&h);
    assert_turn(&m, &gizmo::mul(&rot(Vec3::Y, 35.0), &rot(Vec3::Z, 90.0)));
    assert!((apply_point(&m, centre) - centre).len() < 1e-4, "the body turns on the spot");
}

/// Going back to an angle with a later turn on top keeps every turn made and counts that angle
/// from zero, because no sum of angles makes the turn.
#[test]
fn going_back_to_an_earlier_ring_starts_its_angle_over_and_keeps_the_turns() {
    let rot = |a: Vec3, th: f64| gizmo::rotation(a, th.to_radians());
    let (mut h, centre) = turning_box();
    set_angles(&mut h, ["30 deg", "0 deg", "0 deg"]);
    turn_ring(&mut h, 1, 20.0);
    assert_eq!(shown_angles(&h), ["30 deg", "20 deg", "0 deg"]);
    turn_ring(&mut h, 0, 10.0);
    assert_eq!(shown_angles(&h), ["10 deg", "0 deg", "0 deg"]);
    let m = whole_move(&h);
    assert_turn(&m, &gizmo::mul(&gizmo::mul(&rot(Vec3::X, 10.0), &rot(Vec3::Y, 20.0)), &rot(Vec3::X, 30.0)));
    assert!((apply_point(&m, centre) - centre).len() < 1e-4);
}

/// A dragged angle stays within one turn: a full turn is back at 0, not 360 (or -580).
#[test]
fn a_dragged_angle_stays_within_one_turn() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 20, "width": 20, "height": 20})).unwrap();
    let app = SolveApp::new(s, Services::default());
    for (from, by, want) in [
        ("0 deg", 360.0, "0 deg"),
        ("355 deg", 5.0, "0 deg"),
        ("0 deg", -360.0, "0 deg"),
        ("0 deg", 380.0, "20 deg"),
        ("-220 deg", -360.0, "-220 deg"),
        ("-300 deg", -280.0, "-220 deg"),
        ("0 deg", 355.0, "355 deg"),
    ] {
        let mut d = move_dialog(FREE);
        if let Kind::Move { rx, .. } = &mut d.kind {
            *rx = from.into();
        }
        let change = Change::Rotate { axis: 0, angle: f64::to_radians(by) };
        drag(&app, &mut d, crate::gizmo::Drag { handle: crate::gizmo::Handle::Ring(0), change, started: true, done: true });
        let Kind::Move { rx, .. } = &d.kind else { panic!("not a move") };
        assert_eq!(rx, want, "{from} turned by {by}");
    }
}

/// A ring dragged out and back to where it started is back at its angle: the snapped angle
/// reaches exactly zero on the way back. A click on a ring changes nothing.
#[test]
fn a_ring_dragged_back_to_its_start_is_back_at_its_angle() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 20, "width": 20, "height": 20})).unwrap();
    let app = SolveApp::new(s, Services::default());
    let rx_of = |d: &Dialog| match &d.kind {
        Kind::Move { rx, .. } => rx.clone(),
        _ => panic!("not a move"),
    };
    let ring = |d: &mut Dialog, by: f64, started: bool, done: bool| {
        let change = Change::Rotate { axis: 0, angle: f64::to_radians(by) };
        drag(&app, d, crate::gizmo::Drag { handle: crate::gizmo::Handle::Ring(0), change, started, done });
    };
    let mut d = move_dialog(FREE);
    if let Kind::Move { rx, .. } = &mut d.kind {
        *rx = "12 deg".into();
    }
    ring(&mut d, 10.0, true, false);
    assert_eq!(rx_of(&d), "22 deg");
    ring(&mut d, 5.0, false, false);
    assert_eq!(rx_of(&d), "17 deg");
    ring(&mut d, 0.0, false, true);
    assert_eq!(rx_of(&d), "12 deg", "back where the drag began");

    if let Kind::Move { rx, .. } = &mut d.kind {
        *rx = "12.50 deg".into();
    }
    ring(&mut d, 0.0, true, true);
    assert_eq!(rx_of(&d), "12.50 deg", "a click leaves the typed angle as it is");
}
