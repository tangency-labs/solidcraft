// SPDX-License-Identifier: Apache-2.0
use crate::test_support::native_test::f3d_native_mut;

#[test]
fn only_a_face_recipe_kind_states_a_program_operand_length() {
    use crate::records::recipes::ConstructionRecipeKind;

    assert_eq!(
        crate::validate::recipe_program_operand_length(ConstructionRecipeKind::Face),
        Some(16)
    );
    assert_eq!(
        crate::validate::recipe_program_operand_length(ConstructionRecipeKind::BoundedFace),
        Some(24)
    );
    for kind in [
        ConstructionRecipeKind::Body,
        ConstructionRecipeKind::Edge,
        ConstructionRecipeKind::Vertex,
    ] {
        assert_eq!(crate::validate::recipe_program_operand_length(kind), None);
    }
}

/// The face-operand route refuses a recipe kind that states no face operand.
///
/// `validate_face_operands` reads the operand length through
/// `recipe_program_operand_length`, so a `Body`, `Edge` or `Vertex` recipe
/// states no program offset and the operand is invalid. The `Face` fixture is
/// the control: it differs only in the recipe kind and the operand length that
/// kind states, and it raises no finding.
#[test]
fn a_face_operand_whose_recipe_kind_states_no_face_operand_is_refused() {
    use crate::records::recipes::ConstructionRecipeKind;

    let admitted = face_findings(ConstructionRecipeKind::Face, 16, 1_047);
    assert!(admitted.is_empty(), "{admitted:#?}");
    assert_eq!(
        face_findings(ConstructionRecipeKind::Body, 16, 1_047).len(),
        1
    );
}

fn face_findings(
    kind: crate::records::recipes::ConstructionRecipeKind,
    operand_length: u64,
    recipe_byte_offset: u64,
) -> Vec<cadmpeg_ir::report::check::Finding> {
    scope_face_findings(
        crate::records::feature::scope::DesignFeatureKind::OffsetFaces,
        kind,
        operand_length,
        recipe_byte_offset,
    )
}

fn scope_face_findings(
    scope_kind: crate::records::feature::scope::DesignFeatureKind,
    kind: crate::records::recipes::ConstructionRecipeKind,
    operand_length: u64,
    recipe_byte_offset: u64,
) -> Vec<cadmpeg_ir::report::check::Finding> {
    operand_findings(
        scope_kind,
        kind,
        operand_length,
        recipe_byte_offset,
        None,
        None,
        0,
    )
}

/// A construction-operand group with record index 1 and sole member 100 at
/// scope reference `ordinal`.
fn face_operand_group(
    role: crate::records::topology::extrude_selection::DesignOperandRole,
    ordinal: u32,
) -> crate::records::topology::construction::DesignConstructionOperandGroup {
    use crate::records::{
        identity::Located,
        topology::construction::{
            DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
            DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
            DesignConstructionOperandRole,
        },
    };
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: "f3d:Design/BulkStream.dat:design-construction-operand-group#1".into(),
        scope_record_index: 10,
        scope_reference_ordinal: ordinal,
        record_index: 1,
        byte_offset: 800,
        class_tag: "277".to_owned().try_into().unwrap(),
        members: vec![Located {
            value: 100,
            offset: 826,
        }],
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame::try_from(
            DesignConstructionOperandGroupFrameDraft {
                member_count_offset: 821,
                auxiliary_records: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_records: Vec::new(),
                trailing_transforms: Vec::new(),
                trailing_dual_transforms: Vec::new(),
                trailing_flags: Vec::new(),
                opaque_index: 1,
                opaque_index_offset: 858,
                opaque_scalar: 0.0,
                opaque_scalar_offset: 862,
                variant: false,
                closing_flag: false,
            },
        )
        .unwrap(),
        operand_role: DesignConstructionOperandRole::Other(role),
        role_offset: 840,
        paired_class_tag: "258".to_owned().try_into().unwrap(),
        paired_byte_offset: 880,
    })
    .unwrap()
}

/// The type registration of operand record 100 under `guid`.
fn operand_record_type(guid: &str) -> crate::records::entity_header::SegmentType {
    crate::records::entity_header::SegmentType::try_new(
        "f3d:Design/MetaStream.dat:design-type#0".to_owned(),
        crate::records::entity_header::SegmentTypeData {
            byte_offset: 0,
            type_guid: guid.to_owned().try_into().unwrap(),
            type_guid_offset: 0,
            base_type_guid: crate::records::entity_header::BaseTypeGuid::Absent,
            version: 3,
            version_offset: 0,
            module: "Fusion".into(),
            entities: crate::records::identity::ReferenceRun::unlocated(vec![100]),
        },
    )
    .unwrap()
}

/// The fixture IR of one face operand, record 100, and the operand id.
/// With `group_role`, the operand is the sole member of a group of that role at
/// scope reference zero. Without it, the operand is direct scope reference two.
/// With `next_reference_at`, the operand is scope reference one and the header
/// of scope reference two, record 2, starts at that offset. With
/// `group_ordinal` one, the group is scope reference one.
fn operand_ir(
    scope_kind: crate::records::feature::scope::DesignFeatureKind,
    kind: crate::records::recipes::ConstructionRecipeKind,
    operand_length: u64,
    recipe_byte_offset: u64,
    group_role: Option<crate::records::topology::extrude_selection::DesignOperandRole>,
    next_reference_at: Option<u64>,
    group_ordinal: u32,
) -> (cadmpeg_ir::CadIr, String) {
    use crate::records::{
        decal::DesignRecordHeader,
        feature::scope::DesignParameterScope,
        recipes::ConstructionRecipe,
        references::DesignClassTag,
        topology::{
            body_recipe::DesignOperandGroup, face::DesignFaceOperand, face::DesignFaceOperandDraft,
        },
    };

    let stream = "f3d:Design/BulkStream.dat";
    let operand_id = format!("{stream}:design-face-operand#100");
    let recipe_id = format!("{stream}:construction-recipe#0");

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let mut scope = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#10"),
        scope_kind,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(if next_reference_at.is_some() {
                    vec![1, 100, 2]
                } else if group_ordinal == 1 {
                    vec![2, 1, 100]
                } else {
                    vec![1, 2, 100]
                });
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let class_tag = DesignClassTag::try_from("365".to_owned()).unwrap();
    let recipe_program_offset = recipe_byte_offset.saturating_add(operand_length);
    let operand = DesignFaceOperand::try_new(DesignFaceOperandDraft {
        id: operand_id.clone(),
        scope_record_index: 10,
        scope_reference_ordinal: if group_role.is_some() {
            group_ordinal
        } else {
            2
        },
        group: group_role.map(|_| DesignOperandGroup {
            group_record_index: 1,
            group_member_ordinal: 0,
        }),
        record_index: 100,
        byte_offset: recipe_byte_offset - 47,
        class_tag: class_tag.clone(),
        paired_byte_offset: recipe_byte_offset - 31,
        paired_class_tag: DesignClassTag::try_from("366".to_owned()).unwrap(),
        recipe_record_index: 103,
        recipe_record_byte_offset: recipe_byte_offset - 15,
        recipe_id: recipe_id.clone(),
        recipe_prefix_offset: recipe_byte_offset - 4,
        recipe_prefix_bytes: Vec::new(),
        recipe_references: Vec::new(),
        recipe_kind: kind,
        recipe_program_offset,
        recipe_program: vec![0, -1],
        recipe_nodes: Vec::new(),
        candidate_faces: Vec::new(),
        unreferenced_candidate_faces: Vec::new(),
        alternate_selector_candidate_faces: Vec::new(),
        preceding_candidate_faces: Vec::new(),
        changed_candidate_faces: Vec::new(),
        historical_support_contexts: Vec::new(),
        resolved_face_slots: Vec::new(),
        resolved_active_face: None,
        next_record_index: 105,
        next_byte_offset: recipe_program_offset.saturating_add(8),
    })
    .unwrap();
    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_parameter_scopes = vec![scope];
        native.design_record_headers = vec![DesignRecordHeader {
            id: format!("{stream}:design-record-header#100"),
            record_index: 100,
            class_tag: class_tag.clone(),
            byte_offset: recipe_byte_offset - 47,
        }];
        native
            .design_record_headers
            .extend(next_reference_at.map(|byte_offset| DesignRecordHeader {
                id: format!("{stream}:design-record-header#2"),
                record_index: 2,
                class_tag,
                byte_offset,
            }));
        native.construction_recipes = vec![ConstructionRecipe {
            id: recipe_id.clone(),
            byte_offset: recipe_byte_offset,
            kind,
            design: None,
            recipe_index: 0,
            record_index: None,
        }];
        native.design_face_operands = vec![operand];
        native.design_construction_operand_groups = group_role
            .map(|role| face_operand_group(role, group_ordinal))
            .into_iter()
            .collect();
    }
    (ir, operand_id)
}

/// The invalid-frame findings of the operand `operand_id` in `ir`.
fn frame_findings(
    (ir, operand_id): &(cadmpeg_ir::CadIr, String),
) -> Vec<cadmpeg_ir::report::check::Finding> {
    crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, ir).expect("validate the native namespace")
    })
    .into_iter()
    .filter(|finding| {
        finding.message == "Fusion Design face operand has an invalid scope or recipe frame"
            && finding.entity.as_deref() == Some(operand_id.as_str())
    })
    .collect::<Vec<_>>()
}

/// The invalid-frame findings of the `operand_ir` fixture.
fn operand_findings(
    scope_kind: crate::records::feature::scope::DesignFeatureKind,
    kind: crate::records::recipes::ConstructionRecipeKind,
    operand_length: u64,
    recipe_byte_offset: u64,
    group_role: Option<crate::records::topology::extrude_selection::DesignOperandRole>,
    next_reference_at: Option<u64>,
    group_ordinal: u32,
) -> Vec<cadmpeg_ir::report::check::Finding> {
    frame_findings(&operand_ir(
        scope_kind,
        kind,
        operand_length,
        recipe_byte_offset,
        group_role,
        next_reference_at,
        group_ordinal,
    ))
}

/// A direct `Hole` face operand selects its support face through a face or a
/// bounded-face recipe.
#[test]
fn hole_face_operand_accepts_face_and_bounded_face_recipes() {
    use crate::records::feature::scope::DesignFeatureKind;
    use crate::records::recipes::ConstructionRecipeKind;

    let face = scope_face_findings(
        DesignFeatureKind::Hole,
        ConstructionRecipeKind::Face,
        16,
        1_047,
    );
    assert!(face.is_empty(), "{face:#?}");
    let bounded = scope_face_findings(
        DesignFeatureKind::Hole,
        ConstructionRecipeKind::BoundedFace,
        24,
        1_047,
    );
    assert!(bounded.is_empty(), "{bounded:#?}");
    assert_eq!(
        scope_face_findings(
            DesignFeatureKind::Hole,
            ConstructionRecipeKind::Body,
            16,
            1_047
        )
        .len(),
        1
    );
}

/// A `MoveFace` face operand is a bounded-face member of its selected-face
/// group.
#[test]
fn move_face_operand_requires_a_bounded_face_member_of_its_face_group() {
    use crate::records::feature::scope::DesignFeatureKind;
    use crate::records::recipes::ConstructionRecipeKind;
    use crate::records::topology::extrude_selection::DesignOperandRole;

    let scope_kind = DesignFeatureKind::MoveFace;
    let findings = |kind, operand_length, group_role| {
        operand_findings(
            scope_kind.clone(),
            kind,
            operand_length,
            1_047,
            group_role,
            None,
            0,
        )
    };
    let bounded = ConstructionRecipeKind::BoundedFace;
    let admitted = findings(bounded, 24, Some(DesignOperandRole::ROLE_0X10));
    assert!(admitted.is_empty(), "{admitted:#?}");
    assert_eq!(
        findings(
            ConstructionRecipeKind::Face,
            16,
            Some(DesignOperandRole::ROLE_0X10)
        )
        .len(),
        1
    );
    assert_eq!(
        findings(bounded, 24, Some(DesignOperandRole::BODIES_A)).len(),
        1
    );
    assert_eq!(findings(bounded, 24, None).len(), 1);
}

#[test]
fn face_operand_rejects_overflowed_program_origin() {
    let findings = face_findings(
        crate::records::recipes::ConstructionRecipeKind::Face,
        16,
        u64::MAX - 15,
    );
    assert_eq!(findings.len(), 1);
}

#[test]
fn face_operand_rejects_overflowed_program_extent() {
    let findings = face_findings(
        crate::records::recipes::ConstructionRecipeKind::Face,
        16,
        u64::MAX - 20,
    );
    assert_eq!(findings.len(), 1);
}

/// The header of the scope reference after a group's final face member bounds
/// that member's frame.
#[test]
fn a_final_group_member_frame_ends_at_the_next_scope_reference() {
    use crate::records::feature::scope::DesignFeatureKind;
    use crate::records::recipes::ConstructionRecipeKind;
    use crate::records::topology::extrude_selection::DesignOperandRole;

    let findings = |next_reference_at| {
        operand_findings(
            DesignFeatureKind::SplitFace,
            ConstructionRecipeKind::BoundedFace,
            24,
            1_047,
            Some(DesignOperandRole::ROLE_0X10),
            Some(next_reference_at),
            0,
        )
    };
    let frame_end = 1_047 + 24 + 8;
    let closed = findings(frame_end);
    assert!(closed.is_empty(), "{closed:#?}");
    assert_eq!(findings(frame_end - 1).len(), 1);
    let earlier = findings(900);
    assert!(earlier.is_empty(), "{earlier:#?}");
}

/// A rectangular, circular, or Mirror pattern face seed group at scope
/// reference one holds bounded-face operands.
#[test]
fn a_pattern_face_seed_group_holds_bounded_face_operands() {
    use crate::records::feature::scope::DesignFeatureKind;
    use crate::records::recipes::ConstructionRecipeKind;
    use crate::records::topology::extrude_selection::DesignOperandRole;

    for scope_kind in [
        DesignFeatureKind::RPattern,
        DesignFeatureKind::CPattern,
        DesignFeatureKind::Mirror,
    ] {
        let findings = |kind, operand_length, ordinal| {
            operand_findings(
                scope_kind.clone(),
                kind,
                operand_length,
                1_047,
                Some(DesignOperandRole::ROLE_0X10),
                None,
                ordinal,
            )
        };
        let admitted = findings(ConstructionRecipeKind::BoundedFace, 24, 1);
        assert!(admitted.is_empty(), "{admitted:#?}");
        assert_eq!(findings(ConstructionRecipeKind::Face, 16, 1).len(), 1);
        assert_eq!(
            findings(ConstructionRecipeKind::BoundedFace, 24, 0).len(),
            1
        );
    }
}

/// A `Sweep` profile-group face operand is a bounded-face member whose record
/// is a topology-recipe selection.
#[test]
fn sweep_profile_face_operand_requires_a_topology_recipe_member() {
    use crate::records::feature::scope::DesignFeatureKind;
    use crate::records::recipes::ConstructionRecipeKind;
    use crate::records::topology::extrude_selection::DesignOperandRole;

    let findings = |member_type: Option<&str>| {
        let mut fixture = operand_ir(
            DesignFeatureKind::Sweep,
            ConstructionRecipeKind::BoundedFace,
            24,
            1_047,
            Some(DesignOperandRole::PROFILE),
            None,
            0,
        );
        f3d_native_mut(&mut fixture.0).design_types =
            member_type.map(operand_record_type).into_iter().collect();
        frame_findings(&fixture)
    };
    let admitted = findings(Some(
        crate::records::feature::path_features::pipe::RECIPE_GUID,
    ));
    assert!(admitted.is_empty(), "{admitted:#?}");
    assert_eq!(
        findings(Some("4BD53E5A-0B3E-45E5-AE8B-02044306485A")).len(),
        1
    );
    assert_eq!(findings(None).len(), 1);
}
