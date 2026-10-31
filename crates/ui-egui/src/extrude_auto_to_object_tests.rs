//! Extrude's automatic To Object: with profiles picked, a pick off their plane is the object to
//! extrude to, and the extent turns to To Object without the Extent Type choice.
use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::geom::Vec3;

use crate::dialogs::{Dialog, Kind, apply_commands, face_sel};
use crate::scenario::Harness;
use crate::viewport::Hit;

/// A 10 x 10 profile on XY, a 20 mm box beside it (x 30..50), a cylinder further on (x 65..75)
/// and construction planes 25 mm above and 12.345678 mm below XY; the Extrude dialog open with
/// the profile picked and the automatic To Object preference on.
fn scene() -> Harness {
    scene_with(true)
}

fn scene_with(auto_to_object: bool) -> Harness {
    let mut h = Harness::new();
    h.app.preferences.extrude_auto_to_object = auto_to_object;
    h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20, "corner": [30, 0, 0]})).unwrap();
    h.app.run("solid.cylinder", json!({"radius": 5, "height": 30, "base": [70, 10, 0], "operation": "new_body"})).unwrap();
    h.app.run("construct.plane.offset", json!({"base": "XY", "offset": 25, "name": "Lid"})).unwrap();
    h.app.run("construct.plane.offset", json!({"base": "XY", "offset": -12.345678, "name": "Under"})).unwrap();
    h.app.run("sketch.create", json!({"plane": "XY", "name": "Base"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.start("solid.extrude");
    h.frames(2);
    let sketch = h.app.session.doc.find_feature("Base").unwrap().id;
    let d = h.app.dialog.as_mut().unwrap();
    d.inputs[0].items = vec![Sel::Profile { sketch, index: 0 }];
    d.active = 0;
    h.frames(2);
    h
}

fn dialog(h: &Harness) -> &Dialog {
    h.app.dialog.as_ref().unwrap()
}

/// A click on `hit` as the viewport makes it: the dialog's candidate, then its pick.
fn click(h: &mut Harness, hit: impl FnOnce(&Harness) -> Hit) -> bool {
    let hit = hit(h);
    let mut d = h.app.dialog.take().unwrap();
    let sel = d.candidate(&h.app.session, &hit);
    if let Some(sel) = sel.clone() {
        d.pick(&h.app.session, sel);
    }
    h.app.dialog = Some(d);
    h.frames(2);
    sel.is_some()
}

fn face_hit(h: &Harness, p: Vec3) -> Hit {
    let Some(Sel::Face { body, index, point }) = face_sel(&h.app.session, p) else { panic!("no face at {p:?}") };
    Hit::Face { body, index, point }
}

fn to_object(d: &Dialog) -> Option<&Sel> {
    d.inputs.iter().find(|i| i.label == "To Object").and_then(|i| i.items.first())
}

fn distance(d: &Dialog) -> String {
    let Kind::Extrude { distance, .. } = &d.kind else { panic!("extrude") };
    distance.clone()
}

#[test]
fn face_off_the_profile_plane_turns_the_extent_to_that_face() {
    let mut h = scene();
    assert!(to_object(dialog(&h)).is_none(), "Distance first");
    assert!(click(&mut h, |h| face_hit(h, Vec3::new(40.0, 10.0, 20.0))));
    let d = dialog(&h);
    assert!(matches!(to_object(d), Some(Sel::Face { .. })));
    assert_eq!(d.inputs[0].items.len(), 1, "the profile stays the only profile");
    assert_eq!(d.active, 1, "the To Object input is active");
    assert_eq!(distance(d), "20 mm", "the length is the target's");
    let commands = apply_commands(&h.app, d).unwrap();
    let params = &commands[0].1;
    assert!(params.get("to").is_some());
    let reach = solidcraft_engine::extrude_reach(&h.app.session, params).unwrap();
    assert!((reach - 20.0).abs() < 1e-9, "{reach}");
    // OK builds the extrude up to the face.
    h.step(&json!({"widget": "OK"})).unwrap();
    h.frames(4);
    let state = h.app.session.world_state();
    let volume: f64 = state.bodies.iter().filter_map(|b| solidcraft_engine::doc::kernel::measure(&b.body).ok()).map(|m| m.volume).sum();
    // The box, the cylinder and the 10 x 10 x 20 extrude.
    let expected = 8000.0 + std::f64::consts::PI * 25.0 * 30.0 + 2000.0;
    assert!((volume - expected).abs() < 1.0, "{volume}");
}

#[test]
fn coplanar_face_stays_a_profile() {
    let mut h = scene();
    assert!(click(&mut h, |h| face_hit(h, Vec3::new(40.0, 10.0, 0.0))));
    let d = dialog(&h);
    assert!(to_object(d).is_none());
    assert_eq!(d.inputs[0].items.len(), 2, "the bottom face joins the profiles");
    assert_eq!(distance(d), "10 mm");
}

#[test]
fn vertex_and_curved_face_are_targets() {
    let mut h = scene();
    // A vertex in the profile plane is not something to go to.
    assert!(!click(&mut h, |h| Hit::Vertex { body: dialog_body(h), point: Vec3::new(50.0, 20.0, 0.0) }));
    assert!(to_object(dialog(&h)).is_none());
    assert!(click(&mut h, |h| Hit::Vertex { body: dialog_body(h), point: Vec3::new(50.0, 20.0, 20.0) }));
    assert!(matches!(to_object(dialog(&h)), Some(Sel::Vertex { .. })));

    let mut h = scene();
    assert!(click(&mut h, curved_hit));
    assert!(matches!(to_object(dialog(&h)), Some(Sel::Face { .. })));
}

/// The cylinder's side, at the middle of one of its triangles.
fn curved_hit(h: &Harness) -> Hit {
    let st = h.app.session.world_state();
    for b in &st.bodies {
        let m = b.mesh();
        for (t, f) in m.triangles.iter().zip(&m.tri_face) {
            let Some([p, q, r]) = m.tri(t) else { continue };
            if b.body.planar_face(*f as usize).is_none() {
                return Hit::Face { body: b.name.clone(), index: *f as usize, point: (p + q + r) * (1.0 / 3.0) };
            }
        }
    }
    panic!("no curved face")
}

fn dialog_body(h: &Harness) -> String {
    h.app.session.world_state().bodies.first().unwrap().name.clone()
}

#[test]
fn parallel_construction_plane_is_a_target_and_a_crossing_one_is_not() {
    let mut h = scene();
    assert!(!click(&mut h, |_| Hit::Plane { name: "YZ".into(), point: Vec3::new(0.0, 5.0, 5.0) }));
    assert!(to_object(dialog(&h)).is_none());
    assert!(click(&mut h, |_| Hit::Plane { name: "Lid".into(), point: Vec3::new(5.0, 5.0, 25.0) }));
    let d = dialog(&h);
    // The plane itself, so the extrude follows it (#7494).
    assert_eq!(to_object(d), Some(&Sel::Plane { name: "Lid".into() }));
    assert_eq!(distance(d), "25 mm");
    // The profile plane itself is nowhere to go: the target stays.
    assert!(!click(&mut h, |_| Hit::Plane { name: "XY".into(), point: Vec3::new(5.0, 5.0, 0.0) }));
    assert_eq!(to_object(dialog(&h)), Some(&Sel::Plane { name: "Lid".into() }));
}

#[test]
fn without_profiles_a_face_is_a_profile() {
    let mut h = scene();
    h.app.dialog.as_mut().unwrap().inputs[0].items.clear();
    assert!(click(&mut h, |h| face_hit(h, Vec3::new(40.0, 10.0, 20.0))));
    let d = dialog(&h);
    assert!(to_object(d).is_none());
    assert!(matches!(d.inputs[0].items.as_slice(), [Sel::Face { .. }]));
}

#[test]
fn thin_extrude_keeps_its_picks() {
    let mut h = scene();
    if let Some(Kind::Extrude { thin, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        *thin = true;
    }
    click(&mut h, |h| face_hit(h, Vec3::new(40.0, 10.0, 20.0)));
    assert!(to_object(dialog(&h)).is_none());
}

#[test]
fn target_below_keeps_its_side_and_exact_length() {
    let mut h = scene();
    // As the browser picks a plane: no place on it.
    assert!(click(&mut h, |_| Hit::Plane { name: "Under".into(), point: Vec3::ZERO }));
    let d = dialog(&h);
    assert_eq!(to_object(d), Some(&Sel::Plane { name: "Under".into() }));
    assert_eq!(distance(d), "12.345678 mm");
    assert!(matches!(d.kind, Kind::Extrude { direction: 1, .. }), "the target is below the profile");
}

#[test]
fn off_by_default_a_face_off_the_profile_plane_joins_the_profiles() {
    let h = Harness::new();
    assert!(!h.app.preferences.extrude_auto_to_object, "the preference starts off");
    let mut h = scene_with(false);
    assert!(click(&mut h, |h| face_hit(h, Vec3::new(40.0, 10.0, 20.0))));
    let d = dialog(&h);
    assert!(to_object(d).is_none(), "the extent stays Distance");
    assert_eq!(d.inputs[0].items.len(), 2, "the top face joins the profiles, as before");
    assert_eq!(distance(d), "10 mm");
    // A parallel construction plane is not taken as a target either.
    let mut h = scene_with(false);
    assert!(!click(&mut h, |_| Hit::Plane { name: "Lid".into(), point: Vec3::new(5.0, 5.0, 25.0) }));
    assert!(to_object(dialog(&h)).is_none());
}

#[test]
fn manual_to_object_accepts_a_parallel_plane_with_automatic_switching_off() {
    let mut h = scene_with(false);
    h.step(&json!({"click": {"handle": "dialog:Extent Type"}})).unwrap();
    h.step(&json!({"click": {"handle": "dialog:Extent Type:To Object"}})).unwrap();
    assert_eq!(dialog(&h).active_input().unwrap().label, "To Object");
    assert!(!click(&mut h, |_| Hit::Plane { name: "YZ".into(), point: Vec3::new(0.0, 5.0, 5.0) }));
    assert!(!click(&mut h, |_| Hit::Plane { name: "XY".into(), point: Vec3::new(5.0, 5.0, 0.0) }));
    assert!(to_object(dialog(&h)).is_none());
    assert!(click(&mut h, |_| Hit::Plane { name: "Lid".into(), point: Vec3::new(5.0, 5.0, 25.0) }));
    let commands = apply_commands(&h.app, dialog(&h)).unwrap();
    assert_eq!(commands[0].1["to"], json!({"plane": "Lid"}));
    assert!((solidcraft_engine::extrude_reach(&h.app.session, &commands[0].1).unwrap() - 25.0).abs() < 1e-9);
}
