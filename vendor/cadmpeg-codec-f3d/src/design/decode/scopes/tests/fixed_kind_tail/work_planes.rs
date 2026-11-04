// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::scopes::work_geometry::exact_work_plane_frame;
use crate::layout::work_plane_legacy_321_opaque_matrix_frame as work_plane_321_opaque;
use crate::layout::work_plane_legacy_class_256_matrix_frame as work_plane_class_256;
use crate::layout::work_plane_legacy_class_337_325_matrix_frame as work_plane_class_337_325;
use crate::records::feature::scope::DesignParameterScope;

#[test]
fn legacy_work_plane_class_380_frame_decodes_its_matrix() {
    let mut bytes = vec![0; 325];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"380");
    bytes[7..11].copy_from_slice(&71u32.to_le_bytes());
    let transform = crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY.rows();
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 49 + ordinal * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"262");
    bytes.extend_from_slice(&71u32.to_le_bytes());

    let scope = DesignParameterScope::empty(
        "f3d:test:scope#1",
        crate::records::feature::scope::DesignFeatureKind::WorkPlane,
        1,
    );
    let mut scope = scope;
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![71]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let decoded = exact_work_plane_frame(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
    )
    .expect("class-380 WorkPlane frame");
    assert_eq!(decoded.transform, transform.try_into().unwrap());
    assert_eq!(decoded.transform_offset, 49);
    assert_eq!(decoded.reference, None);
}

#[test]
fn legacy_work_plane_class_256_frame_decodes_its_opaque_prefix_lane() {
    let transform: [[f64; 4]; 4] = [
        [0.0, -1.0, 0.0, 2.0],
        [1.0, 0.0, 0.0, 3.0],
        [0.0, 0.0, 1.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for opaque_u16 in [[0, 0], [0x9b, 0xdc]] {
        let mut bytes = vec![0; work_plane_class_256::LEN];
        bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
        bytes[4..7].copy_from_slice(b"256");
        bytes[7..11].copy_from_slice(&71u32.to_le_bytes());
        bytes
            [work_plane_class_256::OPAQUE_U16..work_plane_class_256::OPAQUE_U16 + opaque_u16.len()]
            .copy_from_slice(&opaque_u16);
        for (ordinal, value) in transform.into_iter().flatten().enumerate() {
            let at = work_plane_class_256::MATRIX + ordinal * 8;
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"262");
        bytes.extend_from_slice(&71u32.to_le_bytes());

        let mut scope = DesignParameterScope::empty(
            "f3d:test:scope#1",
            crate::records::feature::scope::DesignFeatureKind::WorkPlane,
            1,
        );
        scope
            .try_edit(|draft| {
                draft.reference_members =
                    crate::records::identity::ReferenceRun::unlocated(vec![71]);
                draft.layout_fixture_references();
                draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
                draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
                draft.layout_fixture_tail();
            })
            .unwrap();
        let decoded = exact_work_plane_frame(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
        )
        .expect("class-256 WorkPlane frame");
        assert_eq!(decoded.transform, transform.try_into().unwrap());
        assert_eq!(
            decoded.transform_offset,
            u64_from_index(work_plane_class_256::MATRIX)
        );
        assert_eq!(decoded.reference, None);
    }

    let mut invalid = vec![0; work_plane_class_256::LEN];
    invalid[0..4].copy_from_slice(&3u32.to_le_bytes());
    invalid[4..7].copy_from_slice(b"256");
    invalid[7..11].copy_from_slice(&71u32.to_le_bytes());
    invalid[work_plane_class_256::ZERO_PAIR] = 1;
    invalid.extend_from_slice(&3u32.to_le_bytes());
    invalid.extend_from_slice(b"262");
    invalid.extend_from_slice(&71u32.to_le_bytes());
    let mut scope = DesignParameterScope::empty(
        "f3d:test:scope#2",
        crate::records::feature::scope::DesignFeatureKind::WorkPlane,
        2,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![71]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_work_plane_frame(
            &invalid,
            &crate::design::test_support::indexed_record_offsets_for_test(&invalid),
            &scope
        ),
        None
    );
}

#[test]
fn legacy_work_plane_opaque_prefix_frames_use_class_pair_admission() {
    type WorkPlaneOpaqueCase = (&'static [u8; 3], &'static [u8; 3], usize, [u8; 2], u32);

    let transform: [[f64; 4]; 4] = [
        [0.0, -1.0, 0.0, 2.0],
        [1.0, 0.0, 0.0, 3.0],
        [0.0, 0.0, 1.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let cases: [WorkPlaneOpaqueCase; 3] = [
        (b"341", b"261", work_plane_321_opaque::LEN, [0xea, 0x20], 81),
        (b"346", b"262", work_plane_321_opaque::LEN, [0xae, 0x70], 82),
        (
            b"337",
            b"266",
            work_plane_class_337_325::LEN,
            [0x6d, 0x00],
            83,
        ),
    ];

    for (class_tag, paired_class_tag, frame_length, opaque_u16, record_index) in cases {
        let matrix = if frame_length == work_plane_321_opaque::LEN {
            work_plane_321_opaque::MATRIX
        } else {
            work_plane_class_337_325::MATRIX
        };
        let opaque = if frame_length == work_plane_321_opaque::LEN {
            work_plane_321_opaque::OPAQUE_U16
        } else {
            work_plane_class_337_325::OPAQUE_U16
        };
        let zero_pair = if frame_length == work_plane_321_opaque::LEN {
            work_plane_321_opaque::ZERO_PAIR
        } else {
            work_plane_class_337_325::ZERO_PAIR
        };
        let mut bytes = vec![0; frame_length];
        bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
        bytes[4..7].copy_from_slice(class_tag);
        bytes[7..11].copy_from_slice(&record_index.to_le_bytes());
        bytes[opaque..opaque + opaque_u16.len()].copy_from_slice(&opaque_u16);
        assert_eq!(&bytes[zero_pair..zero_pair + 2], &[0, 0]);
        for (ordinal, value) in transform.into_iter().flatten().enumerate() {
            let at = matrix + ordinal * 8;
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(paired_class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());

        let mut scope = DesignParameterScope::empty(
            "f3d:test:scope#opaque",
            crate::records::feature::scope::DesignFeatureKind::WorkPlane,
            1,
        );
        scope
            .try_edit(|draft| {
                draft.reference_members =
                    crate::records::identity::ReferenceRun::unlocated(vec![record_index]);
                draft.layout_fixture_references();
                draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
                draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
                draft.layout_fixture_tail();
            })
            .unwrap();
        let decoded = exact_work_plane_frame(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
        )
        .expect("opaque-prefix WorkPlane frame");
        assert_eq!(decoded.transform, transform.try_into().unwrap());
        assert_eq!(decoded.transform_offset, u64_from_index(matrix));
        assert_eq!(decoded.reference, None);
    }

    let mut invalid = vec![0; work_plane_321_opaque::LEN];
    invalid[0..4].copy_from_slice(&3u32.to_le_bytes());
    invalid[4..7].copy_from_slice(b"341");
    invalid[7..11].copy_from_slice(&84u32.to_le_bytes());
    invalid[work_plane_321_opaque::OPAQUE_U16..work_plane_321_opaque::OPAQUE_U16 + 2]
        .copy_from_slice(&[0xea, 0x20]);
    invalid[work_plane_321_opaque::ZERO_PAIR] = 1;
    invalid.extend_from_slice(&3u32.to_le_bytes());
    invalid.extend_from_slice(b"262");
    invalid.extend_from_slice(&84u32.to_le_bytes());
    let mut scope = DesignParameterScope::empty(
        "f3d:test:scope#invalid",
        crate::records::feature::scope::DesignFeatureKind::WorkPlane,
        2,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![84]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_work_plane_frame(
            &invalid,
            &crate::design::test_support::indexed_record_offsets_for_test(&invalid),
            &scope
        ),
        None
    );
}
