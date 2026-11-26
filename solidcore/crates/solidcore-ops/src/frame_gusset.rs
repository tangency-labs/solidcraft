//! Attached gusset profiles: choose the support material, check both contact strips,
//! and return the kernel's exact outline and extrusion levels. All coordinate
//! calculations are in `solidcore::prism::gusset`; this layer converts and sequences.

use solidcore::math::Transform;
use solidcore::prism::gusset as geometry;
use solidcore_convert as convert;
use solidcore_record::{Plane, Region2, Seg2, Vec3};

use crate::error::status;
use crate::{BooleanOp, Error, Result};

pub use geometry::Dimensions;
pub use geometry::Placement;

/// The frames and tolerances of resolved persistent support faces.
pub struct SupportFrames {
    /// Support plane in the plate's author frame.
    pub plane: Plane,
    /// Common plane in the source body's frame.
    pub source: Plane,
    /// Author coordinates to source coordinates.
    pub to_source: [[f64; 4]; 4],
    /// Source coordinates to author coordinates.
    pub to_author: [[f64; 4]; 4],
    /// Planar boolean tolerance in millimetres.
    pub tolerance: f64,
}

/// Exact support material with its captured placement.
pub struct Support(geometry::Face);

/// The plate's exact planar region and extrusion levels.
pub struct Prepared {
    /// Profile frame in author coordinates.
    pub plane: Plane,
    /// Closed plate outline.
    pub profile: Region2,
    /// Lower extrusion level.
    pub start: f64,
    /// Upper extrusion level.
    pub end: f64,
}

fn refused(reason: &'static str) -> Error {
    Error::Refused {
        operation: "frame_gusset",
        reason,
    }
}

/// Select the common source plane, refusing non-coplanar split faces.
///
/// # Errors
/// The kernel's refusal when the candidate is not on the first face's plane.
pub fn support_plane(
    first: Option<&Plane>,
    candidate: &Plane,
    size: f64,
    tolerance: f64,
) -> Result<Plane> {
    if let Some(first) = first {
        geometry::same_plane(
            &convert::plane(first),
            &convert::plane(candidate),
            size,
            tolerance,
        )
        .map_err(status)?;
        Ok(*first)
    } else {
        Ok(*candidate)
    }
}

/// Reconstruct each split face's material from its trimmed edge pieces, keeping
/// holes and joining split faces by the existing exact planar Boolean operation.
///
/// # Errors
/// Malformed, open or over-budget loops; empty material; the planar Boolean's error.
pub fn support(frames: &SupportFrames, faces: &[Vec<Vec<Seg2>>]) -> Result<Support> {
    if faces.len() > 4096 {
        return Err(refused("gusset support exceeds the 4096-face limit"));
    }
    let mut regions = Vec::new();
    for face in faces {
        if face.len() > 4096 {
            return Err(refused("gusset support face has too many loops"));
        }
        let mut loops = Vec::new();
        for pieces in face {
            if pieces.len() > 65536 {
                return Err(refused("gusset support face has too many edge pieces"));
            }
            let pieces = pieces.iter().map(convert::seg2).collect();
            let lp = geometry::chain(pieces, frames.tolerance).map_err(status)?;
            loops.push(geometry::counter_clockwise(lp).map_err(status)?);
        }
        let areas = loops
            .iter()
            .map(geometry::loop_signed_area)
            .collect::<solidcore::Result<Vec<_>>>()
            .map_err(status)?;
        let outer = areas
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i)
            .ok_or_else(|| refused("gusset support face has no boundary"))?;
        // `outer` came from this nonempty list, in exactly the same order as `loops`.
        let material = Region2 {
            outer: convert::record_loop2(&loops.swap_remove(outer)),
            holes: Vec::new(),
        };
        let holes: Vec<_> = loops
            .iter()
            .map(|outer| Region2 {
                outer: convert::record_loop2(outer),
                holes: Vec::new(),
            })
            .collect();
        let material = if holes.is_empty() {
            vec![material]
        } else {
            crate::prism::planar_regions_boolean(
                &[material],
                &holes,
                BooleanOp::Subtract,
                frames.tolerance,
            )?
        };
        regions = if regions.is_empty() {
            material
        } else {
            crate::prism::planar_regions_boolean(
                &regions,
                &material,
                BooleanOp::Union,
                frames.tolerance,
            )?
        };
    }
    if regions.is_empty() {
        return Err(refused("gusset support has no material"));
    }
    Ok(Support(geometry::Face {
        plane: convert::plane(&frames.plane),
        source: convert::plane(&frames.source),
        regions: regions.iter().map(convert::region2).collect(),
        to_source: Transform {
            m: frames.to_source,
        },
        to_author: Transform {
            m: frames.to_author,
        },
        tolerance: frames.tolerance,
    }))
}

/// Construct the plate outline and prove both contact strips fit on their
/// supports' material, including holes or notches between the strip corners.
///
/// # Errors
/// Invalid dimensions, unsupported support placement, incomplete contact, or the
/// underlying planar Boolean's error. No document state is modified.
pub fn prepare(
    supports: [&Support; 2],
    dimensions: Dimensions,
    placement: Placement,
) -> Result<Prepared> {
    let [a, b] = supports;
    let plan = geometry::plan([&a.0, &b.0], dimensions, placement).map_err(status)?;
    for (support, contact) in [a, b].into_iter().zip(&plan.contacts) {
        let strip = convert::record_region2(&contact.region);
        let regions: Vec<_> = support
            .0
            .regions
            .iter()
            .map(convert::record_region2)
            .collect();
        let left = crate::prism::planar_regions_boolean(
            &[strip],
            &regions,
            BooleanOp::Subtract,
            support.0.tolerance,
        )?;
        let left: Vec<_> = left.iter().map(convert::region2).collect();
        let uncovered = geometry::regions_area(&left).map_err(status)?;
        if !uncovered.is_finite() || uncovered > contact.maximum_uncovered {
            return Err(refused("gusset contact must fit entirely on both selected faces; reduce the legs/thickness or change the joint offset"));
        }
    }
    Ok(Prepared {
        plane: convert::record_plane(&plan.plane),
        profile: convert::record_region2(&plan.profile),
        start: plan.start,
        end: plan.end,
    })
}

/// Return a saved support pick on the current exact plane, for an edit dialog.
#[must_use]
pub fn support_pick(plane: &Plane, at: Vec3) -> Vec3 {
    convert::record_vec3(geometry::support_pick(
        &convert::plane(plane),
        convert::vec3(at),
    ))
}
