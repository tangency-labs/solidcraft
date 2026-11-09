//! `api_brep_copy` (`SmApiBrepCopy`) keeps `Copy` attributes when the caller names the
//! bodies the kernel's own way, without key domains of its own: a box with a numeric
//! `Copy` attribute on each of its 6 faces and 12 edges copies to a box whose 6 faces and
//! 12 edges carry them, and the source keeps its own.
//!
//! Reported by tdamsma on tangency-labs/solidcore#1823 (the `solidcore-smlib-api` crate's
//! copy, a fresh B-rep's `owner_identity` as the copy's naming, gave `(0, 4)`); here on the
//! kernel API with each default naming a caller reaches for: the default arena
//! notification state (`ArenaNotifyState::env`, keyed by `loopuse::notify_ref`) and the
//! source named by its own `Brep::owner_identity` or by `notify_ref`. The existing
//! attribute tests (`issue_1462_boolean_family_attributes.rs`) name both bodies in
//! disjoint key domains.
//!
//! A fresh B-rep's `owner_identity` (arena identities: a face, an edge and a vertex of one
//! index share their key) is not an attribute naming: the copy's face, edge and vertex
//! lists of one index merged and went to the vertex, which gave `(0, 4)`. The copy now
//! refuses it (`InvalidInput`), for the copy's naming and for the source's
//! (`plan/findings/topology.brep_p3--arena-identities-share-attribute-keys.md`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::geom::attribute::{Attribute, Behavior};
use solidcore::geom::object::ObjectRef;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::{p2, p3};
use solidcore::topology::loopuse::notify_ref;
use solidcore::topology::Brep;
use solidcore::{EntityRef, Kernel, SolidError};

const ID: u64 = 1_000_007;

/// The faces and edges of `b` carrying attribute [`ID`].
fn counts(b: &Brep) -> (usize, usize) {
    let has = |e| {
        b.entity_attributes(e)
            .iter()
            .any(|(_, a)| a.attribute_id() == ID)
    };
    (
        b.face_list()
            .iter()
            .filter(|&&f| has(EntityRef::Face(f)))
            .count(),
        b.edge_list()
            .iter()
            .filter(|&&e| has(EntityRef::Edge(e)))
            .count(),
    )
}

/// A 10-cube with a `Copy` attribute (value: its index) on each face and edge.
fn tagged_box(k: &mut Kernel) -> Brep {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let entities: Vec<_> = b
        .face_list()
        .iter()
        .copied()
        .map(EntityRef::Face)
        .chain(b.edge_list().iter().copied().map(EntityRef::Edge))
        .collect();
    for (i, e) in entities.into_iter().enumerate() {
        b.add_attribute(e, Attribute::long(ID, i as i64, Behavior::Copy))
            .unwrap();
    }
    assert_eq!(counts(&b), (6, 12));
    b
}

/// The copy of a tagged box with the copy named by `target` (the default arena state's
/// `notify_ref` when `None`) and the source by `source`; asserts the counts on both.
fn copy_counts(
    target: Option<&dyn Fn(EntityRef) -> ObjectRef>,
    source: &dyn Fn(&Brep, EntityRef) -> ObjectRef,
) -> (usize, usize) {
    let mut k = Kernel::default();
    let b = tagged_box(&mut k);
    let mut state = p2::ArenaNotifyState::new(&Brep::new());
    let mut env = state.env(None);
    if let Some(target) = target {
        env.key_of = target;
    }
    let copy = p3::api_brep_copy(&mut k, &mut env, Some(&b), &|e| source(&b, e)).unwrap();
    assert_eq!(counts(&b), (6, 12), "the source keeps its attributes");
    counts(&copy)
}

#[test]
fn default_state_source_by_owner_identity() {
    assert_eq!(copy_counts(None, &|b, e| b.owner_identity(e)), (6, 12));
}

#[test]
fn default_state_source_by_notify_ref() {
    assert_eq!(copy_counts(None, &|_, e| notify_ref(e)), (6, 12));
}

/// The copy of a tagged box with the copy named by `target` and the source by `source`,
/// when one of them gives two objects one key: refused, the source keeping its attributes.
fn refused_copy(
    target: Option<&dyn Fn(EntityRef) -> ObjectRef>,
    source: &dyn Fn(&Brep, EntityRef) -> ObjectRef,
) {
    let mut k = Kernel::default();
    let b = tagged_box(&mut k);
    let mut state = p2::ArenaNotifyState::new(&Brep::new());
    let mut env = state.env(None);
    if let Some(target) = target {
        env.key_of = target;
    }
    let r = p3::api_brep_copy(&mut k, &mut env, Some(&b), &|e| source(&b, e));
    assert!(matches!(r, Err(SolidError::InvalidInput { .. })), "{r:?}");
    assert_eq!(counts(&b), (6, 12), "the source keeps its attributes");
}

#[test]
fn fresh_owner_identity_target_is_refused() {
    let fresh = Brep::new();
    let target = |e| fresh.owner_identity(e);
    refused_copy(Some(&target), &|b, e| b.owner_identity(e));
}

#[test]
fn fresh_owner_identity_source_is_refused() {
    let fresh = Brep::new();
    refused_copy(None, &|_, e| fresh.owner_identity(e));
}
