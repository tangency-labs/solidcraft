// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::decode::parameters::parse_parameter_owner;
use crate::design::test_support::parameter_owner_frame;
use crate::design::test_support::parameter_record;
use crate::records::entity_header::DesignFeatureTimeline;
use crate::records::entity_header::DesignTimelineFrame;
use crate::records::feature::fixed_parameters::DesignFixedFilletGroup;
use crate::records::feature::fixed_parameters::DesignFixedFilletLaw;
use crate::records::feature::fixed_parameters::DesignFixedFilletParameters;
use crate::records::feature::fixed_parameters::DesignFixedFilletScalar;
use crate::records::feature::scope::DesignFeatureKind;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::scope::DesignParameterScopeDraft;
use crate::records::feature::scope::DesignScopePayload;
use crate::records::feature::scope::DesignScopePayloadMut;
use crate::records::feature::surface_ops::DesignPatchContinuity;
use crate::records::feature::surface_ops::DesignSurfacePatchBoundary;
use crate::records::identity::Located;
use crate::records::identity::ReferenceRun;
use crate::records::mesh::DesignRelaxedGuidText;
use crate::records::parameters::DesignParameter;
use crate::records::parameters::DesignParameterOwner;
use crate::records::parameters::DesignParameterOwnerWire;
use crate::records::references::DesignClassTag;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::construction::DesignConstructionOperandGroupDraft;
use crate::records::topology::construction::DesignConstructionOperandGroupFrame;
use crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft;
use crate::records::topology::construction::DesignConstructionOperandRole;
use crate::records::topology::edge_identity::DesignEdgeIdentityLayout;
use crate::records::topology::edge_identity::DesignEdgeIdentityOperand;
use crate::records::topology::edge_identity::DesignEdgeIdentityOperandDraft;
use crate::records::topology::entity_selection::DesignEntitySelectionOperandDraft;
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::records::topology::fillet::DesignFilletMidpoint;
use crate::records::topology::fillet::DesignFilletRadiusLaw;
use crate::records::topology::fillet::HistoricalBinding;
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::features::FeatureOperation;

fn decode_fillet_radius_groups(
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    owners: &[DesignParameterOwner],
    parameters: &[DesignParameter],
) -> Vec<crate::records::topology::fillet::DesignFilletRadiusGroup> {
    crate::design::decode::operands::decode_fillet_radius_groups(scopes, groups, owners, parameters)
}

#[test]
fn draft_entity_neutral_selection_projects_a_unique_historical_face() {
    use crate::records::{
        feature::{direct_face::DesignDraftOperation, scope::DesignParameterScope},
        topology::{
            body_recipe::AsmHistoricalEntityKind, construction::DesignConstructionOperandGroup,
            construction::DesignConstructionOperandGroupFrame,
            entity_selection::DesignEntitySelectionFaceCandidate,
            entity_selection::DesignEntitySelectionOperand,
        },
    };
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};

    let stream = "f3d:test/Design/BulkStream.dat";
    let mut scope = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#100"),
        DesignFeatureKind::Draft,
        100,
    );
    scope.feature_ordinal = std::num::NonZeroU32::new(1).expect("nonzero ordinal");
    scope
        .try_edit(|draft| {
            draft.previous_history_state_id = Some(7);
            draft.reference_members = ReferenceRun::unlocated(vec![101, 111, 102, 112]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let DesignScopePayloadMut::Draft(slot) = scope.payload_mut() {
        *slot = Some(DesignDraftOperation {
            angle: cadmpeg_ir::scalar::Angle::new(-0.25).unwrap(),
            angle_record_index: 90,
            angle_offset: 0,
            opposite_angle_record_index: 91,
            opposite_angle_offset: 0,
            flags: None,
        });
    }
    let group = |record_index, member, role| {
        DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
            id: format!("{stream}:design-construction-operand-group#{record_index}"),
            scope_record_index: 100,
            scope_reference_ordinal: 0,
            record_index,
            byte_offset: 0,
            class_tag: DesignClassTag::try_from("000".to_owned()).unwrap(),
            members: vec![Located {
                value: member,
                offset: 0,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 0,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 18,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 22,
                    variant: false,
                    closing_flag: false,
                },
            )
            .unwrap(),
            operand_role: DesignConstructionOperandRole::Other(role),
            role_offset: 0,
            paired_class_tag: DesignClassTag::try_from("000".to_owned()).unwrap(),
            paired_byte_offset: 0,
        })
        .unwrap()
    };
    let groups = [
        group(101, 111, DesignOperandRole::ROLE_0X10),
        group(102, 112, DesignOperandRole::ROLE_0X21),
    ];
    let mut selection = DesignEntitySelectionOperand::try_new(DesignEntitySelectionOperandDraft {
        id: format!("{stream}:design-entity-selection-operand#112"),
        scope_record_index: 100,
        group_record_index: 102,
        group_member_ordinal: 0,
        record_index: 112,
        byte_offset: 0,
        class_tag: DesignClassTag::try_from("000".to_owned()).unwrap(),
        asset_id: DesignRelaxedGuidText::try_from(
            "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
        )
        .unwrap(),
        asset_id_offset: 0,
        context_id: DesignRelaxedGuidText::try_from(
            "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
        )
        .unwrap(),
        context_id_offset: 0,
        identity_record_index: 115,
        identity_record_offset: 0,
        primary_identity: 225,
        primary_identity_offset: 21,
        secondary: None,
        historical_edge_candidates: Vec::new(),
        historical_face_candidates: vec![DesignEntitySelectionFaceCandidate {
            history_id: "history".into(),
            historical: HistoricalBinding {
                kind: AsmHistoricalEntityKind::Coedge,
                entity_ref: 225,
                state_ids: vec![7, 6],
            },
            face_slot: 158,
        }],
        resolved_edge_slot: None,
        next_record_index: 114,
        next_byte_offset: 29,
    })
    .unwrap();
    let timeline = DesignFeatureTimeline::try_new(
        crate::ids::native_design_feature_timeline_id_in_stream(stream, 0),
        DesignTimelineFrame::test_items(
            0,
            vec![Located {
                value: 100,
                offset: 0,
            }],
        ),
        DesignClassTag::try_from("256".to_owned()).unwrap(),
        std::num::NonZeroU64::new(1).unwrap(),
        0,
        std::num::NonZeroU64::new(1).unwrap(),
    )
    .unwrap();
    let project = |selection: &DesignEntitySelectionOperand| {
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                decode_ctx,
                &crate::design::feature_project::ProjectInputs {
                    scopes: std::slice::from_ref(&scope),
                    timelines: std::slice::from_ref(&timeline),
                    construction_groups: &groups,
                    entity_selection_operands: std::slice::from_ref(selection),
                    ..Default::default()
                },
            )
        })
        .expect("authored Draft timeline")
    };

    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        project(&selection);
    let FeatureDefinition::Operation(FeatureOperation::Draft {
        anchor:
            cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                plane: neutral_plane,
                pull,
            },
        angle,
        outward,
        ..
    }) = features[0].evaluation.definition()
    else {
        panic!("expected a typed Draft definition");
    };
    assert_eq!(angle.as_ref().map(|angle| angle.get()), Some(-0.25));
    assert_eq!(*outward, Some(true));
    assert!(pull.is_none());
    let FaceSelection::Historical {
        state,
        faces,
        native,
    } = neutral_plane
    else {
        panic!("expected a historical neutral face selection");
    };
    let feature = crate::ids::neutral_feature_id(&scope);
    let feature_key = feature.key();
    let prefix = crate::ids::history_input_prefix(&feature_key, 7);
    assert_eq!(state, &crate::ids::feature_input_topology_id(&feature, 7));
    assert_eq!(
        faces.as_slice(),
        &[crate::ids::history_input_face_id(&prefix, 158)]
    );
    assert_eq!(native.as_str(), &groups[1].id);

    selection
        .historical_face_candidates
        .push(DesignEntitySelectionFaceCandidate {
            history_id: "other-history".into(),
            historical: HistoricalBinding {
                kind: AsmHistoricalEntityKind::Coedge,
                entity_ref: 225,
                state_ids: vec![7],
            },
            face_slot: 159,
        });
    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        project(&selection);
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::Draft,
            ..
        })
    ));
}

#[test]
fn draft_outward_is_derived_from_the_signed_angle() {
    assert!(crate::design::feature_project::draft_outward(-0.25));
    assert!(!crate::design::feature_project::draft_outward(0.0));
    assert!(!crate::design::feature_project::draft_outward(0.25));
}

#[test]
fn variable_fillet_law_orders_endpoint_and_midpoint_parameters() {
    use cadmpeg_ir::scalar::Length;

    let parameter = |record_index, source_kind: &str, unit, value| {
        let mut parameter = parse_design_parameter_record(&parameter_record(
            Some(record_index + 100),
            "value",
            source_kind,
            unit,
            "d1",
            value,
        ))
        .expect("variable Fillet parameter");
        parameter.record_index = record_index;
        parameter
    };
    let start = parameter(1, "StartRadius", Some("mm"), 0.0);
    let end = parameter(2, "EndRadius", Some("mm"), 0.0);
    let radius = parameter(3, "MidRadius", Some("mm"), 0.4);
    let position = parameter(4, "MidParams", None, 0.25);
    let weight = parameter(5, "TangencyWeight", None, 0.75);
    let (points, tangency_weight) = crate::design::feature_project::variable_fillet_law(&[
        (0, &start),
        (1, &end),
        (2, &radius),
        (3, &position),
        (4, &weight),
    ])
    .expect("complete variable Fillet law");
    assert_eq!(
        points
            .as_slice()
            .iter()
            .map(cadmpeg_ir::features::edge_treatments::VariableRadius::to_raw)
            .collect::<Vec<_>>(),
        [
            cadmpeg_ir::features::edge_treatments::VariableRadius {
                parameter: 0.0,
                radius: Length::ZERO,
            },
            cadmpeg_ir::features::edge_treatments::VariableRadius {
                parameter: 0.25,
                radius: Length::new(4.0).unwrap(),
            },
            cadmpeg_ir::features::edge_treatments::VariableRadius {
                parameter: 1.0,
                radius: Length::ZERO,
            },
        ]
    );
    assert_eq!(
        tangency_weight.map(cadmpeg_ir::scalar::FiniteReal::get),
        Some(0.75)
    );
}

#[test]
fn variable_fillet_law_accepts_omitted_tangency_weight() {
    use cadmpeg_ir::scalar::Length;

    let parameter = |record_index, source_kind: &str, unit, value| {
        let mut parameter = parse_design_parameter_record(&parameter_record(
            Some(record_index + 100),
            "value",
            source_kind,
            unit,
            "d1",
            value,
        ))
        .expect("variable Fillet parameter");
        parameter.record_index = record_index;
        parameter
    };
    let start = parameter(1, "StartRadius", Some("mm"), 0.2);
    let end = parameter(2, "EndRadius", Some("mm"), 0.4);
    let (points, tangency_weight) =
        crate::design::feature_project::variable_fillet_law(&[(0, &start), (1, &end)])
            .expect("variable Fillet law without an explicit weight");
    assert_eq!(
        points
            .as_slice()
            .iter()
            .map(cadmpeg_ir::features::edge_treatments::VariableRadius::to_raw)
            .collect::<Vec<_>>(),
        [
            cadmpeg_ir::features::edge_treatments::VariableRadius {
                parameter: 0.0,
                radius: Length::new(2.0).unwrap(),
            },
            cadmpeg_ir::features::edge_treatments::VariableRadius {
                parameter: 1.0,
                radius: Length::new(4.0).unwrap(),
            },
        ]
    );
    assert_eq!(tangency_weight, None);
}

#[test]
fn variable_fillet_law_rejects_duplicate_tangency_weights() {
    let parameter = |record_index, source_kind: &str, unit, value| {
        let mut parameter = parse_design_parameter_record(&parameter_record(
            Some(record_index + 100),
            "value",
            source_kind,
            unit,
            "d1",
            value,
        ))
        .expect("variable Fillet parameter");
        parameter.record_index = record_index;
        parameter
    };
    let start = parameter(1, "StartRadius", Some("mm"), 0.2);
    let end = parameter(2, "EndRadius", Some("mm"), 0.4);
    let weight_one = parameter(3, "TangencyWeight", None, 0.5);
    let weight_two = parameter(4, "TangencyWeight", None, 0.75);
    assert!(crate::design::feature_project::variable_fillet_law(&[
        (0, &start),
        (1, &end),
        (2, &weight_one),
        (3, &weight_two)
    ])
    .is_none());
}

/// A localized Fillet scope whose reference table names its two operand
/// groups and the parameter owners `10, 20, ..., 150` of these tests.
fn localized_fillet_scope() -> DesignParameterScope {
    let mut scope = localized_fillet_scope_without_owners();
    crate::design::test_support::name_scope_references(&mut scope, (10..=150).step_by(10));
    scope
}

fn localized_fillet_scope_without_owners() -> DesignParameterScope {
    DesignParameterScope::try_new(
        DesignParameterScopeDraft {
            id: "f3d:native/BulkStream.dat:scope#12".into(),
            byte_offset: 100,
            class_tag: DesignClassTag::try_from("301".to_owned()).unwrap(),
            record_index: 12,
            frame_length: 200,
            kind_offset: 210,
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
            history_state_id: None,

            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: 180,
            reference_members: ReferenceRun::from_columns(
                vec![100, 101],
                vec![185, 196],
                "reference_members",
            )
            .unwrap(),
            payload: DesignFeatureKind::Conge.try_into().unwrap(),
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: DesignClassTag::try_from("261".to_owned()).unwrap(),
            paired_byte_offset: 300,
        }
        .with_fixture_layout(),
    )
    .unwrap()
}

fn localized_fillet_group(
    record_index: u32,
    ordinal: u32,
    members: Vec<u32>,
) -> DesignConstructionOperandGroup {
    localized_fillet_group_with_role(record_index, ordinal, members, DesignOperandRole::BODIES_B)
}

fn localized_fillet_group_with_role(
    record_index: u32,
    ordinal: u32,
    members: Vec<u32>,
    role: DesignOperandRole,
) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: format!("f3d:native/BulkStream.dat:construction-group#{record_index}"),
        scope_record_index: 12,
        scope_reference_ordinal: ordinal,
        record_index,
        byte_offset: 1000 + u64::from(ordinal) * 200,
        class_tag: DesignClassTag::try_from("288".to_owned()).unwrap(),

        members: members
            .into_iter()
            .enumerate()
            .map(|(index, value)| Located {
                value,
                offset: 1026 + u64::from(ordinal) * 200 + u64_from_index(index) * 11,
            })
            .collect(),
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame::try_from(
            DesignConstructionOperandGroupFrameDraft {
                member_count_offset: 1021 + u64::from(ordinal) * 200,
                auxiliary_records: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_records: vec![Located {
                    value: 300 + ordinal,
                    offset: 1100 + u64::from(ordinal) * 200,
                }],
                trailing_transforms: Vec::new(),
                trailing_dual_transforms: Vec::new(),
                trailing_flags: Vec::new(),
                opaque_index: 100,
                opaque_index_offset: 1128 + u64::from(ordinal) * 200,
                opaque_scalar: 0.5,
                opaque_scalar_offset: 1132 + u64::from(ordinal) * 200,
                variant: false,
                closing_flag: false,
            },
        )
        .unwrap(),
        operand_role: DesignConstructionOperandRole::Other(role),
        role_offset: 1110 + u64::from(ordinal) * 200,

        paired_class_tag: DesignClassTag::try_from("259".to_owned()).unwrap(),
        paired_byte_offset: 1200 + u64::from(ordinal) * 200,
    })
    .unwrap()
}

fn localized_fillet_operand_groups() -> [DesignConstructionOperandGroup; 2] {
    [
        localized_fillet_group(100, 0, vec![200]),
        localized_fillet_group(101, 1, vec![201, 202]),
    ]
}

fn localized_fillet_parameter(
    owner_index: u32,
    record_index: u32,
    source_kind: &str,
    unit: Option<&str>,
    value: f64,
) -> DesignParameter {
    let mut parameter = parse_design_parameter_record(&parameter_record(
        Some(owner_index),
        "value",
        source_kind,
        unit,
        "d1",
        value,
    ))
    .expect("canonical localized Fillet parameter");
    parameter.id = format!("f3d:native/BulkStream.dat:parameter#{record_index}");
    parameter.record_index = record_index;
    parameter
}

fn localized_fillet_owner(
    record_index: u32,
    parameter_record_index: u32,
    local_ordinal: u32,
) -> DesignParameterOwner {
    let mut owner = parse_parameter_owner(&parameter_owner_frame())
        .unwrap()
        .into_record("Design/BulkStream.dat", 0)
        .unwrap();
    {
        let mut wire = DesignParameterOwnerWire::from(owner.clone());
        wire.id = format!("f3d:native/BulkStream.dat:owner#{record_index}");
        wire.record_index = record_index;
        wire.scope_record_index = 12;
        wire.parameter_record_index = parameter_record_index;
        wire.companion_record_index = parameter_record_index + 1;
        wire.local_ordinal = local_ordinal;
        owner = DesignParameterOwner::try_from(wire).unwrap();
    }
    owner
}

#[test]
fn fillet_edge_and_face_groups_share_the_scope_radius() {
    let scope = localized_fillet_scope();
    let operand_groups = [
        localized_fillet_group(100, 0, vec![200, 201]),
        localized_fillet_group_with_role(101, 1, vec![202], DesignOperandRole::ROLE_0X10),
    ];
    let parameters = [
        localized_fillet_parameter(10, 11, "TangencyWeight", None, 1.0),
        localized_fillet_parameter(20, 21, "Radius", Some("mm"), 0.3),
    ];
    let owners = [
        localized_fillet_owner(10, 11, 0),
        localized_fillet_owner(20, 21, 1),
    ];
    let assignments = decode_fillet_radius_groups(
        std::slice::from_ref(&scope),
        &operand_groups,
        &owners,
        &parameters,
    );
    let shared_law = DesignFilletRadiusLaw::Constant {
        radius_parameter_record_index: 21,
    };
    assert_eq!(
        assignments
            .iter()
            .map(|assignment| (
                assignment.group_ordinal,
                assignment.group_record_index,
                &assignment.law,
                assignment.tangency_weight_parameter_record_index,
            ))
            .collect::<Vec<_>>(),
        [
            (0, 100, &shared_law, Some(11)),
            (1, 101, &shared_law, Some(11)),
        ]
    );
    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = std::slice::from_ref(&scope);
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    native: &parameters,
                    owners: &owners,
                    scopes,
                    construction_groups: &operand_groups,
                    fillet_radius_groups: &assignments,
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });
    let FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) =
        features[0].evaluation.definition()
    else {
        panic!("the shared radius projects a Fillet");
    };
    assert_eq!(groups.len(), 2);
    for group in groups {
        assert!(matches!(
            group.radius,
            cadmpeg_ir::features::edge_treatments::RadiusSpec::Constant { radius }
                if (radius.get() - 3.0).abs() < 1.0e-12
        ));
        assert_eq!(
            group
                .tangency_weight
                .map(cadmpeg_ir::scalar::FiniteReal::get),
            Some(1.0)
        );
    }
}

fn named_fillet_parameter(
    owner_index: u32,
    record_index: u32,
    source_kind: &str,
    unit: Option<&str>,
    value: f64,
) -> DesignParameter {
    let mut parameter = parse_design_parameter_record(&parameter_record(
        Some(owner_index),
        "value",
        source_kind,
        unit,
        &format!("d{record_index}"),
        value,
    ))
    .expect("canonical localized Fillet parameter");
    parameter.id = format!("f3d:native/BulkStream.dat:parameter#{record_index}");
    parameter.record_index = record_index;
    parameter
}

#[test]
fn fillet_edge_sets_pair_with_radii_and_leave_the_face_group_unassigned() {
    let scope = localized_fillet_scope();
    let operand_groups = [
        localized_fillet_group(100, 0, vec![200, 201]),
        localized_fillet_group_with_role(101, 1, vec![202], DesignOperandRole::ROLE_0X10),
        localized_fillet_group(102, 2, vec![203]),
    ];
    let parameters = [
        named_fillet_parameter(10, 11, "TangencyWeight", None, 1.0),
        named_fillet_parameter(20, 21, "Radius", Some("mm"), 0.3),
        named_fillet_parameter(30, 31, "TangencyWeight", None, 1.0),
        named_fillet_parameter(40, 41, "Radius", Some("mm"), 0.5),
    ];
    let owners = [
        localized_fillet_owner(10, 11, 0),
        localized_fillet_owner(20, 21, 1),
        localized_fillet_owner(30, 31, 2),
        localized_fillet_owner(40, 41, 3),
    ];
    let assignments = decode_fillet_radius_groups(
        std::slice::from_ref(&scope),
        &operand_groups,
        &owners,
        &parameters,
    );
    assert_eq!(
        assignments
            .iter()
            .map(|assignment| (
                assignment.group_ordinal,
                assignment.group_record_index,
                &assignment.law,
                assignment.tangency_weight_parameter_record_index,
            ))
            .collect::<Vec<_>>(),
        [
            (
                0,
                100,
                &DesignFilletRadiusLaw::Constant {
                    radius_parameter_record_index: 21,
                },
                Some(11)
            ),
            (
                1,
                102,
                &DesignFilletRadiusLaw::Constant {
                    radius_parameter_record_index: 41,
                },
                Some(31)
            ),
        ]
    );
    // The face group has no stated radius set, so the feature stays native.
    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = std::slice::from_ref(&scope);
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    native: &parameters,
                    owners: &owners,
                    scopes,
                    construction_groups: &operand_groups,
                    fillet_radius_groups: &assignments,
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native { .. })
    ));

    // A radius count that differs from the edge-set count assigns nothing.
    assert!(decode_fillet_radius_groups(
        std::slice::from_ref(&scope),
        &operand_groups,
        &owners[..2],
        &parameters[..2],
    )
    .is_empty());
    // A weight count that differs from the edge-set count assigns nothing.
    assert!(decode_fillet_radius_groups(
        std::slice::from_ref(&scope),
        &operand_groups,
        &owners[..3],
        &parameters[..3],
    )
    .is_empty());
}

#[test]
fn detached_fillet_owner_supplies_no_radius() {
    let scope = localized_fillet_scope();
    let operand_groups = [localized_fillet_group(100, 0, vec![200])];
    let parameters = [
        localized_fillet_parameter(20, 21, "Radius", Some("mm"), 0.3),
        localized_fillet_parameter(170, 171, "Radius", Some("mm"), 0.9),
    ];
    let owners = [
        localized_fillet_owner(20, 21, 1),
        localized_fillet_owner(170, 171, 1),
    ];
    let assignments = decode_fillet_radius_groups(
        std::slice::from_ref(&scope),
        &operand_groups,
        &owners,
        &parameters,
    );
    assert_eq!(
        assignments
            .iter()
            .map(|assignment| &assignment.law)
            .collect::<Vec<_>>(),
        [&DesignFilletRadiusLaw::Constant {
            radius_parameter_record_index: 21,
        }]
    );
}

#[test]
fn fillet_edge_group_without_its_face_group_keeps_one_to_one_pairing() {
    let scope = localized_fillet_scope();
    let operand_groups = [
        localized_fillet_group(100, 0, vec![200]),
        localized_fillet_group(101, 1, vec![201]),
    ];
    let parameters = [localized_fillet_parameter(
        20,
        21,
        "Radius",
        Some("mm"),
        0.3,
    )];
    let owners = [localized_fillet_owner(20, 21, 0)];
    assert!(decode_fillet_radius_groups(
        std::slice::from_ref(&scope),
        &operand_groups,
        &owners,
        &parameters,
    )
    .is_empty());
}

#[test]
fn localized_fillet_radius_parameters_pair_with_counted_edge_groups_in_order() {
    let scope = localized_fillet_scope();
    let mut operand_groups = localized_fillet_operand_groups();
    let group = localized_fillet_group;
    let parameter = localized_fillet_parameter;
    let owner = localized_fillet_owner;
    let parameters = [
        parameter(10, 11, "Radius", Some("mm"), 0.5),
        parameter(20, 21, "Radius", Some("mm"), 0.3),
        parameter(30, 31, "TangencyWeight", None, 1.0),
        parameter(40, 41, "TangencyWeight", None, 0.75),
    ];
    let owners = [
        owner(10, 11, 0),
        owner(20, 21, 1),
        owner(30, 31, 2),
        owner(40, 41, 3),
    ];
    let mut indexed_scope = scope.clone();
    if let DesignScopePayloadMut::Fillet(slot)
    | DesignScopePayloadMut::Conge(slot)
    | DesignScopePayloadMut::Abrundung(slot)
    | DesignScopePayloadMut::Arredondamento(slot)
    | DesignScopePayloadMut::Empalme(slot)
    | DesignScopePayloadMut::FilletJa(slot) = indexed_scope.payload_mut()
    {
        *slot = Some(DesignFixedFilletParameters {
            groups: vec![DesignFixedFilletGroup::try_new(
                Some(DesignFixedFilletScalar {
                    value: crate::test_support::real(1.0),
                    record_index: 10,
                    value_offset: 100,
                }),
                DesignFixedFilletLaw::Constant(DesignFixedFilletScalar {
                    value: crate::test_support::real(0.5),
                    record_index: 20,
                    value_offset: 200,
                }),
            )
            .unwrap()],
        });
    }
    crate::design::decode::operands::disambiguate_fixed_fillet_parameters(
        std::slice::from_mut(&mut indexed_scope),
        &owners,
    );
    assert_eq!(indexed_scope.fixed_fillet_parameters(), None);

    let assignments = decode_fillet_radius_groups(
        std::slice::from_ref(&scope),
        &operand_groups,
        &owners,
        &parameters,
    );
    assert_eq!(assignments.len(), 2);
    assert_eq!(assignments[0].edge_operand_record_indices, [200]);
    assert_eq!(
        assignments[0].law,
        DesignFilletRadiusLaw::Constant {
            radius_parameter_record_index: 11,
        }
    );
    assert_eq!(
        assignments[0].tangency_weight_parameter_record_index,
        Some(31)
    );
    assert_eq!(assignments[1].edge_operand_record_indices, [201, 202]);
    assert_eq!(
        assignments[1].law,
        DesignFilletRadiusLaw::Constant {
            radius_parameter_record_index: 21,
        }
    );
    assert_eq!(
        assignments[1].tangency_weight_parameter_record_index,
        Some(41)
    );
    let variable_parameters = [
        parameter(50, 51, "StartRadius", Some("mm"), 0.2),
        parameter(60, 61, "EndRadius", Some("mm"), 0.6),
        parameter(70, 71, "MidRadius", Some("mm"), 0.4),
        parameter(80, 81, "MidParams", None, 0.25),
        parameter(90, 91, "TangencyWeight", None, 0.75),
    ];
    let variable_owners = [
        owner(50, 51, 0),
        owner(60, 61, 1),
        owner(70, 71, 2),
        owner(80, 81, 3),
        owner(90, 91, 4),
    ];
    let variable_assignments = decode_fillet_radius_groups(
        std::slice::from_ref(&scope),
        &operand_groups[..1],
        &variable_owners,
        &variable_parameters,
    );
    assert_eq!(variable_assignments.len(), 1);
    assert_eq!(
        variable_assignments[0].law,
        DesignFilletRadiusLaw::Variable {
            start_radius_parameter_record_index: 51,
            end_radius_parameter_record_index: 61,
            middle: vec![DesignFilletMidpoint {
                radius_parameter_record_index: 71,
                parameter_record_index: 81
            }],
        }
    );
    assert_eq!(
        variable_assignments[0].tangency_weight_parameter_record_index,
        Some(91)
    );
    let variable_without_weight_parameters = [
        parameter(50, 51, "StartRadius", Some("mm"), 0.2),
        parameter(60, 61, "EndRadius", Some("mm"), 0.6),
        parameter(70, 71, "MidRadius", Some("mm"), 0.4),
        parameter(80, 81, "MidParams", None, 0.25),
    ];
    let variable_without_weight_owners = [
        owner(50, 51, 0),
        owner(60, 61, 1),
        owner(70, 71, 2),
        owner(80, 81, 3),
    ];
    let variable_without_weight_assignments = decode_fillet_radius_groups(
        std::slice::from_ref(&scope),
        &operand_groups[..1],
        &variable_without_weight_owners,
        &variable_without_weight_parameters,
    );
    assert_eq!(variable_without_weight_assignments.len(), 1);
    assert_eq!(
        variable_without_weight_assignments[0].tangency_weight_parameter_record_index,
        None
    );
    let mut incomplete_parameters = variable_parameters.to_vec();
    incomplete_parameters.push(parameter(100, 101, "UnknownLawInput", None, 1.0));
    let mut incomplete_owners = variable_owners.to_vec();
    incomplete_owners.push(owner(100, 101, 5));
    assert!(decode_fillet_radius_groups(
        std::slice::from_ref(&scope),
        &operand_groups[..1],
        &incomplete_owners,
        &incomplete_parameters,
    )
    .is_empty());
    let chord_parameters = [
        parameter(110, 111, "TangencyWeight", None, 1.0),
        parameter(120, 121, "ChordLen", Some("in"), 0.25),
    ];
    let chord_owners = [owner(110, 111, 0), owner(120, 121, 1)];
    let chord_assignments = decode_fillet_radius_groups(
        std::slice::from_ref(&scope),
        &operand_groups[..1],
        &chord_owners,
        &chord_parameters,
    );
    assert_eq!(chord_assignments.len(), 1);
    assert_eq!(
        chord_assignments[0].law,
        DesignFilletRadiusLaw::Chordal {
            chord_length_parameter_record_index: 121,
        }
    );
    let crate::design::feature_project::ProjectedParameterDesign {
        features: chord_features,
        ..
    } = crate::test_support::with_decode_context(|ctx| {
        let scopes = std::slice::from_ref(&scope);
        let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            ctx,
            &crate::design::feature_project::ProjectInputs {
                native: &chord_parameters,
                owners: &chord_owners,
                scopes,
                construction_groups: &operand_groups[..1],
                fillet_radius_groups: &chord_assignments,
                timelines: &timelines,
                ..Default::default()
            },
        )
        .expect("test projection has a synthetic exact timeline")
    });
    assert!(matches!(
        chord_features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Fillet { groups })
            if matches!(
                groups.as_slice(),
                [cadmpeg_ir::features::edge_treatments::FilletGroup {
                    radius: cadmpeg_ir::features::edge_treatments::RadiusSpec::Chordal {
                        chord_length: actual_chord_length,
                    },
                    tangency_weight: Some(weight),
                    ..
                }] if weight.get() == 1.0 && actual_chord_length.get() == 2.5
            )
    ));
    let chord_only_parameters = [parameter(120, 121, "ChordLen", Some("in"), 0.25)];
    let chord_only_owners = [owner(120, 121, 0)];
    let chord_only_assignments = decode_fillet_radius_groups(
        std::slice::from_ref(&scope),
        &operand_groups[..1],
        &chord_only_owners,
        &chord_only_parameters,
    );
    assert_eq!(chord_only_assignments.len(), 1);
    assert_eq!(
        chord_only_assignments[0].law,
        DesignFilletRadiusLaw::Chordal {
            chord_length_parameter_record_index: 121,
        }
    );
    assert_eq!(
        chord_only_assignments[0].tangency_weight_parameter_record_index,
        None
    );
    let crate::design::feature_project::ProjectedParameterDesign {
        features: chord_only_features,
        ..
    } = crate::test_support::with_decode_context(|ctx| {
        let scopes = std::slice::from_ref(&scope);
        let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            ctx,
            &crate::design::feature_project::ProjectInputs {
                native: &chord_only_parameters,
                owners: &chord_only_owners,
                scopes,
                construction_groups: &operand_groups[..1],
                fillet_radius_groups: &chord_only_assignments,
                timelines: &timelines,
                ..Default::default()
            },
        )
        .expect("test projection has a synthetic exact timeline")
    });
    assert!(matches!(
        chord_only_features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Fillet { groups })
            if matches!(
                groups.as_slice(),
                [cadmpeg_ir::features::edge_treatments::FilletGroup {
                    radius: cadmpeg_ir::features::edge_treatments::RadiusSpec::Chordal {
                        chord_length: actual_chord_length,
                    },
                    tangency_weight: None,
                    ..
                }] if actual_chord_length.get() == 2.5
            )
    ));
    let asymmetric_parameters = [
        parameter(130, 131, "TangencyWeight", None, 1.0),
        parameter(140, 141, "EdgeOffset1", Some("mm"), 0.2),
        parameter(150, 151, "EdgeOffset2", Some("mm"), 0.7),
    ];
    let asymmetric_owners = [owner(130, 131, 0), owner(140, 141, 1), owner(150, 151, 2)];
    let asymmetric_assignments = decode_fillet_radius_groups(
        std::slice::from_ref(&scope),
        &operand_groups[..1],
        &asymmetric_owners,
        &asymmetric_parameters,
    );
    assert_eq!(asymmetric_assignments.len(), 1);
    assert_eq!(
        asymmetric_assignments[0].law,
        DesignFilletRadiusLaw::Asymmetric {
            offset_one_parameter_record_index: 141,
            offset_two_parameter_record_index: 151,
        }
    );
    let crate::design::feature_project::ProjectedParameterDesign {
        features: asymmetric_features,
        ..
    } = crate::test_support::with_decode_context(|ctx| {
        let scopes = std::slice::from_ref(&scope);
        let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            ctx,
            &crate::design::feature_project::ProjectInputs {
                native: &asymmetric_parameters,
                owners: &asymmetric_owners,
                scopes,
                construction_groups: &operand_groups[..1],
                fillet_radius_groups: &asymmetric_assignments,
                timelines: &timelines,
                ..Default::default()
            },
        )
        .expect("test projection has a synthetic exact timeline")
    });
    assert!(matches!(
        asymmetric_features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Fillet { groups })
            if matches!(
                groups.as_slice(),
                [cadmpeg_ir::features::edge_treatments::FilletGroup {
                    radius: cadmpeg_ir::features::edge_treatments::RadiusSpec::Asymmetric {
                        offset_one: actual_offset_one,
                        offset_two: actual_offset_two,
                    },
                    tangency_weight: Some(weight),
                    ..
                }] if weight.get() == 1.0 && actual_offset_one.get() == 2.0 && actual_offset_two.get() == 7.0
            )
    ));
    operand_groups[0]
        .lost_edge_references
        .push("f3d:native/BulkStream.dat:lost-edge-reference#1".into());

    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = std::slice::from_ref(&scope);
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    native: &parameters,
                    owners: &owners,
                    scopes,
                    construction_groups: &operand_groups,
                    fillet_radius_groups: &assignments,
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });
    let FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) =
        features[0].evaluation.definition()
    else {
        panic!("expected typed localized Fillet");
    };
    assert_eq!(groups.len(), 2);
    assert!(matches!(
        &groups[0],
        cadmpeg_ir::features::edge_treatments::FilletGroup {
            edges: cadmpeg_ir::features::EdgeSelection::Unresolved,
            radius: cadmpeg_ir::features::edge_treatments::RadiusSpec::Constant {
                radius: actual_radius,
            },
            tangency_weight: Some(weight),
        } if weight.get() == 1.0 && actual_radius.get() == 5.0
    ));
    assert!(matches!(
        &groups[1],
        cadmpeg_ir::features::edge_treatments::FilletGroup {
            edges: cadmpeg_ir::features::EdgeSelection::Native(selection),
            radius: cadmpeg_ir::features::edge_treatments::RadiusSpec::Constant {
                radius: actual_radius,
            },
            tangency_weight: Some(weight),
        } if weight.get() == 0.75 && (selection == &operand_groups[1].id) && actual_radius.get() == 3.0
    ));

    let mut patch_scope = scope.clone();
    patch_scope
        .try_edit(|draft| {
            draft.payload = DesignFeatureKind::SurfacePatch.try_into().unwrap();
            draft.frame_length = 354;
            draft.reference_members = ReferenceRun::unlocated(vec![100, 200, 300, 301]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let patch_boundary =
        |scope_reference_ordinal, record_index, model_reference| DesignSurfacePatchBoundary {
            scope_reference_ordinal,
            record_index,
            is_seed_selection: false,
            continuity: DesignPatchContinuity::Connected,
            flip: 2,
            scale: crate::test_support::real(-1.0),
            model_reference,
        };
    if let DesignScopePayloadMut::SurfacePatch(slot) = patch_scope.payload_mut() {
        let slot = &mut slot.boundaries;
        *slot = vec![patch_boundary(2, 300, 100)];
    }
    let mut patch_group = group(100, 0, vec![200]);
    patch_group.operand_role = DesignConstructionOperandRole::Other(DesignOperandRole::BODIES_A);
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| crate::design::feature_project::project_surface_patch(decode_ctx, &patch_scope, std::slice::from_ref(&patch_group), &[], &[], &[])).unwrap(),
        Some(FeatureDefinition::Operation(FeatureOperation::FilledSurface {
            boundary: cadmpeg_ir::features::SurfaceBoundary::Path(
                cadmpeg_ir::features::PathRef::Native(ref native)
            ),
            support_faces: cadmpeg_ir::features::FaceSelection::Faces(ref faces),
            ref continuity,
            merge_result: Some(false),
        })) if continuity.resolved().map(|resolved| resolved.conditions.as_slice())
            == Some(&[cadmpeg_ir::features::FilledSurfaceBoundaryCondition::Contact {}][..])
            && native == &patch_group.id
            && faces.is_empty()
    ));
    for normal in [
        crate::records::feature::surface_ops::DesignSurfacePatchNormalState::Explicit {
            reversed: crate::records::identity::Located {
                value: false,
                offset: patch_scope.byte_offset() + 31,
            },
        },
        crate::records::feature::surface_ops::DesignSurfacePatchNormalState::Explicit {
            reversed: crate::records::identity::Located {
                value: true,
                offset: patch_scope.byte_offset() + 31,
            },
        },
        crate::records::feature::surface_ops::DesignSurfacePatchNormalState::Unsupported,
    ] {
        let mut variant = patch_scope.clone();
        if let DesignScopePayloadMut::SurfacePatch(patch) = variant.payload_mut() {
            patch.normal = Some(normal);
        }
        let projected = crate::test_support::with_decode_context(|ctx| {
            crate::design::feature_project::project_surface_patch(
                ctx,
                &variant,
                std::slice::from_ref(&patch_group),
                &[],
                &[],
                &[],
            )
        })
        .unwrap();
        assert_eq!(
            projected.is_some(),
            matches!(
                normal,
                crate::records::feature::surface_ops::DesignSurfacePatchNormalState::Explicit {
                    reversed: crate::records::identity::Located { value: false, .. }
                }
            )
        );
    }

    patch_scope
        .try_edit(|draft| {
            draft.frame_length = 398;
            draft.reference_members =
                ReferenceRun::unlocated(vec![100, 200, 300, 101, 201, 301, 102]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let DesignScopePayloadMut::SurfacePatch(slot) = patch_scope.payload_mut() {
        let slot = &mut slot.boundaries;
        *slot = vec![patch_boundary(2, 300, 100), patch_boundary(5, 301, 101)];
    }
    let mut second_patch_group = group(101, 3, vec![201]);
    second_patch_group.operand_role =
        DesignConstructionOperandRole::Other(DesignOperandRole::BODIES_A);
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| crate::design::feature_project::project_surface_patch(decode_ctx, &patch_scope, &[patch_group.clone(), second_patch_group.clone()], &[], &[], &[])).unwrap(),
        Some(FeatureDefinition::Operation(FeatureOperation::FilledSurface {
            boundary: cadmpeg_ir::features::SurfaceBoundary::Path(
                cadmpeg_ir::features::PathRef::Native(ref native)
            ),
            ..
        })) if native == &patch_scope.id
    ));

    patch_scope
        .try_edit(|draft| {
            draft.previous_history_state_id = Some(8);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let edge_identity = |record_index, group_record_index, edge| {
        DesignEdgeIdentityOperand::try_new(DesignEdgeIdentityOperandDraft {
            id: format!("f3d:native/BulkStream.dat:edge-identity#{record_index}"),
            scope_record_index: patch_scope.record_index,
            group_record_index,
            group_member_ordinal: 0,
            record_index,
            byte_offset: 0,
            class_tag: DesignClassTag::try_from("297".to_owned()).unwrap(),
            layout: DesignEdgeIdentityLayout::Full,
            local_id: u64::from(record_index),
            asset_id: DesignRelaxedGuidText::try_from(
                "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
            )
            .unwrap(),
            asset_id_offset: 42,
            context_id: DesignRelaxedGuidText::try_from(
                "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
            )
            .unwrap(),
            context_id_offset: 118,
            historical: None,
            treatment_radius_candidates: Vec::new(),
            transition_edge_candidates: Vec::new(),
            resolved_edge_slots: Vec::new(),
            resolved_edge_slot: Some(edge),
            resolution_identity_id: None,
        })
        .unwrap()
    };
    let identities = vec![edge_identity(200, 100, 17), edge_identity(201, 101, 18)];
    let resolved = crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::feature_project::project_surface_patch(
            decode_ctx,
            &patch_scope,
            &[patch_group.clone(), second_patch_group],
            &[],
            &identities,
            &[],
        )
    })
    .unwrap()
    .expect("resolved multi-group SurfacePatch path");
    let FeatureDefinition::Operation(FeatureOperation::FilledSurface {
        boundary:
            cadmpeg_ir::features::SurfaceBoundary::Path(
                cadmpeg_ir::features::PathRef::HistoricalEdges { edges, native, .. },
            ),
        ..
    }) = resolved
    else {
        panic!("expected historical multi-group SurfacePatch path");
    };
    assert_eq!(edges.len(), 2);
    assert_eq!(native.as_str(), patch_scope.id);
    patch_scope
        .try_edit(|draft| {
            draft.previous_history_state_id = None;

            draft.frame_length = 339;
            draft.reference_members = ReferenceRun::unlocated(vec![100, 200, 300]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let DesignScopePayloadMut::SurfacePatch(slot) = patch_scope.payload_mut() {
        let slot = &mut slot.boundaries;
        *slot = vec![patch_boundary(2, 300, 100)];
    }
    patch_group.operand_role = DesignConstructionOperandRole::Other(DesignOperandRole::PROFILE);
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| crate::design::feature_project::project_surface_patch(decode_ctx, &patch_scope, std::slice::from_ref(&patch_group), &[], &[], &[])).unwrap(),
        Some(FeatureDefinition::Operation(FeatureOperation::FilledSurface {
            boundary: cadmpeg_ir::features::SurfaceBoundary::Path(
                cadmpeg_ir::features::PathRef::Native(ref native)
            ),
            ..
        })) if native == &patch_group.id
    ));

    // The earlier scope-envelope generation is fourteen bytes shorter in both
    // forms and projects the same feature from the same reference shape.
    patch_scope
        .try_edit(|draft| {
            draft.frame_length = 325;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::feature_project::project_surface_patch(
                decode_ctx,
                &patch_scope,
                std::slice::from_ref(&patch_group),
                &[],
                &[],
                &[],
            )
        })
        .unwrap(),
        Some(FeatureDefinition::Operation(
            FeatureOperation::FilledSurface { .. }
        ))
    ));
    patch_scope
        .try_edit(|draft| {
            draft.frame_length = 340;
            draft.reference_members = ReferenceRun::unlocated(vec![100, 200, 300, 301]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let DesignScopePayloadMut::SurfacePatch(slot) = patch_scope.payload_mut() {
        let slot = &mut slot.boundaries;
        *slot = vec![patch_boundary(2, 300, 100)];
    }
    patch_group.operand_role = DesignConstructionOperandRole::Other(DesignOperandRole::BODIES_A);
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::feature_project::project_surface_patch(
                decode_ctx,
                &patch_scope,
                std::slice::from_ref(&patch_group),
                &[],
                &[],
                &[],
            )
        })
        .unwrap(),
        Some(FeatureDefinition::Operation(
            FeatureOperation::FilledSurface { .. }
        ))
    ));
    patch_scope
        .try_edit(|draft| {
            draft.reference_members = ReferenceRun::unlocated(vec![100, 200, 300, 301, 302]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert!(crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::feature_project::project_surface_patch(
            decode_ctx,
            &patch_scope,
            std::slice::from_ref(&patch_group),
            &[],
            &[],
            &[],
        )
    })
    .unwrap()
    .is_none());

    patch_scope
        .try_edit(|draft| {
            draft.frame_length = 343;
            draft.reference_members = ReferenceRun::unlocated(vec![100, 200, 201, 202, 203, 300]);
            draft.payload = DesignScopePayload::SurfacePatch(Default::default());
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    patch_group
        .try_set_members(
            vec![200, 201, 202, 203]
                .into_iter()
                .enumerate()
                .map(|(index, value)| Located {
                    value,
                    offset: u64_from_index(index) * 11,
                })
                .collect(),
        )
        .unwrap();
    let grouped_projection = crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::feature_project::project_surface_patch(
            decode_ctx,
            &patch_scope,
            std::slice::from_ref(&patch_group),
            &[],
            &[],
            &[],
        )
    })
    .unwrap();
    assert!(matches!(
        grouped_projection,
        Some(FeatureDefinition::Operation(FeatureOperation::FilledSurface {
            boundary: cadmpeg_ir::features::SurfaceBoundary::Path(
                cadmpeg_ir::features::PathRef::Native(ref native)
            ),
            ref continuity,
            ..
        })) if continuity.uniform_value()
            == Some(cadmpeg_ir::features::SurfaceContinuity::Contact)
            && native == &patch_group.id
    ));

    let mut fill_scope = scope.clone();
    fill_scope
        .try_edit(|draft| {
            draft.payload = DesignFeatureKind::BoundaryFill.try_into().unwrap();
            draft.reference_members = ReferenceRun::unlocated(vec![100, 200, 201, 300, 301, 400]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut tools = group(100, 0, vec![200, 201]);
    tools.operand_role = DesignConstructionOperandRole::Other(DesignOperandRole::BODIES_A);
    let mut cell = group(300, 3, vec![301]);
    cell.operand_role = DesignConstructionOperandRole::Other(DesignOperandRole::ROLE_0X5);
    assert!(matches!(
        crate::design::feature_project::project_boundary_fill(&fill_scope, &[tools.clone(), cell.clone()]),
        Some(FeatureDefinition::Operation(FeatureOperation::BoundaryFill {
            tools: cadmpeg_ir::features::BodySelection::Native(ref tool_selection),
            cells: ref cell_selections,
        })) if tool_selection == &tools.id
            && cell_selections.as_slice() == [cadmpeg_ir::features::BodySelection::Native(cell.id)]
    ));
}

#[test]
fn assigned_and_unassigned_variable_fillet_groups_project_identical_radius_controls() {
    let scope = localized_fillet_scope();
    let operand_groups = localized_fillet_operand_groups();
    let parameter = localized_fillet_parameter;
    let owner = localized_fillet_owner;
    let variable_parameters = [
        parameter(50, 51, "StartRadius", Some("mm"), 0.2),
        parameter(60, 61, "EndRadius", Some("mm"), 0.6),
        parameter(70, 71, "MidRadius", Some("mm"), 0.4),
        parameter(80, 81, "MidParams", None, 0.25),
        parameter(90, 91, "TangencyWeight", None, 0.75),
    ];
    let variable_owners = [
        owner(50, 51, 0),
        owner(60, 61, 1),
        owner(70, 71, 2),
        owner(80, 81, 3),
        owner(90, 91, 4),
    ];
    let variable_assignments = decode_fillet_radius_groups(
        std::slice::from_ref(&scope),
        &operand_groups[..1],
        &variable_owners,
        &variable_parameters,
    );
    let crate::design::feature_project::ProjectedParameterDesign {
        features: variable_features,
        ..
    } = crate::test_support::with_decode_context(|ctx| {
        let scopes = std::slice::from_ref(&scope);
        let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            ctx,
            &crate::design::feature_project::ProjectInputs {
                native: &variable_parameters,
                owners: &variable_owners,
                scopes,
                construction_groups: &operand_groups[1..],
                fillet_radius_groups: &variable_assignments,
                timelines: &timelines,
                ..Default::default()
            },
        )
        .expect("test projection has a synthetic exact timeline")
    });
    let FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) =
        variable_features[0].evaluation.definition()
    else {
        panic!("expected assigned variable Fillet");
    };
    assert_eq!(groups.len(), 1);
    assert_eq!(
        groups[0].edges,
        cadmpeg_ir::features::EdgeSelection::Native(variable_assignments[0].id.clone())
    );
    assert_eq!(
        groups[0]
            .tangency_weight
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        Some(0.75)
    );
    assert!(matches!(
        &groups[0].radius,
        cadmpeg_ir::features::edge_treatments::RadiusSpec::Variable { points } if points.as_slice().len() == 3
    ));
    let cadmpeg_ir::features::edge_treatments::RadiusSpec::Variable { points } = &groups[0].radius
    else {
        panic!("expected assigned variable radius controls");
    };
    assert_eq!(
        points
            .as_slice()
            .iter()
            .map(|point| (point.parameter.get(), point.radius.get()))
            .collect::<Vec<_>>(),
        vec![(0.0, 2.0), (0.25, 4.0), (1.0, 6.0)]
    );
    let crate::design::feature_project::ProjectedParameterDesign {
        features: unassigned_features,
        ..
    } = crate::test_support::with_decode_context(|ctx| {
        let scopes = std::slice::from_ref(&scope);
        let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            ctx,
            &crate::design::feature_project::ProjectInputs {
                native: &variable_parameters,
                owners: &variable_owners,
                scopes,
                construction_groups: &operand_groups[1..],
                timelines: &timelines,
                ..Default::default()
            },
        )
        .expect("test projection has a synthetic exact timeline")
    });
    let FeatureDefinition::Operation(FeatureOperation::Fillet {
        groups: unassigned_groups,
    }) = unassigned_features[0].evaluation.definition()
    else {
        panic!("expected unassigned variable Fillet");
    };
    assert_eq!(unassigned_groups.len(), 1);
    assert_eq!(
        unassigned_groups[0].edges,
        cadmpeg_ir::features::EdgeSelection::Native(operand_groups[1].id.clone())
    );
    assert_eq!(unassigned_groups[0].radius, groups[0].radius);
    assert_eq!(
        unassigned_groups[0].tangency_weight,
        groups[0].tangency_weight
    );
}

#[test]
fn fillet_projection_rejects_mistyped_assignment_records_without_panicking() {
    use crate::records::topology::{
        fillet::DesignFilletRadiusGroup, fillet::DesignFilletRadiusLaw,
    };
    let mut weight = parse_design_parameter_record(&parameter_record(
        Some(1),
        "1",
        "TangencyWeight",
        None,
        "weight",
        1.0,
    ))
    .unwrap();
    weight.record_index = 11;
    let mut radius = parse_design_parameter_record(&parameter_record(
        Some(2),
        "1 mm",
        "Radius",
        Some("mm"),
        "radius",
        1.0,
    ))
    .unwrap();
    radius.record_index = 12;
    let scope = DesignParameterScope::empty("f3d:native:scope#10", DesignFeatureKind::Fillet, 10);
    let laws = [
        (
            DesignFilletRadiusLaw::Constant {
                radius_parameter_record_index: 11,
            },
            vec![(0, &weight)],
        ),
        (
            DesignFilletRadiusLaw::Chordal {
                chord_length_parameter_record_index: 11,
            },
            vec![(0, &weight)],
        ),
        (
            DesignFilletRadiusLaw::Asymmetric {
                offset_one_parameter_record_index: 11,
                offset_two_parameter_record_index: 12,
            },
            vec![(0, &weight), (1, &radius)],
        ),
        (
            DesignFilletRadiusLaw::Variable {
                start_radius_parameter_record_index: 11,
                end_radius_parameter_record_index: 12,
                middle: Vec::new(),
            },
            vec![(0, &weight), (1, &radius)],
        ),
    ];
    for (law, parameters) in laws {
        let assignment = DesignFilletRadiusGroup {
            id: "f3d:native:assignment#20".into(),
            scope_record_index: 10,
            group_ordinal: 0,
            group_record_index: 20,
            edge_operand_record_indices: Vec::new(),
            law,
            tangency_weight_parameter_record_index: None,
        };
        let inputs = crate::design::feature_project::ProjectInputs {
            fillet_radius_groups: std::slice::from_ref(&assignment),
            ..Default::default()
        };
        assert!(matches!(
            crate::test_support::with_decode_context(|decode_ctx| {
                crate::design::feature_project::project_fillet_arm(
                    decode_ctx,
                    &inputs,
                    &scope,
                    &parameters,
                    "f3d:native",
                )
            }),
            Ok(FeatureDefinition::Operation(
                FeatureOperation::Native { .. }
            ))
        ));
    }
}

#[test]
fn fillet_unit_conversion_rejects_finite_overflow() {
    let parameter = |kind, value| {
        parse_design_parameter_record(&parameter_record(
            Some(1),
            "1 mm",
            kind,
            Some("mm"),
            "radius",
            value,
        ))
        .unwrap()
    };
    let start = parameter("StartRadius", f64::MAX);
    let end = parameter("EndRadius", 1.0);
    assert!(crate::design::feature_project::design_length(&start).is_none());
    assert!(
        crate::design::feature_project::variable_fillet_law(&[(0, &start), (1, &end)]).is_none()
    );
}

mod chamfer;

mod tangency_weight;
mod typed_treatments;
