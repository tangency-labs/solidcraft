//! Test-only error-free expansions (Shewchuk, 1997, §2.4–2.5).
//!
//! Components are nonoverlapping, in increasing magnitude, with zeros removed. The
//! fixtures keep products and residuals normal (no overflow or underflow); these are
//! deliberately not general-purpose arbitrary-exponent arithmetic. No square root or
//! rounded coordinate difference enters an oracle.

use std::cmp::Ordering;

use crate::math::Point3;

/// An exact sum of binary64 components, smallest first.
#[derive(Clone, Debug)]
pub(crate) struct Exact(Vec<f64>);

/// Error-free sum: the returned pair sums to the real sum of the inputs.
fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    let bv = s - a;
    let av = s - bv;
    (s, (a - av) + (b - bv))
}

/// Error-free product; Rust's fused `mul_add` preserves the product's residual.
fn two_product(a: f64, b: f64) -> (f64, f64) {
    let p = a * b;
    (p, a.mul_add(b, -p))
}

impl Exact {
    /// The exact value of one finite binary64 input.
    pub(crate) fn of(x: f64) -> Self {
        assert!(x.is_finite());
        Self(if x == 0.0 { Vec::new() } else { vec![x] })
    }

    /// Grow-expansion with zero elimination, preserving nonoverlap.
    fn grow(&self, x: f64) -> Self {
        let mut q = x;
        let mut out = Vec::new();
        for &a in &self.0 {
            let (sum, residual) = two_sum(q, a);
            if residual != 0.0 {
                out.push(residual);
            }
            q = sum;
        }
        if q != 0.0 {
            out.push(q);
        }
        Self(out)
    }

    /// Expansion sum, exactly, by repeated grow-expansion.
    pub(crate) fn add(&self, other: &Self) -> Self {
        other.0.iter().fold(self.clone(), |e, &x| e.grow(x))
    }

    /// Expansion difference, exactly (including coordinate subtraction).
    pub(crate) fn sub(&self, other: &Self) -> Self {
        other.0.iter().fold(self.clone(), |e, &x| e.grow(-x))
    }

    /// Exact product by a binary64 scalar using `TwoProduct`.
    pub(crate) fn scale(&self, x: f64) -> Self {
        self.0.iter().fold(Self::of(0.0), |e, &a| {
            let (p, residual) = two_product(a, x);
            e.grow(residual).grow(p)
        })
    }

    /// Exact product of two expansions.
    pub(crate) fn mul(&self, other: &Self) -> Self {
        other
            .0
            .iter()
            .fold(Self::of(0.0), |e, &x| e.add(&self.scale(x)))
    }

    /// Exact square, without a rounded intermediate product.
    pub(crate) fn square(&self) -> Self {
        self.mul(self)
    }

    /// Exact sign, from the largest nonzero component.
    pub(crate) fn sign(&self) -> Ordering {
        self.0
            .last()
            .copied()
            .unwrap_or(0.0)
            .partial_cmp(&0.0)
            .unwrap()
    }
}

/// Exact squared distance; every coordinate difference uses `TwoSum`.
pub(crate) fn distance2(a: Point3, b: Point3) -> Exact {
    [a.x, a.y, a.z]
        .into_iter()
        .zip([b.x, b.y, b.z])
        .fold(Exact::of(0.0), |s, (x, y)| {
            s.add(&Exact::of(x).sub(&Exact::of(y)).square())
        })
}

/// Assert `lhs <= rhs` exactly, with the signed expansion in failure diagnostics.
pub(crate) fn le(lhs: &Exact, rhs: &Exact) {
    let slack = rhs.sub(lhs);
    assert_ne!(
        slack.sign(),
        Ordering::Less,
        "negative exact slack: {slack:?}"
    );
}

/// Fixed `SplitMix64` stream; the seed spells "rounding", to identify this corpus.
pub(crate) struct Samples(u64);

impl Samples {
    /// Start the same corpus independently in each test.
    pub(crate) fn new() -> Self {
        Self(0x726f_756e_6469_6e67)
    }

    /// A full 52-bit fraction in `[1, 2)`; no integer-to-float rounding.
    pub(crate) fn mantissa(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        f64::from_bits(0x3ff0_0000_0000_0000 | (z & 0x000f_ffff_ffff_ffff))
    }

    /// Four exponent/cancellation regimes, each with 1024 full-mantissa probes.
    /// Coordinates stay below 2^30; tiny offsets are still normal binary64 values.
    pub(crate) fn steps() -> impl Iterator<Item = (Point3, Point3)> {
        let mut rng = Self::new();
        (0..4096).map(move |i| {
            let scale = match i % 4 {
                0 => 1.0,
                1 | 2 => 2.0_f64.powi(29),
                _ => 2.0_f64.powi(-20),
            };
            let a = Point3::new(
                rng.mantissa() * scale,
                -rng.mantissa() * scale,
                rng.mantissa() * scale,
            );
            let z = match i % 4 {
                1 => Point3::new(a.x.next_up(), a.y.next_down(), a.z.next_up()),
                2 => a + Point3::new(rng.mantissa(), rng.mantissa(), -rng.mantissa()),
                _ => Point3::new(
                    -rng.mantissa() * scale,
                    rng.mantissa() * scale,
                    rng.mantissa() * scale,
                ),
            };
            (a, z)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `TwoSum` retains the lost unit in `2^53 + 1`, including cancellation to zero.
    #[test]
    fn the_sum_keeps_its_residual() {
        let a = 2.0_f64.powi(53);
        let (s, e) = two_sum(a, 1.0);
        assert_eq!((s, e), (a, 1.0));
        assert_eq!(
            Exact::of(s).add(&Exact::of(e)).sub(&Exact::of(a)).0,
            vec![1.0]
        );
        assert_eq!(Exact::of(a).sub(&Exact::of(a)).sign(), Ordering::Equal);
    }

    /// `(1+2^-27)(1-2^-27)-1 = -2^-54` despite the rounded product being 1.
    #[test]
    fn the_product_keeps_its_cancellation_sign() {
        let d = 2.0_f64.powi(-27);
        let (p, e) = two_product(1.0 + d, 1.0 - d);
        assert_eq!((p, e), (1.0, -2.0_f64.powi(-54)));
        let difference = Exact::of(1.0 + d)
            .mul(&Exact::of(1.0 - d))
            .sub(&Exact::of(1.0));
        assert_eq!(difference.0, vec![-2.0_f64.powi(-54)]);
    }

    /// The binary64 inputs `0.1 + 0.2 - 0.3` sum exactly to `2^-55`, not zero.
    #[test]
    fn decimal_inputs_are_compared_as_exact_binary_values() {
        let e = Exact::of(0.1).add(&Exact::of(0.2)).sub(&Exact::of(0.3));
        assert_eq!(e.0, vec![2.0_f64.powi(-55)]);
        assert_eq!(e.sign(), Ordering::Greater);
    }
}
