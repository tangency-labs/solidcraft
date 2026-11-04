// SPDX-License-Identifier: Apache-2.0
//! Resolve counted partial-curve members to one atomic region per member set.

use super::{sketch_profile_member_entity, ScopedExtrudeProfileResolution};
use crate::records::sketch_geometry::SketchCurveIdentity;
use crate::records::topology::sketch_profile::DesignSketchProfileOperand;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::PlanarProfileRef;
use cadmpeg_ir::sketches::{Sketch, SketchEntity};

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
    let mut regions = Vec::new();
    for region in &selection.regions {
        let mut members = Vec::new();
        for member in &region.members {
            let Some(entity) =
                sketch_profile_member_entity(stream, owner, member, sketch, curves, entities)
            else {
                return Ok(None);
            };
            members.push(entity.clone());
        }
        let Some(region) = crate::design::geometry::arrangement_region_matching_curve_members(
            sketch,
            entities,
            &members,
            resolution.linear_tolerance,
            resolution.profile_work,
            resolution.ctx,
        )?
        else {
            return Ok(None);
        };
        if regions.contains(&region) {
            return Ok(None);
        }
        regions.push(region);
    }
    Ok(PlanarProfileRef::sketch_regions(sketch.id.clone(), regions).ok())
}
