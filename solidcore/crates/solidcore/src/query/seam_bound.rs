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
//! two sides' nearest points in their boxes (fact 1b narrows the feet to boxes of those).
//! `Θ` is therefore the angle at the **local** nearest point next to the correspondence,
//! not at the global nearest point nor at the point the seam check's own projection finds:
//! where a side has another local nearest point farther along the surface, `Θ` does not
//! see it. Next to a collapsed side (fact 3) the feet are those in a strip along the whole
//! side, which can hold a different local nearest point from fact 1's box; a station's
//! lower bound and a piece's upper bound proved by different facts can then be of
//! different local nearest points.
//!
//! **The proof.** Facts 1 and 2 (with 3 and 4 below), each enclosed in outward-rounded interval arithmetic
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
//!    *1b. The nearest point.* Where it is proved, the box `F` is narrowed to one that
//!    holds the nearest point (`nearest_box`): a zero of `J^T (S − C)` inside the domain, a
//!    zero of the one partial on a domain edge, or a domain corner, each enclosed by a
//!    Krawczyk step. Its width follows the curve piece, where `F`'s does not.
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
//! 3. *Near a side collapsed to a point* (a triangular corner patch's apex, where `σ` and
//!    `N` vanish and fact 1 cannot close): where the net's row along a side is one point
//!    `A` exactly (`NetSurface::collapsed_sides`), the feet lie in a strip along that side
//!    as deep as `|C − A| + G + slack` over the speed at which the surface leaves `A`, and the
//!    normal there is on the line of the fan's `S_st × S_t` (`apex_cones`, which proves
//!    both). It is tried only where facts 1 and 2 do not prove the cones: far from the
//!    point it does not close (the fan turns across the strip), so trying it everywhere
//!    costs every piece and station a strip of failing hulls.
//! 4. *Along the seam* (`seam_piece_upper`, a piece only): the two sides' normals at
//!    their correspondences, `f(t) = n_1(t) − σ n_2(t)`, enclosed as `f(m) + [−h, h] [f']`
//!    over the piece, plus each side's foot term `D ρ` (facts 1 and 2). The sides' turns
//!    along the seam cancel in `f'`, so long pieces close where the sides turn together,
//!    where facts 2 and 3's per-side cones need the turn per piece below the bound.
//!
//! A piece's bound is fact 4's where that is at most `below`, else the least of fact 4's
//! and the largest over its pairs of parts of the pair's upper angle plus their `δ`s (at
//! most `π / 2`, the acute angle's largest); a station's lower bound is the
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
//! Not enclosed (`None`): a side whose surface has no net or is a procedural blend (it
//! evaluates by its definition, its net only approximates it, and the facts bound the net), a sample with no nearest point, or
//! a piece at the deepest bisection with no bound (a box too far past a net's end, a knot
//! line of continuity below C^0, a zero normal or `σ` away from a collapsed side, a foot bound
//! that does not close, more than [`MAX_PARTS`] parts).
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
use crate::math::det;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::query::continuity::SEAM_PARTS;
use crate::query::curve_surface_gap::{correspondence, piece_bound as gap_piece, NetSurface};
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

/// One side prepared: its net, its correspondence and its sides collapsed to a point
/// (`NetSurface::collapsed_sides`).
struct Side {
    net: NetSurface,
    w: NurbsCurve,
    collapsed: Vec<(bool, bool, V3<f64>)>,
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
    round_up(2.0 * det::asin(half.max(0.0)) * (1.0 + ASIN_ROUNDING))
}

/// `2 asin(c / 2)` rounded down.
fn chord_angle_lo(c: f64) -> f64 {
    let half = round_down(0.5 * c);
    if half <= 0.0 {
        return 0.0;
    }
    round_down(2.0 * det::asin(half.min(1.0)) * (1.0 - ASIN_ROUNDING)).max(0.0)
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
    Ok(feet_reach(net, u, v, gap)?.map(|rho| (grow(u, rho), grow(v, rho))))
}

/// `a` widened by `by` each way, rounded outward.
fn grow(a: Ivl, by: f64) -> Ivl {
    if by == 0.0 {
        return a;
    }
    Ivl::new(round_down(a.lo - by), round_up(a.hi + by))
}

/// The foot bound `ρ` of fact 1: every foot at `t` is within `ρ` (Euclidean, in the
/// parameters) of `W(t)`, for a correspondence sweeping `u x v` with gap at most `gap`;
/// `None` where it does not close. 0 where the gap and slack are 0.
fn feet_reach(net: &NetSurface, u: Ivl, v: Ivl, gap: f64) -> Result<Option<f64>> {
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
        return Ok(Some(0.0));
    }
    let Some(s0) = sigma_lo(&p0) else {
        return Ok(None);
    };
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
            return Ok(Some(rho));
        }
        reach = round_up(2.0 * rho);
    }
    Ok(None)
}

/// The Krawczyk steps [`nearest_box`] takes at most.
pub const NEAREST_STEPS: u32 = 4;

/// The box of feet `(fu, fv)` (fact 1) narrowed to a box of the nearest point (fact 1b).
/// The nearest point `w` of the surface (over its domain `D`) to a curve point `C` in `c`
/// lies in `X = (fu, fv) ∩ D` (fact 1), away from the edges of the box `B` that are not
/// edges of `D`, so it minimises `|S − C|^2 / 2` over `B ∩ D` with only `D`'s edges active:
/// in each coordinate it is either inside `D`, where that partial of `F(w) = J(w)^T (S(w) −
/// C)` (`J = [S_u S_v]`) vanishes, or on an edge of `D`. So it is
/// - a zero of `F` in `X`, which lies in the Krawczyk box `K(X) = x − Y F(x) + (I − Y
///   [F'](X)) (X − x)` (`x` the midpoint, `Y` the inverse of `[F']`'s midpoint, `F' =
///   [[S_u·S_u + S_uu·g, S_u·S_v + S_uv·g], [., S_v·S_v + S_vv·g]]`, `g = S − C`; the mean
///   value theorem, `F` Lipschitz where the net is C^1);
/// - or, on an edge of `D` that `X` reaches, a zero of the edge's one partial in the
///   one-dimensional Krawczyk interval of that edge's part in `X`;
/// - or a corner of `D` in `X`.
///
/// The hull of those, intersected with `X`, is the next `X`, at most [`NEAREST_STEPS`]
/// times while a side halves. `F(x)` over `c` is as wide as the curve piece, so the box
/// shrinks with the piece where `W ± ρ` does not. The box is kept as it was where a step is
/// not proved (a box past the net's end, a knot line across which the net is not C^1, a
/// singular midpoint). The bounds then hold at the nearest point, the point the seam check
/// reads (module docs), not at every foot.
fn nearest_box(net: &NetSurface, (fu, fv): (Ivl, Ivl), c: V3<Ivl>) -> Result<(Ivl, Ivl)> {
    let (du, dv) = net.domain();
    let (Some(u), Some(v)) = (meet(fu, du), meet(fv, dv)) else {
        return Ok((fu, fv));
    };
    let mut x = (u, v);
    for _ in 0..NEAREST_STEPS {
        let Some(next) = nearest_step(net, x, (du, dv), c)? else {
            break;
        };
        let shrunk = next.0.hi - next.0.lo < 0.5 * (x.0.hi - x.0.lo)
            || next.1.hi - next.1.lo < 0.5 * (x.1.hi - x.1.lo);
        x = next;
        if !shrunk {
            break;
        }
    }
    Ok(x)
}

/// `a ∩ b`, `None` where empty.
fn meet(a: Ivl, b: Ivl) -> Option<Ivl> {
    let (lo, hi) = (a.lo.max(b.lo), a.hi.min(b.hi));
    (lo <= hi).then(|| Ivl::new(lo, hi))
}

/// One step of [`nearest_box`]: the hull of the nearest point's candidates in `x`, `None`
/// where a candidate set is not proved (or none is left, which fact 1 rules out).
fn nearest_step(
    net: &NetSurface,
    x: (Ivl, Ivl),
    (du, dv): (Ivl, Ivl),
    c: V3<Ivl>,
) -> Result<Option<(Ivl, Ivl)>> {
    let mut hull: Option<(Ivl, Ivl)> = None;
    let mut add = |b: (Ivl, Ivl)| {
        hull = Some(match hull {
            Some(h) => (h.0.hull(b.0), h.1.hull(b.1)),
            None => b,
        });
    };
    // inside D
    let Some(k) = krawczyk(net, x, c)? else {
        return Ok(None);
    };
    if let (Some(u), Some(v)) = (meet(k.0, x.0), meet(k.1, x.1)) {
        add((u, v));
    }
    // on an edge of D that the box reaches
    for (pinned_u, at) in [(true, du.lo), (true, du.hi), (false, dv.lo), (false, dv.hi)] {
        let (pin, free) = if pinned_u { (x.0, x.1) } else { (x.1, x.0) };
        if !(pin.lo <= at && at <= pin.hi) {
            continue;
        }
        let Some(k) = krawczyk_edge(net, pinned_u, at, free, c)? else {
            return Ok(None);
        };
        if let Some(s) = meet(k, free) {
            add(if pinned_u {
                (Ivl::from(at), s)
            } else {
                (s, Ivl::from(at))
            });
        }
    }
    // at a corner of D
    for u in [du.lo, du.hi] {
        for v in [dv.lo, dv.hi] {
            if x.0.lo <= u && u <= x.0.hi && x.1.lo <= v && v <= x.1.hi {
                add((Ivl::from(u), Ivl::from(v)));
            }
        }
    }
    Ok(hull)
}

/// The one-dimensional Krawczyk interval of [`nearest_step`] on the edge of `D` at `at` in
/// u (`pinned_u`) or v, over `free` in the other parameter: every zero there of `f(s) = S_s ·
/// (S − C)` lies in `m − y f(m) + (1 − y [f'](free)) (free − m)`, `f' = S_s · S_s + S_ss ·
/// (S − C)`, `y = 1 / mid [f']`. `None` where not proved.
fn krawczyk_edge(
    net: &NetSurface,
    pinned_u: bool,
    at: f64,
    free: Ivl,
    c: V3<Ivl>,
) -> Result<Option<Ivl>> {
    let box_of = |s: Ivl| {
        if pinned_u {
            (Ivl::from(at), s)
        } else {
            (s, Ivl::from(at))
        }
    };
    let b = box_of(free);
    let (lu, lv) = net.low_lines(b.0, b.1);
    if !(lu.is_empty() && lv.is_empty()) {
        return Ok(None);
    }
    let Some((p, slack)) = partials_over(net, b.0, b.1, 2)? else {
        return Ok(None);
    };
    let m = free.mid();
    let bm = box_of(Ivl::from(m));
    let Some((q, qslack)) = partials_over(net, bm.0, bm.1, 1)? else {
        return Ok(None);
    };
    if slack != 0.0 || qslack != 0.0 {
        return Ok(None);
    }
    let ((i1, j1), (i2, j2)) = if pinned_u {
        ((0, 1), (0, 2))
    } else {
        ((1, 0), (2, 0))
    };
    let f = v_dot(q.at(i1, j1), v_sub(q.at(0, 0), c));
    let s1 = p.at(i1, j1);
    let fp = v_dot(s1, s1) + v_dot(p.at(i2, j2), v_sub(p.at(0, 0), c));
    let ym = fp.mid();
    if !(ym.is_finite() && ym != 0.0) {
        return Ok(None);
    }
    let y = 1.0 / ym;
    if !y.is_finite() {
        return Ok(None);
    }
    let y = Ivl::from(y);
    let k = Ivl::from(m) - y * f + (Ivl::from(1.0) - y * fp) * (free - Ivl::from(m));
    Ok((k.lo.is_finite() && k.hi.is_finite()).then_some(k))
}

/// One Krawczyk box of [`nearest_box`], `None` where it is not proved.
fn krawczyk(net: &NetSurface, (xu, xv): (Ivl, Ivl), c: V3<Ivl>) -> Result<Option<(Ivl, Ivl)>> {
    let (lu, lv) = net.low_lines(xu, xv);
    if !(lu.is_empty() && lv.is_empty()) {
        return Ok(None);
    }
    let Some((p, slack)) = partials_over(net, xu, xv, 2)? else {
        return Ok(None);
    };
    if slack != 0.0 {
        return Ok(None);
    }
    let (mu, mv) = (xu.mid(), xv.mid());
    let Some((q, qslack)) = partials_over(net, Ivl::from(mu), Ivl::from(mv), 1)? else {
        return Ok(None);
    };
    if qslack != 0.0 {
        return Ok(None);
    }
    // F at the midpoint, over the curve points
    let g = v_sub(q.at(0, 0), c);
    let f = [v_dot(q.at(1, 0), g), v_dot(q.at(0, 1), g)];
    // [F'] over the box
    let gx = v_sub(p.at(0, 0), c);
    let (su, sv) = (p.at(1, 0), p.at(0, 1));
    let a = v_dot(su, su) + v_dot(p.at(2, 0), gx);
    let b = v_dot(su, sv) + v_dot(p.at(1, 1), gx);
    let d = v_dot(sv, sv) + v_dot(p.at(0, 2), gx);
    let jac = [[a, b], [b, d]];
    // Y: the midpoint's inverse (any matrix is valid; this one makes K tight)
    let (am, bm, dm) = (a.mid(), b.mid(), d.mid());
    let det = am * dm - bm * bm;
    if !(det.is_finite() && det.abs() > 0.0) {
        return Ok(None);
    }
    let y = [[dm / det, -bm / det], [-bm / det, am / det]];
    if !y.iter().flatten().all(|x| x.is_finite()) {
        return Ok(None);
    }
    // K = mid - Y F(mid) + (I - Y [F']) (X - mid), row by row
    let [[y00, y01], [y10, y11]] = y.map(|r| r.map(Ivl::from));
    let [[j00, j01], [j10, j11]] = jac;
    let [f0, f1] = f;
    let (dxu, dxv) = (xu - Ivl::from(mu), xv - Ivl::from(mv));
    let one = Ivl::from(1.0);
    let row = |at: f64, ya: Ivl, yb: Ivl, ea: Ivl, eb: Ivl| {
        let m0 = ea - (ya * j00 + yb * j10);
        let m1 = eb - (ya * j01 + yb * j11);
        Ivl::from(at) - (ya * f0 + yb * f1) + m0 * dxu + m1 * dxv
    };
    let ku = row(mu, y00, y01, one, Ivl::from(0.0));
    let kv = row(mv, y10, y11, Ivl::from(0.0), one);
    if !([ku.lo, ku.hi, kv.lo, kv.hi].iter().all(|x| x.is_finite())) {
        return Ok(None);
    }
    Ok(Some((ku, kv)))
}

/// `[lo, hi]` cut at `lines` (sorted, strictly inside).
fn cut(a: Ivl, lines: &[f64]) -> Vec<Ivl> {
    let mut out = Vec::new();
    let mut from = a.lo;
    for &x in lines {
        out.push(Ivl::new(from, x));
        from = x;
    }
    out.push(Ivl::new(from, a.hi));
    out
}

/// The cones of side `side` over the feet of the correspondence sweeping `u x v` with gap
/// at most `gap` (facts 1 and 2), `None` where they are not proved. Where those are not
/// proved and the net has a side collapsed to a point, the feet near that point (fact 3,
/// [`apex_cones`]; `c` encloses the curve's points).
fn cones(side: &Side, u: Ivl, v: Ivl, gap: f64, c: V3<Ivl>) -> Result<Option<Vec<Cone>>> {
    let net = &side.net;
    if let Some(regular) = regular_cones(net, u, v, gap, c)? {
        return Ok(Some(regular));
    }
    for &(across_u, at_max, apex) in &side.collapsed {
        if let Some(out) = apex_cones(net, (across_u, at_max, apex), (u, v), gap, c)? {
            return Ok(Some(out));
        }
    }
    Ok(None)
}

/// [`cones`] by facts 1 and 2 alone.
fn regular_cones(
    net: &NetSurface,
    u: Ivl,
    v: Ivl,
    gap: f64,
    c: V3<Ivl>,
) -> Result<Option<Vec<Cone>>> {
    let Some((fu, fv)) = feet_box(net, u, v, gap)? else {
        return Ok(None);
    };
    let (fu, fv) = nearest_box(net, (fu, fv), c)?;
    let (lu, lv) = net.low_lines(fu, fv);
    let (us, vs) = (cut(fu, &lu), cut(fv, &lv));
    if us.len().saturating_mul(vs.len()) > MAX_PARTS {
        return Ok(None);
    }
    let mut out = Vec::new();
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

/// The parts the along-side parameter of a collapsed side's strip is cut into (fact 3), at
/// least: the fan's normal turns along it.
pub const APEX_PARTS: usize = 32;

/// `s x t` as the box `(u, v)`: `across_u` puts the across parameter `t` in u.
fn uv_of(across_u: bool, s: Ivl, t: Ivl) -> (Ivl, Ivl) {
    if across_u {
        (t, s)
    } else {
        (s, t)
    }
}

/// The hull of the partials up to second order over `s x t` (`uv_of`), `t` cut at the knot
/// lines it crosses where the net is not C^1; `None` where the net refuses a part.
fn partials_hull_over(
    net: &NetSurface,
    across_u: bool,
    s: Ivl,
    t: Ivl,
) -> Result<Option<Partials<Ivl>>> {
    let (bu, bv) = uv_of(across_u, s, t);
    let (lu, lv) = net.low_lines(bu, bv);
    let mut out: Option<Partials<Ivl>> = None;
    for pu in cut(bu, &lu) {
        for pv in cut(bv, &lv) {
            let Some((p, slack)) = partials_over(net, pu, pv, 2)? else {
                return Ok(None);
            };
            if slack != 0.0 {
                return Ok(None);
            }
            out = Some(match out {
                None => p,
                Some(h) => crate::blend::fillet_section::certificate::partials_hull(h, &p, 2),
            });
        }
    }
    Ok(out)
}

/// The cones over the feet near a side of the net collapsed to the point `apex` (fact 3;
/// `side`: `(across_u, at_max, apex)` as `NetSurface::collapsed_sides`), for a
/// correspondence sweeping `w` (a `(u, v)` box) with gap at most `gap` and curve points in
/// `c`; `None` where they are not proved.
///
/// Write `s` for the parameter along the collapsed side and `t` across it, `t_e` its value
/// there. `S(s, t_e) = A` for every `s`, so `S_s(s, t_e) = 0` and, along each line of
/// constant `s`, `S_s(s, t) = (t - t_e) Ā` with `Ā` the mean of `S_st` between `t_e` and
/// `t`, in the interval hull of `S_st` over any box holding that segment. The normal `N =
/// S_u x S_v` is `±(t - t_e) Ā x S_t`: off the side, on the line of `M = Ā x S_t`. So over a
/// part `P` of the feet, the normal's line is that of some `M` in `[S_st] x [S_t]` (the hulls
/// over `P` grown to `t_e`), within `δ = atan(|E_⊥| / (|m| - |E_∥|))` of `m`, the box's
/// midpoint, for `E = M - m` split along `m` (`E_∥`) and across it (`E_⊥`), where `|E_∥| <
/// |m|`. On the collapsed side itself (`t =
/// t_e`, where `N` is zero) the normal is taken as its limit, the line of `M` there.
///
/// The feet: a foot is no farther from a curve point `C` than the surface at the clamped
/// correspondence, within `gap` plus the slack of a correspondence past the net's end (as
/// in fact 1), so within `r = |C - A| + gap + slack` of `A`. The side is cut into parts `P` (as for the cones). With `e` the midpoint of
/// `[S_t]` over `P x (t_e to t_e ± η)` (a strip of the net along the side), a foot at `s` in
/// `P` has `|e · (S - A)| = |∫ e · S_t| >= |t - t_e| κ` with `κ > 0` the least of `e · S_t`
/// over `P`'s strip (the integral runs between `t_e` and `t` at that `s`). So `|S - A| >=
/// |t - t_e| κ / |e|` and the feet in the strip are within `η_f`, the largest over the parts
/// of `r |e| / κ`, of the side (each part its own `e`: the side's fan turns along it). Where `η_f < η` and the correspondence
/// is in the strip, the feet (including the nearest point in the strip) are within `η_f`,
/// not on the strip's far edge; the strip is regrown to `2 η_f` where `η_f >= η`, at most
/// [`FOOT_GROWTHS`] strips.
fn apex_cones(
    net: &NetSurface,
    (across_u, at_max, apex): (bool, bool, V3<f64>),
    (wu, wv): (Ivl, Ivl),
    gap: f64,
    c: V3<Ivl>,
) -> Result<Option<Vec<Cone>>> {
    if !(gap.is_finite() && gap >= 0.0) {
        return Ok(None);
    }
    // the slack of a correspondence past the net's end, as fact 1 adds it: `gap` bounds
    // the curve's distance from the continued net at `W`, and the surface at the clamped
    // point is within the slack of that (`NetSurface::partials`)
    let Some((_, slack)) = partials_over(net, wu, wv, 1)? else {
        return Ok(None);
    };
    let (du, dv) = net.domain();
    let (along, across, w_across) = if across_u { (dv, du, wu) } else { (du, dv, wv) };
    let edge = if at_max { across.hi } else { across.lo };
    // (a correspondence a rounding past the net's end: its clamped point, whose distance
    // from the curve the gap bounds with the slack)
    let w_across = Ivl::new(w_across.lo.max(across.lo), w_across.hi.min(across.hi));
    if w_across.lo > w_across.hi {
        return Ok(None);
    }
    // how far the correspondence is from the side, across
    let w_off = if at_max {
        round_up(edge - w_across.lo)
    } else {
        round_up(w_across.hi - edge)
    };
    if !(w_off.is_finite() && w_across.lo >= across.lo && w_across.hi <= across.hi) {
        return Ok(None);
    }
    let a = V3 {
        x: Ivl::from(apex.x),
        y: Ivl::from(apex.y),
        z: Ivl::from(apex.z),
    };
    let Some(ca) = norm_hi(v_sub(c, a)) else {
        return Ok(None);
    };
    let r = round_up(round_up(ca + gap) + slack);
    if !r.is_finite() {
        return Ok(None);
    }
    let width = round_up(across.hi - across.lo);
    // the strip from the side, `eta` deep
    let strip = |eta: f64| {
        if at_max {
            Ivl::new(round_down(edge - eta).max(across.lo), edge)
        } else {
            Ivl::new(edge, round_up(edge + eta).min(across.hi))
        }
    };
    // the parts: the side's parameter cut into APEX_PARTS and at its low lines
    let (ls, _) = {
        let (bu, bv) = uv_of(across_u, along, across);
        let (lu, lv) = net.low_lines(bu, bv);
        if across_u {
            (lv, lu)
        } else {
            (lu, lv)
        }
    };
    let mut lines: Vec<f64> = (1..APEX_PARTS)
        .map(|i| along.lo + (along.hi - along.lo) * (i as f64) / (APEX_PARTS as f64))
        .filter(|x| *x > along.lo && *x < along.hi)
        .collect();
    lines.extend(ls);
    lines.sort_by(f64::total_cmp);
    lines.dedup();
    let parts = cut(along, &lines);
    let mut eta = round_up(2.0 * w_off).max(round_up(width * 1.0e-6));
    let mut feet: Option<f64> = None;
    'grow: for _ in 0..FOOT_GROWTHS {
        if !(eta.is_finite() && eta > 0.0) || eta < w_off {
            return Ok(None);
        }
        let eta_c = eta.min(width);
        // the deepest a foot can be, over the parts of the side (each its own `e`)
        let mut need = 0.0_f64;
        for &ps in &parts {
            let Some(p) = partials_hull_over(net, across_u, ps, strip(eta_c))? else {
                return Ok(None);
            };
            let st = if across_u { p.at(1, 0) } else { p.at(0, 1) };
            let e = V3 {
                x: Ivl::from(st.x.mid()),
                y: Ivl::from(st.y.mid()),
                z: Ivl::from(st.z.mid()),
            };
            let kappa = v_dot(e, st).lo;
            let Some(e_len) = norm_hi(e) else {
                return Ok(None);
            };
            if !(kappa.is_finite() && kappa > 0.0) {
                return Ok(None);
            }
            let part = (Ivl::from(r) * Ivl::from(e_len) / Ivl::from(kappa)).hi;
            if !part.is_finite() {
                return Ok(None);
            }
            need = need.max(part);
        }
        if need < eta_c || (eta_c == width && need <= width) {
            feet = Some(need.min(width));
            break 'grow;
        }
        eta = round_up(2.0 * need);
    }
    let Some(depth) = feet else {
        return Ok(None);
    };
    let fstrip = strip(depth);
    let mut out = Vec::new();
    for &ps in &parts {
        let Some(p) = partials_hull_over(net, across_u, ps, fstrip)? else {
            return Ok(None);
        };
        let (st, s_t) = if across_u {
            (p.at(1, 1), p.at(1, 0))
        } else {
            (p.at(1, 1), p.at(0, 1))
        };
        let m_box = v_cross(st, s_t);
        if !finite(m_box) {
            return Ok(None);
        }
        let mid = |x: Ivl| Ivl::from(x.mid());
        let m = V3 {
            x: mid(m_box.x),
            y: mid(m_box.y),
            z: mid(m_box.z),
        };
        let (Some(m_lo), Some(m_hi)) = (norm_lo(m), norm_hi(m)) else {
            return Ok(None);
        };
        if !(m_lo.is_finite() && m_lo > 0.0) {
            return Ok(None);
        }
        let n = v_scale(m, Ivl::from(1.0) / Ivl::new(m_lo, m_hi));
        // M = m + E: the angle from m is atan(|E_perp| / |m + E_par|), E_par = E . n̂
        let e = v_sub(m_box, m);
        let along_m = v_dot(e, n);
        let e_perp = v_sub(e, v_scale(n, along_m));
        let Some(perp) = norm_hi(e_perp) else {
            return Ok(None);
        };
        let base = round_down(m_lo - along_m.mag());
        if !(base.is_finite() && base > 0.0) {
            return Ok(None);
        }
        let tan = (Ivl::from(perp) / Ivl::from(base)).hi;
        if !tan.is_finite() {
            return Ok(None);
        }
        let delta = round_up(det::atan(tan) * (1.0 + ASIN_ROUNDING));
        out.push(Cone { n, delta });
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
        let Some(c) = derivs(cn, t)? else {
            return Ok(None);
        };
        let gap = if t.lo == t.hi {
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
        let Some(cs) = cones(side, w.x, w.y, gap, c)? else {
            return Ok(None);
        };
        *slot = cs;
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
/// none is: fact 4's ([`seam_piece_upper`]) where it is at most `enough`, else the least of
/// it and facts 1 to 3's ([`cone_piece_upper`]).
fn piece_upper(
    cn: &NurbsCurve,
    sides: &[Side; 2],
    a: f64,
    b: f64,
    enough: f64,
) -> Result<Option<f64>> {
    let along = seam_piece_upper(cn, sides, a, b)?;
    if let Some(x) = along {
        if x <= enough {
            return Ok(Some(x));
        }
    }
    Ok(match (along, cone_piece_upper(cn, sides, a, b)?) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, y) => x.or(y),
    })
}

/// [`piece_upper`] by facts 1 to 3 (each side's cones), `None` where not proved.
fn cone_piece_upper(cn: &NurbsCurve, sides: &[Side; 2], a: f64, b: f64) -> Result<Option<f64>> {
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

/// One side's terms of fact 4 over a piece: the unit normal at `W(m)`, the enclosure of
/// `d/dt n(W(t))` over the piece, and the angle `A` within which every foot's normal lies
/// of `n(W(t))` at the same `t`.
struct SeamSide {
    n_mid: V3<Ivl>,
    n_dot: V3<Ivl>,
    feet: f64,
}

/// The unit normal over the enclosure `p` and its derivative along `w'` (`(u', v')`):
/// `n = N / |N|`, `d/dt n = n_u u' + n_v v'`, `n_u = (N_u − n (n · N_u)) / |N|` (likewise
/// `n_v`); `None` where `|N|` is not proved positive.
fn unit_normal(p: &Partials<Ivl>) -> Option<(V3<Ivl>, Ivl)> {
    let nn = v_cross(p.at(1, 0), p.at(0, 1));
    let (lo, hi) = (norm_lo(nn)?, norm_hi(nn)?);
    if !(lo.is_finite() && lo > 0.0) {
        return None;
    }
    let len = Ivl::new(lo, hi);
    Some((v_scale(nn, Ivl::from(1.0) / len), len))
}

/// Fact 4's terms of `side` over the piece `t` with midpoint `m` (gap at most `gap` over
/// the piece), `None` where one is not proved.
fn seam_side(side: &Side, t: Ivl, m: f64, gap: f64) -> Result<Option<SeamSide>> {
    let net = &side.net;
    let wd = match curve_derivs_over(&side.w, t, 1, None) {
        Ok(d) => d,
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        Err(_) => return Ok(None),
    };
    let (Some(w0), Some(w1)) = (wd.first().copied(), wd.get(1).copied()) else {
        return Ok(None);
    };
    // the feet: within rho of W(t) (fact 1), their normals within D rho of n(W(t))
    let Some(rho) = feet_reach(net, w0.x, w0.y, gap)? else {
        return Ok(None);
    };
    let feet = if rho == 0.0 {
        0.0
    } else {
        let Some((p, _)) = partials_over(net, grow(w0.x, rho), grow(w0.y, rho), 2)? else {
            return Ok(None);
        };
        let Some(d) = normal_lipschitz(&p) else {
            return Ok(None);
        };
        chord_angle_hi(round_up(d * rho))
    };
    // d/dt n(W(t)) over the piece
    let Some((p, _)) = partials_over(net, w0.x, w0.y, 2)? else {
        return Ok(None);
    };
    let Some((n, len)) = unit_normal(&p) else {
        return Ok(None);
    };
    let (su, sv) = (p.at(1, 0), p.at(0, 1));
    let nu = v_add(v_cross(p.at(2, 0), sv), v_cross(su, p.at(1, 1)));
    let nv = v_add(v_cross(p.at(1, 1), sv), v_cross(su, p.at(0, 2)));
    let off = |d: V3<Ivl>| v_scale(v_sub(d, v_scale(n, v_dot(n, d))), Ivl::from(1.0) / len);
    let n_dot = v_add(v_scale(off(nu), w1.x), v_scale(off(nv), w1.y));
    // n(W(m))
    let Some(wm) = derivs(&side.w, Ivl::from(m))? else {
        return Ok(None);
    };
    let Some((q, _)) = partials_over(net, wm.x, wm.y, 1)? else {
        return Ok(None);
    };
    let Some((n_mid, _)) = unit_normal(&q) else {
        return Ok(None);
    };
    if !(finite(n_dot) && finite(n_mid) && feet.is_finite()) {
        return Ok(None);
    }
    Ok(Some(SeamSide { n_mid, n_dot, feet }))
}

/// The proved upper bound of the angle at the feet over the piece `[a, b]` by fact 4, `None`
/// where it is not proved (a side collapsed near the piece, a knot line the net is not C^1
/// across, a foot bound that does not close).
///
/// *4. Along the seam.* Write `n_s(t)` for the unit normal of side `s` at `W_s(t)` and
/// `f(t) = n_1(t) − σ n_2(t)` for a fixed sign `σ` (that of `n_1 · n_2` at the midpoint
/// `m`; any sign bounds the acute angle). The acute angle at the feet at `t` is at most
/// `A_1 + 2 asin(|f(t)| / 2) + A_2` (a metric on lines): `A_s` the angle within which a
/// foot's normal lies of `n_s(t)`, `D ρ` over `W_s(T) ± ρ` (fact 1's `ρ` is a Euclidean
/// bound on `|w − W_s(t)|`, fact 2's `D` the normal's Lipschitz bound; the segment between
/// them is in the box). `f` is Lipschitz on the piece (the net C^1 across every knot line
/// `W_s(T)` crosses), so each coordinate of `f(t) − f(m)` is `(t − m)` times a value of
/// that coordinate of `f'` on the piece: `f(t)` lies in `f(m) + [−h, h] [f']`, `h` the
/// piece's half-length, `[f']` enclosing `n_1' − σ n_2'` with `n_s' = n_u u' + n_v v'`.
/// Unlike fact 2's cones, whose `δ` is the normal's turn over the whole piece on each side,
/// the two sides' turns along the seam cancel in `f'`, so a piece closes once its
/// second-order term is small: long pieces close where the sides turn together.
fn seam_piece_upper(cn: &NurbsCurve, sides: &[Side; 2], a: f64, b: f64) -> Result<Option<f64>> {
    let t = Ivl::new(a, b);
    let m = 0.5 * (a + b);
    if !(a < m && m < b) {
        return Ok(None);
    }
    let mut out: Vec<SeamSide> = Vec::with_capacity(2);
    for side in sides {
        let gap = match gap_piece(cn, &side.w, &side.net, a, b) {
            Ok(Some((ends, term))) => round_up(ends + term),
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Ok(None) | Err(_) => return Ok(None),
        };
        let Some(s) = seam_side(side, t, m, gap)? else {
            return Ok(None);
        };
        out.push(s);
    }
    let [s1, s2] = out.as_slice() else {
        return Ok(None);
    };
    let sign = if v_dot(s1.n_mid, s2.n_mid).mid() < 0.0 {
        Ivl::from(-1.0)
    } else {
        Ivl::from(1.0)
    };
    let f_mid = v_sub(s1.n_mid, v_scale(s2.n_mid, sign));
    let f_dot = v_sub(s1.n_dot, v_scale(s2.n_dot, sign));
    let h = round_up((b - m).max(m - a));
    let f = v_add(f_mid, v_scale(f_dot, Ivl::new(-h, h)));
    let Some(chord) = norm_hi(f) else {
        return Ok(None);
    };
    let upper = round_up(round_up(chord_angle_hi(chord) + s1.feet) + s2.feet);
    Ok(upper
        .is_finite()
        .then_some(upper.min(std::f64::consts::FRAC_PI_2)))
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
        // a procedural blend evaluates by its definition, and its net is only that
        // definition's approximation: the facts bound the net, so nothing is proved of it
        if matches!(
            s.kind(),
            crate::surface::surface::SurfaceKind::ProceduralBlend(_)
        ) {
            return Ok(None);
        }
        let s = s.clone_for_evaluation()?;
        let domain = s.natural_uv_domain_with_nurb(k)?;
        let Some((net, w)) = correspondence(k, &s, &domain, curve, [lo, hi])? else {
            return Ok(None);
        };
        let Ok(w) = NurbsCurve::of(&w) else {
            return Ok(None);
        };
        let collapsed = net.collapsed_sides();
        prepared.push(Side { net, w, collapsed });
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
        match piece_upper(&cn, &sides, a, b, below)? {
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

/// The starting pieces of [`seam_angle_within`]: even pieces of the interval.
pub const WITHIN_PIECES: usize = 16;
/// The deepest bisection of one of [`seam_angle_within`]'s starting pieces: its deepest
/// pieces are as short as [`seam_angle_enclosure`]'s ([`MAX_DEPTH`] below a station gap,
/// 1/256 of the interval or less).
pub const WITHIN_DEPTH: u32 = MAX_DEPTH + 5;

/// Whether the largest acute normal angle between `sides` along `curve` over `interval`
/// (module docs) is proved at most `limit`: every piece of the interval has a proved upper
/// bound at most `limit`, pieces bisected (from [`WITHIN_PIECES`], at most [`WITHIN_DEPTH`]
/// deep) while theirs is above it or not proved. At most `budget` piece bounds are computed;
/// `false` (not proved) once more would be needed, where a piece at the deepest bisection
/// is above `limit` or not bounded, or where the sides are not prepared. Unlike
/// [`seam_angle_enclosure`] no lower bound is computed: the caller only asks whether the
/// angle is within `limit`, so `false` says nothing about the angle.
///
/// # Errors
///
/// Those of [`seam_angle_enclosure`].
pub fn seam_angle_within(
    k: &Kernel,
    curve: &BSplineCurve,
    interval: Extent1d,
    sides: [&Surface; 2],
    limit: f64,
    budget: usize,
) -> Result<bool> {
    let Some((cn, sides)) = prepare(k, curve, interval, sides)? else {
        return Ok(false);
    };
    let ends: Vec<f64> = (0..=WITHIN_PIECES)
        .map(|i| {
            if i == WITHIN_PIECES {
                interval.max()
            } else {
                interval.evaluate(i as f64 / WITHIN_PIECES as f64)
            }
        })
        .collect();
    let mut stack: Vec<(f64, f64, u32)> = ends
        .windows(2)
        .rev()
        .filter_map(|w| match w {
            [a, b] if a < b => Some((*a, *b, 0)),
            _ => None,
        })
        .collect();
    let mut spent = 0_usize;
    while let Some((a, b, depth)) = stack.pop() {
        if spent >= budget {
            return Ok(false);
        }
        spent += 1;
        if let Some(bound) = piece_upper(&cn, &sides, a, b, limit)? {
            if bound <= limit {
                continue;
            }
        }
        let mid = 0.5 * (a + b);
        if depth >= WITHIN_DEPTH || !(a < mid && mid < b) {
            return Ok(false);
        }
        stack.push((mid, b, depth + 1));
        stack.push((a, mid, depth + 1));
    }
    Ok(true)
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
        let alpha = det::atan(1.0e-3_f64);
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
        let most = det::atan(a + b);
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
        let half = det::atan(a + 0.25 * b);
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

    /// Two surfaces whose normals turn together along the x-axis, `atan(b x^2)` and
    /// `atan(e + b x^2)` from `+z` (`b = 0.5`, `e = 1e-3`): the angle between them is at
    /// most `atan(e)`. Fact 2's cones turn by about `b` times a piece's length on each side,
    /// so a piece of 1/64 of the seam has no cone bound within the limit (pieces of 1/256
    /// are still at 5e-3, five times it); fact 4's turns cancel, so the whole seam is proved
    /// within it with at most 256 piece bounds (pieces of 1/64 or longer).
    #[test]
    fn surfaces_turning_together_are_proved_in_long_pieces() {
        let k = Kernel::default();
        let (b, e) = (0.5_f64, 1.0e-3_f64);
        let (one, two) = (leaning(0.0, b), leaning(e, b));
        let limit = det::atan(e) * 1.2;
        let interval = Extent1d::from_min_max(0.0, 1.0);
        assert!(seam_angle_within(
            &k,
            &x_axis(),
            interval,
            [&one, &two],
            limit,
            16 * WITHIN_PIECES
        )
        .unwrap());
        let (cn, sides) = prepare(&k, &x_axis(), interval, [&one, &two])
            .unwrap()
            .unwrap();
        let (a, z) = (0.5, 0.5 + 1.0 / 64.0);
        let cone = cone_piece_upper(&cn, &sides, a, z).unwrap().unwrap();
        let along = seam_piece_upper(&cn, &sides, a, z).unwrap().unwrap();
        assert!(
            cone > limit && along <= limit,
            "{cone:e} {along:e} {limit:e}"
        );
        // and nothing below the angle is proved: the seam reaches atan(e) at x = 0
        assert!(!seam_angle_within(
            &k,
            &x_axis(),
            interval,
            [&one, &two],
            det::atan(e) * (1.0 - 1.0e-6),
            1 << 12
        )
        .unwrap());
    }

    /// A triangle in the plane `z = a y` as a bilinear net whose last row (v = 1) is the apex
    /// `(1, 0, 0)`, against the plane `z = 0`, along the x-axis from `(0, 0, 0)` to the
    /// apex. Near the apex the patch's `σ` vanishes, so fact 1 cannot close there: the
    /// collapsed side's fan (fact 3) encloses the feet and their normal, and the angle,
    /// `atan(a)` everywhere, is enclosed over the whole side (the fan's parts add `a / 64`:
    /// interval cross products of vectors in the plane leave that much across it).
    #[test]
    fn a_triangle_is_enclosed_up_to_its_apex() {
        let k = Kernel::default();
        let a = 2.0e-3_f64;
        let apex = CPoint::new(1.0, 0.0, 0.0, NOW);
        let net = vec![
            vec![CPoint::new(0.0, 0.0, 0.0, NOW), apex],
            vec![CPoint::new(0.0, 1.0, a, NOW), apex],
        ];
        let s = NurbSurface::new(
            net,
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        let triangle = Surface::from(BSplineSurface::from_nurb(s, false));
        let flat = leaning(0.0, 0.0);
        let angle = det::atan(a);
        let e = seam_angle_enclosure(
            &k,
            &x_axis(),
            Extent1d::from_min_max(0.0, 1.0),
            [&triangle, &flat],
            angle + 1.0e-4,
        )
        .unwrap()
        .unwrap();
        assert!(
            e.lower <= angle && angle <= e.upper,
            "{e:?} against {angle:e}"
        );
        assert!(e.upper <= angle + 1.0e-4, "{e:?}");
    }

    /// Fact 1b narrows a box of feet to the nearest point's, inside the domain and on its
    /// edge: on the plane `z = 0` (`x = u`, `y = 2 v − 1`), the nearest point to `(0.3, 0.3,
    /// 1e-3)` is `(0.3, 0.65)`, a zero of `J^T (S − C)`; to `(−1e-3, 0.3, 0)`, past the edge
    /// `u = 0`, it is `(0, 0.65)`, where `F_u ≠ 0` (an unconstrained Krawczyk box would drop
    /// it). Each narrowed box holds the point and is far narrower than the box it started
    /// from.
    #[test]
    fn the_feet_narrow_to_the_nearest_point_inside_and_on_an_edge() {
        let k = Kernel::default();
        let probe = [crate::math::Point2::new(0.5, 0.5)];
        let net = NetSurface::of(&k, &leaning(0.0, 0.0), &probe)
            .unwrap()
            .unwrap();
        let point = |x: f64, y: f64, z: f64| V3 {
            x: Ivl::from(x),
            y: Ivl::from(y),
            z: Ivl::from(z),
        };
        let start = (Ivl::new(0.25, 0.35), Ivl::new(0.6, 0.7));
        let (u, v) = nearest_box(&net, start, point(0.3, 0.3, 1.0e-3)).unwrap();
        assert!(
            u.lo <= 0.3 && 0.3 <= u.hi && v.lo <= 0.65 && 0.65 <= v.hi,
            "{u:?} {v:?}"
        );
        assert!(u.hi - u.lo < 1.0e-9 && v.hi - v.lo < 1.0e-9, "{u:?} {v:?}");
        let start = (Ivl::new(-0.05, 0.05), Ivl::new(0.6, 0.7));
        let (u, v) = nearest_box(&net, start, point(-1.0e-3, 0.3, 0.0)).unwrap();
        assert!(
            u.lo <= 0.0 && 0.0 <= u.hi && v.lo <= 0.65 && 0.65 <= v.hi,
            "{u:?} {v:?}"
        );
        assert!(u.hi - u.lo < 1.0e-9 && v.hi - v.lo < 1.0e-9, "{u:?} {v:?}");
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
