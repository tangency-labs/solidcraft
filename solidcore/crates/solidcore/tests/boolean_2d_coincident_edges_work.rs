//! A plate whose sides are split into n segments each, less n x n square holes cut in 2D
//! (`boolean_2d`, the holes united in balanced rounds first), n = 4 and 8: the work of
//! `FaceIntersect` on the coincident faces it pairs (SolidCraft's sheet metal flat patterns,
//! a panel of many edges less its bends' trims, spent most of each 2D boolean there).
//!
//! `Boolean2D` pairs the plate's face with each hole's (their boxes overlap). Upstream's
//! `FaceIntersect` of two coincident faces copies every edge of both and classifies each on
//! both faces: the plate's 4n edges again for every hole, (4n + 4) n^2 curves, 320 and 2304,
//! n^3, which is what this test counts without the box test. An edge whose box lies apart
//! from the other face's box lies inside no part of it and is left out: the plate's sides
//! lie 18 mm from every hole, so each pair classifies its hole's 4 edges only, 4 n^2 curves,
//! 64 and 256, four times the curves for four times the holes. The plate keeps its exact
//! area either way.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::{boolean_2d, Boolean2d, Boolean2dOutcome};
use solidcore::curve::curve::Curve;
use solidcore::mass::integrator::properties::brep_area;
use solidcore::math::Point3;
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_trim_ops::create_planar_faces;
use solidcore::{Brep, Kernel};

const SIDE: f64 = 4.0;
const PITCH: f64 = 20.0;

fn boolean(k: &mut Kernel, a: Brep, b: Brep, op: Boolean2d) -> Brep {
    match boolean_2d(k, a, b, op).unwrap() {
        Boolean2dOutcome::Result(r) => r,
        Boolean2dOutcome::Unassigned { .. } => panic!("2D boolean left its operands"),
    }
}

/// A planar face bounded by the closed polygon through `corners`.
fn polygon(k: &mut Kernel, corners: &[(f64, f64)]) -> Brep {
    let sides: Vec<Curve> = (0..corners.len())
        .map(|i| {
            let (a, b) = (corners[i], corners[(i + 1) % corners.len()]);
            Curve::from(
                create_line_segment(Point3::new(a.0, a.1, 0.0), Point3::new(b.0, b.1, 0.0))
                    .unwrap(),
            )
        })
        .collect();
    create_planar_faces(k, &sides).unwrap()
}

/// The `w` x `w` plate from the origin, each side split into `n` equal segments.
fn plate(k: &mut Kernel, w: f64, n: u32) -> Brep {
    let step = w / f64::from(n);
    let mut corners = Vec::new();
    for (from, along) in [
        ((0.0, 0.0), (1.0, 0.0)),
        ((w, 0.0), (0.0, 1.0)),
        ((w, w), (-1.0, 0.0)),
        ((0.0, w), (0.0, -1.0)),
    ] {
        for i in 0..n {
            let d = step * f64::from(i);
            corners.push((from.0 + along.0 * d, from.1 + along.1 * d));
        }
    }
    polygon(k, &corners)
}

/// The plate with `n` x `n` square holes cut in 2D: checks its exact area and returns the
/// coincident edge curves `FaceIntersect` classified.
fn curves_classified(n: u32) -> u64 {
    let mut k = Kernel::default();
    k.fixes.boolean_skips_faces_apart = true;
    k.work_counters.reset();
    let h = SIDE / 2.0;
    let mut holes = Vec::new();
    for i in 0..n {
        for j in 0..n {
            let (x, y) = (PITCH * f64::from(i + 1), PITCH * f64::from(j + 1));
            holes.push(polygon(
                &mut k,
                &[
                    (x - h, y - h),
                    (x + h, y - h),
                    (x + h, y + h),
                    (x - h, y + h),
                ],
            ));
        }
    }
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
    let plate = plate(&mut k, w, n);
    let mut cut = boolean(&mut k, plate, holes.pop().unwrap(), Boolean2d::Difference);
    let curves = k.work_counters.coincident_curves_classified();
    let area = brep_area(&k, &mut cut, 1e-8).unwrap();
    let exact = w * w - f64::from(n * n) * SIDE * SIDE;
    println!(
        "plate less {} holes: {curves} coincident curves classified, area {area}",
        n * n
    );
    assert!(
        (area - exact).abs() <= 1e-9 * exact,
        "area {area}, exact {exact}"
    );
    curves
}

#[test]
fn coincident_faces_classify_only_the_edges_near_each_other() {
    assert_eq!((curves_classified(4), curves_classified(8)), (64, 256));
}
