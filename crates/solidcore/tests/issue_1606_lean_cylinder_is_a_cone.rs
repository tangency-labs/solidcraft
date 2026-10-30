//! Regression test for tangency-labs/solidcore#1606: `Cone::lean_cylinder` kept the clone's
//! `SmCylinder` flag. With `cylinder_rebuild_mean_radius` on, the rebuild refused every leaned
//! cylinder (`SM_ERR`); with it off, the leaned surface still reported itself a cylinder.
//!
//! The case is SolidCraft's `draft_a_curved_face`: the side of a cylinder of radius 5 about +Z
//! from the origin, heights 0 to 10, drafted by 0.1 rad about z = 0 (leaning in). Its solid is
//! the frustum of radii R = 5 and r = R - 10 tan 0.1, of volume
//! pi h (R^2 + R r + r^2) / 3 = 638.3350153968712.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent2d::Extent2d;
use solidcore::surface::cone::Cone;
use solidcore::{Kernel, Point2, Point3, Vec3};

const VOLUME: f64 = 638.335_015_396_871_2;

fn rel_close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs())
}

fn cylinder(k: &Kernel, origin: Point3) -> Cone {
    let frame = Axis2Placement::new(origin, Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
    let mut cylinder = Cone::create_canonical_cylinder(k, &frame, 5.0).unwrap();
    let mut domain = Extent2d::default();
    domain.set_min_max_coords(0.0, 0.0, 360.0, 10.0).unwrap();
    cylinder.adjust_step_uv_domain(k, &domain).unwrap();
    cylinder
}

/// The leaned cylinder of the SolidCraft case under `k`: a cone with the drafted radius law,
/// whose analytic data and NURB both bound the frustum's volume.
fn check_draft(k: &Kernel) {
    let cylinder = cylinder(k, Point3::new(0.0, 0.0, 0.0));
    let slope = -(0.1f64).tan();
    let cone = cylinder
        .lean_cylinder(k, slope, 0.0)
        .expect("a leaned cylinder rebuilds as a cone");
    assert!(!cone.is_cylinder(), "a leaned cylinder is a cone");
    assert!(!cone.is_cylinder_class(), "a leaned cylinder is an SmCone");
    assert_eq!(cone.type_string(), "SmCone_TYPE");

    let canonical = cone.canonical();
    assert!((canonical.radius - 5.0).abs() < 1e-9);
    assert!((canonical.semi_angle_deg - slope.atan().to_degrees()).abs() < 1e-9);

    // The analytic cone's frustum over heights 0 to 10.
    let h = 10.0;
    let big = canonical.radius;
    let small = big + h * canonical.semi_angle_deg.to_radians().tan();
    let analytic = std::f64::consts::PI * h * (big * big + big * small + small * small) / 3.0;
    assert!(rel_close(analytic, VOLUME), "{analytic} vs {VOLUME}");

    // The NURB's end circles (its rows are circles about +Z whose radius is linear in the
    // height): the same frustum.
    let nurb = cone.rev().base().natural_uv_domain();
    let mut ends = Vec::new();
    for v in [nurb.min().y, nurb.max().y] {
        let p = cone
            .rev()
            .base()
            .evaluate_point(Point2::new(0.5 * (nurb.min().x + nurb.max().x), v))
            .unwrap();
        ends.push((p.z, (p.x * p.x + p.y * p.y).sqrt()));
    }
    ends.sort_by(|a, b| a.0.total_cmp(&b.0));
    let ((z0, r0), (z1, r1)) = (ends[0], ends[1]);
    assert!(z0.abs() < 1e-9 && (z1 - h).abs() < 1e-9, "{ends:?}");
    let from_nurb = std::f64::consts::PI * (z1 - z0) * (r0 * r0 + r0 * r1 + r1 * r1) / 3.0;
    assert!(rel_close(from_nurb, VOLUME), "{from_nurb} vs {VOLUME}");
}

#[test]
fn lean_cylinder_is_a_cone_with_the_mean_radius_fix() {
    let mut k = Kernel::default();
    k.fixes.cylinder_rebuild_mean_radius = true;
    check_draft(&k);
}

#[test]
fn lean_cylinder_is_a_cone_without_the_mean_radius_fix() {
    check_draft(&Kernel::default());
}

/// The frame and slopes of the unit test (frame at (1, 2, 3), neutral height 4), both signs,
/// are no longer refused under `cylinder_rebuild_mean_radius`.
#[test]
fn leaned_cylinders_are_not_refused_with_the_mean_radius_fix() {
    let mut k = Kernel::default();
    k.fixes.cylinder_rebuild_mean_radius = true;
    let cylinder = cylinder(&k, Point3::new(1.0, 2.0, 3.0));
    for slope in [0.1, -0.1, 1e-3, -0.4] {
        let cone = cylinder
            .lean_cylinder(&k, slope, 4.0)
            .unwrap_or_else(|e| panic!("slope {slope}: {e:?}"));
        assert!(!cone.is_cylinder(), "slope {slope}");
        assert!(
            (cone.canonical().radius - (5.0 - 4.0 * slope)).abs() < 1e-9,
            "slope {slope}: {:?}",
            cone.canonical()
        );
    }
}
