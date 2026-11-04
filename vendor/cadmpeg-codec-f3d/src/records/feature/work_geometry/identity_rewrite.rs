// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignEdgeTreatmentVertexOperand, DesignVertexRecipe, DesignVertexResolution,
    DesignWorkAxisConstruction, DesignWorkAxisSource, DesignWorkPlaneConstruction,
    DesignWorkPointConstruction, DesignWorkPointInput, DesignWorkPointInputCarrier,
    DesignWorkPointPlaneSelection, DesignWorkPointRule, DesignWorkPointRuleForm,
    DesignWorkPointSketchPointSelection,
};

rewrite_native_record!(DesignEdgeTreatmentVertexOperand, []; {id, scope_record_index, scope_reference_ordinal, group_record_index, group_member_ordinal, recipe});
rewrite_native_record!(DesignVertexRecipe, []; {frame, class_tag, paired_byte_offset, paired_class_tag, recipe_record_byte_offset, recipe_id, recipe_prefix_bytes, recipe_references, recipe_program_offset, recipe_program, resolution, next_byte_offset});
rewrite_native_scalar!(DesignVertexResolution);
rewrite_native_record!(DesignWorkAxisConstruction, []; {origin, displacement, origin_offset, displacement_offset, source});
rewrite_native_enum!(DesignWorkAxisSource, []; {
    TwoPoint {point_record_indices, point_offsets},
    DirectCarrier {carrier_record_index, support_record_index},
});
rewrite_native_record!(DesignWorkPlaneConstruction, []; {placement_record_index, inputs});
rewrite_native_record!(DesignWorkPointConstruction, []; {point_record_index, point_record_byte_offset, position, position_offset, rule, reference_type_offset});
rewrite_native_record!(DesignWorkPointInput, []; {record_index, reference_offset, carrier});
rewrite_native_enum!(DesignWorkPointInputCarrier, []; {
    EdgeRecipe {operand_id},
    VertexRecipe {recipe},
    WorkPlane {selection},
    SketchPoint {selection},
}; native work_point_carrier_fields);
rewrite_native_record!(DesignWorkPointPlaneSelection, []; {class_tag, asset_id, asset_id_offset, context_id, context_id_offset, identity_record_offset, primary_identity, work_plane_scope_record_index});
rewrite_native_record!(DesignWorkPointRule, []; {form}; native <DesignWorkPointRuleForm as cadmpeg_ir::schema::rewrite::typed::RewriteIdentities>::rewrite_native_value);
rewrite_native_enum!(DesignWorkPointRuleForm, []; {
    CircleCenter {input},
    TwoEdgeIntersection {inputs},
    ThreePlaneIntersection {inputs},
    Vertex {input},
    EdgePlaneIntersection {inputs},
    DistanceOnEdge {input},
    Native {reference_type, inputs},
}; native work_point_form_fields);
rewrite_native_record!(DesignWorkPointSketchPointSelection, []; {class_tag, asset_id, asset_id_offset, context_id, context_id_offset, identity_record_offset, sketch_record_index, point_persistent_id, point_native_id});

fn work_point_form_fields<F: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: &mut serde_json::Value,
    map: &mut cadmpeg_ir::schema::rewrite::typed::IdentityMap<'_, F>,
) -> Result<(), cadmpeg_core::CodecError> {
    cadmpeg_ir::schema::rewrite::typed::native_fields::rewrite_field(
        ctx,
        value,
        "input",
        map,
        |owner: &DesignWorkPointRuleForm| match owner {
            DesignWorkPointRuleForm::CircleCenter { input }
            | DesignWorkPointRuleForm::Vertex { input }
            | DesignWorkPointRuleForm::DistanceOnEdge { input } => Some(input),
            _ => None,
        },
    )?;
    cadmpeg_ir::schema::rewrite::typed::native_fields::rewrite_field(
        ctx,
        value,
        "inputs",
        map,
        |owner: &DesignWorkPointRuleForm| match owner {
            DesignWorkPointRuleForm::Native { inputs, .. } => Some(inputs),
            _ => None,
        },
    )
}

fn work_point_carrier_fields<F: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: &mut serde_json::Value,
    map: &mut cadmpeg_ir::schema::rewrite::typed::IdentityMap<'_, F>,
) -> Result<(), cadmpeg_core::CodecError> {
    cadmpeg_ir::schema::rewrite::typed::native_fields::rewrite_field(
        ctx,
        value,
        "recipe",
        map,
        |owner: &DesignWorkPointInputCarrier| match owner {
            DesignWorkPointInputCarrier::VertexRecipe { recipe } => Some(recipe),
            _ => None,
        },
    )
}
