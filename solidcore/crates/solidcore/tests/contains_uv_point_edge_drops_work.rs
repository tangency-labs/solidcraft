//! A plate whose sides are split into n segments each, classified at n x n points inside it
//! and at one point on its first side (`face_classify_uv`), n = 4 and 8: the work of the loop
//! containment test (`query::loop::contains_uv_point`) the classification makes for each
//! point (SolidCraft's sheet metal flat patterns, faces of many edges, spent most of each 2D
//! boolean there: the boolean classifies each new edge's midpoint on the face it splits).
//!
//! Upstream's containment test drops the point onto every edge of the loop to see whether it
//! lies on one, up to the edge it lies on: 4n drops for each point inside, 2 for the point on
//! the first side (its segment is the second edge the walk visits), 4n^3 + 2 for the plate,
//! 258 and 2050, which is what this test counts without the box test. A drop finds the point only within the edge's tolerance,
//! so an edge whose box, grown by that tolerance, lies apart from the point is not dropped
//! onto: the points inside lie 10 mm or more from every side, and the point on the first
//! side reaches the box of the one segment it lies on, 1 drop for either plate. Every point
//! keeps its class.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::intersect::point_class::PointClassType;
use solidcore::math::Point3;
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_trim_ops::create_planar_faces;
use solidcore::query::api_queries::{face_classify_uv, surface_closest_point};
use solidcore::topology::face::p2::surface;
use solidcore::{Brep, Kernel};

const WIDTH: f64 = 100.0;

/// The `WIDTH` square plate from the origin, each side split into `n` equal segments.
fn plate(k: &mut Kernel, n: u32) -> Brep {
    let step = WIDTH / f64::from(n);
    let mut corners = Vec::new();
    for (from, along) in [
        ((0.0, 0.0), (1.0, 0.0)),
        ((WIDTH, 0.0), (0.0, 1.0)),
        ((WIDTH, WIDTH), (-1.0, 0.0)),
        ((0.0, WIDTH), (0.0, -1.0)),
    ] {
        for i in 0..n {
            let d = step * f64::from(i);
            corners.push((from.0 + along.0 * d, from.1 + along.1 * d));
        }
    }
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

/// The plate of `4n` edges classified at `n` x `n` points inside it and at the middle of its
/// first segment: checks each class and returns the containment test's edge drops.
fn edge_drops(n: u32) -> u64 {
    let mut k = Kernel::default();
    let mut b = plate(&mut k, n);
    let f = b.face_list()[0];
    let srf = surface(&b, f).unwrap().unwrap().clone();
    let pitch = (WIDTH - 20.0) / f64::from(n - 1);
    let mut points = Vec::new();
    for i in 0..n {
        for j in 0..n {
            let p = Point3::new(
                10.0 + pitch * f64::from(i),
                10.0 + pitch * f64::from(j),
                0.0,
            );
            points.push((p, PointClassType::Face));
        }
    }
    points.push((
        Point3::new(WIDTH / f64::from(2 * n), 0.0, 0.0),
        PointClassType::Edge,
    ));
    let uvs: Vec<_> = points
        .iter()
        .map(|(p, _)| surface_closest_point(&k, &srf, *p).unwrap().uvs[0])
        .collect();
    k.work_counters.reset();
    for ((p, class), uv) in points.iter().zip(uvs) {
        assert_eq!(
            face_classify_uv(&k, &mut b, f, uv).unwrap(),
            *class,
            "at {p:?}"
        );
    }
    let drops = k.work_counters.containment_edge_drops();
    println!(
        "plate of {} edges at {} points: {drops} edge drops",
        4 * n,
        points.len()
    );
    drops
}

#[test]
fn containment_drops_onto_the_edges_near_the_point_only() {
    assert_eq!((edge_drops(4), edge_drops(8)), (1, 1));
}
