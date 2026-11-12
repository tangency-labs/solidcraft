//! A planar profile extruded along a direction as given (any length): straight, by the
//! linear sweep, or drafted, by the taper extrusion, whose height's sign picks the side
//! of the profile's plane the direction points to.
//!
//! Not upstream: SolidCraft's extrude entry (the direction made unit, the drafted
//! profile checked to be one closed loop square to it). Deviations: none (no upstream
//! counterpart). No recursion.

use crate::curve::composite_curve::OffsetCornerType;
use crate::curve::curve::p3::compute_crvs_normal;
use crate::curve::curve::Curve;
use crate::curve::path::{closed_loop, profile_square_to};
use crate::error::{Result, SolidError};
use crate::geom::certified::{dot, unit, unit_error, Bounded};
use crate::geom::vector3d::Vec3;
use crate::kernel::Kernel;
use crate::sweep::api_brep::{create_linear_sweep, create_taper_extrude_with_corner, TaperExtrude};

/// The operation name of every refusal of [`extrude`]'s own checks.
pub const OP: &str = "sweep::extrude::extrude";

/// A draft: its angle (degrees; the sign picks the side, as
/// [`create_taper_extrude_with_corner`] takes it) and its corner rule.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Draft {
    /// The angle in degrees.
    pub angle_deg: f64,
    /// How the walls meet at a corner.
    pub corner: OffsetCornerType,
}

/// The tolerances a drafted profile is checked with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExtrudeTolerances {
    /// How far apart consecutive curves' ends (and the last's end and the first's
    /// start) may be for the profile to be one closed loop.
    pub loop_gap: f64,
    /// How far the profile may lie off the plane square to the direction, relative to
    /// its size (at least 1) ([`profile_square_to`]).
    pub parallel: f64,
}

/// `curves` extruded `distance` along `direction` (any length): straight without a
/// draft (the linear sweep), or drafted with one (the taper extrusion), capped at both
/// ends with `cap`. A drafted profile must be proved one closed loop (to
/// `tol.loop_gap`) lying in the plane square to the direction (every control point
/// within `tol.parallel` of the profile's size of it, [`profile_square_to`]); the height
/// is `distance` towards the side the direction points to, read from the sign of the
/// dot of [`compute_crvs_normal`]'s normal (as a given vector) with the direction, which
/// must be proved away from 0.
///
/// # Errors
///
/// `InvalidInput` (operation [`OP`]) for a direction without a usable length, a
/// distance not finite and positive, a drafted profile not proved one closed loop in
/// the plane square to the direction, or one whose normal is not proved along it; those of [`create_linear_sweep`],
/// [`compute_crvs_normal`] and [`create_taper_extrude_with_corner`].
pub fn extrude(
    k: &mut Kernel,
    curves: &[Curve],
    direction: Vec3,
    distance: f64,
    draft: Option<Draft>,
    cap: bool,
    tol: &ExtrudeTolerances,
) -> Result<TaperExtrude> {
    let Some(dir) = unit(direction) else {
        return Err(SolidError::invalid_input(
            OP,
            "a direction without a usable length",
        ));
    };
    if !(distance.is_finite() && distance > 0.0) {
        return Err(SolidError::invalid_input(
            OP,
            "a distance not finite and positive",
        ));
    }
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let Some(draft) = draft else {
        return create_linear_sweep(k, &refs, dir, distance, cap).map(TaperExtrude::Solid);
    };
    if !closed_loop(k, curves, tol.loop_gap)?.is_yes() {
        return Err(SolidError::invalid_input(
            OP,
            "the profile is not proved one closed loop",
        ));
    }
    if !profile_square_to(curves, dir, tol.parallel)?.is_yes() {
        return Err(SolidError::invalid_input(
            OP,
            "the profile is not proved square to the direction",
        ));
    }
    // Only the side: the taper extrusion's own normal, made unit and taken as a given
    // vector, must be proved along the direction one way or the other (its dot with it
    // away from 0 by a half), so its sign is the side.
    let Some(normal) = compute_crvs_normal(k, curves).ok().and_then(unit) else {
        return Err(SolidError::invalid_input(OP, "the profile has no normal"));
    };
    let along = dot(normal, dir, unit_error(), unit_error());
    let away = Bounded {
        value: along.value.abs(),
        ..along
    };
    if !away.above(0.5).is_yes() {
        return Err(SolidError::invalid_input(
            OP,
            "the profile's normal is not along the direction",
        ));
    }
    let height = if along.value > 0.0 {
        distance
    } else {
        -distance
    };
    create_taper_extrude_with_corner(
        k,
        &refs,
        height,
        draft.angle_deg,
        if cap { 3 } else { 0 },
        draft.corner,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::line::Line;
    use crate::geom::vector3d::Point3;
    use crate::topology::brep::p2::is_manifold_solid;

    const TOL: ExtrudeTolerances = ExtrudeTolerances {
        loop_gap: 1e-6,
        parallel: 1e-9,
    };

    fn square() -> Vec<Curve> {
        let p = [
            Point3::ZERO,
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(2.0, 2.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
        ];
        (0..4)
            .map(|i| Curve::from(Line::create_line_segment(3, p[i], p[(i + 1) % 4], None).unwrap()))
            .collect()
    }

    fn bounds(k: &Kernel, b: &crate::topology::Brep) -> (Point3, Point3) {
        let e = crate::query::api_queries::brep_bounding_box(k, b, true).unwrap();
        (e.min(), e.max())
    }

    #[test]
    fn a_long_direction_extrudes_the_distance() {
        let mut k = Kernel::default();
        let c = square();
        let TaperExtrude::Solid(b) =
            extrude(&mut k, &c, Vec3::new(0.0, 0.0, 7.0), 3.0, None, true, &TOL).unwrap()
        else {
            panic!()
        };
        assert!(is_manifold_solid(&b).unwrap());
        let (lo, hi) = bounds(&k, &b);
        assert!((lo - Point3::ZERO).length() < 1e-9);
        assert!((hi - Point3::new(2.0, 2.0, 3.0)).length() < 1e-9);
    }

    #[test]
    fn a_draft_goes_the_way_the_direction_points() {
        let mut k = Kernel::default();
        let draft = Draft {
            angle_deg: 10.0,
            corner: OffsetCornerType::LinearExtension,
        };
        for z in [5.0, -5.0] {
            let c = square();
            let TaperExtrude::Solid(b) = extrude(
                &mut k,
                &c,
                Vec3::new(0.0, 0.0, z),
                1.0,
                Some(draft),
                true,
                &TOL,
            )
            .unwrap() else {
                panic!()
            };
            let (lo, hi) = bounds(&k, &b);
            if z > 0.0 {
                assert!((hi.z - 1.0).abs() < 1e-9 && lo.z.abs() < 1e-9);
            } else {
                assert!((lo.z + 1.0).abs() < 1e-9 && hi.z.abs() < 1e-9);
            }
        }
    }

    #[test]
    fn a_draft_across_the_profile_or_an_open_profile_is_refused() {
        let mut k = Kernel::default();
        let draft = Some(Draft {
            angle_deg: 10.0,
            corner: OffsetCornerType::LinearExtension,
        });
        let c = square();
        let refused = |r: Result<TaperExtrude>| matches!(r, Err(SolidError::InvalidInput { operation, .. }) if operation == OP);
        assert!(refused(extrude(
            &mut k,
            &c,
            Vec3::new(0.0, 1e-3, 1.0),
            1.0,
            draft,
            true,
            &TOL
        )));
        assert!(refused(extrude(
            &mut k,
            &c[..3],
            Vec3::Z,
            1.0,
            draft,
            true,
            &TOL
        )));
        assert!(refused(extrude(
            &mut k,
            &c,
            Vec3::ZERO,
            1.0,
            None,
            true,
            &TOL
        )));
        assert!(refused(extrude(&mut k, &c, Vec3::Z, 0.0, None, true, &TOL)));
        // Across by 1e-6 radians: 1 - cos is 5e-13, within 1e-9, but the square's corners
        // lie up to 2.8e-6 off the plane square to the direction.
        assert!(refused(extrude(
            &mut k,
            &c,
            Vec3::new(0.0, 1e-6, 1.0),
            1.0,
            draft,
            true,
            &TOL
        )));
        assert!(profile_square_to(&c, Vec3::Z, 1e-9).unwrap().is_yes());
    }
}
