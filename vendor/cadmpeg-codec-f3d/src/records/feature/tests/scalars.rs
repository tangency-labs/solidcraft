use crate::records::feature::{
    direct_face::DesignDraftOperation, path_features::DesignRevolveConstruction,
    sheet_metal::DesignBaseFlangeOperation, surface_ops::DesignSurfaceStitchOperation,
};
use cadmpeg_ir::scalar::{Angle, PositiveReal};
use serde::Deserialize;

#[test]
fn checked_scalars_reject_nonfinite_deserializer_values() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(Angle::new(value).is_none());
        assert!(PositiveReal::new(value).is_none());
        assert!(Angle::deserialize(
            serde::de::value::F64Deserializer::<serde::de::value::Error>::new(value)
        )
        .is_err());
        assert!(
            PositiveReal::deserialize(
                serde::de::value::F64Deserializer::<serde::de::value::Error>::new(value)
            )
            .is_err()
        );
    }
}

#[test]
fn positive_operation_fields_reject_zero_and_negative_values() {
    for value in [0.0, -0.0, -1.0] {
        assert!(
            serde_json::from_value::<DesignSurfaceStitchOperation>(serde_json::json!({
                "gap_tolerance": value, "gap_tolerance_offset": 40,
                "tolerance_record_index": 1, "settings_record_index": 2,
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<DesignBaseFlangeOperation>(serde_json::json!({
                "thickness": value, "thickness_offset": 123,
                "profile_group_record_index": 1, "profile_record_index": 2,
                "thickness_record_index": 3, "settings_record_index": 4,
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<DesignRevolveConstruction>(serde_json::json!({
                "operation": "join", "operation_offset": 12,
                "angle": value, "angle_record_index": 3, "angle_offset": 40,
            }))
            .unwrap_err()
            .to_string()
            .contains("angle")
        );
    }
}

#[test]
fn draft_angle_preserves_signed_and_zero_values() {
    for angle in [-1.0_f64, -0.0, 0.0, 1.0] {
        let wire = serde_json::json!({
            "angle": angle, "angle_record_index": 3, "angle_offset": 40,
            "opposite_angle_record_index": 4, "opposite_angle_offset": 80,
        });
        let operation: DesignDraftOperation = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(operation.angle.get().to_bits(), angle.to_bits());
        assert_eq!(serde_json::to_value(operation).unwrap(), wire);
    }
}
