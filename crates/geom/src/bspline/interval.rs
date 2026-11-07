//! Homogeneous points held in [`Interval`]s, which hold the exact result of every operation on
//! what they hold. This is what the bounds of [`super::curve`] are computed in, so they are
//! proven, not estimated.

use super::{Blend, HPoint};
use crate::enclosure::Interval;

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
        let den = hi - lo;
        ((u - lo) / den, (hi - u) / den)
    }
    fn blend(self, a: Interval, other: HInterval, b: Interval) -> HInterval {
        let f = |p: Interval, q: Interval| p * a + q * b;
        HInterval { x: f(self.x, other.x), y: f(self.y, other.y), z: f(self.z, other.z), w: f(self.w, other.w) }
    }
}
