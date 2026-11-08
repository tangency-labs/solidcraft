//! Face-aware surface intersection without exposing cache-service plumbing.
//!
//! Upstream: `source/SMLib/src/SmSurface.cpp`, `source/SMLib/src/SmFace.cpp`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`face_surface_intersect`] | `SmSurface::GlobalSurfaceIntersect` on a face's surface |
//!
//! Deviations: this convenience entry point binds the arena face and its trimmed
//! cache before delegating to the existing surface solver. No recursion.

use crate::geom::extent2d::Extent2d;
use crate::query::brep::BrepFaceServices;
use crate::surface::surface::{Surface, SurfaceIntersection};
use crate::topology::brep_cache::CacheObject;
use crate::topology::face;
use crate::{Brep, FaceId, Kernel, Result, SolidError};

/// Intersect a face's surface over its UV domain with an independent tool surface
/// over `tool_domain`, using the face's trimmed cache. Both surfaces' boundary
/// curves participate in the solve; contact points and UV curves are retained in
/// the result, as in `GlobalSurfaceIntersect`.
///
/// This is the face-aware surface solve, not a topological face split or an
/// additional clipping/classification pass on its output curves. `tool` must be
/// usable without another face's cache context. Missing UV trims may be created
/// on `brep`; the face caches live for this call. `None` uses the surface solver's
/// default approximation tolerance and angle tolerance.
///
/// # Errors
/// A dangling face, a face without a surface, or errors from the face domain,
/// evaluation copy, cache construction or surface intersection.
pub fn face_surface_intersect(
    k: &Kernel,
    brep: &mut Brep,
    id: FaceId,
    tool: &Surface,
    tool_domain: &Extent2d,
    approx_tol: Option<f64>,
) -> Result<SurfaceIntersection> {
    let surface = face::surface(brep, id)?
        .ok_or_else(|| SolidError::null_pointer("query::face_surface_intersect"))?
        .clone_for_evaluation()?;
    let domain = face::uv_domain(brep, id)?;
    let mut services = BrepFaceServices::new(brep);
    surface.global_surface_intersect_with_context(
        k,
        [Some(CacheObject::Surface(id)), None],
        &mut services,
        &domain,
        tool,
        tool_domain,
        [true, true],
        approx_tol,
        None,
    )
}
