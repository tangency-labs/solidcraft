//! Recognition of one circle or ellipse from planar segments, without changing the input.
//!
//! Not upstream. Moved from SolidCraft's DXF path conversion into the kernel.
//! References: Piegl and Tiller, The NURBS Book, second edition, rational quadratic arcs;
//! Higham, Accuracy and Stability of Numerical Algorithms, second edition, rounding errors.
//! Decisions use the original stored values and outward error enclosures, never samples.
//! Deviations: none. No recursion.

use super::profile::{Loop2, Seg2};
use crate::geom::certified::{next_down, next_up};
use crate::math::Vec2;
use std::f64::consts::{PI, TAU};

// Keep hypot's overflow behaviour and the operation order of the original construction.
pub(super) trait RecognitionVector {
    fn is_finite(&self) -> bool;
    fn cross(self, rhs: Self) -> f64;
    fn length_hypot(self) -> f64;
    fn perp(self) -> Self;
    fn angle(self) -> f64;
    #[cfg(test)]
    fn dist(self, rhs: Self) -> f64;
}
impl RecognitionVector for Vec2 {
    fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
    fn cross(self, rhs: Self) -> f64 {
        self.x * rhs.y - self.y * rhs.x
    }
    fn length_hypot(self) -> f64 {
        self.x.hypot(self.y)
    }
    fn perp(self) -> Self {
        Self::new(-self.y, self.x)
    }
    fn angle(self) -> f64 {
        self.y.atan2(self.x)
    }
    #[cfg(test)]
    fn dist(self, rhs: Self) -> f64 {
        (self - rhs).length_hypot()
    }
}

/// An elliptical arc with a directed, unwrapped parameter interval. Its points are
/// `center + major * cos(t) + major.perp() * ratio * sin(t)`; `end < start` runs clockwise.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EllipseArc {
    /// The centre in the input plane.
    pub center: Vec2,
    /// The directed major semiaxis.
    pub major: Vec2,
    /// The positive minor-to-major axis ratio.
    pub ratio: f64,
    /// The starting parameter in radians.
    pub start: f64,
    /// The ending parameter in radians, retaining direction.
    pub end: f64,
}

/// Recognize the ellipse supporting a rational quadratic with endpoint weights 1.
/// Weights close to 1 and degenerate/non-finite extractions return `None`; callers can
/// retain the original rational curve. The axis ratio is never clamped into range.
pub fn conic_ellipse(a: Vec2, apex: Vec2, b: Vec2, w: f64) -> Option<EllipseArc> {
    if !(a.is_finite() && apex.is_finite() && b.is_finite() && w > 1e-9 && w < 1.0 - 1e-6) {
        return None;
    }
    let m = (a + b) * 0.5;
    let center = m - (apex - m) * (w * w / (1.0 - w * w));
    let alpha = w.acos();
    let left = m - center;
    let right = b - m;
    let sin_alpha = alpha.sin();
    let (u, v) = (
        Vec2::new(left.x / w, left.y / w),
        Vec2::new(right.x / sin_alpha, right.y / sin_alpha),
    );
    if !center.is_finite() || u.cross(v).abs() < 1e-18 {
        return None;
    }
    let (major, minor, ratio, t) = ellipse_axes(u, v)?;
    if major.length_hypot() <= 1e-12 {
        return None;
    }
    let (start, end) = if major.cross(minor) > 0.0 {
        (-alpha - t, alpha - t)
    } else {
        (t + alpha, t - alpha)
    };
    Some(EllipseArc {
        center,
        major,
        ratio,
        start,
        end,
    })
}

fn ellipse_axes(u: Vec2, v: Vec2) -> Option<(Vec2, Vec2, f64, f64)> {
    let (uu, uv, vv) = (u.dot(u), u.dot(v), v.dot(v));
    let cross = u.cross(v);
    if !(u.is_finite()
        && v.is_finite()
        && uu.is_finite()
        && uv.is_finite()
        && vv.is_finite()
        && cross.is_finite())
        || cross == 0.0
    {
        return None;
    }
    let t = 0.5 * (2.0 * uv).atan2(uu - vv);
    if !(2.0 * uv).is_finite() || !(uu - vv).is_finite() {
        return None;
    }
    let major = u * t.cos() + v * t.sin();
    let minor = v * t.cos() - u * t.sin();
    let ratio = minor.length_hypot() / major.length_hypot();
    (major.is_finite() && minor.is_finite() && ratio.is_finite() && ratio > 0.0 && ratio <= 1.0)
        .then_some((major, minor, ratio, t))
}

// Angular recognition uses the rounding cells of the stored parameters, not a spatial
// tolerance. Each +/- propagates operand radii and one rounding of its result. Radius
// additions round outward. This is a finite-precision recognition policy, not permission
// to snap path vertices or to accept an individual sweep outside [-TAU, TAU].
#[derive(Clone, Copy)]
struct AngleResolution {
    value: f64,
    radius: f64,
}

fn rounding_radius(value: f64) -> f64 {
    let spacing = (next_up(value) - value).max(value - next_down(value));
    // Half the minimum subnormal cannot be represented; enclose it outward instead.
    (spacing * 0.5).max(f64::from_bits(1))
}

// Error-free addition residual for finite operands whose sum does not overflow.
fn sum_roundoff(a: f64, b: f64) -> f64 {
    let sum = a + b;
    let virtual_b = sum - a;
    (a - (sum - virtual_b)) + (b - virtual_b)
}

impl AngleResolution {
    fn new(value: f64) -> Self {
        Self {
            value,
            radius: rounding_radius(value),
        }
    }

    fn add(self, rhs: Self) -> Self {
        let value = self.value + rhs.value;
        let radius = next_up(next_up(self.radius + rhs.radius) + rounding_radius(value));
        Self { value, radius }
    }

    fn sub(self, rhs: Self) -> Self {
        self.add(Self {
            value: -rhs.value,
            radius: rhs.radius,
        })
    }

    fn times_integer(self, factor: f64) -> Self {
        let value = self.value * factor;
        let radius = next_up(next_up(self.radius * factor.abs()) + rounding_radius(value));
        Self { value, radius }
    }

    fn contains_zero(self) -> bool {
        self.value.is_finite() && self.radius.is_finite() && self.value.abs() <= self.radius
    }
}

// Fixed precision for whole-ellipse reconstruction. The exact expression is enclosed by
// hi + lo +/- radius; no operation is allowed to enlarge the geometric acceptance cell.
// Crate-private visibility lets the existing area tests inspect the arithmetic certificate.
#[derive(Clone, Copy, Debug)]
pub(super) struct EllipseScalar {
    pub(super) hi: f64,
    pub(super) lo: f64,
    pub(super) radius: f64,
}

fn ellipse_two_sum(a: f64, b: f64) -> Option<(f64, f64)> {
    let sum = a + b;
    let virtual_b = sum - a;
    let virtual_a = sum - virtual_b;
    let error_b = b - virtual_b;
    let error_a = a - virtual_a;
    let error = error_a + error_b;
    [sum, virtual_b, virtual_a, error_b, error_a, error]
        .into_iter()
        .all(f64::is_finite)
        .then_some((sum, error))
}

// Arguments are nonnegative bounds. Preserve exact zero rather than inventing uncertainty
// for a TwoSum of exact inputs; nonzero products include underflow in their outward bound.
fn ellipse_bound_add(a: f64, b: f64) -> f64 {
    if a == 0.0 {
        b
    } else if b == 0.0 {
        a
    } else {
        next_up(a + b)
    }
}

fn ellipse_bound_mul(a: f64, b: f64) -> f64 {
    if a == 0.0 || b == 0.0 {
        0.0
    } else {
        next_up(a * b)
    }
}

impl EllipseScalar {
    const ZERO: Self = Self {
        hi: 0.0,
        lo: 0.0,
        radius: 0.0,
    };
    const ONE: Self = Self {
        hi: 1.0,
        lo: 0.0,
        radius: 0.0,
    };

    fn checked(hi: f64, lo: f64, radius: f64) -> Option<Self> {
        (hi.is_finite() && lo.is_finite() && radius.is_finite() && radius >= 0.0).then_some(Self {
            hi,
            lo,
            radius,
        })
    }

    pub(super) fn exact(value: f64) -> Option<Self> {
        Self::checked(value, 0.0, 0.0)
    }

    pub(super) fn plus(self, rhs: Self) -> Option<Self> {
        let (s, e) = ellipse_two_sum(self.hi, rhs.hi)?;
        let (t, f) = ellipse_two_sum(self.lo, rhs.lo)?;
        let (u, g) = ellipse_two_sum(e, t)?;
        let (v, k) = ellipse_two_sum(s, u)?;
        let (l, r1) = ellipse_two_sum(k, g)?;
        let (l, r2) = ellipse_two_sum(l, f)?;
        let (hi, lo) = ellipse_two_sum(v, l)?;
        let radius = ellipse_bound_add(
            ellipse_bound_add(self.radius, rhs.radius),
            ellipse_bound_add(r1.abs(), r2.abs()),
        );
        Self::checked(hi, lo, radius)
    }

    fn negated(self) -> Self {
        Self {
            hi: -self.hi,
            lo: -self.lo,
            radius: self.radius,
        }
    }

    pub(super) fn minus(self, rhs: Self) -> Option<Self> {
        self.plus(rhs.negated())
    }

    pub(super) fn times(self, rhs: Self) -> Option<Self> {
        let mut product = Self::ZERO;
        for (a, b) in [
            (self.hi, rhs.hi),
            (self.hi, rhs.lo),
            (self.lo, rhs.hi),
            (self.lo, rhs.lo),
        ] {
            let p = a * b;
            let e = a.mul_add(b, -p);
            let (hi, lo) = ellipse_two_sum(p, e)?;
            // FMA's residual can underflow: enclose its rounding instead of assuming an
            // exact TwoProduct, including when the stored residual happens to be zero.
            product = product.plus(Self::checked(hi, lo, rounding_radius(e))?)?;
        }
        let a_bound = ellipse_bound_add(self.hi.abs(), self.lo.abs());
        let b_bound = ellipse_bound_add(rhs.hi.abs(), rhs.lo.abs());
        let input_error = ellipse_bound_add(
            ellipse_bound_add(
                ellipse_bound_mul(a_bound, rhs.radius),
                ellipse_bound_mul(b_bound, self.radius),
            ),
            ellipse_bound_mul(self.radius, rhs.radius),
        );
        Self::checked(
            product.hi,
            product.lo,
            ellipse_bound_add(product.radius, input_error),
        )
    }

    fn lower(self) -> f64 {
        next_down(next_down(self.hi - self.lo.abs()) - self.radius)
    }

    pub(super) fn magnitude_bound(self) -> f64 {
        ellipse_bound_add(ellipse_bound_add(self.hi.abs(), self.lo.abs()), self.radius)
    }

    pub(super) fn sign(self) -> Option<i8> {
        if self.hi == 0.0 && self.lo == 0.0 && self.radius == 0.0 {
            Some(0)
        } else if self.lower() > 0.0 {
            Some(1)
        } else if self.negated().lower() > 0.0 {
            Some(-1)
        } else {
            None
        }
    }

    pub(super) fn divided_by(self, rhs: Self) -> Option<Self> {
        let denominator = next_down(next_down(rhs.hi.abs() - rhs.lo.abs()) - rhs.radius);
        if denominator <= 0.0 || !denominator.is_finite() {
            return None;
        }
        let q0 = Self::exact(self.hi / rhs.hi)?;
        let residual = self.minus(q0.times(rhs)?)?;
        let q1 = (residual.hi + residual.lo) / rhs.hi;
        let (hi, lo) = ellipse_two_sum(q0.hi, q1)?;
        let quotient = Self::checked(hi, lo, 0.0)?;
        // A/B-Q = (A-QB)/B: the predictor gets one correction, then a certificate.
        let residual = self.minus(quotient.times(rhs)?)?;
        Self::checked(hi, lo, next_up(residual.magnitude_bound() / denominator))
    }

    pub(super) fn square_root(self) -> Option<Self> {
        if self.lower() <= 0.0 {
            return None;
        }
        let s0 = Self::exact((self.hi + self.lo).sqrt())?;
        let denominator = 2.0 * s0.hi;
        if denominator <= 0.0 || !denominator.is_finite() {
            return None;
        }
        let residual = self.minus(s0.times(s0)?)?;
        let s1 = (residual.hi + residual.lo) / denominator;
        let (hi, lo) = ellipse_two_sum(s0.hi, s1)?;
        let root = Self::checked(hi, lo, 0.0)?;
        let lower = root.lower();
        if lower <= 0.0 {
            return None;
        }
        // |sqrt(A)-Q| = |A-Q²|/(sqrt(A)+Q) <= |A-Q²|/lower(Q).
        let residual = self.minus(root.times(root)?)?;
        Self::checked(hi, lo, next_up(residual.magnitude_bound() / lower))
    }

    fn exact_half(self) -> Option<Self> {
        let (hi, lo) = (self.hi * 0.5, self.lo * 0.5);
        if self.radius != 0.0 || hi * 2.0 != self.hi || lo * 2.0 != self.lo {
            return None;
        }
        let (hi, lo) = ellipse_two_sum(hi, lo)?;
        Self::checked(hi, lo, 0.0)
    }

    pub(super) fn fits(self, radius: f64) -> bool {
        radius.is_finite() && radius >= 0.0 && self.magnitude_bound() <= radius
    }

    pub(super) fn rounds_to(self, value: f64) -> Option<bool> {
        let value = Self::exact(value)?;
        let lower = value
            .plus(Self::exact(next_down(value.hi))?)?
            .exact_half()?;
        let upper = value.plus(Self::exact(next_up(value.hi))?)?.exact_half()?;
        // Strict inclusion conservatively refuses ties and every uncertain boundary.
        Some(self.minus(lower)?.sign() == Some(1) && upper.minus(self)?.sign() == Some(1))
    }
}

pub(super) fn ellipse_local_resolution(min: f64, max: f64) -> Option<f64> {
    let extent = EllipseScalar::exact(max)?
        .minus(EllipseScalar::exact(min)?)?
        .exact_half()?;
    let lower = extent.lower();
    if lower <= 0.0 || !lower.is_finite() {
        return None;
    }
    let gap = next_up(lower) - lower;
    let radius = gap * 0.5;
    // Choose the lower binade if the extent straddles a power of two. Underflow must
    // not invent a larger cell. Only original vertices contribute to this budget.
    (radius > 0.0 && radius.is_finite() && radius * 2.0 == gap).then_some(radius)
}

#[derive(Clone, Copy)]
struct EllipsePoint {
    x: EllipseScalar,
    y: EllipseScalar,
}

impl EllipsePoint {
    fn relative(point: Vec2, origin: Vec2) -> Option<Self> {
        Some(Self {
            x: EllipseScalar::exact(point.x)?.minus(EllipseScalar::exact(origin.x)?)?,
            y: EllipseScalar::exact(point.y)?.minus(EllipseScalar::exact(origin.y)?)?,
        })
    }

    fn plus(self, rhs: Self) -> Option<Self> {
        Some(Self {
            x: self.x.plus(rhs.x)?,
            y: self.y.plus(rhs.y)?,
        })
    }

    fn minus(self, rhs: Self) -> Option<Self> {
        Some(Self {
            x: self.x.minus(rhs.x)?,
            y: self.y.minus(rhs.y)?,
        })
    }

    fn divided_by(self, rhs: EllipseScalar) -> Option<Self> {
        Some(Self {
            x: self.x.divided_by(rhs)?,
            y: self.y.divided_by(rhs)?,
        })
    }

    fn cross(self, rhs: Self) -> Option<EllipseScalar> {
        self.x.times(rhs.y)?.minus(self.y.times(rhs.x)?)
    }

    fn fits(self, radius: Vec2) -> bool {
        self.x.fits(radius.x) && self.y.fits(radius.y)
    }
}

#[derive(Clone, Copy)]
struct EllipseSupport {
    xx: EllipseScalar,
    xy: EllipseScalar,
    yy: EllipseScalar,
    determinant: EllipseScalar,
}

impl EllipseSupport {
    fn new(major: Vec2, ratio: f64) -> Option<Self> {
        let (x, y, r) = (
            EllipseScalar::exact(major.x)?,
            EllipseScalar::exact(major.y)?,
            EllipseScalar::exact(ratio)?,
        );
        let (x2, y2, r2) = (x.times(x)?, y.times(y)?, r.times(r)?);
        let xx = x2.plus(r2.times(y2)?)?;
        let yy = y2.plus(r2.times(x2)?)?;
        let xy = x.times(y)?.times(EllipseScalar::ONE.minus(r2)?)?;
        let determinant = xx.times(yy)?.minus(xy.times(xy)?)?;
        (xx.sign() == Some(1) && yy.sign() == Some(1) && determinant.sign() == Some(1)).then_some(
            Self {
                xx,
                xy,
                yy,
                determinant,
            },
        )
    }

    fn inner(self, a: EllipsePoint, b: EllipsePoint) -> Option<EllipseScalar> {
        let diagonal = self
            .yy
            .times(a.x)?
            .times(b.x)?
            .plus(self.xx.times(a.y)?.times(b.y)?)?;
        let off_diagonal = self.xy.times(a.x.times(b.y)?.plus(a.y.times(b.x)?)?)?;
        diagonal.minus(off_diagonal)?.divided_by(self.determinant)
    }

    fn witness(self, original: EllipsePoint, radius: Vec2) -> Option<EllipsePoint> {
        let norm = self.inner(original, original)?.square_root()?;
        let witness = original.divided_by(norm)?;
        witness.minus(original)?.fits(radius).then_some(witness)
    }
}

#[derive(Clone, Copy)]
struct EllipseCandidate {
    a: Vec2,
    apex: Vec2,
    b: Vec2,
    w: f64,
    chord2: f64,
}

impl EllipseCandidate {
    fn new(a: Vec2, apex: Vec2, b: Vec2, w: f64) -> Option<Self> {
        if !(a.is_finite() && apex.is_finite() && b.is_finite() && w > 1e-9 && w < 1.0 - 1e-6) {
            return None;
        }
        let canonical = |p: Vec2| {
            Vec2::new(
                if p.x == 0.0 { 0.0 } else { p.x },
                if p.y == 0.0 { 0.0 } else { p.y },
            )
        };
        let (mut a, mut b, apex) = (canonical(a), canonical(b), canonical(apex));
        if (b.x, b.y) < (a.x, a.y) {
            std::mem::swap(&mut a, &mut b);
        }
        let delta = EllipsePoint::relative(b, a)?;
        let d = Vec2::new(delta.x.hi + delta.x.lo, delta.y.hi + delta.y.lo);
        let chord2 = d.length_squared();
        // This finite f64 ranking proposes ONE candidate; it is not a certificate of
        // the mathematically longest chord. Exact ties use the same canonical key.
        (chord2.is_finite() && chord2 > 0.0).then_some(Self {
            a,
            apex,
            b,
            w,
            chord2,
        })
    }

    fn key(self) -> [f64; 7] {
        [
            self.a.x,
            self.a.y,
            self.b.x,
            self.b.y,
            self.apex.x,
            self.apex.y,
            self.w,
        ]
    }

    fn ellipse(self, radius: Vec2) -> Option<(Vec2, Vec2, f64)> {
        let d = EllipsePoint::relative(self.b, self.a)?;
        let p = EllipsePoint::relative(self.apex, self.a)?;
        // Rounded local values are used only to propose a triple. Certification below
        // reconstructs from the original controls with their subtraction residuals.
        let d = Vec2::new(d.x.hi + d.x.lo, d.y.hi + d.y.lo);
        let p = Vec2::new(p.x.hi + p.x.lo, p.y.hi + p.y.lo);
        let m = d * 0.5;
        let h = p - m;
        let q = 1.0 - self.w * self.w;
        let k = self.w * self.w / q;
        let local_center = m - h * k;
        let (cx, error_x) = ellipse_two_sum(self.a.x, local_center.x)?;
        let (cy, error_y) = ellipse_two_sum(self.a.y, local_center.y)?;
        if error_x.abs() > radius.x || error_y.abs() > radius.y {
            return None;
        }
        let (along, chord) = (k / q, 0.25 / q);
        let xx = h.x * h.x * along + d.x * d.x * chord;
        let xy = h.x * h.y * along + d.x * d.y * chord;
        let yy = h.y * h.y * along + d.y * d.y * chord;
        if ![q, k, along, chord, xx, xy, yy]
            .into_iter()
            .all(f64::is_finite)
            || q <= 0.0
            || xx <= 0.0
            || yy <= 0.0
        {
            return None;
        }
        let (mut major, ratio) = if xy == 0.0 {
            // Preserve coordinate axes exactly, including the isotropic case.
            if xx >= yy {
                (Vec2::new(xx.sqrt(), 0.0), (yy / xx).sqrt())
            } else {
                (Vec2::new(0.0, yy.sqrt()), (xx / yy).sqrt())
            }
        } else {
            let difference = xx - yy;
            let discriminant = (difference * difference + 4.0 * xy * xy).sqrt();
            let large = (xx + yy + discriminant) * 0.5;
            let small = (xx * yy - xy * xy) / large;
            if !discriminant.is_finite()
                || !large.is_finite()
                || !small.is_finite()
                || small <= 0.0
                || large <= 0.0
            {
                return None;
            }
            let axis = if xx >= yy {
                Vec2::new(large - yy, xy)
            } else {
                Vec2::new(xy, large - xx)
            };
            let length = axis.length_hypot();
            if !length.is_finite() || length <= 0.0 {
                return None;
            }
            (axis * (large.sqrt() / length), (small / large).sqrt())
        };
        if !major.is_finite() || !ratio.is_finite() || ratio <= 0.0 || ratio > 1.0 {
            return None;
        }
        if major.x < 0.0 || (major.x == 0.0 && major.y < 0.0) {
            major = -major;
        }
        Some((Vec2::new(cx, cy), major, ratio))
    }
}

fn loop_as_general_ellipse(lp: &Loop2) -> Option<(Vec2, Vec2, f64)> {
    let &Seg2::Conic {
        a: first,
        apex,
        b,
        w,
    } = lp.segs.first()?
    else {
        return None;
    };
    let mut candidate = EllipseCandidate::new(first, apex, b, w)?;
    let (mut min, mut max) = (first, first);
    for (seg, next) in lp.segs.iter().zip(lp.segs.iter().cycle().skip(1)) {
        let (&Seg2::Conic { a, apex, b, w }, &Seg2::Conic { a: next_a, .. }) = (seg, next) else {
            return None;
        };
        let proposed = EllipseCandidate::new(a, apex, b, w)?;
        if b != next_a {
            return None;
        }
        if proposed.chord2 > candidate.chord2
            || (proposed.chord2 == candidate.chord2 && proposed.key() < candidate.key())
        {
            candidate = proposed;
        }
        min = Vec2::new(min.x.min(a.x), min.y.min(a.y));
        max = Vec2::new(max.x.max(a.x), max.y.max(a.y));
    }
    let radius = Vec2::new(
        ellipse_local_resolution(min.x, max.x)?,
        ellipse_local_resolution(min.y, max.y)?,
    );
    let (center, major, ratio) = candidate.ellipse(radius)?;
    // Freeze the FINAL returned triple before any reconstruction. No candidate
    // retry, axis renormalization, per-piece support or adjustable error allowance.
    let support = EllipseSupport::new(major, ratio)?;
    let origin = candidate.a;
    let local_center = EllipsePoint::relative(center, origin)?;
    let first_y = EllipsePoint::relative(first, origin)?.minus(local_center)?;
    let first_x = support.witness(first_y, radius)?;
    let (mut from_y, mut from_x) = (first_y, first_x);
    let (mut direction, mut winding) = (None, 0_i32);
    for (index, seg) in lp.segs.iter().enumerate() {
        let &Seg2::Conic { apex, b, w, .. } = seg else {
            return None;
        };
        let (to_y, to_x) = if index == lp.segs.len() - 1 {
            (first_y, first_x)
        } else {
            let y = EllipsePoint::relative(b, origin)?.minus(local_center)?;
            (y, support.witness(y, radius)?)
        };
        let apex_y = EllipsePoint::relative(apex, origin)?.minus(local_center)?;
        let sign = from_y.cross(to_y)?.sign()?;
        // These three signs put the rational numerator in one open cone of width
        // < pi and give its angular derivative a constant nonzero sign.
        if sign == 0
            || from_y.cross(apex_y)?.sign() != Some(sign)
            || apex_y.cross(to_y)?.sign() != Some(sign)
            || direction.is_some_and(|previous| previous != sign)
        {
            return None;
        }
        direction = Some(sign);
        let h = support.inner(from_x, to_x)?;
        let denominator = EllipseScalar::ONE.plus(h)?;
        if denominator.sign() != Some(1) || EllipseScalar::ONE.minus(h)?.sign() != Some(1) {
            return None;
        }
        // The witnesses, required apex and positive minor-arc weight satisfy all
        // five homogeneous conic identities on the SAME fixed support by construction.
        let required_apex = from_x.plus(to_x)?.divided_by(denominator)?;
        let required_weight = denominator
            .times(EllipseScalar::exact(0.5)?)?
            .square_root()?;
        if !required_apex.minus(apex_y)?.fits(radius) || !required_weight.rounds_to(w)? {
            return None;
        }
        let (from_sign, to_sign) = (from_y.y.sign()?, to_y.y.sign()?);
        if (from_sign <= 0 && to_sign > 0 && sign > 0)
            || (to_sign <= 0 && from_sign > 0 && sign < 0)
        {
            winding = winding.checked_add(i32::from(sign))?;
        }
        (from_y, from_x) = (to_y, to_x);
    }
    // Each source conic and its chord stay in the same open cone, so this half-open
    // polygon crossing count is also the curve's winding. No angle sum is needed.
    (winding == 1 || winding == -1).then_some((center, major, ratio))
}

fn loop_as_quarter_ellipse(lp: &Loop2) -> Option<(Vec2, Vec2, f64)> {
    let [q0, q1, q2, q3] = lp.segs.as_slice() else {
        return None;
    };
    let (
        &Seg2::Conic { a: p0, .. },
        &Seg2::Conic { a: p1, .. },
        &Seg2::Conic { a: p2, .. },
        &Seg2::Conic { a: p3, .. },
    ) = (q0, q1, q2, q3)
    else {
        return None;
    };
    let center = (p0 + p2) * 0.5;
    if !center.is_finite() || center != (p1 + p3) * 0.5 {
        return None;
    }
    let offsets = [p0 - center, p1 - center, p2 - center, p3 - center];
    if offsets.iter().any(|d| !d.is_finite()) {
        return None;
    }
    let extent = offsets.iter().fold(Vec2::ZERO, |size, d| {
        Vec2::new(size.x.max(d.x.abs()), size.y.max(d.y.abs()))
    });
    // Do not lose a significant LOCAL displacement when averaging large world
    // coordinates. TwoSum recovers what the world-coordinate sum discarded; half
    // that residue must fit the rounding cell of the local extent in each axis.
    for (a, b) in [(p0, p2), (p1, p3)] {
        let error = Vec2::new(sum_roundoff(a.x, b.x), sum_roundoff(a.y, b.y)) * 0.5;
        if !error.is_finite()
            || error.x.abs() > rounding_radius(extent.x)
            || error.y.abs() > rounding_radius(extent.y)
        {
            return None;
        }
    }
    // A rounded world-coordinate sum can hide asymmetric endpoints. A displacement
    // from either endpoint must reconstruct BOTH original points. Trying both keeps
    // ordinary affine constructions whose inverse subtraction rounds differently.
    let opposite = |a: Vec2, b: Vec2| {
        [a - center, center - b]
            .into_iter()
            .find(|&d| center + d == a && center - d == b)
    };
    let (u, v) = (opposite(p0, p2)?, opposite(p1, p3)?);
    let axes = [u, v, -u, -v];
    let mut direction = None;
    for ((seg, next), (from, to)) in lp
        .segs
        .iter()
        .zip(lp.segs.iter().cycle().skip(1))
        .zip(axes.iter().zip(axes.iter().cycle().skip(1)))
    {
        let (&Seg2::Conic { a, apex, b, w }, &Seg2::Conic { a: next_a, .. }) = (seg, next) else {
            return None;
        };
        if !(a.is_finite() && apex.is_finite() && b.is_finite())
            || w != std::f64::consts::FRAC_1_SQRT_2
            || b != next_a
        {
            return None;
        }
        // Every endpoint and apex must use the SAME pair of accepted offsets.
        // The alternatives only change the association of this affine construction.
        if a != center + *from
            || b != center + *to
            || (apex != a + *to && apex != b + *from && apex != center + (*from + *to))
        {
            return None;
        }
        let cross = from.cross(*to);
        if !cross.is_finite()
            || cross == 0.0
            || direction.is_some_and(|positive| positive != cross.is_sign_positive())
        {
            return None;
        }
        direction = Some(cross.is_sign_positive());
    }
    let (mut major, _, ratio, _) = ellipse_axes(u, v)?;
    if major.x < 0.0 || (major.x == 0.0 && major.y < 0.0) {
        major = -major;
    }
    Some((center, major, ratio))
}

/// Recognize one directed turn of circular arcs on exactly the same center and radius.
/// Joins and the accumulated sweep must agree at the stored angles' floating-point
/// resolution. No trigonometric endpoint or world-space tolerance is used. Ambiguous
/// phases, gaps, mixed directions, repeated turns and non-arc pieces return `None`.
pub fn loop_circle(lp: &Loop2) -> Option<(Vec2, f64)> {
    if lp.segs.len() > 200_000 {
        return None;
    }
    let &Seg2::Arc {
        center,
        radius,
        start: first_start,
        sweep: first_sweep,
    } = lp.segs.first()?
    else {
        return None;
    };
    if !center.is_finite() || !radius.is_finite() || radius <= 0.0 {
        return None;
    }
    let (mut anchor_index, mut anchor) = (0, first_start);
    for (index, seg) in lp.segs.iter().enumerate() {
        let &Seg2::Arc {
            center: c,
            radius: r,
            start,
            sweep,
        } = seg
        else {
            return None;
        };
        if c != center
            || r != radius
            || !start.is_finite()
            || !sweep.is_finite()
            || sweep == 0.0
            || sweep.abs() > TAU
            || sweep.is_sign_positive() != first_sweep.is_sign_positive()
            || start + sweep == start
        {
            return None;
        }
        if start < anchor {
            (anchor_index, anchor) = (index, start);
        }
    }
    // Fix one stored angle instead of independently widening both sides of every
    // joint. All starts must fit this same accumulated phase. Choosing the smallest
    // start makes the anchor independent of cyclic permutations of the segment list.
    let anchor = AngleResolution {
        value: anchor,
        radius: 0.0,
    };
    let mut total = AngleResolution {
        value: 0.0,
        radius: 0.0,
    };
    for (index, seg) in lp
        .segs
        .iter()
        .cycle()
        .skip(anchor_index)
        .take(lp.segs.len())
        .enumerate()
    {
        let &Seg2::Arc { start, sweep, .. } = seg else {
            return None;
        };
        let actual = if index == 0 {
            AngleResolution {
                value: 0.0,
                radius: 0.0,
            }
        } else {
            AngleResolution::new(start).sub(anchor)
        };
        let gap = actual.sub(total);
        // Stored angles may use different turns. The selected integer must still
        // distinguish adjacent turns; its multiplication carries its own roundoff.
        let turns = (gap.value / TAU).round();
        if !turns.is_finite() || turns + 1.0 == turns || turns - 1.0 == turns {
            return None;
        }
        let residual = gap.sub(AngleResolution::new(TAU).times_integer(turns));
        if residual.radius >= sweep.abs() * 0.5 || !residual.contains_zero() {
            return None;
        }
        total = total.add(AngleResolution::new(sweep));
    }
    let residual = total.sub(AngleResolution::new(TAU.copysign(first_sweep)));
    (residual.radius < TAU * 0.5 && residual.contains_zero()).then_some((center, radius))
}

/// Conservatively recognize one turn of an ellipse from rational quadratic pieces.
/// Four quarter pieces retain their common-parallelogram recognition rules. Other
/// partitions require exact stored joins and reconstruct one fixed returned ellipse:
/// shared vertices and tangent controls must fit componentwise cells no larger than
/// half an ULP of the original vertices' local semiextent; required weights must lie
/// strictly inside their stored rounding cells. This is local reconstruction, not
/// exact identity of the original binary rational curves. No controls are changed.
/// General weights outside `1e-9 < w < 1-1e-6`, unresolved arithmetic, gaps, overlaps
/// and mixed/repeated turns return `None`, so callers can preserve the original pieces.
pub fn loop_ellipse(lp: &Loop2) -> Option<(Vec2, Vec2, f64)> {
    if lp.segs.len() > 200_000 {
        return None;
    }
    if lp.segs.len() == 4
        && lp.segs.iter().all(
            |seg| matches!(seg, Seg2::Conic { w, .. } if *w == std::f64::consts::FRAC_1_SQRT_2),
        )
    {
        // Preserve both acceptance and rejection of this already-supported case.
        return loop_as_quarter_ellipse(lp);
    }
    loop_as_general_ellipse(lp)
}

/// The whole outline is one round wall only when every edge was admitted on
/// the same cylindrical support by `outline_segs`. Recover the directed winding
/// from those ordered points, rather than using a long sum of rounded angles.
/// A monotone polar walk with winding +/-1 rejects retraces and multiple turns.
/// This producer-owned certificate never loosens `loop_circle`'s rules.
pub fn certified_round_wall_turn(pts: &[Vec2], center: Vec2) -> Option<f64> {
    if pts.len() < 3 || !center.is_finite() {
        return None;
    }
    let mut direction = 0i64;
    let mut winding = 0i64;
    for i in 0..pts.len() {
        let (Some(a), Some(b)) = (
            pts.get(i),
            pts.get(if i + 1 == pts.len() { 0 } else { i + 1 }),
        ) else {
            return None;
        };
        let (a, b) = (*a - center, *b - center);
        if !a.is_finite() || !b.is_finite() {
            return None;
        }
        let angle = (b.angle() - a.angle() + PI).rem_euclid(TAU) - PI;
        if !angle.is_finite() || angle == 0.0 || angle.abs() >= PI {
            return None;
        }
        let sign = if angle > 0.0 { 1 } else { -1 };
        if direction != 0 && direction != sign {
            return None;
        }
        direction = sign;
        let cross = a.cross(b);
        if !cross.is_finite() {
            return None;
        }
        if a.y <= 0.0 && b.y > 0.0 && cross > 0.0 {
            winding = winding.checked_add(1)?;
        } else if a.y > 0.0 && b.y <= 0.0 && cross < 0.0 {
            winding = winding.checked_sub(1)?;
        }
    }
    (direction != 0 && winding == direction).then_some(TAU * direction as f64)
}
