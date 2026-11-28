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
//! Deviations: out-of-range products are `None` rather than an inexact value. Zero
//! components are eliminated in-place during grow-expansion (Shewchuk 1997, Theorem 12)
//! while preserving exact non-overlapping sums. No recursion. Shared by the local
//! operation's exact plane and axis tests and the native affine trim's exact corner products.

use crate::math::Vec3;

/// `2^-968`: the least rounded product of two normal doubles whose rounding error
/// `fma(a, b, -a b)` is exact. The error is a multiple of `ulp(a) ulp(b) =
/// 2^(ea + eb - 104)` (`ea`, `eb` the exponents) of at most 53 bits, so exact when
/// that unit is not below the least subnormal `2^-1074`, `ea + eb >= -970`; a rounded
/// product of at least `2^-968` has `|a b| > 2^-969` and, as `|a b| < 2^(ea + eb + 2)`,
/// `ea + eb >= -970`.
const LEAST_EXACT_PRODUCT: f64 = f64::MIN_POSITIVE * 18_014_398_509_481_984.0;

/// `a + b` as `(s, e)`: `s = fl(a + b)` and its rounding error (Knuth's `TwoSum`).
#[inline]
fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    let bb = s - a;
    (s, (a - (s - bb)) + (b - bb))
}

/// `a b` as `(err, prod)` on the stack without allocating: `(0.0, 0.0)` when either factor
/// is zero (and both are finite), or `(fma(a, b, -p), p)` when both are normal and `|p|` is
/// in range (`>= LEAST_EXACT_PRODUCT`). `None` out of range.
#[inline]
pub(crate) fn product_pair(a: f64, b: f64) -> Option<(f64, f64)> {
    if a == 0.0 || b == 0.0 {
        return (a.is_finite() && b.is_finite()).then_some((0.0, 0.0));
    }
    let p = a * b;
    let in_range =
        a.is_normal() && b.is_normal() && p.is_finite() && p.abs() >= LEAST_EXACT_PRODUCT;
    in_range.then(|| (a.mul_add(b, -p), p))
}

/// Adds scalar `b` into expansion `acc` in place (Shewchuk's Grow-Expansion with zero
/// elimination): exact sum preserved, non-overlapping components, zero intermediate heap
/// allocations.
#[inline]
pub(crate) fn grow_into(acc: &mut Vec<f64>, b: f64) {
    if b == 0.0 {
        return;
    }
    let mut q = b;
    let mut write = 0usize;
    let len = acc.len();
    for read in 0..len {
        let Some(&c) = acc.get(read) else {
            break;
        };
        if c == 0.0 {
            continue;
        }
        let (s, err) = two_sum(q, c);
        if err != 0.0 {
            if let Some(slot) = acc.get_mut(write) {
                *slot = err;
            }
            write += 1;
        }
        q = s;
    }
    acc.truncate(write);
    if q != 0.0 {
        acc.push(q);
    }
}

/// Adds every non-zero component of `f` into `acc` in place.
#[inline]
pub(crate) fn add_into(acc: &mut Vec<f64>, f: &[f64]) {
    for &c in f {
        grow_into(acc, c);
    }
}

/// Subtracts every non-zero component of `f` from `acc` in place.
#[inline]
pub(crate) fn sub_into(acc: &mut Vec<f64>, f: &[f64]) {
    for &c in f {
        grow_into(acc, -c);
    }
}

/// Adds `b` into `acc` in place with zero elimination, returning `None` if `b` is not finite
/// or if any intermediate sum overflows to a non-finite value.
#[inline]
pub(crate) fn grow_into_checked(acc: &mut Vec<f64>, b: f64) -> Option<()> {
    if !b.is_finite() {
        return None;
    }
    if b == 0.0 {
        return Some(());
    }
    let mut q = b;
    let mut write = 0usize;
    let n = acc.len();
    for read in 0..n {
        let Some(&c) = acc.get(read) else {
            break;
        };
        if c == 0.0 {
            continue;
        }
        let (s, err) = two_sum(q, c);
        if !s.is_finite() || !err.is_finite() {
            return None;
        }
        if err != 0.0 {
            if let Some(slot) = acc.get_mut(write) {
                *slot = err;
            }
            write += 1;
        }
        q = s;
    }
    acc.truncate(write);
    if q != 0.0 {
        acc.try_reserve(1).ok()?;
        acc.push(q);
    }
    Some(())
}

/// Adds `e * b` into `acc` in place without allocating intermediate expansions.
#[inline]
pub(crate) fn add_scaled_into(acc: &mut Vec<f64>, e: &[f64], b: f64) -> Option<()> {
    if !b.is_finite() {
        return None;
    }
    if b == 0.0 {
        return e.iter().all(|c| c.is_finite()).then_some(());
    }
    for &c in e {
        let (err, prod) = product_pair(c, b)?;
        grow_into_checked(acc, err)?;
        grow_into_checked(acc, prod)?;
    }
    Some(())
}

/// Adds `e * f` into `acc` in place without allocating intermediate expansions.
#[inline]
pub(crate) fn add_mul_into(acc: &mut Vec<f64>, e: &[f64], f: &[f64]) -> Option<()> {
    for &b in f {
        add_scaled_into(acc, e, b)?;
    }
    Some(())
}

/// `e + f` as an expansion.
pub(crate) fn add(e: &[f64], f: &[f64]) -> Vec<f64> {
    let mut acc = Vec::new();
    let _ = acc.try_reserve(e.len().saturating_add(f.len()));
    for &c in e {
        if c != 0.0 {
            acc.push(c);
        }
    }
    add_into(&mut acc, f);
    acc
}

/// `e - f` as an expansion.
pub(crate) fn sub(e: &[f64], f: &[f64]) -> Vec<f64> {
    let mut acc = Vec::new();
    let _ = acc.try_reserve(e.len().saturating_add(f.len()));
    for &c in e {
        if c != 0.0 {
            acc.push(c);
        }
    }
    sub_into(&mut acc, f);
    acc
}

/// `a b` as an expansion: the product and its rounding error, `fma(a, b, -p)`; zero
/// exactly when `a` or `b` is. `None` out of range: `a` or `b` subnormal or not
/// finite, or the rounded product not finite or below [`LEAST_EXACT_PRODUCT`] (where
/// the error, or the product itself, could underflow).
pub(crate) fn product(a: f64, b: f64) -> Option<Vec<f64>> {
    if a == 0.0 || b == 0.0 {
        return (a.is_finite() && b.is_finite()).then(|| vec![0.0]);
    }
    let (err, p) = product_pair(a, b)?;
    Some(vec![err, p])
}

/// `e b` as an expansion: each component's product, summed.
pub(crate) fn scale(e: &[f64], b: f64) -> Option<Vec<f64>> {
    if !b.is_finite() {
        return None;
    }
    if b == 0.0 {
        return e.iter().all(|c| c.is_finite()).then(Vec::new);
    }
    let mut acc = Vec::new();
    acc.try_reserve(e.len().saturating_mul(2)).ok()?;
    add_scaled_into(&mut acc, e, b)?;
    Some(acc)
}

/// `a1 * b1 - a2 * b2` as an expansion without intermediate allocations.
#[inline]
fn diff_products(a1: f64, b1: f64, a2: f64, b2: f64) -> Option<Vec<f64>> {
    let (e1, p1) = product_pair(a1, b1)?;
    let (e2, p2) = product_pair(a2, b2)?;
    let mut acc = Vec::new();
    acc.try_reserve(4).ok()?;
    grow_into_checked(&mut acc, e1)?;
    grow_into_checked(&mut acc, p1)?;
    grow_into_checked(&mut acc, -e2)?;
    grow_into_checked(&mut acc, -p2)?;
    Some(acc)
}

/// `u × v`, each component an expansion.
pub(crate) fn cross(u: Vec3, v: Vec3) -> Option<[Vec<f64>; 3]> {
    Some([
        diff_products(u.y, v.z, u.z, v.y)?,
        diff_products(u.z, v.x, u.x, v.z)?,
        diff_products(u.x, v.y, u.y, v.x)?,
    ])
}

/// `u · v` as an expansion.
pub(crate) fn dot(u: Vec3, v: Vec3) -> Option<Vec<f64>> {
    let (ex, px) = product_pair(u.x, v.x)?;
    let (ey, py) = product_pair(u.y, v.y)?;
    let (ez, pz) = product_pair(u.z, v.z)?;
    let mut acc = Vec::new();
    acc.try_reserve(6).ok()?;
    grow_into_checked(&mut acc, ex)?;
    grow_into_checked(&mut acc, px)?;
    grow_into_checked(&mut acc, ey)?;
    grow_into_checked(&mut acc, py)?;
    grow_into_checked(&mut acc, ez)?;
    grow_into_checked(&mut acc, pz)?;
    Some(acc)
}

/// `det(a, b, c) = (a × b) · c` as an expansion.
pub(crate) fn det3(a: Vec3, b: Vec3, c: Vec3) -> Option<Vec<f64>> {
    let [x, y, z] = cross(a, b)?;
    let mut acc = Vec::new();
    acc.try_reserve(12).ok()?;
    add_scaled_into(&mut acc, &x, c.x)?;
    add_scaled_into(&mut acc, &y, c.y)?;
    add_scaled_into(&mut acc, &z, c.z)?;
    Some(acc)
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

    #[test]
    fn zero_elimination_keeps_expansions_compact_and_exact() {
        let mut acc = Vec::new();
        for _ in 0..32 {
            grow_into(&mut acc, 1.0);
            grow_into(&mut acc, -1.0);
        }
        assert!(acc.is_empty());
        assert!(is_zero(&acc));
        assert_eq!(sign(&acc), 0);

        let e = add(&[1e16], &[1.0]);
        let f = sub(&[0.3], &[0.1]);
        let mut prod = Vec::new();
        add_mul_into(&mut prod, &e, &f).unwrap();
        // Exact comparison: (1e16 + 1) * (0.3 - 0.1) == 1e16*(0.3-0.1) + (0.3-0.1)
        let mut ref_p = scale(&f, 1e16).unwrap();
        add_into(&mut ref_p, &f);
        assert!(is_zero(&sub(&prod, &ref_p)));
        assert!(
            prod.len() <= 8,
            "expected compact expansion, got {}",
            prod.len()
        );
    }

    #[test]
    fn non_finite_scales_and_accumulation_overflow_return_none() {
        // Non-finite scale on an empty expansion must be rejected.
        let mut acc = Vec::new();
        assert!(add_scaled_into(&mut acc, &[], f64::NAN).is_none());
        assert!(add_scaled_into(&mut acc, &[], f64::INFINITY).is_none());
        assert!(scale(&[], f64::NAN).is_none());
        assert!(det3(Vec3::X, Vec3::Y, Vec3::new(f64::NAN, 0.0, 1.0)).is_none());
        assert!(det3(Vec3::X, Vec3::Y, Vec3::new(f64::INFINITY, 0.0, 1.0)).is_none());

        // Overflow during accumulation must return None.
        let mut big = vec![f64::MAX];
        assert!(add_scaled_into(&mut big, &[f64::MAX], 1.0).is_none());
        assert!(dot(Vec3::new(f64::MAX, f64::MAX, 0.0), Vec3::new(1.0, 1.0, 0.0)).is_none());

        // Exact cancellation of MAX + (-MAX) stays finite and yields 0.
        let mut cancel = vec![f64::MAX];
        add_scaled_into(&mut cancel, &[f64::MAX], -1.0).unwrap();
        assert!(is_zero(&cancel));
    }
}
