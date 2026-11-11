//! The certificate on a guided edge of a railed loft: a proved upper bound of the distance
//! between the loft's surface and a guide rail, between two stations, by the convex hull of
//! Bernstein coefficients ([`crate::query::distance_bound`]'s pieces and hull).
//!
//! Upstream: none (not in SMLib or SolidCraft; tangency-labs/solidcore#3140, the hull case on #3140).
//!
//! | Rust               | Upstream |
//! |--------------------|----------|
//! | [`span_bound`]     | (none)   |
//!
//! **What is bounded.** Between two stations the rail runs from parameter `t_a` to `t_b`
//! and the surface holds a point near each end, at `(u_a, v_a)` and `(u_b, v_b)`. With
//! `tau` in `[0, 1]`, the correspondence `R(t(tau))` against `S(u(tau), v(tau))`, all three
//! parameters linear in `tau`, is bounded: `D(tau) = S(u(tau), v(tau)) - R(t(tau))`. Every
//! point of the rail's arc is then within the bound of a point of the surface, and every point
//! of the surface's curve `S(u(tau), v(tau))` within it of the rail. The correspondence is any
//! one; the bound holds for it, whatever the nodes' accuracy (they are found by projection,
//! which is not trusted).
//!
//! **How.** `tau` is cut where the surface line crosses a knot line of the surface's Bézier
//! patches or the rail's parameter crosses a knot of its Bézier spans, so each interval lies in
//! one patch and one span. On an interval the patch is restricted to the rectangle the line
//! spans (de Casteljau, [`Bernstein2::restrict`]), turned so the line runs corner to corner,
//! and read along that diagonal: for a tensor patch of degrees `(p, q)`,
//! `P(s, s) = sum_k c_k B_k^(p+q)(s)` with
//! `c_k = sum_(i + j = k) b_ij C(p, i) C(q, j) / C(p + q, k)` (the product of the two
//! Bernstein bases, Farouki and Rajan 1988), a convex combination of the patch's coefficients.
//! The rail's span is restricted likewise. In homogeneous form, both about the same point,
//! `D = N / (W_s W_r)` with `N = S_h W_r - R_h W_s`; with positive weights every value of `D`
//! lies in the box of the Euclidean control points of `(N, W_s W_r)`, widened for the
//! coefficients' error ([`Piece::widened_box`]). The bound is the box's farthest corner from
//! the origin, rounded up.
//!
//! **The coefficients' error** (Higham, *Accuracy and Stability of Numerical Algorithms*,
//! 2nd ed., section 3.1, `gamma_n = n u / (1 - n u)`), carried as one absolute bound per
//! homogeneous coefficient, as [`crate::query::distance_bound`] carries it:
//! - the surface's patches: their own error from [`pieces_about`] (the form's, the
//!   extraction's and the translation's);
//! - the rail's spans: the extraction's where it inserts knots (`gamma_(16 r)` of the net's
//!   largest coefficient, the bound [`pieces_about`] takes for a surface's), and the
//!   translation's, `gamma_2 (|x| + |p| w)` per coordinate;
//! - each restriction: at most two de Casteljau splits each way, each adding
//!   `gamma_(2 (du + dv) + 4)` of the largest coefficient and its error (the bound
//!   [`Piece::quarters`] charges a split both ways);
//! - the diagonal: each `c_k` a sum of at most `min(p, q) + 1` terms, each a coefficient
//!   times a ratio of three exact binomials, so within `gamma_(p + q + 4)` of the largest
//!   coefficient; the input error passes through the convex combination unchanged;
//! - each product of two polynomials ([`Bernstein2::mul`]): a coefficient is a sum of at
//!   most `N = (da + 1)(db + 1)` terms, each two coefficients scaled by two exact binomials
//!   and multiplied, then divided by two, so within `gamma_(N + 7)` of the product of the two
//!   largest coefficients; the inputs' errors `ea`, `eb` add `ea |b| + |a| eb + ea eb`;
//! - the difference: `gamma_1` of the sum of the two products' sizes.
//!
//! **The intervals' ends.** A restriction's split parameters are rounded, so the piece read
//! is the polynomial over an interval whose ends are off by at most `gamma_4` of the patch's
//! unit interval. Over that sliver `D` moves by at most its derivative's bound times its
//! width: the derivative of a Bernstein polynomial of degree `n` is bounded by `2 n` times its
//! largest coefficient, so each interval's bound adds `2 n gamma_4` times the largest
//! Euclidean coefficient of `N` over the least of `W_s W_r`.
//!
//! **Where the bound is too loose.** A linear correspondence slides along the rail where the
//! surface's parameters do not run in step with the rail's, and the slide counts in `|D|`. A
//! piece whose bound exceeds the tolerance is halved at the rail's middle parameter, and its new
//! node is the rail's point there found on the surface (by projection, not trusted), so the
//! correspondence follows the rail piece by piece; at most [`MAX_HALVINGS`] halvings, and at
//! most [`MAX_SEGMENTS`] pieces in all (past them the span is not proved: an infinite bound).
//! A piece whose rail middle is itself farther than the tolerance from the surface (by its
//! projection, which only steers here) is not halved: no correspondence brings it within the
//! tolerance, and its bound stands. The largest bound over the pieces is returned: the
//! caller decides (a station there, or a refusal).
//!
//! Deviations: not upstream. No recursion: the intervals wait on a work list.

use crate::error::{Result, SolidError};
use crate::geom::vector2d::Point2;
use crate::kernel::Kernel;
use crate::math::Point3;
use crate::nurbs::crv_basic::Curve as NurbCurve;
use crate::nurbs::math::{gamma, CPoint, NOW};
use crate::offset::construction::bernstein::{Bernstein2, Binomials};
use crate::query::distance_bound::{pieces_about, Piece};
use crate::surface::surface::Surface;

const OP: &str = "sweep::loft_rails::certificate";

/// Most halvings of one interval between knot crossings.
pub const MAX_HALVINGS: usize = 12;
/// Most pieces one span's correspondence is taken in (a resource bound).
pub const MAX_SEGMENTS: usize = 1 << 16;

fn failed(reason: &'static str) -> SolidError {
    SolidError::Unsupported {
        operation: OP,
        reason,
    }
}

/// A rail's Bézier spans about a point: per span its parameter range, its homogeneous
/// coefficients `(X - p W, Y - p W, Z - p W, W)` as degree-`r` polynomials on `[0, 1]`, and the
/// error each coefficient may carry.
pub struct RailSpans {
    spans: Vec<([f64; 2], [Bernstein2; 4])>,
    err: f64,
}

/// One end of a span: the rail's parameter there and the surface's parameters.
#[derive(Clone, Copy, Debug)]
pub struct Node {
    /// The rail's parameter.
    pub t: f64,
    /// The surface's parameters.
    pub uv: Point2,
}

impl RailSpans {
    /// The rail `nurb`'s spans about `p`.
    ///
    /// # Errors
    ///
    /// `Unsupported` for a weight that is not positive or coefficients that are not finite;
    /// those of the decomposition.
    pub fn about(nurb: &NurbCurve, p: Point3) -> Result<Self> {
        let r = nurb.degree();
        let w = |c: &CPoint| if c.w == NOW { 1.0 } else { c.w };
        let mut net_size = 0.0_f64;
        for c in nurb.cpts() {
            net_size = net_size
                .max(c.x.abs())
                .max(c.y.abs())
                .max(c.z.abs())
                .max(w(c).abs());
        }
        // an interior knot below full multiplicity is where the extraction inserts
        let knots = nurb.knots();
        let (Some(&first), Some(&last)) = (knots.first(), knots.last()) else {
            return Err(failed("a rail without knots"));
        };
        let mut inserts = false;
        let mut i = 0;
        while let Some(&u) = knots.get(i) {
            let mut m = 1;
            while knots.get(i + m) == Some(&u) {
                m += 1;
            }
            if u > first && u < last && m < r {
                inserts = true;
            }
            i += m;
        }
        let mut err = if inserts {
            gamma(16 * r) * net_size
        } else {
            0.0
        };
        let far = p.x.abs().max(p.y.abs()).max(p.z.abs());
        let mut spans = Vec::new();
        for span in nurb.decompose_bez()? {
            let (Some(&a), Some(&b)) = (span.knots().first(), span.knots().last()) else {
                return Err(failed("a rail span without its knots"));
            };
            let mut h: [Vec<f64>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
            for c in span.cpts() {
                let wc = w(c);
                #[allow(clippy::neg_cmp_op_on_partial_ord)] // refuses NaN too
                if !(wc > 0.0) {
                    return Err(failed("a rail's weight that is not positive"));
                }
                let [hx, hy, hz, hw] = &mut h;
                hx.push(c.x - p.x * wc);
                hy.push(c.y - p.y * wc);
                hz.push(c.z - p.z * wc);
                hw.push(wc);
            }
            let d = span.degree();
            let [hx, hy, hz, hw] = h;
            spans.push((
                [a, b],
                [
                    Bernstein2::new(d, 0, hx)?,
                    Bernstein2::new(d, 0, hy)?,
                    Bernstein2::new(d, 0, hz)?,
                    Bernstein2::new(d, 0, hw)?,
                ],
            ));
        }
        // the translation: c - p w, a product and a difference
        err += gamma(2) * (net_size + far * net_size) + err * (1.0 + far);
        if !err.is_finite() {
            return Err(failed("a rail's coefficients are not finite"));
        }
        Ok(Self { spans, err })
    }

    fn knots(&self) -> Vec<f64> {
        let mut out: Vec<f64> = self.spans.iter().flat_map(|(ab, _)| *ab).collect();
        out.sort_by(f64::total_cmp);
        out.dedup();
        out
    }
}

fn largest(h: &[Bernstein2; 4]) -> f64 {
    h.iter()
        .map(Bernstein2::max_abs_coefficient)
        .fold(0.0_f64, f64::max)
}

/// `a`'s restriction to `[lo, hi]` of its first variable (and `[vlo, vhi]` of its second),
/// with the error bound it adds: at most two splits each way.
fn restricted(h: &[Bernstein2; 4], err: f64, u: [f64; 2], v: [f64; 2]) -> ([Bernstein2; 4], f64) {
    let (du, dv) = h[3].degrees();
    let add = 2.0 * gamma(2 * (du + dv) + 4) * (largest(h) + err);
    (
        h.clone().map(|c| c.restrict(u[0], u[1], v[0], v[1])),
        err + add,
    )
}

/// `h` with its first variable reversed (`reverse_u`) and its second (`reverse_v`).
fn reversed(h: &Bernstein2, reverse_u: bool, reverse_v: bool) -> Result<Bernstein2> {
    let (du, dv) = h.degrees();
    let c = h.coefficients();
    let mut out = vec![0.0; c.len()];
    for i in 0..=du {
        for j in 0..=dv {
            let (si, sj) = (
                if reverse_u { du - i } else { i },
                if reverse_v { dv - j } else { j },
            );
            if let (Some(slot), Some(&x)) =
                (out.get_mut(i * (dv + 1) + j), c.get(si * (dv + 1) + sj))
            {
                *slot = x;
            }
        }
    }
    Bernstein2::new(du, dv, out)
}

/// The patch `h` read along its diagonal `(s, s)`: a polynomial of degree `du + dv` in `s`
/// (module docs).
fn diagonal(h: &Bernstein2, b: &Binomials) -> Result<Bernstein2> {
    let (du, dv) = h.degrees();
    let n = du + dv;
    let c = h.coefficients();
    let mut out = vec![0.0; n + 1];
    for i in 0..=du {
        for j in 0..=dv {
            if let (Some(slot), Some(&x)) = (out.get_mut(i + j), c.get(i * (dv + 1) + j)) {
                *slot += x * (b.get(du, i) * b.get(dv, j) / b.get(n, i + j));
            }
        }
    }
    Bernstein2::new(n, 0, out)
}

/// The product `a b` and its error, the inputs within `ea` and `eb` (module docs).
fn product(
    a: &Bernstein2,
    ea: f64,
    b: &Bernstein2,
    eb: f64,
    bi: &Binomials,
) -> Result<(Bernstein2, f64)> {
    let (ma, mb) = (a.max_abs_coefficient(), b.max_abs_coefficient());
    let terms = (a.degrees().0 + 1) * (b.degrees().0 + 1);
    let err = gamma(terms + 7) * ma * mb + ea * mb + ma * eb + ea * eb;
    Ok((a.mul(b, bi)?, err))
}

/// The bound over one interval of `tau`, `[t0, t1]` of the span: the patch `piece` (its
/// rectangle in the surface's parameters), the rail's span `rail` over `range` with error
/// `rail_err`, and the nodes' parameters at the interval's ends.
#[allow(clippy::too_many_arguments)]
fn interval_bound(
    piece: &Piece,
    rail: &[Bernstein2; 4],
    range: [f64; 2],
    rail_err: f64,
    uv0: Point2,
    uv1: Point2,
    t0: f64,
    t1: f64,
    bi: &Binomials,
) -> Result<f64> {
    let [pu0, pu1, pv0, pv1] = piece.uv;
    let local = |x: f64, lo: f64, hi: f64| ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
    let (su0, su1) = (local(uv0.x, pu0, pu1), local(uv1.x, pu0, pu1));
    let (sv0, sv1) = (local(uv0.y, pv0, pv1), local(uv1.y, pv0, pv1));
    let (h, e_s) = restricted(
        &piece.h,
        piece.err,
        [su0.min(su1), su0.max(su1)],
        [sv0.min(sv1), sv0.max(sv1)],
    );
    let mut s = Vec::with_capacity(4);
    for c in &h {
        s.push(diagonal(&reversed(c, su0 > su1, sv0 > sv1)?, bi)?);
    }
    let (p, q) = h[3].degrees();
    let e_s = e_s + gamma(p + q + 4) * (largest(&h) + e_s);
    let (r0, r1) = (local(t0, range[0], range[1]), local(t1, range[0], range[1]));
    let (rh, e_r) = restricted(rail, rail_err, [r0.min(r1), r0.max(r1)], [0.0, 1.0]);
    let mut r = Vec::with_capacity(4);
    for c in &rh {
        r.push(reversed(c, r0 > r1, false)?);
    }
    let (Some(sw), Some(rw)) = (s.get(3), r.get(3)) else {
        return Err(failed("a homogeneous piece without its weight"));
    };
    let mut n: Vec<Bernstein2> = Vec::with_capacity(4);
    let mut e_n = 0.0_f64;
    for k in 0..3 {
        let (Some(sk), Some(rk)) = (s.get(k), r.get(k)) else {
            return Err(failed("a homogeneous piece without its coordinates"));
        };
        let (a, ea) = product(sk, e_s, rw, e_r, bi)?;
        let (b, eb) = product(rk, e_r, sw, e_s, bi)?;
        let size = a.max_abs_coefficient() + b.max_abs_coefficient();
        e_n = e_n.max(ea + eb + gamma(1) * size);
        n.push(a.sub(&b, bi)?);
    }
    let (den, e_d) = product(sw, e_s, rw, e_r, bi)?;
    let [Some(nx), Some(ny), Some(nz)] =
        [n.first(), n.get(1), n.get(2)].map(Option::<&Bernstein2>::cloned)
    else {
        return Err(failed("a homogeneous piece without its coordinates"));
    };
    let degree = den.degrees().0;
    let lifted = Piece {
        h: [nx, ny, nz, den],
        err: e_n.max(e_d),
        lower: 0.0,
        depth: 0,
        uv: [t0, t1, 0.0, 1.0],
    };
    let (lo, hi) = lifted.widened_box()?;
    let mut sum = 0.0_f64;
    let mut far = 0.0_f64;
    for (l, u) in lo.into_iter().zip(hi) {
        let m = l.abs().max(u.abs());
        far = far.max(m);
        sum += m * m;
    }
    // the ends' sliver (module docs): 2 n gamma_4 of the largest Euclidean coefficient
    #[allow(clippy::cast_precision_loss)] // a degree, at most 2 * MAX_DEGREE
    let sliver = 2.0 * degree as f64 * gamma(4) * far;
    let bound = sum.sqrt() * (1.0 + gamma(4)) + sliver;
    if !bound.is_finite() {
        return Err(failed("a span's bound is not finite"));
    }
    Ok(bound)
}

/// A piece of the correspondence: its two nodes (the surface's parameters and the rail's)
/// and how many times it was halved.
#[derive(Clone, Copy)]
struct Segment {
    uv0: Point2,
    t0: f64,
    uv1: Point2,
    t1: f64,
    depth: usize,
}

/// `seg` cut where it crosses a knot line of `pieces` or a knot of `rail_knots`, the cut's
/// crossing coordinate set to the knot itself (so a cut is not cut again); `None` where it
/// crosses none.
fn cut(seg: Segment, pieces: &[Piece], rail_knots: &[f64]) -> Option<Vec<Segment>> {
    // each crossing: its fraction along the segment, and which coordinate is the knot
    let mut cuts: Vec<(f64, usize, f64)> = Vec::new();
    let mut cross = |x0: f64, x1: f64, knot: f64, which: usize| {
        if (x0 < knot && knot < x1) || (x1 < knot && knot < x0) {
            cuts.push(((knot - x0) / (x1 - x0), which, knot));
        }
    };
    for piece in pieces {
        let [u0, u1, v0, v1] = piece.uv;
        for knot in [u0, u1] {
            cross(seg.uv0.x, seg.uv1.x, knot, 0);
        }
        for knot in [v0, v1] {
            cross(seg.uv0.y, seg.uv1.y, knot, 1);
        }
    }
    for &knot in rail_knots {
        cross(seg.t0, seg.t1, knot, 2);
    }
    if cuts.is_empty() {
        return None;
    }
    cuts.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut out = Vec::new();
    let (mut uv, mut t) = (seg.uv0, seg.t0);
    for (f, which, knot) in cuts {
        let mut uv_at = Point2::new(
            seg.uv0.x + (seg.uv1.x - seg.uv0.x) * f,
            seg.uv0.y + (seg.uv1.y - seg.uv0.y) * f,
        );
        let mut t_at = seg.t0 + (seg.t1 - seg.t0) * f;
        match which {
            0 => uv_at.x = knot,
            1 => uv_at.y = knot,
            _ => t_at = knot,
        }
        out.push(Segment {
            uv0: uv,
            t0: t,
            uv1: uv_at,
            t1: t_at,
            depth: seg.depth,
        });
        (uv, t) = (uv_at, t_at);
    }
    out.push(Segment {
        uv0: uv,
        t0: t,
        uv1: seg.uv1,
        t1: seg.t1,
        depth: seg.depth,
    });
    Some(out)
}

/// The proved bound of the distance between the surface `surf` and the rail `rail_curve`
/// (its spans about `p`, [`RailSpans::about`]) between the nodes `a` and `b` (module docs).
/// The correspondence starts as the line between them; a piece whose bound is above `tol` is
/// halved at the rail's middle parameter, its new node the rail's point there found on the
/// surface (by projection, not trusted: the bound holds for any correspondence), at most
/// [`MAX_HALVINGS`] times. Returns the largest bound over the pieces.
///
/// # Errors
///
/// `Unsupported` for a surface or rail whose weights are not proved positive, or a node
/// outside the surface's patches; those of the pieces and evaluations.
#[allow(clippy::too_many_arguments)]
pub fn span_bound(
    k: &Kernel,
    surf: &Surface,
    rail_curve: &crate::curve::curve::Curve,
    rail: &RailSpans,
    p: Point3,
    a: Node,
    b: Node,
    tol: f64,
) -> Result<f64> {
    let (pieces, slack) = pieces_about(k, surf, None, p)?;
    let bi = Binomials::new();
    let rail_knots = rail.knots();
    let mut worst = 0.0_f64;
    let mut work = vec![Segment {
        uv0: a.uv,
        t0: a.t,
        uv1: b.uv,
        t1: b.t,
        depth: 0,
    }];
    let mut budget = MAX_SEGMENTS;
    while let Some(seg) = work.pop() {
        // past its most pieces the span is not proved: an infinite bound (the caller adds a
        // station there, or refuses)
        let Some(left) = budget.checked_sub(1) else {
            return Ok(f64::INFINITY);
        };
        budget = left;
        if let Some(parts) = cut(seg, &pieces, &rail_knots) {
            work.extend(parts);
            continue;
        }
        let uv_mid = Point2::new(0.5 * (seg.uv0.x + seg.uv1.x), 0.5 * (seg.uv0.y + seg.uv1.y));
        let t_mid = 0.5 * (seg.t0 + seg.t1);
        let piece = pieces
            .iter()
            .find(|q| {
                let [u0, u1, v0, v1] = q.uv;
                u0 <= uv_mid.x && uv_mid.x <= u1 && v0 <= uv_mid.y && uv_mid.y <= v1
            })
            .ok_or_else(|| failed("a node outside the surface's patches"))?;
        let (range, span) = rail
            .spans
            .iter()
            .find(|(ab, _)| ab[0] <= t_mid && t_mid <= ab[1])
            .ok_or_else(|| failed("a node outside the rail's spans"))?;
        let bound = interval_bound(
            piece, span, *range, rail.err, seg.uv0, seg.uv1, seg.t0, seg.t1, &bi,
        )? + slack;
        if bound > tol && seg.depth < MAX_HALVINGS {
            // the rail's middle found on the surface: the new node (its line midpoint where the
            // projection fails). Where the rail's middle is itself farther than the tolerance
            // from the surface, no correspondence can bring the piece within it: halving would
            // only spend pieces, so the piece's bound stands (the caller adds a station there).
            let q = rail_curve.evaluate_point(k, t_mid)?;
            let found = node_on(k, surf, q).ok();
            if found.is_some_and(|(_, d)| d > tol) {
                worst = worst.max(bound);
                continue;
            }
            let uv = found.map_or(uv_mid, |(uv, _)| uv);
            let depth = seg.depth + 1;
            work.push(Segment {
                uv0: seg.uv0,
                t0: seg.t0,
                uv1: uv,
                t1: t_mid,
                depth,
            });
            work.push(Segment {
                uv0: uv,
                t0: t_mid,
                uv1: seg.uv1,
                t1: seg.t1,
                depth,
            });
            continue;
        }
        worst = worst.max(bound);
    }
    Ok(worst)
}

/// The surface's point nearest `q` that the certificate starts from, and its parameters: a
/// node, not trusted (the bound holds whatever it is).
///
/// # Errors
///
/// Those of the projection.
pub fn node_on(k: &Kernel, surf: &Surface, q: Point3) -> Result<(Point2, f64)> {
    let c = crate::query::api_queries::surface_closest_point(k, surf, q)?;
    let uv = c
        .uvs
        .first()
        .copied()
        .ok_or_else(|| failed("a rail point with no foot on the loft"))?;
    Ok((uv, c.closest.distance))
}
