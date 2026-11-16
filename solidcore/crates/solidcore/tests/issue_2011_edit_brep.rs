//! tangency-labs/solidcore#2011: `edit_brep`, one scope for composing lower-level
//! topology operations. It enables B-rep editing and restores the flag, runs the
//! notifications with the B-rep's own attributes carried through, makes the traversal
//! marks once, and refreshes the cached topology before it returns, on the error path
//! too. Before it, a caller restored these four by hand around every composition.
//!
//! The issue's acceptance test: `delete_face` on a box through `edit_brep`, then
//! `topology::integrity::check` passes with no explicit refresh, including when the
//! edit returns `Err`.
#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers

use solidcore::geom::attribute::{Attribute, Behavior};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::p3::{delete_face, edit_brep, DeleteFaceOptions};
use solidcore::topology::integrity;
use solidcore::topology::topology_traverser::Marks;
use solidcore::topology::{Brep, FaceId};
use solidcore::{EntityRef, Kernel, Point3, SolidError};

/// A 10 x 20 x 30 box and the faces it has.
fn boxed(k: &mut Kernel) -> (Brep, Vec<FaceId>) {
    let b = create_box(k, Point3::new(0.0, 0.0, 0.0), 10.0, 20.0, 30.0).unwrap();
    let faces = b.faces().ids().collect();
    (b, faces)
}

#[test]
fn delete_face_through_edit_brep_leaves_true_cached_topology() {
    let mut k = Kernel::default();
    let (mut b, faces) = boxed(&mut k);
    assert_eq!(faces.len(), 6);
    let editing = b.editing_enabled();
    let gone = faces[0];

    let deleted = edit_brep(&mut k, &mut b, |k, b, scope| {
        assert!(b.editing_enabled(), "editing is enabled for the edit");
        delete_face(
            k,
            b,
            scope.env,
            &mut scope.marks,
            gone,
            DeleteFaceOptions::default(),
        )
    })
    .unwrap();

    // delete_face lists only the edges and vertices it deletes with
    // delete_edges_vertices, which the default options leave off
    assert!(deleted.is_empty(), "{deleted:?}");
    assert!(b.face(gone).is_err(), "the face is gone");
    assert_eq!(b.faces().ids().count(), 5);
    assert_eq!(b.editing_enabled(), editing, "the flag is restored");
    integrity::check(&b).unwrap();
}

#[test]
fn an_edit_that_fails_still_restores_and_refreshes() {
    let mut k = Kernel::default();
    let (mut b, faces) = boxed(&mut k);
    let editing = b.editing_enabled();

    let r: solidcore::Result<()> = edit_brep(&mut k, &mut b, |k, b, scope| {
        for &f in &faces[..2] {
            delete_face(
                k,
                b,
                scope.env,
                &mut scope.marks,
                f,
                DeleteFaceOptions::default(),
            )?;
        }
        Err(SolidError::invalid_input(
            "test",
            "fails after two deletions",
        ))
    });

    assert!(matches!(r, Err(SolidError::InvalidInput { .. })), "{r:?}");
    assert_eq!(b.faces().ids().count(), 4);
    assert_eq!(b.editing_enabled(), editing, "the flag is restored");
    integrity::check(&b).unwrap();
}

#[test]
fn the_b_rep_keeps_its_attributes_through_the_edit() {
    let mut k = Kernel::default();
    let (mut b, faces) = boxed(&mut k);
    let kept = faces[1];
    b.add_attribute(EntityRef::Face(kept), Attribute::new(20001, Behavior::Copy))
        .unwrap();

    edit_brep(&mut k, &mut b, |k, b, scope| {
        delete_face(
            k,
            b,
            scope.env,
            &mut scope.marks,
            faces[0],
            DeleteFaceOptions::default(),
        )
    })
    .unwrap();

    assert!(b.find_attribute(EntityRef::Face(kept), 20001).is_some());
    integrity::check(&b).unwrap();
}

/// What the scope saves a caller from: the same deletion by hand, without the refresh,
/// leaves the cached topology stale.
#[test]
fn by_hand_without_the_refresh_the_cache_is_stale() {
    let mut k = Kernel::default();
    let (mut b, faces) = boxed(&mut k);
    b.set_editing_enabled(true);
    {
        let mut state = ArenaNotifyState::new(&b);
        let mut env = state.env(None);
        let mut marks = Marks::default();
        delete_face(
            &mut k,
            &mut b,
            &mut env,
            &mut marks,
            faces[0],
            DeleteFaceOptions::default(),
        )
        .unwrap();
    }
    b.set_editing_enabled(false);
    assert!(integrity::check(&b).is_err());
    b.refresh_cached_topology();
    integrity::check(&b).unwrap();
}
