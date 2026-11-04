// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignRegionIncidence, DesignSketchProfileOperand, DesignSketchProfileRegion,
    DesignSketchProfileRegionMember, DesignSketchProfileRegionSelection,
};

rewrite_native_scalar!(DesignRegionIncidence);
rewrite_native_record!(DesignSketchProfileOperand, []; {scope_reference_ordinal, record_index, byte_offset, class_tag, asset_id, asset_id_offset, entity_id, entity_reference_offset, region_selection, paired_class_tag, paired_byte_offset});
rewrite_native_record!(DesignSketchProfileRegion, []; {member_count_offset, members});
rewrite_native_scalar!(DesignSketchProfileRegionMember);
rewrite_native_record!(DesignSketchProfileRegionSelection, []; {record_index, byte_offset, class_tag, region_count_offset, regions, companion_class_tag, companion_byte_offset});
