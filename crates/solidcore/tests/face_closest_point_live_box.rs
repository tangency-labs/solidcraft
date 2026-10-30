//! `face_closest_point_in_shape` (`SmApiFaceClosestPoint`) over a bare `LiveBrepObjects`,
//! as a library caller makes it (no B-rep cache, no `face_entity`), on a `create_box`
//! B-rep as made (not renumbered or renamed), its geometry named by the B-rep's own
//! owner identities (`Brep::owner_identity`). At the centre of each of the box's faces
//! the face's closest point is the centre itself, at distance 0 (the SolidCraft fork's
//! C API picks faces by their centres): plan/findings
//! `query.api_queries--face-closest-point-solve-answers-a-vertex.md`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::geom::object::{ObjectRef, Shape};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::face_closest_point_in_shape;
use solidcore::query::brep::{BrepIdentity, LiveBrepObjects};
use solidcore::topology::brep::p1;
use solidcore::topology::brep_cache::{arena_identity, CacheObject};
use solidcore::topology::{Brep, BrepId, FaceId};
use solidcore::{EntityRef, Kernel};

/// The closest point of face `f` of `b` to `p` and its distance, through a bare live view
/// whose keys are the B-rep's own owner identities.
fn closest(k: &mut Kernel, b: &mut Brep, f: FaceId, p: Point3) -> (Point3, f64) {
    let id = BrepId::from_index(0);
    let shape = Shape::new(k, &(id, &*b), &[], &[(id, f)], &[], &[]).unwrap();
    let names = b.clone();
    let keys = move |o: CacheObject| match o {
        CacheObject::Face(f) => names.owner_identity(EntityRef::Face(f)),
        CacheObject::Edge(e) => names.owner_identity(EntityRef::Edge(e)),
        CacheObject::Vertex(v) => names.owner_identity(EntityRef::Vertex(v)),
        other => arena_identity(other),
    };
    let owner_of = |_: ObjectRef| -> Option<ObjectRef> { None };
    let identity = BrepIdentity {
        brep_id: id,
        keys: &keys,
        owner_of: &owner_of,
    };
    let objects = LiveBrepObjects::new(b, &identity, None).unwrap();
    let c = face_closest_point_in_shape(k, &shape, &objects, (id, f), p).unwrap();
    (c.point, c.distance)
}

#[test]
fn box_face_centres_are_their_own_closest_points() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 20.0, 30.0).unwrap();
    let centres = [
        Point3::new(5.0, 10.0, 0.0),
        Point3::new(5.0, 10.0, 30.0),
        Point3::new(0.0, 10.0, 15.0),
        Point3::new(10.0, 10.0, 15.0),
        Point3::new(5.0, 0.0, 15.0),
        Point3::new(5.0, 20.0, 15.0),
    ];
    let faces = p1::faces(&b, None).unwrap();
    for c in centres {
        let mut answers = Vec::new();
        for &f in &faces {
            let (q, d) = closest(&mut k, &mut b, f, c);
            answers.push((f, q, d));
        }
        eprintln!("centre {c:?}: {answers:?}");
        let on = answers
            .iter()
            .filter(|(_, q, d)| *d <= 1e-9 && (*q - c).length() <= 1e-9)
            .count();
        assert_eq!(on, 1, "centre {c:?}: exactly its own face at distance 0");
        // every other face's answer is no farther than its nearest corner
        for (_, _, d) in answers {
            assert!(d <= ((5.0f64).powi(2) + 10.0f64.powi(2) + 30.0f64.powi(2)).sqrt());
        }
    }
}
