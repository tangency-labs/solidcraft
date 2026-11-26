//! Operation-local face services for shell ray solvers.
//!
//! Upstream: `source/SMLib/src/SmShell.cpp`, `SmFace.cpp`, `SmTrimSrfCache.cpp`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`RayPreparation`] | Lazy `GetOrCreateUVTrimCurve` / `smsurf_GetSurfaceCache` |
//! | `RaySurfaceContext` | Owning-face services during a shell ray solve |
//!
//! Deviations: production shell rays and region-nesting B-rep rays use `live`'s
//! single-pass mutable services. The generic read-only fallback requests missing
//! trims or caches, releases its immutable view and restarts after preparation.
//! Each request is attempted once per edge use or face. That fallback can revisit
//! earlier candidates with raised tolerances and has quadratic worst-case work in
//! the number of requests. Cache handles are mapped explicitly; no B-rep copy is kept.
//! While the body is edited, the tree of a face whose loops close is built with its
//! current trims, or the classification is `SolidError::FaceTrimsUnavailable`
//! (upstream skips the trims then; WR-732); so is one meeting a face whose loops are
//! open mid-edit.

mod live;
pub(crate) use live::{
    solve_mutable_brep_point, solve_mutable_brep_point_kept, solve_mutable_ray, KeptFaceCaches,
};
pub use live::{
    BrepFaceServices, CurveFaceObject, CurveFaceServices, FaceCaches, LiveBrepObjects, SideObject,
    SidedFaceServices,
};

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

use crate::error::query::QueryFailure;
use crate::error::{EntityRef, Result, SolidError};
use crate::kernel::Kernel;
use crate::math::Point2;
use crate::query::face::p2::BrepCacheFace;
use crate::surface::surface::p1::{get_surface_cache_with_face, SurfaceSolveContext};
use crate::surface::surface::Surface;
use crate::surface::surface_cache::face_context::{
    FaceQueries, MappedCacheFace, PreparedFaceQueries,
};
use crate::surface::surface_cache::SurfaceCache;
use crate::topology::brep_cache::CacheObject;
use crate::topology::edgeuse::{p1 as eu1, p3 as eu3};
use crate::topology::loopuse::{entity_of_notify_key, notify_ref};
use crate::topology::{Brep, EdgeuseId, FaceId};

const OP: &str = "query::brep::ray";

/// Geometry requested by a shell ray solve and prepared by its mutable caller.
/// Keeps operation-local caches, not topology. Each edge-use/face request is attempted once.
#[derive(Debug, Default)]
pub struct RayPreparation {
    pending: Cell<Option<PreparationRequest>>,
    /// The B-rep the pending trim request was made on (compared, never dereferenced).
    trim_brep: Cell<usize>,
    attempted: BTreeSet<PreparationRequest>,
    failed_trims: BTreeSet<EdgeuseId>,
    surface_caches: BTreeMap<FaceId, SurfaceCache<CacheObject>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PreparationRequest {
    UvTrim(EdgeuseId),
    SurfaceCache(FaceId),
}

impl RayPreparation {
    pub(crate) fn request_trim(&self, b: &Brep, edgeuse: EdgeuseId) {
        self.trim_brep.set(core::ptr::from_ref(b) as usize);
        self.pending.set(Some(PreparationRequest::UvTrim(edgeuse)));
    }

    /// Whether the last trim request was made on `b` (identity, not equality).
    pub(crate) fn trim_requested_on(&self, b: &Brep) -> bool {
        self.trim_brep.get() == core::ptr::from_ref(b) as usize
    }

    pub(crate) fn trim_failed(&self, edgeuse: EdgeuseId) -> bool {
        self.failed_trims.contains(&edgeuse)
    }

    fn surface_cache(&self, face: FaceId) -> Result<SurfaceCache<CacheObject>> {
        if let Some(cache) = self.surface_caches.get(&face) {
            return cache.map_handles(Ok);
        }
        self.pending
            .set(Some(PreparationRequest::SurfaceCache(face)));
        Err(SolidError::PreparationRequired { operation: OP })
    }

    pub(crate) fn prepared_cache(&self, face: FaceId) -> Option<&SurfaceCache<CacheObject>> {
        self.surface_caches.get(&face)
    }

    pub(crate) fn take_request(&self) -> Option<PreparationRequest> {
        self.pending.take()
    }

    pub(crate) fn prepare(
        &mut self,
        k: &Kernel,
        b: &mut Brep,
        request: PreparationRequest,
    ) -> Result<()> {
        if !self.attempted.insert(request) {
            return Err(SolidError::invalid_input(
                OP,
                "ray preparation made no progress",
            ));
        }
        match request {
            PreparationRequest::UvTrim(eu) => {
                let fetch = eu3::get_or_create_uv_trim_curve(k, b, eu, false, false, true)?;
                if fetch.status.is_err() || eu1::uv_trim_curve(b, eu)?.0.is_none() {
                    self.failed_trims.insert(eu);
                }
            }
            PreparationRequest::SurfaceCache(face) => {
                self.surface_caches
                    .insert(face, prepare_surface_cache(k, b, face)?);
            }
        }
        Ok(())
    }
}

fn cache_handle(entity: EntityRef) -> Result<CacheObject> {
    Ok(match entity {
        EntityRef::Face(f) => CacheObject::Face(f),
        EntityRef::Edge(e) => CacheObject::Edge(e),
        EntityRef::Vertex(v) => CacheObject::Vertex(v),
        other @ (EntityRef::Loopuse(_) | EntityRef::Edgeuse(_) | EntityRef::Vertexuse(_)) => {
            CacheObject::Other(notify_ref(other))
        }
        _ => {
            return Err(SolidError::invalid_input(
                OP,
                "unsupported trim-cache entity",
            ))
        }
    })
}

/// The entity a cache handle names, the inverse of [`cache_handle`]. A use's handle is
/// its notification reference, decoded directly: it reads nothing of the B-rep, so a
/// face's cache costs the same in a body of any size (a map of every use of the
/// B-rep, built per face, made a check of every face quadratic). Only a reference
/// [`notify_ref`] gives a loop, edge or vertex use is a handle. Every handle a cache
/// passes back was made by [`cache_handle`] from a live use of its face during the
/// same call, and the face adapter's lookups reject an entity that is gone.
fn cache_entity(handle: CacheObject) -> Result<EntityRef> {
    match handle {
        CacheObject::Face(f) => Ok(EntityRef::Face(f)),
        CacheObject::Edge(e) => Ok(EntityRef::Edge(e)),
        CacheObject::Vertex(v) => Ok(EntityRef::Vertex(v)),
        CacheObject::Other(r) => entity_of_notify_key(r.key)
            .filter(|&entity| {
                matches!(
                    entity,
                    EntityRef::Loopuse(_) | EntityRef::Edgeuse(_) | EntityRef::Vertexuse(_)
                ) && notify_ref(entity) == r
            })
            .ok_or_else(|| SolidError::invalid_input(OP, "unknown cache topology handle")),
        _ => Err(SolidError::invalid_input(
            OP,
            "unexpected cache geometry handle",
        )),
    }
}

fn prepare_surface_cache(
    k: &Kernel,
    b: &mut Brep,
    face: FaceId,
) -> Result<SurfaceCache<CacheObject>> {
    face_surface_cache(k, b, face, true)
}

/// `smsurf_GetSurfaceCache` of `face`'s surface: the trimmed cache its factory makes
/// (`CacheMakeOrValidate`), with its tree then built when `get_tree` (a solve's
/// `GetTree`, which creates the face's UV trim curves and implants its boundary).
/// Without `get_tree`, a cache the factory leaves without tree (made during a Boolean
/// operation) keeps the state bits the `SmTrimSrfCache` constructor set, as upstream's
/// until its first `GetTree`. (Upstream reuses a cache an earlier call built, with the
/// built tree's bits; the port keeps no record of that.)
///
/// Deviation (WR-732, #2503): the tree of a face whose loops close ([`loops_close`])
/// is built against its current trims even while the B-rep is being edited
/// ([`MappedCacheFace::implanting_while_editing`]).
/// Upstream builds it untrimmed then, and answers right only when it reuses a cache
/// it built before the edit. Such a face whose trims then fail to implant is
/// [`SolidError::FaceTrimsUnavailable`]: the untrimmed surface would give a wrong
/// answer. So is a face whose loops are open mid-edit: it has no boundary to implant
/// (#2624).
fn face_surface_cache(
    k: &Kernel,
    b: &mut Brep,
    face: FaceId,
    get_tree: bool,
) -> Result<SurfaceCache<CacheObject>> {
    let surface = b
        .face(face)?
        .surface
        .as_ref()
        .ok_or_else(|| SolidError::query(OP, QueryFailure::FaceWithoutSurface))?
        .clone_for_evaluation()?;
    // WR-732: while editing, classify against the face's current trims; a face whose
    // loops are open has no boundary to implant, and no answer
    let editing = b.editing_enabled();
    let unavailable = SolidError::FaceTrimsUnavailable {
        operation: OP,
        entity: EntityRef::Face(face),
    };
    if editing && !loops_close(b, face)? {
        return Err(unavailable);
    }
    let mut mapped =
        MappedCacheFace::new(BrepCacheFace { b, f: face }, &cache_handle, &cache_entity)?;
    if editing {
        mapped = mapped.implanting_while_editing();
    }
    // while editing, a failed implant is no trimmed answer (outside editing upstream's
    // fallbacks stay: the plain cache, the status)
    let trims_failed = |e: SolidError| match e {
        e @ (SolidError::Unported { .. }
        | SolidError::Cancelled { .. }
        | SolidError::OutOfFuel { .. }
        | SolidError::PreparationRequired { .. }) => e,
        _ if editing => unavailable.clone(),
        e => e,
    };
    let mut cache =
        get_surface_cache_with_face(k, &surface, Some(CacheObject::Surface(face)), &mut mapped)
            .map_err(trims_failed)?;
    if editing && !cache.base().is_trimmed() {
        // the factory fell back to the plain SmSurfaceCache: no face boundary
        return Err(unavailable);
    }
    // A Boolean operation can defer the factory's tree build until GetTree. A plain
    // SmSurfaceCache (the factory's fallback) runs its own GetTree: no face boundary.
    if get_tree {
        cache
            .get_tree_with_face(k, &mut mapped)
            .map_err(trims_failed)?;
    }
    Ok(cache.into_base())
}

/// Whether every loop use of face `f` closes: each edge use ends where the next one
/// (counter-clockwise) starts. A face edited mid-session can have an open loop, whose
/// trims cannot be implanted. Mid-edit, an edge use can start at a vertex that is
/// being merged into its edge's (a stitch's glue), so two vertices meet when they are
/// one or lie within the sum of their tolerances of each other.
///
/// An edge's ends are read through its edge uses (`SmEdge::GetVertices`): its cached
/// pair ([`crate::topology::Edge::vertices`]) is refreshed only when a public operation
/// returns, and mid-edit can still name a vertex a merge has deleted (#2624).
///
/// A loop that names an entity the edit has deleted is open.
///
/// # Errors
///
/// Those of the topology reads, other than a deleted entity.
fn loops_close(b: &Brep, f: FaceId) -> Result<bool> {
    match loops_close_or_missing(b, f) {
        Err(SolidError::NotFound { .. }) => Ok(false),
        closes => closes,
    }
}

/// [`loops_close`], a deleted entity an error.
fn loops_close_or_missing(b: &Brep, f: FaceId) -> Result<bool> {
    let start = |eu: EdgeuseId| -> Result<Option<crate::topology::VertexId>> {
        match b.edgeuse(eu)?.vertexuse {
            Some(vu) => crate::topology::vertexuse::vertex(b, vu),
            None => Ok(None),
        }
    };
    // an edges ends: where its primary edge use and that uses mate start
    let ends = |edge| -> Result<(Option<_>, Option<_>)> {
        let Some(primary) = eu1::primary_edgeuse(b, edge)? else {
            return Ok((None, None));
        };
        let Some(mate) = eu1::mate(b, primary)? else {
            return Ok((None, None));
        };
        Ok((start(primary)?, start(mate)?))
    };
    let meet = |v: crate::topology::VertexId, w: crate::topology::VertexId| -> Result<bool> {
        if v == w {
            return Ok(true);
        }
        let (v, w) = (b.vertex(v)?, b.vertex(w)?);
        Ok(v.point.distance(w.point) <= v.tolerance + w.tolerance)
    };
    for lu in crate::topology::face::p4::upward_loopuses(b, f, false, None)? {
        for eu in crate::topology::loopuse::edgeuses(b, lu, None, None)? {
            let record = b.edgeuse(eu)?;
            let (Some(edge), Some(next)) = (record.edge, record.ccw) else {
                return Ok(false);
            };
            let (Some(from), Some(to)) = (start(eu)?, start(next)?) else {
                return Ok(false);
            };
            // the edge use ends at its edge's other vertex
            let (Some(v0), Some(v1)) = ends(edge)? else {
                return Ok(false);
            };
            let end = if meet(v0, from)? {
                v1
            } else if meet(v1, from)? {
                v0
            } else {
                return Ok(false);
            };
            if !meet(end, to)? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

pub(crate) struct RaySurfaceContext<'a> {
    pub(crate) brep: &'a Brep,
    pub(crate) preparation: &'a RayPreparation,
}

impl core::fmt::Debug for RaySurfaceContext<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RaySurfaceContext").finish_non_exhaustive()
    }
}

impl RaySurfaceContext<'_> {
    fn classify(
        &self,
        k: &Kernel,
        face: CacheObject,
        uv: Point2,
        src_tol: f64,
        fast_drop: bool,
    ) -> Result<(i32, Option<CacheObject>)> {
        let CacheObject::Face(face) = face else {
            return Err(SolidError::invalid_input(OP, "expected face"));
        };
        let mut pc = crate::intersect::point_class::PointClassification::new(src_tol);
        crate::query::face::p2::point_classify_requested(
            k,
            self.brep,
            face,
            uv,
            &mut pc,
            crate::query::face::p2::PointClassifyOptions {
                src_zone_tol_3d: src_tol,
                do_boundary_intersections: fast_drop,
                use_uv_classification: !fast_drop,
                ..Default::default()
            },
            self.preparation,
        )?;
        Ok((
            pc.point_class().code(),
            pc.object().map(cache_handle).transpose()?,
        ))
    }
}

impl SurfaceSolveContext<CacheObject> for RaySurfaceContext<'_> {
    fn build_surface_cache(
        &mut self,
        _: &Kernel,
        surface: &Surface,
        handle: Option<CacheObject>,
    ) -> Result<SurfaceCache<CacheObject>> {
        let Some(CacheObject::Surface(face)) = handle else {
            return Err(SolidError::invalid_input(OP, "expected face surface"));
        };
        let owner = self
            .brep
            .face(face)?
            .surface
            .as_ref()
            .ok_or_else(|| SolidError::query(OP, QueryFailure::FaceWithoutSurface))?
            .face()?;
        if surface.face()? != owner {
            return Err(SolidError::invalid_input(OP, "surface owner mismatch"));
        }
        self.preparation.surface_cache(face)
    }

    fn face_queries(&self) -> Option<&dyn FaceQueries<CacheObject>> {
        Some(self)
    }
}

impl FaceQueries<CacheObject> for RaySurfaceContext<'_> {
    fn face_zone_tol_3d(&self, face: CacheObject) -> Result<f64> {
        let CacheObject::Face(id) = face else {
            return Err(SolidError::invalid_input(OP, "expected face"));
        };
        PreparedFaceQueries::new(self.brep, id, &cache_handle)?.face_zone_tol_3d(face)
    }

    fn face_has_loopuses(&self, face: CacheObject) -> Result<bool> {
        let CacheObject::Face(id) = face else {
            return Err(SolidError::invalid_input(OP, "expected face"));
        };
        crate::topology::face::p4::has_loopuses(self.brep, id)
    }

    fn face_point_classify(
        &self,
        k: &Kernel,
        face: CacheObject,
        uv: Point2,
        src_tol: f64,
    ) -> Result<(i32, Option<CacheObject>)> {
        self.classify(k, face, uv, src_tol, false)
    }

    fn face_point_classify_fast_drop(
        &self,
        k: &Kernel,
        face: CacheObject,
        uv: Point2,
        src_tol: f64,
    ) -> Result<(i32, Option<CacheObject>)> {
        self.classify(k, face, uv, src_tol, true)
    }

    fn curve_edge_tolerance(&self, curve: CacheObject) -> Option<Result<f64>> {
        let (CacheObject::Curve(edge) | CacheObject::Edge(edge)) = curve else {
            return None;
        };
        Some(crate::topology::edge::tolerance(self.brep, edge))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::curve::Curve;
    use crate::math::Point3;
    use crate::surface::surface::p1::LentSurfaceCache;
    use crate::surface::surface_cache::SurfaceCacheState;

    #[test]
    fn classification_reads_the_prepared_cache_before_vertex_fallback() {
        use crate::geom::tree::{NodeClass, NodeData, NodeId};
        use crate::intersect::point_class::PointClassType;
        let mut k = Kernel::default();
        let mut b = crate::primitive::api_primitives::create_torus(
            &mut k,
            crate::math::Vec3::ZERO,
            5.0,
            1.5,
        )
        .unwrap();
        let face = crate::topology::brep::p1::faces(&b, None)
            .unwrap()
            .into_iter()
            .find(|&f| {
                matches!(
                    b.face(f).unwrap().surface.as_ref().unwrap().kind(),
                    crate::surface::surface::SurfaceKind::Torus(_)
                )
            })
            .unwrap();
        let mut preparation = RayPreparation::default();
        preparation
            .prepare(&k, &mut b, PreparationRequest::SurfaceCache(face))
            .unwrap();
        let cache = preparation.prepared_cache(face).unwrap();
        let (index, _) = cache
            .tree()
            .unwrap()
            .nodes()
            .iter()
            .enumerate()
            .find(|(_, node)| {
                node.child1.is_none()
                    && matches!(node.data,
                NodeData::Surface(aux) if aux.node_class == NodeClass::Inside)
            })
            .unwrap();
        let uv = cache
            .aux(NodeId(index))
            .unwrap()
            .uv_domain
            .evaluate(0.5, 0.5);
        // An oversized source tolerance makes the uncached fallback snap to a
        // vertex. Native cache-first classification returns the interior face.
        let query = RaySurfaceContext {
            brep: &b,
            preparation: &preparation,
        };
        assert_eq!(
            query
                .face_point_classify(&k, CacheObject::Face(face), uv, 100.0)
                .unwrap(),
            (PointClassType::Face.code(), Some(CacheObject::Face(face)))
        );
        preparation
            .surface_caches
            .get_mut(&face)
            .unwrap()
            .face_was_modified = true;
        let query = RaySurfaceContext {
            brep: &b,
            preparation: &preparation,
        };
        assert_eq!(
            query
                .face_point_classify(&k, CacheObject::Face(face), uv, 100.0)
                .unwrap()
                .0,
            PointClassType::Vertex.code()
        );
    }

    /// #20179: whether `GlobalPointSolve`'s fast-drop face test runs depends only on
    /// the face, not on when its cache was built. During a Boolean operation
    /// `smsurf_GetSurfaceCache` defers the tree build (`CacheMakeOrValidate`), so the
    /// cache keeps the `SmTrimSrfCache` constructor's state until a solve's `GetTree`
    /// builds the tree; upstream's build then turns point testing off on a face without
    /// loop uses, so the same face rejected every candidate (a face without loop uses
    /// classifies every point as unknown) before the build and kept them after (the
    /// `blend.fillet_executive_p3` rollover-404 case, a merged face without loop uses).
    /// On a full torus whose face's loops are unlinked from it (`RemoveLoop`: a face
    /// without loop uses) the context reader
    /// ([`SurfaceSolveContext::surface_cache_without_get_tree`] read through
    /// [`SurfaceCache::point_test_with`]) and the live reader
    /// (`live_face_fast_drop_test`) keep the point in both orders: while the tree is
    /// deferred and once it is built.
    #[test]
    fn fast_drop_point_test_on_a_face_without_loops_does_not_depend_on_the_tree_build() {
        let mut k = Kernel::default();
        // a torus whose single face has its loops unlinked
        let loopless = |k: &mut Kernel| {
            let mut b =
                crate::primitive::api_primitives::create_torus(k, Point3::ZERO, 10.0, 2.0).unwrap();
            let faces = crate::topology::brep::p1::faces(&b, None).unwrap();
            assert_eq!(faces.len(), 1);
            let face = faces[0];
            for l in crate::topology::face::p2::loops(&b, face, false, None).unwrap() {
                crate::topology::face::p1::remove_loop(&mut b, face, l).unwrap();
            }
            assert!(crate::topology::face::p2::loops(&b, face, false, None)
                .unwrap()
                .is_empty());
            (b, face)
        };
        let (mut b, face) = loopless(&mut k);
        let surface = b
            .face(face)
            .unwrap()
            .surface
            .as_ref()
            .unwrap()
            .clone_for_evaluation()
            .unwrap();
        k.context.set_doing_boolean(true);
        let handle = Some(CacheObject::Surface(face));
        let on = |c: &SurfaceCacheState<CacheObject>| {
            c.is_trimmed() && c.point_test_enabled && !c.face_was_modified
        };

        // the live reader, on a B-rep made the same way
        let (mut live_b, live_face) = loopless(&mut Kernel::default());
        assert_eq!(live_face, face);
        let mut live_on = None;
        let uv = Point2::new(1.0, 1.0);
        crate::intersect::point_class::live_face_fast_drop_test(
            &k,
            &mut live_b,
            face,
            &mut live_on,
            uv,
            1e-6,
        )
        .unwrap();

        let mut services = BrepFaceServices::new(&mut b);
        let state = services
            .surface_cache_without_get_tree(&k, &surface, handle)
            .unwrap();
        let caches = services.into_caches();
        let kept = caches.get(&face).unwrap();
        assert!(kept.tree().is_none());
        // the cache still reads as trimmed with point testing on, but the face has no
        // loop uses: the point is kept, not classified
        assert!(on(&state));
        let copy = kept.map_handles(Ok).unwrap();
        let services = BrepFaceServices::with_caches(&mut b, caches);
        assert_eq!(
            copy.point_test_with(&k, uv, Some(1e-6), Some(&services))
                .unwrap(),
            (true, None)
        );
        assert_eq!(live_on, Some(false));
        drop(services);

        // outside a Boolean operation, on fresh B-reps (no cache kept from the first half):
        // both readers make the face's cache with its tree, whose build on a face without
        // loops leaves it untrimmed (no trim test); the answer is the same
        let mut k = Kernel::default();
        let (mut b, face) = loopless(&mut k);
        let (mut live_b, _) = loopless(&mut k);
        let surface = b
            .face(face)
            .unwrap()
            .surface
            .as_ref()
            .unwrap()
            .clone_for_evaluation()
            .unwrap();
        let mut live_on = None;
        crate::intersect::point_class::live_face_fast_drop_test(
            &k,
            &mut live_b,
            face,
            &mut live_on,
            uv,
            1e-6,
        )
        .unwrap();
        let mut services = BrepFaceServices::new(&mut b);
        let state = services
            .surface_cache_without_get_tree(&k, &surface, handle)
            .unwrap();
        assert!(services.into_caches().get(&face).unwrap().tree().is_some());
        assert!(!state.is_trimmed());
        assert!(!on(&state));
        assert_eq!(live_on, Some(false));
    }

    /// #20179: a face with loop uses tests the fast drop's points whatever mode its
    /// cache was built in. Upstream's `BuildTree` turns point testing off when the
    /// B-rep is being edited (the standalone branch), and its face branch's restore
    /// brings back the value from before the build, so a cache first built while the
    /// B-rep was edited kept every point, even one far off the face, while the same
    /// face built outside an edit rejected it.
    #[test]
    fn point_test_of_a_face_with_loops_does_not_depend_on_the_build_mode() {
        use crate::query::face::p2::{face_cache_tree, face_surface_cache};
        let answers = |editing: bool| {
            let mut k = Kernel::default();
            let mut b = crate::primitive::api_primitives::create_box(
                &mut k,
                Point3::ZERO,
                10.0,
                10.0,
                10.0,
            )
            .unwrap();
            let face = crate::topology::brep::p1::faces(&b, None).unwrap()[0];
            assert!(crate::topology::face::p4::has_loopuses(&b, face).unwrap());
            let domain = b.face(face).unwrap().uv_domain;
            let inside = Point2::new(
                0.5 * (domain.min.x + domain.max.x),
                0.5 * (domain.min.y + domain.max.y),
            );
            let outside = Point2::new(
                domain.max.x + 10.0 * (domain.max.x - domain.min.x),
                domain.max.y + 10.0 * (domain.max.y - domain.min.y),
            );
            b.set_editing_enabled(editing);
            let mut lease = face_surface_cache(&k, &mut b, face).unwrap();
            face_cache_tree(&k, &mut b, face, &mut lease.cache).unwrap();
            b.set_editing_enabled(false);
            assert!(lease.trimmed);
            let mut test = |uv| {
                lease
                    .cache
                    .point_test_in(&k, &mut BrepCacheFace { b: &mut b, f: face }, uv, None)
                    .unwrap()
                    .0
            };
            let answers = (test(inside), test(outside));
            lease.release();
            answers
        };
        assert_eq!(answers(false), (true, false));
        assert_eq!(answers(true), (true, false));
    }

    /// The state bits a solve reads from a face's kept cache
    /// ([`SurfaceSolveContext::surface_cache_without_get_tree`], read in place by
    /// [`SurfaceCache::mapped_state`]) are those a copy of the cache (`map_handles`)
    /// answers, on a kept cache without its tree (during a Boolean operation) and with
    /// it, and with a stale face binding, where the copy drops the binding: the face
    /// reads as the copy's, not the kept cache's own stale-binding error.
    #[test]
    fn kept_cache_state_reads_as_its_copy() {
        let read_and_copy = |cache: &SurfaceCache<CacheObject>| {
            (
                cache.mapped_state(|c| c).unwrap(),
                cache.map_handles(Ok).unwrap().state().unwrap(),
            )
        };
        let face_answer = |s: &SurfaceCacheState<CacheObject>| format!("{:?}", s.face());
        for doing_boolean in [true, false] {
            let mut k = Kernel::default();
            let mut b =
                crate::primitive::api_primitives::create_cylinder(&mut k, Point3::ZERO, 5.0, 20.0)
                    .unwrap();
            let face = crate::topology::brep::p1::faces(&b, None).unwrap()[0];
            let surface = b
                .face(face)
                .unwrap()
                .surface
                .as_ref()
                .unwrap()
                .clone_for_evaluation()
                .unwrap();
            k.context.set_doing_boolean(doing_boolean);
            let mut services = BrepFaceServices::new(&mut b);
            let state = services
                .surface_cache_without_get_tree(&k, &surface, Some(CacheObject::Surface(face)))
                .unwrap();
            let mut caches = services.into_caches();
            let cache = caches.get_mut(face).unwrap();
            assert_eq!(cache.tree().is_some(), !doing_boolean);
            let (read, copy) = read_and_copy(cache);
            assert_eq!(read, copy);
            assert_eq!(state, read);
            assert_eq!(read, cache.state().unwrap());
            assert!(read.face().unwrap().is_some());

            // the surface no longer owned by the bound face: the copy reads no face
            let owner = cache.surface_mut().set_owner(None).unwrap();
            assert!(cache.state().unwrap().face().is_err());
            let (read, copy) = read_and_copy(cache);
            assert_eq!(read, copy);
            assert_eq!(read.face().unwrap(), None);

            // owned by another face: the copy's answer for that face's handle
            cache
                .surface_mut()
                .set_owner(Some(crate::geom::object::ObjectRef {
                    key: crate::geom::attribute::ObjectKey(owner.key.0.wrapping_add(1)),
                    ..owner
                }));
            let own = cache.state().unwrap();
            let (read, copy) = read_and_copy(cache);
            assert_eq!(read, copy);
            assert_eq!(face_answer(&read), face_answer(&copy));
            assert_ne!(face_answer(&read), face_answer(&own));
        }
    }

    /// A cylinder face's kept cache (made by a first solve's `GetTree`), its surface and
    /// handle, and the B-rep holding it.
    fn kept_cylinder_cache(
        k: &mut Kernel,
    ) -> (Brep, Surface, Option<CacheObject>, FaceId, FaceCaches) {
        let mut b =
            crate::primitive::api_primitives::create_cylinder(k, Point3::ZERO, 5.0, 20.0).unwrap();
        let face = crate::topology::brep::p1::faces(&b, None).unwrap()[0];
        let surface = b
            .face(face)
            .unwrap()
            .surface
            .as_ref()
            .unwrap()
            .clone_for_evaluation()
            .unwrap();
        let handle = Some(CacheObject::Surface(face));
        let mut services = BrepFaceServices::new(&mut b);
        services.build_surface_cache(k, &surface, handle).unwrap();
        let caches = services.into_caches();
        assert!(caches.get(&face).unwrap().tree().is_some());
        (b, surface, handle, face, caches)
    }

    /// The copy a face's services lend a solve that only reads the cache: one shared copy.
    fn lent_copy(
        k: &Kernel,
        services: &mut BrepFaceServices<'_>,
        surface: &Surface,
        handle: Option<CacheObject>,
    ) -> std::sync::Arc<SurfaceCache<CacheObject>> {
        match services.lend_surface_cache(k, surface, handle).unwrap() {
            LentSurfaceCache::Shared(c) => c,
            LentSurfaceCache::Owned(_) => panic!("a kept cache with its tree is lent shared"),
        }
    }

    /// The solves that only read a face's kept cache
    /// ([`SurfaceSolveContext::lend_surface_cache`]) are handed one copy of it, the same
    /// object at every fetch instead of a copy each, also by the next services the kept
    /// caches are handed to: it holds what the copy each fetch made (`map_handles`) holds,
    /// and the kept cache is left as it was.
    #[test]
    fn a_kept_cache_is_lent_as_one_copy() {
        let mut k = Kernel::default();
        let (mut b, surface, handle, face, caches) = kept_cylinder_cache(&mut k);
        let kept = format!("{:?}", caches.get(&face).unwrap());
        let copy = format!("{:?}", caches.get(&face).unwrap().map_handles(Ok).unwrap());
        let mut services = BrepFaceServices::with_caches(&mut b, caches);
        let first = lent_copy(&k, &mut services, &surface, handle);
        let second = lent_copy(&k, &mut services, &surface, handle);
        assert!(std::sync::Arc::ptr_eq(&first, &second));
        assert_eq!(format!("{first:?}"), copy);
        let caches = services.into_caches();
        assert_eq!(format!("{:?}", caches.get(&face).unwrap()), kept);
        // the lent copy is kept with the kept cache for the next services of the B-rep
        let mut services = BrepFaceServices::with_caches(&mut b, caches);
        let next = lent_copy(&k, &mut services, &surface, handle);
        assert!(std::sync::Arc::ptr_eq(&first, &next));
        assert_eq!(format!("{next:?}"), copy);
    }

    /// A solve that changes its cache (here its point-test flag and its surface's owner)
    /// is handed a copy of its own ([`SurfaceSolveContext::build_surface_cache`]): the
    /// kept cache and the copy lent to the readers stay as they were.
    #[test]
    fn a_mutable_consumer_leaves_the_kept_cache() {
        let mut k = Kernel::default();
        let (mut b, surface, handle, face, caches) = kept_cylinder_cache(&mut k);
        let kept = format!("{:?}", caches.get(&face).unwrap());
        let mut services = BrepFaceServices::with_caches(&mut b, caches);
        let lent = lent_copy(&k, &mut services, &surface, handle);
        let lent_before = format!("{lent:?}");
        let mut own = services.build_surface_cache(&k, &surface, handle).unwrap();
        own.point_test_enabled = !own.point_test_enabled;
        own.surface_mut().set_owner(None);
        assert_ne!(format!("{own:?}"), lent_before);
        let again = lent_copy(&k, &mut services, &surface, handle);
        assert!(std::sync::Arc::ptr_eq(&lent, &again));
        assert_eq!(format!("{again:?}"), lent_before);
        let caches = services.into_caches();
        assert_eq!(format!("{:?}", caches.get(&face).unwrap()), kept);
    }

    /// A face whose kept cache holds geometry whose reads keep state a later read sees
    /// ([`SurfaceCache::reads_leave_unchanged`]: an offset surface, or a curve on surface
    /// anywhere in its surface) is not lent shared: each solve reading it gets a copy of
    /// its own, as before.
    #[test]
    fn a_kept_cache_with_read_state_is_not_lent_shared() {
        use crate::curve::crv_on_surf::CrvOnSurf;
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::surface::curve_bounded_surface::CurveBoundedSurface;
        use crate::surface::offset_surface::OffsetSurface;
        type Wrap = fn(&Kernel, &Surface) -> Surface;
        let k = &mut Kernel::default();
        let offset = |k: &Kernel, s: &Surface| {
            Surface::from(
                OffsetSurface::new(k, 1.0, s.clone(), true, crate::geom::extent2d::SS_NONE)
                    .unwrap(),
            )
        };
        let curve_on = |k: &Kernel, s: &Surface| {
            let uv = Curve::from(
                BSplineCurve::create_line_segment(
                    2,
                    Point3::new(0.1, 0.1, 0.0),
                    Point3::new(0.4, 0.3, 0.0),
                )
                .unwrap(),
            );
            let boundary = Curve::from(CrvOnSurf::new(k, uv, s.clone(), None, 0).unwrap());
            Surface::from(
                CurveBoundedSurface::new(s.clone(), vec![boundary], vec![], true).unwrap(),
            )
        };
        let wrappers: [Wrap; 2] = [offset, curve_on];
        for wrap in wrappers {
            let (mut b, surface, handle, face, mut caches) = kept_cylinder_cache(k);
            let cache = caches.get_mut(face).unwrap();
            let wrapped = wrap(k, cache.surface());
            *cache.surface_mut() = wrapped;
            assert!(!caches.get(&face).unwrap().reads_leave_unchanged().unwrap());
            let mut services = BrepFaceServices::with_caches(&mut b, caches);
            for _ in 0..2 {
                assert!(matches!(
                    services.lend_surface_cache(k, &surface, handle).unwrap(),
                    LentSurfaceCache::Owned(_)
                ));
            }
        }
        // the cylinder's own kept cache is lent shared
        let (mut b, surface, handle, _, caches) = kept_cylinder_cache(k);
        assert!(matches!(
            BrepFaceServices::with_caches(&mut b, caches)
                .lend_surface_cache(k, &surface, handle)
                .unwrap(),
            LentSurfaceCache::Shared(_)
        ));
    }

    /// A curve intersection that only reads a B-spline face's cache
    /// ([`Surface::global_curve_intersect_with_context`], no cache given) solves through
    /// the face's lent copy, and the copy still holds what a fresh copy of the kept cache
    /// holds afterwards: the solve's reads (evaluations, the tree solve) leave the lent
    /// copy as it was, so the next reader reads what a copy of its own would hold.
    #[test]
    fn a_read_solve_leaves_the_lent_copy_as_a_fresh_copy() {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::nurbs::b_spline_surface::BSplineSurface;
        let mut k = Kernel::default();
        // the twisted bilinear patch z = x y over the unit square
        let patch = BSplineSurface::create_bilinear_surface(
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(1.0, 1.0, 1.0),
        )
        .unwrap();
        let (mut b, face) =
            crate::topology::brep::sheet::sheet_on(&mut k, Surface::from(patch), 1e-5).unwrap();
        let surface = b
            .face(face)
            .unwrap()
            .surface
            .as_ref()
            .unwrap()
            .clone_for_evaluation()
            .unwrap();
        let handle = Some(CacheObject::Surface(face));
        // a quadratic crossing the patch once, below it at its start and above at its end
        let mut nurb = crate::nurbs::nurbs_crv::allocate_nurb_curve(2, 2, 5).unwrap();
        let points = [(0.3, 0.3, -1.0), (0.5, 0.6, 0.5), (0.7, 0.4, 2.0)];
        for (p, (x, y, z)) in nurb.cpts_mut().iter_mut().zip(points) {
            p.x = x;
            p.y = y;
            p.z = z;
            p.w = -f64::MAX;
        }
        nurb.knots_mut()
            .copy_from_slice(&[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
        let curve = Curve::from(BSplineCurve::from_nurb(nurb, 3, false));

        let mut services = BrepFaceServices::new(&mut b);
        services.build_surface_cache(&k, &surface, handle).unwrap();
        let lent = lent_copy(&k, &mut services, &surface, handle);
        let before = format!("{lent:?}");
        let domain = surface.natural_uv_domain_with_nurb(&k).unwrap();
        let hits = surface
            .global_curve_intersect_with_context(
                &k,
                [None, handle],
                None,
                &mut services,
                &domain,
                &curve,
                &curve.natural_interval(),
                1e-6,
            )
            .unwrap();
        assert_eq!(hits.len(), 1);
        let again = lent_copy(&k, &mut services, &surface, handle);
        assert!(std::sync::Arc::ptr_eq(&lent, &again));
        assert_eq!(format!("{again:?}"), before);
        let caches = services.into_caches();
        let fresh = caches.get(&face).unwrap().map_handles(Ok).unwrap();
        assert_eq!(format!("{again:?}"), format!("{fresh:?}"));
    }

    /// A face's lent copy goes with its kept cache: keeping another cache for the face
    /// drops it, and so does dropping every kept cache, so the next lend makes a new copy
    /// of the cache kept then.
    #[test]
    fn replacing_or_clearing_the_kept_caches_drops_the_lent_copy() {
        let mut k = Kernel::default();
        let (mut b, surface, handle, face, caches) = kept_cylinder_cache(&mut k);
        let mut services = BrepFaceServices::with_caches(&mut b, caches);
        let first = lent_copy(&k, &mut services, &surface, handle);
        let mut caches = services.into_caches();
        let replacement = caches.get(&face).unwrap().map_handles(Ok).unwrap();
        caches.insert(face, replacement);
        let mut services = BrepFaceServices::with_caches(&mut b, caches);
        let second = lent_copy(&k, &mut services, &surface, handle);
        assert!(!std::sync::Arc::ptr_eq(&first, &second));
        assert_eq!(format!("{second:?}"), format!("{first:?}"));
        let mut caches = services.into_caches();
        caches.clear();
        assert!(caches.is_empty());
        let mut services = BrepFaceServices::with_caches(&mut b, caches);
        let third = lent_copy(&k, &mut services, &surface, handle);
        assert!(!std::sync::Arc::ptr_eq(&second, &third));
        assert_eq!(format!("{third:?}"), format!("{first:?}"));
    }

    #[test]
    fn surface_cache_requests_are_scoped_and_bounded() {
        let mut k = Kernel::default();
        let mut b =
            crate::primitive::api_primitives::create_cylinder(&mut k, Point3::ZERO, 5.0, 20.0)
                .unwrap();
        let face = crate::topology::brep::p1::faces(&b, None)
            .unwrap()
            .into_iter()
            .find(|f| {
                matches!(
                    b.face(*f).unwrap().surface.as_ref().unwrap().kind(),
                    crate::surface::surface::SurfaceKind::Cone(_)
                )
            })
            .unwrap();
        let mut preparation = RayPreparation::default();
        assert!(matches!(
            preparation.surface_cache(face),
            Err(SolidError::PreparationRequired { .. })
        ));
        let request = PreparationRequest::SurfaceCache(face);
        assert_eq!(preparation.take_request(), Some(request));
        preparation.prepare(&k, &mut b, request).unwrap();
        let cache = preparation.surface_cache(face).unwrap();
        assert!(cache.tree().is_some());
        assert_eq!(cache.face().unwrap(), Some(CacheObject::Face(face)));
        assert_eq!(preparation.take_request(), None);
        assert!(matches!(
            preparation.prepare(&k, &mut b, request),
            Err(SolidError::InvalidInput { .. })
        ));
    }

    /// A cache handle decodes to its entity without a map of the B-rep's uses: every
    /// face, edge, vertex and use of a body round-trips as the per-face map of every
    /// use read it, and a reference to any other entity kind, or one no entity has,
    /// stays an unknown handle.
    #[test]
    fn cache_handles_decode_as_the_use_map_did() {
        use crate::geom::attribute::ObjectKey;
        use crate::geom::object::ObjectRef;
        let mut k = Kernel::default();
        let b = crate::primitive::api_primitives::create_cylinder(&mut k, Point3::ZERO, 5.0, 20.0)
            .unwrap();
        let uses: BTreeMap<_, _> = b
            .loopuses()
            .iter()
            .map(|(id, _)| EntityRef::Loopuse(id))
            .chain(b.edgeuses().iter().map(|(id, _)| EntityRef::Edgeuse(id)))
            .chain(
                b.vertexuses()
                    .iter()
                    .map(|(id, _)| EntityRef::Vertexuse(id)),
            )
            .map(|entity| (notify_ref(entity), entity))
            .collect();
        assert!(uses.len() > 10);
        for (&r, &entity) in &uses {
            assert_eq!(cache_entity(CacheObject::Other(r)).unwrap(), entity);
            assert_eq!(cache_handle(entity).unwrap(), CacheObject::Other(r));
        }
        for (f, _) in b.faces().iter() {
            let e = EntityRef::Face(f);
            assert_eq!(cache_entity(cache_handle(e).unwrap()).unwrap(), e);
            // a face's notification reference is not a use handle
            assert!(cache_entity(CacheObject::Other(notify_ref(e))).is_err());
        }
        for (e, _) in b.edges().iter() {
            let e = EntityRef::Edge(e);
            assert_eq!(cache_entity(cache_handle(e).unwrap()).unwrap(), e);
        }
        let used = *uses.keys().next().unwrap();
        for bad in [
            ObjectRef {
                type_code: used.type_code + 1,
                key: used.key,
            },
            ObjectRef {
                type_code: used.type_code,
                key: ObjectKey(0),
            },
            ObjectRef {
                type_code: used.type_code,
                key: ObjectKey(1 + (11 << 32)),
            },
        ] {
            assert!(cache_entity(CacheObject::Other(bad)).is_err(), "{bad:?}");
        }
    }
}
