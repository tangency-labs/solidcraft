//! The cliff overflow trimmed at its creases in place (WR-695, tangency-labs/smlib#79
//! step 6, `UpstreamFixes::fillet_overflow_trim_at_creases`).
//!
//! Each case is graded against the closed form of the section the round removes
//! ([`removed_area`] swept along the edge), the face count and `AssertValid` at
//! [`CheckLevel::Level2`]. One case, the round trimmed at a hole, has no closed form here
//! (its crease runs on the hole's cylinder through a general surface intersection): that one
//! keeps the fork's build as a cross-check, at the tolerance it had.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)]

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_standard_solver::p1::FilletLaw;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, vertex, EdgeId};
use solidcore::{Brep, Kernel};

/// The cliff overflow's flags (as `wr631_cliff_overflow_construction`), the trim at the
/// creases on (`trim`) or off (the subtraction).
fn kernel(trim: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_trace_reaches_rails = true;
    k.fixes.fillet_overflow_trim_at_creases = trim;
    k
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-10, &attrs).unwrap()
}

/// The area of the section a round of radius `r` removes between faces `w` and `h` wide
/// (perpendicular), in closed form.
fn removed_area(r: f64, w: f64, h: f64) -> f64 {
    let a = w.min(r);
    let f = |s: f64| s / 2.0 * (r * r - s * s).max(0.0).sqrt() + r * r / 2.0 * (s / r).asin();
    let u_h = if h >= r {
        0.0
    } else {
        r - (r * r - (r - h) * (r - h)).sqrt()
    };
    let mut area = if h < r { h * u_h.min(a) } else { 0.0 };
    if a > u_h {
        area += r * (a - u_h) - (f(r - u_h) - f(r - a));
    }
    area
}

/// The edge from `p` to `q` (either way).
fn edge_between(k: &Kernel, b: &Brep, p: Point3, q: Point3) -> EdgeId {
    b.edges()
        .ids()
        .find(|&e| {
            let ivl = edge::interval(b, e).unwrap();
            let c = edge::curve(b, e).unwrap().unwrap();
            let s = c.evaluate_point(k, ivl.min()).unwrap();
            let t = c.evaluate_point(k, ivl.max()).unwrap();
            (s.distance(p) < 1e-9 && t.distance(q) < 1e-9)
                || (s.distance(q) < 1e-9 && t.distance(p) < 1e-9)
        })
        .expect("edge")
}

/// The outcome of a blend: the error code and comment of a refusal, or the volume and
/// face count of the result (a manifold solid).
#[derive(Debug)]
enum Outcome {
    Built { volume: f64, faces: usize },
    Refused(i32, String),
}

/// Rounds (`chamfer` false: circular section; true: linear) the edge from `p` to `q` of
/// the body `make` builds, by `r`.
fn blend(
    trim: bool,
    make: fn(&mut Kernel) -> Brep,
    p: Point3,
    q: Point3,
    r: f64,
    chamfer: bool,
) -> Outcome {
    let mut k = kernel(trim);
    let mut b = make(&mut k);
    let e = edge_between(&k, &b, p, q);
    let mut session = FilletSession::new(&mut k, &mut b).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &[e], r, u32::from(!chamfer), 1, 1.0)
        .unwrap();
    let result = session.do_fillet(&mut k);
    let info = session.error_info().clone();
    drop(session);
    match result {
        Ok(()) => {
            assert!(
                is_manifold_solid(&b).unwrap(),
                "r {r}: not a manifold solid"
            );
            let built = Outcome::Built {
                volume: volume(&k, &mut b),
                faces: b.faces().ids().count(),
            };
            b.use_arena_naming().unwrap();
            let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
                .unwrap()
                .lines();
            assert!(lines.is_empty(), "r {r}: check_brep: {lines:?}");
            built
        }
        Err(_) => Outcome::Refused(info.code, info.comment),
    }
}

/// Asserts a build of `faces` faces whose volume is `expected` within `rel` relative.
fn assert_builds(outcome: &Outcome, expected: f64, rel: f64, faces: usize) {
    let (v, n) = match outcome {
        Outcome::Built { volume, faces } => (volume, faces),
        Outcome::Refused(code, comment) => {
            panic!("expected a build at {expected}, refused (code {code}): {comment}")
        }
    };
    assert!(
        (v - expected).abs() <= rel * expected,
        "volume {v} against {expected} (rel {})",
        (v - expected).abs() / expected
    );
    assert_eq!(*n, faces, "face count of {outcome:?}");
}

fn p(x: f64, y: f64, z: f64) -> Point3 {
    Point3::new(x, y, z)
}

/// The stepped bar of `wr631_cliff_overflow_choice`: its profile
/// `(0,0) (10,0) (10,5) (7,5) (7,10) (0,10)` swept 20 along z.
fn stepped_bar(k: &mut Kernel) -> Brep {
    const PROFILE: [[f64; 2]; 6] = [
        [0.0, 0.0],
        [10.0, 0.0],
        [10.0, 5.0],
        [7.0, 5.0],
        [7.0, 10.0],
        [0.0, 10.0],
    ];
    let cs: Vec<Curve> = PROFILE
        .iter()
        .zip(PROFILE.iter().cycle().skip(1))
        .map(|(a, b)| {
            Curve::from(
                create_line_segment(Vec3::new(a[0], a[1], 0.0), Vec3::new(b[0], b[1], 0.0))
                    .unwrap(),
            )
        })
        .collect();
    let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 20.0, true).unwrap()
}

/// An 80 x 5 x 5 bar (#1466).
fn bar(k: &mut Kernel) -> Brep {
    create_box(k, Point3::ZERO, 80.0, 5.0, 5.0).unwrap()
}

/// A 40 x 20 x 10 box (#1467).
fn box_40_20_10(k: &mut Kernel) -> Brep {
    create_box(k, Point3::ZERO, 40.0, 20.0, 10.0).unwrap()
}

/// A 40 x 20 x 10 box drilled through by a radius 3 hole at (20, 14.5).
fn block_with_hole(k: &mut Kernel) -> Brep {
    let block = create_box(k, Point3::ZERO, 40.0, 20.0, 10.0).unwrap();
    let drill = create_cylinder(k, p(20.0, 14.5, -1.0), 3.0, 12.0).unwrap();
    boolean_difference(k, block, drill).unwrap()
}

/// A 10 x 20 x 30 box.
fn box_10_20_30(k: &mut Kernel) -> Brep {
    create_box(k, Point3::ZERO, 10.0, 20.0, 30.0).unwrap()
}

/// A 20 cube.
fn cube_20(k: &mut Kernel) -> Brep {
    create_box(k, Point3::ZERO, 20.0, 20.0, 20.0).unwrap()
}

/// The relative volume tolerance against the closed form. [`volume`] integrates at 1e-10,
/// whose error (WR-472's scale: the accuracy times the body's own size) is below it; the
/// six bodies reached 1.8e-10 relative at worst (the bar's round, measured on 2026-11-04).
const CLOSED_REL: f64 = 1e-9;

/// The relative tolerance of the one case with no closed form here, the round trimmed at a
/// hole, against the fork's build: as before, and solidcore sits 1.8e-10 from it.
const FORK_REL: f64 = 1e-8;

// The closed forms below are `removed_area` swept along the edge. The one fork value is from a
// local build of tangency-labs/smlib 5e8c9c1 (#79), `SmFilletExecutive` run on the same body
// and edge without a global merge, its volume by `SmApiBrepComputeVolume(b, 1e-7)`.

/// The stepped bar's edge at x = 10 rounded r6, past the step's 5 high face: the round is
/// cut along its crease with the step's riser, 8 faces and 1546.0445712960786 of the 1700.
/// Without the trim it is subtracted, to the same volume.
#[test]
fn step_round_trimmed_at_its_crease() {
    let (s, t) = (p(10.0, 0.0, 0.0), p(10.0, 0.0, 20.0));
    let closed = 1700.0 - 20.0 * removed_area(6.0, 10.0, 5.0);
    assert_builds(
        &blend(true, stepped_bar, s, t, 6.0, false),
        closed,
        CLOSED_REL,
        8,
    );
    assert_builds(
        &blend(false, stepped_bar, s, t, 6.0, false),
        closed,
        CLOSED_REL,
        8,
    );
}

/// The stepped bar's inner step edge chamfered 4: the chamfer's leg runs past the step's foot
/// under the wall. The trim builds it as the subtraction does, the step's plane carried on to the
/// leg, 1700 - 160 (the fork refuses it, `SM_FILERR_BAD_MERGE`, fork 8832ead's every-face case:
/// its extension's material probe misses the 1 wide strip and the walls beside are not fenced).
#[test]
fn chamfer_past_the_step_foot_builds_as_the_subtraction() {
    let (s, t) = (p(10.0, 5.0, 0.0), p(10.0, 5.0, 20.0));
    assert_builds(
        &blend(true, stepped_bar, s, t, 4.0, true),
        1540.0,
        CLOSED_REL,
        9,
    );
    assert_builds(
        &blend(false, stepped_bar, s, t, 4.0, true),
        1540.0,
        CLOSED_REL,
        9,
    );
}

/// An 80 x 5 x 5 bar's top edge rounded r10, past both faces: 5 faces and 630.2934872554397
/// of the 2000. This is the case the fork's own build grades worst, 5.2e-9 relative over the
/// closed form against solidcore's 1.8e-10 under it.
#[test]
fn bar_round_past_both_faces() {
    let (s, t) = (p(0.0, 5.0, 5.0), p(80.0, 5.0, 5.0));
    let closed = 2000.0 - 80.0 * removed_area(10.0, 5.0, 5.0);
    assert_builds(&blend(true, bar, s, t, 10.0, false), closed, CLOSED_REL, 5);
    assert_builds(&blend(false, bar, s, t, 10.0, false), closed, CLOSED_REL, 5);
}

/// A 40 x 20 x 10 box's top back edge rounded r10.05, just past the 10 high face: 6 faces
/// and 7132.987202861085 of the 8000.
#[test]
fn box_round_just_past_its_face() {
    let (s, t) = (p(0.0, 20.0, 10.0), p(40.0, 20.0, 10.0));
    let closed = 8000.0 - 40.0 * removed_area(10.05, 20.0, 10.0);
    assert_builds(
        &blend(true, box_40_20_10, s, t, 10.05, false),
        closed,
        CLOSED_REL,
        6,
    );
    assert_builds(
        &blend(false, box_40_20_10, s, t, 10.05, false),
        closed,
        CLOSED_REL,
        6,
    );
}

/// The same edge of the box drilled at (20, 14.5) radius 3, rounded r4: the round's crease on
/// the hole's cylinder is a general surface intersection, so the volume has no closed form
/// here. 8 faces, `AssertValid` clean at `Level2` (both from [`blend`]), and the volume
/// against the fork's build as a cross-check.
#[test]
fn round_trimmed_at_a_hole() {
    let (s, t) = (p(0.0, 20.0, 10.0), p(40.0, 20.0, 10.0));
    assert_builds(
        &blend(true, block_with_hole, s, t, 4.0, false),
        7_580.287_342_340_768_2,
        FORK_REL,
        8,
    );
    assert!(matches!(
        blend(false, block_with_hole, s, t, 4.0, false),
        Outcome::Built { .. }
    ));
}

/// A 10 x 20 x 30 box's top edge rounded r12, past the 10 wide top: 6 faces and
/// 5384.178285184314 of the 6000, and the subtraction gives the same.
#[test]
fn round_past_the_top() {
    let (s, t) = (p(0.0, 0.0, 30.0), p(0.0, 20.0, 30.0));
    let closed = 6000.0 - 20.0 * removed_area(12.0, 10.0, 30.0);
    assert_builds(
        &blend(true, box_10_20_30, s, t, 12.0, false),
        closed,
        CLOSED_REL,
        6,
    );
    assert_builds(
        &blend(false, box_10_20_30, s, t, 12.0, false),
        closed,
        CLOSED_REL,
        6,
    );
}

/// A 20 cube's edge rounded r21, past both 20 wide faces: 5 faces and 6107.529369550197 of
/// the 8000, and the subtraction gives the same.
#[test]
fn cube_round_past_both_faces() {
    let (s, t) = (p(0.0, 0.0, 0.0), p(20.0, 0.0, 0.0));
    let closed = 8000.0 - 20.0 * removed_area(21.0, 20.0, 20.0);
    assert_builds(
        &blend(true, cube_20, s, t, 21.0, false),
        closed,
        CLOSED_REL,
        5,
    );
    assert_builds(
        &blend(false, cube_20, s, t, 21.0, false),
        closed,
        CLOSED_REL,
        5,
    );
}

/// The variable round, linear `r0` to `r1`, on the 10 x 20 x 30 box's top edge at x = 0, its
/// volume, face, edge and vertex counts and the largest gap between an edge's curve and
/// its vertices. `along_edge`: `fillet_overflow_crease_along_edge` on too.
fn variable_round(
    trim: bool,
    along_edge: bool,
    r0: f64,
    r1: f64,
) -> (f64, (usize, usize, usize), f64) {
    let mut k = kernel(trim);
    k.fixes.fillet_overflow_crease_along_edge = along_edge;
    let mut b = box_10_20_30(&mut k);
    let e = edge_between(&k, &b, p(0.0, 0.0, 30.0), p(0.0, 20.0, 30.0));
    let mut session = FilletSession::new(&mut k, &mut b).unwrap();
    session
        .prepare_variable_radius_edges(&mut k, &[e], r0, FilletLaw::linear(r0, r1, None), 1, 1)
        .unwrap();
    session
        .do_fillet(&mut k)
        .unwrap_or_else(|e| panic!("{r0} -> {r1} (trim {trim}): {e}"));
    drop(session);
    assert!(
        is_manifold_solid(&b).unwrap(),
        "{r0} -> {r1}: not a manifold solid"
    );
    let mut gap = 0.0_f64;
    for e in b.edges().ids().collect::<Vec<_>>() {
        let ivl = edge::interval(&b, e).unwrap();
        let c = edge::curve(&b, e).unwrap().unwrap();
        let (s, t) = (
            c.evaluate_point(&k, ivl.min()).unwrap(),
            c.evaluate_point(&k, ivl.max()).unwrap(),
        );
        let (sv, ev) = edge::start_end_vertices(&b, e).unwrap();
        let (sp, ep) = (
            vertex::point(&b, sv.unwrap()).unwrap(),
            vertex::point(&b, ev.unwrap()).unwrap(),
        );
        gap = gap.max(s.distance(sp).min(s.distance(ep)));
        gap = gap.max(t.distance(sp).min(t.distance(ep)));
    }
    let counts = (
        b.faces().ids().count(),
        b.edges().ids().count(),
        b.vertices().ids().count(),
    );
    let built = (volume(&k, &mut b), counts, gap);
    b.use_arena_naming().unwrap();
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{r0} -> {r1}: check_brep: {lines:?}");
    built
}

/// #17673: the variable round past the box's top (`wr631_cliff_overflow_construction`'s
/// case and closed forms, Gauss-Legendre to 1e-9), trimmed at its creases, from either
/// end. The creases meet the sheet's end section at its corners: each crease now ends
/// where the section's edge crosses the crease's face, and both are fitted to a
/// thousandth of the body's tolerance, so every edge's curve runs to its own vertices
/// (the end section's stopped 1.1e-5 short of them) and the volume holds the closed form
/// to 1e-7 (it was 1.3e-7 off from r 50 to 2). The face, edge and vertex counts are the
/// subtraction's.
#[test]
fn a_variable_round_past_a_box_top_trimmed_at_its_creases_keeps_its_closed_form() {
    for (r1, closed) in [
        (11.0, 5_802.443_145_423),
        (20.0, 5_503.887_121_531),
        (30.0, 5_184.057_273_581),
        (50.0, 4_693.608_889_860),
    ] {
        for (r0, r1) in [(2.0, r1), (r1, 2.0)] {
            let (_, faces, _) = variable_round(false, false, r0, r1);
            let (v, n, gap) = variable_round(true, false, r0, r1);
            assert!(
                (v - closed).abs() <= 1e-7 * closed,
                "{r0} -> {r1}: volume {v} against {closed} (rel {})",
                (v - closed).abs() / closed
            );
            assert_eq!(
                n, faces,
                "{r0} -> {r1}: the subtraction's faces, edges, vertices"
            );
            assert!(
                gap <= 1e-12,
                "{r0} -> {r1}: an edge's curve {gap} off its vertices"
            );
        }
    }
}

/// #17673 review: the same rounds and checks with `fillet_overflow_crease_along_edge` on
/// as well. A crease piece run on to its end along an edge of the face takes in the runs
/// that fix also splits the sheet along; each such run is split along once (in the piece),
/// so the merge is given no overlapping curves.
#[test]
fn a_variable_round_past_a_box_top_trimmed_at_its_creases_with_creases_along_edges_keeps_its_closed_form(
) {
    for (r1, closed) in [
        (11.0, 5_802.443_145_423),
        (20.0, 5_503.887_121_531),
        (30.0, 5_184.057_273_581),
        (50.0, 4_693.608_889_860),
    ] {
        for (r0, r1) in [(2.0, r1), (r1, 2.0)] {
            let (_, faces, _) = variable_round(false, true, r0, r1);
            let (v, n, gap) = variable_round(true, true, r0, r1);
            assert!(
                (v - closed).abs() <= 1e-7 * closed,
                "{r0} -> {r1}: volume {v} against {closed} (rel {})",
                (v - closed).abs() / closed
            );
            assert_eq!(
                n, faces,
                "{r0} -> {r1}: the subtraction's faces, edges, vertices"
            );
            assert!(
                gap <= 1e-12,
                "{r0} -> {r1}: an edge's curve {gap} off its vertices"
            );
        }
    }
}
