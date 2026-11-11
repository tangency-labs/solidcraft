//! A body's blend faces and their record's definitions (`solidcore_ops::blend_definition`): read
//! off the body's faces for its record, and given back to the faces of a body built from a record,
//! rebuilt and their nets certified again. The flat arrays carry only the nets.

use solidcore::topology::brep::p1 as bl;
use solidcore_record::brep::blend::BlendDefinition;

use super::{Body, with_body};
use crate::Result;

impl super::dump::Answer for Vec<Option<BlendDefinition>> {}

/// Each face's definition, in the body's face order (the arrays'), where its surface carries one a
/// record keeps.
pub(crate) fn definitions(body: &Body) -> Result<Vec<Option<BlendDefinition>>> {
    with_body("blend_definition::record_definition", body, |k, b| {
        let mut out = Vec::new();
        for f in bl::faces(b, None)? {
            out.push(match b.face(f)?.surface.as_ref() {
                Some(surface) => solidcore_ops::blend_definition::record_definition(k, surface)?,
                None => None,
            });
        }
        Ok(out)
    })
}

/// Give the faces `faces` names (in the body's face order) their definitions back.
pub(crate) fn adopt(body: &Body, faces: &[(usize, &BlendDefinition)]) -> Result<()> {
    with_body("blend_definition::adopt", body, |k, b| {
        let ids = bl::faces(b, None)?;
        let mut list = Vec::with_capacity(faces.len());
        for (i, definition) in faces {
            let f = *ids.get(*i).ok_or_else(|| solidcore::SolidError::invalid_input("blend_definition::adopt", "a face the body does not have"))?;
            list.push((f, *definition));
        }
        solidcore_ops::blend_definition::adopt(k, b, &list)
    })
}
