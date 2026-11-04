// SPDX-License-Identifier: Apache-2.0
//! Selected faces of a `MoveFace` or `OffsetFaces` feature, from the transitions
//! that move them.

use super::output_chain::OutputChain;
use super::surface_rows::{Carrier, StateRows};
use crate::history_records::{AsmDeltaState, AsmHistoricalTopology, AsmHistory};
use cadmpeg_ir::features::{FaceMotion, FaceSelection};
use cadmpeg_ir::math::Vector3;
use std::collections::{BTreeSet, HashSet};

/// Millimetres between a stored surface position and the position the motion
/// states.
const FACE_MOTION_POSITION_TOLERANCE: f64 = 1.0e-6;
/// Distance between two unit directions that a motion leaves unchanged.
const FACE_MOTION_DIRECTION_TOLERANCE: f64 = 1.0e-9;

fn same_direction(left: Vector3, right: Vector3) -> Option<bool> {
    Some((left.unit()? - right.unit()?).norm() <= FACE_MOTION_DIRECTION_TOLERANCE)
}

/// Whether a surface row follows the motion. A row carries the rigid copy of
/// its surface, so a translation moves the origin of a plane whose translation
/// lies in the plane.
fn row_follows(before: Carrier<'_>, after: Carrier<'_>, motion: &FaceMotion) -> bool {
    let close = |value: f64| value.abs() <= FACE_MOTION_POSITION_TOLERANCE;
    match (motion, before, after) {
        (
            FaceMotion::Translate {
                direction,
                distance,
            },
            Carrier::Plane(before),
            Carrier::Plane(after),
        ) => {
            let shift = after.origin.vector_from(before.origin);
            same_direction(before.normal, after.normal) == Some(true)
                && (shift - direction.scale(distance.get())).norm()
                    <= FACE_MOTION_POSITION_TOLERANCE
        }
        (
            FaceMotion::Translate {
                direction,
                distance,
            },
            Carrier::Cylinder(before),
            Carrier::Cylinder(after),
        ) => {
            let shift = after.origin.vector_from(before.origin);
            same_direction(before.axis, after.axis) == Some(true)
                && close(after.radius - before.radius)
                && (shift - direction.scale(distance.get())).norm()
                    <= FACE_MOTION_POSITION_TOLERANCE
        }
        (FaceMotion::Offset { distance }, Carrier::Plane(before), Carrier::Plane(after)) => {
            let Some(normal) = before.normal.unit() else {
                return false;
            };
            same_direction(before.normal, after.normal) == Some(true)
                && close(
                    after.origin.vector_from(before.origin).dot(normal).abs()
                        - distance.get().abs(),
                )
        }
        (FaceMotion::Offset { distance }, Carrier::Cylinder(before), Carrier::Cylinder(after)) => {
            let Some(axis) = before.axis.unit() else {
                return false;
            };
            same_direction(before.axis, after.axis) == Some(true)
                && after.origin.vector_from(before.origin).cross(axis).norm()
                    <= FACE_MOTION_POSITION_TOLERANCE
                && close((after.radius - before.radius).abs() - distance.get().abs())
        }
        _ => false,
    }
}

/// Faces of the input state that a `MoveFace` or `OffsetFaces` feature selects,
/// sorted.
///
/// The motion moves every selected face, and a face tangent to a selected face
/// can move with it. When no face is deleted and every updated face has a plane
/// or cylinder row, the moved faces therefore hold the whole selection, so a
/// moved set with one face per distinct member is the selection.
fn moved_face_selection(
    members: &[u32],
    motion: &FaceMotion,
    updated: &BTreeSet<i64>,
    input: &AsmHistoricalTopology,
    output: &AsmHistoricalTopology,
) -> Option<Vec<i64>> {
    let magnitude = match motion {
        FaceMotion::Offset { distance } | FaceMotion::Translate { distance, .. } => distance.get(),
        FaceMotion::Rotate { .. } => return None,
    };
    if magnitude.abs() <= FACE_MOTION_POSITION_TOLERANCE {
        return None;
    }
    let input_rows = StateRows::new(input);
    let output_rows = StateRows::new(output);
    let mut moved = Vec::new();
    for &face in updated {
        if row_follows(
            input_rows.carrier(face)?,
            output_rows.carrier(face)?,
            motion,
        ) {
            moved.push(face);
        }
    }
    let distinct_members = members.iter().collect::<HashSet<_>>().len();
    (!moved.is_empty() && moved.len() == distinct_members).then_some(moved)
}

/// Faces that the transitions from `previous_state_id` to `state` update, or
/// `None` when one of them deletes a face.
fn updated_faces(
    histories: &[AsmHistory],
    history: &AsmHistory,
    state: &AsmDeltaState,
    previous_state_id: i64,
) -> Option<BTreeSet<i64>> {
    let chain = OutputChain::resolve(
        histories,
        state.state_id,
        previous_state_id,
        Some(&history.id),
    )?;
    let mut updated = BTreeSet::new();
    for state_id in &chain.states {
        let mut matching = history
            .states()
            .iter()
            .filter(|member| member.state_id == *state_id);
        let (Some(member), None) = (matching.next(), matching.next()) else {
            return None;
        };
        let faces = &member.transition.as_ref()?.topology.faces;
        if !faces.deleted.is_empty() {
            return None;
        }
        updated.extend(faces.updated.iter().copied());
    }
    Some(updated)
}

/// The states that one `MoveFace` or `OffsetFaces` feature spans.
pub(super) struct MoveFaceStates<'a> {
    pub(super) histories: &'a [AsmHistory],
    pub(super) history: &'a AsmHistory,
    pub(super) state: &'a AsmDeltaState,
    pub(super) previous: &'a AsmDeltaState,
}

/// Bind a selection that still names its operand group to the faces that its
/// transitions move.
pub(super) fn bind_move_face_selection(
    selection: &mut FaceSelection,
    motion: &FaceMotion,
    feature_id: &cadmpeg_ir::features::FeatureId,
    scope: &crate::records::feature::scope::DesignParameterScope,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    states: &MoveFaceStates<'_>,
) {
    let MoveFaceStates {
        histories,
        history,
        state,
        previous,
    } = *states;
    let FaceSelection::Native(native) = selection else {
        return;
    };
    let mut matching = groups.iter().filter(|group| group.id == *native);
    let (Some(group), None) = (matching.next(), matching.next()) else {
        return;
    };
    if group.scope_record_index != scope.record_index {
        return;
    }
    let members: Vec<u32> = group.members().iter().map(|member| member.value).collect();
    let Some(updated) = updated_faces(histories, history, state, previous.state_id) else {
        return;
    };
    let (Some(input), Some(output)) = (history.topology(previous), history.topology(state)) else {
        return;
    };
    let Some(faces) = moved_face_selection(&members, motion, &updated, input, output) else {
        return;
    };
    let prefix = crate::ids::feature_input_prefix(feature_id, previous.state_id);
    let selected = faces
        .into_iter()
        .map(|slot| {
            crate::ids::history_input_face_id(&prefix, cadmpeg_ir::ids::IdentityKey::from(slot))
        })
        .collect();
    if let Ok(historical) = FaceSelection::historical(
        crate::ids::history_input_state_id(&prefix),
        selected,
        native.clone(),
    ) {
        *selection = historical;
    }
}

#[cfg(test)]
mod tests;
