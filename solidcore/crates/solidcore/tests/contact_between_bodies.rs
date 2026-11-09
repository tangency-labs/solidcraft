//! #3144: whether two bodies meet, decided by a certified bracket of their boundary distance
//! (`query::contact`), against closed forms: boxes touching on a face, apart by 1e-9,
//! overlapping in a face, an edge resting on a face, interpenetrating, one inside the other,
//! far apart; spheres apart and crossing; a cylinder crossing a box's face and one against a
//! side along a line; a peg through a hole that it clears.

#![allow(clippy::unwrap_used, clippy::panic, clippy::float_cmp)] // test

use solidcore::boolean::api_brep::boolean_difference;
use solidcore::geom::certified::Verdict;
use solidcore::kernel::Kernel;
use solidcore::primitive::api_primitives::{create_box, create_cylinder, create_sphere};
use solidcore::query::contact::{boundary_distance, meets};
use solidcore::topology::brep::mirror::transform_matrix;
use solidcore::{Brep, Point3};

fn k() -> Kernel {
    Kernel {
        fixes: solidcore::kernel::UpstreamFixes::all(),
        ..Kernel::default()
    }
}

fn cube(k: &mut Kernel, at: [f64; 3], size: f64) -> Brep {
    create_box(k, Point3::new(at[0], at[1], at[2]), size, size, size).unwrap()
}

/// `b` turned by `angle` about the x axis, then moved by `by`.
fn turned_about_x(k: &mut Kernel, b: &mut Brep, angle: f64, by: [f64; 3]) {
    let (c, s) = (angle.cos(), angle.sin());
    // row vectors (`p' = p A + t`): A's rows are the images of x, y and z, t the fourth row
    let m = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, c, s, 0.0],
        [0.0, -s, c, 0.0],
        [by[0], by[1], by[2], 1.0],
    ];
    transform_matrix(k, b, &m, true).unwrap();
}

/// Asserts the verdict at `tol`, the bracket shown on failure.
fn verdict_is(k: &Kernel, a: &mut Brep, b: &mut Brep, tol: f64, want: Verdict) {
    let c = meets(k, a, b, tol).unwrap();
    assert!(c.lower <= c.upper, "{c:?}");
    assert_eq!(c.verdict, want, "at {tol}: {c:?}");
}

/// The bracket of the boundary distance, asserted to hold `exact`.
fn holds(k: &Kernel, a: &mut Brep, b: &mut Brep, exact: f64) {
    let c = boundary_distance(k, a, b, 1e-6).unwrap();
    assert_eq!(c.verdict, Verdict::Yes, "{c:?}");
    assert!(c.lower <= exact && exact <= c.upper, "{exact} not in {c:?}");
    assert!(c.upper - c.lower <= 1e-6 * c.upper.max(1.0), "{c:?}");
}

#[test]
fn boxes_touching_on_a_face_meet_at_any_tolerance() {
    let mut k = k();
    let mut a = cube(&mut k, [0.0; 3], 1.0);
    let mut b = cube(&mut k, [1.0, 0.0, 0.0], 1.0);
    let c = meets(&k, &mut a, &mut b, 1e-12).unwrap();
    assert_eq!(c.verdict, Verdict::Yes, "{c:?}");
    let [p, q] = c.witness.unwrap();
    assert!(
        p.distance(q) <= 1e-12 && (p.x - 1.0).abs() <= 1e-12,
        "{c:?}"
    );
}

#[test]
fn boxes_apart_by_a_nanometre_meet_only_at_a_tolerance_past_it() {
    let mut k = k();
    // the gap as the doubles have it
    let gap = (1.0 + 1e-9) - 1.0;
    let mut a = cube(&mut k, [0.0; 3], 1.0);
    let mut b = cube(&mut k, [1.0 + 1e-9, 0.0, 0.0], 1.0);
    verdict_is(&k, &mut a, &mut b, 1e-8, Verdict::Yes);
    verdict_is(&k, &mut a, &mut b, 1e-10, Verdict::No);
    holds(&k, &mut a, &mut b, gap);
}

#[test]
fn boxes_overlapping_in_a_face_meet() {
    let mut k = k();
    let mut a = cube(&mut k, [0.0; 3], 1.0);
    let mut b = cube(&mut k, [1.0, 0.5, 0.5], 1.0);
    verdict_is(&k, &mut a, &mut b, 1e-12, Verdict::Yes);
}

#[test]
fn an_edge_resting_on_a_face_meets_it() {
    let mut k = k();
    let mut a = cube(&mut k, [0.0; 3], 1.0);
    // a cube centred on the x axis, turned 45 degrees about it (either way: it is square), is
    // a diamond whose lowest edge is at z = -sqrt(2)/2; moved onto a's top face
    let mut b = cube(&mut k, [0.0, -0.5, -0.5], 1.0);
    let half = std::f64::consts::FRAC_1_SQRT_2;
    turned_about_x(
        &mut k,
        &mut b,
        std::f64::consts::FRAC_PI_4,
        [0.0, 0.5, 1.0 + half],
    );
    let c = meets(&k, &mut a, &mut b, 1e-12).unwrap();
    assert_eq!(c.verdict, Verdict::Yes, "{c:?}");
    let [p, _] = c.witness.unwrap();
    assert!((p.z - 1.0).abs() <= 1e-9, "{c:?}");
}

#[test]
fn interpenetrating_boxes_meet() {
    let mut k = k();
    let mut a = cube(&mut k, [0.0; 3], 1.0);
    let mut b = cube(&mut k, [0.5; 3], 1.0);
    verdict_is(&k, &mut a, &mut b, 1e-12, Verdict::Yes);
}

#[test]
fn a_box_inside_another_meets_it_and_their_walls_are_one_apart() {
    let mut k = k();
    let mut a = cube(&mut k, [0.0; 3], 3.0);
    let mut b = cube(&mut k, [1.0; 3], 1.0);
    verdict_is(&k, &mut a, &mut b, 1e-3, Verdict::Yes);
    verdict_is(&k, &mut b, &mut a, 1e-3, Verdict::Yes);
    holds(&k, &mut a, &mut b, 1.0);
}

#[test]
fn boxes_far_apart_do_not_meet_and_their_gap_is_bracketed() {
    let mut k = k();
    let mut a = cube(&mut k, [0.0; 3], 1.0);
    let mut b = cube(&mut k, [5.0, 0.0, 0.0], 1.0);
    verdict_is(&k, &mut a, &mut b, 0.1, Verdict::No);
    holds(&k, &mut a, &mut b, 4.0);
}

#[test]
fn spheres_apart_are_their_centres_gap_less_their_radii() {
    let mut k = k();
    let mut a = create_sphere(&mut k, Point3::new(0.0, 0.0, 0.0), 1.0).unwrap();
    let mut b = create_sphere(&mut k, Point3::new(3.0, 0.0, 0.0), 1.0).unwrap();
    verdict_is(&k, &mut a, &mut b, 0.5, Verdict::No);
    verdict_is(&k, &mut a, &mut b, 2.0, Verdict::Yes);
    holds(&k, &mut a, &mut b, 1.0);
}

#[test]
fn crossing_spheres_meet_where_their_surfaces_cross() {
    let mut k = k();
    let mut a = create_sphere(&mut k, Point3::new(0.0, 0.0, 0.0), 1.0).unwrap();
    let mut b = create_sphere(&mut k, Point3::new(1.5, 0.0, 0.0), 1.0).unwrap();
    let c = meets(&k, &mut a, &mut b, 1e-9).unwrap();
    assert_eq!(c.verdict, Verdict::Yes, "{c:?}");
    // the witness is on the circle where they cross: x = 0.75, |p| = 1
    let [p, _] = c.witness.unwrap();
    assert!((p.x - 0.75).abs() <= 1e-6, "{c:?}");
}

#[test]
fn a_cylinder_crossing_a_box_face_meets_the_box() {
    let mut k = k();
    let mut a = cube(&mut k, [0.0; 3], 1.0);
    let mut b = create_cylinder(&mut k, Point3::new(0.5, 0.5, 0.5), 0.3, 1.0).unwrap();
    verdict_is(&k, &mut a, &mut b, 1e-9, Verdict::Yes);
}

/// A cylinder touching a box side along a line meets it; moved 0.25 off, its bracket holds
/// 0.25 and the verdicts either side of it are exact.
///
/// The bracket is not 1e-6 wide here: trimmed rim pieces stall `lower` at 0.25 - 0.0013 (the
/// end caps' pieces at the rim straddle the circular trim, and each hull carries a sliver of
/// the cap's plane outside the disk toward the box; their subdivision runs out of pairs
/// first). The bracket contains the truth; verdicts outside [0.2487, 0.25] are exact. The
/// width asserted is what the pair budget certifies, not a target.
#[test]
fn a_cylinder_against_a_box_side_meets_it_along_a_line() {
    let mut k = k();
    let mut a = cube(&mut k, [0.0; 3], 1.0);
    let mut b = create_cylinder(&mut k, Point3::new(1.5, 0.5, 0.0), 0.5, 1.0).unwrap();
    verdict_is(&k, &mut a, &mut b, 1e-9, Verdict::Yes);
    let mut far = create_cylinder(&mut k, Point3::new(1.75, 0.5, 0.0), 0.5, 1.0).unwrap();
    verdict_is(&k, &mut a, &mut far, 0.24, Verdict::No);
    verdict_is(&k, &mut a, &mut far, 0.26, Verdict::Yes);
    let c = boundary_distance(&k, &mut a, &mut far, 1e-6).unwrap();
    assert!(c.lower <= 0.25 && 0.25 <= c.upper, "{c:?}");
    assert!(c.lower >= 0.2487 && c.upper - 0.25 <= 1e-12, "{c:?}");
}

#[test]
fn a_peg_through_a_hole_it_clears_does_not_meet_the_plate() {
    let mut k = k();
    // a 4 x 4 x 1 plate with a hole of radius 1 through its middle, and a peg of radius 0.75
    // down the hole's axis: the peg clears the hole's wall by 0.25
    let plate = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 4.0, 4.0, 1.0).unwrap();
    let hole = create_cylinder(&mut k, Point3::new(2.0, 2.0, -1.0), 1.0, 3.0).unwrap();
    let mut plate = boolean_difference(&mut k, plate, hole).unwrap();
    let mut peg = create_cylinder(&mut k, Point3::new(2.0, 2.0, -2.0), 0.75, 5.0).unwrap();
    verdict_is(&k, &mut plate, &mut peg, 0.1, Verdict::No);
    holds(&k, &mut plate, &mut peg, 0.25);
}
