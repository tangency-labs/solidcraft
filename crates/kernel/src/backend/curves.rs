//! solidcore's curves and fitting ([`Curves`]): each the solidcore-ops call this crate's callers
//! made before the interface existed, its refusal's words kept whole.

use solidcore_ops::{curve, curve_fit, fit_projection, pcurve, point_identity};
use solidcore_record::brep::{Curve, Curve2};
use solidcore_record::curve_query::{Crossing, CubicPiece, CurvePoint, CurveRayPoint, FitSplineBounds, PlaneSection};
use solidcore_record::projection::PlanarSpline;
use solidcore_record::{Plane, Vec2};
use solidcraft_kernel_api::{CurveRefused, Curves};

use super::Solidcore;

/// solidcore's refusal, in its words.
fn refused(e: solidcore_ops::Error) -> CurveRefused {
    CurveRefused(e.to_string())
}

impl Curves for Solidcore {
    fn piece_plane_crossings(&self, pieces: &[(&Curve, [f64; 2])], plane: &Plane) -> Result<Vec<Vec2>, CurveRefused> {
        curve::piece_plane_crossings(pieces, plane).map_err(refused)
    }

    fn plane_section(&self, curve: &Curve, interval: [f64; 2], plane: &Plane) -> Result<PlaneSection, CurveRefused> {
        curve::plane_section(curve, interval, plane).map_err(refused)
    }

    fn closest_point(&self, curve: &Curve, p: [f64; 3], tolerance: f64) -> Result<Option<CurvePoint>, CurveRefused> {
        curve::closest_point(curve, p, tolerance).map_err(refused)
    }

    fn plane_crossings(&self, curve: &Curve, interval: [f64; 2], plane: &Plane) -> Result<Vec<Vec2>, CurveRefused> {
        curve::plane_crossings(curve, interval, plane).map_err(refused)
    }

    fn crossings(&self, a: &Curve, b: &Curve, along: Option<[f64; 3]>, tolerance: f64) -> Result<Vec<Crossing>, CurveRefused> {
        curve::crossings(a, b, along, tolerance).map_err(refused)
    }

    fn join_curves(&self, a: &Curve, b: &Curve, tolerance: f64) -> Result<Curve, CurveRefused> {
        curve::join_curves(a, b, tolerance).map_err(refused)
    }

    fn cubic_pieces(&self, points: [[f64; 3]; 4]) -> Result<Vec<CubicPiece>, CurveRefused> {
        curve::cubic_pieces(points).map_err(refused)
    }

    fn arc_through(&self, points: [[f64; 3]; 3]) -> Result<Curve, CurveRefused> {
        curve::arc_through(points).map_err(refused)
    }

    fn closest_to_ray(&self, curve: &Curve, origin: [f64; 3], direction: [f64; 3], tolerance: f64) -> Result<Option<CurveRayPoint>, CurveRefused> {
        curve::closest_to_ray(curve, origin, direction, tolerance).map_err(refused)
    }

    fn fit_spline_bounds(&self, pieces: &[[[f64; 3]; 4]], near: &[(usize, [f64; 3])]) -> Result<FitSplineBounds, CurveRefused> {
        curve_fit::fit_spline_bounds(pieces, near).map_err(refused)
    }

    fn fit_splines(&self, p: &[Vec2], periodic: bool, limit: usize) -> Result<Vec<PlanarSpline>, CurveRefused> {
        fit_projection::fit_splines(p, periodic, limit).map_err(refused)
    }

    fn check_control(&self, n: usize, degree: usize, knots: &[f64], weights: &[f64], limit: usize) -> Result<(), CurveRefused> {
        fit_projection::check_control(n, degree, knots, weights, limit).map_err(refused)
    }

    fn homogeneous_controls(&self, p: &[Vec2], weights: &[f64]) -> Result<Vec<[f64; 4]>, CurveRefused> {
        fit_projection::homogeneous_controls(p, weights).map_err(refused)
    }

    fn reversed_metadata(&self, knots: &[f64], weights: &[f64]) -> (Vec<f64>, Vec<f64>) {
        fit_projection::reversed_metadata(knots, weights)
    }

    fn linear_image(&self, p: &[Vec2], degree: usize, knots: &[f64], weights: &[f64], roundoff: f64) -> Result<Option<[Vec2; 2]>, CurveRefused> {
        fit_projection::linear_image(p, degree, knots, weights, roundoff).map_err(refused)
    }

    fn check_periodic(&self, p: &[Vec2]) -> Result<(), CurveRefused> {
        fit_projection::check_periodic(p).map_err(refused)
    }

    fn fit_beziers(&self, p: &[Vec2], periodic: bool) -> Result<Vec<[Vec2; 4]>, CurveRefused> {
        fit_projection::fit_beziers(p, periodic).map_err(refused)
    }

    fn natural_spans(&self, p: &[Vec2]) -> Result<Vec<(Vec2, Vec2, Vec2, Vec2)>, CurveRefused> {
        fit_projection::natural_spans(p).map_err(refused)
    }

    fn ordered_nearby_points(&self, old: &[Vec2], new: &[Vec2], within: f64, work: usize) -> Result<bool, CurveRefused> {
        point_identity::ordered_nearby_points(old, new, within, work).map_err(refused)
    }

    fn pcurve_eval(&self, c: &Curve2, t: f64) -> Option<[f64; 2]> {
        pcurve::eval(c, t)
    }
}
