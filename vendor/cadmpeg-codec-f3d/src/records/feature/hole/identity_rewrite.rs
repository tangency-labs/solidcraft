// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignHoleConstruction, DesignHoleExtent, DesignHoleFaceSelection, DesignHoleLocation,
    DesignHoleScope, DesignHoleTangentPoint,
};

rewrite_native_record!(DesignHoleConstruction, []; {point_record_index, point_record_byte_offset, position, position_offset, direction, direction_offset, point_parameters, point_parameter_offsets, reference_type, reference_type_offset, tangent_point_data, input_records, face_selection, locations});
rewrite_native_record!(DesignHoleLocation, []; {record_index, position, position_offset, reference_type, vertex_input});
rewrite_native_record!(DesignHoleFaceSelection, []; {record_index, byte_offset, class_tag, asset_id, asset_id_offset, context_id, context_id_offset, identity_record_index, identity_record_offset, primary_identity, primary_identity_offset, secondary, historical_face_candidates, next_record_index, next_byte_offset});
rewrite_native_record!(DesignHoleTangentPoint, []; {prefix, data});

rewrite_native_record!(DesignHoleScope, []; {point, extent});
rewrite_native_record!(DesignHoleExtent, []; {leading_word, extent_control, saved_non_distance_depth, direction_control});
