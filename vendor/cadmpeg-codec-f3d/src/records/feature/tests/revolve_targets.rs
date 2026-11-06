// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::path_features::DesignRevolveConstruction;

const PREFIX: &str = r#"{"operation":"join","operation_offset":25"#;
const OWNER: &str = r#""angle_record_index":95,"angle_offset":2040"#;
const ONE_SIDED: &str = r#""targets":{"side_one":{"value":85,"offset":74}}"#;

fn decode(wire: &str) -> Result<DesignRevolveConstruction, String> {
    serde_json::from_str(wire).map_err(|error| error.to_string())
}

#[test]
fn to_object_targets_preserve_their_wire() {
    for targets in [
        ONE_SIDED,
        r#""targets":{"side_one":{"value":87,"offset":84},"side_two":{"value":85,"offset":62}}"#,
    ] {
        let wire = format!(r#"{PREFIX},{targets},{OWNER},"solid":true}}"#);
        let value = decode(&wire).expect("to-object revolve");
        assert_eq!(serde_json::to_string(&value).expect("revolve wire"), wire);
    }
}

#[test]
fn a_construction_states_exactly_one_travel() {
    for wire in [
        format!("{PREFIX},{OWNER}}}"),
        format!(r#"{PREFIX},"angle":1.5,{ONE_SIDED},{OWNER}}}"#),
    ] {
        let error = decode(&wire).expect_err("travel");
        assert!(
            error.contains("exactly one of angle and targets"),
            "{error}"
        );
    }
}

#[test]
fn a_to_object_construction_refuses_angle_only_fields() {
    for extra in [
        r#""opposite_angle_record_index":4,"opposite_angle_offset":80"#,
        r#""second_side":{"symmetric":false,"angle":1.0,"angle_record_index":5,"angle_offset":60}"#,
        r#""reversed":true"#,
    ] {
        let error = decode(&format!("{PREFIX},{ONE_SIDED},{OWNER},{extra}}}"))
            .expect_err("angle-only field");
        assert!(error.contains("a to-object Revolve has no"), "{error}");
    }
}
