//! Build and check the solids and planar intersections used to certify a reconstruction.
//!
//! All constructions call the kernel's existing planar-face, sweep, and boolean operations.
//! Deviations: no upstream counterpart. No recursion.

use crate::boolean::api_brep::{boolean, boolean_2d, Boolean2dOutcome};
use crate::eval::Frame;
use crate::geom::a_object::BooleanOperation;
use crate::kernel::Kernel;
use crate::mass::integrator::properties::{brep_volume_default, no_unsure_region};
use crate::primitive::api_trim_ops::create_planar_faces;
use crate::primitive::primitive_creation::Boolean2d;
use crate::prism::extrude::extrude_region;
use crate::prism::profile::{check_regions, curve_on_frame, Region2};
use crate::prism::sheet::sheet_regions;
use crate::topology::brep::is_outward_solid;
use crate::{Brep, Result, SolidError};

const PROFILE_TOLERANCE: f64 = 1e-7;
const VOLUME_ACCURACY: f64 = 1e-8;

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input("reconstruct mesh", reason)
}

/// Extrude the already certified disjoint regions and their holes on their recovered frame.
pub(super) fn extrude(
    k: &mut Kernel,
    frame: &Frame,
    regions: &[Region2],
    height: f64,
) -> Result<Brep> {
    check_regions(regions.iter(), PROFILE_TOLERANCE)
        .map_err(|_| invalid("invalid reconstructed profile"))?;
    // These profiles contain fitted circular arcs, certified before the sweep. Use the
    // kernel's existing exact cylinder construction for those arcs, as Convert Mesh did
    // before the move. Restore the caller's sweep settings even when the sweep refuses.
    let fixes = k.fixes;
    // The arc-zero test is already unconditional. The remaining fix can be retired once
    // its cylinder construction is the default too.
    if let Some(arc_is_cylinder) = k.fixes.flag_mut("linear_sweep_arc_is_cylinder") {
        *arc_is_cylinder = true;
    }
    let result = (|| {
        let mut regions = regions.iter();
        let first = regions
            .next()
            .ok_or_else(|| invalid("empty reconstructed profile"))?;
        let mut body = extrude_region(k, frame, first, 0., height)?;
        for region in regions {
            let part = extrude_region(k, frame, region, 0., height)?;
            body = union(k, body, part)?;
        }
        Ok(body)
    })();
    k.fixes = fixes;
    result
}

/// The area the two profiles share in their own plane, using the exact planar Boolean.
pub(super) fn intersection_area(
    k: &mut Kernel,
    a: &Region2,
    b: &Region2,
    tolerance: f64,
) -> Result<f64> {
    check_regions([a, b].into_iter(), tolerance)
        .map_err(|_| invalid("invalid profile for mesh loop certification"))?;
    let frame = Frame {
        origin: [0., 0., 0.],
        x_axis: [1., 0., 0.],
        y_axis: [0., 1., 0.],
    };
    let mut face = |region: &Region2| {
        let curves = region
            .loops()
            .flat_map(|lp| &lp.segs)
            .map(|seg| curve_on_frame(k, &frame, 0., seg))
            .collect::<Result<Vec<_>>>()?;
        create_planar_faces(k, &curves)
    };
    let sa = face(a)?;
    let sb = face(b)?;
    match boolean_2d(k, sa, sb, Boolean2d::Intersection)? {
        Boolean2dOutcome::Unassigned { .. } => Ok(0.),
        Boolean2dOutcome::Result(overlap) if overlap.face_list().is_empty() => Ok(0.),
        Boolean2dOutcome::Result(overlap) => sheet_regions(k, &overlap, &frame, tolerance)?
            .regions
            .iter()
            .try_fold(0., |area, region| Ok(area + region.signed_area()?.value)),
    }
}

/// Join two reconstructed solids through the kernel's B-rep Boolean.
pub(super) fn union(k: &mut Kernel, a: Brep, b: Brep) -> Result<Brep> {
    let body = boolean(k, a, b, BooleanOperation::Union)?.result;
    if body.face_list().is_empty() {
        return Err(invalid("empty reconstructed part union"));
    }
    Ok(body)
}

pub(super) fn volume(k: &mut Kernel, body: &mut Brep) -> Result<f64> {
    no_unsure_region(body)?;
    brep_volume_default(k, body, VOLUME_ACCURACY)
}

pub(super) fn is_valid(k: &mut Kernel, body: &mut Brep) -> Result<bool> {
    is_outward_solid(k, body)
}
