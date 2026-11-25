//! Authoring adapter for kernel-owned exact straight UV restrictions.
use super::{HPoint, NurbsSurface, SplineError, SplineResult};

pub(super) fn error(error: solidcraft_kernel_services::parameter_line::Error) -> SplineError {
    use solidcraft_kernel_services::parameter_line::Error;
    match error {
        Error::Invalid(message) => SplineError::Invalid(message),
        Error::Unsupported(message) => SplineError::Unsupported(message),
        Error::TooLarge(message) => SplineError::TooLarge(message),
    }
}

impl NurbsSurface {
    /// Exact rational curve S(a+t*(b-a)), with knot cuts represented as Bezier pieces.
    pub fn parameter_line(&self, a: [f64; 2], b: [f64; 2]) -> SplineResult<(Vec<f64>, Vec<HPoint>)> {
        let mut remaining = usize::MAX;
        self.parameter_line_bounded(a, b, &mut remaining)
    }

    /// Charge the shared knot-scan, crossing and tensor-product work budget.
    pub fn parameter_line_bounded(&self, a: [f64; 2], b: [f64; 2], remaining: &mut usize) -> SplineResult<(Vec<f64>, Vec<HPoint>)> {
        let (knots, points) =
            solidcraft_kernel_services::parameter_line::extract(self.degree, &self.knots, self.count, &self.ctrl, a, b, remaining, |h| {
                [h.x, h.y, h.z, h.w]
            })
            .map_err(error)?;
        Ok((knots, points.into_iter().map(|[x, y, z, w]| HPoint { x, y, z, w }).collect()))
    }
}
