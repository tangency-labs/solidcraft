//! WR-300 addendum (the fork's tangency-labs/smlib@465b62b, the rule of @1d06159): with
//! `fillet_retry_radius_as_first`, `SmFilletSolver::RailRailIntersect` holds the variable
//! radii at their end values (`SmTemporaryRadiusChange`) only at a corner calculated as
//! constant radius, as the rail/side-edge search does. Held at any other corner, two
//! rounds 0.5 -> 2 along a 10 box's top edges meeting at (10, 10, 10) (the vertical edge
//! there not rounded) had their rails meet at (10, 8, 8), radius 2, where the law gives
//! 1.7, and the trace from that end, its spans held to the law
//! (`fillet_variable_span_follows_fillet`, 4438859), failed with 1001: the `SM_API` test
//! `TestSmVariableRadiusFillet`, which rounds four top edges.
//!
//! The fork's check: a sound body removing more than one round alone,
//! `(1 - pi/4) * 17.5 = 3.7555` (the mean of `r^2` along a linear law 0.5 -> 2 is
//! `(0.25 + 1 + 4) / 3 = 1.75`), and less than both, 6.8226.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::api::variable_radius_fillet;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

/// The edge from `p` to `q`.
fn edge_from_to(k: &Kernel, b: &Brep, p: Point3, q: Point3) -> EdgeId {
    b.edges()
        .ids()
        .find(|&e| {
            let ivl = edge::interval(b, e).unwrap();
            let c = edge::curve(b, e).unwrap().unwrap();
            c.evaluate_point(k, ivl.min()).unwrap().distance(p) < 1e-9
                && c.evaluate_point(k, ivl.max()).unwrap().distance(q) < 1e-9
        })
        .expect("edge")
}

fn kernel(fixed: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_variable_span_follows_fillet = true;
    k.fixes.fillet_retry_radius_as_first = fixed;
    k
}

/// The 10 box with its top edges x = 10 and y = 10 rounded 0.5 -> 2 towards their
/// common vertex (10, 10, 10).
fn two_rounds(k: &mut Kernel) -> solidcore::Result<Brep> {
    let mut b = create_box(k, Vec3::ZERO, 10.0, 10.0, 10.0).unwrap();
    let corner = Point3::new(10.0, 10.0, 10.0);
    let edges = [
        edge_from_to(k, &b, Point3::new(10.0, 0.0, 10.0), corner),
        edge_from_to(k, &b, Point3::new(0.0, 10.0, 10.0), corner),
    ];
    variable_radius_fillet(k, &mut b, &edges, 0.5, 2.0, 1, 1)?;
    Ok(b)
}

#[test]
fn two_variable_rounds_meet_at_their_law_radii() {
    let mut k = kernel(true);
    let mut b = two_rounds(&mut k).unwrap();
    let removed = 1000.0 - volume(&k, &mut b);
    let one = (1.0 - core::f64::consts::FRAC_PI_4) * 17.5;
    assert!(
        removed > one && removed < 6.8226,
        "removed {removed}, one round alone {one}"
    );
}

#[test]
fn upstream_holds_the_end_radii_and_the_trace_fails() {
    let mut k = kernel(false);
    let err = two_rounds(&mut k).unwrap_err();
    assert!(
        matches!(err, solidcore::SolidError::Upstream { code: 1001, .. }),
        "{err:?}"
    );
}

/// `TestSmVariableRadiusFillet`'s four top edges, each 0.5 -> 2 along its own direction:
/// the opposite edges' rounds are disjoint, so the body loses more than two rounds and
/// less than four.
#[test]
fn four_top_edges_round() {
    let mut k = kernel(true);
    let mut b = create_box(&mut k, Vec3::ZERO, 10.0, 10.0, 10.0).unwrap();
    let p = |x: f64, y: f64| Point3::new(x, y, 10.0);
    let edges = [
        edge_from_to(&k, &b, p(10.0, 0.0), p(10.0, 10.0)),
        edge_from_to(&k, &b, p(0.0, 10.0), p(10.0, 10.0)),
        edge_from_to(&k, &b, p(0.0, 0.0), p(0.0, 10.0)),
        edge_from_to(&k, &b, p(0.0, 0.0), p(10.0, 0.0)),
    ];
    variable_radius_fillet(&mut k, &mut b, &edges, 0.5, 2.0, 1, 1).unwrap();
    let removed = 1000.0 - volume(&k, &mut b);
    let one = (1.0 - core::f64::consts::FRAC_PI_4) * 17.5;
    assert!(
        removed > 2.0 * one && removed < 4.0 * one,
        "removed {removed}, one round alone {one}"
    );
}
