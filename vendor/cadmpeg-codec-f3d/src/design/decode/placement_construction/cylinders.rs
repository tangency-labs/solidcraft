// SPDX-License-Identifier: Apache-2.0
//! Bind named cylinder primitives to their solved placement operands.

use crate::ids::native_stream;
use crate::loss::F3dLossCode;
use crate::records::feature::scope::{DesignParameterScope, DesignScopePayloadMut};
use crate::records::identity::Located;
use crate::records::placement_construction::DesignPlacementConstruction;
use crate::records::sketch_placement::SketchPlacementMatrix;
use cadmpeg_ir::report::loss::LossNote;

fn solved_frame(
    scope: &DesignParameterScope,
    placements: &[DesignPlacementConstruction],
) -> Option<Located<SketchPlacementMatrix>> {
    let stream = native_stream(&scope.id)?;
    let mut matches = placements.iter().filter(|placement| {
        native_stream(&placement.id) == Some(stream)
            && scope
                .reference_members()
                .values()
                .any(|record_index| u64::from(*record_index) == placement.record_index)
    });
    let placement = matches.next()?;
    if matches.next().is_some() || placement.type_version != 9 {
        return None;
    }
    super::checked_solved_frame(placement)
}

pub(crate) fn bind(
    scopes: &mut [DesignParameterScope],
    placements: &[DesignPlacementConstruction],
) -> Vec<LossNote> {
    let mut losses = Vec::new();
    for scope in scopes {
        let DesignScopePayloadMut::CylinderPrimitive(Some(cylinder)) = scope.payload_mut() else {
            continue;
        };
        if cylinder.transform.is_some() {
            continue;
        }
        let transform = solved_frame(scope, placements);
        if let DesignScopePayloadMut::CylinderPrimitive(Some(cylinder)) = scope.payload_mut() {
            cylinder.transform = transform;
        }
        if transform.is_none() {
            losses.push(F3dLossCode::CylinderPlacementUnbound.note(format!(
                "CylinderPrimitive {} retains its native placement because no unique checked version-9 placement operand binds its references.",
                scope.id
            )));
        }
    }
    losses
}

#[cfg(test)]
mod tests;
