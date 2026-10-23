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
    d.kind = kind(mode);
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
    h.app.ui.show_origin = true;
    h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20})).unwrap();
    h.app.run("solid.move", json!({"bodies": ["Body1"], "translate": ["30 mm", "20 mm", "0 mm"]})).unwrap();
    h.app.cam.distance = 200.0;
    h.settle();
    let hi = h.app.session.model.state().body("Body1").unwrap().mesh().bounds().max;
    h.app.start("solid.move");
    {
        let d = h.app.dialog.as_mut().unwrap();
        d.kind = kind(POINT_TO_POINT);
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

/// A locked axis keeps its value through a triad drag; the free axes follow (a viewport
/// constraint only, the command parameters stay the same).
#[test]
fn a_locked_axis_ignores_the_triad_drag() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 20, "width": 20, "height": 20})).unwrap();
    let app = SolveApp::new(s, Services::default());
    let mut d = move_dialog(FREE);
    assert!(has_locks(&d.kind));
    assert!(!has_locks(&kind(ROTATE)), "Rotate has no dragged translation to lock");
    d.extra.insert("lock_y".into(), json!(true));
    d.extra.insert("lock_z".into(), json!(true));
    let step = |d: &mut Dialog, started: bool, done: bool| {
        let g = Drag { handle: gizmo::Handle::Plane(2), change: Change::Translate(Vec3::new(5.0, 7.0, 9.0)), started, done };
        drag(&app, d, g);
    };
    step(&mut d, true, false);
    step(&mut d, false, true);
    let Kind::Move { x, y, z, .. } = &d.kind else { panic!("not a move") };
    assert_eq!((x.as_str(), y.as_str(), z.as_str()), ("5 mm", "0 mm", "0 mm"), "only X follows the drag");
    assert_eq!(params(&app, &d).unwrap()["translate"], json!(["5 mm", "0 mm", "0 mm"]));
}

/// Each distance row ends in a lock toggle; clicking it flips that axis's lock.
#[test]
fn the_lock_toggle_at_the_end_of_a_distance_row_flips_its_axis() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20, "body_name": "Block"})).unwrap();
    h.app.run("select.set", json!({"items": [{"type": "body", "name": "Block"}]})).unwrap();
    h.app.start("solid.move");
    h.frames(4);
    let locked =
        |h: &crate::scenario::Harness, key: &str| h.app.dialog.as_ref().and_then(|d| d.extra.get(key)).and_then(Value::as_bool).unwrap_or(false);
    for axis in ["x", "y", "z"] {
        let at = crate::scenario::handle_at(&format!("move:lock:{axis}")).expect("each distance row has a lock toggle");
        assert!(!locked(&h, &format!("lock_{axis}")));
        h.call("ui.click", json!({"x": at.x, "y": at.y}));
        assert!(locked(&h, &format!("lock_{axis}")), "{axis} locked after a click");
        let at = crate::scenario::handle_at(&format!("move:lock:{axis}")).unwrap();
        h.call("ui.click", json!({"x": at.x, "y": at.y}));
        assert!(!locked(&h, &format!("lock_{axis}")), "{axis} free after a second click");
    }
}
