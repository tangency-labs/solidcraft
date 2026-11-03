use super::*;
use solidcraft_geom::brep::history::{Change, EdgeOrigin, EdgeProv, FaceOrigin, FaceProv, History};

fn face(operand: usize, face: usize) -> FaceOrigin {
    FaceOrigin::Face { operand, face }
}

/// Two boxes joined across a face, as `smlib_history_read` lays it out: face 0 is operand 0's
/// face 2 kept, face 1 operand 1's face 4 trimmed, face 2 both boxes' coplanar faces 0 merged.
/// Edge 0 is new between faces 0 and 1, edge 1 is operand 0's edge 7, edge 2 joins edge 3 of
/// each box and bounds faces 1 and 2.
const RECORDS: &[i64] = &[
    1, 3, 3, //
    0, 1, 0, 2, //
    1, 1, 1, 4, //
    3, 2, 0, 0, 1, 0, //
    2, 0, 1, 0, //
    2, 0, 2, 1, 0, 7, //
    2, 1, 2, 2, 0, 3, 1, 3,
];

#[test]
fn decodes_the_c_layout() {
    let raw = Raw::decode(RECORDS).unwrap();
    assert_eq!(raw.faces.len(), 3);
    assert_eq!(raw.faces[2], RawFace { change: Change::Merged, origins: vec![(0, 0), (1, 0)] });
    assert_eq!(raw.edges[0], RawEdge { faces: vec![0, 1], origins: vec![] });
    assert_eq!(raw.edges[2].origins, vec![(0, 3), (1, 3)]);
}

#[test]
fn hostile_records_are_refused() {
    // Wrong version, truncated, trailing values, counts far past the data, an unknown change.
    assert_eq!(Raw::decode(&[2, 0, 0]), None);
    assert_eq!(Raw::decode(&RECORDS[..RECORDS.len() - 1]), None);
    assert_eq!(Raw::decode(&[RECORDS, &[0]].concat()), None);
    assert_eq!(Raw::decode(&[1, i64::MAX, 0]), None);
    assert_eq!(Raw::decode(&[1, 1, 0, 0, i64::MAX]), None);
    assert_eq!(Raw::decode(&[1, 1, 0, 9, 0]), None);
    assert_eq!(Raw::decode(&[1, 1, 0, 0, -1]), None);
    assert_eq!(Raw::decode(&[]), None);
}

#[test]
fn new_edges_are_where_their_faces_meet() {
    let raw = Raw::decode(RECORDS).unwrap();
    let h = raw.history(&[Order::default(), Order::default()], &Order::default()).unwrap();
    assert_eq!(h.operands, 2);
    assert_eq!(h.faces[0].origins, vec![face(0, 2)]);
    assert_eq!(h.faces[0].change, Change::Kept);
    assert_eq!(h.edges[0].origins, vec![EdgeOrigin::meet(face(0, 2), face(1, 4))]);
    assert_eq!(h.edges[1].origins, vec![EdgeOrigin::Edge { operand: 0, edge: 7 }]);
    assert_eq!(h.edges[2].origins, vec![EdgeOrigin::Edge { operand: 0, edge: 3 }, EdgeOrigin::Edge { operand: 1, edge: 3 }]);
    assert_eq!(h.edges[2].faces, vec![1, 2]);
    h.check_counts((3, 3), &[(3, 8), (5, 4)]).unwrap();
}

/// A face split in two (a split line across operand 0's face 0): the new edge lies between two
/// pieces of one face, where nothing met, so it comes from nothing.
#[test]
fn a_split_line_comes_from_nothing() {
    let raw = Raw::decode(&[1, 2, 1, 2, 1, 0, 0, 2, 1, 0, 0, 2, 0, 1, 0]).unwrap();
    let h = raw.history(&[Order::default()], &Order::default()).unwrap();
    assert_eq!(h.faces.iter().map(|f| (f.origins.clone(), f.change)).collect::<Vec<_>>(), vec![(vec![face(0, 0)], Change::Split); 2]);
    assert_eq!(h.edges[0].faces, vec![0, 1]);
    assert!(h.edges[0].origins.is_empty(), "{:?}", h.edges[0].origins);
}

#[test]
fn renumbers_into_canonical_order() {
    let raw = Raw::decode(RECORDS).unwrap();
    // Operand 0's SMLib face 2 is its canonical face 5, edge 7 its edge 0; the result's faces
    // and edges are listed backwards.
    let a = Order { faces: vec![1, 2, 5, 0, 3, 4], edges: vec![1, 2, 3, 4, 5, 6, 7, 0], pieces: vec![] };
    let b = Order { faces: vec![0, 1, 2, 3, 4], edges: vec![0, 1, 2, 3], pieces: vec![] };
    let result = Order { faces: vec![2, 1, 0], edges: vec![2, 1, 0], pieces: vec![] };
    let h = raw.history(&[a, b], &result).unwrap();
    assert_eq!(h.faces[2].origins, vec![face(0, 5)]);
    assert_eq!(h.faces[0].origins, vec![face(0, 1), face(1, 0)]);
    assert_eq!(h.edges[1].origins, vec![EdgeOrigin::Edge { operand: 0, edge: 0 }]);
    // The new edge now bounds result faces 2 and 1, and meets the renumbered origins.
    assert_eq!(h.edges[2].faces, vec![1, 2]);
    assert_eq!(h.edges[2].origins, vec![EdgeOrigin::meet(face(0, 5), face(1, 4))]);
}

#[test]
fn bad_orders_are_errors() {
    let raw = Raw::decode(RECORDS).unwrap();
    let ok = Order::default();
    // An origin on an operand that isn't there.
    assert!(raw.history(std::slice::from_ref(&ok), &ok).is_err());
    // An operand order too short for an origin.
    assert!(raw.history(&[Order { faces: vec![0], edges: vec![], pieces: vec![] }, ok.clone()], &ok).is_err());
    // A result order that isn't a permutation.
    assert!(raw.history(&[ok.clone(), ok.clone()], &Order { faces: vec![0, 0, 1], edges: vec![], pieces: vec![] }).is_err());
    assert!(raw.history(&[ok.clone(), ok.clone()], &Order { faces: vec![0, 1, 3], edges: vec![], pieces: vec![] }).is_err());
}

/// A box with its edge 0 (between faces 4 and 5) filleted, as SMLib reports it: the fillet face
/// (6) has no tag; it lies between faces from 4 and 5 and two end faces.
#[test]
fn a_fillet_face_comes_from_its_edge() {
    let kept = |i: usize| FaceProv { origins: vec![face(0, i)], change: Change::Modified };
    let mut faces: Vec<FaceProv> = (0..6).map(kept).collect();
    faces.push(FaceProv { origins: vec![], change: Change::Generated });
    let edge = |a: usize, b: usize| EdgeProv { faces: vec![a, b], origins: vec![] };
    let mut h = History { operands: 1, faces, edges: vec![edge(4, 6), edge(5, 6), edge(0, 6), edge(2, 6), edge(0, 4)] };
    // The operand's own edges: edge 0 between faces 4 and 5, edge 1 between 0 and 4.
    let operand = vec![edge(4, 5), edge(0, 4)];
    blends(&mut h, &operand, &[0], &[1]);
    assert_eq!(h.faces[6].origins, vec![FaceOrigin::Edge { operand: 0, edge: 0 }]);
    assert!(h.faces[..6].iter().all(|f| f.origins.len() == 1));
}

#[cfg(feature = "native")]
#[test]
fn a_tagged_body_is_its_own_history() {
    let b = Brep::make_box([0.0; 3], [1.0, 2.0, 3.0]).unwrap();
    let raw = b.tagged(4).unwrap().history().unwrap();
    assert_eq!((raw.faces.len(), raw.edges.len()), (6, 12));
    for (i, f) in raw.faces.iter().enumerate() {
        assert_eq!(f, &RawFace { change: Change::Kept, origins: vec![(4, i)] });
    }
    for (i, e) in raw.edges.iter().enumerate() {
        assert_eq!(e.origins, vec![(4, i)]);
        assert_eq!(e.faces.len(), 2);
    }
    // Untagged: every face new, every edge where two of them meet.
    let raw = b.history().unwrap();
    assert!(raw.faces.iter().all(|f| f.change == Change::Generated && f.origins.is_empty()));
    let h = raw.history(&[], &Order::default()).unwrap();
    assert!(h.edges.iter().all(|e| e.origins.is_empty()));
    assert_eq!(b.tagged(usize::MAX).unwrap_err(), Error::Argument);
}
