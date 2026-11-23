//! Certified comparisons: a computed quantity with a bound on the rounding of computing
//! it, and the three-way answer of comparing it with a tolerance. A query that decides
//! with a tolerance answers [`Verdict::Yes`] or [`Verdict::No`] only when the bound
//! proves it, and [`Verdict::Undecided`] when the tolerance lies within the bound: it
//! decides nothing at a tolerance it can't prove.
//!
//! The bounds use Higham's `gamma_k = k u / (1 - k u)` (`u` the unit roundoff, half
//! `f64::EPSILON`): `k` floating-point operations, each correctly rounded, move a
//! result by at most `gamma_k` relative to the size of its terms. `cos`, `sin` and
//! `atan2` are taken as within one ulp of the true value (the platform's libm; IEEE
//! does not round them correctly), counted as two roundings each.
//!
//! Not upstream. Deviations: none (no upstream counterpart). No recursion.

use crate::geom::vector3d::{Point3, Vec3};

/// `gamma_k = k u / (1 - k u)`: the kernel's one copy, [`crate::nurbs::math::gamma`].
pub use crate::nurbs::math::gamma;

/// `a > b`, false for a NaN: a refusal written `!above(..)` refuses NaN too.
#[must_use]
pub fn above(a: f64, b: f64) -> bool {
    matches!(a.partial_cmp(&b), Some(core::cmp::Ordering::Greater))
}

/// The next double above `x` (`f64::next_up` needs Rust 1.86; the MSRV is 1.85). The
/// sign branches prevent integer wrap: positive finite bits are below +infinity's, and
/// negative nonzero bits are above the sign bit. NaN and +infinity are returned as is.
#[must_use]
pub(crate) fn next_up(x: f64) -> f64 {
    if x.is_nan() || x == f64::INFINITY {
        x
    } else if x == 0.0 {
        f64::from_bits(1)
    } else if x > 0.0 {
        f64::from_bits(x.to_bits() + 1)
    } else {
        f64::from_bits(x.to_bits() - 1)
    }
}

/// The next double below `x`, using the same MSRV-compatible primitive as
/// [`next_up`]. Both signed zeros step to the negative minimum subnormal;
/// negative infinity and NaN (including its sign and payload) are unchanged.
#[must_use]
pub(crate) fn next_down(x: f64) -> f64 {
    if x.is_nan() {
        x
    } else {
        -next_up(-x)
    }
}

/// The answer of comparing a certified quantity with a tolerance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Proved: true for every value within the bound.
    Yes,
    /// Proved: false for every value within the bound.
    No,
    /// The tolerance lies within the bound: not decided.
    Undecided,
}

impl Verdict {
    /// Both: `No` when either is, `Yes` when both are, `Undecided` otherwise.
    #[must_use]
    pub const fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::No, _) | (_, Self::No) => Self::No,
            (Self::Yes, Self::Yes) => Self::Yes,
            _ => Self::Undecided,
        }
    }

    /// Whether it is proved true.
    #[must_use]
    pub fn is_yes(self) -> bool {
        self == Self::Yes
    }
}

/// A computed value and a bound on its distance from the exact value of the same
/// expression on the same inputs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounded {
    /// The computed value.
    pub value: f64,
    /// `|value - exact| <= bound`.
    pub bound: f64,
}

impl Bounded {
    /// Whether the exact value is at most `tol`. `Yes` needs the rounded `value + bound`
    /// strictly below `tol`: rounding is monotone and `tol` a double, so the exact sum is then
    /// below it too (a sum that rounds down onto `tol` is `Undecided`); a bound of 0 is an
    /// exact value, compared as it is. `No` needs `value - bound` above it, by the same
    /// argument. A bound that is negative or not a number bounds nothing: `Undecided`.
    #[must_use]
    pub fn at_most(self, tol: f64) -> Verdict {
        if !(self.value.is_finite() && self.bound.is_finite() && self.bound >= 0.0) {
            Verdict::Undecided
        } else if self.value + self.bound < tol || (self.bound == 0.0 && self.value <= tol) {
            Verdict::Yes
        } else if self.value - self.bound > tol {
            Verdict::No
        } else {
            Verdict::Undecided
        }
    }

    /// Whether the exact value is above `tol`.
    #[must_use]
    pub fn above(self, tol: f64) -> Verdict {
        match self.at_most(tol) {
            Verdict::Yes => Verdict::No,
            Verdict::No => Verdict::Yes,
            Verdict::Undecided => Verdict::Undecided,
        }
    }
}

/// The absolute part of [`distance`]'s bound: squares below the smallest normal double
/// round by up to half a subnormal unit each, absolutely, so the sum of three is off by at
/// most `3 * 2^-1075` and its root by the root of that, about `2.8e-162`.
pub const UNDERFLOW_ROOT: f64 = 1e-161;

/// The distance between two given points, `|a - b|` as [`Vec3::length`] computes it:
/// each difference rounds once, the sum of squares three times, the root once, so
/// the computed value is within `gamma_5` of exact, relatively, while the squares are
/// normal doubles, and within [`UNDERFLOW_ROOT`] more, absolutely, when they underflow
/// (points closer than about `1e-154`: two points `1e-170` apart have a computed
/// distance of 0).
#[must_use]
pub fn distance(a: Point3, b: Point3) -> Bounded {
    let value = (a - b).length();
    Bounded {
        value,
        bound: gamma(5) * value + UNDERFLOW_ROOT,
    }
}

/// `|a - b|` for two given numbers: one rounding.
#[must_use]
pub fn difference(a: f64, b: f64) -> Bounded {
    let value = (a - b).abs();
    Bounded {
        value,
        bound: gamma(1) * value,
    }
}

/// `v` made unit by [`Vec3::unitize`], or `None` for a vector not finite or one it refuses
/// (shorter than `1e-12`): each component within [`unit_error`] of `v / |v|`.
#[must_use]
pub fn unit(v: Vec3) -> Option<Vec3> {
    if !(v.x.is_finite() && v.y.is_finite() && v.z.is_finite()) {
        return None;
    }
    v.unitize().ok()
}

/// The error of each component of a unit vector made by [`unit`] from a given `v`:
/// `v / |v|`, the squared length within `gamma_3`, then the root and the quotient,
/// within `gamma_4` absolute (a unit vector's components being at most 1); a vector
/// whose squared length rounds to 1 is kept as it is, as near. Counted as `gamma_6`.
#[must_use]
pub fn unit_error() -> f64 {
    gamma(6)
}

/// The error of each component of a unit vector made by [`unit`] from a `c` whose
/// components are each within `ec` of exact: `|delta c| <= 2 ec` (`sqrt 3 < 2`) moves
/// the direction by at most `2 |delta c| / |c|`, then [`unit_error`]'s rounding.
#[must_use]
pub fn unit_error_of(ec: f64, c_length: f64) -> f64 {
    4.0 * ec / (c_length * (1.0 - gamma(5))) + gamma(6)
}

/// The bound on the dot of two vectors of length at most about 1 whose components
/// are within `ea` and `eb` of exact: `|delta a| <= 2 ea` (`sqrt 3 < 2`), so the inputs
/// move the dot by at most `2 (ea + eb) + 4 ea eb`, and its rounding by `gamma_3` of
/// the terms' size.
#[must_use]
pub fn dot_error(ea: f64, eb: f64) -> f64 {
    2.0 * (ea + eb) + 4.0 * ea * eb + gamma(3) * (1.0 + 2.0 * ea) * (1.0 + 2.0 * eb)
}

/// `a . b`, certified, for vectors as [`dot_error`] takes them.
#[must_use]
pub fn dot(a: Vec3, b: Vec3, ea: f64, eb: f64) -> Bounded {
    Bounded {
        value: a.dot(b),
        bound: dot_error(ea, eb),
    }
}

/// `1 - a . b`, certified, for vectors as [`dot_error`] takes them: the subtraction
/// rounds once more.
#[must_use]
pub fn one_minus_dot(a: Vec3, b: Vec3, ea: f64, eb: f64) -> Bounded {
    let value = 1.0 - a.dot(b);
    Bounded {
        value,
        bound: dot_error(ea, eb) * (1.0 + gamma(1)) + gamma(1) * value.abs(),
    }
}

/// `|a x b|`, certified, for vectors as [`dot_error`] takes them: the inputs move the
/// cross by at most `2 (ea + eb) + 4 ea eb`, each component's two products and
/// difference by `gamma_2` of the terms (at most `4 gamma_2` over the vector), and the
/// length rounds as [`distance`]'s.
#[must_use]
pub fn cross_length(a: Vec3, b: Vec3, ea: f64, eb: f64) -> Bounded {
    let value = a.cross(b).length();
    let input = 2.0 * (ea + eb) + 4.0 * ea * eb;
    let rounding = 4.0 * gamma(2) * (1.0 + 2.0 * ea) * (1.0 + 2.0 * eb);
    Bounded {
        value,
        bound: (input + rounding) * (1.0 + gamma(4)) + gamma(4) * value,
    }
}

/// `x * s` for a bounded `x` and a given non-negative `s`: the bound scaled, plus
/// the product's rounding (relative, and a subnormal unit for a product that underflows).
#[must_use]
pub fn scaled(x: Bounded, s: f64) -> Bounded {
    let value = x.value * s;
    Bounded {
        value,
        bound: x.bound * s * (1.0 + gamma(1)) + gamma(1) * value.abs() + f64::from_bits(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_up_keeps_signed_zero_subnormals_and_extreme_values() {
        let tiny = f64::from_bits(1);
        for (value, expected) in [
            (0.0, tiny),
            (-0.0, tiny),
            (tiny, 2.0 * tiny),
            (-tiny, -0.0),
            (1.0, 1.0 + f64::EPSILON),
            (-1.0, -1.0 + f64::EPSILON / 2.0),
            (f64::MAX, f64::INFINITY),
            (f64::NEG_INFINITY, -f64::MAX),
            (f64::INFINITY, f64::INFINITY),
        ] {
            assert_eq!(next_up(value).to_bits(), expected.to_bits());
        }
        assert!(next_up(f64::NAN).is_nan());
    }

    #[test]
    fn next_down_keeps_signed_zero_subnormals_and_extreme_values() {
        let tiny = f64::from_bits(1);
        for (value, expected) in [
            (0.0, -tiny),
            (-0.0, -tiny),
            (tiny, 0.0),
            (-tiny, -2.0 * tiny),
            (1.0, 1.0 - f64::EPSILON / 2.0),
            (-1.0, -1.0 - f64::EPSILON),
            (
                f64::MIN_POSITIVE,
                f64::from_bits(f64::MIN_POSITIVE.to_bits() - 1),
            ),
            (-f64::MAX, f64::NEG_INFINITY),
            (f64::INFINITY, f64::MAX),
            (f64::NEG_INFINITY, f64::NEG_INFINITY),
        ] {
            assert_eq!(next_down(value).to_bits(), expected.to_bits());
        }
        // Explicit payloads prove that neither sign nor payload is canonicalized.
        for bits in [0x7ff8_0000_0000_0042, 0xfff8_0000_0000_0042] {
            assert_eq!(next_down(f64::from_bits(bits)).to_bits(), bits);
        }
    }

    /// A bound below half an ulp of the value: the rounded sum is the value, but the exact one
    /// may be above it, so it is not proved at most the value (it was `Yes`).
    #[test]
    fn a_sum_that_rounds_down_onto_the_tolerance_is_undecided() {
        let b = Bounded {
            value: 1.0,
            bound: 1e-17,
        };
        assert_eq!(b.at_most(1.0), Verdict::Undecided);
        assert_eq!(b.at_most(1.0 + f64::EPSILON), Verdict::Yes);
        assert_eq!(b.above(1.0 - f64::EPSILON), Verdict::Yes);
    }

    #[test]
    fn a_bound_decides_only_what_it_proves() {
        let x = Bounded {
            value: 1.0,
            bound: 0.1,
        };
        assert_eq!(x.at_most(1.2), Verdict::Yes);
        assert_eq!(x.at_most(0.8), Verdict::No);
        assert_eq!(x.at_most(1.05), Verdict::Undecided);
        assert_eq!(x.above(0.8), Verdict::Yes);
        assert_eq!(x.above(1.05), Verdict::Undecided);
        let nan = Bounded {
            value: f64::NAN,
            bound: 0.0,
        };
        assert_eq!(nan.at_most(1.0), Verdict::Undecided);
    }

    /// A negative or NaN bound bounds nothing: it was a proved `Yes` (value + bound below the
    /// tolerance) for a value far above it.
    #[test]
    fn a_negative_bound_decides_nothing() {
        for bound in [-5.0, f64::NAN] {
            let x = Bounded { value: 1.0, bound };
            assert_eq!(x.at_most(0.5), Verdict::Undecided);
            assert_eq!(x.above(0.5), Verdict::Undecided);
        }
    }

    /// Points 1e-170 apart: the squares underflow and the computed distance is 0, which a
    /// relative bound alone (0 here) took as exact, proving the distance not above 0. Points
    /// 1e-160 apart: a computed distance off by about 1e-4 relatively, within the bound.
    #[test]
    fn an_underflowing_distance_keeps_an_absolute_bound() {
        let d = distance(Point3::new(1e-170, 0.0, 0.0), Point3::ZERO);
        assert_eq!(d.value, 0.0);
        assert_eq!(d.above(0.0), Verdict::Undecided);
        let d = distance(Point3::new(1e-160, 1e-160, 0.0), Point3::ZERO);
        assert!((d.value - 2.0_f64.sqrt() * 1e-160).abs() <= d.bound);
        assert_eq!(d.above(0.0), Verdict::Yes);
        assert_eq!(
            distance(Point3::new(1.0, 0.0, 0.0), Point3::ZERO).above(0.5),
            Verdict::Yes
        );
    }

    #[test]
    fn verdicts_combine() {
        use Verdict::{No, Undecided, Yes};
        assert_eq!(Yes.and(Yes), Yes);
        assert_eq!(Yes.and(Undecided), Undecided);
        assert_eq!(Undecided.and(No), No);
    }

    #[test]
    fn the_distance_of_a_3_4_5_triangle_is_exact() {
        let d = distance(Point3::new(3.0, 4.0, 0.0), Point3::ZERO);
        assert_eq!(d.value, 5.0);
        assert!(d.bound > 0.0 && d.bound < 1e-14);
    }

    #[test]
    fn a_unit_vector_is_the_kernel_s_and_refuses_what_it_cannot_make() {
        assert_eq!(
            unit(Vec3::new(3.0, 4.0, 0.0)),
            Some(Vec3::new(0.6, 0.8, 0.0))
        );
        assert_eq!(
            unit(Vec3::new(3.0, 4.0, 0.0)),
            Vec3::new(3.0, 4.0, 0.0).unitize().ok()
        );
        assert_eq!(unit(Vec3::new(f64::NAN, 0.0, 1.0)), None);
        assert_eq!(unit(Vec3::new(f64::INFINITY, 0.0, 1.0)), None);
        assert_eq!(unit(Vec3::new(1e-13, 0.0, 0.0)), None);
    }

    #[test]
    fn unit_vectors_at_a_known_angle() {
        // 60 degrees: 1 - cos = 1/2, |sin| = sqrt(3)/2.
        let a = Vec3::X;
        let b = Vec3::new(0.5, 0.75_f64.sqrt(), 0.0);
        let d = one_minus_dot(a, b, 0.0, 0.0);
        assert!((d.value - 0.5).abs() <= d.bound);
        let c = cross_length(a, b, 0.0, 0.0);
        assert!((c.value - 0.75_f64.sqrt()).abs() <= c.bound);
    }
}
