//! A proved bound on how far the surface's image of a UV curve runs from a curve
//! (`sup_s dist(S(W(s)), C)`), from the exact polynomials of each piece (not upstream).
//!
//! For any map `φ` of the UV curve's parameter to the curve's, `dist(S(W(s)), C) ≤
//! |C(φ(s)) − S(W(s))|`; [`trim_distance_bound`] bounds the right side with `φ` affine on
//! each piece (`C(φ(t))` is then a polynomial in `t` on a piece inside one knot span of
//! `C`, its Taylor coefficients those of `C` scaled by powers of `φ'`), and below `C` stands
//! for `C ∘ φ`.
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
//! bisected.
//!
//! | Rust                      | Upstream |
//! |---------------------------|----------|
//! | [`trim_distance_bound`]   | none     |
//!
//! Deviations: none (new code). The bisection is bounded by `MAX_PIECES` and `MAX_DEPTH`
//! of [`super`], and polls cancellation at every piece.

use super::{NetSurface, CLOSE_ABSOLUTE, CLOSE_RELATIVE, MAX_DEPTH, MAX_PIECES, OP};
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
/// is bisected instead.
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
    map: Option<Reparam>,
) -> Option<(f64, f64)> {
    let m = 0.5 * (a + b);
    let (da, db) = (Ivl::from(a) - Ivl::from(m), Ivl::from(b) - Ivl::from(m));
    let rho = da.mag().max(db.mag());
    let within_at = |n: &NurbsCurve, x: f64| {
        let k = span(&n.u, n.p, n.n, x);
        (knot(&n.u, k as isize) <= a && b <= knot(&n.u, k as isize + 1)).then_some(k)
    };
    let within = |n: &NurbsCurve| within_at(n, m);
    // (a piece a ulp wide, its middle rounded onto its end `b`, is its span's from `a`)
    let kw = match map {
        None => within(wn)?,
        Some(_) => within(wn).or_else(|| within_at(wn, a))?,
    };
    let c = match map {
        None => curve_taylor(cn, within(cn)?, m),
        Some(r) => {
            // `C(φ(t))`, `φ(t) = at + slope δ`: the span's Taylor coefficients about `at`
            // times `slope^j`, for `φ`'s values over the piece (enclosed) inside one knot
            // span of `C` and inside `[lo, hi]`
            let slope = Ivl::from(r.slope);
            // (a constant map's one value exactly: the sum would round it out a ulp)
            let image = if r.slope == 0.0 {
                Ivl::from(r.at)
            } else {
                (Ivl::from(r.at) + slope * da).hull(Ivl::from(r.at) + slope * db)
            };
            let k = span(&cn.u, cn.p, cn.n, r.at);
            let inside = knot(&cn.u, k as isize) <= image.lo
                && image.hi <= knot(&cn.u, k as isize + 1)
                && r.lo <= image.lo
                && image.hi <= r.hi;
            if !(inside && image.is_finite()) {
                return None;
            }
            let mut c = curve_taylor(cn, k, r.at);
            for comp in &mut c {
                let mut power = Ivl::from(1.0);
                for coefficient in comp.iter_mut() {
                    *coefficient = *coefficient * power;
                    power = power * slope;
                }
            }
            c
        }
    };
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

/// The NURBS data of `c` exactly as stored: its homogeneous control points `(x w, y w, z w, w)`
/// taken as they are (a non-rational point's weight `1`, a 2D point's `z` `0`), so the curve
/// enclosed is the curve evaluated. (`NurbsCurve::of` rebuilds them from the Euclidean
/// points, which rounds: a stored `x w = 3` with `w = 10` comes back as `3.0000000000000004`.)
/// `None` when the degree, knots and points do not agree or a weight is not positive.
fn stored_nurbs(c: &BSplineCurve) -> Option<NurbsCurve> {
    use crate::nurbs::math::{NOW, NOZ};
    let (knots, mult) = c.knots(None).ok()?;
    let p = c.degree().ok()?;
    let cps = c.control_points().ok()?;
    let n = cps.len();
    let mut u = Vec::new();
    for (&x, &m) in knots.iter().zip(&mult) {
        for _ in 0..m {
            u.push(x);
        }
    }
    if p < 1 || u.len() != n + p + 1 {
        return None;
    }
    let mut pw = Vec::new();
    for cp in cps {
        let w = if cp.w == NOW { 1.0 } else { cp.w };
        let z = if cp.z == NOZ { 0.0 } else { cp.z };
        if !(w > 0.0 && w.is_finite()) {
            return None;
        }
        pw.extend_from_slice(&[cp.x, cp.y, z, w]);
    }
    Some(NurbsCurve { p, n, u, pw })
}

/// An affine map `φ(t) = at + slope (t − m)` of a piece's parameter `t` (`m` its middle) to
/// the curve's, whose values over the piece lie in `[lo, hi]` ([`polynomial_piece`]).
#[derive(Clone, Copy, Debug)]
struct Reparam {
    /// `φ(m)`.
    at: f64,
    /// `φ'`.
    slope: f64,
    /// The least value `φ` may take.
    lo: f64,
    /// The greatest value `φ` may take.
    hi: f64,
}

/// `C(t)` and `C'(t)` of `cn` (plain floating point, not enclosed: a guess only), or `None`
/// where a weight vanishes.
fn point_and_tangent(cn: &NurbsCurve, t: f64) -> Option<([f64; 3], [f64; 3])> {
    let k = span(&cn.u, cn.p, cn.n, t);
    let ders = ders_basis(&cn.u, k, cn.p, t, 1);
    let mut a = [[0.0_f64; 4]; 2];
    for (d, row) in a.iter_mut().enumerate() {
        for i in 0..=cn.p {
            let n = ders.get(d).and_then(|r| r.get(i)).copied()?;
            let base = 4 * (k + i).checked_sub(cn.p)?;
            for (comp, x) in row.iter_mut().enumerate() {
                *x += n * cn.pw.get(base + comp).copied()?;
            }
        }
    }
    let [a0, a1] = a;
    if !(a0[3] != 0.0 && a0[3].is_finite()) {
        return None;
    }
    let point = [a0[0] / a0[3], a0[1] / a0[3], a0[2] / a0[3]];
    let tangent = [
        (a1[0] - point[0] * a1[3]) / a0[3],
        (a1[1] - point[1] * a1[3]) / a0[3],
        (a1[2] - point[2] * a1[3]) / a0[3],
    ];
    Some((point, tangent))
}

/// The parameter in `[lo, hi]` of a point of `cn` near `p`: Gauss–Newton steps from
/// `guess`, clamped, each halved until it brings the point nearer (so a step across a
/// corner, where `C'` is short, cannot carry it to another part of the curve). A guess
/// only, which [`trim_distance_bound`] needs no property of.
fn nearest_parameter(cn: &NurbsCurve, p: [f64; 3], guess: f64, [lo, hi]: [f64; 2]) -> f64 {
    let squared = |c: [f64; 3]| {
        let r = [c[0] - p[0], c[1] - p[1], c[2] - p[2]];
        r[0] * r[0] + r[1] * r[1] + r[2] * r[2]
    };
    let mut t = guess.max(lo).min(hi);
    let Some((mut c, mut d)) = point_and_tangent(cn, t) else {
        return t;
    };
    let mut now = squared(c);
    for _ in 0..NEWTON_STEPS {
        let dd = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
        if !(dd > 0.0 && dd.is_finite()) {
            break;
        }
        let r = [c[0] - p[0], c[1] - p[1], c[2] - p[2]];
        let mut step = (d[0] * r[0] + d[1] * r[1] + d[2] * r[2]) / dd;
        let mut moved = None;
        for _ in 0..HALVINGS {
            let next = (t - step).max(lo).min(hi);
            if !next.is_finite() || next == t {
                break;
            }
            if let Some((cn_, dn)) = point_and_tangent(cn, next) {
                let there = squared(cn_);
                if there < now {
                    moved = Some((next, cn_, dn, there));
                    break;
                }
            }
            step *= 0.5;
        }
        let Some((next, cn_, dn, there)) = moved else {
            break;
        };
        (t, c, d, now) = (next, cn_, dn, there);
    }
    t
}

/// The Gauss–Newton steps [`nearest_parameter`] takes at most.
const NEWTON_STEPS: usize = 16;

/// The halvings [`nearest_parameter`] tries a step with before it stops.
const HALVINGS: usize = 30;

/// The parameter `s` in `(a, b)` whose nearest point on `C` is at the knot `x`, as near as
/// a few false-position steps find it (`nearest(s, guess)` gives that point's parameter;
/// `ta < x < tb` are those of `a` and `b`, either way round): a guess only, so a piece cut
/// there has its two lines meet at a point of `C` near `S(W(s))` rather than at the
/// straight line's `s`, where the two parameterisations may be far apart. `None` where
/// `nearest` cannot evaluate the trim.
fn knot_preimage(
    nearest: &dyn Fn(f64, f64) -> Result<Option<f64>>,
    (a, ta): (f64, f64),
    (b, tb): (f64, f64),
    x: f64,
) -> Result<Option<f64>> {
    let (mut lo, mut flo) = (a, ta - x);
    let (mut hi, mut fhi) = (b, tb - x);
    let mut s = a + (x - ta) / (tb - ta) * (b - a);
    let mut side = 0_i8;
    for _ in 0..PREIMAGE_STEPS {
        if !(lo < s && s < hi) {
            s = 0.5 * (lo + hi);
        }
        let guess = ta + (s - a) / (b - a) * (tb - ta);
        let Some(t) = nearest(s, guess)? else {
            return Ok(None);
        };
        let f = t - x;
        if f == 0.0 {
            break;
        }
        // (Illinois: the end kept twice running has its value halved)
        if (f < 0.0) == (flo < 0.0) {
            (lo, flo) = (s, f);
            if side == -1 {
                fhi *= 0.5;
            }
            side = -1;
        } else {
            (hi, fhi) = (s, f);
            if side == 1 {
                flo *= 0.5;
            }
            side = 1;
        }
        let next = lo - flo * (hi - lo) / (fhi - flo);
        if !next.is_finite() || next == s {
            break;
        }
        s = next;
    }
    Ok(Some(s))
}

/// The false-position steps [`knot_preimage`] takes at most.
const PREIMAGE_STEPS: usize = 24;

/// A proved upper bound on `sup_s dist(S(W(s)), C([edge]))` over `s` in `trim`: how far the
/// surface's image of the UV curve `W` (over its own interval) runs from the curve `C` over
/// `edge`, whatever the two parameterisations.
///
/// For any map `φ` of `trim` into `edge`, `dist(S(W(s)), C([edge])) ≤ |C(φ(s)) − S(W(s))|`,
/// so each piece is bounded as [`polynomial_piece`] bounds it with `C` read through an
/// affine `φ` ([`Reparam`]): the line between the parameters of the points of `C` nearest
/// the piece's two ends (found by [`nearest_parameter`] from the normalised map
/// `edge.lo + (s − trim.lo) / |trim| |edge|`, or from the parent piece's line), pulled in by
/// a few ulps about its middle so its enclosed values stay inside one knot span of `C` and
/// inside `edge`. The first pieces end at every knot of `W` inside `trim`; a piece whose line
/// runs across a knot of `C` is cut where the nearest point is at that knot
/// ([`knot_preimage`]). The bound holds for whatever `φ` the guesses give; they decide only
/// how tight it is.
///
/// A piece whose bound is at most `accept` is closed at once, so a result at most `accept`
/// says only that (the distance is no larger). Any other piece is closed once its bound is
/// within `CLOSE_RELATIVE` of its end values (or within `floor` or `CLOSE_ABSOLUTE` of
/// them), and is bisected until then: above `accept` the result stands that close above
/// the largest `|C(φ(s)) − S(W(s))|` at the pieces' ends, each the distance to a point of
/// `C` the guesses found near.
///
/// `None` when the distance is not enclosed: a surface that is not a NURBS net or whose net
/// is not parameterised as the surface, an empty, unordered or not finite range or one past
/// either curve's domain, a `W` that cannot be evaluated, or a piece that does not close
/// before `MAX_DEPTH` or `MAX_PIECES` (its bound may stand far above the distance: a loose
/// bound is not returned). On the last piece the budget allows, the constant maps at its
/// two ends are tried beside the line.
///
/// # Errors
///
/// The kernel's unported and cancelled statuses.
#[allow(clippy::too_many_arguments)]
pub(crate) fn trim_distance_bound(
    k: &Kernel,
    curve: &BSplineCurve,
    edge: [f64; 2],
    uv: &BSplineCurve,
    trim: [f64; 2],
    surface: &Surface,
    floor: f64,
    accept: f64,
) -> Result<Option<f64>> {
    k.check_cancelled(OP)?;
    let ([elo, ehi], [lo, hi]) = (edge, trim);
    let ordered = |a: f64, b: f64| a.is_finite() && b.is_finite() && a < b;
    if !(ordered(elo, ehi) && ordered(lo, hi) && floor.is_finite() && floor >= 0.0) {
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
    // (a copy with a cache slot and NURB cells of its own: a NURB or cache the bound makes
    // stays on the copy, so measuring leaves the B-rep's surface as it was)
    let mut surface = surface.clone_for_evaluation()?;
    surface.detach_for_edit();
    let Some(net) = NetSurface::of(k, &surface, &probe)? else {
        return Ok(None);
    };
    let (Some(cn), Some(wn)) = (stored_nurbs(curve), stored_nurbs(uv)) else {
        return Ok(None);
    };
    let within = |n: &NurbsCurve, a: f64, b: f64| match (n.u.get(n.p), n.u.get(n.n)) {
        (Some(&d0), Some(&d1)) => d0 <= a && b <= d1,
        _ => false,
    };
    if !(within(&cn, elo, ehi) && within(&wn, lo, hi)) {
        return Ok(None);
    }
    // the parameter on `C` of the point nearest `S(W(s))`, from `guess`
    let nearest = |s: f64, guess: f64| -> Result<Option<f64>> {
        let Ok(w) = uv.evaluate_point(s) else {
            return Ok(None);
        };
        let Ok(p) = surface.evaluate_point(k, Point2::new(w.x, w.y)) else {
            return Ok(None);
        };
        Ok(Some(nearest_parameter(&cn, [p.x, p.y, p.z], guess, edge)))
    };
    // `|C(t) − S(W(s))|` in plain floating point (`None` where either is not evaluated)
    let gap_at = |s: f64, t: f64| -> Result<Option<f64>> {
        let Ok(w) = uv.evaluate_point(s) else {
            return Ok(None);
        };
        let Ok(p) = surface.evaluate_point(k, Point2::new(w.x, w.y)) else {
            return Ok(None);
        };
        Ok(point_and_tangent(&cn, t).map(|(c, _)| {
            let d = [c[0] - p.x, c[1] - p.y, c[2] - p.z];
            crate::geom::vector3d::sqrt(d[0] * d[0] + d[1] * d[1] + d[2] * d[2])
        }))
    };
    let normalised = |s: f64| elo + (s - lo) / (hi - lo) * (ehi - elo);
    let mut cuts: Vec<f64> = wn.u.iter().copied().filter(|&x| lo < x && x < hi).collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    if cuts.len() + 1 > MAX_PIECES {
        return Ok(None);
    }
    let mut ends = vec![lo];
    ends.extend(cuts);
    ends.push(hi);
    let mut images = Vec::new();
    for &s in &ends {
        let Some(t) = nearest(s, normalised(s))? else {
            return Ok(None);
        };
        images.push(t);
    }
    let mut stack: Vec<(f64, f64, f64, f64, u32)> = ends
        .windows(2)
        .zip(images.windows(2))
        .filter_map(|pair| match pair {
            ([a, b], [ta, tb]) => Some((*a, *b, *ta, *tb, 0)),
            _ => None,
        })
        .rev()
        .collect();
    let mut worst = 0.0_f64;
    let mut made = stack.len();
    while let Some((a, b, ta, tb, depth)) = stack.pop() {
        k.check_cancelled(OP)?;
        let mid = 0.5 * (a + b);
        let last = depth >= MAX_DEPTH || !(a < mid && mid < b) || made + 2 > MAX_PIECES;
        // a knot of `C` strictly inside the line's values: cut the piece there
        let (t0, t1) = (ta.min(tb), ta.max(tb));
        let knot_inside = cn.u.iter().copied().find(|&x| t0 < x && x < t1);
        if let (Some(x), false) = (knot_inside, last) {
            let Some(s) = knot_preimage(&nearest, (a, ta), (b, tb), x)? else {
                return Ok(None);
            };
            if a < s && s < b {
                // (a cut at a knot is a first cut, as `W`'s knots are: it does not deepen
                // the piece, and each side has fewer of `C`'s knots inside)
                made += 1;
                stack.push((s, b, x, tb, depth));
                stack.push((a, s, ta, x, depth));
                continue;
            }
        }
        // the line through `(a, ta)` and `(b, tb)`, flattened about its middle by a few ulps
        // so its enclosed values stay inside `[ta, tb]` (a knot span of `C`, inside `edge`)
        let at = 0.5 * (ta + tb);
        let half = 0.5 * (tb - ta).abs() - MARGIN_ULPS * f64::EPSILON * at.abs();
        let slope = if half > 0.0 {
            (half / (0.5 * (b - a))).copysign(tb - ta)
        } else {
            0.0
        };
        let map = Reparam {
            at,
            slope,
            lo: elo,
            hi: ehi,
        };
        // (a piece a ulp wide the line is refused for, its values a few ulps across a knot
        // of `C`, is read through the constant map at the line's middle: any map holds)
        // the line, and on a piece the budget would end (or a piece a ulp wide, whose line
        // may be refused for values a few ulps across a knot of `C`) the constant maps at
        // its ends too, the smallest bound kept: any map holds, and where the nearest points
        // jump along `C` over a short piece a constant one is the tighter
        let mut maps = vec![map];
        if last {
            for t in [ta, tb] {
                maps.push(Reparam {
                    at: t,
                    slope: 0.0,
                    ..map
                });
            }
        }
        let bounded = maps
            .into_iter()
            .filter_map(|r| polynomial_piece(&cn, &wn, &net, a, b, Some(r)))
            .min_by(|x, y| x.1.total_cmp(&y.1));
        match bounded {
            Some((_, bound)) if !bound.is_finite() => return Ok(None),
            Some((ends, bound)) => {
                // (the piece's end values, enclosed by a cell alone or taken at 0 on a knot
                // line, with the gaps at its ends in plain floating point: the scale only
                // decides when a piece is closed, never the bound)
                let mut scale = ends;
                for (s, t) in [(a, ta), (b, tb)] {
                    if let Some(g) = gap_at(s, t)? {
                        scale = scale.max(g);
                    }
                }
                let term = (Ivl::from(bound) - Ivl::from(scale)).hi;
                if bound <= accept
                    || term <= (CLOSE_RELATIVE * scale).max(floor).max(CLOSE_ABSOLUTE)
                {
                    worst = worst.max(bound);
                    continue;
                }
                // (a piece the budget ends before its bound closes on its end values says
                // nothing of the distance but that it is below a loose bound: not enclosed)
                if last {
                    return Ok(None);
                }
            }
            None => {
                if last {
                    return Ok(None);
                }
            }
        }
        let Some(tm) = nearest(mid, 0.5 * (ta + tb))? else {
            return Ok(None);
        };
        made += 2;
        stack.push((mid, b, tm, tb, depth + 1));
        stack.push((a, mid, ta, tm, depth + 1));
    }
    Ok(worst.is_finite().then_some(worst))
}

/// The ulps (of its middle) [`trim_distance_bound`] pulls each end of a piece's line in by,
/// so the line's values, enclosed with outward rounding, stay inside the knot span (and the
/// edge interval) its two ends bound.
const MARGIN_ULPS: f64 = 16.0;

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

    /// [`trim_distance_bound`] of `uv`'s image over `trim` from `curve` over `[0, 1]`.
    fn distance(
        k: &Kernel,
        curve: &BSplineCurve,
        uv: &BSplineCurve,
        s: &Surface,
        trim: [f64; 2],
        floor: f64,
        accept: f64,
    ) -> Result<Option<f64>> {
        trim_distance_bound(k, curve, [0.0, 1.0], uv, trim, s, floor, accept)
    }

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
        let g = distance(&k, &curve, &uv, &s, [0.0, 1.0], 1e-12, 0.0)
            .unwrap()
            .unwrap();
        assert!(g >= lift && g <= lift * (1.0 + 1.1e-3), "{g:e}");
        let g = distance(&k, &curve, &uv, &s, [0.0, 1.0], 1e-12, 1e-5)
            .unwrap()
            .unwrap();
        assert!(g >= lift && g <= 1e-5, "{g:e}");
    }

    /// A UV arc moved 1e-4 along u from its curve's shadow (the half circle of radius `r`):
    /// the image's distance from the arc at angle `θ` is `|sqrt(r² + 2 r d cos θ + d²) − r|`,
    /// at most `d` = 1e-4 (at both ends), and held to a tolerance of 1e-5 the bound still
    /// says it is over (at least 1e-4).
    #[test]
    fn a_trim_off_its_edge_is_bounded_above_the_tolerance() {
        let k = Kernel::default();
        let s = plane();
        let curve = half_circle(0.45, 0.45, 0.0);
        let uv = half_circle(0.45 + 1e-4, 0.45, 0.0);
        let g = distance(&k, &curve, &uv, &s, [0.0, 1.0], 1e-9, 1e-5)
            .unwrap()
            .unwrap();
        assert!(
            (1e-4 * (1.0 - 1e-12)..=1e-4 * (1.0 + 1e-3)).contains(&g),
            "{g:e}"
        );
        // (over a part of the trim as well: there the image of each end is off the arc
        // radially, by `| |P − c| − r |` in closed form, and the bound is at least that)
        let g = distance(&k, &curve, &uv, &s, [0.3, 0.7], 1e-9, 1e-5)
            .unwrap()
            .unwrap();
        for t in [0.3, 0.7] {
            let p = uv.evaluate_point(t).unwrap();
            let off = ((p.x - 0.45).hypot(p.y - 0.45) - 0.4).abs();
            assert!(g >= off * (1.0 - 1e-9) && off > 1e-6, "{t}: {g:e} {off:e}");
        }
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
                let g = distance(&k, &curve, &uv, &s, [0.0, 1.0], 1e-12, accept)
                    .unwrap()
                    .unwrap();
                let most = (lift * (1.0 + 1.1e-3)).max(accept);
                assert!(g >= lift && g <= most, "{w1} {accept:e}: {g:e}");
            }
        }
    }

    /// The ramp z = x over x, y in 0..1 as a bilinear net (one cell); a B-spline surface does
    /// not continue past its net, so a box past its end takes `NetSurface::partials`' slack.
    fn ramp() -> Surface {
        let mut points = Vec::new();
        for i in 0..2 {
            for j in 0..2 {
                let x = f64::from(i);
                points.push(Point3::new(x, f64::from(j), x));
            }
        }
        let net = BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: 1,
            v_degree: 1,
            control_points: &points,
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &[2, 2],
            v_multiplicities: &[2, 2],
            u_knots: &[0.0, 1.0],
            v_knots: &[0.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights: Some(&[1.0; 4]),
            uv_domain: None,
        })
        .unwrap();
        Surface::from(net)
    }

    /// A UV line running a little past the ramp's end u = 1 (a seam's parameters a ulp-scale
    /// out, within what `NetSurface::of`'s probe takes for the surface's own point), its
    /// curve 1e-6 above the ramp's line `(u, 0.5, u)`: the continued end cell and
    /// `NetSurface::partials`' slack enclose it, and the bound is at least the distance in
    /// closed form: the trim's start `(0.5, 0.5, 0.5)` is nearest the edge's start
    /// `(0.5, 0.5, 0.5 + 1e-6)` (its foot on the edge's line falls before the edge), `1e-6`
    /// away, and every later point is within that (`1e-6 / sqrt(2)` from the line where
    /// its foot is on the edge). A line far past the end is refused.
    #[test]
    fn a_trim_past_the_net_end_takes_the_slack() {
        let k = Kernel::default();
        let s = ramp();
        let line = |a: [f64; 3], b: [f64; 3]| {
            let c = NurbCurve::new(
                vec![
                    CPoint::new(a[0], a[1], a[2], NOW),
                    CPoint::new(b[0], b[1], b[2], NOW),
                ],
                1,
                vec![0.0, 0.0, 1.0, 1.0],
            )
            .unwrap();
            BSplineCurve::from_nurb(c, 3, false)
        };
        let end = 1.0 + 1e-10;
        let h = 1e-6;
        let truth = h;
        let curve = line([0.5, 0.5, 0.5 + h], [end, 0.5, end + h]);
        let uv = line([0.5, 0.5, 0.0], [end, 0.5, 0.0]);
        for accept in [0.0, 1e-5] {
            let g = distance(&k, &curve, &uv, &s, [0.0, 1.0], 1e-14, accept)
                .unwrap()
                .unwrap();
            assert!(g >= truth * (1.0 - 1e-9), "{accept:e}: {g:e} < {truth:e}");
            assert!(g <= truth * 1.01 || g <= accept, "{accept:e}: {g:e}");
        }
        let far = 1.1;
        let curve = line([0.5, 0.5, 0.5], [far, 0.5, far]);
        let uv = line([0.5, 0.5, 0.0], [far, 0.5, 0.0]);
        let g = distance(&k, &curve, &uv, &s, [0.0, 1.0], 1e-9, 1e-5).unwrap();
        assert_eq!(g, None);
    }

    /// A trim with another parameterisation than its edge (the same arc, its parameter run
    /// as `t²`, here a quadratic B-spline UV curve along the arc's line): far apart at equal
    /// parameters, on the edge geometrically. The same-parameter gap is large, the distance
    /// is bounded at the height it sits above the plane.
    #[test]
    fn a_trim_with_its_own_parameterisation_is_bounded_by_its_distance() {
        let k = Kernel::default();
        let s = plane();
        let lift = 1e-7;
        let at = |x: f64, y: f64, z: f64| CPoint::new(x, y, z, NOW);
        let c = NurbCurve::new(
            vec![at(0.2, 0.3, lift), at(0.9, 0.8, lift)],
            1,
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        let curve = BSplineCurve::from_nurb(c, 3, false);
        // W(s) = A + (B − A) s² (a quadratic Bezier with the middle point at A)
        let w = NurbCurve::new(
            vec![at(0.2, 0.3, 0.0), at(0.2, 0.3, 0.0), at(0.9, 0.8, 0.0)],
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        )
        .unwrap();
        let uv = BSplineCurve::from_nurb(w, 3, false);
        let same = {
            let p = uv.evaluate_point(0.5).unwrap();
            let c = curve.evaluate_point(0.5).unwrap();
            (p.x - c.x).hypot(p.y - c.y)
        };
        assert!(same > 0.1, "{same:e}");
        for accept in [0.0, 1e-5] {
            let g = distance(&k, &curve, &uv, &s, [0.0, 1.0], 1e-12, accept)
                .unwrap()
                .unwrap();
            let most = (lift * (1.0 + 1.1e-3)).max(accept);
            assert!(g >= lift && g <= most, "{accept:e}: {g:e}");
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
    /// plane, the trim the straight line under it) is seen, whatever the tolerance it is
    /// held to.
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
        let g = distance(&k, &spiked, &uv, &s, [0.0, 1.0], 1e-9, 1e-5)
            .unwrap()
            .unwrap();
        // the trim's point under the spike is off both of the spike's legs by `L h /
        // sqrt(L² + h²)` (`L` the leg's run on the plane, `h` its rise), in closed form
        let run = (0.5_f64 / 64.0).hypot(0.3 / 64.0);
        let off = run * 1e-3 / run.hypot(1e-3);
        assert!(g >= off * (1.0 - 1e-9), "{g:e} {off:e}");
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
            let g = distance(&k, &curve, &uv, &s, range, 1e-9, 1e-5).unwrap();
            assert_eq!(g, None, "{range:?}");
            let g = trim_distance_bound(&k, &curve, range, &uv, [0.0, 1.0], &s, 1e-9, 1e-5);
            assert_eq!(g.unwrap(), None, "edge {range:?}");
        }
        let token = crate::kernel::CancelToken::default();
        token.cancel();
        let cancelled = Kernel {
            cancel: Some(token),
            ..Kernel::default()
        };
        let stopped = distance(&cancelled, &curve, &uv, &s, [0.0, 1.0], 1e-9, 1e-5);
        assert!(
            matches!(&stopped, Err(e) if e.is_cancelled()),
            "{stopped:?}"
        );
        // (a cancelled kernel stops the trim bound before any range it would refuse)
        for range in [[0.0, 1.0], [0.7, 0.3], [f64::NAN, 1.0]] {
            let stopped =
                trim_distance_bound(&cancelled, &curve, range, &uv, [0.0, 1.0], &s, 1e-9, 1e-5);
            assert!(
                matches!(&stopped, Err(e) if e.is_cancelled()),
                "{range:?} {stopped:?}"
            );
        }
    }
}
