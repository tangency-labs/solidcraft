// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::needless_pass_by_value
)]

use crate::history::{
    bind_historical_recipe_reference_candidates, direct_face_recipe_candidates,
    recipe_index::RecipeTopologyCache, recipe_reference_common_vertex,
    selection::bind_extrude_selection_history,
};
use crate::history_records::AsmHistory;
use crate::history_records::{
    AsmDeltaState, AsmHistoricalCoedge, AsmHistoricalEdge, AsmHistoricalRelation,
    AsmHistoricalTopology,
};
use crate::records::topology::body_recipe::AsmHistoricalEntityKind;

#[test]
fn extrude_history_identity_resolves_only_in_context_component_breps() {
    fn history(blob: &str, state_id: i64, kind: AsmHistoricalEntityKind) -> AsmHistory {
        let mut topology = AsmHistoricalTopology::default();
        match kind {
            AsmHistoricalEntityKind::Edge => topology.edges.push(42),
            AsmHistoricalEntityKind::Loop => topology.loops.push(42),
            _ => panic!("test history kind"),
        }
        let id = crate::ids::native_scoped_id(
            &format!("Asset/Breps.BlobParts/{blob}"),
            "asm-history",
            0,
        );
        crate::test_support::history::history(
            id.clone(),
            0,
            None,
            false,
            vec![(
                AsmDeltaState {
                    id: format!("{id}:state#{state_id}"),
                    parent: id.clone(),
                    byte_offset: 0,
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
                Some(topology),
                Vec::new(),
            )],
        )
    }

    let design_stream = "Asset/Design1/BulkStream.dat";
    let naming_spaces = vec![
        crate::records::recipes::DesignComponentNamingSpace {
            id: crate::ids::native_design_component_naming_space_id(design_stream, 0),
            byte_offset: 0,
            component_record_index: 10,
            context_uuid: "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"
                .to_owned()
                .try_into()
                .expect("GUID"),
            context_uuid_offset: 12,
            name: None,
            name_offset: None,
        },
        crate::records::recipes::DesignComponentNamingSpace {
            id: crate::ids::native_design_component_naming_space_id(design_stream, 100),
            byte_offset: 100,
            component_record_index: 20,
            context_uuid: "ffffffff-eeee-4ddd-8ccc-bbbbbbbbbbbb"
                .to_owned()
                .try_into()
                .expect("GUID"),
            context_uuid_offset: 112,
            name: None,
            name_offset: None,
        },
    ];
    let binding = |at, entity_suffix, blob_name: &str| {
        crate::records::bodies::DesignBodyBinding::try_from(
            crate::records::bodies::DesignBodyBindingWire {
                component_owner: None,
                component_owner_unresolved: false,
                history_only: false,
                id: crate::ids::native_design_body_binding_id(design_stream, at),
                stream: design_stream.into(),
                pair_count: 1,
                pair_ordinal: 0,
                asm_body_key: 1,
                asm_body_key_offset: at,
                entity_suffix,
                entity_suffix_offset: at + 8,
                blob_name: blob_name.into(),
                blob_name_offset: at + 16,
                body: None,
            },
        )
        .unwrap()
    };
    let body_bindings = vec![
        binding(200, 15, "BREP.a.smbh"),
        binding(300, 25, "BREP.b.smbh"),
    ];
    let histories = vec![
        history("BREP.a.smbh", 1, AsmHistoricalEntityKind::Edge),
        history("BREP.b.smbh", 2, AsmHistoricalEntityKind::Loop),
    ];
    let mut members = vec![
        crate::records::topology::extrude_selection::DesignExtrudeSelectionMember::try_new(
            crate::records::topology::extrude_selection::DesignExtrudeSelectionMemberDraft {
                id: crate::ids::native_scoped_id(design_stream, "extrude-selection-member", 400),
                group_record_index: 1,
                group_member_ordinal: 0,
                record_index: 2,
                byte_offset: 400,
                class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned())
                    .unwrap(),
                local_id: 42,
                local_id_offset: 421,
                asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "11111111-2222-4333-8444-555555555555".to_owned(),
                )
                .unwrap(),
                asset_id_offset: 433,
                context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "ffffffff-eeee-4ddd-8ccc-bbbbbbbbbbbb".to_owned(),
                )
                .unwrap(),
                context_id_offset: 505,
                tail_slot_present: false,
                tail_slot_offset: 581,
                scope_tail: None,
                resolved_geometry: None,
                operand_identity_ids: Vec::new(),
                historical: None,
                next_record_index: 3,
                next_byte_offset: 590,
            },
        )
        .unwrap(),
    ];

    bind_extrude_selection_history(&mut members, &naming_spaces, &body_bindings, &histories);

    assert_eq!(
        members[0].historical.as_ref().map(|binding| binding.kind),
        Some(AsmHistoricalEntityKind::Loop)
    );
    assert_eq!(
        members[0]
            .historical
            .as_ref()
            .map(|binding| binding.entity_ref),
        Some(42)
    );
    assert_eq!(members[0].historical.as_ref().unwrap().state_ids, [2]);
}

#[test]
fn historical_recipe_join_unions_fragments_without_raw_selector_equality() {
    let mut topology = AsmHistoricalTopology {
        faces: vec![10, 11, 12],
        edges: vec![20],
        ..Default::default()
    };
    let tag = |entity_kind, entity_ref, selector, references: Vec<i64>| {
        crate::history_records::AsmHistoricalPersistentSubentityTag {
            entity_kind,
            entity_ref,
            selector,
            token: "rim".into(),
            design_references: references,
            ordinal: 0,
        }
    };
    topology.persistent_subentity_tags = vec![
        tag(AsmHistoricalEntityKind::Face, 10, 7, vec![301]),
        tag(AsmHistoricalEntityKind::Face, 11, 9, vec![301]),
        tag(AsmHistoricalEntityKind::Face, 12, 7, vec![302]),
        tag(AsmHistoricalEntityKind::Edge, 20, 11, vec![301]),
    ];
    let mut reference = crate::records::dimensions::DesignRecipeReference {
        selector: 0x0100_0080,
        selector_offset: 0,
        token: "rim".into(),
        token_offset: 4,
        design_reference: 301,
        design_reference_offset: 8,
        candidate_faces: vec![
            cadmpeg_ir::ids::FaceId::mint("test:model:face#wrong-active-face")
                .expect("identity grammar"),
        ],
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
    };

    {
        bind_historical_recipe_reference_candidates(
            &mut reference,
            &topology,
            &mut RecipeTopologyCache::default(),
        )
    }
    .unwrap();

    assert_eq!(
        reference.candidate_faces,
        [
            cadmpeg_ir::ids::FaceId::mint(crate::ids::brep_entity_id(10))
                .expect("identity grammar"),
            cadmpeg_ir::ids::FaceId::mint(crate::ids::brep_entity_id(11))
                .expect("identity grammar"),
        ]
    );
    assert_eq!(
        reference.candidate_edges,
        [
            cadmpeg_ir::ids::EdgeId::mint(crate::ids::brep_entity_id(20))
                .expect("identity grammar")
        ]
    );
    assert!(reference.alternate_selector_faces.is_empty());
    assert!(reference.alternate_selector_edges.is_empty());
}

#[test]
fn direct_face_recipe_selects_every_fragment_in_its_own_reference_lane() {
    let reference =
        |design_reference, faces: &[i64]| crate::records::dimensions::DesignRecipeReference {
            selector: 0,
            selector_offset: 0,
            token: "face".into(),
            token_offset: 0,
            design_reference,
            design_reference_offset: 0,
            candidate_faces: faces
                .iter()
                .map(|face| {
                    cadmpeg_ir::ids::FaceId::mint(crate::ids::brep_entity_id(face))
                        .expect("identity grammar")
                })
                .collect(),
            candidate_edges: Vec::new(),
            alternate_selector_faces: Vec::new(),
            alternate_selector_edges: Vec::new(),
        };
    let references = [reference(203, &[8, 7]), reference(199, &[9])];

    assert_eq!(
        direct_face_recipe_candidates(
            crate::records::recipes::ConstructionRecipeKind::Face,
            &references,
            203
        ),
        Some(vec![
            cadmpeg_ir::ids::FaceId::mint(crate::ids::brep_entity_id(7)).expect("identity grammar"),
            cadmpeg_ir::ids::FaceId::mint(crate::ids::brep_entity_id(8)).expect("identity grammar"),
        ])
    );
    assert!(direct_face_recipe_candidates(
        crate::records::recipes::ConstructionRecipeKind::BoundedFace,
        &references,
        203
    )
    .is_none());
}

#[test]
fn corner_recipe_intersects_vertex_sets_across_fragment_unions() {
    let relation = |owner_ref, member_refs| AsmHistoricalRelation {
        owner_ref,
        member_refs,
    };
    let topology = AsmHistoricalTopology {
        faces: vec![10, 11, 12],
        loops: vec![100, 101, 102],
        coedges: vec![200, 201, 202],
        edges: vec![20, 21, 22],
        vertices: vec![1, 2, 3, 4, 5],
        face_loops: vec![
            relation(10, vec![100]),
            relation(11, vec![101]),
            relation(12, vec![102]),
        ],
        loop_coedges: vec![
            relation(100, vec![200]),
            relation(101, vec![201]),
            relation(102, vec![202]),
        ],
        coedge_topology: [(200, 100, 20), (201, 101, 21), (202, 102, 22)]
            .into_iter()
            .map(|(coedge, owner_loop, edge)| AsmHistoricalCoedge {
                coedge,
                owner_loop,
                edge,
                next: coedge,
                previous: coedge,
                radial_next: coedge,
            })
            .collect(),
        edge_vertices: [(20, 1, 2), (21, 3, 4), (22, 3, 5)]
            .into_iter()
            .map(|(edge, start_vertex, end_vertex)| AsmHistoricalEdge {
                edge,
                start_vertex,
                end_vertex,
            })
            .collect(),
        ..Default::default()
    };
    let reference =
        |token: &str, faces: &[i64]| crate::records::dimensions::DesignRecipeReference {
            selector: 0,
            selector_offset: 0,
            token: token.into(),
            token_offset: 0,
            design_reference: 301,
            design_reference_offset: 0,
            candidate_faces: faces
                .iter()
                .map(|face| {
                    cadmpeg_ir::ids::FaceId::mint(crate::ids::brep_entity_id(face))
                        .expect("identity grammar")
                })
                .collect(),
            candidate_edges: Vec::new(),
            alternate_selector_faces: Vec::new(),
            alternate_selector_edges: Vec::new(),
        };
    let recipe = crate::records::feature::work_geometry::DesignVertexRecipe::try_new(
        crate::records::feature::work_geometry::DesignVertexRecipeDraft {
            record_index: 1,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("264".to_owned())
                .unwrap(),
            paired_byte_offset: 11,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "258".to_owned(),
            )
            .unwrap(),
            recipe_record_index: 4,
            recipe_record_byte_offset: 44,
            recipe_id: "recipe".into(),
            recipe_prefix_offset: 55,
            recipe_prefix_bytes: Vec::new(),
            recipe_references: vec![reference("rim", &[10, 11]), reference("end", &[12])],
            recipe_program_offset: 66,
            recipe_program: vec![0, -1],
            resolution: None,
            next_record_index: 6,
            next_byte_offset: 77,
        },
    )
    .unwrap();

    assert_eq!(
        recipe_reference_common_vertex(
            &recipe,
            &topology,
            &mut crate::history::topology_cache::TopologyQueryCache::default()
        ),
        Some(3)
    );
}
