//! tangency-labs/solidcore#1875: a caller's mark on the faces `SmBrep::LocalOperation`
//! moves. The SMLib capi's draft (`draft_faces`, `moved_faces`) puts an `SM_AB_COPY`
//! attribute on each surface it hands `LocalOperation` and, on success, answers
//! `SMLIB_ERR_NO_EFFECT` when no result face lies on a marked surface. The port names a
//! surface by its owning face, so the same mark goes on the picked face
//! ([`Brep::add_attribute`]): `faces[i]`'s offset face takes `surfaces[i]`
//! (`SmLocalOperationGeometryCreation::FaceuseOffset`), and the mark travels with that
//! face through the extended offset and the copy back (`CopyFaces`) as upstream's does
//! with the surface. The one difference (a merge that keeps an unpicked survivor) is
//! `plan/findings/topology.brep_local_operation--1875-face-mark-for-surface-mark.md`.

#![allow(clippy::expect_used, clippy::indexing_slicing)] // test helpers

use solidcore::geom::attribute::{Attribute, AttributeStore, Behavior, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::math::{Point3, Vec3};
use solidcore::offset::offset_executive::p2::OffsetNotifications;
use solidcore::primitive::api_primitives::create_box;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::local_operation::local_operation;
use solidcore::topology::topology::{NotifyRecords, Topology};
use solidcore::topology::{edge, vertex, Brep, BrepId, EdgeId, FaceId, RegionId};
use solidcore::{EntityRef, Kernel};

/// An application attribute id above 10000, as upstream asks.
const MARK: u64 = 20_875;

/// Throwaway offset notification stores keyed by entity id (the
/// `topology.brep_local_operation` runner's).
fn with_notifications<R>(
    b: &mut Brep,
    f: impl FnOnce(&mut Brep, &mut OffsetNotifications<'_>) -> R,
) -> R {
    fn key(entity: EntityRef, namespace: u64) -> ObjectRef {
        let (index, type_code) = match entity {
            EntityRef::Brep(id) => (id.index(), solidcore::geom::a_object::BREP_TYPE),
            EntityRef::Region(id) => (id.index(), solidcore::topology::Region::type_code()),
            EntityRef::Shell(id) => (id.index(), solidcore::topology::Shell::type_code()),
            EntityRef::Face(id) => (id.index(), solidcore::topology::face::p2::TYPE),
            EntityRef::Faceuse(id) => (id.index(), 16022),
            EntityRef::Loopuse(id) => (id.index(), solidcore::topology::loopuse::TYPE),
            EntityRef::Edge(id) => (id.index(), edge::TYPE),
            EntityRef::Edgeuse(id) => (id.index(), solidcore::topology::edgeuse::p1::TYPE),
            EntityRef::Vertex(id) => (id.index(), vertex::TYPE),
            EntityRef::Vertexuse(id) => (id.index(), solidcore::topology::vertexuse::TYPE),
            EntityRef::Loop(id) => (id.index(), solidcore::topology::topology::TYPE),
        };
        ObjectRef {
            key: ObjectKey((namespace << 60) | ((type_code as u64) << 32) | u64::from(index)),
            type_code,
        }
    }
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let root = EntityRef::Brep(BrepId::from_index(0));
    let mut entities = vec![root];
    entities.extend(b.regions().iter().map(|(id, _)| EntityRef::Region(id)));
    entities.extend(b.shells().iter().map(|(id, _)| EntityRef::Shell(id)));
    entities.extend(b.faces().iter().map(|(id, _)| EntityRef::Face(id)));
    entities.extend(b.faceuses().iter().map(|(id, _)| EntityRef::Faceuse(id)));
    entities.extend(b.loopuses().iter().map(|(id, _)| EntityRef::Loopuse(id)));
    entities.extend(b.edges().iter().map(|(id, _)| EntityRef::Edge(id)));
    entities.extend(b.edgeuses().iter().map(|(id, _)| EntityRef::Edgeuse(id)));
    entities.extend(b.vertices().iter().map(|(id, _)| EntityRef::Vertex(id)));
    entities.extend(
        b.vertexuses()
            .iter()
            .map(|(id, _)| EntityRef::Vertexuse(id)),
    );
    for entity in entities {
        records.insert(key(entity, 1).key, Topology::new());
    }
    let result_brep = key(root, 3);
    records.insert(result_brep.key, Topology::new());
    let original = |e| key(e, 1);
    let result = |e| key(e, 3);
    let original_curve = |e: EdgeId, t: i64| ObjectRef {
        key: ObjectKey((5_u64 << 60) | ((t as u64) << 32) | u64::from(e.index())),
        type_code: t,
    };
    let result_curve = |e: EdgeId, t: i64| ObjectRef {
        key: ObjectKey((4_u64 << 60) | ((t as u64) << 32) | u64::from(e.index())),
        type_code: t,
    };
    let lists = |_: bool, region: RegionId| {
        [0_u64, 1].map(|list| ObjectRef {
            key: ObjectKey((6_u64 << 60) | (list << 32) | u64::from(region.index())),
            type_code: 0,
        })
    };
    let mut notifications = OffsetNotifications {
        records: &mut records,
        attributes: &mut attributes,
        original_key_of: &original,
        result_key_of: &result,
        original_curve_ref: &original_curve,
        result_curve_ref: &result_curve,
        boolean_source_lists: &lists,
        result_brep,
        merge_identities: None,
    };
    f(b, &mut notifications)
}

/// The point of `f`'s surface at the middle of its parameter domain.
fn mid(k: &Kernel, b: &Brep, f: FaceId) -> Point3 {
    let s = solidcore::topology::face::p2::surface(b, f)
        .expect("surface")
        .expect("some");
    s.evaluate_point(k, b.face(f).expect("face").uv_domain.evaluate(0.5, 0.5))
        .expect("point")
}

/// The face of `b` whose surface's middle lies at `z`.
fn face_at_z(k: &Kernel, b: &Brep, z: f64) -> FaceId {
    let found: Vec<FaceId> = b
        .face_list()
        .iter()
        .copied()
        .filter(|&f| (mid(k, b, f).z - z).abs() < 1e-9)
        .collect();
    assert_eq!(found.len(), 1, "one face at z = {z}");
    found[0]
}

/// The faces of `b` carrying the attribute `id`.
fn marked(b: &Brep, id: u64) -> Vec<FaceId> {
    b.face_list()
        .iter()
        .copied()
        .filter(|&f| b.find_attribute(EntityRef::Face(f), id).is_some())
        .collect()
}

/// The capi's answer after a successful local operation on faces marked `ids`: some
/// result face carries a mark (the operation moved a face), or none does
/// (`SMLIB_ERR_NO_EFFECT`).
fn moved_any(b: &Brep, ids: &[u64]) -> bool {
    ids.iter().any(|&id| !marked(b, id).is_empty())
}

/// A 2-box's top raised to z = 3: the face on the new plane, and only it, carries the mark
/// put on the top, so the capi's check finds the move.
#[test]
fn the_moved_face_keeps_the_mark_of_the_face_it_moves() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 2.0, 2.0, 2.0).expect("box");
    let top = face_at_z(&k, &b, 2.0);
    b.add_attribute(
        EntityRef::Face(top),
        Attribute::long(MARK, 1, Behavior::Copy),
    )
    .expect("mark");
    let raised = Surface::from(Plane::from_normal(Point3::new(0.0, 0.0, 3.0), Vec3::Z));
    with_notifications(&mut b, |b, notes| {
        local_operation(&mut k, b, &[top], vec![raised], notes)
    })
    .expect("local operation");
    assert_eq!(b.face_list().len(), 6);
    assert_eq!(marked(&b, MARK), vec![face_at_z(&k, &b, 3.0)]);
    assert!(moved_any(&b, &[MARK]));
}

/// Two faces moved at once (the top raised to z = 3, the bottom lowered to z = -1, its
/// plane given facing up so the operation turns it): each mark is on its own moved face.
#[test]
fn each_moved_face_keeps_its_own_mark() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 2.0, 2.0, 2.0).expect("box");
    let top = face_at_z(&k, &b, 2.0);
    let bottom = face_at_z(&k, &b, 0.0);
    b.add_attribute(
        EntityRef::Face(top),
        Attribute::long(MARK, 1, Behavior::Copy),
    )
    .expect("mark top");
    b.add_attribute(
        EntityRef::Face(bottom),
        Attribute::long(MARK + 1, 2, Behavior::Copy),
    )
    .expect("mark bottom");
    let raised = Surface::from(Plane::from_normal(Point3::new(0.0, 0.0, 3.0), Vec3::Z));
    let lowered = Surface::from(Plane::from_normal(Point3::new(0.0, 0.0, -1.0), Vec3::Z));
    with_notifications(&mut b, |b, notes| {
        local_operation(&mut k, b, &[top, bottom], vec![raised, lowered], notes)
    })
    .expect("local operation");
    assert_eq!(b.face_list().len(), 6);
    assert_eq!(marked(&b, MARK), vec![face_at_z(&k, &b, 3.0)]);
    assert_eq!(marked(&b, MARK + 1), vec![face_at_z(&k, &b, -1.0)]);
}
