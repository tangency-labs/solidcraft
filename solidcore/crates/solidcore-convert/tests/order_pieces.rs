//! [`Order`]'s inverse and its pieces (#3137): what SolidCraft's `history::Order` gave, read off
//! the conversion's order. A body whose solids touch along an edge comes back with one record
//! edge per pair of faces that edge separates: each is a piece of that kernel edge.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test

use solidcore::boolean::api_brep::boolean_union;
use solidcore::kernel::Kernel;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore_convert::brep::to_record;

/// Two unit boxes touching along one edge (the line x = y = 1): one body, its shared edge one
/// kernel edge with four uses, two record edges.
#[test]
fn an_edge_solids_touch_along_is_two_pieces() {
    let mut k = Kernel::default();
    let a = create_box(&mut k, Point3::ZERO, 1.0, 1.0, 1.0).unwrap();
    let c = create_box(&mut k, Point3::new(1.0, 1.0, 0.0), 1.0, 1.0, 1.0).unwrap();
    let mut b = boolean_union(&mut k, a, c).unwrap();
    let (record, order) = to_record(&mut k, &mut b).unwrap();
    let pieces = order.pieces(&record).unwrap();
    assert_eq!(pieces.len(), 2, "{pieces:?}");
    assert_eq!(pieces[0].kernel, pieces[1].kernel);
    assert_ne!(pieces[0].edge, pieces[1].edge);
    for p in &pieces {
        assert_eq!(p.faces.len(), 2, "{p:?}");
        assert_eq!(order.edges[p.edge as usize], p.kernel);
        // the faces are those of that record edge's own uses
        let mut uses: Vec<u32> = record.edges[p.edge as usize]
            .uses
            .iter()
            .map(|u| u.face)
            .collect();
        uses.sort_unstable();
        uses.dedup();
        assert_eq!(p.faces, uses, "{p:?}");
    }
    // the two pieces bound four different faces, two of each box
    let mut all: Vec<u32> = pieces.iter().flat_map(|p| p.faces.clone()).collect();
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), 4);
    // the inverses name each entity's first record index
    for (i, f) in order.faces.iter().enumerate() {
        assert_eq!(order.record_faces()[f] as usize, i);
    }
    let first = order.record_edges()[&pieces[0].kernel];
    assert_eq!(first, pieces[0].edge.min(pieces[1].edge));
    for (i, v) in order.vertices.iter().enumerate() {
        assert!(order.record_vertices()[v] as usize <= i);
    }
}

/// A body with no touching edge has no pieces.
#[test]
fn a_box_has_no_pieces() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::ZERO, 1.0, 1.0, 1.0).unwrap();
    let (record, order) = to_record(&mut k, &mut b).unwrap();
    assert!(order.pieces(&record).unwrap().is_empty());
    // one record edge per kernel edge here; in general there are at most as many kernel edges
    // as record edges (a touching edge is copied)
    assert!(order.record_edges().len() <= record.edges.len());
    assert_eq!(order.record_edges().len(), record.edges.len());
}
