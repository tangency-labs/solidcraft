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
    let st = h.app.session.world_state().unwrap();
    let b = st.bodies.last().expect("a body").mesh().bounds();
    (b.min.z, b.max.z)
}

fn volume(h: &Harness) -> f64 {
    let st = h.app.session.world_state().unwrap();
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

/// A Cut latched for a drag into the box gives way once the extrude starts from the bottom
/// face: from there the same distance goes out of the box, so it joins (#467).
#[test]
fn a_new_start_object_decides_the_operation_again() {
    let mut h = box_with_top_extrude(false);
    drag_to(&mut h, 5.0);
    assert_eq!(state(&h), (0, CUT, true), "into the box: Cut");
    // Start: From Object, the box's bottom face (as the Start choice and a pick there make it).
    let st = crate::open_meshing::session_world(&h.app.session);
    let b = st.bodies.first().expect("the box");
    let bottom =
        (0..64).find(|i| b.body.planar_face(*i).is_some_and(|p| p.normal().z < -0.99 && p.origin.z.abs() < 1e-9)).expect("the box's bottom face");
    let face = solidcraft_engine::Sel::Face { body: b.name.clone(), index: bottom, point: solidcraft_engine::geom::Vec3::new(10.0, 10.0, 0.0) };
    if let Some(d) = &mut h.app.dialog {
        let mut from = crate::selection::SelInput::new("From Object", crate::selection::FACES | crate::selection::VERTICES, false);
        from.items.push(face);
        d.inputs.push(from);
    }
    h.frames(2);
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs.last().unwrap().items.len(), 1, "the bottom face is the start");
    assert_eq!(state(&h), (0, JOIN, true), "from the bottom face, 5 mm down: Join");
    h.step(&json!({"key":"Enter"})).unwrap();
    assert!(h.app.dialog.is_none());
    assert!((volume(&h) - (4000.0 + 500.0)).abs() < 1e-6, "a 10 x 10 x 5 block under the box: {}", volume(&h));
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

/// Pressing on a top face and pulling it up extrudes it by the distance dragged, without any
/// tool being started; letting go leaves the window open until OK.
#[test]
fn pulling_a_face_up_extrudes_it() {
    let mut h = Harness::new();
    h.app.preferences.direct_interaction = true;
    h.app.run("solid.box", json!({"length":20,"width":20,"height":10,"corner":[0,0,0]})).unwrap();
    h.step(&json!({"call":"ui.view","params":{"view":"home"}})).unwrap();
    h.step(&json!({"call":"ui.view","params":{"view":"fit"}})).unwrap();
    h.settle();
    h.step(&json!({"drag":[{"world":[10,10,10]},{"world":[10,10,20]}]})).unwrap();
    h.frames(4);
    assert!(h.app.dialog.is_some(), "the window stays open after the release");
    assert_eq!(state(&h), (0, JOIN, true), "pulled out: Join");
    h.step(&json!({"key":"Enter"})).unwrap();
    assert!(h.app.dialog.is_none(), "OK applied the extrude");
    let (lo, hi) = bounds_z(&h);
    assert!(lo.abs() < 1e-6 && hi > 10.5, "the box grew upward: z {lo}..{hi}");
}

/// Direct interaction is a setting, off until someone turns it on: a pull then does nothing.
#[test]
fn direct_interaction_is_off_until_the_setting_is_on() {
    let mut h = Harness::new();
    assert!(!h.app.preferences.direct_interaction);
    h.app.run("solid.box", json!({"length":20,"width":20,"height":10,"corner":[0,0,0]})).unwrap();
    h.step(&json!({"call":"ui.view","params":{"view":"home"}})).unwrap();
    h.step(&json!({"call":"ui.view","params":{"view":"fit"}})).unwrap();
    h.settle();
    let before = volume(&h);
    h.step(&json!({"drag":[{"world":[10,10,10]},{"world":[10,10,20]}]})).unwrap();
    h.frames(4);
    assert!(h.app.dialog.is_none(), "no extrude window");
    assert!((volume(&h) - before).abs() < 1e-6);
}

/// A press on a face that moves sideways is still a box selection: nothing is extruded.
#[test]
fn dragging_sideways_over_a_face_does_not_extrude() {
    let mut h = Harness::new();
    h.app.preferences.direct_interaction = true;
    h.app.run("solid.box", json!({"length":20,"width":20,"height":10,"corner":[0,0,0]})).unwrap();
    h.step(&json!({"call":"ui.view","params":{"view":"home"}})).unwrap();
    h.step(&json!({"call":"ui.view","params":{"view":"fit"}})).unwrap();
    h.settle();
    let before = volume(&h);
    h.step(&json!({"drag":[{"world":[10,10,10]},{"world":[18,10,10]}]})).unwrap();
    h.frames(4);
    assert!(h.app.dialog.is_none());
    assert!((volume(&h) - before).abs() < 1e-6);
}

/// Pulling a face down into the body cuts, and the window waits for OK as well.
#[test]
fn pulling_a_face_down_into_the_body_cuts() {
    let mut h = Harness::new();
    h.app.preferences.direct_interaction = true;
    h.app.run("solid.box", json!({"length":20,"width":20,"height":10,"corner":[0,0,0]})).unwrap();
    h.step(&json!({"call":"ui.view","params":{"view":"home"}})).unwrap();
    h.step(&json!({"call":"ui.view","params":{"view":"fit"}})).unwrap();
    h.settle();
    h.step(&json!({"drag":[{"world":[10,10,10]},{"world":[10,10,5]}]})).unwrap();
    h.frames(4);
    assert_eq!(state(&h).1, CUT, "pushed into the box: Cut");
}

/// The automatic operation probes the bodies' triangles: an open window asks for it once per
/// change of its inputs or the design, not every frame.
#[test]
fn an_open_extrude_asks_for_its_operation_only_when_something_changes() {
    let mut h = box_with_top_extrude(false);
    let asked = crate::dialogs::auto_operation_asked();
    h.frames(20);
    assert_eq!(crate::dialogs::auto_operation_asked(), asked, "nothing changed: not asked again");
    drag_to(&mut h, 5.0);
    assert_eq!(state(&h), (0, CUT, true), "into the box: Cut");
    let after_drag = crate::dialogs::auto_operation_asked();
    assert!(after_drag > asked, "a new distance is asked about");
    h.frames(20);
    assert_eq!(crate::dialogs::auto_operation_asked(), after_drag, "held still: not asked again");
    // A change to the design (another body, out of the extrude's way) is asked about again.
    h.app.run("solid.box", json!({"length":5,"width":5,"height":5,"corner":[50,0,0]})).unwrap();
    h.frames(2);
    assert!(crate::dialogs::auto_operation_asked() > after_drag, "a changed design is asked about");
    assert_eq!(state(&h), (0, CUT, true), "still into the box: Cut");
}
