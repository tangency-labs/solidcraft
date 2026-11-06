// SPDX-License-Identifier: Apache-2.0
//! Edge-operand reference contexts name `asm_face_boundaries` records.

use crate::history_records::AsmDeltaState;
use crate::records::topology::edge_identity::DesignEdgeOperand;
use crate::records::topology::historical_context::{
    AsmFaceBoundary, DesignEdgeRecipeReferenceContext, DesignHistoricalFaceLoopContext,
    DesignHistoricalLoopBoundary, DesignHistoricalLoopCoedge,
};
use cadmpeg_ir::ids::FaceId;

const OPERAND_MESSAGE: &str =
    "Fusion Design edge operand names an invalid historical face boundary";
const BOUNDARY_MESSAGE: &str =
    "Fusion ASM face boundary has an invalid state, identity, or reference";

fn state(state_id: i64, byte_offset: u64) -> crate::test_support::history::State {
    (
        AsmDeltaState {
            id: format!("f3d:test:asm-delta-state#{byte_offset:010}"),
            parent: "f3d:test:asm-history#0000000000".into(),
            byte_offset,
            state_id,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: None,
            node_index: state_id,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            transition: None,
        },
        None,
        Vec::new(),
    )
}

fn boundary(state: &AsmDeltaState, face_slot: i64) -> AsmFaceBoundary {
    AsmFaceBoundary {
        id: AsmFaceBoundary::identity(&state.id, state.byte_offset, face_slot).unwrap(),
        parent: state.id.clone(),
        face_slot,
        loops: vec![DesignHistoricalFaceLoopContext {
            loop_slot: face_slot + 1_000,
            boundary: DesignHistoricalLoopBoundary::Coedges(vec![DesignHistoricalLoopCoedge {
                coedge_slot: face_slot + 2_000,
                edge_slot: face_slot + 3_000,
            }]),
        }],
    }
}

fn face(slot: i64) -> FaceId {
    FaceId::mint(format!("f3d:brep:entity#{slot}")).unwrap()
}

/// A Loft scope evaluated from state 6 to state 7, one edge operand with one
/// reference context naming face 100 in both states, and both boundaries.
fn native() -> crate::native::F3dNative {
    let mut result = state(7, 20);
    let preceding = state(6, 10);
    result.0.next_ref = Some(preceding.0.node_index);
    let scope = serde_json::from_value(serde_json::json!({
        "id": "f3d:test:scope#1811",
        "byte_offset": 0,
        "class_tag": "272",
        "record_index": 1811,
        "frame_length": 200,
        "kind": "Loft",
        "kind_offset": 87,
        "feature_ordinal": 1,
        "feature_ordinal_offset": 128,
        "history_state_id": 7,
        "history_state_id_offset": 79,
        "previous_history_state_id": 6,
        "previous_history_state_id_offset": 158,
        "reference_count_offset": 9,
        "reference_members": [1000, 1001, 1819, 1, 2, 3],
        "reference_member_offsets": [14, 25, 36, 47, 58, 69],
        "paired_class_tag": "274",
        "paired_byte_offset": 200
    }))
    .unwrap();
    let mut operand: DesignEdgeOperand = serde_json::from_value(serde_json::json!({
        "id": "f3d:test:edge-operand#1",
        "scope_record_index": 1811,
        "scope_reference_ordinal": 3,
        "record_index": 1,
        "byte_offset": 0,
        "class_tag": "376",
        "paired_byte_offset": 16,
        "paired_class_tag": "260",
        "recipe_record_index": 4,
        "recipe_record_byte_offset": 32,
        "recipe_id": "f3d:test:recipe",
        "recipe_prefix_offset": 43,
        "recipe_prefix_bytes": "",
        "recipe_references": [],
        "recipe_program_offset": 0,
        "recipe_program": [-1, -1, 2],
        "next_record_index": 5,
        "next_byte_offset": 160
    }))
    .unwrap();
    let result_boundary = boundary(&result.0, 100);
    let preceding_boundary = boundary(&preceding.0, 100);
    operand.recipe_state_id = Some(6);
    operand.recipe_reference_contexts = vec![DesignEdgeRecipeReferenceContext {
        reference_ordinal: 0,
        result_faces: vec![face(100)],
        result_face_boundary_ids: vec![result_boundary.id.clone()],
        result_shared_edge_slots: Vec::new(),
        preceding_faces: vec![face(100)],
        preceding_face_boundary_ids: vec![preceding_boundary.id.clone()],
        preceding_support_face_slots: vec![100],
        preceding_support_face_boundary_ids: vec![preceding_boundary.id.clone()],
        shared_edge_slots: Vec::new(),
        changed_shared_edge_slots: Vec::new(),
        changed_reference_edge_slots: Vec::new(),
    }];
    let mut face_boundaries = vec![preceding_boundary, result_boundary];
    face_boundaries.sort_by(|left, right| left.id.cmp(&right.id));
    crate::native::F3dNative {
        design_parameter_scopes: vec![scope],
        design_edge_operands: vec![operand],
        asm_histories: vec![crate::test_support::history::history(
            "f3d:test:asm-history#0000000000".into(),
            0,
            None,
            false,
            vec![preceding, result],
        )],
        asm_face_boundaries: face_boundaries,
        ..Default::default()
    }
}

/// Messages of the face-boundary findings for `native`.
fn findings(
    native: &crate::native::F3dNative,
    expected: Option<&[AsmFaceBoundary]>,
) -> Vec<(String, String)> {
    crate::test_support::with_decode_context(|decode| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let ctx = super::super::Ctx::new(&ir, native, decode);
        let mut findings = Vec::new();
        super::super::validate_face_boundaries(&ctx, &mut findings, expected);
        findings
            .into_iter()
            .map(|finding| (finding.message, finding.entity.unwrap_or_default()))
            .collect()
    })
}

#[test]
fn face_boundary_references_resolve_to_their_states() {
    let valid = native();
    assert!(findings(&valid, None).is_empty());
    assert!(findings(&valid, Some(&valid.asm_face_boundaries)).is_empty());

    let operand = "f3d:test:edge-operand#1".to_owned();
    let result_id = valid.design_edge_operands[0].recipe_reference_contexts[0]
        .result_face_boundary_ids[0]
        .clone();
    let preceding_id = valid.design_edge_operands[0].recipe_reference_contexts[0]
        .preceding_face_boundary_ids[0]
        .clone();

    // A result face named through the preceding-state boundary.
    let mut wrong_state = native();
    wrong_state.design_edge_operands[0].recipe_reference_contexts[0].result_face_boundary_ids =
        vec![preceding_id.clone()];
    assert_eq!(
        findings(&wrong_state, None),
        [
            (OPERAND_MESSAGE.to_owned(), operand.clone()),
            (BOUNDARY_MESSAGE.to_owned(), result_id.clone()),
        ]
    );

    // A boundary of a face absent from the context's face list.
    let mut wrong_face = native();
    wrong_face.design_edge_operands[0].recipe_reference_contexts[0].preceding_support_face_slots =
        vec![101];
    assert_eq!(
        findings(&wrong_face, None),
        [(OPERAND_MESSAGE.to_owned(), operand.clone())]
    );

    // A reference to no record, and the record it leaves unreferenced.
    let mut dangling = native();
    dangling.design_edge_operands[0].recipe_reference_contexts[0].result_face_boundary_ids =
        vec!["f3d:test:asm-face-boundary#0000000020:0000000999".into()];
    assert_eq!(
        findings(&dangling, None),
        [
            (OPERAND_MESSAGE.to_owned(), operand),
            (BOUNDARY_MESSAGE.to_owned(), result_id.clone()),
        ]
    );

    // An identity that disagrees with the owning state and face slot.
    let mut wrong_identity = native();
    wrong_identity.asm_face_boundaries[1].face_slot = 101;
    assert!(findings(&wrong_identity, None).contains(&(BOUNDARY_MESSAGE.to_owned(), result_id)));

    // A boundary that differs from the one re-derived from retained topology.
    let mut expected = valid.asm_face_boundaries.clone();
    expected[0].loops.clear();
    assert_eq!(
        findings(&valid, Some(&expected)),
        [(BOUNDARY_MESSAGE.to_owned(), preceding_id)]
    );
}
