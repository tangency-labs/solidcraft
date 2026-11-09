// SPDX-License-Identifier: Apache-2.0
//! Operation offsets and extent-slot spans of reference-aware Extrude scopes.

use super::super::{direct_extrude_operation_valid, extent_slot_expansion_valid};
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::extrude::{
    DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudeStart,
};
use crate::records::feature::scope::{
    DesignExtrudeScope, DesignFeatureKind, DesignParameterScope, DesignScopePayload,
};
use crate::records::identity::ReferenceRun;
use crate::test_support::native_test::f3d_native_mut;

const STREAM: &str = "f3d:Design/BulkStream.dat";
const SCOPE_ID: &str = "f3d:Design/BulkStream.dat:design-parameter-scope#0";

#[test]
fn plain_scope_starts_its_operation_at_24_or_28() {
    assert!(direct_extrude_operation_valid(100, false, 124));
    assert!(direct_extrude_operation_valid(100, false, 128));
    for operation in [122, 123, 125, 126, 127, 129] {
        assert!(!direct_extrude_operation_valid(100, false, operation));
    }
}

/// A plain reference-aware Extrude scope whose operation fields start at
/// `operation_offset`, with seven absent extent slots and a one-sided
/// distance extent.
fn plain_one_sided_scope(operation_offset: u64) -> DesignParameterScope {
    let first_side = operation_offset + 49;
    let mut scope = DesignParameterScope::empty(SCOPE_ID, DesignFeatureKind::Extrude, 3970);
    scope.class_tag = "375".to_owned().try_into().unwrap();
    scope.paired_class_tag = "260".to_owned().try_into().unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 400;
            draft.paired_byte_offset = 400;
            draft.reference_count_offset = 200;
            draft.reference_members = ReferenceRun::from_columns(
                vec![11, 22, 33],
                vec![205, 216, 227],
                "reference_members",
            )
            .unwrap();
            draft.kind_offset = 245;
            draft.feature_ordinal_offset = 258;
            draft.payload = DesignScopePayload::Extrude(Some(DesignExtrudeScope {
                extrude_prologue: Some(DesignExtrudePrologue::ReferenceAware {
                    reference: None,
                    operation: DesignExtrudeOperation::Join,
                    operation_offset,
                    direction_face_extend_values: [1, 2],
                    side_extent_discriminators: [1, 0],
                    side_extent_discriminator_offsets: [first_side, first_side + 13],
                    first_side_target_ordinal: None,
                    distance_tail: None,
                    extent: DesignExtrudeExtent::OneSidedDistance,
                    direction_face_extend_offsets: [operation_offset + 4, operation_offset + 8],
                    direction_reversed: false,
                    direction_reversed_offset: operation_offset + 12,
                    solid_operation: true,
                    solid_operation_offset: operation_offset + 13,
                    start: DesignExtrudeStart::ProfilePlane,
                    start_offset: operation_offset + 14,
                }),
                ..DesignExtrudeScope::default()
            }));
        })
        .unwrap();
    scope
}

fn has_invalid_frame(scope: DesignParameterScope) -> bool {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    {
        let mut native = f3d_native_mut(&mut ir);
        for record_index in scope
            .reference_members()
            .values()
            .copied()
            .chain(std::iter::once(scope.record_index))
        {
            native.design_record_headers.push(DesignRecordHeader {
                id: format!("{STREAM}:design-record-header#{record_index}"),
                record_index,
                class_tag: scope.class_tag.clone(),
                byte_offset: 0,
            });
        }
        native.design_parameter_scopes.push(scope);
    }
    crate::test_support::with_decode_context(|ctx| crate::validate::validate_native(ctx, &ir))
        .unwrap()
        .iter()
        .any(|finding| {
            finding.entity.as_deref() == Some(SCOPE_ID)
                && finding.message == "Fusion Design parameter scope has an invalid paired frame"
        })
}

#[test]
fn plain_scope_frame_admits_operation_at_24_or_28() {
    assert!(!has_invalid_frame(plain_one_sided_scope(24)));
    assert!(!has_invalid_frame(plain_one_sided_scope(28)));
    for operation_offset in [22, 26] {
        assert!(has_invalid_frame(plain_one_sided_scope(operation_offset)));
    }
}

#[test]
fn typed_scope_starts_its_operation_at_22_or_26() {
    assert!(direct_extrude_operation_valid(100, true, 122));
    assert!(direct_extrude_operation_valid(100, true, 126));
    for operation in [121, 123, 125, 127, 128] {
        assert!(!direct_extrude_operation_valid(100, true, operation));
    }
}

#[test]
fn operation_offset_overflow_is_refused() {
    assert!(!direct_extrude_operation_valid(u64::MAX, false, 27));
    assert!(!direct_extrude_operation_valid(u64::MAX, true, 21));
}

#[test]
fn plain_slots_widen_the_span_by_ten_bytes() {
    for slots in 0..=7 {
        assert!(extent_slot_expansion_valid(false, 10 * slots));
    }
    assert!(!extent_slot_expansion_valid(false, 80));
    assert!(!extent_slot_expansion_valid(false, 5));
    assert!(!extent_slot_expansion_valid(false, 50 + 1));
}

#[test]
fn typed_slots_widen_the_span_by_fifty_bytes() {
    for slots in 0..=7 {
        assert!(extent_slot_expansion_valid(true, 50 * slots));
    }
    assert!(!extent_slot_expansion_valid(true, 400));
    assert!(!extent_slot_expansion_valid(true, 10));
    assert!(!extent_slot_expansion_valid(true, 149));
}

#[test]
fn class_349_reference_aware_prefix_keeps_its_frame_rule() {
    use crate::layout::class_349_two_sided_through_all_extrude_frame as layout;
    use crate::records::feature::extrude::DesignExtrudePrologueReference;

    let mut scope = plain_one_sided_scope(37);
    scope.class_tag = "349".to_owned().try_into().unwrap();
    scope.paired_class_tag = "262".to_owned().try_into().unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = cadmpeg_core::decode::u64_from_index(layout::LEN);
            draft.paired_byte_offset = draft.frame_length;
            draft.reference_count_offset =
                cadmpeg_core::decode::u64_from_index(layout::REFERENCE_COUNT);
            draft.reference_members =
                ReferenceRun::unlocated(vec![11, 22, 33, 44, 55, 66, 77, 88, 99, 111, 122]);
            draft.layout_fixture_references();
            draft.feature_ordinal_offset =
                cadmpeg_core::decode::u64_from_index(layout::FEATURE_ORDINAL);
            if let DesignScopePayload::Extrude(Some(extrude)) = &mut draft.payload {
                if let Some(DesignExtrudePrologue::ReferenceAware {
                    reference,
                    start,
                    side_extent_discriminator_offsets,
                    ..
                }) = &mut extrude.extrude_prologue
                {
                    *reference = Some(DesignExtrudePrologueReference {
                        record_index: 11,
                        record_index_offset: 26,
                        trailing_zero_count: 7,
                        operation_prefix_marker_offset: None,
                    });
                    *start = DesignExtrudeStart::FromFace;
                    *side_extent_discriminator_offsets = [126, 139];
                }
            }
        })
        .unwrap();
    assert!(!has_invalid_frame(scope));
}
