// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{DesignDimensionRecipeRecord, DesignRecipeReference};

rewrite_native_record!(DesignDimensionRecipeRecord, []; {id, companion_record_index, recipe_ordinal, recipe_id, recipe_kind, byte_offset, class_tag, record_index, frame_length, prefix_offset, prefix_bytes, references, program_offset, program, matching_edge_operand_ids});
rewrite_native_record!(DesignRecipeReference, []; {selector, selector_offset, token, token_offset, design_reference, design_reference_offset, candidate_faces, candidate_edges, alternate_selector_faces, alternate_selector_edges});
