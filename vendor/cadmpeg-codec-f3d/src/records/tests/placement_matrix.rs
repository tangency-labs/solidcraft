use crate::records::sketch_placement::SketchPlacementMatrix;
use serde::Deserialize;

#[test]
fn placement_matrix_admission_preserves_translation_reflection_and_signed_zero() {
    for x_axis in [-1.0, 1.0] {
        let mut rows = SketchPlacementMatrix::IDENTITY.rows();
        rows[0][0] = x_axis;
        rows[0][3] = f64::MAX;
        rows[1][3] = -3.0;
        rows[3][0] = -0.0;
        let wire = serde_json::to_value(rows).unwrap();
        let matrix = SketchPlacementMatrix::try_from(rows).unwrap();
        assert_eq!(matrix.rows()[3][0].to_bits(), (-0.0_f64).to_bits());
        assert_eq!(serde_json::to_value(matrix).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<SketchPlacementMatrix>(wire).unwrap(),
            matrix
        );
    }
}

#[test]
fn placement_matrix_rejects_nonfinite_projective_scaled_and_sheared_forms() {
    for (row, column, value) in [
        (0, 3, f64::NAN),
        (0, 3, f64::INFINITY),
        (3, 0, 1.0),
        (3, 3, 0.0),
        (0, 0, 2.0),
        (0, 1, 0.5),
    ] {
        let mut rows = SketchPlacementMatrix::IDENTITY.rows();
        rows[row][column] = value;
        assert!(SketchPlacementMatrix::try_from(rows).is_err());
        assert!(serde_json::from_value::<SketchPlacementMatrix>(
            serde_json::to_value(rows).unwrap()
        )
        .is_err());
    }
}

fn rejects_transform<T: for<'de> Deserialize<'de>>(mut wire: serde_json::Value, field: &str) {
    wire[field] = serde_json::to_value(SketchPlacementMatrix::IDENTITY).unwrap();
    let admitted = serde_json::from_value::<T>(wire.clone());
    assert!(admitted.is_ok(), "valid matrix route: {:?}", admitted.err());
    let mut rows = SketchPlacementMatrix::IDENTITY.rows();
    rows[1][1] = 2.0;
    wire[field] = serde_json::to_value(rows).unwrap();
    assert!(serde_json::from_value::<T>(wire).is_err());
}

#[test]
fn placement_record_serde_routes_reject_unchecked_matrices() {
    use crate::records::{
        feature::{
            assembly::{DesignAssemblyOperandFrame, DesignAssemblySolvedFrame},
            assembly_features::DesignCopyPasteComponentOperation,
            coil::DesignCoilTransform,
            direct_face::DesignMoveOperation,
            primitives::{DesignSpherePrimitive, DesignTorusPrimitive},
            scope::{DesignJointOriginTransform, DesignWorkPlaneTransform},
        },
        topology::{
            construction::DesignConstructionOperandDualTransform,
            construction::DesignConstructionOperandTransform,
        },
    };
    rejects_transform::<DesignAssemblyOperandFrame>(
        serde_json::json!({
            "reference_record_index":1, "reference_offset":20, "transform_offset":40,
        }),
        "transform",
    );
    rejects_transform::<DesignAssemblySolvedFrame>(
        serde_json::json!({
            "reference_record_index":1, "reference_offset":20, "record_byte_offset":30,
            "class_tag":"123", "transform_offset":40,
        }),
        "transform",
    );
    rejects_transform::<DesignCoilTransform>(
        serde_json::json!({"transform_offset":40}),
        "transform",
    );
    rejects_transform::<DesignWorkPlaneTransform>(
        serde_json::json!({"work_plane_transform_offset":40}),
        "work_plane_transform",
    );
    rejects_transform::<DesignJointOriginTransform>(
        serde_json::json!({"joint_origin_transform_offset":40}),
        "joint_origin_transform",
    );
    rejects_transform::<DesignSpherePrimitive>(
        serde_json::json!({
            "transform_offset":40, "diameter":1.0, "diameter_record_index":1,
            "diameter_offset":200, "operation":"join", "operation_offset":220,
        }),
        "transform",
    );
    rejects_transform::<DesignTorusPrimitive>(
        serde_json::json!({
            "transform_offset":40, "major_diameter":2.0, "major_diameter_record_index":1,
            "major_diameter_offset":200, "minor_diameter":1.0, "minor_diameter_record_index":2,
            "minor_diameter_offset":208, "operation":"join", "operation_offset":220,
        }),
        "transform",
    );
    rejects_transform::<DesignMoveOperation>(
        serde_json::json!({
            "transform_offset":40, "transform_record_index":1, "form":1, "form_offset":20,
        }),
        "transform",
    );
    let single = serde_json::json!({
        "record_index":1, "byte_offset":0, "class_tag":"123", "transform_offset":22,
        "following_record_index":2, "following_byte_offset":152, "following_class_tag":"124",
    });
    rejects_transform::<DesignConstructionOperandTransform>(single, "transform");
    for field in ["first_transform", "second_transform"] {
        rejects_transform::<DesignConstructionOperandDualTransform>(
            serde_json::json!({
                "record_index":1, "byte_offset":0, "class_tag":"123",
                "first_transform":SketchPlacementMatrix::IDENTITY.rows(), "first_transform_offset":40,
                "second_transform":SketchPlacementMatrix::IDENTITY.rows(), "second_transform_offset":168,
            }),
            field,
        );
    }
    for field in ["source_transform", "copied_transform"] {
        rejects_transform::<DesignCopyPasteComponentOperation>(
            serde_json::json!({
                "relation_record_index":1,"source_occurrence_record_index":2,"copied_occurrence_record_index":3,
                "component_guid":"00000000-0000-0000-0000-000000000001",
                "source_occurrence_guid":"00000000-0000-0000-0000-000000000002",
                "copied_occurrence_guid":"00000000-0000-0000-0000-000000000003",
                "source_transform":SketchPlacementMatrix::IDENTITY.rows(),"source_transform_offset":40,
                "copied_transform":SketchPlacementMatrix::IDENTITY.rows(),"copied_transform_offset":168,
            }),
            field,
        );
    }
}
