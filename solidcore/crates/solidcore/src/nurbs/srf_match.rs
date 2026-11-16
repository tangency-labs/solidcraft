//! A surface's boundary matched to a boundary of another surface: the control rows next to the
//! boundary moved so the two meet in position (G0), tangent plane (G1) or normal curvature
//! across the seam (G2).
//!
//! Not upstream. The surface and the target are non-rational B-spline surfaces, clamped; the
//! boundary is one side of the surface's parameter rectangle, the target one side of the
//! target's. Everything is worked on the surface turned so its boundary is `u = umin` (`swap_uv`
//! and a reversal of u), and turned back after.
//!
//! **Stations.** The boundary's columns of control points, one station each, at the Greville
//! abscissa `g_j` of the column along the boundary. The target's boundary is mapped onto the
//! surface's linearly, the way round that pairs each end of the boundary with the nearer end of
//! the target's; station `j` meets the target at the mapped parameter.
//!
//! **Refinement where the knots cannot carry the target.** Before the solve, the surface is
//! raised in degree across the boundary to `order + 1` (a G2 seam needs a second derivative
//! across it, and a row moved on a degree-1 net would leave a crease behind it); along the
//! boundary to the target's degree there, with the target's inner knots there inserted where the
//! surface has none (mapped as the stations are); and across the boundary, the first span is
//! halved until the net has at least `rows + 2` rows, so the rows that move leave the far side
//! (its position and tangent) where it was. The surface's shape does not change in refinement.
//!
//! **The row solve.** With `P_0`, `P_1`, `P_2` the rows from the boundary, `u_i` the knots across
//! (clamped: `u_0 = .. = u_p`), the boundary's derivatives across it are the clamped B-spline's
//! (Piegl and Tiller, *The NURBS Book*, eq. 3.8):
//!
//! ```text
//! S(0)   = P_0
//! S'(0)  = c1 (P_1 - P_0),                       c1 = p / (u_(p+1) - u_1)
//! S''(0) = c2 (P_2 - P_1) - c3 (P_1 - P_0),      c2 = p (p - 1) / ((u_(p+1) - u_2) (u_(p+2) - u_2))
//! ```
//!
//! each row taken along the boundary as a B-spline curve on the surface's knots there. Each
//! order is a global interpolation of the stations (the kernel's
//! [`interp_given_params`], on the surface's own knots and degree along the boundary, at the
//! Greville abscissae, which the Schoenberg-Whitney condition admits):
//! - G0: the row `P_0` through the target's boundary points at the stations;
//! - G1: the row `P_1 - P_0` through `S'(0) / c1`, where `S'(0)` is the surface's own derivative
//!   across at each station with its part along the target's normal taken off: the boundary
//!   leaves into the target's tangent plane, as close to how it left before as that allows;
//! - G2: along the target's normal only, `P_2` moved by the interpolant of
//!   `(k_T |S'|^2 - S'' . n) / c2` at the stations, `k_T` the target's normal curvature in the
//!   direction `S'` (its second fundamental form over its first), so the surface's normal
//!   curvature across the seam is the target's.
//!
//! Each row the order does not fix, up to `rows`, moves by `blend` times the move of the row
//! before it (the falloff that spreads the change into the surface); rows the order does not fix
//! do not change what the seam holds. Each row the order fixes carries the moves of the rows
//! before it first (so a G0 move translates the rows behind it rather than bending them).
//!
//! **What holds.** At the stations the seam holds the order asked, to rounding: the solve is
//! exact there. Between stations it holds where the target is the same all along the boundary
//! (a plane, a cylinder along its rulings) and is otherwise the interpolant's; the seam is
//! measured at the stations before and after ([`SeamBounds`]: the largest gap, angle between
//! the normals and jump of the normal curvature across), never assumed.
//!
//! **Refused**, naming why: a rational or unclamped surface or target, a row count outside 1..=3
//! or below the order plus one, a blend outside 0..=1, a target degenerate at a station (no
//! tangent plane there), a boundary shorter than the kernel's zero, and the refinement's and the
//! interpolation's own refusals. Bounded: at most 3 degree elevations a direction, 64 knots
//! along the boundary and 8 halvings across it; no recursion.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing tests

use crate::error::{Result, SolidError};
use crate::math::{Vec3, EFF_ZERO};
use crate::nurbs::crv_basic::Boundary;
use crate::nurbs::crv_fit::p1::interp_given_params;
use crate::nurbs::knots::{KnotVector, Side};
use crate::nurbs::math::CPoint;
use crate::nurbs::srf_basic::{Direction, Surface};
use crate::nurbs::srf_tool::p1::{insert_knot, insert_knots};
use crate::nurbs::srf_tool::p2::elevate_degree;

const OP: &str = "nurbs::srf_match::match_boundary";

/// The most knots of the target inserted along the boundary.
const MAX_INSERTED: usize = 64;
/// The most halvings of the first span across the boundary.
const MAX_HALVINGS: usize = 8;

/// The continuity a matched boundary meets the target with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MatchOrder {
    /// In position.
    G0,
    /// In position and tangent plane.
    G1,
    /// In position, tangent plane and normal curvature across the seam.
    G2,
}

impl MatchOrder {
    /// The order: 0, 1 or 2.
    #[must_use]
    pub const fn order(self) -> usize {
        match self {
            MatchOrder::G0 => 0,
            MatchOrder::G1 => 1,
            MatchOrder::G2 => 2,
        }
    }
}

/// How far the change of a match reaches into the surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MatchRows {
    /// The rows from the boundary that may move, 1 to 3, at least the order plus one.
    pub rows: usize,
    /// Each row beyond those the order fixes moves by this share of the row before it's move,
    /// 0 to 1.
    pub blend: f64,
}

/// A seam measured at the stations: the largest of each measure over them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SeamBounds {
    /// The gap between the boundary and the target's boundary.
    pub gap: f64,
    /// The angle between the two surfaces' normals, radians (0 to pi / 2: normals are lines).
    pub angle: f64,
    /// The jump of the normal curvature across the seam, in the surface's direction across.
    pub curvature: f64,
}

/// A matched surface: the surface (refined as the match needed), the seam before and after,
/// and how far each of the first three rows from the boundary moved (the largest move of a
/// control point of the row; 0 for a row that did not move).
#[derive(Clone, Debug, PartialEq)]
pub struct Matched {
    /// The surface, its boundary matched.
    pub surface: Surface,
    /// The seam before the match (on the refined surface: the same shape).
    pub before: SeamBounds,
    /// The seam after.
    pub after: SeamBounds,
    /// The largest move of a control point of rows 0, 1 and 2 from the boundary.
    pub moved: [f64; 3],
}

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

/// `srf` with the control rows next to its boundary `side` moved so it meets boundary
/// `target_side` of `target` at `order`, at most `how.rows` rows moving, the change spread by
/// `how.blend`; refined first where its knots cannot carry the target. See the module notes.
///
/// # Errors
/// `InvalidInput` for the refusals the module names; those of the refinement, the
/// interpolation and the evaluations.
pub fn match_boundary(
    srf: &Surface,
    side: Boundary,
    target: &Surface,
    target_side: Boundary,
    order: MatchOrder,
    how: MatchRows,
) -> Result<Matched> {
    let k = order.order();
    if !(1..=3).contains(&how.rows) || how.rows < k + 1 {
        return Err(invalid(
            "the rows that move are 1 to 3, and at least the order plus one",
        ));
    }
    if !(how.blend.is_finite() && (0.0..=1.0).contains(&how.blend)) {
        return Err(invalid("a blend factor is between 0 and 1"));
    }
    if srf.is_rational() || target.is_rational() {
        return Err(invalid("a rational surface or target"));
    }
    if !clamped(srf) || !clamped(target) {
        return Err(invalid("an unclamped surface or target"));
    }
    let turned = to_left(srf, side)?;
    let edge = TargetEdge::of(target, target_side)?;
    let reversed = edge.reversed_against(&turned)?;
    let mut s = refine(turned, &edge, reversed, k, how.rows)?;
    let stations = stations(&s, &edge, reversed)?;
    let before = measure(&s, &stations)?;

    let (p, q) = s.degrees();
    let u = s.u_knots().to_vec();
    let v = s.v_knots().to_vec();
    let params: Vec<f64> = stations.iter().map(|st| st.g).collect();
    let old: Vec<Vec<Vec3>> = (0..how.rows).map(|i| row(&s, i)).collect::<Result<_>>()?;
    let mut new: Vec<Vec<Vec3>> = old.clone();

    // G0: the boundary row through the target's points
    let row0 = interp(
        &stations.iter().map(|st| st.point).collect::<Vec<_>>(),
        &params,
        &v,
        q,
    )?;
    set_moved(&mut new, &old, 0, row0)?;

    // G1: the row behind it so the boundary leaves into the target's tangent plane
    if k >= 1 {
        let c1 = p as f64 / span(&u, p + 1, 1)?;
        let across = stations
            .iter()
            .map(|st| -> Result<Vec3> {
                let d = derivs(&s, u_lo(&u)?, st.g, &v)?;
                let su = at2(&d, 1, 0)?;
                Ok((su - st.normal * su.dot(st.normal)) * (1.0 / c1))
            })
            .collect::<Result<Vec<Vec3>>>()?;
        let lean = interp(&across, &params, &v, q)?;
        let row1: Vec<Vec3> = zip_with(row_of(&new, 0)?, &lean, |a, b| a + b)?;
        set_moved(&mut new, &old, 1, row1)?;
    } else {
        spread(&mut new, &old, 1, how)?;
    }

    // G2: the third row along the normal, for the target's normal curvature across
    if k >= 2 {
        carry_into(&mut new, &old, 2)?;
        let trial = with_rows(&s, &new)?;
        let c2 = (p * (p - 1)) as f64 / (span(&u, p + 1, 2)? * span(&u, p + 2, 2)?);
        let lift = stations
            .iter()
            .map(|st| -> Result<Vec3> {
                let d = derivs(&trial, u_lo(&u)?, st.g, &v)?;
                let (su, suu) = (at2(&d, 1, 0)?, at2(&d, 2, 0)?);
                let want = st.normal_curvature(su)? * su.length_squared();
                Ok(Vec3::new((want - suu.dot(st.normal)) / c2, 0.0, 0.0))
            })
            .collect::<Result<Vec<Vec3>>>()?;
        let lift = interp(&lift, &params, &v, q)?;
        let behind = row_of(&new, 2)?.clone();
        let row2 = behind
            .iter()
            .enumerate()
            .map(|(j, b)| -> Result<Vec3> {
                let (Some(st), Some(l)) = (stations.get(j), lift.get(j)) else {
                    return Err(invalid("a column without a station"));
                };
                Ok(*b + st.normal * l.x)
            })
            .collect::<Result<Vec<Vec3>>>()?;
        set_moved(&mut new, &old, 2, row2)?;
    } else {
        spread(&mut new, &old, 2, how)?;
    }

    s = with_rows(&s, &new)?;
    let after = measure(&s, &stations)?;
    let mut moved = [0.0; 3];
    for (i, m) in moved.iter_mut().enumerate() {
        if let (Some(a), Some(b)) = (old.get(i), new.get(i)) {
            *m = a
                .iter()
                .zip(b)
                .map(|(x, y)| (*y - *x).length())
                .fold(0.0, f64::max);
        }
    }
    Ok(Matched {
        surface: from_left(s, side)?,
        before,
        after,
        moved,
    })
}

// ---- the surface turned so its boundary is u = umin ----------------------------------------

/// Whether both knot vectors are clamped: `p + 1` equal knots at each end.
fn clamped(s: &Surface) -> bool {
    let (p, q) = s.degrees();
    let ends = |k: &[f64], d: usize| {
        let (Some(first), Some(last)) = (k.first(), k.last()) else {
            return false;
        };
        k.len() >= 2 * (d + 1)
            && k.iter().take(d + 1).all(|x| x == first)
            && k.iter().rev().take(d + 1).all(|x| x == last)
    };
    ends(s.u_knots(), p) && ends(s.v_knots(), q)
}

/// `s` with u reversed: rows in reverse order, knots `u_0 + u_r - u_(r-i)`.
fn reverse_u(s: &Surface) -> Result<Surface> {
    let (p, q) = s.degrees();
    let u = s.u_knots();
    let (Some(&a), Some(&b)) = (u.first(), u.last()) else {
        return Err(invalid("an empty knot vector"));
    };
    let ru: Vec<f64> = u.iter().rev().map(|x| a + b - x).collect();
    let mut net: Vec<Vec<CPoint>> = s.rows().map(<[CPoint]>::to_vec).collect();
    net.reverse();
    Surface::new(net, p, q, ru, s.v_knots().to_vec())
}

/// `s` turned so its boundary `side` is `u = umin`.
fn to_left(s: &Surface, side: Boundary) -> Result<Surface> {
    match side {
        Boundary::Left => Ok(s.clone()),
        Boundary::Right => reverse_u(s),
        Boundary::Bottom => {
            let mut t = s.clone();
            t.swap_uv();
            Ok(t)
        }
        Boundary::Top => {
            let mut t = s.clone();
            t.swap_uv();
            reverse_u(&t)
        }
    }
}

/// [`to_left`] undone.
fn from_left(s: Surface, side: Boundary) -> Result<Surface> {
    match side {
        Boundary::Left => Ok(s),
        Boundary::Right => reverse_u(&s),
        Boundary::Bottom => {
            let mut t = s;
            t.swap_uv();
            Ok(t)
        }
        Boundary::Top => {
            let mut t = reverse_u(&s)?;
            t.swap_uv();
            Ok(t)
        }
    }
}

// ---- the target's boundary --------------------------------------------------------------------

/// A boundary of the target: the surface, which side, its parameter range along the boundary and
/// the fixed parameter across it.
struct TargetEdge<'a> {
    surface: &'a Surface,
    side: Boundary,
    along: [f64; 2],
    fixed: f64,
}

/// What the target is at a station.
#[derive(Clone, Copy, Debug)]
struct Station {
    /// The station's parameter along the surface's boundary.
    g: f64,
    /// The target's boundary point.
    point: Vec3,
    /// The target's unit normal.
    normal: Vec3,
    /// The target's first and second partials: `T_u`, `T_v`, `T_uu`, `T_uv`, `T_vv`.
    partials: [Vec3; 5],
}

impl Station {
    /// The target's normal curvature in the tangent direction `w` (its second fundamental form
    /// over its first); `w` is taken in the tangent plane.
    fn normal_curvature(&self, w: Vec3) -> Result<f64> {
        let [ts, tt, tss, tst, ttt] = self.partials;
        let (e, f, g) = (ts.dot(ts), ts.dot(tt), tt.dot(tt));
        let det = e * g - f * f;
        if !(det > EFF_ZERO) {
            return Err(invalid("the target has no tangent plane at a station"));
        }
        let (wa, wb) = (w.dot(ts), w.dot(tt));
        let a = (g * wa - f * wb) / det;
        let b = (e * wb - f * wa) / det;
        let first = e * a * a + 2.0 * f * a * b + g * b * b;
        if !(first > EFF_ZERO) {
            return Err(invalid("no direction across the seam at a station"));
        }
        let n = self.normal;
        let second = tss.dot(n) * a * a + 2.0 * tst.dot(n) * a * b + ttt.dot(n) * b * b;
        Ok(second / first)
    }
}

impl<'a> TargetEdge<'a> {
    fn of(surface: &'a Surface, side: Boundary) -> Result<Self> {
        let r = surface.parameter_bounds();
        let (along, fixed) = match side {
            Boundary::Left => ([r.vb, r.vt], r.ul),
            Boundary::Right => ([r.vb, r.vt], r.ur),
            Boundary::Bottom => ([r.ul, r.ur], r.vb),
            Boundary::Top => ([r.ul, r.ur], r.vt),
        };
        if !(along[1] > along[0]) {
            return Err(invalid("the target's boundary has no length"));
        }
        Ok(Self {
            surface,
            side,
            along,
            fixed,
        })
    }

    /// The target's `(u, v)` at parameter `t` along the boundary.
    fn uv(&self, t: f64) -> (f64, f64) {
        match self.side {
            Boundary::Left | Boundary::Right => (self.fixed, t),
            Boundary::Bottom | Boundary::Top => (t, self.fixed),
        }
    }

    /// The target's degree and knots along the boundary.
    fn along_knots(&self) -> (usize, &[f64]) {
        let (p, q) = self.surface.degrees();
        match self.side {
            Boundary::Left | Boundary::Right => (q, self.surface.v_knots()),
            Boundary::Bottom | Boundary::Top => (p, self.surface.u_knots()),
        }
    }

    /// The target at parameter `t` along the boundary.
    fn at(&self, t: f64, g: f64) -> Result<Station> {
        let (u, v) = self.uv(t);
        let r = self.surface.parameter_bounds();
        let d = self
            .surface
            .derivs(u, v, side_at(u, r.ur), side_at(v, r.vt), false, 2, 2)?;
        let (tu, tv) = (at2(&d, 1, 0)?, at2(&d, 0, 1)?);
        let normal = tu
            .cross(tv)
            .unitize()
            .map_err(|_| invalid("the target has no tangent plane at a station"))?;
        Ok(Station {
            g,
            point: at2(&d, 0, 0)?,
            normal,
            partials: [tu, tv, at2(&d, 2, 0)?, at2(&d, 1, 1)?, at2(&d, 0, 2)?],
        })
    }

    /// Whether the boundary of `s` (turned: `u = umin`) runs against the target's: its start
    /// and end nearer the target's end and start than its start and end.
    fn reversed_against(&self, s: &Surface) -> Result<bool> {
        let r = s.parameter_bounds();
        let (s0, s1) = (point(s, r.ul, r.vb)?, point(s, r.ul, r.vt)?);
        let (u0, v0) = self.uv(self.along[0]);
        let (u1, v1) = self.uv(self.along[1]);
        let (t0, t1) = (point(self.surface, u0, v0)?, point(self.surface, u1, v1)?);
        Ok((s0 - t1).length() + (s1 - t0).length() < (s0 - t0).length() + (s1 - t1).length())
    }

    /// The target's parameter for the surface's parameter `g` in `[lo, hi]` along its boundary.
    fn mapped(&self, g: f64, lo: f64, hi: f64, reversed: bool) -> f64 {
        let f = (g - lo) / (hi - lo);
        let f = if reversed { 1.0 - f } else { f };
        self.along[0] + f * (self.along[1] - self.along[0])
    }

    /// The surface's parameter in `[lo, hi]` for the target's `t` (the inverse of [`Self::mapped`]).
    fn unmapped(&self, t: f64, lo: f64, hi: f64, reversed: bool) -> f64 {
        let f = (t - self.along[0]) / (self.along[1] - self.along[0]);
        let f = if reversed { 1.0 - f } else { f };
        lo + f * (hi - lo)
    }
}

/// The half-open side a parameter is evaluated from: the span after it, but the one before at
/// the end of the range.
fn side_at(t: f64, hi: f64) -> Side {
    if t >= hi {
        Side::Right
    } else {
        Side::Left
    }
}

/// `d[k][l]`.
fn at2(d: &[Vec<Vec3>], k: usize, l: usize) -> Result<Vec3> {
    d.get(k)
        .and_then(|r| r.get(l))
        .copied()
        .ok_or_else(|| invalid("a derivative the evaluation did not give"))
}

/// The point of `s` at `(u, v)`.
fn point(s: &Surface, u: f64, v: f64) -> Result<Vec3> {
    let r = s.parameter_bounds();
    at2(
        &s.derivs(u, v, side_at(u, r.ur), side_at(v, r.vt), false, 0, 0)?,
        0,
        0,
    )
}

/// The derivatives of `s` to second order across, on its boundary `u = u0`, at `v` (the side of
/// `v` from the knots `vk`).
fn derivs(s: &Surface, u0: f64, v: f64, vk: &[f64]) -> Result<Vec<Vec<Vec3>>> {
    let hi = vk.last().copied().unwrap_or(f64::INFINITY);
    s.derivs(u0, v, Side::Left, side_at(v, hi), false, 2, 1)
}

// ---- refinement and stations ------------------------------------------------------------------

/// `s` (turned) raised and refined so it can carry the target at order `k` with `rows` rows.
fn refine(
    mut s: Surface,
    edge: &TargetEdge<'_>,
    reversed: bool,
    k: usize,
    rows: usize,
) -> Result<Surface> {
    // across: degree at least k + 1
    let (p, q) = s.degrees();
    if p < k + 1 {
        s = elevate_degree(&s, k + 1 - p, Direction::U)?;
    }
    // along: the target's degree, and its inner knots where the surface has none
    let (tq, tk) = edge.along_knots();
    if tq > q && tq - q <= 3 {
        s = elevate_degree(&s, tq - q, Direction::V)?;
    }
    let (lo, hi) = {
        let r = s.parameter_bounds();
        (r.vb, r.vt)
    };
    let tol = EFF_ZERO * (1.0 + (hi - lo).abs());
    let mut add: Vec<f64> = Vec::new();
    for &t in tk {
        if !(t > edge.along[0] && t < edge.along[1]) {
            continue;
        }
        let g = edge.unmapped(t, lo, hi, reversed);
        let present = s.v_knots().iter().any(|x| (x - g).abs() <= tol)
            || add.iter().any(|x| (x - g).abs() <= tol);
        if !present && g > lo + tol && g < hi - tol && add.len() < MAX_INSERTED {
            add.push(g);
        }
    }
    if !add.is_empty() {
        add.sort_by(f64::total_cmp);
        s = insert_knots(&s, &add, Direction::V)?;
    }
    // across: the first span halved until the rows that move leave the far side's two
    for _ in 0..MAX_HALVINGS {
        if s.n() > rows {
            break;
        }
        let (p, _) = s.degrees();
        let u = s.u_knots();
        let (Some(&a), Some(&b)) = (u.get(p), u.get(p + 1)) else {
            return Err(invalid("a knot vector shorter than its degree"));
        };
        s = insert_knot(&s, (a + b) / 2.0, 1, Direction::U)?;
    }
    if s.n() < rows + 1 {
        return Err(invalid("the surface cannot be refined to rows enough"));
    }
    Ok(s)
}

/// One station per column of the boundary of `s` (turned), at its Greville abscissa.
fn stations(s: &Surface, edge: &TargetEdge<'_>, reversed: bool) -> Result<Vec<Station>> {
    let (_, q) = s.degrees();
    let v = s.v_knots();
    let r = s.parameter_bounds();
    let out = (0..=s.m())
        .map(|j| {
            let g = if q == 0 {
                v.get(j).copied().unwrap_or(r.vb)
            } else {
                let sum: f64 = v.iter().skip(j + 1).take(q).sum();
                sum / q as f64
            };
            edge.at(edge.mapped(g, r.vb, r.vt, reversed), g)
        })
        .collect::<Result<Vec<_>>>()?;
    let Some(first) = out.first().map(|st| st.point) else {
        return Err(invalid("a boundary without stations"));
    };
    let reach = out
        .iter()
        .map(|st| (st.point - first).length())
        .fold(0.0, f64::max);
    if !(reach > EFF_ZERO) {
        return Err(invalid("the target's boundary is a point"));
    }
    Ok(out)
}

/// The seam between `s`'s boundary (turned) and the target at the stations.
fn measure(s: &Surface, stations: &[Station]) -> Result<SeamBounds> {
    let r = s.parameter_bounds();
    let v = s.v_knots();
    let mut out = SeamBounds::default();
    for st in stations {
        let d = derivs(s, r.ul, st.g, v)?;
        let (p, su, sv, suu) = (
            at2(&d, 0, 0)?,
            at2(&d, 1, 0)?,
            at2(&d, 0, 1)?,
            at2(&d, 2, 0)?,
        );
        out.gap = out.gap.max((p - st.point).length());
        let Ok(mut n) = su.cross(sv).unitize() else {
            return Err(invalid("the surface has no tangent plane at a station"));
        };
        if n.dot(st.normal) < 0.0 {
            n *= -1.0;
        }
        // atan2 of the cross and the dot: exact near 0, where acos of a dot loses half the digits
        let angle = n.cross(st.normal).length().atan2(n.dot(st.normal));
        out.angle = out.angle.max(angle);
        let len2 = su.length_squared();
        if len2 > EFF_ZERO {
            let ks = suu.dot(n) / len2;
            let kt = st.normal_curvature(su)?;
            out.curvature = out.curvature.max((ks - kt).abs());
        }
    }
    Ok(out)
}

// ---- rows ------------------------------------------------------------------------------------

/// Row `i` of `s`'s net as points.
fn row(s: &Surface, i: usize) -> Result<Vec<Vec3>> {
    (0..=s.m())
        .map(|j| s.cpt(i, j).map(CPoint::to_euclid))
        .collect()
}

fn row_of(rows: &[Vec<Vec3>], i: usize) -> Result<&Vec<Vec3>> {
    rows.get(i).ok_or_else(|| invalid("a row past the net"))
}

/// `f(a[j], b[j])` over two rows of one length.
fn zip_with(a: &[Vec3], b: &[Vec3], f: impl Fn(Vec3, Vec3) -> Vec3) -> Result<Vec<Vec3>> {
    if a.len() != b.len() {
        return Err(invalid("rows of different lengths"));
    }
    Ok(a.iter().zip(b).map(|(x, y)| f(*x, *y)).collect())
}

/// Row `i` of `new` set to `to`.
fn set_moved(new: &mut [Vec<Vec3>], old: &[Vec<Vec3>], i: usize, to: Vec<Vec3>) -> Result<()> {
    if row_of(old, i)?.len() != to.len() {
        return Err(invalid("a solved row of another length"));
    }
    let slot = new
        .get_mut(i)
        .ok_or_else(|| invalid("a row past the net"))?;
    *slot = to;
    Ok(())
}

/// Row `i` of `new` (when it moves) moved as row `i - 1` moved, before its own solve.
fn carry_into(new: &mut [Vec<Vec3>], old: &[Vec<Vec3>], i: usize) -> Result<()> {
    let Some(prev) = i.checked_sub(1) else {
        return Ok(());
    };
    if i >= new.len() {
        return Ok(());
    }
    let shift = zip_with(row_of(new, prev)?, row_of(old, prev)?, |a, b| a - b)?;
    let carried = zip_with(row_of(old, i)?, &shift, |a, b| a + b)?;
    set_moved(new, old, i, carried)
}

/// Row `i` and the rows after it that move, each moved by `how.blend` times the move of the row
/// before it.
fn spread(new: &mut [Vec<Vec3>], old: &[Vec<Vec3>], from: usize, how: MatchRows) -> Result<()> {
    for i in from..new.len() {
        let Some(prev) = i.checked_sub(1) else {
            continue;
        };
        let shift = zip_with(row_of(new, prev)?, row_of(old, prev)?, |a, b| {
            (a - b) * how.blend
        })?;
        let moved = zip_with(row_of(old, i)?, &shift, |a, b| a + b)?;
        set_moved(new, old, i, moved)?;
    }
    Ok(())
}

/// `s` with its first rows replaced by `rows`.
fn with_rows(s: &Surface, rows: &[Vec<Vec3>]) -> Result<Surface> {
    let mut t = s.clone();
    for (i, r) in rows.iter().enumerate() {
        for (j, p) in r.iter().enumerate() {
            *t.cpt_mut(i, j)? = CPoint::from_point(*p);
        }
    }
    Ok(t)
}

/// The control points of the curve of degree `q` on knots `knots` through `values` at `params`.
fn interp(values: &[Vec3], params: &[f64], knots: &[f64], q: usize) -> Result<Vec<Vec3>> {
    let kv = KnotVector::from_slice(knots)?;
    let c = interp_given_params(values, params, &kv, q)?;
    Ok(c.cpts()
        .iter()
        .take(values.len())
        .copied()
        .map(CPoint::to_euclid)
        .collect())
}

/// The first knot of `u`.
fn u_lo(u: &[f64]) -> Result<f64> {
    u.first()
        .copied()
        .ok_or_else(|| invalid("an empty knot vector"))
}

/// `u[i] - u[j]`, refused when not positive.
fn span(u: &[f64], i: usize, j: usize) -> Result<f64> {
    let (Some(a), Some(b)) = (u.get(i), u.get(j)) else {
        return Err(invalid("a knot past the vector"));
    };
    let d = a - b;
    if d > 0.0 {
        Ok(d)
    } else {
        Err(invalid("an empty first span across the boundary"))
    }
}

#[cfg(test)]
mod tests;
