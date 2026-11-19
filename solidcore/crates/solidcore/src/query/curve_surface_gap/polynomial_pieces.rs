//! A proved bound on `sup |C(t) − S(W(t))|`, a curve's same-parameter gap to the surface's
//! image of a UV curve on it, from the exact polynomials of each piece (not upstream).
//!
//! On a piece `[a, b]` inside one knot span of the curve `C` and one of the UV curve `W`, and
//! with `W`'s values over the piece inside one knot cell of the surface's net `S`, all three
//! are polynomials in homogeneous form: `C = Cn / Cw`, `W = (Wu, Wv) / Ww`, and the cell's
//! `S = Sn / Sw`, of degrees `p` in u and `q` in v. With `U = Wu − u0 Ww`, `V = Wv − v0 Ww`
//! (any `(u0, v0)`), the cell's Taylor expansion about `(u0, v0)` is finite, so
//!
//! `S(W) = H / Hw`, `H = Σ_{a ≤ p, b ≤ q} S_ab U^a V^b Ww^(p+q−a−b)`
//!
//! (`S_ab` the cell's homogeneous partials at `(u0, v0)` over `a! b!`), and
//!
//! `C − S(W) = (Cn Hw − H Cw) / (Cw Hw) = N / D`,
//!
//! a quotient of two polynomials in `δ = t − m` (`m` the piece's middle), each a finite sum
//! `Σ c_j δ^j`. Over `|δ| ≤ ρ` (`ρ` at least half the piece), `|N| ≤ Σ |c_j| ρ^j` per
//! coordinate and `D ≥ d_0 − Σ_{j ≥ 1} |d_j| ρ^j`; where that is positive, `|N| / D` bounds
//! the gap over the piece. Every coefficient is enclosed in outward-rounded interval
//! arithmetic from the stored control points, weights and knots, so the bound holds for the
//! exact curves. Because `N` is formed before it is bounded, the near cancellation of `C`
//! and `S(W)` happens in its coefficients: a trim made from its edge is bounded at its true,
//! tiny gap on a piece as long as a knot span, where the second-order enclosure of
//! [`super::certified_gap`] (`h² / 8 sup |g''|`, each of `C''` and `(S∘W)''` hulled apart)
//! shrinks only with the cube of the piece.
//!
//! Where `W`'s values over a piece reach into several cells, the gap at each `t` is that of
//! the cell holding `W(t)`, so the largest of the cells' bounds (each cell's polynomial
//! taken over the whole piece) bounds it. Past the net's end (a box a ulp out on a seam or
//! boundary) the end cell's polynomial is continued, plus the slack `NetSurface::partials`
//! adds there, as [`super::piece`] does. A piece whose values reach into more than
//! [`MAX_CELLS`] cells, run too far past the net, or whose `D` is not proved positive is
//! bounded by [`super::piece`] instead.
//!
//! | Rust                      | Upstream |
//! |---------------------------|----------|
//! | [`trim_gap_bound`]        | none     |
//!
//! Deviations: none (new code). The bisection is bounded by `MAX_PIECES` and `MAX_DEPTH`
//! of [`super`], and polls cancellation at every piece.

use super::{piece, NetSurface, CLOSE_ABSOLUTE, CLOSE_RELATIVE, MAX_DEPTH, MAX_PIECES, OP};
use crate::blend::fillet_section::certificate::NurbsCurve;
use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_section::series::V3;
use crate::blend::fillet_section::station::{ders_basis, knot, span, Nurbs};
use crate::kernel::Kernel;
use crate::math::Point2;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::surface::surface::Surface;
use crate::Result;

/// The most knot cells of the surface one piece's UV values may reach into before the piece
/// is bisected instead (or bounded by [`super::piece`]).
const MAX_CELLS: usize = 4;

/// A polynomial in `δ`, its coefficients enclosed (`c[j]` of `δ^j`).
type Poly = Vec<Ivl>;

fn zero() -> Ivl {
    Ivl::from(0.0)
}

/// `a b`.
fn mul(a: &[Ivl], b: &[Ivl]) -> Poly {
    let mut out = vec![zero(); (a.len() + b.len()).saturating_sub(1)];
    for (i, &x) in a.iter().enumerate() {
        for (j, &y) in b.iter().enumerate() {
            if let Some(o) = out.get_mut(i + j) {
                *o = *o + x * y;
            }
        }
    }
    out
}

/// `acc += s a`.
fn add_scaled(acc: &mut Poly, a: &[Ivl], s: Ivl) {
    if acc.len() < a.len() {
        acc.resize(a.len(), zero());
    }
    for (o, &x) in acc.iter_mut().zip(a) {
        *o = *o + x * s;
    }
}

/// `a − b`.
fn sub(a: &[Ivl], b: &[Ivl]) -> Poly {
    let mut out = a.to_vec();
    add_scaled(&mut out, b, Ivl::from(-1.0));
    out
}

/// `Σ_{j ≥ from} |c_j| ρ^j`, rounded up (not finite for an unbounded coefficient).
fn magnitude(c: &[Ivl], rho: f64, from: usize) -> f64 {
    let r = Ivl::from(rho);
    let mut power = Ivl::from(1.0);
    let mut sum = zero();
    for (j, &x) in c.iter().enumerate() {
        if j >= from {
            sum = sum + Ivl::from(x.mag()) * power;
        }
        power = power * r;
    }
    if sum.is_finite() {
        sum.hi
    } else {
        f64::INFINITY
    }
}

/// The values of `c` over `|δ| ≤ ρ`, enclosed.
fn range(c: &[Ivl], rho: f64) -> Ivl {
    let rest = magnitude(c, rho, 1);
    c.first().copied().unwrap_or_else(zero) + Ivl::new(-rest, rest)
}

/// `c(x)`, enclosed (Horner).
fn at(c: &[Ivl], x: Ivl) -> Ivl {
    c.iter().rev().fold(zero(), |acc, &cj| acc * x + cj)
}

/// `c(x + s)`'s coefficients, enclosed (repeated synthetic division).
fn shifted(c: &[Ivl], s: f64) -> Poly {
    let s = Ivl::from(s);
    let mut c = c.to_vec();
    let n = c.len();
    for k in 0..n.saturating_sub(1) {
        for j in (k..n - 1).rev() {
            let next = c.get(j + 1).copied().unwrap_or_else(zero);
            if let Some(cj) = c.get_mut(j) {
                *cj = *cj + s * next;
            }
        }
    }
    c
}

/// The `δ` in `[lo, hi]` where `num(δ) / den(δ)`, both of degree at most 1 and `den`
/// positive there, may lie in `[ka, kb]` (`ka` past the net's start and `kb` past its end
/// are `None`, no bound), enclosed: `None` where no `δ` does. A coefficient enclosure that
/// cannot decide a side leaves that side whole.
fn linear_preimage(
    num: &[Ivl],
    den: &[Ivl],
    ka: Option<f64>,
    kb: Option<f64>,
    [mut lo, mut hi]: [f64; 2],
) -> Option<[f64; 2]> {
    let coef = |c: &[Ivl], j: usize| c.get(j).copied().unwrap_or_else(zero);
    // `f(δ) = f0 + f1 δ >= 0`, an affine side
    let mut side = |f0: Ivl, f1: Ivl| {
        let root = (Ivl::from(0.0) - f0) / f1;
        if f1.lo > 0.0 && root.is_finite() {
            lo = lo.max(root.lo);
        } else if f1.hi < 0.0 && root.is_finite() {
            hi = hi.min(root.hi);
        } else if f1.lo == 0.0 && f1.hi == 0.0 && f0.hi < 0.0 {
            hi = f64::NEG_INFINITY;
        }
    };
    // u >= ka: num − ka den >= 0; u <= kb: kb den − num >= 0 (den > 0)
    if let Some(k) = ka {
        let k = Ivl::from(k);
        side(
            coef(num, 0) - k * coef(den, 0),
            coef(num, 1) - k * coef(den, 1),
        );
    }
    if let Some(k) = kb {
        let k = Ivl::from(k);
        side(
            k * coef(den, 0) - coef(num, 0),
            k * coef(den, 1) - coef(num, 1),
        );
    }
    (lo <= hi).then_some([lo, hi])
}

/// `1 / j!` enclosed.
fn inverse_factorial(j: usize) -> Ivl {
    let mut f = Ivl::from(1.0);
    for i in 2..=j {
        f = f * Ivl::from(i as f64);
    }
    Ivl::from(1.0) / f
}

/// The homogeneous Taylor coefficients `(x w, y w, z w, w)` about `m` of the polynomial of
/// knot span `k` of `cn` (exact: the span's polynomial has degree `cn.p`).
fn curve_taylor(cn: &NurbsCurve, k: usize, m: f64) -> [Poly; 4] {
    let p = cn.p;
    let ders = ders_basis(&cn.u, k, p, Ivl::from(m), p);
    let mut out: [Poly; 4] = std::array::from_fn(|_| vec![zero(); p + 1]);
    for j in 0..=p {
        let f = inverse_factorial(j);
        for i in 0..=p {
            let n = ders
                .get(j)
                .and_then(|r| r.get(i))
                .copied()
                .unwrap_or_else(zero);
            let base = (k + i).checked_sub(p).map(|c| 4 * c);
            for (comp, o) in out.iter_mut().enumerate() {
                let w = base
                    .and_then(|b| cn.pw.get(b + comp))
                    .copied()
                    .unwrap_or(f64::NAN);
                if let Some(c) = o.get_mut(j) {
                    *c = *c + n * Ivl::from(w) * f;
                }
            }
        }
    }
    out
}

/// The non-empty knot spans of `knots` (degree `p`, `n` control points) whose closed span
/// meets `[lo, hi]` clamped to the net's domain (a part past an end is on the end span,
/// continued); `None` for an unordered or not finite `[lo, hi]` or past [`MAX_CELLS`] spans.
fn spans_over(knots: &[f64], p: usize, n: usize, lo: f64, hi: f64) -> Option<Vec<usize>> {
    let (d0, d1) = (knot(knots, p as isize), knot(knots, n as isize));
    if !(lo <= hi && lo.is_finite() && hi.is_finite() && d0 < d1) {
        return None;
    }
    let (lo, hi) = (lo.max(d0).min(d1), hi.min(d1).max(d0));
    let mut out = Vec::new();
    // (from the span ending on lo as well: on a knot line both cells are taken, so the bound
    // holds whichever cell the surface is evaluated in there)
    let mut k = span(knots, p, n, lo);
    while k > p && knot(knots, k as isize) >= lo {
        k -= 1;
    }
    while k < n {
        let (a, b) = (knot(knots, k as isize), knot(knots, k as isize + 1));
        if a > hi {
            break;
        }
        if a < b && b >= lo {
            out.push(k);
            if out.len() > MAX_CELLS {
                return None;
            }
        }
        k += 1;
    }
    (!out.is_empty()).then_some(out)
}

/// One cell's `N` (three coordinates) and `D`, as polynomials in `δ`.
struct CellQuotient {
    n: [Poly; 3],
    d: Poly,
}

impl CellQuotient {
    /// The gap's bound over `|δ| ≤ ρ`; `None` where `D` is not proved positive.
    fn bound(&self, rho: f64) -> Option<f64> {
        let [x, y, z] = &self.n;
        let m = |c: &[Ivl]| Ivl::from(magnitude(c, rho, 0));
        let (mx, my, mz) = (m(x), m(y), m(z));
        let norm = (mx * mx + my * my + mz * mz).sqrt();
        let d0 = self.d.first().copied().unwrap_or_else(zero);
        let lower = (Ivl::from(d0.lo) - Ivl::from(magnitude(&self.d, rho, 1))).lo;
        // (a NaN lower bound is no bound either)
        if lower.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
            return None;
        }
        let b = (norm / Ivl::from(lower)).hi;
        b.is_finite().then_some(b)
    }

    /// The gap's bound over `δ` in `[lo, hi]` (the polynomials re-centred there); `None`
    /// where `D` is not proved positive.
    fn bound_over(&self, [lo, hi]: [f64; 2]) -> Option<f64> {
        let mid = 0.5 * (lo + hi);
        let rho = (Ivl::from(hi) - Ivl::from(mid))
            .hi
            .max((Ivl::from(mid) - Ivl::from(lo)).hi);
        if mid == 0.0 {
            return self.bound(rho);
        }
        let [x, y, z] = &self.n;
        let moved = Self {
            n: [shifted(x, mid), shifted(y, mid), shifted(z, mid)],
            d: shifted(&self.d, mid),
        };
        moved.bound(rho)
    }

    /// The gap at `δ`, an upper bound; `None` where `D(δ)` is not proved positive.
    fn at(&self, x: Ivl) -> Option<f64> {
        let [nx, ny, nz] = &self.n;
        let m = |c: &[Ivl]| Ivl::from(at(c, x).mag());
        let (mx, my, mz) = (m(nx), m(ny), m(nz));
        let norm = (mx * mx + my * my + mz * mz).sqrt();
        let d = at(&self.d, x);
        if d.lo.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
            return None;
        }
        let b = (norm / d).hi;
        b.is_finite().then_some(b)
    }
}

/// `N` and `D` of the cell `(ku, kv)` of `net`, for the curve's homogeneous Taylor
/// polynomials `c` and the UV curve's `w` (`(u w, v w, ·, w)`), about `(u0, v0)`.
fn cell_quotient(
    net: &Nurbs,
    (ku, kv): (usize, usize),
    (u0, v0): (f64, f64),
    c: &[Poly; 4],
    w: &[Poly; 4],
) -> CellQuotient {
    let (p, q) = (net.p, net.q);
    let nu = ders_basis(&net.u, ku, p, Ivl::from(u0), p);
    let nv = ders_basis(&net.v, kv, q, Ivl::from(v0), q);
    let [wu, wv, _, ww] = w;
    let uu = sub(wu, &mul(&[Ivl::from(u0)], ww));
    let vv = sub(wv, &mul(&[Ivl::from(v0)], ww));
    let powers = |base: &[Ivl], top: usize| {
        let mut out: Vec<Poly> = vec![vec![Ivl::from(1.0)]];
        for i in 1..=top {
            let next = out.get(i - 1).map_or_else(Vec::new, |prev| mul(prev, base));
            out.push(next);
        }
        out
    };
    let (up, vp, wp) = (powers(&uu, p), powers(&vv, q), powers(ww, p + q));
    let mut h: [Poly; 4] = std::array::from_fn(|_| Vec::new());
    for a in 0..=p {
        for b in 0..=q {
            let f = inverse_factorial(a) * inverse_factorial(b);
            let mut s = [zero(); 4];
            for i in 0..=p {
                for j in 0..=q {
                    let na = nu
                        .get(a)
                        .and_then(|r| r.get(i))
                        .copied()
                        .unwrap_or_else(zero);
                    let nb = nv
                        .get(b)
                        .and_then(|r| r.get(j))
                        .copied()
                        .unwrap_or_else(zero);
                    let li = (ku + i)
                        .checked_sub(p)
                        .zip((kv + j).checked_sub(q))
                        .map(|(ci, cj)| 4 * (ci * net.nv + cj));
                    for (comp, sc) in s.iter_mut().enumerate() {
                        let x = li
                            .and_then(|l| net.pw.get(l + comp))
                            .copied()
                            .unwrap_or(f64::NAN);
                        *sc = *sc + na * nb * Ivl::from(x);
                    }
                }
            }
            let (Some(ua), Some(vb), Some(wc)) = (up.get(a), vp.get(b), wp.get(p + q - a - b))
            else {
                continue;
            };
            let term = mul(&mul(ua, vb), wc);
            for (hc, sc) in h.iter_mut().zip(s) {
                add_scaled(hc, &term, sc * f);
            }
        }
    }
    let [cx, cy, cz, cw] = c;
    let [hx, hy, hz, hw] = &h;
    let n = [
        sub(&mul(cx, hw), &mul(hx, cw)),
        sub(&mul(cy, hw), &mul(hy, cw)),
        sub(&mul(cz, hw), &mul(hz, cw)),
    ];
    CellQuotient { n, d: mul(cw, hw) }
}

/// The bound of one piece `[a, b]` from its cells' quotients: `(end values, bound)`, or
/// `None` where the piece is not inside one knot span of each curve, its UV values leave
/// the net or reach into more than [`MAX_CELLS`] cells, or a denominator is not proved
/// positive.
fn polynomial_piece(
    cn: &NurbsCurve,
    wn: &NurbsCurve,
    s: &NetSurface,
    a: f64,
    b: f64,
) -> Option<(f64, f64)> {
    let m = 0.5 * (a + b);
    let (da, db) = (Ivl::from(a) - Ivl::from(m), Ivl::from(b) - Ivl::from(m));
    let rho = da.mag().max(db.mag());
    let within = |n: &NurbsCurve| {
        let k = span(&n.u, n.p, n.n, m);
        (knot(&n.u, k as isize) <= a && b <= knot(&n.u, k as isize + 1)).then_some(k)
    };
    let (kc, kw) = (within(cn)?, within(wn)?);
    let c = curve_taylor(cn, kc, m);
    let w = curve_taylor(wn, kw, m);
    let [wu, wv, _, ww] = &w;
    let uv_over = |x: Ivl| (at(wu, x) / at(ww, x), at(wv, x) / at(ww, x));
    let (bu, bv) = (
        range(wu, rho) / range(ww, rho),
        range(wv, rho) / range(ww, rho),
    );
    if !(bu.is_finite() && bv.is_finite()) {
        return None;
    }
    let net = &s.net;
    let us = spans_over(&net.u, net.p, net.nu, bu.lo, bu.hi)?;
    let vs = spans_over(&net.v, net.q, net.nv, bv.lo, bv.hi)?;
    if us.len() * vs.len() > MAX_CELLS {
        return None;
    }
    // A box past the net's end (a seam or boundary on the face's edge, its parameters a ulp
    // out): past the end the cells' polynomials are the end span's continued, `S~`, and the
    // surface is read at the clamped point, within `NetSurface::partials`' slack of `S~`
    // (none where the surface is a line in that direction); that refuses a box too far past.
    let past = |knots: &[f64], p: usize, n: usize, x: Ivl| {
        x.lo < knot(knots, p as isize) || x.hi > knot(knots, n as isize)
    };
    let slack = if past(&net.u, net.p, net.nu, bu) || past(&net.v, net.q, net.nv, bv) {
        let w0 = V3 {
            x: bu,
            y: bv,
            z: zero(),
        };
        s.partials(&[w0], 1, 0).ok()?.1
    } else {
        0.0
    };
    let ends_at = [uv_over(da), uv_over(db)];
    let (u0, v0) = (bu.mid(), bv.mid());
    // A UV curve of degree 1 (a line, or a rational one: `u = (a0 + a1 δ) / (b0 + b1 δ)`,
    // monotone where its denominator is positive) reaches each cell over one interval of
    // `δ`, enclosed by `linear_preimage`; each cell's polynomial is bounded over that part
    // alone, so a piece across a knot line closes as one on either side of it would. Any
    // other curve's cells are each bounded over the whole piece.
    let linear = wu.len() <= 2 && wv.len() <= 2 && ww.len() <= 2 && range(ww, rho).lo > 0.0;
    let limits = |knots: &[f64], p: usize, n: usize, k: usize| {
        let (a, b) = (knot(knots, k as isize), knot(knots, k as isize + 1));
        // (the end cells run on past the net's ends, see above)
        let a = (a > knot(knots, p as isize)).then_some(a);
        let b = (b < knot(knots, n as isize)).then_some(b);
        (a, b)
    };
    let mut bound = 0.0_f64;
    let mut ends = [f64::INFINITY; 2];
    for &ku in &us {
        for &kv in &vs {
            let part = if linear {
                let (ua, ub) = limits(&net.u, net.p, net.nu, ku);
                let (va, vb) = limits(&net.v, net.q, net.nv, kv);
                let part = linear_preimage(wu, ww, ua, ub, [-rho, rho]);
                part.and_then(|p| linear_preimage(wv, ww, va, vb, p))
            } else {
                Some([-rho, rho])
            };
            let Some(part) = part else {
                // (no t of the piece has W(t) in this cell)
                continue;
            };
            let cell = cell_quotient(net, (ku, kv), (u0, v0), &c, &w);
            bound = bound.max(cell.bound_over(part)?);
            let holds = |x: Ivl, k: usize, knots: &[f64]| {
                knot(knots, k as isize) <= x.lo && x.hi <= knot(knots, k as isize + 1)
            };
            for ((e, (eu, ev)), d) in ends.iter_mut().zip(ends_at).zip([da, db]) {
                if holds(eu, ku, &net.u) && holds(ev, kv, &net.v) {
                    *e = e.min(cell.at(d)?);
                }
            }
        }
    }
    // (an end whose value no cell holds alone, on a knot line within rounding, is taken at
    // 0: the scale only decides when a piece is closed, never the bound)
    let ends = ends
        .into_iter()
        .map(|e| if e.is_finite() { e } else { 0.0 })
        .fold(0.0_f64, f64::max);
    let bound = (Ivl::from(bound) + Ivl::from(slack)).hi;
    (bound.is_finite() && bound >= 0.0).then_some((ends, bound))
}

/// A proved upper bound on `sup |C(t) − S(W(t))|` over `t` in `range`, as
/// [`super::curve_uv_gap_bound`], for a caller that holds the gap to a tolerance `accept`:
/// a piece whose bound is at most `accept` is closed at once, so a result at most `accept`
/// says only that (the gap is no larger), and above it the result is bounded as tightly as
/// [`super::curve_uv_gap_bound`] bounds it. The first pieces end at every knot of `C` and
/// of `W` inside `range`; each piece is bounded from its exact polynomials
/// ([`polynomial_piece`], the module's bound) where it can be, else by [`super::piece`]
/// (whose second-order bound needs no corner inside a piece: every knot is a first cut).
///
/// `None` when the gap is not enclosed (as [`super::curve_uv_gap_bound`]: a surface that is
/// not a NURBS net, or whose net is not parameterised as the surface, an empty, unordered
/// or not finite `range` or one past either curve's domain, a `W` that cannot be evaluated,
/// or a piece neither bound holds).
///
/// # Errors
///
/// The kernel's unported and cancelled statuses.
pub(crate) fn trim_gap_bound(
    k: &Kernel,
    curve: &BSplineCurve,
    uv: &BSplineCurve,
    surface: &Surface,
    [lo, hi]: [f64; 2],
    floor: f64,
    accept: f64,
) -> Result<Option<f64>> {
    if !(lo.is_finite() && hi.is_finite() && lo < hi && floor.is_finite() && floor >= 0.0) {
        return Ok(None);
    }
    let accept = if accept >= 0.0 { accept } else { 0.0 };
    let mut probe = Vec::new();
    for t in [lo, 0.5 * (lo + hi), hi] {
        let Ok(w) = uv.evaluate_point(t) else {
            return Ok(None);
        };
        probe.push(Point2::new(w.x, w.y));
    }
    let surface = surface.clone_for_evaluation()?;
    let Some(net) = NetSurface::of(k, &surface, &probe)? else {
        return Ok(None);
    };
    let (Ok(cn), Ok(wn)) = (NurbsCurve::of(curve), NurbsCurve::of(uv)) else {
        return Ok(None);
    };
    let within = |n: &NurbsCurve| match (n.u.get(n.p), n.u.get(n.n)) {
        (Some(&d0), Some(&d1)) => d0 <= lo && hi <= d1,
        _ => false,
    };
    if !(within(&cn) && within(&wn)) {
        return Ok(None);
    }
    let mut cuts: Vec<f64> =
        cn.u.iter()
            .chain(&wn.u)
            .copied()
            .filter(|&x| lo < x && x < hi)
            .collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    if cuts.len() + 1 > MAX_PIECES {
        return Ok(None);
    }
    let mut ends = vec![lo];
    ends.extend(cuts);
    ends.push(hi);
    let mut stack: Vec<(f64, f64, u32)> = ends
        .windows(2)
        .filter_map(|w| match w {
            [a, b] => Some((*a, *b, 0)),
            _ => None,
        })
        .rev()
        .collect();
    let mut worst = 0.0_f64;
    let mut made = stack.len();
    while let Some((a, b, depth)) = stack.pop() {
        k.check_cancelled(OP)?;
        let mid = 0.5 * (a + b);
        let last = depth >= MAX_DEPTH || !(a < mid && mid < b) || made + 2 > MAX_PIECES;
        let bounded = match polynomial_piece(&cn, &wn, &net, a, b) {
            Some((ends, bound)) => Some((ends, bound, (Ivl::from(bound) - Ivl::from(ends)).hi)),
            None => match piece(&cn, &wn, &net, a, b) {
                Ok(Some((ends, term))) => {
                    Some((ends, (Ivl::from(ends) + Ivl::from(term)).hi, term))
                }
                Err(e) if e.is_unported_or_cancelled() => return Err(e),
                Ok(None) | Err(_) => None,
            },
        };
        match bounded {
            Some((ends, bound, term)) => {
                if bound <= accept
                    || last
                    || term <= (CLOSE_RELATIVE * ends).max(floor).max(CLOSE_ABSOLUTE)
                {
                    worst = worst.max(bound);
                    continue;
                }
            }
            None => {
                if last {
                    return Ok(None);
                }
            }
        }
        made += 2;
        stack.push((mid, b, depth + 1));
        stack.push((a, mid, depth + 1));
    }
    Ok(worst.is_finite().then_some(worst))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Point3;
    use crate::nurbs::b_spline_curve::KnotType;
    use crate::nurbs::b_spline_surface::{
        BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData,
    };
    use crate::nurbs::crv_basic::Curve as NurbCurve;
    use crate::nurbs::math::{CPoint, NOW};

    /// The plane z = 0 over x, y in 0..1 as a bilinear net cut at 0.5 both ways (four cells).
    fn plane() -> Surface {
        let mut points = Vec::new();
        for i in 0..3 {
            for j in 0..3 {
                points.push(Point3::new(0.5 * f64::from(i), 0.5 * f64::from(j), 0.0));
            }
        }
        let net = BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: 1,
            v_degree: 1,
            control_points: &points,
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &[2, 1, 2],
            v_multiplicities: &[2, 1, 2],
            u_knots: &[0.0, 0.5, 1.0],
            v_knots: &[0.0, 0.5, 1.0],
            knot_type: KnotType::Unspecified,
            weights: Some(&[1.0; 9]),
            uv_domain: None,
        })
        .unwrap();
        Surface::from(net)
    }

    /// Half the circle of radius 0.4 about `(cx, cy)` at height `z`, two rational quadratic
    /// quarters (knots 0, 0.5, 1).
    fn half_circle(cx: f64, cy: f64, z: f64) -> BSplineCurve {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let r = 0.4;
        let at = |x: f64, y: f64, w: f64| CPoint::new((cx + x) * w, (cy + y) * w, z * w, w);
        let c = NurbCurve::new(
            vec![
                at(r, 0.0, 1.0),
                at(r, r, w),
                at(0.0, r, 1.0),
                at(-r, r, w),
                at(-r, 0.0, 1.0),
            ],
            2,
            vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
        )
        .unwrap();
        BSplineCurve::from_nurb(c, 3, false)
    }

    /// An arc `lift` above its own UV arc on a four-cell plane (the arc crossing the cells'
    /// knot line inside its pieces): the gap is `lift` everywhere, and the bound is at least
    /// `lift` and above it by no more than the closing rule leaves (a piece closes once its
    /// bound is within a thousandth of its end values, `CLOSE_RELATIVE`). Held to a tolerance above it, the bound is at least
    /// `lift` and at most that tolerance.
    #[test]
    fn an_arc_over_its_uv_arc_is_bounded_by_its_height() {
        let k = Kernel::default();
        let s = plane();
        let lift = 3.0e-7;
        let curve = half_circle(0.45, 0.45, lift);
        let uv = half_circle(0.45, 0.45, 0.0);
        let g = trim_gap_bound(&k, &curve, &uv, &s, [0.0, 1.0], 1e-12, 0.0)
            .unwrap()
            .unwrap();
        assert!(g >= lift && g <= lift * (1.0 + 1.1e-3), "{g:e}");
        let g = trim_gap_bound(&k, &curve, &uv, &s, [0.0, 1.0], 1e-12, 1e-5)
            .unwrap()
            .unwrap();
        assert!(g >= lift && g <= 1e-5, "{g:e}");
    }

    /// A UV arc moved 1e-4 along u from its curve's shadow: the gap is 1e-4 everywhere, and
    /// held to a tolerance of 1e-5 the bound still says it is over (at least 1e-4).
    #[test]
    fn a_trim_off_its_edge_is_bounded_above_the_tolerance() {
        let k = Kernel::default();
        let s = plane();
        let curve = half_circle(0.45, 0.45, 0.0);
        let uv = half_circle(0.45 + 1e-4, 0.45, 0.0);
        let g = trim_gap_bound(&k, &curve, &uv, &s, [0.0, 1.0], 1e-9, 1e-5)
            .unwrap()
            .unwrap();
        assert!(
            (1e-4 * (1.0 - 1e-12)..=1e-4 * (1.0 + 1e-3)).contains(&g),
            "{g:e}"
        );
        // (over a part of the interval as well)
        let g = trim_gap_bound(&k, &curve, &uv, &s, [0.3, 0.7], 1e-9, 1e-5)
            .unwrap()
            .unwrap();
        assert!(g >= 1e-4 * (1.0 - 1e-12), "{g:e}");
    }

    /// A line `lift` above its UV line across the four cells' knot lines (each cell bounded
    /// over the part of the piece the UV line spends in it), with the UV line also given as
    /// a rational line (weights 1 and 2, the same points at other parameters, so the curve
    /// is its image, rational the same way): at least `lift` and above it by no more than
    /// the closing rule leaves.
    #[test]
    fn a_line_across_knot_lines_is_bounded_by_its_height() {
        let k = Kernel::default();
        let s = plane();
        let lift = 2.0e-7;
        let line = |a: [f64; 4], b: [f64; 4]| {
            let c = NurbCurve::new(
                vec![
                    CPoint::new(a[0] * a[3], a[1] * a[3], a[2] * a[3], a[3]),
                    CPoint::new(b[0] * b[3], b[1] * b[3], b[2] * b[3], b[3]),
                ],
                1,
                vec![0.0, 0.0, 1.0, 1.0],
            )
            .unwrap();
            BSplineCurve::from_nurb(c, 3, false)
        };
        for w1 in [1.0, 2.0] {
            let curve = line([0.2, 0.3, lift, 1.0], [0.9, 0.8, lift, w1]);
            let uv = line([0.2, 0.3, 0.0, 1.0], [0.9, 0.8, 0.0, w1]);
            for accept in [0.0, 1e-5] {
                let g = trim_gap_bound(&k, &curve, &uv, &s, [0.0, 1.0], 1e-12, accept)
                    .unwrap()
                    .unwrap();
                let most = (lift * (1.0 + 1.1e-3)).max(accept);
                assert!(g >= lift && g <= most, "{w1} {accept:e}: {g:e}");
            }
        }
    }

    /// `shifted` re-centres a polynomial: `(x + 2)^2 = x^2 + 4 x + 4`.
    #[test]
    fn shifted_recentres_a_polynomial() {
        let c = shifted(&[zero(), zero(), Ivl::from(1.0)], 2.0);
        let got: Vec<f64> = c.iter().map(|x| x.mid()).collect();
        assert_eq!(got, vec![4.0, 4.0, 1.0]);
    }

    /// A curve with a corner inside the interval (a spike to 1e-3 between two points on the
    /// plane) is cut there and seen, whatever the tolerance it is held to.
    #[test]
    fn a_spike_between_knots_is_seen() {
        let k = Kernel::default();
        let s = plane();
        let at = |t: f64| CPoint::new(0.2 + 0.5 * t, 0.3 + 0.3 * t, 0.0, NOW);
        let mut spike = at(1.0 / 64.0);
        spike.z = 1e-3;
        let c = NurbCurve::new(
            vec![at(0.0), spike, at(1.0 / 32.0), at(1.0)],
            1,
            vec![0.0, 0.0, 1.0 / 64.0, 1.0 / 32.0, 1.0, 1.0],
        )
        .unwrap();
        let spiked = BSplineCurve::from_nurb(c, 3, false);
        let w = NurbCurve::new(vec![at(0.0), at(1.0)], 1, vec![0.0, 0.0, 1.0, 1.0]).unwrap();
        let uv = BSplineCurve::from_nurb(w, 3, false);
        let g = trim_gap_bound(&k, &spiked, &uv, &s, [0.0, 1.0], 1e-9, 1e-5)
            .unwrap()
            .unwrap();
        assert!(g >= 1e-3, "{g:e}");
    }

    /// An empty, reversed, not finite or out-of-domain range encloses nothing; a cancelled
    /// kernel stops the bisection.
    #[test]
    fn a_range_it_cannot_take_encloses_nothing() {
        let k = Kernel::default();
        let s = plane();
        let curve = half_circle(0.45, 0.45, 0.0);
        let uv = half_circle(0.45, 0.45, 0.0);
        for range in [[0.5, 0.5], [0.7, 0.3], [f64::NAN, 1.0], [0.0, 1.5]] {
            let g = trim_gap_bound(&k, &curve, &uv, &s, range, 1e-9, 1e-5).unwrap();
            assert_eq!(g, None, "{range:?}");
        }
        let token = crate::kernel::CancelToken::default();
        token.cancel();
        let cancelled = Kernel {
            cancel: Some(token),
            ..Kernel::default()
        };
        let stopped = trim_gap_bound(&cancelled, &curve, &uv, &s, [0.0, 1.0], 1e-9, 1e-5);
        assert!(
            matches!(&stopped, Err(e) if e.is_cancelled()),
            "{stopped:?}"
        );
    }
}
