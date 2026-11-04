// SPDX-License-Identifier: Apache-2.0
//! Solid primitives: box, cylinder, sphere and torus.

use super::extrude::DesignExtrudeOperation;
use super::scope::DesignScopePayload;
use crate::records::identity::Located;
use crate::records::sketch_placement::SketchPlacementMatrix;
use cadmpeg_ir::scalar::{FiniteReal, PositiveReal};
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(deserialize_transform, SketchPlacementMatrix, "transform");
cadmpeg_core::named_optional_field!(deserialize_transform_offset, u64, "transform_offset");
/// Exact construction data of a solid primitive scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "primitive")]
pub(crate) enum DesignSolidPrimitive {
    /// Axis-aligned box defined by five owned dimensions and offsets.
    Box(DesignBoxPrimitive),
    /// Circular cylinder defined by height and diameter owners.
    Cylinder(DesignCylinderPrimitive),
    /// Sphere defined by a placement frame and diameter.
    Sphere(DesignSpherePrimitive),
    /// Torus defined by a placement frame and two diameters.
    Torus(DesignTorusPrimitive),
}

/// Exact `Box` primitive construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignBoxPrimitive {
    /// Length along the source x-axis in source centimetres.
    pub(crate) length: PositiveReal,
    /// Referenced length owner.
    pub(crate) length_record_index: u32,
    /// Byte offset of the evaluated length.
    pub(crate) length_offset: u64,
    /// Width along the source y-axis in source centimetres.
    pub(crate) width: PositiveReal,
    /// Referenced width owner.
    pub(crate) width_record_index: u32,
    /// Byte offset of the evaluated width.
    pub(crate) width_offset: u64,
    /// Height along the source z-axis in source centimetres.
    pub(crate) height: PositiveReal,
    /// Referenced height owner.
    pub(crate) height_record_index: u32,
    /// Byte offset of the evaluated height.
    pub(crate) height_offset: u64,
    /// Translation along the source x-axis in source centimetres.
    pub(crate) offset_x: FiniteReal,
    /// Referenced x-offset owner.
    pub(crate) offset_x_record_index: u32,
    /// Byte offset of the evaluated x offset.
    pub(crate) offset_x_offset: u64,
    /// Translation along the source y-axis in source centimetres.
    pub(crate) offset_y: FiniteReal,
    /// Referenced y-offset owner.
    pub(crate) offset_y_record_index: u32,
    /// Byte offset of the evaluated y offset.
    pub(crate) offset_y_offset: u64,
    /// Local-to-model frame stored by the typed-reference form, in source
    /// centimetres. The named form stores no frame.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform"
    )]
    pub(crate) transform: Option<SketchPlacementMatrix>,
    /// Byte offset of the typed-reference form frame.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform_offset"
    )]
    pub(crate) transform_offset: Option<u64>,
    /// Result Boolean operation.
    pub(crate) operation: DesignExtrudeOperation,
    /// Byte offset of the operation enum.
    pub(crate) operation_offset: u64,
}

/// Exact `Cylinder` primitive construction.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignCylinderPrimitiveWire",
    into = "DesignCylinderPrimitiveWire"
)]
pub(crate) struct DesignCylinderPrimitive {
    /// Axial height in source centimetres.
    pub(crate) height: PositiveReal,
    /// Referenced height owner.
    pub(crate) height_record_index: u32,
    /// Byte offset of the evaluated height.
    pub(crate) height_offset: u64,
    /// Circular diameter in source centimetres.
    pub(crate) diameter: PositiveReal,
    /// Referenced diameter owner.
    pub(crate) diameter_record_index: u32,
    /// Byte offset of the evaluated diameter.
    pub(crate) diameter_offset: u64,
    /// Local-to-model cylinder placement frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) transform: Option<Located<SketchPlacementMatrix>>,
    /// Result Boolean operation.
    pub(crate) operation: DesignExtrudeOperation,
    /// Byte offset of the operation enum.
    pub(crate) operation_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct DesignCylinderPrimitiveWire {
    /// Axial height in source centimetres.
    height: PositiveReal,
    /// Referenced height owner.
    height_record_index: u32,
    /// Byte offset of the evaluated height.
    height_offset: u64,
    /// Circular diameter in source centimetres.
    diameter: PositiveReal,
    /// Referenced diameter owner.
    diameter_record_index: u32,
    /// Byte offset of the evaluated diameter.
    diameter_offset: u64,
    /// Local-to-model cylinder placement frame.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform"
    )]
    transform: Option<SketchPlacementMatrix>,
    /// Byte offset of the cylinder placement frame.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform_offset"
    )]
    transform_offset: Option<u64>,
    /// Result Boolean operation.
    operation: DesignExtrudeOperation,
    /// Byte offset of the operation enum.
    operation_offset: u64,
}

impl From<DesignCylinderPrimitive> for DesignCylinderPrimitiveWire {
    fn from(value: DesignCylinderPrimitive) -> Self {
        Self {
            height: value.height,
            height_record_index: value.height_record_index,
            height_offset: value.height_offset,
            diameter: value.diameter,
            diameter_record_index: value.diameter_record_index,
            diameter_offset: value.diameter_offset,
            transform: value.transform.map(|located| located.value),
            transform_offset: value.transform.map(|located| located.offset),
            operation: value.operation,
            operation_offset: value.operation_offset,
        }
    }
}

impl TryFrom<DesignCylinderPrimitiveWire> for DesignCylinderPrimitive {
    type Error = String;
    fn try_from(value: DesignCylinderPrimitiveWire) -> Result<Self, Self::Error> {
        Ok(Self {
            height: value.height,
            height_record_index: value.height_record_index,
            height_offset: value.height_offset,
            diameter: value.diameter,
            diameter_record_index: value.diameter_record_index,
            diameter_offset: value.diameter_offset,
            transform: Located::from_wire(value.transform, value.transform_offset, "transform")?,
            operation: value.operation,
            operation_offset: value.operation_offset,
        })
    }
}

/// Exact `Sphere` primitive construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignSpherePrimitive {
    /// Row-major local-to-model placement frame.
    pub(crate) transform: SketchPlacementMatrix,
    /// Byte offset of the placement matrix.
    pub(crate) transform_offset: u64,
    /// Sphere diameter in source centimetres.
    pub(crate) diameter: PositiveReal,
    /// Referenced diameter record.
    pub(crate) diameter_record_index: u32,
    /// Byte offset of the diameter scalar.
    pub(crate) diameter_offset: u64,
    /// Result Boolean operation.
    pub(crate) operation: DesignExtrudeOperation,
    /// Byte offset of the operation enum.
    pub(crate) operation_offset: u64,
}

/// Exact `Torus` primitive construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignTorusPrimitive {
    /// Row-major local-to-model placement frame.
    pub(crate) transform: SketchPlacementMatrix,
    /// Byte offset of the placement matrix.
    pub(crate) transform_offset: u64,
    /// Major diameter in source centimetres.
    pub(crate) major_diameter: PositiveReal,
    /// Referenced major-diameter record.
    pub(crate) major_diameter_record_index: u32,
    /// Byte offset of the major-diameter scalar.
    pub(crate) major_diameter_offset: u64,
    /// Tube diameter in source centimetres.
    pub(crate) minor_diameter: PositiveReal,
    /// Referenced minor-diameter record.
    pub(crate) minor_diameter_record_index: u32,
    /// Byte offset of the minor-diameter scalar.
    pub(crate) minor_diameter_offset: u64,
    /// Result Boolean operation.
    pub(crate) operation: DesignExtrudeOperation,
    /// Byte offset of the operation enum.
    pub(crate) operation_offset: u64,
}

impl From<DesignSolidPrimitive> for DesignScopePayload {
    fn from(value: DesignSolidPrimitive) -> Self {
        match value {
            DesignSolidPrimitive::Box(value) => Self::BoxPrimitive(Some(value)),
            DesignSolidPrimitive::Cylinder(value) => Self::CylinderPrimitive(Some(value)),
            DesignSolidPrimitive::Sphere(value) => Self::SpherePrimitive(Some(value)),
            DesignSolidPrimitive::Torus(value) => Self::TorusPrimitive(Some(value)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DesignBoxPrimitive, DesignCylinderPrimitive, DesignSpherePrimitive, DesignTorusPrimitive,
    };

    #[test]
    fn native_box_refuses_nonpositive_dimension() {
        let mut wire = serde_json::json!({
            "length": 1.0, "length_record_index": 1, "length_offset": 2,
            "width": 2.0, "width_record_index": 3, "width_offset": 4,
            "height": 3.0, "height_record_index": 5, "height_offset": 6,
            "offset_x": 0.0, "offset_x_record_index": 7, "offset_x_offset": 8,
            "offset_y": 0.0, "offset_y_record_index": 9, "offset_y_offset": 10,
            "operation": "join", "operation_offset": 11
        });
        assert!(serde_json::from_value::<DesignBoxPrimitive>(wire.clone()).is_ok());
        wire["length"] = serde_json::json!(0.0);
        assert!(serde_json::from_value::<DesignBoxPrimitive>(wire).is_err());
    }

    #[test]
    fn native_cylinder_refuses_nonpositive_dimension() {
        let mut wire = serde_json::json!({
            "height": 1.0, "height_record_index": 1, "height_offset": 2,
            "diameter": 2.0, "diameter_record_index": 3, "diameter_offset": 4,
            "operation": "join", "operation_offset": 5
        });
        assert!(serde_json::from_value::<DesignCylinderPrimitive>(wire.clone()).is_ok());
        wire["diameter"] = serde_json::json!(-1.0);
        assert!(serde_json::from_value::<DesignCylinderPrimitive>(wire).is_err());
    }

    #[test]
    fn native_sphere_refuses_nonpositive_diameter() {
        let mut wire = serde_json::json!({
            "transform": [[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]],
            "transform_offset": 1,
            "diameter": 2.0, "diameter_record_index": 3, "diameter_offset": 4,
            "operation": "join", "operation_offset": 5
        });
        assert!(serde_json::from_value::<DesignSpherePrimitive>(wire.clone()).is_ok());
        wire["diameter"] = serde_json::json!(0.0);
        assert!(serde_json::from_value::<DesignSpherePrimitive>(wire).is_err());
    }

    #[test]
    fn native_torus_refuses_nonpositive_diameter() {
        let mut wire = serde_json::json!({
            "transform": [[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]],
            "transform_offset": 1,
            "major_diameter": 2.0, "major_diameter_record_index": 3, "major_diameter_offset": 4,
            "minor_diameter": 1.0, "minor_diameter_record_index": 5, "minor_diameter_offset": 6,
            "operation": "join", "operation_offset": 7
        });
        assert!(serde_json::from_value::<DesignTorusPrimitive>(wire.clone()).is_ok());
        wire["minor_diameter"] = serde_json::json!(-1.0);
        assert!(serde_json::from_value::<DesignTorusPrimitive>(wire).is_err());
    }
}

mod identity_rewrite;
