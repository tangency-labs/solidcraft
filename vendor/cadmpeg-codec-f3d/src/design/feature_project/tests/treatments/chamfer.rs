// SPDX-License-Identifier: Apache-2.0
//! Chamfer projection: the edge groups the source states and the parameter
//! lanes they pair with.
use super::{localized_fillet_group, localized_fillet_owner, localized_fillet_parameter};
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::features::FeatureOperation;

fn localized_chamfer_scope() -> DesignParameterScope {
    DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            id: "f3d:native/BulkStream.dat:scope#12".into(),
            byte_offset: 100,
            class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned())
                .unwrap(),
            record_index: 12,
            frame_length: 200,
            kind_offset: 210,
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
            history_state_id: None,
            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: 180,
            reference_members: crate::records::identity::ReferenceRun::from_columns(
                vec![100, 101],
                vec![185, 196],
                "reference_members",
            )
            .unwrap(),
            payload: crate::records::feature::scope::DesignFeatureKind::Chamfer
                .try_into()
                .unwrap(),
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "261".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 300,
        }
        .with_fixture_layout(),
    )
    .unwrap()
}
#[test]
fn a_chamfer_that_states_no_edge_group_refuses_a_one_element_distance_lane() {
    use cadmpeg_ir::features::{
        edge_treatments::{ChamferGroup, ChamferSpec},
        EdgeSelection,
    };

    let scope = localized_chamfer_scope();
    let parameters = [localized_fillet_parameter(
        10,
        11,
        "Distance",
        Some("mm"),
        0.1,
    )];
    let owners = [localized_fillet_owner(10, 11, 0)];

    let crate::design::feature_project::ProjectedParameterDesign {
        features: refused, ..
    } = crate::test_support::with_decode_context(|ctx| {
        let scopes = std::slice::from_ref(&scope);
        let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            ctx,
            &crate::design::feature_project::ProjectInputs {
                native: &parameters,
                owners: &owners,
                scopes,
                timelines: &timelines,
                ..Default::default()
            },
        )
        .expect("test projection has a synthetic exact timeline")
    });
    assert!(matches!(
        refused[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::Chamfer,
            parameters,
        }) if parameters.len() == 1
    ));

    let group = localized_fillet_group(100, 0, vec![200]);
    let crate::design::feature_project::ProjectedParameterDesign {
        features: accepted, ..
    } = crate::test_support::with_decode_context(|ctx| {
        let scopes = std::slice::from_ref(&scope);
        let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            ctx,
            &crate::design::feature_project::ProjectInputs {
                native: &parameters,
                owners: &owners,
                scopes,
                construction_groups: std::slice::from_ref(&group),
                timelines: &timelines,
                ..Default::default()
            },
        )
        .expect("test projection has a synthetic exact timeline")
    });
    assert!(matches!(
        accepted[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. })
            if matches!(groups.as_slice(), [ChamferGroup {
                edges: EdgeSelection::Native(selection),
                spec: ChamferSpec::Distance { distance },
            }] if selection == &group.id && distance.get() == 1.0)
    ));
}
