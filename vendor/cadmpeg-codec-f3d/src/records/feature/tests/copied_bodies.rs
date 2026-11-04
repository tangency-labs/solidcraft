// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::body_ops::DesignCopyPasteBodiesOperation;

fn copied_wire() -> serde_json::Value {
    serde_json::json!({
        "body_group_record_index":501, "body_group_class_tag":"264", "body_group_byte_offset":100,
        "body_operand_record_indices":[502,504], "body_operand_record_offsets":[126,137],
        "relation_record_index":503, "relation_class_tag":"264", "relation_byte_offset":200,
        "source_body_entity_suffixes":[11,13], "source_body_entity_suffix_offsets":[225,255],
        "copied_body_entity_suffixes":[12,14], "copied_body_entity_suffix_offsets":[240,270]
    })
}

#[test]
fn copied_bodies_reject_empty_aliased_and_misaligned_rows() {
    let admitted: DesignCopyPasteBodiesOperation = serde_json::from_value(copied_wire()).unwrap();
    let construct = |bodies| {
        DesignCopyPasteBodiesOperation::try_new(
            bodies,
            crate::records::feature::body_ops::CopyPasteRecordLocation {
                record_index: 501,
                class_tag: "264".to_owned().try_into().unwrap(),
                byte_offset: 100,
            },
            crate::records::feature::body_ops::CopyPasteRecordLocation {
                record_index: 503,
                class_tag: "264".to_owned().try_into().unwrap(),
                byte_offset: 200,
            },
        )
    };
    assert!(construct(Vec::new()).is_err());
    for lane in 0..5 {
        let mut bodies = admitted.bodies().to_vec();
        match lane {
            0 => bodies[1].source.value = bodies[0].source.value,
            1 => bodies[1].copied.value = bodies[0].source.value,
            2 => bodies[1].operand.offset += 1,
            3 => bodies[1].source.offset += 1,
            _ => bodies[0].copied.offset += 1,
        }
        assert!(construct(bodies).is_err());
    }
    let mut empty = copied_wire();
    for field in [
        "body_operand_record_indices",
        "body_operand_record_offsets",
        "source_body_entity_suffixes",
        "source_body_entity_suffix_offsets",
        "copied_body_entity_suffixes",
        "copied_body_entity_suffix_offsets",
    ] {
        empty[field] = serde_json::json!([]);
    }
    assert!(serde_json::from_value::<DesignCopyPasteBodiesOperation>(empty).is_err());
    for (field, lane, value) in [
        ("source_body_entity_suffixes", 1, 11),
        ("copied_body_entity_suffixes", 1, 11),
        ("body_operand_record_offsets", 0, 127),
        ("body_operand_record_offsets", 1, 138),
        ("source_body_entity_suffix_offsets", 0, 226),
        ("source_body_entity_suffix_offsets", 1, 256),
        ("copied_body_entity_suffix_offsets", 0, 241),
    ] {
        let mut wire = copied_wire();
        wire[field][lane] = serde_json::json!(value);
        assert!(
            serde_json::from_value::<DesignCopyPasteBodiesOperation>(wire).is_err(),
            "{field}"
        );
    }
    assert_eq!(serde_json::to_value(admitted).unwrap(), copied_wire());
}

#[test]
fn copied_bodies_reject_overflowed_operand_origin() {
    let mut wire = copied_wire();
    wire["body_group_byte_offset"] = u64::MAX.into();
    wire["body_operand_record_offsets"] = serde_json::json!([u64::MAX, u64::MAX]);
    assert!(serde_json::from_value::<DesignCopyPasteBodiesOperation>(wire).is_err());
}

#[test]
fn copied_bodies_reject_overflowed_source_origin() {
    let mut wire = copied_wire();
    wire["relation_byte_offset"] = u64::MAX.into();
    wire["source_body_entity_suffix_offsets"] = serde_json::json!([u64::MAX, u64::MAX]);
    wire["copied_body_entity_suffix_offsets"] = serde_json::json!([u64::MAX, u64::MAX]);
    assert!(serde_json::from_value::<DesignCopyPasteBodiesOperation>(wire).is_err());
}

#[test]
fn copied_bodies_reject_overflowed_copied_offset() {
    let mut wire = copied_wire();
    wire["relation_byte_offset"] = (u64::MAX - 25).into();
    wire["source_body_entity_suffix_offsets"] = serde_json::json!([u64::MAX, u64::MAX]);
    wire["copied_body_entity_suffix_offsets"] = serde_json::json!([u64::MAX, u64::MAX]);
    assert!(serde_json::from_value::<DesignCopyPasteBodiesOperation>(wire).is_err());
}

#[test]
fn copied_bodies_reject_overflowed_operand_stride() {
    let mut wire = copied_wire();
    wire["body_group_byte_offset"] = (u64::MAX - 26).into();
    wire["body_operand_record_offsets"] = serde_json::json!([u64::MAX, u64::MAX]);
    assert!(serde_json::from_value::<DesignCopyPasteBodiesOperation>(wire).is_err());
}

#[test]
fn copied_bodies_reject_overflowed_source_stride() {
    let mut wire = copied_wire();
    wire["relation_byte_offset"] = (u64::MAX - 50).into();
    wire["source_body_entity_suffix_offsets"] = serde_json::json!([u64::MAX - 25, u64::MAX]);
    wire["copied_body_entity_suffix_offsets"] = serde_json::json!([u64::MAX - 10, u64::MAX]);
    assert!(serde_json::from_value::<DesignCopyPasteBodiesOperation>(wire).is_err());
}
