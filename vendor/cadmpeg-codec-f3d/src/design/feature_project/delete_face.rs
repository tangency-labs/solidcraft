// SPDX-License-Identifier: Apache-2.0
//! `DeleteFace` and `SurfaceDeleteFace` scope projection.

use super::{face_scope_frame, unique_feature_match};
use crate::design::face_resolve::{resolved_face_group, resolved_historical_face_group};
use crate::ids::native_stream;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::{
    construction::DesignConstructionOperandGroup, extrude_selection::DesignOperandRole,
    face::DesignFaceOperand,
};
use cadmpeg_core::CodecError;

/// The frame of a `DeleteFace` scope whose references are typed: 51 bytes per
/// reference-table entry, with the count at offset 163 or 167 and the typed
/// scope tail. `None` for any other frame.
fn typed_delete_face_frame(scope: &DesignParameterScope) -> Option<()> {
    if scope.kind_name() != "DeleteFace"
        || !matches!(
            scope
                .reference_count_offset()
                .checked_sub(scope.byte_offset())?,
            163 | 167
        )
    {
        return None;
    }
    let reference_bytes = u64::try_from(scope.reference_members().len())
        .ok()?
        .checked_mul(u64::try_from(crate::bytes::TYPED_LOCAL_REFERENCE_LEN).ok()?)?;
    let kind_offset = scope
        .reference_count_offset()
        .checked_add(12)?
        .checked_add(reference_bytes)?;
    let kind_bytes = u64::try_from(scope.source_kind_name().encode_utf16().count())
        .ok()?
        .checked_mul(2)?;
    (scope.kind_offset() == kind_offset
        && scope.paired_byte_offset()
            == kind_offset.checked_add(kind_bytes)?.checked_add(
                u64::try_from(crate::layout::typed_reference_parameter_scope_tail::LEN).ok()?,
            )?)
    .then_some(())
}

pub(super) fn project_delete_face(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};

    let reference_count = scope.reference_members().len();
    let heal = if typed_delete_face_frame(scope).is_some() {
        true
    } else {
        let (count_offset, tail_length) = or_none!(face_scope_frame(scope));
        let heal = match scope.kind_name() {
            "DeleteFace" if matches!(count_offset, 123 | 127 | 131) => true,
            "SurfaceDeleteFace" if matches!(count_offset, 123 | 127 | 131 | 134 | 138 | 142) => {
                false
            }
            _ => return Ok(None),
        };
        if !matches!(tail_length, 77 | 78) {
            return Ok(None);
        }
        heal
    };
    if reference_count < 2 {
        return Ok(None);
    }
    let stream = or_none!(native_stream(&scope.id));
    let matching = construction_groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream) && group.scope_record_index == scope.record_index
    });
    let Some(group) = unique_feature_match(matching) else {
        return Ok(None);
    };
    if group.scope_reference_ordinal != 0
        || group.record_index != *or_none!(scope.reference_members().values().next())
        || group.role() != DesignOperandRole::ROLE_0X10
        || !group.members().iter().map(|member| member.value).eq(scope
            .reference_members()
            .values()
            .skip(1)
            .copied())
    {
        return Ok(None);
    }
    let faces = resolved_historical_face_group(
        scope,
        scope.previous_history_state_id(),
        group,
        face_operands,
    )
    .map_or_else(
        || {
            if heal {
                Ok(None)
            } else {
                resolved_face_group(group, face_operands)
            }
        },
        |face| Ok(Some(face)),
    )?;
    let faces = match faces {
        Some(faces) => faces,
        None => FaceSelection::Native(group.id.clone()),
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::DeleteFace { faces, heal },
    )))
}
