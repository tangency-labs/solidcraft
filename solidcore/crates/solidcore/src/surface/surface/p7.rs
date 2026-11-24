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
//! - **Recursion bound.** The degenerate-direction search's certified evaluator looks
//!   through nested curve-bounded bases at most `BASIS_DEPTH` (16) layers deep; past
//!   that the search proves no sign, as for a kind with no certified evaluator.
//! - **A blend's net for averages (#2510).** `ApproxDerivativeLengths`, an average that
//!   scales tolerances, evaluates a procedural blend as its certified net
//!   ([`crate::surface::procedural_blend::NetScope::new`]).
//! - **Debug-only work** compiled out of the release build (drawing, dumps, the
//!   `SM_ASSERT`s, `EvaluateNormal`'s static list of bad surfaces) is not ported, nor
//!   is `FindDegenParamForDirection`'s angle bookkeeping (`dBestRad`,
//!   `dNearBestRad`: computed in release builds but never read).
//! - **Good drop result (WR-56).** `sm_GoodDropResult` takes a result as the foot only
//!   when its gap's components along the unit tangents sum under the local solve's
//!   tight tolerance (upstream: under 1e-4), and refuses an interior result more than
//!   that tight tolerance off along either unit tangent (upstream: 1e-3) (the fork's
//!   tangency-labs/smlib@eaa998a); its last test takes only a result at the guess
//!   itself, where upstream compares the guess and result distances with swapped names
//!   (taking a result farther than its guess, refusing one nearer): a result the local
//!   solve moved to, nearer or not, goes to the global solve (#17773).
//! - **Flat corners (WR-384).** `GetFlatCorners` tests each corner bit while filling
//!   the corner points; upstream masks its return value with `&=` there, so it returns
//!   at most the first corner's flag and point.
//! - **Upstream quirks kept:** `SmSSGlobalSolver::BranchMayContainAnswers` returns
//!   `SM_SUCCESS` (that is, `FALSE`) for a node outside its face and `NER`'s status
//!   (non-zero: `TRUE`) for a missing patch.
//! - **Degenerate-parameter guesses (WR-57).** `FindDegenParamForDirection` measures
//!   each guess at its pulled-in sample `sample_domain.evaluate(f)`, but upstream breaks
//!   near-ties by comparing the `[0, 1]` fraction `f` with the parameter `dS`, and starts
//!   the Newton solve (and returns its unconverged answer) at `sSIvl.Evaluate(f)`, which
//!   for `f = 0` or `1` is the domain boundary, possibly a collapsed corner. The port
//!   compares, starts at and returns the sampled parameter itself, and takes the input
//!   parameter `dS` as the first near-best guess when it is in the interval and its
//!   tangent is defined (the guess nearest the input; where the alignment is the same
//!   over a stretch, the solve stays at its start, so a sample would replace an input
//!   that is already an answer). An answer counts as converged only where it is a
//!   local maximum of the alignment on the closed interval, checked at the answer
//!   with the solve's own `F` and `F'`: an end where the alignment grows outward, a
//!   stationary point with curvature not positive (polished by one Newton step), or a
//!   knot where the one-sided slopes change from `+` to `-` (a kink); on a direction
//!   closed across the seam an end counts only at a crease, otherwise the solve
//!   restarts at the other end. A maximum less
//!   aligned than the best sample is not the largest: then the best sample's solve,
//!   and the end where the best sample is an end sample, are tried too, and the most
//!   aligned maximum is kept. Upstream accepts any answer the solve reports found (a
//!   minimum, or a point beside a kink). At a corner collapsed to a pole, where the
//!   tangent has no value, an end is a maximum by the certified limit of the slope from
//!   inside (an exact pole); where the tangent is not enclosed away from zero otherwise,
//!   the solve's own stationary answer is accepted as upstream accepts it.
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
//! - **No evidence (WR-663).** `IsClosed` is `false` when an end has no position or the
//!   short-arc filter's Frenet frame cannot be evaluated (the fork,
//!   tangency-labs/smlib 21dcf69); where only the derivatives (end, continuity or mid)
//!   cannot be evaluated, as through a cone apex, the end positions (as the evaluation
//!   writes them, [`SurfaceEvaluate::evaluate_point_written`]) decide closure and
//!   the continuity is Discontinuous, the rule `SmCPGlobalSolver::Solve` follows for a
//!   closed curve. The base `IsPlanar` is `false` without its box. The other
//!   statuses this file's functions drop are passed on and their soft answers given
//!   (the convergence tests, `sm_GoodDropResult`, `ApproxDerivativeLengths`,
//!   `FindDegenParamForDirection`, `sm_CheckSeamDrop`, `sm_LoadGuesses`), as in the
//!   other `surface::surface` files
//!   (`plan/upstream-issues/WR-663--addendum-2026-10-31-surface-statuses.md`).

use crate::curve::curve::{Curve, CurveKind};
use crate::error::{ignore_status, ignored, Result, SolidError};
use crate::eval::collapse::certified_partials;
use crate::eval::spline::EvalSurface;
use crate::geom::certified;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent1d::ExtentNd;
use crate::geom::extent2d::Extent2d;
use crate::geom::geom_utility::p2::{
    evaluate_curve_continuity, frenet_from_derivatives, ContinuityType, CurveEval,
};
use crate::geom::global_solver::{
    Branch, GlobalSolve, GlobalSolver, PreferredSolution, SolutionPairType, SolutionRequested,
    SolveRequest, SolverOperation,
};
use crate::geom::local_solve_nd::{BoundaryHandler, LocalSolveNd};
use crate::geom::polar_box::PolarBox;
use crate::geom::pseudo_box::{Planar, PseudoBox};
use crate::geom::solution_array::{Solution, SolutionArray, SolutionEnd, SolutionObject};
use crate::geom::tol::scaled_zero;
use crate::geom::tree::{AuxDataType, Tree};
use crate::geom::vector2d::Vec2;
use crate::geom::vector3d::sqrt as sm_sqrt;
use crate::kernel::Kernel;
use crate::math::{
    max2, Point2, Point3, Vec3, BIG_DOUBLE, EFF_ZERO, EFF_ZERO_SQ, EFF_ZERO_SQRT, UNDEF_DOUBLE,
};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::{
    BSplineSurface, BSplineSurfaceForm, BoundingBoxRequest, LocationType, SurfParamType,
};
use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use crate::nurbs::knots::Side;
use crate::nurbs::math::{gamma, NOW, NOZ};
use crate::surface::procedural_blend::NetScope;
use crate::surface::surface_cache::SurfaceCache;
use crate::topology::BrepId;

use super::p1::{
    evaluate_1st_derivatives, evaluate_2nd_derivatives, evaluate_directional_derivs, is_node_valid,
    CsNormalizeEnfo, OperandDomain, SolveOperands, SurfaceSolveContext,
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
                                                  // (passed on as the fork does, WR-663, tangency-labs/smlib 21dcf69)
                    normal = normal.unitize()?;
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
/// `smgu_EvaluateCurveContinuity` (see the module docs; its G2 test is geometric, WR-380).
pub fn base_is_closed<S: SurfaceEvaluate + ?Sized>(
    k: &Kernel,
    surface: &S,
    uv_domain: &Extent2d,
    param: SurfParamType,
    opt_tol: Option<f64>,
    want_continuity: bool,
) -> Result<(bool, Option<ContinuityType>)> {
    let discontinuous = want_continuity.then_some(ContinuityType::Discontinuous);
    // a surface whose end positions cannot be evaluated is not shown closed
    // (tangency-labs/smlib 21dcf69)

    // locals
    let count = if param == SurfParamType::Both { 2 } else { 1 };
    let mut min_continuity = ContinuityType::CInfinity;
    let mut p_mid: Vec<Vec3> = vec![Vec3::UNDEF; 4];
    let mut g_mid: Vec<Vec3> = vec![Vec3::UNDEF; 4];
    let mut uv_min = Vec2::UNDEF;
    let mut uv_max = Vec2::UNDEF;
    let mut uv_mid = Vec2::UNDEF;

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

            // evaluate isoParameter endPoints (the second derivatives only when the
            // continuity is wanted). Closure is decided from the end positions alone:
            // where the derivatives cannot be evaluated (a pole such as a cone apex) the
            // positions still decide it and the continuity is Discontinuous, as
            // `SmCPGlobalSolver::Solve` does for a closed curve. The position is the
            // one the evaluation writes even when its status fails (a cone's NURB
            // position at the apex); only an end with no position leaves the surface
            // not shown closed (tangency-labs/smlib 21dcf69, WR-663).
            let d2 = if want_continuity {
                match (
                    ignored(evaluate_2nd_derivatives(
                        k,
                        surface,
                        uv_min,
                        Side::Left,
                        Side::Left,
                    ))?,
                    ignored(evaluate_2nd_derivatives(
                        k,
                        surface,
                        uv_max,
                        Side::Left,
                        Side::Left,
                    ))?,
                ) {
                    (Some(d2_min), Some(d2_max)) => Some((d2_min, d2_max)),
                    _ => None,
                }
            } else {
                None
            };
            let (p_min, p_max) = if let Some((d2_min, d2_max)) = d2 {
                (d2_min.point, d2_max.point)
            } else {
                let Some(p_min) = surface.evaluate_point_written(k, uv_min)? else {
                    return Ok((false, discontinuous));
                };
                let Some(p_max) = surface.evaluate_point_written(k, uv_max)? else {
                    return Ok((false, discontinuous));
                };
                if want_continuity {
                    min_continuity = ContinuityType::Discontinuous;
                }
                (p_min, p_max)
            };

            // pick a tolerance
            let scaled_zero = EFF_ZERO * 1000.0 * (1.0 + p_max.max_dimension());
            let tol = opt_tol.unwrap_or(scaled_zero);

            // when the startPoint/endPoint gap is too big - not closed
            let vec_diff = p_min - p_max;
            if vec_diff.length_squared() > tol * tol {
                return Ok((false, discontinuous));
            }

            // when asked - compute and save the lowest continuity across the boundary
            if let Some((d2_min, d2_max)) = d2 {
                let (first_min, second_min, first_max, second_max) =
                    if this_param == SurfParamType::U {
                        (d2_min.du, d2_min.duu, d2_max.du, d2_max.duu)
                    } else {
                        (d2_min.dv, d2_min.dvv, d2_max.dv, d2_max.dvv)
                    };
                let curve_continuity = match evaluate_curve_continuity(
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
                ) {
                    // a continuity that cannot be evaluated is not shown: Discontinuous
                    Err(e) if !e.is_unported_or_cancelled() => ContinuityType::Discontinuous,
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
                // (no derivatives through the middle, an iso line through a pole: the
                // end positions decide closure, the seam is not shown smooth)
                let Some(d) = ignored(evaluate_directional_derivs(
                    k,
                    surface,
                    uv_mid,
                    uv_dir,
                    2,
                    Side::Left,
                    Side::Left,
                ))?
                else {
                    min_continuity = ContinuityType::Discontinuous;
                    continue;
                };
                for (dst, src) in p_mid.iter_mut().zip(d) {
                    *dst = src;
                }

                let p_mid0 = p_mid.first().copied().unwrap_or(Vec3::UNDEF);
                let gap1_sq = (p_mid0 - p_min).length_squared();
                let gap2_sq = (p_mid0 - p_max).length_squared();
                let mid_gap_sq = max2(gap1_sq, gap2_sq);

                // cheesy one point test for a short curve
                if mid_gap_sq < 4.0 * tol * tol {
                    // but some short curves can be very small circles - watch for those
                    // (status ignored upstream, which then reads what the failed call
                    // wrote; the fork shows nothing closed)
                    let Some(g) = ignored(frenet_from_derivatives(&p_mid, 2))? else {
                        return Ok((false, discontinuous));
                    };
                    for (dst, src) in g_mid.iter_mut().zip(g) {
                        *dst = src;
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
            | SurfaceKind::ProceduralBlend(_)
            | SurfaceKind::BallPatch(_)
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
            | SurfaceKind::ProceduralBlend(_)
            | SurfaceKind::BallPatch(_)
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
            SurfaceKind::BallPatch(s) => {
                Ok(planar_of(k, s.net(), &self.natural_uv_domain_with_nurb(k)?)?.is_planar(tol_3d))
            }
            // SmSurface::IsPlanar on the blend's own (virtual) CalculateBoundingBox: the
            // net's pseudo box grown by the bound; a box that fails is not planar (the
            // fork's IsPlanar, the only one a blend meets)
            SurfaceKind::ProceduralBlend(pb) => {
                let req = BoundingBoxRequest {
                    normal: false,
                    pseudo: true,
                    polar: false,
                    pseudo_guess: None,
                    polar_guess: None,
                    expand_by_zone_tol: false,
                    expand_tolerance: None,
                };
                let domain = self.natural_uv_domain_with_nurb(k)?;
                let boxed = ignored(pb.calculate_bounding_box(k, &domain, &req, self))?
                    .and_then(|b| b.pseudo);
                let shown = boxed.is_some();
                let mut planar = boxed.unwrap_or_else(PseudoBox::new).is_planar(tol_3d);
                if !shown {
                    planar.is_planar = false;
                }
                Ok(planar)
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
                // a surface that cannot be boxed is not shown planar, as the fork
                // (tangency-labs/smlib 21dcf69)
                let not_shown = pseudo.is_none();
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

        // optional output: each bit is tested (WR-384; upstream masks the return value
        // while testing, `if (lRtn &= bit)`, so it loses every other corner)
        if let Some(pts) = points.as_mut() {
            for ((c, bit), p) in corners.iter().zip(FLAT_CORNER_BITS).zip(pts.iter_mut()) {
                if rtn & bit != 0 {
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
    /// Each guess is a parameter where its alignment was measured, the input parameter
    /// (when it is in the interval and its tangent is defined) first, then the samples:
    /// the near-equal tie-break compares those parameters with the input's, and the
    /// Newton solve starts at the chosen guess. An answer has converged only where it is
    /// a local maximum of the alignment on the closed interval (an end where the
    /// alignment grows outward, unless the direction is closed across the seam and the
    /// alignment does not also grow toward it at the other end, a stationary point whose
    /// curvature is not positive, or a knot where the slope changes from `+` to `-`).
    /// The answer is the most aligned of the maxima the solve reaches from the chosen
    /// guess and from the best sample and of the interval's ends (the first on a tie
    /// within rounding); when that is less aligned than the best sample the solve also
    /// starts at every sample and at upstream's ten starts, and then on each bracket
    /// between neighbouring ends, starts, samples and knots whose one-sided slopes are
    /// `+` then `-` (a peak narrower than the starts' spacing). A maximum less aligned than
    /// the best sample is not the largest: then the most aligned of it, the best sample
    /// and upstream's return is returned, unconverged. Upstream has no input guess,
    /// compares the `[0, 1]` sample fractions with the input parameter, starts at (and
    /// returns unconverged) the fraction mapped onto the whole interval and accepts any
    /// answer the solve reports found (WR-57).
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
        let search = || {
            Ok(self
                .degen_param_search(k, uv, dir, dir_from_pole, singular_dir, tol, false)?
                .0)
        };
        // (test-only seam of the WR-57 substitution replay, feature `wr57-replay`)
        #[cfg(feature = "wr57-replay")]
        let search = || {
            let input = super::replay::degen_input(uv, dir, dir_from_pole, singular_dir, tol);
            super::replay::wrap(k, self, super::replay::Seam::Degen, &input, false, search)
        };
        search()
    }

    /// [`Surface::find_degen_param_for_direction`], and with `with_roots` also the
    /// singular parameters where its Newton solve reports found from each of upstream's
    /// ten starts (the `[0, 1]` sample fractions mapped onto the whole interval), in
    /// start order: every answer upstream's search can give converged is among them
    /// (WR-57). [`Surface::invert_point_near_singularity`] drops onto the iso curve at
    /// each.
    ///
    /// # Errors
    ///
    /// Those of [`Surface::find_degen_param_for_direction`]; [`SolidError::Unported`]
    /// from the extra solves (their failing statuses only drop that start).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn degen_param_search(
        &self,
        k: &Kernel,
        uv: Point2,
        dir: Vec3,
        dir_from_pole: bool,
        singular_dir: SurfParamType,
        tol: f64,
        with_roots: bool,
    ) -> Result<(DegenParam, Vec<f64>)> {
        // copy and unitize direction to match
        let mut s_dir = dir.unitize()?;

        // locals
        let domain = self.natural_uv_domain_with_nurb(k)?;
        let uv_min = domain.min();
        let uv_max = domain.max();
        let mut s_ivl = Extent1d::new();
        let mut t_ivl = Extent1d::new();
        let singular_u = singular_dir == SurfParamType::U;
        // (the statuses SMLib drops here are passed on, as the fork does, WR-663,
        // tangency-labs/smlib 21dcf69)
        let (ds, dt) = if singular_u {
            s_ivl.set_min_max(uv_min.x, uv_max.x)?;
            t_ivl.set_min_max(uv_min.y, uv_max.y)?;
            (uv.x, uv.y)
        } else {
            s_ivl.set_min_max(uv_min.y, uv_max.y)?;
            t_ivl.set_min_max(uv_min.x, uv_max.x)?;
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
        let mut best_f = -1.0;
        let mut best_param = 0.5;

        // Don't sample right to the boundaries of the domain, pull the sampling
        // domain in a bit.
        let mut sample_domain = s_ivl;
        sample_domain.expand_relative(-0.025);

        // WR-57: the alignment of the unit non-singular tangent with the direction at
        // a singular parameter, `None` where the tangent is zero (undefined).
        let alignment_at = |s: f64| -> Result<Option<f64>> {
            let mut q = uv;
            if singular_u {
                q.x = s;
            } else {
                q.y = s;
            }
            let Some(m) = ignored(self.evaluate(k, q, 1, 1, flags(Side::Left, Side::Left, true)))?
            else {
                return Ok(None);
            };
            let surf_dt = if singular_u {
                at(&m, 0, 1)?
            } else {
                at(&m, 1, 0)?
            };
            Ok(if surf_dt.length_squared() < EFF_ZERO_SQ {
                None
            } else {
                Some(surf_dt.unitize()?.dot(s_dir))
            })
        };

        // WR-57: the guesses are parameters, not upstream's [0, 1] fractions. The
        // input parameter is the first near-best candidate when it is in the interval
        // and its tangent is defined: of nearly equal guesses the one nearest the
        // input is kept, and the input is nearest itself. (Where the alignment is the
        // same over a stretch of parameters, the Newton solve does not move from its
        // start, so a sample there would be returned instead of the input.)
        let mut near_best_f = -1.0;
        let mut near_best_s = sample_domain.evaluate(0.5);
        if s_ivl.contains_value(ds, 0.0) {
            if let Some(f) = alignment_at(ds)? {
                near_best_f = f;
                near_best_s = ds;
            }
        }

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
            let surf_dt = surf_dt.unitize()?;

            // measure how closely test point dir is to target dir
            let this_f = surf_dt.dot(s_dir);
            // (upstream's `AngleBetween` is for debugging only; its status is passed on
            // as the fork does)
            surf_dt.angle_between(s_dir)?;

            // save the best sample point
            if this_f > best_f {
                best_f = this_f;
                best_param = this_param;
            }

            // for nearly equal starting guesses - save the one nearest the input value
            let cand_s = sample_domain.evaluate(this_param);
            if this_f > near_best_f - f_tol
                && (this_f > near_best_f + f_tol || (cand_s - ds).abs() < (near_best_s - ds).abs())
            {
                near_best_f = this_f;
                near_best_s = cand_s;
            }
        }

        // No solution: all sample tangents point away from the given dir.
        if near_best_f < 0.0 && best_f < 0.0 {
            return Err(SolidError::surface_failed(
                OP_DEGEN,
                "every sample tangent points away from the direction",
            ));
        }

        // WR-57: an answer is a local maximum of the alignment `A(s)` on the closed
        // interval `s_ivl`, checked at the answer with the solve's own function
        // (`F = A'`, `F' = A''`, the solve's evaluation): an end where `A` grows
        // outward (`F < 0` at the minimum, `F > 0` at the maximum), or a stationary
        // point to the solve's accuracy (`|F| < tol`) whose curvature is not positive
        // (`F' <= SM_EFF_ZERO`, the solve's own zero for `F'`), or a knot where the
        // one-sided slopes are `+` on the left and `-` on the right. The solve finds where
        // `F` vanishes or changes sign, so it may end at a minimum, on a shoulder or
        // beside a kink; upstream accepts any answer it reports found. A stationary
        // answer takes one more Newton step when that step is still a maximum and
        // does not grow `|F|`: the solve stops at `|F| < SM_EFF_ZERO`, which bounds
        // the parameter only through `F'` (on a sphere in degrees, to `3.3e-9`). Each
        // sign is charged its rounding ([`slope_charges`], from the certified partials
        // of the surface's net): an end's and a kink's slopes must be proven, and a
        // stationary point's curvature counts as not positive unless it is proven
        // above `SM_EFF_ZERO`. Where the net has a certified evaluator but a charge
        // cannot be bounded (`|w|` not above its error near a collapsed corner, the
        // partials failing, a charge not finite) the charges are infinite: no sign is
        // proven there, so no end or kink is a maximum by these charges, and no
        // stationary point either (its curvature has no bound), except at a collapsed
        // corner (below: an exact pole's end by its limit slope, and upstream's
        // stationary test where the tangent is not defined to the rounding). The kinds
        // without a certified evaluator (they evaluate their own way) prove no end or
        // kink either (`F`'s
        // charge infinite), and keep the stationary test on the solve's own `F'`
        // (its charge zero): what they accept is upstream's stationary answer only.
        let certified = certified_alignment_evaluator(k, self)?;
        // A search asks for the certified slopes at the same parameters more than once (the
        // interval's ends, its chosen guess, the samples it re-examines as it brackets); they
        // are kept for the rest of this search ([`KeptSlopes`]) and reused: the same numbers,
        // the same answers, fewer evaluations.
        let kept = KeptSlopes::default();
        let charges_at = |t: f64, f: f64, f_prime: f64| -> (f64, f64, f64) {
            charges_or_unproven(
                certified.as_ref(),
                &kept,
                uv,
                s_dir,
                singular_dir,
                t,
                f,
                f_prime,
            )
        };
        let charges = |t: f64, f: f64, f_prime: f64| -> (f64, f64) {
            let (cf, cfp, _) = charges_at(t, f, f_prime);
            (cf, cfp)
        };
        let efo = super::p1::FindSingularParamWithGivenTangentDir::new(
            k,
            self,
            uv,
            s_dir,
            singular_dir,
            EFF_ZERO,
        );
        // (`F`, `F'` and their charges)
        let slope = |t: f64| -> Result<Option<(f64, f64, f64, f64)>> {
            Ok(ignored(efo.evaluate(t))?.map(|(f, f_prime, _)| {
                let (cf, cfp) = charges(t, f, f_prime);
                (f, f_prime, cf, cfp)
            }))
        };
        // WR-57: where the surface is closed in the singular direction the interval's
        // ends meet at the seam, so an end where `A` grows outward is not a maximum:
        // `A` keeps growing past the seam, from the other end. (The continuity is
        // asked for so that `IsClosed` also runs its test for short, large-radius
        // iso-curves, which are not closed.)
        let closed =
            ignored(self.is_closed(k, &domain, singular_dir, None, true))?.is_some_and(|(c, _)| c);
        let grows_outward =
            |t: f64, f: f64, cf: f64| (t <= s_ivl.min() && f < -cf) || (t >= s_ivl.max() && f > cf);
        // WR-57: at an end where the surface is exactly collapsed (a pole) `w` is zero, so
        // `A` and its slopes have no value there and no charge bounds them; `A` is then
        // taken at its limit from inside, and the end is a maximum where the limit of `F`
        // is proven to grow outward ([`collapsed_end_slope`])
        let [collapsed_min, collapsed_max] = [
            collapsed_end_slope(k, self, uv, s_dir, singular_dir, s_ivl.min(), true)?
                .is_some_and(|(f, e)| f < -e),
            collapsed_end_slope(k, self, uv, s_dir, singular_dir, s_ivl.max(), false)?
                .is_some_and(|(f, e)| f > e),
        ];
        let collapsed_grows_outward =
            |t: f64| (t <= s_ivl.min() && collapsed_min) || (t >= s_ivl.max() && collapsed_max);
        // WR-57: where `w` is not enclosed away from zero (a corner collapsed to the
        // rounding, where the tangent and `A` are not defined to it and no bound on `F'`
        // exists), the solve's own stationary answer is accepted as upstream accepts it
        let undefined_tangent = |t: f64| {
            certified
                .as_ref()
                .is_some_and(|e| tangent_not_enclosed(e, uv, singular_dir, t))
        };
        let is_local_max = |t: f64, (f, f_prime, cf, cfp): (f64, f64, f64, f64)| -> bool {
            (!closed && (grows_outward(t, f, cf) || collapsed_grows_outward(t)))
                || (f.abs() < tol
                    && if cfp.is_finite() {
                        f_prime - cfp <= scaled_zero()
                    } else {
                        undefined_tangent(t)
                    })
        };
        // the interior knots in the singular direction: where `A'` may jump (a kink)
        let knots: Vec<f64> = ignored(self.knots_with_nurb(k, singular_dir, Some(&s_ivl)))?
            .map(|(kv, _)| kv)
            .unwrap_or_default()
            .into_iter()
            .filter(|&x| x > s_ivl.min() && x < s_ivl.max())
            .collect();
        // (a one-sided slope's sign where it is proven: `Some(true)` for `+`,
        // `Some(false)` for `-`; its charge's gap term holds the side's difference)
        let side_sign = |s: f64, side: Side| -> Result<Option<bool>> {
            let Some(f) = ignored(alignment_slope(k, self, uv, s_dir, singular_dir, s, side))?
            else {
                return Ok(None);
            };
            // (the certified evaluator takes a knot's own span, so a one-sided slope is
            // charged at the neighbouring double on its side, plus the slope's drift
            // over that ulp, `|F'|` bounded there)
            let at = if side == Side::Right {
                certified::next_down(s)
            } else {
                certified::next_up(s)
            };
            let (cf, _, curvature) = charges_at(at, f, 0.0);
            let cf = (cf + 2.0 * curvature * (at - s).abs()) * (1.0 + gamma(4));
            Ok(if f > cf {
                Some(true)
            } else if f < -cf {
                Some(false)
            } else {
                None
            })
        };
        let interior_max = |t: f64| -> Result<Option<f64>> {
            let Some(at_t) = slope(t)? else {
                return Ok(None);
            };
            let (f, f_prime, _, _) = at_t;
            if !is_local_max(t, at_t) {
                // a kink: `A` rises from `t` (`F(t)`, the slope on `t`'s right, gives
                // the side) to the first knot that way, where its one-sided slopes are
                // `+` on the left and `-` on the right; the solve's bisection stops
                // beside it
                let kink = if f > 0.0 {
                    knots.iter().copied().find(|&x| x > t)
                } else if f < 0.0 {
                    knots.iter().rev().copied().find(|&x| x <= t)
                } else {
                    None
                };
                if let Some(x) = kink {
                    // (`Side::Right` is the span `(x_prev, x]`: the limit from the left)
                    let left = side_sign(x, Side::Right)?;
                    let right = side_sign(x, Side::Left)?;
                    if left == Some(true) && right == Some(false) {
                        return Ok(Some(x));
                    }
                }
                return Ok(None);
            }
            if f.abs() < tol && f_prime < -scaled_zero() {
                let polished = s_ivl.clamp_value(t - f / f_prime);
                if let Some(at_p) = slope(polished)? {
                    if at_p.0.abs() <= f.abs() && is_local_max(polished, at_p) {
                        return Ok(Some(polished));
                    }
                }
            }
            Ok(Some(t))
        };
        // WR-57: on a closed direction an answer at an end where `A` grows outward
        // continues across the seam: the seam is a maximum where `A` also grows toward
        // it at the other end (a crease), otherwise the solve restarts at the other
        // end and its answer is checked as any other
        let local_max = |t: f64| -> Result<Option<f64>> {
            if closed {
                if let Some((f, _, cf, _)) = slope(t)? {
                    if grows_outward(t, f, cf) {
                        let other = if t <= s_ivl.min() {
                            s_ivl.max()
                        } else {
                            s_ivl.min()
                        };
                        if let Some((of, _, ocf, _)) = slope(other)? {
                            if grows_outward(other, of, ocf) {
                                return Ok(Some(t));
                            }
                        }
                        let restart = singular_param_solve(
                            k,
                            self,
                            uv,
                            s_dir,
                            singular_dir,
                            &s_ivl,
                            other,
                            tol,
                        );
                        return match ignored(restart)? {
                            Some((_, t2)) => interior_max(t2),
                            None => Ok(None),
                        };
                    }
                }
            }
            interior_max(t)
        };

        // solve with NewtonRaphson iteration starting at the near best guess, then
        // the best sample (WR-57: the parameters where the alignment was measured;
        // upstream starts at `s_ivl.evaluate` of the fraction, which for 0 or 1 is the
        // domain boundary, possibly a collapsed corner)
        let first =
            singular_param_solve(k, self, uv, s_dir, singular_dir, &s_ivl, near_best_s, tol);
        let (stat_ok, first_max) = match ignored(first)? {
            Some((_, t)) => (true, local_max(t)?),
            None => (false, None),
        };
        let best_s = sample_domain.evaluate(best_param);
        // WR-57: the answer is the most aligned of the maxima the solve reaches from
        // the near best guess and from the best sample, and of the interval's ends
        // where they are maxima (an end's alignment growing outward), the first on a
        // tie within rounding (`SM_EFF_ZERO`), so on a plateau the answer from the
        // input stays. Of nearly equal guesses upstream keeps the one nearest the
        // input (`f_tol`), but two maxima are not two guesses: a maximum less aligned
        // than another is not the largest. Where none is found the solve also starts
        // at each of upstream's ten starts and at each sample.
        let first_settled = match first_max {
            Some(t) => alignment_at(t)?.is_some_and(|a| a >= best_f - scaled_zero()),
            None => false,
        };
        let mut candidates: Vec<f64> = first_max.into_iter().collect();
        let second = singular_param_solve(k, self, uv, s_dir, singular_dir, &s_ivl, best_s, tol);
        // (status ignored upstream; passed on as the fork does where upstream runs this
        // solve, the first not settled)
        let second = if first_settled {
            ignored(second)?
        } else {
            Some(second?)
        };
        if let Some((_, t)) = second {
            candidates.extend(local_max(t)?);
        }
        for end in [s_ivl.min(), s_ivl.max()] {
            candidates.extend(local_max(end)?);
        }
        // the most aligned candidate, the first on a tie within rounding
        let most_aligned = |cands: &[f64]| -> Result<Option<(f64, f64)>> {
            let mut best: Option<(f64, f64)> = None;
            for &c in cands {
                if let Some(a) = alignment_at(c)? {
                    if best.is_none_or(|(_, ba)| a > ba + scaled_zero()) {
                        best = Some((c, a));
                    }
                }
            }
            Ok(best)
        };
        // a maximum less aligned than the best sample is not the largest
        let below_best_sample =
            |best: Option<(f64, f64)>| best.is_none_or(|(_, a)| a < best_f - scaled_zero());
        let full_start = |ii: usize| s_ivl.evaluate(ii as f64 / (num_guesses - 1) as f64);
        let mut best = most_aligned(&candidates)?;
        if below_best_sample(best) && stat_ok {
            for ii in 0..num_guesses {
                let starts = [
                    full_start(ii),
                    sample_domain.evaluate(ii as f64 / (num_guesses - 1) as f64),
                ];
                for start in starts {
                    let r =
                        singular_param_solve(k, self, uv, s_dir, singular_dir, &s_ivl, start, tol);
                    if let Some((_, t)) = ignored(r)? {
                        candidates.extend(local_max(t)?);
                    }
                }
            }
            // a peak narrower than the starts' spacing: between neighbouring
            // parameters of the ends, starts, samples and knots, where the slope from
            // the right of the lower is `+` and from the left of the upper `-`, a
            // maximum lies between (`A'` is continuous inside a knot span); the solve
            // runs on that bracket from its middle
            let mut points: Vec<f64> = vec![s_ivl.min(), s_ivl.max()];
            for ii in 0..num_guesses {
                points.push(full_start(ii));
                points.push(sample_domain.evaluate(ii as f64 / (num_guesses - 1) as f64));
            }
            points.extend(knots.iter().copied());
            points.retain(|x| x.is_finite());
            points.sort_by(f64::total_cmp);
            points.dedup();
            for pair in points.windows(2) {
                let [a, b] = [pair.first().copied(), pair.get(1).copied()];
                let (Some(a), Some(b)) = (a, b) else {
                    continue;
                };
                let rises = side_sign(a, Side::Left)? == Some(true);
                if !rises || side_sign(b, Side::Right)? != Some(false) {
                    continue;
                }
                let mut bracket = Extent1d::new();
                if ignored(bracket.set_min_max(a, b))?.is_none() {
                    continue;
                }
                let r = singular_param_solve(
                    k,
                    self,
                    uv,
                    s_dir,
                    singular_dir,
                    &bracket,
                    bracket.mid(),
                    tol,
                );
                if let Some((_, t)) = ignored(r)? {
                    candidates.extend(local_max(t)?);
                }
            }
            best = most_aligned(&candidates)?;
        }
        let answer = if below_best_sample(best) {
            None
        } else {
            best.map(|(t, _)| t)
        };

        // Leave our best value in the return arg (WR-57: where no maximum at least as
        // aligned as the best sample was found, the most aligned of the best maximum
        // found, the best sample and upstream's return, the best sample's fraction
        // mapped onto the whole interval)
        let found = answer.is_some();
        let value = if let Some(t) = answer {
            t
        } else {
            let mut fallback: Vec<f64> = vec![best_s, s_ivl.evaluate(best_param)];
            fallback.extend(best.map(|(t, _)| t));
            most_aligned(&fallback)?.map_or(best_s, |(t, _)| t)
        };
        let mut out = uv;
        if singular_u {
            out.x = value;
        } else {
            out.y = value;
        }

        // the roots from upstream's starts, for the inversion
        let mut roots = Vec::new();
        if with_roots {
            for ii in 0..num_guesses {
                let r = singular_param_solve(
                    k,
                    self,
                    uv,
                    s_dir,
                    singular_dir,
                    &s_ivl,
                    full_start(ii),
                    tol,
                );
                if let Some((true, t)) = ignored(r)? {
                    roots.push(t);
                }
            }
        }
        Ok((
            DegenParam {
                uv: out,
                converged: stat_ok && found,
            },
            roots,
        ))
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
        let surface_tree = cache
            .tree()
            .ok_or_else(|| SolidError::surface_missing(OP_SOLVE, "the surface cache's tree"))?;

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
            .ok_or_else(|| SolidError::surface_missing(OP_SOLVE, "the surface cache's tree"))?;
        let curve_cache = match crate::curve::curve::p5::make_curve_cache::<O>(k, curve, handles[0])
        {
            Ok(c) => c,
            Err(e)
                if e.is_unported_or_cancelled()
                    || matches!(e, SolidError::PreparationRequired { .. }) =>
            {
                return Err(e);
            }
            Err(_) => return Err(SolidError::surface_missing(OP_SOLVE, "the curve cache")),
        };
        let curve_tree = curve_cache
            .tree()
            .ok_or_else(|| SolidError::surface_missing(OP_SOLVE, "the curve cache's tree"))?;

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
        let tree1 = cache1.tree().ok_or_else(|| {
            SolidError::surface_missing(OP_SOLVE, "the first surface cache's tree")
        })?;
        let tree2 = cache2.tree().ok_or_else(|| {
            SolidError::surface_missing(OP_SOLVE, "the second surface cache's tree")
        })?;

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
            if e.is_unported_or_cancelled() {
                e
            } else {
                SolidError::surface_missing(OP_PROJ, "the surface cache")
            }
        };
        match context.as_mut() {
            Some((context, handle)) => context
                .build_surface_cache(k, self, *handle)
                .map_err(ner)?
                .surface_continuity(uv_domain)?,
            None => super::p2::surface_cache::<()>(k, self, None)
                .map_err(ner)?
                .surface_continuity(uv_domain)?,
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
        return Err(SolidError::surface_missing(OP_SOLVE, "the target distance"));
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
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        Err(_) => return Err(SolidError::surface_missing(OP_SOLVE, "the surface cache")),
    };
    match cache.get_tree(k) {
        Ok(_) => Ok(cache),
        Err(e) if e.is_unported_or_cancelled() => Err(e),
        Err(_) => Err(SolidError::surface_missing(
            OP_SOLVE,
            "the surface cache's tree",
        )),
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
        Err(e)
            if e.is_unported_or_cancelled()
                || matches!(e, SolidError::PreparationRequired { .. }) =>
        {
            return Err(e);
        }
        Err(_) => return Err(SolidError::surface_missing(OP_SOLVE, "the curve cache")),
    };
    match cache.get_tree(k) {
        Ok(_) => Ok(cache),
        Err(e)
            if e.is_unported_or_cancelled()
                || matches!(e, SolidError::PreparationRequired { .. }) =>
        {
            Err(e)
        }
        Err(_) => Err(SolidError::surface_missing(
            OP_SOLVE,
            "the curve cache's tree",
        )),
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
        return Err(SolidError::surface_missing(OP_SOLVE, "the target distance"));
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
/// `uv_domain` (`ApproxDerivativeLengths`), averaged over the samples that evaluate
/// (`SM_UNDEF_DOUBLE` when none does), as the fork.
///
/// # Errors
///
/// Only [`SolidError::Unported`] (upstream ignores the evaluation statuses).
pub fn approx_derivative_lengths<S: SurfaceEvaluate + ?Sized>(
    k: &Kernel,
    surface: &S,
    uv_domain: &Extent2d,
) -> Result<Vec2> {
    // Method: simple, just evaluate on a grid (4 x 4) and average.
    let samples = [0.15, 0.35, 0.65, 0.85];
    let mut mag_u = 0.0;
    let mut mag_v = 0.0;
    let mut d = (Vec3::UNDEF, Vec3::UNDEF, Vec3::UNDEF);
    // the samples that evaluate are averaged, `SM_UNDEF_DOUBLE` when none does, as the
    // fork (WR-663, tangency-labs/smlib 21dcf69)
    let mut num_evaluated: u32 = 0;
    // an average, a scale for tolerances: a procedural blend's net answers it (#2510)
    let _net = NetScope::new(k);

    for &si in &samples {
        for &sj in &samples {
            let uv = uv_domain.evaluate(si, sj);
            let evaluated = eval_1st_keep(k, surface, uv, Side::Left, Side::Left, &mut d)?;
            if !evaluated {
                continue;
            }
            mag_u += d.1.length();
            mag_v += d.2.length();
            num_evaluated += 1;
        }
    }

    if num_evaluated == 0 {
        return Ok(Vec2::new(UNDEF_DOUBLE, UNDEF_DOUBLE));
    }
    mag_u /= f64::from(num_evaluated);
    mag_v /= f64::from(num_evaluated);
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
    // (status ignored upstream; passed on as the fork does, WR-663,
    // tangency-labs/smlib 21dcf69)
    let dir_uv = dir_uv.unitize()?;

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
        // (status ignored upstream; passed on as the fork does, WR-663,
        // tangency-labs/smlib 21dcf69)
        if let Some(s) = drop_vectors(du, dv, &[diff])?.first().copied() {
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
    // (the point evaluation's status is passed on, as the fork does, WR-663,
    // tangency-labs/smlib 21dcf69)
    if !ok {
        pt = surface.evaluate_point(k, close_uv)?;
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
                pt = surface.evaluate_point(k, uv)?;
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
    // a point that cannot be evaluated is not shown converged, as the fork (WR-663,
    // tangency-labs/smlib 21dcf69)

    // surface eval
    let mut d = (Vec3::UNDEF, Vec3::UNDEF, Vec3::UNDEF);
    if !eval_1st_keep(k, surface, uv, Side::Left, Side::Left, &mut d)? {
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
        None => return Ok(false),
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
    // a point that cannot be evaluated is not shown converged, as the fork (WR-663,
    // tangency-labs/smlib 21dcf69)
    let mut d1 = (Vec3::UNDEF, Vec3::UNDEF, Vec3::UNDEF);
    if !eval_1st_keep(k, surface, uv, Side::Left, Side::Left, &mut d1)? {
        return Ok(false);
    }
    let mut d2 = (Vec3::UNDEF, Vec3::UNDEF, Vec3::UNDEF);
    if !eval_1st_keep(k, surface2, uv2, Side::Left, Side::Left, &mut d2)? {
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
        return Err(SolidError::surface_failed(
            OP_SEAM,
            "the point is at a pole",
        ));
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
    // (both statuses are passed on, as the fork does, WR-663, tangency-labs/smlib
    // 21dcf69)
    let pt_der = evaluate_step(k, this_surf, this_uv)?;

    let (_, other_u, other_v) =
        evaluate_1st_derivatives(k, other_surf, other_uv, Side::Left, Side::Left)?;

    // Unitize them, so that dot-product comparisons mean direction only.
    let mut other_u = other_u.unitize().map_err(|_| {
        SolidError::surface_failed(OP_SEAM, "the other surface's u tangent has zero length")
    })?;
    let mut other_v = other_v.unitize().map_err(|_| {
        SolidError::surface_failed(OP_SEAM, "the other surface's v tangent has zero length")
    })?;

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

/// What [`drop_result_verdict`] makes of a local point drop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropVerdict {
    /// A foot the local solve converged to at its guess (or right on the point): the
    /// drop's answer, without the global solve.
    Foot,
    /// A foot the local solve converged to away from its guess, passing every test but
    /// the last (not upstream, WR-56, #17773): the global solve decides, and the caller
    /// may keep it as a candidate.
    MovedFoot,
    /// Not a converged foot, or a suspicious one: the global solve answers.
    Refused,
}

/// How a local point drop from `guess_uv` (solution value `sol_value` at `sol_uv`)
/// converged (`sm_GoodDropResult`, which returns whether it is [`DropVerdict::Foot`]).
///
/// Not upstream (WR-56; the fork's tangency-labs/smlib@0b4de8c / eaa998a, fork PR #70):
/// - its last test takes only a result at the guess itself (upstream compares the
///   result's and the guess's distances with their names swapped, so it took a result
///   farther than its guess and refused one nearer; in either order a result the local
///   solve moved to can be a local minimum that is not the nearest point, #17773): any
///   other result passing every test is [`DropVerdict::MovedFoot`], for the global
///   solve to decide;
/// - a result is taken as the foot only when the gap's components along the unit
///   tangents sum under the local solve's tight tolerance `EFF_ZERO * (1 + max|P|)`
///   (upstream: under 1e-4), and an interior result with either component over it is
///   refused (upstream: over 1e-3), so a result the local solve did not converge to
///   goes to the global solve. Next to a pole a converged result can fail this test
///   (the component along the unit short tangent divides the solve's residual by that
///   tangent's length); it then goes to the global solve too, which answers the drop
///   as it does without a guess;
/// - not upstream or the fork: a result at the guess itself is taken without measuring
///   the guess again (upstream's `EvaluatePoint` can round one ulp away from the result's
///   evaluation, which then measured the result farther than itself).
///
/// # Errors
///
/// Only [`SolidError::Unported`] (upstream ignores the evaluation statuses).
pub fn drop_result_verdict<S: SurfaceEvaluate + ?Sized>(
    k: &Kernel,
    surface: &S,
    domain: &Extent2d,
    pt_to_drop: Point3,
    guess_uv: Point2,
    sol: &SolutionEnd,
) -> Result<DropVerdict> {
    // Check whether it's right on.
    if sol.solution_value < EFF_ZERO {
        return Ok(DropVerdict::Foot);
    }

    let result_uv = Point2::new(sol.param(0)?, sol.param(1)?);

    // If result is far from guess, it's suspicious.
    let max_step = 0.50;
    let uv_step = result_uv - guess_uv;
    if uv_step.x.abs() > max_step * domain.x_length() {
        return Ok(DropVerdict::Refused);
    }
    if uv_step.y.abs() > max_step * domain.y_length() {
        return Ok(DropVerdict::Refused);
    }

    // Evaluate the error functions: how far from the surface normal.
    // (a drop that cannot be evaluated is not shown good, as the fork, WR-663,
    // tangency-labs/smlib 21dcf69; the fork keeps the dropped `Unitize` statuses)
    let mut d = (Vec3::UNDEF, Vec3::UNDEF, Vec3::UNDEF);
    if !eval_1st_keep(k, surface, result_uv, Side::Left, Side::Left, &mut d)? {
        return Ok(DropVerdict::Refused);
    }
    let (srf_point, su, sv) = d;
    let su = su.unitize_or_z();
    let sv = sv.unitize_or_z();
    let gap = pt_to_drop - srf_point;
    let fu = gap.dot(su);
    let fv = gap.dot(sv);
    // A foot is one the local solve converged to by its own tight tolerance,
    // `LocalPointSolve_1`'s (WR-56; upstream: |Fu| + |Fv| < 1e-4).
    let tight_tol = EFF_ZERO * (1.0 + pt_to_drop.max_dimension());
    if fu.abs() + fv.abs() < tight_tol {
        return Ok(DropVerdict::Foot);
    }

    // Check u-param first.
    if result_uv.x < domain.min().x + EFF_ZERO {
        if fu > EFF_ZERO {
            return Ok(DropVerdict::Refused);
        }
    } else if result_uv.x > domain.max().x - EFF_ZERO {
        if fu < -EFF_ZERO {
            return Ok(DropVerdict::Refused);
        }
    } else if fu.abs() > tight_tol {
        return Ok(DropVerdict::Refused); // interior, bad fcn value.
    }

    // Now check v-param.
    if result_uv.y < domain.min().y + EFF_ZERO {
        if fv > EFF_ZERO {
            return Ok(DropVerdict::Refused);
        }
    } else if result_uv.y > domain.max().y - EFF_ZERO {
        if fv < -EFF_ZERO {
            return Ok(DropVerdict::Refused);
        }
    } else if fv.abs() > tight_tol {
        return Ok(DropVerdict::Refused); // interior, bad fcn value.
    }

    // One more: upstream refuses a result whose 3d point is farther than the guess's,
    // with the two distances' names swapped, so it took a result farther than its
    // guess and sent one nearer to the global solve. Not upstream (WR-56, #17773):
    // neither order shows a result the local solve moved to is the nearest point (a
    // nearer one can be a local minimum the global solve beats), so only a result at
    // the guess itself is taken here; any other goes to the global solve.
    if guess_uv != result_uv {
        return Ok(DropVerdict::MovedFoot);
    }

    Ok(DropVerdict::Foot)
}

/// Whether a local point drop from `guess_uv` converged well enough to skip the
/// global solve (`sm_GoodDropResult`): [`drop_result_verdict`] is
/// [`DropVerdict::Foot`].
///
/// # Errors
///
/// As [`drop_result_verdict`].
pub fn good_drop_result<S: SurfaceEvaluate + ?Sized>(
    k: &Kernel,
    surface: &S,
    domain: &Extent2d,
    pt_to_drop: Point3,
    guess_uv: Point2,
    sol: &SolutionEnd,
) -> Result<bool> {
    Ok(drop_result_verdict(k, surface, domain, pt_to_drop, guess_uv, sol)? == DropVerdict::Foot)
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
        let cache = self
            .surface_cache
            .ok_or_else(|| SolidError::surface_missing(OP_MAY, "the surface cache"))?;
        if !is_node_valid(self.base.tree(1)?.owner(), cache, surface_node)? {
            return Ok(false);
        }

        // If we are normalizing and have a node with a surface do the angular test
        // using the Gauss map of the normal.
        if self.base.operation == SolverOperation::Normalize {
            // When we have a child SurfacePatch node
            if surface_node.aux_type == AuxDataType::BezierSurface && surface_node.child1.is_some()
            {
                let surface_id = branch.get(1).copied().flatten().ok_or_else(|| {
                    SolidError::surface_missing(OP_MAY, "the branch's surface node")
                })?;
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
                            tmp_polar.add_vector(vec, None);
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
                        return Err(SolidError::surface_missing(
                            OP_SOLVE,
                            "a branch's surface node",
                        ));
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

/// The slope `F = A'(s)` of `FindDegenParamForDirection`'s alignment
/// `A(s) = unit(w(s)) . dir` (`w` the derivative across the singular direction) from
/// `side` of `s` (at a knot, [`Side::Left`]'s span `[s, ..)` gives the limit from the
/// right, [`Side::Right`]'s `(.., s]` the limit from the left): the solve's own function
/// ([`super::p1::FindSingularParamWithGivenTangentDir::evaluate`], which takes
/// `Side::Left`), here with either side, for the one-sided slopes at a knot (WR-57).
///
/// # Errors
///
/// Those of the evaluation; `SM_ERR` where `w` is zero (the tangent undefined).
fn alignment_slope(
    k: &Kernel,
    surface: &Surface,
    uv: Point2,
    dir: Vec3,
    singular_dir: SurfParamType,
    s: f64,
    side: Side,
) -> Result<f64> {
    // (only the singular parameter's side changes; the other is the solve's, left)
    let (q, w, ws, wss, sides) = if singular_dir == SurfParamType::U {
        (
            Point2::new(s, uv.y),
            (0, 1),
            (1, 1),
            (2, 1),
            (side, Side::Left),
        )
    } else {
        (
            Point2::new(uv.x, s),
            (1, 0),
            (1, 1),
            (1, 2),
            (Side::Left, side),
        )
    };
    let m = surface.evaluate(k, q, 3, 3, flags(sides.0, sides.1, true))?;
    let derivs = at(&m, w.0, w.1)?.unitized_derivative2(at(&m, ws.0, ws.1)?, at(&m, wss.0, wss.1)?);
    let Ok([unit_t, _]) = derivs else {
        return Err(SolidError::surface_failed(
            OP_DEGEN,
            "the unit derivatives of the singular direction are undefined",
        ));
    };
    Ok(unit_t.dot(dir))
}

/// The curve-bounded layers [`certified_alignment_evaluator`] looks through to a basis
/// (well above any nesting the fixtures reach, which is one).
const BASIS_DEPTH: usize = 16;

/// The certified evaluator ([`crate::eval::collapse::certified_partials`]) of the net
/// whose evaluation is `surface`'s ([`super::p8::evaluation_net`]), for the rounding
/// charged to `FindDegenParamForDirection`'s sign tests (WR-57); `None` for the kinds
/// that evaluate their own way, a power-basis form or a net that is not finite or has
/// a weight that is not positive. A curve-bounded surface evaluates on its basis
/// (`SmCurveBoundedSurface` passes every evaluation along), so its evaluator is its
/// basis's, through at most [`BASIS_DEPTH`] curve-bounded layers (`None` past them).
///
/// # Errors
///
/// Those of making the surface's NURB.
pub fn certified_alignment_evaluator(k: &Kernel, surface: &Surface) -> Result<Option<EvalSurface>> {
    Ok(alignment_net(k, surface)?
        .map(|n| EvalSurface::nurbs_closed(n.degree, n.knots, n.count, n.ctrl, [false; 2])))
}

/// The net [`certified_alignment_evaluator`] evaluates: degrees, knots, counts and the
/// homogeneous control points (`ctrl[j·count[0] + i]`).
struct AlignmentNet {
    degree: [usize; 2],
    knots: [Vec<f64>; 2],
    count: [usize; 2],
    ctrl: Vec<[f64; 4]>,
}

/// [`certified_alignment_evaluator`]'s net, `None` where it has none.
fn alignment_net(k: &Kernel, surface: &Surface) -> Result<Option<AlignmentNet>> {
    let mut surface = surface;
    let mut layers = 0;
    while let SurfaceKind::CurveBounded(c) = surface.kind() {
        if layers == BASIS_DEPTH {
            return Ok(None);
        }
        layers += 1;
        surface = c.basis();
    }
    let Some(net) = super::p8::evaluation_net(k, surface)? else {
        return Ok(None);
    };
    if net.form() == BSplineSurfaceForm::Polynomial {
        return Ok(None);
    }
    let Some(nurb) = net.gw_nurb() else {
        return Ok(None);
    };
    let cp = nurb.cpts();
    let nu = cp.len();
    let nv = cp.first().map_or(0, Vec::len);
    let mut ctrl = Vec::new();
    for j in 0..nv {
        for row in &cp {
            let Some(c) = row.get(j) else {
                return Ok(None);
            };
            let w = if c.w == NOW { 1.0 } else { c.w };
            let z = if c.z == NOZ { 0.0 } else { c.z };
            let h = [c.x, c.y, z, w];
            if w <= 0.0 || h.iter().any(|x| !x.is_finite()) {
                return Ok(None);
            }
            ctrl.push(h);
        }
    }
    Ok(Some(AlignmentNet {
        degree: [nurb.p(), nurb.q()],
        knots: [nurb.u_knots().to_vec(), nurb.v_knots().to_vec()],
        count: [nu, nv],
        ctrl,
    }))
}

/// `a b` exactly, as `fl(a b)` and the remainder `a b - fl(a b)` (exact by a fused
/// multiply-add); `None` where the remainder may underflow or the product overflow.
fn exact_product(a: f64, b: f64) -> Option<(f64, f64)> {
    if a == 0.0 || b == 0.0 {
        return Some((0.0, 0.0));
    }
    let p = a * b;
    // (|a b| at least 2^-968 keeps the remainder, a multiple of ulp(a) ulp(b), normal)
    (p.is_finite() && p.abs() >= f64::from_bits(0x0370_0000_0000_0000))
        .then(|| (p, a.mul_add(b, -p)))
}

/// Whether the iso-curve of `net` at the end `e` of the singular direction (`singular_dir`'s
/// parameter is `s`; `at_min` for the interval's start) is exactly one point, so that the
/// derivative `w` across the singular direction is exactly zero there (WR-57, a collapsed
/// corner): the knots are clamped at `e` (its first or last `degree + 1` knots are `e`), so
/// the iso-curve is the rational curve of that end's row of control points, and every point
/// of the row is exactly the first's (`x_j / w_j = x_0 / w_0`, compared as the exact products
/// `x_j w_0` and `x_0 w_j`). `false` where any of that is not proven.
fn exactly_collapsed(
    net: &AlignmentNet,
    singular_dir: SurfParamType,
    e: f64,
    at_min: bool,
) -> bool {
    let (dir, other) = if singular_dir == SurfParamType::U {
        (0, 1)
    } else {
        (1, 0)
    };
    let (Some(knots), Some(&p), Some(&n), Some(&m)) = (
        net.knots.get(dir),
        net.degree.get(dir),
        net.count.get(dir),
        net.count.get(other),
    ) else {
        return false;
    };
    let clamped = if at_min {
        knots.len() > p && knots.iter().take(p + 1).all(|&x| x == e)
    } else {
        knots.len() > p && knots.iter().rev().take(p + 1).all(|&x| x == e)
    };
    if !clamped || n == 0 || m == 0 {
        return false;
    }
    let row = if at_min { 0 } else { n - 1 };
    let [nu, _] = net.count;
    let point = |j: usize| -> Option<[f64; 4]> {
        let (i_u, i_v) = if dir == 0 { (row, j) } else { (j, row) };
        net.ctrl
            .get(i_v.checked_mul(nu)?.checked_add(i_u)?)
            .copied()
    };
    let Some([x0, y0, z0, w0]) = point(0) else {
        return false;
    };
    (1..m).all(|j| {
        point(j).is_some_and(|[x, y, z, w]| {
            [(x, x0), (y, y0), (z, z0)].iter().all(|&(c, c0)| {
                match (exact_product(c, w0), exact_product(c0, w)) {
                    (Some(a), Some(b)) => a == b,
                    _ => false,
                }
            })
        })
    })
}

/// The certified limit of `FindDegenParamForDirection`'s slope `F = A'(s)` at an end `e`
/// of the singular direction's interval where the surface is exactly collapsed
/// ([`exactly_collapsed`]), with its error bound (WR-57). There `w(e) = 0`, so
/// `w(s) = (s - e) g(s)` with `g` smooth on the end's knot span and `g(e) = w_s(e)`,
/// `g'(e) = w_ss(e) / 2`; where `w_s(e)` is proven non-zero the unit tangent tends to
/// `σ g(e) / |g(e)|` (`σ = +1` from inside at the start, `-1` at the end) and `F` to
/// `σ (P w_ss(e) / 2) / |w_s(e)| . dir` (`P = I - T T^T`), the formula of
/// [`alignment_slopes`]' `F` with `(w, w')` replaced by `(w_s, w_ss / 2)`, charged the same
/// way. That limit proven negative at the start (positive at the end) makes `A`, extended
/// to `e` by its limit, strictly decrease into the interval near `e`: the end is a strict
/// local maximum. `None` where `e` is not exactly collapsed, the partials fail or
/// `|w_s(e)|` is not above its error.
pub fn collapsed_end_slope(
    k: &Kernel,
    surface: &Surface,
    uv: Point2,
    dir: Vec3,
    singular_dir: SurfParamType,
    e: f64,
    at_min: bool,
) -> Result<Option<(f64, f64)>> {
    let Some(net) = alignment_net(k, surface)? else {
        return Ok(None);
    };
    if !exactly_collapsed(&net, singular_dir, e, at_min) {
        return Ok(None);
    }
    let (q, ws, wss) = if singular_dir == SurfParamType::U {
        ([e, uv.y], (1, 1), (2, 1))
    } else {
        ([uv.x, e], (1, 1), (1, 2))
    };
    let eval = EvalSurface::nurbs_closed(net.degree, net.knots, net.count, net.ctrl, [false; 2]);
    let Ok((d, err)) = certified_partials(&eval, q) else {
        return Ok(None);
    };
    let get = |(i, j): (usize, usize)| -> Option<(Vec3, f64)> {
        Some((*d.get(i)?.get(j)?, *err.get(i)?.get(j)?))
    };
    let (Some((a, ea)), Some((b2, eb2))) = (get(ws), get(wss)) else {
        return Ok(None);
    };
    // (halving is exact)
    let (b, eb) = (b2 * 0.5, eb2 * 0.5);
    let r = a.length();
    if r <= ea || r.is_nan() {
        return Ok(None);
    }
    let t = a / r;
    let t1 = (b - t * t.dot(b)) / r;
    let sigma = if at_min { 1.0 } else { -1.0 };
    let fc = sigma * t1.dot(dir);
    // (alignment_slopes' bound on `F`, and its own rounding)
    let rl = r - ea;
    let delta = 2.0 * ea / r;
    let bl = b.length();
    let tau = bl / r;
    let d1 = (eb + (2.0 + delta) * (bl + eb) * delta) / rl + tau * ea / rl;
    let own = gamma(64) * (3.0 * tau * tau + tau + 1.0);
    let error = (d1 + own) * (1.0 + gamma(16));
    Ok((fc.is_finite() && error.is_finite()).then_some((fc, error)))
}

/// The rounding charges of `FindDegenParamForDirection`'s `F = A'(s)` and
/// `F' = A''(s)` at `s` (WR-57): bounds on `|F_exact - f|` and `|F'_exact - f_prime|`
/// for the solve's computed `f` and `f_prime` there, from the certified partials
/// `w`, `w'`, `w''` (`w` the derivative across the singular direction, `'` along it)
/// with their error bounds `ea`, `eb`, `ec`: each unit derivative's perturbation is
/// bounded by its terms' (`|a / |a| - b / |b|| <= 2 |a - b| / |a|`, a projection
/// `I - T T^T` by `(2 + d) d` for `d` that bound, the products'
/// and quotients' by their factors'), and the gap between the certified value and the
/// solve's own is added (they are both computed: the exact value is within the bound
/// of the first, so within that plus the gap of the second; a one-sided evaluation at
/// a knot that differs from the certified side shows up in that gap), and a bound on
/// `|F'_exact|` at `s`, from [`alignment_slopes`]' certified values there. `None` where a
/// charge is not finite.
fn slope_charges(e: &AlignmentSlopes, f: f64, f_prime: f64) -> Option<(f64, f64, f64)> {
    let scale = 1.0 + gamma(16);
    let cf = (e.f_error + (e.f - f).abs()) * scale;
    let cfp = (e.f_prime_error + (e.f_prime - f_prime).abs()) * scale;
    let curvature = (e.f_prime.abs() + e.f_prime_error) * scale;
    (cf.is_finite() && cfp.is_finite() && curvature.is_finite()).then_some((cf, cfp, curvature))
}

/// The certified slopes one search has evaluated, by the bits of their parameter: they
/// depend on the net, `uv`, `dir` and the singular direction (fixed for the search) and
/// the parameter alone, not on the solve's `f`/`f_prime`, so a parameter asked about
/// again reuses the same numbers.
type KeptSlopes = std::cell::RefCell<Vec<(u64, AlignmentSlopes)>>;

/// [`alignment_slopes`] at `s`, from `kept` where this search already evaluated them
/// there (an evaluation that fails is not kept, and is retried: it fails again).
fn kept_alignment_slopes(
    kept: &KeptSlopes,
    eval: &EvalSurface,
    uv: Point2,
    dir: Vec3,
    singular_dir: SurfParamType,
    s: f64,
) -> Option<AlignmentSlopes> {
    let key = s.to_bits();
    let known = kept
        .try_borrow()
        .ok()
        .and_then(|k| k.iter().find(|(at, _)| *at == key).map(|(_, e)| *e));
    if known.is_some() {
        return known;
    }
    let e = alignment_slopes(eval, uv, dir, singular_dir, s)?;
    if let Ok(mut k) = kept.try_borrow_mut() {
        k.push((key, e));
    }
    Some(e)
}

/// [`slope_charges`] where `eval` (the net's certified evaluator) is given, infinite
/// charges where it cannot bound them (no sign is proven then, and no curvature is
/// bounded); for the kinds without a certified evaluator, which evaluate their own way,
/// an infinite charge on `F` and the drift (no end or kink sign is proven) and none on
/// `F'` (the stationary test is the solve's own) (WR-57). The certified slopes are taken
/// from `kept`, the search's, where it has them at `s`.
#[allow(clippy::too_many_arguments)]
fn charges_or_unproven(
    eval: Option<&EvalSurface>,
    kept: &KeptSlopes,
    uv: Point2,
    dir: Vec3,
    singular_dir: SurfParamType,
    s: f64,
    f: f64,
    f_prime: f64,
) -> (f64, f64, f64) {
    match eval {
        Some(e) => kept_alignment_slopes(kept, e, uv, dir, singular_dir, s)
            .and_then(|e| slope_charges(&e, f, f_prime))
            .unwrap_or((f64::INFINITY, f64::INFINITY, f64::INFINITY)),
        None => (f64::INFINITY, 0.0, f64::INFINITY),
    }
}

/// The certified slopes of `FindDegenParamForDirection`'s alignment `A(s)` (WR-57):
/// [`alignment_slopes`]' values of `F = A'(s)` and `F' = A''(s)` and their error
/// bounds, `|F_exact - f| <= f_error` and `|F'_exact - f_prime| <= f_prime_error`
/// (before the rounding of adding them, at most `gamma(16)` relative).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AlignmentSlopes {
    /// The certified evaluation of `F = A'(s)`.
    pub f: f64,
    /// The bound on `F`'s error.
    pub f_error: f64,
    /// The certified evaluation of `F' = A''(s)`.
    pub f_prime: f64,
    /// The bound on `F'`'s error.
    pub f_prime_error: f64,
}

/// The certified `F = A'(s)` and `F' = A''(s)` of `FindDegenParamForDirection`'s
/// alignment `A(s) = T(s).dir` (`T` the unit derivative across the singular
/// direction, `s` along it, at `uv` with `s` replacing its singular coordinate) for
/// `eval`, the net of [`certified_alignment_evaluator`], with their error bounds (the
/// derivation is `slope_charges`'). `None` where the evaluator gives no bound or
/// `|w|` is not above its error.
pub fn alignment_slopes(
    eval: &EvalSurface,
    uv: Point2,
    dir: Vec3,
    singular_dir: SurfParamType,
    s: f64,
) -> Option<AlignmentSlopes> {
    let (q, w, ws, wss) = if singular_dir == SurfParamType::U {
        ([s, uv.y], (0, 1), (1, 1), (2, 1))
    } else {
        ([uv.x, s], (1, 0), (1, 1), (1, 2))
    };
    let (d, e) = certified_partials(eval, q).ok()?;
    let get = |(i, j): (usize, usize)| -> Option<(Vec3, f64)> {
        Some((*d.get(i)?.get(j)?, *e.get(i)?.get(j)?))
    };
    let ((a, ea), (b, eb), (c, ec)) = (get(w)?, get(ws)?, get(wss)?);
    let r = a.length();
    if r <= ea || r.is_nan() {
        return None;
    }
    // the certified values: T = a / r, T' = P b / r, T'' = P c / r - 2 (T.b / r) T'
    // - |T'|^2 T (P = I - T T^T), F = T'.dir, F' = T''.dir
    let t = a / r;
    let t1 = (b - t * t.dot(b)) / r;
    let cb = t.dot(b) / r;
    let t2 = (c - t * t.dot(c)) / r - t1 * (2.0 * cb) - t * t1.length_squared();
    let (fc, fpc) = (t1.dot(dir), t2.dot(dir));
    // their perturbation bounds
    let rl = r - ea;
    let delta = 2.0 * ea / r;
    let (bl, cl) = (b.length(), c.length());
    let tau = bl / r;
    let d1 = (eb + (2.0 + delta) * (bl + eb) * delta) / rl + tau * ea / rl;
    let dpc = (ec + (2.0 + delta) * (cl + ec) * delta) / rl + (cl / r) * ea / rl;
    let dcb = (eb + (bl + eb) * delta) / rl + tau * ea / rl;
    let d2 = dpc
        + 2.0 * (dcb * (tau + d1) + tau * d1)
        + (2.0 * tau + d1) * d1
        + (tau + d1) * (tau + d1) * delta;
    // (and the rounding of the certified values' own formulas, at most a few dozen
    // operations on terms each at most `|c| / r + 3 tau^2 + tau`)
    let own = gamma(64) * (cl / r + 3.0 * tau * tau + tau + 1.0);
    Some(AlignmentSlopes {
        f: fc,
        f_error: d1 + own,
        f_prime: fpc,
        f_prime_error: d2 + own,
    })
}

/// Whether the derivative `w` across the singular direction at `uv` (with `s` replacing
/// its singular coordinate) is not enclosed away from zero by `eval`'s certified partials
/// (`|w|` not above its error bound): a point collapsed to the rounding, where the unit
/// tangent of `FindDegenParamForDirection`'s alignment is not defined to it (WR-57).
/// `false` where the partials fail.
pub fn tangent_not_enclosed(
    eval: &EvalSurface,
    uv: Point2,
    singular_dir: SurfParamType,
    s: f64,
) -> bool {
    let (q, (i, j)) = if singular_dir == SurfParamType::U {
        ([s, uv.y], (0, 1))
    } else {
        ([uv.x, s], (1, 0))
    };
    let Ok((d, e)) = certified_partials(eval, q) else {
        return false;
    };
    match (
        d.get(i).and_then(|r| r.get(j)),
        e.get(i).and_then(|r| r.get(j)),
    ) {
        (Some(w), Some(&ew)) => w.length() <= ew,
        _ => false,
    }
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

    #[test]
    fn issue_2009_unevaluable_samples_give_no_derivative_lengths() {
        // tangency-labs/smlib 21dcf69 (ApproxDerivativeLengths): the samples that
        // evaluate are averaged, SM_UNDEF_DOUBLE when none does. Upstream averages the
        // unset derivatives.
        let s = Surface::from(BSplineSurface::default());
        let dom = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let k = Kernel::default();
        let len = approx_derivative_lengths(&k, &s, &dom).unwrap();
        assert_eq!(len, Vec2::new(UNDEF_DOUBLE, UNDEF_DOUBLE));
    }

    #[test]
    fn issue_2009_unevaluable_points_are_not_shown_converged() {
        // tangency-labs/smlib 21dcf69 (sm_PassSSConvergenceTest, sm_PassSCConvergenceTest):
        // a point that cannot be evaluated is not shown converged. Upstream compares the
        // unset values, which agree.
        let s = Surface::from(BSplineSurface::default());
        let uv = Point2::new(0.5, 0.5);
        let k = Kernel::default();
        assert!(!pass_ss_convergence_test(&k, &s, uv, &s, uv).unwrap());
    }

    #[test]
    fn issue_2009_unevaluable_surface_is_not_shown_closed() {
        // tangency-labs/smlib 21dcf69 (SmSurface::IsClosed): a surface that cannot be
        // evaluated is not shown closed.
        let s = Surface::from(BSplineSurface::default());
        let dom = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let k = Kernel::default();
        assert!(s.evaluate_point(&k, Point2::new(0.0, 0.0)).is_err());
        let closed = s
            .is_closed(&k, &dom, SurfParamType::U, None, false)
            .unwrap()
            .0;
        assert!(!closed);
    }

    /// tangency-labs/solidcore#525 (WR-384): with the corner points wanted, upstream's
    /// `if (lRtn &= bit)` clears the other corners' bits (4 flat corners come back as
    /// none, with only the first point); each bit is tested.
    #[test]
    fn issue_525_flat_corners_keep_their_bits() {
        let k = Kernel::default();
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
        use crate::nurbs::math::CPoint;

        let k = Kernel::default();
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
        use crate::nurbs::math::CPoint;
        let k = Kernel::default();
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

    /// WR-56 (#1852; the fork's tangency-labs/smlib@0b4de8c / eaa998a): the flat patch
    /// (10 u, 10 v, 0) and the point (5, 5, 1), whose foot is (0.5, 0.5). A local result
    /// 5e-4 along the patch from the foot (between upstream's 1e-4 foot test and its
    /// 1e-3 interior refusal, so upstream decided it by the swapped distance test) is
    /// not a converged foot, so it is refused whatever its guess.
    #[test]
    fn wr_56_good_drop_result_compares_the_right_distances() {
        use crate::nurbs::math::CPoint;

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
        let k = Kernel::default();
        let good = |k: &Kernel, guess: Point2| {
            good_drop_result(k, &s, &domain, point, guess, &sol).unwrap()
        };
        // A result 5e-4 off along the surface is not one the local solve converged to
        // (tight tolerance 6e-12 here), whatever the guess (tangency-labs/smlib@eaa998a);
        // the distance test is covered on the domain boundary by
        // `issue_1852_good_drop_compares_the_actual_guess_distance`.
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
        let k = Kernel::default();
        let result_uv = Point2::new(0.5, 0.5);
        let (result_point, su, _) =
            evaluate_1st_derivatives(&k, &s, result_uv, Side::Left, Side::Left).unwrap();
        let su = su.unitize_or_z();
        let mut sol = SolutionEnd::new();
        sol.solution_value = 1.0;
        *sol.param_mut(0).unwrap() = result_uv.x;
        *sol.param_mut(1).unwrap() = result_uv.y;
        let good = |k: &Kernel, d: f64| {
            let p = result_point + d * su;
            good_drop_result(k, &s, &domain, p, result_uv, &sol).unwrap()
        };
        let tight = EFF_ZERO * (1.0 + (result_point + 1e-6 * su).max_dimension());
        assert!(tight < 1e-11);
        // Only the foot within the tight tolerance (upstream took every foot within
        // 1e-4, and an interior one up to 1e-3 that passed the distance test).
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
    /// test rejects a drop that improved on the guess and takes one that got worse;
    /// the port takes only a result at its guess (a moved one, nearer or not, goes to
    /// the global solve, #17773). Every case reaches the final comparison: the
    /// result lies on the domain's `u = 0` side and the point on the plane `1/2048`
    /// beyond it along `-S_u`, so `Fu` is `-1/2048` (allowed on that side) and `Fv`
    /// is zero. The foot test (tangency-labs/smlib@eaa998a) does not take it, and on
    /// that side no interior refusal applies, so the comparison decides.
    #[test]
    fn issue_1852_good_drop_compares_the_actual_guess_distance() {
        let h = 1.0 / 2048.0;
        let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let s = Surface::from(Plane::from_normal(Point3::ZERO, Vec3::Z));
        let k = Kernel::default();
        let result_uv = Point2::new(0.0, 0.5);
        let (result_point, su, _) =
            evaluate_1st_derivatives(&k, &s, result_uv, Side::Left, Side::Left).unwrap();
        let pt_to_drop = result_point - h * su;
        let mut sol = SolutionEnd::new();
        sol.solution_value = 1.0;
        *sol.param_mut(0).unwrap() = result_uv.x;
        *sol.param_mut(1).unwrap() = result_uv.y;

        // The result is closer than the guess: upstream rejected it (its swapped
        // names make the guess look farther), and so does the port: a nearer result
        // is not shown to be the nearest point (#17773).
        let nearer_guess = result_uv + Point2::new(0.1, 0.0);
        // The guess is closer than the result: upstream accepted it; the port sends
        // it to the global solve, as every moved result.
        let farther_guess = result_uv - Point2::new(h, 0.0);
        // Equidistant but moved: upstream accepted it; the port sends it to the
        // global solve too.
        let equal_guess = result_uv - Point2::new(2.0 * h, 0.0);

        let good =
            |guess: Point2| good_drop_result(&k, &s, &domain, pt_to_drop, guess, &sol).unwrap();
        assert!(!good(nearer_guess));
        assert!(!good(farther_guess));
        assert!(!good(equal_guess));
        // A result at its guess is taken; a moved one passing every other test is a
        // candidate for the global solve to decide.
        assert!(good(result_uv));
        let verdict =
            |guess: Point2| drop_result_verdict(&k, &s, &domain, pt_to_drop, guess, &sol).unwrap();
        for g in [nearer_guess, farther_guess, equal_guess] {
            assert_eq!(verdict(g), DropVerdict::MovedFoot);
        }
    }

    /// The surface of the goldens case `surface.surface_p5` gen0494: a quadratic x
    /// linear B-spline whose whole `u = u_min` row of poles is one point (an apex),
    /// and its NURB.
    fn wr56_apex_patch() -> (Surface, crate::nurbs::srf_basic::Surface) {
        use crate::nurbs::math::CPoint;
        use crate::nurbs::srf_basic::Surface as NlSurface;
        const NO_WEIGHT: f64 = -f64::MAX;
        let rows: [[[f64; 3]; 4]; 5] = [
            [[-1.0, 1.5, -0.3528299405334874]; 4],
            [
                [1.1391515183914103, 0.1895073079961052, 0.004900509158002478],
                [1.0708103395113426, 0.8720049041597548, -0.6986994559112379],
                [1.0299464879505382, 1.8721875018478356, 0.5513240971150135],
                [0.9886903687364608, 2.8721080300372805, -0.48747652542845554],
            ],
            [
                [
                    2.1070309924156483,
                    0.11252880589006203,
                    -0.09370566140577241,
                ],
                [1.8849670020261053, 0.8723426468667175, -0.4988989261051539],
                [2.011556087011981, 1.9943285182684183, 0.025765388172125484],
                [1.907948877077642, 3.0838646904906293, -0.3489740696354065],
            ],
            [
                [2.9979566621921303, 0.04456506475475272, -0.7170664294387074],
                [2.96929757059988, 0.9358628089642543, -0.36760236227136067],
                [3.097639384127755, 1.8705066502896766, -0.4996455533550941],
                [3.019229993279216, 3.1669045079545723, 0.3281058049061327],
            ],
            [
                [
                    3.965244874290516,
                    -0.14668692832331526,
                    -0.06349669667494616,
                ],
                [3.907750128272427, 0.9123093020535409, -0.8132176028786268],
                [3.879469837315649, 1.8031101430879306, -0.25026091166160747],
                [3.869116187812298, 2.8362574458656624, 0.3290943172231638],
            ],
        ];
        let net = rows
            .iter()
            .map(|r| {
                r.iter()
                    .map(|&[x, y, z]| CPoint::new(x, y, z, NO_WEIGHT))
                    .collect()
            })
            .collect();
        let u0 = 0.5130221717994283;
        let u1 = 3.34766559576863;
        let u = vec![
            u0,
            u0,
            u0,
            2.1430395924882157,
            2.860429223096711,
            u1,
            u1,
            u1,
        ];
        let v = vec![0.0, 0.0, 0.15893773335490313, 0.4332409252690698, 1.0, 1.0];
        let nl = NlSurface::new(net, 2, 1, u, v).unwrap();
        let mut s = BSplineSurface::from_nurb(nl.clone(), false);
        s.set_b_spline_surface_form(BSplineSurfaceForm::from_code(11).unwrap());
        (Surface::from(s), nl)
    }

    /// WR-56, the goldens case `surface.surface_p7` gen1133: a local result at the guess
    /// itself (on the low `u` and `v` sides of the domain, so only the distance test
    /// decides) is not farther than its guess. `EvaluatePoint` and
    /// `Evaluate1stDerivatives` give points one ulp apart there (distances 1.4e-17
    /// apart), so measuring the guess with the one and the result with the other
    /// refused it (on the fixed path; upstream's swapped names took it by the same
    /// ulp); a result at its guess is measured with its own point.
    #[test]
    fn wr56_a_result_at_its_guess_is_not_farther_than_it() {
        use crate::nurbs::math::CPoint;
        use crate::nurbs::srf_basic::Surface as NlSurface;
        let rows: [[[f64; 3]; 3]; 4] = [
            [
                [
                    -0.07296530912965596,
                    -0.13382827179692552,
                    0.3125667436025059,
                ],
                [0.01658945595744149, 1.1585060264215579, 0.2233463644045514],
                [0.17084750753940459, 1.8042104340655583, 0.2204320166305955],
            ],
            [
                [1.124479345643036, 0.07892894130757921, -0.2544119249279014],
                [0.9615756188060413, 1.0219778380212683, -0.2715618821244925],
                [1.092391702103946, 2.093603799947143, -0.316753420972412],
            ],
            [
                [1.9856421638499013, 0.11995203969368912, -0.1156204836566543],
                [2.0294866006275982, 0.9932631100376418, -0.22997466919577964],
                [1.8926576235496195, 1.811117753252593, 0.12993999364080164],
            ],
            [
                [3.093427183178206, -0.1053012036490268, -0.2067762023192355],
                [2.859093226931018, 0.8968430268449183, -0.15153709187625078],
                [
                    2.9190967480132652,
                    1.8283899891115807,
                    -0.061125560771756994,
                ],
            ],
        ];
        let net = rows
            .iter()
            .map(|r| {
                r.iter()
                    .map(|&[x, y, z]| CPoint::new(x, y, z, -f64::MAX))
                    .collect()
            })
            .collect();
        let u0 = 0.8609971713540157;
        let u1 = 3.5508292632660834;
        let u = vec![u0, u0, 1.1222893888182777, 1.8763197218556462, u1, u1];
        let v = vec![0.0, 0.0, 0.5987377657284333, 1.0, 1.0];
        let mut b = BSplineSurface::from_nurb(NlSurface::new(net, 1, 1, u, v).unwrap(), false);
        b.set_b_spline_surface_form(BSplineSurfaceForm::from_code(11).unwrap());
        let s = Surface::from(b);
        let k = Kernel::default();
        let dom = Extent2d::from_bounds(
            1.6227666457241092,
            0.07002218305834061,
            3.3091732897880504,
            0.7675665673892663,
        );
        let uv = Point2::new(0.8609994164241573, 0.04752690390728609);
        let p = Point3::new(
            -0.07300334888722718,
            -0.13391703284631928,
            0.31251048415221666,
        );
        // the two evaluations differ in the last bit here
        let by_point = s.evaluate_point(&k, uv).unwrap();
        let (by_derivs, su, sv) =
            evaluate_1st_derivatives(&k, &s, uv, Side::Left, Side::Left).unwrap();
        assert_ne!(by_point, by_derivs);
        // and the result's point is the farther one, so measuring the guess with
        // `EvaluatePoint` (distances in order) would refuse the result
        assert!(p.distance_squared(by_derivs) > p.distance_squared(by_point));
        // both sides of the domain allow the gap's components (only the distance test
        // decides)
        assert!(uv.x < dom.min().x && uv.y < dom.min().y);
        let gap = p - by_derivs;
        assert!(gap.dot(su.unitize_or_z()) < 0.0 && gap.dot(sv.unitize_or_z()) < 0.0);
        let mut sol = SolutionEnd::new();
        sol.solution_value = 0.5;
        *sol.param_mut(0).unwrap() = uv.x;
        *sol.param_mut(1).unwrap() = uv.y;
        assert!(good_drop_result(&k, &s, &dom, p, uv, &sol).unwrap());
    }

    /// A lower bound on the distance from `p` to a non-rational Bezier patch of degrees
    /// (2, 1) with control net `net` (3 rows in u, 2 in v), or `None` past `budget`
    /// sub-patches: the patch lies in the convex hull of its net, so in the net's
    /// bounding box; a sub-patch whose box (grown by `margin`) is at least `bound`
    /// from `p` is done, else it is split in half (de Casteljau) along the direction
    /// whose control polygon is longer. Returns `bound` when every piece is done.
    fn bezier21_distance_at_least(
        net: [[Point3; 2]; 3],
        p: Point3,
        bound: f64,
        margin: f64,
        budget: usize,
    ) -> Option<f64> {
        let mut stack = vec![net];
        let mut pieces = 0;
        while let Some(n) = stack.pop() {
            pieces += 1;
            if pieces > budget {
                return None;
            }
            let pts = n.iter().flatten();
            let lo = pts
                .clone()
                .fold(Vec3::new(f64::MAX, f64::MAX, f64::MAX), |a, q| {
                    Vec3::new(a.x.min(q.x), a.y.min(q.y), a.z.min(q.z))
                });
            let hi = pts.fold(Vec3::new(f64::MIN, f64::MIN, f64::MIN), |a, q| {
                Vec3::new(a.x.max(q.x), a.y.max(q.y), a.z.max(q.z))
            });
            let gap = |x: f64, l: f64, h: f64| (l - margin - x).max(x - h - margin).max(0.0);
            let d = Vec3::new(
                gap(p.x, lo.x, hi.x),
                gap(p.y, lo.y, hi.y),
                gap(p.z, lo.z, hi.z),
            );
            if d.length() >= bound {
                continue;
            }
            let u_len = (0..2)
                .map(|j| n[0][j].distance(n[1][j]) + n[1][j].distance(n[2][j]))
                .fold(0.0, f64::max);
            let v_len = (0..3)
                .map(|i| n[i][0].distance(n[i][1]))
                .fold(0.0, f64::max);
            if u_len >= v_len {
                let mut left = n;
                let mut right = n;
                for j in 0..2 {
                    let a01 = (n[0][j] + n[1][j]) * 0.5;
                    let a12 = (n[1][j] + n[2][j]) * 0.5;
                    let mid = (a01 + a12) * 0.5;
                    left[1][j] = a01;
                    left[2][j] = mid;
                    right[0][j] = mid;
                    right[1][j] = a12;
                }
                stack.push(left);
                stack.push(right);
            } else {
                let mut low = n;
                let mut high = n;
                for i in 0..3 {
                    let mid = (n[i][0] + n[i][1]) * 0.5;
                    low[i][1] = mid;
                    high[i][0] = mid;
                }
                stack.push(low);
                stack.push(high);
            }
        }
        Some(bound)
    }

    /// WR-56, the goldens case `surface.surface_p5` gen0494: a point 7.6e-5 from an
    /// apex (a whole row of poles) and 3.36e-5 off the surface, dropped with
    /// [`SolverOperation::Intersect`] from a guess. The local solve stops at the foot
    /// next to the apex, where the gap's component along the unit `S_v` (`|S_v|`
    /// 7.7e-5) is 5.7e-11, over the tight tolerance 2.5e-12, so the foot test sends the
    /// drop to the global solve. That finds no intersection within the intersection
    /// tolerance 2e-5, as the same drop without a guess does: the point is at least
    /// 3.3e-5 from the surface (a convex-hull bound over the Bezier patches, below).
    /// Upstream's 1e-4 foot test returned the foot as a successful intersection only
    /// when given that guess. A minimise drop from the same guess still returns the
    /// foot, at the distance the global solve finds.
    #[test]
    fn wr56_intersect_drop_from_a_guess_answers_as_without_one() {
        use crate::geom::tol::ZONE_TOL_3D;
        use crate::nurbs::srf_tool::p1::decompose_to_bez;
        use crate::surface::surface::p4::NoObject;
        use crate::surface::surface::p5::DropPointOptions;
        // `SM_XSECT_TOL_3D`, the drop's tolerance for a surface without a face
        // (`drop_point_from`: `face_zone_tol_3d()` or `XSECT_TOL_3D`, p5.rs)
        let xsect_tol_3d = 2.0 * ZONE_TOL_3D;
        let (s, nl) = wr56_apex_patch();
        let k = Kernel::default();
        let p = Point3::new(-0.9999526462492969, 1.5000491995315461, -0.3527975781749486);
        let dom = Extent2d::from_bounds(0.5130221717994283, 0.0, 3.34766559576863, 1.0);
        let guess = Point2::new(1.9248388350119723, 0.4139249704993634);
        let opts = |op: SolverOperation, guess_uv: Option<Point2>| DropPointOptions {
            guess_uv,
            operation: op,
            tgt_seam_side: Some(Vec3::new(
                -0.48155218659663124,
                -0.5708253216475445,
                0.7039419181183777,
            )),
            want_solutions: false,
        };
        let drop = |op, g| s.drop_point::<NoObject>(&k, p, &dom, &opts(op, g)).unwrap();

        // The local solve stops at the foot next to the apex; the foot test refuses it.
        let local = s
            .local_point_solve::<NoObject>(&k, &dom, SolverOperation::Intersect, p, guess)
            .unwrap();
        assert!(local.found);
        let foot_uv = Point2::new(
            local.solution.start.param(0).unwrap(),
            local.solution.start.param(1).unwrap(),
        );
        assert!(foot_uv.x - dom.min().x < 3e-5, "{foot_uv:?}");
        assert!(!good_drop_result(&k, &s, &dom, p, guess, &local.solution.start).unwrap());

        // Intersect: no answer, with the guess as without it.
        let with_guess = drop(SolverOperation::Intersect, Some(guess));
        let without = drop(SolverOperation::Intersect, None);
        assert!(!with_guess.success && !without.success);
        assert_eq!(with_guess.gap, BIG_DOUBLE);
        assert_eq!(without.gap, BIG_DOUBLE);

        // Minimise from the guess: the foot, at the global solve's distance.
        let minimise = drop(SolverOperation::Minimize, Some(guess));
        assert!(minimise.success);
        let global = s
            .global_point_solve::<NoObject>(
                &k,
                None,
                &dom,
                SolverOperation::Minimize,
                p,
                xsect_tol_3d,
                None,
                SolutionRequested::All,
            )
            .unwrap();
        let nearest = global
            .as_slice()
            .iter()
            .map(|sl| sl.start.solution_value)
            .fold(f64::INFINITY, f64::min);
        assert!(
            (minimise.gap - nearest).abs() < 1e-15,
            "{} {nearest}",
            minimise.gap
        );
        assert!(
            minimise.gap > 3.3e-5 && minimise.gap < 3.4e-5,
            "{}",
            minimise.gap
        );

        // Certificate: every point of the surface is at least 3.3e-5 from `p`, so no
        // point is within the intersection tolerance. The Bezier patches' control
        // points come from the B-spline's by affine combinations with weights in
        // [0, 1] (a few ulps of coordinates under 4: under 1e-14), and the subdivision
        // midpoints add an ulp per level; the 1e-12 margin covers both.
        let patches = decompose_to_bez(&nl).unwrap();
        let bound = 3.3e-5;
        let u_breaks = [
            dom.min().x,
            2.1430395924882157,
            2.860429223096711,
            dom.max().x,
        ];
        let v_breaks = [0.0, 0.15893773335490313, 0.4332409252690698, 1.0];
        let mut count = 0;
        for (i, row) in patches.iter().enumerate() {
            for (j, patch) in row.iter().enumerate() {
                let c = patch.cpts();
                assert_eq!((c.len(), c[0].len()), (3, 2));
                let pt = |i: usize, j: usize| Point3::new(c[i][j].x, c[i][j].y, c[i][j].z);
                let net = [
                    [pt(0, 0), pt(0, 1)],
                    [pt(1, 0), pt(1, 1)],
                    [pt(2, 0), pt(2, 1)],
                ];
                // the patch is the surface's span (i, j): it interpolates its corners
                for (a, b) in [(0, 0), (0, 1), (2, 0), (2, 1)] {
                    let uv = Point2::new(u_breaks[i + a / 2], v_breaks[j + b]);
                    let corner = s.evaluate_point(&k, uv).unwrap();
                    assert!(corner.distance(net[a][b]) < 1e-14, "{i} {j} {a} {b}");
                }
                // and agrees with it at the u-midpoints of both v ends: with the corners
                // that is six points of a 3 x 2 grid, which determine a degree (2, 1)
                // patch, so the patch is the span (up to the rounding above)
                for b in 0..2 {
                    let mid = (net[0][b] + net[1][b] * 2.0 + net[2][b]) * 0.25;
                    let u_mid = 0.5 * (u_breaks[i] + u_breaks[i + 1]);
                    let on_span = s
                        .evaluate_point(&k, Point2::new(u_mid, v_breaks[j + b]))
                        .unwrap();
                    assert!(on_span.distance(mid) < 1e-14, "{i} {j} mid {b}");
                }
                assert_eq!(
                    bezier21_distance_at_least(net, p, bound, 1e-12, 1_000_000),
                    Some(bound)
                );
                count += 1;
            }
        }
        assert_eq!(count, 9);
        assert!(bound > xsect_tol_3d);
    }

    fn wr57_double_root_patch_u(u_min: f64, u_max: f64) -> Surface {
        use crate::nurbs::math::CPoint;

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
        use crate::nurbs::math::CPoint;

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

    /// WR-57 (#2209): on a sphere reparameterized to `[0, 360]` or `[0, 2 * PI]` in `u`,
    /// the south-pole seam tie-break (`ii = 0` vs `ii = 9` for `+X`) compares the sampled
    /// parameters with `dS`, so `input_s = 5.0` and `input_s = 0.8` keep `u = 0.0`
    /// (upstream compares the `[0, 1]` fractions and flips them to `360.0` and `2 * PI`).
    #[test]
    fn wr57_sphere_pole_seam_tiebreak_on_non_unit_and_unit_domains() {
        use crate::geom::axis2_placement::Axis2Placement;
        use crate::math::PI;
        use crate::surface::sphere::Sphere;

        let k = Kernel::default();
        let sphere = Sphere::create_canonical(&k, &Axis2Placement::default(), 1.0).unwrap();

        // Native `[0, 1]` singular domain: the seam side nearest the input.
        let unit_surf = Surface::from(sphere.clone());
        assert_eq!(
            unit_surf
                .natural_uv_domain_with_nurb(&k)
                .unwrap()
                .u_interval(),
            Extent1d::from_min_max(0.0, 1.0)
        );
        for input_s in [-2.0, 0.0, 0.1, 0.49, 0.51, 0.9, 1.0, 5.0] {
            let r = unit_surf
                .find_degen_param_for_direction(
                    &k,
                    Point2::new(input_s, 0.0),
                    Vec3::X,
                    true,
                    SurfParamType::U,
                    1e-9,
                )
                .unwrap();
            assert!(r.converged);
            let expected = if input_s < 0.5 { 0.0 } else { 1.0 };
            assert!((r.uv.x - expected).abs() < 1e-9, "{input_s} {r:?}");
        }

        // `[0, 360]` angular domain with `input_s = 5.0`: `5.0` is `5.0` from `0.0` vs
        // `355.0` from `360.0` (upstream: `|1.0 - 5.0| = 4.0 < |0.0 - 5.0| = 5.0`).
        let mut sphere_deg = sphere.clone();
        sphere_deg
            .reparameterize(&k, &Extent2d::from_bounds(0.0, 0.0, 360.0, 1.0))
            .unwrap();
        let surf_deg = Surface::from(sphere_deg);
        let r_deg = surf_deg
            .find_degen_param_for_direction(
                &k,
                Point2::new(5.0, 0.0),
                Vec3::X,
                true,
                SurfParamType::U,
                1e-9,
            )
            .unwrap();
        assert!(r_deg.converged);
        assert!(r_deg.uv.x.abs() < 1e-9, "{r_deg:?}");

        // `[0, 2 * PI]` angular domain with `input_s = 0.8` rad (`0.8` from `0.0` vs
        // `2 * PI - 0.8` from `2 * PI`; upstream: `|1.0 - 0.8| = 0.2 < |0.0 - 0.8| = 0.8`).
        let mut sphere_rad = sphere;
        sphere_rad
            .reparameterize(&k, &Extent2d::from_bounds(0.0, 0.0, 2.0 * PI, 1.0))
            .unwrap();
        let surf_rad = Surface::from(sphere_rad);
        let r_rad = surf_rad
            .find_degen_param_for_direction(
                &k,
                Point2::new(0.8, 0.0),
                Vec3::X,
                true,
                SurfParamType::U,
                1e-9,
            )
            .unwrap();
        assert!(r_rad.converged);
        assert!(r_rad.uv.x.abs() < 1e-9, "{r_rad:?}");
    }

    /// WR-57 (#2209): on B-spline degenerate patches with interior double solutions on
    /// `[10, 20]` and `[-20, -10]` (for both `SurfParamType::U` and `SurfParamType::V`),
    /// the tie-break chooses the sampled parameter closer to `dS` (upstream chooses the
    /// sample fraction closer in `[0, 1]`: `17.5`, `17.5`, `-17.5` and `17.5` below).
    #[test]
    fn wr57_nurbs_degenerate_patch_shifted_domains_and_both_singular_directions() {
        let k = Kernel::default();
        let solve = |s: &Surface, uv: Point2, dir: Vec3, from_pole: bool, sdir| {
            s.find_degen_param_for_direction(&k, uv, dir, from_pole, sdir, 1e-9)
                .unwrap()
        };

        // Positive shifted `U` domain `[10, 20]`, interior roots at `12.5` and `17.5`.
        let pos_u = wr57_double_root_patch_u(10.0, 20.0);
        let r = solve(
            &pos_u,
            Point2::new(12.0, 0.0),
            Vec3::X,
            true,
            SurfParamType::U,
        );
        assert!(r.converged);
        assert!((r.uv.x - 12.5).abs() < 1e-9);

        // Inbound direction (`dir_from_pole = false` with `-X`) exercises the negated
        // direction branch with the same tie-break.
        let r = solve(
            &pos_u,
            Point2::new(12.0, 0.0),
            -Vec3::X,
            false,
            SurfParamType::U,
        );
        assert!(r.converged);
        assert!((r.uv.x - 12.5).abs() < 1e-9);

        // When `input_s = 18.0` is genuinely near `17.5`, `17.5`.
        let r = solve(
            &pos_u,
            Point2::new(18.0, 0.0),
            Vec3::X,
            true,
            SurfParamType::U,
        );
        assert!(r.converged);
        assert!((r.uv.x - 17.5).abs() < 1e-9);

        // Negative shifted `U` domain `[-20, -10]`, interior roots at `-17.5` and `-12.5`:
        // at `input_s = -12.0`, `7/9` (`-12.5`; upstream keeps `2/9` because `-12.0 < 0.5`).
        let neg_u = wr57_double_root_patch_u(-20.0, -10.0);
        let r = solve(
            &neg_u,
            Point2::new(-12.0, 0.0),
            Vec3::X,
            true,
            SurfParamType::U,
        );
        assert!(r.converged);
        assert!((r.uv.x + 12.5).abs() < 1e-9);

        // `SurfParamType::V` on `[0, 1] x [10, 20]`, interior roots at `v = 12.5` and `17.5`.
        let pos_v = wr57_double_root_patch_v(10.0, 20.0);
        let r = solve(
            &pos_v,
            Point2::new(0.0, 12.0),
            Vec3::X,
            true,
            SurfParamType::V,
        );
        assert!(r.converged);
        assert!((r.uv.y - 12.5).abs() < 1e-9);

        // `[0, 1]` singular `U` domain: the root on the input's side.
        let unit_u = wr57_double_root_patch_u(0.0, 1.0);
        for input_s in [-2.0, 0.0, 0.2, 0.49, 0.51, 0.8, 1.0, 5.0] {
            let r = solve(
                &unit_u,
                Point2::new(input_s, 0.0),
                Vec3::X,
                true,
                SurfParamType::U,
            );
            assert!(r.converged);
            let expected = if input_s < 0.5 { 0.25 } else { 0.75 };
            assert!((r.uv.x - expected).abs() < 1e-9, "{input_s} {r:?}");
        }
    }

    /// Goldens `surface.surface_p7` gen0435's surface: degree 1 in `u`, its first control
    /// row one point; with its `u` knots and `v_max`.
    fn wr57_gen0435_surface() -> (Surface, [f64; 6], f64) {
        use crate::nurbs::b_spline_surface::BSplineSurfaceForm;
        use crate::nurbs::math::CPoint;

        let row = |pts: [[f64; 3]; 5]| -> Vec<CPoint> {
            pts.iter()
                .map(|p| CPoint::new(p[0], p[1], p[2], NOW))
                .collect()
        };
        let apex = [-1.0, 2.0, -0.42762430077910596];
        let net = vec![
            row([apex; 5]),
            row([
                [0.8738399672861462, -0.1510673462795813, 0.08069159132641086],
                [0.9955225473139466, 0.8179111377519757, 0.09316127625739742],
                [
                    1.1855740800365888,
                    2.0289789047176336,
                    -0.024571212509314846,
                ],
                [0.9112063435757395, 3.07697621860427, -0.2954704463402183],
                [1.1499146183418412, 3.8074327151708607, -0.0919756876720027],
            ]),
            row([
                [
                    2.024971807131176,
                    -0.050149056834845024,
                    0.18932201154263176,
                ],
                [1.9615761832766014, 0.9341773729782138, 0.26049682361654586],
                [2.197490683170156, 1.9116684401463968, 0.09319886307041407],
                [2.1975640559333542, 2.9841928879636317, -0.2419945783159688],
                [2.1545771893058223, 3.927938197137191, 0.29213736310073635],
            ]),
            row([
                [2.8364304364683575, 0.1866403391078159, 0.24545945198203217],
                [2.902087042066616, 1.1544412869142817, -0.3290366735256661],
                [3.1338647757747866, 1.8893554917288837, 0.21583864191961022],
                [3.122212155322879, 3.0905268397911705, -0.0577996414565985],
                [2.980910454495871, 4.193695387966584, 0.17962351804190546],
            ]),
        ];
        let u_knots = [
            1.715616006893217,
            1.715616006893217,
            1.8355757863944349,
            1.8428010635514407,
            2.5658414227962067,
            2.5658414227962067,
        ];
        let (v0, v1) = (1.3850930582803622, 2.7106658347953982);
        let v_knots = [v0, v0, v0, v0, 1.8553827458277017, v1, v1, v1, v1];
        let mut bs = BSplineSurface::from_data(3, 4, 1, 3, 5, 8, &net, &u_knots, &v_knots).unwrap();
        bs.set_b_spline_surface_form(BSplineSurfaceForm::HelicalSweep);
        let s = Surface::from(bs);
        (s, u_knots, v1)
    }

    /// WR-57 (goldens `surface.surface_p7` gen0435): the surface is degree 1 in `u` and
    /// its first control row is one point, so on the first `u` span
    /// `S_v(u, v_max) = a(u) * S_v^row1(v_max)` with `a(u) > 0` for `u > u_min`: the unit
    /// tangent there is exactly `unit(P[1][4] - P[1][3])`, and every `u` of the span
    /// aligns equally with `dir`. The input's `u`, 1.6e-7 above `u_min`, is the
    /// near-best guess; so close to the collapsed corner its Newton solve does not
    /// converge, and the second solve starts at the best sample, sample 0 (the first of
    /// the equal samples), and converges there. Started at `s_ivl.evaluate(0) = u_min`
    /// instead, where `a = 0` and the tangent is undefined, it fails (`SM_ERR`);
    /// upstream's fraction tie-break lands at `u = 1.8101`, the far end of the plateau.
    #[test]
    fn wr57_newton_starts_at_the_sample_not_the_collapsed_corner() {
        let (s, u_knots, v1) = wr57_gen0435_surface();
        let k = Kernel::default();

        let uv = Point2::new(1.7156161692711094, v1);
        let dir = Vec3::new(0.9109237931558096, -0.11434982158434526, 0.6569538269953288);
        let r = s
            .find_degen_param_for_direction(&k, uv, dir, false, SurfParamType::U, 1e-9)
            .unwrap();
        assert!(r.converged, "{r:?}");
        assert_eq!(r.uv.y, v1);

        // on the first span, strictly inside it (not the collapsed corner)
        let u_min = u_knots[0];
        let span_end = u_knots[2];
        assert!(r.uv.x > u_min && r.uv.x < span_end, "{r:?}");

        // at the best sample, sample 0 of the pulled-in sample domain
        let sample0 = u_min + 0.025 * (u_knots[5] - u_min);
        assert!(
            (r.uv.x - sample0).abs() < 1e-4,
            "{r:?} vs sample 0 at {sample0}"
        );

        // and aligned exactly as the plateau's closed form
        let p13 = Vec3::new(0.9112063435757395, 3.07697621860427, -0.2954704463402183);
        let p14 = Vec3::new(1.1499146183418412, 3.8074327151708607, -0.0919756876720027);
        let want = (p14 - p13).unitize().unwrap().dot(dir.unitize().unwrap());
        let m = s
            .evaluate(&k, r.uv, 1, 1, flags(Side::Left, Side::Left, true))
            .unwrap();
        let got = at(&m, 0, 1)
            .unwrap()
            .unitize()
            .unwrap()
            .dot(dir.unitize().unwrap());
        assert!(
            (got - want).abs() < 1e-12,
            "alignment {got} vs closed form {want}"
        );
    }

    /// WR-57 (#17856 r2): at gen0435's collapsed corner (`u_min`, where the first
    /// control row is one point) the derivative across the singular direction is zero,
    /// so its certified error is not below its length and the slopes have no bound:
    /// the charges are then infinite, and no sign of `F` or `F'` is proven there, where
    /// a zero charge would let the raw float signs decide. Inside the span they are
    /// bounded.
    #[test]
    fn wr57_an_unbounded_charge_proves_no_sign() {
        let (s, u_knots, v1) = wr57_gen0435_surface();
        let k = Kernel::default();
        let eval = certified_alignment_evaluator(&k, &s)
            .unwrap()
            .expect("a certified evaluator");
        let uv = Point2::new(1.7156161692711094, v1);
        let dir = Vec3::new(0.9109237931558096, -0.11434982158434526, 0.6569538269953288)
            .unitize()
            .unwrap();
        let u_min = u_knots[0];
        assert_eq!(
            alignment_slopes(&eval, uv, dir, SurfParamType::U, u_min),
            None
        );
        let kept = KeptSlopes::default();
        let (cf, cfp, curvature) = charges_or_unproven(
            Some(&eval),
            &kept,
            uv,
            dir,
            SurfParamType::U,
            u_min,
            0.25,
            -0.5,
        );
        assert!(
            cf == f64::INFINITY && cfp == f64::INFINITY && curvature == f64::INFINITY,
            "{cf} {cfp} {curvature}"
        );
        // (neither sign of a finite `F` is proven against it)
        let signed = |f: f64| f > cf || f < -cf;
        assert!(!signed(0.25) && !signed(-0.25));
        let inside = alignment_slopes(&eval, uv, dir, SurfParamType::U, 1.78)
            .expect("bounded inside the span");
        assert!(
            inside.f_error.is_finite() && inside.f_error < 1e-6,
            "{inside:?}"
        );
        let (cf, _, _) = charges_or_unproven(
            Some(&eval),
            &kept,
            uv,
            dir,
            SurfParamType::U,
            1.78,
            inside.f,
            0.0,
        );
        assert!(cf.is_finite(), "{cf}");
        // (the search keeps the slopes it evaluated, not the failed ones, and a kept
        // parameter asked about again, with another `f`/`f_prime`, charges exactly what
        // a fresh evaluation does)
        assert_eq!(kept.borrow().as_slice(), &[(1.78f64.to_bits(), inside)]);
        for (f, f_prime) in [(inside.f, 0.0), (0.5, -2.0), (-1.0e-3, 3.0)] {
            let again = charges_or_unproven(
                Some(&eval),
                &kept,
                uv,
                dir,
                SurfParamType::U,
                1.78,
                f,
                f_prime,
            );
            let fresh = charges_or_unproven(
                Some(&eval),
                &KeptSlopes::default(),
                uv,
                dir,
                SurfParamType::U,
                1.78,
                f,
                f_prime,
            );
            assert_eq!(again, fresh);
            assert_eq!(Some(again), slope_charges(&inside, f, f_prime));
        }
        assert_eq!(kept.borrow().len(), 1);
        // (a kind without a certified evaluator proves no slope sign, and keeps the
        // solve's own curvature for its stationary test)
        assert_eq!(
            charges_or_unproven(None, &kept, uv, dir, SurfParamType::U, u_min, 0.25, -0.5),
            (f64::INFINITY, 0.0, f64::INFINITY)
        );
    }

    /// WR-57 (goldens `topology.brep_mirror` mat0076, which hung): on gen0435's plateau
    /// (the first `u` span, where every `u` aligns equally with `dir`, see above) an input
    /// inside the span is itself an answer, and it is kept: it is the near-best guess
    /// nearest the input, and its Newton solve stops at once (`F = 0`). Started at the
    /// nearest sample instead (sample 0), the solve stops there and returns it, 0.04
    /// away; a drop curve inverting points near such a pole then jumps at every step.
    #[test]
    fn wr57_plateau_keeps_the_input_parameter() {
        let (s, u_knots, v1) = wr57_gen0435_surface();
        let k = Kernel::default();
        let dir = Vec3::new(0.9109237931558096, -0.11434982158434526, 0.6569538269953288);
        let u_in = 1.78;
        assert!(u_knots[0] < u_in && u_in < u_knots[2]);
        let r = s
            .find_degen_param_for_direction(
                &k,
                Point2::new(u_in, v1),
                dir,
                false,
                SurfParamType::U,
                1e-9,
            )
            .unwrap();
        assert!(r.converged, "{r:?}");
        assert_eq!(r.uv, Point2::new(u_in, v1));
    }

    /// A goldens case's B-spline surface, as the `surface.surface_p7` runner builds it.
    fn wr57_case_surface(
        p: usize,
        q: usize,
        u: &[f64],
        v: &[f64],
        form: u64,
        net: &[&[[f64; 4]]],
    ) -> Surface {
        use crate::nurbs::b_spline_surface::BSplineSurfaceForm;
        use crate::nurbs::math::CPoint;
        use crate::nurbs::srf_basic::Surface as NlSurface;

        let net = net
            .iter()
            .map(|r| {
                r.iter()
                    .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
                    .collect()
            })
            .collect();
        let nl = NlSurface::new(net, p, q, u.to_vec(), v.to_vec()).unwrap();
        let mut bs = BSplineSurface::from_nurb(nl, false);
        bs.set_b_spline_surface_form(BSplineSurfaceForm::from_code(form).unwrap());
        Surface::from(bs)
    }

    /// Goldens `surface.surface_p7` gen0114's surface (a rational-form cubic by quadratic
    /// B-spline, one interior `v` knot).
    fn wr57_gen0114_surface() -> Surface {
        use crate::nurbs::math::NOW;
        wr57_case_surface(
            3,
            2,
            &[
                -0.32446101115588677,
                -0.32446101115588677,
                -0.32446101115588677,
                -0.32446101115588677,
                0.2991082956226988,
                1.1334521839603298,
                1.1334521839603298,
                1.1334521839603298,
                1.1334521839603298,
            ],
            &[
                1.5665666640091418,
                1.5665666640091418,
                1.5665666640091418,
                2.0193823295327666,
                2.234324184886897,
                2.234324184886897,
                2.234324184886897,
            ],
            9,
            &[
                &[
                    [-1.0, 1.5, -0.15153150430064888, NOW],
                    [-1.0, 1.5, -0.15153150430064888, NOW],
                    [-1.0, 1.5, -0.15153150430064888, NOW],
                    [-1.0, 1.5, -0.15153150430064888, NOW],
                ],
                &[
                    [
                        0.9346349187538413,
                        -0.16025675335582035,
                        0.012554679830505372,
                        NOW,
                    ],
                    [
                        1.0238249645276218,
                        0.9670835588172848,
                        0.005199989479247688,
                        NOW,
                    ],
                    [
                        0.8836590819881716,
                        2.1718040264713885,
                        -0.03605789282354702,
                        NOW,
                    ],
                    [
                        0.8100680734850909,
                        3.029914419263989,
                        -0.028590026806983578,
                        NOW,
                    ],
                ],
                &[
                    [
                        2.0812083164083437,
                        0.1042449000603431,
                        0.033454506579291415,
                        NOW,
                    ],
                    [
                        1.8334092351486402,
                        0.8783196404547143,
                        0.020330173697674918,
                        NOW,
                    ],
                    [
                        1.8076124761268801,
                        1.8538152956620184,
                        0.0031217807615215266,
                        NOW,
                    ],
                    [
                        2.0575036852100275,
                        2.816172550559675,
                        -0.025997700295699783,
                        NOW,
                    ],
                ],
                &[
                    [
                        3.0775493249815398,
                        0.11542964057817956,
                        0.021762582745271528,
                        NOW,
                    ],
                    [
                        2.875510225870819,
                        0.9480442638540545,
                        0.03428871973876774,
                        NOW,
                    ],
                    [
                        3.1091781655812785,
                        2.000990310795575,
                        -0.0029049977583734763,
                        NOW,
                    ],
                    [
                        3.1869893078338136,
                        3.0868308232625754,
                        -0.02606005035151585,
                        NOW,
                    ],
                ],
                &[
                    [
                        3.833969286001174,
                        -0.10744550913698117,
                        0.028138706867984385,
                        NOW,
                    ],
                    [
                        3.9699382649146298,
                        0.8642542486265794,
                        0.00959694707569341,
                        NOW,
                    ],
                    [
                        3.8066465977473203,
                        2.18659814041234,
                        0.008803661549190146,
                        NOW,
                    ],
                    [
                        3.8253178134416728,
                        3.106907113681807,
                        0.005208593101187217,
                        NOW,
                    ],
                ],
            ],
        )
    }

    /// Goldens `surface.surface_p7` gen2955's surface (a rational bilinear B-spline with
    /// interior `u` knots `-1.1772...` and `0.8548...`).
    fn wr57_gen2955_surface() -> Surface {
        wr57_case_surface(
            1,
            1,
            &[
                -1.3254852418418386,
                -1.3254852418418386,
                -1.1772239791907688,
                0.8548968118086484,
                1.2523941973855002,
                1.2523941973855002,
            ],
            &[
                -1.9302366276332217,
                -1.9302366276332217,
                -0.9989840508755348,
                -0.710211746252218,
                -0.710211746252218,
            ],
            5,
            &[
                &[
                    [
                        -0.09108594431728984,
                        0.06726425996230058,
                        0.16225047069519544,
                        0.8340679260142728,
                    ],
                    [
                        0.06133422022949313,
                        1.0134173456873652,
                        -0.24296016511994936,
                        0.95422890056938,
                    ],
                    [
                        0.2603032360049141,
                        2.5775692505482084,
                        0.5121972867283143,
                        1.4040164287307144,
                    ],
                ],
                &[
                    [
                        0.6200578072999983,
                        -0.10201901237298894,
                        -0.13757287286090655,
                        0.5406586822956583,
                    ],
                    [
                        1.0082264814718493,
                        1.1557172242872473,
                        0.1218822067575605,
                        1.0910707075668147,
                    ],
                    [
                        0.751450364198601,
                        1.8304748531708714,
                        -0.03805609219977589,
                        0.8957020799968565,
                    ],
                ],
                &[
                    [
                        1.4062620044990386,
                        -0.09905531410179921,
                        0.4796068860144954,
                        0.7769848544707163,
                    ],
                    [
                        2.7460132132740718,
                        1.4894323679796777,
                        0.5851191049049435,
                        1.2589830227175194,
                    ],
                    [
                        3.919588399952234,
                        4.1750810012155535,
                        -0.5531962886367542,
                        1.9432684479346973,
                    ],
                ],
                &[
                    [
                        2.8689316792848185,
                        0.17165325862213687,
                        0.06399826410260992,
                        0.9304502894968791,
                    ],
                    [
                        2.472750767626506,
                        0.8512967598430061,
                        -0.19474881121283197,
                        0.8622581664330136,
                    ],
                    [
                        2.047024856382304,
                        1.3449767471260274,
                        0.43988409891353214,
                        0.725936352123764,
                    ],
                ],
            ],
        )
    }

    /// Goldens `surface.surface_p8` gen1302's surface (quadratic by linear, one interior
    /// `v` knot).
    fn wr57_gen1302_surface() -> Surface {
        wr57_case_surface(
            2,
            1,
            &[0.0, 0.0, 0.0, 0.7143527121849615, 1.0, 1.0, 1.0],
            &[
                1.159185021476047,
                1.159185021476047,
                1.5099273821736103,
                1.7333678245508755,
                1.7333678245508755,
            ],
            11,
            &[
                &[
                    [
                        -0.8612502506812623,
                        0.8612502506812623,
                        -0.12836797026688865,
                        0.8612502506812623,
                    ],
                    [
                        -1.5376146985135923,
                        1.5376146985135923,
                        -0.22917900777920558,
                        1.5376146985135923,
                    ],
                    [
                        -1.4081314819461375,
                        1.4081314819461375,
                        -0.209879741763034,
                        1.4081314819461375,
                    ],
                ],
                &[
                    [
                        0.9979531669974242,
                        0.09742283061658573,
                        0.5131219845229927,
                        1.0000729935941015,
                    ],
                    [
                        1.930745120925356,
                        2.0541013571046043,
                        -1.0317456419409419,
                        1.9244394871371076,
                    ],
                    [
                        0.7006180286316215,
                        1.2451744976234154,
                        -0.2787446425607277,
                        0.6549898423317401,
                    ],
                ],
                &[
                    [
                        3.149842520029806,
                        0.01731749982166718,
                        -0.6897879702289559,
                        1.4981100716204439,
                    ],
                    [
                        2.759176262730861,
                        1.03424316466755,
                        -0.6846977999648701,
                        1.2749933569554919,
                    ],
                    [
                        2.6604684019985565,
                        2.6110123593274297,
                        -0.1930400616016503,
                        1.432138762628583,
                    ],
                ],
                &[
                    [
                        5.869446702653353,
                        0.3293333771083319,
                        0.42408325412134745,
                        1.9768590222912008,
                    ],
                    [
                        3.6669679987890214,
                        1.4163398036805488,
                        0.26323783346197394,
                        1.2684675977219309,
                    ],
                    [
                        4.234343046110121,
                        2.981131594310747,
                        -0.7840684941738906,
                        1.4443778991125038,
                    ],
                ],
            ],
        )
    }

    /// The alignment `unit(w) . unit(dir)` that `FindDegenParamForDirection` maximises,
    /// `w` the derivative across the singular direction at `uv`.
    fn wr57_alignment(s: &Surface, k: &Kernel, uv: Point2, dir: Vec3, sd: SurfParamType) -> f64 {
        let (_, su, sv) = s.evaluate_1st_derivatives(k, uv, false, false).unwrap();
        let w = if sd == SurfParamType::U { sv } else { su };
        w.unitize().unwrap().dot(dir.unitize().unwrap())
    }

    /// WR-57 (goldens `surface.surface_p7` gen0114): the alignment is largest at the
    /// interval's start, `v_min`, where it falls inward (`F = A'(v_min) = -0.269`); the
    /// Newton solve from the best sample ends at an interior root of `F` with `F' > 0`, a
    /// minimum of the alignment, which upstream returns as converged. The answer is the
    /// end, a maximum of `A` on the closed interval.
    #[test]
    fn wr57_an_end_maximum_replaces_an_interior_minimum() {
        let s = wr57_gen0114_surface();
        let k = Kernel::default();
        let uv = Point2::new(1.1334521839603298, 2.0888639673485128);
        let dir = Vec3::new(
            -0.5651479095496454,
            0.26960174577835194,
            -0.8706275464533069,
        );
        let r = s
            .find_degen_param_for_direction(&k, uv, dir, true, SurfParamType::V, 1e-9)
            .unwrap();
        assert!(r.converged, "{r:?}");
        let v_min = 1.5665666640091418;
        assert_eq!(r.uv.y, v_min, "{r:?}");
        // the alignment falls inward from the end: the end is a maximum (the input `u`
        // is the domain's maximum and the search is from the pole, so it aligns with
        // `-dir`)
        let a = |v: f64| wr57_alignment(&s, &k, Point2::new(uv.x, v), -dir, SurfParamType::V);
        assert!(
            a(v_min) > a(v_min + 1e-6),
            "{} vs {}",
            a(v_min),
            a(v_min + 1e-6)
        );
    }

    /// WR-57 (goldens `surface.surface_p7` gen2955): the surface is bilinear, so the
    /// alignment's slope `F = A'` jumps at the `u` knot `0.8548968118086484`, from `+`
    /// on its left to `-` on its right: a kink maximum where `F` has no root. The solve's
    /// bisection stops beside the knot with `|F|` about 0.1; upstream returns that stop
    /// as converged. The answer is the knot itself.
    #[test]
    fn wr57_a_kink_maximum_is_the_knot() {
        let s = wr57_gen2955_surface();
        let k = Kernel::default();
        let uv = Point2::new(-1.3254828514677233, -1.9302366276332217);
        let dir = Vec3::new(0.8875674152136386, 0.5623161016560287, 0.008156115300685718);
        let r = s
            .find_degen_param_for_direction(&k, uv, dir, true, SurfParamType::U, 1e-9)
            .unwrap();
        assert!(r.converged, "{r:?}");
        let knot = 0.8548968118086484;
        assert_eq!(r.uv.x, knot, "{r:?}");
        // a strict maximum: the alignment falls on both sides
        let a = |u: f64| wr57_alignment(&s, &k, Point2::new(u, uv.y), dir, SurfParamType::U);
        assert!(a(knot) > a(knot - 1e-6) && a(knot) > a(knot + 1e-6));
    }

    /// WR-57 (goldens `surface.surface_p8` gen1302, the arguments its
    /// `InvertPointNearSingularity` passes): from the input `v = 1.3623` the solve
    /// reaches a strict local maximum at `v = 1.4461` (`F' = -1.53`), aligned 0.97669;
    /// the sample at `v = 1.6435` is aligned better, so that maximum is not the largest.
    /// The answer is the maximum the best sample's solve reaches, `v = 1.6452`, aligned
    /// 0.98359.
    #[test]
    fn wr57_a_maximum_below_the_best_sample_is_not_kept() {
        let s = wr57_gen1302_surface();
        let k = Kernel::default();
        let uv = Point2::new(0.0, 1.3623429996570933);
        let dir = Vec3::new(
            0.0830790268399677,
            0.011803425127355327,
            0.0008823108237226707,
        );
        let r = s
            .find_degen_param_for_direction(&k, uv, dir, true, SurfParamType::V, 1e-4)
            .unwrap();
        assert!(r.converged, "{r:?}");
        assert!((r.uv.y - 1.6452).abs() < 1e-3, "{r:?}");
        let a = |v: f64| wr57_alignment(&s, &k, Point2::new(0.0, v), dir, SurfParamType::V);
        // better aligned than the input's maximum, and itself a maximum
        assert!(a(r.uv.y) > a(1.4460943352873443) + 1e-3, "{}", a(r.uv.y));
        assert!(a(r.uv.y) >= a(r.uv.y - 1e-6) && a(r.uv.y) >= a(r.uv.y + 1e-6));
    }

    /// The global solve failures keep the operation, status, kind and entity of the bare
    /// `Upstream` statuses they replace (a sample).
    #[test]
    fn typed_surface_failures_keep_their_upstream_answers() {
        use crate::error::surface::SurfaceFailure;
        let samples: [(&'static str, SurfaceFailure, i32); 3] = [
            (
                OP_SOLVE,
                SurfaceFailure::Missing("the surface cache's tree"),
                crate::error::SM_ERR_NULL_POINTER,
            ),
            (
                OP_SEAM,
                SurfaceFailure::Failed("the point is at a pole"),
                crate::error::SM_ERR,
            ),
            (
                OP_DEGEN,
                SurfaceFailure::Failed("every sample tangent points away from the direction"),
                crate::error::SM_ERR,
            ),
        ];
        for (operation, failure, code) in samples {
            let typed = SolidError::surface(operation, failure);
            let before = SolidError::Upstream {
                operation,
                code,
                entity: None,
            };
            assert_eq!(typed.sm_status(), before.sm_status(), "{typed:?}");
            assert_eq!(typed.upstream_status(), Some(code), "{typed:?}");
            assert_eq!(typed.kind(), before.kind(), "{typed:?}");
            assert_eq!(typed.operation(), before.operation(), "{typed:?}");
            assert_eq!(typed.entity(), None, "{typed:?}");
            assert_eq!(typed.is_port_failure(), before.is_port_failure());
            assert_eq!(typed.is_status_answer(), before.is_status_answer());
            assert_eq!(typed.surface_failure(), Some(&failure));
        }
    }
}
