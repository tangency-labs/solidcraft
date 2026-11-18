//! A proved enclosure of the largest normal angle along a seam, over the whole seam and not
//! only at stations (not upstream; WR-613, tangency-labs/solidcraft#18640).
//!
//! [`super::continuity::seam_normal_angle`] reads a seam at stations: the acute angle
//! (`min(θ, π − θ)`) between two surfaces' unit normals at their nearest points to points of
//! the seam's curve. A station reading is a value of that angle, so it says nothing of the
//! angle between stations. This module encloses the largest angle over the whole curve.
//!
//! **What is enclosed.** The curve is `C(t)`, `t` in `[lo, hi]`, a B-spline (a NURBS, an arc
//! included). The sides are surfaces `S_1`, `S_2`, each a NURBS net whose parameters are the
//! surface's ([`super::curve_surface_gap::NetSurface`]). For each side, `W_s(t)` is the
//! correspondence of [`super::curve_surface_gap`]: a B-spline of parameters through the
//! nearest points of the curve's samples, chosen only to be close to the nearest point.
//! Write `g_s(t) = C(t) − S_s(W_s(t))`. The angle at `t` is read at the **feet** of `C(t)`:
//! the points `w` of the surface's domain in the parameter box `W_s(t) ± R` (below) no
//! farther from `C(t)` than the surface at `W_s(t)` (clamped to the domain) is. They include
//! the nearest point of `S_s` to `C(t)` in that box, and the proof shows it is inside the
//! box, not on its edge (a local nearest point). The enclosure is `lower <= Θ <= upper` for
//! `Θ` the largest, over `t` in `[lo, hi]`, of the acute angle between the normals at the
//! two sides' feet. Both bounds hold for every choice of feet.
//!
//! **The proof.** Two facts, each enclosed in outward-rounded interval arithmetic
//! ([`crate::blend::fillet_section::interval`]). The curve's and the correspondences' values
//! come from `curve_derivs_over`, the surfaces' partials over a box from
//! `NetSurface::partials` (each part of the box on its own polynomial piece, the parts
//! hulled; past a net's end the end span's polynomial continued, within `slack` of the
//! surface at the clamped parameters).
//!
//! 1. *The feet are near the correspondence.* Over a set `T` of parameters (a station, or a
//!    piece `[a, b]`), let `G >= |g_s(t)|` ([`super::curve_surface_gap`]'s bound: the value
//!    at a station; over a piece the larger end value and `h^2 / 8 sup|g''|`, or `h sup|g'|`
//!    across a knot line of continuity C^0, with the slack) and `B` the box `W_s(T) ± R`.
//!    A foot `w` at `t` is within `G + slack` of `C(t)`, so `|S(w) − S(W)| <= a := 2G +
//!    slack`. By the mean value theorem along the segment (the surface is continuous and
//!    piecewise polynomial), `S(w) − S(W) = J̄ (w − W)` with the averaged Jacobian `J̄` in the
//!    interval hull `[J]` of `[S_u S_v]` over `B`. Every `J` in `[J]` is `J_m + E` (`J_m` the
//!    midpoints, `|E|_2 <= |E|_F <= r` the radii's root sum of squares), so by Weyl's
//!    inequality `|J̄ d| >= σ |d|` with `σ = σ_min(J_m) − r`, `σ_min(J_m)` enclosed directly.
//!    Hence `|w − W| <= ρ := a / σ`: the feet lie in `F = W_s(T) ± ρ`. The box is first
//!    `R = 2a / σ_0` (`σ_0` read over `W_s(T)`), regrown to `R = 2ρ` where `ρ >= R`, at most
//!    [`FOOT_GROWTHS`] boxes; the bound is used only where `ρ < R`.
//! 2. *The normal over the feet.* `F` is cut at the knot lines across which the net is not
//!    C^1 (`NetSurface::low_lines`), at most [`MAX_PARTS`] parts. On a part `P`, with centre
//!    `c` and half-diagonal `r_P`, the unit normal `n = N / |N|` (`N = S_u × S_v`) is
//!    Lipschitz: `|n(w) − n(c)| <= D |w − c|` with `D = sqrt(sup|n_u|^2 + sup|n_v|^2)` and
//!    `|n_u| <= |N_u| / |N|` (`n_u` is the projection of `N_u / |N|` off `n`), `N_u = S_uu ×
//!    S_v + S_u × S_uv`, `N_v = S_uv × S_v + S_u × S_vv`, all over `P`. So a foot's normal is
//!    within the angle `δ_P = 2 asin(D r_P / 2)` of `n(c)`, enclosed at the point `c`. The
//!    acute angle is a metric on lines, so for parts `P` of side 1 and `Q` of side 2 the angle
//!    at the feet is within `δ_P + δ_Q` of the acute angle between `n_1(c_P)` and `n_2(c_Q)`,
//!    `2 asin(|n_1 − s n_2| / 2)` with `s` the sign of `n_1 · n_2` (enclosed away from 0).
//!
//! A piece's bound is the largest over its pairs of parts of the pair's upper angle plus
//! their `δ`s (at most `π / 2`, the acute angle's largest); a station's lower bound is the
//! least over its pairs of the pair's lower angle less their `δ`s. `upper` is the largest
//! piece bound. The pieces start between the seam check's stations
//! ([`super::continuity::SEAM_PARTS`]) and are bisected while a piece's bound is above the
//! caller's `below` or not proved, at most [`MAX_DEPTH`] deep and [`MAX_PIECES`] in all (an
//! explicit stack, no recursion); once the largest angle is shown above `below` (a station's
//! lower bound, or a piece's bound at the limits) cutting stops, since no cut can then bring
//! `upper` to `below`, and the pieces left keep their bounds uncut. `lower` is the largest station lower bound (0 where none is
//! proved; [`seam_angle_lower_bound`] gives `None` there). `asin` is widened by a relative
//! [`ASIN_ROUNDING`] for the library function's error.
//!
//! Not enclosed (`None`): a side whose surface has no net, a sample with no nearest point, or
//! a piece at the deepest bisection with no bound (a box too far past a net's end, a knot
//! line of continuity below C^0, a zero normal or `σ`, a foot bound that does not close, more
//! than [`MAX_PARTS`] parts).
//!
//! Deviations: none (no upstream counterpart). No recursion.

use crate::blend::fillet_section::certificate::{curve_derivs_over, NurbsCurve};
use crate::blend::fillet_section::interval::{round_down, round_up, Ivl};
use crate::blend::fillet_section::series::V3;
use crate::blend::fillet_section::station::Partials;
use crate::curve::curve::p2::{Curve as SmCurve, CurveKind};
use crate::error::{Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::kernel::Kernel;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::query::continuity::SEAM_PARTS;
use crate::query::curve_surface_gap::{correspondence, piece as gap_piece, NetSurface};
use crate::surface::surface::Surface;
use crate::topology::{Brep, EdgeId};

const OP: &str = "query::seam_bound";

/// The deepest bisection of one of the starting pieces.
pub const MAX_DEPTH: u32 = 12;
/// The most pieces the bisection may make before it stops cutting.
pub const MAX_PIECES: usize = 1 << 16;
/// The most boxes a foot bound tries: each regrown to twice the last box's foot bound
/// where that bound overran it.
pub const FOOT_GROWTHS: u32 = 4;
/// The most parts a box of feet is cut into at the knot lines it crosses.
pub const MAX_PARTS: usize = 16;
/// The relative widening of a computed `asin` (the library's error, a few ulps at most).
pub const ASIN_ROUNDING: f64 = 1.0e-14;

/// A proved enclosure of the largest acute normal angle along a seam (module docs), radians.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeamAngleEnclosure {
    /// No more than the largest angle (the largest station lower bound).
    pub lower: f64,
    /// No less than the largest angle over the whole seam.
    pub upper: f64,
}

/// One side prepared: its net and its correspondence.
struct Side {
    net: NetSurface,
    w: NurbsCurve,
}

/// A part of a box of feet (fact 2): the unit normal at its centre and the angle `δ` within
/// which every normal of the part lies of it.
struct Cone {
    n: V3<Ivl>,
    delta: f64,
}

fn v_add(a: V3<Ivl>, b: V3<Ivl>) -> V3<Ivl> {
    V3 {
        x: a.x + b.x,
        y: a.y + b.y,
        z: a.z + b.z,
    }
}

fn v_sub(a: V3<Ivl>, b: V3<Ivl>) -> V3<Ivl> {
    V3 {
        x: a.x - b.x,
        y: a.y - b.y,
        z: a.z - b.z,
    }
}

fn v_scale(a: V3<Ivl>, s: Ivl) -> V3<Ivl> {
    V3 {
        x: a.x * s,
        y: a.y * s,
        z: a.z * s,
    }
}

fn v_dot(a: V3<Ivl>, b: V3<Ivl>) -> Ivl {
    a.x * b.x + a.y * b.y + a.z * b.z
}

fn v_cross(a: V3<Ivl>, b: V3<Ivl>) -> V3<Ivl> {
    V3 {
        x: a.y * b.z - a.z * b.y,
        y: a.z * b.x - a.x * b.z,
        z: a.x * b.y - a.y * b.x,
    }
}

fn finite(v: V3<Ivl>) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

/// An upper bound of `|v|` over the enclosure, `None` unless finite.
fn norm_hi(v: V3<Ivl>) -> Option<f64> {
    let sq = |a: Ivl| {
        let m = Ivl::from(a.mag());
        m * m
    };
    let hi = (sq(v.x) + sq(v.y) + sq(v.z)).sqrt().hi;
    (finite(v) && hi.is_finite()).then_some(hi)
}

/// A lower bound of `|v|` over the enclosure (0 where it holds 0), `None` unless finite.
fn norm_lo(v: V3<Ivl>) -> Option<f64> {
    let sq = |a: Ivl| {
        let m = Ivl::from(a.mig());
        m * m
    };
    let lo = (sq(v.x) + sq(v.y) + sq(v.z)).sqrt().lo;
    (finite(v) && lo.is_finite()).then_some(lo.max(0.0))
}

/// `2 asin(c / 2)` rounded up: the angle of a chord `c` between unit vectors (π past 2).
fn chord_angle_hi(c: f64) -> f64 {
    let half = round_up(0.5 * c);
    if half.is_nan() || half >= 1.0 {
        return std::f64::consts::PI;
    }
    round_up(2.0 * half.max(0.0).asin() * (1.0 + ASIN_ROUNDING))
}

/// `2 asin(c / 2)` rounded down.
fn chord_angle_lo(c: f64) -> f64 {
    let half = round_down(0.5 * c);
    if half <= 0.0 {
        return 0.0;
    }
    round_down(2.0 * half.min(1.0).asin() * (1.0 - ASIN_ROUNDING)).max(0.0)
}

/// The surface's partials up to `lk` over the parameter box `u x v`, with the slack of a box
/// past the net's end (`NetSurface::partials`: how far the continued polynomial at a point
/// of the box may be from the surface at the clamped point; 0 for `lk` 0 or a box inside
/// the net). Knot lines are crossed where the net is C^1 for `lk` 2, C^0 otherwise. `None`
/// where the net refuses the box.
fn partials_over(
    net: &NetSurface,
    u: Ivl,
    v: Ivl,
    lk: usize,
) -> Result<Option<(Partials<Ivl>, f64)>> {
    let w = [V3 {
        x: u,
        y: v,
        z: Ivl::from(0.0),
    }];
    match net.partials(&w, lk, usize::from(lk >= 2)) {
        Ok(p) => Ok(Some(p)),
        Err(e) if e.is_unported_or_cancelled() => Err(e),
        Err(_) => Ok(None),
    }
}

/// A lower bound of the least singular value of every `[S_u S_v]` in the enclosure, `None`
/// unless proved positive (fact 1, Weyl's inequality about the midpoints).
fn sigma_lo(p: &Partials<Ivl>) -> Option<f64> {
    let (su, sv) = (p.at(1, 0), p.at(0, 1));
    let mid = |a: Ivl| Ivl::from(a.mid());
    let rad = |a: Ivl| {
        let m = a.mid();
        round_up(a.hi - m).max(round_up(m - a.lo))
    };
    let ms = |v: V3<Ivl>| V3 {
        x: mid(v.x),
        y: mid(v.y),
        z: mid(v.z),
    };
    let mut sq = Ivl::from(0.0);
    for c in [su.x, su.y, su.z, sv.x, sv.y, sv.z] {
        let r = Ivl::from(rad(c));
        sq = sq + r * r;
    }
    let r = sq.sqrt().hi;
    let s = point_sigma_lo(ms(su), ms(sv))?;
    let out = round_down(s - r);
    (r.is_finite() && out > 0.0).then_some(out)
}

/// A lower bound of the least singular value of `[S_u S_v]` enclosed in `su`, `sv` (direct;
/// tight for point enclosures), `None` unless proved positive.
fn point_sigma_lo(su: V3<Ivl>, sv: V3<Ivl>) -> Option<f64> {
    let a = v_dot(su, su);
    let b = v_dot(su, sv);
    let c = v_dot(sv, sv);
    let half = Ivl::from(0.5);
    let mean = (a + c) * half;
    let diff = (a - c) * half;
    let lambda = mean - (diff * diff + b * b).sqrt();
    let lo = lambda.lo;
    if !(lo.is_finite() && lo > 0.0) {
        return None;
    }
    let s = round_down(lo.sqrt());
    (s > 0.0).then_some(s)
}

/// The normal's Lipschitz bound `D` over the enclosure (fact 2), `None` unless finite.
fn normal_lipschitz(p: &Partials<Ivl>) -> Option<f64> {
    let (su, sv) = (p.at(1, 0), p.at(0, 1));
    let (suu, suv, svv) = (p.at(2, 0), p.at(1, 1), p.at(0, 2));
    let n = v_cross(su, sv);
    let nu = v_add(v_cross(suu, sv), v_cross(su, suv));
    let nv = v_add(v_cross(suv, sv), v_cross(su, svv));
    let len = norm_lo(n)?;
    if len.is_nan() || len <= 0.0 {
        return None;
    }
    let (a, b) = (norm_hi(nu)?, norm_hi(nv)?);
    let l = Ivl::from(len);
    let du = Ivl::from(a) / l;
    let dv = Ivl::from(b) / l;
    let d = (du * du + dv * dv).sqrt().hi;
    d.is_finite().then_some(d)
}

/// The box of feet `F = W ± ρ` for a correspondence sweeping `u x v` with gap at most `gap`
/// (fact 1), `None` where the foot bound does not close.
fn feet_box(net: &NetSurface, u: Ivl, v: Ivl, gap: f64) -> Result<Option<(Ivl, Ivl)>> {
    if !(gap.is_finite() && gap >= 0.0) {
        return Ok(None);
    }
    let Some((p0, slack)) = partials_over(net, u, v, 1)? else {
        return Ok(None);
    };
    let apart = round_up(round_up(2.0 * gap) + slack);
    if !apart.is_finite() {
        return Ok(None);
    }
    if apart == 0.0 {
        return Ok(Some((u, v)));
    }
    let Some(s0) = sigma_lo(&p0) else {
        return Ok(None);
    };
    let grow = |a: Ivl, by: f64| Ivl::new(round_down(a.lo - by), round_up(a.hi + by));
    let mut reach = round_up(round_up(2.0 * apart) / s0);
    for _ in 0..FOOT_GROWTHS {
        if !reach.is_finite() {
            return Ok(None);
        }
        let Some((p, _)) = partials_over(net, grow(u, reach), grow(v, reach), 1)? else {
            return Ok(None);
        };
        let Some(s) = sigma_lo(&p) else {
            return Ok(None);
        };
        let rho = round_up(apart / s);
        if rho < reach {
            return Ok(Some((grow(u, rho), grow(v, rho))));
        }
        reach = round_up(2.0 * rho);
    }
    Ok(None)
}

/// `[lo, hi]` cut at `lines` (sorted, strictly inside).
fn cut(a: Ivl, lines: &[f64]) -> Vec<Ivl> {
    let mut out = Vec::with_capacity(lines.len() + 1);
    let mut from = a.lo;
    for &x in lines {
        out.push(Ivl::new(from, x));
        from = x;
    }
    out.push(Ivl::new(from, a.hi));
    out
}

/// The cones of side `side` over the feet of the correspondence sweeping `u x v` with gap
/// at most `gap` (facts 1 and 2), `None` where they are not proved.
fn cones(side: &Side, u: Ivl, v: Ivl, gap: f64) -> Result<Option<Vec<Cone>>> {
    let net = &side.net;
    let Some((fu, fv)) = feet_box(net, u, v, gap)? else {
        return Ok(None);
    };
    let (lu, lv) = net.low_lines(fu, fv);
    let (us, vs) = (cut(fu, &lu), cut(fv, &lv));
    if us.len().saturating_mul(vs.len()) > MAX_PARTS {
        return Ok(None);
    }
    let mut out = Vec::with_capacity(us.len() * vs.len());
    for &pu in &us {
        for &pv in &vs {
            let (cu, cv) = (pu.mid(), pv.mid());
            let Some((at, _)) = partials_over(net, Ivl::from(cu), Ivl::from(cv), 1)? else {
                return Ok(None);
            };
            let nc = v_cross(at.at(1, 0), at.at(0, 1));
            let (Some(lo), Some(hi)) = (norm_lo(nc), norm_hi(nc)) else {
                return Ok(None);
            };
            if lo.is_nan() || lo <= 0.0 {
                return Ok(None);
            }
            let n = v_scale(nc, Ivl::from(1.0) / Ivl::new(lo, hi));
            let delta = if pu.lo == pu.hi && pv.lo == pv.hi {
                0.0
            } else {
                let Some((p, _)) = partials_over(net, pu, pv, 2)? else {
                    return Ok(None);
                };
                let Some(d) = normal_lipschitz(&p) else {
                    return Ok(None);
                };
                let hu = round_up(pu.hi - cu).max(round_up(cu - pu.lo));
                let hv = round_up(pv.hi - cv).max(round_up(cv - pv.lo));
                let r = (Ivl::from(hu) * Ivl::from(hu) + Ivl::from(hv) * Ivl::from(hv))
                    .sqrt()
                    .hi;
                chord_angle_hi(round_up(d * r))
            };
            out.push(Cone { n, delta });
        }
    }
    Ok(Some(out))
}

/// The acute angle between the lines of two unit normals enclosed in `a`, `b`: `(lower,
/// upper)`, `None` where the dot product straddles 0 (the sign is not proved).
fn line_angle(a: V3<Ivl>, b: V3<Ivl>) -> Option<(f64, f64)> {
    let dot = v_dot(a, b);
    let sign = if dot.lo > 0.0 {
        1.0
    } else if dot.hi < 0.0 {
        -1.0
    } else {
        return None;
    };
    let chord = v_sub(a, v_scale(b, Ivl::from(sign)));
    Some((
        chord_angle_lo(norm_lo(chord)?),
        chord_angle_hi(norm_hi(chord)?),
    ))
}

/// Both sides' cones over the parameters `t` (a station or a piece).
fn both_cones(cn: &NurbsCurve, sides: &[Side; 2], t: Ivl) -> Result<Option<[Vec<Cone>; 2]>> {
    let mut out: [Vec<Cone>; 2] = [Vec::new(), Vec::new()];
    for (slot, side) in out.iter_mut().zip(sides.iter()) {
        let Some(w) = derivs(&side.w, t)? else {
            return Ok(None);
        };
        let gap = if t.lo == t.hi {
            let Some(c) = derivs(cn, t)? else {
                return Ok(None);
            };
            let Some((p, _)) = partials_over(&side.net, w.x, w.y, 0)? else {
                return Ok(None);
            };
            let Some(g) = norm_hi(v_sub(c, p.at(0, 0))) else {
                return Ok(None);
            };
            g
        } else {
            match gap_piece(cn, &side.w, &side.net, t.lo, t.hi) {
                Ok(Some((ends, term))) => round_up(ends + term),
                Err(e) if e.is_unported_or_cancelled() => return Err(e),
                Ok(None) | Err(_) => return Ok(None),
            }
        };
        let Some(c) = cones(side, w.x, w.y, gap)? else {
            return Ok(None);
        };
        *slot = c;
    }
    Ok(Some(out))
}

/// The value of a curve over `t`, `None` where it is not enclosed.
fn derivs(c: &NurbsCurve, t: Ivl) -> Result<Option<V3<Ivl>>> {
    match curve_derivs_over(c, t, 0, None) {
        Ok(d) => Ok(d.first().copied()),
        Err(e) if e.is_unported_or_cancelled() => Err(e),
        Err(_) => Ok(None),
    }
}

/// The proved lower bound of the angle at the feet at station `t`, `None` where none is.
fn station_lower(cn: &NurbsCurve, sides: &[Side; 2], t: f64) -> Result<Option<f64>> {
    let Some([c1, c2]) = both_cones(cn, sides, Ivl::from(t))? else {
        return Ok(None);
    };
    let mut lower = f64::INFINITY;
    for a in &c1 {
        for b in &c2 {
            let Some((lo, _)) = line_angle(a.n, b.n) else {
                return Ok(None);
            };
            lower = lower.min(round_down(round_down(lo - a.delta) - b.delta).max(0.0));
        }
    }
    Ok(lower.is_finite().then_some(lower))
}

/// The proved upper bound of the angle at the feet over the piece `[a, b]`, `None` where
/// none is.
fn piece_upper(cn: &NurbsCurve, sides: &[Side; 2], a: f64, b: f64) -> Result<Option<f64>> {
    let Some([c1, c2]) = both_cones(cn, sides, Ivl::new(a, b))? else {
        return Ok(None);
    };
    let mut upper = 0.0_f64;
    for p in &c1 {
        for q in &c2 {
            let hi = match line_angle(p.n, q.n) {
                Some((_, hi)) => round_up(round_up(hi + p.delta) + q.delta),
                None => std::f64::consts::FRAC_PI_2,
            };
            upper = upper.max(hi.min(std::f64::consts::FRAC_PI_2));
        }
    }
    Ok(upper.is_finite().then_some(upper))
}

/// The seam check's stations over `interval` ([`SEAM_PARTS`]), sorted, each once.
fn stations(interval: Extent1d) -> Vec<f64> {
    let mut ts: Vec<f64> = Vec::new();
    for ([p0, p1], n) in SEAM_PARTS {
        let last = n.saturating_sub(1).max(1);
        for i in 0..n {
            let s = if i == last {
                p1
            } else {
                p0 + (p1 - p0) * (i as f64) / (last as f64)
            };
            ts.push(interval.evaluate(s));
        }
    }
    ts.sort_by(f64::total_cmp);
    ts.dedup();
    ts
}

/// The curve and each side's net and correspondence over `interval`; `None` where a side
/// has no net or a sample no nearest point.
fn prepare(
    k: &Kernel,
    curve: &BSplineCurve,
    interval: Extent1d,
    sides: [&Surface; 2],
) -> Result<Option<(NurbsCurve, [Side; 2])>> {
    let (lo, hi) = (interval.evaluate(0.0), interval.evaluate(1.0));
    if !(lo.is_finite() && hi.is_finite() && lo < hi) {
        return Err(SolidError::invalid_input(
            OP,
            "an interval not finite and increasing",
        ));
    }
    let Ok(cn) = NurbsCurve::of(curve) else {
        return Ok(None);
    };
    let mut prepared: Vec<Side> = Vec::with_capacity(2);
    for s in sides {
        let s = s.clone_for_evaluation()?;
        let domain = s.natural_uv_domain_with_nurb(k)?;
        let Some((net, w)) = correspondence(k, &s, &domain, curve, [lo, hi])? else {
            return Ok(None);
        };
        let Ok(w) = NurbsCurve::of(&w) else {
            return Ok(None);
        };
        prepared.push(Side { net, w });
    }
    let Ok(sides): std::result::Result<[Side; 2], _> = prepared.try_into() else {
        return Ok(None);
    };
    Ok(Some((cn, sides)))
}

/// The largest station lower bound, `None` where no station's is proved.
fn stations_lower(cn: &NurbsCurve, sides: &[Side; 2], ts: &[f64]) -> Result<Option<f64>> {
    let mut lower: Option<f64> = None;
    for &t in ts {
        if let Some(l) = station_lower(cn, sides, t)? {
            lower = Some(lower.map_or(l, |m| m.max(l)));
        }
    }
    Ok(lower)
}

/// A proved lower bound of the largest acute normal angle between `sides` along `curve` over
/// `interval` (module docs): the largest station lower bound ([`SEAM_PARTS`]'s stations)
/// among the stations where one is proved. Stations where none is are passed over: a lower
/// bound needs only some of them. `None` where no station's is proved.
///
/// # Errors
///
/// Those of [`seam_angle_enclosure`].
pub fn seam_angle_lower_bound(
    k: &Kernel,
    curve: &BSplineCurve,
    interval: Extent1d,
    sides: [&Surface; 2],
) -> Result<Option<f64>> {
    let Some((cn, sides)) = prepare(k, curve, interval, sides)? else {
        return Ok(None);
    };
    stations_lower(&cn, &sides, &stations(interval))
}

/// A proved enclosure of the largest acute normal angle between `sides` along `curve` over
/// `interval` (module docs). Pieces are bisected while their bound is above `below` (pass the
/// bound the caller holds the seam to; `0` bisects only until the angle is shown above it),
/// and not once the angle is shown above `below`. `None` where the angle is
/// not enclosed (module docs).
///
/// # Errors
///
/// The kernel's unported and cancelled statuses; [`SolidError::InvalidInput`] for an interval
/// that is not finite and increasing.
pub fn seam_angle_enclosure(
    k: &Kernel,
    curve: &BSplineCurve,
    interval: Extent1d,
    sides: [&Surface; 2],
    below: f64,
) -> Result<Option<SeamAngleEnclosure>> {
    let Some((cn, sides)) = prepare(k, curve, interval, sides)? else {
        return Ok(None);
    };
    let ts = stations(interval);
    let lower = stations_lower(&cn, &sides, &ts)?.unwrap_or(0.0);
    let mut stack: Vec<(f64, f64, u32)> = ts
        .windows(2)
        .rev()
        .filter_map(|w| match w {
            [a, b] => Some((*a, *b, 0)),
            _ => None,
        })
        .collect();
    let mut made = stack.len();
    let mut upper = 0.0_f64;
    while let Some((a, b, depth)) = stack.pop() {
        let mid = 0.5 * (a + b);
        // once the largest angle is shown above `below` (a station's lower bound, or a piece
        // that stopped above it), no cut can bring `upper` to `below`: stop cutting
        let above = lower > below || upper > below;
        let last = above || depth >= MAX_DEPTH || !(a < mid && mid < b) || made + 2 > MAX_PIECES;
        match piece_upper(&cn, &sides, a, b)? {
            Some(bound) if last || bound <= below => {
                upper = upper.max(bound);
                continue;
            }
            None if last => return Ok(None),
            _ => {}
        }
        made += 2;
        stack.push((mid, b, depth + 1));
        stack.push((a, mid, depth + 1));
    }
    let upper = upper.max(lower);
    Ok((upper.is_finite() && lower.is_finite()).then_some(SeamAngleEnclosure { lower, upper }))
}

/// The B-spline a curve is (a line's, a conic's base), `None` for any other kind.
#[must_use]
pub fn curve_b_spline(c: &SmCurve) -> Option<&BSplineCurve> {
    match c.kind() {
        CurveKind::BSpline(b) => Some(b),
        CurveKind::Line(l) => Some(&l.base),
        CurveKind::Ellipse(e) => Some(e.base()),
        CurveKind::Hyperbola(h) => Some(h.base()),
        CurveKind::Parabola(p) => Some(p.base()),
        _ => None,
    }
}

/// [`seam_angle_enclosure`] of edge `e` of `b` between its two faces' surfaces, along the
/// edge's curve over its interval.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for an edge that is not a seam of two faces, without a curve,
/// or a face without a surface; those of [`seam_angle_enclosure`].
pub fn edge_seam_angle_enclosure(
    k: &Kernel,
    b: &Brep,
    e: EdgeId,
    below: f64,
) -> Result<Option<SeamAngleEnclosure>> {
    let mut fs = crate::topology::edge::faces(b, e, None)?;
    fs.sort();
    fs.dedup();
    let &[fa, fb] = fs.as_slice() else {
        return Err(SolidError::invalid_input(
            OP,
            "an edge that is not a seam of two faces",
        ));
    };
    let surface = |f| -> Result<&Surface> {
        b.face(f)?
            .surface
            .as_ref()
            .ok_or_else(|| SolidError::invalid_input(OP, "face without a surface"))
    };
    let edge = b.edge(e)?;
    let curve = edge
        .curve
        .as_ref()
        .ok_or_else(|| SolidError::invalid_input(OP, "edge without a curve"))?;
    let Some(bs) = curve_b_spline(curve) else {
        return Ok(None);
    };
    seam_angle_enclosure(k, bs, edge.interval, [surface(fa)?, surface(fb)?], below)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Point3;
    use crate::nurbs::b_spline_surface::BSplineSurface;
    use crate::nurbs::math::{CPoint, NOW};
    use crate::nurbs::srf_basic::Surface as NurbSurface;

    /// The surface `z = y (a + b u^2)` over `u` in `[0, 1]` (`x = u`) and `y` in `[-1, 1]`:
    /// degree 2 in `u`, 1 in `v`, exact (the Bezier coefficients of `a + b u^2` are `a, a,
    /// a + b`). Along the x-axis its normal leans from `+z` by `atan(a + b x^2)`.
    fn leaning(a: f64, b: f64) -> Surface {
        let f = [a, a, a + b];
        let net: Vec<Vec<CPoint>> = (0..3)
            .map(|i| {
                [-1.0, 1.0]
                    .iter()
                    .map(|&y| CPoint::new(0.5 * i as f64, y, y * f[i], NOW))
                    .collect()
            })
            .collect();
        let s = NurbSurface::new(
            net,
            2,
            1,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        Surface::from(BSplineSurface::from_nurb(s, false))
    }

    fn x_axis() -> BSplineCurve {
        BSplineCurve::create_line_segment(3, Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0))
            .unwrap()
    }

    /// Two planes through the x-axis at a known angle: the enclosure holds it, tight (both
    /// planes are flat, so no piece adds anything past the stations' rounding).
    #[test]
    fn two_planes_enclose_their_angle() {
        let k = Kernel::default();
        let alpha = 1.0e-3_f64.atan();
        let (flat, tilted) = (leaning(0.0, 0.0), leaning(1.0e-3, 0.0));
        let e = seam_angle_enclosure(
            &k,
            &x_axis(),
            Extent1d::from_min_max(0.0, 1.0),
            [&flat, &tilted],
            alpha + 1.0e-12,
        )
        .unwrap()
        .unwrap();
        assert!(
            e.lower <= alpha && alpha <= e.upper,
            "{e:?} against {alpha:e}"
        );
        assert!(e.upper - e.lower < 1.0e-9, "{e:?}");
    }

    /// A plane and a surface whose normal leans by `atan(a + b x^2)` along the x-axis: the
    /// largest angle, `atan(a + b)` at `x = 1`, is enclosed, the upper bound within the
    /// requested `below` of it; and the bound holds the stations' angles between them.
    #[test]
    fn a_leaning_surface_encloses_its_largest_angle() {
        let k = Kernel::default();
        let (a, b) = (1.0e-3_f64, 2.0e-3_f64);
        let most = (a + b).atan();
        let (flat, lean) = (leaning(0.0, 0.0), leaning(a, b));
        let below = most + 1.0e-7;
        let e = seam_angle_enclosure(
            &k,
            &x_axis(),
            Extent1d::from_min_max(0.0, 1.0),
            [&flat, &lean],
            below,
        )
        .unwrap()
        .unwrap();
        assert!(e.lower <= most && most <= e.upper, "{e:?} against {most:e}");
        assert!(e.upper <= below, "{e:?} against {below:e}");
        // over half the curve the largest angle is at x = 1/2
        let half = (a + 0.25 * b).atan();
        let e = seam_angle_enclosure(
            &k,
            &x_axis(),
            Extent1d::from_min_max(0.0, 0.5),
            [&lean, &flat],
            half + 1.0e-7,
        )
        .unwrap()
        .unwrap();
        assert!(
            e.lower <= half && half <= e.upper && e.upper < most,
            "{e:?}"
        );
    }

    /// Faces at a right angle: the normals' dot product straddles 0, so no station's lower
    /// bound is proved (0) and the upper bound is the acute angle's largest, `π / 2`; a
    /// reversed interval is an input error.
    #[test]
    fn a_right_angle_is_held_by_the_largest_acute_angle() {
        let mut k = Kernel::default();
        let b = crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 1.0, 2.0, 3.0)
            .unwrap();
        let e = crate::query::api_queries::get_edges(&b).unwrap()[0];
        let right = std::f64::consts::FRAC_PI_2;
        assert_eq!(
            edge_seam_angle_enclosure(&k, &b, e, right).unwrap(),
            Some(SeamAngleEnclosure {
                lower: 0.0,
                upper: right
            })
        );
        let flat = leaning(0.0, 0.0);
        assert!(seam_angle_enclosure(
            &k,
            &x_axis(),
            Extent1d::from_raw(1.0, 0.0),
            [&flat, &flat],
            0.0,
        )
        .is_err());
    }
}
