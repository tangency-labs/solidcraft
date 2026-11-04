// SPDX-License-Identifier: Apache-2.0
//! Absence spellings of the flattened coil-record readers.

fn placement(explicit: bool) -> super::DesignCoilPlacement {
    super::DesignCoilPlacement {
        selection_record_index: 1,
        selection_record_byte_offset: 20,
        selection_class_tag: "301".to_owned().try_into().unwrap(),
        selection: super::DesignCoilSelection::Persistent {
            asset_id: "11111111-1111-4111-8111-111111111111"
                .to_owned()
                .try_into()
                .unwrap(),
            context_id: "22222222-2222-4222-8222-222222222222"
                .to_owned()
                .try_into()
                .unwrap(),
            identity_record_index: 2,
            primary_identity: 3,
            secondary: None,
        },
        transform_record_index: 4,
        transform_record_byte_offset: 40,
        transform_class_tag: "302".to_owned().try_into().unwrap(),
        explicit_transform: explicit.then_some(crate::records::identity::Located {
            value: crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
            offset: 50,
        }),
    }
}

#[derive(serde::Serialize)]
struct NestedRecord<'a, T: serde::Serialize> {
    id: &'static str,
    value: &'a T,
}

#[test]
fn coil_placement_borrowed_wire_matches_owned_wire_bytes() {
    for record in [placement(false), placement(true)] {
        let owned = super::DesignCoilPlacementWire::from(record.clone());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn coil_placement_native_retained_limit_refuses_before_clone() {
    let placement = placement(true);
    let record = NestedRecord {
        id: "f3d:native:coil-placement#0",
        value: &placement,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_scopes",
        || super::COIL_PLACEMENT_CLONE_COUNT.with(|count| count.set(0)),
        || super::COIL_PLACEMENT_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn coil_scope_borrowed_wire_matches_owned_wire_bytes() {
    for scope in [
        super::DesignCoilScope::default(),
        super::DesignCoilScope {
            operation: Some(crate::records::identity::RecordedValue {
                value: super::DesignExtrudeOperation::NewBody,
                offset: 10,
            }),
            extent: Some(crate::records::identity::MaybeRecordedValue::Located(
                crate::records::identity::RecordedValue {
                    value: super::DesignCoilExtent::Spiral,
                    offset: 11,
                },
            )),
            placement: Some(placement(true)),
            ..Default::default()
        },
    ] {
        let owned = super::DesignCoilScopeWire::from(scope.clone());
        assert_eq!(
            serde_json::to_vec(&scope).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn coil_scope_native_retained_limit_refuses_before_clone() {
    let scope = super::DesignCoilScope {
        placement: Some(placement(true)),
        ..Default::default()
    };
    let record = NestedRecord {
        id: "f3d:native:coil-scope#0",
        value: &scope,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_scopes",
        || super::COIL_SCOPE_CLONE_COUNT.with(|count| count.set(0)),
        || super::COIL_SCOPE_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn coil_secondary_identity_wire_refuses_a_null_identity() {
    #[derive(serde::Deserialize)]
    struct Probe {
        #[serde(
            flatten,
            deserialize_with = "super::deserialize_coil_secondary_identity"
        )]
        secondary: Option<crate::records::identity::DesignSecondaryIdentity<u64>>,
    }
    for key in ["secondary_identity", "curve_secondary_identity"] {
        let mut wire = serde_json::json!({});
        wire[key] = serde_json::Value::Null;
        assert!(
            serde_json::from_value::<Probe>(wire.clone()).is_err(),
            "{wire}"
        );
    }
    let absent: Probe = serde_json::from_value(serde_json::json!({})).unwrap();
    assert!(absent.secondary.is_none());
}

#[test]
fn coil_recipe_design_wire_refuses_a_null_design_key() {
    #[derive(serde::Deserialize)]
    struct Probe {
        #[serde(flatten, deserialize_with = "super::deserialize_coil_recipe_design")]
        design: Option<crate::records::recipes::ConstructionRecipeDesign<String>>,
    }
    for key in ["design_id", "design_selector"] {
        let mut wire = serde_json::json!({});
        wire[key] = serde_json::Value::Null;
        assert!(
            serde_json::from_value::<Probe>(wire.clone()).is_err(),
            "{wire}"
        );
    }
    let absent: Probe = serde_json::from_value(serde_json::json!({})).unwrap();
    assert!(absent.design.is_none());
}

/// Every flattened coil reader names the key it refuses.
///
/// Serde buffers a flattened field's keys into its own content map before the
/// reader runs, so no path a surrounding deserializer tracks reaches inside
/// one. The key reaches the refusal because the reading declaration states it.
#[test]
fn a_flattened_coil_reader_names_the_null_key_it_refuses() {
    use cadmpeg_test_support::refusal::refusal;

    #[derive(serde::Deserialize)]
    struct Secondary {
        #[serde(
            flatten,
            deserialize_with = "super::deserialize_coil_secondary_identity"
        )]
        #[allow(dead_code)]
        value: Option<crate::records::identity::DesignSecondaryIdentity<u64>>,
    }
    #[derive(serde::Deserialize)]
    struct Design {
        #[serde(flatten, deserialize_with = "super::deserialize_coil_recipe_design")]
        #[allow(dead_code)]
        value: Option<crate::records::recipes::ConstructionRecipeDesign<String>>,
    }

    let refusals = [
        (
            "secondary_identity",
            refusal::<Secondary>("secondary_identity"),
        ),
        (
            "curve_secondary_identity",
            refusal::<Secondary>("curve_secondary_identity"),
        ),
        ("design_id", refusal::<Design>("design_id")),
        ("design_selector", refusal::<Design>("design_selector")),
    ];
    for (key, message) in refusals {
        assert!(
            message.starts_with(&format!("{key}: ")),
            "the refusal of a null {key} states {message}"
        );
        assert!(
            message.contains("it does not state null"),
            "the refusal of a null {key} states {message}"
        );
    }
}
