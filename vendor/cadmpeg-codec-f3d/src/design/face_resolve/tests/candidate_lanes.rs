// SPDX-License-Identifier: Apache-2.0

use super::super::{
    legacy_face_recipe_reference_candidates, resolved_explicit_bounded_face_group,
    resolved_extrude_profile_face_group, resolved_face_group,
};
use super::{face, reference};
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::{
    construction::DesignConstructionOperandGroup, face::DesignFaceOperand,
};
use cadmpeg_ir::ids::FaceId;

#[test]
fn explicit_bounded_face_group_uses_only_its_owned_candidate_lane() {
    crate::test_support::with_decode_context(|decode_ctx| {
        let mut operand: DesignFaceOperand = serde_json::from_value(serde_json::json!({
        "id": "f3d:test:face-operand#200",
        "scope_record_index": 100,
        "scope_reference_ordinal": 0,
        "group_record_index": 150,
        "group_member_ordinal": 0,
        "record_index": 200,
        "byte_offset": 0,
        "class_tag": "346",
        "paired_byte_offset": 325,
        "paired_class_tag": "262",
        "recipe_record_index": 203,
        "recipe_record_byte_offset": 341,
        "recipe_id": "f3d:test:recipe#201",
        "recipe_prefix_offset": 352,
        "recipe_prefix_bytes": "",
        "recipe_references": [],
        "recipe_kind": "bounded_face",
        "recipe_program_offset": 0,
        "recipe_program": [0, -1, 1],
        "recipe_node_offsets": [0],
        "recipe_nodes": [{
            "byte_offset": 0,
            "end_byte_offset": 12,
            "program": [0, -1, 1],
            "recipe_structure": {
                "root": 0,
                "prelude": [0, 2],
                "sides": [
                    {"field_count": 1, "header_value": 0, "payload_entry_count": 0, "payload_prefix": [], "scalars": [], "entries": []},
                    {"field_count": 1, "header_value": 0, "payload_entry_count": 0, "payload_prefix": [], "scalars": [], "entries": []}
                ],
                "postlude": []
            }
        }],
        "candidate_faces": ["f3d:brep:entity#10", "f3d:brep:entity#20"],
        "unreferenced_candidate_faces": [],
        "alternate_selector_candidate_faces": [],
        "preceding_candidate_faces": [],
        "changed_candidate_faces": [],
        "historical_support_contexts": [],
        "resolved_face_slots": [],
        "next_record_index": 202,
        "next_byte_offset": 469
    }))
    .expect("legacy bounded-face operand");
        operand.recipe_references = vec![
            reference(10, "selected-a", 201),
            reference(20, "selected-b", 201),
        ];

        let mut group: DesignConstructionOperandGroup = serde_json::from_value(serde_json::json!({
            "id": "f3d:test:construction-group#150",
            "scope_record_index": 100,
            "scope_reference_ordinal": 0,
            "record_index": 150,
            "byte_offset": 0,
            "class_tag": "346",
            "role": 0x0000_0010_0000_0000_u64,
            "members": [200],
            "member_offsets": [0],
            "frame": {
                "member_count_offset": 0,
                "opaque_index": 1,
                "opaque_index_offset": 18,
                "opaque_scalar": 0.0,
                "opaque_scalar_offset": 22,
                "variant": false
            },
            "role_offset": 0,
            "paired_class_tag": "262",
            "paired_byte_offset": 325,
            "next_record_index": 151,
            "next_byte_offset": 0
        }))
        .expect("legacy Draft face group");

        assert_eq!(
            resolved_explicit_bounded_face_group(&group, &[operand.clone()])
                .expect("resolve the bounded face group"),
            Some(cadmpeg_ir::features::FaceSelection::Resolved {
                faces: vec![face(10), face(20)],
                native: group.id.clone(),
            })
        );
        group.operand_role =
            crate::records::topology::construction::DesignConstructionOperandRole::ExtrudeProfile;
        let scope: DesignParameterScope = serde_json::from_value(serde_json::json!({
            "id": "f3d:test:scope#100",
            "byte_offset": 0,
            "class_tag": "304",
            "record_index": 100,
            "frame_length": 300,
            "kind": "Extrude",
            "kind_offset": 32,
            "feature_ordinal": 1,
            "feature_ordinal_offset": 228,
            "history_state_id": 2,
            "history_state_id_offset": 24,
            "previous_history_state_id": 1,
            "previous_history_state_id_offset": 258,
            "reference_count_offset": 9,
            "reference_members": [150],
            "reference_member_offsets": [14],
            "paired_class_tag": "258",
            "paired_byte_offset": 300
        }))
        .expect("Extrude scope");
        assert_eq!(
            resolved_extrude_profile_face_group(
                decode_ctx,
                &scope,
                &group,
                std::slice::from_ref(&group),
                &[operand.clone()]
            )
            .unwrap(),
            Some(cadmpeg_ir::features::ProfileRef::Planar(
                cadmpeg_ir::features::PlanarProfileRef::Faces(vec![face(10), face(20),])
            ))
        );
        operand.resolved_active_face =
            Some(FaceId::mint("f3d:brep/legacy/brep:entity#30").expect("identity grammar"));
        assert_eq!(
            resolved_face_group(&group, std::slice::from_ref(&operand))
                .expect("resolve the face group"),
            Some(cadmpeg_ir::features::FaceSelection::Resolved {
                faces: vec![
                    FaceId::mint("f3d:brep/legacy/brep:entity#30").expect("identity grammar")
                ],
                native: group.id.clone(),
            })
        );
        operand.resolved_active_face = None;
        operand.preceding_candidate_faces = vec![face(10), face(20)];
        assert_eq!(
            resolved_face_group(&group, std::slice::from_ref(&operand))
                .expect("resolve the face group"),
            None
        );
        operand.preceding_candidate_faces.clear();

        operand.candidate_faces.clear();
        operand.recipe_references = vec![
            reference(30, "context", 202),
            reference(10, "selected-a", 201),
            reference(20, "selected-b", 201),
        ];
        operand.candidate_faces = vec![face(10), face(20)];
        assert_eq!(
            legacy_face_recipe_reference_candidates(&operand, 201),
            Some(vec![face(10), face(20)])
        );
        operand.candidate_faces.clear();
        assert_eq!(
            resolved_explicit_bounded_face_group(&group, &[operand.clone()])
                .expect("resolve the bounded face group"),
            Some(cadmpeg_ir::features::FaceSelection::Resolved {
                faces: vec![face(10), face(20)],
                native: group.id.clone(),
            })
        );
        assert_eq!(
            legacy_face_recipe_reference_candidates(&operand, 201),
            Some(vec![face(10), face(20)])
        );
        assert!(legacy_face_recipe_reference_candidates(&operand, 999).is_none());
        operand.recipe_references.remove(0);
        operand.recipe_references[0]
            .alternate_selector_faces
            .push(face(30));
        assert!(resolved_explicit_bounded_face_group(&group, &[operand])
            .expect("resolve the bounded face group")
            .is_none());
    });
}
