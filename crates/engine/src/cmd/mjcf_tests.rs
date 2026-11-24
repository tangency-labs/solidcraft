//! MJCF export (#20786, docs/mjcf.md): bodies, joints written at the current values, limits and
//! loops closed by equality constraints.

use serde_json::{Value, json};
use solidcraft_io::MjcfPackage;

use super::urdf_tests::{hinge, joint, occ, run, temp, two_cubes};
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

#[test]
fn mjcf_command_writes_the_meshes_then_the_model() {
    let mut s = hinge();
    let d = temp("mjcf-command");
    let path = d.join("robot.xml");
    let r = run(&mut s, "file.export_mjcf", json!({"path": path.to_string_lossy()}));
    assert_eq!(r["format"], "Mjcf", "{r}");
    assert_eq!((r["bodies"].as_u64(), r["joints"].as_u64(), r["equalities"].as_u64()), (Some(2), Some(1), Some(0)), "{r}");
    let files: Vec<String> = r["files"].as_array().into_iter().flatten().filter_map(|f| f.as_str()).map(String::from).collect();
    let names: Vec<String> = files.iter().filter_map(|f| std::path::Path::new(f).file_name()).map(|n| n.to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["robot-LeafA.stl", "robot-LeafB.stl", "robot.xml"], "{r}");
    let xml = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(xml.contains(r#"<mesh name="robot-LeafB" file="robot-LeafB.stl"/>"#), "{xml}");
    let _ = std::fs::remove_dir_all(&d);
}

/// `file.export` writes MJCF when `format` names it, and refuses a body list. A `.xml` path alone
/// is any XML, left to an exporter plugin that claims it.
#[test]
fn file_export_writes_mjcf_when_named() {
    let mut s = hinge();
    let d = temp("mjcf-export");
    let path = d.join("a.xml");
    let r = run(&mut s, "file.export", json!({"path": path.to_string_lossy(), "format": "mjcf"}));
    assert_eq!(r["format"], "Mjcf", "{r}");
    assert!(std::fs::read_to_string(&path).unwrap_or_default().starts_with("<?xml"), "{path:?}");
    assert!(s.execute("file.export", &json!({"path": d.join("b.xml").to_string_lossy()})).is_err());
    assert!(!d.join("b.xml").exists());
    let e = s.execute("file.export", &json!({"path": d.join("c.xml").to_string_lossy(), "format": "mjcf", "bodies": ["LeafA"]}));
    assert!(e.is_err_and(|e| e.to_string().contains("whole assembly")));
    let _ = std::fs::remove_dir_all(&d);
}

/// A part moved but not captured is exported where it is shown, as the URDF export does.
#[test]
fn mjcf_exports_uncaptured_moves() {
    let mut s = two_cubes();
    let b = occ(&s, "B");
    run(&mut s, "occurrence.move", json!({"occurrence": b, "translate": [0, 0, 30], "capture": false}));
    let d = temp("mjcf-pending");
    let path = d.join("m.xml");
    run(&mut s, "file.export", json!({"path": path.to_string_lossy(), "format": "mjcf"}));
    let xml = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(xml.contains(r#"<body name="B" pos="0 0 0.03" quat="1 0 0 0">"#), "{xml}");
    let _ = std::fs::remove_dir_all(&d);
}
