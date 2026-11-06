// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::u64_from_index;

use super::exact_work_point_construction;
use super::POINT_DATA_TYPE_GUID;
use crate::design::decode::scopes::parameter_scope::parse_parameter_scope;
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::work_geometry::DesignWorkPointRule;
use crate::test_support::lp_utf16;
use std::collections::HashMap;

/// A `WorkPoint` scope record, its paired header, and one point-data record
/// frame: the indexed header, the payload prologue with an optional property
/// block, the class-level members of `version`, a base-level run of `inputs`
/// references, and the second header that closes the frame.
fn work_point_stream(
    class_tag: &str,
    version: u32,
    property: bool,
    pick_point: Option<u64>,
    position: [f64; 3],
    reference_type: u32,
    inputs: u32,
) -> (Vec<u8>, DesignParameterScope, usize) {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"427");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "WorkPoint");
    let mut tail = [0; 78];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    tail[31..35].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 11]);

    bytes.extend_from_slice(
        &(u32::try_from(class_tag.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    bytes.extend_from_slice(class_tag.as_bytes());
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.push(0);
    if property {
        bytes.push(1);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&6u32.to_le_bytes());
        bytes.extend_from_slice(b"pt_tag");
        bytes.extend_from_slice(&23u32.to_le_bytes());
        bytes.extend_from_slice(b"IntrinsicMetaTypeuint64");
        bytes.extend_from_slice(&9u64.to_le_bytes());
    } else {
        bytes.push(0);
    }
    if version >= 2 {
        bytes.extend_from_slice(&0i32.to_le_bytes());
    }
    for _ in 0..2 {
        bytes.extend_from_slice(&f64::to_le_bytes(0.0));
    }
    if version >= 1 {
        match pick_point {
            Some(target) => {
                bytes.push(1);
                bytes.extend_from_slice(&target.to_le_bytes());
                bytes.extend_from_slice(&[0, 0]);
            }
            None => bytes.push(0),
        }
    }
    let position_at = bytes.len();
    for value in position {
        bytes.extend_from_slice(&f64::to_le_bytes(value));
    }
    bytes.extend_from_slice(&reference_type.to_le_bytes());
    if version >= 3 {
        for _ in 0..3 {
            bytes.extend_from_slice(&f64::to_le_bytes(-1.0));
        }
    }
    bytes.extend_from_slice(&inputs.to_le_bytes());
    for input in 0..inputs {
        bytes.push(1);
        bytes.extend_from_slice(&u64::from(70 + input).to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
    }
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&55u32.to_le_bytes());

    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 12,
        class_tag: crate::records::references::DesignClassTag::try_from("427".to_owned()).unwrap(),
        byte_offset: 0,
    };
    let scope = parse_parameter_scope(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
    .expect("WorkPoint scope");
    (bytes, scope, position_at)
}

#[test]
fn work_point_reads_the_class_version_its_type_table_stores() {
    let (bytes, scope, position_at) =
        work_point_stream("282", 2, false, None, [4.0, 5.0, 6.0], 5, 1);
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let frame = exact_work_point_construction(
        &bytes,
        &records,
        &scope,
        &HashMap::from([(55, (POINT_DATA_TYPE_GUID, 2))]),
    )
    .expect("work point frame");
    assert_eq!(
        frame.position.map(cadmpeg_ir::scalar::FiniteReal::get),
        [4.0, 5.0, 6.0]
    );
    assert_eq!(frame.position_offset, u64_from_index(position_at));
    // The stored version drives the read: a version that describes a
    // different member sequence does not yield this frame's coordinate.
    assert_ne!(
        exact_work_point_construction(
            &bytes,
            &records,
            &scope,
            &HashMap::from([(55, (POINT_DATA_TYPE_GUID, 0))])
        )
        .map(|frame| frame.position_offset),
        Some(u64_from_index(position_at))
    );
    // An unregistered entity falls back to the agreement sweep.
    assert_eq!(
        exact_work_point_construction(
            &bytes,
            &records,
            &scope,
            &HashMap::from([(9, (POINT_DATA_TYPE_GUID, 0))])
        ),
        exact_work_point_construction(&bytes, &records, &scope, &HashMap::new())
    );
}

#[test]
fn work_point_position_does_not_depend_on_the_segment_local_class_tag() {
    // A class tag is `256` plus an index into the segment's own type table,
    // so the point-data class wears a different tag in every segment. The
    // coordinate is the same wherever the type table names the class.
    for class_tag in ["282", "316", "364", "409", "424", "460", "468"] {
        let (bytes, scope, position_at) =
            work_point_stream(class_tag, 2, false, None, [7.5, 8.5, 9.5], 5, 1);
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);

        let frame = exact_work_point_construction(
            &bytes,
            &records,
            &scope,
            &HashMap::from([(55, (POINT_DATA_TYPE_GUID, 2))]),
        )
        .unwrap_or_else(|| panic!("class tag {class_tag}"));
        assert_eq!(
            frame.position.map(cadmpeg_ir::scalar::FiniteReal::get),
            [7.5, 8.5, 9.5],
            "class tag {class_tag}"
        );
        assert_eq!(
            frame.position_offset,
            u64_from_index(position_at),
            "class tag {class_tag}"
        );
    }
}

#[test]
fn work_point_position_survives_a_property_block_and_a_present_pick_point() {
    let (bytes, scope, position_at) =
        work_point_stream("282", 3, true, Some(9), [1.25, -2.5, 3.75], 20, 1);

    let frame = exact_work_point_construction(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &HashMap::new(),
    )
    .expect("work point frame");
    assert_eq!(
        frame.position.map(cadmpeg_ir::scalar::FiniteReal::get),
        [1.25, -2.5, 3.75]
    );
    assert_eq!(frame.position_offset, u64_from_index(position_at));
    assert_eq!(work_point_input_indices(&frame.rule), [70]);
}

#[test]
fn work_point_position_reads_every_class_version_that_stores_one() {
    for version in 0..=3 {
        let (bytes, scope, position_at) =
            work_point_stream("282", version, false, None, [4.0, 5.0, 6.0], 5, 1);

        let frame = exact_work_point_construction(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
            &HashMap::new(),
        )
        .unwrap_or_else(|| panic!("class version {version}"));
        assert_eq!(
            frame.position.map(cadmpeg_ir::scalar::FiniteReal::get),
            [4.0, 5.0, 6.0],
            "class version {version}"
        );
        assert_eq!(
            frame.position_offset,
            u64_from_index(position_at),
            "class version {version}"
        );
    }
}

#[test]
fn work_point_rejects_a_registered_entity_of_another_type() {
    // The tag says `282`, but the type table names a different class for
    // this entity, so the record is not point data whatever its tag reads.
    let (bytes, scope, _) = work_point_stream("282", 2, false, None, [4.0, 5.0, 6.0], 5, 1);
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);

    assert_eq!(
        exact_work_point_construction(
            &bytes,
            &records,
            &scope,
            &HashMap::from([(55, ("A0A15D26-1F3B-4120-A3F1-9CDDA189AB74", 2))])
        ),
        None
    );
}

#[test]
fn work_point_uses_the_serialized_input_count_for_every_rule() {
    // The input count is a member of the point-data level. It frames the
    // run independently of the rule selector, including three-input
    // constructions.
    let (bytes, scope, _) = work_point_stream("282", 2, false, None, [1.0, 2.0, 3.0], 18, 1);

    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let frame = exact_work_point_construction(&bytes, &records, &scope, &HashMap::new())
        .expect("work point frame");
    assert_eq!(work_point_input_indices(&frame.rule), [70]);
    assert_eq!(frame.rule.reference_type(), 18);
    let (bytes, scope, _) = work_point_stream("282", 2, false, None, [1.0, 2.0, 3.0], 14, 2);
    let frame = exact_work_point_construction(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &HashMap::new(),
    )
    .expect("work point frame");
    assert_eq!(work_point_input_indices(&frame.rule), [70, 71]);

    let (bytes, scope, _) = work_point_stream("282", 2, false, None, [1.0, 2.0, 3.0], 8, 3);
    let frame = exact_work_point_construction(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &HashMap::new(),
    )
    .expect("work point frame");
    assert_eq!(work_point_input_indices(&frame.rule), [70, 71, 72]);

    let (bytes, scope, _) = work_point_stream("282", 2, false, None, [1.0, 2.0, 3.0], 18, 2);
    let frame = exact_work_point_construction(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &HashMap::new(),
    )
    .expect("work point frame");
    assert_eq!(work_point_input_indices(&frame.rule), [70, 71]);
}

#[test]
fn work_point_rule_codes_select_typed_input_arities() {
    for (reference_type, arity) in [(5, 1), (7, 2), (8, 3), (10, 1), (14, 2), (20, 1)] {
        let (bytes, scope, _) = work_point_stream(
            "282",
            2,
            false,
            None,
            [1.0, 2.0, 3.0],
            reference_type,
            arity,
        );
        let frame = exact_work_point_construction(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
            &HashMap::new(),
        )
        .expect("work point frame");
        assert_eq!(frame.rule.reference_type(), reference_type);
        assert_eq!(u32::try_from(frame.rule.inputs().len()).unwrap(), arity);
        assert!(match frame.rule.form() {
            crate::records::feature::work_geometry::DesignWorkPointRuleForm::CircleCenter { .. } =>
                reference_type == 5,
            crate::records::feature::work_geometry::DesignWorkPointRuleForm::TwoEdgeIntersection { .. } =>
                reference_type == 7,
            crate::records::feature::work_geometry::DesignWorkPointRuleForm::ThreePlaneIntersection { .. } =>
                reference_type == 8,
            crate::records::feature::work_geometry::DesignWorkPointRuleForm::Vertex { .. } => reference_type == 10,
            crate::records::feature::work_geometry::DesignWorkPointRuleForm::EdgePlaneIntersection { .. } =>
                reference_type == 14,
            crate::records::feature::work_geometry::DesignWorkPointRuleForm::DistanceOnEdge { .. } =>
                reference_type == 20,
            crate::records::feature::work_geometry::DesignWorkPointRuleForm::Native { .. } => false,
        });
    }
}

#[test]
fn work_point_rule_code_with_wrong_arity_remains_native() {
    let (bytes, scope, _) = work_point_stream("282", 2, false, None, [1.0, 2.0, 3.0], 5, 2);
    let frame = exact_work_point_construction(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &HashMap::new(),
    )
    .expect("work point frame");

    assert!(matches!(
        frame.rule.form(),
        crate::records::feature::work_geometry::DesignWorkPointRuleForm::Native {
            reference_type: 5,
            ref inputs,
        } if inputs.len() == 2
    ));
}

#[test]
fn work_point_rule_rejects_an_incompatible_input_carrier() {
    let (bytes, scope, _) = work_point_stream("282", 2, false, None, [1.0, 2.0, 3.0], 14, 2);
    let frame = exact_work_point_construction(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &HashMap::new(),
    )
    .expect("work point frame");
    let mut wire = serde_json::to_value(&frame.rule).expect("serialize valid rule");

    wire["inputs"][1]["carrier"] = serde_json::json!({
        "kind": "edge_recipe", "operand_id": "f3d:native:edge-operand#wrong-role"
    });
    assert!(serde_json::from_value::<DesignWorkPointRule>(wire).is_err());
}

#[test]
fn work_point_direct_record_carries_model_space_position() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"427");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "WorkPoint");
    let mut tail = [0; 78];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    tail[31..35].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 11]);

    let point_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"282");
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 27]);
    let position_at = bytes.len();
    for value in [1.25, -2.5, 3.75] {
        bytes.extend_from_slice(&f64::to_le_bytes(value));
    }
    bytes.extend_from_slice(&7u32.to_le_bytes());
    for _ in 0..3 {
        bytes.extend_from_slice(&f64::to_le_bytes(-1.0));
    }
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for target in [56u32, 57] {
        bytes.push(1);
        bytes.extend_from_slice(&u64::from(target).to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
    }
    bytes.resize(point_at + 208, 0);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&55u32.to_le_bytes());

    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 12,
        class_tag: crate::records::references::DesignClassTag::try_from("427".to_owned()).unwrap(),
        byte_offset: 0,
    };
    let scope = parse_parameter_scope(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
    .expect("WorkPoint scope");
    let frame = exact_work_point_construction(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &HashMap::new(),
    )
    .expect("work point frame");
    assert_eq!(
        frame.position.map(cadmpeg_ir::scalar::FiniteReal::get),
        [1.25, -2.5, 3.75]
    );
    assert_eq!(frame.position_offset, u64_from_index(position_at));
    assert_eq!(frame.rule.reference_type(), 7);
    assert_eq!(work_point_input_indices(&frame.rule), [56, 57]);
    bytes[point_at + 66..point_at + 70].copy_from_slice(&1u32.to_le_bytes());
    bytes[point_at + 94..point_at + 98].copy_from_slice(&1u32.to_le_bytes());
    bytes.drain(point_at + 197..point_at + 208);
    let frame = exact_work_point_construction(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &HashMap::new(),
    )
    .expect("work point frame");
    assert_eq!(
        frame.position.map(cadmpeg_ir::scalar::FiniteReal::get),
        [1.25, -2.5, 3.75]
    );
    assert_eq!(frame.position_offset, u64_from_index(position_at));
    assert_eq!(frame.rule.reference_type(), 1);
    assert_eq!(work_point_input_indices(&frame.rule), [56]);
}

#[test]
fn work_point_input_count_frames_the_rule_inputs() {
    // The counted input run is framed by its serialized count. The rule
    // selector is retained independently and does not impose a fixed arity.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"427");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "WorkPoint");
    let mut tail = [0; 78];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    tail[31..35].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 11]);

    let point_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"282");
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 27]);
    let position_at = bytes.len();
    for value in [4.0, 5.0, 6.0] {
        bytes.extend_from_slice(&f64::to_le_bytes(value));
    }
    bytes.extend_from_slice(&18u32.to_le_bytes());
    for _ in 0..3 {
        bytes.extend_from_slice(&f64::to_le_bytes(-1.0));
    }
    let count_at = bytes.len();
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for target in [56u32, 57] {
        bytes.push(1);
        bytes.extend_from_slice(&u64::from(target).to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
    }
    bytes.resize(point_at + 208, 0);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&55u32.to_le_bytes());

    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 12,
        class_tag: crate::records::references::DesignClassTag::try_from("427".to_owned()).unwrap(),
        byte_offset: 0,
    };
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let scope = parse_parameter_scope(
        &bytes,
        &records,
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
    .expect("WorkPoint scope");
    let frame = exact_work_point_construction(&bytes, &records, &scope, &HashMap::new())
        .expect("work point frame");
    assert_eq!(frame.rule.reference_type(), 18);
    assert_eq!(work_point_input_indices(&frame.rule), [56, 57]);

    bytes[count_at..count_at + 4].copy_from_slice(&1u32.to_le_bytes());
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let frame = exact_work_point_construction(&bytes, &records, &scope, &HashMap::new())
        .expect("work point frame");
    assert_eq!(frame.rule.reference_type(), 18);
    assert_eq!(work_point_input_indices(&frame.rule), [56]);

    // A rule above the values the shipped range check admitted still names a
    // coordinate when its input arity agrees.
    bytes[position_at + 24..position_at + 28].copy_from_slice(&64u32.to_le_bytes());
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let frame = exact_work_point_construction(&bytes, &records, &scope, &HashMap::new())
        .expect("work point frame");
    assert_eq!(
        frame.position.map(cadmpeg_ir::scalar::FiniteReal::get),
        [4.0, 5.0, 6.0]
    );
    assert_eq!(frame.rule.reference_type(), 64);
    assert_eq!(work_point_input_indices(&frame.rule), [56]);
}

fn work_point_input_indices(rule: &DesignWorkPointRule) -> Vec<u32> {
    rule.inputs()
        .iter()
        .map(crate::records::feature::work_geometry::DesignWorkPointInput::record_index)
        .collect()
}
