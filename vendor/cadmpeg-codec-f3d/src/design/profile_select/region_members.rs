// SPDX-License-Identifier: Apache-2.0
//! Resolve counted partial-curve member rings to atomic regions.

use super::{sketch_profile_member_entity, ScopedExtrudeProfileResolution};
use crate::records::sketch_geometry::SketchCurveIdentity;
use crate::records::topology::sketch_profile::DesignSketchProfileOperand;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::PlanarProfileRef;
use cadmpeg_ir::sketches::{Sketch, SketchEntity, SketchEntityUse};

pub(super) fn resolve(
    stream: &str,
    profile: &DesignSketchProfileOperand,
    sketch: &Sketch,
    curves: &[SketchCurveIdentity],
    entities: &[SketchEntity],
    resolution: ScopedExtrudeProfileResolution<'_>,
) -> Result<Option<PlanarProfileRef>, CodecError> {
    let Some(selection) = &profile.region_selection else {
        return Ok(None);
    };
    let Ok(owner) = u32::try_from(profile.entity_id.suffix()) else {
        return Ok(None);
    };
    let mut rings = Vec::new();
    for region in &selection.regions {
        let mut members = Vec::new();
        for member in &region.members {
            let Some(entity) =
                sketch_profile_member_entity(stream, owner, member, sketch, curves, entities)
            else {
                return Ok(None);
            };
            members.push(SketchEntityUse {
                entity: entity.clone(),
                reversed: member.incidence_flag,
            });
        }
        rings.push(members);
    }
    let Some(selected) = crate::design::geometry::arrangement_regions_matching_member_rings(
        sketch,
        entities,
        &rings,
        resolution.linear_tolerance,
        resolution.profile_work,
        resolution.ctx,
    )?
    else {
        return Ok(None);
    };
    let profile = PlanarProfileRef::sketch_regions(sketch.id.clone(), selected.regions).ok();
    if profile.is_some() {
        if selected.coincident {
            resolution.profile_work.note_coincident_direction_assumed();
        }
        if selected.island_absence {
            resolution.profile_work.note_island_absence_assumed();
        }
    }
    Ok(profile)
}
