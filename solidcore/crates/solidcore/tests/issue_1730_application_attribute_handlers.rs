//! Issue #1730: an application-defined attribute class ([`AttributeHandlers`]) overrides
//! SMLib's `Split`, `Merge`, `Coincident` and `Copy` handlers and edits attributes the
//! target object already carries, as an application's `SmAttribute` subclass does.
//!
//! The reference type is the issue's `Tag` (SolidCraft's tag history): a set of
//! origins, a set of edges and an `at` list of points. Its split, merge and coincident
//! handlers `join` the origins into the target's Tag (or add a Tag with an empty `at`),
//! and its copy is SMLib's default, so a copy onto an object that already has a Tag
//! keeps the target's.
//!
//! The 5-step test is the issue's, run through `SmAObject::Notify`
//! ([`a_object::notify`]); the imprint case reaches the same handlers through a real
//! Boolean.

#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::panic)] // test helpers

use std::collections::BTreeSet;

use solidcore::boolean::api_brep::boolean_with_options;
use solidcore::boolean::merge::p1::MergeOptions;
use solidcore::geom::a_object::{self, BooleanOperation};
use solidcore::geom::attribute::{
    ids, Attribute, AttributeHandlers, AttributeStore, AttributeView, Behavior, ObjectKey,
    Placement,
};
use solidcore::geom::object::{NotifyOperation, ObjectRef};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_plane};
use solidcore::topology::Brep;
use solidcore::{EntityRef, Kernel, Result};

/// `TAG_ID = SM_AI_USER_1 + 7`.
const TAG_ID: u64 = ids::USER_1 + 7;

/// The issue's `Tag`: origins `(operand, index)`, edges and `at`. Copy behaviour.
#[derive(Debug, Clone, PartialEq, Default)]
struct Tag {
    origins: BTreeSet<(u64, u64)>,
    edges: BTreeSet<i64>,
    at: Vec<Point3>,
}

impl Tag {
    fn attribute(self) -> Attribute {
        Attribute::application(TAG_ID, Behavior::Copy, self)
    }
}

/// `join(x, o, e)`.
fn join(
    view: &AttributeView<'_>,
    x: ObjectKey,
    o: &BTreeSet<(u64, u64)>,
    e: &BTreeSet<i64>,
) -> Vec<Placement> {
    if o.is_empty() {
        return Vec::new();
    }
    if let Some((handle, tag)) = view
        .find_attribute(x, TAG_ID)
        .and_then(|(h, a)| a.application_value::<Tag>().map(|t| (h, t)))
    {
        let mut tag = tag.clone();
        tag.origins.extend(o.iter().copied());
        tag.edges.extend(e.iter().copied());
        let value = Tag::attribute(tag).value().clone();
        return vec![Placement::Edit { handle, value }];
    }
    let mut out: Vec<Placement> = view
        .attributes(x)
        .into_iter()
        .filter(|(_, a)| a.attribute_id() == TAG_ID && a.application_value::<Tag>().is_none())
        .map(|(handle, _)| Placement::Remove { on: x, handle })
        .collect();
    out.push(Placement::Add {
        on: x,
        attribute: Tag {
            origins: o.clone(),
            edges: e.clone(),
            at: Vec::new(),
        }
        .attribute(),
    });
    out
}

impl AttributeHandlers for Tag {
    fn split(
        &self,
        view: &AttributeView<'_>,
        split: ObjectKey,
        a: ObjectKey,
        b: ObjectKey,
    ) -> Result<Vec<Placement>> {
        let mut out = Vec::new();
        for x in [a, b] {
            if x != split {
                out.extend(join(view, x, &self.origins, &self.edges));
            }
        }
        Ok(out)
    }

    fn merge(
        &self,
        view: &AttributeView<'_>,
        a: ObjectKey,
        b: ObjectKey,
        result: ObjectKey,
    ) -> Result<Vec<Placement>> {
        let on_result: Vec<_> = view
            .attributes(result)
            .into_iter()
            .map(|(h, _)| h)
            .collect();
        let (mut o, mut e) = (BTreeSet::new(), BTreeSet::new());
        for obj in [a, b] {
            for (h, attr) in view.attributes(obj) {
                if let Some(tag) = attr.application_value::<Tag>() {
                    if !on_result.contains(&h) {
                        o.extend(tag.origins.iter().copied());
                        e.extend(tag.edges.iter().copied());
                    }
                }
            }
        }
        Ok(join(view, result, &o, &e))
    }

    fn coincident(
        &self,
        view: &AttributeView<'_>,
        _from: ObjectKey,
        to: ObjectKey,
    ) -> Result<Vec<Placement>> {
        Ok(join(view, to, &self.origins, &self.edges))
    }
}

fn obj(n: u64) -> ObjectRef {
    ObjectRef {
        type_code: 0,
        key: ObjectKey(n),
    }
}

fn notify(store: &mut AttributeStore, op: NotifyOperation, this: u64, data: [Option<u64>; 3]) {
    let [d1, d2, d3] = data.map(|d| d.map(obj));
    a_object::notify(store, None, op, obj(this), d1, d2, d3).expect("notify");
}

fn tag_of(store: &AttributeStore, o: u64) -> Option<Tag> {
    let tags: Vec<Tag> = store
        .attributes(ObjectKey(o))
        .into_iter()
        .filter_map(|h| store.get(h)?.application_value::<Tag>().cloned())
        .collect();
    assert!(tags.len() <= 1, "object {o} has {} Tags", tags.len());
    tags.into_iter().next()
}

fn tag(origins: &[(u64, u64)], at: &[Point3]) -> Tag {
    Tag {
        origins: origins.iter().copied().collect(),
        edges: BTreeSet::new(),
        at: at.to_vec(),
    }
}

const A: u64 = 1;
const B: u64 = 2;
const C: u64 = 3;
const D: u64 = 4;

/// Steps 1 to 4 of the issue's test; returns the store and `[p, q]`.
fn after_coincident() -> (AttributeStore, [Point3; 2]) {
    let p = Point3::new(1.0, 2.0, 3.0);
    let q = Point3::new(4.0, 5.0, 6.0);
    let mut s = AttributeStore::new();

    // 1. A has Tag {origins {(0,1)}, at [p, q]}.
    let h = s
        .insert(tag(&[(0, 1)], &[p, q]).attribute())
        .expect("insert");
    s.add_attribute(ObjectKey(A), h).expect("add");

    // 2. split(A -> A, B): B gets Tag {(0,1)} with an empty `at`, A is unchanged.
    notify(&mut s, NotifyOperation::Split, A, [Some(A), Some(B), None]);
    assert_eq!(tag_of(&s, A), Some(tag(&[(0, 1)], &[p, q])));
    assert_eq!(tag_of(&s, B), Some(tag(&[(0, 1)], &[])));

    // 3. C has Tag {(1,4)}. merge(A, C -> A): A becomes {(0,1), (1,4)} and keeps
    // `at` [p, q]; C is untouched.
    let h = s.insert(tag(&[(1, 4)], &[]).attribute()).expect("insert");
    s.add_attribute(ObjectKey(C), h).expect("add");
    notify(&mut s, NotifyOperation::Merge, A, [Some(A), Some(C), None]);
    assert_eq!(tag_of(&s, A), Some(tag(&[(0, 1), (1, 4)], &[p, q])));
    assert_eq!(tag_of(&s, C), Some(tag(&[(1, 4)], &[])));

    // 4. coincident(C -> B): B becomes {(0,1), (1,4)}, `at` stays empty.
    notify(
        &mut s,
        NotifyOperation::Coincident,
        B,
        [Some(B), Some(C), None],
    );
    assert_eq!(tag_of(&s, B), Some(tag(&[(0, 1), (1, 4)], &[])));
    assert_eq!(tag_of(&s, C), Some(tag(&[(1, 4)], &[])));
    (s, [p, q])
}

#[test]
fn issue_minimal_test_split_merge_coincident_copy() {
    let (mut s, [p, q]) = after_coincident();
    // 5. copy(A -> D), where D has no Tag: D gets a Tag equal to A's.
    notify(&mut s, NotifyOperation::Copy, A, [Some(D), None, None]);
    assert_eq!(tag_of(&s, D), tag_of(&s, A));
    assert_eq!(tag_of(&s, D), Some(tag(&[(0, 1), (1, 4)], &[p, q])));
    // a copy, not a reference
    let ha = s.find_attribute(ObjectKey(A), TAG_ID);
    let hd = s.find_attribute(ObjectKey(D), TAG_ID);
    assert!(ha.is_some() && hd.is_some() && ha != hd);
}

#[test]
fn copy_onto_a_tagged_target_keeps_the_targets_tag() {
    let (mut s, _) = after_coincident();
    // B already has a Tag: SMLib's default Copy leaves it alone (no union).
    notify(&mut s, NotifyOperation::Copy, A, [Some(B), None, None]);
    assert_eq!(tag_of(&s, B), Some(tag(&[(0, 1), (1, 4)], &[])));
    assert_eq!(s.attributes(ObjectKey(B)).len(), 1);
}

#[test]
fn merge_runs_once_per_attribute_id() {
    // Notify skips C's Tag because A has one of the same id: the union comes from the
    // single call on A's Tag, and C's Tag is never merged onto A a second time.
    let (s, _) = after_coincident();
    assert_eq!(s.attributes(ObjectKey(A)).len(), 1);
    assert_eq!(s.attributes(ObjectKey(C)).len(), 1);
}

#[test]
fn join_replaces_a_foreign_attribute_of_the_tag_id() {
    // A Generic attribute with the Tag's id on the split child is removed and a Tag
    // added in its place.
    let mut s = AttributeStore::new();
    let h = s
        .insert(tag(&[(0, 9)], &[Point3::new(0.0, 0.0, 1.0)]).attribute())
        .expect("insert");
    s.add_attribute(ObjectKey(A), h).expect("add");
    let g = s
        .insert(Attribute::generic(TAG_ID, Behavior::Copy, &[7], &[], &[]))
        .expect("insert");
    s.add_attribute(ObjectKey(B), g).expect("add");
    notify(&mut s, NotifyOperation::Split, A, [Some(A), Some(B), None]);
    assert!(!s.is_live(g));
    assert_eq!(s.attributes(ObjectKey(B)).len(), 1);
    assert_eq!(tag_of(&s, B), Some(tag(&[(0, 9)], &[])));
}

#[test]
fn application_values_clone_and_compare_through_the_box() {
    fn send_sync<T: Send + Sync>() {}
    fn send<T: Send>() {}
    let a = tag(&[(0, 1)], &[Point3::new(1.0, 0.0, 0.0)]).attribute();
    let copy = a.make_copy();
    assert_eq!(copy, a);
    assert_ne!(tag(&[(0, 2)], &[]).attribute(), a);
    send_sync::<AttributeStore>();
    send::<Brep>();
    send::<Kernel>();
}

/// A box whose six faces carry Tags `{(0, i)}` with `at = [face index point]`.
fn tagged_box(k: &mut Kernel) -> Brep {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).expect("box");
    let faces = b.face_list().to_vec();
    for (i, f) in faces.into_iter().enumerate() {
        let t = tag(&[(0, i as u64)], &[Point3::new(i as f64, 0.0, 0.0)]);
        b.add_attribute(EntityRef::Face(f), t.attribute())
            .expect("tag face");
    }
    b
}

fn face_tags(b: &Brep) -> Vec<Tag> {
    let mut out = Vec::new();
    for &f in b.face_list() {
        let tags: Vec<Tag> = b
            .entity_attributes(EntityRef::Face(f))
            .into_iter()
            .filter_map(|(_, a)| a.application_value::<Tag>().cloned())
            .collect();
        assert_eq!(tags.len(), 1, "face {f:?}: {tags:?}");
        out.extend(tags);
    }
    out
}

#[test]
fn imprint_splits_run_the_application_split_handler() {
    // SM_BO_IMPRINT of the box by the sheet at z = 5 splits the four side faces: each
    // piece keeps its face's origin, and a split-off piece gets an empty `at` (the
    // default handler would have copied the origin's `at` onto it).
    let mut k = Kernel::default();
    let a = tagged_box(&mut k);
    let b = create_plane(&mut k, Point3::new(-5.0, -5.0, 5.0), 20.0, 20.0).expect("sheet");
    let out = boolean_with_options(
        &mut k,
        a,
        b,
        BooleanOperation::Imprint,
        MergeOptions::default(),
    )
    .expect("imprint");
    let tags = face_tags(&out.result);
    let mut pieces = [0usize; 6];
    let mut empty_at = [0usize; 6];
    for t in &tags {
        assert_eq!(t.origins.len(), 1, "{t:?}");
        let &(operand, i) = t.origins.iter().next().expect("origin");
        assert_eq!(operand, 0);
        let i = i as usize;
        pieces[i] += 1;
        if t.at.is_empty() {
            empty_at[i] += 1;
        } else {
            assert_eq!(t.at, [Point3::new(i as f64, 0.0, 0.0)]);
        }
    }
    let split: Vec<usize> = (0..6).filter(|&i| pieces[i] > 1).collect();
    assert_eq!(split.len(), 4, "pieces {pieces:?}");
    for i in 0..6 {
        // one piece per face keeps the origin's `at`, every other piece has none
        assert_eq!(
            pieces[i] - empty_at[i],
            1,
            "face {i}: {pieces:?} {empty_at:?}"
        );
    }
}
