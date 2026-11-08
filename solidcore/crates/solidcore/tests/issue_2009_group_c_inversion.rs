//! #2009 group C: the analytic `STEPInversion` answers told apart from its failures,
//! behind `UpstreamFixes::analytic_inversion_answers_apart` (WR-662; the SolidCraft fork's
//! tangency-labs/smlib dfc7cb6). The fork's own test: the statuses and locations for
//! points on, off and outside a quarter cylinder and a sphere.

#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::panic)] // test helpers

use solidcore::error::{SM_ERR, SM_ERR_NOT_WITHIN_TOLERANCE, SM_ERR_OUTSIDE_OF_DOMAIN};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent2d::Extent2d;
use solidcore::math::{Point2, Point3, Vec3};
use solidcore::nurbs::b_spline_surface::LocationType;
use solidcore::surface::cone::Cone;
use solidcore::surface::sphere::Sphere;
use solidcore::surface::surface::Surface;
use solidcore::{Kernel, SolidError};

fn kernel(apart: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.analytic_inversion_answers_apart = apart;
    k
}

fn code<T: std::fmt::Debug>(r: &solidcore::Result<T>) -> i32 {
    match r {
        Err(SolidError::Upstream { code, .. }) => *code,
        other => panic!("expected an upstream status, got {other:?}"),
    }
}

/// A quarter cylinder of radius 2 and height 3 about Z, swept from 0 to 90 degrees.
fn quarter_cylinder() -> Surface {
    Surface::from(Cone::new_cylinder_bounded(
        Point3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        2.0,
        0.0,
        90.0,
        3.0,
        false,
        false,
        false,
    ))
}

fn any_domain() -> Extent2d {
    Extent2d::from_bounds(-720.0, -720.0, 720.0, 720.0)
}

/// fork dfc7cb6 (`SmSurfOfRevolution::STEPInversion`, `SmSurface::STEPInversionAnswered`):
/// on the quarter cylinder, a point on it inverts; off it (radius 3) is
/// `SM_ERR_NOT_WITHIN_TOLERANCE` and outside the sweep (180 degrees)
/// `SM_ERR_OUTSIDE_OF_DOMAIN`, both answers. Upstream returns `SM_ERR` for both, which
/// reads as a failure.
#[test]
fn quarter_cylinder_inversion_answers_apart_from_failures() {
    let s = 0.5_f64.sqrt();
    let on = Point3::new(2.0 * s, 2.0 * s, 1.5);
    let off = Point3::new(3.0 * s, 3.0 * s, 1.5);
    let outside = Point3::new(-2.0, 0.0, 1.5);
    for apart in [false, true] {
        let k = kernel(apart);
        let cyl = quarter_cylinder();
        let d = any_domain();
        let (uv, loc) = cyl
            .step_inversion(&k, &d, on, 1e-9, None)
            .expect("on the cylinder");
        assert!((uv.x - 45.0).abs() < 1e-9, "{uv:?}");
        assert_eq!(loc, LocationType::Interior);

        let r_off = cyl.step_inversion(&k, &d, off, 1e-9, None);
        let r_out = cyl.step_inversion(&k, &d, outside, 1e-9, None);
        if apart {
            assert_eq!(code(&r_off), SM_ERR_NOT_WITHIN_TOLERANCE);
            assert_eq!(code(&r_out), SM_ERR_OUTSIDE_OF_DOMAIN);
        } else {
            assert_eq!(code(&r_off), SM_ERR);
            assert_eq!(code(&r_out), SM_ERR);
        }
        assert_eq!(Surface::step_inversion_answered(&r_off), apart);
        assert_eq!(Surface::step_inversion_answered(&r_out), apart);
    }
}

/// fork dfc7cb6 (`SmSphere::STEPInversion`): a point off the sphere answers
/// `SM_ERR_NOT_WITHIN_TOLERANCE` with the parameters of the point dropped to it, as
/// does the centre of a sphere larger than the tolerance; upstream's `SM_ERR` reads
/// as a failure.
#[test]
fn sphere_inversion_answers_apart_from_failures() {
    for apart in [false, true] {
        let k = kernel(apart);
        let sphere = Sphere::create_canonical(&k, &Axis2Placement::default(), 2.0).expect("sphere");
        let d = any_domain();
        let surf = Surface::from(sphere.clone());

        let on = Point3::new(0.0, 2.0, 0.0);
        let (uv, loc) = surf.step_inversion(&k, &d, on, 1e-9, None).expect("on");
        assert!((uv.x - 90.0).abs() < 1e-9 && uv.y.abs() < 1e-9, "{uv:?}");
        assert_eq!(loc, LocationType::Interior);

        // off the sphere: the dropped point's parameters (latitude 45 degrees)
        let off = Point3::new(0.0, 3.0, 3.0);
        let inv = sphere.step_inversion(&d, off, 1e-9, None);
        assert!(!inv.on_surface && inv.answered);
        assert!(
            (inv.uv - Point2::new(90.0, 45.0)).length() < 1e-9,
            "{:?}",
            inv.uv
        );
        let r_off = surf.step_inversion(&k, &d, off, 1e-9, None);
        let r_centre = surf.step_inversion(&k, &d, Point3::new(0.0, 0.0, 0.0), 1e-9, None);
        let expected = if apart {
            SM_ERR_NOT_WITHIN_TOLERANCE
        } else {
            SM_ERR
        };
        assert_eq!(code(&r_off), expected);
        assert_eq!(code(&r_centre), expected);
        assert_eq!(Surface::step_inversion_answered(&r_off), apart);
    }
}

/// fork dfc7cb6 (`SmSphere::STEPInversion`): the centre of a sphere no larger than the
/// tolerance is on it but has no parameters: `SM_ERR`, a failure, for either setting.
#[test]
fn tiny_sphere_centre_is_a_failure() {
    for apart in [false, true] {
        let k = kernel(apart);
        let sphere =
            Sphere::create_canonical(&k, &Axis2Placement::default(), 1e-6).expect("sphere");
        let d = any_domain();
        let inv = sphere.step_inversion(&d, Point3::new(0.0, 0.0, 0.0), 1e-3, None);
        assert!(!inv.answered && !inv.on_surface);
        assert_eq!(inv.location, LocationType::Exterior);
        let r =
            Surface::from(sphere).step_inversion(&k, &d, Point3::new(0.0, 0.0, 0.0), 1e-3, None);
        assert_eq!(code(&r), SM_ERR);
        assert!(!Surface::step_inversion_answered(&r));
    }
}
