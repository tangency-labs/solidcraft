//! Whether the two faces on either side of an edge meet tangentially all along it, decided
//! exactly from the record.
//!
//! Each use of the edge carries its pcurve `(u(t), v(t))` in its face's parameters
//! ([`Face::parameters`]: a plane, or the B-spline surface SMLib holds the face as), on the edge's
//! own parameter `t`, so the two uses' points at one `t` are one point of the edge. The face's
//! natural normal there is `S_u × S_v` at the pcurve's point. On a piece of `t` where the pcurve
//! stays within one span of its own knots and one knot cell of its surface, both are
//! polynomials: `S` of degree `(p, q)` in `(u, v)` (rational: its homogeneous form, of the same
//! degree), the pcurve of degree `r` in `t`. So `S_u × S_v`, scaled by a positive power of the
//! weights, is a polynomial in `t` of degree at most `(2p + 2q - 2) r` (rational: `(4p + 4q - 2) r`,
//! from `(A_u w - A w_u) × (A_v w - A w_v)`), and a plane's is constant. The cross product of the
//! two faces' normals is then a polynomial vector of degree at most the sum `D` of theirs, and a
//! polynomial of degree `D` that vanishes at `D + 1` points vanishes everywhere: the faces are
//! parallel all along the piece exactly when they are at `D + 1` of its points, its Chebyshev
//! points, and on one side (smooth, not folded back on each other) when their outward normals
//! point the same way there. No point is a guess at where the faces part: one where they do is
//! a witness of a crease (or a cusp, the faces folded back), and agreement at all of them is
//! a proof of tangency, within the angle the caller allows at each (between them, the cross
//! product is bounded by its largest value at them times the Lebesgue constant of `D + 1`
//! Chebyshev points, under `1 + (2/π) ln(D + 1)`).
//!
//! The pieces' ends are the pcurves' knots and the parameters where a pcurve crosses a knot line
//! of its surface (periodic copies included), each a root of `U(t) - k W(t)` on one Bézier span
//! of the pcurve, isolated by subdividing its Bernstein coefficients (no sign change: no root;
//! one: exactly one) and found to rounding by bisection.

use solidcraft_geom::Vec3;
use solidcraft_geom::brep::eval::SurfaceEval;
use solidcraft_geom::brep::{Brep, Curve2, Face, Surface};
use solidcraft_geom::bspline::{HPoint, segment};

use crate::{KernelError, Result};

/// Most points one edge's decision evaluates.
const MAX_POINTS: usize = 1_000_000;
/// Largest degree of a piece's cross product the decision takes.
const MAX_DEGREE: usize = 4096;
/// Subdivisions of one Bernstein polynomial while isolating its roots: its pieces are then
/// 2⁻⁴⁸ of a pcurve span, where a root left unisolated (a pcurve touching a knot line) ends a
/// piece there.
const MAX_DEPTH: u32 = 48;
/// Subdivisions one root isolation may make: a root that touches its knot line (a double root,
/// the only kind that keeps two sign changes as the pieces shrink) costs one a level.
const MAX_SUBDIVISIONS: usize = 4096;
/// Below this sine of the angle between `S_u` and `S_v` a face's normal is undefined at that
/// point (a pole): the normal, and so the cross product, is zero there.
const DEGENERATE: f64 = 1e-9;

/// One use of the edge: its face's surface, the sign turning its natural normal outward, its
/// pcurve and the degree of its normal along a piece.
struct Use<'a> {
    surface: SurfaceEval,
    sign: f64,
    pcurve: &'a Curve2,
    degree: usize,
    /// The knot lines of the surface in u and v (its distinct knots over its domain) and its
    /// periods: a polynomial cell's sides.
    lines: [Vec<f64>; 2],
    period: [Option<f64>; 2],
}

fn failed(msg: String) -> KernelError {
    KernelError::Failed(msg)
}

/// A B-spline's degree from its knot and control point counts.
fn degree_of(knots: usize, count: usize) -> Option<usize> {
    knots.checked_sub(count.checked_add(1)?)
}

/// The distinct knots of a knot vector over its domain `[knots[p], knots[n]]`.
fn lines(knots: &[f64], count: usize) -> Option<Vec<f64>> {
    let p = degree_of(knots.len(), count)?;
    let mut out: Vec<f64> = knots.get(p..=count)?.to_vec();
    out.dedup();
    Some(out)
}

impl<'a> Use<'a> {
    fn new(face: &Face, sign: f64, pcurve: &'a Curve2) -> Result<Use<'a>> {
        let surface = face.parameters();
        let r = match pcurve {
            Curve2::Spline { knots, points } => degree_of(knots.len(), points.len()),
            Curve2::RationalSpline { knots, homogeneous } => degree_of(knots.len(), homogeneous.len()),
        }
        .ok_or_else(|| failed("a pcurve's degree".into()))?;
        // The normal's degree in (u, v) and the knot lines.
        let (uv_degree, lines) = match surface {
            Surface::Plane { .. } | Surface::PlaneThrough { .. } => (0, [Vec::new(), Vec::new()]),
            Surface::Spline { u_knots, v_knots, points } => {
                let (nu, nv) = (points.len(), points.first().map_or(0, Vec::len));
                let (p, q) = (degree_of(u_knots.len(), nu), degree_of(v_knots.len(), nv));
                let (Some(p), Some(q), Some(lu), Some(lv)) = (p, q, lines(u_knots, nu), lines(v_knots, nv)) else {
                    return Err(failed("a spline face's degree".into()));
                };
                ((2 * p + 2 * q).saturating_sub(2), [lu, lv])
            }
            Surface::RationalSpline { u_knots, v_knots, homogeneous } => {
                let (nu, nv) = (homogeneous.len(), homogeneous.first().map_or(0, Vec::len));
                let (p, q) = (degree_of(u_knots.len(), nu), degree_of(v_knots.len(), nv));
                let (Some(p), Some(q), Some(lu), Some(lv)) = (p, q, lines(u_knots, nu), lines(v_knots, nv)) else {
                    return Err(failed("a spline face's degree".into()));
                };
                ((4 * p + 4 * q).saturating_sub(2), [lu, lv])
            }
            // SMLib gives every curved analytic face its NURBS form, which its pcurves are on.
            _ => return Err(crate::blend::unsupported("deciding tangency along an edge of a face that is no plane and has no NURBS form")),
        };
        let eval = SurfaceEval::new(surface).map_err(|e| failed(format!("a face's surface: {e}")))?;
        let period = eval.domain().period;
        Ok(Use { surface: eval, sign, pcurve, degree: uv_degree.saturating_mul(r), lines, period })
    }

    /// The outward unit normal at the pcurve's point at `t`; `Ok(None)` where it is undefined (a
    /// pole: a zero of the normal).
    fn normal(&self, t: f64) -> Result<Option<Vec3>> {
        let uv = self.pcurve.eval(t).ok_or_else(|| failed(format!("a pcurve at {t}")))?;
        let (_, su, sv) = self.surface.derivs(uv[0], uv[1]).ok_or_else(|| failed(format!("a face at ({}, {})", uv[0], uv[1])))?;
        let n = su.cross(sv);
        if !(n.len() > DEGENERATE * su.len() * sv.len()) {
            return Ok(None);
        }
        Ok(n.normalized().map(|n| n * self.sign))
    }

    /// The parameters in `(t0, t1)` where the normal stops being one polynomial: the pcurve's
    /// own knots, and where it crosses a knot line of the surface.
    fn breaks(&self, [t0, t1]: [f64; 2], out: &mut Vec<f64>) -> Result<()> {
        let (knots, ctrl): (&[f64], Vec<HPoint>) = match self.pcurve {
            Curve2::Spline { knots, points } => (knots, points.iter().map(|p| HPoint { x: p[0], y: p[1], z: 0.0, w: 1.0 }).collect()),
            Curve2::RationalSpline { knots, homogeneous } => {
                (knots, homogeneous.iter().map(|h| HPoint { x: h[0], y: h[1], z: 0.0, w: h[2] }).collect())
            }
        };
        let r = degree_of(knots.len(), ctrl.len()).ok_or_else(|| failed("a pcurve's degree".into()))?;
        let mut spans: Vec<f64> = vec![t0];
        spans.extend(knots.iter().copied().filter(|k| *k > t0 && *k < t1));
        spans.push(t1);
        spans.dedup();
        out.extend(spans.iter().copied());
        if self.lines.iter().all(Vec::is_empty) {
            return Ok(());
        }
        for w in spans.windows(2) {
            let &[a, b] = w else { continue };
            let (_, bez) = segment(r, knots, &ctrl, a, b).map_err(|e| failed(format!("a pcurve span: {e}")))?;
            if bez.len() != r + 1 {
                return Err(failed("a pcurve span is no Bézier piece".into()));
            }
            for d in 0..2 {
                let coord = |h: &HPoint| if d == 0 { h.x } else { h.y };
                // The span's range in this direction: its control points' hull (weights > 0).
                let along: Vec<f64> = bez.iter().map(|h| coord(h) / h.w).collect();
                let lo = along.iter().copied().fold(f64::INFINITY, f64::min);
                let hi = along.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                if !(lo.is_finite() && hi.is_finite()) {
                    return Err(failed("a pcurve's control points".into()));
                }
                for k in cell_sides(&self.lines[d], self.period[d], lo, hi)? {
                    // Coefficients within rounding of zero are zero: a pcurve running along
                    // the knot line (a seam, a boundary) crosses none there.
                    let scale = bez.iter().map(|h| coord(h).abs() + (k * h.w).abs()).fold(0.0, f64::max);
                    let floor = 64.0 * f64::EPSILON * scale;
                    let c: Vec<f64> = bez.iter().map(|h| coord(h) - k * h.w).map(|x| if x.abs() <= floor { 0.0 } else { x }).collect();
                    if c.iter().all(|x| *x == 0.0) {
                        continue;
                    }
                    let mut roots = Vec::new();
                    let mut work = MAX_SUBDIVISIONS;
                    bernstein_roots(&c, 0.0, 1.0, MAX_DEPTH, &mut work, &mut roots);
                    out.extend(roots.into_iter().map(|s| a + (b - a) * s));
                }
            }
        }
        Ok(())
    }
}

/// The knot lines `lines` (and, with a period, their copies a whole number of periods away) that
/// lie in `[lo, hi]`.
fn cell_sides(lines: &[f64], period: Option<f64>, lo: f64, hi: f64) -> Result<Vec<f64>> {
    let Some(p) = period.filter(|p| *p > 0.0) else {
        return Ok(lines.iter().copied().filter(|k| *k >= lo && *k <= hi).collect());
    };
    let (Some(&first), Some(&last)) = (lines.first(), lines.last()) else { return Ok(Vec::new()) };
    let (n0, n1) = (((lo - last) / p).floor(), ((hi - first) / p).ceil());
    if !(n1 - n0 <= 64.0) {
        return Err(failed("a pcurve running round its surface too many times".into()));
    }
    let mut out = Vec::new();
    let mut n = n0;
    while n <= n1 {
        out.extend(lines.iter().map(|k| k + n * p).filter(|k| *k >= lo && *k <= hi));
        n += 1.0;
    }
    Ok(out)
}

/// The value at `s` of the polynomial with Bernstein coefficients `c` on `[0, 1]` (de Casteljau).
fn bernstein_value(c: &[f64], s: f64) -> f64 {
    let mut b = c.to_vec();
    while b.len() > 1 {
        b = b.windows(2).map(|w| if let [x, y] = w { x * (1.0 - s) + y * s } else { 0.0 }).collect();
    }
    b.first().copied().unwrap_or(0.0)
}

/// The Bernstein coefficients of the polynomial `c` on `[0, ½]` and `[½, 1]`.
fn bernstein_halves(c: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let mut left = Vec::with_capacity(c.len());
    let mut right = Vec::with_capacity(c.len());
    let mut b = c.to_vec();
    while let (Some(&f), Some(&l)) = (b.first(), b.last()) {
        left.push(f);
        right.push(l);
        b = b.windows(2).map(|w| if let [x, y] = w { 0.5 * (x + y) } else { 0.0 }).collect();
    }
    right.reverse();
    (left, right)
}

/// The roots in `[lo, hi]` of the polynomial with Bernstein coefficients `c` there, mapped
/// into `[lo, hi]`'s place in `[0, 1]`: none where the coefficients keep one sign, exactly one
/// where they change sign once (found by bisection to rounding), otherwise those of each half.
fn bernstein_roots(c: &[f64], lo: f64, hi: f64, depth: u32, work: &mut usize, out: &mut Vec<f64>) {
    let (Some(&first), Some(&last)) = (c.first(), c.last()) else { return };
    if first == 0.0 {
        out.push(lo);
    }
    if last == 0.0 {
        out.push(hi);
    }
    let signs: Vec<bool> = c.iter().filter(|x| **x != 0.0).map(|x| *x > 0.0).collect();
    let changes = signs.windows(2).filter(|w| matches!(w, [x, y] if x != y)).count();
    if changes == 0 {
        return;
    }
    if changes == 1 && first != 0.0 && last != 0.0 {
        let (mut a, mut b) = (0.0, 1.0);
        for _ in 0..64 {
            let m = 0.5 * (a + b);
            if !(m > a && m < b) {
                break;
            }
            if (bernstein_value(c, m) > 0.0) == (first > 0.0) {
                a = m;
            } else {
                b = m;
            }
        }
        out.push(lo + (hi - lo) * 0.5 * (a + b));
        return;
    }
    let mid = 0.5 * (lo + hi);
    // Out of depth or work, the roots are within this piece: it ends a piece of the edge at
    // both its ends.
    if depth == 0 || *work == 0 {
        out.extend([lo, hi]);
        return;
    }
    *work -= 1;
    let (l, r) = bernstein_halves(c);
    bernstein_roots(&l, lo, mid, depth - 1, work, out);
    bernstein_roots(&r, mid, hi, depth - 1, work, out);
}

/// Calls `visit` with the outward unit normals of edge `e`'s two uses' faces at one point of the
/// edge after another, `sign(f)` turning face `f`'s natural normal outward: on each piece of the
/// edge where both normals are polynomials, at the `D + 1` Chebyshev points the module
/// documentation describes, skipping a point where either is undefined (a pole). `visit` returns
/// whether to go on. `Ok(None)` for an edge with other than two uses; else whether some point had
/// both normals.
fn normal_pairs(rec: &Brep, sign: impl Fn(usize) -> Option<f64>, e: usize, mut visit: impl FnMut(Vec3, Vec3) -> bool) -> Result<Option<bool>> {
    let edge = rec.edges.get(e).ok_or_else(|| failed(format!("edge {e}")))?;
    let [ua, ub] = edge.uses.as_slice() else { return Ok(None) };
    let use_of = |u: &solidcraft_geom::brep::EdgeUseRef| -> Result<Use<'_>> {
        let face = rec.faces.get(u.face as usize).ok_or_else(|| failed(format!("face {}", u.face)))?;
        let oriented = match face.loops.get(u.r#loop as usize) {
            Some(solidcraft_geom::brep::Loop::Edges { edges }) => edges.get(u.position as usize),
            _ => None,
        }
        .ok_or_else(|| failed(format!("a use of edge {e}")))?;
        let s = sign(u.face as usize).ok_or_else(|| failed(format!("face {} is in no shell", u.face)))?;
        Use::new(face, s, &oriented.pcurve)
    };
    let (a, b) = (use_of(ua)?, use_of(ub)?);
    let degree = a.degree.saturating_add(b.degree);
    if degree > MAX_DEGREE {
        return Err(failed(format!("deciding tangency along edge {e}: its faces' normals are of degree {degree}")));
    }
    let interval = edge.interval;
    let mut breaks = Vec::new();
    a.breaks(interval, &mut breaks)?;
    b.breaks(interval, &mut breaks)?;
    breaks.retain(|t| *t >= interval[0] && *t <= interval[1]);
    breaks.sort_by(f64::total_cmp);
    breaks.dedup();
    let nodes = degree + 1;
    if breaks.len().saturating_mul(nodes) > MAX_POINTS {
        return Err(failed(format!("deciding tangency along edge {e}: {} pieces of degree {degree}", breaks.len())));
    }
    let mut defined = false;
    for w in breaks.windows(2) {
        let &[t0, t1] = w else { continue };
        let (m, h) = (0.5 * (t0 + t1), 0.5 * (t1 - t0));
        if !(h > 0.0) {
            continue;
        }
        for j in 0..nodes {
            let t = m + h * ((2 * j + 1) as f64 * std::f64::consts::PI / (2 * nodes) as f64).cos();
            if let (Some(na), Some(nb)) = (a.normal(t)?, b.normal(t)?) {
                defined = true;
                if !visit(na, nb) {
                    return Ok(Some(true));
                }
            }
        }
    }
    Ok(Some(defined))
}

/// Whether edge `e`'s two uses' faces meet with outward normals within `max_angle` (radians) of
/// each other all along it, `sign(f)` turning face `f`'s natural normal outward. An edge with
/// other than two uses has no such pair: false. See the module documentation.
pub(crate) fn tangent_all_along(rec: &Brep, sign: impl Fn(usize) -> Option<f64>, e: usize, max_angle: f64) -> Result<bool> {
    let cos = max_angle.cos();
    let mut tangent = true;
    let Some(defined) = normal_pairs(rec, sign, e, |na, nb| {
        tangent = na.dot(nb) >= cos;
        tangent
    })?
    else {
        return Ok(false);
    };
    if !defined {
        return Err(failed(format!("edge {e}: its faces' normals are undefined all along it")));
    }
    Ok(tangent)
}

/// The least angle (radians, 0 to π) between the outward normals of edge `e`'s two uses' faces
/// at the points [`normal_pairs`] visits: the edge's least angle where it is the same all along
/// the edge (two planes, a plane and a cylinder along a generator), at most a little above it
/// elsewhere. `None` for an edge with other than two uses, or whose normals are undefined
/// everywhere.
pub(crate) fn least_angle_along(rec: &Brep, sign: impl Fn(usize) -> Option<f64>, e: usize) -> Result<Option<f64>> {
    let mut least = f64::INFINITY;
    let defined = normal_pairs(rec, sign, e, |na, nb| {
        least = least.min(na.cross(nb).len().atan2(na.dot(nb)));
        true
    })?;
    Ok(defined.filter(|d| *d).map(|_| least))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bernstein_roots_are_isolated_and_exact() {
        // (s - 0.25)(s - 0.75) = s² - s + 3/16 in Bernstein form on [0, 1]: 3/16, 3/16 - 1/2, 3/16.
        let c = [0.1875, 0.1875 - 0.5, 0.1875];
        let mut roots = Vec::new();
        let mut work = MAX_SUBDIVISIONS;
        bernstein_roots(&c, 0.0, 1.0, MAX_DEPTH, &mut work, &mut roots);
        roots.sort_by(f64::total_cmp);
        assert_eq!(roots.len(), 2, "{roots:?}");
        assert!((roots[0] - 0.25).abs() < 1e-15 && (roots[1] - 0.75).abs() < 1e-15, "{roots:?}");
        // No sign change: no root.
        let mut none = Vec::new();
        bernstein_roots(&[1.0, 0.5, 2.0], 0.0, 1.0, MAX_DEPTH, &mut work, &mut none);
        assert!(none.is_empty());
    }

    #[test]
    fn periodic_knot_lines_repeat_by_whole_periods() {
        let sides = cell_sides(&[0.0, 1.0, 2.0], Some(2.0), 1.5, 4.5).unwrap();
        assert_eq!(sides, vec![2.0, 2.0, 3.0, 4.0, 4.0]);
    }
}
