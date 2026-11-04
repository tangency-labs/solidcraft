// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::direct_face::{DesignDraftFlags, DesignDraftOperation};
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::identity::ReferenceRun;
use crate::records::mesh::DesignRelaxedGuidText;
use crate::records::references::DesignClassTag;
use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::entity_selection::{
    DesignEntitySelectionFaceCandidate, DesignEntitySelectionOperand,
    DesignEntitySelectionOperandDraft,
};
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::records::topology::fillet::HistoricalBinding;
use cadmpeg_ir::features::{DraftAnchor, FaceSelection, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::math::Vector3;

const STREAM: &str = "f3d:test/Design/BulkStream.dat";

fn draft_scope(angle: f64, flipped: Option<bool>) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#100"),
        DesignFeatureKind::Draft,
        100,
    );
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
            angle: cadmpeg_ir::scalar::Angle::new(angle).unwrap(),
            angle_record_index: 90,
            angle_offset: 0,
            opposite_angle_record_index: 91,
            opposite_angle_offset: 0,
            flags: flipped.map(|flipped| DesignDraftFlags {
                flipped,
                tangent_chain: false,
            }),
        });
    }
    scope
}

fn group(
    record_index: u32,
    member: u32,
    role: DesignOperandRole,
) -> DesignConstructionOperandGroup {
    serde_json::from_value(serde_json::json!({
        "id": format!("{STREAM}:design-construction-operand-group#{record_index}"),
        "scope_record_index": 100,
        "scope_reference_ordinal": 0,
        "record_index": record_index,
        "byte_offset": 1000,
        "class_tag": "332",
        "members": [member],
        "member_offsets": [1026],
        "frame": {
            "member_count_offset": 1021,
            "opaque_index": 1,
            "opaque_index_offset": 1072,
            "opaque_scalar": 0.0,
            "opaque_scalar_offset": 1076,
            "variant": false
        },
        "role": role.raw(),
        "role_offset": 1054,
        "paired_class_tag": "259",
        "paired_byte_offset": 1125
    }))
    .unwrap()
}

fn groups() -> [DesignConstructionOperandGroup; 2] {
    [
        group(101, 111, DesignOperandRole::ROLE_0X10),
        group(102, 112, DesignOperandRole::ROLE_0X21),
    ]
}

fn selection(primary_identity: u64, face_slots: &[i64]) -> DesignEntitySelectionOperand {
    DesignEntitySelectionOperand::try_new(DesignEntitySelectionOperandDraft {
        id: format!("{STREAM}:design-entity-selection-operand#112"),
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
        primary_identity,
        primary_identity_offset: 21,
        secondary: None,
        historical_edge_candidates: Vec::new(),
        historical_face_candidates: face_slots
            .iter()
            .map(|face_slot| DesignEntitySelectionFaceCandidate {
                history_id: "history".into(),
                historical: HistoricalBinding {
                    kind: AsmHistoricalEntityKind::Coedge,
                    entity_ref: i64::try_from(primary_identity).unwrap(),
                    state_ids: vec![7, 6],
                },
                face_slot: *face_slot,
            })
            .collect(),
        resolved_edge_slot: None,
        next_record_index: 114,
        next_byte_offset: 29,
    })
    .unwrap()
}

struct Projected {
    anchor: DraftAnchor,
    outward: Option<bool>,
}

fn project(
    scope: &DesignParameterScope,
    selections: &[DesignEntitySelectionOperand],
) -> Option<Projected> {
    let definition = super::super::project_draft(
        scope,
        std::slice::from_ref(scope),
        &groups(),
        selections,
        &[],
        &[],
    )
    .unwrap()?;
    let FeatureDefinition::Operation(FeatureOperation::Draft {
        anchor, outward, ..
    }) = definition
    else {
        panic!("expected a Draft");
    };
    Some(Projected { anchor, outward })
}

/// A flipped axis-aligned pull keeps `+0.0` in its zero components.
fn assert_zero_components_are_unsigned(direction: Vector3) {
    for component in [direction.x, direction.y, direction.z] {
        assert!(
            component != 0.0 || !component.is_sign_negative(),
            "{direction:?}"
        );
    }
}

fn pull_direction(anchor: &DraftAnchor) -> Option<Vector3> {
    let DraftAnchor::NeutralPlane { pull, .. } = anchor else {
        panic!("expected a neutral-plane anchor");
    };
    pull.as_ref().map(|pull| *pull.direction)
}

#[test]
fn an_origin_plane_states_its_normal_as_the_pull_direction() {
    for (identity, normal) in [
        (42, Vector3::new(0.0, 0.0, 1.0)),
        (43, Vector3::new(0.0, 1.0, 0.0)),
        (44, Vector3::new(1.0, 0.0, 0.0)),
    ] {
        let projected =
            project(&draft_scope(0.25, Some(false)), &[selection(identity, &[])]).unwrap();
        assert_eq!(pull_direction(&projected.anchor), Some(normal));
        assert_eq!(projected.outward, Some(false));
        let DraftAnchor::NeutralPlane { plane, pull } = &projected.anchor else {
            panic!("expected a neutral-plane anchor");
        };
        assert_eq!(plane, &FaceSelection::Native(groups()[1].id.clone()));
        assert_eq!(pull.as_ref().unwrap().plane, None);
    }
}

#[test]
fn a_flip_reverses_a_stated_pull_and_keeps_the_angle_sense() {
    for (identity, flipped_normal) in [
        (42, Vector3::new(0.0, 0.0, -1.0)),
        (43, Vector3::new(0.0, -1.0, 0.0)),
        (44, Vector3::new(-1.0, 0.0, 0.0)),
    ] {
        let projected =
            project(&draft_scope(0.25, Some(true)), &[selection(identity, &[])]).unwrap();
        let pull = pull_direction(&projected.anchor).unwrap();
        assert_eq!(pull, flipped_normal);
        assert_zero_components_are_unsigned(pull);
        assert_eq!(projected.outward, Some(false));
    }

    let negative = project(&draft_scope(-0.25, Some(true)), &[selection(42, &[])]).unwrap();
    assert_eq!(negative.outward, Some(true));
}

#[test]
fn an_origin_identity_is_not_the_asm_entity_with_the_same_number() {
    for face_slots in [&[158][..], &[158, 159][..]] {
        let projected = project(
            &draft_scope(0.25, Some(false)),
            &[selection(42, face_slots)],
        )
        .unwrap();
        assert_eq!(
            pull_direction(&projected.anchor),
            Some(Vector3::new(0.0, 0.0, 1.0))
        );
    }
}

#[test]
fn a_flip_inverts_outward_when_no_pull_is_stated() {
    for (angle, flipped, expected) in [
        (0.25, Some(false), false),
        (0.25, Some(true), true),
        (-0.25, Some(false), true),
        (-0.25, Some(true), false),
        (0.25, None, false),
    ] {
        let projected = project(&draft_scope(angle, flipped), &[]).unwrap();
        assert_eq!(pull_direction(&projected.anchor), None);
        assert_eq!(projected.outward, Some(expected), "{angle} {flipped:?}");
    }
}

#[test]
fn an_identity_outside_the_origin_planes_keeps_the_historical_rule() {
    let mut scope = draft_scope(0.25, Some(false));
    scope
        .try_edit(|draft| {
            draft.previous_history_state_id = Some(7);
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert!(project(&scope, &[selection(45, &[])]).is_none());
    assert!(project(&scope, &[selection(41, &[])]).is_none());
}

fn work_plane_scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#283"),
        DesignFeatureKind::WorkPlane,
        283,
    );
    if let DesignScopePayloadMut::WorkPlane(slot) = scope.payload_mut() {
        *slot = Some(crate::records::feature::scope::DesignWorkPlaneTransform {
            work_plane_transform: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.5],
                [0.0, 0.0, 0.0, 1.0],
            ]
            .try_into()
            .unwrap(),
            work_plane_transform_offset: 0,
            reference: None,
            work_plane_construction: None,
        });
    }
    scope
}

#[test]
fn a_flip_reverses_the_pull_of_a_work_plane() {
    let plane = work_plane_scope();
    for (flipped, z) in [(false, 1.0), (true, -1.0)] {
        let scope = draft_scope(0.25, Some(flipped));
        let definition = super::super::project_draft(
            &scope,
            &[scope.clone(), plane.clone()],
            &groups(),
            &[selection(282, &[])],
            &[],
            &[],
        )
        .unwrap()
        .unwrap();
        let FeatureDefinition::Operation(FeatureOperation::Draft {
            anchor, outward, ..
        }) = definition
        else {
            panic!("expected a Draft");
        };
        let pull = pull_direction(&anchor).unwrap();
        assert_eq!(pull, Vector3::new(0.0, 0.0, z));
        assert_zero_components_are_unsigned(pull);
        assert_eq!(outward, Some(false));
    }
}

#[test]
fn a_flip_reverses_the_pull_of_a_parting_line() {
    let plane = work_plane_scope();
    let mut scope = draft_scope(0.25, Some(true));
    scope
        .try_edit(|draft| {
            draft.reference_members = ReferenceRun::unlocated(vec![101, 111, 102, 112, 103, 113]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let [faces, neutral] = groups();
    let tool = group(103, 113, DesignOperandRole::ROLE_0X21);
    let definition = super::super::project_draft(
        &scope,
        &[scope.clone(), plane],
        &[faces, neutral, tool],
        &[selection(282, &[])],
        &[],
        &[],
    )
    .unwrap()
    .unwrap();
    let FeatureDefinition::Operation(FeatureOperation::Draft {
        anchor: DraftAnchor::PartingLine { pull, .. },
        outward,
        ..
    }) = definition
    else {
        panic!("expected a parting-line Draft");
    };
    assert_eq!(*pull.direction, Vector3::new(0.0, 0.0, -1.0));
    assert_eq!(outward, Some(false));
}
