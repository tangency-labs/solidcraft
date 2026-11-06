// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{DesignFaceOperand, DesignFaceRecipeNode, DesignFaceRecipeStructure};

rewrite_native_record!(DesignFaceOperand, []; {frame, id, scope_record_index, scope_reference_ordinal, group, class_tag, paired_byte_offset, paired_class_tag, recipe_record_byte_offset, recipe_id, recipe_prefix_bytes, recipe_references, recipe_kind, recipe_program_offset, recipe_program, recipe_nodes, candidate_faces, unreferenced_candidate_faces, alternate_selector_candidate_faces, preceding_candidate_faces, changed_candidate_faces, historical_support_contexts, resolved_face_slots, resolved_active_face, next_record_index, next_byte_offset});
rewrite_native_record!(DesignFaceRecipeNode, []; {byte_offset, end_byte_offset, program, recipe_structure});
rewrite_native_record!(DesignFaceRecipeStructure, []; {root, prelude, sides, postlude_value});
