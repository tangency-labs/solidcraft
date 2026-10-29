//! Issue #1462: caller attributes on operand faces and edges reach the boolean's result
//! as SMLib's default handlers carry them (split: both pieces; copy B -> A: the copy;
//! merge: the survivor gains the ids it lacks). The B-rep owns its attributes
//! (`SmAObject::m_pAttributes`), so the operations keep their signatures: tag the
//! operands, run the boolean, read the result.
//!
//! Each operand face and edge gets its own attribute id (one id per origin, as
//! SolidCraft tags operands), so a result object's origins are its attribute ids.

#![allow(clippy::expect_used, clippy::indexing_slicing)] // test helpers

use solidcore::boolean::api_brep::{boolean_difference, boolean_intersection, boolean_union};
use solidcore::geom::attribute::{Attribute, Behavior};
use solidcore::mass::integrator::properties::{edge_length, face_area};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::Brep;
use solidcore::{EntityRef, Kernel};

/// The first application attribute id SolidCraft uses.
const BASE_ID: u64 = 10007;

/// An origin `(operand, kind, index)`.
type Origin = (u64, u64, u64);
/// Per face: (area in 1e-6 units, origins).
type FaceSummary = Vec<(i64, Vec<Origin>)>;

/// Origin `(operand, kind, index)` encoded in the attribute id: kind 0 face, 1 edge.
fn origin_id(operand: u64, kind: u64, index: u64) -> u64 {
    BASE_ID + operand * 1000 + kind * 500 + index
}

fn decode(id: u64) -> Origin {
    let raw = id - BASE_ID;
    (raw / 1000, raw % 1000 / 500, raw % 500)
}

fn tag(b: &mut Brep, operand: u64, behavior: Behavior) {
    let faces = b.face_list().to_vec();
    for (i, f) in faces.into_iter().enumerate() {
        let id = origin_id(operand, 0, i as u64);
        b.add_attribute(EntityRef::Face(f), Attribute::long(id, id as i64, behavior))
            .expect("tag face");
    }
    let edges = b.edge_list().to_vec();
    for (i, e) in edges.into_iter().enumerate() {
        let id = origin_id(operand, 1, i as u64);
        b.add_attribute(EntityRef::Edge(e), Attribute::long(id, id as i64, behavior))
            .expect("tag edge");
    }
}

fn origins(b: &Brep, e: EntityRef) -> Vec<Origin> {
    let mut out: Vec<_> = b
        .entity_attributes(e)
        .into_iter()
        .filter(|(_, a)| a.attribute_id() >= BASE_ID)
        .map(|(_, a)| {
            assert_eq!(a.long_value(), Some(a.attribute_id() as i64));
            decode(a.attribute_id())
        })
        .collect();
    out.sort_unstable();
    out
}

fn boxes(k: &mut Kernel, behavior: Behavior) -> (Brep, Brep) {
    let mut a = create_box(k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).expect("box a");
    let mut b = create_box(k, Point3::new(5.0, 5.0, 5.0), 10.0, 10.0, 10.0).expect("box b");
    tag(&mut a, 0, behavior);
    tag(&mut b, 1, behavior);
    (a, b)
}

/// Per face: (area rounded to 1e-6, origins), sorted.
fn face_summary(k: &Kernel, r: &mut Brep) -> FaceSummary {
    let faces = r.face_list().to_vec();
    let mut out: Vec<_> = faces
        .into_iter()
        .map(|f| {
            let area = face_area(k, r, f, 1e-7).expect("face area");
            ((area * 1e6).round() as i64, origins(r, EntityRef::Face(f)))
        })
        .collect();
    out.sort();
    out
}

fn edge_origin_counts(r: &Brep) -> (usize, usize, usize) {
    let mut none = 0;
    let mut one = 0;
    let mut more = 0;
    for &e in r.edge_list() {
        let o = origins(r, EntityRef::Edge(e));
        assert!(
            o.iter().all(|&(_, kind, _)| kind == 1),
            "edge with a face origin: {o:?}"
        );
        match o.len() {
            0 => none += 1,
            1 => one += 1,
            _ => more += 1,
        }
    }
    (none, one, more)
}

fn check_faces(summary: &[(i64, Vec<Origin>)], expected: &[(f64, u64, usize)]) {
    // expected: (area, operand, how many faces)
    let mut seen = Vec::new();
    for (area, o) in summary {
        assert_eq!(o.len(), 1, "face of area {area} has origins {o:?}");
        assert_eq!(o[0].1, 0, "face carries an edge origin {o:?}");
        seen.push((*area, o[0].0));
    }
    let mut want = Vec::new();
    for &(area, operand, n) in expected {
        for _ in 0..n {
            want.push(((area * 1e6).round() as i64, operand));
        }
    }
    seen.sort_unstable();
    want.sort_unstable();
    assert_eq!(seen, want);
}

#[test]
fn union_of_overlapping_boxes_keeps_one_origin_per_face() {
    let mut k = Kernel::default();
    let (a, b) = boxes(&mut k, Behavior::Copy);
    let mut r = boolean_union(&mut k, a, b).expect("union");
    assert_eq!(r.face_list().len(), 12);
    let summary = face_summary(&k, &mut r);
    // A's faces at x/y/z = 0 and B's at 15 are whole; the six facing ones are L-shaped.
    check_faces(
        &summary,
        &[(100.0, 0, 3), (75.0, 0, 3), (100.0, 1, 3), (75.0, 1, 3)],
    );
    // every original face shows up exactly once
    let mut all: Vec<_> = summary.iter().map(|(_, o)| o[0]).collect();
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), 12);
    let (none, _, more) = edge_origin_counts(&r);
    assert_eq!(none, 6, "the six new edges where the boxes' faces cross");
    assert_eq!(more, 0);
}

#[test]
fn difference_of_overlapping_boxes() {
    let mut k = Kernel::default();
    let (a, b) = boxes(&mut k, Behavior::Copy);
    let mut r = boolean_difference(&mut k, a, b).expect("difference");
    assert_eq!(r.face_list().len(), 9);
    let summary = face_summary(&k, &mut r);
    // A keeps its three faces at 0 whole and its three at 10 L-shaped; B's three faces at
    // 5 bound the pocket.
    check_faces(&summary, &[(100.0, 0, 3), (75.0, 0, 3), (25.0, 1, 3)]);
    let (none, _, more) = edge_origin_counts(&r);
    assert_eq!(none, 6);
    assert_eq!(more, 0);
}

#[test]
fn intersection_of_overlapping_boxes() {
    let mut k = Kernel::default();
    let (a, b) = boxes(&mut k, Behavior::Copy);
    let mut r = boolean_intersection(&mut k, a, b).expect("intersection");
    assert_eq!(r.face_list().len(), 6);
    let summary = face_summary(&k, &mut r);
    check_faces(&summary, &[(25.0, 0, 3), (25.0, 1, 3)]);
    let (none, _, more) = edge_origin_counts(&r);
    assert_eq!(none, 6);
    assert_eq!(more, 0);
}

#[test]
fn edges_crossed_by_the_other_operand_keep_their_origin_on_the_surviving_piece() {
    let mut k = Kernel::default();
    let (a, b) = boxes(&mut k, Behavior::Copy);
    let r = boolean_union(&mut k, a, b).expect("union");
    // 12 + 12 operand edges (the three of each box crossing the other keep their
    // outside half) and 6 new ones
    assert_eq!(r.edge_list().len(), 30);
    let mut seen = Vec::new();
    let mut halves = [0usize; 2];
    for &e in r.edge_list() {
        let o = origins(&r, EntityRef::Edge(e));
        let length = edge_length(&k, &r, e, 1e-7).expect("edge length");
        match o.as_slice() {
            [] => assert!((length - 5.0).abs() < 1e-9, "new edge of length {length}"),
            [(operand, 1, _)] => {
                if (length - 5.0).abs() < 1e-9 {
                    halves[*operand as usize] += 1;
                } else {
                    assert!((length - 10.0).abs() < 1e-9, "edge of length {length}");
                }
                seen.push(o[0]);
            }
            _ => panic!("edge with origins {o:?}"),
        }
    }
    assert_eq!(halves, [3, 3], "the crossed edges of each box");
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), 24, "every operand edge shows up exactly once");
}

#[test]
fn temporary_attributes_are_not_propagated_and_references_stay_shared() {
    let mut k = Kernel::default();
    let mut a = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).expect("a");
    let b = create_box(&mut k, Point3::new(5.0, 5.0, 5.0), 10.0, 10.0, 10.0).expect("b");
    let faces = a.face_list().to_vec();
    let shared = a
        .add_attribute(
            EntityRef::Face(faces[0]),
            Attribute::long(20001, 1, Behavior::Reference),
        )
        .expect("reference");
    for &f in &faces[1..] {
        a.attach_attribute(EntityRef::Face(f), shared)
            .expect("attach");
    }
    for &f in &faces {
        a.add_attribute(
            EntityRef::Face(f),
            Attribute::long(20002, 2, Behavior::Temp),
        )
        .expect("temp");
    }
    let r = boolean_union(&mut k, a, b).expect("union");
    let with_ref: Vec<_> = r
        .face_list()
        .iter()
        .filter(|&&f| r.find_attribute(EntityRef::Face(f), 20001).is_some())
        .copied()
        .collect();
    assert_eq!(
        with_ref.len(),
        6,
        "A's six faces (whole or trimmed) share the reference"
    );
    let handles: Vec<_> = with_ref
        .iter()
        .map(|&f| {
            r.entity_attributes(EntityRef::Face(f))
                .into_iter()
                .find(|(_, a)| a.attribute_id() == 20001)
                .map(|(h, _)| h)
                .expect("handle")
        })
        .collect();
    assert!(
        handles.windows(2).all(|w| w[0] == w[1]),
        "one shared attribute"
    );
    let users = r.attribute(handles[0]).expect("live").users().len();
    assert_eq!(users, 6);
    // A temporary attribute is never passed on: only A's three faces the union leaves
    // whole keep theirs (as the C++ reference does)
    let mut r = r;
    let mut temp = Vec::new();
    for f in r.face_list().to_vec() {
        if r.find_attribute(EntityRef::Face(f), 20002).is_some() {
            temp.push((face_area(&k, &mut r, f, 1e-7).expect("area") * 1e6).round() as i64);
        }
    }
    assert_eq!(temp, vec![100_000_000; 3]);
}

#[test]
fn coplanar_faces_merged_by_the_cleanup_keep_both_origins() {
    // B shares A's z = 0 and z = 10 planes: each L-shaped result cap is one face made of
    // a face of each box, merged by the boolean's cleanup (`DeleteEdge`), and the
    // survivor gains the other face's id (SMLib's default Merge handler).
    let mut k = Kernel::default();
    let mut a = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).expect("a");
    let mut b = create_box(&mut k, Point3::new(5.0, 5.0, 0.0), 10.0, 10.0, 10.0).expect("b");
    tag(&mut a, 0, Behavior::Copy);
    tag(&mut b, 1, Behavior::Copy);
    let mut r = boolean_union(&mut k, a, b).expect("union");
    assert_eq!(r.face_list().len(), 10);
    let summary = face_summary(&k, &mut r);
    let caps: Vec<_> = summary
        .iter()
        .filter(|(area, _)| *area == 175_000_000)
        .map(|(_, o)| o.clone())
        .collect();
    // bottom: faces 0 of both boxes; top: faces 1 of both boxes
    assert_eq!(
        caps,
        vec![vec![(0, 0, 0), (1, 0, 0)], vec![(0, 0, 1), (1, 0, 1)]]
    );
    for (area, o) in &summary {
        if *area != 175_000_000 {
            assert_eq!(o.len(), 1, "side face of area {area}: {o:?}");
        }
    }
}

#[test]
fn clone_copies_attributes_and_dead_entities_have_none() {
    let mut k = Kernel::default();
    let (a, _) = boxes(&mut k, Behavior::Copy);
    let c = a.clone();
    let f = a.face_list()[0];
    assert_eq!(origins(&c, EntityRef::Face(f)), vec![(0, 0, 0)]);
    let mut d = a.clone();
    assert!(d.clear_attributes(EntityRef::Face(f)).is_ok());
    assert!(d.find_attribute(EntityRef::Face(f), BASE_ID).is_none());
    assert!(a.find_attribute(EntityRef::Face(f), BASE_ID).is_some());
}
