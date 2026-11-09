//! #2826: a round of the join between two cylinders must not depend on where their seams lie.
//!
//! The part: a cylinder A (r 20, z -50..50) united with a cylinder B (r 18.9, h 50) turned
//! -115 degrees about y and moved to (24, 0, 8). The part is symmetric about the plane y = 0.
//! Both primitives put their seam at +x, so both seams lie in that plane and meet the join at
//! the same point, (20, 0, 26.99): there the two halves of the join meet A's seam and B's seam,
//! four edges at one vertex. Turning A about its own axis moves its seam and nothing else.
//!
//! Rounding the whole join r 2 builds wherever A's seam lies, except where it lies in the plane
//! of symmetry: there the round failed (`SM_ERR` at the fillet's finish, "Fillet Corner Error:
//! Unable to Compute Geometry"). At the shared vertex the round's section lies in the plane
//! y = 0, so where its rail on B crosses B's seam, the point on the other rail is on A's seam.
//! Its first guess on A came from a global point solve, which finds a point on a seam twice,
//! once at each end of the period, and two answers were refused. Each half of the join has its
//! own side of A's seam: the guess is now the filleted edge use's own UV point, from its UV trim
//! curve, made where the boolean left none.
//!
//! The rounded part is the same solid wherever A's seam lies: every round builds a closed,
//! manifold solid that passes the Level 2 check, and its volume agrees with the one built with
//! A's seam turned 90 degrees away (where the join is one closed edge) to the rounds' fit.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{Kernel, KernelOptions};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::topology::brep::mirror::transform_matrix;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{Brep, EdgeId};

/// A's radius.
const RA: f64 = 20.0;
/// B's radius.
const RB: f64 = 18.9;
/// B's turn about y (degrees) and where its base centre moves.
const B_TURN: f64 = -115.0;
const B_AT: [f64; 3] = [24.0, 0.0, 8.0];
/// The round's radius.
const R: f64 = 2.0;

/// The turn of `deg` degrees about z, as a row-vector matrix (`transform_matrix`'s).
fn about_z(deg: f64) -> [[f64; 4]; 4] {
    let (s, c) = deg.to_radians().sin_cos();
    [
        [c, s, 0.0, 0.0],
        [-s, c, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

/// The turn of `deg` degrees about y, then the move to `at`.
fn about_y_then_to(deg: f64, at: [f64; 3]) -> [[f64; 4]; 4] {
    let (s, c) = deg.to_radians().sin_cos();
    [
        [c, 0.0, -s, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [s, 0.0, c, 0.0],
        [at[0], at[1], at[2], 1.0],
    ]
}

/// B's axis: its base centre and unit direction.
fn b_axis() -> (Point3, Vec3) {
    let (s, c) = B_TURN.to_radians().sin_cos();
    (Point3::new(B_AT[0], B_AT[1], B_AT[2]), Vec3::new(s, 0.0, c))
}

/// The union, with A turned `a_turn` degrees about its axis first.
fn part(k: &mut Kernel, a_turn: f64) -> Brep {
    let mut a = create_cylinder(k, Point3::new(0.0, 0.0, -50.0), RA, 100.0).unwrap();
    if a_turn != 0.0 {
        transform_matrix(k, &mut a, &about_z(a_turn), true).unwrap();
    }
    let mut b = create_cylinder(k, Point3::new(0.0, 0.0, -25.0), RB, 50.0).unwrap();
    transform_matrix(k, &mut b, &about_y_then_to(B_TURN, B_AT), true).unwrap();
    boolean_union(k, a, b).unwrap()
}

/// The edges of the join: those whose middle lies on both cylinders.
fn join(k: &Kernel, b: &Brep) -> Vec<EdgeId> {
    let (base, dir) = b_axis();
    let mut out = Vec::new();
    for &e in b.edge_list() {
        let edge = b.edge(e).unwrap();
        let Some(curve) = edge.curve.as_ref() else {
            continue;
        };
        let p = curve
            .evaluate_point(k, edge.interval.evaluate(0.5))
            .unwrap();
        let to_a = p.x.hypot(p.y);
        let w = p - base;
        let to_b = (w - dir * w.dot(dir)).length();
        if (to_a - RA).abs() < 1e-4 && (to_b - RB).abs() < 1e-4 {
            out.push(e);
        }
    }
    out
}

fn volume_of(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-9, &attrs).unwrap()
}

/// The whole join rounded r 2 with A turned `a_turn` degrees: one sound solid. Returns its
/// volume.
fn round_join(a_turn: f64) -> f64 {
    let mut k = Kernel::new(&KernelOptions::default());
    let mut b = part(&mut k, a_turn);
    let edges = join(&k, &b);
    assert!(!edges.is_empty(), "A turned {a_turn}: no join edge found");
    if let Err(e) = fillet_edges(&mut k, &mut b, &edges, R, 1, 1, 1.0) {
        panic!("A turned {a_turn}: the round of the join {edges:?} fails: {e:?}");
    }
    assert!(is_manifold_solid(&b).unwrap(), "A turned {a_turn}");
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "A turned {a_turn}: {lines:?}");
    let v = volume_of(&k, &mut b);
    println!("A turned {a_turn}: join {edges:?}, volume {v:.10}");
    v
}

/// A's seam in the plane of symmetry, where it meets B's seam on the join (the reported part,
/// the primitives' default seams), and turned off it either way and a quarter turn away. The
/// rounded part's volume is the same to the rounds' fit: the round is a fit within 4e-5 of
/// the rolling ball's envelope (four times the edges' 1e-5) over a band about 140 x 3.2 mm, at
/// most 1.8e-2 mm3 apart, 1.1e-7 of the volume.
#[test]
fn the_join_rounds_wherever_the_seams_lie() {
    let reference = round_join(90.0);
    for a_turn in [0.0, 10.0, -10.0] {
        let v = round_join(a_turn);
        let bound = 1.1e-7 * reference;
        assert!(
            (v - reference).abs() <= bound,
            "A turned {a_turn}: volume {v}, a quarter turn away {reference} (bound {bound})"
        );
    }
}
