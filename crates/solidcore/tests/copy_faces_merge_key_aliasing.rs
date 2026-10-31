//! `SmBrep::CopyFaces(faces, pDestinationBrep, .., bUseMerge = TRUE)` (#1869 review): every
//! copy is its own object upstream, so each new face and edge carries its own source's
//! `SM_NO_COPY` attributes and the source keeps its own. solidcore's `copy_faces` named
//! the new edges and vertices with the caller's one `key_of`, the source's naming: a copy
//! took the key of the source object of its arena id, and the attribute lists crossed
//! both ways (a copied edge got two attributes, the source edge the copy's). With the
//! source named apart (`copy_faces_from`) each copy carries its own source's; one naming
//! for both (`copy_faces`) names the source apart for the copy, and is refused where the
//! source has attribute lists on keys the copies are given.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use solidcore::geom::attribute::{Attribute, AttributeStore, Behavior, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::geom::tol::USE_DEFAULT;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::p3::new_brep;
use solidcore::topology::brep::p7::{copy_faces, copy_faces_from};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edge::NotifyEnv;
use solidcore::topology::loopuse::notify_ref;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{Brep, EdgeId, FaceId};
use solidcore::{EntityRef, Kernel, SolidError};

/// The source's names, apart from the destination's (`notify_ref`).
fn source_ref(e: EntityRef) -> ObjectRef {
    let r = notify_ref(e);
    ObjectRef {
        type_code: r.type_code,
        key: ObjectKey(r.key.0 + (1 << 45)),
    }
}

/// A 10 mm box whose arena ids start at 0 (a copy of a box's faces into a new B-rep;
/// `create_box` leaves its first edge ids unused), as a B-rep read from a file has.
fn compact_box(k: &mut Kernel) -> Brep {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let faces = b.face_list().to_vec();
    let mut dest = new_brep(k, USE_DEFAULT, USE_DEFAULT).unwrap();
    let mut state = ArenaNotifyState::new(&dest);
    let mut env = state.env(None);
    let face = |f: FaceId| notify_ref(EntityRef::Face(f));
    copy_faces(k, &mut b, &faces, &mut dest, true, &face, &mut env).unwrap();
    dest.refresh_cached_topology();
    assert_eq!(dest.edge_list().first(), Some(&EdgeId::from_index(0)));
    dest
}

/// Tags each face and edge of `b` under `name` with a copy attribute of its own:
/// 1000 + the face's index, 2000 + the edge's.
fn tag(store: &mut AttributeStore, b: &Brep, name: &dyn Fn(EntityRef) -> ObjectRef) {
    for &f in b.face_list() {
        let h = store
            .insert(Attribute::long(
                1000 + u64::from(f.index()),
                0,
                Behavior::Copy,
            ))
            .unwrap();
        store
            .add_attribute(name(EntityRef::Face(f)).key, h)
            .unwrap();
    }
    for &e in b.edge_list() {
        let h = store
            .insert(Attribute::long(
                2000 + u64::from(e.index()),
                0,
                Behavior::Copy,
            ))
            .unwrap();
        store
            .add_attribute(name(EntityRef::Edge(e)).key, h)
            .unwrap();
    }
}

fn ids(store: &AttributeStore, key: ObjectKey) -> Vec<u64> {
    let mut v: Vec<u64> = store
        .attributes(key)
        .into_iter()
        .map(|h| store.get(h).unwrap().attribute_id())
        .collect();
    v.sort_unstable();
    v
}

/// Each source face and edge still carries exactly its own tag under `name`.
fn assert_source_untouched(
    store: &AttributeStore,
    b: &Brep,
    name: &dyn Fn(EntityRef) -> ObjectRef,
) {
    for &f in b.face_list() {
        let key = name(EntityRef::Face(f)).key;
        assert_eq!(
            ids(store, key),
            vec![1000 + u64::from(f.index())],
            "source {f:?}"
        );
    }
    for &e in b.edge_list() {
        let key = name(EntityRef::Edge(e)).key;
        assert_eq!(
            ids(store, key),
            vec![2000 + u64::from(e.index())],
            "source {e:?}"
        );
    }
}

/// The source edge whose curve runs between the ends of `copy`'s.
fn source_edge_of(k: &Kernel, src: &Brep, dest: &Brep, copy: EdgeId) -> EdgeId {
    use solidcore::topology::edge;
    let ends = |b: &Brep, e: EdgeId| {
        let c = edge::curve(b, e).unwrap().unwrap();
        let ivl = edge::interval(b, e).unwrap();
        let mut p = [
            c.evaluate_point(k, ivl.min()).unwrap(),
            c.evaluate_point(k, ivl.max()).unwrap(),
        ];
        p.sort_by(|a, b| (a.x, a.y, a.z).partial_cmp(&(b.x, b.y, b.z)).unwrap());
        p
    };
    let want = ends(dest, copy);
    *src.edge_list()
        .iter()
        .find(|&&e| {
            let got = ends(src, e);
            got.iter().zip(&want).all(|(a, b)| a.distance(*b) < 1e-9)
        })
        .unwrap()
}

#[test]
fn copies_through_the_merge_carry_only_their_own_sources_attributes() {
    for which in [1usize, 3, 5] {
        let mut k = Kernel::default();
        let mut src = compact_box(&mut k);
        let mut dest = new_brep(&mut k, USE_DEFAULT, USE_DEFAULT).unwrap();
        let mut records = NotifyRecords::new();
        let mut store = AttributeStore::default();
        tag(&mut store, &src, &source_ref);
        let face = src.face_list()[which];
        let mut env = NotifyEnv {
            records: &mut records,
            attributes: &mut store,
            context: None,
            key_of: &notify_ref,
        };
        let this_face = |f: FaceId| notify_ref(EntityRef::Face(f));
        let (made, all) = copy_faces_from(
            &mut k,
            &mut src,
            &[face],
            &mut dest,
            true,
            &source_ref,
            &this_face,
            &mut env,
        )
        .unwrap();
        assert!(all);
        dest.refresh_cached_topology();
        assert_eq!(made.len(), 1);
        assert_eq!(
            ids(&store, notify_ref(EntityRef::Face(made[0])).key),
            vec![1000 + u64::from(face.index())]
        );
        assert_eq!(dest.edge_list().len(), 4);
        for &copy in dest.edge_list() {
            let from = source_edge_of(&k, &src, &dest, copy);
            assert_eq!(
                ids(&store, notify_ref(EntityRef::Edge(copy)).key),
                vec![2000 + u64::from(from.index())],
                "copy {copy:?} of {from:?}"
            );
        }
        assert_source_untouched(&store, &src, &source_ref);
        for b in [&mut dest, &mut src] {
            let check = check_brep(&mut k, b, CheckLevel::Level2).unwrap();
            assert_eq!(check.failure_count(), 0, "{:?}", check.lines());
        }
    }
}

#[test]
fn one_naming_for_both_breps_is_refused_where_the_source_has_attributes() {
    let mut k = Kernel::default();
    let mut src = compact_box(&mut k);
    let mut dest = new_brep(&mut k, USE_DEFAULT, USE_DEFAULT).unwrap();
    let mut records = NotifyRecords::new();
    let mut store = AttributeStore::default();
    tag(&mut store, &src, &notify_ref);
    let face = src.face_list()[3];
    let mut env = NotifyEnv {
        records: &mut records,
        attributes: &mut store,
        context: None,
        key_of: &notify_ref,
    };
    let this_face = |f: FaceId| notify_ref(EntityRef::Face(f));
    let r = copy_faces(
        &mut k,
        &mut src,
        &[face],
        &mut dest,
        true,
        &this_face,
        &mut env,
    );
    assert!(matches!(r, Err(SolidError::InvalidInput { .. })), "{r:?}");
    // before anything changed
    dest.refresh_cached_topology();
    assert!(dest.face_list().is_empty() && dest.edge_list().is_empty());
    assert_source_untouched(&store, &src, &notify_ref);
}

#[test]
fn one_naming_for_both_breps_without_attributes_still_copies() {
    // the #1869 caller: one arena naming, no attributes; the source is named apart for
    // the copy
    let mut k = Kernel::default();
    let mut src = compact_box(&mut k);
    let mut dest = new_brep(&mut k, USE_DEFAULT, USE_DEFAULT).unwrap();
    let mut state = ArenaNotifyState::new(&src);
    let mut env = state.env(None);
    let this_face = |f: FaceId| notify_ref(EntityRef::Face(f));
    let face = src.face_list()[3];
    let (made, all) = copy_faces(
        &mut k,
        &mut src,
        &[face],
        &mut dest,
        true,
        &this_face,
        &mut env,
    )
    .unwrap();
    assert!(all);
    assert_eq!(made.len(), 1);
    dest.refresh_cached_topology();
    assert_eq!(
        [
            dest.face_list().len(),
            dest.edge_list().len(),
            dest.vertex_list().len()
        ],
        [1, 4, 4]
    );
    let check = check_brep(&mut k, &mut dest, CheckLevel::Level2).unwrap();
    assert_eq!(check.failure_count(), 0, "{:?}", check.lines());
}
