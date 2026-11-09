//! tangency-labs/solidcore#1513: uniting a cylinder (r 10, h 10) with a crescent prism
//! standing on its rim from outside. The crescent is the r 6 disk centred on the rim,
//! minus the r 10 disk. Upstream, `CloseIntersectionLoop` relates two edges between the
//! same vertices whenever a closest-point drop succeeds. That relates the rim's long arc
//! to the crescent's r 6 arc, so an edge ends up 37.9 off its face.
//!
//! Test-only: the issue does not reproduce in the Rust port. At the default fixes its
//! union of these operands keeps every edge on its faces, with the exact volume.
//!
//! The crescent stands at 0, 45, 90 and 180 degrees, so its rim arc crosses the
//! cylinder's seam or stays clear of it. It is built two ways: as the issue builds it,
//! by sweeping its two arcs, and as the r 6 cylinder minus the r 10 one. Each union is
//! a manifold solid that `check_brep` (level 2) finds clean. Every edge's curve lies on
//! each of its faces' surfaces within the edge's tolerance. The volume is that of the
//! two cylinders' union, (π (10² + 6²) − lens) 10, to 1e-6 relative; the results come
//! within 2e-9.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use std::f64::consts::PI;

use offset_fixture::volume;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::curve::curve::Curve;
use solidcore::math::Vec3;
use solidcore::primitive::api_curves::create_arc;
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::query::api_queries::{brep_is_manifold_solid, surface_closest_point};
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edge;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::{Brep, Kernel};

/// The cylinder's radius; the crescent's circle (r 6) is centred on its rim.
const R: f64 = 10.0;

/// The crescent prism as the issue builds it: a linear sweep (10 up, capped) of the
/// rim's arc inside the r 6 circle and the r 6 arc outside the rim. The two circles
/// meet at x = 8.2 in the crescent's frame.
fn crescent_swept(k: &mut Kernel, angle_deg: f64) -> Brep {
    let y = (R * R - 8.2 * 8.2).sqrt();
    let big = y.atan2(8.2).to_degrees();
    let small = y.atan2(8.2 - R).to_degrees();
    let centre = Vec3::new(
        R * angle_deg.to_radians().cos(),
        R * angle_deg.to_radians().sin(),
        0.0,
    );
    let inner = Curve::from(
        create_arc(
            Vec3::new(0.0, 0.0, 0.0),
            R,
            angle_deg - big,
            angle_deg + big,
        )
        .unwrap(),
    );
    let outer = Curve::from(create_arc(centre, 6.0, angle_deg - small, angle_deg + small).unwrap());
    let curves = [Some(&inner), Some(&outer)];
    create_linear_sweep(k, &curves, Vec3::new(0.0, 0.0, 1.0), 10.0, true).unwrap()
}

/// The crescent prism as the r 6 cylinder minus the r 10 one.
fn crescent_booleaned(k: &mut Kernel, angle_deg: f64) -> Brep {
    let centre = Vec3::new(
        R * angle_deg.to_radians().cos(),
        R * angle_deg.to_radians().sin(),
        0.0,
    );
    let small = create_cylinder(k, centre, 6.0, 10.0).unwrap();
    let big = create_cylinder(k, Vec3::new(0.0, 0.0, 0.0), R, 10.0).unwrap();
    boolean_difference(k, small, big).unwrap()
}

/// The volume of the union of the r 10 and r 6 cylinders (h 10) whose axes are 10 apart.
fn expected_volume() -> f64 {
    let (r, d) = (6.0_f64, R);
    let lens = r * r * ((d * d + r * r - R * R) / (2.0 * d * r)).acos()
        + R * R * ((d * d + R * R - r * r) / (2.0 * d * R)).acos()
        - 0.5 * ((-d + r + R) * (d + r - R) * (d - r + R) * (d + r + R)).sqrt();
    (PI * (R * R + r * r) - lens) * 10.0
}

/// The largest distance, over 9 samples of each edge's curve, of the curve from each of
/// its faces' surfaces, against the edge's tolerance.
fn assert_edges_on_faces(k: &Kernel, b: &Brep) {
    for &e in b.edge_list() {
        let c = edge::curve(b, e).unwrap().unwrap();
        let iv = edge::interval(b, e).unwrap();
        let tol = edge::tolerance(b, e).unwrap();
        for eu in edge::edgeuses(b, e, None, None).unwrap() {
            let f = eu1::face(b, eu).unwrap().unwrap();
            let s = b.face(f).unwrap().surface.as_ref().unwrap();
            for i in 0..=8 {
                let p = c
                    .evaluate_point(k, iv.min() + iv.length() * f64::from(i) / 8.0)
                    .unwrap();
                let gap = surface_closest_point(k, s, p).unwrap().closest.distance;
                assert!(gap <= tol, "edge {e:?} {gap} off face {f:?} (tol {tol})");
            }
        }
    }
}

fn assert_union_sound(angle_deg: f64, swept: bool) {
    let mut k = Kernel::default();
    let cylinder = create_cylinder(&mut k, Vec3::new(0.0, 0.0, 0.0), R, 10.0).unwrap();
    let prism = if swept {
        crescent_swept(&mut k, angle_deg)
    } else {
        crescent_booleaned(&mut k, angle_deg)
    };
    let mut b = boolean_union(&mut k, cylinder, prism).unwrap();
    assert!(brep_is_manifold_solid(&b).unwrap());
    assert_edges_on_faces(&k, &b);
    let expected = expected_volume();
    let v = volume(&k, &mut b).unwrap();
    assert!(
        (v - expected).abs() <= 1e-6 * expected,
        "volume {v}, expected {expected}"
    );
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
}

#[test]
fn swept_crescent_at_0_degrees() {
    assert_union_sound(0.0, true);
}

#[test]
fn swept_crescent_at_45_degrees() {
    assert_union_sound(45.0, true);
}

#[test]
fn swept_crescent_at_90_degrees() {
    assert_union_sound(90.0, true);
}

#[test]
fn swept_crescent_at_180_degrees() {
    assert_union_sound(180.0, true);
}

#[test]
fn booleaned_crescent_at_0_degrees() {
    assert_union_sound(0.0, false);
}

#[test]
fn booleaned_crescent_at_45_degrees() {
    assert_union_sound(45.0, false);
}

#[test]
fn booleaned_crescent_at_90_degrees() {
    assert_union_sound(90.0, false);
}

#[test]
fn booleaned_crescent_at_180_degrees() {
    assert_union_sound(180.0, false);
}
