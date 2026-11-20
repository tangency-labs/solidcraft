//! #15418 (PR #16594): the boolean's cleanup merges faces on different surfaces only where no
//! tolerance has to grow. Two planes cover each other within upstream's plane tolerance (the
//! gain 10 over 10000: `5e-4` of the smaller face tolerance, 5e-9 for faces of tolerance 1e-5),
//! well below the tenth of it under which upstream raises nothing, so near-coplanar faces from
//! a boolean's rounding still merge as upstream merges them.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::boolean::api_brep::boolean_union;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::get_faces;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Kernel, Point3};

/// A 10 cube at the origin united with a 10 x 10 x 10 box at x = 5 moved `dy` off along y: the
/// two front faces lie on the planes y = 0 and y = `dy`, crossing at the edge x = 10.
fn union_offset(dy: f64) -> (Kernel, solidcore::Brep) {
    let mut k = Kernel::default();
    let a = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let b = create_box(&mut k, Point3::new(5.0, dy, 0.0), 10.0, 10.0, 10.0).unwrap();
    let u = boolean_union(&mut k, a, b).unwrap();
    (k, u)
}

/// Fronts `dy` apart below the plane cover tolerance (5e-9) merge: the union is the 15 x 10 x
/// 10 box's 6 faces, valid at Level 2. These include the flanged enclosure's (1.2e-10 to
/// 9.8e-10 apart, #16594's gate), which a cover within a tenth of the face tolerance divided
/// by 10000 (1e-10) left as two faces each.
#[test]
fn near_coplanar_fronts_merge_below_the_plane_cover_tolerance() {
    for dy in [0.0, 2e-10, 1e-9, 4.5e-9] {
        let (mut k, mut u) = union_offset(dy);
        let lines = check_brep(&mut k, &mut u, CheckLevel::Level2)
            .unwrap()
            .lines();
        assert!(lines.is_empty(), "dy {dy}: {lines:?}");
        assert_eq!(get_faces(&u).unwrap().len(), 6, "dy {dy}");
    }
}

/// Fronts `dy` apart above the plane cover tolerance stay two faces, as upstream keeps them;
/// the body is valid at Level 2.
#[test]
fn near_coplanar_fronts_stay_apart_above_the_plane_cover_tolerance() {
    for dy in [5.5e-9, 1e-8] {
        let (mut k, mut u) = union_offset(dy);
        let lines = check_brep(&mut k, &mut u, CheckLevel::Level2)
            .unwrap()
            .lines();
        assert!(lines.is_empty(), "dy {dy}: {lines:?}");
        assert!(get_faces(&u).unwrap().len() > 6, "dy {dy}");
    }
}
