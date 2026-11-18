//! WR-704 (the SolidCraft fork's tangency-labs/smlib@1279539): `SmCone::CreateIsoCircleFromPoint`
//! on a cone whose apex is its lower end (bottom radius 0, semi-angle 70, height 1). A
//! reference point up to the tolerance below the apex passes the height test, but moved onto
//! the cone at its height it lies on the far nappe; upstream's inversion mirrors it through
//! the apex and succeeds with a height outside the cone, and the iso curve there fails with
//! `SM_ERR`. `SmSphere::IntersectWithCone` passes such a point for a sphere on the axis
//! crossing the cone's line just past the apex, so that intersection fails too. The port
//! (the flag `cone_iso_circle_at_axial_height` until its retirement) puts the circle at the
//! point's height clamped to the cone, here the apex, and inside the cone at the point's
//! height.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tol::approx_tol3d;
use solidcore::math::{Point3, Vec3};
use solidcore::surface::cone::Cone;
use solidcore::surface::sphere::{Sphere, SphereOptions};
use solidcore::surface::surface::Surface;
use solidcore::Kernel;

const TOL: f64 = 1e-3;
const SEMI: f64 = 70.0;

fn kernel() -> Kernel {
    Kernel::default()
}

fn placement() -> Axis2Placement {
    Axis2Placement::new(
        Point3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
    )
}

/// The cone with its apex at the origin, its lower end, opening up the Z axis to height 1.
fn apex_cone(k: &Kernel) -> Cone {
    let top = SEMI.to_radians().tan();
    Cone::create_canonical(k, &placement(), 0.0, top, 1.0, false, false).unwrap()
}

/// The height of `cone`'s iso circle through `p` (`None`: no circle), or its error code.
fn circle_height(k: &Kernel, cone: &Cone, p: Point3) -> Result<Option<f64>, i32> {
    match cone.create_iso_circle_from_point(k, p, TOL) {
        Ok(Some(c)) => Ok(Some(
            c.evaluate_point(k, c.natural_interval().min()).unwrap().z,
        )),
        Ok(None) => Ok(None),
        Err(e) => match e.sm_status() {
            Some(code) if !e.is_port_failure() => Err(code),
            _ => panic!("unexpected error {e:?}"),
        },
    }
}

#[test]
fn point_past_the_apex_gives_the_apex_circle() {
    for z in [-1e-6, -5e-4, -9e-4] {
        for (f, off) in [(1.0, 0.0), (0.5, 0.0), (1.0, 0.3), (0.0, 0.0)] {
            let k = kernel();
            let cone = apex_cone(&k);
            let r = cone.radius(z) * f + off;
            let p = Point3::new(0.6 * r, 0.8 * r, z);
            let h = circle_height(&k, &cone, p).unwrap().unwrap();
            assert!(h.abs() < 1e-12, "z {z}, p {p:?}: height {h}");
        }
    }
}

/// Inside the cone the circle is at the point's height, and within the tolerance past the
/// top at the top (the height clamped to [0, 1]); past the tolerance there is none.
#[test]
fn circles_inside_and_past_the_top_are_at_the_clamped_height() {
    for (bottom, swap_uv, inside_out) in [
        (0.0, false, false),
        (0.5, false, false),
        (0.5, true, false),
        (0.5, false, true),
        (0.5, true, true),
    ] {
        for z in [0.0, 1e-3, 0.25, 0.5, 0.999, 1.0, 1.0005, 1.0009, 1.002] {
            let k = kernel();
            let cone = Cone::create_canonical_with_semi_angle(
                &k,
                &placement(),
                bottom,
                SEMI,
                Some(1.0),
                swap_uv,
                inside_out,
            )
            .unwrap();
            let r = cone.radius(z);
            let got = circle_height(&k, &cone, Point3::new(0.6 * r, 0.8 * r, z));
            let what = format!("bottom {bottom} swap {swap_uv} io {inside_out} z {z}");
            if z > 1.0 + TOL {
                assert_eq!(got, Ok(None), "{what}");
            } else {
                let h = got.unwrap().unwrap();
                let want = z.clamp(0.0, 1.0);
                assert!((h - want).abs() < 1e-12, "{what}: height {h}, want {want}");
            }
        }
    }
}

/// The fork's capi test `sphere_cone_top_circle` at the other end: a sphere centred on the
/// axis at (0, 0, 2) through the point 2 tol past the apex along the cone's line, so the apex
/// is within tol of the sphere. The line's other crossing is the circle at height
/// 4 cos² 70 + 2 tol cos 70.
#[test]
fn sphere_crossing_just_past_the_apex() {
    let (s, c) = (SEMI.to_radians().sin(), SEMI.to_radians().cos());
    let centre = Point3::new(0.0, 0.0, 2.0);
    let past = Point3::new(-2.0 * TOL * s, 0.0, -2.0 * TOL * c);
    let radius = past.distance(centre);
    let want = 4.0 * c * c + 2.0 * TOL * c;
    {
        let k = kernel();
        let sphere = Surface::from(
            Sphere::new(
                &k,
                centre,
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Extent2d::from_bounds(0.0, -90.0, 360.0, 90.0),
                radius,
                SphereOptions::default(),
            )
            .unwrap(),
        );
        let cone = Surface::from(apex_cone(&k));
        let d1 = sphere.natural_uv_domain_with_nurb(&k).unwrap();
        let d2 = cone.natural_uv_domain_with_nurb(&k).unwrap();
        let (x, status) = sphere.global_surface_intersect_with_partial(
            &k,
            &d1,
            &cone,
            &d2,
            [true, true],
            Some(approx_tol3d(&k, TOL)),
            None,
        );
        status.unwrap();
        let mut at_want = false;
        for curve in &x.curves_3d {
            let ivl = curve.natural_interval();
            for i in 0..=32 {
                let t = ivl.min() + (ivl.max() - ivl.min()) * f64::from(i) / 32.0;
                let q = curve.evaluate_point(&k, t).unwrap();
                let on_sphere = (q.distance(centre) - radius).abs();
                let on_cone = ((q.x * q.x + q.y * q.y).sqrt() * c - q.z * s).abs();
                assert!(
                    on_sphere < 2.0 * TOL && on_cone < 2.0 * TOL,
                    "{q:?} off the surfaces: {on_sphere:e}, {on_cone:e}"
                );
            }
            let z = curve.evaluate_point(&k, ivl.min()).unwrap().z;
            at_want |= (z - want).abs() < TOL;
        }
        assert!(
            at_want,
            "no circle at height {want}: {} curves",
            x.curves_3d.len()
        );
    }
}

/// An unbounded cone of radius 1 whose semi-angle, 5e-11 degrees, is just above the cylinder
/// test, with swapped parameters, and a point at height 0.7. Upstream's circle, built from the
/// inverted STEP point, is at height 3.95 (as the pinned C++, difftest `surface.cone_p1`
/// `r4_09393_*`). The port's, from the sweep start and the clamped height, is at the point's
/// height to within the NURB's height mapping on this near-cylinder (8.8e-5).
#[test]
fn near_cylinder_swapped_circle_is_at_the_point_height() {
    let p = Point3::new(-1.0, 1e-13, 0.7);
    let k = kernel();
    let cone =
        Cone::create_canonical_with_semi_angle(&k, &placement(), 1.0, 5e-11, None, true, false)
            .unwrap();
    let circle = cone
        .create_iso_circle_from_point(&k, p, 1e-9)
        .unwrap_or_else(|e| panic!("{e:?}"))
        .unwrap_or_else(|| panic!("no circle"));
    let start = circle
        .evaluate_point(&k, circle.natural_interval().min())
        .unwrap();
    let off = (start.z - 0.7).abs();
    assert!(off < 1e-3, "circle at height {}", start.z);
    let radius = (start.x * start.x + start.y * start.y).sqrt();
    assert!((radius - 1.0).abs() < 1e-5, "radius {radius}");
}
