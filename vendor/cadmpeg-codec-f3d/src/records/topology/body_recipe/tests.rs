// SPDX-License-Identifier: Apache-2.0
//! Body-recipe operand wire: optional selector tail, nested frame offsets.

use super::DesignBodyRecipeOperand;
use crate::records::topology::test_support::rejects_changed_fields;
use serde_json::json;

#[test]
fn body_recipe_operand_borrowed_wire_matches_owned_wire_bytes() {
    let prefix = r#"{"id":"operand","scope_record_index":1,"scope_reference_ordinal":0,"record_index":2,"byte_offset":0,"class_tag":"365","asset_id":"0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d","asset_id_offset":44,"context_id":"1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e","context_id_offset":150"#;
    let suffix = r#", "references":[],"nested_record_index":5,"nested_record_index_offset":26,"recipe_id":"recipe","next_record_index":6,"next_byte_offset":240}"#;
    for fields in [
        "",
        ",\"selector_tail\":\"07000000\",\"selector_tail_offset\":220",
    ] {
        let value: DesignBodyRecipeOperand =
            serde_json::from_str(&format!("{prefix}{fields}{suffix}")).unwrap();
        let owned = super::DesignBodyRecipeOperandWire::from(value.clone());
        assert_eq!(
            serde_json::to_vec(&value).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn body_recipe_operand_native_retained_limit_refuses_before_clone() {
    #[derive(serde::Serialize)]
    struct NestedRecord<'a> {
        id: &'static str,
        value: &'a DesignBodyRecipeOperand,
    }
    let wire = r#"{"id":"operand","scope_record_index":1,"scope_reference_ordinal":0,"record_index":2,"byte_offset":0,"class_tag":"365","asset_id":"0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d","asset_id_offset":44,"context_id":"1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e","context_id_offset":150,"references":[],"nested_record_index":5,"nested_record_index_offset":26,"recipe_id":"recipe","next_record_index":6,"next_byte_offset":240}"#;
    let value: DesignBodyRecipeOperand = serde_json::from_str(wire).unwrap();
    let record = NestedRecord {
        id: "f3d:native:body-recipe#0",
        value: &value,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_scopes",
        || super::BODY_RECIPE_OPERAND_CLONE_COUNT.with(|count| count.set(0)),
        || super::BODY_RECIPE_OPERAND_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn body_recipe_selector_tail_preserves_wire_and_rejects_partial_locations() {
    let prefix = r#"{"id":"operand","scope_record_index":1,"scope_reference_ordinal":0,"record_index":2,"byte_offset":0,"class_tag":"365","asset_id":"0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d","asset_id_offset":44,"context_id":"1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e","context_id_offset":150"#;
    let suffix = r#","references":[],"nested_record_index":5,"nested_record_index_offset":26,"recipe_id":"recipe","next_record_index":6,"next_byte_offset":240}"#;
    for fields in [
        "",
        ",\"selector_tail\":\"07000000\",\"selector_tail_offset\":220",
    ] {
        let wire = format!("{prefix}{fields}{suffix}");
        let value: super::DesignBodyRecipeOperand =
            serde_json::from_str(&wire).expect("body recipe operand");
        assert_eq!(
            serde_json::to_string(&value).expect("body recipe operand wire"),
            wire
        );
    }
    for fields in [
        ",\"selector_tail\":\"07000000\"",
        ",\"selector_tail_offset\":220",
    ] {
        let error = serde_json::from_str::<super::DesignBodyRecipeOperand>(&format!(
            "{prefix}{fields}{suffix}"
        ))
        .expect_err("partial selector tail location");
        assert!(error.to_string().contains("selector_tail"));
    }
    for guid in [
        "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d",
        "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e",
    ] {
        let error = serde_json::from_str::<super::DesignBodyRecipeOperand>(
            &format!("{prefix}{suffix}").replace(guid, "not-a-guid"),
        )
        .expect_err("non-GUID body recipe identity")
        .to_string();
        assert!(error.contains("GUID"), "{error}");
    }
}

#[test]
fn body_recipe_reference_count_fixes_nested_frame_offsets() {
    let wire = json!({
        "id": "operand", "scope_record_index": 1, "scope_reference_ordinal": 0,
        "record_index": 2, "byte_offset": 0, "class_tag": "365",
        "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d", "asset_id_offset": 56,
        "context_id": "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e", "context_id_offset": 150,
        "references": [{"design_reference": 1, "design_reference_offset": 25, "form": 0, "form_offset": 33}],
        "nested_record_index": 5, "nested_record_index_offset": 38, "recipe_id": "recipe",
        "next_record_index": 6, "next_byte_offset": 240
    });
    rejects_changed_fields::<DesignBodyRecipeOperand>(
        wire.clone(),
        &[
            "nested_record_index",
            "nested_record_index_offset",
            "asset_id_offset",
        ],
    );
    for (next_record_index, admitted) in [(1, true), (2, false), (5, false), (6, true), (900, true)]
    {
        let mut candidate = wire.clone();
        candidate["next_record_index"] = next_record_index.into();
        assert_eq!(
            serde_json::from_value::<DesignBodyRecipeOperand>(candidate).is_ok(),
            admitted,
            "{next_record_index}"
        );
    }
    let mut admitted = serde_json::from_value::<DesignBodyRecipeOperand>(wire.clone())
        .unwrap()
        .into_draft();
    admitted.references.clear();
    assert!(DesignBodyRecipeOperand::try_new(admitted).is_err());
    for field in ["design_reference_offset", "form_offset"] {
        let mut invalid = wire.clone();
        invalid["references"][0][field] = 0.into();
        assert!(serde_json::from_value::<DesignBodyRecipeOperand>(invalid).is_err());
    }
}
