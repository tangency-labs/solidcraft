//! Adapt fitted rim definitions to the shared kernel's continuous gap proof.
use super::{CurveN, SplineError, SplineResult};
use solidcraft_kernel_services::rational_rim::{self as kernel, Curve};

fn curve(source: &CurveN<4>) -> Curve<'_> {
    Curve { degree: source.degree, knots: &source.knots, ctrl: &source.ctrl }
}

fn pieces(source: &[([f64; 2], CurveN<4>)]) -> SplineResult<Vec<([f64; 2], Curve<'_>)>> {
    if source.is_empty() || source.len() > 512 {
        return super::invalid("rational rim requires a bounded complete piece sequence");
    }
    Ok(source.iter().map(|(range, source)| (*range, curve(source))).collect())
}

fn error(error: kernel::SplineError) -> SplineError {
    match error {
        kernel::SplineError::Invalid(message) => SplineError::Invalid(message),
        kernel::SplineError::TooLarge(message) => SplineError::TooLarge(message),
    }
}

/// Certify the maximum Euclidean gap over the complete paired unit domain.
/// Each reversal flag changes one input's parameter direction. The input
/// pieces must contain packed Bezier spans.
pub fn rational_rim_gap(
    first: &[([f64; 2], CurveN<4>)],
    second: &[([f64; 2], CurveN<4>)],
    reversed: [bool; 2],
    tolerance: f64,
    work: &mut usize,
) -> SplineResult<Option<f64>> {
    kernel::rational_rim_gap(&pieces(first)?, &pieces(second)?, reversed, tolerance, work).map_err(error)
}

/// Certify a fitted rim against a native unit-domain B-spline, including
/// outward-rounded knot insertion in the native curve.
pub fn rational_rim_to_native_gap(
    fitted: &[([f64; 2], CurveN<4>)],
    native: &CurveN<4>,
    reversed: [bool; 2],
    tolerance: f64,
    work: &mut usize,
) -> SplineResult<Option<f64>> {
    kernel::rational_rim_to_native_gap(&pieces(fitted)?, &curve(native), reversed, tolerance, work).map_err(error)
}
