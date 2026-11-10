//! Error-free arithmetic on doubles: a value is an expansion, a sum of doubles whose
//! components do not overlap (Shewchuk's grow-expansion), so sums and products of doubles
//! are held without rounding and a value's sign is read off exactly (an expansion is zero
//! exactly when every component is).
//!
//! A sum (`two_sum`) is error-free at any magnitude short of overflow, which leaves an
//! infinite or NaN component, never a zero. A product (`product`) is error-free only in
//! range: a product that could underflow, or of a subnormal or non-finite factor, is
//! `None`, and so is every value built from it. A caller reads `None` as not shown zero.
//!
//! Upstream: none (Shewchuk, "Adaptive Precision Floating-Point Arithmetic and Fast
//! Robust Geometric Predicates", 1997; moved here from the local operation's private
//! module).
//!
//! Deviations: no zero elimination; out-of-range products are `None` rather than an
//! inexact value. No recursion. Shared by the local operation's exact plane and axis
//! tests and the native affine trim's exact corner products.

use crate::math::Vec3;

/// `2^-968`: the least rounded product of two normal doubles whose rounding error
/// `fma(a, b, -a b)` is exact. The error is a multiple of `ulp(a) ulp(b) =
/// 2^(ea + eb - 104)` (`ea`, `eb` the exponents) of at most 53 bits, so exact when
/// that unit is not below the least subnormal `2^-1074`, `ea + eb >= -970`; a rounded
/// product of at least `2^-968` has `|a b| > 2^-969` and, as `|a b| < 2^(ea + eb + 2)`,
/// `ea + eb >= -970`.
const LEAST_EXACT_PRODUCT: f64 = f64::MIN_POSITIVE * 18_014_398_509_481_984.0;

/// `a + b` as `(s, e)`: `s = fl(a + b)` and its rounding error (Knuth's `TwoSum`).
fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    let bb = s - a;
    (s, (a - (s - bb)) + (b - bb))
}

/// `e + b` as an expansion (Grow-Expansion).
fn grow(e: &[f64], b: f64) -> Vec<f64> {
    let mut q = b;
    let mut out = Vec::new();
    for &c in e {
        let (s, err) = two_sum(q, c);
        out.push(err);
        q = s;
    }
    out.push(q);
    out
}

/// `e + f` as an expansion.
pub(crate) fn add(e: &[f64], f: &[f64]) -> Vec<f64> {
    f.iter().fold(e.to_vec(), |acc, &c| grow(&acc, c))
}

/// `e - f` as an expansion.
pub(crate) fn sub(e: &[f64], f: &[f64]) -> Vec<f64> {
    f.iter().fold(e.to_vec(), |acc, &c| grow(&acc, -c))
}

/// `a b` as an expansion: the product and its rounding error, `fma(a, b, -p)`; zero
/// exactly when `a` or `b` is. `None` out of range: `a` or `b` subnormal or not
/// finite, or the rounded product not finite or below [`LEAST_EXACT_PRODUCT`] (where
/// the error, or the product itself, could underflow).
pub(crate) fn product(a: f64, b: f64) -> Option<Vec<f64>> {
    if a == 0.0 || b == 0.0 {
        return (a.is_finite() && b.is_finite()).then(|| vec![0.0]);
    }
    let p = a * b;
    let in_range =
        a.is_normal() && b.is_normal() && p.is_finite() && p.abs() >= LEAST_EXACT_PRODUCT;
    in_range.then(|| vec![a.mul_add(b, -p), p])
}

/// `e b` as an expansion: each component's product, summed.
pub(crate) fn scale(e: &[f64], b: f64) -> Option<Vec<f64>> {
    e.iter()
        .try_fold(Vec::new(), |acc, &c| Some(add(&acc, &product(c, b)?)))
}

/// `u × v`, each component an expansion.
pub(crate) fn cross(u: Vec3, v: Vec3) -> Option<[Vec<f64>; 3]> {
    Some([
        sub(&product(u.y, v.z)?, &product(u.z, v.y)?),
        sub(&product(u.z, v.x)?, &product(u.x, v.z)?),
        sub(&product(u.x, v.y)?, &product(u.y, v.x)?),
    ])
}

/// `u · v` as an expansion.
pub(crate) fn dot(u: Vec3, v: Vec3) -> Option<Vec<f64>> {
    Some(add(
        &add(&product(u.x, v.x)?, &product(u.y, v.y)?),
        &product(u.z, v.z)?,
    ))
}

/// `det(a, b, c) = (a × b) · c` as an expansion.
pub(crate) fn det3(a: Vec3, b: Vec3, c: Vec3) -> Option<Vec<f64>> {
    let [x, y, z] = cross(a, b)?;
    Some(add(
        &add(&scale(&x, c.x)?, &scale(&y, c.y)?),
        &scale(&z, c.z)?,
    ))
}

/// Whether the expansion is zero exactly.
pub(crate) fn is_zero(e: &[f64]) -> bool {
    e.iter().all(|&c| c == 0.0)
}

/// The expansion's exact sign: `-1`, `0` or `1`. The components do not overlap and
/// are stored least significant first, so the most significant non-zero one's sign
/// is the value's.
pub(crate) fn sign(e: &[f64]) -> i32 {
    match e.iter().rev().find(|&&c| c != 0.0) {
        Some(c) if *c > 0.0 => 1,
        Some(_) => -1,
        None => 0,
    }
}

/// The expansion's value as a double, rounded by summing its components from the most
/// significant down. Not correctly rounded and not canonical: a partial sum can
/// round, and a later component's contribution can then be lost (non-overlapping
/// components can straddle a tie). With `r` non-zero components and no intermediate
/// overflow the error is at most `gamma_(r-1) * sum |e|`, where
/// `gamma_j = j u / (1 - j u)` and `u = 2^-53`; adding a zero is exact, and an exact
/// sum of doubles is a multiple of the least subnormal, so the fold itself has no
/// subnormal underflow. Callers use it for a quotient or a displacement, a
/// rounding-level estimate, and read exactness from [`is_zero`] and [`sign`], never
/// from this value.
pub(crate) fn to_f64(e: &[f64]) -> f64 {
    e.iter().rev().fold(0.0, |sum, &c| sum + c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rounded_sum_is_not_zero_but_its_exact_one_is() {
        // 0.1 + 0.2 - 0.3 rounds to 5.6e-17; exactly it is that too (the doubles are
        // not the decimals), while 1e16 + 1 - 1e16 is 1, not the 0 or 2 of rounding
        assert!(!is_zero(&sub(&add(&[0.1], &[0.2]), &[0.3])));
        assert_eq!(sub(&add(&[1e16], &[1.0]), &[1e16]).iter().sum::<f64>(), 1.0);
        let (p, q) = (product(3.0, 0.1).unwrap(), product(0.1, 3.0).unwrap());
        assert!(is_zero(&sub(&p, &q)));
    }

    #[test]
    fn a_determinant_is_zero_exactly_only_for_dependent_vectors() {
        let a = Vec3::new(0.1, 0.7, 0.3);
        let b = Vec3::new(0.9, 0.2, 0.4);
        assert!(is_zero(&det3(a, b, a).unwrap()));
        let c = Vec3::new(0.1, 0.7, 0.3 + 1e-16 * 0.3);
        assert!(!is_zero(&det3(a, b, c).unwrap()));
    }

    /// A product whose rounding error is not a double is out of range: one that
    /// underflows (rounded to zero, its `fma` error zero too, so it read as zero), one
    /// of a subnormal factor, one that overflows. A zero factor's product is zero, and
    /// one just in range is exact.
    #[test]
    fn a_product_that_could_underflow_is_out_of_range() {
        assert_eq!(LEAST_EXACT_PRODUCT, 2.0_f64.powi(-968));
        assert!(product(1e-200, 1e-200).is_none());
        assert!(product(1e-160, 1e-160).is_none());
        assert!(product(1e-310, 1.0).is_none());
        assert!(product(1.0, -5e-324).is_none());
        assert!(product(f64::MAX, 2.0).is_none());
        assert!(product(f64::NAN, 1.0).is_none());
        assert_eq!(product(0.0, 1e-310), Some(vec![0.0]));
        let h = 2.0_f64.powi(-484);
        assert_eq!(product(h, h), Some(vec![0.0, LEAST_EXACT_PRODUCT]));
        assert!(product(1e-140, 1e-140).is_some());
        let z = Vec3::new(0.0, 0.0, 1e-310);
        assert!(det3(Vec3::X, Vec3::Y, z).is_none());
    }
}
