//! Kernel point operations for mesh editing, with portable record vectors.

use solidcore::{geom::mesh_edit, Transform};
use solidcore_convert::{record_vec3, vec3};
use solidcore_record::Vec3;

pub use mesh_edit::{crossing, mirror, snap_to_plane};

/// Accumulate a weighted displacement from a reference point to a shape point.
pub fn deform(point: Vec3, shape: Vec3, reference: Vec3, weight: f64) -> Option<Vec3> {
    mesh_edit::deform(vec3(point), vec3(shape), vec3(reference), weight).map(record_vec3)
}

/// Whether a mesh placement reverses winding at the requested determinant floor.
pub fn reverses_winding(matrix: &crate::affine::Matrix, minimum: f64) -> Option<bool> {
    mesh_edit::reverses_winding(&Transform { m: *matrix }, minimum)
}

/// Apply a qualified placement and output unit conversion.
pub fn placed_point(matrix: &crate::affine::Matrix, point: Vec3, unit: f64) -> Option<Vec3> {
    mesh_edit::placed_point(&Transform { m: *matrix }, vec3(point), unit).map(record_vec3)
}

/// Convert a point-pair merge distance in object space to a plane distance.
pub fn mirror_seam_limit(
    matrix: &crate::affine::Matrix,
    axis: usize,
    tolerance: f64,
) -> Option<f64> {
    mesh_edit::mirror_seam_limit(&Transform { m: *matrix }, axis, tolerance)
}
