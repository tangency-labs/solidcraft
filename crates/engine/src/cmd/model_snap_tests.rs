use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

/// The kinds the snaps came back as, in order.
fn types(v: &Value) -> Vec<String> {
    v["snaps"].as_array().map(|a| a.iter().filter_map(|s| s["type"].as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// The first snap of a kind, if any.
fn first<'a>(v: &'a Value, kind: &str) -> Option<&'a Value> {
    v["snaps"].as_array()?.iter().find(|s| s["type"] == kind)
}

fn at(s: &Value) -> [f64; 3] {
    let p = |i: usize| s["at"][i].as_f64().unwrap_or(f64::NAN);
    [p(0), p(1), p(2)]
}

fn close(a: [f64; 3], b: [f64; 3], tol: f64) -> bool {
    a.iter().zip(&b).all(|(x, y)| (x - y).abs() <= tol)
}

fn box_session() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "body_name": "Block"}));
    s
}

/// Just off the corner of the box at the far top, within a millimetre of it.
const NEAR_CORNER: [f64; 3] = [20.2, 20.1, 10.1];

#[test]
fn a_corner_of_a_box_comes_back_as_a_vertex() {
    let mut s = box_session();
    // The box runs from its corner at the origin, so it spans 0…20, 0…20, 0…10.
    let v = run(&mut s, "model.snap", json!({"at": NEAR_CORNER, "radius": 1}));
    let vertex = first(&v, "vertex").unwrap_or_else(|| panic!("{v}"));
    assert!(close(at(vertex), [20.0, 20.0, 10.0], 1e-9), "{v}");
    assert_eq!(vertex["body"], "Block", "{v}");
    // Strongest first: the corner beats the edges and the face that reach that place too.
    assert_eq!(types(&v).first().map(String::as_str), Some("vertex"), "{v}");
}

#[test]
fn the_middle_of_an_edge_and_the_centre_of_a_face_come_back() {
    let mut s = box_session();
    // Halfway along the top edge at y = 0.
    let mid = run(&mut s, "model.snap", json!({"at": [10.3, 0.0, 10.0], "radius": 1}));
    let m = first(&mid, "midpoint").unwrap_or_else(|| panic!("{mid}"));
    assert!(close(at(m), [10.0, 0.0, 10.0], 1e-9), "{mid}");
    assert_eq!(types(&mid).first().map(String::as_str), Some("midpoint"), "{mid}");
    // The centre of the top face.
    let face = run(&mut s, "model.snap", json!({"at": [10.4, 10.2, 10.0], "radius": 1}));
    let c = first(&face, "face_center").unwrap_or_else(|| panic!("{face}"));
    assert!(close(at(c), [10.0, 10.0, 10.0], 1e-6), "{face}");
    // A place that only slides over a face is the fallback, never the pick.
    assert_ne!(types(&face).first().map(String::as_str), Some("on_face"), "{face}");
}

#[test]
fn the_centre_of_a_hole_is_what_its_rim_offers() {
    let mut s = box_session();
    run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 20, "base": [10, 10, -5], "operation": "cut", "targets": ["Block"]}));
    // Asked near the axis, the centre of the rim circle comes back.
    let axis = run(&mut s, "model.snap", json!({"at": [10.2, 10.1, 10.0], "radius": 1}));
    let c = first(&axis, "center").unwrap_or_else(|| panic!("{axis}"));
    assert!(close(at(c), [10.0, 10.0, 10.0], 1e-6), "{axis}");
    assert_eq!(c["circle"]["radius"].as_f64().map(|r| (r - 3.0).abs() < 1e-6), Some(true), "{axis}");
    // On the rim, 30° round so it is no quarter turn, the centre is 3 mm off: out of reach of a
    // proximity search…
    let on_rim = [10.0 + 3.0 * 0.75_f64.sqrt(), 11.5, 10.0];
    // (the cut splits the rim into arcs, so an end of one of them is in reach too).
    let rim = run(&mut s, "model.snap", json!({"at": on_rim, "radius": 0.5}));
    assert!(!types(&rim).contains(&"center".to_string()), "{rim}");
    assert!(types(&rim).contains(&"on_edge".to_string()), "{rim}");
    // … but asking what that edge offers does reach it, which is how a hover on the rim picks
    // the axis of a hole.
    let edge = c["edge"].as_u64().unwrap_or_else(|| panic!("{axis}"));
    let hover = run(&mut s, "model.snap", json!({"at": on_rim, "body": "Block", "edge": edge}));
    let hc = first(&hover, "center").unwrap_or_else(|| panic!("{hover}"));
    assert!(close(at(hc), [10.0, 10.0, 10.0], 1e-6), "{hover}");
    // The quarter turns of the rim are there too.
    assert!(types(&hover).contains(&"quadrant".to_string()), "{hover}");
}

#[test]
fn only_the_kinds_and_bodies_asked_for_come_back() {
    let mut s = box_session();
    run(&mut s, "solid.box", json!({"length": 4, "width": 4, "height": 4, "corner": [40, 0, 0], "body_name": "Other"}));
    let only = run(&mut s, "model.snap", json!({"at": NEAR_CORNER, "radius": 1, "kinds": ["vertex"]}));
    assert_eq!(types(&only), ["vertex"], "{only}");
    let none = run(&mut s, "model.snap", json!({"at": NEAR_CORNER, "radius": 1, "bodies": ["Other"]}));
    assert_eq!(types(&none), [] as [String; 0], "{none}");
    let v = run(&mut s, "model.snap", json!({"at": NEAR_CORNER, "radius": 1, "bodies": ["Block"]}));
    assert!(v["snaps"].as_array().is_some_and(|a| a.iter().all(|s| s["body"] == "Block")), "{v}");
}

#[test]
fn a_hidden_body_offers_nothing() {
    let mut s = box_session();
    let shown = run(&mut s, "model.snap", json!({"at": NEAR_CORNER, "radius": 1}));
    assert!(!types(&shown).is_empty(), "{shown}");
    run(&mut s, "browser.visibility", json!({"visible": false, "items": ["Block"]}));
    let hidden = run(&mut s, "model.snap", json!({"at": NEAR_CORNER, "radius": 1}));
    assert_eq!(types(&hidden), [] as [String; 0], "{hidden}");
}

#[test]
fn hostile_parameters_are_refused_and_never_panic() {
    let mut s = box_session();
    for p in [
        json!({}),
        json!({"at": "nope"}),
        json!({"at": [1, 2]}),
        json!({"at": [1, 2, 3], "radius": 0}),
        json!({"at": [1, 2, 3], "radius": -5}),
        json!({"at": [1e308, 1e308, 1e308], "radius": 1e308}),
        json!({"at": [1, 2, 3], "kinds": ["nonesuch"]}),
        json!({"at": [1, 2, 3], "edge": 0}),
        json!({"at": [1, 2, 3], "face": 0}),
        json!({"at": [1, 2, 3], "body": "Block", "edge": 99999}),
        json!({"at": [1, 2, 3], "body": "Block", "face": 99999}),
        json!({"at": [1, 2, 3], "body": "Nope", "edge": 0}),
        json!({"at": [1, 2, 3], "bodies": ["Nope"]}),
    ] {
        assert!(s.execute("model.snap", &p).is_err(), "model.snap {p} should be refused");
    }
    // A place nowhere near the model simply finds nothing.
    let far = run(&mut s, "model.snap", json!({"at": [5000, 5000, 5000], "radius": 1}));
    assert_eq!(types(&far), [] as [String; 0], "{far}");
}

#[test]
fn the_command_changes_nothing() {
    let mut s = box_session();
    let before = s.doc.features.len();
    run(&mut s, "model.snap", json!({"at": [10, 10, 10], "radius": 2}));
    assert_eq!(s.doc.features.len(), before);
    let spec = crate::find_command("model.snap").unwrap_or_else(|| panic!("model.snap is registered"));
    assert!(!spec.undoable, "a query makes no timeline step");
}
