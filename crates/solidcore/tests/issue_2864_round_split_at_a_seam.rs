//! tangency-labs/solidcore#2864: a round whose picked edge is split by a seam.
//!
//! The rig case `fillets/every-block-cylinder-tangent-concave-arc-r2-partial-chain`: a block
//! x 0..40, y -10..10, z 0..15 joined to a cylinder R 10, z 0..30, about the z axis. The block's
//! sides y = +-10 run tangent into the cylinder's wall at x = 0, so its top meets the wall in
//! the concave half circle z = 15, rho = 10, x > 0, which runs on tangentially into the block's
//! top side edges at (0, +-10, 15). The cylinder's seam splits that half circle into two edges.
//!
//! Rounded r 2 on both pieces, the round is the torus about the z axis (centre circle rho 12,
//! z 17) over the block, cut by its side planes |y| = 10: at radius rho in [10, 12] it adds the
//! spandrel height 2 - sqrt(4 - (rho - 12)^2) over the arc of length 2 rho asin(10 / rho), so
//! V = 12000 + 3000 pi - 750 pi + 23.378532906443 = 19091.962003483477 (the case's `method`).
//!
//! Rounded on one piece only, the round would end at the seam's vertex, where its edge runs on
//! into the rest of the same curve between the same two faces. A round never ends there with
//! a step (#2851): until it runs on across the seam's vertex, the blend refuses and says why.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

/// The refusal's words: the round's edge runs on across a seam's vertex.
const ACROSS_SEAM: &str = "across a seam's vertex";

/// Every fix but `geometric_g2_continuity`, as the blend's other End tests.
fn kernel() -> Kernel {
    let mut k = Kernel::default();
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity" {
            *f.flag_mut(name).unwrap() = true;
        }
    }
    k.fixes = f;
    k
}

/// The block joined to the cylinder.
fn part(k: &mut Kernel) -> Brep {
    let block = create_box(k, Point3::new(0.0, -10.0, 0.0), 40.0, 20.0, 15.0).unwrap();
    let cylinder = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 10.0, 30.0).unwrap();
    boolean_union(k, block, cylinder).unwrap()
}

/// The half circle's pieces: edges on z = 15 at radius 10 whose middle has x > 0.
fn half_circle(k: &Kernel, b: &Brep) -> Vec<EdgeId> {
    let mut out = Vec::new();
    for &e in b.edge_list() {
        let Some(c) = edge::curve(b, e).unwrap() else {
            continue;
        };
        let iv = edge::interval(b, e).unwrap();
        let ps = [iv.min(), 0.5 * (iv.min() + iv.max()), iv.max()]
            .map(|t| c.evaluate_point(k, t).unwrap());
        let on = ps
            .iter()
            .all(|p| (p.z - 15.0).abs() < 1e-9 && (p.x.hypot(p.y) - 10.0).abs() < 1e-9);
        if on && ps[1].x > 1e-6 {
            out.push(e);
        }
    }
    out
}

/// `edges` rounded r 2 (a circular round), then the body stitched; else the refusal and its
/// comment.
fn round(k: &mut Kernel, b: &mut Brep, edges: &[EdgeId]) -> Result<(), String> {
    {
        let mut s = FilletSession::new(k, b).unwrap();
        s.prepare_per_edge_constant_radius(
            k,
            edges,
            &vec![2.0; edges.len()],
            &vec![FilletSurfaceGeneratorType::Circular; edges.len()],
            1,
            1.0,
        )
        .unwrap();
        if let Err(err) = s.do_fillet(k) {
            return Err(format!("{err:?}: {}", s.error_info().comment));
        }
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default()).unwrap();
    Ok(())
}

/// `SmApiBrepComputeVolume` at 1e-12.
fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-12, &attrs).unwrap()
}

/// The part before the round: block and cylinder less their overlap, a half disc 15 high.
fn part_volume() -> f64 {
    12000.0 + 3000.0 * std::f64::consts::PI - 750.0 * std::f64::consts::PI
}

/// The part is the closed form, and the seam splits its half circle in two.
#[test]
fn the_part_is_the_closed_form() {
    let mut k = kernel();
    let mut b = part(&mut k);
    let got = volume(&k, &mut b);
    assert!((got - part_volume()).abs() <= 1e-9 * got, "{got}");
    assert_eq!(half_circle(&k, &b).len(), 2);
}

/// One piece rounded alone: its edge runs on across the seam's vertex into the other piece,
/// the same circle between the same two faces. The round must not end there with a step;
/// it is refused, saying why, whichever piece is picked.
#[test]
fn one_piece_alone_is_refused_at_the_seam() {
    let mut k = kernel();
    for which in 0..2 {
        let mut b = part(&mut k);
        let pieces = half_circle(&k, &b);
        assert_eq!(pieces.len(), 2);
        let err = round(&mut k, &mut b, &pieces[which..=which]).unwrap_err();
        assert!(err.contains(ACROSS_SEAM), "{err}");
    }
}

/// Both pieces rounded: the torus over the block, 19091.962003483477 (the case's `method`),
/// valid at level 2.
#[test]
fn both_pieces_round_the_half_circle() {
    let mut k = kernel();
    let mut b = part(&mut k);
    let pieces = half_circle(&k, &b);
    round(&mut k, &mut b, &pieces).unwrap();
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    let got = volume(&k, &mut b);
    assert!((got - 19_091.962_003_483_477).abs() <= 1e-9 * got, "{got}");
}
