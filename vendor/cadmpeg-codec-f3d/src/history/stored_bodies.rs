// SPDX-License-Identifier: Apache-2.0
//! Bind stored-body scopes and report unresolved construction semantics.

use cadmpeg_ir::features::{BodySelection, Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::topology::Body;
use std::collections::HashMap;

use crate::loss::F3dLossCode;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};

pub(super) struct ActiveBodies<'a> {
    by_slot: HashMap<i64, Vec<&'a BodyId>>,
}

impl<'a> ActiveBodies<'a> {
    pub(super) fn new(bodies: &'a [Body]) -> Self {
        let mut by_slot = HashMap::<i64, Vec<&BodyId>>::new();
        for body in bodies {
            if let Some(slot) = super::stable_ref(body.id.as_str()) {
                by_slot.entry(slot).or_default().push(&body.id);
            }
        }
        Self { by_slot }
    }

    pub(super) fn resolve(
        &self,
        slots: &[i64],
        source: Option<&str>,
    ) -> Option<cadmpeg_ir::features::DistinctMembers<BodyId>> {
        let mut bodies = Vec::new();
        for slot in slots {
            let mut matches = self.by_slot.get(slot)?.iter().copied().filter(|body| {
                source.is_none_or(|source| {
                    body.as_str().starts_with("f3d:brep:entity#")
                        || super::entity_in_brep_source(body.as_str(), source)
                })
            });
            let body = matches.next()?;
            if matches.any(|candidate| candidate != body) {
                return None;
            }
            bodies.push(body.clone());
        }
        bodies.try_into().ok()
    }
}

/// Both spellings of the derived base kind. The scope stores a copy of a source
/// that the archive does not contain.
fn is_derived_base(scope: &DesignParameterScope) -> bool {
    matches!(
        scope.kind_name(),
        "Derived Base Feature" | "DerivedBaseFeature"
    )
}

pub(super) fn bind_history_outputs(
    feature: &mut Feature,
    scope: &DesignParameterScope,
    slots: &[i64],
    source: Option<&str>,
    active: &ActiveBodies<'_>,
) -> bool {
    if scope.kind() != DesignFeatureKind::BaseFeature && !is_derived_base(scope) {
        return false;
    }
    let native = match feature.evaluation.definition() {
        FeatureDefinition::Operation(FeatureOperation::Native { .. }) => scope.id.clone(),
        FeatureDefinition::Operation(FeatureOperation::BaseFeature {
            bodies: BodySelection::Native(native),
        }) => native.clone(),
        _ => return false,
    };
    if scope
        .base_feature_construction()
        .is_some_and(|construction| construction.body_entity_suffixes().len() != slots.len())
    {
        return false;
    }
    let Some(bodies) = active.resolve(slots, source) else {
        return false;
    };
    feature.evaluation.set_outputs(bodies);
    feature
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::BaseFeature {
                bodies: BodySelection::Native(native),
            },
        ));
    true
}

pub(super) fn finalize_body_sets(feature: &mut Feature) {
    if let FeatureDefinition::Operation(FeatureOperation::BaseFeature {
        bodies: BodySelection::Resolved { bodies, .. },
    }) = feature.evaluation.definition()
    {
        let outputs = bodies.clone();
        feature.evaluation.set_outputs(outputs);
        return;
    }
    if matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::BaseFeature {
            bodies: BodySelection::Native(_) | BodySelection::Unresolved,
        })
    ) {
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "Base Feature".into(),
                parameters: std::collections::BTreeMap::default(),
            }));
    }
}

pub(crate) fn losses(
    features: &[Feature],
    scopes: &[DesignParameterScope],
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    let mut losses = Vec::new();
    for scope in scopes {
        let feature = features
            .iter()
            .find(|feature| feature.native_ref.as_deref() == Some(scope.id.as_str()));
        match scope.kind_name() {
            "Base Feature" => {
                let selection = feature.and_then(|feature| match feature.evaluation.definition() {
                    FeatureDefinition::Operation(FeatureOperation::BaseFeature { bodies }) => {
                        Some(bodies)
                    }
                    _ => None,
                });
                if let Some(BodySelection::Resolved { bodies, .. }) = selection {
                    if bodies.is_empty() {
                        losses.push(F3dLossCode::FeatureStoredBodiesEmpty.note(format!(
                            "{} has an empty stored-body set and is a no-op.",
                            scope.id
                        )));
                    } else {
                        losses.push(F3dLossCode::FeatureStoredBodiesRecipeAbsent.note(format!(
                            "{} supplies stored bodies without a construction recipe.",
                            scope.id
                        )));
                    }
                } else {
                    losses.push(F3dLossCode::FeatureStoredBodiesUnbound.note(format!(
                        "{} has no complete unique stored-body binding.",
                        scope.id
                    )));
                }
            }
            "Derived Base Feature" | "DerivedBaseFeature" => {
                let stored = feature.and_then(|feature| match feature.evaluation.definition() {
                    FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                        bodies: BodySelection::Resolved { bodies, .. },
                    }) => Some(bodies.is_empty()),
                    _ => None,
                });
                match stored {
                    Some(true) => losses.push(F3dLossCode::FeatureStoredBodiesEmpty.note(format!(
                        "{} has an empty stored-body set and is a no-op.",
                        scope.id
                    ))),
                    Some(false) => losses.push(F3dLossCode::FeatureDerivedBaseUnresolved.note(format!(
                        "{} derives its bodies from a source and frame that the archive does not contain; the result bodies are retained as a stored snapshot.",
                        scope.id
                    ))),
                    None => losses.push(F3dLossCode::FeatureDerivedBaseUnresolved.note(format!(
                        "{} retains its native derived source and frame.",
                        scope.id
                    ))),
                }
            }
            "BoxPrimitive" if scope.payload().is_uninterpreted() => {
                losses.push(F3dLossCode::FeatureBoxPrimitiveUnresolved.note(format!(
                    "{} has no fully decoded box dimensions and local frame.",
                    scope.id
                )));
            }
            _ => {}
        }
    }
    losses
}
