// SPDX-License-Identifier: Apache-2.0

use super::{history_in_source, native_base_feature};
use crate::history::bind_feature_outputs;
use crate::loss::F3dLossCode;
use cadmpeg_ir::features::{BodySelection, DistinctMembers, FeatureDefinition, FeatureOperation};

#[test]
fn feature_outputs_keep_complete_stored_body_selection() {
    for bound in [false, true] {
        let (mut feature, scope, _, body) = native_base_feature();
        if bound {
            feature
                .evaluation
                .set_definition(FeatureDefinition::Operation(
                    FeatureOperation::BaseFeature {
                        bodies: BodySelection::Resolved {
                            bodies: DistinctMembers::try_from(vec![body.id.clone()]).unwrap(),
                            native: scope.id.clone(),
                        },
                    },
                ));
        }
        let losses = bind_feature_outputs(
            std::slice::from_mut(&mut feature),
            std::slice::from_ref(&scope),
            &mut [history_in_source("selected.smbh", &[1, 2])],
            std::slice::from_ref(&body),
            &Default::default(),
            Some("BREP.selected.smbh"),
        )
        .unwrap();
        assert_eq!(feature.evaluation.outputs().as_slice(), [body.id]);
        assert_eq!(
            losses
                .iter()
                .filter(|loss| loss.code == F3dLossCode::FeatureOutputBodyUnbound.kind())
                .count(),
            usize::from(!bound)
        );
        let stored = crate::history::stored_bodies::losses(&[feature], &[scope]);
        assert_eq!(stored.len(), 1);
        assert_eq!(
            stored[0].code,
            if bound {
                F3dLossCode::FeatureStoredBodiesRecipeAbsent.kind()
            } else {
                F3dLossCode::FeatureStoredBodiesUnbound.kind()
            }
        );
    }
}

#[test]
fn feature_outputs_keep_the_copies_of_a_base_feature_copy() {
    use crate::records::feature::scope::DesignFeatureKind;
    for kind in [DesignFeatureKind::BaseFeature, DesignFeatureKind::Move] {
        let (mut feature, base_scope, history, body) = native_base_feature();
        let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
            &base_scope.id,
            kind.clone(),
            1,
        );
        scope
            .try_edit(|draft| {
                draft.history_state_id = base_scope.history_state_id();
                draft.previous_history_state_id = base_scope.previous_history_state_id();
                draft.layout_fixture_tail();
            })
            .unwrap();
        let copy = cadmpeg_ir::ids::BodyId::mint("test:model:body#9").unwrap();
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(FeatureOperation::MoveBody {
                bodies: BodySelection::Resolved {
                    bodies: DistinctMembers::try_from(vec![body.id.clone()]).unwrap(),
                    native: scope.id.clone(),
                },
                translation: cadmpeg_ir::features::FiniteVector3::new(
                    cadmpeg_ir::math::Vector3::new(0.0, 0.0, 0.0),
                )
                .unwrap(),
                rotation: None,
                copies: 1,
            }));
        feature
            .evaluation
            .set_outputs(DistinctMembers::try_from(vec![copy.clone()]).unwrap());
        bind_feature_outputs(
            std::slice::from_mut(&mut feature),
            std::slice::from_ref(&scope),
            &mut [history],
            std::slice::from_ref(&body),
            &Default::default(),
            None,
        )
        .unwrap();
        let expected = if kind == DesignFeatureKind::BaseFeature {
            copy
        } else {
            body.id
        };
        assert_eq!(
            feature.evaluation.outputs().as_slice(),
            [expected],
            "{kind:?}"
        );
    }
}
