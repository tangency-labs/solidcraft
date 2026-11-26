//! Record-coordinate access to subdivision stencils.
use solidcore::geom::form_subdivision as kernel;
use solidcore_convert::{record_vec3, vec3};
use solidcore_record::Vec3;

pub use kernel::StepEdge;

/// Evaluate the ordered Catmull–Clark coordinate masks in the kernel.
pub fn step_points(
    points: &[Vec3],
    faces: &[Vec<usize>],
    edges: &[StepEdge],
    incident: &[Vec<usize>],
    corners: &[f64],
) -> solidcore::Result<Vec<Vec3>> {
    if points
        .len()
        .checked_add(edges.len())
        .and_then(|n| n.checked_add(faces.len()))
        .is_none_or(|n| n > kernel::MAX_STEP_POINTS)
        || faces.len() > kernel::MAX_STEP_FACES
    {
        return Err(solidcore::SolidError::invalid_input(
            "Form subdivision",
            "step point budget exceeded",
        ));
    }
    let points = points.iter().copied().map(vec3).collect::<Vec<_>>();
    kernel::step_points(&points, faces, edges, incident, corners)
        .map(|p| p.into_iter().map(record_vec3).collect())
}

/// Area-weighted preview normals, independent of logical model ownership.
pub fn vertex_normals(points: &[Vec3], triangles: &[[u32; 3]]) -> solidcore::Result<Vec<Vec3>> {
    if points.len() > kernel::MAX_STEP_POINTS || triangles.len() > 2 * kernel::MAX_STEP_FACES {
        return Err(solidcore::SolidError::invalid_input(
            "Form subdivision",
            "display normal budget exceeded",
        ));
    }
    let points = points.iter().copied().map(vec3).collect::<Vec<_>>();
    kernel::vertex_normals(&points, triangles).map(|p| p.into_iter().map(record_vec3).collect())
}

/// Coordinate centroid in the application's prescribed stable order.
pub fn centroid(points: impl ExactSizeIterator<Item = Vec3>) -> solidcore::Result<Vec3> {
    kernel::centroid(points.map(vec3)).map(record_vec3)
}

/// Whether a nonzero extrusion displacement meets the coordinate contract.
pub fn extrusion_delta(delta: Vec3) -> bool {
    kernel::extrusion_delta(vec3(delta))
}

/// Decay one semi-sharp subdivision constraint.
pub fn decay_sharpness(sharpness: f64) -> f64 {
    kernel::decay_sharpness(sharpness)
}

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

/// Portable ruled-wall coordinates, separate from preview topology and ownership.
pub struct TrimWallGeometry {
    /// Translated boundary samples in directed order.
    pub top: Vec<Vec3>,
    /// Reverse-order strip normals.
    pub normals: Vec<Vec3>,
    /// Translated endpoint controls.
    pub controls: [Vec3; 2],
}

/// Validate a bounded nonzero trim-wall displacement.
pub fn validate_trim_wall_delta(delta: Vec3) -> solidcore::Result<()> {
    kernel::validate_trim_wall_delta(vec3(delta))
}

/// Move a trim-wall rim, validating before state mutation.
pub fn move_trim_wall(delta: Vec3, offset: Vec3) -> solidcore::Result<Vec3> {
    kernel::move_trim_wall(vec3(delta), vec3(offset)).map(record_vec3)
}

/// Construct all numerical wall data before application preview mutation.
pub fn trim_wall(
    base: &[Vec3],
    controls: [Vec3; 2],
    delta: Vec3,
) -> solidcore::Result<TrimWallGeometry> {
    if !(2..=1_000_000).contains(&base.len()) {
        return Err(solidcore::SolidError::invalid_input(
            "Form trim wall",
            "trim preview path needs bounded stations",
        ));
    }
    let base: Vec<_> = base.iter().copied().map(vec3).collect();
    let geometry = kernel::trim_wall(&base, controls.map(vec3), vec3(delta))?;
    Ok(TrimWallGeometry {
        top: geometry.top.into_iter().map(record_vec3).collect(),
        normals: geometry.normals.into_iter().map(record_vec3).collect(),
        controls: geometry.controls.map(record_vec3),
    })
}
