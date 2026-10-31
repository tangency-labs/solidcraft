//! Intervals that hold the exact result of every operation on what they hold: each bound is the
//! nearest number to its exact value, moved one step outward, and a nearest number is never more
//! than half a step from the exact one (normal, subnormal or underflowed to zero alike). This is
//! what the bounds of [`super::curve`] are computed in, so they are proven, not estimated.

use super::{Blend, HPoint};

/// The numbers from `lo` to `hi`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Interval {
    pub(crate) lo: f64,
    pub(crate) hi: f64,
}

impl Interval {
    pub(crate) const ZERO: Interval = Interval { lo: 0.0, hi: 0.0 };
    const ENTIRE: Interval = Interval { lo: f64::NEG_INFINITY, hi: f64::INFINITY };

    /// Exactly `x`.
    pub(crate) const fn point(x: f64) -> Interval {
        Interval { lo: x, hi: x }
    }
    /// The rounded `lo` and `hi` moved one step outward.
    fn out(lo: f64, hi: f64) -> Interval {
        if lo.is_nan() || hi.is_nan() { Interval::ENTIRE } else { Interval { lo: lo.next_down(), hi: hi.next_up() } }
    }
    /// The four rounded products or quotients of the bounds, moved outward.
    fn hull(c: [f64; 4]) -> Interval {
        if c.iter().any(|x| x.is_nan()) {
            return Interval::ENTIRE;
        }
        Interval::out(c.iter().copied().fold(f64::INFINITY, f64::min), c.iter().copied().fold(f64::NEG_INFINITY, f64::max))
    }
    pub(crate) fn add(self, o: Interval) -> Interval {
        Interval::out(self.lo + o.lo, self.hi + o.hi)
    }
    pub(crate) fn sub(self, o: Interval) -> Interval {
        Interval::out(self.lo - o.hi, self.hi - o.lo)
    }
    pub(crate) fn mul(self, o: Interval) -> Interval {
        Interval::hull([self.lo * o.lo, self.lo * o.hi, self.hi * o.lo, self.hi * o.hi])
    }
    /// Anything, when `o` holds zero.
    pub(crate) fn div(self, o: Interval) -> Interval {
        if !(o.lo > 0.0 || o.hi < 0.0) {
            return Interval::ENTIRE;
        }
        Interval::hull([self.lo / o.lo, self.lo / o.hi, self.hi / o.lo, self.hi / o.hi])
    }
    pub(crate) fn is_finite(self) -> bool {
        self.lo.is_finite() && self.hi.is_finite()
    }
    /// The largest absolute value held.
    pub(crate) fn mag(self) -> f64 {
        self.lo.abs().max(self.hi.abs())
    }
    /// A number held: the midpoint, correctly rounded, so between the bounds even in the
    /// subnormal range.
    pub(crate) fn mid(self) -> f64 {
        f64::midpoint(self.lo, self.hi)
    }
}

/// A homogeneous point each of whose coordinates is held by an interval.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HInterval {
    pub(crate) x: Interval,
    pub(crate) y: Interval,
    pub(crate) z: Interval,
    pub(crate) w: Interval,
}

impl HInterval {
    /// Exactly `h`.
    pub(crate) fn point(h: HPoint) -> HInterval {
        HInterval { x: Interval::point(h.x), y: Interval::point(h.y), z: Interval::point(h.z), w: Interval::point(h.w) }
    }
    /// Finite, with a weight that is surely positive.
    pub(crate) fn is_sound(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite() && self.w.is_finite() && self.w.lo > 0.0
    }
    /// A point held by it.
    pub(crate) fn mid(self) -> HPoint {
        HPoint { x: self.x.mid(), y: self.y.mid(), z: self.z.mid(), w: self.w.mid() }
    }
}

impl Blend for HInterval {
    type Factor = Interval;
    const ZERO: HInterval = HInterval { x: Interval::ZERO, y: Interval::ZERO, z: Interval::ZERO, w: Interval::ZERO };
    fn factors(lo: f64, u: f64, hi: f64) -> (Interval, Interval) {
        let (lo, u, hi) = (Interval::point(lo), Interval::point(u), Interval::point(hi));
        let den = hi.sub(lo);
        (u.sub(lo).div(den), hi.sub(u).div(den))
    }
    fn blend(self, a: Interval, other: HInterval, b: Interval) -> HInterval {
        let f = |p: Interval, q: Interval| p.mul(a).add(q.mul(b));
        HInterval { x: f(self.x, other.x), y: f(self.y, other.y), z: f(self.z, other.z), w: f(self.w, other.w) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each operation holds its exact result (checked with a fused multiply-add, which rounds
    /// once, so its sign is the exact one's), including where it rounds below the normal range or
    /// to zero, and where `1 − α` loses most of α's digits; and a midpoint is held even there.
    #[test]
    fn every_operation_holds_its_exact_result() {
        let holds = |x: Interval, k: f64, target: f64| x.lo.mul_add(k, -target) <= 0.0 && x.hi.mul_add(k, -target) >= 0.0;
        let third = Interval::point(1.0).div(Interval::point(3.0));
        assert!(holds(third, 3.0, 1.0), "{third:?}");
        let tiny = Interval::point(f64::from_bits(1)).mul(Interval::point(0.5));
        assert!(tiny.lo < 0.0 && tiny.hi > 0.0, "{tiny:?}");
        // α = (1.5 − 2⁻⁵²)/1.5; the exact 1 − α is 2⁻⁵²/1.5.
        let rest = Interval::point(1.0).sub(Interval::point(1.5 - 2f64.powi(-52)).div(Interval::point(1.5)));
        assert!(holds(rest, 1.5, 2f64.powi(-52)), "{rest:?}");
        assert_eq!(Interval::point(1.0).div(Interval::ZERO), Interval::ENTIRE);
        assert!(!Interval::point(f64::MAX).add(Interval::point(f64::MAX)).is_finite());
        assert!(!Interval::point(0.0).mul(Interval::point(f64::INFINITY)).is_finite());
        // `lo/2 + hi/2` rounds the smallest subnormal to 0, outside [η, η].
        for x in [f64::from_bits(1), f64::from_bits(3)] {
            assert_eq!(Interval::point(x).mid(), x);
        }
    }
}
