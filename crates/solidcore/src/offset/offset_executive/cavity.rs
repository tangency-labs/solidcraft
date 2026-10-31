//! Which finite regions of an inset's merged offset are cavity (fork, not upstream).
//!
//! Upstream: none at the pinned commit; ported from the SMLib fork (tangency-labs/smlib@37af643,
//! "offset: an inset keeps the regions that are cavity, not every finite region",
//! `source/SMLib/src/SmOffsetExecutive.cpp`), WR-667.
//!
//! | Rust                        | Fork                  |
//! |-----------------------------|-----------------------|
//! | `is_cavity_region` (crate)  | `sm_IsCavityRegion`   |
//!
//! Deviations: opt-in (`Kernel::fixes.offset_keep_cavity_regions`, read by
//! `do_extended_offset`). The body's `BrepPointSolve` runs on one point-solve cache of the
//! body made by the caller (`p1`'s original-body solve, as `RemoveExcessFaces` uses it);
//! a failed internal point, point solve or normal skips the face as the fork does, but an
//! unported dependency still propagates. No recursion.

use super::p1::{solve_original_brep_point, OriginalKeys};
use crate::error::{ignored, Result, SolidError};
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::geom::vector2d::Point2;
use crate::intersect::point_class::live_owned_surface_global_point_solve;
use crate::kernel::Kernel;
use crate::math::Point3;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::knots::Side;
use crate::topology::brep_cache::BrepCache;
use crate::topology::edgeuse::p1 as eu;
use crate::topology::face::p2 as face;
use crate::topology::{Brep, RegionId};

const OP: &str = "offset::offset_executive::cavity::is_cavity_region";

/// Whether the body's boundary lies within `hit` of `point` (`sOnBody`): the body's
/// nearest point (`BrepPointSolve`, minimize, a single answer, at the body's tolerance).
fn on_body(
    kernel: &Kernel,
    body: &mut Brep,
    cache: &(BrepCache, OriginalKeys),
    point: Point3,
    hit: f64,
) -> Result<bool> {
    let solutions = solve_original_brep_point(kernel, body, &cache.0, &cache.1, point)?;
    Ok(solutions
        .as_slice()
        .first()
        .is_some_and(|s| s.start.solution_value <= hit))
}

/// `sm_IsCavityRegion` (fork): whether finite region `region` of the merged offset
/// `result` is cavity. A face of the cavity is the offset of a face of `body`, `wall`
/// from it, and the body's face lies behind it: from a point inside the face, `wall`
/// back along the normal of the face use that bounds the region (which points into it)
/// reaches the body's boundary. A region the merge closes where two faces' offsets cross
/// has no such face: its faces lie on the body's own faces, or with nothing `wall`
/// behind them, or with the body's face on the region's side.
///
/// `cache` is the body's point-solve cache; the body's editing is disabled for the
/// solves (`SmTemporaryChangeValue`) and restored after.
///
/// # Errors
///
/// Those of the body's point solve (SER), a face's surface point solve and normal (SER),
/// and unported dependencies of the internal point.
pub(super) fn is_cavity_region(
    kernel: &Kernel,
    body: &mut Brep,
    cache: &(BrepCache, OriginalKeys),
    result: &mut Brep,
    region: RegionId,
    wall: f64,
) -> Result<bool> {
    let previous = body.editing_enabled();
    body.set_editing_enabled(false);
    let status = count_behind_ahead(kernel, body, cache, result, region, wall);
    body.set_editing_enabled(previous);
    let (behind, ahead) = status?;
    Ok(behind > 0 && ahead == 0)
}

/// The counts of the region's faces with the body's face `wall` behind them and ahead
/// of them (`lBehind`, `lAhead`).
fn count_behind_ahead(
    kernel: &Kernel,
    body: &mut Brep,
    cache: &(BrepCache, OriginalKeys),
    result: &mut Brep,
    region: RegionId,
    wall: f64,
) -> Result<(usize, usize)> {
    let hit = 10.0 * body.tolerance();
    let mut behind = 0_usize;
    let mut ahead = 0_usize;
    let shells = result.region(region)?.get_shells(None, |_, _| Ok(false))?;
    for shell in shells {
        let faceuses = result.shell(shell)?.get_faceuses()?;
        for fu in faceuses {
            let Some(f) = result.faceuse(fu)?.face else {
                continue;
            };
            // CalculateAnInternalPoint: a failure skips the face
            let Some(point) = ignored(face::calculate_an_internal_point(kernel, result, f))? else {
                continue;
            };
            let (surface, uv_domain) = {
                let face_data = result.face(f)?;
                let Some(surface) = face_data.surface.as_ref() else {
                    // (NER on GetSurface: a face without one is a null pointer)
                    return Err(SolidError::null_pointer(OP));
                };
                (surface.clone_for_evaluation()?, face_data.uv_domain)
            };
            let solutions = live_owned_surface_global_point_solve(
                kernel,
                &surface,
                result,
                f,
                &uv_domain,
                SolverOperation::Minimize,
                point,
                hit,
                SolutionRequested::Single,
            )?;
            let Some(first) = solutions.first() else {
                continue;
            };
            let uv = Point2::new(first.param(0)?, first.param(1)?);
            let normal = surface.evaluate_normal(kernel, uv, Side::Left, Side::Left)?;
            let Ok(mut normal) = normal.unitize() else {
                continue;
            };
            if eu::faceuse_orientation(result, fu)? != OrientType::Same {
                normal *= -1.0;
            }
            if on_body(kernel, body, cache, point, hit)? {
                // an open face's copy, or a face on the body's own
                continue;
            }
            let is_behind = on_body(kernel, body, cache, point - normal * wall, hit)?;
            let is_ahead = on_body(kernel, body, cache, point + normal * wall, hit)?;
            if is_behind {
                behind += 1;
            }
            if is_ahead {
                ahead += 1;
            }
        }
    }
    Ok((behind, ahead))
}
