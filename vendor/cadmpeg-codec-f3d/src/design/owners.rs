// SPDX-License-Identifier: Apache-2.0
//! Bind component-local construction history to neutral product definitions.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    DistinctMembers, FeatureDefinition, FeatureOperation, SketchFeatureBinding,
};
use cadmpeg_ir::{
    document::Model, ids::ProductDefinitionId, report::loss::LossNote, sketches::SketchId,
};

use super::feature_project::{ComponentHistoryNamespace, ScopeHistoryGraph};
use crate::{ids::native_stream, loss::F3dLossCode, native::F3dNative};

/// Loss reason when a sketch collection names another product definition.
const COLLECTION_CONFLICT: &str = "its sketch collection names a different product definition";
/// Loss reason when two Sketch scopes in different definitions bind one
/// unlisted sketch.
const CLUSTER_CONFLICT: &str =
    "another Sketch scope in a different product definition binds the same unlisted sketch";

pub(crate) fn attach(
    scan: &crate::container::ContainerScan<'_>,
    native: &F3dNative,
    model: &mut Model,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    if model.features.is_empty() && model.parameters.is_empty() {
        return Ok(());
    }
    let components = super::decode::components::decode_local_components(
        scan,
        &native.design_component_naming_spaces,
    )?;
    let implicit_roots = components
        .values()
        .filter_map(|stream| {
            stream
                .root_component
                .and_then(|root| stream.roots.get(&root))
                .or_else(|| {
                    if stream.roots.len() == 1 {
                        stream.roots.values().next()
                    } else {
                        None
                    }
                })
        })
        .map(|root| crate::ids::neutral_component_id(&root.context_uuid))
        .collect();
    attach_with_implicit_roots(native, model, losses, &implicit_roots)
}

fn attach_with_implicit_roots(
    native: &F3dNative,
    model: &mut Model,
    losses: &mut Vec<LossNote>,
    implicit_roots: &BTreeSet<ProductDefinitionId>,
) -> Result<(), CodecError> {
    let mut owners = BTreeMap::new();
    for scope in &native.design_parameter_scopes {
        if !model
            .features
            .iter()
            .any(|row| row.native_ref.as_deref() == Some(scope.id.as_str()))
        {
            continue;
        }
        let owner = resolve(
            native,
            model,
            losses,
            implicit_roots,
            &scope.id,
            scope.record_index,
        );
        owners.insert(scope.id.as_str(), owner);
    }
    for feature in &mut model.features {
        feature.product_definition = feature
            .native_ref
            .as_deref()
            .and_then(|id| owners.get(id))
            .cloned()
            .flatten();
    }
    let mut moved = Vec::new();
    let member_root = crate::ids::archive_product_definition_id();
    let unowned = model
        .bodies
        .iter()
        .map(|body| &body.id)
        .filter(|body| {
            !model
                .product_definitions
                .iter()
                .any(|row| row.bodies.contains(body))
        })
        .cloned()
        .collect::<BTreeSet<_>>();
    // Sketch collection listings as decoded, before scope ownership adds
    // unlisted sketches; the explicit collection reference outranks every
    // later ownership move.
    let mut listed = BTreeMap::<SketchId, BTreeSet<ProductDefinitionId>>::new();
    for definition in &model.product_definitions {
        for sketch in &definition.sketches {
            listed
                .entry(sketch.clone())
                .or_default()
                .insert(definition.id.clone());
        }
    }
    let mut bound_by_scope = BTreeMap::new();
    for feature in &mut model.features {
        let owned_bodies = feature.product_definition.as_ref().and_then(|owner| {
            model
                .product_definitions
                .iter()
                .find(|row| &row.id == owner)
                .map(|row| row.bodies.iter().collect::<BTreeSet<_>>())
                .or_else(|| (owner == &member_root).then(|| unowned.iter().collect()))
        });
        if let Some(owned_bodies) = owned_bodies {
            let outputs = feature.evaluation.outputs();
            let retained = outputs
                .iter()
                .filter(|body| owned_bodies.contains(body))
                .cloned()
                .collect::<Vec<_>>();
            // A feature can place its result bodies in another component than
            // the one whose naming space holds its scope. When one product
            // definition owns every output body, that definition owns the
            // feature.
            let output_owner = if retained.len() == outputs.len() || outputs.is_empty() {
                None
            } else {
                let mut holders = model
                    .product_definitions
                    .iter()
                    .filter(|row| outputs.iter().all(|body| row.bodies.contains(body)));
                match (holders.next(), holders.next()) {
                    (Some(holder), None) => Some(holder.id.clone()),
                    _ => None,
                }
            };
            if let Some(holder) = output_owner {
                feature.product_definition = Some(holder.clone());
                moved.push((feature.id.clone(), holder));
            } else if retained.len() != outputs.len() {
                losses.push(F3dLossCode::ScopeOutputOwnerMismatch.note(format!(
                    "Feature {} has {} output body link(s) outside its product definition; those links remain native.",
                    feature.id, outputs.len() - retained.len()
                )));
                feature
                    .evaluation
                    .set_outputs(DistinctMembers::try_from(retained).map_err(CodecError::from)?);
            }
        }
        if let FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: SketchFeatureBinding::Planar(Some(sketch)),
        }) = feature.evaluation.definition()
        {
            let Some(cluster_owner) = feature.product_definition.clone() else {
                continue;
            };
            let sketch = sketch.clone();
            let mut holders = model
                .product_definitions
                .iter()
                .filter(|row| row.sketches.contains(&sketch));
            // The sketch collection links a sketch to its component by an
            // explicit reference; the entity-id cluster of the Sketch scope
            // is positional. A sketch listed by one definition belongs to the
            // Sketch feature that binds it, whichever cluster holds the scope.
            let owner = match (holders.next(), holders.next()) {
                (None, _) => {
                    // No collection lists the sketch: its scope cluster
                    // places it, and a second cluster claiming it conflicts.
                    // The first claim in model order keeps the sketch.
                    match bound_by_scope.insert(sketch.clone(), cluster_owner.clone()) {
                        Some(earlier) if earlier != cluster_owner => Err(CLUSTER_CONFLICT),
                        _ => Ok(cluster_owner),
                    }
                }
                (Some(holder), None) => match bound_by_scope.get(&sketch) {
                    Some(earlier) if earlier != &cluster_owner => Err(COLLECTION_CONFLICT),
                    _ => Ok(holder.id.clone()),
                },
                (Some(_), Some(_)) => Err(COLLECTION_CONFLICT),
            };
            let owner = match owner {
                Ok(owner) => owner,
                Err(reason) => {
                    losses.push(F3dLossCode::ScopeSketchOwnerMismatch.note(format!(
                        "Feature {} cannot own its sketch: {reason}; the sketch binding remains native.", feature.id
                    )));
                    feature
                        .evaluation
                        .set_definition(FeatureDefinition::Operation(FeatureOperation::Sketch {
                            sketch: SketchFeatureBinding::Planar(None),
                        }));
                    continue;
                }
            };
            feature.product_definition = Some(owner.clone());
            if let Some(definition) = model
                .product_definitions
                .iter_mut()
                .find(|row| row.id == owner)
            {
                if !definition.sketches.contains(&sketch) {
                    definition.sketches.push(sketch);
                }
            }
        }
    }
    follow_consumed_inputs(model, moved, &listed)?;
    for index in 0..model.parameters.len() {
        let parameter = &model.parameters[index];
        let inherited = parameter
            .owner
            .as_ref()
            .and_then(|owner| model.features.iter().find(|row| &row.id == owner))
            .map(|row| row.product_definition.clone());
        let source = parameter
            .native_ref
            .as_deref()
            .and_then(|id| native.design_parameters.iter().find(|row| row.id == id));
        let owner = if let Some(inherited) = inherited {
            inherited
        } else if let Some(source) = source {
            let record_index = source
                .owner_record_index()
                .and_then(|record| {
                    native.design_parameter_owners.iter().find(|row| {
                        native_stream(row.id()) == native_stream(&source.id)
                            && row.record_index() == record
                    })
                })
                .map_or(
                    source.record_index,
                    crate::records::parameters::DesignParameterOwner::scope_record_index,
                );
            resolve(
                native,
                model,
                losses,
                implicit_roots,
                &source.id,
                record_index,
            )
        } else {
            None
        };
        model.parameters[index].product_definition = owner;
    }
    Ok(())
}

/// Moves the inputs a feature consumes to the definition its outputs moved
/// it to, so that definition's history replays on its own.
///
/// A consumed input is a dependency feature, or the sketch feature whose
/// sketch the definition names. An input moves only when it produces no body
/// and every feature that consumes it already belongs to the new definition;
/// a moved sketch feature carries its planar sketch along. A sketch feature
/// whose sketch a collection of another definition lists stays with that
/// definition: the explicit collection reference outranks consumption.
fn follow_consumed_inputs(
    model: &mut Model,
    mut moved: Vec<(cadmpeg_ir::features::FeatureId, ProductDefinitionId)>,
    listed: &BTreeMap<SketchId, BTreeSet<ProductDefinitionId>>,
) -> Result<(), CodecError> {
    if moved.is_empty() {
        return Ok(());
    }
    let sketch_rows = model
        .features
        .iter()
        .enumerate()
        .filter_map(|(index, row)| match row.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: SketchFeatureBinding::Planar(Some(sketch)),
            }) => Some((sketch.as_str().to_owned(), index)),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let consumed = model
        .features
        .iter()
        .map(|row| {
            let mut inputs = row
                .dependencies
                .iter()
                .filter_map(|id| model.features.iter().position(|other| &other.id == id))
                .collect::<BTreeSet<_>>();
            let text = serde_json::to_string(row.evaluation.definition()).map_err(|error| {
                CodecError::malformed(format_args!(
                    "feature definition does not serialize: {error}"
                ))
            })?;
            for (sketch, index) in &sketch_rows {
                if text.contains(&format!("\"{sketch}\"")) && model.features[*index].id != row.id {
                    inputs.insert(*index);
                }
            }
            Ok(inputs)
        })
        .collect::<Result<Vec<_>, CodecError>>()?;
    while let Some((feature, owner)) = moved.pop() {
        let Some(index) = model.features.iter().position(|row| row.id == feature) else {
            continue;
        };
        for &input in &consumed[index] {
            let row = &model.features[input];
            if row.product_definition.as_ref() == Some(&owner)
                || !row.evaluation.outputs().is_empty()
            {
                continue;
            }
            let all_consumers_moved = consumed.iter().enumerate().all(|(consumer, inputs)| {
                !inputs.contains(&input)
                    || model.features[consumer].product_definition.as_ref() == Some(&owner)
            });
            if !all_consumers_moved {
                continue;
            }
            if let FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: SketchFeatureBinding::Planar(Some(sketch)),
            }) = model.features[input].evaluation.definition()
            {
                if listed
                    .get(sketch)
                    .is_some_and(|holders| !holders.contains(&owner))
                {
                    continue;
                }
            }
            let previous = model.features[input]
                .product_definition
                .replace(owner.clone());
            if let FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: SketchFeatureBinding::Planar(Some(sketch)),
            }) = model.features[input].evaluation.definition().clone()
            {
                if let Some(definition) = previous.as_ref().and_then(|old| {
                    model
                        .product_definitions
                        .iter_mut()
                        .find(|row| &row.id == old)
                }) {
                    definition.sketches.retain(|row| row != &sketch);
                }
                if let Some(definition) = model
                    .product_definitions
                    .iter_mut()
                    .find(|row| row.id == owner)
                {
                    if !definition.sketches.contains(&sketch) {
                        definition.sketches.push(sketch);
                    }
                }
            }
            moved.push((model.features[input].id.clone(), owner.clone()));
        }
    }
    Ok(())
}

fn resolve(
    native: &F3dNative,
    model: &Model,
    losses: &mut Vec<LossNote>,
    implicit_roots: &BTreeSet<ProductDefinitionId>,
    id: &str,
    record_index: u32,
) -> Option<ProductDefinitionId> {
    let namespace = ScopeHistoryGraph::component_namespace_at(
        id,
        record_index,
        &native.design_component_naming_spaces,
    );
    let mut candidates = BTreeSet::new();
    match namespace {
        Some(ComponentHistoryNamespace::Component(record)) => {
            for space in &native.design_component_naming_spaces {
                if space.component_record_index == record
                    && native_stream(&space.id) == native_stream(id)
                {
                    let id = crate::ids::neutral_component_id(&space.context_uuid);
                    if model
                        .product_definitions
                        .iter()
                        .any(|definition| definition.id == id)
                    {
                        candidates.insert(id);
                    } else if implicit_roots.contains(&id) {
                        candidates.insert(crate::ids::archive_product_definition_id());
                    }
                }
            }
        }
        Some(ComponentHistoryNamespace::Aggregate) => {
            candidates.insert(crate::ids::archive_product_definition_id());
        }
        None => return None,
    }
    let owner: Option<ProductDefinitionId> = if candidates.len() == 1 {
        candidates.into_iter().next()
    } else {
        let cause = if candidates.is_empty() {
            "missing-definition"
        } else {
            "multiple-definitions"
        };
        losses.push(
            F3dLossCode::ScopeOwnerUnresolved
                .note(format!("Scope {id} product ownership unresolved: {cause}.")),
        );
        None
    };
    owner
}

/// Clears product owners that name a definition absent from the document.
///
/// Root-level design history names the archive member definition. F3Z merge
/// creates that definition for each external member at every decode depth; a
/// standalone document and the archive root have none, so their root-level
/// history has no product owner.
pub(crate) fn release_unbound(model: &mut Model) {
    let defined = model
        .product_definitions
        .iter()
        .map(|row| row.id.clone())
        .collect::<BTreeSet<_>>();
    for feature in &mut model.features {
        if feature
            .product_definition
            .as_ref()
            .is_some_and(|owner| !defined.contains(owner))
        {
            feature.product_definition = None;
        }
    }
    for parameter in &mut model.parameters {
        if parameter
            .product_definition
            .as_ref()
            .is_some_and(|owner| !defined.contains(owner))
        {
            parameter.product_definition = None;
        }
    }
}

#[cfg(test)]
mod tests;
