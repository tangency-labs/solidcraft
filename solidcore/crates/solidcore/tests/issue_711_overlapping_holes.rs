//! tangency-labs/solidcore#711: a second hole cut overlapping a first one. Upstream
//! (usd-brep 1.0.1) sends cylinder/cylinder to `SmCone::IntersectWithCone`, whose
//! parallel-cylinder branch tests each crossing line at the tool's bottom (v = 0), below
//! the first hole's wall trimmed to the disc, so both lines are dropped and the cut
//! fails in `SmBrep::MakeFace` (`SM_ERR`). solidcore clips each line to both faces'
//! domains and tests it at its middle (WR-259).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Brep, Kernel};

fn volume(k: &Kernel, b: &mut Brep) -> solidcore::Result<f64> {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs)
}

/// The cut body passes the Level 2 check.
fn assert_valid(k: &mut Kernel, b: &mut Brep) {
    let check = check_brep(k, b, CheckLevel::Level2).unwrap();
    assert!(check.is_valid(), "{:?}", check.lines());
}

#[test]
fn issue_711_second_overlapping_hole_cuts() {
    let mut k = Kernel::default();
    // a disc r 20, 10 thick
    let mut disc = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 20.0, 10.0).unwrap();
    // two r 5 holes through it, 6 apart, the tools 12 high from z = -1
    let centres = [(6.0, 0.0), (3.0, 3.0 * 3.0_f64.sqrt())];
    for (x, y) in centres {
        let tool = create_cylinder(&mut k, Point3::new(x, y, -1.0), 5.0, 12.0).unwrap();
        disc = boolean_difference(&mut k, disc, tool).unwrap();
    }

    // the disc less the union of the two discs r 5 whose centres are 6 apart
    let (r, d) = (5.0_f64, 6.0_f64);
    let lens = 2.0 * r * r * (d / (2.0 * r)).acos() - 0.5 * d * (4.0 * r * r - d * d).sqrt();
    let holes = 2.0 * core::f64::consts::PI * r * r - lens;
    let want = (core::f64::consts::PI * 400.0 - holes) * 10.0;
    let got = volume(&k, &mut disc).unwrap();
    assert!((got - want).abs() <= 1e-5 * want, "{got} vs {want}");
    assert_valid(&mut k, &mut disc);
}

/// The 0..4 plate is below the tool's midpoint at z = 5: the crossing lines are
/// clipped to the faces' domains, so they are tested inside the plate.
#[test]
fn issue_711_second_hole_cuts_below_the_tools_midpoint() {
    let (r, d) = (5.0_f64, 6.0_f64);
    let lens = 2.0 * r * r * (d / (2.0 * r)).acos() - 0.5 * d * (4.0 * r * r - d * d).sqrt();
    let holes = 2.0 * core::f64::consts::PI * r * r - lens;
    let want = (core::f64::consts::PI * 400.0 - holes) * 4.0;
    let mut k = Kernel::default();
    let mut disc = create_cylinder(&mut k, Point3::ZERO, 20.0, 4.0).unwrap();
    let first = create_cylinder(&mut k, Point3::new(6.0, 0.0, -1.0), 5.0, 12.0).unwrap();
    disc = boolean_difference(&mut k, disc, first).unwrap();
    let second = create_cylinder(
        &mut k,
        Point3::new(3.0, 3.0 * 3.0_f64.sqrt(), -1.0),
        5.0,
        12.0,
    )
    .unwrap();
    let mut result = boolean_difference(&mut k, disc, second).unwrap();
    let got = volume(&k, &mut result).unwrap();
    assert!((got - want).abs() <= 1e-5 * want, "{got} vs {want}");
    assert_valid(&mut k, &mut result);
}
