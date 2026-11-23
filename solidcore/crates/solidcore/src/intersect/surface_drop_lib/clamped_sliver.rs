//! Spans of a dropped curve that clamping into the domain collapses to a point (not
//! upstream).
//!
//! Upstream: none. The drop's span loop (`SmSurface::DropCurve`,
//! `source/SMLib/src/SmSurfaceDropLib.cpp`) adds every span it accepts with
//! `sm_AddToCurve`, which this guards.
//!
//! | Rust                       | Upstream |
//! |----------------------------|----------|
//! | [`is_clamped_sliver`]      | none     |
//! | [`stretch_deviation`]      | none     |
//!
//! **The defect.** `sm_AddToCurve` makes a span's Bezier polygon from its Hermite data:
//! `P1 = uv(t0)`, `P2 = P1 + h uv'(t0) / 3`, `P3 = P4 - h uv'(t1) / 3`, `P4 = uv(t1)`, and
//! then clamps `P1` and `P4` into the domain only. Where a curve end lies a little outside
//! the domain (by less than the drop's tolerance), the span that covers that sliver gets
//! both ends clamped onto the boundary while `P2` and `P3` keep the unclamped tangent:
//! the span runs out of the domain and back, an S whose UV image doubles back on itself.
//! The fix-up of the flush (`sm_FixBoundaryCurves`) moves the polygon onto the boundary
//! only where the curve runs along it, so the S stays in the trim. The drop's own span
//! test does not see it: it clamps its samples into the domain before measuring them.
//!
//! **The rule here.** A span is a *clamped sliver* when
//! 1. neither end is on a pole (the pole's dog-leg straightening moves those polygons),
//! 2. a control point of its polygon lies outside the domain (the span leaves it: a span
//!    clamping never moves is a genuine piece of the trim, however short),
//! 3. the box of its four control points, each clamped into the domain, is no wider than
//!    the drop's UV tolerance in either coordinate: clamping is monotone in each
//!    coordinate and the span lies in the convex hull of its polygon, so the span clamped
//!    into the domain lies in that box (exact: the box is of the polygon `sm_AddToCurve`
//!    would emit, made by the same operations), and
//! 4. the curve piece it replaces lies in a box ([`Curve::certified_bounding_box`]) whose
//!    diagonal, rounded up, is within the drop's 3D tolerance.
//!
//! Such a span carries no trim information inside the domain: its clamped image is one
//! UV point to within the UV tolerance. The drop leaves it out where it is the first or
//! the last span of a curve, and the neighbouring span takes over its parameter range:
//! the polygon is unchanged and the knot at the joint moves to the sliver's far end. A
//! sliver between two spans is added as before, and so is any span after a first one
//! was left out, so a curve keeps at least one span: at most one first span is left
//! out, and a last span is left out only after another was added. The polygon then joins its neighbour at
//! the clamped end point it already had.
//!
//! **The bound.** The stretched span keeps its image and is reparametrised linearly, so
//! at every parameter `t` it shows the point the old span showed at some `s` with
//! `|s - t| <= h`, `h` the sliver's parameter length. Its distance from the curve at `t`
//! is at most the old span's distance at `s` plus `|C(s) - C(t)| <= h sup |C'|`, the
//! supremum over the stretched range ([`Curve::speed_bound`], from the curve's own data).
//! [`stretch_deviation`] gives that `h sup |C'|`, rounded up, for the drop to add to the
//! deviation it reports. A curve with no such bound keeps its sliver.
//!
//! Deviations: not upstream (above).

use crate::blend::fillet_section::interval::round_up;
use crate::curve::curve::Curve;
use crate::error::Result;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::vector2d::Point2;
use crate::kernel::Kernel;

use super::p1::DropPt;

/// Whether the span from `start` to `end` is a clamped sliver (module docs): its polygon
/// leaves `uv_domain`, clamped into it spans at most `uv_tol_x` by `uv_tol_y`, and the curve piece
/// over it is proved within `tol_3d` across. `false` for a span of no length, a span with
/// an end on a pole, and a curve with no certified box or speed bound.
///
/// # Errors
///
/// Those of the curve's certified box.
#[allow(clippy::too_many_arguments)] // the span, the domain and the drop's tolerances
pub(crate) fn is_clamped_sliver(
    k: &Kernel,
    curve: &Curve,
    uv_domain: &Extent2d,
    start: &DropPt,
    end: &DropPt,
    uv_tol_x: f64,
    uv_tol_y: f64,
    tol_3d: f64,
) -> Result<bool> {
    if start.on_pole != Some(false) || end.on_pole != Some(false) {
        return Ok(false);
    }
    // the polygon of `sm_AddToCurve`, made the same way
    let scale = end.t - start.t;
    if scale.is_nan() || scale <= 0.0 {
        return Ok(false);
    }
    let p1 = start.uv_pd[0];
    let d1 = start.uv_pd[1] * scale;
    let p4 = end.uv_pd[0];
    let d2 = end.uv_pd[1] * scale;
    let p2 = p1 + d1 / 3.0;
    let p3 = p4 - d2 / 3.0;
    let polygon = [p1, p2, p3, p4];
    if !polygon.iter().any(|&p| uv_domain.clamp_point(p) != p) {
        return Ok(false);
    }
    if !clamped_box_within(uv_domain, polygon, uv_tol_x, uv_tol_y) {
        return Ok(false);
    }
    if curve
        .speed_bound(&Extent1d::from_min_max(start.t, end.t))
        .is_none()
    {
        return Ok(false);
    }
    let Some(piece) = curve.certified_bounding_box(k, &Extent1d::from_min_max(start.t, end.t))?
    else {
        return Ok(false);
    };
    let diagonal = piece.size().length();
    Ok(diagonal.is_finite() && round_up(round_up(diagonal)) <= tol_3d)
}

/// Whether the points clamped into `domain` lie in a box at most `tol_x` by `tol_y`.
fn clamped_box_within(domain: &Extent2d, points: [Point2; 4], tol_x: f64, tol_y: f64) -> bool {
    let clamped = points.map(|p| domain.clamp_point(p));
    let extent = |f: fn(&Point2) -> f64| {
        let (lo, hi) = clamped
            .iter()
            .map(f)
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| {
                (lo.min(x), hi.max(x))
            });
        hi - lo
    };
    let width = extent(|p| p.x);
    let height = extent(|p| p.y);
    // a NaN coordinate fails both comparisons
    width <= tol_x && height <= tol_y
}

/// The most a span's distance from `curve` grows when its parameter range is stretched
/// by `shift` to cover `range` (module docs, the bound): `shift sup |C'|` over `range`,
/// rounded up; `None` without a speed bound.
pub(crate) fn stretch_deviation(curve: &Curve, range: &Extent1d, shift: f64) -> Option<f64> {
    let speed = curve.speed_bound(range)?;
    let bound = round_up(round_up(shift.abs()) * speed);
    bound.is_finite().then_some(bound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::line::{BoundedLineOptions, Line};
    use crate::geom::vector2d::Vec2;
    use crate::geom::vector3d::Vec3;

    fn unit() -> Extent2d {
        Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0)
    }

    fn pt(t: f64, uv: Point2, tan: Vec2) -> DropPt {
        let mut d = DropPt::new(t, Some(false));
        d.uv_pd = [uv, tan];
        d.on_pole = Some(false);
        d
    }

    /// The line `t len X` over `[0, 1]`: speed `len`.
    fn line(len: f64) -> Curve {
        Curve::from(Line::bounded(
            Vec3::ZERO,
            Vec3::X,
            &Extent1d::from_min_max(0.0, 1.0),
            &BoundedLineOptions {
                scale: len,
                ..BoundedLineOptions::default()
            },
        ))
    }

    /// The face 7 sliver: a span from just outside `v = 0` to the boundary whose
    /// tangent runs across it clamps to one point.
    #[test]
    fn a_span_outside_the_boundary_clamps_to_a_sliver() {
        let k = Kernel::default();
        let h = 4.2e-8;
        let start = pt(0.5 - h, Point2::new(0.25, -2.1e-8), Vec2::new(0.0, 0.5));
        let end = pt(0.5, Point2::new(0.25, 0.0), Vec2::new(0.0, 0.5));
        let c = line(1.0e-6);
        assert!(is_clamped_sliver(&k, &c, &unit(), &start, &end, 1e-9, 1e-9, 1e-5).unwrap());
    }

    /// A span on the boundary that runs along it is kept: its clamped image is long.
    #[test]
    fn a_span_along_the_boundary_is_kept() {
        let k = Kernel::default();
        let start = pt(0.0, Point2::new(0.25, 0.0), Vec2::new(0.5, 0.0));
        let end = pt(0.1, Point2::new(0.30, 0.0), Vec2::new(0.5, 0.0));
        let c = line(1.0e-6);
        assert!(!is_clamped_sliver(&k, &c, &unit(), &start, &end, 1e-9, 1e-9, 1e-5).unwrap());
    }

    /// A span that touches the boundary and leaves it into the domain is kept.
    #[test]
    fn a_span_leaving_the_boundary_inward_is_kept() {
        let k = Kernel::default();
        let start = pt(0.0, Point2::new(0.25, 0.0), Vec2::new(0.0, 0.5));
        let end = pt(0.1, Point2::new(0.25, 0.05), Vec2::new(0.0, 0.5));
        let c = line(1.0e-6);
        assert!(!is_clamped_sliver(&k, &c, &unit(), &start, &end, 1e-9, 1e-9, 1e-5).unwrap());
    }

    /// A tiny span that stays inside the domain is kept: clamping moves none of its
    /// points, so it is a genuine piece of the trim however short.
    #[test]
    fn a_tiny_span_inside_the_domain_is_kept() {
        let k = Kernel::default();
        let h = 4.2e-8;
        let start = pt(0.5 - h, Point2::new(0.25, 0.5), Vec2::new(0.0, 0.5));
        let end = pt(0.5, Point2::new(0.25, 0.5 + 2.1e-8), Vec2::new(0.0, 0.5));
        let c = line(1.0e-6);
        assert!(!is_clamped_sliver(&k, &c, &unit(), &start, &end, 1e-9, 1e-9, 1e-5).unwrap());
    }

    /// A sliver in UV whose curve piece is longer than the 3D tolerance is kept.
    #[test]
    fn a_long_curve_piece_is_kept() {
        let k = Kernel::default();
        let h = 4.2e-8;
        let start = pt(0.5 - h, Point2::new(0.25, -2.1e-8), Vec2::new(0.0, 0.5));
        let end = pt(0.5, Point2::new(0.25, 0.0), Vec2::new(0.0, 0.5));
        // a line of length 1e3 over [0, 1]: the piece is 4.2e-5 long
        let c = line(1.0e3);
        assert!(!is_clamped_sliver(&k, &c, &unit(), &start, &end, 1e-9, 1e-9, 1e-5).unwrap());
    }

    /// An end on a pole keeps the span (the dog-leg straightening owns it).
    #[test]
    fn a_span_on_a_pole_is_kept() {
        let k = Kernel::default();
        let h = 4.2e-8;
        let mut start = pt(0.5 - h, Point2::new(0.25, -2.1e-8), Vec2::new(0.0, 0.5));
        start.on_pole = Some(true);
        let end = pt(0.5, Point2::new(0.25, 0.0), Vec2::new(0.0, 0.5));
        let c = line(1.0e-6);
        assert!(!is_clamped_sliver(&k, &c, &unit(), &start, &end, 1e-9, 1e-9, 1e-5).unwrap());
    }

    /// The stretch bound is the shift times the line's speed, rounded up.
    #[test]
    fn stretch_bound_is_shift_times_speed() {
        let c = line(2.0);
        let b = stretch_deviation(&c, &Extent1d::from_min_max(0.0, 1.0), 1e-3).unwrap();
        assert!(b >= 2e-3);
        assert!(b <= 2e-3 * (1.0 + 1e-12));
    }
}
