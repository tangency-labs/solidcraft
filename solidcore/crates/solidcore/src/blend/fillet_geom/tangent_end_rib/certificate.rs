//! The certificate of a tangent-end rib between its stations (not upstream, #17744).
//!
//! The rib is its definition: at the curves' parameter `t` the section `sigma(t)` of the
//! fillet surface S meets the end surface E, `F(t, y) = S(sigma(t), alpha) − E(u, v) = 0` in
//! the unknowns `y = (alpha, u, v)`. Over a piece `T` of a span, Krawczyk's test
//! (`crate::geom::krawczyk`) on `z = y − c(t)`, `c` the quadratic through the Newton roots at
//! the piece's ends and middle, proves for every `t` in `T` exactly one root `y(t)` in
//! `c(t) + Z`: `F(t, c(t))` is enclosed by its mean-value form about the middle and the
//! partials over `T x (c(T) + Z)`. At a piece's end, a point, the root is in a box of
//! rounding's width. Each of the rib's three curves (3D; on the fillet surface's UV; on
//! the end surface's UV) has an error `e` against the root: its 3D point less the rib's
//! point, its UV point less `(sigma, alpha)` or `(u, v)`. At the piece's ends `e` is
//! enclosed from those small boxes; between them `|e(t)|` is at most the larger end's plus
//! `h²/8 max |e''|` (the linear interpolation's remainder), with `e''` enclosed over the
//! piece: the curves' second derivatives over `T`, the root's `y'` and `y''` from
//! differentiating `F = 0` twice, each solved over the box by the preconditioned form
//! `x ∈ C b + (I − C J) x`. A UV curve's error maps to 3D through its surface's first
//! partials over the box both points lie in. Every operation rounds outward.
//!
//! A span is cut into 1, 2, 4, ... up to `2^LEVELS` pieces until every piece is bounded
//! within the tolerance (as `fillet_section::certificate::gn_certify_span` cuts its spans);
//! a piece end already farther than the tolerance from the rib ends the search, since no
//! finer cut can bound the span within it.
//!
//! Each face is enclosed as its NURBS data over every polynomial piece the box covers,
//! hulled: across a knot line inside the box only where the face is at least `C^1` (the
//! remainder needs `e'` continuous), and past the knots on its end piece continued, as the
//! face is evaluated there ([`crate::blend::fillet_section::station::Nurbs`]).

use crate::blend::fillet_section::certificate::{
    curve_derivs_over, knot_cuts, nurbs_partials_over_piece, nurbs_partials_over_piece_about,
    partials_hull, NurbsCurve,
};
use crate::blend::fillet_section::interval::{cmax, Ivl};
use crate::blend::fillet_section::series::{v_add, v_scale, v_sub, V3};
use crate::blend::fillet_section::station::{knot, Nurbs, Partials};
use crate::error::Result;
use crate::geom::krawczyk::{contraction, inverse_of_columns3, krawczyk, plus_c_times};

/// The continuity a face needs across a knot line inside a box for its second partials:
/// `C^1`, so that the root and the errors are `C^1` and the remainder holds with `e''`
/// enclosed on each side. Its points and first partials are enclosed across any line
/// (Krawczyk's mean-value form and the centred form need them continuous only).
const CONTINUITY: usize = 1;

/// The most times a piece of a span is halved.
const LEVELS: u32 = 6;

/// The most pieces the first span is graded into toward the tangent end.
const GRADES: usize = 64;

const ZERO: Ivl = Ivl::new(0.0, 0.0);

/// A Newton root of the rib `(alpha, u, v)` at `t` from a guess, or `None`.
pub(super) type Solve<'a> = dyn Fn(f64, [f64; 3]) -> Result<Option<[f64; 3]>> + 'a;

/// The parts of `[lo, hi]` on the pieces of a knot vector: inside the knots cut at the
/// knot lines strictly inside ([`knot_cuts`]), past them the end piece continued.
fn cuts(
    knots: &[f64],
    p: usize,
    n: usize,
    lo: f64,
    hi: f64,
    continuity: usize,
) -> Option<Vec<(f64, f64, usize)>> {
    let first = knot(knots, isize::try_from(p).ok()?);
    let last = knot(knots, isize::try_from(n).ok()?);
    let mut out = Vec::new();
    if lo < first {
        out.push((lo, hi.min(first), p));
    }
    let (a, b) = (lo.max(first), hi.min(last));
    if a <= b {
        out.extend(knot_cuts(knots, p, n, a, b, continuity).ok()?);
    }
    if hi > last {
        out.push((lo.max(last), hi, n.checked_sub(1)?));
    }
    Some(out)
}

/// A face's partials `P[a][b]`, `a + b <= lk`, over `u x v`, enclosed: the hull over the
/// pieces the box covers, each of `S − o` with `o` the face's point at the box's middle
/// ([`nurbs_partials_over_piece_about`]: a rational face's quotient then scales with the
/// box's size, not with its distance from the origin), `o` added back to `P[0][0]`. `None`
/// where a part is not enclosed.
fn partials_over(n: &Nurbs, u: Ivl, v: Ivl, lk: usize) -> Option<Partials<Ivl>> {
    let continuity = if lk >= 2 { CONTINUITY } else { 0 };
    let us = cuts(&n.u, n.p, n.nu, u.lo, u.hi, continuity)?;
    let vs = cuts(&n.v, n.q, n.nv, v.lo, v.hi, continuity)?;
    let (um, vm) = (u.mid(), v.mid());
    let (&(_, _, ku), &(_, _, kv)) = (
        us.iter().find(|c| c.0 <= um && um <= c.1)?,
        vs.iter().find(|c| c.0 <= vm && vm <= c.1)?,
    );
    let o = nurbs_partials_over_piece(n, ku, kv, um, um, vm, vm, 0)
        .ok()?
        .at(0, 0);
    let o = [o.x.mid(), o.y.mid(), o.z.mid()];
    let mut out: Option<Partials<Ivl>> = None;
    for &(a0, a1, ku) in &us {
        for &(b0, b1, kv) in &vs {
            let p = nurbs_partials_over_piece_about(n, ku, kv, a0, a1, b0, b1, lk, o).ok()?;
            out = Some(match out {
                None => p,
                Some(h) => partials_hull(h, &p, lk),
            });
        }
    }
    let mut out = out?;
    let [x, y, z] = o.map(Ivl::from);
    out.set(0, 0, v_add(out.at(0, 0), V3 { x, y, z }));
    Some(out)
}

fn neg(a: V3<Ivl>) -> V3<Ivl> {
    v_scale(a, Ivl::from(-1.0))
}

fn arr(a: V3<Ivl>) -> [Ivl; 3] {
    [a.x, a.y, a.z]
}

/// `|a|`, rounded up.
fn norm(a: V3<Ivl>) -> f64 {
    let sq = |x: Ivl| Ivl::from(x.mag()) * Ivl::from(x.mag());
    (sq(a.x) + sq(a.y) + sq(a.z)).sqrt().hi
}

/// The solutions `x` of `J x = b` for every matrix of the columns `cols` and every `b`,
/// enclosed: with `C` the inverse of `J`'s middle and `q = ||I − C J|| < 1`,
/// `|x| <= |C b| / (1 − q)` and `x ∈ C b + (I − C J) x`. `None` where `q >= 1`.
fn solve_over(cols: [V3<Ivl>; 3], b: V3<Ivl>) -> Option<[Ivl; 3]> {
    let mid = cols.map(|c| [c.x.mid(), c.y.mid(), c.z.mid()]);
    let c = inverse_of_columns3(&mid)?;
    let cols = cols.map(arr);
    let rows: [[Ivl; 3]; 3] =
        core::array::from_fn(|r| cols.map(|col| col.get(r).copied().unwrap_or(ZERO)));
    let (cm, q) = contraction(&c, &rows);
    (q < 1.0).then_some(())?;
    let b = arr(b);
    let cb = c.map(|row| {
        row.iter()
            .zip(&b)
            .fold(ZERO, |s, (&r, &x)| s + Ivl::from(r) * x)
    });
    let size = cb.iter().fold(0.0_f64, |m, x| cmax(m, x.mag()));
    let r = (Ivl::from(size) / (Ivl::from(1.0) - Ivl::from(q))).hi;
    if !r.is_finite() {
        return None;
    }
    Some(plus_c_times(cb, &cm, &[Ivl::new(-r, r); 3]))
}

/// The rib's definition and its three curves, for the certificate of its spans.
pub(super) struct Rib<'a> {
    /// The fillet surface's NURBS data.
    pub fillet: &'a Nurbs,
    /// The end surface's NURBS data.
    pub end: &'a Nurbs,
    /// The fillet surface's section parameter: 0 u, 1 v.
    pub section: usize,
    /// `sigma(t) = sigma0 + d_sigma t^m`, `m` 2 for an odd contact order, else 1.
    pub sigma0: f64,
    /// See `sigma0`.
    pub d_sigma: f64,
    /// Whether the contact order is odd.
    pub odd: bool,
    /// The contact order.
    pub order: u32,
    /// The 3D curve, the fillet surface's UV curve and the end surface's, in `t`.
    pub curves: [NurbsCurve; 3],
}

/// A piece end: its parameter, the Newton root there and the errors' magnitudes (3D x, y,
/// z; fillet sigma, alpha; end u, v).
#[derive(Clone, Copy)]
struct End {
    t: f64,
    y: [f64; 3],
    e: [f64; 7],
}

impl Rib<'_> {
    fn sigma(&self, t: Ivl) -> (Ivl, Ivl, Ivl) {
        let d = Ivl::from(self.d_sigma);
        if self.odd {
            let two = Ivl::from(2.0);
            (Ivl::from(self.sigma0) + d * (t * t), two * d * t, two * d)
        } else {
            (Ivl::from(self.sigma0) + d * t, d, ZERO)
        }
    }

    /// The fillet surface's partials over `sigma x alpha`, and an accessor of
    /// `d^(i+j) S / dsigma^i dalpha^j`.
    fn fillet_over(
        &self,
        sigma: Ivl,
        alpha: Ivl,
        lk: usize,
    ) -> Option<impl Fn(usize, usize) -> V3<Ivl>> {
        let section = self.section;
        let p = if section == 0 {
            partials_over(self.fillet, sigma, alpha, lk)?
        } else {
            partials_over(self.fillet, alpha, sigma, lk)?
        };
        Some(move |i, j| if section == 0 { p.at(i, j) } else { p.at(j, i) })
    }

    fn value(&self, t: Ivl, y: [Ivl; 3]) -> Option<[Ivl; 3]> {
        let [a, u, v] = y;
        let s = self.fillet_over(self.sigma(t).0, a, 0)?;
        let e = partials_over(self.end, u, v, 0)?;
        Some(arr(v_sub(s(0, 0), e.at(0, 0))))
    }

    fn columns(&self, t: Ivl, y: [Ivl; 3]) -> Option<[[Ivl; 3]; 3]> {
        let [a, u, v] = y;
        let s = self.fillet_over(self.sigma(t).0, a, 1)?;
        let e = partials_over(self.end, u, v, 1)?;
        Some([arr(s(0, 1)), arr(neg(e.at(1, 0))), arr(neg(e.at(0, 1)))])
    }

    /// The box about `mid` that Krawczyk's test proves to hold the one root at `t`, or
    /// `None`.
    fn root_at(&self, t: f64, mid: [f64; 3]) -> Result<Option<[Ivl; 3]>> {
        let t = Ivl::from(t);
        let Some(j) = self.columns(t, mid.map(Ivl::from)) else {
            return Ok(None);
        };
        let Some(c) = inverse_of_columns3(&j.map(|col| col.map(Ivl::mid))) else {
            return Ok(None);
        };
        krawczyk(mid, c, &|y| Ok(self.value(t, y)), &|y| {
            Ok(self.columns(t, y))
        })
    }

    /// The box that holds the root for every `t` of the piece between `a` and `b` (the
    /// Newton root `mid` at its middle), or `None`. The root moves across the piece, so
    /// Krawczyk's test runs on `z = y − c(t)`, `c` the quadratic through the three Newton
    /// roots: `F(t, c(t))` is enclosed by its mean-value form about the middle, which
    /// shrinks with the piece's width squared where a fixed centre's shrinks with it once.
    fn path_over(&self, a: &End, b: &End, mid: [f64; 3]) -> Result<Option<[Ivl; 3]>> {
        let tm = 0.5 * (a.t + b.t);
        let h = 0.5 * (b.t - a.t);
        // c(t) = mid + beta (t − t_m) + gamma (t − t_m)², per unknown
        let quad = |ya: f64, ym: f64, yb: f64| {
            (
                ym,
                (yb - ya) / (2.0 * h),
                (yb - 2.0 * ym + ya) / (2.0 * h * h),
            )
        };
        let ([a0, a1, a2], [m0, m1, m2], [b0, b1, b2]) = (a.y, mid, b.y);
        let coef = [quad(a0, m0, b0), quad(a1, m1, b1), quad(a2, m2, b2)];
        if !coef
            .iter()
            .all(|&(_, be, ga)| be.is_finite() && ga.is_finite())
        {
            return Ok(None);
        }
        let t = Ivl::new(a.t, b.t);
        let d = t - Ivl::from(tm);
        let two = Ivl::from(2.0);
        let c = coef.map(|(m, be, ga)| Ivl::from(m) + Ivl::from(be) * d + Ivl::from(ga) * (d * d));
        let [dc0, dc1, dc2] = coef.map(|(_, be, ga)| Ivl::from(be) + two * Ivl::from(ga) * d);
        let shift = |[z0, z1, z2]: [Ivl; 3]| {
            let [c0, c1, c2] = c;
            [c0 + z0, c1 + z1, c2 + z2]
        };
        let [ca, cu, cv] = c;
        // F(t, c(t)) ∈ F(t_m, mid) + (t − t_m) (S_sigma sigma' + F_y c') over the piece
        let value = |_: [Ivl; 3]| -> Result<Option<[Ivl; 3]>> {
            let Some([x, y, z]) = self.value(Ivl::from(tm), mid.map(Ivl::from)) else {
                return Ok(None);
            };
            let (sigma, ds, _) = self.sigma(t);
            let Some(s) = self.fillet_over(sigma, ca, 1) else {
                return Ok(None);
            };
            let Some(e) = partials_over(self.end, cu, cv, 1) else {
                return Ok(None);
            };
            let slope = v_add(
                v_add(v_scale(s(1, 0), ds), v_scale(s(0, 1), dc0)),
                neg(v_add(v_scale(e.at(1, 0), dc1), v_scale(e.at(0, 1), dc2))),
            );
            Ok(Some(arr(v_add(V3 { x, y, z }, v_scale(slope, d)))))
        };
        let Some(j) = self.columns(Ivl::from(tm), mid.map(Ivl::from)) else {
            return Ok(None);
        };
        let Some(inv) = inverse_of_columns3(&j.map(|col| col.map(Ivl::mid))) else {
            return Ok(None);
        };
        let z = krawczyk([0.0; 3], inv, &value, &|z| Ok(self.columns(t, shift(z))))?;
        Ok(z.map(shift))
    }

    /// The errors' magnitudes at `t`, the root in the box `y`.
    fn errors_at(&self, t: f64, y: [Ivl; 3]) -> Option<[f64; 7]> {
        let at = Ivl::from(t);
        let [a, u, v] = y;
        let x = partials_over(self.end, u, v, 0)?.at(0, 0);
        let [c3, cf, ce] = self.curves.each_ref().map(|c| {
            curve_derivs_over(c, at, 0, None)
                .ok()
                .and_then(|d| d.first().copied())
        });
        let (c3, cf, ce) = (c3?, cf?, ce?);
        let (cs, ca) = if self.section == 0 {
            (cf.x, cf.y)
        } else {
            (cf.y, cf.x)
        };
        Some([
            (c3.x - x.x).mag(),
            (c3.y - x.y).mag(),
            (c3.z - x.z).mag(),
            (cs - self.sigma(at).0).mag(),
            (ca - a).mag(),
            (ce.x - u).mag(),
            (ce.y - v).mag(),
        ])
    }

    /// `|e''|` of the seven errors over the piece `t`, its root in the box `y`.
    fn second_over(&self, t: Ivl, y: [Ivl; 3]) -> Option<[f64; 7]> {
        let [a, u, v] = y;
        let (_, ds, dds) = self.sigma(t);
        let s = self.fillet_over(self.sigma(t).0, a, 2)?;
        let e = partials_over(self.end, u, v, 2)?;
        let cols = [s(0, 1), neg(e.at(1, 0)), neg(e.at(0, 1))];
        // J y' = −S_sigma sigma'
        let [a1, u1, v1] = solve_over(cols, neg(v_scale(s(1, 0), ds)))?;
        let two = Ivl::from(2.0);
        let s2 = v_add(
            v_add(v_scale(s(2, 0), ds * ds), v_scale(s(1, 1), two * ds * a1)),
            v_add(v_scale(s(0, 2), a1 * a1), v_scale(s(1, 0), dds)),
        );
        let e2 = v_add(
            v_add(
                v_scale(e.at(2, 0), u1 * u1),
                v_scale(e.at(1, 1), two * u1 * v1),
            ),
            v_scale(e.at(0, 2), v1 * v1),
        );
        // J y'' = −(the second derivative of F without its y'' terms)
        let [a2, u2, v2] = solve_over(cols, neg(v_sub(s2, e2)))?;
        let x2 = v_add(e2, v_add(v_scale(e.at(1, 0), u2), v_scale(e.at(0, 1), v2)));
        let [c3, cf, ce] = self.curves.each_ref().map(|c| {
            curve_derivs_over(c, t, 2, None)
                .ok()
                .and_then(|d| d.get(2).copied())
        });
        let (c3, cf, ce) = (c3?, cf?, ce?);
        let (cs, ca) = if self.section == 0 {
            (cf.x, cf.y)
        } else {
            (cf.y, cf.x)
        };
        Some([
            (c3.x - x2.x).mag(),
            (c3.y - x2.y).mag(),
            (c3.z - x2.z).mag(),
            (cs - dds).mag(),
            (ca - a2).mag(),
            (ce.x - u2).mag(),
            (ce.y - v2).mag(),
        ])
    }

    /// `|e'|` of the seven errors over the piece `t`, its root in the box `y`.
    fn first_over(&self, t: Ivl, y: [Ivl; 3]) -> Option<[f64; 7]> {
        let [a, u, v] = y;
        let (sigma, ds, _) = self.sigma(t);
        let s = self.fillet_over(sigma, a, 1)?;
        let e = partials_over(self.end, u, v, 1)?;
        let cols = [s(0, 1), neg(e.at(1, 0)), neg(e.at(0, 1))];
        let [a1, u1, v1] = solve_over(cols, neg(v_scale(s(1, 0), ds)))?;
        let x1 = v_add(v_scale(e.at(1, 0), u1), v_scale(e.at(0, 1), v1));
        let [c3, cf, ce] = self.curves.each_ref().map(|c| {
            curve_derivs_over(c, t, 1, None)
                .ok()
                .and_then(|d| d.get(1).copied())
        });
        let (c3, cf, ce) = (c3?, cf?, ce?);
        let (cs, ca) = if self.section == 0 {
            (cf.x, cf.y)
        } else {
            (cf.y, cf.x)
        };
        Some([
            (c3.x - x1.x).mag(),
            (c3.y - x1.y).mag(),
            (c3.z - x1.z).mag(),
            (cs - ds).mag(),
            (ca - a1).mag(),
            (ce.x - u1).mag(),
            (ce.y - v1).mag(),
        ])
    }

    /// The piece end at `t` from the Newton root `y` there: the root proved in a point box
    /// and the errors there, or `None`.
    fn end_at(&self, t: f64, y: [f64; 3]) -> Result<Option<End>> {
        let Some(b) = self.root_at(t, y)? else {
            return Ok(None);
        };
        Ok(self.errors_at(t, b).map(|e| End { t, y, e }))
    }

    /// The bound of the piece between `a` and `b` (the Newton root `mid` at its middle):
    /// the largest distance of the three curves from the rib over it, or `None`. Each
    /// error is bounded by the remainder about the piece's ends where `e''` encloses,
    /// else (or where that bound is over `tol`) by the centred form `|e(t_m)| + w/2 |e'|`
    /// about its middle, whichever is less.
    fn piece(&self, a: &End, b: &End, mid: [f64; 3], tol: f64) -> Result<Option<f64>> {
        let t = Ivl::new(a.t, b.t);
        let Some(y) = self.path_over(a, b, mid)? else {
            return Ok(None);
        };
        let w = Ivl::from(b.t) - Ivl::from(a.t);
        let mut c = [f64::INFINITY; 7];
        if let Some(e2) = self.second_over(t, y) {
            let rem = w * w / Ivl::from(8.0);
            for (((ci, &ea), &eb), &d2) in c.iter_mut().zip(&a.e).zip(&b.e).zip(&e2) {
                *ci = (Ivl::from(ea.max(eb)) + rem * Ivl::from(d2)).hi;
            }
        }
        let first = self.distance(t, y, c);
        if first.is_some_and(|d| d <= tol) {
            return Ok(first);
        }
        let Some(m) = self.end_at(0.5 * (a.t + b.t), mid)? else {
            return Ok(first);
        };
        let Some(e1) = self.first_over(t, y) else {
            return Ok(first);
        };
        let half = w / Ivl::from(2.0);
        for ((ci, &em), &d1) in c.iter_mut().zip(&m.e).zip(&e1) {
            *ci = ci.min((Ivl::from(em) + half * Ivl::from(d1)).hi);
        }
        Ok(self.distance(t, y, c))
    }

    /// The largest distance of the three curves from the rib over the piece `t` (its root
    /// in the box `y`) for the errors' bounds `c`, or `None`.
    fn distance(&self, t: Ivl, y: [Ivl; 3], c: [f64; 7]) -> Option<f64> {
        if !c.iter().all(|x| x.is_finite()) {
            return None;
        }
        let [cx, cy, cz, cs, ca, cu, cv] = c;
        let d3 = norm(V3 {
            x: Ivl::from(cx),
            y: Ivl::from(cy),
            z: Ivl::from(cz),
        });
        // a UV curve's point and the rib's lie in the box widened by the UV errors
        let wide = |x: Ivl, r: f64| x + Ivl::new(-r, r);
        let [ya, yu, yv] = y;
        let s = self.fillet_over(wide(self.sigma(t).0, cs), wide(ya, ca), 1)?;
        let e = partials_over(self.end, wide(yu, cu), wide(yv, cv), 1)?;
        let d_fil = (Ivl::from(norm(s(1, 0))) * Ivl::from(cs)
            + Ivl::from(norm(s(0, 1))) * Ivl::from(ca))
        .hi;
        let d_end = (Ivl::from(norm(e.at(1, 0))) * Ivl::from(cu)
            + Ivl::from(norm(e.at(0, 1))) * Ivl::from(cv))
        .hi;
        let d = d3.max(d_fil).max(d_end);
        d.is_finite().then_some(d)
    }

    /// The certified bound of the span `[t0, t1]` (the roots `y0`, `y1` at its stations)
    /// within `tol`, or `None` where it is over `tol` or not bounded (module docs): a piece
    /// not bounded within `tol` is halved, at most `LEVELS` times. `solve` finds the roots
    /// inside it.
    pub(super) fn span_bound(
        &self,
        solve: &Solve<'_>,
        (t0, y0): (f64, [f64; 3]),
        (t1, y1): (f64, [f64; 3]),
        tol: f64,
    ) -> Result<Option<f64>> {
        let (Some(a), Some(b)) = (self.end_at(t0, y0)?, self.end_at(t1, y1)?) else {
            return Ok(None);
        };
        // a piece end off the rib by more than tol: no finer cut bounds the span
        let off = |e: &End| {
            let [x, y, z, ..] = e.e;
            norm(V3 {
                x: Ivl::from(x),
                y: Ivl::from(y),
                z: Ivl::from(z),
            }) > tol
        };
        let mut worst = 0.0_f64;
        let mut pieces = vec![(a, b, 0)];
        while let Some((a, b, depth)) = pieces.pop() {
            if off(&a) || off(&b) {
                return Ok(None);
            }
            let mut guess = a.y;
            for (g, &bi) in guess.iter_mut().zip(&b.y) {
                *g = 0.5 * (*g + bi);
            }
            let t = 0.5 * (a.t + b.t);
            let mid = solve(t, guess)?.unwrap_or(guess);
            match self.piece(&a, &b, mid, tol)? {
                Some(d) if d <= tol => worst = worst.max(d),
                _ if depth < LEVELS => {
                    let Some(m) = self.end_at(t, mid)? else {
                        return Ok(None);
                    };
                    pieces.push((m, b, depth + 1));
                    pieces.push((a, m, depth + 1));
                }
                _ => return Ok(None),
            }
        }
        Ok(Some(worst))
    }

    /// The certified bound of the first span `[0, t1]` (the root `y1` at `t1`, `p` at the
    /// tangent end) down to `t*`, with `t*`, or `None` (#17744). Its pieces are graded
    /// geometrically toward the tangent end, `[t/2, t]`, each bounded as a span
    /// ([`Self::span_bound`]): the Jacobian is singular at the tangent end, and over
    /// `[t/2, t]` its least singular value changes by the same factor at every depth, so
    /// a piece needs as many halvings at every depth. The pieces go on while their roots
    /// are certified; `t*` is the last piece's lower end, where the root's along-rail
    /// offset from the tangent end is within `tol`. The rib over `[0, t*]` is not bounded
    /// here.
    pub(super) fn first_span_bound(
        &self,
        solve: &Solve<'_>,
        p: [f64; 3],
        (t1, y1): (f64, [f64; 3]),
        tol: f64,
    ) -> Result<Option<(f64, f64)>> {
        // the offsets from the tangent end grow as t^m: alpha ~ s^(k/2), s ~ t or t^2
        let m = if self.odd { self.order } else { self.order / 2 }.max(1);
        let near = |y: &[f64; 3]| {
            let ([a, ..], [pa, ..]) = (*y, p);
            self.fillet_over(Ivl::from(self.sigma0), Ivl::new(a.min(pa), a.max(pa)), 1)
                .is_some_and(|s| {
                    (Ivl::from(norm(s(0, 1))) * (Ivl::from(a) - Ivl::from(pa))).mag() <= tol
                })
        };
        let mut worst = 0.0_f64;
        let (mut hi_t, mut hi_y) = (t1, y1);
        for _ in 0..GRADES {
            let lo_t = 0.5 * hi_t;
            let mut guess = p;
            for (g, &h) in guess.iter_mut().zip(&hi_y) {
                *g += (h - *g) * 0.5_f64.powi(i32::try_from(m).unwrap_or(1));
            }
            let lo_y = solve(lo_t, guess)?;
            let bound = match lo_y {
                Some(lo_y) => self.span_bound(solve, (lo_t, lo_y), (hi_t, hi_y), tol)?,
                None => None,
            };
            match (lo_y, bound) {
                (Some(lo_y), Some(b)) => {
                    worst = worst.max(b);
                    hi_t = lo_t;
                    hi_y = lo_y;
                }
                _ => break,
            }
        }
        Ok(near(&hi_y).then_some((worst, hi_t)))
    }
}
