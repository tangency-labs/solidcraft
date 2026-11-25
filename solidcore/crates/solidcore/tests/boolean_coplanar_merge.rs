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
use solidcore::mass::integrator::properties::brep_volume_default;
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

/// One operand of [`interleaved_grids_union`]: the strips of an `n` x `n` grid whose index
/// has parity `parity`, `w` wide and `2 w` apart, on z = 0: those along x 1 high, those along
/// y 2 high, united one after another into one grid whose bottom is one face.
fn grid(k: &mut Kernel, n: u32, w: f64, parity: u32) -> solidcore::Brep {
    let length = 2.0 * w * f64::from(n);
    let mut all: Option<solidcore::Brep> = None;
    for i in (0..n).filter(|i| i % 2 == parity) {
        let at = 2.0 * w * f64::from(i);
        for strip in [
            create_box(k, Point3::new(0.0, at, 0.0), length, w, 1.0).unwrap(),
            create_box(k, Point3::new(at, 0.0, 0.0), w, length, 2.0).unwrap(),
        ] {
            all = Some(match all {
                Some(a) => boolean_union(k, a, strip).unwrap(),
                None => strip,
            });
        }
    }
    all.unwrap()
}

/// The even strips of an `n` x `n` woven grid united with the odd ones, as a woven panel's
/// halves are joined: the two bottoms lie on one plane and coincide on the squares where a
/// strip of one grid crosses a strip of the other. Checks the union's exact volume (a closed
/// form) and that it is valid at Level 2, and returns the points its imprint classified on
/// the faces it split ([`solidcore::kernel::WorkCounters::split_face_classifications`]) and
/// its faces, edges and vertices.
fn interleaved_grids_union(n: u32) -> (u64, [usize; 3]) {
    let mut k = Kernel::default();
    let w = 1.0;
    let a = grid(&mut k, n, w, 0);
    let b = grid(&mut k, n, w, 1);
    k.work_counters.reset();
    let mut u = boolean_union(&mut k, a, b).unwrap();
    let classified = k.work_counters.split_face_classifications();
    let length = 2.0 * w * f64::from(n);
    let exact = f64::from(n) * length * w * (1.0 + 2.0) - f64::from(n * n) * w * w;
    let volume = brep_volume_default(&k, &mut u, 1e-9).unwrap();
    let lines = check_brep(&mut k, &mut u, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{n} x {n}: {lines:?}");
    let topology = [
        get_faces(&u).unwrap().len(),
        solidcore::topology::brep::p1::edges(&u, None)
            .unwrap()
            .len(),
        solidcore::topology::brep::p1::vertices(&u, None)
            .unwrap()
            .len(),
    ];
    println!("{n} x {n} interleaved grids: {classified} points classified on split faces, volume {volume}, faces, edges, vertices {topology:?}");
    assert!(
        (volume - exact).abs() <= 1e-9 * exact,
        "volume {volume}, exact {exact}"
    );
    (classified, topology)
}

/// Joining two woven grids whose bottoms are each one face imprints every crossing of a
/// strip of one with a strip of the other in both bottoms; each imprint splits a bottom
/// face and classifies what is still to be imprinted there on its two parts: 61 and 288
/// points for 4 x 4 and 8 x 8 strips, about four times for four times the crossings.
/// Upstream's `UpdateFaces` also classified every coincident proto edge of every proto face
/// on the split face, all of them on the one bottom: 226 and 8265 points, 37 times for four
/// times the crossings. A woven panel of 144 strips spent most of its joins' time there.
#[test]
fn interleaved_grids_on_one_plane_classify_no_coincident_edges_at_face_splits() {
    let counts = [4, 8].map(interleaved_grids_union);
    assert_eq!(counts.map(|c| c.0), [61, 288]);
    assert_eq!(counts.map(|c| c.1), [[66, 219, 146], [226, 819, 546]]);
}
