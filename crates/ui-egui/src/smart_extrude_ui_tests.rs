//! Smart Extrude in the real window (#1257): the direction follows the side a profile is picked
//! from, a drag into a body cuts and stays a Cut past the far side, and explicit choices win.
use crate::dialogs::{Dialog, Kind};
use crate::scenario::Harness;
use serde_json::json;

const JOIN: usize = 1;
const CUT: usize = 2;

/// The extrude window's direction, operation and whether the operation is still automatic.
fn state(h: &Harness) -> (usize, usize, bool) {
    let Some(Dialog { kind: Kind::Extrude { direction, operation, auto_op, .. }, .. }) = &h.app.dialog else {
        panic!("extrude dialog: {:?}", h.app.dialog)
    };
    (*direction, *operation, *auto_op)
}

fn bounds_z(h: &Harness) -> (f64, f64) {
    let st = h.app.session.world_state();
    let b = st.bodies.last().expect("a body").mesh().bounds();
    (b.min.z, b.max.z)
}

fn volume(h: &Harness) -> f64 {
    let st = h.app.session.world_state();
    st.bodies.iter().map(|b| solidcraft_engine::doc::kernel::measure(&b.body).unwrap().volume).sum()
}

/// A sketch on XY extruded from the top view goes up, and from the bottom view goes down.
#[test]
fn a_profile_picked_from_below_extrudes_down() {
    for (view, dir, z) in [("top", 0, (0.0, 10.0)), ("bottom", 1, (-10.0, 0.0))] {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,20]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("select.clear", json!({})).unwrap();
        h.step(&json!({"call":"ui.view","params":{"view":view}})).unwrap();
        h.settle();
        h.step(&json!({"start":"solid.extrude"})).unwrap();
        // Pick the profile in the viewport, as a hand does.
        h.step(&json!({"click":{"world":[10,10,0]}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items.len(), 1, "{view}: the profile is picked");
        assert_eq!(state(&h).0, dir, "{view}: direction");
        h.step(&json!({"key":"Enter"})).unwrap();
        assert!(h.app.dialog.is_none(), "{view}: OK");
        let (lo, hi) = bounds_z(&h);
        assert!((lo - z.0).abs() < 1e-6 && (hi - z.1).abs() < 1e-6, "{view}: the body spans z {lo}..{hi}");
    }
}

/// A 20 x 20 x 10 box and an extrude from its top: the top face itself (Press Pull), or a
/// 10 x 10 square sketched on it.
fn box_with_top_extrude(face: bool) -> Harness {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":20,"width":20,"height":10,"corner":[0,0,0]})).unwrap();
    if !face {
        h.app.run("sketch.create", json!({"plane":{"origin":[0,0,10],"x_dir":[1,0,0],"y_dir":[0,1,0]}})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[5,5],"p1":[15,15]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("select.clear", json!({})).unwrap();
    }
    h.step(&json!({"call":"ui.view","params":{"view":"home"}})).unwrap();
    h.step(&json!({"call":"ui.view","params":{"view":"fit"}})).unwrap();
    h.settle();
    if face {
        h.step(&json!({"start":"solid.press_pull"})).unwrap();
        h.step(&json!({"click":{"world":[10,10,10]}})).unwrap();
    } else {
        // The new sketch's profile is taken at once.
        h.step(&json!({"start":"solid.extrude"})).unwrap();
    }
    h.frames(2);
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items.len(), 1, "the top is picked");
    h
}

fn drag_to(h: &mut Harness, z: f64) {
    h.step(&json!({"drag":[{"handle":"arrow"},{"world":[10,10,z]}]})).unwrap();
    h.frames(2);
}

/// Pushed into the box the top cuts; dragged on far past the bottom it stays a Cut (a cut
/// through), for a face (Press Pull) and a profile on it alike.
#[test]
fn a_drag_past_the_far_side_stays_a_cut() {
    for face in [true, false] {
        let mut h = box_with_top_extrude(face);
        assert_eq!(state(&h), (0, JOIN, true), "face {face}: outward from the top: Join");
        drag_to(&mut h, 5.0);
        assert_eq!(state(&h), (0, CUT, true), "face {face}: into the box: Cut");
        drag_to(&mut h, -40.0);
        assert_eq!(state(&h), (0, CUT, true), "face {face}: far past the bottom: still Cut");
        if face {
            continue;
        }
        h.step(&json!({"key":"Enter"})).unwrap();
        assert!(h.app.dialog.is_none());
        assert!((volume(&h) - (4000.0 - 1000.0)).abs() < 1e-6, "a square hole right through: {}", volume(&h));
    }
}

/// The reverse of a drag: out the other side of zero the choice is made afresh.
#[test]
fn reversing_past_the_start_joins_again() {
    let mut h = box_with_top_extrude(true);
    drag_to(&mut h, 4.0);
    assert_eq!(state(&h).1, CUT);
    drag_to(&mut h, 16.0);
    assert_eq!(state(&h), (0, JOIN, true), "pulled out: Join");
    h.step(&json!({"key":"Enter"})).unwrap();
    assert!((volume(&h) - 20.0 * 20.0 * 16.0).abs() < 1e-6, "{}", volume(&h));
}

/// Reverse is an explicit choice: the operation stops following the geometry.
#[test]
fn an_explicit_reverse_keeps_the_operation() {
    let mut h = box_with_top_extrude(true);
    h.step(&json!({"widget":"Reverse Direction"})).unwrap();
    h.frames(2);
    assert_eq!(state(&h), (1, JOIN, false), "reversed into the box, the operation stays as it was");
    drag_to(&mut h, 2.0);
    assert_eq!(state(&h), (1, JOIN, false));
}

/// Press Pull on a hole's wall shows an arrow along the wall's normal: dragged 2 mm towards the
/// axis, the 5 mm hole closes to 3 mm (#2873).
#[test]
fn press_pull_drags_a_hole_wall() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
    h.app.run("solid.cylinder", json!({"radius":5,"height":20,"base":[20,15,0],"operation":"cut"})).unwrap();
    let st = h.app.session.model.state();
    let wall = st.body("Body1").unwrap().body.faces(0.01).unwrap().into_iter().find(|f| f.plane_normal.is_none()).unwrap().index;
    drop(st);
    h.step(&json!({"call":"ui.view","params":{"view":"home"}})).unwrap();
    h.step(&json!({"call":"ui.view","params":{"view":"fit"}})).unwrap();
    h.settle();
    let p = solidcraft_engine::geom::Vec3::new(25.0, 15.0, 10.0);
    h.app.session.selection = vec![solidcraft_engine::Sel::Face { body: "Body1".into(), index: wall, point: p }];
    h.step(&json!({"start":"solid.press_pull"})).unwrap();
    h.frames(2);
    h.step(&json!({"drag":[{"handle":"arrow"},{"world":[23,15,10]}]})).unwrap();
    h.frames(2);
    let Some(Dialog { kind: Kind::OffsetFaces { distance }, .. }) = &h.app.dialog else { panic!("offset dialog: {:?}", h.app.dialog) };
    let d = h.app.session.doc.eval(distance, solidcraft_engine::doc::expr::Kind::Length).unwrap();
    assert!((d - 2.0).abs() < 0.05, "{distance}");
    h.step(&json!({"key":"Enter"})).unwrap();
    assert!(h.app.dialog.is_none());
    let v = volume(&h);
    assert!((v - (24000.0 - std::f64::consts::PI * 9.0 * 20.0)).abs() / v < 5e-4, "{v}");
}
