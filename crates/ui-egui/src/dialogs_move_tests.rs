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

/// The rotation and the translation a solid.move's parameters make.
fn made_turn(app: &SolveApp, p: &serde_json::Value) -> ([[f64; 3]; 3], Vec3) {
    let v = |x: &serde_json::Value| x.as_f64().unwrap();
    let axis = Vec3::new(v(&p["axis"][0]), v(&p["axis"][1]), v(&p["axis"][2]));
    let t: [String; 3] = serde_json::from_value(p["translate"].clone()).unwrap();
    let t = t.map(|s| len(app, &s).unwrap());
    (gizmo::rotation(axis, ang(app, p["angle"].as_str().unwrap()).unwrap()), Vec3::new(t[0], t[1], t[2]))
}

/// Fixed, the rings stay on the world axes, so a turn dragged on one goes about the axis it shows,
/// on top of the turns already made, whichever came first. The body's centre stays where it is.
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
    h.app.dialog.as_mut().unwrap().extra.insert("fixed".into(), json!(true));
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
    let (got, translate) = made_turn(&h.app, &p);
    for (got, want) in got.iter().flatten().zip(want.iter().flatten()) {
        assert!((got - want).abs() < 1e-5, "{p}");
    }
    assert!((gizmo::apply(&got, centre) + translate - centre).len() < 1e-4, "{p}");
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

fn set_angles(h: &mut crate::scenario::Harness, a: [&str; 3]) {
    if let Some(Kind::Move { rx, ry, rz, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        [*rx, *ry, *rz] = a.map(String::from);
    }
    h.frames(3);
}

fn shown_angles(h: &crate::scenario::Harness) -> [String; 3] {
    let Kind::Move { rx, ry, rz, .. } = &h.app.dialog.as_ref().unwrap().kind else { panic!("not a move") };
    [rx.clone(), ry.clone(), rz.clone()]
}

/// The whole move the dialog makes, in the world.
fn whole_move(h: &crate::scenario::Harness) -> Mat {
    let d = h.app.dialog.as_ref().unwrap();
    let p = params(&h.app, d).unwrap();
    transform(&h.app, d, &p).unwrap()
}

fn assert_turn(m: &Mat, want: &[[f64; 3]; 3]) {
    for (got, want) in rows_of(m).iter().flatten().zip(want.iter().flatten()) {
        assert!((got - want).abs() < 1e-5, "{:?} != {want:?}", rows_of(m));
    }
}

/// After a turn, the next ring's box reads what was dragged and the first angle stays, in both
/// frames. It read "Y Angle 0 deg" when one set of angles described the combined turn: after a
/// quarter turn about Z, a turn about Y is all X in that form.
#[test]
fn the_second_angle_reads_what_was_dragged() {
    let rot = |a: Vec3, th: f64| gizmo::rotation(a, th.to_radians());
    for fixed in [false, true] {
        let (mut h, centre) = turning_box();
        h.app.dialog.as_mut().unwrap().extra.insert("fixed".into(), json!(fixed));
        set_angles(&mut h, ["0 deg", "0 deg", "90 deg"]);
        turn_ring(&mut h, 1, 35.0);
        assert_eq!(shown_angles(&h), ["0 deg", "35 deg", "90 deg"], "fixed {fixed}");
        let mut d = h.app.dialog.clone().unwrap();
        assert_eq!(d.primary().map(|p| (p.0, p.2.clone())), Some(("Y Angle", "35 deg".to_string())), "fixed {fixed}");
        let want = if fixed { gizmo::mul(&rot(Vec3::Y, 35.0), &rot(Vec3::Z, 90.0)) } else { gizmo::mul(&rot(Vec3::Z, 90.0), &rot(Vec3::Y, 35.0)) };
        let m = whole_move(&h);
        assert_turn(&m, &want);
        assert!((apply_point(&m, centre) - centre).len() < 1e-4, "the body turns on the spot");
    }
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
    assert_turn(&m, &gizmo::mul(&gizmo::mul(&rot(Vec3::X, 30.0), &rot(Vec3::Y, 20.0)), &rot(Vec3::X, 10.0)));
    assert!((apply_point(&m, centre) - centre).len() < 1e-4);
}

/// Local, the triad turns with the body, so its arrows and rings stay on the body; Fixed, they
/// stay on the world. A dragged arrow changes its own distance alone, and the box reads it.
#[test]
fn the_triad_turns_with_the_body_unless_fixed() {
    for (fixed, way) in [(false, Vec3::Y), (true, Vec3::X)] {
        let (mut h, centre) = turning_box();
        h.app.dialog.as_mut().unwrap().extra.insert("fixed".into(), json!(fixed));
        set_angles(&mut h, ["0 deg", "0 deg", "90 deg"]);
        let t = triad(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        assert!((gizmo::axis(&t.axes, 0) - way).len() < 1e-9, "fixed {fixed}: {:?}", t.axes);
        let by = way * 20.0;
        h.step(&json!({"drag": [{"gizmo": "x"}, {"gizmo": "x", "by": [by.x, by.y, by.z]}], "steps": 12})).unwrap();
        h.frames(2);
        let Kind::Move { x, y, z, .. } = &h.app.dialog.as_ref().unwrap().kind else { panic!("not a move") };
        assert_eq!((x.as_str(), y.as_str(), z.as_str()), ("20 mm", "0 mm", "0 mm"), "fixed {fixed}");
        assert!((apply_point(&whole_move(&h), centre) - centre - by).len() < 1e-4, "fixed {fixed}");
    }
}

/// A turn spins the body where the distances have put it: they go on reading along the triad's
/// axes, and the body stays where it is.
#[test]
fn a_turn_after_a_distance_leaves_the_body_there() {
    let (mut h, centre) = turning_box();
    if let Some(Kind::Move { x, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        *x = "10 mm".into();
    }
    h.frames(2);
    turn_ring(&mut h, 2, 90.0);
    let Kind::Move { x, y, .. } = &h.app.dialog.as_ref().unwrap().kind else { panic!("not a move") };
    assert_eq!((x.as_str(), y.as_str()), ("0 mm", "-10 mm"));
    assert!((apply_point(&whole_move(&h), centre) - centre - Vec3::X * 10.0).len() < 1e-4);
}

/// Changing frames keeps the move; only the values start from zero on the new axes.
#[test]
fn local_and_fixed_can_swap_mid_move() {
    let (mut h, centre) = turning_box();
    set_angles(&mut h, ["0 deg", "0 deg", "90 deg"]);
    let before = whole_move(&h);
    let mut d = h.app.dialog.take().unwrap();
    set_fixed(&h.app, &mut d, true);
    assert_eq!(triad(&h.app, &d).map(|t| t.axes), Some(WORLD));
    h.app.dialog = Some(d);
    h.frames(2);
    assert_eq!(shown_angles(&h), ["0 deg", "0 deg", "0 deg"]);
    let after = whole_move(&h);
    assert_turn(&after, &rows_of(&before));
    assert!((apply_point(&after, centre) - centre).len() < 1e-4);
}

/// Own axes are the body's own: as its component places it, and as earlier moves turned it.
#[test]
fn the_triad_starts_on_the_bodys_own_axes() {
    let z = |h: &crate::scenario::Harness| {
        let mut d = h.app.dialog.clone().unwrap();
        d.inputs[0].items = vec![Sel::Body { name: "Body1".into() }];
        gizmo::axis(&triad(&h.app, &d).unwrap().axes, 2)
    };
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20})).unwrap();
    h.app.run("solid.move", json!({"bodies": ["Body1"], "translate": ["0", "0", "0"], "axis": [1.0, 0.0, 0.0], "angle": "90 deg"})).unwrap();
    h.app.start("solid.move");
    assert!((z(&h) + Vec3::Y).len() < 1e-9, "{:?}", z(&h));

    let mut h = crate::scenario::Harness::new();
    h.app.run("component.create", json!({"name": "Part", "grounded": false})).unwrap();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    h.app.run("occurrence.move", json!({"occurrence": "Part:1", "axis": [1, 0, 0], "angle": "90 deg", "translate": [50, 10, 20]})).unwrap();
    h.app.run("component.activate", json!({"component": "root"})).unwrap();
    h.app.start("solid.move");
    assert!((z(&h) + Vec3::Y).len() < 1e-9, "{:?}", z(&h));
}

/// The pivot is placed in two picks, a point and then a line, and accepted with the tick; the
/// body then turns about it.
#[test]
fn a_placed_pivot_is_what_the_body_turns_about() {
    let (mut h, centre) = turning_box();
    let mut d = h.app.dialog.take().unwrap();
    begin_pivot(&h.app, &mut d);
    assert_eq!(d.active, 1, "picks place the pivot, not bodies");
    let corner = Vec3::new(20.0, 30.0, 0.0);
    d.inputs[1].items = vec![Sel::Vertex { body: "Body1".into(), point: corner }];
    sync(&h.app, &mut d);
    assert!(d.inputs[1].items.is_empty(), "the pick is taken up, and the next one replaces it");
    d.inputs[1].items = vec![Sel::Axis { name: "X".into() }];
    sync(&h.app, &mut d);
    let t = triad(&h.app, &d).unwrap();
    assert!((t.center - corner).len() < 1e-5, "a line does not move the point: {:?}", t.center);
    assert!((gizmo::axis(&t.axes, 2) - Vec3::X).len() < 1e-7, "{:?}", t.axes);
    accept_pivot(&h.app, &mut d);
    assert_eq!(d.active, 0);
    h.app.dialog = Some(d);
    set_angles(&mut h, ["0 deg", "0 deg", "90 deg"]);
    // About its Z, which was laid along the world's X, through the corner.
    let m = whole_move(&h);
    assert_turn(&m, &gizmo::rotation(Vec3::X, std::f64::consts::FRAC_PI_2));
    assert!((apply_point(&m, corner) - corner).len() < 1e-4);
    let spun = corner + gizmo::apply(&gizmo::rotation(Vec3::X, std::f64::consts::FRAC_PI_2), centre - corner);
    assert!((apply_point(&m, centre) - spun).len() < 1e-4);
    // Fixed holds the same axes; only the triad stops turning.
    let mut d = h.app.dialog.take().unwrap();
    set_fixed(&h.app, &mut d, true);
    let t = triad(&h.app, &d).unwrap();
    assert!((t.center - corner).len() < 1e-4);
    let z = gizmo::apply(&gizmo::rotation(Vec3::X, std::f64::consts::FRAC_PI_2), Vec3::X);
    assert!((gizmo::axis(&t.axes, 2) - z).len() < 1e-7, "{:?}", t.axes);
    // Back to the body's own centre and axes, the move kept.
    let made_so_far = chain(&d).unwrap();
    clear_pivot(&h.app, &mut d);
    assert!(mat_inverse(&made_so_far).is_some_and(|back| is_identity(&mat_mul(&back, &chain(&d).unwrap()))));
    assert!((triad(&h.app, &d).unwrap().center - spun).len() < 1e-4);
}

/// A round pick places the pivot on the middle of its circle, along its axis; a planar face on
/// the face, along its normal. The triad's rings and arrows then move the pivot alone.
#[test]
fn a_pivot_lands_on_what_is_picked_and_the_triad_adjusts_it() {
    let (mut h, _) = turning_box();
    h.app.home.open = false;
    h.app.cam.distance = 25.0;
    h.app.dialog = h.app.dialog.take().map(|mut d| {
        begin_pivot(&h.app, &mut d);
        d
    });
    h.frames(2);
    let top = h.at(&json!({"world": [18, 27, 10]})).unwrap();
    h.step(&json!({"click": top})).unwrap();
    h.frames(2);
    let d = h.app.dialog.clone().unwrap();
    let t = triad(&h.app, &d).unwrap();
    assert!((t.center - Vec3::new(10.0, 15.0, 10.0)).len() < 1e-5, "{:?}", t.center);
    assert!((gizmo::axis(&t.axes, 2) - Vec3::Z).len() < 1e-7);
    let Kind::Move { z, .. } = &d.kind else { panic!("not a move") };
    assert_eq!(z, "5 mm", "the values read how far the pick has put it");
    let body = h.app.session.world_state().body("Body1").unwrap().mesh().bounds();

    h.step(&json!({"drag": [{"gizmo": "x"}, {"gizmo": "x", "by": [15.0, 0.0, 0.0]}], "steps": 12})).unwrap();
    h.frames(2);
    let d = h.app.dialog.as_ref().unwrap();
    let t = triad(&h.app, d).unwrap();
    assert!((t.center - Vec3::new(25.0, 15.0, 10.0)).len() < 2.0, "{:?}", t.center);
    let Kind::Move { x, .. } = &d.kind else { panic!("not a move") };
    assert_ne!(x, "0 mm", "the value box reads how far it has gone");
    assert!(is_identity(&whole_move(&h)), "placing moves no body");
    let still = h.app.session.world_state().body("Body1").unwrap().mesh().bounds();
    assert_eq!((still.min, still.max), (body.min, body.max));
}

/// While a pivot is placed the values measure it from where it started and move no body; the
/// tick or Enter keeps it, Esc goes back, and the values are the move's again, from zero.
#[test]
fn the_values_place_the_pivot_until_it_is_accepted() {
    let (mut h, centre) = turning_box();
    let values = |h: &mut crate::scenario::Harness, x: &str, turn: &str| {
        if let Some(Kind::Move { x: vx, rz, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            (*vx, *rz) = (x.into(), turn.into());
        }
        h.frames(3);
    };
    let begin = |h: &mut crate::scenario::Harness| {
        h.app.dialog = h.app.dialog.take().map(|mut d| {
            begin_pivot(&h.app, &mut d);
            d
        });
        h.frames(2);
    };
    let at = |h: &crate::scenario::Harness| triad(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    begin(&mut h);
    values(&mut h, "12 mm", "90 deg");
    let spot = centre + Vec3::X * 12.0;
    assert!((at(&h).center - spot).len() < 1e-5, "{:?}", at(&h).center);
    assert!((gizmo::axis(&at(&h).axes, 0) - Vec3::Y).len() < 1e-9, "the angles turn the pivot");
    assert_eq!(pivot_trail(&h.app, h.app.dialog.as_ref().unwrap()).map(|t| t.0), Some(centre));
    assert!(is_identity(&whole_move(&h)), "no body moves");

    h.step(&json!({"key": "Enter"})).unwrap();
    h.frames(2);
    let d = h.app.dialog.as_ref().expect("Enter accepts the pivot and keeps the move open");
    assert!(!placing_pivot(d) && d.active == 0);
    assert_eq!(shown_angles(&h), ["0 deg", "0 deg", "0 deg"]);
    assert!((at(&h).center - spot).len() < 1e-5, "{:?}", at(&h).center);
    assert!((gizmo::axis(&at(&h).axes, 0) - Vec3::Y).len() < 1e-9);
    assert!(is_identity(&whole_move(&h)));

    // The values drive the move again, about the accepted pivot.
    values(&mut h, "0 mm", "90 deg");
    assert!((apply_point(&whole_move(&h), spot) - spot).len() < 1e-4);

    values(&mut h, "0 mm", "0 deg");
    begin(&mut h);
    values(&mut h, "30 mm", "0 deg");
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    let d = h.app.dialog.as_ref().expect("Esc discards the pivot and keeps the move open");
    assert!(!placing_pivot(d));
    let Kind::Move { x, .. } = &d.kind else { panic!("not a move") };
    assert_eq!(x, "0 mm");
    assert!((at(&h).center - spot).len() < 1e-5, "the pivot there was stays: {:?}", at(&h).center);
}

/// Placing a pivot, Local goes to the object's own axes as it stands, turns made included, and
/// Fixed to the orientation of the target picked. Each keeps what it is given, and the pivot
/// stays where it is.
#[test]
fn local_and_fixed_switch_the_pivot_between_its_references() {
    let quarter = std::f64::consts::FRAC_PI_2;
    let (mut h, _) = turning_box();
    h.app.run("select.clear", json!({})).unwrap();
    h.app.dialog = None;
    h.app.run("solid.move", json!({"bodies": ["Body1"], "translate": ["0", "0", "0"], "axis": [0.0, 0.0, 1.0], "angle": "90 deg"})).unwrap();
    h.app.run("select.set", json!({"items": [Sel::Body { name: "Body1".into() }]})).unwrap();
    h.app.start("solid.move");
    h.frames(3);
    let own = gizmo::rotation(Vec3::Z, quarter);
    let near = |a: &Axes, b: &Axes| a.iter().flatten().zip(b.iter().flatten()).all(|(x, y)| (x - y).abs() < 1e-7);
    let mut d = h.app.dialog.take().unwrap();
    assert!(same_axes(&triad(&h.app, &d).unwrap().axes, &own), "Local starts on the object as it stands");
    begin_pivot(&h.app, &mut d);
    let spot = Vec3::new(3.0, 4.0, 5.0);
    d.inputs[1].items = vec![Sel::Vertex { body: String::new(), point: spot }];
    sync(&h.app, &mut d);
    let t = triad(&h.app, &d).unwrap();
    assert!(!fixed(&d) && same_axes(&t.axes, &own), "a point moves it, the object's axes stay");
    assert!((t.center - spot).len() < 1e-5);

    // The origin point is on no body: its frame is the world's.
    set_fixed(&h.app, &mut d, true);
    let t = triad(&h.app, &d).unwrap();
    assert!(near(&t.axes, &WORLD), "{:?}", t.axes);
    assert!((t.center - spot).len() < 1e-5);

    // A direction is the target's orientation, and switches to it from Local.
    set_fixed(&h.app, &mut d, false);
    assert!(near(&triad(&h.app, &d).unwrap().axes, &own));
    d.inputs[1].items = vec![Sel::Axis { name: "X".into() }];
    sync(&h.app, &mut d);
    assert!(fixed(&d));
    let target = triad(&h.app, &d).unwrap().axes;
    assert!((gizmo::axis(&target, 2) - Vec3::X).len() < 1e-7, "{target:?}");

    // The values read how far it has turned from where it started, as well as how far it went.
    let start = placing(&d).unwrap().1;
    let shown = angles(&h.app, &d).unwrap();
    assert!(shown.iter().any(|a| a.abs() > 1e-3), "{shown:?}");
    let remade = gizmo::mul(&start, &gizmo::turn_of(shown, order(&d)));
    assert!(remade.iter().flatten().zip(target.iter().flatten()).all(|(a, b)| (a - b).abs() < 1e-7), "{remade:?}");

    // Changes go on from a reference and stay with it through a switch away and back.
    let change = Change::Rotate { axis: 2, angle: quarter };
    drag(&h.app, &mut d, crate::gizmo::Drag { handle: crate::gizmo::Handle::Ring(2), change, started: true, done: true });
    let turned = gizmo::mul(&target, &gizmo::rotation(Vec3::Z, quarter));
    assert!(near(&triad(&h.app, &d).unwrap().axes, &turned));
    set_fixed(&h.app, &mut d, false);
    assert!(near(&triad(&h.app, &d).unwrap().axes, &own));
    set_fixed(&h.app, &mut d, true);
    assert!(near(&triad(&h.app, &d).unwrap().axes, &turned));

    // Both stay after the tick, and the toggle goes on switching.
    accept_pivot(&h.app, &mut d);
    let t = triad(&h.app, &d).unwrap();
    assert!(near(&t.axes, &turned) && (t.center - spot).len() < 1e-5);
    set_fixed(&h.app, &mut d, false);
    let t = triad(&h.app, &d).unwrap();
    assert!(near(&t.axes, &own) && (t.center - spot).len() < 1e-5, "{:?}", t.axes);
}

/// The preference keeps Move as it was: its triad starts on fixed axes and stays on them.
#[test]
fn the_preference_starts_move_on_fixed_axes() {
    let (mut h, _) = turning_box();
    h.app.dialog = None;
    h.app.preferences.move_starts_fixed = true;
    h.app.run("select.set", json!({"items": [Sel::Body { name: "Body1".into() }]})).unwrap();
    h.app.start("solid.move");
    h.frames(3);
    set_angles(&mut h, ["0 deg", "0 deg", "90 deg"]);
    let d = h.app.dialog.as_ref().unwrap();
    assert!(fixed(d));
    assert_eq!(triad(&h.app, d).map(|t| t.axes), Some(WORLD));
}

/// The pivot can be picked on the bodies where the move has put them.
#[test]
fn a_pivot_lands_on_a_body_that_has_been_moved() {
    let (mut h, _) = turning_box();
    h.app.home.open = false;
    h.app.cam.distance = 25.0;
    for (distance, turn) in [("50 mm", "0 deg"), ("0 mm", "90 deg")] {
        if let Some(Kind::Move { x, rz, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            (*x, *rz) = (distance.into(), turn.into());
        }
        h.frames(3);
        let m = whole_move(&h);
        h.app.dialog = h.app.dialog.take().map(|mut d| {
            begin_pivot(&h.app, &mut d);
            d
        });
        let centre = apply_point(&m, Vec3::new(10.0, 15.0, 5.0));
        h.app.cam.target = centre;
        h.frames(3);
        let at = placing(h.app.dialog.as_ref().unwrap()).unwrap().0;
        assert!((at - centre).len() < 1e-6, "{at:?}");
        let on = apply_point(&m, Vec3::new(18.0, 27.0, 10.0));
        h.step(&json!({"click": {"world": [on.x, on.y, on.z]}})).unwrap();
        h.frames(3);
        let at = triad(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().center;
        assert!((at - apply_point(&m, Vec3::new(10.0, 15.0, 10.0))).len() < 1e-5, "{distance} {turn}: {at:?}");
        h.app.dialog = h.app.dialog.take().map(|mut d| {
            accept_pivot(&h.app, &mut d);
            clear_pivot(&h.app, &mut d);
            d
        });
    }
}

/// Free Move's window: the pivot is a button, not a picking chip, beside Local and Fixed.
#[test]
fn the_pivot_row_places_a_pivot_from_a_button() {
    let (mut h, _) = turning_box();
    h.step(&json!({"click": {"handle": "dialog:pivot:place"}})).unwrap();
    h.frames(2);
    assert!(placing_pivot(h.app.dialog.as_ref().unwrap()));
    h.step(&json!({"click": {"handle": "dialog:pivot:accept"}})).unwrap();
    h.frames(2);
    let d = h.app.dialog.as_ref().unwrap();
    assert!(!placing_pivot(d) && d.extra.contains_key("pivot_axes"), "{:?}", d.extra);
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

/// Move works in the world, so the triad of a body in a placed component sits on the body where
/// it is placed, not where it was modelled, and a turn leaves the body's centre there.
#[test]
fn the_triad_sits_on_a_body_where_its_component_places_it() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("component.create", json!({"name": "Part", "grounded": false})).unwrap();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    h.app.run("occurrence.move", json!({"occurrence": "Part:1", "axis": [1, 0, 0], "angle": "90 deg", "translate": [50, 10, 20]})).unwrap();
    h.app.run("component.activate", json!({"component": "root"})).unwrap();
    h.app.start("solid.move");
    let mut d = h.app.dialog.clone().unwrap();
    d.inputs[0].items = vec![Sel::Body { name: "Body1".into() }];
    let world = h.app.session.world_state().body("Body1").unwrap().mesh().bounds().center();
    let at = triad(&h.app, &d).unwrap().center;
    assert!((at - world).len() < 1e-6, "triad at {at:?}, body at {world:?}");
    if let Kind::Move { rz, .. } = &mut d.kind {
        *rz = "90 deg".into();
    }
    for (command, params) in crate::dialogs::apply_commands(&h.app, &d).unwrap() {
        h.app.run(&command, params).unwrap();
    }
    let turned = h.app.session.world_state().body("Body1").unwrap().mesh().bounds().center();
    assert!((turned - world).len() < 1e-6, "turned from {world:?} to {turned:?}");
    // Editing a later move finds the body the same way, where it stands before that move.
    let moved = h.app.run("solid.move", json!({"bodies": ["Body1"], "translate": ["5 mm", "0 mm", "0 mm"]})).unwrap()["feature"].as_u64().unwrap();
    let d = crate::dialogs::for_feature(&h.app, moved, None).unwrap();
    let at = triad(&h.app, &d).unwrap().center;
    assert!((at - turned - Vec3::X * 5.0).len() < 1e-6, "triad at {at:?}, body was at {turned:?}");
}

/// Rotate holds the picked axis still, and a move's type can change on the way, in a placed
/// component too: the feature turns about the component's origin, not the world's.
#[test]
fn a_placed_body_turns_about_the_picked_axis_through_a_type_change() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("component.create", json!({"name": "Part", "grounded": false})).unwrap();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    h.app.run("occurrence.move", json!({"occurrence": "Part:1", "translate": [50, 10, 20]})).unwrap();
    h.app.run("component.activate", json!({"component": "root"})).unwrap();
    let world = h.app.session.world_state().body("Body1").unwrap().mesh().bounds().center();
    h.app.start("solid.move");
    let mut d = h.app.dialog.clone().unwrap();
    d.inputs[0].items = vec![Sel::Body { name: "Body1".into() }];
    if let Kind::Move { rz, .. } = &mut d.kind {
        *rz = "90 deg".into();
    }
    switch_type(&h.app, &mut d, ROTATE);
    d.inputs[1].items = vec![Sel::Axis { name: "Z".into() }];
    if let Kind::Move { angle, .. } = &mut d.kind {
        *angle = "90 deg".into();
    }
    for (command, params) in crate::dialogs::apply_commands(&h.app, &d).unwrap() {
        h.app.run(&command, params).unwrap();
    }
    // A quarter turn on the spot, then a quarter turn about the world's Z through the origin.
    let want = gizmo::apply(&gizmo::rotation(Vec3::Z, std::f64::consts::FRAC_PI_2), world);
    let got = h.app.session.world_state().body("Body1").unwrap().mesh().bounds().center();
    assert!((got - want).len() < 1e-5, "{got:?} != {want:?}");
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
    let bounds = |h: &crate::scenario::Harness| h.app.session.world_state().body("Body1").unwrap().mesh().bounds();
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

/// In a turned component the stored axis is the component's, like the distances: read as the
/// world's, the reopened move drew the body elsewhere, and OK would have moved it there.
#[test]
fn reopening_a_move_in_a_turned_component_changes_nothing() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("component.create", json!({"name": "Part", "grounded": false})).unwrap();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    h.app.run("occurrence.move", json!({"occurrence": "Part:1", "axis": [0, 0, 1], "angle": "30 deg", "translate": [130, 20, 20]})).unwrap();
    h.app.run("component.activate", json!({"component": "root"})).unwrap();
    let id = h
        .app
        .run("solid.move", json!({"bodies": ["Body1"], "translate": ["10 mm", "-5 mm", "0 mm"], "axis": [1.0, 0.0, 0.0], "angle": "25 deg"}))
        .unwrap()["feature"]
        .as_u64()
        .unwrap();
    let bounds = |h: &crate::scenario::Harness| h.app.session.world_state().body("Body1").unwrap().mesh().bounds();
    let before = bounds(&h);
    let d = crate::dialogs::for_feature(&h.app, id, None).unwrap();
    let at = triad(&h.app, &d).unwrap().center;
    assert!((at - before.center()).len() < 1e-4, "triad at {at:?}, body at {:?}", before.center());
    for (command, params) in crate::dialogs::apply_commands(&h.app, &d).unwrap() {
        h.app.run(&command, params).unwrap();
    }
    let after = bounds(&h);
    assert!((after.min - before.min).len() < 1e-3 && (after.max - before.max).len() < 1e-3, "{before:?} -> {after:?}");
}

/// A stored turn is about the origin, but its triad sits on the body where the move leaves it,
/// not at the origin plus the stored distance. The stored values stay until a ring is dragged,
/// which then turns the body on the spot.
#[test]
fn a_reopened_turned_move_has_its_triad_on_the_body() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20})).unwrap();
    let id = h
        .app
        .run("solid.move", json!({"bodies": ["Body1"], "translate": ["30 mm", "0 mm", "0 mm"], "axis": [0.0, 0.0, 1.0], "angle": "90 deg"}))
        .unwrap()["feature"]
        .as_u64()
        .unwrap();
    let body = h.app.session.world_state().body("Body1").unwrap().mesh().bounds().center();
    let mut d = crate::dialogs::for_feature(&h.app, id, None).unwrap();
    let at = triad(&h.app, &d).unwrap().center;
    assert!((at - body).len() < 1e-6, "triad at {at:?}, body at {body:?}");
    let p = params(&h.app, &d).unwrap();
    assert_eq!((&p["translate"][0], &p["angle"]), (&json!("30 mm"), &json!("90 deg")), "the stored values stay: {p}");

    let change = Change::Rotate { axis: 2, angle: 10_f64.to_radians() };
    drag(&h.app, &mut d, crate::gizmo::Drag { handle: crate::gizmo::Handle::Ring(2), change, started: true, done: true });
    let Kind::Move { rz, .. } = &d.kind else { panic!("not a move") };
    assert_eq!(rz, "100 deg");
    let at = triad(&h.app, &d).unwrap().center;
    assert!((at - body).len() < 1e-4, "a ring turns the body on the spot: {at:?}, body at {body:?}");
    let p = params(&h.app, &d).unwrap();
    let m = transform(&h.app, &d, &p).unwrap();
    assert!((apply_point(&m, Vec3::new(10.0, 10.0, 10.0)) - body).len() < 1e-4, "{p}");
}
