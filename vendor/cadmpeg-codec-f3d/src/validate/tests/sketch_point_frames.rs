// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::references::DesignClassTag;
use crate::records::topology::construction::{
    DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
    DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
    DesignConstructionOperandRole,
};
use crate::records::topology::entity_selection::{
    DesignEntitySelectionOperand, DesignEntitySelectionOperandDraft,
};
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::test_support::native_test::f3d_native_mut;

const STREAM: &str = "f3d:Design/BulkStream.dat";

fn group(role: DesignOperandRole) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: format!("{STREAM}:design-construction-operand-group#100"),
        scope_record_index: 10,
        scope_reference_ordinal: 0,
        record_index: 100,
        byte_offset: 900,
        class_tag: DesignClassTag::try_from("277".to_owned()).unwrap(),
        members: vec![crate::records::identity::Located {
            value: 200,
            offset: 926,
        }],
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame::try_from(
            DesignConstructionOperandGroupFrameDraft {
                member_count_offset: 921,
                auxiliary_records: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_records: Vec::new(),
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
        operand_role: DesignConstructionOperandRole::Other(role),
        role_offset: 953,
        paired_class_tag: DesignClassTag::try_from("265".to_owned()).unwrap(),
        paired_byte_offset: 1024,
    })
    .unwrap()
}

/// The compact sketch-point selection of group 100's member.
fn sketch_point_selection() -> DesignEntitySelectionOperand {
    DesignEntitySelectionOperand::try_new(DesignEntitySelectionOperandDraft {
        id: format!("{STREAM}:design-entity-selection-operand#1000"),
        scope_record_index: 10,
        group_record_index: 100,
        group_member_ordinal: 0,
        record_index: 200,
        byte_offset: 1_000,
        class_tag: DesignClassTag::try_from("369".to_owned()).unwrap(),
        serialization_form: Default::default(),
        asset_id: "11111111-2222-4333-8444-555555555555"
            .to_owned()
            .try_into()
            .unwrap(),
        asset_id_offset: 1_034,
        context_id: "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"
            .to_owned()
            .try_into()
            .unwrap(),
        context_id_offset: 1_100,
        identity_record_index: 203,
        identity_record_offset: 2_000,
        primary_identity: 265,
        primary_identity_offset: 2_021,
        secondary: Some(crate::records::identity::DesignSecondaryIdentity {
            identity: crate::records::identity::Located {
                value: 110,
                offset: 2_029,
            },
            curve_identity: None,
        }),
        historical_edge_candidates: Vec::new(),
        historical_face_candidates: Vec::new(),
        resolved_edge_slot: None,
        next_record_index: 204,
        next_byte_offset: 2_037,
    })
    .unwrap()
}

/// Whether validation reports the sketch-point selection of a group with
/// `role` under the scopes `kinds`, all with record index 10.
fn reports_the_selection(role: DesignOperandRole, kinds: &[DesignFeatureKind]) -> bool {
    let operand = sketch_point_selection();
    assert!(operand.sketch_point().is_some());
    let operand_id = operand.id.clone();
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_construction_operand_groups.push(group(role));
        native
            .design_record_headers
            .push(crate::records::decal::DesignRecordHeader {
                id: format!("{STREAM}:design-record-header#1000"),
                byte_offset: 1_000,
                class_tag: DesignClassTag::try_from("369".to_owned()).unwrap(),
                record_index: 200,
            });
        native.design_entity_selection_operands.push(operand);
        for (ordinal, kind) in kinds.iter().enumerate() {
            native
                .design_parameter_scopes
                .push(DesignParameterScope::empty(
                    &format!("{STREAM}:design-parameter-scope#{ordinal}"),
                    kind.clone(),
                    10,
                ));
        }
    }
    crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate the native namespace")
    })
    .iter()
    .any(|finding| {
        finding.entity.as_deref() == Some(operand_id.as_str())
            && finding.message
                == "Fusion Design entity-selection operand has an invalid nested frame"
    })
}

#[test]
fn a_sketch_point_selection_belongs_to_a_loft_point_group() {
    assert!(!reports_the_selection(
        DesignOperandRole::ROLE_0X21,
        &[DesignFeatureKind::Loft]
    ));
    for (role, kinds) in [
        (DesignOperandRole::ROLE_0X5, &[DesignFeatureKind::Loft][..]),
        (DesignOperandRole::ROLE_0X21, &[DesignFeatureKind::Hole]),
        (DesignOperandRole::ROLE_0X21, &[]),
        (
            DesignOperandRole::ROLE_0X21,
            &[DesignFeatureKind::Loft, DesignFeatureKind::Loft],
        ),
    ] {
        assert!(reports_the_selection(role, kinds), "{role:?} {kinds:?}");
    }
}
