//! A proved bound on how far a curve lies from a face's surface, over the whole curve (not
//! upstream, tangency-labs/solidcraft#16508): what a construction that fits a surface to its
//! input curves records as its edges' tolerance, so the B-rep states its real construction
//! error (a sampled distance is a lower estimate of the largest one,
//! `docs/validity-and-tolerances.md` section 8).
//!
//! The curve `C(t)` is a B-spline, the surface `S` a NURBS net, and `W(t)` a B-spline of
//! parameters `(u, v, 0)` on that net, a correspondence chosen only to be close to the nearest
//! point (it is built through the nearest points of the curve's own samples). Whatever `W` is,
//! `dist(C(t), S) <= |g(t)|` for `g = C - S(W)`, because `S(W(t))` is a point of `S`. On a
//! piece `[a, b]` of the curve, `g` is within `h^2 / 8 * sup |g''|` of the chord between its two
//! end values (`h = b - a`), so
//!
//! `max over [a, b] of dist(C, S) <= max(|g(a)|, |g(b)|) + h^2 / 8 * sup |g''|`.
//!
//! The end values and `sup |g''|` are enclosed in outward-rounded interval arithmetic: the
//! curve's and `W`'s derivatives over the piece ([`curve_derivs_over`]), the surface's partials
//! over the box `W` sweeps there (`NetSurface::partials`, which cuts the box at the knot lines
//! it crosses, each part on its own polynomial piece, and refuses one crossed where the surface
//! is not continuous enough for the order asked). Because
//! `C` and `S(W)` nearly coincide, `g''` is small and the second term shrinks with the fourth
//! power of the piece, so the pieces are bisected until it no longer matters. A piece whose box
//! crosses a knot line the surface is only C^0 across (a patch corner the curve ends on) has no
//! second derivative to bound; there `g` is Lipschitz, and the piece is bounded by its larger
//! end value plus `h * sup |g'|` over the one-sided partials, bisected the same way.
//!
//! A surface that is not a NURBS net (or an analytic surface's exact net), a net whose
//! parameters are not the surface's, a box past the net's end where the surface does not
//! continue, and a piece the bisection cannot close all give `None`: the gap is then not
//! enclosed.

use crate::blend::fillet_section::certificate::{
    curve_derivs_over, knot_cuts, nurbs_partials_over_piece, partials_hull, EndContinuation,
    NurbsCurve,
};
use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_section::series::V3;
use crate::blend::fillet_section::station::{
    as_b_spline, knot, nurbs_partials, span, Nurbs, Partials,
};
use crate::kernel::Kernel;
use crate::math::Point2;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::surface::surface::Surface;
use crate::{Result, SolidError};

pub(crate) mod polynomial_pieces;

/// A direction's knot samples (parameter, spacing, span index) and its slack.
type KnotSamples = (Vec<(f64, f64, usize)>, f64);
/// A side of a control net: whether it runs across u, whether at the max, and its points'
/// indices.
type Edge = (bool, bool, Vec<(usize, usize)>);

const OP: &str = "query::curve_surface_gap";

/// The most pieces the bisection may make before the gap is called not enclosed.
const MAX_PIECES: usize = 1 << 15;
/// The deepest bisection of one piece.
const MAX_DEPTH: u32 = 30;
/// A piece is closed once its second-order term is at most this fraction of its end values.
const CLOSE_RELATIVE: f64 = 1.0e-3;
/// ... or at most this absolute amount, whatever the caller's floor: below any tolerance a
/// model carries.
const CLOSE_ABSOLUTE: f64 = 1.0e-13;
/// Points of the curve the correspondence `W` passes through (their nearest points on the
/// surface): only how close `W` stays to the nearest point, so how tight the bound is, depends
/// on it, never whether it holds.
const CORRESPONDENCE_POINTS: usize = 257;
/// Pieces the bisection starts from.
const FIRST_PIECES: usize = 32;

/// A surface prepared for enclosure: its net and the directions it continues past the net.
pub(crate) struct NetSurface {
    net: Nurbs,
    ends: EndContinuation,
}

impl NetSurface {
    /// The net of `surface`, or `None` when it has none (an offset, a step surface, a surface
    /// in a volume, a curve-bounded one) or its net's parameters are not the surface's: the
    /// net and the surface must give one point at the parameters `probe`.
    ///
    /// The bounds built on it take the net as the surface. That holds by construction for
    /// the kinds [`as_b_spline`] returns whose evaluation is their own net (a B-spline
    /// surface, and the analytic kinds through their exact base B-spline); the probe checks
    /// the agreement only at `probe` (to `1e-9` relative), so it catches a parameterisation
    /// mismatch but is no proof of agreement between the probes. A procedural blend's net
    /// only approximates its definition, by which it evaluates: not exact by construction
    /// (`query::seam_bound` refuses it).
    pub(crate) fn of(k: &Kernel, surface: &Surface, probe: &[Point2]) -> Result<Option<Self>> {
        let Some(bs) = as_b_spline(k, surface)? else {
            return Ok(None);
        };
        let Ok(net) = Nurbs::of(&bs) else {
            return Ok(None);
        };
        let ends = EndContinuation::of(surface);
        let ends = if ends.apex.is_some() {
            EndContinuation::default()
        } else {
            ends
        };
        let me = Self { net, ends };
        for &uv in probe {
            let Some(on_net) = me.point(uv) else {
                return Ok(None);
            };
            let flags = crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags {
                u_side: crate::nurbs::knots::Side::Left,
                v_side: crate::nurbs::knots::Side::Left,
                only_upper_half: true,
                non_zero_tangents: true,
            };
            let Some(on_surface) = crate::error::ignored(surface.evaluate(k, uv, 0, 0, flags))?
                .and_then(|d| d.into_iter().next().and_then(|row| row.into_iter().next()))
            else {
                return Ok(None);
            };
            let scale = 1.0 + on_surface.x.abs() + on_surface.y.abs() + on_surface.z.abs();
            if on_net.distance(on_surface) > 1.0e-9 * scale {
                return Ok(None);
            }
        }
        Ok(Some(me))
    }

    /// The knot lines strictly inside `u x v` across which the net is not C^1 (a knot of
    /// multiplicity at least the degree): the `u` values, then the `v` values, each sorted
    /// and once. A box cut at them has each part's second partials enclosed by
    /// [`Self::partials`].
    pub(crate) fn low_lines(&self, u: Ivl, v: Ivl) -> (Vec<f64>, Vec<f64>) {
        let n = &self.net;
        let lines = |knots: &[f64], p: usize, lo: f64, hi: f64| {
            let mut out: Vec<f64> = Vec::new();
            for &x in knots {
                if x > lo && x < hi && out.last() != Some(&x) {
                    let m = knots.iter().filter(|&&y| y == x).count();
                    if m >= p {
                        out.push(x);
                    }
                }
            }
            out.sort_by(f64::total_cmp);
            out.dedup();
            out
        };
        (lines(&n.u, n.p, u.lo, u.hi), lines(&n.v, n.q, v.lo, v.hi))
    }

    /// The net's domain: `([u_lo, u_hi], [v_lo, v_hi])`.
    pub(crate) fn domain(&self) -> (Ivl, Ivl) {
        let n = &self.net;
        (
            Ivl::new(knot(&n.u, n.p as isize), knot(&n.u, n.nu as isize)),
            Ivl::new(knot(&n.v, n.q as isize), knot(&n.v, n.nv as isize)),
        )
    }

    /// The net's sides collapsed to a point: those whose row of control points (homogeneous
    /// coordinates, weight included) is one point exactly, so the surface is that point all
    /// along the side. Each is `(across_u, at_max, point)`: `across_u` for a side at
    /// constant u (the first or last column), `at_max` for the last row or column, and the
    /// point (`None` from a net with a zero weight there). Only an end whose knots are
    /// clamped (the first or last `degree + 1` equal) is a side: the surface's edge there
    /// is that row; an unclamped end's row is not on the surface, and gives no side.
    pub(crate) fn collapsed_sides(&self) -> Vec<(bool, bool, V3<f64>)> {
        let n = &self.net;
        // the end of a knot vector (degree `p`, `count` control points) is clamped
        let clamped = |kn: &[f64], p: usize, count: usize, at_max: bool| -> bool {
            if at_max {
                let end = knot(kn, count as isize);
                (1..=p).all(|i| knot(kn, (count + i) as isize) == end)
            } else {
                let end = knot(kn, p as isize);
                (0..p).all(|i| knot(kn, i as isize) == end)
            }
        };
        let at = |i: usize, j: usize| -> Option<[f64; 4]> {
            let k = (i.checked_mul(n.nv)?.checked_add(j)?).checked_mul(4)?;
            let s = n.pw.get(k..k.checked_add(4)?)?;
            Some([*s.first()?, *s.get(1)?, *s.get(2)?, *s.get(3)?])
        };
        let mut out = Vec::new();
        let (Some(lu), Some(lv)) = (n.nu.checked_sub(1), n.nv.checked_sub(1)) else {
            return out;
        };
        // (across u, at the max, the side's control points)
        let sides: [Edge; 4] = [
            (true, false, (0..n.nv).map(|j| (0, j)).collect()),
            (true, true, (0..n.nv).map(|j| (lu, j)).collect()),
            (false, false, (0..n.nu).map(|i| (i, 0)).collect()),
            (false, true, (0..n.nu).map(|i| (i, lv)).collect()),
        ];
        for (across_u, at_max, row) in sides {
            let ends_clamped = if across_u {
                clamped(&n.u, n.p, n.nu, at_max)
            } else {
                clamped(&n.v, n.q, n.nv, at_max)
            };
            if !ends_clamped {
                continue;
            }
            let pts: Option<Vec<[f64; 4]>> = row.iter().map(|&(i, j)| at(i, j)).collect();
            let Some(pts) = pts else {
                continue;
            };
            let Some(first) = pts.first().copied() else {
                continue;
            };
            if pts.len() < 2 || pts.iter().any(|q| *q != first) {
                continue;
            }
            let [x, y, z, w] = first;
            if !(w.is_finite() && w > 0.0 && x.is_finite() && y.is_finite() && z.is_finite()) {
                continue;
            }
            out.push((
                across_u,
                at_max,
                V3 {
                    x: x / w,
                    y: y / w,
                    z: z / w,
                },
            ));
        }
        out
    }

    /// The net's point at `uv` (double arithmetic, a sanity probe only).
    fn point(&self, uv: Point2) -> Option<crate::math::Vec3> {
        let n = &self.net;
        let ku = span(&n.u, n.p, n.nu, uv.x);
        let kv = span(&n.v, n.q, n.nv, uv.y);
        let p = nurbs_partials(n, ku, kv, uv.x, uv.y, 0);
        let v = p.at(0, 0);
        let v = crate::math::Vec3::new(v.x, v.y, v.z);
        (v.x.is_finite() && v.y.is_finite() && v.z.is_finite()).then_some(v)
    }

    /// The surface's partials up to order `lk` over the box `w[0]` sweeps, with the slack the
    /// box's overshoot past the net needs (zero for a box inside it).
    ///
    /// The surface in the box is the net's polynomial pieces, the end span's polynomial
    /// continued past an end of the net: a surface that is a line in that direction continues
    /// (a plane, a cylinder's generator), and it is then the surface itself. Elsewhere only
    /// rounding overshoots a net's end (a seam on the face's boundary, its parameters a ulp
    /// out): the continued polynomial `S~` differs from the surface at the clamped point by at
    /// most the first partials' size times the overshoot, which is the slack, added to the
    /// bound: `dist(C, S) <= |C - S~(W)| + |S~(W) - S(clamp W)|`.
    pub(crate) fn partials(
        &self,
        w: &[V3<Ivl>],
        lk: usize,
        continuity: usize,
    ) -> Result<(Partials<Ivl>, f64)> {
        let refused = |reason| SolidError::Unsupported {
            operation: OP,
            reason,
        };
        let Some(w0) = w.first() else {
            return Err(refused("no parameter box"));
        };
        let n = &self.net;
        let dir = |knots: &[f64],
                   p: usize,
                   count: usize,
                   lo: f64,
                   hi: f64,
                   continues: bool|
         -> Result<KnotSamples> {
            let (d0, d1) = (knot(knots, p as isize), knot(knots, count as isize));
            if !(lo <= hi && lo.is_finite() && hi.is_finite()) {
                return Err(refused("an unordered parameter box"));
            }
            let slack_range = 0.05 * (d1 - d0).abs();
            let over = (d0 - lo).max(hi - d1).max(0.0);
            let continues = continues && p == 1;
            if over > 0.0 && !continues && over > slack_range {
                return Err(refused("a box past the net's end"));
            }
            let mut out = Vec::new();
            // (a part past an end runs on to the end, so the way back to the clamped point is
            // inside the box the first partials are enclosed over)
            if lo < d0 {
                out.push((lo, d0, span(knots, p, count, d0)));
            }
            let (a, b) = (lo.max(d0), hi.min(d1));
            if a <= b {
                out.extend(knot_cuts(knots, p, count, a, b, continuity)?);
            }
            if hi > d1 {
                out.push((d1, hi, span(knots, p, count, d1)));
            }
            Ok((out, if continues { 0.0 } else { over }))
        };
        let (us, over_u) = dir(&n.u, n.p, n.nu, w0.x.lo, w0.x.hi, self.ends.u)?;
        let (vs, over_v) = dir(&n.v, n.q, n.nv, w0.y.lo, w0.y.hi, self.ends.v)?;
        let mut out: Option<Partials<Ivl>> = None;
        for &(a0, a1, ku) in &us {
            for &(b0, b1, kv) in &vs {
                let p = nurbs_partials_over_piece(n, ku, kv, a0, a1, b0, b1, lk)?;
                out = Some(match out {
                    None => p,
                    Some(h) => partials_hull(h, &p, lk),
                });
            }
        }
        let Some(out) = out else {
            return Err(refused("no piece encloses the box"));
        };
        // (the point values of an end are of the continued polynomial itself; only a span's
        // bound needs the slack, and it has the first partials)
        let mut slack = 0.0_f64;
        if lk >= 1 && (over_u > 0.0 || over_v > 0.0) {
            slack = Self::slack(&out, over_u, over_v)?;
        }
        Ok((out, slack))
    }

    fn slack(p: &Partials<Ivl>, over_u: f64, over_v: f64) -> Result<f64> {
        let (Some(su), Some(sv)) = (norm_upper(p.at(1, 0)), norm_upper(p.at(0, 1))) else {
            return Err(SolidError::Unsupported {
                operation: OP,
                reason: "the surface's first partials are not enclosed",
            });
        };
        Ok((Ivl::from(su) * Ivl::from(over_u) + Ivl::from(sv) * Ivl::from(over_v)).hi)
    }
}

/// `sqrt(x^2 + y^2 + z^2)` of an enclosure, an upper bound; `None` unless finite.
fn norm_upper(v: V3<Ivl>) -> Option<f64> {
    let sq = |a: Ivl| {
        let m = Ivl::from(a.mag());
        m * m
    };
    let s = sq(v.x) + sq(v.y) + sq(v.z);
    let hi = s.sqrt().hi;
    (v.x.is_finite() && v.y.is_finite() && v.z.is_finite() && hi.is_finite()).then_some(hi)
}

fn scaled(v: V3<Ivl>, a: Ivl) -> V3<Ivl> {
    V3 {
        x: v.x * a,
        y: v.y * a,
        z: v.z * a,
    }
}

fn plus(a: V3<Ivl>, b: V3<Ivl>) -> V3<Ivl> {
    V3 {
        x: a.x + b.x,
        y: a.y + b.y,
        z: a.z + b.z,
    }
}

fn minus(a: V3<Ivl>, b: V3<Ivl>) -> V3<Ivl> {
    V3 {
        x: a.x - b.x,
        y: a.y - b.y,
        z: a.z - b.z,
    }
}

/// The bound of one piece: `(end values, second-order term)`; the gap over `[a, b]` is at
/// most their sum.
pub(crate) fn piece(
    cn: &NurbsCurve,
    wn: &NurbsCurve,
    s: &NetSurface,
    a: f64,
    b: f64,
) -> Result<Option<(f64, f64)>> {
    let mut ends = 0.0_f64;
    for t in [a, b] {
        let at = Ivl::from(t);
        let c = curve_derivs_over(cn, at, 0, None)?;
        let w = curve_derivs_over(wn, at, 0, None)?;
        let (Some(c0), Some(_)) = (c.first().copied(), w.first()) else {
            return Ok(None);
        };
        let surf = s.partials(&w, 0, 0)?.0.at(0, 0);
        let Some(g) = norm_upper(minus(c0, surf)) else {
            return Ok(None);
        };
        ends = ends.max(g);
    }
    let span = Ivl::new(a, b);
    let cd = curve_derivs_over(cn, span, 2, None)?;
    let wd = curve_derivs_over(wn, span, 2, None)?;
    let (Some(c1), Some(c2), Some(w1), Some(w2)) = (cd.get(1), cd.get(2), wd.get(1), wd.get(2))
    else {
        return Ok(None);
    };
    let (u1, v1) = (w1.x, w1.y);
    let (u2, v2) = (w2.x, w2.y);
    let h = Ivl::from(b) - Ivl::from(a);
    // The second order, where the surface is C^1 across every knot line the box crosses:
    // (S o W)'' = S_uu u'^2 + 2 S_uv u'v' + S_vv v'^2 + S_u u'' + S_v v''
    let second = s.partials(&wd, 2, 1).map(|(p, slack)| {
        let mut swpp = scaled(p.at(2, 0), u1 * u1);
        swpp = plus(swpp, scaled(p.at(1, 1), Ivl::from(2.0) * u1 * v1));
        swpp = plus(swpp, scaled(p.at(0, 2), v1 * v1));
        swpp = plus(swpp, scaled(p.at(1, 0), u2));
        swpp = plus(swpp, scaled(p.at(0, 1), v2));
        (minus(*c2, swpp), slack)
    });
    let term = match second {
        Ok((gpp, slack)) => {
            let Some(m) = norm_upper(gpp) else {
                return Ok(None);
            };
            (h * h * Ivl::from(m) * Ivl::from(0.125) + Ivl::from(slack)).hi
        }
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        // a knot line the surface is only C^0 across (a patch corner the curve ends on):
        // g is Lipschitz there, |g(t) - g(a)| <= h sup |g'| with the one-sided partials hulled
        Err(_) => {
            let (p, slack) = s.partials(&wd, 1, 0)?;
            let sw = plus(scaled(p.at(1, 0), u1), scaled(p.at(0, 1), v1));
            let Some(m) = norm_upper(minus(*c1, sw)) else {
                return Ok(None);
            };
            (h * Ivl::from(m) + Ivl::from(slack)).hi
        }
    };
    Ok(term.is_finite().then_some((ends, term)))
}

/// A proved upper bound on the distance from the curve `curve` to the surface `surface`
/// over its parameters `[lo, hi]` (within its domain), where `uv` is a B-spline of parameters `(u, v, 0)` (the correspondence
/// `W`, on the curve's own parameter). A piece closes once its second term is at most a
/// thousandth of its end values or `floor` (the caller's scale: a hundredth of the gap it
/// expects, below which an excess in the bound changes nothing; a nearest point is found only
/// to some 1e-8 of a parameter, which the bound carries as well). A piece that cannot be cut
/// further is bounded as it stands, so the result is a bound however the pieces ended; `None`
/// when some piece has no bound at all.
///
/// The first pieces are also cut at every [`kinks`] of `C` and of `W` inside `[lo, hi]`:
/// the second-order term bounds `|g − ℓ|` (`ℓ` the chord of the end values) by `h² / 8 sup
/// |g''|` only where `g` is C¹, and the spans' second derivatives, hulled over a piece,
/// know nothing of a jump in the first (a corner of `C` between two pieces' ends would be
/// missed). No piece then has a corner of either inside it: it ends on the corner instead.
///
/// # Errors
///
/// Cancellation (polled at every piece), and the unported statuses of the evaluations.
fn certified_gap(
    k: &Kernel,
    curve: &BSplineCurve,
    uv: &BSplineCurve,
    surface: &NetSurface,
    [lo, hi]: [f64; 2],
    first_pieces: usize,
    floor: f64,
) -> Result<Option<f64>> {
    let (Ok(cn), Ok(wn)) = (NurbsCurve::of(curve), NurbsCurve::of(uv)) else {
        return Ok(None);
    };
    let (Some(&d0), Some(&d1)) = (cn.u.get(cn.p), cn.u.get(cn.n)) else {
        return Ok(None);
    };
    if !(lo.is_finite() && hi.is_finite() && d0 <= lo && lo < hi && hi <= d1) || first_pieces == 0 {
        return Ok(None);
    }
    let mut corners = kinks(&cn, lo, hi);
    corners.extend(kinks(&wn, lo, hi));
    corners.sort_by(f64::total_cmp);
    corners.dedup();
    let f = |j: usize| {
        if j == first_pieces {
            hi
        } else {
            lo + (hi - lo) * (j as f64) / (first_pieces as f64)
        }
    };
    let mut stack: Vec<(f64, f64, u32)> = Vec::new();
    for i in 0..first_pieces {
        let (mut a, b) = (f(i), f(i + 1));
        let start = a;
        for &x in corners.iter().filter(|&&x| start < x && x < b) {
            stack.push((a, x, 0));
            a = x;
        }
        stack.push((a, b, 0));
    }
    stack.reverse();
    let mut worst = 0.0_f64;
    let mut made = stack.len();
    while let Some((a, b, depth)) = stack.pop() {
        k.check_cancelled(OP)?;
        let mid = 0.5 * (a + b);
        // (a piece that cannot be cut further is bounded as it is: the bound holds whether or
        // not the second term has shrunk, closing only makes it tight)
        let last = depth >= MAX_DEPTH || !(a < mid && mid < b) || made + 2 > MAX_PIECES;
        match piece(&cn, &wn, surface, a, b) {
            Ok(Some((ends, term))) => {
                let bound = (Ivl::from(ends) + Ivl::from(term)).hi;
                if last || term <= (CLOSE_RELATIVE * ends).max(floor).max(CLOSE_ABSOLUTE) {
                    worst = worst.max(bound);
                    continue;
                }
            }
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            // a box past the net, on a knot line of too low a continuity: a smaller piece may
            // clear it; at the bottom the gap is not enclosed
            Ok(None) | Err(_) => {
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

/// The parameters strictly inside `(lo, hi)` where the curve `cn` may not be C¹: its
/// interior knots of multiplicity at least its degree (a span's polynomial meets the next
/// only in its value there), in increasing order.
fn kinks(cn: &NurbsCurve, lo: f64, hi: f64) -> Vec<f64> {
    let Some(knots) = cn.u.get(cn.p..=cn.n) else {
        return Vec::new();
    };
    knots
        .chunk_by(|a, b| a == b)
        .filter(|run| run.len() >= cn.p.max(1))
        .filter_map(|run| run.first().copied())
        .filter(|&x| lo < x && x < hi)
        .collect()
}

/// A proved upper bound on `sup |C(t) − S(W(t))|` over `t` in `range`, for the 3D curve
/// `curve` (`C`), a B-spline `uv` of parameters `(u, v, 0)` on the same parameter (`W`: a
/// UV curve or trim of `C` on the surface), and `surface`'s net (`S`): how far the curve
/// strays from the surface's image of its UV curve, the bound [`certified_gap`] encloses
/// (not upstream, tangency-labs/solidcraft#18166). For a surface kept as the net of a
/// definition (a blend's or a sweep's), `S` is the net: the caller adds the net's own bound.
/// `floor` is the caller's scale, as for [`curve_face_gap_bound`].
///
/// `None` when the gap is not enclosed: an empty, unordered or not finite `range`, a
/// surface that is not a NURBS net or whose net's parameters are not the surface's
/// ([`NetSurface::of`], probed at `W`'s ends and middle), a `W` that cannot be evaluated,
/// or a piece the bisection cannot bound.
///
/// # Errors
///
/// The kernel's unported and cancelled statuses.
pub(crate) fn curve_uv_gap_bound(
    k: &Kernel,
    curve: &BSplineCurve,
    uv: &BSplineCurve,
    surface: &Surface,
    range: [f64; 2],
    floor: f64,
) -> Result<Option<f64>> {
    let [lo, hi] = range;
    if !(lo.is_finite() && hi.is_finite() && lo < hi && floor.is_finite() && floor >= 0.0) {
        return Ok(None);
    }
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
    certified_gap(k, curve, uv, &net, range, FIRST_PIECES, floor)
}

/// A proved upper bound on the distance from every point of `curve` over its parameters
/// `range` to the surface of face `face` over that surface's parameter box: no point of the
/// curve there is farther from the surface. `floor` is the caller's scale (a hundredth of the
/// tolerance the bound is held to, below which a tighter bound changes nothing).
///
/// `None` when the gap is not enclosed: a face without a surface or whose surface is not a
/// NURBS net ([`NetSurface::of`]), a curve point the surface cannot be dropped from, an empty
/// or unordered `range` or one past the curve's domain, or a piece of the curve the bisection
/// cannot bound.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling face; the kernel's unported and cancelled statuses.
pub fn curve_face_gap_bound(
    k: &Kernel,
    brep: &crate::topology::Brep,
    face: crate::topology::FaceId,
    curve: &BSplineCurve,
    range: [f64; 2],
    floor: f64,
) -> Result<Option<f64>> {
    let [lo, hi] = range;
    if !(lo.is_finite() && hi.is_finite() && lo < hi && floor.is_finite() && floor >= 0.0) {
        return Ok(None);
    }
    let rec = brep.face(face)?;
    let Some(surface) = rec.surface.as_ref() else {
        return Ok(None);
    };
    let surface = surface.clone_for_evaluation()?;
    let Some((net, correspondence)) = correspondence(k, &surface, &rec.uv_domain, curve, [lo, hi])?
    else {
        return Ok(None);
    };
    certified_gap(k, curve, &correspondence, &net, range, FIRST_PIECES, floor)
}

/// The correspondence `W` of `curve` over `[lo, hi]` on `surface` (module docs): a cubic
/// B-spline of parameters `(u, v, 0)` on the curve's own parameter, through the nearest points
/// in `domain` of [`CORRESPONDENCE_POINTS`] equally spaced points of the curve, with the
/// surface's net. `None` where a nearest point is not found or the surface has no net whose
/// parameters are the surface's ([`NetSurface::of`]).
///
/// # Errors
///
/// The kernel's unported and cancelled statuses; those of the curve's evaluation.
pub(crate) fn correspondence(
    k: &Kernel,
    surface: &Surface,
    domain: &crate::geom::extent2d::Extent2d,
    curve: &BSplineCurve,
    [lo, hi]: [f64; 2],
) -> Result<Option<(NetSurface, BSplineCurve)>> {
    use crate::curve::curve::Curve;
    use crate::error::ignored;
    use crate::nurbs::crv_fit::p1::ParamMethod;
    use crate::surface::surface::p4::NoObject;
    use crate::surface::surface::p5::DropPointOptions;

    let evaluated = Curve::from(curve.clone_exact());
    let params: Vec<f64> = (0..CORRESPONDENCE_POINTS)
        .map(|i| {
            if i + 1 == CORRESPONDENCE_POINTS {
                hi
            } else {
                lo + (hi - lo) * (i as f64) / ((CORRESPONDENCE_POINTS - 1) as f64)
            }
        })
        .collect();
    // The nearest points are followed from the middle of the curve out to each end, each drop
    // started from its neighbour's: an end on a side of the net collapsed to one point (the apex
    // of a loft between curves that meet there) has every parameter of that side for its nearest
    // point, and a drop with nothing to start from takes any of them, so `W` would jump there
    // and run past the net. Continued from its neighbour it takes the neighbour's; the bound holds
    // for any `W`, only whether the bisection closes depends on this.
    let n = params.len();
    let mid = n / 2;
    let order: Vec<(usize, Option<usize>)> = (mid..n)
        .map(|i| (i, (i > mid).then(|| i - 1)))
        .chain((0..mid).rev().map(|i| (i, Some(i + 1))))
        .collect();
    let mut found: Vec<Option<Point2>> = vec![None; n];
    for (i, from) in order {
        let Some(&t) = params.get(i) else {
            return Ok(None);
        };
        let guess = from.and_then(|j| found.get(j).copied().flatten());
        let Some(point) =
            ignored(evaluated.evaluate(k, t, 0, true, false))?.and_then(|d| d.into_iter().next())
        else {
            return Ok(None);
        };
        let opts = DropPointOptions {
            guess_uv: guess,
            ..DropPointOptions::default()
        };
        let Some(drop) = ignored(surface.drop_point::<NoObject>(k, point, domain, &opts))? else {
            return Ok(None);
        };
        if !drop.success {
            return Ok(None);
        }
        if let Some(slot) = found.get_mut(i) {
            *slot = Some(drop.result_uv);
        }
    }
    let uvs: Vec<Point2> = found.into_iter().flatten().collect();
    let Some(net) = NetSurface::of(k, surface, &uvs)? else {
        return Ok(None);
    };
    let flat: Vec<crate::math::Vec3> = uvs
        .iter()
        .map(|uv| crate::math::Vec3::new(uv.x, uv.y, 0.0))
        .collect();
    let Ok(correspondence) = BSplineCurve::interpolate_points(
        &flat,
        Some(&params),
        3,
        None,
        None,
        false,
        ParamMethod::ChordLength,
    ) else {
        return Ok(None);
    };
    Ok(Some((net, correspondence)))
}

// ---- a curve piece against a point, a surface or a curve ------------------------------------

/// What [`curve_piece_gap_bound`] bounds a curve piece's distance to: a point, a surface
/// with the parameters of its points that correspond to the piece's two ends, or a curve
/// with the parameters of those points.
#[derive(Clone, Copy, Debug)]
pub(crate) enum PieceTarget<'a> {
    /// A fixed point.
    Point(crate::math::Point3),
    /// A surface and its parameters at the points matching the piece's first and last end.
    Surface(&'a Surface, [Point2; 2]),
    /// A curve and its parameters at the points matching the piece's first and last end.
    Curve(&'a crate::curve::curve::Curve, [f64; 2]),
}

/// The NURBS data a curve evaluates over `[lo, hi]`, with the error bound of its points
/// against the curve's own: the B-spline a B-spline, ellipse, circle, hyperbola or parabola
/// evaluates (no error: it is that B-spline), and for a line the segment between its two
/// computed end points, each within `e` per coordinate of the line's (so every point of the
/// line over `[lo, hi]`, a convex combination of the exact ends, is within `sqrt(3) e <= 2 e`
/// of the segment's point at that parameter). `None` for other kinds, a NURB that is not
/// well formed, and a range outside the curve's domain.
fn piece_nurbs(
    curve: &crate::curve::curve::Curve,
    [lo, hi]: [f64; 2],
) -> Option<(NurbsCurve, f64)> {
    use crate::curve::curve::p2::CurveKind;
    let (cn, err) = match curve.kind() {
        CurveKind::Line(l) => {
            let [(p0, e0), (p1, e1)] =
                l.segment_over(&crate::geom::extent1d::Extent1d::from_min_max(lo, hi))?;
            let err = 2.0 * e0.max(e1);
            (
                NurbsCurve {
                    p: 1,
                    n: 2,
                    u: vec![lo, lo, hi, hi],
                    pw: vec![p0.x, p0.y, p0.z, 1.0, p1.x, p1.y, p1.z, 1.0],
                },
                err,
            )
        }
        CurveKind::BSpline(_)
        | CurveKind::Ellipse(_)
        | CurveKind::Hyperbola(_)
        | CurveKind::Parabola(_) => (NurbsCurve::of(curve.b_spline_base()?).ok()?, 0.0),
        _ => return None,
    };
    let (d0, d1) = (*cn.u.get(cn.p)?, *cn.u.get(cn.n)?);
    (lo.is_finite() && hi.is_finite() && d0 <= lo && lo <= hi && hi <= d1 && err.is_finite())
        .then_some((cn, err))
}

/// The distinct knots of `cn` strictly inside `(lo, hi)`, in order.
fn interior_knots(cn: &NurbsCurve, lo: f64, hi: f64) -> Vec<f64> {
    let mut out: Vec<f64> = cn.u.iter().copied().filter(|&u| lo < u && u < hi).collect();
    out.dedup();
    out
}

/// The target of one piece, prepared: its NURBS data, the parameter map from the piece's
/// curve parameter and the error bound of its points.
enum Prepared {
    Point(V3<Ivl>),
    Surface(NetSurface, [Point2; 2]),
    Curve(NurbsCurve, [f64; 2]),
}

/// `w(t)` of the straight map from `[lo, hi]` onto `[w0, w1]`, enclosed for every `t` in `t`.
fn along(t: Ivl, [lo, hi]: [f64; 2], [w0, w1]: [f64; 2]) -> Ivl {
    let slope = (Ivl::from(w1) - Ivl::from(w0)) / (Ivl::from(hi) - Ivl::from(lo));
    Ivl::from(w0) + (t - Ivl::from(lo)) * slope
}

/// [`along`]'s slope, enclosed.
fn slope([lo, hi]: [f64; 2], [w0, w1]: [f64; 2]) -> Ivl {
    (Ivl::from(w1) - Ivl::from(w0)) / (Ivl::from(hi) - Ivl::from(lo))
}

/// `x` cut to `[a, b]` (both ends of `x` past it become its ends): the map's exact values lie
/// between its exact ends, which lie in `[a, b]`, so cutting the enclosure keeps them.
fn clamp_ivl(x: Ivl, a: f64, b: f64) -> Ivl {
    let (lo, hi) = (a.min(b), a.max(b));
    Ivl::new(x.lo.max(lo).min(hi), x.hi.min(hi).max(lo))
}

/// The bound of one piece `[a, b]` of the curve against the target: `(end values, second
/// term)`, `None` when some enclosure is not finite (module docs; the same chord bound as
/// [`piece`] with the straight map in place of the correspondence `W`). A target curve whose
/// knot lies inside the piece's image, or a surface only C^0 across a knot line the box
/// crosses, is bounded by the Lipschitz form, `h sup |g'|`.
fn target_piece(
    cn: &NurbsCurve,
    range: [f64; 2],
    target: &Prepared,
    a: f64,
    b: f64,
) -> Result<Option<(f64, f64)>> {
    let mut ends = 0.0_f64;
    for t in [a, b] {
        let at = Ivl::from(t);
        let Some(c0) = curve_derivs_over(cn, at, 0, None)?.first().copied() else {
            return Ok(None);
        };
        let q = match target {
            Prepared::Point(p) => *p,
            Prepared::Surface(s, [w0, w1]) => {
                let w = V3 {
                    x: clamp_ivl(along(at, range, [w0.x, w1.x]), w0.x, w1.x),
                    y: clamp_ivl(along(at, range, [w0.y, w1.y]), w0.y, w1.y),
                    z: Ivl::from(0.0),
                };
                s.partials(&[w], 0, 0)?.0.at(0, 0)
            }
            Prepared::Curve(dn, [s0, s1]) => {
                let s = clamp_ivl(along(at, range, [*s0, *s1]), *s0, *s1);
                let Some(d0) = curve_derivs_over(dn, s, 0, None)?.first().copied() else {
                    return Ok(None);
                };
                d0
            }
        };
        let Some(g) = norm_upper(minus(c0, q)) else {
            return Ok(None);
        };
        ends = ends.max(g);
    }
    let span = Ivl::new(a, b);
    let h = Ivl::from(b) - Ivl::from(a);
    let cd = curve_derivs_over(cn, span, 2, None)?;
    let (Some(c1), Some(c2)) = (cd.get(1).copied(), cd.get(2).copied()) else {
        return Ok(None);
    };
    // (g', g'') of g = c - target(w), the map straight (w'' = 0); `None` for the second
    // where it is not defined over the piece
    let (g1, g2) = match target {
        Prepared::Point(_) => (c1, Some(c2)),
        Prepared::Surface(s, [w0, w1]) => {
            let (u1, v1) = (slope(range, [w0.x, w1.x]), slope(range, [w0.y, w1.y]));
            let w = V3 {
                x: clamp_ivl(along(span, range, [w0.x, w1.x]), w0.x, w1.x),
                y: clamp_ivl(along(span, range, [w0.y, w1.y]), w0.y, w1.y),
                z: Ivl::from(0.0),
            };
            // (S o w)'' = S_uu u'^2 + 2 S_uv u'v' + S_vv v'^2 where the surface is C^1
            // across every knot line the box crosses; else g is Lipschitz, with the
            // one-sided first partials hulled
            let term = match s.partials(&[w], 2, 1) {
                Ok((p, slack)) => {
                    let mut swpp = scaled(p.at(2, 0), u1 * u1);
                    swpp = plus(swpp, scaled(p.at(1, 1), Ivl::from(2.0) * u1 * v1));
                    swpp = plus(swpp, scaled(p.at(0, 2), v1 * v1));
                    let Some(m) = norm_upper(minus(c2, swpp)) else {
                        return Ok(None);
                    };
                    (h * h * Ivl::from(m) * Ivl::from(0.125) + Ivl::from(slack)).hi
                }
                Err(e) if e.is_unported_or_cancelled() => return Err(e),
                Err(_) => {
                    let (p, slack) = s.partials(&[w], 1, 0)?;
                    let sw = plus(scaled(p.at(1, 0), u1), scaled(p.at(0, 1), v1));
                    let Some(m) = norm_upper(minus(c1, sw)) else {
                        return Ok(None);
                    };
                    (h * Ivl::from(m) + Ivl::from(slack)).hi
                }
            };
            return Ok(term.is_finite().then_some((ends, term)));
        }
        Prepared::Curve(dn, [s0, s1]) => {
            let w1 = slope(range, [*s0, *s1]);
            let s = clamp_ivl(along(span, range, [*s0, *s1]), *s0, *s1);
            let dd = curve_derivs_over(dn, s, 2, None)?;
            let (Some(d1), Some(d2)) = (dd.get(1).copied(), dd.get(2).copied()) else {
                return Ok(None);
            };
            // a knot of the target inside the image: its derivatives are hulled over two
            // pieces, the second-order bound needs one
            let kinked = !interior_knots(dn, s.lo, s.hi).is_empty();
            let g1 = minus(c1, scaled(d1, w1));
            let g2 = (!kinked).then(|| minus(c2, scaled(d2, w1 * w1)));
            (g1, g2)
        }
    };
    let term = if let Some(gpp) = g2 {
        let Some(m) = norm_upper(gpp) else {
            return Ok(None);
        };
        (h * h * Ivl::from(m) * Ivl::from(0.125)).hi
    } else {
        let Some(m) = norm_upper(g1) else {
            return Ok(None);
        };
        (h * Ivl::from(m)).hi
    };
    Ok(term.is_finite().then_some((ends, term)))
}

/// Not upstream (WR-410): a proved upper bound on the distance from every point of `curve`
/// between its parameters `t[0]` and `t[1]` (either order) to `target`: no point of that
/// piece is farther from the target. A surface or curve target is matched to the piece by
/// the straight map from `t` onto its own parameters at the two ends (a witness only:
/// `dist(c(t), S) <= |c(t) - S(w(t))|` for any `w`, so the map decides how tight the bound
/// is, never whether it holds), and the bound is that of [`certified_gap`]'s module docs: the
/// larger end value plus `h^2 / 8 sup |g''|` per piece, the pieces cut at the curve's knots
/// and bisected, every value enclosed in outward-rounded intervals. Bisection stops once the
/// bound passes `limit` (it is then returned) or closes below `floor`.
///
/// `None` when no bound is made: a curve kind without NURBS data ([`piece_nurbs`]), a range
/// or target range outside its curve's domain, a surface without a net matching it at the
/// two parameters (`NetSurface::of`), or a piece the bisection cannot bound.
///
/// # Errors
///
/// The kernel's unported and cancelled statuses.
pub(crate) fn curve_piece_gap_bound(
    k: &Kernel,
    curve: &crate::curve::curve::Curve,
    t: [f64; 2],
    target: PieceTarget<'_>,
    floor: f64,
    limit: f64,
) -> Result<Option<f64>> {
    let [t0, t1] = t;
    if !(t0.is_finite() && t1.is_finite() && t0 != t1 && floor >= 0.0) {
        return Ok(None);
    }
    // the piece runs from lo to hi; the target's ends follow it
    let (lo, hi, flip) = if t0 < t1 {
        (t0, t1, false)
    } else {
        (t1, t0, true)
    };
    let Some((cn, err)) = piece_nurbs(curve, [lo, hi]) else {
        return Ok(None);
    };
    let (prepared, target_err) = match target {
        PieceTarget::Point(p) => {
            let p = V3 {
                x: Ivl::from(p.x),
                y: Ivl::from(p.y),
                z: Ivl::from(p.z),
            };
            (Prepared::Point(p), 0.0)
        }
        PieceTarget::Surface(s, [w0, w1]) => {
            let ends = if flip { [w1, w0] } else { [w0, w1] };
            let Some(net) = NetSurface::of(k, s, &ends)? else {
                return Ok(None);
            };
            (Prepared::Surface(net, ends), 0.0)
        }
        PieceTarget::Curve(c, [s0, s1]) => {
            let ends = if flip { [s1, s0] } else { [s0, s1] };
            let Some((dn, derr)) = piece_nurbs(c, [ends[0].min(ends[1]), ends[0].max(ends[1])])
            else {
                return Ok(None);
            };
            (Prepared::Curve(dn, ends), derr)
        }
    };
    let range = [lo, hi];
    let mut cuts = vec![lo];
    cuts.extend(interior_knots(&cn, lo, hi));
    cuts.push(hi);
    let mut stack: Vec<(f64, f64, u32)> = cuts
        .windows(2)
        .rev()
        .filter_map(|w| match *w {
            [a, b] => Some((a, b, 0)),
            _ => None,
        })
        .collect();
    let extra = err + target_err;
    let mut worst = 0.0_f64;
    let mut made = stack.len();
    while let Some((a, b, depth)) = stack.pop() {
        let mid = 0.5 * (a + b);
        let last = depth >= MAX_DEPTH || !(a < mid && mid < b) || made + 2 > MAX_PIECES;
        match target_piece(&cn, range, &prepared, a, b) {
            Ok(Some((ends, term))) => {
                let bound = (Ivl::from(ends) + Ivl::from(term) + Ivl::from(extra)).hi;
                if !bound.is_finite() {
                    return Ok(None);
                }
                // (past the limit at an end: no cut can bring it back)
                if (Ivl::from(ends) + Ivl::from(extra)).hi > limit {
                    return Ok(Some(bound));
                }
                if last || term <= (CLOSE_RELATIVE * ends).max(floor).max(CLOSE_ABSOLUTE) {
                    worst = worst.max(bound);
                    continue;
                }
            }
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Ok(None) | Err(_) => {
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
    use crate::nurbs::crv_basic::Curve as NurbCurve;
    use crate::nurbs::math::{CPoint, NOW};

    /// A line `lift` above a box's top face, inside it: its gap to the top face's plane is
    /// `lift` everywhere, so the bound is at least `lift` and, the line and the plane being
    /// polynomial, no more than rounding above it; the faces it is far from bound it far.
    #[test]
    fn a_line_above_a_face_is_bounded_by_its_height() {
        let mut k = Kernel::default();
        let b = crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 1.0, 2.0, 3.0)
            .unwrap();
        let lift = 6.8e-5;
        let z = 3.0 + lift;
        let line = NurbCurve::new(
            vec![CPoint::new(0.2, 0.3, z, NOW), CPoint::new(0.7, 1.6, z, NOW)],
            1,
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        let line = BSplineCurve::from_nurb(line, 3, false);
        let mut bounds = Vec::new();
        for f in crate::query::api_queries::get_faces(&b).unwrap() {
            if let Some(g) = curve_face_gap_bound(&k, &b, f, &line, [0.0, 1.0], 1e-7).unwrap() {
                bounds.push(g);
            }
        }
        let nearest = bounds.iter().copied().fold(f64::INFINITY, f64::min);
        assert!(
            nearest >= lift && nearest <= lift * (1.0 + 1e-6),
            "{nearest:e} against {lift:e}: {bounds:?}"
        );
        // an empty or reversed range encloses nothing
        let top = crate::query::api_queries::get_faces(&b).unwrap();
        assert!(top.iter().all(
            |f| curve_face_gap_bound(&k, &b, *f, &line, [0.5, 0.5], 1e-7)
                .unwrap()
                .is_none()
        ));
    }

    /// [`curve_uv_gap_bound`] of a line `lift` above the bilinear plane patch z = 0 (x, y
    /// in 0..1), at the UV line under it with the same parameters: at least `lift` and no
    /// more than rounding above it. The same line read at the UV line's reversed parameters
    /// (the same image, other points) is bounded by its true gap, which at either end is
    /// the shadow's whole length (about 0.5831); an empty range encloses nothing. A corner of
    /// the curve inside a first piece (a spike to 1e-3 between two ends on the plane) is
    /// seen: the pieces are cut there.
    #[test]
    fn a_line_over_its_uv_line_is_bounded_by_its_height() {
        use crate::nurbs::b_spline_curve::KnotType;
        use crate::nurbs::b_spline_surface::{
            BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData,
        };
        let k = Kernel::default();
        let plane = BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: 1,
            v_degree: 1,
            control_points: &[
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
            ],
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
        let surface = Surface::from(plane);
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
        let lift = 4.2e-7;
        let curve = line([0.2, 0.3, lift], [0.7, 0.6, lift]);
        let uv = line([0.2, 0.3, 0.0], [0.7, 0.6, 0.0]);
        let g = curve_uv_gap_bound(&k, &curve, &uv, &surface, [0.0, 1.0], 1e-9)
            .unwrap()
            .unwrap();
        assert!(g >= lift && g <= lift * (1.0 + 1e-6), "{g:e}");
        let reversed = line([0.7, 0.6, 0.0], [0.2, 0.3, 0.0]);
        let g = curve_uv_gap_bound(&k, &curve, &reversed, &surface, [0.0, 1.0], 1e-9)
            .unwrap()
            .unwrap();
        // (at either end the curve is the shadow's length from its image: the whole gap, not
        // a part of it; its rounding is far below the 1e-12 allowed)
        let full = (0.5_f64 * 0.5 + 0.3 * 0.3 + lift * lift).sqrt();
        assert!(g >= full * (1.0 - 1e-12), "{g:e} against {full:e}");
        let empty = curve_uv_gap_bound(&k, &curve, &uv, &surface, [0.5, 0.5], 1e-9).unwrap();
        assert_eq!(empty, None);
        // (a corner of the curve inside a first piece, `[0, 1/32]`: the line over its UV
        // line but for a spike to 1e-3 at t = 1/64, back at t = 1/32; the piece's ends and
        // the spans' second derivatives are all 0, so only a cut at the corner sees it)
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
        let g = curve_uv_gap_bound(&k, &spiked, &uv, &surface, [0.0, 1.0], 1e-9)
            .unwrap()
            .unwrap();
        assert!(g >= 1e-3, "{g:e}");
        // (a cancelled kernel stops the bisection: cancellation, not an unenclosed gap)
        let token = crate::kernel::CancelToken::default();
        token.cancel();
        let cancelled = Kernel {
            cancel: Some(token),
            ..Kernel::default()
        };
        let stopped = curve_uv_gap_bound(&cancelled, &curve, &uv, &surface, [0.0, 1.0], 1e-9);
        assert!(
            matches!(&stopped, Err(e) if e.is_cancelled()),
            "{stopped:?}"
        );
    }

    /// The top face (`z = 3`) of a 1 x 2 x 3 box: its surface, a plane with a net.
    fn top_plane(k: &mut Kernel) -> Surface {
        let b =
            crate::primitive::api_primitives::create_box(k, Point3::ZERO, 1.0, 2.0, 3.0).unwrap();
        for f in crate::query::api_queries::get_faces(&b).unwrap() {
            let s = crate::topology::face::p2::surface(&b, f)
                .unwrap()
                .unwrap()
                .clone();
            if let crate::surface::surface::p2::SurfaceKind::Plane(p) = s.kind() {
                let uv = p.project_point_to_uv_domain(Point3::new(0.5, 1.0, 7.0));
                if (s.evaluate_point(k, uv).unwrap().z - 3.0).abs() < 1e-12 {
                    return s;
                }
            }
        }
        panic!("no top face");
    }

    /// The UV of `p` dropped onto the plane `s`.
    fn plane_uv(s: &Surface, p: Point3) -> Point2 {
        match s.kind() {
            crate::surface::surface::p2::SurfaceKind::Plane(pl) => pl.project_point_to_uv_domain(p),
            _ => panic!("not a plane"),
        }
    }

    /// A line piece `lift` above a plane, matched to it at its two feet: every point is
    /// exactly `lift` above it, the line and the plane are polynomial, so the bound is at
    /// least `lift` and no more than rounding above it, either way round the parameters run.
    #[test]
    fn a_line_piece_above_a_plane_is_bounded_by_its_height() {
        let mut k = Kernel::default();
        let s = top_plane(&mut k);
        let lift = 6.8e-5;
        let (a, b) = (
            Point3::new(0.2, 0.3, 3.0 + lift),
            Point3::new(0.7, 1.6, 3.0 + lift),
        );
        let line = crate::curve::curve::Curve::from(
            crate::curve::line::Line::create_line_segment(
                3,
                a,
                b,
                Some(&crate::geom::extent1d::Extent1d::from_min_max(0.0, 1.0)),
            )
            .unwrap(),
        );
        let (ua, ub) = (plane_uv(&s, a), plane_uv(&s, b));
        for (t, uv) in [([0.0, 1.0], [ua, ub]), ([1.0, 0.0], [ub, ua])] {
            let g = curve_piece_gap_bound(&k, &line, t, PieceTarget::Surface(&s, uv), 0.0, 1.0)
                .unwrap()
                .unwrap();
            assert!(
                g >= lift && g <= lift * (1.0 + 1e-6),
                "{g:e} against {lift:e}"
            );
        }
        // the half piece matched to the wrong half of the plane: the witness is a bound
        // only, so the bound grows with the mismatch, never below the true gap
        let mid = plane_uv(&s, Point3::new(0.45, 0.95, 3.0));
        let g = curve_piece_gap_bound(
            &k,
            &line,
            [0.0, 1.0],
            PieceTarget::Surface(&s, [ua, mid]),
            0.0,
            1.0,
        )
        .unwrap()
        .unwrap();
        assert!(g >= lift, "{g:e}");
    }

    /// A segment against a point off its middle: the farthest point of the segment is an
    /// end, `sqrt(0.25 + d^2)` away (the segment's second derivative is zero, so the bound
    /// is the end values and rounding).
    #[test]
    fn a_segment_against_a_point_is_bounded_by_its_farther_end() {
        let k = Kernel::default();
        let line = crate::curve::curve::Curve::from(
            crate::curve::line::Line::create_line_segment(
                3,
                Point3::ZERO,
                Point3::new(1.0, 0.0, 0.0),
                Some(&crate::geom::extent1d::Extent1d::from_min_max(0.0, 1.0)),
            )
            .unwrap(),
        );
        let d = 3.0e-3;
        let want = (0.25_f64 + d * d).sqrt();
        let g = curve_piece_gap_bound(
            &k,
            &line,
            [0.0, 1.0],
            PieceTarget::Point(Point3::new(0.5, d, 0.0)),
            0.0,
            10.0,
        )
        .unwrap()
        .unwrap();
        assert!(
            g >= want && g <= want * (1.0 + 1e-9),
            "{g:e} against {want:e}"
        );
    }

    /// An arc of radius `r` over `2 theta` against its chord: the arc's middle is the
    /// sagitta `r (1 - cos theta)` from the chord, and the bound, whatever witness map is
    /// used, is no smaller; at a limit below the sagitta the certificate does not accept.
    #[test]
    fn an_arc_against_its_chord_is_bounded_by_at_least_the_sagitta() {
        let k = Kernel::default();
        let (r, deg) = (2.0_f64, 10.0_f64);
        let arc = crate::curve::curve::Curve::from(
            crate::primitive::api_curves::create_arc(Point3::ZERO, r, -deg, deg).unwrap(),
        );
        let base = NurbsCurve::of(arc.b_spline_base().unwrap()).unwrap();
        let (t0, t1) = (base.u[base.p], base.u[base.n]);
        let th = deg.to_radians();
        let (a, b) = (
            Point3::new(r * th.cos(), -r * th.sin(), 0.0),
            Point3::new(r * th.cos(), r * th.sin(), 0.0),
        );
        let chord = crate::curve::curve::Curve::from(
            crate::curve::line::Line::create_line_segment(
                3,
                a,
                b,
                Some(&crate::geom::extent1d::Extent1d::from_min_max(0.0, 1.0)),
            )
            .unwrap(),
        );
        let sagitta = r * (1.0 - th.cos());
        let g = curve_piece_gap_bound(
            &k,
            &arc,
            [t0, t1],
            PieceTarget::Curve(&chord, [0.0, 1.0]),
            0.0,
            1.0,
        )
        .unwrap()
        .unwrap();
        assert!(
            g >= sagitta && g < 2.0 * sagitta,
            "{g:e} against {sagitta:e}"
        );
        let tol = 0.5 * sagitta;
        let g = curve_piece_gap_bound(
            &k,
            &arc,
            [t0, t1],
            PieceTarget::Curve(&chord, [0.0, 1.0]),
            0.01 * tol,
            tol,
        )
        .unwrap()
        .unwrap();
        assert!(g > tol, "{g:e} accepted at {tol:e}");
    }

    /// The near-tangent counterexample to a reach argued from the crossing angle: a curve
    /// that touches the plane at both ends, `z = 3 + k u (1 - u)(u^2 + eps)`, nearly
    /// tangent at `u = 0` (slope `k eps`), whose middle bows `k / 8`-ish off it. Both ends
    /// lie on the plane, any reach argued from them accepts the piece; the certificate
    /// bounds the bow and refuses at a tolerance below it.
    #[test]
    fn a_piece_on_the_plane_at_both_ends_that_bows_off_it_is_refused() {
        let mut k = Kernel::default();
        let s = top_plane(&mut k);
        let (kk, eps) = (8.0e-4_f64, 1.0e-6_f64);
        // z - 3 in Bernstein form of degree 4: (0, k eps / 4, k eps / 3, k (eps + 1) / 4, 0)
        let z = [
            0.0,
            kk * eps / 4.0,
            kk * eps / 3.0,
            kk * (eps + 1.0) / 4.0,
            0.0,
        ];
        let pts = (0..5)
            .map(|i| CPoint::new(0.2 + 0.5 * i as f64 / 4.0, 0.5, 3.0 + z[i], NOW))
            .collect();
        let bez = NurbCurve::new(
            pts,
            4,
            vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        )
        .unwrap();
        let bow = crate::curve::curve::Curve::from(BSplineCurve::from_nurb(bez, 3, false));
        // the true largest gap, from the polynomial on a fine grid (a lower bound)
        let peak = (0..=1000)
            .map(|i| {
                let u = f64::from(i) / 1000.0;
                kk * u * (1.0 - u) * (u * u + eps)
            })
            .fold(0.0_f64, f64::max);
        let uv = [
            plane_uv(&s, Point3::new(0.2, 0.5, 3.0)),
            plane_uv(&s, Point3::new(0.7, 0.5, 3.0)),
        ];
        let tol = 0.5 * peak;
        let g = curve_piece_gap_bound(
            &k,
            &bow,
            [0.0, 1.0],
            PieceTarget::Surface(&s, uv),
            0.01 * tol,
            tol,
        )
        .unwrap()
        .unwrap();
        assert!(g > tol, "{g:e} accepted at {tol:e} (peak {peak:e})");
        let g = curve_piece_gap_bound(&k, &bow, [0.0, 1.0], PieceTarget::Surface(&s, uv), 0.0, 1.0)
            .unwrap()
            .unwrap();
        assert!(
            g >= peak && g <= peak * (1.0 + 1e-2),
            "{g:e} against {peak:e}"
        );
    }
}
