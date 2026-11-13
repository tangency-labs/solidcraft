//! Record-facing restrictions of native Form guide curves.

use solidcore::eval::form_guide_curves as kernel;
use solidcore_record::brep::Curve;

pub use kernel::{bezier_pieces, BezierPiece, MAX_CONTROLS};

/// Extract a record curve on its original parameter interval.
pub fn source_curve(
    curve: &Curve,
    range: [f64; 2],
    remaining: &mut usize,
) -> Result<Vec<Vec<[f64; 4]>>, String> {
    match curve {
        Curve::Polyline { points } if points.len() > MAX_CONTROLS => {
            return Err("native Form polyline exceeds its exact source budget".into())
        }
        Curve::Spline { knots, points }
            if points.len() > MAX_CONTROLS || knots.len() > MAX_CONTROLS =>
        {
            return Err("native Form spline exceeds its borrowed-size budget".into());
        }
        Curve::RationalSpline { knots, homogeneous }
            if homogeneous.len() > MAX_CONTROLS || knots.len() > MAX_CONTROLS =>
        {
            return Err("native Form spline exceeds its borrowed-size budget".into());
        }
        Curve::SurfaceIntersection { .. } => {
            return Err("procedural native Form intersection guides need a certified curve restriction; their leader is not an exact replacement".into());
        }
        _ => {}
    }
    kernel::source_curve(
        &solidcore_convert::curve(curve).map_err(|error| error.to_string())?,
        range,
        remaining,
    )
}
