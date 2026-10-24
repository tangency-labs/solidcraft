//! A Single hole placed by distances from reference edges, through the window (#37): pick the
//! face, pick two edges (each distance is measured from the pick), type the distances (one in
//! the dialog, one in its box on the canvas), OK. The hole sits at those distances; Edit Feature
//! picks the edges again with their distances, and moving the hole's place measures them again.
//! A Cylinder standing on a picked face takes reference edges the same way (#6380).

use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::doc::FeatureKind;
use solidcraft_engine::geom::Vec3;

use crate::dialogs::Kind;
use crate::scenario::Harness;

/// Where a world point is on screen.
fn screen(h: &mut Harness, p: Vec3) -> [f64; 2] {
    h.at(&json!({"world": [p.x, p.y, p.z]})).unwrap()
}

/// A 50 × 40 × 10 plate from the origin, seen from the top.
fn plate() -> Harness {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.run("solid.box", json!({"length": 50, "width": 40, "height": 10, "corner": [0, 0, 0]})).unwrap();
    h.step(&json!({"call": "ui.view", "params": {"view": "top"}})).unwrap();
    h.settle();
    h
}

/// The open Hole dialog's reference distances.
fn distances(h: &Harness) -> [String; 2] {
    match h.app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::Hole { opts, .. }) => opts.refs.distance.clone(),
        _ => panic!("the hole dialog is open"),
    }
}

/// Where the open dialog places the hole (its face pick).
fn place(h: &Harness) -> Vec3 {
    match h.app.dialog.as_ref().and_then(|d| d.inputs.first()).and_then(|i| i.items.first()) {
        Some(Sel::Face { point, .. }) => *point,
        other => panic!("a face place, not {other:?}"),
    }
}

/// The edge picked into the open dialog's input `label`.
fn picked_edge(h: &Harness, label: &str) -> Option<Vec3> {
    let d = h.app.dialog.as_ref()?;
    match d.inputs.iter().find(|i| i.label == label)?.items.first()? {
        Sel::Edge { point, .. } => Some(*point),
        _ => None,
    }
}

/// Replace the text of the field at `handle` with `text`.
fn type_into(h: &mut Harness, handle: &str, text: &str) {
    h.step(&json!({"click": {"handle": handle}})).unwrap();
    h.step(&json!({"key": "A", "cmd": true})).unwrap();
    h.step(&json!({"text": text})).unwrap();
    h.frames(2);
}

/// Whether the model holds material at `p`.
fn solid_at(h: &Harness, p: Vec3) -> bool {
    h.app.session.model.state().bodies.iter().any(|b| b.body.contains(p))
}

#[test]
fn a_hole_is_placed_by_its_distances_from_two_edges() {
    let mut h = plate();
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let at = screen(&mut h, Vec3::new(30.0, 25.0, 10.0));
    h.step(&json!({"click": at})).unwrap();
    assert!(place(&h).dist(Vec3::new(30.0, 25.0, 10.0)) < 0.5, "the face is picked where clicked: {:?}", place(&h));
    let picked = place(&h);
    // The reference inputs come after the face; picking an edge measures its distance from the
    // pick, and the second input takes the next edge.
    h.step(&json!({"click": {"handle": "dialog:Reference 1"}})).unwrap();
    let left = screen(&mut h, Vec3::new(0.0, 20.0, 10.0));
    h.step(&json!({"click": left})).unwrap();
    let front = screen(&mut h, Vec3::new(25.0, 0.0, 10.0));
    h.step(&json!({"click": front})).unwrap();
    h.frames(2);
    assert!(picked_edge(&h, "Reference 1").is_some_and(|p| p.x.abs() < 1e-6), "the left edge is Reference 1: {:?}", picked_edge(&h, "Reference 1"));
    assert!(picked_edge(&h, "Reference 2").is_some_and(|p| p.y.abs() < 1e-6), "the front edge is Reference 2: {:?}", picked_edge(&h, "Reference 2"));
    let measured = distances(&h);
    let value = |s: &str| h.app.session.doc.eval(s, solidcraft_engine::doc::expr::Kind::Length).unwrap();
    assert!((value(&measured[0]) - picked.x).abs() < 1e-3 && (value(&measured[1]) - picked.y).abs() < 1e-3, "{measured:?} from {picked:?}");
    // The distances show on the canvas, each with a box to type in.
    assert!(h.at(&json!({"handle": "canvas:Distance 1"})).is_ok() && h.at(&json!({"handle": "canvas:Distance 2"})).is_ok());
    // Typed in the dialog and on the canvas: the hole goes there.
    type_into(&mut h, "dialog:Distance 1", "10 mm");
    type_into(&mut h, "canvas:Distance 2", "12 mm");
    assert_eq!(distances(&h), ["10 mm", "12 mm"]);
    assert!(place(&h).dist(Vec3::new(10.0, 12.0, 10.0)) < 1e-6, "the hole moved to its distances: {:?}", place(&h));
    h.step(&json!({"widget": "OK"})).unwrap();
    assert!(h.app.dialog.is_none(), "OK closed the dialog: {:?}", h.app.dialog.as_ref().map(|d| &d.error));
    let f = h.app.session.doc.features.last().unwrap().clone();
    let FeatureKind::Hole { position, references, .. } = &f.kind else { panic!("the last feature is a hole: {}", f.kind.type_name()) };
    assert!(position.0.dist(Vec3::new(10.0, 12.0, 10.0)) < 1e-6, "{position:?}");
    assert_eq!(references.iter().map(|r| r.distance.to_string()).collect::<Vec<_>>(), ["10 mm", "12 mm"]);
    assert!(h.app.session.model.result(f.id).is_some_and(|r| r.error.is_none()));
    assert!(
        !solid_at(&h, Vec3::new(10.0, 12.0, 8.0)) && solid_at(&h, Vec3::new(30.0, 25.0, 8.0)),
        "drilled at (10, 12), not where the face was picked"
    );

    // Edit Feature: the edges picked again with their distances, the hole where they put it.
    h.step(&json!({"call": "ui.editFeature", "params": {"feature": f.id}})).unwrap();
    h.frames(4);
    assert!(picked_edge(&h, "Reference 1").is_some_and(|p| p.x.abs() < 1e-6), "{:?}", picked_edge(&h, "Reference 1"));
    assert!(picked_edge(&h, "Reference 2").is_some_and(|p| p.y.abs() < 1e-6), "{:?}", picked_edge(&h, "Reference 2"));
    assert_eq!(distances(&h), ["10 mm", "12 mm"]);
    assert!(place(&h).dist(Vec3::new(10.0, 12.0, 10.0)) < 1e-6, "{:?}", place(&h));
    // The hole moved on its face (a new pick, or its handle dragged): the distances follow.
    if let Some(Sel::Face { point, .. }) = h.app.dialog.as_mut().and_then(|d| d.inputs.first_mut()).and_then(|i| i.items.first_mut()) {
        *point = Vec3::new(20.0, 30.0, 10.0);
    }
    h.frames(2);
    assert_eq!(distances(&h), ["20 mm", "30 mm"]);
    h.step(&json!({"widget": "OK"})).unwrap();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog.as_ref().map(|d| &d.error));
    assert!(!solid_at(&h, Vec3::new(20.0, 30.0, 8.0)) && solid_at(&h, Vec3::new(10.0, 12.0, 8.0)), "the edit moved the hole to (20, 30)");
}

/// Reference edges place one hole: with places on two faces, OK says to pick one.
#[test]
fn reference_edges_place_one_hole() {
    let mut h = plate();
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let at = screen(&mut h, Vec3::new(30.0, 25.0, 10.0));
    h.step(&json!({"click": at})).unwrap();
    // A second hole on another face (the bottom), as a pick there would place it.
    let d = h.app.dialog.as_mut().unwrap();
    let Some(Sel::Face { body, index, .. }) = d.inputs[0].items.first().cloned() else { panic!("a face place") };
    d.inputs[0].items.push(Sel::Face { body, index: index + 1, point: Vec3::new(40.0, 10.0, 0.0) });
    h.step(&json!({"click": {"handle": "dialog:Reference 1"}})).unwrap();
    let left = screen(&mut h, Vec3::new(0.0, 20.0, 10.0));
    h.step(&json!({"click": left})).unwrap();
    h.frames(2);
    let d = h.app.dialog.as_ref().unwrap();
    let err = crate::dialogs::apply_commands(&h.app, d).unwrap_err();
    assert!(err.contains("one hole"), "{err}");
}

/// A number typed straight after an edge pick is that edge's distance, not the diameter.
#[test]
fn a_number_typed_after_an_edge_pick_sets_its_distance() {
    let mut h = plate();
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let at = screen(&mut h, Vec3::new(30.0, 25.0, 10.0));
    h.step(&json!({"click": at})).unwrap();
    h.step(&json!({"click": {"handle": "dialog:Reference 1"}})).unwrap();
    let left = screen(&mut h, Vec3::new(0.0, 20.0, 10.0));
    h.step(&json!({"click": left})).unwrap();
    h.frames(2);
    h.step(&json!({"text": "7 mm"})).unwrap();
    h.frames(2);
    assert_eq!(distances(&h)[0], "7 mm");
    assert!((place(&h).x - 7.0).abs() < 1e-6, "the hole is 7 mm from the left edge: {:?}", place(&h));
    let Some(Kind::Hole { diameter, .. }) = h.app.dialog.as_ref().map(|d| &d.kind) else { panic!("the hole dialog") };
    assert_eq!(diameter, "5 mm", "the diameter is left as it was");
}

/// The open Cylinder dialog's reference distances and where it stands.
fn cylinder_dialog(h: &Harness) -> ([String; 2], Vec3) {
    match h.app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::Primitive { cmd: "solid.cylinder", refs, at: Some((p, _)), .. }) => (refs.distance.clone(), *p),
        other => panic!("the cylinder dialog is open on a face: {other:?}"),
    }
}

/// With the mouse alone: pick the top face, start Cylinder, pick two edges (each distance
/// measured from the pick), OK. Typing a distance moves the base; Edit Feature picks the edges
/// again.
#[test]
fn a_cylinder_is_placed_by_its_distances_from_two_edges() {
    let mut h = plate();
    let at = screen(&mut h, Vec3::new(30.0, 25.0, 10.0));
    h.step(&json!({"click": at})).unwrap();
    h.step(&json!({"start": "solid.cylinder"})).unwrap();
    h.frames(2);
    let (_, picked) = cylinder_dialog(&h);
    assert!(picked.dist(Vec3::new(30.0, 25.0, 10.0)) < 0.5, "it stands where the face was picked: {picked:?}");
    // Reference 1 takes the first edge picked, Reference 2 the next.
    let left = screen(&mut h, Vec3::new(0.0, 20.0, 10.0));
    h.step(&json!({"click": left})).unwrap();
    let front = screen(&mut h, Vec3::new(25.0, 0.0, 10.0));
    h.step(&json!({"click": front})).unwrap();
    h.frames(2);
    assert!(picked_edge(&h, "Reference 1").is_some_and(|p| p.x.abs() < 1e-6), "{:?}", picked_edge(&h, "Reference 1"));
    assert!(picked_edge(&h, "Reference 2").is_some_and(|p| p.y.abs() < 1e-6), "{:?}", picked_edge(&h, "Reference 2"));
    let (measured, _) = cylinder_dialog(&h);
    let value = |s: &str| h.app.session.doc.eval(s, solidcraft_engine::doc::expr::Kind::Length).unwrap();
    assert!((value(&measured[0]) - picked.x).abs() < 1e-3 && (value(&measured[1]) - picked.y).abs() < 1e-3, "{measured:?} from {picked:?}");
    assert!(h.at(&json!({"handle": "canvas:Distance 1"})).is_ok() && h.at(&json!({"handle": "canvas:Distance 2"})).is_ok());
    h.step(&json!({"widget": "OK"})).unwrap();
    assert!(h.app.dialog.is_none(), "OK closed the dialog: {:?}", h.app.dialog.as_ref().map(|d| &d.error));
    let f = h.app.session.doc.features.last().unwrap().clone();
    let FeatureKind::Cylinder { references, .. } = &f.kind else { panic!("the last feature is a cylinder: {}", f.kind.type_name()) };
    assert_eq!(references.len(), 2);
    assert_eq!(f.edge_names.len(), 2);
    assert!(h.app.session.model.result(f.id).is_some_and(|r| r.error.is_none()));

    // Edit Feature: the edges again with their distances; a typed distance moves the base.
    h.step(&json!({"call": "ui.editFeature", "params": {"feature": f.id}})).unwrap();
    h.frames(4);
    assert!(picked_edge(&h, "Reference 1").is_some_and(|p| p.x.abs() < 1e-6), "{:?}", picked_edge(&h, "Reference 1"));
    assert!(picked_edge(&h, "Reference 2").is_some_and(|p| p.y.abs() < 1e-6), "{:?}", picked_edge(&h, "Reference 2"));
    assert_eq!(cylinder_dialog(&h).0, measured);
    type_into(&mut h, "dialog:Distance 1", "10 mm");
    type_into(&mut h, "canvas:Distance 2", "12 mm");
    assert!(cylinder_dialog(&h).1.dist(Vec3::new(10.0, 12.0, 10.0)) < 1e-6, "{:?}", cylinder_dialog(&h).1);
    h.step(&json!({"widget": "OK"})).unwrap();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog.as_ref().map(|d| &d.error));
    assert!(solid_at(&h, Vec3::new(10.0, 12.0, 12.0)) && !solid_at(&h, Vec3::new(picked.x, picked.y, 12.0)), "the edit moved it to (10, 12)");
}

/// A cylinder placed at the origin (nothing picked) has no reference edges to take.
#[test]
fn a_cylinder_off_a_face_takes_no_reference_edges() {
    let mut h = plate();
    h.step(&json!({"start": "solid.cylinder"})).unwrap();
    h.frames(2);
    let d = h.app.dialog.as_ref().unwrap();
    assert!(d.inputs.is_empty() && !crate::hole_refs::takes(d));
}
