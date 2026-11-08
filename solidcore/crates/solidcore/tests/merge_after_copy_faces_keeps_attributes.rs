//! The two attribute-carrying copies compose: a B-rep made by `CopyFaces`
//! (`topology::brep::p7::copy_faces_from` under `p2::CopyNotifyState`, #1908, as the
//! feature executive copies its feature), then merged into another B-rep by `MergeBrep`
//! (`topology::brep::p3::merge_brep`, which lends the merged B-rep's attributes, as
//! upstream's live on the objects it copies), keeps the attributes of the faces and edges
//! it was copied from.
//!
//! The case: a 10-cube with a `Copy` long attribute on each of its 6 faces and 12 edges;
//! all its faces copied into an empty B-rep (6 faces and 12 edges tagged), and that B-rep
//! merged into a second, untagged cube at x 20: 6 of its 12 faces and 12 of its 24 edges
//! tagged, each with its source's value, the second cube's own untouched.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::geom::attribute::{Attribute, Behavior};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p2::{copy_source_ref, CopyNotifyState};
use solidcore::topology::brep::{p3, p7};
use solidcore::topology::loopuse::notify_ref;
use solidcore::topology::{Brep, FaceId};
use solidcore::{EntityRef, Kernel};

const ID: u64 = 1_000_007;

/// The values of attribute [`ID`] on `b`'s faces and on its edges, sorted.
fn tags(b: &Brep) -> (Vec<i64>, Vec<i64>) {
    let value = |e| {
        b.entity_attributes(e)
            .iter()
            .find(|(_, a)| a.attribute_id() == ID)
            .and_then(|(_, a)| a.long_value())
    };
    let mut faces: Vec<i64> = b
        .face_list()
        .iter()
        .filter_map(|&f| value(EntityRef::Face(f)))
        .collect();
    let mut edges: Vec<i64> = b
        .edge_list()
        .iter()
        .filter_map(|&e| value(EntityRef::Edge(e)))
        .collect();
    faces.sort_unstable();
    edges.sort_unstable();
    (faces, edges)
}

#[test]
fn a_copy_faces_copy_merged_keeps_its_attributes() {
    let mut k = Kernel::default();
    let mut src = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let entities: Vec<EntityRef> = src
        .face_list()
        .iter()
        .copied()
        .map(EntityRef::Face)
        .chain(src.edge_list().iter().copied().map(EntityRef::Edge))
        .collect();
    for (i, e) in entities.into_iter().enumerate() {
        src.add_attribute(
            e,
            Attribute::long(ID, i64::try_from(i).unwrap(), Behavior::Copy),
        )
        .unwrap();
    }
    let source_tags = tags(&src);
    assert_eq!((source_tags.0.len(), source_tags.1.len()), (6, 12));

    // CopyFaces of every face into an empty B-rep, carrying both B-reps' attributes
    let mut copied = Brep::new_with_infinite_region().unwrap();
    let faces: Vec<FaceId> = src.face_list().to_vec();
    {
        let mut state = CopyNotifyState::with_attributes_of(&mut src, &mut copied).unwrap();
        let face_ref = |f: FaceId| notify_ref(EntityRef::Face(f));
        let r = p7::copy_faces_from(
            &mut k,
            &mut src,
            &faces,
            &mut copied,
            true,
            &copy_source_ref,
            &face_ref,
            &mut state.env(None),
        );
        state.restore_attributes(&mut src, &mut copied).unwrap();
        r.unwrap();
    }
    assert_eq!(
        tags(&copied),
        source_tags,
        "CopyFaces carries the attributes"
    );
    assert_eq!(tags(&src), source_tags, "the source keeps its own");

    // MergeBrep of the copy into a second, untagged cube
    let mut target = create_box(&mut k, Point3::new(20.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let serial = p3::notify_serial(&mut k.context, &mut copied).unwrap();
    let source_key = move |e| p3::topology_ref(serial, e);
    let merged_from = &copied;
    p3::with_kernel_callbacks(&mut k, &mut target, |k, b, env| {
        p3::merge_brep(k, env, b, merged_from, &source_key, None, None, true)
    })
    .unwrap();
    assert_eq!(target.face_list().len(), 12);
    assert_eq!(
        tags(&target),
        source_tags,
        "MergeBrep carries the copy's attributes"
    );
    assert_eq!(tags(&copied), source_tags, "the merged B-rep keeps its own");
}
