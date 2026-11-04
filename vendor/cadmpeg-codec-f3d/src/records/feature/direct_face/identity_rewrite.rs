// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignDraftFlags, DesignDraftOperation, DesignMoveForm, DesignMoveOperation,
    DesignOffsetFacesOperation, DesignShellOperation, DesignShellOutsideWall, DesignShellWallSlots,
    DesignThickenOperation,
};

rewrite_native_record!(DesignDraftOperation, []; {angle, angle_record_index, angle_offset, opposite_angle_record_index, opposite_angle_offset, flags});
rewrite_native_record!(DesignDraftFlags, []; {flipped, tangent_chain});
rewrite_native_scalar!(DesignMoveForm);
rewrite_native_record!(DesignMoveOperation, []; {transform, transform_offset, transform_record_index, form, form_offset});
rewrite_native_record!(DesignOffsetFacesOperation, []; {distance, distance_record_index, distance_offset});
rewrite_native_record!(DesignShellOperation, []; {thickness, thickness_record_index, thickness_offset, outward, outward_offset, wall_slots});
rewrite_native_record!(DesignShellOutsideWall, []; {thickness, thickness_record_index, thickness_offset});
rewrite_native_record!(DesignShellWallSlots, []; {tangent_chain, rounded_corners, outside});
rewrite_native_record!(DesignThickenOperation, []; {signed_thickness, thickness_record_index, thickness_offset});
