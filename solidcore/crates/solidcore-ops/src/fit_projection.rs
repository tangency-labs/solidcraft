//! Fit-spline projection workflows, converting planar records to the kernel's construction.
//! Every interpolation, compact conversion and certified extent calculation is kernel-owned.

use solidcore::curve::fit_projection as kernel;
use solidcore_convert::{record_vec2, vec2};
use solidcore_record::Vec2;

use crate::error::status;
use crate::spline_projection::PlanarSpline;
use crate::Result;

fn points(p: &[Vec2]) -> Vec<solidcore::math::Vec2> {
    p.iter().copied().map(vec2).collect()
}

/// Convert the selected natural or cyclic fit to exact control splines, each within `limit`.
pub fn fit_splines(p: &[Vec2], periodic: bool, limit: usize) -> Result<Vec<PlanarSpline>> {
    kernel::fit_splines(&points(p), periodic, limit)
        .map(|curves| {
            curves
                .into_iter()
                .map(|s| PlanarSpline {
                    degree: s.degree,
                    knots: s.knots,
                    weights: s.weights,
                    points: s.points.into_iter().map(record_vec2).collect(),
                })
                .collect()
        })
        .map_err(status)
}

/// The exact cubic Bézier pieces of the selected natural or cyclic fit.
pub fn fit_beziers(p: &[Vec2], periodic: bool) -> Result<Vec<[Vec2; 4]>> {
    kernel::fit_beziers(&points(p), periodic)
        .map(|spans| {
            spans
                .into_iter()
                .map(|span| span.map(record_vec2))
                .collect()
        })
        .map_err(status)
}

/// The natural interpolation's Hermite pieces, retaining its chord-length parameters.
pub fn natural_spans(p: &[Vec2]) -> Result<Vec<(Vec2, Vec2, Vec2, Vec2)>> {
    kernel::natural_spans(&points(p))
        .map(|spans| {
            spans
                .into_iter()
                .map(|(a, b, c, d)| {
                    (
                        record_vec2(a),
                        record_vec2(b),
                        record_vec2(c),
                        record_vec2(d),
                    )
                })
                .collect()
        })
        .map_err(status)
}

/// Validate the closure, distinct samples and chord regularity of cyclic interpolation.
pub fn check_periodic(p: &[Vec2]) -> Result<()> {
    kernel::check_periodic(&points(p)).map_err(status)
}

/// Check a control spline's explicit clamped knots and positive weights.
pub fn check_control(
    n: usize,
    degree: usize,
    knots: &[f64],
    weights: &[f64],
    limit: usize,
) -> Result<()> {
    kernel::check_control(n, degree, knots, weights, limit).map_err(status)
}

/// Effective degree and clamped uniform knots for an implicit control spline.
pub fn uniform_knots(n: usize, degree: usize) -> Result<(usize, Vec<f64>)> {
    kernel::uniform_knots(n, degree).map_err(status)
}

/// A point or a certified complete line image, or `None` for a planar curve.
pub fn linear_image(
    p: &[Vec2],
    degree: usize,
    knots: &[f64],
    weights: &[f64],
    roundoff: f64,
) -> Result<Option<[Vec2; 2]>> {
    kernel::linear_image(&points(p), degree, knots, weights, roundoff)
        .map(|line| line.map(|ends| ends.map(record_vec2)))
        .map_err(status)
}

/// Homogeneous controls normalized by the largest weight without changing the represented curve.
pub fn homogeneous_controls(p: &[Vec2], weights: &[f64]) -> Result<Vec<[f64; 4]>> {
    kernel::homogeneous_controls(&points(p), weights).map_err(status)
}

/// Reverse explicit metadata while preserving the spline's exact knot spacings.
pub fn reversed_metadata(knots: &[f64], weights: &[f64]) -> (Vec<f64>, Vec<f64>) {
    kernel::reversed_metadata(knots, weights)
}
