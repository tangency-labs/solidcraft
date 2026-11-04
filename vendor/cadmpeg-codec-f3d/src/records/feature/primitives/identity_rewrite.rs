// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignBoxPrimitive, DesignCylinderPrimitive, DesignSpherePrimitive, DesignTorusPrimitive,
};

rewrite_native_record!(DesignBoxPrimitive, []; {length, length_record_index, length_offset, width, width_record_index, width_offset, height, height_record_index, height_offset, offset_x, offset_x_record_index, offset_x_offset, offset_y, offset_y_record_index, offset_y_offset, transform, transform_offset, operation, operation_offset});
rewrite_native_scalar!(DesignCylinderPrimitive);
rewrite_native_record!(DesignSpherePrimitive, []; {transform, transform_offset, diameter, diameter_record_index, diameter_offset, operation, operation_offset});
rewrite_native_record!(DesignTorusPrimitive, []; {transform, transform_offset, major_diameter, major_diameter_record_index, major_diameter_offset, minor_diameter, minor_diameter_record_index, minor_diameter_offset, operation, operation_offset});
