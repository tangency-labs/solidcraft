//! solidcore's points and picks ([`Points`]): each the solidcore-ops call this crate's callers
//! made before the interface existed, its refusal's words kept whole.

use solidcore_ops::{hole_spacing, measure, pick};
use solidcore_record::brep::Brep;
use solidcraft_kernel_api::points::{Distance, HoleSpacing, PitchCircle, PointSetSummary, Points, PointsRefused, SpacingPattern};

use super::Solidcore;

/// solidcore's refusal, in its words.
fn refused(e: solidcore_ops::Error) -> PointsRefused {
    PointsRefused(e.to_string())
}

impl Points for Solidcore {
    fn hole_spacing(&self, items: &[([f64; 3], [f64; 3])]) -> Result<HoleSpacing, PointsRefused> {
        let s = hole_spacing::spacing(items).map_err(refused)?;
        Ok(HoleSpacing {
            pattern: match s.pattern {
                hole_spacing::Pattern::Line => SpacingPattern::Line,
                hole_spacing::Pattern::Circle => SpacingPattern::Circle,
                hole_spacing::Pattern::None => SpacingPattern::None,
            },
            order: s.order,
            gaps: s.gaps,
            gap_points: s.gap_points,
            span: s.span,
            even: s.even,
            pitch: s.pitch,
            odd_gaps: s.odd_gaps,
            circle: s.circle.map(|c| PitchCircle {
                diameter: c.diameter,
                centre: c.centre,
                angles_deg: c.angles_deg,
                angle_pitch_deg: c.angle_pitch_deg,
            }),
        })
    }

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

    fn polyline_2d_bounds(&self, points: &[[f64; 2]]) -> Result<Option<([f64; 2], [f64; 2])>, PointsRefused> {
        measure::polyline_2d_bounds(points).map_err(refused)
    }

    fn segment_2d_intersection(&self, a1: [f64; 2], a2: [f64; 2], b1: [f64; 2], b2: [f64; 2], tol: f64) -> Result<Option<[f64; 2]>, PointsRefused> {
        measure::segment_2d_intersection(a1, a2, b1, b2, tol).map_err(refused)
    }

    fn polyline_2d_crossings(&self, a: &[[f64; 2]], b: &[[f64; 2]], tol: f64) -> Result<Vec<[f64; 2]>, PointsRefused> {
        measure::polyline_2d_crossings(a, b, tol).map_err(refused)
    }

    fn tangent_contact_line_circle_2d(
        &self,
        a: [f64; 2],
        b: [f64; 2],
        center: [f64; 2],
        radius: f64,
        tol: f64,
    ) -> Result<Option<[f64; 2]>, PointsRefused> {
        measure::tangent_contact_line_circle_2d(a, b, center, radius, tol).map_err(refused)
    }

    fn tangent_contact_circle_circle_2d(&self, c1: [f64; 2], r1: f64, c2: [f64; 2], r2: f64, tol: f64) -> Result<Option<[f64; 2]>, PointsRefused> {
        measure::tangent_contact_circle_circle_2d(c1, r1, c2, r2, tol).map_err(refused)
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

    fn unit_direction(&self, direction: [f64; 3]) -> Option<[f64; 3]> {
        measure::unit_direction(direction)
    }
}
