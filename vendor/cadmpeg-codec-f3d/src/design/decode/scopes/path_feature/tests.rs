// SPDX-License-Identifier: Apache-2.0
use super::{exact_path_feature_construction, loft_reference_loss, three_flag_loft_settings_loss};
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::path_features::DesignPathFeatureConstruction;
use crate::records::feature::scope::DesignParameterScope;
use crate::test_support::indexed_header;
use cadmpeg_core::decode::u64_from_index;

fn loft_prefix(operation_offset: usize, standard: bool, three_flags: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"449", 20);
    bytes.resize(128, 0);
    if standard {
        bytes[20] = 1;
    } else if three_flags {
        bytes[21..24].copy_from_slice(&[1, 1, 0]);
    } else {
        bytes[21..25].fill(1);
    }
    bytes[operation_offset..operation_offset + 4].copy_from_slice(&1u32.to_le_bytes());
    bytes[operation_offset + 5..operation_offset + 9].fill(0xff);
    bytes
}

fn loft_scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "generated:loft#20",
        crate::records::feature::scope::DesignFeatureKind::Loft,
        20,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 128;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

fn decoded_loft(
    bytes: &[u8],
) -> Option<crate::records::feature::path_features::DesignLoftConstruction> {
    match exact_path_feature_construction(
        bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(bytes),
        &loft_scope(),
        &[],
    )? {
        DesignPathFeatureConstruction::Loft(construction) => Some(construction),
        _ => None,
    }
}

fn assert_loft_operation(bytes: &[u8], offset: u64) {
    assert_eq!(
        decoded_loft(bytes),
        Some(
            crate::records::feature::path_features::DesignLoftConstruction {
                operation: DesignExtrudeOperation::Join,
                operation_offset: offset,
                reference_target: None,
            }
        )
    );
}

#[test]
fn compact_loft_reads_operation() {
    assert_loft_operation(&loft_prefix(25, false, false), 25);
}

#[test]
fn three_flag_compact_loft_reads_operation() {
    let mut bytes = loft_prefix(24, false, true);
    assert_loft_operation(&bytes, 24);
    bytes[23] = 1;
    assert_loft_operation(&bytes, 24);
}

#[test]
fn standard_loft_reads_operation() {
    assert_loft_operation(&loft_prefix(29, true, false), 29);
}

#[test]
fn three_flag_standard_loft_reads_operation() {
    assert_loft_operation(&loft_prefix(28, true, true), 28);
}

#[test]
fn standard_loft_reads_marked_group_reference() {
    let mut bytes = loft_prefix(29, true, false);
    bytes[33..38].copy_from_slice(&[0, 1, 0, 0, 0]);
    assert_loft_operation(&bytes, 29);

    bytes[33] = 1;
    bytes[34..42].copy_from_slice(&16015u64.to_le_bytes());
    bytes[42..48].copy_from_slice(&[0, 0, 0xff, 0xff, 0xff, 0xff]);
    let construction = decoded_loft(&bytes).expect("standard Loft with a marked reference");
    assert_eq!(
        construction,
        crate::records::feature::path_features::DesignLoftConstruction {
            operation: DesignExtrudeOperation::Join,
            operation_offset: 29,
            reference_target: Some(16015),
        }
    );
    let loss = loft_reference_loss(&loft_scope(), &construction).expect("reference loss");
    assert_eq!(
        loss.code,
        crate::loss::F3dLossCode::FeatureLoftReferenceUnresolved.kind()
    );
    assert_eq!(
        loss.message,
        "Loft scope generated:loft#20 stores a non-null marked reference with target 16015 at prefix offset 33; the target and the Loft setting it selects are unresolved (DR-268), and a Loft projected from the scope uses closed false, solid true, ruled false, and linearize false."
    );

    let scope = DesignParameterScope::empty(
        "generated:loft#20",
        crate::records::feature::scope::DesignFeatureKind::Loft,
        20,
    );
    for reference in [
        [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [1, 0x8f, 0x3e, 0, 0, 0, 0, 0, 0, 1, 0],
        [2, 0x8f, 0x3e, 0, 0, 0, 0, 0, 0, 0, 0],
    ] {
        bytes[33..44].copy_from_slice(&reference);
        assert_eq!(
            exact_path_feature_construction(
                &bytes,
                &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
                &scope,
                &[]
            ),
            None
        );
    }
}

#[test]
fn null_reference_loft_has_no_reference_loss() {
    let construction = decoded_loft(&loft_prefix(29, true, false)).expect("standard Loft");
    assert_eq!(loft_reference_loss(&loft_scope(), &construction), None);
}

#[test]
fn three_flag_standard_loft_with_zero_byte_33_reads_operation_at_28() {
    for (value, operation) in [
        (1u32, DesignExtrudeOperation::Join),
        (2, DesignExtrudeOperation::Cut),
        (3, DesignExtrudeOperation::Intersect),
        (4, DesignExtrudeOperation::NewBody),
    ] {
        let mut bytes = loft_prefix(28, true, true);
        bytes[28..32].copy_from_slice(&value.to_le_bytes());
        bytes[32..34].fill(0);
        assert_eq!(
            decoded_loft(&bytes),
            Some(
                crate::records::feature::path_features::DesignLoftConstruction {
                    operation,
                    operation_offset: 28,
                    reference_target: None,
                }
            )
        );
    }
}

#[test]
fn three_flag_standard_loft_requires_null_reference() {
    let mut bytes = loft_prefix(28, true, true);
    bytes[32] = 0;
    assert_loft_operation(&bytes, 28);
    bytes[32] = 1;
    let scope = DesignParameterScope::empty(
        "generated:loft#20",
        crate::records::feature::scope::DesignFeatureKind::Loft,
        20,
    );
    assert_eq!(
        exact_path_feature_construction(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
            &[]
        ),
        None
    );
}

#[test]
fn loft_forms_reject_invalid_operations() {
    for (offset, standard, three_flags) in [
        (25, false, false),
        (24, false, true),
        (29, true, false),
        (28, true, true),
    ] {
        let mut bytes = loft_prefix(offset, standard, three_flags);
        let scope = DesignParameterScope::empty(
            "generated:loft#20",
            crate::records::feature::scope::DesignFeatureKind::Loft,
            20,
        );
        for value in [0u32, 5] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert_eq!(
                exact_path_feature_construction(
                    &bytes,
                    &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
                    &scope,
                    &[]
                ),
                None
            );
        }
    }
}

#[test]
fn unrecognized_loft_has_no_construction() {
    let mut bytes = loft_prefix(25, false, false);
    bytes[21] = 2;
    let scope = DesignParameterScope::empty(
        "generated:loft#20",
        crate::records::feature::scope::DesignFeatureKind::Loft,
        20,
    );
    assert_eq!(
        exact_path_feature_construction(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
            &[]
        ),
        None
    );
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

fn settings_loss_message(bytes: &[u8]) -> Option<String> {
    let construction = decoded_loft(bytes).expect("Loft construction");
    three_flag_loft_settings_loss(bytes, &loft_scope(), &construction).map(|loss| {
        assert_eq!(
            loss.code,
            crate::loss::F3dLossCode::FeatureLoftSettingsAssumed.kind()
        );
        loss.message
    })
}

#[test]
fn three_flag_compact_loft_reports_assumed_settings() {
    let mut bytes = loft_prefix(24, false, true);
    assert_loft_operation(&bytes, 24);
    assert_eq!(
        settings_loss_message(&bytes).as_deref(),
        Some("Loft scope generated:loft#20 stores the three-flag compact prefix with flag bytes 01 01 00 at prefix offset 21; the Loft settings these flags select are unresolved (DR-294), and a Loft projected from the scope uses closed false, solid true, ruled false, and linearize false.")
    );
    bytes[23] = 1;
    assert_loft_operation(&bytes, 24);
    assert_eq!(
        settings_loss_message(&bytes).as_deref(),
        Some("Loft scope generated:loft#20 stores the three-flag compact prefix with flag bytes 01 01 01 at prefix offset 21; the Loft settings these flags select are unresolved (DR-294), and a Loft projected from the scope uses closed false, solid true, ruled false, and linearize false.")
    );
}

#[test]
fn three_flag_standard_loft_reports_assumed_settings() {
    let mut bytes = loft_prefix(28, true, true);
    bytes[25..28].copy_from_slice(&[1, 0, 1]);
    assert_loft_operation(&bytes, 28);
    assert_eq!(
        settings_loss_message(&bytes).as_deref(),
        Some("Loft scope generated:loft#20 stores the three-flag standard prefix with flag bytes 01 00 01 at prefix offset 25; the Loft settings these flags select are unresolved (DR-294), and a Loft projected from the scope uses closed false, solid true, ruled false, and linearize false.")
    );
}

#[test]
fn four_flag_loft_forms_have_no_settings_loss() {
    assert_eq!(settings_loss_message(&loft_prefix(25, false, false)), None);
    assert_eq!(settings_loss_message(&loft_prefix(29, true, false)), None);
}

/// A six-lane Sweep scope at offset 200 with its six fixed scalar records, and
/// the scalar value offsets.
fn six_lane_sweep_bytes(prologue: &[(usize, u8)]) -> (Vec<u8>, Vec<u64>) {
    let sweep_start = 200usize;
    let mut bytes = vec![0; sweep_start];
    indexed_header(&mut bytes, *b"277", 90);
    bytes.resize(sweep_start + 523, 0);
    for (offset, value) in prologue {
        bytes[sweep_start + offset] = *value;
    }
    let mut value_offsets = Vec::new();
    for (ordinal, record_index) in (91u32..97).enumerate() {
        let scalar_start = bytes.len();
        let mut scalar = vec![0; 100];
        scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
        scalar[4..7].copy_from_slice(b"311");
        scalar[7..11].copy_from_slice(&record_index.to_le_bytes());
        scalar[19..24].copy_from_slice(&[1, 1, 0, 0, 0]);
        scalar[24] = 1;
        scalar[25..29].copy_from_slice(&90u32.to_le_bytes());
        scalar[35] = u8::try_from(ordinal).expect("u8 ordinal");
        scalar[40..48].copy_from_slice(&1.0f64.to_le_bytes());
        scalar.extend_from_slice(&3u32.to_le_bytes());
        scalar.extend_from_slice(b"263");
        scalar.extend_from_slice(&record_index.to_le_bytes());
        value_offsets.push(u64_from_index(scalar_start) + 40);
        bytes.extend_from_slice(&scalar);
    }
    (bytes, value_offsets)
}

fn six_lane_sweep_scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "generated:sweep#90",
        crate::records::feature::scope::DesignFeatureKind::Sweep,
        90,
    );
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("277".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("263".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.byte_offset = 200;
            draft.frame_length = 523;
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                1, 2, 3, 4, 5, 6, 7, 91, 92, 93, 94, 95, 96,
            ]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

fn six_lane_sweep_operation(prologue: &[(usize, u8)]) -> Option<(DesignExtrudeOperation, u64)> {
    let (bytes, value_offsets) = six_lane_sweep_bytes(prologue);
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    match exact_path_feature_construction(&bytes, &records, &six_lane_sweep_scope(), &[])? {
        DesignPathFeatureConstruction::Sweep(sweep) => {
            assert_eq!(sweep.record_indexes, (91..97).collect::<Vec<_>>());
            assert_eq!(sweep.value_offsets, value_offsets);
            Some((sweep.operation, sweep.operation_offset))
        }
        _ => None,
    }
}

#[test]
fn six_lane_sweep_reads_the_operation_of_its_prologue_form() {
    assert_eq!(
        six_lane_sweep_operation(&[(20, 1), (25, 2), (29, 1)]),
        Some((DesignExtrudeOperation::Cut, 225))
    );
    assert_eq!(
        six_lane_sweep_operation(&[(21, 2), (25, 1), (26, 3)]),
        Some((DesignExtrudeOperation::Cut, 221))
    );
    assert_eq!(
        six_lane_sweep_operation(&[(21, 1), (25, 1), (26, 3)]),
        Some((DesignExtrudeOperation::Join, 221))
    );
    assert_eq!(six_lane_sweep_operation(&[(21, 5), (25, 1)]), None);
    assert_eq!(
        six_lane_sweep_operation(&[(15, 1), (21, 2), (25, 1)]),
        Some((DesignExtrudeOperation::Join, 225))
    );
}
