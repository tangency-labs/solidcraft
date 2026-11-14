//! WR-646 (#1989; the fork's tangency-labs/smlib@4438859): with
//! `fillet_variable_span_follows_fillet`, `SmFilletIntersector::ComputeNextPoint` rejects
//! a variable round's span whose Hermite rails stray from the fillet solved inside it. On
//! planar faces every uv curve lies on its face, so upstream's test passes any span, the
//! step grows to the angle tolerance and the rails cut across the radius law between the
//! traced points: the round removes the wrong volume and its surface leaves the law's
//! circles.
//!
//! The round of a cube's edge between two perpendicular planes is the union of its balls
//! (WR-685; since the G^n section's flag was retired, on every kernel: WR-685 addendum
//! 2026-11-06). Each ball touches the balls' envelope along its touch circle, square to the
//! centre line and moved along it by `-r r'/|c'|`, not along its great circle. The volume
//! removed is the corner less the union of the balls, by quadrature ([`ENVELOPE_REMOVED`]);
//! the great circles' `(1 - pi/4) r(s)^2` per section, integrated along the edge, removes
//! 65.6068, 2.3% more.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_standard_solver::p1::FilletLaw;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::extent1d::Extent1d;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::p1::{
    BSplineCurve, BSplineCurveForm, CanonicalData, KnotType,
};
use solidcore::nurbs::math::gamma;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::surface_closest_point;
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::face::p2 as face;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
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

/// The cubic Bezier radius law with control radii `r`.
fn cubic_law(r: [f64; 4]) -> FilletLaw {
    let control = r.map(|x| Vec3::new(x, 0.0, 0.0));
    let curve = BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree: 3,
        control_points: &control,
        form: BSplineCurveForm::default(),
        knot_multiplicities: &[4, 4],
        knots: &[0.0, 1.0],
        knot_type: KnotType::default(),
        weights: None,
        trim: None,
    })
    .unwrap();
    FilletLaw::bspline(curve, &Extent1d::from_min_max(0.0, 1.0))
}

/// The law 6, 6, 1, 1 at `t`.
fn law(t: f64) -> f64 {
    let s = 1.0 - t;
    6.0 * s * s * s + 18.0 * t * s * s + 3.0 * t * t * s + t * t * t
}

/// The law's slope `dr/dt` at `t`.
fn law_slope(t: f64) -> f64 {
    let s = 1.0 - t;
    // 3 (6 - 6) s^2 + 6 (1 - 6) s t + 3 (1 - 1) t^2
    -30.0 * s * t
}

/// The point of the ball at `x = 20 t` on its touch circle in the plane of symmetry
/// `y = z`, on the edge's side: the ball of radius `r` centred `c = (x, 20 - r, 20 - r)`
/// touches the balls' envelope along the circle where `<p - c, c'> = -r r'`
/// (`c' = (1, -r', -r')`, `r' = dr/dx`), of radius `r sqrt(1 - r'^2 / |c'|^2)` about
/// `c - (r r' / |c'|^2) c'`.
fn touch_point(x: f64, r: f64, dr_dx: f64) -> Point3 {
    let c = Vec3::new(x, 20.0 - r, 20.0 - r);
    let cd = Vec3::new(1.0, -dr_dx, -dr_dx);
    let n2 = cd.dot(cd);
    let centre = c - cd * (r * dr_dx / n2);
    let radius = r * (1.0 - dr_dx * dr_dx / n2).sqrt();
    let d = Vec3::new(0.0, 1.0, 1.0);
    let d = d - cd * (d.dot(cd) / n2);
    let p = centre + d * (radius / d.length());
    Point3::new(p.x, p.y, p.z)
}

/// The largest distance from the round's surface of the balls' touch points
/// ([`touch_point`]) at `x = 20 t` (taking the law from whichever end of the edge fits it).
fn surface_off_law(k: &Kernel, b: &Brep) -> f64 {
    let rounds: Vec<_> = b
        .faces()
        .ids()
        .filter_map(|f| face::surface(b, f).unwrap())
        .filter(|s| !matches!(s.kind(), SurfaceKind::Plane(_)))
        .collect();
    assert_eq!(rounds.len(), 1, "one round face");
    let s = &rounds[0];
    let off = |reversed: bool| {
        (1..40)
            .map(|i| {
                let x = f64::from(i) * 0.5;
                let t = if reversed { 1.0 - x / 20.0 } else { x / 20.0 };
                let r = law(t);
                let sign = if reversed { -1.0 } else { 1.0 };
                let p = touch_point(x, r, sign * law_slope(t) / 20.0);
                let uv = surface_closest_point(k, s, p).unwrap().uvs[0];
                s.evaluate_point(k, uv).unwrap().distance(p)
            })
            .fold(0.0_f64, f64::max)
    };
    off(false).min(off(true))
}

/// The volume of the 20 mm cube after the law 6, 6, 1, 1 rounds its edge y = z = 20,
/// and the largest distance of the law's sections from the round
/// ([`surface_off_law`]).
fn rounded_cube(fixed: bool) -> (f64, f64) {
    let mut k = Kernel::default();
    k.fixes.fillet_variable_span_follows_fillet = fixed;
    let mut b = create_box(&mut k, Vec3::ZERO, 20.0, 20.0, 20.0).unwrap();
    let e = edge_between(
        &k,
        &b,
        Point3::new(0.0, 20.0, 20.0),
        Point3::new(20.0, 20.0, 20.0),
    );
    {
        let mut session = FilletSession::new(&mut k, &mut b).unwrap();
        session
            .prepare_variable_radius_edges(&mut k, &[e], 6.0, cubic_law([6.0, 6.0, 1.0, 1.0]), 1, 1)
            .unwrap();
        session.do_fillet(&mut k).unwrap();
    }
    let off = surface_off_law(&k, &b);
    (volume(&k, &mut b), off)
}

/// The volume the union of the balls of the law 6, 6, 1, 1 takes off the cube's edge: the
/// integral along the edge of the area each plane `x` cuts between the rails
/// (`y, z > 20 - r(x)`) above the union of the balls' discs (Gauss-Legendre, 16 points in
/// each of 40 panels along the edge and 8 across, the union's top over 20001 balls; 40001
/// balls agree to 1.1e-6).
const ENVELOPE_REMOVED: f64 = 64.159_535;

#[test]
fn variable_round_on_planes_removes_the_laws_volume() {
    let expected = 8000.0 - ENVELOPE_REMOVED;
    let (v, off) = rounded_cube(true);
    eprintln!(
        "fixed: {v} expected {expected} diff {} off {off}",
        v - expected
    );
    // the fork's bound: the law's volume to 1e-4 (here 5e-5 of the volume removed)
    assert!(
        (v - expected).abs() < 1e-4 * ENVELOPE_REMOVED,
        "{v} vs {expected}"
    );
    // the round holds its balls' touch circles
    assert!(off < 3e-4, "{off}");
}

/// Without the span fix the rails stray between the traced points, but the round's skin no
/// longer follows them: it is held to its balls between stations (#15256). So the default
/// kernel's surface is within T = 4 x the edge tolerance (1e-5 mm) of the balls' touch
/// circles (sampled here, at the law's sections). A skin within T of the envelope moves
/// the removed volume by at most about T x the envelope's area (A = 110.7 mm^2 here,
/// 4.43e-3 mm^3), to first order: a necessary condition, which the result before #15256
/// failed (6.1e-4 off the circles, 6.2e-3 mm^3 off the volume). The volume and the area are
/// the balls' closed-form quadratures ([`envelope_removed`], [`envelope_area`]). This test
/// asserted that defect (`off > 5e-4`).
#[test]
fn variable_round_on_planes_holds_its_balls_without_the_span_fix() {
    let reference = envelope_removed(&law_6611, 20.0, 6.0, 24);
    let bound = 4e-5 * envelope_area(&law_6611, 20.0, 24);
    let (v, off) = rounded_cube(false);
    let removed = 8000.0 - v;
    eprintln!(
        "default: removed {removed} reference {reference} diff {} bound {bound} off {off}",
        removed - reference
    );
    assert!(
        (removed - reference).abs() <= bound,
        "{removed} vs {reference}"
    );
    assert!(off < 4e-5, "{off}");
}

// ---- Closed-form ball references for #15256 ----------------------------------------
//
// The round of a straight edge between two perpendicular planes, from its balls alone:
// the ball at edge position `s` has radius `r(s)` and centre `e(s) + r(s) w`, `w` the sum
// of the planes' inward normals. The radius law is a one-span Bezier polynomial in closed
// form. Past the edge's ends it is the same polynomial: upstream's
// `SmBSplineFilletLaw::Evaluate` (`SmFilletExecutive.cpp`) continues a law past its curve's
// domain by its end span, and for a one-span law that is the polynomial itself; the
// skin's stations past the ends are balls of that continued law.

/// The Bezier polynomial with control values `c` and its derivative at `u`.
fn bezier(c: &[f64], u: f64) -> (f64, f64) {
    let n = c.len() - 1;
    let mut b = c.to_vec();
    for k in (1..=n).rev() {
        for i in 0..k {
            b[i] = (1.0 - u) * b[i] + u * b[i + 1];
        }
    }
    let mut d: Vec<f64> = c.windows(2).map(|w| n as f64 * (w[1] - w[0])).collect();
    for k in (1..n).rev() {
        for i in 0..k {
            d[i] = (1.0 - u) * d[i] + u * d[i + 1];
        }
    }
    (b[0], d.first().copied().unwrap_or(0.0))
}

/// The monomial coefficients `a` (`p(u) = sum a_j u^j`) of the Bezier polynomial with
/// control values `c`, degree at most 3.
fn monomial(c: &[f64]) -> [f64; 4] {
    match *c {
        [c0] => [c0, 0.0, 0.0, 0.0],
        [c0, c1] => [c0, c1 - c0, 0.0, 0.0],
        [c0, c1, c2] => [c0, 2.0 * (c1 - c0), c0 - 2.0 * c1 + c2, 0.0],
        [c0, c1, c2, c3] => [
            c0,
            3.0 * (c1 - c0),
            3.0 * (c0 - 2.0 * c1 + c2),
            c3 - 3.0 * c2 + 3.0 * c1 - c0,
        ],
        _ => panic!("degree above 3"),
    }
}

/// The exact range of `p` (monomial, degree at most 3) over `[a, b]`: its values at the
/// ends and at the real roots of `p'` inside.
fn poly_range(p: [f64; 4], a: f64, b: f64) -> (f64, f64) {
    let at = |u: f64| p[0] + u * (p[1] + u * (p[2] + u * p[3]));
    let mut xs = vec![a, b];
    let (qa, qb, qc) = (3.0 * p[3], 2.0 * p[2], p[1]);
    if qa.abs() > 0.0 {
        let disc = qb * qb - 4.0 * qa * qc;
        if disc >= 0.0 {
            xs.push((-qb + disc.sqrt()) / (2.0 * qa));
            xs.push((-qb - disc.sqrt()) / (2.0 * qa));
        }
    } else if qb.abs() > 0.0 {
        xs.push(-qc / qb);
    }
    xs.into_iter()
        .filter(|x| *x >= a && *x <= b)
        .map(at)
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
            (lo.min(v), hi.max(v))
        })
}

/// `p'` of a monomial `p`.
fn derivative(p: [f64; 4]) -> [f64; 4] {
    [p[1], 2.0 * p[2], 3.0 * p[3], 0.0]
}

/// Gauss-Legendre nodes and weights on `[-1, 1]` (Newton on `P_n`).
fn gauss_legendre(n: usize) -> Vec<(f64, f64)> {
    (0..n)
        .map(|i| {
            let mut x = (std::f64::consts::PI * (i as f64 + 0.75) / (n as f64 + 0.5)).cos();
            let mut dp = 0.0;
            for _ in 0..100 {
                let (mut p0, mut p1) = (1.0, x);
                for k in 2..=n {
                    let p2 = ((2 * k - 1) as f64 * x * p1 - (k - 1) as f64 * p0) / k as f64;
                    p0 = p1;
                    p1 = p2;
                }
                dp = n as f64 * (x * p1 - p0) / (x * x - 1.0);
                let dx = p1 / dp;
                x -= dx;
                if dx.abs() < 1e-15 {
                    break;
                }
            }
            (x, 2.0 / ((1.0 - x * x) * dp * dp))
        })
        .collect()
}

/// `int_a^b f`, Gauss-Legendre with 16 points in each of `panels` panels.
fn integrate(a: f64, b: f64, panels: usize, f: &mut dyn FnMut(f64) -> f64) -> f64 {
    let g = gauss_legendre(16);
    let h = (b - a) / panels as f64;
    let mut sum = 0.0;
    for p in 0..panels {
        let mid = a + h * (p as f64 + 0.5);
        for &(x, w) in &g {
            sum += w * 0.5 * h * f(mid + 0.5 * h * x);
        }
    }
    sum
}

/// How a section's area is integrated across it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Across {
    /// Smooth: in `u`, `a = r(s) u^2` (the union's lowest point leaves each plane like
    /// `sqrt(a)`).
    Smooth,
    /// Plain Gauss-Legendre in `a`, as `ENVELOPE_REMOVED`'s quadrature was.
    Plain,
}

/// The lowest point of the union of the balls `r` on the line at distance `a` from one
/// plane in the section `s`: `min_t r_t - sqrt(r_t^2 - (t - s)^2 - (a - r_t)^2)` over the
/// balls `t` within `1.05 r_max` of `s` (`samples` samples, then golden section).
fn union_low(r: &dyn Fn(f64) -> (f64, f64), r_max: f64, samples: u32, s: f64, a: f64) -> f64 {
    let g = |t: f64| {
        let rt = r(t).0;
        let q = rt * rt - (t - s) * (t - s) - (a - rt) * (a - rt);
        if q >= 0.0 {
            rt - q.sqrt()
        } else {
            f64::INFINITY
        }
    };
    let (t0, t1) = (s - 1.05 * r_max, s + 1.05 * r_max);
    let h = (t1 - t0) / f64::from(samples);
    let (mut bi, mut bv) = (0, f64::INFINITY);
    for i in 0..=samples {
        let v = g(t0 + h * f64::from(i));
        if v < bv {
            (bi, bv) = (i, v);
        }
    }
    let c = t0 + h * f64::from(bi);
    let (mut lo, mut hi) = (c - h, c + h);
    let phi = 0.5 * (5.0_f64.sqrt() - 1.0);
    for _ in 0..60 {
        let (m1, m2) = (hi - phi * (hi - lo), lo + phi * (hi - lo));
        if g(m1) < g(m2) {
            hi = m2;
        } else {
            lo = m1;
        }
    }
    bv.min(g(0.5 * (lo + hi)))
}

/// The volume the union of the balls `r` (radius and slope at an edge position) takes
/// off the edge `[0, len]`: the integral along the edge of the area each section `s`
/// cuts between the rails (both distances from the planes below `r(s)`: a ball touches
/// each plane at its own foot) above the union of the balls' discs ([`union_low`]).
/// Gauss-Legendre, 16 points in each of `panels` panels along the edge and `panels / 4`
/// across (or `across_panels` for [`Across::Plain`]).
fn envelope_removed_with(
    r: &dyn Fn(f64) -> (f64, f64),
    len: f64,
    r_max: f64,
    panels: usize,
    samples: u32,
    across: Across,
) -> f64 {
    integrate(0.0, len, panels, &mut |s| {
        let rs = r(s).0;
        match across {
            Across::Smooth => integrate(0.0, 1.0, (panels / 4).max(1), &mut |u| {
                2.0 * rs * u * union_low(r, r_max, samples, s, rs * u * u).clamp(0.0, rs)
            }),
            Across::Plain => integrate(0.0, rs, 8, &mut |a| {
                union_low(r, r_max, samples, s, a).clamp(0.0, rs)
            }),
        }
    })
}

/// [`envelope_removed_with`], smooth across, 600 samples per minimisation.
fn envelope_removed(r: &dyn Fn(f64) -> (f64, f64), len: f64, r_max: f64, panels: usize) -> f64 {
    envelope_removed_with(r, len, r_max, panels, 600, Across::Smooth)
}

/// The arc of the ball at `t` between its feet on the planes with inward normals `na`
/// and `nb`, at the fraction `sigma` of the way: the point and its derivative in
/// `sigma`. The ball touches the balls' envelope along the circle where
/// `<p - c, c'> = -r r'` (centre `c - (r r'/|c'|^2) c'`, radius
/// `r sqrt(1 - r'^2/|c'|^2)`); its feet `c - r na`, `c - r nb` lie on that circle, and
/// the round is the arc between them on the edge's side.
fn touch_arc(
    r: &dyn Fn(f64) -> (f64, f64),
    origin: Vec3,
    e: Vec3,
    na: Vec3,
    nb: Vec3,
    t: f64,
    sigma: f64,
) -> (Vec3, Vec3) {
    let w = na + nb;
    let (rt, dr) = r(t);
    let c = origin + e * t + w * rt;
    let cd = e + w * dr;
    let n2 = cd.dot(cd);
    let m = c - cd * (rt * dr / n2);
    let radius = rt * (1.0 - dr * dr / n2).sqrt();
    let (fa, fb) = (c - na * rt - m, c - nb * rt - m);
    let u1 = fa * (1.0 / fa.length());
    let v = fb - u1 * fb.dot(u1);
    let u2 = v * (1.0 / v.length());
    let phi = (fb.dot(u1) / fb.length()).clamp(-1.0, 1.0).acos();
    let a = sigma * phi;
    (
        m + (u1 * a.cos() + u2 * a.sin()) * radius,
        (u2 * a.cos() - u1 * a.sin()) * (radius * phi),
    )
}

/// The area of the balls' envelope over the edge `[0, len]` ([`touch_arc`]), as
/// `|E_t x E_s|` (`E_t` by central differences), Gauss-Legendre with 16 points in each of
/// `panels` panels both ways.
fn envelope_area(r: &dyn Fn(f64) -> (f64, f64), len: f64, panels: usize) -> f64 {
    let (e, na, nb) = (
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
    );
    integrate(0.0, len, panels, &mut |t| {
        let h = 1e-6 * len;
        integrate(0.0, 1.0, panels, &mut |sigma| {
            let et = (touch_arc(r, Vec3::ZERO, e, na, nb, t + h, sigma).0
                - touch_arc(r, Vec3::ZERO, e, na, nb, t - h, sigma).0)
                * (0.5 / h);
            let es = touch_arc(r, Vec3::ZERO, e, na, nb, t, sigma).1;
            cross(et, es).length()
        })
    })
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

/// The law 6, 6, 1, 1 along the 20 mm edge: radius and slope at `s`.
fn law_6611(s: f64) -> (f64, f64) {
    let (r, dr) = bezier(&[6.0, 6.0, 1.0, 1.0], s / 20.0);
    (r, dr / 20.0)
}

/// The balls of a linear law `r = r0 + b s` fill a solid cone about their centres' line
/// (`|b| < |c'|`): a point at distance `rho` from that line and `tau` along it (from the
/// centre of `s = 0`, in the direction of `c' = e + b w`) is in it iff
/// `rho sqrt(1 - beta^2) <= r0 + beta tau`, `beta = b / |c'|`. The union's lowest point on
/// the line at distance `a` from one plane in the section `s` is where that holds with
/// equality: a quadratic in the other distance. The edge runs along x, the planes are
/// y = 0 and z = 0.
fn cone_low(r0: f64, b: f64, s: f64, a: f64) -> f64 {
    let cd = Vec3::new(1.0, b, b);
    let n = cd.length();
    let d = cd * (1.0 / n);
    let beta = b / n;
    let k = (1.0 - beta * beta).sqrt();
    let origin = Vec3::new(0.0, r0, r0);
    // X(z) = (s, a, z): rho^2 k^2 = (r0 + beta tau)^2 with tau = (X - origin).d
    let f = |z: f64| {
        let x = Vec3::new(s, a, z) - origin;
        let tau = x.dot(d);
        let rho2 = x.dot(x) - tau * tau;
        (rho2 * k * k, r0 + beta * tau)
    };
    // quadratic in z: A z^2 + B z + C = 0 from three samples (exact for a quadratic)
    let g = |z: f64| {
        let (p, q) = f(z);
        p - q * q
    };
    let (g0, g1, g2) = (g(0.0), g(1.0), g(-1.0));
    let qa = 0.5 * (g1 + g2) - g0;
    let qb = 0.5 * (g1 - g2);
    let disc = qb * qb - 4.0 * qa * g0;
    if disc < 0.0 {
        return f64::INFINITY;
    }
    let roots = [
        (-qb - disc.sqrt()) / (2.0 * qa),
        (-qb + disc.sqrt()) / (2.0 * qa),
    ];
    // the lower intersection with the cone's nappe that holds the balls (r0 + beta tau > 0)
    roots
        .into_iter()
        .filter(|z| f(*z).1 > 0.0)
        .fold(f64::INFINITY, f64::min)
}

/// The closed-form quadrature: exact for a constant radius (the corner less a quarter
/// disc, `(1 - pi/4) r^2 len`, and the quarter cylinder's area `pi/2 r len`), its ball
/// search exact for a varying radius (the union of a linear law's balls is a cone, whose
/// lowest point on each line is a quadratic's root, [`cone_low`]), converged for the law
/// 6, 6, 1, 1 (24 panels and 600 samples against 48 and 1200), and near
/// [`ENVELOPE_REMOVED`]. That constant (mm^3) is a quadrature over 20001 discrete balls,
/// converged in the number of balls to 1.1e-6, but with 8 plain Gauss-Legendre panels
/// across a section, where the union's lowest point leaves each plane like `sqrt(a)`:
/// that scheme, run here with the exact ball search, reproduces the constant to 1.3e-6,
/// so the constant's gap to this quadrature (4.53e-4 mm^3) is that scheme's, and on a
/// cylinder of radius 6, whose volume is exact, the same scheme is 1.03e-3 mm^3 low.
#[test]
fn the_balls_quadrature_is_exact_for_a_cylinder_and_a_cone_and_converges() {
    let r = 3.0;
    let cylinder = |_: f64| (r, 0.0);
    let v = envelope_removed(&cylinder, 20.0, r, 8);
    let exact = (1.0 - std::f64::consts::FRAC_PI_4) * r * r * 20.0;
    assert!((v - exact).abs() < 1e-9 * exact, "{v} vs {exact}");
    let a = envelope_area(&cylinder, 20.0, 8);
    let exact = std::f64::consts::FRAC_PI_2 * r * 20.0;
    assert!((a - exact).abs() < 1e-9 * exact, "{a} vs {exact}");

    // the ball search against the cone's closed form, over sections and lines
    let (r0, b) = (1.0, 0.15);
    let linear = |s: f64| (r0 + b * s, b);
    for i in 0..=20 {
        let s = f64::from(i);
        let rs = r0 + b * s;
        for j in 1..20 {
            let a = rs * f64::from(j) / 20.0;
            let searched = union_low(&linear, r0 + b * 25.0, 600, s, a);
            let closed = cone_low(r0, b, s, a);
            assert!(
                (searched - closed).abs() < 1e-10,
                "s {s} a {a}: {searched} vs {closed}"
            );
        }
    }

    let v = envelope_removed_with(&law_6611, 20.0, 6.0, 24, 600, Across::Smooth);
    let fine = envelope_removed_with(&law_6611, 20.0, 6.0, 48, 1200, Across::Smooth);
    eprintln!("removed {v} (48 panels, 1200 samples: {fine}) against {ENVELOPE_REMOVED}");
    assert!((v - fine).abs() < 1e-7, "{v} vs {fine}");

    // ENVELOPE_REMOVED's scheme: 40 panels along, 8 plain panels across
    let old = envelope_removed_with(&law_6611, 20.0, 6.0, 40, 1200, Across::Plain);
    let old_cylinder = envelope_removed_with(&|_| (6.0, 0.0), 20.0, 6.0, 40, 1200, Across::Plain);
    let exact_cylinder = (1.0 - std::f64::consts::FRAC_PI_4) * 36.0 * 20.0;
    eprintln!(
        "old scheme: law {old} (constant {ENVELOPE_REMOVED}, smooth {v}); cylinder r 6 {old_cylinder} (exact {exact_cylinder})"
    );
    assert!(
        (old - ENVELOPE_REMOVED).abs() < 2e-6,
        "{old} vs {ENVELOPE_REMOVED}"
    );
    assert!((v - old - 4.534e-4).abs() < 1e-6, "{v} - {old}");
    assert!(
        (exact_cylinder - old_cylinder - 1.0335e-3).abs() < 1e-6,
        "{old_cylinder} vs {exact_cylinder}"
    );
}

/// A `blend.fillet_executive_p3` law case (the goldens fixture
/// `bspline_radius_law.json`): the box, the edge's index, the law's control radii and
/// edge map, and the solver's continuity.
struct LawCase {
    name: &'static str,
    dims: [f64; 3],
    radius: f64,
    radii: &'static [f64],
    edge_map: [f64; 2],
    edge: usize,
    continuity: u32,
}

/// The six rows #15256 moves.
const MOVED_ROWS: [LawCase; 6] = [
    LawCase {
        name: "law-cube20-e0-deg2-smooth-4-2-4",
        dims: [20.0; 3],
        radius: 4.0,
        radii: &[4.0, 2.0, 4.0],
        edge_map: [0.0, 1.0],
        edge: 0,
        continuity: 1,
    },
    LawCase {
        name: "law-cube20-e0-deg2-smooth-g2",
        dims: [20.0; 3],
        radius: 4.0,
        radii: &[4.0, 2.0, 4.0],
        edge_map: [0.0, 1.0],
        edge: 0,
        continuity: 2,
    },
    LawCase {
        name: "law-cube20-e0-deg3-asymmetric",
        dims: [20.0; 3],
        radius: 2.0,
        radii: &[2.0, 3.0, 5.0, 3.0],
        edge_map: [0.0, 1.0],
        edge: 0,
        continuity: 1,
    },
    LawCase {
        name: "law-cube20-e7-deg3-asymmetric",
        dims: [20.0; 3],
        radius: 2.0,
        radii: &[2.0, 3.0, 5.0, 3.0],
        edge_map: [0.0, 1.0],
        edge: 7,
        continuity: 1,
    },
    LawCase {
        name: "law-box10x20x30-e0-deg2-edge-map-sub-range",
        dims: [10.0, 20.0, 30.0],
        radius: 0.5,
        radii: &[0.5, 3.0, 1.0],
        edge_map: [0.2, 0.8],
        edge: 0,
        continuity: 1,
    },
    // `SmExtent1d(0.8, 0.2)` swaps its ends (`SmExtent1d.cpp`): the same law as the sub-range
    LawCase {
        name: "law-box10x20x30-e0-deg2-edge-map-reversed",
        dims: [10.0, 20.0, 30.0],
        radius: 0.5,
        radii: &[0.5, 3.0, 1.0],
        edge_map: [0.8, 0.2],
        edge: 0,
        continuity: 1,
    },
];

/// The Bezier law of `c` as the session takes it.
fn bezier_law(c: &LawCase) -> FilletLaw {
    let control: Vec<Vec3> = c.radii.iter().map(|r| Vec3::new(*r, 0.0, 0.0)).collect();
    let n = c.radii.len();
    let curve = BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree: n - 1,
        control_points: &control,
        form: BSplineCurveForm::default(),
        knot_multiplicities: &[n, n],
        knots: &[0.0, 1.0],
        knot_type: KnotType::default(),
        weights: None,
        trim: None,
    })
    .unwrap();
    FilletLaw::bspline(curve, &Extent1d::from_min_max(c.edge_map[0], c.edge_map[1]))
}

/// A moved row built as the goldens runner builds it, with its edge's frame and law.
struct BuiltRow {
    k: Kernel,
    body: Brep,
    tol: f64,
    origin: Vec3,
    e: Vec3,
    na: Vec3,
    nb: Vec3,
    len: f64,
    /// The law's monomial coefficients in its own parameter `u`, and the map
    /// `u = m0 + k s` (`k = (m1 - m0) / len`) from the edge position `s`.
    poly: [f64; 4],
    m0: f64,
    slope: f64,
    radii: &'static [f64],
}

impl BuiltRow {
    /// The law's radius and slope at edge position `s`, running from the edge's start
    /// (`reverse` false) or end. The session builds the solver with
    /// `bReverseOrientation = FALSE` (`SetFilletParameters`, `SmFilletExecutive.cpp`: `new
    /// SmVariableRadiusFS(.., *pOptVarRadLaw, FALSE)`), and `SmBSplineFilletLaw::Evaluate`
    /// maps a parameter `t` of the edge interval to `dT = (t - min) / (max - min)` and the
    /// law parameter `m_vEdgeMap.Evaluate(dT)`: the law runs from the edge's start vertex
    /// (its interval's minimum) towards its end.
    fn law(&self, reverse: bool) -> impl Fn(f64) -> (f64, f64) + Copy + Send + Sync + 'static {
        let (len, radii, m0, slope) = (self.len, self.radii, self.m0, self.slope);
        move |s: f64| {
            let f = if reverse { len - s } else { s };
            let (r, dr) = bezier(radii, m0 + slope * f);
            (r, if reverse { -dr * slope } else { dr * slope })
        }
    }

    /// The edge's frame: its start, direction and the two faces' inward normals.
    fn frame(&self) -> Frame {
        Frame {
            origin: self.origin,
            e: self.e,
            na: self.na,
            nb: self.nb,
        }
    }
}

/// A box edge's frame: its start, unit direction and the two faces' inward normals. The
/// ball at edge position `s` is centred at `origin + s e + r(s) (na + nb)`.
#[derive(Clone, Copy)]
struct Frame {
    origin: Vec3,
    e: Vec3,
    na: Vec3,
    nb: Vec3,
}

fn build_row(c: &LawCase) -> BuiltRow {
    let mut k = Kernel::default();
    let [x, y, z] = c.dims;
    let mut b = create_box(&mut k, Vec3::ZERO, x, y, z).unwrap();
    let e = solidcore::topology::brep::p1::edges(&b, None).unwrap()[c.edge];
    let tol = edge::tolerance(&b, e).unwrap();
    let ivl = edge::interval(&b, e).unwrap();
    let curve = edge::curve(&b, e).unwrap().unwrap();
    let (p0, p1) = (
        curve.evaluate_point(&k, ivl.min()).unwrap(),
        curve.evaluate_point(&k, ivl.max()).unwrap(),
    );
    let len = p0.distance(p1);
    let mut inward = Vec::new();
    for (i, d) in c.dims.iter().enumerate() {
        let (a, q) = ([p0.x, p0.y, p0.z][i], [p1.x, p1.y, p1.z][i]);
        if (a - q).abs() < 1e-12 {
            let mut n = [0.0; 3];
            n[i] = if a.abs() < 1e-12 {
                1.0
            } else {
                assert!((a - d).abs() < 1e-12);
                -1.0
            };
            inward.push(Vec3::new(n[0], n[1], n[2]));
        }
    }
    {
        let mut session = FilletSession::new(&mut k, &mut b).unwrap();
        session
            .prepare_variable_radius_edges(&mut k, &[e], c.radius, bezier_law(c), 1, c.continuity)
            .unwrap();
        session.do_fillet(&mut k).unwrap();
    }
    let (m0, m1) = (
        c.edge_map[0].min(c.edge_map[1]),
        c.edge_map[0].max(c.edge_map[1]),
    );
    BuiltRow {
        k,
        body: b,
        tol,
        origin: Vec3::new(p0.x, p0.y, p0.z),
        e: (Vec3::new(p1.x, p1.y, p1.z) - Vec3::new(p0.x, p0.y, p0.z)) * (1.0 / len),
        na: inward[0],
        nb: inward[1],
        len,
        poly: monomial(c.radii),
        m0,
        slope: (m1 - m0) / len,
        radii: c.radii,
    }
}

/// Each moved row's round removes its balls' volume to within T x the envelope's area
/// (T = 4 x the edge tolerance). A first-order consistency check: a skin within T of the
/// envelope moves the volume by at most about that, so a row outside it would be off its
/// balls, but a row inside it is not shown to be within T (that is
/// [`certify_the_moved_rows_skins_within_t`]). The reference volume and area come from
/// the balls in closed form ([`envelope_removed`], [`envelope_area`]); the law runs from
/// each edge's start ([`BuiltRow::law`]).
#[test]
fn the_moved_law_rounds_remove_their_balls_volume() {
    for c in &MOVED_ROWS {
        let mut row = build_row(c);
        let removed = c.dims.iter().product::<f64>() - volume(&row.k, &mut row.body);
        let (len, tol) = (row.len, row.tol);
        let r = row.law(false);
        let r_max = (0..=200)
            .map(|i| r(-0.25 * len + 1.5 * len * f64::from(i) / 200.0).0)
            .fold(0.0_f64, f64::max);
        let reference = envelope_removed(&r, len, r_max, 24);
        let area = envelope_area(&r, len, 24);
        let bound = 4.0 * tol * area;
        eprintln!(
            "{}: removed {removed:.7} reference {reference:.7} diff {:.3e} bound T x A = {:.0e} x {area:.4} = {bound:.3e}",
            c.name,
            removed - reference,
            4.0 * tol
        );
        assert!((removed - reference).abs() <= bound, "{}", c.name);
    }
}

/// Per moved row, the reference volume at twice the panels and twice the ball samples.
#[test]
#[ignore = "proof run for #15257 (about a minute): the quadrature's convergence per row"]
fn the_moved_rows_reference_volumes_converge() {
    for c in &MOVED_ROWS {
        let row = build_row(c);
        let len = row.len;
        let r = row.law(false);
        let r_max = (0..=200)
            .map(|i| r(-0.25 * len + 1.5 * len * f64::from(i) / 200.0).0)
            .fold(0.0_f64, f64::max);
        let v = envelope_removed_with(&r, len, r_max, 24, 600, Across::Smooth);
        let fine = envelope_removed_with(&r, len, r_max, 48, 1200, Across::Smooth);
        eprintln!(
            "{}: reference {v:.10} (48 panels, 1200 samples: {fine:.10}) spread {:.2e}",
            c.name,
            fine - v
        );
        assert!((fine - v).abs() < 1e-7, "{}", c.name);
    }
}

/// The skin's B-spline data on one knot-span patch, bounding its first and second
/// derivatives there.
struct PatchBounds {
    /// `max |S_u|`, `max |S_uu|`, `max |S_v|`, `max |S_vv|` over the patch.
    su: f64,
    suu: f64,
    sv: f64,
    svv: f64,
}

/// The full knot vector from distinct knots and multiplicities.
fn full_knots(knots: &[f64], mults: &[usize]) -> Vec<f64> {
    knots
        .iter()
        .zip(mults)
        .flat_map(|(k, m)| std::iter::repeat_n(*k, *m))
        .collect()
}

/// Bounds of a rational B-spline surface's derivatives on the patch of spans `i` (u) and
/// `j` (v): with `S = A / w` (`A` the weighted points, translated to a point of the patch
/// so `|S - R|` is small), the derivative surfaces of `A` and `w` are B-splines whose
/// control values are scaled differences of the patch's control values, and lie in their
/// convex hull on the patch (non-negative basis summing to 1). Then
/// `S_u = (A_u - (S - R) w_u) / w` and `S_uu = (A_uu - 2 S_u w_u - (S - R) w_uu) / w`, with
/// `w >= ` the patch's least weight and `|S - R| <=` its control points' farthest from `R`.
fn patch_bounds(
    net: &[[f64; 4]],
    nv: usize,
    knots: [&[f64]; 2],
    degrees: [usize; 2],
    span: [usize; 2],
) -> PatchBounds {
    let [ku, kv] = knots;
    let [p, q] = degrees;
    let [i, j] = span;
    let at = |a: usize, b: usize| net[a * nv + b];
    let rows: Vec<usize> = (i - p..=i).collect();
    let cols: Vec<usize> = (j - q..=j).collect();
    let pt = |c: [f64; 4]| Vec3::new(c[0] / c[3], c[1] / c[3], c[2] / c[3]);
    let reference = pt(at(i - p, j - q));
    // translated homogeneous points: (w (x - R), w)
    let hom = |a: usize, b: usize| {
        let c = at(a, b);
        let x = pt(c) - reference;
        [x.x * c[3], x.y * c[3], x.z * c[3], c[3]]
    };
    let mut w_min = f64::INFINITY;
    let mut far = 0.0_f64;
    for &a in &rows {
        for &b in &cols {
            let c = at(a, b);
            w_min = w_min.min(c[3]);
            far = far.max((pt(c) - reference).length());
        }
    }
    assert!(w_min > 0.0, "a non-positive weight");
    let diff = |x: [f64; 4], y: [f64; 4], f: f64| {
        [
            (x[0] - y[0]) * f,
            (x[1] - y[1]) * f,
            (x[2] - y[2]) * f,
            (x[3] - y[3]) * f,
        ]
    };
    let norm3 = |x: [f64; 4]| (x[0] * x[0] + x[1] * x[1] + x[2] * x[2]).sqrt();
    // one direction's first and second derivative control values' bounds
    let bound = |along_u: bool| -> (f64, f64, f64, f64) {
        let (deg, kn, idx) = if along_u { (p, ku, i) } else { (q, kv, j) };
        let others: &[usize] = if along_u { &cols } else { &rows };
        let mut a1 = 0.0_f64;
        let mut w1 = 0.0_f64;
        let mut a2 = 0.0_f64;
        let mut w2 = 0.0_f64;
        for &o in others {
            let h = |m: usize| if along_u { hom(m, o) } else { hom(o, m) };
            let first: Vec<[f64; 4]> = (idx - deg..idx)
                .map(|m| {
                    let den = kn[m + deg + 1] - kn[m + 1];
                    if den > 0.0 {
                        diff(h(m + 1), h(m), deg as f64 / den)
                    } else {
                        [0.0; 4]
                    }
                })
                .collect();
            for d in &first {
                a1 = a1.max(norm3(*d));
                w1 = w1.max(d[3].abs());
            }
            if deg >= 2 {
                for (n, m) in (idx - deg..idx - 1).enumerate() {
                    let den = kn[m + deg + 1] - kn[m + 2];
                    let d = if den > 0.0 {
                        diff(first[n + 1], first[n], (deg - 1) as f64 / den)
                    } else {
                        [0.0; 4]
                    };
                    a2 = a2.max(norm3(d));
                    w2 = w2.max(d[3].abs());
                }
            }
        }
        (a1, w1, a2, w2)
    };
    let derivs = |along_u: bool| {
        let (a1, w1, a2, w2) = bound(along_u);
        let s1 = (a1 + far * w1) / w_min;
        let s2 = (a2 + 2.0 * s1 * w1 + far * w2) / w_min;
        (s1, s2)
    };
    let (su, suu) = derivs(true);
    let (sv, svv) = derivs(false);
    PatchBounds { su, suu, sv, svv }
}

/// A row's law over the balls' positions (`s` in the range): exact ranges of `r`, `|r'|`
/// and `|r''|` from its polynomial, widened by their rounding, and the size that scales
/// the rounding of `psi`.
#[derive(Clone, Copy)]
struct LawBounds {
    r_lo: f64,
    r_hi: f64,
    r1: f64,
    r2: f64,
    /// `|o|_inf + max |s| + 3 r_hi + 1`: with `|p|_inf`, the magnitudes `psi` is formed from.
    scale: f64,
}

impl LawBounds {
    /// `|c'| <= sqrt(1 + 2 r'^2)` (`c' = e + r' w`, `e` normal to `w`, `|w| = sqrt 2`).
    fn c1(&self) -> f64 {
        (1.0 + 2.0 * self.r1 * self.r1).sqrt() * (1.0 + gamma(8))
    }

    /// A bound on the rounding of a computed `psi(s) = |p - c(s)| - r(s)` (and of `rho`):
    /// each coordinate of `c(s) = o + s e + r(s) w` is at most 16 rounded operations from the
    /// data (the Bezier law's de Casteljau included), the difference, the three squares,
    /// their sum, the root and the subtraction 8 more; by the standard model each is within
    /// `gamma(64)` relative of the sum of the magnitudes it is formed from, at most
    /// `4 (|p|_inf + scale)` per coordinate.
    fn psi_err(&self, p: Vec3) -> f64 {
        gamma(64) * 4.0 * (p.x.abs().max(p.y.abs()).max(p.z.abs()) + self.scale)
    }

    /// `|psi''| <= |c'|^2 / rho + |r''| (1 + |w|)` where `rho >= rho_lo > 0`.
    fn psi_ss(&self, rho_lo: f64) -> f64 {
        if rho_lo <= 0.0 {
            return f64::INFINITY;
        }
        (self.c1() * self.c1() / rho_lo + (1.0 + 2.0_f64.sqrt()) * self.r2) * (1.0 + gamma(8))
    }
}

/// A ball sample of the distance search: `(s, psi(s), rho(s))`.
type Sample = (f64, f64, f64);

/// What a node's certificate gives: `|phi|` at the node is at most `abs`, and the nearest
/// ball is `t0`.
struct NodeCert {
    lo: f64,
    hi: f64,
    t0: f64,
}

/// Bounds of the signed distance `phi(p) = min_s psi(s)`, `psi(s) = |p - c(s)| - r(s)`, from
/// `p` to the balls' envelope over the balls at `s` in `range`, by branch and bound, with no
/// assumption on `phi` itself. On an interval `[a, b]`,
/// `psi >= min(psi(a), psi(b)) - m (b - a)^2 / 8` when `|psi''| <= m` there, and
/// `|psi''| <= |c'|^2 / rho + |r''| (1 + |w|)` with `rho = |p - c(s)|`, which is
/// `|c'|`-Lipschitz in `s`, so `rho >= (rho(a) + rho(b)) / 2 - |c'| (b - a) / 2` on the
/// interval: `m` comes from the interval's own ends. An interval whose bound is at least the
/// best value found less `EPS` cannot hold a lower minimum and is dropped; the others are
/// halved. Every computed `psi` is within [`LawBounds::psi_err`] of the true one, which the
/// bounds carry. Returns `lo <= phi(p) <= hi` and the best ball `t0`, which must be inside
/// the range when `interior` (asserted: the balls past the range are not part of the family).
///
/// With `cell = Some(d)`, also certifies that for every point `p'` within `d` of `p` the
/// nearest ball is unique and nondegenerate, so `phi` is twice differentiable there with
/// the Hessian bound [`certify_the_moved_rows_skins_within_t`] uses:
/// - outside `I0 = [t0 - delta, t0 + delta]`, `psi(s; p) > hi + 2 d` (the same branch and bound
///   over the outside, to that threshold). Since `psi` is 1-Lipschitz in `p`,
///   `psi(s; p') >= psi(s; p) - d > hi + d >= psi(t0; p') >= phi(p')`: the nearest ball of
///   every `p'` is in `I0`;
/// - on `I0`, for every `p'`, `psi_ss = (|c'|^2 - (u.c')^2) / rho - r'' (1 + u.w) > 0`, with
///   `u.c' = -psi_s - r'`, `|psi_s(s; p')| <= |psi_s(t0; p)| + M_ss delta + (|c'| / rho) d =: P`,
///   so `|c'|^2 - (u.c')^2 >= 1 + 2 r'^2 - (P + |r'|)^2 >= 1 - 2 P^2`: `psi(.; p')` is
///   strictly convex on `I0`, its minimum there unique and nondegenerate.
///
/// Within the envelope's reach `phi` is the signed distance to the envelope on both sides:
/// outside the union it is the distance to the union, and inside, the envelope of the balls
/// shrunk by `d` is the envelope's inner offset at `d` (a canal surface's offsets are the
/// canal surfaces of its spine with the radii moved).
fn node_certificate(
    row: &Frame,
    r: &dyn Fn(f64) -> (f64, f64),
    p: Vec3,
    range: (f64, f64),
    lb: &LawBounds,
    interior: bool,
    cell: Option<f64>,
) -> Result<NodeCert, String> {
    const EPS: f64 = 1e-9;
    let w = row.na + row.nb;
    let err = lb.psi_err(p);
    let c1 = lb.c1();
    // (psi, rho) at s
    let at = |s: f64| {
        let (rs, _) = r(s);
        let rho = (p - (row.origin + row.e * s + w * rs)).length();
        (rho - rs, rho)
    };
    // the interval's `m`, from its ends' `rho`
    let m_of = |a: Sample, b: Sample| {
        let rho_lo = 0.5 * (a.2 + b.2) - c1 * (b.0 - a.0) / 2.0 - 2.0 * err;
        lb.psi_ss(rho_lo)
    };
    let drop_below = |a: Sample, b: Sample, level: f64| {
        let h = b.0 - a.0;
        let bound = a.1.min(b.1) - err - m_of(a, b) * h * h / 8.0 * (1.0 + gamma(8));
        bound >= level
    };
    let n = 64;
    let h = (range.1 - range.0) / f64::from(n);
    let samples: Vec<Sample> = (0..=n)
        .map(|i| {
            let s = if i == n {
                range.1
            } else {
                range.0 + h * f64::from(i)
            };
            let (f, rho) = at(s);
            (s, f, rho)
        })
        .collect();
    let (mut t0, mut best) = samples
        .iter()
        .fold((range.0, f64::INFINITY), |acc, &(s, v, _)| {
            if v < acc.1 {
                (s, v)
            } else {
                acc
            }
        });
    let mut stack: Vec<(Sample, Sample)> = samples.windows(2).map(|x| (x[0], x[1])).collect();
    let mut evaluations = 0usize;
    while let Some((a, b)) = stack.pop() {
        if drop_below(a, b, best - EPS - err) {
            continue;
        }
        let c = 0.5 * (a.0 + b.0);
        let (fc, rho_c) = at(c);
        evaluations += 1;
        if evaluations > 200_000 || b.0 - a.0 < 1e-12 {
            return Err("uncertified: the distance search did not close".into());
        }
        if fc < best {
            (t0, best) = (c, fc);
        }
        stack.push((a, (c, fc, rho_c)));
        stack.push(((c, fc, rho_c), b));
    }
    if interior && !(t0 > range.0 + h && t0 < range.1 - h) {
        return Err("the nearest ball is at the range's end".into());
    }
    let (lo, hi) = (best - EPS - 2.0 * err, best + err);
    let Some(d) = cell else {
        return Ok(NodeCert { lo, hi, t0 });
    };

    // the nearest ball of every point within `d` lies in `I0`
    let rho0 = best + r(t0).0;
    let delta = (8.0 * rho0 * (2.0 * d + 1e-9)).sqrt();
    let (ia, ib) = (t0 - delta, t0 + delta);
    if !(ia > range.0 && ib < range.1) {
        return Err("the nearest ball's neighbourhood leaves the range".into());
    }
    let threshold = hi + 2.0 * d + err;
    let mut outside: Vec<Sample> = samples
        .iter()
        .copied()
        .filter(|x| x.0 < ia || x.0 > ib)
        .collect();
    for s in [ia, ib] {
        let (f, rho) = at(s);
        outside.push((s, f, rho));
    }
    outside.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut stack: Vec<(Sample, Sample)> = outside
        .windows(2)
        .filter(|x| !(x[0].0 < ib && x[1].0 > ia))
        .map(|x| (x[0], x[1]))
        .collect();
    while let Some((a, b)) = stack.pop() {
        if drop_below(a, b, threshold) {
            continue;
        }
        let c = 0.5 * (a.0 + b.0);
        let (fc, rho_c) = at(c);
        evaluations += 1;
        if evaluations > 400_000 || b.0 - a.0 < 1e-9 {
            return Err("uncertified: a second ball is as near".into());
        }
        stack.push((a, (c, fc, rho_c)));
        stack.push(((c, fc, rho_c), b));
    }

    // strict convexity on `I0` for every point within `d`
    let (rt, drt) = r(t0);
    let cpt = row.origin + row.e * t0 + w * rt;
    let cd = row.e + w * drt;
    let u = (p - cpt) * (1.0 / rho0);
    let psi_s0 = -u.dot(cd) - drt;
    let rho_min = rho0 - c1 * delta - d - 2.0 * err;
    let rho_max = rho0 + c1 * delta + d + 2.0 * err;
    if rho_min <= 0.0 {
        return Err("uncertified: a ball's centre too near".into());
    }
    // `psi_s0`'s rounding: `u` is `p - c(t0)` (each coordinate within `err`) over `rho0`
    // (within `2 err`), so `|u - u_true| <= 4 err / rho0` (`|u| <= 1 + that`), times `|c'| <= c1`
    // in the dot product; the dot product and the sum add `gamma(32) (c1 + |r'| + 1)`
    let psi_s0_err = 4.0 * err / rho_min * c1 + gamma(32) * (c1 + lb.r1 + 1.0);
    let big_p = (psi_s0.abs() + psi_s0_err) + lb.psi_ss(rho_min) * delta + c1 / rho_min * d;
    let convex = ((1.0 - 2.0 * big_p * big_p) / rho_max - (1.0 + 2.0_f64.sqrt()) * lb.r2)
        * (1.0 - gamma(16));
    if convex <= 0.0 {
        return Err(format!("uncertified: psi_ss lower bound {convex}"));
    }
    Ok(NodeCert { lo, hi, t0 })
}

/// A knot-span patch of the skin's control net, evaluated without the kernel
/// (Cox-de Boor on the span's nonzero basis functions).
struct Patch {
    span: [usize; 2],
    degrees: [usize; 2],
    /// The span's `(p + 1) x (q + 1)` homogeneous control points.
    hom: Vec<[f64; 4]>,
}

impl Patch {
    fn new(net: &[[f64; 4]], nv: usize, degrees: [usize; 2], span: [usize; 2]) -> Self {
        let [p, q] = degrees;
        let [i, j] = span;
        let mut hom = Vec::new();
        for a in i - p..=i {
            for b in j - q..=j {
                // (the net's points are stored weighted: `(w x, w y, w z, w)`)
                hom.push(net[a * nv + b]);
            }
        }
        Self { span, degrees, hom }
    }

    /// The `deg + 1` basis functions nonzero on span `i` at `x` (The NURBS Book, A2.2).
    fn basis(kn: &[f64], i: usize, deg: usize, x: f64) -> Vec<f64> {
        let mut n = vec![0.0_f64; deg + 1];
        let mut left = vec![0.0_f64; deg + 1];
        let mut right = vec![0.0_f64; deg + 1];
        n[0] = 1.0;
        for j in 1..=deg {
            left[j] = x - kn[i + 1 - j];
            right[j] = kn[i + j] - x;
            let mut saved = 0.0;
            for r in 0..j {
                let temp = n[r] / (right[r + 1] + left[j - r]);
                n[r] = saved + right[r + 1] * temp;
                saved = left[j - r] * temp;
            }
            n[j] = saved;
        }
        n
    }

    fn eval(&self, knots: [&[f64]; 2], u: f64, v: f64) -> Vec3 {
        let [p, q] = self.degrees;
        let bu = Self::basis(knots[0], self.span[0], p, u);
        let bv = Self::basis(knots[1], self.span[1], q, v);
        let mut acc = [0.0_f64; 4];
        for (a, nu) in bu.iter().enumerate() {
            for (b, nv) in bv.iter().enumerate() {
                let c = self.hom[a * (q + 1) + b];
                for (x, y) in acc.iter_mut().zip(c) {
                    *x += nu * nv * y;
                }
            }
        }
        Vec3::new(acc[0] / acc[3], acc[1] / acc[3], acc[2] / acc[3])
    }
}

/// A patch of the certification and its cells: `(start, end, count)` each way, its
/// derivative bounds and the rounding of its evaluation.
struct Work {
    patch: Patch,
    u: (f64, f64, usize),
    v: (f64, f64, usize),
    /// `max |S_u|`, `max |S_uu|`, `max |S_v|`, `max |S_vv|` on the patch, rounding included.
    derivs: [f64; 4],
    /// The rounding of the patch's evaluation.
    eps_s: f64,
}

/// Past this many cells a row is uncertified (its run would take hours).
const CELL_CAP: usize = 600_000_000;

/// The rail offset's certified supremum is the best value found plus this.
const W_EPS: f64 = 1e-9;

/// Certifies that each moved row's round face lies within T of its balls' envelope (G2),
/// and computes the bound an argument gives for the other direction (G3, argued, not
/// certified).
///
/// G2. The deviation is `f = phi o S`, `S` the face's surface and `phi` the signed distance
/// to the envelope ([`node_certificate`]). On a cell `[u0, u0 + h_u] x [v0, v0 + h_v]`,
/// `|f| <= max |f(corners)| + h_u^2 / 8 max |f_uu| + h_v^2 / 8 max |f_vv|` (the bilinear
/// interpolant's remainder), with `|f_uu| <= |S_uu| + K |S_u|^2` (`|grad phi| = 1`,
/// `|hess phi| <= K`). `S_u`, `S_uu` are bounded per knot-span patch from the control net
/// ([`patch_bounds`]). Each node's certificate shows that every point within the cell's
/// reach `d` (its diameter plus the node's evaluation error) has a unique, nondegenerate
/// nearest ball, so `phi` is twice differentiable on the cell. `K` comes from the law in
/// closed form: at the nearest ball `t*`, `hess phi = hess_p psi - m m^T / psi_tt`
/// (`psi = |p - c(t)| - r(t)`, `m = d/dt grad_p psi`), `|hess_p psi| = 1 / rho`,
/// `|m|^2 = (1 + r'^2) / rho^2` and `psi_tt >= 1 / rho_max - |r''| (1 + sqrt 2) > 0`, with
/// `rho` bounded per cell a priori from `|phi| <= min |phi(corners)| + d` (1-Lipschitz),
/// never from the result. The ranges of `r`, `r'`, `r''` over the balls' positions come from
/// the law's polynomial ([`poly_range`]). Every bound carries its rounding
/// ([`LawBounds::psi_err`], the patch evaluation's `eps_s`, and `gamma` factors). Every
/// cell of the face's whole uv box is covered (its boundary and rails too, no point
/// classified away). The skin is evaluated from its control net ([`Patch`], checked against
/// the kernel at each patch's middle). The cells are sized from a coarse estimate of the
/// largest distance, a third of the margin to T each way; past 600M cells the row is
/// uncertified and fails.
///
/// The smooth-g2 and reversed rows build the same face, control net, knots and uv domain as
/// the smooth-4-2-4 and sub-range rows (asserted), so four rows are certified.
/// The face's u runs along the edge and v across it (asserted). The law runs from the
/// edge's start. For the rows whose law is not symmetric the reversed law is far off
/// (asserted); the smooth-4-2-4 law is symmetric, so its orientation cannot be told apart.
///
/// Also certified: how far the rails lie off their planes (`h_rail`, from the rail
/// columns' control points), and how far they lie off the law's contact lines (`w`, by a
/// 1-D branch and bound; reported, #15600).
///
/// G3, argued given premises P1 to P3, not certified: the bound on the distance from the true
/// round to the body that the argument in the code gives (see the comment there, which also
/// names a step that does not hold in general). P1 is asserted at the nodes only, P2 is not
/// checked, and P3 rests on the measured column: the distance from the true round (the touch
/// arcs between the feet inside the box) to the body's boundary by closest points.
#[test]
#[ignore = "proof run for #15257 (about 47 minutes on 8 threads): the certified skin-to-envelope bound"]
fn certify_the_moved_rows_skins_within_t() {
    use solidcore::geom::vector2d::Point2;
    use solidcore::nurbs::b_spline_surface::p1::SurfParamType;
    // the smooth-g2 and reversed rows build the same round face as the rows certified
    let net_of = |c: &LawCase| {
        let row = build_row(c);
        let rounds: Vec<_> = row
            .body
            .faces()
            .ids()
            .filter(|&f| {
                face::surface(&row.body, f)
                    .unwrap()
                    .is_some_and(|s| !matches!(s.kind(), SurfaceKind::Plane(_)))
            })
            .collect();
        assert_eq!(rounds.len(), 1, "{}: one round face", c.name);
        let srf = face::surface(&row.body, rounds[0])
            .unwrap()
            .unwrap()
            .clone();
        let dom = face::uv_domain(&row.body, rounds[0]).unwrap();
        let SurfaceKind::BSpline(bs) = srf.kind() else {
            panic!("{}: the round is not a B-spline", c.name)
        };
        let (nu, nv, pts) = bs.control_points().unwrap();
        let pts: Vec<[f64; 4]> = pts.iter().map(|p| [p.x, p.y, p.z, p.w]).collect();
        (
            nu,
            nv,
            pts,
            bs.knots(SurfParamType::U, None).unwrap(),
            bs.knots(SurfParamType::V, None).unwrap(),
            [dom.min().x, dom.min().y, dom.max().x, dom.max().y],
        )
    };
    for (a, b) in [(1, 0), (5, 4)] {
        assert!(
            net_of(&MOVED_ROWS[a]) == net_of(&MOVED_ROWS[b]),
            "{} differs from {}",
            MOVED_ROWS[a].name,
            MOVED_ROWS[b].name
        );
    }
    for c in MOVED_ROWS
        .iter()
        .filter(|c| !c.name.ends_with("smooth-g2") && !c.name.ends_with("reversed"))
    {
        let row = build_row(c);
        let t = 4.0 * row.tol;
        let r = row.law(false);
        let range = (-0.25 * row.len, 1.25 * row.len);
        // the law's exact ranges over the balls' positions
        let ua = row.m0 + row.slope * range.0;
        let ub = row.m0 + row.slope * range.1;
        let (ua, ub) = (ua.min(ub), ua.max(ub));
        let (r_lo, r_hi) = poly_range(row.poly, ua, ub);
        let d1 = poly_range(derivative(row.poly), ua, ub);
        let d2 = poly_range(derivative(derivative(row.poly)), ua, ub);
        let r1 = d1.0.abs().max(d1.1.abs()) * row.slope.abs();
        let r2 = d2.0.abs().max(d2.1.abs()) * row.slope * row.slope;
        // (the ranges come from the polynomial's coefficients and roots in a few dozen
        // rounded operations: widened by gamma(64) of the magnitudes)
        let widen = gamma(64) * (r_hi.abs() + 10.0);
        let lb = LawBounds {
            r_lo: r_lo - widen,
            r_hi: r_hi + widen,
            r1: r1 * (1.0 + gamma(64)) + widen,
            r2: r2 * (1.0 + gamma(64)) + widen,
            scale: row
                .origin
                .x
                .abs()
                .max(row.origin.y.abs())
                .max(row.origin.z.abs())
                + range.0.abs().max(range.1.abs())
                + 3.0 * (r_hi + widen)
                + 1.0,
        };
        let (r_lo, r_hi, r1, r2) = (lb.r_lo, lb.r_hi, lb.r1, lb.r2);
        // `K`, the bound on `|hess phi|`, for a cell where `|phi| <= big_phi` (a-priori, from the
        // cell's corners): at the nearest ball `t*`, `hess phi = hess_p psi - m m^T / psi_tt`,
        // `|hess_p psi| = 1 / rho`, `|m|^2 = (1 + r'^2) / rho^2`,
        // `psi_tt = (1 + r'^2) / rho - r'' (1 + u.w) >= 1 / rho_max - |r''| (1 + sqrt 2)`, with
        // `rho = phi + r(t*)` in `[r_lo - big_phi, r_hi + big_phi]`
        let k_of = |big_phi: f64| -> Option<f64> {
            let (rho_lo, rho_hi) = (r_lo - big_phi, r_hi + big_phi);
            let psi_tt = 1.0 / rho_hi - r2 * (1.0 + 2.0_f64.sqrt());
            (rho_lo > 0.0 && psi_tt > 0.0).then(|| {
                (1.0 / rho_lo + (1.0 + r1 * r1) / (rho_lo * rho_lo * psi_tt)) * (1.0 + gamma(16))
            })
        };
        // (for sizing the cells only)
        let kk = k_of(1e-3).unwrap();

        let rounds: Vec<_> = row
            .body
            .faces()
            .ids()
            .filter(|&f| {
                face::surface(&row.body, f)
                    .unwrap()
                    .is_some_and(|s| !matches!(s.kind(), SurfaceKind::Plane(_)))
            })
            .collect();
        assert_eq!(rounds.len(), 1, "{}: one round face", c.name);
        let face = rounds[0];
        let srf = face::surface(&row.body, face).unwrap().unwrap().clone();
        let SurfaceKind::BSpline(bs) = srf.kind() else {
            panic!("{}: the round is not a B-spline", c.name)
        };
        let dom = face::uv_domain(&row.body, face).unwrap();
        let (u0, u1, v0, v1) = (dom.min().x, dom.max().x, dom.min().y, dom.max().y);
        let k = &row.k;
        let eval = |u: f64, v: f64| {
            let p = srf.evaluate_point(k, Point2::new(u, v)).unwrap();
            Vec3::new(p.x, p.y, p.z)
        };
        // u runs along the edge, v across it
        let vm = 0.5 * (v0 + v1);
        let um = 0.5 * (u0 + u1);
        let along_u = (eval(u1, vm) - eval(u0, vm)).dot(row.e).abs();
        let along_v = (eval(um, v1) - eval(um, v0)).dot(row.e).abs();
        assert!(
            along_u > 10.0 * along_v,
            "{}: u does not run along the edge",
            c.name
        );

        // the control net, its layout checked against the kernel's evaluation
        let (nu, nv, pts) = bs.control_points().unwrap();
        let net: Vec<[f64; 4]> = pts
            .iter()
            .map(|p| {
                if p.w == solidcore::nurbs::math::NOW {
                    [p.x, p.y, p.z, 1.0]
                } else {
                    [p.x, p.y, p.z, p.w]
                }
            })
            .collect();
        let (pu, pv) = (bs.degree(SurfParamType::U), bs.degree(SurfParamType::V));
        let (kud, kum) = bs.knots(SurfParamType::U, None).unwrap();
        let (kvd, kvm) = bs.knots(SurfParamType::V, None).unwrap();
        let (ku, kv) = (full_knots(&kud, &kum), full_knots(&kvd, &kvm));
        assert_eq!(ku.len(), nu + pu + 1);
        assert_eq!(kv.len(), nv + pv + 1);
        let de_boor = |u: f64, v: f64| {
            let basis = |kn: &[f64], deg: usize, n: usize, x: f64| -> Vec<f64> {
                // Cox-de Boor, all n functions at x (the last span closed)
                let m = kn.len() - 1;
                let mut nb: Vec<f64> = (0..m)
                    .map(|i| {
                        let last = x >= kn[m] && kn[i] < kn[i + 1] && kn[i + 1] >= kn[m];
                        if (kn[i] <= x && x < kn[i + 1]) || last {
                            1.0
                        } else {
                            0.0
                        }
                    })
                    .collect();
                for d in 1..=deg {
                    for i in 0..m - d {
                        let a = if kn[i + d] > kn[i] {
                            (x - kn[i]) / (kn[i + d] - kn[i]) * nb[i]
                        } else {
                            0.0
                        };
                        let b = if kn[i + d + 1] > kn[i + 1] {
                            (kn[i + d + 1] - x) / (kn[i + d + 1] - kn[i + 1]) * nb[i + 1]
                        } else {
                            0.0
                        };
                        nb[i] = a + b;
                    }
                }
                nb.truncate(n);
                nb
            };
            let (bu, bv) = (basis(&ku, pu, nu, u), basis(&kv, pv, nv, v));
            let mut acc = [0.0; 4];
            for a in 0..nu {
                for b in 0..nv {
                    let wgt = bu[a] * bv[b];
                    for (x, y) in acc.iter_mut().zip(net[a * nv + b]) {
                        *x += wgt * y;
                    }
                }
            }
            Vec3::new(acc[0] / acc[3], acc[1] / acc[3], acc[2] / acc[3])
        };
        for (fu, fv) in [(0.1, 0.2), (0.5, 0.5), (0.9, 0.7), (0.33, 0.95)] {
            let (u, v) = (u0 + fu * (u1 - u0), v0 + fv * (v1 - v0));
            let off = (de_boor(u, v) - eval(u, v)).length();
            assert!(off < 1e-9, "{}: control net layout ({off})", c.name);
        }

        let frame = row.frame();
        let distance = |p: Vec3| {
            let n = node_certificate(&frame, &r, p, range, &lb, true, None)
                .unwrap_or_else(|e| panic!("{}: {e}", c.name));
            n.lo.abs().max(n.hi.abs())
        };

        // the cells' size: a third of the margin to T each way, from a coarse estimate of
        // the largest distance (this sizes the cells only; the bound is the certificate's)
        let mut estimate = 0.0_f64;
        for a in 0..=80 {
            for b in 0..=10 {
                let p = eval(
                    u0 + (u1 - u0) * f64::from(a) / 80.0,
                    v0 + (v1 - v0) * f64::from(b) / 10.0,
                );
                estimate = estimate.max(distance(p));
            }
        }
        let budget = (t - 1.05 * estimate) / 3.0;
        assert!(
            budget > 0.0,
            "{}: uncertified: estimated distance {estimate} leaves no margin to T {t}",
            c.name
        );

        // the patches over the face's uv box, each checked against the kernel's evaluation
        let spans = |kn: &[f64], deg: usize, lo: f64, hi: f64| -> Vec<(usize, f64, f64)> {
            (deg..kn.len() - deg - 1)
                .filter(|&i| kn[i + 1] > kn[i] && kn[i + 1] > lo && kn[i] < hi)
                .map(|i| (i, kn[i].max(lo), kn[i + 1].min(hi)))
                .collect()
        };
        let mut work = Vec::new();
        let mut total = 0usize;
        for &(i, ua_, ub_) in &spans(&ku, pu, u0, u1) {
            for &(j, va_, vb_) in &spans(&kv, pv, v0, v1) {
                let pb = patch_bounds(&net, nv, [&ku, &kv], [pu, pv], [i, j]);
                let muu = pb.suu + kk * pb.su * pb.su;
                let mvv = pb.svv + kk * pb.sv * pb.sv;
                let n_u = (((ub_ - ua_) * (muu / (8.0 * budget)).sqrt()).ceil() as usize).max(1);
                let n_v = (((vb_ - va_) * (mvv / (8.0 * budget)).sqrt()).ceil() as usize).max(1);
                let patch = Patch::new(&net, nv, [pu, pv], [i, j]);
                let (um_, vm_) = (0.5 * (ua_ + ub_), 0.5 * (va_ + vb_));
                let off = (patch.eval([&ku, &kv], um_, vm_) - eval(um_, vm_)).length();
                assert!(off < 1e-9, "{}: patch evaluation ({off})", c.name);
                total = total.saturating_add(n_u.saturating_mul(n_v));
                // the patch's evaluation error (the span's basis functions by A2.2, a weighted
                // sum of (p + 1) (q + 1) terms and a division: within gamma(4 (p + q) + 32)
                // of `|P|_inf w_max / w_min` per coordinate) and its derivative bounds widened by
                // their rounding (a few dozen operations each)
                let (mut w_min, mut w_max, mut big) = (f64::INFINITY, 0.0_f64, 0.0_f64);
                for h in &patch.hom {
                    w_min = w_min.min(h[3]);
                    w_max = w_max.max(h[3]);
                    big = big
                        .max((h[0] / h[3]).abs())
                        .max((h[1] / h[3]).abs())
                        .max((h[2] / h[3]).abs());
                }
                let eps_s = gamma(4 * (pu + pv) + 32) * 4.0 * big * (w_max / w_min);
                let up = 1.0 + gamma(64);
                work.push(Work {
                    patch,
                    u: (ua_, ub_, n_u),
                    v: (va_, vb_, n_v),
                    derivs: [pb.su * up, pb.suu * up, pb.sv * up, pb.svv * up],
                    eps_s,
                });
            }
        }
        eprintln!(
            "CELLS {}: {} patches, {total} cells, budget {budget:.2e} each way (estimate {estimate:.3e})",
            c.name,
            work.len()
        );
        assert!(
            total <= CELL_CAP,
            "{}: uncertified: budget exceeded ({total} cells > {CELL_CAP})",
            c.name
        );

        // every node's distance, the patches shared among 8 threads
        let next = AtomicUsize::new(0);
        let done = AtomicUsize::new(0);
        let running = AtomicU64::new(0); // the certified bound so far (non-negative f64 bits)
        let (knots_u, knots_v) = (&ku[..], &kv[..]);
        let results: Vec<(f64, f64, usize, usize, f64)> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        let (mut node_max, mut certified, mut nodes, mut cells) =
                            (0.0_f64, 0.0_f64, 0usize, 0usize);
                        let mut front_max = f64::NEG_INFINITY;
                        loop {
                            let i = next.fetch_add(1, Ordering::Relaxed);
                            let Some(wk) = work.get(i) else { break };
                            let ((ua_, ub_, n_u), (va_, vb_, n_v)) = (wk.u, wk.v);
                            let (hu, hv) = ((ub_ - ua_) / n_u as f64, (vb_ - va_) / n_v as f64);
                            let [su, suu, sv, svv] = wk.derivs;
                            // a cell's reach from its computed node: `|S(u, v) - S(u0, v0)| <=
                            // |S_u| h_u + |S_v| h_v`, plus the node's evaluation error `eps_s`
                            // (the computed node is within `eps_s` of `S(u0, v0)`)
                            let d = (su * hu + sv * hv) * (1.0 + gamma(8)) + wk.eps_s;
                            // `|phi|` at a node, the evaluation's rounding added (`phi` is
                            // 1-Lipschitz); each node is the first corner of its cell, so its
                            // certificate covers every point within `d`
                            // (and on which side of its ball the point lies: past either foot when
                            // `max(u.na, u.nb) > 0`, `u` from the nearest ball's centre)
                            let node = |u: f64, v: f64| -> (f64, f64) {
                                let p = wk.patch.eval([knots_u, knots_v], u, v);
                                let n = node_certificate(&frame, &r, p, range, &lb, true, Some(d))
                                    .unwrap_or_else(|e| panic!("{}: node ({u}, {v}): {e}", c.name));
                                let (rt, _) = r(n.t0);
                                let centre = frame.origin + frame.e * n.t0 + (frame.na + frame.nb) * rt;
                                let dir = (p - centre) * (1.0 / (p - centre).length());
                                (n.lo.abs().max(n.hi.abs()) + wk.eps_s, dir.dot(frame.na).max(dir.dot(frame.nb)))
                            };
                            let mut prev = vec![0.0_f64; n_v + 1];
                            let mut cur = vec![0.0_f64; n_v + 1];
                            let mut patch_max = 0.0_f64;
                            for a in 0..=n_u {
                                let u = if a == n_u { ub_ } else { ua_ + hu * a as f64 };
                                for (b, slot) in cur.iter_mut().enumerate() {
                                    let v = if b == n_v { vb_ } else { va_ + hv * b as f64 };
                                    let (value, side) = node(u, v);
                                    *slot = value;
                                    node_max = node_max.max(value);
                                    front_max = front_max.max(side);
                                }
                                nodes += n_v + 1;
                                if a > 0 {
                                    for b in 0..n_v {
                                        let corners = [prev[b], prev[b + 1], cur[b], cur[b + 1]];
                                        let corner = corners.iter().copied().fold(0.0_f64, f64::max);
                                        // a-priori: `|phi| <= |phi(corner)| + d` on the cell
                                        let big_phi = corners.iter().copied().fold(f64::INFINITY, f64::min) + d;
                                        let kc = k_of(big_phi).unwrap_or_else(|| {
                                            panic!("{}: uncertified: no Hessian bound at |phi| <= {big_phi}", c.name)
                                        });
                                        let rem = (hu * hu / 8.0 * (suu + kc * su * su)
                                            + hv * hv / 8.0 * (svv + kc * sv * sv))
                                            * (1.0 + gamma(16));
                                        patch_max = patch_max.max((corner + rem) * (1.0 + gamma(4)));
                                    }
                                    cells += n_v;
                                }
                                std::mem::swap(&mut prev, &mut cur);
                            }
                            certified = certified.max(patch_max);
                            running.fetch_max(patch_max.to_bits(), Ordering::Relaxed);
                            done.fetch_add(1, Ordering::Relaxed);
                        }
                        (node_max, certified, nodes, cells, front_max)
                    })
                })
                .collect();
            // progress at least every 30 s, so a slow run is told from a stuck one
            let start = std::time::Instant::now();
            let mut last = start;
            while done.load(Ordering::Relaxed) < work.len() {
                std::thread::sleep(std::time::Duration::from_millis(500));
                if last.elapsed().as_secs() >= 30 {
                    last = std::time::Instant::now();
                    eprintln!(
                        "PROGRESS {}: {}/{} patches, running certified max {:.3e}, {} s",
                        c.name,
                        done.load(Ordering::Relaxed),
                        work.len(),
                        f64::from_bits(running.load(Ordering::Relaxed)),
                        start.elapsed().as_secs()
                    );
                }
            }
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let node_max = results.iter().map(|x| x.0).fold(0.0_f64, f64::max);
        let certified = results.iter().map(|x| x.1).fold(0.0_f64, f64::max);
        let nodes: usize = results.iter().map(|x| x.2).sum();
        let cells: usize = results.iter().map(|x| x.3).sum();
        let front_max = results
            .iter()
            .map(|x| x.4)
            .fold(f64::NEG_INFINITY, f64::max);
        assert_eq!(cells, total, "{}: every cell evaluated", c.name);

        // the reversed law, on a coarse grid
        let reversed = row.law(true);
        let mut reversed_max = 0.0_f64;
        for a in 0..=60 {
            for b in 0..=12 {
                let p = eval(
                    u0 + (u1 - u0) * f64::from(a) / 60.0,
                    v0 + (v1 - v0) * f64::from(b) / 12.0,
                );
                let n = node_certificate(&frame, &reversed, p, range, &lb, false, None)
                    .unwrap_or_else(|e| panic!("{}: reversed: {e}", c.name));
                reversed_max = reversed_max.max(n.lo.abs().max(n.hi.abs()));
            }
        }

        // The other way, true round to body (measured, not certified): each point of the
        // touch arcs between the feet inside the box is within T of the body's boundary, the
        // round face or a plane face. A plane face holds a point's projection when the
        // projection is beyond the round face's rail on that plane. The distance to the
        // round face is through the kernel's closest point on its surface, which never
        // reports less than the true distance.
        let on_plane = |v: f64, n: Vec3| (eval(um, v) - row.origin).dot(n).abs();
        let v_on = |n: Vec3| {
            if on_plane(v0, n) < on_plane(v1, n) {
                v0
            } else {
                v1
            }
        };
        // the rail on the plane of `n`: its distance from the other plane at `along`
        let rail_across = |n: Vec3, other: Vec3, along: f64| {
            let v = v_on(n);
            let at_u = |u: f64| (eval(u, v) - row.origin).dot(row.e);
            let rising = at_u(u1) > at_u(u0);
            let (mut lo, mut hi) = (u0, u1);
            for _ in 0..80 {
                let mid = 0.5 * (lo + hi);
                if (at_u(mid) < along) == rising {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            (eval(lo, v) - row.origin).dot(other)
        };
        let planes = [(row.na, row.nb), (row.nb, row.na)];
        let mut back = 0.0_f64;
        for a in 0..=400 {
            let s = -0.1 * row.len + 1.2 * row.len * f64::from(a) / 400.0;
            for b in 0..=40 {
                let (p, _) = touch_arc(
                    &r,
                    row.origin,
                    row.e,
                    row.na,
                    row.nb,
                    s,
                    f64::from(b) / 40.0,
                );
                let along = (p - row.origin).dot(row.e);
                if !(0.0..=row.len).contains(&along) {
                    continue;
                }
                let q = Point3::new(p.x, p.y, p.z);
                let uv = surface_closest_point(k, &srf, q).unwrap().uvs[0];
                let mut d = srf.evaluate_point(k, uv).unwrap().distance(q);
                for (n, other) in planes {
                    if (p - row.origin).dot(other) >= rail_across(n, other, along) {
                        d = d.min((p - row.origin).dot(n).abs());
                    }
                }
                back = back.max(d);
            }
        }
        // The rails against the law's contact lines: the foot on the plane of `n` is `r(s)`
        // from the other plane, so the rail's offset is `g(u) = (R(u) - o).other - r(a(u))`,
        // `a(u) = (R(u) - o).e`, along the face's boundary iso-curve `R` on that plane (the
        // net's first or last column). Measured at 401 positions, and certified by branch
        // and bound per u-span: `|g| <= max(|g(a)|, |g(b)|) + m h^2 / 8` on `[a, b]`, with
        // `|g''| <= |R''| (1 + max|r'|) + max|r''| |R'|^2` from the span's control-net bounds.
        let mut slip = 0.0_f64;
        for a in 0..=2000 {
            let s = row.len * f64::from(a) / 2000.0;
            for (n, other) in planes {
                slip = slip.max((rail_across(n, other, s) - r(s).0).abs());
            }
        }
        assert!(
            (v0 - kv[0]).abs() < 1e-12 && (v1 - kv[kv.len() - 1]).abs() < 1e-12,
            "{}: the face's v runs over the net's",
            c.name
        );
        let mut w_cert = 0.0_f64;
        let mut rail_evaluations = 0usize;
        for (n, other) in planes {
            let column = if on_plane(v0, n) < on_plane(v1, n) {
                0
            } else {
                nv - 1
            };
            for i in pu..nu {
                let (lo_k, hi_k) = (ku[i], ku[i + 1]);
                if !(hi_k > lo_k && hi_k > u0 && lo_k < u1) {
                    continue;
                }
                let (lo, hi) = (lo_k.max(u0), hi_k.min(u1));
                let hom: Vec<[f64; 4]> = (i - pu..=i).map(|m| net[m * nv + column]).collect();
                let at = |u: f64| {
                    let basis = Patch::basis(&ku, i, pu, u);
                    let mut acc = [0.0_f64; 4];
                    for (nb, h) in basis.iter().zip(&hom) {
                        for (x, y) in acc.iter_mut().zip(h) {
                            *x += nb * y;
                        }
                    }
                    Vec3::new(acc[0] / acc[3], acc[1] / acc[3], acc[2] / acc[3])
                };
                let mid = 0.5 * (lo + hi);
                let off = (at(mid) - eval(mid, v_on(n))).length();
                assert!(off < 1e-9, "{}: rail evaluation ({off})", c.name);
                // the span's first and second derivative bounds, as in `patch_bounds`
                let pt = |h: [f64; 4]| Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]);
                let reference = pt(hom[0]);
                let tr: Vec<[f64; 4]> = hom
                    .iter()
                    .map(|h| {
                        let x = pt(*h) - reference;
                        [x.x * h[3], x.y * h[3], x.z * h[3], h[3]]
                    })
                    .collect();
                let w_min = hom.iter().map(|h| h[3]).fold(f64::INFINITY, f64::min);
                assert!(w_min > 0.0, "a non-positive weight");
                let far = hom
                    .iter()
                    .map(|h| (pt(*h) - reference).length())
                    .fold(0.0_f64, f64::max);
                let first: Vec<[f64; 4]> = (0..pu)
                    .map(|k| {
                        let m = i - pu + k;
                        let den = ku[m + pu + 1] - ku[m + 1];
                        let f = if den > 0.0 { pu as f64 / den } else { 0.0 };
                        [0, 1, 2, 3].map(|q| (tr[k + 1][q] - tr[k][q]) * f)
                    })
                    .collect();
                let second: Vec<[f64; 4]> = (0..pu.saturating_sub(1))
                    .map(|k| {
                        let m = i - pu + k;
                        let den = ku[m + pu + 1] - ku[m + 2];
                        let f = if den > 0.0 {
                            (pu - 1) as f64 / den
                        } else {
                            0.0
                        };
                        [0, 1, 2, 3].map(|q| (first[k + 1][q] - first[k][q]) * f)
                    })
                    .collect();
                let n3 = |d: &[f64; 4]| (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                let a1 = first.iter().map(n3).fold(0.0_f64, f64::max);
                let w1 = first.iter().map(|d| d[3].abs()).fold(0.0_f64, f64::max);
                let a2 = second.iter().map(n3).fold(0.0_f64, f64::max);
                let w2 = second.iter().map(|d| d[3].abs()).fold(0.0_f64, f64::max);
                let s1 = (a1 + far * w1) / w_min;
                let s2 = (a2 + 2.0 * s1 * w1 + far * w2) / w_min;
                let m = s2 * (1.0 + r1) + r2 * s1 * s1;
                let g = |u: f64| {
                    let q = at(u) - row.origin;
                    q.dot(other) - r(q.dot(row.e)).0
                };
                // the rounding of a computed `g`: the rail's evaluation (as a patch's) and the
                // law's ([`LawBounds::psi_err`] covers it)
                let (mut cw_min, mut cw_max, mut cbig) = (f64::INFINITY, 0.0_f64, 0.0_f64);
                for h in &hom {
                    cw_min = cw_min.min(h[3]);
                    cw_max = cw_max.max(h[3]);
                    cbig = cbig
                        .max((h[0] / h[3]).abs())
                        .max((h[1] / h[3]).abs())
                        .max((h[2] / h[3]).abs());
                }
                let err_g = gamma(4 * pu + 32) * 4.0 * cbig * (cw_max / cw_min) * (1.0 + r1)
                    + lb.psi_err(at(mid));
                let samples: Vec<(f64, f64)> = (0..=16)
                    .map(|k| {
                        let u = if k == 16 {
                            hi
                        } else {
                            lo + (hi - lo) * f64::from(k) / 16.0
                        };
                        (u, g(u))
                    })
                    .collect();
                let mut best = samples.iter().map(|x| x.1.abs()).fold(0.0_f64, f64::max);
                let mut stack: Vec<((f64, f64), (f64, f64))> =
                    samples.windows(2).map(|x| (x[0], x[1])).collect();
                while let Some(((ua_, ga), (ub_, gb))) = stack.pop() {
                    let h = ub_ - ua_;
                    if ga.abs().max(gb.abs()) + m * h * h / 8.0 * (1.0 + gamma(8)) + err_g
                        <= best + W_EPS
                    {
                        continue;
                    }
                    let um_ = 0.5 * (ua_ + ub_);
                    let gm = g(um_);
                    rail_evaluations += 1;
                    assert!(
                        rail_evaluations < 20_000_000,
                        "{}: uncertified: the rail search did not close",
                        c.name
                    );
                    best = best.max(gm.abs());
                    stack.push(((ua_, ga), (um_, gm)));
                    stack.push(((um_, gm), (ub_, gb)));
                }
                w_cert = w_cert.max(best + W_EPS + err_g);
            }
        }
        // The true round within T of the body (G3): ARGUED, not certified. The steps below are
        // prose, and they rest on premises this test does not certify: P1, each skin point's
        // nearest ball direction lies on the front arc between the feet (asserted only at the
        // nodes, below); P2, near its rails the round face is a graph over the plane (not
        // checked); P3, near the box's end planes the end faces close the body (only the
        // sampled `back` column). The second step also takes the arc's crossing with the
        // projected rail as the nearest point of the body, which is not so in general. The
        // code computes the bound the argument gives. `eps` is the certified bound above;
        // every skin point is within it of the envelope.
        // - The rails lie on their planes within `h_rail`: the rail is the net's first or last
        //   column, a rational curve with positive weights inside its control points' hull.
        // - A point of the true round near plane a whose projection onto the plane is beyond
        //   the rail is above the plane face by at most the height of the arc's crossing `Q`
        //   with the projected rail: on a ball tangent to the plane at its foot `F`, the height
        //   is `|X - F|^2 / (2 r)`, which grows along the arc from `F`, and `Q` is within `eps`
        //   of a rail point, so its height is at most `h_rail + eps`.
        // - Otherwise the skin lies over the point's projection: the vertical through it meets
        //   the skin at `S*`, and along that vertical `phi` changes at least as fast as
        //   `cos = (r_lo - h) / (r_lo + eps)`, `h` the greatest height on the segment (the
        //   gradient of `phi` is the unit vector from the nearest ball's centre). So
        //   `|S* - P| <= eps / cos`, solved with `h = h_rail + eps + |S* - P|`.
        // - Elsewhere every point of the true round is the projection of a skin point, at its
        //   distance `|phi| <= eps`.
        let mut h_rail = 0.0_f64;
        for (n, _) in planes {
            let column = if on_plane(v0, n) < on_plane(v1, n) {
                0
            } else {
                nv - 1
            };
            for a in 0..nu {
                let h = net[a * nv + column];
                let x = Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]);
                h_rail = h_rail.max((x - row.origin).dot(n).abs() + lb.psi_err(x));
            }
        }
        let eps = certified;
        let mut slide = 2.0 * eps;
        for _ in 0..4 {
            let height = h_rail + eps + slide;
            let cos = (r_lo - height) / (r_lo + eps);
            // (from `r_lo` and the heights only; it does not check P2)
            assert!(cos > 0.5, "{}: the vertical's slope bound {cos}", c.name);
            let next = eps / cos * (1.0 + gamma(8));
            assert!(
                next <= slide,
                "{}: the vertical bound does not close",
                c.name
            );
            slide = next;
        }
        let g3 = eps.max(h_rail + eps).max(slide) * (1.0 + gamma(4));
        assert!(g3 < t, "{}: true round to body {g3}", c.name);

        eprintln!(
            "CERT {}: K {kk:.4}, {cells} cells ({nodes} nodes); nodes max {node_max:.3e}; certified max {certified:.3e} < T {t:.1e}; reversed law {reversed_max:.3e}; true round to body: argued given P1 to P3 {g3:.4e} (rails off their planes, certified, {h_rail:.1e}), measured {back:.3e}; rails off the contact lines: measured {slip:.3e}, certified {w_cert:.4e} ({rail_evaluations} evaluations); P1 at the nodes: max(u.na, u.nb) {front_max:.2e}",
            c.name
        );
        assert!(
            certified < t,
            "{}: certified {certified} not below T {t}",
            c.name
        );
        let symmetric = c.radii.iter().eq(c.radii.iter().rev())
            && (row.m0 + row.slope * row.len / 2.0 - 0.5).abs() < 1e-12;
        if !symmetric {
            assert!(
                reversed_max > 10.0 * t,
                "{}: the reversed law is {reversed_max} off",
                c.name
            );
        }
        assert!(back < t, "{}: true round to body {back}", c.name);
        // P1 at the nodes only: past a foot by an angle `theta` (`max(u.na, u.nb) = sin theta`)
        // the nearest point leaves the front arc; between the nodes this is not certified
        assert!(
            front_max <= 1e-3,
            "{}: P1: a node's nearest ball direction is past a foot ({front_max})",
            c.name
        );
    }
}
