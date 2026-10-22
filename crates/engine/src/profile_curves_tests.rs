//! Profiles picked by the curves around them stay the same region when a parameter moves the
//! sketch, where a pick by point can land in another region.

use serde_json::{Value, json};

use crate::Session;
use solidcraft_doc::{FeatureKind, ProfileSel};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn volumes(s: &Session) -> Vec<f64> {
    s.model.state().bodies.iter().map(|b| b.body.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-2)).unwrap().measure().volume).collect()
}

fn last_error(s: &Session) -> Option<String> {
    let id = s.doc.features.last().unwrap().id;
    s.model.result(id).and_then(|r| r.error.clone())
}

/// A 60 × 20 tray split in two by a divider `split` mm from its left side: the left bay is
/// bounded by the left side, the divider and the pieces of the bottom and top the divider cuts.
fn tray() -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "split", "expression": "30 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Tray"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [60, 0], [60, 20], [0, 20]], "closed": true, "ids": ["bottom", "right", "top", "left"]}));
    run(&mut s, "sketch.constraint.fix", json!({"entities": ["bottom", "right", "top", "left"]}));
    run(&mut s, "sketch.line", json!({"points": [[30, 0], [30, 20]], "ids": ["div"]}));
    run(&mut s, "sketch.constraint.coincident", json!({"a": "div.start", "b": "bottom"}));
    run(&mut s, "sketch.constraint.coincident", json!({"a": "div.end", "b": "top"}));
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": "div"}));
    run(&mut s, "sketch.dimension", json!({"entities": ["left", "div"], "value": "split"}));
    run(&mut s, "sketch.finish", json!({}));
    s
}

fn extrude_kind(s: &Session) -> ProfileSel {
    match &s.doc.features.last().unwrap().kind {
        FeatureKind::Extrude { profiles, .. } => profiles.clone(),
        k => panic!("{k:?}"),
    }
}

/// The user's case: the left bay picked by its curves keeps extruding the left bay when
/// `split` moves the divider past the old pick point; a pick by that point (as a design saved
/// before picks were stored by curves holds it) silently extrudes the right bay instead.
#[test]
fn a_profile_picked_by_curves_follows_a_parameter_change() {
    let mut s = tray();
    run(&mut s, "solid.extrude", json!({"sketch": "Tray", "profiles": [{"curves": ["left", "bottom", "div", "top"]}], "distance": 10}));
    let v = volumes(&s);
    assert_eq!(v.len(), 1);
    assert!((v[0] - 30.0 * 20.0 * 10.0).abs() < 1.0, "{v:?}");
    run(&mut s, "parameters.change", json!({"name": "split", "expression": "20 mm"}));
    assert_eq!(last_error(&s), None);
    let v = volumes(&s);
    assert!((v[0] - 20.0 * 20.0 * 10.0).abs() < 1.0, "still the left bay, now 20 wide: {v:?}");

    // The same bay picked by a point (25, 10), stored as a point.
    let mut s = tray();
    run(&mut s, "solid.extrude", json!({"sketch": "Tray", "profiles": [{"point": [25, 10]}], "distance": 10}));
    let id = s.doc.features.last().unwrap().id;
    if let Some(f) = s.doc_mut().feature_mut(id)
        && let FeatureKind::Extrude { profiles, .. } = &mut f.kind
    {
        *profiles = ProfileSel::Points { points: vec![solidcraft_geom::Vec2::new(25.0, 10.0)] };
    }
    s.refresh();
    assert!((volumes(&s)[0] - 6000.0).abs() < 1.0);
    run(&mut s, "parameters.change", json!({"name": "split", "expression": "20 mm"}));
    let v = volumes(&s);
    assert!((v[0] - 40.0 * 20.0 * 10.0).abs() < 1.0, "the point now sits in the right bay: {v:?}");
}

/// A pick by point is stored by the curves around the picked profile, so it follows too.
#[test]
fn a_point_pick_is_stored_by_curves() {
    let mut s = tray();
    run(&mut s, "solid.extrude", json!({"sketch": "Tray", "profiles": [{"point": [25, 10]}], "distance": 10}));
    let ProfileSel::Curves { loops } = extrude_kind(&s) else { panic!("{:?}", extrude_kind(&s)) };
    let mut ids = loops[0].clone();
    ids.sort();
    assert_eq!(ids, ["bottom", "div", "left", "top"]);
    run(&mut s, "parameters.change", json!({"name": "split", "expression": "20 mm"}));
    assert_eq!(last_error(&s), None);
    assert!((volumes(&s)[0] - 4000.0).abs() < 1.0, "{:?}", volumes(&s));
}

/// New explicit API lists are sets even when reordered or repeated. Their saved
/// form must not accidentally become a legacy graph-span multiset.
#[test]
fn explicit_curve_pick_normalizes_repeats_before_save_and_parameter_edit() {
    let mut s = tray();
    run(&mut s, "solid.extrude", json!({"sketch":"Tray","profiles":[{"curves":["top","bottom","div","left","bottom","top"]}],"distance":10}));
    let ProfileSel::Curves { loops } = extrude_kind(&s) else { panic!("{:?}", extrude_kind(&s)) };
    assert_eq!(loops, vec![vec!["top", "bottom", "div", "left"]]);
    let mut reopened = Session::new(solidcraft_doc::format::read(&s.doc.to_json()).unwrap());
    run(&mut reopened, "parameters.change", json!({"name":"split","expression":"20 mm"}));
    assert_eq!(last_error(&reopened), None);
    assert!((volumes(&reopened)[0] - 4000.0).abs() < 1.0);
}

#[test]
fn repeated_explicit_curve_ids_do_not_count_split_graph_spans() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY","name":"S"}));
    run(&mut s, "sketch.line", json!({"points":[[0,0],[10,0],[10,10],[0,10]],"closed":true,"ids":["a","b","c","d"]}));
    run(&mut s, "sketch.line", json!({"points":[[5,-5],[5,0]],"ids":["stub"]}));
    run(&mut s, "sketch.finish", json!({}));
    assert_eq!(s.model.state().sketches[0].profiles[0].outer_curves.iter().filter(|id| *id == "a").count(), 2);
    run(&mut s, "solid.extrude", json!({"sketch":"S","profiles":[{"curves":["d","a","a","a","c","b"]}],"distance":2}));
    let ProfileSel::Curves { loops } = extrude_kind(&s) else { panic!("{:?}", extrude_kind(&s)) };
    assert_eq!(loops, vec![vec!["d", "a", "c", "b"]]);
    let reopened = Session::new(solidcraft_doc::format::read(&s.doc.to_json()).unwrap());
    assert_eq!(last_error(&reopened), None);
    assert!((volumes(&reopened)[0] - 200.0).abs() < 1e-3);
}

/// Curves cut into several pieces of one loop still name it: a line ending on the square's
/// bottom from outside cuts the bottom in two, both pieces on the square's loop.
#[test]
fn a_curve_cut_into_pieces_still_names_its_profile() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0], [10, 10], [0, 10]], "closed": true, "ids": ["a", "b", "c", "d"]}));
    run(&mut s, "sketch.line", json!({"points": [[5, -5], [5, 0]], "ids": ["stub"]}));
    run(&mut s, "sketch.finish", json!({}));
    let ps = &s.model.state().sketches[0].profiles;
    assert_eq!(ps.len(), 1);
    assert_eq!(ps[0].outer_curves.iter().filter(|c| *c == "a").count(), 2, "the bottom is in two pieces: {:?}", ps[0].outer_curves);
    run(&mut s, "solid.extrude", json!({"sketch": "S", "profiles": [{"curves": ["a", "b", "c", "d"]}], "distance": 2}));
    assert_eq!(last_error(&s), None);
    assert!((volumes(&s)[0] - 200.0).abs() < 1e-3, "{:?}", volumes(&s));
}

/// Both halves of a circle cut by a line are bounded by the same two curves: an error, not one
/// of them. A list no profile matches is an error too.
#[test]
fn ambiguous_and_missing_curve_picks_are_errors() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10, "id": "ring"}));
    run(&mut s, "sketch.line", json!({"points": [[-20, 0], [20, 0]], "ids": ["cut"]}));
    run(&mut s, "sketch.circle.center", json!({"center": [40, 0], "radius": 3, "id": "dot"}));
    run(&mut s, "sketch.finish", json!({}));
    let e = s.execute("solid.extrude", &json!({"sketch": "S", "profiles": [{"curves": ["ring", "cut"]}], "distance": 2})).unwrap_err().to_string();
    assert!(e.contains("2 profiles"), "{e}");
    assert!(s.model.state().bodies.is_empty());
    let e = s.execute("solid.extrude", &json!({"sketch": "S", "profiles": [{"curves": ["ring"]}], "distance": 2})).unwrap_err().to_string();
    assert!(e.contains("no profile"), "{e}");
    let e = s.execute("solid.extrude", &json!({"sketch": "S", "profiles": [{"curves": ["dot", "cut"]}], "distance": 2})).unwrap_err().to_string();
    assert!(e.contains("no profile"), "{e}");
    // A closed curve alone.
    run(&mut s, "solid.extrude", json!({"sketch": "S", "profiles": [{"curves": ["dot"]}], "distance": 2}));
    assert!((volumes(&s)[0] - std::f64::consts::PI * 9.0 * 2.0).abs() < 0.5, "{:?}", volumes(&s));
}

/// An island inside the picked outline is its hole, as with a pick by point; the island is a
/// profile of its own, picked by its curve.
#[test]
fn a_curve_pick_keeps_islands_as_holes() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [20, 0], [20, 20], [0, 20]], "closed": true, "ids": ["a", "b", "c", "d"]}));
    run(&mut s, "sketch.circle.center", json!({"center": [10, 10], "radius": 5, "id": "hole"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "S", "profiles": [{"curves": ["a", "b", "c", "d"]}], "distance": 1}));
    let disc = std::f64::consts::PI * 25.0;
    assert!((volumes(&s)[0] - (400.0 - disc)).abs() < 0.5, "{:?}", volumes(&s));
    run(&mut s, "solid.extrude", json!({"sketch": "S", "profiles": [{"curves": ["hole"]}], "distance": 1, "operation": "new"}));
    assert!((volumes(&s)[1] - disc).abs() < 0.5, "{:?}", volumes(&s));
}

/// Curve picks are saved in the design file and read back; timeline.edit takes the command's
/// `profiles` form.
#[test]
fn curve_picks_save_load_and_edit() {
    let mut s = tray();
    run(
        &mut s,
        "solid.extrude",
        json!({"sketch": "Tray", "profiles": [{"curves": ["left", "bottom", "div", "top"]}], "distance": 10, "name": "Bay"}),
    );
    let text = serde_json::to_string(&*s.doc).unwrap();
    let doc = solidcraft_doc::format::read(&text).unwrap();
    let mut t = Session::new(doc);
    assert_eq!(extrude_kind(&t), extrude_kind(&s));
    run(&mut t, "parameters.change", json!({"name": "split", "expression": "20 mm"}));
    assert!((volumes(&t)[0] - 4000.0).abs() < 1.0, "{:?}", volumes(&t));
    // Switch to the right bay through timeline.edit.
    run(&mut t, "timeline.edit", json!({"feature": "Bay", "set": {"profiles": [{"curves": ["div", "bottom", "right", "top"]}]}}));
    assert!((volumes(&t)[0] - 8000.0).abs() < 1.0, "{:?}", volumes(&t));
    run(&mut t, "timeline.edit", json!({"feature": "Bay", "set": {"profiles": [{"point": [5, 5]}]}}));
    assert!(matches!(extrude_kind(&t), ProfileSel::Curves { .. }), "{:?}", extrude_kind(&t));
    assert!((volumes(&t)[0] - 4000.0).abs() < 1.0, "{:?}", volumes(&t));
    // An old design's point pick still loads and evaluates.
    let old = text.replace(r#"{"type":"curves","loops":[["left","bottom","div","top"]]}"#, r#"{"type":"points","points":[[25.0,10.0]]}"#);
    assert_ne!(old, text);
    let t = Session::new(solidcraft_doc::format::read(&old).unwrap());
    assert!((volumes(&t)[0] - 6000.0).abs() < 1.0, "{:?}", volumes(&t));
}
