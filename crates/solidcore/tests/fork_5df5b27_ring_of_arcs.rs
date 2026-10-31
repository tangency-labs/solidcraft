//! WR-385 (the SolidCraft fork's tangency-labs/smlib@5df5b27, a cherry-pick of f06b48a), the
//! fork's own capi `check.cpp` case: a ring r 10 / r 9, each circle drawn as 256 arcs, extruded
//! 3. `StitchAndOrient` turns the hole's walls over (`SwapUV`) and each `SmCylinder`'s analytic
//! form is rebuilt from its NURB, whose radii read back differ by rounding (9.0000000000985 and
//! 9.0000000000757 in the fork's trace). Upstream equalises them only within
//! `SM_EFF_ZERO * (1 + r)`, which gives the cylinders a semi-angle their own check
//! (`SmCylinder::Test #0 Zero SemiAngle`) refuses. With
//! `Kernel::fixes.cylinder_rebuild_mean_radius` the ring checks clean at level 2.
//!
//! The arcs are built as the fork's capi `smlib_extrude` builds them (`arc` in
//! `capi/src/smlib_sweeps.cpp`): the frame turned to the arc's start, the interval
//! `[0, sweep]` in degrees. The arcs are `SmCylinder` walls only with
//! `Kernel::fixes.linear_sweep_arc_is_cylinder` (WR-565, also in the fork), so both legs set it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::math::{Point3, Vec3};
use solidcore::surface::surface::SurfaceKind;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::face::p2 as face;
use solidcore::{Brep, Kernel};

const ARCS: usize = 256;

/// The capi arc: centre `centre`, axis `axis`, reference direction `reference`, `radius`,
/// from `start` to `end` radians.
fn arc(
    k: &Kernel,
    centre: Point3,
    axis: Vec3,
    reference: Vec3,
    radius: f64,
    start: f64,
    end: f64,
) -> Curve {
    let sweep = end - start;
    let y = axis.cross(reference);
    let x0 = reference * start.cos() + y * start.sin();
    let y0 = axis.cross(x0);
    let mut frame = Axis2Placement::default();
    frame.set_canonical(centre, x0, y0).unwrap();
    let degrees = Extent1d::from_min_max(0.0, 360.0_f64.min(sweep * 180.0 / core::f64::consts::PI));
    Curve::from(Ellipse::create_canonical_circle(k, &frame, radius, Some(&degrees), None).unwrap())
}

/// The ring extruded with the WR-385 opt-in `fix`.
fn ring(fix: bool) -> (Kernel, Brep) {
    let mut k = Kernel::default();
    k.fixes.linear_sweep_arc_is_cylinder = true;
    k.fixes.cylinder_rebuild_mean_radius = fix;
    let pi = core::f64::consts::PI;
    let mut curves = Vec::new();
    for radius in [10.0, 9.0] {
        let turn = if radius == 10.0 { 1.0 } else { -1.0 };
        for i in 0..ARCS {
            let a0 = 2.0 * pi * i as f64 / ARCS as f64;
            curves.push(arc(
                &k,
                Point3::new(0.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, turn),
                Vec3::new(1.0, 0.0, 0.0),
                radius,
                a0,
                a0 + 2.0 * pi / ARCS as f64,
            ));
        }
    }
    let profile: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let b = create_linear_sweep(&mut k, &profile, Vec3::new(0.0, 0.0, 1.0), 3.0, true).unwrap();
    (k, b)
}

/// The semi-angles of the ring's `SmCylinder` walls.
fn cylinder_semi_angles(b: &Brep) -> Vec<f64> {
    let mut angles = Vec::new();
    for f in p1::faces(b, None).unwrap() {
        let Some(s) = face::surface(b, f).unwrap() else {
            continue;
        };
        match s.kind() {
            SurfaceKind::Cone(c) if c.is_cylinder_class() => angles.push(c.semi_angle_deg()),
            _ => {}
        }
    }
    angles
}

#[test]
fn wr385_default_gives_the_hole_walls_a_semi_angle() {
    let (mut k, mut b) = ring(false);
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(
        lines.iter().any(|l| l.contains("Zero SemiAngle")),
        "the pinned rebuild's semi-angle is refused: {lines:?}"
    );
}

#[test]
fn wr385_ring_of_arcs_checks_clean_with_the_fix() {
    let (mut k, mut b) = ring(true);
    let angles = cylinder_semi_angles(&b);
    assert_eq!(angles.len(), 2 * ARCS, "every arc's wall is an SmCylinder");
    assert!(angles.iter().all(|&a| a == 0.0), "{angles:?}");
    let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert_eq!(check.failure_count(), 0, "{:?}", check.lines());
}
