// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::scope::DesignScopePayload;
use crate::test_support::pipe::{Config, Fixture, Layout};

#[test]
fn pipe_source_uses_structural_prefixes_and_canonical_owners() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for legacy in [false, true] {
        for hollow in [false, true] {
            for renumber in [false, true] {
                for variant in [false, true] {
                    let fixture = Fixture::new(Config {
                        legacy,
                        hollow,
                        variant,
                        layout: Layout {
                            shift_lanes: true,
                            renumber,
                            ..Default::default()
                        },
                        ..Default::default()
                    });
                    let native = fixture.decode(&ctx).unwrap();
                    assert_eq!(native.design_parameter_scopes.len(), 1);
                    let scope = &native.design_parameter_scopes[0];
                    let DesignScopePayload::Pipe(Some(pipe)) = scope.payload() else {
                        panic!("complete Pipe source")
                    };
                    assert_eq!(pipe.is_hollow, hollow);
                    assert_eq!(pipe.operation_offset, if legacy { 26 } else { 22 });
                    assert_eq!(pipe.is_hollow_offset, pipe.operation_offset - 1);
                    assert_eq!(pipe.opaque_post_operation_offset, pipe.operation_offset + 4);
                    assert_eq!(pipe.opaque_post_operation, [1, 1]);
                    assert_eq!(pipe.record_indexes, [100, 103, 106, 109]);
                    for (ordinal, expected) in [1.0, 0.0, 0.6, 0.15].into_iter().enumerate() {
                        let owner = native
                            .design_parameter_owners
                            .iter()
                            .find(|owner| owner.record_index() == pipe.record_indexes[ordinal])
                            .unwrap();
                        assert_eq!(owner.evaluated_value().get(), expected);
                        assert_eq!(
                            owner.evaluated_value_offset(),
                            cadmpeg_core::decode::u64_from_index(fixture.lane_starts[ordinal] + 40)
                        );
                        assert_eq!(owner.local_ordinal(), u32::try_from(ordinal).unwrap());
                        assert_eq!(owner.frame_length(), if variant { 104 } else { 103 });
                    }
                }
            }
        }
    }
}

#[test]
fn pipe_source_preserves_all_control_values_and_raw_bytes() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for (values, hollow, operation) in [
        ([1.0, 0.0, 3.0, 0.2], true, 4),
        ([0.0, 1.0, 1.27, 0.0889], true, 4),
        ([1.0, 0.0, 1.0, 0.25], false, 4),
        ([1.0, 0.0, 2.0, 0.5], false, 1),
    ] {
        let native = Fixture::new(Config {
            hollow,
            operation,
            values,
            ..Default::default()
        })
        .decode(&ctx)
        .unwrap();
        let DesignScopePayload::Pipe(Some(pipe)) = native.design_parameter_scopes[0].payload()
        else {
            panic!("control-shaped source")
        };
        assert_eq!(pipe.is_hollow, hollow);
        assert_eq!(pipe.opaque_post_operation, [1, 1]);
        for (ordinal, id) in pipe.record_indexes.iter().enumerate() {
            let owner = native
                .design_parameter_owners
                .iter()
                .find(|owner| owner.record_index() == *id)
                .unwrap();
            assert_eq!(
                owner.evaluated_value().get().to_bits(),
                values[ordinal].to_bits()
            );
        }
    }
}

#[test]
fn pipe_source_requires_the_complete_scalar_and_owner_grammars() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for (offset, byte) in [
        (11, 1),
        (19, 0),
        (20, 0),
        (24, 0),
        (29, 1),
        (35, 1),
        (36, 1),
        (39, 1),
        (48, 0),
        (53, 1),
        (63, 1),
        (67, 0),
        (68, 11),
        (72, 1),
        (80, 0),
        (85, 1),
        (92, 0),
        (93, 11),
        (97, 1),
    ] {
        let mut fixture = Fixture::new(Config::default());
        fixture.bulk[fixture.lane_starts[0] + offset] = byte;
        let native = fixture.decode(&ctx).unwrap();
        assert!(
            matches!(
                native.design_parameter_scopes[0].payload(),
                DesignScopePayload::Pipe(None)
            ),
            "scalar mutation {offset}"
        );
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let native = Fixture::new(Config {
            values: [1.0, 0.0, value, 0.15],
            ..Default::default()
        })
        .decode(&ctx)
        .unwrap();
        assert!(matches!(
            native.design_parameter_scopes[0].payload(),
            DesignScopePayload::Pipe(None)
        ));
    }
    for offset in [79, 80, 81, 93] {
        let mut fixture = Fixture::new(Config {
            variant: true,
            ..Default::default()
        });
        fixture.bulk[fixture.lane_starts[0] + offset] = 3;
        let native = fixture.decode(&ctx).unwrap();
        assert!(
            matches!(
                native.design_parameter_scopes[0].payload(),
                DesignScopePayload::Pipe(None)
            ),
            "variant mutation {offset}"
        );
    }
}

#[test]
fn pipe_source_rejects_false_operation25_and_invalid_hollow() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for hollow in [2_u8, 255] {
        let mut fixture = Fixture::new(Config::default());
        fixture.bulk[21] = hollow;
        let native = fixture.decode(&ctx).unwrap();
        assert!(matches!(
            native.design_parameter_scopes[0].payload(),
            DesignScopePayload::Pipe(None)
        ));
    }
    let mut fixture = Fixture::new(Config::default());
    fixture.bulk[21..32].fill(0);
    fixture.bulk[25..29].copy_from_slice(&4_u32.to_le_bytes());
    let native = fixture.decode(&ctx).unwrap();
    assert!(matches!(
        native.design_parameter_scopes[0].payload(),
        DesignScopePayload::Pipe(None)
    ));
}

#[test]
fn pipe_source_preserves_optional_point_versions_and_shifted_links() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for version in [2, 3] {
        for legacy in [false, true] {
            let fixture = Fixture::new(Config {
                legacy,
                point_version: Some(version),
                layout: Layout {
                    shift_lanes: true,
                    ..Default::default()
                },
                ..Default::default()
            });
            let native = fixture.decode(&ctx).unwrap();
            let DesignScopePayload::Pipe(Some(pipe)) = native.design_parameter_scopes[0].payload()
            else {
                panic!("point Pipe")
            };
            let selection = pipe.point_selection.as_ref().unwrap();
            assert_eq!(
                selection.prefix_reference_offset,
                if legacy { 33 } else { 29 }
            );
            assert_eq!(selection.point_data.point_record_index, 500);
            assert_eq!(
                selection
                    .point_data
                    .position
                    .map(cadmpeg_ir::scalar::FiniteReal::get),
                [0.25, 0.5, 0.75]
            );
            assert_eq!(selection.point_data.rule.reference_type(), 1);
            assert_eq!(selection.point_data.rule.inputs().len(), 1);
            assert_eq!(selection.point_data.rule.inputs()[0].record_index(), 510);
        }
    }
}

#[test]
fn pipe_source_rejects_duplicate_swapped_and_unclassified_references() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for (ordinal, id) in [(0, 103_u32), (0, 200), (4, 100), (6, 500)] {
        let mut fixture = Fixture::new(Config::default());
        let at = fixture.table + 5 + 11 * ordinal;
        fixture.bulk[at..at + 4].copy_from_slice(&id.to_le_bytes());
        let native = fixture.decode(&ctx).unwrap();
        assert!(matches!(
            native.design_parameter_scopes[0].payload(),
            DesignScopePayload::Pipe(None)
        ));
    }
}

#[test]
fn pipe_source_requires_semantic_types_and_exact_indexed_boundaries() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for old in [
        super::pipe::SCALAR_GUID,
        super::pipe::SCALAR_BASE,
        super::pipe::PIPE_BASE,
    ] {
        let mut fixture = Fixture::new(Config::default());
        let at = fixture
            .meta
            .windows(old.len())
            .position(|bytes| bytes == old.as_bytes())
            .unwrap();
        fixture.meta[at] = b'0';
        let native = fixture.decode(&ctx).unwrap();
        assert!(matches!(
            native.design_parameter_scopes[0].payload(),
            DesignScopePayload::Pipe(None)
        ));
    }
    for primary in [false, true] {
        let mut fixture = Fixture::new(Config::default());
        let secondary_at = fixture.meta.len() - 20 - 16 * fixture.secondary.len();
        let table = if primary {
            secondary_at - 16 * fixture.records.len()
        } else {
            secondary_at + 4
        };
        let at = table + 16 + 8;
        let original = if primary {
            fixture.records[1].1
        } else {
            fixture.secondary[1].1
        };
        fixture.meta[at..at + 8].copy_from_slice(&(original + 1).to_le_bytes());
        let native = fixture.decode(&ctx).unwrap();
        assert!(matches!(
            native.design_parameter_scopes[0].payload(),
            DesignScopePayload::Pipe(None)
        ));
    }
}

/// Prepend `prefix` to the `BulkStream` and shift both `MetaStream` indexes past it.
fn prepend_indexed(fixture: &mut Fixture, mut prefix: Vec<u8>) {
    let secondary_at = fixture.meta.len() - 20 - 16 * fixture.secondary.len();
    let primary_at = secondary_at - 16 * fixture.records.len();
    let shift = cadmpeg_core::decode::u64_from_index(prefix.len());
    for (ordinal, (_, offset)) in fixture.records.iter_mut().enumerate() {
        *offset += shift;
        let at = primary_at + 16 * ordinal + 8;
        fixture.meta[at..at + 8].copy_from_slice(&offset.to_le_bytes());
    }
    for (ordinal, (_, offset)) in fixture.secondary.iter_mut().enumerate() {
        *offset += shift;
        let at = secondary_at + 4 + 16 * ordinal + 8;
        fixture.meta[at..at + 8].copy_from_slice(&offset.to_le_bytes());
    }
    prefix.extend_from_slice(&fixture.bulk);
    fixture.bulk = prefix;
}

#[test]
fn pipe_source_rejects_a_second_complete_same_id_scalar() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut fixture = Fixture::new(Config::default());
    let start = fixture.lane_starts[0];
    let duplicate = fixture.bulk[start..start + 122].to_vec();
    prepend_indexed(&mut fixture, duplicate);
    let native = fixture.decode(&ctx).unwrap();
    assert!(matches!(
        native.design_parameter_scopes[0].payload(),
        DesignScopePayload::Pipe(None)
    ));
}

#[test]
fn pipe_source_ignores_the_first_unreferenced_scalar_decoy() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut fixture = Fixture::new(Config::default());
    let start = fixture.lane_starts[0];
    let mut decoy = fixture.bulk[start..start + 122].to_vec();
    for (at, id) in [(7, 600_u32), (49, 601), (81, 602), (110, 600)] {
        decoy[at..at + 4].copy_from_slice(&id.to_le_bytes());
    }
    prepend_indexed(&mut fixture, decoy);
    let native = fixture.decode(&ctx).unwrap();
    let DesignScopePayload::Pipe(Some(pipe)) = native.design_parameter_scopes[0].payload() else {
        panic!("indexed relation survives an unreferenced first decoy")
    };
    assert_eq!(pipe.record_indexes, [100, 103, 106, 109]);
}

#[test]
fn pipe_source_rejects_an_extra_indexed_unclassified_reference() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let native = Fixture::new(Config {
        layout: Layout {
            extra_reference: true,
            ..Default::default()
        },
        ..Default::default()
    })
    .decode(&ctx)
    .unwrap();
    assert!(matches!(
        native.design_parameter_scopes[0].payload(),
        DesignScopePayload::Pipe(None)
    ));
}

#[test]
fn pipe_source_rejects_foreign_group_owner_and_nonzero_role_low_word() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for owner in [false, true] {
        let mut fixture = Fixture::new(Config::default());
        let start =
            usize::try_from(fixture.records.iter().find(|(id, _)| *id == 310).unwrap().1).unwrap();
        let paired = usize::try_from(
            fixture
                .secondary
                .iter()
                .find(|(id, _)| *id == 310)
                .unwrap()
                .1,
        )
        .unwrap();
        let at = if owner { paired - 10 } else { start + 42 };
        fixture.bulk[at..at + 4].copy_from_slice(&11_u32.to_le_bytes());
        let native = fixture.decode(&ctx).unwrap();
        assert!(matches!(
            native.design_parameter_scopes[0].payload(),
            DesignScopePayload::Pipe(None)
        ));
    }
}

#[test]
fn pipe_source_requires_both_point_links_and_the_single_input() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for mutation in 0..4 {
        let mut fixture = Fixture::new(Config {
            point_version: Some(2),
            ..Default::default()
        });
        let start =
            usize::try_from(fixture.records.iter().find(|(id, _)| *id == 500).unwrap().1).unwrap();
        let paired = usize::try_from(
            fixture
                .secondary
                .iter()
                .find(|(id, _)| *id == 500)
                .unwrap()
                .1,
        )
        .unwrap();
        match mutation {
            0 => fixture.bulk[29..33].copy_from_slice(&510_u32.to_le_bytes()),
            1 => fixture.bulk[paired - 10..paired - 6].copy_from_slice(&410_u32.to_le_bytes()),
            2 => fixture.bulk[paired - 15..paired - 11].copy_from_slice(&u32::MAX.to_le_bytes()),
            3 => fixture.bulk[start + 42..start + 50].copy_from_slice(&f64::NAN.to_le_bytes()),
            _ => unreachable!(),
        }
        let native = fixture.decode(&ctx).unwrap();
        assert!(
            matches!(
                native.design_parameter_scopes[0].payload(),
                DesignScopePayload::Pipe(None)
            ),
            "point mutation {mutation}"
        );
    }
}

#[test]
fn pipe_source_rejects_consistent_foreign_scalar_owners_and_reversed_groups() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut fixture = Fixture::new(Config::default());
    for relative in [25, 68, 93] {
        let at = fixture.lane_starts[0] + relative;
        fixture.bulk[at..at + 4].copy_from_slice(&11_u32.to_le_bytes());
    }
    let native = fixture.decode(&ctx).unwrap();
    assert!(matches!(
        native.design_parameter_scopes[0].payload(),
        DesignScopePayload::Pipe(None)
    ));
    let mut fixture = Fixture::new(Config {
        operation: 1,
        ..Default::default()
    });
    for (ordinal, id) in [(5, 310_u32), (6, 410), (7, 300), (8, 400)] {
        let at = fixture.table + 5 + 11 * ordinal;
        fixture.bulk[at..at + 4].copy_from_slice(&id.to_le_bytes());
    }
    let native = fixture.decode(&ctx).unwrap();
    assert!(matches!(
        native.design_parameter_scopes[0].payload(),
        DesignScopePayload::Pipe(None)
    ));
}

#[test]
fn pipe_point_input_count_rejects_an_unbounded_run() {
    let mut fixture = Fixture::new(Config {
        point_version: Some(3),
        ..Default::default()
    });
    let paired = usize::try_from(
        fixture
            .secondary
            .iter()
            .find(|(id, _)| *id == 500)
            .unwrap()
            .1,
    )
    .unwrap();
    fixture.bulk[paired - 15..paired - 11].copy_from_slice(&u32::MAX.to_le_bytes());
    let records = crate::design::decode::sketch::IndexedRecordOffsets::build(&fixture.bulk);
    let types = std::collections::HashMap::from([(500_u64, (super::pipe::POINT_GUID, 3_u32))]);
    assert!(super::exact_point_data_construction(
        &fixture.bulk,
        &records,
        std::iter::once(&500_u32),
        &types
    )
    .is_none());
}

#[test]
fn pipe_prefixes_keep_independent_hollow_and_opaque_fields() {
    use crate::records::feature::extrude::DesignExtrudeOperation;
    let controls: &[(&str, bool, DesignExtrudeOperation, usize)] = &[
        (
            "0300000033343011020000000000000000000000010000000001040000000101",
            true,
            DesignExtrudeOperation::NewBody,
            26,
        ),
        (
            "0300000032383607010000000000000000000000010000000001040000000101",
            true,
            DesignExtrudeOperation::NewBody,
            26,
        ),
        (
            "030000003338377a020000000000000000000000000004000000010101850200",
            false,
            DesignExtrudeOperation::NewBody,
            22,
        ),
        (
            "03000000333837bf020000000000000000000000000001000000010101d10200",
            false,
            DesignExtrudeOperation::Join,
            22,
        ),
    ];
    for (encoded, hollow, operation, operation_at) in controls {
        let raw: cadmpeg_ir::native::bytes::NativeBytes<Vec<u8>> =
            serde_json::from_value(serde_json::json!(encoded)).unwrap();
        let mut bytes = vec![0; 17];
        bytes.extend_from_slice(&raw);
        let actual = super::exact_pipe_prefix(&bytes, 17).unwrap();
        assert_eq!(actual.operation, *operation);
        assert_eq!(
            actual.operation_offset,
            u64::try_from(17 + operation_at).unwrap()
        );
        assert_eq!(actual.is_hollow, *hollow);
        assert_eq!(
            actual.is_hollow_offset,
            u64::try_from(16 + operation_at).unwrap()
        );
        assert_eq!(actual.opaque_post_operation, [1, 1]);
        assert_eq!(
            actual.opaque_post_operation_offset,
            u64::try_from(21 + operation_at).unwrap()
        );
        let mut altered = bytes.clone();
        altered[17 + operation_at + 4..17 + operation_at + 6].copy_from_slice(&[0, 255]);
        assert_eq!(
            super::exact_pipe_prefix(&altered, 17)
                .unwrap()
                .opaque_post_operation,
            [0, 255]
        );
        for (at, value) in [(17 + operation_at - 1, 2), (28, 1), (17 + operation_at, 0)] {
            let mut invalid = bytes.clone();
            invalid[at] = value;
            assert!(super::exact_pipe_prefix(&invalid, 17).is_none());
        }
        assert!(super::exact_pipe_prefix(&bytes[..17 + operation_at + 5], 17).is_none());
    }
    let mut unobserved = vec![0; 31];
    unobserved[25..29].copy_from_slice(&4u32.to_le_bytes());
    unobserved[29..31].copy_from_slice(&[1, 1]);
    assert!(super::exact_pipe_prefix(&unobserved, 0).is_none());
}
