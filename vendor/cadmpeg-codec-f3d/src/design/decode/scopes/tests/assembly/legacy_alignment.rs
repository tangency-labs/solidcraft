// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::u64_from_index;

use super::EPS_EXACT_FIXTURE;
use crate::design::decode::scopes::assembly_alignment::exact_assembly_alignment;
use crate::records::feature::assembly::DesignAssemblyLimitKind;
use crate::records::feature::scope::DesignFeatureKind;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::identity::ReferenceRun;
use crate::records::parameters::DesignParameterOwner;
use crate::records::parameters::DesignParameterOwnerWire;
use crate::records::references::DesignClassTag;
use crate::records::sketch_placement::SketchPlacementMatrix;
use crate::test_support::indexed_header;

#[test]
fn legacy_as_built_421_alignment_retains_ordered_limits_without_operand_projection() {
    let owner = |scope_record_index: u32,
                 record_index: u32,
                 local_ordinal: u32,
                 class_tag: &str,
                 value: f64,
                 offset: u64| {
        DesignParameterOwner::try_from(DesignParameterOwnerWire {
            id: format!("f3d:Design/BulkStream.dat:design-parameter-owner#{record_index}"),
            byte_offset: (offset) - 40,
            frame_length: 103,
            class_tag: DesignClassTag::try_from(class_tag.to_owned()).unwrap(),
            record_index,
            scope_record_index,
            local_ordinal,
            evaluated_value: value,
            evaluated_value_offset: offset,
            parameter_record_index: record_index + 1,
            owned_ordinal: local_ordinal,
            variant: None,
            companion_record_index: record_index + 2,
        })
        .unwrap()
    };
    for (class_tag, paired_class_tag, owner_class, expected_limit_kind, reverse_limit_order) in [
        ("364", "272", "293", DesignAssemblyLimitKind::Angular, false),
        ("420", "262", "378", DesignAssemblyLimitKind::Linear, true),
        ("417", "263", "318", DesignAssemblyLimitKind::Linear, true),
        ("457", "258", "418", DesignAssemblyLimitKind::Linear, false),
    ] {
        let generation = crate::design::assembly::legacy_as_built_421_generation(
            421,
            class_tag,
            paired_class_tag,
        )
        .expect("fixture generation is admitted");
        assert_eq!(generation.owner_class_tag(), owner_class);
        assert_eq!(generation.limit_kind(), expected_limit_kind);
        assert_eq!(generation.reverse_limit_order(), reverse_limit_order);
        let scope_record_index = 10_u32;
        let owner_record_indices = [100, 101, 102, 103, 104, 105, 106];
        let reference_members = [
            20,
            21,
            22,
            23,
            owner_record_indices[0],
            owner_record_indices[1],
            owner_record_indices[2],
            owner_record_indices[3],
            200,
            owner_record_indices[5],
            owner_record_indices[6],
        ];
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#0",
            DesignFeatureKind::AsBuilt,
            scope_record_index,
        );
        scope.class_tag = DesignClassTag::try_from(class_tag.to_owned()).unwrap();
        scope.paired_class_tag = DesignClassTag::try_from(paired_class_tag.to_owned()).unwrap();
        scope
            .try_edit(|draft| {
                draft.frame_length = 421;
                draft.paired_byte_offset = 421;
                draft.reference_count_offset = 185;
                draft.reference_members = ReferenceRun::from_columns(
                    reference_members.to_vec(),
                    (0..11)
                        .map(|ordinal| u64::try_from(190 + ordinal * 11).expect("offset fits u64"))
                        .collect(),
                    "reference_members",
                )
                .unwrap();
                draft.feature_ordinal_offset = 334;
                draft.layout_fixture_references();
                draft.previous_history_state_id_offset =
                    draft.previous_history_state_id.map(|previous| {
                        draft.history_state_id.get_or_insert(previous + 1);
                        draft.feature_ordinal_offset + 30
                    });
            })
            .unwrap();

        let mut bytes = vec![0_u8; 421];
        bytes[185..189].copy_from_slice(&11_u32.to_le_bytes());
        for (ordinal, record_index) in reference_members.into_iter().enumerate() {
            let at = 189 + ordinal * 11;
            bytes[at] = 1;
            bytes[at + 1..at + 5].copy_from_slice(&record_index.to_le_bytes());
        }
        bytes[310..314].copy_from_slice(&u32::MAX.to_le_bytes());
        bytes[314..318].copy_from_slice(&8_u32.to_le_bytes());
        for (ordinal, value) in "As-built".encode_utf16().enumerate() {
            bytes[318 + ordinal * 2..320 + ordinal * 2].copy_from_slice(&value.to_le_bytes());
        }
        bytes[334..338].copy_from_slice(&2_u32.to_le_bytes());
        indexed_header(
            &mut bytes,
            paired_class_tag.as_bytes().try_into().unwrap(),
            scope_record_index,
        );
        let frame_start = bytes.len();
        let frame_class_tag = generation.frame_class_tag();
        indexed_header(
            &mut bytes,
            frame_class_tag.as_bytes().try_into().unwrap(),
            200,
        );
        let matrix_prefix = generation.matrix_prefix();
        let transform_offset = generation.matrix_offset();
        let frame_length = generation.frame_length();
        bytes.resize(frame_start + frame_length, 0);
        bytes[frame_start + matrix_prefix..frame_start + transform_offset]
            .copy_from_slice(&[1, 1, 0, 0]);
        let mut solved_transform = SketchPlacementMatrix::IDENTITY.rows();
        solved_transform[0][3] = 9.0;
        solved_transform[1][3] = 8.0;
        solved_transform[2][3] = 7.0;
        for (ordinal, value) in solved_transform.into_iter().flatten().enumerate() {
            let at = frame_start + transform_offset + ordinal * 8;
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        indexed_header(
            &mut bytes,
            paired_class_tag.as_bytes().try_into().unwrap(),
            200,
        );

        let (limit_first_value, limit_second_value) = if reverse_limit_order {
            (1.5, -1.0)
        } else {
            (-1.0, 1.5)
        };
        let owners = [
            owner(scope_record_index, 100, 0, owner_class, 1.0, 1_000),
            owner(scope_record_index, 101, 1, owner_class, 2.0, 1_001),
            owner(scope_record_index, 102, 2, owner_class, 3.0, 1_002),
            owner(scope_record_index, 103, 3, owner_class, 0.25, 1_003),
            owner(
                scope_record_index,
                105,
                4,
                owner_class,
                limit_first_value,
                1_005,
            ),
            owner(
                scope_record_index,
                106,
                5,
                owner_class,
                limit_second_value,
                1_006,
            ),
        ];
        let alignment = exact_assembly_alignment(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
            &owners,
        )
        .unwrap()
        .expect("exact 421-byte As-built alignment");
        assert!((alignment.angle() - 0.25).abs() <= EPS_EXACT_FIXTURE);
        for (actual, expected) in alignment.offset().into_iter().zip([1.0, 2.0, 3.0]) {
            assert!((actual - expected).abs() <= EPS_EXACT_FIXTURE);
        }
        assert_eq!(
            alignment
                .owners
                .iter()
                .map(|owner| owner.value)
                .collect::<Vec<_>>(),
            [103, 100, 101, 102]
        );
        assert_eq!(
            alignment
                .owners
                .iter()
                .map(|owner| owner.offset)
                .collect::<Vec<_>>(),
            [1_003, 1_000, 1_001, 1_002]
        );
        let limits = alignment.limits().expect("assembly limits");
        assert_eq!(limits.kind, expected_limit_kind);
        assert!((limits.minimum() - -1.0).abs() <= EPS_EXACT_FIXTURE);
        assert!((limits.maximum() - 1.5).abs() <= EPS_EXACT_FIXTURE);
        assert_eq!(
            limits.owner_record_indices,
            if reverse_limit_order {
                [106, 105]
            } else {
                [105, 106]
            }
        );
        assert_eq!(
            limits.value_offsets,
            if reverse_limit_order {
                [1_006, 1_005]
            } else {
                [1_005, 1_006]
            }
        );
        assert!(alignment.operand_frames().is_none());
        assert!(alignment.operand_paths().is_none());
        let solved_frame = alignment.solved_frame().expect("solved frame carrier");
        assert_eq!(solved_frame.reference_record_index, 200);
        assert_eq!(solved_frame.reference_offset, 190 + 8 * 11);
        assert_eq!(solved_frame.record_byte_offset, u64_from_index(frame_start));
        assert_eq!(solved_frame.class_tag.as_str(), frame_class_tag);
        assert!((solved_frame.transform[0][3] - 9.0).abs() <= EPS_EXACT_FIXTURE);
        assert_eq!(
            solved_frame.transform_offset,
            u64_from_index(frame_start + transform_offset)
        );
    }
}
