//! Record-vector access to checked SolidCore affine maps.

use solidcore_ops::affine as ops;
use solidcore_record::Vec3;

use crate::{KernelError, Result};

pub use ops::{Fit, Matrix, fit_between, frame_point, frame_vector, from_frame, mapped_axes, point, proper_axes, vector};

/// Stack two maps in application order.
pub fn then(first: &Matrix, second: &Matrix) -> Result<Matrix> {
    ops::then(first, second).map_err(|_| KernelError::Invalid("non-finite transform composition".into()))
}

/// The checked inverse of a regular affine map.
pub fn inverse(matrix: &Matrix) -> Result<Matrix> {
    ops::inverse(matrix).map_err(|_| KernelError::Invalid("singular or ill-conditioned affine transform".into()))
}

/// Rotate about the line through `origin`, then translate.
pub fn rigid(translation: Vec3, origin: Vec3, axis: Vec3, angle: f64) -> Result<Matrix> {
    if !(translation.is_finite() && origin.is_finite() && angle.is_finite()) {
        return Err(KernelError::Invalid("non-finite transform".into()));
    }
    ops::rigid_motion(translation, origin, axis, angle).map_err(|_| KernelError::Invalid("invalid rigid transform".into()))
}
