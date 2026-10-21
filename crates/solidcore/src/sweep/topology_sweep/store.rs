//! Production caller-owned storage for topology sweeps.
//!
//! Upstream: `source/SMLib/src/SmTopologySweep.cpp` (object/context access).
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`SweepStore`] | caller B-rep/context ownership |
//! | [`SweepIdentities`] | caller object pointer identities |
//!
//! Deviations: append-only handles replace pointers; callers supply all object
//! identities, including identities of future topology and original curves.
//! Mark exhaustion returns an error rather than upstream's fallback to an already
//! locked mark, so an adapter never releases another owner's lock.

use super::access::{EdgeRef, FaceRef, SweepAccess, SweepMergeId};
use super::SweepEntity;
use crate::blend::hcr::Hcr;
use crate::curve::curve::Curve;
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::attribute::ObjectKey;
use crate::geom::attribute::{ids, Attribute, AttributeStore, AttributeValue, Behavior, TagData};
use crate::geom::context::{MarkLock, MarkType};
use crate::geom::object::ObjectRef;
use crate::kernel::Kernel;
use crate::topology::edge::NotifyEnv;
use crate::topology::topology::Topology;
use crate::topology::topology_traverser::{self, Entity, Marks};
use crate::topology::{Brep, BrepId, EdgeId, FaceId};
use std::collections::BTreeMap;

const OP: &str = "sweep::topology_sweep::store";
fn invalid() -> SolidError {
    SolidError::invalid_input(OP, "unknown or released sweep handle")
}

enum Model<'a> {
    Borrowed(&'a mut Brep),
    Owned(Box<Brep>),
    Hcr(Box<Hcr>),
}
impl Model<'_> {
    fn get(&self) -> &Brep {
        match self {
            Self::Borrowed(b) => b,
            Self::Owned(b) => b,
            Self::Hcr(h) => h.scene_brep(),
        }
    }
    fn get_mut(&mut self) -> &mut Brep {
        match self {
            Self::Borrowed(b) => b,
            Self::Owned(b) => b,
            Self::Hcr(h) => h.scene_brep_mut(),
        }
    }
}
struct Entry<'a> {
    model: Model<'a>,
    context: usize,
}

/// Caller identity provider. It must assign stable distinct identities across
/// B-reps and cover future entities. Curve identities must refer to the original
/// geometry object rather than its owning edge.
pub trait SweepIdentities {
    /// Resolve a topology identity in its B-rep namespace.
    fn topology(&self, brep: BrepId, entity: EntityRef) -> ObjectRef;
    /// Enumerate exclusively owned auxiliary keys for temporary deletion. Include
    /// geometry and removed topology retained by the model; exclude shared/borrowed originals.
    /// The caller owns the geometry registry and must apply its actual ownership.
    fn owned_auxiliary_objects(&self, brep: BrepId, model: &Brep) -> Vec<ObjectKey>;
    /// Resolve the original curve object of an edge.
    fn curve(&self, brep: BrepId, edge: EdgeId, curve: &Curve) -> ObjectRef;
}

/// Production implementation of [`SweepAccess`] over actual caller B-reps and
/// kernels. Register shared kernels once and reuse their returned context index.
/// Borrowed originals are never deleted. Temporary IDs are never reused.
/// Attributes and notification records remain in the caller's stores. The identity
/// provider maps borrowed originals to stable caller identities. Reuse the caller
/// handle counter across sessions so temporary IDs cannot alias earlier entries.
pub struct SweepStore<'a> {
    kernels: Vec<&'a mut Kernel>,
    handle_base: u32,
    next_handle: &'a mut u32,
    entries: Vec<Option<Entry<'a>>>,
    identities: &'a dyn SweepIdentities,
    records: &'a mut BTreeMap<ObjectKey, Topology>,
    attributes: &'a mut AttributeStore,
    locks: Vec<(usize, MarkType, MarkLock)>,
}
impl<'a> SweepStore<'a> {
    /// Bind persistent caller-owned identity, notification and attribute state.
    /// `next_handle` is a monotonic caller registry counter, retained across stores.
    /// Never reset it while any handles, identities, records or attributes survive.
    pub fn new(
        identities: &'a dyn SweepIdentities,
        records: &'a mut BTreeMap<ObjectKey, Topology>,
        attributes: &'a mut AttributeStore,
        next_handle: &'a mut u32,
    ) -> Self {
        Self {
            kernels: Vec::new(),
            handle_base: *next_handle,
            next_handle,
            entries: Vec::new(),
            identities,
            records,
            attributes,
            locks: Vec::new(),
        }
    }
    /// Register one actual context. Reuse the index for shared-context B-reps.
    pub fn register_kernel(&mut self, kernel: &'a mut Kernel) -> usize {
        let id = self.kernels.len();
        self.kernels.push(kernel);
        id
    }
    /// Register an original borrowed model with an existing caller context.
    /// Its existing `Brep::key` and topology/geometry identities must agree with
    /// the identity provider. Registration preserves the original model's key.
    pub fn register_brep(&mut self, context: usize, brep: &'a mut Brep) -> Result<BrepId> {
        self.insert(context, Model::Borrowed(brep))
    }
    fn insert(&mut self, context: usize, mut model: Model<'a>) -> Result<BrepId> {
        self.kernels.get(context).ok_or_else(invalid)?;
        let index = *self.next_handle;
        let id = BrepId::from_index(index);
        if matches!(model, Model::Borrowed(_))
            && model.get().key() != self.identities.topology(id, EntityRef::Brep(id)).key
        {
            return Err(SolidError::invalid_input(
                OP,
                "borrowed B-rep identity disagrees with caller resolver",
            ));
        }
        *self.next_handle = index.checked_add(1).ok_or_else(invalid)?;
        if !matches!(model, Model::Borrowed(_)) {
            model
                .get_mut()
                .set_key(self.identities.topology(id, EntityRef::Brep(id)).key);
        }
        self.entries.push(Some(Entry { model, context }));
        Ok(id)
    }
    fn slot(&self, id: BrepId) -> Result<usize> {
        id.index()
            .checked_sub(self.handle_base)
            .map(|i| i as usize)
            .ok_or_else(invalid)
    }
    fn entry(&self, id: BrepId) -> Result<&Entry<'a>> {
        self.entries
            .get(self.slot(id)?)
            .and_then(Option::as_ref)
            .ok_or_else(invalid)
    }
    fn key(&self, entity: SweepEntity) -> Result<ObjectRef> {
        let (id, e, _) = entity_parts(entity);
        let b = self.brep(id)?;
        match entity {
            SweepEntity::Face(_, f) => {
                b.face(f)?;
            }
            SweepEntity::Edge(_, e) => {
                b.edge(e)?;
            }
            SweepEntity::Vertex(_, v) => {
                b.vertex(v)?;
            }
            SweepEntity::Region(_, r) => {
                b.region(r)?;
            }
        }
        Ok(self.identities.topology(id, e))
    }
    fn find_tag(&self, object: ObjectRef) -> Option<TagData> {
        self.attributes
            .find_attribute(object.key, ids::TAG)
            .and_then(|h| self.attributes.get(h))
            .and_then(|a| match a.value() {
                AttributeValue::Tag(t) => Some(*t),
                _ => None,
            })
    }
}
fn entity_parts(entity: SweepEntity) -> (BrepId, EntityRef, Option<Entity>) {
    match entity {
        SweepEntity::Face(b, f) => (b, EntityRef::Face(f), Some(Entity::Face(f))),
        SweepEntity::Edge(b, e) => (b, EntityRef::Edge(e), Some(Entity::Edge(e))),
        SweepEntity::Vertex(b, v) => (b, EntityRef::Vertex(v), Some(Entity::Vertex(v))),
        SweepEntity::Region(b, r) => (b, EntityRef::Region(r), None),
    }
}
impl SweepAccess for SweepStore<'_> {
    fn brep(&self, id: BrepId) -> Result<&Brep> {
        Ok(self.entry(id)?.model.get())
    }
    fn brep_mut(&mut self, id: BrepId) -> Result<&mut Brep> {
        let slot = self.slot(id)?;
        Ok(self
            .entries
            .get_mut(slot)
            .and_then(Option::as_mut)
            .ok_or_else(invalid)?
            .model
            .get_mut())
    }
    fn kernel(&self, id: BrepId) -> Result<&Kernel> {
        self.kernels
            .get(self.entry(id)?.context)
            .map(|k| &**k)
            .ok_or_else(invalid)
    }
    fn kernel_mut(&mut self, id: BrepId) -> Result<&mut Kernel> {
        let c = self.entry(id)?.context;
        self.kernels
            .get_mut(c)
            .map(|k| &mut **k)
            .ok_or_else(invalid)
    }
    fn with_brep_mut<T>(
        &mut self,
        id: BrepId,
        body: impl FnOnce(
            &mut Kernel,
            &mut Brep,
            &dyn Fn(FaceId) -> ObjectRef,
            &mut NotifyEnv<'_>,
        ) -> Result<T>,
    ) -> Result<T> {
        let slot = self.slot(id)?;
        let e = self
            .entries
            .get_mut(slot)
            .and_then(Option::as_mut)
            .ok_or_else(invalid)?;
        let k = self.kernels.get_mut(e.context).ok_or_else(invalid)?;
        let key = |entity| self.identities.topology(id, entity);
        let face = |f| key(EntityRef::Face(f));
        crate::geom::context::Context::with_notification_context(k, |k, context| {
            let mut env = NotifyEnv {
                records: self.records,
                attributes: self.attributes,
                context: Some(context),
                key_of: &key,
            };
            body(k, e.model.get_mut(), &face, &mut env)
        })
    }
    fn with_breps_mut<T>(
        &mut self,
        source: BrepId,
        destination: BrepId,
        body: impl FnOnce(
            &mut Kernel,
            &mut Brep,
            &mut Brep,
            &dyn Fn(FaceId) -> ObjectRef,
            &mut NotifyEnv<'_>,
        ) -> Result<T>,
    ) -> Result<T> {
        if source == destination {
            return Err(SolidError::invalid_input(
                OP,
                "source and destination alias",
            ));
        }
        self.entry(source)?;
        self.entry(destination)?;
        let s = self.slot(source)?;
        let d = self.slot(destination)?;
        let (first, rest) = self.entries.split_at_mut(s.max(d));
        let (src, dst) = if s < d {
            (first.get_mut(s), rest.first_mut())
        } else {
            (rest.first_mut(), first.get_mut(d))
        };
        let src = src.and_then(Option::as_mut).ok_or_else(invalid)?;
        let dst = dst.and_then(Option::as_mut).ok_or_else(invalid)?;
        let k = self.kernels.get_mut(dst.context).ok_or_else(invalid)?;
        let key = |entity| self.identities.topology(destination, entity);
        let face = |f| key(EntityRef::Face(f));
        crate::geom::context::Context::with_notification_context(k, |k, context| {
            let mut env = NotifyEnv {
                records: self.records,
                attributes: self.attributes,
                context: Some(context),
                key_of: &key,
            };
            body(k, src.model.get_mut(), dst.model.get_mut(), &face, &mut env)
        })
    }
    fn with_brep_notifications<T>(
        &mut self,
        id: BrepId,
        body: impl FnOnce(&mut Brep, &mut NotifyEnv<'_>) -> Result<T>,
    ) -> Result<T> {
        let slot = self.slot(id)?;
        let e = self
            .entries
            .get_mut(slot)
            .and_then(Option::as_mut)
            .ok_or_else(invalid)?;
        let k = self.kernels.get_mut(e.context).ok_or_else(invalid)?;
        let key = |entity| self.identities.topology(id, entity);
        let mut env = NotifyEnv {
            records: self.records,
            attributes: self.attributes,
            context: Some(&mut k.context),
            key_of: &key,
        };
        body(e.model.get_mut(), &mut env)
    }
    fn retain_hcr(&mut self, id: BrepId, hcr: Hcr) -> Result<BrepId> {
        let c = self.entry(id)?.context;
        self.insert(c, Model::Hcr(Box::new(hcr)))
    }
    fn hcr_session(&self, id: BrepId) -> Result<&Hcr> {
        match &self.entry(id)?.model {
            Model::Hcr(h) => Ok(h),
            _ => Err(invalid()),
        }
    }
    fn new_brep(&mut self, id: BrepId) -> Result<BrepId> {
        let c = self.entry(id)?.context;
        self.insert(c, Model::Owned(Box::new(Brep::new_with_infinite_region()?)))
    }
    fn delete_brep(&mut self, id: BrepId) {
        let Ok(index) = self.slot(id) else {
            return;
        };
        let Some(slot) = self.entries.get_mut(index) else {
            return;
        };
        if !slot
            .as_ref()
            .is_some_and(|e| !matches!(e.model, Model::Borrowed(_)))
        {
            return;
        }
        if let Some(entry) = slot.take() {
            let b = entry.model.get();
            let mut keys = self.identities.owned_auxiliary_objects(id, b);
            keys.push(self.identities.topology(id, EntityRef::Brep(id)).key);
            macro_rules! keys {
                ($arena:ident,$variant:ident) => {
                    for entity in b.$arena().ids() {
                        keys.push(
                            self.identities
                                .topology(id, EntityRef::$variant(entity))
                                .key,
                        );
                    }
                };
            }
            keys!(shells, Shell);
            keys!(faces, Face);
            keys!(loops, Loop);
            keys!(edges, Edge);
            keys!(vertices, Vertex);
            keys!(regions, Region);
            keys!(faceuses, Faceuse);
            keys!(loopuses, Loopuse);
            keys!(edgeuses, Edgeuse);
            keys!(vertexuses, Vertexuse);
            for key in keys {
                let _ = self.attributes.clear_attributes(key);
                self.records.remove(&key);
            }
        }
    }
    fn tag(&self, e: SweepEntity) -> Result<Option<TagData>> {
        Ok(self.find_tag(self.key(e)?))
    }
    fn add_tag(&mut self, e: SweepEntity, tag: TagData) -> Result<()> {
        let key = self.key(e)?.key;
        let h = self
            .attributes
            .insert(Attribute::tag(ids::TAG, tag, Behavior::Copy))?;
        self.attributes.add_attribute(key, h)
    }
    fn curve_tag(&self, edge: EdgeRef) -> Result<Option<TagData>> {
        let curve = self
            .brep(edge.0)?
            .edge(edge.1)?
            .curve
            .as_ref()
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        Ok(self.find_tag(self.identities.curve(edge.0, edge.1, curve)))
    }
    fn set_editing(&mut self, id: BrepId, editing: bool) -> Result<bool> {
        let b = self.brep_mut(id)?;
        let old = b.editing_enabled();
        b.set_editing_enabled(editing);
        Ok(old)
    }
    fn delete_merge(&mut self, _: SweepMergeId) {} // No merge registration can be created by this adapter.
    fn new_merge(
        &mut self,
        _: BrepId,
        _: BrepId,
        _: BrepId,
        _: f64,
        _: f64,
    ) -> Result<SweepMergeId> {
        // UNPORTED(boolean.merge_p2): SmMerge copy-notification registration bridge.
        // MergeState::with_breps exists; it does not register this caller's object identities.
        Err(SolidError::Unported {
            upstream: "SmMerge copy-notification registration bridge",
            unit: "boolean.merge_p2",
        })
    }
    fn lock_mark(&mut self, id: BrepId, mask: i32) -> Result<MarkType> {
        let c = self.entry(id)?.context;
        let k = self.kernels.get_mut(c).ok_or_else(invalid)?;
        if ![
            MarkType::Mark,
            MarkType::Mark2,
            MarkType::Mark3,
            MarkType::MarkIo,
            MarkType::MarkAssert,
        ]
        .into_iter()
        .any(|m| mask & m.code() != 0 && !k.context.is_mark_locked(m))
        {
            return Err(SolidError::invalid_input(OP, "no permitted unlocked mark"));
        }
        let lock = MarkLock::new(std::slice::from_mut(&mut k.context), 0, mask)?;
        let mark = lock.mark_type();
        self.locks.push((c, mark, lock));
        Ok(mark)
    }
    fn unlock_mark(&mut self, id: BrepId, mark: MarkType) -> Result<()> {
        let c = self.entry(id)?.context;
        if let Some(i) = self
            .locks
            .iter()
            .position(|(ctx, m, _)| *ctx == c && *m == mark)
        {
            let (_, _, mut lock) = self.locks.remove(i);
            let k = self.kernels.get_mut(c).ok_or_else(invalid)?;
            lock.release(std::slice::from_mut(&mut k.context))?;
        }
        Ok(())
    }
    fn is_marked(&self, e: SweepEntity, mark: MarkType) -> Result<bool> {
        let (id, _, _) = entity_parts(e);
        let key = self.key(e)?.key;
        Ok(self
            .records
            .get(&key)
            .copied()
            .unwrap_or_default()
            .is_marked(&self.kernel(id)?.context, mark))
    }
    fn mark(&mut self, e: SweepEntity, mark: MarkType) -> Result<()> {
        let (id, _, _) = entity_parts(e);
        let key = self.key(e)?.key;
        let c = self.entry(id)?.context;
        let k = self.kernels.get(c).ok_or_else(invalid)?;
        self.records
            .entry(key)
            .or_default()
            .mark(Some(&k.context), mark);
        Ok(())
    }
    fn collect_faces(
        &mut self,
        face: FaceRef,
        mark: MarkType,
        faces: &mut Vec<FaceId>,
    ) -> Result<()> {
        let e = self.entry(face.0)?;
        let k = self.kernels.get(e.context).ok_or_else(invalid)?;
        let b = e.model.get();
        let mut marks = Marks::default();
        let mut entities = Vec::new();
        macro_rules! collect {
            ($arena:ident,$variant:ident) => {
                for id in b.$arena().ids() {
                    let entity = Entity::$variant(id);
                    let key = self
                        .identities
                        .topology(face.0, EntityRef::$variant(id))
                        .key;
                    marks.set_record(entity, self.records.get(&key).copied().unwrap_or_default());
                    entities.push((entity, key));
                }
            };
        }
        collect!(faces, Face);
        collect!(edges, Edge);
        collect!(vertices, Vertex);
        let status = topology_traverser::collect_faces(
            k,
            b,
            &mut marks,
            Some(face.1),
            mark,
            faces,
            None,
            None,
        );
        for (entity, key) in entities {
            if let Some(record) = marks.record(entity) {
                self.records.insert(key, *record);
            }
        }
        status
    }
}
impl Drop for SweepStore<'_> {
    fn drop(&mut self) {
        for (c, _, lock) in &mut self.locks {
            if let Some(k) = self.kernels.get_mut(*c) {
                let _ = lock.release(std::slice::from_mut(&mut k.context));
            }
        }
        for i in 0..self.entries.len() {
            if let Ok(i) = u32::try_from(i) {
                if let Some(id) = self.handle_base.checked_add(i) {
                    self.delete_brep(BrepId::from_index(id));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::context::ALL_MARKS;
    use crate::topology::{Face, FaceuseId, LoopId, ShellId, Vertex, VertexId};
    pub(super) fn register<'a>(
        store: &mut SweepStore<'a>,
        context: usize,
        brep: &'a mut Brep,
    ) -> Result<BrepId> {
        let id = BrepId::from_index(*store.next_handle);
        brep.set_key(store.identities.topology(id, EntityRef::Brep(id)).key);
        store.register_brep(context, brep)
    }
    pub(super) struct Identities;
    impl SweepIdentities for Identities {
        fn topology(&self, b: BrepId, e: EntityRef) -> ObjectRef {
            let (kind, index) = match e {
                EntityRef::Brep(i) => (0, i.index()),
                EntityRef::Shell(i) => (1, i.index()),
                EntityRef::Face(i) => (2, i.index()),
                EntityRef::Loop(i) => (3, i.index()),
                EntityRef::Edge(i) => (4, i.index()),
                EntityRef::Vertex(i) => (5, i.index()),
                EntityRef::Region(i) => (6, i.index()),
                EntityRef::Faceuse(i) => (7, i.index()),
                EntityRef::Loopuse(i) => (8, i.index()),
                EntityRef::Edgeuse(i) => (9, i.index()),
                EntityRef::Vertexuse(i) => (10, i.index()),
            };
            let type_code = match e {
                EntityRef::Brep(_) => 16011,
                EntityRef::Shell(_) => 16013,
                EntityRef::Face(_) => 16002,
                EntityRef::Loop(_) => 16003,
                EntityRef::Edge(_) => 16016,
                EntityRef::Vertex(_) => 16021,
                EntityRef::Region(_) => 16012,
                EntityRef::Faceuse(_) => 16022,
                EntityRef::Loopuse(_) => 16004,
                EntityRef::Edgeuse(_) => 16005,
                EntityRef::Vertexuse(_) => 16006,
            };
            ObjectRef {
                type_code,
                key: ObjectKey((u64::from(b.index()) << 40) | (kind << 32) | u64::from(index)),
            }
        }
        fn curve(&self, b: BrepId, e: EdgeId, _: &Curve) -> ObjectRef {
            ObjectRef {
                type_code: 11001,
                key: ObjectKey((u64::from(b.index()) << 40) | (20 << 32) | u64::from(e.index())),
            }
        }
        fn owned_auxiliary_objects(&self, _: BrepId, _: &Brep) -> Vec<ObjectKey> {
            Vec::new()
        }
    }
    #[test]
    fn borrowed_key_mismatch_rejected_without_consuming_handle() {
        let mut k = Kernel::default();
        let mut b = Brep::new();
        b.set_key(ObjectKey(991));
        let mut records = BTreeMap::new();
        let mut attributes = AttributeStore::new();
        let mut next = 15;
        let mut store = SweepStore::new(&Identities, &mut records, &mut attributes, &mut next);
        let c = store.register_kernel(&mut k);
        assert!(store.register_brep(c, &mut b).is_err());
        assert_eq!(*store.next_handle, 15);
        assert!(store.entries.is_empty());
    }
    #[test]
    fn actual_shared_and_distinct_contexts_and_borrow_orders() {
        let mut k1 = Kernel::default();
        let mut k2 = Kernel::default();
        k1.context.set_doing_boolean(true);
        k1.tolerances.zone_tol_3d = 0.002;
        k2.tolerances.zone_tol_3d = 0.007;
        let mut a = Brep::new();
        let mut b = Brep::new();
        let mut c = Brep::new();
        a.set_editing_enabled(true);
        let mut records = BTreeMap::new();
        let mut attributes = AttributeStore::new();
        let mut next_handle = 0;
        let mut store =
            SweepStore::new(&Identities, &mut records, &mut attributes, &mut next_handle);
        let shared = store.register_kernel(&mut k1);
        let distinct = store.register_kernel(&mut k2);
        let a = register(&mut store, shared, &mut a).unwrap();
        let b = register(&mut store, shared, &mut b).unwrap();
        let c = register(&mut store, distinct, &mut c).unwrap();
        assert!(std::ptr::eq(
            store.kernel(a).unwrap(),
            store.kernel(b).unwrap()
        ));
        assert!(!std::ptr::eq(
            store.kernel(a).unwrap(),
            store.kernel(c).unwrap()
        ));
        for (source, destination) in [(a, c), (c, a), (a, b), (b, a)] {
            let expected = store.kernel(destination).unwrap().context.doing_boolean();
            let tolerance = store.kernel(destination).unwrap().tolerances.zone_tol_3d;
            store
                .with_breps_mut(source, destination, |k, src, dst, face, env| {
                    assert_eq!(k.context.doing_boolean(), expected);
                    assert_eq!(k.tolerances.zone_tol_3d, tolerance);
                    assert!(env.context.is_some());
                    assert_eq!(
                        face(FaceId::from_index(12)),
                        (env.key_of)(EntityRef::Face(FaceId::from_index(12)))
                    );
                    src.set_editing_enabled(true);
                    dst.set_editing_enabled(false);
                    Ok(())
                })
                .unwrap();
        }
        assert!(store.with_breps_mut(a, a, |_, _, _, _, _| Ok(())).is_err());
        assert!(store
            .with_breps_mut(a, BrepId::from_index(u32::MAX), |_, _, _, _, _| Ok(()))
            .is_err());
        let error: Result<()> = store.with_brep_mut(a, |_, _, _, env| {
            assert!(env.context.is_some());
            Err(invalid())
        });
        assert!(error.is_err());
        assert!(store.kernel(a).unwrap().context.doing_boolean());
    }
    #[test]
    fn retained_marks_partial_traversal_and_owned_cleanup() {
        let mut k = Kernel::default();
        let mut b = Brep::new();
        let mut face = Face::new(ShellId::from_index(0));
        face.loops.push(LoopId::from_index(99));
        face.faceuse = Some(FaceuseId::from_index(99));
        let f = b.add_face_record(face).unwrap();
        let mut records = BTreeMap::new();
        let mut attributes = AttributeStore::new();
        let mut next_handle = 0;
        let mut store =
            SweepStore::new(&Identities, &mut records, &mut attributes, &mut next_handle);
        let ctx = store.register_kernel(&mut k);
        let id = register(&mut store, ctx, &mut b).unwrap();
        let m = store.lock_mark(id, ALL_MARKS).unwrap();
        assert!(store.kernel(id).unwrap().context.is_mark_locked(m));
        let mut faces = vec![FaceId::from_index(77)];
        assert!(store.collect_faces((id, f), m, &mut faces).is_err());
        assert_eq!(faces, vec![f]);
        assert!(store.is_marked(SweepEntity::Face(id, f), m).unwrap());
        store.collect_faces((id, f), m, &mut faces).unwrap();
        assert!(faces.is_empty());
        // A notification-side record update is the next traversal's authority.
        store
            .with_brep_notifications(id, |_, env| {
                let key = (env.key_of)(EntityRef::Face(f)).key;
                env.records.insert(key, Topology::default());
                Ok(())
            })
            .unwrap();
        assert!(!store.is_marked(SweepEntity::Face(id, f), m).unwrap());
        assert!(store.collect_faces((id, f), m, &mut faces).is_err());
        assert_eq!(faces, vec![f]);
        let m2 = store.lock_mark(id, ALL_MARKS).unwrap();
        assert_ne!(m, m2);
        store.unlock_mark(id, m2).unwrap();
        assert!(store.kernel(id).unwrap().context.is_mark_locked(m));
        store.unlock_mark(id, m).unwrap();
        assert!(store
            .mark(SweepEntity::Vertex(id, VertexId::from_index(90)), m)
            .is_err());
        let owned = store.new_brep(id).unwrap();
        let v = store
            .brep_mut(owned)
            .unwrap()
            .add_vertex_record(Vertex::default())
            .unwrap();
        let entity = SweepEntity::Vertex(owned, v);
        store
            .add_tag(
                entity,
                TagData {
                    primary_id: 41,
                    ..TagData::default()
                },
            )
            .unwrap();
        store
            .add_tag(
                entity,
                TagData {
                    primary_id: 42,
                    ..TagData::default()
                },
            )
            .unwrap();
        assert_eq!(store.tag(entity).unwrap().unwrap().primary_id, 41);
        let key = store.key(entity).unwrap().key;
        store.records.insert(key, Topology::default());
        store.delete_brep(owned);
        store.delete_brep(owned);
        assert!(store.brep(owned).is_err());
        assert!(store.attributes.attributes(key).is_empty());
        assert!(!store.records.contains_key(&key));
        let later = store.new_brep(id).unwrap();
        assert!(later.index() > owned.index());
        store.delete_brep(id);
        assert!(store.brep(id).is_ok());
    }
}

#[cfg(test)]
mod production_path_tests {
    use super::tests::{register, Identities};
    use super::*;
    use crate::blend::hcr::HcrOptions;
    use crate::geom::axis2_placement::Axis2Placement;
    use crate::geom::extent3d::Extent3d;
    use crate::sweep::topology_sweep::engine::SweepTargets;
    use crate::sweep::topology_sweep::geometry::SweepGeometry;
    use crate::sweep::topology_sweep::{SweepGeometryCreation, TopologySweep};
    #[test]
    fn public_sweep_early_failure_restores_shared_and_distinct_flags() {
        for shared in [false, true] {
            let mut k1 = Kernel::default();
            let mut k2 = Kernel::default();
            k2.context.set_doing_boolean(true);
            let mut original = Brep::new_with_infinite_region().unwrap();
            let mut destination = Brep::new_with_infinite_region().unwrap();
            let mut records = BTreeMap::new();
            let mut attrs = AttributeStore::new();
            let mut next_handle = 0;
            let mut store =
                SweepStore::new(&Identities, &mut records, &mut attrs, &mut next_handle);
            let c1 = store.register_kernel(&mut k1);
            let c2 = if shared {
                c1
            } else {
                store.register_kernel(&mut k2)
            };
            let source = register(&mut store, c1, &mut original).unwrap();
            let dest = register(&mut store, c2, &mut destination).unwrap();
            let region = store.infinite_region(dest).unwrap();
            let before_source = store.kernel(source).unwrap().context.doing_boolean();
            let before_dest = store.kernel(dest).unwrap().context.doing_boolean();
            let mut targets = SweepTargets {
                faces_higher: Some(Vec::new()),
                ..SweepTargets::default()
            };
            let error = TopologySweep::default()
                .do_sweep(
                    &mut store,
                    &SweepGeometry::Base(SweepGeometryCreation::default()),
                    Some(source),
                    Some((dest, region)),
                    &mut targets,
                )
                .unwrap_err();
            assert!(error.is_unported());
            assert_eq!(
                store.kernel(source).unwrap().context.doing_boolean(),
                before_source
            );
            assert_eq!(
                store.kernel(dest).unwrap().context.doing_boolean(),
                before_dest
            );
            assert_eq!(
                store
                    .entries
                    .iter()
                    .filter(|e| e
                        .as_ref()
                        .is_some_and(|e| !matches!(e.model, Model::Borrowed(_))))
                    .count(),
                0
            );
        }
    }
    #[test]
    fn hcr_scene_access_and_idempotent_release() {
        let mut k = Kernel::default();
        let mut original = Brep::new_with_infinite_region().unwrap();
        let mut records = BTreeMap::new();
        let mut attrs = AttributeStore::new();
        let mut next_handle = 0;
        let mut store = SweepStore::new(&Identities, &mut records, &mut attrs, &mut next_handle);
        let c = store.register_kernel(&mut k);
        let original = register(&mut store, c, &mut original).unwrap();
        let hcr = Hcr::from_scene(
            store.kernel(original).unwrap(),
            Brep::new_with_infinite_region().unwrap(),
            &Axis2Placement::default(),
            Extent3d::new(),
            HcrOptions {
                approx_tol_3d: 0.01,
                ang_tol_rad: 0.02,
                display_seams: true,
                display_smooth_edges: true,
            },
        );
        let scene = store.retain_hcr(original, hcr).unwrap();
        assert!(std::ptr::eq(
            store.brep(scene).unwrap(),
            store.hcr_session(scene).unwrap().scene_brep()
        ));
        store.brep_mut(scene).unwrap().set_editing_enabled(true);
        assert!(store
            .hcr_session(scene)
            .unwrap()
            .scene_brep()
            .editing_enabled());
        store
            .with_breps_mut(scene, original, |_, s, d, _, _| {
                assert!(s.editing_enabled());
                assert!(!d.editing_enabled());
                Ok(())
            })
            .unwrap();
        store.delete_brep(scene);
        store.delete_brep(scene);
        assert!(store.hcr_session(scene).is_err());
        assert!(store.brep(scene).is_err());
        assert!(store.brep(original).is_ok());
    }
}

#[cfg(test)]
mod persistence_tests {
    use super::tests::{register, Identities};
    use super::*;
    use crate::curve::line::Line;
    use crate::geom::context::{NotifyCallback, ALL_MARKS};
    use crate::geom::object::{self, NotifyOperation};
    use crate::math::Vec3;
    use crate::topology::{Edge, Vertex, VertexId};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[derive(Debug)]
    struct Counter(Arc<AtomicUsize>);
    impl NotifyCallback for Counter {
        fn execute(
            &mut self,
            _: NotifyOperation,
            _: ObjectRef,
            _: Option<ObjectRef>,
            _: Option<ObjectRef>,
            _: Option<ObjectRef>,
        ) -> Result<()> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }
    #[test]
    fn actual_callbacks_survive_early_error_and_mark_exhaustion_preserves_owner() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut k = Kernel::default();
        k.context
            .set_user_notify_callback(Some(Box::new(Counter(calls.clone()))));
        let mut foreign = MarkLock::new(
            std::slice::from_mut(&mut k.context),
            0,
            MarkType::Mark.code(),
        )
        .unwrap();
        let mut b = Brep::new();
        let mut records = BTreeMap::new();
        let mut attrs = AttributeStore::new();
        let mut next = 0;
        {
            let mut store = SweepStore::new(&Identities, &mut records, &mut attrs, &mut next);
            let c = store.register_kernel(&mut k);
            let id = register(&mut store, c, &mut b).unwrap();
            let error: Result<()> = store.with_brep_mut(id, |_, _, _, env| {
                object::notify(
                    env.context.as_deref_mut(),
                    NotifyOperation::PreEdit,
                    (env.key_of)(EntityRef::Brep(id)),
                    None,
                    None,
                    None,
                );
                Err(invalid())
            });
            assert!(error.is_err());
            store
                .with_brep_notifications(id, |_, env| {
                    object::notify(
                        env.context.as_deref_mut(),
                        NotifyOperation::PostEdit,
                        (env.key_of)(EntityRef::Brep(id)),
                        None,
                        None,
                        None,
                    );
                    Ok(())
                })
                .unwrap();
            assert_eq!(calls.load(Ordering::SeqCst), 2);
            assert!(store.lock_mark(id, MarkType::Mark.code()).is_err());
            assert!(store
                .kernel(id)
                .unwrap()
                .context
                .is_mark_locked(MarkType::Mark));
            let _owned = store.lock_mark(id, ALL_MARKS).unwrap();
        }
        assert!(k.context.is_mark_locked(MarkType::Mark));
        assert!(!k.context.is_mark_locked(MarkType::Mark2));
        assert!(k.context.user_notify_callback().is_some());
        foreign
            .release(std::slice::from_mut(&mut k.context))
            .unwrap();
    }
    struct StableOriginal {
        local: BrepId,
    }
    impl SweepIdentities for StableOriginal {
        fn topology(&self, b: BrepId, e: EntityRef) -> ObjectRef {
            Identities.topology(
                if b == self.local {
                    BrepId::from_index(0)
                } else {
                    b
                },
                e,
            )
        }
        fn curve(&self, _: BrepId, _: EdgeId, c: &Curve) -> ObjectRef {
            ObjectRef {
                type_code: c.type_code(),
                key: ObjectKey(900_000),
            }
        }
        fn owned_auxiliary_objects(&self, _: BrepId, _: &Brep) -> Vec<ObjectKey> {
            Vec::new()
        }
    }
    #[test]
    fn persistent_original_attributes_monotonic_handles_and_curve_identity() {
        let mut k = Kernel::default();
        let mut original = Brep::new();
        let v = original.add_vertex_record(Vertex::default()).unwrap();
        let mut edge = Edge::new([v, v]);
        edge.curve =
            Some(Line::from_points(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), 3).into());
        let edge = original.add_edge_record(edge).unwrap();
        let mut records = BTreeMap::new();
        let mut attrs = AttributeStore::new();
        let mut next = 0;
        let old_temporary;
        let curve_tag = attrs
            .insert(Attribute::tag(
                ids::TAG,
                TagData {
                    primary_id: 73,
                    ..TagData::default()
                },
                Behavior::Copy,
            ))
            .unwrap();
        attrs.add_attribute(ObjectKey(900_000), curve_tag).unwrap();
        {
            let identities = StableOriginal {
                local: BrepId::from_index(next),
            };
            let mut store = SweepStore::new(&identities, &mut records, &mut attrs, &mut next);
            let c = store.register_kernel(&mut k);
            let id = register(&mut store, c, &mut original).unwrap();
            store
                .add_tag(
                    SweepEntity::Vertex(id, v),
                    TagData {
                        primary_id: 81,
                        ..TagData::default()
                    },
                )
                .unwrap();
            assert_eq!(store.curve_tag((id, edge)).unwrap().unwrap().primary_id, 73);
            old_temporary = store.new_brep(id).unwrap();
            let temporary_v = store
                .brep_mut(old_temporary)
                .unwrap()
                .add_vertex_record(Vertex::default())
                .unwrap();
            store
                .add_tag(
                    SweepEntity::Vertex(old_temporary, temporary_v),
                    TagData::default(),
                )
                .unwrap();
        }
        let temporary_key = Identities
            .topology(old_temporary, EntityRef::Vertex(VertexId::from_index(0)))
            .key;
        assert!(attrs.attributes(temporary_key).is_empty());
        {
            let identities = StableOriginal {
                local: BrepId::from_index(next),
            };
            let mut store = SweepStore::new(&identities, &mut records, &mut attrs, &mut next);
            let c = store.register_kernel(&mut k);
            let id = register(&mut store, c, &mut original).unwrap();
            assert_eq!(
                store
                    .tag(SweepEntity::Vertex(id, v))
                    .unwrap()
                    .unwrap()
                    .primary_id,
                81
            );
            assert_eq!(store.curve_tag((id, edge)).unwrap().unwrap().primary_id, 73);
            assert!(store.brep(old_temporary).is_err());
            let new = store.new_brep(id).unwrap();
            assert!(new.index() > old_temporary.index());
        }
        assert!(attrs.is_live(curve_tag));
    }
}

#[cfg(test)]
mod partial_sweep_tests {
    use super::tests::{register, Identities};
    use super::*;
    use crate::math::Vec3;
    use crate::sweep::topology_sweep::engine::SweepTargets;
    use crate::sweep::topology_sweep::geometry::SweepGeometry;
    use crate::sweep::topology_sweep::TopologySweep;
    use crate::sweep::trans_sweep_geometry::TranslationalSweepGeometry;
    #[test]
    fn public_translation_keeps_completed_destination_vertex_on_missing_wire_constructor() {
        for shared in [false, true] {
            let mut k1 = Kernel::default();
            let mut k2 = Kernel::default();
            let mut original = Brep::new_with_infinite_region().unwrap();
            let mut destination = Brep::new_with_infinite_region().unwrap();
            let v = original
                .add_vertex_record(crate::topology::Vertex::default())
                .unwrap();
            let mut records = BTreeMap::new();
            let mut attributes = AttributeStore::new();
            let mut next = 0;
            let mut store = SweepStore::new(&Identities, &mut records, &mut attributes, &mut next);
            let c1 = store.register_kernel(&mut k1);
            let c2 = if shared {
                c1
            } else {
                store.register_kernel(&mut k2)
            };
            let source = register(&mut store, c1, &mut original).unwrap();
            let dest = register(&mut store, c2, &mut destination).unwrap();
            let region = store.infinite_region(dest).unwrap();
            let mut sweep = TopologySweep {
                do_stitching: false,
                ..TopologySweep::default()
            };
            let mut targets = SweepTargets {
                vertices_higher: Some(vec![Some((source, v))]),
                ..SweepTargets::default()
            };
            let geometry = SweepGeometry::Translation(TranslationalSweepGeometry::new(Vec3::new(
                0.0, 0.0, 2.0,
            )));
            let error = sweep
                .do_sweep(
                    &mut store,
                    &geometry,
                    Some(source),
                    Some((dest, region)),
                    &mut targets,
                )
                .unwrap_err();
            assert!(matches!(
                error,
                SolidError::Unported {
                    upstream: "SmBrep::MakeWireEdgeVertex",
                    ..
                }
            ));
            assert_eq!(store.brep(dest).unwrap().vertices().len(), 1);
            assert_eq!(store.brep(dest).unwrap().shells().len(), 1);
            assert_eq!(store.brep(source).unwrap().vertices().len(), 1);
            assert_eq!(store.brep(source).unwrap().shells().len(), 0);
            assert!(sweep.to_same.is_empty());
            assert!(sweep.to_higher.is_empty());
            assert!(!store.kernel(source).unwrap().context.doing_boolean());
            assert!(!store.kernel(dest).unwrap().context.doing_boolean());
        }
    }
}
