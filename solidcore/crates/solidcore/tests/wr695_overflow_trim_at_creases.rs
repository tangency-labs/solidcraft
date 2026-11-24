//! The cliff overflow trimmed at its creases in place (WR-695, tangency-labs/smlib#79
//! step 6; once `UpstreamFixes::fillet_overflow_trim_at_creases`, now unconditional).
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
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, vertex, EdgeId};
use solidcore::{Brep, Kernel};

/// The cliff overflow's flags (as `wr631_cliff_overflow_construction`).
fn kernel() -> Kernel {
    Kernel::default()
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
fn blend(make: fn(&mut Kernel) -> Brep, p: Point3, q: Point3, r: f64, chamfer: bool) -> Outcome {
    let mut k = kernel();
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
#[test]
fn step_round_trimmed_at_its_crease() {
    let (s, t) = (p(10.0, 0.0, 0.0), p(10.0, 0.0, 20.0));
    let closed = 1700.0 - 20.0 * removed_area(6.0, 10.0, 5.0);
    assert_builds(&blend(stepped_bar, s, t, 6.0, false), closed, CLOSED_REL, 8);
}

/// The stepped bar's inner step edge chamfered 4: the chamfer's leg runs past the step's foot
/// under the wall. The trim builds it (as the removed subtraction did), the step's plane
/// carried on to the leg, 1700 - 160 (the fork refuses it, `SM_FILERR_BAD_MERGE`, fork
/// 8832ead's every-face case: its extension's material probe misses the 1 wide strip and the
/// walls beside are not fenced).
#[test]
fn chamfer_past_the_step_foot_builds() {
    let (s, t) = (p(10.0, 5.0, 0.0), p(10.0, 5.0, 20.0));
    assert_builds(&blend(stepped_bar, s, t, 4.0, true), 1540.0, CLOSED_REL, 9);
}

/// An 80 x 5 x 5 bar's top edge rounded r10, past both faces: 5 faces and 630.2934872554397
/// of the 2000. This is the case the fork's own build grades worst, 5.2e-9 relative over the
/// closed form against solidcore's 1.8e-10 under it.
#[test]
fn bar_round_past_both_faces() {
    let (s, t) = (p(0.0, 5.0, 5.0), p(80.0, 5.0, 5.0));
    let closed = 2000.0 - 80.0 * removed_area(10.0, 5.0, 5.0);
    assert_builds(&blend(bar, s, t, 10.0, false), closed, CLOSED_REL, 5);
}

/// A 40 x 20 x 10 box's top back edge rounded r10.05, just past the 10 high face: 6 faces
/// and 7132.987202861085 of the 8000.
#[test]
fn box_round_just_past_its_face() {
    let (s, t) = (p(0.0, 20.0, 10.0), p(40.0, 20.0, 10.0));
    let closed = 8000.0 - 40.0 * removed_area(10.05, 20.0, 10.0);
    assert_builds(
        &blend(box_40_20_10, s, t, 10.05, false),
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
        &blend(block_with_hole, s, t, 4.0, false),
        7_580.287_342_340_768_2,
        FORK_REL,
        8,
    );
}

/// A 10 x 20 x 30 box's top edge rounded r12, past the 10 wide top: 6 faces and
/// 5384.178285184314 of the 6000.
#[test]
fn round_past_the_top() {
    let (s, t) = (p(0.0, 0.0, 30.0), p(0.0, 20.0, 30.0));
    let closed = 6000.0 - 20.0 * removed_area(12.0, 10.0, 30.0);
    assert_builds(
        &blend(box_10_20_30, s, t, 12.0, false),
        closed,
        CLOSED_REL,
        6,
    );
}

/// A 20 cube's edge rounded r21, past both 20 wide faces: 5 faces and 6107.529369550197 of
/// the 8000.
#[test]
fn cube_round_past_both_faces() {
    let (s, t) = (p(0.0, 0.0, 0.0), p(20.0, 0.0, 0.0));
    let closed = 8000.0 - 20.0 * removed_area(21.0, 20.0, 20.0);
    assert_builds(&blend(cube_20, s, t, 21.0, false), closed, CLOSED_REL, 5);
}

/// The variable round, linear `r0` to `r1`, on the 10 x 20 x 30 box's top edge at x = 0, its
/// volume, face, edge and vertex counts and the largest gap between an edge's curve and
/// its vertices.
fn variable_round(r0: f64, r1: f64) -> (f64, (usize, usize, usize), f64) {
    let mut k = kernel();
    let mut b = box_10_20_30(&mut k);
    let e = edge_between(&k, &b, p(0.0, 0.0, 30.0), p(0.0, 20.0, 30.0));
    let mut session = FilletSession::new(&mut k, &mut b).unwrap();
    session
        .prepare_variable_radius_edges(&mut k, &[e], r0, FilletLaw::linear(r0, r1, None), 1, 1)
        .unwrap();
    session
        .do_fillet(&mut k)
        .unwrap_or_else(|e| panic!("{r0} -> {r1}: {e}"));
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
/// to 1e-7 (it was 1.3e-7 off from r 50 to 2). A crease piece run on to its end along an
/// edge of the face takes in the runs WR-717 also splits the sheet along; each such run
/// is split along once (in the piece), so the merge is given no overlapping curves. The
/// face, edge and vertex counts are those the removed subtraction built (this test
/// compared against it while it was the flag-off path).
#[test]
fn a_variable_round_past_a_box_top_trimmed_at_its_creases_keeps_its_closed_form() {
    for (r1, closed, counts) in [
        (11.0, 5_802.443_145_423, (7, 15, 10)),
        (20.0, 5_503.887_121_531, (7, 15, 10)),
        (30.0, 5_184.057_273_581, (7, 14, 9)),
        (50.0, 4_693.608_889_860, (7, 15, 10)),
    ] {
        for (r0, r1) in [(2.0, r1), (r1, 2.0)] {
            let (v, n, gap) = variable_round(r0, r1);
            assert!(
                (v - closed).abs() <= 1e-7 * closed,
                "{r0} -> {r1}: volume {v} against {closed} (rel {})",
                (v - closed).abs() / closed
            );
            assert_eq!(
                n, counts,
                "{r0} -> {r1}: the subtraction's faces, edges, vertices"
            );
            assert!(
                gap <= 1e-12,
                "{r0} -> {r1}: an edge's curve {gap} off its vertices"
            );
        }
    }
}

/// Blends (`chamfer` false: circular section; true: linear) the edges between each pair of
/// points of `edges` of the body `make` builds, by `r`, in one fillet.
fn blend_edges(
    make: fn(&mut Kernel) -> Brep,
    edges: &[(Point3, Point3)],
    r: f64,
    chamfer: bool,
) -> Outcome {
    let mut k = kernel();
    let mut b = make(&mut k);
    let es: Vec<EdgeId> = edges
        .iter()
        .map(|&(p, q)| edge_between(&k, &b, p, q))
        .collect();
    let mut session = FilletSession::new(&mut k, &mut b).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &es, r, u32::from(!chamfer), 1, 1.0)
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

/// The volume and face count of a build (panics on a refusal).
fn built(outcome: &Outcome) -> (f64, usize) {
    match outcome {
        Outcome::Built { volume, faces } => (*volume, *faces),
        Outcome::Refused(code, comment) => panic!("refused (code {code}): {comment}"),
    }
}

/// The stepped bar swept 30 along z and slotted between z = 10 and 20 through its step
/// (x 4 to 11, y -1 to 6): the step's 3 wide top (y = 5) is two faces of one plane, its
/// outer edge at x = 10 two edges, z 0 to 10 and 20 to 30.
fn slotted_stepped_bar(k: &mut Kernel) -> Brep {
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
    let bar = create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 30.0, true).unwrap();
    let slot = create_box(k, p(4.0, -1.0, 10.0), 7.0, 7.0, 10.0).unwrap();
    boolean_difference(k, bar, slot).unwrap()
}

/// #18646: the slotted stepped bar's outer step edges chamfered 4, one and both. Each
/// chamfer's leg runs past the step's foot under the wall, and each lies on the plane of
/// both step tops; the other top, apart, is not the face it runs past, so it is not
/// carried on to it (it was, and even one chamfer was refused: "ends off the lines of the
/// face's side edges"). The bar is 85 x 30 = 2550 less the slot's 33 x 10, and each
/// chamfer takes 4 x 4 / 2 over its 10: 2140 for one, 2060 for both, each its own new
/// faces.
#[test]
fn two_chamfers_past_the_feet_of_two_step_tops_of_one_plane_build() {
    let low = (p(10.0, 5.0, 0.0), p(10.0, 5.0, 10.0));
    let high = (p(10.0, 5.0, 20.0), p(10.0, 5.0, 30.0));
    let mut k = kernel();
    let mut bar = slotted_stepped_bar(&mut k);
    let bar_faces = bar.faces().ids().count();
    assert!((volume(&k, &mut bar) - 2220.0).abs() <= CLOSED_REL * 2220.0);
    let (v_low, n_low) = built(&blend_edges(slotted_stepped_bar, &[low], 4.0, true));
    assert!(
        (v_low - 2140.0).abs() <= CLOSED_REL * 2140.0,
        "volume {v_low}"
    );
    let (_, n_high) = built(&blend_edges(slotted_stepped_bar, &[high], 4.0, true));
    for edges in [[low, high], [high, low]] {
        assert_builds(
            &blend_edges(slotted_stepped_bar, &edges, 4.0, true),
            2060.0,
            CLOSED_REL,
            n_low + n_high - bar_faces,
        );
    }
}

/// The issue's hooked bar: its profile (0, 5), an R3 arc about (3/√2, 5 + 3/√2) to its far
/// point, a 3 long flat on to the tip, an 8.3 long flat up at 45° to (8, ...), down x = 8,
/// back along y = 0, swept 30 along z; slotted between z = 10 and 20 for x below 7. The
/// tip's 90° edge is two edges, z 0 to 10 and 20 to 30, and the 3 long flat that runs on
/// tangentially into the arc two faces of one plane.
fn slotted_hook(k: &mut Kernel) -> Brep {
    let s = 3.0 / 2.0_f64.sqrt();
    let centre = (s, 5.0 + s);
    let near = (2.0 * s, 5.0 + 2.0 * s);
    let tip = (s, 5.0 + 3.0 * s);
    let top = (8.0, 5.0 + 3.0 * s + (8.0 - s));
    let line = |a: (f64, f64), b: (f64, f64)| {
        Curve::from(
            create_line_segment(Vec3::new(a.0, a.1, 0.0), Vec3::new(b.0, b.1, 0.0)).unwrap(),
        )
    };
    let cs = [
        Curve::from(create_arc(p(centre.0, centre.1, 0.0), 3.0, -135.0, 45.0).unwrap()),
        line(near, tip),
        line(tip, top),
        line(top, (8.0, 0.0)),
        line((8.0, 0.0), (0.0, 0.0)),
        line((0.0, 0.0), (0.0, 5.0)),
    ];
    let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
    let hook = create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 30.0, true).unwrap();
    let slot = create_box(k, p(-1.0, -1.0, 10.0), 8.0, 20.0, 10.0).unwrap();
    boolean_difference(k, hook, slot).unwrap()
}

/// #18646: the slotted hook's two tip edges rounded R5 together, as each alone. Each round
/// runs past the 3 wide flat into the material behind the arc, on the plane of both
/// flats; it was refused for the pair ("the largest radius that fits is 3 mm") while each
/// built alone. The two rounds lie 10 apart, so the pair takes off what each takes alone:
/// its volume is the two singles' less the bar's, and its faces the bar's and each
/// round's new ones.
#[test]
fn two_rounds_past_two_flats_of_one_plane_build_as_each_alone() {
    let s = 3.0 / 2.0_f64.sqrt();
    let tip = |z: f64| p(s, 5.0 + 3.0 * s, z);
    let low = (tip(0.0), tip(10.0));
    let high = (tip(20.0), tip(30.0));
    let mut k = kernel();
    let mut bar = slotted_hook(&mut k);
    let (bar_volume, bar_faces) = (volume(&k, &mut bar), bar.faces().ids().count());
    let (v_low, n_low) = built(&blend_edges(slotted_hook, &[low], 5.0, false));
    let (v_high, n_high) = built(&blend_edges(slotted_hook, &[high], 5.0, false));
    // In the orthonormal frame along the tip's flats, the old R3 arc has centre
    // (3, -3). The R5 centre is (5, sqrt(60) - 3), eight from that centre;
    // their contact angle is acos(1/4). Integrating the two circular arcs around
    // the removed section gives 8 sqrt(15) - 9 - 8 acos(1/4) - 9 pi/4.
    let removed = 8.0 * 15.0_f64.sqrt() - 9.0 - 8.0 * 0.25_f64.acos() - 2.25 * std::f64::consts::PI;
    let exact_bar = 1295.0 + 510.0 * 2.0_f64.sqrt() - 90.0 * std::f64::consts::PI;
    assert!((bar_volume - exact_bar).abs() <= CLOSED_REL * exact_bar);
    for single in [v_low, v_high] {
        let exact_single = exact_bar - 10.0 * removed;
        assert!((single - exact_single).abs() <= CLOSED_REL * exact_single);
    }
    let expected = v_low + v_high - bar_volume;
    let exact_pair = exact_bar - 20.0 * removed;
    assert!((expected - exact_pair).abs() <= CLOSED_REL * exact_pair);
    for edges in [[low, high], [high, low]] {
        assert_builds(
            &blend_edges(slotted_hook, &edges, 5.0, false),
            expected,
            CLOSED_REL,
            n_low + n_high - bar_faces,
        );
    }
}
