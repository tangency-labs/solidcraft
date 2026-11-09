//! The fork's perforated plate (tangency-labs/smlib@edf7414, capi test `ops_kernel`; the
//! case of WR-527, #1526/#1527, #2363), ported: k x k r 2 holes on a 20 mm pitch, k = 8
//! and 16, united in balanced rounds with `boolean_2d` and cut from the plate in 2D.
//! Upstream's work there grows with the square of the holes: `Boolean2D` grows every
//! face's plane to cover the whole sheet, so every pair of faces passed the coincident
//! surface check and classified its edges on the other face, and each hole imprinted in
//! the plate classified every hole imprinted before it.
//!
//! The fork's test checked a ratio of times (`t256 <= 8 t64 + 0.1`), which tripped under
//! load. It now counts the work (`SmWorkCounters`, here [`Kernel::work_counters`]) and
//! prints the time without checking it. With `boolean_skips_faces_apart` the holes' boxes
//! lie apart, so the unions pair no faces, and the cut pairs the plate's one face with each
//! hole: k^2 face pairs, 64 and 256, four times the pairs for four times the holes. Each
//! hole imprinted in the plate lies outside the other holes' boxes, so no inner loop is
//! classified: 0 and 0. With that box test switched off the same run classifies
//! 4032 = 64 x 63 and 65280 = 256 x 255 loops, the n^2 the count is there to catch (the
//! fork's numbers and the port's). With the flag off (upstream) the port also pairs every
//! face, 2080 = 64 x 65 / 2 and 32896 = 256 x 257 / 2 pairs, so the test fails. The plate
//! keeps its exact area either way.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::f64::consts::PI;
use std::time::Instant;

use solidcore::boolean::api_brep::{boolean_2d, Boolean2d, Boolean2dOutcome};
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::mass::integrator::properties::brep_area;
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_trim_ops::create_planar_faces;
use solidcore::{Brep, Kernel};

const RADIUS: f64 = 2.0;
const PITCH: f64 = 20.0;

fn boolean(k: &mut Kernel, a: Brep, b: Brep, op: Boolean2d) -> Brep {
    match boolean_2d(k, a, b, op).unwrap() {
        Boolean2dOutcome::Result(r) => r,
        Boolean2dOutcome::Unassigned { .. } => panic!("2D boolean left its operands"),
    }
}

/// A planar face bounded by the circle r 2 about (x, y, 0) (`smlib_planar_faces` of one
/// full arc: `SmApiCreatePlanarFaces` of an `SmCircle`).
fn disk(k: &mut Kernel, x: f64, y: f64) -> Brep {
    let frame = Axis2Placement::new(Point3::new(x, y, 0.0), Vec3::X, Vec3::Y);
    let circle = Ellipse::create_canonical_circle(k, &frame, RADIUS, None, None).unwrap();
    create_planar_faces(k, &[Curve::from(circle)]).unwrap()
}

/// The `w` x `w` plate from the origin (`smlib_planar_faces` of its four sides).
fn plate(k: &mut Kernel, w: f64) -> Brep {
    let corners = [(0.0, 0.0), (w, 0.0), (w, w), (0.0, w), (0.0, 0.0)];
    let sides: Vec<Curve> = corners
        .windows(2)
        .map(|s| {
            let (start, end) = (s[0], s[1]);
            Curve::from(
                create_line_segment(
                    Point3::new(start.0, start.1, 0.0),
                    Point3::new(end.0, end.1, 0.0),
                )
                .unwrap(),
            )
        })
        .collect();
    create_planar_faces(k, &sides).unwrap()
}

/// The plate with `n` x `n` holes cut in 2D: checks its exact area, its face pairs (n^2)
/// and its inner loops classified (none), and returns the face pairs.
fn plate_pairs(n: u32) -> u64 {
    let mut k = Kernel::default();
    k.fixes.boolean_skips_faces_apart = true;
    let t0 = Instant::now();
    k.work_counters.reset();
    let mut holes = Vec::new();
    for i in 0..n {
        for j in 0..n {
            let (x, y) = (PITCH * f64::from(i + 1), PITCH * f64::from(j + 1));
            holes.push(disk(&mut k, x, y));
        }
    }
    // united in balanced rounds
    while holes.len() > 1 {
        let mut next = Vec::new();
        let mut round = holes.into_iter();
        while let Some(a) = round.next() {
            match round.next() {
                Some(b) => next.push(boolean(&mut k, a, b, Boolean2d::Union)),
                None => next.push(a),
            }
        }
        holes = next;
    }
    let w = PITCH * f64::from(n + 1);
    let plate = plate(&mut k, w);
    let holes = holes.pop().unwrap();
    assert_eq!(holes.face_list().len(), (n * n) as usize);
    let mut cut = boolean(&mut k, plate, holes, Boolean2d::Difference);
    let seconds = t0.elapsed().as_secs_f64();
    let pairs = k.work_counters.face_pairs();
    let loops = k.work_counters.loops_classified();
    let area = brep_area(&k, &mut cut, 1e-8).unwrap();
    let exact = w * w - f64::from(n * n) * PI * RADIUS * RADIUS;
    println!(
        "plate with {} holes in 2D: {seconds:.3} s, area {area:.6}, exact {exact:.6}",
        n * n
    );
    println!("  {pairs} face pairs examined, {loops} inner loops classified");
    assert!(
        (area - exact).abs() <= 1e-7 * exact.abs(),
        "area {area}, exact {exact}"
    );
    assert_eq!((pairs, loops), (u64::from(n * n), 0));
    pairs
}

#[test]
fn fork_edf7414_perforated_plate_work_grows_as_the_holes() {
    let (pairs64, pairs256) = (plate_pairs(8), plate_pairs(16));
    assert_eq!(pairs256, 4 * pairs64);
}
