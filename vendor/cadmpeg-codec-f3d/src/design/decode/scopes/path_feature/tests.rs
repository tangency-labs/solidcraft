// SPDX-License-Identifier: Apache-2.0
use super::exact_path_feature_construction;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::path_features::DesignPathFeatureConstruction;
use crate::records::feature::scope::DesignParameterScope;
use crate::test_support::indexed_header;
use cadmpeg_core::decode::u64_from_index;

#[test]
fn compact_loft_prefix_reads_operation_at_offset_25_for_any_dynamic_class_tag() {
    for class_tag in ["301", "449"] {
        let mut bytes = Vec::new();
        let class_tag_bytes = class_tag
            .as_bytes()
            .try_into()
            .expect("three-byte class tag");
        indexed_header(&mut bytes, class_tag_bytes, 20);
        bytes.resize(128, 0);
        bytes[21..25].fill(1);
        bytes[25..29].copy_from_slice(&1u32.to_le_bytes());
        bytes[30..34].fill(0xff);

        let mut scope = DesignParameterScope::empty(
            "generated:loft#20",
            crate::records::feature::scope::DesignFeatureKind::Loft,
            20,
        );
        scope.class_tag =
            crate::records::references::DesignClassTag::try_from(class_tag.to_owned()).unwrap();
        scope
            .try_edit(|draft| {
                draft.frame_length = 128;
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.layout_fixture_tail();
            })
            .unwrap();
        let construction = exact_path_feature_construction(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
            &[],
        )
        .expect("compact Loft operation");
        assert_eq!(
            construction,
            DesignPathFeatureConstruction::Loft(
                crate::records::feature::path_features::DesignLoftConstruction {
                    operation: DesignExtrudeOperation::Join,
                    operation_offset: 25,
                }
            )
        );

        bytes[24] = 0;
        assert_eq!(
            exact_path_feature_construction(
                &bytes,
                &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
                &scope,
                &[],
            ),
            None
        );
    }
}

#[test]
fn two_lane_sweep_scope_from_corpus_admits_exact_along_and_against_distances() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let corpus_path = manifest_dir.join("../../corpus/1WireRGBWController.f3d");
    if !corpus_path.exists() {
        return;
    }
    let file_bytes = std::fs::read(&corpus_path).expect("read corpus file");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&file_bytes, &arena, &policy).unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let view = archive
        .open(&ctx, "FusionAssetName[Active]/Design1/BulkStream.dat")
        .unwrap();
    let bytes = view.unread().to_vec();

    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let mut scope = DesignParameterScope::empty(
        "f3d:FusionAssetName[Active]/Design1/BulkStream.dat:design-parameter-scope#97558",
        crate::records::feature::scope::DesignFeatureKind::Sweep,
        97558,
    );
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("372".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("260".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.byte_offset = 1_148_744;
            draft.frame_length = 433;
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                97559, 97562, 97566, 97569, 97573, 97576, 97580, 97583, 97586,
            ]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();

    let construction = exact_path_feature_construction(&bytes, &records, &scope, &[])
        .expect("two-lane sweep construction from corpus bytes");
    assert_eq!(
        construction,
        DesignPathFeatureConstruction::Sweep(
            crate::records::feature::path_features::DesignSweepConstruction {
                operation: DesignExtrudeOperation::Join,
                operation_offset: 1_148_765,
                values: vec![
                    cadmpeg_ir::scalar::FiniteReal::new(1.0).unwrap(),
                    cadmpeg_ir::scalar::FiniteReal::new(0.0).unwrap(),
                ],
                record_indexes: vec![97580, 97583],
                value_offsets: vec![1_152_647, 1_152_971],
            }
        )
    );
}

#[test]
fn compact_two_lane_sweep_pins_admission_rules_and_rejects_mutations() {
    let sweep_start = 200usize;
    let mut bytes = vec![0; sweep_start];
    indexed_header(&mut bytes, *b"372", 90);
    bytes.resize(sweep_start + 433, 0);
    bytes[sweep_start + 21..sweep_start + 25].copy_from_slice(&1u32.to_le_bytes());
    bytes[sweep_start + 25] = 1;

    let sweep_values = [1.0f64, 0.0f64];
    let scalar_indices = [91u32, 92u32];
    let mut value_offsets = Vec::new();
    for (ordinal, (record_index, value)) in scalar_indices.into_iter().zip(sweep_values).enumerate()
    {
        let scalar_start = bytes.len();
        let mut scalar = vec![0; 100];
        scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
        scalar[4..7].copy_from_slice(b"277");
        scalar[7..11].copy_from_slice(&record_index.to_le_bytes());
        scalar[19..24].copy_from_slice(&[1, 1, 0, 0, 0]);
        scalar[24] = 1;
        scalar[25..29].copy_from_slice(&90u32.to_le_bytes());
        scalar[35] = u8::try_from(ordinal).expect("u8 ordinal");
        scalar[40..48].copy_from_slice(&value.to_le_bytes());
        scalar.extend_from_slice(&3u32.to_le_bytes());
        scalar.extend_from_slice(b"261");
        scalar.extend_from_slice(&record_index.to_le_bytes());
        value_offsets.push(u64_from_index(scalar_start) + 40);
        bytes.extend_from_slice(&scalar);
    }

    let make_scope = || {
        let mut scope = DesignParameterScope::empty(
            "generated:sweep#90",
            crate::records::feature::scope::DesignFeatureKind::Sweep,
            90,
        );
        scope.class_tag =
            crate::records::references::DesignClassTag::try_from("372".to_owned()).unwrap();
        scope.paired_class_tag =
            crate::records::references::DesignClassTag::try_from("260".to_owned()).unwrap();
        scope
            .try_edit(|draft| {
                draft.byte_offset = u64_from_index(sweep_start);
                draft.frame_length = 433;
                draft.reference_count_offset = draft.byte_offset + 9;
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                    1, 2, 3, 4, 5, 6, 7, 91, 92,
                ]);
                draft.layout_fixture_references();
                draft.layout_fixture_tail();
            })
            .unwrap();
        scope
    };

    let scope = make_scope();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let construction = exact_path_feature_construction(&bytes, &records, &scope, &[])
        .expect("exact 2-lane sweep construction");
    assert_eq!(
        construction,
        DesignPathFeatureConstruction::Sweep(
            crate::records::feature::path_features::DesignSweepConstruction {
                operation: DesignExtrudeOperation::Join,
                operation_offset: u64_from_index(sweep_start + 21),
                values: vec![
                    cadmpeg_ir::scalar::FiniteReal::new(1.0).unwrap(),
                    cadmpeg_ir::scalar::FiniteReal::new(0.0).unwrap(),
                ],
                record_indexes: vec![91, 92],
                value_offsets: value_offsets.clone(),
            }
        )
    );

    // Sentinel at offset 25 != 1
    let mut mutated = bytes.clone();
    mutated[sweep_start + 25] = 0;
    assert_eq!(
        exact_path_feature_construction(&mutated, &records, &scope, &[]),
        None
    );
    mutated[sweep_start + 25] = 2;
    assert_eq!(
        exact_path_feature_construction(&mutated, &records, &scope, &[]),
        None
    );

    // Operation at offset 21 invalid
    let mut mutated = bytes.clone();
    mutated[sweep_start + 21] = 0;
    assert_eq!(
        exact_path_feature_construction(&mutated, &records, &scope, &[]),
        None
    );
    mutated[sweep_start + 21] = 5;
    assert_eq!(
        exact_path_feature_construction(&mutated, &records, &scope, &[]),
        None
    );

    // Non-zero byte in offsets 11..21
    let mut mutated = bytes.clone();
    mutated[sweep_start + 15] = 1;
    assert_eq!(
        exact_path_feature_construction(&mutated, &records, &scope, &[]),
        None
    );

    // Class tag mismatch
    let mut mutated_scope = make_scope();
    mutated_scope.class_tag =
        crate::records::references::DesignClassTag::try_from("373".to_owned()).unwrap();
    assert_eq!(
        exact_path_feature_construction(&bytes, &records, &mutated_scope, &[]),
        None
    );

    // Paired class tag mismatch
    let mut mutated_scope = make_scope();
    mutated_scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("261".to_owned()).unwrap();
    assert_eq!(
        exact_path_feature_construction(&bytes, &records, &mutated_scope, &[]),
        None
    );

    // Frame length mismatch (payload length != 423)
    let mut mutated_scope = make_scope();
    mutated_scope
        .try_edit(|draft| {
            draft.frame_length = 434;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_path_feature_construction(&bytes, &records, &mutated_scope, &[]),
        None
    );

    // Member count mismatch != 9
    let mut mutated_scope = make_scope();
    mutated_scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![1, 2, 3, 4, 5, 6, 91, 92]);
            draft.layout_fixture_references();
        })
        .unwrap();
    assert_eq!(
        exact_path_feature_construction(&bytes, &records, &mutated_scope, &[]),
        None
    );

    // Non-[0, 1] scalar ordinals (e.g. ordinal 2 on second scalar)
    let mut mutated = bytes.clone();
    let second_scalar_offset = usize::try_from(value_offsets[1] - 40).expect("usize offset");
    mutated[second_scalar_offset + 35] = 2;
    let mutated_records = crate::design::test_support::indexed_record_offsets_for_test(&mutated);
    assert_eq!(
        exact_path_feature_construction(&mutated, &mutated_records, &scope, &[]),
        None
    );
}
