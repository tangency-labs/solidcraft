//! The Roll-over's creases: where its torus meets the cliff faces continued, as explicit
//! graphs over the cliff's profile, fitted by cubic Bézier pieces and certified by
//! second-order Taylor enclosures in rounded interval arithmetic.
//!
//! Upstream: none (SMLib has no Roll-over corner).
//!
//! The torus is the concave ball of radius `r2` rolled over the convex round's section, a
//! circle of radius `r1` about the round's axis A1: its tube's centre circle has radius
//! `R = r1 + r2` about A1, in the plane `y0` (the balls' centres, `r2` short of the end
//! face E). In the frame of A1 a torus point is `(rho cos t, rho sin t, h)` with
//! `rho = R - r2 cos p` and `h = r2 sin p`, so `h^2 = r2^2 - (R - rho)^2 =
//! (rho - r1)(r2 + R - rho)`: the height over the centres' plane depends on the distance
//! from A1 alone. The cliff faces are planes and cylinders parallel to A1, so each is its
//! profile `G` in the plane square to A1, and their crease with the torus is the graph
//! `h(rho(q))` over `q` in `G`, for `r1 <= rho <= R` (the meridian from the round's
//! contact, `rho = r1`, to the end face's, `rho = R`).
//!
//! Where a cliff face touches the round (the round's other face, tangent to it along its
//! rail), `rho - r1` vanishes to second order at the tangent point qA and the crease's
//! dihedral angle fades to zero there. `rho^2 - r1^2` is then `kappa |q - qA|^2` (a line
//! tangent at qA: `kappa = 1`, by Pythagoras; a circle of radius `Rb` touching the
//! round's circle from outside, its centre `d = Rb + r1` from A1: `kappa = d / Rb`), so
//! `h = |q - qA| sqrt(kappa (r2 + R - rho) / (rho + r1))` is smooth through qA and is
//! enclosed without the square root's singularity.
//!
//! | Rust | What |
//! |------|------|
//! | [`Tay2`] | a value with its first and second derivative, each an [`Ivl`] |
//! | [`Piece`] | one piece of the cliff's profile: a segment, a segment from the tangent point, an arc from the tangent point |
//! | [`Piece::height`] | the crease's height `h`, times the arc's weight function on an arc |
//! | [`fit`] | the cubic Bézier pieces of that function, from its values and derivatives |
//! | [`certify`] | the certified bound of a fit's distance from the function |
//! | [`fit_certified`] | the fit refined until its bound is under a target, or refused |
//!
//! Deviations: none (not a port). Bounded work: at most [`MAX_LEVEL`] halvings of
//! [`SPANS_0`] spans, each span enclosed on [`BOXES`] boxes; no recursion.

use crate::blend::fillet_section::interval::{round_up, Ivl};
use crate::error::{Result, SolidError};
use crate::kernel::Kernel;

const OP: &str = "blend::blend_corner::rollover::crease";

/// The spans a fit starts with.
pub const SPANS_0: usize = 8;
/// The halvings of the spans a fit may take (`SPANS_0 * 2^MAX_LEVEL` spans at most).
pub const MAX_LEVEL: u32 = 6;
/// The boxes each span is enclosed on.
pub const BOXES: usize = 16;

/// A value with its first and second derivatives in one variable, each enclosed by an
/// interval: the second-order Taylor enclosure of a function over a box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tay2 {
    /// The value.
    pub v: Ivl,
    /// The first derivative.
    pub d: Ivl,
    /// The second derivative.
    pub dd: Ivl,
}

impl Tay2 {
    /// The constant `c`.
    #[must_use]
    pub fn constant(c: f64) -> Self {
        Self {
            v: Ivl::from(c),
            d: Ivl::from(0.0),
            dd: Ivl::from(0.0),
        }
    }

    /// The variable over the box `x`.
    #[must_use]
    pub fn var(x: Ivl) -> Self {
        Self {
            v: x,
            d: Ivl::from(1.0),
            dd: Ivl::from(0.0),
        }
    }

    /// `a x + b` of the variable `x`, with `a`, `b` exact.
    #[must_use]
    pub fn affine(x: Ivl, a: f64, b: f64) -> Self {
        Self {
            v: x * Ivl::from(a) + Ivl::from(b),
            d: Ivl::from(a),
            dd: Ivl::from(0.0),
        }
    }

    /// The sum.
    #[must_use]
    pub fn plus(self, o: Self) -> Self {
        Self {
            v: self.v + o.v,
            d: self.d + o.d,
            dd: self.dd + o.dd,
        }
    }

    /// The difference.
    #[must_use]
    pub fn minus(self, o: Self) -> Self {
        Self {
            v: self.v - o.v,
            d: self.d - o.d,
            dd: self.dd - o.dd,
        }
    }

    /// The product.
    #[must_use]
    pub fn times(self, o: Self) -> Self {
        Self {
            v: self.v * o.v,
            d: self.d * o.v + self.v * o.d,
            dd: self.dd * o.v + Ivl::from(2.0) * self.d * o.d + self.v * o.dd,
        }
    }

    /// The product with the exact `c`.
    #[must_use]
    pub fn scale(self, c: f64) -> Self {
        let c = Ivl::from(c);
        Self {
            v: self.v * c,
            d: self.d * c,
            dd: self.dd * c,
        }
    }

    /// The quotient; the whole line where the divisor may vanish.
    #[must_use]
    pub fn over(self, o: Self) -> Self {
        let v = self.v / o.v;
        let d = (self.d - v * o.d) / o.v;
        let dd = (self.dd - Ivl::from(2.0) * d * o.d - v * o.dd) / o.v;
        Self { v, d, dd }
    }

    /// The square root of a positive function; the whole line where it may not be.
    #[must_use]
    pub fn sqrt(self) -> Self {
        if !self.v.is_positive() {
            return Self {
                v: Ivl::whole(),
                d: Ivl::whole(),
                dd: Ivl::whole(),
            };
        }
        let v = self.v.sqrt();
        let two_v = Ivl::from(2.0) * v;
        let d = self.d / two_v;
        let dd = (self.dd - Ivl::from(2.0) * d * d) / two_v;
        Self { v, d, dd }
    }
}

/// The torus's radii: the convex round's `r1`, the concave ball's `r2`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Radii {
    /// The convex round's radius.
    pub r1: f64,
    /// The concave round's radius.
    pub r2: f64,
}

impl Radii {
    /// `R = r1 + r2`, the radius of the tube's centre circle.
    #[must_use]
    pub fn major(self) -> f64 {
        self.r1 + self.r2
    }
}

/// One piece of a cliff face's profile in the plane square to A1, in coordinates about
/// A1, parameterised over `[0, 1]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Piece {
    /// The segment from `a` to `b` on a line clear of the round's circle.
    Segment {
        /// The start.
        a: [f64; 2],
        /// The end.
        b: [f64; 2],
    },
    /// The segment from the tangent point `a` (on the round's circle, the line tangent to
    /// it there) to `b`.
    Tangent {
        /// The tangent point.
        a: [f64; 2],
        /// The end.
        b: [f64; 2],
    },
    /// The circular arc from the tangent point `a` to `b`, the rational quadratic with
    /// middle control point `p1` of weight `w1` (positive); the circle touches the
    /// round's circle at `a` from outside, `kappa = d / Rb`.
    Arc {
        /// The tangent point.
        a: [f64; 2],
        /// The middle control point.
        p1: [f64; 2],
        /// The end.
        b: [f64; 2],
        /// The middle weight.
        w1: f64,
        /// `d / Rb`: the circle's centre's distance from A1 over its radius.
        kappa: f64,
    },
}

fn tay2_pair(x: Tay2, y: Tay2) -> [Tay2; 2] {
    [x, y]
}

impl Piece {
    /// Every number finite, the arc's weight and `kappa` positive.
    #[must_use]
    pub fn is_finite(&self) -> bool {
        let ok = |p: &[f64; 2]| p.iter().all(|c| c.is_finite());
        match self {
            Self::Segment { a, b } | Self::Tangent { a, b } => ok(a) && ok(b),
            Self::Arc {
                a,
                p1,
                b,
                w1,
                kappa,
            } => ok(a) && ok(p1) && ok(b) && *w1 > 0.0 && *kappa > 0.0 && kappa.is_finite(),
        }
    }

    /// The weight function `W(s)` (1 off an arc) and the homogeneous point `W q` of the
    /// profile at `s`.
    fn homogeneous(&self, s: Tay2) -> (Tay2, [Tay2; 2]) {
        let one = Tay2::constant(1.0);
        match *self {
            Self::Segment { a, b } | Self::Tangent { a, b } => {
                let q = |i: usize| {
                    let (ai, bi) = (
                        a.get(i).copied().unwrap_or(0.0),
                        b.get(i).copied().unwrap_or(0.0),
                    );
                    Tay2::constant(ai).plus(s.scale(bi - ai))
                };
                (one, tay2_pair(q(0), q(1)))
            }
            Self::Arc { a, p1, b, w1, .. } => {
                let u = one.minus(s);
                let b0 = u.times(u);
                let b1 = u.times(s).scale(2.0);
                let b2 = s.times(s);
                let w = b0.plus(b1.scale(w1)).plus(b2);
                let q = |i: usize| {
                    let at = |p: [f64; 2]| p.get(i).copied().unwrap_or(0.0);
                    b0.scale(at(a))
                        .plus(b1.scale(w1 * at(p1)))
                        .plus(b2.scale(at(b)))
                };
                (w, tay2_pair(q(0), q(1)))
            }
        }
    }

    /// The function a fit approximates at `s`: the crease's height `h` over the balls'
    /// centres' plane, times the weight function `W` on an arc (so that the fit is the
    /// homogeneous coordinate of a rational curve with the arc's weights).
    #[must_use]
    pub fn height(&self, radii: Radii, s: Tay2) -> Tay2 {
        let (w, [hx, hy]) = self.homogeneous(s);
        let (qx, qy) = (hx.over(w), hy.over(w));
        let rho2 = qx.times(qx).plus(qy.times(qy));
        let rho = rho2.sqrt();
        let (r1, r2, big_r) = (radii.r1, radii.r2, radii.major());
        // sqrt(rho^2 - r1^2), smooth through a tangent point
        let root_g = match *self {
            Self::Segment { .. } => rho2.minus(Tay2::constant(r1 * r1)).sqrt(),
            Self::Tangent { a, b } => {
                let len = ((b[0] - a[0]) * (b[0] - a[0]) + (b[1] - a[1]) * (b[1] - a[1])).sqrt();
                s.scale(len)
            }
            Self::Arc {
                a,
                p1,
                b,
                w1,
                kappa,
            } => {
                // W (q - qA) = s (2 (1 - s) w1 (p1 - a) + s (b - a))
                let u = Tay2::constant(1.0).minus(s);
                let comp = |i: usize| {
                    let at = |p: [f64; 2]| p.get(i).copied().unwrap_or(0.0);
                    u.scale(2.0 * w1 * (at(p1) - at(a)))
                        .plus(s.scale(at(b) - at(a)))
                };
                let (cx, cy) = (comp(0), comp(1));
                let n = cx.times(cx).plus(cy.times(cy)).sqrt();
                s.times(n).over(w).scale(kappa.sqrt())
            }
        };
        let ratio = Tay2::constant(r2 + big_r)
            .minus(rho)
            .over(rho.plus(Tay2::constant(r1)));
        let h = root_g.times(ratio.sqrt());
        match self {
            Self::Arc { .. } => h.times(w),
            _ => h,
        }
    }
}

/// A fit: the cubic Bézier pieces of a function over `[0, 1]`, `spans` equal spans, each
/// span's four control values.
#[derive(Clone, Debug, PartialEq)]
pub struct Fit {
    /// The control values, four per span.
    pub control: Vec<[f64; 4]>,
}

impl Fit {
    /// The number of spans.
    #[must_use]
    pub fn spans(&self) -> usize {
        self.control.len()
    }

    /// The span's ends.
    #[must_use]
    pub fn span(&self, i: usize) -> (f64, f64) {
        #[allow(clippy::cast_precision_loss)] // span counts are small
        let n = self.control.len().max(1) as f64;
        #[allow(clippy::cast_precision_loss)]
        let i = i as f64;
        (i / n, (i + 1.0) / n)
    }

    /// The fit's second derivative over the box `[m - r, m + r]` in span `i`: its value at
    /// `m` plus the span's constant third derivative times `[-r, r]` (a cubic's Taylor
    /// expansion, exact but for its rounding, which the intervals carry). The natural
    /// interval extension of the Bernstein form would not shrink with the box.
    fn second_over(&self, i: usize, m: f64, r: f64) -> Ivl {
        let (a, b) = self.span(i);
        let width = Ivl::from(b) - Ivl::from(a);
        let c = self.control.get(i).copied().unwrap_or([0.0; 4]);
        let at_m = self.eval(i, Tay2::var(Ivl::from(m))).dd;
        // d3/du3 of the cubic Bezier: 6 (c3 - 3 c2 + 3 c1 - c0), over width^3 in s
        let third = Ivl::from(6.0)
            * (Ivl::from(c[3]) - Ivl::from(3.0) * Ivl::from(c[2])
                + Ivl::from(3.0) * Ivl::from(c[1])
                - Ivl::from(c[0]))
            / (width * width * width);
        at_m + third * Ivl::new(-r, r)
    }

    /// The fit's value at `s` in span `i`, as a [`Tay2`] of `s`.
    fn eval(&self, i: usize, s: Tay2) -> Tay2 {
        let (a, b) = self.span(i);
        let width = b - a;
        let u = s.minus(Tay2::constant(a)).scale(1.0 / width);
        let c = self.control.get(i).copied().unwrap_or([0.0; 4]);
        let v = Tay2::constant(1.0).minus(u);
        let b0 = v.times(v).times(v);
        let b1 = v.times(v).times(u).scale(3.0);
        let b2 = v.times(u).times(u).scale(3.0);
        let b3 = u.times(u).times(u);
        b0.scale(c[0])
            .plus(b1.scale(c[1]))
            .plus(b2.scale(c[2]))
            .plus(b3.scale(c[3]))
    }
}

/// The cubic Hermite fit of `piece`'s [`Piece::height`] on `spans` equal spans: each span's
/// control values from the function's value and derivative at its ends (taken at the
/// enclosures' middles: the fit needs no rigour, its [`certify`] does).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for no spans; [`SolidError::Unsupported`] where the
/// function is not finite at a knot (outside the torus's meridian, or an input not
/// finite).
pub fn fit(piece: &Piece, radii: Radii, spans: usize) -> Result<Fit> {
    if spans == 0 {
        return Err(SolidError::InvalidInput {
            operation: OP,
            reason: "a fit of no spans",
        });
    }
    let mut knots = Vec::new();
    for i in 0..=spans {
        #[allow(clippy::cast_precision_loss)]
        let s = i as f64 / spans as f64;
        let t = piece.height(radii, Tay2::var(Ivl::from(s)));
        let (f, d) = (t.v.mid(), t.d.mid());
        if !(f.is_finite() && d.is_finite()) {
            return Err(SolidError::Unsupported {
                operation: OP,
                reason: "the crease's height is not finite at a knot of its fit",
            });
        }
        knots.push((s, f, d));
    }
    let mut control = Vec::new();
    for pair in knots.windows(2) {
        let [(sa, fa, da), (sb, fb, db)] = *pair else {
            continue;
        };
        let w = sb - sa;
        control.push([fa, fa + w * da / 3.0, fb - w * db / 3.0, fb]);
    }
    Ok(Fit { control })
}

/// The certified bound of `|fit - h|` over `[0, 1]`, `h` the piece's [`Piece::height`]: on
/// each of [`BOXES`] boxes `J` of each span, with `m` its middle and `r` its half width,
/// `|e(m)| + |e'(m)| r + max |e''(J)| r^2 / 2`, `e = fit - h` enclosed in rounded interval
/// arithmetic, summed rounded up. Infinite where an enclosure is not finite (a NaN, a
/// box the function is undefined on).
#[must_use]
pub fn certify(piece: &Piece, radii: Radii, fit: &Fit) -> f64 {
    let mut worst = 0.0_f64;
    for i in 0..fit.spans() {
        let (a, b) = fit.span(i);
        #[allow(clippy::cast_precision_loss)]
        let step = (b - a) / BOXES as f64;
        for j in 0..BOXES {
            #[allow(clippy::cast_precision_loss)]
            let lo = a + step * j as f64;
            let hi = if j + 1 == BOXES { b } else { lo + step };
            let m = 0.5 * (lo + hi);
            let r = round_up(round_up(hi - m).max(round_up(m - lo)));
            let at_m = Tay2::var(Ivl::from(m));
            let em = fit.eval(i, at_m).minus(piece.height(radii, at_m));
            let box_ = Tay2::var(Ivl::new(lo, hi));
            let ej_dd = fit.second_over(i, m, r) - piece.height(radii, box_).dd;
            if !(em.v.is_finite() && em.d.is_finite() && ej_dd.is_finite()) {
                return f64::INFINITY;
            }
            let bound = round_up(
                round_up(em.v.mag() + round_up(em.d.mag() * r))
                    + round_up(round_up(ej_dd.mag() * round_up(r * r)) * 0.5),
            );
            // a NaN bound is no bound: `max` would drop it
            if bound.is_nan() {
                return f64::INFINITY;
            }
            worst = worst.max(bound);
        }
    }
    worst
}

/// A fit of `piece` certified within `target`: [`SPANS_0`] spans, halved up to
/// [`MAX_LEVEL`] times until [`certify`]'s bound is at most `target`. Returns the fit and
/// its bound.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a piece or radii not finite or a target not positive;
/// [`SolidError::Unsupported`] where no fit within the levels is certified within
/// `target`; those of [`fit`]; the kernel's cancellation.
pub fn fit_certified(k: &Kernel, piece: &Piece, radii: Radii, target: f64) -> Result<(Fit, f64)> {
    let radii_ok = radii.r1 > 0.0 && radii.r2 > 0.0 && radii.r1.is_finite() && radii.r2.is_finite();
    if !(piece.is_finite() && radii_ok && target > 0.0 && target.is_finite()) {
        return Err(SolidError::InvalidInput {
            operation: OP,
            reason: "a crease piece, radii or target not finite and positive",
        });
    }
    let mut spans = SPANS_0;
    for _ in 0..=MAX_LEVEL {
        k.check_cancelled(OP)?;
        let f = fit(piece, radii, spans)?;
        let bound = certify(piece, radii, &f);
        if bound <= target {
            return Ok((f, bound));
        }
        spans = spans.saturating_mul(2);
    }
    Err(SolidError::Unsupported {
        operation: OP,
        reason: "the crease's fit is not certified within the tolerance",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNIT: Radii = Radii { r1: 1.0, r2: 1.0 };

    /// The planar model's cliff, the plane u = 0, in coordinates about A1 (up, side):
    /// A1 is at u = 1, w = -1, so the plane is the line side = 1, from the tangent point
    /// (0, 1) (w = -1) up to where the rail leaves the end face, rho = 2: up = sqrt 3.
    fn planar() -> Piece {
        Piece::Tangent {
            a: [0.0, 1.0],
            b: [3f64.sqrt(), 1.0],
        }
    }

    #[test]
    fn the_planar_crease_is_its_closed_form() {
        // sin t (2 - cos p) = 1 on the torus's (t, p): up = rho cos t, side = rho sin t
        // = 1 with rho = 2 - cos p, h = sin p
        let p = planar();
        for i in 0..=20 {
            let s = f64::from(i) / 20.0;
            let h = p.height(UNIT, Tay2::var(Ivl::from(s))).v.mid();
            let up = s * 3f64.sqrt();
            let rho = (up * up + 1.0).sqrt();
            let cos_p = 2.0 - rho;
            let sin_t = 1.0 / rho;
            assert!((sin_t * (2.0 - cos_p) - 1.0).abs() < 1e-15);
            assert!(
                (h - (1.0 - cos_p * cos_p).max(0.0).sqrt()).abs() < 1e-13,
                "{s}: {h}"
            );
        }
    }

    #[test]
    fn a_fit_is_certified_and_the_bound_holds() {
        let k = Kernel::default();
        let (f, bound) = fit_certified(&k, &planar(), UNIT, 1e-10).unwrap();
        assert!(bound <= 1e-10);
        // the bound is an enclosure: no sample may pass it
        for i in 0..f.spans() {
            let (a, b) = f.span(i);
            for j in 0..=10 {
                let s = a + (b - a) * f64::from(j) / 10.0;
                let x = Tay2::var(Ivl::from(s));
                let e = f.eval(i, x).minus(planar().height(UNIT, x)).v.mag();
                assert!(e <= bound, "{e} > {bound}");
            }
        }
    }

    #[test]
    fn an_arc_from_the_tangent_point_is_smooth_there() {
        // the ledge's bore: centre (0, 33) and radius 12 in (x, z); A1 = (sqrt 156, 32.5)
        // r1 = r2 = 0.5, axes up = +z, side = -x
        let a1 = (156f64.sqrt(), 32.5);
        let to_local = |x: f64, z: f64| [z - a1.1, a1.0 - x];
        let cb = (0.0, 33.0);
        let rb = 12.0;
        let d = ((a1.0 - cb.0).powi(2) + (a1.1 - cb.1).powi(2)).sqrt();
        assert!((d - 12.5).abs() < 1e-12);
        let ua = ((a1.0 - cb.0) / d, (a1.1 - cb.1) / d);
        let qa = (cb.0 + rb * ua.0, cb.1 + rb * ua.1);
        // to D = (12, 33): the arc's angles about the bore's centre
        let (ta, tb) = (ua.1.atan2(ua.0), 0.0_f64);
        let half = 0.5 * (tb - ta);
        let w1 = half.cos();
        let mid = 0.5 * (ta + tb);
        let p1 = (cb.0 + rb / w1 * mid.cos(), cb.1 + rb / w1 * mid.sin());
        let piece = Piece::Arc {
            a: to_local(qa.0, qa.1),
            p1: to_local(p1.0, p1.1),
            b: to_local(12.0, 33.0),
            w1,
            kappa: d / rb,
        };
        let radii = Radii { r1: 0.5, r2: 0.5 };
        let k = Kernel::default();
        let (_, bound) = fit_certified(&k, &piece, radii, 1e-10).unwrap();
        assert!(bound <= 1e-10);
        // h = 0 at the tangent point; at D on the seam, rho^2 = (12.49 - 12)^2 + 0.5^2
        let h0 = piece.height(radii, Tay2::var(Ivl::from(0.0))).v.mid();
        assert!(h0.abs() < 1e-15);
        let h1 = piece.height(radii, Tay2::var(Ivl::from(1.0))).v.mid();
        let rho = ((a1.0 - 12.0).powi(2) + 0.25_f64).sqrt();
        let want = ((rho - 0.5) * (0.5 + 1.0 - rho)).sqrt();
        assert!((h1 - want).abs() < 1e-14, "{h1} {want}");
    }

    #[test]
    fn nan_refuses() {
        let k = Kernel::default();
        let bad = Piece::Segment {
            a: [f64::NAN, 1.0],
            b: [1.0, 1.0],
        };
        assert!(fit_certified(&k, &bad, UNIT, 1e-9).is_err());
        assert!(fit_certified(&k, &planar(), UNIT, f64::NAN).is_err());
        let r = Radii {
            r1: f64::NAN,
            r2: 1.0,
        };
        assert!(fit_certified(&k, &planar(), r, 1e-9).is_err());
    }

    #[test]
    fn a_piece_past_the_end_face_is_not_certified() {
        // rho > R past the end face's contact: the height is undefined there
        let k = Kernel::default();
        let past = Piece::Tangent {
            a: [0.0, 1.0],
            b: [3.0, 1.0],
        };
        assert!(fit_certified(&k, &past, UNIT, 1e-9).is_err());
    }
}
