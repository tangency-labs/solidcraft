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
//! [`Marks`] per B-rep under the kernel context's mark locks. Services the model does
//! not resolve yet return [`SolidError::Unported`] naming the upstream call: retaining
//! an HCR session (`SmHCR`), the face-curve classification of a rotational sweep's
//! axis (`SmFace::CurveOnClassify`), the topology intersector's face deletion
//! (`SmTopologyIntersector::DeleteFaces`) and `SmBrep::ProjectAndTrim`. Their units are partial: the model, not the unit,
//! lacks the bridge; each error names the unit that owns the call. No recursion.

use std::collections::BTreeMap;

use crate::blend::hcr::Hcr;
use crate::curve::composite_curve::OffsetMapAttribute;
use crate::curve::curve::Curve;
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::attribute::TagData;
use crate::geom::context::{MarkLock, MarkType};
use crate::geom::extent2d::Extent2d;
use crate::geom::object::ObjectRef;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::nurbs::b_spline_curve::p1::BSplineCurve;
use crate::primitive::primitive_creation::p1::PrimitiveCreation;
use crate::primitive::primitive_creation::p2::PrimitiveCreationAccess;
use crate::primitive::primitive_creation::p3::PrimitiveAccess;
use crate::surface::surface::Surface;
use crate::sweep::topology_sweep::access::{
    EdgeRef, FaceRef, RegionRef, SweepAccess, SweepMergeId,
};
use crate::sweep::topology_sweep::SweepEntity;
use crate::topology::brep::p10::TrimType;
use crate::topology::brep::p2::ArenaNotifyState;
use crate::topology::brep::p8::FaceBuildEnv;
use crate::topology::edge::NotifyEnv;
use crate::topology::loopuse::notify_ref;
use crate::topology::topology_traverser::{Entity, Marks};
use crate::topology::{Brep, BrepId, FaceId};

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
        Ok((model.into_brep()?, value))
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

    fn slot(&mut self, id: BrepId) -> Result<&mut Brep> {
        self.breps
            .get_mut(id.index() as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| SolidError::null_pointer(OP))
    }

    fn with_face_env<T>(
        &mut self,
        brep: BrepId,
        body: impl FnOnce(&mut Kernel, &mut Brep, &mut FaceBuildEnv<'_, '_>) -> Result<T>,
    ) -> Result<T> {
        self.with_brep_mut(brep, |k, b, this_face, env| {
            let mut marks = Marks::default();
            let mut fenv = FaceBuildEnv {
                env,
                marks: &mut marks,
                this_face,
            };
            body(k, b, &mut fenv)
        })
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
            &dyn Fn(FaceId) -> ObjectRef,
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
        body(self.k, src, dst, &face_ref, &mut env)
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
    fn delete_merge(&mut self, _: SweepMergeId) {}
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
    fn planar_faces(
        &mut self,
        region: RegionRef,
        curves: &mut Vec<Option<Curve>>,
        tolerance: f64,
        faces: &mut Vec<FaceId>,
    ) -> Result<()> {
        let input: Vec<Curve> = curves.iter().flatten().cloned().collect();
        *faces = self.with_face_env(region.0, |k, b, fenv| {
            crate::topology::brep::p8::create_planar_faces_with_3d_curves(
                k,
                b,
                Some(region.1),
                &input,
                tolerance,
                fenv,
            )
        })?;
        curves.clear();
        Ok(())
    }
    fn planar_face(
        &mut self,
        region: RegionRef,
        curves: Vec<Curve>,
        tolerance: f64,
        face: &mut Option<FaceId>,
    ) -> Result<()> {
        *face = self.with_face_env(region.0, |k, b, fenv| {
            crate::topology::brep::p8::create_planar_face_with_3d_curves(
                k,
                b,
                Some(region.1),
                &curves,
                tolerance,
                fenv,
            )
        })?;
        Ok(())
    }
    fn surface_faces(
        &mut self,
        brep: BrepId,
        surface: Surface,
        domain: Extent2d,
        faces: &mut Vec<FaceId>,
    ) -> Result<()> {
        let made = self.with_face_env(brep, |k, b, fenv| {
            crate::topology::brep::p8::create_faces_from_surface(
                k,
                b,
                Some(surface),
                &domain,
                crate::geom::geom_utility::p2::ContinuityType::G1,
                fenv,
            )
        })?;
        faces.extend(made);
        Ok(())
    }
    fn surface_face(
        &mut self,
        brep: BrepId,
        surface: Surface,
        domain: Extent2d,
        face: &mut Option<FaceId>,
    ) -> Result<()> {
        *face = Some(self.with_face_env(brep, |k, b, fenv| {
            crate::topology::brep::p8::create_face_from_surface(k, b, Some(surface), &domain, fenv)
        })?);
        Ok(())
    }
    fn delete_primitive_faces(&mut self, _: BrepId, _: &[FaceId]) -> Result<()> {
        Err(SolidError::unported(
            "SmTopologyIntersector::DeleteFaces",
            "boolean.topology_intersector_p2",
        ))
    }
    fn axis_inside_face(&mut self, _: FaceRef, _: &Curve) -> Result<bool> {
        Err(SolidError::unported(
            "SmFace::CurveOnClassify",
            "query.face_p2",
        ))
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
        self.with_breps_mut(source, destination, |k, src, dst, this_face, env| {
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
        _: BrepId,
        _: &BSplineCurve,
        _: Vec3,
        _: Point3,
        _: TrimType,
        _: f64,
    ) -> Result<()> {
        Err(SolidError::unported(
            "SmBrep::ProjectAndTrim",
            "topology.brep_p10",
        ))
    }
}
