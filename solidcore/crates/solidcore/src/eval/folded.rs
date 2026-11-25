//! Pcurves of B-spline edges folded across a cylinder or cone, which no NURBS is in the
//! surface's (u, v): a flat-pattern edge folded across a sheet metal bend, a helix. Each is
//! degree-7 Hermite pieces on the edge's own parameter, from the edge's exact derivatives
//! ([`folded_pcurve`]); nothing is fitted.
//!
//! Origin: SolidCraft's `crates/kernel/src/sew.rs` (MIT OR Apache-2.0), moved here. It takes the
//! kernel's evaluation types; the record's edges and faces are read by `solidcore-ops`'
//! `pcurve::folded_pcurve`, which converts them.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing tests

use super::plan::basis::{basis_derivatives, binomial, Side};
use super::vec::{v3, VecExt};
use super::{Curve, Curve2, CurveEval, Surface, SurfaceEval};
use crate::math::Vec3;
use crate::nurbs::knots::joints;
use std::f64::consts::{FRAC_PI_2, FRAC_PI_8, TAU};

/// Most Hermite pieces of a folded pcurve.
pub const FOLD_PIECES: usize = 1024;
/// How close, against the edge's size, a folded pcurve lifts onto its edge: as close as the
/// folded edge's own chain is to the fold (SolidCraft's sheet metal `BEND_CHAIN_TOL`).
pub const FOLD_TOL: f64 = 1e-7;

/// The point and first three derivatives at `t` of a B-spline or NURBS curve (homogeneous
/// `[x·w, y·w, z·w, w]` points; weight 1 for a B-spline), from `side` of `t` (at a joint the two
/// differ; elsewhere they agree): the homogeneous derivatives `A⁽ᵏ⁾`, `w⁽ᵏ⁾`, then
/// `C⁽ᵏ⁾ = (A⁽ᵏ⁾ − Σⱼ₌₁ᵏ C(k, j) w⁽ʲ⁾ C⁽ᵏ⁻ʲ⁾) / w`.
fn curve_derivs(knots: &[f64], points: &[[f64; 4]], t: f64, side: Side) -> Option<[Vec3; 4]> {
    let degree = knots.len().checked_sub(points.len() + 1)?;
    // From the left, the curve cut at `t` (the control points before it and their knots), whose
    // domain ends there: a B-spline's basis at the end of its domain is its limit from the left.
    let count = match side {
        Side::Left => knots.partition_point(|k| *k < t).min(points.len()),
        Side::Right => points.len(),
    };
    let (first, ders) = basis_derivatives(knots.get(..count + degree + 1)?, degree, count, t, 3)?;
    let (mut a, mut w) = ([Vec3::ZERO; 4], [0.0f64; 4]);
    for (i, d) in ders.iter().enumerate() {
        let &[x, y, z, h] = points.get(first + i)?;
        for (k, (ak, wk)) in a.iter_mut().zip(&mut w).enumerate() {
            let n = d.get(k).copied().unwrap_or(0.0);
            *ak += Vec3::new(x, y, z) * n;
            *wk += h * n;
        }
    }
    if !(w[0].abs() > 0.0) {
        return None;
    }
    let mut c = [Vec3::ZERO; 4];
    for k in 0..4 {
        let mut num = *a.get(k)?;
        for j in 1..=k {
            num -= *c.get(k - j)? * (binomial(k, j) * *w.get(j)?);
        }
        *c.get_mut(k)? = num * (1.0 / w[0]);
    }
    Some(c)
}

/// A complex number as `(re, im)`.
type Cx = (f64, f64);

/// `p q`.
fn cmul(p: Cx, q: Cx) -> Cx {
    (p.0 * q.0 - p.1 * q.1, p.0 * q.1 + p.1 * q.0)
}

/// `p / q`.
fn cdiv(p: Cx, q: Cx) -> Cx {
    let n = q.0 * q.0 + q.1 * q.1;
    ((p.0 * q.0 + p.1 * q.1) / n, (p.1 * q.0 - p.0 * q.1) / n)
}

/// The Bézier control points of the degree-7 Hermite piece on `[s0, s1]` with the values and
/// first three derivatives `p` at its start and `r` at its end (the k-th derivative of a Bézier of
/// degree 7 at an end is 7!/(7 − k)! times the k-th difference of the control points there,
/// scaled by `(s1 − s0)^k`).
fn hermite7(p: [[f64; 2]; 4], r: [[f64; 2]; 4], h: f64) -> [[f64; 2]; 8] {
    let (k1, k2, k3) = (h / 7.0, h * h / 42.0, h * h * h / 210.0);
    let at = |[[x0, y0], [x1, y1], [x2, y2], [x3, y3]]: [[f64; 2]; 4],
              [m0, m1, m2, m3]: [f64; 4]| {
        [
            x0 * m0 + x1 * m1 + x2 * m2 + x3 * m3,
            y0 * m0 + y1 * m1 + y2 * m2 + y3 * m3,
        ]
    };
    [
        p[0],
        at(p, [1.0, k1, 0.0, 0.0]),
        at(p, [1.0, 2.0 * k1, k2, 0.0]),
        at(p, [1.0, 3.0 * k1, 3.0 * k2, k3]),
        at(r, [1.0, -3.0 * k1, 3.0 * k2, -k3]),
        at(r, [1.0, -2.0 * k1, k2, 0.0]),
        at(r, [1.0, -k1, 0.0, 0.0]),
        r[0],
    ]
}

/// A Bézier curve's point at `f` in `[0, 1]` (de Casteljau, The NURBS Book §1.4); `None` where it
/// is not finite.
fn bezier_point(ctrl: &[[f64; 2]], f: f64) -> Option<[f64; 2]> {
    let mut pts = ctrl.to_vec();
    while pts.len() > 1 {
        pts = pts
            .windows(2)
            .map(|w| match *w {
                [[ax, ay], [bx, by]] => [ax * (1.0 - f) + bx * f, ay * (1.0 - f) + by * f],
                _ => [0.0; 2],
            })
            .collect();
    }
    pts.first()
        .copied()
        .filter(|p| p.iter().all(|c| c.is_finite()))
}

/// The pcurve on a cylinder or cone face (`label`) of a B-spline edge with no closed form there:
/// a flat-pattern edge folded across a bend, a helix, which no NURBS is (its 3D curve is already
/// degree-7 Hermite pieces of the fold). The same construction in (u, v): degree-7 Hermite pieces
/// on the edge's own parameter, matching the point and first three derivatives of its angle about
/// the axis (through the form's quarter parameter when `form` is given) and of its height along
/// the axis at each piece's ends. The pieces break where the angle crosses a quarter (the form's
/// parameter is only once differentiable there) and at the edge's own corners (a chain's
/// joints), and are halved until they lift onto the edge within [`FOLD_TOL`]. An approximation
/// by construction from exact derivatives, like the edge itself; nothing is fitted. `None` for
/// another face or curve, or past the piece budget ([`FOLD_PIECES`]).
#[must_use]
pub fn folded_pcurve(
    label: &Surface,
    form: Option<&Surface>,
    curve: &Curve,
    [t0, t1]: [f64; 2],
) -> Option<Curve2> {
    let (Surface::Cylinder { frame, .. } | Surface::Cone { frame, .. }) = label else {
        return None;
    };
    let (knots, points): (&[f64], Vec<[f64; 4]>) = match curve {
        Curve::Spline { knots, points } => (
            knots,
            points.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect(),
        ),
        Curve::RationalSpline { knots, homogeneous } => (knots, homogeneous.clone()),
        _ => return None,
    };
    let points = points.as_slice();
    let (o, x, y) = (
        v3(frame.origin),
        v3(frame.x_axis).normalized()?,
        v3(frame.y_axis).normalized()?,
    );
    let z = x.cross(y).normalized()?;
    let on = SurfaceEval::new(form.unwrap_or(label)).ok()?;
    let edge = CurveEval::new(curve).ok()?;
    let raw_angle = |t: f64| -> Option<f64> {
        let d = edge.point(t)? - o;
        Some(d.dot(y).atan2(d.dot(x)))
    };
    let angle_near = |t: f64, near: f64| raw_angle(t).map(|a| a + ((near - a) / TAU).round() * TAU);
    // The angle's branch at the start: inside the form's u range when there is one.
    let mut start = raw_angle(t0)?;
    if let Some(Surface::RationalSpline { u_knots, .. }) = form {
        let (lo, hi) = (*u_knots.first()?, *u_knots.last()?);
        let inside = |a: f64| (lo - 1e-9..=hi + 1e-9).contains(&super::form::param(a));
        start = (-2..=2)
            .map(|k| start + f64::from(k) * TAU)
            .find(|a| inside(*a))?;
    }
    // The angle along the edge, unwrapped from its start, on a grid of 32 points to each of the
    // edge's spans (a span of a fold turns through far less than a half turn).
    let spans = knots
        .windows(2)
        .filter(|w| matches!(**w, [a, b] if b > a && b > t0 && a < t1))
        .count();
    let grid = 32 * spans.max(1);
    let ts: Vec<f64> = (0..=grid)
        .map(|i| t0 + (t1 - t0) * i as f64 / grid as f64)
        .collect();
    let mut angles = Vec::new();
    let mut prev = start;
    for &t in &ts {
        prev = angle_near(t, prev)?;
        angles.push(prev);
    }
    // Breaks: the ends, and on a form each crossing of a quarter, found by bisection.
    let mut breaks = vec![(t0, *angles.first()?)];
    if form.is_some() {
        for i in 1..ts.len() {
            let (a0, a1) = (*angles.get(i - 1)?, *angles.get(i)?);
            let (q0, q1) = ((a0 / FRAC_PI_2).floor(), (a1 / FRAC_PI_2).floor());
            if q0 == q1 {
                continue;
            }
            let target = q0.max(q1) * FRAC_PI_2;
            let (mut lo, mut hi) = (*ts.get(i - 1)?, *ts.get(i)?);
            for _ in 0..80 {
                let mid = 0.5 * (lo + hi);
                if (angle_near(mid, a0)? - target).signum() == (a0 - target).signum() {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            breaks.push((0.5 * (lo + hi), target));
        }
    }
    // And at the edge's own corners, its joints (knots as many as its degree, where a chain's
    // pieces meet): no piece matches the edge's derivatives across one.
    let degree = knots.len().checked_sub(points.len() + 1)?;
    for k in joints(knots, degree, [t0, t1]) {
        let i = ts.partition_point(|t| *t <= k).saturating_sub(1);
        breaks.push((k, angle_near(k, *angles.get(i)?)?));
    }
    breaks.sort_by(|a, b| a.0.total_cmp(&b.0));
    breaks.push((t1, *angles.last()?));
    let size = points.iter().fold(1.0f64, |m, h| {
        [h[0], h[1], h[2]]
            .iter()
            .fold(m, |m, c| m.max((c / h[3]).abs()))
    });
    // A crossing within rounding of another break (or of an end), or nearer to it on the edge
    // than the pcurve lifts onto the edge (`FOLD_TOL`), is that break; the end never goes into
    // the start. An edge starting 3e-12 of its parameter short of a quarter had pieces 1.6e-12
    // long, whose knots SMLib took as one, past a multiplicity of the degree + 1.
    let close = 1e-12 * (t1 - t0).abs().max(1.0);
    let lifts = |a: f64, b: f64| matches!((edge.point(a), edge.point(b)), (Some(p), Some(q)) if p.dist(q) <= FOLD_TOL * size);
    let mut kept: Vec<(f64, f64)> = Vec::new();
    for (i, b) in breaks.iter().enumerate() {
        let is_end = i + 1 == breaks.len();
        let both_ends = is_end && kept.len() == 1;
        match kept.last_mut() {
            Some(last) if (b.0 - last.0).abs() <= close || (!both_ends && lifts(last.0, b.0)) => {
                if is_end {
                    *last = *b;
                }
            }
            _ => kept.push(*b),
        }
    }
    let breaks = kept;
    // u and v with their first three derivatives at `t` from `side`, angle `a`, in quarter `q` of
    // the form.
    let tq = FRAC_PI_8.tan();
    let uv = |t: f64, side: Side, a: f64, q: f64| -> Option<[[f64; 2]; 4]> {
        let c = curve_derivs(knots, points, t, side)?;
        let w = |d: Vec3| (d.dot(x), d.dot(y));
        let (w0, r1, r2, r3) = (w(c[0] - o), w(c[1]), w(c[2]), w(c[3]));
        let (r1, r2, r3) = (cdiv(r1, w0), cdiv(r2, w0), cdiv(r3, w0));
        // θ = Im log w: (log w)' = w'/w, (log w)'' = w''/w − (w'/w)², (log w)''' = w'''/w −
        // 3 w''w'/w² + 2 (w'/w)³.
        let a1 = r1.1;
        let a2 = r2.1 - cmul(r1, r1).1;
        let a3 = r3.1 - 3.0 * cmul(r2, r1).1 + 2.0 * cmul(cmul(r1, r1), r1).1;
        let [h0, h1, h2, h3] = [(c[0] - o).dot(z), c[1].dot(z), c[2].dot(z), c[3].dot(z)];
        let [u0, u1, u2, u3] = if form.is_some() {
            // The quarter parameter q + ½(1 + g/tan(π/8)), g = tan(ψ/2 − π/8), and its derivatives
            // in θ, through to t by the chain rule.
            let g = (0.5 * (a - q * FRAC_PI_2) - FRAC_PI_8).tan();
            let g1 = 0.5 * (1.0 + g * g);
            let g2 = g * g1;
            let g3 = g1 * g1 + g * g2;
            let (f1, f2, f3) = (g1 / (2.0 * tq), g2 / (2.0 * tq), g3 / (2.0 * tq));
            [
                q + 0.5 * (1.0 + g / tq),
                f1 * a1,
                f2 * a1 * a1 + f1 * a2,
                f3 * a1 * a1 * a1 + 3.0 * f2 * a1 * a2 + f1 * a3,
            ]
        } else {
            [a, a1, a2, a3]
        };
        Some([[u0, h0], [u1, h1], [u2, h2], [u3, h3]])
    };
    let mut pieces = 1usize;
    while pieces * breaks.len() <= FOLD_PIECES {
        let mut knots2 = vec![t0; 8];
        let mut ctrl: Vec<[f64; 2]> = Vec::new();
        let mut worst = 0.0f64;
        for w in breaks.windows(2) {
            let (&(a, aa), &(e, ea)) = (w.first()?, w.get(1)?);
            let q = (0.5 * (aa + ea) / FRAC_PI_2).floor();
            let mut last = aa;
            for k in 0..pieces {
                let (s0, s1) = (
                    a + (e - a) * k as f64 / pieces as f64,
                    a + (e - a) * (k + 1) as f64 / pieces as f64,
                );
                let (b0, b1) = (
                    last,
                    if k + 1 == pieces {
                        ea
                    } else {
                        angle_near(s1, last)?
                    },
                );
                last = b1;
                // Its start read from the right and its end from the left: the edge on the piece's
                // own side of a joint there, where the edge turns.
                let piece = hermite7(
                    uv(s0, Side::Right, b0, q)?,
                    uv(s1, Side::Left, b1, q)?,
                    s1 - s0,
                );
                // Each piece after the first starts at the end of the one before.
                let skip = usize::from(!ctrl.is_empty());
                ctrl.extend_from_slice(piece.get(skip..)?);
                knots2.extend(std::iter::repeat_n(s1, 7));
                for j in 1..8 {
                    let f = f64::from(j) / 8.0;
                    let [u, v] = bezier_point(&piece, f)?;
                    worst = worst.max(on.point([u, v])?.dist(edge.point(s0 + (s1 - s0) * f)?));
                }
            }
        }
        knots2.push(t1);
        if worst <= FOLD_TOL * size {
            return Some(Curve2::Spline {
                knots: knots2,
                points: ctrl,
            });
        }
        pieces *= 2;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::Frame;
    use std::f64::consts::{FRAC_1_SQRT_2, SQRT_2};

    /// A chain turning a corner on the cylinder of radius 2 about z, degree 2 with its joint at
    /// the knot 1 (held twice): the quarter circle z = 0 from (2, 0, 0) to (0, 2, 0) (weights 1,
    /// w = √2/2, 1), then the ruling from there up to (0, 2, 3) at a constant speed.
    fn corner_on_a_cylinder() -> (Vec<f64>, Vec<[f64; 4]>) {
        let w = FRAC_1_SQRT_2;
        let knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 2.0];
        (
            knots,
            vec![
                [2.0, 0.0, 0.0, 1.0],
                [2.0 * w, 2.0 * w, 0.0, w],
                [0.0, 2.0, 0.0, 1.0],
                [0.0, 2.0, 1.5, 1.0],
                [0.0, 2.0, 3.0, 1.0],
            ],
        )
    }

    /// At a joint the edge turns: from the left it is the piece before, from the right the piece
    /// after, each exactly (to rounding), nothing read a little way off the joint.
    #[test]
    fn a_chain_is_read_from_each_side_of_its_joint() {
        let (knots, points) = corner_on_a_cylinder();
        let s2 = SQRT_2;
        // The quarter circle at its end t = 1, from the left. Its weight W = 1 there, W' = 2 − 2w
        // = 2 − √2, W'' = 4 − 4w = 2W'. C' = 2w (P₂ − P₁) = (−2√2, 0, 0): speed √2 r = 2√2 round
        // z. C'' = ((2√2 − 2) r, −2r, 0) = (4√2 − 4, −4, 0): |C'|²/r = 4 towards the axis, and the
        // speed falling. C''' = −3W'(C'' + 2C') = 12(2 − √2)(1, 1, 0) = (7.0294, 7.0294, 0). Read
        // 1e-12 short of the joint instead, C' is off by 1e-12 C'' (4e-12 in y): past 1e-13.
        let third = Vec3::new(1.0, 1.0, 0.0) * (12.0 * (2.0 - s2));
        let want = [
            Vec3::new(0.0, 2.0, 0.0),
            Vec3::new(-2.0 * s2, 0.0, 0.0),
            Vec3::new(4.0 * s2 - 4.0, -4.0, 0.0),
            third,
        ];
        let left = curve_derivs(&knots, &points, 1.0, Side::Left).unwrap();
        for (k, (got, want)) in left.into_iter().zip(want).enumerate() {
            assert!(
                (got - want).len() < 1e-13,
                "from the left, derivative {k}: {got:?} vs {want:?}"
            );
        }
        // The ruling at its start, from the right: z = 3(t − 1), so C' = (0, 0, 3),
        // C'' = C''' = 0.
        let want = [
            Vec3::new(0.0, 2.0, 0.0),
            Vec3::new(0.0, 0.0, 3.0),
            Vec3::ZERO,
            Vec3::ZERO,
        ];
        let right = curve_derivs(&knots, &points, 1.0, Side::Right).unwrap();
        for (k, (got, want)) in right.into_iter().zip(want).enumerate() {
            assert!(
                (got - want).len() < 1e-13,
                "from the right, derivative {k}: {got:?} vs {want:?}"
            );
        }
    }

    /// The pcurve of that chain folded onto its cylinder (no form: u the angle, v the height)
    /// turns at the joint as the edge does, from each side.
    #[test]
    fn a_folded_pcurve_turns_with_its_edge_at_a_joint() {
        let (knots, homogeneous) = corner_on_a_cylinder();
        let frame = Frame {
            origin: [0.0; 3],
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
        };
        let cylinder = Surface::Cylinder { frame, radius: 2.0 };
        let edge = Curve::RationalSpline { knots, homogeneous };
        let Some(Curve2::Spline { knots, points }) =
            folded_pcurve(&cylinder, None, &edge, [0.0, 2.0])
        else {
            panic!("no folded pcurve")
        };
        // The pcurve from each side of t = 1, where its Hermite pieces meet (a knot it holds 7
        // times), its points as (u, v, 0) with weight 1.
        let h: Vec<[f64; 4]> = points.iter().map(|p| [p[0], p[1], 0.0, 1.0]).collect();
        let (left, right) = (
            curve_derivs(&knots, &h, 1.0, Side::Left).unwrap(),
            curve_derivs(&knots, &h, 1.0, Side::Right).unwrap(),
        );
        // The angle θ = arg(C − o) in the xy plane: θ' = Im(C'/w₀),
        // θ'' = Im(C''/w₀) − Im((C'/w₀)²), w₀ = 2i at the joint. The quarter circle's end:
        // C'/w₀ = −2√2/2i = √2 i, so θ' = √2; C''/w₀ = (4√2 − 4 − 4i)/2i = −2 − (2√2 − 2) i and
        // (√2 i)² = −2, so θ'' = 2 − 2√2; v' = v'' = 0 (z = 0). The ruling's start:
        // θ' = θ'' = 0, v' = 3, v'' = 0. Both at (π/2, 0).
        let s2 = SQRT_2;
        let corner = Vec3::new(FRAC_PI_2, 0.0, 0.0);
        // Each derivative comes back from control points holding it times the piece's length h
        // (the quarter circle lifts within FOLD_TOL on 4 pieces, h = 1/4), so a rounding of a
        // point (2e-16 of π/2) comes back times 7/h in the first and 42/h² in the second: within
        // 1e-12 and 1e-10.
        let quarter = [
            corner,
            Vec3::new(s2, 0.0, 0.0),
            Vec3::new(2.0 - 2.0 * s2, 0.0, 0.0),
        ];
        let ruling = [corner, Vec3::new(0.0, 3.0, 0.0), Vec3::ZERO];
        for (side, got, want) in [("left", left, quarter), ("right", right, ruling)] {
            for (k, (tol, want)) in [1e-12, 1e-12, 1e-10].into_iter().zip(want).enumerate() {
                assert!(
                    (got[k] - want).len() < tol,
                    "from the {side}, derivative {k}: {:?} vs {want:?}",
                    got[k]
                );
            }
        }
    }
}
