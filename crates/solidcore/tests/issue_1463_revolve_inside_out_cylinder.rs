//! Regression tests for tangency-labs/solidcore#1463: a revolve's inside-out cylinder
//! comes back from its NURBS with its axis reversed.
//!
//! Two defects combine (WR-496):
//! - `Cone::is_nurb_surface_cylinder` (`SmCylinder::IsNurbSurfaceCylinder`) keeps the
//!   bottom-knot circle and the NURB's linear direction for an inside-out NURB, so the
//!   recognised cylinder's Z is reversed and its STEP evaluation is mirrored in height.
//!   Upstream does the same; `fixes.nurb_cylinder_inside_out_frame` (opt-in) builds the
//!   frame as `is_nurb_surface_cone` does.
//! - With `fixes.revolve_analytic_profile_faces`, the revolve made an inside-out
//!   cylinder (or cone) for a profile line running against the axis. Fork commit
//!   tangency-labs/smlib 9853e54, which that flag ports, builds them from the low end
//!   upwards, never inside out.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use solidcore::surface::cone::Cone;
use solidcore::surface::surface::SurfaceKind;
use solidcore::sweep::api_brep::create_rotational_sweep;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::{Kernel, Point3, Vec3};

/// `SmCone::AssertValid` #3: `((P - foot) x dP/d(sweep)) . Z`, positive for a valid cone.
fn test3(c: &Cone) -> f64 {
    let can = c.canonical();
    let (o, z) = (can.origin.origin(), can.origin.z_axis());
    let base = c.rev().base();
    let uv = base.natural_uv_domain().evaluate(0.45678, 0.45678);
    let d = base.evaluate(uv, 1, 1, EvaluateFlags::default()).unwrap();
    let sweep = if can.swap_uv { d[0][1] } else { d[1][0] };
    (d[0][0] - (o + z * (d[0][0] - o).dot(z)))
        .cross(sweep)
        .dot(z)
}

/// The largest distance between the cone's STEP and NURB evaluations over its domain.
fn step_nurb_gap(k: &Kernel, c: &Cone) -> f64 {
    let domain = c.rev().base().natural_uv_domain();
    let mut worst: f64 = 0.0;
    for (s, t) in [(0.4567, 0.7654), (0.1, 0.9), (0.9, 0.1), (0.5, 0.5)] {
        let uv = domain.evaluate(s, t);
        let step = c.evaluate_point(k, uv).unwrap();
        let nurb = c.rev().base().evaluate_point(uv).unwrap();
        worst = worst.max((step - nurb).length());
    }
    worst
}

/// Radius 8 about +Z, height 22, its NURB's linear direction from the top (inside out).
fn inside_out_cylinder(k: &Kernel) -> Cone {
    let mut c = Cone::new_cylinder_bounded(
        Point3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        8.0,
        0.0,
        360.0,
        22.0,
        false,
        true,
        false,
    );
    c.make_nurb(k).unwrap();
    c
}

#[test]
fn issue_1463_inside_out_nurb_cylinder_keeps_its_axis() {
    let mut k = Kernel::default();
    let c = inside_out_cylinder(&k);
    let made = test3(&c);
    assert!(made > 0.0, "the made cylinder is valid: {made}");

    // the fix on: the cylinder it was given
    k.fixes.nurb_cylinder_inside_out_frame = true;
    let r = Cone::is_nurb_surface_cylinder(&k, c.rev().base(), 1.0)
        .unwrap()
        .unwrap();
    let can = r.canonical();
    assert!(r.inside_out());
    assert!(
        (can.origin.origin() - Vec3::ZERO).length() < 1e-12,
        "{:?}",
        can.origin.origin()
    );
    assert!(
        (can.origin.z_axis() - Vec3::Z).length() < 1e-12,
        "{:?}",
        can.origin.z_axis()
    );
    assert!((r.base_radius() - 8.0).abs() < 1e-12);
    assert!(
        (test3(&r) - made).abs() < 1e-9 * made,
        "{} {made}",
        test3(&r)
    );
    let gap = step_nurb_gap(&k, &r);
    assert!(gap < 1e-12, "STEP vs NURB {gap}");

    // the fix off: upstream's reversed axis from the bottom-knot circle (z = 22)
    k.fixes.nurb_cylinder_inside_out_frame = false;
    let r = Cone::is_nurb_surface_cylinder(&k, c.rev().base(), 1.0)
        .unwrap()
        .unwrap();
    let can = r.canonical();
    assert!(r.inside_out());
    assert!((can.origin.origin() - Vec3::new(0.0, 0.0, 22.0)).length() < 1e-12);
    assert!((can.origin.z_axis() + Vec3::Z).length() < 1e-12);
    assert!(
        (test3(&r) + made).abs() < 1e-9 * made,
        "{} {made}",
        test3(&r)
    );
    assert!(step_nurb_gap(&k, &r) > 1.0);
}

fn seg(a: Vec3, b: Vec3) -> Curve {
    Curve::from(BSplineCurve::create_line_segment(3, a, b).unwrap())
}

/// The cones of a revolve of the rig's profile (sweeps/726: 8..30 x 0..22 about Z),
/// either winding, turned `angle` degrees, scaled by `scale`; the revolve must be a
/// manifold solid of the profile's swept volume.
fn revolve_cones(k: &mut Kernel, reverse: bool, angle: f64, scale: f64) -> Vec<Cone> {
    let p = |x: f64, z: f64| Vec3::new(x * scale, 0.0, z * scale);
    let mut pts = [
        p(8.0, 0.0),
        p(30.0, 0.0),
        p(30.0, 22.0),
        p(18.0, 22.0),
        p(18.0, 10.0),
        p(8.0, 10.0),
    ];
    if reverse {
        pts.reverse();
    }
    let curves: Vec<Curve> = (0..pts.len())
        .map(|i| seg(pts[i], pts[(i + 1) % pts.len()]))
        .collect();
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let mut b =
        create_rotational_sweep(k, &refs, Vec3::ZERO, Vec3::Z, angle, angle < 360.0).unwrap();
    assert!(is_manifold_solid(&b).unwrap(), "{reverse} {angle} {scale}");
    // the integral of x over the profile: 10 (30^2 - 8^2) / 2 + 12 (30^2 - 18^2) / 2
    let want = angle.to_radians() * 7636.0 * scale.powi(3);
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let got = brep_volume(k, &mut b, 1e-6, &attrs).unwrap();
    assert!(
        (got - want).abs() <= 1e-6 * want,
        "{reverse} {angle} {scale}: volume {got} {want}"
    );
    b.face_list()
        .iter()
        .filter_map(
            |&f| match b.face(f).unwrap().surface.as_ref().unwrap().kind() {
                SurfaceKind::Cone(c) => Some((**c).clone()),
                _ => None,
            },
        )
        .collect()
}

#[test]
fn issue_1463_revolved_cylinders_are_never_inside_out() {
    for (reverse, angle, scale) in [
        (false, 360.0, 1.0),
        (true, 360.0, 1.0),
        (false, 180.0, 1.0),
        (true, 180.0, 1.0),
        (true, 90.0, 1e-3),
        (false, 270.0, 1e3),
    ] {
        let case = format!("reverse {reverse}, {angle} deg, scale {scale}");
        let mut k = Kernel::default();
        k.fixes.revolve_analytic_profile_faces = true;
        let cones = revolve_cones(&mut k, reverse, angle, scale);
        assert_eq!(cones.len(), 3, "{case}");
        for c in &cones {
            assert!(c.is_cylinder_class(), "{case}");
            // fork 9853e54: built from the low end upwards along the revolve's axis
            assert!(!c.inside_out(), "{case}: an inside-out cylinder");
            assert!(
                (c.canonical().origin.z_axis() - Vec3::Z).length() < 1e-12,
                "{case}"
            );
            let made = test3(c);
            assert!(made > 0.0, "{case}: the made cylinder {made}");
            // the C++ reader's round trip, with upstream's IsNurbSurfaceCylinder and
            // with the fix: the same cylinder
            for fix in [false, true] {
                k.fixes.nurb_cylinder_inside_out_frame = fix;
                let r = Cone::is_nurb_surface_cylinder(&k, c.rev().base(), 1.0)
                    .unwrap()
                    .unwrap();
                let gap = step_nurb_gap(&k, &r);
                assert!(gap < 1e-12 * (1.0 + 30.0 * scale), "{case} {fix}: {gap}");
                assert!(
                    (test3(&r) - made).abs() <= 1e-9 * made,
                    "{case} {fix}: {} {made}",
                    test3(&r)
                );
                let (a, b) = (r.canonical().origin, c.canonical().origin);
                assert!((a.z_axis() - b.z_axis()).length() < 1e-12, "{case} {fix}");
            }
            k.fixes.nurb_cylinder_inside_out_frame = false;
        }
    }
}
