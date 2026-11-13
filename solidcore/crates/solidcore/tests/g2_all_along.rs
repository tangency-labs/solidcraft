//! `query::tangent_edge::g2_all_along` (solidcraft#13914): whether an edge's faces bend alike
//! across it all along it (G2), against closed forms:
//! - a cylinder's seam and a torus's seams (rings): one surface on both sides, continuous;
//! - a box's top front edge rounded r 1 (a B-spline fit, not exact): a jump at SolidCraft's bend,
//!   none where twice the scale is asked;
//! - a box's edge, its faces square: not tangent, so not G2 (decided by the tangency first);
//! - a jump asked that is not finite and non-negative is refused.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder, create_torus};
use solidcore::query::api_queries::{edge_closest_point, get_edges};
use solidcore::query::tangent_edge::{g2_all_along, G2};
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel};
use std::f64::consts::PI;

/// SolidCraft's `ANGLE`: half a degree, the tangency asked.
const SMOOTH: f64 = 0.5 * PI / 180.0;
/// SolidCraft's `BEND`: half a degree, relative, the curvature jump asked.
const BEND: f64 = 0.5 * PI / 180.0;

fn pick(k: &Kernel, b: &Brep, q: Point3) -> EdgeId {
    get_edges(b)
        .unwrap()
        .into_iter()
        .filter_map(|e| {
            edge_closest_point(k, b, e, q)
                .ok()
                .map(|c| (e, c.closest.distance))
        })
        .filter(|&(_, d)| d <= 1e-6)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .expect("an edge through the point")
        .0
}

#[test]
fn a_cylinder_s_seam_is_continuous() {
    let mut k = Kernel::default();
    let mut b = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 3.0, 7.0).unwrap();
    let seam = pick(&k, &b, Point3::new(3.0, 0.0, 3.5));
    assert_eq!(
        g2_all_along(&k, &mut b, seam, SMOOTH, BEND).unwrap(),
        Some(G2::Continuous)
    );
}

#[test]
fn a_torus_s_rings_are_continuous() {
    let mut k = Kernel::default();
    let mut b = create_torus(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 3.0).unwrap();
    let edges = get_edges(&b).unwrap();
    assert!(!edges.is_empty());
    for e in edges {
        assert_eq!(
            g2_all_along(&k, &mut b, e, SMOOTH, BEND).unwrap(),
            Some(G2::Continuous),
            "{e:?}"
        );
    }
}

/// The box's top front edge rounded r 1: the round runs into the top along y = 1, z = 4 and
/// into the front along y = 0, z = 3, each flat, while the round bends about 1 / r = 1 across:
/// a jump at SolidCraft's bend, none where twice the scale is asked.
#[test]
fn a_round_jumps_from_its_flat_faces() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 6.0, 4.0).unwrap();
    let e = pick(&k, &b, Point3::new(5.0, 0.0, 4.0));
    fillet_edges(&mut k, &mut b, &[e], 1.0, 1, 2, 0.0).unwrap();
    for at in [Point3::new(5.0, 1.0, 4.0), Point3::new(5.0, 0.0, 3.0)] {
        let e = pick(&k, &b, at);
        // the round is a B-spline fit of the cylinder (not exact), so only the verdicts are
        // asserted here; the closed-form value is the stadium's, on an exact cylinder
        match g2_all_along(&k, &mut b, e, SMOOTH, BEND).unwrap() {
            Some(G2::Jumps { .. }) => {}
            other => panic!("{at:?}: {other:?}"),
        }
        // the jump is 1 of the scale 1 (the round's curvature): continuous where 2 is asked
        assert_eq!(
            g2_all_along(&k, &mut b, e, SMOOTH, 2.0).unwrap(),
            Some(G2::Continuous),
            "{at:?}"
        );
    }
}

#[test]
fn a_box_s_square_edge_is_not_tangent() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 6.0, 4.0).unwrap();
    let e = pick(&k, &b, Point3::new(5.0, 0.0, 4.0));
    assert_eq!(
        g2_all_along(&k, &mut b, e, SMOOTH, BEND).unwrap(),
        Some(G2::NotTangent)
    );
}

#[test]
fn a_jump_not_finite_and_non_negative_is_refused() {
    let mut k = Kernel::default();
    let mut b = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 3.0, 7.0).unwrap();
    let seam = pick(&k, &b, Point3::new(3.0, 0.0, 3.5));
    for bad in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(
            g2_all_along(&k, &mut b, seam, SMOOTH, bad).is_err(),
            "{bad}"
        );
    }
}
