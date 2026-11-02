//! WR-696 (#2273, step 5 of the generic blend; the fork's tangency-labs/smlib#79, 862971f,
//! e568e45 and aac4288, merged at 5e8c9c1): with `fixes.fillet_gn_section_generator` a
//! convex blend that would take all of the part between its rails somewhere along its edge
//! (a round or chamfer through the bridge over a slot) is refused before anything is built
//! (`SmFilletSolver::BandCutsThePart`), naming the largest size that keeps the part whole and
//! noting it, less the fit margin, as the fit size ([`FilletSession::fit_size`]).
//!
//! Values: closed forms from the fork's capi tests (`ops_kernel.cpp` at 5e8c9c1,
//! `scenario_round_cutting_a_bridge_refused`, `scenario_chamfer_cutting_a_bridge_refused`),
//! and the fork's own numbers from a local build of 5e8c9c1's `SMLib` and `SM_API` run the
//! same way (`SetFilletParameters`, `CreateFilletCorners`, `DoFilleting`, `GetFitRadius`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_executive::{FitKind, NoRoomReason};
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::kernel::UpstreamFixes;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel};

fn kernel(gn: bool) -> Kernel {
    let mut k = Kernel::default();
    let mut fixes = UpstreamFixes::all();
    fixes.fillet_gn_section_generator = gn;
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
    let mut k = kernel(true);
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
    let mut k = kernel(true);
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

/// Without the flag the cut is not judged: no refusal names it, and no fit is noted.
#[test]
fn without_the_flag_the_cut_is_not_judged() {
    let mut k = kernel(false);
    let mut b = bridged_block(&mut k);
    if let Err(refusal) = blend(&mut k, &mut b, Point3::new(2.0, 0.0, 10.0), 9.0, 1) {
        assert!(refusal.fit.is_none());
        assert_ne!(refusal.reason, Some(NoRoomReason::CutsThePart));
        assert!(
            !refusal.comment.contains("cut it into pieces"),
            "{}",
            refusal.comment
        );
    }
}
