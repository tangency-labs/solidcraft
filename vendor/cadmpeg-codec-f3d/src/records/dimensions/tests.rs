// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use crate::records::{
    dimensions::{
        DesignDimensionAnnotationFrame as Frame, DesignDimensionAnnotationFrameDraft as Draft,
        DesignDimensionAnnotationOperand as Operand,
    },
    identity::Located,
};
use std::num::NonZeroU32;

fn locus_group(state: u32, count: usize) -> super::DesignDimensionLocusGroup {
    super::DesignDimensionLocusGroup {
        id: "locus-group".into(),
        companion_record_index: 2,
        byte_offset: 100,
        class_tag: "256".to_owned().try_into().unwrap(),
        record_index: 3,
        frame_length: 200,
        loci: (0..count)
            .map(|index| super::DesignDimensionLocus {
                returned: Located {
                    value: 30 + u32::try_from(index).expect("fixture value fits u32"),
                    offset: 140 + u64_from_index(index) * 8,
                },
                geometry_record_index: 10 + u32::try_from(index).expect("fixture value fits u32"),
                geometry_reference_offset: 120 + u64_from_index(index) * 8,
                role: 1,
                role_offset: 124 + u64_from_index(index) * 8,
            })
            .collect(),
        owner_reference: 4,
        owner_reference_offset: 180,
        owner_role: 1,
        owner_role_offset: 184,
        state,
        state_offset: 188,
        next_class_tag: "259".to_owned().try_into().unwrap(),
        next_record_index: 5,
        next_byte_offset: 300,
    }
}

#[test]
fn dimension_locus_group_borrowed_wire_matches_owned_wire_bytes() {
    for (state, count) in [(0, 0), (0x8000_0001, 1), (0x0000_1010, 3)] {
        let group = locus_group(state, count);
        let owned = super::DesignDimensionLocusGroupWire::from(group.clone());
        assert_eq!(
            serde_json::to_vec(&group).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
        assert_eq!(
            serde_json::from_slice::<super::DesignDimensionLocusGroup>(
                &serde_json::to_vec(&group).unwrap()
            )
            .unwrap(),
            group
        );
    }
}

#[test]
fn dimension_locus_group_native_retained_limit_refuses_before_clone() {
    #[derive(serde::Serialize)]
    struct NestedRecord<'a> {
        id: &'static str,
        value: &'a super::DesignDimensionLocusGroup,
    }
    let group = locus_group(0x1010, 3);
    let record = NestedRecord {
        id: "f3d:native:locus-group#0",
        value: &group,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_scopes",
        || super::DIMENSION_LOCUS_GROUP_CLONE_COUNT.with(|count| count.set(0)),
        || super::DIMENSION_LOCUS_GROUP_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

fn draft(base: u64) -> Draft {
    Draft {
        id: "annotation".into(),
        companion_record_index: None,
        governing_companion_record_index: 2,
        byte_offset: base,
        class_tag: "256".to_owned().try_into().unwrap(),
        record_index: 3,
        frame_length: 300,
        operands: [0, 10, 11, 10]
            .into_iter()
            .enumerate()
            .map(|(ordinal, index)| Operand {
                geometry_record_index: NonZeroU32::new(index),
                geometry_reference_offset: base + 25 + u64_from_index(ordinal) * 15,
                role: 1,
                role_offset: base + 35 + u64_from_index(ordinal) * 15,
            })
            .collect(),
        entity_genesis: Some(0),
        annotation_bytes: vec![7, 8],
        annotation_byte_offset: base + 141,
        governing_owner_record_index: 4,
        governing_owner_reference_offset: base + 144,
        return_members: [10, 10, 11]
            .into_iter()
            .enumerate()
            .map(|(ordinal, index)| Located {
                value: NonZeroU32::new(index).unwrap(),
                offset: base + 159 + u64_from_index(ordinal) * 11,
            })
            .collect(),
        paired_class_tag: "259".to_owned().try_into().unwrap(),
        paired_byte_offset: base + 300,
        owner_reference: 5,
        owner_reference_offset: base + 320,
    }
}

#[test]
fn annotation_frame_preserves_nulls_duplicate_geometry_and_return_order() {
    let original = draft(100);
    let frame = Frame::try_new(original.clone())
        .map_err(|error| error.to_string())
        .unwrap();
    assert_eq!(frame.clone().into_draft(), original);
    assert_eq!(
        frame
            .clone()
            .into_draft()
            .return_members
            .iter()
            .map(|index| index.value.get())
            .collect::<Vec<_>>(),
        [10, 10, 11]
    );
    let wire = serde_json::to_value(&frame).unwrap();
    assert_eq!(serde_json::from_value::<Frame>(wire).unwrap(), frame);
    let mut all_null = original;
    for operand in &mut all_null.operands {
        operand.geometry_record_index = None;
    }
    all_null.return_members.clear();
    assert!(Frame::try_new(all_null)
        .map_err(|error| error.to_string())
        .is_ok());
}

#[test]
fn annotation_frame_borrowed_wire_matches_owned_wire_bytes() {
    for companion in [None, Some(1)] {
        let mut input = draft(100);
        input.companion_record_index = companion;
        let frame = Frame::try_new(input)
            .map_err(|error| error.to_string())
            .unwrap();
        let owned = super::DesignDimensionAnnotationFrameWire::from(frame.clone());
        assert_eq!(
            serde_json::to_vec(&frame).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn annotation_frame_native_retained_limit_refuses_before_clone() {
    #[derive(serde::Serialize)]
    struct NestedRecord<'a> {
        id: &'static str,
        value: &'a Frame,
    }
    let frame = Frame::try_new(draft(100))
        .map_err(|error| error.to_string())
        .unwrap();
    let record = NestedRecord {
        id: "f3d:native:annotation-frame#0",
        value: &frame,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_scopes",
        || super::DIMENSION_ANNOTATION_CLONE_COUNT.with(|count| count.set(0)),
        || super::DIMENSION_ANNOTATION_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn annotation_frame_rejects_stale_offsets_and_changed_multisets() {
    let frame = Frame::try_new(draft(100))
        .map_err(|error| error.to_string())
        .unwrap();
    let wire = serde_json::to_value(&frame).unwrap();
    for field in [
        "annotation_byte_offset",
        "governing_owner_reference_offset",
        "paired_byte_offset",
        "owner_reference_offset",
    ] {
        let mut invalid = wire.clone();
        invalid[field] = (invalid[field].as_u64().unwrap() + 1).into();
        assert!(serde_json::from_value::<Frame>(invalid)
            .unwrap_err()
            .to_string()
            .contains(field));
    }
    for field in ["geometry_reference_offset", "role_offset"] {
        let mut invalid = wire.clone();
        invalid["operands"][0][field] = 0.into();
        assert!(serde_json::from_value::<Frame>(invalid)
            .unwrap_err()
            .to_string()
            .contains(field));
    }
    let mut invalid = wire.clone();
    invalid["return_member_offsets"][0] = 0.into();
    assert!(serde_json::from_value::<Frame>(invalid)
        .unwrap_err()
        .to_string()
        .contains("return_member_offsets"));
    let mut invalid = wire;
    invalid["return_members"][0] = 11.into();
    assert!(serde_json::from_value::<Frame>(invalid)
        .unwrap_err()
        .to_string()
        .contains("return_members"));
    let mut empty = draft(100);
    empty.operands.clear();
    assert!(Frame::try_new(empty)
        .map_err(|error| error.to_string())
        .unwrap_err()
        .contains("operands"));
}

#[test]
fn annotation_frame_rejects_unrepresentable_extents() {
    let valid = draft(u64::MAX - 320);
    assert_eq!(
        Frame::try_new(valid.clone())
            .map_err(|error| error.to_string())
            .unwrap()
            .owner_reference_offset(),
        u64::MAX
    );
    let mut overflow = valid;
    overflow.frame_length += 1;
    assert!(Frame::try_new(overflow)
        .map_err(|error| error.to_string())
        .unwrap_err()
        .contains("owner_reference_offset"));
    let mut overflow = draft(0);
    overflow.byte_offset = u64::MAX;
    assert!(Frame::try_new(overflow)
        .map_err(|error| error.to_string())
        .unwrap_err()
        .contains("annotation_byte_offset"));
}

fn locus_pair(has_first: bool) -> super::DesignDimensionLocusPair {
    let prefix = if has_first { 40 } else { 25 };
    super::DesignDimensionLocusPair::try_new(super::DesignDimensionLocusPairDraft {
        id: "f3d:native:locus-pair#0".into(),
        companion_record_index: 2,
        governing_companion_record_index: 3,
        byte_offset: 100,
        class_tag: "256".to_owned().try_into().unwrap(),
        record_index: 4,
        frame_length: 100,
        opaque_index: has_first.then_some(Located {
            value: 7,
            offset: 135,
        }),
        loci: [
            Operand {
                geometry_record_index: has_first.then(|| NonZeroU32::new(7).unwrap()),
                geometry_reference_offset: 100 + prefix,
                role: 1,
                role_offset: 110 + prefix,
            },
            Operand {
                geometry_record_index: NonZeroU32::new(8),
                geometry_reference_offset: 115 + prefix,
                role: 2,
                role_offset: 125 + prefix,
            },
        ],
        paired_class_tag: "257".to_owned().try_into().unwrap(),
        paired_byte_offset: 200,
    })
    .unwrap()
}

#[test]
fn dimension_locus_pair_borrowed_wire_matches_owned_wire_bytes() {
    for record in [locus_pair(false), locus_pair(true)] {
        let owned = super::DesignDimensionLocusPairWire::from(record.clone());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn dimension_locus_pair_native_retained_limit_refuses_before_record_clone() {
    let record = locus_pair(true);
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_dimension_locus_pairs",
        || super::DIMENSION_LOCUS_PAIR_CLONE_COUNT.with(|count| count.set(0)),
        || super::DIMENSION_LOCUS_PAIR_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn annotation_frame_empty_metadata_validates_wire_offsets_and_operand_shape() {
    let mut input = draft(100);
    input.operands.pop();
    input.entity_genesis = None;
    input.annotation_byte_offset = 174;
    input.governing_owner_reference_offset = 177;
    input.return_members.remove(1);
    for (ordinal, member) in input.return_members.iter_mut().enumerate() {
        member.offset = 192 + u64_from_index(ordinal) * 11;
    }
    let frame = Frame::try_new(input.clone()).unwrap();
    assert_eq!(frame.clone().into_draft(), input);
    let wire = serde_json::to_value(&frame).unwrap();
    assert!(wire.get("entity_genesis").is_none());
    assert_eq!(
        serde_json::from_value::<Frame>(wire.clone()).unwrap(),
        frame
    );
    for field in ["annotation_byte_offset", "governing_owner_reference_offset"] {
        let mut wrong = wire.clone();
        wrong[field] = serde_json::json!(0);
        assert!(serde_json::from_value::<Frame>(wrong).is_err());
    }
    let mut null = wire;
    null["entity_genesis"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<Frame>(null).is_err());
    let mut populated = input.clone();
    populated.entity_genesis = Some(0);
    assert!(Frame::try_new(populated).is_err());
    let mut ambiguous = input;
    ambiguous.operands[0].geometry_record_index = NonZeroU32::new(10);
    assert!(Frame::try_new(ambiguous).is_err());
}
