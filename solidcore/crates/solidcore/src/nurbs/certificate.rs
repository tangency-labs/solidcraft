//! The certificate on a guided edge of a railed loft: a proved upper bound of the distance
//! between the loft's surface and a guide rail, between two stations, by the convex hull of
//! Bernstein coefficients ([`crate::query::distance_bound`]'s pieces and hull).
//!
//! Upstream: none (not in SMLib or SolidCraft; tangency-labs/solidcore#3140, the hull case on #3140).
//!
//! | Rust               | Upstream |
//! |--------------------|----------|
//! | [`span_bound`]     | (none)   |
//! | [`turn_about_axis`] | (none)  |
//! | [`on_circle`]      | (none)   |
//! | [`meridian_collapse`] | (none) |
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
//! Deviations: not upstream. No recursion: the intervals wait on a work list ([`on_circle`]'s
//! pieces too, at most [`MAX_CIRCLE_HALVINGS`] deep and [`MAX_CIRCLE_PIECES`] in all, and
//! [`meridian_collapse`]'s, at most [`MAX_COLLAPSE_HALVINGS`] deep and
//! [`MAX_COLLAPSE_PIECES`] in all).

use crate::error::{Result, SolidError};
use crate::geom::vector2d::Point2;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::nurbs::crv_basic::Curve as NurbCurve;
use crate::nurbs::math::{gamma, CPoint, NOW};
use crate::offset::construction::bernstein::{Bernstein2, Binomials, MAX_DEGREE};
use crate::query::distance_bound::{pieces_about, Piece};
use crate::surface::surface::Surface;

const OP: &str = "nurbs::certificate";

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
    /// `Unsupported` for a point `p`, a control point, a weight or a knot that is not finite
    /// (checked before any bound is taken: `f64::max` passes a NaN over), a weight that is
    /// not positive, or coefficients that are not finite; those of the decomposition.
    pub fn about(nurb: &NurbCurve, p: Point3) -> Result<Self> {
        let r = nurb.degree();
        let w = |c: &CPoint| if c.w == NOW { 1.0 } else { c.w };
        if !(p.x.is_finite() && p.y.is_finite() && p.z.is_finite()) {
            return Err(failed("a rail's centre that is not finite"));
        }
        if !nurb.knots().iter().all(|u| u.is_finite()) {
            return Err(failed("a rail's knot that is not finite"));
        }
        let mut net_size = 0.0_f64;
        let mut max_w = 0.0_f64;
        for c in nurb.cpts() {
            if ![c.x, c.y, c.z, w(c)].iter().all(|v| v.is_finite()) {
                return Err(failed("a rail's control point that is not finite"));
            }
            net_size = net_size
                .max(c.x.abs())
                .max(c.y.abs())
                .max(c.z.abs())
                .max(w(c).abs());
            max_w = max_w.max(w(c).abs());
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
        // the translation: c - p w, a product and a difference, of magnitude at most
        // |c| + far w (a length: `far` times a weight, not times a coordinate). The spans'
        // coefficients are convex combinations of the net's (positive weights), so the net's
        // largest coordinate and weight bound theirs.
        err += gamma(2) * (net_size + far * max_w) + err * (1.0 + far);
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

/// Which way round the line through `origin` along `axis` the curve `nurb` turns over
/// `interval` of its parameter, proved: `Some(1.0)` when `axis . ((c(t) - origin) x c'(t))` is
/// positive all along it (counter-clockwise seen from the tip of `axis`), `Some(-1.0)` when it
/// is negative all along it, `None` when that is not proved: the curve turns back or stands
/// still about the line somewhere, or the bound is too loose to tell. Not upstream
/// (solidcraft#12920: an edge on a cylinder projected along the axis is an arc of its circle
/// only if it turns one way round). `axis` need not be a unit vector.
///
/// **How.** On each Bézier span, in homogeneous form about `origin` ([`RailSpans::about`]),
/// `c - origin = H / W` and `c' = (H' W - H W') / W^2`, so `(c - origin) x c' = (H x H') / W^2`
/// (the `H x H` term vanishes). With positive weights the sign along `axis` is that of
/// `q = axis . (H x H')`, a polynomial of degree `2r - 1` on the span. By the convex hull
/// property it lies between its least and largest Bernstein coefficient, so the sign is proved
/// where every coefficient clears its error bound with one sign, on every span the interval
/// meets (a span it meets in part is restricted to that part first).
///
/// **The coefficients' error**, from the spans' own ([`RailSpans::about`], [`restricted`]):
/// `H'` (`r` times differences) within `2 r e + gamma_2 2 r |H|`; each product of `H x H'` as
/// [`product`]; each component a difference, `gamma_1` of the two products' sizes; `q` three
/// scaled sums, `gamma_4` of their size on top of the components' errors scaled by `|axis|`.
///
/// # Errors
///
/// `Unsupported` for an interval that is not increasing and finite, a weight that is not
/// positive or coefficients that are not finite; those of the decomposition.
pub fn turn_about_axis(
    nurb: &NurbCurve,
    origin: Point3,
    axis: Vec3,
    interval: [f64; 2],
) -> Result<Option<f64>> {
    let [t0, t1] = interval;
    #[allow(clippy::neg_cmp_op_on_partial_ord)] // refuses NaN too
    if !(t0 < t1) || !t0.is_finite() || !t1.is_finite() {
        return Err(failed("an interval that is not increasing and finite"));
    }
    let spans = RailSpans::about(nurb, origin)?;
    let bi = Binomials::new();
    let a = [axis.x, axis.y, axis.z];
    let mut sign = None;
    for ([sa, sb], h) in &spans.spans {
        let (lo, hi) = (t0.max(*sa), t1.min(*sb));
        if lo >= hi || sb <= sa {
            continue;
        }
        let (u0, u1) = ((lo - sa) / (sb - sa), (hi - sa) / (sb - sa));
        let (h, e) = if u0 > 0.0 || u1 < 1.0 {
            restricted(h, spans.err, [u0, u1], [0.0, 1.0])
        } else {
            (h.clone(), spans.err)
        };
        let [hx, hy, hz, _] = &h;
        let r = hx.degrees().0;
        #[allow(clippy::cast_precision_loss)]
        let k = 2.0 * r as f64;
        let m = [hx, hy, hz]
            .iter()
            .map(|c| c.max_abs_coefficient())
            .fold(0.0_f64, f64::max);
        let ed = k * e + gamma(2) * k * m;
        let (dx, dy, dz) = (hx.deriv_u(), hy.deriv_u(), hz.deriv_u());
        // a component of H x H', p dq - q dp, and its error
        let comp = |p: &Bernstein2,
                    dq: &Bernstein2,
                    q: &Bernstein2,
                    dp: &Bernstein2|
         -> Result<(Bernstein2, f64)> {
            let (l, el) = product(p, e, dq, ed, &bi)?;
            let (rr, er) = product(q, e, dp, ed, &bi)?;
            let size = l.max_abs_coefficient() + rr.max_abs_coefficient();
            Ok((l.sub(&rr, &bi)?, el + er + gamma(1) * size))
        };
        let (cx, ex) = comp(hy, &dz, hz, &dy)?;
        let (cy, ey) = comp(hz, &dx, hx, &dz)?;
        let (cz, ez) = comp(hx, &dy, hy, &dx)?;
        let q = cx
            .scale(a[0])
            .add(&cy.scale(a[1]), &bi)?
            .add(&cz.scale(a[2]), &bi)?;
        let size = a[0].abs() * cx.max_abs_coefficient()
            + a[1].abs() * cy.max_abs_coefficient()
            + a[2].abs() * cz.max_abs_coefficient();
        let eq = a[0].abs() * ex + a[1].abs() * ey + a[2].abs() * ez + gamma(4) * size;
        let (lo_q, hi_q) = (q.min_coefficient(), q.max_coefficient());
        if !eq.is_finite() || !lo_q.is_finite() || !hi_q.is_finite() {
            return Err(failed("a curve's turn coefficients are not finite"));
        }
        let here = if lo_q > eq {
            1.0
        } else if hi_q < -eq {
            -1.0
        } else {
            return Ok(None);
        };
        match sign {
            Some(s) if s != here => return Ok(None),
            _ => sign = Some(here),
        }
    }
    Ok(sign)
}

/// Most pieces [`on_circle`] bounds in all (a resource bound; past it: [`OnCircle::Unproved`]).
pub const MAX_CIRCLE_PIECES: usize = 1 << 12;
/// Most halvings of one Bézier span in [`on_circle`].
pub const MAX_CIRCLE_HALVINGS: usize = 40;

/// One piece of [`on_circle`]'s work list: its span, its range in the span's `[0, 1]`, its
/// homogeneous coefficients about the centre, their error and its halving depth.
type CirclePiece = (usize, [f64; 2], [Bernstein2; 4], f64, usize);

/// What [`on_circle`] proves of a curve and a circle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OnCircle {
    /// Every point of the curve is proved within the tolerance of the circle's radius about
    /// its centre and within it of its plane.
    Within,
    /// The curve's point at this parameter is farther than the tolerance from the circle's
    /// radius or plane, past its evaluation's rounding (a witness).
    Outside(f64),
    /// Neither, within [`MAX_CIRCLE_HALVINGS`] and [`MAX_CIRCLE_PIECES`]: the curve comes to
    /// the tolerance's edge, or its bound does not tighten.
    Unproved,
}

/// Whether every point `X` of the curve `nurb` lies within `tol` of the circle about `center`
/// of radius `radius` in the plane through `center` normal to `normal`: `| |X - center| -
/// radius | <= tol` and `|normal . (X - center)| / |normal| <= tol`, the two tests
/// [`crate::curve::curve::p5::is_arc_base`] makes at its samples, here over the whole curve.
/// Not upstream (solidcraft#17790's review: a sampled arc test decided that a surface of
/// revolution's offset collapses).
///
/// **How.** On each Bézier span, in homogeneous form about `center` ([`RailSpans::about`]),
/// `X - center = H / W`. Then `|X - center|^2 - radius^2 = F / W^2` with
/// `F = H . H - radius^2 W^2` (degree `2r`), and `normal . (X - center) = G / W` with
/// `G = normal . H` (degree `r`). By the convex hull property `|F|` and `|G|` are at most their
/// largest coefficient magnitude and `W` at least its least coefficient, so
/// `| |X - center|^2 - radius^2 | <= f` and, as `|X - center| - radius` is that divided by
/// `|X - center| + radius >= radius + sqrt(max(radius^2 - f, 0))`, the radial deviation is
/// at most `f / (radius + sqrt(max(radius^2 - f, 0)))`. For a rational arc of the circle
/// `F` and `G` vanish identically, so their coefficients are rounding and the bound is
/// rounding: an exact arc is proved at once, with no subdivision.
///
/// A span whose bound exceeds `tol` is read at its ends and middle: a point past `tol` by
/// more than its evaluation's rounding is a witness ([`OnCircle::Outside`]). Otherwise the
/// span is halved at its middle (de Casteljau at `1/2`, so the pieces' ends are exact
/// dyadic parameters and cover the span with no sliver) and each half bounded again, at
/// most [`MAX_CIRCLE_HALVINGS`] deep and [`MAX_CIRCLE_PIECES`] pieces in all; past them
/// the curve is [`OnCircle::Unproved`]. No recursion: the pieces wait on a work list, left
/// half first.
///
/// **The coefficients' error**, from the spans' own ([`RailSpans::about`]): each halving
/// `gamma_(2r + 4)` of the largest coefficient and its error (a de Casteljau split at `1/2`
/// rounds once per level, a convex combination passes the input error through); each
/// square as [`product`]; `F` a sum of four terms, one scaled by `radius^2`, within
/// `gamma_6` of the terms' sizes on top of their errors; `G` three scaled terms, `gamma_4`
/// of their size on top of `|normal|_1` times the error. The least weight is reduced by
/// its error and must stay positive. The final divisions and the square root are rounded
/// up by `1 + gamma_8`.
///
/// # Errors
///
/// `Unsupported` for a centre, normal, radius or tolerance that is not finite, a radius
/// that is not positive, a normal whose length is zero or not finite (a finite normal such
/// as `(0, 0, 1e200)` overflows it, and dividing by it would make every plane bound zero)
/// or a negative tolerance; those of [`RailSpans::about`] and of the products;
/// [`SolidError::Cancelled`] when `k`'s cancel token is set (polled once per piece).
pub fn on_circle(
    k: &Kernel,
    nurb: &NurbCurve,
    center: Point3,
    normal: Vec3,
    radius: f64,
    tol: f64,
) -> Result<OnCircle> {
    let finite = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
    #[allow(clippy::neg_cmp_op_on_partial_ord)] // refuses NaN too
    if !finite(center) || !finite(normal) || !(radius > 0.0) || !radius.is_finite() {
        return Err(failed("a circle that is not finite and positive"));
    }
    #[allow(clippy::neg_cmp_op_on_partial_ord)] // refuses NaN too
    if !(tol >= 0.0) || !tol.is_finite() {
        return Err(failed("a tolerance that is not finite and non-negative"));
    }
    let normal_len = normal.length();
    #[allow(clippy::neg_cmp_op_on_partial_ord)] // refuses NaN too
    if !(normal_len > 0.0) || !normal_len.is_finite() {
        return Err(failed(
            "a circle's normal whose length is zero or not finite",
        ));
    }
    let spans = RailSpans::about(nurb, center)?;
    let bi = Binomials::new();
    let mut work: Vec<CirclePiece> = spans
        .spans
        .iter()
        .enumerate()
        .rev()
        .map(|(i, (_, h))| (i, [0.0, 1.0], h.clone(), spans.err, 0))
        .collect();
    let mut pieces = 0_usize;
    while let Some((i, [u0, u1], h, e, depth)) = work.pop() {
        k.check_cancelled(OP)?;
        pieces += 1;
        if pieces > MAX_CIRCLE_PIECES {
            return Ok(OnCircle::Unproved);
        }
        if circle_piece_within(&h, e, normal, normal_len, radius, tol, &bi)? {
            continue;
        }
        let Some(([a, b], _)) = spans.spans.get(i) else {
            return Err(failed("a span of the curve"));
        };
        for s in [0.0, 0.5, 1.0] {
            if circle_point_outside(&h, e, s, normal, normal_len, radius, tol) {
                return Ok(OnCircle::Outside(a + (u0 + s * (u1 - u0)) * (b - a)));
            }
        }
        if depth >= MAX_CIRCLE_HALVINGS {
            return Ok(OnCircle::Unproved);
        }
        let d = h[3].degrees().0;
        let e = e + gamma(2 * d + 4) * (largest(&h) + e);
        let halves = h.clone().map(|c| c.split_u(0.5));
        let left = halves.clone().map(|(l, _)| l);
        let right = halves.map(|(_, r)| r);
        let m = 0.5 * (u0 + u1);
        work.push((i, [m, u1], right, e, depth + 1));
        work.push((i, [u0, m], left, e, depth + 1));
    }
    Ok(OnCircle::Within)
}

/// Whether the piece `h` (homogeneous about the centre, within `e`) is proved within `tol`
/// of the circle's radius and plane ([`on_circle`]).
fn circle_piece_within(
    h: &[Bernstein2; 4],
    e: f64,
    normal: Vec3,
    normal_len: f64,
    radius: f64,
    tol: f64,
    bi: &Binomials,
) -> Result<bool> {
    let [hx, hy, hz, hw] = h;
    let w_lo = hw.min_coefficient() - e;
    #[allow(clippy::neg_cmp_op_on_partial_ord)] // refuses NaN too
    if !(w_lo > 0.0) {
        return Ok(false);
    }
    let (xx, exx) = product(hx, e, hx, e, bi)?;
    let (yy, eyy) = product(hy, e, hy, e, bi)?;
    let (zz, ezz) = product(hz, e, hz, e, bi)?;
    let (ww, eww) = product(hw, e, hw, e, bi)?;
    let r2 = radius * radius;
    let f = xx.add(&yy, bi)?.add(&zz, bi)?.sub(&ww.scale(r2), bi)?;
    let size = xx.max_abs_coefficient()
        + yy.max_abs_coefficient()
        + zz.max_abs_coefficient()
        + r2 * ww.max_abs_coefficient();
    let ef = exx + eyy + ezz + r2 * eww + gamma(6) * size;
    let g = hx
        .scale(normal.x)
        .add(&hy.scale(normal.y), bi)?
        .add(&hz.scale(normal.z), bi)?;
    let n1 = normal.x.abs() + normal.y.abs() + normal.z.abs();
    let eg = n1 * e
        + gamma(4)
            * (normal.x.abs() * hx.max_abs_coefficient()
                + normal.y.abs() * hy.max_abs_coefficient()
                + normal.z.abs() * hz.max_abs_coefficient());
    let up = 1.0 + gamma(8);
    let f_max = (f.max_abs_coefficient() + ef) / (w_lo * w_lo) * up;
    // (the square root's argument rounded down: `r2` carries one rounding, the difference
    // another, so a cancellation cannot raise the denominator)
    let radial = f_max / (radius + (r2 * (1.0 - gamma(2)) - f_max).max(0.0).sqrt()) * up;
    let plane = (g.max_abs_coefficient() + eg) / w_lo / normal_len * up;
    Ok(radial <= tol && plane <= tol)
}

/// Whether the point of the piece `h` (homogeneous about the centre, within `e`) at `s` of
/// it lies farther than `tol` from the circle's radius or plane by more than its
/// evaluation's rounding ([`on_circle`]'s witness).
fn circle_point_outside(
    h: &[Bernstein2; 4],
    e: f64,
    s: f64,
    normal: Vec3,
    normal_len: f64,
    radius: f64,
    tol: f64,
) -> bool {
    let [hx, hy, hz, hw] = h;
    let d = hw.degrees().0;
    // de Casteljau rounds once per level: within gamma_(2d) of the largest coefficient
    let eh = e + gamma(2 * d + 4) * (largest(h) + e);
    let w = hw.eval(s, 0.0);
    #[allow(clippy::neg_cmp_op_on_partial_ord)] // a NaN is no witness
    if !(w - eh > 0.0) {
        return false;
    }
    let x = Vec3::new(hx.eval(s, 0.0), hy.eval(s, 0.0), hz.eval(s, 0.0)) / w;
    let dist = x.length();
    // |x| moves by at most (sqrt(3) eh + |x| eh) / (w - eh) for the numerators' and the
    // weight's errors, doubled with the norms' and the products' own rounding
    let slack = 2.0 * (2.0 * eh + dist * eh) / (w - eh) + gamma(8) * (dist + radius);
    let radial = (dist - radius).abs() - slack;
    let plane = normal.dot(x).abs() / normal_len - slack;
    radial > tol || plane > tol
}

/// Most pieces [`meridian_collapse`] bounds in all (a resource bound; past it:
/// [`Collapse::Unproved`]).
pub const MAX_COLLAPSE_PIECES: usize = 1 << 12;
/// Most halvings of one Bézier span in [`meridian_collapse`].
pub const MAX_COLLAPSE_HALVINGS: usize = 30;

/// What [`meridian_collapse`] proves of a generatrix and an offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Collapse {
    /// The surface's profile bends to one side all along (and the generatrix keeps to one
    /// side of the axis, so the offset moves to the same side of it everywhere), and at
    /// every point the profile's radius of curvature less the offset is below the
    /// tolerance: the offset passes every point's centre of curvature.
    Everywhere,
    /// As [`Self::Everywhere`], but at every point the radius of curvature less the offset
    /// is at least the tolerance: no point's offset reaches its centre of curvature.
    Nowhere,
    /// Neither: the profile is not proved to bend to one side all along, or the generatrix
    /// to keep to one side of the axis (it crosses it, comes more than the tolerance past
    /// it at an end, or touches it inside other than where a piece ends), its radius of
    /// curvature comes to the offset plus the tolerance (or crosses it), or the bounds do
    /// not settle within [`MAX_COLLAPSE_HALVINGS`] and [`MAX_COLLAPSE_PIECES`].
    Unproved,
}

/// A point of the axis through `axis_point` along `axis` close to the foot of `near` on
/// it, and a bound on how far each of its coordinates is from a point of that axis. Not
/// upstream (solidcraft#18018's review: bounds taken about a far `axis_point` carry its
/// rounding, which grows with its distance from the geometry).
///
/// With `tau = axis . (near - axis_point) / |axis|^2` as rounded (any `tau` names a
/// point of the axis), `axis_point + tau axis` is held exactly as an expansion
/// ([`crate::geom::expansion`]) and rounded once per coordinate: off by at most
/// `gamma_2` of the sum of its components' magnitudes ([`crate::geom::expansion::to_f64`]),
/// charged as `gamma_4` of that sum (the sum and the bound rounded too). The bound is in
/// the frame of the point found, not of `axis_point`: far along the axis it stays the
/// rounding of a coordinate near `near`.
///
/// `None` for a `tau` that is not finite (a zero axis) or a product out of the expansion's
/// range ([`crate::geom::expansion::product`]).
#[must_use]
pub fn axis_foot(axis_point: Point3, axis: Vec3, near: Point3) -> Option<(Point3, f64)> {
    use crate::geom::expansion;
    let tau = axis.dot(near - axis_point) / axis.length_squared();
    if !tau.is_finite() {
        return None;
    }
    let mut foot = [0.0_f64; 3];
    let mut off = 0.0_f64;
    for (slot, (p, v)) in foot.iter_mut().zip([
        (axis_point.x, axis.x),
        (axis_point.y, axis.y),
        (axis_point.z, axis.z),
    ]) {
        let exact = expansion::add(&[p], &expansion::product(v, tau)?);
        *slot = expansion::to_f64(&exact);
        let size: f64 = exact.iter().map(|c| c.abs()).sum();
        off = off.max(gamma(4) * size);
    }
    let [x, y, z] = foot;
    (x.is_finite() && y.is_finite() && z.is_finite() && off.is_finite())
        .then(|| (Point3::new(x, y, z), off))
}

/// Whether the offset by `along` towards the centres of curvature of the profile of a
/// surface of revolution, generatrix `nurb`, about the axis through `axis_point` along
/// `axis`, passes every point's centre of curvature (the profile's radius of curvature
/// `rho` less `along` below `tol` all along) or none (`rho - along >= tol` all along).
/// `center` is a point near the generatrix (its arc's centre) the coefficients are taken
/// about, to keep them small. Not upstream (solidcraft#17907's review: the collapse of a
/// revolved arc was decided from its circle's radius and one curvature sample).
///
/// **The profile.** A surface of revolution is the revolution of its profile: the curve
/// `(rho(t), z(t))` its generatrix traces in a half-plane through the axis (`rho` the
/// distance from the axis, `z` the height along it). Its meridians are copies of the
/// profile, and the offset folds where it passes their centres of curvature. The bounds
/// are taken about a point of the axis near `center` ([`axis_foot`], its rounding charged),
/// never about `axis_point`, so a far `axis_point` adds only its own coordinates' rounding.
///
/// **The side.** The caller reads at one point which side of the profile the offset moves
/// to (`along > 0` towards the centre of curvature). It holds all along when the profile
/// bends to one side and the surface's normal keeps to one side of the profile: with
/// `S_t x S_theta = rho (rho' a - z' e_rho)` (`e_rho` the unit radial direction), the
/// normal is a fixed sign (of the parametrisation) times the profile's left normal
/// wherever `rho > 0`. So the generatrix must keep off the axis (in a meridian plane, its
/// signed distance from the axis keeps one sign: the normal flips where it crosses).
///
/// **A generatrix in a plane through the axis** (the plane through the axis and the
/// control point farthest from it, normal `m`, `G = m . (X - foot) W` with no coefficient
/// above its rounding and the plane's tilt times the span's reach along the axis, as
/// [`on_circle`]'s plane test) is its own profile, to the rounding the test can show. Its
/// signed radial coordinate `x = (a x m) . (X - foot)` must keep one sign on every piece
/// ([`one_sign`]: some Bernstein coefficient beyond its error with that sign, none beyond it
/// with the other). **PROVEN:** then `x` keeps that sign along the whole generatrix to
/// rounding, and within `tol` of the axis at an end of the generatrix: for a positive sign,
/// `x >= -2 e / w` on every piece that does not end the generatrix and
/// `x >= -max(tol, 2 e / w)` everywhere (`e` the coefficients' error, `w` the weight's lower
/// bound; [`one_sign`], `meridian_piece`). So the generatrix may end on the axis (a pole) or
/// up to `tol` past it (a pole snapped with noise, decided to the tolerance; only that
/// depth is bounded, not how far along the axis the part past it runs), and may touch the
/// axis inside only where a piece ends with a zero (a horn torus, whose touch is a knot or a
/// halving point; an interior touch elsewhere leaves a coefficient of the other sign and is
/// refused). Where
/// `x` keeps its sign the normal `N ~ x (x' a - z' e)` keeps its side of the profile; past a
/// tangential touch it does not change. With
/// `A = W H' - W' H` (so `X' = A / W^2`) and `B = (A x A') . m` the curvature is
/// `B W^2 / |A|^3`, `rho = |A|^3 / (W^2 |B|)`; `B` keeps one strict sign, and
/// `rho < q` (`q = along + tol > 0`) exactly where `(A . A)^3 - q^2 W^4 B^2 < 0` (degree
/// `12 r - 6`).
///
/// **Any other generatrix** is decided on its profile. With the axis `v` as given (no
/// rounding of its direction), `Y = X - foot = H / W` (about the foot), `S = |v x Y|^2 =
/// sigma / W^2` and `Z = v . Y = zeta / W` are the profile scaled by `|v|`; `sigma` must be
/// strictly positive (off the axis). With `alpha = sigma' W - 2 sigma W'`,
/// `beta = zeta' W - zeta W'`, `alpha_2 = alpha' W - 3 alpha W'`, `beta_2 = beta' W - 2
/// beta W'` (so `S' = alpha / W^3`, `S'' = alpha_2 / W^4`, `Z' = beta / W^2`, `Z'' = beta_2
/// / W^3`), the profile's curvature `(rho' Z'' - Z' rho'') / |(rho', Z')|^3` with
/// `rho = sqrt S` is `2 K / D^(3/2)`, `K = 2 S S' Z'' - 2 S Z' S'' + Z' S'^2 = kappa / W^8`,
/// `D = S'^2 + 4 S Z'^2 = delta / W^6`, where `kappa = 2 sigma alpha beta_2 - 2 sigma beta
/// alpha_2 + beta alpha^2` and `delta = alpha^2 + 4 sigma beta^2`. `kappa` keeps one strict
/// sign, and `|v| rho < |v| q` exactly where `delta^3 - 4 |v|^2 q^2 kappa^2 W^2 < 0` (degree
/// `18 r - 6`). A generatrix that meets the axis is not proved this way (`sigma` comes to
/// zero), nor is a profile with an inflection or a straight one (a circle square to the
/// axis).
///
/// **Both.** For `q <= 0` (rounding keeps the sign of a sum, and gives zero only for an
/// exact zero) no point collapses: `rho - along >= rho + tol >= tol`. A piece whose test
/// polynomial has every coefficient below minus its error collapses, one with every
/// coefficient above its error does not, and pieces must agree (and on the signs above); an
/// undecided piece is halved (de Casteljau at `1/2`) at most [`MAX_COLLAPSE_HALVINGS`]
/// deep and [`MAX_COLLAPSE_PIECES`] pieces in all. No recursion: the pieces wait on a work
/// list. A degree past [`MAX_DEGREE`]'s reach is [`Collapse::Unproved`].
///
/// **The coefficients' error**, from the spans' own: each derivative (`n` times
/// differences) `2 n e + gamma_2 2 n` of the largest coefficient; each product as
/// [`product`]; each difference or sum of two `gamma_1`, of three `gamma_2`, a
/// combination of three scaled terms `gamma_3` (`B`'s, `x`'s and `G`'s, with the foot's
/// offset, `gamma_4`/`gamma_5`); a scaling by 3 `gamma_1` (by 2 or 4 exact); `q^2` and its
/// scaling `gamma_4` of `q^2 |W^4 B^2|`, and `4 |v|^2 q^2` and its scaling `gamma_9`; the
/// foot's distance from the axis times the weights; the direction of `a x m` (`gamma_6`
/// of `|a|_1 |m|_1` times the reach); each halving `gamma_(2r + 4)` as [`on_circle`]'s.
///
/// # Errors
///
/// `Unsupported` for an axis, a point, an offset or a tolerance that is not finite, an
/// axis whose length is zero or not finite, or a negative tolerance; those of
/// [`RailSpans::about`] and of the products; [`SolidError::Cancelled`] when `k`'s cancel
/// token is set (polled once per piece).
pub fn meridian_collapse(
    k: &Kernel,
    nurb: &NurbCurve,
    axis_point: Point3,
    axis: Vec3,
    center: Point3,
    along: f64,
    tol: f64,
) -> Result<Collapse> {
    let finite = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
    if !finite(axis_point) || !finite(axis) || !finite(center) || !along.is_finite() {
        return Err(failed("an axis, centre or offset that is not finite"));
    }
    #[allow(clippy::neg_cmp_op_on_partial_ord)] // refuses NaN too
    if !(tol >= 0.0) || !tol.is_finite() {
        return Err(failed("a tolerance that is not finite and non-negative"));
    }
    let axis_len = axis.length();
    #[allow(clippy::neg_cmp_op_on_partial_ord)] // refuses NaN too
    if !(axis_len > 0.0) || !axis_len.is_finite() {
        return Err(failed("an axis whose length is zero or not finite"));
    }
    let a = axis / axis_len;
    // (finite control points and positive weights: checked here first)
    let spans = RailSpans::about(nurb, center)?;
    // the bounds are taken about a point of the axis near the generatrix, never about a far
    // `axis_point` (solidcraft#18018's review)
    let Some((foot, off)) = axis_foot(axis_point, axis, center) else {
        return Ok(Collapse::Unproved);
    };
    let bi = Binomials::new();
    let q = along + tol;
    if let Some(plane) = meridian_plane(nurb, &spans, a, foot, off, center, &bi)? {
        if 12 * nurb.degree() > MAX_DEGREE + 6 {
            return Ok(Collapse::Unproved);
        }
        return collapse_pieces(k, &spans, |h, e, ends| {
            meridian_piece(h, e, ends, &plane, q, tol, &bi)
        });
    }
    if 18 * nurb.degree() > MAX_DEGREE + 6 {
        return Ok(Collapse::Unproved);
    }
    let mut about_foot = RailSpans::about(nurb, foot)?;
    // `foot` is within `off` of a point of the axis in every coordinate: about that point
    // each coefficient moves by at most `off` times the weight's
    let w_max = about_foot
        .spans
        .iter()
        .map(|(_, h)| h[3].max_abs_coefficient())
        .fold(0.0_f64, f64::max);
    about_foot.err += gamma(2) * about_foot.err + off * (w_max + about_foot.err) * (1.0 + gamma(3));
    // `4 |v|^2 q^2`, its rounding charged in `profile_piece`
    let f = 4.0 * axis.length_squared() * (q * q);
    collapse_pieces(k, &about_foot, |h, e, _| {
        profile_piece(h, e, axis, q, f, &bi)
    })
}

/// A generatrix proved in a plane through the axis ([`meridian_collapse`]): the plane's
/// normal `m`, the radial direction `e = a x m` in it, `e . (center - foot)` and its error,
/// and the error factor of `e`'s direction.
struct Meridian {
    m: Vec3,
    e: Vec3,
    s_e: f64,
    es_e: f64,
    cp_one: f64,
    dir: f64,
}

/// [`meridian_collapse`]'s plane test: the plane through the axis (unit `a` through `foot`,
/// within `off` of it) and the control point farthest from it, if every span lies in it to
/// the test's rounding; `None` otherwise.
fn meridian_plane(
    nurb: &NurbCurve,
    spans: &RailSpans,
    a: Vec3,
    foot: Point3,
    off: f64,
    center: Point3,
    bi: &Binomials,
) -> Result<Option<Meridian>> {
    let mut farthest: Option<(f64, Vec3)> = None;
    for c in nurb.cpts() {
        let w = if c.w == NOW { 1.0 } else { c.w };
        let v = a.cross(Point3::new(c.x / w, c.y / w, c.z / w) - foot);
        let l = v.length();
        if l.is_finite() && farthest.is_none_or(|(best, _)| l > best) {
            farthest = Some((l, v));
        }
    }
    let Some((l, v)) = farthest.filter(|&(l, _)| l > 0.0) else {
        return Ok(None);
    };
    let m = v / l;
    let one = |v: Vec3| v.x.abs() + v.y.abs() + v.z.abs();
    let cp = center - foot;
    let s = m.dot(cp);
    let z = a.dot(cp);
    // `n . (center - foot)`: its rounding, and the foot's distance from the axis
    let shift = |n: Vec3| {
        gamma(4)
            * (n.x.abs() * (center.x.abs() + foot.x.abs())
                + n.y.abs() * (center.y.abs() + foot.y.abs())
                + n.z.abs() * (center.z.abs() + foot.z.abs()))
            + one(n) * off * (1.0 + gamma(3))
    };
    let es = shift(m);
    // (the direction of `a`, normalised from the axis, is within `gamma_2` of it)
    let tilt = m.dot(a).abs() + gamma(5) * one(m) * one(a);
    for (_, [hx, hy, hz, hw]) in &spans.spans {
        let g = hx
            .scale(m.x)
            .add(&hy.scale(m.y), bi)?
            .add(&hz.scale(m.z), bi)?
            .add(&hw.scale(s), bi)?;
        let w_max = hw.max_abs_coefficient();
        let size = m.x.abs() * hx.max_abs_coefficient()
            + m.y.abs() * hy.max_abs_coefficient()
            + m.z.abs() * hz.max_abs_coefficient()
            + s.abs() * w_max;
        let reach = a.x.abs() * hx.max_abs_coefficient()
            + a.y.abs() * hy.max_abs_coefficient()
            + a.z.abs() * hz.max_abs_coefficient()
            + z.abs() * w_max
            + (one(a) + z.abs()) * spans.err;
        let eg = (one(m) + s.abs()) * spans.err
            + es * (w_max + spans.err)
            + gamma(5) * size
            + tilt * reach;
        #[allow(clippy::neg_cmp_op_on_partial_ord)] // refuses NaN too
        if !(g.max_abs_coefficient() <= eg) {
            return Ok(None);
        }
    }
    let e = a.cross(m);
    Ok(Some(Meridian {
        m,
        e,
        s_e: e.dot(cp),
        es_e: shift(e),
        cp_one: one(cp),
        // `a x m` against the exact axis's: `a` within `gamma_2` of its direction, the
        // cross product's rounding `gamma_2`, both of `|a|_1 |m|_1`
        dir: gamma(6) * one(a) * one(m),
    }))
}

/// [`meridian_collapse`]'s pieces: the spans' work list, each piece `decide`d (with whether
/// it starts the generatrix and whether it ends it), halved where undecided. Every decided
/// piece must agree on the signs it reports and on the collapse.
fn collapse_pieces(
    k: &Kernel,
    spans: &RailSpans,
    decide: impl Fn(&[Bernstein2; 4], f64, [bool; 2]) -> Result<PieceCollapse>,
) -> Result<Collapse> {
    let last = spans.spans.len().saturating_sub(1);
    let mut work: Vec<([Bernstein2; 4], f64, usize, [bool; 2])> = spans
        .spans
        .iter()
        .enumerate()
        .rev()
        .map(|(i, (_, h))| (h.clone(), spans.err, 0, [i == 0, i == last]))
        .collect();
    let mut side: Option<[bool; 2]> = None;
    let mut answer: Option<bool> = None;
    let mut pieces = 0_usize;
    while let Some((h, e, depth, ends)) = work.pop() {
        k.check_cancelled(OP)?;
        pieces += 1;
        if pieces > MAX_COLLAPSE_PIECES {
            return Ok(Collapse::Unproved);
        }
        match decide(&h, e, ends)? {
            PieceCollapse::Decided(signs, collapses) => {
                if side.is_some_and(|b| b != signs) || answer.is_some_and(|c| c != collapses) {
                    return Ok(Collapse::Unproved);
                }
                side = Some(signs);
                answer = Some(collapses);
            }
            PieceCollapse::Halve => {
                if depth >= MAX_COLLAPSE_HALVINGS {
                    return Ok(Collapse::Unproved);
                }
                let d = h[3].degrees().0;
                let e = e + gamma(2 * d + 4) * (largest(&h) + e);
                let halves = h.map(|c| c.split_u(0.5));
                let left = halves.clone().map(|(l, _)| l);
                let right = halves.map(|(_, r)| r);
                work.push((right, e, depth + 1, [false, ends[1]]));
                work.push((left, e, depth + 1, [ends[0], false]));
            }
            PieceCollapse::Unbounded => return Ok(Collapse::Unproved),
        }
    }
    Ok(match answer {
        Some(true) => Collapse::Everywhere,
        Some(false) => Collapse::Nowhere,
        None => Collapse::Unproved,
    })
}

/// What a piece of [`meridian_collapse`] finds.
enum PieceCollapse {
    /// The signs that must hold all along (the curvature's, `true` positive, and the
    /// radial coordinate's in a meridian plane; `true` for a profile off the axis), and
    /// whether the offset passes every centre of curvature there.
    Decided([bool; 2], bool),
    /// Not settled at this size: halve it.
    Halve,
    /// A bound that is not finite: halving does not help.
    Unbounded,
}

/// The sign `p` keeps on its piece (`true` positive): some coefficient beyond `err` of zero
/// with that sign, none beyond `err` with the other, except a coefficient at an end of the
/// generatrix (`ends`: the piece starts it, ends it), which may be within `end_allow`
/// (at least `err`) of zero on either side. `None` if no coefficient is beyond `err`, two
/// beyond it have opposite signs, or one is NaN.
///
/// **PROVEN (the bound it gives, for a positive answer; a negative one is its mirror).**
/// Each true coefficient is within `err` of the computed one, so it is at least `-2 err`, or
/// `-(end_allow + err)` at an end of the generatrix. The Bernstein basis is non-negative and
/// sums to one, so on the piece `p >= -2 err (1 - B_end) - (end_allow + err) B_end >=
/// -max(2 err, end_allow + err)`, `B_end` the end coefficients' basis functions summed.
fn one_sign(p: &Bernstein2, err: f64, ends: [bool; 2], end_allow: f64) -> Option<bool> {
    let c = p.coefficients();
    let last = c.len().saturating_sub(1);
    let mut sign: Option<bool> = None;
    for (i, &x) in c.iter().enumerate() {
        let at_end = (i == 0 && ends[0]) || (i == last && ends[1]);
        // at an end of the generatrix, a zero within the end's allowance signs nothing (a
        // pole, to the tolerance)
        if at_end && x.abs() <= end_allow.max(err) {
            continue;
        }
        let s = if x > err {
            true
        } else if x < -err {
            false
        } else if x.abs() <= err {
            // a zero within rounding signs nothing (the generatrix touching the axis where
            // a piece ends, as a horn torus's)
            continue;
        } else {
            // NaN
            return None;
        };
        if sign.is_some_and(|t| t != s) {
            return None;
        }
        sign = Some(s);
    }
    sign
}

/// [`meridian_collapse`] on one piece `h` of a generatrix in the plane `plane`
/// (homogeneous about the centre, within `e`; `ends` as [`collapse_pieces`]'), with
/// `q = along + tol`: the radial coordinate's sign, then [`collapse_piece`].
fn meridian_piece(
    h: &[Bernstein2; 4],
    e: f64,
    ends: [bool; 2],
    plane: &Meridian,
    q: f64,
    tol: f64,
    bi: &Binomials,
) -> Result<PieceCollapse> {
    let [hx, hy, hz, hw] = h;
    let n = plane.e;
    let x = hx
        .scale(n.x)
        .add(&hy.scale(n.y), bi)?
        .add(&hz.scale(n.z), bi)?
        .add(&hw.scale(plane.s_e), bi)?;
    let w_max = hw.max_abs_coefficient();
    let (mx, my, mz) = (
        hx.max_abs_coefficient(),
        hy.max_abs_coefficient(),
        hz.max_abs_coefficient(),
    );
    let size = n.x.abs() * mx + n.y.abs() * my + n.z.abs() * mz + plane.s_e.abs() * w_max;
    let one_n = n.x.abs() + n.y.abs() + n.z.abs();
    let ex = (one_n + plane.s_e.abs()) * e
        + plane.es_e * (w_max + e)
        + gamma(5) * size
        + plane.dir * (mx + my + mz + 3.0 * e + plane.cp_one * (w_max + e));
    if !ex.is_finite() {
        return Ok(PieceCollapse::Unbounded);
    }
    // At an end of the generatrix the radial coordinate may come up to `tol` past the axis
    // (a pole snapped to the axis, to the tolerance; solidcraft#18018's second review):
    // `w_lo` bounds the true weight below on the piece, so a true end coefficient
    // `>= -(end_allow + ex) >= -tol w_lo` bounds `x / W >= -tol` there ([`one_sign`]). The
    // rounding of `end_allow` (at most four roundings up) is charged by the `1 - gamma_4`.
    let w_lo = hw.min_coefficient() - e;
    let end_allow = if w_lo > 0.0 {
        tol * w_lo * (1.0 - gamma(4)) - ex
    } else {
        ex
    };
    let Some(outside) = one_sign(&x, ex, ends, end_allow) else {
        return Ok(PieceCollapse::Halve);
    };
    Ok(match collapse_piece(h, e, plane.m, q, bi)? {
        PieceCollapse::Decided([bends, _], collapses) => {
            PieceCollapse::Decided([bends, outside], collapses)
        }
        other => other,
    })
}

/// A derivative (`n` times differences) and its error.
fn deriv(p: &Bernstein2, e: f64) -> (Bernstein2, f64) {
    #[allow(clippy::cast_precision_loss)]
    let n = 2.0 * p.degrees().0 as f64;
    (p.deriv_u(), n * e + gamma(2) * n * p.max_abs_coefficient())
}

/// `a b - c d` (each with its error) and its error.
fn diff(
    a: (&Bernstein2, f64),
    b: (&Bernstein2, f64),
    c: (&Bernstein2, f64),
    d: (&Bernstein2, f64),
    bi: &Binomials,
) -> Result<(Bernstein2, f64)> {
    let (l, el) = product(a.0, a.1, b.0, b.1, bi)?;
    let (r, er) = product(c.0, c.1, d.0, d.1, bi)?;
    let size = l.max_abs_coefficient() + r.max_abs_coefficient();
    Ok((l.sub(&r, bi)?, el + er + gamma(1) * size))
}

/// The sign of `p` beyond `err` on the piece (`Some(true)` positive), or what to do.
fn strict_sign(p: &Bernstein2, err: f64) -> std::result::Result<bool, PieceCollapse> {
    let (lo, hi) = (p.min_coefficient(), p.max_coefficient());
    if !err.is_finite() || !lo.is_finite() || !hi.is_finite() {
        Err(PieceCollapse::Unbounded)
    } else if lo > err {
        Ok(true)
    } else if hi < -err {
        Ok(false)
    } else {
        Err(PieceCollapse::Halve)
    }
}

/// [`meridian_collapse`] on one piece `h` of any generatrix, decided on its profile
/// (homogeneous about the foot, within `e`), with the axis `v` as given, `q = along + tol`
/// and `f = 4 |v|^2 q^2` as rounded.
fn profile_piece(
    h: &[Bernstein2; 4],
    e: f64,
    v: Vec3,
    q: f64,
    f: f64,
    bi: &Binomials,
) -> Result<PieceCollapse> {
    let [hx, hy, hz, hw] = h;
    if hw.degrees().0 == 0 {
        // a point: no tangent
        return Ok(PieceCollapse::Unbounded);
    }
    // `x0 p0 + x1 p1 + x2 p2` of the coordinates (exact factors) and its error
    let lin = |terms: [(&Bernstein2, f64); 3]| -> Result<(Bernstein2, f64)> {
        let [(p0, x0), (p1, x1), (p2, x2)] = terms;
        let r = p0
            .scale(x0)
            .add(&p1.scale(x1), bi)?
            .add(&p2.scale(x2), bi)?;
        let size = x0.abs() * p0.max_abs_coefficient()
            + x1.abs() * p1.max_abs_coefficient()
            + x2.abs() * p2.max_abs_coefficient();
        Ok((r, (x0.abs() + x1.abs() + x2.abs()) * e + gamma(3) * size))
    };
    // `v x H` and `v . H`
    let (c1, ec1) = lin([(hz, v.y), (hy, -v.z), (hx, 0.0)])?;
    let (c2, ec2) = lin([(hx, v.z), (hz, -v.x), (hy, 0.0)])?;
    let (c3, ec3) = lin([(hy, v.x), (hx, -v.y), (hz, 0.0)])?;
    let (zeta, ezeta) = lin([(hx, v.x), (hy, v.y), (hz, v.z)])?;
    let (s1, es1) = product(&c1, ec1, &c1, ec1, bi)?;
    let (s2, es2) = product(&c2, ec2, &c2, ec2, bi)?;
    let (s3, es3) = product(&c3, ec3, &c3, ec3, bi)?;
    let sigma = s1.add(&s2, bi)?.add(&s3, bi)?;
    let esig = es1
        + es2
        + es3
        + gamma(2)
            * (s1.max_abs_coefficient() + s2.max_abs_coefficient() + s3.max_abs_coefficient());
    // off the axis
    match strict_sign(&sigma, esig) {
        Ok(true) => {}
        Ok(false) => return Ok(PieceCollapse::Unbounded),
        Err(other) => return Ok(other),
    }
    let (dw, edw) = deriv(hw, e);
    let (ds, eds) = deriv(&sigma, esig);
    let (dz, edz) = deriv(&zeta, ezeta);
    let sigma2 = sigma.scale(2.0);
    let (alpha, ea) = diff((&ds, eds), (hw, e), (&sigma2, 2.0 * esig), (&dw, edw), bi)?;
    let (beta, eb) = diff((&dz, edz), (hw, e), (&zeta, ezeta), (&dw, edw), bi)?;
    let (da, eda) = deriv(&alpha, ea);
    let (db, edb) = deriv(&beta, eb);
    let alpha3 = alpha.scale(3.0);
    let ea3 = 3.0 * ea + gamma(1) * alpha3.max_abs_coefficient();
    let (alpha2, ea2) = diff((&da, eda), (hw, e), (&alpha3, ea3), (&dw, edw), bi)?;
    let beta2x = beta.scale(2.0);
    let (beta2, eb2) = diff((&db, edb), (hw, e), (&beta2x, 2.0 * eb), (&dw, edw), bi)?;
    // kappa = 2 sigma alpha beta_2 - 2 sigma beta alpha_2 + beta alpha^2
    let (ab2, eab2) = product(&alpha, ea, &beta2, eb2, bi)?;
    let (t1, et1) = product(&sigma, esig, &ab2, eab2, bi)?;
    let (ba2, eba2) = product(&beta, eb, &alpha2, ea2, bi)?;
    let (t2, et2) = product(&sigma, esig, &ba2, eba2, bi)?;
    let (aa, eaa) = product(&alpha, ea, &alpha, ea, bi)?;
    let (t3, et3) = product(&beta, eb, &aa, eaa, bi)?;
    let d12 = t1.sub(&t2, bi)?;
    let ed12 = et1 + et2 + gamma(1) * (t1.max_abs_coefficient() + t2.max_abs_coefficient());
    let kappa = d12.scale(2.0).add(&t3, bi)?;
    let ek =
        2.0 * ed12 + et3 + gamma(1) * (2.0 * d12.max_abs_coefficient() + t3.max_abs_coefficient());
    let bends = match strict_sign(&kappa, ek) {
        Ok(b) => b,
        Err(other) => return Ok(other),
    };
    // (rounding keeps a sum's sign: `q <= 0` is `along + tol <= 0` exactly)
    if q <= 0.0 {
        return Ok(PieceCollapse::Decided([bends, true], false));
    }
    // delta = alpha^2 + 4 sigma beta^2
    let (bb, ebb) = product(&beta, eb, &beta, eb, bi)?;
    let (sbb, esbb) = product(&sigma, esig, &bb, ebb, bi)?;
    let sbb4 = sbb.scale(4.0);
    let delta = aa.add(&sbb4, bi)?;
    let ed = eaa + 4.0 * esbb + gamma(1) * (aa.max_abs_coefficient() + sbb4.max_abs_coefficient());
    let (d2, ed2) = product(&delta, ed, &delta, ed, bi)?;
    let (d3, ed3) = product(&d2, ed2, &delta, ed, bi)?;
    let (kk, ekk) = product(&kappa, ek, &kappa, ek, bi)?;
    let (ww, eww) = product(hw, e, hw, e, bi)?;
    let (kw, ekw) = product(&kk, ekk, &ww, eww, bi)?;
    // `f` as rounded: `|v|^2` `gamma_3`, `q` `gamma_1` twice, `q q` and the product
    // `gamma_1` each (the 4 exact); the scaling `gamma_1`
    let t = kw.scale(f);
    let et = f * ekw + gamma(9) * f * (kw.max_abs_coefficient() + ekw);
    let p = d3.sub(&t, bi)?;
    let ep = ed3 + et + gamma(1) * (d3.max_abs_coefficient() + t.max_abs_coefficient());
    Ok(match strict_sign(&p, ep) {
        Ok(above) => PieceCollapse::Decided([bends, true], !above),
        Err(other) => other,
    })
}

/// [`meridian_collapse`] on one piece `h` (homogeneous about the centre, within `e`), with
/// the plane's normal `m` and `q = along + tol`: `B`'s sign and the curvature test (the
/// radial sign is [`meridian_piece`]'s).
fn collapse_piece(
    h: &[Bernstein2; 4],
    e: f64,
    m: Vec3,
    q: f64,
    bi: &Binomials,
) -> Result<PieceCollapse> {
    let [hx, hy, hz, hw] = h;
    if hw.degrees().0 == 0 {
        // a point: no tangent
        return Ok(PieceCollapse::Unbounded);
    }
    let (dw, edw) = deriv(hw, e);
    // A = W H' - W' H, a component at a time
    let tangent = |hi: &Bernstein2| {
        let (dh, edh) = deriv(hi, e);
        diff((hw, e), (&dh, edh), (&dw, edw), (hi, e), bi)
    };
    let (ax, eax) = tangent(hx)?;
    let (ay, eay) = tangent(hy)?;
    let (az, eaz) = tangent(hz)?;
    let (dax, edax) = deriv(&ax, eax);
    let (day, eday) = deriv(&ay, eay);
    let (daz, edaz) = deriv(&az, eaz);
    let (cx, ecx) = diff((&ay, eay), (&daz, edaz), (&az, eaz), (&day, eday), bi)?;
    let (cy, ecy) = diff((&az, eaz), (&dax, edax), (&ax, eax), (&daz, edaz), bi)?;
    let (cz, ecz) = diff((&ax, eax), (&day, eday), (&ay, eay), (&dax, edax), bi)?;
    let b = cx
        .scale(m.x)
        .add(&cy.scale(m.y), bi)?
        .add(&cz.scale(m.z), bi)?;
    let eb = m.x.abs() * ecx
        + m.y.abs() * ecy
        + m.z.abs() * ecz
        + gamma(4)
            * (m.x.abs() * cx.max_abs_coefficient()
                + m.y.abs() * cy.max_abs_coefficient()
                + m.z.abs() * cz.max_abs_coefficient());
    let (lo, hi) = (b.min_coefficient(), b.max_coefficient());
    if !eb.is_finite() || !lo.is_finite() || !hi.is_finite() {
        return Ok(PieceCollapse::Unbounded);
    }
    let bends = if lo > eb {
        true
    } else if hi < -eb {
        false
    } else {
        return Ok(PieceCollapse::Halve);
    };
    // (rounding keeps a sum's sign: `q <= 0` is `along + tol <= 0` exactly)
    if q <= 0.0 {
        return Ok(PieceCollapse::Decided([bends, true], false));
    }
    let (xx, exx) = product(&ax, eax, &ax, eax, bi)?;
    let (yy, eyy) = product(&ay, eay, &ay, eay, bi)?;
    let (zz, ezz) = product(&az, eaz, &az, eaz, bi)?;
    let aa = xx.add(&yy, bi)?.add(&zz, bi)?;
    let eaa = exx
        + eyy
        + ezz
        + gamma(2)
            * (xx.max_abs_coefficient() + yy.max_abs_coefficient() + zz.max_abs_coefficient());
    let (aa2, eaa2) = product(&aa, eaa, &aa, eaa, bi)?;
    let (aa3, eaa3) = product(&aa2, eaa2, &aa, eaa, bi)?;
    let (w2, ew2) = product(hw, e, hw, e, bi)?;
    let (w4, ew4) = product(&w2, ew2, &w2, ew2, bi)?;
    let (b2, eb2) = product(&b, eb, &b, eb, bi)?;
    let (wb, ewb) = product(&w4, ew4, &b2, eb2, bi)?;
    let q2 = q * q;
    let t = wb.scale(q2);
    let et = q2 * ewb + gamma(4) * q2 * wb.max_abs_coefficient();
    let p = aa3.sub(&t, bi)?;
    let ep = eaa3 + et + gamma(1) * (aa3.max_abs_coefficient() + t.max_abs_coefficient());
    let (lo, hi) = (p.min_coefficient(), p.max_coefficient());
    if !ep.is_finite() || !lo.is_finite() || !hi.is_finite() {
        return Ok(PieceCollapse::Unbounded);
    }
    Ok(if hi < -ep {
        PieceCollapse::Decided([bends, true], true)
    } else if lo > ep {
        PieceCollapse::Decided([bends, true], false)
    } else {
        PieceCollapse::Halve
    })
}

#[cfg(test)]
mod turn_tests {
    #![allow(clippy::unwrap_used)] // test helpers
    use super::*;

    fn curve(points: &[[f64; 4]], degree: usize, knots: &[f64]) -> NurbCurve {
        let pw = points
            .iter()
            .map(|&[x, y, z, w]| CPoint::new(x * w, y * w, z * w, w))
            .collect();
        NurbCurve::new(pw, degree, knots.to_vec()).unwrap()
    }

    /// A quarter circle about the z axis (rational, exact) turns one way: counter-clockwise
    /// seen from +z, clockwise seen from -z, and the other way when it runs backwards.
    #[test]
    fn a_quarter_circle_turns_one_way_round_its_axis() {
        let h = core::f64::consts::FRAC_1_SQRT_2;
        let pts = [
            [1.0, 0.0, 3.0, 1.0],
            [1.0, 1.0, 3.0, h],
            [0.0, 1.0, 3.0, 1.0],
        ];
        let knots = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        let c = curve(&pts, 2, &knots);
        let (o, z) = (Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0));
        assert_eq!(turn_about_axis(&c, o, z, [0.0, 1.0]).unwrap(), Some(1.0));
        assert_eq!(
            turn_about_axis(&c, o, z * -2.0, [0.0, 1.0]).unwrap(),
            Some(-1.0)
        );
        assert_eq!(turn_about_axis(&c, o, z, [0.25, 0.5]).unwrap(), Some(1.0));
        let mut back = pts;
        back.reverse();
        let b = curve(&back, 2, &knots);
        assert_eq!(turn_about_axis(&b, o, z, [0.0, 1.0]).unwrap(), Some(-1.0));
    }

    /// A curve that swings round the axis and comes back (as a radial hole's rim does about
    /// the wall's axis) is not proved, nor is a line through the axis, which stands still
    /// about it; the part before the turn is.
    #[test]
    fn a_curve_that_turns_back_or_stands_still_is_not_proved() {
        let (o, z) = (Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0));
        let knots = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        // from angle 0 up past 60 degrees and back to about 27
        let back = curve(
            &[
                [1.0, 0.0, 0.0, 1.0],
                [0.0, 3.0, 0.0, 1.0],
                [1.0, 0.5, 0.0, 1.0],
            ],
            2,
            &knots,
        );
        assert_eq!(turn_about_axis(&back, o, z, [0.0, 1.0]).unwrap(), None);
        assert_eq!(turn_about_axis(&back, o, z, [0.0, 0.2]).unwrap(), Some(1.0));
        let radial = curve(
            &[[1.0, 0.0, 0.0, 1.0], [2.0, 0.0, 1.0, 1.0]],
            1,
            &[0.0, 0.0, 1.0, 1.0],
        );
        assert_eq!(turn_about_axis(&radial, o, z, [0.0, 1.0]).unwrap(), None);
        assert!(turn_about_axis(&radial, o, z, [1.0, 0.0]).is_err());
    }
}

#[cfg(test)]
mod circle_tests {
    #![allow(clippy::unwrap_used)] // test helpers
    use super::*;

    /// The quarter circle r 2 about (5, 0, 1) in the plane z = 1, one rational quadratic
    /// arc, its middle control point moved out by `bulge` of its distance from the centre.
    fn quarter(bulge: f64) -> NurbCurve {
        let h = core::f64::consts::FRAC_1_SQRT_2;
        let m = 2.0 * (1.0 + bulge);
        let pts = [
            [7.0, 0.0, 1.0, 1.0],
            [5.0 + m, m, 1.0, h],
            [5.0, 2.0, 1.0, 1.0],
        ];
        let pw = pts
            .iter()
            .map(|&[x, y, z, w]| CPoint::new(x * w, y * w, z * w, w))
            .collect();
        NurbCurve::new(pw, 2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]).unwrap()
    }

    const C: Point3 = Point3::new(5.0, 0.0, 1.0);
    const Z: Vec3 = Vec3::new(0.0, 0.0, 1.0);

    fn kernel() -> Kernel {
        Kernel::default()
    }

    /// An exact arc is proved on its circle at once; at a tolerance of zero (its rounding
    /// is never below it, and no point is off by more) it is neither proved nor refuted.
    #[test]
    fn an_exact_arc_is_proved_on_its_circle() {
        let c = quarter(0.0);
        assert_eq!(
            on_circle(&kernel(), &c, C, Z, 2.0, 1e-9).unwrap(),
            OnCircle::Within
        );
        assert_eq!(
            on_circle(&kernel(), &c, C, Z * 3.0, 2.0, 1e-12).unwrap(),
            OnCircle::Within
        );
        assert_eq!(
            on_circle(&kernel(), &c, C, Z, 2.0, 0.0).unwrap(),
            OnCircle::Unproved
        );
    }

    /// A bulge of 1e-4 of the radius is found past a tolerance of 1e-7 with its point as a
    /// witness, and proved within a tolerance above the bulge's height; another radius,
    /// centre or plane is refuted.
    #[test]
    fn a_bulge_or_another_circle_is_refuted() {
        let c = quarter(1e-4);
        let OnCircle::Outside(t) = on_circle(&kernel(), &c, C, Z, 2.0, 1e-7).unwrap() else {
            panic!("the bulge is a witness");
        };
        assert!((0.0..=1.0).contains(&t), "{t}");
        assert_eq!(
            on_circle(&kernel(), &c, C, Z, 2.0, 1e-3).unwrap(),
            OnCircle::Within
        );
        let exact = quarter(0.0);
        for (center, normal, radius) in [
            (C, Z, 2.001),
            (Point3::new(5.0, 0.0, 1.01), Z, 2.0),
            (C, Vec3::new(0.0, 0.1, 1.0), 2.0),
        ] {
            assert!(
                matches!(
                    on_circle(&kernel(), &exact, center, normal, radius, 1e-7).unwrap(),
                    OnCircle::Outside(_)
                ),
                "{center:?} {normal:?} {radius}"
            );
        }
    }

    /// An exact arc far from the origin is still proved at a tolerance of 1e-9: the
    /// translation about the centre rounds as a length (`far` times a weight), not as an
    /// area (`far` times a coordinate: a radial bound of about 2e-8 here, which left the
    /// arc unproved).
    #[test]
    fn an_exact_arc_far_from_the_origin_is_proved() {
        let far = Vec3::new(1e4, 1e4, 0.0);
        let c = quarter(0.0);
        let moved = c
            .cpts()
            .iter()
            .map(|p| CPoint::new(p.x + far.x * p.w, p.y + far.y * p.w, p.z + far.z * p.w, p.w))
            .collect();
        let moved = NurbCurve::new(moved, 2, c.knots().to_vec()).unwrap();
        assert_eq!(
            on_circle(&kernel(), &moved, C + far, Z, 2.0, 1e-9).unwrap(),
            OnCircle::Within
        );
    }

    /// Non-finite or degenerate circles and tolerances are refused, not answered.
    #[test]
    fn a_circle_that_is_not_finite_is_refused() {
        let c = quarter(0.0);
        assert!(on_circle(&kernel(), &c, C, Z, f64::NAN, 1e-7).is_err());
        assert!(on_circle(&kernel(), &c, C, Z, 0.0, 1e-7).is_err());
        assert!(on_circle(&kernel(), &c, C, Vec3::new(0.0, 0.0, 0.0), 2.0, 1e-7).is_err());
        assert!(on_circle(&kernel(), &c, C, Z, 2.0, f64::NAN).is_err());
        assert!(on_circle(&kernel(), &c, C, Z, 2.0, -1.0).is_err());
    }

    /// solidcraft#17907's review: a finite normal whose length overflows, `(0, 0, 1e200)`,
    /// is refused. Divided by its infinite length every plane bound was zero, and an arc a
    /// unit off the plane through `(5, 0, 0)` (at the radius `sqrt 5` from it) was `Within`.
    #[test]
    fn a_normal_whose_length_overflows_is_refused() {
        let c = quarter(0.0);
        let r = on_circle(
            &kernel(),
            &c,
            Point3::new(5.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1e200),
            5.0_f64.sqrt(),
            1e-7,
        );
        assert!(matches!(r, Err(SolidError::Unsupported { .. })), "{r:?}");
        // the same normal at a length that does not overflow finds the unit gap
        assert!(matches!(
            on_circle(
                &kernel(),
                &c,
                Point3::new(5.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, 1e100),
                5.0_f64.sqrt(),
                1e-7
            )
            .unwrap(),
            OnCircle::Outside(_)
        ));
    }

    /// solidcraft#17907's review: a NaN control point or weight is refused before any bound
    /// is taken (`f64::max` passes a NaN over, so the bounds alone missed it).
    #[test]
    fn a_curve_that_is_not_finite_is_refused() {
        let good = quarter(0.0);
        for (i, field) in [(1, 0), (1, 3), (2, 2)] {
            let mut pw = good.cpts().to_vec();
            let p = pw.get_mut(i).unwrap();
            match field {
                0 => p.x = f64::NAN,
                2 => p.z = f64::NAN,
                _ => p.w = f64::NAN,
            }
            let c = NurbCurve::new(pw, 2, good.knots().to_vec()).unwrap();
            let r = on_circle(&kernel(), &c, C, Z, 2.0, 1e-7);
            assert!(
                matches!(r, Err(SolidError::Unsupported { .. })),
                "{i} {field}: {r:?}"
            );
            let r = meridian_collapse(&kernel(), &c, P, X, C, 4.0, 1e-7);
            assert!(
                matches!(r, Err(SolidError::Unsupported { .. })),
                "{i} {field}: {r:?}"
            );
        }
    }

    /// solidcraft#17907's review: both certificates poll the kernel's cancel token.
    #[test]
    fn the_circle_certificates_poll_the_cancel_token() {
        let mut k = kernel();
        let token = crate::kernel::CancelToken::new();
        token.cancel();
        k.set_cancel_token(token);
        let c = quarter(0.0);
        assert!(matches!(
            on_circle(&k, &c, C, Z, 2.0, 1e-7),
            Err(SolidError::Cancelled { .. } | SolidError::OutOfFuel { .. })
        ));
        assert!(matches!(
            meridian_collapse(&k, &c, P, X, C, 4.0, 1e-7),
            Err(SolidError::Cancelled { .. } | SolidError::OutOfFuel { .. })
        ));
    }

    /// A point of the axis in the arc's plane `z = 1`, and the axis along X in it: the arc
    /// turned about it is a torus' quarter, the arc in a plane through the axis.
    const P: Point3 = Point3::new(0.0, 0.0, 1.0);
    const X: Vec3 = Vec3::new(1.0, 0.0, 0.0);

    /// An exact arc r 2 in a plane through the axis: its offset collapses past its radius,
    /// not short of it nor away from its centre, and at the radius less the tolerance (its
    /// radius of curvature exactly the offset plus the tolerance) it is not proved either
    /// way. Just past that edge, at the radius, it collapses.
    #[test]
    fn an_exact_arc_through_the_axis_collapses_past_its_radius() {
        let k = kernel();
        let c = quarter(0.0);
        let at = |along: f64| meridian_collapse(&k, &c, P, X, C, along, 1e-7).unwrap();
        assert_eq!(at(4.0), Collapse::Everywhere);
        assert_eq!(at(2.0), Collapse::Everywhere);
        assert_eq!(at(2.0 - 1e-6), Collapse::Nowhere);
        assert_eq!(at(1.0), Collapse::Nowhere);
        assert_eq!(at(-4.0), Collapse::Nowhere);
        assert_eq!(at(2.0 - 1e-7), Collapse::Unproved);
    }

    /// `c` (one rational span) at `t`.
    fn point(c: &NurbCurve, t: f64) -> Point3 {
        let b = [(1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t];
        let mut h = [0.0_f64; 4];
        for (p, bi) in c.cpts().iter().zip(b) {
            h[0] += bi * p.x;
            h[1] += bi * p.y;
            h[2] += bi * p.z;
            h[3] += bi * p.w;
        }
        Point3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3])
    }

    /// The smallest and largest radius of curvature of `c`'s profile about the axis
    /// through `p` along `axis`, by central differences at 400 inner steps (a measurement
    /// for the tests' margins, independent of the certificate).
    fn profile_radius_range(c: &NurbCurve, p: Point3, axis: Vec3) -> (f64, f64) {
        let a = axis / axis.length();
        let profile = |t: f64| {
            let y = point(c, t) - p;
            let z = a.dot(y);
            ((y - a * z).length(), z)
        };
        let n = 400;
        let h = 1e-4;
        let (mut lo, mut hi) = (f64::INFINITY, 0.0_f64);
        for i in 1..n {
            let t = f64::from(i) / f64::from(n);
            let (u, m, w) = (profile(t - h), profile(t), profile(t + h));
            let (r1, z1) = ((w.0 - u.0) / (2.0 * h), (w.1 - u.1) / (2.0 * h));
            let (r2, z2) = (
                (w.0 - 2.0 * m.0 + u.0) / (h * h),
                (w.1 - 2.0 * m.1 + u.1) / (h * h),
            );
            let rho = (r1 * r1 + z1 * z1).powf(1.5) / (r1 * z2 - z1 * r2).abs();
            lo = lo.min(rho);
            hi = hi.max(rho);
        }
        (lo, hi)
    }

    /// solidcraft#18018's review (B3): an arc out of every plane through the axis is
    /// decided on its profile, whose radius of curvature is not the arc's. About the axis
    /// along `(1, 1, 1)` through `P` and along `(1, 0, 2^-10)` through the origin (no point
    /// of the arc on the axis), the offset below the profile's smallest radius (MEASURED)
    /// is `Nowhere` and past its largest `Everywhere`, and one inside the range is not
    /// proved. A straight profile (the plane square to the axis Z) is not proved, nor is
    /// one whose generatrix starts with its tangent square to both the axis and the radial
    /// direction (the arc's start, about the axes nearly along X: through `(0, 0, 1.001)`
    /// along X, through `P` along `(1, 0, 1e-6)`, through the origin along
    /// `(1, 0, 2^-10)`): the profile's parametrisation stops there, so the curvature's
    /// numerator and denominator both come to zero at the end and no strict sign is shown.
    /// Never answered the wrong way.
    #[test]
    fn an_arc_out_of_the_axis_plane_is_decided_on_its_profile() {
        let k = kernel();
        let c = quarter(0.0);
        for (p, axis) in [
            (P, Vec3::new(1.0, 1.0, 1.0)),
            (Point3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 1.0)),
        ] {
            let (lo, hi) = profile_radius_range(&c, p, axis);
            assert!(lo > 0.1 && hi < 100.0 && hi > lo, "{axis:?}: {lo} {hi}");
            let at = |along: f64| meridian_collapse(&k, &c, p, axis, C, along, 1e-7).unwrap();
            assert_eq!(at(0.5 * lo), Collapse::Nowhere, "{axis:?}: {lo} {hi}");
            assert_eq!(at(-2.0 * hi), Collapse::Nowhere, "{axis:?}: {lo} {hi}");
            assert_eq!(at(2.0 * hi), Collapse::Everywhere, "{axis:?}: {lo} {hi}");
            if hi > 1.01 * lo {
                assert_eq!(
                    at(0.5 * (lo + hi)),
                    Collapse::Unproved,
                    "{axis:?}: {lo} {hi}"
                );
            }
        }
        for along in [1.0, 4.0] {
            assert_eq!(
                meridian_collapse(&k, &c, P, Z, C, along, 1e-7).unwrap(),
                Collapse::Unproved
            );
        }
        for (p, axis) in [
            (Point3::new(0.0, 0.0, 1.001), X),
            (P, Vec3::new(1.0, 0.0, 1e-6)),
            (
                Point3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 1.0 / 1024.0),
            ),
        ] {
            assert_ne!(
                meridian_collapse(&k, &c, p, axis, C, 4.0, 1e-7).unwrap(),
                Collapse::Nowhere,
                "{axis:?}"
            );
            assert_ne!(
                meridian_collapse(&k, &c, p, axis, C, 1.0, 1e-7).unwrap(),
                Collapse::Everywhere,
                "{axis:?}"
            );
        }
    }

    /// solidcraft#18018's review (B2): the bounds are taken about a point of the axis near
    /// the arc, not about the axis's given point. Far along the axis (`1e12`, `1e14`,
    /// `1e16`, held exactly) the answers are those of the near point: the planarity bound no
    /// longer grows with the square of the distance, so a tilted axis is never taken for a
    /// plane through it, and a meridian axis is still proved.
    #[test]
    fn a_far_axis_point_gives_the_near_answers() {
        let k = kernel();
        let c = quarter(0.0);
        let alongs = [4.0, 3.0, 2.1, 2.0, 2.0 - 1e-6, 1.9, 1.0, 0.5, -4.0];
        let answers_at = |p: Point3, axis: Vec3, alongs: &[f64]| -> Vec<Collapse> {
            alongs
                .iter()
                .map(|&along| meridian_collapse(&k, &c, p, axis, C, along, 1e-7).unwrap())
                .collect()
        };
        let answers = |p: Point3, axis: Vec3| answers_at(p, axis, &alongs);
        // the meridian axis X through (L, 0, 1)
        let near = answers(P, X);
        assert_eq!(
            near,
            [
                Collapse::Everywhere,
                Collapse::Everywhere,
                Collapse::Everywhere,
                Collapse::Everywhere,
                Collapse::Nowhere,
                Collapse::Nowhere,
                Collapse::Nowhere,
                Collapse::Nowhere,
                Collapse::Nowhere
            ]
        );
        for l in [1e12, 1e14, 1e16] {
            assert_eq!(answers(Point3::new(l, 0.0, 1.0), X), near, "{l}");
        }
        // tilted axes through the origin, and through `L` times their direction (exact)
        let origin = Point3::new(0.0, 0.0, 0.0);
        // (about the first the arc's start stops the profile's parametrisation, see
        // `an_arc_out_of_the_axis_plane_is_decided_on_its_profile`: not proved near or far;
        // before the foot, its planarity allowance proved it in a plane from `1e14` out)
        for (axis, decided) in [
            (Vec3::new(1.0, 0.0, 1.0 / 1024.0), false),
            (Vec3::new(1.0, 1.0, 1.0), true),
        ] {
            let (lo, hi) = profile_radius_range(&c, origin, axis);
            // far from the profile's radii (MEASURED): decided, and the same far out
            let clear = [2.0 * hi, 0.5 * lo, -2.0 * hi];
            let near_clear = answers_at(origin, axis, &clear);
            let expected = if decided {
                [Collapse::Everywhere, Collapse::Nowhere, Collapse::Nowhere]
            } else {
                [Collapse::Unproved; 3]
            };
            assert_eq!(near_clear, expected, "{axis:?}: {lo} {hi}");
            let near = answers(origin, axis);
            for l in [1e12, 1e14, 1e16] {
                let far_point = Point3::new(l * axis.x, l * axis.y, l * axis.z);
                assert_eq!(
                    answers_at(far_point, axis, &clear),
                    near_clear,
                    "{axis:?} {l}"
                );
                let far = answers(far_point, axis);
                for ((along, n), f) in alongs.iter().zip(&near).zip(&far) {
                    assert!(
                        f == n || *f == Collapse::Unproved,
                        "{axis:?} {l} {along}: near {n:?}, far {f:?}"
                    );
                }
            }
        }
    }

    /// An arc bulged by 1e-4 of its radius: its radius of curvature varies by more than the
    /// offset's distance from the radius, so an offset at the radius is on both sides of it
    /// and not proved; far from the radius it is decided on both sides (a real collapse is
    /// still found, and a real non-collapse still built).
    #[test]
    fn a_bulged_arc_is_decided_only_away_from_its_radius() {
        let k = kernel();
        let c = quarter(1e-4);
        let at = |along: f64| meridian_collapse(&k, &c, P, X, C, along, 1e-7).unwrap();
        assert_eq!(at(2.0), Collapse::Unproved);
        assert_eq!(at(4.0), Collapse::Everywhere);
        assert_eq!(at(1.0), Collapse::Nowhere);
    }
}
