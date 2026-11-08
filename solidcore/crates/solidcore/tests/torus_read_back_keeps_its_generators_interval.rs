#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! A torus read back from its NURB keeps its V interval its generator's (solidcore#3204,
//! solidcraft#12757).
//!
//! With `found_torus_sphere_keep_tested_nurb` (WR-493), `Torus::is_nurb_surface_torus`
//! rebuilds the analytic data from the NURB it tested: it snaps the generator's angle
//! interval to 0 or 360 within `SM_EFF_ZERO_DEG` for the torus's V interval, then builds the
//! generator circle on the NURB's iso curve, which measures its own interval, unsnapped. The
//! two differed by up to `SM_EFF_ZERO_DEG`, and the torus failed `SmSurfOfRevolution` test #2
//! and `SmTorus` test #2 (tolerance 1e-12).
use solidcore::geom::assert_array::{AssertArray, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::Point2;
use solidcore::kernel::{KernelOptions, UpstreamFixes};
use solidcore::math::{Point3, Vec3};
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::surface::torus::{Torus, TorusOptions};
use solidcore::Kernel;

/// The failed checks of the torus read back from the NURB of `at`'s torus over `v`.
fn failed_checks(k: &Kernel, at: &Axis2Placement, v: (f64, f64)) -> Vec<String> {
    let t = Torus::create_canonical(k, at, 49.0, 0.2, TorusOptions::default(), None).unwrap();
    let mut s = Surface::from(t);
    let d = Extent2d::from_points(Point2::new(0.0, v.0), Point2::new(180.0, v.1));
    s.adjust_step_uv_domain(k, &d).unwrap();
    let SurfaceKind::Torus(t) = s.kind() else {
        panic!("a torus")
    };
    let back = Torus::is_nurb_surface_torus(k, t.base().base(), 1.0)
        .unwrap()
        .expect("read back as a torus");
    let mut list = AssertArray::new();
    Surface::from(back)
        .assert_valid(k, Some(&mut list), 2, ObjectKey(1))
        .unwrap();
    list.reports()
        .iter()
        .filter(|r| !r.ok)
        .map(|r| format!("type {} test {} {}", r.reporting_type, r.test_index, r.name))
        .collect()
}

#[test]
fn a_torus_read_back_from_its_nurb_passes_its_check() {
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes = UpstreamFixes::all();
    // A quarter of the tube, axis +Y, as a round on a part's edge: its generator's
    // measured interval ends a hair past 0.
    let at = Axis2Placement::new(Point3::new(51.0, 44.0, 14.75), -Vec3::X, Vec3::Z);
    assert_eq!(failed_checks(&k, &at, (-90.0, 0.0)), Vec::<String>::new());
    // Frames a little off the axes, as an imported part has them, over a quarter, a quarter
    // past a half turn, and a half.
    for i in 0..20 {
        let a = f64::from(i) * 0.731;
        let z = Vec3::new(a.sin() * 1e-3, 1.0, a.cos() * 1e-4)
            .unitize()
            .unwrap();
        let x = Vec3::new(-1.0, 0.0, 1.7e-4)
            .cross(z)
            .cross(z)
            .unitize()
            .unwrap();
        let x = if x.dot(-Vec3::X) < 0.0 { -x } else { x };
        let at = Axis2Placement::new(Point3::new(51.0, 44.0, 14.75), x, z.cross(x));
        for v in [(-90.0, 0.0), (180.0, 270.0), (-90.0, 90.0)] {
            assert_eq!(
                failed_checks(&k, &at, v),
                Vec::<String>::new(),
                "frame {i}, V {v:?}"
            );
        }
    }
}
