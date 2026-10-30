//! Issue #1462 (P2): caller attributes on B-rep objects reach the results of the rest
//! of the boolean family (`merge_breps`, `boolean_trees`, `boolean_lists`), of
//! `api_brep_copy`, and of the plain (env-less) topology edits, as SMLib's default
//! handlers carry them: split gives both pieces the attribute, copy gives the copy
//! copy and reference attributes but not temporary ones, merge gives the survivor the
//! ids it lacks, and a deleted object's attributes die with it.
//!
//! Each operand face and edge gets its own attribute id (one id per origin).

#![allow(clippy::expect_used, clippy::indexing_slicing)] // test helpers

use solidcore::boolean::api_brep::{boolean_lists, boolean_trees, merge_breps};
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::attribute::{Attribute, Behavior, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::{p2, p3, p9};
use solidcore::topology::Brep;
use solidcore::{EntityRef, Kernel};

const BASE_ID: u64 = 10007;

/// `(operand, kind, index)`: kind 0 face, 1 edge.
type Origin = (u64, u64, u64);

fn origin_id(operand: u64, kind: u64, index: u64) -> u64 {
    BASE_ID + operand * 1000 + kind * 500 + index
}

fn decode(id: u64) -> Origin {
    let raw = id - BASE_ID;
    (raw / 1000, raw % 1000 / 500, raw % 500)
}

fn tag(b: &mut Brep, operand: u64) {
    let faces = b.face_list().to_vec();
    for (i, f) in faces.into_iter().enumerate() {
        let id = origin_id(operand, 0, i as u64);
        b.add_attribute(
            EntityRef::Face(f),
            Attribute::long(id, id as i64, Behavior::Copy),
        )
        .expect("tag face");
    }
    let edges = b.edge_list().to_vec();
    for (i, e) in edges.into_iter().enumerate() {
        let id = origin_id(operand, 1, i as u64);
        b.add_attribute(
            EntityRef::Edge(e),
            Attribute::long(id, id as i64, Behavior::Copy),
        )
        .expect("tag edge");
    }
}

fn origins(b: &Brep, e: EntityRef) -> Vec<Origin> {
    let mut out: Vec<_> = b
        .entity_attributes(e)
        .into_iter()
        .filter(|(_, a)| a.attribute_id() >= BASE_ID)
        .map(|(_, a)| decode(a.attribute_id()))
        .collect();
    out.sort_unstable();
    out
}

fn tagged_box(k: &mut Kernel, corner: (f64, f64, f64), size: f64, operand: u64) -> Brep {
    let mut b = create_box(
        k,
        Point3::new(corner.0, corner.1, corner.2),
        size,
        size,
        size,
    )
    .expect("box");
    tag(&mut b, operand);
    b
}

/// Every face carries exactly one face origin; returns how many faces each operand
/// gave (indexed by operand).
fn face_operands(r: &Brep, operands: usize) -> Vec<usize> {
    let mut count = vec![0; operands];
    for &f in r.face_list() {
        let o = origins(r, EntityRef::Face(f));
        assert_eq!(o.len(), 1, "face {f:?} has origins {o:?}");
        assert_eq!(o[0].1, 0, "face {f:?} carries an edge origin {o:?}");
        count[o[0].0 as usize] += 1;
    }
    count
}

/// Every edge carries edge origins only; returns how many edges carry one.
fn tagged_edges(r: &Brep) -> usize {
    r.edge_list()
        .iter()
        .filter(|&&e| {
            let o = origins(r, EntityRef::Edge(e));
            assert!(o.iter().all(|&(_, kind, _)| kind == 1), "edge {e:?}: {o:?}");
            !o.is_empty()
        })
        .count()
}

#[test]
fn merge_breps_fold_of_three_boxes_keeps_each_face_origin() {
    let mut k = Kernel::default();
    let a = tagged_box(&mut k, (0.0, 0.0, 0.0), 10.0, 0);
    let b = tagged_box(&mut k, (5.0, 5.0, 5.0), 10.0, 1);
    let c = tagged_box(&mut k, (-5.0, -5.0, -5.0), 10.0, 2);
    let mut list = vec![a, b, c];
    let r = merge_breps(&mut k, &mut list, BooleanOperation::Union).expect("merge_breps");
    assert!(list.is_empty());
    // each box keeps all six of its faces (trimmed), each with its own origin only
    assert_eq!(face_operands(&r, 3), [6, 6, 6]);
    assert!(tagged_edges(&r) > 0);
}

#[test]
fn boolean_tree_union_then_difference() {
    let mut k = Kernel::default();
    let a = tagged_box(&mut k, (0.0, 0.0, 0.0), 10.0, 0);
    let b = tagged_box(&mut k, (5.0, 5.0, 5.0), 10.0, 1);
    let c = tagged_box(&mut k, (-5.0, -5.0, -5.0), 10.0, 2);
    // ((A u B) - C)
    let mut breps = vec![Some(a), Some(b), Some(c)];
    boolean_trees(&mut k, &mut breps, &[0, 1, -1, 2, -3]).expect("boolean_trees");
    assert_eq!(breps.len(), 1);
    let r = breps[0].as_ref().expect("tree result");
    // C cuts the corner of A: the three faces of C inside A bound the result
    let count = face_operands(r, 3);
    assert_eq!(count, [6, 6, 3]);
    assert!(tagged_edges(r) > 0);
}

#[test]
fn boolean_lists_carry_list_attributes_to_every_component() {
    let mut k = Kernel::default();
    let a = tagged_box(&mut k, (0.0, 0.0, 0.0), 10.0, 0);
    let far = tagged_box(&mut k, (100.0, 0.0, 0.0), 10.0, 1);
    let b = tagged_box(&mut k, (5.0, 5.0, 5.0), 10.0, 2);
    // union of [A, far] with [B]: two separate solids
    let out = boolean_lists(&mut k, &[a, far], &[], &[b], &[], 1).expect("boolean_lists");
    assert_eq!(out.breps.len(), 2);
    let mut counts: Vec<Vec<usize>> = out.breps.iter().map(|r| face_operands(r, 3)).collect();
    counts.sort();
    assert_eq!(counts, [vec![0, 6, 0], vec![6, 0, 6]]);
    for r in &out.breps {
        assert!(tagged_edges(r) > 0);
    }
}

fn copy_key(entity: EntityRef, domain: u64) -> ObjectRef {
    let reference = p3::topology_ref(0, entity);
    ObjectRef {
        type_code: reference.type_code,
        key: ObjectKey(
            domain + reference.type_code as u64 * 1_000_000 + (reference.key.0 & 0xffff_ffff),
        ),
    }
}

#[test]
fn api_brep_copy_keeps_copy_and_reference_and_drops_temp() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 1.0, 1.0, 1.0).expect("box");
    let faces = b.face_list().to_vec();
    b.add_attribute(
        EntityRef::Face(faces[0]),
        Attribute::long(BASE_ID, 1, Behavior::Copy),
    )
    .expect("copy attribute");
    let shared = b
        .add_attribute(
            EntityRef::Face(faces[1]),
            Attribute::long(BASE_ID + 1, 2, Behavior::Reference),
        )
        .expect("reference attribute");
    b.attach_attribute(EntityRef::Face(faces[2]), shared)
        .expect("share reference");
    b.add_attribute(
        EntityRef::Face(faces[3]),
        Attribute::long(BASE_ID + 2, 3, Behavior::Temp),
    )
    .expect("temp attribute");
    let before = b.attribute_store().clone();

    let mut state = p2::ArenaNotifyState::new(&Brep::new());
    let mut env = state.env(None);
    let target_key = |entity| copy_key(entity, 1 << 40);
    env.key_of = &target_key;
    let copy = p3::api_brep_copy(&mut k, &mut env, Some(&b), &|e| copy_key(e, 0)).expect("copy");
    // the source keeps its attributes, and the caller's store lends none of them on
    assert_eq!(b.attribute_store(), &before);
    for &f in &faces {
        assert!(env
            .attributes
            .attributes(copy_key(EntityRef::Face(f), 0).key)
            .is_empty());
    }

    let ids = |f| {
        let mut v: Vec<_> = copy
            .entity_attributes(EntityRef::Face(f))
            .into_iter()
            .map(|(_, a)| (a.attribute_id(), a.long_value()))
            .collect();
        v.sort_unstable();
        v
    };
    let copied = copy.face_list().to_vec();
    assert_eq!(copied.len(), 6);
    let tagged: Vec<_> = copied.iter().map(|&f| ids(f)).collect();
    let mut all: Vec<_> = tagged.iter().flatten().copied().collect();
    all.sort_unstable();
    // the copy attribute once, the reference attribute on two faces, no temp one
    assert_eq!(
        all,
        [
            (BASE_ID, Some(1)),
            (BASE_ID + 1, Some(2)),
            (BASE_ID + 1, Some(2))
        ]
    );
    // the two reference users share one attribute in the copy
    let handles: Vec<_> = copied
        .iter()
        .filter_map(|&f| {
            copy.entity_attributes(EntityRef::Face(f))
                .into_iter()
                .find(|(_, a)| a.attribute_id() == BASE_ID + 1)
                .map(|(h, _)| h)
        })
        .collect();
    assert_eq!(handles.len(), 2);
    assert_eq!(handles[0], handles[1]);
    assert_eq!(copy.attribute(handles[0]).expect("live").users().len(), 2);
}

#[test]
fn plain_edits_split_and_delete_as_upstream() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 1.0, 1.0, 1.0).expect("box");
    let e = b.edge_list()[0];
    b.add_attribute(
        EntityRef::Edge(e),
        Attribute::long(BASE_ID, 7, Behavior::Copy),
    )
    .expect("edge attribute");
    // an edge split under the kernel's identities (`with_kernel_callbacks`): both
    // pieces carry the edge's attribute (SM_NO_SPLIT_IN_BREP)
    b.set_editing_enabled(true);
    let out = p3::with_kernel_callbacks(&mut k, &mut b, |k, b, env| {
        let serial = b.notify_serial().expect("serial");
        p9::make_vertex_split_edge(k, b, e, 0.5, env, &|edge, kind| ObjectRef {
            key: p3::topology_ref(serial, EntityRef::Edge(edge)).key,
            type_code: kind,
        })
    })
    .expect("split");
    let pieces = out.edges.expect("split edges");
    assert_eq!(pieces.len(), 2);
    for piece in pieces {
        let found = b
            .find_attribute(EntityRef::Edge(piece), BASE_ID)
            .expect("piece keeps the edge attribute");
        assert_eq!(found.long_value(), Some(7));
    }
}

#[test]
fn plain_remove_faces_clears_the_deleted_face_attributes() {
    // a reference attribute on two faces loses the user `remove_faces` deletes; a
    // standalone attribute of the deleted face survives without users
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 1.0, 1.0, 1.0).expect("box");
    b.set_editing_enabled(true);
    let faces = b.face_list().to_vec();
    let shared = b
        .add_attribute(
            EntityRef::Face(faces[0]),
            Attribute::long(BASE_ID + 1, 1, Behavior::Reference),
        )
        .expect("reference");
    b.attach_attribute(EntityRef::Face(faces[1]), shared)
        .expect("share");
    let standalone = b
        .add_attribute(
            EntityRef::Face(faces[0]),
            Attribute::long(BASE_ID + 2, 2, Behavior::StandaloneCopy),
        )
        .expect("standalone");
    solidcore::topology::brep::p5::remove_faces(&mut k, &mut b, true, &[faces[0]])
        .expect("remove face");
    assert_eq!(b.attribute(shared).expect("live").users().len(), 1);
    assert!(b
        .find_attribute(EntityRef::Face(faces[1]), BASE_ID + 1)
        .is_some());
    assert!(b
        .attribute(standalone)
        .expect("survives")
        .users()
        .is_empty());
}

#[test]
fn boolean_lists_reference_users_are_the_result_objects() {
    // coplanar boxes: the stitch glues the copied edges and deletes the merged ones; a
    // reference attribute's users are then the result's objects that carry it only
    let mut k = Kernel::default();
    let mut a = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).expect("a");
    let mut b = create_box(&mut k, Point3::new(5.0, 0.0, 0.0), 10.0, 10.0, 10.0).expect("b");
    for (operand, x) in [(0u64, &mut a), (1, &mut b)] {
        let edges = x.edge_list().to_vec();
        for (i, e) in edges.into_iter().enumerate() {
            let id = origin_id(operand, 1, i as u64);
            x.add_attribute(
                EntityRef::Edge(e),
                Attribute::long(id, 0, Behavior::Reference),
            )
            .expect("tag");
        }
    }
    let out = boolean_lists(&mut k, &[a], &[], &[b], &[], 1).expect("lists");
    let r = &out.breps[0];
    let mut holders = std::collections::BTreeMap::new();
    for &e in r.edge_list() {
        for (h, _) in r.entity_attributes(EntityRef::Edge(e)) {
            *holders.entry(h).or_insert(0usize) += 1;
        }
    }
    assert!(!holders.is_empty());
    for (h, n) in holders {
        assert_eq!(r.attribute(h).expect("live").users().len(), n, "{h:?}");
    }
}
