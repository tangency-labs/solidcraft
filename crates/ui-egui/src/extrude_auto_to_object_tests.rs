//! Extrude's automatic To Object: with profiles picked, a pick off their plane is the object to
//! extrude to, and the extent turns to To Object without the Extent Type choice.
use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_engine::Sel;
use solidcraft_engine::geom::prelude::*;

use crate::dialogs::{Dialog, Kind, apply_commands, face_sel};
use crate::scenario::Harness;
use crate::selection::SelectionExt;
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
    // Extrude decides its operation off the window thread after the profile is picked; Join
    // shows Target Bodies and New does not, so the form has its final rows only then (#17657).
    for _ in 0..500 {
        h.frames(1);
        if !h.app.dialog.as_ref().is_some_and(|d| d.op_auto.pending) {
            break;
        }
    }
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
    let state = crate::open_meshing::session_world(&h.app.session);
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
    let st = crate::open_meshing::session_world(&h.app.session);
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
    crate::open_meshing::session_world(&h.app.session).bodies.first().unwrap().name.clone()
}

#[test]
fn parallel_construction_plane_is_a_target_and_a_crossing_one_is_not() {
    let mut h = scene();
    assert!(!click(&mut h, |_| Hit::Plane { name: "YZ".into(), point: Vec3::new(0.0, 5.0, 5.0) }));
    assert!(to_object(dialog(&h)).is_none());
    assert!(click(&mut h, |_| Hit::Plane { name: "Lid".into(), point: Vec3::new(5.0, 5.0, 25.0) }));
    let d = dialog(&h);
    assert!(matches!(to_object(d), Some(Sel::Vertex { body, .. }) if body.is_empty()));
    assert_eq!(distance(d), "25 mm");
    // The profile plane itself is nowhere to go: the target stays.
    assert!(!click(&mut h, |_| Hit::Plane { name: "XY".into(), point: Vec3::new(5.0, 5.0, 0.0) }));
    assert!(matches!(to_object(dialog(&h)), Some(Sel::Vertex { point, .. }) if (point.z - 25.0).abs() < 1e-9));
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
    assert!(matches!(to_object(d), Some(Sel::Vertex { point, .. }) if (point.z + 12.345678).abs() < 1e-9));
    assert_eq!(distance(d), "12.345678 mm");
    assert!(matches!(d.kind, Kind::Extrude { direction: 1, .. }), "the target is below the profile");
}

#[test]
fn with_the_preference_off_a_face_off_the_profile_plane_joins_the_profiles() {
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
    assert_eq!(commands[0].1["to"], json!([0.0, 0.0, 25.0]));
    assert!((solidcraft_engine::extrude_reach(&h.app.session, &commands[0].1).unwrap() - 25.0).abs() < 1e-9);
}

/// A snap point (the end of a sketch line, woken by hovering the line) reaches the dialog as a
/// vertex of no body. To Object and From Object take it as the place to extrude to or from (#17394).
#[test]
fn to_and_from_object_take_a_snap_point() {
    let mut h = scene_with(false);
    h.step(&json!({"click": {"handle": "dialog:Extent Type"}})).unwrap();
    h.step(&json!({"click": {"handle": "dialog:Extent Type:To Object"}})).unwrap();
    assert_eq!(dialog(&h).active_input().unwrap().label, "To Object");
    let end = Vec3::new(5.0, 5.0, 30.0);
    assert!(click(&mut h, |_| Hit::Vertex { body: String::new(), point: end }), "a sketch line's end is a To Object");
    assert_eq!(to_object(dialog(&h)), Some(&Sel::Vertex { body: String::new(), point: end }));
    let commands = apply_commands(&h.app, dialog(&h)).unwrap();
    assert_eq!(commands[0].1["to"], json!([5.0, 5.0, 30.0]));
    assert!((solidcraft_engine::extrude_reach(&h.app.session, &commands[0].1).unwrap() - 30.0).abs() < 1e-9);

    h.step(&json!({"click": {"handle": "dialog:section:Advanced"}})).unwrap();
    h.step(&json!({"click": {"handle": "dialog:Start"}})).unwrap();
    h.step(&json!({"click": {"handle": "dialog:Start:From Object"}})).unwrap();
    assert_eq!(dialog(&h).active_input().unwrap().label, "From Object");
    assert!(click(&mut h, |_| Hit::Vertex { body: String::new(), point: Vec3::new(5.0, 5.0, 4.0) }), "and a From Object");
    let commands = apply_commands(&h.app, dialog(&h)).unwrap();
    assert_eq!(commands[0].1["from"], json!([5.0, 5.0, 4.0]));
}

#[test]
fn manual_from_object_accepts_parallel_and_coplanar_planes() {
    for automatic in [false, true] {
        let mut h = scene_with(automatic);
        h.step(&json!({"click": {"handle": "dialog:section:Advanced"}})).unwrap();
        h.step(&json!({"click": {"handle": "dialog:Start"}})).unwrap();
        h.step(&json!({"click": {"handle": "dialog:Start:From Object"}})).unwrap();
        assert_eq!(dialog(&h).active_input().unwrap().label, "From Object");
        assert!(!click(&mut h, |_| Hit::Plane { name: "YZ".into(), point: Vec3::new(0.0, 5.0, 5.0) }));
        assert!(dialog(&h).active_input().unwrap().items.is_empty());

        for (name, z) in [("Lid", 25.0), ("XY", 0.0), ("Under", -12.345678)] {
            // Browser plane picks carry no point; use the plane's exact world origin.
            assert!(click(&mut h, |_| Hit::Plane { name: name.into(), point: Vec3::ZERO }), "{name}, automatic={automatic}");
            let d = dialog(&h);
            assert_eq!(d.active_input().unwrap().label, "From Object");
            assert!(to_object(d).is_none(), "a start pick must not change the end extent");
            assert_eq!(d.inputs[0].items.len(), 1);
            assert_eq!(distance(d), "10 mm");
            let commands = apply_commands(&h.app, d).unwrap();
            assert_eq!(commands[0].1["from"], json!([0.0, 0.0, z]));
            assert!(commands[0].1.get("to").is_none());
        }

        // Refusing a crossing plane must preserve the previous valid start.
        assert!(!click(&mut h, |_| Hit::Plane { name: "YZ".into(), point: Vec3::ZERO }));
        let commands = apply_commands(&h.app, dialog(&h)).unwrap();
        assert_eq!(commands[0].1["from"], json!([0.0, 0.0, -12.345678]));

        assert!(click(&mut h, |h| face_hit(h, Vec3::new(40.0, 10.0, 20.0))));
        let commands = apply_commands(&h.app, dialog(&h)).unwrap();
        assert_eq!(commands[0].1["from"], json!([40.0, 10.0, 20.0]));
        assert!(click(&mut h, |h| Hit::Vertex { body: dialog_body(h), point: Vec3::new(50.0, 20.0, 0.0) }));
        let commands = apply_commands(&h.app, dialog(&h)).unwrap();
        assert_eq!(commands[0].1["from"], json!([50.0, 20.0, 0.0]));
        assert!(to_object(dialog(&h)).is_none());
    }
}

#[test]
fn manual_from_object_keeps_the_selected_end_plane() {
    let mut h = scene();
    assert!(click(&mut h, |_| Hit::Plane { name: "Lid".into(), point: Vec3::ZERO }));
    h.step(&json!({"click": {"handle": "dialog:section:Advanced"}})).unwrap();
    h.step(&json!({"click": {"handle": "dialog:Start"}})).unwrap();
    h.step(&json!({"click": {"handle": "dialog:Start:From Object"}})).unwrap();
    assert!(click(&mut h, |_| Hit::Plane { name: "Under".into(), point: Vec3::ZERO }));
    let commands = apply_commands(&h.app, dialog(&h)).unwrap();
    assert_eq!(commands[0].1["from"], json!([0.0, 0.0, -12.345678]));
    assert_eq!(commands[0].1["to"], json!([0.0, 0.0, 25.0]));
}

#[test]
fn the_preference_starts_on() {
    assert!(Harness::new().app.preferences.extrude_auto_to_object);
}

/// The scene with the box's top face (z = 20) picked as the profile instead of the sketch.
fn face_profile_scene() -> Harness {
    let mut h = scene();
    let top = face_sel(&h.app.session, Vec3::new(40.0, 10.0, 20.0)).unwrap();
    h.app.dialog.as_mut().unwrap().inputs[0].items = vec![top];
    h.frames(2);
    h
}

#[test]
fn face_profile_then_the_opposite_face_is_the_target() {
    let mut h = face_profile_scene();
    assert!(click(&mut h, |h| face_hit(h, Vec3::new(40.0, 10.0, 0.0))));
    let d = dialog(&h);
    assert!(matches!(to_object(d), Some(Sel::Face { .. })), "the bottom face is where it goes to");
    assert_eq!(d.inputs[0].items.len(), 1, "the top face stays the only profile");
    let commands = apply_commands(&h.app, d).unwrap();
    assert!(commands[0].1.get("to").is_some());
    let reach = solidcraft_engine::extrude_reach(&h.app.session, &commands[0].1).unwrap();
    assert!((reach + 20.0).abs() < 1e-9, "{reach}: 20 mm down through the body");
}

#[test]
fn face_profile_then_a_parallel_face_of_another_body_is_the_target() {
    let mut h = face_profile_scene();
    assert!(click(&mut h, |h| face_hit(h, Vec3::new(70.0, 10.0, 30.0))));
    let d = dialog(&h);
    assert!(matches!(to_object(d), Some(Sel::Face { .. })));
    assert_eq!(d.inputs[0].items.len(), 1);
}

#[test]
fn a_side_face_is_a_target_at_its_picked_point() {
    let mut h = face_profile_scene();
    assert!(click(&mut h, |h| face_hit(h, Vec3::new(50.0, 10.0, 10.0))));
    let d = dialog(&h);
    assert!(matches!(to_object(d), Some(Sel::Face { .. })), "a non-parallel planar face is taken as the target");
    assert_eq!(d.inputs[0].items.len(), 1);
}

#[test]
fn clicking_into_the_profiles_box_adds_an_off_plane_face_as_a_profile() {
    let mut h = scene();
    let at = crate::scenario::rect_of("dialog:field:Profiles").expect("the Profiles field is drawn").center();
    h.step(&json!({"click": [at.x, at.y]})).unwrap();
    h.frames(2);
    assert!(click(&mut h, |h| face_hit(h, Vec3::new(40.0, 10.0, 20.0))));
    let d = dialog(&h);
    assert!(to_object(d).is_none(), "the box was chosen, so the pick is a profile");
    assert_eq!(d.inputs[0].items.len(), 2);
}

/// A base, a target 30 mm above it, and a 20 x 20 profile sketched on the base's top face; the
/// Extrude dialog open with the profile picked and the operation chosen as Join.
fn join_to_scene() -> Harness {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 40, "width": 40, "height": 10, "body_name": "Base"})).unwrap();
    h.app.run("solid.box", json!({"corner": [0, 0, 40], "length": 40, "width": 40, "height": 20, "body_name": "Target"})).unwrap();
    let sketch = h.app.run("sketch.create", json!({"plane": {"face": [20, 20, 10]}, "name": "Top"})).unwrap()["sketch"].as_u64().unwrap();
    let plane = h.app.session.model.state().sketch(sketch).unwrap().plane;
    let at = |x: f64, y: f64| {
        let p = plane.to_local(Vec3::new(x, y, 10.0));
        json!([p.x, p.y])
    };
    h.app.run("sketch.rectangle.two_point", json!({"p0": at(10.0, 10.0), "p1": at(30.0, 30.0)})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.start("solid.extrude");
    h.frames(2);
    let d = h.app.dialog.as_mut().unwrap();
    d.inputs[0].items = vec![Sel::Profile { sketch, index: 0 }];
    set_operation(&mut h, 1);
    h
}

/// Choose the operation by hand (an index of `dialogs::OPS`), as the Operation row does.
fn set_operation(h: &mut Harness, op: usize) {
    if let Some(Kind::Extrude { operation, auto_op, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        *operation = op;
        *auto_op = false;
    }
    h.frames(3);
}

/// Pick the To Object at `p` as a click into the To Object box does.
fn pick_to(h: &mut Harness, p: Vec3) {
    let sel = face_sel(&h.app.session, p).expect("a face there");
    let d = h.app.dialog.as_mut().unwrap();
    crate::dialogs::preset_to_object(d, true);
    d.inputs.iter_mut().find(|i| i.label == "To Object").unwrap().items = vec![sel];
    h.frames(3);
}

fn target_bodies(d: &Dialog) -> Vec<String> {
    d.inputs.iter().find(|i| i.label == "Target Bodies").map(|i| i.items.body_names()).unwrap_or_default()
}

fn remove_target(h: &mut Harness, name: &str) {
    let d = h.app.dialog.as_mut().unwrap();
    d.inputs.iter_mut().find(|i| i.label == "Target Bodies").unwrap().items.retain(|x| !matches!(x, Sel::Body { name: n } if n == name));
    h.frames(3);
}

/// The parameters OK gives `solid.extrude`, directly or through an edit's `timeline.redefine`.
fn extrude_params(h: &Harness) -> serde_json::Value {
    let commands = apply_commands(&h.app, dialog(h)).unwrap();
    commands
        .into_iter()
        .find_map(|(id, p)| match id.as_str() {
            "solid.extrude" => Some(p),
            "timeline.redefine" if p["command"] == "solid.extrude" => Some(p["params"].clone()),
            _ => None,
        })
        .expect("an extrude")
}

/// What the empty Target Bodies chip says.
fn targets_chip(h: &Harness) -> Option<&'static str> {
    let d = dialog(h);
    crate::dialogs::empty_field_prompt(d, d.inputs.iter().find(|i| i.label == "Target Bodies").expect("Target Bodies"))
}

/// Target Bodies shows what OK stores as soon as it is known: the body the profile was sketched
/// on, for Join, Cut and Intersect alike, and for a Join To Object the body it runs to (#6424).
#[test]
fn target_bodies_follow_the_default_as_the_picks_change() {
    let mut h = join_to_scene();
    assert_eq!(target_bodies(dialog(&h)), ["Base"], "the sketch's face's body, before OK");
    pick_to(&mut h, Vec3::new(20.0, 20.0, 40.0));
    assert_eq!(target_bodies(dialog(&h)), ["Base", "Target"]);
    let params = extrude_params(&h);
    assert_eq!(params["targets"], json!(["Base", "Target"]));
    assert_eq!(params["to_body_apart"], json!(true));
    assert_eq!(params["participants"], json!([]), "Target Bodies alone say which bodies take part");
    for op in [2, 3] {
        set_operation(&mut h, op);
        assert_eq!(target_bodies(dialog(&h)), ["Base"], "operation {op}: the profile's body, not the To body");
        assert_eq!(extrude_params(&h)["targets"], json!(["Base"]));
    }
    set_operation(&mut h, 1);
    assert_eq!(target_bodies(dialog(&h)), ["Base", "Target"]);
    set_operation(&mut h, 0);
    assert!(dialog(&h).inputs.iter().all(|i| i.label != "Target Bodies"), "a new body has none");
}

/// Until a face or profile is picked, Target Bodies is empty and says All: no body is known yet.
#[test]
fn target_bodies_wait_for_a_face_or_profile() {
    let mut h = join_to_scene();
    h.app.dialog.as_mut().unwrap().inputs[0].items.clear();
    h.frames(3);
    assert!(target_bodies(dialog(&h)).is_empty());
    assert_eq!(targets_chip(&h), Some("All"));
    let sketch = h.app.session.doc.find_feature("Top").unwrap().id;
    h.app.dialog.as_mut().unwrap().inputs[0].items = vec![Sel::Profile { sketch, index: 0 }];
    h.frames(3);
    assert_eq!(target_bodies(dialog(&h)), ["Base"]);
}

/// A body taken out of Target Bodies by hand stays out (#18428), and the Join leaves it alone;
/// cleared, the box says All and stays so, and OK names no bodies (`all_bodies_touched`).
#[test]
fn a_target_taken_out_by_hand_stays_out() {
    let mut h = join_to_scene();
    pick_to(&mut h, Vec3::new(20.0, 20.0, 40.0));
    remove_target(&mut h, "Target");
    assert_eq!(target_bodies(dialog(&h)), ["Base"]);
    set_operation(&mut h, 2);
    set_operation(&mut h, 1);
    assert_eq!(target_bodies(dialog(&h)), ["Base"], "a choice by hand stays");
    assert_eq!(extrude_params(&h)["targets"], json!(["Base"]));
    h.app.run("solid.extrude", extrude_params(&h)).unwrap();
    let state = h.app.session.model.state();
    assert_eq!(state.bodies.len(), 2, "the target stays its own body");
    assert!((solidcraft_engine::doc::kernel::measure(&state.body("Target").unwrap().body).unwrap().volume - 32_000.0).abs() < 1e-6);
    let mut h = join_to_scene();
    pick_to(&mut h, Vec3::new(20.0, 20.0, 40.0));
    remove_target(&mut h, "Target");
    remove_target(&mut h, "Base");
    assert!(target_bodies(dialog(&h)).is_empty(), "a cleared box stays cleared");
    assert_eq!(targets_chip(&h), Some("All"));
    pick_to(&mut h, Vec3::new(20.0, 20.0, 60.0));
    set_operation(&mut h, 2);
    set_operation(&mut h, 1);
    assert!(target_bodies(dialog(&h)).is_empty(), "nothing is put back in");
    let params = extrude_params(&h);
    assert!(params.get("targets").is_none());
    assert_eq!(params["all_bodies_touched"], json!(true));
    h.app.run("solid.extrude", params).unwrap();
    assert_eq!(h.app.session.model.state().bodies.len(), 1, "every body it touches joins, the To body among them");
}

/// Hidden bodies get no rule of their own in Target Bodies: the profile's body fills the box
/// whatever else is hidden, a profile on no body leaves it All, and All cuts a hidden body too,
/// storing no bodies, so showing it again changes nothing.
#[test]
fn hidden_bodies_get_no_rule_of_their_own_in_target_bodies() {
    let mut h = join_to_scene();
    h.app.dialog = None;
    h.app.run("browser.visibility", json!({"items": ["Target"], "visible": false})).unwrap();
    let sketch = h.app.session.doc.find_feature("Top").unwrap().id;
    h.app.start("solid.extrude");
    h.frames(2);
    h.app.dialog.as_mut().unwrap().inputs[0].items = vec![Sel::Profile { sketch, index: 0 }];
    set_operation(&mut h, 1);
    assert_eq!(target_bodies(dialog(&h)), ["Base"], "the profile's body, not the shown ones");
    h.app.dialog = None;
    let floor = floor_sketch(&mut h);
    h.app.start("solid.extrude");
    h.frames(2);
    h.app.dialog.as_mut().unwrap().inputs[0].items = vec![Sel::Profile { sketch: floor, index: 0 }];
    set_operation(&mut h, 2);
    assert!(target_bodies(dialog(&h)).is_empty());
    assert_eq!(targets_chip(&h), Some("All"));
    let mut params = extrude_params(&h);
    assert!(params.get("targets").is_none());
    assert_eq!(params["all_bodies_touched"], json!(true));
    assert_eq!(params["participants"], json!([]));
    // Through both: the base (16 000 mm³) and the hidden target (32 000) each lose 20 × 20 × their height.
    params["distance"] = json!("70 mm");
    params["direction"] = json!("positive");
    let id = h.app.run("solid.extrude", params).unwrap()["feature"].as_u64().unwrap();
    assert!(h.app.session.doc.feature(id).unwrap().participants.is_empty(), "no shown bodies kept");
    check_volumes(&h, &[("Base", 12_000.0), ("Target", 24_000.0)]);
    h.app.run("browser.visibility", json!({"items": ["Target"], "visible": true})).unwrap();
    check_volumes(&h, &[("Base", 12_000.0), ("Target", 24_000.0)]);
}

/// An extrude made with no targets while a body was hidden keeps the shown bodies as its
/// participants (#2464). Editing it shows them in Target Bodies, and clearing the box there makes
/// it take every body it touches.
#[test]
fn editing_an_extrude_shows_its_participants_in_target_bodies() {
    let mut h = join_to_scene();
    h.app.dialog = None;
    let floor = floor_sketch(&mut h);
    h.app.run("browser.visibility", json!({"items": ["Target"], "visible": false})).unwrap();
    h.app.run("solid.extrude", json!({"sketch": floor, "distance": 70, "operation": "cut", "name": "Hole"})).unwrap();
    h.app.run("browser.visibility", json!({"items": ["Target"], "visible": true})).unwrap();
    assert_eq!(h.app.session.doc.find_feature("Hole").unwrap().participants, ["Base"]);
    check_volumes(&h, &[("Base", 12_000.0), ("Target", 32_000.0)]);
    h.step(&json!({"call": "ui.editFeature", "params": {"feature": "Hole"}})).unwrap();
    h.frames(8);
    assert_eq!(target_bodies(dialog(&h)), ["Base"], "what it changes");
    remove_target(&mut h, "Base");
    assert_eq!(targets_chip(&h), Some("All"));
    h.step(&json!({"click": {"handle": "dialog:ok"}})).unwrap();
    h.frames(8);
    assert!(h.app.dialog.is_none(), "OK closed it");
    assert!(h.app.session.doc.find_feature("Hole").unwrap().participants.is_empty());
    check_volumes(&h, &[("Base", 12_000.0), ("Target", 24_000.0)]);
}

/// A 20 × 20 profile on the XY plane under the base, on no body's face: the sketch's id.
fn floor_sketch(h: &mut Harness) -> u64 {
    let id = h.app.run("sketch.create", json!({"plane": "XY", "name": "Floor"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [10, 10], "p1": [30, 30]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    id
}

fn check_volumes(h: &Harness, expected: &[(&str, f64)]) {
    let state = h.app.session.model.state();
    for (name, volume) in expected {
        let measured = solidcraft_engine::doc::kernel::measure(&state.body(name).unwrap().body).unwrap().volume;
        assert!((measured - volume).abs() < 1e-6, "{name}: {measured}, expected {volume}");
    }
}

/// Editing a Join To Object made before the body it runs to was one of its targets, which joins
/// that body whatever they say, shows that body in Target Bodies, and OK makes them decide.
/// One made since keeps the targets it was given.
#[test]
fn editing_a_join_to_object_shows_the_body_it_joins_in_target_bodies() {
    for (apart, shown) in [(false, vec!["Base", "Target"]), (true, vec!["Base"])] {
        let mut h = join_to_scene();
        h.app.dialog = None;
        let sketch = h.app.session.doc.find_feature("Top").unwrap().id;
        h.app
            .run(
                "solid.extrude",
                json!({"sketch": sketch, "to": [20, 20, 40], "operation": "join", "targets": ["Base"], "to_body_apart": apart, "name": "Post"}),
            )
            .unwrap();
        h.step(&json!({"call": "ui.editFeature", "params": {"feature": "Post"}})).unwrap();
        h.frames(8);
        assert_eq!(target_bodies(dialog(&h)), shown, "to_body_apart: {apart}");
        let params = extrude_params(&h);
        assert_eq!(params["targets"], json!(shown));
        assert_eq!(params["to_body_apart"], json!(true));
    }
}
