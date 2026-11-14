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

#[test]
fn boolean_2d_preserves_attributes_from_both_operands() {
    use solidcore::geom::attribute::AttributeValue;
    const A_MARK: u64 = 20_001;
    const B_MARK: u64 = 20_002;
    for op in [
        Boolean2d::Union,
        Boolean2d::Intersection,
        Boolean2d::Difference,
        Boolean2d::ExclusiveOr,
    ] {
        let mut k = Kernel::default();
        let mut a = sheet(&mut k, 0.0, 0.0, 10.0);
        let mut b = sheet(&mut k, 5.0, 5.0, 10.0);
        let mut sources = Vec::new();
        for (operand, id) in [(&mut a, A_MARK), (&mut b, B_MARK)] {
            let mut positions = Vec::new();
            for (index, edge) in operand.edge_list().to_vec().into_iter().enumerate() {
                let vertices = operand.edge(edge).unwrap().vertices;
                positions.push(vertices.map(|v| operand.vertex(v).unwrap().point));
                operand
                    .add_attribute(
                        EntityRef::Edge(edge),
                        Attribute::long(id, index as i64, Behavior::Copy),
                    )
                    .unwrap();
            }
            sources.push(positions);
        }
        let Boolean2dOutcome::Result(result) = boolean_2d(&mut k, a, b, op).unwrap() else {
            panic!("no result")
        };
        let mut counts = [0, 0];
        for &edge in result.edge_list() {
            let vertices = result.edge(edge).unwrap().vertices;
            let ends = vertices.map(|v| result.vertex(v).unwrap().point);
            let attrs = result.entity_attributes(EntityRef::Edge(edge));
            let mut matched = false;
            for (_, attr) in attrs {
                let operand = match attr.attribute_id() {
                    A_MARK => 0,
                    B_MARK => 1,
                    _ => continue,
                };
                let AttributeValue::Long(index) = attr.value() else {
                    panic!("invalid source attribute")
                };
                let [a, b] = sources[operand][*index as usize];
                let v = b - a;
                for point in ends {
                    let u = point - a;
                    assert!(
                        u.cross(v).length() < 1e-6
                            && u.dot(v) >= -1e-6
                            && u.dot(v) <= v.dot(v) + 1e-6,
                        "{op:?}: incorrect source for edge {edge:?}"
                    );
                }
                counts[operand] += 1;
                matched = true;
            }
            assert!(matched, "{op:?}: result edge {edge:?} has no source");
        }
        assert!(
            counts[0] > 0 && counts[1] > 0,
            "{op:?}: missing one operand"
        );
    }
}

#[test]
fn boolean_2d_coincident_edges_keep_both_sources() {
    for op in [Boolean2d::Union, Boolean2d::Intersection] {
        let mut k = Kernel::default();
        let mut a = sheet(&mut k, 0.0, 0.0, 10.0);
        let mut b = sheet(&mut k, 0.0, 0.0, 10.0);
        for (operand, id) in [(&mut a, 20_011), (&mut b, 20_012)] {
            for edge in operand.edge_list().to_vec() {
                operand
                    .add_attribute(
                        EntityRef::Edge(edge),
                        Attribute::long(id, 1, Behavior::Copy),
                    )
                    .unwrap();
            }
        }
        let Boolean2dOutcome::Result(result) = boolean_2d(&mut k, a, b, op).unwrap() else {
            panic!("no result")
        };
        for &edge in result.edge_list() {
            let attrs = result.entity_attributes(EntityRef::Edge(edge));
            assert_eq!(
                attrs
                    .iter()
                    .filter(|(_, attr)| attr.attribute_id() == 20_011)
                    .count(),
                1,
                "{op:?}: expected one A mark"
            );
            assert_eq!(
                attrs
                    .iter()
                    .filter(|(_, attr)| attr.attribute_id() == 20_012)
                    .count(),
                1,
                "{op:?}: expected one B mark"
            );
        }
    }
}
