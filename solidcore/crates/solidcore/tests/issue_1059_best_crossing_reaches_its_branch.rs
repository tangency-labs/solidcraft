//! tangency-labs/solidcore#1059 (WR-410), `Kernel::fixes.boolean_vertex_by_best_crossing`.
//!
//! What this test shows, and what it does not: the fork's case (a rounded pad on a
//! lofted cap, rig crashes/4092, an edge start 1.2e-4 from its vertex) is not
//! reproducible here, and no box geometry found so far shows the fix's material effect.
//! A sweep of 32 placements of a 20 x 4 x 4 bar piercing a 10 mm cube's top face
//! (tilt -1 to -0.001 degrees, turn 10 to 85, roll 0 or 30) gave a worst edge-end gap of
//! at most 4.07e-10 with the flag off and the same with it on. So this test only shows
//! that the flag changes nothing here: with the bar tilted -1 degree and turned 30 the
//! result is unchanged within 1e-13 (the same topology, volume and vertices). That the
//! flag's branch is reached for this geometry is shown by the probe
//! (`issue_1059_grazing_edge_crossing_probe.rs`: the beta = 30 rows move vertices by at
//! most 1e-14, round-off), not asserted, since that move is rounding noise. With the flag off the same union is compared with the
//! C++ in `fixtures/boolean.api_brep/issue_1059_grazing_bar.json` (it passes).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::boolean_union;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p1;
use solidcore::topology::proto_topology::p1::api_transform::{
    api_rotate, api_translate, TransformObject,
};
use solidcore::{Brep, Kernel};

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

/// The cube [0,10]^3 united with the bar (-10,-2,-2) 20 x 4 x 4 tilted `alpha` about y,
/// turned `beta` about z and moved by (5, 5, 8), as the fixture.
fn union(fix: bool, alpha: f64, beta: f64) -> (Kernel, Brep) {
    let mut k = Kernel::default();
    k.fixes.boolean_vertex_by_best_crossing = fix;
    let a = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let mut b = create_box(&mut k, Point3::new(-10.0, -2.0, -2.0), 20.0, 4.0, 4.0).unwrap();
    let o = Point3::new(0.0, 0.0, 0.0);
    api_rotate(
        &mut k,
        Some(TransformObject::Brep(&mut b)),
        o,
        Vec3::new(0.0, 1.0, 0.0),
        alpha,
    )
    .unwrap();
    api_rotate(
        &mut k,
        Some(TransformObject::Brep(&mut b)),
        o,
        Vec3::new(0.0, 0.0, 1.0),
        beta,
    )
    .unwrap();
    api_translate(
        &mut k,
        Some(TransformObject::Brep(&mut b)),
        Vec3::new(5.0, 5.0, 8.0),
    )
    .unwrap();
    let u = boolean_union(&mut k, a, b).unwrap();
    solidcore::topology::integrity::check(&u).unwrap();
    (k, u)
}

fn points(b: &Brep) -> Vec<Point3> {
    p1::vertices(b, None)
        .unwrap()
        .into_iter()
        .map(|v| b.vertex(v).unwrap().point)
        .collect()
}

#[test]
fn issue_1059_best_crossing_result_unchanged() {
    let (k_off, mut off) = union(false, -1.0, 30.0);
    let (k_on, mut on) = union(true, -1.0, 30.0);
    let counts = |b: &Brep| {
        (
            p1::faces(b, None).unwrap().len(),
            p1::edges(b, None).unwrap().len(),
            p1::vertices(b, None).unwrap().len(),
        )
    };
    assert_eq!(counts(&off), counts(&on), "topology");
    let (v_off, v_on) = (volume(&k_off, &mut off), volume(&k_on, &mut on));
    assert!(
        (v_off - v_on).abs() <= 1e-13 * v_off,
        "volume {v_off} vs {v_on}"
    );
    let (p_off, p_on) = (points(&off), points(&on));
    let moved = p_off
        .iter()
        .map(|p| p_on.iter().map(|q| p.distance(*q)).fold(f64::MAX, f64::min))
        .fold(0.0_f64, f64::max);
    eprintln!("issue_1059: largest vertex move {moved:e}, volume {v_off} vs {v_on}");
    // nothing material changes; that the flag's branch is reached here is shown by the
    // probe sweep (vertices move by round-off), not asserted: that move is ~1e-15 noise
    assert!(moved <= 1e-13, "vertex moved {moved:e}");
}
