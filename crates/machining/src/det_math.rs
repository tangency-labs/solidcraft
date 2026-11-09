//! Trigonometry for the arc sweeps, kept behind this one module so it can be swapped.
//! shortcut: std trig, bit-identical across wasm/native not guaranteed; replace with own IEEE-only versions before release.

/// `(sin x, cos x)`.
pub fn sin_cos(x: f64) -> (f64, f64) {
    x.sin_cos()
}

/// Angle of the vector `(x, y)`, in `(-pi, pi]`.
pub fn atan2(y: f64, x: f64) -> f64 {
    y.atan2(x)
}

/// `cos x`.
pub fn cos(x: f64) -> f64 {
    x.cos()
}

/// `acos x`, for `x` in `[-1, 1]`.
pub fn acos(x: f64) -> f64 {
    x.acos()
}
