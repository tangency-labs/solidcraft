//! Thin portable ordered-solve façade for the fill assemblers.
use crate::bspline::SplineError;
use solidcraft_kernel_services::ordered_solve as ops;
pub(crate) use solidcraft_kernel_services::ordered_solve::{Band, Normal};
impl From<ops::Failure> for SplineError {
    fn from(error: ops::Failure) -> Self {
        let text = match error {
            ops::Failure::InvalidShape => "the fit's equations are out of shape",
            ops::Failure::FitSingular => "the fit's equations are singular",
            ops::Failure::FitNonFinite => "the fit is not finite",
            ops::Failure::PatchSingular => "the patch's equations are singular",
            ops::Failure::PatchNonFinite => "the patch's equations have no finite solution",
            ops::Failure::BlocksNonFinite => "the patches' equations have no finite solution",
            ops::Failure::SharedLimit { given } => {
                return SplineError::TooLarge(format!("{given} unknowns shared by the patches about the centre (at most 4000)"));
            }
            ops::Failure::TooLarge => return SplineError::TooLarge("the ordered solve dimension exceeds its limit".into()),
            ops::Failure::Allocation => return SplineError::TooLarge("the ordered solve storage cannot be allocated".into()),
        };
        SplineError::Invalid(text.into())
    }
}
