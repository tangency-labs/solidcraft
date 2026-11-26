//! solidcore's points and picks ([`Points`]): each the solidcore-ops call this crate's callers
//! made before the interface existed, its refusal's words kept whole.

use solidcore_ops::{measure, pick};
use solidcore_record::brep::Brep;
use solidcraft_kernel_api::points::{Distance, PointSetSummary, Points, PointsRefused};

use super::Solidcore;

/// solidcore's refusal, in its words.
fn refused(e: solidcore_ops::Error) -> PointsRefused {
    PointsRefused(e.to_string())
}

impl Points for Solidcore {
    fn point_distance(&self, a: [f64; 3], b: [f64; 3]) -> Result<Distance, PointsRefused> {
        measure::point_distance(a, b).map(|d| Distance { value: d.value, bound: d.bound }).map_err(refused)
    }

    fn points_meet(&self, a: [f64; 3], b: [f64; 3], tol: f64) -> Result<bool, PointsRefused> {
        measure::points_meet(a, b, tol).map_err(refused)
    }

    fn point_displacement(&self, from: [f64; 2], to: [f64; 2]) -> Result<[f64; 2], PointsRefused> {
        measure::point_displacement(from, to).map_err(refused)
    }

    fn centroid_and_mean_radius(&self, points: &[[f64; 2]]) -> Result<PointSetSummary, PointsRefused> {
        measure::centroid_and_mean_radius(points).map(|s| PointSetSummary { centroid: s.centroid, mean_distance: s.mean_distance }).map_err(refused)
    }

    fn polyline_2d_closest_point(&self, point: [f64; 2], points: &[[f64; 2]]) -> Result<Option<(f64, [f64; 2])>, PointsRefused> {
        measure::polyline_2d_closest_point(point, points).map_err(refused)
    }

    fn stored_face(&self, record: &Brep, face: usize, point: [f64; 3]) -> Result<bool, PointsRefused> {
        pick::stored_face(record, face, point).map_err(refused)
    }

    fn stored_edge(&self, record: &Brep, edge: usize, point: [f64; 3]) -> Result<bool, PointsRefused> {
        pick::stored_edge(record, edge, point).map_err(refused)
    }

    fn flush_contact_points(&self, target: &Brep, tool: &Brep) -> Option<Vec<[f64; 3]>> {
        let (target, _) = solidcore_ops::Body::from_record(target, true).ok()?;
        let (tool, _) = solidcore_ops::Body::from_record(tool, true).ok()?;
        solidcore_ops::flush_contact_points(&target, &tool).ok()
    }
}
