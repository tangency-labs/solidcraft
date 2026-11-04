// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignCombineBodyNode, DesignCombineBodySelection, DesignCombineExternalBodyIdentity,
    DesignCombineForm, DesignCombineOperation, DesignCombineTools, DesignExternalVersion,
};

rewrite_native_record!(DesignCombineBodyNode, []; {record_index, body_entity_suffix, creating_scope_record_index});
rewrite_native_record!(DesignCombineBodySelection, []; {record_index, external_identity, body_node});
rewrite_native_record!(DesignCombineExternalBodyIdentity, []; {selector_asset_id, selector_asset_id_offset, selector_context_id, selector_context_id_offset, occurrence_reference, occurrence_reference_offset, external_body_reference, external_body_reference_offset, external_segment, external_segment_offset, external_asset_id_offset, external_link_name, external_link_name_offset, external_version, tail_values, tail_value_offsets});
rewrite_native_scalar!(DesignCombineForm);
rewrite_native_record!(DesignCombineOperation, []; {form, operation, operation_offset, keep_tools, keep_tools_offset, target_record_index, target_body_node, tools});
rewrite_native_record!(DesignCombineTools, []; {first, additional});
rewrite_native_record!(DesignExternalVersion, []; {property_key, version_urn});
