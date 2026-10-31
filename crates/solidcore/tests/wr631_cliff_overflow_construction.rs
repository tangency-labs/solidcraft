//! WR-631 / WR-638 (#960; the fork's tangency-labs/smlib@8ba4667 and ae2a6de, rev 68fb9162):
//! with `fillet_refuse_rails_leaving_faces` and `fillet_cliff_overflow`, a convex blend
//! whose rail leaves its face across a cliff is built on the faces' continuations, closed
//! into the region it removes and subtracted from the body. Without the flag the same blends
//! are refused (`NoRoom`), as before.
//!
//! Every expected volume is the body less the blend's corner region clipped to the body, in
//! closed form: for a round of radius r on an edge between two perpendicular faces w and h
//! wide, the section removed is `{0 <= u <= min(w, r), 0 <= v <= min(h, r)}` outside the
//! circle of radius r about `(r, r)`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)]

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_geom::p1::FilletErrorType;
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
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

fn kernel(cliff: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_refuse_rails_leaving_faces = true;
    k.fixes.fillet_trace_reaches_rails = true;
    k.fixes.fillet_rail_end_on_continued_surface = true;
    k.fixes.fillet_cliff_overflow = cliff;
    k
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
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

/// The outcome of a blend: the error code and comment of a refusal, or the volume of the
/// result (a manifold solid).
#[derive(Debug)]
enum Outcome {
    Built(f64),
    Refused(i32, String),
}

/// Rounds (`chamfer` false: circular section; true: linear) the edge from `p` to `q` of
/// the body `make` builds, by `r`.
fn blend(
    cliff: bool,
    make: fn(&mut Kernel) -> Brep,
    p: Point3,
    q: Point3,
    r: f64,
    chamfer: bool,
) -> Outcome {
    let mut k = kernel(cliff);
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
            Outcome::Built(volume(&k, &mut b))
        }
        Err(_) => Outcome::Refused(info.code, info.comment),
    }
}

fn assert_builds(outcome: &Outcome, expected: f64, rel: f64) {
    let Outcome::Built(v) = outcome else {
        panic!("expected a build at {expected}, got {outcome:?}");
    };
    assert!(
        (v - expected).abs() <= rel * expected,
        "volume {v} against {expected} (rel {})",
        (v - expected).abs() / expected
    );
}

fn assert_no_room(outcome: &Outcome) {
    let Outcome::Refused(code, _) = outcome else {
        panic!("expected a refusal, got {outcome:?}");
    };
    assert_eq!(*code, FilletErrorType::NoRoom as i32, "{outcome:?}");
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

#[test]
fn a_round_over_a_step_rim_is_subtracted() {
    // r6 at (10, 0): the rail on the 5 high side runs past the step's rim
    let expected = 1700.0 - 20.0 * removed_area(6.0, 10.0, 5.0);
    let (a, b) = (p(10.0, 0.0, 0.0), p(10.0, 0.0, 20.0));
    assert_builds(&blend(true, stepped_bar, a, b, 6.0, false), expected, 1e-8);
    assert_no_room(&blend(false, stepped_bar, a, b, 6.0, false));
}

#[test]
fn a_chamfer_into_a_wall_foot_is_subtracted() {
    // chamfer 4 at (10, 5): it reaches x = 6 on the 3 wide step, past the wall's foot; the
    // triangle it removes lies inside the body
    let (a, b) = (p(10.0, 5.0, 0.0), p(10.0, 5.0, 20.0));
    assert_builds(
        &blend(true, stepped_bar, a, b, 4.0, true),
        1700.0 - 20.0 * 8.0,
        1e-9,
    );
    assert_no_room(&blend(false, stepped_bar, a, b, 4.0, true));
}

#[test]
fn a_round_wider_than_a_bar_is_subtracted() {
    // #1466: the long top edge of an 80 x 5 x 5 bar; r9 and r10 run past both faces
    let (a, b) = (p(0.0, 5.0, 5.0), p(80.0, 5.0, 5.0));
    for r in [9.0, 10.0, 12.0] {
        let expected = 80.0 * 25.0 - 80.0 * removed_area(r, 5.0, 5.0);
        assert_builds(&blend(true, bar, a, b, r, false), expected, 1e-8);
    }
    assert_no_room(&blend(false, bar, a, b, 10.0, false));
}

#[test]
fn a_round_just_past_a_box_side_is_subtracted() {
    // #1467: the top edge of the 40 x 20 x 10 box on its 10 high face, r just over 10 (the
    // radii of the fork's test)
    let (a, b) = (p(0.0, 20.0, 10.0), p(40.0, 20.0, 10.0));
    for r in [10.05, 10.07, 12.0] {
        let expected = 8000.0 - 40.0 * removed_area(r, 20.0, 10.0);
        assert_builds(&blend(true, box_40_20_10, a, b, r, false), expected, 1e-8);
    }
    assert_no_room(&blend(false, box_40_20_10, a, b, 10.05, false));
}

/// A 40 x 20 x 10 block with a through hole of radius 3 at (20, 14.5) (#422, SolidCraft
/// rig 324, `fixtures/solidcraft/324-round-beside-through-hole.json`).
fn block_with_hole(k: &mut Kernel) -> Brep {
    let block = create_box(k, Point3::ZERO, 40.0, 20.0, 10.0).unwrap();
    let drill = create_cylinder(k, p(20.0, 14.5, -1.0), 3.0, 12.0).unwrap();
    boolean_difference(k, block, drill).unwrap()
}

#[test]
fn a_round_over_a_hole_is_subtracted() {
    // #422: R4 on the top edge at y = 20; its rail on the top face runs into the hole, whose
    // rim is a cliff. The block less the hole and the round's corner, plus the part of that
    // corner inside the hole: O = int_16^17.5 2 sqrt(9 - (y - 14.5)^2) (4 - sqrt(16 - (y -
    // 16)^2)) dy = 0.3758296360 (the fixture's formula, its value)
    let (a, b) = (p(0.0, 20.0, 10.0), p(40.0, 20.0, 10.0));
    let expected = 7580.287315387334;
    assert_builds(
        &blend(true, block_with_hole, a, b, 4.0, false),
        expected,
        1e-8,
    );
    assert_no_room(&blend(false, block_with_hole, a, b, 4.0, false));
}

/// The 10 x 20 x 30 box of `wr609_trace_reaches_rails`; its top edge at x = 0 is 20 long,
/// the top face 10 wide, the side 30 high.
fn box_10_20_30(k: &mut Kernel) -> Brep {
    create_box(k, Point3::ZERO, 10.0, 20.0, 30.0).unwrap()
}

#[test]
fn a_constant_round_past_a_box_top_is_subtracted() {
    // R12: the rail on the 10 wide top runs past its far edge (the fork's 5384.17828518)
    let (a, b) = (p(0.0, 0.0, 30.0), p(0.0, 20.0, 30.0));
    let expected = 6000.0 - 20.0 * removed_area(12.0, 10.0, 30.0);
    assert!((expected - 5384.17828518).abs() < 1e-8);
    assert_builds(
        &blend(true, box_10_20_30, a, b, 12.0, false),
        expected,
        1e-8,
    );
    assert_no_room(&blend(false, box_10_20_30, a, b, 12.0, false));
}

/// The variable round from `r0` to `r1` (linear) of the box's top edge at x = 0.
fn variable_round(cliff: bool, r0: f64, r1: f64) -> Outcome {
    let mut k = kernel(cliff);
    let mut b = box_10_20_30(&mut k);
    let e = edge_between(&k, &b, p(0.0, 0.0, 30.0), p(0.0, 20.0, 30.0));
    let mut session = FilletSession::new(&mut k, &mut b).unwrap();
    session
        .prepare_variable_radius_edges(&mut k, &[e], r0, FilletLaw::linear(r0, r1, None), 1, 1)
        .unwrap();
    let result = session.do_fillet(&mut k);
    let info = session.error_info().clone();
    drop(session);
    match result {
        Ok(()) => {
            assert!(is_manifold_solid(&b).unwrap(), "not a manifold solid");
            Outcome::Built(volume(&k, &mut b))
        }
        Err(_) => Outcome::Refused(info.code, info.comment),
    }
}

/// The box less the variable round's section along the edge, by the midpoint rule (the
/// rolling ball's section across the edge is the circle of the radius there, to the
/// accuracy of the round's cubic sections).
fn variable_round_expected(r0: f64, r1: f64) -> f64 {
    let n = 20_000;
    let mut removed = 0.0;
    for i in 0..n {
        let t = (f64::from(i) + 0.5) / f64::from(n);
        removed += removed_area(r0 + (r1 - r0) * t, 10.0, 30.0);
    }
    6000.0 - removed * 20.0 / f64::from(n)
}

#[test]
fn a_variable_round_past_a_box_top_is_subtracted() {
    // the fork's 2 -> 11, 20, 30, 50 (5789.69878, 5391.54581, 4819.37775, 3546.18768), from
    // either end, to 1e-4 (the variable round's cubic sections' accuracy)
    for (r1, fork) in [
        (11.0, 5789.69878),
        (20.0, 5391.54581),
        (30.0, 4819.37775),
        (50.0, 3546.18768),
    ] {
        let expected = variable_round_expected(2.0, r1);
        assert!((expected - fork).abs() < 1e-4, "{r1}: {expected}");
        assert_builds(&variable_round(true, 2.0, r1), expected, 1e-4);
        assert_builds(&variable_round(true, r1, 2.0), expected, 1e-4);
    }
    assert_no_room(&variable_round(false, 2.0, 11.0));
}

/// The 40 x 20 x 10 box with its x = 0 end cut concave by a vertical cylinder of radius 30
/// about (-25, 10): the top edge at y = 20 ends on that cylinder.
fn box_with_round_end(k: &mut Kernel) -> Brep {
    let block = create_box(k, Point3::ZERO, 40.0, 20.0, 10.0).unwrap();
    let cutter = create_cylinder(k, p(-25.0, 10.0, -1.0), 30.0, 12.0).unwrap();
    boolean_difference(k, block, cutter).unwrap()
}

#[test]
fn an_overflow_the_construction_cannot_cut_reports_its_own_error() {
    // WR-638 (ae2a6de): R12 on the top edge at y = 20 runs past the 10 high side; at the
    // cylinder end its rails miss their side edges, and the end face that would cut the
    // fillet off there is not planar. The refusal is the construction's own, not the generic
    // "larger than the faces" (which the flag off still gives).
    let x0 = -25.0 + 800.0f64.sqrt();
    let (a, b) = (p(x0, 20.0, 10.0), p(40.0, 20.0, 10.0));
    let Outcome::Refused(code, comment) = blend(true, box_with_round_end, a, b, 12.0, false) else {
        panic!("expected a refusal");
    };
    assert_eq!(code, FilletErrorType::NoRoom as i32);
    assert_eq!(
        comment,
        "Fillet Error: a cliff overflow runs past a fillet end whose end face is not planar"
    );
    let Outcome::Refused(code, comment) = blend(false, box_with_round_end, a, b, 12.0, false)
    else {
        panic!("expected a refusal");
    };
    assert_eq!(code, FilletErrorType::NoRoom as i32);
    assert_eq!(
        comment,
        "Fillet Error: a rail leaves the face it rolls on (the blend is larger than the faces allow)"
    );
}

#[test]
fn a_cliff_overflow_is_deterministic() {
    let (a, b) = (p(10.0, 0.0, 0.0), p(10.0, 0.0, 20.0));
    let runs: Vec<f64> = (0..2)
        .map(|_| match blend(true, stepped_bar, a, b, 6.0, false) {
            Outcome::Built(v) => v,
            other @ Outcome::Refused(..) => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(runs[0].to_bits(), runs[1].to_bits());
}

#[test]
fn an_overflow_whose_subtraction_leaves_no_solid_is_refused() {
    // #1467's box at r10.01 and r10.02: the round's face meets the bottom face a hair (2e-5
    // at r10.02) from its edge, and the Boolean succeeded with an empty or open body. A
    // built blend must leave a manifold solid; this one is refused as no room instead
    // (`CliffSubtractionLeavesNoSolid`, not the fork's: the fork checks only the Boolean's
    // status), and the body is left as it was.
    let (a, b) = (p(0.0, 20.0, 10.0), p(40.0, 20.0, 10.0));
    for r in [10.01, 10.02] {
        let mut k = kernel(true);
        let mut body = box_40_20_10(&mut k);
        let e = edge_between(&k, &body, a, b);
        let mut session = FilletSession::new(&mut k, &mut body).unwrap();
        session
            .prepare_constant_radius_edges(&mut k, &[e], r, 1, 1, 1.0)
            .unwrap();
        let result = session.do_fillet(&mut k);
        let info = session.error_info().clone();
        drop(session);
        if result.is_ok() {
            assert!(
                is_manifold_solid(&body).unwrap(),
                "r {r}: built a body that is not a manifold solid"
            );
            let expected = 8000.0 - 40.0 * removed_area(r, 20.0, 10.0);
            let v = volume(&k, &mut body);
            assert!(
                (v - expected).abs() <= 1e-8 * expected,
                "r {r}: {v} against {expected}"
            );
            continue;
        }
        assert_eq!(info.code, FilletErrorType::NoRoom as i32, "r {r}: {info:?}");
        assert_eq!(
            info.comment,
            "Fillet Error: subtracting a cliff overflow's removal region leaves no solid",
            "r {r}"
        );
        assert!(
            is_manifold_solid(&body).unwrap(),
            "r {r}: the body must be left whole"
        );
        assert!((volume(&k, &mut body) - 8000.0).abs() < 1e-6, "r {r}");
    }
}
