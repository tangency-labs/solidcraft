//! The sweeps family's kernel calls on numbers as an application gives them: a revolution
//! about an axis, a projection along a direction and a section square to a normal, each
//! vector of any length, and a line segment whose ends must be proved apart.
//!
//! Not upstream: SolidCraft's sweep entries made the vectors unit and compared the ends
//! themselves; that is geometry, so it is the kernel's ([`unit`], [`distance`]), and the
//! application layer only passes the numbers on. Each refusal of a number here is
//! `InvalidInput` with this module's operation name ([`OP`]), so a caller tells it from
//! the refusals of the calls it makes. Deviations: none (no upstream counterpart). No
//! recursion.

use crate::curve::curve::Curve;
use crate::curve::line::Line;
use crate::error::{Result, SolidError};
use crate::geom::certified::{distance, unit};
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::sweep::api_brep::create_rotational_sweep;
use crate::topology::brep::p1::{
    create_parallel_projection_curves, ParallelProjectionCurves, ParallelProjectionOptions,
};
use crate::topology::brep::planar_section::{
    create_planar_section_curves, PlanarSection, PlanarSectionOptions,
};
use crate::topology::Brep;

/// The operation name of every refusal of this module's own checks.
pub const OP: &str = "sweep::directed";

/// `v` made unit ([`unit`]), or `InvalidInput` naming `what` for a vector not finite or
/// not longer than `1e-12`.
fn direction(v: Vec3, what: &'static str) -> Result<Vec3> {
    unit(v).ok_or_else(|| SolidError::invalid_input(OP, what))
}

/// The bounded segment from `start` to `end` (`CreateLineSegment` over `[0, 1]`), its ends
/// proved more than `apart` apart: their certified distance is above `apart` beyond its
/// rounding bound.
///
/// # Errors
///
/// `InvalidInput` ([`OP`]) for ends not proved more than `apart` apart (a NaN among them
/// too); those of [`Line::create_line_segment`].
pub fn segment_apart(start: Point3, end: Point3, apart: f64) -> Result<Line> {
    if !distance(end, start).above(apart).is_yes() {
        return Err(SolidError::invalid_input(
            OP,
            "the segment's ends are not proved apart",
        ));
    }
    Line::create_line_segment(3, start, end, None)
}

/// [`create_rotational_sweep`] about the axis through `base_pt` along `axis` (any
/// length), made unit here.
///
/// # Errors
///
/// `InvalidInput` ([`OP`]) for an axis not finite or not longer than `1e-12`; those of
/// [`create_rotational_sweep`].
pub fn revolve(
    k: &mut Kernel,
    curves: &[Option<&Curve>],
    base_pt: Point3,
    axis: Vec3,
    angle_deg: f64,
    cap_ends: bool,
) -> Result<Brep> {
    let axis = direction(axis, "an axis not finite or not longer than 1e-12")?;
    create_rotational_sweep(k, curves, base_pt, axis, angle_deg, cap_ends)
}

/// [`create_parallel_projection_curves`] along `direction` (any length), made unit here.
///
/// # Errors
///
/// `InvalidInput` ([`OP`]) for a direction not finite or not longer than `1e-12`; those
/// of [`create_parallel_projection_curves`].
pub fn project_along(
    k: &Kernel,
    b: &mut Brep,
    curve: &Curve,
    direction_: Vec3,
    opts: &ParallelProjectionOptions<'_>,
) -> Result<ParallelProjectionCurves> {
    let dir = direction(
        direction_,
        "a projection direction not finite or not longer than 1e-12",
    )?;
    create_parallel_projection_curves(k, b, curve, dir, opts)
}

/// [`create_planar_section_curves`] by the plane through `plane_point` square to
/// `plane_normal` (any length), made unit here.
///
/// # Errors
///
/// `InvalidInput` ([`OP`]) for a normal not finite or not longer than `1e-12`; those of
/// [`create_planar_section_curves`].
pub fn section_square_to(
    k: &Kernel,
    b: &mut Brep,
    plane_point: Point3,
    plane_normal: Vec3,
    opts: &PlanarSectionOptions,
) -> Result<PlanarSection> {
    let normal = direction(
        plane_normal,
        "a plane normal not finite or not longer than 1e-12",
    )?;
    create_planar_section_curves(k, b, plane_point, normal, opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused_here(r: Result<impl Sized>) -> bool {
        matches!(r, Err(SolidError::InvalidInput { operation, .. }) if operation == OP)
    }

    /// A segment of length 5 (3-4-0) is made with ends where given; ends 1e-10 apart, the
    /// same point and a NaN are refused for `apart` 1e-9.
    #[test]
    fn a_segment_needs_its_ends_proved_apart() {
        let a = Point3::new(1.0, 2.0, 3.0);
        let b = Point3::new(4.0, 6.0, 3.0);
        let l = segment_apart(a, b, 1e-9).unwrap();
        assert_eq!(l.line_point(), a);
        assert!(refused_here(segment_apart(
            a,
            Point3::new(1.0 + 1e-10, 2.0, 3.0),
            1e-9
        )));
        assert!(refused_here(segment_apart(a, a, 1e-9)));
        assert!(refused_here(segment_apart(
            a,
            Point3::new(f64::NAN, 0.0, 0.0),
            1e-9
        )));
    }

    /// A direction of length 5 is made unit; zero, too short and NaN directions are
    /// refused with this module's operation, before any kernel call.
    #[test]
    fn a_direction_is_made_unit_or_refused() {
        let d = direction(Vec3::new(3.0, 4.0, 0.0), "x").unwrap();
        assert!(
            (d - Vec3::new(0.6, 0.8, 0.0)).length() <= 4.0 * f64::EPSILON,
            "{d:?}"
        );
        for v in [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1e-13, 0.0, 0.0),
            Vec3::new(f64::NAN, 1.0, 0.0),
        ] {
            assert!(refused_here(direction(v, "x")), "{v:?}");
        }
        let mut k = Kernel::default();
        let mut b = Brep::new_with_infinite_region().unwrap();
        assert!(refused_here(revolve(
            &mut k,
            &[],
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 0.0),
            90.0,
            false
        )));
        assert!(refused_here(section_square_to(
            &k,
            &mut b,
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(f64::INFINITY, 0.0, 0.0),
            &PlanarSectionOptions::default()
        )));
    }
}
