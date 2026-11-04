// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::scopes::direct_face::exact_direct_face_operation;
use crate::records::feature::direct_face::DesignDirectFaceOperation;
use crate::records::feature::scope::DesignParameterScope;
use crate::test_support::indexed_header;
use crate::test_support::write_marked_reference;

#[test]
fn class_347_thicken_frame_admits_group_before_scalar() {
    let mut frame = vec![0; 291];
    frame[0..4].copy_from_slice(&3u32.to_le_bytes());
    frame[4..7].copy_from_slice(b"347");
    frame[7..11].copy_from_slice(&1u32.to_le_bytes());
    frame[21..25].copy_from_slice(&4u32.to_le_bytes());
    frame[25..29].copy_from_slice(&1u32.to_le_bytes());
    write_marked_reference(&mut frame, 29, 200);
    frame[40..42].copy_from_slice(&[1, 1]);
    write_marked_reference(&mut frame, 42, 74);
    frame[53..57].copy_from_slice(&1u32.to_le_bytes());
    write_marked_reference(&mut frame, 57, 300);
    frame[76..80].copy_from_slice(&36u32.to_le_bytes());
    let guid = "00000000-0000-0000-0000-000000000000";
    for (ordinal, unit) in guid.encode_utf16().enumerate() {
        frame[80 + ordinal * 2..82 + ordinal * 2].copy_from_slice(&unit.to_le_bytes());
    }
    frame[155..159].copy_from_slice(&3u32.to_le_bytes());
    write_marked_reference(&mut frame, 159, 200);
    write_marked_reference(&mut frame, 170, 201);
    write_marked_reference(&mut frame, 181, 74);
    frame[192..196].copy_from_slice(&1u32.to_le_bytes());
    frame[196..200].copy_from_slice(&7u32.to_le_bytes());
    for (ordinal, unit) in "Thicken".encode_utf16().enumerate() {
        frame[200 + ordinal * 2..202 + ordinal * 2].copy_from_slice(&unit.to_le_bytes());
    }
    frame[214..218].copy_from_slice(&1u32.to_le_bytes());
    frame[245..249].copy_from_slice(&1u32.to_le_bytes());

    let mut bytes = frame;
    indexed_header(&mut bytes, *b"258", 1);
    let scalar_start = bytes.len();
    indexed_header(&mut bytes, *b"277", 74);
    bytes.resize(scalar_start + 104, 0);
    bytes[scalar_start + 40..scalar_start + 48].copy_from_slice(&(-1.0f64).to_le_bytes());
    indexed_header(&mut bytes, *b"261", 74);

    let mut scope = DesignParameterScope::empty(
        "f3d:test:thicken#1",
        crate::records::feature::scope::DesignFeatureKind::Thicken,
        1,
    );
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("347".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("258".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 291;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![200, 201, 74]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert!(matches!(
        exact_direct_face_operation(&bytes, &crate::design::test_support::indexed_record_offsets_for_test(&bytes), &scope),
        Some(DesignDirectFaceOperation::Thicken(
            crate::records::feature::direct_face::DesignThickenOperation {
                signed_thickness,
                thickness_record_index: 74,
                ..
            }
        )) if signed_thickness.get() == -1.0
    ));

    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("259".to_owned()).unwrap();
    assert_eq!(
        exact_direct_face_operation(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope
        ),
        None
    );
}
