//! Record-coordinate access to subdivision stencils.
use solidcore::geom::form_subdivision as kernel;
use solidcore_convert::{record_vec3, vec3};
use solidcore_record::Vec3;

/// Limit point along a boundary or crease.
pub fn crease_limit(point: Vec3, neighbors: [Vec3; 2]) -> Vec3 {
    record_vec3(kernel::crease_limit(vec3(point), neighbors.map(vec3)))
}

/// Limit point of a smooth quad fan.
pub fn smooth_limit(
    point: Vec3,
    neighbors: impl IntoIterator<Item = Vec3>,
    diagonals: impl IntoIterator<Item = Vec3>,
    valence: usize,
) -> Vec3 {
    record_vec3(kernel::smooth_limit(
        vec3(point),
        neighbors.into_iter().map(vec3),
        diagonals.into_iter().map(vec3),
        valence,
    ))
}

/// Whether a semi-sharp corner's successive values have converged.
pub fn converged_limit(current: Vec3, previous: Vec3) -> bool {
    kernel::converged_limit(vec3(current), vec3(previous))
}

/// Convert a uniform 4×4 spline span to its Bézier controls.
pub fn regular_bezier(net: [Vec3; 16]) -> [Vec3; 16] {
    kernel::regular_bezier(net.map(vec3)).map(record_vec3)
}

/// Project a finite displacement onto its reflection plane.
pub fn mirror_tangent(delta: Vec3, axis: usize) -> Option<Vec3> {
    kernel::mirror_tangent(vec3(delta), axis).map(record_vec3)
}
