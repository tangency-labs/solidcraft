//! A Single hole placed by distances from reference edges (#37): the centre sits at the typed
//! distances from straight edges of its face, measured in the face's plane, follows the edges
//! when the part changes (also after the design is saved and reopened), and a bad reference is
//! refused with an error.

use std::f64::consts::PI;

use serde_json::{Value, json};
use solidcraft_doc::FeatureKind;
use solidcraft_geom::Vec3;

use crate::{EngineError, Session};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn solid_at(s: &Session, p: Vec3) -> bool {
    s.model.state().bodies.iter().any(|b| b.body.contains(p))
}

/// The bore (5 mm across, 4 mm deep, down from the top face at z = `top`) is about (x, y):
/// empty on its axis and up to 2 mm from it, solid 3 mm from it in every direction (the margins
/// leave room for the facets of a larger body's coarser mesh).
fn bore_at(s: &Session, x: f64, y: f64, top: f64) {
    let at = Vec3::new(x, y, top - 2.0);
    for k in 0..8 {
        let a = PI / 4.0 * f64::from(k);
        let p = |r: f64| at + Vec3::new(a.cos(), a.sin(), 0.0) * r;
        assert!(!solid_at(s, p(0.0)) && !solid_at(s, p(2.0)), "the bore about ({x}, {y}) is empty {a} rad from +x");
        assert!(solid_at(s, p(3.0)), "the wall about ({x}, {y}) is solid {a} rad from +x");
    }
}

/// The last feature's position and reference edges.
fn hole(s: &Session) -> (Vec3, Vec<solidcraft_doc::HoleReference>) {
    match &s.doc.features.last().map(|f| &f.kind) {
        Some(FeatureKind::Hole { position, references, .. }) => (position.0, references.clone()),
        other => panic!("the last feature is not a hole: {other:?}"),
    }
}

/// A 50 × 40 × 10 box from the origin whose length and width are parameters.
fn plate() -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "len", "expression": "50 mm"}));
    run(&mut s, "parameters.add", json!({"name": "wid", "expression": "40 mm"}));
    run(&mut s, "solid.box", json!({"length": "len", "width": "wid", "height": 10, "corner": [0, 0, 0]}));
    s
}

/// A Ø5 × 4 hole picked at (30, 25) on the top, with these references.
fn referenced(references: Value) -> Value {
    json!({"position": [30, 25, 10], "diameter": 5, "depth": 4, "name": "H", "references": references})
}

#[test]
fn a_hole_sits_at_its_distances_from_two_edges() {
    let mut s = plate();
    // 10 mm from the left edge (x = 0) and 12 mm from the front edge (y = 0) of the top.
    run(&mut s, "solid.hole", referenced(json!([{"edge": [0, 20, 10], "distance": 10}, {"edge": [25, 0, 10], "distance": "12 mm"}])));
    let (position, refs) = hole(&s);
    assert!(position.dist(Vec3::new(10.0, 12.0, 10.0)) < 1e-6, "the hole is placed at (10, 12), not {position:?}");
    assert_eq!(refs.len(), 2);
    bore_at(&s, 10.0, 12.0, 10.0);
    assert!(solid_at(&s, Vec3::new(30.0, 25.0, 8.0)), "nothing is drilled where the face was picked");
    // The distances are the hole's parameters, named like its other inputs.
    let f = s.doc.find_feature("H").unwrap();
    let labels: Vec<&str> = f.kind.inputs().iter().map(|(label, _, _)| *label).collect();
    assert_eq!(labels.iter().filter(|l| **l == "Reference distance").count(), 2, "{labels:?}");
    assert_eq!(f.param_names.len(), labels.len());
    assert_eq!(f.edge_names.len(), 2, "each reference edge keeps its persistent name");
    // One reference: the hole moves square to that edge and keeps its place along it.
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "solid.hole", referenced(json!([{"edge": [0, 20, 10], "distance": 10}])));
    bore_at(&s, 10.0, 25.0, 10.0);
}

#[test]
fn a_hole_follows_its_edges_when_the_box_grows() {
    let mut s = plate();
    // From the far edges: 10 mm from x = len, 12 mm from y = wid.
    run(&mut s, "solid.hole", referenced(json!([{"edge": [50, 20, 10], "distance": 10}, {"edge": [25, 40, 10], "distance": 12}])));
    bore_at(&s, 40.0, 28.0, 10.0);
    let id = s.doc.find_feature("H").unwrap().id;
    run(&mut s, "parameters.change", json!({"name": "len", "expression": "80 mm"}));
    run(&mut s, "parameters.change", json!({"name": "wid", "expression": "65 mm"}));
    assert!(s.model.result(id).is_some_and(|r| r.error.is_none()), "{:?}", s.model.result(id).map(|r| &r.error));
    bore_at(&s, 70.0, 53.0, 10.0);
    assert!(solid_at(&s, Vec3::new(40.0, 28.0, 8.0)), "the old spot is solid again");
    // A distance is a parameter too: changing it moves the hole.
    let d = s.doc.find_feature("H").unwrap().param_names.last().cloned().unwrap();
    run(&mut s, "parameters.change", json!({"name": d, "expression": "20 mm"}));
    bore_at(&s, 70.0, 45.0, 10.0);
    // Smaller than where the hole was placed: it still follows its edges.
    run(&mut s, "parameters.change", json!({"name": "len", "expression": "30 mm"}));
    bore_at(&s, 20.0, 45.0, 10.0);
    // Saved and reopened, the hole keeps its references and its place.
    let mut t = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    t.refresh();
    bore_at(&t, 20.0, 45.0, 10.0);
    assert_eq!(hole(&t).1, hole(&s).1);
}

#[test]
fn a_bad_reference_is_refused() {
    let mut s = plate();
    run(&mut s, "solid.hole", json!({"position": [25, 20, 10], "diameter": 10, "name": "Bore"}));
    let before = s.doc.features.len();
    let edge = |p: [f64; 3], d: Value| json!({"edge": p, "distance": d});
    for (refs, why) in [
        (json!([edge([25.0, 30.0, 10.0], json!(5))]), "no edge"),
        (json!([edge([30.0, 20.0, 10.0], json!(5))]), "not straight"),
        (json!([edge([0.0, 0.0, 5.0], json!(5))]), "square to the face"),
        (json!([edge([0.0, 20.0, 10.0], json!(5)), edge([50.0, 20.0, 10.0], json!(5))]), "parallel"),
        (json!([edge([0.0, 20.0, 10.0], json!(5)), edge([25.0, 0.0, 10.0], json!(5)), edge([50.0, 20.0, 10.0], json!(5))]), "at most 2"),
        (json!([edge([0.0, 20.0, 10.0], json!("-5 mm"))]), "0 or more"),
        (json!([edge([0.0, 20.0, 10.0], json!(500))]), "off its face"),
        (json!([edge([0.0, 20.0, 10.0], json!("5 deg"))]), "references"),
        (json!([edge([0.0, 20.0, 10.0], json!("nope"))]), "references"),
        (json!([{"edge": [0, 20], "distance": 5}]), "edge"),
        (json!([{"edge": [0, 20, 10]}]), "distance"),
        (json!({"edge": [0, 20, 10], "distance": 5}), "list"),
    ] {
        let p = referenced(refs.clone());
        match s.execute("solid.hole", &p) {
            Err(EngineError::Internal(..)) | Ok(_) => panic!("{refs} is refused"),
            Err(e) => assert!(e.to_string().contains(why), "{refs}: {e}"),
        }
        assert_eq!(s.doc.features.len(), before, "{refs} adds nothing");
    }
    // Not with sketch points or a position given as expressions.
    for p in [
        json!({"position": ["30 mm", "25 mm", "10 mm"], "diameter": 5, "references": [edge([0.0, 20.0, 10.0], json!(5))]}),
        json!({"sketch": 1, "points": ["p1"], "diameter": 5, "references": [edge([0.0, 20.0, 10.0], json!(5))]}),
    ] {
        assert!(s.execute("solid.hole", &p).is_err(), "{p}");
    }
}

#[test]
fn hostile_references_never_panic() {
    let mut s = plate();
    let values = crate::tests::hostile_values();
    let mut panicked = Vec::new();
    for v in &values {
        for refs in [
            v.clone(),
            json!([v]),
            json!([v, v]),
            json!([{"edge": v, "distance": 5}]),
            json!([{"edge": [0, 20, 10], "distance": v}]),
            json!([{"edge": [0, 20, 10], "distance": 5}, {"edge": v, "distance": v}]),
        ] {
            for at in [json!([30, 25, 10]), json!([0, 0, 10]), json!([1e300, 0, 10]), v.clone()] {
                let p = json!({"position": at, "diameter": 5, "references": refs});
                if let Err(EngineError::Internal(id, msg)) = s.execute("solid.hole", &p) {
                    panicked.push(format!("{id} {p}: {msg}"));
                }
            }
        }
    }
    assert!(panicked.is_empty(), "solid.hole panicked:\n{}", panicked.join("\n"));
}
