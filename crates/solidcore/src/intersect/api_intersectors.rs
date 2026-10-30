//! `SM_API` intersectors: the section of a B-rep by a plane.
//!
//! Upstream: `source/SM_API/src/SmApiIntersectors.cpp`, `source/SM_API/inc/SmApiIntersectors.h`.
//!
//! | Rust                          | Upstream                       |
//! |-------------------------------|--------------------------------|
//! | [`intersect_brep_with_plane`] | `SmApiIntersectBrepWithPlane`  |
//!
//! Deviations: `SmApiGetOrCreateContext()` is the caller's [`Kernel`]. The B-rep is a
//! reference, so upstream's `SM_ERR_INVALID_INPUT` for a null B-rep has no
//! counterpart. The section curves are returned; upstream appends them to the caller's
//! array (keeping the curves already in it), which the caller does with them.
//! `SM_ASSERT_VALID` and the `SM_DEBUG_CODE` drawing are not ported.

use crate::curve::curve::Curve;
use crate::error::{Result, SolidError};
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::topology::brep::planar_section::{
    create_planar_section_curves, PlanarSectionOptions, SectionOutputs,
};
use crate::topology::Brep;

const OP: &str = "intersect::api_intersectors::intersect_brep_with_plane";

/// `SmApiIntersectBrepWithPlane`: the section curves of `b` by the infinite plane
/// through `plane_point` with normal `plane_normal` (any length; it is unitized):
/// `SmBrep::CreatePlanarSectionCurves` with the default tolerances, 3D curves only.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] (`SM_ERR_INVALID_INPUT`) for a zero normal; those of
/// [`create_planar_section_curves`].
pub fn intersect_brep_with_plane(
    k: &Kernel,
    b: &mut Brep,
    plane_point: Point3,
    plane_normal: Vec3,
) -> Result<Vec<Curve>> {
    let unit_normal = plane_normal
        .unitize()
        .map_err(|_| SolidError::invalid_input(OP, "zero plane normal"))?;

    // The kernel resets the array it is given, so collect into a local one (the caller
    // appends it, keeping any curves already in its array).
    let section = create_planar_section_curves(
        k,
        b,
        plane_point,
        unit_normal,
        &PlanarSectionOptions {
            outputs: SectionOutputs {
                curves_3d: true,
                ..SectionOutputs::default()
            },
            ..PlanarSectionOptions::default()
        },
    )?;
    Ok(section.curves_3d.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_normal_is_invalid_input_and_length_does_not_matter() {
        let mut k = Kernel::default();
        let mut b =
            crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 5.0, 10.0, 15.0)
                .unwrap();
        let at = Point3::new(0.0, 0.0, 7.0);
        let r = intersect_brep_with_plane(&k, &mut b, at, Vec3::ZERO);
        assert!(matches!(r, Err(SolidError::InvalidInput { .. })));
        let unit = intersect_brep_with_plane(&k, &mut b, at, Vec3::Z).unwrap();
        let long = intersect_brep_with_plane(&k, &mut b, at, Vec3::new(0.0, 0.0, 5.0)).unwrap();
        assert_eq!(unit.len(), 4);
        assert_eq!(long.len(), unit.len());
    }
}
