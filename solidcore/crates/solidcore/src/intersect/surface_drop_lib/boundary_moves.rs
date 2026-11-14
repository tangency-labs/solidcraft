//! Keeps a dropped UV curve inside its domain without moving it off its 3D curve by more
//! than the tolerance the drop has left: a certified form of the boundary fix-up of
//! [`super::p1::fix_boundary_curves`] (not upstream).
//!
//! Upstream: `sm_FixBoundaryCurves` (`source/SMLib/src/SmSurfaceDropLib.cpp`), which this
//! replaces where the surface is a B-spline or an offset of one.
//!
//! | Rust                            | Upstream               |
//! |---------------------------------|------------------------|
//! | [`fix_boundary_curves_bounded`] | `sm_FixBoundaryCurves` |
//! | [`MoveBound`]                   | none                   |
//!
//! **The defect.** `sm_FixBoundaryCurves` moves both control-polygon neighbours of an
//! interior knot point within the UV tolerance of a domain boundary onto the knot's
//! coordinate, however far they are from it. For a curve running along the boundary that
//! makes it an exact isoline. A curve that only reaches the boundary at a shallow angle is
//! flattened onto it over a whole span instead. That happened to the mated crease edge of
//! the `HoodCorner` sheet thickened by 2 (tangency-labs/solidcraft#15282): it runs into the
//! corner of each face's shrunk surface, and its UV trims came out 4.5e-4 off the 3D curve
//! at a 2e-5 tolerance. Where the drop's own tangent fix-up (`FixBoundaryTangents`) had
//! zeroed the cross-boundary tangent of the short last span only, the flattening was what
//! kept the polygon collinear through the knot.
//!
//! **The rule here.** A polygon holding a number that is not finite, or a domain that is
//! empty or not a number, is refused. Then:
//! 1. knot points outside the domain are snapped onto it, and the inner neighbour of an end
//!    knot point on a boundary is clamped into the domain, as upstream does;
//! 2. at an interior knot point within the UV tolerance of a boundary, in the coordinate
//!    across it, upstream's move (both neighbours onto the knot's coordinate: an exact
//!    isoline, collinear through the knot) is made when its bound fits what is left of the
//!    budget of both spans;
//! 3. otherwise the polygon is made collinear through the knot again, C1 with the knots'
//!    spacing (`(B - K) h_prev = (K - A) h_next` in that coordinate), by moving one
//!    neighbour onto the line through the knot and the other neighbour: the one of the two
//!    choices that stays in the domain and fits the budget, the smaller bound first;
//! 4. otherwise neither neighbour moves for the collinearity;
//! 5. every control point still outside the domain is clamped into it, so the curve (in the
//!    convex hull of its control points) lies in the domain, as upstream's moves left it;
//! 6. the polygon's total move is bounded span by span against the polygon as traced; a
//!    span that moved and whose bound is not proved or does not fit the budget (which is
//!    negative where the trace accepted a short span over the tolerance; an unmoved span
//!    needs nothing proved) refuses the whole curve
//!    (an error: the drop fails, and its callers' recoveries run), and the largest span
//!    bound is reported ([`MoveBound::spent`]) for the drop to add to the deviation it
//!    returns.
//!
//! **The bound.** Moving control point `P_j` of a cubic Bezier span by `delta_j` moves its
//! UV point at `t` by at most `sum_j b_j |delta_j|`, `b_j = max B_j` (1 for the ends, 4/9
//! inside). The drop's deviation is measured on the traced span with points outside the
//! domain clamped into it (`DropPt::test_span_accuracy`), so the moved span `c'` is compared
//! with the clamped traced span `clamp(c)`: both lie in the domain and in the bounding box
//! `X` of the old and new control points, so the segment between their points lies in
//! `X` intersected with the domain, and clamping does not lengthen a distance (the
//! domain is convex):
//! `|S(c'(t)) - S(clamp(c(t)))| <= sup |S_u| sum_j b_j |du_j| + sup |S_v| sum_j b_j |dv_j|`,
//! suprema over that box. They come from the B-spline net's first partials enclosed in
//! rounded interval arithmetic ([`crate::mass::enclosure::partials_over`]); for an offset
//! `S + d n` of a net (the drop runs on offset faces before they are made B-splines),
//! `|d| |N_axis| / |N|` is added (`N = S_u x S_v`; `n_axis` is `N_axis` less its part
//! along `n`, over `|N|`), from the net's second partials with `|N|` proved positive, and
//! only over the net's own domain. The budget is the drop's 3D tolerance less the largest
//! deviation it measured while tracing (that deviation is sampled, as upstream's span test
//! samples it; only the moves are bounded).
//!
//! Deviations: not upstream (above). Used for B-spline surfaces and offsets of them;
//! elsewhere the drop keeps upstream's [`super::p1::fix_boundary_curves`].

use crate::blend::fillet_section::interval::{round_down, round_up, Ivl};
use crate::blend::fillet_section::series::V3;
use crate::blend::fillet_section::station::Nurbs;
use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::geom::vector3d::Point3;
use crate::kernel::Kernel;
use crate::mass::enclosure::partials_over;
use crate::surface::surface::p2::SurfaceKind;
use crate::surface::surface::Surface;
use core::cell::Cell;

const OP: &str = "intersect::surface_drop_lib::fix_boundary_curves_bounded";

/// The largest Bernstein basis value of an interior control point of a cubic Bezier span
/// (`B_1(1/3) = B_2(2/3) = 4/9`), rounded up.
fn interior_basis_max() -> f64 {
    round_up(4.0 / 9.0)
}

/// What [`fix_boundary_curves_bounded`] may move a span by: the surface's net, for its
/// partials, and the 3D distance a span may move in all.
pub struct MoveBound {
    net: Nurbs,
    /// `Some((d, domain))` for an offset by `d` of the net, whose bound holds over the
    /// net's own domain only (outside it the offset may evaluate an extension).
    offset: Option<(f64, Extent2d)>,
    /// The 3D distance a span may be moved by, all its moves together.
    pub budget: f64,
    /// The largest bound of a span's total move in the last fix-up.
    spent: Cell<f64>,
    /// Whether a fix-up refused its curve.
    refused: Cell<bool>,
}

type IV3 = V3<Ivl>;

/// An upper bound of `|v|`, rounding included.
fn norm_up(v: IV3) -> f64 {
    let (x, y, z) = (v.x.mag(), v.y.mag(), v.z.mag());
    round_up(round_up(round_up(x * x + y * y) + z * z).sqrt())
}

/// A lower bound of `|v|`, rounding included (0 where a component's enclosure holds 0
/// in every component).
fn norm_down(v: IV3) -> f64 {
    let sq = |c: Ivl| {
        if c.lo > 0.0 {
            round_down(c.lo * c.lo)
        } else if c.hi < 0.0 {
            round_down(c.hi * c.hi)
        } else {
            0.0
        }
    };
    let s = round_down(round_down(sq(v.x) + sq(v.y)) + sq(v.z));
    if s > 0.0 {
        round_down(s.sqrt())
    } else {
        0.0
    }
}

fn cross(a: IV3, b: IV3) -> IV3 {
    V3 {
        x: a.y * b.z - a.z * b.y,
        y: a.z * b.x - a.x * b.z,
        z: a.x * b.y - a.y * b.x,
    }
}

fn plus(a: IV3, b: IV3) -> IV3 {
    V3 {
        x: a.x + b.x,
        y: a.y + b.y,
        z: a.z + b.z,
    }
}

impl MoveBound {
    /// The bound of a B-spline surface, or of an offset of one (its own parameters are
    /// its net's); `None` for any other kind or a net that does not agree.
    #[must_use]
    pub fn of(k: &Kernel, surface: &Surface, budget: f64) -> Option<Self> {
        match surface.kind() {
            SurfaceKind::BSpline(b) => Nurbs::of(b).ok().map(|net| Self {
                net,
                offset: None,
                budget,
                spent: Cell::new(0.0),
                refused: Cell::new(false),
            }),
            SurfaceKind::OffsetSurface(o) => {
                let SurfaceKind::BSpline(b) = o.base_surface().kind() else {
                    return None;
                };
                let d = o.offset_distance();
                let domain = o.base_surface().natural_uv_domain_with_nurb(k).ok()?;
                (d.is_finite())
                    .then(|| Nurbs::of(b).ok())
                    .flatten()
                    .map(|net| Self {
                        net,
                        offset: Some((d, domain)),
                        budget,
                        spent: Cell::new(0.0),
                        refused: Cell::new(false),
                    })
            }
            _ => None,
        }
    }

    /// Upper bounds of `|S_u|` and `|S_v|` over the box: the net's first partials, and for
    /// an offset `S + d n` also `|d| |N_u| / |N|` and `|d| |N_v| / |N|` (`N = S_u x S_v`,
    /// `n_u` is `N_u` less its part along `n`, over `|N|`), from its second partials;
    /// `None` where they are not proved (a box outside an offset's net, weights or `|N|`
    /// not proved positive).
    fn lipschitz(&self, u: Ivl, v: Ivl) -> Option<(f64, f64)> {
        let order = if self.offset.is_some() { 2 } else { 1 };
        if let Some((_, dom)) = &self.offset {
            if u.lo < dom.u_min() || u.hi > dom.u_max() || v.lo < dom.v_min() || v.hi > dom.v_max()
            {
                return None;
            }
        }
        let p = partials_over(&self.net, u, v, order).ok().flatten()?;
        let (su, sv) = (p.at(1, 0), p.at(0, 1));
        let (mut lu, mut lv) = (norm_up(su), norm_up(sv));
        if let Some((d, _)) = self.offset {
            let (suu, suv, svv) = (p.at(2, 0), p.at(1, 1), p.at(0, 2));
            let n_lo = norm_down(cross(su, sv));
            if n_lo <= 0.0 {
                return None;
            }
            let nu = norm_up(plus(cross(suu, sv), cross(su, suv)));
            let nv = norm_up(plus(cross(suv, sv), cross(su, svv)));
            lu = round_up(lu + round_up(round_up(d.abs() * nu) / n_lo));
            lv = round_up(lv + round_up(round_up(d.abs() * nv) / n_lo));
        }
        (lu.is_finite() && lv.is_finite()).then_some((lu, lv))
    }

    /// The largest bound of a span's total move in the last fix-up (0 before one), for the
    /// drop to add to the deviation it measured, and clears it.
    pub fn spent(&self) -> f64 {
        self.spent.replace(0.0)
    }

    /// Whether a fix-up refused its curve since this was made.
    #[must_use]
    pub fn refused(&self) -> bool {
        self.refused.get()
    }

    /// A bound of how far the surface image of a cubic Bezier span moves from the traced
    /// span `old` (points outside `dom` clamped) to `new` (module docs): 0 where nothing
    /// moves, `None` where the partials are not proved over the box or the bound is not
    /// finite.
    fn span_bound(&self, old: [Point3; 4], new: [Point3; 4], dom: &Extent2d) -> Option<f64> {
        let interior = interior_basis_max();
        let b_max: [f64; 4] = [1.0, interior, interior, 1.0];
        if old == new {
            return Some(0.0);
        }
        // a coordinate that does not move adds exactly nothing
        let term = |b: f64, d: f64| {
            if d == 0.0 {
                0.0
            } else {
                round_up(b * round_up(d.abs()))
            }
        };
        let (mut su, mut sv) = (0.0_f64, 0.0_f64);
        for ((o, n), b) in old.iter().zip(&new).zip(b_max) {
            su = round_up(su + term(b, n.x - o.x));
            sv = round_up(sv + term(b, n.y - o.y));
        }
        if !(su.is_finite() && sv.is_finite()) {
            return None;
        }
        let (mut lo_u, mut hi_u, mut lo_v, mut hi_v) = (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        );
        for p in old.iter().chain(&new) {
            lo_u = lo_u.min(p.x);
            hi_u = hi_u.max(p.x);
            lo_v = lo_v.min(p.y);
            hi_v = hi_v.max(p.y);
        }
        let (lo_u, hi_u) = (lo_u.max(dom.u_min()), hi_u.min(dom.u_max()));
        let (lo_v, hi_v) = (lo_v.max(dom.v_min()), hi_v.min(dom.v_max()));
        if !(lo_u <= hi_u && lo_v <= hi_v) {
            return None;
        }
        let (lu, lv) = self.lipschitz(Ivl::new(lo_u, hi_u), Ivl::new(lo_v, hi_v))?;
        let bound = round_up(round_up(su * lu) + round_up(sv * lv));
        bound.is_finite().then_some(bound)
    }
}

/// One parameter axis of a UV point (`x` = u, `y` = v).
#[derive(Clone, Copy)]
enum Axis {
    U,
    V,
}

fn coord(p: Point3, axis: Axis) -> f64 {
    match axis {
        Axis::U => p.x,
        Axis::V => p.y,
    }
}

fn with_coord(mut p: Point3, axis: Axis, c: f64) -> Point3 {
    match axis {
        Axis::U => p.x = c,
        Axis::V => p.y = c,
    }
    p
}

/// [`super::p1::fix_boundary_curves`] with every move bounded as the module docs say;
/// `knots` are the breakpoints (one per knot point of the polygon). Returns whether a
/// boundary adjustment was made (set where upstream sets it).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a polygon of `3k` points (as
/// [`super::p1::fix_boundary_curves`]) or a breakpoint count other than its knot points';
/// an intersection failure where the curve is refused (a number not finite, or a span's
/// move not proved within the budget; [`MoveBound::refused`] is then set).
pub fn fix_boundary_curves_bounded(
    ctrl_poly: &mut [Point3],
    knots: &[f64],
    uv_domain: &Extent2d,
    uv_tol_x: f64,
    uv_tol_y: f64,
    bound: &MoveBound,
) -> Result<bool> {
    let num_pts = ctrl_poly.len();
    if num_pts < 4 {
        return Ok(false);
    }
    let num_spans = num_pts / 3;
    if 3 * num_spans >= num_pts {
        return Err(SolidError::invalid_input(
            OP,
            "control polygon of 3k points reads past its end",
        ));
    }
    if knots.len() != num_spans + 1 {
        return Err(SolidError::invalid_input(
            OP,
            "a breakpoint count other than the polygon's knot points",
        ));
    }
    let refuse = |reason: &'static str| {
        bound.refused.set(true);
        SolidError::intersect_failed(OP, reason)
    };
    let (dlo, dhi) = (uv_domain.min(), uv_domain.max());
    if !(dlo.x <= dhi.x && dlo.y <= dhi.y) {
        return Err(refuse("a domain that is empty or not a number"));
    }
    if ctrl_poly
        .iter()
        .any(|p| !(p.x.is_finite() && p.y.is_finite()))
    {
        return Err(refuse("a control point that is not a finite number"));
    }
    let bad = || SolidError::invalid_input(OP, "control point index");
    let (lo, hi) = (uv_domain.min(), uv_domain.max());
    let traced = ctrl_poly.to_vec();
    let mut changed = false;

    // 1. upstream's knot snaps and end clamps
    for ii in 0..=num_spans {
        let this_pt = 3 * ii;
        let knot = {
            let k = ctrl_poly.get_mut(this_pt).ok_or_else(bad)?;
            k.x = uv_domain.u_interval().clamp_value(k.x);
            k.y = uv_domain.v_interval().clamp_value(k.y);
            *k
        };
        if ii == 0 || ii == num_spans {
            let n = if ii == 0 { this_pt + 1 } else { this_pt - 1 };
            let next = ctrl_poly.get_mut(n).ok_or_else(bad)?;
            if knot.x < lo.x + uv_tol_x || knot.x > hi.x - uv_tol_x {
                next.x = uv_domain.u_interval().clamp_value(next.x);
                changed = true;
            }
            if knot.y < lo.y + uv_tol_y || knot.y > hi.y - uv_tol_y {
                next.y = uv_domain.v_interval().clamp_value(next.y);
                changed = true;
            }
        }
    }

    // 2.-4. the interior knot points on a boundary
    for ii in 1..num_spans {
        let knot = *ctrl_poly.get(3 * ii).ok_or_else(bad)?;
        let (h_prev, h_next) = match (knots.get(ii - 1), knots.get(ii), knots.get(ii + 1)) {
            (Some(a), Some(b), Some(c)) => (b - a, c - b),
            _ => return Err(bad()),
        };
        for (axis, l, h, tol) in [
            (Axis::U, lo.x, hi.x, uv_tol_x),
            (Axis::V, lo.y, hi.y, uv_tol_y),
        ] {
            let kc = coord(knot, axis);
            if !(kc < l + tol || kc > h - tol) {
                continue;
            }
            // upstream sets the flag whenever the knot is on a boundary
            changed = true;
            fix_interior(
                ctrl_poly,
                &traced,
                ii,
                axis,
                (l, h),
                (h_prev, h_next),
                uv_domain,
                bound,
            )?;
        }
    }

    // 5. every control point into the domain
    for p in ctrl_poly.iter_mut() {
        let (x, y) = (
            uv_domain.u_interval().clamp_value(p.x),
            uv_domain.v_interval().clamp_value(p.y),
        );
        if x != p.x || y != p.y {
            p.x = x;
            p.y = y;
            changed = true;
        }
    }

    // 6. the total move of every span, against the traced polygon
    let mut spent = 0.0_f64;
    for s in 0..num_spans {
        let (Some(old), Some(new)) = (span_of(&traced, s), span_of(ctrl_poly, s)) else {
            return Err(bad());
        };
        match bound.span_bound(old, new, uv_domain) {
            // a span that did not move needs nothing certified, whatever the budget (it
            // is negative where the trace accepted a short span over the tolerance)
            Some(b) if b == 0.0 || b <= bound.budget => spent = spent.max(b),
            _ => {
                return Err(refuse(
                    "a span's move is not proved within the tolerance left",
                ))
            }
        }
    }
    bound.spent.set(bound.spent.get().max(spent));
    Ok(changed)
}

/// The four control points of span `s`.
fn span_of(poly: &[Point3], s: usize) -> Option<[Point3; 4]> {
    match poly.get(3 * s..3 * s + 4) {
        Some(&[a, b, c, d]) => Some([a, b, c, d]),
        _ => None,
    }
}

/// The moves of the neighbours of interior knot point `ii` across one boundary (module
/// docs, steps 2 to 4), each tried against the span as traced (`traced`).
#[allow(clippy::too_many_arguments)] // one knot's geometry, the domain and the bound
fn fix_interior(
    ctrl_poly: &mut [Point3],
    traced: &[Point3],
    ii: usize,
    axis: Axis,
    (l, h): (f64, f64),
    (h_prev, h_next): (f64, f64),
    dom: &Extent2d,
    bound: &MoveBound,
) -> Result<()> {
    let bad = || SolidError::invalid_input(OP, "control point index");
    let this_pt = 3 * ii;
    let (Some(prev_span), Some(next_span)) = (span_of(ctrl_poly, ii - 1), span_of(ctrl_poly, ii))
    else {
        return Err(bad());
    };
    let (Some(old_prev), Some(old_next)) = (span_of(traced, ii - 1), span_of(traced, ii)) else {
        return Err(bad());
    };
    let [_, _, a_pt, k_pt] = prev_span;
    let [_, b_pt, _, _] = next_span;
    let (kc, a, b) = (coord(k_pt, axis), coord(a_pt, axis), coord(b_pt, axis));
    let set = |poly: &mut [Point3], i: usize, c: f64| -> Result<()> {
        let p = poly.get_mut(i).ok_or_else(bad)?;
        *p = with_coord(*p, axis, c);
        Ok(())
    };
    // the bound of a span moved to `new`, if it fits
    let fits = |old: [Point3; 4], new: [Point3; 4]| {
        bound
            .span_bound(old, new, dom)
            .filter(|c| *c == 0.0 || *c <= bound.budget)
    };
    let with = |span: [Point3; 4], j: usize, c: f64| {
        let mut out = span;
        if let Some(p) = out.get_mut(j) {
            *p = with_coord(*p, axis, c);
        }
        out
    };

    // 2. upstream's move: both neighbours onto the knot's coordinate
    let flat_a = fits(old_prev, with(prev_span, 2, kc));
    let flat_b = fits(old_next, with(next_span, 1, kc));
    if flat_a.is_some() && flat_b.is_some() {
        set(ctrl_poly, this_pt - 1, kc)?;
        set(ctrl_poly, this_pt + 1, kc)?;
        return Ok(());
    }

    // 3. collinear through the knot again, C1 with the knots' spacing
    if !(h_prev > 0.0 && h_next > 0.0 && h_prev.is_finite() && h_next.is_finite()) {
        return Ok(());
    }
    let inside = |c: f64| c.is_finite() && l <= c && c <= h;
    let new_b = kc + (kc - a) * (h_next / h_prev);
    let new_a = kc - (b - kc) * (h_prev / h_next);
    let move_b = if inside(new_b) && inside(a) {
        fits(old_next, with(next_span, 1, new_b))
    } else {
        None
    };
    let move_a = if inside(new_a) && inside(b) {
        fits(old_prev, with(prev_span, 2, new_a))
    } else {
        None
    };
    match (move_a, move_b) {
        (Some(ca), Some(cb)) if ca < cb => {
            set(ctrl_poly, this_pt - 1, new_a)?;
        }
        (_, Some(_)) => set(ctrl_poly, this_pt + 1, new_b)?,
        (Some(_), None) => set(ctrl_poly, this_pt - 1, new_a)?,
        // 4. nothing fits: no move for the collinearity
        (None, None) => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nurbs::b_spline_surface::p1::BSplineSurface;
    use crate::nurbs::math::CPoint;

    /// The plane `S(u, v) = (su u, sv v, 0)` over `[0, 1]^2` as a bilinear B-spline: its
    /// partials are `(su, 0, 0)` and `(0, sv, 0)` everywhere.
    fn plane(su: f64, sv: f64, budget: f64) -> MoveBound {
        let pw = vec![
            vec![
                CPoint::new(0.0, 0.0, 0.0, 1.0),
                CPoint::new(0.0, sv, 0.0, 1.0),
            ],
            vec![
                CPoint::new(su, 0.0, 0.0, 1.0),
                CPoint::new(su, sv, 0.0, 1.0),
            ],
        ];
        let s =
            BSplineSurface::from_data(1, 1, 1, 1, 3, 3, &pw, &[0., 0., 1., 1.], &[0., 0., 1., 1.])
                .expect("plane");
        MoveBound::of(&Kernel::default(), &Surface::from(s), budget).expect("bound")
    }

    const DOM: Extent2d = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);

    /// A two-span polygon whose middle knot is at `knot`, with neighbours `a` and `b`,
    /// mapped to a side of the domain: `side` 0..4 is v = 0, v = 1, u = 0, u = 1, the
    /// polygon given for v = 0 (`x` along, `y` across).
    fn poly(side: usize, a: (f64, f64), knot: (f64, f64), b: (f64, f64)) -> Vec<Point3> {
        let pts = [(0.1, 0.3), (0.2, 0.2), a, knot, b, (0.8, 0.2), (0.9, 0.3)];
        pts.iter().map(|&(x, y)| map(side, x, y)).collect()
    }

    fn map(side: usize, x: f64, y: f64) -> Point3 {
        match side {
            0 => Point3::new(x, y, 0.0),
            1 => Point3::new(x, 1.0 - y, 0.0),
            2 => Point3::new(y, x, 0.0),
            _ => Point3::new(1.0 - y, x, 0.0),
        }
    }

    /// `(along, across)` of a mapped point.
    fn unmap(side: usize, p: Point3) -> (f64, f64) {
        match side {
            0 => (p.x, p.y),
            1 => (p.x, 1.0 - p.y),
            2 => (p.y, p.x),
            _ => (p.y, 1.0 - p.x),
        }
    }

    const TOL: f64 = 1e-6;

    fn fix(p: &mut [Point3], knots: &[f64], b: &MoveBound) -> bool {
        fix_boundary_curves_bounded(p, knots, &DOM, TOL, TOL, b).expect("fix")
    }

    /// A curve along the boundary (neighbours a little off it) is flattened onto it as
    /// upstream flattens it: an exact isoline, on every side.
    #[test]
    fn an_isoline_is_snapped_on_every_side() {
        for side in 0..4 {
            let b = plane(10.0, 10.0, 1e-5);
            let mut p = poly(side, (0.4, 1e-7), (0.5, 0.5e-6), (0.6, -1e-7));
            assert!(fix(&mut p, &[0.0, 1.0, 2.0], &b));
            for (i, q) in p.iter().enumerate().take(5).skip(2) {
                let (_, c) = unmap(side, *q);
                assert!((c - 0.5e-6).abs() < 1e-15, "side {side} point {i}: {c}");
            }
        }
    }

    /// A curve reaching the boundary at a shallow angle, whose next span's cross tangent
    /// was zeroed: flattening would move it too far, so the next neighbour goes onto the
    /// line through the previous one and the knot (C1 with the spacing), on every side.
    #[test]
    fn a_shallow_arrival_is_made_collinear_on_every_side() {
        for side in 0..4 {
            let b = plane(100.0, 100.0, 1e-5);
            let (hp, hn) = (9.0, 0.05);
            let mut p = poly(side, (0.45, 9e-6), (0.5, 1e-7), (0.501, 1e-7));
            let before_a = p[2];
            assert!(fix(&mut p, &[0.0, hp, hp + hn], &b));
            assert_eq!(p[2], before_a, "side {side}: the previous neighbour stays");
            let (_, a) = unmap(side, p[2]);
            let (_, k) = unmap(side, p[3]);
            let (_, n) = unmap(side, p[4]);
            assert!(
                ((n - k) * hp - (k - a) * hn).abs() < 1e-9 * hp * (k - a).abs(),
                "side {side}: not C1"
            );
            assert!(n >= 0.0, "side {side}: left the domain");
            // the along coordinate is untouched
            assert_eq!(unmap(side, p[4]).0, 0.501);
        }
    }

    /// Where neither choice fits the budget, nothing moves.
    #[test]
    fn nothing_moves_past_the_budget() {
        let b = plane(100.0, 100.0, 1e-12);
        let mut p = poly(0, (0.45, 9e-6), (0.5, 1e-7), (0.55, 5e-6));
        let before = p.clone();
        assert!(fix(&mut p, &[0.0, 1.0, 2.0], &b));
        assert_eq!(p, before);
    }

    /// A polygon holding a number that is not finite is refused.
    #[test]
    fn a_nan_control_point_is_refused() {
        let b = plane(1.0, 1.0, 1e-5);
        let mut p = poly(0, (0.45, f64::NAN), (0.5, 0.0), (0.55, 0.3));
        assert!(fix_boundary_curves_bounded(&mut p, &[0.0, 1.0, 2.0], &DOM, TOL, TOL, &b).is_err());
        assert!(b.refused());
    }

    /// A neighbour outside the domain never stays there: with the budget for it, it is
    /// moved in (every control point ends in the domain, so the curve does); without, the
    /// curve is refused. On every side.
    #[test]
    fn an_outside_neighbour_is_moved_in_or_the_curve_refused() {
        for side in 0..4 {
            let b = plane(100.0, 100.0, 1e-2);
            let mut p = poly(side, (0.45, -1e-5), (0.5, 1e-7), (0.55, 5e-6));
            assert!(fix(&mut p, &[0.0, 1.0, 2.0], &b));
            for (i, q) in p.iter().enumerate() {
                assert!(
                    DOM.contains_point(crate::geom::vector2d::Point2::new(q.x, q.y), 0.0),
                    "side {side} point {i}: {q:?}"
                );
            }
            assert!(b.spent() > 0.0);
            let small = plane(100.0, 100.0, 1e-6);
            let mut p = poly(side, (0.45, -1e-5), (0.5, 1e-7), (0.55, 5e-6));
            assert!(
                fix_boundary_curves_bounded(&mut p, &[0.0, 1.0, 2.0], &DOM, TOL, TOL, &small)
                    .is_err()
            );
            assert!(small.refused(), "side {side}");
        }
    }

    /// A biquadratic bump over `[0, 1]^2`: curved, so its second partials are not zero.
    fn bump() -> Surface {
        let z = [[0.0, 0.3, 0.0], [0.4, 1.0, 0.2], [0.0, 0.5, 0.1]];
        let pw: Vec<Vec<CPoint>> = (0..3)
            .map(|i| {
                (0..3)
                    .map(|j| {
                        CPoint::new(
                            f64::from(i as u8) * 5.0,
                            f64::from(j as u8) * 4.0,
                            z[i][j] * 3.0,
                            1.0,
                        )
                    })
                    .collect()
            })
            .collect();
        let knots = [0., 0., 0., 1., 1., 1.];
        Surface::from(
            BSplineSurface::from_data(2, 2, 2, 2, 5, 5, &pw, &knots, &knots).expect("bump"),
        )
    }

    /// The Lipschitz bounds of a curved net and of an offset of it hold against the
    /// surfaces' own first derivatives on a grid over the box; outside the net's domain an
    /// offset has none.
    #[test]
    fn the_partial_bounds_hold_on_a_curved_net_and_its_offset() {
        use crate::surface::offset_surface::OffsetSurface;
        let k = Kernel::default();
        let net = bump();
        let off = Surface::from(OffsetSurface::new(&k, 0.8, net.clone(), true, 0).expect("offset"));
        for s in [&net, &off] {
            let b = MoveBound::of(&k, s, 1.0).expect("bound");
            let (lu, lv) = b
                .lipschitz(Ivl::new(0.2, 0.6), Ivl::new(0.3, 0.7))
                .expect("proved");
            for i in 0..=10 {
                for j in 0..=10 {
                    let uv = crate::geom::vector2d::Point2::new(
                        0.2 + 0.04 * f64::from(i),
                        0.3 + 0.04 * f64::from(j),
                    );
                    let (_, du, dv) = s
                        .evaluate_1st_derivatives(&k, uv, true, true)
                        .expect("eval");
                    assert!(
                        du.length() <= lu && dv.length() <= lv,
                        "{uv:?}: {} {} > {lu} {lv}",
                        du.length(),
                        dv.length()
                    );
                }
            }
        }
        let b = MoveBound::of(&k, &off, 1.0).expect("bound");
        assert!(b
            .lipschitz(Ivl::new(0.9, 1.1), Ivl::new(0.3, 0.7))
            .is_none());
    }

    /// The trace may accept a short span over the tolerance, leaving a negative budget: a
    /// curve that the fix-up does not move is still output, unchanged (on a boundary knot
    /// whose neighbours are already on it, and away from the boundary).
    #[test]
    fn an_unmoved_curve_is_kept_with_a_negative_budget() {
        let b = plane(10.0, 10.0, -1e-6);
        let mut p = poly(0, (0.4, 0.0), (0.5, 0.0), (0.6, 0.0));
        let before = p.clone();
        assert!(fix(&mut p, &[0.0, 1.0, 2.0], &b));
        assert_eq!(p, before);
        assert!(!b.refused());
        let mut q = poly(0, (0.4, 0.2), (0.5, 0.25), (0.6, 0.2));
        let before = q.clone();
        assert!(!fix(&mut q, &[0.0, 1.0, 2.0], &b));
        assert_eq!(q, before);
    }

    /// A domain that is not a number is refused before anything is clamped.
    #[test]
    fn a_nan_domain_is_refused() {
        let b = plane(1.0, 1.0, 1e-5);
        let mut p = poly(0, (0.4, 0.2), (0.5, 0.25), (0.6, 0.2));
        let dom = Extent2d::from_bounds(0.0, f64::NAN, 1.0, 1.0);
        assert!(fix_boundary_curves_bounded(&mut p, &[0.0, 1.0, 2.0], &dom, TOL, TOL, &b).is_err());
        assert!(b.refused());
    }

    /// The breakpoints must match the polygon.
    #[test]
    fn a_wrong_breakpoint_count_is_refused() {
        let b = plane(1.0, 1.0, 1e-5);
        let mut p = poly(0, (0.45, 0.1), (0.5, 0.0), (0.55, 0.1));
        assert!(fix_boundary_curves_bounded(&mut p, &[0.0, 1.0], &DOM, TOL, TOL, &b).is_err());
    }
}
