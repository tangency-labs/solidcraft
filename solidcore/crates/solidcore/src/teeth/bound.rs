//! Rounded enclosures for the tooth module's certificates.
//!
//! Nothing here trusts the platform's `sin`, `cos`, `atan` or `hypot`: Rust leaves their precision
//! unspecified, and it differs from one target to the next. A sine, a cosine and an arc tangent
//! are enclosed from their series ([`sin_cos_at`], [`atan_at`]) and a length from its square root,
//! with the operations IEEE 754 rounds correctly (`+ - * /` and the square root), each an `Ivl`
//! operation widened outward. Where a curve's points are computed with the platform's functions,
//! as the tooth curves' are, each computed point is measured against the enclosure of the true
//! one ([`off`]): the platform's functions only nominate a point, and its distance from the curve
//! is proved.
//!
//! An interval here is the kernel's [`Ivl`]: its `+ - * /` and `sqrt` are widened outward past
//! their rounding, so each holds the exact result for every number of its operands, and anything
//! undefined (a NaN, a division by an interval that holds 0) is the whole line, which no test
//! passes. A single number enters an interval only through [`exact`], and an interval is negated
//! only through [`neg`].
//!
//! Not upstream: SolidCraft's tooth outlines (see [`crate::teeth`]). No recursion.

use crate::blend::fillet_section::interval::{round_down, round_up, Ivl};
use crate::geom::certified::next_up;
use crate::teeth::outline::Point;

/// The point interval `[x, x]` (`Ivl::new(x, x)`); the whole line if `x` is not finite. The only
/// way a number enters an interval in the tooth module. Counts nothing: no operation is done.
pub(crate) fn exact(x: f64) -> Ivl {
    if x.is_finite() {
        Ivl::new(x, x)
    } else {
        Ivl::whole()
    }
}

/// `-x`, exactly: `[-x.hi, -x.lo]`. The only way an interval is negated (`Ivl` has no `Neg`). A
/// change of sign is exact, so nothing is counted.
pub(crate) fn neg(x: Ivl) -> Ivl {
    Ivl::new(-x.hi, -x.lo)
}

/// `x` squared, never negative. With `a = min(|lo|, |hi|)` and `b = max(|lo|, |hi|)`: when `x`
/// holds 0 (`lo <= 0 <= hi`) the lower end is exactly 0.0; otherwise it is
/// `max(0.0, round_down(a * a))` (the absolute part of about 1e-300 in `round_down` would make
/// `round_down(0.0)`, or that of a tiny square, negative, which a square never is). The upper end
/// is `round_up(b * b)`. The whole line for a non-finite `x`. Counted: the two products, each
/// moved outward by `round_down` / `round_up`, which cover one rounding of each.
pub(crate) fn sqr(x: Ivl) -> Ivl {
    if !(x.lo.is_finite() && x.hi.is_finite() && x.lo <= x.hi) {
        return Ivl::whole();
    }
    let (a, b) = (x.lo.abs().min(x.hi.abs()), x.lo.abs().max(x.hi.abs()));
    let lo = if x.lo <= 0.0 && x.hi >= 0.0 {
        0.0
    } else {
        round_down(a * a).max(0.0)
    };
    Ivl::new(lo, round_up(b * b))
}

/// The largest size of an angle whose sine and cosine [`sin_cos_at`] encloses more closely than
/// `[-1, 1]`: `2^20`. The quarter turns taken off an angle are known only as closely as pi is, so
/// the enclosure widens with their count; no tooth has an angle near this.
const MOST_ANGLE: f64 = 1_048_576.0;

/// How many terms after the first the series of [`sin_cos_at`] take.
const SINE_TERMS: u32 = 10;

/// What the series of [`sin_cos_at`] leave out, for an angle of size at most 1: the first term
/// left out bounds it (the terms alternate and shrink), `1 / 22!` for the cosine and less for the
/// sine, below this.
const SINE_REST: f64 = 1e-21;

/// How many terms after the first the series of [`atan_at`] takes.
const ATAN_TERMS: u32 = 13;

/// What the series of [`atan_at`] leaves out, for a number of size at most 1/4: the first term
/// left out, `(1/4)^29 / 29`, below this.
const ATAN_REST: f64 = 1.5e-19;

/// The sine and the cosine of the one angle `x`, enclosed, with nothing taken from the platform's
/// functions.
///
/// The nearest whole count `k` of quarter turns is taken off: `t = x - k pi/2`, in intervals with
/// pi enclosed ([`pi`]), which is true for any whole `k` (the nearest only keeps `t` small). Then,
/// with `y = t^2`, `sin t = t (1 - y/(2 3) (1 - y/(4 5) (1 - ...)))` and
/// `cos t = 1 - y/(1 2) (1 - y/(3 4) (1 - ...))`, each cut after [`SINE_TERMS`] terms, with
/// [`SINE_REST`] either way for the rest (the terms alternate and shrink for `|t| <= 1`, so the
/// first one left out bounds what is). The quarter turns are put back by `k mod 4`. An angle
/// that is not a number, or larger than [`MOST_ANGLE`], gives `([-1, 1], [-1, 1])`.
///
/// Counted: every operation is an `Ivl` one, widened outward (the divisors are whole numbers
/// below 500, exact in doubles); the series' remainders are added as intervals.
pub(crate) fn sin_cos_at(x: f64) -> (Ivl, Ivl) {
    let any = Ivl::new(-1.0, 1.0);
    if !(x.abs() <= MOST_ANGLE) {
        return (any, any);
    }
    let quarters = (x / std::f64::consts::FRAC_PI_2).round();
    let t = exact(x) - exact(quarters) * (pi() * exact(0.5));
    if !(t.mag() <= 1.0) {
        return (any, any);
    }
    let (one, y) = (exact(1.0), sqr(t));
    let (mut sine, mut cosine) = (one, one);
    for k in (1..=SINE_TERMS).rev() {
        let k = f64::from(k);
        sine = one - y / exact(2.0 * k * (2.0 * k + 1.0)) * sine;
        cosine = one - y / exact((2.0 * k - 1.0) * 2.0 * k) * cosine;
    }
    let rest = Ivl::new(-SINE_REST, SINE_REST);
    let (s, c) = (t * sine + rest, cosine + rest);
    // A whole number of at most 2^20 in size, so the cast is exact; its two lowest bits are the
    // quarter turns left over, for a negative count too.
    let (s, c) = match (quarters as i64) & 3 {
        0 => (s, c),
        1 => (c, neg(s)),
        2 => (neg(s), neg(c)),
        _ => (neg(c), s),
    };
    (cut_to_one(s), cut_to_one(c))
}

/// The arc tangent of the one number `x`, enclosed, with nothing taken from the platform's
/// functions.
///
/// A number larger than 1 in size is turned over, `atan x = +-pi/2 - atan(1/x)`. Then the angle
/// is halved twice, `atan t = 2 atan(t / (1 + sqrt(1 + t^2)))`, which leaves a number of size at
/// most `tan(pi/16) < 1/4`, and with `y = u^2`, `atan u = u (1 - y/3 + y^2/5 - ...)`, cut after
/// [`ATAN_TERMS`] terms, with [`ATAN_REST`] either way for the rest (the terms alternate and
/// shrink). An infinite `x` gives the enclosure of a quarter turn, a NaN the whole line.
///
/// Counted: every operation is an `Ivl` one, widened outward, the square roots among them; the
/// series' remainder is added as an interval.
pub(crate) fn atan_at(x: f64) -> Ivl {
    if x.is_nan() {
        return Ivl::whole();
    }
    let (one, quarter_turn) = (exact(1.0), pi() * exact(0.5));
    if x.is_infinite() {
        return if x > 0.0 {
            quarter_turn
        } else {
            neg(quarter_turn)
        };
    }
    let turned = x.abs() > 1.0;
    let t = if turned { one / exact(x) } else { exact(x) };
    let halved = |t: Ivl| t / (one + (one + sqr(t)).sqrt());
    let u = halved(halved(t));
    if !(u.mag() <= 0.25) {
        return Ivl::whole();
    }
    let y = sqr(u);
    let mut series = one / exact(f64::from(2 * ATAN_TERMS + 1));
    for k in (0..ATAN_TERMS).rev() {
        series = one / exact(f64::from(2 * k + 1)) - y * series;
    }
    let inner = exact(4.0) * (u * series + Ivl::new(-ATAN_REST, ATAN_REST));
    if !turned {
        inner
    } else if x > 0.0 {
        quarter_turn - inner
    } else {
        neg(quarter_turn) - inner
    }
}

/// An upper bound of the distance from the computed point `p` to every point of the box `held`,
/// which holds the true point: how far the computed point can be from the true one. Infinite
/// where a coordinate is not a number. Counted: the two differences, the squares, the sum and the
/// root as `Ivl` operations ([`magnitude`]).
pub(crate) fn off(p: Point, held: [Ivl; 2]) -> f64 {
    let ([x, y], [hx, hy]) = (p, held);
    magnitude([exact(x) - hx, exact(y) - hy])
}

/// An upper bound of the distance between two points, with nothing taken from the platform's
/// `hypot`. Counted as [`off`].
pub(crate) fn apart(a: Point, b: Point) -> f64 {
    let [x, y] = b;
    off(a, [exact(x), exact(y)])
}

/// pi enclosed: `[PI, next_up(PI)]`. The double `PI` lies below pi, and the next double above it
/// lies above pi.
pub(crate) fn pi() -> Ivl {
    Ivl::new(std::f64::consts::PI, next_up(std::f64::consts::PI))
}

/// The middle `m = 0.5 (lo + hi)` of a stretch and a half width `r >= max(m - lo, hi - m)`,
/// rounded up (`r = next_up(max(m - lo, hi - m))`). Takes `lo <= hi`. Counted: the two
/// differences, each rounded to nearest (at most half a unit in the last place), covered by the
/// step up; the middle needs no count, as it is the centre the half width is measured from. An
/// infinite half width where a difference is not a number.
pub(crate) fn half_width(lo: f64, hi: f64) -> (f64, f64) {
    let m = 0.5 * (lo + hi);
    let below = m - lo;
    let above = hi - m;
    let r = if below.is_nan() || above.is_nan() {
        f64::INFINITY
    } else {
        next_up(below.max(above))
    };
    (m, r)
}

/// sin and cos over every angle of `angle`: with `(m, r) = half_width(angle.lo, angle.hi)` and
/// `(Sm, Cm)` the enclosures of the sine and cosine of the middle ([`sin_cos_at`]),
/// `S = Sm + Cm [-r, r] + [-q, q]`, `C = Cm + Sm [-r, r] + [-q, q]`,
/// `q = (exact(r) exact(r) exact(0.5)).hi` (Taylor, `|sin''|, |cos''| <= 1`), each then cut to
/// `[-1, 1]`. A non-finite angle gives `([-1, 1], [-1, 1])`. Counted: the middle's sine and
/// cosine as [`sin_cos_at`] encloses them; the rest as `Ivl` operations; the Taylor remainder
/// `r^2 / 2` rounded up.
pub(crate) fn sin_cos(angle: Ivl) -> (Ivl, Ivl) {
    let any = Ivl::new(-1.0, 1.0);
    if !angle.is_finite() {
        return (any, any);
    }
    let (m, r) = half_width(angle.lo, angle.hi);
    if !r.is_finite() {
        return (any, any);
    }
    let (s, c) = sin_cos_at(m);
    let spread = Ivl::new(-r, r);
    let q = (exact(r) * exact(r) * exact(0.5)).hi;
    let rest = Ivl::new(-q, q);
    let sine = s + c * spread + rest;
    let cosine = c + s * spread + rest;
    (cut_to_one(sine), cut_to_one(cosine))
}

/// An enclosure of a sine or a cosine cut to `[-1, 1]`, which holds every one of them.
fn cut_to_one(x: Ivl) -> Ivl {
    let lo = if x.lo > -1.0 { x.lo } else { -1.0 };
    let hi = if x.hi < 1.0 { x.hi } else { 1.0 };
    Ivl::new(lo, hi)
}

/// atan over every number of `x` (atan increases): from the lower end of the enclosure of
/// `atan(x.lo)` to the upper end of that of `atan(x.hi)` ([`atan_at`]); the whole line for NaN.
pub(crate) fn atan_over(x: Ivl) -> Ivl {
    if x.lo.is_nan() || x.hi.is_nan() {
        return Ivl::whole();
    }
    let (lo, hi) = (atan_at(x.lo).lo, atan_at(x.hi).hi);
    if lo <= hi {
        Ivl::new(lo, hi)
    } else {
        Ivl::whole()
    }
}

/// `R(angle) v` enclosed for every angle of `angle`: with `(S, C) = sin_cos(angle)`,
/// `[C v0 - S v1, S v0 + C v1]`. (Named `rotated_ivl` so that it never clashes with
/// `outline::rotated(p: Point, angle: f64)`, which involute.rs and fit.rs already import.)
/// Counted: [`sin_cos`], then four products and two sums as `Ivl` operations.
pub(crate) fn rotated_ivl(angle: Ivl, v: [Ivl; 2]) -> [Ivl; 2] {
    let (s, c) = sin_cos(angle);
    let [v0, v1] = v;
    [c * v0 - s * v1, s * v0 + c * v1]
}

/// An upper bound of `|v|` for every `v` of the box: `sqrt(mag(v0)^2 + mag(v1)^2)` in `Ivl`, its
/// upper end (infinite when not finite, or when an end of the box is not a number). Counted: the
/// squares, the sum and the root as `Ivl` operations.
pub(crate) fn magnitude(v: [Ivl; 2]) -> f64 {
    let [v0, v1] = v;
    if has_nan(v0) || has_nan(v1) {
        return f64::INFINITY;
    }
    let size = (sqr(exact(v0.mag())) + sqr(exact(v1.mag()))).sqrt().hi;
    if size.is_finite() {
        size
    } else {
        f64::INFINITY
    }
}

/// Whether an end of an interval is not a number. The `Ivl` operations never make one, but an
/// interval built from outside values can hold one, and `Ivl::mag` would then report the other
/// end's size.
fn has_nan(x: Ivl) -> bool {
    x.lo.is_nan() || x.hi.is_nan()
}

/// The second-order Taylor form over a stretch of half width `r` about its middle:
/// `value + slope [-r, r] + curvature [0, q]`, `q = (exact(r) exact(r) exact(0.5)).hi`. `value`
/// and `slope` are enclosures at the middle, `curvature` an enclosure of the second derivative
/// over the stretch. Counted: `q` rounded up, then two products and two sums as `Ivl` operations.
pub(crate) fn taylor(value: Ivl, slope: Ivl, curvature: Ivl, r: f64) -> Ivl {
    let q = (exact(r) * exact(r) * exact(0.5)).hi;
    value + slope * Ivl::new(-r, r) + curvature * Ivl::new(0.0, q)
}

/// The same form anchored at an end of a stretch of width `h` (`h` at least the width, rounded
/// up): from the start (forward) `value + slope [0, h] + curvature [0, q]`; from the end
/// (backward) `value + slope [-h, 0] + curvature [0, q]`; `q = (exact(h) exact(h) exact(0.5)).hi`.
/// Counted as [`taylor`].
pub(crate) fn taylor_from_end(
    value: Ivl,
    slope: Ivl,
    curvature: Ivl,
    h: f64,
    forward: bool,
) -> Ivl {
    let q = (exact(h) * exact(h) * exact(0.5)).hi;
    let step = if forward {
        Ivl::new(0.0, h)
    } else {
        Ivl::new(-h, 0.0)
    };
    value + slope * step + curvature * Ivl::new(0.0, q)
}

/// What a curve does over a stretch of its parameter: enclosures of its first and second derivative
/// vectors and a bound on the size of the second. How far a computed point of the curve lies from
/// the true one is not part of it: that is proved for each point by itself ([`off`]).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Bend {
    /// Encloses `P'(t)`, each coordinate, for every `t` of the stretch.
    pub(crate) first: [Ivl; 2],
    /// Encloses `P''(t)`, each coordinate, for every `t` of the stretch.
    pub(crate) second: [Ivl; 2],
    /// An upper bound of `|P''(t)|` over the stretch (may be tighter than `magnitude(second)`).
    pub(crate) second_size: f64,
}

impl Bend {
    /// Nothing known: whole intervals, an infinite size. Proves nothing.
    pub(crate) fn unknown() -> Bend {
        Bend {
            first: [Ivl::whole(), Ivl::whole()],
            second: [Ivl::whole(), Ivl::whole()],
            second_size: f64::INFINITY,
        }
    }
}

/// An upper bound of the Hausdorff distance (both ways, as sets) between the curve over a stretch
/// of parameter width at most `h` and the chord between its two computed end points `p0`, `p1`,
/// each within `e` of the true point at its end of the stretch (proved for the computed points
/// themselves, [`off`]). Infinite (never NaN) where nothing is proved.
///
/// Two bounds are formed and the smaller is taken; both hold. By size: `h^2/8` times the size of
/// `P''` (`Bend::second_size`). Across the chord: `h^2/8` times the size of the part of `P''`
/// across the chord, which holds where `P'` runs along the chord, or against it, on the whole
/// stretch (`along.lo > 0.0 || along.hi < 0.0`, so a curve sampled from a larger to a smaller
/// parameter has it too). It is what keeps a curve with a large `P''` along its own direction, as
/// an involute flank near its base circle has, from being cut far finer than it needs.
///
/// Proof. With `E = e`, `s` in `[0, 1]` and `L(s)` the chord between the exact end
/// points `P(t0)` and `P(t1)`, `|P(t0 + s h) - L(s)| <= h^2/8 max |P''|` (the Peano kernel of
/// linear interpolation, in vector form by the integral): the bound by size holds at matched
/// parameters, and so as sets. Where `P'.c` has one sign on the whole stretch (`c` the exact
/// chord; positive, or negative for a curve that runs against its chord, as when `sampled` is
/// called with `from > to`), the curve is a graph over the chord whose projection covers the chord
/// segment, so its distance from the segment is `|n.(P - P(t0))|` (`n` the unit normal of the
/// chord), a function that vanishes at both ends, bounded by `h^2/8 max |n.P''|`. The exact chord
/// lies in the box `C = (p1 - p0) + [-2E, 2E]^2`, so every interval below is taken over `C`; the
/// computed chord is within `E` of `L` everywhere. Hence the Hausdorff distance between the curve
/// over the stretch and the computed chord is at most the result. The bound across the chord does
/// NOT bound the distance at matched parameters (a point at the same parameter may lie farther
/// along the chord); nothing in the tooth module needs matched parameters from `sampled`. Where
/// the along test fails (near the involute's cusp on the base circle, where `P' = 0`) only the
/// bound by size is left.
///
/// Counted: `h^2/8` as `exact(h) exact(h) exact(0.125)` rounded outward; every other operation an
/// `Ivl` operation; the end points' rounding `E` added last and the sum rounded up.
pub(crate) fn chord_bound(h: f64, p0: Point, p1: Point, e: f64, bend: &Bend) -> f64 {
    if !(e >= 0.0) {
        return f64::INFINITY;
    }
    // An end point that is not a number proves nothing, whatever the bends are (the bound by size
    // never reads the end points).
    let [x0, y0] = p0;
    let [x1, y1] = p1;
    if !(x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite()) {
        return f64::INFINITY;
    }
    let hh = exact(h) * exact(h) * exact(0.125);
    let plain = (hh * exact(bend.second_size)).hi;
    let across = across_bound(hh, e, p0, p1, bend);
    let best = if across < plain { across } else { plain };
    let result = (exact(best) + exact(e)).hi;
    if result.is_nan() {
        f64::INFINITY
    } else {
        result
    }
}

/// The bound across the chord of [`chord_bound`], before the rounding of the end points is added:
/// `h^2/8` times the size of the component of `P''` across the chord, or infinite where `P'` does
/// not run along the chord (or against it) on the whole stretch, or where the chord may have no
/// length.
fn across_bound(hh: Ivl, e: f64, p0: Point, p1: Point, bend: &Bend) -> f64 {
    let wide = 2.0 * e;
    let slack = Ivl::new(-wide, wide);
    let ([x0, y0], [x1, y1]) = (p0, p1);
    let c0 = exact(x1) - exact(x0) + slack;
    let c1 = exact(y1) - exact(y0) + slack;
    let len = (sqr(c0) + sqr(c1)).sqrt();
    if !(len.lo > 0.0) {
        return f64::INFINITY;
    }
    let [f0, f1] = bend.first;
    let along = c0 * f0 + c1 * f1;
    if !(along.lo > 0.0 || along.hi < 0.0) {
        return f64::INFINITY;
    }
    let [s0, s1] = bend.second;
    let cross = (c0 * s1 - c1 * s0) / len;
    if has_nan(cross) {
        return f64::INFINITY;
    }
    let bound = (hh * exact(cross.mag())).hi;
    if bound.is_nan() {
        f64::INFINITY
    } else {
        bound
    }
}
