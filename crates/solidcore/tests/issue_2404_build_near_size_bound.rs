//! #2404 (carried over from tangency-labs/smlib#95; the generic blend, hub #2243): the bossed
//! can's top rim rounded just under the size at which its rail reaches the boss's footprint
//! builds. With keep features (WR-703) the boss's walls continued down were swept
//! in the sheet of fillet faces, and the sweep's own stitch (its second pass at 1.6e-3 here)
//! glued the round's rail to the footprint where the rail ran within that of it; the walls went
//! again (the band does not reach the boss) and took the rail with them: a bare
//! `SM_ERR` from `FindFilletBrepEdge`. The walls are now swept in a body of their own.
//!
//! The bridged block of the same issue has another cause: within a thousandth of its bound a
//! blend leaves a strip of the bridge whose height is about the trim's tolerance. Above
//! the tolerances of a sheet vertex and a body vertex together (5e-5) it builds; at or under
//! them it is refused by name, where it failed at the sewing or fell in two.
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
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_executive::NoRoomReason;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, PI};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::brep::p1::regions;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel};

/// Every fix on (SolidCraft's set).
fn kernel() -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes = UpstreamFixes::all();
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

/// The can rounded `r`: one solid of `faces` faces of the closed-form volume.
fn builds(r: f64, faces: usize) {
    let mut k = kernel();
    let (mut b, at) = bossed_can(&mut k);
    round(&mut k, &mut b, at, r).unwrap_or_else(|e| panic!("r {r}: {e:?}"));
    assert_one_sound_solid(&mut k, &mut b, faces, bossed_can_volume(r), 1e-8);
}

/// The fork's case: a hundred of the edge's tolerances (1e-5) under the bound, r = 4.299, the
/// round leaves a 0.001 wide annulus of top round the boss's footprint. Six faces: the can's
/// side and bottom, the round, the annulus, the boss's wall and top.
#[test]
fn a_round_a_thousandth_under_a_boss_footprint_builds() {
    builds(4.3 - 1e-3, 6);
}

/// Ten of the edge's tolerances under the bound: a 1e-4 wide annulus.
#[test]
fn a_round_a_ten_thousandth_under_a_boss_footprint_builds() {
    builds(4.3 - 1e-4, 6);
}

/// At the bound the round takes the whole top: its rail is the footprint, and it meets the
/// boss's wall there. Five faces.
#[test]
fn a_round_reaching_the_boss_footprint_takes_the_top() {
    builds(4.3, 5);
}

/// Farther under (the fork's "4.29 builds") the round built the same before and after.
#[test]
fn a_round_clear_of_the_boss_footprint_builds() {
    builds(4.29, 6);
}

/// A refusal's reason and fit.
type Refused = (Option<NoRoomReason>, f64);

/// The bridged block of `wr696_band_cuts.rs`, its top front edge blended `size` by a section
/// of `xsect` (1 a round, 0 a chamfer), with the fix set of [`kernel`] without the outline
/// (which ends a round before the strip): `Ok` built and stitched, else the reason and fit.
fn bridge(size: f64, xsect: u32) -> (Kernel, Brep, Result<(), Refused>) {
    let mut k = kernel();
    k.fixes.fillet_round_ends_on_outline = false;
    let block = create_box(&mut k, Point3::ZERO, 20.0, 4.0, 10.0).unwrap();
    let slot = create_box(&mut k, Point3::new(8.0, -1.0, -1.0), 4.0, 6.0, 10.0).unwrap();
    let mut b = boolean_difference(&mut k, block, slot).unwrap();
    let e = pick(&k, &b, Point3::new(2.0, 0.0, 10.0));
    let done = {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_constant_radius_edges(&mut k, &[e], size, xsect, 2, 0.0)
            .unwrap();
        s.do_fillet(&mut k).map_err(|_| {
            (
                s.error_info().no_room_reason(),
                s.fit_size().map_or(f64::NAN, |f| f.size),
            )
        })
    };
    (k, b, done)
}

/// The chamfer of setback `s` (4 to 5) of the bridged block: the plane y + (10 - z) = s over
/// the 16 of full section and the 4 of bridge (`Int_0^1 min(s - t, 4) dt` off the bridge's).
fn bridge_chamfer_volume(s: f64) -> f64 {
    let knee = s - 4.0;
    let full = s * s / 2.0 - knee * knee / 2.0;
    let bridge = 4.0 * knee + (4.0 + s - 1.0) / 2.0 * (1.0 - knee);
    656.0 - 16.0 * full - 4.0 * bridge
}

/// A strip of height 7e-5 (1e-4 under the bound of 5) stands: the closed form, one solid.
#[test]
fn a_chamfer_leaving_a_strip_the_part_resolves_builds() {
    let (mut k, mut b, done) = bridge(5.0 - 1e-4, 0);
    done.unwrap();
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    assert_eq!(lumps(&b), 1);
    let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert!(check.lines().is_empty(), "{:?}", check.lines());
    let (v, want) = (volume_of(&k, &mut b), bridge_chamfer_volume(5.0 - 1e-4));
    assert!((v - want).abs() <= 1e-9 * want, "{v} vs {want}");
}

/// Heights 4.9e-5, 3.5e-5, 2.1e-5 (7e-5, 5e-5, 3e-5 under the bound) failed at the sewing
/// ("does not sew", "reaches every face") or fell in two with a bad vertex gap. Each is
/// refused naming the size less the fit margin (a thousand edge tolerances of 1e-5), and the
/// body is put back; that size builds the closed form.
#[test]
fn a_chamfer_leaving_a_strip_below_resolution_is_refused_with_the_size_that_keeps_it() {
    for under in [7e-5, 5e-5, 3e-5] {
        let size = 5.0 - under;
        let (_, b, done) = bridge(size, 0);
        let (reason, fit) = done.expect_err("refused");
        assert_eq!(reason, Some(NoRoomReason::StripBelowResolution), "{under}");
        assert!((fit - (size - 0.01)).abs() <= 1e-9, "{under}: fit {fit}");
        assert_eq!(b.face_list().len(), 10, "{under}: the body put back");
        let (k, mut built, done) = bridge(fit, 0);
        done.unwrap();
        assert_eq!(lumps(&built), 1);
        let (v, want) = (volume_of(&k, &mut built), bridge_chamfer_volume(fit));
        assert!((v - want).abs() <= 1e-9 * want, "{v} vs {want}");
    }
}

/// The round at the sizes of the issue: 1.2e-4 and 1e-4 under its bound 5 + 2 sqrt 2 (strip
/// heights 4.3e-5, 3.6e-5) said only "does not sew"; 7e-5 under fell in two with a bad loop.
/// 1.5e-4 under (5.4e-5) builds, as before.
#[test]
fn a_round_keeps_the_strip_it_resolves_and_refuses_the_one_it_does_not() {
    let bound = 5.0 + 2.0 * 2.0_f64.sqrt();
    let (_, b, done) = bridge(bound - 1.5e-4, 1);
    done.unwrap();
    assert_eq!(lumps(&b), 1);
    for under in [1.2e-4, 1e-4, 7e-5] {
        let (_, _, done) = bridge(bound - under, 1);
        let (reason, _) = done.expect_err("refused");
        assert_eq!(reason, Some(NoRoomReason::StripBelowResolution), "{under}");
    }
}
