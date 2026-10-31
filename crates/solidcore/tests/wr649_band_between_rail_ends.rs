//! Regression test for WR-649 (the fork's tangency-labs/smlib@d548305): with
//! `fillet_refuse_overlapping_bands` (WR-542) and `fillet_mitre_concave_face_corners`
//! (WR-642), a blend's band on a face is its rail between the rail's end vertices. The
//! rails are not trimmed yet when the bands are checked, and run on past the corners that end
//! them; sampled over the whole curve, the rail of SolidCraft's #871 part's plate-top round
//! on the end face, run on past the shoulder's mitre (y 13.244), crossed the band of the
//! window's y 14 round (rail y 13.5) at (23.5, 13.8999, 9.5), and r 0.5 was refused as no
//! room.
//!
//! The part: a 14 wide stepped bar with a window through it, all 24 edges of both end
//! faces rounded. The expected volume is the fork's closed form
//! `V0 - 2 L r^2 (1 - pi/4)` (L = 107.6, each end face's 12 edges; the six mitres' and
//! six convex corners' terms cancel at each end). At r 1 the rounds overlap across the
//! 1.26 wall and the fillet is still refused.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, PI};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel};

const A: f64 = 12.744_373_655_385_26;
const B: f64 = 22.692_759_633_669_36;

fn p(x: f64, y: f64, z: f64) -> Point3 {
    Point3::new(x, y, z)
}

fn boxed(k: &mut Kernel, lo: Point3, hi: Point3) -> Brep {
    create_box(k, lo, hi.x - lo.x, hi.y - lo.y, hi.z - lo.z).unwrap()
}

/// SolidCraft's #871 part: a base, a top on it, a window through both.
fn part(k: &mut Kernel) -> Brep {
    let base = boxed(k, p(9.5, 8.0, 0.0), p(23.5, 29.5, 10.0));
    let top = boxed(k, p(9.5, A, 10.0), p(23.5, B, 20.0));
    let joined = boolean_union(k, base, top).unwrap();
    let window = boxed(k, p(9.5, 14.0, 7.0), p(23.5, 19.2, 14.1));
    boolean_difference(k, joined, window).unwrap()
}

fn v0() -> f64 {
    14.0 * (21.5 * 10.0 + (B - A) * 10.0 - 5.2 * 7.1)
}

/// The (y, z) of a point on each of an end face's 12 edges.
const END_EDGE_POINTS: [(f64, f64); 12] = [
    (A, 15.0),
    (10.372_186_827_692_63, 10.0),
    (8.0, 5.0),
    (18.75, 0.0),
    (29.5, 5.0),
    (26.096_379_816_834_68, 10.0),
    (B, 15.0),
    (17.718_566_644_527_31, 20.0),
    (16.6, 7.0),
    (14.0, 10.55),
    (16.6, 14.1),
    (19.2, 10.55),
];

/// The edge of `b` through `q` (every edge of the part is a line segment).
fn edge_through(k: &Kernel, b: &Brep, q: Point3) -> EdgeId {
    let found: Vec<EdgeId> = b
        .edges()
        .iter()
        .filter(|(_, e)| {
            let c = e.curve.as_ref().unwrap();
            let s = c.evaluate_point(k, e.interval.min()).unwrap();
            let t = c.evaluate_point(k, e.interval.max()).unwrap();
            let d = t - s;
            let len2 = d.dot(d);
            let u = ((q - s).dot(d) / len2).clamp(0.0, 1.0);
            (s + d * u).distance(q) < 1e-6
        })
        .map(|(id, _)| id)
        .collect();
    assert_eq!(found.len(), 1, "one edge through {q:?}");
    found[0]
}

fn round_the_ends(r: f64, fixed: bool) -> Result<(usize, f64), String> {
    let mut k = Kernel::default();
    k.fixes.fillet_mitre_concave_face_corners = true;
    k.fixes.fillet_refuse_overlapping_bands = fixed;
    let mut b = part(&mut k);
    let mut ids = Vec::new();
    for x in [23.5, 9.5] {
        for &(y, z) in &END_EDGE_POINTS {
            ids.push(edge_through(&k, &b, p(x, y, z)));
        }
    }
    fillet_edges(&mut k, &mut b, &ids, r, 1, 1, 1.0).map_err(|e| format!("{e}"))?;
    assert!(
        is_manifold_solid(&b).unwrap(),
        "r {r}: not a manifold solid"
    );
    b.refresh_cached_topology();
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "r {r}: check_brep: {lines:?}");
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let v = brep_volume(&k, &mut b, 1e-6, &attrs).unwrap();
    Ok((b.faces().ids().count(), v))
}

#[test]
fn wr649_end_face_rounds_mitred_at_concave_corners() {
    let r = 0.5;
    let (faces, v) = round_the_ends(r, true).expect("r 0.5 builds");
    let expected = v0() - 2.0 * 107.6 * r * r * (1.0 - PI / 4.0);
    println!("r {r}: {faces} faces, volume {v:.6} against {expected:.6}");
    assert_eq!(faces, 38);
    // the fork: 3874.348250 against 3874.348458 (the corners' curves are fitted
    // intersections)
    assert!(
        (v - expected).abs() <= 1e-7 * expected,
        "volume {v} against {expected} (diff {:e})",
        v - expected
    );
    // r 1: the rounds overlap across the 1.26 wall, still no room
    assert!(round_the_ends(1.0, true).is_err());
}

#[test]
fn wr649_without_the_band_check() {
    // Off, the band check does not run (upstream SMLib has none). r 0.5 builds the same
    // body, and r 1 is not refused (the body it builds is not a manifold solid).
    let (faces, v) = round_the_ends(0.5, false).expect("r 0.5 builds");
    assert_eq!(faces, 38);
    let (on_faces, on_v) = round_the_ends(0.5, true).expect("r 0.5 builds");
    assert_eq!((faces, v.to_bits()), (on_faces, on_v.to_bits()));
}
