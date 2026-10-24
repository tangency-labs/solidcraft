//! Definition-local visual coordinates, independent of geometry and engineering axes.
//! This numerical foundation installs no texture sampler, shader or persistent record.

use solidcraft_geom::{Aabb3, Vec3};

/// Admission bounds, not a GPU precision or engineering applicability guarantee.
pub const MAX_LOCAL_MM: f64 = 1e9;
pub const MIN_REPEAT_MM: f64 = 1e-3;
pub const MAX_REPEAT_MM: f64 = 1e6;
pub const MAX_REPEAT_COORDINATE: f64 = 1e6;
/// Absolute tolerance for unit-length squared, orthogonality and handedness checks.
pub const FRAME_TOLERANCE: f64 = 1e-10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MappingError {
    Position,
    Frame,
    RepeatSize,
    Coordinates,
    Bounds,
}

impl std::fmt::Display for MappingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Position => "visual mapping positions must be finite and within +/-1e9 mm",
            Self::Frame => "visual mapping axes must form a right-handed orthonormal frame",
            Self::RepeatSize => "visual repeat sizes must be finite and within [0.001, 1e6] mm",
            Self::Coordinates => "visual repeat coordinates exceed the +/-1e6 admission limit",
            Self::Bounds => "grain suggestion needs finite ordered local bounds with a positive extent",
        })
    }
}
impl std::error::Error for MappingError {}

/// A one-time visual suggestion; not a material-local mechanical L/R/T assignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrainAxis {
    X,
    Y,
    Z,
}

impl GrainAxis {
    /// First axis is the visual grain direction. The other two complete a right-handed frame.
    pub fn axes(self) -> [Vec3; 3] {
        match self {
            Self::X => [Vec3::X, Vec3::Y, Vec3::Z],
            Self::Y => [Vec3::Y, Vec3::Z, Vec3::X],
            Self::Z => [Vec3::Z, Vec3::X, Vec3::Y],
        }
    }
}

/// Suggest from definition-local bounds only, with exact ties resolved X, then Y, then Z.
/// The caller stores an accepted frame once; this function never observes a later rebuild.
pub fn suggest_grain_axis(local_bounds: Aabb3) -> Result<GrainAxis, MappingError> {
    if !bounded_position(local_bounds.min) || !bounded_position(local_bounds.max) || local_bounds.is_empty() {
        return Err(MappingError::Bounds);
    }
    let extent = local_bounds.max - local_bounds.min;
    if extent.x.max(extent.y).max(extent.z) <= 0.0 {
        return Err(MappingError::Bounds);
    }
    if extent.x >= extent.y && extent.x >= extent.z {
        Ok(GrainAxis::X)
    } else if extent.y >= extent.z {
        Ok(GrainAxis::Y)
    } else {
        Ok(GrainAxis::Z)
    }
}

/// Immutable admitted visual frame. Fields cannot be changed without readmission.
/// No serde implementation: eventual document/catalog readers must use `new`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VisualMapping {
    origin_mm: Vec3,
    axes: [Vec3; 3],
    repeat_mm: Vec3,
}

impl VisualMapping {
    /// Axes are unit directions in definition-local space, in grain/across/depth order.
    /// Repeat sizes are distances along those directions, not body-bound fractions.
    /// Small floating-point frame error is admitted within `FRAME_TOLERANCE`; no skew,
    /// reflection, rescaling, orthogonalization or replacement frame is manufactured.
    pub fn new(origin_mm: Vec3, axes: [Vec3; 3], repeat_mm: Vec3) -> Result<Self, MappingError> {
        if !bounded_position(origin_mm) {
            return Err(MappingError::Position);
        }
        let [x, y, z] = axes;
        if !axes.into_iter().all(|v| v.is_finite() && (v.len2() - 1.0).abs() <= FRAME_TOLERANCE)
            || x.dot(y).abs() > FRAME_TOLERANCE
            || x.dot(z).abs() > FRAME_TOLERANCE
            || y.dot(z).abs() > FRAME_TOLERANCE
            || (x.cross(y).dot(z) - 1.0).abs() > FRAME_TOLERANCE
        {
            return Err(MappingError::Frame);
        }
        if ![repeat_mm.x, repeat_mm.y, repeat_mm.z].into_iter().all(|v| v.is_finite() && (MIN_REPEAT_MM..=MAX_REPEAT_MM).contains(&v)) {
            return Err(MappingError::RepeatSize);
        }
        Ok(Self { origin_mm, axes, repeat_mm })
    }

    pub fn origin_mm(self) -> Vec3 {
        self.origin_mm
    }
    pub fn axes(self) -> [Vec3; 3] {
        self.axes
    }
    pub fn repeat_mm(self) -> Vec3 {
        self.repeat_mm
    }

    /// Unwrapped signed repeat coordinates, calculated in f64. No clamping or UV seam.
    /// The host must first invert the selected instance placement into definition-local
    /// space. World-space bounding boxes and camera/mesh positions are not inputs here.
    /// Samplers choose wrapping/filtering; GPU consumers need their own precision budget.
    pub fn coordinates(self, local_point_mm: Vec3) -> Result<Vec3, MappingError> {
        if !bounded_position(local_point_mm) {
            return Err(MappingError::Position);
        }
        let delta = local_point_mm - self.origin_mm;
        let [x, y, z] = self.axes;
        let result = Vec3::new(delta.dot(x) / self.repeat_mm.x, delta.dot(y) / self.repeat_mm.y, delta.dot(z) / self.repeat_mm.z);
        if !result.is_finite() || [result.x, result.y, result.z].into_iter().any(|v| v.abs() > MAX_REPEAT_COORDINATE) {
            return Err(MappingError::Coordinates);
        }
        Ok(result)
    }
}

fn bounded_position(value: Vec3) -> bool {
    value.is_finite() && [value.x, value.y, value.z].into_iter().all(|v| v.abs() <= MAX_LOCAL_MM)
}

#[cfg(test)]
#[path = "mapping_tests.rs"]
mod tests;
