//! Adapt a kernel sweep to a Form cage and retain its ordered terminal rings.
use super::{Cage, Result, bad};
use solidcore_record::Vec3;

pub(super) struct Sweep {
    pub cage: Cage,
    pub ends: [Vec<usize>; 2],
    pub tangents: [Vec3; 2],
}

/// Sweep a sampled path. A closed path has no terminal caps; an open path may
/// leave either terminal ring open for a network junction to attach to it.
pub fn swept_path(points: &[Vec3], radius: f64, density: usize, closed: bool, caps: [bool; 2]) -> Result<Cage> {
    Ok(tube(points, radius, density, closed, caps)?.cage)
}

pub(super) fn tube(points: &[Vec3], radius: f64, density: usize, closed: bool, caps: [bool; 2]) -> Result<Sweep> {
    let sweep = solidcore_ops::form_pipe::sweep(points, radius, density, closed, caps).map_err(|error| bad(&error.to_string()))?;
    let cage = Cage { points: sweep.cage.points, faces: sweep.cage.faces, ..Default::default() };
    cage.validate()?;
    Ok(Sweep { cage, ends: sweep.ends, tangents: sweep.tangents })
}
