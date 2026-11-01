//! Truncated power series in two variables, sigma (along the leader's parameter s) and eps
//! (`tau - 1`), over `f64` (the jets) or [`Ivl`] (their enclosures): `c[i][k]` the
//! coefficient of `sigma^i eps^k`, kept up to the orders of a [`Trunc`].
//!
//! Fork: `sm_Ser`, `sm_VSer` and their operations in `source/SMLib/src/SmFilletSection.cpp`
//! (tangency-labs/smlib@11ea2d9); see [`super`] for the table.

use core::ops::{Add, Div, Mul, Sub};

use super::interval::Ivl;
use crate::error::{Result, SolidError};
use crate::math::Vec3;

/// `SM_GN_MAXS`: the highest power of sigma a series holds.
pub const MAX_S: usize = 7;
/// `SM_GN_MAXE`: the highest power of eps a series holds.
pub const MAX_E: usize = 4;
/// `SM_GN_TAYLOR`: the Taylor form's order of the certificate's fourth s-derivatives (their
/// Taylor polynomial of degree `TAYLOR - 1` at the span's middle, and the remainder bound
/// over the span).
pub const TAYLOR: usize = 2;
/// `SM_GN_MAXK`: the highest order of a face's partial derivatives the series need.
pub const MAX_K: usize = MAX_S + MAX_E + 2;
/// The highest power of eps a series stores: one past [`MAX_E`], for the one truncation
/// that keeps it, `(0, MAX_E + 1)` (`ContactOrderAt` at order 4). The fork's
/// `c[SM_GN_MAXS + 1][SM_GN_MAXE + 1]` has no room for `c[0][5]` and writes it over
/// `c[1][0]`, which a series truncated at sigma^0 never reads; this column holds it instead.
pub const STORE_E: usize = MAX_E + 1;

/// The number type of the fork's templates (`double` or `sm_Ivl`).
pub trait GnScalar:
    Copy + Add<Output = Self> + Sub<Output = Self> + Mul<Output = Self> + Div<Output = Self>
{
    /// `T(double)`.
    fn of(d: f64) -> Self;
    /// `sm_Positive`: (every number in) it is positive.
    fn is_positive(self) -> bool;
    /// `sm_Sqrt`: `smos_Sqrt` for `double`, the enclosure for `sm_Ivl`.
    #[must_use]
    fn sqrt(self) -> Self;
}

impl GnScalar for f64 {
    fn of(d: f64) -> Self {
        d
    }
    fn is_positive(self) -> bool {
        self > 0.0
    }
    fn sqrt(self) -> Self {
        crate::geom::vector3d::sqrt(self)
    }
}

impl GnScalar for Ivl {
    fn of(d: f64) -> Self {
        Ivl::from(d)
    }
    fn is_positive(self) -> bool {
        Ivl::is_positive(self)
    }
    fn sqrt(self) -> Self {
        Ivl::sqrt(self)
    }
}

/// `sm_Trunc`: the highest powers of sigma (`s`) and eps (`e`) kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trunc {
    s: usize,
    e: usize,
}

impl Trunc {
    /// A truncation keeping `sigma^s` and `eps^e`; errors past [`MAX_S`] or [`MAX_E`]
    /// (past [`STORE_E`] for `s = 0`).
    pub fn new(s: usize, e: usize) -> Result<Self> {
        let e_max = if s == 0 { STORE_E } else { MAX_E };
        if s > MAX_S || e > e_max {
            return Err(SolidError::InvalidInput {
                operation: "blend::fillet_section::Trunc::new",
                reason: "a series' orders exceed SM_GN_MAXS or SM_GN_MAXE",
            });
        }
        Ok(Self { s, e })
    }

    /// The highest power of sigma kept.
    #[must_use]
    pub const fn s(self) -> usize {
        self.s
    }

    /// The highest power of eps kept.
    #[must_use]
    pub const fn e(self) -> usize {
        self.e
    }
}

/// `sm_Ser`: the coefficients `c[i][k]` of `sigma^i eps^k`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ser<T> {
    /// `c[i][k]`, `i <= MAX_S`, `k <= STORE_E`.
    pub c: [[T; STORE_E + 1]; MAX_S + 1],
}

impl<T: GnScalar> Ser<T> {
    /// `sm_SerConst`: the constant series `v`.
    #[must_use]
    pub fn constant(v: T) -> Self {
        let mut c = [[T::of(0.0); STORE_E + 1]; MAX_S + 1];
        if let Some(c00) = c.first_mut().and_then(|row| row.first_mut()) {
            *c00 = v;
        }
        Self { c }
    }

    /// The coefficient of `sigma^i eps^k` (0 past the series' size).
    #[must_use]
    pub fn at(&self, i: usize, k: usize) -> T {
        self.c
            .get(i)
            .and_then(|row| row.get(k))
            .copied()
            .unwrap_or_else(|| T::of(0.0))
    }

    /// Sets the coefficient of `sigma^i eps^k` (ignored past the series' size).
    pub fn set(&mut self, i: usize, k: usize, v: T) {
        if let Some(x) = self.c.get_mut(i).and_then(|row| row.get_mut(k)) {
            *x = v;
        }
    }

    /// The coefficients kept by `tr` set from `f(i, k)`, the rest 0 (the fork's
    /// `sm_SerConst<T>(T(0.0))` then its loops over `i <= tr.s`, `k <= tr.e`).
    fn from_fn(tr: Trunc, mut f: impl FnMut(usize, usize) -> T) -> Self {
        let mut r = Self::constant(T::of(0.0));
        for (i, row) in r.c.iter_mut().enumerate().take(tr.s + 1) {
            for (k, x) in row.iter_mut().enumerate().take(tr.e + 1) {
                *x = f(i, k);
            }
        }
        r
    }

    /// `sm_SerAdd`.
    #[must_use]
    pub fn add(&self, b: &Self, tr: Trunc) -> Self {
        Self::from_fn(tr, |i, k| self.at(i, k) + b.at(i, k))
    }

    /// `sm_SerSub`.
    #[must_use]
    pub fn sub(&self, b: &Self, tr: Trunc) -> Self {
        Self::from_fn(tr, |i, k| self.at(i, k) - b.at(i, k))
    }

    /// `sm_SerScale`: each coefficient times `f`.
    #[must_use]
    pub fn scale(&self, f: T, tr: Trunc) -> Self {
        Self::from_fn(tr, |i, k| self.at(i, k) * f)
    }

    /// `sm_SerMul`: the truncated product (the sum over `i1 <= i`, then `k1 <= k`).
    #[must_use]
    pub fn mul(&self, b: &Self, tr: Trunc) -> Self {
        Self::from_fn(tr, |i, k| {
            let mut sum = T::of(0.0);
            for i1 in 0..=i {
                for k1 in 0..=k {
                    sum = sum + self.at(i1, k1) * b.at(i - i1, k - k1);
                }
            }
            sum
        })
    }

    /// `sm_SerInvSqrt`: `x^(-1/2)` for a series whose constant term is positive,
    /// `x0^(-1/2) (1 + z)^(-1/2)`, `z = x / x0 - 1`, summed as the binomial series in z
    /// (which has no constant term) to `tr.s + tr.e` terms.
    pub fn inv_sqrt(&self, tr: Trunc) -> Result<Self> {
        let x0 = self.at(0, 0);
        if !x0.is_positive() {
            // SER_MSG(SM_ERR, "SmGnSectionFSG: a face's normal vanishes at a foot")
            return Err(SolidError::sm_err("blend::fillet_section::Ser::inv_sqrt"));
        }
        let mut z = self.scale(T::of(1.0) / x0, tr);
        z.set(0, 0, T::of(0.0));
        let mut pow = Self::constant(T::of(1.0));
        let mut sum = Self::constant(T::of(1.0));
        let mut binom = 1.0_f64;
        for m in 1..=(tr.s + tr.e) {
            // dBinom *= (-0.5 - (m - 1)) / m, m an int
            binom *= (-0.5 - (m - 1) as f64) / m as f64;
            pow = pow.mul(&z, tr);
            sum = sum.add(&pow.scale(T::of(binom), tr), tr);
        }
        Ok(sum.scale(T::of(1.0) / x0.sqrt(), tr))
    }
}

/// `sm_V3`: a vector of `T`s.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct V3<T> {
    /// x.
    pub x: T,
    /// y.
    pub y: T,
    /// z.
    pub z: T,
}

impl<T: GnScalar> V3<T> {
    /// `sm_V3From`: `T(v.x), T(v.y), T(v.z)`.
    #[must_use]
    pub fn from_vec3(v: Vec3) -> Self {
        Self {
            x: T::of(v.x),
            y: T::of(v.y),
            z: T::of(v.z),
        }
    }
}

/// `sm_VSer`: a vector of series.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VSer<T> {
    /// The x, y and z series.
    pub a: [Ser<T>; 3],
}

impl<T: GnScalar> VSer<T> {
    /// `sm_VSerConst`.
    #[must_use]
    pub fn constant(v: V3<T>) -> Self {
        Self {
            a: [Ser::constant(v.x), Ser::constant(v.y), Ser::constant(v.z)],
        }
    }

    /// `sm_VSerAdd`.
    #[must_use]
    pub fn add(&self, q: &Self, tr: Trunc) -> Self {
        let [p0, p1, p2] = &self.a;
        let [q0, q1, q2] = &q.a;
        Self {
            a: [p0.add(q0, tr), p1.add(q1, tr), p2.add(q2, tr)],
        }
    }

    /// `sm_VSerSub`.
    #[must_use]
    pub fn sub(&self, q: &Self, tr: Trunc) -> Self {
        let [p0, p1, p2] = &self.a;
        let [q0, q1, q2] = &q.a;
        Self {
            a: [p0.sub(q0, tr), p1.sub(q1, tr), p2.sub(q2, tr)],
        }
    }

    /// `sm_VSerTimes`: each component times the series `s`.
    #[must_use]
    pub fn times(&self, s: &Ser<T>, tr: Trunc) -> Self {
        let [p0, p1, p2] = &self.a;
        Self {
            a: [p0.mul(s, tr), p1.mul(s, tr), p2.mul(s, tr)],
        }
    }

    /// `sm_VSerDot`: `(p0 q0 + p1 q1) + p2 q2`.
    #[must_use]
    pub fn dot(&self, q: &Self, tr: Trunc) -> Ser<T> {
        let [p0, p1, p2] = &self.a;
        let [q0, q1, q2] = &q.a;
        let r = p0.mul(q0, tr);
        let r = r.add(&p1.mul(q1, tr), tr);
        r.add(&p2.mul(q2, tr), tr)
    }

    /// `sm_VSerCross`.
    #[must_use]
    pub fn cross(&self, q: &Self, tr: Trunc) -> Self {
        let [p0, p1, p2] = &self.a;
        let [q0, q1, q2] = &q.a;
        Self {
            a: [
                p1.mul(q2, tr).sub(&p2.mul(q1, tr), tr),
                p2.mul(q0, tr).sub(&p0.mul(q2, tr), tr),
                p0.mul(q1, tr).sub(&p1.mul(q0, tr), tr),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The series of `1 + 2 sigma + 3 eps` (and 0 elsewhere).
    fn lin() -> Ser<f64> {
        let mut x = Ser::constant(1.0);
        x.set(1, 0, 2.0);
        x.set(0, 1, 3.0);
        x
    }

    #[test]
    fn truncation_is_bounded() {
        assert!(Trunc::new(MAX_S, MAX_E).is_ok());
        assert!(Trunc::new(MAX_S + 1, 0).is_err());
        assert!(Trunc::new(1, MAX_E + 1).is_err());
        assert!(Trunc::new(0, MAX_E + 1).is_ok());
        assert!(Trunc::new(0, MAX_E + 2).is_err());
    }

    #[test]
    fn product_is_the_truncated_polynomial_product() {
        let tr = Trunc::new(3, 2).unwrap();
        // (1 + 2 s + 3 e)^2 = 1 + 4 s + 6 e + 4 s^2 + 12 s e + 9 e^2
        let sq = lin().mul(&lin(), tr);
        assert_eq!(sq.c[0][0], 1.0);
        assert_eq!(sq.c[1][0], 4.0);
        assert_eq!(sq.c[0][1], 6.0);
        assert_eq!(sq.c[2][0], 4.0);
        assert_eq!(sq.c[1][1], 12.0);
        assert_eq!(sq.c[0][2], 9.0);
        // nothing kept past the truncation
        let tr0 = Trunc::new(1, 0).unwrap();
        let sq0 = lin().mul(&lin(), tr0);
        assert_eq!(sq0.c[0][1], 0.0);
        assert_eq!(sq0.c[2][0], 0.0);
    }

    #[test]
    fn inv_sqrt_squares_to_the_reciprocal() {
        let tr = Trunc::new(MAX_S, MAX_E).unwrap();
        let mut x = Ser::constant(4.0);
        x.set(1, 0, 1.0);
        x.set(0, 1, -0.5);
        x.set(1, 1, 0.25);
        let y = x.inv_sqrt(tr).unwrap();
        // y^2 x = 1 to every kept order
        let one = y.mul(&y, tr).mul(&x, tr);
        for i in 0..=MAX_S {
            for k in 0..=MAX_E {
                let want = if i == 0 && k == 0 { 1.0 } else { 0.0 };
                // the binomial sum stops at tr.s + tr.e terms, which covers every kept order
                assert!(
                    (one.c[i][k] - want).abs() < 1e-12,
                    "{i} {k} {}",
                    one.c[i][k]
                );
            }
        }
        assert_eq!(y.c[0][0], 0.5);
        assert!(Ser::constant(0.0_f64).inv_sqrt(tr).is_err());
    }

    #[test]
    fn interval_series_enclose_the_double_ones() {
        let tr = Trunc::new(2, 2).unwrap();
        let xd = lin();
        let mut xi = Ser::constant(Ivl::from(1.0));
        xi.set(1, 0, Ivl::from(2.0));
        xi.set(0, 1, Ivl::from(3.0));
        let yd = xd.inv_sqrt(tr).unwrap();
        let yi = xi.inv_sqrt(tr).unwrap();
        for i in 0..=2 {
            for k in 0..=2 {
                assert!(yi.c[i][k].lo <= yd.c[i][k] && yd.c[i][k] <= yi.c[i][k].hi);
            }
        }
    }

    #[test]
    fn vector_series_dot_and_cross() {
        let tr = Trunc::new(1, 1).unwrap();
        let e1 = VSer::constant(V3::from_vec3(Vec3::new(1.0, 0.0, 0.0)));
        let e2 = VSer::constant(V3::from_vec3(Vec3::new(0.0, 1.0, 0.0)));
        let e3 = e1.cross(&e2, tr);
        assert_eq!(e3.a[2].c[0][0], 1.0);
        assert_eq!(e1.dot(&e2, tr).c[0][0], 0.0);
        let s = lin();
        let p = e1.add(&e2, tr).times(&s, tr).sub(&e2, tr);
        assert_eq!(p.a[0].c[1][0], 2.0);
        assert_eq!(p.a[1].c[0][0], 0.0);
    }
}
