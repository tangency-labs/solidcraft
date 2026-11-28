//! The router's points and picks ([`solidcraft_kernel_api::Points`]), as
//! `solidcraft_kernel::points`: distances and centroids of points, a polyline's closest point,
//! stored picks checked against their face or edge, and flush contacts between bodies.

use solidcore_record::brep::Brep;
use solidcraft_kernel_api::{OpKind, Points};

pub use solidcraft_kernel_api::points::{
    ClassifiedPoint, Distance, HoleSpacing, PitchCircle, PointNormalFace, PointSetSummary, PointsRefused, SpacingPattern, UnitNormal,
};

use super::{Which, which};
use crate::backend::Solidcore;
use crate::body::Body;

/// The spacing of round items, each its centre and its axis direction: holes along a row or round
/// a bolt circle, measured centre to centre in the plane square to the axis.
///
/// # Errors
/// The kernel's refusal, in its words: fewer than two items, axes that are not parallel, two
/// items on one centre.
pub fn hole_spacing(items: &[([f64; 3], [f64; 3])]) -> Result<HoleSpacing, PointsRefused> {
    match which(OpKind::Points) {
        Which::Solidcore => Solidcore.hole_spacing(items),
    }
}

/// The distance from `a` to `b`, with its proved rounding bound.
pub fn point_distance(a: [f64; 3], b: [f64; 3]) -> Result<Distance, PointsRefused> {
    match which(OpKind::Points) {
        Which::Solidcore => Solidcore.point_distance(a, b),
    }
}

/// Whether `a` and `b` meet: their rounded distance is not more than `tol`.
pub fn points_meet(a: [f64; 3], b: [f64; 3], tol: f64) -> Result<bool, PointsRefused> {
    match which(OpKind::Points) {
        Which::Solidcore => Solidcore.points_meet(a, b, tol),
    }
}

/// The vector from `from` to `to`.
pub fn point_displacement(from: [f64; 2], to: [f64; 2]) -> Result<[f64; 2], PointsRefused> {
    match which(OpKind::Points) {
        Which::Solidcore => Solidcore.point_displacement(from, to),
    }
}

/// The centroid and mean radial distance of a non-empty planar point set.
pub fn centroid_and_mean_radius(points: &[[f64; 2]]) -> Result<PointSetSummary, PointsRefused> {
    match which(OpKind::Points) {
        Which::Solidcore => Solidcore.centroid_and_mean_radius(points),
    }
}

/// The distance from `point` to the polyline through `points`, and its closest point; `None`
/// for a polyline of no points.
pub fn polyline_2d_closest_point(point: [f64; 2], points: &[[f64; 2]]) -> Result<Option<(f64, [f64; 2])>, PointsRefused> {
    match which(OpKind::Points) {
        Which::Solidcore => Solidcore.polyline_2d_closest_point(point, points),
    }
}

/// Axis-aligned `(min, max)` bounds of a 2D polyline; `None` for an empty slice.
pub fn polyline_2d_bounds(points: &[[f64; 2]]) -> Result<Option<([f64; 2], [f64; 2])>, PointsRefused> {
    match which(OpKind::Points) {
        Which::Solidcore => Solidcore.polyline_2d_bounds(points),
    }
}

/// Intersection point of two 2D segments within `tol`, if they meet.
pub fn segment_2d_intersection(a1: [f64; 2], a2: [f64; 2], b1: [f64; 2], b2: [f64; 2], tol: f64) -> Result<Option<[f64; 2]>, PointsRefused> {
    match which(OpKind::Points) {
        Which::Solidcore => Solidcore.segment_2d_intersection(a1, a2, b1, b2, tol),
    }
}

/// Crossing points between two 2D polylines within `tol`.
pub fn polyline_2d_crossings(a: &[[f64; 2]], b: &[[f64; 2]], tol: f64) -> Result<Vec<[f64; 2]>, PointsRefused> {
    match which(OpKind::Points) {
        Which::Solidcore => Solidcore.polyline_2d_crossings(a, b, tol),
    }
}

/// Tangent contact point between a 2D line `(a, b)` and circle `(center, radius)` within `tol`.
pub fn tangent_contact_line_circle_2d(a: [f64; 2], b: [f64; 2], center: [f64; 2], radius: f64, tol: f64) -> Result<Option<[f64; 2]>, PointsRefused> {
    match which(OpKind::Points) {
        Which::Solidcore => Solidcore.tangent_contact_line_circle_2d(a, b, center, radius, tol),
    }
}

/// Tangent contact point between two 2D circles `(c1, r1)` and `(c2, r2)` within `tol`.
pub fn tangent_contact_circle_circle_2d(c1: [f64; 2], r1: f64, c2: [f64; 2], r2: f64, tol: f64) -> Result<Option<[f64; 2]>, PointsRefused> {
    match which(OpKind::Points) {
        Which::Solidcore => Solidcore.tangent_contact_circle_circle_2d(c1, r1, c2, r2, tol),
    }
}

/// Whether `point` still lies on face `face` of `record`, on its surface and within its trims,
/// to a thousandth of the body's box diagonal.
pub fn stored_face(record: &Brep, face: usize, point: [f64; 3]) -> Result<bool, PointsRefused> {
    match which(OpKind::Points) {
        Which::Solidcore => Solidcore.stored_face(record, face, point),
    }
}

/// Whether `point` still lies on edge `edge` of `record`, its closest-point distance within a
/// thousandth of the body's box diagonal.
pub fn stored_edge(record: &Brep, edge: usize, point: [f64; 3]) -> Result<bool, PointsRefused> {
    match which(OpKind::Points) {
        Which::Solidcore => Solidcore.stored_edge(record, edge, point),
    }
}

/// Centroids of `tool`'s planar faces that lie back to back on a planar face of `target`; `None`
/// where either record doesn't convert or the kernel fails.
pub fn flush_contact_points(target: &Body, tool: &Body) -> Option<Vec<[f64; 3]>> {
    match which(OpKind::Points) {
        Which::Solidcore => Solidcore.flush_contact_points(target.record(), tool.record()),
    }
}

/// `direction` made unit, however short it is: only a direction that is exactly zero or not finite
/// has none.
#[must_use]
pub fn unit_direction(direction: [f64; 3]) -> Option<[f64; 3]> {
    match which(OpKind::Points) {
        Which::Solidcore => Solidcore.unit_direction(direction),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_ops::measure;

    #[test]
    fn routed_points_are_solidcores() {
        let d = point_distance([0.0; 3], [3.0, 4.0, 0.0]).unwrap();
        let k = measure::point_distance([0.0; 3], [3.0, 4.0, 0.0]).unwrap();
        assert_eq!((d.value, d.bound), (k.value, k.bound));
        assert_eq!(
            points_meet([0.0; 3], [0.0, 0.0, 1e-12], 1e-9),
            measure::points_meet([0.0; 3], [0.0, 0.0, 1e-12], 1e-9).map_err(|e| PointsRefused(e.to_string()))
        );
        let pts = [[0.0, 0.0], [2.0, 0.0], [1.0, 3.0]];
        let s = centroid_and_mean_radius(&pts).unwrap();
        let k = measure::centroid_and_mean_radius(&pts).unwrap();
        assert_eq!((s.centroid, s.mean_distance), (k.centroid, k.mean_distance));
        assert_eq!(polyline_2d_closest_point([1.0, 1.0], &pts).unwrap(), measure::polyline_2d_closest_point([1.0, 1.0], &pts).unwrap());
        for v in [[3.0, 4.0, 0.0], [1e-300, 0.0, 0.0], [0.0; 3], [f64::NAN, 1.0, 0.0]] {
            assert_eq!(unit_direction(v), measure::unit_direction(v));
        }
        let z = [0.0, 0.0, 1.0];
        let row = [([0.0; 3], z), ([10.0, 0.0, 0.0], z), ([30.0, 0.0, 4.0], z)];
        let (s, k) = (hole_spacing(&row).unwrap(), solidcore_ops::hole_spacing::spacing(&row).unwrap());
        assert_eq!((s.pattern, &s.order, &s.gaps, s.span, s.even, s.pitch), (SpacingPattern::Line, &k.order, &k.gaps, k.span, k.even, k.pitch));
        assert_eq!((s.gaps.as_slice(), s.span), ([10.0, 20.0].as_slice(), 30.0));
        let words = solidcore_ops::hole_spacing::spacing(&row[..1]).unwrap_err().to_string();
        assert_eq!(hole_spacing(&row[..1]), Err(PointsRefused(words)));
        // a refusal carries solidcore's words whole
        let words = measure::point_distance([f64::NAN, 0.0, 0.0], [0.0; 3]).unwrap_err().to_string();
        assert_eq!(point_distance([f64::NAN, 0.0, 0.0], [0.0; 3]), Err(PointsRefused(words)));
    }
}
