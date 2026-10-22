//! A loft section that is a ring keeps its passage (#2435); incompatible sections
//! are refused atomically rather than silently filling the hole (#1184).

use super::*;

/// A sketch `name` at height `z` with circles of radius 20 and 12 about the origin.
fn ring(s: &mut Session, name: &str, z: f64) {
    run(s, "sketch.create", json!({"plane": "XY", "offset": z, "name": name}));
    run(s, "sketch.circle.center", json!({"center": [0, 0], "radius": 20}));
    run(s, "sketch.circle.center", json!({"center": [0, 0], "radius": 12}));
    run(s, "sketch.finish", json!({}));
}

fn refused(s: &mut Session, sections: Value, message: &str) {
    let saved = s.doc.to_json();
    let err = s.execute("solid.loft", &json!({"sections": sections, "operation": "new"})).err().map(|e| e.to_string());
    assert!(err.as_deref().is_some_and(|e| e.contains(message)), "{err:?}");
    assert_eq!(s.doc.to_json(), saved);
}

#[test]
fn a_ring_section_keeps_its_hole_and_rejects_a_disc() {
    let mut s = Session::default();
    ring(&mut s, "A", 0.0);
    ring(&mut s, "B", 20.0);
    let ring_at = |n: &str| json!({"sketch": n, "profiles": [{"point": [16, 0]}]});
    let sections = json!([ring_at("A"), ring_at("B")]);
    run(&mut s, "solid.loft", json!({"sections": sections, "operation": "new"}));
    let want = PI * (400.0 - 144.0) * 20.0;
    assert!(rel(volume(&mut s), want) < 1e-3);
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert!(reopened.model.results.iter().all(|r| r.error.is_none()));
    let st = reopened.model.state();
    let body = &st.bodies[0];
    assert!(body.body.validity().is_empty());
    assert_eq!(body.body.shell_count(), 1);
    for face in 0..body.body.face_count() {
        assert!(solidcraft_doc::naming::face_name_exact(body, face), "face {face} lost its sketch-curve provenance");
    }
    assert!(!body.body.mesh(&solidcraft_kernel::MeshTolerance::chord(0.001)).unwrap().contains(solidcraft_geom::Vec3::new(0.0, 0.0, 10.0)));
    run(&mut s, "edit.undo", json!({}));
    assert!(s.model.state().bodies.is_empty());
    run(&mut s, "edit.redo", json!({}));
    assert!(rel(volume(&mut s), want) < 1e-3);
    run(&mut s, "edit.undo", json!({}));
    // A ring and a plain disc, in either order.
    let disc_b = json!({"sketch": "B", "profiles": [{"point": [0, 0]}]});
    refused(&mut s, json!([ring_at("A"), disc_b]), "same number of holes");
    refused(&mut s, json!([disc_b, ring_at("A")]), "same number of holes");
    // The discs inside the holes still loft into a cylinder of radius 12.
    run(&mut s, "solid.loft", json!({"sections": [{"sketch": "A", "profiles": [{"point": [0, 0]}]}, disc_b], "operation": "new"}));
    let want = std::f64::consts::PI * 12.0 * 12.0 * 20.0;
    let got = volume(&mut s);
    assert!(rel(got, want) < 1e-3, "{got} vs {want}");
}

#[test]
fn a_ring_lofted_to_a_point_is_refused() {
    let mut s = Session::default();
    // Square, since lofting curved sections to a point is not supported at all yet.
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "A"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-20, -20], "p1": [20, 20]}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-12, -12], "p1": [12, 12]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 20, "name": "Top"}));
    run(&mut s, "sketch.point", json!({"point": [0, 0], "id": "apex"}));
    run(&mut s, "sketch.finish", json!({}));
    refused(
        &mut s,
        json!([{"sketch": "A", "profiles": [{"point": [16, 0]}]}, {"sketch": "Top", "point": "apex"}]),
        "lofting a section with holes to a point",
    );
}

#[test]
fn a_ring_loft_joins_cuts_and_intersects_without_filling_the_hole() {
    for operation in ["join", "cut", "intersect"] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 50, "width": 50, "height": 10, "corner": [-25, -25, -5]}));
        ring(&mut s, "A", 0.0);
        ring(&mut s, "B", 20.0);
        let sections = json!([{"sketch": "A", "profiles": [{"point": [16, 0]}]}, {"sketch": "B", "profiles": [{"point": [16, 0]}]}]);
        run(&mut s, "solid.loft", json!({"sections": sections, "operation": operation}));
        let area = PI * (400.0 - 144.0);
        let want = match operation {
            "join" => 25000.0 + area * 15.0,
            "cut" => 25000.0 - area * 5.0,
            _ => area * 5.0,
        };
        assert!(rel(volume(&mut s), want) < 1e-3, "{operation}");
        for b in &s.model.state().bodies {
            assert!(b.body.validity().is_empty(), "{operation}: {:?}", b.body.validity());
        }
    }
}

#[test]
fn tapered_ring_loft_reproduction() {
    let mut s = Session::default();
    s.run_script(&serde_json::from_str(include_str!("../../../../examples/regressions/loft_rings.json")).unwrap()).unwrap();
    let want = PI * 30.0 / 3.0 * ((400.0 + 280.0 + 196.0) - (144.0 + 96.0 + 64.0));
    assert!(rel(volume(&mut s), want) < 1e-3);
}
