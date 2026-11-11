//! A body's blend faces between its record and the kernel: a face's definition read off its
//! surface as the record keeps it ([`record_definition`]), and the definitions a record holds given
//! back to the faces of the body built from it, rebuilt and their nets certified again
//! ([`adopt`]). The kernel reads and rebuilds; `solidcore-convert` copies the fields.

use solidcore::kernel::Kernel;
use solidcore::surface::procedural_blend::convert::{adopt_definitions, face_definition};
use solidcore::surface::surface::Surface;
use solidcore::{Brep, FaceId, SolidError};
use solidcore_record::brep::blend::BlendDefinition;

/// The definition a face's surface carries, as a record keeps it: a procedural blend's, or a round
/// of an exact class's whose net has no analytic label of its own (`face_definition`). `None` for
/// any other surface.
///
/// # Errors
///
/// Those of reading the definition off the surface.
pub fn record_definition(
    k: &Kernel,
    surface: &Surface,
) -> Result<Option<BlendDefinition>, SolidError> {
    Ok(face_definition(k, surface)?.map(solidcore_convert::blend::record_blend))
}

/// Give each face of `faces` the definition its record holds: the face's surface, its net, becomes
/// the blend the definition makes, the net certified against it again (`adopt_definitions`).
///
/// # Errors
///
/// `SolidError::DefinitionNotCertified` for a net its definition does not certify; those of the
/// rebuilds.
pub fn adopt(
    k: &Kernel,
    b: &mut Brep,
    faces: &[(FaceId, &BlendDefinition)],
) -> Result<(), SolidError> {
    let definitions: Vec<_> = faces
        .iter()
        .map(|(f, d)| (*f, solidcore_convert::blend::blend(d)))
        .collect();
    adopt_definitions(
        k,
        b,
        &solidcore::topology::loopuse::notify_ref,
        &definitions,
    )
}
