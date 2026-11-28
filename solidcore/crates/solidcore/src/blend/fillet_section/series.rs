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
use crate::error::blend::{BlendFailure, SectionDegeneracy};
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
    /// Whether the blend's system takes each face's offset point `S + r N` as one Taylor
    /// expansion in the foot's parameters ([`super::station::offset_taylor`]) rather than
    /// as the sum of the expansions of `S` and of `r N`. The two are the same algebra; an
    /// enclosure (an interval or a Taylor model) keeps the cancellation between `S` and
    /// `r N` only in the first, which matters where the offset nears the face's focal
    /// distance (a cylinder's chamfer near its radius), and it takes the first only there
    /// (`station::offset_near_focal`). The floating-point jets keep the sum, as the fork
    /// computes them.
    const JOINT_OFFSET: bool = false;
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
    const JOINT_OFFSET: bool = true;
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

    /// The highest sigma index `i` that can satisfy [`Trunc::computes`].
    #[inline]
    const fn max_i(self) -> usize {
        if self.s < self.degree {
            self.s
        } else {
            self.degree
        }
    }

    /// The highest eps index `k` at sigma index `i <= self.max_i()` that can satisfy
    /// [`Trunc::computes`].
    #[inline]
    const fn max_k(self, i: usize) -> usize {
        let e = if self.e < self.eps { self.e } else { self.eps };
        let rem = self.degree.saturating_sub(i);
        if e < rem {
            e
        } else {
            rem
        }
    }

    /// Whether `(i, k)` is not masked out by `self.skip`.
    #[inline]
    const fn unskipped(self, i: usize, k: usize) -> bool {
        self.skip & Self::bit(i, k) == 0
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

/// `sum_{i1=0..=i} sum_{k1=0..=k} a[i1][k1] * b[i - i1][k - k1]`, `i1` then `k1` ascending.
#[inline]
fn cauchy_at<T: GnScalar>(a: &[[T; STORE_E + 1]], b: &[[T; STORE_E + 1]], i: usize, k: usize) -> T {
    let mut sum = T::of(0.0);
    let rows_b = b.iter().take(i + 1).rev();
    for (ra, rb) in a.iter().take(i + 1).zip(rows_b) {
        for (&x, &y) in ra.iter().take(k + 1).zip(rb.iter().take(k + 1).rev()) {
            sum = sum + x * y;
        }
    }
    sum
}

/// [`cauchy_at`] restricted to terms with `i1 + k1 >= va` and `(i - i1) + (k - k1) >= vb`,
/// in the same `i1` then `k1` ascending order.
#[inline]
fn cauchy_valued_at<T: GnScalar>(
    a: &[[T; STORE_E + 1]],
    b: &[[T; STORE_E + 1]],
    i: usize,
    k: usize,
    va: usize,
    vb: usize,
) -> T {
    let mut sum = T::of(0.0);
    let tot = i + k;
    if tot < va + vb {
        return sum;
    }
    let i1_hi = i.min(tot - vb);
    let rows_b = b.iter().take(i + 1).rev().take(i1_hi + 1);
    for (i1, (ra, rb)) in a.iter().take(i1_hi + 1).zip(rows_b).enumerate() {
        let k1_lo = va.saturating_sub(i1);
        let k1_hi = k.min(tot - vb - i1);
        if k1_lo > k1_hi {
            continue;
        }
        let count = k1_hi - k1_lo + 1;
        let xa = ra.iter().skip(k1_lo).take(count);
        let yb = rb.iter().take(k - k1_lo + 1).rev().take(count);
        for (&x, &y) in xa.zip(yb) {
            sum = sum + x * y;
        }
    }
    sum
}

/// Scalar `x0^(-1/2)` matching [`Ser::inv_sqrt`] at `Trunc(0, 0)` bit for bit, without
/// allocating a series.
pub(crate) fn inv_sqrt_scalar<T: GnScalar>(x0: T) -> Result<T> {
    if !x0.is_positive() {
        return Err(SolidError::blend(
            "blend::fillet_section::Ser::inv_sqrt",
            BlendFailure::SectionDegenerate(SectionDegeneracy::VectorVanishes),
        ));
    }
    Ok(T::of(1.0) * (T::of(1.0) / x0.sqrt()))
}

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

    /// Writes `f(i, k)` into every coefficient `tr` computes, leaving all other slots
    /// untouched (no allocation).
    pub fn write_from_fn(&mut self, tr: Trunc, mut f: impl FnMut(usize, usize) -> T) {
        let i_len = tr.max_i() + 1;
        for (i, row) in self.c.rows_mut().iter_mut().enumerate().take(i_len) {
            let k_len = tr.max_k(i) + 1;
            for (k, x) in row.iter_mut().enumerate().take(k_len) {
                if tr.unskipped(i, k) {
                    *x = f(i, k);
                }
            }
        }
    }

    /// Zeroes every coefficient `tr` computes in place, leaving all other slots untouched.
    pub fn zero_computed(&mut self, tr: Trunc) {
        let zero = T::of(0.0);
        self.write_from_fn(tr, |_, _| zero);
    }

    /// Sets `self` to the constant `v` at every coefficient `tr` computes (`(0, 0)` to `v`,
    /// all other computed coefficients to 0), leaving uncomputed slots untouched.
    pub fn set_constant_computed(&mut self, v: T, tr: Trunc) {
        let zero = T::of(0.0);
        self.write_from_fn(tr, |i, k| if i == 0 && k == 0 { v } else { zero });
    }

    /// Copies all coefficients in `0..=tr.s`, `0..=tr.e` from `src` into `self` in place.
    pub fn copy_trunc_from(&mut self, src: &Self, tr: Trunc) {
        for (row, rs) in self
            .c
            .rows_mut()
            .iter_mut()
            .zip(src.c.rows())
            .take(tr.s + 1)
        {
            for (x, &y) in row.iter_mut().zip(rs).take(tr.e + 1) {
                *x = y;
            }
        }
    }

    /// Zeroes every stored coefficient that `tr` does not compute in place.
    pub fn mask_in_place(&mut self, tr: Trunc) {
        let zero = T::of(0.0);
        for (i, row) in self.c.rows_mut().iter_mut().enumerate() {
            for (k, x) in row.iter_mut().enumerate() {
                if !tr.computes(i, k) {
                    *x = zero;
                }
            }
        }
    }

    /// The coefficients kept by `tr` set from `f(i, k)`, the rest 0 (the fork's
    /// `sm_SerConst<T>(T(0.0))` then its loops over `i <= tr.s`, `k <= tr.e`), where `tr`
    /// computes them ([`Trunc::computing`]).
    fn from_fn(tr: Trunc, f: impl FnMut(usize, usize) -> T) -> Self {
        let mut r = Self::constant(T::of(0.0));
        r.write_from_fn(tr, f);
        r
    }

    /// `sm_SerAdd`.
    #[must_use]
    pub fn add(&self, b: &Self, tr: Trunc) -> Self {
        Self::from_fn(tr, |i, k| self.at(i, k) + b.at(i, k))
    }

    /// Writes `self + b` at every coefficient `tr` computes into `out`, leaving other slots
    /// of `out` untouched.
    pub fn add_into(&self, b: &Self, tr: Trunc, out: &mut Self) {
        let i_len = tr.max_i() + 1;
        for (i, ((ro, ra), rb)) in out
            .c
            .rows_mut()
            .iter_mut()
            .zip(self.c.rows())
            .zip(b.c.rows())
            .enumerate()
            .take(i_len)
        {
            let k_len = tr.max_k(i) + 1;
            for (k, ((o, &x), &y)) in ro.iter_mut().zip(ra).zip(rb).enumerate().take(k_len) {
                if tr.unskipped(i, k) {
                    *o = x + y;
                }
            }
        }
    }

    /// `sm_SerSub`.
    #[must_use]
    pub fn sub(&self, b: &Self, tr: Trunc) -> Self {
        Self::from_fn(tr, |i, k| self.at(i, k) - b.at(i, k))
    }

    /// Writes `self - b` at every coefficient `tr` computes into `out`, leaving other slots
    /// of `out` untouched.
    pub fn sub_into(&self, b: &Self, tr: Trunc, out: &mut Self) {
        let i_len = tr.max_i() + 1;
        for (i, ((ro, ra), rb)) in out
            .c
            .rows_mut()
            .iter_mut()
            .zip(self.c.rows())
            .zip(b.c.rows())
            .enumerate()
            .take(i_len)
        {
            let k_len = tr.max_k(i) + 1;
            for (k, ((o, &x), &y)) in ro.iter_mut().zip(ra).zip(rb).enumerate().take(k_len) {
                if tr.unskipped(i, k) {
                    *o = x - y;
                }
            }
        }
    }

    /// `self = self.add(&b.scale(f, tr), tr)` in place, for a `self` that is 0 at every
    /// coefficient `tr` does not compute (as every result of these operations under `tr`
    /// is): each computed coefficient is `c + b f`, rounded as the two operations round it.
    pub fn add_scaled(&mut self, b: &Self, f: T, tr: Trunc) {
        let i_len = tr.max_i() + 1;
        for (i, (row, rb)) in self
            .c
            .rows_mut()
            .iter_mut()
            .zip(b.c.rows())
            .enumerate()
            .take(i_len)
        {
            let k_len = tr.max_k(i) + 1;
            for (k, (x, &y)) in row.iter_mut().zip(rb).enumerate().take(k_len) {
                if tr.unskipped(i, k) {
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
        if tr.degree < v {
            return;
        }
        let i_len = tr.max_i() + 1;
        for (i, (row, rb)) in self
            .c
            .rows_mut()
            .iter_mut()
            .zip(b.c.rows())
            .enumerate()
            .take(i_len)
        {
            let k_len = tr.max_k(i) + 1;
            for (k, (x, &y)) in row.iter_mut().zip(rb).enumerate().take(k_len) {
                if i + k >= v && tr.unskipped(i, k) {
                    *x = *x + y * f;
                }
            }
        }
    }

    /// Copies `src`'s coefficients that `tr` computes into `self`, leaving the rest: an
    /// incremental evaluation's update of a kept series with the coefficients just
    /// computed (or its accumulator's reset to its initial value there).
    pub fn merge_computed(&mut self, src: &Self, tr: Trunc) {
        let i_len = tr.max_i() + 1;
        for (i, (row, rs)) in self
            .c
            .rows_mut()
            .iter_mut()
            .zip(src.c.rows())
            .enumerate()
            .take(i_len)
        {
            let k_len = tr.max_k(i) + 1;
            for (k, (x, &y)) in row.iter_mut().zip(rs).enumerate().take(k_len) {
                if tr.unskipped(i, k) {
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

    /// Writes `self * f` at every coefficient `tr` computes into `out`, leaving other slots
    /// of `out` untouched.
    pub fn scale_into(&self, f: T, tr: Trunc, out: &mut Self) {
        let i_len = tr.max_i() + 1;
        for (i, (ro, ra)) in out
            .c
            .rows_mut()
            .iter_mut()
            .zip(self.c.rows())
            .enumerate()
            .take(i_len)
        {
            let k_len = tr.max_k(i) + 1;
            for (k, (o, &x)) in ro.iter_mut().zip(ra).enumerate().take(k_len) {
                if tr.unskipped(i, k) {
                    *o = x * f;
                }
            }
        }
    }

    /// Multiplies every coefficient `tr` computes by `f` in place.
    pub fn scale_in_place(&mut self, f: T, tr: Trunc) {
        let i_len = tr.max_i() + 1;
        for (i, row) in self.c.rows_mut().iter_mut().enumerate().take(i_len) {
            let k_len = tr.max_k(i) + 1;
            for (k, x) in row.iter_mut().enumerate().take(k_len) {
                if tr.unskipped(i, k) {
                    *x = *x * f;
                }
            }
        }
    }

    /// `sm_SerMul`: the truncated product (the sum over `i1 <= i`, then `k1 <= k`).
    #[must_use]
    pub fn mul(&self, b: &Self, tr: Trunc) -> Self {
        let mut r = Self::constant(T::of(0.0));
        self.mul_into(b, tr, &mut r);
        r
    }

    /// Writes the truncated product `self * b` at every coefficient `tr` computes into `out`,
    /// leaving other slots of `out` untouched.
    pub fn mul_into(&self, b: &Self, tr: Trunc, out: &mut Self) {
        let a_rows = self.c.rows();
        let b_rows = b.c.rows();
        let i_len = tr.max_i() + 1;
        for (i, row) in out.c.rows_mut().iter_mut().enumerate().take(i_len) {
            let k_len = tr.max_k(i) + 1;
            for (k, x) in row.iter_mut().enumerate().take(k_len) {
                if tr.unskipped(i, k) {
                    *x = cauchy_at(a_rows, b_rows, i, k);
                }
            }
        }
    }

    /// Writes `self + b * c` at every coefficient `tr` computes into `out`, with the same
    /// rounding order as `self.add(&b.mul(c, tr), tr)` and no temporary allocation.
    pub fn add_mul_into(&self, b: &Self, c: &Self, tr: Trunc, out: &mut Self) {
        let b_rows = b.c.rows();
        let c_rows = c.c.rows();
        let i_len = tr.max_i() + 1;
        for (i, (ro, ra)) in out
            .c
            .rows_mut()
            .iter_mut()
            .zip(self.c.rows())
            .enumerate()
            .take(i_len)
        {
            let k_len = tr.max_k(i) + 1;
            for (k, (o, &x)) in ro.iter_mut().zip(ra).enumerate().take(k_len) {
                if tr.unskipped(i, k) {
                    *o = x + cauchy_at(b_rows, c_rows, i, k);
                }
            }
        }
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
        let mut r = Self::constant(T::of(0.0));
        self.mul_valued_into(b, va, vb, tr, &mut r);
        r
    }

    /// Writes [`Ser::mul_valued`] at every coefficient `tr` computes into `out`, leaving
    /// other slots of `out` untouched.
    pub fn mul_valued_into(&self, b: &Self, va: usize, vb: usize, tr: Trunc, out: &mut Self) {
        let a_rows = self.c.rows();
        let b_rows = b.c.rows();
        let i_len = tr.max_i() + 1;
        for (i, row) in out.c.rows_mut().iter_mut().enumerate().take(i_len) {
            let k_len = tr.max_k(i) + 1;
            for (k, x) in row.iter_mut().enumerate().take(k_len) {
                if tr.unskipped(i, k) {
                    *x = cauchy_valued_at(a_rows, b_rows, i, k, va, vb);
                }
            }
        }
    }

    /// `sm_SerInvSqrt`: `x^(-1/2)` for a series whose constant term is positive,
    /// `x0^(-1/2) (1 + z)^(-1/2)`, `z = x / x0 - 1`, summed as the binomial series in z
    /// (which has no constant term) to `tr.s + tr.e` terms.
    pub fn inv_sqrt(&self, tr: Trunc) -> Result<Self> {
        let x0 = self.at(0, 0);
        let inv_root = inv_sqrt_scalar(x0)?;
        let max_m = (tr.s + tr.e).min(tr.degree);
        if max_m == 0 {
            let mut out = Self::constant(T::of(0.0));
            if tr.computes(0, 0) {
                out.set(0, 0, inv_root);
            }
            return Ok(out);
        }
        let mut z = self.scale(T::of(1.0) / x0, tr);
        z.set(0, 0, T::of(0.0));
        let mut pow = Self::constant(T::of(1.0));
        let mut next = Self::constant(T::of(0.0));
        let mut sum = Self::constant(T::of(0.0));
        sum.set_constant_computed(T::of(1.0), tr);
        let mut binom = 1.0_f64;
        for m in 1..=max_m {
            // dBinom *= (-0.5 - (m - 1)) / m, m an int
            binom *= (-0.5 - (m - 1) as f64) / m as f64;
            // z^m is 0 below degree m (z has no constant term)
            pow.mul_valued_into(&z, m - 1, 1, tr, &mut next);
            core::mem::swap(&mut pow, &mut next);
            if m == 1 {
                // Clear the initial 1.0 swapped into `next` so uncomputed slots remain 0.
                next.set(0, 0, T::of(0.0));
            }
            sum.add_scaled_from(&pow, T::of(binom), m, tr);
        }
        sum.scale_in_place(T::of(1.0) / x0.sqrt(), tr);
        Ok(sum)
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
            return Err(SolidError::blend(
                "blend::fillet_section::Ser::inv_sqrt",
                BlendFailure::SectionDegenerate(SectionDegeneracy::VectorVanishes),
            ));
        }
        x.scale_into(T::of(1.0) / x0, tr, &mut self.z);
        self.z.set(0, 0, T::of(0.0));
        self.sum.set_constant_computed(T::of(1.0), tr);
        let max_m = (tr.s + tr.e).min(tr.degree);
        while self.pow.len() <= max_m {
            self.pow.push(Ser::constant(T::of(0.0)));
        }
        let mut binom = 1.0_f64;
        for m in 1..=max_m {
            binom *= (-0.5 - (m - 1) as f64) / m as f64;
            let Some((left, right)) = self.pow.split_at_mut_checked(m) else {
                return Err(SolidError::blend(
                    "blend::fillet_section::Ser::inv_sqrt",
                    BlendFailure::Defect {
                        why: "the kept powers of z lack this order after resizing to max_m",
                    },
                ));
            };
            let (Some(prev), Some(pm)) = (left.last(), right.first_mut()) else {
                return Err(SolidError::blend(
                    "blend::fillet_section::Ser::inv_sqrt",
                    BlendFailure::Defect {
                        why: "the kept powers of z lack the previous or current order",
                    },
                ));
            };
            prev.mul_valued_into(&self.z, m - 1, 1, tr, pm);
            self.sum.add_scaled_from(pm, T::of(binom), m, tr);
        }
        self.sum
            .scale_into(T::of(1.0) / x0.sqrt(), tr, &mut self.out);
        Ok(&self.out)
    }

    /// The last updated output series.
    #[must_use]
    pub const fn out(&self) -> &Ser<T> {
        &self.out
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

    /// Zeroes every coefficient `tr` computes in each component in place.
    pub fn zero_computed(&mut self, tr: Trunc) {
        for c in &mut self.a {
            c.zero_computed(tr);
        }
    }

    /// Copies `src`'s coefficients that `tr` computes into `self` in place.
    pub fn merge_computed(&mut self, src: &Self, tr: Trunc) {
        for (a, b) in self.a.iter_mut().zip(&src.a) {
            a.merge_computed(b, tr);
        }
    }

    /// Zeroes every stored coefficient that `tr` does not compute in each component in place.
    pub fn mask_in_place(&mut self, tr: Trunc) {
        for c in &mut self.a {
            c.mask_in_place(tr);
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

    /// Writes `self + q` at every coefficient `tr` computes into `out`.
    pub fn add_into(&self, q: &Self, tr: Trunc, out: &mut Self) {
        for ((p, b), o) in self.a.iter().zip(&q.a).zip(&mut out.a) {
            p.add_into(b, tr, o);
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

    /// Writes `self - q` at every coefficient `tr` computes into `out`.
    pub fn sub_into(&self, q: &Self, tr: Trunc, out: &mut Self) {
        for ((p, b), o) in self.a.iter().zip(&q.a).zip(&mut out.a) {
            p.sub_into(b, tr, o);
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

    /// Writes `self * s` at every coefficient `tr` computes into `out`.
    pub fn times_into(&self, s: &Ser<T>, tr: Trunc, out: &mut Self) {
        for (p, o) in self.a.iter().zip(&mut out.a) {
            p.mul_into(s, tr, o);
        }
    }

    /// Writes `self + b * s` at every coefficient `tr` computes into `out`, with the same
    /// rounding order as `self.add(&b.times(s, tr), tr)` and no temporary allocation.
    pub fn add_times_into(&self, b: &Self, s: &Ser<T>, tr: Trunc, out: &mut Self) {
        for ((a, bv), o) in self.a.iter().zip(&b.a).zip(&mut out.a) {
            a.add_mul_into(bv, s, tr, o);
        }
    }

    /// `sm_VSerDot`: `(p0 q0 + p1 q1) + p2 q2`.
    #[must_use]
    pub fn dot(&self, q: &Self, tr: Trunc) -> Ser<T> {
        let mut r = Ser::constant(T::of(0.0));
        self.dot_into(q, tr, &mut r);
        r
    }

    /// Writes `(p0 q0 + p1 q1) + p2 q2` at every coefficient `tr` computes into `out`, with
    /// the exact rounding order of [`VSer::dot`] and no temporary allocation.
    pub fn dot_into(&self, q: &Self, tr: Trunc, out: &mut Ser<T>) {
        let [p0, p1, p2] = &self.a;
        let [q0, q1, q2] = &q.a;
        let (p0r, p1r, p2r) = (p0.c.rows(), p1.c.rows(), p2.c.rows());
        let (q0r, q1r, q2r) = (q0.c.rows(), q1.c.rows(), q2.c.rows());
        let i_len = tr.max_i() + 1;
        for (i, row) in out.c.rows_mut().iter_mut().enumerate().take(i_len) {
            let k_len = tr.max_k(i) + 1;
            for (k, x) in row.iter_mut().enumerate().take(k_len) {
                if tr.unskipped(i, k) {
                    let s0 = cauchy_at(p0r, q0r, i, k);
                    let s1 = cauchy_at(p1r, q1r, i, k);
                    let s2 = cauchy_at(p2r, q2r, i, k);
                    *x = (s0 + s1) + s2;
                }
            }
        }
    }

    /// `sm_VSerCross`.
    #[must_use]
    pub fn cross(&self, q: &Self, tr: Trunc) -> Self {
        let zero = V3 {
            x: T::of(0.0),
            y: T::of(0.0),
            z: T::of(0.0),
        };
        let mut r = Self::constant(zero);
        self.cross_into(q, tr, &mut r);
        r
    }

    /// Writes `self x q` at every coefficient `tr` computes into `out`, with the exact
    /// rounding order of [`VSer::cross`] and no temporary allocation.
    pub fn cross_into(&self, q: &Self, tr: Trunc, out: &mut Self) {
        let [p0, p1, p2] = &self.a;
        let [q0, q1, q2] = &q.a;
        let (p0r, p1r, p2r) = (p0.c.rows(), p1.c.rows(), p2.c.rows());
        let (q0r, q1r, q2r) = (q0.c.rows(), q1.c.rows(), q2.c.rows());
        let [o0, o1, o2] = &mut out.a;
        let i_len = tr.max_i() + 1;
        for (i, ((r0, r1), r2)) in
            o0.c.rows_mut()
                .iter_mut()
                .zip(o1.c.rows_mut())
                .zip(o2.c.rows_mut())
                .enumerate()
                .take(i_len)
        {
            let k_len = tr.max_k(i) + 1;
            for (k, ((x0, x1), x2)) in r0.iter_mut().zip(r1).zip(r2).enumerate().take(k_len) {
                if tr.unskipped(i, k) {
                    *x0 = cauchy_at(p1r, q2r, i, k) - cauchy_at(p2r, q1r, i, k);
                    *x1 = cauchy_at(p2r, q0r, i, k) - cauchy_at(p0r, q2r, i, k);
                    *x2 = cauchy_at(p0r, q1r, i, k) - cauchy_at(p1r, q0r, i, k);
                }
            }
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
        let why = BlendFailure::SectionDegenerate(SectionDegeneracy::VectorVanishes);
        let error = Ser::constant(0.0).inv_sqrt(tr).unwrap_err();
        assert_eq!(error, SolidError::blend(op, why));
        assert_eq!(error.upstream_status(), Some(crate::error::SM_ERR));
        let mut pows = InvSqrtSeries::<f64>::default();
        let error = pows.update(&Ser::constant(-1.0), tr).unwrap_err();
        assert_eq!(error, SolidError::blend(op, why));
    }

    #[test]
    fn in_place_series_ops_match_reference_bit_for_bit() {
        let mut a_f = no_constant();
        a_f.set(0, 0, 2.25);
        let mut b_f = no_constant();
        b_f.set(0, 0, -1.5);
        b_f.set(2, 1, 0.75);
        let mut a_i = Ser::constant(Ivl::new(2.0, 2.5));
        let mut b_i = Ser::constant(Ivl::new(-2.0, -1.0));
        for i in 0..=MAX_S {
            for k in 0..=MAX_E {
                if i + k > 0 {
                    let av = a_f.at(i, k);
                    let bv = b_f.at(i, k);
                    a_i.set(i, k, Ivl::new(av - 0.05, av + 0.05));
                    b_i.set(i, k, Ivl::new(bv - 0.03, bv + 0.07));
                }
            }
        }
        let sentinel_i = Ivl::new(-999.0, 999.0);
        for (s, e) in [(MAX_S, MAX_E), (6, 2), (2, 0), (0, STORE_E)] {
            let base = Trunc::new(s, e).unwrap();
            let skip = if s >= 1 { Trunc::bit(1, 0) } else { 0 };
            for tr in [
                base,
                base.computing((s + e) / 2, e.min(2)),
                base.skipping(skip),
            ] {
                let check_ivl = |got: &Ser<Ivl>, want: &Ser<Ivl>, label: &str| {
                    for i in 0..=MAX_S {
                        for k in 0..=STORE_E {
                            if tr.computes(i, k) {
                                assert_eq!(
                                    got.at(i, k),
                                    want.at(i, k),
                                    "{label} ({s},{e}) ({i},{k})"
                                );
                            } else {
                                assert_eq!(
                                    got.at(i, k),
                                    sentinel_i,
                                    "{label} clobbered uncomputed ({i},{k})"
                                );
                            }
                        }
                    }
                };
                let mut out = Ser::<Ivl> {
                    c: <Ivl as GnScalar>::Storage::filled(sentinel_i),
                };
                a_i.add_into(&b_i, tr, &mut out);
                check_ivl(&out, &a_i.add(&b_i, tr), "add_into");

                out.c = <Ivl as GnScalar>::Storage::filled(sentinel_i);
                a_i.sub_into(&b_i, tr, &mut out);
                check_ivl(&out, &a_i.sub(&b_i, tr), "sub_into");

                let f = Ivl::new(-1.25, 0.75);
                out.c = <Ivl as GnScalar>::Storage::filled(sentinel_i);
                a_i.scale_into(f, tr, &mut out);
                check_ivl(&out, &a_i.scale(f, tr), "scale_into");

                out.c = <Ivl as GnScalar>::Storage::filled(sentinel_i);
                a_i.mul_into(&b_i, tr, &mut out);
                check_ivl(&out, &a_i.mul(&b_i, tr), "mul_into");

                out.c = <Ivl as GnScalar>::Storage::filled(sentinel_i);
                a_i.mul_valued_into(&b_i, 1, 1, tr, &mut out);
                check_ivl(&out, &a_i.mul_valued(&b_i, 1, 1, tr), "mul_valued_into");

                out.c = <Ivl as GnScalar>::Storage::filled(sentinel_i);
                a_i.add_mul_into(&a_i, &b_i, tr, &mut out);
                let want_am = a_i.add(&a_i.mul(&b_i, tr), tr);
                check_ivl(&out, &want_am, "add_mul_into");

                // VSer dot_into and cross_into match reference (including rounding order).
                let va = VSer {
                    a: [a_i.clone(), b_i.clone(), a_i.add(&b_i, base)],
                };
                let vb = VSer {
                    a: [b_i.clone(), a_i.sub(&b_i, base), a_i.clone()],
                };
                out.c = <Ivl as GnScalar>::Storage::filled(sentinel_i);
                va.dot_into(&vb, tr, &mut out);
                let want_dot = va.a[0]
                    .mul(&vb.a[0], tr)
                    .add(&va.a[1].mul(&vb.a[1], tr), tr)
                    .add(&va.a[2].mul(&vb.a[2], tr), tr);
                check_ivl(&out, &want_dot, "dot_into");

                let mut vout = VSer {
                    a: core::array::from_fn(|_| Ser::<Ivl> {
                        c: <Ivl as GnScalar>::Storage::filled(sentinel_i),
                    }),
                };
                va.cross_into(&vb, tr, &mut vout);
                let want_cross = [
                    va.a[1]
                        .mul(&vb.a[2], tr)
                        .sub(&va.a[2].mul(&vb.a[1], tr), tr),
                    va.a[2]
                        .mul(&vb.a[0], tr)
                        .sub(&va.a[0].mul(&vb.a[2], tr), tr),
                    va.a[0]
                        .mul(&vb.a[1], tr)
                        .sub(&va.a[1].mul(&vb.a[0], tr), tr),
                ];
                for (got, want) in vout.a.iter().zip(&want_cross) {
                    check_ivl(got, want, "cross_into");
                }
            }
            // InvSqrtSeries incremental updates match Ser::inv_sqrt bit for bit.
            let want_inv_i = a_i.inv_sqrt(base).unwrap();
            let want_inv_f = a_f.inv_sqrt(base).unwrap();
            let mut inc_i = InvSqrtSeries::<Ivl>::default();
            let mut inc_f = InvSqrtSeries::<f64>::default();
            let mut seen = 0_u64;
            for deg in 0..=(s + e) {
                let step = base.computing(deg, e).skipping(seen);
                let gi = inc_i.update(&a_i, step).unwrap().clone();
                let gf = inc_f.update(&a_f, step).unwrap();
                seen |= step.computed_mask();
                for i in 0..=s {
                    for k in 0..=e {
                        if i + k <= deg {
                            assert_eq!(gi.at(i, k), want_inv_i.at(i, k));
                            assert_eq!(gf.at(i, k).to_bits(), want_inv_f.at(i, k).to_bits());
                        }
                    }
                }
            }
        }
        // Edge cases: wide intervals, whole line, NaN, and non-positive constant term.
        let tr = Trunc::new(2, 1).unwrap();
        let mut wide = Ser::constant(Ivl::new(0.25, 100.0));
        wide.set(1, 0, Ivl::new(-10.0, 10.0));
        wide.set(0, 1, Ivl::whole());
        let mut inc_w = InvSqrtSeries::<Ivl>::default();
        let got_w = inc_w.update(&wide, tr).unwrap();
        let want_w = wide.inv_sqrt(tr).unwrap();
        assert_eq!(got_w, &want_w);
        assert!(Ser::constant(Ivl::new(-1.0, 1.0)).inv_sqrt(tr).is_err());
        assert!(Ser::constant(Ivl::new(f64::NAN, 1.0)).inv_sqrt(tr).is_err());
        assert!(Ser::constant(Ivl::whole()).inv_sqrt(tr).is_err());
    }
}
