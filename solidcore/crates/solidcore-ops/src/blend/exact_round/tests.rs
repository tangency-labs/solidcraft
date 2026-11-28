//! The rounds' topology on a block's record: which faces, edges and vertices they keep, cut
//! back and make, and which rounds they leave to the fillet sessions.
use super::*;

/// A box's record, from solidcore-ops' own box.
fn box_record(to: [f64; 3]) -> r::Brep {
    crate::make_box([0.0; 3], to)
        .unwrap()
        .to_record()
        .unwrap()
        .0
}

/// The record index of the edge whose ends are `p` and `q`.
fn edge_between(rec: &r::Brep, p: [f64; 3], q: [f64; 3]) -> u32 {
    let near = |a: [f64; 3], b: [f64; 3]| a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-9);
    let i = rec
        .edges
        .iter()
        .position(|e| {
            let [a, b] = e.vertices.map(|v| rec.vertices[v as usize].point);
            (near(a, p) && near(b, q)) || (near(a, q) && near(b, p))
        })
        .unwrap();
    i as u32
}

/// The twelve edges of the box from the origin to `to`.
fn all_edges(rec: &r::Brep, [a, b, c]: [f64; 3]) -> Vec<u32> {
    let mut out = Vec::new();
    for (y, z) in [(0.0, 0.0), (b, 0.0), (0.0, c), (b, c)] {
        out.push(edge_between(rec, [0.0, y, z], [a, y, z]));
    }
    for (x, z) in [(0.0, 0.0), (a, 0.0), (0.0, c), (a, c)] {
        out.push(edge_between(rec, [x, 0.0, z], [x, b, z]));
    }
    for (x, y) in [(0.0, 0.0), (a, 0.0), (0.0, b), (a, b)] {
        out.push(edge_between(rec, [x, y, 0.0], [x, y, c]));
    }
    out
}

#[test]
fn a_box_edge_rounds_into_seven_faces_keeping_the_rest() {
    let rec = box_record([10.0; 3]);
    let e = edge_between(&rec, [0.0; 3], [10.0, 0.0, 0.0]);
    let r = round(&Kernel::default(), &rec, &[e], &[1.0]).expect("built in closed form");
    r.record.validate_built().unwrap();
    assert_eq!(
        (
            r.record.faces.len(),
            r.record.edges.len(),
            r.record.vertices.len()
        ),
        (7, 15, 10)
    );
    // every face kept but the round's; the picked edge and its two vertices gone, every other
    // edge kept (four of them cut back), four new edges and vertices
    assert_eq!(r.made.len(), 1);
    assert_eq!(r.faces.iter().filter(|f| f.is_none()).count(), 1);
    assert_eq!(r.edges.iter().filter(|e| e.is_none()).count(), 4);
    assert!(!r.edges.contains(&Some(e)));
    assert_eq!(r.vertices.iter().filter(|v| v.is_none()).count(), 4);
}

#[test]
fn every_edge_of_a_box_rounds_with_ball_corners() {
    let rec = box_record([10.0; 3]);
    let edges = all_edges(&rec, [10.0; 3]);
    let r = round(&Kernel::default(), &rec, &edges, &[1.0]).expect("built in closed form");
    r.record.validate_built().unwrap();
    assert_eq!(
        (
            r.record.faces.len(),
            r.record.edges.len(),
            r.record.vertices.len()
        ),
        (26, 48, 24)
    );
    assert_eq!(r.made.len(), 20);
}

/// Rounds this does not build are left to the sessions: one wider than the faces beside it,
/// two whose strips meet on a face too narrow for both, two meeting at a corner with no third,
/// corners of different radii.
#[test]
fn rounds_it_does_not_cover_are_left_to_the_sessions() {
    let k = Kernel::default();
    let rec = box_record([10.0, 2.0, 10.0]);
    let front = edge_between(&rec, [0.0, 0.0, 10.0], [10.0, 0.0, 10.0]);
    let back = edge_between(&rec, [0.0, 2.0, 10.0], [10.0, 2.0, 10.0]);
    let side = edge_between(&rec, [0.0, 0.0, 10.0], [0.0, 2.0, 10.0]);
    // wider than the 2 wide end faces
    assert!(round(&k, &rec, &[front], &[3.0]).is_none());
    // fits alone, not beside the round of the back edge on the 2 wide top
    assert!(round(&k, &rec, &[front], &[1.5]).is_some());
    assert!(round(&k, &rec, &[front, back], &[1.5]).is_none());
    assert!(round(&k, &rec, &[front, back], &[0.5]).is_some());
    // two rounds meeting at a corner: the sessions' mitre
    assert!(round(&k, &rec, &[front, side], &[0.5]).is_none());
    // a corner of two radii
    let rec = box_record([10.0; 3]);
    let edges = all_edges(&rec, [10.0; 3]);
    let mut radii = vec![1.0; 12];
    radii[0] = 2.0;
    assert!(round(&k, &rec, &edges, &radii).is_none());
}
