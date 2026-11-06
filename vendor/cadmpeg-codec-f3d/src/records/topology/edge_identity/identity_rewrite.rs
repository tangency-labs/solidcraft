// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{DesignEdgeOperand, DesignEdgeTreatmentRadiusCandidate};

rewrite_native_record!(DesignEdgeOperand, []; {frame, id, scope_record_index, scope_reference_ordinal, class_tag, paired_byte_offset, paired_class_tag, recipe_record_byte_offset, recipe_id, recipe_prefix_bytes, recipe_references, recipe_program_offset, recipe_program, recipe_structure, surface_patch_recipe_structure, local_topology_references, candidate_faces, result_candidate_faces, result_boundary_edge_slots, preceding_candidate_faces, terminal_candidate_faces, changed_candidate_faces, preceding_boundary_edge_slots, terminal_boundary_edge_slots, changed_boundary_edge_slots, deleted_boundary_edge_slots, updated_boundary_edge_slots, treatment_radius_candidates, changed_boundary_edge_contexts, terminal_boundary_edge_contexts, terminal_reference_edge_slots, recipe_reference_contexts, recipe_selectors, recipe_state_id, resolved_edge_slot, resolved_axis, next_record_index, next_byte_offset});
rewrite_native_record!(DesignEdgeTreatmentRadiusCandidate, []; {edge_slot, radius});
