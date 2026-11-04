//! tangency-labs/solidcore#2176: an R2 corner arc of a plate's outline, whose both ends run on
//! tangentially into straight edges that are not picked, rounded r 0.5 alone.
//!
//! The plate is an R25 disc with a tab (sides x = +-4, R2 corners, top y = 280/9 + 2),
//! extruded 2. At each end of the arc three edges meet: the arc, the straight top edge that
//! continues it tangentially with its convexity, and the vertical seam where the corner's
//! cylinder runs tangent into the flat side. The round's rail on the top face (a circle of
//! radius 1.5) runs alongside the straight edge and never meets it. Pinned SMLib fails the
//! corner (`SM_FILERR_VERTEX_PROBLEM`, 1001); with `fillet_nx1_end_rib_global_merge` (WR-303) the
//! end rib is deleted for the global merge, which cannot end a round lying along the
//! continued edge, and `DoFilleting` leaves an open body (non-manifold, nothing removed),
//! which `fillet_edges` refuses with `fillet_refuse_non_manifold_result`.
//!
//! With `fillet_round_ends_at_tangent_continuation` (WR-674) the round ends at its section
//! through each vertex: the removed region is the round's section (1 - pi/4) r^2 about the
//! corner's axis over the quarter turn, closed by flat ends, so by Pappus it removes exactly
//! (1 - pi/4) r^2 (pi/2) (2 - c), c the section's centroid off the corner's wall.
//!
//! The fork's End at a smooth vertex (smlib#124, 41a39ec; WR-697) ends the round there
//! instead, on the plane square to its edge: the same body, with or without WR-674 (whose
//! path the End replaces). Its neighbours end the same way
//! where they run on tangentially into an edge not picked. Their figures are the fork's at
//! 18e96f1 (`SmApiFilletEdges` on the same plate, `SmApiBrepComputeVolume` at 1e-10).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{Brep, EdgeId};
use std::f64::consts::PI;

const RADIUS: f64 = 0.5;
/// A point on the R2 corner arc at x > 0.
const CORNER: [f64; 3] = [3.414_213_56, 32.525_324_7, 2.0];

/// Every opt-in fix on except `geometric_g2_continuity` (SolidCraft's set, as in the issue),
/// with WR-674 as given.
fn kernel(wr674: bool) -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity" {
            *f.flag_mut(name).unwrap() = true;
        }
    }
    f.fillet_round_ends_at_tangent_continuation = wr674;
    k.fixes = f;
    k
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

/// The volume to the fork's comparison accuracy (1e-10).
fn fine_volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-10, &attrs).unwrap()
}

/// The edge nearest `p`.
fn pick(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
    let q = Point3::new(p[0], p[1], p[2]);
    let mut best = (f64::INFINITY, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance < best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.unwrap()
}

fn line(a: (f64, f64), b: (f64, f64)) -> Curve {
    Curve::from(create_line_segment(Vec3::new(a.0, a.1, 0.0), Vec3::new(b.0, b.1, 0.0)).unwrap())
}

fn arc(c: (f64, f64), r: f64, from: f64, to: f64) -> Curve {
    Curve::from(
        create_arc(
            Vec3::new(c.0, c.1, 0.0),
            r,
            from.to_degrees(),
            to.to_degrees(),
        )
        .unwrap(),
    )
}

/// R25 disc with a tab (sides x = +-4, R2 corners, top y = 280/9 + 2), 2 thick.
fn plate(k: &mut Kernel) -> Brep {
    let t = (4.0_f64 / 25.0).acos();
    let y = (625.0_f64 - 16.0).sqrt();
    let cy = 280.0 / 9.0;
    let curves = [
        arc((0.0, 0.0), 25.0, PI - t - 2.0 * PI, t),
        line((4.0, y), (4.0, cy)),
        arc((2.0, cy), 2.0, 0.0, PI / 2.0),
        line((2.0, cy + 2.0), (-2.0, cy + 2.0)),
        arc((-2.0, cy), 2.0, PI / 2.0, PI),
        line((-4.0, cy), (-4.0, y)),
    ];
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 2.0, true).unwrap()
}

/// What the quarter-turn round with flat ends removes (Pappus).
fn pappus() -> f64 {
    let c = RADIUS * (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI));
    (1.0 - PI / 4.0) * RADIUS * RADIUS * (PI / 2.0) * (2.0 - c)
}

#[test]
fn the_corner_arc_rounds_alone_with_flat_ends() {
    let mut k = kernel(true);
    let mut b = plate(&mut k);
    let before = volume(&k, &mut b);
    let e = pick(&k, &b, CORNER);
    fillet_edges(&mut k, &mut b, &[e], RADIUS, 1, 1, 1.0).unwrap();
    b.refresh_cached_topology();
    assert!(is_manifold_solid(&b).unwrap());
    let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert!(check.is_valid(), "{:?}", check.lines());
    // the plate's 8 faces, the round and its two flat ends
    assert_eq!(b.face_list().len(), 11);
    let removed = before - volume(&k, &mut b);
    assert!(
        (removed - pappus()).abs() <= 1e-7,
        "removed {removed}, Pappus {}",
        pappus()
    );
}

/// `DoFilleting` alone (as SolidCraft drives the executive) builds the same closed body.
#[test]
fn do_filleting_builds_the_closed_body() {
    let mut k = kernel(true);
    let mut b = plate(&mut k);
    let before = volume(&k, &mut b);
    let e = pick(&k, &b, CORNER);
    {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_per_edge_constant_radius(
            &mut k,
            &[e],
            &[RADIUS],
            &[FilletSurfaceGeneratorType::Circular],
            1,
            1.0,
        )
        .unwrap();
        s.create_corners(&k, None).unwrap();
        s.do_filleting(&mut k, None).unwrap();
    }
    b.refresh_cached_topology();
    assert!(is_manifold_solid(&b).unwrap());
    let removed = before - volume(&k, &mut b);
    assert!((removed - pappus()).abs() <= 1e-7, "removed {removed}");
}

/// Without WR-674 the blend corner's End at a smooth vertex (the fork's 41a39ec) builds the
/// round with flat ends: the fork's 0.159135839575356 (Pappus to 1.5e-10).
#[test]
fn the_blend_corner_ends_the_round_at_the_smooth_vertices() {
    let mut k = kernel(false);
    let mut b = plate(&mut k);
    let before = fine_volume(&k, &mut b);
    let e = pick(&k, &b, CORNER);
    fillet_edges(&mut k, &mut b, &[e], RADIUS, 1, 1, 1.0).unwrap();
    b.refresh_cached_topology();
    assert!(is_manifold_solid(&b).unwrap());
    assert_eq!(b.face_list().len(), 11);
    let removed = before - fine_volume(&k, &mut b);
    assert!(
        (removed - 0.159_135_839_575_356).abs() <= 1e-9,
        "removed {removed}"
    );
}

/// The straight edges next to the corner and the rim, each rounded alone. The tab's side
/// and its top run on tangentially into the corner arcs, which are not picked. The fork
/// (41a39ec) ends each there at the smooth vertex, on the plane square to it. With
/// tangency-labs/solidcore#2656 each runs on past the vertex, the
/// arc turning into the top face away from it, until it leaves the part: past each such end
/// the round's section, cut by the corner arc's wall `R - sqrt(R^2 - s^2)` in from the
/// straight side at `s` past the vertex (R = 2), takes off `RUN_ON` more
/// (`int_0^sqrt(R^2 - (R - r)^2) A(r - R + sqrt(R^2 - s^2)) ds`, `A(t) = r t - t sqrt(r^2 -
/// t^2) / 2 - r^2 asin(t / r) / 2`, Simpson): the fork's figures plus `RUN_ON` once for the
/// tab's side and twice for its top. The tab side's other end and the rim's ends are
/// mixed-convexity Ends (the fork's 3e26f76). The fork's figures are at 18e96f1 on the same
/// plate (this test's fixes add WR-682's plane curve, among others: solidcore's are within 0
/// and 4.5e-11 of them for the tab's top and the rim), except the tab side's. The fork's tab
/// side round removes 0.344196265039045 and leaves its End rib's UV curve off its 3D curve
/// (`check_brep` `Bad Gap`). Here `FlushCurve` keeps the short last span (WR-697 addendum,
/// end-rib flush merge, solidcore's own), the body is sound and the capped round removes
/// 0.344196254843155, 1.0e-8 less. Each body is checked sound (`check_brep` Level2).
#[test]
fn the_neighbours_keep_their_rounds() {
    // the run-on past one end; where it leaves the part the round's face is cut by the
    // arc's wall along their traced intersection, within 1.5e-6 of the closed form here
    // (measured 1.4e-7 and 1.5e-6)
    const RUN_ON: f64 = 0.031_715_918_945_862;
    for (at, removed_by_fork, tol) in [
        ([4.0, 28.0, 2.0], 0.344_196_254_843_155 + RUN_ON, 2e-6),
        (
            [0.0, 33.111_111_1, 2.0],
            0.214_601_836_334_623 + 2.0 * RUN_ON,
            2e-6,
        ),
        ([0.0, -25.0, 2.0], 7.958_672_926_831_696, 1e-10),
    ] {
        let mut k = kernel(true);
        let mut b = plate(&mut k);
        let before = fine_volume(&k, &mut b);
        let e = pick(&k, &b, at);
        fillet_edges(&mut k, &mut b, &[e], RADIUS, 1, 1, 1.0).unwrap();
        b.refresh_cached_topology();
        let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
        assert!(check.is_valid(), "{at:?}: {:?}", check.lines());
        let removed = before - fine_volume(&k, &mut b);
        assert!(
            (removed - removed_by_fork).abs() <= tol,
            "{at:?}: {removed}"
        );
    }
}
