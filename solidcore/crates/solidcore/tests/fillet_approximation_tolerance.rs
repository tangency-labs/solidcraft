//! A round's approximation tolerance comes from how accurately its edge lies on its faces, not
//! from the tolerance the edge carries.
//!
//! `SetFilletParameters` sets each solver's approximation tolerance to 4 times its edge's
//! tolerance, and the fillet B-rep takes the largest of them as its own: every new edge and
//! vertex is made at that tolerance. An edge's tolerance is an upper bound on its gap to its
//! faces, and earlier operations leave it loose: an exact edge can carry 2.5e-3 on a B-rep of
//! tolerance 1e-5. The round was then built at 1e-2, and its entities failed the full check's
//! bound on entity tolerances (under 1000 times the B-rep's: `ValidatePointers` tests 5, 59 and
//! 82), though the body it was built on passed it. A small round built at a tenth of its radius
//! also left gaps of that size between its loops' edges.
//!
//! The rounds below are on a 40 x 20 x 10 box, the top edge along x at y = 0 rounded r, with
//! that edge's stored tolerance raised (geometry untouched; the body passes the check). Each
//! result is a valid solid whose volume is the box less the spandrel, `(1 - pi/4) r^2 * 40`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};
use std::f64::consts::PI;

const L: f64 = 40.0;
const W: f64 = 20.0;
const H: f64 = 10.0;

/// Every flag on, as SolidCraft's backend runs blends (`fillet_refuse_rounds_breaking_through`
/// off, its preset).
fn kernel() -> Kernel {
    let mut k = Kernel::default();
    let mut fixes = UpstreamFixes::all();
    fixes.fillet_refuse_rounds_breaking_through = false;
    k.fixes = fixes;
    k
}

fn pick(k: &Kernel, b: &Brep, q: [f64; 3]) -> EdgeId {
    let q = Point3::new(q[0], q[1], q[2]);
    b.edges()
        .ids()
        .filter_map(|e| {
            edge_closest_point(k, b, e, q)
                .ok()
                .map(|c| (e, c.closest.distance))
        })
        .filter(|&(_, d)| d <= 1e-6)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .expect("edge")
        .0
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-10, &attrs).unwrap()
}

/// The full check's failure lines, empty for a valid body.
fn failures(k: &mut Kernel, b: &mut Brep) -> Vec<String> {
    check_brep(k, b, CheckLevel::Level2).unwrap().lines()
}

/// The box with the top front edge's tolerance raised to `stored` (and its vertices', as an
/// operation raising an edge's tolerance does), that edge rounded `r`.
fn rounded(stored: f64, r: f64) -> (Kernel, Brep) {
    let mut k = kernel();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), L, W, H).unwrap();
    let e = pick(&k, &b, [L / 2.0, 0.0, H]);
    edge::set_tolerance(&mut b, e, stored, true, true).unwrap();
    assert_eq!(
        failures(&mut k, &mut b),
        Vec::<String>::new(),
        "the operand passes the check"
    );
    let e = pick(&k, &b, [L / 2.0, 0.0, H]);
    fillet_edges(&mut k, &mut b, &[e], r, 1, 1, 1.0).unwrap();
    (k, b)
}

/// The box less the spandrel of a round `r` along the 40 edge.
fn expected(r: f64) -> f64 {
    L * W * H - (1.0 - PI / 4.0) * r * r * L
}

fn assert_round(stored: f64, r: f64) {
    let (mut k, mut b) = rounded(stored, r);
    let lines = failures(&mut k, &mut b);
    assert!(
        lines.is_empty(),
        "stored {stored:e}, r {r}: the check refuses the round:\n{}",
        lines.join("\n")
    );
    assert!(
        is_manifold_solid(&b).unwrap(),
        "stored {stored:e}, r {r}: not a manifold solid"
    );
    let v = volume(&k, &mut b);
    assert!(
        (v - expected(r)).abs() < 1e-6,
        "stored {stored:e}, r {r}: volume {v}, expected {}",
        expected(r)
    );
}

/// The control: the edge at the B-rep's own tolerance, rounded r 1 and r 0.1.
#[test]
fn a_round_of_an_edge_at_the_breps_tolerance_is_valid_and_exact() {
    let mut k = kernel();
    let t = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), L, W, H)
        .unwrap()
        .tolerance();
    assert_round(t, 1.0);
    assert_round(t, 0.1);
}

/// An exact edge carrying a loose stored tolerance, across the band where 4 times it passes the
/// B-rep's entity bound (10 x 1e-5 x 1000 / 4 = 2.5e-3) and on both sides of it.
#[test]
fn a_round_of_an_exact_edge_carrying_a_loose_tolerance_is_valid_and_exact() {
    for stored in [1e-4, 1e-3, 2e-3, 2.4e-3, 2.6e-3, 3e-3, 5e-3] {
        assert_round(stored, 1.0);
    }
}

/// A small round: built at 4 times a loose tolerance it would be approximated at a tenth of its
/// radius.
#[test]
fn a_small_round_of_an_exact_edge_carrying_a_loose_tolerance_is_valid_and_exact() {
    for stored in [1e-3, 2.6e-3, 5e-3] {
        assert_round(stored, 0.1);
    }
}
