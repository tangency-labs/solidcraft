//! Continuous bounds on paired, already-resolved rational rim geometry.
pub use kernel::Curve;
use solidcore::geom::rim_coincidence as kernel;

/// Portable failure of a rational-rim proof, independent of kernel error types.
#[derive(Debug, Clone, PartialEq)]
pub enum SplineError {
    /// The geometry does not meet the proof input contract.
    Invalid(String),
    /// The proof exceeded its bounded workspace or caller work budget.
    TooLarge(String),
}
impl std::fmt::Display for SplineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) => write!(f, "invalid spline: {message}"),
            Self::TooLarge(message) => write!(f, "spline too large: {message}"),
        }
    }
}
impl std::error::Error for SplineError {}

fn portable(error: solidcore::SolidError) -> SplineError {
    use solidcore::error::{nurbs::NurbsFailure, Reason};
    match error {
        solidcore::SolidError::Failed(failure) => match failure.reason {
            Reason::Nurbs(NurbsFailure::Storage | NurbsFailure::Memory) => {
                SplineError::TooLarge("rational rim proof exceeded its work or span budget".into())
            }
            _ => SplineError::Invalid(
                "rational rim requires finite bounded compatible spline data".into(),
            ),
        },
        _ => SplineError::Invalid(
            "rational rim requires finite bounded compatible spline data".into(),
        ),
    }
}

/// Certify the complete paired unit domains of two fitted rational rim sequences.
/// Each reversal flag changes one input's parameter direction. None is an
/// unresolved/refused coincidence within tolerance, not a positive certificate.
pub fn rational_rim_gap(
    first: &[([f64; 2], Curve<'_>)],
    second: &[([f64; 2], Curve<'_>)],
    reversed: [bool; 2],
    tolerance: f64,
    work: &mut usize,
) -> Result<Option<f64>, SplineError> {
    kernel::rational_rim_gap(first, second, reversed, tolerance, work).map_err(portable)
}

/// Certify a fitted rim against a native rational spline including knot-insertion rounding.
pub fn rational_rim_to_native_gap(
    fitted: &[([f64; 2], Curve<'_>)],
    native: &Curve<'_>,
    reversed: [bool; 2],
    tolerance: f64,
    work: &mut usize,
) -> Result<Option<f64>, SplineError> {
    kernel::rational_rim_to_native_gap(fitted, native, reversed, tolerance, work).map_err(portable)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standard_core_refusals_keep_the_portable_invalid_and_budget_seam() {
        let knots = [0.0, 0.0, 1.0, 1.0];
        let controls = [[0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]];
        let curve = Curve {
            degree: 1,
            knots: &knots,
            ctrl: &controls,
        };
        let pieces = [([0.0, 1.0], curve)];
        assert!(matches!(
            rational_rim_gap(&pieces, &pieces, [false; 2], 0.001, &mut 0),
            Err(SplineError::TooLarge(_))
        ));
        let invalid = [([0.0, 1.0], Curve { degree: 0, ..curve })];
        assert!(matches!(
            rational_rim_gap(&invalid, &pieces, [false; 2], 0.001, &mut 100),
            Err(SplineError::Invalid(_))
        ));
    }
}
