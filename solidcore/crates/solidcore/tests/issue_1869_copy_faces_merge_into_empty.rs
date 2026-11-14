//! #1869: `SmBrep::CopyFaces(faces, pDestinationBrep, &newFaces, bUseMerge = TRUE)` copies one
//! face of a box into an empty B-rep: upstream gives one new face with its four edges and four
//! vertices. solidcore's `copy_faces` with `use_merge = true` returned no face and left the
//! destination empty; with `use_merge = false` the same call copied the face.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use solidcore::geom::tol::USE_DEFAULT;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::p3::new_brep;
use solidcore::topology::brep::p7::copy_faces;
use solidcore::topology::FaceId;
use solidcore::{EntityRef, Kernel};

fn copied(use_merge: bool) -> (usize, [usize; 3]) {
    let mut k = Kernel::default();
    let mut src = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let f = p1::faces(&src, None).unwrap()[0];
    let mut dest = new_brep(&mut k, USE_DEFAULT, USE_DEFAULT).unwrap();
    let mut state = ArenaNotifyState::new(&dest);
    let mut env = state.env(None);
    let key_of = env.key_of;
    let this_face = |f: FaceId| key_of(EntityRef::Face(f));
    let (made, _) = copy_faces(
        &mut k,
        &mut src,
        &[f],
        &mut dest,
        use_merge,
        &this_face,
        &mut env,
    )
    .unwrap();
    dest.refresh_cached_topology();
    (
        made.len(),
        [
            dest.face_list().len(),
            dest.edge_list().len(),
            dest.vertex_list().len(),
        ],
    )
}

#[test]
fn copy_faces_without_the_merge_copies_the_face() {
    assert_eq!(copied(false), (1, [1, 4, 4]));
}

#[test]
fn copy_faces_through_the_merge_copies_the_face() {
    // One face, bounded by four edges and four vertices (a 10 x 10 square).
    assert_eq!(copied(true), (1, [1, 4, 4]));
}
