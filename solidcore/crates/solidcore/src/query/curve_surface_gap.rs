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
use crate::curve::curve::Curve;
use crate::curve::line::Line;
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

    /// Not upstream (#18166): the net moved by `-t` ([`translated_pw`]), with the slack no
    /// moved control point is farther than from the exact moved one. The continuations are
    /// directions only ([`Self::of`] keeps no apex), so they move unchanged.
    fn translated(&self, t: [f64; 3]) -> Option<(Self, f64)> {
        let (pw, slack) = translated_pw(&self.net.pw, t)?;
        let net = Nurbs {
            pw,
            ..self.net.clone()
        };
        Some((
            Self {
                net,
                ends: self.ends,
            },
            slack,
        ))
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

/// `sqrt(x^2 + y^2 + z^2)` of an enclosure, a lower bound (each component's least
/// magnitude); `None` unless finite.
fn norm_lower(v: V3<Ivl>) -> Option<f64> {
    let sq = |a: Ivl| {
        let m = if a.lo > 0.0 {
            a.lo
        } else if a.hi < 0.0 {
            -a.hi
        } else {
            0.0
        };
        let m = Ivl::from(m);
        m * m
    };
    let s = sq(v.x) + sq(v.y) + sq(v.z);
    let lo = s.sqrt().lo.max(0.0);
    (v.x.is_finite() && v.y.is_finite() && v.z.is_finite() && lo.is_finite()).then_some(lo)
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

/// A cut that keeps at least this share of its piece's slack has stopped shrinking it: the
/// slack is the overshoot of the piece's UV box past its net's end, which a shorter piece
/// halves while the box is wider than the trim's own overshoot, and keeps once it is not
/// ([`PieceBound::closes`]).
const SLACK_KEPT: f64 = 0.75;

/// The bound of one piece of a bisection: its end values, its second-order term, the
/// surface's slack past its net, and the term with the slack added (`h² / 8 sup |g''| +
/// slack`, rounded up), the piece's bound being the end values plus the last.
#[derive(Clone, Copy)]
struct PieceBound {
    ends: f64,
    term: f64,
    slack: f64,
    with_slack: f64,
}

impl PieceBound {
    /// A piece of no slack (`with_slack` the term itself).
    fn exact(ends: f64, term: f64) -> Self {
        Self {
            ends,
            term,
            slack: 0.0,
            with_slack: term,
        }
    }

    /// `term` (an enclosure, rounded up here) with `slack` added.
    fn with(ends: f64, term: Ivl, slack: f64) -> Option<Self> {
        let b = Self {
            ends,
            term: term.hi,
            slack,
            with_slack: (term + Ivl::from(slack)).hi,
        };
        (b.term.is_finite() && b.with_slack.is_finite()).then_some(b)
    }

    /// Whether the piece closes: its second term with the slack is at most a `relative` of
    /// its end values or `floor`; or its second term alone is and the cut that made it kept
    /// at least [`SLACK_KEPT`] of its parent's slack `parent` (infinite for a first piece):
    /// cutting it further would leave the slack as it is, so a piece held open by it would be
    /// cut down to [`MAX_DEPTH`] all along its stretch and spend the [`MAX_PIECES`] the other
    /// pieces need (#18166, `single_face_fillet1019_8`: a trim running past its surface's
    /// net, the slack 1.3e-6, held the bound at 0.098 for a gap of 2.2e-4). Closing decides
    /// only how tight the bound is, never whether it holds.
    fn closes(&self, parent: f64, relative: f64, floor: f64) -> bool {
        let at_most = (relative * self.ends).max(floor).max(CLOSE_ABSOLUTE);
        self.with_slack <= at_most
            || (self.term <= at_most && self.slack > 0.0 && self.slack >= SLACK_KEPT * parent)
    }
}

/// The bound of one piece as `(end values, second-order term with the slack)`: the gap over
/// `[a, b]` is at most their sum ([`piece`]).
///
/// # Errors
///
/// Those of [`piece`].
pub(crate) fn piece_bound(
    cn: &NurbsCurve,
    wn: &NurbsCurve,
    s: &NetSurface,
    a: f64,
    b: f64,
) -> Result<Option<(f64, f64)>> {
    Ok(piece(cn, wn, s, a, b)?.map(|p| (p.ends, p.with_slack)))
}

/// The bound of one piece ([`second_term`]).
fn piece(
    cn: &NurbsCurve,
    wn: &NurbsCurve,
    s: &NetSurface,
    a: f64,
    b: f64,
) -> Result<Option<PieceBound>> {
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
    let (Some(c1), Some(c2)) = (cd.get(1), cd.get(2)) else {
        return Ok(None);
    };
    let h = Ivl::from(b) - Ivl::from(a);
    let term = second_term(s, &wd, *c1, *c2, h)?;
    Ok(term.and_then(|(term, slack)| PieceBound::with(ends, term, slack)))
}

/// `ψ`'s value and derivatives up to `ld` over `t`, its value clamped to `range`: `ψ` is a
/// non-rational B-spline whose control values lie in `range` ([`reparametrisation`]), so by the
/// convex hull property its values do as well. `None` when they are not enclosed.
fn psi_over(psi: &NurbsCurve, t: Ivl, ld: usize, [lo, hi]: [f64; 2]) -> Result<Option<Vec<Ivl>>> {
    let mut out: Vec<Ivl> = curve_derivs_over(psi, t, ld, None)?
        .iter()
        .map(|d| d.x)
        .collect();
    let Some(r) = out.first_mut() else {
        return Ok(None);
    };
    let (a, b) = (r.lo.max(lo), r.hi.min(hi));
    if !(a <= b && out.iter().all(|d| d.is_finite())) {
        return Ok(None);
    }
    if let Some(r) = out.first_mut() {
        *r = Ivl::new(a, b);
    }
    Ok(Some(out))
}

/// The curve, the trim and the reparametrisation [`trim_image_gap_bound`] compares, with
/// the knots each may have a corner at.
struct Sides<'a> {
    curve: &'a NurbsCurve,
    trim: &'a NurbsCurve,
    psi: &'a NurbsCurve,
    curve_kinks: Vec<f64>,
    trim_kinks: Vec<f64>,
    range: [f64; 2],
}

/// The bound of one piece `[a, b]` of the trim's parameter `s` of `g = C(ψ(s)) - S(W(s))`:
/// `(end values, second-order term)`, with `(C o ψ)' = C'(ψ) ψ'` and
/// `(C o ψ)'' = C''(ψ) ψ'^2 + C'(ψ) ψ''`, `C`'s derivatives enclosed over the range `ψ` sweeps.
/// Where the trim has a knot inside the piece, or `ψ` sweeps over one of the curve's, that it
/// may not be C^1 across, `g` is only Lipschitz there: the term is `h sup |g'|`, with the
/// one-sided derivatives hulled (as for a C^0 knot line of the surface).
fn piece_reparametrised(
    sides: &Sides<'_>,
    s: &NetSurface,
    a: f64,
    b: f64,
) -> Result<Option<PieceBound>> {
    let Sides {
        curve: cn,
        trim: wn,
        psi,
        range,
        ..
    } = *sides;
    let mut ends = 0.0_f64;
    for t in [a, b] {
        let at = Ivl::from(t);
        let Some(r) = psi_over(psi, at, 0, range)? else {
            return Ok(None);
        };
        let Some(&r0) = r.first() else {
            return Ok(None);
        };
        let c = curve_derivs_over(cn, r0, 0, None)?;
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
    let Some(r) = psi_over(psi, span, 2, range)? else {
        return Ok(None);
    };
    let (Some(&r0), Some(&p1), Some(&p2)) = (r.first(), r.get(1), r.get(2)) else {
        return Ok(None);
    };
    let cd = curve_derivs_over(cn, r0, 2, None)?;
    let wd = curve_derivs_over(wn, span, 2, None)?;
    let (Some(c1), Some(c2), Some(w1)) = (cd.get(1), cd.get(2), wd.get(1)) else {
        return Ok(None);
    };
    let cp = scaled(*c1, p1);
    let h = Ivl::from(b) - Ivl::from(a);
    let corner = sides.trim_kinks.iter().any(|&x| a < x && x < b)
        || sides.curve_kinks.iter().any(|&x| r0.lo < x && x < r0.hi);
    if corner {
        let (p, slack) = s.partials(&wd, 1, 0)?;
        let sw = plus(scaled(p.at(1, 0), w1.x), scaled(p.at(0, 1), w1.y));
        let Some(m) = norm_upper(minus(cp, sw)) else {
            return Ok(None);
        };
        return Ok(PieceBound::with(ends, h * Ivl::from(m), slack));
    }
    let cpp = plus(scaled(*c2, p1 * p1), scaled(*c1, p2));
    let term = second_term(s, &wd, cp, cpp, h)?;
    Ok(term.and_then(|(term, slack)| PieceBound::with(ends, term, slack)))
}

/// The second-order term of a piece of length `h` of `g = C~ - S(W)`, where `wd` are `W`'s
/// derivatives over the piece and `c1`, `c2` the first two derivatives of the curve side `C~`
/// there: `h^2 / 8 * sup |g''|`, or `h * sup |g'|` across a knot line the surface is only C^0
/// across, enclosed; and apart from it the surface's slack past its net (the box's overshoot
/// past the net's end times the first partials, [`NetSurface::partials`]). `None` when it is
/// not enclosed.
fn second_term(
    s: &NetSurface,
    wd: &[V3<Ivl>],
    c1: V3<Ivl>,
    c2: V3<Ivl>,
    h: Ivl,
) -> Result<Option<(Ivl, f64)>> {
    let (Some(w1), Some(w2)) = (wd.get(1), wd.get(2)) else {
        return Ok(None);
    };
    let (u1, v1) = (w1.x, w1.y);
    let (u2, v2) = (w2.x, w2.y);
    // The second order, where the surface is C^1 across every knot line the box crosses:
    // (S o W)'' = S_uu u'^2 + 2 S_uv u'v' + S_vv v'^2 + S_u u'' + S_v v''
    let second = s.partials(wd, 2, 1).map(|(p, slack)| {
        let mut swpp = scaled(p.at(2, 0), u1 * u1);
        swpp = plus(swpp, scaled(p.at(1, 1), Ivl::from(2.0) * u1 * v1));
        swpp = plus(swpp, scaled(p.at(0, 2), v1 * v1));
        swpp = plus(swpp, scaled(p.at(1, 0), u2));
        swpp = plus(swpp, scaled(p.at(0, 1), v2));
        (minus(c2, swpp), slack)
    });
    let term = match second {
        Ok((gpp, slack)) => {
            let Some(m) = norm_upper(gpp) else {
                return Ok(None);
            };
            (h * h * Ivl::from(m) * Ivl::from(0.125), slack)
        }
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        // a knot line the surface is only C^0 across (a patch corner the curve ends on):
        // g is Lipschitz there, |g(t) - g(a)| <= h sup |g'| with the one-sided partials hulled
        Err(_) => {
            let (p, slack) = s.partials(wd, 1, 0)?;
            let sw = plus(scaled(p.at(1, 0), u1), scaled(p.at(0, 1), v1));
            let Some(m) = norm_upper(minus(c1, sw)) else {
                return Ok(None);
            };
            (h * Ivl::from(m), slack)
        }
    };
    Ok(Some(term))
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
    if !(lo.is_finite() && hi.is_finite() && d0 <= lo && lo < hi && hi <= d1) {
        return Ok(None);
    }
    let mut corners = kinks(&cn, lo, hi);
    corners.extend(kinks(&wn, lo, hi));
    corners.sort_by(f64::total_cmp);
    corners.dedup();
    bisect(
        [lo, hi],
        first_pieces,
        &corners,
        CLOSE_RELATIVE,
        floor,
        [f64::NEG_INFINITY, f64::INFINITY],
        None,
        |a, b| {
            k.check_cancelled(OP)?;
            piece(&cn, &wn, surface, a, b)
        },
    )
}

/// The bisection of `[lo, hi]` that bounds a gap `max |g|` from `piece`'s [`PieceBound`] of
/// each piece (the piece's bound its end values plus its second term with the slack): a piece
/// closes as [`PieceBound::closes`] says (its second term small against its end values or
/// `floor`, and its slack too unless the cut that made it no longer shrank it), and one that
/// cannot be cut further is bounded as it stands. `None` when some piece has no bound at all. Once a closed piece's bound is above
/// `over`, so is the whole bound, and the bisection stops there with an infinite bound (an
/// `over` of `f64::INFINITY` never stops it): the pieces before it are cut exactly as without
/// it, so the comparison of the bound with `over` is the same.
///
/// A piece whose bound is at most `enough` closes as it stands (an `enough` of
/// `f64::NEG_INFINITY` never closes one): for a caller that only asks whether the bound is at
/// most `enough`, the pieces cut further would only tighten what already answers it.
///
/// The first pieces are also cut at every point of `corners` (sorted, distinct) inside
/// `(lo, hi)`, so no first piece has one inside it ([`certified_gap`] passes the curves'
/// [`kinks`]).
///
/// `budget` is the most pieces the bisection may make while it may still show the bound at most
/// `enough`, [`MAX_PIECES`] when `None`. Without one a piece left open when the pieces run out
/// is bounded as it stands, as one cut to the deepest; with one, the gap is then not enclosed
/// (`None`): a caller that only acts on a bound at most a few times its tolerance can afford
/// neither the whole budget nor the looser bound for a gap just within `enough`, which only
/// fine pieces close below it.
///
/// With a budget, the pieces already proved are not cut further. Every end value a piece
/// evaluates is part of the bound of a piece that closes, and no cut lowers it (a cut piece's
/// halves evaluate the same ends), so the bound is at least the largest end value `seen` so far:
/// - a piece whose bound is at most `seen` closes as it stands, since cutting it cannot lower
///   the bound, and closing it keeps the bound's value;
/// - once `seen` is above `over`, so is the bound: it is infinite at once;
/// - once `seen` is above `enough`, the bound is not at most `enough` whatever the cuts, and
///   the pieces still open bound a gap the caller acts on: they get the whole [`MAX_PIECES`],
///   not the budget, which only stops the search for a bound at most `enough`. The pieces the
///   budget stopped before (whatever the order the pieces were reached in) are set aside, and
///   cut on once the other pieces are done if `seen` is then above `enough`; if it is not,
///   the gap is not enclosed.
fn bisect(
    [lo, hi]: [f64; 2],
    first_pieces: usize,
    corners: &[f64],
    relative: f64,
    floor: f64,
    [enough, over]: [f64; 2],
    budget: Option<usize>,
    piece: impl Fn(f64, f64) -> Result<Option<PieceBound>>,
) -> Result<Option<f64>> {
    if !(lo.is_finite() && hi.is_finite() && lo < hi) || first_pieces == 0 {
        return Ok(None);
    }
    let f = |j: usize| {
        if j == first_pieces {
            hi
        } else {
            lo + (hi - lo) * (j as f64) / (first_pieces as f64)
        }
    };
    // (each piece with its parent's slack, infinite for a first piece)
    let mut stack: Vec<(f64, f64, u32, f64)> = Vec::new();
    for i in 0..first_pieces {
        let (mut a, b) = (f(i), f(i + 1));
        let start = a;
        for &x in corners.iter().filter(|&&x| start < x && x < b) {
            stack.push((a, x, 0, f64::INFINITY));
            a = x;
        }
        stack.push((a, b, 0, f64::INFINITY));
    }
    stack.reverse();
    let mut worst = 0.0_f64;
    let mut made = stack.len();
    // (the largest end value evaluated, with a budget: the bound is at least it)
    let mut seen = f64::NEG_INFINITY;
    // (the open pieces the budget stopped, in the order they were reached)
    let mut parked: Vec<(f64, f64, u32, f64)> = Vec::new();
    loop {
        while let Some((a, b, depth, parent)) = stack.pop() {
            let mid = 0.5 * (a + b);
            let bottom = depth >= MAX_DEPTH || !(a < mid && mid < b);
            // (the budget holds while the bound may still be at most `enough`; a piece that
            // cannot be cut further is bounded as it is: the bound holds whether or not the
            // second term has shrunk, closing only makes it tight)
            let out_of = |seen: f64| {
                let capped = budget.filter(|_| seen <= enough);
                (capped.is_some(), made + 2 > capped.unwrap_or(MAX_PIECES))
            };
            let mut slack = f64::INFINITY;
            match piece(a, b) {
                Ok(Some(p)) => {
                    let bound = (Ivl::from(p.ends) + Ivl::from(p.with_slack)).hi;
                    if budget.is_some() {
                        seen = seen.max(p.ends);
                        if seen > over {
                            return Ok(Some(f64::INFINITY));
                        }
                    }
                    let (capped, out_of_pieces) = out_of(seen);
                    let proved = budget.is_some() && bound <= seen;
                    let closed = bound <= enough || proved || p.closes(parent, relative, floor);
                    // (an open piece the budget stopped is not bounded as it stands)
                    if !closed && capped && out_of_pieces {
                        parked.push((a, b, depth, parent));
                        continue;
                    }
                    if bottom || out_of_pieces || closed {
                        if bound > over {
                            return Ok(Some(f64::INFINITY));
                        }
                        worst = worst.max(bound);
                        continue;
                    }
                    slack = p.slack;
                }
                Err(e) if e.is_unported_or_cancelled() => return Err(e),
                // a box past the net, on a knot line of too low a continuity: a smaller piece
                // may clear it; at the bottom the gap is not enclosed
                Ok(None) | Err(_) => {
                    let (capped, out_of_pieces) = out_of(seen);
                    if capped && out_of_pieces {
                        parked.push((a, b, depth, parent));
                        continue;
                    }
                    if bottom || out_of_pieces {
                        return Ok(None);
                    }
                }
            }
            made += 2;
            stack.push((mid, b, depth + 1, slack));
            stack.push((a, mid, depth + 1, slack));
        }
        // (a gap the budget stopped while it may still be at most `enough` is not enclosed;
        // past `enough`, the stopped pieces are cut on without it)
        if parked.is_empty() {
            break;
        }
        if seen <= enough {
            return Ok(None);
        }
        parked.reverse();
        stack.append(&mut parked);
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
pub fn curve_uv_gap_bound(
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

/// A proved lower bound on `|C(t) − S(W(t))|` at the one parameter `t`, for the curves and
/// surface of [`curve_uv_gap_bound`]: the same enclosures of `C(t)`, `W(t)` and the net's
/// point over the box `W(t)` encloses, every rounding included, and the norm's lower end.
/// It bounds the supremum [`curve_uv_gap_bound`] encloses from below (not upstream,
/// tangency-labs/solidcraft#18195, a witness for ranking two gaps).
///
/// `None` when the gap at `t` is not enclosed: `t` outside either curve's domain or not
/// finite, a surface that is not a NURBS net or whose net's parameters are not the
/// surface's (probed at `W(t)`), or a box `W(t)` too far past the net's end. A box past the
/// end by rounding has the continued polynomial's slack taken off.
///
/// # Errors
///
/// The kernel's unported and cancelled statuses.
pub fn curve_uv_gap_lower_bound(
    k: &Kernel,
    curve: &BSplineCurve,
    uv: &BSplineCurve,
    surface: &Surface,
    t: f64,
) -> Result<Option<f64>> {
    // (the homogeneous points as stored, so the curves enclosed are the ones evaluated: a
    // rebuild from the Euclidean points rounds a rational point's `x w`)
    let (Some(cn), Some(wn)) = (
        polynomial_pieces::stored_nurbs(curve),
        polynomial_pieces::stored_nurbs(uv),
    ) else {
        return Ok(None);
    };
    let inside = |n: &NurbsCurve| matches!((n.u.get(n.p), n.u.get(n.n)), (Some(&d0), Some(&d1)) if d0 <= t && t <= d1);
    if !(t.is_finite() && inside(&cn) && inside(&wn)) {
        return Ok(None);
    }
    let Ok(w) = uv.evaluate_point(t) else {
        return Ok(None);
    };
    let surface = surface.clone_for_evaluation()?;
    let Some(net) = NetSurface::of(k, &surface, &[Point2::new(w.x, w.y)])? else {
        return Ok(None);
    };
    let at = Ivl::from(t);
    let c = curve_derivs_over(&cn, at, 0, None)?;
    let w = curve_derivs_over(&wn, at, 0, None)?;
    let Some(c0) = c.first().copied() else {
        return Ok(None);
    };
    // a box past the net's end (rounding of a parameter on the boundary) is the continued
    // polynomial `S~` there: `|C - S(clamp W)| >= |C - S~(W)| - slack`, the slack of
    // [`NetSurface::partials`] bounding `|S~(W) - S(clamp W)|`
    let (surf, slack) = match net.partials(&w, 1, 0) {
        Ok((p, slack)) => (p.at(0, 0), slack),
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        Err(_) => return Ok(None),
    };
    Ok(norm_lower(minus(c0, surf)).map(|d| (Ivl::from(d) - Ivl::from(slack)).lo.max(0.0)))
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

/// Not upstream (#18166): a proved upper bound on the distance from every point of `curve`
/// over its parameters `range` to the surface of face `face` over that surface's parameter
/// box, as [`curve_face_gap_bound`], but one that tends to the largest distance itself as the
/// pieces shrink. [`curve_face_gap_bound`] matches the curve to the surface by one fixed
/// correspondence through the nearest points of [`CORRESPONDENCE_POINTS`] samples, so its
/// bound stops at that correspondence's own error between the samples (a curve lying in a
/// plane face, 3e-14 from it, was bounded at 4.9e-5: `primitive.api_primitives` gen0083).
/// Here every piece's two ends are matched to their own nearest points on the surface, and
/// the piece to the straight map between them in the surface's parameters ([`surface_piece`]):
/// each end value is then the end's distance to the surface, a cut drops the curve's point at
/// the cut afresh, and the second-order term shrinks with the piece.
///
/// A piece whose bound is at most `enough` closes as it stands, and so does one within the
/// largest end value `seen` so far (the bound is at least that, and no cut lowers it but by
/// rounding: the halves keep their ends' nearest points). While `seen` is at most `enough` a
/// piece above it is cut until it closes below `enough`, at [`CLOSE_ABSOLUTE`] or at the
/// bottom, so a gap just within `enough` is shown within it; once `seen` is above `enough`
/// the pieces close at a thousandth of their end values or `floor`, as
/// [`curve_face_gap_bound`]'s. Once `seen` is above `over`, so is the bound: it is infinite at
/// once. An `enough` of `f64::NEG_INFINITY` and an `over` of `f64::INFINITY` give the bound to
/// the tightness `floor` asks.
///
/// Each nearest point is a witness only (`dist(C(t), S) <= |C(t) - S(w)|` for any `w` in the
/// parameter box): where the surface's point is not dropped at a cut, the straight map's
/// middle stands for it, and the bound holds either way.
///
/// `None` when the gap is not enclosed: as [`curve_face_gap_bound`], and where the nearest
/// point of one of the first pieces' ends is not found.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling face; the kernel's unported and cancelled statuses.
pub fn curve_face_gap_bound_within(
    k: &Kernel,
    brep: &crate::topology::Brep,
    face: crate::topology::FaceId,
    curve: &BSplineCurve,
    range: [f64; 2],
    floor: f64,
    [enough, over]: [f64; 2],
) -> Result<Option<f64>> {
    use crate::error::ignored;
    use crate::surface::surface::p4::NoObject;
    use crate::surface::surface::p5::DropPointOptions;

    let [lo, hi] = range;
    if !(lo.is_finite() && hi.is_finite() && lo < hi && floor.is_finite() && floor >= 0.0) {
        return Ok(None);
    }
    let rec = brep.face(face)?;
    let Some(surface) = rec.surface.as_ref() else {
        return Ok(None);
    };
    let surface = surface.clone_for_evaluation()?;
    let domain = &rec.uv_domain;
    let Ok(cn) = NurbsCurve::of(curve) else {
        return Ok(None);
    };
    let (Some(&d0), Some(&d1)) = (cn.u.get(cn.p), cn.u.get(cn.n)) else {
        return Ok(None);
    };
    if !(d0 <= lo && hi <= d1) {
        return Ok(None);
    }
    let evaluated = Curve::from(curve.clone_exact());
    // (the nearest point of the curve's point at `t`, from `guess`; `None` where it is not found)
    let drop_at = |t: f64, guess: Option<Point2>| -> Result<Option<Point2>> {
        let Some(point) =
            ignored(evaluated.evaluate(k, t, 0, true, false))?.and_then(|d| d.into_iter().next())
        else {
            return Ok(None);
        };
        let opts = DropPointOptions {
            guess_uv: guess,
            ..DropPointOptions::default()
        };
        let drop = ignored(surface.drop_point::<NoObject>(k, point, domain, &opts))?;
        Ok(drop.filter(|d| d.success).map(|d| d.result_uv))
    };
    // The first pieces' ends: equally spaced, and every kink of the curve, so no first piece
    // has a corner inside it ([`certified_gap`]); their nearest points are followed from the
    // middle out to each end, each drop started from its neighbour's ([`correspondence`]).
    let mut cuts: Vec<f64> = (0..=FIRST_PIECES)
        .map(|j| {
            if j == FIRST_PIECES {
                hi
            } else {
                lo + (hi - lo) * (j as f64) / (FIRST_PIECES as f64)
            }
        })
        .collect();
    cuts.extend(kinks(&cn, lo, hi));
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    let n = cuts.len();
    let mid = n / 2;
    let mut feet: Vec<Option<Point2>> = vec![None; n];
    let order = (mid..n)
        .map(|i| (i, (i > mid).then(|| i - 1)))
        .chain((0..mid).rev().map(|i| (i, Some(i + 1))));
    for (i, from) in order {
        let Some(&t) = cuts.get(i) else {
            return Ok(None);
        };
        let guess = from.and_then(|j| feet.get(j).copied().flatten());
        let Some(uv) = drop_at(t, guess)? else {
            return Ok(None);
        };
        if let Some(slot) = feet.get_mut(i) {
            *slot = Some(uv);
        }
    }
    let feet: Vec<Point2> = feet.into_iter().flatten().collect();
    let Some(net) = NetSurface::of(k, &surface, &feet)? else {
        return Ok(None);
    };
    // (the bound of the curve data `cn` against the net `net`, with the pieces closing at
    // `enough`; infinite past `over`)
    let run = |cn: &NurbsCurve, net: &NetSurface, enough: f64| -> Result<Option<f64>> {
        // (each piece with its ends' nearest points, its depth and its parent's slack, infinite
        // for a first piece)
        let mut stack: Vec<(f64, f64, [Point2; 2], u32, f64)> = Vec::new();
        for (ts, ws) in cuts.windows(2).zip(feet.windows(2)) {
            if let ([a, b], [wa, wb]) = (ts, ws) {
                stack.push((*a, *b, [*wa, *wb], 0, f64::INFINITY));
            }
        }
        stack.reverse();
        let mut worst = 0.0_f64;
        let mut made = stack.len();
        let mut seen = f64::NEG_INFINITY;
        while let Some((a, b, ends, depth, parent)) = stack.pop() {
            k.check_cancelled(OP)?;
            let mid = 0.5 * (a + b);
            let last = depth >= MAX_DEPTH || !(a < mid && mid < b) || made + 2 > MAX_PIECES;
            let mut slack = f64::INFINITY;
            match surface_piece(cn, [a, b], net, ends, a, b) {
                Ok(Some(p)) => {
                    let bound = (Ivl::from(p.ends) + Ivl::from(p.with_slack)).hi;
                    seen = seen.max(p.ends);
                    if seen > over {
                        return Ok(Some(f64::INFINITY));
                    }
                    // (while the bound may still be at most `enough`, a piece above it is cut on)
                    let (relative, at_least) = if seen <= enough {
                        (0.0, 0.0)
                    } else {
                        (CLOSE_RELATIVE, floor)
                    };
                    let closed =
                        bound <= enough || bound <= seen || p.closes(parent, relative, at_least);
                    if last || closed {
                        if bound > over {
                            return Ok(Some(f64::INFINITY));
                        }
                        worst = worst.max(bound);
                        continue;
                    }
                    slack = p.slack;
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
            let [wa, wb] = ends;
            let straight = Point2::new(0.5 * (wa.x + wb.x), 0.5 * (wa.y + wb.y));
            let wm = drop_at(mid, Some(straight))?.unwrap_or(straight);
            made += 2;
            stack.push((mid, b, [wm, wb], depth + 1, slack));
            stack.push((a, mid, [wa, wm], depth + 1, slack));
        }
        Ok(worst.is_finite().then_some(worst))
    };
    let first = run(&cn, &net, enough)?;
    recentred_retry(first, [enough, over], &cn, &net, run)
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
    Curve(&'a Curve, [f64; 2]),
}

/// The NURBS data a curve evaluates over `[lo, hi]`, with the error bound of its points
/// against the curve's own: the B-spline a B-spline, ellipse, circle, hyperbola or parabola
/// evaluates (no error: it is that B-spline), and for a line the segment between its two
/// computed end points, each within `e` per coordinate of the line's (so every point of the
/// line over `[lo, hi]`, a convex combination of the exact ends, is within `sqrt(3) e <= 2 e`
/// of the segment's point at that parameter). `None` for other kinds, a NURB that is not
/// well formed, and a range outside the curve's domain.
fn piece_nurbs(curve: &Curve, [lo, hi]: [f64; 2]) -> Option<(NurbsCurve, f64)> {
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

/// The bound of one piece `[a, b]` of the curve against the surface `s`, matched to it by the
/// straight map from `range` onto `[w0, w1]` ([`target_piece`]'s surface target): the larger
/// end value plus `h^2 / 8 sup |g''|` with the surface's slack past its net, or the Lipschitz
/// form `h sup |g'|` where the box crosses a knot line the surface is only C^0 across. `None`
/// when some enclosure is not finite.
fn surface_piece(
    cn: &NurbsCurve,
    range: [f64; 2],
    s: &NetSurface,
    [w0, w1]: [Point2; 2],
    a: f64,
    b: f64,
) -> Result<Option<PieceBound>> {
    let mut ends = 0.0_f64;
    for t in [a, b] {
        let at = Ivl::from(t);
        let Some(c0) = curve_derivs_over(cn, at, 0, None)?.first().copied() else {
            return Ok(None);
        };
        let w = V3 {
            x: clamp_ivl(along(at, range, [w0.x, w1.x]), w0.x, w1.x),
            y: clamp_ivl(along(at, range, [w0.y, w1.y]), w0.y, w1.y),
            z: Ivl::from(0.0),
        };
        let q = s.partials(&[w], 0, 0)?.0.at(0, 0);
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
            (h * h * Ivl::from(m) * Ivl::from(0.125), slack)
        }
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        Err(_) => {
            let (p, slack) = s.partials(&[w], 1, 0)?;
            let sw = plus(scaled(p.at(1, 0), u1), scaled(p.at(0, 1), v1));
            let Some(m) = norm_upper(minus(c1, sw)) else {
                return Ok(None);
            };
            (h * Ivl::from(m), slack)
        }
    };
    Ok(PieceBound::with(ends, term.0, term.1))
}

/// The bound of one piece `[a, b]` of the curve against the target (a surface's with its slack
/// past its net, as [`second_term`]), `None` when some enclosure is not finite (module docs; the same chord bound as [`piece`] with the straight map in place of the
/// correspondence `W`). A target curve whose
/// knot lies inside the piece's image, or a surface only C^0 across a knot line the box
/// crosses, is bounded by the Lipschitz form, `h sup |g'|`.
fn target_piece(
    cn: &NurbsCurve,
    range: [f64; 2],
    target: &Prepared,
    a: f64,
    b: f64,
) -> Result<Option<PieceBound>> {
    if let Prepared::Surface(s, w) = target {
        return surface_piece(cn, range, s, *w, a, b);
    }
    let mut ends = 0.0_f64;
    for t in [a, b] {
        let at = Ivl::from(t);
        let Some(c0) = curve_derivs_over(cn, at, 0, None)?.first().copied() else {
            return Ok(None);
        };
        let q = match target {
            Prepared::Point(p) => *p,
            // (bounded by `surface_piece` above)
            Prepared::Surface(..) => return Ok(None),
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
        // (bounded by `surface_piece` above)
        Prepared::Surface(..) => return Ok(None),
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
    Ok(term.is_finite().then(|| PieceBound::exact(ends, term)))
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
    curve: &Curve,
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
    // (each piece with its parent's slack, infinite for a first piece)
    let mut stack: Vec<(f64, f64, u32, f64)> = cuts
        .windows(2)
        .rev()
        .filter_map(|w| match *w {
            [a, b] => Some((a, b, 0, f64::INFINITY)),
            _ => None,
        })
        .collect();
    let extra = err + target_err;
    let mut worst = 0.0_f64;
    let mut made = stack.len();
    while let Some((a, b, depth, parent)) = stack.pop() {
        let mid = 0.5 * (a + b);
        let last = depth >= MAX_DEPTH || !(a < mid && mid < b) || made + 2 > MAX_PIECES;
        let mut slack = f64::INFINITY;
        match target_piece(&cn, range, &prepared, a, b) {
            Ok(Some(p)) => {
                let bound = (Ivl::from(p.ends) + Ivl::from(p.with_slack) + Ivl::from(extra)).hi;
                if !bound.is_finite() {
                    return Ok(None);
                }
                // (past the limit at an end: no cut can bring it back)
                if (Ivl::from(p.ends) + Ivl::from(extra)).hi > limit {
                    return Ok(Some(bound));
                }
                if last || p.closes(parent, CLOSE_RELATIVE, floor) {
                    worst = worst.max(bound);
                    continue;
                }
                slack = p.slack;
            }
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Ok(None) | Err(_) => {
                if last {
                    return Ok(None);
                }
            }
        }
        made += 2;
        stack.push((mid, b, depth + 1, slack));
        stack.push((a, mid, depth + 1, slack));
    }
    Ok(worst.is_finite().then_some(worst))
}

/// Points of a UV trim at which its surface's net is checked to give the surface's point
/// ([`NetSurface::of`]): a sanity probe that the net is parameterised as the surface, never
/// part of the bound.
const TRIM_PROBES: usize = 9;

/// The exact NURBS data of `c` as the kernel evaluates it, with the rounding by which its
/// points can differ from that data's: a B-spline or an ellipse is its NURB (the kernel
/// evaluates it there, so the slack is zero); a line is evaluated analytically, `P + s V`,
/// and its NURB is the segment through its two control points, the slack how far the two can
/// be apart ([`line_nurb_slack`]).
/// `None` for any other kind, or data that is not a valid NURBS net.
fn exact_curve_data(c: &Curve, is_2d: bool) -> Result<Option<(NurbsCurve, f64)>> {
    use crate::curve::curve::CurveKind;
    use crate::surface::surface::exact_pieces::exact_nurbs_curve_data;
    let line_base;
    let (bs, line) = match c.kind() {
        CurveKind::BSpline(bs) => (bs, None),
        CurveKind::Ellipse(e) => (e.base(), None),
        CurveKind::Line(l) => {
            line_base = l.base_with_nurb()?;
            (&*line_base, Some(l))
        }
        _ => return Ok(None),
    };
    let Ok((cn, _, _, _, _)) = exact_nurbs_curve_data(bs, is_2d) else {
        return Ok(None);
    };
    let Some(l) = line else {
        return Ok(Some((cn, 0.0)));
    };
    Ok(line_nurb_slack(l, &cn, is_2d).map(|slack| (cn, slack)))
}

/// A bound `L` with `|S(p) - S(q)| <= L d` for the plane `s` and UV points `p`, `q` within `d`
/// of each other in each coordinate, both within `d` of the segment between the UV points
/// `ends` (a UV line's NURB): `None` unless `s` is a plane whose NURB is a polynomial bilinear
/// patch. Over its domain `[u0, u1] x [v0, v1]` with corners `P_ij`, `S_u` at the `v` fraction
/// `β` is `((1 - β)(P_10 - P_00) + β (P_11 - P_01)) / (u1 - u0)`, whose norm is convex in `β`,
/// so over any `β` range it is at most the larger of its values at the range's ends; the range
/// taken is the segment's box widened by `d`, joined with `[0, 1]`. Likewise `S_v`, and
/// `|S(p) - S(q)| <= (max |S_u| + max |S_v|) d`. A difference of two floats rounds by at most
/// `ε` of itself, a quotient, product or square root likewise; each is covered by a factor
/// `1 + 16 ε` (and an absolute `8 ε` of the box's coordinates for its fractions).
///
/// Not upstream (#18166): it carries a UV line's evaluation slack ([`line_evaluation_slack`])
/// to the plane, for a trim that is a line.
#[must_use]
pub fn plane_uv_lipschitz(s: &Surface, ends: &[crate::nurbs::math::CPoint], d: f64) -> Option<f64> {
    use crate::nurbs::math::{CPoint, NOW};
    use crate::surface::surface::p2::SurfaceKind;
    let SurfaceKind::Plane(plane) = s.kind() else {
        return None;
    };
    let base = plane.base_with_nurb().ok()?;
    let net = base.gw_nurb()?;
    let [u0, _, _, u1] = <[f64; 4]>::try_from(net.u_knots()).ok()?;
    let [v0, _, _, v1] = <[f64; 4]>::try_from(net.v_knots()).ok()?;
    let rows = net.cpts();
    if net.degrees() != (1, 1)
        || !(u1 > u0 && v1 > v0)
        || rows.len() != 2
        || rows.iter().any(|r| r.len() != 2)
    {
        return None;
    }
    let polynomial = |c: &CPoint| c.w == NOW || c.w == 1.0;
    let pt = |i: usize, j: usize| -> Option<[f64; 3]> {
        let c = rows.get(i)?.get(j)?;
        polynomial(c).then_some([c.x, c.y, c.z])
    };
    let (p00, p10, p01, p11) = (pt(0, 0)?, pt(1, 0)?, pt(0, 1)?, pt(1, 1)?);
    let grow = 1.0 + 16.0 * f64::EPSILON;
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for c in ends {
        if !polynomial(c) {
            return None;
        }
        for ((l, h), x) in lo.iter_mut().zip(hi.iter_mut()).zip([c.x, c.y]) {
            *l = l.min(x - d);
            *h = h.max(x + d);
        }
    }
    // the fraction of `x` in `[a, b]`, moved out by its rounding (`lower` toward -inf)
    let frac = |x: f64, a: f64, b: f64, lower: bool| {
        let r = (x - a) / (b - a);
        let e = (r.abs() * 16.0 + (x.abs() + a.abs()) / (b - a) * 8.0) * f64::EPSILON;
        if lower {
            (r - e).min(0.0)
        } else {
            (r + e).max(1.0)
        }
    };
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    // `|(1 - t) a + t b|` rounded up: each term within `(|a| + |b|)(1 + |t|) 4 ε` of exact
    let norm_at = |a: [f64; 3], b: [f64; 3], t: f64| {
        let [a0, a1, a2] = a;
        let [b0, b1, b2] = b;
        let v = [
            (1.0 - t) * a0 + t * b0,
            (1.0 - t) * a1 + t * b1,
            (1.0 - t) * a2 + t * b2,
        ];
        let m = a.iter().chain(&b).fold(0.0_f64, |m, x| m.max(x.abs()));
        ((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
            + m * (1.0 + t.abs()) * 16.0 * f64::EPSILON)
            * grow
    };
    let su = |t: f64| norm_at(sub(p10, p00), sub(p11, p01), t) * grow;
    let sv = |t: f64| norm_at(sub(p01, p00), sub(p11, p10), t) * grow;
    let (b_lo, b_hi) = (frac(lo[1], v0, v1, true), frac(hi[1], v0, v1, false));
    let (a_lo, a_hi) = (frac(lo[0], u0, u1, true), frac(hi[0], u0, u1, false));
    let lu = su(b_lo).max(su(b_hi)) / ((u1 - u0) / grow);
    let lv = sv(a_lo).max(sv(a_hi)) / ((v1 - v0) / grow);
    let l = (lu + lv) * grow;
    l.is_finite().then_some(l)
}

/// Not upstream: [`line_nurb_slack`] of the line `l` and its NURB (made on a copy when it has
/// none), a UV line when `is_2d`: how far its analytic point can be from its NURB's exact point
/// at the same parameter. `None` where that NURB's data is not two control points of weight
/// one, or a number is not finite.
#[must_use]
pub fn line_evaluation_slack(l: &Line, is_2d: bool) -> Option<f64> {
    use crate::surface::surface::exact_pieces::exact_nurbs_curve_data;
    let base = l.base_with_nurb().ok()?;
    let (cn, _, _, _, _) = exact_nurbs_curve_data(&base, is_2d).ok()?;
    line_nurb_slack(l, &cn, is_2d)
}

/// Not upstream: a bound on the distance between a line's analytic point (`P + s scale V`,
/// [`Line::evaluate_point`]) at a NURB parameter `t` of the domain of `cn`, the line's NURB
/// data (two control points `Q_0`, `Q_1` of weight one), and the exact point of that NURB at
/// `t`. With `E(t)` the line's exact point (affine in `t`): the analytic point is within
/// `e(t)` ([`Curve::point_error_bound`]) of `E(t)` in each coordinate, and `e` grows with `|s|`,
/// affine in `t`, so it is at most its larger value at the domain's ends `t_0`, `t_1`. Each
/// `Q_i` is within `|Q_i - A(t_i)| + e(t_i)` of `E(t_i)` in each coordinate (`A` the analytic
/// point, the difference measured and rounded up), so the NURB's point, the blend of the `Q_i`
/// by `1 - λ` and `λ` with `E` affine, is within the larger of those of `E(t)`. The sum of the
/// two, per coordinate, times `√3` for the distance, rounded up. Unlike a bound in the size of
/// the NURB's own points, this holds for a piece of a line far from its point `P`, and for a
/// NURB that is not the line's (the difference is counted). `None` for data of another shape or
/// numbers not finite.
fn line_nurb_slack(l: &Line, cn: &NurbsCurve, is_2d: bool) -> Option<f64> {
    if cn.p != 1 || cn.n != 2 {
        return None;
    }
    let (t0, t1) = (*cn.u.get(1)?, *cn.u.get(2)?);
    let up = |x: f64| x * (1.0 + 4.0 * f64::EPSILON);
    let dims = if is_2d { 2 } else { 3 };
    let mut e_max = 0.0_f64;
    let mut q_max = 0.0_f64;
    for (i, t) in [t0, t1].into_iter().enumerate() {
        let q = cn.pw.get(4 * i..4 * i + 4)?;
        if q.get(3) != Some(&1.0) {
            return None;
        }
        let e = l.point_error(t)?;
        let a = l.evaluate_point(t);
        let off = [a.x, a.y, a.z]
            .iter()
            .zip(q)
            .take(dims)
            .fold(0.0_f64, |m, (a, q)| m.max(up((a - q).abs())));
        e_max = e_max.max(e);
        q_max = q_max.max(up(off + e));
    }
    let slack = up(up(e_max + q_max) * up(3.0_f64.sqrt()));
    slack.is_finite().then_some(slack)
}

/// Newton steps toward the nearest curve parameter of each trim image point ([`reparametrisation`]):
/// only how tight the bound is depends on them, never whether it holds.
const NEAREST_STEPS: usize = 6;
/// The most points `ψ` passes through, and the deepest it halves a gap between two of them
/// ([`reparametrisation`]).
const PSI_MAX_POINTS: usize = 4097;
const PSI_MAX_DEPTH: u32 = 16;
/// The narrowest gap between two points of `ψ` that halving makes: a piece of the bisection
/// this many halvings above its deepest ([`MAX_DEPTH`]), so a steep stretch of `ψ` spans
/// pieces the bisection can still split (`ψ` steep over a narrower gap leaves its second-order
/// term large in the deepest piece that holds it).
const PSI_MIN_GAP_LEVELS: u32 = 10;
/// The pieces each knot span of the curve and of the trim is cut into for `ψ`'s first points.
const PSI_PER_SPAN: usize = 4;

/// The curve parameter near `guess` (within `[lo, hi]`) of the curve point nearest `target`,
/// by a few Newton steps, with the curve's speed there; the guess itself where a step fails.
/// The parameter returned is the one of the steps' points (the guess included) nearest
/// `target`: where the curve is slow, a step can overshoot to a far stretch of it, and a
/// point of `ψ` there pulls `C(ψ)` off the image (#18166, `brep_mirror` `mat0094`).
fn nearest_param(
    k: &Kernel,
    curve: &Curve,
    target: crate::math::Point3,
    guess: f64,
    [lo, hi]: [f64; 2],
) -> Result<(f64, f64)> {
    use crate::error::ignored;
    let mut t = guess;
    // (parameter, distance, speed) of the nearest point so far
    let mut best = (guess, f64::INFINITY, 0.0);
    for step in 0..=NEAREST_STEPS {
        let Some(d) = ignored(curve.evaluate(k, t, 1, true, false))? else {
            break;
        };
        let (Some(&c0), Some(&c1)) = (d.first(), d.get(1)) else {
            break;
        };
        let den = c1.dot(c1);
        let dist = c0.distance(target);
        if dist < best.1 {
            best = (t, dist, den.sqrt());
        }
        let next = t + (target - c0).dot(c1) / den;
        if step == NEAREST_STEPS || !(den > 0.0 && next.is_finite()) {
            break;
        }
        t = next.max(lo).min(hi);
    }
    Ok((best.0, best.2))
}

/// The reparametrisation `ψ` of the curve onto the trim's parameter `s` over `range`: the C^1
/// cubic [`hermite_psi`] through points `(s_i, t_i)`, `t_i` the parameter of the curve point
/// nearest the trim image at `s_i` (a few Newton steps), `ψ(lo) = lo` and `ψ(hi) = hi`. The
/// points are uniform and cut each span between the `knots` (the curve's and the trim's) into
/// [`PSI_PER_SPAN`], and a gap between
/// two is halved where the nearest parameter at its middle is off their mean by more than
/// `refine` along the curve (a trim image that runs ahead or behind the curve over a short
/// stretch), up to [`PSI_MAX_POINTS`] and [`PSI_MAX_DEPTH`]. The built `ψ` is then checked at
/// each gap's middle and refined where `C(ψ)` there is farther from the image than at both of
/// the gap's ends by more than `refine`, in at most [`PSI_MAX_DEPTH`] rounds. Neither halves a
/// gap below twice the piece of [`PSI_MIN_GAP_LEVELS`]: where the nearest parameter jumps (the
/// trim's image leaves one stretch of the curve for another, #18166 `feature_executive`
/// `single_face_fillet1019_8`), halving would chase the jump down to a gap a few units in the
/// last place wide, and `ψ`'s steep step there would hold the bound at 15. Any `ψ` gives a
/// bound; this one keeps `C(ψ(s))` near the image point at `s`, so the bound measures the
/// distance between the two curves, not how far apart their common parameters slip. `None`
/// when it cannot be built.
fn reparametrisation(
    k: &Kernel,
    surface: &Surface,
    curve: &Curve,
    uv: &Curve,
    [lo, hi]: [f64; 2],
    knots: &[f64],
    refine: f64,
) -> Result<Option<NurbsCurve>> {
    use crate::error::ignored;

    let image = |si: f64| -> Result<Option<crate::math::Point3>> {
        let Some(w) = ignored(uv.evaluate_point(k, si))? else {
            return Ok(None);
        };
        ignored(surface.evaluate_point(k, Point2::new(w.x, w.y)))
    };
    // the first points: uniform, and each knot span of the trim and of the curve (on the
    // curve's parameter, close to the trim's) cut into a few, where the image can turn
    let uniform = CORRESPONDENCE_POINTS - 1;
    let mut breaks: Vec<f64> = vec![lo];
    breaks.extend(knots.iter().copied().filter(|&x| lo < x && x < hi));
    breaks.push(hi);
    breaks.sort_by(f64::total_cmp);
    breaks.dedup();
    let mut at: Vec<f64> = (0..uniform)
        .map(|i| lo + (hi - lo) * (i as f64) / (uniform as f64))
        .collect();
    if (breaks.len() - 1) * PSI_PER_SPAN + uniform < PSI_MAX_POINTS {
        for w in breaks.windows(2) {
            if let [a, b] = w {
                at.extend(
                    (0..PSI_PER_SPAN).map(|j| a + (b - a) * (j as f64) / (PSI_PER_SPAN as f64)),
                );
            }
        }
    }
    at.push(hi);
    at.retain(|&x| lo <= x && x <= hi);
    at.sort_by(f64::total_cmp);
    // (a knot at a uniform point, rounded apart by a few units in the last place: two points
    // that close, each with its own nearest parameter, would make `ψ` all but vertical there;
    // the ends stay `lo` and `hi`)
    at.dedup_by(|x, prev| *x - *prev <= 4.0 * f64::EPSILON * x.abs().max(prev.abs()));
    if let Some(last) = at.last_mut() {
        *last = hi;
    }
    if at.len() < 2 {
        at = vec![lo, hi];
    }
    let n = at.len();
    // the narrowest gap halving makes: a bisection piece `PSI_MIN_GAP_LEVELS` above its deepest
    let min_gap =
        (hi - lo) / (FIRST_PIECES as f64) / f64::from(1_u32 << (MAX_DEPTH - PSI_MIN_GAP_LEVELS));
    let mut first: Vec<(f64, f64)> = Vec::new();
    for (i, &si) in at.iter().enumerate() {
        k.check_cancelled(OP)?;
        let Some(p) = image(si)? else {
            return Ok(None);
        };
        let t = if i == 0 {
            lo
        } else if i + 1 == n {
            hi
        } else {
            nearest_param(k, curve, p, si, [lo, hi])?.0
        };
        first.push((si, t));
    }
    let mut points: Vec<(f64, f64)> = first.iter().take(1).copied().collect();
    let mut count = n;
    for pair in first.windows(2) {
        let [a, b] = pair else {
            return Ok(None);
        };
        let mut stack = vec![(*a, *b, 0_u32)];
        while let Some((a, b, depth)) = stack.pop() {
            let m = 0.5 * (a.0 + b.0);
            if depth < PSI_MAX_DEPTH
                && count < PSI_MAX_POINTS
                && b.0 - a.0 >= 2.0 * min_gap
                && a.0 < m
                && m < b.0
            {
                k.check_cancelled(OP)?;
                let guess = 0.5 * (a.1 + b.1);
                let Some(p) = image(m)? else {
                    return Ok(None);
                };
                let (tm, speed) = nearest_param(k, curve, p, guess, [lo, hi])?;
                if (tm - guess).abs() * speed > refine {
                    count += 1;
                    stack.push(((m, tm), b, depth + 1));
                    stack.push((a, (m, tm), depth + 1));
                    continue;
                }
            }
            points.push(b);
        }
    }
    // The halving above compares the nearest parameter at a gap's middle with the mean of its
    // ends, but `ψ` is a cubic whose slopes come from the neighbouring points: a gap whose ends
    // are on the correspondence can still bow off it (a slope set to zero where the points turn
    // back over a sub-micron knot span leaves the next gap lagging by 4/27 of its width, #18166).
    // So the built `ψ` is checked at each gap's middle: where `C(ψ)` there is farther from the
    // image than at both of the gap's ends by more than `refine`, it bows the gap's `g` above
    // what its end values show, and the nearest point found from `ψ` is added. The test is on
    // the distance, not the parameter: where the parameter is off but the distance is not (the
    // last gap, where `ψ(hi) = hi` holds `ψ` to the curve's end while the image's nearest point
    // is short of it), a point would only make `ψ` steeper and its second-order term larger.
    // `ψ` is rebuilt until no middle bows, in at most [`PSI_MAX_DEPTH`] rounds.
    let Some(mut psi) = hermite_psi(&points, [lo, hi]) else {
        return Ok(None);
    };
    // |C(ψ(s)) - S(W(s))| in double arithmetic (only steering), with `ψ(s)` and the image point
    // (`ψ(s)` by `psi_at`, not enclosed: a reparametrisation's checks are a thousand of these
    // per trim, and only which points it gets, never whether its bound holds, depends on them)
    let off = |psi: &NurbsCurve, si: f64| -> Result<Option<(f64, f64, crate::math::Point3)>> {
        let Some(at) = psi_at(psi, si, [lo, hi]) else {
            return Ok(None);
        };
        let (Some(p), Some(c)) = (image(si)?, ignored(curve.evaluate_point(k, at))?) else {
            return Ok(None);
        };
        Ok(Some((c.distance(p), at, p)))
    };
    // (each gap's piece of `ψ` depends only on a few points around it, so a round checks only
    // the gaps a point added in the round before can have changed (`dirty`): the others have
    // the same piece, bit for bit, and did not bow; and a point's own distance, which two
    // gaps share, is found once a round (`ends`))
    let mut dirty = vec![true; points.len().saturating_sub(1)];
    for _ in 0..PSI_MAX_DEPTH {
        let mut extra: Vec<(f64, f64)> = Vec::new();
        let mut ends: Vec<Option<f64>> = vec![None; points.len()];
        let mut end_off = |psi: &NurbsCurve, i: usize, s: f64| -> Result<Option<f64>> {
            if let Some(&Some(d)) = ends.get(i) {
                return Ok(Some(d));
            }
            let d = off(psi, s)?.map(|x| x.0);
            if let (Some(slot), Some(d)) = (ends.get_mut(i), d) {
                *slot = Some(d);
            }
            Ok(d)
        };
        for (i, pair) in points.windows(2).enumerate() {
            let [a, b] = pair else {
                return Ok(None);
            };
            let m = 0.5 * (a.0 + b.0);
            if !(b.0 - a.0 >= 2.0 * min_gap && a.0 < m && m < b.0) {
                continue;
            }
            if count >= PSI_MAX_POINTS {
                break;
            }
            if dirty.get(i) != Some(&true) {
                continue;
            }
            k.check_cancelled(OP)?;
            let (Some(da), Some(db), Some((dm, at, p))) = (
                end_off(&psi, i, a.0)?,
                end_off(&psi, i + 1, b.0)?,
                off(&psi, m)?,
            ) else {
                return Ok(None);
            };
            // (refined only where the middle is farther than both ends by `refine`; a NaN
            // anywhere is no reason to)
            if dm.partial_cmp(&(da.max(db) + refine)) != Some(std::cmp::Ordering::Greater) {
                continue;
            }
            let (tm, _) = nearest_param(k, curve, p, at, [lo, hi])?;
            count += 1;
            extra.push((m, tm));
        }
        if extra.is_empty() {
            break;
        }
        // (points added, flagged, merged in order: each extra point lies inside its own gap)
        let mut merged: Vec<((f64, f64), bool)> = points.iter().map(|&x| (x, false)).collect();
        merged.extend(extra.into_iter().map(|x| (x, true)));
        merged.sort_by(|x, y| x.0 .0.total_cmp(&y.0 .0));
        points = merged.iter().map(|x| x.0).collect();
        // gap `i` (points `i`, `i + 1`) is checked again where a point from `i - 3` to `i + 4`
        // is new: its piece's control values come from the pieces either side of it, whose
        // slopes reach one point further (`i - 2` to `i + 3`), and an end's distance may be
        // taken on the piece before it
        dirty = (0..points.len().saturating_sub(1))
            .map(|i| {
                merged
                    .get(i.saturating_sub(3)..(i + 5).min(merged.len()))
                    .is_some_and(|w| w.iter().any(|x| x.1))
            })
            .collect();
        let Some(rebuilt) = hermite_psi(&points, [lo, hi]) else {
            return Ok(None);
        };
        psi = rebuilt;
    }
    Ok(Some(psi))
}

/// The C^1 piecewise cubic through `points` (`(s_i, t_i)`, `s` increasing from `lo` to `hi`)
/// with Fritsch-Butland slopes (no overshoot where the `t_i` are monotone), as a non-rational
/// B-spline with double interior knots: its control values are the inner Bezier points of each
/// piece, clamped into `[lo, hi]`, its first `lo` and its last `hi`. The double knots make the
/// B-spline C^1 whatever its control values are; the values only decide how close it stays to
/// the points. `None` for fewer than two points or `s` not increasing.
fn hermite_psi(points: &[(f64, f64)], [lo, hi]: [f64; 2]) -> Option<NurbsCurve> {
    let pieces = points.len().saturating_sub(1);
    if pieces == 0 || points.windows(2).any(|w| !matches!(w, [a, b] if a.0 < b.0)) {
        return None;
    }
    let slope = |i: usize| -> f64 {
        match (points.get(i), points.get(i + 1)) {
            (Some(a), Some(b)) => (b.1 - a.1) / (b.0 - a.0),
            _ => 0.0,
        }
    };
    let width = |i: usize| -> f64 {
        match (points.get(i), points.get(i + 1)) {
            (Some(a), Some(b)) => b.0 - a.0,
            _ => 0.0,
        }
    };
    let tangent = |i: usize| -> f64 {
        if i == 0 {
            return slope(0);
        }
        if i == pieces {
            return slope(pieces - 1);
        }
        let (d0, d1) = (slope(i - 1), slope(i));
        if d0 * d1 <= 0.0 {
            return 0.0;
        }
        let (h0, h1) = (width(i - 1), width(i));
        let (w1, w2) = (2.0 * h1 + h0, h1 + 2.0 * h0);
        (w1 + w2) / (w1 / d0 + w2 / d1)
    };
    let mut u = vec![lo; 4];
    let mut pw = vec![lo, 0.0, 0.0, 1.0];
    for i in 0..pieces {
        let (Some(a), Some(b)) = (points.get(i), points.get(i + 1)) else {
            return None;
        };
        let h = b.0 - a.0;
        let inner = [a.1 + h * tangent(i) / 3.0, b.1 - h * tangent(i + 1) / 3.0];
        for x in inner {
            if !x.is_finite() {
                return None;
            }
            pw.extend([x.max(lo).min(hi), 0.0, 0.0, 1.0]);
        }
        if i + 1 < pieces {
            u.extend([b.0, b.0]);
        }
    }
    pw.extend([hi, 0.0, 0.0, 1.0]);
    u.extend([hi; 4]);
    let n = pw.len() / 4;
    if u.len() != n + 4 {
        return None;
    }
    Some(NurbsCurve { p: 3, n, u, pw })
}

/// `ψ(s)` of a reparametrisation [`hermite_psi`] built, in double arithmetic (de Boor's
/// algorithm on its non-rational control values), clamped to `[lo, hi]`: the steering value of
/// [`reparametrisation`]'s checks, not an enclosure. `None` outside `ψ`'s domain or where the
/// value is not finite.
fn psi_at(psi: &NurbsCurve, s: f64, [lo, hi]: [f64; 2]) -> Option<f64> {
    let p = psi.p;
    let (&d0, &d1) = (psi.u.get(p)?, psi.u.get(psi.n)?);
    if !(d0 <= s && s <= d1) {
        return None;
    }
    // the span `[u_k, u_k+1)` holding `s` (the last non-empty one at the domain's end)
    let span = psi.u.get(p..psi.n)?;
    let k = p + span.partition_point(|&x| x <= s).saturating_sub(1);
    let mut d: Vec<f64> = (0..=p)
        .map(|j| psi.pw.get(4 * (k + j - p)).copied())
        .collect::<Option<_>>()?;
    for r in 1..=p {
        for j in (r..=p).rev() {
            let i = k + j - p;
            let (&ui, &uip) = (psi.u.get(i)?, psi.u.get(i + p + 1 - r)?);
            let den = uip - ui;
            let a = if den > 0.0 { (s - ui) / den } else { 0.0 };
            let (&prev, &cur) = (d.get(j - 1)?, d.get(j)?);
            *d.get_mut(j)? = (1.0 - a) * prev + a * cur;
        }
    }
    let at = *d.get(p)?;
    at.is_finite().then(|| at.max(lo).min(hi))
}

/// A proved upper bound on the distance between the curve `curve` (`C`, an edge's curve) over
/// its parameters `range` (`[lo, hi]`) and the image `S(W)` of the UV trim `uv` (`W`, on the
/// same parameters) on `surface` (`S`), in both directions: every point of the image is within
/// the bound of the curve and every point of the curve within it of the image (their Hausdorff
/// distance), so within it of the surface: what an edge's tolerance must cover for that trim.
/// `floor` is the caller's scale (below it a tighter bound changes nothing, as for
/// [`curve_face_gap_bound`]).
///
/// The two curves are compared through a reparametrisation `ψ` of `[lo, hi]` onto itself
/// ([`reparametrisation`]), continuous with `ψ(lo) = lo` and `ψ(hi) = hi`: for each `s`,
/// `dist(S(W(s)), C) <= |C(ψ(s)) - S(W(s))|`, and since `ψ` takes every value of `[lo, hi]`,
/// every `C(t)` is `C(ψ(s))` for some `s`, within the same of the image. The same-parameter
/// gap `|C(t) - S(W(t))|` would bound it too, but it also counts how far the two parameters
/// slip along the curve, which follows the fit's tolerance rather than the geometry.
/// `max |C(ψ) - S(W)|` is enclosed as [`certified_gap`] encloses its gap, with the chain rule
/// for `C o ψ`: the surface's exact NURBS net (an analytic surface's, or the B-spline's own),
/// the curve's, the trim's and `ψ`'s NURBS data, every coefficient in rounded interval
/// arithmetic, plus the rounding of a line's analytic evaluation ([`exact_curve_data`]).
///
/// `None` when the bound is not enclosed: a surface without an exact net
/// (`exact_nurbs_surface`: a procedural blend or sweep, an offset, a step surface ...) or whose
/// net is not parameterised as the surface, a curve or trim of another kind than a B-spline,
/// an ellipse or a line, an empty or unordered `range`
/// or one past either curve's domain, a piece of the trim whose box runs past the net, or a
/// piece the bisection cannot bound.
///
/// # Errors
///
/// The kernel's unported and cancelled statuses.
pub fn trim_image_gap_bound(
    k: &Kernel,
    surface: &Surface,
    curve: &Curve,
    uv: &Curve,
    range: [f64; 2],
    floor: f64,
) -> Result<Option<f64>> {
    trim_image_gap_bound_up_to(k, surface, curve, uv, range, floor, f64::INFINITY)
}

/// [`trim_image_gap_bound`] for a caller that only asks whether the bound is above `over`: the
/// same bound where it is at most `over`, and `Some(f64::INFINITY)` as soon as a piece shows it
/// is above it, without bounding the rest. Every piece before that one is cut exactly as in
/// [`trim_image_gap_bound`], so the answer to that question is the same.
///
/// # Errors
///
/// The kernel's unported and cancelled statuses.
pub fn trim_image_gap_bound_up_to(
    k: &Kernel,
    surface: &Surface,
    curve: &Curve,
    uv: &Curve,
    range: [f64; 2],
    floor: f64,
    over: f64,
) -> Result<Option<f64>> {
    trim_image_gap_bound_within(
        k,
        surface,
        curve,
        uv,
        range,
        floor,
        [f64::NEG_INFINITY, over],
        None,
    )
}

/// [`trim_image_gap_bound_up_to`] for a caller that also takes any bound at most `enough` as
/// it comes: a piece whose bound (with the curve's slack) is at most `enough` is not cut
/// further. The bound is still a proved bound, but where it is at most `enough` it may be any
/// value up to `enough` rather than the tightest the bisection reaches; whether it is at most
/// `enough` is answered as by [`trim_image_gap_bound_up_to`] wherever that one stays within its
/// piece budget (pruned pieces leave the budget to the others, which can only tighten it).
/// An `enough` of `f64::NEG_INFINITY` is [`trim_image_gap_bound_up_to`].
///
/// `budget` caps the bisection's pieces as for [`bisect`]: `None` is the whole budget, with a
/// piece still open at its end bounded as it stands; with a cap, which holds only while the
/// bound may still be at most `enough`, a gap the cap stops is not enclosed, and pieces within
/// the largest end value close as they stand.
///
/// # Errors
///
/// The kernel's unported and cancelled statuses.
pub fn trim_image_gap_bound_within(
    k: &Kernel,
    surface: &Surface,
    curve: &Curve,
    uv: &Curve,
    range: [f64; 2],
    floor: f64,
    [enough, over]: [f64; 2],
    budget: Option<usize>,
) -> Result<Option<f64>> {
    use crate::error::ignored;
    use crate::surface::surface::exact_pieces::exact_nurbs_surface;

    let [lo, hi] = range;
    if !(lo.is_finite() && hi.is_finite() && lo < hi && floor.is_finite() && floor >= 0.0) {
        return Ok(None);
    }
    k.check_cancelled(OP)?;
    if exact_nurbs_surface(k, surface)?.is_none() {
        return Ok(None);
    }
    let (Some((cn, c_slack)), Some((wn, w_slack))) =
        (exact_curve_data(curve, false)?, exact_curve_data(uv, true)?)
    else {
        return Ok(None);
    };
    // A trim's own rounding (a line's) moves its parameters, not a point: no bound follows from
    // it, so only an exactly evaluated trim is enclosed.
    if w_slack != 0.0 {
        return Ok(None);
    }
    let within = |n: &NurbsCurve| match (n.u.get(n.p), n.u.get(n.n)) {
        (Some(&d0), Some(&d1)) => d0 <= lo && hi <= d1,
        _ => false,
    };
    if !(within(&cn) && within(&wn)) {
        return Ok(None);
    }
    let mut probes = Vec::new();
    for i in 0..TRIM_PROBES {
        let t = if i + 1 == TRIM_PROBES {
            hi
        } else {
            lo + (hi - lo) * (i as f64) / ((TRIM_PROBES - 1) as f64)
        };
        let Some(p) = ignored(uv.evaluate_point(k, t))? else {
            return Ok(None);
        };
        probes.push(Point2::new(p.x, p.y));
    }
    let Some(net) = NetSurface::of(k, surface, &probes)? else {
        return Ok(None);
    };
    let knots: Vec<f64> = cn.u.iter().chain(wn.u.iter()).copied().collect();
    let Some(psi) = reparametrisation(k, surface, curve, uv, range, &knots, floor)? else {
        return Ok(None);
    };
    let [r0, r1] = range;
    if !kinks(&psi, r0, r1).is_empty() {
        return Ok(None);
    }
    let curve_kinks = kinks(&cn, r0, r1);
    let trim_kinks = kinks(&wn, r0, r1);
    // (the bound of the curve data `cn` against the net `net` through the reparametrisation
    // `psi`, the curve's slack added, with the pieces closing at `enough`)
    let run = |psi: &NurbsCurve,
               cn: &NurbsCurve,
               net: &NetSurface,
               enough: f64,
               cuts: &[f64]|
     -> Result<Option<f64>> {
        let sides = Sides {
            curve: cn,
            trim: &wn,
            psi,
            curve_kinks: curve_kinks.clone(),
            trim_kinks: trim_kinks.clone(),
            range,
        };
        // (the bound adds the curve's slack, at least 0, to the gap: a gap above `over` is
        // enough; a piece across a corner is bounded to first order by
        // `piece_reparametrised`, so the first pieces need no cut there)
        // (a gap at most `enough` less the curve's slack, rounded down, has a bound at most
        // `enough`: the sum's exact value is at most `enough`, so rounding it up stays there)
        let enough_gap = (Ivl::from(enough) - Ivl::from(c_slack)).lo;
        let Some(gap) = bisect(
            range,
            FIRST_PIECES,
            cuts,
            CLOSE_RELATIVE,
            floor,
            [enough_gap, over],
            budget,
            |a, b| {
                k.check_cancelled(OP)?;
                piece_reparametrised(&sides, net, a, b)
            },
        )?
        else {
            return Ok(None);
        };
        if gap == f64::INFINITY {
            return Ok(Some(gap));
        }
        let bound = (Ivl::from(gap) + Ivl::from(c_slack)).hi;
        Ok(bound.is_finite().then_some(bound))
    };
    // Where the bound is above `enough` (or not enclosed), the first pieces are cut again at
    // every interior knot of `ψ` and the smaller bound kept: `ψ` is a C^1 cubic spline whose
    // second derivative jumps at its knots, and where its refinement chased a steep step in the
    // correspondence down to spans far narrower than the bisection's deepest piece, a piece
    // across those spans has a second-order term as large as the steepest span's, which no
    // halving shrinks (#18166: `topology.brep_local_operation` fc0000, bounds 0.43 and 8.7e-3
    // from one piece at depth 30, for trims 1.2e-6 and 1.2e-7 from their edges by dense
    // samples). Pieces within one span of `ψ` each have a cubic's bounded second derivative.
    // (The cuts are `ψ`'s distinct interior knots: one per interpolated point, so at most
    // [`PSI_MAX_POINTS`]. The first pieces are then at most [`FIRST_PIECES`] plus the cuts,
    // and no piece is cut further past [`MAX_PIECES`]: the work stays bounded.)
    let with_psi = |psi: &NurbsCurve| -> Result<Option<f64>> {
        let mut first = run(psi, &cn, &net, enough, &[])?;
        let mut cuts: Vec<f64> = Vec::new();
        if !first.is_some_and(|g| g <= enough) {
            cuts = psi
                .u
                .iter()
                .copied()
                .filter(|&x| r0 < x && x < r1)
                .collect();
            cuts.dedup();
            if !cuts.is_empty() {
                if let Some(g) = run(psi, &cn, &net, enough, &cuts)? {
                    first = Some(first.map_or(g, |b| b.min(g)));
                }
            }
        }
        recentred_retry(first, [enough, over], &cn, &net, |cn, net, enough| {
            run(psi, cn, net, enough, &cuts)
        })
    };
    let mut bound = with_psi(&psi)?;
    // Any reparametrisation gives a bound. A line's against a trim of degree 1 corresponds
    // by their common parameter as it stands (on a plane, the image is the line itself), and
    // the fitted `psi` only adds its own steps, each as long as the curve's speed makes it:
    // `primitive.api_primitives` gen1108, a line 5.5e8 long bounded at 1.4e-3 and 1e4 against
    // its trims, 1e-7 by samples. The identity is tried there too, the smaller bound kept.
    if cn.p == 1 && wn.p == 1 && !bound.is_some_and(|g| g <= enough) {
        if let Some(identity) = hermite_psi(&[(r0, r0), (r1, r1)], range) {
            if let Some(g) = with_psi(&identity)? {
                bound = Some(bound.map_or(g, |b| b.min(g)));
            }
        }
    }
    let Some(bound) = bound else {
        return Ok(None);
    };
    // (the cutoff applies to the bound returned, the curve's slack included: a gap at most
    // `over` whose slack takes it past `over` is above it too)
    Ok(Some(if bound > over { f64::INFINITY } else { bound }))
}

/// Not upstream (#18166): the bound `first` of a curve against a surface (`None` where it is
/// not enclosed), tightened where it is above `enough` (or not enclosed) and the curve lies
/// far from the origin for its size ([`recentre`]): `run` bounds the same pair with both moved
/// by `-T`, `T` the middle of the curve's control points, and the smaller bound is kept.
///
/// Far from the origin the interval enclosures widen with the coordinates themselves, not
/// with the geometry: a rational curve's derivative `(A' - w' C) / w` encloses `A'` and
/// `w' C` apart, each as wide as the coordinate `C` is large, so the second-order term of a
/// piece stops shrinking with it, and the bisection runs out of pieces with the bound held
/// there (`primitive.api_primitives` gen0353: an ellipse at `z = 4.5e8`, 1.2e-7 from its
/// faces by samples, bounded at 4.76e4; gen0748: lines at 6e9, 9.5e-7 by samples, bounded
/// at 1.2e-4). A translation keeps every distance, so it only moves the coordinates the
/// enclosures widen with.
///
/// The moved data are rounded ([`translated_pw`]): each control point lands within its
/// slack of the exact moved point, so each moved NURBS (positive weights: its points are
/// convex combinations of the control points) is within its largest slack of the exact
/// moved curve or surface, and the bound of the moved pair plus both slacks bounds the
/// distance of the given pair. `run` is given `enough` less those slacks; a moved bound that
/// is not finite, or past `over`, keeps `first`.
///
/// # Errors
///
/// Those of `run`.
fn recentred_retry(
    first: Option<f64>,
    [enough, over]: [f64; 2],
    cn: &NurbsCurve,
    net: &NetSurface,
    run: impl FnOnce(&NurbsCurve, &NetSurface, f64) -> Result<Option<f64>>,
) -> Result<Option<f64>> {
    if first.is_some_and(|g| g <= enough) {
        return Ok(first);
    }
    let Some(t) = recentre(cn) else {
        return Ok(first);
    };
    let (Some((moved_curve, curve_slack)), Some((moved_net, net_slack))) =
        (translated_curve(cn, t), net.translated(t))
    else {
        return Ok(first);
    };
    let slack = (Ivl::from(curve_slack) + Ivl::from(net_slack)).hi;
    let moved_enough = (Ivl::from(enough) - Ivl::from(slack)).lo;
    let Some(moved) = run(&moved_curve, &moved_net, moved_enough)? else {
        return Ok(first);
    };
    let moved = (Ivl::from(moved) + Ivl::from(slack)).hi;
    if !(moved.is_finite() && moved <= over) {
        return Ok(first);
    }
    Ok(Some(first.map_or(moved, |g| g.min(moved))))
}

/// The middle `T` of the box of `cn`'s control points, where some coordinate of it is larger
/// than the box's diagonal (the curve lies farther from the origin than its size, so its
/// coordinates, not its shape, set the enclosures' width: [`recentred_retry`]); `None` near
/// the origin, or for data that are not finite or a weight that is not positive.
fn recentre(cn: &NurbsCurve) -> Option<[f64; 3]> {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in cn.pw.chunks_exact(4) {
        let [x, y, z, w] = [*p.first()?, *p.get(1)?, *p.get(2)?, *p.get(3)?];
        if !(w.is_finite() && w > 0.0) {
            return None;
        }
        for (i, c) in [x, y, z].into_iter().enumerate() {
            let c = c / w;
            if !c.is_finite() {
                return None;
            }
            let (l, h) = (lo.get_mut(i)?, hi.get_mut(i)?);
            *l = l.min(c);
            *h = h.max(c);
        }
    }
    let [l0, l1, l2] = lo;
    let [h0, h1, h2] = hi;
    let t = [0.5 * (l0 + h0), 0.5 * (l1 + h1), 0.5 * (l2 + h2)];
    let size = ((h0 - l0).powi(2) + (h1 - l1).powi(2) + (h2 - l2).powi(2)).sqrt();
    let far = t.iter().fold(0.0_f64, |m, c| m.max(c.abs()));
    (far.is_finite() && size.is_finite() && far > size).then_some(t)
}

/// The homogeneous control points `pw` (`(x w, y w, z w, w)` each) moved by `-t`, rounded,
/// with a slack no moved point is farther than from the exact moved point `x - t`.
///
/// Each moved coordinate is `X' = fl(X - fl(w t))`, so `|X' - (X - w t)| <= u |w t| +
/// u |X'| / (1 - u)` (`u` the unit roundoff), and the moved point's coordinate `X' / w` is
/// within `u (|t| + |X'| / w / (1 - u))` of `x - t`: bounded by `(|t| + fl(|X'| / w)) * 2u`
/// (twice `u` covers the quotient's rounding and the `1 / (1 - u)`), the three coordinates'
/// norm rounded up. `None` for data that are not finite or a weight that is not positive.
fn translated_pw(pw: &[f64], t: [f64; 3]) -> Option<(Vec<f64>, f64)> {
    let mut out = Vec::new();
    let mut slack = 0.0_f64;
    for p in pw.chunks_exact(4) {
        let [x, y, z, w] = [*p.first()?, *p.get(1)?, *p.get(2)?, *p.get(3)?];
        if !(w.is_finite() && w > 0.0) {
            return None;
        }
        let mut sq = Ivl::from(0.0);
        for (c, tc) in [x, y, z].into_iter().zip(t) {
            let moved = c - w * tc;
            if !moved.is_finite() {
                return None;
            }
            out.push(moved);
            let e = (Ivl::from(tc.abs()) + Ivl::from(moved.abs()) / Ivl::from(w))
                * Ivl::from(f64::EPSILON);
            sq = sq + e * e;
        }
        out.push(w);
        slack = slack.max(sq.sqrt().hi);
    }
    (out.len() == pw.len() && slack.is_finite()).then_some((out, slack))
}

/// `cn` moved by `-t` ([`translated_pw`]), with its slack.
fn translated_curve(cn: &NurbsCurve, t: [f64; 3]) -> Option<(NurbsCurve, f64)> {
    let (pw, slack) = translated_pw(&cn.pw, t)?;
    Some((
        NurbsCurve {
            p: cn.p,
            n: cn.n,
            u: cn.u.clone(),
            pw,
        },
        slack,
    ))
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

    /// A curve bowing within a plane parallel to a box's face is bounded against the face
    /// by its height to within a thousandth, whatever the bow, by the bound that drops every
    /// piece's ends afresh (#18166); one asked whether it is within a hundredth past the
    /// height is shown within it, and a bound past `over` is infinite.
    #[test]
    fn a_curve_bowing_over_a_face_is_bounded_by_its_height_as_the_pieces_shrink() {
        let mut k = Kernel::default();
        let b = crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 1.0, 2.0, 3.0)
            .unwrap();
        let lift = 6.8e-5;
        let z = 3.0 + lift;
        let bow = NurbCurve::new(
            vec![
                CPoint::new(0.2, 0.3, z, NOW),
                CPoint::new(0.9, 0.5, z, NOW),
                CPoint::new(0.3, 1.7, z, NOW),
            ],
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        )
        .unwrap();
        let bow = BSplineCurve::from_nurb(bow, 3, false);
        let faces = crate::query::api_queries::get_faces(&b).unwrap();
        let run = |f, within: [f64; 2]| {
            curve_face_gap_bound_within(&k, &b, f, &bow, [0.0, 1.0], 1e-9, within).unwrap()
        };
        let tight: Vec<f64> = faces
            .iter()
            .filter_map(|&f| run(f, [f64::NEG_INFINITY, f64::INFINITY]))
            .collect();
        let nearest = tight.iter().copied().fold(f64::INFINITY, f64::min);
        assert!(
            nearest >= lift && nearest <= lift * (1.0 + 1e-3),
            "{nearest:e} against {lift:e}: {tight:?}"
        );
        let top = faces
            .iter()
            .copied()
            .find(|&f| run(f, [f64::NEG_INFINITY, f64::INFINITY]) == Some(nearest))
            .unwrap();
        let within = run(top, [1.01 * lift, f64::INFINITY]);
        assert!(
            within.is_some_and(|g| g >= lift && g <= 1.01 * lift),
            "{within:?}"
        );
        assert_eq!(run(top, [0.5 * lift, 0.5 * lift]), Some(f64::INFINITY));
        // an empty range encloses nothing
        assert!(curve_face_gap_bound_within(
            &k,
            &b,
            top,
            &bow,
            [0.5, 0.5],
            1e-9,
            [f64::NEG_INFINITY, f64::INFINITY]
        )
        .unwrap()
        .is_none());
    }

    /// The top face of a box (its plane `surface`, its UV domain mapped to `[0, 1]^2` by
    /// `uv`), its unit normal, and a curve of `cps` control points (degree `p`, knots
    /// `knots`, each point `at(f)` of fractions `f` of the domain, lifted `lift` along the
    /// normal) as a B-spline of dimension `dim`.
    struct Top {
        k: Kernel,
        surface: Surface,
        lo: Point2,
        size: Point2,
        normal: Point3,
    }

    impl Top {
        fn new() -> Self {
            let mut k = Kernel::default();
            let b =
                crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 1.0, 2.0, 3.0)
                    .unwrap();
            for f in crate::query::api_queries::get_faces(&b).unwrap() {
                let rec = b.face(f).unwrap();
                let surface = rec.surface.clone().unwrap();
                let d = rec.uv_domain;
                let lo = Point2::new(d.min().x, d.min().y);
                let size = Point2::new(d.max().x - lo.x, d.max().y - lo.y);
                let at = |x: f64, y: f64| {
                    surface
                        .evaluate_point(&k, Point2::new(lo.x + x * size.x, lo.y + y * size.y))
                        .unwrap()
                };
                let (o, a, c) = (at(0.0, 0.0), at(1.0, 0.0), at(0.0, 1.0));
                if (o.z - 3.0).abs() > 1e-12
                    || (a.z - 3.0).abs() > 1e-12
                    || (c.z - 3.0).abs() > 1e-12
                {
                    continue;
                }
                let n = (a - o).cross(c - o);
                let normal = n * (1.0 / n.length());
                return Self {
                    k,
                    surface,
                    lo,
                    size,
                    normal,
                };
            }
            panic!("no top face");
        }

        fn uv(&self, f: [f64; 2]) -> Point3 {
            Point3::new(
                self.lo.x + f[0] * self.size.x,
                self.lo.y + f[1] * self.size.y,
                0.0,
            )
        }

        fn point(&self, f: [f64; 2], lift: f64) -> Point3 {
            let q = self.uv(f);
            self.surface
                .evaluate_point(&self.k, Point2::new(q.x, q.y))
                .unwrap()
                + self.normal * lift
        }

        fn curve(points: &[Point3], p: usize, knots: Vec<f64>, two_d: bool) -> Curve {
            let cps = points
                .iter()
                .map(|q| CPoint::new(q.x, q.y, q.z, NOW))
                .collect();
            let c = NurbCurve::new(cps, p, knots).unwrap();
            let mut c = BSplineCurve::from_nurb(c, 3, false);
            if two_d {
                c.convert_to_2d().unwrap();
            }
            Curve::from(c)
        }

        fn bound(&self, curve: &Curve, trim: &Curve) -> Option<f64> {
            trim_image_gap_bound(&self.k, &self.surface, curve, trim, [0.0, 1.0], 1e-9).unwrap()
        }
    }

    /// A trim of the edge curve lifted `lift` off the plane, the curve parameterised at another
    /// speed than the trim (a quadratic with its middle point off centre along the line): the
    /// same-parameter gap counts that slip (here about a tenth of the line), but the trim's image
    /// lies `lift` from the curve, so the bound is at least `lift` (it is an upper bound of a
    /// distance at least `lift` at the ends) and within twice the bisection's closing fraction
    /// (`CLOSE_RELATIVE`) above it.
    #[test]
    fn a_trim_is_bounded_by_its_distance_not_its_parameter_slip() {
        let t = Top::new();
        let lift = 3.0e-4;
        let (a, b) = ([0.2, 0.3], [0.7, 0.6]);
        let mid = [0.2 + 0.7 * 0.5, 0.3 + 0.7 * 0.3];
        let trim = Top::curve(&[t.uv(a), t.uv(b)], 1, vec![0.0, 0.0, 1.0, 1.0], true);
        let curve = Top::curve(
            &[t.point(a, lift), t.point(mid, lift), t.point(b, lift)],
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            false,
        );
        let slip = (t.point([0.45, 0.45], 0.0) - curve.evaluate_point(&t.k, 0.5).unwrap()).length();
        assert!(slip > 100.0 * lift, "{slip:e}");
        let g = t.bound(&curve, &trim).unwrap();
        assert!(
            g >= lift && g <= lift * (1.0 + 2.0 * CLOSE_RELATIVE),
            "{g:e} against {lift:e}"
        );
    }

    /// A trim bowed off its curve (a quadratic whose middle point is pushed across the line in
    /// UV): the bound must cover the image's real distance from the curve, sampled here (a
    /// lower bound of the true maximum), so a trim off its edge is never certified close.
    #[test]
    fn a_trim_off_its_curve_is_not_bounded_close() {
        let t = Top::new();
        let (a, b) = ([0.2, 0.3], [0.7, 0.6]);
        let bow = [0.45 - 0.03, 0.45 + 0.05];
        let trim = Top::curve(
            &[t.uv(a), t.uv(bow), t.uv(b)],
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            true,
        );
        let curve = Top::curve(
            &[t.point(a, 0.0), t.point(b, 0.0)],
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            false,
        );
        let (pa, pb) = (t.point(a, 0.0), t.point(b, 0.0));
        let dir = (pb - pa) * (1.0 / (pb - pa).length());
        let mut seen = 0.0_f64;
        for i in 0..=1000 {
            let w = trim.evaluate_point(&t.k, f64::from(i) / 1000.0).unwrap();
            let q = t
                .surface
                .evaluate_point(&t.k, Point2::new(w.x, w.y))
                .unwrap();
            let off = q - pa;
            seen = seen.max((off - dir * off.dot(dir)).length());
        }
        assert!(seen > 1e-2, "{seen:e}");
        let g = t.bound(&curve, &trim).unwrap();
        assert!(g >= seen && g <= 2.0 * seen, "{g:e} against {seen:e}");
    }

    /// A trim that first steps back by a nanometre over a sub-micron knot span, then runs along
    /// its curve, `lift` off it (#18166, fillet1019's face 0): the nearest parameters on that
    /// span all clamp to the start, so the cubic `ψ` has a zero slope at the span's end, and the
    /// gap after it, whose middle sits on the chord of its ends, lags the image by up to 4/27 of
    /// its width along the curve (some 2e-4 here). `ψ` is checked against the image at each
    /// gap's middle, so the bound stays within the bisection's closing fraction of `lift`.
    #[test]
    fn a_trim_that_turns_back_over_a_tiny_span_is_bounded_by_its_distance() {
        let t = Top::new();
        let lift = 1.0e-4;
        let (a, b) = ([0.2, 0.3], [0.7, 0.6]);
        let eps = 5.0e-7;
        let back = [a[0] - 1.0e-9 * 0.5, a[1] - 1.0e-9 * 0.3];
        let trim = Top::curve(
            &[t.uv(a), t.uv(back), t.uv(b)],
            1,
            vec![0.0, 0.0, eps, 1.0, 1.0],
            true,
        );
        let curve = Top::curve(
            &[t.point(a, lift), t.point(b, lift)],
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            false,
        );
        let g = t.bound(&curve, &trim).unwrap();
        assert!(
            g >= lift && g <= lift * (1.0 + 2.0 * CLOSE_RELATIVE) + 1.0e-8,
            "{g:e} against {lift:e}"
        );
    }

    /// A trim `lift` off its curve whose image stops `short` of the curve's end along it: near
    /// the end the image's nearest curve point is short of the end, while `ψ(hi) = hi` holds
    /// `ψ` to it. The two curves are within `sqrt(lift^2 + short^2)` of each other (the image's
    /// end against the curve's), so the bound is at least that and within the bisection's
    /// closing fraction of it: `ψ` is refined only where it bows `C(ψ)` off the image, not
    /// toward the pinned end, which would make it ever steeper there.
    #[test]
    fn a_trim_that_stops_short_of_its_curve_end_is_bounded_by_its_distance() {
        let t = Top::new();
        let (lift, short) = (1.0e-4, 1.0e-5);
        let (a, b) = ([0.2, 0.3], [0.7, 0.6]);
        let (pa, pb) = (t.point(a, 0.0), t.point(b, 0.0));
        let f = 1.0 - short / (pb - pa).length();
        let end = [a[0] + f * (b[0] - a[0]), a[1] + f * (b[1] - a[1])];
        let trim = Top::curve(&[t.uv(a), t.uv(end)], 1, vec![0.0, 0.0, 1.0, 1.0], true);
        let curve = Top::curve(
            &[t.point(a, lift), t.point(b, lift)],
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            false,
        );
        let d = (lift * lift + short * short).sqrt();
        let g = t.bound(&curve, &trim).unwrap();
        // (the curve's end is `d` from the image, which `d` holds up to the rounding of the
        // construction, well inside a part in 1e9; a bound missing the shortfall along the
        // curve would sit near `lift`, half a per cent below)
        assert!(
            g >= d * (1.0 - 1e-9) && g <= d * (1.0 + 2.0 * CLOSE_RELATIVE) + 1.0e-8,
            "{g:e} against {d:e}"
        );
    }

    /// A trim `h` off its curve where the curve starts with a short drop from near the trim's
    /// image to `h` below it: the image's nearest curve point jumps from the drop's top to the
    /// long stretch below, so the nearest parameters step there, however finely the trim is
    /// cut. `ψ` is not halved past what the bisection can split, so the bound stays near the
    /// distance between the two curves (at least `h`, at the ends) and is not held up by a step
    /// of `ψ` a few units in the last place wide (#18166, `single_face_fillet1019_8`).
    #[test]
    fn a_trim_whose_nearest_curve_point_jumps_is_bounded_by_its_distance() {
        let t = Top::new();
        let h = 2.0e-4;
        let (x0, x1, y) = (0.2, 0.7, 0.3);
        let trim = Top::curve(
            &[t.uv([x0, y + h]), t.uv([x1, y + h])],
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            true,
        );
        let curve = Top::curve(
            &[
                t.point([x0, y + 0.9 * h], 0.0),
                t.point([x0 + 1.0e-6, y], 0.0),
                t.point([x1, y], 0.0),
            ],
            1,
            vec![0.0, 0.0, 1.0e-3, 1.0, 1.0],
            false,
        );
        let d = t.point([x1, y + h], 0.0).distance(t.point([x1, y], 0.0));
        let g = t.bound(&curve, &trim).unwrap();
        assert!(
            g >= d * (1.0 - 1.0e-9) && g <= 2.0 * d,
            "{g:e} against {d:e}"
        );
    }

    /// The bisection of `[0, 1]` (32 first pieces, floor `floor`) of pieces whose end values
    /// are `ends`, whose second term is `0.1 h²` and whose slack is `slack(a, h)`.
    fn bisect_with_slack(ends: f64, floor: f64, slack: impl Fn(f64, f64) -> f64) -> f64 {
        bisect(
            [0.0, 1.0],
            32,
            &[],
            CLOSE_RELATIVE,
            floor,
            [f64::NEG_INFINITY, f64::INFINITY],
            None,
            |a, b| {
                let h = b - a;
                Ok(PieceBound::with(ends, Ivl::from(0.1 * h * h), slack(a, h)))
            },
        )
        .unwrap()
        .unwrap()
    }

    /// `psi_at` is `ψ` where its enclosure is: inside [`psi_over`]'s enclosure of a point (to
    /// a few units in the last place), at the points it passes through and between them.
    #[test]
    fn psi_at_is_within_psi_over() {
        let points = [
            (0.0, 0.0),
            (0.1, 0.13),
            (0.25, 0.24),
            (0.25 + 1e-9, 0.2400001),
            (0.6, 0.55),
            (0.61, 0.62),
            (1.0, 1.0),
        ];
        let psi = hermite_psi(&points, [0.0, 1.0]).unwrap();
        for &(s, t) in &points {
            let at = psi_at(&psi, s, [0.0, 1.0]).unwrap();
            assert!(
                (at - t).abs() <= 4.0 * f64::EPSILON,
                "{s}: {at} against {t}"
            );
        }
        for i in 0..=1000 {
            let s = f64::from(i) / 1000.0;
            let at = psi_at(&psi, s, [0.0, 1.0]).unwrap();
            let r = psi_over(&psi, Ivl::from(s), 0, [0.0, 1.0])
                .unwrap()
                .unwrap()[0];
            let pad = 4.0 * f64::EPSILON;
            assert!(
                r.lo - pad <= at && at <= r.hi + pad,
                "{s}: {at} not in {r:?}"
            );
        }
        assert_eq!(psi_at(&psi, -1e-9, [0.0, 1.0]), None);
        assert_eq!(psi_at(&psi, 1.0 + 1e-9, [0.0, 1.0]), None);
    }

    /// A budget the open pieces outlast leaves a gap just within `enough` not enclosed, where
    /// the whole budget bounds the pieces it stops as they stand (#18166); a budget the pieces
    /// close within gives the bound without one, bit for bit.
    #[test]
    fn a_budget_run_out_is_not_enclosed() {
        let run = |relative: f64, enough: f64, budget: Option<usize>| {
            bisect(
                [0.0, 1.0],
                32,
                &[],
                relative,
                0.0,
                [enough, f64::INFINITY],
                budget,
                |a, b| {
                    let h = b - a;
                    Ok(PieceBound::with(1.0, Ivl::from(0.1 * h * h), 0.0))
                },
            )
            .unwrap()
        };
        // (closing at 0.1 h² <= 5e-14 needs pieces of 7e-7: more than 1 << 11)
        let just_within = 1.0 + 1.0e-13;
        assert_eq!(run(0.0, just_within, Some(1 << 11)), None);
        assert!(run(0.0, just_within, None).is_some_and(|g| g >= 1.0));
        // (closing at 0.1 h² <= 1e-3 needs no cut of the 32 first pieces)
        assert_eq!(
            run(CLOSE_RELATIVE, f64::NEG_INFINITY, Some(1 << 11)).map(f64::to_bits),
            run(CLOSE_RELATIVE, f64::NEG_INFINITY, None).map(f64::to_bits)
        );
    }

    /// A piece whose end values are NaN (a non-finite control point) leaves the gap not
    /// enclosed, with or without a budget: its bound's interval sum is the whole line
    /// (`Ivl::checked`), so it is never dropped by the `max` of the closed pieces' bounds while
    /// the rest pass as the gap's bound (#18166).
    #[test]
    fn a_nan_piece_bound_is_not_enclosed() {
        for budget in [None, Some(64)] {
            let gap = bisect(
                [0.0, 1.0],
                32,
                &[],
                CLOSE_RELATIVE,
                0.0,
                [f64::NEG_INFINITY, f64::INFINITY],
                budget,
                |a, b| {
                    let h = b - a;
                    let ends = if (0.25..0.5).contains(&a) {
                        f64::NAN
                    } else {
                        1.0
                    };
                    Ok(PieceBound::with(ends, Ivl::from(0.1 * h * h), 0.0))
                },
            )
            .unwrap();
            assert_eq!(gap, None, "budget {budget:?}");
        }
    }

    /// With a budget, a gap whose end values pass `enough` is bounded with every piece, not the
    /// budget, whether the end value past it comes before or after the budget runs out (the
    /// budget only stops the search for a bound at most `enough`), a piece whose bound is
    /// within the largest end value closes as it stands, and an end value past `over` gives an
    /// infinite bound at once (#18166).
    #[test]
    fn a_budget_stops_only_the_search_below_enough() {
        // (end values 1 at the middle, 0.5 elsewhere; a term 0.1 h² closes at h of 1e-6)
        let piece = |a: f64, b: f64| {
            let h = b - a;
            let ends = if a <= 0.5 && 0.5 <= b { 1.0 } else { 0.5 };
            Ok(PieceBound::with(ends, Ivl::from(0.1 * h * h), 0.0))
        };
        let run = |[enough, over]: [f64; 2], budget: Option<usize>| {
            bisect(
                [0.0, 1.0],
                32,
                &[],
                0.0,
                1.0e-13,
                [enough, over],
                budget,
                piece,
            )
            .unwrap()
        };
        // (past `enough` = 0.9 at the middle: the budget of 64 does not stop it)
        let g = run([0.9, f64::INFINITY], Some(64));
        assert!(
            g.is_some_and(|g| (1.0..=1.0 + 1.0e-12).contains(&g)),
            "{g:?}"
        );
        // (the first pieces, within 1e-4 of `enough` = 0.5 + 1e-13, spend the budget before the
        // middle shows an end value past it: they are cut on after it, and close within it)
        let g = run([0.5 + 1.0e-13, f64::INFINITY], Some(64));
        assert!(
            g.is_some_and(|g| (1.0..=1.0 + 1.0e-12).contains(&g)),
            "{g:?}"
        );
        // (within `enough` = 1.0 + 1e-13 everywhere: the budget stops it)
        assert_eq!(run([1.0 + 1.0e-13, f64::INFINITY], Some(64)), None);
        // (an end value past `over`)
        assert_eq!(run([0.9, 0.95], Some(64)), Some(f64::INFINITY));
    }

    /// A slack no cut shrinks (a trim past its surface's net, #18166) does not hold a piece
    /// open: over half the range the pieces carry a slack above the closing threshold, and
    /// cutting them down to the deepest level would spend the pieces the other half needs,
    /// whose second-order term then stays at its first pieces' size (1e-4, against a bound of
    /// at most `ends + slack + floor`).
    #[test]
    fn a_slack_no_cut_shrinks_does_not_hold_the_bisection_open() {
        let (ends, slack, floor) = (1.0e-6, 1.0e-6, 1.0e-7);
        let g = bisect_with_slack(ends, floor, |a, _| if a < 0.5 { slack } else { 0.0 });
        assert!(g >= ends + slack && g <= ends + slack + floor, "{g:e}");
    }

    /// A slack that a cut shrinks (a box wider than the trim's own overshoot past the net,
    /// halved with the piece) is cut down as before: closing on the second term alone would
    /// stop with the slack still 1e-6 (at a piece of 1e-3), ten times the floor, where cutting
    /// on brings it below the floor.
    #[test]
    fn a_slack_a_cut_shrinks_is_cut_down() {
        let (ends, floor) = (1.0e-6, 1.0e-7);
        let g = bisect_with_slack(ends, floor, |a, h| if a < 0.5 { 1.0e-3 * h } else { 0.0 });
        assert!(g >= ends && g <= ends + floor, "{g:e}");
    }

    /// Newton's step toward the nearest parameter is `(target - C) . C' / |C'|^2`: on a stretch
    /// where the curve is slow (here its first three control points 1e-9 apart) it overshoots
    /// to a far part of the curve, and the steps after need not come back (#18166, `brep_mirror`
    /// `mat0094`, a `ψ` point 0.011 off along the curve). The parameter returned is never
    /// farther from the target than the guess.
    #[test]
    fn the_nearest_parameter_is_never_farther_than_its_guess() {
        let k = Kernel::default();
        let at = |x: f64, y: f64| Point3::new(x, y, 0.0);
        let curve = Top::curve(
            &[at(0.0, 0.0), at(1.0e-9, 0.0), at(2.0e-9, 0.0), at(1.0, 1.0)],
            3,
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            false,
        );
        let target = at(1.0e-6, 1.0e-3);
        let near = |t: f64| curve.evaluate_point(&k, t).unwrap().distance(target);
        for guess in [0.0, 1.0e-4, 1.0e-3] {
            let (t, _) = nearest_param(&k, &curve, target, guess, [0.0, 1.0]).unwrap();
            assert!(
                near(t) <= near(guess),
                "{guess:e}: {t:e} at {:e}, guess at {:e}",
                near(t),
                near(guess)
            );
        }
    }

    /// The bowed trim of [`a_trim_off_its_curve_is_not_bounded_close`] asked only whether it is
    /// above `over`: at or above its full bound the answer is that bound, below it infinity.
    #[test]
    fn a_bound_up_to_a_cutoff_answers_as_the_full_bound() {
        let t = Top::new();
        let (a, b) = ([0.2, 0.3], [0.7, 0.6]);
        let bow = [0.45 - 0.03, 0.45 + 0.05];
        let trim = Top::curve(
            &[t.uv(a), t.uv(bow), t.uv(b)],
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            true,
        );
        let curve = Top::curve(
            &[t.point(a, 0.0), t.point(b, 0.0)],
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            false,
        );
        let up_to = |over: f64| {
            trim_image_gap_bound_up_to(&t.k, &t.surface, &curve, &trim, [0.0, 1.0], 1e-9, over)
                .unwrap()
                .unwrap()
        };
        let g = up_to(f64::INFINITY);
        assert_eq!(g, t.bound(&curve, &trim).unwrap());
        assert_eq!(up_to(g).to_bits(), g.to_bits());
        assert_eq!(up_to(2.0 * g).to_bits(), g.to_bits());
        assert_eq!(up_to(0.5 * g), f64::INFINITY);
    }

    /// A line edge whose point `P` is far from the piece, so its evaluation carries a slack
    /// (`line_evaluation_slack`), over a straight trim `lift` below it: asked whether the bound
    /// is above a cutoff between the bisected gap and the gap plus the slack, the answer is
    /// that it is (infinity), as the full bound says; at the full bound it is that bound.
    #[test]
    fn a_cutoff_counts_a_line_edges_slack() {
        use crate::curve::line::BoundedLineOptions;
        use crate::geom::extent1d::Extent1d;
        let t = Top::new();
        let lift = 2.0e-4;
        let (a, b) = ([0.2, 0.3], [0.7, 0.6]);
        let (pa, pb) = (t.point(a, lift), t.point(b, lift));
        let len = (pb - pa).length();
        let unit = (pb - pa) * (1.0 / len);
        let far = 1e6;
        let mut line = Line::bounded(
            pa - unit * far,
            unit,
            &Extent1d::from_min_max(far, far + len),
            &BoundedLineOptions::default(),
        );
        line.edit_parameterization(&Extent1d::from_min_max(0.0, 1.0))
            .unwrap();
        let slack = line_evaluation_slack(&line, false).unwrap();
        assert!(slack > 0.0, "{slack:e}");
        let curve = Curve::from(line);
        let trim = Top::curve(&[t.uv(a), t.uv(b)], 1, vec![0.0, 0.0, 1.0, 1.0], true);
        let up_to = |over: f64| {
            trim_image_gap_bound_up_to(&t.k, &t.surface, &curve, &trim, [0.0, 1.0], 1e-9, over)
                .unwrap()
                .unwrap()
        };
        let g = up_to(f64::INFINITY);
        assert_eq!(g, t.bound(&curve, &trim).unwrap());
        assert!(
            g >= lift && slack > g - g.next_down(),
            "{g:e}, slack {slack:e}"
        );
        assert_eq!(up_to(g).to_bits(), g.to_bits());
        // (at most the gap plus a part of the slack: the bisection's gap is below it)
        assert_eq!(up_to(g - 0.5 * slack), f64::INFINITY);
        assert_eq!(up_to(g.next_down()), f64::INFINITY);
    }

    /// A curve with a corner (a degree-1 polyline with an interior knot of full multiplicity)
    /// and its trim with the same corner, lifted `lift`: the corner is bounded by the Lipschitz
    /// term, which still closes, so the bound stays at least `lift` and within twice the
    /// bisection's closing fraction above it.
    #[test]
    fn a_trim_with_a_corner_is_bounded() {
        let t = Top::new();
        let lift = 2.0e-4;
        let pts = [[0.2, 0.3], [0.5, 0.7], [0.8, 0.35]];
        let knots = vec![0.0, 0.0, 0.5, 1.0, 1.0];
        let trim = Top::curve(&pts.map(|f| t.uv(f)), 1, knots.clone(), true);
        let curve = Top::curve(&pts.map(|f| t.point(f, lift)), 1, knots, false);
        let g = t.bound(&curve, &trim).unwrap();
        assert!(
            g >= lift && g <= lift * (1.0 + 2.0 * CLOSE_RELATIVE),
            "{g:e} against {lift:e}"
        );
        // a range past the curve's domain encloses nothing
        let past = trim_image_gap_bound(&t.k, &t.surface, &curve, &trim, [0.0, 1.5], 1e-9).unwrap();
        assert!(past.is_none());
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

    /// A half-unit piece of a line whose point `P` is `1e8` away: the piece lies near the origin,
    /// so its NURB's control points are small, but the analytic point `P + s V` rounds at the
    /// scale of `P` (its STEP parameter `s` is near `-1e8`), about `1e-8`. The slack is at
    /// least the measured distance between the analytic point and its NURB's point at every
    /// sampled parameter (this shows the measured gap only; the bound itself is derived), and
    /// a bound in the size of the control points alone (`γ_32` of `8 m`) falls short of it.
    #[test]
    fn a_line_piece_far_from_its_point_has_a_slack_at_the_point_scale() {
        use crate::curve::line::BoundedLineOptions;
        use crate::geom::extent1d::Extent1d;
        use crate::math::Vec3;
        let far = 1e8;
        for (dir, lo) in [
            (Vec3::new(1.0, 0.0, 0.0), -far + 0.25),
            (Vec3::new(3.0, 4.0, 12.0), -far + 0.1),
        ] {
            let unit = dir / dir.length();
            let p = Point3::new(far * unit.x, far * unit.y, far * unit.z);
            let mut line = Line::bounded(
                p,
                dir,
                &Extent1d::from_min_max(lo, lo + 0.5),
                &BoundedLineOptions::default(),
            );
            // (its NURB over `[0, 1]`: the STEP parameter is then computed from it, rounding
            // at the scale of `P`; over the STEP interval itself the NURB parameter carries
            // the same rounding and the two agree)
            line.edit_parameterization(&Extent1d::from_min_max(0.0, 1.0))
                .unwrap();
            let slack = line_evaluation_slack(&line, false).unwrap();
            let (cn, curve_slack) = exact_curve_data(&Curve::from(line.clone()), false)
                .unwrap()
                .unwrap();
            assert_eq!(slack.to_bits(), curve_slack.to_bits());
            let (t0, t1) = (cn.u[1], cn.u[2]);
            let q = |i: usize| [cn.pw[4 * i], cn.pw[4 * i + 1], cn.pw[4 * i + 2]];
            let (q0, q1) = (q(0), q(1));
            let m = q0.iter().chain(&q1).fold(0.0_f64, |a, x| a.max(x.abs()));
            assert!(m < 1.0, "the piece is near the origin: {m:e}");
            let old = crate::nurbs::math::gamma(32) * 8.0 * m;
            let mut measured = 0.0_f64;
            for i in 0..=1000 {
                let t = t0 + (t1 - t0) * f64::from(i) / 1000.0;
                let lam = (t - t0) / (t1 - t0);
                let a = line.evaluate_point(t);
                let d = [a.x, a.y, a.z]
                    .iter()
                    .zip(q0.iter().zip(&q1))
                    .map(|(a, (x0, x1))| a - ((1.0 - lam) * x0 + lam * x1))
                    .fold(0.0_f64, |s, d| s + d * d)
                    .sqrt();
                measured = measured.max(d);
            }
            assert!(measured > old, "{measured:e} against the old {old:e}");
            assert!(slack >= measured, "{slack:e} against {measured:e}");
            // and of the order of the point's rounding, not far beyond it
            assert!(slack < 1e-6, "{slack:e}");
        }
    }

    /// [`curve_uv_gap_lower_bound`] of a line `lift` above the bilinear plane patch z = 0 (x,
    /// y in 0..1) at its UV line: at every parameter the gap is exactly `lift`, and the lower
    /// bound is at most that (and at most [`curve_uv_gap_bound`]'s upper bound) and below it
    /// by no more than rounding, a UV point on the net's end (`u = 1`) included. Read at the
    /// reversed UV line, at `t = 0` the gap is the shadow's whole length. Nothing is enclosed
    /// at a parameter outside the domain or not finite, nor where the UV line runs far past
    /// the net's end.
    #[test]
    fn a_line_over_its_uv_line_has_its_height_as_lower_bound() {
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
        let lower = |c: &BSplineCurve, w: &BSplineCurve, t: f64| {
            curve_uv_gap_lower_bound(&k, c, w, &surface, t).unwrap()
        };
        // (to the net's end u = 1 at t = 1)
        let curve = line([0.2, 0.3, lift], [1.0, 0.6, lift]);
        let uv = line([0.2, 0.3, 0.0], [1.0, 0.6, 0.0]);
        let upper = curve_uv_gap_bound(&k, &curve, &uv, &surface, [0.0, 1.0], 1e-9)
            .unwrap()
            .unwrap();
        // the bound gives up rounding only: coordinates near 1 cost a few ulps of 1 (at
        // t = 1 the UV box rounds past the net's end, and the continuation's slack, about
        // 6e-15, is taken off)
        for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let lo = lower(&curve, &uv, t).unwrap_or_else(|| panic!("t {t}: not enclosed"));
            assert!(
                lo <= lift && lo <= upper && lo >= lift - 1e-13,
                "t {t}: {lo:e} against {lift:e} (upper {upper:e})"
            );
        }
        let reversed = line([1.0, 0.6, 0.0], [0.2, 0.3, 0.0]);
        let full = (0.8_f64 * 0.8 + 0.3 * 0.3 + lift * lift).sqrt();
        let lo = lower(&curve, &reversed, 0.0).unwrap();
        assert!(lo <= full && lo >= full * (1.0 - 1e-12), "{lo:e} {full:e}");
        for t in [-0.5, 1.5, f64::NAN, f64::INFINITY] {
            assert_eq!(lower(&curve, &uv, t), None, "t {t}");
        }
        let far = line([0.2, 0.3, 0.0], [1.5, 0.3, 0.0]);
        let over = line([0.2, 0.3, lift], [1.5, 0.3, lift]);
        assert!(lower(&over, &far, 0.0).is_some());
        assert_eq!(lower(&over, &far, 1.0), None);
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
        let line = Curve::from(
            Line::create_line_segment(
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
        let line = Curve::from(
            Line::create_line_segment(
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
        let arc = Curve::from(
            crate::primitive::api_curves::create_arc(Point3::ZERO, r, -deg, deg).unwrap(),
        );
        let base = NurbsCurve::of(arc.b_spline_base().unwrap()).unwrap();
        let (t0, t1) = (base.u[base.p], base.u[base.n]);
        let th = deg.to_radians();
        let (a, b) = (
            Point3::new(r * th.cos(), -r * th.sin(), 0.0),
            Point3::new(r * th.cos(), r * th.sin(), 0.0),
        );
        let chord = Curve::from(
            Line::create_line_segment(
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
        let bow = Curve::from(BSplineCurve::from_nurb(bez, 3, false));
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
