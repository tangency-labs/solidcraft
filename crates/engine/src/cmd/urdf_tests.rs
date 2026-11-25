//! URDF export (#719, docs/urdf.md): files, names, limits, mass and forward kinematics against
//! the joint solver.

use solidcraft_geom::prelude::*;
use std::path::PathBuf;

use serde_json::{Value, json};
use solidcraft_doc::{Mat, joints, mat_mul};
use solidcraft_io::urdf::{CollisionGeom, Step, UrdfModel, UrdfOptions, UrdfPackage};

use crate::Session;

pub(super) fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

pub(super) fn occ(s: &Session, component: &str) -> u64 {
    let c = s.doc.find_component(component).unwrap_or(0);
    s.doc.occurrence_of(c).map(|o| o.id).unwrap_or(0)
}

pub(super) fn ground(s: &mut Session, component: &str, grounded: bool) {
    let o = occ(s, component);
    run(s, "occurrence.ground", json!({"occurrence": o, "grounded": grounded}));
}

/// A hinge leaf: a 40 x 30 x 3 plate and an r4 knuckle along X at y = 33, z = 4.
pub(super) fn leaf(s: &mut Session, name: &str) {
    run(s, "component.activate", json!({"component": "root"}));
    run(s, "component.create", json!({"name": name}));
    run(s, "solid.box", json!({"length": 40, "width": 30, "height": 3, "corner": [0, 0, 0]}));
    run(s, "solid.cylinder", json!({"base": [0, 33, 4], "axis": [1, 0, 0], "radius": 4, "height": 40, "operation": "new"}));
}

/// LeafA (grounded) and LeafB on a revolute joint "Hinge" without limits.
pub(super) fn hinge() -> Session {
    let mut s = Session::default();
    leaf(&mut s, "LeafA");
    leaf(&mut s, "LeafB");
    ground(&mut s, "LeafA", true);
    run(
        &mut s,
        "joint.create",
        json!({"type": "revolute", "a": {"occurrence": "LeafA", "circle": [1, 37, 4]}, "b": {"occurrence": "LeafB", "circle": [1, 37, 4]}, "flip": true, "name": "Hinge"}),
    );
    s
}

/// 10 mm cubes in components A (grounded, at the origin) and B (at x = 20).
pub(super) fn two_cubes() -> Session {
    let mut s = Session::default();
    for (name, x) in [("A", 0), ("B", 20)] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
    }
    ground(&mut s, "A", true);
    s
}

/// B's bottom face on A's top face.
pub(super) fn joint(s: &mut Session, kind: &str, name: &str, limits: Value) {
    run(s, "joint.create", json!({"type": kind, "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "limits": limits, "name": name}));
}

fn package(s: &Session) -> UrdfPackage {
    match solidcraft_io::urdf(&s.doc, &s.doc.painted(&s.model.state()), "t", &UrdfOptions::default()) {
        Ok(p) => p,
        Err(e) => panic!("urdf: {e}"),
    }
}

fn close(a: &Mat, b: &Mat) -> bool {
    a.iter().flatten().zip(b.iter().flatten()).all(|(x, y)| (x - y).abs() < 1e-9)
}

/// Every placement's link, composed at the design's current joint values, sits where SolidCraft
/// places it.
fn assert_fk(m: &UrdfModel, s: &Session, what: &str) {
    let vals = joints::joint_values(&s.doc.assembly);
    let frames = m.link_frames(|j| j.source.and_then(|(id, k)| vals.get(&id)?.get(k).copied()).unwrap_or(0.0));
    let placements = s.doc.placements();
    let mut seen = 0;
    for (l, f) in m.links.iter().zip(&frames) {
        if l.path.is_empty() {
            continue;
        }
        let Some((_, _, want)) = placements.iter().find(|p| p.1 == l.path) else { panic!("{what}: {} has no placement", l.name) };
        let got = mat_mul(f, &l.mesh_origin);
        assert!(close(&got, want), "{what}: {} at {got:?}, SolidCraft has {want:?}", l.name);
        seen += 1;
    }
    assert_eq!(seen, placements.len() - 1, "{what}: a link per placement");
}

pub(super) fn temp(tag: &str) -> PathBuf {
    let d = solidcraft_io::vfs::temp_dir().join(format!("solidcraft-urdf-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Volume (m³) of an STL whose triangles share their corners (`None`: not closed).
fn stl_volume(bytes: &[u8]) -> Option<f64> {
    let mut index: std::collections::HashMap<[u32; 3], u32> = std::collections::HashMap::new();
    let mut mesh = solidcraft_geom::Mesh::default();
    for t in solidcraft_io::read_stl(bytes).ok()? {
        let mut tri = [0u32; 3];
        for (k, p) in t.iter().enumerate() {
            let n = mesh.positions.len() as u32;
            tri[k] = *index.entry(p.map(f32::to_bits)).or_insert_with(|| {
                mesh.positions.push(solidcore_record::Vec3::new(p[0] as f64, p[1] as f64, p[2] as f64));
                n
            });
        }
        mesh.triangles.push(tri);
    }
    mesh.mass_properties().map(|m| m.volume)
}

/// A decal on a part goes to the part's link as a textured COLLADA visual, with its image beside
/// it and its corners in the part's frame, lifted off the face. A shown canvas is written the same
/// way; a hidden one is not.
#[test]
fn urdf_writes_decals_as_textured_visuals() {
    use image::ImageEncoder;
    let mut s = two_cubes();
    run(&mut s, "component.activate", json!({"component": "B"}));
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png).write_image(&[200u8; 4 * 8], 4, 2, image::ExtendedColorType::Rgba8).unwrap();
    let png = crate::tests::image_file(&png);
    run(&mut s, "canvas.decal", json!({"path": png, "face": [25, 5, 10], "width": 8, "name": "Logo"}));
    run(&mut s, "canvas.insert", json!({"path": png, "plane": "XY", "width": 8, "name": "Tag"}));
    run(&mut s, "canvas.insert", json!({"path": png, "plane": "XY", "width": 8, "name": "Hidden"}));
    run(&mut s, "canvas.edit", json!({"canvas": "Hidden", "visible": false}));
    // Asked for the usual way, the export writes no picture, as before.
    let plain = temp("decal-plain");
    run(&mut s, "file.export", json!({"path": plain.join("t.urdf").to_string_lossy()}));
    let mut files: Vec<String> = std::fs::read_dir(&plain).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
    files.sort();
    assert_eq!(files, ["t-A.stl", "t-B.stl", "t.urdf"]);
    let d = temp("decal");
    let path = d.join("t.urdf");
    let r = run(&mut s, "file.export", json!({"path": path.to_string_lossy(), "images": true}));
    let mut files: Vec<String> = std::fs::read_dir(&d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
    files.sort();
    assert_eq!(files, ["t-A.stl", "t-B-Logo.dae", "t-B-Logo.png", "t-B-Tag.dae", "t-B-Tag.png", "t-B.stl", "t.urdf"], "{r}");
    let xml = std::fs::read_to_string(&path).unwrap();
    let link_b = xml.split(r#"<link name="B">"#).nth(1).and_then(|x| x.split("</link>").next()).unwrap_or_default();
    assert!(link_b.contains(r#"<mesh filename="t-B-Logo.dae"/>"#) && link_b.contains(r#"<mesh filename="t-B.stl"/>"#), "{xml}");
    let dae = std::fs::read_to_string(d.join("t-B-Logo.dae")).unwrap();
    assert!(dae.contains("<init_from>t-B-Logo.png</init_from>"), "{dae}");
    // The front quad is the decal as SolidCraft shows it, in B's frame, in metres, 0.2 mm off the
    // face along its normal; the back quad as far the other way.
    let positions: Vec<f64> = dae
        .split(r#"<float_array id="decal-positions-array" count="24">"#)
        .nth(1)
        .and_then(|x| x.split('<').next())
        .map(|x| x.split_whitespace().filter_map(|v| v.parse().ok()).collect())
        .unwrap_or_default();
    assert_eq!(positions.len(), 24);
    // Worked out by hand: B is placed where it was built, so its frame is the world's. The decal
    // is 8 x 4 mm (the image is 2:1) about (25, 5) on B's top face at z = 10, along the face's
    // axes either way round; the front quad 0.2 mm above the face, the back quad 0.2 mm below.
    let corners: Vec<[f64; 3]> = positions.chunks(3).map(|p| [p[0] * 1e3, p[1] * 1e3, p[2] * 1e3]).collect();
    let mut front: Vec<(i64, i64)> = corners[..4].iter().map(|p| ((p[0] * 1e3).round() as i64, (p[1] * 1e3).round() as i64)).collect();
    front.sort();
    let along_x = [(21000, 3000), (21000, 7000), (29000, 3000), (29000, 7000)];
    let along_y = [(23000, 1000), (23000, 9000), (27000, 1000), (27000, 9000)];
    assert!(front == along_x || front == along_y, "{corners:?}");
    let mut back: Vec<(i64, i64)> = corners[4..].iter().map(|p| ((p[0] * 1e3).round() as i64, (p[1] * 1e3).round() as i64)).collect();
    back.sort();
    assert_eq!(back, front, "the back quad has the front's corners");
    assert!(corners[..4].iter().all(|p| (p[2] - 10.2).abs() < 1e-9) && corners[4..].iter().all(|p| (p[2] - 9.8).abs() < 1e-9), "{corners:?}");
}

fn names(m: &UrdfModel) -> (Vec<&str>, Vec<&str>) {
    (m.links.iter().map(|l| l.name.as_str()).collect(), m.joints.iter().map(|j| j.name.as_str()).collect())
}

fn has_warning(p: &UrdfPackage, needle: &str) -> bool {
    p.warnings.iter().any(|w| w.contains(needle))
}

#[test]
fn urdf_of_a_hinge() {
    let mut s = hinge();
    run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": 90}));
    let d = temp("hinge");
    let path = d.join("hinge.urdf");
    let r = run(&mut s, "file.export", json!({"path": path.to_string_lossy()}));
    assert_eq!(r["format"], "Urdf", "{r}");
    assert_eq!(r["links"], 3, "{r}");
    let mut files: Vec<String> = std::fs::read_dir(&d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
    files.sort();
    assert_eq!(files, ["hinge-LeafA.stl", "hinge-LeafB.stl", "hinge.urdf"]);
    let xml = std::fs::read_to_string(&path).unwrap();
    for want in [
        r#"<link name="base_link">"#,
        r#"<link name="LeafA">"#,
        r#"<link name="LeafB">"#,
        r#"<joint name="LeafA_fixed" type="fixed">"#,
        r#"<joint name="Hinge" type="continuous">"#,
        r#"<mesh filename="hinge-LeafB.stl"/>"#,
    ] {
        assert!(xml.contains(want), "{want} missing from\n{xml}");
    }
    // Every mesh is a closed solid in metres.
    for f in ["hinge-LeafA.stl", "hinge-LeafB.stl"] {
        let v = stl_volume(&std::fs::read(d.join(f)).unwrap());
        assert!(v.is_some_and(|v| v > 0.0 && v < 1e-5), "{f}: {v:?}");
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn urdf_forward_kinematics_match_the_assembly() {
    let cases = [
        ("revolute", json!([30]), json!([-50])),
        ("slider", json!([5]), json!([-3])),
        ("cylindrical", json!([30, 4]), json!([-70, -2])),
        ("pin_slot", json!([30, 3]), json!([100, -6])),
        ("planar", json!([2, 3, 40]), json!([-4, 7, -20])),
        ("ball", json!([20, 30, 40]), json!([-60, 10, 75])),
    ];
    for (kind, first, second) in cases {
        // The tree reaches the child on B, then (B grounded) on A.
        for b_grounded in [false, true] {
            let mut s = two_cubes();
            if b_grounded {
                ground(&mut s, "A", false);
                ground(&mut s, "B", true);
            }
            joint(&mut s, kind, "J", Value::Null);
            run(&mut s, "joint.drive", json!({"joint": "J", "values": first}));
            let p = package(&s);
            let what = format!("{kind} (B grounded: {b_grounded})");
            assert_fk(&p.model, &s, &what);
            // The same URDF follows SolidCraft to other values.
            run(&mut s, "joint.drive", json!({"joint": "J", "values": second}));
            assert_fk(&p.model, &s, &what);
            let moving = p.model.joints.iter().filter(|j| j.step != Step::Fixed).count();
            assert_eq!(moving, joints::JointKind::parse(kind).map(|k| k.dofs().len()).unwrap_or(0), "{what}");
        }
    }
}

/// A saved lateral offset locates the hinge axis in the exported model, in either tree direction.
#[test]
fn urdf_lateral_joint_offsets_keep_their_frame_and_motion_axis() {
    for kind in ["rigid", "revolute"] {
        for b_grounded in [false, true] {
            let mut s = two_cubes();
            if b_grounded {
                ground(&mut s, "A", false);
                ground(&mut s, "B", true);
            }
            run(
                &mut s,
                "joint.create",
                json!({"type":kind, "name":"Offset joint", "flip":true, "angle":0,
                    "a":{"occurrence":"A","point":[5,5,10],"z":[1,0,0],"x":[0,0,1]},
                    "b":{"occurrence":"B","point":[25,5,0],"z":[1,0,0],"x":[0,0,1]},
                    "offset_x":2,"offset_y":3,"offset":4}),
            );
            let p = package(&s);
            let what = format!("lateral {kind}, B grounded {b_grounded}");
            assert_fk(&p.model, &s, &what);
            if kind == "revolute" {
                run(&mut s, "joint.drive", json!({"joint":"Offset joint","value":90}));
                assert_fk(&p.model, &s, &what);
            }
        }
    }
}

#[test]
fn urdf_limits_convert_units() {
    let mut s = two_cubes();
    joint(&mut s, "slider", "Rail", json!([[0, 10]]));
    let x = package(&s).xml;
    assert!(x.contains(r#"<joint name="Rail" type="prismatic">"#), "{x}");
    assert!(x.contains(r#"<limit lower="0" upper="0.01" effort="1000" velocity="10"/>"#), "{x}");

    // Older documents can contain reversed limits even though joint commands now reject them.
    let mut doc = (*s.doc).clone();
    doc.assembly.joints[0].limits = vec![Some((10.0, 0.0))];
    let p = solidcraft_io::urdf(&doc, &s.doc.painted(&s.model.state()), "t", &UrdfOptions::default()).unwrap();
    assert!(p.xml.contains(r#"<limit lower="0" upper="0.01" effort="1000" velocity="10"/>"#), "{}", p.xml);

    let mut s = two_cubes();
    joint(&mut s, "revolute", "Turn", json!([[-30, 45]]));
    let p = package(&s);
    let Some(j) = p.model.joints.iter().find(|j| j.name == "Turn") else { panic!("{:?}", names(&p.model)) };
    let Some((lo, hi)) = j.limit else { panic!("no limit") };
    assert!((lo + 30f64.to_radians()).abs() < 1e-12 && (hi - 45f64.to_radians()).abs() < 1e-12);
    assert!(p.xml.contains(r#"type="revolute">"#), "{}", p.xml);
    assert!(has_warning(&p, "effort and velocity"), "{:?}", p.warnings);

    // A prismatic step needs a limit: an unlimited slide gets ±1000 m and a warning.
    let mut s = two_cubes();
    joint(&mut s, "slider", "Free", Value::Null);
    let p = package(&s);
    assert!(p.xml.contains(r#"lower="-1000" upper="1000""#), "{}", p.xml);
    assert!(has_warning(&p, "Free"), "{:?}", p.warnings);
}

/// A limit that is not finite (a hand-edited file) counts as none, never as `0`.
#[test]
fn urdf_treats_infinite_limits_as_none() {
    let mut s = two_cubes();
    joint(&mut s, "revolute", "Turn", Value::Null);
    let mut doc = (*s.doc).clone();
    for j in &mut doc.assembly.joints {
        j.limits = vec![Some((f64::NEG_INFINITY, f64::INFINITY))];
    }
    let p = solidcraft_io::urdf(&doc, &s.doc.painted(&s.model.state()), "t", &UrdfOptions::default()).unwrap();
    assert!(p.xml.contains(r#"<joint name="Turn" type="continuous">"#), "{}", p.xml);
}

#[test]
fn urdf_effort_and_velocity_are_written_when_given() {
    let mut s = two_cubes();
    joint(&mut s, "revolute", "Turn", Value::Null);
    let d = temp("effort");
    let path = d.join("e.urdf");
    let r = run(&mut s, "file.export", json!({"path": path.to_string_lossy(), "effort": 12.5, "velocity": 0.5}));
    assert!(r["warnings"].as_array().unwrap().iter().all(|w| !w.as_str().unwrap_or("").contains("effort")), "{r}");
    let xml = std::fs::read_to_string(&path).unwrap();
    assert!(xml.contains(r#"<limit effort="12.5" velocity="0.5"/>"#), "{xml}");
    for bad in [json!(0), json!(-1), json!("x")] {
        assert!(s.execute("file.export", &json!({"path": path.to_string_lossy(), "effort": bad})).is_err(), "{bad}");
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// A moved occurrence whose position is not captured yet exports where it is shown.
#[test]
fn urdf_exports_uncaptured_moves() {
    let mut s = two_cubes();
    let b = occ(&s, "B");
    run(&mut s, "occurrence.move", json!({"occurrence": b, "translate": [0, 0, 30], "capture": false}));
    assert!(!s.pending_moves.is_empty());
    let d = temp("pending");
    let path = d.join("m.urdf");
    run(&mut s, "file.export", json!({"path": path.to_string_lossy()}));
    let xml = std::fs::read_to_string(&path).unwrap();
    // B hangs on base_link where it is shown: 30 mm up.
    assert!(xml.contains(r#"<origin xyz="0 0 0.03" rpy="0 0 0"/>"#), "{xml}");
    let _ = std::fs::remove_dir_all(&d);
}

/// A jointed occurrence moved without capturing exports where its joint puts it: its joint
/// would put it back as soon as it moves, so the move must not become part of its link.
#[test]
fn urdf_ignores_uncaptured_moves_of_jointed_occurrences() {
    let mut s = two_cubes();
    joint(&mut s, "revolute", "Turn", Value::Null);
    let d = temp("pending-jointed");
    let path = d.join("j.urdf");
    run(&mut s, "file.export", json!({"path": path.to_string_lossy()}));
    let solved = std::fs::read_to_string(&path).unwrap();
    let b = occ(&s, "B");
    run(&mut s, "occurrence.move", json!({"occurrence": b, "translate": [0, 0, 30], "capture": false}));
    let r = run(&mut s, "file.export", json!({"path": path.to_string_lossy()}));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), solved);
    assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap_or("").contains("not captured")), "{r}");
    let _ = std::fs::remove_dir_all(&d);
}

/// A joint whose origins no longer resolve does not place its occurrence, so that occurrence's
/// uncaptured move counts.
#[test]
fn urdf_keeps_uncaptured_moves_when_the_joint_is_broken() {
    let mut s = two_cubes();
    joint(&mut s, "revolute", "Turn", Value::Null);
    let Some(f) = s.doc.features.iter().rev().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::Box { .. })).map(|f| f.id) else {
        panic!("box")
    };
    run(&mut s, "timeline.suppress", json!({"feature": f, "suppressed": true}));
    let d = temp("pending-broken");
    let path = d.join("k.urdf");
    run(&mut s, "file.export", json!({"path": path.to_string_lossy()}));
    let before = std::fs::read_to_string(&path).unwrap();
    let b = occ(&s, "B");
    run(&mut s, "occurrence.move", json!({"occurrence": b, "translate": [0, 0, 30], "capture": false}));
    let r = run(&mut s, "file.export", json!({"path": path.to_string_lossy()}));
    assert_ne!(std::fs::read_to_string(&path).unwrap(), before, "B exports where it is shown");
    assert!(r["warnings"].as_array().unwrap().iter().all(|w| !w.as_str().unwrap_or("").contains("not captured")), "{r}");
    let _ = std::fs::remove_dir_all(&d);
}

/// A group the solver leaves where it is (grounded A elsewhere, C and D only joined to each
/// other) keeps an uncaptured move of C: no joint puts C back.
#[test]
fn urdf_keeps_uncaptured_moves_the_solver_does_not_undo() {
    let mut s = two_cubes();
    for (name, x) in [("C", 40), ("D", 60)] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
    }
    // A and B are joined, so the solver walks from grounded A and never anchors C.
    joint(&mut s, "revolute", "Turn", Value::Null);
    run(&mut s, "joint.create", json!({"type": "revolute", "a": {"face": [45, 5, 10]}, "b": {"face": [65, 5, 0]}, "name": "Pair"}));
    let d = temp("pending-floating");
    let path = d.join("f.urdf");
    run(&mut s, "file.export", json!({"path": path.to_string_lossy()}));
    let before = std::fs::read_to_string(&path).unwrap();
    let c = occ(&s, "C");
    run(&mut s, "occurrence.move", json!({"occurrence": c, "translate": [0, 0, 30], "capture": false}));
    let r = run(&mut s, "file.export", json!({"path": path.to_string_lossy()}));
    assert_ne!(std::fs::read_to_string(&path).unwrap(), before, "C exports where it is shown");
    assert!(r["warnings"].as_array().unwrap().iter().all(|w| !w.as_str().unwrap_or("").contains("not captured")), "{r}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn urdf_mesh_prefix_is_written_and_checked() {
    let mut s = hinge();
    let d = temp("prefix");
    let path = d.join("p.urdf");
    run(&mut s, "file.export", json!({"path": path.to_string_lossy(), "mesh_prefix": "package://robot/meshes/"}));
    let xml = std::fs::read_to_string(&path).unwrap();
    assert!(xml.contains(r#"<mesh filename="package://robot/meshes/p-LeafA.stl"/>"#), "{xml}");
    // XML cannot hold a control character, even escaped.
    let e = s.execute("file.export", &json!({"path": d.join("q.urdf").to_string_lossy(), "mesh_prefix": "a\u{1}b"}));
    assert!(e.is_err_and(|e| e.to_string().contains("mesh_prefix")));
    assert!(!d.join("q.urdf").exists());
    let _ = std::fs::remove_dir_all(&d);
}

/// The visual's colour keeps the body's opacity, and a too long prefix says so.
#[test]
fn urdf_colour_keeps_opacity() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Glass"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    let body = s.model.state().bodies.first().map(|b| b.name.clone()).unwrap_or_default();
    run(&mut s, "appearance.assign", json!({"bodies": [body], "color": [255, 0, 0], "opacity": 0.5}));
    let p = package(&s);
    assert!(p.xml.contains(r#"<color rgba="1 0 0 0.5"/>"#), "{}", p.xml);
    let d = temp("long-prefix");
    let e = s.execute("file.export", &json!({"path": d.join("l.urdf").to_string_lossy(), "mesh_prefix": "a".repeat(5000)}));
    assert!(e.is_err_and(|e| e.to_string().contains("longer than")));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn urdf_inertial_is_mass_weighted() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Cube"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [100, 0, 0]}));
    let body = s.model.state().bodies.first().map(|b| b.name.clone()).unwrap_or_default();
    run(&mut s, "material.assign", json!({"bodies": [body], "material": "Aluminum"}));
    let p = package(&s);
    let Some(l) = p.model.links.iter().find(|l| l.name == "Cube") else { panic!("{:?}", names(&p.model)) };
    let Some(i) = l.inertial else { panic!("no inertial") };
    let m = 0.0027;
    assert!((i.mass - m).abs() < 1e-12, "{}", i.mass);
    let ixx = m * (0.01f64.powi(2) * 2.0) / 12.0;
    for k in 0..3 {
        assert!((i.inertia[k][k] - ixx).abs() < 1e-12 * ixx.max(1.0) + 1e-15, "{:?}", i.inertia);
    }
    assert!(i.inertia[0][1].abs() < 1e-15);
    // The centre of mass is the cube's centre, in the link frame.
    assert!(i.com.dist(solidcraft_doc::apply_point(&l.mesh_origin, solidcore_record::Vec3::new(105.0, 5.0, 5.0))) < 1e-9, "{:?}", i.com);
}

#[test]
fn urdf_names_are_unique_across_links_joints_and_files() {
    let mut s = Session::default();
    for name in ["Arm", "base", "Arm_link1"] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name, "grounded": false}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0]}));
    }
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [50, 50, 0]}));
    ground(&mut s, "base", true);
    run(
        &mut s,
        "joint.create",
        json!({"type": "cylindrical", "a": {"occurrence": "base", "face": [5, 5, 10]}, "b": {"occurrence": "Arm", "face": [5, 5, 0]}, "name": "Arm"}),
    );
    let p = package(&s);
    let (links, joints) = names(&p.model);
    let mut all: Vec<&str> = links.iter().chain(&joints).copied().collect();
    all.extend(p.meshes.iter().map(|(f, _)| f.as_str()));
    let n = all.len();
    all.sort();
    all.dedup();
    assert_eq!(all.len(), n, "{links:?} {joints:?}");
    assert!(joints.contains(&"Arm"), "{joints:?}");
}

#[test]
fn urdf_collision_skips_open_bodies() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Part"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0]}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    run(&mut s, "sketch.line", json!({"points": [[30, 0], [40, 0]], "ids": ["l1"]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "surface.extrude", json!({"sketch": "S", "curves": ["l1"], "distance": 10, "body_name": "Wall"}));
    let p = package(&s);
    let Some(l) = p.model.links.iter().find(|l| l.name == "Part") else { panic!("{:?}", names(&p.model)) };
    let (Some(v), [CollisionGeom::Mesh { file: c, .. }]) = (&l.visual, l.collision.as_slice()) else { panic!("{l:?}") };
    assert_ne!(v, c, "a separate collision mesh");
    let bytes = &p.meshes.iter().find(|(f, _)| f == c).unwrap().1;
    assert!(stl_volume(bytes).is_some(), "the collision mesh is closed");
    assert!(has_warning(&p, "not a closed solid"), "{:?}", p.warnings);
    // Mass from the cube alone (default density).
    let rho = s.doc.density(&s.model.state().bodies.first().map(|b| b.name.clone()).unwrap_or_default());
    assert!(l.inertial.is_some_and(|i| (i.mass - 1000.0 * rho * 1e-6).abs() < 1e-12));
}

#[test]
fn urdf_open_only_component_is_drawn() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Sheet"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    run(&mut s, "sketch.line", json!({"points": [[30, 0], [40, 0]], "ids": ["l1"]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "surface.extrude", json!({"sketch": "S", "curves": ["l1"], "distance": 10, "body_name": "Wall"}));
    let p = package(&s);
    let Some(l) = p.model.links.iter().find(|l| l.name == "Sheet") else { panic!("{:?}", names(&p.model)) };
    assert!(l.visual.is_some() && l.collision.is_empty(), "{l:?}");
    assert!(l.inertial.is_some_and(|i| i.mass == 1e-6), "{l:?}");
}

#[test]
fn urdf_skips_a_joint_that_closes_a_loop() {
    let mut s = two_cubes();
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "C"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [40, 0, 0]}));
    run(&mut s, "joint.rigid_group", json!({"occurrences": ["A", "B", "C"]}));
    run(&mut s, "joint.as_built", json!({"type": "rigid", "a": "B", "b": "C"}));
    let p = package(&s);
    assert!(has_warning(&p, "closes a loop"), "{:?}", p.warnings);
    assert_tree(&p.model);
    assert_fk(&p.model, &s, "loop");
}

/// Two grounded occurrences each reach C by a joint: the solver walks them in id order, so the
/// URDF keeps the joint from the one with the lower id, whatever order the file lists them in.
#[test]
fn urdf_walks_grounded_occurrences_in_id_order() {
    let mut s = two_cubes();
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "C"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [40, 0, 0]}));
    ground(&mut s, "B", true);
    run(&mut s, "joint.create", json!({"type": "rigid", "a": {"face": [25, 5, 10]}, "b": {"face": [45, 5, 0]}, "name": "FromB"}));
    run(&mut s, "joint.create", json!({"type": "rigid", "a": {"face": [5, 5, 10]}, "b": {"occurrence": "C", "face": [25, 5, 20]}, "name": "FromA"}));
    let mut doc = (*s.doc).clone();
    doc.occurrences.reverse();
    let p = solidcraft_io::urdf(&doc, &s.doc.painted(&s.model.state()), "t", &UrdfOptions::default()).unwrap();
    assert!(p.model.joints.iter().any(|j| j.name == "FromA"), "{:?}", names(&p.model));
    assert!(has_warning(&p, "joint `FromB` closes a loop"), "{:?}", p.warnings);
}

/// Every link but `base_link` has exactly one parent joint.
fn assert_tree(m: &UrdfModel) {
    for (i, l) in m.links.iter().enumerate().skip(1) {
        assert_eq!(m.joints.iter().filter(|j| j.child == i).count(), 1, "{}", l.name);
    }
    assert_eq!(m.joints.iter().filter(|j| j.child == 0).count(), 0);
}

#[test]
fn urdf_fixes_an_unjointed_occurrence_to_base() {
    let s = two_cubes();
    let p = package(&s);
    assert!(has_warning(&p, "`B` has no joint"), "{:?}", p.warnings);
    assert_tree(&p.model);
    assert_fk(&p.model, &s, "loose");
}

#[test]
fn urdf_of_an_ungrounded_assembly_keeps_its_joints() {
    let mut s = two_cubes();
    ground(&mut s, "A", false);
    joint(&mut s, "revolute", "Turn", Value::Null);
    run(&mut s, "joint.drive", json!({"joint": "Turn", "value": 20}));
    let p = package(&s);
    assert!(has_warning(&p, "`A` is not grounded"), "{:?}", p.warnings);
    assert!(p.model.joints.iter().any(|j| j.name == "Turn" && j.step == Step::Revolute));
    assert_fk(&p.model, &s, "ungrounded");
    run(&mut s, "joint.drive", json!({"joint": "Turn", "value": -70}));
    assert_fk(&p.model, &s, "ungrounded, driven");
}

#[test]
fn urdf_skips_a_joint_with_an_error() {
    let mut s = two_cubes();
    joint(&mut s, "revolute", "Turn", Value::Null);
    // Snap the joint to a face that then goes away.
    let Some(f) = s.doc.features.iter().rev().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::Box { .. })).map(|f| f.id) else {
        panic!("box")
    };
    run(&mut s, "timeline.suppress", json!({"feature": f, "suppressed": true}));
    let p = package(&s);
    assert!(has_warning(&p, "joint `Turn` is skipped"), "{:?}", p.warnings);
    assert_tree(&p.model);
}

#[test]
fn urdf_nested_occurrence_moves_with_its_parent() {
    let mut s = hinge();
    let b = s.doc.find_component("LeafB").unwrap_or(0);
    run(&mut s, "component.activate", json!({"component": "LeafB"}));
    run(&mut s, "component.create", json!({"name": "Pin", "parent": b}));
    run(&mut s, "solid.box", json!({"length": 4, "width": 4, "height": 4, "corner": [10, 10, 3]}));
    run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": 30}));
    let p = package(&s);
    assert!(!has_warning(&p, "Pin"), "{:?}", p.warnings);
    let pin = p.model.links.iter().position(|l| l.name == "Pin").unwrap_or(0);
    let leaf_b = p.model.links.iter().position(|l| l.name == "LeafB").unwrap_or(0);
    assert!(p.model.joints.iter().any(|j| j.child == pin && j.parent == leaf_b), "Pin hangs on LeafB");
    assert_fk(&p.model, &s, "nested");
    run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": 120}));
    assert_fk(&p.model, &s, "nested, driven");
}

#[test]
fn urdf_copies_nested_occurrences_of_each_placement() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Leg"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 30, "corner": [0, 0, 0]}));
    let leg = s.doc.find_component("Leg").unwrap_or(0);
    run(&mut s, "component.create", json!({"name": "Foot", "parent": leg}));
    run(&mut s, "solid.box", json!({"length": 12, "width": 12, "height": 2, "corner": [-1, -1, -2]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "occurrence.copy", json!({"component": "Leg", "translate": [50, 0, 0]}));
    assert_eq!(s.doc.placements().len(), 5, "root, two legs, two feet");
    let p = package(&s);
    assert_fk(&p.model, &s, "copies");
    assert_tree(&p.model);
}

/// Deeper than `placements()` walks (a file can hold it; commands stop at 64 levels): the
/// export says copies past the limit are left out.
#[test]
fn urdf_warns_when_placements_are_cut_off() {
    let s = Session::default();
    let mut doc = (*s.doc).clone();
    for k in 1..=66u64 {
        doc.components.push(solidcraft_doc::Component {
            id: 1000 + k,
            name: format!("C{k}"),
            parent: if k == 1 { 0 } else { 999 + k },
            ..Default::default()
        });
        doc.occurrences.push(solidcraft_doc::Occurrence {
            id: 2000 + k,
            component: 1000 + k,
            parent: if k == 1 { 0 } else { 999 + k },
            name: format!("C{k}:1"),
            transform: solidcraft_doc::IDENTITY,
            base_transform: None,
            grounded: false,
            bom_type: None,
            expressed_moves: Vec::new(),
        });
    }
    // A component placed twice with a part nested in it, next to the deep chain: its copy is
    // still there, since only the branch past the limit stops.
    doc.components.push(solidcraft_doc::Component { id: 3001, name: "Leg".into(), parent: 0, ..Default::default() });
    doc.components.push(solidcraft_doc::Component { id: 3002, name: "Foot".into(), parent: 3001, ..Default::default() });
    // Listed first, so the walk (a stack) goes down the chain before reaching them.
    for (id, component, parent, name) in [(4003, 3002, 3001, "Foot:1"), (4002, 3001, 0, "Leg:2"), (4001, 3001, 0, "Leg:1")] {
        let o = solidcraft_doc::Occurrence {
            id,
            component,
            parent,
            name: name.into(),
            transform: solidcraft_doc::IDENTITY,
            base_transform: None,
            grounded: false,
            bom_type: None,
            expressed_moves: Vec::new(),
        };
        doc.occurrences.insert(0, o);
    }
    let p = solidcraft_io::urdf(&doc, &s.model.state(), "t", &UrdfOptions::default()).unwrap();
    assert!(has_warning(&p, "left out"), "{:?}", p.warnings);
    assert!(p.model.links.iter().any(|l| l.path == [4002, 4003]), "the copy of Foot under Leg:2");
    assert_tree(&p.model);
}

#[test]
fn urdf_robot_name_is_xml_safe() {
    let s = hinge();
    for stem in ["a--b", "x\u{1}y", "<&>"] {
        let p = match solidcraft_io::urdf(&s.doc, &s.doc.painted(&s.model.state()), stem, &UrdfOptions::default()) {
            Ok(p) => p,
            Err(e) => panic!("{e}"),
        };
        assert!(well_formed(&p.xml).is_ok(), "{stem}: {:?}\n{}", well_formed(&p.xml), p.xml);
        assert!(p.xml.chars().all(|c| c == '\n' || !c.is_control()), "{stem}");
    }
}

/// The XML is well formed: every element closes, in order.
fn well_formed(xml: &str) -> Result<(), String> {
    let mut stack: Vec<String> = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find('<') {
        rest = &rest[i + 1..];
        let Some(end) = rest.find('>') else { return Err("unclosed tag".into()) };
        let tag = &rest[..end];
        rest = &rest[end + 1..];
        if tag.starts_with('?') || tag.starts_with('!') || tag.ends_with('/') {
            continue;
        }
        if let Some(name) = tag.strip_prefix('/') {
            if stack.pop().as_deref() != Some(name) {
                return Err(format!("</{name}> out of order"));
            }
        } else {
            stack.push(tag.split_whitespace().next().unwrap_or("").to_string());
        }
    }
    if stack.is_empty() { Ok(()) } else { Err(format!("unclosed {stack:?}")) }
}

#[test]
fn urdf_needs_the_whole_assembly() {
    let mut s = hinge();
    let d = temp("bodies");
    let e = s.execute("file.export", &json!({"path": d.join("x.urdf").to_string_lossy(), "bodies": ["Body1"]}));
    assert!(e.is_err_and(|e| e.to_string().contains("whole assembly")));
    assert_eq!(std::fs::read_dir(&d).map(|r| r.count()).unwrap_or(0), 0);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn urdf_files_stay_inside_allow_paths() {
    let mut s = hinge();
    let inside = temp("scope-in");
    let outside = temp("scope-out");
    let refused = solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([inside.clone()])), || {
        s.execute("file.export", &json!({"path": outside.join("r.urdf").to_string_lossy()}))
    });
    assert!(refused.is_err());
    assert_eq!(std::fs::read_dir(&outside).map(|r| r.count()).unwrap_or(0), 0, "nothing written");
    let ok = solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([inside.clone()])), || {
        s.execute("file.export", &json!({"path": inside.join("r.urdf").to_string_lossy()}))
    });
    assert!(ok.is_ok(), "{ok:?}");
    let _ = std::fs::remove_dir_all(&inside);
    let _ = std::fs::remove_dir_all(&outside);
}

#[test]
fn joint_dynamics_are_written_per_joint_and_the_export_value_is_only_a_fallback() {
    let mut s = hinge();
    run(&mut s, "joint.dynamics", json!({"joint": "Hinge", "effort": 12.5, "velocity": 2.0, "damping": 0.1}));
    let options = solidcraft_io::UrdfOptions { effort: Some(77.0), velocity: Some(7.0), ..Default::default() };
    let xml = solidcraft_io::urdf(&s.doc, &s.doc.painted(&s.model.state()), "robot", &options).map(|p| p.xml).unwrap_or_else(|e| panic!("{e}"));
    assert!(xml.contains(r#"effort="12.5" velocity="2""#), "{xml}");
    assert!(xml.contains(r#"<dynamics damping="0.1"/>"#), "{xml}");
    assert!(!xml.contains(r#"effort="77""#), "{xml}");
}

#[test]
fn a_joint_with_only_some_dynamics_takes_the_rest_from_the_fallback() {
    let mut s = hinge();
    run(&mut s, "joint.dynamics", json!({"joint": "Hinge", "effort": 12.5}));
    let p = package(&s);
    assert!(p.xml.contains(r#"effort="12.5" velocity="10""#), "{}", p.xml);
    assert!(!p.xml.contains("<dynamics"), "{}", p.xml);
    assert!(has_warning(&p, "Hinge"), "{:?}", p.warnings);
}

#[test]
fn a_joint_with_saved_limits_is_not_warned_about() {
    let mut s = hinge();
    run(&mut s, "joint.dynamics", json!({"joint": "Hinge", "effort": 12.5, "velocity": 2.0}));
    let p = package(&s);
    assert!(!has_warning(&p, "not given"), "{:?}", p.warnings);
}

#[test]
fn urdf_without_dynamics_is_unchanged() {
    let p = package(&hinge());
    assert!(!p.xml.contains("<dynamics"), "{}", p.xml);
    assert!(p.xml.contains(r#"effort="1000" velocity="10""#), "{}", p.xml);
    assert!(has_warning(&p, "not given"), "{:?}", p.warnings);
}

#[test]
fn a_chain_reached_from_its_b_side_keeps_each_values_dynamics() {
    let mut s = two_cubes();
    ground(&mut s, "A", false);
    ground(&mut s, "B", true);
    joint(&mut s, "cylindrical", "Sleeve", json!([]));
    run(&mut s, "joint.dynamics", json!({"joint": "Sleeve", "index": 0, "damping": 0.1}));
    run(&mut s, "joint.dynamics", json!({"joint": "Sleeve", "index": 1, "damping": 0.2}));
    let p = package(&s);
    let damping = |step| p.model.joints.iter().find(|j| j.step == step).and_then(|j| j.dynamics.damping);
    assert_eq!(damping(Step::Revolute), Some(0.1), "{}", p.xml);
    assert_eq!(damping(Step::Prismatic), Some(0.2), "{}", p.xml);
}

#[test]
fn hand_edited_dynamics_are_ignored_with_a_warning() {
    let mut s = hinge();
    let bad =
        solidcraft_doc::joints::JointDynamics { effort: Some(f64::NAN), velocity: Some(-1.0), damping: Some(f64::INFINITY), friction: Some(-2.0) };
    if let Some(j) = s.doc_mut().assembly.joints.first_mut() {
        j.dynamics = vec![Some(bad); 5];
    }
    let p = package(&s);
    assert!(!p.xml.contains("<dynamics"), "{}", p.xml);
    assert!(p.xml.contains(r#"effort="1000" velocity="10""#), "{}", p.xml);
    assert!(has_warning(&p, "dynamics"), "{:?}", p.warnings);
}

/// A grounded cube and `kinds.len()` more, each on a joint to it named `J0`, `J1`...
pub(super) fn fan(kinds: &[&str]) -> Session {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Hub"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0]}));
    ground(&mut s, "Hub", true);
    for (i, kind) in kinds.iter().enumerate() {
        let x = 20 * (i + 1);
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": format!("P{i}")}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
        run(&mut s, "joint.create", json!({"type": kind, "a": {"face": [5, 5, 10]}, "b": {"face": [x + 5, 5, 0]}, "name": format!("J{i}")}));
    }
    s
}

/// `b = ratio · a + offset` in stored units (rad, mm), between values 0 of two joints.
pub(super) fn link(s: &mut Session, a: &str, b: &str, ratio: f64, offset: f64) {
    let id = |s: &Session, n: &str| s.doc.assembly.joints.iter().find(|j| j.name == n).map_or(0, |j| j.id);
    let (a, b) = (id(s, a), id(s, b));
    s.doc_mut().assembly.links.push(joints::MotionLink { a, ia: 0, b, ib: 0, ratio, offset, coupling: None, lift: Vec::new() });
}

fn mimic_package(s: &Session) -> UrdfPackage {
    let options = UrdfOptions { mimic: true, ..Default::default() };
    match solidcraft_io::urdf(&s.doc, &s.doc.painted(&s.model.state()), "t", &options) {
        Ok(p) => p,
        Err(e) => panic!("urdf: {e}"),
    }
}

/// (driver's name, multiplier, offset) of a URDF joint.
fn mimic_of(p: &UrdfPackage, joint: &str) -> Option<(String, f64, f64)> {
    let m = p.model.joints.iter().find(|j| j.name == joint)?.mimic?;
    Some((p.model.joints.get(m.joint)?.name.clone(), m.multiplier, m.offset))
}

#[test]
fn a_gear_pair_writes_mimic() {
    let mut s = fan(&["revolute", "revolute"]);
    run(&mut s, "joint.gear", json!({"a": "J0", "b": "J1", "ra": 10, "rb": 20}));
    let p = mimic_package(&s);
    let (driver, multiplier, offset) = mimic_of(&p, "J1").unwrap_or_else(|| panic!("{}", p.xml));
    assert_eq!(driver, "J0");
    // A gear twice the size turns half as far, the other way.
    assert!((multiplier + 0.5).abs() < 1e-12, "{multiplier}");
    assert!(offset.abs() < 1e-12, "{offset}");
    assert!(p.xml.contains(r#"<mimic joint="J0" multiplier="-0.5" offset="0"/>"#), "{}", p.xml);
    assert!(mimic_of(&p, "J0").is_none());
}

#[test]
fn a_rack_and_pinion_converts_mm_per_rad_to_m_per_rad() {
    let mut s = fan(&["revolute", "slider"]);
    run(&mut s, "joint.rack_pinion", json!({"a": "J0", "b": "J1", "radius": 5}));
    let p = mimic_package(&s);
    let (_, multiplier, _) = mimic_of(&p, "J1").unwrap_or_else(|| panic!("{}", p.xml));
    assert!((multiplier.abs() - 0.005).abs() < 1e-12, "{multiplier}");
}

#[test]
fn a_prismatic_driver_converts_rad_per_mm_to_rad_per_m() {
    let mut s = fan(&["slider", "revolute"]);
    link(&mut s, "J0", "J1", 2.0, 0.25);
    let p = mimic_package(&s);
    assert_eq!(mimic_of(&p, "J1"), Some(("J0".into(), 2000.0, 0.25)), "{}", p.xml);
}

#[test]
fn a_prismatic_follower_writes_its_offset_in_metres() {
    let mut s = fan(&["slider", "slider"]);
    link(&mut s, "J0", "J1", 3.0, 7.0);
    let p = mimic_package(&s);
    assert_eq!(mimic_of(&p, "J1"), Some(("J0".into(), 3.0, 0.007)), "{}", p.xml);
}

#[test]
fn linked_links_are_flattened_to_the_first_driver() {
    let mut s = fan(&["revolute", "revolute", "revolute"]);
    link(&mut s, "J0", "J1", 2.0, 0.0);
    link(&mut s, "J1", "J2", 3.0, 0.1);
    let p = mimic_package(&s);
    assert_eq!(mimic_of(&p, "J1"), Some(("J0".into(), 2.0, 0.0)), "{}", p.xml);
    assert_eq!(mimic_of(&p, "J2"), Some(("J0".into(), 6.0, 0.1)), "{}", p.xml);
}

#[test]
fn flattening_scales_the_earlier_offset() {
    let mut s = fan(&["revolute", "revolute", "revolute"]);
    link(&mut s, "J0", "J1", 2.0, 0.5);
    link(&mut s, "J1", "J2", 3.0, 0.1);
    let p = mimic_package(&s);
    let (_, m, o) = mimic_of(&p, "J2").unwrap_or_else(|| panic!("{}", p.xml));
    assert_eq!(m, 6.0);
    assert!((o - 1.6).abs() < 1e-12, "{o}");
}

#[test]
fn a_cycle_of_links_writes_no_mimic_and_warns() {
    let mut s = fan(&["revolute", "revolute"]);
    link(&mut s, "J0", "J1", 2.0, 0.0);
    link(&mut s, "J1", "J0", 0.5, 0.0);
    let p = mimic_package(&s);
    assert!(!p.xml.contains("<mimic"), "{}", p.xml);
    assert!(has_warning(&p, "cycle"), "{:?}", p.warnings);
}

#[test]
fn a_suppressed_driver_leaves_the_follower_free_and_warns() {
    let mut s = fan(&["revolute", "revolute"]);
    link(&mut s, "J0", "J1", 2.0, 0.0);
    run(&mut s, "joint.edit", json!({"joint": "J0", "suppressed": true}));
    let p = mimic_package(&s);
    assert!(!p.xml.contains("<mimic"), "{}", p.xml);
    assert!(p.warnings.iter().any(|w| w.contains("J1") && w.contains("free")), "{:?}", p.warnings);
}

#[test]
fn a_follower_mimics_the_farthest_driver_still_in_the_urdf() {
    // J0 drives J1 drives J2; with J0 suppressed J1 is free, so J2 follows J1.
    let mut s = fan(&["revolute", "revolute", "revolute"]);
    link(&mut s, "J0", "J1", 2.0, 0.5);
    link(&mut s, "J1", "J2", 3.0, 0.1);
    run(&mut s, "joint.edit", json!({"joint": "J0", "suppressed": true}));
    let p = mimic_package(&s);
    assert_eq!(mimic_of(&p, "J1"), None, "{}", p.xml);
    assert_eq!(mimic_of(&p, "J2"), Some(("J1".into(), 3.0, 0.1)), "{}", p.xml);
}

#[test]
fn a_driver_that_closes_a_loop_leaves_the_follower_free_and_warns() {
    let mut s = fan(&["revolute", "revolute"]);
    // P0 to P1 closes the loop Hub, P0, P1: the tree leaves this joint out.
    run(
        &mut s,
        "joint.create",
        json!({"type": "revolute", "a": {"occurrence": "P0:1", "face": [25, 5, 10]}, "b": {"occurrence": "P1:1", "face": [45, 5, 0]}, "name": "Closing"}),
    );
    let id = s.doc.assembly.joints.iter().find(|j| j.name == "Closing").map_or(0, |j| j.id);
    let j1 = s.doc.assembly.joints.iter().find(|j| j.name == "J1").map_or(0, |j| j.id);
    s.doc_mut().assembly.links.push(joints::MotionLink { a: id, ia: 0, b: j1, ib: 0, ratio: 2.0, offset: 0.0, coupling: None, lift: Vec::new() });
    let p = mimic_package(&s);
    assert!(!p.xml.contains("<mimic"), "{}", p.xml);
    assert!(p.warnings.iter().any(|w| w.contains("J1") && w.contains("free")), "{:?}", p.warnings);
}

#[test]
fn mimic_values_match_solidcraft_when_the_chain_runs_reversed() {
    // The Hub is on B's side of the follower: its chain runs backwards.
    let mut s = Session::default();
    for (name, x) in [("Hub", 0), ("Driver", 20), ("Follower", 40)] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
    }
    ground(&mut s, "Hub", true);
    run(
        &mut s,
        "joint.create",
        json!({"type": "revolute", "a": {"occurrence": "Hub:1", "face": [5, 5, 10]}, "b": {"occurrence": "Driver:1", "face": [25, 5, 0]}, "name": "D"}),
    );
    run(
        &mut s,
        "joint.create",
        json!({"type": "revolute", "a": {"occurrence": "Follower:1", "face": [45, 5, 0]}, "b": {"occurrence": "Hub:1", "face": [5, 5, 0]}, "name": "F"}),
    );
    link(&mut s, "D", "F", 2.0, 0.1);
    run(&mut s, "joint.drive", json!({"joint": "D", "values": [20]}));
    let p = mimic_package(&s);
    let (driver, m, o) = mimic_of(&p, "F").unwrap_or_else(|| panic!("{}", p.xml));
    assert_eq!(driver, "D");
    let driver_value = 20f64.to_radians();
    let frames = p.model.link_frames(|j| match j.name.as_str() {
        "D" => driver_value,
        "F" => m * driver_value + o,
        _ => 0.0,
    });
    let placements = s.doc.placements();
    for (l, f) in p.model.links.iter().zip(&frames) {
        if l.path.is_empty() {
            continue;
        }
        let Some((_, _, want)) = placements.iter().find(|x| x.1 == l.path) else { panic!("{}", l.name) };
        assert!(close(&mat_mul(f, &l.mesh_origin), want), "{}", l.name);
    }
}

#[test]
fn mimic_false_gives_the_previous_output() {
    let mut s = fan(&["revolute", "revolute"]);
    link(&mut s, "J0", "J1", 2.0, 0.0);
    assert!(!package(&s).xml.contains("<mimic"));
}

#[test]
fn file_export_writes_mimic_only_when_asked() {
    let mut s = fan(&["revolute", "revolute"]);
    link(&mut s, "J0", "J1", 2.0, 0.0);
    let dir = temp("mimic");
    let on = dir.join("on.urdf");
    let off = dir.join("off.urdf");
    run(&mut s, "file.export", json!({"path": on.to_string_lossy(), "mimic": true}));
    run(&mut s, "file.export", json!({"path": off.to_string_lossy()}));
    assert!(std::fs::read_to_string(&on).unwrap_or_default().contains("<mimic"));
    assert!(!std::fs::read_to_string(&off).unwrap_or_default().contains("<mimic"));
}

#[test]
fn a_follower_limit_the_driver_can_exceed_warns() {
    let mut s = fan(&["revolute", "revolute"]);
    link(&mut s, "J0", "J1", 2.0, 0.0);
    run(&mut s, "joint.limits", json!({"joint": "J0", "min": -30, "max": 30}));
    run(&mut s, "joint.limits", json!({"joint": "J1", "min": -20, "max": 20}));
    assert!(has_warning(&mimic_package(&s), "exceed"), "driver reaches 60 deg");
    let mut s = fan(&["revolute", "revolute"]);
    link(&mut s, "J0", "J1", 2.0, 0.0);
    run(&mut s, "joint.limits", json!({"joint": "J0", "min": -30, "max": 30}));
    run(&mut s, "joint.limits", json!({"joint": "J1", "min": -90, "max": 90}));
    assert!(!has_warning(&mimic_package(&s), "exceed"));
}

/// `two_cubes` on a revolute joint, with a flagged origin "tool0" on the top of B.
pub(super) fn tool_frame() -> Session {
    let mut s = two_cubes();
    joint(&mut s, "revolute", "Turn", json!([]));
    let b = occ(&s, "B");
    // The joint has moved B onto A: its top face is at z = 20 now.
    run(&mut s, "joint.origin", json!({"name": "tool0", "occurrence": b, "face": [5, 5, 20], "export_frame": true}));
    s
}

/// A link's frame at the design's current joint values.
fn frame_of(p: &UrdfPackage, s: &Session, link: &str) -> Mat {
    let vals = joints::joint_values(&s.doc.assembly);
    let frames = p.model.link_frames(|j| j.source.and_then(|(id, k)| vals.get(&id)?.get(k).copied()).unwrap_or(0.0));
    let at = p.model.links.iter().position(|l| l.name == link).unwrap_or_else(|| panic!("no link {link}: {}", p.xml));
    frames[at]
}

fn world_of_origin(s: &Session, name: &str) -> Mat {
    let o = s.doc.assembly.origins.iter().find(|o| o.name == name).map(|o| o.origin.clone());
    let o = o.unwrap_or_else(|| panic!("no origin {name}"));
    s.doc.origin_world(&s.model.state(), &o).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn a_flagged_origin_becomes_a_fixed_massless_link() {
    let s = tool_frame();
    let p = package(&s);
    assert!(p.xml.contains(r#"<link name="tool0">"#) || p.xml.contains(r#"<link name="tool0"/>"#), "{}", p.xml);
    let l = p.model.links.iter().find(|l| l.name == "tool0").unwrap_or_else(|| panic!("{}", p.xml));
    assert!(l.inertial.is_none() && l.visual.is_none() && l.collision.is_empty());
    let at = p.model.links.iter().position(|l| l.name == "tool0");
    let j = p.model.joints.iter().find(|j| Some(j.child) == at).unwrap_or_else(|| panic!("{}", p.xml));
    assert_eq!(j.step, Step::Fixed);
    assert_eq!(p.model.links[j.parent].name, "B");
    let frame = frame_of(&p, &s, "tool0");
    assert!(close(&frame, &world_of_origin(&s, "tool0")));
    assert!((frame[3][0] - 5.0).abs() < 1e-9 && (frame[3][1] - 5.0).abs() < 1e-9 && (frame[3][2] - 20.0).abs() < 1e-9, "{frame:?}");
}

#[test]
fn an_unflagged_origin_adds_nothing() {
    let mut s = tool_frame();
    let before = package(&s).model.links.len();
    run(&mut s, "joint.origin_frame", json!({"origin": "tool0", "frame": false}));
    assert_eq!(package(&s).model.links.len(), before - 1);
    run(&mut s, "joint.origin_frame", json!({"origin": "tool0", "frame": true}));
    assert_eq!(package(&s).model.links.len(), before);
    assert!(s.execute("joint.origin_frame", &json!({"origin": "nobody", "frame": true})).is_err());
    // `frame` on joint.origin is an explicit frame; the flag is `export_frame`, and only a bool.
    let b = occ(&s, "B");
    assert!(s.execute("joint.origin", &json!({"name": "t1", "occurrence": b, "face": [5, 5, 20], "export_frame": "yes"})).is_err());
}

#[test]
fn a_frame_moves_with_its_part_when_the_joint_drives() {
    let mut s = tool_frame();
    run(&mut s, "joint.drive", json!({"joint": "Turn", "values": [40]}));
    let p = package(&s);
    assert!(close(&frame_of(&p, &s, "tool0"), &world_of_origin(&s, "tool0")));
}

#[test]
fn a_frame_name_taken_by_a_joint_gets_a_suffix() {
    let mut s = two_cubes();
    joint(&mut s, "revolute", "Turn", json!([]));
    let b = occ(&s, "B");
    run(&mut s, "joint.origin", json!({"name": "Turn", "occurrence": b, "face": [5, 5, 20], "export_frame": true}));
    let p = package(&s);
    assert!(p.model.links.iter().any(|l| l.name == "Turn_2"), "{}", p.xml);
    assert!(p.model.joints.iter().any(|j| j.name == "Turn"), "{}", p.xml);
}

#[test]
fn a_frame_whose_face_is_gone_is_skipped_with_a_warning() {
    let mut s = tool_frame();
    let mut lost = s.doc.assembly.origins[0].origin.clone();
    lost.occurrence = 987_654;
    s.doc_mut().assembly.origins[0].origin = lost;
    let p = package(&s);
    assert!(!p.model.links.iter().any(|l| l.name == "tool0"), "{}", p.xml);
    assert!(p.warnings.iter().any(|w| w.contains("tool0")), "{:?}", p.warnings);
}

#[test]
fn a_frame_on_a_component_placed_twice_appears_once() {
    let mut s = tool_frame();
    run(&mut s, "occurrence.copy", json!({"component": "B", "translate": [50, 0, 0]}));
    let p = package(&s);
    assert_eq!(p.model.links.iter().filter(|l| l.name.starts_with("tool0")).count(), 1, "{}", p.xml);
}

/// A link's mass, centre of mass (mm) and inertia.
fn inertial_of(p: &UrdfPackage, link: &str) -> solidcraft_io::urdf::Inertial {
    let l = p.model.links.iter().find(|l| l.name == link).unwrap_or_else(|| panic!("{}", p.xml));
    l.inertial.unwrap_or_else(|| panic!("{} has no inertial", l.name))
}

#[test]
fn a_mass_override_scales_inertia_and_keeps_the_centre() {
    let mut s = two_cubes();
    let before = inertial_of(&package(&s), "B");
    let mass = before.mass * 2.0;
    run(&mut s, "component.physical_override", json!({"component": "B", "mass": mass}));
    let after = inertial_of(&package(&s), "B");
    assert!((after.mass - mass).abs() < 1e-12);
    for (a, b) in after.inertia.iter().flatten().zip(before.inertia.iter().flatten()) {
        assert!((a - 2.0 * b).abs() < 1e-15, "{a} {b}");
    }
    let moved = after.com - before.com;
    assert!(moved.dot(moved) < 1e-24);
}

#[test]
fn a_full_override_replaces_all_three() {
    let mut s = two_cubes();
    run(&mut s, "component.physical_override", json!({"component": "B", "mass": 3.0, "com": [21, 2, 3], "inertia": [1.0, 2.0, 3.0, 0.1, 0.2, 0.3]}));
    let i = inertial_of(&package(&s), "B");
    assert_eq!(i.mass, 3.0);
    let off = i.com - solidcore_record::Vec3::new(21.0, 2.0, 3.0);
    assert!(off.dot(off) < 1e-18, "{:?}", i.com);
    assert_eq!(i.inertia, [[1.0, 0.1, 0.2], [0.1, 2.0, 0.3], [0.2, 0.3, 3.0]]);
}

/// A link's centre of mass (mm) and inertia, in world axes.
fn world_inertial(p: &UrdfPackage, s: &Session, link: &str) -> (solidcore_record::Vec3, [[f64; 3]; 3]) {
    let i = inertial_of(p, link);
    let f = frame_of(p, s, link);
    let com = solidcraft_doc::apply_point(&f, i.com);
    let r = |a: usize, b: usize| f[b][a];
    let mut out = [[0.0; 3]; 3];
    for (x, row) in out.iter_mut().enumerate() {
        for (y, cell) in row.iter_mut().enumerate() {
            for a in 0..3 {
                for b in 0..3 {
                    *cell += r(x, a) * i.inertia[a][b] * r(y, b);
                }
            }
        }
    }
    (com, out)
}

fn assert_world_inertial(p: &UrdfPackage, s: &Session, link: &str, com: [f64; 3], diagonal: [f64; 3]) {
    let (c, i) = world_inertial(p, s, link);
    let off = c - solidcore_record::Vec3::new(com[0], com[1], com[2]);
    assert!(off.dot(off) < 1e-16, "{link}: centre {c:?}, expected {com:?}");
    for k in 0..3 {
        assert!((i[k][k] - diagonal[k]).abs() < 1e-9, "{link}: {i:?}, expected {diagonal:?}");
    }
}

#[test]
fn each_placement_gets_the_override_in_its_own_frame() {
    let mut s = two_cubes();
    run(&mut s, "component.physical_override", json!({"component": "B", "mass": 3.0, "com": [21, 0, 0], "inertia": [1.0, 2.0, 3.0, 0.0, 0.0, 0.0]}));
    run(&mut s, "occurrence.copy", json!({"component": "B"}));
    let copy = s.doc.occurrences.iter().map(|o| o.id).max().unwrap_or(0);
    // A quarter turn about z: x goes to y, so xx and yy swap and the centre swings to +y.
    run(&mut s, "occurrence.move", json!({"occurrence": copy, "axis": [0, 0, 1], "angle": "90 deg", "origin": [0, 0, 0]}));
    let p = package(&s);
    assert_world_inertial(&p, &s, "B_1", [21.0, 0.0, 0.0], [1.0, 2.0, 3.0]);
    assert_world_inertial(&p, &s, "B_2", [0.0, 21.0, 0.0], [2.0, 1.0, 3.0]);
}

#[test]
fn an_override_on_a_jointed_part_follows_it() {
    let mut s = two_cubes();
    // B's bottom centre (25, 5, 0) goes to A's top centre (5, 5, 10): a move of (-20, 0, 10).
    joint(&mut s, "revolute", "Turn", json!([]));
    run(&mut s, "component.physical_override", json!({"component": "B", "mass": 3.0, "com": [25, 5, 5], "inertia": [1.0, 2.0, 3.0, 0.0, 0.0, 0.0]}));
    let p = package(&s);
    assert_world_inertial(&p, &s, "B", [5.0, 5.0, 15.0], [1.0, 2.0, 3.0]);
}

#[test]
fn nested_components_keep_their_own_values() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Leg"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 30, "corner": [0, 0, 0]}));
    let leg = s.doc.find_component("Leg").unwrap_or(0);
    run(&mut s, "component.create", json!({"name": "Foot", "parent": leg}));
    run(&mut s, "solid.box", json!({"length": 12, "width": 12, "height": 2, "corner": [-1, -1, -2]}));
    let foot = inertial_of(&package(&s), "Foot").mass;
    run(&mut s, "component.physical_override", json!({"component": "Leg", "mass": 9.0}));
    let p = package(&s);
    assert_eq!(inertial_of(&p, "Leg").mass, 9.0);
    assert_eq!(inertial_of(&p, "Foot").mass, foot);
}

#[test]
fn the_root_design_is_refused() {
    let mut s = two_cubes();
    assert!(s.execute("component.physical_override", &json!({"component": "root", "mass": 1.0})).is_err());
}

#[test]
fn mass_alone_on_open_surfaces_is_refused_and_works_with_inertia() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Sheet"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    run(&mut s, "sketch.line", json!({"points": [[30, 0], [40, 0]], "ids": ["l1"]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "surface.extrude", json!({"sketch": "S", "curves": ["l1"], "distance": 10, "body_name": "Wall"}));
    assert!(s.execute("component.physical_override", &json!({"component": "Sheet", "mass": 1.0})).is_err());
    run(&mut s, "component.physical_override", json!({"component": "Sheet", "mass": 1.0, "inertia": [1, 1, 1, 0, 0, 0]}));
    assert_eq!(inertial_of(&package(&s), "Sheet").mass, 1.0);
}

#[test]
fn hand_edited_overrides_are_ignored_with_a_warning() {
    let mut s = two_cubes();
    let before = inertial_of(&package(&s), "B").mass;
    let bad = solidcraft_doc::PhysicalOverride { mass: f64::NAN, com: None, inertia: None };
    let id = s.doc.find_component("B").unwrap_or(0);
    if let Some(c) = s.doc_mut().components.iter_mut().find(|c| c.id == id) {
        c.physical_override = Some(bad);
    }
    let p = package(&s);
    assert_eq!(inertial_of(&p, "B").mass, before);
    assert!(p.warnings.iter().any(|w| w.contains("override") && w.contains('B')), "{:?}", p.warnings);
}

#[test]
fn physical_override_refuses_hostile_values() {
    let mut s = two_cubes();
    for p in [
        json!({"component": "B", "mass": 0.0}),
        json!({"component": "B", "mass": -1.0}),
        json!({"component": "B", "mass": 1e300}),
        json!({"component": "B", "mass": 1.0, "com": [1, 2]}),
        json!({"component": "B", "mass": 1.0, "inertia": [1, 1, 1]}),
        json!({"component": "B", "mass": 1.0, "inertia": [0, 1, 1, 0, 0, 0]}),
        json!({"component": "B", "com": [1, 2, 3]}),
        json!({"component": "Nobody", "mass": 1.0}),
    ] {
        assert!(s.execute("component.physical_override", &p).is_err(), "{p}");
    }
}

#[test]
fn omitting_every_value_clears_the_override() {
    let mut s = two_cubes();
    let before = inertial_of(&package(&s), "B").mass;
    run(&mut s, "component.physical_override", json!({"component": "B", "mass": 5.0}));
    run(&mut s, "component.physical_override", json!({"component": "B"}));
    assert_eq!(inertial_of(&package(&s), "B").mass, before);
}

#[test]
fn properties_keep_the_computed_mass_and_note_the_override() {
    let mut s = two_cubes();
    let before = run(&mut s, "component.properties", json!({"component": "B"}));
    run(&mut s, "component.physical_override", json!({"component": "B", "mass": 5.0}));
    let after = run(&mut s, "component.properties", json!({"component": "B"}));
    assert_eq!(after["mass"], before["mass"]);
    assert_eq!(after["physical_override"]["mass"], json!(5.0), "{after}");
}

#[test]
fn a_body_keeps_its_collision_shape_when_renamed() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Block"}));
    run(&mut s, "body.collision", json!({"body": "Block", "shape": "box"}));
    assert_eq!(s.doc.collision_shape("Block"), solidcraft_doc::CollisionShape::Box);
    run(&mut s, "body.rename", json!({"body": "Block", "name": "Brick"}));
    assert_eq!(s.doc.collision_shape("Brick"), solidcraft_doc::CollisionShape::Box);
    assert_eq!(s.doc.collision_shape("Block"), solidcraft_doc::CollisionShape::Auto);
}

#[test]
fn setting_a_component_sets_all_its_bodies() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "C"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "One"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "body_name": "Two", "operation": "new"}));
    run(&mut s, "body.collision", json!({"component": "C", "shape": "none"}));
    for b in ["One", "Two"] {
        assert_eq!(s.doc.collision_shape(b), solidcraft_doc::CollisionShape::None, "{b}");
    }
}

#[test]
fn auto_removes_the_entry() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Block"}));
    run(&mut s, "body.collision", json!({"body": "Block", "shape": "sphere"}));
    run(&mut s, "body.collision", json!({"body": "Block", "shape": "auto"}));
    assert!(s.doc.collision_shapes.is_empty());
}

#[test]
fn body_collision_refuses_bad_requests() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "C"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Block"}));
    for p in [
        json!({"body": "Block", "component": "C", "shape": "box"}),
        json!({"shape": "box"}),
        json!({"body": "Block", "shape": "blob"}),
        json!({"body": "Nobody", "shape": "box"}),
        json!({"component": "Nobody", "shape": "box"}),
        json!({"body": "Block"}),
    ] {
        assert!(s.execute("body.collision", &p).is_err(), "{p}");
    }
    assert!(s.doc.collision_shapes.is_empty());
}

#[test]
fn a_pasted_component_gets_its_bodies_shapes() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "C"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Block"}));
    run(&mut s, "body.collision", json!({"body": "Block", "shape": "sphere"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.paste_new", json!({"component": "C"}));
    assert_eq!(s.doc.collision_shapes.len(), 2, "{:?}", s.doc.collision_shapes);
    assert!(s.doc.collision_shapes.values().all(|x| *x == solidcraft_doc::CollisionShape::Sphere));
}

fn collisions_of<'a>(p: &'a UrdfPackage, link: &str) -> &'a [CollisionGeom] {
    p.model.links.iter().find(|l| l.name == link).map_or(&[], |l| l.collision.as_slice())
}

#[test]
fn an_unconfigured_design_shares_the_visual_mesh_as_before() {
    let p = package(&two_cubes());
    let files: Vec<&str> = p.meshes.iter().map(|m| m.0.as_str()).collect();
    assert_eq!(files, ["t-A.stl", "t-B.stl"]);
    assert!(matches!(collisions_of(&p, "B"), [CollisionGeom::Mesh { file, .. }] if file == "t-B.stl"));
}

#[test]
fn none_writes_no_collision_and_no_warning() {
    let mut s = two_cubes();
    run(&mut s, "body.collision", json!({"component": "B", "shape": "none"}));
    let p = package(&s);
    assert!(collisions_of(&p, "B").is_empty());
    assert!(!collisions_of(&p, "A").is_empty());
    assert!(!p.warnings.iter().any(|w| w.contains("collision")), "{:?}", p.warnings);
    assert_eq!(p.meshes.len(), 2, "no mesh for a body with no collision: {:?}", p.meshes.iter().map(|m| &m.0).collect::<Vec<_>>());
}

#[test]
fn a_box_shape_writes_a_metre_primitive_with_its_origin() {
    let mut s = two_cubes();
    run(&mut s, "body.collision", json!({"component": "B", "shape": "box"}));
    let p = package(&s);
    assert!(p.xml.contains(r#"<box size="0.01 0.01 0.01"/>"#), "{}", p.xml);
    let [CollisionGeom::Box { origin, .. }] = collisions_of(&p, "B") else { panic!("{}", p.xml) };
    // The cube at x 20..30, y 0..10, z 0..10 is centred at (25, 5, 5).
    let link = frame_of(&p, &s, "B");
    let at = mat_mul(&link, origin);
    assert!((at[3][0] - 25.0).abs() < 1e-9 && (at[3][1] - 5.0).abs() < 1e-9 && (at[3][2] - 5.0).abs() < 1e-9, "{at:?}");
}

#[test]
fn a_cylinder_shape_points_along_urdf_z() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Post"}));
    // Long along world y: the cylinder's z has to turn onto it.
    run(&mut s, "solid.box", json!({"length": 10, "width": 40, "height": 10, "corner": [0, 0, 0]}));
    run(&mut s, "body.collision", json!({"component": "Post", "shape": "cylinder"}));
    let p = package(&s);
    let [CollisionGeom::Cylinder { origin, radius, length }] = collisions_of(&p, "Post") else { panic!("{}", p.xml) };
    // The 10 x 40 x 10 box: along y the cylinder is the smallest, 40 long and as wide as the
    // larger of the other two sides, 10, so radius 5.
    assert!((length - 40.0).abs() < 1e-9, "{length}");
    assert!((radius - 5.0).abs() < 1e-9, "{radius}");
    let at = mat_mul(&frame_of(&p, &s, "Post"), origin);
    assert!(at[2][1].abs() > 1.0 - 1e-9, "{at:?}");
    assert!((at[3][1] - 20.0).abs() < 1e-9, "{at:?}");
}

#[test]
fn a_sphere_shape_is_as_wide_as_the_bodys_largest_side() {
    let mut s = two_cubes();
    run(&mut s, "body.collision", json!({"component": "B", "shape": "sphere"}));
    let p = package(&s);
    let [CollisionGeom::Sphere { radius, .. }] = collisions_of(&p, "B") else { panic!("{}", p.xml) };
    // The 10 mm cube: radius 5.
    assert!((radius - 5.0).abs() < 1e-9, "{radius}");
}

/// A flat open surface in component "Sheet", body "Wall".
fn sheet() -> Session {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Sheet"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    run(&mut s, "sketch.line", json!({"points": [[30, 0], [40, 0]], "ids": ["l1"]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "surface.extrude", json!({"sketch": "S", "curves": ["l1"], "distance": 10, "body_name": "Wall"}));
    s
}

#[test]
fn convex_hull_from_a_development_build_reads_as_a_mesh_and_is_refused_now() {
    let read: solidcraft_doc::CollisionShape = serde_json::from_value(json!("convex_hull")).unwrap_or_default();
    assert_eq!(read, solidcraft_doc::CollisionShape::Mesh);
    let mut s = two_cubes();
    assert!(s.execute("body.collision", &json!({"component": "B", "shape": "convex_hull"})).is_err());
}

#[test]
fn an_open_surface_given_a_box_collides_and_adds_no_mass() {
    let mut s = sheet();
    run(&mut s, "body.collision", json!({"component": "Sheet", "shape": "box"}));
    let p = package(&s);
    assert!(matches!(collisions_of(&p, "Sheet"), [CollisionGeom::Box { .. }]), "{}", p.xml);
    assert_eq!(inertial_of(&p, "Sheet").mass, 1e-6);
}

#[test]
fn an_inertia_breaking_the_triangle_inequality_warns() {
    let mut s = two_cubes();
    run(&mut s, "component.physical_override", json!({"component": "B", "mass": 1.0, "inertia": [10.0, 1.0, 1.0, 0.0, 0.0, 0.0]}));
    let p = package(&s);
    assert!(p.warnings.iter().any(|w| w.contains("inertia") && w.contains('B')), "{:?}", p.warnings);
}

#[test]
fn a_plain_hinge_gets_no_plausibility_warning() {
    let p = package(&hinge());
    assert!(!p.warnings.iter().any(|w| w.contains("inertia") || w.contains("no mass") || w.contains("triangles")), "{:?}", p.warnings);
}

#[test]
fn a_ball_chains_in_between_links_get_no_plausibility_warning() {
    let mut s = two_cubes();
    joint(&mut s, "ball", "Ball", json!([]));
    let p = package(&s);
    assert!(!p.warnings.iter().any(|w| w.contains("no mass")), "{:?}", p.warnings);
}

#[test]
fn a_pasted_component_keeps_its_mass_override() {
    let mut s = two_cubes();
    run(&mut s, "component.physical_override", json!({"component": "B", "mass": 5.0}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.paste_new", json!({"component": "B"}));
    let with: Vec<_> = s.doc.components.iter().filter(|c| c.physical_override.is_some()).collect();
    assert_eq!(with.len(), 2, "{:?}", s.doc.components.iter().map(|c| &c.name).collect::<Vec<_>>());
}

#[test]
fn body_collision_refuses_a_component_with_no_bodies_of_its_own() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Group"}));
    let group = s.doc.find_component("Group").unwrap_or(0);
    run(&mut s, "component.create", json!({"name": "Child", "parent": group}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    assert!(s.execute("body.collision", &json!({"component": "Group", "shape": "box"})).is_err());
    assert!(s.doc.collision_shapes.is_empty());
}

#[test]
fn a_mass_only_override_with_nothing_to_scale_warns() {
    let mut s = sheet();
    let id = s.doc.find_component("Sheet").unwrap_or(0);
    let o = solidcraft_doc::PhysicalOverride { mass: 2.0, com: None, inertia: None };
    if let Some(c) = s.doc_mut().components.iter_mut().find(|c| c.id == id) {
        c.physical_override = Some(o);
    }
    let p = package(&s);
    assert!(p.warnings.iter().any(|w| w.contains("override") && w.contains("Sheet")), "{:?}", p.warnings);
}

/// Exports `two_cubes` with `prefix` to `urdf` under a fresh folder, and returns that folder.
fn export_with_prefix(tag: &str, urdf: &str, prefix: &str) -> (PathBuf, std::path::PathBuf, String) {
    let mut s = two_cubes();
    let dir = temp(tag);
    let path = dir.join(urdf);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    run(&mut s, "file.export", json!({"path": path.to_string_lossy(), "mesh_prefix": prefix, "place_meshes": true}));
    let xml = std::fs::read_to_string(&path).unwrap_or_default();
    (dir, path, xml)
}

#[test]
fn a_folder_prefix_creates_the_folder_and_the_meshes_are_in_it() {
    let (dir, _, xml) = export_with_prefix("prefix-folder", "robot.urdf", "meshes/");
    assert!(xml.contains(r#"filename="meshes/robot-B.stl""#), "{xml}");
    assert!(dir.join("meshes/robot-B.stl").exists());
    assert!(!dir.join("robot-B.stl").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_nested_folder_prefix_without_a_trailing_slash_is_a_name_start_in_that_folder() {
    let (dir, _, xml) = export_with_prefix("prefix-name", "robot.urdf", "out/parts/v1_");
    assert!(xml.contains(r#"filename="out/parts/v1_robot-B.stl""#), "{xml}");
    assert!(dir.join("out/parts/v1_robot-B.stl").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_parent_folder_prefix_sits_beside_the_urdf_folder() {
    let (dir, _, xml) = export_with_prefix("prefix-parent", "urdf/robot.urdf", "../meshes/");
    assert!(xml.contains(r#"filename="../meshes/robot-B.stl""#), "{xml}");
    assert!(dir.join("meshes/robot-B.stl").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_package_prefix_writes_into_the_package_when_the_urdf_is_in_it() {
    let (dir, _, xml) = export_with_prefix("prefix-pkg", "robot/urdf/r.urdf", "package://robot/meshes/");
    assert!(xml.contains(r#"filename="package://robot/meshes/r-B.stl""#), "{xml}");
    assert!(dir.join("robot/meshes/r-B.stl").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_package_prefix_writes_its_folder_beside_a_urdf_outside_the_package() {
    let (dir, _, _) = export_with_prefix("prefix-pkg-out", "r.urdf", "package://robot/meshes/");
    assert!(dir.join("meshes/r-B.stl").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_url_prefix_writes_to_that_folder() {
    let dir = temp("prefix-file");
    let target = dir.join("abs");
    let (other, _, xml) = export_with_prefix("prefix-file-2", "r.urdf", &format!("file://{}/", target.to_string_lossy()));
    assert!(xml.contains("file://"), "{xml}");
    assert!(target.join("r-B.stl").exists());
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&other);
}

#[test]
fn a_file_url_prefix_is_decoded_like_a_url() {
    let dir = std::path::Path::new("/somewhere");
    let place = |prefix: &str| super::mesh_place(prefix, dir).map(|(f, start)| (f.to_string_lossy().to_string(), start));
    assert_eq!(place("file:///opt/my%20robot/meshes/"), Some(("/opt/my robot/meshes".into(), String::new())));
    assert_eq!(place("file:///C:/robot/meshes/v1_"), Some(("C:/robot/meshes".into(), "v1_".into())));
    assert_eq!(place("file:///opt/r%zz/"), None);
}

#[test]
fn by_default_every_mesh_stays_beside_the_urdf_as_before() {
    let mut s = two_cubes();
    let dir = temp("prefix-unplaced");
    let path = dir.join("robot.urdf");
    run(&mut s, "file.export", json!({"path": path.to_string_lossy(), "mesh_prefix": "meshes/"}));
    let xml = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(xml.contains(r#"filename="meshes/robot-B.stl""#), "{xml}");
    assert!(dir.join("robot-B.stl").exists());
    assert!(!dir.join("meshes").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn file_export_refuses_urdf_choices_of_the_wrong_type() {
    let mut s = two_cubes();
    let dir = temp("export-wrong-types");
    let path = dir.join("r.urdf").to_string_lossy().to_string();
    for (k, v) in [("mimic", json!("false")), ("place_meshes", json!(0))] {
        let r = s.execute("file.export", &json!({"path": path, k: v}));
        assert!(r.is_err(), "{k}: {v}");
    }
    assert!(!dir.join("r.urdf").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_web_prefix_keeps_the_meshes_beside_the_urdf_and_says_so() {
    let mut s = two_cubes();
    let dir = temp("prefix-web");
    let path = dir.join("r.urdf");
    let r = run(&mut s, "file.export", json!({"path": path.to_string_lossy(), "mesh_prefix": "https://example.com/m/", "place_meshes": true}));
    assert!(dir.join("r-B.stl").exists());
    assert!(r["warnings"].to_string().contains("mesh_prefix"), "{r}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Called from a script or MCP, Export URDF with Options exports as Export URDF does: only the
/// app opens its dialog.
#[test]
fn the_options_command_exports_as_export_urdf_does() {
    let mut s = fan(&["revolute", "revolute"]);
    link(&mut s, "J0", "J1", 2.0, 0.0);
    let dir = temp("export-urdf-options");
    s.param_check = crate::schema::ParamCheck::Strict;
    for (id, name) in [("file.export_urdf", "a.urdf"), ("file.export_urdf_options", "b.urdf")] {
        run(&mut s, id, json!({"path": dir.join(name).to_string_lossy(), "mimic": true}));
    }
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap_or_default().replace("b-", "a-").replace("\"b\"", "\"a\"");
    let (a, b) = (read("a.urdf"), read("b.urdf"));
    assert!(a.contains("<mimic") && !a.is_empty(), "{a}");
    assert_eq!(a, b);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn file_export_urdf_takes_the_mimic_choice() {
    let mut s = fan(&["revolute", "revolute"]);
    link(&mut s, "J0", "J1", 2.0, 0.0);
    let dir = temp("export-urdf-cmd");
    let path = dir.join("r.urdf");
    // Strict: a parameter the command's schema doesn't list is refused.
    s.param_check = crate::schema::ParamCheck::Strict;
    run(&mut s, "file.export_urdf", json!({"path": path.to_string_lossy(), "mimic": false}));
    let xml = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(!xml.contains("<mimic"), "{xml}");
    let _ = std::fs::remove_dir_all(&dir);
}

// Robot exports read back as any URDF reader reads them. Each design is exported with
// `file.export` as the desktop dialog does it (motion links as `<mimic>`), the written file is
// read with a general XML reader, and the expected values are worked out by hand, never taken
// from SolidCraft's output. What a simulator does with the file is checked through the numbers it
// reads: the pendulum's period from the written inertia, a free slide from the joint's axis and
// dynamics, and where a dropped ball first touches each collision shape.

/// Lengths in m: the URDF's own units.
pub(super) type M4 = [[f64; 4]; 4];

const I4: M4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

pub(super) fn m4_mul(a: &M4, b: &M4) -> M4 {
    let mut out = [[0.0; 4]; 4];
    for (r, row) in out.iter_mut().enumerate() {
        for (c, v) in row.iter_mut().enumerate() {
            *v = (0..4).map(|k| a[r][k] * b[k][c]).sum();
        }
    }
    out
}

pub(super) fn m4_apply(m: &M4, p: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|r| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + m[r][3])
}

fn m4_turn(m: &M4, v: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|r| m[r][0] * v[0] + m[r][1] * v[1] + m[r][2] * v[2])
}

pub(super) fn m4_shift(v: [f64; 3]) -> M4 {
    let mut m = I4;
    for r in 0..3 {
        m[r][3] = v[r];
    }
    m
}

/// Rotation by `angle` about the unit `axis` (Rodrigues).
pub(super) fn m4_rot(axis: [f64; 3], angle: f64) -> M4 {
    let [x, y, z] = axis;
    let (s, c) = angle.sin_cos();
    let t = 1.0 - c;
    [
        [t * x * x + c, t * x * y - s * z, t * x * z + s * y, 0.0],
        [t * x * y + s * z, t * y * y + c, t * y * z - s * x, 0.0],
        [t * x * z - s * y, t * y * z + s * x, t * z * z + c, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

/// An `<origin xyz rpy>`: fixed-axis roll, pitch, yaw, as the URDF spec reads it.
#[derive(Clone, Copy, Default, Debug)]
struct XOrigin {
    xyz: [f64; 3],
    rpy: [f64; 3],
}

impl XOrigin {
    fn frame(&self) -> M4 {
        let r = m4_mul(&m4_mul(&m4_rot([0.0, 0.0, 1.0], self.rpy[2]), &m4_rot([0.0, 1.0, 0.0], self.rpy[1])), &m4_rot([1.0, 0.0, 0.0], self.rpy[0]));
        m4_mul(&m4_shift(self.xyz), &r)
    }
}

#[derive(Debug)]
enum XGeom {
    Box([f64; 3]),
    Cylinder { radius: f64, length: f64 },
    Sphere(f64),
    Mesh { file: String, scale: [f64; 3] },
}

#[derive(Default, Debug)]
struct XLink {
    name: String,
    /// The inertial's origin, mass and `ixx ixy ixz iyy iyz izz`.
    inertial: Option<(XOrigin, f64, [f64; 6])>,
    collision: Vec<(XOrigin, Option<XGeom>)>,
}

#[derive(Default, Debug)]
struct XJoint {
    name: String,
    kind: String,
    parent: String,
    child: String,
    origin: XOrigin,
    axis: Option<[f64; 3]>,
    limit: std::collections::BTreeMap<String, f64>,
    dynamics: std::collections::BTreeMap<String, f64>,
    /// The joint it follows, multiplier and offset.
    mimic: Option<(String, f64, f64)>,
}

#[derive(Default, Debug)]
struct XRobot {
    links: Vec<XLink>,
    joints: Vec<XJoint>,
}

fn numbers(text: &str) -> Vec<f64> {
    text.split_whitespace().map(|t| t.parse::<f64>().unwrap_or_else(|e| panic!("{t:?} is not a number: {e}"))).collect()
}

fn three(text: &str) -> [f64; 3] {
    let v = numbers(text);
    assert_eq!(v.len(), 3, "{text:?}: three numbers");
    [v[0], v[1], v[2]]
}

/// Reads a URDF with quick-xml, as any URDF reader would: links, joints and what a simulator uses.
fn read_robot(xml: &str) -> XRobot {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut robot = XRobot::default();
    let mut path: Vec<String> = Vec::new();
    loop {
        let (e, empty) = match reader.read_event() {
            Ok(Event::Start(e)) => (e, false),
            Ok(Event::Empty(e)) => (e, true),
            Ok(Event::End(_)) => {
                path.pop();
                continue;
            }
            Ok(Event::Eof) => break,
            Ok(_) => continue,
            Err(e) => panic!("the URDF is not XML: {e}"),
        };
        let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
        let a: std::collections::BTreeMap<String, String> = e
            .attributes()
            .map(|a| {
                let a = a.unwrap_or_else(|e| panic!("<{name}>: {e}"));
                (String::from_utf8_lossy(a.key.as_ref()).to_string(), String::from_utf8_lossy(&a.value).to_string())
            })
            .collect();
        let get = |k: &str| a.get(k).cloned().unwrap_or_else(|| panic!("<{name}> has no {k}"));
        let origin = || XOrigin { xyz: a.get("xyz").map_or([0.0; 3], |t| three(t)), rpy: a.get("rpy").map_or([0.0; 3], |t| three(t)) };
        let inside = |what: &str| path.iter().any(|p| p == what);
        match (path.first().map(String::as_str), name.as_str()) {
            (Some("robot"), "link") if path.len() == 1 => robot.links.push(XLink { name: get("name"), ..Default::default() }),
            (Some("robot"), "joint") if path.len() == 1 => robot.joints.push(XJoint { name: get("name"), kind: get("type"), ..Default::default() }),
            _ => {}
        }
        let in_link = path.len() >= 2 && path[1] == "link";
        let in_joint = path.len() >= 2 && path[1] == "joint";
        if in_link && let Some(link) = robot.links.last_mut() {
            match name.as_str() {
                "inertial" => link.inertial = Some((XOrigin::default(), 0.0, [0.0; 6])),
                "origin" if inside("inertial") => {
                    if let Some(i) = link.inertial.as_mut() {
                        i.0 = origin();
                    }
                }
                "mass" if inside("inertial") => {
                    if let Some(i) = link.inertial.as_mut() {
                        i.1 = numbers(&get("value"))[0];
                    }
                }
                "inertia" if inside("inertial") => {
                    if let Some(i) = link.inertial.as_mut() {
                        i.2 = ["ixx", "ixy", "ixz", "iyy", "iyz", "izz"].map(|k| numbers(&get(k))[0]);
                    }
                }
                "collision" => link.collision.push((XOrigin::default(), None)),
                "origin" if inside("collision") => {
                    if let Some(c) = link.collision.last_mut() {
                        c.0 = origin();
                    }
                }
                "box" | "cylinder" | "sphere" | "mesh" if inside("collision") => {
                    let g = match name.as_str() {
                        "box" => XGeom::Box(three(&get("size"))),
                        "cylinder" => XGeom::Cylinder { radius: numbers(&get("radius"))[0], length: numbers(&get("length"))[0] },
                        "sphere" => XGeom::Sphere(numbers(&get("radius"))[0]),
                        _ => XGeom::Mesh { file: get("filename"), scale: a.get("scale").map_or([1.0; 3], |t| three(t)) },
                    };
                    if let Some(c) = link.collision.last_mut() {
                        c.1 = Some(g);
                    }
                }
                _ => {}
            }
        }
        if in_joint && let Some(j) = robot.joints.last_mut() {
            match name.as_str() {
                "origin" => j.origin = origin(),
                "parent" => j.parent = get("link"),
                "child" => j.child = get("link"),
                "axis" => j.axis = Some(three(&get("xyz"))),
                "limit" | "dynamics" => {
                    let into = if name == "limit" { &mut j.limit } else { &mut j.dynamics };
                    for (k, v) in &a {
                        into.insert(k.clone(), numbers(v)[0]);
                    }
                }
                "mimic" => {
                    let n = |k: &str, d: f64| a.get(k).map_or(d, |t| numbers(t)[0]);
                    j.mimic = Some((get("joint"), n("multiplier", 1.0), n("offset", 0.0)));
                }
                _ => {}
            }
        }
        if !empty {
            path.push(name);
        }
    }
    robot
}

impl XRobot {
    fn link(&self, name: &str) -> &XLink {
        self.links
            .iter()
            .find(|l| l.name == name)
            .unwrap_or_else(|| panic!("no link {name}: {:?}", self.links.iter().map(|l| &l.name).collect::<Vec<_>>()))
    }

    fn joint(&self, name: &str) -> &XJoint {
        self.joints
            .iter()
            .find(|j| j.name == name)
            .unwrap_or_else(|| panic!("no joint {name}: {:?}", self.joints.iter().map(|j| &j.name).collect::<Vec<_>>()))
    }

    /// What urdfdom's `check_urdf` refuses: a joint naming a missing link, a link with two
    /// parents, a tree with other than one root, an unknown joint type, or a moving joint without
    /// a usable axis or limits.
    fn assert_valid(&self) {
        let names: std::collections::BTreeSet<&str> = self.links.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names.len(), self.links.len(), "link names repeat");
        let mut parents = std::collections::BTreeMap::new();
        for j in &self.joints {
            assert!(names.contains(j.parent.as_str()) && names.contains(j.child.as_str()), "{}: {} -> {}", j.name, j.parent, j.child);
            assert!(parents.insert(j.child.as_str(), j.name.as_str()).is_none(), "{} has two parents", j.child);
            assert!(["revolute", "continuous", "prismatic", "fixed", "floating", "planar"].contains(&j.kind.as_str()), "{}: type {}", j.name, j.kind);
            if j.kind != "fixed" {
                let axis = j.axis.unwrap_or([1.0, 0.0, 0.0]);
                assert!((norm(axis) - 1.0).abs() < 1e-9, "{}: axis {axis:?}", j.name);
            }
            if j.kind == "revolute" || j.kind == "prismatic" {
                for k in ["effort", "velocity"] {
                    assert!(j.limit.contains_key(k), "{}: <limit> has no {k}", j.name);
                }
            }
        }
        let roots: Vec<&str> = names.iter().copied().filter(|n| !parents.contains_key(n)).collect();
        assert_eq!(roots, ["base_link"], "one root");
    }

    /// The value each joint takes when the given joints take theirs, the rest 0, followers
    /// following their `<mimic>`.
    fn values(&self, q: &[(&str, f64)]) -> std::collections::BTreeMap<String, f64> {
        let given = |n: &str| q.iter().find(|(k, _)| *k == n).map_or(0.0, |(_, v)| *v);
        self.joints
            .iter()
            .map(|j| {
                let v = match &j.mimic {
                    Some((driver, m, o)) => m * given(driver) + o,
                    None => given(&j.name),
                };
                (j.name.clone(), v)
            })
            .collect()
    }

    /// The link's frame in base_link's at those joint values.
    fn frame(&self, link: &str, q: &std::collections::BTreeMap<String, f64>) -> M4 {
        let Some(j) = self.joints.iter().find(|j| j.child == link) else { return I4 };
        m4_mul(&self.joint_frame(j, q), &self.motion(j, q.get(&j.name).copied().unwrap_or(0.0)))
    }

    /// A joint's frame (its origin in its parent) in base_link's.
    fn joint_frame(&self, j: &XJoint, q: &std::collections::BTreeMap<String, f64>) -> M4 {
        m4_mul(&self.frame(&j.parent, q), &j.origin.frame())
    }

    fn motion(&self, j: &XJoint, v: f64) -> M4 {
        let axis = j.axis.unwrap_or([1.0, 0.0, 0.0]);
        match j.kind.as_str() {
            "revolute" | "continuous" => m4_rot(axis, v),
            "prismatic" => m4_shift(axis.map(|x| x * v)),
            _ => I4,
        }
    }

    /// The links a joint moves: its child and everything below it.
    fn moved_by(&self, joint: &str) -> Vec<&XLink> {
        let mut out = vec![self.joint(joint).child.as_str()];
        let mut k = 0;
        while k < out.len() {
            let below: Vec<&str> = self.joints.iter().filter(|j| j.parent == out[k]).map(|j| j.child.as_str()).collect();
            out.extend(below);
            k += 1;
        }
        out.into_iter().map(|n| self.link(n)).collect()
    }
}

/// Builds the design from its commands and exports it as the desktop dialog does.
fn checker_case(tag: &str, commands: &[(&str, Value)]) -> (XRobot, PathBuf) {
    let mut s = Session::default();
    for (id, p) in commands {
        run(&mut s, id, p.clone());
    }
    let dir = temp(&format!("checker-{tag}"));
    let path = dir.join(format!("{tag}.urdf"));
    run(&mut s, "file.export", json!({"path": path.to_string_lossy(), "mimic": true}));
    let xml = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{tag}: {e}"));
    let robot = read_robot(&xml);
    robot.assert_valid();
    (robot, dir)
}

/// 10 mm cubes in components `A` (grounded, at the origin) and `B` (at x = 20), with `joint`
/// from B's bottom face to A's top face.
fn cubes_with(kind: &str, name: &str) -> Vec<(&'static str, Value)> {
    vec![
        ("component.activate", json!({"component": "root"})),
        ("component.create", json!({"name": "A"})),
        ("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0]})),
        ("component.activate", json!({"component": "root"})),
        ("component.create", json!({"name": "B"})),
        ("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0]})),
        ("occurrence.ground", json!({"occurrence": "A:1", "grounded": true})),
        ("joint.create", json!({"type": kind, "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "name": name})),
    ]
}

/// The angle a rotation turns by.
fn turn_of(r: &M4) -> f64 {
    ((r[0][0] + r[1][1] + r[2][2] - 1.0) / 2.0).clamp(-1.0, 1.0).acos()
}

/// The rotation from the link's rest pose to its pose at `q`.
fn relative_turn(robot: &XRobot, link: &str, q: &[(&str, f64)]) -> M4 {
    let rest = robot.frame(link, &robot.values(&[]));
    let at = robot.frame(link, &robot.values(q));
    let mut rel = [[0.0; 4]; 4];
    for (r, row) in rel.iter_mut().enumerate().take(3) {
        for (c, v) in row.iter_mut().enumerate().take(3) {
            *v = (0..3).map(|k| at[r][k] * rest[c][k]).sum();
        }
    }
    rel[3][3] = 1.0;
    rel
}

fn origin_at(robot: &XRobot, link: &str, q: &[(&str, f64)]) -> [f64; 3] {
    m4_apply(&robot.frame(link, &robot.values(q)), [0.0; 3])
}

/// At `q`, the link's origin is at `xyz` (m) and it has turned by `turn` about `about`.
fn assert_turned(robot: &XRobot, link: &str, q: &[(&str, f64)], xyz: [f64; 3], turn: f64, about: Option<[f64; 3]>) {
    let at = origin_at(robot, link, q);
    assert!(norm(sub(at, xyz)) < 1e-6, "{link} at {q:?}: origin {at:?}, expected {xyz:?}");
    let rel = relative_turn(robot, link, q);
    assert!((turn_of(&rel) - turn).abs() < 1e-6, "{link} at {q:?}: turned {}, expected {turn}", turn_of(&rel));
    if let Some(axis) = about {
        assert!(norm(sub(m4_turn(&rel, axis), axis)) < 1e-6, "{link} at {q:?}: did not turn about {axis:?}");
    }
}

#[test]
fn checker_hinge_turns_b_about_the_hinge_axis() {
    let (robot, dir) = checker_case("hinge", &cubes_with("revolute", "Hinge"));
    // The hinge's axis is the joint face's normal, z, through the top face's centre (5, 5, 10) mm.
    assert_turned(&robot, "B", &[("Hinge", 0.5)], [0.005, 0.005, 0.01], 0.5, Some([0.0, 0.0, 1.0]));
    assert_turned(&robot, "B", &[("Hinge", -1.2)], [0.005, 0.005, 0.01], 1.2, Some([0.0, 0.0, 1.0]));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn checker_hinge_dynamics_are_what_a_reader_gets() {
    let mut cmds = cubes_with("revolute", "Hinge");
    cmds.push(("joint.dynamics", json!({"joint": "Hinge", "effort": 12.5, "velocity": 2.0, "damping": 0.1, "friction": 0.05})));
    let (robot, dir) = checker_case("hinge_dynamics", &cmds);
    let j = robot.joint("Hinge");
    for (map, key, want) in [(&j.limit, "effort", 12.5), (&j.limit, "velocity", 2.0), (&j.dynamics, "damping", 0.1), (&j.dynamics, "friction", 0.05)]
    {
        let got = map.get(key).copied();
        assert!(got.is_some_and(|g| (g - want).abs() < 1e-6), "Hinge {key}: {got:?}, expected {want}");
    }
    assert_turned(&robot, "B", &[("Hinge", 0.5)], [0.005, 0.005, 0.01], 0.5, Some([0.0, 0.0, 1.0]));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn checker_mass_override_is_the_links_mass() {
    let mut cmds = cubes_with("revolute", "Hinge");
    cmds.push(("component.physical_override", json!({"component": "B", "mass": 2.5})));
    let (robot, dir) = checker_case("mass_override", &cmds);
    let mass = robot.link("B").inertial.map(|i| i.1);
    assert!(mass.is_some_and(|m| (m - 2.5).abs() < 1e-6), "B's mass: {mass:?}");
    assert_turned(&robot, "B", &[("Hinge", 0.5)], [0.005, 0.005, 0.01], 0.5, Some([0.0, 0.0, 1.0]));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn checker_tool_frame_turns_with_its_part() {
    let mut cmds = cubes_with("revolute", "Hinge");
    cmds.push(("joint.origin", json!({"name": "tool0", "occurrence": "B:1", "face": [5, 5, 20], "export_frame": true})));
    let (robot, dir) = checker_case("tool_frame", &cmds);
    // B's top face centre, 10 mm above the hinge on its axis: it stays put and turns.
    assert_turned(&robot, "tool0", &[("Hinge", 0.6)], [0.005, 0.005, 0.02], 0.6, Some([0.0, 0.0, 1.0]));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Cubes `H` (grounded), `A` and `B` (at x = 0, 20, 40), A and B each on a revolute or slider
/// from H's top face, and the motion link `link` between them.
fn driven_pair(b_kind: &str, link: (&'static str, Value)) -> Vec<(&'static str, Value)> {
    let mut cmds = Vec::new();
    for (name, x) in [("H", 0), ("A", 20), ("B", 40)] {
        cmds.push(("component.activate", json!({"component": "root"})));
        cmds.push(("component.create", json!({"name": name})));
        cmds.push(("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]})));
    }
    cmds.push(("occurrence.ground", json!({"occurrence": "H:1", "grounded": true})));
    cmds.push((
        "joint.create",
        json!({"type": "revolute", "a": {"occurrence": "H:1", "face": [5, 5, 10]}, "b": {"occurrence": "A:1", "face": [25, 5, 0]}, "name": "JA"}),
    ));
    cmds.push((
        "joint.create",
        json!({"type": b_kind, "a": {"occurrence": "H:1", "face": [5, 5, 10]}, "b": {"occurrence": "B:1", "face": [45, 5, 0]}, "name": "JB"}),
    ));
    cmds.push(link);
    cmds
}

#[test]
fn checker_gear_pair_turns_b_half_as_far() {
    let (robot, dir) = checker_case("gear_pair", &driven_pair("revolute", ("joint.gear", json!({"a": "JA", "b": "JB", "ra": 10, "rb": 20}))));
    // Radii 10 and 20: B turns 0.4 × 10 / 20 = 0.2 while A turns 0.4, both on H's top face.
    assert_turned(&robot, "A", &[("JA", 0.4)], [0.005, 0.005, 0.01], 0.4, Some([0.0, 0.0, 1.0]));
    assert_turned(&robot, "B", &[("JA", 0.4)], [0.005, 0.005, 0.01], 0.2, Some([0.0, 0.0, 1.0]));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn checker_rack_and_pinion_slides_b_by_the_arc() {
    let (robot, dir) = checker_case("rack_pinion", &driven_pair("slider", ("joint.rack_pinion", json!({"a": "JA", "b": "JB", "radius": 5}))));
    assert_turned(&robot, "A", &[("JA", 0.4)], [0.005, 0.005, 0.01], 0.4, Some([0.0, 0.0, 1.0]));
    // Radius 5 mm: 0.4 rad of A moves B 5 × 0.4 = 2 mm along the slider.
    let moved = norm(sub(origin_at(&robot, "B", &[("JA", 0.4)]), [0.005, 0.005, 0.01]));
    assert!((moved - 0.002).abs() < 1e-6, "B moved {moved} m, expected 0.002");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn checker_ball_joint_chain_turns_and_swings_as_the_cube_should() {
    let (robot, dir) = checker_case("ball_chain", &cubes_with("ball", "Ball"));
    assert_turned(&robot, "B", &[("Ball_dof1", 0.4)], [0.005, 0.005, 0.01], 0.4, Some([0.0, 0.0, 1.0]));
    assert_turned(&robot, "B", &[("Ball_dof2", 0.3)], [0.005, 0.005, 0.01], 0.3, None);
    // Hung from the joint at the top face's centre (gravity +z), B swings about Ball_dof2's axis
    // as a physical pendulum: the 10 mm cube's centre is d = 5 mm from the pivot, and about the
    // pivot I = m (a² / 6 + d²), so T = 2π √((a² / 6 + d²) / (g d)) = 0.1831 s for any mass.
    // A simulator reads exactly this from the file: each moved link's mass, centre and inertia
    // (the chain's in-between links weigh 1e-6 kg), about the joint's axis.
    let q = robot.values(&[]);
    let j = robot.joint("Ball_dof2");
    let jf = robot.joint_frame(j, &q);
    let pivot = m4_apply(&jf, [0.0; 3]);
    let axis = m4_turn(&jf, j.axis.unwrap_or([1.0, 0.0, 0.0]));
    let gravity_arm = |r: [f64; 3]| {
        let along = dot(r, axis);
        sub(r, axis.map(|x| x * along))
    };
    let (mut inertia, mut moment) = (0.0, [0.0; 3]);
    for link in robot.moved_by(&j.name) {
        let Some((origin, mass, i)) = link.inertial else { continue };
        let frame = m4_mul(&robot.frame(&link.name, &q), &origin.frame());
        let n = (0..3).map(|c| (0..3).map(|r| frame[r][c] * axis[r]).sum::<f64>()).collect::<Vec<_>>();
        let [ixx, ixy, ixz, iyy, iyz, izz] = i;
        let own = n[0] * n[0] * ixx + n[1] * n[1] * iyy + n[2] * n[2] * izz + 2.0 * (n[0] * n[1] * ixy + n[0] * n[2] * ixz + n[1] * n[2] * iyz);
        let arm = gravity_arm(sub(m4_apply(&frame, [0.0; 3]), pivot));
        inertia += own + mass * dot(arm, arm);
        moment = std::array::from_fn(|k| moment[k] + mass * arm[k]);
    }
    let period = 2.0 * std::f64::consts::PI * (inertia / (9.81 * norm(moment))).sqrt();
    assert!((period - 0.1831).abs() < 0.03 * 0.1831, "period {period:.4} s, expected 0.1831 s");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn checker_planar_joint_slides_and_turns_in_its_plane() {
    let (robot, dir) = checker_case("planar_chain", &cubes_with("planar", "Plane"));
    for (joint, d) in [("Plane_dof1", 0.03), ("Plane_dof2", 0.02)] {
        let at = origin_at(&robot, "B", &[(joint, d)]);
        let moved = norm(sub(at, [0.005, 0.005, 0.01]));
        assert!((moved - d).abs() < 1e-6 && (at[2] - 0.01).abs() < 1e-6, "B at {joint} = {d}: {at:?}");
    }
    assert_turned(&robot, "B", &[("Plane", 0.7)], [0.005, 0.005, 0.01], 0.7, Some([0.0, 0.0, 1.0]));
    // Sliding freely under gravity along Plane_dof1 for 0.1 s covers ½ g t² = 0.04905 m: nothing
    // in the file holds it back (no damping or friction, a range that reaches that far) and it
    // carries B's mass.
    let j = robot.joint("Plane_dof1");
    assert_eq!(j.kind, "prismatic");
    for k in ["damping", "friction"] {
        assert!(j.dynamics.get(k).is_none_or(|v| *v == 0.0), "Plane_dof1 {k}: {:?}", j.dynamics);
    }
    let reach: f64 = 0.5 * 9.81 * 0.1 * 0.1;
    assert!((reach - 0.04905).abs() < 1e-9);
    if let Some(upper) = j.limit.get("upper") {
        assert!(*upper >= reach, "Plane_dof1 stops at {upper} m");
    }
    let carried: f64 = robot.moved_by("Plane_dof1").iter().filter_map(|l| l.inertial.map(|i| i.1)).sum();
    assert!(carried > 1e-3, "Plane_dof1 carries {carried} kg");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Where a ball of radius `r` dropped straight down at `xy` first touches a collision shape,
/// as the height of its centre (`None`: it falls past them all).
fn drop_height(robot: &XRobot, dir: &std::path::Path, xy: [f64; 2], r: f64) -> Option<f64> {
    let q = robot.values(&[]);
    let mut best: Option<f64> = None;
    let mut keep = |z: f64| best = Some(best.map_or(z, |b: f64| b.max(z)));
    for link in &robot.links {
        for (origin, geom) in &link.collision {
            let frame = m4_mul(&robot.frame(&link.name, &q), &origin.frame());
            let centre = m4_apply(&frame, [0.0; 3]);
            let mut tris: Vec<[[f64; 3]; 3]> = Vec::new();
            match geom {
                Some(XGeom::Sphere(radius)) => {
                    let h = norm(sub([xy[0], xy[1], 0.0], [centre[0], centre[1], 0.0]));
                    if h <= radius + r {
                        keep(centre[2] + ((radius + r).powi(2) - h * h).sqrt());
                    }
                }
                Some(XGeom::Box(size)) => {
                    let h = size.map(|s| s / 2.0);
                    let c = |i: usize| {
                        [if i & 1 == 0 { -h[0] } else { h[0] }, if i & 2 == 0 { -h[1] } else { h[1] }, if i & 4 == 0 { -h[2] } else { h[2] }]
                    };
                    for [a, b, cc, d] in [[0, 1, 3, 2], [4, 5, 7, 6], [0, 1, 5, 4], [2, 3, 7, 6], [0, 2, 6, 4], [1, 3, 7, 5]] {
                        tris.push([c(a), c(b), c(cc)].map(|p| m4_apply(&frame, p)));
                        tris.push([c(a), c(cc), c(d)].map(|p| m4_apply(&frame, p)));
                    }
                }
                Some(XGeom::Cylinder { radius, length }) => {
                    // Its axis is the shape frame's z: the caps as fans of 64 triangles.
                    for z in [-length / 2.0, length / 2.0] {
                        for k in 0..64 {
                            let p = |k: usize| {
                                let t = k as f64 * std::f64::consts::TAU / 64.0;
                                [radius * t.cos(), radius * t.sin(), z]
                            };
                            tris.push([[0.0, 0.0, z], p(k), p(k + 1)].map(|p| m4_apply(&frame, p)));
                        }
                    }
                }
                Some(XGeom::Mesh { file, scale }) => {
                    let bytes = std::fs::read(dir.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
                    for t in solidcraft_io::read_stl(&bytes).unwrap_or_else(|e| panic!("{file}: {e}")) {
                        tris.push(t.map(|p| m4_apply(&frame, std::array::from_fn(|k| p[k] as f64 * scale[k]))));
                    }
                }
                None => panic!("{}: a <collision> with no geometry", link.name),
            }
            // The ball comes down on a face's top: under its centre, the highest triangle.
            for [a, b, c] in tris {
                let area = |p: [f64; 3], q: [f64; 3], s: [f64; 2]| (q[0] - p[0]) * (s[1] - p[1]) - (q[1] - p[1]) * (s[0] - p[0]);
                let (wa, wb, wc) = (area(b, c, xy), area(c, a, xy), area(a, b, xy));
                let whole = wa + wb + wc;
                if whole.abs() < 1e-15 || !(wa / whole >= -1e-9 && wb / whole >= -1e-9 && wc / whole >= -1e-9) {
                    continue;
                }
                keep((wa * a[2] + wb * b[2] + wc * c[2]) / whole + r);
            }
        }
    }
    best
}

#[test]
fn checker_a_dropped_ball_lands_on_each_collision_shape() {
    let mut cmds = Vec::new();
    for (k, shape) in ["mesh", "box", "cylinder", "sphere", "none"].into_iter().enumerate() {
        let (component, body) = (format!("S{k}"), format!("B{k}"));
        cmds.push(("component.activate", json!({"component": "root"})));
        cmds.push(("component.create", json!({"name": component})));
        cmds.push(("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [30 * k, 0, 0], "body_name": body})));
        cmds.push(("body.collision", json!({"body": body, "shape": shape})));
        cmds.push(("occurrence.ground", json!({"occurrence": format!("{component}:1"), "grounded": true})));
    }
    let (robot, dir) = checker_case("collision_shapes", &cmds);
    // A 5 mm ball over each 10 mm cube's centre comes to rest on its top at 10 + 5 mm: the mesh,
    // the box and the cylinder (along z, radius 5) all end at the top face, and the sphere, radius
    // 5 about the cube's centre at 5, too; through `none` it falls.
    for (k, want) in [Some(0.015), Some(0.015), Some(0.015), Some(0.015), None].into_iter().enumerate() {
        let got = drop_height(&robot, &dir, [0.005 + 0.03 * k as f64, 0.005], 0.005);
        match (got, want) {
            (Some(g), Some(w)) => assert!((g - w).abs() < 1e-3, "shape {k}: the ball lands at {g:.4} m, expected {w:.4} m"),
            (None, None) => {}
            _ => panic!("shape {k}: lands at {got:?}, expected {want:?}"),
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
