//! A body's blend faces and their record's definitions, through the modelling layer
//! (`solidcore_ops::blend_definition`): read off the body's faces for its record, and given back to
//! the faces of a body built from a record, rebuilt and their nets certified again. The flat
//! arrays carry only the nets.

use solidcore_record::brep::blend::BlendDefinition;
use solidcore_record::brep::sweep::SweepDefinition;

use super::{Body, with_body};
use crate::{Error, Result};

impl super::dump::Answer for solidcore_ops::Result<Vec<Option<BlendDefinition>>> {}
impl super::dump::Answer for solidcore_ops::Result<()> {}
impl super::dump::Answer for solidcore_ops::Result<Vec<Option<SweepDefinition>>> {}

/// The modelling layer's refusal, as this crate's.
fn refused(e: solidcore_ops::Error) -> Error {
    Error::Solidcore(e.to_string())
}

/// Each face's definition, in the body's face order (the arrays'), where its surface carries one a
/// record keeps.
pub(crate) fn definitions(body: &Body) -> Result<Vec<Option<BlendDefinition>>> {
    with_body("blend_definition::definitions", body, |k, b| Ok(solidcore_ops::blend_definition::definitions(k, b)))?.map_err(refused)
}

/// Give the faces `faces` names (by their index in the body's face order) their definitions back.
pub(crate) fn adopt(body: &Body, faces: &[(usize, &BlendDefinition)]) -> Result<()> {
    with_body("blend_definition::adopt", body, |k, b| Ok(solidcore_ops::blend_definition::adopt(k, b, faces)))?.map_err(refused)
}

/// Each face's screw-sweep definition, in the body's face order (the arrays'), where its surface is
/// a screw-swept wall's net carrying one (`solidcore_ops::sweep_definition::definitions`).
pub(crate) fn sweeps(body: &Body) -> Result<Vec<Option<SweepDefinition>>> {
    with_body("sweep_definition::definitions", body, |k, b| Ok(solidcore_ops::sweep_definition::definitions(k, b)))?.map_err(refused)
}

/// Give the faces `faces` names (by their index in the body's face order) their screw-sweep
/// definitions back; a face whose net a definition does not describe keeps its net alone.
pub(crate) fn adopt_sweeps(body: &Body, faces: &[(usize, &SweepDefinition)]) -> Result<()> {
    with_body("sweep_definition::adopt", body, |k, b| Ok(solidcore_ops::sweep_definition::adopt(k, b, faces)))?.map_err(refused)
}
