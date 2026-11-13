//! Ordered source-curve controls and rim tests in portable record coordinates.
use crate::affine::Matrix;
use solidcore::{geom::form_curves as kernel, Transform};
use solidcore_convert::{record_list, record_vec3, vec3};
use solidcore_record::{Plane, Vec3};

pub use kernel::RimPhase;

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
