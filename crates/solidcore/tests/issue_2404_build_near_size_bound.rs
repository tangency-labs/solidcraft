//! #2404 (carried over from tangency-labs/smlib#95; the generic blend, hub #2243): the bossed
//! can's top rim rounded just under the size at which its rail reaches the boss's footprint
//! builds. With `fixes.fillet_keep_features` (WR-703) the boss's walls continued down were swept
//! in the sheet of fillet faces, and the sweep's own stitch (its second pass at 1.6e-3 here)
//! glued the round's rail to the footprint where the rail ran within that of it; the walls went
//! again (the band does not reach the boss) and took the rail with them: a bare
//! `SM_ERR` from `FindFilletBrepEdge`. The walls are now swept in a body of their own.
//!
//! The bridged block of the same issue (the strip a round leaves on a bridge 1e-4 under its
//! bound, lost in the trim at the creases) has another cause and is not fixed here.
//!
//! Inputs and closed forms: the fork's capi test `scenario_round_just_under_a_boss_footprint`
//! (`ops_kernel.cpp` at 5e8c9c1, held there as known failing: "status 1001 with no reason").

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, PI};
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::brep::p1::regions;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel};

/// Every fix on (SolidCraft's set), `fillet_keep_features` as given.
fn kernel(keep: bool) -> Kernel {
    let mut k = Kernel::default();
    let mut fixes = UpstreamFixes::all();
    fixes.fillet_keep_features = keep;
    k.fixes = fixes;
    k
}

fn pick(k: &Kernel, b: &Brep, q: Point3) -> EdgeId {
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

fn volume_of(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-7, &attrs).unwrap()
}

fn lumps(b: &Brep) -> usize {
    let infinite = b.infinite_region();
    regions(b, None)
        .unwrap()
        .into_iter()
        .filter(|r| Some(*r) != infinite)
        .count()
}

/// The bossed can: a cylinder r7 h48.2 with a boss r2.7, 2 tall, on its top, and a point on
/// its top rim. Its top rim's round of radius r leaves the top an annulus between the boss's
/// footprint (2.7) and its rail (7 - r): the rail reaches the footprint at r = 4.3.
fn bossed_can(k: &mut Kernel) -> (Brep, Point3) {
    let can = create_cylinder(k, Point3::ZERO, 7.0, 48.2).unwrap();
    let boss = create_cylinder(k, Point3::new(0.0, 0.0, 48.2), 2.7, 2.0).unwrap();
    let at = 100.0_f64.to_radians();
    (
        boolean_union(k, can, boss).unwrap(),
        Point3::new(7.0 * at.cos(), 7.0 * at.sin(), 48.2),
    )
}

/// The bossed can's volume after the round r: the round takes (1 - pi/4) r^2 about the axis
/// at its section's centroid, 7 - r (10 - 3 pi) / (12 - 3 pi) out (the fork's capi test).
fn bossed_can_volume(r: f64) -> f64 {
    let off =
        2.0 * PI * (7.0 - r * (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI)) * (1.0 - PI / 4.0) * r * r;
    PI * 49.0 * 48.2 + PI * 2.7 * 2.7 * 2.0 - off
}

/// Rounds `at`'s edge `r` through the public API (`SmApiFilletEdges`' steps, then stitched).
fn round(k: &mut Kernel, b: &mut Brep, at: Point3, r: f64) -> solidcore::Result<()> {
    let e = pick(k, b, at);
    fillet_edges(k, b, &[e], r, 1, 2, 0.0)
}

/// One solid of `faces` faces, `check_brep` clean at level 2, its volume `want` within `rel`.
fn assert_one_sound_solid(k: &mut Kernel, b: &mut Brep, faces: usize, want: f64, rel: f64) {
    assert_eq!(lumps(b), 1, "one lump");
    assert_eq!(b.face_list().len(), faces, "faces");
    let check = check_brep(k, b, CheckLevel::Level2).unwrap();
    assert!(check.lines().is_empty(), "check_brep: {:?}", check.lines());
    let v = volume_of(k, b);
    assert!(
        (v - want).abs() <= rel * want,
        "volume {v:.12}, want {want:.12}"
    );
}

/// The can rounded `r` with `keep`: one solid of `faces` faces of the closed-form volume.
fn builds(keep: bool, r: f64, faces: usize) {
    let mut k = kernel(keep);
    let (mut b, at) = bossed_can(&mut k);
    round(&mut k, &mut b, at, r).unwrap_or_else(|e| panic!("r {r}: {e:?}"));
    assert_one_sound_solid(&mut k, &mut b, faces, bossed_can_volume(r), 1e-8);
}

/// The fork's case: a hundred of the edge's tolerances (1e-5) under the bound, r = 4.299, the
/// round leaves a 0.001 wide annulus of top round the boss's footprint. Six faces: the can's
/// side and bottom, the round, the annulus, the boss's wall and top.
#[test]
fn a_round_a_thousandth_under_a_boss_footprint_builds() {
    builds(true, 4.3 - 1e-3, 6);
}

/// Ten of the edge's tolerances under the bound: a 1e-4 wide annulus.
#[test]
fn a_round_a_ten_thousandth_under_a_boss_footprint_builds() {
    builds(true, 4.3 - 1e-4, 6);
}

/// At the bound the round takes the whole top: its rail is the footprint, and it meets the
/// boss's wall there. Five faces.
#[test]
fn a_round_reaching_the_boss_footprint_takes_the_top() {
    builds(true, 4.3, 5);
}

/// Farther under (the fork's "4.29 builds") the round built the same before and after.
#[test]
fn a_round_clear_of_the_boss_footprint_builds() {
    builds(true, 4.29, 6);
}

/// Without keep features no wall is swept: the same rounds build (the pinned path).
#[test]
fn without_keep_features_the_rounds_build_the_same() {
    builds(false, 4.3 - 1e-3, 6);
    builds(false, 4.3 - 1e-4, 6);
    builds(false, 4.3, 5);
}
