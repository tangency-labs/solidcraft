// SPDX-License-Identifier: Apache-2.0
//! The fallback face of an Extrude bounded-face operand and its recipe lanes.

use std::collections::HashMap;

use cadmpeg_core::decode::u64_from_index;
use cadmpeg_ir::ids::FaceId;
use cadmpeg_ir::report::loss::LossNote;

use crate::history::{bind_face_operand_history_candidates, BindingHistories};
use crate::history_records::{
    AsmDeltaState, AsmEntityVersion, AsmHistoricalPersistentSubentityTag, AsmHistory,
};
use crate::loss::F3dLossCode;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
use crate::records::topology::face::DesignFaceOperand;
use crate::test_support::history::{bounded_face_operand, box_topology, recipe_operand_reference};

const OPERAND: &str = "f3d:Design/BulkStream.dat:design-face-operand#110";

fn face(slot: i64) -> FaceId {
    FaceId::mint(format!("f3d:brep:entity#{slot}")).unwrap()
}

fn scope(kind: DesignFeatureKind) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#42",
        kind,
        42,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(2);
            draft.previous_history_state_id = Some(1);
            draft.reference_members = {
                let values: Vec<u32> = vec![100, 110];
                let offsets = (0..values.len())
                    .map(|ordinal| 14 + 11 * u64_from_index(ordinal))
                    .collect();
                crate::records::identity::ReferenceRun::from_columns(
                    values,
                    offsets,
                    "reference_members",
                )
                .unwrap()
            };
            draft.reference_count_offset = *draft.reference_members.offsets().next().unwrap() - 5;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

/// The token that names a lane's faces in the fixture topology.
fn token(faces: &[i64]) -> String {
    faces
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("-")
}

/// A bounded-face operand with one recipe operand per lane whose only
/// unreferenced candidate is the bottom face 40 of the box.
fn operand(lanes: &[&[i64]]) -> DesignFaceOperand {
    let mut operand = bounded_face_operand(
        lanes
            .iter()
            .zip(0..)
            .map(|(faces, ordinal)| recipe_operand_reference(ordinal, &token(faces), 300, faces))
            .collect(),
    );
    operand.candidate_faces = [10, 20, 40].into_iter().map(face).collect();
    operand.unreferenced_candidate_faces = vec![face(40)];
    operand
}

/// Two box states whose faces carry the tags that bind `lanes`; the second
/// state revises the records of the `updated` faces.
fn history(lanes: &[&[i64]], updated: &[i64]) -> AsmHistory {
    let mut topology = box_topology();
    for faces in lanes {
        for face in *faces {
            topology
                .persistent_subentity_tags
                .push(AsmHistoricalPersistentSubentityTag {
                    entity_kind: AsmHistoricalEntityKind::Face,
                    entity_ref: *face,
                    selector: 1,
                    token: token(faces),
                    design_references: vec![300],
                    ordinal: 0,
                });
        }
    }
    topology
        .persistent_subentity_tags
        .sort_by_key(|tag| tag.entity_ref);
    let versions = |state_id: i64| -> Vec<AsmEntityVersion> {
        [10, 20, 30, 40, 50]
            .into_iter()
            .map(|entity_ref| AsmEntityVersion {
                entity_ref,
                record_ref: if state_id == 2 && updated.contains(&entity_ref) {
                    entity_ref + 1000
                } else {
                    entity_ref
                },
            })
            .collect()
    };
    let state = |state_id, next_ref| {
        (
            AsmDeltaState {
                id: format!("f3d:history:state#{state_id}"),
                parent: "f3d:history".into(),
                byte_offset: 0,
                state_id,
                version_flag: 1,
                state_flag: 0,
                previous_ref: None,
                next_ref,
                node_index: state_id,
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: Vec::new(),
                records: Vec::new(),
                transition: None,
            },
            Some(topology.clone()),
            versions(state_id),
        )
    };
    crate::test_support::history::history(
        "f3d:history".into(),
        0,
        None,
        false,
        vec![state(1, None), state(2, Some(1))],
    )
}

fn bind(kind: DesignFeatureKind, lanes: &[&[i64]]) -> (Vec<i64>, Vec<LossNote>) {
    let mut operands = vec![operand(lanes)];
    let history = history(lanes, &[]);
    let losses = crate::test_support::with_decode_context(|ctx| {
        bind_face_operand_history_candidates(
            ctx,
            &mut operands,
            &[scope(kind)],
            &[],
            &[],
            BindingHistories::Shared(std::slice::from_ref(&history)),
            &HashMap::new(),
        )
    })
    .unwrap();
    (operands[0].resolved_face_slots.clone(), losses)
}

#[test]
fn an_extrude_fallback_face_apart_from_a_recipe_operand_is_dropped() {
    let (slots, losses) = bind(DesignFeatureKind::Extrude, &[&[20], &[10]]);
    assert!(slots.is_empty());
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        F3dLossCode::FeatureFaceOperandFallbackContradicted.kind()
    );
    assert!(losses[0].message.contains(OPERAND));
}

#[test]
fn an_extrude_fallback_face_that_touches_every_lane_is_kept() {
    let (slots, losses) = bind(DesignFeatureKind::Extrude, &[&[20], &[30]]);
    assert_eq!(slots, [40]);
    assert!(losses.is_empty());
}

#[test]
fn a_fallback_face_outside_an_extrude_is_kept() {
    let (slots, losses) = bind(DesignFeatureKind::Draft, &[&[20], &[10]]);
    assert_eq!(slots, [40]);
    assert!(losses.is_empty());
}

#[test]
fn a_face_recipe_fallback_is_kept() {
    // A face recipe takes its candidates from its references: 40 and 10.
    let lanes: &[&[i64]] = &[&[40], &[10]];
    let mut operands = vec![operand(lanes)];
    operands[0].recipe_kind = crate::records::recipes::ConstructionRecipeKind::Face;
    let history = history(lanes, &[40]);
    let losses = crate::test_support::with_decode_context(|ctx| {
        bind_face_operand_history_candidates(
            ctx,
            &mut operands,
            &[scope(DesignFeatureKind::Extrude)],
            &[],
            &[],
            BindingHistories::Shared(std::slice::from_ref(&history)),
            &HashMap::new(),
        )
    })
    .unwrap();
    assert_eq!(operands[0].resolved_face_slots, [40]);
    assert!(losses.is_empty());
}
