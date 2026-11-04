//! WR-512 (`UpstreamFixes::ssi_singular_start_inside_direction`, #1522, #1576) at operation
//! level: #1522's knife-edge part. A 41 1/6 x 21 plate is joined with the corner a circle of
//! radius 21 leaves, 10 high, whose arc wall runs out into the back face in a knife edge; its
//! top front edge is rounded R2, then the arc wall's top edge R2, both picked at the points of
//! the fork's capi test (tangency-labs/smlib@d37ca80, `capi/tests/ops_kernel.cpp`). The arc
//! round ends on the front round where both are tangent to the top face, a start point of
//! their intersection with four directions out of it. Upstream starts the curve along the
//! first of them, which need not point into either face, and the round fails with 1001; with
//! the flag it starts along the inside direction nearest the given one and the round builds.
//!
//! The pinned C++ (v1.0.1) fails the arc round with 1001 too. With only the fork's line added
//! to `SmAdvSurfaceIntersector::ComputeSingularityPoint` (`SmAdvSurfaceIntersector.cpp`
//! recompiled and linked ahead of the pinned `libSMLib.a`) it builds one manifold solid of
//! volume 9531.3100142533 (`SmApiBrepComputeVolume` at 1e-8), the reference below. There is
//! no closed form: the round's ends, on the front round and into the knife edge, are traced.
//! #1522's second case (a 20 cube, its top back edge rounded r3, then the end face's top
//! edge r1) builds to the same volume with and without the flag, in the pinned C++ and with
//! the fix, so it does not reach this guard and is not tested here.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::{brep_is_manifold_solid, edge_closest_point};
use solidcore::topology::brep::p1::regions;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{Brep, EdgeId};
use solidcore::{Kernel, SolidError};

/// The plate's length, where the arc wall starts: 62 1/6 - 21.
const PLATE: f64 = 41.0 + 1.0 / 6.0;
/// The arc wall's radius, the part's depth.
const WALL: f64 = 21.0;
/// Both rounds' radius.
const RADIUS: f64 = 2.0;
/// The fork's capi pick on the top front edge.
const FRONT: [f64; 3] = [20.583333333333332, -21.0, 10.0];
/// The fork's capi pick on the arc wall's top edge.
const ARC: [f64; 3] = [48.045114044109852, -5.4635486396119797, 10.0];

/// The pinned C++ with the fork's line: the part after both rounds.
const BOTH_ROUNDS: f64 = 9531.3100142533;

/// The plate joined with the corner box less the cylinder, as the fork's capi test builds it.
fn part(k: &mut Kernel) -> Brep {
    let corner = create_box(k, Point3::new(PLATE, -WALL, 0.0), WALL, WALL, 10.0).unwrap();
    let disc = create_cylinder(k, Point3::new(PLATE + WALL, -WALL, 0.0), WALL, 10.0).unwrap();
    let corner = boolean_difference(k, corner, disc).unwrap();
    let plate = create_box(k, Point3::new(0.0, -WALL, 0.0), PLATE, WALL, 10.0).unwrap();
    boolean_union(k, plate, corner).unwrap()
}

/// The edge nearest `q`, within 0.05.
fn pick(k: &Kernel, b: &Brep, [x, y, z]: [f64; 3]) -> EdgeId {
    let q = Point3::new(x, y, z);
    b.edges()
        .ids()
        .filter_map(|e| {
            edge_closest_point(k, b, e, q)
                .ok()
                .map(|c| (e, c.closest.distance))
        })
        .filter(|&(_, d)| d <= 0.05)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .expect("edge")
        .0
}

/// `SmApiFilletEdges` of the edge at `at`: circular, G2, thumbweight 0.
fn round(k: &mut Kernel, b: &mut Brep, at: [f64; 3]) -> solidcore::Result<()> {
    let e = pick(k, b, at);
    fillet_edges(k, b, &[e], RADIUS, 1, 2, 0.0)
}

/// The part with its top front edge rounded, then the result of the arc wall's top edge.
fn both_rounds(k: &mut Kernel) -> (Brep, solidcore::Result<()>) {
    let mut b = part(k);
    round(k, &mut b, FRONT).unwrap();
    let r = round(k, &mut b, ARC);
    (b, r)
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

/// The arc round fails as upstream does: `SmFilletExecutive` status 1001.
fn assert_refused(r: solidcore::Result<()>) {
    match r {
        Err(SolidError::Upstream { code: 1001, .. }) => {}
        other => panic!("expected status 1001, got {other:?}"),
    }
}

/// The arc round built one manifold solid that checks clean, of the C++ volume within `tol`
/// relative.
fn assert_built(k: &mut Kernel, b: &mut Brep, r: solidcore::Result<()>, tol: f64) {
    assert_built_to(k, b, r, tol, BOTH_ROUNDS);
}

/// [`assert_built`] against the volume `want`.
fn assert_built_to(k: &mut Kernel, b: &mut Brep, r: solidcore::Result<()>, tol: f64, want: f64) {
    r.unwrap();
    let infinite = b.infinite_region();
    let solids = regions(b, None)
        .unwrap()
        .into_iter()
        .filter(|&r| Some(r) != infinite)
        .count();
    assert_eq!(solids, 1);
    assert!(brep_is_manifold_solid(b).unwrap());
    let lines = check_brep(k, b, CheckLevel::Level2).unwrap().lines();
    assert!(lines.is_empty(), "{lines:?}");
    let v = volume(k, b);
    println!("volume {v:.10} (reference: {want})");
    assert!(
        (v - want).abs() <= tol * want,
        "volume {v}, expected {want}"
    );
}

/// The kernel with every fix but `fillet_blend_corner` (the fork's blend corner End, which
/// builds this round by itself), and the WR-512 fix as given.
fn every_fix(wr512: bool) -> Kernel {
    let mut fixes = UpstreamFixes::all();
    fixes.fillet_blend_corner = false;
    fixes.ssi_singular_start_inside_direction = wr512;
    Kernel {
        fixes,
        ..Kernel::default()
    }
}

#[test]
fn default_kernel_refuses_the_arc_round() {
    let mut k = Kernel::default();
    assert!(!k.fixes.ssi_singular_start_inside_direction);
    let (_, r) = both_rounds(&mut k);
    assert_refused(r);
}

/// The default kernel with the flag: the C++ body with the fork's line, to 1e-9 (the gate's
/// tolerance; the same construction and integration on both sides).
#[test]
fn the_flag_builds_the_arc_round() {
    let mut k = Kernel::default();
    k.fixes.ssi_singular_start_inside_direction = true;
    let (mut b, r) = both_rounds(&mut k);
    assert_built(&mut k, &mut b, r, 1e-9);
}

/// Every other fix (but the blend corner End, which the fork has) does not reach the start
/// point: without WR-512 the round still fails.
#[test]
fn every_other_fix_still_refuses_the_arc_round() {
    let mut k = every_fix(false);
    let (_, r) = both_rounds(&mut k);
    assert_refused(r);
}

/// Every fix with the blend corner End: the arc round's end on the front round, where the
/// arc wall's top edge runs on tangentially into the front round's end arc on the wall, is
/// no End at a smooth vertex (tangency-labs/solidcore#2656: the end arc turns into the wall
/// away from the top edge continued, so the round run on leaves the part). It runs on as
/// before smlib#124: without the WR-512 line it is refused as the pinned C++ refuses it,
/// with it it builds the pinned C++'s body with that line, [`BOTH_ROUNDS`], to 1e-8 (the
/// fork's End, tangency-labs/smlib@18e96f1, caps it there instead: 9532.2987121741 with
/// `SmApiFilletEdges` and `SmApiBrepComputeVolume` at 1e-8, 0.99 more).
#[test]
fn the_blend_corner_runs_the_arc_round_on_as_the_pinned_with_wr512() {
    for wr512 in [false, true] {
        let mut fixes = UpstreamFixes::all();
        fixes.ssi_singular_start_inside_direction = wr512;
        let mut k = Kernel {
            fixes,
            ..Kernel::default()
        };
        let (mut b, r) = both_rounds(&mut k);
        if wr512 {
            assert_built_to(&mut k, &mut b, r, 1e-8, BOTH_ROUNDS);
        } else {
            assert_refused(r);
        }
    }
}

/// Every fix but the blend corner End: the other fixes build the rounds' faces their own way (the definition rounds of
/// `fillet_gn_section_generator` and others), so the volume is held to 1e-8, the integration's
/// accuracy. A round whose end came out a point, or a lost face, is off by far more: the arc
/// round removes 24.74 mm^3, 2.6e-3 of the part.
#[test]
fn every_fix_builds_the_arc_round() {
    let mut k = every_fix(true);
    let (mut b, r) = both_rounds(&mut k);
    assert_built(&mut k, &mut b, r, 1e-8);
}
