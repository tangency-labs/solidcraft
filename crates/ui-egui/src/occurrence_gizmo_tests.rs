//! Moving a component with Move/Copy's triad from its Browser menu: the component moves by its
//! placement when OK is pressed, as typed values move it, independent of clipboard paste.
use crate::{
    context_menu::{self, Target},
    dialogs::Kind,
    scenario::Harness,
};
use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::{Sel, doc};

/// Opens Move/Copy on the component placed at `path` from its Browser menu.
fn open_move(h: &mut Harness, path: Vec<u64>) {
    let target = Target::Occurrence { path: path.clone() };
    let item = context_menu::items(&h.app, &target).into_iter().find(|item| item.id == "ui.moveOccurrence").unwrap();
    context_menu::run_item(&mut h.app, &item, egui::Pos2::ZERO);
    h.frames(3);
    assert_eq!(crate::dialogs_move::moving(&h.app), Some(path), "Move/Copy opens on the component");
}

/// The open Move/Copy dialog's X distance (mm) and angles (degrees).
fn values(h: &Harness) -> (f64, [f64; 3]) {
    let Some(Kind::Move { x, rx, ry, rz, .. }) = h.app.dialog.as_ref().map(|d| &d.kind) else { panic!("no Move/Copy open") };
    let number = |s: &str, unit: &str| s.trim_end_matches(unit).trim().parse::<f64>().unwrap();
    (number(x, "mm"), [rx, ry, rz].map(|a| number(a, "deg")))
}

fn press_ok(h: &mut Harness) {
    h.step(&json!({"click": {"handle": "dialog:ok"}})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog.as_ref().and_then(|d| d.error.clone()));
}

fn origin(h: &Harness, path: &[u64]) -> Vec3 {
    doc::apply_point(&h.app.session.occurrence_placement(path).unwrap().world, Vec3::ZERO)
}

#[test]
fn occurrence_gizmo_drag_matches_numeric_input_and_preserves_joints_capture_and_undo() {
    let mut h = Harness::new();
    let part = h.app.run("component.create", json!({"name":"Block","grounded":false})).unwrap();
    let first = part["occurrence"].as_u64().unwrap();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
    h.app.run("component.activate", json!({"component":0})).unwrap();
    let second = h.app.run("occurrence.copy", json!({"occurrence":first,"translate":[50,0,0]})).unwrap()["occurrence"].as_u64().unwrap();
    h.app
        .run(
            "joint.create",
            json!({"name":"Fixed peer","type":"rigid","flip":true,"a":{"occurrence":0,"point":[50,0,0]},"b":{"occurrence":second,"point":[50,0,0]}}),
        )
        .unwrap();
    let joints = serde_json::to_value(&h.app.session.doc.assembly.joints).unwrap();
    let original = h.app.session.doc.to_json();
    let initial = origin(&h, &[first]);
    let peer = origin(&h, &[second]);
    h.app.cam.target = initial;
    h.app.cam.distance = 100.;
    h.frames(3);
    open_move(&mut h, vec![first]);
    let camera = h.app.cam;
    let start = crate::gizmo::handle_point("x", Vec3::ZERO, 60.).unwrap();
    let end = start + Vec3::X * 12.;
    h.step(&json!({"drag":[{"world":[start.x,start.y,start.z]},{"world":[end.x,end.y,end.z]}],"steps":12})).unwrap();
    h.frames(3);
    let (dragged, _) = values(&h);
    assert!(dragged > 1., "the arrow sets the X distance: {dragged}");
    assert_eq!(h.app.session.doc.to_json(), original, "nothing moves before OK: the preview draws it");
    assert!(h.app.session.pending_moves.is_empty());
    press_ok(&mut h);
    let arrow = origin(&h, &[first]);
    assert!((arrow.x - initial.x - dragged).abs() < 1e-6, "the arrow moves this occurrence: {initial:?}->{arrow:?}");
    assert_eq!(h.app.cam, camera);
    assert!(h.app.session.pending_moves.is_empty(), "Capture All Positions is ticked at first");
    assert_eq!(origin(&h, &[second]), peer);
    assert_eq!(serde_json::to_value(&h.app.session.doc.assembly.joints).unwrap(), joints);
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), original, "one undo takes the move back");

    open_move(&mut h, vec![first]);
    if let Some(Kind::Move { x, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        *x = format!("{dragged} mm");
    }
    h.frames(2);
    press_ok(&mut h);
    let numeric = origin(&h, &[first]);
    assert!((numeric - arrow).len() < 1e-6, "numeric and arrow routes must agree: {numeric:?}, {arrow:?}");
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), original);
    h.app.run("edit.redo", json!({})).unwrap();
    assert!((origin(&h, &[first]) - numeric).len() < 1e-6);
    assert_eq!(serde_json::to_value(&h.app.session.doc.assembly.joints).unwrap(), joints);
    assert!(!h.app.session.selection.contains(&Sel::Occurrence { path: vec![second] }));
}

/// A second ring reads what was dragged and turns about the world axis it shows, on top of the
/// first turn.
#[test]
fn occurrence_rings_turn_about_the_axes_they_show() {
    let mut h = Harness::new();
    let part = h.app.run("component.create", json!({"name":"Block","grounded":false})).unwrap();
    let first = part["occurrence"].as_u64().unwrap();
    h.app.run("solid.box", json!({"length":10,"width":20,"height":30})).unwrap();
    h.app.run("component.activate", json!({"component":0})).unwrap();
    h.app.cam = h.app.cam.looking_from(Vec3::new(1.0, 0.8, 0.6));
    h.app.cam.target = Vec3::ZERO;
    h.app.cam.distance = 200.;
    h.frames(3);
    open_move(&mut h, vec![first]);
    if let Some(Kind::Move { ry, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        *ry = "40 deg".into();
    }
    h.frames(3);
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    let t = crate::dialogs_move::triad(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    let grip = crate::gizmo::ring_grip(&proj, &t, 0).unwrap();
    let [start, end] = [grip, grip + 35.].map(|a| crate::gizmo::handle_point("rx", Vec3::ZERO, a).unwrap());
    h.step(&json!({"drag":[{"world":[start.x,start.y,start.z]},{"world":[end.x,end.y,end.z]}],"steps":12})).unwrap();
    h.frames(3);
    let (_, angles) = values(&h);
    assert_eq!(angles.map(|a| a.round()), [35., 40., 0.]);
    press_ok(&mut h);
    let rotation = crate::gizmo::rotation;
    let want = crate::gizmo::mul(&rotation(Vec3::X, angles[0].to_radians()), &rotation(Vec3::Y, 40_f64.to_radians()));
    let world = h.app.session.occurrence_placement(&[first]).unwrap().world;
    let rows = [0, 1, 2].map(|r| [world[0][r], world[1][r], world[2][r]]);
    for (got, want) in rows.iter().flatten().zip(want.iter().flatten()) {
        assert!((got - want).abs() < 1e-6, "{world:?}");
    }
}

/// A ring dragged out and back to where it started leaves the occurrence where it was.
#[test]
fn an_occurrence_ring_dragged_back_to_its_start_leaves_it_there() {
    let mut h = Harness::new();
    let part = h.app.run("component.create", json!({"name":"Block","grounded":false})).unwrap();
    let first = part["occurrence"].as_u64().unwrap();
    h.app.run("solid.box", json!({"length":10,"width":20,"height":30})).unwrap();
    h.app.run("component.activate", json!({"component":0})).unwrap();
    h.app.cam = h.app.cam.looking_from(Vec3::new(1.0, 0.8, 0.6));
    h.app.cam.target = Vec3::ZERO;
    h.app.cam.distance = 200.;
    h.frames(3);
    let before = h.app.session.occurrence_placement(&[first]).unwrap().world;
    open_move(&mut h, vec![first]);

    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    let t = crate::dialogs_move::triad(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    let grip = crate::gizmo::ring_grip(&proj, &t, 0).unwrap();
    let [start, out] = [grip, grip + 20.].map(|a| crate::gizmo::handle_point("rx", Vec3::ZERO, a).unwrap());
    let drag = [start, out, start].map(|p| json!({"world": [p.x, p.y, p.z]}));
    h.step(&json!({"drag": drag, "steps": 12})).unwrap();
    h.frames(3);
    assert_eq!(values(&h).1, [0., 0., 0.]);
    press_ok(&mut h);
    let world = h.app.session.occurrence_placement(&[first]).unwrap().world;
    for (got, want) in world.iter().flatten().zip(before.iter().flatten()) {
        assert!((got - want).abs() < 1e-9, "{world:?} != {before:?}");
    }
}

/// The triad sits on the part, wherever its component's origin is: a part modelled away from
/// that origin must not be turned and moved from over there. The part turns on the spot.
#[test]
fn occurrence_triad_sits_on_the_part_and_turns_it_there() {
    let mut h = Harness::new();
    let part = h.app.run("component.create", json!({"name":"Block","grounded":false})).unwrap();
    let first = part["occurrence"].as_u64().unwrap();
    h.app.run("solid.box", json!({"length":10,"width":20,"height":30,"corner":[100,40,0]})).unwrap();
    h.app.run("component.activate", json!({"component":0})).unwrap();
    let centre = Vec3::new(105., 50., 15.);
    h.app.cam = h.app.cam.looking_from(Vec3::new(1.0, 0.8, 0.6));
    h.app.cam.target = centre;
    h.app.cam.distance = 200.;
    h.frames(3);
    open_move(&mut h, vec![first]);
    let t = crate::dialogs_move::triad(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert!((t.center - centre).len() < 1e-6, "triad at {:?}, part at {centre:?}", t.center);

    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    let grip = crate::gizmo::ring_grip(&proj, &t, 0).unwrap();
    let [start, end] = [grip, grip + 35.].map(|a| crate::gizmo::handle_point("rx", Vec3::ZERO, a).unwrap());
    h.step(&json!({"drag":[{"world":[start.x,start.y,start.z]},{"world":[end.x,end.y,end.z]}],"steps":12})).unwrap();
    h.frames(3);
    assert_eq!(values(&h).1.map(|a| a.round()), [35., 0., 0.]);
    press_ok(&mut h);
    let bounds = h.app.session.world_state().unwrap().bodies.first().unwrap().mesh().bounds();
    assert!((bounds.center() - centre).len() < 1e-6, "the part stays where it is: {bounds:?}");
}
