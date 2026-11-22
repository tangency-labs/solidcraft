//! MJCF export (#20786, docs/mjcf.md): bodies, joints written at the current values, limits and
//! loops closed by equality constraints.

use serde_json::{Value, json};
use solidcraft_io::MjcfPackage;

use super::urdf_tests::{hinge, joint, run, two_cubes};
use crate::Session;

fn package(s: &Session) -> MjcfPackage {
    match solidcraft_io::mjcf(&s.doc, &s.doc.painted(&s.model.state()), "t") {
        Ok(p) => p,
        Err(e) => panic!("{e}"),
    }
}

fn count(xml: &str, needle: &str) -> usize {
    xml.matches(needle).count()
}

#[test]
fn mjcf_of_a_hinge() {
    let mut s = hinge();
    run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": 30}));
    let p = package(&s);
    let x = &p.xml;
    assert!(x.contains(r#"<mujoco model="t">"#), "{x}");
    assert!(x.contains(r#"<mesh name="t-LeafA" file="t-LeafA.stl"/>"#), "{x}");
    assert_eq!(count(x, "<body "), 2, "{x}");
    assert_eq!(count(x, "<joint "), 1, "{x}");
    // Written where the design is: the hinge's reference is its current value.
    let ref_ = format!(r#"ref="{}""#, 30f64.to_radians());
    assert!(x.contains(r#"<joint name="Hinge" type="hinge""#) && x.contains(&ref_), "{x}");
    let files: Vec<&str> = p.meshes.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(files, ["t-LeafA.stl", "t-LeafB.stl"]);
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
}

/// A joint with several values is one body with a joint per value, or one `ball`, with none of
/// the URDF chain's in-between links.
#[test]
fn mjcf_keeps_a_joint_with_several_values_in_one_body() {
    for (kind, values, joints) in [("planar", json!([2, 3, 40]), 3), ("pin_slot", json!([30, 3]), 2), ("ball", json!([20, 30, 40]), 1)] {
        let mut s = two_cubes();
        joint(&mut s, kind, "J", Value::Null);
        run(&mut s, "joint.drive", json!({"joint": "J", "values": values}));
        let x = package(&s).xml;
        assert_eq!(count(&x, "<body "), 2, "{kind}: {x}");
        assert_eq!(count(&x, "<joint "), joints, "{kind}: {x}");
        assert!(!x.contains("_link"), "{kind}: {x}");
    }
    let mut s = two_cubes();
    joint(&mut s, "ball", "J", Value::Null);
    assert!(package(&s).xml.contains(r#"<joint name="J" type="ball"/>"#));
}

#[test]
fn mjcf_writes_a_range_only_where_there_is_a_limit() {
    let mut s = two_cubes();
    joint(&mut s, "slider", "Rail", json!([[0, 10]]));
    let x = package(&s).xml;
    assert!(x.contains(r#"type="slide""#) && x.contains(r#"range="0 0.01""#), "{x}");

    let mut s = two_cubes();
    joint(&mut s, "slider", "Free", Value::Null);
    let p = package(&s);
    assert!(p.xml.contains(r#"type="slide""#) && !p.xml.contains("range="), "{}", p.xml);
    assert!(p.warnings.is_empty(), "MJCF needs no limit: {:?}", p.warnings);
}

/// A sits on the ground, B turns on it (`AB`), C hangs on A a millimetre above B, and `BC` closes
/// the loop on B's top face, on AB's axis.
fn loop_of(kind: &str) -> Session {
    let mut s = two_cubes();
    joint(&mut s, "revolute", "AB", Value::Null);
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "C"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 21]}));
    run(&mut s, "joint.as_built", json!({"type": "rigid", "a": "A", "b": "C", "name": "AC"}));
    run(&mut s, "joint.as_built", json!({"type": kind, "a": "B", "b": "C", "at": {"face": [5, 5, 20]}, "name": "BC"}));
    s
}

#[test]
fn mjcf_closes_a_loop_with_equality_constraints() {
    let x = package(&loop_of("rigid")).xml;
    assert!(x.contains(r#"<weld name="BC" body1="B" body2="C"/>"#), "{x}");

    // A hinge is held at two points of its axis, 10 mm apart: B's top face is 10 mm above its frame.
    let x = package(&loop_of("revolute")).xml;
    assert_eq!(count(&x, "<connect "), 2, "{x}");
    assert!(x.contains(r#"<connect name="BC_1" body1="B" body2="C" anchor="0 "#), "{x}");
    assert!(x.contains(r#"0.02"/>"#), "{x}");

    let x = package(&loop_of("ball")).xml;
    assert_eq!(count(&x, "<connect "), 1, "{x}");

    let p = package(&loop_of("slider"));
    assert!(!p.xml.contains("<equality>"), "{}", p.xml);
    assert!(p.warnings.iter().any(|w| w.contains("joint `BC` closes a loop as a Slider joint")), "{:?}", p.warnings);
}

#[test]
fn mjcf_words_a_relation_for_mjcf() {
    let mut s = two_cubes();
    joint(&mut s, "perpendicular", "J", Value::Null);
    let p = package(&s);
    assert!(p.warnings.iter().any(|w| w.contains("which MJCF has no joint for")), "{:?}", p.warnings);
    assert!(!p.warnings.iter().any(|w| w.contains("URDF")), "{:?}", p.warnings);
}
