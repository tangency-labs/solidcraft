// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignCoilExtent, DesignCoilPlacement, DesignCoilScope, DesignCoilSection,
    DesignCoilSectionPlacement, DesignCoilSelection, DesignCoilTransform,
};

rewrite_native_scalar!(DesignCoilExtent);
rewrite_native_record!(DesignCoilPlacement, []; {selection_record_index, selection_record_byte_offset, selection_class_tag, selection, transform_record_index, transform_record_byte_offset, transform_class_tag, explicit_transform});
rewrite_native_record!(DesignCoilScope, []; {operation, extent, section, section_placement, clockwise, placement, transform});
rewrite_native_scalar!(DesignCoilSection);
rewrite_native_scalar!(DesignCoilSectionPlacement);
rewrite_native_enum!(DesignCoilSelection, []; {
    Persistent {asset_id, context_id, identity_record_index, primary_identity, secondary},
    FaceRecipe {asset_id, context_id, recipe_record_index, recipe_record_byte_offset, recipe_id, recipe_kind, design},
});
rewrite_native_record!(DesignCoilTransform, []; {transform, transform_offset});
