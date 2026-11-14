//! Taylor models in one variable for the G^n certificate: a function of `t` on `[-1, 1]`
//! enclosed as a polynomial with interval coefficients plus an interval remainder. A
//! quantity built from several functions of one parameter (the ball's radii from its rails
//! and its centre, all moving along s) keeps their correlation, where intervals over the
//! whole range lose it at every operation.
//!
//! Not upstream (#14405). A [`Tm`] encloses a function `f` on `T = [-1, 1]`: for every t in
//! T there are numbers `c_j` in [`Tm::c`] and `e` in [`Tm::rem`] with
//! `f(t) = sum_j c_j t^j + e`. Every operation's result encloses the operation on any
//! functions its operands enclose, by construction:
//! - **Sum, difference.** Coefficient by coefficient, the remainders added.
//! - **Product.** The coefficients' products to degree [`TM_DEG`]; each product term of a
//!   higher degree n is bounded over T (`t^n` lies in `[-1, 1]`, or in `[0, 1]` for even
//!   n) into the remainder, as are the remainders times the other factor's polynomial
//!   bounded over T, and the remainders' product.
//! - **Reciprocal.** `q`, the Taylor polynomial of `1 / f` at `t = 0` from the
//!   coefficients' middles (any polynomial would do), and the exact identity
//!   `1 / f = q + (1 - f q) / f`: the remainder is `1 - f q`, a Taylor model whose
//!   polynomial part cancels to rounding, bounded over T and divided by f's range there,
//!   which must exclude 0.
//! - **Square root.** `S`, the Taylor polynomial of `sqrt(f)` at `t = 0` from the middles,
//!   and `sqrt(f) = S + (f - S^2) / (sqrt(f) + S)`, both bounded over T; f's range must be
//!   positive (otherwise the result is the square roots of its range's non-negative part,
//!   as [`Ivl::sqrt`], a constant).
//!
//! Every coefficient and remainder is an [`Ivl`], rounded outward, so the arithmetic of
//! the coefficients needs no rounding analysis of its own. A coefficient that is exactly 0
//! is skipped in products and sums (its exact contribution is 0), which keeps a model of a
//! polynomial free of the `1e-300` an interval product of zeros would add.
//!
//! The certificate's series are [`super::tay::Tay`]s over this ring: for s in a piece
//! `[m - delta, m + delta]`, `t = (s - m) / delta`, each Taylor coefficient of a function of
//! s is a [`Tm`] in t.
//!
//! [`TmN`] holds `N` coefficients (degree `N - 1`, at most [`TM_MAX`]): the arc's rows take
//! [`Tm`] (degree [`TM_DEG`]); the feet's enclosure over a piece
//! ([`super::feet_tm`]) takes a lower degree, its series being many more operations.

use core::ops::{Add, Div, Mul, Sub};

use super::interval::Ivl;
use super::series::GnScalar;
use super::tay::{Ring, TayScalar};

/// The degree of a [`Tm`]'s polynomial.
pub const TM_DEG: usize = 10;
/// The most coefficients a [`TmN`] holds.
pub const TM_MAX: usize = 16;
/// The most coefficients of a product before truncation.
const FULL: usize = 2 * TM_MAX - 1;

const ZERO: Ivl = Ivl::new(0.0, 0.0);

/// Whether an interval is exactly 0.
#[inline]
fn is_zero(x: Ivl) -> bool {
    x.lo == 0.0 && x.hi == 0.0
}

/// The range of `t^j` over `[-1, 1]`.
#[inline]
fn power_range(j: usize) -> Ivl {
    if j == 0 {
        Ivl::from(1.0)
    } else if j % 2 == 1 {
        Ivl::new(-1.0, 1.0)
    } else {
        Ivl::new(0.0, 1.0)
    }
}

/// `a + b`, exact where either is exactly 0.
#[inline]
fn add0(a: Ivl, b: Ivl) -> Ivl {
    if is_zero(a) {
        b
    } else if is_zero(b) {
        a
    } else {
        a + b
    }
}

/// `-b`, exact.
#[inline]
fn neg(b: Ivl) -> Ivl {
    Ivl::new(-b.hi, -b.lo)
}

/// `a - b`, exact where either is exactly 0.
#[inline]
fn sub0(a: Ivl, b: Ivl) -> Ivl {
    if is_zero(b) {
        a
    } else if is_zero(a) {
        neg(b)
    } else {
        a - b
    }
}

/// `a * b`, exactly 0 where either is exactly 0 (any real number times 0, also one of an
/// unbounded interval).
#[inline]
fn mul0(a: Ivl, b: Ivl) -> Ivl {
    if is_zero(a) || is_zero(b) {
        ZERO
    } else {
        a * b
    }
}

/// A Taylor model in t on `[-1, 1]` with `N` coefficients, `N <= TM_MAX` (see the
/// [module](self) for what it encloses). Past [`TM_MAX`] a product is the whole line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TmN<const N: usize> {
    /// The coefficients of `t^0 ..= t^(N - 1)`.
    pub c: [Ivl; N],
    /// The remainder.
    pub rem: Ivl,
}

/// The arc's rows' Taylor model, of degree [`TM_DEG`].
pub type Tm = TmN<{ TM_DEG + 1 }>;

impl<const N: usize> TmN<N> {
    /// The constant `x`.
    #[must_use]
    pub fn constant(x: Ivl) -> Self {
        let mut c = [ZERO; N];
        if let Some(c0) = c.first_mut() {
            *c0 = x;
        }
        Self { c, rem: ZERO }
    }

    /// Any function at all (the remainder is the whole line).
    #[must_use]
    pub fn whole() -> Self {
        Self {
            c: [ZERO; N],
            rem: Ivl::whole(),
        }
    }

    /// The polynomial `sum_j coeffs[j] t^j` plus `rem`: the terms past [`TM_DEG`] bounded
    /// over `[-1, 1]` into the remainder.
    #[must_use]
    pub fn from_poly(coeffs: &[Ivl], rem: Ivl) -> Self {
        let mut c = [ZERO; N];
        let mut rem = rem;
        for (j, &x) in coeffs.iter().enumerate() {
            match c.get_mut(j) {
                Some(cj) => *cj = x,
                None => rem = add0(rem, mul0(x, power_range(j))),
            }
        }
        Self { c, rem }
    }

    /// The polynomial whose coefficients are the numbers `p` (exact points).
    fn points(p: &[f64; N]) -> Self {
        Self {
            c: p.map(Ivl::from),
            rem: ZERO,
        }
    }

    /// The polynomial whose coefficients are the numbers `p` (exact points; those past
    /// `N` bounded over `[-1, 1]` into the remainder).
    #[must_use]
    pub fn from_points(p: &[f64]) -> Self {
        let mut c = [ZERO; N];
        let mut rem = ZERO;
        for (j, &x) in p.iter().enumerate() {
            match c.get_mut(j) {
                Some(cj) => *cj = Ivl::from(x),
                None => rem = add0(rem, mul0(Ivl::from(x), power_range(j))),
            }
        }
        Self { c, rem }
    }

    /// The polynomial of the coefficients' middles (exact points), no remainder: one
    /// function close to every one this encloses.
    #[must_use]
    pub fn mid_poly(&self) -> Self {
        Self {
            c: self
                .c
                .map(|x| if is_zero(x) { ZERO } else { Ivl::from(x.mid()) }),
            rem: ZERO,
        }
    }

    /// This with `r` added to its remainder.
    #[must_use]
    pub fn with_rem(self, r: Ivl) -> Self {
        Self {
            c: self.c,
            rem: add0(self.rem, r),
        }
    }

    /// Each coefficient and the remainder times the number range `x`.
    #[must_use]
    pub fn scale(self, x: Ivl) -> Self {
        Self {
            c: self.c.map(|ci| mul0(ci, x)),
            rem: mul0(self.rem, x),
        }
    }

    /// This plus the number range `x` (on the constant coefficient).
    #[must_use]
    pub fn add_ivl(self, x: Ivl) -> Self {
        let mut c = self.c;
        if let Some(c0) = c.first_mut() {
            *c0 = add0(*c0, x);
        }
        Self { c, rem: self.rem }
    }

    /// The polynomial's range over `[-1, 1]`, bounded term by term.
    #[must_use]
    pub fn poly_range(&self) -> Ivl {
        let mut r = ZERO;
        for (j, &cj) in self.c.iter().enumerate() {
            r = add0(r, mul0(cj, power_range(j)));
        }
        r
    }

    /// The range over `[-1, 1]`: the polynomial's, bounded term by term, plus the
    /// remainder.
    #[must_use]
    pub fn range(&self) -> Ivl {
        add0(self.poly_range(), self.rem)
    }

    /// The enclosure at one `t` in `[-1, 1]`: `sum_j c_j t^j + rem`.
    #[must_use]
    pub fn at(&self, t: f64) -> Ivl {
        let mut r = ZERO;
        let mut pow = Ivl::from(1.0);
        for (j, &cj) in self.c.iter().enumerate() {
            if j > 0 {
                pow = pow * Ivl::from(t);
            }
            r = add0(r, mul0(cj, pow));
        }
        add0(r, self.rem)
    }

    /// The coefficients' middles, with the constant's replaced by `fallback` where it is
    /// not a usable expansion value (`ok` false on it).
    fn middles(&self, fallback: f64, ok: impl Fn(f64) -> bool) -> [f64; N] {
        let mut m = self.c.map(Ivl::mid);
        if let Some(m0) = m.first_mut() {
            if !ok(*m0) {
                *m0 = fallback;
            }
        }
        m
    }

    /// `1 / f`: the whole line where f's range over `[-1, 1]` holds 0.
    #[must_use]
    pub fn recip(self) -> Self {
        let r = self.range();
        let excludes_zero = r.lo > 0.0 || r.hi < 0.0;
        if !excludes_zero {
            return Self::whole();
        }
        // q: the Taylor polynomial of 1 / p at t = 0, p the coefficients' middles
        let m = self.middles(r.mid(), |x| x != 0.0 && x.is_finite());
        let mut q = [0.0_f64; N];
        let m0 = m.first().copied().unwrap_or(f64::NAN);
        for n in 0..N {
            let mut s = if n == 0 { 1.0 } else { 0.0 };
            for j in 1..=n {
                let (Some(&mj), Some(&qk)) = (m.get(j), q.get(n - j)) else {
                    continue;
                };
                s -= mj * qk;
            }
            if let Some(qn) = q.get_mut(n) {
                *qn = s / m0;
            }
        }
        let qt = Self::points(&q);
        // 1 / f - q = (1 - f q) / f for every t
        let e = (Self::constant(Ivl::from(1.0)) - self * qt).range();
        Self {
            c: qt.c,
            rem: e / r,
        }
    }

    /// `sqrt(f)` where f's range over `[-1, 1]` is positive; otherwise the square roots of
    /// that range's non-negative part (as [`Ivl::sqrt`]), a constant.
    #[must_use]
    pub fn sqrt_tm(self) -> Self {
        let r = self.range();
        let plain = Self::constant(r.sqrt());
        if !r.is_positive() {
            return plain;
        }
        // S: the Taylor polynomial of sqrt(p) at t = 0
        let m = self.middles(r.mid(), |x| x > 0.0 && x.is_finite());
        let mut sq = [0.0_f64; N];
        let s0 = crate::geom::vector3d::sqrt(m.first().copied().unwrap_or(f64::NAN));
        for n in 0..N {
            let v = if n == 0 {
                s0
            } else {
                let mut a = m.get(n).copied().unwrap_or(0.0);
                for k in 1..n {
                    let (Some(&sk), Some(&sl)) = (sq.get(k), sq.get(n - k)) else {
                        continue;
                    };
                    a -= sk * sl;
                }
                a / (2.0 * s0)
            };
            if let Some(x) = sq.get_mut(n) {
                *x = v;
            }
        }
        let st = Self::points(&sq);
        // sqrt(f) - S = (f - S^2) / (sqrt(f) + S) for every t
        let num = (self - st * st).range();
        let den = r.sqrt() + st.poly_range();
        if !den.is_positive() {
            return plain;
        }
        Self {
            c: st.c,
            rem: num / den,
        }
    }
}

impl<const N: usize> Add for TmN<N> {
    type Output = Self;
    fn add(self, b: Self) -> Self {
        let mut c = [ZERO; N];
        for (ci, (&ai, &bi)) in c.iter_mut().zip(self.c.iter().zip(&b.c)) {
            *ci = add0(ai, bi);
        }
        Self {
            c,
            rem: add0(self.rem, b.rem),
        }
    }
}

impl<const N: usize> Sub for TmN<N> {
    type Output = Self;
    fn sub(self, b: Self) -> Self {
        let mut c = [ZERO; N];
        for (ci, (&ai, &bi)) in c.iter_mut().zip(self.c.iter().zip(&b.c)) {
            *ci = sub0(ai, bi);
        }
        Self {
            c,
            rem: sub0(self.rem, b.rem),
        }
    }
}

impl<const N: usize> Mul for TmN<N> {
    type Output = Self;
    fn mul(self, b: Self) -> Self {
        if N > TM_MAX {
            // the truncation's scratch holds TM_MAX coefficients' products
            return Self::whole();
        }
        let mut full = [ZERO; FULL];
        for (i, &ai) in self.c.iter().enumerate() {
            if is_zero(ai) {
                continue;
            }
            for (j, &bj) in b.c.iter().enumerate() {
                if let Some(f) = full.get_mut(i + j) {
                    *f = add0(*f, mul0(ai, bj));
                }
            }
        }
        let mut c = [ZERO; N];
        let mut rem = ZERO;
        for (n, &fnn) in full.iter().enumerate().take((2 * N).saturating_sub(1)) {
            match c.get_mut(n) {
                Some(cn) => *cn = fnn,
                None => rem = add0(rem, mul0(fnn, power_range(n))),
            }
        }
        if !is_zero(self.rem) {
            rem = add0(rem, mul0(self.rem, b.poly_range()));
        }
        if !is_zero(b.rem) {
            rem = add0(rem, mul0(b.rem, self.poly_range()));
        }
        rem = add0(rem, mul0(self.rem, b.rem));
        Self { c, rem }
    }
}

impl<const N: usize> Div for TmN<N> {
    type Output = Self;
    fn div(self, b: Self) -> Self {
        // a / b = a * (1 / b)
        Mul::mul(self, b.recip())
    }
}

impl<const N: usize> Ring for TmN<N> {
    fn lift(d: f64) -> Self {
        Self::constant(Ivl::from(d))
    }
}

impl<const N: usize> TayScalar for TmN<N> {
    fn sqrt_s(self) -> Self {
        self.sqrt_tm()
    }
    /// The range's arc cosine, a constant (the certificate's rows take none).
    fn acos_s(self) -> Self {
        Self::constant(self.range().acos_s())
    }
    /// The range's sine and cosine, constants (the certificate's rows take none).
    fn sin_cos_s(self) -> (Self, Self) {
        let (s, c) = self.range().sin_cos_s();
        (Self::constant(s), Self::constant(c))
    }
}

impl<const N: usize> GnScalar for TmN<N> {
    type Storage = Box<[[Self; super::series::STORE_E + 1]]>;
    fn of(d: f64) -> Self {
        Self::lift(d)
    }
    /// Its range over `[-1, 1]` is positive.
    fn is_positive(self) -> bool {
        self.range().is_positive()
    }
    fn sqrt(self) -> Self {
        self.sqrt_tm()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed-seed generator in `[0, 1)`.
    fn rng(seed: u64) -> impl FnMut() -> f64 {
        let mut s = seed;
        move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s >> 11) as f64 / (1_u64 << 53) as f64
        }
    }

    fn contains(i: Ivl, x: f64) -> bool {
        i.lo <= x && x <= i.hi
    }

    /// A model of the polynomial `p` (point coefficients) and its value at t.
    fn poly(p: &[f64]) -> (Tm, impl Fn(f64) -> f64 + '_) {
        let tm = Tm::from_poly(&p.iter().map(|&x| Ivl::from(x)).collect::<Vec<_>>(), ZERO);
        let f = move |t: f64| p.iter().rev().fold(0.0, |acc, &c| acc * t + c);
        (tm, f)
    }

    /// Products, quotients and square roots of random polynomials of degree up to 9 (past
    /// the model's degree) hold their values at 401 points of `[-1, 1]`, and so does a
    /// composition of them (`sqrt(a b) / (1 + c^2)`), over 200 fixed-seed draws. The
    /// polynomials under a square root or a quotient stay at least 1 on `[-1, 1]` (their
    /// constant term is at least the sum of the others' bounds plus 1), where the functions
    /// are defined.
    #[test]
    fn operations_hold_their_sampled_values() {
        let mut next = rng(0x2545_f491_4f6c_dd1d);
        let mut judged = 0;
        for _ in 0..200 {
            let mut draw = |deg: usize, base: f64| -> Vec<f64> {
                let mut p: Vec<f64> = (0..=deg).map(|_| 2.0 * next() - 1.0).collect();
                if let Some(p0) = p.first_mut() {
                    *p0 += base;
                }
                p
            };
            let (pa, pb, pc) = (draw(9, 11.0), draw(4, 6.0), draw(3, 0.0));
            let (a, fa) = poly(&pa);
            let (b, fb) = poly(&pb);
            let (c, fc) = poly(&pc);
            let prod = a * b;
            let quot = a / b;
            let root = a.sqrt_tm();
            let comp = (a * b).sqrt_tm() / (Tm::lift(1.0) + c * c);
            for i in 0..=400 {
                let t = -1.0 + 2.0 * f64::from(i) / 400.0;
                let (x, y, z) = (fa(t), fb(t), fc(t));
                // the point evaluation's own rounding: a few ulps of the terms' size
                let slack = |v: f64| 1e-13 * (1.0 + v.abs());
                for (m, v) in [
                    (prod, x * y),
                    (quot, x / y),
                    (root, x.sqrt()),
                    (comp, (x * y).sqrt() / (1.0 + z * z)),
                ] {
                    let e = m.at(t);
                    assert!(
                        e.lo - slack(v) <= v && v <= e.hi + slack(v),
                        "t {t}: {v:e} outside [{:e}, {:e}]",
                        e.lo,
                        e.hi
                    );
                    assert!(contains(m.range(), v) || (m.range().lo - slack(v) <= v));
                    judged += 1;
                }
            }
        }
        assert_eq!(judged, 200 * 401 * 4);
    }

    /// A model keeps a cancellation intervals lose: `(1 + t)^2 - (1 + 2t + t^2)` is
    /// within rounding of 0, where its terms' ranges are `[0, 4]` each.
    #[test]
    fn a_difference_of_equal_functions_cancels() {
        let (a, _) = poly(&[1.0, 1.0]);
        let (b, _) = poly(&[1.0, 2.0, 1.0]);
        let d = (a * a - b).range();
        assert!(d.mag() < 1e-14, "{d:?}");
        let r = a.range();
        assert!(r.lo <= 0.0 && r.hi >= 2.0);
    }

    /// A reciprocal or square root of a range holding 0 is unbounded or the plain interval
    /// one, never a model that claims more.
    #[test]
    fn a_range_holding_zero_proves_nothing() {
        let (a, _) = poly(&[0.5, 1.0]);
        assert_eq!(a.recip().rem, Ivl::whole());
        let s = a.sqrt_tm();
        assert_eq!(s.c.iter().skip(1).filter(|c| !is_zero(**c)).count(), 0);
        assert!(contains(s.range(), 0.0) && contains(s.range(), 1.5_f64.sqrt()));
    }
}
