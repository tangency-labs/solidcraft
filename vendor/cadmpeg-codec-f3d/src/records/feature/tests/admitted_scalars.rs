// SPDX-License-Identifier: Apache-2.0
//! Records that hold an admitted positive scalar refuse a stored value outside it.

use crate::records::feature::{
    body_ops::DesignScaleOperation,
    direct_face::DesignShellOperation,
    fixed_parameters::DesignFixedChamferDistance,
    mirror::DesignMirrorConstruction,
    patterns::{DesignCircularPatternAxis, DesignCircularPatternConstruction},
    surface_ops::{
        DesignSurfaceExtendMethod, DesignSurfaceExtendOperation, DesignSurfaceOffsetSupport,
    },
};
use crate::records::topology::edge_identity::DesignEdgeTreatmentRadiusCandidate;
use cadmpeg_ir::scalar::{PositiveAngle, PositiveReal};
use serde::de::DeserializeOwned;
use serde::Serialize;

fn positive(value: f64) -> PositiveReal {
    PositiveReal::new(value).expect("positive fixture value")
}

/// The record round-trips, and replacing `field` with zero is refused with an
/// error naming `refusal`.
fn refuses_zero<T>(record: &T, field: &str, refusal: &str)
where
    T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let wire = serde_json::to_value(record).expect("serialize record");
    assert_eq!(
        &serde_json::from_value::<T>(wire.clone()).expect("round trip"),
        record
    );
    let mut invalid = wire;
    invalid[field] = serde_json::json!(0.0);
    let error = serde_json::from_value::<T>(invalid)
        .expect_err("zero is refused")
        .to_string();
    assert!(error.contains(refusal), "{error}");
}

#[test]
fn scale_operation_refuses_a_nonpositive_uniform_factor() {
    refuses_zero(
        &DesignScaleOperation {
            body_group_record_index: 102,
            center_record_index: 105,
            center_position: None,
            uniform_factor: positive(2.5),
            uniform_factor_offset: 21,
        },
        "uniform_factor",
        "uniform_factor must be positive and finite",
    );
}

#[test]
fn mirror_construction_refuses_a_nonpositive_stitch_tolerance() {
    let wire = r#"{"count":2,"count_record_index":11,"count_offset":0,"stitch_tolerance":0.001,"stitch_tolerance_offset":51,"stitch_tolerance_scope":{"marker":89,"marker_offset":47,"repeated_marker_offset":59,"first_reference":12,"first_reference_offset":63,"second_reference":11,"second_reference_offset":76},"seed_group_record_index":20,"plane_group_record_index":30}"#;
    let construction: DesignMirrorConstruction =
        serde_json::from_str(wire).expect("inline mirror tolerance");
    refuses_zero(
        &construction,
        "stitch_tolerance",
        "stitch_tolerance must be positive and finite",
    );
}

#[test]
fn shell_operation_refuses_a_nonpositive_thickness() {
    refuses_zero(
        &DesignShellOperation {
            thickness: positive(0.25),
            thickness_record_index: 7,
            thickness_offset: 40,
            outward: true,
            outward_offset: 60,
            wall_slots: None,
        },
        "thickness",
        "PositiveReal",
    );
}

#[test]
fn chamfer_distance_refuses_a_nonpositive_value() {
    refuses_zero(
        &DesignFixedChamferDistance {
            value: positive(0.04),
            record_index: 9,
            value_offset: 40,
        },
        "value",
        "PositiveReal",
    );
}

#[test]
fn surface_extend_operation_refuses_a_nonpositive_tolerance() {
    refuses_zero(
        &DesignSurfaceExtendOperation {
            distance: crate::test_support::real(1.0),
            distance_offset: 10,
            distance_record_index: 3,
            method: DesignSurfaceExtendMethod::Tangent,
            method_offset: 20,
            boundary_record_index: 4,
            boundary_reference_record_index: 5,
            boundary_reference_offset: 24,
            edge_record_indices: vec![6],
            tolerance: positive(1.0e-6),
            tolerance_offset: 57,
        },
        "tolerance",
        "PositiveReal",
    );
}

#[test]
fn surface_offset_boundary_carrier_refuses_a_nonpositive_tolerance() {
    let support = DesignSurfaceOffsetSupport::BoundaryCarrier {
        boundary_record_index: 4,
        boundary_reference_record_index: 5,
        boundary_reference_offset: 24,
        edge_record_indices: vec![6],
        tolerance: positive(1.0e-6),
        tolerance_offset: 57,
    };
    let wire = serde_json::to_value(&support).expect("serialize support");
    assert_eq!(
        serde_json::from_value::<DesignSurfaceOffsetSupport>(wire.clone()).expect("round trip"),
        support
    );
    let mut invalid = wire;
    invalid["value"]["tolerance"] = serde_json::json!(0.0);
    let error = serde_json::from_value::<DesignSurfaceOffsetSupport>(invalid)
        .expect_err("zero is refused")
        .to_string();
    assert!(error.contains("PositiveReal"), "{error}");
}

#[test]
fn circular_pattern_construction_refuses_a_nonpositive_angle() {
    refuses_zero(
        &DesignCircularPatternConstruction {
            count: std::num::NonZeroU32::new(25).unwrap(),
            count_record_index: 11,
            count_offset: 40,
            angle: PositiveAngle::new(std::f64::consts::TAU).expect("positive angle"),
            angle_record_index: 12,
            angle_offset: 80,
            axis: DesignCircularPatternAxis::Inline {
                origin: crate::test_support::reals([1.0, 2.0, 3.0]),
                origin_offset: 100,
                direction: cadmpeg_ir::units::UnitVector3::normalized(
                    cadmpeg_ir::math::Vector3::new(-1.0, 0.0, 0.0),
                )
                .expect("unit axis"),
                direction_offset: 124,
            },
            axis_record_index: 13,
            selection_record_index: 14,
        },
        "angle",
        "PositiveAngle",
    );
}

#[test]
fn edge_treatment_radius_candidate_refuses_a_nonpositive_radius() {
    refuses_zero(
        &DesignEdgeTreatmentRadiusCandidate {
            edge_slot: 4,
            radius: positive(3.0),
        },
        "radius",
        "PositiveReal",
    );
}

#[test]
fn circular_pattern_construction_refuses_zero_count() {
    let wire = serde_json::json!({
        "count": 0, "count_record_index": 1, "count_offset": 2,
        "angle": 1.0, "angle_record_index": 3, "angle_offset": 4,
        "axis": {"kind":"inline", "origin":[0.0,0.0,0.0], "origin_offset":5,
            "direction":[0.0,0.0,1.0], "direction_offset":6},
        "axis_record_index":7, "selection_record_index":8
    });
    let error = serde_json::from_value::<DesignCircularPatternConstruction>(wire).unwrap_err();
    assert!(error.to_string().contains("nonzero"), "{error}");
}

#[test]
fn mirror_native_plane_refuses_zero_and_nonunit_normals() {
    let wire = r#"{"count":2,"count_record_index":11,"count_offset":0,"stitch_tolerance":0.001,"stitch_tolerance_offset":51,"stitch_tolerance_record_index":12,"seed_group_record_index":20,"plane_group_record_index":30,"plane_origin":[0.0,0.0,0.0]}"#;
    let mut value: serde_json::Value = serde_json::from_str(wire).unwrap();
    for normal in [[0.0, 0.0, 0.0], [0.0, 0.0, 2.0]] {
        value["plane_normal"] = serde_json::json!(normal);
        let error = serde_json::from_value::<DesignMirrorConstruction>(value.clone()).unwrap_err();
        assert!(error
            .to_string()
            .contains("plane_normal must be a unit vector"));
    }
    value["plane_origin"] = serde_json::to_value(
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0))
            .unwrap(),
    )
    .unwrap();
    value["plane_normal"] = serde_json::to_value(cadmpeg_ir::features::FiniteVector3::from(
        cadmpeg_ir::units::UnitVector3::Z_AXIS,
    ))
    .unwrap();
    let plane: DesignMirrorConstruction = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(plane).unwrap(), value);
}
