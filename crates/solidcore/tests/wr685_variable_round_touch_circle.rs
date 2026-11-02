//! WR-685 addendum (#2250; the fork's tangency-labs/smlib d2a6125, f2ec002, e9560e3 and
//! 2714d37 in d01d7b0, rebased from 6b346f6, 5db8feb, fb5c060 and d622107): with
//! `fixes.fillet_gn_section_generator` a variable round is the union of its balls. Its
//! circle section is the circle along which the balls' envelope touches the ball, square
//! to the centre line and moved along it by `-r r'/|c'|`, not the great circle through the
//! rails; and a round whose balls fold over one another is refused (`NoRoom`, naming the
//! radius there).
//!
//! A linear law between two perpendicular planes rolls its balls inside a cone: the ball
//! of radius `r(x)` at `x` along the edge has its centre `r(x) sqrt 2` from the edge, so
//! the balls' envelope is the cone through the edge's apex `r = 0`. The round removes the
//! corner between the planes and that cone (the fork's capi test
//! `section_variable_round_on_its_cone`: box 20 x 30 x 20, its edge y = 30, z = 20, law 1
//! to 5 from x = 0, volume 11956.2224267359).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_executive::NoRoomReason;
use solidcore::blend::fillet_standard_solver::p1::FilletLaw;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::extent1d::Extent1d;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::p1::{
    BSplineCurve, BSplineCurveForm, CanonicalData, KnotType,
};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

/// The edge from `p` to `q`, and whether it runs from `q` to `p`.
fn edge_between(k: &Kernel, b: &Brep, p: Point3, q: Point3) -> (EdgeId, bool) {
    b.edges()
        .ids()
        .find_map(|e| {
            let ivl = edge::interval(b, e).unwrap();
            let c = edge::curve(b, e).unwrap().unwrap();
            let s = c.evaluate_point(k, ivl.min()).unwrap();
            let t = c.evaluate_point(k, ivl.max()).unwrap();
            if s.distance(p) < 1e-9 && t.distance(q) < 1e-9 {
                Some((e, false))
            } else if s.distance(q) < 1e-9 && t.distance(p) < 1e-9 {
                Some((e, true))
            } else {
                None
            }
        })
        .expect("edge")
}

/// The kernel: every fix on (the fork's own behaviour) when `fork`, else upstream's; the
/// G^n section generator as `gn`. Without it the kernel is the fork before d01d7b0, whose
/// rounds' sections are still upstream's approximated arcs: the exact circle
/// (`fillet_exact_arc_sections`, WR-708, the fork's 523bc25) comes after the G^n section
/// and is off with it.
fn kernel(fork: bool, gn: bool) -> Kernel {
    let mut k = Kernel::default();
    let mut fixes = if fork {
        UpstreamFixes::all()
    } else {
        UpstreamFixes::default()
    };
    fixes.fillet_gn_section_generator = gn;
    if !gn {
        fixes.fillet_exact_arc_sections = false;
    }
    k.fixes = fixes;
    k
}

/// The fork at d01d7b0 (and 58d03ef): every fix but the exact circle of 523bc25
/// (`fillet_exact_arc_sections`, WR-708).
fn kernel_d01d7b0() -> Kernel {
    let mut k = kernel(true, true);
    k.fixes.fillet_exact_arc_sections = false;
    k
}

/// The fork at 5e8c9c1 (and 523bc25): every fix but WR-711's one arc count of the exact
/// circles (`fillet_exact_arc_spans`), which comes after them.
fn kernel_5e8c9c1() -> Kernel {
    let mut k = kernel(true, true);
    k.fixes.fillet_exact_arc_spans = false;
    k
}

/// The box 20 x 30 x 20 after the law 1 to 5 from x = 0 rounds its edge y = 30, z = 20.
fn cone_round(mut k: Kernel) -> f64 {
    let mut b = create_box(&mut k, Vec3::ZERO, 20.0, 30.0, 20.0).unwrap();
    let (e, reversed) = edge_between(
        &k,
        &b,
        Point3::new(0.0, 30.0, 20.0),
        Point3::new(20.0, 30.0, 20.0),
    );
    let (r0, r1) = if reversed { (5.0, 1.0) } else { (1.0, 5.0) };
    {
        let mut session = FilletSession::new(&mut k, &mut b).unwrap();
        session
            .prepare_variable_radius_edges(&mut k, &[e], r0, FilletLaw::linear(r0, r1, None), 1, 1)
            .unwrap();
        session.do_fillet(&mut k).unwrap();
    }
    volume(&k, &mut b)
}

/// The cubic law with control radii `r` over the knots 0, 0, 0, 0, 0.5, 1, 1, 1, 1, its
/// y the law's parameter (control points at the Greville abscissae, as the fork's capi
/// builds `smlib_fillet_variable`'s law; a law curve without speed is not extended).
fn spline_law(r: [f64; 5]) -> FilletLaw {
    let g = [0.0, 1.0 / 6.0, 0.5, 5.0 / 6.0, 1.0];
    let control: Vec<Vec3> = (0..5).map(|i| Vec3::new(r[i], g[i], 0.0)).collect();
    let curve = BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree: 3,
        control_points: &control,
        form: BSplineCurveForm::default(),
        knot_multiplicities: &[4, 1, 4],
        knots: &[0.0, 0.5, 1.0],
        knot_type: KnotType::default(),
        weights: None,
        trim: None,
    })
    .unwrap();
    FilletLaw::bspline(curve, &Extent1d::from_min_max(0.0, 1.0))
}

/// The box 10 x 20 x 30 after the cubic law `r` from y = 0 rounds its edge x = 10,
/// z = 30 (the fork's capi test `section_variable_round_spline_folds`): its volume, or
/// the fillet's error and its typed `NoRoom` reason.
fn spline_round(mut k: Kernel, r: [f64; 5]) -> Result<f64, (String, Option<NoRoomReason>)> {
    let mut b = create_box(&mut k, Vec3::ZERO, 10.0, 20.0, 30.0).unwrap();
    let (e, reversed) = edge_between(
        &k,
        &b,
        Point3::new(10.0, 0.0, 30.0),
        Point3::new(10.0, 20.0, 30.0),
    );
    let mut law = r;
    if reversed {
        law.reverse();
    }
    let done = {
        let mut session = FilletSession::new(&mut k, &mut b).unwrap();
        session
            .prepare_variable_radius_edges(&mut k, &[e], law[0], spline_law(law), 1, 1)
            .unwrap();
        session.do_fillet(&mut k).map_err(|err| {
            let info = session.error_info();
            (format!("{err:?}: {}", info.comment), info.no_room_reason())
        })
    };
    done.map(|()| volume(&k, &mut b))
}

/// The fork's capi test `section_variable_round_on_its_cone`: the box's volume less the
/// corner between the planes and the balls' cone, in closed form.
const CONE_VOLUME: f64 = 11956.2224267359;

/// The fold law's peak and the law that holds (the fork's capi test
/// `section_variable_round_spline_folds`).
const FOLDS: [f64; 5] = [2.0, 2.0, 40.0, 3.0, 2.0];
const HOLDS: [f64; 5] = [2.0, 2.0, 6.0, 3.0, 0.0];

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / a.abs().max(b.abs())
}

#[test]
fn a_linear_round_is_the_balls_cone() {
    for fork in [false, true] {
        let v = cone_round(kernel(fork, true));
        let removed = 12000.0 - v;
        eprintln!(
            "fork fixes {fork}: {v} removed {removed}, cone {}",
            12000.0 - CONE_VOLUME
        );
        // the fork's grade: the volume to 1e-5 (here 2.0e-7; 5.6e-5 of the 43.778 removed)
        assert!(rel(v, CONE_VOLUME) < 1e-5, "{v} vs {CONE_VOLUME}");
        assert!(rel(removed, 12000.0 - CONE_VOLUME) < 1e-4, "{removed}");
    }
    // the fork's 5e8c9c1 build (SmFilletExecutive as SmApiVariableRadiusFillet runs it,
    // SmApiBrepComputeVolume at 1e-8): 11956.222433581395 (523bc25: 11956.222433581312),
    // its sections the balls' exact circles; to the last digit here
    let v = cone_round(kernel_5e8c9c1());
    assert!(rel(v, 11956.222433581395) < 1e-9, "{v}");
    // the fork's d01d7b0 and 58d03ef builds, the approximated arcs: 11956.219913611159
    let v = cone_round(kernel_d01d7b0());
    assert!(rel(v, 11956.219913611159) < 1e-9, "{v}");
}

#[test]
fn a_linear_round_keeps_the_great_circles_without_the_fix() {
    for fork in [false, true] {
        let v = cone_round(kernel(fork, false));
        // the great circles leave the cone: 44.353 removed, 1.3% over the cone's 43.778
        // (SolidCraft's measurement of solidcore before the fix)
        let removed = 12000.0 - v;
        eprintln!("fork fixes {fork}, no G^n section: {v} removed {removed}");
        assert!((removed - 44.353).abs() < 1e-3, "{removed}");
        assert!(rel(v, CONE_VOLUME) > 1e-5, "{v}");
    }
}

#[test]
fn folding_balls_are_refused_naming_the_radius() {
    let (err, reason) =
        spline_round(kernel(true, true), FOLDS).expect_err("a folded round is refused");
    eprintln!("{err}");
    assert_eq!(reason, Some(NoRoomReason::BallsFold));
    // since the fork's tangency-labs/smlib#79 (410dfe1, WR-696) the fold is found by
    // `BallIsRegular` before the corners, where the law first stops being progressive:
    // the fork's 5e8c9c1 build names 19.63563 (its capi test: 19.6356317), and 7.838673
    // for the law peaking at 13.2 (capi: 7.8386732); d01d7b0's `sm_BallsFold` named 19.6284
    assert!(
        err.contains("fold over one another where its radius is 19.63563:"),
        "{err}"
    );
    let (err, reason) = spline_round(kernel(true, true), [2.0, 2.0, 13.2, 3.0, 2.0])
        .expect_err("a folded round is refused");
    assert_eq!(reason, Some(NoRoomReason::BallsFold));
    assert!(
        err.contains("fold over one another where its radius is 7.838673:"),
        "{err}"
    );
}

#[test]
fn folding_balls_build_folded_without_the_fix() {
    // the great circles never fold: the round builds, 726.56 removed against the union of
    // the balls' 332.2 (the fork's capi test, before d622107: 726.53)
    let v = spline_round(kernel(true, false), FOLDS).expect("builds");
    eprintln!("no G^n section: {v} removed {}", 6000.0 - v);
    assert!((6000.0 - v - 726.5).abs() < 0.5, "{v}");
}

#[test]
fn a_progressive_spline_round_builds() {
    // the fork's 5e8c9c1 and 523bc25 builds, the exact circles: 5957.716621492882 (here
    // 2.2e-13 off)
    let v = spline_round(kernel_5e8c9c1(), HOLDS).expect("builds");
    eprintln!(
        "{v} vs 5957.716621492882, rel {}",
        rel(v, 5957.716621492882)
    );
    assert!(rel(v, 5957.716621492882) < 1e-9, "{v}");
    // the fork's d01d7b0 and 58d03ef builds, the approximated arcs: 5957.7178399654949
    let v = spline_round(kernel_d01d7b0(), HOLDS).expect("builds");
    eprintln!(
        "{v} vs 5957.7178399654949, rel {}",
        rel(v, 5957.7178399654949)
    );
    assert!(rel(v, 5957.7178399654949) < 1e-9, "{v}");
}

/// The box 10 x 20 x 30 after the linear law `r0` to `r1` from y = 0 rounds its top edge
/// x = 0, z = 30: the rail on the 10 wide top runs past its far edge, a cliff overflow
/// (`fixes.fillet_cliff_overflow`, WR-631) whose round must hold its law
/// (`sm_HoldsItsLaw`).
fn cliff_round(mut k: Kernel, r0: f64, r1: f64) -> Result<f64, (String, Option<NoRoomReason>)> {
    let mut b = create_box(&mut k, Vec3::ZERO, 10.0, 20.0, 30.0).unwrap();
    let (e, reversed) = edge_between(
        &k,
        &b,
        Point3::new(0.0, 0.0, 30.0),
        Point3::new(0.0, 20.0, 30.0),
    );
    let (r0, r1) = if reversed { (r1, r0) } else { (r0, r1) };
    let done = {
        let mut session = FilletSession::new(&mut k, &mut b).unwrap();
        session
            .prepare_variable_radius_edges(&mut k, &[e], r0, FilletLaw::linear(r0, r1, None), 1, 1)
            .unwrap();
        session.do_fillet(&mut k).map_err(|err| {
            let info = session.error_info();
            (format!("{err:?}: {}", info.comment), info.no_room_reason())
        })
    };
    done.map(|()| volume(&k, &mut b))
}

#[test]
fn a_cliff_overflows_variable_round_holds_its_law_on_its_touch_circles() {
    // the fork's 5e8c9c1 build (SmFilletExecutive as SmApiVariableRadiusFillet runs it),
    // the exact circles: here to the last digit printed
    for (r0, r1, fork) in [
        (2.0, 11.0, 5802.4435349841779),
        (11.0, 2.0, 5802.4435208241775),
        (2.0, 20.0, 5503.8871700927575),
        (2.0, 30.0, 5184.0573578203966),
        (2.0, 50.0, 4693.6093103438334),
    ] {
        let v = cliff_round(kernel_5e8c9c1(), r0, r1);
        eprintln!("{r0} -> {r1}: {v:?} fork {fork}");
        let v = v.expect("builds");
        assert!(rel(v, fork) < 1e-12, "{r0} -> {r1}: {v} vs {fork}");
    }
    // the fork's d01d7b0 and 58d03ef builds, the approximated arcs
    for (r0, r1, fork) in [
        (2.0, 11.0, 5802.4277529973015),
        (11.0, 2.0, 5802.4277531390981),
        (2.0, 20.0, 5503.8406897389505),
        (2.0, 30.0, 5184.0105363417642),
        (2.0, 50.0, 4693.4880446800344),
    ] {
        let v = cliff_round(kernel_d01d7b0(), r0, r1);
        eprintln!("{r0} -> {r1}: {v:?} fork {fork}");
        let v = v.expect("builds");
        // within 1.4e-8 (2 -> 20): the subtraction's volume; the sections match the fork
        // to 1e-15 on the cone and the spline round
        assert!(rel(v, fork) < 1e-7, "{r0} -> {r1}: {v} vs {fork}");
    }
}

/// A variable round with a curvature-continuous section (`SmApiVariableRadiusFillet` with
/// cross section 2, continuity 2): its parameters take `SetFilletParameters`' default
/// thumbweight, which with the G^n section is the fork's `SM_GN_DEFAULT_THUMBWEIGHT`, not
/// upstream's 1.0 (which the G^n section refuses as invalid input). The fork's d01d7b0
/// build accepts the parameters (`SetFilletParameters` 1000) and fails the fillet (1001,
/// its G^n section is built for constant radii only).
#[test]
fn a_variable_blend_curve_round_takes_the_gn_default_thumbweight() {
    let mut k = kernel(true, true);
    let mut b = create_box(&mut k, Vec3::ZERO, 20.0, 30.0, 20.0).unwrap();
    let (e, reversed) = edge_between(
        &k,
        &b,
        Point3::new(0.0, 30.0, 20.0),
        Point3::new(20.0, 30.0, 20.0),
    );
    let (r0, r1) = if reversed { (5.0, 1.0) } else { (1.0, 5.0) };
    let mut session = FilletSession::new(&mut k, &mut b).unwrap();
    session
        .prepare_variable_radius_edges(&mut k, &[e], r0, FilletLaw::linear(r0, r1, None), 2, 2)
        .expect("the parameters are accepted");
    let done = session.do_fillet(&mut k);
    eprintln!("{done:?}: {}", session.error_info().comment);
    assert!(done.is_err());
}

/// The cliff overflows' volumes in closed form (the box less the corner between the planes
/// and the balls' cone, cut by the 10 wide top's far face), by Gauss-Legendre quadrature of
/// the area each plane across the edge cuts above the union of the balls' discs: 2 -> 11
/// (and 11 -> 2) to 1e-5 (80 and 160 panels of 16 points along the edge agree to 9e-6),
/// the balls continued past the edge's ends as the cone is; the same quadrature gives the
/// cone's [`CONE_VOLUME`] to 5e-13. The approximated arcs (d01d7b0) are 1.5e-2 under it.
const CLIFF_2_11_VOLUME: f64 = 5_802.443_142;

/// WR-711 (`fillet_exact_arc_spans`) with every fix: the linear round stays on its cone and
/// the cliff overflow of 2 -> 11 on its closed form.
#[test]
fn wr711_linear_rounds_keep_their_closed_forms() {
    let v = cone_round(kernel(true, true));
    eprintln!("cone {v} vs {CONE_VOLUME}, rel {}", rel(v, CONE_VOLUME));
    assert!(rel(v, CONE_VOLUME) < 1e-9, "{v}");
    for (r0, r1) in [
        (2.0, 11.0),
        (11.0, 2.0),
        (2.0, 20.0),
        (2.0, 30.0),
        (2.0, 50.0),
    ] {
        let v = cliff_round(kernel(true, true), r0, r1).expect("builds");
        eprintln!(
            "cliff {r0} -> {r1}: {v:.10}, rel to 2 -> 11's {}",
            rel(v, CLIFF_2_11_VOLUME)
        );
        if r1 == 11.0 || r0 == 11.0 {
            assert!(rel(v, CLIFF_2_11_VOLUME) < 1e-7, "{r0} -> {r1}: {v}");
        }
    }
}
