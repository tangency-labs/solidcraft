//! Which parameter direction a surface is collapsed along at a point: the one classifier of
//! poles ([`uv::degenerate`](super::uv::degenerate),
//! [`uv::degenerate_within`](super::uv::degenerate_within),
//! [`SurfaceEval::pole`](super::SurfaceEval::pole),
//! [`SurfaceEval::pole_within`](super::SurfaceEval::pole_within)).
//!
//! A surface is collapsed along `d` at `(u, v)` when its iso-line through the point along `d`
//! is one point: moving along `d` does not move the point, to any order. That is decided on the
//! iso-line itself, never from derivatives at the point (which vanish at an isolated singular
//! point too, and leave a pole of high order undecided), and on that iso-line alone:
//!
//! - A rational B-spline's iso-line along `v` at `u0` is the B-spline whose homogeneous control
//!   points `Q_j = (x_j, w_j)` are the columns' points at `u0` (de Boor along `u`). With every
//!   `w_j` non-zero, of whatever sign, it is one point `c` exactly when every `x_j / w_j = c`:
//!   then its numerator is `c` times its weight wherever that weight is not zero (where it is,
//!   the surface has no point), and conversely `C ≡ c` gives `Σ N_j (x_j − c w_j) = 0`, so
//!   `x_j = c w_j` by the basis's linear independence. This holds at any degree.
//! - Each `Q_j` is computed with a certified bound on its rounding from its own column's scale
//!   (Higham's [`gamma`] with [`UNIT`] on de Boor's convex combinations), and each Euclidean
//!   point's from its own `|w_j|`. A `w_j` not certified non-zero against its rounding is refused.
//! - A revolution's iso-line along its angle is the circle of the profile point's distance from
//!   the axis; along its profile it is the turned profile, decided on the profile's control
//!   points the same way. The affine map after the turn scales a movement by at most
//!   `sqrt(‖M‖₁ ‖M‖∞)`, 1 for a rotation. (A singular or strongly sheared map could collapse a
//!   direction the profile does not; only rigid and similarity maps are expected, and a
//!   singular one is refused.)
//! - The analytic forms ([`SurfaceEval`](super::SurfaceEval)) give their circles' radii in
//!   closed form.
//!
//! Within `reach` (a boundary point that lies on a pole to within a tolerance), when the
//! iso-line is not one point to rounding:
//! - with its weights of one sign (all negative is as good as all positive) it lies in its
//!   control points' hull, so it moves by at most their bounding box's diagonal: collapsed
//!   within reach when that, plus rounding, is within `reach`; a circle moves by its diameter;
//! - with weights of both signs it can leave the hull (or have no bound at all), so the hull is
//!   never used: two of its points farther apart than `reach` certify it is not collapsed within
//!   reach, and otherwise the question is refused.
//!
//! On the analytic forms ([`SurfaceEval::collapse`](super::SurfaceEval::collapse)) a direction is
//! collapsed within `reach` only where the surface narrows to a pole: where its circles along
//! that direction are somewhere wider than `reach`. A form nowhere wider (a cylinder, a sphere's
//! meridian, a torus's tube, or any of them no more than `reach` across) is thin, not collapsed,
//! and only an exact pole is one there. Splines and revolutions do not follow that rule yet:
//! their iso-lines are decided within `reach` as above, so a round written as a B-spline thinner
//! than `reach` still reads as collapsed where its analytic form does not.
//!
//! The iso-line is taken over the spline's whole knot range along `d`, or over `range` when
//! one is given ([`collapsed_on`]): over a sub-range only the control points of the knot spans
//! it meets matter (the B-splines not zero on an interval are independent on it), so a face
//! trimmed to the part of a spline where a row collapses is told a pole there.
//!
//! What cannot be decided is refused with a typed error: non-finite parameters, reach or
//! control points in the iso-line's columns, a weight not certified non-zero, a parameter off
//! the surface, knots with an empty span, a revolution's axis not of unit length, a direction
//! past v.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing tests

use super::spline::{
    linear, onto_knots, rotation_derivative, EvalCurve, EvalSurface, Partials, MAX_PARTIAL_ORDER,
    MAX_SPLINE_DEGREE,
};
use super::vec::VecExt;
use super::EvalError;
use crate::math::Vec3;
use crate::nurbs::math::{gamma, UNIT};

/// The directions a surface is collapsed along at a point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Collapse {
    /// Moving along either direction moves the point.
    Neither,
    /// Moving along u does not move the point (the iso-line along u is one point).
    U,
    /// Moving along v does not move the point.
    V,
    /// Neither direction moves it: a corner where two collapsed sides meet.
    Both,
}

impl Collapse {
    /// From whether each direction is collapsed.
    #[must_use]
    pub fn of([u, v]: [bool; 2]) -> Collapse {
        match (u, v) {
            (false, false) => Collapse::Neither,
            (true, false) => Collapse::U,
            (false, true) => Collapse::V,
            (true, true) => Collapse::Both,
        }
    }

    /// Whether each direction (u, v) is collapsed.
    #[must_use]
    pub fn flags(self) -> [bool; 2] {
        [
            matches!(self, Collapse::U | Collapse::Both),
            matches!(self, Collapse::V | Collapse::Both),
        ]
    }

    /// Whether direction `d` (0: u, 1: v) is collapsed; [`EvalError::Invalid`] past v.
    pub fn along(self, d: usize) -> Result<bool, EvalError> {
        self.flags().get(d).copied().ok_or_else(|| past(d))
    }
}

fn past(d: usize) -> EvalError {
    EvalError::Invalid(format!("pole classification: no parameter direction {d}"))
}

fn refused(what: &str) -> EvalError {
    EvalError::Unevaluable(format!("pole classification: {what}"))
}

fn malformed(what: &str) -> EvalError {
    EvalError::Invalid(format!("pole classification: {what}"))
}

/// The query's own checks, made once by every entry point: finite parameters, a finite
/// non-negative reach.
pub(crate) fn check(uv: [f64; 2], reach: f64) -> Result<(), EvalError> {
    if !(uv[0].is_finite() && uv[1].is_finite()) {
        return Err(refused(&format!(
            "parameters ({}, {}) not finite",
            uv[0], uv[1]
        )));
    }
    if !(reach >= 0.0 && reach.is_finite()) {
        return Err(malformed(&format!("reach {reach} is not a finite length")));
    }
    Ok(())
}

/// The one decision on a point set of bounding-box diagonal `diameter` (a circle's: twice its
/// radius), rounded within `err`: one point to rounding, or (when the set bounds the movement:
/// a hull of one-signed weights, a circle) one moving by at most `scale × (diameter + err)`
/// within `reach`.
fn decide(diameter: f64, err: f64, reach: f64, scale: f64) -> bool {
    diameter <= err || (reach > 0.0 && scale * (diameter + err) <= reach)
}

/// [`decide`] for a circle of radius `r` rounded within `err`, of unit scale.
pub(crate) fn circle(r: f64, err: f64, reach: f64) -> Result<bool, EvalError> {
    if !(r.is_finite() && err.is_finite()) {
        return Err(refused("a radius is not finite"));
    }
    Ok(decide(2.0 * r.abs(), 2.0 * err, reach, 1.0))
}

/// A homogeneous point with certified bounds on the rounding of its coordinates (`ex` on each
/// of x, y, z; `ew` on w).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Hom {
    h: [f64; 4],
    ex: f64,
    ew: f64,
}

impl Hom {
    fn exact(h: [f64; 4]) -> Hom {
        Hom {
            h,
            ex: 0.0,
            ew: 0.0,
        }
    }

    /// The Euclidean point and a certified bound on its error; refused when the weight is not
    /// certified non-zero or a coordinate is not finite.
    fn euclid(self) -> Result<(Vec3, f64), EvalError> {
        let [x, y, z, w] = self.h;
        if !self.h.iter().all(|c| c.is_finite()) {
            return Err(refused("a control point is not finite"));
        }
        if !(w.abs() > self.ew) {
            return Err(refused(&format!(
                "a weight {w} is not certified non-zero (its rounding is up to {})",
                self.ew
            )));
        }
        let p = Vec3::new(x, y, z) * (1.0 / w);
        // The numerator's and the weight's errors over the weight's floor, and the division's
        // own rounding.
        let e = (3f64.sqrt() * self.ex + p.len() * self.ew) / (w.abs() - self.ew)
            + 2.0 * UNIT * p.len();
        if !(p.is_finite() && e.is_finite()) {
            return Err(refused("a point is not finite"));
        }
        Ok((p, e))
    }
}

/// Why de Boor's algorithm has no point.
#[derive(Clone, Copy)]
enum Off {
    /// The parameter is off the knot range.
    Range,
    /// The knots have an empty span where one is needed, or the data do not fit them.
    Malformed,
}

fn off_message(off: Off, uv: [f64; 2]) -> EvalError {
    match off {
        Off::Range => refused(&format!("({}, {}) is off the surface", uv[0], uv[1])),
        Off::Malformed => malformed("knots with an empty span, or a net that does not fit them"),
    }
}

/// The point at `t` of the homogeneous B-spline of degree `p` on `knots` over `n` control
/// points given by `at` (each with its rounding), by de Boor's algorithm on a scratch array,
/// with its rounding: the inputs' plus `γ_(12p+4)` of their largest coordinate (each of the `p`
/// levels rounds its coefficient by three operations and its convex combination by four).
fn de_boor(
    p: usize,
    knots: &[f64],
    n: usize,
    at: impl Fn(usize) -> Option<Hom>,
    t: f64,
) -> Result<Hom, Off> {
    if p > MAX_SPLINE_DEGREE || n <= p {
        return Err(Off::Malformed);
    }
    let t = onto_knots(knots, p, n, t).ok_or(Off::Range)?;
    let last = n.checked_sub(1).ok_or(Off::Malformed)?;
    let mut k = p;
    while k < last
        && knots
            .get(k.checked_add(1).ok_or(Off::Malformed)?)
            .is_some_and(|x| *x <= t)
    {
        k = k.checked_add(1).ok_or(Off::Malformed)?;
    }
    let first = k.checked_sub(p).ok_or(Off::Malformed)?;
    let mut d = [Hom::default(); MAX_SPLINE_DEGREE + 1];
    let (mut ex, mut ew, mut sx, mut sw) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for (j, slot) in d.iter_mut().take(p + 1).enumerate() {
        let q = at(first.checked_add(j).ok_or(Off::Malformed)?).ok_or(Off::Malformed)?;
        ex = ex.max(q.ex);
        ew = ew.max(q.ew);
        sx = sx.max(q.h[0].abs()).max(q.h[1].abs()).max(q.h[2].abs());
        sw = sw.max(q.h[3].abs());
        *slot = q;
    }
    for r in 1..=p {
        for j in (r..=p).rev() {
            let i = first.checked_add(j).ok_or(Off::Malformed)?;
            let lo = *knots.get(i).ok_or(Off::Malformed)?;
            let hi_at = i
                .checked_add(p + 1)
                .and_then(|x| x.checked_sub(r))
                .ok_or(Off::Malformed)?;
            let span = *knots.get(hi_at).ok_or(Off::Malformed)? - lo;
            if !(span > 0.0) {
                return Err(Off::Malformed);
            }
            let a = (t - lo) / span;
            let prev = d
                .get(j.checked_sub(1).ok_or(Off::Malformed)?)
                .ok_or(Off::Malformed)?
                .h;
            let cur = d.get_mut(j).ok_or(Off::Malformed)?;
            for (c, q) in cur.h.iter_mut().zip(prev) {
                *c = (1.0 - a) * q + a * *c;
            }
        }
    }
    let g = gamma(12 * p + 4);
    let mut out = *d.get(p).ok_or(Off::Malformed)?;
    out.ex = ex + g * sx;
    out.ew = ew + g * sw;
    Ok(out)
}

/// An iso-line's homogeneous control points, gathered: their Euclidean bounding box, the
/// largest of their rounding bounds, and whether their weights share one sign.
struct Row {
    lo: Vec3,
    hi: Vec3,
    err: f64,
    sign: Option<bool>,
    mixed: bool,
    /// A control point at infinity (a zero weight under a numerator that is not zero).
    infinite: bool,
}

impl Row {
    fn new() -> Row {
        let inf = f64::INFINITY;
        Row {
            lo: Vec3::new(inf, inf, inf),
            hi: Vec3::new(-inf, -inf, -inf),
            err: 0.0,
            sign: None,
            mixed: false,
            infinite: false,
        }
    }

    fn push(&mut self, q: Hom) -> Result<(), EvalError> {
        // A weight not certified non-zero: with a numerator certified non-zero the point is
        // at infinity (`x_j = c w_j` cannot hold), so the line is no point, and it has no hull
        // to bound it; with both uncertain the control point is 0 / 0, refused.
        let [x, y, z, w] = q.h;
        if q.h.iter().all(|c| c.is_finite()) && !(w.abs() > q.ew) {
            if Vec3::new(x, y, z).len() > 3f64.sqrt() * q.ex {
                self.infinite = true;
                self.mixed = true;
                return Ok(());
            }
            return Err(refused(&format!(
                "a control point ({x}, {y}, {z}, {w}) is 0 / 0 to its rounding"
            )));
        }
        let (p, e) = q.euclid()?;
        self.lo = Vec3::new(self.lo.x.min(p.x), self.lo.y.min(p.y), self.lo.z.min(p.z));
        self.hi = Vec3::new(self.hi.x.max(p.x), self.hi.y.max(p.y), self.hi.z.max(p.z));
        self.err = self.err.max(e);
        let positive = q.h[3] > 0.0;
        match self.sign {
            None => self.sign = Some(positive),
            Some(s) => self.mixed |= s != positive,
        }
        Ok(())
    }

    /// The box's diagonal and a certified bound on its error: every coordinate of every point
    /// within `err`, so each extent within `2 err`; the extents' differences, squares, sums and
    /// square root rounded within `γ_6`.
    fn diameter(&self) -> (f64, f64) {
        let diag = (self.hi - self.lo).len();
        (diag, 2.0 * 3f64.sqrt() * self.err + gamma(6) * diag)
    }
}

/// The iso-line's span of control point indices along its direction (degree `p` on `knots`
/// over `n`), over its whole knot range or `range` (clamped to it): the points of the knot
/// spans the range meets, and the range's ends.
type Span = (usize, usize, f64, f64);

fn indices(p: usize, knots: &[f64], n: usize, range: Option<(f64, f64)>) -> Option<Span> {
    let (lo, hi) = (*knots.get(p)?, *knots.get(n)?);
    let (a, b) = match range {
        Some((a, b)) if a.is_finite() && b.is_finite() => (a.min(b).max(lo), a.max(b).min(hi)),
        Some(_) => return None,
        None => (lo, hi),
    };
    if !(a <= b) {
        return None;
    }
    let last = n.checked_sub(1)?;
    // The last span k in [p, n − 1] whose start is at or before t (`strict`: before t).
    let span = |t: f64, strict: bool| -> Option<usize> {
        let mut k = p;
        while k < last {
            let next = *knots.get(k.checked_add(1)?)?;
            if next < t || (!strict && next <= t) {
                k = k.checked_add(1)?;
            } else {
                break;
            }
        }
        Some(k)
    };
    let start = span(a, false)?;
    let first = start.checked_sub(p)?;
    let end = span(b, true)?.max(start);
    Some((first, end, a, b))
}

/// Whether the iso-line whose control points are `pts(j)`, `j` in the span's indices (degree
/// `p` on `knots` over `n`, taken over the span's range), is one point, or within `reach`
/// after a map that stretches by at most `scale` and shrinks by at least `floor` (module doc).
#[allow(clippy::too_many_arguments)]
fn iso_line(
    p: usize,
    knots: &[f64],
    n: usize,
    (first, end, a, b): Span,
    pts: &dyn Fn(usize) -> Result<Hom, EvalError>,
    reach: f64,
    scale: f64,
    floor: f64,
) -> Result<bool, EvalError> {
    let mut row = Row::new();
    for j in first..=end {
        row.push(pts(j)?)?;
    }
    let (diag, err) = row.diameter();
    if diag <= err && !row.infinite {
        return Ok(true);
    }
    if reach == 0.0 {
        return Ok(false);
    }
    if !row.mixed {
        return Ok(decide(diag, err, reach, scale));
    }
    // Weights of both signs: no hull. Two of the line's points farther apart than reach (after
    // the map, at least `floor` times their distance) certify that it moves more; else refuse.
    let mid = 0.5 * (a + b);
    let mut seen: [Option<(Vec3, f64)>; 3] = [None; 3];
    for (slot, t) in seen.iter_mut().zip([a, mid, b]) {
        if let Ok(q) = de_boor(p, knots, n, |j| pts(j).ok(), t) {
            *slot = q.euclid().ok();
        }
    }
    for (i, x) in seen.iter().enumerate() {
        for y in seen.iter().skip(i + 1) {
            if let (Some((x, ex)), Some((y, ey))) = (x, y) {
                if floor * (x.dist(*y) - ex - ey - gamma(5) * (x.len() + y.len())) > reach {
                    return Ok(false);
                }
            }
        }
    }
    Err(refused(
        "an iso-line with weights of both signs, not one point, whose points stay within reach",
    ))
}

/// `sqrt(‖M‖₁ ‖M‖∞)` of a row-major affine map's linear part (a bound on its spectral norm, 1
/// for a rotation), rounded up, and a lower bound on its smallest singular value
/// (`|det| / ‖M‖₂²`, the product of the singular values over the largest's square).
fn norms(m: &[[f64; 4]; 3]) -> (f64, f64) {
    let at = |r: usize, c: usize| m.get(r).and_then(|row| row.get(c)).copied().unwrap_or(0.0);
    let one = (0..3)
        .map(|c| (0..3).map(|r| at(r, c).abs()).sum::<f64>())
        .fold(0.0, f64::max);
    let inf = (0..3)
        .map(|r| (0..3).map(|c| at(r, c).abs()).sum::<f64>())
        .fold(0.0, f64::max);
    let top = (one * inf).sqrt() * (1.0 + gamma(12));
    let det = at(0, 0) * (at(1, 1) * at(2, 2) - at(1, 2) * at(2, 1))
        - at(0, 1) * (at(1, 0) * at(2, 2) - at(1, 2) * at(2, 0))
        + at(0, 2) * (at(1, 0) * at(2, 1) - at(1, 1) * at(2, 0));
    let bottom = det.abs() * (1.0 - gamma(16)) / (top * top);
    (top, bottom)
}

/// Whether `s` is collapsed along `d` (0: u, 1: v) at `uv` over the iso-line's whole range,
/// exactly (to rounding) or within `reach` mm (module doc).
pub fn collapsed(s: &EvalSurface, uv: [f64; 2], d: usize, reach: f64) -> Result<bool, EvalError> {
    collapsed_on(s, uv, d, reach, None)
}

/// [`collapsed`] over the iso-line's parameters within `range` only (a trimmed face's range
/// along `d`), `None` for its whole range. A plane is collapsed only where its axis is zero; a
/// revolution's circle along its angle has one radius whatever the range.
pub fn collapsed_on(
    s: &EvalSurface,
    uv: [f64; 2],
    d: usize,
    reach: f64,
    range: Option<(f64, f64)>,
) -> Result<bool, EvalError> {
    check(uv, reach)?;
    if d > 1 {
        return Err(past(d));
    }
    along(s, uv, d, reach, range)
}

/// [`collapsed_on`] after its checks; `d` is 0 or 1.
fn along(
    s: &EvalSurface,
    uv: [f64; 2],
    d: usize,
    reach: f64,
    range: Option<(f64, f64)>,
) -> Result<bool, EvalError> {
    match s {
        // An unbounded parameter moves the point without end unless its axis is zero.
        EvalSurface::Plane { du, dv, .. } => {
            let axis = if d == 0 { du } else { dv };
            if !axis.is_finite() {
                return Err(refused("a plane's axis is not finite"));
            }
            Ok(axis.len() == 0.0)
        }
        EvalSurface::Nurbs(n) => {
            let q = s.wrap(uv);
            let ctrl = n.ctrl();
            let (deg, knots, count) = (n.degree(), n.knots(), n.count());
            let o = usize::from(d == 0);
            let get = |a: &[usize; 2], k: usize| a.get(k).copied().unwrap_or(0);
            let (pd, po) = (get(&deg, d), get(&deg, o));
            let (nd, no, nu) = (get(&count, d), get(&count, o), get(&count, 0));
            let none: &[f64] = &[];
            let (kd, ko) = (
                knots.get(d).map_or(none, Vec::as_slice),
                knots.get(o).map_or(none, Vec::as_slice),
            );
            let t0 = q.get(o).copied().unwrap_or(f64::NAN);
            // Control point `along` (index along d) and `across` (along o) of the net.
            let net = |along: usize, across: usize| -> Option<Hom> {
                let (i, j) = if d == 0 {
                    (along, across)
                } else {
                    (across, along)
                };
                ctrl.get(j.checked_mul(nu)?.checked_add(i)?)
                    .copied()
                    .map(Hom::exact)
            };
            // The iso-line's control point j: its column's point at t0, de Boor along o.
            let pts = |j: usize| -> Result<Hom, EvalError> {
                de_boor(po, ko, no, |i| net(j, i), t0).map_err(|e| off_message(e, uv))
            };
            let span = indices(pd, kd, nd, range)
                .ok_or_else(|| malformed("a range off the spline, or knots that do not fit"))?;
            iso_line(pd, kd, nd, span, &pts, reach, 1.0, 1.0)
        }
        EvalSurface::Revolved {
            profile,
            origin,
            axis,
            transform,
            swap,
        } => {
            if !((axis.len() - 1.0).abs() <= gamma(16)) {
                return Err(malformed(&format!(
                    "a revolution's axis of length {} is not unit",
                    axis.len()
                )));
            }
            let (top, bottom) = norms(transform);
            if !(top.is_finite() && bottom.is_finite() && bottom > 0.0) {
                return Err(malformed("a revolution's map is not finite or is singular"));
            }
            let [a, b] = s.wrap(uv);
            let prof = if *swap { b } else { a };
            if (d == 1) ^ *swap {
                // Along the angle: the circle of the profile point's distance from the axis.
                let (r, err) = radius(profile, prof, *origin, *axis, uv)?;
                Ok(decide(2.0 * r, 2.0 * err, reach, top))
            } else {
                // Along the profile: the turned profile, decided on its control points (a turn
                // keeps distances; the map scales them within [bottom, top]).
                let (p, knots, ctrl) = (profile.degree(), profile.knots(), profile.ctrl());
                let pts = |j: usize| -> Result<Hom, EvalError> {
                    ctrl.get(j)
                        .copied()
                        .map(Hom::exact)
                        .ok_or_else(|| off_message(Off::Malformed, uv))
                };
                let span = indices(p, knots, ctrl.len(), range).ok_or_else(|| {
                    malformed("a range off the profile, or knots that do not fit")
                })?;
                iso_line(p, knots, ctrl.len(), span, &pts, reach, top, bottom)
            }
        }
    }
}

/// A profile point's distance from the axis through `origin` along the unit `axis`, and a
/// certified bound on its rounding; the profile is evaluated at `s` by de Boor.
fn radius(
    c: &EvalCurve,
    s: f64,
    origin: Vec3,
    axis: Vec3,
    uv: [f64; 2],
) -> Result<(f64, f64), EvalError> {
    let ctrl = c.ctrl();
    let q = de_boor(
        c.degree(),
        c.knots(),
        ctrl.len(),
        |j| ctrl.get(j).copied().map(Hom::exact),
        s,
    )
    .map_err(|e| off_message(e, uv))?;
    let (x, e) = q.euclid()?;
    let dv = x - origin;
    let r = (dv - axis * dv.dot(axis)).len();
    // The point's own error, and the difference, a dot product, a scaled difference and a length
    // on |x| + |origin| (the axis unit to within γ_16).
    let err = e + gamma(32) * (x.len() + origin.len());
    if !(r.is_finite() && err.is_finite()) {
        return Err(refused("a radius is not finite"));
    }
    Ok((r, err))
}

// ---- Certified partials ----
//
// The partials a pole's limit normal is read from ([`super::pole::limit_along`]), each with a
// certified bound on its rounding, from the same machinery: a B-spline's derivative is the
// B-spline of degree p − 1 on the inner knots whose homogeneous control points are
// `p (P_(i+1) − P_i) / (u_(i+p+1) − u_(i+1))` (each with its rounding), evaluated by
// [`de_boor`] (which carries its inputs' bounds); a rational surface's partials then follow from
// the homogeneous ones by Leibniz's rule, `A_ij = Σ C(i,k) C(j,l) w_kl S_(i−k)(j−l)`, the bounds
// carried through each product, sum and the division by the weight.

/// The derivative's control points and knots of the homogeneous B-spline of degree `p` on
/// `knots` over `pts`, each point with its rounding: the difference, the knot span and the
/// scale by `p / Δ` (`γ_4` of the result), on the inputs' own errors. A span of zero length
/// carries no basis function: its point is zero.
fn derive(p: usize, knots: &[f64], pts: &[Hom]) -> Option<(Vec<Hom>, Vec<f64>)> {
    let n = pts.len();
    if p == 0 || n < 2 {
        return None;
    }
    let mut out = Vec::new();
    for i in 0..n - 1 {
        let (a, b) = (pts.get(i)?, pts.get(i + 1)?);
        let span = knots.get(i + p + 1)? - knots.get(i + 1)?;
        if span == 0.0 {
            out.push(Hom::default());
            continue;
        }
        let f = p as f64 / span;
        let mut h = [0.0; 4];
        for ((x, bk), ak) in h.iter_mut().zip(b.h).zip(a.h) {
            *x = f * (bk - ak);
        }
        let g = gamma(4);
        let sx = h[0].abs().max(h[1].abs()).max(h[2].abs());
        out.push(Hom {
            h,
            ex: f.abs() * (a.ex + b.ex) * (1.0 + g) + g * sx,
            ew: f.abs() * (a.ew + b.ew) * (1.0 + g) + g * h[3].abs(),
        });
    }
    let inner = knots.get(1..knots.len().checked_sub(1)?)?.to_vec();
    Some((out, inner))
}

/// The `i`-th derivative at `t` of the homogeneous B-spline of degree `p` on `knots` over
/// `pts`, with its rounding: zero past its degree.
fn derivative_at(p: usize, knots: &[f64], pts: &[Hom], i: usize, t: f64) -> Result<Hom, Off> {
    if i > p {
        return Ok(Hom::default());
    }
    let (mut pts, mut knots) = (pts.to_vec(), knots.to_vec());
    for r in 0..i {
        let (q, k) = derive(p - r, &knots, &pts).ok_or(Off::Malformed)?;
        pts = q;
        knots = k;
    }
    let n = pts.len();
    de_boor(p - i, &knots, n, |j| pts.get(j).copied(), t)
}

/// A vector and a certified bound on its error's length.
type Bounded = (Vec3, f64);

/// Leibniz's quotient on a table of homogeneous partials `a[i][j]` (with their rounding):
/// the Euclidean partials `S_ij` for `i + j ≤ MAX_PARTIAL_ORDER`, each with its bound.
fn quotient(
    a: &[[Hom; MAX_PARTIAL_ORDER + 1]; MAX_PARTIAL_ORDER + 1],
) -> Result<[[Bounded; MAX_PARTIAL_ORDER + 1]; MAX_PARTIAL_ORDER + 1], EvalError> {
    let binom =
        |n: usize, k: usize| -> f64 { (1..=k).map(|i| (n + 1 - i) as f64 / i as f64).product() };
    let at = |i: usize, j: usize| a.get(i).and_then(|r| r.get(j)).copied().unwrap_or_default();
    let w00 = at(0, 0);
    let (w, ew) = (w00.h[3], w00.ew);
    if !(w.abs() > ew) {
        return Err(refused(&format!(
            "the weight {w} is not certified non-zero (rounding {ew})"
        )));
    }
    let mut s = [[(Vec3::ZERO, 0.0); MAX_PARTIAL_ORDER + 1]; MAX_PARTIAL_ORDER + 1];
    for order in 0..=MAX_PARTIAL_ORDER {
        for i in 0..=order {
            let j = order - i;
            let aij = at(i, j);
            let mut num = Vec3::new(aij.h[0], aij.h[1], aij.h[2]);
            let mut err = 3f64.sqrt() * aij.ex;
            let mut size = num.len();
            let mut terms = 0usize;
            for k in 0..=i {
                for l in 0..=j {
                    if k == 0 && l == 0 {
                        continue;
                    }
                    let c = binom(i, k) * binom(j, l);
                    let wk = at(k, l);
                    let (sv, se) = s
                        .get(i - k)
                        .and_then(|r| r.get(j - l))
                        .copied()
                        .unwrap_or((Vec3::ZERO, 0.0));
                    let t = sv * (c * wk.h[3]);
                    num -= t;
                    err += c * (wk.h[3].abs() * se + wk.ew * sv.len() + wk.ew * se);
                    size += t.len();
                    terms += 1;
                }
            }
            let q = num * (1.0 / w);
            // The sum's rounding over its terms, then the division with the weight's error.
            let e_num = err + gamma(2 * terms + 4) * size;
            let e = (e_num + q.len() * ew) / (w.abs() - ew) + 2.0 * UNIT * q.len();
            if !(q.is_finite() && e.is_finite()) {
                return Err(refused("a partial is not finite"));
            }
            if let Some(slot) = s.get_mut(i).and_then(|r| r.get_mut(j)) {
                *slot = (q, e * (1.0 + gamma(8)));
            }
        }
    }
    Ok(s)
}

/// The partials of `s` at `uv` to the fourth order ([`Partials`]) and a certified bound on each
/// one's rounding, from the net or profile by the classifier's de Boor machinery (above).
pub fn certified_partials(
    s: &EvalSurface,
    uv: [f64; 2],
) -> Result<
    (
        Partials,
        [[f64; MAX_PARTIAL_ORDER + 1]; MAX_PARTIAL_ORDER + 1],
    ),
    EvalError,
> {
    check(uv, 0.0)?;
    let mut d: Partials = [[Vec3::ZERO; MAX_PARTIAL_ORDER + 1]; MAX_PARTIAL_ORDER + 1];
    let mut e = [[0.0; MAX_PARTIAL_ORDER + 1]; MAX_PARTIAL_ORDER + 1];
    let mut put = |i: usize, j: usize, (v, err): Bounded| {
        if let (Some(x), Some(y)) = (
            d.get_mut(i).and_then(|r| r.get_mut(j)),
            e.get_mut(i).and_then(|r| r.get_mut(j)),
        ) {
            *x = v;
            *y = err;
        }
    };
    match s {
        EvalSurface::Plane { origin, du, dv } => {
            put(
                0,
                0,
                (
                    *origin + *du * uv[0] + *dv * uv[1],
                    gamma(4) * (origin.len() + (*du * uv[0]).len() + (*dv * uv[1]).len()),
                ),
            );
            put(1, 0, (*du, 0.0));
            put(0, 1, (*dv, 0.0));
        }
        EvalSurface::Nurbs(n) => {
            let [u, v] = s.wrap(uv);
            let [pu, pv] = n.degree();
            let [nu, nv] = n.count();
            let [ku, kv] = n.knots();
            let ctrl = n.ctrl();
            let off = |o: Off| off_message(o, uv);
            let mut a = [[Hom::default(); MAX_PARTIAL_ORDER + 1]; MAX_PARTIAL_ORDER + 1];
            for i in 0..=MAX_PARTIAL_ORDER {
                // Each v-column's i-th u-derivative at u: a homogeneous curve in v.
                let line: Vec<Hom> = (0..nv)
                    .map(|l| {
                        let col: Vec<Hom> = (0..nu)
                            .map(|k| {
                                ctrl.get(l * nu + k)
                                    .copied()
                                    .map(Hom::exact)
                                    .ok_or(Off::Malformed)
                            })
                            .collect::<Result<_, _>>()?;
                        derivative_at(pu, ku, &col, i, u)
                    })
                    .collect::<Result<_, _>>()
                    .map_err(off)?;
                for j in 0..=MAX_PARTIAL_ORDER - i {
                    let h = derivative_at(pv, kv, &line, j, v).map_err(off)?;
                    if let Some(x) = a.get_mut(i).and_then(|r| r.get_mut(j)) {
                        *x = h;
                    }
                }
            }
            let q = quotient(&a)?;
            for (i, row) in q.iter().enumerate() {
                for (j, b) in row.iter().enumerate() {
                    if i + j <= MAX_PARTIAL_ORDER {
                        put(i, j, *b);
                    }
                }
            }
        }
        EvalSurface::Revolved {
            profile,
            origin,
            axis,
            transform,
            swap,
        } => {
            let [a0, b0] = s.wrap(uv);
            let (t, angle) = if *swap { (b0, a0) } else { (a0, b0) };
            let pts: Vec<Hom> = profile.ctrl().iter().copied().map(Hom::exact).collect();
            let off = |o: Off| off_message(o, uv);
            let mut a = [[Hom::default(); MAX_PARTIAL_ORDER + 1]; MAX_PARTIAL_ORDER + 1];
            for i in 0..=MAX_PARTIAL_ORDER {
                let h =
                    derivative_at(profile.degree(), profile.knots(), &pts, i, t).map_err(off)?;
                if let Some(x) = a.get_mut(i).and_then(|r| r.get_mut(0)) {
                    *x = h;
                }
            }
            // The profile's Euclidean derivatives (a curve: the table's first column).
            let c = quotient(&a)?;
            let (top, _) = norms(transform);
            for i in 0..=MAX_PARTIAL_ORDER {
                let (ci, ei) = c
                    .get(i)
                    .and_then(|r| r.first())
                    .copied()
                    .unwrap_or((Vec3::ZERO, 0.0));
                let x = if i == 0 { ci - *origin } else { ci };
                let ex = if i == 0 {
                    ei + gamma(1) * (ci.len() + origin.len())
                } else {
                    ei
                };
                for j in 0..=MAX_PARTIAL_ORDER - i {
                    let r = rotation_derivative(*axis, x, angle, j);
                    // A rotation's derivatives move a vector by at most its length; the turn's
                    // own rounding (trigonometry, products, sums) within γ_16 of it.
                    let er = ex + gamma(16) * x.len();
                    let (val, err) = if i == 0 && j == 0 {
                        let p = super::spline::affine(transform, *origin + r);
                        (
                            p,
                            top * (er + gamma(2) * (origin.len() + r.len())) + gamma(8) * p.len(),
                        )
                    } else {
                        let p = linear(transform, r);
                        (p, top * er + gamma(8) * top * r.len())
                    };
                    if *swap {
                        put(j, i, (val, err));
                    } else {
                        put(i, j, (val, err));
                    }
                }
            }
        }
    }
    Ok((d, e))
}

/// Which directions `s` is collapsed along at `uv`, exactly (to rounding), or within `reach`
/// mm (module doc), each over the iso-line's whole range. Refused with a typed error where that
/// cannot be decided.
pub fn collapse(s: &EvalSurface, uv: [f64; 2], reach: f64) -> Result<Collapse, EvalError> {
    check(uv, reach)?;
    Ok(Collapse::of([
        along(s, uv, 0, reach, None)?,
        along(s, uv, 1, reach, None)?,
    ]))
}

/// [`collapse`] at `uv`, or, where `uv` is off a spline's knot range (past it by more than the
/// evaluator's rounding, as a pcurve's end fitted a little beyond the surface is), at the
/// nearest point of the range with `reach` less `onto_range`'s certified bound `δ`: every
/// point of an iso-line through `uv` (the surface's end polynomials continued past the range)
/// lies within `δ / 2` of a point of the one through that nearest point, so a line of diameter
/// at most `reach − δ` there has one of at most `reach` at `uv` (the triangle inequality), and
/// a pole answered is a pole within `reach`. A shift not under `reach` (at no reach, any shift)
/// is refused ([`EvalError::OffRange`], `OffRangeBeyondReach`), never answered; so is a
/// rational net off the range. Parameters on the range, a closed direction (wrapped by whole
/// periods: the same point), and any other surface go to [`collapse`] unchanged.
pub fn collapse_near(s: &EvalSurface, uv: [f64; 2], reach: f64) -> Result<Collapse, EvalError> {
    check(uv, reach)?;
    let Some((near, shift)) = onto_range(s, uv)? else {
        return collapse(s, uv, reach);
    };
    if !(shift < reach) {
        return Err(EvalError::OffRange(format!(
            "OffRangeBeyondReach: ({}, {}) is off the spline's knot range; moving it onto the \
             range shifts the surface by up to {shift} mm, not under the reach {reach} mm",
            uv[0], uv[1]
        )));
    }
    // The difference rounded down, so the reach asked is never above reach − δ.
    collapse(s, near, (reach - shift) * (1.0 - 2.0 * UNIT))
}

/// Where `uv` is off a polynomial spline's knot range: the nearest point of the range and a
/// certified bound `δ` on twice the distance a move between them shifts the surface's points,
/// along either iso-line; `None` on the range.
///
/// For a direction `e` that `uv` is off by `o_e`, take the strip of the net over `e`'s end knot
/// span (the span holding the nearest point) and the whole of the other direction `f`. On it
/// the partial `∂S/∂e` is the polynomial whose B-spline coefficients are
/// `p_e (P_(i+1) − P_i) / (t_(i+p_e+1) − t_(i+1))` over the strip's control points, within their
/// largest length `M_e` (a convex combination of them, in each patch's Bézier form after knot
/// insertion). Continued past a span of length `h` by `o` at degree `q`, the Bernstein basis
/// sums in absolute value to `(1 + 2o/h)^q`, so on the strip continued past the range
/// `|∂S/∂e| ≤ M_e (1 + 2o_e/h_e)^(p_e − 1) (1 + 2o_f/h_f)^(p_f)`, `h_f` the shortest span along
/// `f`. Every point of an iso-line through `uv` along `f` is then within `o_e` times that of the
/// line through the nearest point (moving across `e`), and the piece of an iso-line along `e`
/// past the range within the same of its end; `δ` is twice the sum over the directions, rounded
/// up. A strip with a weight other than the net's first, or not positive, is refused
/// ([`EvalError::OffRange`]): a rational continuation is not bounded here.
fn onto_range(s: &EvalSurface, uv: [f64; 2]) -> Result<Option<([f64; 2], f64)>, EvalError> {
    let EvalSurface::Nurbs(n) = s else {
        return Ok(None);
    };
    let q = s.wrap(uv);
    let (deg, knots, count, ctrl) = (n.degree(), n.knots(), n.count(), n.ctrl());
    let bad = || malformed("knots or a net that do not fit");
    let none: &[f64] = &[];
    let kn = |d: usize| knots.get(d).map_or(none, Vec::as_slice);
    let get = |a: &[usize; 2], d: usize| a.get(d).copied().unwrap_or(0);
    let mut near = q;
    let mut off = [0.0f64; 2];
    for d in 0..2 {
        let (p, m, k) = (get(&deg, d), get(&count, d), kn(d));
        let (&lo, &hi) = (k.get(p).ok_or_else(bad)?, k.get(m).ok_or_else(bad)?);
        let (Some(x), Some(o)) = (near.get_mut(d), off.get_mut(d)) else {
            return Err(bad());
        };
        if onto_knots(k, p, m, *x).is_none() {
            let t = *x;
            *x = if t < lo { lo } else { hi };
            *o = (t - *x).abs() * (1.0 + UNIT);
        }
    }
    if off == [0.0, 0.0] {
        return Ok(None);
    }
    // Each direction's end span at the nearest point (as de Boor takes it): its first control
    // point index and length, and the direction's shortest span.
    let mut span = [(0usize, 0.0f64, 0.0f64); 2];
    for d in 0..2 {
        let (p, m, k) = (get(&deg, d), get(&count, d), kn(d));
        let c = onto_knots(k, p, m, near.get(d).copied().unwrap_or(f64::NAN)).ok_or_else(bad)?;
        let last = m.checked_sub(1).ok_or_else(bad)?;
        let mut at = p;
        while at < last && k.get(at + 1).is_some_and(|y| *y <= c) {
            at += 1;
        }
        let h = k.get(at + 1).ok_or_else(bad)? - k.get(at).ok_or_else(bad)?;
        let shortest = (p..=last)
            .filter_map(|i| Some(k.get(i + 1)? - k.get(i)?))
            .filter(|h| *h > 0.0)
            .fold(f64::INFINITY, f64::min);
        let spans = h > 0.0 && shortest > 0.0 && shortest.is_finite();
        if !spans {
            return Err(bad());
        }
        if let Some(slot) = span.get_mut(d) {
            *slot = (at.checked_sub(p).ok_or_else(bad)?, h, shortest);
        }
    }
    let nu = get(&count, 0);
    let w0 = ctrl.first().ok_or_else(bad)?[3];
    let rational = || {
        EvalError::OffRange(format!(
            "({}, {}) is off the spline's knot range on a rational net, whose continuation past \
             the range is not bounded",
            uv[0], uv[1]
        ))
    };
    // Control point `i` along e, `j` across, as a Euclidean point of the one weight w0.
    let point = |e: usize, i: usize, j: usize| -> Result<Vec3, EvalError> {
        let (iu, iv) = if e == 0 { (i, j) } else { (j, i) };
        let c = ctrl
            .get(
                iv.checked_mul(nu)
                    .and_then(|x| x.checked_add(iu))
                    .ok_or_else(bad)?,
            )
            .copied()
            .ok_or_else(bad)?;
        let polynomial = c[3] == w0 && w0 > 0.0 && c.iter().all(|x| x.is_finite());
        if !polynomial {
            return Err(rational());
        }
        Ok(Vec3::new(c[0], c[1], c[2]) * (1.0 / w0))
    };
    let grow = |o: f64, h: f64, power: usize| -> f64 {
        (0..power).fold(1.0, |acc, _| acc * (1.0 + 2.0 * o / h))
    };
    let mut shift = 0.0f64;
    for e in 0..2 {
        let f = 1 - e;
        let (o_e, o_f) = (
            off.get(e).copied().unwrap_or(0.0),
            off.get(f).copied().unwrap_or(0.0),
        );
        let (p, pf, m_f, k) = (get(&deg, e), get(&deg, f), get(&count, f), kn(e));
        if o_e == 0.0 || p == 0 {
            continue;
        }
        let (Some(&(first, h_e, _)), Some(&(_, _, h_f))) = (span.get(e), span.get(f)) else {
            return Err(bad());
        };
        let mut top = 0.0f64;
        for a in first..first + p {
            let step = k.get(a + p + 1).ok_or_else(bad)? - k.get(a + 1).ok_or_else(bad)?;
            if !(step > 0.0) {
                return Err(bad());
            }
            for b in 0..m_f {
                let (x, y) = (point(e, a, b)?, point(e, a + 1, b)?);
                // The difference's length with its rounding on the points' own sizes (each
                // point's division, the difference, the length).
                let len = (y - x).len() + gamma(8) * (x.len() + y.len());
                top = top.max(p as f64 * len / step);
            }
        }
        shift += o_e * top * grow(o_e, h_e, p - 1) * grow(o_f, h_f, pf);
    }
    // Twice the bound, rounded up over its few dozen operations (each point's division, the
    // differences, lengths, quotients, powers and products).
    let delta = 2.0 * shift * (1.0 + gamma(64));
    if !delta.is_finite() {
        return Err(refused("a shift onto the knot range is not finite"));
    }
    Ok(Some((near, delta)))
}

#[cfg(test)]
mod tests;
