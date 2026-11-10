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
//! With `UpstreamFixes::brep_ray_classifies_current_trims` (WR-732), the tree of a
//! face whose loops close is built with its current trims while the body is edited,
//! or the classification is `SolidError::FaceTrimsUnavailable` (upstream skips the
//! trims then); so is one meeting a face whose loops are open mid-edit.

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

use crate::error::{EntityRef, Result, SolidError};
use crate::geom::object::ObjectRef;
use crate::kernel::Kernel;
use crate::math::Point2;
use crate::query::face::p2::BrepCacheFace;
use crate::surface::surface::p1::{get_surface_cache_with_face, SurfaceSolveContext};
use crate::surface::surface::{CacheTreeState, Surface};
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
/// operation, or reused unbuilt: [`Surface::cache_tree_state`]) keeps the state bits
/// the `SmTrimSrfCache` constructor set, as upstream's until its first `GetTree`; a
/// surface whose cache upstream has built is built here as well.
///
/// Deviation (WR-732, #2503), only with
/// [`crate::kernel::UpstreamFixes::brep_ray_classifies_current_trims`]: the tree of a
/// face whose loops close ([`loops_close`]) is built against its current trims even
/// while the B-rep is being edited ([`MappedCacheFace::implanting_while_editing`]).
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
        .ok_or(SolidError::null_pointer(OP))?
        .clone_for_evaluation()?;
    // upstream reuses a cache whose tree is built, with the built tree's state bits
    let get_tree = get_tree || surface.cache_tree_state() == CacheTreeState::Built;
    // The loop, edge and vertex uses a topology handle can name. A handle names one by its
    // notification key ([`notify_ref`]), which is decoded and looked up in the B-rep when
    // asked. Listing every use of the B-rep into a map for each face made the check of a
    // body with thousands of faces quadratic (#14329). Arena slots are never reused, so a
    // use made while the cache is built (a slot past these counts) is left out, as it was
    // from the list. A use removed meanwhile is no longer named: the list still named it,
    // and reading the removed use then failed instead.
    let slots = (
        b.loopuses().slot_count(),
        b.edgeuses().slot_count(),
        b.vertexuses().slot_count(),
    );
    let entity_of = move |b: &Brep, handle| match handle {
        CacheObject::Face(f) => Ok(EntityRef::Face(f)),
        CacheObject::Edge(e) => Ok(EntityRef::Edge(e)),
        CacheObject::Vertex(v) => Ok(EntityRef::Vertex(v)),
        CacheObject::Other(key) => {
            k.work_counters.add_cache_handle_uses(1);
            topology_use(b, slots, key)
                .ok_or_else(|| SolidError::invalid_input(OP, "unknown cache topology handle"))
        }
        _ => Err(SolidError::invalid_input(
            OP,
            "unexpected cache geometry handle",
        )),
    };
    // WR-732: while editing, classify against the face's current trims; a face whose
    // loops are open has no boundary to implant, and no answer
    let editing = k.fixes.brep_ray_classifies_current_trims && b.editing_enabled();
    let unavailable = SolidError::FaceTrimsUnavailable {
        operation: OP,
        entity: EntityRef::Face(face),
    };
    if editing && !loops_close(b, face)? {
        return Err(unavailable);
    }
    let mut mapped =
        MappedCacheFace::new_reading_brep(BrepCacheFace { b, f: face }, &cache_handle, &entity_of)?;
    if editing {
        mapped = mapped.implanting_while_editing();
    }
    // while editing, a failed implant is no trimmed answer (outside editing upstream's
    // fallbacks stay: the plain cache, the status)
    let trims_failed = |e: SolidError| match e {
        e @ (SolidError::Unported { .. }
        | SolidError::Cancelled { .. }
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

/// The loop, edge or vertex use of `b` whose notification key ([`notify_ref`]) is `key`,
/// if it is one of the first `slots` (loop uses, edge uses, vertex uses) and still there.
fn topology_use(b: &Brep, slots: (usize, usize, usize), key: ObjectRef) -> Option<EntityRef> {
    let entity = entity_of_notify_key(key.key)?;
    let (index, slots, live) = match entity {
        EntityRef::Loopuse(id) => (id.index(), slots.0, b.loopuses().get(id).is_some()),
        EntityRef::Edgeuse(id) => (id.index(), slots.1, b.edgeuses().get(id).is_some()),
        EntityRef::Vertexuse(id) => (id.index(), slots.2, b.vertexuses().get(id).is_some()),
        _ => return None,
    };
    (live && (index as usize) < slots && notify_ref(entity) == key).then_some(entity)
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
            .ok_or(SolidError::null_pointer(OP))?
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

    /// During a Boolean operation `smsurf_GetSurfaceCache` defers the tree build
    /// (`CacheMakeOrValidate`), so `GlobalPointSolve`'s fast-drop face test reads the
    /// `SmTrimSrfCache` constructor's state bits until a solve's `GetTree` builds the
    /// tree (whose build turns point testing off on a face without UV trim curves:
    /// `blend.fillet_executive_p3` rollover-404).
    #[test]
    fn fast_drop_cache_state_defers_the_tree_during_a_boolean() {
        let mut k = Kernel::default();
        let mut b = crate::primitive::api_primitives::create_box(
            &mut k,
            crate::math::Point3::ZERO,
            10.0,
            10.0,
            10.0,
        )
        .unwrap();
        let face = crate::topology::brep::p1::faces(&b, None)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        let surface = b
            .face(face)
            .unwrap()
            .surface
            .as_ref()
            .unwrap()
            .clone_for_evaluation()
            .unwrap();
        surface.set_cache_tree_state(CacheTreeState::NoCache);
        k.context.set_doing_boolean(true);
        let handle = Some(CacheObject::Surface(face));
        let mut services = BrepFaceServices::new(&mut b);

        let state = services.surface_cache_state(&k, &surface, handle).unwrap();
        assert!(state.is_trimmed());
        assert!(state.tree().is_none());
        assert!(state.point_test_enabled);
        assert!(!state.have_t_surface_cache);
        assert_eq!(surface.cache_tree_state(), CacheTreeState::Unbuilt);

        // a solve's GetTree builds the kept cache; later readers see the built tree
        let built = services.build_surface_cache(&k, &surface, handle).unwrap();
        assert!(built.tree().is_some());
        assert_eq!(surface.cache_tree_state(), CacheTreeState::Built);
        let state = services.surface_cache_state(&k, &surface, handle).unwrap();
        assert!(state.tree().is_some());
        assert_eq!(state.point_test_enabled, built.point_test_enabled);
    }

    #[test]
    fn surface_cache_requests_are_scoped_and_bounded() {
        let mut k = Kernel::default();
        let mut b = crate::primitive::api_primitives::create_cylinder(
            &mut k,
            crate::math::Point3::ZERO,
            5.0,
            20.0,
        )
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

    /// An `n`-sided prism: one body with `n + 2` faces.
    fn prism(k: &mut Kernel, n: usize) -> Brep {
        use crate::curve::curve::Curve;
        use crate::primitive::api_curves::create_line_segment;
        let point = |i: usize| {
            let t = core::f64::consts::TAU * i as f64 / n as f64;
            crate::math::Vec3::new(30.0 * t.cos(), 30.0 * t.sin(), 0.0)
        };
        let curves: Vec<Curve> = (0..n)
            .map(|i| Curve::from(create_line_segment(point(i), point(i + 1)).unwrap()))
            .collect();
        let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
        crate::sweep::api_brep::create_linear_sweep(
            k,
            &refs,
            crate::math::Vec3::new(0.0, 0.0, 2.0),
            0.035,
            true,
        )
        .unwrap()
    }

    fn uses(b: &Brep) -> Vec<EntityRef> {
        b.loopuses()
            .ids()
            .map(EntityRef::Loopuse)
            .chain(b.edgeuses().ids().map(EntityRef::Edgeuse))
            .chain(b.vertexuses().ids().map(EntityRef::Vertexuse))
            .collect()
    }

    /// A cache handle names the use the list of every use of the B-rep gave it before
    /// (#14329): the same use for each key of a use, and none for any other key (a removed
    /// use, a slot made after the cache started, another kind, another type code).
    #[test]
    fn a_cache_handle_names_the_use_the_list_of_uses_named() {
        let mut k = Kernel::default();
        let mut b = prism(&mut k, 12);
        let removed = b.edgeuses().ids().nth(3).unwrap();
        b.remove_edgeuse_record(removed).unwrap();
        let listed: BTreeMap<ObjectRef, EntityRef> =
            uses(&b).into_iter().map(|e| (notify_ref(e), e)).collect();
        let slots = (
            b.loopuses().slot_count(),
            b.edgeuses().slot_count(),
            b.vertexuses().slot_count(),
        );
        let mut keys: Vec<ObjectRef> = listed.keys().copied().collect();
        keys.push(notify_ref(EntityRef::Edgeuse(removed)));
        let past = u32::try_from(slots.1).unwrap();
        keys.push(notify_ref(EntityRef::Edgeuse(EdgeuseId::from_index(past))));
        keys.push(notify_ref(EntityRef::Face(b.faces().ids().next().unwrap())));
        let edgeuse = notify_ref(EntityRef::Edgeuse(b.edgeuses().ids().next().unwrap()));
        keys.push(ObjectRef {
            type_code: edgeuse.type_code + 1,
            key: edgeuse.key,
        });
        for key in keys {
            assert_eq!(
                topology_use(&b, slots, key),
                listed.get(&key).copied(),
                "{key:?}"
            );
        }
        // a use made after the cache started is not named
        let last = b.edgeuses().ids().last().unwrap();
        let fewer = (slots.0, last.index() as usize, slots.2);
        assert_eq!(
            topology_use(&b, fewer, notify_ref(EntityRef::Edgeuse(last))),
            None
        );
    }

    /// Checking a body reads, to translate its faces' cache handles, one use per handle:
    /// work that grows with the body. It used to list every use of the B-rep for each face
    /// whose cache it made, uses × faces, so checking an extrusion of an outline of
    /// thousands of lines took minutes (#14329). With 96 sides the body has 98 faces and
    /// 2,500 uses: the check now reads 3,750, where the lists held up to 245,000.
    #[test]
    fn checking_a_body_reads_each_cache_handle_once() {
        use crate::topology::brep::validation::{check_brep, CheckLevel};
        let mut k = Kernel {
            fixes: crate::kernel::UpstreamFixes::all(),
            ..Kernel::default()
        };
        let mut b = prism(&mut k, 96);
        let uses = uses(&b).len() as u64;
        k.work_counters.reset();
        let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
        assert_eq!(check.assert_valid, Some(true));
        let read = k.work_counters.cache_handle_uses();
        assert!(read > 0);
        assert!(read <= 2 * uses, "{read} uses read for a body of {uses}");
    }
}
