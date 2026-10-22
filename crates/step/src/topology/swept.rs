//! Swept, offset and open-conic geometry as exact schema 4 kinds. STEP's SURFACE_OF_REVOLUTION
//! becomes a `placed_revolution` (u the angle, v the profile's parameter, as STEP has them);
//! SURFACE_OF_LINEAR_EXTRUSION becomes a spline surface, the profile as an exact (rational)
//! B-spline times degree 1 along the extrusion vector (a circle extruded along its axis, and a
//! circle revolved in a plane through the axis, become the cylinder and ring torus they are, so
//! whole turns keep a periodic parameter); OFFSET_SURFACE of an analytic surface
//! becomes that surface moved along its normal; HYPERBOLA and PARABOLA become exact rational
//! quadratic and quadratic splines.
//!
//! Profiles are bounded where the face needs them: a line, a conic or an open conic over the
//! parameters of the face's boundary points (with a margin), a closed conic starting at the
//! face's seam or in the widest gap between them, so its seam lies outside the face. A
//! revolved profile stops where it meets the axis (beyond it the surface would fold over). The
//! surfaces' parameters may differ from STEP's (an extrusion's v, a conic's rational
//! parameter); pcurves and domains come from the records, never from STEP's parameters.

use std::f64::consts::{FRAC_PI_2, TAU};

use solidcraft_geom::Vec3;
use solidcraft_geom::brep::{Curve as BCurve, Surface as BSurface};

use super::{MAX_NESTING, R, frame, map_bspline_curve, unsupported, v3};
use crate::geom::{Axis1, Curve, P3, Placement, Surface};

/// Margin around a face's parameters on a profile, as a fraction of their span.
const MARGIN: f64 = 0.1;
/// Most sweep of one rational quadratic conic piece.
const CONIC_PIECE: f64 = FRAC_PI_2;
/// Most parameter step of one rational quadratic hyperbola piece (its weight `cosh(step / 2)`
/// stays near 1).
const HYPERBOLA_STEP: f64 = 1.0;
/// Most hyperbola pieces.
const MAX_PIECES: usize = 64;
/// Samples when looking for where a revolved profile meets its axis.
const AXIS_SAMPLES: usize = 128;

/// Points of a face's boundary (its edges sampled) and of its seam edges (used twice).
#[derive(Default)]
pub(super) struct Extent {
    pub points: Vec<Vec3>,
    pub seam: Vec<Vec3>,
}

fn p3(v: Vec3) -> P3 {
    [v.x, v.y, v.z]
}

/// The ellipse of `radii` in `p` from angle `t0` over `sweep` > 0 as rational quadratic pieces
/// whose knots are the pieces' end angles.
pub(super) fn conic_spline(p: &Placement, radii: [f64; 2], t0: f64, sweep: f64) -> BCurve {
    let n = ((sweep / CONIC_PIECE).ceil() as usize).clamp(1, 8);
    let dt = sweep / n as f64;
    let w = (0.5 * dt).cos();
    let at = |c: f64, s: f64| p.at(radii[0] * c, radii[1] * s, 0.0);
    let mut knots = vec![t0; 3];
    let first = at(t0.cos(), t0.sin());
    let mut homogeneous = vec![[first[0], first[1], first[2], 1.0]];
    for i in 0..n {
        let ta = t0 + dt * i as f64;
        let (tm, tb) = (ta + 0.5 * dt, ta + dt);
        // The meeting point of the end tangents, weighted cos of the half sweep.
        let m = at(tm.cos() / w, tm.sin() / w);
        homogeneous.push([m[0] * w, m[1] * w, m[2] * w, w]);
        let b = at(tb.cos(), tb.sin());
        homogeneous.push([b[0], b[1], b[2], 1.0]);
        let k = if i + 1 == n { t0 + sweep } else { tb };
        knots.extend([k, k]);
    }
    knots.push(t0 + sweep);
    BCurve::RationalSpline { knots, homogeneous }
}

/// The hyperbola branch `a cosh t X + b sinh t Y` from `t0` to `t1` (t0 < t1) as rational
/// quadratic pieces whose knots are the pieces' end parameters.
pub(super) fn hyperbola_spline(p: &Placement, a: f64, b: f64, t0: f64, t1: f64) -> BCurve {
    let n = (((t1 - t0) / HYPERBOLA_STEP).ceil() as usize).clamp(1, MAX_PIECES);
    let dt = (t1 - t0) / n as f64;
    let w = (0.5 * dt).cosh();
    let at = |c: f64, s: f64| p.at(a * c, b * s, 0.0);
    let mut knots = vec![t0; 3];
    let first = at(t0.cosh(), t0.sinh());
    let mut homogeneous = vec![[first[0], first[1], first[2], 1.0]];
    for i in 0..n {
        let ta = t0 + dt * i as f64;
        let (tm, tb) = (ta + 0.5 * dt, ta + dt);
        let m = at(tm.cosh() / w, tm.sinh() / w);
        homogeneous.push([m[0] * w, m[1] * w, m[2] * w, w]);
        let e = at(tb.cosh(), tb.sinh());
        homogeneous.push([e[0], e[1], e[2], 1.0]);
        let k = if i + 1 == n { t1 } else { tb };
        knots.extend([k, k]);
    }
    knots.push(t1);
    BCurve::RationalSpline { knots, homogeneous }
}

/// The parabola `f t² X + 2 f t Y` from `t0` to `t1` (t0 < t1): one quadratic Bézier on knots
/// `t0, t1`, so its parameter is STEP's t exactly.
pub(super) fn parabola_spline(p: &Placement, f: f64, t0: f64, t1: f64) -> BCurve {
    let at = |t: f64| p.at(f * t * t, 2.0 * f * t, 0.0);
    // The end tangents meet at f·(t0·t1, t0 + t1).
    let mid = p.at(f * t0 * t1, f * (t0 + t1), 0.0);
    BCurve::Spline { knots: vec![t0, t0, t0, t1, t1, t1], points: vec![at(t0), mid, at(t1)] }
}

/// The STEP line from `t0` to `t1` as a degree-1 spline on knots `t0, t1`: STEP's parameter.
fn line_spline(origin: P3, direction: P3, magnitude: f64, t0: f64, t1: f64) -> BCurve {
    let at = |t: f64| [0, 1, 2].map(|i| origin[i] + direction[i] * magnitude * t);
    BCurve::Spline { knots: vec![t0, t0, t1, t1], points: vec![at(t0), at(t1)] }
}

/// A polyline as a degree-1 spline: segment i on [i, i + 1], as STEP has it.
fn polyline_spline(points: &[P3]) -> BCurve {
    let n = points.len();
    let mut knots = vec![0.0];
    knots.extend((0..n).map(|i| i as f64));
    knots.push(n.saturating_sub(1) as f64);
    BCurve::Spline { knots, points: points.to_vec() }
}

/// A curve under its SURFACE_CURVE and TRIMMED_CURVE wrappers.
fn basis(c: &Curve) -> R<&Curve> {
    let mut c = c;
    for _ in 0..=MAX_NESTING {
        match c {
            Curve::SurfaceCurve { curve_3d, .. } => c = curve_3d,
            Curve::Trimmed { basis, .. } => c = basis,
            _ => return Ok(c),
        }
    }
    Err("profile curves nested too deeply".into())
}

/// The conic angle of a point in the conic's plane.
fn conic_angle(p: &Placement, radii: [f64; 2], q: Vec3) -> f64 {
    let d = q - v3(p.origin);
    (d.dot(v3(p.y())) / radii[1]).atan2(d.dot(v3(p.ref_direction)) / radii[0])
}

/// The STEP parameter of `q` (a point on, or projected into the plane of, a planar curve):
/// angle, hyperbola or parabola parameter, or line parameter.
fn planar_param(c: &Curve, q: Vec3) -> Option<f64> {
    let local = |p: &Placement| {
        let d = q - v3(p.origin);
        (d.dot(v3(p.ref_direction)), d.dot(v3(p.y())))
    };
    let t = match c {
        Curve::Line { origin, direction, magnitude } => (q - v3(*origin)).dot(v3(*direction)) / magnitude,
        Curve::Circle { position, radius } => conic_angle(position, [*radius; 2], q),
        Curve::Ellipse { position, semi_axis_1, semi_axis_2 } => conic_angle(position, [*semi_axis_1, *semi_axis_2], q),
        Curve::Hyperbola { position, semi_imag_axis, .. } => (local(position).1 / semi_imag_axis).asinh(),
        Curve::Parabola { position, focal_dist } => local(position).1 / (2.0 * focal_dist),
        _ => return None,
    };
    t.is_finite().then_some(t)
}

/// [min, max] of the parameters, or `None` without any.
fn range(ts: &[f64]) -> Option<(f64, f64)> {
    let (lo, hi) = ts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), t| (a.min(*t), b.max(*t)));
    (lo.is_finite() && hi.is_finite()).then_some((lo, hi))
}

/// Middle-free description of the widest gap between angles: (where it starts, its width).
fn widest_gap(angles: &[f64]) -> Option<(f64, f64)> {
    let mut a: Vec<f64> = angles.iter().map(|x| x.rem_euclid(TAU)).filter(|x| x.is_finite()).collect();
    a.sort_by(f64::total_cmp);
    let (first, last) = (*a.first()?, *a.last()?);
    let mut best = (last, first + TAU - last);
    for w in a.windows(2) {
        if let [x, y] = w
            && y - x > best.1
        {
            best = (*x, y - x);
        }
    }
    Some(best)
}

/// The angle all the given angles share (within 1e-6 rad), if there are any.
fn common_angle(angles: &[f64]) -> Option<f64> {
    let first = *angles.first()?;
    let off = |a: f64| ((a - first) + 0.5 * TAU).rem_euclid(TAU) - 0.5 * TAU;
    angles.iter().all(|a| off(*a).abs() < 1e-6).then_some(first)
}

/// A profile's parameter range: where the face is (`inner`) and the bounded range with its
/// margin (`outer`), or a whole closed conic from a start angle.
enum Span {
    Open { inner: (f64, f64), outer: (f64, f64) },
    Turn { start: f64 },
}

/// The range a profile is built over, from the parameters of the face's points on it and of its
/// seam points; `closed` for a circle or ellipse.
fn span(ts: &[f64], seam: &[f64], closed: bool) -> R<Span> {
    if closed {
        if let Some(s) = common_angle(seam) {
            return Ok(Span::Turn { start: s });
        }
        let (gap_start, gap) = widest_gap(ts).ok_or("a face without boundary points")?;
        let covered = TAU - gap;
        if gap < 1e-3 {
            return Ok(Span::Turn { start: gap_start + gap });
        }
        let start = gap_start + gap;
        let m = (MARGIN * covered + 1e-3).min(0.25 * gap);
        return Ok(Span::Open { inner: (start, start + covered), outer: (start - m, start + covered + m) });
    }
    let (lo, hi) = range(ts).ok_or("a face without boundary points")?;
    let m = MARGIN * (hi - lo) + 1e-3 * (1.0 + lo.abs().max(hi.abs()));
    Ok(Span::Open { inner: (lo, hi), outer: (lo - m, hi + m) })
}

/// The profile over its span as an exact spline (B-spline and polyline profiles whole).
fn bounded(c: &Curve, s: Span) -> R<BCurve> {
    let (lo, hi) = match s {
        Span::Open { outer, .. } => outer,
        Span::Turn { start } => (start, start + TAU),
    };
    if !(lo < hi && lo.is_finite() && hi.is_finite()) {
        return Err("a profile without extent".into());
    }
    Ok(match c {
        Curve::Line { origin, direction, magnitude } => line_spline(*origin, *direction, *magnitude, lo, hi),
        Curve::Circle { position, radius } => conic_spline(position, [*radius; 2], lo, hi - lo),
        Curve::Ellipse { position, semi_axis_1, semi_axis_2 } => conic_spline(position, [*semi_axis_1, *semi_axis_2], lo, hi - lo),
        Curve::Hyperbola { position, semi_axis, semi_imag_axis } => {
            if hi - lo > HYPERBOLA_STEP * MAX_PIECES as f64 {
                return Err(unsupported("a hyperbola profile that long"));
            }
            hyperbola_spline(position, *semi_axis, *semi_imag_axis, lo, hi)
        }
        Curve::Parabola { position, focal_dist } => parabola_spline(position, *focal_dist, lo, hi),
        _ => return Err(unsupported("this profile curve")),
    })
}

/// A profile that needs no bounds (a B-spline, a polyline), if it is one.
fn whole(c: &Curve) -> R<Option<BCurve>> {
    Ok(match c {
        Curve::BSpline(b) => Some(map_bspline_curve(b)?),
        Curve::Polyline(points) => {
            if points.len() < 2 || points.len() > super::MAX_SPLINE_POINTS || points.windows(2).any(|w| w.first() == w.get(1)) {
                return Err(unsupported(format!("a polyline profile of {} points", points.len())));
            }
            Some(polyline_spline(points))
        }
        Curve::Line { .. } | Curve::Circle { .. } | Curve::Ellipse { .. } | Curve::Hyperbola { .. } | Curve::Parabola { .. } => None,
        Curve::OffsetCurve3d { .. } => return Err(unsupported("an OFFSET_CURVE_3D profile")),
        Curve::Trimmed { .. } | Curve::SurfaceCurve { .. } => return Err(unsupported("nested profile curves")),
    })
}

/// The middle parameter of a TRIMMED_CURVE trimmed by parameters (on a closed conic, the
/// middle of the arc its sense runs over).
fn trimmed_middle(c: &Curve) -> Option<f64> {
    let Curve::Trimmed { basis, trim_1, trim_2, sense, .. } = c else { return None };
    let param = |t: &[crate::geom::Trim]| {
        t.iter().find_map(|x| match x {
            crate::geom::Trim::Parameter(p) => Some(*p),
            crate::geom::Trim::Point(_) => None,
        })
    };
    let (a, mut b) = (param(trim_1)?, param(trim_2)?);
    if is_closed(basis) {
        b = if *sense { a + (b - a).rem_euclid(TAU) } else { a - (a - b).rem_euclid(TAU) };
    }
    let m = 0.5 * (a + b);
    m.is_finite().then_some(m)
}

fn is_closed(c: &Curve) -> bool {
    matches!(c, Curve::Circle { .. } | Curve::Ellipse { .. })
}

/// SURFACE_OF_REVOLUTION: the profile turned about the axis, u the angle and v the profile's
/// parameter (`forward: false`), so the natural normal is ∂S/∂angle × ∂S/∂profile, STEP's.
fn revolution(profile: &Curve, axis: &Axis1, ext: &Extent) -> R<BSurface> {
    let c = basis(profile)?;
    let (o, k) = (v3(axis.origin), v3(axis.direction));
    let k = k.normalized().ok_or("a revolution about no direction")?;
    let curve = match whole(c)? {
        Some(curve) => curve,
        None => {
            // The meridian half-plane the profile lies in: the radial direction of its point
            // farthest from the axis among a few.
            let radial = |q: Vec3| {
                let d = q - o;
                d - k * d.dot(k)
            };
            let probe = |t: f64| c.point(t).map(v3);
            let mut e = None::<Vec3>;
            let mut best = 0.0;
            // A trimmed profile's own middle says which side of the axis it is on (a profile
            // across the axis covers the surface twice, once each way round).
            let probes = match trimmed_middle(profile) {
                Some(t) => vec![t],
                None => vec![-1.0, 0.0, 0.5, 1.0, 2.0, 3.0, 4.0, 5.0],
            };
            for t in probes {
                if let Some(r) = probe(t).map(radial)
                    && r.len() > best
                {
                    best = r.len();
                    e = r.normalized();
                }
            }
            let e = e.ok_or_else(|| unsupported("a profile on its axis"))?;
            // Each boundary point turned into the meridian plane, then its profile parameter:
            // on whichever side of the axis the profile has a point where it lands.
            let meridian = |q: Vec3| ((q - o).dot(k), radial(q).len());
            let param = |q: Vec3| {
                let (h, r) = meridian(q);
                [e, -e]
                    .into_iter()
                    .filter_map(|side| {
                        let at = o + k * h + side * r;
                        let t = planar_param(c, at)?;
                        Some((t, probe(t)?.dist(at)))
                    })
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(t, _)| t)
            };
            let ts: Vec<f64> = ext.points.iter().filter_map(|q| param(*q)).collect();
            let seam: Vec<f64> = ext.seam.iter().filter_map(|q| param(*q)).collect();
            // The profile's side of the axis where the face is.
            let mid = ts.get(ts.len() / 2).copied().ok_or("a face without boundary points")?;
            let e = probe(mid).map(radial).and_then(|r| r.normalized()).unwrap_or(e);
            let s = span(&ts, &seam, is_closed(c))?;
            // Stop where the profile crosses the axis outside the face.
            let side = |t: f64| probe(t).map(|q| (q - o).dot(e));
            let s = match s {
                Span::Open { inner, outer } => {
                    let lo = crossing(&side, inner.0, outer.0).unwrap_or(outer.0);
                    let hi = crossing(&side, inner.1, outer.1).unwrap_or(outer.1);
                    Span::Open { inner, outer: (lo, hi) }
                }
                turn => turn,
            };
            bounded(c, s)?
        }
    };
    let identity = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
    Ok(BSurface::PlacedRevolution { profile: Box::new(curve), origin: p3(o), axis: p3(k), matrix: identity, forward: false })
}

/// Going from `from` (where `side` is positive or zero) towards `to`, the first parameter
/// where `side` reaches zero, if it does.
fn crossing(side: &dyn Fn(f64) -> Option<f64>, from: f64, to: f64) -> Option<f64> {
    let s0 = side(from)?;
    let mut prev = (from, s0);
    for i in 1..=AXIS_SAMPLES {
        let t = from + (to - from) * i as f64 / AXIS_SAMPLES as f64;
        let s = side(t)?;
        if s.signum() != prev.1.signum() || s == 0.0 {
            let (mut a, mut b, fa) = (prev.0, t, prev.1);
            for _ in 0..100 {
                let m = 0.5 * (a + b);
                let fm = side(m)?;
                if fm.signum() == fa.signum() && fm != 0.0 {
                    a = m;
                } else {
                    b = m;
                }
            }
            return Some(b);
        }
        prev = (t, s);
    }
    None
}

/// SURFACE_OF_LINEAR_EXTRUSION: the profile as an exact spline along u, degree 1 along the
/// vector in v, over the face's reach along it.
fn extrusion(profile: &Curve, direction: P3, magnitude: f64, ext: &Extent) -> R<BSurface> {
    let c = basis(profile)?;
    let d = v3(direction) * magnitude;
    let dd = d.len2();
    if !(dd > 0.0 && dd.is_finite()) {
        return Err("an extrusion of no length".into());
    }
    let curve = match whole(c)? {
        Some(curve) => curve,
        None => {
            // Each boundary point slid along the vector onto the profile.
            let onto = |q: Vec3| -> Option<f64> {
                match c {
                    Curve::Line { origin, direction: e, magnitude: m } => {
                        // q − origin = s·m·e + v·d, least squares in (s, v).
                        let (e, r) = (v3(*e) * *m, q - v3(*origin));
                        let (a, b, cc) = (e.len2(), e.dot(d), dd);
                        let det = a * cc - b * b;
                        (det.abs() > 1e-12 * a * cc).then(|| (r.dot(e) * cc - r.dot(d) * b) / det)
                    }
                    Curve::Circle { position, .. }
                    | Curve::Ellipse { position, .. }
                    | Curve::Hyperbola { position, .. }
                    | Curve::Parabola { position, .. } => {
                        let n = v3(position.axis);
                        let dn = d.dot(n);
                        if !(dn.abs() > 1e-9 * d.len()) {
                            return None;
                        }
                        planar_param(c, q - d * ((q - v3(position.origin)).dot(n) / dn))
                    }
                    _ => None,
                }
            };
            let ts: Vec<f64> = ext.points.iter().filter_map(|q| onto(*q)).collect();
            if ts.len() < ext.points.len() {
                return Err(unsupported("an extrusion along its profile's plane"));
            }
            let seam: Vec<f64> = ext.seam.iter().filter_map(|q| onto(*q)).collect();
            bounded(c, span(&ts, &seam, is_closed(c))?)?
        }
    };
    let (knots, homogeneous, rational) = match curve {
        BCurve::Spline { knots, points } => (knots, points.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect::<Vec<_>>(), false),
        BCurve::RationalSpline { knots, homogeneous } => (knots, homogeneous, true),
        _ => return Err("an extrusion profile".into()),
    };
    // v: the face's points' reach along the vector, less the profile's (its control points
    // bound it), with a margin.
    let along = |p: Vec3| p.dot(d) / dd;
    let (plo, phi) = range(&ext.points.iter().map(|p| along(*p)).collect::<Vec<_>>()).ok_or("a face without boundary points")?;
    let cs: Vec<f64> = homogeneous.iter().filter(|h| h[3] > 0.0).map(|h| along(Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]))).collect();
    let (clo, chi) = range(&cs).ok_or("an extrusion profile")?;
    let (lo, hi) = (plo - chi, phi - clo);
    let m = MARGIN * (hi - lo) + 1e-3 / dd.sqrt();
    let (lo, hi) = (lo - m, hi + m);
    let shift = |h: &[f64; 4], s: f64| [h[0] + d.x * s * h[3], h[1] + d.y * s * h[3], h[2] + d.z * s * h[3], h[3]];
    let rows: Vec<Vec<[f64; 4]>> = homogeneous.iter().map(|h| vec![shift(h, lo), shift(h, hi)]).collect();
    let v_knots = vec![lo, lo, hi, hi];
    Ok(if rational {
        BSurface::RationalSpline { u_knots: knots, v_knots, homogeneous: rows }
    } else {
        BSurface::Spline { u_knots: knots, v_knots, points: rows.into_iter().map(|r| r.into_iter().map(|h| [h[0], h[1], h[2]]).collect()).collect() }
    })
}

/// An analytic surface moved `d` along its natural normal (every analytic kind's natural
/// normal points out of its axis, or along z × … for a plane): checked at each level.
fn offset(s: BSurface, d: f64) -> R<BSurface> {
    let bad = |what: &str| Err(format!("an offset {what}"));
    Ok(match s {
        BSurface::Plane { origin, u_axis, v_axis } => {
            let n = v3(u_axis).cross(v3(v_axis)).normalized().ok_or("a degenerate plane")?;
            BSurface::Plane { origin: p3(v3(origin) + n * d), u_axis, v_axis }
        }
        BSurface::Cylinder { frame, radius } => {
            if !(radius + d > 1e-9) {
                return bad("cylinder that collapses or turns inside out");
            }
            BSurface::Cylinder { frame, radius: radius + d }
        }
        BSurface::Sphere { frame, radius } => {
            if !(radius + d > 1e-9) {
                return bad("sphere that collapses or turns inside out");
            }
            BSurface::Sphere { frame, radius: radius + d }
        }
        BSurface::Cone { frame, radius_at_origin, half_angle } => {
            // The normal is (cos a, −sin a) in (radial, axial): the reference radius grows by
            // d / cos a, the angle stays.
            let r = radius_at_origin + d / half_angle.cos();
            if !(r >= 0.0 && r.is_finite()) {
                return bad("cone whose reference radius turns negative");
            }
            BSurface::Cone { frame, radius_at_origin: r, half_angle }
        }
        BSurface::Torus { frame, major_radius, minor_radius } => {
            let r = minor_radius + d;
            if !(r > 1e-9 && r < major_radius) {
                return bad("torus that collapses or meets its axis");
            }
            BSurface::Torus { frame, major_radius, minor_radius: r }
        }
        _ => return Err(unsupported("an OFFSET_SURFACE of a spline or swept surface")),
    })
}

/// A face's surface as a schema surface, and whether the record's natural normal is opposite
/// STEP's surface normal (an extruded circle or revolved circle read as a cylinder or torus,
/// whose natural normals point away from their axes); `ext` bounds swept surfaces' profiles,
/// and a cone that stays within `tol` (the file's distance uncertainty) of its limit plane
/// over `ext` is read as that plane.
pub(super) fn map_surface(s: &Surface, ext: &Extent, tol: f64) -> R<(BSurface, bool)> {
    let mut reversed = false;
    let s = map_surface_in(s, ext, tol, 0, &mut reversed)?;
    Ok((s, reversed))
}

/// An extruded circle along its axis is a cylinder; a circle in a plane through the axis,
/// revolved, a ring torus. `None` for anything else.
fn canonical(s: &Surface, reversed: &mut bool) -> Option<BSurface> {
    let c = match s {
        Surface::Extrusion { profile, .. } | Surface::Revolution { profile, .. } => basis(profile).ok()?,
        _ => return None,
    };
    let Curve::Circle { position, radius } = c else { return None };
    let n = v3(position.axis);
    match s {
        Surface::Extrusion { direction, .. } => {
            let d = v3(*direction);
            if d.cross(n).len() > 1e-12 {
                return None;
            }
            // STEP's normal is the profile's tangent × the vector: out of the axis when the
            // vector runs along the circle's axis.
            *reversed = d.dot(n) < 0.0;
            Some(BSurface::Cylinder { frame: frame(position), radius: *radius })
        }
        Surface::Revolution { axis, .. } => {
            let (o, k) = (v3(axis.origin), v3(axis.direction).normalized()?);
            let centre = v3(position.origin);
            let scale = 1.0 + (centre - o).len() + radius;
            if n.dot(k).abs() > 1e-12 || (centre - o).dot(n).abs() > 1e-12 * scale {
                return None;
            }
            let foot = o + k * (centre - o).dot(k);
            let big = (centre - foot).len();
            if !(big > *radius * (1.0 + 1e-9)) {
                return None;
            }
            let x = (centre - foot).normalized()?;
            // STEP's normal at the circle's angle 0: ∂angle × ∂profile; the torus's points out
            // of the tube.
            let (cx, cy) = (v3(position.ref_direction), v3(position.y()));
            let at = centre + cx * *radius;
            let step_normal = k.cross(at - o).cross(cy * *radius);
            *reversed = step_normal.dot(cx) < 0.0;
            Some(BSurface::Torus {
                frame: solidcraft_geom::brep::Frame { origin: p3(foot), x_axis: p3(x), y_axis: p3(k.cross(x)) },
                major_radius: big,
                minor_radius: *radius,
            })
        }
        _ => None,
    }
}

fn map_surface_in(s: &Surface, ext: &Extent, tol: f64, depth: usize, reversed: &mut bool) -> R<BSurface> {
    if depth > MAX_NESTING {
        return Err("surfaces nested too deeply".into());
    }
    if let Some(c) = canonical(s, reversed) {
        return Ok(c);
    }
    Ok(match s {
        Surface::Plane { position } => BSurface::Plane { origin: position.origin, u_axis: position.ref_direction, v_axis: position.y() },
        Surface::Cylinder { position, radius } => BSurface::Cylinder { frame: frame(position), radius: *radius },
        Surface::Cone { position, radius, semi_angle } => {
            if let Some(plane) = flat_cone(position, *radius, *semi_angle, ext, tol, reversed) {
                return Ok(plane);
            }
            if !(*radius >= 0.0) || !(semi_angle.abs() > 1e-12 && semi_angle.abs() < FRAC_PI_2) {
                return Err(unsupported(format!("a cone of radius {radius} and semi-angle {semi_angle}")));
            }
            BSurface::Cone { frame: frame(position), radius_at_origin: *radius, half_angle: *semi_angle }
        }
        Surface::Sphere { position, radius } => BSurface::Sphere { frame: frame(position), radius: *radius },
        Surface::Torus { position, major_radius, minor_radius } => {
            if !(major_radius > minor_radius) {
                return Err(unsupported("a spindle or horn torus"));
            }
            BSurface::Torus { frame: frame(position), major_radius: *major_radius, minor_radius: *minor_radius }
        }
        Surface::DegenerateTorus { .. } => return Err(unsupported("DEGENERATE_TOROIDAL_SURFACE")),
        Surface::BSpline(b) => super::map_bspline_surface(b)?,
        Surface::RectangularTrimmed { basis, .. } => map_surface_in(basis, ext, tol, depth + 1, reversed)?,
        Surface::Revolution { profile, axis } => revolution(profile, axis, ext)?,
        Surface::Extrusion { profile, direction, magnitude } => extrusion(profile, *direction, *magnitude, ext)?,
        Surface::Offset { basis, distance, .. } => {
            // Along STEP's normal: the record's, or its opposite.
            let b = map_surface_in(basis, ext, tol, depth + 1, reversed)?;
            offset(b, if *reversed { -*distance } else { *distance })?
        }
    })
}

/// A cone written for a plane (a semi-angle a whisker short of 90°, as exporters write one) as
/// that plane: the plane through the placement's origin across its axis, the cone's limit as
/// its angle opens to 90°. The cone's height above it is `(rho - radius) cot a` at parameter
/// radius `rho`, monotone in `rho` away from the apex: over the face it stays within the
/// heights of the boundary points and, should the face hold the axis, the apex's
/// (`radius cot a`). `None` unless that deviation is within `tol`. STEP's normal there is
/// `-rho tan a` along the axis, so the record's (the axis) is opposite STEP's where
/// `rho tan a > 0`.
fn flat_cone(position: &Placement, radius: f64, semi_angle: f64, ext: &Extent, tol: f64, reversed: &mut bool) -> Option<BSurface> {
    let (o, k) = (v3(position.origin), v3(position.axis).normalized()?);
    let cot = semi_angle.cos() / semi_angle.sin();
    let mut deviation = (radius * cot).abs();
    let mut widest: Option<(f64, f64)> = None;
    for p in &ext.points {
        let d = *p - o;
        let h = d.dot(k);
        deviation = deviation.max(h.abs());
        let r = (d - k * h).len();
        if widest.is_none_or(|(w, _)| r > w) {
            widest = Some((r, h));
        }
    }
    let (_, h) = widest?;
    if !(deviation <= tol) {
        return None;
    }
    // The parameter radius at the widest boundary point (it keeps one sign off the apex).
    *reversed ^= (radius + h * semi_angle.tan()) * semi_angle.tan() > 0.0;
    Some(BSurface::Plane { origin: position.origin, u_axis: position.ref_direction, v_axis: position.y() })
}

/// The STEP parameters of `ps` and `pe` on a hyperbola or parabola.
pub(super) fn open_conic_params(c: &Curve, ps: P3, pe: P3) -> R<(f64, f64)> {
    match (planar_param(c, v3(ps)), planar_param(c, v3(pe))) {
        (Some(a), Some(b)) => Ok((a, b)),
        _ => Err("no parameter on the conic".into()),
    }
}
