// SPDX-License-Identifier: Apache-2.0
//! Entity-selection operands and the loft legacy body carrier.

use super::DesignEntitySelectionOperand;
use crate::records::topology::test_support::rejects_changed_fields;
use serde_json::json;

fn entity_selection_wire_for_borrowed() -> serde_json::Value {
    json!({
        "id": "f3d:native:entity-selection#0", "scope_record_index": 1,
        "group_record_index": 2, "group_member_ordinal": 0,
        "record_index": 7, "byte_offset": 100, "class_tag": "338",
        "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d", "asset_id_offset": 130,
        "context_id": "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e", "context_id_offset": 210,
        "identity_record_index": 10, "identity_record_offset": 300,
        "primary_identity": 1, "primary_identity_offset": 321,
        "next_record_index": 99, "next_byte_offset": 329
    })
}

#[test]
fn entity_selection_borrowed_wire_matches_owned_wire_bytes() {
    let base = entity_selection_wire_for_borrowed();
    for (primary, secondary, next) in [
        (321, None, 329),
        (329, Some(337), 345),
        (333, Some(341), 349),
    ] {
        let mut wire = base.clone();
        wire["primary_identity_offset"] = primary.into();
        wire["next_byte_offset"] = next.into();
        if let Some(offset) = secondary {
            wire["secondary_identity"] = 2.into();
            wire["secondary_identity_offset"] = offset.into();
            wire["next_record_index"] = 11.into();
        }
        let record: DesignEntitySelectionOperand = serde_json::from_value(wire).unwrap();
        let owned = super::DesignEntitySelectionOperandWire::from(record.clone());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn entity_selection_native_retained_limit_refuses_before_clone() {
    let record: DesignEntitySelectionOperand =
        serde_json::from_value(entity_selection_wire_for_borrowed()).unwrap();
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_entity_selection_operands",
        || super::ENTITY_SELECTION_CLONE_COUNT.with(|count| count.set(0)),
        || super::ENTITY_SELECTION_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

fn loft_carrier_for_borrowed_wire(trailing: bool) -> super::DesignLoftLegacyBodyCarrier {
    let prefix = r#"{"id":"carrier","scope_record_index":12,"scope_reference_ordinal":0,"record_index":20,"byte_offset":0,"class_tag":"322","owner_scope_record_index":12,"owner_scope_record_index_offset":20,"members":[22],"member_offsets":[30],"member_count":1,"member_count_offset":26,"opaque_index":1,"opaque_index_offset":34,"opaque_scalar":1.0,"opaque_scalar_offset":38,"repeated_opaque_index":1,"repeated_opaque_index_offset":46,"next_next_record_index":22,"next_next_reference_offset":50,"flags":"0000","flags_offset":59,"next_record_index":21,"next_reference_offset":61"#;
    let fields = if trailing {
        ",\"trailing_scope_record_index\":12,\"trailing_scope_reference_offset\":88"
    } else {
        ""
    };
    let suffix = r#","paired_class_tag":"262","paired_byte_offset":98}"#;
    let mut record: super::DesignLoftLegacyBodyCarrier =
        serde_json::from_str(&format!("{prefix}{fields}{suffix}")).unwrap();
    record.id = "f3d:native:loft-legacy-body-carrier#0".into();
    record
}

#[test]
fn loft_legacy_body_carrier_borrowed_wire_matches_owned_wire_bytes() {
    for record in [
        loft_carrier_for_borrowed_wire(false),
        loft_carrier_for_borrowed_wire(true),
    ] {
        let owned = super::DesignLoftLegacyBodyCarrierSerde::from(record.clone());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn loft_legacy_body_carrier_native_retained_limit_refuses_before_clone() {
    let record = loft_carrier_for_borrowed_wire(true);
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_loft_legacy_body_carriers",
        || super::LOFT_LEGACY_BODY_CARRIER_CLONE_COUNT.with(|count| count.set(0)),
        || super::LOFT_LEGACY_BODY_CARRIER_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn loft_trailing_scope_reference_preserves_wire_and_rejects_partial_locations() {
    let prefix = r#"{"id":"carrier","scope_record_index":12,"scope_reference_ordinal":0,"record_index":20,"byte_offset":0,"class_tag":"322","owner_scope_record_index":12,"owner_scope_record_index_offset":20,"members":[22],"member_offsets":[30],"member_count":1,"member_count_offset":26,"opaque_index":1,"opaque_index_offset":34,"opaque_scalar":1.0,"opaque_scalar_offset":38,"repeated_opaque_index":1,"repeated_opaque_index_offset":46,"next_next_record_index":22,"next_next_reference_offset":50,"flags":"0000","flags_offset":59,"next_record_index":21,"next_reference_offset":61"#;
    let suffix = r#","paired_class_tag":"262","paired_byte_offset":98}"#;
    for fields in [
        "",
        ",\"trailing_scope_record_index\":12,\"trailing_scope_reference_offset\":88",
    ] {
        let wire = format!("{prefix}{fields}{suffix}");
        let value: super::DesignLoftLegacyBodyCarrier =
            serde_json::from_str(&wire).expect("loft carrier");
        assert_eq!(
            serde_json::to_string(&value).expect("loft carrier wire"),
            wire
        );
    }
    let error = serde_json::from_str::<super::DesignLoftLegacyBodyCarrier>(
        &format!("{prefix},\"trailing_scope_record_index\":13,\"trailing_scope_reference_offset\":88{suffix}"),
    ).expect_err("conflicting owning scope");
    assert!(error.to_string().contains("trailing_scope_record_index"));
    for field in [
        "trailing_scope_record_index",
        "trailing_scope_reference_offset",
    ] {
        let error = serde_json::from_str::<super::DesignLoftLegacyBodyCarrier>(&format!(
            "{prefix},\"{field}\":12{suffix}"
        ))
        .expect_err("partial loft scope reference");
        assert!(error.to_string().contains(field));
    }
    let base: serde_json::Value = serde_json::from_str(&format!("{prefix}{suffix}")).unwrap();
    for (field, value) in [
        ("owner_scope_record_index", serde_json::json!(13)),
        ("repeated_opaque_index", serde_json::json!(2)),
        ("flags", serde_json::json!("0001")),
    ] {
        let mut invalid = base.clone();
        invalid[field] = value;
        assert!(
            serde_json::from_value::<super::DesignLoftLegacyBodyCarrier>(invalid)
                .expect_err("invalid derived field")
                .to_string()
                .contains(field)
        );
    }
    for ordinal in [0, 255, 256, u32::MAX] {
        let mut wire = base.clone();
        wire["opaque_index"] = ordinal.into();
        wire["repeated_opaque_index"] = ordinal.into();
        let parsed = serde_json::from_value::<super::DesignLoftLegacyBodyCarrier>(wire.clone());
        if ordinal == 255 {
            assert_eq!(
                serde_json::to_value(parsed.expect("maximum ordinal")).unwrap(),
                wire
            );
        } else {
            assert!(parsed
                .expect_err("invalid ordinal")
                .to_string()
                .contains("opaque_index"));
        }
    }
}

#[test]
fn entity_selection_retains_primary_paired_and_class_338_forms() {
    let base = json!({
        "id": "operand", "scope_record_index": 1, "group_record_index": 2, "group_member_ordinal": 0,
        "record_index": 7, "byte_offset": 100, "class_tag": "338",
        "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d", "asset_id_offset": 130,
        "context_id": "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e", "context_id_offset": 210,
        "identity_record_index": 10, "identity_record_offset": 300,
        "primary_identity": 1, "primary_identity_offset": 321,
        "next_record_index": 99, "next_byte_offset": 329
    });
    for (primary, secondary, next) in [
        (321, None, 329),
        (329, Some(337), 345),
        (333, Some(341), 349),
    ] {
        let mut wire = base.clone();
        wire["primary_identity_offset"] = primary.into();
        wire["next_byte_offset"] = next.into();
        if let Some(offset) = secondary {
            wire["secondary_identity"] = 2.into();
            wire["secondary_identity_offset"] = offset.into();
            wire["next_record_index"] = 11.into();
        }
        rejects_changed_fields::<DesignEntitySelectionOperand>(
            wire,
            &[
                "identity_record_index",
                "primary_identity_offset",
                "next_byte_offset",
            ],
        );
    }
}

#[test]
fn paired_entity_selection_retains_independent_successor_index() {
    for next_record_index in [6u32, 11, 21, u32::MAX] {
        let mut wire = entity_selection_wire_for_borrowed();
        wire["primary_identity_offset"] = 329.into();
        wire["secondary_identity"] = 2.into();
        wire["secondary_identity_offset"] = 337.into();
        wire["next_byte_offset"] = 345.into();
        wire["next_record_index"] = next_record_index.into();
        let operand: DesignEntitySelectionOperand = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(operand.next_record_index(), next_record_index);
        assert_eq!(serde_json::to_value(&operand).unwrap(), wire);
        rejects_changed_fields::<DesignEntitySelectionOperand>(
            wire,
            &[
                "identity_record_index",
                "primary_identity_offset",
                "next_byte_offset",
            ],
        );
    }
}
