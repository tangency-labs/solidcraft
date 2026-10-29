//! tangency-labs/solidcore#1439: `FilletSession::set_mark_corner_patches` (the fork's
//! `SmFilletExecutive::SetMarkCornerPatches`, tangency-labs/smlib `60af574`). A 60 x 40
//! x 20 box with r5 rounds on the three edges at (60, 40, 20), that corner set back 8.
//! With the option the faces of the corner's patch carry a mark naming the corner; they
//! make a disc whose rim runs through the six section ends, each rim edge between a
//! patch face and one other face. As in the fork's own test (`ops_kernel.cpp`, the
//! same box), removing the patch would leave 9 faces, 30 edges and 22 vertices and a
//! hole of twelve edges. Off (the default), no face is marked and the body is the same.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // test helpers

use std::collections::BTreeSet;

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::{edge, vertex, Brep, FaceId};
use solidcore::{Kernel, VertexId};

const CORNER: [f64; 3] = [60.0, 40.0, 20.0];

fn at_corner(b: &Brep, v: VertexId) -> bool {
    let p = vertex::point(b, v).unwrap();
    (p.x - CORNER[0]).abs() + (p.y - CORNER[1]).abs() + (p.z - CORNER[2]).abs() < 1e-9
}

/// The filleted box, the marked faces (corner index, face) and the index of the
/// set-back corner in the session's corner list.
fn build(mark: Option<bool>) -> (Brep, Vec<(usize, FaceId)>, usize) {
    let mut k = Kernel::default();
    k.fixes.fillet_setback_from_start_vertex = true;
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 20.0).unwrap();
    let mut edges = Vec::new();
    for e in b.edges().ids() {
        let (s, t) = edge::start_end_vertices(&b, e).unwrap();
        if [s, t].into_iter().flatten().any(|v| at_corner(&b, v)) {
            edges.push(e);
        }
    }
    assert_eq!(edges.len(), 3);
    let (marked, index);
    {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        if let Some(on) = mark {
            s.set_mark_corner_patches(on);
        }
        s.prepare_constant_radius_edges(&mut k, &edges, 5.0, 1, 1, 1.0)
            .unwrap();
        s.create_corners(&k, None).unwrap();
        let corners = s.fillet_corners().unwrap();
        let found: Vec<usize> = corners
            .iter()
            .enumerate()
            .filter(|(_, &id)| {
                s.corner(id)
                    .unwrap()
                    .vertex
                    .is_some_and(|v| at_corner(s.target(), v))
            })
            .map(|(i, _)| i)
            .collect();
        assert_eq!(found.len(), 1);
        index = found[0];
        s.set_corner_setback(corners[index], 8.0).unwrap();
        s.do_filleting(&mut k, None).unwrap();
        marked = s.corner_patch_faces().unwrap();
    }
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    (b, marked, index)
}

#[test]
fn the_set_back_patch_is_marked_and_rimmed_by_twelve_edges() {
    let (b, marked, index) = build(Some(true));
    assert!(!marked.is_empty());
    assert!(marked.iter().all(|&(i, _)| i == index), "{marked:?}");
    let patch: BTreeSet<FaceId> = marked.iter().map(|&(_, f)| f).collect();
    assert_eq!(patch.len(), marked.len());
    let (mut rim, mut inner) = (0, 0);
    for e in p1::edges(&b, None).unwrap() {
        let faces = edge::faces(&b, e, None).unwrap();
        let inside = faces.iter().filter(|f| patch.contains(f)).count();
        if inside == 0 {
            continue;
        }
        if inside == faces.len() {
            inner += 1;
            continue;
        }
        assert_eq!((faces.len(), inside), (2, 1), "rim edge {e:?}");
        rim += 1;
    }
    assert_eq!(rim, 12);
    let faces = p1::faces(&b, None).unwrap().len();
    let edges = p1::edges(&b, None).unwrap().len();
    let vertices = p1::vertices(&b, None).unwrap();
    let only_patch = vertices
        .iter()
        .filter(|&&v| {
            vertex::faces(&b, v)
                .unwrap()
                .iter()
                .all(|f| patch.contains(f))
        })
        .count();
    assert_eq!(faces - patch.len(), 9);
    assert_eq!(edges - inner, 30);
    assert_eq!(vertices.len() - only_patch, 22);
}

#[test]
fn no_marks_by_default_or_off_and_the_same_body() {
    let (on, _, _) = build(Some(true));
    for mark in [None, Some(false)] {
        let (b, marked, _) = build(mark);
        assert!(marked.is_empty(), "{marked:?}");
        for (x, y) in [
            (
                p1::faces(&b, None).unwrap().len(),
                p1::faces(&on, None).unwrap().len(),
            ),
            (
                p1::edges(&b, None).unwrap().len(),
                p1::edges(&on, None).unwrap().len(),
            ),
        ] {
            assert_eq!(x, y);
        }
    }
}
