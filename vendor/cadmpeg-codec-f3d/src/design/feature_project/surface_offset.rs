// SPDX-License-Identifier: Apache-2.0
//! Project a `SurfaceOffset` scope as a neutral offset surface.

use crate::design::face_resolve::{resolved_face_group, resolved_historical_face_group};
use crate::ids::native_stream;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::surface_ops::{
    DesignSurfaceOffsetOperation, DesignSurfaceOffsetSupport,
};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::records::topology::face::DesignFaceOperand;
use cadmpeg_core::CodecError;

pub(super) fn project_surface_offset(
    scope: &DesignParameterScope,
    operation: &DesignSurfaceOffsetOperation,
    groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    histories: &[crate::history_records::AsmHistory],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};

    let Some(stream) = native_stream(&scope.id) else {
        return Ok(None);
    };
    let Some(distance) = cadmpeg_ir::scalar::Length::new(operation.distance.get() * 10.0) else {
        return Ok(None);
    };
    let DesignSurfaceOffsetSupport::FaceGroups {
        group_record_indices,
    } = &operation.support
    else {
        let DesignSurfaceOffsetSupport::BoundaryCarrier {
            boundary_record_index,
            ..
        } = &operation.support
        else {
            return Ok(None);
        };
        return Ok(Some(FeatureDefinition::Operation(
            FeatureOperation::OffsetSurface {
                faces: FaceSelection::Native(format!(
                    "{}{}{}",
                    stream,
                    ":design-record#",
                    u64::from(*boundary_record_index)
                )),
                distance: Some(distance),
            },
        )));
    };
    let mut support_groups = Vec::new();
    for group_record_index in group_record_indices {
        let mut matching_groups = groups.iter().filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
                && group.record_index == *group_record_index
                && group.role() == DesignOperandRole::PROFILE
                && !group.members().is_empty()
        });
        let Some(group) = matching_groups.next() else {
            return Ok(None);
        };
        if matching_groups.next().is_some() {
            return Ok(None);
        }
        support_groups.push(group);
    }
    let native = scope.id.clone();
    let faces = if let Some(faces) = resolved_surface_offset_faces(&support_groups, face_operands)?
    {
        FaceSelection::Resolved { faces, native }
    } else {
        historical_surface_offset_faces(scope, &support_groups, face_operands, histories)
            .and_then(|(state, faces)| FaceSelection::historical(state, faces, native.clone()).ok())
            .unwrap_or(FaceSelection::Native(native))
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::OffsetSurface {
            faces,
            distance: Some(distance),
        },
    )))
}

fn resolved_surface_offset_faces(
    groups: &[&DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
) -> Result<Option<Vec<cadmpeg_ir::ids::FaceId>>, CodecError> {
    let mut faces = Vec::new();
    for group in groups {
        let Some(cadmpeg_ir::features::FaceSelection::Resolved {
            faces: group_faces, ..
        }) = resolved_face_group(group, face_operands)?
        else {
            return Ok(None);
        };
        for face in group_faces {
            if !faces.contains(&face) {
                faces.push(face);
            }
        }
    }
    Ok(Some(faces))
}

fn historical_surface_offset_faces(
    scope: &DesignParameterScope,
    groups: &[&DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    histories: &[crate::history_records::AsmHistory],
) -> Option<(
    cadmpeg_ir::ids::FeatureInputTopologyId,
    Vec<cadmpeg_ir::ids::HistoricalFaceId>,
)> {
    let previous_state_id =
        crate::history::effective_scope_previous_history_state_id(scope, histories);
    let mut state = None;
    let mut faces = Vec::new();
    for group in groups {
        let cadmpeg_ir::features::FaceSelection::Historical {
            state: group_state,
            faces: group_faces,
            ..
        } = resolved_historical_face_group(scope, previous_state_id, group, face_operands)?
        else {
            return None;
        };
        state = Some(group_state);
        for face in group_faces {
            if !faces.contains(&face) {
                faces.push(face);
            }
        }
    }
    Some((state?, faces))
}
