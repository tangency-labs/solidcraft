//! Lazy topology preparation for a shell's read-only ray solver.
//!
//! Upstream: `source/SMLib/src/SmShell.cpp`, `SmFace.cpp`, `SmTrimSrfCache.cpp`.
//!
//! Deviations: a missing trim or owning-face cache is requested during the solve
//! and built after its immutable B-rep view is released. Each request is attempted
//! once. Cache handles are mapped explicitly; no B-rep copy is retained.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

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
use crate::topology::loopuse::notify_ref;
use crate::topology::{Brep, EdgeuseId, FaceId};

const OP: &str = "query::brep::ray";

/// Geometry requested by a shell ray solve and prepared by its mutable caller.
/// Keeps operation-local caches, not topology. Each request is attempted once.
#[derive(Debug, Default)]
pub struct RayPreparation {
    pending: Cell<Option<PreparationRequest>>,
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
    pub(crate) fn request_trim(&self, edgeuse: EdgeuseId) {
        self.pending.set(Some(PreparationRequest::UvTrim(edgeuse)));
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
        Err(SolidError::unported(
            "requested owning-face surface solve cache",
            "surface.trim_srf_cache",
        ))
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
    let surface = b
        .face(face)?
        .surface
        .as_ref()
        .ok_or(SolidError::null_pointer(OP))?
        .clone_for_evaluation()?;
    let entities: Vec<EntityRef> = b
        .loopuses()
        .iter()
        .map(|(id, _)| EntityRef::Loopuse(id))
        .chain(b.edgeuses().iter().map(|(id, _)| EntityRef::Edgeuse(id)))
        .chain(
            b.vertexuses()
                .iter()
                .map(|(id, _)| EntityRef::Vertexuse(id)),
        )
        .collect();
    let entity_of = |handle| match handle {
        CacheObject::Face(f) => Ok(EntityRef::Face(f)),
        CacheObject::Edge(e) => Ok(EntityRef::Edge(e)),
        CacheObject::Vertex(v) => Ok(EntityRef::Vertex(v)),
        CacheObject::Other(key) => entities
            .iter()
            .copied()
            .find(|e| notify_ref(*e) == key)
            .ok_or_else(|| SolidError::invalid_input(OP, "unknown cache topology handle")),
        _ => Err(SolidError::invalid_input(
            OP,
            "unexpected cache geometry handle",
        )),
    };
    let mut mapped = MappedCacheFace::new(BrepCacheFace { b, f: face }, &cache_handle, &entity_of)?;
    let mut cache =
        get_surface_cache_with_face(k, &surface, Some(CacheObject::Surface(face)), &mut mapped)?;
    // A Boolean operation can defer the factory's tree build until GetTree.
    if cache.base().tree().is_none() {
        cache.build_tree_with_face(k, &mut mapped)?;
    }
    Ok(cache.into_base())
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
    fn queries(&self, face: CacheObject) -> Result<PreparedFaceQueries<'_, CacheObject>> {
        let CacheObject::Face(face) = face else {
            return Err(SolidError::invalid_input(OP, "expected face"));
        };
        PreparedFaceQueries::new(self.brep, face, &cache_handle)
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
        self.queries(face)?.face_zone_tol_3d(face)
    }

    fn face_point_classify(
        &self,
        k: &Kernel,
        face: CacheObject,
        uv: Point2,
        src_tol: f64,
    ) -> Result<(i32, Option<CacheObject>)> {
        self.queries(face)?
            .face_point_classify(k, face, uv, src_tol)
    }

    fn face_point_classify_fast_drop(
        &self,
        k: &Kernel,
        face: CacheObject,
        uv: Point2,
        src_tol: f64,
    ) -> Result<(i32, Option<CacheObject>)> {
        self.queries(face)?
            .face_point_classify_fast_drop(k, face, uv, src_tol)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(preparation.surface_cache(face).unwrap_err().is_unported());
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
}
