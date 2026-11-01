//! A sketch on an end face of a loft whose side edges are rounded (#8284). The rounds' fitted edges
//! end a few 1e-5 mm short of the face's other edges, and where a round dies away at a corner the
//! mesh keeps an edge from a point to itself; the face's outline still closes into one profile.

use solidcraft_geom::Vec3;

use super::*;

/// A 40 mm square at z = 0 lofted to a circle of radius 25 at z = 40, its four side edges rounded.
fn rounded_loft(s: &mut Session) {
    run(s, "sketch.create", json!({"plane": "XY", "name": "Square"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [-20, -20], "p1": [20, 20]}));
    run(s, "sketch.finish", json!({}));
    run(s, "sketch.create", json!({"plane": "XY", "offset": 40, "name": "Circle"}));
    run(s, "sketch.circle.center", json!({"center": [0, 0], "radius": 25}));
    run(s, "sketch.finish", json!({}));
    let section = |name: &str| json!({"sketch": name, "profiles": [{"point": [0, 0]}]});
    run(s, "solid.loft", json!({"sections": [section("Square"), section("Circle")], "operation": "new", "body_name": "Loft"}));
    // Each side edge runs from a corner of the square to the circle; pick it halfway up.
    let mid = (20.0 + 25.0 * std::f64::consts::FRAC_1_SQRT_2) / 2.0;
    let edges: Vec<Value> = [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)].iter().map(|(x, y)| json!([x * mid, y * mid, 20])).collect();
    run(s, "solid.fillet", json!({"edges": edges, "radius": 3}));
}

#[test]
fn a_sketch_on_either_end_of_a_rounded_loft_takes_the_whole_face() {
    let mut s = Session::default();
    rounded_loft(&mut s);
    for up in [-1.0, 1.0] {
        let world = s.world_state().unwrap();
        let loft = world.body("Loft").unwrap();
        let face = loft.body.faces(0.01).unwrap().into_iter().find(|f| f.plane_normal.is_some_and(|n| n.dot(Vec3::Z * up) > 0.999)).unwrap();
        let pick = json!({"body": "Loft", "index": face.index, "face": [face.centroid.x, face.centroid.y, face.centroid.z]});
        let id = run(&mut s, "sketch.create", json!({"plane": pick}))["sketch"].as_u64().unwrap();
        run(&mut s, "sketch.finish", json!({}));
        let mut s2 = Session::new((*s.doc).clone());
        run(&mut s2, "solid.extrude", json!({"sketch": id, "distance": 5, "operation": "new", "body_name": "Pad"}));
        let state = s2.model.state();
        let pad = state.bodies.iter().find(|b| b.name == "Pad").unwrap();
        let v = pad.body.mesh(&crate::kernel::MeshTolerance::chord(1e-2)).unwrap().measure().volume;
        // The projected outline fits the round with splines, so it is close to the face, not exact.
        assert!(rel(v, face.area * 5.0) < 1e-2, "end {up}: {v} vs {}", face.area * 5.0);
    }
}
