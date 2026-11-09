//! A round that splits an earlier round's rim leaves its new vertex on the corner of the faces
//! there, not on the rim's curve.
//!
//! An earlier round ending on a cone has a rim (the round's section on the cone) that SMLib
//! approximates: its curve may lie up to its tolerance off one of its faces, and does here by more
//! than the B-rep's intersection tolerance (twice its zone tolerance). That is valid: only a vertex
//! must be within the intersection tolerance of each of its faces (`SmVertex` test 1). A later round
//! of the edge running up into the earlier one splits that rim, and the vertex it puts on the rim's
//! curve inherits the curve's gap; the full check then refused a result the operand passed. After
//! the fillet's merge, the vertices the fillet made go to the corner their faces define, as the
//! boolean's merge places its own (#3090), with the same guards.
//!
//! The body: a cone frustum (base radius `RB`, top `RT`, height `H`) cut by the plane x = `XP`
//! (material on x < `XP`); the top edge of that plane rounded `BIG`, whose ends run onto the cone;
//! then the edge where the plane meets the cone rounded `SMALL` up into the first round.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cone};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel};
use std::f64::consts::PI;

const H: f64 = 12.0;
const BIG: f64 = 3.0;

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
        .filter(|&(_, d)| d <= 1e-3)
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

/// The frustum less its part past x = `xp`: the integral over z of the circle less its segment
/// past the chord (Simpson, 2000 steps).
fn cut_cone_volume(rb: f64, rt: f64, xp: f64) -> f64 {
    let area = |z: f64| {
        let r = rb + (rt - rb) * z / H;
        let segment = r * r * (xp / r).acos() - xp * (r * r - xp * xp).sqrt();
        PI * r * r - segment
    };
    let n = 2000;
    let dz = H / f64::from(n);
    (0..=n)
        .map(|i| {
            let w = if i == 0 || i == n {
                1.0
            } else if i % 2 == 1 {
                4.0
            } else {
                2.0
            };
            w * area(f64::from(i) * dz)
        })
        .sum::<f64>()
        * dz
        / 3.0
}

/// The two rounds on the cut frustum: the body, its volume before the rounds, and the volume
/// the rounds may remove at most and at least. The first round's spandrel is (1 - pi/4) BIG^2
/// along the top edge's length (the plane and the top are square); each of its ends, where it
/// runs onto the cone, changes it by less than the BIG cube around it. The second round's
/// spandrel at its narrowest material angle (the plane and the cone meet at no more than 90 deg)
/// along the edge's height is a lower bound, its ends again bounded by a SMALL cube each.
fn rounded(rb: f64, rt: f64, xp: f64, small: f64) -> (Kernel, Brep, f64, [f64; 2]) {
    let mut k = kernel();
    let cone = create_cone(&mut k, Point3::new(0.0, 0.0, 0.0), rb, rt, H).unwrap();
    let cutter = create_box(&mut k, Point3::new(xp, -40.0, -10.0), 80.0, 80.0, 80.0).unwrap();
    let mut b = boolean_difference(&mut k, cone, cutter).unwrap();
    let base = volume(&k, &mut b);
    let top = pick(&k, &b, [xp, 0.0, H]);
    fillet_edges(&mut k, &mut b, &[top], BIG, 1, 1, 1.0).unwrap();
    let z = (H - BIG) / 2.0;
    let r = rb + (rt - rb) * z / H;
    let side = pick(&k, &b, [xp, (r * r - xp * xp).sqrt(), z]);
    fillet_edges(&mut k, &mut b, &[side], small, 1, 1, 1.0).unwrap();
    let spandrel = 1.0 - PI / 4.0;
    let top_length = 2.0 * (rt * rt - xp * xp).sqrt();
    let big = spandrel * BIG * BIG * top_length;
    let small_lower = spandrel * small * small * (H - BIG - small);
    let removed = [
        big + small_lower - 2.0 * BIG.powi(3) - 2.0 * small.powi(3),
        big + 2.0 * BIG.powi(3) + spandrel * small * small * H + 2.0 * small.powi(3),
    ];
    (k, b, base, removed)
}

fn assert_valid_rounds(rb: f64, rt: f64, xp: f64, small: f64) {
    let (mut k, mut b, base, removed) = rounded(rb, rt, xp, small);
    let analytic = cut_cone_volume(rb, rt, xp);
    assert!(
        (base - analytic).abs() < 1e-6 * analytic,
        "the cut frustum's volume {base}, analytic {analytic}"
    );
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(
        lines.is_empty(),
        "cone {rb}..{rt}, plane {xp}, rounds {BIG} then {small}: the check refuses:\n{}",
        lines.join("\n")
    );
    assert!(is_manifold_solid(&b).unwrap(), "not a manifold solid");
    let gone = base - volume(&k, &mut b);
    assert!(
        removed[0] < gone && gone < removed[1],
        "the rounds removed {gone}, outside {removed:?}"
    );
}

/// The control: a frustum, plane and rounds whose new vertex lands within the intersection
/// tolerance of the earlier round's face wherever it stands on the rim.
#[test]
fn rounds_meeting_on_a_rim_within_the_intersection_tolerance_are_valid() {
    assert_valid_rounds(20.0, 20.0, 3.0, 1.0);
}

/// The defect: the second round's vertex on the earlier round's rim stood more than the
/// intersection tolerance off that round's face.
#[test]
fn a_round_splitting_an_earlier_rounds_rim_places_its_vertex_on_their_faces_corner() {
    assert_valid_rounds(22.0, 21.0, 3.0, 1.0);
    assert_valid_rounds(22.0, 21.0, 8.0, 0.8);
    assert_valid_rounds(22.0, 21.0, 8.0, 0.5);
}
