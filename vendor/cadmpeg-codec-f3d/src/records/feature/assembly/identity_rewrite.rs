// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignAssemblyAlignment, DesignAssemblyAlignmentForm, DesignAssemblyAxialOperandTarget,
    DesignAssemblyAxialSelectorIdentity, DesignAssemblyLegacyOperand, DesignAssemblyLegacyOperands,
    DesignAssemblyLegacySelection, DesignAssemblyLimitKind, DesignAssemblyLimits,
    DesignAssemblyOperandFrame, DesignAssemblyOperandPath, DesignAssemblyOperandPathLink,
    DesignAssemblyOperandQualifier, DesignAssemblyOwnerRun, DesignAssemblySolvedFrame,
    DesignJointFrameSense, DesignQualifiedAssemblyOperand,
};

rewrite_native_record!(DesignAssemblyAlignment, []; {angle, offset, owners, form, frame_sense, additional_owner_run, additional_members}; native assembly_fields);
rewrite_native_record!(DesignAssemblyOwnerRun, []; {count, references});
rewrite_native_scalar!(DesignJointFrameSense);
rewrite_native_enum!(DesignAssemblyAlignmentForm, []; {
    DatumEnvelope {joint_origin_scope_record_index},
    LimitsOnly {limits},
    SolvedOnly {solved_frame, limits},
    LegacyAsBuilt421 {carriers, solved_frame, limits, frames_field_present},
    Frames {frames},
    Qualified(field0),
});
rewrite_native_enum!(DesignAssemblyAxialOperandTarget, []; {
    ComponentInsertOccurrence {component_insert_scope_record_index, construction_record_index, construction_class_tag, construction_byte_offset, construction_transform_offset, axis_record_index_offsets, construction_paired_class_tag, construction_paired_byte_offset, selectors},
    DocumentRootJointOrigin {scope_record_index},
});
rewrite_native_record!(DesignAssemblyAxialSelectorIdentity, []; {axis_record_index, axis_class_tag, axis_byte_offset, axis_paired_class_tag, axis_paired_byte_offset, selector_record_index, selector_class_tag, selector_byte_offset, selector_paired_class_tag, selector_paired_byte_offset, nested_record_index, nested_record_index_offset, selector_asset_id, selector_asset_id_offset, selector_context_id, selector_context_id_offset, occurrence_reference, occurrence_reference_offset, external_object_reference, external_object_reference_offset, external_segment, external_segment_offset, external_asset_id, external_asset_id_offset, external_link_name, external_link_name_offset, external_version, role_record_index, role_class_tag, role_byte_offset, occurrence_role, occurrence_role_offset});
rewrite_native_record!(DesignAssemblyLegacyOperand<C>, [C]; {construction_class_tag, construction, selection, reference_offset});
rewrite_native_record!(DesignAssemblyLegacyOperands, []; {point, hole}; native legacy_operand_fields);
rewrite_native_record!(DesignAssemblyLegacySelection, []; {record_index, byte_offset, class_tag, asset_id, asset_id_offset, context_id, context_id_offset, recipe_record_index, recipe_record_byte_offset, recipe_id, recipe_kind, recipe_references, next_byte_offset});
rewrite_native_scalar!(DesignAssemblyLimitKind);
rewrite_native_record!(DesignAssemblyLimits, []; {kind, minimum, maximum, owner_record_indices, value_offsets});
rewrite_native_record!(DesignAssemblyOperandFrame, []; {reference_record_index, reference_offset, transform, transform_offset});
rewrite_native_record!(DesignAssemblyOperandPath, []; {link, record_index, class_tag, byte_offset, occurrence_guids, identity_guids, paired_occurrence_keys});
rewrite_native_record!(DesignAssemblyOperandPathLink, []; {locator_reference_offset, locator_record_index, locator_class_tag, locator_byte_offset, locator_scope_reference_offset, wrapper_record_index, wrapper_reference_offset, wrapper_class_tag, wrapper_byte_offset, path_reference_offset});
rewrite_native_enum!(DesignAssemblyOperandQualifier, []; {
    OccurrencePath {path},
    AxialTarget {target},
    JointOrigin {scope_record_index, class_tag, byte_offset, paired_class_tag, paired_byte_offset},
});
rewrite_native_record!(DesignAssemblySolvedFrame, []; {reference_record_index, reference_offset, record_byte_offset, class_tag, transform, transform_offset});
rewrite_native_record!(DesignQualifiedAssemblyOperand, []; {frame, qualifier});

fn assembly_fields<F: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: &mut serde_json::Value,
    map: &mut cadmpeg_ir::schema::rewrite::typed::IdentityMap<'_, F>,
) -> Result<(), cadmpeg_core::CodecError> {
    cadmpeg_ir::schema::rewrite::typed::native_fields::rewrite_field(
        ctx,
        value,
        "legacy_operand_carriers",
        map,
        |owner: &DesignAssemblyAlignment| match owner.form.as_ref() {
            Some(DesignAssemblyAlignmentForm::LegacyAsBuilt421 { carriers, .. }) => Some(carriers),
            _ => None,
        },
    )
}

fn legacy_operand_fields<F: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: &mut serde_json::Value,
    map: &mut cadmpeg_ir::schema::rewrite::typed::IdentityMap<'_, F>,
) -> Result<(), cadmpeg_core::CodecError> {
    let serde_json::Value::Array(operands) = value else {
        return Ok(());
    };
    for operand in operands.iter_mut() {
        cadmpeg_ir::schema::rewrite::typed::native_fields::rewrite_field(
            ctx,
            operand,
            "selection",
            map,
            |owner: &DesignAssemblyLegacyOperands| Some(&owner.point.selection),
        )?;
    }
    if let Some(point) = operands.first_mut() {
        if let Some(construction) = point.get_mut("construction") {
            cadmpeg_ir::schema::rewrite::typed::native_fields::rewrite_field(
                ctx,
                construction,
                "value",
                map,
                |owner: &DesignAssemblyLegacyOperands| Some(&owner.point.construction),
            )?;
        }
    }
    Ok(())
}
