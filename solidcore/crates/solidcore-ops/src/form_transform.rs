//! Point transforms in a Form editor, converted to the kernel's coordinate types.

use solidcore::eval::form_transform as kernel;
use solidcore_convert::{record_vec3, vec3};
use solidcore_record::Vec3;
use std::collections::{BTreeMap, BTreeSet};

pub use kernel::{WeightError, MAX_WEIGHT_POINTS};

/// Translate a Form point by a weighted displacement.
pub fn translate(point: Vec3, delta: Vec3, weight: f64) -> Vec3 {
    record_vec3(kernel::translate(vec3(point), vec3(delta), weight))
}

/// Rotate a Form point around a pivot by a weighted angle.
pub fn rotate(point: Vec3, pivot: Vec3, axis: Vec3, angle: f64, weight: f64) -> Option<Vec3> {
    kernel::rotate(vec3(point), vec3(pivot), vec3(axis), angle, weight).map(record_vec3)
}

/// Scale a Form point in its orthonormal frame.
pub fn scale(point: Vec3, pivot: Vec3, basis: [Vec3; 3], factors: [f64; 3], weight: f64) -> Vec3 {
    record_vec3(kernel::scale(
        vec3(point),
        vec3(pivot),
        basis.map(vec3),
        factors,
        weight,
    ))
}

/// Apply the numeric Form transform in a single frame.
pub fn numeric(
    point: Vec3,
    pivot: Vec3,
    basis: [Vec3; 3],
    lengths: [f64; 3],
    angles: [f64; 3],
    factors: [f64; 3],
    weight: f64,
) -> Option<Vec3> {
    kernel::numeric(
        vec3(point),
        vec3(pivot),
        basis.map(vec3),
        lengths,
        angles,
        factors,
        weight,
    )
    .map(record_vec3)
}

/// Use the same oriented scale factors for the transform and its feedback.
pub fn scale_factors(delta: Vec3, length: f64, basis: [Vec3; 3]) -> [f64; 3] {
    kernel::scale_factors(vec3(delta), length, basis.map(vec3))
}

/// Use the same uniform scale factors for the transform and its feedback.
pub fn uniform_factors(delta: Vec3, length: f64, right: Vec3, up: Vec3) -> [f64; 3] {
    kernel::uniform_factors(vec3(delta), length, vec3(right), vec3(up))
}

/// Length and axis components of a Form translation request.
pub fn translation_measure(delta: Vec3, basis: [Vec3; 3]) -> (f64, [f64; 3]) {
    kernel::translation_measure(vec3(delta), basis.map(vec3))
}

/// The axes of a plane facing a viewport ray.
pub fn view_plane_basis(direction: Vec3, right: Vec3) -> Option<[Vec3; 2]> {
    kernel::view_plane_basis(vec3(direction), vec3(right)).map(|axes| axes.map(record_vec3))
}

/// A fixed axis or plane for one translation gesture.
pub struct SnapFrame(kernel::Frame);

impl SnapFrame {
    /// A line constraint for axis translation.
    pub fn axis(direction: Vec3) -> Option<Self> {
        kernel::Frame::axis(vec3(direction)).map(Self)
    }

    /// A plane constraint with two orthogonal axes.
    pub fn plane(axes: [Vec3; 2]) -> Option<Self> {
        kernel::Frame::plane(axes.map(vec3)).map(Self)
    }

    /// Quantise a displacement in this frame.
    pub fn quantise(&self, delta: Vec3, step: f64) -> Option<Vec3> {
        self.0.quantise(vec3(delta), step).map(record_vec3)
    }
}

/// Weights for a Form selection, using distance along its logical edges.
pub fn weights(
    points: &[Vec3],
    faces: &[Vec<usize>],
    seeds: &BTreeSet<usize>,
    editable: &BTreeSet<usize>,
    locked: &BTreeSet<usize>,
    radius: f64,
    kind: usize,
) -> Result<BTreeMap<usize, f64>, WeightError> {
    kernel::check_weight_limits(points.len(), faces)?;
    let points: Vec<_> = points.iter().copied().map(vec3).collect();
    kernel::weights(&points, faces, seeds, editable, locked, radius, kind)
}
