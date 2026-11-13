//! A point joint origin keeps the x it is given: two origins given at one point with the same z
//! and x meet as they are, whichever way their parts are turned.

use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use super::tests::{occ, place, run};
use crate::Session;

/// A 100 x 20 x 10 base at the origin and an 80 x 20 x 10 block on it, the block turned a quarter
/// turn about z (about (60, 10)).
fn blocks() -> Session {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Base"}));
    run(&mut s, "solid.box", json!({"length": 100, "width": 20, "height": 10, "corner": [0, 0, 0]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Block"}));
    run(&mut s, "solid.box", json!({"length": 80, "width": 20, "height": 10, "corner": [20, 0, 10]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let (base, block) = (occ(&s, "Base"), occ(&s, "Block"));
    run(&mut s, "occurrence.ground", json!({"occurrence": base, "grounded": true}));
    run(&mut s, "occurrence.move", json!({"occurrence": block, "axis": [0, 0, 1], "angle": "90 deg", "origin": [60, 10, 10]}));
    s
}

#[test]
fn origins_given_the_same_x_meet_as_they_are() {
    let mut s = blocks();
    let corner = |s: &Session| place(s, "Block", Vec3::new(20.0, 0.0, 10.0));
    let before = corner(&s);
    // Both origins at the block's base centre, z up and x along world X: the block is already there.
    let at = |o: &str| json!({"occurrence": o, "point": [60, 10, 10], "z": [0, 0, 1], "x": [1, 0, 0]});
    let r = run(&mut s, "joint.create", json!({"type": "rigid", "a": at("Base"), "b": at("Block"), "flip": true}));
    assert_eq!(r["conflicts"], json!([]), "{r}");
    assert!(corner(&s).dist(before) < 1e-9, "the block moved from {before:?} to {:?}", corner(&s));
    // Saved and read back, the x stays.
    let saved = serde_json::to_string(&s.doc.assembly.joints).unwrap_or_default();
    assert!(saved.contains("\"x\""), "{saved}");
    let back: Vec<solidcraft_doc::joints::Joint> = serde_json::from_str(&saved).unwrap_or_default();
    assert_eq!(back, s.doc.assembly.joints);
}

#[test]
fn a_point_origin_without_x_is_placed_as_before_and_a_bad_x_is_refused() {
    let mut s = blocks();
    // No x: the frame's x comes from z alone, as designs made before had it (the turned block
    // turns back a quarter turn to line its frame up with the base's).
    let at = |o: &str| json!({"occurrence": o, "point": [60, 10, 10], "z": [0, 0, 1]});
    run(&mut s, "joint.create", json!({"type": "rigid", "a": at("Base"), "b": at("Block"), "flip": true, "name": "J"}));
    let snap = s.doc.assembly.joints.first().map(|j| j.b.snap.clone());
    assert!(matches!(snap, Some(solidcraft_doc::joints::Snap::Point { x: None, .. })), "{snap:?}");
    let far = place(&s, "Block", Vec3::new(100.0, 10.0, 10.0));
    assert!(far.dist(Vec3::new(100.0, 10.0, 10.0)) < 1e-9, "{far:?}");
    // An x along z, or no direction at all, is refused.
    for x in [json!([0, 0, 5]), json!([0, 0, -1]), json!([0, 0, 0])] {
        let p = json!({"name": "O", "occurrence": "Base", "point": [60, 10, 10], "z": [0, 0, 1], "x": x});
        assert!(s.execute("joint.origin", &p).is_err(), "{p}");
    }
}
