// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::extrude::{
    DesignExtrudeDistanceTail, DesignExtrudeMeasurement, DesignExtrudePrologue,
};

fn symmetric(extent: &str, measurement: &str) -> String {
    format!(
        r#"{{"layout":"reference_aware","reference":null,"operation":"new_body","operation_offset":28229,"direction_face_extend_values":[3,1],"side_extent_discriminators":[1,0],"side_extent_discriminator_offsets":[28308,28321],"distance_tail":{{"target_option":1,"target_option_offset":28317,"measurement":"{measurement}","measurement_offset":28343}},"extent":"{extent}","direction_face_extend_offsets":[28233,28237],"direction_reversed":false,"direction_reversed_offset":28241,"solid_operation":true,"solid_operation_offset":28242,"start":"profile_plane","start_offset":28243}}"#
    )
}

#[test]
fn a_distance_tail_round_trips_with_its_prologue() {
    let prologue: DesignExtrudePrologue =
        serde_json::from_str(&symmetric("symmetric_distance", "each_side")).unwrap();
    assert_eq!(
        prologue.distance_tail(),
        Some(DesignExtrudeDistanceTail {
            target_option: 1,
            target_option_offset: 28317,
            measurement: DesignExtrudeMeasurement::EachSide,
            measurement_offset: 28343,
        })
    );
    let again: DesignExtrudePrologue =
        serde_json::from_str(&serde_json::to_string(&prologue).unwrap()).unwrap();
    assert_eq!(again, prologue);
}

#[test]
fn a_measurement_word_must_agree_with_the_extent() {
    for (extent, measurement) in [
        ("symmetric_distance", "sided"),
        ("two_sided_distance", "each_side"),
        ("two_sided_distance", "whole_travel"),
    ] {
        assert!(
            serde_json::from_str::<DesignExtrudePrologue>(&symmetric(extent, measurement)).is_err(),
            "{extent} {measurement}"
        );
    }
    assert!(serde_json::from_str::<DesignExtrudePrologue>(&symmetric(
        "two_sided_distance",
        "sided"
    ))
    .is_ok());
}
