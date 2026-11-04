// SPDX-License-Identifier: Apache-2.0
//! Extrude `0x5` groups that select bodies instead of termination faces.

use crate::design::decode::operands::release_extrude_body_target_groups;
use crate::records::topology::body_recipe::{
    DesignBodyRecipeOperand, DesignBodyRecipeOperandDraft, DesignBodyRecipeReference,
    DesignOperandOwner,
};
use crate::records::topology::construction::{
    DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
    DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
    DesignConstructionOperandRole,
};
use crate::records::topology::extrude_selection::{
    DesignExtrudeFaceEncoding, DesignExtrudeFaceRole, DesignOperandRole,
};
use cadmpeg_ir::ids::FaceId;

const SCOPE: u32 = 80;
const GROUP: u32 = 90;
const MEMBER: u32 = 201;

fn face_run(usage: DesignExtrudeFaceRole) -> DesignConstructionOperandRole {
    DesignConstructionOperandRole::ExtrudeFaces {
        encoding: DesignExtrudeFaceEncoding::SelectedStart,
        usage,
    }
}

fn group(role: DesignConstructionOperandRole) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: format!("f3d:Design/BulkStream.dat:operand-group#{GROUP}"),
        scope_record_index: SCOPE,
        scope_reference_ordinal: 7,
        record_index: GROUP,
        byte_offset: 900,
        class_tag: crate::records::references::DesignClassTag::try_from("331".to_owned()).unwrap(),
        members: vec![crate::records::identity::Located {
            value: MEMBER,
            offset: 926,
        }],
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame::try_from(
            DesignConstructionOperandGroupFrameDraft {
                member_count_offset: 921,
                auxiliary_records: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_records: vec![crate::records::identity::Located {
                    value: 200,
                    offset: 943,
                }],
                trailing_transforms: Vec::new(),
                trailing_dual_transforms: Vec::new(),
                trailing_flags: Vec::new(),
                opaque_index: 1,
                opaque_index_offset: 971,
                opaque_scalar: 0.0,
                opaque_scalar_offset: 975,
                variant: false,
                closing_flag: false,
            },
        )
        .unwrap(),
        operand_role: role,
        role_offset: 953,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("262".to_owned())
            .unwrap(),
        paired_byte_offset: 1024,
    })
    .unwrap()
}

fn operand(scope: u32, group_record_index: u32, ordinal: u32) -> DesignBodyRecipeOperand {
    DesignBodyRecipeOperand::try_new(DesignBodyRecipeOperandDraft {
        id: format!("f3d:Design/BulkStream.dat:body-recipe-operand#{MEMBER}"),
        scope_record_index: scope,
        owner: DesignOperandOwner::Group {
            group_record_index,
            group_member_ordinal: ordinal,
        },
        record_index: MEMBER,
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("350".to_owned()).unwrap(),
        asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
            "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
        )
        .unwrap(),
        asset_id_offset: 56,
        context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
            "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
        )
        .unwrap(),
        context_id_offset: 132,
        selector_tail: None,
        references: vec![DesignBodyRecipeReference {
            design_reference: 301,
            design_reference_offset: 25,
            form: 33,
            form_offset: 33,
            candidate_faces: vec![FaceId::mint("f3d:brep:entity#12").expect("identity grammar")],
            preceding_candidate_faces: Vec::new(),
            preceding_body_slots: Vec::new(),
        }],
        nested_record_index: 204,
        nested_record_index_offset: 38,
        recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#205".into(),
        resolved_face_slot: None,
        resolved_body_state_id: None,
        resolved_body_slot: None,
        resolved_body_face_slots: Vec::new(),
        next_record_index: 205,
        next_byte_offset: 256,
    })
    .unwrap()
}

fn released(
    role: DesignConstructionOperandRole,
    operands: &[DesignBodyRecipeOperand],
) -> DesignConstructionOperandRole {
    let mut groups = [group(role)];
    release_extrude_body_target_groups(&mut groups, operands);
    groups[0].operand_role
}

#[test]
fn termination_run_of_body_recipes_returns_to_its_source_role() {
    let termination = face_run(DesignExtrudeFaceRole::Termination);
    assert_eq!(
        released(termination, &[operand(SCOPE, GROUP, 0)]),
        DesignConstructionOperandRole::Other(DesignOperandRole::ROLE_0X5)
    );
}

#[test]
fn face_runs_and_foreign_operands_keep_their_role() {
    let termination = face_run(DesignExtrudeFaceRole::Termination);
    // No body operand, another scope, another group, or another member slot.
    for operands in [
        Vec::new(),
        vec![operand(SCOPE + 1, GROUP, 0)],
        vec![operand(SCOPE, GROUP + 1, 0)],
        vec![operand(SCOPE, GROUP, 1)],
    ] {
        assert_eq!(released(termination, &operands), termination);
    }
    // A start run is a face selection whatever its members decode as.
    let start = face_run(DesignExtrudeFaceRole::Start);
    assert_eq!(released(start, &[operand(SCOPE, GROUP, 0)]), start);
}
