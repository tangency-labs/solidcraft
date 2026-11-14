//! Truncated power series in two variables, sigma (along the leader's parameter s) and eps
//! (`tau - 1`), over `f64` (the jets) or [`Ivl`] (their enclosures): `c[i][k]` the
//! coefficient of `sigma^i eps^k`, kept up to the orders of a [`Trunc`].
//!
//! Fork: `sm_Ser`, `sm_VSer` and their operations in `source/SMLib/src/SmFilletSection.cpp`
//! (tangency-labs/smlib@11ea2d9); see [`super`] for the table.
//!
//! Deviations: interval and Taylor-model series keep their coefficient tables on the
//! heap, so the certificate's nested systems and workspaces fit on a small thread stack.
//! Floating-point series retain their inline tables. Storage never changes arithmetic order.

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
    Copy
    + core::fmt::Debug
    + PartialEq
    + Add<Output = Self>
    + Sub<Output = Self>
    + Mul<Output = Self>
    + Div<Output = Self>
{
    /// Storage for a series' coefficients, inline for floating-point scalars and
    /// heap-owned for enclosures, whose many inline tables exhaust small stacks.
    type Storage: SeriesStorage<Self>;
    /// `T(double)`.
    fn of(d: f64) -> Self;
    /// `sm_Positive`: (every number in) it is positive.
    fn is_positive(self) -> bool;
    /// `sm_Sqrt`: `smos_Sqrt` for `double`, the enclosure for `sm_Ivl`.
    #[must_use]
    fn sqrt(self) -> Self;
}

impl GnScalar for f64 {
    type Storage = [[Self; STORE_E + 1]; MAX_S + 1];
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
    type Storage = Box<[[Self; STORE_E + 1]]>;
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

/// The fixed coefficient table of a series, independent of where it is allocated.
pub trait SeriesStorage<T>: Clone + core::fmt::Debug + PartialEq {
    /// Fill the table with `value`.
    fn filled(value: T) -> Self;
    /// Coefficients in ascending sigma and eps order.
    fn rows(&self) -> &[[T; STORE_E + 1]];
    /// Mutable coefficients in the same order.
    fn rows_mut(&mut self) -> &mut [[T; STORE_E + 1]];
}

impl<T: Copy + core::fmt::Debug + PartialEq> SeriesStorage<T> for [[T; STORE_E + 1]; MAX_S + 1] {
    fn filled(value: T) -> Self {
        [[value; STORE_E + 1]; MAX_S + 1]
    }
    fn rows(&self) -> &[[T; STORE_E + 1]] {
        self
    }
    fn rows_mut(&mut self) -> &mut [[T; STORE_E + 1]] {
        self
    }
}

impl<T: Copy + core::fmt::Debug + PartialEq> SeriesStorage<T> for Box<[[T; STORE_E + 1]]> {
    /// Filled on the heap directly: no table of this size is ever built on the stack.
    fn filled(value: T) -> Self {
        vec![[value; STORE_E + 1]; MAX_S + 1].into_boxed_slice()
    }
    fn rows(&self) -> &[[T; STORE_E + 1]] {
        self
    }
    fn rows_mut(&mut self) -> &mut [[T; STORE_E + 1]] {
        self
    }
}

/// `sm_Trunc`: the highest powers of sigma (`s`) and eps (`e`) kept.
///
/// Not the fork's: a truncation may also leave out every coefficient past a total degree
/// `i + k` and a power of eps ([`Trunc::computing`]): those are left 0, and nothing else
/// changes. A coefficient of a sum or a product depends only on the operands' coefficients
/// at the same or lower powers of both sigma and eps, so the coefficients computed are
/// bit for bit those of the whole truncation; the loops the orders set (the binomial
/// series' terms, a composition's powers) keep the whole truncation's counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trunc {
    s: usize,
    e: usize,
    /// The highest total degree `i + k` computed.
    degree: usize,
    /// The highest power of eps computed.
    eps: usize,
    /// The coefficients left as they are (bit [`Trunc::bit`] of `c[i][k]`), whatever the
    /// rest computes: an incremental evaluation's coefficients already computed
    /// ([`Trunc::skipping`]).
    skip: u64,
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
        Ok(Self {
            s,
            e,
            degree: s + e,
            eps: e,
            skip: 0,
        })
    }

    /// This truncation, computing only the coefficients `c[i][k]` with `i + k <= degree`
    /// and `k <= eps` (the rest left 0; see [`Trunc`]). The orders [`Trunc::s`] and
    /// [`Trunc::e`] are this truncation's.
    #[must_use]
    pub const fn computing(self, degree: usize, eps: usize) -> Self {
        Self {
            s: self.s,
            e: self.e,
            degree,
            eps,
            skip: self.skip,
        }
    }

    /// This truncation, also leaving out the coefficients `c[i][k]` whose bit
    /// ([`Trunc::bit`]) `mask` sets: an operation leaves them as they are (an operation
    /// that returns a new series leaves them 0). Each coefficient computed is bit for bit
    /// the same as without the mask, as it depends only on the operands' coefficients
    /// (see [`Trunc`]); an incremental evaluation (`station::GnIncremental`) so computes
    /// only the coefficients it has not kept from an earlier one.
    #[must_use]
    pub const fn skipping(self, mask: u64) -> Self {
        Self {
            s: self.s,
            e: self.e,
            degree: self.degree,
            eps: self.eps,
            skip: self.skip | mask,
        }
    }

    /// The bit of `c[i][k]` in a [`Trunc::skipping`] mask (`i <= MAX_S`, `k <= STORE_E`).
    #[must_use]
    pub const fn bit(i: usize, k: usize) -> u64 {
        1_u64 << (i * (STORE_E + 1) + k)
    }

    /// Whether `c[i][k]` is computed.
    #[inline]
    #[must_use]
    pub const fn computes(self, i: usize, k: usize) -> bool {
        i <= self.s
            && k <= self.e
            && k <= self.eps
            && i + k <= self.degree
            && self.skip & Self::bit(i, k) == 0
    }

    /// The [`Trunc::bit`]s of every coefficient computed.
    #[must_use]
    pub fn computed_mask(self) -> u64 {
        let mut m = 0_u64;
        for i in 0..=self.s {
            for k in 0..=self.e {
                if self.computes(i, k) {
                    m |= Self::bit(i, k);
                }
            }
        }
        m
    }

    /// The highest total degree `i + k` computed.
    #[must_use]
    pub const fn degree(self) -> usize {
        self.degree
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
#[derive(Clone, Debug, PartialEq)]
pub struct Ser<T: GnScalar> {
    /// `c[i][k]`, `i <= MAX_S`, `k <= STORE_E`.
    pub c: T::Storage,
}

impl<T: GnScalar> Copy for Ser<T> where T::Storage: Copy {}

impl<T: GnScalar> Ser<T> {
    /// `sm_SerConst`: the constant series `v`.
    #[must_use]
    pub fn constant(v: T) -> Self {
        let mut c = T::Storage::filled(T::of(0.0));
        if let Some(c00) = c.rows_mut().first_mut().and_then(|row| row.first_mut()) {
            *c00 = v;
        }
        Self { c }
    }

    /// The coefficient of `sigma^i eps^k` (0 past the series' size).
    #[must_use]
    pub fn at(&self, i: usize, k: usize) -> T {
        self.c
            .rows()
            .get(i)
            .and_then(|row| row.get(k))
            .copied()
            .unwrap_or_else(|| T::of(0.0))
    }

    /// Sets the coefficient of `sigma^i eps^k` (ignored past the series' size).
    pub fn set(&mut self, i: usize, k: usize, v: T) {
        if let Some(x) = self.c.rows_mut().get_mut(i).and_then(|row| row.get_mut(k)) {
            *x = v;
        }
    }

    /// The coefficients kept by `tr` set from `f(i, k)`, the rest 0 (the fork's
    /// `sm_SerConst<T>(T(0.0))` then its loops over `i <= tr.s`, `k <= tr.e`), where `tr`
    /// computes them ([`Trunc::computing`]).
    fn from_fn(tr: Trunc, mut f: impl FnMut(usize, usize) -> T) -> Self {
        let mut r = Self::constant(T::of(0.0));
        for (i, row) in r.c.rows_mut().iter_mut().enumerate().take(tr.s + 1) {
            for (k, x) in row.iter_mut().enumerate().take(tr.e + 1) {
                if tr.computes(i, k) {
                    *x = f(i, k);
                }
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

    /// `self = self.add(&b.scale(f, tr), tr)` in place, for a `self` that is 0 at every
    /// coefficient `tr` does not compute (as every result of these operations under `tr`
    /// is): each computed coefficient is `c + b f`, rounded as the two operations round it.
    pub fn add_scaled(&mut self, b: &Self, f: T, tr: Trunc) {
        for (i, (row, rb)) in self
            .c
            .rows_mut()
            .iter_mut()
            .zip(b.c.rows())
            .enumerate()
            .take(tr.s + 1)
        {
            for (k, (x, &y)) in row.iter_mut().zip(rb).enumerate().take(tr.e + 1) {
                if tr.computes(i, k) {
                    *x = *x + y * f;
                }
            }
        }
    }

    /// [`Ser::add_scaled`] for a `b` that is 0 by construction at every total degree
    /// `i + k` below `v` (a product of `v` factors without constant terms): those
    /// coefficients of `self` are left as they are, as adding an exact 0 leaves them; in
    /// f64 bit for bit [`Ser::add_scaled`]'s, in intervals the same or narrower (b's stored
    /// coefficients there are the rounding of exact zeros).
    pub fn add_scaled_from(&mut self, b: &Self, f: T, v: usize, tr: Trunc) {
        for (i, (row, rb)) in self
            .c
            .rows_mut()
            .iter_mut()
            .zip(b.c.rows())
            .enumerate()
            .take(tr.s + 1)
        {
            for (k, (x, &y)) in row.iter_mut().zip(rb).enumerate().take(tr.e + 1) {
                if i + k >= v && tr.computes(i, k) {
                    *x = *x + y * f;
                }
            }
        }
    }

    /// Copies `src`'s coefficients that `tr` computes into `self`, leaving the rest: an
    /// incremental evaluation's update of a kept series with the coefficients just
    /// computed (or its accumulator's reset to its initial value there).
    pub fn merge_computed(&mut self, src: &Self, tr: Trunc) {
        for (i, (row, rs)) in self
            .c
            .rows_mut()
            .iter_mut()
            .zip(src.c.rows())
            .enumerate()
            .take(tr.s + 1)
        {
            for (k, (x, &y)) in row.iter_mut().zip(rs).enumerate().take(tr.e + 1) {
                if tr.computes(i, k) {
                    *x = y;
                }
            }
        }
    }

    /// The coefficients `tr` computes, the rest 0.
    #[must_use]
    pub fn masked(&self, tr: Trunc) -> Self {
        Self::from_fn(tr, |i, k| self.at(i, k))
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
            // c[i1][k1] b[i - i1][k - k1], i1 then k1 ascending
            let mut sum = T::of(0.0);
            let rows_b = b.c.rows().iter().take(i + 1).rev();
            for (ra, rb) in self.c.rows().iter().take(i + 1).zip(rows_b) {
                for (&x, &y) in ra.iter().take(k + 1).zip(rb.iter().take(k + 1).rev()) {
                    sum = sum + x * y;
                }
            }
            sum
        })
    }

    /// [`Ser::mul`] for factors that are 0 by construction at every total degree below `va`
    /// (`self`) and `vb` (`b`), as a power of a series without a constant term is: the
    /// product is 0 below `va + vb`, and its sums leave out the terms with a factor below
    /// its degree, in [`Ser::mul`]'s order. In f64 bit for bit [`Ser::mul`]'s (the terms
    /// left out are signed zeros); in intervals the same enclosure of the same product or
    /// a narrower one (the factors' stored coefficients there are the rounding of exact
    /// zeros, and every term left out is one rounding less).
    #[must_use]
    pub fn mul_valued(&self, b: &Self, va: usize, vb: usize, tr: Trunc) -> Self {
        Self::from_fn(tr, |i, k| {
            let mut sum = T::of(0.0);
            if i + k < va + vb {
                return sum;
            }
            // c[i1][k1] b[i - i1][k - k1], i1 then k1 ascending, both factors at or above
            // their own degree
            let rows_b = b.c.rows().iter().take(i + 1).rev();
            for (i1, (ra, rb)) in self.c.rows().iter().take(i + 1).zip(rows_b).enumerate() {
                let terms = ra.iter().take(k + 1).zip(rb.iter().take(k + 1).rev());
                for (k1, (&x, &y)) in terms.enumerate() {
                    if i1 + k1 >= va && (i - i1) + (k - k1) >= vb {
                        sum = sum + x * y;
                    }
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
            return Err(SolidError::blend_failed(
                "blend::fillet_section::Ser::inv_sqrt",
                "a face's normal vanishes at a foot",
            ));
        }
        let mut z = self.scale(T::of(1.0) / x0, tr);
        z.set(0, 0, T::of(0.0));
        let mut pow = Self::constant(T::of(1.0));
        let mut sum = Self::constant(T::of(1.0));
        let mut binom = 1.0_f64;
        for m in 1..=(tr.s + tr.e) {
            // dBinom *= (-0.5 - (m - 1)) / m, m an int
            binom *= (-0.5 - (m - 1) as f64) / m as f64;
            // z^m is 0 below degree m (z has no constant term)
            pow = pow.mul_valued(&z, m - 1, 1, tr);
            sum.add_scaled_from(&pow, T::of(binom), m, tr);
        }
        Ok(sum.scale(T::of(1.0) / x0.sqrt(), tr))
    }
}

/// [`Ser::inv_sqrt`] evaluated incrementally: the series it takes (z, its powers, the
/// binomial sum and the result) are kept between calls, and a call computes only the
/// coefficients its truncation computes ([`Trunc::skipping`]: the caller leaves out those
/// whose operands' coefficients are unchanged). Its operations, their order and so every
/// coefficient are [`Ser::inv_sqrt`]'s, bit for bit; the two must change together.
#[derive(Clone, Debug)]
pub struct InvSqrtSeries<T: GnScalar> {
    z: Ser<T>,
    /// `z^m`, `m = 0..`: `pow[0]` is 1.
    pow: Vec<Ser<T>>,
    sum: Ser<T>,
    out: Ser<T>,
}

impl<T: GnScalar> Default for InvSqrtSeries<T> {
    fn default() -> Self {
        let zero = Ser::constant(T::of(0.0));
        Self {
            z: zero.clone(),
            pow: vec![Ser::constant(T::of(1.0))],
            sum: zero.clone(),
            out: zero,
        }
    }
}

impl<T: GnScalar> InvSqrtSeries<T> {
    /// `x^(-1/2)` as [`Ser::inv_sqrt`], its coefficients that `tr` computes updated (the
    /// rest kept from earlier calls). `x` must hold every coefficient below each one
    /// computed.
    ///
    /// # Errors
    ///
    /// As [`Ser::inv_sqrt`].
    pub fn update(&mut self, x: &Ser<T>, tr: Trunc) -> Result<&Ser<T>> {
        let x0 = x.at(0, 0);
        if !x0.is_positive() {
            return Err(SolidError::blend_failed(
                "blend::fillet_section::Ser::inv_sqrt",
                "a face's normal vanishes at a foot",
            ));
        }
        self.z.merge_computed(&x.scale(T::of(1.0) / x0, tr), tr);
        self.z.set(0, 0, T::of(0.0));
        self.sum.merge_computed(&Ser::constant(T::of(1.0)), tr);
        let mut binom = 1.0_f64;
        for m in 1..=(tr.s + tr.e) {
            binom *= (-0.5 - (m - 1) as f64) / m as f64;
            let Some(prev) = self.pow.get(m - 1) else {
                return Err(SolidError::blend_failed(
                    "blend::fillet_section::Ser::inv_sqrt",
                    "the series has no power of the previous order",
                ));
            };
            let next = prev.mul_valued(&self.z, m - 1, 1, tr);
            if self.pow.len() <= m {
                self.pow.push(Ser::constant(T::of(0.0)));
            }
            let Some(pm) = self.pow.get_mut(m) else {
                return Err(SolidError::blend_failed(
                    "blend::fillet_section::Ser::inv_sqrt",
                    "the series has no power of this order",
                ));
            };
            pm.merge_computed(&next, tr);
            self.sum.add_scaled_from(pm, T::of(binom), m, tr);
        }
        self.out
            .merge_computed(&self.sum.scale(T::of(1.0) / x0.sqrt(), tr), tr);
        Ok(&self.out)
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

/// The zero interval vector `(0, 0, 0)`.
pub const VZ: V3<Ivl> = V3 {
    x: Ivl::new(0.0, 0.0),
    y: Ivl::new(0.0, 0.0),
    z: Ivl::new(0.0, 0.0),
};

/// `a + b`, componentwise in rounded intervals.
#[inline]
#[must_use]
pub fn v_add(a: V3<Ivl>, b: V3<Ivl>) -> V3<Ivl> {
    V3 {
        x: a.x + b.x,
        y: a.y + b.y,
        z: a.z + b.z,
    }
}

/// `a - b`, componentwise in rounded intervals.
#[inline]
#[must_use]
pub fn v_sub(a: V3<Ivl>, b: V3<Ivl>) -> V3<Ivl> {
    V3 {
        x: a.x - b.x,
        y: a.y - b.y,
        z: a.z - b.z,
    }
}

/// `a s`, componentwise in rounded intervals.
#[inline]
#[must_use]
pub fn v_scale(a: V3<Ivl>, s: Ivl) -> V3<Ivl> {
    V3 {
        x: a.x * s,
        y: a.y * s,
        z: a.z * s,
    }
}

/// Componentwise interval hull of `a` and `b`.
#[inline]
#[must_use]
pub fn v_hull(a: V3<Ivl>, b: V3<Ivl>) -> V3<Ivl> {
    V3 {
        x: a.x.hull(b.x),
        y: a.y.hull(b.y),
        z: a.z.hull(b.z),
    }
}

/// `sm_VSer`: a vector of series.
#[derive(Clone, Debug, PartialEq)]
pub struct VSer<T: GnScalar> {
    /// The x, y and z series.
    pub a: [Ser<T>; 3],
}

impl<T: GnScalar> Copy for VSer<T> where T::Storage: Copy {}

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

    /// A series without a constant term, with coefficients of mixed sign and size.
    fn no_constant() -> Ser<f64> {
        let mut x = Ser::constant(0.0);
        let mut v = 0.37_f64;
        for i in 0..=MAX_S {
            for k in 0..=MAX_E {
                if i + k > 0 {
                    v = (v * 7.3 + 0.11) % 2.0 - 1.0;
                    x.set(i, k, v * 10f64.powi(i as i32 - 3));
                }
            }
        }
        x
    }

    #[test]
    fn a_power_left_without_its_zero_terms_is_the_product_bit_for_bit_in_f64() {
        let du = no_constant();
        let mut dv = no_constant();
        dv.set(1, 0, -2.5);
        for (s, e) in [(MAX_S, 2), (6, 2), (5, 3), (2, MAX_E)] {
            let tr = Trunc::new(s, e).unwrap();
            for degree in 0..=(s + e) {
                let trd = tr.computing(degree, e);
                let (mut pu, mut pv) = (vec![Ser::constant(1.0)], vec![Ser::constant(1.0)]);
                let (mut qu, mut qv) = (pu.clone(), pv.clone());
                for a in 1..=(s + e) {
                    pu.push(pu[a - 1].mul(&du, trd));
                    pv.push(pv[a - 1].mul(&dv, trd));
                    qu.push(qu[a - 1].mul_valued(&du, a - 1, 1, trd));
                    qv.push(qv[a - 1].mul_valued(&dv, a - 1, 1, trd));
                    assert_eq!(pu[a], qu[a], "{s} {e} {degree} {a}");
                }
                let (mut o, mut q) = (Ser::constant(0.25), Ser::constant(0.25));
                for a in 0..=(s + e) {
                    for b in 0..=(s + e - a) {
                        o.add_scaled(&pu[a].mul(&pv[b], trd), -1.75, trd);
                        q.add_scaled_from(&qu[a].mul_valued(&qv[b], a, b, trd), -1.75, a + b, trd);
                    }
                }
                assert_eq!(o, q, "{s} {e} {degree}");
            }
        }
        let tr = Trunc::new(6, 2).unwrap();
        let mut x = no_constant();
        x.set(0, 0, 3.0);
        let y = x.inv_sqrt(tr).unwrap();
        // the binomial sum the old way: pow.mul and add of scale
        let mut z = x.scale(1.0 / 3.0, tr);
        z.set(0, 0, 0.0);
        let (mut pow, mut sum, mut binom) = (Ser::constant(1.0), Ser::constant(1.0), 1.0_f64);
        for m in 1..=8 {
            binom *= (-0.5 - f64::from(m - 1)) / f64::from(m);
            pow = pow.mul(&z, tr);
            sum = sum.add(&pow.scale(binom, tr), tr);
        }
        assert_eq!(y, sum.scale(1.0 / 3.0_f64.sqrt(), tr));
    }

    #[test]
    fn a_power_left_without_its_zero_terms_encloses_the_product_in_intervals() {
        let tr = Trunc::new(6, 2).unwrap();
        let d = no_constant();
        let mut di = Ser::constant(Ivl::from(0.0));
        for i in 0..=MAX_S {
            for k in 0..=MAX_E {
                di.set(i, k, Ivl::from(d.c[i][k]));
            }
        }
        let (mut p, mut q, mut pd) = (
            Ser::constant(Ivl::from(1.0)),
            Ser::constant(Ivl::from(1.0)),
            Ser::constant(1.0),
        );
        for a in 1..=8 {
            p = p.mul(&di, tr);
            q = q.mul_valued(&di, a - 1, 1, tr);
            pd = pd.mul(&d, tr);
            for i in 0..=MAX_S {
                for k in 0..=MAX_E {
                    let (pc, qc) = (p.c[i][k], q.c[i][k]);
                    // within the old enclosure, and still holding the double product
                    assert!(
                        pc.lo <= qc.lo && qc.hi <= pc.hi,
                        "{a} {i} {k} {pc:?} {qc:?}"
                    );
                    assert!(qc.lo <= pd.c[i][k] && pd.c[i][k] <= qc.hi, "{a} {i} {k}");
                    if i + k < a {
                        assert_eq!((qc.lo, qc.hi), (0.0, 0.0));
                    }
                }
            }
        }
    }

    #[test]
    fn vector_series_dot_and_cross() {
        let tr = Trunc::new(1, 1).unwrap();
        let e1 = VSer::<f64>::constant(V3::from_vec3(Vec3::new(1.0, 0.0, 0.0)));
        let e2 = VSer::constant(V3::from_vec3(Vec3::new(0.0, 1.0, 0.0)));
        let e3 = e1.cross(&e2, tr);
        assert_eq!(e3.a[2].c[0][0], 1.0);
        assert_eq!(e1.dot(&e2, tr).c[0][0], 0.0);
        let s = lin();
        let p = e1.add(&e2, tr).times(&s, tr).sub(&e2, tr);
        assert_eq!(p.a[0].c[1][0], 2.0);
        assert_eq!(p.a[1].c[0][0], 0.0);
    }

    #[test]
    fn a_vanishing_normal_is_a_typed_failure() {
        let tr = Trunc::new(1, 0).unwrap();
        let op = "blend::fillet_section::Ser::inv_sqrt";
        let why = "a face's normal vanishes at a foot";
        let error = Ser::constant(0.0).inv_sqrt(tr).unwrap_err();
        assert_eq!(error, SolidError::blend_failed(op, why));
        assert_eq!(error.upstream_status(), Some(crate::error::SM_ERR));
        let mut pows = InvSqrtSeries::<f64>::default();
        let error = pows.update(&Ser::constant(-1.0), tr).unwrap_err();
        assert_eq!(error, SolidError::blend_failed(op, why));
    }
}
