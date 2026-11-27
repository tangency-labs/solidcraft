//! The transcendental functions, called through one module.
//!
//! The kernel calls these instead of `f64::sin`, `f64::exp`, `f64::powf` and the other
//! transcendental methods of `std`. Each one calls the `std` method of the same name, so it
//! returns the bits `std` returns. `std` calls the platform's C math library: Apple's libm, glibc
//! (whose `x86_64` build picks an FMA variant by CPU), the MSVC CRT and wasm's libm round some
//! arguments differently, by one to a few ulps, so the same design built on two machines can
//! differ. With every call here, one pinned libm behind these functions gives every platform the
//! same bits (#13866).
//!
//! `sqrt`, `mul_add`, `abs`, `floor`, `ceil`, `round`, `trunc`, `min`, `max`, `copysign` and
//! `rem_euclid` stay `std`'s: IEEE 754 rounds them correctly, or they are exact, so every
//! platform agrees on them already. (`min` and `max` may return either zero for `-0.0` and
//! `0.0`; a value that must not depend on the sign of a zero compares instead.)
//!
//! Not upstream (SMLib calls the platform's C library). Deviations: none. No recursion.

/// `sin x`.
#[inline]
#[must_use]
pub fn sin(x: f64) -> f64 {
    x.sin()
}

/// `cos x`.
#[inline]
#[must_use]
pub fn cos(x: f64) -> f64 {
    x.cos()
}

/// `(sin x, cos x)`.
#[inline]
#[must_use]
pub fn sin_cos(x: f64) -> (f64, f64) {
    x.sin_cos()
}

/// `tan x`.
#[inline]
#[must_use]
pub fn tan(x: f64) -> f64 {
    x.tan()
}

/// `asin x`, in `[-pi/2, pi/2]`; NaN outside `[-1, 1]`.
#[inline]
#[must_use]
pub fn asin(x: f64) -> f64 {
    x.asin()
}

/// `acos x`, in `[0, pi]`; NaN outside `[-1, 1]`.
#[inline]
#[must_use]
pub fn acos(x: f64) -> f64 {
    x.acos()
}

/// `atan x`, in `[-pi/2, pi/2]`.
#[inline]
#[must_use]
pub fn atan(x: f64) -> f64 {
    x.atan()
}

/// The angle of `(x, y)`, in `[-pi, pi]` (`y.atan2(x)` of `std`: `y` first).
#[inline]
#[must_use]
pub fn atan2(y: f64, x: f64) -> f64 {
    y.atan2(x)
}

/// `sinh x`.
#[inline]
#[must_use]
pub fn sinh(x: f64) -> f64 {
    x.sinh()
}

/// `cosh x`.
#[inline]
#[must_use]
pub fn cosh(x: f64) -> f64 {
    x.cosh()
}

/// `tanh x`.
#[inline]
#[must_use]
pub fn tanh(x: f64) -> f64 {
    x.tanh()
}

/// `asinh x`.
#[inline]
#[must_use]
pub fn asinh(x: f64) -> f64 {
    x.asinh()
}

/// `acosh x`; NaN below 1.
#[inline]
#[must_use]
pub fn acosh(x: f64) -> f64 {
    x.acosh()
}

/// `atanh x`; NaN outside `[-1, 1]`.
#[inline]
#[must_use]
pub fn atanh(x: f64) -> f64 {
    x.atanh()
}

/// `e^x`.
#[inline]
#[must_use]
pub fn exp(x: f64) -> f64 {
    x.exp()
}

/// `2^x`.
#[inline]
#[must_use]
pub fn exp2(x: f64) -> f64 {
    x.exp2()
}

/// `e^x - 1`, accurate near 0.
#[inline]
#[must_use]
pub fn exp_m1(x: f64) -> f64 {
    x.exp_m1()
}

/// The natural logarithm.
#[inline]
#[must_use]
pub fn ln(x: f64) -> f64 {
    x.ln()
}

/// `ln(1 + x)`, accurate near 0.
#[inline]
#[must_use]
pub fn ln_1p(x: f64) -> f64 {
    x.ln_1p()
}

/// The base-2 logarithm.
#[inline]
#[must_use]
pub fn log2(x: f64) -> f64 {
    x.log2()
}

/// The base-10 logarithm.
#[inline]
#[must_use]
pub fn log10(x: f64) -> f64 {
    x.log10()
}

/// The logarithm of `x` to `base`.
#[inline]
#[must_use]
pub fn log(x: f64, base: f64) -> f64 {
    x.log(base)
}

/// `x^y`.
#[inline]
#[must_use]
pub fn powf(x: f64, y: f64) -> f64 {
    x.powf(y)
}

/// `x^n` for an integer `n`.
#[inline]
#[must_use]
pub fn powi(x: f64, n: i32) -> f64 {
    x.powi(n)
}

/// The cube root.
#[inline]
#[must_use]
pub fn cbrt(x: f64) -> f64 {
    x.cbrt()
}

/// `sqrt(x^2 + y^2)` without overflow or underflow in the squares.
#[inline]
#[must_use]
pub fn hypot(x: f64, y: f64) -> f64 {
    x.hypot(y)
}
