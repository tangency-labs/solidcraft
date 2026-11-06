// SPDX-License-Identifier: Apache-2.0

use crate::design::feature_project::project_solid_primitive;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::primitives::DesignCylinderPrimitive;
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::identity::Located;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

fn cylinder(transform: Option<[[f64; 4]; 4]>) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#40",
        DesignFeatureKind::CylinderPrimitive,
        40,
    );
    if let DesignScopePayloadMut::CylinderPrimitive(slot) = scope.payload_mut() {
        *slot = Some(DesignCylinderPrimitive {
            height: cadmpeg_ir::scalar::PositiveReal::new(2.0).unwrap(),
            height_record_index: 41,
            height_offset: 0,
            diameter: cadmpeg_ir::scalar::PositiveReal::new(1.0).unwrap(),
            diameter_record_index: 42,
            diameter_offset: 0,
            transform: transform.map(|rows| Located {
                value: rows.try_into().unwrap(),
                offset: 72,
            }),
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: 22,
        });
    }
    scope
}

/// The shifted cylinder's frame is the primitive's placement, with its
/// translation in millimetres; a cylinder without a frame has none.
#[test]
fn shifted_cylinder_frame_places_the_primitive() {
    // Axis along model -y (frame z = -y), base centre at (1, 2, 3) cm.
    let frame = [
        [1.0, 0.0, 0.0, 1.0],
        [0.0, 0.0, -1.0, 2.0],
        [0.0, 1.0, 0.0, 3.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let Some(FeatureDefinition::Operation(FeatureOperation::Primitive { placement, .. })) =
        project_solid_primitive(&cylinder(Some(frame)))
    else {
        unreachable!("cylinder primitive");
    };
    let rows = placement.expect("shifted cylinder placement").rows();
    assert_eq!(rows[0], [1.0, 0.0, 0.0, 10.0]);
    assert_eq!(rows[1], [0.0, 0.0, -1.0, 20.0]);
    assert_eq!(rows[2], [0.0, 1.0, 0.0, 30.0]);

    assert!(matches!(
        project_solid_primitive(&cylinder(None)),
        Some(FeatureDefinition::Operation(FeatureOperation::Primitive {
            placement: None,
            ..
        }))
    ));
}

#[test]
fn stored_body_box_requires_dimensions_and_preserves_local_offsets() {
    use cadmpeg_ir::features::BooleanOp;
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#60",
        DesignFeatureKind::BoxPrimitive,
        60,
    );
    assert!(project_solid_primitive(&scope).is_none());
    if let DesignScopePayloadMut::BoxPrimitive(slot) = scope.payload_mut() {
        *slot = Some(
            serde_json::from_value(serde_json::json!({
                "length":2.0,"length_record_index":61,"length_offset":0,
                "width":3.0,"width_record_index":62,"width_offset":0,
                "height":4.0,"height_record_index":63,"height_offset":0,
                "offset_x":-1.0,"offset_x_record_index":64,"offset_x_offset":0,
                "offset_y":2.0,"offset_y_record_index":65,"offset_y_offset":0,
                "operation":"new_body","operation_offset":0
            }))
            .unwrap(),
        );
    }
    let Some(FeatureDefinition::Operation(FeatureOperation::Block {
        dimensions: Some(dimensions),
        placement: Some(placement),
        op,
    })) = project_solid_primitive(&scope)
    else {
        panic!("complete box");
    };
    assert_eq!(
        dimensions.map(cadmpeg_ir::scalar::PositiveLength::get),
        [20.0, 30.0, 40.0]
    );
    assert_eq!(
        placement.rows(),
        [
            [1.0, 0.0, 0.0, -10.0],
            [0.0, 1.0, 0.0, 20.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    );
    assert_eq!(op, BooleanOp::NewBody);
}

#[test]
fn typed_reference_box_projects_frame_and_agrees_with_corner_extents() {
    /// Tolerance for agreement between frame-derived and expected corners.
    const EPS_BOX_CORNER: f64 = 1.0e-9;
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#70",
        DesignFeatureKind::BoxPrimitive,
        70,
    );
    if let DesignScopePayloadMut::BoxPrimitive(slot) = scope.payload_mut() {
        *slot = Some(
            serde_json::from_value(serde_json::json!({
                "length":1.2,"length_record_index":71,"length_offset":0,
                "width":0.8,"width_record_index":72,"width_offset":0,
                "height":2.5,"height_record_index":73,"height_offset":0,
                "offset_x":0.0,"offset_x_record_index":74,"offset_x_offset":0,
                "offset_y":0.0,"offset_y_record_index":75,"offset_y_offset":0,
                "transform_offset":385,
                "operation":"new_body","operation_offset":22
            }))
            .unwrap(),
        );
        if let Some(primitive) = slot {
            primitive.transform = Some(
                [
                    [0.0, 1.0, 0.0, 3.0],
                    [0.0, 0.0, 1.0, -1.5],
                    [1.0, 0.0, 0.0, 0.25],
                    [0.0, 0.0, 0.0, 1.0],
                ]
                .try_into()
                .unwrap(),
            );
        }
    }
    let Some(FeatureDefinition::Operation(FeatureOperation::Block {
        dimensions: Some(dimensions),
        placement: Some(placement),
        ..
    })) = project_solid_primitive(&scope)
    else {
        panic!("typed-reference box");
    };
    let lengths = dimensions.map(cadmpeg_ir::scalar::PositiveLength::get);
    let rows = placement.rows();
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for corner in 0..8u32 {
        let local = [0, 1, 2].map(|axis| {
            if corner >> axis & 1 == 1 {
                lengths[axis]
            } else {
                0.0
            }
        });
        for axis in 0..3 {
            let value = rows[axis][0] * local[0]
                + rows[axis][1] * local[1]
                + rows[axis][2] * local[2]
                + rows[axis][3];
            low[axis] = low[axis].min(value);
            high[axis] = high[axis].max(value);
        }
    }
    // Expected extents in millimetres: the first length runs along the frame
    // x column (global z), the second along column 1 (global x) and the third
    // along column 2 (global y), from the origin (30, -15, 2.5).
    let expected_low = [30.0, -15.0, 2.5];
    let expected_high = [38.0, 10.0, 14.5];
    for axis in 0..3 {
        assert!((low[axis] - expected_low[axis]).abs() <= EPS_BOX_CORNER);
        assert!((high[axis] - expected_high[axis]).abs() <= EPS_BOX_CORNER);
    }
}

#[test]
fn esfera_primitiva_projects_like_sphere() {
    use crate::records::feature::scope::{
        DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
    };
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#80",
        DesignFeatureKind::EsferaPrimitiva,
        80,
    );
    if let DesignScopePayloadMut::EsferaPrimitiva(slot) = scope.payload_mut() {
        *slot = Some(
            serde_json::from_value(serde_json::json!({
                "transform": [
                    [1.0, 0.0, 0.0, 1.0],
                    [0.0, 1.0, 0.0, 2.0],
                    [0.0, 0.0, 1.0, 3.0],
                    [0.0, 0.0, 0.0, 1.0]
                ],
                "transform_offset": 1,
                "diameter": 2.0,
                "diameter_record_index": 81,
                "diameter_offset": 0,
                "operation": "new_body",
                "operation_offset": 0
            }))
            .unwrap(),
        );
    }
    let Some(FeatureDefinition::Operation(FeatureOperation::Sphere { center, radius, .. })) =
        project_solid_primitive(&scope)
    else {
        panic!("expected sphere from EsferaPrimitiva");
    };
    assert_eq!(
        center.get(),
        cadmpeg_ir::math::Point3::new(10.0, 20.0, 30.0)
    );
    assert!((radius.get() - 10.0).abs() < f64::EPSILON);
}
