// SPDX-License-Identifier: Apache-2.0

use crate::records::dimensions::{DesignDimensionLocus, DesignDimensionLocusGroup};
use crate::records::identity::Located;

fn locus_group_with_saturated_offsets(base: u64, count: usize) -> DesignDimensionLocusGroup {
    let loci_start = base.saturating_add(24);
    let owner_start = loci_start.saturating_add(cadmpeg_core::decode::u64_from_index(count) * 15);
    let returns_start = owner_start.saturating_add(24);
    let end = returns_start
        .saturating_add(cadmpeg_core::decode::u64_from_index(count) * 11)
        .saturating_add(1);
    DesignDimensionLocusGroup {
        id: "f3d:Design/BulkStream.dat:locus-group#9".into(),
        companion_record_index: 8,
        byte_offset: base,
        class_tag: "256".to_owned().try_into().unwrap(),
        record_index: 9,
        frame_length: end - base,
        loci: (0..count)
            .map(|ordinal| {
                let ordinal = cadmpeg_core::decode::u64_from_index(ordinal);
                let start = loci_start.saturating_add(ordinal * 15);
                DesignDimensionLocus {
                    returned: Located {
                        value: 10,
                        offset: returns_start.saturating_add(ordinal * 11).saturating_add(1),
                    },
                    geometry_record_index: 10,
                    geometry_reference_offset: start.saturating_add(1),
                    role: 1,
                    role_offset: start.saturating_add(11),
                }
            })
            .collect(),
        owner_reference: 4,
        owner_reference_offset: owner_start.saturating_add(2),
        owner_role: 1,
        owner_role_offset: owner_start.saturating_add(12),
        state: 1,
        state_offset: owner_start.saturating_add(16),
        next_class_tag: "259".to_owned().try_into().unwrap(),
        next_record_index: 11,
        next_byte_offset: end,
    }
}

fn locus_findings(group: DesignDimensionLocusGroup) -> Vec<cadmpeg_ir::report::check::Finding> {
    crate::test_support::with_decode_context(|decode| {
        use crate::records::entity_header::{
            DesignEntityHeader, DesignEntityRegistration, DESIGN_MODULE_SKETCH,
        };
        use crate::records::identity::{DesignEntityId, ReferenceRun};
        use crate::records::parameters::{
            DesignParameter, DesignParameterCompanion, DesignParameterDraft, DesignParameterOwner,
            DesignParameterOwnerWire, DesignParameterSource,
        };
        let parameter = DesignParameter::try_from(DesignParameterDraft {
            id: "f3d:Design/BulkStream.dat:parameter#7".into(),
            byte_offset: 20,
            class_tag: "305".to_owned().try_into().unwrap(),
            record_index: 7,
            source_ordinal: 0,
            source: DesignParameterSource::new("Dimension".into(), Some(6), None).unwrap(),
            expression: "1".into(),
            expression_offset: 32,
            source_kind_offset: 52,
            unit: None,
            name: "Dimension".into(),
            name_offset: 82,
            evaluated_value: 1.0,
            evaluated_value_offset: 92,
        })
        .unwrap();
        let owner = DesignParameterOwner::try_from(DesignParameterOwnerWire {
            id: "f3d:Design/BulkStream.dat:owner#6".into(),
            byte_offset: 0,
            frame_length: 99,
            class_tag: "268".to_owned().try_into().unwrap(),
            record_index: 6,
            scope_record_index: 4,
            local_ordinal: 0,
            evaluated_value: 1.0,
            evaluated_value_offset: 40,
            parameter_record_index: 7,
            owned_ordinal: 0,
            variant: None,
            companion_record_index: 8,
        })
        .unwrap();
        let companion = DesignParameterCompanion::unbound(
            "f3d:Design/BulkStream.dat:companion#8".into(),
            0,
            "258".to_owned().try_into().unwrap(),
            8,
            6,
            std::num::NonZeroU64::new(1).unwrap(),
            42,
        );
        let entity = DesignEntityHeader {
            id: "f3d:Design/BulkStream.dat:entity#4".into(),
            byte_offset: 0,
            entity_id: DesignEntityId::from_parts("sketch", 4),
            class_tag: "112".to_owned().try_into().unwrap(),
            optional_slot_present: false,
            registration: DesignEntityRegistration::new(
                Some(DESIGN_MODULE_SKETCH.into()),
                None,
                ReferenceRun::unlocated(Vec::new()),
            )
            .unwrap(),
        };
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = crate::native::F3dNative::default();
        native.design_dimension_locus_groups.push(group);
        let mut ctx = super::super::Ctx::new(&ir, &native, decode);
        let stream = super::super::design_stream(&native.design_dimension_locus_groups[0].id);
        ctx.parameters_by_index.insert((stream, 7), &parameter);
        ctx.owners_by_index.insert((stream, 6), &owner);
        ctx.companions_by_index.insert((stream, 8), &companion);
        ctx.entities_by_suffix.insert((stream, 4), &entity);
        ctx.sketch_geometry_indices.insert((stream, 10));
        let mut findings = Vec::new();
        super::super::validate_dimension_locus_groups(&ctx, &mut findings);
        findings
    })
}

#[test]
fn locus_group_accepts_representable_layout() {
    assert!(locus_findings(locus_group_with_saturated_offsets(100, 2)).is_empty());
}

macro_rules! overflow_case {
    ($name:ident, $base:expr, $count:expr) => {
        #[test]
        fn $name() {
            let findings = locus_findings(locus_group_with_saturated_offsets($base, $count));
            assert_eq!(findings.len(), 1);
            assert_eq!(
                findings[0].message,
                "Fusion Design dimension locus group has an invalid counted frame or geometry link"
            );
        }
    };
}

overflow_case!(locus_group_rejects_overflowed_origin, u64::MAX, 1);
overflow_case!(
    locus_group_rejects_overflowed_geometry_slot,
    u64::MAX - 24,
    1
);
overflow_case!(locus_group_rejects_overflowed_role_slot, u64::MAX - 34, 1);
overflow_case!(
    locus_group_rejects_overflowed_locus_stride,
    u64::MAX - 24,
    2
);
overflow_case!(
    locus_group_rejects_overflowed_owner_origin,
    u64::MAX - 38,
    1
);
overflow_case!(
    locus_group_rejects_overflowed_owner_reference,
    u64::MAX - 40,
    1
);
overflow_case!(locus_group_rejects_overflowed_owner_role, u64::MAX - 49, 1);
overflow_case!(locus_group_rejects_overflowed_state, u64::MAX - 53, 1);
overflow_case!(
    locus_group_rejects_overflowed_return_origin,
    u64::MAX - 60,
    1
);
overflow_case!(locus_group_rejects_overflowed_return_slot, u64::MAX - 63, 1);
overflow_case!(
    locus_group_rejects_overflowed_return_stride,
    u64::MAX - 87,
    2
);
overflow_case!(locus_group_rejects_overflowed_end_stride, u64::MAX - 93, 2);

fn companion_findings(
    byte_offset: u64,
    timestamp_offset: u64,
    payload_offset: Option<u64>,
) -> Vec<cadmpeg_ir::report::check::Finding> {
    crate::test_support::with_decode_context(|decode| {
        use crate::records::decal::DesignRecordHeader;
        use crate::records::parameters::{
            DesignCompanionPayload, DesignParameterCompanion, DesignParameterOwner,
            DesignParameterOwnerWire,
        };
        let owner = DesignParameterOwner::try_from(DesignParameterOwnerWire {
            id: "f3d:Design/BulkStream.dat:owner#6".into(),
            byte_offset: 0,
            frame_length: 99,
            class_tag: "268".to_owned().try_into().unwrap(),
            record_index: 6,
            scope_record_index: 4,
            local_ordinal: 0,
            evaluated_value: 1.0,
            evaluated_value_offset: 40,
            parameter_record_index: 7,
            owned_ordinal: 0,
            variant: None,
            companion_record_index: 8,
        })
        .unwrap();
        let mut companion = DesignParameterCompanion::unbound(
            "f3d:Design/BulkStream.dat:companion#8".into(),
            byte_offset,
            "258".to_owned().try_into().unwrap(),
            8,
            6,
            std::num::NonZeroU64::new(1).unwrap(),
            timestamp_offset,
        );
        if let Some(offset) = payload_offset {
            companion = companion.bound(DesignCompanionPayload::new(offset, 0, Vec::new()));
        }
        let header = DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:header#8".into(),
            byte_offset,
            class_tag: "258".to_owned().try_into().unwrap(),
            record_index: 8,
        };
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = crate::native::F3dNative::default();
        native.design_parameter_companions.push(companion);
        let mut ctx = super::super::Ctx::new(&ir, &native, decode);
        let stream = super::super::design_stream(native.design_parameter_companions[0].id());
        ctx.owners_by_index.insert((stream, 6), &owner);
        ctx.records_by_index.insert((stream, 8), &header);
        let mut findings = Vec::new();
        super::super::validate_parameter_companions(&ctx, &mut findings);
        findings
    })
}

#[test]
fn companion_rejects_overflowed_timestamp_origin() {
    let findings = companion_findings(u64::MAX - 41, u64::MAX, None);
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].message,
        "Fusion Design parameter companion has an invalid prefix or owner link"
    );
}

#[test]
fn companion_rejects_overflowed_payload_origin() {
    let findings = companion_findings(u64::MAX - 57, u64::MAX - 15, Some(u64::MAX));
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].message,
        "Fusion Design parameter companion has an invalid prefix or owner link"
    );
}

#[test]
fn companion_accepts_representable_prefix() {
    assert!(companion_findings(100, 142, None).is_empty());
    assert!(companion_findings(100, 142, Some(158)).is_empty());
}

fn annotation_findings(payload_length: u64) -> Vec<cadmpeg_ir::report::check::Finding> {
    annotation_interval_findings(payload_length, false)
}

fn annotation_interval_findings(
    payload_length: u64,
    prefix: bool,
) -> Vec<cadmpeg_ir::report::check::Finding> {
    annotation_metadata_interval_findings(payload_length, prefix, Some(0))
}

fn annotation_metadata_interval_findings(
    payload_length: u64,
    prefix: bool,
    entity_genesis: Option<u64>,
) -> Vec<cadmpeg_ir::report::check::Finding> {
    crate::test_support::with_decode_context(|decode| {
        use crate::records::entity_header::{
            DesignEntityHeader, DesignEntityRegistration, DESIGN_MODULE_SKETCH,
        };
        use crate::records::identity::{DesignEntityId, ReferenceRun};
        use crate::records::parameters::{
            DesignParameter, DesignParameterCompanion, DesignParameterDraft, DesignParameterOwner,
            DesignParameterOwnerWire, DesignParameterSource,
        };
        let parameter_at = if prefix { 600 } else { 20 };
        let parameter = DesignParameter::try_from(DesignParameterDraft {
            id: "f3d:Design/BulkStream.dat:parameter#7".into(),
            byte_offset: parameter_at,
            class_tag: "305".to_owned().try_into().unwrap(),
            record_index: 7,
            source_ordinal: 0,
            source: DesignParameterSource::new("Dimension".into(), Some(6), None).unwrap(),
            expression: "1".into(),
            expression_offset: parameter_at + 12,
            source_kind_offset: parameter_at + 32,
            unit: None,
            name: "Dimension".into(),
            name_offset: parameter_at + 62,
            evaluated_value: 1.0,
            evaluated_value_offset: parameter_at + 72,
        })
        .unwrap();
        let owner = DesignParameterOwner::try_from(DesignParameterOwnerWire {
            id: "f3d:Design/BulkStream.dat:owner#6".into(),
            byte_offset: 0,
            frame_length: 99,
            class_tag: "268".to_owned().try_into().unwrap(),
            record_index: 6,
            scope_record_index: 4,
            local_ordinal: 0,
            evaluated_value: 1.0,
            evaluated_value_offset: 40,
            parameter_record_index: 7,
            owned_ordinal: 0,
            variant: None,
            companion_record_index: 8,
        })
        .unwrap();
        let companion = DesignParameterCompanion::unbound(
            "f3d:Design/BulkStream.dat:companion#8".into(),
            0,
            "258".to_owned().try_into().unwrap(),
            8,
            6,
            std::num::NonZeroU64::new(1).unwrap(),
            42,
        )
        .bound(crate::records::parameters::DesignCompanionPayload::new(
            58,
            payload_length,
            Vec::new(),
        ));
        let entity = DesignEntityHeader {
            id: "f3d:Design/BulkStream.dat:entity#4".into(),
            byte_offset: 0,
            entity_id: DesignEntityId::from_parts("sketch", 4),
            class_tag: "112".to_owned().try_into().unwrap(),
            optional_slot_present: false,
            registration: DesignEntityRegistration::new(
                Some(DESIGN_MODULE_SKETCH.into()),
                None,
                ReferenceRun::unlocated(Vec::new()),
            )
            .unwrap(),
        };
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = crate::native::F3dNative::default();
        let frame = crate::records::dimensions::DesignDimensionAnnotationFrame::try_new(
            crate::records::dimensions::DesignDimensionAnnotationFrameDraft {
                id: "f3d:Design/BulkStream.dat:annotation#9".into(),
                companion_record_index: if prefix { None } else { Some(8) },
                governing_companion_record_index: 8,
                byte_offset: 100,
                class_tag: "256".to_owned().try_into().unwrap(),
                record_index: 9,
                frame_length: 300,
                operands: (if entity_genesis.is_some() {
                    &[0, 10, 11, 10][..]
                } else {
                    &[0, 10, 11][..]
                })
                .iter()
                .copied()
                .enumerate()
                .map(|(ordinal, index)| {
                    crate::records::dimensions::DesignDimensionAnnotationOperand {
                        geometry_record_index: std::num::NonZeroU32::new(index),
                        geometry_reference_offset: 125
                            + cadmpeg_core::decode::u64_from_index(ordinal) * 15,
                        role: 1,
                        role_offset: 135 + cadmpeg_core::decode::u64_from_index(ordinal) * 15,
                    }
                })
                .collect(),
                entity_genesis,
                annotation_bytes: vec![7, 8],
                annotation_byte_offset: if entity_genesis.is_some() { 241 } else { 174 },
                governing_owner_record_index: 6,
                governing_owner_reference_offset: if entity_genesis.is_some() { 244 } else { 177 },
                return_members: (if entity_genesis.is_some() {
                    &[10, 10, 11][..]
                } else {
                    &[10, 11][..]
                })
                .iter()
                .copied()
                .enumerate()
                .map(|(ordinal, index)| Located {
                    value: std::num::NonZeroU32::new(index).unwrap(),
                    offset: (if entity_genesis.is_some() { 259 } else { 192 })
                        + cadmpeg_core::decode::u64_from_index(ordinal) * 11,
                })
                .collect(),
                paired_class_tag: "259".to_owned().try_into().unwrap(),
                paired_byte_offset: 400,
                owner_reference: 4,
                owner_reference_offset: 420,
            },
        )
        .unwrap();
        native.design_dimension_annotation_frames.push(frame);
        if prefix {
            native.design_parameter_scopes.push(
                crate::records::feature::scope::DesignParameterScope::empty(
                    "f3d:Design/BulkStream.dat:scope#4",
                    crate::records::feature::scope::DesignFeatureKind::Sketch,
                    4,
                ),
            );
        }
        let mut ctx = super::super::Ctx::new(&ir, &native, decode);
        let stream = super::super::design_stream(&native.design_dimension_annotation_frames[0].id);
        ctx.parameters_by_index.insert((stream, 7), &parameter);
        ctx.owners_by_index.insert((stream, 6), &owner);
        ctx.companions_by_index.insert((stream, 8), &companion);
        ctx.entities_by_suffix.insert((stream, 4), &entity);
        ctx.sketch_geometry_indices
            .extend([(stream, 10), (stream, 11)]);
        let mut findings = Vec::new();
        super::super::validate_dimension_annotation_frames(&ctx, &mut findings);
        findings
    })
}

#[test]
fn annotation_rejects_overflowed_companion_extent() {
    let findings = annotation_findings(u64::MAX - 57);
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].message,
        "Fusion Design dimension annotation frame has invalid links or offsets"
    );
}

#[test]
fn annotation_accepts_representable_companion_extent() {
    assert!(annotation_findings(500).is_empty());
}

#[test]
fn annotation_prefix_can_follow_an_earlier_parameter_companion() {
    assert!(annotation_interval_findings(500, true).is_empty());
}

#[test]
fn dimension_annotation_metadata_forms_validate_companion_and_scope_intervals() {
    for entity_genesis in [None, Some(0)] {
        for prefix in [false, true] {
            assert!(annotation_metadata_interval_findings(500, prefix, entity_genesis).is_empty());
        }
        assert!(
            !annotation_metadata_interval_findings(u64::MAX - 57, false, entity_genesis).is_empty()
        );
    }
}
