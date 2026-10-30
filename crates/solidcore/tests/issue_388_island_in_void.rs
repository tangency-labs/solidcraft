//! tangency-labs/solidcore#388 (WR-265): nested solids after a boolean fail SMLib's own check,
//! `SmBrep` test #12 ("Inner shell is not geometrically inside its containing outerShell").
//!
//! The issue's own case, a cube of 20 joined into the cavity of a hollow cube (50 minus
//! 40) and then cut by a cube of 10, passes the check in the port with or without the fix,
//! as SMLib's own union-then-cut does in the fork (the reporter's failure came from the C
//! API's import). The case that fails is the second repro in the thread: a union that
//! seals the void. `ManifoldBoolean` leaves the cavity's shell first in the solid region;
//! `Kernel::fixes.boolean_outer_shell_first` puts the outer shell first.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, PI};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::topology::brep::p1;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Brep, Kernel};

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

/// SMLib's check of `b` at level 2: the number of test #12 failures and every line.
fn test_12(k: &mut Kernel, b: &mut Brep) -> (usize, Vec<String>) {
    let lines = check_brep(k, b, CheckLevel::Level2).unwrap().lines();
    let n = lines
        .iter()
        .filter(|l| l.contains("Test #12 Bad Inner shells"))
        .count();
    (n, lines)
}

/// The issue's case: the cube 0..50 minus 5..45, joined with the cube 15..35, minus the
/// cube of 10 at `cut`.
fn island(fix: bool, cut: Point3) -> (Kernel, Brep) {
    let mut k = Kernel::default();
    k.fixes.boolean_outer_shell_first = fix;
    let outer = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 50.0, 50.0, 50.0).unwrap();
    let cavity = create_box(&mut k, Point3::new(5.0, 5.0, 5.0), 40.0, 40.0, 40.0).unwrap();
    let hollow = boolean_difference(&mut k, outer, cavity).unwrap();
    let cube = create_box(&mut k, Point3::new(15.0, 15.0, 15.0), 20.0, 20.0, 20.0).unwrap();
    let joined = boolean_union(&mut k, hollow, cube).unwrap();
    let ten = create_box(&mut k, cut, 10.0, 10.0, 10.0).unwrap();
    let b = boolean_difference(&mut k, joined, ten).unwrap();
    (k, b)
}

/// The second repro: discs of r 37 stacked along -Z, a top z -3.6..0, a ring r 35..37 at
/// z -5.6..-3.6 and a bottom z -8..-5.6. Their union seals the void r < 35, z -5.6..-3.6.
fn sealed(fix: bool, ring_and_bottom_first: bool) -> (Kernel, Brep) {
    let mut k = Kernel::default();
    k.fixes.boolean_outer_shell_first = fix;
    let top = create_cylinder(&mut k, Point3::new(0.0, 0.0, -3.6), 37.0, 3.6).unwrap();
    let ring_out = create_cylinder(&mut k, Point3::new(0.0, 0.0, -5.6), 37.0, 2.0).unwrap();
    let ring_in = create_cylinder(&mut k, Point3::new(0.0, 0.0, -6.6), 35.0, 4.0).unwrap();
    let ring = boolean_difference(&mut k, ring_out, ring_in).unwrap();
    let bottom = create_cylinder(&mut k, Point3::new(0.0, 0.0, -8.0), 37.0, 2.4).unwrap();
    let b = if ring_and_bottom_first {
        let rb = boolean_union(&mut k, ring, bottom).unwrap();
        boolean_union(&mut k, top, rb).unwrap()
    } else {
        let tr = boolean_union(&mut k, top, ring).unwrap();
        boolean_union(&mut k, tr, bottom).unwrap()
    };
    (k, b)
}

#[test]
fn issue_388_flag_is_off_by_default() {
    assert!(!Kernel::default().fixes.boolean_outer_shell_first);
}

#[test]
fn issue_388_island_in_a_void_passes_test_12() {
    // a cut inside the island (a void in it), one across its corner, and one into its side
    let cuts = [
        (Point3::new(20.0, 20.0, 20.0), 1000.0),
        (Point3::new(30.0, 30.0, 30.0), 125.0),
        (Point3::new(10.0, 20.0, 20.0), 500.0),
    ];
    for fix in [false, true] {
        for (cut, cut_off) in cuts {
            let (mut k, mut b) = island(fix, cut);
            let (n, lines) = test_12(&mut k, &mut b);
            assert_eq!(n, 0, "fix {fix} cut {cut:?}: {lines:?}");
            let want = 50.0_f64.powi(3) - 40.0_f64.powi(3) + 20.0_f64.powi(3) - cut_off;
            let got = volume(&k, &mut b);
            assert!((got - want).abs() <= 1e-9 * want, "{got} vs {want}");
        }
    }
}

#[test]
fn issue_388_union_sealing_a_void_passes_test_12_with_the_fix() {
    let want = PI * 37.0 * 37.0 * 6.0 + 2.0 * PI * (37.0 * 37.0 - 35.0 * 35.0);
    for ring_and_bottom_first in [false, true] {
        let (mut k, mut b) = sealed(true, ring_and_bottom_first);
        let (n, lines) = test_12(&mut k, &mut b);
        assert_eq!(n, 0, "{lines:?}");
        let infinite = b.infinite_region();
        let solids: Vec<_> = p1::regions(&b, None)
            .unwrap()
            .into_iter()
            .filter(|&g| Some(g) != infinite && b.region(g).unwrap().is_void() == 0)
            .collect();
        assert_eq!(solids.len(), 1);
        let shells = &b.region(solids[0]).unwrap().shells;
        assert_eq!(shells.len(), 2, "the outer shell and the void's");
        let got = volume(&k, &mut b);
        assert!((got - want).abs() <= 1e-6 * want, "{got} vs {want}");
    }
}

#[test]
fn issue_388_union_sealing_a_void_fails_test_12_upstream() {
    for ring_and_bottom_first in [false, true] {
        let (mut k, mut b) = sealed(false, ring_and_bottom_first);
        let (n, lines) = test_12(&mut k, &mut b);
        assert_eq!(n, 1, "{lines:?}");
    }
}
