//! The router's curves and fitting ([`solidcraft_kernel_api::Curves`]), as
//! `solidcraft_kernel::curves`: free curves queried, fit and control splines checked and fitted,
//! pcurves evaluated, point lists matched.

use solidcore_record::brep::{Curve, Curve2};
use solidcore_record::projection::PlanarSpline;
use solidcore_record::{Plane, Vec2};
use solidcraft_kernel_api::{Curves, OpKind};

pub use solidcore_record::curve_query::{Crossing, CubicPiece, CubicStanding, CurvePoint, CurveRayPoint, FitSplineBounds, PlanePart, PlaneSection};
pub use solidcraft_kernel_api::curves::{CurveRefused, CurveSample};

use super::{Which, which};
use crate::backend::Solidcore;

/// Where each piece (a curve over its interval) crosses `plane`, in the plane's coordinates.
pub fn piece_plane_crossings(pieces: &[(&Curve, [f64; 2])], plane: &Plane) -> Result<Vec<Vec2>, CurveRefused> {
    match which(OpKind::Curves) {
        Which::Solidcore => Solidcore.piece_plane_crossings(pieces, plane),
    }
}

/// Where `curve` over `interval` meets `plane`: lying in it, or its isolated points and parts.
pub fn plane_section(curve: &Curve, interval: [f64; 2], plane: &Plane) -> Result<PlaneSection, CurveRefused> {
    match which(OpKind::Curves) {
        Which::Solidcore => Solidcore.plane_section(curve, interval, plane),
    }
}

/// The point of `curve` nearest `p`, within `tolerance`; `None` where the solve finds none.
pub fn closest_point(curve: &Curve, p: [f64; 3], tolerance: f64) -> Result<Option<CurvePoint>, CurveRefused> {
    match which(OpKind::Curves) {
        Which::Solidcore => Solidcore.closest_point(curve, p, tolerance),
    }
}

/// The isolated points where `curve` over `interval` crosses `plane`.
pub fn plane_crossings(curve: &Curve, interval: [f64; 2], plane: &Plane) -> Result<Vec<Vec2>, CurveRefused> {
    match which(OpKind::Curves) {
        Which::Solidcore => Solidcore.plane_crossings(curve, interval, plane),
    }
}

/// Where `a` and `b` cross within `tolerance`, in space or as seen `along` a direction.
pub fn crossings(a: &Curve, b: &Curve, along: Option<[f64; 3]>, tolerance: f64) -> Result<Vec<Crossing>, CurveRefused> {
    match which(OpKind::Curves) {
        Which::Solidcore => Solidcore.crossings(a, b, along, tolerance),
    }
}

/// `a` followed by `b` as one curve, their meeting ends within `tolerance`.
pub fn join_curves(a: &Curve, b: &Curve, tolerance: f64) -> Result<Curve, CurveRefused> {
    match which(OpKind::Curves) {
        Which::Solidcore => Solidcore.join_curves(a, b, tolerance),
    }
}

/// The cubic Bézier through `points` as pieces a model keeps, each with its standing.
pub fn cubic_pieces(points: [[f64; 3]; 4]) -> Result<Vec<CubicPiece>, CurveRefused> {
    match which(OpKind::Curves) {
        Which::Solidcore => Solidcore.cubic_pieces(points),
    }
}

/// The arc from `points[0]` through `points[1]` to `points[2]`.
pub fn arc_through(points: [[f64; 3]; 3]) -> Result<Curve, CurveRefused> {
    match which(OpKind::Curves) {
        Which::Solidcore => Solidcore.arc_through(points),
    }
}

/// The point of `curve` nearest the ray from `origin` along `direction`, within `tolerance`.
pub fn closest_to_ray(curve: &Curve, origin: [f64; 3], direction: [f64; 3], tolerance: f64) -> Result<Option<CurveRayPoint>, CurveRefused> {
    match which(OpKind::Curves) {
        Which::Solidcore => Solidcore.closest_to_ray(curve, origin, direction, tolerance),
    }
}

/// Exact arc length of `curve` over `interval` (or its natural range when `None`).
pub fn arc_length(curve: &Curve, interval: Option<[f64; 2]>) -> Result<f64, CurveRefused> {
    match which(OpKind::Curves) {
        Which::Solidcore => Solidcore.arc_length(curve, interval),
    }
}

/// Sample `curve` over `interval` (or its natural range when `None`) at `distance` arc length from
/// its start (or from its end when `from_end` is `true`).
pub fn point_at_arc_length(curve: &Curve, interval: Option<[f64; 2]>, from_end: bool, distance: f64) -> Result<CurveSample, CurveRefused> {
    match which(OpKind::Curves) {
        Which::Solidcore => Solidcore.point_at_arc_length(curve, interval, from_end, distance),
    }
}

/// What a fit spline of cubic `pieces` certifies against the points `near` its fit points.
pub fn fit_spline_bounds(pieces: &[[[f64; 3]; 4]], near: &[(usize, [f64; 3])]) -> Result<FitSplineBounds, CurveRefused> {
    match which(OpKind::Curves) {
        Which::Solidcore => Solidcore.fit_spline_bounds(pieces, near),
    }
}

/// Splines through `p` (closed when `periodic`), each at most `limit` points.
pub fn fit_splines(p: &[Vec2], periodic: bool, limit: usize) -> Result<Vec<PlanarSpline>, CurveRefused> {
    match which(OpKind::Curves) {
        Which::Solidcore => Solidcore.fit_splines(p, periodic, limit),
    }
}

/// Whether `n` control points of `degree` with explicit clamped `knots` and positive `weights`
/// make a spline, at most `limit` points.
pub fn check_control(n: usize, degree: usize, knots: &[f64], weights: &[f64], limit: usize) -> Result<(), CurveRefused> {
    match which(OpKind::Curves) {
        Which::Solidcore => Solidcore.check_control(n, degree, knots, weights, limit),
    }
}

/// Control points with their weights as homogeneous points, normalised by the largest weight
/// without changing the curve.
pub fn homogeneous_controls(p: &[Vec2], weights: &[f64]) -> Result<Vec<[f64; 4]>, CurveRefused> {
    match which(OpKind::Curves) {
        Which::Solidcore => Solidcore.homogeneous_controls(p, weights),
    }
}

/// A spline's knots and weights for its reversed curve, its knot spacings kept exactly.
pub fn reversed_metadata(knots: &[f64], weights: &[f64]) -> (Vec<f64>, Vec<f64>) {
    match which(OpKind::Curves) {
        Which::Solidcore => Solidcore.reversed_metadata(knots, weights),
    }
}

/// The point or certified complete line a control spline's image is, to `roundoff`; `None`
/// for a planar curve.
pub fn linear_image(p: &[Vec2], degree: usize, knots: &[f64], weights: &[f64], roundoff: f64) -> Result<Option<[Vec2; 2]>, CurveRefused> {
    match which(OpKind::Curves) {
        Which::Solidcore => Solidcore.linear_image(p, degree, knots, weights, roundoff),
    }
}

/// Whether `p` closes, with distinct samples and regular chords, as a cyclic fit needs.
pub fn check_periodic(p: &[Vec2]) -> Result<(), CurveRefused> {
    match which(OpKind::Curves) {
        Which::Solidcore => Solidcore.check_periodic(p),
    }
}

/// The cubic Bézier pieces of the spline through `p` (closed when `periodic`).
pub fn fit_beziers(p: &[Vec2], periodic: bool) -> Result<Vec<[Vec2; 4]>, CurveRefused> {
    match which(OpKind::Curves) {
        Which::Solidcore => Solidcore.fit_beziers(p, periodic),
    }
}

/// The natural fit's Hermite pieces through `p`, keeping its chord-length parameters.
pub fn natural_spans(p: &[Vec2]) -> Result<Vec<(Vec2, Vec2, Vec2, Vec2)>, CurveRefused> {
    match which(OpKind::Curves) {
        Which::Solidcore => Solidcore.natural_spans(p),
    }
}

/// Whether `new` are `old` in order, each within `within`, spending at most `work` steps. False
/// includes an ambiguous match and exhausted work; it never means the two are equal.
pub fn ordered_nearby_points(old: &[Vec2], new: &[Vec2], within: f64, work: usize) -> Result<bool, CurveRefused> {
    match which(OpKind::Curves) {
        Which::Solidcore => Solidcore.ordered_nearby_points(old, new, within, work),
    }
}

/// The point of pcurve `c` at `t`; `None` outside its interval or for a malformed record.
pub fn pcurve_eval(c: &Curve2, t: f64) -> Option<[f64; 2]> {
    match which(OpKind::Curves) {
        Which::Solidcore => Solidcore.pcurve_eval(c, t),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_ops::{curve, fit_projection};

    #[test]
    fn routed_curves_are_solidcores() {
        let arc = arc_through([[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]]).unwrap();
        assert_eq!(arc, curve::arc_through([[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]]).unwrap());
        let near = closest_point(&arc, [0.0, 2.0, 0.0], 1e-9).unwrap();
        assert_eq!(near, curve::closest_point(&arc, [0.0, 2.0, 0.0], 1e-9).unwrap());
        let len = arc_length(&arc, None).unwrap();
        assert_eq!(len, curve::arc_length(&arc, None).unwrap());
        let sample = point_at_arc_length(&arc, None, false, len * 0.5).unwrap();
        assert_eq!(sample.point, curve::point_at_arc_length(&arc, None, false, len * 0.5).unwrap().point);
        let cubic = [[0.0, 0.0, 0.0], [1.0, 2.0, 0.0], [3.0, 2.0, 0.0], [4.0, 0.0, 0.0]];
        assert_eq!(cubic_pieces(cubic).unwrap(), curve::cubic_pieces(cubic).unwrap());
        let pts = [Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0), Vec2::new(2.0, 0.0), Vec2::new(3.0, 1.0)];
        assert_eq!(fit_beziers(&pts, false).unwrap(), fit_projection::fit_beziers(&pts, false).unwrap());
        assert_eq!(fit_splines(&pts, false, 100).unwrap(), fit_projection::fit_splines(&pts, false, 100).unwrap());
        // a refusal carries solidcore's words whole
        let words = curve::arc_through([[0.0; 3], [0.0; 3], [0.0; 3]]).unwrap_err().to_string();
        assert_eq!(arc_through([[0.0; 3], [0.0; 3], [0.0; 3]]), Err(CurveRefused(words)));
    }
}
