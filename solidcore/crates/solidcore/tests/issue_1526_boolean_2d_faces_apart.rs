//! Regression test for WR-527 (#1526, #1527; the fork's tangency-labs/smlib@60b1c6a):
//! with `Kernel::fixes.boolean_skips_faces_apart` a 2D boolean skips the face pairs
//! whose boxes lie apart and the inner loops outside a new face's outer loop box. The
//! fork's case: a plate with r 2 holes, the disks united in balanced rounds and cut
//! from the plate in 2D, keeps its exact area, and gives the same faces as without
//! the fix.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::{boolean_2d, Boolean2d, Boolean2dOutcome};
use solidcore::mass::integrator::properties::face_area;
use solidcore::primitive::api_primitives::{create_planar_circle, create_plane};
use solidcore::topology::Brep;
use solidcore::{Kernel, Vec3};

fn result(outcome: Boolean2dOutcome) -> Brep {
    match outcome {
        Boolean2dOutcome::Result(b) => b,
        Boolean2dOutcome::Unassigned { .. } => panic!("2D boolean left its operands"),
    }
}

/// An `n` x `n` grid of r 2 holes, 10 apart, cut from a `10 n` square plate; the face
/// areas of the result, ascending.
fn holed_plate_areas(fixed: bool, n: u32) -> Vec<f64> {
    let mut k = Kernel::default();
    k.fixes.boolean_skips_faces_apart = fixed;
    let mut disks = Vec::new();
    for i in 0..n {
        for j in 0..n {
            let centre = Vec3::new(5.0 + 10.0 * f64::from(i), 5.0 + 10.0 * f64::from(j), 0.0);
            disks.push(create_planar_circle(&mut k, centre, 2.0).unwrap());
        }
    }
    // united in balanced rounds
    while disks.len() > 1 {
        let mut next = Vec::new();
        let mut round = disks.into_iter();
        while let Some(a) = round.next() {
            match round.next() {
                Some(b) => next.push(result(boolean_2d(&mut k, a, b, Boolean2d::Union).unwrap())),
                None => next.push(a),
            }
        }
        disks = next;
    }
    let side = 10.0 * f64::from(n);
    let plate = create_plane(&mut k, Vec3::new(0.0, 0.0, 0.0), side, side).unwrap();
    let holes = disks.pop().unwrap();
    let mut cut = result(boolean_2d(&mut k, plate, holes, Boolean2d::Difference).unwrap());
    let faces = cut.face_list().to_vec();
    let mut areas: Vec<f64> = faces
        .into_iter()
        .map(|f| face_area(&k, &mut cut, f, 1e-6).unwrap())
        .collect();
    areas.sort_by(f64::total_cmp);
    areas
}

#[test]
fn holed_plate_keeps_its_area_and_faces_with_the_fix() {
    let n = 4;
    let default = holed_plate_areas(false, n);
    let fixed = holed_plate_areas(true, n);
    assert_eq!(default.len(), fixed.len());
    for (a, b) in default.iter().zip(&fixed) {
        assert!(
            (a - b).abs() <= 1e-9 * a.abs().max(b.abs()),
            "{default:?} vs {fixed:?}"
        );
    }
    let side = 10.0 * f64::from(n);
    let exact = side * side - f64::from(n * n) * std::f64::consts::PI * 4.0;
    let total: f64 = fixed.iter().sum();
    assert!(
        (total - exact).abs() <= 1e-4 * exact,
        "area {total}, expected {exact}"
    );
}
