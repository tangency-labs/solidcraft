// SPDX-License-Identifier: Apache-2.0

use super::{DesignSketchPlacement, DesignSketchPlacementWire, SKETCH_PLACEMENT_CLONE_COUNT};

fn placement(member: bool) -> DesignSketchPlacement {
    let (frame_length, paired_byte_offset, member_run_head) = if member {
        (34, 150, r#","member_run_head":true"#)
    } else {
        (201, 301, "")
    };
    serde_json::from_str(&format!(
        r#"{{"id":"f3d:native:sketch-placement#0","entity_id":"0_35","byte_offset":100,"class_tag":"305","record_index":5,"frame_length":{frame_length},"transform":[[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]],"paired_class_tag":"306","paired_byte_offset":{paired_byte_offset}{member_run_head}}}"#
    ))
    .unwrap()
}

#[test]
fn sketch_placement_borrowed_wire_matches_owned_wire_bytes() {
    for record in [placement(false), placement(true)] {
        let owned = DesignSketchPlacementWire::from(record.clone());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn sketch_placement_native_retained_limit_refuses_before_record_clone() {
    let record = placement(true);
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_sketch_placements",
        || SKETCH_PLACEMENT_CLONE_COUNT.with(|count| count.set(0)),
        || SKETCH_PLACEMENT_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn sketch_visibility_collection_wire_requires_omission_or_a_positive_identity() {
    let mut visibility =
        super::DesignSketchVisibility::new(std::num::NonZeroU32::new(2).unwrap(), 30, false)
            .unwrap();
    visibility.ordinal_collection = std::num::NonZeroU64::new(300);
    let wire = serde_json::to_value(visibility).unwrap();
    assert_eq!(
        serde_json::from_value::<super::DesignSketchVisibility>(wire.clone()).unwrap(),
        visibility
    );
    let mut omitted = wire.clone();
    omitted
        .as_object_mut()
        .unwrap()
        .remove("ordinal_collection");
    assert!(
        serde_json::from_value::<super::DesignSketchVisibility>(omitted)
            .unwrap()
            .ordinal_collection
            .is_none()
    );
    for invalid in [serde_json::Value::Null, serde_json::json!(0)] {
        let mut invalid_wire = wire.clone();
        invalid_wire["ordinal_collection"] = invalid;
        assert!(serde_json::from_value::<super::DesignSketchVisibility>(invalid_wire).is_err());
    }
}
