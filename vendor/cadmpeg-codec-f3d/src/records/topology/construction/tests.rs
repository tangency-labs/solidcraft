// SPDX-License-Identifier: Apache-2.0
//! Construction operand groups, frames, paths and identities on the wire.

use super::{
    DesignConstructionOperandGroup, DesignConstructionOperandGroupFrame,
    DesignConstructionOperandGroupFrameWire, DesignConstructionOperandTransform,
    DesignConstructionPersistentIdentity, DesignConstructionTrackingPath,
};
use crate::records::identity::Located;
use crate::records::topology::test_support::rejects_changed_fields;
use cadmpeg_test_support::refusal::{refusal, states_the_key};
use serde_json::json;
fn frame_wire() -> DesignConstructionOperandGroupFrameWire {
    let matrix = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    serde_json::from_value(json!({
        "member_count_offset": 21,
        "opaque_index": 1, "opaque_index_offset": 118,
        "opaque_scalar": 0.0, "opaque_scalar_offset": 122, "variant": false,
        "trailing_record_indices": [300], "trailing_record_offsets": [90],
        "trailing_flags": [{"record_index":300,"byte_offset":1000,"class_tag":"277","value":false,"value_offset":1022}],
        "trailing_transforms": [{"record_index":300,"byte_offset":1000,"class_tag":"277","transform":matrix,"transform_offset":1022,"following_record_index":301,"following_byte_offset":1152,"following_class_tag":"278"}],
        "trailing_dual_transforms": [{"record_index":300,"byte_offset":1000,"class_tag":"277","first_transform":matrix,"first_transform_offset":1021,"second_transform":matrix,"second_transform_offset":1149}],
        "auxiliary_paths": [{"record_index":400,"byte_offset":2000,"class_tag":"277","entity_ref":1,"entity_ref_offset":2022,"compact_variant":false,"scope_record_index":7,"scope_record_index_offset":2035,"nested_record_index":402,"nested_record_index_offset":2046,"following_record_index":401,"following_byte_offset":2062,"following_class_tag":"278"}]
    })).unwrap()
}

#[test]
fn construction_transform_borrowed_wire_matches_owned_wire_bytes() {
    let transform = frame_wire().trailing_transforms.into_iter().next().unwrap();
    let owned = super::DesignConstructionOperandTransformDraft::from(transform.clone());
    assert_eq!(
        serde_json::to_vec(&transform).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn construction_transform_native_retained_limit_refuses_before_clone() {
    #[derive(serde::Serialize)]
    struct NestedRecord<'a> {
        id: &'static str,
        value: &'a DesignConstructionOperandTransform,
    }
    let transform = frame_wire().trailing_transforms.into_iter().next().unwrap();
    let record = NestedRecord {
        id: "f3d:native:construction-transform#0",
        value: &transform,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_scopes",
        || super::CONSTRUCTION_TRANSFORM_CLONE_COUNT.with(|count| count.set(0)),
        || super::CONSTRUCTION_TRANSFORM_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn construction_frame_rejects_invalid_scalars_offsets_and_trailing_arity() {
    for value in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut wire = frame_wire();
        wire.opaque_scalar = value;
        assert!(DesignConstructionOperandGroupFrame::try_from(wire).is_err());
    }
    for (field, value) in [
        ("opaque_index", json!(0)),
        ("opaque_scalar", json!(-1.0)),
        ("opaque_index_offset", json!(17)),
        ("opaque_index_offset", json!(u64::MAX)),
        ("opaque_scalar_offset", json!(123)),
    ] {
        let mut wire = serde_json::to_value(frame_wire()).unwrap();
        wire[field] = value;
        assert!(serde_json::from_value::<DesignConstructionOperandGroupFrame>(wire).is_err());
    }
    let mut wire = frame_wire();
    wire.trailing_record_indices.push(301);
    wire.trailing_record_offsets.push(101);
    assert!(DesignConstructionOperandGroupFrame::try_from(wire.clone()).is_err());
    assert!(
        serde_json::from_value::<DesignConstructionOperandGroupFrame>(
            serde_json::to_value(wire).unwrap()
        )
        .is_err()
    );
}

#[test]
fn construction_frame_collections_reject_duplicates_and_keep_failed_edits_atomic() {
    let frame = DesignConstructionOperandGroupFrame::try_from(frame_wire()).unwrap();
    macro_rules! check_collection {
        ($field:ident, $setter:ident) => {{
            let mut wire = frame_wire();
            wire.$field.push(wire.$field[0].clone());
            assert!(DesignConstructionOperandGroupFrame::try_from(wire.clone()).is_err());
            assert!(
                serde_json::from_value::<DesignConstructionOperandGroupFrame>(
                    serde_json::to_value(&wire).unwrap()
                )
                .is_err()
            );
            let mut edited = frame.clone();
            assert!(edited.$setter(wire.$field).is_err());
            assert_eq!(edited, frame);
        }};
    }
    check_collection!(trailing_flags, try_set_trailing_flags);
    check_collection!(trailing_transforms, try_set_trailing_transforms);
    check_collection!(trailing_dual_transforms, try_set_trailing_dual_transforms);
    check_collection!(auxiliary_paths, try_set_auxiliary_paths);
    let wire = serde_json::to_value(frame_wire()).unwrap();
    assert_eq!(serde_json::to_value(frame).unwrap(), wire);
}

#[test]
fn construction_frame_borrowed_wire_matches_owned_wire_bytes() {
    for wire in [
        serde_json::json!({
            "member_count_offset": 20,
            "opaque_index": 1, "opaque_index_offset": 80,
            "opaque_scalar": 0.0, "opaque_scalar_offset": 84, "variant": false
        }),
        serde_json::to_value(frame_wire()).unwrap(),
    ] {
        let frame: DesignConstructionOperandGroupFrame = serde_json::from_value(wire).unwrap();
        let owned = DesignConstructionOperandGroupFrameWire::from(frame.clone());
        assert_eq!(
            serde_json::to_vec(&frame).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn construction_frame_native_retained_limit_refuses_before_clone() {
    #[derive(serde::Serialize)]
    struct NestedRecord<'a> {
        id: &'static str,
        value: &'a DesignConstructionOperandGroupFrame,
    }
    let frame = DesignConstructionOperandGroupFrame::try_from(frame_wire()).unwrap();
    let record = NestedRecord {
        id: "f3d:native:construction-frame#0",
        value: &frame,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_construction_operand_groups",
        || super::CONSTRUCTION_FRAME_CLONE_COUNT.with(|count| count.set(0)),
        || super::CONSTRUCTION_FRAME_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn construction_group_borrowed_wire_matches_owned_wire_bytes() {
    let wire = json!({
        "id":"f3d:native:construction-group#0", "scope_record_index":7,
        "scope_reference_ordinal":0, "record_index":9,"byte_offset":0,
        "class_tag":"277", "members":[10,11],"member_offsets":[26,37],
        "lost_edge_references":["edge#1"], "frame":frame_wire(),
        "role":0,"role_offset":100,"paired_class_tag":"278","paired_byte_offset":200
    });
    let mut group: DesignConstructionOperandGroup = serde_json::from_value(wire).unwrap();
    for role in [
        super::DesignConstructionOperandRole::Other(super::DesignOperandRole::from_raw(0)),
        super::DesignConstructionOperandRole::ExtrudeBodiesA,
        super::DesignConstructionOperandRole::ExtrudeProfile,
        super::DesignConstructionOperandRole::ExtrudeFaces {
            encoding: super::DesignExtrudeFaceEncoding::SelectedStart,
            usage: super::DesignExtrudeFaceRole::Start,
        },
    ] {
        group.operand_role = role;
        let owned = super::DesignConstructionOperandGroupSerde::from(group.clone());
        assert_eq!(
            serde_json::to_vec(&group).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn construction_group_native_retained_limit_refuses_before_clone() {
    let wire = json!({
        "id":"f3d:native:construction-group#0", "scope_record_index":7,
        "scope_reference_ordinal":0, "record_index":9,"byte_offset":0,
        "class_tag":"277", "members":[10,11],"member_offsets":[26,37],
        "frame":frame_wire(), "role":0,"role_offset":100,
        "paired_class_tag":"278","paired_byte_offset":200
    });
    let group: DesignConstructionOperandGroup = serde_json::from_value(wire).unwrap();
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &group,
        "design_construction_operand_groups",
        || super::CONSTRUCTION_GROUP_CLONE_COUNT.with(|count| count.set(0)),
        || super::CONSTRUCTION_GROUP_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn construction_group_owns_member_stride_and_derives_role_offset() {
    let wire = json!({
        "id":"stream:group", "scope_record_index":7, "scope_reference_ordinal":0,
        "record_index":9,"byte_offset":0,"class_tag":"277",
        "members":[10,11],"member_offsets":[26,37],"frame":frame_wire(),
        "role":0,"role_offset":100,"paired_class_tag":"278","paired_byte_offset":200
    });
    let mut group: DesignConstructionOperandGroup = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(group.role_offset(), 100);
    for offsets in [json!([26, 36]), json!([u64::MAX, u64::MAX])] {
        let mut invalid = wire.clone();
        invalid["member_offsets"] = offsets;
        assert!(serde_json::from_value::<DesignConstructionOperandGroup>(invalid).is_err());
    }
    let mut invalid = wire.clone();
    invalid["role_offset"] = json!(101);
    assert!(serde_json::from_value::<DesignConstructionOperandGroup>(invalid).is_err());
    let before = group.clone();
    assert!(group
        .try_set_members(vec![
            Located {
                value: 10,
                offset: 26
            },
            Located {
                value: 11,
                offset: 36
            }
        ])
        .is_err());
    assert_eq!(group, before);
    assert_eq!(serde_json::to_value(group).unwrap(), wire);
}

#[test]
fn tracking_identities_preserve_wire_and_reject_partial_locations() {
    let prefix = r#"{"wrapper_record_index":300,"wrapper_byte_offset":0,"wrapper_class_tag":"361","carrier_record_index":301,"carrier_byte_offset":33,"carrier_class_tag":"362","primary_identity":268,"primary_identity_offset":70,"selector":1,"selector_offset":90,"kind":3,"kind_offset":94"#;
    let suffix =
        r#","following_record_index":302,"following_byte_offset":130,"following_class_tag":"363"}"#;
    for fields in ["", ",\"first_related_identity\":113,\"first_related_identity_offset\":110,\"second_related_identity\":119,\"second_related_identity_offset\":122"] {
        let suffix = if fields.is_empty() { suffix.replace("130", "114") } else { suffix.to_owned() };
        let wire = format!("{prefix}{fields}{suffix}");
        let value: super::DesignConstructionTrackingPath = serde_json::from_str(&wire).expect("tracking path");
        assert_eq!(serde_json::to_string(&value).expect("tracking wire"), wire);
    }
    for field in [
        "first_related_identity",
        "first_related_identity_offset",
        "second_related_identity",
        "second_related_identity_offset",
    ] {
        let error = serde_json::from_str::<super::DesignConstructionTrackingPath>(&format!(
            "{prefix},\"{field}\":1{suffix}"
        ))
        .expect_err("partial identity location");
        assert!(error.to_string().contains(field));
    }
}

#[test]
fn construction_tracking_path_borrowed_wire_matches_owned_wire_bytes() {
    let prefix = r#"{"wrapper_record_index":300,"wrapper_byte_offset":0,"wrapper_class_tag":"361","carrier_record_index":301,"carrier_byte_offset":33,"carrier_class_tag":"362","primary_identity":268,"primary_identity_offset":70,"selector":1,"selector_offset":90,"kind":3,"kind_offset":94"#;
    for (fields, end) in [
        ("", 114),
        (",\"first_related_identity\":113,\"first_related_identity_offset\":110,\"second_related_identity\":119,\"second_related_identity_offset\":122", 130),
    ] {
        let wire = format!("{prefix}{fields},\"following_record_index\":302,\"following_byte_offset\":{end},\"following_class_tag\":\"363\"}}");
        let path: DesignConstructionTrackingPath = serde_json::from_str(&wire).unwrap();
        let owned = super::DesignConstructionTrackingPathWire::from(path.clone());
        assert_eq!(serde_json::to_vec(&path).unwrap(), serde_json::to_vec(&owned).unwrap());
    }
}

#[test]
fn construction_tracking_path_native_retained_limit_refuses_before_clone() {
    #[derive(serde::Serialize)]
    struct NestedRecord<'a> {
        id: &'static str,
        value: &'a DesignConstructionTrackingPath,
    }
    let wire = r#"{"wrapper_record_index":300,"wrapper_byte_offset":0,"wrapper_class_tag":"361","carrier_record_index":301,"carrier_byte_offset":33,"carrier_class_tag":"362","primary_identity":268,"primary_identity_offset":70,"selector":1,"selector_offset":90,"kind":3,"kind_offset":94,"following_record_index":302,"following_byte_offset":114,"following_class_tag":"363"}"#;
    let path: DesignConstructionTrackingPath = serde_json::from_str(wire).unwrap();
    let record = NestedRecord {
        id: "f3d:native:tracking-path#0",
        value: &path,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_scopes",
        || super::CONSTRUCTION_TRACKING_PATH_CLONE_COUNT.with(|count| count.set(0)),
        || super::CONSTRUCTION_TRACKING_PATH_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
// Fixture fields are appended from the bounded table of explicit test cases.
#[allow(clippy::format_push_string)]
fn construction_path_preserves_layout_wire_and_rejects_mixed_forms() {
    let prefix = r#"{"record_index":100,"byte_offset":0,"class_tag":"304","entity_ref":174,"entity_ref_offset":22"#;
    let suffix = r#","scope_record_index":90,"scope_record_index_offset":163,"nested_record_index":102,"nested_record_index_offset":174,"following_record_index":101,"following_byte_offset":190,"following_class_tag":"390"}"#;
    let fields = [
        (
            "transform",
            "[[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]]",
        ),
        ("transform_offset", "33"),
        ("compact_variant", "false"),
    ];
    for mask in 0..8 {
        let mut wire = prefix.to_owned();
        for (index, (field, value)) in fields.iter().enumerate() {
            if mask & (1 << index) != 0 {
                wire.push_str(&format!(",\"{field}\":{value}"));
            }
        }
        let suffix = if mask == 4 {
            suffix
                .replace(":163", ":35")
                .replace(":174", ":46")
                .replace(":190", ":62")
        } else {
            suffix.to_owned()
        };
        wire.push_str(&suffix);
        let result = serde_json::from_str::<super::DesignConstructionOperandPath>(&wire);
        if mask == 3 || mask == 4 {
            assert_eq!(
                serde_json::to_string(&result.expect("complete placement form"))
                    .expect("path wire"),
                wire
            );
        } else {
            let error = result.expect_err("invalid placement form").to_string();
            for (field, _) in fields {
                assert!(error.contains(field));
            }
        }
    }
}

#[test]
fn construction_path_borrowed_wire_matches_owned_wire_bytes() {
    for wire in [
        r#"{"record_index":100,"byte_offset":0,"class_tag":"304","entity_ref":174,"entity_ref_offset":22,"compact_variant":false,"scope_record_index":90,"scope_record_index_offset":35,"nested_record_index":102,"nested_record_index_offset":46,"following_record_index":101,"following_byte_offset":62,"following_class_tag":"390"}"#,
        r#"{"record_index":100,"byte_offset":0,"class_tag":"304","entity_ref":174,"entity_ref_offset":22,"transform":[[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]],"transform_offset":33,"scope_record_index":90,"scope_record_index_offset":163,"nested_record_index":102,"nested_record_index_offset":174,"following_record_index":101,"following_byte_offset":190,"following_class_tag":"390"}"#,
    ] {
        let path: super::DesignConstructionOperandPath = serde_json::from_str(wire).unwrap();
        let owned = super::DesignConstructionOperandPathWire::from(path.clone());
        assert_eq!(
            serde_json::to_vec(&path).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn construction_path_native_retained_limit_refuses_before_clone() {
    #[derive(serde::Serialize)]
    struct NestedRecord<'a> {
        id: &'static str,
        value: &'a super::DesignConstructionOperandPath,
    }
    let path: super::DesignConstructionOperandPath = serde_json::from_str(
        r#"{"record_index":100,"byte_offset":0,"class_tag":"304","entity_ref":174,"entity_ref_offset":22,"compact_variant":false,"scope_record_index":90,"scope_record_index_offset":35,"nested_record_index":102,"nested_record_index_offset":46,"following_record_index":101,"following_byte_offset":62,"following_class_tag":"390"}"#
    ).unwrap();
    let record = NestedRecord {
        id: "f3d:native:construction-path#0",
        value: &path,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_construction_operand_identities",
        || super::CONSTRUCTION_PATH_CLONE_COUNT.with(|count| count.set(0)),
        || super::CONSTRUCTION_PATH_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn identity_wrapper_rows_preserve_wire_and_reject_unequal_arrays() {
    for count in 0..=2 {
        for offsets in 0..=2 {
            for tags in 0..=2 {
                let indices = ["[]", "[300]", "[300,305]"][count];
                let offsets_wire = ["[]", "[0]", "[0,24]"][offsets];
                let tags_wire = ["[]", "[\"384\"]", "[\"384\",\"289\"]"][tags];
                let following_byte_offset = count * 24;
                let wire = format!(
                    r#"{{"id":"identity#0","group_record_index":200,"wrapper_record_indices":{indices},"wrapper_byte_offsets":{offsets_wire},"wrapper_class_tags":{tags_wire},"following_record_index":310,"following_byte_offset":{following_byte_offset},"following_class_tag":"304"}}"#
                );
                let parsed =
                    serde_json::from_str::<super::DesignConstructionOperandIdentity>(&wire);
                if count == offsets && count == tags {
                    assert_eq!(
                        serde_json::to_string(&parsed.expect("complete rows"))
                            .expect("identity wire"),
                        wire
                    );
                } else {
                    let error = parsed.expect_err("unequal wrapper arrays").to_string();
                    assert!(error.contains("wrapper_record_indices"));
                    assert!(error.contains("wrapper_byte_offsets"));
                    assert!(error.contains("wrapper_class_tags"));
                }
            }
        }
    }
}

#[test]
fn construction_identity_borrowed_wire_matches_owned_wire_bytes() {
    for count in 0..=2 {
        let indices = ["[]", "[300]", "[300,305]"][count];
        let offsets = ["[]", "[0]", "[0,24]"][count];
        let tags = ["[]", "[\"384\"]", "[\"384\",\"289\"]"][count];
        let following_byte_offset = count * 24;
        let wire = format!(
            r#"{{"id":"f3d:native:construction-identity#0","group_record_index":200,"wrapper_record_indices":{indices},"wrapper_byte_offsets":{offsets},"wrapper_class_tags":{tags},"following_record_index":310,"following_byte_offset":{following_byte_offset},"following_class_tag":"304"}}"#
        );
        let identity: super::DesignConstructionOperandIdentity =
            serde_json::from_str(&wire).unwrap();
        let owned = super::DesignConstructionOperandIdentityWire::from(identity.clone());
        assert_eq!(
            serde_json::to_vec(&identity).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn construction_identity_native_retained_limit_refuses_before_clone() {
    let identity: super::DesignConstructionOperandIdentity = serde_json::from_str(
        r#"{"id":"f3d:native:construction-identity#0","group_record_index":200,"wrapper_record_indices":[300,305],"wrapper_byte_offsets":[0,24],"wrapper_class_tags":["384","289"],"following_record_index":310,"following_byte_offset":48,"following_class_tag":"304"}"#
    ).unwrap();
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &identity,
        "design_construction_operand_identities",
        || super::CONSTRUCTION_IDENTITY_CLONE_COUNT.with(|count| count.set(0)),
        || super::CONSTRUCTION_IDENTITY_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn construction_auxiliary_rows_preserve_wire_and_reject_unequal_offsets() {
    for fields in [
        "",
        r#","auxiliary_record_indices":[103,106],"auxiliary_record_offsets":[37,48]"#,
    ] {
        let wire = format!(
            r#"{{"member_count_offset":20{fields},"opaque_index":1,"opaque_index_offset":80,"opaque_scalar":0.0,"opaque_scalar_offset":84,"variant":false}}"#
        );
        let frame: super::DesignConstructionOperandGroupFrame =
            serde_json::from_str(&wire).expect("construction frame");
        assert_eq!(
            serde_json::to_string(&frame).expect("construction wire"),
            wire
        );
    }
    for fields in [
        r#", "auxiliary_record_indices":[103]"#,
        r#", "auxiliary_record_offsets":[37]"#,
        r#", "auxiliary_record_indices":[103,106],"auxiliary_record_offsets":[37]"#,
    ] {
        let wire = format!(
            r#"{{"member_count_offset":20{fields},"opaque_index":1,"opaque_index_offset":80,"opaque_scalar":0.0,"opaque_scalar_offset":84,"variant":false}}"#
        );
        let error = serde_json::from_str::<super::DesignConstructionOperandGroupFrame>(&wire)
            .expect_err("unequal auxiliary arrays")
            .to_string();
        assert!(error.contains("auxiliary_record_indices"));
        assert!(error.contains("auxiliary_record_offsets"));
    }
}

#[test]
fn construction_trailing_rows_preserve_wire_and_reject_unequal_offsets() {
    for fields in [
        "",
        r#","trailing_record_indices":[300],"trailing_record_offsets":[1044]"#,
    ] {
        let wire = format!(
            r#"{{"member_count_offset":20{fields},"opaque_index":1,"opaque_index_offset":80,"opaque_scalar":0.0,"opaque_scalar_offset":84,"variant":false}}"#
        );
        let frame: super::DesignConstructionOperandGroupFrame =
            serde_json::from_str(&wire).expect("construction frame");
        assert_eq!(
            serde_json::to_string(&frame).expect("construction wire"),
            wire
        );
    }
    for fields in [
        r#","trailing_record_indices":[300]"#,
        r#","trailing_record_offsets":[1044]"#,
        r#","trailing_record_indices":[300,301],"trailing_record_offsets":[1044]"#,
    ] {
        let wire = format!(
            r#"{{"member_count_offset":20{fields},"opaque_index":1,"opaque_index_offset":80,"opaque_scalar":0.0,"opaque_scalar_offset":84,"variant":false}}"#
        );
        let error = serde_json::from_str::<super::DesignConstructionOperandGroupFrame>(&wire)
            .expect_err("unequal trailing arrays")
            .to_string();
        assert!(error.contains("trailing_record_indices"));
        assert!(error.contains("trailing_record_offsets"));
    }
}

#[test]
fn construction_member_rows_preserve_wire_and_reject_unequal_offsets() {
    for (members, offsets) in [("[]", "[]"), ("[10]", "[0]"), ("[10,11]", "[26,37]")] {
        let wire = format!(
            r#"{{"id":"group","scope_record_index":7,"scope_reference_ordinal":0,"record_index":9,"byte_offset":0,"class_tag":"277","members":{members},"member_offsets":{offsets},"frame":{{"member_count_offset":21,"opaque_index":1,"opaque_index_offset":78,"opaque_scalar":0.0,"opaque_scalar_offset":82,"variant":false}},"role":0,"role_offset":60,"paired_class_tag":"278","paired_byte_offset":100}}"#
        );
        let group: super::DesignConstructionOperandGroup =
            serde_json::from_str(&wire).expect("construction group");
        assert_eq!(
            serde_json::to_string(&group).expect("construction wire"),
            wire
        );
        let invalid = wire.replace(
            &format!("\"member_offsets\":{offsets}"),
            "\"member_offsets\":[1,2,3]",
        );
        let error = serde_json::from_str::<super::DesignConstructionOperandGroup>(&invalid)
            .expect_err("unequal member arrays")
            .to_string();
        assert!(error.contains("members"));
        assert!(error.contains("member_offsets"));
        for (roles, valid) in [
            (r#""extrude_role":"bodies","#, true),
            (
                r#""extrude_role":"faces","extrude_face_role":"start","#,
                true,
            ),
            (r#""extrude_role":"faces","#, false),
            (r#""extrude_face_role":"start","#, false),
            (
                r#""extrude_role":"bodies","extrude_face_role":"start","#,
                false,
            ),
        ] {
            let role = if roles.contains("bodies") {
                super::super::extrude_selection::DesignOperandRole::BODIES_A
            } else {
                super::super::extrude_selection::DesignOperandRole::FACES
            };
            let tagged = wire.replace(r#""role":0,"#, &format!("\"role\":{},{roles}", role.raw()));
            let parsed = serde_json::from_str::<super::DesignConstructionOperandGroup>(&tagged);
            if valid {
                assert_eq!(
                    serde_json::to_string(&parsed.expect("tagged construction group"))
                        .expect("tagged construction wire"),
                    tagged
                );
            } else {
                assert!(parsed
                    .expect_err("unpaired extrude role")
                    .to_string()
                    .contains("extrude_face_role"));
            }
        }
    }
}

#[test]
fn construction_group_wire_requires_source_and_extrude_roles_to_agree() {
    use super::super::extrude_selection::DesignOperandRole;
    let wire = serde_json::json!({
        "id": "group", "scope_record_index": 1, "scope_reference_ordinal": 0,
        "record_index": 2, "byte_offset": 10, "class_tag": "256",
        "members": [], "member_offsets": [],
        "frame": {"member_count_offset": 20, "opaque_index": 1,
            "opaque_index_offset": 58, "opaque_scalar": 0.0,
            "opaque_scalar_offset": 62, "variant": false},
        "role": DesignOperandRole::PROFILE.raw(), "extrude_role": "profile",
        "role_offset": 40, "paired_class_tag": "257", "paired_byte_offset": 50
    });
    let group: DesignConstructionOperandGroup = serde_json::from_value(wire.clone()).unwrap();
    let encoded = serde_json::to_value(&group).unwrap();
    assert_eq!(encoded["role"], wire["role"]);
    assert_eq!(encoded["extrude_role"], wire["extrude_role"]);
    assert_eq!(
        serde_json::from_value::<DesignConstructionOperandGroup>(encoded).unwrap(),
        group
    );
    for (role, extrude_role, face_role) in [
        (DesignOperandRole::PROFILE, "bodies", None),
        (DesignOperandRole::BODIES_A, "profile", None),
        (DesignOperandRole::PROFILE, "faces", Some("start")),
        (DesignOperandRole::FACES, "faces", None),
        (DesignOperandRole::PROFILE, "profile", Some("termination")),
    ] {
        let mut invalid = wire.clone();
        invalid["role"] = serde_json::json!(role.raw());
        invalid["extrude_role"] = serde_json::json!(extrude_role);
        if let Some(face_role) = face_role {
            invalid["extrude_face_role"] = serde_json::json!(face_role);
        }
        let error = serde_json::from_value::<DesignConstructionOperandGroup>(invalid).unwrap_err();
        assert!(error.to_string().contains("role"));
    }
}

#[test]
fn trailing_transform_rejects_displaced_frames_and_overflow() {
    let wire = json!({
        "record_index": 7, "byte_offset": 100, "class_tag": "300",
        "transform": crate::records::identity::IDENTITY_MATRIX, "transform_offset": 122,
        "following_record_index": 8, "following_byte_offset": 252,
        "following_class_tag": "301"
    });
    rejects_changed_fields::<DesignConstructionOperandTransform>(
        wire.clone(),
        &[
            "record_index",
            "byte_offset",
            "transform_offset",
            "following_byte_offset",
        ],
    );
    for index in [0, 6, 7] {
        let mut invalid = wire.clone();
        invalid["following_record_index"] = index.into();
        assert!(serde_json::from_value::<DesignConstructionOperandTransform>(invalid).is_err());
    }
    for index in [9, 10, 4000] {
        let mut independent = wire.clone();
        independent["following_record_index"] = index.into();
        let parsed: DesignConstructionOperandTransform =
            serde_json::from_value(independent).unwrap();
        assert_eq!(parsed.following_record_index(), index);
    }
    let mut boundary = wire;
    boundary["record_index"] = (u32::MAX - 1).into();
    boundary["following_record_index"] = u32::MAX.into();
    boundary["byte_offset"] = (u64::MAX - 152).into();
    boundary["transform_offset"] = (u64::MAX - 130).into();
    boundary["following_byte_offset"] = u64::MAX.into();
    let admitted: DesignConstructionOperandTransform =
        serde_json::from_value(boundary.clone()).unwrap();
    let mut draft = admitted.into_draft();
    draft.record_index = u32::MAX;
    assert!(DesignConstructionOperandTransform::try_new(draft).is_err());
    boundary["byte_offset"] = (u64::MAX - 151).into();
    assert!(serde_json::from_value::<DesignConstructionOperandTransform>(boundary).is_err());
}

#[test]
fn tracking_optional_identities_select_the_frame_extent() {
    for first in [false, true] {
        for second in [false, true] {
            let mut wire = json!({
                "wrapper_record_index": 7, "wrapper_byte_offset": 100, "wrapper_class_tag": "300",
                "carrier_record_index": 8, "carrier_byte_offset": 133, "carrier_class_tag": "301",
                "primary_identity": 9, "primary_identity_offset": 170,
                "selector": 1, "selector_offset": 190, "kind": 3, "kind_offset": 194,
                "following_record_index": 9, "following_byte_offset": 214 + u64::from(first) * 8 + u64::from(second) * 8,
                "following_class_tag": "302"
            });
            if first {
                wire["first_related_identity"] = 0.into();
                wire["first_related_identity_offset"] = 210.into();
            }
            if second {
                wire["second_related_identity"] = 0.into();
                wire["second_related_identity_offset"] = (214 + u64::from(first) * 8).into();
            }
            rejects_changed_fields::<DesignConstructionTrackingPath>(
                wire,
                &[
                    "carrier_record_index",
                    "carrier_byte_offset",
                    "primary_identity_offset",
                    "selector_offset",
                    "kind_offset",
                    "following_record_index",
                    "following_byte_offset",
                ],
            );
        }
    }
}

#[test]
fn persistent_identity_admits_both_tail_extents_and_rejects_displaced_offsets() {
    for (tail, next) in [(0, 190), (185, 200)] {
        let wire = json!({
            "local_id": 1, "local_id_offset": 21,
            "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d", "asset_id_offset": 33,
            "context_id": "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e", "context_id_offset": 109,
            "tail_slot_present": false, "tail_slot_offset": tail,
            "next_record_index": 0, "next_byte_offset": next
        });
        rejects_changed_fields::<DesignConstructionPersistentIdentity>(
            wire,
            &["local_id_offset", "asset_id_offset", "next_byte_offset"],
        );
    }
}

#[test]
fn persistent_identity_borrowed_wire_matches_owned_wire_bytes() {
    for (tail, next) in [(0, 190), (185, 200)] {
        let value: DesignConstructionPersistentIdentity = serde_json::from_value(json!({
            "local_id": 1, "local_id_offset": 21,
            "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d", "asset_id_offset": 33,
            "context_id": "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e", "context_id_offset": 109,
            "tail_slot_present": false, "tail_slot_offset": tail,
            "next_record_index": 0, "next_byte_offset": next
        }))
        .unwrap();
        let owned = super::DesignConstructionPersistentIdentityDraft::from(value.clone());
        assert_eq!(
            serde_json::to_vec(&value).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn persistent_identity_native_retained_limit_refuses_before_clone() {
    #[derive(serde::Serialize)]
    struct NestedRecord<'a> {
        id: &'static str,
        value: &'a DesignConstructionPersistentIdentity,
    }
    let value: DesignConstructionPersistentIdentity = serde_json::from_value(json!({
        "local_id": 1, "local_id_offset": 21,
        "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d", "asset_id_offset": 33,
        "context_id": "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e", "context_id_offset": 109,
        "tail_slot_present": false, "tail_slot_offset": 0,
        "next_record_index": 0, "next_byte_offset": 190
    }))
    .unwrap();
    let record = NestedRecord {
        id: "f3d:native:persistent-identity#0",
        value: &value,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_scopes",
        || super::PERSISTENT_IDENTITY_CLONE_COUNT.with(|count| count.set(0)),
        || super::PERSISTENT_IDENTITY_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

/// A top-level optional key on a topology record names itself in its refusal.
#[test]
fn a_top_level_topology_key_names_itself_in_its_refusal() {
    for key in ["transform", "transform_offset", "compact_variant"] {
        states_the_key(
            key,
            &refusal::<super::DesignConstructionOperandPathWire>(key),
        );
    }
    for key in ["first_related_identity", "second_related_identity"] {
        states_the_key(
            key,
            &refusal::<super::DesignConstructionTrackingPathWire>(key),
        );
    }
}

#[test]
fn tracking_fragment_domain_rejects_nonpositive_and_out_of_range_ordinals() {
    let base = json!({
        "wrapper_record_index":300,"wrapper_byte_offset":0,"wrapper_class_tag":"361",
        "carrier_record_index":301,"carrier_byte_offset":33,"carrier_class_tag":"362",
        "primary_identity":268,"primary_identity_offset":70,
        "selector":1,"selector_offset":90,"kind":1,"kind_offset":94,
        "following_record_index":302,"following_byte_offset":114,"following_class_tag":"363"
    });
    for (selector, kind) in [(1, 1), (1, 3), (3, 3), (i32::MAX, u32::MAX)] {
        let mut wire = base.clone();
        wire["selector"] = json!(selector);
        wire["kind"] = json!(kind);
        let value: DesignConstructionTrackingPath = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(value).unwrap(), wire);
    }
    for (selector, kind) in [(-1, 3), (0, 3), (4, 3), (1, 0), (i32::MIN, u32::MAX)] {
        let mut wire = base.clone();
        wire["selector"] = json!(selector);
        wire["kind"] = json!(kind);
        let error = serde_json::from_value::<DesignConstructionTrackingPath>(wire).unwrap_err();
        assert!(error
            .to_string()
            .contains(if kind == 0 { "kind" } else { "selector" }));
    }
}
