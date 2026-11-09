// SPDX-License-Identifier: Apache-2.0
//! Bind solved placement geometry to construction-plane scopes.

use std::collections::HashMap;

use crate::ids::native_stream;
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut, DesignWorkPlaneTransform,
};
use crate::records::placement_construction::DesignPlacementConstruction;

pub(crate) fn has_operand(
    scope: &DesignParameterScope,
    placements: &[DesignPlacementConstruction],
) -> bool {
    placements
        .iter()
        .any(|placement| references(scope, placement))
}

fn references(scope: &DesignParameterScope, placement: &DesignPlacementConstruction) -> bool {
    native_stream(&scope.id).is_some_and(|stream| native_stream(&placement.id) == Some(stream))
        && scope
            .reference_members()
            .values()
            .any(|index| u64::from(*index) == placement.record_index)
}

fn unique_placement<'a>(
    scope: &DesignParameterScope,
    placements: &'a [DesignPlacementConstruction],
) -> Option<&'a DesignPlacementConstruction> {
    if scope.kind() != DesignFeatureKind::WorkPlane {
        return None;
    }
    let mut candidates = placements
        .iter()
        .filter(|placement| references(scope, placement));
    let placement = candidates.next()?;
    candidates.next().is_none().then_some(placement)
}

pub(crate) fn solved_frame(
    scope: &DesignParameterScope,
    placements: &[DesignPlacementConstruction],
) -> Option<DesignWorkPlaneTransform> {
    let frame = super::checked_solved_frame(unique_placement(scope, placements)?)?;
    let (matrix, offset) = (frame.value, frame.offset);
    Some(DesignWorkPlaneTransform {
        work_plane_transform: matrix,
        work_plane_transform_offset: offset,
        reference: None,
        work_plane_construction: None,
    })
}

pub(crate) fn bind(
    scopes: &mut [DesignParameterScope],
    placements: &[DesignPlacementConstruction],
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    let mut losses = Vec::new();
    for scope in scopes {
        if scope.kind() != DesignFeatureKind::WorkPlane {
            continue;
        }
        if has_operand(scope, placements) {
            let frame = solved_frame(scope, placements);
            if let Some(placement) = unique_placement(scope, placements).filter(|placement| {
                frame.is_some()
                    && (placement.fields.inferred_x_axis.value != 0
                        || placement.fields.inferred_z_axis.value != 0)
            }) {
                losses.push(crate::loss::F3dLossCode::WorkPlaneInferredAxisAssumed.note(format!(
                    "WorkPlane {} takes its frame from the stored matrix or identity marker of placement {}, whose isInferredXAxis byte is {} and isInferredZAxis byte is {}.",
                    scope.id,
                    placement.id,
                    placement.fields.inferred_x_axis.value,
                    placement.fields.inferred_z_axis.value
                )));
            }
            if let DesignScopePayloadMut::WorkPlane(slot) = scope.payload_mut() {
                *slot = frame;
            }
        }
        if scope.work_plane_frame().is_none() {
            losses.push(crate::loss::F3dLossCode::WorkPlaneFrameUnbound.note(format!(
                "WorkPlane {} retains its native definition because no unique checked solved frame binds its operands.",
                scope.id
            )));
        }
    }
    losses
}

pub(crate) fn class_pair_inferred_axis_losses(
    scopes: &[DesignParameterScope],
    placements: &[DesignPlacementConstruction],
    inferred_axes: &HashMap<String, (u64, [u8; 2])>,
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    scopes
        .iter()
        .filter(|scope| !has_operand(scope, placements))
        .filter_map(|scope| {
            let frame = scope.work_plane_frame()?;
            let &(offset, [inferred_x_axis, inferred_z_axis]) = inferred_axes.get(&scope.id)?;
            (frame.work_plane_transform_offset == offset).then(|| {
                crate::loss::F3dLossCode::WorkPlaneInferredAxisAssumed.note(format!(
                    "WorkPlane {} takes its frame from the class-pair placement matrix at offset {offset}, whose isInferredXAxis byte is {inferred_x_axis} and isInferredZAxis byte is {inferred_z_axis}.",
                    scope.id
                ))
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
