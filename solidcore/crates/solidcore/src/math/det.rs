//! The transcendental functions, the same bits on every platform.
//!
//! `f64::sin`, `f64::exp`, `f64::powf` and the other transcendental methods of `std` call the
//! platform's C math library: Apple's libm, glibc (whose `x86_64` build picks an FMA variant by
//! CPU), the MSVC CRT and wasi-libc round some arguments differently, by one to a few ulps. A
//! moved last bit moves a Newton iterate, a refusal or the order of a sort, so the same design
//! built on two machines could differ. Every crate of the workspace calls these functions
//! instead. They are the pure-Rust `libm` crate (a port of musl, itself FreeBSD's msun, and
//! CORE-MATH for `cbrt`), pinned to one version, so every target computes the same bits.
//! `clippy.toml` disallows the `std` methods, so a new call to one fails the lint gate.
//!
//! `sqrt`, `mul_add`, `abs`, `floor`, `ceil`, `round`, `trunc`, `min`, `max`, `copysign` and
//! `rem_euclid` stay `std`'s: IEEE 754 rounds them correctly, or they are exact, so every
//! platform agrees on them already. (`min` and `max` may return either zero for `-0.0` and
//! `0.0`; a value that must not depend on the sign of a zero compares instead.)
//!
//! ## Error bounds
//!
//! The largest error, in ulps of the correctly rounded result, that the `libm` crate's test
//! suite allows against MPFR for `f64` (`libm-test/src/precision.rs`, `default_ulp`, at the
//! pinned release). A bound in the kernel that takes a function's error cites this table.
//!
//! | Function                      | Bound                                          |
//! |-------------------------------|------------------------------------------------|
//! | [`sin`], [`cos`], [`sin_cos`] | 1 ulp                                          |
//! | [`tan`]                       | 1 ulp                                          |
//! | [`asin`], [`acos`], [`atan`]  | 1 ulp                                          |
//! | [`atan2`]                     | 2 ulp                                          |
//! | [`sinh`], [`tanh`]            | 2 ulp                                          |
//! | [`cosh`]                      | 1 ulp                                          |
//! | [`asinh`], [`atanh`]          | 2 ulp                                          |
//! | [`acosh`]                     | 4 ulp                                          |
//! | [`exp`], [`exp2`], [`exp_m1`] | 1 ulp                                          |
//! | [`ln`], [`ln_1p`]             | 1 ulp                                          |
//! | [`log2`], [`log10`]           | 1 ulp                                          |
//! | [`log`]                       | two [`ln`]s and a division                     |
//! | [`powf`]                      | 1 ulp                                          |
//! | [`hypot`]                     | 1 ulp                                          |
//! | [`cbrt`]                      | 0 (correctly rounded, CORE-MATH)               |
//! | [`powi`]                      | `|n| - 1` products, and a division for `n < 0` |
//!
//! [`powi`] is not `libm`'s `pow`: it multiplies, in the order compiler-rt's `__powidf2` does,
//! so it gives the bits `f64::powi` gives today; `std` leaves `f64::powi`'s precision
//! unspecified and free to change between platforms and releases, so it is pinned here too.
//!
//! Not upstream (SMLib calls the platform's C library). Deviations: none. No recursion.

/// `sin x`.
#[inline]
#[must_use]
pub fn sin(x: f64) -> f64 {
    libm::sin(x)
}

/// `cos x`.
#[inline]
#[must_use]
pub fn cos(x: f64) -> f64 {
    libm::cos(x)
}

/// `(sin x, cos x)`: the two functions' own values, as `f64::sin_cos` is.
#[inline]
#[must_use]
pub fn sin_cos(x: f64) -> (f64, f64) {
    (libm::sin(x), libm::cos(x))
}

/// `tan x`.
#[inline]
#[must_use]
pub fn tan(x: f64) -> f64 {
    libm::tan(x)
}

/// `asin x`, in `[-pi/2, pi/2]`; NaN outside `[-1, 1]`.
#[inline]
#[must_use]
pub fn asin(x: f64) -> f64 {
    libm::asin(x)
}

/// `acos x`, in `[0, pi]`; NaN outside `[-1, 1]`.
#[inline]
#[must_use]
pub fn acos(x: f64) -> f64 {
    libm::acos(x)
}

/// `atan x`, in `[-pi/2, pi/2]`.
#[inline]
#[must_use]
pub fn atan(x: f64) -> f64 {
    libm::atan(x)
}

/// The angle of `(x, y)`, in `[-pi, pi]` (`y.atan2(x)` of `std`: `y` first).
#[inline]
#[must_use]
pub fn atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}

/// `sinh x`.
#[inline]
#[must_use]
pub fn sinh(x: f64) -> f64 {
    libm::sinh(x)
}

/// `cosh x`.
#[inline]
#[must_use]
pub fn cosh(x: f64) -> f64 {
    libm::cosh(x)
}

/// `tanh x`.
#[inline]
#[must_use]
pub fn tanh(x: f64) -> f64 {
    libm::tanh(x)
}

/// `asinh x`.
#[inline]
#[must_use]
pub fn asinh(x: f64) -> f64 {
    libm::asinh(x)
}

/// `acosh x`; NaN below 1.
#[inline]
#[must_use]
pub fn acosh(x: f64) -> f64 {
    libm::acosh(x)
}

/// `atanh x`; NaN outside `[-1, 1]`.
#[inline]
#[must_use]
pub fn atanh(x: f64) -> f64 {
    libm::atanh(x)
}

/// `e^x`.
#[inline]
#[must_use]
pub fn exp(x: f64) -> f64 {
    libm::exp(x)
}

/// `2^x`.
#[inline]
#[must_use]
pub fn exp2(x: f64) -> f64 {
    libm::exp2(x)
}

/// `e^x - 1`, accurate near 0.
#[inline]
#[must_use]
pub fn exp_m1(x: f64) -> f64 {
    libm::expm1(x)
}

/// The natural logarithm.
#[inline]
#[must_use]
pub fn ln(x: f64) -> f64 {
    libm::log(x)
}

/// `ln(1 + x)`, accurate near 0.
#[inline]
#[must_use]
pub fn ln_1p(x: f64) -> f64 {
    libm::log1p(x)
}

/// The base-2 logarithm.
#[inline]
#[must_use]
pub fn log2(x: f64) -> f64 {
    libm::log2(x)
}

/// The base-10 logarithm.
#[inline]
#[must_use]
pub fn log10(x: f64) -> f64 {
    libm::log10(x)
}

/// The logarithm of `x` to `base`: `ln x / ln base`, as `std`'s `f64::log` computes it.
#[inline]
#[must_use]
pub fn log(x: f64, base: f64) -> f64 {
    libm::log(x) / libm::log(base)
}

/// `x^y`.
#[inline]
#[must_use]
pub fn powf(x: f64, y: f64) -> f64 {
    libm::pow(x, y)
}

/// The cube root, correctly rounded.
#[inline]
#[must_use]
pub fn cbrt(x: f64) -> f64 {
    libm::cbrt(x)
}

/// `sqrt(x^2 + y^2)` without overflow or underflow in the squares.
#[inline]
#[must_use]
pub fn hypot(x: f64, y: f64) -> f64 {
    libm::hypot(x, y)
}

/// `x^n` by repeated squaring, in compiler-rt's `__powidf2` order: the product starts at 1 and
/// takes the current square at each set bit of `|n|` from the lowest, and `1 / product` is
/// returned for `n < 0`. Integer arithmetic and IEEE products only, so it is the same on every
/// platform.
#[inline]
#[must_use]
pub fn powi(x: f64, n: i32) -> f64 {
    let reciprocal = n < 0;
    let mut square = x;
    let mut b = n;
    let mut product = 1.0;
    loop {
        if b & 1 != 0 {
            product *= square;
        }
        b /= 2;
        if b == 0 {
            break;
        }
        square *= square;
    }
    if reciprocal {
        1.0 / product
    } else {
        product
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `powi` gives `std`'s bits (compiler-rt's order) for every small exponent, both signs.
    #[test]
    #[allow(clippy::disallowed_methods)] // the comparison is against std's powi
    fn powi_is_compiler_rts_order() {
        for &x in &[0.1, 1.0 / 3.0, -2.75, 1.0e-30, 7.123_456_789, -0.0, 3.0e200] {
            for n in -40..=40 {
                let (a, b) = (powi(x, n), x.powi(n));
                assert!(
                    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan()),
                    "{x}^{n}: {a} vs {b}"
                );
            }
        }
        assert_eq!(powi(2.0, i32::MIN), 0.0);
        assert_eq!(powi(0.5, i32::MIN), f64::INFINITY);
    }

    /// Values of the pinned `libm`, the same bits on every platform: most correctly rounded,
    /// `exp(1)` one ulp above `e` (within its one-ulp bound).
    #[test]
    fn fixed_values() {
        use core::f64::consts::{E, FRAC_PI_4, LN_10};
        assert_eq!(sin(1.0).to_bits(), 0.841_470_984_807_896_5_f64.to_bits());
        assert_eq!(exp(1.0).to_bits(), E.to_bits() + 1);
        assert_eq!(ln(10.0).to_bits(), LN_10.to_bits());
        assert_eq!(atan2(1.0, 1.0).to_bits(), FRAC_PI_4.to_bits());
        assert_eq!(cbrt(27.0), 3.0);
        assert_eq!(hypot(3.0, 4.0), 5.0);
        assert_eq!(powf(2.0, 10.0), 1024.0);
        assert_eq!(sin_cos(0.5), (sin(0.5), cos(0.5)));
    }
}
