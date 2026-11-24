//! Ordered source-curve controls and rim tests in portable record coordinates.
use crate::affine::Matrix;
use solidcore::{geom::form_curves as kernel, Transform};
use solidcore_convert::{record_list, record_vec3, vec3};
use solidcore_record::{Plane, Vec3};

pub use kernel::RimPhase;

/// Unit direction of a regular authored segment.
pub fn segment_direction(vector: Vec3) -> Option<Vec3> {
    kernel::segment_direction(vec3(vector)).map(record_vec3)
}

/// Signed component along a direction, or alignment of two unit directions.
pub fn component_along(vector: Vec3, direction: Vec3) -> f64 {
    kernel::component_along(vec3(vector), vec3(direction))
}

/// Sine of the angle between regular chart directions.
pub fn transverse_sine(a: Vec3, b: Vec3) -> Option<f64> {
    kernel::transverse_sine(vec3(a), vec3(b))
}

/// Carry an offset between the original and reattached guide frames.
pub fn transport_detail(origin: Vec3, delta: Vec3, before: [Vec3; 3], after: [Vec3; 3]) -> Vec3 {
    record_vec3(kernel::transport_detail(
        vec3(origin),
        vec3(delta),
        before.map(vec3),
        after.map(vec3),
    ))
}

/// Material frame of a guide grid.
pub fn guide_frame(tangent: Vec3, across: Vec3) -> Option<[Vec3; 3]> {
    kernel::guide_frame(vec3(tangent), vec3(across)).map(|axes| axes.map(record_vec3))
}

/// Orient the normal of a certified boundary plane to its stations.
pub fn winding_normal(origin: Vec3, normal: Vec3, stations: impl Iterator<Item = Vec3>) -> Vec3 {
    record_vec3(kernel::winding_normal(
        vec3(origin),
        vec3(normal),
        stations.map(vec3),
    ))
}

/// Fraction and distance of a point's closest station on a boundary segment.
pub fn segment_proximity(point: Vec3, start: Vec3, end: Vec3) -> Option<(f64, f64)> {
    kernel::segment_proximity(vec3(point), vec3(start), vec3(end))
}

/// Normalize a finite editing direction without an absolute length threshold.
pub fn selection_unit(vector: Vec3) -> Option<Vec3> {
    kernel::selection_unit(vec3(vector)).map(record_vec3)
}

/// Oriented normal of a regular surface differential.
pub fn selection_normal(du: Vec3, dv: Vec3) -> Option<Vec3> {
    kernel::selection_normal(vec3(du), vec3(dv)).map(record_vec3)
}

/// Proper editing axes from a directed tangent and an oriented normal.
pub fn selection_axes(tangent: Vec3, normal: Vec3) -> Option<[Vec3; 3]> {
    kernel::selection_axes(vec3(tangent), vec3(normal)).map(|axes| axes.map(record_vec3))
}

/// Proper editing axes at an exact surface differential and directed UV reference.
pub fn selection_frame(du: Vec3, dv: Vec3, direction: [f64; 2]) -> Option<[Vec3; 3]> {
    kernel::selection_frame(vec3(du), vec3(dv), direction).map(|axes| axes.map(record_vec3))
}

/// Transfer a coefficient between ordinary and continued stencils.
pub fn continued_control(point: Vec3, ordinary: Vec3, continued: Vec3) -> Vec3 {
    record_vec3(kernel::continued_control(
        vec3(point),
        vec3(ordinary),
        vec3(continued),
    ))
}

/// Compare evaluated chart stations.
pub fn stations_agree(a: Vec3, b: Vec3, tolerance: f64) -> bool {
    kernel::stations_agree(vec3(a), vec3(b), tolerance)
}

/// Evaluate and place a fitted tensor station without allocating another control net.
pub fn fitted_station<C>(
    degree: [usize; 2],
    knots: &[Vec<f64>; 2],
    count: [usize; 2],
    controls: &[C],
    uv: [f64; 2],
    placement: &Matrix,
    components: impl Fn(&C) -> [f64; 4],
) -> Option<Vec3> {
    kernel::fitted_station(
        degree,
        knots,
        count,
        controls,
        uv,
        &Transform { m: *placement },
        components,
    )
    .map(record_vec3)
}

/// Find a common normal for a set of oriented planes.
pub fn coplanar_normal(planes: impl Iterator<Item = Plane>) -> Option<Vec3> {
    kernel::coplanar_normal(planes.map(|p| (vec3(p.origin), vec3(p.x), vec3(p.y)))).map(record_vec3)
}

/// Offsets for both wall caps and the native sewing tolerance.
pub fn wall_offsets(normal: Vec3, thickness: f64, symmetric: bool) -> Option<([Vec3; 2], f64)> {
    kernel::wall_offsets(vec3(normal), thickness, symmetric)
        .map(|(offsets, tolerance)| (offsets.map(record_vec3), tolerance))
}

/// A tensor strip between two translated copies of the exact trimmed boundary.
pub fn swept_controls(
    degree: usize,
    knots: &[f64],
    controls: &[[f64; 4]],
    range: [f64; 2],
    offsets: [Vec3; 2],
    work: &mut usize,
) -> Option<(Vec<f64>, Vec<[f64; 4]>)> {
    kernel::swept_controls(degree, knots, controls, range, offsets.map(vec3), work)
}
/// Sample a positive-weight spline with a conservative chord bound.
pub fn chord_samples(
    degree: usize,
    knots: &[f64],
    controls: &[[f64; 4]],
    range: [f64; 2],
    tolerance: f64,
    limit: usize,
) -> Option<Vec<Vec3>> {
    kernel::chord_samples(degree, knots, controls, range, tolerance, limit)
        .map(|path| record_list(&path))
}

/// Match a directed edge's endpoints in either orientation.
pub fn endpoint_directions(source: [Vec3; 2], target: [Vec3; 2], tolerance: f64) -> [bool; 2] {
    kernel::endpoint_directions(source.map(vec3), target.map(vec3), tolerance)
}

/// Nominate a reversed cyclic phase before checking continuous rim coincidence.
pub fn reversed_rim_phase(first: &[[Vec3; 2]], second: &[[Vec3; 2]], tolerance: f64) -> RimPhase {
    if first.len() != second.len() || first.len() > 256 {
        return RimPhase::Missing;
    }
    let converted =
        |edges: &[[Vec3; 2]]| edges.iter().map(|edge| edge.map(vec3)).collect::<Vec<_>>();
    kernel::reversed_rim_phase(&converted(first), &converted(second), tolerance)
}

/// Express rational controls in the support's definition frame.
pub fn rational_controls_in_frame(
    controls: &[[f64; 4]],
    inverse: &Matrix,
) -> Option<(Vec<Vec3>, Vec<f64>)> {
    let (points, weights) =
        kernel::rational_controls_in_frame(controls, &Transform { m: *inverse })?;
    Some((record_list(&points), weights))
}

/// Compare the ordered points after accounting for translation and positive uniform scale.
pub fn controls_follow(saved: &[Vec3], current: &[Vec3]) -> bool {
    kernel::controls_follow(
        saved.iter().copied().map(vec3),
        current.iter().copied().map(vec3),
    )
}

/// Whether one continued subdivision coefficient agrees to roundoff.
pub fn continued_control_within_roundoff(actual: Vec3, expected: Vec3, levels: usize) -> bool {
    kernel::continued_control_within_roundoff(vec3(actual), vec3(expected), levels)
}
