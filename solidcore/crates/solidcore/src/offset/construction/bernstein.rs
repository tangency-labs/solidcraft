//! Tensor-product Bernstein polynomials on the unit square: the exact algebra the
//! offset certificates bound (products, derivatives, subdivision, coefficient bounds).
//!
//! Upstream: none (not upstream; solidcore-native, tangency-labs/solidcore#2350 §1.1).
//!
//! | Rust              | Upstream |
//! |-------------------|----------|
//! | [`Bernstein2`]    | none     |
//! | [`Binomials`]     | none     |
//!
//! A polynomial of degrees `(du, dv)` is `sum c[i][j] B_i^du(s) B_j^dv(t)` on
//! `[0, 1]^2`. Its values lie in the convex hull of its coefficients, so
//! [`Bernstein2::min_coefficient`] and [`Bernstein2::max_coefficient`] bound it
//! (Farouki & Rajan 1987); subdividing tightens the bound.
//!
//! Deviations: none (new code). Degrees are bounded by [`MAX_DEGREE`].

use crate::error::{Result, SolidError};

const OP: &str = "offset::construction::bernstein";

/// The largest degree in either direction a product may reach (well above the
/// certificates' degrees, which stay below 40 for bicubic inputs).
pub const MAX_DEGREE: usize = 96;

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
        let column: Vec<f64> = self.rows().map(|r| eval_1d(r, v)).collect();
        eval_1d(&column, u)
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

/// The value at `t` of the Bernstein coefficients `c` (de Casteljau).
fn eval_1d(c: &[f64], t: f64) -> f64 {
    let mut cur = c.to_vec();
    while cur.len() > 1 {
        cur = cur
            .windows(2)
            .map(|w| match *w {
                [a, b] => (1.0 - t) * a + t * b,
                _ => 0.0,
            })
            .collect();
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
