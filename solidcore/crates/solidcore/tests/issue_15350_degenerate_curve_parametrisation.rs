//! tangency-labs/solidcraft#15350: whether a surface degenerates to a curve does not depend on its
//! parametrisation. A cylinder of radius 1 and height `h` spans `h` across its height
//! and `2 pi` around: below the sweep's tolerance (`ZONE_TOL_3D / 10` = 1e-6) in `h` it
//! is a curve, whether it is the analytic cone (height parameter of span `h`,
//! `|Sv| = 1`) or its NURBS form (span 1, `|Sv| = h`). Before #15350 the analytic
//! cylinder 5e-7 high was not degenerate and its NURBS form was, so once #14425 made a
//! partial cone's side the analytic cylinder, a cone of that height refused with a
//! missing-entity error (1006), not the degeneracy (1007; the ub215 goldens rows).
#![allow(clippy::unwrap_used)] // test helpers

use solidcore::blend::fillet_section::station::as_b_spline;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::math::Vec3;
use solidcore::surface::cone::Cone;
use solidcore::surface::surface::Surface;
use solidcore::Kernel;

/// The sweep's 3D tolerance, `ZONE_TOL_3D / 10`, and its angle tolerance in degrees.
const TOL: f64 = 1.0e-5 / 10.0;
const ANGLE: f64 = 1.0e-4;

/// Whether the cone (radii `r1`, `r2`, height `h`) and its NURBS form are degenerate
/// curves.
fn degenerate(r1: f64, r2: f64, h: f64) -> (bool, bool) {
    let k = Kernel::default();
    let pl = Axis2Placement::new(Vec3::new(0.0, 0.0, 0.0), Vec3::X, Vec3::Y);
    let s = Surface::from(Cone::create_canonical(&k, &pl, r1, r2, h, false, false).unwrap());
    let form = Surface::from(as_b_spline(&k, &s).unwrap().unwrap().into_owned());
    (
        s.is_degenerate_curve(&k, ANGLE, None, TOL).unwrap(),
        form.is_degenerate_curve(&k, ANGLE, None, TOL).unwrap(),
    )
}

#[test]
fn a_thin_cylinder_is_a_curve_in_either_parametrisation() {
    // spans 5e-7 and 9.9e-7 across the height: below 1e-6
    assert_eq!(degenerate(1.0, 1.0, 5e-7), (true, true));
    assert_eq!(degenerate(1.0, 1.0, 9.9e-7), (true, true));
    // the ub215b cone, radii 0.45056741 and 0.45056751, 2.14e-7 high: its generator
    // is sqrt(h^2 + (r2 - r1)^2) = 2.36e-7 long
    assert_eq!(
        degenerate(
            0.450_567_413_729_598_1,
            0.450_567_513_527_301_23,
            2.139_793_659_242_570_6e-7
        ),
        (true, true)
    );
}

#[test]
fn a_cylinder_one_high_is_not_a_curve() {
    assert_eq!(degenerate(1.0, 1.0, 1.0), (false, false));
    // 2e-6 high: twice the tolerance
    assert_eq!(degenerate(1.0, 1.0, 2e-6), (false, false));
}
