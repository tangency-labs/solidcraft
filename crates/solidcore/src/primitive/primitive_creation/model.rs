//! A self-contained B-rep model that resolves the services `SmPrimitiveCreation`
//! calls, so the `SM_API` entry points can run it on a new B-rep.
//!
//! Upstream: `source/SMLib/src/SmPrimitiveCreation.cpp` (the `SmBrep`, `SmContext` and
//! `SmTopology` services its methods call through object pointers).
//!
//! | Rust                   | Upstream                                              |
//! |------------------------|-------------------------------------------------------|
//! | [`PrimitiveModel`]     | the caller's `SmBrep` objects and `SmContext`         |
//!
//! Deviations: upstream objects reach their B-rep and context through pointers; here
//! the model owns the B-reps (the first is the one being built, later ones are
//! scratch B-reps from `new SmBrep()`) and borrows the caller's [`Kernel`]. Edits
//! notify an arena-only store ([`ArenaNotifyState`]): no observer holds identities for
//! these objects. Tags and offset-map attributes live in the model (`SmAObject`
//! attributes); topology marks use
//! [`Marks`] per B-rep under the kernel context's mark locks. `SmMerge` objects are
//! retained sessions over the model's B-reps (`merge`). Services the model does not
//! resolve yet return [`SolidError::Unported`] naming the upstream call: retaining an
//! HCR session (`SmHCR`); its unit is partial: the model, not the unit, lacks the
//! bridge. No recursion.

use std::collections::BTreeMap;

use crate::blend::hcr::Hcr;
use crate::boolean::topology_intersector::p1::Relations;
use crate::curve::composite_curve::OffsetMapAttribute;
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::attribute::TagData;
use crate::geom::context::{MarkLock, MarkType};
use crate::geom::object::ObjectRef;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::nurbs::b_spline_curve::p1::BSplineCurve;
use crate::primitive::primitive_creation::p1::PrimitiveCreation;
use crate::primitive::primitive_creation::p2::PrimitiveCreationAccess;
use crate::primitive::primitive_creation::p3::PrimitiveAccess;
use crate::sweep::topology_sweep::access::{
    BTreeEdgeMap, EdgeRef, FaceRef, SweepAccess, SweepMergeId,
};
use crate::sweep::topology_sweep::SweepEntity;
use crate::topology::brep::p10::TrimType;
use crate::topology::brep::p2::ArenaNotifyState;
use crate::topology::edge::NotifyEnv;
use crate::topology::loopuse::notify_ref;
use crate::topology::topology_traverser::{Entity, Marks};
use crate::topology::{Brep, BrepId, FaceId};

mod merge;

const OP: &str = "primitive::primitive_creation::model";

fn face_ref(f: FaceId) -> ObjectRef {
    notify_ref(EntityRef::Face(f))
}

/// The B-reps an `SmPrimitiveCreation` call works on, with the caller's kernel.
#[derive(Debug)]
pub struct PrimitiveModel<'k> {
    k: &'k mut Kernel,
    breps: Vec<Option<Brep>>,
    marks: Vec<Marks>,
    tags: BTreeMap<SweepEntity, TagData>,
    offset_maps: BTreeMap<FaceRef, OffsetMapAttribute>,
    locks: Vec<(MarkType, MarkLock)>,
    merges: Vec<Option<merge::Session>>,
}

impl<'k> PrimitiveModel<'k> {
    /// The id of the B-rep the model was made with.
    pub const MAIN: BrepId = BrepId::from_index(0);

    /// A model over `brep` (id [`Self::MAIN`]) in `k`'s context.
    pub fn new(k: &'k mut Kernel, brep: Brep) -> Self {
        Self {
            k,
            breps: vec![Some(brep)],
            marks: vec![Marks::default()],
            tags: BTreeMap::new(),
            offset_maps: BTreeMap::new(),
            locks: Vec::new(),
            merges: Vec::new(),
        }
    }

    /// The B-rep the model was made with.
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` if it was deleted.
    pub fn into_brep(mut self) -> Result<Brep> {
        self.breps
            .get_mut(0)
            .and_then(Option::take)
            .ok_or_else(|| SolidError::null_pointer(OP))
    }

    /// Adds `brep` to the model (a caller's B-rep whose objects an operation reads);
    /// its id.
    ///
    /// # Errors
    ///
    /// `SM_ERR_INVALID_INPUT` past `u32::MAX` B-reps.
    pub fn add_brep(&mut self, brep: Brep) -> Result<BrepId> {
        let index = u32::try_from(self.breps.len())
            .map_err(|_| SolidError::invalid_input(OP, "too many B-reps"))?;
        self.breps.push(Some(brep));
        self.marks.push(Marks::default());
        Ok(BrepId::from_index(index))
    }

    /// Copies the caller's B-reps of `objects` into the model, each once (the same
    /// B-rep, by identity, is copied once), and returns every object with the id of
    /// its copy: upstream objects reach their own B-rep through their pointers.
    ///
    /// # Errors
    ///
    /// Those of [`Self::add_brep`].
    pub fn add_owner_breps<T: Copy>(&mut self, objects: &[(&Brep, T)]) -> Result<Vec<(BrepId, T)>> {
        let mut seen: Vec<(&Brep, BrepId)> = Vec::new();
        let mut out = Vec::new();
        for &(brep, object) in objects {
            let id = if let Some(&(_, id)) = seen.iter().find(|(b, _)| core::ptr::eq(*b, brep)) {
                id
            } else {
                let id = self.add_brep(brep.clone())?;
                seen.push((brep, id));
                id
            };
            out.push((id, object));
        }
        Ok(out)
    }

    /// `new SmBrep()` in `k`'s context, `SmPrimitiveCreation sPC(pBrep->GetInfiniteRegion(),
    /// 0, TRUE)` and `body` run on both through a model; the new B-rep and `body`'s
    /// value. On error the new B-rep is dropped (upstream deletes it).
    ///
    /// # Errors
    ///
    /// Those of the B-rep constructor and of `body`.
    pub fn build_new<T>(
        k: &'k mut Kernel,
        body: impl FnOnce(&mut PrimitiveCreation, &mut Self) -> Result<T>,
    ) -> Result<(Brep, T)> {
        let brep = crate::topology::brep::p3::new_brep(
            k,
            crate::geom::tol::USE_DEFAULT,
            crate::geom::tol::USE_DEFAULT,
        )?;
        let mut pc = PrimitiveCreation::new(brep.infinite_region(), 0, true);
        let mut model = Self::new(k, brep);
        let value = body(&mut pc, &mut model)?;
        let mut brep = model.into_brep()?;
        // (not upstream: the port's cached topology fields, true for the caller)
        brep.refresh_cached_topology();
        Ok((brep, value))
    }

    /// Takes B-rep `id` out of the model (a B-rep an operation made).
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` for an unknown or deleted B-rep.
    pub fn take_brep(&mut self, id: BrepId) -> Result<Brep> {
        self.breps
            .get_mut(id.index() as usize)
            .and_then(Option::take)
            .ok_or_else(|| SolidError::null_pointer(OP))
    }

    /// Runs `action` on merge `id` with the model's kernel; the session returns to its
    /// slot afterwards, also on error.
    fn run_merge<T>(
        &mut self,
        id: SweepMergeId,
        action: impl FnOnce(&mut Kernel, &mut merge::Driver<'_>) -> Result<T>,
    ) -> Result<T> {
        let slot = self
            .merges
            .get_mut(id.0 as usize)
            .ok_or_else(|| SolidError::invalid_input(OP, "unknown merge"))?;
        let mut session = slot
            .take()
            .ok_or_else(|| SolidError::invalid_input(OP, "unknown merge"))?;
        let mut driver = merge::Driver {
            breps: &mut self.breps,
            marks: &mut self.marks,
            offset_maps: &mut self.offset_maps,
            session: &mut session,
        };
        let result = action(self.k, &mut driver);
        if let Some(slot) = self.merges.get_mut(id.0 as usize) {
            *slot = Some(session);
        }
        result
    }

    fn slot(&mut self, id: BrepId) -> Result<&mut Brep> {
        self.breps
            .get_mut(id.index() as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| SolidError::null_pointer(OP))
    }
}

fn entity(e: SweepEntity) -> Result<(BrepId, Entity)> {
    match e {
        SweepEntity::Vertex(b, v) => Ok((b, Entity::Vertex(v))),
        SweepEntity::Edge(b, x) => Ok((b, Entity::Edge(x))),
        SweepEntity::Face(b, f) => Ok((b, Entity::Face(f))),
        SweepEntity::Region(..) => Err(SolidError::invalid_input(OP, "regions carry no mark")),
    }
}

impl SweepAccess for PrimitiveModel<'_> {
    fn brep(&self, id: BrepId) -> Result<&Brep> {
        self.breps
            .get(id.index() as usize)
            .and_then(Option::as_ref)
            .ok_or_else(|| SolidError::null_pointer(OP))
    }
    fn brep_mut(&mut self, id: BrepId) -> Result<&mut Brep> {
        self.slot(id)
    }
    fn kernel(&self, _: BrepId) -> Result<&Kernel> {
        Ok(self.k)
    }
    fn kernel_mut(&mut self, _: BrepId) -> Result<&mut Kernel> {
        Ok(self.k)
    }
    fn with_brep_mut<T>(
        &mut self,
        brep: BrepId,
        body: impl FnOnce(
            &mut Kernel,
            &mut Brep,
            &dyn Fn(FaceId) -> ObjectRef,
            &mut NotifyEnv<'_>,
        ) -> Result<T>,
    ) -> Result<T> {
        let b = self
            .breps
            .get_mut(brep.index() as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let mut state = ArenaNotifyState::new(b);
        let mut env = state.env(None);
        body(self.k, b, &face_ref, &mut env)
    }
    fn with_breps_mut<T>(
        &mut self,
        source: BrepId,
        destination: BrepId,
        body: impl FnOnce(
            &mut Kernel,
            &mut Brep,
            &mut Brep,
            &dyn Fn(EntityRef) -> ObjectRef,
            &dyn Fn(FaceId) -> ObjectRef,
            Option<&mut crate::geom::context::Context>,
            &mut NotifyEnv<'_>,
        ) -> Result<T>,
    ) -> Result<T> {
        let si = source.index() as usize;
        let di = destination.index() as usize;
        if si == di {
            return Err(SolidError::invalid_input(OP, "aliased B-reps"));
        }
        let (left, right) = self
            .breps
            .split_at_mut_checked(si.max(di))
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let (src, dst) = if si < di {
            (left.get_mut(si), right.first_mut())
        } else {
            (right.first_mut(), left.get_mut(di))
        };
        let src = src
            .and_then(Option::as_mut)
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let dst = dst
            .and_then(Option::as_mut)
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let mut state = ArenaNotifyState::new(dst);
        state.seed(src);
        let mut env = state.env(None);
        body(self.k, src, dst, &notify_ref, &face_ref, None, &mut env)
    }
    fn with_brep_notifications<T>(
        &mut self,
        brep: BrepId,
        body: impl FnOnce(&mut Brep, &mut NotifyEnv<'_>) -> Result<T>,
    ) -> Result<T> {
        let b = self
            .breps
            .get_mut(brep.index() as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let mut state = ArenaNotifyState::new(b);
        let mut env = state.env(Some(&mut self.k.context));
        body(b, &mut env)
    }
    fn retain_hcr(&mut self, _: BrepId, _: Hcr) -> Result<BrepId> {
        Err(SolidError::unported("SmHCR::SmHCR", "blend.hcr"))
    }
    fn hcr_session(&self, _: BrepId) -> Result<&Hcr> {
        Err(SolidError::unported("SmHCR::SmHCR", "blend.hcr"))
    }
    fn new_brep(&mut self, _: BrepId) -> Result<BrepId> {
        let b = crate::topology::brep::p3::new_brep(
            self.k,
            crate::geom::tol::USE_DEFAULT,
            crate::geom::tol::USE_DEFAULT,
        )?;
        self.add_brep(b)
    }
    fn delete_brep(&mut self, id: BrepId) {
        if id != Self::MAIN {
            if let Some(slot) = self.breps.get_mut(id.index() as usize) {
                *slot = None;
            }
        }
    }
    fn delete_merge(&mut self, merge: SweepMergeId) {
        if let Some(session) = self.merges.get_mut(merge.0 as usize).and_then(Option::take) {
            for brep in session.temporaries {
                self.delete_brep(brep);
            }
        }
    }
    fn new_merge(
        &mut self,
        _context_of: BrepId,
        destination: BrepId,
        source: BrepId,
        tolerance: f64,
        angle: f64,
    ) -> Result<SweepMergeId> {
        // SmMerge(*pContext, destination, source, tolerance, angle): the model has one
        // context.
        let session =
            merge::Session::new(self.k, &self.breps, destination, source, tolerance, angle)?;
        let id = SweepMergeId(
            u32::try_from(self.merges.len())
                .map_err(|_| SolidError::invalid_input(OP, "too many merges"))?,
        );
        self.merges.push(Some(session));
        Ok(id)
    }
    fn merge_operands(&self, merge: SweepMergeId) -> Result<(BrepId, BrepId)> {
        self.merges
            .get(merge.0 as usize)
            .and_then(Option::as_ref)
            .ok_or_else(|| SolidError::invalid_input(OP, "unknown merge"))?
            .operands()
    }
    fn merge_relations(&mut self, merge: SweepMergeId) -> Result<&mut Relations> {
        Ok(self
            .merges
            .get_mut(merge.0 as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| SolidError::invalid_input(OP, "unknown merge"))?
            .relations())
    }
    fn copy_faces(
        &mut self,
        source: BrepId,
        faces: &[FaceId],
        destination: BrepId,
        merge: SweepMergeId,
        edge_mates: &mut BTreeEdgeMap,
    ) -> Result<()> {
        let result = self.run_merge(merge, |k, driver| {
            driver.copy_faces(k, source, faces, destination)
        });
        edge_mates.extend(self.merge_relations(merge)?.edges.other_to_primary());
        result
    }
    fn piecewise_merge(
        &mut self,
        merge: SweepMergeId,
        use_existing_topology: bool,
        create_new_breps_for_faces: bool,
    ) -> Result<BrepId> {
        self.run_merge(merge, |k, driver| {
            crate::boolean::merge::p3::piecewise_merge(
                k,
                driver,
                None,
                use_existing_topology,
                create_new_breps_for_faces,
            )?
            .ok_or_else(|| SolidError::null_pointer(OP))
        })
    }
    fn union(&mut self, merge: SweepMergeId) -> Result<BrepId> {
        self.run_merge(merge, |k, driver| driver.union(k))
    }
    fn tag(&self, e: SweepEntity) -> Result<Option<TagData>> {
        Ok(self.tags.get(&e).copied())
    }
    fn add_tag(&mut self, e: SweepEntity, tag: TagData) -> Result<()> {
        self.tags.insert(e, tag);
        Ok(())
    }
    fn curve_tag(&self, edge: EdgeRef) -> Result<Option<TagData>> {
        self.brep(edge.0)?
            .edge(edge.1)?
            .curve
            .as_ref()
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        Ok(None)
    }
    fn set_editing(&mut self, brep: BrepId, editing: bool) -> Result<bool> {
        let b = self.slot(brep)?;
        let previous = b.editing_enabled();
        b.set_editing_enabled(editing);
        Ok(previous)
    }
    fn lock_mark(&mut self, _: BrepId, mask: i32) -> Result<MarkType> {
        let lock = MarkLock::new(std::slice::from_mut(&mut self.k.context), 0, mask)?;
        let mark = lock.mark_type();
        self.locks.push((mark, lock));
        Ok(mark)
    }
    fn unlock_mark(&mut self, _: BrepId, mark: MarkType) -> Result<()> {
        if let Some(at) = self.locks.iter().rposition(|(m, _)| *m == mark) {
            let (_, mut lock) = self.locks.remove(at);
            lock.release(std::slice::from_mut(&mut self.k.context))?;
        }
        Ok(())
    }
    fn is_marked(&self, e: SweepEntity, mark: MarkType) -> Result<bool> {
        let (b, x) = entity(e)?;
        let marks = self
            .marks
            .get(b.index() as usize)
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        Ok(marks.is_marked(self.k, x, mark))
    }
    fn mark(&mut self, e: SweepEntity, mark: MarkType) -> Result<()> {
        let (b, x) = entity(e)?;
        let marks = self
            .marks
            .get_mut(b.index() as usize)
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        marks.mark(self.k, x, mark);
        Ok(())
    }
    fn collect_faces(
        &mut self,
        face: FaceRef,
        mark: MarkType,
        faces: &mut Vec<FaceId>,
    ) -> Result<()> {
        let index = face.0.index() as usize;
        let b = self
            .breps
            .get(index)
            .and_then(Option::as_ref)
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let marks = self
            .marks
            .get_mut(index)
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        crate::topology::topology_traverser::collect_faces(
            self.k,
            b,
            marks,
            Some(face.1),
            mark,
            faces,
            None,
            None,
        )
    }
}

impl PrimitiveAccess for PrimitiveModel<'_> {
    fn with_primitive_brep<T>(
        &mut self,
        brep: BrepId,
        body: impl FnOnce(&mut Kernel, &mut Brep) -> Result<T>,
    ) -> Result<T> {
        let b = self
            .breps
            .get_mut(brep.index() as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        body(self.k, b)
    }
    fn delete_primitive_faces(&mut self, brep: BrepId, faces: &[FaceId]) -> Result<()> {
        // The rotational-sweep caller constructs a fresh context-only
        // intersector (SmPrimitiveCreation.cpp:2982), with no retained mates.
        // Reuse this model's actual notification records and object identities.
        self.with_brep_mut(brep, |k, b, _, env| {
            crate::boolean::merge::p4::KernelManifoldIntersector::delete_faces_with_keys(
                k,
                b,
                faces,
                env.records,
                env.attributes,
                env.key_of,
            )
        })
    }
}

impl PrimitiveCreationAccess for PrimitiveModel<'_> {
    fn copy_faces_into(
        &mut self,
        source: BrepId,
        faces: &[FaceId],
        destination: BrepId,
    ) -> Result<()> {
        // `SmBrep::CopyFaces(faces, destination, NULL, FALSE)`; its result is unused.
        self.with_breps_mut(source, destination, |k, src, dst, _, this_face, _, env| {
            crate::topology::brep::p7::copy_faces(k, src, faces, dst, false, this_face, env)
                .map(drop)
        })
    }
    fn attach_offset_map(&mut self, face: FaceRef, map: OffsetMapAttribute) -> Result<()> {
        self.brep(face.0)?.face(face.1)?;
        self.offset_maps.insert(face, map);
        Ok(())
    }
    fn offset_map(&self, face: FaceRef) -> Result<Option<OffsetMapAttribute>> {
        Ok(self.offset_maps.get(&face).copied())
    }
    fn project_and_trim(
        &mut self,
        brep: BrepId,
        curve: &BSplineCurve,
        direction: Vec3,
        reference: Point3,
        trim_type: TrimType,
        approx_tol: f64,
    ) -> Result<()> {
        // `ProjectAndTrim(curve, direction, reference, type, &tol, NULL, NULL)`: the
        // B-rep's objects keep the model's identities (`loopuse::notify_ref`).
        let keys = |object| crate::topology::brep::p2::caller_cache_key(&notify_ref, object);
        let env = crate::topology::brep::p10::ProjectEnv {
            brep_id: brep,
            keys: &keys,
            owner_of: &|_| None,
        };
        let b = self
            .breps
            .get_mut(brep.index() as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        // the merges' edits notify the B-rep's records under the same identities
        let mut state = ArenaNotifyState::new(b);
        let mut notify = state.env(None);
        crate::topology::brep::p10::project_and_trim(
            self.k,
            b,
            &env,
            &mut notify,
            curve,
            direction,
            reference,
            trim_type,
            &crate::topology::brep::p10::ProjectAndTrimOptions {
                approximation_tol_3d: Some(approx_tol),
                ..Default::default()
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::axis2_placement::Axis2Placement;
    use crate::geom::extent2d::Extent2d;
    use crate::math::Point2;
    use crate::surface::plane::Plane;
    use crate::surface::surface::Surface;

    fn plane_face(model: &mut PrimitiveModel<'_>, brep: BrepId) -> FaceId {
        let plane = Plane::create_canonical(&Axis2Placement::IDENTITY).unwrap();
        let mut face = None;
        model
            .surface_face(
                brep,
                Surface::from(plane),
                Extent2d::from_points(Point2::ZERO, Point2::new(1.0, 1.0)),
                &mut face,
            )
            .unwrap();
        face.unwrap()
    }

    #[test]
    fn rotational_face_deletion_edits_only_the_selected_model_brep() {
        let mut kernel = Kernel::default();
        let brep = crate::topology::brep::p3::new_brep(
            &mut kernel,
            crate::geom::tol::USE_DEFAULT,
            crate::geom::tol::USE_DEFAULT,
        )
        .unwrap();
        let mut model = PrimitiveModel::new(&mut kernel, brep);
        let main_face = plane_face(&mut model, PrimitiveModel::MAIN);
        let second = model.new_brep(PrimitiveModel::MAIN).unwrap();
        let second_face = plane_face(&mut model, second);
        // Stable arena indices overlap across these two distinct B-reps.
        assert_eq!(main_face, second_face);
        model.set_editing(second, true).unwrap();
        model
            .delete_primitive_faces(second, &[second_face])
            .unwrap();
        let selected = model.brep(second).unwrap();
        assert!(selected.face_list().is_empty());
        assert!(selected.edge_list().is_empty());
        assert!(selected.vertex_list().is_empty());
        let main = model.brep(PrimitiveModel::MAIN).unwrap();
        assert_eq!(main.face_list(), &[main_face]);
        assert_eq!(main.edge_list().len(), 4);
        assert_eq!(main.vertex_list().len(), 4);
        assert!(model
            .delete_primitive_faces(BrepId::from_index(99), &[main_face])
            .is_err());
        assert_eq!(
            model.brep(PrimitiveModel::MAIN).unwrap().face_list(),
            &[main_face]
        );
    }
}
