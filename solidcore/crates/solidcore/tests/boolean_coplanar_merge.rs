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
/// strip of one grid crosses a strip of the other. Checks the union's exact volume and
/// returns the points its imprint classified on the faces it split
/// ([`solidcore::kernel::WorkCounters::split_face_classifications`]).
fn interleaved_grids_union(n: u32) -> u64 {
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
    println!("{n} x {n} interleaved grids: {classified} points classified on split faces, volume {volume}");
    assert!(
        (volume - exact).abs() <= 1e-9 * exact,
        "volume {volume}, exact {exact}"
    );
    classified
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
    assert_eq!(counts, [61, 288]);
}

/// A strip `w` wide across the `n` x `n` grid of [`grid`] on its diagonal, standing on z = 0:
/// its sides are planes, its top a cubic B-spline that waves between 1.4 and 2.6 high, so it
/// runs over the grid's strips of height 1 and dips into those of height 2.
fn wavy_diagonal_strip(k: &mut Kernel, n: u32, w: f64) -> solidcore::Brep {
    use solidcore::curve::curve::Curve;
    use solidcore::math::Vec3;
    use solidcore::primitive::api_curves::{create_curve, create_line_segment};
    use solidcore::sweep::api_brep::create_linear_sweep;
    use std::f64::consts::{FRAC_1_SQRT_2, SQRT_2};
    let length = 2.0 * w * f64::from(n) * SQRT_2;
    let at =
        |s: f64, z: f64| Point3::new(FRAC_1_SQRT_2 * s + 0.5 * w, FRAC_1_SQRT_2 * s - 0.5 * w, z);
    let waves = 4 * n;
    let top: Vec<Point3> = (0..=waves)
        .map(|i| {
            let s = length * (1.0 - f64::from(i) / f64::from(waves));
            at(s, if i % 2 == 0 { 1.4 } else { 2.6 })
        })
        .collect();
    let line = |a: Point3, b: Point3| Curve::from(create_line_segment(a, b).unwrap());
    let curves = [
        line(at(0.0, 0.0), at(length, 0.0)),
        line(at(length, 0.0), at(length, 1.4)),
        Curve::from(create_curve(&top).unwrap()),
        line(at(0.0, 1.4), at(0.0, 0.0)),
    ];
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_linear_sweep(
        k,
        &refs,
        Vec3::new(-FRAC_1_SQRT_2, FRAC_1_SQRT_2, 0.0),
        w,
        true,
    )
    .unwrap()
}

/// The faces marked by the shell walks of the union of the woven `n` x `n` grid with
/// [`wavy_diagonal_strip`] ([`solidcore::kernel::WorkCounters::shell_traversal_faces`]),
/// after checking the union's volume against the grid's and the strip's less their common
/// part's.
fn grid_and_strip_shell_walks(n: u32) -> u64 {
    use solidcore::boolean::api_brep::boolean_intersection;
    let mut k = Kernel::default();
    let w = 1.0;
    let (a, b) = (grid(&mut k, n, w, 0), grid(&mut k, n, w, 1));
    let mut woven = boolean_union(&mut k, a, b).unwrap();
    let mut strip = wavy_diagonal_strip(&mut k, n, w);
    let woven_volume = brep_volume_default(&k, &mut woven, 1e-9).unwrap();
    let strip_volume = brep_volume_default(&k, &mut strip, 1e-9).unwrap();
    let (c, d) = (grid(&mut k, n, w, 0), grid(&mut k, n, w, 1));
    let woven_copy = boolean_union(&mut k, c, d).unwrap();
    let strip_copy = wavy_diagonal_strip(&mut k, n, w);
    let mut common = boolean_intersection(&mut k, woven_copy, strip_copy).unwrap();
    let common_volume = brep_volume_default(&k, &mut common, 1e-9).unwrap();
    k.work_counters.reset();
    let mut u = boolean_union(&mut k, woven, strip).unwrap();
    let marked = k.work_counters.shell_traversal_faces();
    let volume = brep_volume_default(&k, &mut u, 1e-9).unwrap();
    let expected = woven_volume + strip_volume - common_volume;
    println!("{n} x {n} grid and a diagonal strip: {marked} faces marked by shell walks, volume {volume}");
    assert!(
        (volume - expected).abs() <= 1e-9 * expected,
        "volume {volume}, expected {expected}"
    );
    marked
}

/// The union deletes the faces of the grid inside the strip and of the strip inside the
/// grid, one by one, and after each asks whether what the face joined is still one shell:
/// the walk from one of the face's vertices ends when it has reached the others, which the
/// faces next to the deleted one join a step away, and deleting a wire edge left behind
/// asks the same of its two ends. Breadth first, the walks marked 862 and 3194 faces for
/// 4 x 4 and 8 x 8 strips. Depth first, as upstream walks, they wandered over the far side
/// of the shell first, and the wire edges' walks covered the whole shell every time: 4936
/// and 27464 faces.
#[test]
fn deleting_faces_inside_a_union_walks_only_near_the_face() {
    let counts = [4, 8].map(grid_and_strip_shell_walks);
    assert_eq!(counts, [862, 3194]);
}
