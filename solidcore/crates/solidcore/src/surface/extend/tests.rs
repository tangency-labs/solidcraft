use super::*;
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::vector2d::Vec2;
use crate::geom::vector3d::Vec3;
use crate::math::EFF_ZERO_PARAM;
use crate::nurbs::knots::Side;
use crate::nurbs::srf_tool::p1::extract_iso_crv;
use crate::surface::cone::Cone;
use crate::surface::plane::Plane;
use crate::surface::procedural_blend::ProceduralBlend;
use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, PI};

fn k() -> Kernel {
    Kernel::default()
}

/// A B-spline surface from homogeneous rows `rows[i along u][j along v]`, `w = None` for a
/// polynomial one.
fn spline(
    rows: &[Vec<[f64; 4]>],
    p: usize,
    q: usize,
    u: &[f64],
    v: &[f64],
    rational: bool,
) -> Surface {
    let net: Vec<Vec<CPoint>> = rows
        .iter()
        .map(|r| {
            r.iter()
                .map(|&[x, y, z, w]| CPoint::new(x, y, z, if rational { w } else { NOW }))
                .collect()
        })
        .collect();
    let n = Net::new(net, p, q, u.to_vec(), v.to_vec()).unwrap();
    Surface::from(BSplineSurface::from_gw_nurb(&n))
}

/// The parabola `y = x^2`, `x` in `[0, 1]` along u, extruded along z over `[0, 1]` in v.
fn parabola() -> Surface {
    let row = |x: f64, y: f64| vec![[x, y, 0.0, 1.0], [x, y, 1.0, 1.0]];
    spline(
        &[row(0.0, 0.0), row(0.5, 0.0), row(1.0, 1.0)],
        2,
        1,
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        &[0.0, 0.0, 1.0, 1.0],
        false,
    )
}

/// A quarter circle of radius 2 about z along u (rational), extruded along z over `[0, 1]`.
fn quarter() -> Surface {
    let h = FRAC_1_SQRT_2;
    let row = |x: f64, y: f64, w: f64| vec![[w * x, w * y, 0.0, w], [w * x, w * y, w, w]];
    spline(
        &[row(2.0, 0.0, 1.0), row(2.0, 2.0, h), row(0.0, 2.0, 1.0)],
        2,
        1,
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        &[0.0, 0.0, 1.0, 1.0],
        true,
    )
}

fn dom(s: &Surface) -> Extent2d {
    s.natural_uv_domain_with_nurb(&k()).unwrap()
}

/// The result's points along u at the side's middle v, over its extension.
fn past(e: &Extended, side: BoxSide) -> Vec<Vec3> {
    let d = dom(&e.surface);
    let (a, b) = if side.is_high() {
        (e.dom.max().x, d.max().x)
    } else {
        (e.dom.min().x, d.min().x)
    };
    (0..=8)
        .map(|i| {
            let u = a + (b - a) * f64::from(i) / 8.0;
            e.surface.evaluate_point(&k(), Vec2::new(u, 0.5)).unwrap()
        })
        .collect()
}

/// The parabola's arc length from `x = 0` to `x`.
fn arc(x: f64) -> f64 {
    0.5 * x * (1.0 + 4.0 * x * x).sqrt() + 0.25 * (2.0 * x).asinh()
}

/// The enclosure holds `d` within the stop: `max(1e-9 d, its truncation and rounding parts)`,
/// every leaf converged, the truncation part at most `1e-9 d` and the rounding part at most
/// `1e-6 d` (owner); the source kept and the extension's bound finite.
fn meets(e: &Extended, d: f64) {
    let a = e.across;
    assert!(a.converged, "{a:?}");
    assert!(a.truncation <= 1e-9 * d, "truncation {a:?} for {d}");
    assert!(a.rounding <= 1e-6 * d, "rounding {a:?} for {d}");
    assert!(
        e.extension <= 1e-6 * d,
        "extension bound {} for {d}",
        e.extension
    );
    let b = a.enclosure;
    let tol = (1e-9 * d).max(a.truncation + a.rounding);
    assert!(b.lower >= d - tol && b.upper <= d + tol, "{b:?} for {d}");
    assert_eq!(e.source, 0.0);
    assert!(e.extension >= 0.0, "{}", e.extension);
}

fn is_spline(s: &Surface) -> bool {
    matches!(s.kind(), SurfaceKind::BSpline(_))
}

#[test]
fn natural_continues_the_parabola_on_itself_at_both_ends() {
    let s = parabola();
    let d = 0.75;
    let e = s
        .extended_by_distance(&k(), &dom(&s), BoxSide::UMax, Continuity::Natural, d)
        .unwrap()
        .expect("extended");
    assert!(is_spline(&e.surface));
    meets(&e, d);
    let pts = past(&e, BoxSide::UMax);
    for p in &pts {
        assert!((p.y - p.x * p.x).abs() < 1e-12, "{p:?}");
    }
    // the closed form: the arc from x = 1 to the end is d
    let end = pts.last().unwrap().x;
    assert!((arc(end) - arc(1.0) - d).abs() < 1e-7, "{end}");
    // the low end: x < 0 on the same parabola
    let e = s
        .extended_by_distance(&k(), &dom(&s), BoxSide::UMin, Continuity::Natural, d)
        .unwrap()
        .expect("extended");
    meets(&e, d);
    let pts = past(&e, BoxSide::UMin);
    for p in &pts {
        assert!((p.y - p.x * p.x).abs() < 1e-12, "{p:?}");
    }
    assert!((arc(-pts.last().unwrap().x) - d).abs() < 1e-7);
}

#[test]
fn curvature_on_a_quadratic_is_its_natural_continuation() {
    // a quadratic row matching value, slope and second derivative is the same quadratic
    let s = parabola();
    let e = s
        .extended_by_distance(&k(), &dom(&s), BoxSide::UMax, Continuity::Curvature, 0.4)
        .unwrap()
        .expect("extended");
    assert!(is_spline(&e.surface));
    meets(&e, 0.4);
    for p in past(&e, BoxSide::UMax) {
        assert!((p.y - p.x * p.x).abs() < 1e-12, "{p:?}");
    }
}

#[test]
fn tangent_is_the_mirror_of_the_boundary_piece() {
    let s = parabola();
    let e = s
        .extended_by_distance(&k(), &dom(&s), BoxSide::UMax, Continuity::Tangent, 0.4)
        .unwrap()
        .expect("extended");
    meets(&e, 0.4);
    // the extension at 1 + t is C(1 - t) mirrored in the plane through (1, 1) normal to the
    // tangent (1, 2)
    let ext = dom(&e.surface).max().x - 1.0;
    let n = Vec3::new(1.0, 2.0, 0.0) / 5f64.sqrt();
    let o = Vec3::new(1.0, 1.0, 0.5);
    for i in 1..=4 {
        let t = ext * f64::from(i) / 4.0;
        let c = Vec3::new(1.0 - t, (1.0 - t) * (1.0 - t), 0.5);
        let m = c - n * (2.0 * (c - o).dot(n));
        let got = e
            .surface
            .evaluate_point(&k(), Vec2::new(1.0 + t, 0.5))
            .unwrap();
        assert!(got.distance(m) < 1e-12, "{got:?} vs {m:?}");
    }
}

/// The unit tangent and the curvature vector of the u iso curve at v = 0.5, either side of `u`.
fn jets(s: &Surface, u: f64) -> [(Vec3, Vec3); 2] {
    let SurfaceKind::BSpline(b) = s.kind() else {
        panic!()
    };
    let c = extract_iso_crv(b.gw_nurb().unwrap(), 0.5, Direction::U).unwrap();
    [Side::Left, Side::Right].map(|side| {
        let d = c.derivs(u, side, 2).unwrap();
        let (d1, d2) = (d[1], d[2]);
        let t = d1 / d1.length();
        let kappa = (d2 - t * d2.dot(t)) / d1.dot(d1);
        (t, kappa)
    })
}

#[test]
fn a_circle_continues_on_itself_and_mirrors_tangent_and_curvature_continuous() {
    let s = quarter();
    let r = 2.0;
    // natural: the conic continued is the circle; the across length is the arc
    let e = s
        .extended_by_distance(&k(), &dom(&s), BoxSide::UMax, Continuity::Natural, 1.0)
        .unwrap()
        .expect("extended");
    assert!(is_spline(&e.surface));
    meets(&e, 1.0);
    for p in past(&e, BoxSide::UMax) {
        assert!((p.x.hypot(p.y) - r).abs() < 1e-12, "{p:?}");
    }
    let end = *past(&e, BoxSide::UMax).last().unwrap();
    // from angle 90 by an arc of 1 on radius 2
    let want = FRAC_PI_2 + 0.5;
    assert!((end.y.atan2(end.x) - want).abs() < 1e-7, "{end:?}");
    // curvature within the boundary span: the homogeneous quadratic matched to second order is
    // the circle's own
    let e = s
        .extended_by_distance(&k(), &dom(&s), BoxSide::UMax, Continuity::Curvature, 1.0)
        .unwrap()
        .expect("extended");
    meets(&e, 1.0);
    for p in past(&e, BoxSide::UMax) {
        assert!((p.x.hypot(p.y) - r).abs() < 1e-11, "{p:?}");
    }
    // tangent: a different conic, tangent continuous at the joint
    let e = s
        .extended_by_distance(&k(), &dom(&s), BoxSide::UMax, Continuity::Tangent, 1.0)
        .unwrap()
        .expect("extended");
    meets(&e, 1.0);
    let [(tl, _), (tr, _)] = jets(&e.surface, 1.0);
    assert!(tl.distance(tr) < 1e-12, "{tl:?} {tr:?}");
}

#[test]
fn curvature_is_curvature_continuous_across_interior_knots() {
    // a cubic row with an interior knot, extended past more than its last span
    let row = |x: f64, y: f64| vec![[x, y, 0.0, 1.0], [x, y, 1.0, 1.0]];
    let s = spline(
        &[
            row(0.0, 0.0),
            row(1.0, 2.0),
            row(2.0, -1.0),
            row(3.0, 1.0),
            row(4.0, 0.5),
        ],
        3,
        1,
        &[0.0, 0.0, 0.0, 0.0, 0.4, 1.0, 1.0, 1.0, 1.0],
        &[0.0, 0.0, 1.0, 1.0],
        false,
    );
    let e = s
        .extended_by_distance(&k(), &dom(&s), BoxSide::UMax, Continuity::Curvature, 2.0)
        .unwrap()
        .expect("extended");
    meets(&e, 2.0);
    let [(tl, kl), (tr, kr)] = jets(&e.surface, 1.0);
    assert!(tl.distance(tr) < 1e-12, "{tl:?} {tr:?}");
    assert!(
        kl.distance(kr) < 1e-9 * kl.length().max(1.0),
        "{kl:?} {kr:?}"
    );
    // longer than the surface's range: mirrored in steps, still continuous at the joint
    let e = s
        .extended_by_distance(&k(), &dom(&s), BoxSide::UMax, Continuity::Tangent, 30.0)
        .unwrap()
        .expect("extended");
    meets(&e, 30.0);
    let [(tl, _), (tr, _)] = jets(&e.surface, 1.0);
    assert!(tl.distance(tr) < 1e-12);
}

#[test]
fn a_weight_reaching_zero_is_refused_naming_the_distance_that_works() {
    // a rational cubic with weights 1, 3.4, 1.8, 1: continued naturally, its second weight
    // control point reaches 0 at t = 2.25 of the span (the weight itself still 11 there)
    let row = |x: f64, y: f64, w: f64| vec![[w * x, w * y, 0.0, w], [w * x, w * y, w, w]];
    let s = spline(
        &[
            row(0.0, 0.0, 1.0),
            row(1.0, 1.0, 3.4),
            row(2.0, 1.0, 1.8),
            row(3.0, 0.0, 1.0),
        ],
        3,
        1,
        &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        &[0.0, 0.0, 1.0, 1.0],
        true,
    );
    let err = s
        .extended_by_distance(&k(), &dom(&s), BoxSide::UMax, Continuity::Natural, 1e6)
        .unwrap_err();
    let SolidError::ExtendRefused {
        refusal: ExtendRefusal::WeightVanishes,
        distance: Some(longest),
        ..
    } = err
    else {
        panic!("{err:?}");
    };
    assert!(longest > 0.0 && longest < 1e6, "{longest}");
    // the distance named works, exactly, with every weight positive
    let e = s
        .extended_by_distance(&k(), &dom(&s), BoxSide::UMax, Continuity::Natural, longest)
        .unwrap()
        .expect("extended");
    meets(&e, longest);
    let SurfaceKind::BSpline(b) = e.surface.kind() else {
        panic!()
    };
    assert!(b.gw_nurb().unwrap().rows().flatten().all(|c| c.w > 0.0));
}

#[test]
fn analytic_kinds_widen_and_keep_their_kind() {
    let kk = k();
    let pl = Axis2Placement::new(Vec3::new(1.0, 2.0, 3.0), Vec3::X, Vec3::Y);
    let mut c = Cone::create_canonical(&kk, &pl, 2.0, 2.0, 1.0, false, false).unwrap();
    c.adjust_step_uv_domain(&kk, &Extent2d::from_bounds(0.0, 0.0, 90.0, 3.0))
        .unwrap();
    let s = Surface::from(c);
    for side in BoxSide::ALL {
        let e = s
            .extended_by_distance(&kk, &dom(&s), side, Continuity::Natural, 1.5)
            .unwrap()
            .expect("extended");
        assert!(matches!(e.surface.kind(), SurfaceKind::Cone(_)), "{side:?}");
        meets(&e, 1.5);
    }
    // a plane: every continuity is its continuation, the plane itself
    let p = Surface::from(Plane::new(
        Vec3::ZERO,
        Vec3::X,
        Vec3::Y,
        Vec2::new(1.0, 1.0),
        &Extent2d::from_bounds(0.0, 0.0, 10.0, 10.0),
    ));
    for how in [
        Continuity::Natural,
        Continuity::Tangent,
        Continuity::Curvature,
    ] {
        let e = p
            .extended_by_distance(&kk, &dom(&p), BoxSide::UMax, how, 5.0)
            .unwrap()
            .expect("extended");
        assert!(matches!(e.surface.kind(), SurfaceKind::Plane(_)));
        meets(&e, 5.0);
    }
    // round a cylinder tangent: its B-spline form mirrored, a spline at bound 0
    let e = s
        .extended_by_distance(&kk, &dom(&s), BoxSide::UMax, Continuity::Tangent, 1.0)
        .unwrap()
        .expect("extended");
    assert!(is_spline(&e.surface) || matches!(e.surface.kind(), SurfaceKind::Cone(_)));
    meets(&e, 1.0);
}

#[test]
fn bad_asks_are_refused() {
    let s = parabola();
    let d = dom(&s);
    for x in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(s
            .extended_by_distance(&k(), &d, BoxSide::UMax, Continuity::Natural, x)
            .is_err());
    }
    // a side that is not the form's end
    let inner = Extent2d::from_bounds(0.0, 0.0, 0.5, 1.0);
    assert!(s
        .extended_by_distance(&k(), &inner, BoxSide::UMax, Continuity::Tangent, 0.5)
        .is_err());
    // an analytic form moved off its label
    let kk = k();
    let pl = Axis2Placement::new(Vec3::ZERO, Vec3::X, Vec3::Y);
    let mut c = Cone::create_canonical(&kk, &pl, 2.0, 2.0, 1.0, false, false).unwrap();
    c.adjust_step_uv_domain(&kk, &Extent2d::from_bounds(0.0, 0.0, 90.0, 3.0))
        .unwrap();
    let shift = Axis2Placement::new(Vec3::new(1e-6, 0.0, 0.0), Vec3::X, Vec3::Y);
    c.rev.base.transform(&shift, None).unwrap();
    let m = Surface::from(c);
    assert!(m
        .extended_by_distance(&kk, &dom(&m), BoxSide::UMax, Continuity::Tangent, 1.0)
        .is_err());
}

/// A refusal as not exact, its named distance.
fn not_exact_naming(err: SolidError) -> Option<f64> {
    match err {
        SolidError::ExtendRefused {
            refusal: ExtendRefusal::NotExact,
            distance,
            ..
        } => distance,
        e => panic!("{e:?}"),
    }
}

/// The quarter circle mirrored far: Curvature's C2 rows carry the rounding of the rows before
/// them, so past a few spans its extension bound runs past `1e-6 d`; it is refused as not
/// exact, naming a distance that works, never accepted with a wide enclosure (owner; before
/// the caps, Curvature at 20 came back with an enclosure [0, 4705] and Tangent at 1e5 with one
/// 2.4% wide). Every result met has its parts within the caps.
#[test]
fn a_far_mirror_is_met_within_the_caps_or_refused_naming_a_distance_that_works() {
    let s = quarter();
    for (how, d) in [(Continuity::Curvature, 20.0), (Continuity::Tangent, 1e5)] {
        match s.extended_by_distance(&k(), &dom(&s), BoxSide::UMax, how, d) {
            Ok(Some(e)) => meets(&e, d),
            Ok(None) => panic!("{how:?} {d}: unchanged"),
            Err(err) => {
                // not exact, or (Tangent far) a mirrored weight vanishing first: either names
                // a distance that works
                let named = match err {
                    SolidError::ExtendRefused {
                        refusal: ExtendRefusal::WeightVanishes,
                        distance,
                        ..
                    } => distance,
                    e => not_exact_naming(e),
                }
                .unwrap_or_else(|| panic!("{how:?} {d}"));
                assert!(named > 0.0 && named < d, "{how:?} {d}: {named}");
                let e = s
                    .extended_by_distance(&k(), &dom(&s), BoxSide::UMax, how, named)
                    .unwrap_or_else(|e| panic!("{how:?} {named}: {e:?}"))
                    .expect("extended");
                meets(&e, named);
            }
        }
    }
    // Tangent by 20 is built, its bound small
    let e = s
        .extended_by_distance(&k(), &dom(&s), BoxSide::UMax, Continuity::Tangent, 20.0)
        .unwrap()
        .expect("extended");
    meets(&e, 20.0);
    assert!(e.extension < 1e-9, "{}", e.extension);
}

/// Curvature is continuous in the extension parameter: across a knot of the mirror the
/// length runs on, so every distance in the range is met (REVIEW2 finding 2: before one span a
/// step, 9 fell in a gap between 8.98 and 9.53).
#[test]
fn curvature_meets_every_distance_across_a_mirror_knot() {
    let s = quarter();
    // the mirror's knots are at e = 1 and 2 (one span of length 1 a step): L(1) is about 2.0
    for d in [1.5, 1.9, 2.0, 2.1, 2.5, 3.0, 3.3] {
        let e = s
            .extended_by_distance(&k(), &dom(&s), BoxSide::UMax, Continuity::Curvature, d)
            .unwrap_or_else(|e| panic!("{d}: {e:?}"))
            .expect("extended");
        meets(&e, d);
    }
    // and the length grows with the parameter, without a jump
    let source = Local::of(
        match &s.kind() {
            SurfaceKind::BSpline(b) => b.gw_nurb().unwrap(),
            _ => panic!(),
        },
        BoxSide::UMax,
    )
    .unwrap();
    let mut last = 0.0;
    for i in 1..=60 {
        let e = 0.85 + 0.005 * f64::from(i);
        let Some(g) = source.extended(Continuity::Curvature, e).unwrap() else {
            break;
        };
        let out = Surface::from(BSplineSurface::from_gw_nurb(
            &g.surface(BoxSide::UMax).unwrap(),
        ));
        let l = out
            .length_across_side(&k(), &dom(&s), BoxSide::UMax, 1e-8)
            .unwrap()
            .enclosure
            .mid();
        assert!(
            i == 1 || (l > last && l - last < 0.2),
            "e {e}: {last} -> {l}"
        );
        last = l;
    }
}

/// Tangent's first point past the joint is `2 Pn - Pn1`, exactly, on every row (REVIEW2
/// finding 9): the tangent across the joint carries no rounding of the mirror map.
#[test]
fn tangents_first_point_is_the_exact_reflection() {
    let s = quarter();
    let SurfaceKind::BSpline(b) = s.kind() else {
        panic!()
    };
    let net = b.gw_nurb().unwrap().clone();
    let source = Local::of(&net, BoxSide::UMax).unwrap();
    let g = source.extended(Continuity::Tangent, 1.0).unwrap().unwrap();
    for (r, src) in g.rows.iter().zip(&source.rows) {
        let n = src.len();
        let (pn, pn1) = (src[n - 1], src[n - 2]);
        assert_eq!(r[n], lin(2.0, pn, -1.0, pn1));
    }
}

/// Natural past a short span amplifies the rounding (`t^p`): the extension carries a certified
/// bound that holds the result to the closed form (REVIEW2 finding 5). `y = x^3` on [0, 1] with
/// a knot at 0.999, along u, extruded along z. Far past the short span the bound runs past
/// `1e-6 d` (`t` is about `1 + d / 0.001`), so that is refused as not exact, naming a
/// distance that works.
#[test]
fn natural_carries_a_bound_that_holds_the_closed_form() {
    let a = 0.999_f64;
    let p = [[0.0, 0.0], [1.0 / 3.0, 0.0], [2.0 / 3.0, 0.0], [1.0, 1.0]];
    let mix = |x: [f64; 2], y: [f64; 2]| [(1.0 - a) * x[0] + a * y[0], (1.0 - a) * x[1] + a * y[1]];
    let pts = [
        p[0],
        mix(p[0], p[1]),
        mix(p[1], p[2]),
        mix(p[2], p[3]),
        p[3],
    ];
    let rows: Vec<Vec<[f64; 4]>> = pts
        .iter()
        .map(|&[x, y]| vec![[x, y, 0.0, 1.0], [x, y, 1.0, 1.0]])
        .collect();
    let s = spline(
        &rows,
        3,
        1,
        &[0.0, 0.0, 0.0, 0.0, a, 1.0, 1.0, 1.0, 1.0],
        &[0.0, 0.0, 1.0, 1.0],
        false,
    );
    let named = not_exact_naming(
        s.extended_by_distance(&k(), &dom(&s), BoxSide::UMax, Continuity::Natural, 10.0)
            .unwrap_err(),
    )
    .unwrap();
    assert!(named > 0.0 && named < 10.0, "{named}");
    for d in [0.5, named] {
        let e = s
            .extended_by_distance(&k(), &dom(&s), BoxSide::UMax, Continuity::Natural, d)
            .unwrap_or_else(|e| panic!("{d}: {e:?}"))
            .expect("extended");
        meets(&e, d);
        let mut off = 0.0_f64;
        for q in past(&e, BoxSide::UMax) {
            off = off.max((q.y - q.x.powi(3)).abs());
        }
        assert!(e.extension > 0.0, "{d}");
        assert!(
            off <= e.extension,
            "{d}: off {off} past the bound {}",
            e.extension
        );
    }
}

/// A length that is not measured to the stop is no measurement: not met however close
/// (owner): a leaf not converged, a truncation part past `1e-9 d`, a rounding part past
/// `1e-6 d`, an extension bound not finite, or no length at all.
///
/// The last case once expected a narrow enclosure 3e-9 off `d` to be `Met` when the stored
/// net's resolution was 4e-9 (the stop then widened by the resolution) and `Short` at 1e-9.
/// The stop is exact (#2977): the enclosure is 3e-9 off `d` and the stop is `max(1e-9 d, its
/// parts)` = 1e-9, so it is `Short` at either resolution.
#[test]
fn a_length_not_measured_to_the_stop_is_unproved() {
    let at = |lower: f64, upper: f64, truncation: f64, rounding: f64, converged: bool| Across {
        enclosure: crate::surface::side::Bracket { lower, upper },
        truncation,
        rounding,
        converged,
        resolution: 0.0,
    };
    let d = 1.0;
    assert_eq!(says(Some(&at(1.0, 1.0, 0.0, 0.0, true)), 0.0, d), Says::Met);
    assert_eq!(
        says(Some(&at(1.0, 1.0, 0.0, 0.0, false)), 0.0, d),
        Says::Unproved
    );
    assert_eq!(
        says(Some(&at(1.0, 1.0, 0.0, 0.0, true)), f64::INFINITY, d),
        Says::Unproved
    );
    assert_eq!(says(None, 0.0, d), Says::Unproved);
    // the truncation part over 1e-9 d: not met, though the enclosure holds d
    assert_eq!(
        says(Some(&at(1.0 - 1e-9, 1.0 + 1e-9, 2e-9, 0.0, true)), 0.0, d),
        Says::Unproved
    );
    // the rounding part within 1e-6 d: met; over it: not
    assert_eq!(
        says(Some(&at(1.0 - 5e-7, 1.0 + 4e-7, 1e-10, 8e-7, true)), 0.0, d),
        Says::Met
    );
    assert_eq!(
        says(Some(&at(1.0 - 1e-6, 1.0 + 1e-6, 1e-10, 2e-6, true)), 0.0, d),
        Says::Unproved
    );
    // proved short and past, however wide
    assert!(matches!(
        says(Some(&at(0.1, 0.5, 0.0, 0.4, true)), 0.0, d),
        Says::Short(_)
    ));
    assert!(matches!(
        says(Some(&at(2.0, 9.0, 0.0, 7.0, true)), 0.0, d),
        Says::Past(_)
    ));
    // a narrow enclosure off d by more than the stop: short, whatever the net's resolution
    let off = |resolution: f64| Across {
        resolution,
        ..at(1.0 - 3e-9, 1.0 - 3e-9 + 1e-15, 0.0, 1e-15, true)
    };
    assert!(matches!(says(Some(&off(4e-9)), 0.0, d), Says::Short(_)));
    assert!(matches!(says(Some(&off(1e-9)), 0.0, d), Says::Short(_)));
}

/// The stop is the measure's own: the quarter circle's and a rational cubic's lengths, their
/// truncation and rounding parts as reported, the enclosure within them of the distance.
#[test]
fn the_stop_is_the_reported_parts() {
    let s = quarter();
    let e = s
        .extended_by_distance(&k(), &dom(&s), BoxSide::UMax, Continuity::Curvature, 3.0)
        .unwrap()
        .expect("extended");
    meets(&e, 3.0);
    let a = e.across;
    let width = a.enclosure.upper - a.enclosure.lower;
    assert!(
        (width - (a.truncation + a.rounding)).abs() <= 1e-12 * 3.0,
        "{a:?}"
    );
    let row = |x: f64, y: f64, w: f64| vec![[w * x, w * y, 0.0, w], [w * x, w * y, w, w]];
    let cubic = spline(
        &[
            row(0.0, 0.0, 1.0),
            row(1.0, 1.0, 3.4),
            row(2.0, 1.0, 1.8),
            row(3.0, 0.0, 1.0),
        ],
        3,
        1,
        &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        &[0.0, 0.0, 1.0, 1.0],
        true,
    );
    let e = cubic
        .extended_by_distance(&k(), &dom(&cubic), BoxSide::UMax, Continuity::Natural, 1.0)
        .unwrap()
        .expect("extended");
    meets(&e, 1.0);
}

/// Tangent and Curvature on a rational row past every side: the quarter circle along u
/// (`UMax` and `UMin`) and along v (`VMax` and `VMin`, its net transposed). Curvature within the boundary
/// span is the circle's own (REVIEW2's missing tests).
#[test]
fn mirrors_on_a_rational_row_past_every_side() {
    let h = FRAC_1_SQRT_2;
    let circle = [(2.0, 0.0, 1.0), (2.0, 2.0, h), (0.0, 2.0, 1.0)];
    let along_v = spline(
        &[
            circle
                .iter()
                .map(|&(x, y, w)| [w * x, w * y, 0.0, w])
                .collect(),
            circle
                .iter()
                .map(|&(x, y, w)| [w * x, w * y, w, w])
                .collect(),
        ],
        1,
        2,
        &[0.0, 0.0, 1.0, 1.0],
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        true,
    );
    let u = quarter();
    for (s, side) in [
        (&u, BoxSide::UMax),
        (&u, BoxSide::UMin),
        (&along_v, BoxSide::VMax),
        (&along_v, BoxSide::VMin),
    ] {
        for how in [Continuity::Tangent, Continuity::Curvature] {
            let e = s
                .extended_by_distance(&k(), &dom(s), side, how, 1.0)
                .unwrap_or_else(|e| panic!("{side:?} {how:?}: {e:?}"))
                .expect("extended");
            meets(&e, 1.0);
            if how == Continuity::Curvature {
                let d = dom(&e.surface);
                for i in 0..=8 {
                    let t = f64::from(i) / 8.0;
                    let uv = if side.is_u() {
                        let a = if side.is_high() {
                            e.dom.max().x
                        } else {
                            e.dom.min().x
                        };
                        let b = if side.is_high() { d.max().x } else { d.min().x };
                        Vec2::new(a + (b - a) * t, 0.5)
                    } else {
                        let a = if side.is_high() {
                            e.dom.max().y
                        } else {
                            e.dom.min().y
                        };
                        let b = if side.is_high() { d.max().y } else { d.min().y };
                        Vec2::new(0.5, a + (b - a) * t)
                    };
                    let p = e.surface.evaluate_point(&k(), uv).unwrap();
                    assert!((p.x.hypot(p.y) - 2.0).abs() < 1e-11, "{side:?}: {p:?}");
                }
            }
        }
    }
}

/// The offset by `h` of the quarter cylinder of radius 2 about z.
fn offset_quarter(h: f64) -> Surface {
    let kk = k();
    Surface::from(OffsetSurface::new(&kk, h, quarter(), true, SS_UNKNOWN).unwrap())
}

#[test]
fn an_offset_surface_is_the_offset_of_its_base_extended() {
    let kk = k();
    let h = 0.5;
    let s = offset_quarter(h);
    // the offset's radius, off the source's box
    let radius = |p: Vec3| p.x.hypot(p.y);
    let r = radius(s.evaluate_point(&kk, Vec2::new(0.5, 0.5)).unwrap());
    assert!((r - 2.0).abs() > 0.4, "{r}");
    for (how, side) in [
        (Continuity::Natural, BoxSide::UMax),
        (Continuity::Natural, BoxSide::UMin),
        (Continuity::Tangent, BoxSide::VMax),
    ] {
        let e = s
            .extended_by_distance(&kk, &dom(&s), side, how, 1.0)
            .unwrap()
            .expect("extended");
        let SurfaceKind::OffsetSurface(o) = e.surface.kind() else {
            panic!("{side:?}: not an offset surface");
        };
        assert_eq!(o.offset_distance(), h);
        // the length across is measured on the base, which is the quarter extended
        meets(&e, 1.0);
        // the source's box: the source's offset; past it, Natural round the axis stays on
        // the offset cylinder (the base continued on its circle)
        let d = dom(&e.surface);
        for i in 0..=8 {
            let t = f64::from(i) / 8.0;
            let inside = Vec2::new(t, 0.5);
            let a = e.surface.evaluate_point(&kk, inside).unwrap();
            let b = s.evaluate_point(&kk, inside).unwrap();
            assert!(a.distance(b) <= 1e-12, "{side:?} {a:?} {b:?}");
            if side.is_u() {
                let u = if side.is_high() {
                    1.0 + t * (d.max().x - 1.0)
                } else {
                    d.min().x * t
                };
                let p = e.surface.evaluate_point(&kk, Vec2::new(u, 0.5)).unwrap();
                assert!(
                    (radius(p) - r).abs() <= 1e-12,
                    "{side:?} {u}: {}",
                    radius(p)
                );
            }
        }
    }
}

#[test]
fn every_side_extends_in_turn_and_closed_or_collapsed_sides_are_left() {
    let kk = k();
    let s = quarter();
    let e = s
        .extended_sides(&kk, &BoxSide::ALL, Continuity::Natural, 1.0)
        .unwrap();
    for (side, done) in BoxSide::ALL.iter().zip(e.sides) {
        let SideExtension::Extended(a) = done else {
            panic!("{side:?}: {done:?}");
        };
        let tol = (1e-9_f64).max(a.truncation + a.rounding);
        assert!((a.enclosure.mid() - 1.0).abs() <= tol, "{side:?}: {a:?}");
    }
    // every side ran along the earlier ones: the box grew on all four
    let (d0, d1) = (dom(&s), dom(&e.surface));
    assert!(d1.min().x < d0.min().x && d1.max().x > d0.max().x);
    assert!(d1.min().y < d0.min().y && d1.max().y > d0.max().y);
    // only the asked sides
    let one = s
        .extended_sides(&kk, &[BoxSide::VMax], Continuity::Tangent, 1.0)
        .unwrap();
    assert!(matches!(one.sides[0], SideExtension::NotAsked));
    assert!(matches!(one.sides[3], SideExtension::Extended(_)));
    // a full turn closes round the axis: those sides are left, the rulings extended
    let pl = Axis2Placement::new(Vec3::ZERO, Vec3::Z, Vec3::X);
    let mut c = Cone::create_canonical(&kk, &pl, 2.0, 2.0, 1.0, false, false).unwrap();
    c.adjust_step_uv_domain(&kk, &Extent2d::from_bounds(0.0, 0.0, 360.0, 3.0))
        .unwrap();
    let c = Surface::from(c);
    let e = c
        .extended_sides(&kk, &BoxSide::ALL, Continuity::Natural, 1.0)
        .unwrap();
    let closed = e
        .sides
        .iter()
        .filter(|x| matches!(x, SideExtension::Closed))
        .count();
    let grown = e
        .sides
        .iter()
        .filter(|x| matches!(x, SideExtension::Extended(_)))
        .count();
    assert_eq!((closed, grown), (2, 2), "{:?}", e.sides);
    // a side collapsed to a point (a pole) is left: the triangle with its corner at u = 0
    let row = |x: f64, y: f64| vec![[x, y, 0.0, 1.0], [x, y + x, 0.0, 1.0]];
    let t = spline(
        &[row(0.0, 0.0), row(1.0, 0.0), row(2.0, 0.0)],
        2,
        1,
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        &[0.0, 0.0, 1.0, 1.0],
        false,
    );
    let e = t
        .extended_sides(&kk, &BoxSide::ALL, Continuity::Natural, 0.5)
        .unwrap();
    assert!(
        matches!(e.sides[0], SideExtension::Collapsed),
        "{:?}",
        e.sides
    );
    assert!(
        matches!(e.sides[1], SideExtension::Extended(_)),
        "{:?}",
        e.sides
    );
}

#[test]
fn an_infinite_planes_sides_are_unbounded_and_a_finite_planes_extend_its_own_net() {
    use crate::surface::implicit::{lies_on, AnalyticSupport};
    let kk = k();
    let inf = Surface::from(Plane::from_normal(Vec3::new(0.0, 0.0, 3.0), Vec3::Z));
    let e = inf
        .extended_sides(&kk, &BoxSide::ALL, Continuity::Tangent, 4.33)
        .unwrap();
    assert!(
        e.sides
            .iter()
            .all(|x| matches!(x, SideExtension::Unbounded)),
        "{:?}",
        e.sides
    );
    let p = Surface::from(Plane::new(
        Vec3::ZERO,
        Vec3::X,
        Vec3::Y,
        Vec2::new(1.0, 1.0),
        &Extent2d::from_bounds(0.0, 0.0, 10.0, 10.0),
    ));
    // widened as a plane
    let e = p
        .extended_sides(&kk, &BoxSide::ALL, Continuity::Tangent, 4.33)
        .unwrap();
    assert!(matches!(e.surface.kind(), SurfaceKind::Plane(_)));
    let kept = inf
        .extended_sides_keeping_parameters(&kk, &BoxSide::ALL, Continuity::Ruled, 4.33)
        .unwrap();
    assert!(kept
        .sides
        .iter()
        .all(|x| matches!(x, SideExtension::Unbounded)));
    // keeping its parameters
    let e = p
        .extended_sides_keeping_parameters(&kk, &BoxSide::ALL, Continuity::Tangent, 4.33)
        .unwrap();
    assert!(
        e.sides
            .iter()
            .all(|x| matches!(x, SideExtension::Extended(_))),
        "{:?}",
        e.sides
    );
    // its own net continued: a B-spline on the plane's own parameters, on the plane
    let SurfaceKind::BSpline(grown) = e.surface.kind() else {
        panic!("not a B-spline: {:?}", e.surface.kind());
    };
    let on = AnalyticSupport::check(&kk, &p).unwrap().implicit();
    assert!(lies_on(grown, &on).unwrap());
    // one straight span each way (its joint knots removed): the plane's own 2 x 2 form
    assert_eq!(grown.gw_nurb().unwrap().cpts().len(), 2);
    assert_eq!(grown.gw_nurb().unwrap().u_knots().len(), 4);
    for (u, v) in [(0.0, 0.0), (10.0, 0.0), (3.5, 7.25), (-4.0, 14.0)] {
        let uv = Point2::new(u, v);
        // the plane's own parameters: origin + u X + v Y at scale 1
        let at = Vec3::new(u, v, 0.0);
        assert!(
            (grown.evaluate_point(uv).unwrap() - at).length() <= 1e-13,
            "{uv:?}"
        );
    }
    // the across length is the distance on every side, in the plane's own parameters
    let d = grown.gw_nurb().unwrap();
    // (the extension parameter solved to 1e-12 of the distance)
    let ends = |k: &[f64]| (k[0], *k.last().unwrap());
    for (lo, hi) in [ends(d.u_knots()), ends(d.v_knots())] {
        assert!(
            (lo + 4.33).abs() <= 1e-9 && (hi - 14.33).abs() <= 1e-9,
            "{lo} {hi}"
        );
    }
    for a in e.sides {
        let SideExtension::Extended(a) = a else {
            unreachable!()
        };
        assert!((a.enclosure.mid() - 4.33).abs() <= 1e-9 * 4.33, "{a:?}");
    }
}

#[test]
fn a_cone_extends_its_own_net_along_up_to_its_apex_and_round_its_axis_a_quarter_of_its_open_turn() {
    use crate::surface::implicit::{lies_on, AnalyticSupport};
    let kk = k();
    // radius 2 at the bottom, 1 at height 1: the apex at height 2, sqrt 2 slant past the top
    let pl = Axis2Placement::new(Vec3::ZERO, Vec3::Z, Vec3::X);
    let mut c = Cone::create_canonical(&kk, &pl, 2.0, 1.0, 1.0, false, false).unwrap();
    c.adjust_step_uv_domain(&kk, &Extent2d::from_bounds(0.0, 0.0, 90.0, 1.0))
        .unwrap();
    let c = Surface::from(c);
    let on = AnalyticSupport::check(&kk, &c).unwrap().implicit();
    let src = crate::blend::fillet_section::station::as_b_spline(&kk, &c)
        .unwrap()
        .unwrap()
        .into_owned();
    let src_net = src.gw_nurb().unwrap().clone();
    for how in [Continuity::Tangent, Continuity::Natural] {
        let e = c.extended_sides(&kk, &BoxSide::ALL, how, 5.0).unwrap();
        let SurfaceKind::BSpline(grown) = e.surface.kind() else {
            panic!("{how:?}: not a B-spline");
        };
        // the source's circles' points and knots bit for bit (along the rulings the span is
        // widened, one straight span: its end circles to rounding), the result on the cone
        let out = grown.gw_nurb().unwrap();
        let (src_u, src_v) = (src_net.u_knots(), src_net.v_knots());
        assert!(src_u.iter().all(|x| out.u_knots().contains(x)), "{how:?}");
        assert_eq!(out.v_knots().len(), 4, "{how:?}: one straight span along");
        assert_eq!(out.cpts().len(), src_net.cpts().len() + 4, "{how:?}");
        for i in 0..=4 {
            for j in 0..=4 {
                let uv = Point2::new(
                    src_u[0] + f64::from(i) / 4.0 * (src_u.last().unwrap() - src_u[0]),
                    src_v[0] + f64::from(j) / 4.0 * (src_v.last().unwrap() - src_v[0]),
                );
                let d = grown.evaluate_point(uv).unwrap() - src.evaluate_point(uv).unwrap();
                assert!(d.length() <= 1e-14, "{how:?} {uv:?}: {d:?}");
            }
        }
        assert!(lies_on(grown, &on).unwrap());
        // round the axis (u): each side a quarter of the 270 degrees left open, said so: the
        // radius at the side's middle (1.5) times 3 pi / 8
        for i in [0, 1] {
            let SideExtension::UpToLimit(a) = e.sides[i] else {
                panic!("{:?}", e.sides);
            };
            let want = 1.5 * 0.375 * PI;
            assert!((a.enclosure.mid() - want).abs() <= 1e-9 * want, "{a:?}");
        }
        // round the axis each end turned 67.5 degrees from the source's
        let (d, sd) = (grown.gw_nurb().unwrap(), src_net.clone());
        let ends =
            |n: &crate::nurbs::srf_basic::Surface| (n.u_knots()[0], *n.u_knots().last().unwrap());
        let vm = 0.5 * (sd.v_knots()[0] + *sd.v_knots().last().unwrap());
        let ((u0, u1), (s0, s1)) = (ends(d), ends(&sd));
        let crate::surface::implicit::Implicit::Cone { origin, axis, .. } = on else {
            panic!("not a cone");
        };
        let flat = |s: &BSplineSurface, u: f64| {
            let p = s.evaluate_point(Point2::new(u, vm)).unwrap() - origin;
            p - axis * axis.dot(p)
        };
        let deg = |a: Vec3, b: Vec3| a.cross(b).length().atan2(a.dot(b)).to_degrees();
        assert!((deg(flat(grown, u0), flat(&src, s0)) - 67.5).abs() <= 1e-6);
        assert!((deg(flat(grown, u1), flat(&src, s1)) - 67.5).abs() <= 1e-6);
        // along: one side runs the whole 5, the other stops at the apex
        let along = |x: &SideExtension| match x {
            SideExtension::Extended(a) => Some((false, a.enclosure.mid())),
            SideExtension::UpToLimit(a) => Some((true, a.enclosure.mid())),
            _ => None,
        };
        let mut lengths: Vec<(bool, f64)> = e.sides[2..].iter().filter_map(along).collect();
        lengths.sort_by(|a, b| a.1.total_cmp(&b.1));
        let [(true, short), (false, long)] = lengths[..] else {
            panic!("{:?}", e.sides);
        };
        assert!((long - 5.0).abs() <= 1e-9 * 5.0, "{long}");
        assert!((short - 2f64.sqrt()).abs() <= 1e-9, "{short}");
    }
}

#[test]
fn ruled_on_a_rational_row_is_the_straight_tangent_line_at_any_distance() {
    // the quarter cylinder of radius 2 ends at (0, 2) heading -x: ruled by 50, the extension
    // is the line y = 2 from x = 0 to x = -50, tangent (C1) at the joint, the source kept
    let s = quarter();
    for (side, end) in [(BoxSide::UMax, [0.0, 2.0]), (BoxSide::UMin, [2.0, 0.0])] {
        let e = s
            .extended_by_distance(&k(), &dom(&s), side, Continuity::Ruled, 50.0)
            .unwrap_or_else(|e| panic!("{side:?}: {e:?}"))
            .expect("extended");
        assert!(is_spline(&e.surface));
        meets(&e, 50.0);
        // the end weight ratio is the same on both rows: C1, no skew, rounding only
        assert!(e.extension < 1e-12 * 50.0, "{}", e.extension);
        let pts = past(&e, side);
        let len = e.across.enclosure.mid();
        for (i, p) in pts.iter().enumerate() {
            // on the tangent line, equally spaced: the line's own parameter
            let t = len * f64::from(u32::try_from(i).unwrap()) / 8.0;
            let want = if side.is_high() {
                Vec3::new(end[0] - t, end[1], 0.5)
            } else {
                Vec3::new(end[0], end[1] - t, 0.5)
            };
            assert!(p.distance(want) < 1e-10, "{side:?} {i}: {p:?} vs {want:?}");
        }
        let u = if side.is_high() { 1.0 } else { 0.0 };
        let [(tl, _), (tr, _)] = jets(&e.surface, u);
        assert!(tl.distance(tr) < 1e-12, "{tl:?} {tr:?}");
        // the derivative itself matches across the joint (C1): S_u = (p / h) rho (P_n - P_(n-1))
        let SurfaceKind::BSpline(b) = e.surface.kind() else {
            panic!()
        };
        let c = extract_iso_crv(b.gw_nurb().unwrap(), 0.5, Direction::U).unwrap();
        let l = c.derivs(u, Side::Left, 1).unwrap()[1];
        let r = c.derivs(u, Side::Right, 1).unwrap()[1];
        assert!(l.distance(r) < 1e-12 * l.length(), "{l:?} {r:?}");
        assert!(
            (l.length() - 2.0 * 2.0 * FRAC_1_SQRT_2).abs() < 1e-12,
            "{l:?}"
        );
    }
}

#[test]
fn ruled_on_a_polynomial_row_is_the_elevated_tangent_line() {
    // y = x^2 ends at (1, 1) with tangent (1, 2): ruled by 3, the line (1, 1) + s (1, 2) / sqrt 5
    let s = parabola();
    let d = 3.0;
    let e = s
        .extended_by_distance(&k(), &dom(&s), BoxSide::UMax, Continuity::Ruled, d)
        .unwrap()
        .expect("extended");
    meets(&e, d);
    let n = 5.0_f64.sqrt();
    for p in past(&e, BoxSide::UMax) {
        // on the tangent line, within its measured length
        let off = (p.y - 1.0) - 2.0 * (p.x - 1.0);
        assert!(off.abs() < 1e-12, "{p:?}");
        assert!(p.x >= 1.0 - 1e-12 && (p.x - 1.0) * n <= d + 1e-8, "{p:?}");
    }
    let end = *past(&e, BoxSide::UMax).last().unwrap();
    assert!(((end.x - 1.0) * n - d).abs() < 1e-8, "{end:?}");
}

#[test]
fn ruled_with_end_weight_ratios_that_differ_is_c1_straight_and_reaches_any_distance() {
    // a rational row pair whose last two weights differ in ratio (1 and 2 on the two rows):
    // within one rho's reach its form, the source's net kept; past it the ruled surface S + s
    // S_u (#2878); straight at every v, C1 at the joint, the source the same surface and
    // parameters either way
    let rows = [
        vec![[0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]],
        vec![[0.5, 0.0, 0.0, 1.0], [1.0, 0.0, 2.0, 2.0]],
        vec![[1.0, 1.0, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0]],
    ];
    let s = spline(
        &rows,
        2,
        1,
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        &[0.0, 0.0, 1.0, 1.0],
        true,
    );
    for distance in [0.1, 10.0, 1e3] {
        let e = s
            .extended_by_distance(&k(), &dom(&s), BoxSide::UMax, Continuity::Ruled, distance)
            .unwrap()
            .expect("extended");
        let a = e.across;
        assert!(a.converged, "{a:?}");
        let tol = (1e-9 * distance).max(a.truncation + a.rounding);
        assert!(
            a.enclosure.lower >= distance - tol && a.enclosure.upper <= distance + tol,
            "{a:?} for {distance}"
        );
        assert!(e.source <= 1e-12, "{}", e.source);
        if distance == 0.1 {
            assert_eq!(e.source, 0.0);
        } else if distance == 1e3 {
            assert!(e.source > 0.0);
        }
        for u in [0.0, 0.25, 0.5, 0.75, 1.0] {
            for v in [0.0, 0.3, 0.5, 0.9, 1.0] {
                let uv = Vec2::new(u, v);
                let was = s.evaluate_point(&k(), uv).unwrap();
                let now = e.surface.evaluate_point(&k(), uv).unwrap();
                assert!(was.distance(now) <= e.source, "{u} {v}: {was:?} {now:?}");
            }
        }
        let SurfaceKind::BSpline(b) = e.surface.kind() else {
            panic!()
        };
        let d = dom(&e.surface);
        for v in [0.0, 0.3, 0.5, 0.9, 1.0] {
            let c = extract_iso_crv(b.gw_nurb().unwrap(), v, Direction::U).unwrap();
            // C1 at the joint
            let l = c.derivs(1.0, Side::Left, 1).unwrap()[1];
            let r = c.derivs(1.0, Side::Right, 1).unwrap()[1];
            assert!(l.distance(r) < 1e-12 * l.length(), "{v}: {l:?} {r:?}");
            // straight past it: every point on the chord from the joint to the far end (along
            // S_u, with C1 above), to the rounding of making and evaluating the net
            let bound = straightness_bound(b.gw_nurb().unwrap());
            let p0 = e.surface.evaluate_point(&k(), Vec2::new(1.0, v)).unwrap();
            let chord = e
                .surface
                .evaluate_point(&k(), Vec2::new(d.max().x, v))
                .unwrap()
                - p0;
            for i in 1..=8 {
                let u = 1.0 + (d.max().x - 1.0) * f64::from(i) / 8.0;
                let p = e.surface.evaluate_point(&k(), Vec2::new(u, v)).unwrap();
                let off = (p - p0).cross(chord).length() / chord.length();
                assert!(off <= bound, "{v} {u}: {p:?} off {off}, bound {bound}");
            }
        }
    }
}

/// The most a point of an extension's row past the joint can be off the chord through two others
/// of the row, from rounding: the exact rows are straight, the Ruled points `X1 + t X2` (or
/// `H_n + t D`) are each `lin`s of a product and a sum (`gamma_4` of the homogeneous coordinates,
/// over the least weight), and evaluating the net at a point is the kernel's de Boor,
/// `16 (p + q + 2)` operations of the 3D size as [`crate::surface::implicit`]'s form checks
/// take it. Three points each within that of a common line put the middle one within three
/// times it of the chord of the others.
fn straightness_bound(net: &Net) -> f64 {
    use crate::nurbs::math::gamma;
    let (mut h, mut w_min, mut size) = (0.0_f64, f64::INFINITY, 0.0_f64);
    for c in net.rows().flatten() {
        let w = if c.w == NOW { 1.0 } else { c.w };
        h = h.max(c.x.abs()).max(c.y.abs()).max(c.z.abs());
        w_min = w_min.min(w);
        size = size
            .max((c.x / w).abs())
            .max((c.y / w).abs())
            .max((c.z / w).abs());
    }
    let (p, q) = net.degrees();
    3.0 * (gamma(4) * h / w_min + gamma(16 * (p + q + 2)) * size)
}

/// A rational net of degree 2 both ways, of 3 spans in v (one knot double), whose weights are
/// no product (`1 + 0.4 i j`): the end ratios differ at every side.
fn ruled_product_net() -> Surface {
    let rows: Vec<Vec<[f64; 4]>> = (0..3_u32)
        .map(|i| {
            (0..6_u32)
                .map(|j| {
                    let (x, y) = (f64::from(i), f64::from(j));
                    let w = 1.0 + 0.4 * x * y;
                    let z = if (i + j) % 2 == 0 { 0.5 } else { -0.25 };
                    [x * w, y * w, z * w, w]
                })
                .collect()
        })
        .collect();
    spline(
        &rows,
        2,
        2,
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        &[0.0, 0.0, 0.0, 0.25, 0.25, 0.6, 1.0, 1.0, 1.0],
        true,
    )
}

/// Ruled over rows whose end ratios differ, past every side and over interior knots across
/// (one of them already double), past the rows' reach: the source is the same surface and
/// parameters to the bound reported, and past the joint every row is straight along the side's
/// tangent, C1.
#[test]
fn the_ruled_product_form_past_every_side_keeps_the_source_and_every_row_is_straight() {
    let s = ruled_product_net();
    let (nu, nv) = (5, 5);
    for side in BoxSide::ALL {
        let e = s
            .extended_by_distance(&k(), &dom(&s), side, Continuity::Ruled, 400.0)
            .unwrap()
            .expect("extended");
        assert!(
            e.source > 0.0 && e.source <= 1e-11,
            "{side:?}: {}",
            e.source
        );
        for i in 0..=nu {
            for j in 0..=nv {
                let uv = Vec2::new(f64::from(i) / f64::from(nu), f64::from(j) / f64::from(nv));
                let was = s.evaluate_point(&k(), uv).unwrap();
                let now = e.surface.evaluate_point(&k(), uv).unwrap();
                assert!(
                    was.distance(now) <= e.source,
                    "{side:?} {uv:?}: {}",
                    was.distance(now)
                );
            }
        }
        let SurfaceKind::BSpline(b) = e.surface.kind() else {
            panic!()
        };
        let net = b.gw_nurb().unwrap();
        assert_eq!(if side.is_u() { net.q() } else { net.p() }, 4);
        let d = dom(&e.surface);
        let (joint, far) = if side.is_high() {
            (1.0, if side.is_u() { d.max().x } else { d.max().y })
        } else {
            (0.0, if side.is_u() { d.min().x } else { d.min().y })
        };
        let uv = |along: f64, t: f64| {
            if side.is_u() {
                Vec2::new(along, t)
            } else {
                Vec2::new(t, along)
            }
        };
        for t in [0.0, 0.25, 0.4, 0.6, 1.0] {
            let dir = if side.is_u() {
                Direction::U
            } else {
                Direction::V
            };
            let c = extract_iso_crv(net, t, dir).unwrap();
            let l = c.derivs(joint, Side::Left, 1).unwrap()[1];
            let r = c.derivs(joint, Side::Right, 1).unwrap()[1];
            assert!(
                l.distance(r) < 1e-9 * l.length(),
                "{side:?} {t}: {l:?} {r:?}"
            );
            // straight past it, as for the row pair: on the chord to the far end, to the
            // rounding of making and evaluating the net
            let bound = straightness_bound(net);
            let p0 = e.surface.evaluate_point(&k(), uv(joint, t)).unwrap();
            let chord = e.surface.evaluate_point(&k(), uv(far, t)).unwrap() - p0;
            for i in 1..=8 {
                let a = joint + (far - joint) * f64::from(i) / 8.0;
                let p = e.surface.evaluate_point(&k(), uv(a, t)).unwrap();
                let off = (p - p0).cross(chord).length() / chord.length();
                assert!(off <= bound, "{side:?} {t} {a}: off {off}, bound {bound}");
            }
        }
    }
}

/// The form on the form: a second side doubles the degree again, the sum of the two bounds
/// still holding the source.
#[test]
fn the_ruled_product_form_extends_past_a_second_side() {
    let s = ruled_product_net();
    let sides = s
        .extended_sides(
            &k(),
            &[BoxSide::UMin, BoxSide::UMax],
            Continuity::Ruled,
            400.0,
        )
        .unwrap();
    assert!(
        // the second form's rounding is of the first's reach: held to the stop's rounding part
        sides.source > 0.0 && sides.source <= 1e-6 * 400.0,
        "{}",
        sides.source
    );
    let SurfaceKind::BSpline(b) = sides.surface.kind() else {
        panic!()
    };
    assert_eq!(b.gw_nurb().unwrap().q(), 8);
    for (u, v) in [(0.0, 0.0), (0.5, 0.3), (0.25, 0.6), (1.0, 1.0)] {
        let uv = Vec2::new(u, v);
        let was = s.evaluate_point(&k(), uv).unwrap();
        let now = sides.surface.evaluate_point(&k(), uv).unwrap();
        assert!(was.distance(now) <= sides.source, "{uv:?}");
    }
}

/// A degree across whose double has no B-spline form here keeps the rows' refusal, naming a
/// distance that works.
#[test]
fn the_ruled_product_form_past_the_largest_degree_keeps_the_rows_refusal() {
    let q = MAX_DEGREE / 2 + 1;
    let rows: Vec<Vec<[f64; 4]>> = (0..2_u32)
        .map(|i| {
            (0..=q)
                .map(|j| {
                    #[allow(clippy::cast_precision_loss)]
                    let y = j as f64;
                    let x = f64::from(i);
                    let w = 1.0 + 0.1 * x * y;
                    [x * w, y * w, 0.0, w]
                })
                .collect()
        })
        .collect();
    let v: Vec<f64> = std::iter::repeat_n(0.0, q + 1)
        .chain(std::iter::repeat_n(1.0, q + 1))
        .collect();
    let s = spline(&rows, 1, q, &[0.0, 0.0, 1.0, 1.0], &v, true);
    let d = 1e3;
    let err = s
        .extended_by_distance(&k(), &dom(&s), BoxSide::UMax, Continuity::Ruled, d)
        .unwrap_err();
    let named = not_exact_naming(err).unwrap();
    assert!(named > 0.0 && named < d, "{named}");
    let e = s
        .extended_by_distance(&k(), &dom(&s), BoxSide::UMax, Continuity::Ruled, named)
        .unwrap()
        .expect("extended");
    assert_eq!(e.source, 0.0);
    meets(&e, named);
}

/// A ball of radius 2 between the planes y = 0 (side -1) and z = 0 (side +1) along the
/// leader (0, 2, 2) -> (10, 2, 2), of order `order`, on a toy net (a unit square, bound
/// 1e-3); with `certified`, on its own net and bound ([`net_of_definition`]). The planes
/// run to x = 20, so the feet have room along.
fn round_blend(order: u32, certified: bool) -> (Kernel, ProceduralBlend) {
    round_blend_with(order, certified, 0.5)
}

/// [`round_blend`] with the G^n section's thumbweight rule `thumbweight`.
fn round_blend_with(order: u32, certified: bool, thumbweight: f64) -> (Kernel, ProceduralBlend) {
    use crate::blend::fillet_section::recertify::net_of_definition;
    use crate::blend::fillet_section::section::GnSectionFsg;
    use crate::geom::vector3d::Point3;
    use crate::nurbs::b_spline_curve::BSplineCurve;
    use crate::surface::procedural_blend::BlendDefinitionInput;
    let mut kk = k();
    kk.fixes.fillet_round_as_definition = true;
    let (l, r) = (10.0, 2.0);
    let bilinear =
        |p: [Point3; 4]| BSplineSurface::create_bilinear_surface(p[0], p[1], p[2], p[3]).unwrap();
    let o = Point3::new(0.0, 0.0, 0.0);
    let a = Surface::from(bilinear([
        o,
        Point3::new(2.0 * l, 0.0, 0.0),
        Point3::new(0.0, 0.0, l),
        Point3::new(2.0 * l, 0.0, l),
    ]));
    let b = Surface::from(bilinear([
        o,
        Point3::new(2.0 * l, 0.0, 0.0),
        Point3::new(0.0, l, 0.0),
        Point3::new(2.0 * l, l, 0.0),
    ]));
    let toy = bilinear([
        o,
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
        Point3::new(1.0, 1.0, 0.0),
    ]);
    let line = |a: Point3, b: Point3| BSplineCurve::create_line_segment(3, a, b).unwrap();
    let leader = line(Point3::new(0.0, r, r), Point3::new(l, r, r));
    let seed = line(Point3::new(0.0, r / l, 0.0), Point3::new(0.5, r / l, 0.0));
    let input = BlendDefinitionInput {
        supports: [&a, &b],
        sides: [-1.0, 1.0],
        offsets: [r, r],
        law: None,
        order,
        thumbweight: if order >= 2 { thumbweight } else { 0.0 },
        leader: &leader,
        seeds: [&seed, &seed],
        bound: 1e-3,
        chamfer: None,
    };
    let valid = |n: u32, w: f64| GnSectionFsg::is_valid(n as usize, w);
    let pb = ProceduralBlend::create(&kk, &toy, &input, valid).unwrap();
    if !certified {
        return (kk, pb);
    }
    let (net, bound) = net_of_definition(&kk, &pb, 1e-12).unwrap();
    let input = BlendDefinitionInput { bound, ..input };
    let pb = ProceduralBlend::create(&kk, &net, &input, valid).unwrap();
    (kk, pb)
}

#[test]
fn a_round_continues_on_its_ball_past_a_foot_with_its_bound_grown_in_closed_form() {
    let (kk, pb) = round_blend(1, true);
    let s = Surface::from(pb.clone());
    let d0 = s.natural_uv_domain_with_nurb(&kk).unwrap();
    // past the foot on z = 0 (t = 1, the net's v) by an arc of 1 on the ball of radius 2
    let e = s
        .extended_by_distance(&kk, &d0, BoxSide::VMax, Continuity::Ruled, 1.0)
        .unwrap()
        .expect("extended");
    meets(&e, 1.0);
    let SurfaceKind::ProceduralBlend(out) = e.surface.kind() else {
        panic!("not a blend: {:?}", e.surface.kind());
    };
    let d = e.surface.natural_uv_domain_with_nurb(&kk).unwrap();
    let te = d.max().y;
    assert!(te > 1.0, "{d:?}");
    // the growth, in closed form: T_4(2 te - 1) times the net's weight ratio (W is at least
    // 1 past the foot, at most 1 on the source)
    let x = 2.0 * te - 1.0;
    let t4 = 8.0 * x.powi(4) - 8.0 * x * x + 1.0;
    let ws = |n: &crate::nurbs::srf_basic::Surface| {
        let w: Vec<f64> = n.cpts().iter().flatten().map(|c| c.w).collect();
        (
            w.iter().copied().fold(0.0_f64, f64::max),
            w.iter().copied().fold(f64::INFINITY, f64::min),
        )
    };
    let (vhi, _) = ws(pb.net.gw_nurb().unwrap());
    let (_, vlo) = ws(out.net.gw_nurb().unwrap());
    let want = pb.bound() * t4 * vhi / vlo;
    assert!(
        out.bound() >= want && out.bound() <= want * (1.0 + 1e-12) + 1e-15 + e.extension,
        "{} vs {want}",
        out.bound()
    );
    // the continued part: on the ball's circle about the leader, within the bound of the
    // definition continued, and the end an arc of 1 past the foot
    for i in 0..=8 {
        for j in 0..=8 {
            let u = f64::from(i) / 8.0;
            let v = 1.0 + (te - 1.0) * f64::from(j) / 8.0;
            let p = e.surface.evaluate_point(&kk, Vec2::new(u, v)).unwrap();
            let st = out.definition_parameters(Point2::new(u, v));
            let def = out.evaluate_definition(&kk, st.x, st.y, 0, 0).unwrap()[0][0];
            assert!(p.distance(def) <= out.bound(), "{u} {v}: {p:?} {def:?}");
            let rad = (p.y - 2.0).hypot(p.z - 2.0);
            assert!((rad - 2.0).abs() <= out.bound() + 1e-12, "{p:?}");
        }
    }
    let end = e.surface.evaluate_point(&kk, Vec2::new(0.5, te)).unwrap();
    let phi = (end.z - 2.0).atan2(end.y - 2.0);
    assert!((phi - (-FRAC_PI_2 + 0.5)).abs() < 1e-8, "{end:?} {phi}");
    // the source part is the source's net, bit for bit
    let src = pb.net.gw_nurb().unwrap();
    let got = out.net.gw_nurb().unwrap();
    assert!(keeps(src, got, BoxSide::VMax).unwrap());
}

#[test]
fn a_round_continues_along_its_spine_by_a_span_of_its_definition() {
    let (kk, pb) = round_blend(1, true);
    let s = Surface::from(pb.clone());
    let d0 = s.natural_uv_domain_with_nurb(&kk).unwrap();
    // past x = 10 by 3 along the straight spine: the ball's cylinder about the leader
    let e = s
        .extended_by_distance(&kk, &d0, BoxSide::UMax, Continuity::Tangent, 3.0)
        .unwrap()
        .expect("extended");
    meets(&e, 3.0);
    let SurfaceKind::ProceduralBlend(out) = e.surface.kind() else {
        panic!("not a blend: {:?}", e.surface.kind());
    };
    let d = e.surface.natural_uv_domain_with_nurb(&kk).unwrap();
    let ue = d.max().x;
    assert!((ue - 1.3).abs() < 1e-6, "{d:?}");
    assert!(
        out.bound() >= pb.bound() && out.bound() < 1e-6,
        "{}",
        out.bound()
    );
    for i in 0..=8 {
        for j in 0..=8 {
            let u = 1.0 + (ue - 1.0) * f64::from(i) / 8.0;
            let v = f64::from(j) / 8.0;
            let p = e.surface.evaluate_point(&kk, Vec2::new(u, v)).unwrap();
            let st = out.definition_parameters(Point2::new(u, v));
            let def = out.evaluate_definition(&kk, st.x, st.y, 0, 0).unwrap()[0][0];
            assert!(p.distance(def) <= out.bound(), "{u} {v}: {p:?} {def:?}");
            let rad = (p.y - 2.0).hypot(p.z - 2.0);
            assert!((rad - 2.0).abs() <= out.bound() + 1e-12, "{p:?}");
            assert!((p.x - 10.0 * u).abs() <= out.bound() + 1e-9, "{p:?}");
        }
    }
    // its seeds are on the new station, and the net re-certifies against its definition
    let sigma = out.definition().seeds[0].knots(None).unwrap().0;
    assert_eq!(sigma.len(), 3, "{sigma:?}");
    let again = crate::blend::fillet_section::recertify::recertify(&kk, out).unwrap();
    assert!(again <= out.bound(), "{again} vs {}", out.bound());
}

#[test]
fn a_side_inside_the_spine_runs_the_definitions_spans_then_past_its_end() {
    use crate::nurbs::b_spline_surface::SurfParamType;
    let (kk, pb) = round_blend(1, true);
    // the half of the round up to x = 5 (u = 1/2, inside the spine's one span)
    let (left, _) = pb.split_at(&kk, 0.5, SurfParamType::U).unwrap();
    let s = Surface::from(left.clone());
    let d0 = s.natural_uv_domain_with_nurb(&kk).unwrap();
    assert!((d0.max().x - 0.5).abs() < 1e-15, "{d0:?}");
    for (d, ue, stations) in [(3.0, 0.8, 2), (8.0, 1.3, 3)] {
        let e = s
            .extended_by_distance(&kk, &d0, BoxSide::UMax, Continuity::Tangent, d)
            .unwrap()
            .expect("extended");
        meets(&e, d);
        let SurfaceKind::ProceduralBlend(out) = e.surface.kind() else {
            panic!("not a blend: {:?}", e.surface.kind());
        };
        let dd = e.surface.natural_uv_domain_with_nurb(&kk).unwrap();
        assert!((dd.max().x - ue).abs() < 1e-6, "{d}: {dd:?}");
        assert!(
            out.bound() >= left.bound() && out.bound() < 1e-6,
            "{}",
            out.bound()
        );
        for i in 0..=8 {
            for j in 0..=8 {
                let u = 0.5 + (dd.max().x - 0.5) * f64::from(i) / 8.0;
                let v = f64::from(j) / 8.0;
                let p = e.surface.evaluate_point(&kk, Vec2::new(u, v)).unwrap();
                let st = out.definition_parameters(Point2::new(u, v));
                let def = out.evaluate_definition(&kk, st.x, st.y, 0, 0).unwrap()[0][0];
                assert!(p.distance(def) <= out.bound(), "{u} {v}: {p:?} {def:?}");
                let rad = (p.y - 2.0).hypot(p.z - 2.0);
                assert!((rad - 2.0).abs() <= out.bound() + 1e-12, "{p:?}");
            }
        }
        // inside the spine the definition is its own; past its end it gains a station
        let sigma = out.definition().seeds[0].knots(None).unwrap().0;
        assert_eq!(sigma.len(), stations, "{d}: {sigma:?}");
        let src = left.net.gw_nurb().unwrap();
        assert!(keeps(src, out.net.gw_nurb().unwrap(), BoxSide::UMax).unwrap());
    }
}

#[test]
fn a_gn_section_continues_past_a_foot_with_its_bound_grown_over_its_weight_in_closed_form() {
    // the G^2 section at the constant thumbweight 1/2
    let (kk, pb) = round_blend(2, true);
    let s = Surface::from(pb.clone());
    let d0 = s.natural_uv_domain_with_nurb(&kk).unwrap();
    let e = s
        .extended_by_distance(&kk, &d0, BoxSide::VMax, Continuity::Tangent, 0.3)
        .unwrap()
        .expect("extended");
    meets(&e, 0.3);
    let SurfaceKind::ProceduralBlend(out) = e.surface.kind() else {
        panic!("not a blend: {:?}", e.surface.kind());
    };
    let d = e.surface.natural_uv_domain_with_nurb(&kk).unwrap();
    let te = d.max().y;
    assert!(te > 1.0 && te < 2.0, "{d:?}");
    // the growth in closed form: T_m(2 te - 1), m = 3 n + 1 + the net's degree across,
    // times the net's weight ratio, over W's least past the foot: A >= 1, B >= 1 and
    // C = 1 - te / 2 at the end (W at most 1 on the source)
    let src = pb.net.gw_nurb().unwrap();
    let m = 7 + src.q();
    let x = 2.0 * te - 1.0;
    let (mut prev, mut cur) = (1.0_f64, x);
    for _ in 1..m {
        let next = 2.0 * x * cur - prev;
        prev = cur;
        cur = next;
    }
    let ws = |n: &crate::nurbs::srf_basic::Surface| {
        let w: Vec<f64> = n
            .cpts()
            .iter()
            .flatten()
            .map(|c| if c.w == NOW { 1.0 } else { c.w })
            .collect();
        (
            w.iter().copied().fold(0.0_f64, f64::max),
            w.iter().copied().fold(f64::INFINITY, f64::min),
        )
    };
    let (vhi, _) = ws(src);
    let (_, vlo) = ws(out.net.gw_nurb().unwrap());
    let c = 1.0 - 0.5 * te;
    let want = pb.bound() * cur * vhi / vlo / (c * c);
    assert!(
        out.bound() >= want && out.bound() <= want * (1.0 + 1e-9) + 1e-15 + e.extension,
        "{} vs {want}",
        out.bound()
    );
    // the continued part within the bound of the definition continued
    for i in 0..=8 {
        for j in 0..=8 {
            let u = f64::from(i) / 8.0;
            let v = 1.0 + (te - 1.0) * f64::from(j) / 8.0;
            let p = e.surface.evaluate_point(&kk, Vec2::new(u, v)).unwrap();
            let st = out.definition_parameters(Point2::new(u, v));
            let def = out.evaluate_definition(&kk, st.x, st.y, 0, 0).unwrap()[0][0];
            assert!(p.distance(def) <= out.bound(), "{u} {v}: {p:?} {def:?}");
        }
    }
    // the source part is the source's net, bit for bit
    assert!(keeps(src, out.net.gw_nurb().unwrap(), BoxSide::VMax).unwrap());
    // a long ask stays short of the section's weight's zero (C = 0 at t = 2): met there
    // with the bound grown by its closed form, or refused
    match s.extended_by_distance(&kk, &d0, BoxSide::VMax, Continuity::Tangent, 50.0) {
        Ok(Some(long)) => {
            let d = long.surface.natural_uv_domain_with_nurb(&kk).unwrap();
            assert!(d.max().y < 2.0, "{d:?}");
        }
        Err(SolidError::ExtendRefused { .. }) => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_equal_area_gn_section_continues_over_its_thumbweights_proved_enclosure() {
    use crate::blend::fillet_section::recertify::recertify_with_thumbweight;
    use crate::blend::fillet_section::section::GN_THUMBWEIGHT_EQUAL_AREA;
    let (kk, pb) = round_blend_with(2, true, GN_THUMBWEIGHT_EQUAL_AREA);
    // the certificate's interval Newton roots enclose the thumbweight: one w along this
    // straight round, below 1
    let (_, w) = recertify_with_thumbweight(&kk, &pb).unwrap();
    let [lo, hi] = w.unwrap();
    assert!(lo <= hi && hi < 1.0 && hi - lo < 1e-6, "{lo} {hi}");
    let s = Surface::from(pb.clone());
    let d0 = s.natural_uv_domain_with_nurb(&kk).unwrap();
    let e = s
        .extended_by_distance(&kk, &d0, BoxSide::VMax, Continuity::Tangent, 0.3)
        .unwrap()
        .expect("extended");
    meets(&e, 0.3);
    let SurfaceKind::ProceduralBlend(out) = e.surface.kind() else {
        panic!("not a blend");
    };
    assert!(out.bound().is_finite() && out.bound() >= pb.bound());
    // a constant thumbweight is its own
    let (kk, pb) = round_blend(2, true);
    assert!(recertify_with_thumbweight(&kk, &pb).unwrap().1.is_none());
}

#[test]
fn a_round_along_past_its_supports_is_refused_naming_a_distance_that_works() {
    // the planes run to x = 20 and the spine ends at x = 10: past x = 20 a foot leaves its
    // support, so 15 along is refused, naming a distance under 10 that is met
    let (kk, pb) = round_blend(1, true);
    let s = Surface::from(pb);
    let d0 = s.natural_uv_domain_with_nurb(&kk).unwrap();
    let named = match s.extended_by_distance(&kk, &d0, BoxSide::UMax, Continuity::Tangent, 15.0) {
        Err(SolidError::ExtendRefused {
            distance: Some(x), ..
        }) => x,
        other => panic!("{other:?}"),
    };
    assert!(named > 0.0 && named <= 10.0 + 1e-6, "{named}");
    let e = s
        .extended_by_distance(&kk, &d0, BoxSide::UMax, Continuity::Tangent, named)
        .unwrap()
        .expect("extended");
    meets(&e, named);
}

/// The octant ball patch of radius 2 about `(1, -2, 0.5)`, its pole on +x.
fn octant_ball() -> (Surface, Vec3, f64) {
    let (c, r) = (Vec3::new(1.0, -2.0, 0.5), 2.0);
    let dirs = [Vec3::X, Vec3::Y, Vec3::Z];
    let (net, _, _) =
        crate::blend::blend_corner::ball::ball_triangle_patch(c, r, &dirs, 0, false).unwrap();
    let patch = crate::surface::ball_patch::BallPatch::create(&net, c, r).unwrap();
    (Surface::from(patch), c, r)
}

#[test]
fn a_ball_patch_extends_its_own_net_on_its_sphere_up_to_its_caps() {
    use crate::surface::implicit::{lies_on, Implicit};
    let kk = k();
    let (s, c, r) = octant_ball();
    let SurfaceKind::BallPatch(src) = s.kind() else {
        panic!("not a ball patch");
    };
    let src_net = src.net().gw_nurb().unwrap().clone();
    // a short ask is met on every side but the pole's; a long one stops at the caps: round
    // the pole r times a quarter of the 270 degrees left open, away half the 90 degrees
    // left from the third side to the antipode
    for (d, round, away) in [(0.1, 0.1, 0.1), (100.0, r * 0.375 * PI, r * 0.25 * PI)] {
        let e = s
            .extended_sides(&kk, &BoxSide::ALL, Continuity::Ruled, d)
            .unwrap();
        let SurfaceKind::BallPatch(out) = e.surface.kind() else {
            panic!("{d}: not a ball patch");
        };
        assert_eq!(e.sides[2], SideExtension::Collapsed, "{:?}", e.sides);
        for (i, want) in [(0, round), (1, round), (3, away)] {
            let a = match e.sides[i] {
                SideExtension::Extended(a) if d < 1.0 => a,
                SideExtension::UpToLimit(a) if d > 1.0 => a,
                other => panic!("{d}: side {i}: {other:?}"),
            };
            assert!(
                (a.enclosure.mid() - want).abs() <= 1e-9 * want,
                "{d}: {i}: {a:?}"
            );
        }
        // the source's net bit for bit, the whole result on the sphere
        let out_net = out.net().gw_nurb().unwrap();
        let kept = src_net.cpts().iter().flatten().all(|p| {
            out_net
                .cpts()
                .iter()
                .flatten()
                .any(|q| p.x == q.x && p.y == q.y && p.z == q.z && p.w == q.w)
        });
        assert!(kept, "{d}: the source's net moved");
        assert!(lies_on(
            out.net(),
            &Implicit::Sphere {
                centre: c,
                radius: r
            }
        )
        .unwrap());
        assert!((out.radius() - r).abs() == 0.0 && out.centre() == c);
    }
}

/// A small plane far from the origin extended far, keeping its parameters: the end cap of a
/// bar at (147..152, -25.5, 846..852), 5.83 mm square, turned 2 degrees about x by a draft,
/// extended 547.405 past every side by the draft's local operation (its placement and net as
/// SolidCraft's draft hands them over). Its net lies on the plane to its rounding, about 1e-13
/// at this size, and the continuation carries that out about 190 times each way: the
/// extended net is 6.4e-11 off the plane, where the rounding of the check alone is 8e-12. It
/// is the plane's own net continued, on the plane within what the source's residual grows
/// to, and is extended, not refused as not exact.
#[test]
fn a_small_tilted_plane_far_from_the_origin_extends_far_on_its_plane() {
    use crate::surface::implicit::{lies_on, AnalyticSupport, Implicit};
    let kk = k();
    let origin = Vec3::new(152.33308696865228, -25.520154849846044, 852.1498844507267);
    let x_axis = Vec3::new(0.0, 0.03489949670250011, -0.9993908270190959);
    let y_axis = Vec3::new(-1.0, 1.9539925233402755e-14, 0.0);
    let (lu, lv) = (5.830273937304582, 5.830273937304639);
    let rows = [
        vec![
            [
                152.33308696865228,
                -25.520154849846044,
                852.1498844507267,
                1.0,
            ],
            [
                146.50281303134764,
                -25.52015484984593,
                852.1498844507267,
                1.0,
            ],
        ],
        vec![
            [
                152.33308696865228,
                -25.316681223796408,
                846.323162158776,
                1.0,
            ],
            [
                146.50281303134764,
                -25.316681223796287,
                846.323162158776,
                1.0,
            ],
        ],
    ];
    let SurfaceKind::BSpline(form) =
        spline(&rows, 1, 1, &[0.0, 0.0, lu, lu], &[0.0, 0.0, lv, lv], false)
            .kind()
            .clone()
    else {
        unreachable!()
    };
    let p = Surface::from(Plane::from_stored(
        Axis2Placement::from_stored(origin, x_axis, y_axis),
        Vec2::new(1.0000000000000169, 1.0),
        Extent2d::from_bounds(0.0, 0.0, lu, lv),
        form.clone(),
    ));
    let on = AnalyticSupport::check(&kk, &p).unwrap().implicit();
    let Implicit::Plane { origin, normal } = on else {
        panic!("not a plane: {on:?}");
    };
    let d = 547.4052982921842;
    let e = p
        .extended_sides_keeping_parameters(&kk, &BoxSide::ALL, Continuity::Ruled, d)
        .unwrap();
    assert!(
        e.sides
            .iter()
            .all(|x| matches!(x, SideExtension::Extended(_))),
        "{:?}",
        e.sides
    );
    let SurfaceKind::BSpline(grown) = e.surface.kind() else {
        panic!("not a B-spline: {:?}", e.surface.kind());
    };
    // every control point on the plane to the rounding of building it from the plane's own
    // origin and axes (the continued net was 6.4e-11 off it), and the plane's own point at
    // its parameters: the extension is the plane over the larger box
    let SurfaceKind::Plane(pl) = p.kind() else {
        unreachable!()
    };
    let g = grown.gw_nurb().unwrap();
    let (u, v) = (g.u_knots(), g.v_knots());
    assert_eq!((u.len(), v.len()), (4, 4), "one bilinear span");
    for (i, row) in g.cpts().iter().enumerate() {
        for (j, c) in row.iter().enumerate() {
            let x = Vec3::new(c.x, c.y, c.z);
            let off = normal.dot(x - origin);
            assert!(off.abs() <= 1e-12, "{c:?}: {off}");
            let at = Point2::new(u[if i == 0 { 0 } else { 3 }], v[if j == 0 { 0 } else { 3 }]);
            let own = pl.evaluate_point_fast(at);
            assert!((x - own).length() <= 1e-9, "{c:?} vs {own:?}");
        }
    }
    assert!(lies_on(grown, &on).unwrap());
    // the check refuses a net 1e-9 off the plane at a far corner
    let mut net = g.cpts();
    let c = &mut net[0][0];
    let moved = Vec3::new(c.x, c.y, c.z) + normal * 1e-9;
    *c = CPoint::new(moved.x, moved.y, moved.z, NOW);
    let off = Net::new(net, 1, 1, u.to_vec(), v.to_vec()).unwrap();
    assert!(!lies_on(&BSplineSurface::from_gw_nurb(&off), &on).unwrap());
    for a in e.sides {
        let SideExtension::Extended(a) = a else {
            unreachable!()
        };
        assert!((a.enclosure.mid() - d).abs() <= 1e-9 * d, "{a:?}");
    }
}

/// Every control point of `s`'s net on the analytic `on`, certified (`lies_on`), and the
/// net's points on a grid over its whole box within `tol` of it by the closed form `off`.
fn on_its_analytic(
    s: &BSplineSurface,
    on: &crate::surface::implicit::Implicit,
    off: impl Fn(Vec3) -> f64,
    tol: f64,
) {
    use crate::surface::implicit::lies_on;
    assert!(lies_on(s, on).unwrap());
    let d = ball::knot_box(s).unwrap();
    for i in 0..=8 {
        for j in 0..=8 {
            let uv = d.evaluate(f64::from(i) / 8.0, f64::from(j) / 8.0);
            let x = s.evaluate_point(uv).unwrap();
            assert!(off(x).abs() <= tol, "{uv:?}: {}", off(x));
        }
    }
}

/// A narrow strip of a cylinder far from the origin (5 degrees of radius 3, 2 high, at
/// (800, -300, 900)) extended far along its rulings and round its axis up to its cap: its
/// short spans carried 20 times their length past each end. Each new point is built on the
/// cylinder from its axis and radius, so the result is on it to the rounding of one
/// evaluation, and is extended, not refused as not exact.
#[test]
fn a_narrow_cylinder_far_from_the_origin_extends_far_on_its_cylinder() {
    use crate::surface::implicit::{AnalyticSupport, Implicit};
    let kk = k();
    let o = Vec3::new(800.0, -300.0, 900.0);
    let pl = Axis2Placement::new(o, Vec3::Z, Vec3::X);
    let mut c = Cone::create_canonical(&kk, &pl, 3.0, 3.0, 2.0, false, false).unwrap();
    c.adjust_step_uv_domain(&kk, &Extent2d::from_bounds(0.0, 0.0, 5.0, 2.0))
        .unwrap();
    let c = Surface::from(c);
    let on = AnalyticSupport::check(&kk, &c).unwrap().implicit();
    let Implicit::Cone { origin, axis, .. } = on else {
        panic!("not a cone: {on:?}");
    };
    let e = c
        .extended_sides(&kk, &BoxSide::ALL, Continuity::Natural, 40.0)
        .unwrap();
    let SurfaceKind::BSpline(grown) = e.surface.kind() else {
        panic!("not a B-spline: {:?}", e.surface.kind());
    };
    let radial = |x: Vec3| {
        let d = x - origin;
        (d - axis * axis.dot(d)).length() - 3.0
    };
    on_its_analytic(grown, &on, radial, 1e-11);
    // round the axis a quarter of the 355 degrees left open, along the whole 40
    for (i, s) in e.sides.iter().enumerate() {
        match (i < 2, s) {
            (true, SideExtension::UpToLimit(a)) => {
                let want = 3.0 * 0.25 * 355f64.to_radians();
                assert!((a.enclosure.mid() - want).abs() <= 1e-9 * want, "{a:?}");
            }
            (false, SideExtension::Extended(a)) => {
                assert!((a.enclosure.mid() - 40.0).abs() <= 1e-9 * 40.0, "{a:?}");
            }
            _ => panic!("{:?}", e.sides),
        }
    }
}

/// A small patch of a sphere far from the origin (4 by 4 degrees of radius 4, from 20 to 24
/// degrees of latitude) extended 2 past every side: round its axis and along its meridians,
/// each new row's arc built on the sphere from its centre and radius.
#[test]
fn a_small_sphere_patch_far_from_the_origin_extends_on_its_sphere() {
    use crate::surface::implicit::{AnalyticSupport, Implicit};
    use crate::surface::sphere::Sphere;
    let kk = k();
    let pl = Axis2Placement::new(Vec3::new(800.0, -300.0, 900.0), Vec3::Z, Vec3::X);
    let mut sp = Sphere::create_canonical(&kk, &pl, 4.0).unwrap();
    sp.adjust_step_uv_domain(&kk, &Extent2d::from_bounds(0.0, 20.0, 4.0, 24.0))
        .unwrap();
    let s = Surface::from(sp);
    let on = AnalyticSupport::check(&kk, &s).unwrap().implicit();
    let Implicit::Sphere { centre, radius } = on else {
        panic!("not a sphere: {on:?}");
    };
    let e = s
        .extended_sides(&kk, &BoxSide::ALL, Continuity::Natural, 2.0)
        .unwrap();
    let SurfaceKind::BSpline(grown) = e.surface.kind() else {
        panic!("not a B-spline: {:?}", e.surface.kind());
    };
    on_its_analytic(grown, &on, |x| (x - centre).length() - radius, 1e-11);
    for s in e.sides {
        let SideExtension::Extended(a) = s else {
            panic!("{:?}", e.sides);
        };
        assert!((a.enclosure.mid() - 2.0).abs() <= 1e-9 * 2.0, "{a:?}");
    }
}

/// A quarter of a sphere up to its pole (half a turn round the axis, 0 to 90 degrees of
/// latitude) extended round its axis: the pole's row is on the axis and has no angle of its
/// own, and turns with the others, its middle weight from the same half angle.
#[test]
fn a_sphere_patch_up_to_its_pole_extends_round_its_axis_on_its_sphere() {
    use crate::surface::implicit::{AnalyticSupport, Implicit};
    use crate::surface::sphere::{Sphere, SphereOptions};
    let kk = k();
    let mut sp = Sphere::new(
        &kk,
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::X,
        Vec3::Y,
        Extent2d::from_bounds(0.0, 0.0, 180.0, 90.0),
        10.0,
        SphereOptions::default(),
    )
    .unwrap();
    sp.make_nurb(&kk).unwrap();
    let s = Surface::from(sp);
    let on = AnalyticSupport::check(&kk, &s).unwrap().implicit();
    let Implicit::Sphere { centre, radius } = on else {
        panic!("not a sphere: {on:?}");
    };
    let e = s
        .extended_sides(
            &kk,
            &[BoxSide::UMin, BoxSide::UMax],
            Continuity::Natural,
            1.0,
        )
        .unwrap();
    let SurfaceKind::BSpline(grown) = e.surface.kind() else {
        panic!("not a B-spline: {:?}", e.surface.kind());
    };
    on_its_analytic(grown, &on, |x| (x - centre).length() - radius, 1e-11);
}

/// A small patch of a torus far from the origin (4 degrees round its axis by 6 of its tube,
/// radii 6 and 2) extended 1.5 past every side: round its axis and along its tube.
#[test]
fn a_small_torus_patch_far_from_the_origin_extends_on_its_torus() {
    use crate::surface::implicit::{AnalyticSupport, Implicit};
    use crate::surface::torus::{Torus, TorusOptions};
    let kk = k();
    let pl = Axis2Placement::new(Vec3::new(800.0, -300.0, 900.0), Vec3::Z, Vec3::X);
    let t = Torus::create_canonical(
        &kk,
        &pl,
        6.0,
        2.0,
        TorusOptions::default(),
        Some(&Extent2d::from_bounds(0.0, 30.0, 4.0, 36.0)),
    )
    .unwrap();
    let s = Surface::from(t);
    let on = AnalyticSupport::check(&kk, &s).unwrap().implicit();
    let Implicit::Torus {
        centre,
        axis,
        major,
        minor,
    } = on
    else {
        panic!("not a torus: {on:?}");
    };
    let e = s
        .extended_sides(&kk, &BoxSide::ALL, Continuity::Natural, 1.5)
        .unwrap();
    let SurfaceKind::BSpline(grown) = e.surface.kind() else {
        panic!("not a B-spline: {:?}", e.surface.kind());
    };
    let tube = |x: Vec3| {
        let d = x - centre;
        let h = axis.dot(d);
        let r = (d - axis * h).length() - major;
        r.hypot(h) - minor
    };
    on_its_analytic(grown, &on, tube, 1e-11);
    for s in e.sides {
        let SideExtension::Extended(a) = s else {
            panic!("{:?}", e.sides);
        };
        assert!((a.enclosure.mid() - 1.5).abs() <= 1e-9 * 1.5, "{a:?}");
    }
}

/// A cylinder whose label's axis is tilted off its form's rulings past the proof (recognition
/// can leave one on a body): the label is dropped and the form is extended, not refused.
#[test]
fn a_label_its_form_does_not_prove_extends_as_the_form() {
    use crate::geom::axis2_placement::Axis2Placement;
    use crate::surface::cone::p1::Cone;
    use crate::surface::implicit::AnalyticSupport;

    let k = Kernel::default();
    let mut s = Surface::from(Cone::new_cylinder_bounded(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        40.0,
        0.0,
        9.0,
        94.0,
        false,
        false,
        false,
    ));
    let SurfaceKind::Cone(c) = s.kind_mut() else {
        panic!("a cone");
    };
    c.make_nurb(&k).unwrap();
    c.rev.position = Axis2Placement::from_stored(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 1e-9),
        Vec3::new(0.0, 1.0, 0.0),
    );
    assert!(!AnalyticSupport::label_stands(&k, &s).unwrap());
    assert!(AnalyticSupport::check(&k, &s).is_err());
    let d = s.natural_uv_domain_with_nurb(&k).unwrap();
    let grown = s
        .extended_sides_keeping_parameters(&k, &BoxSide::ALL, Continuity::Ruled, 5.0)
        .unwrap()
        .surface;
    assert!(!matches!(grown.kind(), SurfaceKind::Cone(_)));
    let n = grown.natural_uv_domain_with_nurb(&k).unwrap();
    assert!(n.min().x < d.min().x && n.max().x > d.max().x);
    let uv = Point2::new(d.u_interval().mid(), d.v_interval().mid());
    // the form, not the label: the result is the form's surface
    let form = crate::blend::fillet_section::station::as_b_spline(&k, &s)
        .unwrap()
        .unwrap()
        .into_owned();
    let (a, b) = (
        Surface::from(form).evaluate_point(&k, uv).unwrap(),
        grown.evaluate_point(&k, uv).unwrap(),
    );
    assert!((a - b).length() < 1e-12, "{a:?} {b:?}");
}

/// No knot of `s` repeated past its degree plus one, in either direction (the knot fixup
/// merges a span shorter than the ends' scaled zero onto its neighbour).
fn knots_well_formed(s: &BSplineSurface) -> bool {
    let n = s.gw_nurb().unwrap();
    let ok = |knots: &[f64], p: usize| knots.chunk_by(|a, b| a == b).all(|run| run.len() <= p + 1);
    ok(n.u_knots(), n.p()) && ok(n.v_knots(), n.q())
}

/// The cylinder of the tests below: radius 1, its rulings from z = -1 to 3.
fn unit_cylinder(kk: &Kernel) -> Surface {
    Surface::from(
        crate::primitive::api_primitives::create_cylinder_surface(
            kk,
            crate::math::Point3::new(0.0, 0.0, -1.0),
            1.0,
            4.0,
        )
        .unwrap(),
    )
}

/// A rounding-sized distance (the reach beside a face whose new surface passes through
/// its old edges) is below the side's resolution: the shortest extension its knot can carry
/// (`Local::least_step`, about `EFF_ZERO_PARAM` of the parameter) is proved past it, so no
/// extension is built (one would either put a knot back on the side's own, a span of no
/// length, or carry a real extension the distance never asked for). Both ruling sides come
/// back `Unchanged`, the round closed, and the surface is the cylinder's own form continued
/// by nothing: a B-spline on the cylinder over the source's parameters, its knots well formed
/// (the same family as any longer reach gives, not the analytic kind).
#[test]
fn a_rounding_sized_distance_leaves_a_cylinder_unchanged() {
    use crate::surface::implicit::{lies_on, AnalyticSupport};
    let kk = k();
    let c = unit_cylinder(&kk);
    let on = AnalyticSupport::check(&kk, &c).unwrap().implicit();
    for d in [3.5691962659704166e-16, f64::MIN_POSITIVE] {
        let e = c
            .extended_sides_keeping_parameters(&kk, &BoxSide::ALL, Continuity::Natural, d)
            .unwrap();
        let unchanged = e
            .sides
            .iter()
            .filter(|s| matches!(s, SideExtension::Unchanged))
            .count();
        let closed = e
            .sides
            .iter()
            .filter(|s| matches!(s, SideExtension::Closed))
            .count();
        assert_eq!((unchanged, closed), (2, 2), "{d:e} {:?}", e.sides);
        let SurfaceKind::BSpline(form) = e.surface.kind() else {
            panic!("{d:e}: not the cylinder's B-spline form");
        };
        assert!(lies_on(form, &on).unwrap(), "{d:e}");
        assert!(knots_well_formed(form), "{d:e}");
        assert_eq!(dom(&e.surface), dom(&c), "{d:e}");
        assert_eq!(e.extension, 0.0, "{d:e}");
    }
}

/// Second review, finding 9: the cylinder's ruling sides extended by a rounding-sized
/// distance through `extended_by_distance` (an analytic widened on its own parameters, not
/// kept on the source's): below the side's resolution, `None` (it was refused `NotExact`),
/// and `extended_sides` leaves both sides `Unchanged`, the surface as it is.
#[test]
fn a_rounding_sized_distance_leaves_a_widened_cylinder_unchanged() {
    let kk = k();
    let c = unit_cylinder(&kk);
    for d in [3.5691962659704166e-16, f64::MIN_POSITIVE] {
        for side in [BoxSide::VMin, BoxSide::VMax] {
            let e = c
                .extended_by_distance(&kk, &dom(&c), side, Continuity::Natural, d)
                .unwrap_or_else(|err| panic!("{d:e} {side:?}: {err:?}"));
            assert!(e.is_none(), "{d:e} {side:?}");
        }
        let e = c
            .extended_sides(&kk, &BoxSide::ALL, Continuity::Natural, d)
            .unwrap();
        let unchanged = e
            .sides
            .iter()
            .filter(|s| matches!(s, SideExtension::Unchanged))
            .count();
        assert_eq!(unchanged, 2, "{d:e} {:?}", e.sides);
        assert_eq!(dom(&e.surface), dom(&c), "{d:e}");
    }
    // a distance the widened's measure certifies is still widened (1e-7 and 1e-6 are past
    // the least step but refused `NotExact`: the measure's rounding, 4.6e-12, is above
    // 1e-6 d there; tangency-labs/solidcore#2980)
    let e = c
        .extended_by_distance(&kk, &dom(&c), BoxSide::VMax, Continuity::Natural, 1e-5)
        .unwrap();
    assert!(e.is_some());
}

/// A straight analytic iso has a distance-sized rounding budget even when the widening is
/// much shorter than the cylinder's radius. The axial parameter runs to 3, where one ulp
/// (4.4e-16) is already `STOP` of a distance of 4.4e-7: below that no stored parameter
/// meets the distance, and the answer is none or a refusal, never a claim that it does.
#[test]
fn a_cylinder_widens_by_small_distances_along_its_axis() {
    let kk = k();
    let s = unit_cylinder(&kk);
    for d in [1e-6, 2e-6, 5e-6] {
        let e = s
            .extended_by_distance(&kk, &dom(&s), BoxSide::VMax, Continuity::Natural, d)
            .unwrap_or_else(|err| panic!("by {d:e}: {err:?}"))
            .unwrap_or_else(|| panic!("by {d:e}: not extended"));
        meets(&e, d);
    }
    for d in [5e-9, 1e-8, 1e-7] {
        match s.extended_by_distance(&kk, &dom(&s), BoxSide::VMax, Continuity::Natural, d) {
            Ok(None) | Err(SolidError::ExtendRefused { .. }) => {}
            other => panic!("by {d:e}: {:?}", other.map(|e| e.map(|e| e.across))),
        }
    }
}

/// A ball patch far from the origin keeps the same small-distance certificate as at the
/// origin; the sphere centre is the local frame for its iso.
#[test]
fn a_far_ball_patch_extends_by_the_same_small_distance_as_at_the_origin() {
    use crate::surface::implicit::{lies_on, Implicit};
    let kk = k();
    for o in [0.0, 1e3, 1e5] {
        let c = Vec3::new(o, o, 0.0);
        let r = 2.0;
        let (net, _, _) = crate::blend::blend_corner::ball::ball_triangle_patch(
            c,
            r,
            &[Vec3::X, Vec3::Y, Vec3::Z],
            0,
            false,
        )
        .unwrap();
        let s = Surface::from(crate::surface::ball_patch::BallPatch::create(&net, c, r).unwrap());
        let d = 1e-3;
        let e = s
            .extended_sides(&kk, &[BoxSide::UMin], Continuity::Natural, d)
            .unwrap_or_else(|err| panic!("at {o:e} by {d:e}: {err:?}"));
        let SideExtension::Extended(a) = e.sides[0] else {
            panic!("at {o:e}: {:?}", e.sides);
        };
        assert!(exact(&a, e.extension, d), "at {o:e}: {a:?}");
        assert!(
            (a.enclosure.mid() - d).abs() <= 1e-12 * d,
            "at {o:e}: {a:?}"
        );
        let SurfaceKind::BallPatch(out) = e.surface.kind() else {
            panic!("not a ball patch");
        };
        assert!(lies_on(
            out.net(),
            &Implicit::Sphere {
                centre: c,
                radius: r
            }
        )
        .unwrap());
    }
}

/// A finite plane kept on its parameters at a rounding-sized reach (the local operation's
/// margin beside a face whose new surface passes through its old edges): every side
/// `Unchanged`, and the result is still the plane's own B-spline form over its box (the
/// family a longer reach gives), never the plane kind, which the shell's offset takes other
/// branches for.
#[test]
fn a_rounding_sized_distance_keeps_a_planes_own_form() {
    use crate::surface::implicit::{lies_on, AnalyticSupport};
    let kk = k();
    let p = Surface::from(Plane::new(
        Vec3::ZERO,
        Vec3::X,
        Vec3::Y,
        Vec2::new(1.0, 1.0),
        &Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
    ));
    let on = AnalyticSupport::check(&kk, &p).unwrap().implicit();
    for d in [1.8731196724739204e-14, f64::MIN_POSITIVE] {
        let e = p
            .extended_sides_keeping_parameters(&kk, &BoxSide::ALL, Continuity::Ruled, d)
            .unwrap();
        assert!(
            e.sides
                .iter()
                .all(|x| matches!(x, SideExtension::Unchanged)),
            "{d:e} {:?}",
            e.sides
        );
        let SurfaceKind::BSpline(form) = e.surface.kind() else {
            panic!(
                "{d:e}: not the plane's B-spline form: {:?}",
                e.surface.kind()
            );
        };
        assert!(lies_on(form, &on).unwrap(), "{d:e}");
        assert!(knots_well_formed(form), "{d:e}");
        assert_eq!(dom(&e.surface), dom(&p), "{d:e}");
    }
}

/// A distance at least the least step's length still extends the cylinder's ruling sides,
/// exactly, with every knot repeated at most degree + 1 times.
#[test]
fn a_distance_past_the_least_step_extends_a_cylinder() {
    use crate::surface::implicit::{lies_on, AnalyticSupport};
    let kk = k();
    let c = unit_cylinder(&kk);
    let on = AnalyticSupport::check(&kk, &c).unwrap().implicit();
    // unit speed along the rulings: the least step is about 4 EFF_ZERO_PARAM long
    for d in [1e-7, 1e-6, 0.5] {
        let e = c
            .extended_sides_keeping_parameters(&kk, &BoxSide::ALL, Continuity::Natural, d)
            .unwrap();
        let SurfaceKind::BSpline(grown) = e.surface.kind() else {
            panic!("{d:e}: not a B-spline");
        };
        assert!(lies_on(grown, &on).unwrap(), "{d:e}");
        assert!(knots_well_formed(grown), "{d:e}");
        let mut extended = 0;
        for s in e.sides {
            let SideExtension::Extended(a) = s else {
                continue;
            };
            extended += 1;
            assert!(
                a.enclosure.upper >= d && a.enclosure.lower <= d * (1.0 + 1e-6),
                "{d:e} {a:?}"
            );
        }
        assert_eq!(extended, 2, "{d:e} {:?}", e.sides);
    }
}

/// The parabola with its u knots at `[3, 4]` (speed about 2.2 at x = 1).
fn unit_parabola() -> Surface {
    let row = |x: f64, y: f64| vec![[x, y, 0.0, 1.0], [x, y, 1.0, 1.0]];
    spline(
        &[row(0.0, 0.0), row(0.5, 0.0), row(1.0, 1.0)],
        2,
        1,
        &[3.0, 3.0, 3.0, 4.0, 4.0, 4.0],
        &[0.0, 0.0, 1.0, 1.0],
        false,
    )
}

/// The B-spline path's own solve, at a rounding-sized distance: the least step (the new
/// end knot `EFF_ZERO_PARAM` past 4) is proved past it, so every continuity makes no
/// extension (`None`), and the side is reported `Unchanged` with the surface as it was.
#[test]
fn a_rounding_sized_distance_leaves_a_b_spline_side_unchanged() {
    let s = unit_parabola();
    for how in [
        Continuity::Natural,
        Continuity::Tangent,
        Continuity::Curvature,
        Continuity::Ruled,
    ] {
        for d in [3.5691962659704166e-16, f64::MIN_POSITIVE] {
            let e = s
                .extended_by_distance(&k(), &dom(&s), BoxSide::UMax, how, d)
                .unwrap_or_else(|err| panic!("{how:?} {d:e}: {err:?}"));
            assert!(e.is_none(), "{how:?} {d:e}: extended");
            let e = s
                .extended_sides(&k(), &[BoxSide::UMax], how, d)
                .unwrap_or_else(|err| panic!("{how:?} {d:e}: {err:?}"));
            assert_eq!(
                e.sides[1],
                SideExtension::Unchanged,
                "{how:?} {d:e} {:?}",
                e.sides
            );
            assert_eq!(dom(&e.surface), dom(&s), "{how:?} {d:e}");
        }
    }
}

/// A distance at least the least step's length still extends the parabola's side on every
/// continuity: the source kept (joint knot 4 at multiplicity 2), the new end knot past 4,
/// no knot repeated past degree + 1, the length meeting the distance to the stop. Down to a
/// few times the least step's length (about `2.7e-8` here): the across measure's rounding,
/// on the net's differences from the joint, shrinks with the extension.
#[test]
fn a_distance_past_the_least_step_extends_a_b_spline_side() {
    let s = unit_parabola();
    for how in [
        Continuity::Natural,
        Continuity::Tangent,
        Continuity::Curvature,
        Continuity::Ruled,
    ] {
        for d in [1e-6, 1e-5, 1e-3, 0.1] {
            let e = s
                .extended_by_distance(&k(), &dom(&s), BoxSide::UMax, how, d)
                .unwrap_or_else(|err| panic!("{how:?} {d:e}: {err:?}"))
                .unwrap_or_else(|| panic!("{how:?} {d:e}: not extended"));
            let SurfaceKind::BSpline(b) = e.surface.kind() else {
                panic!("{how:?} {d:e}: not a B-spline");
            };
            let end = dom(&e.surface).max().x;
            assert!(end - 4.0 >= EFF_ZERO_PARAM, "{how:?} {d:e} {end}");
            let knots = b.gw_nurb().unwrap().u_knots().to_vec();
            assert_eq!(knots.iter().filter(|&&x| x == 4.0).count(), 2, "{knots:?}");
            assert!(knots_well_formed(b), "{how:?} {d:e} {knots:?}");
            meets(&e, d);
        }
    }
}

/// A side of a part about 170 from the origin and about 60 across: a cubic row (two spans)
/// along u, or a quarter circle of radius 50 (rational), each swept 50 along z in v (the two
/// rows the same in x, y and weight, so the middle iso is the row's own curve, at the rows'
/// middle z).
fn part_side(rational: bool) -> Surface {
    let (cx, cy) = (170.0, -120.0);
    let row =
        |x: f64, y: f64, w: f64| vec![[w * x, w * y, w * 90.0, w], [w * x, w * y, w * 140.0, w]];
    if rational {
        let h = FRAC_1_SQRT_2;
        spline(
            &[
                row(cx + 50.0, cy, 1.0),
                row(cx + 50.0, cy + 50.0, h),
                row(cx, cy + 50.0, 1.0),
            ],
            2,
            1,
            &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            &[0.0, 0.0, 1.0, 1.0],
            true,
        )
    } else {
        spline(
            &[
                row(cx, cy, 1.0),
                row(cx + 15.0, cy + 12.0, 1.0),
                row(cx + 30.0, cy + 8.0, 1.0),
                row(cx + 45.0, cy + 25.0, 1.0),
                row(cx + 60.0, cy + 20.0, 1.0),
            ],
            3,
            1,
            &[0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0],
            &[0.0, 0.0, 1.0, 1.0],
            false,
        )
    }
}

/// The B-spline basis functions of degree `p` on `knots` at `t` in the span `[k_s, k_(s+1)]`
/// and their derivatives (Cox-de Boor), as `(index, N, N')`.
fn basis(knots: &[f64], p: usize, s: usize, t: f64) -> Vec<(usize, f64, f64)> {
    // degree-0 .. p triangle; `n[j]` the function of index `s - d + j`
    let mut n = vec![1.0];
    let mut prev = vec![1.0];
    for d in 1..=p {
        prev = n.clone();
        let mut next = vec![0.0; d + 1];
        for (j, &v) in prev.iter().enumerate() {
            let i = s + j - (d - 1);
            let (a, b) = (knots[i], knots[i + d]);
            let (c, e) = (knots[i - 1 + 1], knots[i + d]);
            let _ = (c, e);
            // N_(i,d) gets (t - k_i)/(k_(i+d) - k_i) of N_(i,d-1); N_(i-1,d) gets the rest
            let r = if b > a { v / (b - a) } else { 0.0 };
            next[j + 1] += (t - a) * r;
            next[j] += (b - t) * r;
        }
        n = next;
    }
    // derivatives from the degree p - 1 functions
    let mut dn = vec![0.0; p + 1];
    if p > 0 {
        for (j, &v) in prev.iter().enumerate() {
            let i = s + j - (p - 1);
            let (a, b) = (knots[i], knots[i + p]);
            let r = if b > a { p as f64 * v / (b - a) } else { 0.0 };
            dn[j] -= r;
            dn[j + 1] += r;
        }
    }
    (0..=p).map(|j| (s + j - p, n[j], dn[j])).collect()
}

/// A high-precision reference for the length of the middle iso of `e`'s result across its
/// side past the source's `side`: the row's homogeneous points less the joint's point times
/// their weight (each `H - J w` with one rounding, by a fused product: the curve translated
/// by `J` with its points' own size), its speed `|D' w - D w'| / w^2` by Gauss-Legendre over
/// 64 equal steps of every span past the joint. Within about `1e-15` of the length.
fn reference_length(e: &Extended, side: BoxSide) -> f64 {
    let SurfaceKind::BSpline(b) = e.surface.kind() else {
        panic!("not a B-spline");
    };
    let n = b.gw_nurb().unwrap();
    let knots = n.u_knots().to_vec();
    let p = n.p();
    let cpts = n.cpts();
    let h = |i: usize, j: usize| {
        let c = cpts[i][j];
        let w = if c.w == NOW { 1.0 } else { c.w };
        [c.x, c.y, w]
    };
    // the rows the same in x, y and weight: the middle iso is row 0's curve
    for i in 0..cpts.len() {
        assert_eq!(h(i, 0), h(i, 1), "rows differ at {i}");
    }
    let at = e.side_of(side).at(&e.dom);
    let joint = knots.iter().position(|&x| x == at).unwrap();
    // the joint's point: the column whose function is 1 there
    let s = if side.is_high() {
        knots.iter().rposition(|&x| x == at).unwrap() - p
    } else {
        joint - 1
    };
    let [jx, jy, jw] = h(s, 0);
    let (jx, jy) = (jx / jw, jy / jw);
    let d: Vec<[f64; 3]> = (0..cpts.len())
        .map(|i| {
            let [x, y, w] = h(i, 0);
            [(-jx).mul_add(w, x), (-jy).mul_add(w, y), w]
        })
        .collect();
    let (lo, hi) = if side.is_high() {
        (at, *knots.last().unwrap())
    } else {
        (*knots.first().unwrap(), at)
    };
    let gauss = [
        (-0.906_179_845_938_664, 0.236_926_885_056_189_1),
        (-0.538_469_310_105_683_1, 0.478_628_670_499_366_5),
        (0.0, 0.568_888_888_888_888_9),
        (0.538_469_310_105_683_1, 0.478_628_670_499_366_5),
        (0.906_179_845_938_664, 0.236_926_885_056_189_1),
    ];
    let mut total = 0.0;
    for s in p..knots.len() - p - 1 {
        let (a, b) = (knots[s], knots[s + 1]);
        if b <= a || a < lo || b > hi {
            continue;
        }
        let steps = 64;
        for k in 0..steps {
            let (u0, u1) = (
                a + (b - a) * f64::from(k) / f64::from(steps),
                a + (b - a) * f64::from(k + 1) / f64::from(steps),
            );
            for &(x, wt) in &gauss {
                let t = 0.5 * (u0 + u1) + 0.5 * (u1 - u0) * x;
                let (mut c, mut dc) = ([0.0; 3], [0.0; 3]);
                for (i, f, df) in basis(&knots, p, s, t) {
                    for (m, v) in d[i].iter().enumerate() {
                        c[m] += f * v;
                        dc[m] += df * v;
                    }
                }
                let w2 = c[2] * c[2];
                let sx = (dc[0] * c[2] - c[0] * dc[2]) / w2;
                let sy = (dc[1] * c[2] - c[1] * dc[2]) / w2;
                total += 0.5 * (u1 - u0) * wt * sx.hypot(sy);
            }
        }
    }
    total
}

/// A tolerance-sized extension of a side about 170 from the origin, past either end, on a
/// polynomial and a rational net, Natural and Ruled (the local operation's): the across
/// measure works on the net's differences from the joint, so its rounding is the extension's
/// size times a few ulps, not the part's; the construction bound counts a weight's error at
/// the weights' size. (Tangent and Curvature are certified at a far part in difference form:
/// `a_mirror_extension_far_from_the_origin_is_bounded_in_difference_form`.)
///
/// The stop is exact (#2977). The stored points near 170 lie on a lattice of one ulp,
/// `2^-45 = 2.8e-14`, so the length across moves in steps of that order and meets `d` to
/// `1e-9 d` only where a step lands within it: for `d >= s / 2e-9` (`s` the step, 1.4e-5) every
/// `d` is reachable by this argument, and `1e-4` and `1.0133e-3` (a pyramid file's tolerance)
/// are asked below and built. `1e-5` (a B-rep tolerance at part size, which this test once
/// asked and expected built, the stop then widened by the net's resolution) is reached only
/// where a step lands within `1e-14` of it: where it is not, the extension is refused
/// `NotExact`, naming the length it reaches (9.99999998706e-6 for the polynomial net's `UMax`,
/// Ruled), and asking that length builds and meets it. The enclosure of every built extension
/// holds a high-precision reference length and its parts bound the measure's real error.
#[test]
fn a_tolerance_sized_extension_of_a_part_size_side_is_certified() {
    let check = |e: &Extended, side: BoxSide, d: f64, tag: &str| {
        meets(e, d);
        let a = e.across;
        let exact = reference_length(e, side);
        let b = a.enclosure;
        assert!(
            b.lower <= exact && exact <= b.upper,
            "{tag}: {b:?} does not hold {exact}"
        );
        // the measure's real error within its rounding part (its truncation part is the
        // leaves' gap, the rest of the enclosure's width)
        let error = (b.mid() - exact).abs();
        assert!(
            error <= a.rounding + a.truncation,
            "{tag}: error {error:e} past {a:?}"
        );
    };
    let mut refused_polynomial_umax_ruled = false;
    for rational in [false, true] {
        let s = part_side(rational);
        for side in [BoxSide::UMin, BoxSide::UMax] {
            for how in [Continuity::Natural, Continuity::Ruled] {
                let ask = |d: f64| s.extended_by_distance(&k(), &dom(&s), side, how, d);
                // reached: built
                for d in [1e-4, 1.013_333_358_765e-3] {
                    let tag = format!("rational {rational} {side:?} {how:?} {d:e}");
                    let e = ask(d)
                        .unwrap_or_else(|err| panic!("{tag}: {err:?}"))
                        .unwrap_or_else(|| panic!("{tag}: not extended"));
                    check(&e, side, d, &tag);
                }
                // a tolerance whose step may not land within the stop: built, or refused
                // naming the length reached, which builds
                let d = 1e-5;
                let tag = format!("rational {rational} {side:?} {how:?} {d:e}");
                match ask(d) {
                    Ok(Some(e)) => check(&e, side, d, &tag),
                    Err(SolidError::ExtendRefused {
                        refusal: ExtendRefusal::NotExact,
                        distance: Some(named),
                        ..
                    }) => {
                        refused_polynomial_umax_ruled |=
                            !rational && side == BoxSide::UMax && how == Continuity::Ruled;
                        assert!(named < d && d - named < 1e-7 * d, "{tag}: {named:e}");
                        let e = ask(named)
                            .unwrap_or_else(|err| panic!("{tag} named {named:e}: {err:?}"))
                            .unwrap_or_else(|| panic!("{tag} named {named:e}: not extended"));
                        check(&e, side, named, &tag);
                    }
                    other => panic!("{tag}: {:?}", other.map(|e| e.map(|e| e.across))),
                }
            }
        }
        // below the least step: no extension, the side unchanged
        let e = s
            .extended_sides(&k(), &[BoxSide::UMax], Continuity::Ruled, 1e-12)
            .unwrap();
        assert_eq!(e.sides[1], SideExtension::Unchanged, "rational {rational}");
        assert_eq!(dom(&e.surface), dom(&s));
    }
    assert!(
        refused_polynomial_umax_ruled,
        "1e-5 on the polynomial net's UMax, Ruled, is not reached by its lattice"
    );
}

/// A side of the pyramid the difftest unit's `Solids/pyramid.smb` is: a bicubic patch over
/// `[0, 1]^2`, rows along the base (83.6 by 87.1, 101.3 high), its far row collapsed on the
/// apex. Extended Ruled by the file's B-rep tolerance past both ends of its base and then
/// past the base: the base's extension runs along the ends' slivers (spans of about
/// `1.2e-5`), which are no denominator of its extraction in the middle, and is certified.
#[test]
fn a_pyramid_side_extends_by_its_tolerance_past_its_base_after_its_ends() {
    let (x, y, h) = (
        83.555_557_250_976_56,
        87.111_114_501_953_12,
        101.333_335_876_464_84,
    );
    let (a, b, apex) = ([-x, -y, 0.0], [x, -y, 0.0], [0.0, 0.0, h]);
    let third = |p: [f64; 3], q: [f64; 3], i: u32| {
        let t = f64::from(i) / 3.0;
        [
            p[0] + (q[0] - p[0]) * t,
            p[1] + (q[1] - p[1]) * t,
            p[2] + (q[2] - p[2]) * t,
        ]
    };
    let rows: Vec<Vec<[f64; 4]>> = (0..4)
        .map(|i| {
            let foot = third(a, b, i);
            (0..4)
                .map(|j| {
                    let p = third(foot, apex, j);
                    [p[0], p[1], p[2], 1.0]
                })
                .collect()
        })
        .collect();
    let knots = [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
    let s = spline(&rows, 3, 3, &knots, &knots, false);
    let d = 1.013_333_358_765e-3;
    let e = s
        .extended_sides(
            &k(),
            &[BoxSide::UMin, BoxSide::UMax, BoxSide::VMin],
            Continuity::Ruled,
            d,
        )
        .unwrap();
    for (side, done) in BoxSide::ALL.iter().zip(e.sides) {
        if *side == BoxSide::VMax {
            assert_eq!(done, SideExtension::NotAsked);
            continue;
        }
        let SideExtension::Extended(a) = done else {
            panic!("{side:?}: {done:?}");
        };
        assert!(a.rounding <= 1e-6 * d, "{side:?}: {a:?}");
        let tol = (1e-9 * d).max(a.truncation + a.rounding);
        assert!(
            a.enclosure.lower >= d - tol && a.enclosure.upper <= d + tol,
            "{side:?}: {a:?}"
        );
    }
    assert!(e.extension <= 1e-6 * d, "{}", e.extension);
}

/// The cone of the test above, a quarter turn, moved off the origin and extended round its
/// axis by a tolerance-sized distance. The widened path measures the analytic's chord in
/// the part's own frame, so the rounding is the same at the origin and at 1e5.
#[test]
fn a_far_cone_extends_round_its_axis_by_a_tolerance_as_at_the_origin() {
    let kk = k();
    for o in [0.0, 1e3, 1e5] {
        let pl = Axis2Placement::new(Vec3::new(o, o, 0.0), Vec3::Z, Vec3::X);
        let mut c = Cone::create_canonical(&kk, &pl, 2.0, 1.0, 1.0, false, false).unwrap();
        c.adjust_step_uv_domain(&kk, &Extent2d::from_bounds(0.0, 0.0, 90.0, 1.0))
            .unwrap();
        let c = Surface::from(c);
        for d in [1e-5, 1e-4, 1e-3] {
            let e = c
                .extended_by_distance(&kk, &dom(&c), BoxSide::UMin, Continuity::Natural, d)
                .unwrap_or_else(|err| panic!("at {o:e} by {d:e}: {err:?}"))
                .unwrap_or_else(|| panic!("at {o:e} by {d:e}: not extended"));
            meets(&e, d);
        }
    }
}

/// At a side's least step the solve accepts an extension that cannot be made shorter only
/// when its length is proved to reach the distance: an enclosure that merely holds `d`
/// (below it by more than the stop allowance) may be short and is not accepted; one whose
/// lower end is at `d` is.
#[test]
fn least_step_accepts_only_a_length_proved_to_reach() {
    let d = 1.0;
    let across = |lower: f64, upper: f64| Across {
        enclosure: crate::surface::side::Bracket { lower, upper },
        truncation: 0.0,
        rounding: 1e-12,
        converged: true,
        resolution: 0.0,
    };
    assert!(
        !reaches(Some(&across(0.999, 1.001)), 0.0, d),
        "holds d, may be short"
    );
    assert!(
        reaches(Some(&across(1.0, 1.001)), 0.0, d),
        "proved past or at d"
    );
    assert!(
        reaches(Some(&across(1.0 - 5e-10, 1.0)), 0.0, d),
        "within the stop"
    );
    assert!(!reaches(None, 0.0, d), "no measurement");
    let mut unconverged = across(1.0, 1.001);
    unconverged.converged = false;
    assert!(!reaches(Some(&unconverged), 0.0, d), "not measured");
}

/// Tangent asked a little more than one full mirror step: the remainder, half a least step,
/// is shorter than the side's knot can carry, and a mirror step that short leaves a sliver
/// span the knot fixup merges or spreads. What is left is split in two instead, so every
/// span of the extension is at least a least step long, and its end is the length asked.
#[test]
fn tangent_past_one_mirror_step_leaves_no_sliver_span() {
    let s = quarter();
    let SurfaceKind::BSpline(b) = s.kind() else {
        panic!()
    };
    let net = b.gw_nurb().unwrap().clone();
    let source = Local::of(&net, BoxSide::UMax).unwrap();
    let least = source.least_step().unwrap();
    let e = source.range().unwrap() + 0.5 * least;
    let g = source.extended(Continuity::Tangent, e).unwrap().unwrap();
    let mut gaps = g
        .knots
        .windows(2)
        .map(|w| w[1] - w[0])
        .filter(|&gap| gap > 0.0);
    assert!(gaps.all(|gap| gap >= least), "{:?}", g.knots);
    let end = source.end().unwrap() + e;
    assert!(
        (g.end().unwrap() - end).abs() <= 4.0 * f64::EPSILON * end.abs(),
        "{:?}",
        g.knots
    );
}

/// (#3093) Tangent asked a little more than two ranges of a degree 1 side: the second step's
/// reflection falls 4.4e-16 off the joint knot the first step left, which is within the knot
/// fixup's merge scale (1.2e-8). Mirrored there it made a span of 4.4e-16 that the fixup
/// merges, leaving the end knot repeated past `p + 1`. The reflection is put on the knot, so
/// every span of the extension is at least the merge scale, the end knot has `p + 1` copies and
/// it is the ask to within that scale.
#[test]
fn tangent_reflecting_within_the_merge_scale_of_a_knot_leaves_no_sliver_span() {
    let s = parabola();
    let SurfaceKind::BSpline(b) = s.kind() else {
        panic!()
    };
    let net = b.gw_nurb().unwrap().clone();
    let source = Local::of(&net, BoxSide::VMax).unwrap();
    let least = source.least_step().unwrap();
    // two ranges and an ulp: the second step is 1 + 4.4e-16 long, its reflection 4.4e-16 off
    // the knot 1 the first step left
    let e = 2.0 * source.range().unwrap() + 2.0 * f64::EPSILON;
    let g = source.extended(Continuity::Tangent, e).unwrap().unwrap();
    let mut gaps = g
        .knots
        .windows(2)
        .map(|w| w[1] - w[0])
        .filter(|&gap| gap > 0.0);
    assert!(gaps.all(|gap| gap >= least), "{:?}", g.knots);
    let end = g.end().unwrap();
    let copies = g.knots.iter().filter(|&&x| x == end).count();
    assert_eq!(copies, g.p + 1, "{:?}", g.knots);
    // the reflection snapped onto the knot: the end is the ask to within the merge scale
    let asked = source.end().unwrap() + e;
    assert!(
        (end - asked).abs() <= merge_scale(0.0, end),
        "{:?}",
        g.knots
    );
}

/// A degree 1 side `v` over `[0, 10]` with a curved net across (the issue's extrusion: a quadratic
/// in u extruded 10 along z).
fn tall_extrusion() -> Surface {
    let row = |x: f64, y: f64| vec![[x, y, 0.0, 1.0], [x, y, 10.0, 1.0]];
    spline(
        &[row(0.0, 0.0), row(0.5, 0.0), row(1.0, 1.0)],
        2,
        1,
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        &[0.0, 0.0, 10.0, 10.0],
        false,
    )
}

/// The result's end knot has `p + 1` copies and no span is below the merge scale.
fn assert_clamped_and_no_sliver(g: &Local) {
    let end = g.end().unwrap();
    let copies = g.knots.iter().filter(|&&x| x == end).count();
    assert_eq!(copies, g.p + 1, "{:?}", g.knots);
    let scale = merge_scale(g.start().unwrap(), end);
    let mut gaps = g
        .knots
        .windows(2)
        .map(|w| w[1] - w[0])
        .filter(|&gap| gap > 0.0);
    assert!(gaps.all(|gap| gap >= scale), "{:?}", g.knots);
}

/// (#3093) The issue's extrusion, extended 20 past each v side of its face `[0, 10]`: the low
/// side's solved extension is 20 + 3.6e-15 (the knots `[-20.000000000000004 .. 30.0]`), whose
/// second mirror step reflects 3.6e-15 off the joint knot. Tangent and Curvature both leave each
/// end knot at `p + 1` copies and no span below the merge scale.
#[test]
fn the_issues_extrusion_extends_20_past_both_v_sides_with_clamped_ends() {
    let s = tall_extrusion();
    let SurfaceKind::BSpline(b) = s.kind() else {
        panic!()
    };
    let net = b.gw_nurb().unwrap().clone();
    for side in [BoxSide::VMin, BoxSide::VMax] {
        let source = Local::of(&net, side).unwrap();
        for e in [20.0, 20.0 + 3.6e-15, 20.0 - 3.6e-15] {
            for how in [Continuity::Tangent, Continuity::Curvature] {
                let g = source.extended(how, e).unwrap().unwrap();
                assert_clamped_and_no_sliver(&g);
                let reach = g.end().unwrap() - source.end().unwrap();
                assert!(
                    (reach - e).abs() <= merge_scale(0.0, g.end().unwrap()),
                    "{side:?} {how:?} {e}: {:?}",
                    g.knots
                );
            }
        }
    }
}

/// Curvature across the last Bezier span `h` of a multi-span quadratic: asks just below, at and
/// just above `h` meet their reach and keep clamped ends; asks within the merge scale below `h`
/// are the one extension at `h` (the reflection on the knot `c`, where `b - (b - c)` can miss
/// `c` by an ulp), so the extension is continuous in `e`.
#[test]
fn curvature_across_the_last_span_is_continuous_in_the_ask() {
    let row = |x: f64, y: f64| vec![[x, y, 0.0, 1.0], [x, y, 1.0, 1.0]];
    let s = spline(
        &[row(0.0, 0.0), row(0.3, 0.0), row(0.7, 0.5), row(1.0, 1.0)],
        2,
        1,
        &[0.0, 0.0, 0.0, 0.3, 1.0, 1.0, 1.0],
        &[0.0, 0.0, 1.0, 1.0],
        false,
    );
    let SurfaceKind::BSpline(b) = s.kind() else {
        panic!()
    };
    let net = b.gw_nurb().unwrap().clone();
    let source = Local::of(&net, BoxSide::UMax).unwrap();
    let h = source.end().unwrap() - source.before_end(0).unwrap();
    let make = |e: f64| source.extended(Continuity::Curvature, e).unwrap().unwrap();
    for e in [0.9 * h, 1.1 * h] {
        let g = make(e);
        assert_clamped_and_no_sliver(&g);
        let reach = g.end().unwrap() - source.end().unwrap();
        assert!((reach - e).abs() <= 1e-12, "{e}: {:?}", g.knots);
    }
    // within the merge scale of the knot: below it the step snaps onto the knot, the extension
    // the one at `h`; above it the remainder under a least step is split off and the ask met
    let at = make(h);
    for e in [h * (1.0 - 1e-12), h - 1e-15] {
        let g = make(e);
        assert_eq!(g.knots, at.knots, "{e}");
        assert_eq!(g.rows, at.rows, "{e}");
    }
    for e in [h * (1.0 + 1e-12), h + 1e-15] {
        let g = make(e);
        assert_clamped_and_no_sliver(&g);
        let reach = g.end().unwrap() - source.end().unwrap();
        assert!((reach - e).abs() <= 1e-12, "{e}: {:?}", g.knots);
    }
    assert_clamped_and_no_sliver(&at);
}

/// A quadratic with a knot at 0.3 along u, extruded 1 along z.
fn multi_span_quadratic() -> Surface {
    let row = |x: f64, y: f64| vec![[x, y, 0.0, 1.0], [x, y, 1.0, 1.0]];
    spline(
        &[row(0.0, 0.0), row(0.3, 0.0), row(0.7, 0.5), row(1.0, 1.0)],
        2,
        1,
        &[0.0, 0.0, 0.0, 0.3, 1.0, 1.0, 1.0],
        &[0.0, 0.0, 1.0, 1.0],
        false,
    )
}

/// The snap window is the fixup's merge scale (1.2e-8 in parameter): a Tangent step reflecting
/// 1e-9 off the knot 0.3 is the step that reflects on it, bit for bit; one reflecting 2e-8 off
/// it is untouched, the mirror of its own reflection point, which becomes a knot.
#[test]
fn a_reflection_inside_the_merge_scale_snaps_and_one_outside_is_untouched() {
    let s = multi_span_quadratic();
    let SurfaceKind::BSpline(b) = s.kind() else {
        panic!()
    };
    let net = b.gw_nurb().unwrap().clone();
    let source = Local::of(&net, BoxSide::UMax).unwrap();
    let end = source.end().unwrap();
    let on = source
        .extended(Continuity::Tangent, end - 0.3)
        .unwrap()
        .unwrap();
    // above the knot: the snapped step is the longer one, the extension the step on the knot
    let e = end - (0.3 + 1e-9);
    let g = source.extended(Continuity::Tangent, e).unwrap().unwrap();
    assert_eq!(g.knots, on.knots);
    assert_eq!(g.rows, on.rows);
    // below it: the snapped step is the shorter one and leaves a remainder under a least
    // step, split off so the extension reaches the ask
    let e = end - (0.3 - 1e-9);
    let g = source.extended(Continuity::Tangent, e).unwrap().unwrap();
    assert_clamped_and_no_sliver(&g);
    assert!((g.end().unwrap() - end - e).abs() <= merge_scale(0.0, g.end().unwrap()));
    for delta in [2e-8, -2e-8] {
        let e = end - (0.3 + delta);
        let g = source.extended(Continuity::Tangent, e).unwrap().unwrap();
        let direct = source.mirrored(end - e, false).unwrap();
        assert_eq!(g.knots, direct.knots, "{delta}");
        // the extension is made in each row's local frame, so its rows differ from the
        // direct mirror by the frame's translations: one rounding each way, within a few
        // gamma_8 of the net's size
        assert_eq!(g.rows.len(), direct.rows.len(), "{delta}");
        let tol = 4.0 * gamma(8) * g.size();
        for (a, b) in g.rows.iter().zip(&direct.rows) {
            assert_eq!(a.len(), b.len(), "{delta}");
            for (x, y) in a.iter().zip(b) {
                for (u, v) in x.iter().zip(y) {
                    assert!((u - v).abs() <= tol, "{delta}: {u} vs {v}");
                }
            }
        }
        assert_ne!(g.knots, on.knots, "{delta}");
    }
}

/// A snapped step that leaves a remainder shorter than the least step is split in two when it
/// can be, and refused naming the length reached when it cannot: an extension is never short.
#[test]
fn a_snapped_step_leaves_no_short_remainder() {
    let s = multi_span_quadratic();
    let SurfaceKind::BSpline(b) = s.kind() else {
        panic!()
    };
    let net = b.gw_nurb().unwrap().clone();
    let source = Local::of(&net, BoxSide::UMax).unwrap();
    let least = source.least_step().unwrap();
    let end = source.end().unwrap();
    // the first step (the whole piece, 1.0) reflects on the start; the ask is half a least
    // step more: split in two, the end the ask
    let e = source.range().unwrap() + 0.5 * least;
    let g = source.extended(Continuity::Tangent, e).unwrap().unwrap();
    assert!((g.end().unwrap() - end - e).abs() <= merge_scale(0.0, g.end().unwrap()));
    // the second step of an ask 1.0 + 0.7 + 0.5 least: the whole piece [0, 2] is not yet
    // reached by 0.7 (it reflects 0.3 beside the knot 1.3 mirrored): never short
    let e = 0.7 + 0.5 * least;
    match source.extended(Continuity::Tangent, e) {
        Ok(Some(g)) => assert!(
            g.end().unwrap() - end >= e - merge_scale(0.0, g.end().unwrap()),
            "{:?}",
            g.knots
        ),
        Err(SolidError::ExtendRefused {
            refusal: ExtendRefusal::NotExact,
            distance: Some(w),
            ..
        }) => assert!(source.extended(Continuity::Tangent, w).unwrap().is_some()),
        other => panic!("{other:?}"),
    }
}

/// The length solve at an ask whose reflection is within the snap window of a knot: the issue's
/// extrusion (rulings 10 long) extended 20 + 1e-9 past its high side. It meets the length
/// within the contract, or is refused naming a length that works: never Ok short.
#[test]
fn the_length_solve_at_a_snapped_ask_meets_the_length_or_names_one_that_works() {
    let kk = k();
    let s = tall_extrusion();
    let d0 = dom(&s);
    for d in [20.0 + 1e-9, 20.0 + 1e-8, 20.0 + 3e-8] {
        match s.extended_by_distance(&kk, &d0, BoxSide::VMax, Continuity::Tangent, d) {
            Ok(Some(e)) => meets(&e, d),
            Err(SolidError::ExtendRefused {
                refusal: ExtendRefusal::NotExact,
                distance: Some(w),
                ..
            }) => {
                let e = s
                    .extended_by_distance(&kk, &d0, BoxSide::VMax, Continuity::Tangent, w)
                    .unwrap()
                    .unwrap();
                meets(&e, w);
            }
            other => panic!("{d}: {other:?}"),
        }
    }
}

/// A ball patch's side is reported met only when its measured length is within the
/// bisection's stop of the length it was solved to: one the bisection left short (out of
/// tries) is not, nor one whose measure is too coarse to say; at the least step, a length
/// past the goal (no shorter extension exists) is.
#[test]
fn a_ball_side_is_met_only_within_the_bisections_stop() {
    use super::ball::solved_length_met;
    let goal = 2.0;
    let across = |mid: f64, rounding: f64| Across {
        enclosure: crate::surface::side::Bracket {
            lower: mid - rounding,
            upper: mid + rounding,
        },
        truncation: 0.0,
        rounding,
        converged: true,
        resolution: 0.0,
    };
    assert!(solved_length_met(
        &across(goal * (1.0 + 1e-13), 5e-9),
        1e-9,
        goal,
        false
    ));
    assert!(!solved_length_met(
        &across(goal * (1.0 - 1e-6), 5e-9),
        1e-9,
        goal,
        false
    ));
    assert!(
        !solved_length_met(&across(goal, 1e-3), 1e-9, goal, false),
        "measure too coarse"
    );
    assert!(solved_length_met(
        &across(goal * 1.01, 5e-9),
        1e-9,
        goal,
        true
    ));
    assert!(!solved_length_met(
        &across(goal * 0.99, 5e-9),
        1e-9,
        goal,
        true
    ));
}

/// The bilinear sheet of the goldens case `extend_bilinear_huge_scale`, its corners 1e9
/// apart, extended 1e7 on every side, against its closed form: a degree-1 net continued is
/// the same bilinear polynomial, so the result evaluates as `S(u, v)` over the whole new
/// domain (to rounding at the part's scale), and its iso lines are straight, so the length
/// across each side is the straight distance from the old side to the new one, the 1e7 asked
/// (to the solve's stop, 1e-9 of it). Neither check uses the extension's own measure.
#[test]
fn a_huge_bilinear_sheet_extends_as_its_closed_form() {
    let p = [
        [0.0, 0.0, 0.0],
        [0.0, 1e9, 0.0],
        [1e9, 0.0, 62_500_000.0],
        [1e9, 1e9, 187_500_000.0],
    ];
    let s = spline(
        &[
            vec![
                [p[0][0], p[0][1], p[0][2], 1.0],
                [p[1][0], p[1][1], p[1][2], 1.0],
            ],
            vec![
                [p[2][0], p[2][1], p[2][2], 1.0],
                [p[3][0], p[3][1], p[3][2], 1.0],
            ],
        ],
        1,
        1,
        &[0.0, 0.0, 1.0, 1.0],
        &[0.0, 0.0, 1.0, 1.0],
        false,
    );
    let closed = |u: f64, v: f64| {
        let w = [(1.0 - u) * (1.0 - v), (1.0 - u) * v, u * (1.0 - v), u * v];
        let c = |i: usize| w.iter().zip(&p).map(|(w, q)| w * q[i]).sum::<f64>();
        Vec3::new(c(0), c(1), c(2))
    };
    let d = 1e7;
    let e = s
        .extended_sides(&k(), &BoxSide::ALL, Continuity::Natural, d)
        .unwrap();
    let nd = dom(&e.surface);
    assert!(nd.min().x < 0.0 && nd.min().y < 0.0 && nd.max().x > 1.0 && nd.max().y > 1.0);
    for (a, b) in [(0.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.5, 0.5), (0.03, 0.97)] {
        let uv = nd.evaluate(a, b);
        let got = e.surface.evaluate_point(&k(), uv).unwrap();
        let want = closed(uv.x, uv.y);
        assert!(got.distance(want) <= 1e-6, "{uv:?}: {got:?} vs {want:?}");
    }
    let across = [
        closed(nd.min().x, 0.5).distance(closed(0.0, 0.5)),
        closed(nd.max().x, 0.5).distance(closed(1.0, 0.5)),
        closed(0.5, nd.min().y).distance(closed(0.5, 0.0)),
        closed(0.5, nd.max().y).distance(closed(0.5, 1.0)),
    ];
    for l in across {
        assert!((l - d).abs() <= 1e-9 * d, "across {l} vs {d}");
    }
}

/// A round's face box ending 1e-11 short of its definition's end station, extended along
/// the spine: the first piece of spine is far shorter than a span the net's knot fixup keeps,
/// so no ask joins on it. The extension is refused with a typed reason, never built on a
/// sliver span merged or spread away.
#[test]
fn a_round_side_just_before_a_station_is_not_joined_on_a_sliver() {
    let (kk, pb) = round_blend(1, true);
    let s = Surface::from(pb);
    let d0 = s.natural_uv_domain_with_nurb(&kk).unwrap();
    let short = Extent2d::from_bounds(d0.min().x, d0.min().y, d0.max().x - 1e-11, d0.max().y);
    let r = s.extended_by_distance(&kk, &short, BoxSide::UMax, Continuity::Tangent, 3.0);
    match r {
        Err(SolidError::ExtendRefused { .. }) => {}
        other => panic!("{other:?}"),
    }
}

/// Sides extended in turn each meet the distance at the face, not where the sides before
/// them grew the surface. The triangle `(x, y) -> (x, y + x v)`, its corner at u = 0, widens
/// along u, so after `UMax` is extended its v sides are wider at the grown box's middle than at
/// the face's. Its v isos are straight, so each v side's reach at the face's middle (u = 0.5)
/// is a plain distance: the 0.5 asked.
#[test]
fn sides_extended_in_turn_each_meet_the_distance_at_the_face() {
    let kk = k();
    let row = |x: f64, y: f64| vec![[x, y, 0.0, 1.0], [x, y + x, 0.0, 1.0]];
    let t = spline(
        &[row(0.0, 0.0), row(1.0, 0.0), row(2.0, 0.0)],
        2,
        1,
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        &[0.0, 0.0, 1.0, 1.0],
        false,
    );
    let d = 0.5;
    let e = t
        .extended_sides(&kk, &BoxSide::ALL, Continuity::Natural, d)
        .unwrap();
    assert!(
        matches!(e.sides[1], SideExtension::Extended(_)),
        "{:?}",
        e.sides
    );
    let nd = dom(&e.surface);
    let at = |u: f64, v: f64| e.surface.evaluate_point(&kk, Vec2::new(u, v)).unwrap();
    for (v_face, v_end) in [(0.0, nd.min().y), (1.0, nd.max().y)] {
        let reach = at(0.5, v_end).distance(at(0.5, v_face));
        assert!((reach - d).abs() <= 1e-9 * d, "{v_face}: {reach}");
    }
}

/// The parabola of [`parabola`] moved by `(o, o, o)`.
fn moved_parabola(o: f64) -> Surface {
    let row = |x: f64, y: f64| vec![[x + o, y + o, o, 1.0], [x + o, y + o, 1.0 + o, 1.0]];
    spline(
        &[row(0.0, 0.0), row(0.5, 0.0), row(1.0, 1.0)],
        2,
        1,
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        &[0.0, 0.0, 1.0, 1.0],
        false,
    )
}

/// The mirror extension's rounding is measured in difference form about each row's own end
/// point (#2977): at a part 1e5 from the origin its bound is the rounding of moving the points
/// back (`2 gamma_2` of their size) and the origin's own bound, not the mirror's gain on the
/// absolute coordinates (`gamma_12 gain` of 1e5, six times that and more).
#[test]
fn a_mirror_extension_far_from_the_origin_is_bounded_in_difference_form() {
    let kk = k();
    let o = 1e5;
    let floor = 2.0 * gamma(2) * (o + 2.0) * (1.0 + gamma(16));
    for how in [Continuity::Tangent, Continuity::Curvature] {
        let d = 0.4;
        let at = |o: f64| {
            let s = moved_parabola(o);
            s.extended_by_distance(&kk, &dom(&s), BoxSide::UMax, how, d)
                .unwrap_or_else(|err| panic!("{how:?} at {o:e}: {err:?}"))
                .unwrap_or_else(|| panic!("{how:?} at {o:e}: not extended"))
        };
        let (near, far) = (at(0.0), at(o));
        meets(&near, d);
        meets(&far, d);
        assert!(
            far.extension <= 2.0 * floor + 64.0 * near.extension,
            "{how:?}: {:e} at 1e5, {:e} at the origin, floor {floor:e}",
            far.extension,
            near.extension
        );
    }
}

/// A straight strip of two points along x at 1e5, extended by Tangent: the end point's x is
/// stored on the lattice of one ulp (`2^-36` there), so the length across takes only the
/// values `k 2^-36`. A distance half a lattice step off every one of them is not met to the
/// stop (1e-9 of it, 1e-12, is far below the step): the extension is refused as not exact,
/// naming the longest length it does reach, and asking that length succeeds (#2977).
#[test]
fn a_distance_the_stored_net_cannot_resolve_is_refused_naming_the_reachable_length() {
    let kk = k();
    let o = 1e5;
    let row = |z: f64| vec![[o, 0.0, z, 1.0], [o + 1.0, 0.0, z, 1.0]];
    let s = spline(
        &[row(0.0), row(1.0)],
        1,
        1,
        &[0.0, 0.0, 1.0, 1.0],
        &[0.0, 0.0, 1.0, 1.0],
        false,
    );
    // the strip runs along v, the side extended is VMax
    let step = 2f64.powi(-36);
    let d = ((1e-3 / step).round() + 0.5) * step;
    let ask = |d: f64| s.extended_by_distance(&kk, &dom(&s), BoxSide::VMax, Continuity::Tangent, d);
    let Err(SolidError::ExtendRefused {
        refusal: ExtendRefusal::NotExact,
        distance: Some(named),
        ..
    }) = ask(d)
    else {
        panic!("{:?}", ask(d).map(|e| e.map(|e| e.across)));
    };
    assert!(named > 0.0 && named < d, "{named:e} for {d:e}");
    assert!(d - named < 2.0 * step, "{named:e} for {d:e}");
    let e = ask(named)
        .unwrap_or_else(|err| panic!("the named {named:e}: {err:?}"))
        .expect("extended");
    meets(&e, named);
}

/// (#3093, jyviko's review of #3177) A degree 1 piece over `[0, 10]` (the issue's extrusion,
/// `z = v` along its rows) extended Tangent by `e = 20 + ulp(20)`, under #3176's rule:
///
/// - step 1 mirrors the whole piece: the reflection at `a = 0`, the new end `20`;
/// - step 2 asks for the rest, `left = e - 10 = 10 + ulp`; its reflection `20 - left` is an
///   ulp off the knot 10 and snaps onto it, which would leave `ulp` behind, a sliver below
///   `least` (the piece's least step after step 1). The ask is split: the reflection is
///   `r2 = 20 - (left - 2 least)` (no knot within the merge scale of it), the step
///   `first = 20 - r2`, the new end `k4 = 2 * 20 - r2`;
/// - step 3 takes the remainder `left - first`, about `2 least` ("the remainder two least
///   steps"): the reflection `r3 = k4 - (left - first)`, the end `2 k4 - r3`.
///
/// So the knots are `[0, 0, 10, 20, k4, end, end]` with a last span of about `2 least` (2.4e-8
/// here) and the end within `2 least` of 30 (two ulps past it, where one was asked). Each
/// mirror point is `2 Pn - X`, `X` the point knot insertion makes at the reflection (`alpha`
/// of the span's two ends). The property jyviko's review allows: no span shorter than the
/// merge scale and the end within two least steps of 30; and the exact knots and net that
/// rule derives.
#[test]
fn a_degree_one_piece_extended_tangent_by_twenty_and_an_ulp_has_exact_knots_and_net() {
    let s = tall_extrusion();
    let SurfaceKind::BSpline(b) = s.kind() else {
        panic!()
    };
    let net = b.gw_nurb().unwrap().clone();
    let source = Local::of(&net, BoxSide::VMax).unwrap();
    let e = f64::from_bits(20.0_f64.to_bits() + 1);
    let g = source.extended(Continuity::Tangent, e).unwrap().unwrap();
    // the least step after step 1 (the piece mirrored once, over [0, 20])
    let once = source.extended(Continuity::Tangent, 10.0).unwrap().unwrap();
    assert_eq!(once.knots, vec![0.0, 0.0, 10.0, 20.0, 20.0]);
    let least = once.least_step().unwrap();
    // (a) the property
    assert_clamped_and_no_sliver(&g);
    let end = g.end().unwrap();
    assert!((end - 30.0).abs() <= 2.0 * least, "{:?}", g.knots);
    // (b) the knots #3176's rule derives
    let left = e - 10.0;
    let r2 = 20.0 - (left - 2.0 * least);
    let first = 20.0 - r2;
    let k4 = 2.0 * 20.0 - r2;
    let rest = left - first;
    let r3 = k4 - rest;
    let k5 = 2.0 * k4 - r3;
    assert_eq!(g.knots, vec![0.0, 0.0, 10.0, 20.0, k4, k5, k5]);
    // and the net: z by knot insertion at each reflection and the point mirror `2 Pn - X`
    let at = |t: f64, (c, zc): (f64, f64), (d, zd): (f64, f64)| {
        let alpha = (t - c) / (d - c);
        alpha * zd + (1.0 - alpha) * zc
    };
    let z4 = 2.0 * 20.0 - at(r2, (10.0, 10.0), (20.0, 20.0));
    let z5 = 2.0 * z4 - at(r3, (20.0, 20.0), (k4, z4));
    let points = [(0.0, 0.0), (0.5, 0.0), (1.0, 1.0)];
    assert_eq!(g.rows.len(), points.len());
    for (row, &(x, y)) in g.rows.iter().zip(&points) {
        let z: Vec<f64> = row.iter().map(|p| p[2]).collect();
        assert_eq!(z, vec![0.0, 10.0, 20.0, z4, z5], "{:?}", g.rows);
        assert!(row.iter().all(|p| p[0] == x && p[1] == y), "{:?}", g.rows);
    }
}
