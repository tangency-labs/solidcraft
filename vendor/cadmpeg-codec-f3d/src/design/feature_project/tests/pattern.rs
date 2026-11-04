// SPDX-License-Identifier: Apache-2.0

use crate::design::feature_project::{
    project_circular_pattern, project_rectangular_pattern_scalars,
};
use crate::records::{
    feature::{patterns::DesignRectangularPatternConstruction, scope::DesignParameterScope},
    topology::{
        construction::DesignConstructionOperandGroup,
        construction::DesignConstructionOperandGroupFrame, extrude_selection::DesignOperandRole,
    },
};
use cadmpeg_ir::features::{
    patterns::{PatternSeed, PatternTransform},
    BodySelection, FaceSelection, FeatureDefinition, FeatureOperation,
};

const EPS_SPACING: f64 = 1.0e-12;

fn group(
    scope_record_index: u32,
    record_index: u32,
    role: DesignOperandRole,
) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: format!(
                "f3d:Design/BulkStream.dat:design-construction-operand-group#{record_index}"
            ),
            scope_record_index,
            scope_reference_ordinal: 1,
            record_index,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("313".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: record_index + 1,
                offset: 0,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 0,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 99,
                    opaque_index_offset: 18,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 22,
                    variant: false,
                    closing_flag: false,
                },
            )
            .unwrap(),
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::Other(role),
            role_offset: 0,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "263".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 0,
        },
    )
    .unwrap()
}

fn rectangular_scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::RPattern,
        10,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::RPattern(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::RectangularPattern(slot) =
        scope.payload_mut()
    {
        *slot = Some(
            DesignRectangularPatternConstruction::try_from(
                crate::records::feature::patterns::DesignRectangularPatternConstructionWire {
                    u_count: 3,
                    v_count: 1,
                    u_extent: 10.0,
                    v_extent: 0.0,
                    owner_record_indices: [11, 12, 13, 14],
                    value_offsets: [101, 102, 103, 104],
                    instances: None,
                    instance_search_cap: None,
                    directions: Vec::new(),
                    secondary_direction: None,
                },
            )
            .unwrap(),
        );
    }
    scope
}

fn circular_scope() -> DesignParameterScope {
    use crate::records::feature::patterns::{
        DesignCircularPatternAxis, DesignCircularPatternConstruction,
    };
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::CPattern,
        10,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::CPattern(slot) =
        scope.payload_mut()
    {
        *slot = Some(DesignCircularPatternConstruction {
            count: std::num::NonZeroU32::new(3).unwrap(),
            count_record_index: 11,
            count_offset: 0,
            angle: cadmpeg_ir::scalar::PositiveAngle::new(std::f64::consts::TAU).unwrap(),
            angle_record_index: 12,
            angle_offset: 0,
            axis: DesignCircularPatternAxis::Inline {
                origin: crate::test_support::reals([0.0, 0.0, 0.0]),
                origin_offset: 0,
                direction: cadmpeg_ir::units::UnitVector3::normalized(
                    cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                )
                .unwrap(),
                direction_offset: 0,
            },
            axis_record_index: 13,
            selection_record_index: 14,
        });
    }
    scope
}

#[test]
fn circular_pattern_seed_role_selects_body_or_face() {
    for (role, expected_seed) in [
        (
            DesignOperandRole::BODIES_B,
            PatternSeed::Bodies(BodySelection::Native(
                "f3d:Design/BulkStream.dat:design-construction-operand-group#20".into(),
            )),
        ),
        (
            DesignOperandRole::BODIES_A,
            PatternSeed::Faces(FaceSelection::Native(
                "f3d:Design/BulkStream.dat:design-construction-operand-group#20".into(),
            )),
        ),
    ] {
        let definition =
            project_circular_pattern(&circular_scope(), &[group(10, 20, role)], &[], &[], &[])
                .expect("circular pattern");
        let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) = definition
        else {
            panic!("circular pattern definition");
        };
        assert_eq!(seeds, vec![expected_seed]);
        assert!(matches!(
            pattern.definition(),
            PatternTransform::Circular { .. }
        ));
    }
}

fn assert_linear_seed(definition: FeatureDefinition, expected_seed: PatternSeed) {
    let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) = definition
    else {
        panic!("rectangular pattern definition");
    };
    assert_eq!(seeds, vec![expected_seed]);
    let PatternTransform::Linear {
        direction,
        spacing,
        count,
        second,
        omitted_instances,
    } = pattern.definition().clone()
    else {
        panic!("linear rectangular pattern");
    };
    assert!(omitted_instances.is_empty());
    assert!(direction.is_none());
    assert!((spacing.get() - 50.0).abs() < EPS_SPACING);
    assert_eq!(count, 3);
    assert!(second.is_none());
}

#[test]
fn rectangular_pattern_seed_role_selects_body_or_face() {
    let body_scope = rectangular_scope();
    let body_group = group(10, 20, DesignOperandRole::BODIES_B);
    let body_definition =
        project_rectangular_pattern_scalars(&body_scope, &[body_group], &[], &[], &[])
            .expect("body rectangular pattern");
    assert_linear_seed(
        body_definition,
        PatternSeed::Bodies(BodySelection::Native(
            "f3d:Design/BulkStream.dat:design-construction-operand-group#20".into(),
        )),
    );

    let face_scope = rectangular_scope();
    let face_group = group(10, 30, DesignOperandRole::BODIES_A);
    let face_definition =
        project_rectangular_pattern_scalars(&face_scope, &[face_group], &[], &[], &[])
            .expect("face rectangular pattern");
    assert_linear_seed(
        face_definition,
        PatternSeed::Faces(FaceSelection::Native(
            "f3d:Design/BulkStream.dat:design-construction-operand-group#30".into(),
        )),
    );
}

fn direction_record(
    record_index: u32,
    displacement: [f64; 3],
) -> crate::records::feature::patterns::DesignRectangularPatternDirection {
    crate::records::feature::patterns::DesignRectangularPatternDirection {
        record_index,
        selection_record_index: record_index + 3,
        selection_kind: 8,
        distance_type: 8,
        origin: crate::test_support::reals([0.0, 0.0, 0.0]),
        origin_offset: 0,
        displacement: crate::test_support::reals(displacement),
        displacement_offset: 24,
    }
}

fn two_direction_scope(
    directions: Vec<crate::records::feature::patterns::DesignRectangularPatternDirection>,
) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::RPattern,
        10,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::RPattern(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::RectangularPattern(slot) =
        scope.payload_mut()
    {
        *slot = Some(
            DesignRectangularPatternConstruction::try_from(
                crate::records::feature::patterns::DesignRectangularPatternConstructionWire {
                    u_count: 4,
                    v_count: 2,
                    u_extent: 3.0,
                    v_extent: -1.2,
                    owner_record_indices: [11, 12, 13, 14],
                    value_offsets: [101, 102, 103, 104],
                    instances: None,
                    instance_search_cap: None,
                    directions,
                    secondary_direction: None,
                },
            )
            .unwrap(),
        );
    }
    scope
}

#[test]
fn rectangular_pattern_direction_records_supply_both_directions() {
    let scope = two_direction_scope(vec![
        direction_record(20, [2.0, 0.0, 0.0]),
        direction_record(30, [0.0, 2.5, 0.0]),
    ]);
    let definition = project_rectangular_pattern_scalars(&scope, &[], &[], &[], &[])
        .expect("two-direction rectangular pattern");
    let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) = definition else {
        panic!("rectangular pattern definition");
    };
    let PatternTransform::Linear {
        direction,
        spacing,
        count,
        second,
        omitted_instances,
    } = pattern.definition().clone()
    else {
        panic!("linear rectangular pattern");
    };
    assert!(omitted_instances.is_empty());
    let direction =
        cadmpeg_ir::features::FiniteVector3::from(direction.expect("first direction")).get();
    assert!((direction.x - 1.0).abs() < EPS_SPACING);
    assert!(direction.y.abs() < EPS_SPACING && direction.z.abs() < EPS_SPACING);
    assert!((spacing.get() - 10.0).abs() < EPS_SPACING);
    assert_eq!(count, 4);
    let second = second.expect("second direction");
    let second_direction = cadmpeg_ir::features::FiniteVector3::from(second.direction).get();
    assert!((second_direction.y + 1.0).abs() < EPS_SPACING);
    assert!(second_direction.x.abs() < EPS_SPACING && second_direction.z.abs() < EPS_SPACING);
    assert!((second.spacing.get() - 12.0).abs() < EPS_SPACING);
    assert_eq!(second.count, 2);
}

#[test]
fn two_active_directions_without_direction_records_do_not_project() {
    assert!(project_rectangular_pattern_scalars(
        &two_direction_scope(Vec::new()),
        &[],
        &[],
        &[],
        &[]
    )
    .is_none());
}

fn single_direction_scope_with_run(
    directions: Vec<crate::records::feature::patterns::DesignRectangularPatternDirection>,
) -> DesignParameterScope {
    use crate::records::feature::patterns::{
        DesignPatternInstance, DesignRectangularPatternInstances,
    };
    use crate::records::sketch_placement::SketchPlacementMatrix;

    let at = |x: f64| {
        let mut matrix: [[f64; 4]; 4] = SketchPlacementMatrix::IDENTITY.into();
        matrix[0][3] = x;
        SketchPlacementMatrix::try_from(matrix).unwrap()
    };
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::RPattern,
        10,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::RPattern(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::RectangularPattern(slot) =
        scope.payload_mut()
    {
        *slot = Some(
            DesignRectangularPatternConstruction::try_from(
                crate::records::feature::patterns::DesignRectangularPatternConstructionWire {
                    u_count: 3,
                    v_count: 1,
                    u_extent: 2.0,
                    v_extent: 0.0,
                    owner_record_indices: [11, 12, 13, 14],
                    value_offsets: [101, 102, 103, 104],
                    instances: Some(DesignRectangularPatternInstances::Bodies(
                        [0.0, 1.0, 2.0]
                            .into_iter()
                            .enumerate()
                            .map(|(index, x)| DesignPatternInstance {
                                record_index: 40 + u32::try_from(index).unwrap(),
                                transform: crate::records::identity::Located {
                                    value: at(x),
                                    offset: 1000 * u64::try_from(index).unwrap(),
                                },
                            })
                            .collect(),
                    )),
                    instance_search_cap: None,
                    directions,
                    secondary_direction: None,
                },
            )
            .unwrap(),
        );
    }
    scope
}

fn linear_direction(definition: FeatureDefinition) -> Option<cadmpeg_ir::math::Vector3> {
    let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) = definition else {
        panic!("rectangular pattern definition");
    };
    let PatternTransform::Linear {
        direction,
        omitted_instances,
        ..
    } = pattern.definition().clone()
    else {
        panic!("linear rectangular pattern");
    };
    assert!(omitted_instances.is_empty());
    direction.map(|direction| cadmpeg_ir::features::FiniteVector3::from(direction).get())
}

#[test]
fn a_single_direction_record_must_agree_with_the_instance_run() {
    let run = linear_direction(
        project_rectangular_pattern_scalars(
            &single_direction_scope_with_run(Vec::new()),
            &[],
            &[],
            &[],
            &[],
        )
        .expect("instance-run pattern"),
    )
    .expect("instance-run direction");
    assert!((run.x - 1.0).abs() < EPS_SPACING);

    let agreeing = linear_direction(
        project_rectangular_pattern_scalars(
            &single_direction_scope_with_run(vec![direction_record(20, [1.5, 0.0, 0.0])]),
            &[],
            &[],
            &[],
            &[],
        )
        .expect("agreeing pattern"),
    )
    .expect("agreeing direction");
    assert!((agreeing.x - 1.0).abs() < EPS_SPACING);

    let disagreeing = project_rectangular_pattern_scalars(
        &single_direction_scope_with_run(vec![direction_record(20, [0.0, 1.5, 0.0])]),
        &[],
        &[],
        &[],
        &[],
    )
    .expect("disagreeing pattern");
    assert!(linear_direction(disagreeing).is_none());
}

fn seed_scope(record_index: u32, history_state_id: Option<i64>) -> DesignParameterScope {
    DesignParameterScope::try_new(crate::records::feature::scope::DesignParameterScopeDraft {
        id: format!("f3d:Design/BulkStream.dat:design-parameter-scope#{record_index}"),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("353".to_owned()).unwrap(),
        record_index,
        frame_length: 128,
        kind_offset: 32,
        feature_ordinal: std::num::NonZeroU32::MIN,
        feature_ordinal_offset: 48,
        history_state_id,
        previous_history_state_id: None,
        previous_history_state_id_offset: None,
        reference_count_offset: 9,
        reference_members: crate::records::identity::ReferenceRun::located(vec![
            crate::records::identity::Located {
                value: record_index,
                offset: 14,
            },
        ]),
        payload: crate::records::feature::scope::DesignFeatureKind::Extrude
            .try_into()
            .unwrap(),
        unclosed_construction_operand_groups: Vec::new(),
        paired_class_tag: crate::records::references::DesignClassTag::try_from("263".to_owned())
            .unwrap(),
        paired_byte_offset: 128,
    })
    .unwrap()
}

/// A compact entity selection: identity record `N + 3` stores one u64
/// identity at offset 21, and the next header follows at offset 29.
fn compact_entity_selection(
    group_record_index: u32,
    record_index: u32,
    identity: u64,
) -> crate::records::topology::entity_selection::DesignEntitySelectionOperand {
    serde_json::from_value(serde_json::json!({
        "id": format!("f3d:Design/BulkStream.dat:design-entity-selection-operand#{record_index}"),
        "scope_record_index": 10,
        "group_record_index": group_record_index, "group_member_ordinal": 0,
        "record_index": record_index, "byte_offset": 100, "class_tag": "335",
        "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d", "asset_id_offset": 130,
        "context_id": "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e", "context_id_offset": 210,
        "identity_record_index": record_index + 3, "identity_record_offset": 300,
        "primary_identity": identity, "primary_identity_offset": 321,
        "next_record_index": record_index + 4, "next_byte_offset": 329
    }))
    .unwrap()
}

#[test]
fn compact_entity_selection_seed_names_the_seed_feature() {
    let feature = seed_scope(5, Some(2));
    let expected = PatternSeed::Feature(crate::ids::neutral_feature_id(&feature));
    let rectangular = project_rectangular_pattern_scalars(
        &rectangular_scope(),
        &[group(10, 30, DesignOperandRole::BODIES_A)],
        &[],
        &[compact_entity_selection(30, 31, 5)],
        &[rectangular_scope(), feature.clone()],
    )
    .expect("rectangular pattern");
    assert_linear_seed(rectangular, expected.clone());

    let circular = project_circular_pattern(
        &circular_scope(),
        &[group(10, 30, DesignOperandRole::BODIES_A)],
        &[],
        &[compact_entity_selection(30, 31, 5)],
        &[circular_scope(), feature],
    )
    .expect("circular pattern");
    let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) = circular else {
        panic!("circular pattern definition");
    };
    assert_eq!(seeds, vec![expected]);
}

#[test]
fn compact_entity_selection_seed_without_a_feature_scope_stays_native() {
    let native = PatternSeed::Faces(FaceSelection::Native(
        "f3d:Design/BulkStream.dat:design-construction-operand-group#30".into(),
    ));
    for (identity, scopes) in [
        (5, vec![seed_scope(5, None)]),
        (6, vec![seed_scope(5, Some(2))]),
        (10, vec![rectangular_scope()]),
    ] {
        let definition = project_rectangular_pattern_scalars(
            &rectangular_scope(),
            &[group(10, 30, DesignOperandRole::BODIES_A)],
            &[],
            &[compact_entity_selection(30, 31, identity)],
            &scopes,
        )
        .expect("rectangular pattern");
        assert_linear_seed(definition, native.clone());
    }
}

#[test]
fn spacing_pattern_payload_projects_both_axes_and_feature_seed() {
    use crate::records::feature::patterns::DesignRectangularPatternConstructionWire;
    let mut direction = direction_record(20, [0.0, 2.0, 0.0]);
    direction.distance_type = 7;
    let mut scope = two_direction_scope(Vec::new());
    let mut wire = DesignRectangularPatternConstructionWire::from(
        scope.rectangular_pattern_construction().unwrap().clone(),
    );
    wire.u_count = 5;
    wire.v_count = 3;
    wire.u_extent = 1.7;
    wire.v_extent = -0.9;
    wire.directions = vec![direction];
    wire.secondary_direction = Some(crate::records::identity::Located {
        value: crate::test_support::reals([0.0, 0.0, 4.0]),
        offset: 63,
    });
    if let crate::records::feature::scope::DesignScopePayloadMut::RPattern(slot) =
        scope.payload_mut()
    {
        *slot = Some(DesignRectangularPatternConstruction::try_from(wire).unwrap());
    }
    let feature = seed_scope(5, Some(2));
    let expected = PatternSeed::Feature(crate::ids::neutral_feature_id(&feature));
    let definition = project_rectangular_pattern_scalars(
        &scope,
        &[group(10, 30, DesignOperandRole::BODIES_A)],
        &[],
        &[compact_entity_selection(30, 31, 5)],
        &[feature],
    )
    .expect("spacing pattern");
    let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) = definition
    else {
        panic!("pattern");
    };
    assert_eq!(seeds, vec![expected]);
    let PatternTransform::Linear {
        direction,
        spacing,
        count,
        second,
        omitted_instances,
    } = pattern.definition()
    else {
        panic!("linear pattern");
    };
    assert!(omitted_instances.is_empty());
    assert_eq!(*count, 5);
    assert!((spacing.get() - 17.0).abs() < EPS_SPACING);
    assert_eq!(
        cadmpeg_ir::features::FiniteVector3::from(direction.unwrap()).get(),
        cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)
    );
    let second = second.as_ref().unwrap();
    assert_eq!(second.count, 3);
    assert!((second.spacing.get() - 9.0).abs() < EPS_SPACING);
    assert_eq!(
        cadmpeg_ir::features::FiniteVector3::from(second.direction).get(),
        cadmpeg_ir::math::Vector3::new(0.0, 0.0, -1.0)
    );
    let record = scope.rectangular_pattern_construction().unwrap();
    assert_eq!(
        serde_json::from_value::<DesignRectangularPatternConstruction>(
            serde_json::to_value(record).unwrap()
        )
        .unwrap(),
        *record
    );
}
