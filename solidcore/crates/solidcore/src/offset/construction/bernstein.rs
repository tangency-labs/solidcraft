//! Tensor-product Bernstein polynomials on the unit square: the exact algebra the
//! offset certificates bound (products, derivatives, subdivision, coefficient bounds).
//!
//! Upstream: none (not upstream; solidcore-native, tangency-labs/solidcore#2350 §1.1).
//!
//! | Rust              | Upstream |
//! |-------------------|----------|
//! | [`Bernstein2`]    | none     |
//! | [`Binomials`]     | none     |
//! | [`Bounded`]       | none     |
//!
//! A polynomial of degrees `(du, dv)` is `sum c[i][j] B_i^du(s) B_j^dv(t)` on
//! `[0, 1]^2`. Its values lie in the convex hull of its coefficients, so
//! [`Bernstein2::min_coefficient`] and [`Bernstein2::max_coefficient`] bound it
//! (Farouki & Rajan 1987); subdividing tightens the bound.
//!
//! A [`Bounded`] polynomial carries, beside the coefficients floating point gave it, a bound
//! of their distance from the exact polynomial's: a running error bound (Higham 2002, §3.3),
//! each operation's rounding counted as `gamma` of the magnitudes it combines
//! ([`product_rounding`]) and the errors of its operands carried through.
//!
//! Deviations: none (new code). Degrees are bounded by [`MAX_DEGREE`].

use crate::error::{Result, SolidError};
use crate::nurbs::math::gamma;

const OP: &str = "offset::construction::bernstein";

/// The largest degree in either direction a product may reach: above the certificates'
/// degrees, the highest of which is the regularity test's tangent bound on a G2 round's
/// rational septic net, `q1 (x2 + c^2)` of degree 100 (`sheets::RegularityForms`).
pub const MAX_DEGREE: usize = 128;

fn invalid(reason: &'static str) -> SolidError {
    SolidError::InvalidInput {
        operation: OP,
        reason,
    }
}

/// Binomial coefficients `C(n, k)` for `n <= MAX_DEGREE * 2`, as `f64`.
#[derive(Debug, Clone)]
pub struct Binomials {
    rows: Vec<Vec<f64>>,
}

impl Default for Binomials {
    fn default() -> Self {
        Self::new()
    }
}

impl Binomials {
    /// Pascal's triangle up to `2 * MAX_DEGREE`.
    #[must_use]
    pub fn new() -> Self {
        let mut rows: Vec<Vec<f64>> = Vec::new();
        let mut row = vec![1.0];
        for _ in 0..=2 * MAX_DEGREE {
            let next: Vec<f64> = core::iter::once(1.0)
                .chain(row.windows(2).map(|w| match *w {
                    [a, b] => a + b,
                    _ => 0.0,
                }))
                .chain(core::iter::once(1.0))
                .collect();
            rows.push(row);
            row = next;
        }
        Self { rows }
    }

    /// `C(n, k)`; zero for `k > n`.
    #[must_use]
    pub fn get(&self, n: usize, k: usize) -> f64 {
        self.rows
            .get(n)
            .and_then(|r| r.get(k))
            .copied()
            .unwrap_or(0.0)
    }
}

/// A tensor-product Bernstein polynomial on `[0, 1]^2`.
#[derive(Debug, Clone, PartialEq)]
pub struct Bernstein2 {
    du: usize,
    dv: usize,
    /// `c[i * (dv + 1) + j]`.
    c: Vec<f64>,
}

impl Bernstein2 {
    /// The polynomial of degrees `(du, dv)` with coefficients `c` (row `i` holds
    /// `c[i][0..=dv]`).
    ///
    /// # Errors
    ///
    /// `InvalidInput` when `c` does not hold `(du + 1) (dv + 1)` values or a degree
    /// exceeds [`MAX_DEGREE`].
    pub fn new(du: usize, dv: usize, c: Vec<f64>) -> Result<Self> {
        if du > MAX_DEGREE || dv > MAX_DEGREE {
            return Err(invalid("Bernstein degree too large"));
        }
        if c.len() != (du + 1) * (dv + 1) {
            return Err(invalid("Bernstein coefficient count"));
        }
        Ok(Self { du, dv, c })
    }

    /// The constant `x`.
    #[must_use]
    pub fn constant(x: f64) -> Self {
        Self {
            du: 0,
            dv: 0,
            c: vec![x],
        }
    }

    /// The degrees `(du, dv)`.
    #[must_use]
    pub const fn degrees(&self) -> (usize, usize) {
        (self.du, self.dv)
    }

    /// The coefficients, row by row.
    #[must_use]
    pub fn coefficients(&self) -> &[f64] {
        &self.c
    }

    fn rows(&self) -> impl Iterator<Item = &[f64]> + '_ {
        self.c.chunks(self.dv + 1)
    }

    /// The smallest coefficient: a lower bound of the polynomial on the square.
    #[must_use]
    pub fn min_coefficient(&self) -> f64 {
        self.c.iter().copied().fold(f64::INFINITY, f64::min)
    }

    /// The largest coefficient: an upper bound of the polynomial on the square.
    #[must_use]
    pub fn max_coefficient(&self) -> f64 {
        self.c.iter().copied().fold(f64::NEG_INFINITY, f64::max)
    }

    /// The largest coefficient magnitude: a bound of `|f|` on the square.
    #[must_use]
    pub fn max_abs_coefficient(&self) -> f64 {
        self.c.iter().fold(0.0, |m, x| m.max(x.abs()))
    }

    /// `x * self`.
    #[must_use]
    pub fn scale(&self, x: f64) -> Self {
        Self {
            du: self.du,
            dv: self.dv,
            c: self.c.iter().map(|c| c * x).collect(),
        }
    }

    /// The product `self * other`, of degrees the sums of theirs.
    ///
    /// # Errors
    ///
    /// `InvalidInput` when a degree of the product exceeds [`MAX_DEGREE`].
    pub fn mul(&self, other: &Self, b: &Binomials) -> Result<Self> {
        let (du, dv) = (self.du + other.du, self.dv + other.dv);
        if du > MAX_DEGREE || dv > MAX_DEGREE {
            return Err(invalid("Bernstein product degree too large"));
        }
        let mut out = vec![0.0; (du + 1) * (dv + 1)];
        for (ia, ra) in self.rows().enumerate() {
            for (ja, &x) in ra.iter().enumerate() {
                let x = x * b.get(self.du, ia) * b.get(self.dv, ja);
                for (ib, rb) in other.rows().enumerate() {
                    let row = (ia + ib) * (dv + 1);
                    for (jb, &y) in rb.iter().enumerate() {
                        let y = y * b.get(other.du, ib) * b.get(other.dv, jb);
                        if let Some(slot) = out.get_mut(row + ja + jb) {
                            *slot += x * y;
                        }
                    }
                }
            }
        }
        for (i, row) in out.chunks_mut(dv + 1).enumerate() {
            for (j, slot) in row.iter_mut().enumerate() {
                *slot /= b.get(du, i) * b.get(dv, j);
            }
        }
        Self::new(du, dv, out)
    }

    /// The same polynomial written in degrees `(du, dv)` (at least its own).
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a degree below the polynomial's own or above [`MAX_DEGREE`].
    pub fn elevate(&self, du: usize, dv: usize, b: &Binomials) -> Result<Self> {
        if du < self.du || dv < self.dv {
            return Err(invalid("Bernstein elevation below the degree"));
        }
        if du == self.du && dv == self.dv {
            return Ok(self.clone());
        }
        let ones = Self::new(
            du - self.du,
            dv - self.dv,
            vec![1.0; (du - self.du + 1) * (dv - self.dv + 1)],
        )?;
        self.mul(&ones, b)
    }

    /// `self + x * other`, in the larger of the two degrees.
    ///
    /// # Errors
    ///
    /// Those of [`Self::elevate`].
    pub fn add_scaled(&self, other: &Self, x: f64, b: &Binomials) -> Result<Self> {
        let (du, dv) = (self.du.max(other.du), self.dv.max(other.dv));
        let a = self.elevate(du, dv, b)?;
        let o = other.elevate(du, dv, b)?;
        let c = a.c.iter().zip(&o.c).map(|(p, q)| p + x * q).collect();
        Self::new(du, dv, c)
    }

    /// `self + other`.
    ///
    /// # Errors
    ///
    /// Those of [`Self::elevate`].
    pub fn add(&self, other: &Self, b: &Binomials) -> Result<Self> {
        self.add_scaled(other, 1.0, b)
    }

    /// `self - other`.
    ///
    /// # Errors
    ///
    /// Those of [`Self::elevate`].
    pub fn sub(&self, other: &Self, b: &Binomials) -> Result<Self> {
        self.add_scaled(other, -1.0, b)
    }

    /// The partial derivative in the first parameter (degree `du - 1`; the zero
    /// polynomial of degree `(0, dv)` for a constant in it).
    #[must_use]
    pub fn deriv_u(&self) -> Self {
        if self.du == 0 {
            return Self {
                du: 0,
                dv: self.dv,
                c: vec![0.0; self.dv + 1],
            };
        }
        let k = self.du as f64;
        let rows: Vec<&[f64]> = self.rows().collect();
        let c = rows
            .windows(2)
            .flat_map(|w| match *w {
                [a, b] => a
                    .iter()
                    .zip(b.iter())
                    .map(|(x, y)| k * (y - x))
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            })
            .collect();
        Self {
            du: self.du - 1,
            dv: self.dv,
            c,
        }
    }

    /// The partial derivative in the second parameter.
    #[must_use]
    pub fn deriv_v(&self) -> Self {
        if self.dv == 0 {
            return Self {
                du: self.du,
                dv: 0,
                c: vec![0.0; self.du + 1],
            };
        }
        let k = self.dv as f64;
        let c = self
            .rows()
            .flat_map(|r| {
                r.windows(2)
                    .map(|w| match *w {
                        [x, y] => k * (y - x),
                        _ => 0.0,
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        Self {
            du: self.du,
            dv: self.dv - 1,
            c,
        }
    }

    fn transpose(&self) -> Self {
        let mut c = Vec::with_capacity(self.c.len());
        for j in 0..=self.dv {
            c.extend(self.c.iter().skip(j).step_by(self.dv + 1));
        }
        Self {
            du: self.dv,
            dv: self.du,
            c,
        }
    }

    /// The two halves of the polynomial split at `t` of the second parameter, each
    /// reparametrised to `[0, 1]` (de Casteljau).
    #[must_use]
    pub fn split_v(&self, t: f64) -> (Self, Self) {
        let mut left = Vec::with_capacity(self.c.len());
        let mut right = Vec::with_capacity(self.c.len());
        for row in self.rows() {
            let (l, r) = split_1d(row, t);
            left.extend(l);
            right.extend(r);
        }
        (
            Self {
                du: self.du,
                dv: self.dv,
                c: left,
            },
            Self {
                du: self.du,
                dv: self.dv,
                c: right,
            },
        )
    }

    /// The two halves split at `t` of the first parameter.
    #[must_use]
    pub fn split_u(&self, t: f64) -> (Self, Self) {
        let (l, r) = self.transpose().split_v(t);
        (l.transpose(), r.transpose())
    }

    /// The polynomial on `[u0, u1] x [v0, v1]` (inside the square), reparametrised to
    /// the unit square.
    #[must_use]
    pub fn restrict(&self, u0: f64, u1: f64, v0: f64, v1: f64) -> Self {
        let mut p = self.clone();
        if u0 > 0.0 {
            p = p.split_u(u0).1;
        }
        if u1 < 1.0 {
            p = p.split_u((u1 - u0) / (1.0 - u0)).0;
        }
        if v0 > 0.0 {
            p = p.split_v(v0).1;
        }
        if v1 < 1.0 {
            p = p.split_v((v1 - v0) / (1.0 - v0)).0;
        }
        p
    }

    /// The value at `(u, v)` (de Casteljau).
    #[must_use]
    pub fn eval(&self, u: f64, v: f64) -> f64 {
        let mut scratch = Vec::new();
        let column: Vec<f64> = self.rows().map(|r| eval_1d(r, v, &mut scratch)).collect();
        eval_1d(&column, u, &mut scratch)
    }
}

/// de Casteljau's halves of the Bernstein coefficients `c` at `t`.
fn split_1d(c: &[f64], t: f64) -> (Vec<f64>, Vec<f64>) {
    let mut left = Vec::with_capacity(c.len());
    let mut right = Vec::with_capacity(c.len());
    let mut cur = c.to_vec();
    while let (Some(&first), Some(&last)) = (cur.first(), cur.last()) {
        left.push(first);
        right.push(last);
        if cur.len() == 1 {
            break;
        }
        cur = cur
            .windows(2)
            .map(|w| match *w {
                [a, b] => (1.0 - t) * a + t * b,
                _ => 0.0,
            })
            .collect();
    }
    right.reverse();
    (left, right)
}

/// The value at `t` of the Bernstein coefficients `c` (de Casteljau), reduced in place in
/// `cur` (its contents are replaced; passed in so one buffer serves many calls). Each level
/// replaces `cur[i]` by `(1 - t) cur[i] + t cur[i + 1]`, walking down from the end so each
/// right neighbour is still the previous level's.
fn eval_1d(c: &[f64], t: f64, cur: &mut Vec<f64>) -> f64 {
    cur.clear();
    cur.extend_from_slice(c);
    let mut len = cur.len();
    while len > 1 {
        len -= 1;
        let Some((last, head)) = cur.get_mut(..=len).and_then(<[f64]>::split_last_mut) else {
            break;
        };
        let mut right = *last;
        for x in head.iter_mut().rev() {
            let a = *x;
            *x = (1.0 - t) * a + t * right;
            right = a;
        }
    }
    cur.first().copied().unwrap_or(0.0)
}

/// A vector of three [`Bernstein2`]: a polynomial map of the square into space.
pub type Bernstein3 = [Bernstein2; 3];

/// `a . b`.
///
/// # Errors
///
/// Those of [`Bernstein2::mul`].
pub fn dot(a: &Bernstein3, b: &Bernstein3, bi: &Binomials) -> Result<Bernstein2> {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    ax.mul(bx, bi)?
        .add(&ay.mul(by, bi)?, bi)?
        .add(&az.mul(bz, bi)?, bi)
}

/// `a x b`.
///
/// # Errors
///
/// Those of [`Bernstein2::mul`].
pub fn cross(a: &Bernstein3, b: &Bernstein3, bi: &Binomials) -> Result<Bernstein3> {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    Ok([
        ay.mul(bz, bi)?.sub(&az.mul(by, bi)?, bi)?,
        az.mul(bx, bi)?.sub(&ax.mul(bz, bi)?, bi)?,
        ax.mul(by, bi)?.sub(&ay.mul(bx, bi)?, bi)?,
    ])
}

/// `a - b`, componentwise.
///
/// # Errors
///
/// Those of [`Bernstein2::sub`].
pub fn sub3(a: &Bernstein3, b: &Bernstein3, bi: &Binomials) -> Result<Bernstein3> {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    Ok([ax.sub(bx, bi)?, ay.sub(by, bi)?, az.sub(bz, bi)?])
}

/// `a * s`, componentwise by the scalar polynomial `s`.
///
/// # Errors
///
/// Those of [`Bernstein2::mul`].
pub fn mul3(a: &Bernstein3, s: &Bernstein2, bi: &Binomials) -> Result<Bernstein3> {
    let [ax, ay, az] = a;
    Ok([ax.mul(s, bi)?, ay.mul(s, bi)?, az.mul(s, bi)?])
}

/// The componentwise first-parameter derivative.
#[must_use]
pub fn deriv_u3(a: &Bernstein3) -> Bernstein3 {
    let [x, y, z] = a;
    [x.deriv_u(), y.deriv_u(), z.deriv_u()]
}

/// The componentwise second-parameter derivative.
#[must_use]
pub fn deriv_v3(a: &Bernstein3) -> Bernstein3 {
    let [x, y, z] = a;
    [x.deriv_v(), y.deriv_v(), z.deriv_v()]
}

/// The componentwise restriction ([`Bernstein2::restrict`]).
#[must_use]
pub fn restrict3(a: &Bernstein3, u0: f64, u1: f64, v0: f64, v1: f64) -> Bernstein3 {
    let [x, y, z] = a;
    [
        x.restrict(u0, u1, v0, v1),
        y.restrict(u0, u1, v0, v1),
        z.restrict(u0, u1, v0, v1),
    ]
}

/// The componentwise value.
#[must_use]
pub fn eval3(a: &Bernstein3, u: f64, v: f64) -> crate::math::Vec3 {
    let [x, y, z] = a;
    crate::math::Vec3::new(x.eval(u, v), y.eval(u, v), z.eval(u, v))
}

/// The rounded operations behind one coefficient of a Bernstein product of degrees
/// `(du, dv)` ([`Bernstein2::mul`]), relative to the product of the factors' largest
/// coefficient magnitudes. Each of the at most `(du + 1)(dv + 1)` terms multiplies each
/// factor's coefficient by two binomials and the two together, five products; the binomials
/// come from Pascal's triangle, sums of positive numbers rounded past `2^53`, so one of row
/// `n` is within `gamma(n)`, and the four of a term are of rows adding to `du + dv`. The sum
/// takes a rounding per term, and the quotient two binomials of rows `du` and `dv` and two
/// divisions. By Vandermonde's identity the weights of each sum add to 1, so the terms'
/// magnitudes add to at most the factors' largest magnitudes multiplied. A degree elevation
/// is a product with ones and counts the same.
#[must_use]
pub fn product_rounding(du: usize, dv: usize) -> usize {
    let terms = du.saturating_add(1).saturating_mul(dv.saturating_add(1));
    du.saturating_add(dv)
        .saturating_mul(2)
        .saturating_add(terms)
        .saturating_add(7)
}

/// `x` raised past the rounding of the few operations that computed it from nonnegative
/// terms (at most 8, each within the unit roundoff of its value), so a bound computed in
/// floating point is a bound.
fn rounded_up(x: f64) -> f64 {
    x * (1.0 + gamma(16))
}

/// A polynomial as floating point computed it, and `err`, a bound of every coefficient's
/// distance from the coefficient the same operations on the exact operands give (a running
/// error bound, Higham 2002, §3.3; underflow aside, as in the standard model).
#[derive(Debug, Clone)]
pub struct Bounded {
    /// The polynomial as computed.
    pub f: Bernstein2,
    /// The bound of its coefficients' distance from the exact ones.
    pub err: f64,
}

impl Bounded {
    /// `f`, exact.
    #[must_use]
    pub const fn exact(f: Bernstein2) -> Self {
        Self { f, err: 0.0 }
    }

    /// Whether every coefficient and the bound are finite. An overflow leaves an infinite
    /// or NaN coefficient, which the coefficients' extremes pass over, so a form that is
    /// not finite bounds nothing.
    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.err.is_finite() && self.f.coefficients().iter().all(|x| x.is_finite())
    }

    /// The product: the operands' errors carried (`|a| e_b + e_a |b| + e_a e_b`) and the
    /// product's own rounding ([`product_rounding`]).
    ///
    /// # Errors
    ///
    /// Those of [`Bernstein2::mul`].
    pub fn mul(&self, other: &Self, b: &Binomials) -> Result<Self> {
        let f = self.f.mul(&other.f, b)?;
        let (ma, mb) = (self.f.max_abs_coefficient(), other.f.max_abs_coefficient());
        let (du, dv) = f.degrees();
        let carried = ma * other.err + self.err * mb + self.err * other.err;
        let err = rounded_up(carried + gamma(product_rounding(du, dv)) * ma * mb);
        Ok(Self { f, err })
    }

    /// `self + x * other` ([`Bernstein2::add_scaled`]): both raised to the larger degrees
    /// (a product with ones each), then two roundings per coefficient.
    ///
    /// # Errors
    ///
    /// Those of [`Bernstein2::add_scaled`].
    pub fn add_scaled(&self, other: &Self, x: f64, b: &Binomials) -> Result<Self> {
        let f = self.f.add_scaled(&other.f, x, b)?;
        let (du, dv) = f.degrees();
        let size = self.f.max_abs_coefficient() + x.abs() * other.f.max_abs_coefficient();
        let ops = product_rounding(du, dv).saturating_add(2);
        let err = rounded_up(self.err + x.abs() * other.err + gamma(ops) * size);
        Ok(Self { f, err })
    }

    /// `self - other`.
    ///
    /// # Errors
    ///
    /// Those of [`Bernstein2::add_scaled`].
    pub fn sub(&self, other: &Self, b: &Binomials) -> Result<Self> {
        self.add_scaled(other, -1.0, b)
    }

    /// `self + other`.
    ///
    /// # Errors
    ///
    /// Those of [`Bernstein2::add_scaled`].
    pub fn add(&self, other: &Self, b: &Binomials) -> Result<Self> {
        self.add_scaled(other, 1.0, b)
    }

    /// `x * self`: one rounding.
    #[must_use]
    pub fn scale(&self, x: f64) -> Self {
        let f = self.f.scale(x);
        let err = rounded_up(x.abs() * self.err + gamma(1) * f.max_abs_coefficient());
        Self { f, err }
    }

    /// The first-parameter derivative: each coefficient `n (c_(i+1) - c_i)`, two roundings
    /// of its own value, and twice `n` times the carried error.
    #[must_use]
    pub fn deriv_u(&self) -> Self {
        let f = self.f.deriv_u();
        #[allow(clippy::cast_precision_loss)]
        let n = self.f.degrees().0 as f64;
        let err = rounded_up(2.0 * n * self.err + gamma(2) * f.max_abs_coefficient());
        Self { f, err }
    }

    /// The second-parameter derivative, as [`Self::deriv_u`].
    #[must_use]
    pub fn deriv_v(&self) -> Self {
        let f = self.f.deriv_v();
        #[allow(clippy::cast_precision_loss)]
        let n = self.f.degrees().1 as f64;
        let err = rounded_up(2.0 * n * self.err + gamma(2) * f.max_abs_coefficient());
        Self { f, err }
    }

    /// One quarter of the square: `[0, 1/2]` in the first parameter, or `[1/2, 1]` where
    /// `upper_u`, and the same in the second with `upper_v` ([`Bernstein2::restrict`]): one
    /// de Casteljau split at 1/2 in each parameter, `du + dv` levels that each halve exactly
    /// and add with one rounding, convex combinations that carry the error unchanged.
    #[must_use]
    pub fn quarter(&self, upper_u: bool, upper_v: bool) -> Self {
        let (u0, u1) = if upper_u { (0.5, 1.0) } else { (0.0, 0.5) };
        let (v0, v1) = if upper_v { (0.5, 1.0) } else { (0.0, 0.5) };
        let f = self.f.restrict(u0, u1, v0, v1);
        let (du, dv) = f.degrees();
        let err = rounded_up(self.err + gamma(du + dv) * self.f.max_abs_coefficient());
        Self { f, err }
    }
}

/// A vector of three [`Bounded`].
pub type Bounded3 = [Bounded; 3];

/// `a . b` with its error bound, as [`dot`].
///
/// # Errors
///
/// Those of [`Bernstein2::mul`].
pub fn dot_bounded(a: &Bounded3, b: &Bounded3, bi: &Binomials) -> Result<Bounded> {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    ax.mul(bx, bi)?
        .add(&ay.mul(by, bi)?, bi)?
        .add(&az.mul(bz, bi)?, bi)
}

/// `a x b` with its error bound, as [`cross`].
///
/// # Errors
///
/// Those of [`Bernstein2::mul`].
pub fn cross_bounded(a: &Bounded3, b: &Bounded3, bi: &Binomials) -> Result<Bounded3> {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    Ok([
        ay.mul(bz, bi)?.sub(&az.mul(by, bi)?, bi)?,
        az.mul(bx, bi)?.sub(&ax.mul(bz, bi)?, bi)?,
        ax.mul(by, bi)?.sub(&ay.mul(bx, bi)?, bi)?,
    ])
}

/// `a - b` with its error bound, as [`sub3`].
///
/// # Errors
///
/// Those of [`Bernstein2::sub`].
pub fn sub3_bounded(a: &Bounded3, b: &Bounded3, bi: &Binomials) -> Result<Bounded3> {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    Ok([ax.sub(bx, bi)?, ay.sub(by, bi)?, az.sub(bz, bi)?])
}

/// `a * s` with its error bound, as [`mul3`].
///
/// # Errors
///
/// Those of [`Bernstein2::mul`].
pub fn mul3_bounded(a: &Bounded3, s: &Bounded, bi: &Binomials) -> Result<Bounded3> {
    let [ax, ay, az] = a;
    Ok([ax.mul(s, bi)?, ay.mul(s, bi)?, az.mul(s, bi)?])
}

/// The componentwise first-parameter derivative with its error bound.
#[must_use]
pub fn deriv_u3_bounded(a: &Bounded3) -> Bounded3 {
    let [x, y, z] = a;
    [x.deriv_u(), y.deriv_u(), z.deriv_u()]
}

/// The componentwise second-parameter derivative with its error bound.
#[must_use]
pub fn deriv_v3_bounded(a: &Bounded3) -> Bounded3 {
    let [x, y, z] = a;
    [x.deriv_v(), y.deriv_v(), z.deriv_v()]
}

/// The polynomials of `a`, without their bounds.
#[must_use]
pub fn unbounded3(a: Bounded3) -> Bernstein3 {
    let [x, y, z] = a;
    [x.f, y.f, z.f]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::expansion;

    fn poly(du: usize, dv: usize, f: impl Fn(usize, usize) -> f64) -> Bernstein2 {
        let mut c = Vec::new();
        for i in 0..=du {
            for j in 0..=dv {
                c.push(f(i, j));
            }
        }
        Bernstein2::new(du, dv, c).unwrap()
    }

    const SAMPLES: [f64; 5] = [0.0, 0.2, 0.5, 0.7, 1.0];

    #[test]
    fn product_derivative_split_and_elevation_agree_with_values() {
        let b = Binomials::new();
        let p = poly(2, 1, |i, j| (i as f64) * 1.5 - (j as f64) + 0.25);
        let q = poly(1, 3, |i, j| ((i + 2 * j) as f64).sin());
        let pq = p.mul(&q, &b).unwrap();
        let e = p.elevate(4, 3, &b).unwrap();
        let du = pq.deriv_u();
        let (l, r) = pq.split_u(0.3);
        let s = pq.restrict(0.1, 0.6, 0.2, 0.9);
        let h = 1e-6;
        for &u in &SAMPLES {
            for &v in &SAMPLES {
                let want = p.eval(u, v) * q.eval(u, v);
                assert!((pq.eval(u, v) - want).abs() < 1e-12);
                assert!((e.eval(u, v) - p.eval(u, v)).abs() < 1e-12);
                let fd = (pq.eval((u + h).min(1.0), v) - pq.eval((u - h).max(0.0), v))
                    / ((u + h).min(1.0) - (u - h).max(0.0));
                assert!((du.eval(u, v) - fd).abs() < 1e-5, "{u} {v}");
                let lv = l.eval(u, v);
                assert!((lv - pq.eval(0.3 * u, v)).abs() < 1e-12);
                assert!((r.eval(u, v) - pq.eval(0.3 + 0.7 * u, v)).abs() < 1e-12);
                let sv = s.eval(u, v);
                assert!((sv - pq.eval(0.1 + 0.5 * u, 0.2 + 0.7 * v)).abs() < 1e-12);
                assert!(pq.min_coefficient() <= want && want <= pq.max_coefficient());
            }
        }
    }

    #[test]
    fn degrees_past_the_bound_are_refused() {
        let b = Binomials::new();
        let p = poly(MAX_DEGREE, 0, |_, _| 1.0);
        assert!(p.mul(&poly(1, 0, |_, _| 1.0), &b).is_err());
        assert!(Bernstein2::new(1, 1, vec![0.0; 3]).is_err());
    }

    /// The row width `dv + 1` is never zero and cannot overflow: `new` refuses a
    /// degree past [`MAX_DEGREE`] before it counts coefficients, and splitting,
    /// restricting and transposing keep the degrees.
    #[test]
    fn extreme_degrees_neither_panic_nor_change_values() {
        assert!(Bernstein2::new(usize::MAX, usize::MAX, Vec::new()).is_err());
        assert!(Bernstein2::new(0, MAX_DEGREE + 1, vec![0.0; MAX_DEGREE + 2]).is_err());
        let k = Bernstein2::constant(2.5);
        let wide = poly(0, MAX_DEGREE, |_, j| 1.0 + (j as f64) * 0.01);
        let tall = poly(MAX_DEGREE, 0, |i, _| 1.0 - (i as f64) * 0.01);
        for p in [&k, &wide, &tall] {
            let (lu, ru) = p.split_u(0.4);
            let (lv, rv) = p.split_v(0.4);
            let s = p.restrict(0.1, 0.9, 0.2, 0.8);
            for &u in &SAMPLES {
                for &v in &SAMPLES {
                    assert!((lu.eval(u, v) - p.eval(0.4 * u, v)).abs() < 1e-9);
                    assert!((ru.eval(u, v) - p.eval(0.4 + 0.6 * u, v)).abs() < 1e-9);
                    assert!((lv.eval(u, v) - p.eval(u, 0.4 * v)).abs() < 1e-9);
                    assert!((rv.eval(u, v) - p.eval(u, 0.4 + 0.6 * v)).abs() < 1e-9);
                    assert!((s.eval(u, v) - p.eval(0.1 + 0.8 * u, 0.2 + 0.6 * v)).abs() < 1e-9);
                }
            }
            assert_eq!(lu.degrees(), p.degrees());
            assert_eq!(rv.degrees(), p.degrees());
        }
    }

    /// Whether `|c - e / den| <= err`, exactly: `|c den - e| <= err den` in expansions,
    /// `e` the exact numerator and `den > 0` exact.
    fn within(c: f64, e: &[f64], den: f64, err: f64) -> bool {
        let diff = expansion::sub(&expansion::product(c, den).unwrap(), e);
        let room = expansion::product(err, den).unwrap();
        expansion::sign(&expansion::sub(&room, &diff)) >= 0
            && expansion::sign(&expansion::add(&room, &diff)) >= 0
    }

    /// Coefficients of both signs over five decades, none a short binary fraction.
    fn mixed(du: usize, dv: usize, seed: f64) -> Bernstein2 {
        poly(du, dv, |i, j| {
            let k = (i * (dv + 1) + j) as f64;
            ((k + seed) * 0.7).sin() * 10f64.powi((i + 2 * j) as i32 % 5 - 2) + 1.0 / 3.0
        })
    }

    /// The weights of the exact half `[0, 1/2]` (`left`) or `[1/2, 1]` of degree `n` on
    /// coefficient `j` of the new coefficient `i`, dyadic, so exact.
    fn half_weight(b: &Binomials, n: usize, i: usize, j: usize, left: bool) -> f64 {
        let (k, m) = if left {
            (i, j)
        } else {
            (n - i, j.wrapping_sub(i))
        };
        if m > k {
            return 0.0;
        }
        b.get(k, m) * 0.5f64.powi(k as i32)
    }

    /// The exact product of `fa` and `fb`, coefficient by coefficient (row by row): the
    /// numerator, an expansion, and the denominator `C(du, i) C(dv, j)`, exact.
    fn exact_product(b: &Binomials, fa: &Bernstein2, fb: &Bernstein2) -> Vec<(Vec<f64>, f64)> {
        let ((au, av), (bu, bv)) = (fa.degrees(), fb.degrees());
        let (du, dv) = (au + bu, av + bv);
        let mut out = Vec::new();
        for i in 0..=du {
            for j in 0..=dv {
                let mut sum = vec![0.0];
                for (ia, ra) in fa.rows().enumerate() {
                    for (ja, &x) in ra.iter().enumerate() {
                        for (ib, rb) in fb.rows().enumerate() {
                            for (jb, &y) in rb.iter().enumerate() {
                                if ia + ib != i || ja + jb != j {
                                    continue;
                                }
                                let w =
                                    b.get(au, ia) * b.get(av, ja) * b.get(bu, ib) * b.get(bv, jb);
                                let xy = expansion::product(x, y).unwrap();
                                sum = expansion::add(&sum, &expansion::scale(&xy, w).unwrap());
                            }
                        }
                    }
                }
                out.push((sum, b.get(du, i) * b.get(dv, j)));
            }
        }
        out
    }

    /// [`Bounded::mul`] and [`Bounded::quarter`] hold each coefficient within their bound of
    /// the exact one, checked in exact arithmetic (`geom::expansion`): the product's
    /// coefficient times `C(du, i) C(dv, j)` against the exact sum of its terms, and a
    /// quarter of the product, taken as exact, against its exact de Casteljau weights. The
    /// rounding happens (some coefficient is off the exact one) and is counted.
    #[test]
    fn the_counted_rounding_holds_the_exact_coefficients() {
        let b = Binomials::new();
        let (fa, fb) = (mixed(3, 2, 0.1), mixed(4, 3, 0.9));
        let p = Bounded::exact(fa.clone())
            .mul(&Bounded::exact(fb.clone()), &b)
            .unwrap();
        let (du, dv) = p.f.degrees();
        let mut rounded = 0;
        for (k, (sum, den)) in exact_product(&b, &fa, &fb).iter().enumerate() {
            let c = p.f.coefficients()[k];
            assert!(within(c, sum, *den, p.err), "product {k}");
            rounded += usize::from(!within(c, sum, *den, 0.0));
        }
        assert!(rounded > 0);
        for (u0, v0) in [(0.0, 0.5), (0.5, 0.0)] {
            let q = Bounded::exact(p.f.clone()).quarter(u0 > 0.0, v0 > 0.0);
            let mut rounded = 0;
            for i in 0..=du {
                for j in 0..=dv {
                    let mut sum = vec![0.0];
                    for (k, row) in p.f.rows().enumerate() {
                        for (l, &x) in row.iter().enumerate() {
                            let w = half_weight(&b, du, i, k, u0 == 0.0)
                                * half_weight(&b, dv, j, l, v0 == 0.0);
                            if w != 0.0 {
                                sum = expansion::add(&sum, &expansion::product(x, w).unwrap());
                            }
                        }
                    }
                    let c = q.f.coefficients()[i * (dv + 1) + j];
                    assert!(within(c, &sum, 1.0, q.err), "quarter {u0} {v0} {i} {j}");
                    rounded += usize::from(!within(c, &sum, 1.0, 0.0));
                }
            }
            assert!(rounded > 0);
        }
    }

    /// [`Bounded::add_scaled`], [`Bounded::scale`], the derivatives and [`Bounded::mul`] of
    /// an operand that is itself off the exact polynomial `g` by up to its bound: each result
    /// is within its bound of the same operation on `g`, in exact arithmetic, so the operands'
    /// bounds are carried as well as each operation's own rounding counted.
    #[test]
    fn the_counted_rounding_carries_through_sums_scalings_and_derivatives() {
        let b = Binomials::new();
        let (g, h) = (mixed(3, 2, 0.1), mixed(3, 2, 0.9));
        let e = 8.0 * f64::EPSILON * g.max_abs_coefficient();
        let off = poly(3, 2, |i, j| {
            let c = g.coefficients()[i * 3 + j];
            if (i + j) % 2 == 0 {
                c + 0.5 * e
            } else {
                c - 0.25 * e
            }
        });
        let a = Bounded { f: off, err: e };
        let x = -0.7;
        let gc = g.coefficients();
        let s = a.add_scaled(&Bounded::exact(h.clone()), x, &b).unwrap();
        for (k, &c) in s.f.coefficients().iter().enumerate() {
            let want = expansion::add(
                &[gc[k]],
                &expansion::product(x, h.coefficients()[k]).unwrap(),
            );
            assert!(within(c, &want, 1.0, s.err), "sum {k}");
        }
        let t = a.scale(x);
        for (k, &c) in t.f.coefficients().iter().enumerate() {
            let want = expansion::product(x, gc[k]).unwrap();
            assert!(within(c, &want, 1.0, t.err), "scale {k}");
        }
        let du = a.deriv_u();
        for i in 0..3 {
            for j in 0..3 {
                let step = expansion::sub(&[gc[(i + 1) * 3 + j]], &[gc[i * 3 + j]]);
                let want = expansion::scale(&step, 3.0).unwrap();
                assert!(
                    within(du.f.coefficients()[i * 3 + j], &want, 1.0, du.err),
                    "u {i} {j}"
                );
            }
        }
        let dv = a.deriv_v();
        for i in 0..4 {
            for j in 0..2 {
                let step = expansion::sub(&[gc[i * 3 + j + 1]], &[gc[i * 3 + j]]);
                let want = expansion::scale(&step, 2.0).unwrap();
                assert!(
                    within(dv.f.coefficients()[i * 2 + j], &want, 1.0, dv.err),
                    "v {i} {j}"
                );
            }
        }
        let p = a.mul(&Bounded::exact(h.clone()), &b).unwrap();
        for (k, (sum, den)) in exact_product(&b, &g, &h).iter().enumerate() {
            assert!(
                within(p.f.coefficients()[k], sum, *den, p.err),
                "product {k}"
            );
        }
    }
}
