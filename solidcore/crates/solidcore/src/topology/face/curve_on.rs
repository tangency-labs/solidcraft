//! Whether a curve lies on a face (not upstream, #16713): on the face's surface, over the whole
//! of its interval, and inside the face's trim. The test of a section a split makes, which must
//! be a piece of the face it was cut by.
//!
//! The curve is on the surface by the kernel's simple coincidence check
//! ([`crate::surface::surface::Surface::simple_coincidence_checker`], the one the B-rep's
//! coincidence validator runs on an edge and a face), with every sampled point of the interval
//! within the tolerance of the surface and the curve tangent to it there. It is inside the trim
//! by the kernel's classification of each part of the interval against the face's boundary
//! (`classify_interval_on_face`): each part is in the face or on
//! an edge of it, none outside.

use crate::curve::curve::Curve;
use crate::error::{Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::solution_array::SolutionType;
use crate::intersect::point_class::PointClassType;
use crate::kernel::Kernel;
use crate::query::brep::BrepFaceServices;
use crate::topology::brep_cache::CacheObject;
use crate::topology::face::{p1, p2};
use crate::topology::{Brep, FaceId};

const OP: &str = "topology::face::curve_on";

/// Whether `curve` over `interval` lies on face `f` of `b` within `tol` (3D): every sampled
/// point of the interval on the face's surface with the curve tangent to it, and the whole
/// interval inside the face's trim loops or on their edges. A curve with a part off the
/// surface or outside the trim is `false`, not an error. The classification may add UV trim
/// curves to `b`.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a face without a surface or for a `tol` that is not finite
/// and positive; [`SolidError::NotFound`] for a dangling id; the kernel's errors from the
/// evaluations and the classification.
pub fn curve_lies_on_face(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    curve: &Curve,
    interval: Extent1d,
    tol: f64,
) -> Result<bool> {
    if !(tol.is_finite() && tol > 0.0) {
        return Err(SolidError::invalid_input(
            OP,
            "the tolerance is not positive",
        ));
    }
    let domain = p1::calculate_uv_domain_from_uv_trim_curves(k, b, f, true, false, false)?.domain;
    let Some(surface) = b.face(f)?.surface.as_ref() else {
        return Err(SolidError::invalid_input(OP, "the face has no surface"));
    };
    let surface = surface.clone_for_evaluation()?;
    // Full coincidence (no partial), so a part of the interval off the surface is no. The
    // face's own surface: its trimmed-surface services come from the B-rep.
    let found = {
        let mut services = BrepFaceServices::new(b);
        surface.simple_coincidence_checker_with_context(
            k,
            &domain,
            curve,
            &interval,
            tol,
            false,
            [None::<CacheObject>, Some(CacheObject::Surface(f))],
            None,
            &mut services,
        )?
    };
    let Some(found) = found else {
        return Ok(false);
    };
    if found.solution_type != SolutionType::RangeOfValues
        || found.start.solution_value > tol
        || found.end.solution_value > tol
    {
        return Ok(false);
    }
    Ok(
        p2::classify_interval_on_face(k, b, f, curve, interval, tol)?
            .iter()
            .all(|class| matches!(class, PointClassType::Face | PointClassType::Edge)),
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::curve::line::Line;
    use crate::math::Point3;
    use crate::primitive::api_primitives::create_box;
    use crate::topology::brep::p1 as brep1;

    /// The faces of a 10 by 20 by 30 box at the origin that the segment `p` to `q` lies on.
    fn faces_holding(p: [f64; 3], q: [f64; 3]) -> usize {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 20.0, 30.0).unwrap();
        let line = Curve::from(
            Line::create_line_segment(
                3,
                Point3::new(p[0], p[1], p[2]),
                Point3::new(q[0], q[1], q[2]),
                None,
            )
            .unwrap(),
        );
        let interval = line.natural_interval();
        brep1::faces(&b, None)
            .unwrap()
            .into_iter()
            .filter(|f| curve_lies_on_face(&k, &mut b, *f, &line, interval, 1.0e-6).unwrap())
            .count()
    }

    #[test]
    fn a_segment_across_a_face_lies_on_that_face_only() {
        assert_eq!(faces_holding([2.0, 3.0, 30.0], [8.0, 15.0, 30.0]), 1);
        assert_eq!(faces_holding([0.0, 5.0, 10.0], [0.0, 15.0, 20.0]), 1);
    }

    #[test]
    fn a_segment_off_the_surface_or_past_the_trim_lies_on_no_face() {
        assert_eq!(
            faces_holding([2.0, 3.0, 29.0], [8.0, 15.0, 29.0]),
            0,
            "inside the box"
        );
        assert_eq!(
            faces_holding([2.0, 3.0, 31.0], [8.0, 15.0, 31.0]),
            0,
            "above the box"
        );
        assert_eq!(
            faces_holding([-5.0, 3.0, 30.0], [8.0, 15.0, 30.0]),
            0,
            "on the top's plane, running past its edge"
        );
        assert_eq!(
            faces_holding([2.0, 3.0, 29.0], [8.0, 15.0, 30.0]),
            0,
            "tilted off the top"
        );
    }

    #[test]
    fn a_tolerance_that_is_not_positive_is_refused() {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 1.0, 1.0, 1.0).unwrap();
        let line = Curve::from(
            Line::create_line_segment(
                3,
                Point3::new(0.2, 0.2, 1.0),
                Point3::new(0.8, 0.8, 1.0),
                None,
            )
            .unwrap(),
        );
        let f = brep1::faces(&b, None).unwrap()[0];
        assert!(curve_lies_on_face(&k, &mut b, f, &line, line.natural_interval(), 0.0).is_err());
    }
}
