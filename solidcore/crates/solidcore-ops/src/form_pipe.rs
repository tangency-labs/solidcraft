//! Convert sampled Form paths for arc-length and junction queries.

use crate::form_cage::{record_cage, CageGeometry};
use solidcore::geom::form_pipe as kernel;
use solidcore_convert::{record_vec3, vec3};
use solidcore_record::Vec3;

pub use kernel::{JOIN_TOLERANCE, MAX_PATHS, MAX_PATH_SAMPLES};

/// An ordered ring of existing branch controls and its alignment parameters.
pub struct JunctionPort {
    /// Branch direction away from the junction.
    pub direction: Vec3,
    /// Ordered branch control IDs.
    pub ring: Vec<usize>,
    /// Branch radius.
    pub radius: f64,
    /// Whether to consume the branch ring in reverse order.
    pub reverse: bool,
}

/// Build only the controls and faces appended by the junction. Face indices
/// reference the complete assembled cage, including its existing controls.
pub fn append_junction(
    points: &[Vec3],
    face_count: usize,
    center: Vec3,
    ports: &[JunctionPort],
    density: usize,
) -> solidcore::Result<CageGeometry> {
    kernel::validate_branch_count(ports.len())?;
    if !(1..=12).contains(&density)
        || ports.len() < 3
        || ports.iter().any(|p| p.ring.len() != 8 * density)
    {
        return Err(solidcore::SolidError::InvalidInput {
            operation: "Form pipe",
            reason: "a pipe junction needs 3..6 compatible branch rings",
        });
    }
    let ports = ports
        .iter()
        .map(|port| {
            let ring = port
                .ring
                .iter()
                .map(|&id| {
                    points
                        .get(id)
                        .copied()
                        .map(|point| (id, vec3(point)))
                        .ok_or(solidcore::SolidError::InvalidInput {
                            operation: "Form pipe",
                            reason: "pipe junction lost a branch rim control",
                        })
                })
                .collect::<solidcore::Result<_>>()?;
            Ok(kernel::junction::Port {
                direction: vec3(port.direction),
                ring,
                radius: port.radius,
                reverse: port.reverse,
            })
        })
        .collect::<solidcore::Result<Vec<_>>>()?;
    kernel::junction::append_geometry(points.len(), face_count, vec3(center), &ports, density)
        .map(record_cage)
}

/// A swept cage and its directed, ordered terminal rings.
pub struct Sweep {
    /// Quad cage before attaching other branches.
    pub cage: CageGeometry,
    /// Initial and final rings in section order.
    pub ends: [Vec<usize>; 2],
    /// Path tangents at the two ends, both in the sweep direction.
    pub tangents: [Vec3; 2],
}

/// Sweep a sampled path with optional caps on the open ends.
pub fn sweep(
    points: &[Vec3],
    radius: f64,
    density: usize,
    closed: bool,
    caps: [bool; 2],
) -> solidcore::Result<Sweep> {
    let sweep = kernel::sweep::tube(&path(points)?, radius, density, closed, caps)?;
    Ok(Sweep {
        cage: record_cage(sweep.cage),
        ends: sweep.ends,
        tangents: sweep.tangents.map(record_vec3),
    })
}

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
    kernel::validate_branch_count(chords.len())?;
    let chords = chords
        .iter()
        .map(|&(points, diameter)| (points.map(vec3), diameter))
        .collect::<Vec<_>>();
    kernel::clearance(&chords)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn junction_branch_limit_has_its_own_refusal_reason() {
        assert!(matches!(
            clearance(&[([Vec3::ZERO, Vec3::X], 1.0); 7]),
            Err(solidcore::SolidError::InvalidInput {
                reason: "pipe junction supports at most six branches",
                ..
            })
        ));
    }
}
