//! Face-aware surface queries without exposing cache-service plumbing: a face's surface
//! intersected with a tool surface, and the feet of points on a face's untrimmed surface.
//!
//! Upstream: `source/SMLib/src/SmSurface.cpp`, `source/SMLib/src/SmFace.cpp`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`face_surface_intersect`] | `SmSurface::GlobalSurfaceIntersect` on a face's surface |
//! | [`face_surface_feet`] | not upstream: `SmSurface::GlobalPointSolve` (minimize) on a face's surface, untrimmed |
//!
//! Deviations: this convenience entry point binds the arena face and its trimmed
//! cache before delegating to the existing surface solver. No recursion.

use crate::error::query::QueryFailure;
use crate::geom::extent2d::Extent2d;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::nurbs::knots::Side;
use crate::query::brep::BrepFaceServices;
use crate::surface::extend::Continuity;
use crate::surface::implicit::AnalyticSupport;
use crate::surface::side::BoxSide;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::{Surface, SurfaceIntersection};
use crate::topology::brep_cache::CacheObject;
use crate::topology::face;
use crate::{Brep, FaceId, Kernel, Point2, Point3, Result, SolidError};

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
        .ok_or_else(|| {
            SolidError::query(
                "query::face_surface_intersect",
                QueryFailure::FaceWithoutSurface,
            )
        })?
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

/// The foot of a point on a face's untrimmed surface ([`face_surface_feet`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceFoot {
    /// The foot's (u, v) on the surface.
    pub uv: Point2,
    /// The foot.
    pub point: Point3,
    /// The distance from the point to its foot.
    pub distance: f64,
}

/// For each of `points`, its foot on face `id`'s surface untrimmed: the surface's point nearest
/// it (the face's trims play no part), where the segment from the point to it is square to the
/// surface there; `None` for a point beside the surface, whose nearest point is held on a side
/// of its domain without being square to it.
///
/// **The surface is its definition** ([`untrimmed`]): a B-spline over its natural domain; a
/// plane, cylinder, cone, sphere or torus as that analytic surface continued, since the box
/// its stored form spans is bookkeeping (a sheet's plane is made a little larger than its
/// curves' bounding sphere), not where the surface ends.
///
/// The nearest point is the global solve's (`Surface::global_point_solve`, minimize, every
/// solution, the trim test off), the nearest of its solutions, never a local solve from a
/// guess. A closed direction (a periodic seam) and a collapsed side (a pole, an apex) bound no
/// part of the surface: a foot there is square to the surface like any other, so it is kept.
/// A foot held on an open side is not square there (the zero-cosine test of point inversion,
/// in distance rather than angle): the point's offset along the surface, `|(p - foot) x n|`
/// with `n` the unit normal at the foot, is the distance the side cut off. It is within the
/// face's tolerance exactly where the point lies over the surface (a point square over the
/// side itself included), and the foot is kept there.
///
/// The surface and its domain are made once for all the points.
///
/// # Errors
/// A dangling face, a face without a surface; those of the evaluation copy, the continuation
/// ([`untrimmed`]), the natural domain, the global solve and the evaluations at the foot.
pub fn face_surface_feet(
    k: &Kernel,
    brep: &Brep,
    id: FaceId,
    points: &[Point3],
) -> Result<Vec<Option<SurfaceFoot>>> {
    let stored = face::surface(brep, id)?
        .ok_or_else(|| {
            SolidError::query("query::face_surface_feet", QueryFailure::FaceWithoutSurface)
        })?
        .clone_for_evaluation()?;
    let surface = untrimmed(k, stored, points)?;
    let domain = surface.natural_uv_domain_with_nurb(k)?;
    let tolerance = face::tolerance(brep, id)?;
    points
        .iter()
        .map(|&p| surface_foot(k, &surface, &domain, tolerance, p))
        .collect()
}

/// `surface` as far as its definition reaches for `points` ([`face_surface_feet`]): a surface
/// that is not an analytic kind as it is; an analytic one continued Natural past every side of
/// its box by `reach` (`Surface::extended_sides`: exact, the same analytic, its stored part
/// bit for bit), a side that closes, collapses or is unbounded left as it is and one toward an
/// apex, pole or full turn taken up to it.
///
/// `reach` is the largest distance from a point to `c`, the surface's point at the middle of
/// its box, and it is enough: a point's foot on a plane, or its axial or generator coordinate
/// on a cylinder or cone, is its projection there, which moves no further from `c` (or from
/// `c` turned about the axis to the point's side, no further from the point than `c`) than
/// the point is. So no foot is cut off by a side the continuation added.
fn untrimmed(k: &Kernel, surface: Surface, points: &[Point3]) -> Result<Surface> {
    if AnalyticSupport::of(&surface).is_none() {
        return Ok(surface);
    }
    let middle = surface
        .natural_uv_domain_with_nurb(k)?
        .evaluate(MIDDLE, MIDDLE);
    let c = surface.evaluate_point(k, middle)?;
    let reach = points.iter().map(|p| p.distance(c)).fold(0.0, f64::max);
    // every point at the middle: the box already holds each foot
    if reach <= 0.0 {
        return Ok(surface);
    }
    Ok(surface
        .extended_sides(k, &BoxSide::ALL, Continuity::Natural, reach)?
        .surface)
}

/// The middle of a box side's normalised parameter.
const MIDDLE: f64 = 0.5;

/// One point's foot on `surface` over `domain` ([`face_surface_feet`]).
fn surface_foot(
    k: &Kernel,
    surface: &Surface,
    domain: &Extent2d,
    tolerance: f64,
    p: Point3,
) -> Result<Option<SurfaceFoot>> {
    let solutions = surface.global_point_solve_point_test_off::<NoObject>(
        k,
        None,
        domain,
        SolverOperation::Minimize,
        p,
        tolerance,
        None,
        SolutionRequested::All,
    )?;
    let mut nearest: Option<SurfaceFoot> = None;
    for s in solutions.as_slice() {
        let uv = Point2::new(s.start.param(0)?, s.start.param(1)?);
        let point = surface.evaluate_point(k, uv)?;
        let distance = p.distance(point);
        if nearest.is_none_or(|n| distance < n.distance) {
            nearest = Some(SurfaceFoot {
                uv,
                point,
                distance,
            });
        }
    }
    let Some(foot) = nearest else {
        return Ok(None);
    };
    let normal = surface.evaluate_normal(k, foot.uv, Side::Left, Side::Left)?;
    let along = (p - foot.point).cross(normal).length();
    Ok((along <= tolerance).then_some(foot))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::query::QueryFailure;

    /// A face of the caller's body without a surface is the caller's input, named, with
    /// upstream's null-pointer status.
    #[test]
    fn a_face_without_a_surface_is_refused_by_name() {
        let k = Kernel::default();
        let mut b = Brep::new();
        let f = crate::query::face::p2::tests::square(&mut b);
        b.face_mut(f).unwrap().surface = None;
        let e = face_surface_feet(&k, &b, f, &[Point3::new(0.0, 0.0, 0.0)]).unwrap_err();
        assert_eq!(
            e.query_failure(),
            Some(&QueryFailure::FaceWithoutSurface),
            "{e}"
        );
        assert_eq!(e.sm_status(), Some(crate::error::SM_ERR_NULL_POINTER));
        assert_eq!(e.kind(), crate::error::ErrorKind::InvalidInput);
    }
}
