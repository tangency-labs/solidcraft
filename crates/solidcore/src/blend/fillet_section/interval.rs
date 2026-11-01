//! Rounded interval arithmetic for the G^n section's certificate: every result widened
//! outward past its rounding error, so it holds the exact result of the operation on any
//! numbers in the operands. An empty or undefined result (a division by an interval holding
//! 0, a NaN) is the whole line, which no test passes.
//!
//! Fork: `sm_Ivl` and its helpers in `source/SMLib/src/SmFilletSection.cpp`
//! (tangency-labs/smlib@11ea2d9); see [`super`] for the table.

use core::ops::{Add, Div, Mul, Sub};

/// `std::min(a, b)`: `(b < a) ? b : a` (a NaN `a` is kept).
#[inline]
pub(crate) fn cmin(a: f64, b: f64) -> f64 {
    if b < a {
        b
    } else {
        a
    }
}

/// `std::max(a, b)`: `(a < b) ? b : a` (a NaN `a` is kept).
#[inline]
pub(crate) fn cmax(a: f64, b: f64) -> f64 {
    if a < b {
        b
    } else {
        a
    }
}

/// `sm_Dn`: moved down by twice the rounding error of an operation that gave `x`
/// (`|x| 2^-53`, or `2^-1075` below the normal range).
#[inline]
#[must_use]
pub fn round_down(x: f64) -> f64 {
    x - (x.abs() * 2.3e-16 + 1.0e-300)
}

/// `sm_Up`: moved up by twice the rounding error of an operation that gave `x`.
#[inline]
#[must_use]
pub fn round_up(x: f64) -> f64 {
    x + (x.abs() * 2.3e-16 + 1.0e-300)
}

/// `sm_Ivl`: the closed interval `[lo, hi]`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ivl {
    /// The lower end.
    pub lo: f64,
    /// The upper end.
    pub hi: f64,
}

impl From<f64> for Ivl {
    /// `sm_Ivl(double d)`: the point interval `[d, d]`.
    fn from(d: f64) -> Self {
        Self { lo: d, hi: d }
    }
}

impl Ivl {
    /// `sm_Ivl(double a, double b)`.
    #[must_use]
    pub const fn new(lo: f64, hi: f64) -> Self {
        Self { lo, hi }
    }

    /// `sm_Whole`: the whole line.
    #[must_use]
    pub const fn whole() -> Self {
        Self::new(f64::NEG_INFINITY, f64::INFINITY)
    }

    /// `sm_Checked`: `[a, b]` rounded outward, or the whole line if either is NaN.
    #[must_use]
    pub fn checked(a: f64, b: f64) -> Self {
        if a.is_nan() || b.is_nan() {
            Self::whole()
        } else {
            Self::new(round_down(a), round_up(b))
        }
    }

    /// `sm_Positive(const sm_Ivl &)`: every number in it is positive.
    #[must_use]
    pub fn is_positive(self) -> bool {
        self.lo > 0.0
    }

    /// `sm_Sqrt(const sm_Ivl &)`: the square roots of its non-negative part (the whole line
    /// if it has none).
    #[must_use]
    pub fn sqrt(self) -> Self {
        // !(x.hi >= 0.0): a NaN end fails it too
        let has_non_negative = self.hi >= 0.0;
        if !has_non_negative {
            return Self::whole();
        }
        Self::checked(
            crate::geom::vector3d::sqrt(cmax(self.lo, 0.0)),
            crate::geom::vector3d::sqrt(self.hi),
        )
    }

    /// `sm_Mag`: the largest magnitude in it.
    #[must_use]
    pub fn mag(self) -> f64 {
        cmax(self.lo.abs(), self.hi.abs())
    }

    /// `sm_Mid`: its middle.
    #[must_use]
    pub fn mid(self) -> f64 {
        0.5 * (self.lo + self.hi)
    }

    /// `sm_Hull`: the smallest interval holding both.
    #[must_use]
    pub fn hull(self, b: Self) -> Self {
        Self::new(cmin(self.lo, b.lo), cmax(self.hi, b.hi))
    }
}

impl Add for Ivl {
    type Output = Self;
    fn add(self, b: Self) -> Self {
        Self::checked(self.lo + b.lo, self.hi + b.hi)
    }
}

impl Sub for Ivl {
    type Output = Self;
    fn sub(self, b: Self) -> Self {
        Self::checked(self.lo - b.hi, self.hi - b.lo)
    }
}

impl Mul for Ivl {
    type Output = Self;
    fn mul(self, b: Self) -> Self {
        let p0 = self.lo * b.lo;
        let rest = [self.lo * b.hi, self.hi * b.lo, self.hi * b.hi];
        let mut lo = p0;
        let mut hi = p0;
        for pi in rest {
            if pi.is_nan() {
                return Self::whole();
            }
            lo = cmin(lo, pi);
            hi = cmax(hi, pi);
        }
        Self::checked(lo, hi)
    }
}

impl Div for Ivl {
    type Output = Self;
    fn div(self, b: Self) -> Self {
        let excludes_zero = b.lo > 0.0 || b.hi < 0.0;
        if !excludes_zero {
            return Self::whole();
        }
        self * Self::checked(1.0 / b.hi, 1.0 / b.lo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operations_enclose_their_exact_results() {
        let a = Ivl::new(1.0, 2.0);
        let b = Ivl::new(-3.0, 0.5);
        let s = a + b;
        assert!(s.lo < -2.0 && s.hi > 2.5);
        let d = a - b;
        assert!(d.lo < 0.5 && d.hi > 5.0);
        let m = a * b;
        assert!(m.lo < -6.0 && m.hi > 1.0);
        assert!(m.lo > -6.0 - 1e-14 && m.hi < 1.0 + 1e-14);
        // 1 / 3 is not a double: the quotient's interval straddles it
        let q = Ivl::from(1.0) / Ivl::from(3.0);
        assert!(q.lo < 1.0 / 3.0 && q.hi > 1.0 / 3.0);
        assert!(q.is_positive());
    }

    #[test]
    fn undefined_results_are_the_whole_line() {
        assert_eq!(Ivl::from(1.0) / Ivl::new(-1.0, 1.0), Ivl::whole());
        assert_eq!(Ivl::new(-2.0, -1.0).sqrt(), Ivl::whole());
        assert_eq!(Ivl::whole() * Ivl::from(0.0), Ivl::whole());
        assert_eq!(Ivl::from(f64::NAN) + Ivl::from(1.0), Ivl::whole());
        // a NaN in the first product is kept by std::min and caught by sm_Checked
        assert_eq!(Ivl::new(f64::NAN, 1.0) * Ivl::from(2.0), Ivl::whole());
    }

    #[test]
    fn sqrt_mag_mid_hull() {
        let r = Ivl::new(-1.0, 4.0).sqrt();
        assert!(r.lo < 0.0 && r.lo > -1e-299 && r.hi > 2.0 && r.hi < 2.0 + 1e-15);
        assert_eq!(Ivl::new(-3.0, 2.0).mag(), 3.0);
        assert_eq!(Ivl::new(-3.0, 2.0).mid(), -0.5);
        assert_eq!(
            Ivl::new(0.0, 1.0).hull(Ivl::new(-1.0, 0.5)),
            Ivl::new(-1.0, 1.0)
        );
    }
}
