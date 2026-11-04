// SPDX-License-Identifier: Apache-2.0

use super::{
    DesignComponentInsertConstruction, DesignComponentInsertConstructionWire,
    DesignComponentOccurrence, DesignComponentOccurrenceWire, COMPONENT_INSERT_CLONE_COUNT,
    COMPONENT_OCCURRENCE_CLONE_COUNT,
};

fn occurrence(explicit: bool) -> DesignComponentOccurrence {
    let placement = if explicit {
        r#","occurrence_ordinal":2,"transform":[[1.0,0.0,0.0,2.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]],"transform_offset":219,"owner_entity":3,"owner_entity_offset":357"#
    } else {
        r#","occurrence_ordinal":1,"owner_entity":3,"owner_entity_offset":229"#
    };
    serde_json::from_str(&format!(
        r#"{{"id":"f3d:native:occurrence#0","class_tag":"292","record_index":1,"byte_offset":10,"component_record_index":2,"component_guid":"aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee","component_guid_offset":58,"occurrence_guid":"ffffffff-bbbb-4ccc-8ddd-eeeeeeeeeeee","occurrence_guid_offset":134{placement}}}"#
    ))
    .unwrap()
}

#[test]
fn component_occurrence_borrowed_wire_matches_owned_wire_bytes() {
    for record in [occurrence(false), occurrence(true)] {
        let owned = DesignComponentOccurrenceWire::from(record.clone());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn component_occurrence_native_retained_limit_refuses_before_record_clone() {
    let record = occurrence(true);
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_component_occurrences",
        || COMPONENT_OCCURRENCE_CLONE_COUNT.with(|count| count.set(0)),
        || COMPONENT_OCCURRENCE_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

fn component_insert() -> DesignComponentInsertConstruction {
    serde_json::from_str(
        r#"{"relation_record_index":1,"carrier_record_index":2,"neutron_role":"role","neutron_role_offset":30,"transform":[[1.0,0.0,0.0,2.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]],"transform_offset":50,"carrier_transform_offset":40}"#,
    )
    .unwrap()
}

#[test]
fn component_insert_borrowed_wire_matches_owned_wire_bytes() {
    for record in [
        component_insert(),
        serde_json::from_str(
            r#"{"relation_record_index":1,"carrier_record_index":2,"neutron_role":"role","neutron_role_offset":30,"transform":[[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]]}"#,
        )
        .unwrap(),
    ] {
        let owned = DesignComponentInsertConstructionWire::from(record.clone());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn component_insert_native_retained_limit_refuses_before_record_clone() {
    #[derive(serde::Serialize)]
    struct NativeRecord<'a> {
        id: &'static str,
        construction: &'a DesignComponentInsertConstruction,
    }

    let construction = component_insert();
    let record = NativeRecord {
        id: "f3d:native:component-insert#0",
        construction: &construction,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_scopes",
        || COMPONENT_INSERT_CLONE_COUNT.with(|count| count.set(0)),
        || COMPONENT_INSERT_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn typed_component_occurrence_wire_retains_offsets_and_native_fields() {
    for (explicit, owner_offset) in [(false, 299_u64), (false, 298), (true, 426)] {
        let mut wire = serde_json::to_value(occurrence(explicit)).unwrap();
        wire["component_guid_offset"] = (10 + 88).into();
        wire["occurrence_guid_offset"] = (10 + 164).into();
        wire["owner_entity_offset"] = (10 + owner_offset).into();
        wire["owner_type_guid"] = super::OCCURRENCE_OWNER_TYPE_GUID.into();
        wire["path_discriminator"] = 3.into();
        if explicit {
            wire["transform_offset"] = (10 + 288).into();
        }
        let record: DesignComponentOccurrence = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&record).unwrap(), wire);
        let owned = DesignComponentOccurrenceWire::from(record.clone());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
        wire["owner_entity_offset"] = (12 + owner_offset).into();
        assert!(serde_json::from_value::<DesignComponentOccurrence>(wire).is_err());
    }
}
