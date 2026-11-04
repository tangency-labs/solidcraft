// SPDX-License-Identifier: Apache-2.0
use super::{
    DesignPlacementConstruction, PlacementFields, PlacementReference, PlacementReferenceList,
};
rewrite_native_record!(PlacementReference, []; {offset, encoding});
rewrite_native_record!(PlacementReferenceList, []; {count, members});
rewrite_native_record!(PlacementFields<Ref, List>, [Ref, List]; {linear_offset_on_periodic_face, align_up_direction, arc_length_for_periodic_face, periodic_face_mode, axis_flipped, use_u_direction, definer_creator, extra_offset, global_parameters, inferred_x_axis, inferred_z_axis, z_flip, identity, matrix, old_rotation, pick_point_parameter, pick_point, edge_references, origin, plane_references, x_axis, z_axis, ref_type, resize_type, values});
rewrite_native_identity_record!(DesignPlacementConstruction; record_index, class_tag, type_version, byte_offset, member_offset, paired_byte_offset, fields, base_level);
