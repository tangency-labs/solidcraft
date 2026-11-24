//! A certified enclosure of the volume a variable round's balls take off a box edge, in
//! rounded interval arithmetic ([`Ivl`]). Every bound below holds for the exact reals.
//!
//! **What is enclosed.** The edge runs along `s` in `[0, len]` between two planes. The
//! ball at edge position `t` has the law's radius `r(t)` and touches both planes. The law
//! is the row's Bezier polynomial in `w = m0 + k t`, with `k = (m1 - m0) / len`, continued
//! past the edge's ends. The family is the balls at `t` in `F = [-len / 4, 5 len / 4]`, the
//! same range the skin-to-envelope certificate takes. The section at `s` is cut by the line
//! at distance `a` from one plane. Ball `t = s + d` reaches that line where
//! `q = a (2 r(t) - a) - d^2 >= 0`, and its lowest point on the line lies at distance
//! `g(d) = r(t) - sqrt(q)` from the other plane. The union's lowest point is
//! `L(s, a) = min_d g(d)`.
//!
//! `0 <= L <= r(s)`: the ball `d = 0` gives `r(s) - sqrt(a (2 r(s) - a)) <= r(s)`, and every
//! `g >= 0` because `q <= r(t)^2`. So the volume is
//! `V* = int_0^len int_0^r(s) L da ds = int_0^len int_0^1 2 r(s) u L(s, r(s) u^2) du ds`.
//! This is the reference `wr646_variable_round_follows_law.rs` approximates by quadrature
//! (there, the balls within `1.05 r_max` of `s`). Since `a = r u^2`, `sqrt(a)` is linear in
//! `u`, so the integrand is smooth up to `u = 0`.
//!
//! **The strip `u <= u0 = 2^-8`**, per cell `S` of `s`.
//! - Above: `L <= g(0) = r - r u sqrt(2 - u^2)`. So `int_0^u0 2 r u L du <=
//!   r^2 (u0^2 - 2/3 u0^3 sqrt(2 - u0^2))`.
//! - Below: a reaching ball has `d^2 <= 2 a r(t) <= 2 a R`, with `R = sup_F r`. It has
//!   `g >= r(t) - sqrt(2 a r(t))`, which increases with `r(t)` past `a / 2`. So
//!   `L >= rho - u sqrt(2 r(s) rho)`, with `rho` the least radius within `sqrt(2 a R)` of
//!   `S`, and `int_0^u0 2 r u L du >= r rho u0^2 - 2/3 u0^3 r sqrt(2 r rho)`.
//!
//! **A cell `C = S x U` of the rest**, with `d* (s, u)` the minimising ball.
//! - Pick any affine `tau(s, u)` near `d*` (Newton at the centre and nearby points).
//! - Certify, for every `(s, u)` in `C`, that the minimum is in an interval `T` that holds
//!   `tau(C)`:
//!   - on `T x C`: `q > 0` and `g_dd >= m > 0`, so `g` is convex on `T`;
//!   - `g_d < 0` at `T`'s lower end and `g_d > 0` at its upper end;
//!   - outward from each end, a contiguous run where `q > 0` and `g_dd > 0` holds, so `g`
//!     keeps rising away from `T`;
//!   - past the run, by branch and bound on `d` up to `R + 1` (no family ball reaches
//!     past `|d| = R`): `q < 0` (no reach), or `r - sqrt(q_hi) > G`. Here
//!     `G = sup_C g(tau) >= min_T g`.
//! - Then `L = min_T g`, and `g(tau) - g_d(tau)^2 / (2 m) <= L <= g(tau)`: below by the
//!   convex quadratic bound, above because `tau` is a ball.
//! - Both bounds are smooth on `C`. Each is integrated by the midpoint rule, with its
//!   remainder enclosed by the bound's Hessian over `C`, from a two-variable second-order
//!   interval jet. For `f(c + x) = f(c) + grad f(c) . x + x^T H(xi) x / 2`, the gradient
//!   term integrates to 0 on the symmetric cell, and
//!   `int_C f in |C| (f(c) + h_s^2 / 24 [H_ss] + h_u^2 / 24 [H_uu] + [-1, 1] w(H_su) h_s h_u / 32)`.
//!   Here `w` is the interval's width (`x_s x_u` integrates to 0, so only `H_su`'s spread
//!   counts, against `int |x_s x_u| = |C| h_s h_u / 16`).
//! - A cell whose certificate fails, or whose enclosure is wider than `EPS` per unit area,
//!   is halved.
//!
//! Cells are dyadic, so every centre and width is exact.
//!
//! [`Ivl`]: solidcore::blend::fillet_section::interval::Ivl

use solidcore::blend::fillet_section::interval::Ivl;
use std::collections::BTreeMap;
use std::ops::{Add, Div, Mul, Sub};
use std::sync::{Arc, Mutex, OnceLock};

const Z: Ivl = Ivl::new(0.0, 0.0);

fn p(x: f64) -> Ivl {
    Ivl::from(x)
}

/// `x^2`, never below 0.
fn sq(x: Ivl) -> Ivl {
    let y = x * x;
    if x.lo <= 0.0 && x.hi >= 0.0 {
        Ivl::new(0.0, y.hi)
    } else {
        y
    }
}

fn width(x: Ivl) -> Ivl {
    p(x.hi) - p(x.lo)
}

fn meet(a: Ivl, b: Ivl) -> Option<Ivl> {
    let (lo, hi) = (a.lo.max(b.lo), a.hi.min(b.hi));
    (lo <= hi).then_some(Ivl::new(lo, hi))
}

/// A value over a cell with its first and second derivatives in `(s, u)`, each enclosed:
/// `h` holds the `ss`, `su` and `uu` second derivatives.
#[derive(Clone, Copy)]
struct J {
    v: Ivl,
    d: [Ivl; 2],
    h: [Ivl; 3],
}

const PAIRS: [(usize, usize); 3] = [(0, 0), (0, 1), (1, 1)];

impl J {
    fn c(v: Ivl) -> Self {
        Self {
            v,
            d: [Z; 2],
            h: [Z; 3],
        }
    }

    fn var(v: Ivl, i: usize) -> Self {
        let mut d = [Z; 2];
        d[i] = p(1.0);
        Self { v, d, h: [Z; 3] }
    }

    fn scale(self, c: Ivl) -> Self {
        Self {
            v: self.v * c,
            d: self.d.map(|x| x * c),
            h: self.h.map(|x| x * c),
        }
    }

    /// `f(self)`, given `f`, `f'` and `f''` enclosed over `self.v`.
    fn chain(self, f: Ivl, f1: Ivl, f2: Ivl) -> Self {
        let mut h = [Z; 3];
        for (k, &(i, j)) in PAIRS.iter().enumerate() {
            let dd = if i == j {
                sq(self.d[i])
            } else {
                self.d[i] * self.d[j]
            };
            h[k] = f1 * self.h[k] + f2 * dd;
        }
        Self {
            v: f,
            d: self.d.map(|x| f1 * x),
            h,
        }
    }

    fn sqrt(self) -> Self {
        let y = self.v.sqrt();
        self.chain(y, p(1.0) / (p(2.0) * y), p(-1.0) / (p(4.0) * y * y * y))
    }

    fn recip(self) -> Self {
        let y = p(1.0) / self.v;
        self.chain(y, p(-1.0) * sq(y), p(2.0) * y * y * y)
    }
}

impl Add for J {
    type Output = Self;
    fn add(self, b: Self) -> Self {
        Self {
            v: self.v + b.v,
            d: [self.d[0] + b.d[0], self.d[1] + b.d[1]],
            h: [self.h[0] + b.h[0], self.h[1] + b.h[1], self.h[2] + b.h[2]],
        }
    }
}

impl Sub for J {
    type Output = Self;
    fn sub(self, b: Self) -> Self {
        Self {
            v: self.v - b.v,
            d: [self.d[0] - b.d[0], self.d[1] - b.d[1]],
            h: [self.h[0] - b.h[0], self.h[1] - b.h[1], self.h[2] - b.h[2]],
        }
    }
}

impl Mul for J {
    type Output = Self;
    fn mul(self, b: Self) -> Self {
        let a = self;
        let mut h = [Z; 3];
        for (k, &(i, j)) in PAIRS.iter().enumerate() {
            h[k] = a.h[k] * b.v + a.d[i] * b.d[j] + a.d[j] * b.d[i] + a.v * b.h[k];
        }
        Self {
            v: a.v * b.v,
            d: [
                a.d[0] * b.v + a.v * b.d[0],
                a.d[1] * b.v + a.v * b.d[1],
            ],
            h,
        }
    }
}

impl Div for J {
    type Output = Self;
    // a / b is a * (1 / b), the reciprocal's jet by the chain rule
    #[allow(clippy::suspicious_arithmetic_impl)]
    fn div(self, b: Self) -> Self {
        self * b.recip()
    }
}

fn horner(c: &[Ivl], w: Ivl) -> Ivl {
    c.iter().rev().fold(Z, |acc, &ci| acc * w + ci)
}

fn horner_j(c: &[Ivl], w: J) -> J {
    c.iter().rev().fold(J::c(Z), |acc, &ci| acc * w + J::c(ci))
}

fn horner_f(c: &[f64], w: f64) -> f64 {
    c.iter().rev().fold(0.0, |acc, &ci| acc * w + ci)
}

fn derivative(c: &[Ivl]) -> Vec<Ivl> {
    c.iter()
        .enumerate()
        .skip(1)
        .map(|(i, &ci)| p(i as f64) * ci)
        .collect()
}

/// A row's radius law on the edge: `r(t) = P(m0 + k t)`, `P` the Bezier polynomial in
/// monomial form, each coefficient enclosing the exact one.
pub(super) struct Law {
    c: Vec<Ivl>,
    c1: Vec<Ivl>,
    c2: Vec<Ivl>,
    cf: [Vec<f64>; 3],
    m0: Ivl,
    k: Ivl,
    len: f64,
    /// The family's range of `t`.
    fam: Ivl,
    /// `R`: at least every family ball's radius.
    r_max: f64,
}

impl Law {
    /// The law with Bezier control radii `radii` mapped from `edge_map` onto an edge of
    /// length `len`.
    pub(super) fn new(radii: &[f64], edge_map: [f64; 2], len: f64) -> Self {
        let b: Vec<Ivl> = radii.iter().map(|&r| p(r)).collect();
        let (two, three) = (p(2.0), p(3.0));
        let c = match *b.as_slice() {
            [p0, p1, p2] => vec![p0, two * (p1 - p0), p0 - two * p1 + p2],
            [p0, p1, p2, p3] => vec![
                p0,
                three * (p1 - p0),
                three * (p0 - two * p1 + p2),
                p3 - three * p2 + three * p1 - p0,
            ],
            _ => panic!("a quadratic or cubic Bezier law, not {radii:?}"),
        };
        let c1 = derivative(&c);
        let c2 = derivative(&c1);
        let mid = |v: &[Ivl]| v.iter().map(|x| x.mid()).collect::<Vec<f64>>();
        let (m0, m1) = (edge_map[0].min(edge_map[1]), edge_map[0].max(edge_map[1]));
        let fam = Ivl::new(-0.25 * len, 1.25 * len);
        assert!(fam.lo * -4.0 == len && fam.hi * 0.8 == len, "the family's ends are exact");
        let mut law = Self {
            cf: [mid(&c), mid(&c1), mid(&c2)],
            c,
            c1,
            c2,
            m0: p(m0),
            k: (p(m1) - p(m0)) / p(len),
            len,
            fam,
            r_max: f64::INFINITY,
        };
        let n = 1024;
        let step = (fam.hi - fam.lo) / f64::from(n);
        law.r_max = (0..n)
            .map(|i| {
                let lo = fam.lo + step * f64::from(i);
                let hi = if i + 1 == n { fam.hi } else { lo + step };
                law.r(Ivl::new(lo, hi)).hi
            })
            .fold(0.0, f64::max);
        law
    }

    fn w(&self, t: Ivl) -> Ivl {
        self.m0 + self.k * t
    }

    fn r(&self, t: Ivl) -> Ivl {
        horner(&self.c, self.w(t))
    }

    fn r1(&self, t: Ivl) -> Ivl {
        self.k * horner(&self.c1, self.w(t))
    }

    fn r2(&self, t: Ivl) -> Ivl {
        sq(self.k) * horner(&self.c2, self.w(t))
    }

    /// `r`, `r'` and `r''` at `t`, in floating point (for Newton only).
    fn rf(&self, t: f64) -> (f64, f64, f64) {
        let (k, w) = (self.k.mid(), self.m0.mid() + self.k.mid() * t);
        (
            horner_f(&self.cf[0], w),
            k * horner_f(&self.cf[1], w),
            k * k * horner_f(&self.cf[2], w),
        )
    }
}

/// `(a, q, r(t), r'(t), r''(t))` over the boxes `s`, `u`, `d`, with `t` a box holding every
/// `s + d` that is a family ball.
fn parts(law: &Law, s: Ivl, u: Ivl, d: Ivl, t: Ivl) -> (Ivl, Ivl, Ivl, Ivl, Ivl) {
    let a = law.r(s) * sq(u);
    let rt = law.r(t);
    let q = a * (p(2.0) * rt - a) - sq(d);
    (a, q, rt, law.r1(t), law.r2(t))
}

/// `g_d = r' - (a r' - d) / sqrt(q)`.
fn g_d(a: Ivl, q: Ivl, r1: Ivl, d: Ivl) -> Ivl {
    r1 - (a * r1 - d) / q.sqrt()
}

/// `g_dd = r'' - (a r'' - 1) / sqrt(q) + (a r' - d)^2 / q^(3/2)`.
fn g_dd(a: Ivl, q: Ivl, r1: Ivl, r2: Ivl, d: Ivl) -> Ivl {
    let root = q.sqrt();
    r2 - (a * r2 - p(1.0)) / root + sq(a * r1 - d) / (q * root)
}

/// `(r(s), g(tau), g_d(tau))` as jets in `(s, u)`.
fn jets(law: &Law, s: J, u: J, d: J) -> (J, J, J) {
    let rs = horner_j(&law.c, J::c(law.m0) + s.scale(law.k));
    let a = rs * u * u;
    let w = J::c(law.m0) + (s + d).scale(law.k);
    let rt = horner_j(&law.c, w);
    let r1t = horner_j(&law.c1, w).scale(law.k);
    let q = a * (rt.scale(p(2.0)) - a) - d * d;
    let root = q.sqrt();
    let g = rt - root;
    let gd = r1t - (a * r1t - d) / root;
    (rs, g, gd)
}

/// The ball `d` that minimises `g` at `(s, u)`, by damped Newton from `d = 0`.
fn argmin(law: &Law, s: f64, u: f64) -> f64 {
    let a = law.rf(s).0 * u * u;
    let at = |d: f64| {
        let (rt, r1, r2) = law.rf(s + d);
        let q = a * (2.0 * rt - a) - d * d;
        let root = q.sqrt();
        let e = a * r1 - d;
        (q, r1 - e / root, r2 - (a * r2 - 1.0) / root + e * e / (q * root))
    };
    let mut d = 0.0;
    for _ in 0..80 {
        let (q, g1, g2) = at(d);
        if q <= 0.0 || !g1.is_finite() {
            break;
        }
        let mut step = if g2 > 0.0 { g1 / g2 } else { g1.signum() * 1e-3 };
        let mut next = d - step;
        for _ in 0..80 {
            if at(next).0 > 0.0 {
                break;
            }
            step *= 0.5;
            next = d - step;
        }
        let done = (next - d).abs() <= 1e-15 * (1.0 + d.abs());
        d = next;
        if done {
            break;
        }
    }
    d
}

/// Every family ball `d` from `x0` outward in direction `dir` (`1` or `-1`) is no lower on the
/// line than `min_T g` for every `(s, u)` in the cell: a contiguous convex run first (`g`
/// rises along it, given `g_d`'s sign at `x0`), then branch and bound against `g_hi`.
fn outside(law: &Law, s: Ivl, u: Ivl, x0: f64, dir: f64, step0: f64, g_hi: f64) -> bool {
    let bound = law.r_max + 1.0;
    let span = |x: f64, y: f64| Ivl::new(x.min(y), x.max(y));
    let (mut x, mut step) = (x0, step0);
    let min_step = step0 * 1e-3;
    while dir * x < bound {
        let y = x + dir * step;
        let d = span(x, y);
        let t = s + d;
        let convex = law.fam.lo <= t.lo && t.hi <= law.fam.hi && {
            let (a, q, _, r1, r2) = parts(law, s, u, d, t);
            q.lo > 0.0 && g_dd(a, q, r1, r2, d).lo > 0.0
        };
        if convex {
            x = y;
            step *= 2.0;
        } else if step > min_step {
            step *= 0.5;
        } else {
            break;
        }
    }
    if dir * x >= bound {
        return true;
    }
    let mut stack = vec![span(x, dir * bound)];
    let mut evals = 0;
    while let Some(d) = stack.pop() {
        evals += 1;
        if evals > 200_000 {
            return false;
        }
        let Some(t) = meet(s + d, law.fam) else {
            continue;
        };
        let (_, q, rt, _, _) = parts(law, s, u, d, t);
        if q.hi < 0.0 {
            continue;
        }
        let low = (p(rt.lo) - p(q.hi.max(0.0)).sqrt()).lo;
        if low > g_hi {
            continue;
        }
        let m = d.mid();
        if !(d.lo < m && m < d.hi) || d.hi - d.lo < 1e-12 {
            return false;
        }
        stack.push(Ivl::new(d.lo, m));
        stack.push(Ivl::new(m, d.hi));
    }
    true
}

/// `m > 0` with `g_dd >= m` on `T x C` for a certified `T` holding `tau`, or `None`.
fn certify(law: &Law, s: Ivl, u: Ivl, tau: Ivl, g_hi: f64, reach: f64) -> Option<f64> {
    for frac in [0.02, 0.05, 0.1, 0.2, 0.4] {
        let w = frac * reach + (tau.hi - tau.lo);
        let tt = Ivl::new(tau.lo - w, tau.hi + w);
        let t = s + tt;
        if !(law.fam.lo <= t.lo && t.hi <= law.fam.hi) {
            continue;
        }
        let (a, q, _, r1, r2) = parts(law, s, u, tt, t);
        if q.lo.is_nan() || q.lo <= 0.0 {
            continue;
        }
        let m = g_dd(a, q, r1, r2, tt).lo;
        if m.is_nan() || m <= 0.0 {
            continue;
        }
        let end = |x: f64| {
            let d = p(x);
            let (a, q, _, r1, _) = parts(law, s, u, d, s + d);
            g_d(a, q, r1, d)
        };
        if !(end(tt.lo).hi < 0.0 && end(tt.hi).lo > 0.0) {
            continue;
        }
        if outside(law, s, u, tt.hi, 1.0, w, g_hi) && outside(law, s, u, tt.lo, -1.0, w, g_hi) {
            return Some(m);
        }
    }
    None
}

/// The midpoint rule's enclosure of `int_C f`, from the centre value and the Hessian.
fn midpoint(fc: Ivl, f: J, hs: f64, hu: f64) -> Ivl {
    let area = p(hs) * p(hu);
    let rem = sq(p(hs)) / p(24.0) * f.h[0]
        + sq(p(hu)) / p(24.0) * f.h[2]
        + Ivl::new(-1.0, 1.0) * width(f.h[1]) * p(hs) * p(hu) / p(32.0);
    area * (fc + rem)
}

/// What one cell gives: its enclosure, and whether `s` is the better side to halve.
enum Cell {
    Done(Ivl, bool),
    Uncertified,
}

fn cell(law: &Law, s0: f64, s1: f64, u0: f64, u1: f64) -> Cell {
    let (sc, uc) = (0.5 * (s0 + s1), 0.5 * (u0 + u1));
    let (hs, hu) = (s1 - s0, u1 - u0);
    let tau0 = argmin(law, sc, uc);
    let ds = (argmin(law, sc + 0.25 * hs, uc) - argmin(law, sc - 0.25 * hs, uc)) / (0.5 * hs);
    let du = (argmin(law, sc, uc + 0.25 * hu) - argmin(law, sc, uc - 0.25 * hu)) / (0.5 * hu);
    if !(tau0.is_finite() && ds.is_finite() && du.is_finite()) {
        return Cell::Uncertified;
    }
    let (s, u) = (Ivl::new(s0, s1), Ivl::new(u0, u1));
    let tau = J {
        v: p(tau0) + p(ds) * (s - p(sc)) + p(du) * (u - p(uc)),
        d: [p(ds), p(du)],
        h: [Z; 3],
    };
    let (rs, g, gd) = jets(law, J::var(s, 0), J::var(u, 1), tau);
    if !(g.v.is_finite() && gd.v.is_finite()) {
        return Cell::Uncertified;
    }
    let reach = {
        let r = law.rf(sc).0;
        let a = r * uc * uc;
        (a * (2.0 * r - a)).sqrt()
    };
    let Some(m) = certify(law, s, u, tau.v, g.v.hi, reach) else {
        return Cell::Uncertified;
    };
    let two_m = p(2.0) * p(m);
    let up = (rs * J::var(u, 1) * g).scale(p(2.0));
    let lo = (rs * J::var(u, 1) * (g - (gd * gd).scale(p(1.0) / two_m))).scale(p(2.0));
    let (rc, gc, gdc) = jets(
        law,
        J::var(p(sc), 0),
        J::var(p(uc), 1),
        J::c(p(tau0)),
    );
    let base = p(2.0) * rc.v * p(uc);
    let up = midpoint(base * gc.v, up, hs, hu);
    let lo = midpoint(base * (gc.v - sq(gdc.v) / two_m), lo, hs, hu);
    let enc = Ivl::new(lo.lo, up.hi);
    if !enc.is_finite() {
        return Cell::Uncertified;
    }
    let s_side = sq(p(hs)) * width(rs.h[0] * g.v + g.h[0]);
    let u_side = sq(p(hu)) * width(g.h[2]);
    Cell::Done(enc, s_side.hi > u_side.hi)
}

/// The certified result for one law.
#[derive(Clone, Copy, Debug)]
pub(super) struct Envelope {
    /// Holds `V*`.
    pub(super) volume: Ivl,
    /// `sqrt 2 int_0^len r ds`, rounded down: the envelope's area is at least this (each
    /// section curve joins the two contact lines, `sqrt 2 r(s)` apart).
    pub(super) area_lo: f64,
    /// How many cells the enclosure took.
    pub(super) cells: usize,
}

/// Width per unit of `(s, u)` area at which a cell is kept.
const EPS: f64 = 1e-6;
/// The deepest a cell is halved.
const MAX_DEPTH: u32 = 22;

fn enclose(law: &Law) -> Envelope {
    let len = law.len;
    let u0 = 2.0_f64.powi(-8);
    let mut total = Z;
    // the strip u <= u0
    let n_strip = 4096;
    let hs = len / f64::from(n_strip);
    let (two, three) = (p(2.0), p(3.0));
    let u0i = p(u0);
    let mut sum_r = Z;
    for i in 0..n_strip {
        let s = Ivl::new(hs * f64::from(i), hs * f64::from(i + 1));
        let r = law.r(s);
        sum_r = sum_r + p(hs) * p(r.lo);
        let reach = (two * p(r.hi) * sq(u0i) * p(law.r_max)).sqrt().hi;
        let near = meet(s + Ivl::new(-reach, reach), law.fam).expect("near balls");
        let rho = p(law.r(near).lo);
        let cube = sq(u0i) * u0i;
        let above = sq(r) * (sq(u0i) - two / three * cube * (two - sq(u0i)).sqrt());
        let below = r * rho * sq(u0i) - two / three * cube * r * (two * r * rho).sqrt();
        total = total + p(hs) * Ivl::new(below.lo.max(0.0), above.hi);
    }
    // the cells, u from u0 to 1 in octaves of 8 rows, s in 64 columns
    let mut stack: Vec<(f64, f64, f64, f64, u32)> = Vec::new();
    let hs = len / 64.0;
    for j in 1..=8 {
        let (ua, ub) = (2.0_f64.powi(-j), 2.0_f64.powi(1 - j));
        let hu = (ub - ua) / 8.0;
        for iu in 0..8 {
            for is in 0..64 {
                let (s0, uu0) = (hs * f64::from(is), ua + hu * f64::from(iu));
                stack.push((s0, s0 + hs, uu0, uu0 + hu, 0));
            }
        }
    }
    let mut cells = 0;
    while let Some((s0, s1, u_lo, u_hi, depth)) = stack.pop() {
        let (sm, um) = (0.5 * (s0 + s1), 0.5 * (u_lo + u_hi));
        match cell(law, s0, s1, u_lo, u_hi) {
            Cell::Done(enc, s_side) => {
                let area = (s1 - s0) * (u_hi - u_lo);
                if enc.hi - enc.lo <= EPS * area || depth >= MAX_DEPTH {
                    total = total + enc;
                    cells += 1;
                } else if s_side {
                    stack.push((s0, sm, u_lo, u_hi, depth + 1));
                    stack.push((sm, s1, u_lo, u_hi, depth + 1));
                } else {
                    stack.push((s0, s1, u_lo, um, depth + 1));
                    stack.push((s0, s1, um, u_hi, depth + 1));
                }
            }
            Cell::Uncertified => {
                assert!(
                    depth < MAX_DEPTH,
                    "no certificate for the cell s [{s0}, {s1}] u [{u_lo}, {u_hi}]"
                );
                for (a, b) in [(s0, sm), (sm, s1)] {
                    stack.push((a, b, u_lo, um, depth + 1));
                    stack.push((a, b, um, u_hi, depth + 1));
                }
            }
        }
    }
    Envelope {
        volume: total,
        area_lo: (two.sqrt() * sum_r).lo,
        cells,
    }
}

/// [`enclose`] for the law, once per law however many rows share it.
pub(super) fn envelope(radii: &[f64], edge_map: [f64; 2], len: f64) -> Envelope {
    type Slot = Arc<OnceLock<Envelope>>;
    static DONE: OnceLock<Mutex<BTreeMap<String, Slot>>> = OnceLock::new();
    let (m0, m1) = (edge_map[0].min(edge_map[1]), edge_map[0].max(edge_map[1]));
    let key = format!("{radii:?} {m0:?} {m1:?} {len:?}");
    let slot = {
        let mut map = DONE
            .get_or_init(|| Mutex::new(BTreeMap::new()))
            .lock()
            .unwrap();
        map.entry(key).or_default().clone()
    };
    *slot.get_or_init(|| enclose(&Law::new(radii, edge_map, len)))
}
