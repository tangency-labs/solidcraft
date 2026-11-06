// SPDX-License-Identifier: Apache-2.0
//! Variable fillet midpoints and the flattened historical binding.

use cadmpeg_test_support::refusal::{refusal, states_the_key};

#[test]
fn fillet_radius_law_borrowed_wire_matches_owned_wire_bytes() {
    let cases = [
        super::DesignFilletRadiusLaw::Constant {
            radius_parameter_record_index: 3,
        },
        super::DesignFilletRadiusLaw::Chordal {
            chord_length_parameter_record_index: 4,
        },
        super::DesignFilletRadiusLaw::Asymmetric {
            offset_one_parameter_record_index: 5,
            offset_two_parameter_record_index: 6,
        },
        super::DesignFilletRadiusLaw::Variable {
            start_radius_parameter_record_index: 7,
            end_radius_parameter_record_index: 8,
            middle: vec![super::DesignFilletMidpoint {
                radius_parameter_record_index: 9,
                parameter_record_index: 10,
            }],
        },
    ];
    for law in cases {
        let owned = super::DesignFilletRadiusLawWire::from(law.clone());
        assert_eq!(
            serde_json::to_vec(&law).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn fillet_radius_law_native_retained_limit_refuses_before_clone() {
    let record = super::DesignFilletRadiusGroup {
        id: "f3d:native:fillet-radius-group#0".into(),
        scope_record_index: 1,
        group_ordinal: 0,
        group_record_index: 2,
        edge_operand_record_indices: vec![3],
        law: super::DesignFilletRadiusLaw::Variable {
            start_radius_parameter_record_index: 4,
            end_radius_parameter_record_index: 5,
            middle: vec![super::DesignFilletMidpoint {
                radius_parameter_record_index: 6,
                parameter_record_index: 7,
            }],
        },
        tangency_weight_parameter_record_index: None,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_fillet_radius_groups",
        || super::FILLET_RADIUS_LAW_CLONE_COUNT.with(|count| count.set(0)),
        || super::FILLET_RADIUS_LAW_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn variable_fillet_midpoints_preserve_wire_and_reject_unpaired_records() {
    let wire = r#"{"kind":"variable","start_radius_parameter_record_index":51,"end_radius_parameter_record_index":61,"middle_radius_parameter_record_indices":[71],"middle_parameter_record_indices":[81]}"#;
    let law: super::DesignFilletRadiusLaw = serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_string(&law).unwrap(), wire);
    for invalid in [wire.replace("[71]", "[]"), wire.replace("[81]", "[]")] {
        let error = serde_json::from_str::<super::DesignFilletRadiusLaw>(&invalid)
            .unwrap_err()
            .to_string();
        assert!(error.contains("middle_radius_parameter_record_indices"));
        assert!(error.contains("middle_parameter_record_indices"));
    }
}

/// The flattened historical-binding reader names the key it refuses.
///
/// Serde buffers a flattened field's keys into its own content map before the
/// reader runs, so no path a surrounding deserializer tracks reaches inside
/// one. The key reaches the refusal because the reading declaration states it.
#[test]
fn the_flattened_historical_binding_names_the_null_key_it_refuses() {
    #[derive(serde::Deserialize)]
    struct Probe {
        #[serde(flatten, deserialize_with = "super::deserialize_historical_binding")]
        binding: Option<super::HistoricalBinding>,
    }
    for key in ["historical_entity_kind", "historical_entity_ref"] {
        states_the_key(key, &refusal::<Probe>(key));
    }
    let absent: Probe = serde_json::from_value(serde_json::json!({})).unwrap();
    assert!(absent.binding.is_none());
}
