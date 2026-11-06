// SPDX-License-Identifier: Apache-2.0
use crate::history::bind_profile_face_group_cardinality;
use crate::history_records::AsmDeltaState;
use crate::history_records::AsmHistoricalCarrierBinding;
use crate::history_records::AsmHistoricalTopology;
use cadmpeg_core::decode::u64_from_index;
use std::collections::HashMap;

#[test]
fn nested_extrude_profile_uses_root_cardinality_and_member_order() {
    use crate::records::{
        feature::scope::DesignParameterScope,
        topology::{construction::DesignConstructionOperandGroup, face::DesignFaceOperand},
    };
    use cadmpeg_ir::features::{PlanarProfileRef, ProfileRef};

    let group = |record_index, scope_reference_ordinal, members: Vec<u32>| {
        let member_offsets = (0..members.len())
            .map(|index| u64_from_index(index) * 11)
            .collect::<Vec<_>>();
        serde_json::from_value::<DesignConstructionOperandGroup>(serde_json::json!({
            "id": format!(
                "f3d:Design/BulkStream.dat:design-construction-operand-group#{record_index}"
            ),
            "scope_record_index": 42,
            "scope_reference_ordinal": scope_reference_ordinal,
            "record_index": record_index,
            "byte_offset": 0,
            "class_tag": "267",
            "members": members,
            "member_offsets": member_offsets,
            "frame": {
                "member_count_offset": 0,
                "opaque_index": 1,
                "opaque_index_offset": 18,
                "opaque_scalar": 0.0,
                "opaque_scalar_offset": 22,
                "variant": false
            },
            "role": 279_172_874_240_u64,
            "extrude_role": "profile",
            "role_offset": 0,
            "paired_class_tag": "260",
            "paired_byte_offset": 0
        }))
        .expect("profile group")
    };
    let paired_prefix = || {
        let mut prefix = vec![0; 10];
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(&2u32.to_le_bytes());
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.push(b'2');
        prefix.extend_from_slice(&[0; 4]);
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(&305u32.to_le_bytes());
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.push(b'3');
        prefix.extend_from_slice(&0u32.to_le_bytes());
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(&305u32.to_le_bytes());
        prefix.extend_from_slice(&0u32.to_le_bytes());
        prefix
    };
    let face_operand = |record_index, group_record_index, scope_reference_ordinal| {
        let mut operand = serde_json::from_value::<DesignFaceOperand>(serde_json::json!({
            "id": format!("f3d:Design/BulkStream.dat:design-face-operand#{record_index}"),
            "scope_record_index": 42,
            "scope_reference_ordinal": scope_reference_ordinal,
            "group_record_index": group_record_index,
            "group_member_ordinal": 0,
            "record_index": record_index,
            "byte_offset": 0,
            "class_tag": "297",
            "paired_byte_offset": 16,
            "paired_class_tag": "259",
            "recipe_record_index": record_index + 3,
            "recipe_record_byte_offset": 32,
            "recipe_id": format!("f3d:Design/BulkStream.dat:construction-recipe#{}", record_index + 3),
            "recipe_prefix_offset": 43,
            "recipe_prefix_bytes": "",
            "recipe_references": [],
            "recipe_kind": "bounded_face",
            "recipe_program_offset": 0,
            "recipe_program": [0],
            "recipe_node_offsets": [],
            "recipe_nodes": [],
            "next_record_index": record_index + 4,
            "next_byte_offset": 160
        }))
        .expect("profile face operand");
        *operand.recipe_prefix_bytes = paired_prefix();
        operand.recipe_references =
            crate::design::decode::dimension_frames::decode_recipe_references(
                &operand.recipe_prefix_bytes,
                0,
            );
        operand
    };

    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#42",
        crate::records::feature::scope::DesignFeatureKind::Extrude,
        42,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(2);
            draft.previous_history_state_id = Some(1);
            draft.reference_members = {
                let reference_values: Vec<u32> = vec![100, 110, 111, 120, 121];
                let reference_offsets = (0..reference_values.len())
                    .map(|ordinal| 14 + 11 * u64_from_index(ordinal))
                    .collect();
                crate::records::identity::ReferenceRun::from_columns(
                    reference_values,
                    reference_offsets,
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
    let groups = vec![
        group(100, 0, vec![110, 120]),
        group(110, 1, vec![111]),
        group(120, 3, vec![121]),
    ];
    let mut operands = vec![face_operand(111, 110, 1), face_operand(121, 120, 3)];
    operands[0].candidate_faces =
        vec![
            cadmpeg_ir::ids::FaceId::mint(crate::ids::brep_entity_id(10))
                .expect("identity grammar"),
        ];
    operands[1].unreferenced_candidate_faces =
        vec![
            cadmpeg_ir::ids::FaceId::mint(crate::ids::brep_entity_id(11))
                .expect("identity grammar"),
        ];

    let roots = crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::face_resolve::extrude_profile_group_roots(decode_ctx, &scope, &groups)
    })
    .unwrap()
    .expect("valid profile hierarchy");
    assert_eq!(
        roots
            .iter()
            .map(|group| group.record_index)
            .collect::<Vec<_>>(),
        [100]
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::face_resolve::extrude_profile_group_operand_indices(
                decode_ctx, roots[0], &groups, &operands,
            )
        })
        .unwrap()
        .expect("one leaf operand per root member"),
        [0, 1]
    );
    let mut repeated_child = groups.clone();
    let repeated_members = repeated_child[0]
        .members()
        .iter()
        .copied()
        .chain([crate::records::identity::Located {
            value: 110,
            offset: repeated_child[0].members().last().unwrap().offset + 11,
        }])
        .collect();
    repeated_child[0].try_set_members(repeated_members).unwrap();
    assert!(crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::face_resolve::extrude_profile_group_roots(
            decode_ctx,
            &scope,
            &repeated_child,
        )
    })
    .unwrap()
    .is_none());

    let previous_topology = AsmHistoricalTopology {
        faces: vec![10, 11, 20],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 1000,
            },
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 1001,
            },
            AsmHistoricalCarrierBinding {
                entity: 20,
                carrier: 2000,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    let state = |state_id, topology, next_ref| {
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
            topology,
            Vec::new(),
        )
    };
    let previous = state(1, Some(previous_topology), None);
    let current = state(
        2,
        Some(AsmHistoricalTopology {
            faces: vec![20],
            ..Default::default()
        }),
        Some(1),
    );
    let history = crate::test_support::history::history(
        "f3d:history".into(),
        0,
        None,
        false,
        vec![previous, current],
    );
    let bound_history_id = history.id.clone();
    let mut unrelated_history = history.clone();
    unrelated_history.id = "f3d:other-history".into();
    crate::test_support::history::edit(&mut unrelated_history, 0, |topology, _| {
        topology.faces = vec![30, 31, 40];
    });
    let histories = vec![history, unrelated_history];
    let scope_histories = HashMap::from([(scope.id.clone(), bound_history_id)]);

    crate::test_support::with_decode_context(|decode_ctx| {
        bind_profile_face_group_cardinality(
            decode_ctx,
            &mut operands,
            std::slice::from_ref(&scope),
            &groups,
            &histories,
            &scope_histories,
        )
    })
    .unwrap();
    assert_eq!(operands[0].resolved_face_slots, [10]);
    assert_eq!(operands[1].resolved_face_slots, [11]);
    let profile = crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::face_resolve::resolved_extrude_profile_face_group(
            decode_ctx, &scope, roots[0], &groups, &operands,
        )
    })
    .unwrap()
    .expect("resolved root profile");
    let feature = crate::ids::neutral_feature_id(&scope);
    let feature_key = feature.key();
    let prefix = crate::ids::history_input_prefix(&feature_key, 1);
    assert!(matches!(
        profile,
        ProfileRef::Planar(PlanarProfileRef::HistoricalFaces {
            state,
            faces,
            native,
        }) if state == crate::ids::feature_input_topology_id(&feature, 1)
            && faces.as_slice() == [
                crate::ids::history_input_face_id(&prefix, 10),
                crate::ids::history_input_face_id(&prefix, 11),
            ]
            && native.as_slice() == [groups[0].id.clone()]
    ));
}
