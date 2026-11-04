// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignBaseFlangeScope, DesignExtrudeScope, DesignFaceRecipeKind, DesignJointOriginReference,
    DesignJointOriginTransform, DesignNativeFeatureName, DesignParameterScope,
    DesignSketchEntityBinding, DesignSweepScope, DesignWorkPlaneReference,
    DesignWorkPlaneTransform,
};

rewrite_native_record!(DesignBaseFlangeScope, []; {base_flange_operation, base_flange_profile});
rewrite_native_record!(DesignExtrudeScope, []; {extrude_prologue, fixed_extrude_parameters, extrude_profile, extrude_native_profiles});
rewrite_native_scalar!(DesignFaceRecipeKind);
rewrite_native_record!(DesignJointOriginReference, []; {joint_origin_reference, joint_origin_reference_offset});
rewrite_native_record!(DesignJointOriginTransform, []; {joint_origin_transform, joint_origin_transform_offset, reference});
rewrite_native_scalar!(DesignNativeFeatureName);
rewrite_native_record!(DesignParameterScope, []; {id, byte_offset, class_tag, record_index, frame_length, kind_offset, history_state_id_offset, feature_ordinal, feature_ordinal_offset, history_state_id, previous_history_state_id, previous_history_state_id_offset, reference_count_offset, reference_members, occurrence_transfer, payload, unclosed_construction_operand_groups, paired_class_tag, paired_byte_offset}; native scope_fields);
rewrite_native_record!(DesignSketchEntityBinding, []; {entity_id, entity_reference_offset});
rewrite_native_record!(DesignSweepScope, []; {construction, sweep_profile});
rewrite_native_record!(DesignWorkPlaneReference, []; {work_plane_reference, work_plane_reference_offset});
rewrite_native_record!(DesignWorkPlaneTransform, []; {work_plane_transform, work_plane_transform_offset, reference, work_plane_construction});

fn scope_fields<F: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: &mut serde_json::Value,
    map: &mut cadmpeg_ir::schema::rewrite::typed::IdentityMap<'_, F>,
) -> Result<(), cadmpeg_core::CodecError> {
    cadmpeg_ir::schema::rewrite::typed::native_fields::rewrite_field(
        ctx,
        value,
        "emboss_construction",
        map,
        DesignParameterScope::emboss_construction,
    )?;
    cadmpeg_ir::schema::rewrite::typed::native_fields::rewrite_field(
        ctx,
        value,
        "assembly_alignment",
        map,
        DesignParameterScope::assembly_alignment,
    )?;
    cadmpeg_ir::schema::rewrite::typed::native_fields::rewrite_field(
        ctx,
        value,
        "work_plane_construction",
        map,
        |owner: &DesignParameterScope| {
            owner
                .work_plane_frame()
                .and_then(|frame| frame.work_plane_construction.as_ref())
        },
    )?;
    cadmpeg_ir::schema::rewrite::typed::native_fields::rewrite_field(
        ctx,
        value,
        "work_point_construction",
        map,
        DesignParameterScope::work_point_construction,
    )
}
