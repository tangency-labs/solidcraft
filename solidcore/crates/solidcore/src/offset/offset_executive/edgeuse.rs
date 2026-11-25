//! Cross-arena geometry adapter for offset edge caps.
//!
//! Upstream: `source/SMLib/src/SmOffsetExecutive.cpp` (2848–3100, the
//! `BuildOffsetOfEdgeuses` call to `EdgeuseOffset`),
//! `source/SMLib/src/SmOffsetGeometryCreation.cpp` (`EdgeuseOffset` and its helpers).
//!
//! | Rust | Upstream |
//! |------|----------|
//! | `build` (private) | `m_rGeometryCreation.EdgeuseOffset(..)` in `BuildOffsetOfEdgeuses` |
//! | `EdgeBrep` (private) | the original and offset B-reps behind `SmEdgeuse*`/`SmFace*`/curve pointers |
//! | `Executive` (private) | what `EdgeuseOffset` reads of `SmOffsetExecutive` |
//!
//! Deviations: arena-tagged handles stand in for pointers into the two B-reps and
//! for the creator's new curves and surfaces; owned values replace the output
//! pointers. Edits of either B-rep notify through the caller's stores under that
//! arena's identities, with the context the caller selected. Edge-use marks are read
//! from the caller's mark source. The extension cache is keyed by the face's
//! identity (upstream: the face surface's pointer); a new extension is copied into
//! the adapter's store, where its handle resolves. Successfully mated uses and selected
//! periodic-boundary events pass to the native executive independently of traversal
//! marks. Concrete curve kinds are unchanged. No new recursion.

use std::collections::{BTreeMap, BTreeSet};

use crate::curve::curve::Curve;
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::attribute::AttributeStore;
use crate::geom::context::{Context, MarkType};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::object::ObjectRef;
use crate::geom::solution_array::{ObjectKind, SolutionObject};
use crate::intersect::point_class::{ClassGeometry, ClassObject, ClassObjectKind};
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec3};
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::offset::offset_geometry_creation::{
    EdgeuseOffset, OffsetBrep, OffsetExecutive as CreatorExecutive, OffsetGeometryCreation,
};
use crate::query::edge::ClosestPoint;
use crate::surface::surface::Surface;
use crate::surface::surface_cache::face_context::{FaceQueries, PreparedFaceQueries};
use crate::surface::surface_cache::{SurfaceCache, SurfaceCacheState};
use crate::surface::volume::Volume;
use crate::topology::edge::{self, NotifyEnv, SetCurveOptions};
use crate::topology::edgeuse::{p1 as eu, p2 as eu2, p3 as eu3};
use crate::topology::face::{p1 as face1, p2 as face2};
use crate::topology::proto_topology::ProtoGeometry;
use crate::topology::{vertex, Brep, EdgeId, EdgeuseId, FaceId, VertexId};

use super::p1::{OffsetExecutive, OffsetOperation};

const OP: &str = "offset::offset_executive::edgeuse";

/// Which B-rep a topology handle or face-owned geometry handle names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Arena {
    /// The executive's original B-rep.
    Original,
    /// The executive's offset (temporary) B-rep.
    Offset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Object {
    Topology(Arena, EntityRef),
    EdgeCurve(Arena, EdgeId),
    /// The UV trim curve stored on this edge use.
    UvCurve(Arena, EdgeuseId),
    FaceSurface(Arena, FaceId),
    NewCurve(usize),
    NewSurface(usize),
}

/// An object handle of the creator; curve handles carry their natural interval.
#[derive(Debug, Clone, Copy)]
pub(super) struct Handle {
    object: Object,
    interval: Option<[f64; 2]>,
}

impl PartialEq for Handle {
    fn eq(&self, other: &Self) -> bool {
        self.object == other.object
    }
}

impl Handle {
    const fn new(object: Object) -> Self {
        Self {
            object,
            interval: None,
        }
    }

    const fn topology(arena: Arena, entity: EntityRef) -> Self {
        Self::new(Object::Topology(arena, entity))
    }

    fn curve(object: Object, curve: &Curve) -> Self {
        let interval = curve.natural_interval();
        Self {
            object,
            interval: Some([interval.min(), interval.max()]),
        }
    }

    fn entity(self) -> Result<(Arena, EntityRef)> {
        match self.object {
            Object::Topology(arena, entity) => Ok((arena, entity)),
            _ => Err(invalid_handle()),
        }
    }

    fn vertex(self) -> Result<(Arena, VertexId)> {
        match self.entity()? {
            (arena, EntityRef::Vertex(id)) => Ok((arena, id)),
            _ => Err(invalid_handle()),
        }
    }

    fn edge(self) -> Result<(Arena, EdgeId)> {
        match self.entity()? {
            (arena, EntityRef::Edge(id)) => Ok((arena, id)),
            _ => Err(invalid_handle()),
        }
    }

    fn face(self) -> Result<(Arena, FaceId)> {
        match self.entity()? {
            (arena, EntityRef::Face(id)) => Ok((arena, id)),
            _ => Err(invalid_handle()),
        }
    }

    fn edgeuse(self) -> Result<(Arena, EdgeuseId)> {
        match self.entity()? {
            (arena, EntityRef::Edgeuse(id)) => Ok((arena, id)),
            _ => Err(invalid_handle()),
        }
    }

    fn faceuse(self) -> Result<(Arena, crate::topology::FaceuseId)> {
        match self.entity()? {
            (arena, EntityRef::Faceuse(id)) => Ok((arena, id)),
            _ => Err(invalid_handle()),
        }
    }
}

impl ClassObject for Handle {
    fn class_kind(&self) -> ClassObjectKind {
        match self.object {
            Object::Topology(_, entity) => match entity {
                EntityRef::Vertex(_) => ClassObjectKind::Vertex,
                EntityRef::Edge(_) => ClassObjectKind::Edge,
                EntityRef::Face(_) => ClassObjectKind::Face,
                EntityRef::Loop(_) => ClassObjectKind::Loop,
                EntityRef::Region(_) => ClassObjectKind::Region,
                EntityRef::Vertexuse(_) => ClassObjectKind::Vertexuse,
                EntityRef::Edgeuse(_) => ClassObjectKind::Edgeuse,
                EntityRef::Faceuse(_) => ClassObjectKind::Faceuse,
                EntityRef::Loopuse(_) => ClassObjectKind::Loopuse,
                EntityRef::Shell(_) | EntityRef::Brep(_) => ClassObjectKind::Other,
            },
            Object::EdgeCurve(..) | Object::UvCurve(..) | Object::NewCurve(_) => {
                ClassObjectKind::Curve
            }
            Object::FaceSurface(..) | Object::NewSurface(_) => ClassObjectKind::Surface,
        }
    }
}

impl SolutionObject for Handle {
    fn kind(&self) -> ObjectKind {
        match self.object {
            Object::Topology(_, EntityRef::Vertex(_)) => ObjectKind::Vertex,
            Object::Topology(_, EntityRef::Edge(_)) => ObjectKind::Edge,
            Object::Topology(_, EntityRef::Face(_)) => ObjectKind::Face,
            Object::Topology(..) => ObjectKind::Other,
            Object::EdgeCurve(..) | Object::UvCurve(..) | Object::NewCurve(_) => ObjectKind::Curve,
            Object::FaceSurface(..) | Object::NewSurface(_) => ObjectKind::Surface,
        }
    }

    fn natural_interval(&self) -> Option<Extent1d> {
        self.interval
            .map(|[min, max]| Extent1d::from_min_max(min, max))
    }
}

fn invalid_handle() -> SolidError {
    SolidError::invalid_input(
        OP,
        "edge creator handle does not resolve to the requested kind",
    )
}

/// Where `SmEdgeuse::IsMarked` reads its marks.
#[derive(Clone, Copy)]
pub(super) enum MarkSource<'a> {
    /// The caller's topology records (the context's current mark values).
    Records,
    /// Original edge uses the calling loop has marked.
    Set(&'a BTreeSet<EdgeuseId>),
}

/// The two B-reps, the creator's new geometry, and the notification state of edits.
struct EdgeBrep<'a> {
    /// The kernel of the creator's calls, for the one query the trait passes none.
    kernel: &'a Kernel,
    original: &'a mut Brep,
    offset: &'a mut Brep,
    curves: Vec<Curve>,
    surfaces: Vec<Option<Surface>>,
    records: &'a mut crate::topology::topology::NotifyRecords,
    attributes: &'a mut AttributeStore,
    context: Option<&'a mut Context>,
    original_key_of: &'a dyn Fn(EntityRef) -> ObjectRef,
    offset_key_of: &'a dyn Fn(EntityRef) -> ObjectRef,
    marks: MarkSource<'a>,
}

impl core::fmt::Debug for EdgeBrep<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("EdgeBrep")
            .field("curves", &self.curves.len())
            .field("surfaces", &self.surfaces.len())
            .finish_non_exhaustive()
    }
}

impl EdgeBrep<'_> {
    fn brep(&self, arena: Arena) -> &Brep {
        match arena {
            Arena::Original => self.original,
            Arena::Offset => self.offset,
        }
    }

    fn brep_mut(&mut self, arena: Arena) -> &mut Brep {
        match arena {
            Arena::Original => self.original,
            Arena::Offset => self.offset,
        }
    }

    fn key_of(&self, arena: Arena) -> &dyn Fn(EntityRef) -> ObjectRef {
        match arena {
            Arena::Original => self.original_key_of,
            Arena::Offset => self.offset_key_of,
        }
    }

    /// Runs an edit of `arena` with that arena's notification environment.
    fn with_env<T>(
        &mut self,
        arena: Arena,
        run: impl FnOnce(&mut Brep, &mut NotifyEnv<'_>) -> Result<T>,
    ) -> Result<T> {
        let key_of = match arena {
            Arena::Original => self.original_key_of,
            Arena::Offset => self.offset_key_of,
        };
        let brep = match arena {
            Arena::Original => &mut *self.original,
            Arena::Offset => &mut *self.offset,
        };
        let mut env = NotifyEnv {
            records: &mut *self.records,
            attributes: &mut *self.attributes,
            context: self.context.as_deref_mut(),
            key_of,
        };
        run(brep, &mut env)
    }

    fn resolve_curve(&self, object: Object) -> Option<&Curve> {
        match object {
            Object::EdgeCurve(arena, e) => self.brep(arena).edge(e).ok()?.curve.as_ref(),
            Object::UvCurve(arena, owner) => {
                self.brep(arena).edgeuse(owner).ok()?.uv_trim_curve.as_ref()
            }
            Object::NewCurve(index) => self.curves.get(index),
            _ => None,
        }
    }

    fn resolve_surface(&self, object: Object) -> Option<&Surface> {
        match object {
            Object::FaceSurface(arena, f) => self.brep(arena).face(f).ok()?.surface.as_ref(),
            Object::NewSurface(index) => self.surfaces.get(index)?.as_ref(),
            _ => None,
        }
    }

    fn cloned_curve(&self, handle: Handle) -> Result<Curve> {
        self.resolve_curve(handle.object)
            .cloned()
            .ok_or_else(invalid_handle)
    }

    /// The face of either arena whose surface names `owner` (an offset copy keeps
    /// its base face's owner, but its handle is not a face-surface handle).
    fn face_for_owner(&self, owner: ObjectRef) -> Result<Option<(Arena, FaceId)>> {
        let mut found = None;
        for arena in [Arena::Original, Arena::Offset] {
            let brep = self.brep(arena);
            for f in brep.faces().ids() {
                let Some(surface) = brep.face(f)?.surface.as_ref() else {
                    continue;
                };
                if surface.face()? != Some(owner) {
                    continue;
                }
                if found.is_some() {
                    return Err(SolidError::invalid_input(
                        OP,
                        "surface owner names more than one face",
                    ));
                }
                found = Some((arena, f));
            }
        }
        Ok(found)
    }

    fn arena_of(&self, brep: &Brep) -> Option<Arena> {
        if core::ptr::eq(brep, self.original) {
            Some(Arena::Original)
        } else if core::ptr::eq(brep, self.offset) {
            Some(Arena::Offset)
        } else {
            None
        }
    }

    fn entity_owner_ref(&self, arena: Arena, entity: EntityRef) -> Option<ObjectRef> {
        let b = self.brep(arena);
        let expected = (self.key_of(arena))(entity);
        match entity {
            EntityRef::Edge(edge) => {
                let actual = b.edge(edge).ok()?.curve.as_ref().and_then(Curve::owner);
                if actual == Some(expected) {
                    return Some(expected);
                }
                if let Some(proven) = b.geometry_owner_key(entity) {
                    if actual == Some(proven) {
                        return Some(proven);
                    }
                }
                if let Some(serial) = b.notify_serial() {
                    let canonical = crate::topology::brep::p3::topology_ref(serial, entity);
                    if actual == Some(canonical) {
                        return Some(canonical);
                    }
                }
                Some(b.geometry_owner_key(entity).unwrap_or(expected))
            }
            EntityRef::Face(face) => {
                let actual = b.face(face).ok()?.surface.as_ref().and_then(Surface::owner);
                if actual == Some(expected) {
                    return Some(expected);
                }
                if let Some(proven) = b.geometry_owner_key(entity) {
                    if actual == Some(proven) {
                        return Some(proven);
                    }
                }
                if let Some(serial) = b.notify_serial() {
                    let canonical = crate::topology::brep::p3::topology_ref(serial, entity);
                    if actual == Some(canonical) {
                        return Some(canonical);
                    }
                }
                Some(b.geometry_owner_key(entity).unwrap_or(expected))
            }
            _ => None,
        }
    }

    fn take_output(mut self, made: EdgeuseOffset<Handle>) -> Result<EdgeCap> {
        let surface = match made.surface {
            None => None,
            Some(Handle {
                object: Object::NewSurface(index),
                ..
            }) => Some(
                self.surfaces
                    .get_mut(index)
                    .and_then(Option::take)
                    .ok_or_else(invalid_handle)?,
            ),
            Some(_) => return Err(invalid_handle()),
        };
        let mut taken = BTreeSet::new();
        let mut trim_curves = Vec::new();
        for handle in made.trimming_curves {
            let Object::NewCurve(index) = handle.object else {
                return Err(invalid_handle());
            };
            if !taken.insert(index) {
                return Err(invalid_handle());
            }
            trim_curves.push(self.curves.get(index).cloned().ok_or_else(invalid_handle)?);
        }
        Ok(EdgeCap {
            surface,
            trim_curves,
            trim_orientations: made.trim_orientations,
            closing_wall: made.closing_wall,
        })
    }
}

/// The cap geometry `EdgeuseOffset` made for one synchronized pair.
pub(super) struct EdgeCap {
    /// `pNewSurface`.
    pub(super) surface: Option<Surface>,
    /// `s3DTrimCurves`.
    pub(super) trim_curves: Vec<Curve>,
    /// `sTrimOrientations`.
    pub(super) trim_orientations: Vec<OrientType>,
    /// Not upstream (`bClosingWall`, the WR-530 addendum's step walls):
    /// the cap is a step wall, kept out of the face extensions.
    pub(super) closing_wall: bool,
}

impl ClassGeometry<Handle> for EdgeBrep<'_> {
    fn curve(&self, o: Handle) -> Option<&Curve> {
        self.resolve_curve(o.object)
    }
    fn surface(&self, o: Handle) -> Option<&Surface> {
        self.resolve_surface(o.object)
    }
    fn volume(&self, _o: Handle) -> Option<&Volume> {
        None
    }
    fn point(&self, _o: Handle) -> Option<Point3> {
        None
    }
    fn classification_vertex_point(&self, o: Handle) -> Option<Point3> {
        let (arena, v) = o.vertex().ok()?;
        vertex::point(self.brep(arena), v).ok()
    }
    fn topology_zone_tol_3d(&self, o: Handle) -> Option<f64> {
        let (arena, entity) = o.entity().ok()?;
        let b = self.brep(arena);
        match entity {
            EntityRef::Vertex(v) => vertex::tolerance(b, v).ok(),
            EntityRef::Edge(e) => edge::tolerance(b, e).ok(),
            EntityRef::Face(f) => face2::tolerance(b, f).ok(),
            EntityRef::Edgeuse(id) => eu::tolerance(b, id).ok(),
            _ => None,
        }
    }
    fn topology_entity(&self, o: Handle) -> Option<(&Brep, EntityRef)> {
        let (arena, entity) = o.entity().ok()?;
        Some((self.brep(arena), entity))
    }
    fn topology_handle_in(&self, brep: &Brep, entity: EntityRef) -> Option<Handle> {
        Some(Handle::topology(self.arena_of(brep)?, entity))
    }
    fn brep_id(&self, brep: &Brep) -> Option<crate::topology::BrepId> {
        let idx = match self.arena_of(brep)? {
            Arena::Original => 0,
            Arena::Offset => 1,
        };
        Some(crate::topology::BrepId::from_index(idx))
    }
    fn owner_entity(&self, owner: ObjectRef) -> Option<(&Brep, EntityRef)> {
        if owner.type_code == edge::TYPE {
            for arena in [Arena::Original, Arena::Offset] {
                let b = self.brep(arena);
                for e in b.edges().ids() {
                    let entity = EntityRef::Edge(e);
                    if self.entity_owner_ref(arena, entity) == Some(owner) {
                        return Some((b, entity));
                    }
                }
            }
            let mut fallback = None;
            for arena in [Arena::Original, Arena::Offset] {
                let b = self.brep(arena);
                for e in b.edges().ids() {
                    let actual = b.edge(e).ok().and_then(|r| r.curve.as_ref()?.owner());
                    if actual == Some(owner)
                        && crate::topology::brep::p1::default_edge_owner(b, e) == owner
                    {
                        if fallback.is_some() {
                            return None;
                        }
                        fallback = Some((b, EntityRef::Edge(e)));
                    }
                }
            }
            return fallback;
        }
        if owner.type_code == face2::TYPE {
            for arena in [Arena::Original, Arena::Offset] {
                let b = self.brep(arena);
                for f in b.faces().ids() {
                    let entity = EntityRef::Face(f);
                    if self.entity_owner_ref(arena, entity) == Some(owner) {
                        return Some((b, entity));
                    }
                }
            }
            let mut fallback = None;
            for arena in [Arena::Original, Arena::Offset] {
                let b = self.brep(arena);
                for f in b.faces().ids() {
                    let actual = b.face(f).ok().and_then(|r| r.surface.as_ref()?.owner());
                    if actual == Some(owner)
                        && crate::topology::brep::p1::default_face_owner(b, f) == owner
                    {
                        if fallback.is_some() {
                            return None;
                        }
                        fallback = Some((b, EntityRef::Face(f)));
                    }
                }
            }
            return fallback;
        }
        None
    }
    fn owner_of(&self, owner: ObjectRef) -> Option<ObjectRef> {
        if owner.type_code == eu::TYPE {
            for arena in [Arena::Original, Arena::Offset] {
                let b = self.brep(arena);
                for id in b.edgeuses().ids() {
                    let entity = EntityRef::Edgeuse(id);
                    let matches_key = (self.key_of(arena))(entity) == owner
                        || b.notify_serial().is_some_and(|s| {
                            crate::topology::brep::p3::topology_ref(s, entity) == owner
                        });
                    if matches_key {
                        let e = eu::edge(b, id).ok()??;
                        return self.entity_owner_ref(arena, EntityRef::Edge(e));
                    }
                }
            }
            let mut fallback = None;
            for arena in [Arena::Original, Arena::Offset] {
                let b = self.brep(arena);
                for id in b.edgeuses().ids() {
                    let actual = b
                        .edgeuse(id)
                        .ok()
                        .and_then(|r| r.uv_trim_curve.as_ref()?.owner());
                    if actual == Some(owner) {
                        let e = eu::edge(b, id).ok()??;
                        let edge_owner = self.entity_owner_ref(arena, EntityRef::Edge(e))?;
                        if fallback.is_some() {
                            return None;
                        }
                        fallback = Some(edge_owner);
                    }
                }
            }
            return fallback;
        }
        None
    }
}

impl ProtoGeometry<Handle> for EdgeBrep<'_> {
    fn curve_mut(&mut self, o: Handle) -> Option<&mut Curve> {
        match o.object {
            Object::EdgeCurve(arena, e) => self.brep_mut(arena).edge_mut(e).ok()?.curve.as_mut(),
            Object::UvCurve(arena, owner) => self
                .brep_mut(arena)
                .edgeuse_mut(owner)
                .ok()?
                .uv_trim_curve
                .as_mut(),
            Object::NewCurve(index) => self.curves.get_mut(index),
            _ => None,
        }
    }
}

fn cache_handle(arena: Arena) -> impl Fn(EntityRef) -> Result<Handle> {
    move |entity| Ok(Handle::topology(arena, entity))
}

impl FaceQueries<Handle> for EdgeBrep<'_> {
    fn face_zone_tol_3d(&self, face: Handle) -> Result<f64> {
        let (arena, f) = face.face()?;
        face2::tolerance(self.brep(arena), f)
    }

    fn face_has_loopuses(&self, face: Handle) -> Result<bool> {
        let (arena, f) = face.face()?;
        crate::topology::face::p4::has_loopuses(self.brep(arena), f)
    }

    fn face_point_classify(
        &self,
        k: &Kernel,
        face: Handle,
        uv: Point2,
        src_tol: f64,
    ) -> Result<(i32, Option<Handle>)> {
        let (arena, f) = face.face()?;
        let map = cache_handle(arena);
        PreparedFaceQueries::new(self.brep(arena), f, &map)?
            .face_point_classify(k, face, uv, src_tol)
    }

    fn face_point_classify_fast_drop(
        &self,
        k: &Kernel,
        face: Handle,
        uv: Point2,
        src_tol: f64,
    ) -> Result<(i32, Option<Handle>)> {
        let (arena, f) = face.face()?;
        let map = cache_handle(arena);
        PreparedFaceQueries::new(self.brep(arena), f, &map)?
            .face_point_classify_fast_drop(k, face, uv, src_tol)
    }

    fn curve_edge_tolerance(&self, curve: Handle) -> Option<Result<f64>> {
        if let Object::EdgeCurve(arena, id) = curve.object {
            Some(edge::tolerance(self.brep(arena), id))
        } else {
            let c = self.resolve_curve(curve.object)?;
            let edge_ref = match c.edge(|r| self.owner_of(r)) {
                Ok(Some(r)) => r,
                Ok(None) => return None,
                Err(e) => return Some(Err(e)),
            };
            let (b, EntityRef::Edge(id)) = self.owner_entity(edge_ref)? else {
                return None;
            };
            Some(edge::tolerance(b, id))
        }
    }
}

impl EdgeBrep<'_> {
    /// The B-rep face that owns `surface` (named by `handle`, else found by its owner),
    /// with the owner key: `None` for a surface no face of these B-reps owns, whose own
    /// cache `smsurf_GetSurfaceCache` makes.
    ///
    /// # Errors
    ///
    /// `InvalidInput` when the face named does not own the surface.
    fn surface_cache_face(
        &mut self,
        surface: &Surface,
        handle: Option<Handle>,
    ) -> Result<Option<(Arena, FaceId, ObjectRef)>> {
        let Some(owner) = surface.face()? else {
            return Ok(None);
        };
        let (arena, face) = if let Some(Object::FaceSurface(arena, face)) = handle.map(|h| h.object)
        {
            (arena, face)
        } else {
            let Some(found) = self.face_for_owner(owner)? else {
                return Ok(None);
            };
            found
        };
        let actual_owner = face2::surface(self.brep_mut(arena), face)?
            .ok_or_else(|| SolidError::invalid_input(OP, "cache face has no surface"))?
            .face()?;
        if actual_owner != Some(owner) {
            return Err(SolidError::invalid_input(
                OP,
                "surface owner is not the selected B-rep face",
            ));
        }
        Ok(Some((arena, face, owner)))
    }

    /// `smsurf_GetSurfaceCache` of `surface` for a solve, with its `GetTree`
    /// ([`OffsetBrep::prepare_surface_solve_cache`]).
    fn solve_surface_cache(
        &mut self,
        k: &Kernel,
        surface: &Surface,
        handle: Option<Handle>,
    ) -> Result<SurfaceCache<Handle>> {
        use crate::query::face::p2::solve_surface_cache_of_face;
        use crate::surface::surface::p1::{get_surface_cache, make_surface_cache};
        let Some((arena, face, owner)) = self.surface_cache_face(surface, handle)? else {
            // the solve's GetTree: a reused cache whose tree is not built builds it now
            let mut cache = get_surface_cache(k, surface, handle)?;
            if cache.tree().is_none() {
                cache.get_tree(k)?;
            }
            return Ok(cache);
        };
        // the surface's trimmed cache, kept with it as upstream keeps it (reused by
        // later solves on the same surface)
        let b = self.brep_mut(arena);
        match solve_surface_cache_of_face(k, b, face, owner, surface, handle, cache_handle(arena))?
        {
            Some(cache) => Ok(cache),
            None => make_surface_cache(k, surface, handle),
        }
    }

    /// The state bits of `smsurf_GetSurfaceCache` of `surface` as handed to a reader of
    /// them only, without the solve's `GetTree`
    /// ([`OffsetBrep::prepare_surface_state_cache`]).
    fn surface_cache_state(
        &mut self,
        k: &Kernel,
        surface: &Surface,
        handle: Option<Handle>,
    ) -> Result<SurfaceCacheState<Handle>> {
        use crate::query::face::p2::surface_cache_state_of_face;
        use crate::surface::surface::p1::{get_surface_cache, make_surface_cache};
        let Some((arena, face, owner)) = self.surface_cache_face(surface, handle)? else {
            return get_surface_cache(k, surface, handle)?.state();
        };
        // the surface's trimmed cache, kept with it as upstream keeps it
        let b = self.brep_mut(arena);
        match surface_cache_state_of_face(k, b, face, owner, surface, |entity| {
            Handle::topology(arena, entity)
        })? {
            Some(state) => Ok(state),
            None => make_surface_cache(k, surface, handle)?.state(),
        }
    }
}

impl OffsetBrep<Handle> for EdgeBrep<'_> {
    fn prepare_surface_solve_cache(
        &mut self,
        k: &Kernel,
        surface: &Surface,
        handle: Option<Handle>,
    ) -> Result<SurfaceCache<Handle>> {
        self.solve_surface_cache(k, surface, handle)
    }

    fn prepare_surface_state_cache(
        &mut self,
        k: &Kernel,
        surface: &Surface,
        handle: Option<Handle>,
    ) -> Result<SurfaceCacheState<Handle>> {
        self.surface_cache_state(k, surface, handle)
    }

    fn prepared_face_queries(&self) -> Option<&dyn FaceQueries<Handle>> {
        Some(self)
    }

    fn insert_curve(&mut self, curve: Curve) -> Option<Handle> {
        let handle = Handle::curve(Object::NewCurve(self.curves.len()), &curve);
        self.curves.push(curve);
        Some(handle)
    }

    fn insert_surface(&mut self, surface: Surface) -> Option<Handle> {
        let handle = Handle::new(Object::NewSurface(self.surfaces.len()));
        self.surfaces.push(Some(surface));
        Some(handle)
    }

    fn surface_mut(&mut self, o: Handle) -> Option<&mut Surface> {
        match o.object {
            Object::FaceSurface(arena, f) => {
                self.brep_mut(arena).face_mut(f).ok()?.surface.as_mut()
            }
            Object::NewSurface(index) => self.surfaces.get_mut(index)?.as_mut(),
            _ => None,
        }
    }

    fn brep_tolerance(&self, topo: Handle) -> Result<f64> {
        let (arena, _) = topo.entity()?;
        Ok(self.brep(arena).tolerance())
    }

    fn faceuse_face(&self, fu: Handle) -> Result<Option<Handle>> {
        let (arena, fu) = fu.faceuse()?;
        Ok(eu::faceuse_face(self.brep(arena), fu)?
            .map(|f| Handle::topology(arena, EntityRef::Face(f))))
    }

    fn faceuse_orientation(&self, fu: Handle) -> Result<OrientType> {
        let (arena, fu) = fu.faceuse()?;
        eu::faceuse_orientation(self.brep(arena), fu)
    }

    fn face_surface(&self, face: Handle) -> Result<Option<Handle>> {
        let (arena, f) = face.face()?;
        Ok(self
            .brep(arena)
            .face(f)?
            .surface
            .as_ref()
            .map(|_| Handle::new(Object::FaceSurface(arena, f))))
    }

    fn face_uv_domain(&self, face: Handle) -> Result<Extent2d> {
        let (arena, f) = face.face()?;
        face2::uv_domain(self.brep(arena), f)
    }

    fn set_face_uv_domain(&mut self, face: Handle, domain: Extent2d) -> Result<()> {
        let (arena, f) = face.face()?;
        face2::set_uv_domain(self.brep_mut(arena), f, &domain)
    }

    fn face_uv_domain_from_uv_trim_curves(&mut self, k: &Kernel, face: Handle) -> Result<Extent2d> {
        let (arena, f) = face.face()?;
        Ok(face1::calculate_uv_domain_from_uv_trim_curves(
            k,
            self.brep_mut(arena),
            f,
            true,
            false,
            false,
        )?
        .domain)
    }

    fn edge_curve(&self, e: Handle) -> Result<Option<Handle>> {
        let (arena, e) = e.edge()?;
        Ok(self
            .brep(arena)
            .edge(e)?
            .curve
            .as_ref()
            .map(|c| Handle::curve(Object::EdgeCurve(arena, e), c)))
    }

    fn edge_interval(&self, e: Handle) -> Result<Extent1d> {
        let (arena, e) = e.edge()?;
        edge::interval(self.brep(arena), e)
    }

    fn set_edge_interval(&mut self, e: Handle, interval: Extent1d) -> Result<()> {
        let (arena, e) = e.edge()?;
        edge::set_interval(self.brep_mut(arena), e, &interval)
    }

    fn edge_tolerance(&self, e: Handle) -> Result<f64> {
        let (arena, e) = e.edge()?;
        edge::tolerance(self.brep(arena), e)
    }

    fn set_edge_tolerance(&mut self, _k: &Kernel, e: Handle, tolerance: f64) -> Result<()> {
        let (arena, e) = e.edge()?;
        edge::set_tolerance(self.brep_mut(arena), e, tolerance, true, true)
    }

    fn edge_set_curve(&mut self, _k: &Kernel, e: Handle, curve: Handle) -> Result<()> {
        let (arena, e) = e.edge()?;
        let curve = self.cloned_curve(curve)?;
        let options = SetCurveOptions {
            delete_old_curve: true,
            dbg_warn_leaks: true,
            call_notify: true,
        };
        self.with_env(arena, |b, env| {
            edge::set_curve(b, e, Some(curve), options, env, [None, None])?;
            let owner = (env.key_of)(EntityRef::Edge(e));
            if let Some(c) = b.edge_mut(e)?.curve.as_mut() {
                c.set_owner(Some(owner), None);
            }
            b.set_geometry_owner_key(EntityRef::Edge(e), Some(owner));
            Ok(())
        })
    }

    fn brep_mate_lamina_edge_to_surface(
        &mut self,
        k: &Kernel,
        e: Handle,
        surface: Handle,
        surface_extension_factor: f64,
        uv_curve_extension_factor: f64,
    ) -> Result<()> {
        let (arena, e) = e.edge()?;
        // a const reference upstream: the mating surface is only read
        let mating = self
            .resolve_surface(surface.object)
            .cloned()
            .ok_or_else(invalid_handle)?;
        self.with_env(arena, |b, env| {
            crate::topology::brep::p2::mate_lamina_edge_to_surface(
                k,
                b,
                env,
                e,
                &mating,
                surface_extension_factor,
                uv_curve_extension_factor,
            )
        })
    }

    fn brep_replace_surface(
        &mut self,
        k: &Kernel,
        face: Handle,
        surface: Handle,
        add_to_deleted_list: bool,
        create_uv_trim_curves: bool,
    ) -> Result<()> {
        use crate::topology::brep::p2::OldSurface;
        let (arena, f) = face.face()?;
        let new_surface = self
            .resolve_surface(surface.object)
            .cloned()
            .ok_or_else(invalid_handle)?;
        let old = if add_to_deleted_list {
            OldSurface::AddToDeletedList
        } else {
            OldSurface::Delete
        };
        self.with_env(arena, |b, env| {
            let face_ref = (env.key_of)(EntityRef::Face(f));
            crate::topology::brep::p2::replace_surface(
                k,
                b,
                env,
                f,
                new_surface,
                face_ref,
                old,
                create_uv_trim_curves,
            )
            .map(|_| ())
        })
    }

    fn edge_faces(&self, e: Handle) -> Result<Vec<Handle>> {
        let (arena, e) = e.edge()?;
        Ok(edge::faces(self.brep(arena), e, None)?
            .into_iter()
            .map(|f| Handle::topology(arena, EntityRef::Face(f)))
            .collect())
    }

    fn edge_edgeuses(&self, e: Handle) -> Result<Vec<Handle>> {
        let (arena, e) = e.edge()?;
        Ok(edge::edgeuses(self.brep(arena), e, None, None)?
            .into_iter()
            .map(|u| Handle::topology(arena, EntityRef::Edgeuse(u)))
            .collect())
    }

    fn edge_is_closed(&self, e: Handle) -> Result<bool> {
        let (arena, e) = e.edge()?;
        edge::is_closed(self.brep(arena), e)
    }

    fn vertex_edges(&self, v: Handle) -> Result<Vec<Handle>> {
        let (arena, v) = v.vertex()?;
        Ok(vertex::edges(self.brep(arena), v, None)?
            .into_iter()
            .map(|e| Handle::topology(arena, EntityRef::Edge(e)))
            .collect())
    }

    fn faceuse_shell(&self, fu: Handle) -> Result<Option<Handle>> {
        let (arena, fu) = fu.faceuse()?;
        Ok(eu::faceuse_shell(self.brep(arena), fu)?
            .map(|s| Handle::topology(arena, EntityRef::Shell(s))))
    }

    fn edge_is_lamina(&self, e: Handle) -> Result<bool> {
        let (arena, e) = e.edge()?;
        edge::is_lamina(self.brep(arena), e)
    }

    fn edge_is_seam_of_face_surface(
        &mut self,
        k: &Kernel,
        e: Handle,
        face: Handle,
    ) -> Result<bool> {
        let (arena, e) = e.edge()?;
        let (face_arena, f) = face.face()?;
        if face_arena != arena {
            return Err(SolidError::invalid_input(OP, "face of the other B-rep"));
        }
        edge::is_seam_of_surface(k, self.brep_mut(arena), e, f)
    }

    fn edge_vertex(&self, e: Handle) -> Result<Option<Handle>> {
        let (arena, e) = e.edge()?;
        Ok(edge::vertex(self.brep(arena), e)?
            .map(|v| Handle::topology(arena, EntityRef::Vertex(v))))
    }

    fn edge_other_vertex(&self, e: Handle, v: Handle) -> Result<Option<Handle>> {
        let (arena, e) = e.edge()?;
        // a vertex of the other B-rep is no end of this edge
        let v = match v.vertex()? {
            (a, v) if a == arena => Some(v),
            _ => None,
        };
        Ok(edge::other_vertex(self.brep(arena), e, v)?
            .map(|v| Handle::topology(arena, EntityRef::Vertex(v))))
    }

    fn edge_closest_point(
        &self,
        k: &Kernel,
        e: Handle,
        point: Point3,
    ) -> Result<Option<ClosestPoint>> {
        let (arena, e) = e.edge()?;
        crate::query::edge::closest_point(k, self.brep(arena), e, point)
    }

    fn edge_create_trimmed_nurbs_curve(&mut self, k: &Kernel, e: Handle) -> Result<Option<Handle>> {
        let (arena, e) = e.edge()?;
        match edge::create_trimmed_nurbs_curve(k, self.brep(arena), e)? {
            Some(curve) => Ok(self.insert_curve(curve)),
            None => Ok(None),
        }
    }

    fn vertex_point(&self, v: Handle) -> Result<Point3> {
        let (arena, v) = v.vertex()?;
        vertex::point(self.brep(arena), v)
    }

    fn vertex_tolerance(&self, v: Handle) -> Result<f64> {
        let (arena, v) = v.vertex()?;
        vertex::tolerance(self.brep(arena), v)
    }

    fn set_vertex_point(&mut self, _k: &Kernel, v: Handle, point: Point3) -> Result<()> {
        let (arena, v) = v.vertex()?;
        self.with_env(arena, |b, env| {
            let key_of = env.key_of;
            let this = key_of(EntityRef::Vertex(v));
            vertex::set_point(
                b,
                v,
                point,
                env.records,
                env.attributes,
                env.context.as_deref_mut(),
                this,
                &|vu| key_of(EntityRef::Vertexuse(vu)),
            )
        })
    }

    fn vertex_is_lamina(&self, v: Handle) -> Result<bool> {
        let (arena, v) = v.vertex()?;
        vertex::is_lamina_vertex(self.brep(arena), v)
    }

    fn vertex_are_edges_tangent(
        &self,
        v: Handle,
        e1: Handle,
        e2: Handle,
        ang_tol_deg: f64,
    ) -> Result<bool> {
        let (arena, v) = v.vertex()?;
        let (a1, e1) = e1.edge()?;
        let (a2, e2) = e2.edge()?;
        if a1 != arena || a2 != arena {
            return Err(SolidError::invalid_input(OP, "edges of the other B-rep"));
        }
        vertex::are_edges_tangent(self.kernel, self.brep(arena), v, e1, e2, ang_tol_deg)
    }

    fn edgeuse_edge(&self, u: Handle) -> Result<Option<Handle>> {
        let (arena, u) = u.edgeuse()?;
        Ok(eu::edge(self.brep(arena), u)?.map(|e| Handle::topology(arena, EntityRef::Edge(e))))
    }

    fn edgeuse_mate(&self, u: Handle) -> Result<Option<Handle>> {
        let (arena, u) = u.edgeuse()?;
        Ok(eu::mate(self.brep(arena), u)?.map(|m| Handle::topology(arena, EntityRef::Edgeuse(m))))
    }

    fn edgeuse_radial(&self, u: Handle) -> Result<Option<Handle>> {
        let (arena, u) = u.edgeuse()?;
        Ok(
            eu::radial(self.brep(arena), u)?
                .map(|r| Handle::topology(arena, EntityRef::Edgeuse(r))),
        )
    }

    fn edgeuse_cw(&self, u: Handle) -> Result<Option<Handle>> {
        let (arena, u) = u.edgeuse()?;
        Ok(eu::cw_edgeuse(self.brep(arena), u)?
            .map(|r| Handle::topology(arena, EntityRef::Edgeuse(r))))
    }

    fn edgeuse_ccw(&self, u: Handle) -> Result<Option<Handle>> {
        let (arena, u) = u.edgeuse()?;
        Ok(eu::ccw_edgeuse(self.brep(arena), u)?
            .map(|r| Handle::topology(arena, EntityRef::Edgeuse(r))))
    }

    fn edgeuse_vertex(&self, u: Handle) -> Result<Option<Handle>> {
        let (arena, u) = u.edgeuse()?;
        let b = self.brep(arena);
        let Some(vu) = eu::vertexuse(b, u)? else {
            return Ok(None);
        };
        Ok(b.vertexuse(vu)?
            .vertex
            .map(|v| Handle::topology(arena, EntityRef::Vertex(v))))
    }

    fn edgeuse_face(&self, u: Handle) -> Result<Option<Handle>> {
        let (arena, u) = u.edgeuse()?;
        Ok(eu::face(self.brep(arena), u)?.map(|f| Handle::topology(arena, EntityRef::Face(f))))
    }

    fn edgeuse_orientation(&self, u: Handle) -> Result<OrientType> {
        let (arena, u) = u.edgeuse()?;
        eu::orientation(self.brep(arena), u)
    }

    fn edgeuse_faceuse(&self, u: Handle) -> Result<Option<Handle>> {
        let (arena, u) = u.edgeuse()?;
        Ok(eu::faceuse(self.brep(arena), u)?
            .map(|f| Handle::topology(arena, EntityRef::Faceuse(f))))
    }

    fn edgeuse_uv_trim_curve(&self, u: Handle) -> Result<Option<Handle>> {
        let (arena, u) = u.edgeuse()?;
        Ok(match eu::uv_trim_curve(self.brep(arena), u)? {
            (Some(c), Some(owner)) => Some(Handle::curve(Object::UvCurve(arena, owner), c)),
            _ => None,
        })
    }

    fn edgeuse_set_uv_trim_curve(
        &mut self,
        k: &Kernel,
        u: Handle,
        curve: Option<Handle>,
        max_dist: f64,
    ) -> Result<()> {
        let (arena, u) = u.edgeuse()?;
        let mut raise_by = max_dist;
        let arg = match curve {
            Some(h) => {
                let c = self.cloned_curve(h)?;
                // (not upstream, #18009: bounded by the trim's proved distance from the edge
                // curve where it is enclosed, see the trait method)
                raise_by = eu3::trim_raise_distance(k, self.brep_mut(arena), u, &c, max_dist)?;
                eu::UvTrimCurveArg::New(Box::new(c))
            }
            None => eu::UvTrimCurveArg::Null,
        };
        eu::set_uv_trim_curve(self.brep_mut(arena), u, arg, raise_by, true, None)?;
        Ok(())
    }

    fn edgeuse_is_tangent_sector(&mut self, k: &Kernel, u: Handle, tol_deg: f64) -> Result<bool> {
        let (arena, u) = u.edgeuse()?;
        eu2::is_tangent_sector(k, self.brep_mut(arena), u, tol_deg, false, true)
    }

    fn edgeuse_is_convex_radial_sector(
        &mut self,
        k: &Kernel,
        u: Handle,
        num_samples: u64,
    ) -> Result<bool> {
        let (arena, u) = u.edgeuse()?;
        eu2::is_convex_radial_sector(k, self.brep_mut(arena), u, num_samples, false, 0.0)
    }

    fn edgeuse_is_marked(&self, k: &Kernel, u: Handle, mark: MarkType) -> Result<bool> {
        let (arena, id) = u.edgeuse()?;
        self.brep(arena).edgeuse(id)?;
        match &self.marks {
            MarkSource::Set(set) => Ok(arena == Arena::Original && set.contains(&id)),
            MarkSource::Records => {
                let key = (self.key_of(arena))(EntityRef::Edgeuse(id)).key;
                Ok(self
                    .records
                    .get(&key)
                    .is_some_and(|record| record.is_marked(&k.context, mark)))
            }
        }
    }

    fn edgeuse_middle_binormal(&mut self, k: &Kernel, u: Handle) -> Result<Vec3> {
        let (arena, u) = u.edgeuse()?;
        let b = self.brep_mut(arena);
        let e = eu::edge(b, u)?
            .ok_or_else(|| SolidError::invalid_input(OP, "edge use without edge"))?;
        let middle = edge::interval(b, e)?.evaluate(0.5);
        let options = eu3::BinormalOptions::default();
        Ok(eu3::evaluate_binormal(k, b, u, middle, false, &options)?.binormal)
    }

    fn edgeuse_normalized_evaluate(
        &mut self,
        k: &Kernel,
        u: Handle,
        t: f64,
        parameter_space_eval: bool,
        want_first_deriv: bool,
        ok_to_make_uv_trim_curve: bool,
    ) -> Result<Point3> {
        let (arena, u) = u.edgeuse()?;
        Ok(eu3::normalized_evaluate(
            k,
            self.brep_mut(arena),
            u,
            t,
            parameter_space_eval,
            want_first_deriv,
            ok_to_make_uv_trim_curve,
        )?
        .point)
    }

    fn edgeuse_create_uv_trim_curve(
        &mut self,
        k: &Kernel,
        u: Handle,
        want_max_deviation: bool,
        want_drop_curve_fail: bool,
    ) -> Result<(f64, Curve)> {
        let (arena, u) = u.edgeuse()?;
        let mut fail = crate::surface::surface::p2::DropCurveFail::default();
        let made = eu3::create_uv_trim_curve(
            k,
            self.brep_mut(arena),
            u,
            want_max_deviation,
            want_drop_curve_fail.then_some(&mut fail),
        )?;
        Ok((made.max_dist_to_surf, made.curve))
    }
}

/// What `EdgeuseOffset` reads of the executive, over the adapter's handles.
struct Executive<'a> {
    inner: &'a mut OffsetExecutive,
    /// The face identities that key the extension cache (as the extended offset's
    /// face pass keys it), per arena.
    original_key_of: &'a dyn Fn(EntityRef) -> ObjectRef,
    offset_key_of: &'a dyn Fn(EntityRef) -> ObjectRef,
    /// The handles of extensions copied into the adapter's store, by cache key.
    made: BTreeMap<u64, Handle>,
}

impl CreatorExecutive<Handle> for Executive<'_> {
    fn has_mated_edgeuse(&self, original: Handle) -> bool {
        match original.object {
            Object::Topology(Arena::Original, EntityRef::Edgeuse(u)) => {
                self.inner.mated_edgeuses.contains(&u)
            }
            _ => false,
        }
    }

    fn note_mated_edgeuse(&mut self, original: Handle) -> Result<()> {
        let Object::Topology(Arena::Original, EntityRef::Edgeuse(u)) = original.object else {
            return Err(SolidError::port_invariant(
                OP,
                "mated edge use outside the source arena",
            ));
        };
        self.inner.mated_edgeuses.insert(u);
        Ok(())
    }

    fn note_periodic_boundary_end(
        &mut self,
        event: crate::offset::offset_geometry_creation::PeriodicBoundaryEnd<Handle>,
    ) -> Result<()> {
        let (
            Object::Topology(Arena::Original, EntityRef::Edgeuse(edgeuse)),
            Object::Topology(Arena::Original, EntityRef::Vertex(vertex)),
            Object::Topology(Arena::Original, EntityRef::Edgeuse(seam)),
            Object::Topology(Arena::Original, EntityRef::Edgeuse(across)),
        ) = (
            event.edgeuse.object,
            event.vertex.object,
            event.seam.object,
            event.across.object,
        )
        else {
            return Err(SolidError::port_invariant(
                OP,
                "periodic boundary event outside the source arena",
            ));
        };
        self.inner
            .periodic_boundary_ends
            .push(super::periodic_boundary::BoundaryEnd {
                edgeuse,
                vertex,
                seam,
                across,
                point: event.point,
            });
        Ok(())
    }

    fn inset(&self) -> bool {
        self.inner.inset()
    }

    fn extend_convex_edges(&self) -> bool {
        self.inner.extend_convex_edges()
    }

    fn shell_faces(&self) -> Vec<Handle> {
        self.inner
            .shell_faces()
            .iter()
            .map(|&f| Handle::topology(Arena::Original, EntityRef::Face(f)))
            .collect()
    }

    fn is_local_operation(&self) -> bool {
        self.inner.operation() == OffsetOperation::LocalOperation
    }

    fn keeps_edge(&self, original: Handle) -> bool {
        match original.object {
            Object::Topology(Arena::Original, EntityRef::Edgeuse(u)) => self.inner.keeps_edgeuse(u),
            _ => false,
        }
    }

    fn crosses_at(&self, original: Handle) -> bool {
        match original.object {
            Object::Topology(Arena::Original, EntityRef::Edgeuse(u)) => {
                self.inner.crosses_at_edgeuse(u)
            }
            _ => false,
        }
    }

    fn has_collapsed_faces(&self) -> bool {
        !self.inner.collapsed_faces.is_empty()
    }

    fn note_inverted_face(&mut self, offset_face: Handle) {
        if let Object::Topology(Arena::Offset, EntityRef::Face(f)) = offset_face.object {
            self.inner.inverted_faces.insert(f);
        }
    }

    fn is_collapsed_face(&self, face: Handle) -> bool {
        match face.object {
            Object::Topology(Arena::Original, EntityRef::Face(f)) => {
                self.inner.is_collapsed_face(f)
            }
            _ => false,
        }
    }

    fn offset_edgeuse(&self, original: Handle) -> Option<Handle> {
        match original.object {
            Object::Topology(Arena::Original, EntityRef::Edgeuse(u)) => self
                .inner
                .offset_edgeuse(u)
                .map(|o| Handle::topology(Arena::Offset, EntityRef::Edgeuse(o))),
            _ => None,
        }
    }

    fn note_unmated_edgeuse(&mut self, original: Handle) {
        if let Object::Topology(Arena::Original, EntityRef::Edgeuse(u)) = original.object {
            self.inner.unmated_edgeuses.insert(u);
        }
    }

    /// The face's recorded margin, as its extension's (the derived reach, at least the
    /// body's tolerance in a local operation; the reach and the offset otherwise): not
    /// upstream's five offset distances, which in a local operation (whose offset distance
    /// is half the body's bounding radius) is 2.5 times that radius. A surface no face
    /// owns has no recorded reach: the body's largest.
    fn analytic_extension_margin(&self, base: Handle, _upstream: f64) -> f64 {
        match base.object {
            Object::FaceSurface(Arena::Original, f) => self.inner.original_extension_margin(f),
            Object::FaceSurface(Arena::Offset, f) => self.inner.offset_extension_margin(f),
            _ => self.inner.body_extension_reach(),
        }
    }

    fn get_create_extended_surface<B: OffsetBrep<Handle>>(
        &mut self,
        k: &Kernel,
        brep: &mut B,
        base: Handle,
    ) -> Result<(Handle, Extent2d)> {
        let Object::FaceSurface(arena, f) = base.object else {
            return Err(SolidError::invalid_input(
                OP,
                "GetCreateExtendedSurface of a surface no face owns",
            ));
        };
        let surface = ClassGeometry::surface(brep, base)
            .ok_or_else(|| {
                SolidError::invalid_input(OP, "GetCreateExtendedSurface of a null surface")
            })?
            .clone();
        let key_of = match arena {
            Arena::Original => self.original_key_of,
            Arena::Offset => self.offset_key_of,
        };
        let key = key_of(EntityRef::Face(f)).key.0;
        let margin = match arena {
            Arena::Original => self.inner.original_extension_margin(f),
            Arena::Offset => self.inner.offset_extension_margin(f),
        };
        let extension = self
            .inner
            .get_create_extended_surface(k, key, &surface, margin)?
            .clone();
        let domain = extension.natural_uv_domain_with_nurb(k)?;
        if let Some(&handle) = self.made.get(&key) {
            return Ok((handle, domain));
        }
        let handle = brep
            .insert_surface(extension)
            .ok_or_else(|| SolidError::invalid_input(OP, "surface store is full"))?;
        self.made.insert(key, handle);
        Ok((handle, domain))
    }
}

/// The caller's notification data for an edge cap's edits.
pub(super) struct CapNotifications<'a> {
    pub(super) records: &'a mut crate::topology::topology::NotifyRecords,
    pub(super) attributes: &'a mut AttributeStore,
    pub(super) context: Option<&'a mut Context>,
    pub(super) original_key_of: &'a dyn Fn(EntityRef) -> ObjectRef,
    pub(super) offset_key_of: &'a dyn Fn(EntityRef) -> ObjectRef,
}

/// `EdgeuseOffset(ctx, this, pEdgeuse, pOtherEdgeuse, pOffsetEU, pOtherOffEU, ..,
/// eMarkType)` over the executive's original and offset B-reps.
#[allow(clippy::too_many_arguments)]
pub(super) fn build(
    kernel: &Kernel,
    creator: &OffsetGeometryCreation,
    executive: &mut OffsetExecutive,
    original: &mut Brep,
    offset: &mut Brep,
    edgeuse: EdgeuseId,
    other: EdgeuseId,
    offset_edgeuse: Option<EdgeuseId>,
    other_offset_edgeuse: Option<EdgeuseId>,
    mark_type: MarkType,
    marks: MarkSource<'_>,
    notifications: CapNotifications<'_>,
) -> Result<EdgeCap> {
    let mut backend = EdgeBrep {
        kernel,
        original,
        offset,
        curves: Vec::new(),
        surfaces: Vec::new(),
        records: notifications.records,
        attributes: notifications.attributes,
        context: notifications.context,
        original_key_of: notifications.original_key_of,
        offset_key_of: notifications.offset_key_of,
        marks,
    };
    let mut exec = Executive {
        inner: executive,
        original_key_of: notifications.original_key_of,
        offset_key_of: notifications.offset_key_of,
        made: BTreeMap::new(),
    };
    let original_use = |u| Handle::topology(Arena::Original, EntityRef::Edgeuse(u));
    let offset_use = |u| Handle::topology(Arena::Offset, EntityRef::Edgeuse(u));
    let made = creator.edgeuse_offset(
        kernel,
        &mut backend,
        &mut exec,
        original_use(edgeuse),
        Some(original_use(other)),
        offset_edgeuse.map(offset_use),
        other_offset_edgeuse.map(offset_use),
        mark_type,
    )?;
    backend.take_output(made)
}

/// Not upstream (WR-346): `MateEdges(ctx, this, pEdgeuse, pAcross, pOffsetEU,
/// pOffAcross, eMarkType)` over the executive's original and offset B-reps, mating the
/// offsets of the faces on either side of a collapsed face.
#[allow(clippy::too_many_arguments)]
pub(super) fn mate(
    kernel: &Kernel,
    creator: &OffsetGeometryCreation,
    executive: &mut OffsetExecutive,
    original: &mut Brep,
    offset: &mut Brep,
    edgeuse: EdgeuseId,
    other: EdgeuseId,
    offset_edgeuse: EdgeuseId,
    other_offset_edgeuse: EdgeuseId,
    mark_type: MarkType,
    marks: MarkSource<'_>,
    notifications: CapNotifications<'_>,
) -> Result<()> {
    let mut backend = EdgeBrep {
        kernel,
        original,
        offset,
        curves: Vec::new(),
        surfaces: Vec::new(),
        records: notifications.records,
        attributes: notifications.attributes,
        context: notifications.context,
        original_key_of: notifications.original_key_of,
        offset_key_of: notifications.offset_key_of,
        marks,
    };
    let mut exec = Executive {
        inner: executive,
        original_key_of: notifications.original_key_of,
        offset_key_of: notifications.offset_key_of,
        made: BTreeMap::new(),
    };
    let original_use = |u| Handle::topology(Arena::Original, EntityRef::Edgeuse(u));
    let offset_use = |u| Handle::topology(Arena::Offset, EntityRef::Edgeuse(u));
    creator.mate_edges(
        kernel,
        &mut backend,
        &mut exec,
        Some(original_use(edgeuse)),
        Some(original_use(other)),
        Some(offset_use(offset_edgeuse)),
        Some(offset_use(other_offset_edgeuse)),
        mark_type,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::attribute::ObjectKey;
    use crate::primitive::api_primitives::create_box;
    use crate::topology::topology::NotifyRecords;

    fn dummy_key(entity: EntityRef) -> ObjectRef {
        let idx = match entity {
            EntityRef::Brep(i) => i.index(),
            EntityRef::Region(i) => i.index(),
            EntityRef::Shell(i) => i.index(),
            EntityRef::Face(i) => i.index(),
            EntityRef::Faceuse(i) => i.index(),
            EntityRef::Loop(i) => i.index(),
            EntityRef::Loopuse(i) => i.index(),
            EntityRef::Edge(i) => i.index(),
            EntityRef::Edgeuse(i) => i.index(),
            EntityRef::Vertex(i) => i.index(),
            EntityRef::Vertexuse(i) => i.index(),
        };
        ObjectRef {
            key: ObjectKey(u64::from(idx) + 1),
            type_code: 0,
        }
    }

    #[test]
    fn shared_vertex_move_refused_when_exceeding_incident_edge_tolerance() -> Result<()> {
        let mut k = Kernel::default();
        let mut original = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0)?;
        let mut offset = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0)?;

        // Pick a corner vertex of `offset` and one of its 3 incident edges (`self_e`).
        // Even if `self_e` has a wide tolerance (`0.05`), moving the shared corner vertex
        // by `0.2` pulls it away from the other two tight-tolerance (`1e-5`) incident
        // edges past `tau_inc + max(tau_v, tau_self, tau_inc) = 1e-5 + 0.05 = 0.05001`,
        // so `verify_vertex_move_on_incident_edges` must refuse the move.
        let v_id = crate::topology::brep::p1::vertices(&offset, None)?[0];
        let v_pt = offset.vertex(v_id)?.point;
        let inc_edges = vertex::edges(&offset, v_id, None)?;
        assert_eq!(inc_edges.len(), 3);
        let self_e = inc_edges[0];

        let mut records = NotifyRecords::new();
        let mut attributes = AttributeStore::default();
        let mut backend = EdgeBrep {
            kernel: &k,
            original: &mut original,
            offset: &mut offset,
            curves: Vec::new(),
            surfaces: Vec::new(),
            records: &mut records,
            attributes: &mut attributes,
            context: None,
            original_key_of: &dummy_key,
            offset_key_of: &dummy_key,
            marks: MarkSource::Records,
        };

        let v_h = Handle::topology(Arena::Offset, EntityRef::Vertex(v_id));
        let self_h = Handle::topology(Arena::Offset, EntityRef::Edge(self_e));

        // Within tolerance (`0.01 <= 1e-5 + 0.05`): accepted.
        OffsetGeometryCreation::verify_vertex_move_on_incident_edges(
            &k,
            &mut backend,
            Some(v_h),
            v_pt + Point3::new(0.01, 0.0, 0.0),
            self_h,
            0.05,
            None,
        )?;

        // Beyond incident edge endpoint bound (`0.2 > 1e-5 + 0.05`): refused with `SM_ERR`.
        let err = OffsetGeometryCreation::verify_vertex_move_on_incident_edges(
            &k,
            &mut backend,
            Some(v_h),
            v_pt + Point3::new(0.2, 0.0, 0.0),
            self_h,
            0.05,
            None,
        )
        .unwrap_err();
        assert_eq!(err.sm_status(), Some(1001));
        Ok(())
    }

    /// Splitting a cylinder rim adds a vertex between the same two supports, not
    /// a third material support. Moving its UV curve by whole periods changes no
    /// 3D point, even when it no longer crosses either source seam chart line.
    /// At zero offset, the ordinary endpoint candidates must keep the half rim.
    #[test]
    fn periodic_chart_miss_without_a_third_support_keeps_ordinary_trim() -> Result<()> {
        use crate::geom::axis2_placement::Axis2Placement;
        use crate::nurbs::b_spline_curve_n_lib::TrimMode;
        use crate::nurbs::b_spline_surface::SurfParamType;
        use crate::primitive::api_primitives::create_cylinder;
        use crate::surface::surface::SurfaceKind;

        let mut k = Kernel::default();
        let mut original = create_cylinder(&mut k, Point3::ZERO, 5.0, 10.0)?;
        let tolerance = original.tolerance();
        let side = original
            .faces()
            .ids()
            .find(|&f| {
                matches!(
                    original.face(f).unwrap().surface.as_ref().unwrap().kind(),
                    SurfaceKind::Cone(_)
                )
            })
            .unwrap();
        let domain = original
            .face(side)?
            .surface
            .as_ref()
            .unwrap()
            .natural_uv_domain_with_nurb(&k)?;
        assert!(original.face(side)?.surface.as_ref().unwrap().is_periodic(
            &k,
            &domain,
            SurfParamType::U
        )?);
        let period = domain.u_interval().length();
        let rim = original
            .edges()
            .ids()
            .find(|&e| edge::is_closed(&original, e).unwrap())
            .unwrap();
        let at = edge::interval(&original, rim)?.evaluate(0.5);
        original.set_editing_enabled(true);
        let (first, _, inserted) =
            crate::heal::stitch::p1::make_vertex_split_edge(&mut k, &mut original, rim, at)?;
        assert_eq!(vertex::edges(&original, inserted.unwrap(), None)?.len(), 2);
        let first = first.unwrap();
        let expected = edge::interval(&original, first)?;
        let base = edge::upward_edgeuse_of_face(&original, first, Some(side))?.unwrap();
        let other = eu::radial(&original, base)?.unwrap();
        assert_ne!(eu::face(&original, base)?, eu::face(&original, other)?);
        let neighbours = [
            eu::cw_edgeuse(&original, base)?.unwrap(),
            eu::ccw_edgeuse(&original, base)?.unwrap(),
        ];
        assert!(neighbours.into_iter().any(|u| {
            let e = eu::edge(&original, u).unwrap().unwrap();
            edge::is_seam_of_surface(&k, &mut original, e, side).unwrap()
        }));

        let mut curve = edge::curve(&original, first)?.unwrap().clone();
        let mut interval = expected;
        curve.trim(&k, &mut interval, TrimMode::Silent)?;
        assert_eq!(interval, expected);
        let mut uv = eu3::create_uv_trim_curve(&k, &mut original, base, false, None)?.curve;
        let ordinary_uv = uv.copy(&k)?;
        uv.transform(
            &k,
            &Axis2Placement::new(Point3::new(2.0 * period, 0.0, 0.0), Vec3::X, Vec3::Y),
            None,
        )?;
        for t in [uv.natural_interval().min(), uv.natural_interval().max()] {
            assert!(uv.evaluate_point(&k, t)?.x > domain.max().x);
        }
        // The native evaluator reads the natural chart. Identify the translated
        // chart with it before evaluating the same lift at both ends and inside.
        for t in [expected.min(), expected.evaluate(0.5), expected.max()] {
            let shifted = uv.evaluate_point(&k, t)?;
            let point = original
                .face(side)?
                .surface
                .as_ref()
                .unwrap()
                .evaluate_point(&k, Point2::new(shifted.x - 2.0 * period, shifted.y))?;
            assert!(point.distance(curve.evaluate_point(&k, t)?) <= tolerance);
        }
        let other_uv = eu3::create_uv_trim_curve(&k, &mut original, other, false, None)?.curve;
        let mut offset = original.clone_standing_for_self()?;
        let marked: BTreeSet<_> = original.edgeuses().ids().collect();
        let mut inner = OffsetExecutive::new(OffsetOperation::SolidOffset, 0.0, tolerance);
        for &u in &marked {
            inner.relate_edgeuses(u, u);
        }
        let mut executive = Executive {
            inner: &mut inner,
            original_key_of: &dummy_key,
            offset_key_of: &dummy_key,
            made: BTreeMap::new(),
        };
        let mut records = NotifyRecords::new();
        let mut attributes = AttributeStore::default();
        let mut backend = EdgeBrep {
            kernel: &k,
            original: &mut original,
            offset: &mut offset,
            curves: Vec::new(),
            surfaces: Vec::new(),
            records: &mut records,
            attributes: &mut attributes,
            context: None,
            original_key_of: &dummy_key,
            offset_key_of: &dummy_key,
            marks: MarkSource::Set(&marked),
        };
        let curve = backend.insert_curve(curve).unwrap();
        let ordinary_uv = backend.insert_curve(ordinary_uv).unwrap();
        let uv = backend.insert_curve(uv).unwrap();
        let other_uv = backend.insert_curve(other_uv).unwrap();
        for uv in [ordinary_uv, uv] {
            let trimmed = OffsetGeometryCreation::new(0.0, tolerance).trim_extended_edge(
                &k,
                &mut backend,
                &mut executive,
                Handle::topology(Arena::Original, EntityRef::Edgeuse(base)),
                Some(Handle::topology(Arena::Original, EntityRef::Edgeuse(other))),
                None,
                None,
                curve,
                Some(uv),
                Some(other_uv),
                MarkType::Mark,
            )?;
            assert_eq!(trimmed, Some(expected));
        }
        assert!(inner.periodic_boundary_ends.is_empty());
        Ok(())
    }

    /// What [`trim_seam_piece`] returns.
    struct SeamTrim {
        /// The "same point" tolerance of the trim (10 x approx tol).
        same_tol: f64,
        /// The trim on the whole circle.
        trimmed: Option<Extent1d>,
        /// The piece's ends carried radially onto the whole circle, in the order of
        /// the piece's own interval.
        want: [Point3; 2],
        /// The whole circle's points at the ends of the trim.
        got: Option<[Point3; 2]>,
    }

    /// Trims, at offset `dist`, the short piece of a cylinder rim (radius 5) split
    /// `chord` from its seam vertex (after the seam when `after`, else before it), onto
    /// the rim's whole closed circle grown by `dist` (its seam where the rim's is), from
    /// the edge use that starts at the seam vertex.
    fn trim_seam_piece(after: bool, chord: f64, dist: f64) -> Result<SeamTrim> {
        use crate::primitive::api_primitives::create_cylinder;
        use crate::surface::surface::SurfaceKind;

        let mut k = Kernel::default();
        let mut original = create_cylinder(&mut k, Point3::ZERO, 5.0, 10.0)?;
        let tolerance = original.tolerance();
        let side = original
            .faces()
            .ids()
            .find(|&f| {
                matches!(
                    original.face(f).unwrap().surface.as_ref().unwrap().kind(),
                    SurfaceKind::Cone(_)
                )
            })
            .unwrap();
        let rim = original
            .edges()
            .ids()
            .find(|&e| edge::is_closed(&original, e).unwrap())
            .unwrap();
        let rim_ivl = edge::interval(&original, rim)?;
        let circle = edge::curve(&original, rim)?.unwrap().clone();
        let seam = circle.evaluate_point(&k, rim_ivl.min())?;
        // the parameter `chord` along the rim (radius 5) from the seam
        let step = 2.0 * (chord / 10.0).asin() / (2.0 * std::f64::consts::PI) * rim_ivl.length();
        let at = if after {
            rim_ivl.min() + step
        } else {
            rim_ivl.max() - step
        };
        original.set_editing_enabled(true);
        let (first, second, _) =
            crate::heal::stitch::p1::make_vertex_split_edge(&mut k, &mut original, rim, at)?;
        let short = [first.unwrap(), second.unwrap()]
            .into_iter()
            .min_by(|&a, &b| {
                let la = edge::interval(&original, a).unwrap().length();
                let lb = edge::interval(&original, b).unwrap().length();
                la.total_cmp(&lb)
            })
            .unwrap();
        let piece = edge::interval(&original, short)?;
        let up = edge::upward_edgeuse_of_face(&original, short, Some(side))?.unwrap();
        let down = eu::radial(&original, up)?.unwrap();
        let mut offset = original.clone_standing_for_self()?;
        let marked: BTreeSet<_> = original.edgeuses().ids().collect();
        let mut inner = OffsetExecutive::new(OffsetOperation::SolidOffset, dist, tolerance);
        for &u in &marked {
            inner.relate_edgeuses(u, u);
        }
        let mut executive = Executive {
            inner: &mut inner,
            original_key_of: &dummy_key,
            offset_key_of: &dummy_key,
            made: BTreeMap::new(),
        };
        // the whole circle grown by `dist` about its centre (the origin: the rim at
        // the cylinder's base), its parameter that of the rim
        let center = Point3::ZERO;
        let rim_mid = circle.evaluate_point(&k, rim_ivl.evaluate(0.5))?;
        assert!((seam + rim_mid).length() < 1e-12, "{seam:?} {rim_mid:?}");
        let s = (5.0 + dist) / 5.0;
        let grown = |p: Point3| center + (p - center) * s;
        let mut whole = circle.copy(&k)?;
        if dist != 0.0 {
            whole.transform(
                &k,
                &crate::geom::axis2_placement::Axis2Placement::IDENTITY,
                Some(Vec3::new(s, s, s)),
            )?;
        }
        assert_eq!(whole.natural_interval(), rim_ivl);
        let want = [
            grown(circle.evaluate_point(&k, piece.min())?),
            grown(circle.evaluate_point(&k, piece.max())?),
        ];
        let whole_copy = whole.copy(&k)?;
        let uv_up = eu3::create_uv_trim_curve(&k, &mut original, up, false, None)?.curve;
        let uv_down = eu3::create_uv_trim_curve(&k, &mut original, down, false, None)?.curve;
        let mut records = NotifyRecords::new();
        let mut attributes = AttributeStore::default();
        let mut backend = EdgeBrep {
            kernel: &k,
            original: &mut original,
            offset: &mut offset,
            curves: Vec::new(),
            surfaces: Vec::new(),
            records: &mut records,
            attributes: &mut attributes,
            context: None,
            original_key_of: &dummy_key,
            offset_key_of: &dummy_key,
            marks: MarkSource::Set(&marked),
        };
        let use_of = |u| Handle::topology(Arena::Original, EntityRef::Edgeuse(u));
        // the edge use that starts at the seam vertex is the base
        let starts_at_seam = |b: &EdgeBrep<'_>, u| -> Result<bool> {
            let v = b.edgeuse_vertex(use_of(u))?.unwrap();
            Ok(b.vertex_point(v)?.distance(seam) <= tolerance)
        };
        let (base, other, uv_base, uv_other) = if starts_at_seam(&backend, up)? {
            (up, down, uv_up, uv_down)
        } else {
            assert!(starts_at_seam(&backend, down)?);
            (down, up, uv_down, uv_up)
        };
        let circle_h = backend.insert_curve(whole).unwrap();
        let uv_base = backend.insert_curve(uv_base).unwrap();
        let uv_other = backend.insert_curve(uv_other).unwrap();
        let creation = OffsetGeometryCreation::new(dist, tolerance);
        let same_tol = 10.0 * creation.this_approx_tol_3d();
        let trimmed = creation.trim_extended_edge(
            &k,
            &mut backend,
            &mut executive,
            use_of(base),
            Some(use_of(other)),
            None,
            None,
            circle_h,
            Some(uv_base),
            Some(uv_other),
            MarkType::Mark,
        )?;
        let got = match trimmed {
            Some(t) => Some([
                whole_copy.evaluate_point(&k, t.min())?,
                whole_copy.evaluate_point(&k, t.max())?,
            ]),
            None => None,
        };
        Ok(SeamTrim {
            same_tol,
            trimmed,
            want,
            got,
        })
    }

    /// Follow-up to #17806 (WR-673): a short open piece of a cylinder rim, on either side
    /// of the rim circle's seam, trims at zero offset onto the whole circle to its own
    /// interval, the short arc between its vertices. Before the fix the piece just before
    /// the seam whose vertices lie within the "same point" tolerance (chord 5e-5) lost its
    /// second trim point: the span from it round to the first point at the natural end
    /// was taken for the first point again, and the trim gave no interval, while its
    /// mirror just after the seam kept `[0, 5e-5]`. With that fixed, the piece with a
    /// chord of 1.5e-5 before the seam (its other arc's middle within approx tol of the
    /// trim points) took the long way round, `[0, 2*pi*5 - 1.5e-5]`. The same holds onto a
    /// circle grown or shrunk by the offset, where the base edge lies off the curve.
    #[test]
    fn a_short_open_edge_beside_a_closed_curves_seam_keeps_its_trim_interval() -> Result<()> {
        for dist in [0.0, 0.5, -0.5] {
            for after in [true, false] {
                for chord in [0.5, 1e-3, 5e-5, 1.5e-5] {
                    let r = trim_seam_piece(after, chord, dist)?;
                    if chord < 1e-3 {
                        // the case the fix is about: the vertices within the tolerance
                        assert!(chord < r.same_tol, "{chord} {}", r.same_tol);
                    }
                    let case = format!("{dist} {after} {chord}: {:?}", r.trimmed);
                    let got = r.got.unwrap_or_else(|| panic!("{case}: no trim"));
                    // the trim runs from the piece's first end to its second, the short
                    // arc, up to the projection's last digits
                    for (g, w) in got.into_iter().zip(r.want) {
                        assert!(g.distance(w) <= 1e-9, "{case}: {g:?} {w:?}");
                    }
                    let t = r.trimmed.unwrap();
                    assert!(t.length() < 1.0, "{case}");
                }
            }
        }
        Ok(())
    }
}
