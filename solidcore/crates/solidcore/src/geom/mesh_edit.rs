//! Point operations for editing and importing polygonal cages.

use crate::eval::vec::VecExt;
use crate::{Transform, Vec3};

/// Accumulate a weighted displacement from a reference point to a shape point.
pub fn deform(point: Vec3, shape: Vec3, reference: Vec3, weight: f64) -> Option<Vec3> {
    let result = point + (shape - reference) * weight;
    result.is_finite().then_some(result)
}

/// Determine whether a mesh placement reverses winding. The determinant
/// threshold retains the importer's minimum scale policy.
pub fn reverses_winding(frame: &Transform, minimum: f64) -> Option<bool> {
    let [x, y, z] = std::array::from_fn(|i| {
        let [a, b, c, _] = frame.m[i];
        Vec3::new(a, b, c)
    });
    let det = x.dot(y.cross(z));
    (det.is_finite() && det.abs() >= minimum).then_some(det < 0.0)
}

/// Apply a qualified placement, then express the point in the requested unit.
pub fn placed_point(frame: &Transform, point: Vec3, unit: f64) -> Option<Vec3> {
    let placed = super::affine_fit::point(frame, point) * unit;
    placed.is_finite().then_some(placed)
}

/// Snap both sides of a coordinate plane inside the bisect tolerance.
pub fn snap_to_plane(mut point: [f64; 3], axis: usize, tolerance: f64) -> [f64; 3] {
    if let Some(value) = point.get_mut(axis).filter(|value| value.abs() <= tolerance) {
        *value = 0.0;
    }
    point
}

/// Intersect a segment crossing a coordinate plane with that plane.
pub fn crossing(a: [f64; 3], b: [f64; 3], axis: usize) -> Option<[f64; 3]> {
    let (&da, &db) = a.get(axis).zip(b.get(axis))?;
    let t = (da / (da - db)).clamp(0.0, 1.0);
    let mut point = std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t);
    *point.get_mut(axis)? = 0.0;
    point.iter().all(|p| p.is_finite()).then_some(point)
}

/// Reflect in a coordinate plane, reporting whether the pair merges.
pub fn mirror(mut point: [f64; 3], axis: usize, seam_limit: f64) -> ([f64; 3], bool) {
    let Some(value) = point.get_mut(axis) else {
        return (point, false);
    };
    let merge = value.abs() < seam_limit;
    *value = if merge { 0.0 } else { -*value };
    (point, merge)
}

/// Convert a point-pair merge distance in object space to a plane distance.
pub fn mirror_seam_limit(frame: &Transform, axis: usize, tolerance: f64) -> Option<f64> {
    let &[x, y, z, _] = frame.m.get(axis).filter(|_| axis < 3)?;
    let length = Vec3::new(x, y, z).len();
    (length.is_finite() && length > 0.0).then(|| tolerance / (2.0 * length))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bisect_snaps_both_sides_of_the_plane_and_pins_crossings() {
        assert_eq!(snap_to_plane([-0.001, 2.0, 3.0], 0, 0.001), [0.0, 2.0, 3.0]);
        assert_eq!(snap_to_plane([0.001, 2.0, 3.0], 0, 0.001), [0.0, 2.0, 3.0]);
        assert_eq!(
            snap_to_plane([0.002, 2.0, 3.0], 0, 0.001),
            [0.002, 2.0, 3.0]
        );
        assert_eq!(
            crossing([-1.0, 2.0, 3.0], [3.0, 6.0, 3.0], 0),
            Some([0.0, 3.0, 3.0])
        );
    }

    #[test]
    fn shape_delta_and_weight_preserve_the_reference_offset() {
        let point = Vec3::new(2.0, 4.0, 6.0);
        let shape = Vec3::new(8.0, 2.0, 4.0);
        let reference = Vec3::new(4.0, 2.0, 2.0);
        assert_eq!(
            deform(point, shape, reference, 0.5),
            Some(Vec3::new(4.0, 4.0, 7.0))
        );
        assert_eq!(
            deform(Vec3::ZERO, shape, Vec3::ZERO, 0.5),
            Some(Vec3::new(4.0, 1.0, 2.0))
        );
    }
}
