// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    AsmHistoricalEntityKind, DesignBodyRecipeOperand, DesignBodyRecipeReference,
    DesignOperandGroup, DesignOperandOwner,
};

rewrite_native_scalar!(AsmHistoricalEntityKind);
rewrite_native_record!(DesignBodyRecipeOperand, []; {frame, id, scope_record_index, owner, class_tag, asset_id, context_id, context_id_offset, selector_tail, references, recipe_id, resolved_face_slot, resolved_body_state_id, resolved_body_slot, resolved_body_face_slots, next_byte_offset});
rewrite_native_record!(DesignBodyRecipeReference, []; {design_reference, design_reference_offset, form, form_offset, candidate_faces, preceding_candidate_faces, preceding_body_slots});
rewrite_native_scalar!(DesignOperandGroup);
rewrite_native_scalar!(DesignOperandOwner);
