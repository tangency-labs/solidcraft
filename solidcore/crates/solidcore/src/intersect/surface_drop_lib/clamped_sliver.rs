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
//! | [`stretched`]              | none     |
//! | [`polygon_in_domain`]      | none     |
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
//! the last span of a curve, and the neighbouring span takes over its parameter range
//! ([`stretched`]): the same polygon, so the span joins its neighbour at the clamped end
//! point it already had, over the neighbour's range and the sliver's. A sliver between
//! two spans is added as before, and so is a sliver whose stretched neighbour's polygon
//! leaves the domain: that neighbour has the same defect, and stretched to the curve's end
//! it would lose the flush's fix-up of the knot it shared with the sliver (the neighbours
//! of an interior knot on the boundary move onto it; those of an end knot are only
//! clamped), which can make its fold larger than the sliver's. Only a neighbour whose
//! polygon neither the clamp nor the fix-up moves takes over. Any span after a first one
//! was left out is added as before: at
//! most one first span is left out, and a last span is left out only after another was
//! added. A sliver is left out only where the trace has no bounded boundary fix-up (whose
//! budget each span added spends).
//!
//! **The span test.** Stretching reparametrises the neighbour, which moves the point it
//! shows at each curve parameter, so the stretched span is held to the trace's own test
//! of every span it accepts (`DropPt::test_span_accuracy`, and the sphere check that a
//! span is not proved off the curve), with no pass for a short span; the drop reports the
//! larger of what that test measured and the deviation it had. Where the stretched span
//! fails, the sliver is kept: a first span is added back ahead of its neighbour, and a
//! last span is added as before. A first span left out that no span followed is added
//! back at the end of its curve, so a curve keeps every span it had but those stretched
//! over.
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
/// an end on a pole, and a curve with no certified box.
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
    let Some(polygon) = polygon(start, end) else {
        return Ok(false);
    };
    if !polygon.iter().any(|&p| uv_domain.clamp_point(p) != p) {
        return Ok(false);
    }
    if !clamped_box_within(uv_domain, polygon, uv_tol_x, uv_tol_y) {
        return Ok(false);
    }
    let Some(piece) = curve.certified_bounding_box(k, &Extent1d::from_min_max(start.t, end.t))?
    else {
        return Ok(false);
    };
    let diagonal = piece.size().length();
    Ok(diagonal.is_finite() && round_up(round_up(diagonal)) <= tol_3d)
}

/// The Bezier polygon `sm_AddToCurve` makes of the span from `start` to `end`, before its
/// clamp, made the same way; `None` for a span of no length.
fn polygon(start: &DropPt, end: &DropPt) -> Option<[Point2; 4]> {
    let scale = end.t - start.t;
    if scale.is_nan() || scale <= 0.0 {
        return None;
    }
    let p1 = start.uv_pd[0];
    let d1 = start.uv_pd[1] * scale;
    let p4 = end.uv_pd[0];
    let d2 = end.uv_pd[1] * scale;
    let p2 = p1 + d1 / 3.0;
    let p3 = p4 - d2 / 3.0;
    Some([p1, p2, p3, p4])
}

/// Whether the polygon `sm_AddToCurve` makes of the span from `start` to `end` lies in
/// `uv_domain`, so that neither its clamp nor the flush's fix-up moves it.
pub(crate) fn polygon_in_domain(uv_domain: &Extent2d, start: &DropPt, end: &DropPt) -> bool {
    polygon(start, end).is_some_and(|q| q.iter().all(|&p| uv_domain.clamp_point(p) == p))
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

/// The span from `start` to `end` stretched to run over `from..to`, a range that
/// contains `start.t..end.t` (module docs): the same ends, with `t` moved to `from` and
/// `to`, and each tangent scaled by `(end.t - start.t) / (to - from)`, so the polygon
/// `sm_AddToCurve` makes of it is that of the span (to the rounding of the scaled
/// tangents) over the longer range.
pub(crate) fn stretched(start: &DropPt, end: &DropPt, from: f64, to: f64) -> (DropPt, DropPt) {
    let ratio = (end.t - start.t) / (to - from);
    let mut s = *start;
    let mut e = *end;
    s.t = from;
    e.t = to;
    s.uv_pd[1] = start.uv_pd[1] * ratio;
    e.uv_pd[1] = end.uv_pd[1] * ratio;
    (s, e)
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

    /// A neighbour whose tangent leaves the domain at its clamped first point does not
    /// take over a sliver; one whose polygon lies in the domain does.
    #[test]
    fn only_a_polygon_in_the_domain_takes_over() {
        let start = pt(0.0, Point2::new(0.0, 0.0), Vec2::new(-0.3, 0.6));
        let end = pt(0.1, Point2::new(0.0, 0.05), Vec2::new(0.0, 0.5));
        assert!(!polygon_in_domain(&unit(), &start, &end));
        let start = pt(0.0, Point2::new(0.0, 0.0), Vec2::new(0.3, 0.6));
        assert!(polygon_in_domain(&unit(), &start, &end));
        // a span of no length has no polygon
        assert!(!polygon_in_domain(&unit(), &start, &start));
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

    /// A stretched span keeps its ends and its polygon: each tangent times the span's
    /// length is unchanged, to rounding.
    #[test]
    fn a_stretched_span_keeps_its_polygon() {
        let start = pt(0.5, Point2::new(0.25, 0.0), Vec2::new(0.3, 0.5));
        let end = pt(0.75, Point2::new(0.4, 0.2), Vec2::new(0.6, -0.1));
        let (s, e) = stretched(&start, &end, 0.5 - 4.2e-8, 0.75);
        assert_eq!((s.t, e.t), (0.5 - 4.2e-8, 0.75));
        assert_eq!((s.uv_pd[0], e.uv_pd[0]), (start.uv_pd[0], end.uv_pd[0]));
        for (new, old) in [(s, start), (e, end)] {
            let a = new.uv_pd[1] * (e.t - s.t);
            let b = old.uv_pd[1] * (end.t - start.t);
            assert!(
                (a - b).length() <= 4.0 * f64::EPSILON * b.length(),
                "{a:?} {b:?}"
            );
        }
    }
}
