//! `loopuse::notify_ref` is public (#1766): a caller that builds bodies itself names their
//! entities with solidcore's own scheme, the type code of the entity's kind and a key from
//! its kind and arena index, so its names match those of the Euler operators.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // test helpers

use solidcore::geom::attribute::ObjectKey;
use solidcore::geom::object::ObjectRef;
use solidcore::topology::loopuse::notify_ref;
use solidcore::topology::{
    BrepId, EdgeId, EdgeuseId, FaceId, FaceuseId, LoopId, LoopuseId, RegionId, ShellId, VertexId,
    VertexuseId,
};
use solidcore::EntityRef;

#[test]
fn a_face_and_a_vertex_are_named_by_type_code_kind_and_index() {
    assert_eq!(
        notify_ref(EntityRef::Face(FaceId::from_index(3))),
        ObjectRef {
            type_code: 16002,
            key: ObjectKey(1 + (4 << 32) + 3)
        }
    );
    assert_eq!(
        notify_ref(EntityRef::Vertex(VertexId::from_index(0))),
        ObjectRef {
            type_code: 16021,
            key: ObjectKey(1 + (10 << 32))
        }
    );
}

#[test]
fn every_kind_has_its_own_type_code_and_keys() {
    let i = 7;
    let refs = [
        EntityRef::Brep(BrepId::from_index(i)),
        EntityRef::Region(RegionId::from_index(i)),
        EntityRef::Shell(ShellId::from_index(i)),
        EntityRef::Faceuse(FaceuseId::from_index(i)),
        EntityRef::Face(FaceId::from_index(i)),
        EntityRef::Loopuse(LoopuseId::from_index(i)),
        EntityRef::Loop(LoopId::from_index(i)),
        EntityRef::Edgeuse(EdgeuseId::from_index(i)),
        EntityRef::Edge(EdgeId::from_index(i)),
        EntityRef::Vertexuse(VertexuseId::from_index(i)),
        EntityRef::Vertex(VertexId::from_index(i)),
    ]
    .map(notify_ref);
    let mut codes: Vec<i64> = refs.iter().map(|r| r.type_code).collect();
    let mut keys: Vec<u64> = refs.iter().map(|r| r.key.0).collect();
    codes.sort_unstable();
    codes.dedup();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!((codes.len(), keys.len()), (refs.len(), refs.len()));
}
