// SPDX-License-Identifier: Apache-2.0

use super::extrude_fallbacks::{profile_group, scope};
use crate::design::feature_project::project_fixed_loft;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::path_features::DesignLoftConstruction;
use crate::records::feature::scope::DesignScopePayload;
use crate::records::topology::construction::DesignConstructionOperandRole;
use crate::records::topology::entity_selection::DesignEntitySelectionOperand;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, LoftSection};

fn loft_input(
    roles: [DesignOperandRole; 2],
) -> (
    crate::records::feature::scope::DesignParameterScope,
    [crate::records::topology::construction::DesignConstructionOperandGroup; 2],
) {
    let mut scope = scope();
    scope
        .try_edit(|draft| {
            draft.payload = DesignScopePayload::Loft(Some(DesignLoftConstruction {
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: 128,
                reference_target: None,
            }));
        })
        .unwrap();
    let mut first = profile_group(100, 0);
    first.operand_role = DesignConstructionOperandRole::Other(roles[0]);
    let mut second = profile_group(101, 1);
    second.operand_role = DesignConstructionOperandRole::Other(roles[1]);
    (scope, [first, second])
}

fn project(
    ctx: &DecodeContext<'_>,
    scope: &crate::records::feature::scope::DesignParameterScope,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    entity_selection_operands: &[DesignEntitySelectionOperand],
) -> Result<Option<FeatureDefinition>, CodecError> {
    project_fixed_loft(
        scope,
        &crate::design::feature_project::ProjectInputs {
            construction_groups: groups,
            entity_selection_operands,
            ..Default::default()
        },
        ctx,
    )
}

/// The compact sketch-point selection that forms the member of `group`.
fn point_selection(
    group: &crate::records::topology::construction::DesignConstructionOperandGroup,
) -> DesignEntitySelectionOperand {
    let mut draft = loft_point_sources().1.into_draft();
    draft.group_record_index = group.record_index;
    DesignEntitySelectionOperand::try_new(at_record(draft, group.members()[0].value)).unwrap()
}

/// `draft` moved to member record `record_index` with its nested records.
fn at_record(
    mut draft: crate::records::topology::entity_selection::DesignEntitySelectionOperandDraft,
    record_index: u32,
) -> crate::records::topology::entity_selection::DesignEntitySelectionOperandDraft {
    draft.record_index = record_index;
    draft.identity_record_index = record_index + 3;
    draft.next_record_index = record_index + 4;
    draft
}

fn assert_loft_sections(roles: [DesignOperandRole; 2], has_point: bool) {
    let (scope, groups) = loft_input(roles);
    let definition = crate::test_support::with_decode_context(|decode_ctx| {
        project(decode_ctx, &scope, &groups, &[])
    })
    .unwrap()
    .unwrap();
    assert!(matches!(definition,
        FeatureDefinition::Operation(FeatureOperation::Loft { sections, .. })
            if sections.len() == 2
                && sections.iter().any(|section| matches!(section, LoftSection::Point(_))) == has_point
    ));
}

#[test]
fn loft_projects_point_section() {
    assert_loft_sections(
        [DesignOperandRole::ROLE_0X43, DesignOperandRole::ROLE_0X5],
        true,
    );
}

#[test]
fn loft_projects_native_sections() {
    assert_loft_sections(
        [DesignOperandRole::ROLE_0X5, DesignOperandRole::ROLE_0X5],
        false,
    );
}

fn projected_sections(roles: [DesignOperandRole; 2]) -> Option<Vec<LoftSection>> {
    let (scope, groups) = loft_input(roles);
    let selections = groups
        .iter()
        .filter(|group| group.role() == DesignOperandRole::ROLE_0X21)
        .map(point_selection)
        .collect::<Vec<_>>();
    let definition = crate::test_support::with_decode_context(|decode_ctx| {
        project(decode_ctx, &scope, &groups, &selections)
    })
    .unwrap()?;
    let FeatureDefinition::Operation(FeatureOperation::Loft { sections, .. }) = definition else {
        panic!("Loft definition: {definition:?}");
    };
    Some(sections)
}

#[test]
fn a_profile_and_a_point_group_project_in_scope_reference_order() {
    use cadmpeg_ir::features::{LoftPointSection, PlanarProfileRef, ProfileRef};
    let profile_then_point =
        projected_sections([DesignOperandRole::PROFILE, DesignOperandRole::ROLE_0X21])
            .expect("profile-to-point Loft");
    assert!(matches!(
        profile_then_point.as_slice(),
        [
            LoftSection::Profile(ProfileRef::Planar(PlanarProfileRef::Native(profile))),
            LoftSection::Point(LoftPointSection::Native(point)),
        ] if profile.ends_with("#100") && point.as_str().ends_with("#101")
    ));
    let point_then_profile =
        projected_sections([DesignOperandRole::ROLE_0X21, DesignOperandRole::PROFILE])
            .expect("point-to-profile Loft");
    assert!(matches!(
        point_then_profile.as_slice(),
        [
            LoftSection::Point(LoftPointSection::Native(_)),
            LoftSection::Profile(_)
        ]
    ));
    for roles in [
        [DesignOperandRole::ROLE_0X43, DesignOperandRole::ROLE_0X21],
        [DesignOperandRole::ROLE_0X21, DesignOperandRole::ROLE_0X21],
    ] {
        assert!(projected_sections(roles).is_none(), "{roles:?}");
    }
    let (scope, [profile, mut point]) =
        loft_input([DesignOperandRole::PROFILE, DesignOperandRole::ROLE_0X21]);
    point
        .try_set_members(vec![
            crate::records::identity::Located {
                value: 201,
                offset: 1026,
            },
            crate::records::identity::Located {
                value: 202,
                offset: 1037,
            },
        ])
        .unwrap();
    let selection = point_selection(&point);
    assert!(crate::test_support::with_decode_context(|decode_ctx| {
        project(decode_ctx, &scope, &[profile, point], &[selection])
    })
    .unwrap()
    .is_none());
}

#[test]
fn a_profile_to_point_loft_needs_one_selection_of_its_point_member() {
    let (scope, [profile, point]) =
        loft_input([DesignOperandRole::PROFILE, DesignOperandRole::ROLE_0X21]);
    let selection = point_selection(&point);
    let mut second_member = selection.clone().into_draft();
    second_member.group_member_ordinal = 1;
    let other_record = at_record(selection.clone().into_draft(), selection.record_index() + 1);
    let projects = |selections: Vec<DesignEntitySelectionOperand>| {
        crate::test_support::with_decode_context(|decode_ctx| {
            project(
                decode_ctx,
                &scope,
                &[profile.clone(), point.clone()],
                &selections,
            )
        })
        .unwrap()
        .is_some()
    };
    assert!(projects(vec![selection.clone()]));
    for selections in [
        Vec::new(),
        vec![DesignEntitySelectionOperand::try_new(second_member).unwrap()],
        vec![DesignEntitySelectionOperand::try_new(other_record).unwrap()],
        vec![selection.clone(), selection],
    ] {
        assert!(!projects(selections));
    }
}

const LOFT_STREAM: &str = "f3d:Design/BulkStream.dat";

/// The one-member point group, its sketch-point selection, the native point,
/// and the planar sketch with its point entity.
fn loft_point_sources() -> (
    crate::records::topology::construction::DesignConstructionOperandGroup,
    crate::records::topology::entity_selection::DesignEntitySelectionOperand,
    crate::records::sketch_geometry::SketchPoint,
    cadmpeg_ir::sketches::Sketch,
    cadmpeg_ir::sketches::SketchEntity,
) {
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    let mut group = profile_group(101, 1);
    group.operand_role = DesignConstructionOperandRole::Other(DesignOperandRole::ROLE_0X21);
    let operand =
        crate::records::topology::entity_selection::DesignEntitySelectionOperand::try_new(
            crate::records::topology::entity_selection::DesignEntitySelectionOperandDraft {
                id: format!("{LOFT_STREAM}:design-entity-selection-operand#201"),
                scope_record_index: 12,
                group_record_index: 101,
                group_member_ordinal: 0,
                record_index: 201,
                byte_offset: 0,
                class_tag: "369".to_owned().try_into().unwrap(),
                serialization_form: Default::default(),
                asset_id: "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                asset_id_offset: 0,
                context_id: "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                context_id_offset: 0,
                identity_record_index: 204,
                identity_record_offset: 0,
                primary_identity: 265,
                primary_identity_offset: 21,
                secondary: Some(crate::records::identity::DesignSecondaryIdentity {
                    identity: crate::records::identity::Located {
                        value: 110,
                        offset: 29,
                    },
                    curve_identity: None,
                }),
                historical_edge_candidates: Vec::new(),
                historical_face_candidates: Vec::new(),
                resolved_edge_slot: None,
                next_record_index: 205,
                next_byte_offset: 37,
            },
        )
        .unwrap();
    let point = crate::records::sketch_geometry::SketchPoint::try_from(
        crate::records::sketch_geometry::SketchPointDraft {
            id: format!("{LOFT_STREAM}:sketch-point#300"),
            record_index: 300,
            owner_reference: Some(265),
            geometry_group: None,
            class_tag: "256".to_owned().try_into().unwrap(),
            byte_offset: 300,
            coordinate_offset: 141,
            companion: crate::records::sketch_geometry::SketchPointCompanion {
                incident_curves: Vec::new(),
            },
            record_form: crate::records::sketch_geometry::SketchPointRecordForm::version11(
                110,
                crate::records::sketch_geometry::SketchPointClosure::Selector0State0,
                Some(2),
                0.0,
            ),
            paired_reference: 0,
            coordinates: Point2::new(3.0, 4.0),
        },
    )
    .unwrap();
    let sketch_id = cadmpeg_ir::sketches::SketchId::mint("f3d:model:sketch#265").unwrap();
    let sketch = cadmpeg_ir::sketches::Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 20.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(0.0, 1.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(Vec::new()).unwrap(),
        native_ref: None,
    };
    let mut entity = cadmpeg_ir::sketches::SketchEntity::new(
        cadmpeg_ir::sketches::SketchEntityId::mint("f3d:model:sketch#265p110").unwrap(),
        sketch_id,
        cadmpeg_ir::sketches::SketchGeometry::try_from(
            cadmpeg_ir::sketches::SketchGeometryDefinition::Point {
                position: Point2::new(3.0, 4.0),
            },
        )
        .unwrap(),
    );
    entity.native_ref = Some(point.id.clone());
    (group, operand, point, sketch, entity)
}

/// A Loft from profile group 100 to point group `point_group`.
fn loft_to_point(point_group: &str) -> cadmpeg_ir::features::Feature {
    use cadmpeg_ir::features::{
        BooleanOp, Feature, FeatureEvaluation, FeatureId, LoftGuidance, LoftPointSection,
        PlanarProfileRef, ProfileRef,
    };
    Feature {
        product_definition: None,
        id: FeatureId::mint("test:model:feature#loft").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Loft".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::Loft {
                sections: vec![
                    LoftSection::Profile(ProfileRef::Planar(PlanarProfileRef::Native(format!(
                        "{LOFT_STREAM}:operand-group#100"
                    )))),
                    LoftSection::Point(LoftPointSection::Native(
                        cadmpeg_core::text::NonBlankString::new(point_group.to_owned()).unwrap(),
                    )),
                ],
                guidance: LoftGuidance::Guides(Vec::new()),
                op: BooleanOp::NewBody,
                closed: false,
                solid: true,
                ruled: false,
                linearize: false,
                max_degree: None,
                allow_multi_profile_faces: None,
            }),
            Default::default(),
        ),
        native_ref: None,
    }
}

fn bound_point_section(
    feature: cadmpeg_ir::features::Feature,
    sources: &crate::design::feature_project::LoftPointSources<'_>,
) -> cadmpeg_ir::features::LoftPointSection {
    let mut features = [feature];
    crate::design::feature_project::bind_loft_sketch_point_sections(&mut features, sources);
    let FeatureDefinition::Operation(FeatureOperation::Loft { sections, .. }) =
        features[0].evaluation.definition()
    else {
        panic!("Loft definition");
    };
    let LoftSection::Point(point) = &sections[1] else {
        panic!("point section");
    };
    point.clone()
}

#[test]
fn a_loft_point_section_solves_to_its_sketch_point_in_model_space() {
    use cadmpeg_ir::features::LoftPointSection;
    let (group, operand, point, sketch, entity) = loft_point_sources();
    let group_id = group.id.clone();
    let sources = crate::design::feature_project::LoftPointSources {
        groups: std::slice::from_ref(&group),
        entity_operands: std::slice::from_ref(&operand),
        sketch_points: std::slice::from_ref(&point),
        sketches: std::slice::from_ref(&sketch),
        sketch_entities: std::slice::from_ref(&entity),
    };
    let LoftPointSection::Point(solved) = bound_point_section(loft_to_point(&group_id), &sources)
    else {
        panic!("solved point section");
    };
    assert_eq!(solved.get(), cadmpeg_ir::math::Point3::new(-4.0, 3.0, 20.0));
}

#[test]
fn a_loft_point_section_without_one_sketch_point_stays_native() {
    use cadmpeg_ir::features::LoftPointSection;
    let (group, operand, point, sketch, entity) = loft_point_sources();
    let group_id = group.id.clone();
    let mut other_owner = point.clone();
    other_owner.owner_reference = Some(266);
    let mut twin = point.clone();
    twin.id = format!("{LOFT_STREAM}:sketch-point#301");
    let mut two_members = group.clone();
    two_members
        .try_set_members(vec![
            crate::records::identity::Located {
                value: 201,
                offset: 1026,
            },
            crate::records::identity::Located {
                value: 202,
                offset: 1037,
            },
        ])
        .unwrap();
    for points in [vec![other_owner], vec![point.clone(), twin]] {
        let sources = crate::design::feature_project::LoftPointSources {
            groups: std::slice::from_ref(&group),
            entity_operands: std::slice::from_ref(&operand),
            sketch_points: &points,
            sketches: std::slice::from_ref(&sketch),
            sketch_entities: std::slice::from_ref(&entity),
        };
        assert!(matches!(
            bound_point_section(loft_to_point(&group_id), &sources),
            LoftPointSection::Native(_)
        ));
    }
    let mut second_member = operand.clone();
    second_member.group_member_ordinal = 1;
    let sources = crate::design::feature_project::LoftPointSources {
        groups: std::slice::from_ref(&group),
        entity_operands: std::slice::from_ref(&second_member),
        sketch_points: std::slice::from_ref(&point),
        sketches: std::slice::from_ref(&sketch),
        sketch_entities: std::slice::from_ref(&entity),
    };
    assert!(matches!(
        bound_point_section(loft_to_point(&group_id), &sources),
        LoftPointSection::Native(_)
    ));
    let sources = crate::design::feature_project::LoftPointSources {
        groups: std::slice::from_ref(&two_members),
        entity_operands: std::slice::from_ref(&operand),
        sketch_points: std::slice::from_ref(&point),
        sketches: std::slice::from_ref(&sketch),
        sketch_entities: std::slice::from_ref(&entity),
    };
    assert!(matches!(
        bound_point_section(loft_to_point(&group_id), &sources),
        LoftPointSection::Native(_)
    ));
}

#[test]
fn a_loft_point_section_needs_a_sketch_point_frame() {
    use crate::layout::marked_sketch_curve_identity as curve_identity;
    use cadmpeg_ir::features::LoftPointSection;
    let (group, operand, point, sketch, entity) = loft_point_sources();
    let group_id = group.id.clone();
    let mut pair = operand.clone().into_draft();
    pair.primary_identity_offset = 29;
    if let Some(secondary) = pair.secondary.as_mut() {
        secondary.identity.offset = 37;
    }
    pair.next_byte_offset = 45;
    let mut curve = operand.clone().into_draft();
    curve.class_tag = "338".to_owned().try_into().unwrap();
    curve.primary_identity_offset = u64::try_from(curve_identity::OWNER_RECORD_INDEX).unwrap();
    if let Some(secondary) = curve.secondary.as_mut() {
        secondary.identity.offset = u64::try_from(curve_identity::CURVE_PERSISTENT_ID).unwrap();
    }
    curve.next_byte_offset = u64::try_from(curve_identity::LEN).unwrap();
    for draft in [pair, curve] {
        let colliding =
            crate::records::topology::entity_selection::DesignEntitySelectionOperand::try_new(
                draft,
            )
            .unwrap();
        assert_eq!(colliding.primary_identity, 265);
        assert_eq!(colliding.secondary().unwrap().identity.value, 110);
        let sources = crate::design::feature_project::LoftPointSources {
            groups: std::slice::from_ref(&group),
            entity_operands: std::slice::from_ref(&colliding),
            sketch_points: std::slice::from_ref(&point),
            sketches: std::slice::from_ref(&sketch),
            sketch_entities: std::slice::from_ref(&entity),
        };
        assert!(matches!(
            bound_point_section(loft_to_point(&group_id), &sources),
            LoftPointSection::Native(_)
        ));
    }
}

#[test]
fn a_loft_point_section_needs_the_selection_of_its_point_member() {
    use cadmpeg_ir::features::LoftPointSection;
    let (group, operand, point, sketch, entity) = loft_point_sources();
    let group_id = group.id.clone();
    let mut section_group = group.clone();
    section_group.operand_role = DesignConstructionOperandRole::Other(DesignOperandRole::ROLE_0X5);
    let other_record = DesignEntitySelectionOperand::try_new(at_record(
        operand.clone().into_draft(),
        operand.record_index() + 1,
    ))
    .unwrap();
    for (group, operand) in [(&group, &other_record), (&section_group, &operand)] {
        let sources = crate::design::feature_project::LoftPointSources {
            groups: std::slice::from_ref(group),
            entity_operands: std::slice::from_ref(operand),
            sketch_points: std::slice::from_ref(&point),
            sketches: std::slice::from_ref(&sketch),
            sketch_entities: std::slice::from_ref(&entity),
        };
        assert!(matches!(
            bound_point_section(loft_to_point(&group_id), &sources),
            LoftPointSection::Native(_)
        ));
    }
}
