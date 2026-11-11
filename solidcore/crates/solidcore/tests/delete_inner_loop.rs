//! `SmFace::DeleteInnerLoop` (`topology::face::p1::delete_inner_loop`): a planar face
//! with three square holes (`SmApiCreatePlanarFaces`) loses its holes one by one; its
//! area grows by each hole's area up to the outer rectangle's, the B-rep checks clean
//! after every step and the deleted loops' edges, vertices and uses are gone. The outer
//! loop and a loop already deleted are refused with `SM_ERR`, and an attribute on a
//! hole edge goes with the edge while one on an outer edge stays.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{Attribute, Behavior};
use solidcore::mass::integrator::properties::face_area;
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_trim_ops::create_planar_faces;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::face::p1::delete_inner_loop_with_kernel_callbacks;
use solidcore::topology::face::p2 as fp2;
use solidcore::topology::{Brep, FaceId, LoopId};
use solidcore::{EntityRef, Kernel, SolidError, Vec3};

/// The closed polygon through `points` as line segments.
fn polygon(points: &[(f64, f64)]) -> Vec<Curve> {
    (0..points.len())
        .map(|i| {
            let (x0, y0) = points[i];
            let (x1, y1) = points[(i + 1) % points.len()];
            Curve::from(
                create_line_segment(Vec3::new(x0, y0, 0.0), Vec3::new(x1, y1, 0.0)).unwrap(),
            )
        })
        .collect()
}

fn square(x: f64, y: f64, s: f64) -> Vec<Curve> {
    polygon(&[(x, y), (x + s, y), (x + s, y + s), (x, y + s)])
}

/// A 10 x 6 plate with holes of areas 1, 4 and 2.25.
fn plate(k: &mut Kernel) -> Brep {
    let mut curves = polygon(&[(0.0, 0.0), (10.0, 0.0), (10.0, 6.0), (0.0, 6.0)]);
    curves.extend(square(1.0, 1.0, 1.0));
    curves.extend(square(3.0, 2.0, 2.0));
    curves.extend(square(7.0, 3.0, 1.5));
    create_planar_faces(k, &curves).unwrap()
}

fn only_face(b: &Brep) -> FaceId {
    let faces = b.face_list();
    assert_eq!(faces.len(), 1);
    faces[0]
}

fn area(k: &Kernel, b: &mut Brep, f: FaceId) -> f64 {
    face_area(k, b, f, 1e-9).unwrap()
}

fn assert_clean(k: &mut Kernel, b: &mut Brep) {
    let c = check_brep(k, b, CheckLevel::Level2).unwrap();
    assert_eq!(c.failure_count(), 0, "{:?}", c.lines());
    assert!(c.is_valid());
}

fn is_sm_err(r: &Result<(), SolidError>) -> bool {
    matches!(r, Err(SolidError::Upstream { code, .. }) if *code == solidcore::error::SM_ERR)
}

/// The (loops, loop uses, edges, edge uses, vertices, vertex uses) records alive.
fn counts(b: &Brep) -> [usize; 6] {
    [
        b.loops().len(),
        b.loopuses().len(),
        b.edges().len(),
        b.edgeuses().len(),
        b.vertices().len(),
        b.vertexuses().len(),
    ]
}

#[test]
fn deleting_every_hole_gives_the_outer_area_and_a_clean_brep() {
    let mut k = Kernel::default();
    let mut b = plate(&mut k);
    let f = only_face(&b);
    assert_clean(&mut k, &mut b);
    let loops = fp2::loops(&b, f, false, None).unwrap();
    assert_eq!(loops.len(), 4);
    // (the arenas also hold the records CreateFacesWithCurves left unlinked: compare
    // what each deletion removes)
    let start = counts(&b);
    assert_eq!([start[2], start[3], start[4]], [16, 32, 16]);
    let outer = loops[0];
    let holes: Vec<LoopId> = loops[1..].to_vec();

    let mut expected = 60.0 - 1.0 - 4.0 - 2.25;
    let a = area(&k, &mut b, f);
    assert!((a - expected).abs() < 1e-6, "{a} vs {expected}");

    // the hole areas by loop: find each hole's area from the area gained
    let mut gained = Vec::new();
    for (i, &l) in holes.iter().enumerate() {
        delete_inner_loop_with_kernel_callbacks(&mut k, &mut b, f, l).unwrap();
        let after = area(&k, &mut b, f);
        assert!(
            after > expected,
            "area {after} did not grow from {expected}"
        );
        gained.push(after - expected);
        expected = after;
        assert_clean(&mut k, &mut b);
        let left = 3 - i;
        assert_eq!(fp2::loops(&b, f, false, None).unwrap().len(), left);
        let gone = 3 - left + 1;
        let now = counts(&b);
        let removed: Vec<usize> = start.iter().zip(now).map(|(s, n)| s - n).collect();
        assert_eq!(
            removed,
            vec![gone, 2 * gone, 4 * gone, 8 * gone, 4 * gone, 8 * gone]
        );
    }
    gained.sort_by(f64::total_cmp);
    for (g, h) in gained.iter().zip([1.0, 2.25, 4.0]) {
        assert!((g - h).abs() < 1e-6, "gained {g} for a hole of {h}");
    }
    let a = area(&k, &mut b, f);
    assert!((a - 60.0).abs() < 1e-6, "{a}");
    assert_eq!(fp2::loops(&b, f, false, None).unwrap(), vec![outer]);
}

#[test]
fn the_outer_loop_and_a_deleted_loop_are_refused() {
    let mut k = Kernel::default();
    let mut b = plate(&mut k);
    let f = only_face(&b);
    let loops = fp2::loops(&b, f, false, None).unwrap();
    let before = counts(&b);
    // Outer loop?
    let r = delete_inner_loop_with_kernel_callbacks(&mut k, &mut b, f, loops[0]);
    assert!(is_sm_err(&r), "{r:?}");
    assert_eq!(counts(&b), before);
    delete_inner_loop_with_kernel_callbacks(&mut k, &mut b, f, loops[2]).unwrap();
    // One of our loops? (no longer)
    let r = delete_inner_loop_with_kernel_callbacks(&mut k, &mut b, f, loops[2]);
    assert!(is_sm_err(&r), "{r:?}");
    assert_clean(&mut k, &mut b);
}

#[test]
fn a_hole_edge_takes_its_attribute_with_it() {
    let mut k = Kernel::default();
    let mut b = plate(&mut k);
    let f = only_face(&b);
    let loops = fp2::loops(&b, f, false, None).unwrap();
    let outer_edge = solidcore::topology::loopuse::loop_edges(&b, loops[0]).unwrap()[0].unwrap();
    let hole_edge = solidcore::topology::loopuse::loop_edges(&b, loops[1]).unwrap()[0].unwrap();
    b.add_attribute(
        EntityRef::Edge(outer_edge),
        Attribute::new(20001, Behavior::Copy),
    )
    .unwrap();
    b.add_attribute(
        EntityRef::Edge(hole_edge),
        Attribute::new(20002, Behavior::Copy),
    )
    .unwrap();
    b.add_attribute(EntityRef::Face(f), Attribute::new(20003, Behavior::Copy))
        .unwrap();
    delete_inner_loop_with_kernel_callbacks(&mut k, &mut b, f, loops[1]).unwrap();
    assert!(b.edge(hole_edge).is_err());
    assert!(b
        .find_attribute(EntityRef::Edge(outer_edge), 20001)
        .is_some());
    assert!(b.find_attribute(EntityRef::Face(f), 20003).is_some());
    assert!(b
        .find_attribute(EntityRef::Edge(hole_edge), 20002)
        .is_none());
}
