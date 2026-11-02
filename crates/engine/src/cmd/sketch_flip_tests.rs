//! `sketch.create` `flip`: a sketch on a plane seen from its other side (#1586). Without it a
//! sketch keeps the plane's own side, and older designs open, rebuild and save as before.

use serde_json::{Value, json};
use solidcraft_doc::{FeatureKind, PlaneRef};
use solidcraft_geom::{Plane, Vec2, Vec3};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn sketch_plane(s: &Session, id: u64) -> Plane {
    s.model.state().sketch(id).map(|ss| ss.plane).expect("an evaluated sketch")
}

fn measured(s: &mut Session) -> Value {
    run(s, "inspect.measure", json!({}))["bodies"].clone()
}

fn close(a: Vec3, b: Vec3) -> bool {
    a.dist(b) < 1e-9
}

/// The origin planes' sketch axes, as `sketch.create` documents them: sketch x and y run along world +X, +Y on XY;
/// +X, -Z on XZ; +Y, +Z on YZ.
#[test]
fn origin_planes_put_the_sketch_axes_where_sketch_create_says() {
    let (x, y, z) = (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0));
    for (name, u, v) in [("XY", x, y), ("XZ", x, z * -1.0), ("YZ", y, z)] {
        let mut s = Session::default();
        let id = run(&mut s, "sketch.create", json!({"plane": name}))["sketch"].as_u64().unwrap();
        let p = sketch_plane(&s, id);
        let (su, sv) = (p.to_world(Vec2::new(1.0, 0.0)), p.to_world(Vec2::new(0.0, 1.0)));
        assert!(close(su, u) && close(sv, v), "{name}: sketch x lands on {su:?}, sketch y on {sv:?}");
    }
}

#[test]
fn without_flip_a_sketch_keeps_the_planes_own_side_and_saves_as_before() {
    for name in ["XY", "XZ", "YZ"] {
        let mut s = Session::default();
        let id = run(&mut s, "sketch.create", json!({"plane": name}))["sketch"].as_u64().unwrap();
        let saved = serde_json::to_value(&s.doc.feature(id).unwrap().kind).unwrap();
        assert_eq!(saved["plane"], json!({"type": "origin", "name": name}), "{name}");
        assert_eq!(sketch_plane(&s, id), Plane::named(name).unwrap(), "{name}");
    }
}

#[test]
fn flip_turns_the_plane_about_the_axis_asked_for() {
    // (plane, flip, expected x, expected y)
    let cases = [
        ("XZ", json!(true), Vec3::X, Vec3::Z),
        ("XZ", json!("y"), -Vec3::X, -Vec3::Z),
        ("YZ", json!(true), -Vec3::Y, Vec3::Z),
        ("YZ", json!("x"), Vec3::Y, -Vec3::Z),
        ("XY", json!(true), Vec3::X, -Vec3::Y),
        ("XY", json!("Y"), -Vec3::X, Vec3::Y),
    ];
    for (name, flip, x, y) in cases {
        let mut s = Session::default();
        let id = run(&mut s, "sketch.create", json!({"plane": name, "flip": flip}))["sketch"].as_u64().unwrap();
        let p = sketch_plane(&s, id);
        assert!(close(p.x, x) && close(p.y, y), "{name} flip {flip}: {p:?}");
        assert!(close(p.normal(), -Plane::named(name).unwrap().normal()), "{name} flip {flip}");
        assert!(matches!(&s.doc.feature(id).unwrap().kind, FeatureKind::Sketch { plane: PlaneRef::Flipped { .. }, .. }));
    }
    let mut s = Session::default();
    let id = run(&mut s, "sketch.create", json!({"plane": "XZ", "flip": false}))["sketch"].as_u64().unwrap();
    assert!(matches!(&s.doc.feature(id).unwrap().kind, FeatureKind::Sketch { plane: PlaneRef::Origin { .. }, .. }));
    assert!(s.execute("sketch.create", &json!({"plane": "XZ", "flip": "z"})).is_err());
}

#[test]
fn a_flipped_sketch_extrudes_the_other_way_and_offsets_along_the_planes_own_normal() {
    let mut s = Session::default();
    let id = run(&mut s, "sketch.create", json!({"plane": "XZ", "flip": true, "offset": "5 mm"}))["sketch"].as_u64().unwrap();
    assert!(close(sketch_plane(&s, id).origin, Vec3::new(0.0, 5.0, 0.0)));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 4]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": id, "distance": 3, "operation": "new_body", "body_name": "B"}));
    let bounds = s.model.state().body("B").map(|b| b.mesh().bounds()).unwrap();
    // Sketch y is world +Z; extruded along -Y, from y = 5 to y = 2.
    assert!((bounds.min.y - 2.0).abs() < 1e-9 && (bounds.max.y - 5.0).abs() < 1e-9, "{bounds:?}");
    assert!((bounds.min.z).abs() < 1e-9 && (bounds.max.z - 4.0).abs() < 1e-9, "{bounds:?}");
    // Saved and opened again, it is the same.
    let path = std::env::temp_dir().join(format!("solidcraft-flip-{}.solidcraft", std::process::id()));
    run(&mut s, "file.save", json!({"path": path}));
    let mut reopened = Session::default();
    run(&mut reopened, "doc.open", json!({"path": path}));
    assert_eq!(*reopened.doc, *s.doc);
    assert_eq!(measured(&mut reopened), measured(&mut s));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn a_construction_plane_flips_too_and_follows_its_parameter() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "h", "expression": "7 mm"}));
    run(&mut s, "construct.plane.offset", json!({"base": "XY", "offset": "h", "name": "P"}));
    let id = run(&mut s, "sketch.create", json!({"plane": "P", "flip": true}))["sketch"].as_u64().unwrap();
    let p = sketch_plane(&s, id);
    assert!(close(p.origin, Vec3::new(0.0, 0.0, 7.0)) && close(p.normal(), -Vec3::Z), "{p:?}");
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "parameters.change", json!({"name": "h", "expression": "9 mm"}));
    let p = sketch_plane(&s, id);
    assert!(close(p.origin, Vec3::new(0.0, 0.0, 9.0)) && close(p.normal(), -Vec3::Z), "{p:?}");
}

#[test]
fn flip_is_refused_on_a_face() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    assert!(s.execute("sketch.create", &json!({"plane": {"face": [5, 5, 10]}, "flip": true})).is_err());
}

/// Designs from before `flip` open with every sketch where it was, never flipped, rebuild the
/// same bodies and save and reopen unchanged.
#[test]
fn older_designs_open_rebuild_and_save_unchanged() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/solidcraft-cli/tests/corpus");
    let mut files: Vec<_> =
        std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "solidcraft")).collect();
    files.sort();
    assert!(files.len() >= 7, "corpus designs in {}", dir.display());
    for old in files {
        let mut s = Session::default();
        run(&mut s, "doc.open", json!({"path": old}));
        assert!(!serde_json::to_string(&*s.doc).unwrap().contains("\"flipped\""), "{}", old.display());
        // The oldest one's XZ sketch: sketch y along +Z, as it was drawn.
        if old.ends_with("m0-revolve-primitives.solidcraft") {
            let id = s.doc.features.iter().find(|f| matches!(f.kind, FeatureKind::Sketch { .. })).map(|f| f.id).unwrap();
            let p = sketch_plane(&s, id);
            assert!(close(p.origin, Vec3::ZERO) && close(p.x, Vec3::X) && close(p.y, Vec3::Z), "{p:?}");
        }
        let bodies = measured(&mut s);
        let path = std::env::temp_dir().join(format!("solidcraft-old-{}.solidcraft", std::process::id()));
        run(&mut s, "file.save", json!({"path": path}));
        let mut reopened = Session::default();
        run(&mut reopened, "doc.open", json!({"path": path}));
        assert_eq!(*reopened.doc, *s.doc, "{}", old.display());
        assert_eq!(measured(&mut reopened), bodies, "{}", old.display());
        std::fs::remove_file(path).unwrap();
    }
}
