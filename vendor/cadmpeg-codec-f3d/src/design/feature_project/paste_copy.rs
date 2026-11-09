// SPDX-License-Identifier: Apache-2.0
//! Project a paste of copies within one component as a body move.

use crate::ids::{self, native_stream};
use crate::records::bodies::DesignBodyBinding;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_ir::math::Vector3;

/// A paste of distinct copies whose every source and copy are current bodies
/// of one component: a move with one copy and no transform.
pub(super) fn same_component_paste_copy(
    scope: &DesignParameterScope,
    construction: &crate::records::feature::base_feature::DesignBaseFeatureConstruction,
    body_bindings: &[DesignBodyBinding],
) -> Option<(
    cadmpeg_ir::features::FeatureDefinition,
    Vec<cadmpeg_ir::ids::BodyId>,
)> {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};

    let stream = native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM);
    let current_body = |suffix: u64| {
        let mut matches = body_bindings.iter().filter(|binding| {
            native_stream(binding.id()) == Some(stream) && binding.entity_suffix == suffix
        });
        let binding = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        let component = binding.component_owner.as_ref()?.component_for(suffix)?;
        Some((binding.body.as_ref()?, component))
    };
    let sources = construction.copy_source_suffixes();
    let copies = construction.body_entity_suffixes();
    if sources.len() == 0 || sources.len() != copies.len() {
        return None;
    }
    let mut source_bodies = Vec::new();
    let mut copy_bodies = Vec::new();
    let mut component = None;
    for (source, copy) in sources.zip(copies) {
        let (source_body, source_component) = current_body(source)?;
        let (copy_body, copy_component) = current_body(copy)?;
        if source_component != copy_component
            || *component.get_or_insert(source_component) != source_component
        {
            return None;
        }
        source_bodies.push(source_body.clone());
        copy_bodies.push(copy_body.clone());
    }
    let distinct_copies =
        cadmpeg_ir::features::DistinctMembers::try_from(copy_bodies.clone()).is_ok();
    if !distinct_copies || copy_bodies.iter().any(|copy| source_bodies.contains(copy)) {
        return None;
    }
    Some((
        FeatureDefinition::Operation(FeatureOperation::MoveBody {
            bodies: BodySelection::Resolved {
                bodies: cadmpeg_ir::features::DistinctMembers::try_from(source_bodies).ok()?,
                native: scope.id.clone(),
            },
            translation: cadmpeg_ir::features::FiniteVector3::new(Vector3::new(0.0, 0.0, 0.0))?,
            rotation: None,
            copies: 1,
        }),
        copy_bodies,
    ))
}

#[cfg(test)]
mod tests;
