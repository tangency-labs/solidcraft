//! `SmSurface`, part 7: the unit normal (with its fall-backs at poles, singular and
//! degenerate points), closure and planarity tests, first-derivative based helpers
//! (approximate derivative lengths, higher-order directional derivatives, the inward
//! vector, dropping points and vectors into the tangent plane), the global point,
//! curve and surface solves and the local curve solve, the degenerate-boundary
//! direction search, flat corners, the parallel projection of a curve, and the
//! file-static helpers of `SmSurface.cpp` owned by this unit.
//!
//! Upstream: `source/SMLib/src/SmSurface.cpp`, `source/SMLib/inc/SmSurface.h`.
//!
//! | Rust                                              | Upstream                                         |
//! |---------------------------------------------------|--------------------------------------------------|
//! | [`Surface::evaluate_normal`], [`base_evaluate_normal`] | `EvaluateNormal` (virtual: the `SmPlane` and `SmCurveBoundedSurface` overrides; the `SmSurface` body) |
//! | `SurfaceEvaluate::evaluate_normal` (trait method in `p2`) | `EvaluateNormal` for code generic over the surface type (default: the `SmSurface` body; [`Surface`] dispatches) |
//! | [`Surface::is_closed`], [`base_is_closed`]        | `IsClosed` (virtual: the `SmPlane` override; the `SmSurface` body) |
//! | [`Surface::is_planar`]                            | `IsPlanar` (virtual: the `SmPlane` override; the `SmSurface` body) |
//! | [`b_spline_is_planar`]                            | `SmSurface::IsPlanar` of an `SmBSplineSurface` (over its `CalculateBoundingBox` override) |
//! | [`Surface::brep`]                                 | `GetBrep`                                        |
//! | [`Surface::flat_corners`], [`FlatCorners`]        | `GetFlatCorners`                                 |
//! | [`approx_derivative_lengths`]                     | `ApproxDerivativeLengths`                        |
//! | [`compute_higher_order_derivs`], [`HigherOrderDerivs`] | `ComputeHigherOrderDerivs`                  |
//! | [`compute_inward_vector`]                         | `ComputeInwardVector`                            |
//! | [`drop_points_to_tangent_plane`]                  | `DropPointsToTangentPlane`                       |
//! | [`drop_vectors_at`]                               | `DropVectors`                                    |
//! | [`Surface::find_degen_param_for_direction`], [`DegenParam`] | `FindDegenParamForDirection`           |
//! | [`Surface::global_point_solve_0`]                 | `GlobalPointSolve_0`                             |
//! | [`Surface::global_curve_solve`]                   | `GlobalCurveSolve`                               |
//! | [`Surface::global_surface_solve`]                 | `GlobalSurfaceSolve`                             |
//! | [`Surface::local_curve_solve`], [`CurveSurfaceSolution`] | `LocalCurveSolve`                         |
//! | [`Surface::create_parallel_projection_curves`]    | `CreateParallelProjectionCurves`                 |
//! | [`SpGlobalSolver`]                                | `SmSPGlobalSolver` (`BranchMayContainAnswers`)   |
//! | [`SsGlobalSolver`]                                | `SmSSGlobalSolver` (`BranchMayContainAnswers`)   |
//! | [`load_guesses`]                                  | `sm_LoadGuesses` (static)                        |
//! | [`pass_sc_convergence_test`]                      | `sm_PassSCConvergenceTest` (static)              |
//! | [`pass_ss_convergence_test`]                      | `sm_PassSSConvergenceTest` (static)              |
//! | [`check_seam_drop`]                               | `sm_CheckSeamDrop` (static)                      |
//! | [`good_drop_result`]                              | `sm_GoodDropResult` (static)                     |
//! | [`evaluate_1st_derivatives`] (co-ported)          | `SmSurface::Evaluate1stDerivatives` (unit `surface.surface_p1`) |
//! | [`evaluate_2nd_derivatives`], [`SecondDerivatives`] (co-ported) | `SmSurface::Evaluate2ndDerivatives` (unit `surface.surface_p1`) |
//! | [`evaluate_directional_derivs`] (co-ported)       | `SmSurface::EvaluateDirectionalDerivs` (unit `surface.surface_p1`) |
//!
//! Deviations:
//! - **Generic bodies.** The `SmSurface` bodies that only call the evaluation virtuals
//!   (`Evaluate`, `EvaluatePoint`, `GetNaturalUVDomain`) are generic over
//!   [`SurfaceEvaluate`], so a caller holding a bare [`BSplineSurface`] (which does not
//!   override them) runs the same code as one holding a [`Surface`].
//! - **Co-ported callees.** `Evaluate1stDerivatives`, `Evaluate2ndDerivatives` and
//!   `EvaluateDirectionalDerivs` belong to unit `surface.surface_p1` but are thin,
//!   non-virtual wrappers around the virtual `Evaluate` that almost every function
//!   here calls first; they are ported here (recorded as `CO-PORTED` in that unit's
//!   plan notes) instead of being stubbed.
//! - **Ignored statuses.** Where upstream ignores an evaluation's status, its
//!   outputs keep their previous values, initially those of the default constructors
//!   ([`Vec3::UNDEF`], [`Vec2::UNDEF`]); the port does the same, and still propagates
//!   [`SolidError::Unported`].
//! - **Out-params** become return values: `IsClosed` returns the continuity only when
//!   asked ([`Surface::is_closed`]'s `want_continuity`), `IsPlanar` returns the whole
//!   [`Planar`] record, `FindDegenParamForDirection` returns the moved `uv` with a
//!   `converged` flag ([`DegenParam`]: upstream moves `rUV` and still returns `SM_ERR`
//!   when the Newton solve fails), `LocalCurveSolve` returns `None` for "no answer".
//!   `sm_LoadGuesses` returns its one guess, and takes the surface (upstream's
//!   `pSurfaceNode->GetOwnerObject()`) instead of the node and the unused patch.
//!   `LocalCurveSolve`'s `cpdOptTargetDistance` (`NotUsed` upstream) and
//!   `CreateParallelProjectionCurves`' context are dropped.
//! - **Debug-only work** compiled out of the release build (drawing, dumps, the
//!   `SM_ASSERT`s, `EvaluateNormal`'s static list of bad surfaces) is not ported, nor
//!   is `FindDegenParamForDirection`'s angle bookkeeping (`dBestRad`,
//!   `dNearBestRad`: computed in release builds but never read).
//! - **Upstream quirks kept:** `GetFlatCorners` masks its return value with `&=` while
//!   filling the corner points; `SmSSGlobalSolver::BranchMayContainAnswers` returns
//!   `SM_SUCCESS` (that is, `FALSE`) for a node outside its face and `NER`'s status
//!   (non-zero: `TRUE`) for a missing patch; `sm_GoodDropResult` compares the guess and
//!   result distances with swapped names; `FindDegenParamForDirection` compares a
//!   sample fraction with the parameter `dS`.
//! - **Unported callees** (return [`SolidError::Unported`]): the surface cache and its
//!   tree (`smsurf_GetSurfaceCache` through `SmSurface::CacheMakeOrValidate`, unit
//!   `surface.surface_p1`; `SmSurfaceCache::GetTree` is
//!   [`crate::surface::surface_cache::SurfaceCache::get_tree`] once a cache handle
//!   resolves) for the global solves and the
//!   parallel projection; the solver constructors, `SmCSNormalizeENFO`,
//!   `SmFindSingularParamWithGivenTangentDirEFO`, `sm_IsNodeValid`, `IsFlatCorner`,
//!   `EvaluateSTEP`, `GetFace` and the base `CalculateBoundingBox`
//!   (`surface.surface_p1`; the solver constructors also hand over the caches holding
//!   the patch data of tree nodes); `SmAdvSurfaceIntersector`
//!   (`intersect.adv_surface_intersector`).

use crate::curve::curve::Curve;
use crate::error::{ignored, Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::geom_utility::p1::directional_derivs;
use crate::geom::geom_utility::p2::{
    evaluate_curve_continuity, frenet_from_derivatives, ContinuityType, CurveEval,
};
use crate::geom::global_solver::{
    Branch, GlobalSolve, GlobalSolver, PreferredSolution, SolutionPairType, SolutionRequested,
    SolverOperation,
};
use crate::geom::pseudo_box::{Planar, PseudoBox};
use crate::geom::solution_array::{Solution, SolutionArray, SolutionEnd, SolutionObject};
use crate::geom::tree::AuxDataType;
use crate::geom::vector2d::Vec2;
use crate::geom::vector3d::sqrt as sm_sqrt;
use crate::math::{max2, Point2, Point3, Vec3, BIG_DOUBLE, EFF_ZERO, EFF_ZERO_SQ, EFF_ZERO_SQRT};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::{
    BSplineSurface, BoundingBoxRequest, LocationType, SurfParamType,
};
use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use crate::nurbs::knots::Side;
use crate::topology::BrepId;

use super::p2::{derivative_limit, drop_vectors, normal_limit, surface_cache, SurfaceEvaluate};
use super::{Surface, SurfaceKind};

const OP_DEGEN: &str = "surface::surface::p7::Surface::find_degen_param_for_direction";
const OP_SEAM: &str = "surface::surface::p7::check_seam_drop";
const OP_SOLVE: &str = "surface::surface::p7::Surface::global_solve";
const OP_LOCAL: &str = "surface::surface::p7::Surface::local_curve_solve";
const OP_PROJ: &str = "surface::surface::p7::Surface::create_parallel_projection_curves";
const OP_MATRIX: &str = "surface::surface::p7::evaluate";

/// `SM_CONTINUITY_ANGLE` (`SM_ANG_TOL_DEG`, degrees).
const CONTINUITY_ANGLE: f64 = 0.5;

/// `IsClosed`'s `NUM_TEST_POINTS`.
const NUM_TEST_POINTS: usize = 5;

// ---- co-ported evaluation wrappers (unit surface.surface_p1) ------------------------------

/// Entry `[i][j]` of an evaluation matrix (`i` times in `u`, `j` times in `v`).
fn at(m: &[Vec<Vec3>], i: usize, j: usize) -> Result<Vec3> {
    m.get(i)
        .and_then(|r| r.get(j))
        .copied()
        .ok_or_else(|| SolidError::invalid_input(OP_MATRIX, "evaluation matrix too small"))
}

const fn flags(u_side: Side, v_side: Side, non_zero_tangents: bool) -> EvaluateFlags {
    EvaluateFlags {
        u_side,
        v_side,
        only_upper_half: true,
        non_zero_tangents,
    }
}

/// The point and first partial derivatives `(P, S_u, S_v)` at `uv`, zero first
/// derivatives replaced by short, properly directed ones
/// (`SmSurface::Evaluate1stDerivatives`, co-ported from unit `surface.surface_p1`).
///
/// # Errors
///
/// Those of the surface's `Evaluate`.
pub fn evaluate_1st_derivatives<S: SurfaceEvaluate + ?Sized>(
    surface: &S,
    uv: Point2,
    u_side: Side,
    v_side: Side,
) -> Result<(Point3, Vec3, Vec3)> {
    let m = surface.evaluate(uv, 1, 1, flags(u_side, v_side, true))?;
    Ok((at(&m, 0, 0)?, at(&m, 1, 0)?, at(&m, 0, 1)?))
}

/// The point and the first and second partial derivatives at a surface point
/// ([`evaluate_2nd_derivatives`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SecondDerivatives {
    /// `rPoint`.
    pub point: Point3,
    /// `rDU`.
    pub du: Vec3,
    /// `rDV`.
    pub dv: Vec3,
    /// `rDUV`.
    pub duv: Vec3,
    /// `rDUU`.
    pub duu: Vec3,
    /// `rDVV`.
    pub dvv: Vec3,
}

/// The point and the first and second partial derivatives at `uv`, zero first
/// derivatives replaced (`SmSurface::Evaluate2ndDerivatives`, co-ported from unit
/// `surface.surface_p1`).
///
/// # Errors
///
/// Those of the surface's `Evaluate`.
pub fn evaluate_2nd_derivatives<S: SurfaceEvaluate + ?Sized>(
    surface: &S,
    uv: Point2,
    u_side: Side,
    v_side: Side,
) -> Result<SecondDerivatives> {
    let m = surface.evaluate(uv, 2, 2, flags(u_side, v_side, true))?;
    Ok(SecondDerivatives {
        point: at(&m, 0, 0)?,
        du: at(&m, 1, 0)?,
        dv: at(&m, 0, 1)?,
        duv: at(&m, 1, 1)?,
        duu: at(&m, 2, 0)?,
        dvv: at(&m, 0, 2)?,
    })
}

/// The position and the directional derivatives up to `highest_deriv` (at most 3) of
/// the surface along `uv_dir` at `uv`, from exact (not zero-corrected) partial
/// derivatives (`SmSurface::EvaluateDirectionalDerivs`, co-ported from unit
/// `surface.surface_p1`).
///
/// # Errors
///
/// `SM_ERR` for `highest_deriv > 3`; those of `Evaluate` and `smgu_DirectionalDerivs`.
pub fn evaluate_directional_derivs<S: SurfaceEvaluate + ?Sized>(
    surface: &S,
    uv: Point2,
    uv_dir: Vec2,
    highest_deriv: usize,
    u_side: Side,
    v_side: Side,
) -> Result<Vec<Vec3>> {
    const OP: &str = "surface::surface::p7::evaluate_directional_derivs";
    // check state
    if highest_deriv > 3 {
        return Err(SolidError::sm_err(OP));
    }

    // make the surface evaluation
    let m = surface.evaluate(
        uv,
        highest_deriv,
        highest_deriv,
        flags(u_side, v_side, false),
    )?;

    // pass the call along (smgu_DirectionalDerivs reads the matrix row-major,
    // `[i * (highest_deriv + 1) + j]`)
    let mut derivs = Vec::new();
    for i in 0..=highest_deriv {
        for j in 0..=highest_deriv {
            derivs.push(
                m.get(i)
                    .and_then(|r| r.get(j))
                    .copied()
                    .unwrap_or(Vec3::UNDEF),
            );
        }
    }
    directional_derivs(uv_dir, highest_deriv, highest_deriv, &derivs)
}

/// `Evaluate1stDerivatives` whose status upstream ignores: the outputs are
/// updated only on success.
fn eval_1st_keep<S: SurfaceEvaluate + ?Sized>(
    surface: &S,
    uv: Point2,
    u_side: Side,
    v_side: Side,
    out: &mut (Point3, Vec3, Vec3),
) -> Result<bool> {
    match ignored(evaluate_1st_derivatives(surface, uv, u_side, v_side))? {
        Some(v) => {
            *out = v;
            Ok(true)
        }
        None => Ok(false),
    }
}

// ---- EvaluateNormal ----------------------------------------------------------------------

/// The unit normal `Unitized(S_u x S_v)` at `uv` (the `SmSurface::EvaluateNormal`
/// body): the plain cross product, else that of unit first derivatives, else (for
/// parallel non-zero derivatives) a two-point extrapolation from step-offs towards the
/// domain's inside, else a cross product with a second derivative in place of a zero
/// first one, else the first good normal at small diagonal step-offs; the zero
/// vector when every trick fails. Upstream always returns `SM_SUCCESS`.
///
/// # Errors
///
/// Only [`SolidError::Unported`] from the surface's evaluation.
pub fn base_evaluate_normal<S: SurfaceEvaluate + ?Sized>(
    surface: &S,
    uv: Point2,
    u_side: Side,
    v_side: Side,
) -> Result<Vec3> {
    // locals
    let mut d1 = (Vec3::UNDEF, Vec3::UNDEF, Vec3::UNDEF);

    // evaluate 1st derivatives at uv
    let first_ok = eval_1st_keep(surface, uv, u_side, v_side, &mut d1)?;
    let (pnt, mut su, mut sv) = d1;

    let deriv_lim = derivative_limit(pnt);
    let norm_lim = normal_limit();

    if first_ok {
        // First just try the simple cross product.
        let mut normal = su.cross(sv);
        let norm_len = normal.length();
        if norm_len > norm_lim {
            normal /= norm_len;
            return Ok(normal);
        }

        // Check for sSu and sSv both being small
        let mut l_u = su.length();
        let mut l_v = sv.length();
        if l_u > deriv_lim {
            su /= l_u;
            l_u = 1.0;
        }
        if l_v > deriv_lim {
            sv /= l_v;
            l_v = 1.0;
        }

        // when Su and Sv were large enough to unitize - check their cross product
        if l_u > deriv_lim && l_v > deriv_lim {
            normal = su.cross(sv);
            let norm_len = normal.length();
            if norm_len > norm_lim {
                normal /= norm_len;
                return Ok(normal);
            }
        }
    }

    let uv_domain = surface.natural_uv_domain();
    let mut step = 1.0e-8;

    // First check for the parallel-derivatives case (the lengths of the
    // zero-corrected, possibly unitized, first derivatives).
    let mut len_su = su.length();
    let mut len_sv = sv.length();

    if len_su >= deriv_lim && len_sv >= deriv_lim {
        // Pick one direction, step until answer, then one more eval farther away and
        // do linear extrapolation back.
        let mut step_uv = Vec2::new(0.0, 0.0);
        let size = uv_domain.size();

        // If it's near an end of the domain, point it away from that end.
        if uv.x < uv_domain.min().x + size.x / 20.0 {
            step_uv.x = size.x;
        } else if uv.x > uv_domain.max().x - size.x / 20.0 {
            step_uv.x = -size.x;
        }

        if uv.y < uv_domain.min().y + size.y / 20.0 {
            step_uv.y = size.y;
        } else if uv.y > uv_domain.max().y - size.y / 20.0 {
            step_uv.y = -size.y;
        }

        // If not close to any domain boundary, just move towards center.
        if step_uv.length_squared() < EFF_ZERO {
            let mid = uv_domain.mid();
            step_uv.x = if uv.x < mid.x { size.x } else { -size.x };
            step_uv.y = if uv.y < mid.y { size.y } else { -size.y };
        }

        // Starting with a small step, try stepoffs until we get a good one.
        while step < 0.1 {
            let suv = uv + step * step_uv;

            // 1st derivatives at step point -- zero-corrected.
            let mut d = (pnt, su, sv);
            if !eval_1st_keep(surface, suv, u_side, v_side, &mut d)? {
                step *= 10.0;
                continue;
            }
            (_, su, sv) = d;

            // Unitize these first derivs: won't change the result.
            len_su = su.length();
            len_sv = sv.length();
            if len_su > deriv_lim {
                su /= len_su;
            }
            if len_sv > deriv_lim {
                sv /= len_sv;
            }

            // If cross product is 'big enough', go with it.
            let mut normal = su.cross(sv);
            let norm_length = normal.length();
            if norm_length > norm_lim {
                normal /= norm_length; // Unitize

                // Step off twice the distance, and do linear extrapolation back.
                let suv2 = uv + 2.0 * step * step_uv;

                let mut d2 = (pnt, su, sv);
                if !eval_1st_keep(surface, suv2, u_side, v_side, &mut d2)? {
                    // If the double-stepoff didn't work, just return the stepoff that did.
                    return Ok(normal);
                }
                let (_, mut su2, mut sv2) = d2;

                // Both evals worked.  Do the two-point extrapolation.
                let norm1 = normal;

                let l_u = su2.length();
                let l_v = sv2.length();
                if l_u > deriv_lim {
                    su2 /= l_u;
                }
                if l_v > deriv_lim {
                    sv2 /= l_v;
                }

                let mut norm2 = su2.cross(sv2);
                let len = norm2.length();
                if len > norm_lim {
                    // They both produced non-zero normals.
                    norm2 /= len;
                    normal = 2.0 * norm1 - norm2; // Linear extrapolation.
                    normal = normal.unitize_or_z();
                }

                return Ok(normal);
            }

            step *= 10.0; // Try again with a bigger stepoff.
        }
    }

    // Use higher-order derivatives to see how the zero-derivative is changing.
    // Don't have it try to correct zero derivatives.
    if let Some(m) = ignored(surface.evaluate(uv, 2, 2, flags(u_side, v_side, false)))? {
        // If we were able to get a good first deriv in the first call, use that.
        if len_su < 0.9 {
            su = at(&m, 1, 0)?;
        }
        if len_sv < 0.9 {
            sv = at(&m, 0, 1)?;
        }
        let mut suu = at(&m, 2, 0)?;
        let mut suv = at(&m, 1, 1)?;
        let mut svv = at(&m, 0, 2)?;

        // If the uv point is at the top of the domain, then its direction just inside
        // the domain would be the opposite of how it is changing.
        let flip_u = if uv.x > uv_domain.u_interval().mid() {
            -1.0
        } else {
            1.0
        };
        let flip_v = if uv.y > uv_domain.v_interval().mid() {
            -1.0
        } else {
            1.0
        };

        // Note, here we're looking for macroscopic results, so use a bigger tolerance.
        let tol2 = norm_lim * 1000.0;

        // 1. Try sSuv for sSu:
        let len_suv = suv.length();
        if len_suv > deriv_lim {
            suv /= len_suv;
        }

        let mut normal = suv.cross(sv);
        let norm_len = normal.length();
        if norm_len > tol2 {
            normal *= flip_v / norm_len;
            return Ok(normal);
        }

        // 2. Try sSuv for sSv:
        normal = su.cross(suv);
        let norm_len = normal.length();
        if norm_len > tol2 {
            normal *= flip_u / norm_len;
            return Ok(normal);
        }

        // 3. Try sSuu for sSu:
        let len_suu = suu.length();
        if len_suu > deriv_lim {
            suu /= len_suu;
        }
        normal = suu.cross(sv);
        let norm_len = normal.length();
        if norm_len > tol2 {
            normal *= flip_u / norm_len;
            return Ok(normal);
        }

        // 4. Try sSvv for sSv:
        let len_svv = svv.length();
        if len_svv > deriv_lim {
            svv /= len_svv;
        }
        normal = su.cross(svv);
        let norm_len = normal.length();
        if norm_len > tol2 {
            normal *= flip_v / norm_len;
            return Ok(normal);
        }

        // 5. Try sSuu for sSu and Suv for Sv:
        normal = suu.cross(suv);
        let norm_len = normal.length();
        if norm_len > tol2 {
            normal *= flip_u * flip_u / norm_len;
            return Ok(normal);
        }

        // 6. Try sSuv for sSu and Svv for Sv:
        normal = suv.cross(svv);
        let norm_len = normal.length();
        if norm_len > tol2 {
            normal *= flip_v * flip_v / norm_len;
            return Ok(normal);
        }

        // 7. Try sSuu for sSu and Svv for Sv:
        normal = suu.cross(svv);
        let norm_len = normal.length();
        if norm_len > tol2 {
            normal *= flip_u * flip_v / norm_len;
            return Ok(normal);
        }
    }

    // Step around a little and see if we can get a good answer: return the first
    // normal found by taking small diagonal uv steps, growing in size from 1.0E-8 to
    // .1 of the domain size, from the target parameter point.
    step = 1.0e-8;
    let u_size = uv_domain.u_interval().length();
    let v_size = uv_domain.v_interval().length();

    // until a large enough step size is taken to get a good normal value
    while step < 0.1 {
        let u_step = step * u_size;
        let v_step = step * v_size;

        // candidate best small step from the input eval point, then all diagonals
        let uvs = [
            Point2::new(
                if u_side == Side::Left {
                    uv.x + u_step
                } else {
                    uv.x - u_step
                },
                if v_side == Side::Left {
                    uv.y + v_step
                } else {
                    uv.y - v_step
                },
            ),
            Point2::new(uv.x + u_step, uv.y + v_step),
            Point2::new(uv.x - u_step, uv.y - v_step),
            Point2::new(uv.x - u_step, uv.y + v_step),
            Point2::new(uv.x + u_step, uv.y - v_step),
        ];

        // for every candidate small step point - get Normal from 1st derivatives
        for &suv in &uvs {
            let mut d = (pnt, su, sv);
            if !eval_1st_keep(surface, suv, u_side, v_side, &mut d)? {
                continue;
            }
            (_, su, sv) = d;

            // Unitize these first derivs: won't change the result.
            len_su = su.length();
            len_sv = sv.length();
            if len_su > deriv_lim {
                su /= len_su;
            }
            if len_sv > deriv_lim {
                sv /= len_sv;
            }

            // check cross product - when it's nonZero return that value
            let normal = su.cross(sv);
            if normal.length() > norm_lim {
                return Ok(normal.unitize_or_z());
            }
        }

        // increase step size and try again.
        step *= 10.0;
    }

    // arrive here for undefined surface normal - set output to zero vector
    Ok(Vec3::new(0.0, 0.0, 0.0))
}

// ---- IsClosed ----------------------------------------------------------------------------

/// The C0 closure (and optionally the continuity across the seam) of a surface over
/// `uv_domain` in direction `param` (the `SmSurface::IsClosed` body): six points along
/// the domain diagonal must match their partners on the opposite boundary within
/// `opt_tol` (default `1000 * SM_EFF_ZERO * (1 + |P|)`), and the iso-curve through each
/// pair must not be a short, large-radius arc. A `param` other than `U` and `Both`
/// tests `V`, as upstream.
///
/// # Errors
///
/// [`SolidError::Unported`] from the surface's evaluation; those of
/// `smgu_EvaluateCurveContinuity` (see the module docs).
pub fn base_is_closed<S: SurfaceEvaluate + ?Sized>(
    surface: &S,
    uv_domain: &Extent2d,
    param: SurfParamType,
    opt_tol: Option<f64>,
    want_continuity: bool,
) -> Result<(bool, Option<ContinuityType>)> {
    let discontinuous = want_continuity.then_some(ContinuityType::Discontinuous);

    // locals
    let count = if param == SurfParamType::Both { 2 } else { 1 };
    let mut min_continuity = ContinuityType::CInfinity;
    let mut p_min = Vec3::UNDEF;
    let mut p_max = Vec3::UNDEF;
    let mut p_mid: Vec<Vec3> = vec![Vec3::UNDEF; 4];
    let mut g_mid: Vec<Vec3> = vec![Vec3::UNDEF; 4];
    let mut uv_min = Vec2::UNDEF;
    let mut uv_max = Vec2::UNDEF;
    let mut uv_mid = Vec2::UNDEF;
    let zero = Vec3::new(0.0, 0.0, 0.0);
    let mut d2_min = SecondDerivatives {
        point: Vec3::UNDEF,
        du: zero,
        dv: zero,
        duv: zero,
        duu: zero,
        dvv: zero,
    };
    let mut d2_max = d2_min;

    // for every param direction to be checked
    for ii in 0..count {
        let this_param = if param == SurfParamType::Both && ii == 0 {
            SurfParamType::U
        } else if param == SurfParamType::Both && ii == 1 {
            SurfParamType::V
        } else {
            param
        };
        let uv_dir = if this_param == SurfParamType::U {
            Vec2::new(1.0, 0.0)
        } else {
            Vec2::new(0.0, 1.0)
        };

        // for every sample point
        for jj in 0..=NUM_TEST_POINTS {
            // get next UVPoint on UVDomain diagonal - avoid endPoints
            let f = (jj as f64 + 1.0) / (2.0 + NUM_TEST_POINTS as f64);
            let uv = uv_domain.evaluate(f, f);

            // set uv_min/uv_max to be requested isoParameters lines endPoints
            if this_param == SurfParamType::U {
                uv_min.x = uv_domain.min().x;
                uv_max.x = uv_domain.max().x;
                uv_mid.x = (uv_min.x + uv_max.x) / 2.0;
                uv_min.y = uv.y;
                uv_max.y = uv.y;
                uv_mid.y = uv.y;
            } else {
                uv_min.y = uv_domain.min().y;
                uv_max.y = uv_domain.max().y;
                uv_mid.y = (uv_min.y + uv_max.y) / 2.0;
                uv_min.x = uv.x;
                uv_max.x = uv.x;
                uv_mid.x = uv.x;
            }

            // evaluate isoParameter endPoints
            if want_continuity {
                if let Some(d) = ignored(evaluate_2nd_derivatives(
                    surface,
                    uv_min,
                    Side::Left,
                    Side::Left,
                ))? {
                    d2_min = d;
                }
                if let Some(d) = ignored(evaluate_2nd_derivatives(
                    surface,
                    uv_max,
                    Side::Left,
                    Side::Left,
                ))? {
                    d2_max = d;
                }
                p_min = d2_min.point;
                p_max = d2_max.point;
            } else {
                if let Some(p) = ignored(surface.evaluate_point(uv_min))? {
                    p_min = p;
                }
                if let Some(p) = ignored(surface.evaluate_point(uv_max))? {
                    p_max = p;
                }
            }

            // pick a tolerance
            let scaled_zero = EFF_ZERO * 1000.0 * (1.0 + p_max.max_dimension());
            let tol = opt_tol.unwrap_or(scaled_zero);

            // when the startPoint/endPoint gap is too big - not closed
            let vec_diff = p_min - p_max;
            if vec_diff.length_squared() > tol * tol {
                return Ok((false, discontinuous));
            }

            // when asked - compute and save the lowest continuity across the boundary
            if want_continuity {
                let (first_min, second_min, first_max, second_max) =
                    if this_param == SurfParamType::U {
                        (d2_min.du, d2_min.duu, d2_max.du, d2_max.duu)
                    } else {
                        (d2_min.dv, d2_min.dvv, d2_max.dv, d2_max.dvv)
                    };
                let curve_continuity = evaluate_curve_continuity(
                    &CurveEval {
                        position: p_min,
                        first: first_min,
                        second: second_min,
                        third: None,
                    },
                    &CurveEval {
                        position: p_max,
                        first: first_max,
                        second: second_max,
                        third: None,
                    },
                    CONTINUITY_ANGLE,
                )?;

                // save lowest found continuity
                if (min_continuity as i32) > (curve_continuity as i32) {
                    min_continuity = curve_continuity;
                }
            }

            // extra test
            if (min_continuity as i32) > (ContinuityType::Discontinuous as i32) {
                // filter out very short curve passing as closed curves
                if let Some(d) = ignored(evaluate_directional_derivs(
                    surface,
                    uv_mid,
                    uv_dir,
                    2,
                    Side::Left,
                    Side::Left,
                ))? {
                    for (dst, src) in p_mid.iter_mut().zip(d) {
                        *dst = src;
                    }
                }

                let p_mid0 = p_mid.first().copied().unwrap_or(Vec3::UNDEF);
                let gap1_sq = (p_mid0 - p_min).length_squared();
                let gap2_sq = (p_mid0 - p_max).length_squared();
                let mid_gap_sq = max2(gap1_sq, gap2_sq);

                // cheesy one point test for a short curve
                if mid_gap_sq < 4.0 * tol * tol {
                    // but some short curves can be very small circles - watch for those
                    // (status ignored; on failure upstream has already written the
                    // position, the tangent and a zero curvature vector)
                    if let Some(g) = ignored(frenet_from_derivatives(&p_mid, 2))? {
                        for (dst, src) in g_mid.iter_mut().zip(g) {
                            *dst = src;
                        }
                    } else {
                        for (dst, src) in g_mid.iter_mut().zip(p_mid.iter().take(2)) {
                            *dst = *src;
                        }
                        if let Some(c) = g_mid.get_mut(2) {
                            *c = Vec3::new(0.0, 0.0, 0.0);
                        }
                    }

                    // get curvature and radius of curvature
                    let mid_curv_sq = g_mid
                        .get(2)
                        .copied()
                        .unwrap_or(Vec3::UNDEF)
                        .length_squared();
                    let mid_radius = if mid_curv_sq > EFF_ZERO_SQ {
                        1.0 / sm_sqrt(mid_curv_sq)
                    } else {
                        BIG_DOUBLE
                    };

                    // small circles can be closed - but short large radius arcs can't be
                    if mid_radius > tol {
                        return Ok((false, discontinuous));
                    }
                }
            }
        }
    }

    // when all test endPoint gaps pass tolerance test - surface is closed
    Ok((true, want_continuity.then_some(min_continuity)))
}

// ---- the virtual dispatchers ---------------------------------------------------------------

/// The corners of a surface's natural domain where the surface is flat
/// ([`Surface::flat_corners`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlatCorners {
    /// `SM_FC_NONE` (0) or a combination of `SM_FC_UMIN_VMIN` (1), `SM_FC_UMIN_VMAX` (2),
    /// `SM_FC_UMAX_VMIN` (4) and `SM_FC_UMAX_VMAX` (8).
    pub flags: u32,
    /// The corner points `[UMinVMin, UMinVMax, UMaxVMin, UMaxVMax]` when asked; corners
    /// not evaluated are [`Vec3::UNDEF`].
    pub points: Option<[Point3; 4]>,
}

/// `SM_FC_UMIN_VMIN`, `SM_FC_UMIN_VMAX`, `SM_FC_UMAX_VMIN`, `SM_FC_UMAX_VMAX`.
const FLAT_CORNER_BITS: [u32; 4] = [1, 2, 4, 8];

/// The result of [`Surface::find_degen_param_for_direction`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DegenParam {
    /// The point on the degenerate boundary, moved along it (`rUV`).
    pub uv: Point2,
    /// The Newton solve converged (`SM_SUCCESS`); upstream returns `SM_ERR` with
    /// `rUV` moved to the best sample otherwise.
    pub converged: bool,
}

/// A curve/surface local solution ([`Surface::local_curve_solve`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveSurfaceSolution {
    /// The distance between the curve and surface points (signed for
    /// `SM_SO_3D_SIGNED_DIRECTED_MINIMIZE`: positive when the curve-to-surface vector
    /// points along the first given vector).
    pub distance: f64,
    /// The curve parameter.
    pub t: f64,
    /// The surface parameters.
    pub uv: Point2,
}

impl Surface {
    /// The unit normal at `uv` (`EvaluateNormal`, virtual): the plane's axis for a
    /// plane, the basis surface's normal for a curve-bounded surface, the
    /// [`base_evaluate_normal`] body otherwise.
    ///
    /// # Errors
    ///
    /// Only [`SolidError::Unported`].
    pub fn evaluate_normal(&self, uv: Point2, u_side: Side, v_side: Side) -> Result<Vec3> {
        match self.kind() {
            SurfaceKind::BSpline(_) => base_evaluate_normal(self, uv, u_side, v_side),
            SurfaceKind::CurveBounded(s) => s.evaluate_normal(uv, u_side, v_side),
            SurfaceKind::Plane(p) => Ok(p.evaluate_normal()),
        }
    }

    /// Whether the surface is closed over `uv_domain` in direction `param`, and the
    /// continuity across the seam when `want_continuity` (`IsClosed`, virtual: a plane
    /// is never closed; the others run [`base_is_closed`]).
    ///
    /// # Errors
    ///
    /// Those of [`base_is_closed`].
    pub fn is_closed(
        &self,
        uv_domain: &Extent2d,
        param: SurfParamType,
        opt_tol: Option<f64>,
        want_continuity: bool,
    ) -> Result<(bool, Option<ContinuityType>)> {
        match self.kind() {
            SurfaceKind::Plane(p) => {
                let (closed, continuity) = p.is_closed();
                Ok((closed, want_continuity.then_some(continuity)))
            }
            SurfaceKind::BSpline(_) | SurfaceKind::CurveBounded(_) => {
                base_is_closed(self, uv_domain, param, opt_tol, want_continuity)
            }
        }
    }

    /// Whether the surface is planar within `tol_3d`, with a plane point, unit normal
    /// and the largest deviation seen (`IsPlanar`, virtual): a plane always is; the
    /// others test the pseudo box of `CalculateBoundingBox` over the natural domain.
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] from the bounding box (the base
    /// `SmSurface::CalculateBoundingBox` of a curve-bounded surface).
    pub fn is_planar(&self, tol_3d: f64) -> Result<Planar> {
        match self.kind() {
            SurfaceKind::Plane(p) => {
                let (point, normal, deviation) = p.is_planar();
                Ok(Planar {
                    is_planar: true,
                    point,
                    normal,
                    deviation,
                })
            }
            SurfaceKind::BSpline(s) => {
                Ok(planar_of(s, &self.natural_uv_domain())?.is_planar(tol_3d))
            }
            SurfaceKind::CurveBounded(_) => {
                // UNPORTED(surface.surface_p1): SmSurface::CalculateBoundingBox
                Err(SolidError::unported(
                    "SmSurface::CalculateBoundingBox",
                    "surface.surface_p1",
                ))
            }
        }
    }

    /// The B-rep of the face this surface belongs to (`GetBrep`): `None` for a surface
    /// no face owns.
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] for an owned surface (`SmSurface::GetFace`).
    pub fn brep(&self) -> Result<Option<BrepId>> {
        if self.owner().is_none() {
            return Ok(None);
        }
        // UNPORTED(surface.surface_p1): SmSurface::GetFace (then SmFace::GetBrep)
        Err(SolidError::unported(
            "SmSurface::GetFace",
            "surface.surface_p1",
        ))
    }

    /// The flat corners of the natural domain within `angle_tol_deg` and, when
    /// `want_points`, their positions (`GetFlatCorners`; upstream's default angle is
    /// `SM_EFF_ZERO_DEG`).
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`]: `SmSurface::IsFlatCorner` (`surface.surface_p1`).
    pub fn flat_corners(&self, angle_tol_deg: f64, want_points: bool) -> Result<FlatCorners> {
        // init output
        let mut points = want_points.then_some([Vec3::UNDEF; 4]);

        // locals
        let d = self.natural_uv_domain();
        let corners = [
            Point2::new(d.u_min(), d.v_min()),
            Point2::new(d.u_min(), d.v_max()),
            Point2::new(d.u_max(), d.v_min()),
            Point2::new(d.u_max(), d.v_max()),
        ];

        // check each corner
        let mut rtn = 0u32;
        for (c, bit) in corners.iter().zip(FLAT_CORNER_BITS) {
            if is_flat_corner(self, *c, angle_tol_deg)? {
                rtn |= bit;
            }
        }

        // optional output (upstream masks the return value while testing)
        if let Some(pts) = points.as_mut() {
            for ((c, bit), p) in corners.iter().zip(FLAT_CORNER_BITS).zip(pts.iter_mut()) {
                rtn &= bit;
                if rtn != 0 {
                    if let Some(v) = ignored(self.evaluate_point(*c))? {
                        *p = v;
                    }
                }
            }
        }

        Ok(FlatCorners { flags: rtn, points })
    }

    /// Slides `uv` along a degenerate boundary (singular in `singular_dir`) to where
    /// the non-singular tangent best matches `dir` (`FindDegenParamForDirection`):
    /// samples ten parameters, then runs a Newton solve from the best one.
    ///
    /// # Errors
    ///
    /// The status of `Unitize` for a zero `dir`; `SM_ERR` when every sampled tangent
    /// points away from `dir`; those of `Evaluate`; [`SolidError::Unported`] for the
    /// Newton solve (`SmFindSingularParamWithGivenTangentDirEFO`,
    /// `surface.surface_p1`).
    pub fn find_degen_param_for_direction(
        &self,
        uv: Point2,
        dir: Vec3,
        dir_from_pole: bool,
        singular_dir: SurfParamType,
        tol: f64,
    ) -> Result<DegenParam> {
        // copy and unitize direction to match
        let mut s_dir = dir.unitize()?;

        // locals
        let domain = self.natural_uv_domain();
        let uv_min = domain.min();
        let uv_max = domain.max();
        let mut s_ivl = Extent1d::new();
        let mut t_ivl = Extent1d::new();
        let singular_u = singular_dir == SurfParamType::U;
        let (ds, dt) = if singular_u {
            ignored(s_ivl.set_min_max(uv_min.x, uv_max.x))?;
            ignored(t_ivl.set_min_max(uv_min.y, uv_max.y))?;
            (uv.x, uv.y)
        } else {
            ignored(s_ivl.set_min_max(uv_min.y, uv_max.y))?;
            ignored(t_ivl.set_min_max(uv_min.x, uv_max.x))?;
            (uv.y, uv.x)
        };

        // domain tolerances
        let f_tol = 2.0 / 50.0;

        // Check whether we're looking for parallel or anti-parallel derivative.
        if (dt > t_ivl.mid() && dir_from_pole) || (dt <= t_ivl.mid() && !dir_from_pole) {
            s_dir = -s_dir;
        }

        // iteration locals
        let num_guesses = 10usize;
        let mut test_uv = uv;
        let mut near_best_f = -1.0;
        let mut near_best_param = 0.5;
        let mut best_f = -1.0;
        let mut best_param = 0.5;

        // Don't sample right to the boundaries of the domain, pull the sampling
        // domain in a bit.
        let mut sample_domain = s_ivl;
        sample_domain.expand_relative(-0.025);

        // for every sample point - find biggest ( sSurf_dt(si,v) dot Dir) value
        for ii in 0..num_guesses {
            let this_param = ii as f64 / (num_guesses - 1) as f64;

            // vary the singular param
            if singular_u {
                test_uv.x = sample_domain.evaluate(this_param);
            } else {
                test_uv.y = sample_domain.evaluate(this_param);
            }

            // Evaluate target pos and tangents
            let m = self.evaluate(test_uv, 1, 1, flags(Side::Left, Side::Left, true))?;

            // save the nonSingular Unitized tangent
            let surf_dt = if singular_u {
                at(&m, 0, 1)?
            } else {
                at(&m, 1, 0)?
            }
            .unitize_or_z();

            // measure how closely test point dir is to target dir
            let this_f = surf_dt.dot(s_dir);

            // save the best sample point
            if this_f > best_f {
                best_f = this_f;
                best_param = this_param;
            }

            // for nearly equal starting guesses - save the one nearest the input value
            if this_f > near_best_f - f_tol
                && (this_f > near_best_f + f_tol
                    || (this_param - ds).abs() < (near_best_param - ds).abs())
            {
                near_best_f = this_f;
                near_best_param = this_param;
            }
        }

        // No solution: all sample tangents point away from the given dir.
        if near_best_f < 0.0 && best_f < 0.0 {
            return Err(SolidError::sm_err(OP_DEGEN));
        }

        // solve with NewtonRaphson iteration starting at the near best param, then
        // the best param
        let first = singular_param_solve(
            self,
            uv,
            s_dir,
            singular_dir,
            &s_ivl,
            s_ivl.evaluate(near_best_param),
            tol,
        );
        let (stat_ok, mut found, mut found_t) = match ignored(first)? {
            Some((f, t)) => (true, f, t),
            None => (false, false, 0.0),
        };
        let mut found_param = near_best_param;
        if !found {
            if let Some((f, t)) = ignored(singular_param_solve(
                self,
                uv,
                s_dir,
                singular_dir,
                &s_ivl,
                s_ivl.evaluate(best_param),
                tol,
            ))? {
                found = f;
                found_t = t;
            }
            found_param = best_param;
        }

        // Leave our best value in the return arg
        let value = if found {
            found_t
        } else {
            s_ivl.evaluate(found_param)
        };
        let mut out = uv;
        if singular_u {
            out.x = value;
        } else {
            out.y = value;
        }
        Ok(DegenParam {
            uv: out,
            converged: stat_ok && found,
        })
    }

    /// The extrema of the distance from `test_point` to the surface over `uv_domain`
    /// (`GlobalPointSolve_0`, the original global point solve over the surface
    /// cache's decomposition tree).
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`]: the surface cache (`smsurf_GetSurfaceCache`).
    pub fn global_point_solve_0<O: SolutionObject>(
        &self,
        uv_domain: &Extent2d,
        op: SolverOperation,
        test_point: Point3,
        distance_tolerance: f64,
        opt_target_distance: Option<f64>,
        requested: SolutionRequested,
    ) -> Result<SolutionArray<O>> {
        // get surface spatial decomposition tree from Surface's cache
        let cache = surface_cache(self)?;
        surface_tree(&cache)?;
        // UNPORTED(surface.surface_p1): SmSPGlobalSolver::SmSPGlobalSolver (then SolveIt)
        let _ = (uv_domain, test_point, distance_tolerance, requested);
        let _best = point_solve_best_answer_sq(op, opt_target_distance)?;
        Err(SolidError::unported(
            "SmSPGlobalSolver::SmSPGlobalSolver",
            "surface.surface_p1",
        ))
    }

    /// The global curve/surface solve over `uv_domain` and the curve's `interval`
    /// (`GlobalCurveSolve`: minimize, maximize, signed directed minimize, normalize).
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`]: the surface cache (`smsurf_GetSurfaceCache`).
    pub fn global_curve_solve<O: SolutionObject>(
        &self,
        uv_domain: &Extent2d,
        curve: &Curve,
        interval: &Extent1d,
        op: SolverOperation,
        distance_tolerance: f64,
        opt_target_distance: Option<f64>,
        opt_vectors: &[Vec3],
        requested: SolutionRequested,
    ) -> Result<SolutionArray<O>> {
        // get target cache locals
        let cache = surface_cache(self)?;
        surface_tree(&cache)?;
        // then the curve's cache and tree (SmCacheMgr::GetOrCreateObjectCache(SM_OC_CURVE),
        // SmCurveCache::GetTree: curve::curve::p5::make_curve_cache, CurveCache::tree)
        // UNPORTED(surface.surface_p1): SmCSGlobalSolver::SmCSGlobalSolver (then SolveIt)
        let _ = (
            uv_domain,
            curve,
            interval,
            distance_tolerance,
            opt_vectors,
            requested,
        );
        let _best = pair_solve_best_answer_sq(op, opt_target_distance, false)?;
        Err(SolidError::unported(
            "SmCSGlobalSolver::SmCSGlobalSolver",
            "surface.surface_p1",
        ))
    }

    /// The global surface/surface solve (`GlobalSurfaceSolve`: minimize, maximize,
    /// normalize, signed directed minimize/maximize, and intersect for nodes).
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`]: the surface cache (`smsurf_GetSurfaceCache`).
    pub fn global_surface_solve<O: SolutionObject>(
        &self,
        uv_domain: &Extent2d,
        other: &Surface,
        other_uv_domain: &Extent2d,
        op: SolverOperation,
        distance_tolerance: f64,
        opt_target_distance: Option<f64>,
        opt_vectors: &[Vec3],
        requested: SolutionRequested,
    ) -> Result<SolutionArray<O>> {
        let cache = surface_cache(self)?;
        let cache2 = surface_cache(other)?;
        surface_tree(&cache)?;
        surface_tree(&cache2)?;
        // UNPORTED(surface.surface_p1): SmSSGlobalSolver::SmSSGlobalSolver (then SolveIt)
        let _ = (
            uv_domain,
            other_uv_domain,
            distance_tolerance,
            opt_vectors,
            requested,
        );
        let _best = pair_solve_best_answer_sq(op, opt_target_distance, true)?;
        Err(SolidError::unported(
            "SmSSGlobalSolver::SmSSGlobalSolver",
            "surface.surface_p1",
        ))
    }

    /// The local curve/surface solve from the guesses (`LocalCurveSolve`: minimize,
    /// maximize, normalize, or signed directed minimize along `opt_vectors[0]`);
    /// `None` when no answer was found.
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`]: the evaluator `SmCSNormalizeENFO`
    /// (`surface.surface_p1`, `surface.surface_p4`); those of the evaluations;
    /// [`SolidError::InvalidInput`] for a signed directed solve without a vector
    /// (upstream dereferences `NULL`).
    pub fn local_curve_solve(
        &self,
        uv_domain: &Extent2d,
        curve: &Curve,
        interval: &Extent1d,
        op: SolverOperation,
        distance_tolerance: f64,
        opt_vectors: &[Vec3],
        uv_guess: Point2,
        t_guess: f64,
    ) -> Result<Option<CurveSurfaceSolution>> {
        let signed = op == SolverOperation::Signed3dDirectedMinimize;

        // the problem intervals [t, u, v (, distance)] and guess
        let mut intervals = vec![
            *interval,
            Extent1d::from_min_max(uv_domain.min().x, uv_domain.max().x),
            Extent1d::from_min_max(uv_domain.min().y, uv_domain.max().y),
        ];
        let mut guess = vec![t_guess, uv_guess.x, uv_guess.y];
        if signed {
            intervals.push(Extent1d::from_min_max(-BIG_DOUBLE, BIG_DOUBLE));
            guess.push(0.0);
        }

        let Some(solution) = cs_normalize_local_solve(
            self,
            curve,
            op,
            distance_tolerance,
            opt_vectors,
            &intervals,
            &guess,
        )?
        else {
            return Ok(None);
        };

        let s0 = solution.first().copied().unwrap_or(0.0);
        let s1 = solution.get(1).copied().unwrap_or(0.0);
        let s2 = solution.get(2).copied().unwrap_or(0.0);
        let p1 = curve.evaluate_point(s0)?;
        let uv = Point2::new(s1, s2);
        let p2 = self.evaluate_point(uv)?;
        let mut dist = p1.distance(p2);
        if signed {
            // Vector going from curve to surface
            let vec = p2 - p1;
            let along = opt_vectors
                .first()
                .ok_or_else(|| SolidError::invalid_input(OP_LOCAL, "no direction vector"))?;
            if vec.dot(*along) < 0.0 {
                dist = -dist;
            }
        }
        Ok(Some(CurveSurfaceSolution {
            distance: dist,
            t: s0,
            uv,
        }))
    }

    /// The curves of the parallel projection of `curve` along `projection_vector`
    /// onto the surface over `uv_domain` (`CreateParallelProjectionCurves`): the
    /// intersection with the curve's linear sweep.
    ///
    /// # Errors
    ///
    /// `SM_ERR_INVALID_INPUT` for a zero projection vector; [`SolidError::Unported`]:
    /// the surface cache (`smsurf_GetSurfaceCache`).
    pub fn create_parallel_projection_curves(
        &self,
        uv_domain: &Extent2d,
        curve: &BSplineCurve,
        projection_vector: Vec3,
        opt_projection_distance: Option<f64>,
        opt_approx_tol_3d: Option<f64>,
        opt_angle_tol_rad: Option<f64>,
    ) -> Result<(Vec<Curve>, Vec<Curve>)> {
        // check input (ZERO_VEC_ER: fails unless |v|^2 >= SM_EFF_ZERO_SQ, so also for NaN)
        if projection_vector
            .length_squared()
            .partial_cmp(&EFF_ZERO_SQ)
            .is_none_or(core::cmp::Ordering::is_lt)
        {
            return Err(SolidError::invalid_input(OP_PROJ, "zero projection vector"));
        }

        // check state - internal surface continuities are better than C0
        let _cache = surface_cache(self)?;
        let _ = (
            uv_domain,
            curve,
            opt_projection_distance,
            opt_approx_tol_3d,
            opt_angle_tol_rad,
        );
        // then: the cache's SurfaceCache::surface_continuity (only reported), the curve's
        // CalculateContinuities (only reported), the projection distance (given, or 4x the
        // largest box distance), the curve's linear sweep along the unit vector times the
        // distance (translated back by half when not given), and:
        // UNPORTED(intersect.adv_surface_intersector): SmAdvSurfaceIntersector::SmAdvSurfaceIntersector
        // (then DoIntersection)
        Err(SolidError::unported(
            "SmAdvSurfaceIntersector::SmAdvSurfaceIntersector",
            "intersect.adv_surface_intersector",
        ))
    }
}

/// `SmSurface::IsPlanar(tol_3d)` of a B-spline surface (the base body over the
/// `SmBSplineSurface::CalculateBoundingBox` override).
///
/// # Errors
///
/// [`SolidError::Unported`] from the bounding box.
pub fn b_spline_is_planar(s: &BSplineSurface, tol_3d: f64) -> Result<Planar> {
    Ok(planar_of(s, &s.natural_uv_domain())?.is_planar(tol_3d))
}

/// The pseudo box of a B-spline surface over `uv_domain`; a failed box is the default
/// box, as upstream ignores the status.
fn planar_of(s: &BSplineSurface, uv_domain: &Extent2d) -> Result<PseudoBox> {
    let req = BoundingBoxRequest {
        normal: false,
        pseudo: true,
        polar: false,
        pseudo_guess: None,
        polar_guess: None,
        expand_by_zone_tol: true,
    };
    Ok(ignored(s.calculate_bounding_box(uv_domain, &req))?
        .and_then(|b| b.pseudo)
        .unwrap_or_else(PseudoBox::new))
}

/// `dBestAnswerSoFarSq` of `GlobalPointSolve_0`.
fn point_solve_best_answer_sq(op: SolverOperation, target: Option<f64>) -> Result<f64> {
    let mut best = BIG_DOUBLE;
    if op == SolverOperation::AtDistance && target.is_none() {
        return Err(SolidError::null_pointer(OP_SOLVE));
    }
    if op == SolverOperation::Minimize || op == SolverOperation::Normalize {
        if let Some(t) = target {
            best = t * t;
        }
    }
    if op == SolverOperation::Maximize {
        best = 0.0;
        if let Some(t) = target {
            best = t * t;
        }
    }
    Ok(best)
}

/// `dBestAnswerSoFarSq` of `GlobalCurveSolve` (and, with `signed_max`, of
/// `GlobalSurfaceSolve`).
fn pair_solve_best_answer_sq(
    op: SolverOperation,
    target: Option<f64>,
    signed_max: bool,
) -> Result<f64> {
    let mut best = BIG_DOUBLE;
    if op == SolverOperation::AtDistance && target.is_none() {
        return Err(SolidError::null_pointer(OP_SOLVE));
    }
    if op == SolverOperation::Minimize {
        if let Some(t) = target {
            best = t * t;
        }
    }
    if op == SolverOperation::Maximize {
        best = 0.0;
        if let Some(t) = target {
            best = t * t;
        }
    }
    if op == SolverOperation::Signed3dDirectedMinimize {
        if let Some(t) = target {
            best = t * t.abs();
        }
    }
    if signed_max && op == SolverOperation::Signed3dDirectedMaximize {
        best = 0.0;
        if let Some(t) = target {
            best = t * t.abs();
        }
    }
    Ok(best)
}

// ---- first-derivative helpers --------------------------------------------------------------

/// The average first derivative magnitudes `(|S_u|, |S_v|)` over a 4x4 grid of
/// `uv_domain` (`ApproxDerivativeLengths`).
///
/// # Errors
///
/// Only [`SolidError::Unported`] (upstream ignores the evaluation statuses).
pub fn approx_derivative_lengths<S: SurfaceEvaluate + ?Sized>(
    surface: &S,
    uv_domain: &Extent2d,
) -> Result<Vec2> {
    // Method: simple, just evaluate on a grid and average.
    const NUM_SAMPLES: usize = 4;
    let samples = [0.15, 0.35, 0.65, 0.85];
    let mut mag_u = 0.0;
    let mut mag_v = 0.0;
    let mut d = (Vec3::UNDEF, Vec3::UNDEF, Vec3::UNDEF);

    for &si in &samples {
        for &sj in &samples {
            let uv = uv_domain.evaluate(si, sj);
            eval_1st_keep(surface, uv, Side::Left, Side::Left, &mut d)?;
            mag_u += d.1.length();
            mag_v += d.2.length();
        }
    }

    mag_u /= (NUM_SAMPLES * NUM_SAMPLES) as f64;
    mag_v /= (NUM_SAMPLES * NUM_SAMPLES) as f64;

    Ok(Vec2::new(mag_u, mag_v))
}

/// The second (and optionally third and fourth) derivatives of a surface curve with
/// first derivative `g1` at `uv` ([`compute_higher_order_derivs`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HigherOrderDerivs {
    /// `rG2Deriv`.
    pub g2: Vec3,
    /// `pOptG3Deriv`, when asked.
    pub g3: Option<Vec3>,
    /// `pOptG4Deriv`, when asked.
    pub g4: Option<Vec3>,
}

/// Finite-difference second, third and fourth directional derivatives of the surface
/// along the parameter-space direction `g1` at `uv` (`ComputeHigherOrderDerivs`): a
/// step of `1e-9 * g1`, backwards when the forward step leaves the natural domain.
///
/// # Errors
///
/// Those of `Evaluate1stDerivatives`.
pub fn compute_higher_order_derivs<S: SurfaceEvaluate + ?Sized>(
    surface: &S,
    uv: Point2,
    g1: Vec2,
    want_g3: bool,
    want_g4: bool,
) -> Result<HigherOrderDerivs> {
    // locals
    let domain = surface.natural_uv_domain();

    // finite difference step - watch for stepping out of surface boundaries
    let mut step = EFF_ZERO_SQRT / 1000.0; // 1.0e-9
    let mut uv1 = uv + step * g1;
    if !domain.contains_point(uv1, 0.0) {
        step = -step;
        uv1 = uv + step * g1;
    }

    // compute 1st derivatives and first directional derivative at target point
    let (_, du, dv) = evaluate_1st_derivatives(surface, uv, Side::Left, Side::Left)?;
    let vec = du * g1.x + dv * g1.y;

    // compute 1st derivatives and first directional derivative at finite step point
    let (_, dud, dvd) = evaluate_1st_derivatives(surface, uv1, Side::Left, Side::Left)?;
    let vec_d1 = dud * g1.x + dvd * g1.y;

    // finite difference approx to 2nd directional derivative
    let delta_g1 = vec_d1 - vec;
    let g2 = delta_g1 / step;

    let mut out = HigherOrderDerivs {
        g2,
        g3: None,
        g4: None,
    };

    // for 3rd and 4th order directional derivatives
    if want_g3 || want_g4 {
        let uv2 = uv1 + step * g1;
        let (_, dud, dvd) = evaluate_1st_derivatives(surface, uv2, Side::Left, Side::Left)?;

        // finite difference approximation to 3rd order directional derivative
        let vec_d2 = dud * g1.x + dvd * g1.y;
        let delta_g1_2 = vec_d1 - vec_d2;
        let delta_g2 = delta_g1_2 - delta_g1;
        if want_g3 {
            out.g3 = Some(delta_g2 / step);
        }

        // get a 4th finite difference point
        let uv3 = uv2 + step * g1;
        let (_, dud, dvd) = evaluate_1st_derivatives(surface, uv3, Side::Left, Side::Left)?;

        // finite difference approximation to 4th order directional derivative
        let vec_d3 = dud * g1.x + dvd * g1.y;
        let delta_g1_3 = vec_d2 - vec_d3;
        let delta_g2_2 = delta_g1_3 - delta_g1_2;
        let delta_g3 = delta_g2_2 - delta_g2;
        if want_g4 {
            out.g4 = Some(delta_g3 / step);
        }
    }

    Ok(out)
}

/// The unit directional derivative at `uv` towards the middle of the natural domain
/// (`ComputeInwardVector`).
///
/// # Errors
///
/// Those of `Evaluate1stDerivatives`; the status of `Unitize` for a zero result.
pub fn compute_inward_vector<S: SurfaceEvaluate + ?Sized>(surface: &S, uv: Point2) -> Result<Vec3> {
    // Surface eval
    let (_, du, dv) = evaluate_1st_derivatives(surface, uv, Side::Left, Side::Left)?;

    // towards midPoint UV vec
    let mid_uv = surface.natural_uv_domain().evaluate(0.5, 0.5);
    let dir_uv = mid_uv - uv;
    let dir_uv = dir_uv.unitize().unwrap_or(dir_uv);

    // directional derivative
    let inward = dir_uv.x * du + dir_uv.y * dv;
    inward.unitize()
}

/// The parameter-space points of `points` dropped into the tangent plane at `uv`
/// (`DropPointsToTangentPlane`); every point maps to `uv` when a first derivative is
/// zero.
///
/// # Errors
///
/// Those of `Evaluate1stDerivatives`.
pub fn drop_points_to_tangent_plane<S: SurfaceEvaluate + ?Sized>(
    surface: &S,
    uv: Point2,
    u_side: Side,
    v_side: Side,
    points: &[Point3],
) -> Result<Vec<Point2>> {
    // get surface point and 1st derivs for input domain point.
    let (pnt, du, dv) = evaluate_1st_derivatives(surface, uv, u_side, v_side)?;

    // no work - a 1st deriv is zero - set output = input and quit
    if du.length_squared() < EFF_ZERO_SQ || dv.length_squared() < EFF_ZERO_SQ {
        return Ok(points.iter().map(|_| uv).collect());
    }

    let mut uv_step = Vec2::UNDEF;
    let mut out = Vec::new();
    for p in points {
        let diff = *p - pnt;
        if let Some(s) = ignored(drop_vectors(du, dv, &[diff]))?.and_then(|v| v.first().copied()) {
            uv_step = s;
        }
        out.push(uv + uv_step);
    }
    Ok(out)
}

/// The parameter-space vectors of `vectors` at `uv` (`DropVectors`).
///
/// # Errors
///
/// Those of `Evaluate1stDerivatives` and `smsurf_DropVectors`.
pub fn drop_vectors_at<S: SurfaceEvaluate + ?Sized>(
    surface: &S,
    uv: Point2,
    u_side: Side,
    v_side: Side,
    vectors: &[Vec3],
) -> Result<Vec<Vec2>> {
    let (_, du, dv) = evaluate_1st_derivatives(surface, uv, u_side, v_side)?;

    // pass the call along
    drop_vectors(du, dv, vectors)
}

// ---- file-static helpers ------------------------------------------------------------------

/// A local point-solve guess for `test_pnt` on `surface` over `uv_domain`
/// (`sm_LoadGuesses`): the best of the domain midpoint, a 2x2 grid (1 sample along a
/// direction with two control points) and one clamped Newton step from each grid
/// point, "best" depending on `op` (minimize, maximize, normalize, intersect).
///
/// # Errors
///
/// Only [`SolidError::Unported`] (upstream ignores the evaluation statuses).
pub fn load_guesses(
    surface: &Surface,
    uv_domain: &Extent2d,
    test_pnt: Point3,
    op: SolverOperation,
) -> Result<Point2> {
    const GRID_SIZE: usize = 2;

    // set gridSize = 1 for linear geometry (an SmBSplineSurface: B-splines and planes)
    let is_bss = surface.is_kind_of(BSplineSurface::TYPE)?;
    let grid_u = if is_bss && surface.number_control_points(SurfParamType::U) == 2 {
        1
    } else {
        GRID_SIZE
    };
    let grid_v = if is_bss && surface.number_control_points(SurfParamType::V) == 2 {
        1
    } else {
        GRID_SIZE
    };

    // get UV mid-point as 1st guess
    let mut close_uv = uv_domain.evaluate(0.5, 0.5);

    // Evaluate MidPoint
    let mut pt = Vec3::UNDEF;
    let mut su = Vec3::UNDEF;
    let mut sv = Vec3::UNDEF;
    let mut ok = match ignored(evaluate_1st_derivatives(
        surface,
        close_uv,
        Side::Left,
        Side::Left,
    ))? {
        Some(d) => {
            (pt, su, sv) = d;
            true
        }
        None => false,
    };
    if !ok {
        if let Some(p) = ignored(surface.evaluate_point(close_uv))? {
            pt = p;
        }
    }

    // get current distance and upcoming NewtonRaphson values
    let mut surf_to_pt = pt - test_pnt;
    let mut f = surf_to_pt.dot(su);
    let mut g = surf_to_pt.dot(sv);

    // save current values as initial best values
    let mut best_dist = surf_to_pt.length();
    let mut best_fg = if ok { f.abs() + g.abs() } else { BIG_DOUBLE };

    // Make a clamping domain that runs halfway between the outermost sample points
    // and the real domain boundary.
    let uv_ext = Extent2d::from_points(
        uv_domain.evaluate(0.5 / ((2 * grid_u) as f64), 0.5 / ((2 * grid_v) as f64)),
        uv_domain.evaluate(
            1.0 - 0.5 / ((2 * grid_u) as f64),
            1.0 - 0.5 / ((2 * grid_v) as f64),
        ),
    );

    let mut suv = Vec3::UNDEF;
    let mut suu = Vec3::UNDEF;
    let mut svv = Vec3::UNDEF;
    let better = |ok: bool, dist: f64, fg: f64, best_dist: f64, best_fg: f64| {
        (op == SolverOperation::Minimize && dist < best_dist)
            || (op == SolverOperation::Maximize && dist > best_dist)
            || (op == SolverOperation::Normalize && ok && fg < best_fg)
            || (op == SolverOperation::Intersect && dist < best_dist)
    };

    // Sample every grid point - Take 1 NewtonRaphson Step from there and keep the best
    for ii in 0..grid_u {
        let prm_u = (2 * ii + 1) as f64 / (2 * grid_u) as f64;
        for jj in 0..grid_v {
            let prm_v = (2 * jj + 1) as f64 / (2 * grid_v) as f64;

            // skip grid points that don't evaluate to 2nd derivatives
            let mut uv = uv_domain.evaluate(prm_u, prm_v);
            ok = match ignored(evaluate_2nd_derivatives(
                surface,
                uv,
                Side::Left,
                Side::Left,
            ))? {
                Some(d) => {
                    pt = d.point;
                    su = d.du;
                    sv = d.dv;
                    suv = d.duv;
                    suu = d.duu;
                    svv = d.dvv;
                    true
                }
                None => false,
            };

            // get dist and NewtonRaphson function values from SamplePoint to TestPoint
            surf_to_pt = pt - test_pnt;
            f = surf_to_pt.dot(su);
            g = surf_to_pt.dot(sv);
            let mut this_dist = surf_to_pt.length();
            let mut this_fg = if ok { f.abs() + g.abs() } else { BIG_DOUBLE };

            // Save the best grid point for the current operation
            if better(ok, this_dist, this_fg, best_dist, best_fg) {
                close_uv = uv;
                best_dist = this_dist;
                if this_fg < best_fg {
                    best_fg = this_fg;
                }
            }

            // skip sample points that failed to generate 2nd order derivative values
            if !ok {
                continue;
            }

            // Take 1 clamped Newton-Raphson step towards F=0 and G=0
            let fu = su.dot(su) + surf_to_pt.dot(suu);
            let fv = su.dot(sv) + surf_to_pt.dot(suv);
            let gu = fv;
            let gv = sv.dot(sv) + surf_to_pt.dot(svv);

            let den = fu * gv - fv * gu;
            if den.abs() < EFF_ZERO_SQ {
                continue; // just ignore if singular.
            }

            let num_u = g * fv - gv * f;
            let num_v = f * gu - g * fu;

            uv = Point2::new(uv.x + num_u / den, uv.y + num_v / den);
            uv = uv_ext.clamp_point(uv);

            // evaluate and judge the New Point
            ok = match ignored(evaluate_1st_derivatives(
                surface,
                uv,
                Side::Left,
                Side::Left,
            ))? {
                Some(d) => {
                    (pt, su, sv) = d;
                    true
                }
                None => false,
            };
            if !ok {
                if let Some(p) = ignored(surface.evaluate_point(uv))? {
                    pt = p;
                }
            }

            surf_to_pt = pt - test_pnt;
            f = surf_to_pt.dot(su);
            g = surf_to_pt.dot(sv);
            this_dist = surf_to_pt.length();
            this_fg = f.abs() + g.abs();

            // Save the best NewtonRaphson point for the current operation
            if better(ok, this_dist, this_fg, best_dist, best_fg) {
                close_uv = uv;
                best_dist = this_dist;
                if this_fg < best_fg {
                    best_fg = this_fg;
                }
            }
        }
    }

    Ok(close_uv)
}

/// Whether the surface point at `uv` and the curve point at `parameter` are a
/// converged curve/surface extremum (`sm_PassSCConvergenceTest`): the displacement is
/// (numerically) perpendicular to both surface tangents and the curve tangent.
///
/// # Errors
///
/// Only [`SolidError::Unported`] (upstream ignores the evaluation statuses).
pub fn pass_sc_convergence_test<S: SurfaceEvaluate + ?Sized>(
    surface: &S,
    uv: Point2,
    curve: &Curve,
    parameter: f64,
) -> Result<bool> {
    // surface eval
    let mut d = (Vec3::UNDEF, Vec3::UNDEF, Vec3::UNDEF);
    eval_1st_keep(surface, uv, Side::Left, Side::Left, &mut d)?;
    let (pnt, mut du, mut dv) = d;

    // curve eval
    let mut pv = [Vec3::UNDEF, Vec3::UNDEF];
    if let Some(v) = ignored(curve.evaluate(parameter, 1, true, true))? {
        for (dst, src) in pv.iter_mut().zip(v) {
            *dst = src;
        }
    }
    let [pv0, mut pv1] = pv;

    // curve surface displacement
    let mut diff = pnt - pv0;
    let scale = 1.0 + pnt.max_dimension();

    if du.dot(diff).abs() > EFF_ZERO * scale
        || dv.dot(diff).abs() > EFF_ZERO * scale
        || pv1.dot(diff).abs() > EFF_ZERO * scale
    {
        diff = diff.unitize_or_z();
        if du.length_squared() > EFF_ZERO_SQ {
            du = du.unitize_or_z();
            if du.dot(diff).abs() > EFF_ZERO {
                return Ok(false);
            }
        }
        if dv.length_squared() > EFF_ZERO_SQ {
            dv = dv.unitize_or_z();
            if dv.dot(diff).abs() > EFF_ZERO {
                return Ok(false);
            }
        }
        if pv1.length_squared() > EFF_ZERO_SQ {
            pv1 = pv1.unitize_or_z();
            if pv1.dot(diff).abs() > EFF_ZERO {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// Whether the points at `uv` on `surface` and `uv2` on `surface2` are a converged
/// surface/surface extremum (`sm_PassSSConvergenceTest`).
///
/// # Errors
///
/// Only [`SolidError::Unported`] (upstream ignores the evaluation statuses).
pub fn pass_ss_convergence_test<S1, S2>(
    surface: &S1,
    uv: Point2,
    surface2: &S2,
    uv2: Point2,
) -> Result<bool>
where
    S1: SurfaceEvaluate + ?Sized,
    S2: SurfaceEvaluate + ?Sized,
{
    let mut d1 = (Vec3::UNDEF, Vec3::UNDEF, Vec3::UNDEF);
    eval_1st_keep(surface, uv, Side::Left, Side::Left, &mut d1)?;
    let mut d2 = (Vec3::UNDEF, Vec3::UNDEF, Vec3::UNDEF);
    eval_1st_keep(surface2, uv2, Side::Left, Side::Left, &mut d2)?;
    let (pnt, mut du, mut dv) = d1;
    let (pnt2, mut du2, mut dv2) = d2;
    let mut diff = pnt - pnt2;
    if du.dot(diff).abs() > EFF_ZERO
        || dv.dot(diff).abs() > EFF_ZERO
        || du2.dot(diff).abs() > EFF_ZERO
        || dv2.dot(diff).abs() > EFF_ZERO
    {
        if diff.length_squared() > EFF_ZERO_SQ {
            diff = diff.unitize_or_z();
        }
        for v in [&mut du, &mut dv, &mut du2, &mut dv2] {
            if v.length_squared() > EFF_ZERO_SQ {
                *v = v.unitize_or_z();
                if v.dot(diff).abs() > EFF_ZERO {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

/// Which side of a seam of `this_surf` a point dropped from a boundary point of
/// `other_surf` should be on (`sm_CheckSeamDrop`): `this_uv` unchanged off seams,
/// moved to the domain boundary whose tangent agrees with the other surface's inward
/// tangent on a seam.
///
/// # Errors
///
/// `SM_ERR` at a pole or for a zero tangent of the other surface;
/// [`SolidError::Unported`] on a seam (`SmSurface::EvaluateSTEP`).
pub fn check_seam_drop<S: SurfaceEvaluate + ?Sized>(
    this_surf: &Surface,
    this_uv: Point2,
    this_domain: &Extent2d,
    other_surf: &S,
    other_uv: Point2,
    other_domain: &Extent2d,
    location: LocationType,
) -> Result<Point2> {
    if location == LocationType::Pole {
        return Err(SolidError::sm_err(OP_SEAM));
    }

    if location != LocationType::USeam
        && location != LocationType::VSeam
        && location != LocationType::UvSeam
    {
        return Ok(this_uv);
    }

    // pOther point is dropped onto a seam in pThis (evaluated in its STEP
    // parameterisation). Upstream ignores the status: on failure sPtDer keeps its
    // default-constructed (undefined) vectors.
    let pt_der = ignored(evaluate_step(this_surf, this_uv))?
        .unwrap_or_else(|| vec![vec![Vec3::UNDEF; 2]; 2]);

    let mut d = (Vec3::UNDEF, Vec3::UNDEF, Vec3::UNDEF);
    eval_1st_keep(other_surf, other_uv, Side::Left, Side::Left, &mut d)?;
    let (_, other_u, other_v) = d;

    // Unitize them, so that dot-product comparisons mean direction only.
    let mut other_u = other_u.unitize().map_err(|_| SolidError::sm_err(OP_SEAM))?;
    let mut other_v = other_v.unitize().map_err(|_| SolidError::sm_err(OP_SEAM))?;

    // Flip directions of the derivatives if they are at the top of their domains.
    if other_uv.x > other_domain.u_interval().mid() {
        other_u = -other_u;
    }
    if other_uv.y > other_domain.v_interval().mid() {
        other_v = -other_v;
    }

    let mut out = this_uv;
    let pick = |this_deriv: Vec3| {
        let dot_u = this_deriv.dot(other_u);
        let dot_v = this_deriv.dot(other_v);
        let other_deriv = if dot_u.abs() > dot_v.abs() {
            other_u
        } else {
            other_v
        };
        this_deriv.dot(other_deriv) > 0.0
    };

    if location == LocationType::USeam || location == LocationType::UvSeam {
        // Closed in U: get our u-deriv
        out.x = if pick(at(&pt_der, 1, 0)?) {
            this_domain.min().x
        } else {
            this_domain.max().x
        };
    }

    if location == LocationType::VSeam || location == LocationType::UvSeam {
        // Closed in V: get our v-deriv
        out.y = if pick(at(&pt_der, 0, 1)?) {
            this_domain.min().y
        } else {
            this_domain.max().y
        };
    }

    Ok(out)
}

/// Whether a local point drop from `guess_uv` (solution value `sol_value` at
/// `sol_uv`) converged well enough to skip the global solve (`sm_GoodDropResult`).
///
/// # Errors
///
/// Only [`SolidError::Unported`] (upstream ignores the evaluation statuses).
pub fn good_drop_result<S: SurfaceEvaluate + ?Sized>(
    surface: &S,
    domain: &Extent2d,
    pt_to_drop: Point3,
    guess_uv: Point2,
    sol: &SolutionEnd,
) -> Result<bool> {
    // Check whether it's right on.
    if sol.solution_value < EFF_ZERO {
        return Ok(true);
    }

    let result_uv = Point2::new(sol.param(0)?, sol.param(1)?);

    // If result is far from guess, it's suspicious.
    let max_step = 0.50;
    let uv_step = result_uv - guess_uv;
    if uv_step.x.abs() > max_step * domain.x_length() {
        return Ok(false);
    }
    if uv_step.y.abs() > max_step * domain.y_length() {
        return Ok(false);
    }

    // Evaluate the error functions: how far from the surface normal.
    let mut d = (Vec3::UNDEF, Vec3::UNDEF, Vec3::UNDEF);
    eval_1st_keep(surface, result_uv, Side::Left, Side::Left, &mut d)?;
    let (srf_point, su, sv) = d;
    let su = su.unitize_or_z();
    let sv = sv.unitize_or_z();
    let gap = pt_to_drop - srf_point;
    let fu = gap.dot(su);
    let fv = gap.dot(sv);
    if fu.abs() + fv.abs() < 0.0001 {
        return Ok(true);
    }

    // Check u-param first.
    if result_uv.x < domain.min().x + EFF_ZERO {
        if fu > EFF_ZERO {
            return Ok(false);
        }
    } else if result_uv.x > domain.max().x - EFF_ZERO {
        if fu < -EFF_ZERO {
            return Ok(false);
        }
    } else if fu.abs() > 0.001 {
        return Ok(false); // interior, bad fcn value.
    }

    // Now check v-param.
    if result_uv.y < domain.min().y + EFF_ZERO {
        if fv > EFF_ZERO {
            return Ok(false);
        }
    } else if result_uv.y > domain.max().y - EFF_ZERO {
        if fv < -EFF_ZERO {
            return Ok(false);
        }
    } else if fv.abs() > 0.001 {
        return Ok(false); // interior, bad fcn value.
    }

    // One more: if the solution 3d point is farther than the guess (upstream's names
    // are swapped).
    let mut guess_3d = Vec3::UNDEF;
    if let Some(p) = ignored(surface.evaluate_point(guess_uv))? {
        guess_3d = p;
    }
    let dsq_guess = pt_to_drop.distance_squared(srf_point);
    let dsq_result = pt_to_drop.distance_squared(guess_3d);
    if dsq_result > dsq_guess {
        return Ok(false);
    }

    Ok(true)
}

// ---- the global solvers --------------------------------------------------------------------

/// The point/surface global solver (`SmSPGlobalSolver`): a one-node point tree and
/// the surface cache tree. Its constructor belongs to unit `surface.surface_p1`; the
/// fields are public.
#[derive(Debug, Clone)]
pub struct SpGlobalSolver<'t, 's, O> {
    /// The `SmGlobalSolver` state.
    pub base: GlobalSolver<'t, O>,
    /// `m_pSurf`.
    pub surface: Option<&'s Surface>,
    /// `m_crUVDomain`.
    pub uv_domain: Extent2d,
}

impl<O: SolutionObject> SpGlobalSolver<'_, '_, O> {
    /// `BranchMayContainAnswers`: false for a surface node outside its face, and when
    /// normalizing, for a patch whose normal cone misses every direction to the point;
    /// else the default branch test.
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`]: `sm_IsNodeValid` and the patch data (the solver's
    /// cache, from its constructor) (`surface.surface_p1`).
    pub fn may_contain_answers(&self, branch: &Branch) -> Result<bool> {
        let surface_node = self.base.branch_node(branch, 1)?;
        if !is_node_valid(surface_node)? {
            return Ok(false);
        }

        // If we are normalizing and have a node with a surface do the angular test
        // using the Gauss map of the normal.
        if self.base.operation == SolverOperation::Normalize
            && surface_node.aux_type == AuxDataType::BezierSurface
            && surface_node.child1.is_some()
        {
            // SmBezierPatch::GetPseudoBox / GetPolarBox are ported (BezierPatch::pseudo_box,
            // SurfaceCache::polar_box), and the patch (the node's m_pData) is
            // SurfaceCache::patch(node) of the surface's cache, but the solver holds no
            // cache until its constructor (surface.surface_p1) hands one over.
            // UNPORTED(surface.surface_p1): SmSPGlobalSolver::SmSPGlobalSolver
            // (then the point inside the pseudo box, else the polar box of the vectors to
            // its 8 corners against the patch's polar box)
            return Err(SolidError::unported(
                "SmSPGlobalSolver::SmSPGlobalSolver",
                "surface.surface_p1",
            ));
        }

        // check current node using the Default Branch check
        self.base.branch_may_contain_answers(branch)
    }
}

impl<'t, O: SolutionObject + 't> GlobalSolve<'t, O> for SpGlobalSolver<'t, '_, O> {
    fn solver(&self) -> &GlobalSolver<'t, O> {
        &self.base
    }

    fn solver_mut(&mut self) -> &mut GlobalSolver<'t, O> {
        &mut self.base
    }

    fn branch_may_contain_answers(&mut self, branch: &Branch) -> Result<bool> {
        self.may_contain_answers(branch)
    }

    fn local_solve(&mut self, _branch: &Branch) -> Result<bool> {
        // UNPORTED(surface.surface_p8): SmSPGlobalSolver::LocalSolve
        Err(SolidError::unported(
            "SmSPGlobalSolver::LocalSolve",
            "surface.surface_p8",
        ))
    }

    fn identical_solutions(
        &self,
        _sol1: &Solution<O>,
        _sol2: &Solution<O>,
    ) -> Result<(SolutionPairType, Option<PreferredSolution>)> {
        // UNPORTED(surface.surface_p5): SmSPGlobalSolver::IdenticalSolutions
        Err(SolidError::unported(
            "SmSPGlobalSolver::IdenticalSolutions",
            "surface.surface_p5",
        ))
    }
}

/// The surface/surface global solver (`SmSSGlobalSolver`): two surface cache trees.
/// Its constructor belongs to unit `surface.surface_p1`; the fields are public.
#[derive(Debug, Clone)]
pub struct SsGlobalSolver<'t, 's, O> {
    /// The `SmGlobalSolver` state.
    pub base: GlobalSolver<'t, O>,
    /// `m_pSurf1`.
    pub surface1: Option<&'s Surface>,
    /// `m_pSurf2`.
    pub surface2: Option<&'s Surface>,
    /// `m_crUVDomain1`.
    pub uv_domain1: Extent2d,
    /// `m_crUVDomain2`.
    pub uv_domain2: Extent2d,
}

impl<O: SolutionObject> SsGlobalSolver<'_, '_, O> {
    /// `BranchMayContainAnswers`: when normalizing two Bezier patch nodes, false when
    /// the patches' normal cones exclude a common normal direction; else the default
    /// branch test.
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] for two patch nodes when normalizing (the patch data
    /// needs the solver's caches, from its constructor: `surface.surface_p1`).
    pub fn may_contain_answers(&self, branch: &Branch) -> Result<bool> {
        if self.base.operation == SolverOperation::Normalize {
            let node1 = self.base.branch_node(branch, 0)?;
            let node2 = self.base.branch_node(branch, 1)?;
            if node1.aux_type == AuxDataType::BezierSurface
                && node2.aux_type == AuxDataType::BezierSurface
            {
                // The patches (the nodes' m_pData) are SurfaceCache::patch(node) of the
                // two surface caches, but the solver holds no caches until its
                // constructor (surface.surface_p1) hands them over.
                // UNPORTED(surface.surface_p1): SmSSGlobalSolver::SmSSGlobalSolver
                // (then sm_IsNodeValid of both nodes, returning SM_SUCCESS = FALSE when
                // outside, ComputeConeBoundTo and the polar box cone tests)
                return Err(SolidError::unported(
                    "SmSSGlobalSolver::SmSSGlobalSolver",
                    "surface.surface_p1",
                ));
            }
        }

        self.base.branch_may_contain_answers(branch)
    }
}

impl<'t, O: SolutionObject + 't> GlobalSolve<'t, O> for SsGlobalSolver<'t, '_, O> {
    fn solver(&self) -> &GlobalSolver<'t, O> {
        &self.base
    }

    fn solver_mut(&mut self) -> &mut GlobalSolver<'t, O> {
        &mut self.base
    }

    fn branch_may_contain_answers(&mut self, branch: &Branch) -> Result<bool> {
        self.may_contain_answers(branch)
    }

    fn local_solve(&mut self, _branch: &Branch) -> Result<bool> {
        // UNPORTED(surface.surface_p11): SmSSGlobalSolver::LocalSolve
        Err(SolidError::unported(
            "SmSSGlobalSolver::LocalSolve",
            "surface.surface_p11",
        ))
    }

    fn identical_solutions(
        &self,
        _sol1: &Solution<O>,
        _sol2: &Solution<O>,
    ) -> Result<(SolutionPairType, Option<PreferredSolution>)> {
        // UNPORTED(surface.surface_p6): SmSSGlobalSolver::IdenticalSolutions
        Err(SolidError::unported(
            "SmSSGlobalSolver::IdenticalSolutions",
            "surface.surface_p6",
        ))
    }
}

// ---- callees in units not ported yet -------------------------------------------------------

/// `SmSurfaceCache::GetTree` of a surface cache.
fn surface_tree<C>(_cache: &C) -> Result<()> {
    // GetTree is SurfaceCache::get_tree, but no cache store resolves a surface cache's
    // handle yet: smsurf_GetSurfaceCache makes the caches through
    // SmSurface::CacheMakeOrValidate.
    // UNPORTED(surface.surface_p1): SmSurface::CacheMakeOrValidate
    Err(SolidError::unported(
        "SmSurface::CacheMakeOrValidate",
        "surface.surface_p1",
    ))
}

/// `SmSurface::IsFlatCorner(uv, angle_tol_deg)`.
fn is_flat_corner(_surface: &Surface, _uv: Point2, _angle_tol_deg: f64) -> Result<bool> {
    // UNPORTED(surface.surface_p1): SmSurface::IsFlatCorner
    Err(SolidError::unported(
        "SmSurface::IsFlatCorner",
        "surface.surface_p1",
    ))
}

/// `SmSurface::EvaluateSTEP(uv, 1, 1, TRUE, TRUE, TRUE, ..)`.
fn evaluate_step(_surface: &Surface, _uv: Point2) -> Result<Vec<Vec<Vec3>>> {
    // UNPORTED(surface.surface_p1): SmSurface::EvaluateSTEP
    Err(SolidError::unported(
        "SmSurface::EvaluateSTEP",
        "surface.surface_p1",
    ))
}

/// `sm_IsNodeValid`: false for a node known to lie outside its face.
fn is_node_valid<O>(_node: &crate::geom::tree::TreeNode<O>) -> Result<bool> {
    // UNPORTED(surface.surface_p1): sm_IsNodeValid
    Err(SolidError::unported("sm_IsNodeValid", "surface.surface_p1"))
}

/// The Newton solve of `FindDegenParamForDirection`
/// (`SmFindSingularParamWithGivenTangentDirEFO` with `SmLocalSolve1d::SolveIt` from
/// `start`): `(found, t)`.
fn singular_param_solve(
    _surface: &Surface,
    _uv: Point2,
    _dir: Vec3,
    _singular_dir: SurfParamType,
    _ivl: &Extent1d,
    _start: f64,
    _tol: f64,
) -> Result<(bool, f64)> {
    // UNPORTED(surface.surface_p1): SmFindSingularParamWithGivenTangentDirEFO::SmFindSingularParamWithGivenTangentDirEFO
    Err(SolidError::unported(
        "SmFindSingularParamWithGivenTangentDirEFO::SmFindSingularParamWithGivenTangentDirEFO",
        "surface.surface_p1",
    ))
}

/// The `SmLocalSolveNd` solve of `LocalCurveSolve` over `SmCSNormalizeENFO`
/// (boundary handler `SM_BH_TOTAL_BOUNDARY_HITS` 5, accuracy `SM_EFF_ZERO_SQRT`,
/// evaluator tolerance `SM_EFF_ZERO^2`, no periodicity): the solution vector, `None`
/// when not found.
fn cs_normalize_local_solve(
    _surface: &Surface,
    _curve: &Curve,
    _op: SolverOperation,
    _distance_tolerance: f64,
    _opt_vectors: &[Vec3],
    _intervals: &[Extent1d],
    _guess: &[f64],
) -> Result<Option<Vec<f64>>> {
    // UNPORTED(surface.surface_p1): SmCSNormalizeENFO::SmCSNormalizeENFO
    // UNPORTED(surface.surface_p4): SmCSNormalizeENFO::Evaluate
    Err(SolidError::unported(
        "SmCSNormalizeENFO::SmCSNormalizeENFO",
        "surface.surface_p1",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::plane::Plane;

    fn plane() -> Surface {
        Surface::from(Plane::from_normal(
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(0.0, 0.0, 1.0),
        ))
    }

    #[test]
    fn plane_normal_is_axis() {
        let s = plane();
        let n = s
            .evaluate_normal(Point2::new(0.2, 0.3), Side::Left, Side::Left)
            .unwrap();
        assert!((n.z - 1.0).abs() < 1e-12);
    }

    #[test]
    fn base_normal_of_plane_part() {
        let s = plane();
        let n = base_evaluate_normal(&s, Point2::new(0.2, 0.3), Side::Left, Side::Left).unwrap();
        assert!((n.z.abs() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn best_answer_init() {
        assert_eq!(
            point_solve_best_answer_sq(SolverOperation::Maximize, None).unwrap(),
            0.0
        );
        assert_eq!(
            pair_solve_best_answer_sq(SolverOperation::Signed3dDirectedMinimize, Some(-2.0), false)
                .unwrap(),
            -4.0
        );
        assert!(point_solve_best_answer_sq(SolverOperation::AtDistance, None).is_err());
    }

    #[test]
    fn inward_vector_points_to_middle() {
        let s = plane();
        let d = s.natural_uv_domain();
        let v = compute_inward_vector(&s, d.min()).unwrap();
        assert!((v.length() - 1.0).abs() < 1e-12);
    }
}
