//! `SmBrep::UnStitch` (the fork's version, which returns its status; the fork's law
//! corners unstitch a round's faces to rebuild them, tangency-labs/smlib@7babe17):
//! the faces asked for (or all of them) broken out of the stitched body into separate
//! faces, each on its own surface again, with edges and vertices of its own. Stitching
//! them back gives the body it was.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p1::{edges, faces, vertices};
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::p7::unstitch;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edge;
use solidcore::{Brep, Kernel};

fn counts(b: &Brep) -> (usize, usize, usize) {
    (
        faces(b, None).unwrap().len(),
        edges(b, None).unwrap().len(),
        vertices(b, None).unwrap().len(),
    )
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// The surfaces of `b`'s faces, as their points at the middle of their domains, sorted.
fn surface_points(k: &Kernel, b: &Brep) -> Vec<[i64; 3]> {
    let mut out: Vec<[i64; 3]> = faces(b, None)
        .unwrap()
        .into_iter()
        .map(|f| {
            let s = b.face(f).unwrap().surface.clone().unwrap();
            let d = s.natural_uv_domain_with_nurb(k).unwrap();
            let p: Point3 = s.evaluate_point(k, d.mid()).unwrap();
            [p.x, p.y, p.z].map(|c| (c * 1e6).round() as i64)
        })
        .collect();
    out.sort_unstable();
    out
}

/// The edges of `b` with one face (an edge of a face on its own).
fn lone_edges(b: &Brep) -> usize {
    edges(b, None)
        .unwrap()
        .into_iter()
        .filter(|&e| edge::faces(b, e, None).unwrap().len() == 1)
        .count()
}

// A box's top face unstitched: the six faces on their six surfaces still, the top face's
// four edges and four vertices its own (edges 12 + 4, vertices 8 + 4), its four edges and
// the four of the faces round it each on one face. Stitched back: the box, its 8000.
#[test]
fn a_face_unstitched_has_edges_of_its_own_and_stitches_back() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Vec3::ZERO, 20.0, 20.0, 20.0).unwrap();
    let before = surface_points(&k, &b);
    let top = faces(&b, None)
        .unwrap()
        .into_iter()
        .find(|&f| {
            let s = b.face(f).unwrap().surface.clone().unwrap();
            let d = s.natural_uv_domain_with_nurb(&k).unwrap();
            (s.evaluate_point(&k, d.mid()).unwrap().z - 20.0).abs() < 1e-9
        })
        .unwrap();
    unstitch(&mut k, &mut b, Some(&[top])).unwrap();
    assert_eq!(counts(&b), (6, 16, 12));
    assert_eq!(lone_edges(&b), 8);
    assert_eq!(surface_points(&k, &b), before);
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    assert_eq!(counts(&b), (6, 12, 8));
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    assert!((volume(&k, &mut b) - 8000.0).abs() <= 1e-9 * 8000.0);
}

// A face the copy cannot make (the top face without its surface; the merge passes over a
// face it fails to make) is refused before anything is removed: the body keeps its six
// faces, twelve edges and eight vertices, the other faces their surfaces.
#[test]
fn a_face_the_copy_skips_is_refused_and_the_body_kept() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Vec3::ZERO, 20.0, 20.0, 20.0).unwrap();
    let top = faces(&b, None)
        .unwrap()
        .into_iter()
        .find(|&f| {
            let s = b.face(f).unwrap().surface.clone().unwrap();
            let d = s.natural_uv_domain_with_nurb(&k).unwrap();
            (s.evaluate_point(&k, d.mid()).unwrap().z - 20.0).abs() < 1e-9
        })
        .unwrap();
    b.face_mut(top).unwrap().surface = None;
    let result = unstitch(&mut k, &mut b, Some(&[top]));
    assert!(result.is_err(), "{result:?}");
    assert_eq!(counts(&b), (6, 12, 8));
    assert!(faces(&b, None).unwrap().contains(&top));
    assert_eq!(lone_edges(&b), 0);
}

// Every face unstitched: six faces with four edges and four vertices each, every edge on
// one face; stitched back, the box.
#[test]
fn every_face_unstitched_is_on_its_own_and_stitches_back() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Vec3::ZERO, 20.0, 20.0, 20.0).unwrap();
    let before = surface_points(&k, &b);
    unstitch(&mut k, &mut b, None).unwrap();
    assert_eq!(counts(&b), (6, 24, 24));
    assert_eq!(lone_edges(&b), 24);
    assert_eq!(surface_points(&k, &b), before);
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    assert_eq!(counts(&b), (6, 12, 8));
    assert!((volume(&k, &mut b) - 8000.0).abs() <= 1e-9 * 8000.0);
}
