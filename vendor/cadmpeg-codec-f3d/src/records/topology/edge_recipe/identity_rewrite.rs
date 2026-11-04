// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignEdgeRecipeSelectorClause, DesignEdgeRecipeSelectorContext, DesignEdgeRecipeStructure,
    DesignSurfacePatchRecipeClause, DesignSurfacePatchRecipeStructure, DesignTopologyIncident,
    DesignTopologyIncidentSide, DesignTopologyRecipeEntry, DesignTopologyRecipeSide,
    DesignTopologyRecipeTriplet,
};

rewrite_native_record!(DesignEdgeRecipeSelectorClause, []; {entry, triplet_edge_slots});
rewrite_native_record!(DesignEdgeRecipeSelectorContext, []; {selector, clauses, incidence_matching_edge_slots, boundary_count_matching_edge_slots});
rewrite_native_record!(DesignEdgeRecipeStructure, []; {root, sides});
rewrite_native_record!(DesignSurfacePatchRecipeClause, []; {fields, face_reference_ordinals, edge_reference_ordinals, entries});
rewrite_native_record!(DesignSurfacePatchRecipeStructure, []; {clauses});
rewrite_native_scalar!(DesignTopologyIncident);
rewrite_native_scalar!(DesignTopologyIncidentSide);
rewrite_native_scalar!(DesignTopologyRecipeEntry);
rewrite_native_record!(DesignTopologyRecipeSide, []; {header_value, scalars, payload_prefix, entries});
rewrite_native_scalar!(DesignTopologyRecipeTriplet);
