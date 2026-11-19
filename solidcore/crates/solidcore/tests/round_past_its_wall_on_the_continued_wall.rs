//! A round bigger than the wall it rolls on is built on the wall continued past the rim and
//! trimmed at the crease (`docs/generic-blend.md` §3.4, the cliff overflow): the ball stays
//! tangent to the face it keeps and to the wall continued below the plate.
//!
//! The plate is 100 x 100 x 10, its top edge at x = 0 rounded r (L = 100, t = 10): the ball
//! touches the top face at x = r and the continued wall at z = t - r, and the material above
//! that arc goes. The 2 mm disc wall (R13 x 2, r 2.5) is the plate's section turned about the
//! disc's axis, by Pappus. A round breaking into a hollow is refused, and one ending on a
//! curved face is left to the cliff overflow, which refuses it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::blend::fillet_executive::NoRoomReason;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::brep::p1::regions;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel, SolidError};
use std::f64::consts::PI;

/// The plate's length across the round, its thickness, and its length along the edge.
const L: f64 = 100.0;
const T: f64 = 10.0;
const W: f64 = 100.0;

/// Every flag on, as SolidCraft's backend runs blends.
fn kernel() -> Kernel {
    Kernel {
        fixes: UpstreamFixes::all(),
        ..Kernel::default()
    }
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
        .filter(|&(_, d)| d <= 0.1)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .expect("edge")
        .0
}

/// A round (`fillet_edges` with SolidCraft's arguments: G1, thumbweight 1) of the edge
/// through `at`.
fn round(k: &mut Kernel, b: &mut Brep, at: [f64; 3], r: f64) -> solidcore::Result<()> {
    let e = pick(k, b, at);
    fillet_edges(k, b, &[e], r, 1, 1, 1.0)
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

/// `∫ sqrt(r² - u²) du` from 0 to `u`.
fn quarter(u: f64, r: f64) -> f64 {
    (u * (r * r - u * u).sqrt() + r * r * (u / r).asin()) / 2.0
}

/// The plate: the ball of centre (r, t - r) on the wall continued below
/// the plate, crossing the bottom at xm = r - sqrt(r² - (r - t)²): the section loses the
/// whole thickness up to xm and the region above the arc from there to x = r.
fn on_continued_wall(r: f64) -> f64 {
    let xm = r - (r * r - (r - T) * (r - T)).sqrt();
    let lost = T * xm + r * (r - xm) - quarter(r - xm, r);
    L * T * W - W * lost
}

fn plate(k: &mut Kernel) -> Brep {
    create_box(k, Point3::new(0.0, 0.0, 0.0), L, W, T).unwrap()
}

/// One lump, a manifold solid, `check_brep` clean at Level2, `faces` faces and the volume
/// `want` to `rel`.
fn holds(k: &mut Kernel, b: &mut Brep, faces: usize, want: f64, rel: f64) {
    let infinite = b.infinite_region();
    let lumps = regions(b, None)
        .unwrap()
        .into_iter()
        .filter(|r| Some(*r) != infinite)
        .count();
    assert_eq!(lumps, 1, "one lump");
    assert!(is_manifold_solid(b).unwrap(), "a manifold solid");
    let check = check_brep(k, b, CheckLevel::Level2).unwrap();
    assert!(check.lines().is_empty(), "check_brep: {:?}", check.lines());
    assert_eq!(b.faces().ids().count(), faces, "faces");
    let v = volume(k, b);
    assert!((v - want).abs() <= rel * want, "{v} vs {want}");
}

/// The round is on the wall continued below the plate.
#[test]
fn the_plate_round_is_on_the_continued_wall() {
    // (from r = L the rail on the top reaches its far edge and the top face goes)
    for r in [20.0, 50.0, 90.0] {
        let mut k = kernel();
        let mut b = plate(&mut k);
        round(&mut k, &mut b, [0.0, 50.0, T], r).unwrap();
        holds(&mut k, &mut b, 6, on_continued_wall(r), 1e-9);
    }
}

/// The disc R13 x 2, its top rim rounded r 2.5 (rig `190-fillet-disc-rim-past-its-2mm-wall`):
/// the plate's section on the continued wall turned about the axis.
#[test]
fn the_disc_round_is_on_the_continued_wall() {
    let (big, t, r) = (13.0_f64, 2.0_f64, 2.5_f64);
    let s = 3.125 * PI / 2.0 - (0.5 * 6.0_f64.sqrt() / 2.0 + 3.125 * 0.2_f64.asin());
    let removed = PI * (58.75 * 2.0 - 21.0 * s - (12.5 - (15.625 - 0.125) / 3.0));
    let continued = 338.0 * PI - removed;
    let mut k = kernel();
    let mut b = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), big, t).unwrap();
    round(&mut k, &mut b, [0.0, big, t], r).unwrap();
    holds(&mut k, &mut b, 3, continued, 1e-9);
}

/// The volume and face count of a build, or its error.
fn outcome(k: &mut Kernel, mut b: Brep, at: [f64; 3], r: f64) -> Result<(f64, usize), String> {
    match round(k, &mut b, at, r) {
        Ok(()) => Ok((volume(k, &mut b), b.faces().ids().count())),
        Err(e) => Err(format!("{e:?}")),
    }
}

/// Rig `fillets/5236`: the end rim of a cylinder r 10 rounded r 10, a hemispherical end.
#[test]
fn a_round_taking_a_whole_end_face_is_a_hemisphere() {
    let mut k = kernel();
    let body = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 30.0).unwrap();
    let (v, faces) = outcome(&mut k, body, [0.0, 10.0, 30.0], 10.0).expect("built");
    // the cylinder to 20 and the half ball
    let want = PI * 100.0 * 20.0 + 2.0 / 3.0 * PI * 1000.0;
    assert!((v - want).abs() <= 1e-9 * want, "{v} vs {want}");
    assert_eq!(faces, 3);
}

/// #2299: R7 on the outer top edge of a box shelled by 2 breaks through the roof into the
/// hollow behind, and is refused with the fit of `wr707_round_breaking_through.rs`.
#[test]
fn a_round_breaking_into_a_hollow_is_refused() {
    let mut k = kernel();
    let outer = create_box(&mut k, Point3::new(-52.0, -32.0, 0.0), 104.0, 64.0, 42.0).unwrap();
    let inner = create_box(&mut k, Point3::new(-50.0, -30.0, -1.0), 100.0, 60.0, 41.0).unwrap();
    let mut b = boolean_difference(&mut k, outer, inner).unwrap();
    let err = round(&mut k, &mut b, [0.0, 32.0, 42.0], 7.0).expect_err("refused");
    let SolidError::FilletRefused { reason, fit, .. } = err else {
        panic!("{err:?}");
    };
    assert_eq!(reason, Some(NoRoomReason::BreaksThrough));
    let limit = 2.0 * 2.0_f64.sqrt() / (2.0_f64.sqrt() - 1.0);
    assert!((fit.size - (limit - 0.01)).abs() <= 1e-4, "{}", fit.size);
}

/// A 10 x 20 x 30 block with a hole of radius 12 about (5, 30) cut into its far end, its top
/// edge x = 10, z = 30 rounded past the 10 mm top. The round would end on the cylindrical
/// end: the cliff overflow refuses it (`CliffEndFaceNotPlanar`).
#[test]
fn a_round_ending_on_a_curved_face_is_refused() {
    for r in [12.0, 10.01] {
        let mut k = kernel();
        let body = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 20.0, 30.0).unwrap();
        let c = create_cylinder(&mut k, Point3::new(5.0, 30.0, -1.0), 12.0, 32.0).unwrap();
        let body = boolean_difference(&mut k, body, c).unwrap();
        let mut body = body;
        let err = round(&mut k, &mut body, [10.0, 9.0, 30.0], r).expect_err("refused");
        let SolidError::FilletRefused { reason, .. } = err else {
            panic!("r {r}: {err:?}");
        };
        assert_eq!(reason, Some(NoRoomReason::CliffEndFaceNotPlanar), "r {r}");
    }
}
