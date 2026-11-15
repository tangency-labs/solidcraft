//! #2928: `Cone::adjust_step_uv_domain` (`AdjustSTEPUVDomain`) on a whole-turn u window whose
//! ends sit a rounding error outside ±180° (the NIST `ctc_02` AP203 face #1152: u
//! −180.0000000000005 … 179.99999999999955°). The window spans 360.00000000000006°, and
//! the NURB's sweep circles were asked for that span, which `create_circle_segment`
//! refused past 360°. A whole-turn window is accepted wherever its ends fall, and the NURB built
//! for it lies on the cone: across the window, the NURB point at each STEP parameter's
//! NURB parameter (the stored NURB itself, evaluated there) is the cone's own (analytic)
//! point.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::geom::{axis2_placement::Axis2Placement, extent2d::Extent2d, vector2d::Point2};
use solidcore::{kernel::Kernel, math::Vec3, surface::cone::Cone};

/// The window's height span (the file's v range).
const V_MAX: f64 = 39.31893628625494;

/// The issue's cone placement (face #1152's `CONICAL_SURFACE`).
fn frame() -> Axis2Placement {
    Axis2Placement::new(
        Vec3::new(-2.4980018054066e-13, 820.0, -140.0),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(-1.0, 0.0, 0.0),
    )
}

/// Every whole-turn window builds, and its NURB lies on the cone across the window.
#[test]
fn a_whole_turn_window_a_rounding_error_past_180_is_accepted() {
    let k = Kernel::default();
    for semi_angle in [3.0, 2.999999996572] {
        for (u0, u1) in [
            (-180.0000000000005, 179.99999999999955),
            (-180.0, 180.0),
            (0.0, 360.0),
            (-179.9999999999995, 180.0000000000005),
        ] {
            let mut cone = Cone::create_canonical_with_semi_angle(
                &k,
                &frame(),
                100.0,
                semi_angle,
                None,
                false,
                false,
            )
            .unwrap();
            let r = cone.adjust_step_uv_domain(&k, &Extent2d::from_bounds(u0, 0.0, u1, V_MAX));
            assert!(r.is_ok(), "semi-angle {semi_angle}, u {u0}..{u1}: {r:?}");
            for i in 0..=8 {
                for v in [0.0, 0.5 * V_MAX, V_MAX] {
                    let u = u0 + (u1 - u0) * f64::from(i) / 8.0;
                    let step_uv = Point2::new(u, v);
                    let on_cone = cone.evaluate_step_point(step_uv).unwrap();
                    let nurb_uv = cone.convert_uv_from_step_to_nurbs(&k, step_uv).unwrap();
                    let on_nurb = cone.rev().base().evaluate_point(nurb_uv).unwrap();
                    let gap = (on_nurb - on_cone).length();
                    assert!(
                        gap <= 1e-9 * 1000.0,
                        "semi-angle {semi_angle}, u {u0}..{u1}, at ({u}, {v}): gap {gap:e}"
                    );
                }
            }
        }
    }
}
