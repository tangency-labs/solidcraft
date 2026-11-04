// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignBaseFlangeOperation, DesignBendPosition, DesignEdgeFlangeEdge,
    DesignEdgeFlangeHeightExtent, DesignEdgeFlangeOperation, DesignEdgeFlangeSelection,
    DesignEdgeFlangeShape, DesignEdgeFlangeWidthParameterSource, DesignFlangeEdgeWidth,
    DesignHemOperation, DesignHemParameterOwners, DesignRecipeGroupIndex,
    DesignSheetMetalHeightDatum,
};

rewrite_native_record!(DesignBaseFlangeOperation, []; {thickness, thickness_offset, profile_group_record_index, profile_record_index, thickness_record_index, settings_record_index});
rewrite_native_scalar!(DesignBendPosition);
rewrite_native_scalar!(DesignEdgeFlangeEdge);
rewrite_native_scalar!(DesignEdgeFlangeHeightExtent);
rewrite_native_record!(DesignEdgeFlangeOperation, []; {selection, height_owner_record_index, angle_owner_record_index, auxiliary_reference_record_indices, settings_record_index, bend_radius, bend_radius_offset, height_datum, bend_position});
rewrite_native_record!(DesignEdgeFlangeSelection, []; {shape, aggregate_group_record_index});
rewrite_native_enum!(DesignEdgeFlangeShape, []; {
    FullEdge {edges, height},
    Symmetric {edges, owner},
    TwoSides {edges, owners},
    SymmetricPerEdge(field0),
    TwoSidesPerEdge {edges, source},
});
rewrite_native_scalar!(DesignEdgeFlangeWidthParameterSource);
rewrite_native_record!(DesignFlangeEdgeWidth<T>, [T]; {edge, owners});
rewrite_native_scalar!(DesignHemOperation);
rewrite_native_scalar!(DesignHemParameterOwners);
rewrite_native_scalar!(DesignRecipeGroupIndex);
rewrite_native_scalar!(DesignSheetMetalHeightDatum);
