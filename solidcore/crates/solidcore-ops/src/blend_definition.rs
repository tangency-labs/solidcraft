//! A body's blend faces between its record and the kernel, body by body in the body's face order:
//! each face's definition as the record keeps it ([`definitions`]), and the definitions a record
//! holds given back to the faces of the body built from it, rebuilt and their nets certified again
//! ([`adopt`]). The kernel reads and rebuilds; `solidcore-convert` copies the fields.

use solidcore::kernel::Kernel;
use solidcore::surface::procedural_blend::convert::{adopt_definitions, face_definition};
use solidcore::{Brep, SolidError};
use solidcore_record::brep::blend::BlendDefinition;

use crate::error::{status, Error, Result};

/// Each face's definition, in the body's face order, where its surface carries one a record keeps:
/// a procedural blend's, or a round of an exact class's whose net has no analytic label of its own
/// (`face_definition`). `None` for every other face.
///
/// # Errors
///
/// The kernel's, as the operation's ([`Error`]).
pub fn definitions(k: &Kernel, b: &Brep) -> Result<Vec<Option<BlendDefinition>>> {
    let mut out = Vec::with_capacity(b.face_list().len());
    for &f in b.face_list() {
        out.push(match b.face(f).map_err(status)?.surface.as_ref() {
            Some(surface) => face_definition(k, surface)
                .map_err(status)?
                .map(solidcore_convert::blend::record_blend),
            None => None,
        });
    }
    Ok(out)
}

/// Give the faces `faces` names (each by its index in the body's face order) the definitions their
/// record holds: the face's surface, its net, becomes the blend the definition makes, the net
/// certified against it again (`adopt_definitions`).
///
/// # Errors
///
/// [`Error::Argument`] for an index past the body's faces; [`Error::DefinitionNotCertified`] for a
/// net its definition does not certify; the kernel's other refusals, as the operation's.
pub fn adopt(k: &Kernel, b: &mut Brep, faces: &[(usize, &BlendDefinition)]) -> Result<()> {
    let ids = b.face_list().to_vec();
    for (i, d) in faces {
        let f = *ids.get(*i).ok_or(Error::Argument)?;
        let definition = [(f, solidcore_convert::blend::blend(d))];
        adopt_definitions(k, b, &solidcore::topology::loopuse::notify_ref, &definition).map_err(
            |e| match e {
                SolidError::DefinitionNotCertified { eps, .. } => {
                    Error::DefinitionNotCertified { face: *i, eps }
                }
                e => status(e),
            },
        )?;
    }
    Ok(())
}
