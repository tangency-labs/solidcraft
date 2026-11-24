//! WR-685 addendum (#2250; the fork's tangency-labs/smlib d2a6125, f2ec002, e9560e3 and
//! 2714d37 in d01d7b0, rebased from 6b346f6, 5db8feb, fb5c060 and d622107): a variable
//! round is the union of its balls (on every kernel since the G^n section's flag,
//! `fillet_gn_section_generator`, was retired: WR-685 addendum 2026-11-06). Its
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
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)]

#[path = "support/volume_leak.rs"]
mod volume_leak;
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_executive::NoRoomReason;
use solidcore::blend::fillet_standard_solver::p1::FilletLaw;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::extent1d::Extent1d;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point2, Point3, Vec3};
use solidcore::nurbs::b_spline_curve::p1::{
    BSplineCurve, BSplineCurveForm, CanonicalData, KnotType,
};
use solidcore::primitive::api_primitives::create_box;
use solidcore::surface::surface::p2::PLANE_TYPE;
use solidcore::topology::brep::p1::faces;
use solidcore::topology::face::p2 as face;
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

/// The kernel: every fix on (the fork's own behaviour) when `fork`, else the default.
fn kernel(fork: bool) -> Kernel {
    Kernel {
        fixes: if fork {
            UpstreamFixes::all()
        } else {
            UpstreamFixes::default()
        },
        ..Kernel::default()
    }
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
        let v = cone_round(kernel(fork));
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
    let v = cone_round(kernel(true));
    assert!(rel(v, 11956.222433581395) < 1e-9, "{v}");
}

#[test]
fn folding_balls_are_refused_naming_the_radius() {
    let (err, reason) = spline_round(kernel(true), FOLDS).expect_err("a folded round is refused");
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
    let (err, reason) = spline_round(kernel(true), [2.0, 2.0, 13.2, 3.0, 2.0])
        .expect_err("a folded round is refused");
    assert_eq!(reason, Some(NoRoomReason::BallsFold));
    assert!(
        err.contains("fold over one another where its radius is 7.838673:"),
        "{err}"
    );
}

/// The progressive law's round is within 3.1e-8 of its balls' envelope (5957.718502, the
/// quadrature of `fork_ea59e9c_section_orders`' `HOLDS_VOLUME`): its exact circles take one
/// arc count (WR-711). The fork's 5e8c9c1, whose circles change arc count, gives
/// 5957.716621492882, 3.2e-7 under it.
#[test]
fn a_progressive_spline_round_builds() {
    let v = spline_round(kernel(true), HOLDS).expect("builds");
    eprintln!(
        "{v} vs the envelope's 5957.718502, rel {}",
        rel(v, 5957.718502)
    );
    assert!(rel(v, 5957.718502) < 3.1e-8, "{v}");
}

/// The box 10 x 20 x 30 after the linear law `r0` to `r1` from y = 0 rounds its top edge
/// x = 0, z = 30: the rail on the 10 wide top runs past its far edge, a cliff overflow
/// (WR-631) whose round must hold its law
/// (`sm_HoldsItsLaw`).
fn cliff_round(k: Kernel, r0: f64, r1: f64) -> Result<f64, (String, Option<NoRoomReason>)> {
    cliff_rounded(k, r0, r1).map(|(k, mut b)| volume(&k, &mut b))
}

/// [`cliff_round`]'s kernel and body.
fn cliff_rounded(
    mut k: Kernel,
    r0: f64,
    r1: f64,
) -> Result<(Kernel, Brep), (String, Option<NoRoomReason>)> {
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
    done.map(|()| (k, b))
}

#[test]
fn a_cliff_overflows_variable_round_holds_its_law_on_its_touch_circles() {
    // `fork` and `want` are records. `fork`: the fork's 5e8c9c1 build (SmFilletExecutive as
    // SmApiVariableRadiusFillet runs it), the exact circles, to the last digit printed; it
    // leaks through that body's tolerant edges as the old integrator did (#16735). `want`:
    // #16737's closed-gap volume of its own body, pinned until #17673, as the Gauss-Kronrod
    // trim pieces integrate it (forced to accuracy 1e-11 and 1e-12 within 6.3e-14 of
    // themselves, the Runge-Kutta trim pieces forced to 1e-12 within 7.7e-14; the
    // Runge-Kutta default figures were 1.5e-10 to 1.6e-9 off). Neither body can be
    // rebuilt here, so neither record bounds anything but this body (`want`, below); the
    // fork's figure is printed against the closed form only.
    //
    // #17673's crease trim joins each crease to the sheet's end edge at their true meet, so
    // the round's tolerant edges close up: the widest strip over its tolerance falls from
    // 0.492 (2 -> 11 on #16737's body) to 0.019 (0.061, 0.087, 0.078 for 2 -> 20, 30, 50),
    // and the old leak bound from 8.2e-3 to 3.1e-5 (MEASURED).
    //
    // `want`'s 1e-12 pin holds for 2 -> 11 and 11 -> 2 (moved 9.0e-14, 2.2e-13). For
    // 2 -> 20, 30, 50 the volume moved 1.15e-8, 2.71e-8, 1.41e-7 from `want` (rel 2.1e-12,
    // 5.2e-12, 3.0e-11): checked within this body's own old leak bound, live.
    //
    // Every row is checked against the closed form (`closed`, as
    // `wr631_cliff_overflow_construction`, checked against `cliff_closed_form`), from this
    // body's live figures only. The body's round is a B-spline skin through the balls'
    // circles, not their cone: it keeps the volume between skin and cone (`skin_off_its_cone`;
    // 2 -> 11, 20, 30, 50: -2.30e-7, -2.73e-7, +9.95e-8, -2.890e-6), which is taken off the
    // difference. What is left, its closed region against the solid it bounds, differs only
    // inside its edges' tubes and vertices' balls, `sum pi tol^2 L + sum 4/3 pi tol^3`
    // (#16735's derivation), when every strip and fan lies within its stored tolerance
    // (asserted below); plus the volume's quadrature share at 1e-8 (twice its difference from
    // the volume at 1e-10); plus the closed form's own spread ([`CLOSED_FORM_SPREAD`]).
    // ARGUED, not proven: the strips' widths are sampled (`volume_leak`), the quadrature
    // share is an estimate, and the skin's volume off the cone is a sampled sum.
    for (r0, r1, fork, want, pinned, closed) in [
        (
            2.0,
            11.0,
            5802.4435349841779,
            5802.443145192929,
            true,
            5_802.443_145_423,
        ),
        (
            11.0,
            2.0,
            5802.4435208241775,
            5802.443145280857,
            true,
            5_802.443_145_423,
        ),
        (
            2.0,
            20.0,
            5503.8871700927575,
            5503.887121258945,
            false,
            5_503.887_121_531,
        ),
        (
            2.0,
            30.0,
            5184.0573578203966,
            5184.0572736820695,
            false,
            5_184.057_273_581,
        ),
        (
            2.0,
            50.0,
            4693.6093103438334,
            4693.60888697157,
            false,
            4_693.608_889_860,
        ),
    ] {
        let (k, mut b) = cliff_rounded(kernel(true), r0, r1).expect("builds");
        let v = volume(&k, &mut b);
        eprintln!(
            "{r0} -> {r1}: {v:?}, want {want} (moved {:e}), closed {closed} (off {:e}), \
             fork {fork} (off the closed form {:e}, a record)",
            v - want,
            v - closed,
            fork - closed
        );
        let l = volume_leak::leak(&k, &mut b, 1e-10, 8);
        assert!(
            l.worst_gap_over_tolerance < 1.0 && l.worst_fan_over_tolerance < 1.0,
            "{r0} -> {r1}: strip / tol {}, fan / tol {}",
            l.worst_gap_over_tolerance,
            l.worst_fan_over_tolerance
        );
        if pinned {
            assert!(rel(v, want) < 1e-12, "{r0} -> {r1}: {v} vs {want}");
        }
        // `closed` is the cone's own volume to its print's half ulp (the section's closed form,
        // `cliff_closed_form`; ARGUED to 1.3e-10 by its quadrature's panel agreement, and a
        // 50-digit evaluation agrees)
        let exact = cliff_closed_form(r0, r1);
        assert!(
            (closed - exact).abs() <= 5e-10,
            "{r0} -> {r1}: the closed form is {exact}, not {closed}"
        );
        let bound = l.new_bound + 2.0 * (v - l.volume).abs() + CLOSED_FORM_SPREAD;
        // the round's skin is a B-spline through the balls' circles, not their cone: the body
        // differs from the closed form by the volume between the two, which the bound does not
        // hold (2 -> 50: -2.890e-6). MEASURED: a midpoint sum, whose grid halved moves it by
        // under 1% of the bound; the skin itself must stay on the cone ([`SKIN_DEPTH`])
        let (skin, deepest) = skin_off_its_cone(&k, &b, r0, r1, 800);
        let (skin_half, _) = skin_off_its_cone(&k, &b, r0, r1, 400);
        eprintln!(
            "{r0} -> {r1}: skin off its cone {skin:e} (grid halved {skin_half:e}), {deepest:e} deep"
        );
        assert!(
            deepest <= SKIN_DEPTH,
            "{r0} -> {r1}: the skin is {deepest:e} off its balls' cone"
        );
        assert!(
            (skin - skin_half).abs() < 1e-2 * bound,
            "{r0} -> {r1}: {skin} vs {skin_half}"
        );
        assert!(
            (v - closed - skin).abs() <= bound,
            "{r0} -> {r1}: {v} vs the closed form {closed} and the skin's {skin:e}, off {:e} past {bound:e}",
            (v - closed - skin).abs()
        );
        // this body moved from #16737's closed volume within its own old leak
        volume_leak::assert_moved_by_the_old_leak(&k, &mut b, want, v, 1e-8);
    }
}

/// The signed distance of `p` from the balls' cone of the linear law `r0` to `r1` along the
/// cliff round's edge (x = 0, z = 30, y from 0 to 20): positive outside the cone.
fn off_the_cone(p: Point3, r0: f64, r1: f64) -> f64 {
    let k = (r1 - r0) / 20.0;
    let m = (1.0 + 2.0 * k * k).sqrt();
    // the apex (r = 0), and the axis towards the growing balls
    let apex = Point3::new(0.0, -r0 / k, 30.0);
    let axis = Vec3::new(k.abs(), k.signum(), -k.abs()) * (1.0 / m);
    let (sin, cos) = (k.abs() / m, (1.0 + k * k).sqrt() / m);
    let w = p - apex;
    let along = w.dot(axis);
    (w - axis * along).length() * cos - along * sin
}

/// The volume between the cliff round's skin (its one face on no plane) and the balls' cone,
/// inside the box: `sum off_the_cone |S_u x S_v| du dv` over the midpoints of an `n x n` grid
/// of the skin's domain (positive where the skin keeps more than the balls), and the
/// farthest of those grid points from the cone.
fn skin_off_its_cone(k: &Kernel, b: &Brep, r0: f64, r1: f64, n: usize) -> (f64, f64) {
    let mut skins = faces(b, None).unwrap().into_iter().filter(|&f| {
        !face::surface(b, f)
            .unwrap()
            .unwrap()
            .is_kind_of(PLANE_TYPE)
            .unwrap()
    });
    let f = skins.next().expect("the round");
    assert!(skins.next().is_none(), "one round");
    let s = face::surface(b, f).unwrap().unwrap();
    let d = s.natural_uv_domain_if_built();
    let (lo, hi) = (d.min(), d.max());
    let (du, dv) = ((hi.x - lo.x) / n as f64, (hi.y - lo.y) / n as f64);
    let at = |u: f64, v: f64| s.evaluate_point(k, Point2::new(u, v)).unwrap();
    let mut sum = 0.0;
    let mut deepest = 0.0f64;
    for i in 0..n {
        for j in 0..n {
            let (u, v) = (lo.x + (i as f64 + 0.5) * du, lo.y + (j as f64 + 0.5) * dv);
            let p = at(u, v);
            if !(0.0..=10.0).contains(&p.x)
                || !(0.0..=20.0).contains(&p.y)
                || !(0.0..=30.0).contains(&p.z)
            {
                continue;
            }
            let (hu, hv) = (1e-6 * (hi.x - lo.x), 1e-6 * (hi.y - lo.y));
            let su = (at(u + hu, v) - at(u - hu, v)) * (0.5 / hu);
            let sv = (at(u, v + hv) - at(u, v - hv)) * (0.5 / hv);
            let off = off_the_cone(p, r0, r1);
            deepest = deepest.max(off.abs());
            sum += off * su.cross(sv).length() * du * dv;
        }
    }
    (sum, deepest)
}

/// The cliff round's volume in closed form: the box 10 x 20 x 30 less, in each plane y across
/// the edge, the corner its balls' cone leaves. With `u = x`, `v = 30 - z` and the law
/// `r = r0 + c y`, the balls `(u - r)^2 + (y - t)^2 + (v - r)^2 <= r(t)^2` cover a point when
/// the least over t of that quadratic in t is at most 0; that least is 0 on the conic
/// `m (u^2 + v^2 - 2 r0 (u + v) + r0^2 + y^2) = (c (u + v) + y - r0 c)^2`, `m = 1 + c^2`, whose
/// branch nearer the corner is
/// `v = h(u) = (r0 m + c w) - sqrt((r0 m + c w)^2 - m ((u - r0)^2 + y^2) + w^2)`,
/// `w = c u + y - r0 c`. It touches v = 0 at u = r(y) and, by its symmetry in u and v, u = 0
/// at v = r(y), falling between them. So the corner's area is `int_0^min(10, r) min(30, h)`,
/// and h = 30 at u = h(30) when r > 30 (on these laws the arc leaves the bottom before the far
/// face: asserted). Gauss-Legendre on the pieces between the kinks r = 10 and r = 30, with
/// `u = s^2` taking out h's square root at u = 0; 4 and 8 panels of 20 points agree to 1e-13
/// of the corner (asserted; ARGUED, the agreement is observed, not bounded). A 50-digit evaluation of the same integrals gives 5802.443145422674,
/// 5503.887121530622, 5184.057273580749 and 4693.608889859801 for 2 -> 11, 20, 30, 50, and
/// the cone's 11956.222426735870 (as [`CONE_VOLUME`]).
fn cliff_closed_form(r0: f64, r1: f64) -> f64 {
    let (a, b) = (cliff_corner(r0, r1, 4), cliff_corner(r0, r1, 8));
    assert!((a - b).abs() < 1e-13 * b, "{a} vs {b}");
    6000.0 - b
}

/// The volume of the corner [`cliff_closed_form`] takes off, on `panels` panels per piece.
fn cliff_corner(r0: f64, r1: f64, panels: usize) -> f64 {
    let (w_far, h_bottom, len) = (10.0, 30.0, 20.0);
    let c = (r1 - r0) / len;
    let m = 1.0 + c * c;
    let h = |u: f64, y: f64| {
        let w = c * u + y - r0 * c;
        let b = r0 * m + c * w;
        b - (b * b - m * ((u - r0).powi(2) + y * y) + w * w)
            .max(0.0)
            .sqrt()
    };
    let (x, wt) = gauss_legendre(20);
    let quad = |f: &dyn Fn(f64) -> f64, a: f64, b: f64| {
        let step = (b - a) / panels as f64;
        let mut sum = 0.0;
        for p in 0..panels {
            let mid = a + (p as f64 + 0.5) * step;
            for (xi, wi) in x.iter().zip(&wt) {
                sum += wi * 0.5 * step * f(mid + 0.5 * step * xi);
            }
        }
        sum
    };
    let area = |y: f64| {
        let r = r0 + c * y;
        let top = r.min(w_far);
        let on_s = |s: f64| h(s * s, y) * 2.0 * s;
        if r <= h_bottom {
            quad(&on_s, 0.0, top.sqrt())
        } else {
            let u30 = h(h_bottom, y);
            assert!(u30 < top, "the arc meets the far face's bottom edge");
            h_bottom * u30 + quad(&on_s, u30.sqrt(), top.sqrt())
        }
    };
    let mut cuts = vec![0.0, len];
    for lim in [w_far, h_bottom] {
        let y = (lim - r0) / c;
        if y > 0.0 && y < len {
            cuts.push(y);
        }
    }
    cuts.sort_by(f64::total_cmp);
    cuts.windows(2).map(|p| quad(&area, p[0], p[1])).sum()
}

/// The `n` point Gauss-Legendre rule on [-1, 1] (Newton on the Legendre polynomial).
fn gauss_legendre(n: usize) -> (Vec<f64>, Vec<f64>) {
    let mut x = Vec::new();
    let mut w = Vec::new();
    for i in 1..=n {
        let mut z = (std::f64::consts::PI * (i as f64 - 0.25) / (n as f64 + 0.5)).cos();
        let mut dp = 1.0;
        for _ in 0..100 {
            let (mut p0, mut p1) = (1.0, z);
            for j in 2..=n {
                let jf = j as f64;
                (p0, p1) = (p1, ((2.0 * jf - 1.0) * z * p1 - (jf - 1.0) * p0) / jf);
            }
            dp = n as f64 * (z * p1 - p0) / (z * z - 1.0);
            let dz = p1 / dp;
            z -= dz;
            if dz.abs() < 1e-16 {
                break;
            }
        }
        x.push(z);
        w.push(2.0 / ((1.0 - z * z) * dp * dp));
    }
    (x, w)
}

/// How far the cliff rounds' skins may lie from their balls' cone, inside the box (MEASURED:
/// 2.4e-9 for 2 -> 11, 1.1e-8 for 2 -> 50, the deepest; their construction's approximation
/// tolerance is far looser, so this pins the skins as built, with twice the room).
const SKIN_DEPTH: f64 = 2.2e-8;

/// The closed forms' own spread: `wr631_cliff_overflow_construction`'s Gauss-Legendre
/// quadrature at 5 and 10 panels along the edge agree to 1e-9, printed to 1e-9 (half an
/// ulp of the print, 5e-10, added). Each is within 3.8e-10 of [`cliff_closed_form`]
/// (asserted to 5e-10), so this holds with room.
const CLOSED_FORM_SPREAD: f64 = 1.5e-9;

/// A variable round with a curvature-continuous section (`SmApiVariableRadiusFillet` with
/// cross section 2, continuity 2): its parameters take `SetFilletParameters`' default
/// thumbweight, which with the G^n section is the fork's `SM_GN_DEFAULT_THUMBWEIGHT`, not
/// upstream's 1.0 (which the G^n section refuses as invalid input). The fork's d01d7b0
/// build accepts the parameters (`SetFilletParameters` 1000) and fails the fillet (1001,
/// its G^n section is built for constant radii only).
#[test]
fn a_variable_blend_curve_round_takes_the_gn_default_thumbweight() {
    let mut k = kernel(true);
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

/// WR-711 (the exact circles' one arc count) with every fix: the linear round stays on its cone and
/// the cliff overflow of 2 -> 11 on its closed form.
#[test]
fn wr711_linear_rounds_keep_their_closed_forms() {
    let v = cone_round(kernel(true));
    eprintln!("cone {v} vs {CONE_VOLUME}, rel {}", rel(v, CONE_VOLUME));
    assert!(rel(v, CONE_VOLUME) < 1e-9, "{v}");
    for (r0, r1) in [
        (2.0, 11.0),
        (11.0, 2.0),
        (2.0, 20.0),
        (2.0, 30.0),
        (2.0, 50.0),
    ] {
        let v = cliff_round(kernel(true), r0, r1).expect("builds");
        eprintln!(
            "cliff {r0} -> {r1}: {v:.10}, rel to 2 -> 11's {}",
            rel(v, CLIFF_2_11_VOLUME)
        );
        if r1 == 11.0 || r0 == 11.0 {
            assert!(rel(v, CLIFF_2_11_VOLUME) < 1e-7, "{r0} -> {r1}: {v}");
        }
    }
}
