//! Proven enclosures of real functions (#10414): intervals that hold the exact value of an
//! expression over a range of its variable, and the same with the first derivative.
//!
//! [`Interval`] rounds every bound outward. Only `+ − × ÷` and `√` are correctly rounded by IEEE 754
//! on every target, wasm included; the standard library leaves the precision of `sin`, `exp`
//! and the other transcendentals unspecified, so no padding around their results is a proof.
//! Each of those is computed here from the four basic operations alone (`elementary`).
//!
//! [`Real`] is one number type for all three uses: `f64` (the plain value, with the standard
//! library's functions, so code generic over it computes exactly what it did before),
//! [`Interval`] (the range), and [`Jet`] (a value with its first derivative).
//!
//! The functions are the ones the expression language has, with its meanings: `signum(±0) = ±1`,
//! `rem_euclid` for `mod`, `powf` for powers, `log10` for `log`. Where an enclosure can't be
//! given, the reason is an [`Unproven`]: outside the function's domain, across a jump, or beyond
//! what is certified.

mod consts;
mod elementary;
mod interval;
mod jet;
#[cfg(test)]
mod tests;

use std::cmp::Ordering;
use std::fmt::Debug;

pub use interval::Interval;
pub use jet::Jet;

/// Why a value has no enclosure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unproven {
    /// The argument is, or may be, outside the function's domain (`sqrt` of a negative, `ln` of
    /// zero, a division by what may be zero).
    Domain,
    /// The function may jump inside the range (`floor`, `signum`, `mod`, `tan` at a pole, `atan2`
    /// across the negative x-axis).
    Jump,
    /// The value is beyond what is proven: an overflow, `sin` of a huge argument.
    Uncertified,
}

pub type Proven<T> = Result<T, Unproven>;

/// A number type the expression language computes in.
pub trait Real: Copy + Debug {
    /// Exactly `x`, a constant.
    fn exact(x: f64) -> Proven<Self>;
    /// How it compares with `c`, when that is decided for every number it stands for.
    fn cmp_f64(self, c: f64) -> Option<Ordering>;
    /// The one number it is, if it is known to be a single number.
    fn as_point(self) -> Option<f64>;
    /// The numbers either stands for. `f64` has only one: it refuses two different ones.
    fn hull(self, o: Self) -> Proven<Self>;

    fn neg(self) -> Self;
    fn add(self, o: Self) -> Proven<Self>;
    fn sub(self, o: Self) -> Proven<Self>;
    fn mul(self, o: Self) -> Proven<Self>;
    fn div(self, o: Self) -> Proven<Self>;
    /// `self` to the power `e`, as `powf`.
    fn pow(self, e: Self) -> Proven<Self>;
    fn sqrt(self) -> Proven<Self>;
    fn exp(self) -> Proven<Self>;
    fn ln(self) -> Proven<Self>;
    fn log10(self) -> Proven<Self>;
    fn sin(self) -> Proven<Self>;
    fn cos(self) -> Proven<Self>;
    fn tan(self) -> Proven<Self>;
    fn asin(self) -> Proven<Self>;
    fn acos(self) -> Proven<Self>;
    fn atan(self) -> Proven<Self>;
    /// The angle of `(x, self)`, as `f64::atan2`.
    fn atan2(self, x: Self) -> Proven<Self>;
    fn sinh(self) -> Proven<Self>;
    fn cosh(self) -> Proven<Self>;
    fn tanh(self) -> Proven<Self>;
    fn asinh(self) -> Proven<Self>;
    fn acosh(self) -> Proven<Self>;
    fn atanh(self) -> Proven<Self>;
    fn hypot(self, o: Self) -> Proven<Self>;
    fn abs(self) -> Proven<Self>;
    fn signum(self) -> Proven<Self>;
    fn floor(self) -> Proven<Self>;
    fn ceil(self) -> Proven<Self>;
    fn round(self) -> Proven<Self>;
    fn trunc(self) -> Proven<Self>;
    fn rem_euclid(self, o: Self) -> Proven<Self>;
    /// `self` when it is below `o`, else `o`.
    fn min(self, o: Self) -> Proven<Self>;
    /// `self` when it is above `o`, else `o`.
    fn max(self, o: Self) -> Proven<Self>;
}

/// The plain value: the standard library's operations, never refused, so a computation generic
/// over [`Real`] gives the same bits as one written for `f64`.
impl Real for f64 {
    fn exact(x: f64) -> Proven<f64> {
        Ok(x)
    }
    fn cmp_f64(self, c: f64) -> Option<Ordering> {
        self.partial_cmp(&c)
    }
    fn as_point(self) -> Option<f64> {
        Some(self)
    }
    fn hull(self, o: f64) -> Proven<f64> {
        if self == o { Ok(self) } else { Err(Unproven::Uncertified) }
    }
    fn neg(self) -> f64 {
        -self
    }
    fn add(self, o: f64) -> Proven<f64> {
        Ok(self + o)
    }
    fn sub(self, o: f64) -> Proven<f64> {
        Ok(self - o)
    }
    fn mul(self, o: f64) -> Proven<f64> {
        Ok(self * o)
    }
    fn div(self, o: f64) -> Proven<f64> {
        Ok(self / o)
    }
    fn pow(self, e: f64) -> Proven<f64> {
        Ok(self.powf(e))
    }
    fn sqrt(self) -> Proven<f64> {
        Ok(f64::sqrt(self))
    }
    fn exp(self) -> Proven<f64> {
        Ok(f64::exp(self))
    }
    fn ln(self) -> Proven<f64> {
        Ok(f64::ln(self))
    }
    fn log10(self) -> Proven<f64> {
        Ok(f64::log10(self))
    }
    fn sin(self) -> Proven<f64> {
        Ok(f64::sin(self))
    }
    fn cos(self) -> Proven<f64> {
        Ok(f64::cos(self))
    }
    fn tan(self) -> Proven<f64> {
        Ok(f64::tan(self))
    }
    fn asin(self) -> Proven<f64> {
        Ok(f64::asin(self))
    }
    fn acos(self) -> Proven<f64> {
        Ok(f64::acos(self))
    }
    fn atan(self) -> Proven<f64> {
        Ok(f64::atan(self))
    }
    fn atan2(self, x: f64) -> Proven<f64> {
        Ok(f64::atan2(self, x))
    }
    fn sinh(self) -> Proven<f64> {
        Ok(f64::sinh(self))
    }
    fn cosh(self) -> Proven<f64> {
        Ok(f64::cosh(self))
    }
    fn tanh(self) -> Proven<f64> {
        Ok(f64::tanh(self))
    }
    fn asinh(self) -> Proven<f64> {
        Ok(f64::asinh(self))
    }
    fn acosh(self) -> Proven<f64> {
        Ok(f64::acosh(self))
    }
    fn atanh(self) -> Proven<f64> {
        Ok(f64::atanh(self))
    }
    fn hypot(self, o: f64) -> Proven<f64> {
        Ok(f64::hypot(self, o))
    }
    fn abs(self) -> Proven<f64> {
        Ok(f64::abs(self))
    }
    fn signum(self) -> Proven<f64> {
        Ok(f64::signum(self))
    }
    fn floor(self) -> Proven<f64> {
        Ok(f64::floor(self))
    }
    fn ceil(self) -> Proven<f64> {
        Ok(f64::ceil(self))
    }
    fn round(self) -> Proven<f64> {
        Ok(f64::round(self))
    }
    fn trunc(self) -> Proven<f64> {
        Ok(f64::trunc(self))
    }
    fn rem_euclid(self, o: f64) -> Proven<f64> {
        Ok(f64::rem_euclid(self, o))
    }
    fn min(self, o: f64) -> Proven<f64> {
        Ok(if self < o { self } else { o })
    }
    fn max(self, o: f64) -> Proven<f64> {
        Ok(if self > o { self } else { o })
    }
}

/// The range: every result holds the function's exact value at every number the arguments
/// hold.
impl Real for Interval {
    fn exact(x: f64) -> Proven<Interval> {
        elementary::finite(Interval::point(x))
    }
    fn cmp_f64(self, c: f64) -> Option<Ordering> {
        if self.lo > c {
            Some(Ordering::Greater)
        } else if self.hi < c {
            Some(Ordering::Less)
        } else if self.lo == c && self.hi == c {
            Some(Ordering::Equal)
        } else {
            None
        }
    }
    fn as_point(self) -> Option<f64> {
        Interval::as_point(self)
    }
    fn hull(self, o: Interval) -> Proven<Interval> {
        Ok(Interval::hull(self, o))
    }
    fn neg(self) -> Interval {
        -self
    }
    fn add(self, o: Interval) -> Proven<Interval> {
        elementary::finite(self + o)
    }
    fn sub(self, o: Interval) -> Proven<Interval> {
        elementary::finite(self - o)
    }
    fn mul(self, o: Interval) -> Proven<Interval> {
        elementary::finite(self * o)
    }
    fn div(self, o: Interval) -> Proven<Interval> {
        if o.lo <= 0.0 && o.hi >= 0.0 {
            return Err(Unproven::Domain);
        }
        elementary::finite(self / o)
    }
    fn pow(self, e: Interval) -> Proven<Interval> {
        elementary::pow(self, e)
    }
    fn sqrt(self) -> Proven<Interval> {
        elementary::sqrt(self)
    }
    fn exp(self) -> Proven<Interval> {
        elementary::exp(self)
    }
    fn ln(self) -> Proven<Interval> {
        elementary::ln(self)
    }
    fn log10(self) -> Proven<Interval> {
        elementary::log10(self)
    }
    fn sin(self) -> Proven<Interval> {
        elementary::sin(self)
    }
    fn cos(self) -> Proven<Interval> {
        elementary::cos(self)
    }
    fn tan(self) -> Proven<Interval> {
        elementary::tan(self)
    }
    fn asin(self) -> Proven<Interval> {
        elementary::asin(self)
    }
    fn acos(self) -> Proven<Interval> {
        elementary::acos(self)
    }
    fn atan(self) -> Proven<Interval> {
        elementary::atan(self)
    }
    fn atan2(self, x: Interval) -> Proven<Interval> {
        elementary::atan2(self, x)
    }
    fn sinh(self) -> Proven<Interval> {
        elementary::sinh(self)
    }
    fn cosh(self) -> Proven<Interval> {
        elementary::cosh(self)
    }
    fn tanh(self) -> Proven<Interval> {
        elementary::tanh(self)
    }
    fn asinh(self) -> Proven<Interval> {
        elementary::asinh(self)
    }
    fn acosh(self) -> Proven<Interval> {
        elementary::acosh(self)
    }
    fn atanh(self) -> Proven<Interval> {
        elementary::atanh(self)
    }
    fn hypot(self, o: Interval) -> Proven<Interval> {
        elementary::hypot(self, o)
    }
    fn abs(self) -> Proven<Interval> {
        Ok(Interval::abs(self))
    }
    fn signum(self) -> Proven<Interval> {
        elementary::signum(self)
    }
    fn floor(self) -> Proven<Interval> {
        elementary::floor(self)
    }
    fn ceil(self) -> Proven<Interval> {
        elementary::ceil(self)
    }
    fn round(self) -> Proven<Interval> {
        elementary::round(self)
    }
    fn trunc(self) -> Proven<Interval> {
        elementary::trunc(self)
    }
    fn rem_euclid(self, o: Interval) -> Proven<Interval> {
        elementary::rem_euclid(self, o)
    }
    fn min(self, o: Interval) -> Proven<Interval> {
        Ok(elementary::min(self, o))
    }
    fn max(self, o: Interval) -> Proven<Interval> {
        Ok(elementary::max(self, o))
    }
}
