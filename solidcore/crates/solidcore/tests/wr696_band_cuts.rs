//! WR-696 (#2273, step 5 of the generic blend; the fork's tangency-labs/smlib#79, 862971f,
//! e568e45 and aac4288, merged at 5e8c9c1): a convex blend that would take all of the part between its rails somewhere along its edge
//! (a round or chamfer through the bridge over a slot) is refused before anything is built
//! (`SmFilletSolver::BandCutsThePart`), naming the largest size that keeps the part whole and
//! noting it, less the fit margin, as the fit size ([`FilletSession::fit_size`]).
//!
//! Values: closed forms from the fork's capi tests (`ops_kernel.cpp` at 5e8c9c1,
//! `scenario_round_cutting_a_bridge_refused`, `scenario_chamfer_cutting_a_bridge_refused`),
//! and the fork's own numbers from a local build of 5e8c9c1's `SMLib` and `SM_API` run the
//! same way (`SetFilletParameters`, `CreateFilletCorners`, `DoFilleting`, `GetFitRadius`).
//!
//! A smaller round or chamfer past the top's back rim, whose front rail crosses the slot's
//! opening, leaves the bridge a strip and builds one piece: closed forms from the fork's
//! c1905d0 (`scenario_round_past_the_rim_over_an_opening`,
//! `scenario_chamfer_past_the_rim_over_an_opening`), to the fork's tolerances.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_executive::{FitKind, NoRoomReason};
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::math::Vec3;
use solidcore::primitive::api_curves as curves;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p1::regions;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel};

fn kernel() -> Kernel {
    Kernel {
        fixes: UpstreamFixes::all(),
        ..Kernel::default()
    }
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

struct Refusal {
    comment: String,
    reason: Option<NoRoomReason>,
    fit: Option<(f64, FitKind, bool)>,
}

/// `xsect`: 1 a round, 0 a chamfer (a linear section); `r` the ball's radius (a chamfer's
/// setback on this square edge).
fn blend(k: &mut Kernel, b: &mut Brep, at: Point3, r: f64, xsect: u32) -> Result<(), Refusal> {
    let e = pick(k, b, at);
    let mut session = FilletSession::new(k, b).unwrap();
    session
        .prepare_constant_radius_edges(k, &[e], r, xsect, 2, 0.0)
        .unwrap();
    session.do_fillet(k).map_err(|_| {
        let info = session.error_info();
        Refusal {
            comment: info.comment.clone(),
            reason: info.no_room_reason(),
            fit: session
                .fit_size()
                .map(|f| (f.size, f.kind, f.edge == Some(e))),
        }
    })
}

/// A 20 x 4 x 10 block with a slot 4 wide (x 8 to 12) cut through it from below to 1 under
/// its top: two pillars joined by a bridge 1 thick. Its top front edge (y = 0, z = 10) is
/// blended.
fn bridged_block(k: &mut Kernel) -> Brep {
    let block = create_box(k, Point3::ZERO, 20.0, 4.0, 10.0).unwrap();
    let slot = create_box(k, Point3::new(8.0, -1.0, -1.0), 4.0, 6.0, 10.0).unwrap();
    boolean_difference(k, block, slot).unwrap()
}

/// Rounded r9 the round takes all of the bridge: refused naming where
/// `d(9) = r - sqrt(2 r - 1) = 4`, `r = 5 + 2 sqrt 2`; the fit is that less a thousand
/// edge tolerances of 1e-5. The fork's build: "a radius below 7.828427 keeps it whole",
/// fit 7.8184271464117412 (2.2e-8 over the closed form's).
#[test]
fn a_round_through_a_bridge_is_refused_naming_the_radius_that_keeps_it_whole() {
    let mut k = kernel();
    // the round on the continued wall; with `fillet_round_ends_on_outline` it ends on the
    // body's outline (`wr730_round_ends_on_outline.rs`)
    k.fixes.fillet_round_ends_on_outline = false;
    let mut b = bridged_block(&mut k);
    let refusal = blend(&mut k, &mut b, Point3::new(2.0, 0.0, 10.0), 9.0, 1).expect_err("refused");
    eprintln!("{} fit {:?}", refusal.comment, refusal.fit);
    assert_eq!(refusal.reason, Some(NoRoomReason::CutsThePart));
    assert_eq!(
        refusal.comment,
        "Fillet Error: the round would take all of the part between its rails along the edge and cut it into pieces: a radius below 7.828427 keeps it whole"
    );
    let (fit, kind, on_edge) = refusal.fit.expect("a fit");
    assert_eq!(kind, FitKind::Radius);
    assert!(on_edge);
    let closed = 5.0 + 2.0 * 2.0_f64.sqrt() - 0.01;
    // the bound is judged at 32 sections and found by 40 bisections: within 1e-8 of it
    assert!(
        (fit - closed).abs() <= 1e-8 * closed,
        "{fit} vs closed form {closed}"
    );
    assert!(
        (fit - 7.8184271464117412).abs() <= 1e-12 * fit,
        "{fit} vs fork"
    );
}

/// Chamfered 6 the plane takes all of the bridge: refused naming the setback 5 at which it
/// reaches the bridge's underside. The fork's build: "a chamfer set back less than 5 keeps it
/// whole", fit 4.9900000000036382.
#[test]
fn a_chamfer_through_a_bridge_is_refused_naming_the_setback_that_keeps_it_whole() {
    let mut k = kernel();
    let mut b = bridged_block(&mut k);
    let refusal = blend(&mut k, &mut b, Point3::new(2.0, 0.0, 10.0), 6.0, 0).expect_err("refused");
    eprintln!("{} fit {:?}", refusal.comment, refusal.fit);
    assert_eq!(refusal.reason, Some(NoRoomReason::CutsThePart));
    assert_eq!(
        refusal.comment,
        "Fillet Error: the chamfer would take all of the part between its sides along the edge and cut it into pieces: a chamfer set back less than 5 keeps it whole"
    );
    let (fit, kind, on_edge) = refusal.fit.expect("a fit");
    assert_eq!(kind, FitKind::Radius);
    assert!(on_edge);
    assert!(
        (fit - 4.99).abs() <= 1e-9 * 5.0,
        "{fit} vs closed form 5 less the margin"
    );
    assert!(
        (fit - 4.9900000000036382).abs() <= 1e-12 * fit,
        "{fit} vs fork"
    );
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-7, &attrs).unwrap()
}

/// Blends the bridged block's top front edge by `r` (`xsect` as [`blend`]): built, a
/// manifold solid in one piece (the outside and one solid region), its volume.
fn built_volume(r: f64, xsect: u32) -> f64 {
    let mut k = kernel();
    // the round on the continued wall; with `fillet_round_ends_on_outline` it ends on the
    // body's outline (`wr730_round_ends_on_outline.rs`)
    k.fixes.fillet_round_ends_on_outline = false;
    let mut b = bridged_block(&mut k);
    if let Err(refusal) = blend(&mut k, &mut b, Point3::new(2.0, 0.0, 10.0), r, xsect) {
        panic!("r {r} refused: {}", refusal.comment);
    }
    assert!(
        is_manifold_solid(&b).unwrap(),
        "r {r}: not a manifold solid"
    );
    assert_eq!(regions(&b, None).unwrap().len(), 2, "r {r}: not one piece");
    volume(&k, &mut b)
}

/// Rounded r6, wider than the 4 deep top: its contact line on the top lies past the back
/// and the one on the front, at z = 4, crosses the slot's opening; the bridge keeps a strip.
/// The round takes `y < d(z) = 6 - sqrt(36 - (z - 4)^2)`, at most the depth 4 (reached at
/// `z = 4 + sqrt 32`): with `G(t) = 6 t - (t/2) sqrt(36 - t^2) - 18 asin(t/6)` the full
/// section loses `G(sqrt 32) + 4 (6 - sqrt 32)` and the bridge's that less `G(5)` (its
/// z < 9 is the slot), over 16 and 4 of the length (fork c1905d0,
/// `scenario_round_past_the_rim_over_an_opening`, to 1e-7).
#[test]
fn a_round_past_the_rim_over_the_opening_keeps_the_bridge() {
    let g = |t: f64| 6.0 * t - 0.5 * t * (36.0 - t * t).sqrt() - 18.0 * (t / 6.0).asin();
    let q = 32.0_f64.sqrt();
    let full = g(q) + 4.0 * (6.0 - q);
    let bridge = full - g(5.0);
    let closed = 656.0 - (16.0 * full + 4.0 * bridge);
    let v = built_volume(6.0, 1);
    eprintln!("round r6: {v:.12} vs {closed:.12}");
    assert!(
        (v - closed).abs() <= 1e-7 * closed,
        "{v} vs closed form {closed}"
    );
}

/// Chamfered 4.5, past the 4 deep top too (a chamfer past a rim builds): the plane
/// `y + (10 - z) = 4.5` takes the triangle of legs 4.5 less its 0.5 past the back, 10, off
/// the full section, and `Int_0^1 min(4.5 - t, 4) dt = 3.875` off the bridge's:
/// `656 - 160 - 15.5 = 480.5` (fork c1905d0, `scenario_chamfer_past_the_rim_over_an_opening`,
/// to 1e-9).
#[test]
fn a_chamfer_past_the_rim_over_the_opening_keeps_the_bridge() {
    let v = built_volume(4.5, 0);
    eprintln!("chamfer 4.5: {v:.12} vs 480.5");
    assert!(
        (v - 480.5).abs() <= 1e-9 * 480.5,
        "{v} vs closed form 480.5"
    );
}

/// A sheet: two walls, the open profile (0, 0) -> (10, 0) -> (10, 10) swept 10 up without
/// caps, their corner edge x = 10, y = 0 rounded r 1 on the default kernel. A sheet's edges
/// bound no material, so there is no part for the band to cut, and the round builds. Before,
/// `sm_BlendAddsMaterial` found no use of the edge on a material side and failed
/// (`SM_ERR` from `IsConcaveEdge`), and the fillet with it (the goldens' `stubs4-*` rounds of
/// upstream's open sheets, `test8.smb`, `shell_fillet.smb` and others).
#[test]
fn a_round_on_a_sheet_has_no_part_to_cut() {
    let mut k = Kernel::default();
    let p = Point3::new;
    let profile = [
        Curve::from(curves::create_line_segment(p(0.0, 0.0, 0.0), p(10.0, 0.0, 0.0)).unwrap()),
        Curve::from(curves::create_line_segment(p(10.0, 0.0, 0.0), p(10.0, 10.0, 0.0)).unwrap()),
    ];
    let refs: Vec<Option<&Curve>> = profile.iter().map(Some).collect();
    let mut b = create_linear_sweep(&mut k, &refs, Vec3::new(0.0, 0.0, 1.0), 10.0, false).unwrap();
    let e = pick(&k, &b, p(10.0, 0.0, 5.0));
    let mut session = FilletSession::new(&mut k, &mut b).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &[e], 1.0, 1, 1, 1.0)
        .unwrap();
    let done = session.do_fillet(&mut k);
    assert!(done.is_ok(), "{done:?}: {}", session.error_info().comment);
}
