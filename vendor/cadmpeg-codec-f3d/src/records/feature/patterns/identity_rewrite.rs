// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignAxis, DesignCircularPatternAxis, DesignCircularPatternConstruction,
    DesignPatternAxisWrapper, DesignPatternComponentInstance, DesignPatternInstance, DesignPlane,
    DesignRectangularPatternConstruction, DesignRectangularPatternInstances,
};

rewrite_native_scalar!(DesignAxis);
rewrite_native_enum!(DesignCircularPatternAxis, []; {
    Inline {origin, origin_offset, direction, direction_offset},
    HistoricalEdge {wrappers, persistent_identity, resolved},
});
rewrite_native_record!(DesignCircularPatternConstruction, []; {count, count_record_index, count_offset, angle, angle_record_index, angle_offset, axis, axis_record_index, selection_record_index});
rewrite_native_scalar!(DesignPatternAxisWrapper);
rewrite_native_record!(DesignPatternComponentInstance, []; {instance, occurrence_guid});
rewrite_native_scalar!(DesignPatternInstance);
rewrite_native_scalar!(DesignPlane);
rewrite_native_record!(DesignRectangularPatternConstruction, []; {u_count, v_count, u_extent, v_extent, owner_record_indices, value_offsets, instances});
rewrite_native_enum!(DesignRectangularPatternInstances, []; {
    Bodies(field0),
    Components {component_guid, seed, generated},
});
