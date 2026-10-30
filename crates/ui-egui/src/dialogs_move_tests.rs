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
    let bounds = |h: &crate::scenario::Harness| h.app.session.world_state().body("Body1").unwrap().mesh().bounds();
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
        let translation: [String; 3] = serde_json::from_value(p["translate"].clone()).unwrap();
        let translation = translation.map(|v| len(&h.app, &v).unwrap());
        let moved_origin = gizmo::apply(&gizmo::rotation(axis, actual), origin) + Vec3::new(translation[0], translation[1], translation[2]);
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

/// Floating Move values hand Tab to the dialog, whose fields retain forward/reverse editing.
#[test]
fn dialog_tab_move_enters_the_dialog_instead_of_cycling_canvas_axes() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20})).unwrap();
    h.settle();
    h.app.run("select.set", json!({"items": [Sel::Body { name: "Body1".into() }]})).unwrap();
    h.app.start("solid.move");
    h.frames(3);
    assert_eq!(h.app.dialog.as_mut().and_then(|d| d.primary()).map(|p| p.0), Some("X"));
    h.step(&json!({"key": "Tab"})).unwrap();
    h.frames(2);
    assert_eq!(h.app.dialog.as_mut().and_then(|d| d.primary()).map(|p| p.0), Some("X"), "Tab no longer cycles the floating axis");
    let focused = h.ctx.memory(|m| m.focused()).unwrap();
    assert!(h.app.viewport.dialog_rect.unwrap().contains(h.ctx.read_response(focused).unwrap().rect.center()));
    h.step(&json!({"click":{"handle":"dialog:Y Distance"}})).unwrap();
    let y_field = h.ctx.memory(|m| m.focused()).unwrap();
    h.step(&json!({"key": "Tab"})).unwrap();
    h.step(&json!({"key": "Tab", "shift": true})).unwrap();
    h.frames(2);
    assert_eq!(h.ctx.memory(|m| m.focused()), Some(y_field), "Shift+Tab returns to Y in the dialog");
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
    let before = corners(h.app.session.world_state().body("Body1").unwrap().mesh().bounds());
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
    let after = h.app.session.world_state().body("Body1").unwrap().mesh().bounds();
    let turn = |p: Vec3| Vec3::new(-p.y, p.x + 10.0, p.z);
    let mut want = solidcraft_engine::geom::Aabb3::EMPTY;
    for c in before {
        want.add(turn(c));
    }
    assert!((after.min - want.min).len() < 1e-6 && (after.max - want.max).len() < 1e-6, "{after:?} != {want:?}");
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
