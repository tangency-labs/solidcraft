// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignEdgeRecipeReferenceContext, DesignHistoricalEdgeContext, DesignHistoricalEdgeLoopContext,
    DesignHistoricalFaceBoundaryContext, DesignHistoricalFaceLoopContext,
    DesignHistoricalFaceSupportContext, DesignHistoricalLoopBoundary, DesignHistoricalLoopCoedge,
    DesignHistoricalLoopPoint, DesignHistoricalLoopPosition, DesignHistoricalLoopVertex,
};

rewrite_native_record!(DesignEdgeRecipeReferenceContext, []; {reference_ordinal, result_faces, result_face_boundary_ids, result_shared_edge_slots, preceding_faces, preceding_face_boundary_ids, preceding_support_face_slots, preceding_support_face_boundary_ids, shared_edge_slots, changed_shared_edge_slots, changed_reference_edge_slots});
rewrite_native_record!(DesignHistoricalEdgeContext, []; {edge_slot, incident_loops});
rewrite_native_record!(DesignHistoricalEdgeLoopContext, []; {coedge_slot, loop_slot, face_slot, boundary_edge_count, coedge_ordinal, previous_edge_slot, next_edge_slot});
rewrite_native_record!(DesignHistoricalFaceBoundaryContext, []; {face_slot, loops});
rewrite_native_record!(DesignHistoricalFaceLoopContext, []; {loop_slot, boundary});
rewrite_native_record!(DesignHistoricalFaceSupportContext, []; {active_face_slot, surface_slot, preceding_face_slots, preceding_face_boundaries, changed_preceding_face_slots});
rewrite_native_enum!(DesignHistoricalLoopBoundary, []; {
    Coedges(field0),
    Vertices(field0),
    Points(field0),
    Positions(field0),
});
rewrite_native_record!(DesignHistoricalLoopCoedge, []; {coedge_slot, edge_slot});
rewrite_native_record!(DesignHistoricalLoopPoint, []; {vertex, point_slot});
rewrite_native_record!(DesignHistoricalLoopPosition, []; {point, position});
rewrite_native_record!(DesignHistoricalLoopVertex, []; {coedge, vertex_slot});
