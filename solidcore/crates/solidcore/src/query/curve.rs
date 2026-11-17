//! Free-curve queries (not upstream): the point of a curve nearest a ray.
//!
//! | Rust                                          | Upstream |
//! |-----------------------------------------------|----------|
//! | [`closest_point_to_ray`], [`RayClosestPoint`] | (none)   |
//!
//! An edge's [`super::edge::closest_point_to_ray`] is this on the edge's curve, interval and
//! tolerance. No recursion.

use super::edge::CurveHandle;
use crate::curve::curve::p9::CurveSolveOptions;
use crate::curve::curve::Curve;
use crate::curve::line::Line;
use crate::error::{Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent3d::Extent3d;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::nurbs::b_spline_curve::BoundingBoxRequest;

const OP_CLOSEST_RAY: &str = "query::curve::closest_point_to_ray";

/// The result of [`closest_point_to_ray`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayClosestPoint {
    /// The curve's parameter of the curve point nearest the ray.
    pub parameter: f64,
    /// That curve point.
    pub point: Point3,
    /// The ray's point nearest it.
    pub on_ray: Point3,
    /// The distance between the two.
    pub distance: f64,
}

/// The point of `curve` over `interval` nearest the ray from `origin` along `direction` (a
/// pick through the view, which the curve's point nearest one point on the ray answers only
/// where that point happens to be the nearest one): the first solution of the global
/// minimising curve/curve solve ([`Curve::global_curve_solve`]) of the curve over `interval`
/// against the ray as a line segment from `origin`, `tolerance` the distance tolerance; `None`
/// when there is none. The segment runs past the curve: as long as the farthest point of the
/// curve's box over `interval` from `origin` is away, plus a millimetre, and a curve point's
/// foot on the ray is never farther from `origin` than the point itself, so the segment's
/// nearest point is the ray's. A ray pointing away from the curve is nearest it at `origin`.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a `direction` of no length, a coordinate that is not
/// finite or a tolerance not finite and positive; those of the curve's box, the line and the
/// solve.
pub fn closest_point_to_ray(
    k: &Kernel,
    curve: &Curve,
    interval: &Extent1d,
    origin: Point3,
    direction: Vec3,
    tolerance: f64,
) -> Result<Option<RayClosestPoint>> {
    let finite = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
    if !(finite(origin) && finite(direction)) {
        return Err(SolidError::invalid_input(
            OP_CLOSEST_RAY,
            "a coordinate that is not finite",
        ));
    }
    if !(tolerance.is_finite() && tolerance > 0.0) {
        return Err(SolidError::invalid_input(
            OP_CLOSEST_RAY,
            "a tolerance that is not finite and positive",
        ));
    }
    let direction = direction
        .unitize()
        .map_err(|_| SolidError::invalid_input(OP_CLOSEST_RAY, "a direction of no length"))?;
    let req = BoundingBoxRequest {
        normal: true,
        pseudo: false,
        polar: false,
    };
    let curve_box = curve
        .calculate_bounding_box(k, interval, req, true)?
        .normal
        .ok_or_else(|| SolidError::query_failed(OP_CLOSEST_RAY, "the curve has no box"))?;
    let reach = curve_box.max_distance(&Extent3d::from_points(origin, origin)) + 1.0;
    let ray = Curve::from(Line::create_line_segment(
        3,
        origin,
        origin + direction * reach,
        None,
    )?);
    let ray_interval = ray.natural_interval();
    let opts = CurveSolveOptions {
        handles: [
            Some(CurveHandle {
                id: 0,
                ivl: curve.natural_interval(),
            }),
            Some(CurveHandle {
                id: 1,
                ivl: ray_interval,
            }),
        ],
        ..CurveSolveOptions::default()
    };
    let solutions = curve.global_curve_solve(
        k,
        interval,
        &ray,
        &ray_interval,
        SolverOperation::Minimize,
        tolerance,
        &opts,
        SolutionRequested::Single,
    )?;
    let Some(sol) = solutions.get(0) else {
        return Ok(None);
    };
    let parameter = sol.start.param(0)?;
    Ok(Some(RayClosestPoint {
        parameter,
        point: curve.evaluate_point(k, parameter)?,
        on_ray: ray.evaluate_point(k, sol.start.param(1)?)?,
        distance: sol.start.solution_value,
    }))
}
