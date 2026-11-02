//! The elementary functions on intervals, each proven to hold its function's range.
//!
//! Four series carry the proofs, each on a small reduced argument, with a bound on what the
//! terms left out add up to:
//!
//! | series | used on | what is left out, at most |
//! |---|---|---|
//! | `expm1 r = Σ rⁿ/n!`, n ≥ 1 | `|r| ≤ ln 2 / 2` after `x = k·ln 2 + r` | `e^ρ ρᴺ⁺¹/(N+1)! ≤ 2ρᴺ⁺¹/(N+1)!` |
//! | `atanh z = Σ z²ⁿ⁺¹/(2n+1)` | `ln m = 2 atanh((m−1)/(m+1))`, `m ∈ [1/√2, √2]`, `|z| ≤ 0.172` | `|z|²ᴺ⁺¹/((2N+1)(1−z²))` |
//! | `sin r`, `cos r` (Taylor) | `|r| ≤ π/4` after `x = k·π/2 + r` | `ρᵐ/m!`, m the first degree left out |
//! | `atan z = Σ (−1)ⁿ z²ⁿ⁺¹/(2n+1)` | `|z| ≤ 0.1` after `1/x` and halving the angle | the first term left out (alternating) |
//!
//! Every step runs in [`Interval`]s, so rounding is accounted for, and the reduction constants
//! are themselves enclosed ([`super::consts`]). Everything else is built from these with
//! `+ − × ÷` and `√`. A function's range over an interval comes from where it is monotone: the
//! values at the two ends, and at any critical point inside (a maximum of `sin`, the minimum of
//! `cosh`). Where that can't be decided, the answer is wider, never narrower.

use std::f64::consts::{FRAC_2_PI, FRAC_PI_2};

use super::consts::{HALF_PI_HEAD, HALF_PI_TAIL, LN_2, LN_2_HEAD, LN_2_TAIL, LN_10, PI, half_pi, reduce};
use super::{Interval, Proven, Unproven};

/// The largest argument `sin`, `cos` and `tan` take, 2²²: the reduction by π/2 is proven for any
/// argument, but loses a digit for every doubling past this, and no design needs more.
const MAX_TRIG: f64 = 4_194_304.0;
/// `asinh` and `acosh` switch to their large-argument forms above 2²⁶, where `x² ± 1` would round.
const LARGE: f64 = 67_108_864.0;
/// Terms of each series.
const EXP_TERMS: u32 = 22;
const ATANH_TERMS: u32 = 24;
const TRIG_TERMS: u32 = 14;
const ATAN_TERMS: u32 = 16;

/// `i`, when both of its bounds are finite.
pub(super) fn finite(i: Interval) -> Proven<Interval> {
    if i.is_finite() { Ok(i) } else { Err(Unproven::Uncertified) }
}

fn n(x: u32) -> Interval {
    Interval::point(f64::from(x))
}

/// `[−b, b]` for `b = ρᵖ/p!` rounded up: what a series leaves out, before any factor.
fn tail(rho: f64, p: u32) -> Interval {
    let rho = Interval::point(rho);
    let mut t = Interval::ONE;
    for i in 1..=p {
        t = t * rho / n(i);
    }
    Interval::new(-t.hi, t.hi)
}

/// `eʳ − 1` for `|r| ≤ ln 2`.
fn expm1_series(r: Interval) -> Interval {
    // Σ_{n=1}^{N} rⁿ/n! = r(1 + r/2(1 + r/3(… (1 + r/N)))).
    let mut t = Interval::ONE;
    for k in (2..=EXP_TERMS).rev() {
        t = Interval::ONE + r * t / n(k);
    }
    // e^ρ ≤ 2 for ρ ≤ ln 2.
    r * t + tail(r.mag(), EXP_TERMS + 1).scale2(1)
}

/// `eˣ` at one number.
fn exp_at(x: f64) -> Proven<Interval> {
    if x.is_nan() || x > 709.79 {
        return Err(Unproven::Uncertified);
    }
    if x < -745.2 {
        // Below half the smallest subnormal.
        return Ok(Interval { lo: 0.0, hi: f64::from_bits(1) });
    }
    if x.abs() <= 0.34 {
        return Ok(Interval::ONE + expm1_series(Interval::point(x)));
    }
    let k = (x / std::f64::consts::LN_2).round();
    let r = reduce(x, k, LN_2_HEAD, LN_2_TAIL);
    // |k| ≤ 1076: a whole number that fits.
    finite((Interval::ONE + expm1_series(r)).scale2(k as i32))
}

/// `eˣ − 1` at one number, accurate relative to its size near zero.
fn expm1_at(x: f64) -> Proven<Interval> {
    if x.abs() <= 0.34 { Ok(expm1_series(Interval::point(x))) } else { Ok(exp_at(x)? - Interval::ONE) }
}

/// `x = m·2ᵏ` exactly, with `m` in `[1/√2, √2]`, for a positive finite `x`.
fn split(x: f64) -> (f64, i32) {
    let (x, shift) = if x < f64::MIN_POSITIVE { (x * f64::from_bits((1023 + 54) << 52), -54) } else { (x, 0) };
    let bits = x.to_bits();
    let k = ((bits >> 52) & 0x7ff) as i32 - 1023 + shift;
    let m = f64::from_bits((bits & 0x000f_ffff_ffff_ffff) | 0x3ff0_0000_0000_0000);
    if m > std::f64::consts::SQRT_2 { (m / 2.0, k + 1) } else { (m, k) }
}

/// `Σ_{n<N} z²ⁿ⁺¹/(2n+1)` with what is left out: `atanh z` for `|z| ≤ 0.2`.
fn atanh_series(z: Interval) -> Interval {
    let w = z.sqr();
    let mut t = Interval::ZERO;
    for k in (0..ATANH_TERMS).rev() {
        t = Interval::ONE / n(2 * k + 1) + w * t;
    }
    let rho = z.mag();
    let p = 2 * ATANH_TERMS + 1;
    // |z|^p / (p (1 − z²)), rounded up.
    let mut b = Interval::ONE;
    for _ in 0..p {
        b = b * Interval::point(rho);
    }
    let b = b / (n(p) * (Interval::ONE - Interval::point(rho).sqr()));
    z * t + Interval::new(-b.hi, b.hi)
}

/// `ln x` at one positive finite number.
fn ln_at(x: f64) -> Interval {
    let (m, k) = split(x);
    // m − 1 is exact (m and 1 are within a factor of 2); m + 1 rounds.
    let z = (Interval::point(m) - Interval::ONE) / (Interval::point(m) + Interval::ONE);
    let k = Interval::point(f64::from(k));
    k * Interval::point(LN_2_HEAD) + k * LN_2_TAIL + atanh_series(z).scale2(1)
}

/// `sin r` and `cos r` for `|r| ≤ 0.8`.
fn sin_cos_series(r: Interval) -> (Interval, Interval) {
    let w = r.sqr();
    let rho = r.mag();
    // sin r = r(1 − w/(2·3)(1 − w/(4·5)(…))), to degree 2N+1.
    let mut s = Interval::ONE;
    for k in (1..=TRIG_TERMS).rev() {
        s = Interval::ONE - w * s / n((2 * k) * (2 * k + 1));
    }
    // cos r = 1 − w/(1·2)(1 − w/(3·4)(…)), to degree 2N.
    let mut c = Interval::ONE;
    for k in (1..=TRIG_TERMS).rev() {
        c = Interval::ONE - w * c / n((2 * k - 1) * (2 * k));
    }
    (r * s + tail(rho, 2 * TRIG_TERMS + 3), c + tail(rho, 2 * TRIG_TERMS + 2))
}

/// `sin x` and `cos x` at one number, `|x| ≤ 2²²`.
fn sin_cos_at(x: f64) -> Proven<(Interval, Interval)> {
    if !(x.abs() <= MAX_TRIG) {
        return Err(Unproven::Uncertified);
    }
    let k = (x * FRAC_2_PI).round();
    let r = reduce(x, k, HALF_PI_HEAD, HALF_PI_TAIL);
    let (s, c) = sin_cos_series(r);
    let unit = Interval { lo: -1.0, hi: 1.0 };
    let clamp = |i: Interval| i.meet(unit).ok_or(Unproven::Uncertified);
    // |k| < 2²²: fits.
    let (s, c) = match (k as i64).rem_euclid(4) {
        0 => (s, c),
        1 => (c, -s),
        2 => (-s, -c),
        _ => (-c, s),
    };
    Ok((clamp(s)?, clamp(c)?))
}

/// `atan z` for `0 ≤ z ≤ 0.1`, the series alternating with shrinking terms.
fn atan_series(z: Interval) -> Interval {
    let w = z.sqr();
    // Σ (−1)ᵏ wᵏ/(2k+1) by Horner: t_k = 1/(2k+1) − w·t_{k+1}.
    let mut t = Interval::ZERO;
    for k in (0..ATAN_TERMS).rev() {
        t = Interval::ONE / n(2 * k + 1) - w * t;
    }
    let p = 2 * ATAN_TERMS + 1;
    let mut b = Interval::ONE;
    for _ in 0..p {
        b = b * Interval::point(z.mag());
    }
    let b = b / n(p);
    z * t + Interval::new(-b.hi, b.hi)
}

/// `atan x` at one number.
fn atan_at(x: f64) -> Proven<Interval> {
    if !x.is_finite() {
        return Err(Unproven::Uncertified);
    }
    if x == 0.0 {
        return Ok(Interval::ZERO);
    }
    let a = x.abs();
    let big = a > 1.0;
    let mut z = if big { Interval::ONE / Interval::point(a) } else { Interval::point(a) };
    // atan z = 2 atan(z / (1 + √(1 + z²))): three halvings take 1 to below 0.1.
    let mut halvings = 0;
    while z.hi > 0.1 && halvings < 4 {
        z = z / (Interval::ONE + (Interval::ONE + z.sqr()).sqrt_nonneg());
        halvings += 1;
    }
    if z.hi > 0.1 || z.lo < 0.0 {
        return Err(Unproven::Uncertified);
    }
    let t = atan_series(z).scale2(halvings);
    let t = if big { half_pi() - t } else { t };
    finite(if x < 0.0 { -t } else { t })
}

/// The range of a function increasing on `x`, from its enclosures at the ends.
fn rising(x: Interval, f: impl Fn(f64) -> Proven<Interval>) -> Proven<Interval> {
    let (a, b) = if x.lo == x.hi {
        let a = f(x.lo)?;
        (a, a)
    } else {
        (f(x.lo)?, f(x.hi)?)
    };
    finite(Interval { lo: a.lo, hi: b.hi })
}

/// The range of a function decreasing on `x`.
fn falling(x: Interval, f: impl Fn(f64) -> Proven<Interval>) -> Proven<Interval> {
    let (a, b) = (f(x.lo)?, f(x.hi)?);
    finite(Interval { lo: b.lo, hi: a.hi })
}

pub(super) fn exp(x: Interval) -> Proven<Interval> {
    rising(x, exp_at)
}

pub(super) fn ln(x: Interval) -> Proven<Interval> {
    if !(x.lo > 0.0) {
        return Err(Unproven::Domain);
    }
    rising(x, |v| finite(ln_at(v)))
}

pub(super) fn log10(x: Interval) -> Proven<Interval> {
    finite(ln(x)? / LN_10)
}

pub(super) fn sqrt(x: Interval) -> Proven<Interval> {
    if x.lo < 0.0 {
        return Err(Unproven::Domain);
    }
    finite(x.sqrt_nonneg())
}

/// Whether some `c + jπ` (j whole) may lie in `x`, for each j that might, with its parity.
fn crossings(x: Interval, c: Interval, f: f64) -> impl Iterator<Item = (i64, bool)> {
    let first = ((x.lo - f) / PI.lo).floor() - 1.0;
    let last = ((x.hi - f) / PI.lo).ceil() + 1.0;
    // Callers bound |x| by 2²², so these fit.
    let (first, last) = (first as i64, last as i64);
    (first..=last).map(move |j| {
        let at = c + Interval::point(j as f64) * PI;
        (j, at.overlaps(x))
    })
}

fn trig(x: Interval, cos: bool) -> Proven<Interval> {
    if !(x.mag() <= MAX_TRIG) {
        return Err(Unproven::Uncertified);
    }
    let unit = Interval { lo: -1.0, hi: 1.0 };
    // Over more than a period the range is all of it; `[−1, 1]` holds it in any case.
    if x.width() >= 8.0 {
        return Ok(unit);
    }
    let pick = |v: f64| sin_cos_at(v).map(|(s, c)| if cos { c } else { s });
    let mut out = pick(x.lo)?.hull(pick(x.hi)?);
    // sin peaks at π/2 + jπ, cos at jπ, at (−1)ʲ.
    let (c, f) = if cos { (Interval::ZERO, 0.0) } else { (half_pi(), FRAC_PI_2) };
    for (j, inside) in crossings(x, c, f) {
        if inside {
            out = out.hull(Interval::point(if j % 2 == 0 { 1.0 } else { -1.0 }));
        }
    }
    out.meet(unit).ok_or(Unproven::Uncertified)
}

pub(super) fn sin(x: Interval) -> Proven<Interval> {
    trig(x, false)
}

pub(super) fn cos(x: Interval) -> Proven<Interval> {
    trig(x, true)
}

/// Refused across a pole.
pub(super) fn tan(x: Interval) -> Proven<Interval> {
    if !(x.mag() <= MAX_TRIG) {
        return Err(Unproven::Uncertified);
    }
    if crossings(x, half_pi(), FRAC_PI_2).any(|(_, inside)| inside) {
        return Err(Unproven::Jump);
    }
    rising(x, |v| {
        let (s, c) = sin_cos_at(v)?;
        finite(s / c)
    })
}

pub(super) fn atan(x: Interval) -> Proven<Interval> {
    rising(x, atan_at)
}

/// `asin x = 2 atan(x / (1 + √((1 − x)(1 + x))))`, which holds on all of `[−1, 1]`.
pub(super) fn asin(x: Interval) -> Proven<Interval> {
    if x.lo < -1.0 || x.hi > 1.0 {
        return Err(Unproven::Domain);
    }
    rising(x, |v| {
        let v = Interval::point(v);
        let s = ((Interval::ONE - v) * (Interval::ONE + v)).sqrt_nonneg();
        Ok(atan(v / (Interval::ONE + s))?.scale2(1))
    })
}

pub(super) fn acos(x: Interval) -> Proven<Interval> {
    finite(half_pi() - asin(x)?)
}

/// `atan2(y, x)`, the angle of `(x, y)` in `(−π, π]`. On a box that misses the origin and the
/// negative x-axis it is continuous, and its extremes are at the box's corners. A box touching
/// the negative x-axis is refused, as the angle jumps there (and at zero depends on its sign).
pub(super) fn atan2(y: Interval, x: Interval) -> Proven<Interval> {
    let y_zero = y.lo <= 0.0 && y.hi >= 0.0;
    if y_zero && x.lo <= 0.0 {
        return Err(if x.hi >= 0.0 { Unproven::Uncertified } else { Unproven::Jump });
    }
    let at = |yv: f64, xv: f64| -> Proven<Interval> {
        let q = Interval::point(yv) / Interval::point(xv);
        if xv > 0.0 {
            atan(q)
        } else if xv < 0.0 {
            // y is not zero here.
            let t = atan(q)?;
            finite(if yv > 0.0 { t + PI } else { t - PI })
        } else {
            let h = half_pi();
            Ok(if yv > 0.0 { h } else { -h })
        }
    };
    let mut out = at(y.lo, x.lo)?;
    for (yv, xv) in [(y.lo, x.hi), (y.hi, x.lo), (y.hi, x.hi)] {
        out = out.hull(at(yv, xv)?);
    }
    Ok(out)
}

fn sinh_at(x: f64) -> Proven<Interval> {
    finite((expm1_at(x)? - expm1_at(-x)?).scale2(-1))
}

fn cosh_at(x: f64) -> Proven<Interval> {
    finite((exp_at(x)? + exp_at(-x)?).scale2(-1))
}

/// `tanh x = −E/(2 + E)` with `E = expm1(−2x)` for `x ≥ 0`; odd.
fn tanh_at(x: f64) -> Proven<Interval> {
    let a = x.abs();
    let e = expm1_at(-2.0 * a)?;
    let t = (-e / (Interval::point(2.0) + e)).meet(Interval { lo: 0.0, hi: 1.0 }).ok_or(Unproven::Uncertified)?;
    Ok(if x < 0.0 { -t } else { t })
}

pub(super) fn sinh(x: Interval) -> Proven<Interval> {
    rising(x, sinh_at)
}

pub(super) fn cosh(x: Interval) -> Proven<Interval> {
    if x.lo >= 0.0 {
        rising(x, cosh_at)
    } else if x.hi <= 0.0 {
        falling(x, cosh_at)
    } else {
        let hi = cosh_at(x.lo)?.hi.max(cosh_at(x.hi)?.hi);
        finite(Interval { lo: 1.0, hi })
    }
}

pub(super) fn tanh(x: Interval) -> Proven<Interval> {
    rising(x, tanh_at)
}

/// `asinh x = ln(|x| + √(x² + 1))`, odd; above 2²⁶ `ln 2|x|` plus at most `1/(4x²)`.
fn asinh_at(x: f64) -> Proven<Interval> {
    let a = x.abs();
    let t = if a < LARGE {
        let v = Interval::point(a);
        ln(v + (v.sqr() + Interval::ONE).sqrt_nonneg())?
    } else {
        let u = (Interval::ONE / (Interval::point(a).sqr() * Interval::point(4.0))).hi;
        ln_at(a) + LN_2 + Interval { lo: 0.0, hi: u }
    };
    finite(if x < 0.0 { -t } else { t })
}

pub(super) fn asinh(x: Interval) -> Proven<Interval> {
    rising(x, asinh_at)
}

/// `acosh x = ln(x + √((x − 1)(x + 1)))`, `x ≥ 1`; above 2²⁶ `ln 2x` less at most `1/x²`.
fn acosh_at(x: f64) -> Proven<Interval> {
    if x < LARGE {
        let v = Interval::point(x);
        ln(v + ((v - Interval::ONE) * (v + Interval::ONE)).sqrt_nonneg())
    } else {
        let u = (Interval::ONE / Interval::point(x).sqr()).hi;
        finite(ln_at(x) + LN_2 - Interval { lo: 0.0, hi: u })
    }
}

pub(super) fn acosh(x: Interval) -> Proven<Interval> {
    if x.lo < 1.0 {
        return Err(Unproven::Domain);
    }
    rising(x, acosh_at)
}

/// `atanh x = (ln(1 + x) − ln(1 − x))/2` on `(−1, 1)`.
pub(super) fn atanh(x: Interval) -> Proven<Interval> {
    if !(x.lo > -1.0 && x.hi < 1.0) {
        return Err(Unproven::Domain);
    }
    rising(x, |v| {
        let v = Interval::point(v);
        Ok((ln(Interval::ONE + v)? - ln(Interval::ONE - v)?).scale2(-1))
    })
}

pub(super) fn hypot(a: Interval, b: Interval) -> Proven<Interval> {
    finite((a.sqr() + b.sqr()).sqrt_nonneg())
}

/// `xⁿ` at one number, by squaring.
fn powi_at(x: f64, n: u64) -> Interval {
    let mut out = Interval::ONE;
    let mut base = Interval::point(x);
    let mut n = n;
    while n > 0 {
        if n & 1 == 1 {
            out = out * base;
        }
        n >>= 1;
        if n > 0 {
            base = base.sqr();
        }
    }
    out
}

/// `xⁿ` for a whole `n`: odd powers rise, even ones follow `|x|`; a negative power is refused
/// where `x` may be zero. `x⁰ = 1` for every `x`, as `powf` has it.
fn powi(x: Interval, n: f64) -> Proven<Interval> {
    if n == 0.0 {
        return Ok(Interval::ONE);
    }
    if !(n.abs() < 9.0e18) {
        return Err(Unproven::Uncertified);
    }
    // Whole and within u64.
    let m = n.abs() as u64;
    let pos = if m % 2 == 1 {
        finite(Interval { lo: powi_at(x.lo, m).lo, hi: powi_at(x.hi, m).hi })?
    } else {
        finite(Interval { lo: powi_at(x.mig(), m).lo, hi: powi_at(x.mag(), m).hi })?
    };
    if n > 0.0 {
        Ok(pos)
    } else if x.lo <= 0.0 && x.hi >= 0.0 {
        Err(Unproven::Domain)
    } else {
        finite(Interval::ONE / pos)
    }
}

/// `xʸ` as `powf` defines it: a whole power of any `x`; a fractional one of `x ≥ 0` (of `x > 0` for
/// a negative power); a power that varies, of `x > 0`.
pub(super) fn pow(x: Interval, y: Interval) -> Proven<Interval> {
    if let Some(e) = y.as_point() {
        if e == e.trunc() {
            return powi(x, e);
        }
        let at = |v: f64| -> Proven<Interval> { if v == 0.0 { Ok(Interval::ZERO) } else { exp(Interval::point(e) * finite(ln_at(v))?) } };
        if e > 0.0 {
            if x.lo < 0.0 {
                return Err(Unproven::Domain);
            }
            return rising(x, at);
        }
        if !(x.lo > 0.0) {
            return Err(Unproven::Domain);
        }
        return falling(x, at);
    }
    if !(x.lo > 0.0) {
        return Err(Unproven::Domain);
    }
    exp(y * ln(x)?)
}

/// A step function's value where it can't step inside `x`.
fn step(x: Interval, f: fn(f64) -> f64) -> Proven<Interval> {
    let (a, b) = (f(x.lo), f(x.hi));
    if a == b { Ok(Interval::point(a)) } else { Err(Unproven::Jump) }
}

pub(super) fn floor(x: Interval) -> Proven<Interval> {
    step(x, f64::floor)
}

pub(super) fn ceil(x: Interval) -> Proven<Interval> {
    step(x, f64::ceil)
}

pub(super) fn round(x: Interval) -> Proven<Interval> {
    step(x, f64::round)
}

pub(super) fn trunc(x: Interval) -> Proven<Interval> {
    step(x, f64::trunc)
}

/// `sign`: 1, 0 or −1; refused where `x` may be zero without being only zero.
pub(super) fn sign(x: Interval) -> Proven<Interval> {
    if x.lo > 0.0 {
        Ok(Interval::ONE)
    } else if x.hi < 0.0 {
        Ok(-Interval::ONE)
    } else if x == Interval::ZERO {
        Ok(Interval::ZERO)
    } else {
        Err(Unproven::Jump)
    }
}

/// `x mod y` as `rem_euclid`: `x − |y|·⌊x/|y|⌋`, in `[0, |y|)`, while that floor doesn't change.
pub(super) fn rem_euclid(x: Interval, y: Interval) -> Proven<Interval> {
    if y.lo <= 0.0 && y.hi >= 0.0 {
        return Err(Unproven::Domain);
    }
    let m = y.abs();
    let q = x / m;
    let (a, b) = (q.lo.floor(), q.hi.floor());
    if a != b {
        return Err(Unproven::Jump);
    }
    let r = x - Interval::point(a) * m;
    r.meet(Interval { lo: 0.0, hi: m.hi }).ok_or(Unproven::Uncertified)
}

pub(super) fn min(a: Interval, b: Interval) -> Interval {
    Interval { lo: a.lo.min(b.lo), hi: a.hi.min(b.hi) }
}

pub(super) fn max(a: Interval, b: Interval) -> Interval {
    Interval { lo: a.lo.max(b.lo), hi: a.hi.max(b.hi) }
}
