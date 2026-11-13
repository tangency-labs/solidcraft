//! Convert sampled Form paths for arc-length and junction queries.

use solidcore::geom::form_pipe as kernel;
use solidcore_convert::{record_vec3, vec3};
use solidcore_record::Vec3;

pub use kernel::{MAX_PATHS, MAX_PATH_SAMPLES};

fn budget() -> solidcore::SolidError {
    solidcore::SolidError::InvalidInput {
        operation: "Form pipe",
        reason: "pipe path sample budget exceeded",
    }
}

fn path(points: &[Vec3]) -> solidcore::Result<Vec<solidcore::Vec3>> {
    if points.len() > MAX_PATH_SAMPLES {
        return Err(budget());
    }
    Ok(points.iter().copied().map(vec3).collect())
}

/// Read the arc length of a bounded sampled path.
pub fn length(points: &[Vec3]) -> solidcore::Result<f64> {
    kernel::length(&path(points)?)
}

/// Trim a path by arc-length distances measured from its ends.
pub fn trim(points: &[Vec3], start: f64, end: f64) -> solidcore::Result<Vec<Vec3>> {
    kernel::trim(&path(points)?, start, end).map(|path| path.into_iter().map(record_vec3).collect())
}

/// Sample ordered, oriented paths with the requested average interval count.
pub fn sample(
    paths: &[(&[Vec3], bool)],
    average: usize,
    minimum: usize,
) -> solidcore::Result<Vec<Vec3>> {
    if paths.len() > MAX_PATHS
        || paths
            .iter()
            .fold(0usize, |n, (points, _)| n.saturating_add(points.len()))
            > MAX_PATH_SAMPLES
    {
        return Err(budget());
    }
    let converted = paths
        .iter()
        .map(|(points, forward)| Ok((path(points)?, *forward)))
        .collect::<solidcore::Result<Vec<_>>>()?;
    let views = converted
        .iter()
        .map(|(points, forward)| (points.as_slice(), *forward))
        .collect::<Vec<_>>();
    kernel::sample(&views, average, minimum).map(|path| path.into_iter().map(record_vec3).collect())
}

/// Read the junction clearance from outward branch chords and diameters.
pub fn clearance(chords: &[([Vec3; 2], f64)]) -> solidcore::Result<f64> {
    if chords.len() > 6 {
        return Err(budget());
    }
    let chords = chords
        .iter()
        .map(|&(points, diameter)| (points.map(vec3), diameter))
        .collect::<Vec<_>>();
    kernel::clearance(&chords)
}
