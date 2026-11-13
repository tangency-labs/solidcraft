//! tangency-labs/solidcore#1671: `create_rotational_sweep(.., cap_ends = false)` of an
//! open profile built the profile's wire edge under the kernel's identities of the
//! B-rep (`topology_ref`, a plain edit) while every other entity of the sweep was named
//! by the primitive's notification scheme, so the start edge's curve was owned by
//! another identity and `check_brep` failed `SmBrep` test #61 on it. Upstream's owner is
//! the edge itself (`SmApiCreateRotationalSweep` checks valid).
#![allow(clippy::unwrap_used)]

use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::curve::line::Line;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::mass::integrator::properties::brep_area;
use solidcore::sweep::api_brep::create_rotational_sweep;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Kernel, Vec3};

fn failed(k: &mut Kernel, b: &mut solidcore::Brep) -> Vec<u64> {
    let c = check_brep(k, b, CheckLevel::Level2).unwrap();
    c.pointer_reports
        .reports()
        .iter()
        .chain(c.reports.reports())
        .filter(|r| !r.ok)
        .map(|r| r.test_index)
        .collect()
}

fn assert_area(k: &Kernel, b: &mut solidcore::Brep, expected: f64) {
    let area = brep_area(k, b, 1e-9).unwrap();
    assert!(
        (area - expected).abs() <= 1e-6 * expected,
        "area {area}, expected {expected}"
    );
}

/// A quarter arc of radius 10 in the xz plane, half a turn about z: a quarter-sphere
/// sheet of area 100 pi.
#[test]
fn uncapped_half_turn_of_a_quarter_arc_checks_valid() {
    let mut k = Kernel::default();
    let z = Vec3::new(0.0, 0.0, 1.0);
    let frame = Axis2Placement::new(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
    );
    let extent = Extent1d::from_min_max(0.0, 90.0);
    let arc = Curve::from(
        Ellipse::create_canonical_circle(&k, &frame, 10.0, Some(&extent), None).unwrap(),
    );
    let mut b =
        create_rotational_sweep(&mut k, &[Some(&arc)], Vec3::ZERO, z, 180.0, false).unwrap();
    assert_eq!(failed(&mut k, &mut b), Vec::<u64>::new());
    assert_area(&k, &mut b, 100.0 * std::f64::consts::PI);
}

/// A slanted line from (20, 0, 0) to (30, 0, 10), a quarter turn about z: a cone
/// frustum strip of area (pi / 2) sqrt(200) 25.
#[test]
fn uncapped_quarter_turn_of_a_slanted_line_checks_valid() {
    let mut k = Kernel::default();
    let z = Vec3::new(0.0, 0.0, 1.0);
    let l = Curve::from(
        Line::create_line_segment(
            3,
            Vec3::new(20.0, 0.0, 0.0),
            Vec3::new(30.0, 0.0, 10.0),
            None,
        )
        .unwrap(),
    );
    let mut b = create_rotational_sweep(&mut k, &[Some(&l)], Vec3::ZERO, z, 90.0, false).unwrap();
    assert_eq!(failed(&mut k, &mut b), Vec::<u64>::new());
    assert_area(
        &k,
        &mut b,
        std::f64::consts::FRAC_PI_2 * 200f64.sqrt() * 25.0,
    );
}
