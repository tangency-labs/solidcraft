//! Local polynomial fields of a free-form pair: the separation of patch A from
//! patch B along a fixed direction, as a Taylor polynomial about a point of A.
//!
//! For a pair with no implicit form, the field path's g is unavailable; the
//! separation λ(s, t), with A(s, t) + λ n = B(u, v) for a direction n across
//! both surfaces, is zero exactly on the intersection and critical there where
//! the surfaces are tangent, like g. About a point of A it is a power series in
//! (s, t), found by Newton's method on truncated power series (each step gains
//! an order). It is one global function; the polynomial is only its local
//! enclosure, for proofs: every point placed is solved on the surfaces
//! themselves, so neighboring cells' polynomials never disagree where it
//! matters (`ImplicitCAM`'s lesson: per-cell formulas, read one per point, made
//! its field jump where neighbors met, `docs/FREP_GLUING.md` there).

use crate::field::{casteljau_level, Bern};
use crate::piece::{cross, dot, norm, scale, sub, unit, Piece, V3};

/// A power series in (x, y) truncated at total order n: the coefficient of
/// x^(d - j) y^j at d (d + 1) / 2 + j.
#[derive(Clone, Debug)]
pub(crate) struct Series {
    n: usize,
    c: Vec<f64>,
}

fn at(d: usize, j: usize) -> usize {
    d * (d + 1) / 2 + j
}

impl Series {
    pub fn constant(n: usize, value: f64) -> Series {
        let mut c = vec![0.0; at(n + 1, 0)];
        if let Some(first) = c.first_mut() {
            *first = value;
        }
        Series { n, c }
    }

    /// value + x (k = 0) or value + y (k = 1).
    pub fn variable(n: usize, value: f64, k: usize) -> Series {
        let mut s = Series::constant(n, value);
        if n >= 1 {
            if let Some(c) = s.c.get_mut(at(1, k)) {
                *c = 1.0;
            }
        }
        s
    }

    pub fn value(&self) -> f64 {
        self.coef(0, 0)
    }

    /// The coefficient of x^(d - j) y^j; NaN past the order.
    fn coef(&self, d: usize, j: usize) -> f64 {
        self.c.get(at(d, j)).copied().unwrap_or(f64::NAN)
    }

    pub fn add(&self, other: &Series) -> Series {
        let c = self.c.iter().zip(&other.c).map(|(a, b)| a + b).collect();
        Series { n: self.n, c }
    }

    pub fn sub(&self, other: &Series) -> Series {
        let c = self.c.iter().zip(&other.c).map(|(a, b)| a - b).collect();
        Series { n: self.n, c }
    }

    pub fn scaled(&self, k: f64) -> Series {
        Series {
            n: self.n,
            c: self.c.iter().map(|a| a * k).collect(),
        }
    }

    pub fn mul(&self, other: &Series) -> Series {
        let n = self.n;
        let mut c = vec![0.0; self.c.len()];
        for d1 in 0..=n {
            for j1 in 0..=d1 {
                let a = self.coef(d1, j1);
                if a == 0.0 {
                    continue;
                }
                for d2 in 0..=n - d1 {
                    for j2 in 0..=d2 {
                        if let Some(z) = c.get_mut(at(d1 + d2, j1 + j2)) {
                            *z += a * other.coef(d2, j2);
                        }
                    }
                }
            }
        }
        Series { n, c }
    }

    /// 1 / self, by Newton's step r = r (2 - self r), each doubling the orders.
    pub fn recip(&self) -> Series {
        let mut r = Series::constant(self.n, 1.0 / self.value());
        let two = Series::constant(self.n, 2.0);
        let mut order = 1;
        while order <= self.n {
            r = r.mul(&two.sub(&self.mul(&r)));
            order *= 2;
        }
        r.mul(&two.sub(&self.mul(&r)))
    }

    #[cfg(test)]
    pub fn eval(&self, x: f64, y: f64) -> f64 {
        let mut total = 0.0;
        for d in 0..=self.n {
            for j in 0..=d {
                total += self.coef(d, j) * x.powi((d - j) as i32) * y.powi(j as i32);
            }
        }
        total
    }

    /// The largest the terms of total order d can be over |x| <= a, |y| <= b.
    #[cfg(test)]
    pub fn order_bound(&self, d: usize, a: f64, b: f64) -> f64 {
        (0..=d)
            .map(|j| self.coef(d, j).abs() * a.powi((d - j) as i32) * b.powi(j as i32))
            .sum()
    }

    /// The polynomial in Bernstein form of degree (n, n) on the cell
    /// x in [x0, x1], y in [y0, y1], as cell-local [0, 1]^2.
    pub fn bernstein(&self, x0: f64, x1: f64, y0: f64, y1: f64) -> Bern {
        let n = self.n;
        // Monomials in the cell-local (p, q): x = x0 + (x1 - x0) p, y = y0 + (y1 - y0) q.
        let binom = |n: usize, k: usize| -> f64 {
            (0..k).fold(1.0, |c, i| c * (n - i) as f64 / (i + 1) as f64)
        };
        let line = |start: f64, width: f64, e: usize| -> Vec<f64> {
            // (start + width p)^e in powers of p.
            (0..=e)
                .map(|k| binom(e, k) * start.powi((e - k) as i32) * width.powi(k as i32))
                .collect()
        };
        let mut m = vec![vec![0.0; n + 1]; n + 1];
        for d in 0..=n {
            for j in 0..=d {
                let c = self.coef(d, j);
                if c == 0.0 {
                    continue;
                }
                let (px, py) = (line(x0, x1 - x0, d - j), line(y0, y1 - y0, j));
                for (m_row, a) in m.iter_mut().zip(&px) {
                    for (z, b) in m_row.iter_mut().zip(&py) {
                        *z += c * a * b;
                    }
                }
            }
        }
        // Power basis to Bernstein, in each direction: b_i = sum over k <= i of C(i, k) / C(n, k) a_k.
        let convert = |a: &[f64]| -> Vec<f64> {
            (0..=n)
                .map(|i| {
                    a.iter()
                        .take(i + 1)
                        .enumerate()
                        .map(|(k, ak)| binom(i, k) / binom(n, k) * ak)
                        .sum()
                })
                .collect()
        };
        let rows: Vec<Vec<f64>> = m.iter().map(|r| convert(r)).collect();
        let mut c = vec![0.0; (n + 1) * (n + 1)];
        for j in 0..=n {
            let column: Vec<f64> = rows
                .iter()
                .map(|r| r.get(j).copied().unwrap_or(f64::NAN))
                .collect();
            for (row, b) in c.chunks_mut(n + 1).zip(convert(&column)) {
                if let Some(z) = row.get_mut(j) {
                    *z = b;
                }
            }
        }
        Bern { du: n, dv: n, c }
    }
}

/// A piece's homogeneous point at series parameters (s, t), and its Cartesian point.
pub(crate) fn piece_point(piece: &Piece, s: &Series, t: &Series) -> [Series; 3] {
    let n = s.n;
    let basis = |degree: usize, x: &Series| -> Vec<Series> {
        let one = Series::constant(n, 1.0);
        let rest = one.sub(x);
        let mut px = vec![one.clone()];
        let mut pr = vec![one];
        for _ in 1..=degree {
            let (Some(last_x), Some(last_r)) = (px.last(), pr.last()) else {
                break;
            };
            let (next_x, next_r) = (last_x.mul(x), last_r.mul(&rest));
            px.push(next_x);
            pr.push(next_r);
        }
        px.iter()
            .zip(pr.iter().rev())
            .enumerate()
            .map(|(i, (a, b))| {
                let c = (0..i).fold(1.0, |c, k| c * (degree - k) as f64 / (k + 1) as f64);
                a.mul(b).scaled(c)
            })
            .collect()
    };
    let (bu, bv) = (basis(piece.du, s), basis(piece.dv, t));
    let mut h: [Series; 4] = std::array::from_fn(|_| Series::constant(n, 0.0));
    for (row, a) in piece.c.chunks(piece.dv + 1).zip(&bu) {
        for (c, b) in row.iter().zip(&bv) {
            let w = a.mul(b);
            for (hk, &ck) in h.iter_mut().zip(c) {
                *hk = hk.add(&w.scaled(ck));
            }
        }
    }
    let r = h[3].recip();
    [h[0].mul(&r), h[1].mul(&r), h[2].mul(&r)]
}

/// Solves J x = r for a 3 x 3 J given by its columns.
fn solve3(cols: [V3; 3], r: V3) -> Option<V3> {
    let det = dot(cols[0], cross(cols[1], cols[2]));
    if !(det.abs() > 0.0) {
        return None;
    }
    Some([
        dot(r, cross(cols[1], cols[2])) / det,
        dot(cols[0], cross(r, cols[2])) / det,
        dot(cols[0], cross(cols[1], r)) / det,
    ])
}

/// The separation λ of A from B along n at piece-local (s, t) of A, with B's
/// piece-local (u, v): A(s, t) + λ n = B(u, v), by Newton from (u, v, λ).
pub(crate) fn separation(
    a: &Piece,
    b: &Piece,
    n: V3,
    (s, t): (f64, f64),
    start: (f64, f64, f64),
) -> Option<(f64, f64, f64)> {
    ray(b, n, a.evaluate(s, t).point, start)
}

/// Where the line p + λ n meets the piece b: its piece-local (u, v) and λ, by
/// Newton from `start`; None when that fails or does not converge.
pub(crate) fn ray(b: &Piece, n: V3, p: V3, mut start: (f64, f64, f64)) -> Option<(f64, f64, f64)> {
    for _ in 0..50 {
        let (u, v, l) = start;
        let e = b.evaluate(u, v);
        let r = sub(e.point, [p[0] + l * n[0], p[1] + l * n[1], p[2] + l * n[2]]);
        // F = B(u, v) - A - λ n, J = [B_u, B_v, -n]: the step -J^-1 F solves
        // [-B_u, -B_v, n] d = F.
        let d = solve3([scale(e.ds, -1.0), scale(e.dt, -1.0), n], r)?;
        start = (u + d[0], v + d[1], l + d[2]);
        if !(start.0.is_finite() && start.1.is_finite() && start.2.is_finite()) {
            return None;
        }
        if norm(d) <= 1e-15 * (1.0 + start.2.abs()) {
            return Some(start);
        }
    }
    None
}

/// B's (u, v) and λ about A's piece-local (s0, t0) as series of order n in
/// (s - s0, t - t0), their values there given: chord Newton on series, with
/// the Jacobian at the center, one order a step.
pub(crate) fn separation_series(
    a: &Piece,
    b: &Piece,
    dir: V3,
    (s0, t0): (f64, f64),
    (u0, v0, l0): (f64, f64, f64),
    n: usize,
) -> Option<[Series; 3]> {
    let pa = piece_point(a, &Series::variable(n, s0, 0), &Series::variable(n, t0, 1));
    let eb = b.evaluate(u0, v0);
    let cols = [scale(eb.ds, -1.0), scale(eb.dt, -1.0), dir];
    let (mut u, mut v, mut l) = (
        Series::constant(n, u0),
        Series::constant(n, v0),
        Series::constant(n, l0),
    );
    for _ in 0..n + 3 {
        let pb = piece_point(b, &u, &v);
        // F = B(u, v) - A - λ n; J = [B_u, B_v, -n] = -cols; step = -J^-1 F = cols^-1 F.
        let f: [Series; 3] = [
            pb[0].sub(&pa[0]).sub(&l.scaled(dir[0])),
            pb[1].sub(&pa[1]).sub(&l.scaled(dir[1])),
            pb[2].sub(&pa[2]).sub(&l.scaled(dir[2])),
        ];
        let mut step: [Series; 3] = std::array::from_fn(|_| Series::constant(n, 0.0));
        let [s0, s1, s2] = &mut step;
        let coefficients = f[0].c.iter().zip(&f[1].c).zip(&f[2].c);
        let steps = s0.c.iter_mut().zip(s1.c.iter_mut().zip(s2.c.iter_mut()));
        for (((&x, &y), &z), (d0, (d1, d2))) in coefficients.zip(steps) {
            let d = solve3(cols, [x, y, z])?;
            (*d0, *d1, *d2) = (d[0], d[1], d[2]);
        }
        u = u.add(&step[0]);
        v = v.add(&step[1]);
        l = l.add(&step[2]);
    }
    Some([u, v, l])
}

/// A local field on a cell of A's piece: λ's polynomial in Bernstein form on
/// the cell (cell-local [0, 1]^2), and proved bounds on how far λ itself, its
/// first derivatives and its second derivatives (per unit of A's piece-local
/// parameters) are from the polynomial's anywhere on the cell.
pub(crate) struct Local {
    pub g: Bern,
    pub band: f64,
    pub gradient: f64,
    pub hessian: f64,
}

/// The polynomial differentiated i times along s and j along t.
fn partial(b: &Bern, i: usize, j: usize) -> Bern {
    let mut d = b.clone();
    for _ in 0..i {
        d = d.derivative(0);
    }
    for _ in 0..j {
        d = d.derivative(1);
    }
    d
}

fn choose(n: usize, k: usize) -> f64 {
    (0..k).fold(1.0, |c, i| c * (n - i) as f64 / (i + 1) as f64)
}

/// Bounds over the square, for i + j <= order, of `|d^(i+j) (h_k / w) / ds^i dt^j|`,
/// the largest over k, per unit of parameters the square spans (widths)
/// of: from w (h / w)^(α) = h^(α) - Σ over 0 < β <= α of C(α, β) w^(β) (h / w)^(α-β)
/// (Leibniz), each term bounded by its polynomials' coefficients and w from below.
fn quotient_bounds(
    h: &[Bern],
    w: &Bern,
    (ws, wt): (f64, f64),
    order: usize,
) -> Option<Vec<Vec<f64>>> {
    let floor = least(w);
    if !(floor > 0.0) {
        return None;
    }
    let scale_of = |i: usize, j: usize| ws.powi(i as i32) * wt.powi(j as i32);
    let mut hmax = vec![vec![0.0; order + 1]; order + 1];
    let mut wmax = vec![vec![0.0; order + 1]; order + 1];
    for i in 0..=order {
        for j in 0..=order - i {
            *entry_mut(&mut hmax, i, j)? = h
                .iter()
                .map(|b| partial(b, i, j).max_abs())
                .fold(0.0, f64::max)
                / scale_of(i, j);
            *entry_mut(&mut wmax, i, j)? = partial(w, i, j).max_abs() / scale_of(i, j);
        }
    }
    let mut q = vec![vec![0.0; order + 1]; order + 1];
    for total in 0..=order {
        for i in 0..=total {
            let j = total - i;
            let mut sum = entry(&hmax, i, j)?;
            for a in 0..=i {
                for b in 0..=j {
                    if a + b > 0 {
                        sum += choose(i, a)
                            * choose(j, b)
                            * entry(&wmax, a, b)?
                            * entry(&q, i - a, j - b)?;
                    }
                }
            }
            *entry_mut(&mut q, i, j)? = sum / floor;
        }
    }
    Some(q)
}

/// Entry (i, j) of a table of rows.
fn entry(table: &[Vec<f64>], i: usize, j: usize) -> Option<f64> {
    table.get(i)?.get(j).copied()
}

/// Entry (i, j) of a table of rows, to write.
fn entry_mut(table: &mut [Vec<f64>], i: usize, j: usize) -> Option<&mut f64> {
    table.get_mut(i)?.get_mut(j)
}

/// A piece's homogeneous coordinate k as a Bernstein polynomial on its square.
fn coordinate(piece: &Piece, k: usize) -> Bern {
    Bern {
        du: piece.du,
        dv: piece.dv,
        c: piece
            .c
            .iter()
            .map(|h| h.get(k).copied().unwrap_or(f64::NAN))
            .collect(),
    }
}

/// The polynomial over [s0, s1] x [t0, t1] on its own [0, 1]^2, by blossoms:
/// coefficient i is de Casteljau's at s0 taken n - i times and at s1 i times.
/// `Bern::restrict` gets there by two splits, the second at s0 / s1, which
/// for a box past [0, 1] extrapolates by far more (the reach of a pair whose
/// points fall beside B's piece, [-0.58, 0.023]: a split at -25, rounding
/// times 25^8, B's derivatives bounded 10^5 too high).
fn window(b: &Bern, s0: f64, s1: f64, t0: f64, t1: f64) -> Bern {
    let line = |c: &[f64], lo: f64, hi: f64| -> Vec<f64> {
        let n = c.len().saturating_sub(1);
        (0..=n)
            .map(|i| {
                let mut w = c.to_vec();
                for level in 0..n {
                    let x = if level < n - i { lo } else { hi };
                    casteljau_level(&mut w, n - level, |p, q| (1.0 - x) * p + x * q);
                }
                w.first().copied().unwrap_or(f64::NAN)
            })
            .collect()
    };
    let (du, dv) = (b.du, b.dv);
    // Along t in each row i, then along s in each column j.
    let mut rows = vec![0.0; b.c.len()];
    for (out, row) in rows.chunks_mut(dv + 1).zip(b.c.chunks(dv + 1)).take(du + 1) {
        for (z, x) in out.iter_mut().zip(line(row, t0, t1)) {
            *z = x;
        }
    }
    let mut c = vec![0.0; b.c.len()];
    for j in 0..=dv {
        let column: Vec<f64> = rows
            .chunks(dv + 1)
            .take(du + 1)
            .map(|row| row.get(j).copied().unwrap_or(f64::NAN))
            .collect();
        for (row, x) in c.chunks_mut(dv + 1).zip(line(&column, s0, s1)) {
            if let Some(z) = row.get_mut(j) {
                *z = x;
            }
        }
    }
    Bern { du, dv, c }
}

/// b raised to degree (du, dv): times the polynomial 1 of the difference.
fn raised(b: &Bern, du: usize, dv: usize) -> Bern {
    if (b.du, b.dv) == (du, dv) {
        return b.clone();
    }
    let (eu, ev) = (du.saturating_sub(b.du), dv.saturating_sub(b.dv));
    b.mul(&Bern {
        du: eu,
        dv: ev,
        c: vec![1.0; (eu + 1) * (ev + 1)],
    })
}

/// The smallest coefficient: a lower bound of the polynomial (convex hull).
fn least(b: &Bern) -> f64 {
    b.c.iter().fold(f64::INFINITY, |m, &x| m.min(x))
}

/// The piece's homogeneous coordinates composed with (u, v), Bernstein
/// polynomials on a cell: `B_i(u) B_j(v)` built from powers of u and 1 - u.
fn compose_piece(piece: &Piece, u: &Bern, v: &Bern) -> [Bern; 4] {
    let basis = |degree: usize, x: &Bern| -> Vec<Bern> {
        let one = Bern {
            du: 0,
            dv: 0,
            c: vec![1.0],
        };
        let rest = Bern {
            du: x.du,
            dv: x.dv,
            c: x.c.iter().map(|c| 1.0 - c).collect(),
        };
        let mut px = vec![one.clone()];
        let mut pr = vec![one];
        for _ in 1..=degree {
            let (Some(last_x), Some(last_r)) = (px.last(), pr.last()) else {
                break;
            };
            let (next_x, next_r) = (last_x.mul(x), last_r.mul(&rest));
            px.push(next_x);
            pr.push(next_r);
        }
        px.iter()
            .zip(pr.iter().rev())
            .enumerate()
            .map(|(i, (a, r))| {
                let c = (0..i).fold(1.0, |c, k| c * (degree - k) as f64 / (k + 1) as f64);
                let b = a.mul(r);
                Bern {
                    du: b.du,
                    dv: b.dv,
                    c: b.c.iter().map(|x| x * c).collect(),
                }
            })
            .collect()
    };
    let (bu, bv) = (basis(piece.du, u), basis(piece.dv, v));
    let mut out: [Option<Bern>; 4] = Default::default();
    for (row, a) in piece.c.chunks(piece.dv + 1).zip(&bu) {
        for (h, b) in row.iter().zip(&bv) {
            let w = a.mul(b);
            for (slot, &hk) in out.iter_mut().zip(h) {
                let term: Vec<f64> = w.c.iter().map(|x| x * hk).collect();
                *slot = Some(match slot.take() {
                    None => Bern {
                        du: w.du,
                        dv: w.dv,
                        c: term,
                    },
                    Some(acc) => Bern {
                        du: acc.du,
                        dv: acc.dv,
                        c: acc.c.iter().zip(&term).map(|(x, y)| x + y).collect(),
                    },
                });
            }
        }
    }
    out.map(|b| {
        b.unwrap_or(Bern {
            du: 0,
            dv: 0,
            c: vec![0.0],
        })
    })
}

/// The largest `|B_x - at|` over B's piece-local box [u0, u1] x [v0, v1], for
/// x = u (dir 0) or v (dir 1), each coordinate: `B_x = (H_x W - H W_x) / W^2`,
/// its numerator less at W^2 bounded by its coefficients, W^2 from below.
fn derivative_spread(
    piece: &Piece,
    (u0, u1, v0, v1): (f64, f64, f64, f64),
    dir: usize,
    at: V3,
) -> Option<V3> {
    let h: [Bern; 4] = std::array::from_fn(|k| window(&coordinate(piece, k), u0, u1, v0, v1));
    let width = if dir == 0 { u1 - u0 } else { v1 - v0 };
    let w = &h[3];
    let wx = w.derivative(dir);
    let w2 = w.mul(w);
    let floor = least(&w2);
    if !(floor > 0.0) {
        return None;
    }
    let mut out = [0.0; 3];
    for ((o, hk), &atk) in out.iter_mut().zip(&h).zip(&at) {
        let hx = hk.derivative(dir);
        // Per unit of the box's own parameter, so divided by its width.
        let num = Bern::sum(&[(1.0 / width, &hx.mul(w)), (-1.0 / width, &hk.mul(&wx))]);
        let num = raised(&num, w2.du, w2.dv);
        let off = Bern::sum(&[(1.0, &num), (-atk, &w2)]);
        *o = off.max_abs() / floor;
    }
    Some(out)
}

/// λ on the cell [s0, s1] x [t0, t1] of A's piece, as a polynomial of the given
/// order about the cell's center (B's (u, v) and λ there given), with a proved
/// bound on how far λ is from it on the cell. At each (s, t), (u, v, λ) is the
/// root of Φ(w) = B(u, v) - λ n - A(s, t), and the polynomials (U, V, L) are an
/// approximate root with residual G = Φ(U, V, L). With Y the inverse of Φ's
/// Jacobian at the center and κ a bound of |Y (J(w) - J(center))| over every
/// w the cell's points can reach, the map w - Y Φ(w) contracts by κ there, and
/// the root lies within |Y G| / (1 - κ) of (U, V, L) (the contraction mapping
/// theorem). G is bounded by the coefficients of its numerator, B(U, V)'s
/// composed in Bernstein form, over its denominator's least coefficient; κ by
/// the spread of B's derivatives over the box of (u, v) reached. Rounding is
/// allowed for at a few ulps of each term's size per degree.
pub(crate) fn local_field(
    a: &Piece,
    b: &Piece,
    dir: V3,
    (s0, s1, t0, t1): (f64, f64, f64, f64),
    start: (f64, f64, f64),
    order: usize,
) -> Option<Local> {
    let center = (0.5 * (s0 + s1), 0.5 * (t0 + t1));
    let (ha, hb) = (0.5 * (s1 - s0), 0.5 * (t1 - t0));
    let [su, sv, sl] = separation_series(a, b, dir, center, start, order)?;
    let (u, v, l) = (
        su.bernstein(-ha, ha, -hb, hb),
        sv.bernstein(-ha, ha, -hb, hb),
        sl.bernstein(-ha, ha, -hb, hb),
    );
    // The residual's numerator, over W_B(U, V) W_A.
    let hb_uv = compose_piece(b, &u, &v);
    let ha_cell: [Bern; 4] = std::array::from_fn(|k| window(&coordinate(a, k), s0, s1, t0, t1));
    let (wb, wa) = (&hb_uv[3], &ha_cell[3]);
    let den = wb.mul(wa);
    let floor = least(&den);
    if !(floor > 0.0) {
        return None;
    }
    let lwb = l.mul(wb).mul(wa);
    let (du, dv) = (lwb.du, lwb.dv);
    let mut residual = [0.0; 3];
    let mut nums = Vec::with_capacity(3);
    let mut roundings = [0.0; 3];
    let coordinates = hb_uv.iter().zip(&ha_cell).zip(&dir);
    for ((r, rounding), ((hbk, hak), &dk)) in
        residual.iter_mut().zip(&mut roundings).zip(coordinates)
    {
        let first = raised(&hbk.mul(wa), du, dv);
        let second = raised(&hak.mul(wb), du, dv);
        let num = Bern::sum(&[(1.0, &first), (-1.0, &second), (-dk, &lwb)]);
        let size = first.max_abs() + second.max_abs() + dk.abs() * lwb.max_abs();
        *rounding = 4.0 * f64::EPSILON * (du + dv) as f64 * size;
        *r = (num.max_abs() + *rounding) / floor;
        nums.push(num);
    }
    // Y: the inverse of [B_u, B_v, -n] at the center, by its columns' cross products.
    let e = b.evaluate(start.0, start.1);
    let cols = [e.ds, e.dt, scale(dir, -1.0)];
    let det = dot(cols[0], cross(cols[1], cols[2]));
    if !(det.abs() > 0.0) {
        return None;
    }
    let y = [
        scale(cross(cols[1], cols[2]), 1.0 / det),
        scale(cross(cols[2], cols[0]), 1.0 / det),
        scale(cross(cols[0], cols[1]), 1.0 / det),
    ];
    let yg: [f64; 3] = y.map(|row| {
        row.iter()
            .zip(&residual)
            .map(|(yk, rk)| yk.abs() * rk)
            .sum()
    });
    let push = yg.iter().fold(0.0_f64, |m, &x| m.max(x));
    // The (u, v) the cell reaches, widened by the radius being proved.
    let span = |b: &Bern| {
        (
            least(b),
            b.c.iter().fold(f64::NEG_INFINITY, |m, &x| m.max(x)),
        )
    };
    let ((ul, uh), (vl, vh)) = (span(&u), span(&v));
    let mut radius = 2.0 * push + 1e-300;
    for _ in 0..8 {
        let reach = (ul - radius, uh + radius, vl - radius, vh + radius);
        let spread = [
            derivative_spread(b, reach, 0, e.ds)?,
            derivative_spread(b, reach, 1, e.dt)?,
        ];
        // κ_i = Σ over the two moving columns j and coordinates k of |Y_ik| d_kj.
        let kappa: [f64; 3] = y.map(|row| {
            spread
                .iter()
                .map(|d| row.iter().zip(d).map(|(yk, dk)| yk.abs() * dk).sum::<f64>())
                .sum()
        });
        let worst = kappa.iter().fold(0.0_f64, |m, &x| m.max(x));
        if !(worst < 1.0) {
            return None;
        }
        let proved = push / (1.0 - worst);
        if proved <= radius {
            // λ's own bound: its row of the contraction, at the radius proved.
            let band = yg[2] + kappa[2] * proved;
            let rounding = 4.0 * f64::EPSILON * (2 * order) as f64 * l.max_abs();
            let (gradient, hessian) = derivative_bands(
                b,
                reach,
                (nums.as_slice(), &den, roundings, floor),
                [&u, &v, &l],
                (s1 - s0, t1 - t0),
                (&y, worst, proved),
            )?;
            return Some(Local {
                g: l,
                band: band + rounding,
                gradient,
                hessian,
            });
        }
        radius = 2.0 * proved;
    }
    None
}

/// Bounds over the square, for i + j <= order, of `|d^(i+j) (h_k / w) / ds^i dt^j|`,
/// the largest over k, per unit of parameters the square spans (widths), each
/// from its exact numerator: `(N / w^p)_x = (N_x w - p N w_x) / w^(p+1)`, formed in
/// Bernstein form so that h's and w's variations cancel where they do (a
/// rational patch's weights varying along u made a Leibniz bound of B's second
/// derivatives 10^5 times too large on a piece 1e-3 across, ssi-corpus
/// `y23_fig1a_two_loops_cross`), and bounded by its coefficients over w's least
/// to the power.
fn exact_quotient_bounds(
    h: &[Bern],
    w: &Bern,
    (ws, wt): (f64, f64),
    order: usize,
) -> Option<Vec<Vec<f64>>> {
    let floor = least(w);
    if !(floor > 0.0) {
        return None;
    }
    let (wd_s, wd_t) = (w.derivative(0), w.derivative(1));
    // Numerators by multi-index, each over w^(i + j + 1).
    let mut nums: Vec<Vec<Option<Vec<Bern>>>> = vec![vec![None; order + 1]; order + 1];
    *nums.first_mut()?.first_mut()? = Some(h.to_vec());
    let mut q = vec![vec![0.0; order + 1]; order + 1];
    for total in 0..=order {
        for i in 0..=total {
            let j = total - i;
            if total > 0 {
                // From the parent one derivative lower, along s where i > 0.
                let (pi, pj, dir) = if i > 0 { (i - 1, j, 0) } else { (i, j - 1, 1) };
                let power = (pi + pj + 1) as f64;
                let wd = if dir == 0 { &wd_s } else { &wd_t };
                let next: Vec<Bern> = numerators(&nums, pi, pj)?
                    .iter()
                    .map(|n| Bern::sum(&[(1.0, &n.derivative(dir).mul(w)), (-power, &n.mul(wd))]))
                    .collect();
                *nums.get_mut(i)?.get_mut(j)? = Some(next);
            }
            let largest = numerators(&nums, i, j)?
                .iter()
                .map(Bern::max_abs)
                .fold(0.0, f64::max);
            *entry_mut(&mut q, i, j)? =
                largest / floor.powi((total + 1) as i32) / (ws.powi(i as i32) * wt.powi(j as i32));
        }
    }
    Some(q)
}

/// The numerators of multi-index (i, j), when made.
fn numerators(nums: &[Vec<Option<Vec<Bern>>>], i: usize, j: usize) -> Option<&[Bern]> {
    nums.get(i)?.get(j)?.as_deref()
}

/// Bounds on the errors of the polynomials' first and second derivatives. The
/// root w = (u, v, λ) has `J(w) w_x = A_x` for x = s, t, and the polynomials w~
/// have `J(w~) w~_x = A_x + G_x`, so `J(w) (w_x - w~_x) = -G_x + (J(w~) - J(w)) w~_x`:
/// with `|J(w)^-1| <= |Y| / (1 - κ)`, `|J(w~) - J(w)| <= Lip r` (B's second
/// derivatives over the box reached) and `|w~_x| <= P1`,
/// `|w_x - w~_x| <= δ1 = |Y| / (1 - κ) (|G_x| + Lip r P1)`. Once more, with
/// `Q(w, a, b) = (B_uu a_u + B_uv a_v) b_u + (B_uv a_u + B_vv a_v) b_v`, the sum
/// `J(w) w_xy` plus `Q(w, w_x, w_y)` is `A_xy`, and the error of `w_xy` is at most
/// `|Y| / (1 - κ) (|G_xy| + Lip r P2 + 8 M3 r P1^2 + 4 M2 δ1 (2 P1 + δ1))`,
/// with M2 and M3 bounding B's second and third derivatives there.
fn derivative_bands(
    b: &Piece,
    (u0, u1, v0, v1): (f64, f64, f64, f64),
    (nums, den, roundings, floor): (&[Bern], &Bern, [f64; 3], f64),
    approx: [&Bern; 3],
    widths: (f64, f64),
    (y, kappa, radius): (&[V3; 3], f64, f64),
) -> Option<(f64, f64)> {
    let ynorm = y
        .iter()
        .map(|row| row.iter().map(|x| x.abs()).sum::<f64>())
        .fold(0.0, f64::max);
    let inverse = ynorm / (1.0 - kappa);
    // The residual's derivatives, its numerators' rounding carried as a constant
    // term (it adds nothing to derivatives but is kept on the value).
    let g = quotient_bounds(nums, den, widths, 2)?;
    let rounding = roundings.iter().fold(0.0_f64, |m, &x| m.max(x)) / floor;
    let (g1, g2) = (
        entry(&g, 1, 0)?.max(entry(&g, 0, 1)?) + rounding,
        entry(&g, 2, 0)?.max(entry(&g, 1, 1)?).max(entry(&g, 0, 2)?) + rounding,
    );
    // B's derivatives over the box of (u, v) reached.
    let hb: Vec<Bern> = (0..3)
        .map(|k| window(&coordinate(b, k), u0, u1, v0, v1))
        .collect();
    let wb = window(&coordinate(b, 3), u0, u1, v0, v1);
    let m = exact_quotient_bounds(&hb, &wb, (u1 - u0, v1 - v0), 3)?;
    let m2 = entry(&m, 2, 0)?.max(entry(&m, 1, 1)?).max(entry(&m, 0, 2)?);
    let m3 = entry(&m, 3, 0)?
        .max(entry(&m, 2, 1)?)
        .max(entry(&m, 1, 2)?)
        .max(entry(&m, 0, 3)?);
    let lip = entry(&m, 2, 0)? + 2.0 * entry(&m, 1, 1)? + entry(&m, 0, 2)?;
    // The polynomials' own derivatives on the cell.
    let (ws, wt) = widths;
    let pmax = |i: usize, j: usize| {
        approx
            .iter()
            .map(|p| partial(p, i, j).max_abs())
            .fold(0.0, f64::max)
            / (ws.powi(i as i32) * wt.powi(j as i32))
    };
    let p1 = pmax(1, 0).max(pmax(0, 1));
    let p2 = pmax(2, 0).max(pmax(1, 1)).max(pmax(0, 2));
    let d1 = inverse * (g1 + lip * radius * p1);
    let d2 = inverse
        * (g2 + lip * radius * p2 + 8.0 * m3 * radius * p1 * p1 + 4.0 * m2 * d1 * (2.0 * p1 + d1));
    Some((d1, d2))
}

/// The point of A's piece nearest (s, t) on the intersection, by Newton on the
/// separation λ itself (not a polynomial of it) along its gradient, which the
/// root's derivatives give: `J w_x = A_x` with `J = [B_u, B_v, -n]`. A point the
/// steps would carry further than `POLISH_REACH` is left as it is: beside a
/// crossing, where λ's gradient vanishes, they hop between branches, and the
/// curve sampled through them was cut into 1e-7 steps (ssi-corpus
/// `y23_fig1a_two_loops_cross`).
pub(crate) fn polish(a: &Piece, b: &Piece, dir: V3, at: (f64, f64)) -> (f64, f64) {
    let (mut s, mut t) = at;
    let mut start = (0.5, 0.5, 0.0);
    for _ in 0..8 {
        let Some(w) = separation(a, b, dir, (s, t), start) else {
            break;
        };
        start = w;
        let (ea, eb) = (a.evaluate(s, t), b.evaluate(w.0, w.1));
        let cols = [eb.ds, eb.dt, scale(dir, -1.0)];
        let (Some(xs), Some(xt)) = (solve3(cols, ea.ds), solve3(cols, ea.dt)) else {
            break;
        };
        let (gs, gt) = (xs[2], xt[2]);
        let g2 = gs * gs + gt * gt;
        if !(g2 > 0.0) || w.2 == 0.0 {
            break;
        }
        let (ds, dt) = (w.2 * gs / g2, w.2 * gt / g2);
        s -= ds;
        t -= dt;
        if (s - at.0).abs().max((t - at.1).abs()) > POLISH_REACH {
            return at;
        }
        if ds.abs().max(dt.abs()) <= 4.0 * f64::EPSILON {
            break;
        }
    }
    (s, t)
}

/// How far, in a piece's parameters, `polish` may move a point.
const POLISH_REACH: f64 = 1e-4;

/// The unit normal of a piece at (s, t).
pub(crate) fn normal(piece: &Piece, s: f64, t: f64) -> V3 {
    let e = piece.evaluate(s, t);
    unit(cross(e.ds, e.dt))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_series_reciprocal_inverts() {
        let f = Series::variable(6, 2.0, 0)
            .add(&Series::variable(6, 0.0, 1).mul(&Series::variable(6, 0.5, 1)));
        let one = f.mul(&f.recip());
        assert!((one.value() - 1.0).abs() < 1e-15);
        assert!(one.c[1..].iter().all(|x| x.abs() < 1e-14), "{:?}", one.c);
    }

    use super::test_json as json;

    use crate::invert::Side;
    use crate::trace2d::{critical, Inside};
    use crate::Patch;

    fn nurbs(s: &json::Json) -> Patch {
        let num = |k: &str| s.get(k).and_then(json::Json::num).unwrap();
        let nums = |k: &str| s.get(k).and_then(json::Json::nums).unwrap();
        let degree = nums("degree");
        Patch {
            u_degree: degree[0] as usize,
            v_degree: degree[1] as usize,
            u_count: num("nu") as usize,
            v_count: num("nv") as usize,
            poles: s
                .get("points")
                .and_then(json::Json::arr)
                .unwrap()
                .iter()
                .map(|p| p.nums().unwrap().try_into().unwrap())
                .collect(),
            weights: Some(nums("weights")),
            u_knots: nums("knots_u"),
            v_knots: nums("knots_v"),
            analytic: None,
        }
    }

    /// The piece holding patch parameters (u, v), and the local (s, t) there.
    fn local(pieces: &[Piece], (u, v): (f64, f64)) -> (&Piece, (f64, f64)) {
        let p = pieces
            .iter()
            .find(|p| p.u0 <= u && u <= p.u1 && p.v0 <= v && v <= p.v1)
            .unwrap();
        (p, ((u - p.u0) / (p.u1 - p.u0), (v - p.v0) / (p.v1 - p.v0)))
    }

    /// Feasibility gate (run with --ignored --nocapture): on a cell of one of
    /// ssi-corpus `y23_fig1a`'s two tubes about a singular point (the two loops
    /// cross there, the normals parallel), how well a Taylor polynomial of the
    /// separation holds it, whether the cell test proves the saddle, and
    /// whether two neighboring cells' polynomials agree on their shared edge.
    /// Reads the case from an ssi-corpus clone: `SSI_CORPUS` when set, else
    /// `corpus/ssi` of the Solidcore workspace (`scripts/fetch-ssi-corpus.sh`).
    #[test]
    #[ignore = "a report, not a check: reads the ssi-corpus (SSI_CORPUS)"]
    fn gate_fig1a_singular_point() {
        let corpus = std::env::var_os("SSI_CORPUS").map_or_else(
            || std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/ssi"),
            std::path::PathBuf::from,
        );
        let path = corpus.join("cases/data/y23_fig1a_two_loops_cross.json");
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{}: {e}; set SSI_CORPUS to an ssi-corpus clone",
                path.display()
            )
        });
        let case = json::parse(&text).unwrap();
        let surfaces = case.get("surfaces").and_then(json::Json::arr).unwrap();
        let (pa, pb) = (nurbs(&surfaces[0]), nurbs(&surfaces[1]));
        let (pieces_a, pieces_b) = (pa.bezier_pieces(0).unwrap(), pb.bezier_pieces(1).unwrap());
        let side_a = Side::with_pieces(&pa, 0, pieces_a.clone()).unwrap();
        let side_b = Side::with_pieces(&pb, 1, pieces_b.clone()).unwrap();
        let star = [0.0, 0.0, 0.2027129965];
        let (a, (sa, ta)) = local(&pieces_a, side_a.invert(star, None).unwrap());
        let (b, (ub, vb)) = local(&pieces_b, side_b.invert(star, None).unwrap());
        let ea = a.evaluate(sa, ta);
        let dir = normal(a, sa, ta);
        let nb = normal(b, ub, vb);
        eprintln!(
            "A local ({sa:.6}, {ta:.6}) at {:?}, B local ({ub:.6}, {vb:.6}); normals' angle {:.2e}; |A_s| {:.3} |A_t| {:.3}",
            ea.point,
            dot(dir, nb).abs().min(1.0).acos(),
            norm(ea.ds),
            norm(ea.dt)
        );
        let placements = [(-0.3, 0.2, "inside"), (-0.3, -1.0, "edge")];
        for (&size, &(fs, ft, place)) in [0.1, 0.03, 0.01, 0.003, 0.001]
            .iter()
            .flat_map(|s| placements.iter().map(move |f| (s, f)))
        {
            let (ha, hb) = (size / norm(ea.ds), size / norm(ea.dt));
            // The singular point off the cell's center, as a cell would hold it,
            // or on the cell's edge (here a knot line).
            let center = (sa + fs * ha, ta + ft * hb);
            let start = match separation(a, b, dir, center, (ub, vb, 0.0)) {
                Some(x) if x.2.is_finite() => x,
                _ => {
                    eprintln!("cell {size} {place}: no separation at the center");
                    continue;
                }
            };
            for &order in &[4usize, 6, 8] {
                let [_, _, series] = separation_series(a, b, dir, center, start, order).unwrap();
                let [_, _, longer] =
                    separation_series(a, b, dir, center, start, order + 3).unwrap();
                let tail: f64 = (order + 1..=order + 3)
                    .map(|d| longer.order_bound(d, ha, hb))
                    .sum();
                let (mut error, mut range) = (0.0_f64, 0.0_f64);
                for i in 0..=20 {
                    for j in 0..=20 {
                        let (x, y) = (
                            ha * (f64::from(i) / 10.0 - 1.0),
                            hb * (f64::from(j) / 10.0 - 1.0),
                        );
                        let truth = separation(a, b, dir, (center.0 + x, center.1 + y), start)
                            .map_or(f64::NAN, |r| r.2);
                        if truth.is_finite() {
                            error = error.max((series.eval(x, y) - truth).abs());
                            range = range.max(truth.abs());
                        }
                    }
                }
                let g = series.bernstein(-ha, ha, -hb, hb);
                let band = error.max(tail).max(1e-300);
                let verdict = match critical(&g, band / 1e-14) {
                    Inside::Free => "free".to_string(),
                    Inside::Unknown => "unknown".to_string(),
                    Inside::One(c) => {
                        let (s, t) = (
                            center.0 - ha + 2.0 * ha * c.s,
                            center.1 - hb + 2.0 * hb * c.t,
                        );
                        let p = a.evaluate(s, t).point;
                        format!(
                            "one, index {} value {:.1e} at {:.1e} from the singular point",
                            c.index,
                            g.at(c.s, c.t),
                            norm(sub(p, star))
                        )
                    }
                };
                let cell = (center.0 - ha, center.0 + ha, center.1 - hb, center.1 + hb);
                let proved = match local_field(a, b, dir, cell, start, order) {
                    Some(local) => {
                        // The true gradient at the grid's roots: J w_x = A_x, its λ row.
                        let mut gerr = 0.0_f64;
                        let (gs, gt) = (local.g.derivative(0), local.g.derivative(1));
                        let mut hess = 0.0_f64;
                        for i in 0..=10 {
                            for j in 0..=10 {
                                let (cs, ct) = (f64::from(i) / 10.0, f64::from(j) / 10.0);
                                let (ps, pt) = (cell.0 + 2.0 * ha * cs, cell.2 + 2.0 * hb * ct);
                                let Some(w) = separation(a, b, dir, (ps, pt), start) else {
                                    continue;
                                };
                                let (ea, eb) = (a.evaluate(ps, pt), b.evaluate(w.0, w.1));
                                let cols = [eb.ds, eb.dt, scale(dir, -1.0)];
                                let (Some(xs), Some(xt)) =
                                    (solve3(cols, ea.ds), solve3(cols, ea.dt))
                                else {
                                    continue;
                                };
                                gerr = gerr
                                    .max((xs[2] - gs.at(cs, ct) / (2.0 * ha)).abs())
                                    .max((xt[2] - gt.at(cs, ct) / (2.0 * hb)).abs());
                                hess = hess.max(
                                    (local.g.derivative(0).derivative(0).at(cs, ct)
                                        / (4.0 * ha * ha))
                                        .abs(),
                                );
                            }
                        }
                        format!(
                            "proved {:.1e} ({:.1}x measured{}), gradient {:.1e} ({:.1}x measured {:.1e}{}), Hessian {:.1e} of |L_ss| {:.1e}",
                            local.band,
                            local.band / error,
                            if local.band >= error { "" } else { ", UNSOUND" },
                            local.gradient,
                            local.gradient / gerr,
                            gerr,
                            if local.gradient >= gerr { "" } else { ", UNSOUND" },
                            local.hessian,
                            hess
                        )
                    }
                    None => "not proved".to_string(),
                };
                eprintln!(
                    "cell {size:>6} {place:<6} order {order}: field up to {range:.2e}, off by {error:.1e}, {proved}: {verdict}"
                );
            }
        }
        // Cost of one local field, by order, on the 1% cell.
        for &order in &[4usize, 6, 8] {
            let (ha, hb) = (0.01 / norm(ea.ds), 0.01 / norm(ea.dt));
            let center = (sa - 0.3 * ha, ta + 0.2 * hb);
            let start = separation(a, b, dir, center, (ub, vb, 0.0)).unwrap();
            let cell = (center.0 - ha, center.0 + ha, center.1 - hb, center.1 + hb);
            let clock = std::time::Instant::now();
            for _ in 0..20 {
                std::hint::black_box(local_field(a, b, dir, cell, start, order));
            }
            eprintln!(
                "cost order {order}: {:.2} ms a local field",
                clock.elapsed().as_secs_f64() * 1e3 / 20.0
            );
        }
        // Two neighbors' polynomials on their shared edge s = s0 + ha.
        let (size, order) = (0.01, 6);
        let (ha, hb) = (size / norm(ea.ds), size / norm(ea.dt));
        let left = (sa - 0.3 * ha, ta + 0.2 * hb);
        let right = (left.0 + 2.0 * ha, left.1);
        let fields: Vec<(Bern, f64)> = [left, right]
            .iter()
            .map(|&c| {
                let start = separation(a, b, dir, c, (ub, vb, 0.0)).unwrap();
                let [_, _, series] = separation_series(a, b, dir, c, start, order).unwrap();
                let mut error = 0.0_f64;
                for i in 0..=20 {
                    for j in 0..=20 {
                        let (x, y) = (
                            ha * (f64::from(i) / 10.0 - 1.0),
                            hb * (f64::from(j) / 10.0 - 1.0),
                        );
                        let truth = separation(a, b, dir, (c.0 + x, c.1 + y), start).unwrap().2;
                        error = error.max((series.eval(x, y) - truth).abs());
                    }
                }
                (series.bernstein(-ha, ha, -hb, hb), error)
            })
            .collect();
        let edge_roots = |g: &Bern, e: usize, band: f64| crate::field::roots(&g.edge(e), band);
        let from_left = edge_roots(&fields[0].0, 1, fields[0].1);
        let from_right = edge_roots(&fields[1].0, 0, fields[1].1);
        // The separation itself along the edge: its sign changes, by bisection.
        let s_edge = left.0 + ha;
        let start = separation(a, b, dir, (s_edge, left.1), (ub, vb, 0.0)).unwrap();
        let lam = |t: f64| separation(a, b, dir, (s_edge, t), start).unwrap().2;
        let mut truth = Vec::new();
        let steps = 400;
        for k in 0..steps {
            let (mut lo, mut hi) = (
                left.1 - hb + 2.0 * hb * f64::from(k) / f64::from(steps),
                left.1 - hb + 2.0 * hb * f64::from(k + 1) / f64::from(steps),
            );
            if (lam(lo) > 0.0) != (lam(hi) > 0.0) {
                for _ in 0..60 {
                    let mid = 0.5 * (lo + hi);
                    if (lam(mid) > 0.0) == (lam(lo) > 0.0) {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                truth.push((0.5 * (lo + hi) - (left.1 - hb)) / (2.0 * hb));
            }
        }
        eprintln!(
            "shared edge, cell 0.01 order 6: left cell's roots {from_left:?} (band {:.1e}), right cell's {from_right:?} (band {:.1e}), the separation's own {truth:?}",
            fields[0].1, fields[1].1
        );
    }

    #[test]
    fn a_window_past_the_square_matches_the_polynomial() {
        let b = Bern {
            du: 3,
            dv: 2,
            c: (0..12).map(|k| f64::from((k * 7) % 5) - 1.5).collect(),
        };
        let w = window(&b, -0.6, 0.02, 1.1, 1.7);
        for &(x, y) in &[(0.0, 0.0), (0.4, 0.9), (1.0, 1.0), (0.7, 0.2)] {
            let (s, t) = (-0.6 + 0.62 * x, 1.1 + 0.6 * y);
            assert!((w.at(x, y) - b.at(s, t)).abs() < 1e-12, "{x} {y}");
        }
    }

    #[test]
    fn bernstein_form_matches_the_series() {
        let x = Series::variable(4, 0.3, 0);
        let y = Series::variable(4, -0.2, 1);
        let p = x
            .mul(&x)
            .mul(&y)
            .add(&y.scaled(3.0))
            .add(&Series::constant(4, 0.5));
        let b = p.bernstein(-0.1, 0.2, 0.05, 0.4);
        for &(cs, ct) in &[(0.0, 0.0), (0.3, 0.7), (1.0, 0.5), (0.9, 1.0)] {
            let (xx, yy) = (-0.1 + 0.3 * cs, 0.05 + 0.35 * ct);
            assert!((b.at(cs, ct) - p.eval(xx, yy)).abs() < 1e-14, "{cs} {ct}");
        }
    }
}

// The corpus runner's JSON reader, shared with the tests above. Declared here, outside the inline
// `tests` module, so its #[path] is relative to src/ (inside `tests` it went through the
// nonexistent src/taylor/tests/, which fails to resolve on Linux).
#[cfg(test)]
#[path = "bin/ssi_corpus_runner/json.rs"]
#[allow(dead_code)]
mod test_json;
