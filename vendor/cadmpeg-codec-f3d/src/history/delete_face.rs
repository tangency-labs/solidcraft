// SPDX-License-Identifier: Apache-2.0
//! Selected faces of a healed `DeleteFace`, from the transition that removes them.

use crate::history_records::{AsmHistoricalTopology, AsmHistoricalTransition};
use cadmpeg_ir::features::FaceSelection;
use std::collections::HashSet;

/// Faces of the preceding state that a healed `DeleteFace` with `members` selects,
/// sorted.
///
/// The operation removes every selected face, and healing can remove further
/// faces. So the deleted faces are the selection only when their count equals
/// the count of distinct members.
fn delete_face_selection(
    members: &[u32],
    previous_state_id: i64,
    transition: &AsmHistoricalTransition,
    preceding: &AsmHistoricalTopology,
) -> Option<Vec<i64>> {
    if transition.previous_state_id != Some(previous_state_id) {
        return None;
    }
    let deleted = &transition.topology.faces.deleted;
    let distinct_members = members.iter().collect::<HashSet<_>>().len();
    if deleted.is_empty() || deleted.len() != distinct_members {
        return None;
    }
    let preceding_faces: HashSet<i64> = preceding.faces.iter().copied().collect();
    if !deleted.iter().all(|face| preceding_faces.contains(face)) {
        return None;
    }
    let mut faces = deleted.clone();
    faces.sort_unstable();
    Some(faces)
}

/// Bind a native healed `DeleteFace` group to the faces its transition removes.
pub(super) fn bind_delete_face_selection(
    selection: &mut FaceSelection,
    feature_id: &cadmpeg_ir::features::FeatureId,
    scope: &crate::records::feature::scope::DesignParameterScope,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    previous_state_id: i64,
    transition: &AsmHistoricalTransition,
    preceding: Option<&AsmHistoricalTopology>,
) {
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
    let Some(faces) = preceding.and_then(|preceding| {
        delete_face_selection(&members, previous_state_id, transition, preceding)
    }) else {
        return;
    };
    let prefix = crate::ids::feature_input_prefix(feature_id, previous_state_id);
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
