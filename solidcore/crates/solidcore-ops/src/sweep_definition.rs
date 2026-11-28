//! A body's screw-swept walls between its record and the kernel, body by body in the body's face
//! order: each face's definition as the record keeps it ([`definitions`]), and the definitions a
//! record holds given back to the faces of the body built from it as their labels ([`adopt`]):
//! the kernel works on the nets, as on walls read back as plain B-splines, and writes the
//! definitions back with them. The kernel reads and rebuilds; `solidcore-convert` copies the fields.

use solidcore::kernel::Kernel;
use solidcore::surface::procedural_sweep::record::{adopt_sweeps, face_sweep};
use solidcore::Brep;
use solidcore_record::brep::sweep::SweepDefinition;

use crate::error::{status, Error, Result};

/// Each face's definition, in the body's face order, where its surface is a screw-swept wall's
/// net carrying its definition (`face_sweep`). `None` for every other face.
///
/// # Errors
///
/// The kernel's reading of the body's faces, as the operation's ([`Error`]).
pub fn definitions(k: &Kernel, b: &Brep) -> Result<Vec<Option<SweepDefinition>>> {
    let mut out = Vec::with_capacity(b.face_list().len());
    for &f in b.face_list() {
        out.push(match b.face(f).map_err(status)?.surface.as_ref() {
            // A definition that does not read is no definition: the face is written as its net.
            Some(surface) => face_sweep(k, surface)
                .ok()
                .flatten()
                .and_then(|d| solidcore_convert::sweep::record_sweep(&d)),
            None => None,
        });
    }
    Ok(out)
}

/// Give the faces `faces` names (each by its index in the body's face order) the definitions their
/// record holds, as their labels: the face's net carries its wall's definition again, which no
/// evaluation reads (`adopt_sweeps`). A face whose net does not agree with its definition
/// keeps its net alone; nothing valid is refused.
///
/// # Errors
///
/// [`Error::Argument`] for an index past the body's faces; the kernel's refusals of a definition
/// it cannot rebuild, as the operation's.
pub fn adopt(k: &Kernel, b: &mut Brep, faces: &[(usize, &SweepDefinition)]) -> Result<()> {
    let ids = b.face_list().to_vec();
    let mut made = Vec::with_capacity(faces.len());
    for (i, d) in faces {
        let f = *ids.get(*i).ok_or(Error::Argument)?;
        let Some(record) = solidcore_convert::sweep::sweep(d) else {
            continue;
        };
        made.push((f, record));
    }
    adopt_sweeps(k, b, &made).map_err(status)
}
