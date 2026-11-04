// SPDX-License-Identifier: Apache-2.0
//! Historical path selection binding.

use cadmpeg_ir::features::{FeatureId, PathRef};

fn path_fixture() -> (
    crate::records::feature::scope::DesignParameterScope,
    Vec<crate::records::topology::construction::DesignConstructionOperandGroup>,
    Vec<crate::records::topology::entity_selection::DesignEntitySelectionOperand>,
    PathRef,
) {
    use crate::records::topology::construction::{
        DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
        DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
        DesignConstructionOperandRole,
    };
    use crate::records::topology::entity_selection::{
        DesignEntitySelectionOperand, DesignEntitySelectionOperandDraft,
    };
    let stream = "f3d:Design/BulkStream.dat";
    let scope_id = format!("{stream}:design-parameter-scope#42");
    let group_id = format!("{stream}:design-construction-operand-group#100");
    let scope = crate::records::feature::scope::DesignParameterScope::empty(
        &scope_id,
        crate::records::feature::scope::DesignScopePayload::Sweep(None),
        42,
    );
    let group = DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: group_id.clone(),
        scope_record_index: 42,
        scope_reference_ordinal: 0,
        record_index: 100,
        byte_offset: 0,
        class_tag: "282".to_owned().try_into().unwrap(),
        members: vec![crate::records::identity::Located {
            value: 200,
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
        operand_role: DesignConstructionOperandRole::Other(
            crate::records::topology::extrude_selection::DesignOperandRole::ROLE_0X5,
        ),
        role_offset: 0,
        paired_class_tag: "261".to_owned().try_into().unwrap(),
        paired_byte_offset: 0,
    })
    .unwrap();
    let operand = DesignEntitySelectionOperand::try_new(DesignEntitySelectionOperandDraft {
        id: format!("{stream}:design-entity-selection-operand#200"),
        scope_record_index: 42,
        group_record_index: 100,
        group_member_ordinal: 0,
        record_index: 200,
        byte_offset: 0,
        class_tag: "377".to_owned().try_into().unwrap(),
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
        identity_record_index: 203,
        identity_record_offset: 0,
        primary_identity: 7,
        primary_identity_offset: 21,
        secondary: None,
        historical_edge_candidates: Vec::new(),
        historical_face_candidates: Vec::new(),
        resolved_edge_slot: Some(7),
        next_record_index: 202,
        next_byte_offset: 29,
    })
    .unwrap();
    (scope, vec![group], vec![operand], PathRef::Native(group_id))
}

fn bind() -> PathRef {
    let (scope, groups, operands, mut path) = path_fixture();
    let feature = FeatureId::mint("f3d:model:feature#path").unwrap();
    super::super::bind_entity_selection_path(&mut path, &feature, 1, &scope, &groups, &operands);
    path
}

#[test]
fn path_edges_keep_historical_identity() {
    assert!(matches!(bind(), PathRef::HistoricalEdges { .. }));
}
