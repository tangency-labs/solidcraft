// SPDX-License-Identifier: Apache-2.0
//! Independent lineage selectors for compact edge-treatment scopes.
use super::{entity_in_brep_source, historical_brep_source, history_state_pair};
use crate::history_records::AsmHistory;
use crate::ids::same_native_occurrence;
use crate::records::{
    feature::scope::DesignParameterScope,
    topology::{
        body_recipe::DesignBodyRecipeOperand,
        construction::{DesignConstructionOperandGroup, DesignConstructionOperandIdentity},
        edge_identity::DesignEdgeIdentityOperand,
    },
};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy)]
pub(crate) struct Inputs<'a> {
    pub(crate) scopes: &'a [DesignParameterScope],
    pub(crate) groups: &'a [DesignConstructionOperandGroup],
    pub(crate) identities: &'a [DesignConstructionOperandIdentity],
    pub(crate) operands: &'a [DesignEdgeIdentityOperand],
    pub(crate) recipes: &'a [DesignBodyRecipeOperand],
    pub(crate) owners: &'a HashMap<String, Vec<String>>,
    pub(crate) histories: &'a [AsmHistory],
}

fn exact_candidates<'a>(
    scope: &DesignParameterScope,
    histories: &'a [AsmHistory],
) -> Vec<&'a AsmHistory> {
    let (Some(current), Some(previous)) =
        (scope.history_state_id(), scope.previous_history_state_id())
    else {
        return Vec::new();
    };
    histories
        .iter()
        .filter(|history| history_state_pair(history, current, previous, true).is_some())
        .collect()
}

fn consensus<'a>(selectors: impl IntoIterator<Item = HashSet<&'a str>>) -> Option<&'a str> {
    let mut agreed = None;
    for candidates in selectors {
        if candidates.len() != 1 {
            continue;
        }
        let selected = candidates.into_iter().next()?;
        if agreed.is_some_and(|prior| prior != selected) {
            return None;
        }
        agreed = Some(selected);
    }
    agreed
}

pub(crate) fn bind(inputs: Inputs<'_>, bindings: &mut HashMap<String, String>) -> HashSet<String> {
    let mut lineage_scopes = HashSet::new();
    for scope in inputs.scopes.iter().filter(|scope| {
        matches!(
            crate::design::design_feature_family(&scope.kind()),
            Some(
                crate::design::DesignFeatureFamily::Fillet
                    | crate::design::DesignFeatureFamily::Chamfer
            )
        )
    }) {
        let mut groups = inputs.groups.iter().filter(|group| {
            same_native_occurrence(&group.id, &scope.id)
                && group.scope_record_index == scope.record_index
        });
        let Some(group) = groups.next().filter(|_| groups.next().is_none()) else {
            continue;
        };
        let [member] = group.members() else {
            continue;
        };
        let mut operands = inputs.operands.iter().filter(|operand| {
            same_native_occurrence(&operand.id, &scope.id)
                && operand.scope_record_index == scope.record_index
                && operand.group_record_index == group.record_index
                && operand.record_index() == member.value
                && operand.layout().is_compact()
        });
        let Some(operand) = operands.next().filter(|_| operands.next().is_none()) else {
            continue;
        };
        let candidates = exact_candidates(scope, inputs.histories);
        if candidates.len() < 2 {
            continue;
        }
        let recipe = candidates
            .iter()
            .filter(|history| {
                historical_brep_source(&history.id).is_some_and(|source| {
                    inputs
                        .recipes
                        .iter()
                        .filter(|recipe| {
                            same_native_occurrence(&recipe.id, &operand.id)
                                && (recipe.asset_id.as_str(), recipe.context_id.as_str())
                                    == operand.lineage_key()
                        })
                        .flat_map(DesignBodyRecipeOperand::references)
                        .flat_map(|reference| &reference.candidate_faces)
                        .any(|face| entity_in_brep_source(face.as_str(), source))
                })
            })
            .map(|history| history.id.as_str())
            .collect();
        let owner = candidates
            .iter()
            .filter(|history| {
                historical_brep_source(&history.id).is_some_and(|source| {
                    inputs
                        .owners
                        .get(&group.id)
                        .is_some_and(|sources| sources.iter().any(|candidate| candidate == source))
                })
            })
            .map(|history| history.id.as_str())
            .collect();
        let mut prior = HashSet::new();
        for identity in inputs.identities.iter().filter(|identity| {
            same_native_occurrence(&identity.id, &operand.id)
                && identity.persistent_identity().is_some_and(|persistent| {
                    (persistent.asset_id.as_str(), persistent.context_id.as_str())
                        == operand.lineage_key()
                })
        }) {
            for linked_group in inputs.groups.iter().filter(|linked| {
                same_native_occurrence(&linked.id, &identity.id)
                    && linked.record_index == identity.group_record_index
            }) {
                let mut linked_scopes = inputs.scopes.iter().filter(|linked| {
                    same_native_occurrence(&linked.id, &linked_group.id)
                        && linked.record_index == linked_group.scope_record_index
                });
                let Some(linked_scope) = linked_scopes
                    .next()
                    .filter(|_| linked_scopes.next().is_none())
                else {
                    continue;
                };
                if let [history] = exact_candidates(linked_scope, inputs.histories).as_slice() {
                    if candidates
                        .iter()
                        .any(|candidate| candidate.id == history.id)
                    {
                        prior.insert(history.id.as_str());
                    }
                }
            }
        }
        let has_selector = [&owner, &recipe, &prior]
            .iter()
            .any(|candidates| candidates.len() == 1);
        if has_selector {
            bindings.remove(&scope.id);
            if let Some(history) = consensus([owner, recipe, prior]) {
                bindings.insert(scope.id.clone(), history.to_owned());
                lineage_scopes.insert(scope.id.clone());
            }
        }
    }
    lineage_scopes
}

#[cfg(test)]
mod tests {
    use super::consensus;
    use std::collections::HashSet;

    #[test]
    fn compact_lineage_requires_at_least_one_singleton_selector() {
        assert_eq!(
            consensus([HashSet::new(), HashSet::from(["body-a", "body-b"])]),
            None
        );
        assert_eq!(
            consensus([
                HashSet::from(["body-a"]),
                HashSet::new(),
                HashSet::from(["body-a", "body-b"])
            ]),
            Some("body-a")
        );
    }

    #[test]
    fn compact_lineage_rejects_disagreement_even_with_a_majority() {
        for selectors in [
            [
                HashSet::from(["body-a"]),
                HashSet::from(["body-b"]),
                HashSet::from(["body-a"]),
            ],
            [
                HashSet::from(["body-b"]),
                HashSet::from(["body-a"]),
                HashSet::from(["body-a"]),
            ],
        ] {
            assert_eq!(consensus(selectors), None);
        }
        assert_eq!(
            consensus([HashSet::from(["body-b"]), HashSet::from(["body-b"])]),
            Some("body-b")
        );
    }
}
