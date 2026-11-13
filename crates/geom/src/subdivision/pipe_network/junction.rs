//! Attach a kernel junction to the existing branch cage.
use super::{Cage, Result, bad};
use solidcore_record::Vec3;

pub(super) use solidcore_ops::form_pipe::JunctionPort as Port;

pub(super) fn attach(cage: &mut Cage, center: Vec3, ports: &[Port], density: usize) -> Result<()> {
    let addition =
        solidcore_ops::form_pipe::append_junction(&cage.points, cage.faces.len(), center, ports, density).map_err(|error| bad(&error.to_string()))?;
    cage.points.extend(addition.points);
    cage.faces.extend(addition.faces);
    Ok(())
}
