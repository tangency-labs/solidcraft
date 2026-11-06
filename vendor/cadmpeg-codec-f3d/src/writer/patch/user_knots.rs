// SPDX-License-Identifier: Apache-2.0
//! Validate user-knot edits against unchanged source control cages.

use std::collections::BTreeMap;

use cadmpeg_core::CodecError;
use cadmpeg_ir::{subd::SubdCage, CadIr};

pub(super) fn edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, SubdCage>, CodecError> {
    let before = &baseline.model.subds;
    let after: BTreeMap<_, _> = target
        .model
        .subds
        .iter()
        .map(|surface| (&surface.id, surface))
        .collect();
    if before.len() != after.len() || after.len() != target.model.subds.len() {
        return Err(CodecError::NotImplemented(
            "T-spline user-knot edits require the unchanged surface-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for surface in before {
        let Some(candidate) = after.get(&surface.id) else {
            return Err(CodecError::NotImplemented(
                "T-spline user-knot edits require the unchanged surface-id set".into(),
            ));
        };
        if surface == *candidate {
            continue;
        }
        if surface.cage.edges().len() != candidate.cage.edges().len() {
            return Err(CodecError::NotImplemented(
                "T-spline user-knot edits require unchanged edge topology".into(),
            ));
        }
        let mut supported = surface.clone();
        supported
            .cage
            .edit_edges(|edges| {
                for (edge, desired) in edges.iter_mut().zip(candidate.cage.edges()) {
                    edge.user_knot = desired.user_knot;
                }
                Ok(())
            })
            .map_err(|error| CodecError::malformed(error.to_string()))?;
        if supported != **candidate {
            return Err(CodecError::NotImplemented(
                "T-spline user-knot edits require unchanged cage geometry and topology".into(),
            ));
        }
        let source = surface
            .source_object
            .as_ref()
            .filter(|source| source.format == cadmpeg_ir::CodecFormat::F3d)
            .ok_or_else(|| {
                CodecError::NotImplemented(
                    "T-spline user-knot edits require a source TSM member".into(),
                )
            })?;
        if edits
            .insert(source.object_id.as_str().to_owned(), candidate.cage.clone())
            .is_some()
        {
            return Err(CodecError::malformed(
                "T-spline surfaces share a source TSM member",
            ));
        }
    }
    Ok(edits)
}
