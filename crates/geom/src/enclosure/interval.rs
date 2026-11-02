//! Intervals that hold the exact result of every operation on what they hold.
//!
//! Each bound is the rounded result, kept where it is exact or already on the right side of the
//! exact value, and otherwise moved one step outward. A rounded result is never more than half a
//! step from the exact one, so one step is always enough. Which side the exact value lies on is
//! read from the rounding error, found exactly: by TwoSum for a sum, and by a fused multiply-add
//! for a product, a quotient and a square root, whose single rounding keeps the error's sign.
//!
//! A fused multiply-add can round a nonzero error to zero only when the error is below the
//! smallest subnormal. [`EXACT_FLOOR`] is where that stops being possible (see [`prod`],
//! [`quot`], [`root`]); below it a zero error is not trusted and both bounds step.
//!
//! The bounds are never NaN. An infinite bound means "unbounded on this side": [`Interval::ENTIRE`]
//! stands for anything, as when dividing by an interval that holds zero.

use std::cmp::Ordering;
use std::ops::{Add, Div, Mul, Neg, Sub};

/// 2⁻⁹⁶⁸. From here up, a product, a quotient or a square root whose fused multiply-add residual
/// is zero is exact.
pub(super) const EXACT_FLOOR: f64 = f64::from_bits(55 << 52);

/// The numbers from `lo` to `hi`, `lo <= hi`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Interval {
    pub(crate) lo: f64,
    pub(crate) hi: f64,
}

/// The bounds of a rounded result `r` given the sign of `exact − r`.
fn around(r: f64, side: Ordering) -> (f64, f64) {
    match side {
        Ordering::Greater => (r, r.next_up()),
        Ordering::Less => (r.next_down(), r),
        Ordering::Equal => (r, r),
    }
}

/// Both neighbours of `r`: the exact value's side is unknown.
fn both(r: f64) -> (f64, f64) {
    (r.next_down(), r.next_up())
}

/// Bounds for a result that is not finite: NaN (an undefined combination such as ∞ − ∞ or 0·∞)
/// bounds nothing, and an overflow from finite operands lies beyond the largest finite number.
fn not_finite(r: f64, finite_operands: bool) -> (f64, f64) {
    if r.is_nan() {
        (f64::NEG_INFINITY, f64::INFINITY)
    } else if finite_operands && r > 0.0 {
        (f64::MAX, f64::INFINITY)
    } else if finite_operands {
        (f64::NEG_INFINITY, -f64::MAX)
    } else {
        (r, r)
    }
}

/// Bounds of the exact `x + y`. TwoSum's error term is exact for any finite operands whose rounded
/// sum is finite (an underflowing sum is exact, so there is no lower limit).
pub(super) fn sum(x: f64, y: f64) -> (f64, f64) {
    let s = x + y;
    if !s.is_finite() || !x.is_finite() || !y.is_finite() {
        return not_finite(s, x.is_finite() && y.is_finite());
    }
    let b = s - x;
    let e = (x - (s - b)) + (y - b);
    around(s, e.partial_cmp(&0.0).unwrap_or(Ordering::Equal))
}

/// Bounds of the exact `x·y`. The residual `fma(x, y, −p)` has the sign of `x·y − p`. When it is
/// zero and `|p| ≥ 2⁻⁹⁶⁸`, the product is exact: writing `x = M·2^a`, `y = N·2^b` with whole `M, N`
/// below 2⁵³, `|x·y| ≥ 2⁻⁹⁶⁹` gives `a + b ≥ −1074`, so a nonzero `x·y − p`, a multiple of
/// `2^(a+b)`, is at least the smallest subnormal and does not round to zero.
pub(super) fn prod(x: f64, y: f64) -> (f64, f64) {
    if (x == 0.0 && y.is_finite()) || (y == 0.0 && x.is_finite()) {
        return (0.0, 0.0);
    }
    let p = x * y;
    if !p.is_finite() || !x.is_finite() || !y.is_finite() {
        return not_finite(p, x.is_finite() && y.is_finite());
    }
    let e = x.mul_add(y, -p);
    if e == 0.0 && p.abs() < EXACT_FLOOR {
        return both(p);
    }
    around(p, e.partial_cmp(&0.0).unwrap_or(Ordering::Equal))
}

/// Bounds of the exact `x/y`, `y ≠ 0`. The remainder `r = fma(−q, y, x) = x − q·y` (rounded once)
/// gives the side: `x/y − q = (x − q·y)/y`. When it is zero and `|x| ≥ 2⁻⁹⁶⁸`, the quotient is
/// exact: the exact remainder is a multiple of the smaller of `x`'s and `q·y`'s quanta, and with
/// `q·y` within a rounding of `x` (or one of `q`, `y` subnormal and the other above 2⁵⁴) the
/// latter is at least 2⁻¹⁰⁷³, so a nonzero remainder does not round to zero. A quotient that
/// underflows to zero leaves `r = x`, whose sign is right.
pub(super) fn quot(x: f64, y: f64) -> (f64, f64) {
    let q = x / y;
    if !q.is_finite() || !x.is_finite() || !y.is_finite() {
        // A finite number over an unbounded divisor tends to zero from the quotient's side.
        if x.is_finite() && y.is_infinite() {
            return (0.0, 0.0);
        }
        return not_finite(q, x.is_finite() && y.is_finite());
    }
    if x == 0.0 {
        return (0.0, 0.0);
    }
    let r = (-q).mul_add(y, x);
    if r == 0.0 && x.abs() < EXACT_FLOOR {
        return both(q);
    }
    let side = r.partial_cmp(&0.0).unwrap_or(Ordering::Equal);
    around(q, if y > 0.0 { side } else { side.reverse() })
}

/// Bounds of the exact `√x`, `x ≥ 0`. The residual `fma(r, r, −x)` has the sign of `r − √x`. When it
/// is zero and `x ≥ 2⁻⁹⁶⁸`, the root is exact: `r ≥ 2⁻⁴⁸⁴` has a quantum of at least 2⁻⁵³⁶, so
/// `r² − x` is a multiple of 2⁻¹⁰⁷⁴ and a nonzero one does not round to zero.
pub(super) fn root(x: f64) -> (f64, f64) {
    let r = x.sqrt();
    if x == 0.0 || !r.is_finite() {
        return (r, r);
    }
    let e = r.mul_add(r, -x);
    if e == 0.0 && x < EXACT_FLOOR {
        return both(r);
    }
    around(r, e.partial_cmp(&0.0).unwrap_or(Ordering::Equal).reverse())
}

impl Interval {
    pub const ZERO: Interval = Interval { lo: 0.0, hi: 0.0 };
    pub const ONE: Interval = Interval { lo: 1.0, hi: 1.0 };
    pub const ENTIRE: Interval = Interval { lo: f64::NEG_INFINITY, hi: f64::INFINITY };

    /// Exactly `x`; anything for a number that is not finite.
    pub const fn point(x: f64) -> Interval {
        if x.is_finite() { Interval { lo: x, hi: x } } else { Interval::ENTIRE }
    }
    /// The numbers from `lo` to `hi`, whichever order they come in; anything if either is NaN.
    pub fn new(lo: f64, hi: f64) -> Interval {
        if lo.is_nan() || hi.is_nan() { Interval::ENTIRE } else { Interval { lo: lo.min(hi), hi: lo.max(hi) } }
    }
    pub fn lo(self) -> f64 {
        self.lo
    }
    pub fn hi(self) -> f64 {
        self.hi
    }
    pub fn is_finite(self) -> bool {
        self.lo.is_finite() && self.hi.is_finite()
    }
    /// The one number it holds, if it holds only one.
    pub fn as_point(self) -> Option<f64> {
        (self.lo == self.hi).then_some(self.lo)
    }
    pub fn contains(self, x: f64) -> bool {
        self.lo <= x && x <= self.hi
    }
    /// The width, rounded up.
    pub fn width(self) -> f64 {
        sum(self.hi, -self.lo).1
    }
    /// The largest magnitude it holds.
    pub fn mag(self) -> f64 {
        self.lo.abs().max(self.hi.abs())
    }
    /// The smallest magnitude it holds.
    pub fn mig(self) -> f64 {
        if self.lo <= 0.0 && self.hi >= 0.0 { 0.0 } else { self.lo.abs().min(self.hi.abs()) }
    }
    /// A number held: the midpoint, correctly rounded, so between the bounds even in the
    /// subnormal range.
    pub fn mid(self) -> f64 {
        f64::midpoint(self.lo, self.hi)
    }
    /// Both halves, split at the midpoint.
    pub fn bisect(self) -> (Interval, Interval) {
        let m = self.mid();
        (Interval { lo: self.lo, hi: m }, Interval { lo: m, hi: self.hi })
    }
    /// The smallest interval holding both.
    pub fn hull(self, o: Interval) -> Interval {
        Interval { lo: self.lo.min(o.lo), hi: self.hi.max(o.hi) }
    }
    /// The numbers both hold, if any.
    pub fn meet(self, o: Interval) -> Option<Interval> {
        let (lo, hi) = (self.lo.max(o.lo), self.hi.min(o.hi));
        (lo <= hi).then_some(Interval { lo, hi })
    }
    /// Whether they share a number.
    pub fn overlaps(self, o: Interval) -> bool {
        self.lo <= o.hi && o.lo <= self.hi
    }
    /// The squares: exact range, so `[−1, 2]²` is `[0, 4]`, not `[−2, 4]`.
    pub fn sqr(self) -> Interval {
        let (a, b) = (prod(self.lo, self.lo), prod(self.hi, self.hi));
        if self.lo >= 0.0 {
            Interval { lo: a.0, hi: b.1 }
        } else if self.hi <= 0.0 {
            Interval { lo: b.0, hi: a.1 }
        } else {
            Interval { lo: 0.0, hi: a.1.max(b.1) }
        }
    }
    /// The square roots of the part at or above zero (callers check the domain).
    pub fn sqrt_nonneg(self) -> Interval {
        Interval { lo: root(self.lo.max(0.0)).0, hi: root(self.hi.max(0.0)).1 }
    }
    /// Magnitudes: exact range.
    pub fn abs(self) -> Interval {
        Interval { lo: self.mig(), hi: self.mag() }
    }
    /// Times `2^k`, exact unless it overflows or falls below the normal range.
    pub fn scale2(self, k: i32) -> Interval {
        let mut out = self;
        let mut k = k;
        while k != 0 {
            let step = k.clamp(-1000, 1000);
            // 2^step, built from its bits: a normal number for |step| ≤ 1000.
            let factor = f64::from_bits(((1023 + i64::from(step)) as u64) << 52);
            out = out * Interval::point(factor);
            k -= step;
        }
        out
    }
}

impl Neg for Interval {
    type Output = Interval;
    fn neg(self) -> Interval {
        Interval { lo: -self.hi, hi: -self.lo }
    }
}

impl Add for Interval {
    type Output = Interval;
    fn add(self, o: Interval) -> Interval {
        Interval { lo: sum(self.lo, o.lo).0, hi: sum(self.hi, o.hi).1 }
    }
}

impl Sub for Interval {
    type Output = Interval;
    fn sub(self, o: Interval) -> Interval {
        self + -o
    }
}

impl Mul for Interval {
    type Output = Interval;
    fn mul(self, o: Interval) -> Interval {
        let c = [prod(self.lo, o.lo), prod(self.lo, o.hi), prod(self.hi, o.lo), prod(self.hi, o.hi)];
        Interval { lo: c.iter().fold(f64::INFINITY, |m, b| m.min(b.0)), hi: c.iter().fold(f64::NEG_INFINITY, |m, b| m.max(b.1)) }
    }
}

impl Div for Interval {
    type Output = Interval;
    /// Anything, when `o` holds zero.
    fn div(self, o: Interval) -> Interval {
        if o.lo <= 0.0 && o.hi >= 0.0 {
            return Interval::ENTIRE;
        }
        let c = [quot(self.lo, o.lo), quot(self.lo, o.hi), quot(self.hi, o.lo), quot(self.hi, o.hi)];
        Interval { lo: c.iter().fold(f64::INFINITY, |m, b| m.min(b.0)), hi: c.iter().fold(f64::NEG_INFINITY, |m, b| m.max(b.1)) }
    }
}
