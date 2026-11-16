//! tangency-labs/solidcore#1462 (SolidCraft's boolean2d history): `boolean_2d` carries
//! the primary operand's attributes through the Boolean, as upstream's live on the
//! objects `SmPrimitiveCreation::Boolean2D` splits, glues and keeps: A - B keeps the
//! face's mark, and both pieces of a split edge keep theirs.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::cast_possible_truncation
)] // test helpers

use solidcore::boolean::api_brep::{boolean_2d, Boolean2dOutcome};
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{Attribute, Behavior};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::primitive_creation::p1::Boolean2d;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::Brep;
use solidcore::{EntityRef, Kernel};

const MARK: u64 = 20_000;

fn sheet(k: &mut Kernel, x: f64, y: f64, s: f64) -> Brep {
    let c = Curve::from(
        create_line_segment(Point3::new(x, y, 0.0), Point3::new(x + s, y, 0.0)).unwrap(),
    );
    create_linear_sweep(k, &[Some(&c)], Vec3::new(0.0, 1.0, 0.0), s, false).unwrap()
}

fn marked(b: &Brep, e: EntityRef) -> bool {
    b.entity_attributes(e)
        .into_iter()
        .any(|(_, a)| a.attribute_id() == MARK)
}

/// The end points of each marked edge, each pair sorted, the list sorted.
fn marked_edges(r: &Brep) -> Vec<[(i64, i64); 2]> {
    let mut out = Vec::new();
    for &e in r.edge_list() {
        if marked(r, EntityRef::Edge(e)) {
            let ends = r.edge(e).unwrap().vertices.map(|v| {
                let p = r.vertex(v).unwrap().point;
                (p.x.round() as i64, p.y.round() as i64)
            });
            let mut ends = ends;
            ends.sort_unstable();
            out.push(ends);
        }
    }
    out.sort_unstable();
    out
}

fn run(op: Boolean2d) -> Brep {
    let mut k = Kernel::default();
    let mut a = sheet(&mut k, 0.0, 0.0, 10.0);
    for f in a.face_list().to_vec() {
        a.add_attribute(EntityRef::Face(f), Attribute::long(MARK, 1, Behavior::Copy))
            .unwrap();
    }
    for e in a.edge_list().to_vec() {
        a.add_attribute(EntityRef::Edge(e), Attribute::long(MARK, 2, Behavior::Copy))
            .unwrap();
    }
    let b = sheet(&mut k, 5.0, 5.0, 10.0);
    let Boolean2dOutcome::Result(r) = boolean_2d(&mut k, a, b, op).unwrap() else {
        panic!("no result")
    };
    r
}

#[test]
fn boolean_2d_difference_keeps_the_face_and_edge_marks() {
    let r = run(Boolean2d::Difference);
    assert_eq!(r.face_list().len(), 1);
    assert!(marked(&r, EntityRef::Face(r.face_list()[0])));
    assert_eq!(
        marked_edges(&r),
        vec![
            [(0, 0), (0, 10)],
            [(0, 0), (10, 0)],
            [(0, 10), (5, 10)],
            [(10, 0), (10, 5)],
        ]
    );
}

#[test]
fn boolean_2d_union_keeps_both_pieces_of_a_split_edge_marked() {
    let r = run(Boolean2d::Union);
    assert_eq!(
        marked_edges(&r),
        vec![
            [(0, 0), (0, 10)],
            [(0, 0), (10, 0)],
            [(0, 10), (5, 10)],
            [(10, 0), (10, 5)],
        ]
    );
}

#[test]
fn boolean_2d_intersection_keeps_the_split_edges_marks() {
    let r = run(Boolean2d::Intersection);
    // the overlap [5,10]^2: A's x=10 and y=10 pieces bound it
    assert_eq!(
        marked_edges(&r),
        vec![[(5, 10), (10, 10)], [(10, 5), (10, 10)]]
    );
}
