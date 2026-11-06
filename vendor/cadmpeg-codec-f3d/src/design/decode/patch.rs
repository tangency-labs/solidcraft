// SPDX-License-Identifier: Apache-2.0
//! Decode the boundary-settings record a `SurfacePatch` scope references once
//! per boundary component.

use super::sketch::IndexedRecordOffsets;
use crate::records::feature::surface_ops::{DesignPatchContinuity, DesignSurfacePatchBoundary};
use cadmpeg_core::decode::View;

/// The boundary-settings records a `SurfacePatch` scope references, in scope
/// reference order.
///
/// The settings record occupies one fixed ordinal per boundary component in
/// each settings-bearing `SurfacePatch` scope form. Every reference member is
/// offered to the record grammar and only the members it closes are kept. The
/// single-group path form carries no settings record and therefore yields none.
pub(super) fn surface_patch_boundaries(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    reference_members: &[u32],
) -> Vec<DesignSurfacePatchBoundary> {
    let mut boundaries = Vec::new();
    for (ordinal, record_index) in reference_members.iter().enumerate() {
        let Some(mut boundary) = records.first_at_or_after(0, *record_index).and_then(|at| {
            let end = records
                .frames(*record_index)
                .find_map(|(start, end)| (start == at).then_some(end))?;
            exact_surface_patch_boundary(bytes.get(..end)?, at)
        }) else {
            continue;
        };
        let Ok(ordinal) = u32::try_from(ordinal) else {
            continue;
        };
        boundary.scope_reference_ordinal = ordinal;
        boundary.record_index = *record_index;
        boundaries.push(boundary);
    }
    boundaries
}

/// One boundary-settings record read at the indexed header offset `at`.
///
/// The class level is two zero bytes, `u8 IsSeedSel`, `u32 PatchContinuity`,
/// `u32 PatchFlip`, `f64 PatchScale`, and the `rPatchModelRef` reference. The
/// base level's reference run closes the record and carries no settings.
fn exact_surface_patch_boundary(bytes: &[u8], at: usize) -> Option<DesignSurfacePatchBoundary> {
    use crate::layout::surface_patch_boundary_settings_prefix as layout;
    let frame = bytes.get(at..)?;
    let name_offset = layout::PROLOGUE.checked_sub(4)?;
    if View::u32_le_at(frame, name_offset)? != 0
        || frame.get(layout::PROLOGUE..layout::IS_SEED_SELECTION)? != [0; 2]
    {
        return None;
    }
    let is_seed_selection = match frame.get(layout::IS_SEED_SELECTION)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let scale = cadmpeg_ir::scalar::FiniteReal::new(View::f64_le_at(frame, layout::RAW_WEIGHT)?)?;
    let mut cursor = layout::LEN;
    let model_reference =
        u32::try_from(crate::bytes::take_reference(frame, &mut cursor)?.local()?.0).ok()?;
    Some(DesignSurfacePatchBoundary {
        scope_reference_ordinal: 0,
        record_index: 0,
        is_seed_selection,
        continuity: DesignPatchContinuity::from_code(View::u32_le_at(frame, layout::CONTINUITY)?),
        flip: View::u32_le_at(frame, layout::SIDE_ORDINAL)?,
        scale,
        model_reference,
    })
}

pub(super) fn surface_patch_normal(
    bytes: &[u8],
    scope: &crate::records::feature::scope::DesignParameterScope,
    type_info: Option<(&str, u32)>,
) -> Option<crate::records::feature::surface_ops::DesignSurfacePatchNormalState> {
    use crate::layout::surface_patch_v7_normal_flip_prefix as layout;
    use crate::records::feature::scope::DesignFeatureKind;
    use crate::records::feature::surface_ops::DesignSurfacePatchNormalState;
    use crate::records::identity::Located;
    if scope.kind() != DesignFeatureKind::SurfacePatch {
        return None;
    }
    let Some((guid, version)) = type_info else {
        return Some(DesignSurfacePatchNormalState::Unsupported);
    };
    if !guid.eq_ignore_ascii_case("BF77AE1A-E8F6-43AB-A325-A2C984AF1C6B") {
        return Some(DesignSurfacePatchNormalState::Unsupported);
    }
    match version {
        1 | 5 | 6 => return None,
        7 => {}
        _ => return Some(DesignSurfacePatchNormalState::Unsupported),
    }
    let admitted = || {
        let start = usize::try_from(scope.byte_offset()).ok()?;
        let end = usize::try_from(scope.paired_byte_offset()).ok()?;
        let frame = bytes.get(start..end)?;
        if View::u32_le_at(frame, layout::FOLLOWING_WORD) != Some(layout::FOLLOWING_WORD_VALUE) {
            return None;
        }
        let reversed = match *frame.get(layout::NORMAL_FLIP)? {
            0 => false,
            1 => true,
            _ => return None,
        };
        Some(DesignSurfacePatchNormalState::Explicit {
            reversed: Located {
                value: reversed,
                offset: scope
                    .byte_offset()
                    .checked_add(u64::try_from(layout::NORMAL_FLIP).ok()?)?,
            },
        })
    };
    Some(admitted().unwrap_or(DesignSurfacePatchNormalState::Unsupported))
}

#[cfg(test)]
mod tests;
