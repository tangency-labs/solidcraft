// SPDX-License-Identifier: Apache-2.0

use super::parse_construction_operand_group;
use crate::design::decode::operands::RecordFrame;

use crate::records::decal::DesignRecordHeader;

use crate::records::feature::extrude::DesignExtrudeExtent;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::extrude::DesignExtrudePrologue;
use crate::records::feature::extrude::DesignExtrudeStart;
use crate::records::feature::scope::DesignParameterScope;

use crate::records::topology::extrude_selection::DesignExtrudeFaceRole;
use crate::records::topology::extrude_selection::DesignExtrudeOperandRole;

#[test]
fn class_296_two_sided_to_faces_role_0x12_is_a_face_group_only_in_its_exact_scope() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#296536",
        crate::records::feature::scope::DesignFeatureKind::Extrude,
        296_536,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = 1000;
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("296".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("261".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 536;
            draft.reference_count_offset = 1291;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(
                (0..13).map(|index| 296_500 + index).collect(),
            );
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::Extrude(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::Extrusion(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::Extrusao(slot) = scope.payload_mut()
    {
        slot.get_or_insert_with(Default::default).extrude_prologue =
            Some(DesignExtrudePrologue::LegacyShifted {
                operation_prefix_marker_offset: None,
                operation: DesignExtrudeOperation::Join,
                operation_offset: 1026,
                direction_face_extend_values: [2, 2],
                side_extent_discriminators: [2, 0],
                side_extent_discriminator_offsets: [1115, 1287],
                extent: Some(DesignExtrudeExtent::TwoSidedToFaces),
                direction_face_extend_offsets: [1030, 1034],
                direction_reversed: false,
                direction_reversed_offset: 1038,
                solid_operation: true,
                solid_operation_offset: 1039,
                start: DesignExtrudeStart::ProfilePlane,
                start_offset: 1040,
            });
    }

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"323");
    bytes.extend_from_slice(&296_501_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0x0000_0012_0000_0000u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&91u32.to_le_bytes());
    bytes.extend_from_slice(&0.125f64.to_le_bytes());
    bytes.extend_from_slice(&91u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&296_503_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&[1, 1, 0]);
    bytes.push(1);
    bytes.extend_from_slice(&296_502_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.push(0);
    bytes.push(1);
    bytes.extend_from_slice(&scope.record_index.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"261");
    bytes.extend_from_slice(&296_501_u32.to_le_bytes());

    let header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:group#296501".into(),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("323".to_owned()).unwrap(),
        record_index: 296_501,
    };
    let mut group =
        parse_construction_operand_group(&bytes, &scope, 0, &RecordFrame::from(&header))
            .complete()
            .expect("class-296 two-sided-to-faces construction group");
    assert_eq!(group.extrude_role(), None);
    crate::design::decode::operands::assign_extrude_face_roles(
        &scope,
        std::slice::from_mut(&mut group),
    );
    assert_eq!(
        group.extrude_role(),
        Some(DesignExtrudeOperandRole::Faces(
            DesignExtrudeFaceRole::Termination
        ))
    );

    let mut wrong_length = scope.clone();
    wrong_length
        .try_edit(|draft| {
            draft.frame_length = 537;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let group =
        parse_construction_operand_group(&bytes, &wrong_length, 0, &RecordFrame::from(&header))
            .complete()
            .expect("construction group with otherwise valid frame");
    assert_eq!(group.extrude_role(), None);

    let mut wrong_extent = scope;
    let Some(DesignExtrudePrologue::LegacyShifted { extent, .. }) =
        wrong_extent.extrude_prologue_mut()
    else {
        panic!("synthetic class-296 two-sided-to-faces prologue");
    };
    *extent = Some(DesignExtrudeExtent::SymmetricDistance);
    let group =
        parse_construction_operand_group(&bytes, &wrong_extent, 0, &RecordFrame::from(&header))
            .complete()
            .expect("construction group with otherwise valid frame");
    assert_eq!(group.extrude_role(), None);
}
