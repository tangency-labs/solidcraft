// SPDX-License-Identifier: Apache-2.0
//! Base-feature construction wire and native admission.

use super::{DesignBaseFeatureConstruction, DesignBaseFeatureConstructionWire};
use serde_json::json;

fn construction_wires() -> Vec<serde_json::Value> {
    vec![
        json!({
            "body_entity_suffixes":[101], "body_entity_suffix_offsets":[22],
            "body_entity_fields":["000001000000"],
            "body_reference_records":[201], "body_reference_record_offsets":[52],
            "body_reference_fields":["000001000000"],
            "repeated_reference_fields":["000001000000"],
            "metadata_record":401, "metadata_record_offset":110,
            "metadata_field":"0000", "result_records":[301],
            "result_record_offsets":[82], "result_fields":["000001000000"]
        }),
        json!({
            "body_entity_suffixes":[201], "body_entity_suffix_offsets":[22],
            "body_reference_records":[201], "body_reference_record_offsets":[22],
            "parameter_body_record":198, "parameter_body_record_offset":100,
            "auxiliary_record":202, "auxiliary_record_offset":120,
            "envelope_guid":"fcec56e3-832f-4468-88a4-d710e62e629f",
            "envelope_guid_offset":140, "tag_body_based_on_faces":true,
            "tag_body_based_on_faces_offset":90
        }),
        json!({
            "form":"compact_one_body", "mode":0, "mode_offset":17,
            "body_entity_suffixes":[101], "body_entity_suffix_offsets":[22],
            "body_entity_fields":["000001000000"],
            "body_reference_records":[101], "body_reference_record_offsets":[22],
            "parameter_body_records":[301], "parameter_body_record_offsets":[50],
            "auxiliary_records":[303], "auxiliary_record_offsets":[70],
            "scope_reference":90, "scope_reference_offset":100,
            "envelope_guid":"11111111-2222-3333-4444-555555555555",
            "envelope_guid_offset":110, "tag_body_based_on_faces":true,
            "tag_body_based_on_faces_offset":190
        }),
        json!({
            "form":"expanded_two_body",
            "body_entity_suffixes":[101,102], "body_entity_suffix_offsets":[22,37],
            "body_entity_fields":["000001000000","000001000000"],
            "body_reference_records":[101,102], "body_reference_record_offsets":[22,37],
            "parameter_body_records":[301,302], "parameter_body_record_offsets":[50,60],
            "auxiliary_records":[303,304], "auxiliary_record_offsets":[70,80],
            "scope_reference":90, "scope_reference_offset":100,
            "envelope_guid":"11111111-2222-3333-4444-555555555555",
            "envelope_guid_offset":110, "tag_body_based_on_faces":true,
            "tag_body_based_on_faces_offset":190
        }),
        json!({
            "body_entity_suffixes":[101,202], "body_entity_suffix_offsets":[22,37],
            "body_entity_fields":["000003040506","000004030201"],
            "related_guids":["aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
                "cccccccc-cccc-4ccc-8ccc-cccccccccccc"],
            "related_guid_offsets":[66,142,275], "linkage_record":301,
            "linkage_record_offset":234, "auxiliary_record":401,
            "auxiliary_record_offset":253
        }),
    ]
}

#[test]
fn base_feature_construction_borrowed_wire_matches_owned_wire_bytes() {
    for wire in construction_wires() {
        let value: DesignBaseFeatureConstruction = serde_json::from_value(wire).unwrap();
        let owned = DesignBaseFeatureConstructionWire::from(value.clone());
        assert_eq!(
            serde_json::to_vec(&value).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn base_feature_construction_native_retained_limit_refuses_before_clone() {
    #[derive(serde::Serialize)]
    struct NestedRecord<'a> {
        id: &'static str,
        value: &'a DesignBaseFeatureConstruction,
    }
    let value: DesignBaseFeatureConstruction =
        serde_json::from_value(construction_wires().remove(3)).unwrap();
    let record = NestedRecord {
        id: "f3d:native:base-feature#0",
        value: &value,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_scopes",
        || super::BASE_FEATURE_CONSTRUCTION_CLONE_COUNT.with(|count| count.set(0)),
        || super::BASE_FEATURE_CONSTRUCTION_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn base_feature_native_fixed_body_fields_refuse_nonlocal_flags() {
    for wire in construction_wires() {
        for name in ["body_entity_fields", "body_reference_fields"] {
            let Some(fields) = wire.get(name).and_then(serde_json::Value::as_array) else {
                continue;
            };
            if fields.is_empty() {
                continue;
            }
            for encoding in ["010000000000", "000100000000", "240000000000"] {
                let mut invalid = wire.clone();
                invalid[name][0] = json!(encoding);
                assert!(serde_json::from_value::<DesignBaseFeatureConstruction>(invalid).is_err());
            }
            let mut maximum_member = wire.clone();
            maximum_member[name][0] = json!("0000ffffffff");
            let admitted: DesignBaseFeatureConstruction =
                serde_json::from_value(maximum_member).unwrap();
            assert_eq!(
                serde_json::to_value(admitted).unwrap()[name][0],
                "0000ffffffff"
            );
        }
    }
}
