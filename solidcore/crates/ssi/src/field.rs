//! The field on a Bézier piece's parameter rectangle: an analytic surface's
//! implicit equation composed with the piece, as a tensor Bernstein polynomial.
//!
//! On a rational piece x(s, t) = X(s, t) / W(s, t), an implicit equation f of
//! degree k gives g = W^k f(X / W), a polynomial of degree (k du, k dv). W is
//! positive, so g has the sign of f at every point of the piece, and g's
//! Bernstein coefficients bound it there (Farouki 1986; Sederberg and Nishita
//! 1990). Every coefficient zero: the piece lies on the surface. Every
//! coefficient of one sign: the piece is apart from it. Both verdicts are read
//! off the coefficients, a proof up to rounding, and neither looks at a sampled
//! point's distance.

use std::cell::Cell;

use crate::geom3::{Frame, V3};
use crate::surface3::Surf;

use crate::algebraic::Algebraic;
use crate::piece::Piece;

/// A tensor Bernstein polynomial of degree (du, dv), coefficient (i, j) at i * (dv + 1) + j.
#[derive(Clone, Debug)]
pub struct Bern {
    /// The degree in s.
    pub du: usize,
    /// The degree in t.
    pub dv: usize,
    /// The (du + 1) (dv + 1) coefficients, (i, j) at i * (dv + 1) + j.
    pub c: Vec<f64>,
}

/// The rows of Pascal's triangle held as constants: every product's but the
/// highest-degree fields' (the ssi-corpus asks for rows up to 166), whose
/// rows are built past them.
const PASCAL_ROWS: usize = 128;
const PASCAL_LEN: usize = PASCAL_ROWS * (PASCAL_ROWS + 1) / 2;

/// Pascal's triangle to row `PASCAL_ROWS` - 1, row n from n (n + 1) / 2, each
/// entry the sum in f64 of the two above it (exact to row 56, rounded as
/// added past it). Built at compile time: rebuilt for every product the rows
/// cost as much as the product itself, and a cache would be global state.
static PASCAL: [f64; PASCAL_LEN] = pascal();

// Evaluated at compile time only: an index out of range stops the build rather
// than a run, and the array is the static's value, never a frame at run time.
#[allow(clippy::indexing_slicing, clippy::large_stack_arrays)]
const fn pascal() -> [f64; PASCAL_LEN] {
    let mut t = [0.0; PASCAL_LEN];
    let mut n = 0;
    while n < PASCAL_ROWS {
        let start = n * (n + 1) / 2;
        let mut k = 0;
        while k <= n {
            t[start + k] = if k == 0 || k == n {
                1.0
            } else {
                t[start - n + k - 1] + t[start - n + k]
            };
            k += 1;
        }
        n += 1;
    }
    t
}

/// Pascal's triangle to some row: the constant rows, then those built past
/// them as the constant rows are (each entry the sum of the two above it).
struct Rows {
    past: Vec<Vec<f64>>,
}

impl Rows {
    /// The triangle to row n at least.
    fn to(n: usize) -> Rows {
        let mut past: Vec<Vec<f64>> = Vec::new();
        for m in PASCAL_ROWS..=n {
            let last = past
                .last()
                .map_or_else(|| constant_row(m - 1), Vec::as_slice);
            let row = (0..=m)
                .map(|k| {
                    if k == 0 || k == m {
                        1.0
                    } else {
                        last.get(k - 1)
                            .zip(last.get(k))
                            .map_or(f64::NAN, |(a, b)| a + b)
                    }
                })
                .collect();
            past.push(row);
        }
        Rows { past }
    }

    /// Row n (empty past the rows held).
    fn row(&self, n: usize) -> &[f64] {
        match n.checked_sub(PASCAL_ROWS) {
            None => constant_row(n),
            Some(m) => self.past.get(m).map_or(&[], Vec::as_slice),
        }
    }

    /// C(n, k) as the triangle holds it; NaN past it.
    fn binomial(&self, n: usize, k: usize) -> f64 {
        self.row(n).get(k).copied().unwrap_or(f64::NAN)
    }
}

/// Row n of the constant rows.
fn constant_row(n: usize) -> &'static [f64] {
    let start = n * (n + 1) / 2;
    PASCAL.get(start..=start + n).unwrap_or_default()
}

/// One de Casteljau level on the first `count + 1` values: value i becomes
/// `step(value i, value i + 1)`, for i below `count`.
pub(crate) fn casteljau_level(work: &mut [f64], count: usize, step: impl Fn(f64, f64) -> f64) {
    for pair in Cell::from_mut(work)
        .as_slice_of_cells()
        .windows(2)
        .take(count)
    {
        if let [a, b] = pair {
            a.set(step(a.get(), b.get()));
        }
    }
}

/// de Casteljau halving of `work` at x, handing out the left half's points
/// (index r, value) and the right half's: one level per point, as
/// `casteljau_level` with `step`.
fn casteljau_halves(
    work: &mut [f64],
    step: impl Fn(f64, f64) -> f64,
    mut left: impl FnMut(usize, f64),
    mut right: impl FnMut(usize, f64),
) {
    let n = work.len().saturating_sub(1);
    for r in 0..=n {
        if let Some(&w) = work.first() {
            left(r, w);
        }
        if let Some(&w) = work.get(n - r) {
            right(n - r, w);
        }
        casteljau_level(work, n - r, &step);
    }
}

impl Bern {
    /// The product, by the Bernstein product rule: C(m, i) C(n, k) / C(m + n, i + k)
    /// in each direction, scaled in and out once instead of per term (binomial
    /// rows computed once per product; per term they were most of its cost).
    #[must_use]
    pub fn mul(&self, o: &Bern) -> Bern {
        let (du, dv) = (self.du + o.du, self.dv + o.dv);
        self.mul_with(o, &Rows::to(du.max(dv)))
    }

    /// `mul` with Pascal's triangle at hand.
    fn mul_with(&self, o: &Bern, rows: &Rows) -> Bern {
        let (du, dv) = (self.du + o.du, self.dv + o.dv);
        // Scaled coefficients C(m, i) C(p, j) a_ij, multiplied as power-like terms, then divided back.
        let scaled = |x: &Bern| -> Vec<f64> {
            let (bu, bv) = (rows.row(x.du), rows.row(x.dv));
            x.c.chunks(x.dv + 1)
                .zip(bu)
                .flat_map(|(row, &bi)| row.iter().zip(bv).map(move |(&c, &bj)| bi * bj * c))
                .collect()
        };
        let (a, b) = (scaled(self), scaled(o));
        let mut c = vec![0.0; (du + 1) * (dv + 1)];
        for (i, a_row) in a.chunks(self.dv + 1).enumerate() {
            for (j, &x) in a_row.iter().enumerate() {
                if x == 0.0 {
                    continue;
                }
                for (k, row_b) in b.chunks(o.dv + 1).enumerate() {
                    let base = (i + k) * (dv + 1) + j;
                    if let Some(out) = c.get_mut(base..base + row_b.len()) {
                        for (z, y) in out.iter_mut().zip(row_b) {
                            *z += x * y;
                        }
                    }
                }
            }
        }
        let (bu, bv) = (rows.row(du), rows.row(dv));
        for (row, &bi) in c.chunks_mut(dv + 1).zip(bu) {
            for (z, &bj) in row.iter_mut().zip(bv) {
                *z /= bi * bj;
            }
        }
        Bern { du, dv, c }
    }

    /// Σ weight × term, over terms of one degree (the zero constant for none).
    pub(crate) fn sum(terms: &[(f64, &Bern)]) -> Bern {
        let Some((_, first)) = terms.first() else {
            return Bern {
                du: 0,
                dv: 0,
                c: vec![0.0],
            };
        };
        let mut c = vec![0.0; first.c.len()];
        for (w, t) in terms {
            for (z, x) in c.iter_mut().zip(&t.c) {
                *z += w * x;
            }
        }
        Bern {
            du: first.du,
            dv: first.dv,
            c,
        }
    }

    /// The largest coefficient's magnitude, which bounds |g| on the square.
    pub fn max_abs(&self) -> f64 {
        self.c.iter().fold(0.0, |m, x| m.max(x.abs()))
    }

    /// Coefficient (i, j); NaN outside the net.
    fn get(&self, i: usize, j: usize) -> f64 {
        self.c
            .get(i * (self.dv + 1) + j)
            .copied()
            .unwrap_or(f64::NAN)
    }

    /// The rows of coefficients along t, one per i.
    fn rows(&self) -> std::slice::Chunks<'_, f64> {
        self.c.chunks(self.dv + 1)
    }

    /// The value at (s, t) in [0, 1]^2, by de Casteljau along t then along s,
    /// on the stack up to degree 63 (a field point's root takes a dozen of
    /// these, and heap vectors for them were most of an emitted point's cost).
    pub fn at(&self, s: f64, t: f64) -> f64 {
        if self.du >= STACK || self.dv >= STACK {
            let column: Vec<f64> = self
                .rows()
                .take(self.du + 1)
                .map(|row| casteljau(row, t))
                .collect();
            return casteljau(&column, s);
        }
        let mut column = [0.0; STACK];
        for (value, row) in column.iter_mut().zip(self.rows()).take(self.du + 1) {
            *value = casteljau(row, t);
        }
        casteljau(column.get(..=self.du).unwrap_or_default(), s)
    }

    /// The two halves at s = x (dir 0) or t = x (dir 1), each over its own [0, 1].
    #[must_use]
    pub fn split(&self, dir: usize, x: f64) -> (Bern, Bern) {
        let (du, dv) = (self.du, self.dv);
        let (n, m) = if dir == 0 { (du, dv) } else { (dv, du) };
        let index = |i: usize, j: usize| {
            if dir == 0 {
                i * (dv + 1) + j
            } else {
                j * (dv + 1) + i
            }
        };
        let mut left = vec![0.0; self.c.len()];
        let mut right = vec![0.0; self.c.len()];
        let mut work = vec![0.0; n + 1];
        for j in 0..=m {
            for (i, w) in work.iter_mut().enumerate() {
                *w = self.c.get(index(i, j)).copied().unwrap_or(f64::NAN);
            }
            casteljau_halves(
                &mut work,
                |a, b| (1.0 - x) * a + x * b,
                |r, w| {
                    if let Some(z) = left.get_mut(index(r, j)) {
                        *z = w;
                    }
                },
                |r, w| {
                    if let Some(z) = right.get_mut(index(r, j)) {
                        *z = w;
                    }
                },
            );
        }
        (Bern { du, dv, c: left }, Bern { du, dv, c: right })
    }

    /// The polynomial over [s0, s1] x [t0, t1], on its own [0, 1]^2.
    #[must_use]
    pub fn restrict(&self, s0: f64, s1: f64, t0: f64, t1: f64) -> Bern {
        let mut b = self.clone();
        for (dir, lo, hi) in [(0, s0, s1), (1, t0, t1)] {
            // Bounds past [0, 1] extrapolate: g's polynomial runs on past its square.
            if hi != 1.0 {
                b = b.split(dir, hi).0;
            }
            if lo != 0.0 {
                b = b.split(dir, lo / hi).1;
            }
        }
        b
    }

    /// The polynomial along the segment from (s0, t0) to (s1, t1) of its square,
    /// as a one-variable Bernstein polynomial of degree du + dv over the segment:
    /// the tensor's coefficients over the box with those corners (rows restricted
    /// along t, then columns along s, either way round), summed along the box's
    /// diagonal with weights C(du, i) C(dv, j) / C(du + dv, i + j).
    pub fn along(&self, (s0, t0): (f64, f64), (s1, t1): (f64, f64)) -> Vec<f64> {
        let (du, dv) = (self.du, self.dv);
        let mut q = self.c.clone();
        let mut work = Vec::with_capacity(du.max(dv) + 1);
        for row in q.chunks_mut(dv + 1) {
            restrict1_into(row, t0, t1, &mut work);
        }
        let mut column = vec![0.0; du + 1];
        for j in 0..=dv {
            for (x, row) in column.iter_mut().zip(q.chunks(dv + 1)) {
                *x = row.get(j).copied().unwrap_or(f64::NAN);
            }
            restrict1_into(&mut column, s0, s1, &mut work);
            for (x, row) in column.iter().zip(q.chunks_mut(dv + 1)) {
                if let Some(y) = row.get_mut(j) {
                    *y = *x;
                }
            }
        }
        let n = du + dv;
        let rows = Rows::to(n);
        let mut out = vec![0.0; n + 1];
        for (i, row) in q.chunks(dv + 1).enumerate() {
            for (j, &y) in row.iter().enumerate() {
                if let Some(o) = out.get_mut(i + j) {
                    *o += rows.binomial(du, i) * rows.binomial(dv, j) * y;
                }
            }
        }
        for (k, x) in out.iter_mut().enumerate() {
            *x /= rows.binomial(n, k);
        }
        out
    }

    /// h with g = x h, x the factor that vanishes on edge e (s, 1 - s, t or
    /// 1 - t for edges 0 to 3), for g zero on that edge: one degree lower in
    /// that direction, from x `B_k^(n-1)` = (k + 1) / n `B_(k+1)^n` and
    /// (1 - x) `B_k^(n-1)` = (n - k) / n `B_k^n`.
    #[must_use]
    pub fn divide_edge(&self, e: usize) -> Bern {
        let (du, dv) = (self.du, self.dv);
        let mut c = Vec::with_capacity(self.c.len());
        match e {
            0 | 1 => {
                for k in 0..du {
                    for j in 0..=dv {
                        c.push(if e == 0 {
                            self.get(k + 1, j) * du as f64 / (k + 1) as f64
                        } else {
                            self.get(k, j) * du as f64 / (du - k) as f64
                        });
                    }
                }
                Bern {
                    du: du.saturating_sub(1),
                    dv,
                    c,
                }
            }
            _ => {
                for i in 0..=du {
                    for k in 0..dv {
                        c.push(if e == 2 {
                            self.get(i, k + 1) * dv as f64 / (k + 1) as f64
                        } else {
                            self.get(i, k) * dv as f64 / (dv - k) as f64
                        });
                    }
                }
                Bern {
                    du,
                    dv: dv.saturating_sub(1),
                    c,
                }
            }
        }
    }

    /// The polynomial on the line s = x (dir 0, in t) or t = x (dir 1, in s).
    pub fn line(&self, dir: usize, x: f64) -> Vec<f64> {
        let (du, dv) = (self.du, self.dv);
        if dir == 0 {
            let mut column = vec![0.0; du + 1];
            (0..=dv)
                .map(|j| {
                    for (i, c) in column.iter_mut().enumerate() {
                        *c = self.get(i, j);
                    }
                    casteljau(&column, x)
                })
                .collect()
        } else {
            self.rows()
                .take(du + 1)
                .map(|row| casteljau(row, x))
                .collect()
        }
    }

    /// The derivative along s (dir 0) or t (dir 1), a polynomial one degree lower there.
    #[must_use]
    pub fn derivative(&self, dir: usize) -> Bern {
        let (du, dv) = (self.du, self.dv);
        if (dir == 0 && du == 0) || (dir == 1 && dv == 0) {
            return Bern {
                du: 0,
                dv: 0,
                c: vec![0.0],
            };
        }
        let (nu, nv) = if dir == 0 { (du - 1, dv) } else { (du, dv - 1) };
        let mut c = Vec::with_capacity((nu + 1) * (nv + 1));
        for i in 0..=nu {
            for j in 0..=nv {
                c.push(if dir == 0 {
                    du as f64 * (self.get(i + 1, j) - self.get(i, j))
                } else {
                    dv as f64 * (self.get(i, j + 1) - self.get(i, j))
                });
            }
        }
        Bern { du: nu, dv: nv, c }
    }

    /// The edge s = 0, s = 1, t = 0 or t = 1 (edges 0 to 3) as a polynomial in the other parameter.
    pub fn edge(&self, e: usize) -> Vec<f64> {
        let (du, dv) = (self.du, self.dv);
        match e {
            0 => (0..=dv).map(|j| self.get(0, j)).collect(),
            1 => (0..=dv).map(|j| self.get(du, j)).collect(),
            2 => (0..=du).map(|i| self.get(i, 0)).collect(),
            _ => (0..=du).map(|i| self.get(i, dv)).collect(),
        }
    }

    /// Whether every coefficient is above `clear`, or every one below -`clear`.
    pub fn one_sign(&self, clear: f64) -> bool {
        self.c.iter().all(|&x| x > clear) || self.c.iter().all(|&x| x < -clear)
    }
}

/// Degrees below this are evaluated on the stack.
const STACK: usize = 64;

/// A one-variable Bernstein polynomial at t.
pub fn casteljau(c: &[f64], t: f64) -> f64 {
    let n = c.len();
    let step = |a: f64, b: f64| (1.0 - t) * a + t * b;
    if n > STACK {
        let mut work = c.to_vec();
        for r in 1..n {
            casteljau_level(&mut work, n - r, step);
        }
        return work.first().copied().unwrap_or(f64::NAN);
    }
    let mut work = [0.0; STACK];
    if let Some(head) = work.get_mut(..n) {
        head.copy_from_slice(c);
    }
    for r in 1..n {
        casteljau_level(&mut work, n - r, step);
    }
    work[0]
}

/// The roots in [0, 1] of a one-variable Bernstein polynomial, each isolated by
/// subdivision until its coefficients change sign once (Descartes' rule for
/// Bernstein coefficients), then found by bisection held to that interval. A
/// polynomial that is zero to `clear` everywhere has no isolated roots: None.
pub fn roots(c: &[f64], clear: f64) -> Option<Vec<f64>> {
    if c.iter().all(|x| x.abs() <= clear) {
        return None;
    }
    let mut out = Vec::new();
    // Intervals on which the polynomial is zero to `clear` throughout: a double
    // root's rounding band, each run of them one root at its middle. Halving on
    // inside such a band keeps both halves of it alive at every level (2^52 of
    // them at the depth limit, a hang); it is stopped where the coefficients say so.
    let mut flat: Vec<(f64, f64)> = Vec::new();
    let mut stack = vec![(c.to_vec(), 0.0, 1.0, 0)];
    let mut budget = 20_000;
    while let Some((b, lo, hi, depth)) = stack.pop() {
        budget -= 1;
        if b.iter().all(|&x| x > clear) || b.iter().all(|&x| x < -clear) {
            continue;
        }
        if b.iter().all(|x| x.abs() <= clear) || depth > 52 || budget <= 0 {
            flat.push((lo, hi));
            continue;
        }
        let changes = b
            .windows(2)
            .filter(|w| matches!(w, [x, y] if (*x > 0.0) != (*y > 0.0)))
            .count();
        let positive = |x: Option<&f64>| x.is_some_and(|&x| x > 0.0);
        let ends_differ = positive(b.first()) != positive(b.last());
        if changes == 1 && ends_differ {
            // One simple root: bisection on the sign, to the last bit.
            let (mut a, mut z) = (0.0_f64, 1.0_f64);
            let fa = casteljau(&b, 0.0);
            for _ in 0..80 {
                let mid = 0.5 * (a + z);
                if (casteljau(&b, mid) > 0.0) == (fa > 0.0) {
                    a = mid;
                } else {
                    z = mid;
                }
            }
            out.push(lo + 0.5 * (a + z) * (hi - lo));
            continue;
        }
        let mid = 0.5 * (lo + hi);
        let (l, r) = split1(&b);
        stack.push((r, mid, hi, depth + 1));
        stack.push((l, lo, mid, depth + 1));
    }
    flat.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut run: Option<(f64, f64)> = None;
    for (lo, hi) in flat {
        run = match run {
            Some((a, z)) if lo <= z + 1e-12 => Some((a, z.max(hi))),
            Some((a, z)) => {
                out.push(0.5 * (a + z));
                Some((lo, hi))
            }
            None => Some((lo, hi)),
        };
    }
    if let Some((a, z)) = run {
        out.push(0.5 * (a + z));
    }
    out.sort_by(f64::total_cmp);
    out.dedup_by(|a, b| (*a - *b).abs() <= 1e-14);
    Some(out)
}

/// A one-variable Bernstein polynomial's coefficients over [a, b] of its [0, 1]
/// (b below a runs it backward): coefficient k is its blossom at a (n - k
/// times) and b (k times). In place, `work` a scratch buffer.
pub(crate) fn restrict1_into(c: &mut [f64], a: f64, b: f64, work: &mut Vec<f64>) {
    let n = c.len().saturating_sub(1);
    let source = c.to_vec();
    for (k, out) in c.iter_mut().enumerate() {
        work.clear();
        work.extend_from_slice(&source);
        for r in 0..n {
            let x = if r < k { b } else { a };
            casteljau_level(work, n - r, |p, q| (1.0 - x) * p + x * q);
        }
        *out = work.first().copied().unwrap_or(f64::NAN);
    }
}

/// A one-variable Bernstein polynomial raised to degree `to` (at least its own).
pub(crate) fn elevate1(c: &[f64], to: usize) -> Vec<f64> {
    let mut c = c.to_vec();
    if c.is_empty() {
        return c;
    }
    while c.len() - 1 < to {
        let n = c.len() - 1;
        let mut up = Vec::with_capacity(n + 2);
        up.extend(c.first());
        for (i, pair) in c.windows(2).enumerate() {
            if let &[before, here] = pair {
                let a = (i + 1) as f64 / (n + 1) as f64;
                up.push(a * before + (1.0 - a) * here);
            }
        }
        up.extend(c.last());
        c = up;
    }
    c
}

/// A one-variable Bernstein polynomial's halves at 1/2.
pub(crate) fn split1(c: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let mut work = c.to_vec();
    let mut left = vec![0.0; c.len()];
    let mut right = vec![0.0; c.len()];
    casteljau_halves(
        &mut work,
        |a, b| 0.5 * (a + b),
        |r, w| {
            if let Some(z) = left.get_mut(r) {
                *z = w;
            }
        },
        |r, w| {
            if let Some(z) = right.get_mut(r) {
                *z = w;
            }
        },
    );
    (left, right)
}

/// How a piece stands to an analytic surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    /// Every coefficient of g is zero (to 1e-9 of the terms' scale): the piece lies on the surface.
    On,
    /// Every coefficient has one sign, clear of rounding: the piece does not meet the surface.
    Apart,
    /// Neither: the piece may cross or touch the surface.
    Unknown,
}

/// The piece's homogeneous points in the frame: local x, y, z (each times W) and W, of the piece's degree.
fn local(piece: &Piece, f: &Frame) -> [Bern; 4] {
    let (y, z) = (f.y(), f.z);
    let axes = [f.x, y, z];
    let mut out = [(); 4].map(|()| Bern {
        du: piece.du,
        dv: piece.dv,
        c: Vec::with_capacity(piece.c.len()),
    });
    for h in &piece.c {
        let d = V3::new(
            h[0] - f.o.x * h[3],
            h[1] - f.o.y * h[3],
            h[2] - f.o.z * h[3],
        );
        for (o, axis) in out.iter_mut().zip(&axes) {
            o.c.push(d.dot(*axis));
        }
        out[3].c.push(h[3]);
    }
    out
}

/// The composed field g = W^k f(X / W) and the scale of its terms, or None for a B-spline surface.
pub fn compose(piece: &Piece, surf: &Surf) -> Option<(Bern, f64)> {
    let f = surf.frame()?;
    let [lx, ly, lz, w] = local(piece, f);
    // m bounds every term's size: the piece's coordinates and the surface's own lengths times W.
    let length = match surf {
        Surf::Plane { .. } | Surf::Nurbs(_) => 0.0,
        Surf::Cylinder { r, .. } | Surf::Sphere { r, .. } => *r,
        Surf::Cone { r, .. } => r.abs(),
        Surf::Torus { major, minor, .. } => major + minor,
    };
    let m = lx
        .max_abs()
        .max(ly.max_abs())
        .max(lz.max_abs())
        .max(length * w.max_abs());
    let square = |b: &Bern| b.mul(b);
    let radial = || Bern::sum(&[(1.0, &square(&lx)), (1.0, &square(&ly))]);
    let g = match surf {
        Surf::Plane { .. } => return Some((lz, m)),
        Surf::Sphere { r, .. } => {
            Bern::sum(&[(1.0, &radial()), (1.0, &square(&lz)), (-r * r, &square(&w))])
        }
        Surf::Cylinder { r, .. } => Bern::sum(&[(1.0, &radial()), (-r * r, &square(&w))]),
        Surf::Cone { r, a, .. } => {
            // STEP's cone: radius r + v tan a at height v.
            let rim = Bern::sum(&[(*r, &w), (a.tan(), &lz)]);
            Bern::sum(&[(1.0, &radial()), (-1.0, &square(&rim))])
        }
        Surf::Torus { major, minor, .. } => {
            let w2 = square(&w);
            let s = Bern::sum(&[
                (1.0, &radial()),
                (1.0, &square(&lz)),
                (major * major - minor * minor, &w2),
            ]);
            Bern::sum(&[
                (1.0, &square(&s)),
                (-4.0 * major * major, &w2.mul(&radial())),
            ])
        }
        Surf::Nurbs(_) => return None,
    };
    let k = if matches!(surf, Surf::Torus { .. }) {
        4
    } else {
        2
    };
    Some((g, m.powi(k)))
}

/// A surface with an implicit equation: analytic, or a B-spline patch's proved
/// algebraic form (`algebraic`); or a piece of a patch with neither, seen along
/// a fixed direction (`taylor`): its field is a cell's local polynomial of the
/// separation, and the foot of a point is where the line through it along the
/// direction meets the piece.
#[derive(Clone, Copy)]
pub enum Field<'f> {
    /// An analytic surface's equation.
    Analytic(&'f Surf),
    /// A B-spline patch's proved algebraic form.
    Algebraic(&'f Algebraic),
    /// A piece seen along a fixed unit direction.
    Local(&'f Piece, [f64; 3]),
}

impl std::fmt::Debug for Field<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Field::Analytic(surf) => write!(f, "Analytic({surf:?})"),
            Field::Algebraic(form) => write!(f, "Algebraic(degree {})", form.degree),
            Field::Local(piece, dir) => write!(
                f,
                "Local(patch {} [{}, {}] x [{}, {}], along {dir:?})",
                piece.patch, piece.u0, piece.u1, piece.v0, piece.v1
            ),
        }
    }
}

impl Field<'_> {
    /// g = W^k f(X / W) on the piece and the scale of its terms, as `compose`.
    pub fn compose(&self, piece: &Piece) -> Option<(Bern, f64)> {
        match self {
            Field::Analytic(surf) => compose(piece, surf),
            Field::Algebraic(form) => Some(form.compose(piece)),
            Field::Local(..) => None,
        }
    }

    /// The foot of p on the surface (to first order for an algebraic form).
    pub fn foot(&self, p: [f64; 3]) -> [f64; 3] {
        match self {
            Field::Analytic(surf) => {
                let f = surf.foot(V3::new(p[0], p[1], p[2]));
                [f.x, f.y, f.z]
            }
            Field::Algebraic(form) => form.foot(p),
            Field::Local(piece, dir) => crate::taylor::ray(piece, *dir, p, (0.5, 0.5, 0.0))
                .map_or([f64::NAN; 3], |(_, _, l)| {
                    [p[0] + l * dir[0], p[1] + l * dir[1], p[2] + l * dir[2]]
                }),
        }
    }

    /// How exact the implicit equation is, relative to the scale of its
    /// terms: an analytic surface's is exact, an algebraic form's has the
    /// residual its proof met.
    pub fn residual(&self) -> f64 {
        match self {
            Field::Analytic(_) | Field::Local(..) => 0.0,
            Field::Algebraic(form) => form.residual,
        }
    }

    /// The implicit equation's degree: plane 1, quadrics 2, torus 4.
    pub fn degree(&self) -> usize {
        match self {
            Field::Analytic(Surf::Plane { .. }) => 1,
            Field::Analytic(Surf::Torus { .. }) => 4,
            Field::Analytic(_) => 2,
            Field::Algebraic(form) => form.degree,
            Field::Local(..) => 0,
        }
    }
}

/// How a piece stands to an analytic surface: on it, apart from it, or unknown.
pub fn relate(piece: &Piece, surf: &Surf) -> Relation {
    relate_field(piece, &Field::Analytic(surf))
}

/// `relate` for any field.
pub fn relate_field(piece: &Piece, field: &Field) -> Relation {
    let Some((g, scale)) = field.compose(piece) else {
        return Relation::Unknown;
    };
    if !(scale > 0.0) {
        return Relation::Unknown;
    }
    if g.max_abs() <= 1e-9 * scale {
        return Relation::On;
    }
    let clear = 1e-12 * scale;
    if g.c.iter().all(|&x| x > clear) || g.c.iter().all(|&x| x < -clear) {
        return Relation::Apart;
    }
    Relation::Unknown
}
