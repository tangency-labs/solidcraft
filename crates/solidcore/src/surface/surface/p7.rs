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
//! | [`Surface::global_curve_solve`], [`Surface::global_curve_solve_with_context`] | `GlobalCurveSolve`        |
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
//!
//! Deviations:
//! - **Generic bodies.** The `SmSurface` bodies that only call the evaluation virtuals
//!   (`Evaluate`, `EvaluatePoint`, `GetNaturalUVDomain`) are generic over
//!   [`SurfaceEvaluate`], so a caller holding a bare [`BSplineSurface`] (which does not
//!   override them) runs the same code as one holding a [`Surface`].
//! - **Evaluation wrappers.** `Evaluate1stDerivatives`, `Evaluate2ndDerivatives` and
//!   `EvaluateDirectionalDerivs` (first co-ported here) are
//!   [`crate::surface::surface::p1`]'s generic functions.
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
//!   filling the corner points (the opt-in `Kernel::fixes.flat_corners_test_bits` tests each
//!   bit instead, WR-384); `SmSSGlobalSolver::BranchMayContainAnswers` returns
//!   `SM_SUCCESS` (that is, `FALSE`) for a node outside its face and `NER`'s status
//!   (non-zero: `TRUE`) for a missing patch; `sm_GoodDropResult` compares the guess and
//!   result distances with swapped names (the opt-in
//!   `Kernel::fixes.good_drop_result_distances_in_order` compares them the right way round
//!   and takes a foot only at the local solve's tight tolerance, WR-56); `FindDegenParamForDirection` compares a sample fraction with the parameter
//!   `dS` (the opt-in `Kernel::fixes.degen_param_tiebreak_uses_actual_param` compares the
//!   mapped singular parameter `s_ivl.evaluate(...)` instead, WR-57).
//! - **Projection curve class.** `CreateParallelProjectionCurves` takes a [`Curve`] for
//!   upstream's `const SmBSplineCurve&`; a curve outside the `SmBSplineCurve` classes
//!   (B-spline, line, ellipse, hyperbola, parabola), which upstream's type rules out,
//!   is `SM_ERR_INVALID_INPUT` before the sweep.
//! - **Projection approximation tolerance (HANG-32).** The public
//!   `CreateParallelProjectionCurves` refuses a supplied approximation tolerance that
//!   is not finite, positive and normal before preparing the cache or geometry. The
//!   C++ reference is reported to run for more than 15 minutes on the smallest
//!   subnormal (the looping routine was not located), and the FFI already refuses such
//!   inputs. `None` and every finite positive normal value are accepted, with no upper
//!   cap. This is a deterministic early public-API contract, not a claim that the Rust
//!   solver would otherwise fail to terminate: its global solves are bounded
//!   ([`crate::geom::global_solver::MAX_SOLVE_DEPTH`] /
//!   [`crate::geom::global_solver::MAX_SOLVE_BRANCHES`]).
//! - **`GlobalPointSolve_0`** takes the kernel and the surface's handle `surface: O`
//!   (the cache's owner object and the solutions' object), as `GlobalLineIntersect`
//!   does; it and `SmSPGlobalSolver::BranchMayContainAnswers` (which reads the cache
//!   through the solver's `surface_cache`) run the solver whose leaf solve is
//!   [`SpGlobalSolver::local_solve_branch`] (`surface.surface_p8`).
//! - **Unported callees** (return [`SolidError::Unported`]): the surface cache and its
//!   tree (`smsurf_GetSurfaceCache` through `SmSurface::CacheMakeOrValidate`, unit
//!   `surface.surface_p1`; `SmSurfaceCache::GetTree` is
//!   [`crate::surface::surface_cache::SurfaceCache::get_tree`] once a cache handle
//!   resolves) for the global solves and the
//!   parallel projection; the solver constructors, `SmCSNormalizeENFO`,
//!   `SmFindSingularParamWithGivenTangentDirEFO`, `sm_IsNodeValid`, `IsFlatCorner`,
//!   `EvaluateSTEP`, `GetFace` and the base `CalculateBoundingBox`
//!   (`surface.surface_p1`; the solver constructors also hand over the caches holding
//!   the patch data of tree nodes).
//! - **No evidence (WR-663).** With the opt-in `Kernel::fixes.query_statuses_pass_on`,
//!   `IsClosed` is `false` when an end, the continuity, the mid derivatives or the
//!   Frenet frame cannot be evaluated, and the base `IsPlanar` is `false` without its
//!   box, as the fork (tangency-labs/smlib 21dcf69). The same flag passes on the other
//!   statuses this file's functions drop and gives their soft answers (the convergence
//!   tests, `sm_GoodDropResult`, `ApproxDerivativeLengths`, `FindDegenParamForDirection`,
//!   `sm_CheckSeamDrop`, `sm_LoadGuesses`), as do the other `surface::surface` files
//!   (`plan/upstream-issues/WR-663--addendum-2026-10-31-surface-statuses.md`).

use crate::curve::curve::{Curve, CurveKind};
use crate::error::{
    ignore_or_pass_on, ignore_status, ignored, ignored_or_pass_on, passed_on, Result, SolidError,
};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent1d::ExtentNd;
use crate::geom::extent2d::Extent2d;
use crate::geom::geom_utility::p2::{
    evaluate_curve_continuity_with, frenet_from_derivatives, ContinuityOptions, ContinuityType,
    CurveEval,
};
use crate::geom::global_solver::{
    Branch, GlobalSolve, GlobalSolver, PreferredSolution, SolutionPairType, SolutionRequested,
    SolveRequest, SolverOperation,
};
use crate::geom::local_solve_nd::{BoundaryHandler, LocalSolveNd};
use crate::geom::polar_box::PolarBox;
use crate::geom::pseudo_box::{Planar, PseudoBox};
use crate::geom::solution_array::{Solution, SolutionArray, SolutionEnd, SolutionObject};
use crate::geom::tree::{AuxDataType, Tree};
use crate::geom::vector2d::Vec2;
use crate::geom::vector3d::sqrt as sm_sqrt;
use crate::kernel::Kernel;
use crate::math::{
    max2, Point2, Point3, Vec3, BIG_DOUBLE, EFF_ZERO, EFF_ZERO_SQ, EFF_ZERO_SQRT, UNDEF_DOUBLE,
};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::{
    BSplineSurface, BoundingBoxRequest, LocationType, SurfParamType,
};
use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use crate::nurbs::knots::Side;
use crate::surface::surface_cache::SurfaceCache;
use crate::topology::BrepId;

use super::p1::{
    evaluate_1st_derivatives, evaluate_2nd_derivatives, evaluate_directional_derivs, is_node_valid,
    CsNormalizeEnfo, OperandDomain, SecondDerivatives, SolveOperands, SurfaceSolveContext,
};
use super::p2::{derivative_limit, drop_vectors, normal_limit, SurfaceEvaluate};
use super::{Surface, SurfaceKind};

const OP_DEGEN: &str = "surface::surface::p7::Surface::find_degen_param_for_direction";
const OP_SEAM: &str = "surface::surface::p7::check_seam_drop";
const OP_SOLVE: &str = "surface::surface::p7::Surface::global_solve";
const OP_MAY: &str = "surface::surface::p7::SpGlobalSolver::may_contain_answers";
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

/// `Evaluate1stDerivatives` whose status upstream ignores: the outputs are
/// updated only on success.
fn eval_1st_keep<S: SurfaceEvaluate + ?Sized>(
    k: &Kernel,
    surface: &S,
    uv: Point2,
    u_side: Side,
    v_side: Side,
    out: &mut (Point3, Vec3, Vec3),
) -> Result<bool> {
    match ignored(evaluate_1st_derivatives(k, surface, uv, u_side, v_side))? {
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
    k: &Kernel,
    surface: &S,
    uv: Point2,
    u_side: Side,
    v_side: Side,
) -> Result<Vec3> {
    // locals
    let mut d1 = (Vec3::UNDEF, Vec3::UNDEF, Vec3::UNDEF);

    // evaluate 1st derivatives at uv
    let first_ok = eval_1st_keep(k, surface, uv, u_side, v_side, &mut d1)?;
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

    let uv_domain = surface.natural_uv_domain_with_nurb(k)?;
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
            if !eval_1st_keep(k, surface, suv, u_side, v_side, &mut d)? {
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
                if !eval_1st_keep(k, surface, suv2, u_side, v_side, &mut d2)? {
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
                                                  // (passed on with the opt-in fix, WR-663, tangency-labs/smlib 21dcf69)
                    normal = if k.fixes.query_statuses_pass_on {
                        normal.unitize()?
                    } else {
                        normal.unitize_or_z()
                    };
                }

                return Ok(normal);
            }

            step *= 10.0; // Try again with a bigger stepoff.
        }
    }

    // Use higher-order derivatives to see how the zero-derivative is changing.
    // Don't have it try to correct zero derivatives.
    if let Some(m) = ignored(surface.evaluate(k, uv, 2, 2, flags(u_side, v_side, false)))? {
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
            if !eval_1st_keep(k, surface, suv, u_side, v_side, &mut d)? {
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
/// `smgu_EvaluateCurveContinuity` (see the module docs; WR-380's opt-in G2 test from
/// `Kernel::fixes.geometric_g2_continuity`).
pub fn base_is_closed<S: SurfaceEvaluate + ?Sized>(
    k: &Kernel,
    surface: &S,
    uv_domain: &Extent2d,
    param: SurfParamType,
    opt_tol: Option<f64>,
    want_continuity: bool,
) -> Result<(bool, Option<ContinuityType>)> {
    let discontinuous = want_continuity.then_some(ContinuityType::Discontinuous);
    // with the opt-in fix a surface that cannot be evaluated is not shown closed
    // (tangency-labs/smlib 21dcf69)
    let not_shown = k.fixes.query_statuses_pass_on;

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
                match ignored(evaluate_2nd_derivatives(
                    k,
                    surface,
                    uv_min,
                    Side::Left,
                    Side::Left,
                ))? {
                    Some(d) => d2_min = d,
                    None if not_shown => return Ok((false, discontinuous)),
                    None => (),
                }
                match ignored(evaluate_2nd_derivatives(
                    k,
                    surface,
                    uv_max,
                    Side::Left,
                    Side::Left,
                ))? {
                    Some(d) => d2_max = d,
                    None if not_shown => return Ok((false, discontinuous)),
                    None => (),
                }
                p_min = d2_min.point;
                p_max = d2_max.point;
            } else {
                match ignored(surface.evaluate_point(k, uv_min))? {
                    Some(p) => p_min = p,
                    None if not_shown => return Ok((false, discontinuous)),
                    None => (),
                }
                match ignored(surface.evaluate_point(k, uv_max))? {
                    Some(p) => p_max = p,
                    None if not_shown => return Ok((false, discontinuous)),
                    None => (),
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
                let curve_continuity = match evaluate_curve_continuity_with(
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
                    &ContinuityOptions {
                        geometric_g2: k.fixes.geometric_g2_continuity,
                    },
                ) {
                    Err(e) if not_shown && !e.is_unported() => return Ok((false, discontinuous)),
                    r => r?,
                };

                // save lowest found continuity
                if (min_continuity as i32) > (curve_continuity as i32) {
                    min_continuity = curve_continuity;
                }
            }

            // extra test
            if (min_continuity as i32) > (ContinuityType::Discontinuous as i32) {
                // filter out very short curve passing as closed curves
                match ignored(evaluate_directional_derivs(
                    k,
                    surface,
                    uv_mid,
                    uv_dir,
                    2,
                    Side::Left,
                    Side::Left,
                ))? {
                    Some(d) => {
                        for (dst, src) in p_mid.iter_mut().zip(d) {
                            *dst = src;
                        }
                    }
                    None if not_shown => return Ok((false, discontinuous)),
                    None => (),
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
                    } else if not_shown {
                        return Ok((false, discontinuous));
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
    /// plane, the basis surface's normal for a curve-bounded surface, the offset
    /// surface's override, the [`base_evaluate_normal`] body otherwise.
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`]; those of
    /// [`OffsetSurface::evaluate_normal`](crate::surface::offset_surface::OffsetSurface::evaluate_normal).
    pub fn evaluate_normal(
        &self,
        k: &Kernel,
        uv: Point2,
        u_side: Side,
        v_side: Side,
    ) -> Result<Vec3> {
        match self.kind_with_nurb(k)? {
            // SmSphere::EvaluateNormal (the side flags are not used)
            SurfaceKind::Sphere(s) => s.evaluate_normal(uv),
            SurfaceKind::Cone(_)
            | SurfaceKind::Torus(_)
            | SurfaceKind::BSpline(_)
            | SurfaceKind::SurfOfRevolution(_)
            | SurfaceKind::SurfOfExtrusion(_)
            | SurfaceKind::Step(_) => base_evaluate_normal(k, self, uv, u_side, v_side),
            SurfaceKind::CurveBounded(s) => s.evaluate_normal(k, uv, u_side, v_side),
            SurfaceKind::SrfInVolume(s) => s.evaluate_normal(k, uv, u_side, v_side),
            SurfaceKind::Plane(p) => Ok(p.evaluate_normal()),
            SurfaceKind::OffsetSurface(s) => s.evaluate_normal(k, uv, u_side, v_side),
        }
    }

    /// Whether the surface is closed over `uv_domain` in direction `param`, and the
    /// continuity across the seam when `want_continuity` (`IsClosed`, virtual: a plane
    /// is never closed, a sphere runs its override; the others run [`base_is_closed`]).
    ///
    /// # Errors
    ///
    /// Those of [`base_is_closed`].
    pub fn is_closed(
        &self,
        k: &Kernel,
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
            // SmSphere::IsClosed (the tolerance is not used)
            SurfaceKind::Sphere(s) => {
                let (closed, continuity) = s.is_closed(uv_domain, param);
                Ok((closed, want_continuity.then_some(continuity)))
            }

            SurfaceKind::Cone(_)
            | SurfaceKind::Torus(_)
            | SurfaceKind::BSpline(_)
            | SurfaceKind::CurveBounded(_)
            | SurfaceKind::SurfOfRevolution(_)
            | SurfaceKind::SurfOfExtrusion(_)
            | SurfaceKind::Step(_)
            | SurfaceKind::OffsetSurface(_)
            | SurfaceKind::SrfInVolume(_) => {
                base_is_closed(k, self, uv_domain, param, opt_tol, want_continuity)
            }
        }
    }

    /// Whether the surface is planar within `tol_3d`, with a plane point, unit normal
    /// and the largest deviation seen (`IsPlanar`, virtual): a plane always is; the
    /// others test the pseudo box of `CalculateBoundingBox` over the natural domain,
    /// not expanded by the zone tolerance.
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] from the bounding box (the base
    /// `SmSurface::CalculateBoundingBox` of a curve-bounded surface); `Unported` errors
    /// of an offset surface's bounding box (its other failures are ignored, as
    /// upstream).
    pub fn is_planar(&self, k: &Kernel, tol_3d: f64) -> Result<Planar> {
        // the unpadded box: the zone-tolerance padding made flat surfaces fail small
        // tolerances
        match self.kind_with_nurb(k)? {
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
                Ok(planar_of(k, s, &self.natural_uv_domain_with_nurb(k)?)?.is_planar(tol_3d))
            }
            // SmSurfOfRevolution (and SmCone, SmSphere, SmTorus) inherit
            // SmBSplineSurface::CalculateBoundingBox
            SurfaceKind::Cone(s) => {
                Ok(
                    planar_of(k, s.rev().base(), &self.natural_uv_domain_with_nurb(k)?)?
                        .is_planar(tol_3d),
                )
            }
            SurfaceKind::Sphere(s) => {
                Ok(
                    planar_of(k, s.base().base(), &self.natural_uv_domain_with_nurb(k)?)?
                        .is_planar(tol_3d),
                )
            }
            SurfaceKind::Torus(s) => {
                Ok(
                    planar_of(k, s.base().base(), &self.natural_uv_domain_with_nurb(k)?)?
                        .is_planar(tol_3d),
                )
            }
            SurfaceKind::SurfOfRevolution(s) => {
                Ok(
                    planar_of(k, s.base(), &self.natural_uv_domain_with_nurb(k)?)?
                        .is_planar(tol_3d),
                )
            }
            // (and SmSurfOfExtrusion)
            // (`CalculateBoundingBox`'s `GetOrCreateGwNurbPointer` makes a recognised
            // extrusion's NURB)
            SurfaceKind::SurfOfExtrusion(s) => {
                let base = s.base_with_nurb(k)?;
                Ok(planar_of(k, &base, &self.natural_uv_domain_with_nurb(k)?)?.is_planar(tol_3d))
            }
            // SmSurface body: use the virtual box and ignore ordinary failures,
            // which leave the default pseudo box. The kernel is available here.
            SurfaceKind::OffsetSurface(_)
            | SurfaceKind::CurveBounded(_)
            | SurfaceKind::Step(_)
            | SurfaceKind::SrfInVolume(_) => {
                let pseudo = ignored(self.calculate_bounding_box(
                    k,
                    &self.natural_uv_domain_with_nurb(k)?,
                    [false, true, false],
                    false,
                ))?;
                // with the opt-in fix a surface that cannot be boxed is not shown planar
                // (tangency-labs/smlib 21dcf69)
                let not_shown = pseudo.is_none() && k.fixes.query_statuses_pass_on;
                let pseudo = pseudo
                    .and_then(|b| b.pseudo_box)
                    .unwrap_or_else(PseudoBox::new);
                let mut planar = pseudo.is_planar(tol_3d);
                if not_shown {
                    planar.is_planar = false;
                }
                Ok(planar)
            }
        }
    }

    /// The B-rep of the face this surface belongs to (`GetBrep`): `None` for a surface
    /// no face owns.
    ///
    /// # Errors
    ///
    /// Those of [`Surface::face`]; [`SolidError::Unported`] for a surface with a face
    /// (`SmFace::GetBrep`).
    pub fn brep(&self) -> Result<Option<BrepId>> {
        if self.face()?.is_none() {
            return Ok(None);
        }
        // UNPORTED(topology.face_p4): SmFace::GetBrep
        // Proof (unreachable): no library caller; a surface's owner is an `ObjectRef`
        // key, which cannot reach the face's arena. Code holding the face holds its
        // `Brep` (the arena is the B-rep), so it never asks the surface.
        Err(SolidError::unported("SmFace::GetBrep", "topology.face_p4"))
    }

    /// The flat corners of the natural domain within `angle_tol_deg` and, when
    /// `want_points`, their positions (`GetFlatCorners`; upstream's default angle is
    /// `SM_EFF_ZERO_DEG`).
    ///
    /// # Errors
    ///
    /// Those of [`Surface::is_flat_corner`].
    pub fn flat_corners(
        &self,
        k: &Kernel,
        angle_tol_deg: f64,
        want_points: bool,
    ) -> Result<FlatCorners> {
        // init output
        let mut points = want_points.then_some([Vec3::UNDEF; 4]);

        // locals
        let d = self.natural_uv_domain_with_nurb(k)?;
        let corners = [
            Point2::new(d.u_min(), d.v_min()),
            Point2::new(d.u_min(), d.v_max()),
            Point2::new(d.u_max(), d.v_min()),
            Point2::new(d.u_max(), d.v_max()),
        ];

        // check each corner
        let mut rtn = 0u32;
        for (c, bit) in corners.iter().zip(FLAT_CORNER_BITS) {
            if self.is_flat_corner(k, *c, angle_tol_deg)? {
                rtn |= bit;
            }
        }

        // optional output (upstream masks the return value while testing: `if (lRtn &=
        // bit)`; the opt-in `Kernel::fixes.flat_corners_test_bits` tests each bit, WR-384)
        let test_bits = k.fixes.flat_corners_test_bits;
        if let Some(pts) = points.as_mut() {
            for ((c, bit), p) in corners.iter().zip(FLAT_CORNER_BITS).zip(pts.iter_mut()) {
                let flat = if test_bits {
                    rtn & bit
                } else {
                    rtn &= bit;
                    rtn
                };
                if flat != 0 {
                    if let Some(v) = ignored(self.evaluate_point(k, *c))? {
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
    /// In the near-equal starting-guess tie-break, upstream compares the `[0, 1]`
    /// sample fractions `dThisParam` and `dNearBestParam` directly against the input
    /// singular parameter `dS`; with
    /// `Kernel::fixes.degen_param_tiebreak_uses_actual_param` (WR-57) it compares the
    /// mapped singular parameters in `s_ivl` instead.
    ///
    /// # Errors
    ///
    /// The status of `Unitize` for a zero `dir`; `SM_ERR` when every sampled tangent
    /// points away from `dir`; those of `Evaluate`; [`SolidError::Unported`] for the
    /// Newton solve (`SmFindSingularParamWithGivenTangentDirEFO`,
    /// `surface.surface_p1`).
    pub fn find_degen_param_for_direction(
        &self,
        k: &Kernel,
        uv: Point2,
        dir: Vec3,
        dir_from_pole: bool,
        singular_dir: SurfParamType,
        tol: f64,
    ) -> Result<DegenParam> {
        // copy and unitize direction to match
        let mut s_dir = dir.unitize()?;

        // locals
        let domain = self.natural_uv_domain_with_nurb(k)?;
        let uv_min = domain.min();
        let uv_max = domain.max();
        let mut s_ivl = Extent1d::new();
        let mut t_ivl = Extent1d::new();
        let singular_u = singular_dir == SurfParamType::U;
        // (the statuses SMLib drops here are passed on with the opt-in fix, WR-663,
        // tangency-labs/smlib 21dcf69)
        let pass = k.fixes.query_statuses_pass_on;
        let (ds, dt) = if singular_u {
            ignore_or_pass_on(s_ivl.set_min_max(uv_min.x, uv_max.x), pass)?;
            ignore_or_pass_on(t_ivl.set_min_max(uv_min.y, uv_max.y), pass)?;
            (uv.x, uv.y)
        } else {
            ignore_or_pass_on(s_ivl.set_min_max(uv_min.y, uv_max.y), pass)?;
            ignore_or_pass_on(t_ivl.set_min_max(uv_min.x, uv_max.x), pass)?;
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
            let m = self.evaluate(k, test_uv, 1, 1, flags(Side::Left, Side::Left, true))?;

            // save the nonSingular Unitized tangent
            let surf_dt = if singular_u {
                at(&m, 0, 1)?
            } else {
                at(&m, 1, 0)?
            };
            let surf_dt = if pass {
                surf_dt.unitize()?
            } else {
                surf_dt.unitize_or_z()
            };

            // measure how closely test point dir is to target dir
            let this_f = surf_dt.dot(s_dir);
            // (upstream's `AngleBetween` is for debugging only; its status is passed on
            // with the opt-in fix)
            if pass {
                surf_dt.angle_between(s_dir)?;
            }

            // save the best sample point
            if this_f > best_f {
                best_f = this_f;
                best_param = this_param;
            }

            // for nearly equal starting guesses - save the one nearest the input value
            // (WR-57: pinned C++ compares the [0, 1] fraction directly against `ds`)
            let (cand_s, near_best_s) = if k.fixes.degen_param_tiebreak_uses_actual_param {
                (s_ivl.evaluate(this_param), s_ivl.evaluate(near_best_param))
            } else {
                (this_param, near_best_param)
            };
            if this_f > near_best_f - f_tol
                && (this_f > near_best_f + f_tol || (cand_s - ds).abs() < (near_best_s - ds).abs())
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
            k,
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
            if let Some((f, t)) = ignored_or_pass_on(
                singular_param_solve(
                    k,
                    self,
                    uv,
                    s_dir,
                    singular_dir,
                    &s_ivl,
                    s_ivl.evaluate(best_param),
                    tol,
                ),
                pass,
            )? {
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
    /// cache's decomposition tree). `surface` is this surface's handle in the
    /// solutions' objects and the cache's owner.
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` for `AtDistance` without a target distance, or when the
    /// surface cache or its tree cannot be built; those of the solve
    /// ([`SpGlobalSolver::local_solve_branch`]).
    #[allow(clippy::too_many_arguments)]
    pub fn global_point_solve_0<O: SolutionObject>(
        &self,
        k: &Kernel,
        surface: O,
        uv_domain: &Extent2d,
        op: SolverOperation,
        test_point: Point3,
        distance_tolerance: f64,
        opt_target_distance: Option<f64>,
        requested: SolutionRequested,
    ) -> Result<SolutionArray<O>> {
        self.global_point_solve_0_in(
            k,
            surface,
            None,
            None,
            false,
            uv_domain,
            op,
            test_point,
            distance_tolerance,
            opt_target_distance,
            requested,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn global_point_solve_0_in<O: SolutionObject>(
        &self,
        k: &Kernel,
        surface: O,
        cache: Option<&SurfaceCache<O>>,
        mut context: Option<&mut dyn SurfaceSolveContext<O>>,
        point_test_off: bool,
        uv_domain: &Extent2d,
        op: SolverOperation,
        test_point: Point3,
        distance_tolerance: f64,
        opt_target_distance: Option<f64>,
        requested: SolutionRequested,
    ) -> Result<SolutionArray<O>> {
        // get surface spatial decomposition tree from Surface's cache
        let mut owned;
        let cache = if let Some(c) = cache {
            if point_test_off {
                owned = c.clone();
                owned.point_test_enabled = false;
                &owned
            } else {
                c
            }
        } else {
            owned = match context.as_deref_mut() {
                Some(ctx) => solve_surface_cache_with(k, self, Some(surface), ctx)?,
                None => self.solve_cache(k, surface, OP_SOLVE)?,
            };
            if point_test_off {
                owned.point_test_enabled = false;
            }
            &owned
        };
        let surface_tree = cache.tree().ok_or(SolidError::null_pointer(OP_SOLVE))?;

        // put point into a single node PointTree
        let point_tree: Tree<O> = Tree::from_point(test_point, 0.0, false);

        // instance global Surface/Point solver
        let mut gs = SpGlobalSolver::new(k, &point_tree, surface_tree, Some(self), *uv_domain);
        gs.surface_cache = Some(cache);
        if let Some(ctx) = context.as_deref() {
            gs.face_queries = ctx.face_queries();
        }

        // init solver.dBestAnswerSoFarSq to properly handle SM_SR_SINGLE requests
        let best = point_solve_best_answer_sq(op, opt_target_distance)?;
        if op == SolverOperation::AtDistance {
            if let Some(t) = opt_target_distance {
                gs.base.set_at_distance(t);
            }
        }

        // solve it
        let mut solutions = SolutionArray::new();
        gs.solve_it(
            &SolveRequest {
                operation: op,
                solution_requested: requested,
                tolerance_3d: distance_tolerance,
                best_answer_so_far_sq: best,
                opt_vectors: &[],
            },
            &mut solutions,
        )?;
        Ok(solutions)
    }

    /// The global curve/surface solve over `uv_domain` and the curve's `interval`
    /// (`GlobalCurveSolve`: minimize, maximize, signed directed minimize, normalize):
    /// the solutions `[t, u, v]`, naming `handles` (the curve's, this surface's: the
    /// owners of their cache trees; upstream `&crCurve`, `this`).
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` when a cache cannot be built or has no tree, or for
    /// [`SolverOperation::AtDistance`] without a target; those of the solve (`SER`):
    /// the leaf solve of unit `surface.surface_p11`
    /// ([`super::p1::CsGlobalSolver::local_solve_branch`]).
    pub fn global_curve_solve<O: SolutionObject>(
        &self,
        k: &Kernel,
        uv_domain: &Extent2d,
        curve: &Curve,
        interval: &Extent1d,
        op: SolverOperation,
        distance_tolerance: f64,
        opt_target_distance: Option<f64>,
        opt_vectors: &[Vec3],
        requested: SolutionRequested,
        handles: [Option<O>; 2],
    ) -> Result<SolutionArray<O>> {
        self.global_curve_solve_in(
            k,
            uv_domain,
            curve,
            interval,
            op,
            distance_tolerance,
            opt_target_distance,
            opt_vectors,
            requested,
            handles,
            None,
        )
    }

    /// [`Surface::global_curve_solve`] of a face's surface by a caller that holds the
    /// face: `context` makes the surface's cache (`smsurf_GetSurfaceCache`, the face's
    /// trimmed cache) from the surface's handle, and its face queries serve the leaf
    /// solve's trim tests.
    ///
    /// # Errors
    ///
    /// As [`Surface::global_curve_solve`], and those of the context's cache
    /// preparation.
    pub fn global_curve_solve_with_context<O: SolutionObject>(
        &self,
        k: &Kernel,
        uv_domain: &Extent2d,
        curve: &Curve,
        interval: &Extent1d,
        op: SolverOperation,
        distance_tolerance: f64,
        opt_target_distance: Option<f64>,
        opt_vectors: &[Vec3],
        requested: SolutionRequested,
        handles: [Option<O>; 2],
        context: &mut dyn SurfaceSolveContext<O>,
    ) -> Result<SolutionArray<O>> {
        self.global_curve_solve_in(
            k,
            uv_domain,
            curve,
            interval,
            op,
            distance_tolerance,
            opt_target_distance,
            opt_vectors,
            requested,
            handles,
            Some(context),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn global_curve_solve_in<O: SolutionObject>(
        &self,
        k: &Kernel,
        uv_domain: &Extent2d,
        curve: &Curve,
        interval: &Extent1d,
        op: SolverOperation,
        distance_tolerance: f64,
        opt_target_distance: Option<f64>,
        opt_vectors: &[Vec3],
        requested: SolutionRequested,
        handles: [Option<O>; 2],
        mut context: Option<&mut dyn SurfaceSolveContext<O>>,
    ) -> Result<SolutionArray<O>> {
        // get target cache locals (NER: a cache that cannot be built or has no tree)
        let surface_cache = match context.as_deref_mut() {
            Some(context) => solve_surface_cache_with(k, self, handles[1], context)?,
            None => solve_surface_cache(k, self, handles[1])?,
        };
        let surface_tree = surface_cache
            .tree()
            .ok_or(SolidError::null_pointer(OP_SOLVE))?;
        let curve_cache = match crate::curve::curve::p5::make_curve_cache::<O>(k, curve, handles[0])
        {
            Ok(c) => c,
            Err(e) if e.is_unported() || matches!(e, SolidError::PreparationRequired { .. }) => {
                return Err(e);
            }
            Err(_) => return Err(SolidError::null_pointer(OP_SOLVE)),
        };
        let curve_tree = curve_cache
            .tree()
            .ok_or(SolidError::null_pointer(OP_SOLVE))?;

        // instance the solver (the leaf solve reads the curve and the surface cache)
        let mut gs = super::p1::CsGlobalSolver::new(
            k,
            curve_tree,
            surface_tree,
            Some(self),
            *interval,
            *uv_domain,
        );
        gs.curve = Some(curve);
        gs.surface_cache = Some(&surface_cache);
        if let Some(context) = context.as_deref() {
            gs.face_queries = context.face_queries();
        }
        let domain = self.natural_uv_domain_with_nurb(k)?;
        gs.operands = Some(SolveOperands {
            operands: [
                handles[0].map(|h| (h, OperandDomain::Curve(curve.natural_interval()))),
                handles[1].map(|h| (h, OperandDomain::Surface(domain))),
            ],
        });

        let best = pair_solve_best_answer_sq(op, opt_target_distance, false)?;
        if op == SolverOperation::AtDistance {
            if let Some(t) = opt_target_distance {
                gs.base.set_at_distance(t);
            }
        }

        // pass call along to GlobalSolver (SER: checked)
        let mut solutions = SolutionArray::new();
        gs.solve_it(
            &SolveRequest {
                operation: op,
                solution_requested: requested,
                tolerance_3d: distance_tolerance,
                best_answer_so_far_sq: best,
                opt_vectors,
            },
            &mut solutions,
        )?;
        Ok(solutions)
    }

    /// The global surface/surface solve (`GlobalSurfaceSolve`: minimize, maximize,
    /// normalize, signed directed minimize/maximize, and intersect for nodes): the
    /// solutions `[u, v, other u, other v]` (for intersect, the node pairs), naming
    /// `handles` (this surface's and the other's: the owners of their cache trees).
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` when a cache cannot be built or has no tree, or for
    /// [`SolverOperation::AtDistance`] without a target; those of the solve (`SER`):
    /// the leaf solve of unit `surface.surface_p11`
    /// ([`SsGlobalSolver::local_solve_branch`]).
    pub fn global_surface_solve<O: SolutionObject>(
        &self,
        k: &Kernel,
        uv_domain: &Extent2d,
        other: &Surface,
        other_uv_domain: &Extent2d,
        op: SolverOperation,
        distance_tolerance: f64,
        opt_target_distance: Option<f64>,
        opt_vectors: &[Vec3],
        requested: SolutionRequested,
        handles: [Option<O>; 2],
    ) -> Result<SolutionArray<O>> {
        Ok(self
            .global_surface_solve_with(
                k,
                uv_domain,
                other,
                other_uv_domain,
                op,
                distance_tolerance,
                opt_target_distance,
                opt_vectors,
                requested,
                handles,
            )?
            .0)
    }

    /// [`Surface::global_surface_solve`], also returning for every solution the boxes
    /// and patch UV domains of its two tree nodes (`m_apNodes[i]->m_sBBox`,
    /// `GetUVDomain()` of their `SmBezierPatch`es; what `SM_SR_NODES` callers read).
    ///
    /// # Errors
    ///
    /// As [`Surface::global_surface_solve`]; `InvalidInput` for a solution node that
    /// is not a patch of its cache.
    pub fn global_surface_solve_with<O: SolutionObject>(
        &self,
        k: &Kernel,
        uv_domain: &Extent2d,
        other: &Surface,
        other_uv_domain: &Extent2d,
        op: SolverOperation,
        distance_tolerance: f64,
        opt_target_distance: Option<f64>,
        opt_vectors: &[Vec3],
        requested: SolutionRequested,
        handles: [Option<O>; 2],
    ) -> Result<(SolutionArray<O>, Vec<SolutionNodes>)> {
        self.global_surface_solve_in(
            k,
            uv_domain,
            other,
            other_uv_domain,
            op,
            distance_tolerance,
            opt_target_distance,
            opt_vectors,
            requested,
            handles,
            None,
        )
    }

    /// [`Surface::global_surface_solve`] of face surfaces by a caller that holds the
    /// faces: `context` makes each surface's cache (`smsurf_GetSurfaceCache`, the
    /// face's trimmed cache) from its handle, and its face queries serve the leaf
    /// solve's trim tests.
    ///
    /// # Errors
    ///
    /// As [`Surface::global_surface_solve`], and those of the context's cache
    /// preparation.
    pub fn global_surface_solve_with_context<O: SolutionObject>(
        &self,
        k: &Kernel,
        uv_domain: &Extent2d,
        other: &Surface,
        other_uv_domain: &Extent2d,
        op: SolverOperation,
        distance_tolerance: f64,
        opt_target_distance: Option<f64>,
        opt_vectors: &[Vec3],
        requested: SolutionRequested,
        handles: [Option<O>; 2],
        context: &mut dyn SurfaceSolveContext<O>,
    ) -> Result<SolutionArray<O>> {
        Ok(self
            .global_surface_solve_in(
                k,
                uv_domain,
                other,
                other_uv_domain,
                op,
                distance_tolerance,
                opt_target_distance,
                opt_vectors,
                requested,
                handles,
                Some(context),
            )?
            .0)
    }

    #[allow(clippy::too_many_arguments)]
    fn global_surface_solve_in<O: SolutionObject>(
        &self,
        k: &Kernel,
        uv_domain: &Extent2d,
        other: &Surface,
        other_uv_domain: &Extent2d,
        op: SolverOperation,
        distance_tolerance: f64,
        opt_target_distance: Option<f64>,
        opt_vectors: &[Vec3],
        requested: SolutionRequested,
        handles: [Option<O>; 2],
        mut context: Option<&mut dyn SurfaceSolveContext<O>>,
    ) -> Result<(SolutionArray<O>, Vec<SolutionNodes>)> {
        // both surface caches and trees (NER)
        let (cache1, cache2) = match context.as_deref_mut() {
            Some(context) => {
                let cache1 = solve_surface_cache_with(k, self, handles[0], context)?;
                (
                    cache1,
                    solve_surface_cache_with(k, other, handles[1], context)?,
                )
            }
            None => (
                solve_surface_cache(k, self, handles[0])?,
                solve_surface_cache(k, other, handles[1])?,
            ),
        };
        let tree1 = cache1.tree().ok_or(SolidError::null_pointer(OP_SOLVE))?;
        let tree2 = cache2.tree().ok_or(SolidError::null_pointer(OP_SOLVE))?;

        let mut gs = SsGlobalSolver::new(
            tree1,
            tree2,
            [Some(self), Some(other)],
            *uv_domain,
            *other_uv_domain,
            k,
        );
        gs.caches = [Some(&cache1), Some(&cache2)];
        if let Some(context) = context.as_deref() {
            gs.face_queries = context.face_queries();
        }
        let (domain, other_domain) = (
            self.natural_uv_domain_with_nurb(k)?,
            other.natural_uv_domain_with_nurb(k)?,
        );
        gs.operands = Some(SolveOperands {
            operands: [
                handles[0].map(|h| (h, OperandDomain::Surface(domain))),
                handles[1].map(|h| (h, OperandDomain::Surface(other_domain))),
            ],
        });

        let best = pair_solve_best_answer_sq(op, opt_target_distance, true)?;
        if op == SolverOperation::AtDistance {
            if let Some(t) = opt_target_distance {
                gs.base.set_at_distance(t);
            }
        }

        // (SER: checked)
        let mut solutions = SolutionArray::new();
        gs.solve_it(
            &SolveRequest {
                operation: op,
                solution_requested: requested,
                tolerance_3d: distance_tolerance,
                best_answer_so_far_sq: best,
                opt_vectors,
            },
            &mut solutions,
        )?;

        let mut nodes = Vec::new();
        for s in solutions.as_slice() {
            let mut bboxes = [crate::geom::extent3d::Extent3d::new(); 2];
            let mut uv_domains = [Extent2d::new(); 2];
            for (i, (tree, cache)) in [(tree1, &cache1), (tree2, &cache2)].into_iter().enumerate() {
                if let (Some(id), Some(b), Some(d)) = (
                    s.nodes.get(i).copied().flatten(),
                    bboxes.get_mut(i),
                    uv_domains.get_mut(i),
                ) {
                    *b = tree.node(id)?.bbox;
                    *d = cache.patch(id)?.aux.uv_domain;
                }
            }
            nodes.push(SolutionNodes { bboxes, uv_domains });
        }
        Ok((solutions, nodes))
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
        k: &Kernel,
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
            k,
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
        let p1 = curve.evaluate_point(k, s0)?;
        let uv = Point2::new(s1, s2);
        let p2 = self.evaluate_point(k, uv)?;
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
    /// intersection with the curve's linear sweep along the unit projection vector
    /// times the projection distance (given, else four times the largest distance
    /// between the curve's and the surface's boxes, the sweep then moved back by half
    /// to run both ways), through `SmAdvSurfaceIntersector::DoIntersection`. Returns
    /// the 3D curves and their UV curves on this surface (`None` for a `NULL` entry).
    /// The surface and curve continuity checks only report. `curve` is one of the
    /// `SmBSplineCurve` classes (B-spline, line, ellipse, hyperbola, parabola): its
    /// continuities, box and natural interval dispatch as upstream's virtuals, and the
    /// sweep uses its NURB.
    ///
    /// # Errors
    ///
    /// `SM_ERR_INVALID_INPUT` for a zero projection vector, a supplied approximation
    /// tolerance that is not finite, positive and normal (HANG-32), or a curve of
    /// another class; `SM_ERR_NULL_POINTER` without a surface cache; those of the
    /// continuities, the boxes, the sweep, its move and the intersection;
    /// [`SolidError::Unported`] from unported callees of the surface cache and the
    /// intersector.
    pub fn create_parallel_projection_curves(
        &self,
        k: &Kernel,
        uv_domain: &Extent2d,
        curve: &Curve,
        projection_vector: Vec3,
        opt_projection_distance: Option<f64>,
        opt_approx_tol_3d: Option<f64>,
        opt_angle_tol_rad: Option<f64>,
    ) -> Result<(Vec<Curve>, Vec<Option<Curve>>)> {
        self.create_parallel_projection_curves_in::<super::p4::NoObject>(
            k,
            uv_domain,
            curve,
            projection_vector,
            opt_projection_distance,
            opt_approx_tol_3d,
            opt_angle_tol_rad,
            None,
        )
    }

    /// [`Surface::create_parallel_projection_curves`] of a face's surface by a caller
    /// that holds the face: `context` makes this surface's cache from `handle`
    /// (`smsurf_GetSurfaceCache`, the face's trimmed cache) for the continuity check
    /// and the intersection, and the swept surface's (no handle) as its own.
    ///
    /// # Errors
    ///
    /// As [`Surface::create_parallel_projection_curves`], and those of the context's
    /// cache preparation.
    #[allow(clippy::too_many_arguments)]
    pub fn create_parallel_projection_curves_with_context<O: SolutionObject>(
        &self,
        k: &Kernel,
        uv_domain: &Extent2d,
        curve: &Curve,
        projection_vector: Vec3,
        opt_projection_distance: Option<f64>,
        opt_approx_tol_3d: Option<f64>,
        opt_angle_tol_rad: Option<f64>,
        context: &mut dyn SurfaceSolveContext<O>,
        handle: Option<O>,
    ) -> Result<(Vec<Curve>, Vec<Option<Curve>>)> {
        self.create_parallel_projection_curves_in(
            k,
            uv_domain,
            curve,
            projection_vector,
            opt_projection_distance,
            opt_approx_tol_3d,
            opt_angle_tol_rad,
            Some((context, handle)),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn create_parallel_projection_curves_in<O: SolutionObject>(
        &self,
        k: &Kernel,
        uv_domain: &Extent2d,
        curve: &Curve,
        projection_vector: Vec3,
        opt_projection_distance: Option<f64>,
        opt_approx_tol_3d: Option<f64>,
        opt_angle_tol_rad: Option<f64>,
        mut context: Option<(&mut dyn SurfaceSolveContext<O>, Option<O>)>,
    ) -> Result<(Vec<Curve>, Vec<Option<Curve>>)> {
        // check input (ZERO_VEC_ER: fails unless |v|^2 >= SM_EFF_ZERO_SQ, so also for NaN)
        if projection_vector
            .length_squared()
            .partial_cmp(&EFF_ZERO_SQ)
            .is_none_or(core::cmp::Ordering::is_lt)
        {
            return Err(SolidError::invalid_input(OP_PROJ, "zero projection vector"));
        }

        // check input - a supplied approximation tolerance must be finite, positive and
        // normal (HANG-32): a subnormal one spins the C++ projector, and the FFI already
        // refuses it. Done before any cache or geometry preparation; `None` is unchanged.
        if opt_approx_tol_3d.is_some_and(|t| !(t.is_finite() && t >= f64::MIN_POSITIVE)) {
            return Err(SolidError::invalid_input(
                OP_PROJ,
                "approximation tolerance must be finite, positive and normal (HANG-32)",
            ));
        }

        // check state - internal surface continuities are better than C0 (NER; the
        // continuity is only reported)
        let ner = |e: SolidError| {
            if e.is_unported() {
                e
            } else {
                SolidError::null_pointer(OP_PROJ)
            }
        };
        match context.as_mut() {
            Some((context, handle)) => context
                .build_surface_cache(k, self, *handle)
                .map_err(ner)?
                .surface_continuity_with_kernel(k, uv_domain)?,
            None => super::p2::surface_cache::<()>(k, self, None)
                .map_err(ner)?
                .surface_continuity_with_kernel(k, uv_domain)?,
        };

        // check state - internal curve continuities are better than C0 (only reported)
        curve.calculate_continuities(k, CONTINUITY_ANGLE)?;

        // Select projection distance to be greater than max dist between curve and
        // surface
        let projection_distance = if let Some(d) = opt_projection_distance {
            d
        } else {
            let surf_box = self
                .calculate_bounding_box(k, uv_domain, [true, false, false], true)?
                .normal_box
                .unwrap_or_default();
            let req = crate::nurbs::b_spline_curve::p2::BoundingBoxRequest {
                normal: true,
                pseudo: false,
                polar: false,
            };
            let curve_box = curve
                .calculate_bounding_box(k, &curve.natural_interval(), req, true)?
                .normal
                .unwrap_or_default();
            4.0 * curve_box.max_distance(&surf_box)
        };

        // sweep vector
        let sweep_vec = projection_vector.unitize()? * projection_distance;

        // swept surface (`const SmBSplineCurve&`: the class's NURB)
        // (the analytic classes' own NURB, swept exactly with no knot fix-up; when it is
        // missing, their virtual MakeNurb makes it on a copy)
        let mut sweep_curve: BSplineCurve = match curve.kind() {
            CurveKind::BSpline(c) => c.clone_exact(),
            CurveKind::Line(c) => c.base_with_nurb()?.clone_exact(),
            CurveKind::Ellipse(c) if c.base().gw_nurb().is_some() => c.base().clone_exact(),
            CurveKind::Ellipse(c) => {
                let mut c = c.clone();
                ignore_status(c.make_nurb(k))?;
                c.base().clone_exact()
            }
            CurveKind::Hyperbola(c) if c.base().gw_nurb().is_some() => c.base().clone_exact(),
            CurveKind::Hyperbola(c) => {
                let mut c = c.clone();
                ignore_status(c.make_nurb(None))?;
                c.base().clone_exact()
            }
            CurveKind::Parabola(c) if c.base().gw_nurb().is_some() => c.base().clone_exact(),
            CurveKind::Parabola(c) => {
                let mut c = c.clone();
                ignore_status(c.make_nurb(None))?;
                c.base().clone_exact()
            }
            _ => return Err(SolidError::invalid_input(OP_PROJ, "not an SmBSplineCurve")),
        };
        let mut projection_surface = Surface::from(BSplineSurface::create_linear_sweep(
            &mut sweep_curve,
            sweep_vec,
        )?);

        // when working with guessed Projection distance - move the swept surface so that
        // it sweeps forward and backwards from its start point
        if opt_projection_distance.is_none() {
            let mut transform = crate::geom::axis2_placement::Axis2Placement::default();
            transform.translate(sweep_vec * -0.5);
            projection_surface.transform(k, &transform, None)?;
        }

        // intersection
        let projection_domain = projection_surface.natural_uv_domain_with_nurb(k)?;
        let x = match context {
            Some((context, handle)) => {
                crate::intersect::surface_intersector::p5::do_intersection_with_context(
                    k,
                    self,
                    uv_domain,
                    &projection_surface,
                    &projection_domain,
                    [true, true],
                    opt_approx_tol_3d,
                    opt_angle_tol_rad,
                    context,
                    [handle, None],
                )?
            }
            None => crate::intersect::surface_intersector::p5::do_intersection(
                k,
                self,
                uv_domain,
                &projection_surface,
                &projection_domain,
                [true, true],
                opt_approx_tol_3d,
                opt_angle_tol_rad,
            )?,
        };
        Ok((x.curves_3d, x.surface1_uv_curves))
    }
}

/// `SmSurface::IsPlanar(tol_3d)` of a B-spline surface (the base body over the
/// `SmBSplineSurface::CalculateBoundingBox` override).
///
/// # Errors
///
/// [`SolidError::Unported`] from the bounding box.
pub fn b_spline_is_planar(k: &Kernel, s: &BSplineSurface, tol_3d: f64) -> Result<Planar> {
    Ok(planar_of(k, s, &s.natural_uv_domain())?.is_planar(tol_3d))
}

/// The pseudo box of a B-spline surface over `uv_domain`, not expanded by the zone
/// tolerance; a failed box is the default box, as upstream ignores the status.
fn planar_of(k: &Kernel, s: &BSplineSurface, uv_domain: &Extent2d) -> Result<PseudoBox> {
    let req = BoundingBoxRequest {
        normal: false,
        pseudo: true,
        polar: false,
        pseudo_guess: None,
        polar_guess: None,
        expand_by_zone_tol: false,
        expand_tolerance: None,
    };
    Ok(ignored(s.calculate_bounding_box(k, uv_domain, &req))?
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

/// The tree nodes of one solution of [`Surface::global_surface_solve_with`]: their
/// boxes and the UV domains of their Bezier patches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolutionNodes {
    /// `m_apNodes[0]->m_sBBox`, `m_apNodes[1]->m_sBBox` (empty for a missing node).
    pub bboxes: [crate::geom::extent3d::Extent3d; 2],
    /// The nodes' `SmBezierPatch::GetUVDomain()` (empty for a missing node).
    pub uv_domains: [Extent2d; 2],
}

/// `smsurf_GetSurfaceCache(surface)` and its `GetTree()` for a global solve (`NER`:
/// a cache that cannot be built or has no tree is `SM_ERR_NULL_POINTER`). The cache
/// is built for the solve (no cache store: unit `surface.surface_p1`'s
/// `CacheMakeOrValidate`); its contents are those of upstream's cached one.
fn solve_surface_cache<O: SolutionObject>(
    k: &Kernel,
    surface: &Surface,
    handle: Option<O>,
) -> Result<SurfaceCache<O>> {
    let mut cache = match super::p1::get_surface_cache(k, surface, handle) {
        Ok(c) => c,
        Err(e) if e.is_unported() => return Err(e),
        Err(_) => return Err(SolidError::null_pointer(OP_SOLVE)),
    };
    match cache.get_tree(k) {
        Ok(_) => Ok(cache),
        Err(e) if e.is_unported() => Err(e),
        Err(_) => Err(SolidError::null_pointer(OP_SOLVE)),
    }
}

/// [`solve_surface_cache`] through the caller's owning-face context: the context
/// makes the cache from the surface's handle (`smsurf_GetSurfaceCache` reaching the
/// face), with the same `NER` mapping of a cache or tree that cannot be built.
fn solve_surface_cache_with<O: SolutionObject>(
    k: &Kernel,
    surface: &Surface,
    handle: Option<O>,
    context: &mut dyn SurfaceSolveContext<O>,
) -> Result<SurfaceCache<O>> {
    let mut cache = match context.build_surface_cache(k, surface, handle) {
        Ok(c) => c,
        Err(e) if e.is_unported() || matches!(e, SolidError::PreparationRequired { .. }) => {
            return Err(e);
        }
        Err(_) => return Err(SolidError::null_pointer(OP_SOLVE)),
    };
    match cache.get_tree(k) {
        Ok(_) => Ok(cache),
        Err(e) if e.is_unported() || matches!(e, SolidError::PreparationRequired { .. }) => Err(e),
        Err(_) => Err(SolidError::null_pointer(OP_SOLVE)),
    }
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
    k: &Kernel,
    surface: &S,
    uv_domain: &Extent2d,
) -> Result<Vec2> {
    // Method: simple, just evaluate on a grid and average.
    const NUM_SAMPLES: usize = 4;
    let samples = [0.15, 0.35, 0.65, 0.85];
    let mut mag_u = 0.0;
    let mut mag_v = 0.0;
    let mut d = (Vec3::UNDEF, Vec3::UNDEF, Vec3::UNDEF);
    // with the opt-in fix the samples that evaluate are averaged, `SM_UNDEF_DOUBLE` when
    // none does (WR-663, tangency-labs/smlib 21dcf69)
    let only_evaluated = k.fixes.query_statuses_pass_on;
    let mut num_evaluated: u32 = 0;

    for &si in &samples {
        for &sj in &samples {
            let uv = uv_domain.evaluate(si, sj);
            let evaluated = eval_1st_keep(k, surface, uv, Side::Left, Side::Left, &mut d)?;
            if only_evaluated && !evaluated {
                continue;
            }
            mag_u += d.1.length();
            mag_v += d.2.length();
            num_evaluated += 1;
        }
    }

    if only_evaluated {
        if num_evaluated == 0 {
            return Ok(Vec2::new(UNDEF_DOUBLE, UNDEF_DOUBLE));
        }
        mag_u /= f64::from(num_evaluated);
        mag_v /= f64::from(num_evaluated);
        return Ok(Vec2::new(mag_u, mag_v));
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
/// Those of `Evaluate1stDerivatives` and of the natural domain (a lazy analytic
/// surface makes its NURB first).
pub fn compute_higher_order_derivs<S: SurfaceEvaluate + ?Sized>(
    k: &Kernel,
    surface: &S,
    uv: Point2,
    g1: Vec2,
    want_g3: bool,
    want_g4: bool,
) -> Result<HigherOrderDerivs> {
    // locals
    let domain = surface.natural_uv_domain_with_nurb(k)?;

    // finite difference step - watch for stepping out of surface boundaries
    let mut step = EFF_ZERO_SQRT / 1000.0; // 1.0e-9
    let mut uv1 = uv + step * g1;
    if !domain.contains_point(uv1, 0.0) {
        step = -step;
        uv1 = uv + step * g1;
    }

    // compute 1st derivatives and first directional derivative at target point
    let (_, du, dv) = evaluate_1st_derivatives(k, surface, uv, Side::Left, Side::Left)?;
    let vec = du * g1.x + dv * g1.y;

    // compute 1st derivatives and first directional derivative at finite step point
    let (_, dud, dvd) = evaluate_1st_derivatives(k, surface, uv1, Side::Left, Side::Left)?;
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
        let (_, dud, dvd) = evaluate_1st_derivatives(k, surface, uv2, Side::Left, Side::Left)?;

        // finite difference approximation to 3rd order directional derivative
        let vec_d2 = dud * g1.x + dvd * g1.y;
        let delta_g1_2 = vec_d1 - vec_d2;
        let delta_g2 = delta_g1_2 - delta_g1;
        if want_g3 {
            out.g3 = Some(delta_g2 / step);
        }

        // get a 4th finite difference point
        let uv3 = uv2 + step * g1;
        let (_, dud, dvd) = evaluate_1st_derivatives(k, surface, uv3, Side::Left, Side::Left)?;

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
/// Those of `Evaluate1stDerivatives` and of the natural domain (a lazy analytic
/// surface makes its NURB first); the status of `Unitize` for a zero result.
pub fn compute_inward_vector<S: SurfaceEvaluate + ?Sized>(
    k: &Kernel,
    surface: &S,
    uv: Point2,
) -> Result<Vec3> {
    // Surface eval
    let (_, du, dv) = evaluate_1st_derivatives(k, surface, uv, Side::Left, Side::Left)?;

    // towards midPoint UV vec
    let mid_uv = surface.natural_uv_domain_with_nurb(k)?.evaluate(0.5, 0.5);
    let dir_uv = mid_uv - uv;
    // (passed on with the opt-in fix, WR-663, tangency-labs/smlib 21dcf69)
    let dir_uv = match passed_on(dir_uv.unitize(), k.fixes.query_statuses_pass_on)? {
        Ok(d) => d,
        Err(_) => dir_uv,
    };

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
    k: &Kernel,
    surface: &S,
    uv: Point2,
    u_side: Side,
    v_side: Side,
    points: &[Point3],
) -> Result<Vec<Point2>> {
    // get surface point and 1st derivs for input domain point.
    let (pnt, du, dv) = evaluate_1st_derivatives(k, surface, uv, u_side, v_side)?;

    // no work - a 1st deriv is zero - set output = input and quit
    if du.length_squared() < EFF_ZERO_SQ || dv.length_squared() < EFF_ZERO_SQ {
        return Ok(points.iter().map(|_| uv).collect());
    }

    let mut uv_step = Vec2::UNDEF;
    let mut out = Vec::new();
    for p in points {
        let diff = *p - pnt;
        // (passed on with the opt-in fix, WR-663, tangency-labs/smlib 21dcf69)
        if let Some(s) = ignored_or_pass_on(
            drop_vectors(du, dv, &[diff]),
            k.fixes.query_statuses_pass_on,
        )?
        .and_then(|v| v.first().copied())
        {
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
    k: &Kernel,
    surface: &S,
    uv: Point2,
    u_side: Side,
    v_side: Side,
    vectors: &[Vec3],
) -> Result<Vec<Vec2>> {
    let (_, du, dv) = evaluate_1st_derivatives(k, surface, uv, u_side, v_side)?;

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
    k: &Kernel,
    surface: &Surface,
    uv_domain: &Extent2d,
    test_pnt: Point3,
    op: SolverOperation,
) -> Result<Point2> {
    const GRID_SIZE: usize = 2;

    // set gridSize = 1 for linear geometry (an SmBSplineSurface: B-splines and planes)
    let is_bss = surface.is_kind_of(BSplineSurface::TYPE)?;
    let grid_u = if is_bss && surface.number_control_points(k, SurfParamType::U) == 2 {
        1
    } else {
        GRID_SIZE
    };
    let grid_v = if is_bss && surface.number_control_points(k, SurfParamType::V) == 2 {
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
        k,
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
    // (the point evaluation's status is passed on with the opt-in fix, WR-663,
    // tangency-labs/smlib 21dcf69)
    let pass = k.fixes.query_statuses_pass_on;
    if !ok {
        if let Some(p) = ignored_or_pass_on(surface.evaluate_point(k, close_uv), pass)? {
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
                k,
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
                k,
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
                if let Some(p) = ignored_or_pass_on(surface.evaluate_point(k, uv), pass)? {
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
    k: &Kernel,
    surface: &S,
    uv: Point2,
    curve: &Curve,
    parameter: f64,
) -> Result<bool> {
    // with the opt-in fix a point that cannot be evaluated is not shown converged
    // (WR-663, tangency-labs/smlib 21dcf69)
    let not_shown = k.fixes.query_statuses_pass_on;

    // surface eval
    let mut d = (Vec3::UNDEF, Vec3::UNDEF, Vec3::UNDEF);
    if !eval_1st_keep(k, surface, uv, Side::Left, Side::Left, &mut d)? && not_shown {
        return Ok(false);
    }
    let (pnt, mut du, mut dv) = d;

    // curve eval
    let mut pv = [Vec3::UNDEF, Vec3::UNDEF];
    match ignored(curve.evaluate(k, parameter, 1, true, true))? {
        Some(v) => {
            for (dst, src) in pv.iter_mut().zip(v) {
                *dst = src;
            }
        }
        None if not_shown => return Ok(false),
        None => (),
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
    k: &Kernel,
    surface: &S1,
    uv: Point2,
    surface2: &S2,
    uv2: Point2,
) -> Result<bool>
where
    S1: SurfaceEvaluate + ?Sized,
    S2: SurfaceEvaluate + ?Sized,
{
    // with the opt-in fix a point that cannot be evaluated is not shown converged
    // (WR-663, tangency-labs/smlib 21dcf69)
    let not_shown = k.fixes.query_statuses_pass_on;
    let mut d1 = (Vec3::UNDEF, Vec3::UNDEF, Vec3::UNDEF);
    if !eval_1st_keep(k, surface, uv, Side::Left, Side::Left, &mut d1)? && not_shown {
        return Ok(false);
    }
    let mut d2 = (Vec3::UNDEF, Vec3::UNDEF, Vec3::UNDEF);
    if !eval_1st_keep(k, surface2, uv2, Side::Left, Side::Left, &mut d2)? && not_shown {
        return Ok(false);
    }
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
    k: &Kernel,
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
    // (both statuses are passed on with the opt-in fix, WR-663, tangency-labs/smlib
    // 21dcf69)
    let pass = k.fixes.query_statuses_pass_on;
    let pt_der = ignored_or_pass_on(evaluate_step(k, this_surf, this_uv), pass)?
        .unwrap_or_else(|| vec![vec![Vec3::UNDEF; 2]; 2]);

    let mut d = (Vec3::UNDEF, Vec3::UNDEF, Vec3::UNDEF);
    if pass {
        d = evaluate_1st_derivatives(k, other_surf, other_uv, Side::Left, Side::Left)?;
    } else {
        eval_1st_keep(k, other_surf, other_uv, Side::Left, Side::Left, &mut d)?;
    }
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
/// Its last test, meant to refuse a result farther from the point than the guess,
/// measures the two distances the wrong way round, as upstream; with
/// `Kernel::fixes.good_drop_result_distances_in_order` (WR-56, the fork's
/// tangency-labs/smlib@0b4de8c / eaa998a) it measures them the right way, and takes a
/// result as the foot only when the gap's components along the unit tangents are
/// within the local solve's tight tolerance `EFF_ZERO * (1 + max|P|)` (upstream: their
/// sum under 1e-4, an interior result each under 1e-3).
///
/// # Errors
///
/// Only [`SolidError::Unported`] (upstream ignores the evaluation statuses).
pub fn good_drop_result<S: SurfaceEvaluate + ?Sized>(
    k: &Kernel,
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
    // (with the opt-in fix a drop that cannot be evaluated is not shown good, WR-663,
    // tangency-labs/smlib 21dcf69; the fork keeps the dropped `Unitize` statuses)
    let not_shown = k.fixes.query_statuses_pass_on;
    let mut d = (Vec3::UNDEF, Vec3::UNDEF, Vec3::UNDEF);
    if !eval_1st_keep(k, surface, result_uv, Side::Left, Side::Left, &mut d)? && not_shown {
        return Ok(false);
    }
    let (srf_point, su, sv) = d;
    let su = su.unitize_or_z();
    let sv = sv.unitize_or_z();
    let gap = pt_to_drop - srf_point;
    let fu = gap.dot(su);
    let fv = gap.dot(sv);
    // Upstream takes any foot 1e-4 off along the surface (an interior one 1e-3 off in
    // each direction). With the opt-in (tangency-labs/smlib@eaa998a, with the distance
    // swap below), a foot is one the local solve converged to by its own tight
    // tolerance, `LocalPointSolve_1`'s; a stalled solve goes to the global solve.
    let (foot_tol, interior_tol) = if k.fixes.good_drop_result_distances_in_order {
        let tight_tol = EFF_ZERO * (1.0 + pt_to_drop.max_dimension());
        (tight_tol, tight_tol)
    } else {
        (0.0001, 0.001)
    };
    if fu.abs() + fv.abs() < foot_tol {
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
    } else if fu.abs() > interior_tol {
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
    } else if fv.abs() > interior_tol {
        return Ok(false); // interior, bad fcn value.
    }

    // One more: if the solution 3d point is farther than the guess (upstream's names
    // are swapped).
    let mut guess_3d = Vec3::UNDEF;
    match ignored(surface.evaluate_point(k, guess_uv))? {
        Some(p) => guess_3d = p,
        None if not_shown => return Ok(false),
        None => (),
    }
    let (dsq_guess, dsq_result) = if k.fixes.good_drop_result_distances_in_order {
        // Not upstream (WR-56; tangency-labs/smlib@0b4de8c): each distance to its own point
        (
            pt_to_drop.distance_squared(guess_3d),
            pt_to_drop.distance_squared(srf_point),
        )
    } else {
        (
            pt_to_drop.distance_squared(srf_point),
            pt_to_drop.distance_squared(guess_3d),
        )
    };
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
    /// Caller context for virtual evaluations and the leaf solve (upstream reaches
    /// the context through the surface).
    pub kernel: &'s Kernel,
    /// The `SmGlobalSolver` state.
    pub base: GlobalSolver<'t, O>,
    /// `m_pSurf`.
    pub surface: Option<&'s Surface>,
    /// `m_crUVDomain`.
    pub uv_domain: Extent2d,
    /// The cache holding the patches of the surface tree's nodes, read by the leaf
    /// solve (upstream reaches it through the node's tree owner).
    pub surface_cache: Option<&'s SurfaceCache<O>>,
    /// Prepared face classification shared with other caches in this operation.
    pub face_queries: Option<&'s dyn crate::surface::surface_cache::face_context::FaceQueries<O>>,
}

impl<O: SolutionObject> SpGlobalSolver<'_, '_, O> {
    /// `BranchMayContainAnswers`: false for a surface node outside its face, and when
    /// normalizing, for a patch whose normal cone misses every direction to the point;
    /// else the default branch test.
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` without `surface_cache`; `InvalidInput` for a node that is
    /// not a patch node of that cache; those of the polar box build.
    pub fn may_contain_answers(&self, branch: &Branch) -> Result<bool> {
        let k = self.kernel;
        // When point testing is on and node is known to be outside of Surface->Face
        let surface_node = self.base.branch_node(branch, 1)?;
        let cache = self.surface_cache.ok_or(SolidError::null_pointer(OP_MAY))?;
        if !is_node_valid(self.base.tree(1)?.owner(), cache, surface_node)? {
            return Ok(false);
        }

        // If we are normalizing and have a node with a surface do the angular test
        // using the Gauss map of the normal.
        if self.base.operation == SolverOperation::Normalize {
            // When we have a child SurfacePatch node
            if surface_node.aux_type == AuxDataType::BezierSurface && surface_node.child1.is_some()
            {
                let surface_id = branch
                    .get(1)
                    .copied()
                    .flatten()
                    .ok_or(SolidError::null_pointer(OP_MAY))?;
                let patch = cache.patch(surface_id)?;

                // (the second sm_IsNodeValid test upstream repeats the first)

                // when the point is in the surfaceNode's pseudoBox
                let test_pnt = self.base.branch_node(branch, 0)?.bbox.min();
                if !patch.pseudo_box.contains_point(test_pnt, EFF_ZERO) {
                    // get pseudoBox's 8 corner points and build a polarBox of
                    // corner-point vectors
                    let mut tmp_polar = PolarBox::default();
                    for corner in patch.pseudo_box.corners() {
                        let vec = corner - test_pnt;
                        if vec.length_squared() > EFF_ZERO {
                            tmp_polar.add_vector_with_kernel(k, vec, None);
                        }
                    }

                    // when the polar boxes are disjoint
                    if tmp_polar.are_disjoint(&cache.polar_box_unstored(k, surface_id)?) {
                        return Ok(false); // No answer
                    }
                }
            }
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

    fn local_solve(&mut self, branch: &Branch) -> Result<bool> {
        self.local_solve_branch(branch)
    }

    fn identical_solutions(
        &self,
        sol1: &Solution<O>,
        sol2: &Solution<O>,
    ) -> Result<(SolutionPairType, Option<PreferredSolution>)> {
        let k = self.kernel;
        self.surface_identical_solutions(k, sol1, sol2)
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
    /// The kernel (upstream's `SmContext`), for the surfaces' seam tests.
    pub kernel: &'s Kernel,
    /// The caches holding the patches of the two trees' nodes, for the leaf solve
    /// (unit `surface.surface_p11`; upstream reaches them through the nodes' tree
    /// owners).
    pub caches: [Option<&'s SurfaceCache<O>>; 2],
    /// One prepared classifier for both caches, which may belong to one B-rep.
    pub face_queries: Option<&'s dyn crate::surface::surface_cache::face_context::FaceQueries<O>>,
    /// The handles and natural UV domains of the two surfaces, which the solutions
    /// name (`IdenticalSolutions` reads them through upstream's objects); `None` reads
    /// them from the handles themselves.
    pub operands: Option<SolveOperands<O>>,
}

impl<O: SolutionObject> SsGlobalSolver<'_, '_, O> {
    /// `BranchMayContainAnswers`: when normalizing two Bezier patch nodes, false when
    /// the patches' normal cones exclude a common normal direction; else the default
    /// branch test.
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] for two patch nodes when normalizing without
    /// [`SsGlobalSolver::caches`] (the patch data lives in the surface caches:
    /// `surface.surface_cache_p1`); the production solve always sets them.
    pub fn may_contain_answers(&self, branch: &Branch) -> Result<bool> {
        let k = self.kernel;
        if self.base.operation == SolverOperation::Normalize {
            let node1 = self.base.branch_node(branch, 0)?;
            let node2 = self.base.branch_node(branch, 1)?;
            if node1.aux_type == AuxDataType::BezierSurface
                && node2.aux_type == AuxDataType::BezierSurface
            {
                if let [Some(cache1), Some(cache2)] = self.caches {
                    // the patches of the caches the leaf solve reads (unit
                    // surface.surface_p11 hands them over)
                    let id1 = branch.first().copied().flatten();
                    let id2 = branch.get(1).copied().flatten();
                    let (Some(id1), Some(id2)) = (id1, id2) else {
                        return Err(SolidError::null_pointer(OP_SOLVE));
                    };
                    let tree1 = self.base.tree(0)?;
                    let tree2 = self.base.tree(1)?;
                    // (NER on m_pData: a patch node always holds its patch here; a node
                    // that is not a patch of these caches is a caller error,
                    // InvalidInput, not upstream's NULL data)
                    cache1.patch(id1)?;
                    cache2.patch(id2)?;
                    // If the classification of the node is outside then we can quit
                    // here (SM_SUCCESS: FALSE)
                    if !is_node_valid(tree1.owner(), cache1, node1)? {
                        return Ok(false);
                    }
                    if !is_node_valid(tree2.owner(), cache2, node2)? {
                        return Ok(false);
                    }
                    // (SER inside a SmBoolean function: a failure returns its non-zero
                    // status, TRUE)
                    let Some((cone_vec, cone_angle)) =
                        ignored(node1.bbox.cone_bound_to(&node2.bbox))?
                    else {
                        return Ok(true);
                    };
                    let polar1 = cache1.polar_box_unstored(k, id1)?;
                    let polar2 = cache2.polar_box_unstored(k, id2)?;
                    if polar1.is_disjoint_with_cone(cone_vec, cone_angle)
                        || polar2.is_disjoint_with_cone(cone_vec, cone_angle)
                        || polar1.are_disjoint(&polar2)
                    {
                        return Ok(false);
                    }
                    return self.base.branch_may_contain_answers(branch);
                }
                // The patches (the nodes' m_pData) are SurfaceCache::patch(node) of the
                // two surface caches, which the branch above reads. Without them (a
                // solver built without caches: tests and runners; the production SS
                // solve, Surface::global_surface_solve_in here, always sets `caches`
                // after SsGlobalSolver::new) the polar boxes are out of reach; the
                // error names the owner of the patch data (surface.surface_cache_p1).
                // (then sm_IsNodeValid of both nodes, returning SM_SUCCESS = FALSE when
                // outside, ComputeConeBoundTo and the polar box cone tests)
                return Err(SolidError::unported(
                    "SmBezierAux2d::GetPolarBox",
                    "surface.surface_cache_p1",
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

    fn local_solve(&mut self, branch: &Branch) -> Result<bool> {
        // (surface::surface::p11)
        self.local_solve_branch(branch)
    }

    fn identical_solutions(
        &self,
        sol1: &Solution<O>,
        sol2: &Solution<O>,
    ) -> Result<(SolutionPairType, Option<PreferredSolution>)> {
        let k = self.kernel;
        // (surface::surface::p6)
        self.solutions_identical(k, sol1, sol2)
    }
}

// ---- callees in units not ported yet -------------------------------------------------------

/// `SmSurface::EvaluateSTEP(uv, 1, 1, TRUE, TRUE, TRUE, ..)`.
fn evaluate_step(k: &Kernel, surface: &Surface, uv: Point2) -> Result<Vec<Vec<Vec3>>> {
    surface.evaluate_step(k, uv, 1, 1, flags(Side::Left, Side::Left, true))
}

/// The Newton solve of `FindDegenParamForDirection`
/// (`SmFindSingularParamWithGivenTangentDirEFO` with `SmLocalSolve1d::SolveIt` from
/// `start`): `(found, t)`.
fn singular_param_solve(
    k: &Kernel,
    surface: &Surface,
    uv: Point2,
    dir: Vec3,
    singular_dir: SurfParamType,
    ivl: &Extent1d,
    start: f64,
    tol: f64,
) -> Result<(bool, f64)> {
    // set up NR solver (the EFO's tolerance is SM_EFF_ZERO, not used)
    let mut efo = super::p1::FindSingularParamWithGivenTangentDir::new(
        k,
        surface,
        uv,
        dir,
        singular_dir,
        EFF_ZERO,
    );
    let mut ls = crate::geom::local_solve1d::LocalSolve1d::with_interval(*ivl, false);
    ls.set_failed_guess_no_sign_change(k.fixes.oscillating_skips_failed_guesses);
    let r = ls.solve_it(&mut efo, start, tol)?;
    Ok((r.found, r.t))
}

/// The `SmLocalSolveNd` solve of `LocalCurveSolve` over `SmCSNormalizeENFO`
/// (boundary handler `SM_BH_TOTAL_BOUNDARY_HITS` 5, accuracy `SM_EFF_ZERO_SQRT`,
/// evaluator tolerance `SM_EFF_ZERO^2`, no periodicity): the solution vector, `None`
/// when not found.
fn cs_normalize_local_solve(
    k: &Kernel,
    surface: &Surface,
    curve: &Curve,
    op: SolverOperation,
    distance_tolerance: f64,
    opt_vectors: &[Vec3],
    intervals: &[Extent1d],
    guess: &[f64],
) -> Result<Option<Vec<f64>>> {
    let mut bounds = ExtentNd::new(intervals.len());
    for (i, ivl) in intervals.iter().enumerate() {
        if let Some(slot) = bounds.get_mut(i) {
            *slot = *ivl;
        }
    }
    // Do not do periodicity for this function. Local solvers should not cross seams.
    let periodicities = vec![false; guess.len()];
    let mut equations = CsNormalizeEnfo::new(
        k,
        curve,
        surface,
        op,
        distance_tolerance,
        (!opt_vectors.is_empty()).then_some(opt_vectors),
        EFF_ZERO_SQ,
    );
    let mut solver = LocalSolveNd::new(&mut equations, Some(&bounds), Some(&periodicities));
    solver.set_skips_failed_evaluations(k.fixes.oscillating_skips_failed_guesses);
    solver.set_boundary_handler(BoundaryHandler::TotalBoundaryHits, 5);
    let solution = solver.solve_it(guess, EFF_ZERO_SQRT)?;
    // No: see note in upstream [B234]: not found is not found, whatever the reason.
    Ok(solution.found.then_some(solution.x))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::hermite_curve::HermiteCurve;
    use crate::surface::plane::Plane;

    fn issue_2009_kernels() -> (Kernel, Kernel) {
        let off = Kernel::default();
        let mut on = Kernel::default();
        on.fixes.query_statuses_pass_on = true;
        (off, on)
    }

    #[test]
    fn issue_2009_unevaluable_samples_give_no_derivative_lengths() {
        // tangency-labs/smlib 21dcf69 (ApproxDerivativeLengths): the samples that
        // evaluate are averaged, SM_UNDEF_DOUBLE when none does. Upstream averages the
        // unset derivatives.
        let s = Surface::from(BSplineSurface::default());
        let dom = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let (off, on) = issue_2009_kernels();
        let len_off = approx_derivative_lengths(&off, &s, &dom).unwrap();
        let len_on = approx_derivative_lengths(&on, &s, &dom).unwrap();
        assert_eq!(len_on, Vec2::new(UNDEF_DOUBLE, UNDEF_DOUBLE));
        assert_ne!(len_off, len_on);
    }

    #[test]
    fn issue_2009_unevaluable_points_are_not_shown_converged() {
        // tangency-labs/smlib 21dcf69 (sm_PassSSConvergenceTest, sm_PassSCConvergenceTest):
        // a point that cannot be evaluated is not shown converged. Upstream compares the
        // unset values, which agree.
        let s = Surface::from(BSplineSurface::default());
        let uv = Point2::new(0.5, 0.5);
        let (off, on) = issue_2009_kernels();
        let ss_off = pass_ss_convergence_test(&off, &s, uv, &s, uv).unwrap();
        let ss_on = pass_ss_convergence_test(&on, &s, uv, &s, uv).unwrap();
        assert!(!ss_on);
        assert!(ss_off);
    }

    #[test]
    fn issue_2009_unevaluable_surface_is_not_shown_closed() {
        // tangency-labs/smlib 21dcf69 (SmSurface::IsClosed): a surface that cannot be
        // evaluated is not shown closed. Coverage only: upstream's unset ends coincide
        // but its short-curve test then rejects them too.
        let s = Surface::from(BSplineSurface::default());
        let dom = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        assert!(s
            .evaluate_point(&Kernel::default(), Point2::new(0.0, 0.0))
            .is_err());
        let off = Kernel::default();
        let mut on = Kernel::default();
        on.fixes.query_statuses_pass_on = true;
        let closed_off = s
            .is_closed(&off, &dom, SurfParamType::U, None, false)
            .unwrap()
            .0;
        let closed_on = s
            .is_closed(&on, &dom, SurfParamType::U, None, false)
            .unwrap()
            .0;
        assert!(!closed_on);
        assert!(!closed_off);
    }

    /// tangency-labs/solidcore#525 (WR-384): with the corner points wanted, upstream's
    /// `if (lRtn &= bit)` clears the other corners' bits (4 flat corners come back as
    /// none, with only the first point); the opt-in tests each bit.
    #[test]
    fn issue_525_flat_corners_keep_their_bits() {
        let mut k = Kernel::default();
        // a degenerate bilinear patch: every corner's derivatives are parallel
        let s = Surface::from(
            BSplineSurface::create_bilinear_surface(
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(2.0, 0.0, 0.0),
                Vec3::new(3.0, 0.0, 0.0),
            )
            .unwrap(),
        );
        assert_eq!(s.flat_corners(&k, 1.0, false).unwrap().flags, 15);
        let up = s.flat_corners(&k, 1.0, true).unwrap();
        assert_eq!(up.flags, 0);
        k.fixes.flat_corners_test_bits = true;
        let fixed = s.flat_corners(&k, 1.0, true).unwrap();
        assert_eq!(fixed.flags, 15);
        assert!(fixed.points.unwrap().iter().all(|p| *p != Vec3::UNDEF));
    }

    fn plane() -> Surface {
        Surface::from(Plane::from_normal(
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(0.0, 0.0, 1.0),
        ))
    }

    #[test]
    fn flat_corner_points_preserve_each_mask_bit() {
        use crate::nurbs::math::{CPoint, NOW};

        let mut k = Kernel::default();
        k.fixes.flat_corners_test_bits = true;
        // Only the lower/lower corner has nonzero parallel tangents. Reverse
        // either control-net direction to exercise each of the four bits.
        for reverse_u in [false, true] {
            for reverse_v in [false, true] {
                let mut net = vec![
                    vec![
                        CPoint::new(0.0, 0.0, 0.0, NOW),
                        CPoint::new(1.0, 0.0, 0.0, NOW),
                    ],
                    vec![
                        CPoint::new(2.0, 0.0, 0.0, NOW),
                        CPoint::new(1.0, 1.0, 0.0, NOW),
                    ],
                ];
                if reverse_u {
                    net.reverse();
                }
                if reverse_v {
                    for row in &mut net {
                        row.reverse();
                    }
                }
                let knots = [0.0, 0.0, 1.0, 1.0];
                let s = Surface::from(
                    BSplineSurface::from_data(1, 1, 1, 1, 3, 3, &net, &knots, &knots).unwrap(),
                );
                let index = usize::from(reverse_u) * 2 + usize::from(reverse_v);
                let flags = 1 << index;
                assert_eq!(s.flat_corners(&k, 1e-8, false).unwrap().flags, flags);
                let result = s.flat_corners(&k, 1e-8, true).unwrap();
                assert_eq!(result.flags, flags);
                for (i, point) in result.points.unwrap().iter().enumerate() {
                    assert_eq!(*point, if i == index { Vec3::ZERO } else { Vec3::UNDEF });
                }
            }
        }
    }

    #[test]
    fn flat_corner_points_preserve_multiple_bits_and_regular_corners() {
        use crate::nurbs::math::{CPoint, NOW};
        let mut k = Kernel::default();
        k.fixes.flat_corners_test_bits = true;
        let net = vec![
            vec![
                CPoint::new(0.0, 0.0, 0.0, NOW),
                CPoint::new(2.0, 0.0, 0.0, NOW),
            ],
            vec![
                CPoint::new(1.0, 0.0, 0.0, NOW),
                CPoint::new(3.0, 0.0, 0.0, NOW),
            ],
        ];
        let knots = [0.0, 0.0, 1.0, 1.0];
        let s = Surface::from(
            BSplineSurface::from_data(1, 1, 1, 1, 3, 3, &net, &knots, &knots).unwrap(),
        );
        assert_eq!(s.flat_corners(&k, 1e-8, false).unwrap().flags, 15);
        let result = s.flat_corners(&k, 1e-8, true).unwrap();
        assert_eq!(result.flags, 15);
        assert_eq!(
            result.points.unwrap(),
            [
                Vec3::ZERO,
                Vec3::new(2.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(3.0, 0.0, 0.0)
            ]
        );
        assert_eq!(
            plane().flat_corners(&k, 1e-8, true).unwrap(),
            FlatCorners {
                flags: 0,
                points: Some([Vec3::UNDEF; 4])
            }
        );
    }

    /// WR-56 (#1852; the fork's tangency-labs/smlib@0b4de8c): the flat patch
    /// (10 u, 10 v, 0) and the point (5, 5, 1), whose foot is (0.5, 0.5). A local result
    /// 5e-4 along the patch from the foot passes every test before the last (the gap
    /// along the patch is between 1e-4 and 1e-3); the last should take it when its guess
    /// is farther from the point and refuse it when its guess is the foot itself.
    #[test]
    fn wr_56_good_drop_result_compares_the_right_distances() {
        use crate::nurbs::math::{CPoint, NOW};

        let net = vec![
            vec![
                CPoint::new(0.0, 0.0, 0.0, NOW),
                CPoint::new(0.0, 10.0, 0.0, NOW),
            ],
            vec![
                CPoint::new(10.0, 0.0, 0.0, NOW),
                CPoint::new(10.0, 10.0, 0.0, NOW),
            ],
        ];
        let knots = [0.0, 0.0, 1.0, 1.0];
        let s = Surface::from(
            BSplineSurface::from_data(1, 1, 1, 1, 3, 3, &net, &knots, &knots).unwrap(),
        );
        let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let point = Point3::new(5.0, 5.0, 1.0);
        let mut sol = SolutionEnd::new();
        sol.solution_value = 1.0;
        sol.parameters[0] = 0.500_05;
        sol.parameters[1] = 0.5;
        // the result is nearer the point than the guess (0.3, 0.5), farther than the
        // guess (0.5, 0.5)
        let nearer_guess = Point2::new(0.3, 0.5);
        let foot_guess = Point2::new(0.5, 0.5);
        let mut k = Kernel::default();
        let good = |k: &Kernel, guess: Point2| {
            good_drop_result(k, &s, &domain, point, guess, &sol).unwrap()
        };
        // upstream: the nearer result is refused, the farther one taken
        assert!(!good(&k, nearer_guess));
        assert!(good(&k, foot_guess));
        // With the opt-in, a result 5e-4 off along the surface is not one the local
        // solve converged to (tight tolerance 6e-12 here), whatever the guess
        // (tangency-labs/smlib@eaa998a); the distance test is covered on the domain
        // boundary by `issue_1852_good_drop_compares_the_actual_guess_distance`.
        k.fixes.good_drop_result_distances_in_order = true;
        assert!(!good(&k, nearer_guess));
        assert!(!good(&k, foot_guess));
    }

    /// WR-56 (tangency-labs/smlib@eaa998a): with the opt-in, `sm_GoodDropResult` takes
    /// a local result as the foot only when `|Fu| + |Fv|` is under the local solve's
    /// tight tolerance `EFF_ZERO * (1 + max|P|)`, and refuses an interior result with
    /// either component over it; upstream takes any foot whose components sum under
    /// 1e-4 (and an interior one each under 1e-3). The point lies on the plane `d`
    /// from the result along `S_u`; the guess is the result itself, so the distance
    /// test never refuses.
    #[test]
    fn wr_56_good_drop_result_takes_only_a_tightly_converged_foot() {
        let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let s = Surface::from(Plane::from_normal(Point3::ZERO, Vec3::Z));
        let mut k = Kernel::default();
        let result_uv = Point2::new(0.5, 0.5);
        let (result_point, su, _) =
            evaluate_1st_derivatives(&k, &s, result_uv, Side::Left, Side::Left).unwrap();
        let su = su.unitize_or_z();
        let mut sol = SolutionEnd::new();
        sol.solution_value = 1.0;
        *sol.param_mut(0).unwrap() = result_uv.x;
        *sol.param_mut(1).unwrap() = result_uv.y;
        let good = |k: &Kernel, d: f64| {
            good_drop_result(k, &s, &domain, result_point + d * su, result_uv, &sol).unwrap()
        };
        let tight = EFF_ZERO * (1.0 + (result_point + 1e-6 * su).max_dimension());
        assert!(tight < 1e-11);
        // Upstream: every foot within 1e-4 is taken, and an interior one up to 1e-3
        // that passes the distance test.
        assert!(good(&k, 1e-6));
        assert!(good(&k, 5e-5));
        assert!(good(&k, 5e-4));
        assert!(good(&k, 1e-13));
        assert!(!good(&k, 2e-3));
        // Opt-in: only the foot within the tight tolerance.
        k.fixes.good_drop_result_distances_in_order = true;
        assert!(!good(&k, 1e-6));
        assert!(!good(&k, 5e-5));
        assert!(!good(&k, 5e-4));
        assert!(good(&k, 1e-13));
        assert!(!good(&k, 2e-3));
    }

    /// HANG-32: the public projection entry rejects a supplied approximation
    /// tolerance that is not finite, positive and normal before it prepares any cache
    /// or geometry; the FFI already refuses the same inputs. The unsupported Hermite
    /// curve keeps the accepted cases on the cheap path to the existing
    /// `not an SmBSplineCurve` refusal (no intersector), so this is a contract test,
    /// not a successful projection.
    #[test]
    fn hang_32_projection_rejects_non_normal_approximation_tolerance() {
        let k = Kernel::default();
        let s = plane();
        let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let unsupported = Curve::from(HermiteCurve::new(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::X,
            Point3::new(1.0, 0.0, 0.0),
            Vec3::X,
            3,
        ));
        let call = |approx: Option<f64>, vector: Vec3| {
            s.create_parallel_projection_curves(
                &k,
                &domain,
                &unsupported,
                vector,
                Some(1.0),
                approx,
                None,
            )
        };

        let rejected = |approx: f64| match call(Some(approx), Vec3::Z) {
            Err(SolidError::InvalidInput { operation, reason }) => {
                assert_eq!(operation, OP_PROJ);
                assert_eq!(
                    reason,
                    "approximation tolerance must be finite, positive and normal (HANG-32)"
                );
            }
            other => panic!("approximation tolerance {approx:e} was not rejected: {other:?}"),
        };
        for approx in [
            f64::from_bits(1),
            f64::from_bits(0x000F_FFFF_FFFF_FFFF),
            0.0,
            -0.0,
            -1.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            rejected(approx);
        }

        // The precondition accepts `None` and every finite positive normal value, with
        // no upper cap: each reaches the later unsupported-kind refusal.
        for approx in [None, Some(f64::MIN_POSITIVE), Some(1e-6), Some(f64::MAX)] {
            match call(approx, Vec3::Z) {
                Err(SolidError::InvalidInput { operation, reason }) => {
                    assert_eq!(operation, OP_PROJ);
                    assert_eq!(reason, "not an SmBSplineCurve");
                }
                other => {
                    panic!("approximation tolerance {approx:?} was rejected too early: {other:?}")
                }
            }
        }

        // The zero projection vector keeps its precedence over the new check.
        match call(Some(f64::from_bits(1)), Vec3::ZERO) {
            Err(SolidError::InvalidInput { operation, reason }) => {
                assert_eq!(operation, OP_PROJ);
                assert_eq!(reason, "zero projection vector");
            }
            other => panic!("zero vector did not take precedence: {other:?}"),
        }
    }

    #[test]
    fn plane_normal_is_axis() {
        let k = &Kernel::default();
        let s = plane();
        let n = s
            .evaluate_normal(k, Point2::new(0.2, 0.3), Side::Left, Side::Left)
            .unwrap();
        assert!((n.z - 1.0).abs() < 1e-12);
    }

    #[test]
    fn base_normal_of_plane_part() {
        let k = &Kernel::default();
        let s = plane();
        let n = base_evaluate_normal(k, &s, Point2::new(0.2, 0.3), Side::Left, Side::Left).unwrap();
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
        let k = &Kernel::default();
        let s = plane();
        let d = s.natural_uv_domain_if_built();
        let v = compute_inward_vector(k, &s, d.min()).unwrap();
        assert!((v.length() - 1.0).abs() < 1e-12);
    }

    /// tangency-labs/solidcore#1852 (WR-56): `sm_GoodDropResult` stores the result
    /// distance in `dDsqGuess` and the guess distance in `dDsqResult`, so its last
    /// test rejects a drop that improved on the guess. The opt-in
    /// `Kernel::fixes.good_drop_result_distances_in_order` compares the actual
    /// guess and result distances. Every case reaches the final comparison: the
    /// result lies on the domain's `u = 0` side and the point on the plane `1/2048`
    /// beyond it along `-S_u`, so `Fu` is `-1/2048` (allowed on that side) and `Fv`
    /// is zero, past the residual early return in both modes. On that side the
    /// opt-in's tight foot test (tangency-labs/smlib@eaa998a), which refuses an
    /// interior result that far off, leaves the comparison reachable.
    #[test]
    fn issue_1852_good_drop_compares_the_actual_guess_distance() {
        let h = 1.0 / 2048.0;
        let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let s = Surface::from(Plane::from_normal(Point3::ZERO, Vec3::Z));
        let mut k = Kernel::default();
        let result_uv = Point2::new(0.0, 0.5);
        let (result_point, su, _) =
            evaluate_1st_derivatives(&k, &s, result_uv, Side::Left, Side::Left).unwrap();
        let pt_to_drop = result_point - h * su;
        let mut sol = SolutionEnd::new();
        sol.solution_value = 1.0;
        *sol.param_mut(0).unwrap() = result_uv.x;
        *sol.param_mut(1).unwrap() = result_uv.y;

        // The result is closer than the guess: upstream rejects it (its swapped
        // names make the guess look farther); the fix accepts it.
        let nearer_guess = result_uv + Point2::new(0.1, 0.0);
        // The guess is closer than the result: upstream accepts it; the fix
        // rejects it, as the final test intends.
        let farther_guess = result_uv - Point2::new(h, 0.0);
        // Equidistant: both accept.
        let equal_guess = result_uv - Point2::new(2.0 * h, 0.0);

        assert!(!good_drop_result(&k, &s, &domain, pt_to_drop, nearer_guess, &sol).unwrap());
        assert!(good_drop_result(&k, &s, &domain, pt_to_drop, farther_guess, &sol).unwrap());
        assert!(good_drop_result(&k, &s, &domain, pt_to_drop, equal_guess, &sol).unwrap());

        k.fixes.good_drop_result_distances_in_order = true;
        assert!(good_drop_result(&k, &s, &domain, pt_to_drop, nearer_guess, &sol).unwrap());
        assert!(!good_drop_result(&k, &s, &domain, pt_to_drop, farther_guess, &sol).unwrap());
        assert!(good_drop_result(&k, &s, &domain, pt_to_drop, equal_guess, &sol).unwrap());
    }

    fn wr57_double_root_patch_u(u_min: f64, u_max: f64) -> Surface {
        use crate::nurbs::math::{CPoint, NOW};

        // S(u, v) = v * (1, 16 * (s - 0.25) * (s - 0.75), 0), s = (u - u_min) / (u_max - u_min):
        // singular along v = 0, with exact +X non-singular tangents at s = 0.25 and s = 0.75
        // and symmetric sample dot products at ii = 2 (2/9) and ii = 7 (7/9).
        let net = vec![
            vec![
                CPoint::new(0.0, 0.0, 0.0, NOW),
                CPoint::new(1.0, 3.0, 0.0, NOW),
            ],
            vec![
                CPoint::new(0.0, 0.0, 0.0, NOW),
                CPoint::new(1.0, -5.0, 0.0, NOW),
            ],
            vec![
                CPoint::new(0.0, 0.0, 0.0, NOW),
                CPoint::new(1.0, 3.0, 0.0, NOW),
            ],
        ];
        let u_knots = [u_min, u_min, u_min, u_max, u_max, u_max];
        let v_knots = [0.0, 0.0, 1.0, 1.0];
        Surface::from(
            BSplineSurface::from_data(2, 1, 2, 1, 5, 3, &net, &u_knots, &v_knots).unwrap(),
        )
    }

    fn wr57_double_root_patch_v(v_min: f64, v_max: f64) -> Surface {
        use crate::nurbs::math::{CPoint, NOW};

        let net = vec![
            vec![
                CPoint::new(0.0, 0.0, 0.0, NOW),
                CPoint::new(0.0, 0.0, 0.0, NOW),
                CPoint::new(0.0, 0.0, 0.0, NOW),
            ],
            vec![
                CPoint::new(1.0, 3.0, 0.0, NOW),
                CPoint::new(1.0, -5.0, 0.0, NOW),
                CPoint::new(1.0, 3.0, 0.0, NOW),
            ],
        ];
        let u_knots = [0.0, 0.0, 1.0, 1.0];
        let v_knots = [v_min, v_min, v_min, v_max, v_max, v_max];
        Surface::from(
            BSplineSurface::from_data(1, 2, 1, 2, 3, 5, &net, &u_knots, &v_knots).unwrap(),
        )
    }

    /// WR-57 (#2209): `UpstreamFixes::degen_param_tiebreak_uses_actual_param` defaults to
    /// `false`, is enabled by `UpstreamFixes::all()`, and can be toggled through
    /// `UpstreamFixes::flag_mut`.
    #[test]
    fn wr57_flag_default_off_and_lookup_by_name() {
        use crate::kernel::UpstreamFixes;

        let mut k = Kernel::default();
        assert!(!k.fixes.degen_param_tiebreak_uses_actual_param);
        assert!(UpstreamFixes::all().degen_param_tiebreak_uses_actual_param);
        assert!(UpstreamFixes::NAMES.contains(&"degen_param_tiebreak_uses_actual_param"));

        let flag = k
            .fixes
            .flag_mut("degen_param_tiebreak_uses_actual_param")
            .unwrap();
        assert!(!*flag);
        *flag = true;
        assert!(k.fixes.degen_param_tiebreak_uses_actual_param);

        *k.fixes
            .flag_mut("degen_param_tiebreak_uses_actual_param")
            .unwrap() = false;
        assert!(!k.fixes.degen_param_tiebreak_uses_actual_param);
    }

    /// WR-57 (#2209): on a sphere reparameterized to `[0, 360]` or `[0, 2 * PI]` in `u`,
    /// the south-pole seam tie-break (`ii = 0` vs `ii = 9` for `+X`) compares `[0, 1]`
    /// fractions against `dS` by default (flipping `input_s = 5.0` to `360.0` and
    /// `input_s = 0.8` to `2 * PI`), whereas the opt-in compares the mapped singular
    /// parameters and keeps `u = 0.0`; on `[0, 1]` both modes agree identically.
    #[test]
    fn wr57_sphere_pole_seam_tiebreak_on_non_unit_and_unit_domains() {
        use crate::geom::axis2_placement::Axis2Placement;
        use crate::math::PI;
        use crate::surface::sphere::Sphere;

        let k_off = Kernel::default();
        let mut k_on = Kernel::default();
        k_on.fixes.degen_param_tiebreak_uses_actual_param = true;

        let sphere = Sphere::create_canonical(&k_off, &Axis2Placement::default(), 1.0).unwrap();

        // Native `[0, 1]` singular domain: default and opt-in behave identically.
        let unit_surf = Surface::from(sphere.clone());
        assert_eq!(
            unit_surf
                .natural_uv_domain_with_nurb(&k_off)
                .unwrap()
                .u_interval(),
            Extent1d::from_min_max(0.0, 1.0)
        );
        for input_s in [-2.0, 0.0, 0.1, 0.49, 0.51, 0.9, 1.0, 5.0] {
            let off = unit_surf
                .find_degen_param_for_direction(
                    &k_off,
                    Point2::new(input_s, 0.0),
                    Vec3::X,
                    true,
                    SurfParamType::U,
                    1e-9,
                )
                .unwrap();
            let on = unit_surf
                .find_degen_param_for_direction(
                    &k_on,
                    Point2::new(input_s, 0.0),
                    Vec3::X,
                    true,
                    SurfParamType::U,
                    1e-9,
                )
                .unwrap();
            assert_eq!(off, on);
            assert!(off.converged);
            let expected = if input_s < 0.5 { 0.0 } else { 1.0 };
            assert!((off.uv.x - expected).abs() < 1e-9);
        }

        // `[0, 360]` angular domain with `input_s = 5.0`: `5.0` is `5.0` from `0.0` vs
        // `355.0` from `360.0`, yet `|1.0 - 5.0| = 4.0 < |0.0 - 5.0| = 5.0`.
        let mut sphere_deg = sphere.clone();
        sphere_deg
            .reparameterize(&k_off, &Extent2d::from_bounds(0.0, 0.0, 360.0, 1.0))
            .unwrap();
        let surf_deg = Surface::from(sphere_deg);
        let off_deg = surf_deg
            .find_degen_param_for_direction(
                &k_off,
                Point2::new(5.0, 0.0),
                Vec3::X,
                true,
                SurfParamType::U,
                1e-9,
            )
            .unwrap();
        let on_deg = surf_deg
            .find_degen_param_for_direction(
                &k_on,
                Point2::new(5.0, 0.0),
                Vec3::X,
                true,
                SurfParamType::U,
                1e-9,
            )
            .unwrap();
        assert!(off_deg.converged);
        assert!((off_deg.uv.x - 360.0).abs() < 1e-9);
        assert!(on_deg.converged);
        assert!(on_deg.uv.x.abs() < 1e-9);

        // `[0, 2 * PI]` angular domain with `input_s = 0.8` rad (`0.8` from `0.0` vs
        // `2 * PI - 0.8` from `2 * PI`, yet `|1.0 - 0.8| = 0.2 < |0.0 - 0.8| = 0.8`).
        let mut sphere_rad = sphere;
        sphere_rad
            .reparameterize(&k_off, &Extent2d::from_bounds(0.0, 0.0, 2.0 * PI, 1.0))
            .unwrap();
        let surf_rad = Surface::from(sphere_rad);
        let off_rad = surf_rad
            .find_degen_param_for_direction(
                &k_off,
                Point2::new(0.8, 0.0),
                Vec3::X,
                true,
                SurfParamType::U,
                1e-9,
            )
            .unwrap();
        let on_rad = surf_rad
            .find_degen_param_for_direction(
                &k_on,
                Point2::new(0.8, 0.0),
                Vec3::X,
                true,
                SurfParamType::U,
                1e-9,
            )
            .unwrap();
        assert!(off_rad.converged);
        assert!((off_rad.uv.x - 2.0 * PI).abs() < 1e-9);
        assert!(on_rad.converged);
        assert!(on_rad.uv.x.abs() < 1e-9);
    }

    /// WR-57 (#2209): on B-spline degenerate patches with interior double solutions on
    /// `[10, 20]` and `[-20, -10]` (for both `SurfParamType::U` and `SurfParamType::V`),
    /// default mode chooses the sample fraction closer in `[0, 1]` while opt-in chooses
    /// the mapped singular parameter closer to `dS`, and `[0, 1]` patches match bit for
    /// bit across both modes.
    #[test]
    fn wr57_nurbs_degenerate_patch_shifted_domains_and_both_singular_directions() {
        let k_off = Kernel::default();
        let mut k_on = Kernel::default();
        k_on.fixes.degen_param_tiebreak_uses_actual_param = true;

        // Positive shifted `U` domain `[10, 20]`, interior roots at `12.5` and `17.5`.
        let pos_u = wr57_double_root_patch_u(10.0, 20.0);
        let off_pos = pos_u
            .find_degen_param_for_direction(
                &k_off,
                Point2::new(12.0, 0.0),
                Vec3::X,
                true,
                SurfParamType::U,
                1e-9,
            )
            .unwrap();
        let on_pos = pos_u
            .find_degen_param_for_direction(
                &k_on,
                Point2::new(12.0, 0.0),
                Vec3::X,
                true,
                SurfParamType::U,
                1e-9,
            )
            .unwrap();
        assert!(off_pos.converged);
        assert!((off_pos.uv.x - 17.5).abs() < 1e-9);
        assert!(on_pos.converged);
        assert!((on_pos.uv.x - 12.5).abs() < 1e-9);

        // Inbound direction (`dir_from_pole = false` with `-X`) exercises the negated
        // direction branch with the same tie-break.
        let off_in = pos_u
            .find_degen_param_for_direction(
                &k_off,
                Point2::new(12.0, 0.0),
                -Vec3::X,
                false,
                SurfParamType::U,
                1e-9,
            )
            .unwrap();
        let on_in = pos_u
            .find_degen_param_for_direction(
                &k_on,
                Point2::new(12.0, 0.0),
                -Vec3::X,
                false,
                SurfParamType::U,
                1e-9,
            )
            .unwrap();
        assert!((off_in.uv.x - 17.5).abs() < 1e-9);
        assert!((on_in.uv.x - 12.5).abs() < 1e-9);

        // When `input_s = 18.0` is genuinely near `17.5`, both modes pick `17.5`.
        let off_upper = pos_u
            .find_degen_param_for_direction(
                &k_off,
                Point2::new(18.0, 0.0),
                Vec3::X,
                true,
                SurfParamType::U,
                1e-9,
            )
            .unwrap();
        let on_upper = pos_u
            .find_degen_param_for_direction(
                &k_on,
                Point2::new(18.0, 0.0),
                Vec3::X,
                true,
                SurfParamType::U,
                1e-9,
            )
            .unwrap();
        assert!((off_upper.uv.x - 17.5).abs() < 1e-9);
        assert!((on_upper.uv.x - 17.5).abs() < 1e-9);

        // Negative shifted `U` domain `[-20, -10]`, interior roots at `-17.5` and `-12.5`:
        // at `input_s = -12.0`, default keeps `2/9` (`-17.5`) because `-12.0 < 0.5`,
        // whereas opt-in switches to `7/9` (`-12.5`).
        let neg_u = wr57_double_root_patch_u(-20.0, -10.0);
        let off_neg = neg_u
            .find_degen_param_for_direction(
                &k_off,
                Point2::new(-12.0, 0.0),
                Vec3::X,
                true,
                SurfParamType::U,
                1e-9,
            )
            .unwrap();
        let on_neg = neg_u
            .find_degen_param_for_direction(
                &k_on,
                Point2::new(-12.0, 0.0),
                Vec3::X,
                true,
                SurfParamType::U,
                1e-9,
            )
            .unwrap();
        assert!(off_neg.converged);
        assert!((off_neg.uv.x + 17.5).abs() < 1e-9);
        assert!(on_neg.converged);
        assert!((on_neg.uv.x + 12.5).abs() < 1e-9);

        // `SurfParamType::V` on `[0, 1] x [10, 20]`, interior roots at `v = 12.5` and `17.5`.
        let pos_v = wr57_double_root_patch_v(10.0, 20.0);
        let off_v = pos_v
            .find_degen_param_for_direction(
                &k_off,
                Point2::new(0.0, 12.0),
                Vec3::X,
                true,
                SurfParamType::V,
                1e-9,
            )
            .unwrap();
        let on_v = pos_v
            .find_degen_param_for_direction(
                &k_on,
                Point2::new(0.0, 12.0),
                Vec3::X,
                true,
                SurfParamType::V,
                1e-9,
            )
            .unwrap();
        assert!(off_v.converged);
        assert!((off_v.uv.y - 17.5).abs() < 1e-9);
        assert!(on_v.converged);
        assert!((on_v.uv.y - 12.5).abs() < 1e-9);

        // `[0, 1]` singular `U` domain: bit-identical in both modes.
        let unit_u = wr57_double_root_patch_u(0.0, 1.0);
        for input_s in [-2.0, 0.0, 0.2, 0.49, 0.51, 0.8, 1.0, 5.0] {
            let off = unit_u
                .find_degen_param_for_direction(
                    &k_off,
                    Point2::new(input_s, 0.0),
                    Vec3::X,
                    true,
                    SurfParamType::U,
                    1e-9,
                )
                .unwrap();
            let on = unit_u
                .find_degen_param_for_direction(
                    &k_on,
                    Point2::new(input_s, 0.0),
                    Vec3::X,
                    true,
                    SurfParamType::U,
                    1e-9,
                )
                .unwrap();
            assert_eq!(off, on);
            assert!(off.converged);
            let expected = if input_s < 0.5 { 0.25 } else { 0.75 };
            assert!((off.uv.x - expected).abs() < 1e-9);
        }
    }
}
