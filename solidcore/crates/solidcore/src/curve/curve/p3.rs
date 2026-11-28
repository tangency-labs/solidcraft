//! `SmCurve`, part 3: the normal of a planar curve loop, closing small gaps in a
//! curve chain, stepping a given arc length along a curve, the local (Newton)
//! solvers of the curve (point, curve/curve, curve intersection) and the local solve
//! of the curve property global solver, with their evaluator objects
//! (`SmFindPCExtremaEFO`, `SmBestLineFitENFO`).
//!
//! Upstream: `source/SMLib/src/SmCurve.cpp`, `source/SMLib/inc/SmCurve.h`.
//!
//! | Rust                                              | Upstream                                         |
//! |---------------------------------------------------|--------------------------------------------------|
//! | [`compute_crvs_normal`]                           | `SmCurve::ComputeCrvsNormal` (static)            |
//! | [`crvs_fix_gaps`], [`FixedGaps`]                  | `SmCurve::CrvsFixGaps` (static)                  |
//! | [`Curve::find_parameter_at_arc_length`], [`ArcLengthParameter`] | `SmCurve::FindParameterAtArcLength` |
//! | `Curve::find_parameter_at_arc_length_with_zone_tol` (crate helper) | `SmCurve::FindParameterAtArcLength` (edge-owned curve) |
//! | [`Curve::local_point_solve`], [`PointSolveOptions`] | `SmCurve::LocalPointSolve`                     |
//! | [`Curve::local_curve_solve`], [`local_curve_solve`] | `SmCurve::LocalCurveSolve`                     |
//! | [`local_curve_intersect`], [`CurveIntersection`]  | `SmCurve::LocalCurveIntersect`                   |
//! | [`CurvePropertyGs::local_solve_branch_with_note`], [`CurvePropertyNote`] | `SmCurvePropertyGS::LocalSolve` |
//! | [`CurvePropertyGs::local_solve_branch`] ([`GlobalSolve::local_solve`] of [`CurvePropertyGs`]) | `SER(LocalSolve(...))` in `SmGlobalSolver::SolveIt` |
//! | `EvalFunction for` [`FindPcExtremaEfo`]           | `SmFindPCExtremaEFO::Evaluate`                   |
//! | [`BestLineFitEnfo`]                               | `SmBestLineFitENFO` (class, constructor, `HasIntervals`, `SetIntervals`, `Evaluate`) |
//! | `EvalNFunctions for` [`CcTangentIntEnfo`]         | `SmCCTangentIntENFO::Evaluate` (co-ported, unit `curve.curve_p5`) |
//!
//! Deviations:
//! - **Loop normal.** `ComputeCrvsNormal` sums the sampled polygon's area vector
//!   instead of upstream's weighted turns, so reflex corners cannot reverse a planar
//!   loop's normal (WR-587). An open chain closes by its chord. As the fork's change
//!   (tangency-labs/smlib be45e8d, unchanged to eaa998a), a failed point evaluation
//!   returns its status and a zero area is `SM_ERR_INVALID_INPUT` (`SER(Unitize)`),
//!   except that an open chain whose chord-closed area has no certain direction but
//!   which spans a plane answers that plane's normal (on the area's side, or its turns'
//!   when that is uncertain), and an area within its own rounding of zero is
//!   `SM_ERR_INVALID_INPUT` too. A sample that is not finite is `SM_ERR_INVALID_INPUT`
//!   (upstream would unitize a NaN); samples whose sums overflow are scaled by a power
//!   of two first (upstream would unitize an infinity). The sampling (5 chords per curve, 1 for a
//!   two-point curve) is upstream's.
//! - **Curve kinds.** `ComputeCrvsNormal` and `CrvsFixGaps` cast every curve to
//!   `SmBSplineCurve`. `ComputeCrvsNormal` only calls virtual members and the
//!   non-virtual `SmCurve::IsLinear`, so it takes any [`Curve`]. `CrvsFixGaps` calls
//!   the non-virtual `SmBSplineCurve::SetControlPoint`: moving an end point of any
//!   kind but [`CurveKind::BSpline`] is `InvalidInput` (upstream writes through the
//!   bad cast). Its curve list is reordered by [`fix_chain_curves_list`] (with
//!   duplicate deletion, as upstream) and returned with the count.
//! - **Out-params.** `FindParameterAtArcLength` returns [`ArcLengthParameter`]:
//!   `SM_ERR_NOT_CONVERGING` (returned upstream *with* the end parameter) is
//!   [`ArcLengthParameter::converged`] `false`. The local solvers return whether an
//!   answer was found with the [`Solution`] they filled in (a fresh
//!   [`Solution::new`] stands for the caller's; fields upstream does not write keep
//!   its values); the solution objects are the caller's handles. A start parameter
//!   below the first knot when stepping backwards (an out of bounds read upstream) is
//!   `InvalidInput`; a failed `SetMinMax` keeps the previous interval, as upstream.
//! - **Owner tolerance.** The crate-private arc-length entry point carries the
//!   owning edge's resolved zone tolerance through every length cache. This is
//!   independent of Newton's integration accuracy; the public entry point keeps
//!   the ordinary unresolved-owner behavior.
//! - **Generic operands.** [`local_curve_solve`] and [`local_curve_intersect`] are
//!   generic over [`CurveEvaluate`] (so the ellipse and B-spline callers can use
//!   them). The `SM_SO_3D_SIGNED_DIRECTED_MINIMIZE` operation builds
//!   `SmProjectedCurve`s of both curves and needs [`Curve`] operands: it is
//!   [`Curve::local_curve_solve`]; the generic function returns `InvalidInput` for it.
//!   Option vectors and values an operation dereferences without a `NULL` check are
//!   `InvalidInput` when missing; upstream's `NER` checks stay `SM_ERR_NULL_POINTER` (a ray
//!   solve's missing options are [`CurveFailure::PointSolveOptionMissing`]: that status, the
//!   caller's error, not a defect).
//! - **Property solver.** `SmCurvePropertyGS::LocalSolve` reads the curve back from
//!   the tree node's owner object; here the solver carries it
//!   ([`CurvePropertyGs::curve`], `InvalidInput` when not set) and the solution
//!   objects are the tree's owner handle. `return TRUE` from the `SmStatus` function
//!   (a projected corner at the rotation origin) is not an error: the local solve
//!   returns [`CurvePropertyNote::CornerOnViewAxis`] with its result, and the solver's
//!   step ([`CurvePropertyGs::local_solve_branch`]) stops on it as `SolveIt`'s `SER`
//!   does (a curve failure whose comparison status is `1`). A node without data is `InvalidInput`
//!   with status `SM_ERR_NULL_POINTER` (`NER`; a cache's tree has data at every node); a node whose data is not a leaf span is `InvalidInput` (upstream casts
//!   whatever `m_pData` holds).
//! - **`SmFindPCExtremaEFO::Evaluate`** is implemented for [`Curve`] (it calls the
//!   virtual `IsDegenerate`). An infinite evaluated tangent returns `SM_ERR`
//!   instead of looping forever while dividing infinity by ten; finite tangents
//!   retain upstream's exact scaling operations and order. For the minimisation that
//!   decides an intersection, `F` and `F'` are divided by `|C'|` where it is below 1
//!   (not upstream, #17735): the step is unchanged, and the `|F| < SM_EFF_ZERO` stop,
//!   which left the gap's component along a short curve's tangent up to
//!   `SM_EFF_ZERO / |C'|` from a point on it (beyond the intersection's tolerance), leaves
//!   it below `SM_EFF_ZERO` (the residual stop; the gap and angle tests stop as before). A
//!   stop ends the iteration only; the intersection is decided by the foot's distance
//!   against its tolerance. The
//!   separate [`MAX_PC_SCALE_STEPS`] cap is retained as a backstop and returns
//!   [`SolidError::InvalidInput`] after 400 divisions (a finite tangent needs at
//!   most 310). Upstream loops forever on the infinite rational B-spline tangent
//!   in fuzz `curve.curve_p9` fuzz01222.
//! - **`SmBestLineFitENFO`** keeps the solver's intervals as copies; its
//!   `m_dSolutionSize` / `m_dScaledZero` are fields read through
//!   [`EvalNFunctions::solution_size`] / [`EvalNFunctions::scaled_zero`]. The status
//!   of the tangent branch's `StaticEvaluate` is ignored, as upstream.
//! - **Ignored statuses.** Where upstream ignores a status (the evaluations of
//!   `ComputeCrvsNormal`, `CrvsFixGaps` and `LocalPointSolve`'s final point, the
//!   first span lengths of `FindParameterAtArcLength`, `GetKnots`, `SetControlPoint`,
//!   `SetCanonical`) the error is ignored and the previous value kept, except
//!   [`SolidError::Unported`], which is always propagated.
//! - **Callees of unit `curve.curve_p5`** (`SmCurve::Length`,
//!   `SmCurve::LocalPropertyAnalysis`, `sm_FindAdditionalCpGuess`, `SmCurve::IsPeriodic`
//!   and the `Evaluate` members of `SmCCIntersectENFO` and `SmFindCCNormalENFO`) are
//!   wired. `FindParameterAtArcLength` takes the kernel for `Length` (its curve cache
//!   reads the kernel's tolerances); `Length` of curved spans reaches `SmIntegrator`
//!   (the ported unit `mass.integrator`).
//! - **Statuses passed on (WR-663).** The statuses SMLib drops are passed on, as the
//!   fork (tangency-labs/smlib 8311f8e):`ComputeCrvsNormal` (a zero normal is the status),
//!   `FindParameterAtArcLength`, `LocalPointSolve`'s found point, the tangent-intersection
//!   evaluators and the projected `LocalCurveSolve`'s check; `CrvsFixGaps` leaves a gap
//!   that cannot be measured or closed open (not counted).

use crate::curve::curve::p2::{
    proj_signed_angle, property_angle, unitize_tangent, Curve, CurveEvaluate, CurveKind,
    CurvePropertyGs, CurvePropertyType,
};
use crate::curve::curve::p4::{
    fix_chain_curves_list, proj_dist, CcIntersectEnfo, CcTangentIntEnfo, ChainedCurves,
    FindCcNormalEnfo, FindPcExtremaEfo,
};
use crate::curve::curve::p5::{find_additional_cp_guess, is_periodic, local_property_analysis};
use crate::curve::projected_curve::{OwnsCurve, ProjectedCurve};
use crate::error::curve::{CurveFailure, PointSolveOption};
use crate::error::{ignored, Failure, Result, SolidError};
use crate::geom::extent1d::{Extent1d, ExtentNd};
use crate::geom::geom_utility::p1::line_closest_point;
use crate::geom::geom_utility::p2::ContinuityType;
use crate::geom::global_solver::{Branch, GlobalSolve, SolverOperation};
use crate::geom::local_solve1d::{EvalFunction, FunctionValue, LocalSolve1d, TerminationReason};
use crate::geom::local_solve_nd::{BoundaryHandler, EvalNFunctions, LocalSolveNd};
use crate::geom::matrix::Matrix;
use crate::geom::solution_array::{Solution, SolutionEnd, SolutionObject, SolutionType, SortKey};
use crate::geom::tol::{are_same, are_same_to_tol};
use crate::geom::tree::NodeData;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::{max2, BIG_DOUBLE, EFF_ZERO, EFF_ZERO_RAD, EFF_ZERO_SQ, EFF_ZERO_SQRT, PI};
use crate::nurbs::b_spline_curve::{ControlPointForm, ProjectionType};

const OP_NORMAL: &str = "curve::curve::p3::compute_crvs_normal";
const OP_GAPS: &str = "curve::curve::p3::crvs_fix_gaps";
const OP_ARC: &str = "curve::curve::p3::Curve::find_parameter_at_arc_length";
const OP_POINT: &str = "curve::curve::p3::Curve::local_point_solve";
const OP_CURVE: &str = "curve::curve::p3::local_curve_solve";
const OP_GS: &str = "curve::curve::p3::CurvePropertyGs::local_solve_branch";
const OP_PC: &str = "curve::curve::p3::FindPcExtremaEfo::evaluate";
const OP_LINE_FIT: &str = "curve::curve::p3::BestLineFitEnfo::evaluate";

/// Most divisions by ten `SmFindPCExtremaEFO::Evaluate` makes to bring `|C'|^2` down
/// to `1e4` (not upstream, which loops forever on an infinite tangent). A finite
/// tangent needs at most 310: its squared length overflows to infinity until each
/// component is below about `1e154`, and then needs at most 152 more divisions.
pub const MAX_PC_SCALE_STEPS: u64 = 400;

/// `EvaluatePoint` whose status upstream ignores, or `None` when the point cannot be
/// evaluated (the fork's curve-list helpers then skip the gap, tangency-labs/smlib
/// 8311f8e); [`SolidError::Unported`] is propagated.
fn point_or_none<C: CurveEvaluate + ?Sized>(
    k: &Kernel,
    curve: &C,
    t: f64,
) -> Result<Option<Point3>> {
    ignored(curve.evaluate_point(k, t))
}

/// `Evaluate` (from the left) into an `N`-entry array; entries not written stay
/// [`Vec3::UNDEF`].
pub(super) fn eval_n<const N: usize, C: CurveEvaluate + ?Sized>(
    k: &Kernel,
    curve: &C,
    t: f64,
    num_derivs: usize,
    from_left: bool,
    non_zero_tangents: bool,
) -> Result<[Vec3; N]> {
    let v = curve.evaluate(k, t, num_derivs, from_left, non_zero_tangents)?;
    let mut out = [Vec3::UNDEF; N];
    for (d, s) in out.iter_mut().zip(v) {
        *d = s;
    }
    Ok(out)
}

/// `v / d` (upstream `operator/`, a plain division: not `operator/=`, which skips
/// tiny divisors).
fn div(v: Vec3, d: f64) -> Vec3 {
    v / d
}

/// Sets `ivl` to `[min, max]`; upstream ignores the status of `SetMinMax`, which
/// leaves the interval unchanged for `min > max`. That status is returned, as the fork
/// (`SER`, tangency-labs/smlib 8311f8e).
fn set_min_max(ivl: &mut Extent1d, min: f64, max: f64) -> Result<()> {
    ivl.set_min_max(min, max)?;
    Ok(())
}

/// `m_vStart[i] = v` / `m_vEnd[i] = v`.
pub(crate) fn set_param(end: &mut SolutionEnd, i: usize, v: f64) -> Result<()> {
    *end.param_mut(i)? = v;
    Ok(())
}

impl<C1: CurveEvaluate + ?Sized, C2: CurveEvaluate + ?Sized> EvalNFunctions
    for CcIntersectEnfo<'_, C1, C2>
{
    fn has_intervals(&self) -> bool {
        CcIntersectEnfo::has_intervals(self)
    }

    fn set_intervals(
        &mut self,
        intervals: Option<&ExtentNd>,
        periodicities: Option<&[bool]>,
    ) -> Result<()> {
        CcIntersectEnfo::set_intervals(self, intervals, periodicities);
        Ok(())
    }

    fn solution_size(&self) -> f64 {
        self.solution_size
    }

    fn scaled_zero(&self) -> f64 {
        self.scaled_zero
    }

    /// `SmCCIntersectENFO::Evaluate` ([`CcIntersectEnfo::evaluate_equations`]).
    fn evaluate(
        &mut self,
        x: &[f64],
        f: &mut [f64],
        jacobian: Option<&mut Matrix>,
    ) -> Result<bool> {
        let k = self.kernel;
        self.evaluate_equations(k, x, f, jacobian)
    }
}

impl<C1: CurveEvaluate + ?Sized, C2: CurveEvaluate + ?Sized> EvalNFunctions
    for FindCcNormalEnfo<'_, C1, C2>
{
    /// `SmFindCCNormalENFO::Evaluate` ([`FindCcNormalEnfo::evaluate_equations`]).
    fn evaluate(
        &mut self,
        x: &[f64],
        f: &mut [f64],
        jacobian: Option<&mut Matrix>,
    ) -> Result<bool> {
        let k = self.kernel;
        self.evaluate_equations(k, x, f, jacobian)
    }
}

impl<C1: CurveEvaluate + ?Sized, C2: CurveEvaluate + ?Sized> EvalNFunctions
    for CcTangentIntEnfo<'_, C1, C2>
{
    /// `SmCCTangentIntENFO::Evaluate`: passes the call along to
    /// [`CcTangentIntEnfo::static_evaluate`] (co-ported from unit `curve.curve_p5`).
    fn evaluate(
        &mut self,
        x: &[f64],
        f: &mut [f64],
        jacobian: Option<&mut Matrix>,
    ) -> Result<bool> {
        let k = self.kernel;
        // pass the call along
        CcTangentIntEnfo::static_evaluate(k, self.curve1, self.curve2, x, f, jacobian)
    }
}

// ---- ComputeCrvsNormal ------------------------------------------------------------------

/// The unit normal of a (planar) sequence of curves (`SmCurve::ComputeCrvsNormal`):
/// the sampled polygon's area vector, from 5 chords per curve (1 for a two-point
/// curve). It follows a concave planar loop's winding, and an open chain is closed by
/// its chord (WR-587; the fork's be45e8d). Upstream sums the turns between
/// consecutive chords instead, which a reflex corner can reverse. An open chain whose
/// chord-closed area has no certain direction (it can cancel exactly) but which
/// certainly spans a plane answers that plane's normal (`area_normal_into` gives the
/// rules and their derived bounds).
///
/// # Errors
///
/// `SM_ERR` for a single linear curve; those of the control point counts and the
/// point evaluations; `InvalidInput` (`SM_ERR_INVALID_INPUT`) for a zero area or one
/// within its rounding of zero (a closed loop enclosing no area, or samples that do not
/// certainly span a plane) or a sample that is not finite, as the fork's `SER`s return
/// them.
pub fn compute_crvs_normal(k: &Kernel, curves: &[Curve]) -> Result<Vec3> {
    let mut normal = Vec3::ZERO;
    crvs_normal_into(k, curves, &mut normal)?;
    Ok(normal)
}

/// [`compute_crvs_normal`] with the normal upstream's `crCrvsNormal` holds when the call
/// returns, and its status (`None` for `SM_SUCCESS`): the zero vector it is set to first,
/// the partial sum when a point evaluation fails, and `(0, 0, 1)` when the final
/// `Unitize` fails, which writes it before returning `SM_ERR_INVALID_INPUT`. For the
/// callers that read the normal whatever the status, as upstream's do.
///
/// # Errors
///
/// [`SolidError::Unported`] and [`SolidError::Cancelled`], which always propagate.
pub(crate) fn compute_crvs_normal_written(
    k: &Kernel,
    curves: &[Curve],
) -> Result<(Vec3, Option<SolidError>)> {
    let mut normal = Vec3::ZERO;
    match crvs_normal_into(k, curves, &mut normal) {
        Ok(()) => Ok((normal, None)),
        Err(e) if e.is_unported_or_cancelled() => Err(e),
        Err(e) => Ok((normal, Some(e))),
    }
}

/// `SER(crCrvsNormal.Unitize())`: `normal` unitized, or `(0, 0, 1)` (what `Unitize`
/// writes for a zero vector) and the status, mapped by `status`.
fn unitize_written(normal: &mut Vec3, status: impl FnOnce(SolidError) -> SolidError) -> Result<()> {
    match normal.unitize() {
        Ok(u) => {
            *normal = u;
            Ok(())
        }
        Err(e) => {
            *normal = Vec3::Z;
            Err(status(e))
        }
    }
}

/// Twice the sampled polygon's oriented area, summed as a fan from its first
/// point (the closing chord contributes zero to that fan), then unitized (the fork's
/// be45e8d: `SER(EvaluatePoint)` and `SER(Unitize)`, so a failed evaluation or a zero
/// area is the call's status), written into `normal` as upstream writes
/// `crCrvsNormal`.
fn crvs_normal_into(k: &Kernel, curves: &[Curve], normal: &mut Vec3) -> Result<()> {
    area_normal_into(k, curves, normal)
}

/// The unit roundoff of `f64`.
const UNIT_ROUNDOFF: f64 = f64::EPSILON / 2.0;

/// Whether a vector computed within `error` of an exact one certainly has that one's
/// direction to within the kernel's angular zero (`SM_EFF_ZERO_RAD`): the unit vectors of
/// two vectors `error` apart differ by at most `2 error / (|v| - error)`, and `Unitize`
/// rounds by a few `u` more.
fn direction_is_certain(length: f64, error: f64) -> bool {
    length > error && 2.0 * error / (length - error) + 8.0 * UNIT_ROUNDOFF <= EFF_ZERO_RAD
}

/// The fan sum `A` of `samples` about the first (the sum of `a x b` over consecutive
/// samples' offsets `a`, `b` from it) and its rounding bound `E` ([`area_normal_into`]).
fn fan_area(samples: &[Point3]) -> (Vec3, f64) {
    let mut area = Vec3::ZERO;
    let mut magnitude = 0.0;
    let mut componentwise = Vec3::ZERO;
    let mut terms = 0.0;
    if let Some(&origin) = samples.first() {
        for w in samples.windows(2) {
            let (Some(&previous), Some(&point)) = (w.first(), w.get(1)) else {
                continue;
            };
            let (a, b) = (previous - origin, point - origin);
            area += a.cross(b);
            magnitude += norm(a) * norm(b);
            componentwise += Vec3::new(
                (a.y * b.z).abs() + (a.z * b.y).abs(),
                (a.z * b.x).abs() + (a.x * b.z).abs(),
                (a.x * b.y).abs() + (a.y * b.x).abs(),
            );
            terms += 1.0;
        }
    }
    let error = (terms + 8.0) * UNIT_ROUNDOFF * magnitude.min(norm(componentwise)) * 1.01;
    (area, error)
}

/// `|v|`, without the overflow or underflow of its squares: the plain length where its
/// square is safely in range (the same bits), else the scaled one of
/// [`Vec3::length_and_unit`].
fn norm(v: Vec3) -> f64 {
    let length = v.length();
    if length.is_finite() && length > 1e-150 {
        return length;
    }
    v.length_and_unit().map_or(length, |(l, _)| l)
}

/// [`crvs_normal_into`]'s computation.
///
/// The fan sum `A` is within `E` of the exact fan area vector of the samples, the
/// smaller of two rigorous bounds over its `N` terms `a x b` of rounded differences:
/// `(N + 8) u S` (`S` the sum of `|a| |b|`: each term is within
/// `sqrt 2 gamma_4 |a| |b| < 6 u |a| |b|` of its exact value, and summing `N` vectors adds
/// at most `gamma_(N-1)` of the sum of their lengths), and `(N + 8) u |M|` (`M` the
/// vector of the componentwise sums of `|a_j b_l| + |a_l b_j|`: component `i` of a term is
/// within `gamma_4` of its `M` share, and its sum within `gamma_(N-1)` of `M_i`), which is
/// far the smaller for nearly parallel fan vectors (a long thin loop, or samples far
/// along one axis). The factor 1.01 covers the rounding of `S`, `M` and the `gamma`s.
/// Samples whose sums overflow are first scaled by a power of two (exact) to a largest
/// coordinate in `[0.5, 1)`; the tests against `SM_EFF_ZERO` stay on the unscaled
/// lengths. The answer:
/// 1. `A`'s direction when it unitizes (`|A| >= SM_EFF_ZERO`) and that direction is
///    certain to the kernel's angular zero ([`direction_is_certain`]);
/// 2. else, for an open chain that certainly spans a plane and lies in it, that plane's
///    normal ([`open_chain_plane_normal`]), on the side of `A` when `A`'s component along
///    it is certainly not zero (`|A . n| > E`), else on the side of the chain's turns;
/// 3. else a refusal when `A` has no direction at all (`|A| <= E`: zero is within its
///    rounding, as for collinear samples placed far out) or does not unitize;
/// 4. else `A`'s direction (upstream's `Unitize`), the best the samples give: within
///    `2 E / (|A| - E)` of the exact one.
fn area_normal_into(k: &Kernel, curves: &[Curve], normal: &mut Vec3) -> Result<()> {
    *normal = Vec3::ZERO;
    let mut samples: Vec<Point3> = Vec::new();
    for curve in curves {
        let domain = curve.natural_interval();
        if curve.is_linear(k, EFF_ZERO, false)?.linear && curves.len() == 1 {
            return Err(SolidError::curve(
                OP_NORMAL,
                CurveFailure::SingleLinearCurve,
            ));
        }
        let count = if curve.number_control_points()? == 2 {
            1_u32
        } else {
            5_u32
        };
        for j in 0..=count {
            let point =
                curve.evaluate_point(k, domain.evaluate(f64::from(j) / f64::from(count)))?;
            // a sample that is not finite has no normal (`Unitize` would pass a NaN on)
            if !(point.x.is_finite() && point.y.is_finite() && point.z.is_finite()) {
                *normal = Vec3::Z;
                return Err(SolidError::invalid_input(
                    OP_NORMAL,
                    "loop normal of a curve with a point that is not finite",
                ));
            }
            samples.push(point);
        }
    }
    // an open chain: its end is not its start (decided before any scaling)
    let open = match (samples.first(), samples.last()) {
        (Some(first), Some(last)) => !first.approx_eq(*last),
        _ => false,
    };
    let (mut area_vector, mut error) = fan_area(&samples);
    let mut area = norm(area_vector);
    // the power of two the samples are scaled by: `1 / scale^2` takes a scaled area back
    let mut scale = 1.0;
    if !(area.is_finite() && error.is_finite()) {
        // samples so large that the sums overflow: the same samples scaled by a power of
        // two (exact while no coordinate falls below 2^-1022) to a largest coordinate in
        // [0.5, 1) have the same normal and certainty
        let mut largest = samples
            .iter()
            .map(|p| p.max_dimension())
            .fold(0.0_f64, f64::max);
        while largest >= 1.0 {
            largest *= 0.5;
            scale *= 0.5;
        }
        for p in &mut samples {
            *p *= scale;
        }
        (area_vector, error) = fan_area(&samples);
        area = norm(area_vector);
        if !(area.is_finite() && error.is_finite()) {
            *normal = Vec3::Z;
            return Err(SolidError::invalid_input(
                OP_NORMAL,
                "loop normal of samples too large for their area vector",
            ));
        }
    }
    // the samples' own area reaches SM_EFF_ZERO (an overflow to infinity does)
    let unitizes = area / scale / scale >= EFF_ZERO;
    let direction = |normal: &mut Vec3, a: Vec3| -> Result<()> {
        // (a vector whose squared length overflows unitizes to zero: it takes the scaled way)
        if scale == 1.0 && a.length().is_finite() {
            // SER(crCrvsNormal.Unitize()): SM_ERR_INVALID_INPUT for an area below SM_EFF_ZERO
            *normal = a;
            return unitize_written(normal, |_| {
                SolidError::invalid_input(OP_NORMAL, "zero area loop normal")
            });
        }
        match a.length_and_unit() {
            Some((_, unit)) if unitizes => {
                *normal = unit;
                Ok(())
            }
            _ => {
                *normal = Vec3::Z;
                Err(SolidError::invalid_input(
                    OP_NORMAL,
                    "zero area loop normal",
                ))
            }
        }
    };
    if unitizes && direction_is_certain(area, error) {
        return direction(normal, area_vector);
    }
    if open {
        if let Some(plane) = open_chain_plane_normal(&samples, scale, area_vector, error) {
            *normal = plane;
            return Ok(());
        }
    }
    if unitizes && area <= error {
        *normal = Vec3::Z;
        return Err(SolidError::invalid_input(
            OP_NORMAL,
            "loop normal of samples whose area vector is within its rounding of zero",
        ));
    }
    direction(normal, area_vector)
}

/// The normal of the plane an OPEN chain of `samples` spans, for a chain whose area
/// vector (closed by its chord) is zero or of no certain direction: its area can cancel
/// exactly, as `(0,0)-(1,1)-(2,-2)-(2,0)`'s does, though the chain spans a plane. `None`
/// for a closed chain (its last sample `approx_eq` its first, upstream's test for a
/// closed loop: a closed loop enclosing no area has no winding, so no orientation), and
/// for samples that do not certainly span a plane or do not all lie in it (within the
/// kernel's angular zero, `SM_EFF_ZERO_RAD`, of `|p - P0|`).
///
/// The plane is that of the widest triangle `P0 Pf Pg`: `Pf` the sample farthest from
/// the first, `Pg` the one farthest from the line `P0 Pf`, `c = d x e` with
/// `d = Pf - P0`, `e = Pg - P0`. The rounded `c` is within `E = 8 u |d| |e|` of the exact
/// one (each component `d_j e_l - d_l e_j` of rounded differences is within
/// `gamma_4 (|d_j e_l| + |d_l e_j|)` of it, whose norm is at most
/// `sqrt 2 gamma_4 |d| |e| < 6 u |d| |e|`). The plane is answered only when `c` unitizes
/// (`|c| >= SM_EFF_ZERO`, the area's own bound), its direction is certain
/// ([`direction_is_certain`], within `delta = 2 E / (|c| - E) + 8 u <= SM_EFF_ZERO_RAD`)
/// and some sample `P` is off the line `P0 Pf` by more than the kernel's angular zero
/// as seen from the first (`|d x (P - P0)| > SM_EFF_ZERO_RAD |d| |P - P0|`: samples all
/// within that angle of a line are collinear, as the plane test below counts them). For a planar chain every triangle of samples
/// has this plane's normal.
///
/// Its sense is that of the area vector `area` (within `area_error`) when its component
/// along `n` is certainly not zero: `|area . n| > area_error` (`n` a unit vector, its
/// rounding adding a few `u` of `|area|`, inside the factor 1.01 of `area_error`'s
/// derivation). Otherwise it is that of the chain's turns along `n`: the sum of the cross
/// products `p x q` of its consecutive nonzero chords (as upstream's `ComputeCrvsNormal`
/// sums them, without its weights, and here only as a tie-break for a chain whose area
/// does not decide). A turn
/// `n . (p x q)` is within `e_k = (9 u + delta) |p| |q|` of the exact one along the exact
/// normal, and their sum within `(N u + delta + 9 u) T` (`T` the sum of `|p| |q|`, 1.01
/// covering the rest). The chain's own turns are no better than its points: a symmetric S
/// placed far from the origin is no longer exactly symmetric once its points are rounded,
/// so a turn below the kernel's angular zero (`|p x q| <= SM_EFF_ZERO_RAD |p| |q|`)
/// counts as none, and so does a sum below that fraction of `T` (the weighted mean turn:
/// a tolerance on the sum, not an angle). When the sum is none (a symmetric S),
/// the sense is that of the first turn that is not; with none, `None`.
///
/// `samples` are an open chain's (the caller decides that before any scaling), scaled by
/// `scale` (a power of two, 1 unless their sums overflowed): `|c| / scale^2` is the
/// unscaled `|c|`.
fn open_chain_plane_normal(
    samples: &[Point3],
    scale: f64,
    area: Vec3,
    area_error: f64,
) -> Option<Vec3> {
    let &first = samples.first()?;
    let mut d = Vec3::ZERO;
    for &p in samples {
        let v = p - first;
        if v.length_squared() > d.length_squared() {
            d = v;
        }
    }
    let (mut c, mut e) = (Vec3::ZERO, Vec3::ZERO);
    let mut off_line = false;
    for &p in samples {
        let v = p - first;
        let w = d.cross(v);
        if w.length_squared() > c.length_squared() {
            (c, e) = (w, v);
        }
        off_line |= norm(w) > EFF_ZERO_RAD * norm(d) * norm(v);
    }
    let error = 8.0 * UNIT_ROUNDOFF * norm(d) * norm(e);
    let length = norm(c);
    if !direction_is_certain(length, error) || !off_line {
        return None;
    }
    let n = if scale == 1.0 && c.length().is_finite() {
        c.unitize().ok()?
    } else if length / scale / scale >= EFF_ZERO {
        c.length_and_unit()?.1
    } else {
        return None;
    };
    let delta = 2.0 * error / (length - error) + 8.0 * UNIT_ROUNDOFF;
    // every sample lies in the plane (to the kernel's angular zero, as seen from the first;
    // `delta`, the plane's own uncertainty, is at most that)
    let off_plane = EFF_ZERO_RAD * 1.01;
    if samples
        .iter()
        .any(|&p| n.dot(p - first).abs() > off_plane * norm(p - first))
    {
        return None;
    }
    let along = area.dot(n);
    if along.abs() > area_error + 4.0 * UNIT_ROUNDOFF * norm(area) {
        return Some(if along > 0.0 { n } else { -n });
    }
    // (a turn within the kernel's angular zero of none is none)
    let per_turn = ((9.0 * UNIT_ROUNDOFF + delta) * 1.01).max(EFF_ZERO_RAD);
    let mut turns = 0.0;
    let mut weight = 0.0;
    let mut count = 0.0;
    let mut first_turn = None;
    let mut previous: Option<Vec3> = None;
    for w in samples.windows(2) {
        let (Some(&a), Some(&b)) = (w.first(), w.get(1)) else {
            continue;
        };
        let chord = b - a;
        if chord == Vec3::ZERO {
            continue;
        }
        if let Some(prev) = previous {
            let turn = n.dot(prev.cross(chord));
            let size = norm(prev) * norm(chord);
            turns += turn;
            weight += size;
            count += 1.0;
            if first_turn.is_none() && turn.abs() > per_turn * size {
                first_turn = Some(turn);
            }
        }
        previous = Some(chord);
    }
    let sum_error = (count * UNIT_ROUNDOFF * 1.01 + per_turn) * weight;
    let sense = if turns.abs() > sum_error {
        turns
    } else {
        first_turn?
    };
    if sense > 0.0 {
        Some(n)
    } else {
        Some(-n)
    }
}

// ---- CrvsFixGaps ------------------------------------------------------------------------

/// Result of [`crvs_fix_gaps`].
#[derive(Debug, Clone)]
pub struct FixedGaps {
    /// The return value: the number of end points moved.
    pub fixed: u64,
    /// The curves chained by [`fix_chain_curves_list`], with the moved end points.
    pub chained: ChainedCurves,
}

/// Moves the last control point of the B-spline `curve` to `point`
/// (`SetControlPoint(SM_CP_NON_RATIONAL, n - 1, point, 1.0)`, status ignored).
/// `false` when the point cannot be set: the fork does not count that gap as closed
/// (tangency-labs/smlib 8311f8e).
fn move_end_point(curve: &mut Curve, point: Point3) -> Result<bool> {
    let n = curve.number_control_points()?;
    let CurveKind::BSpline(c) = curve.kind_mut() else {
        return Err(SolidError::invalid_input(
            OP_GAPS,
            "end point of a curve that is not a B-spline",
        ));
    };
    let set =
        ignored(c.set_control_point(ControlPointForm::NonRational, n.wrapping_sub(1), point, 1.0))?;
    Ok(set.is_some())
}

/// Chains `curves` head to tail ([`fix_chain_curves_list`] with `tol`, deleting
/// duplicates) and closes every gap between consecutive curves of a chain, and
/// between the end and the start of a chain, that is larger than `SM_EFF_ZERO` but
/// smaller than `tol`, by moving the end point of the earlier curve onto the start
/// of the next (`SmCurve::CrvsFixGaps`).
///
/// # Errors
///
/// Those of [`fix_chain_curves_list`] and the control point counts; `InvalidInput`
/// when an end point to move belongs to a curve that is not a B-spline;
/// [`SolidError::Unported`] from the evaluations.
pub fn crvs_fix_gaps(k: &Kernel, curves: Vec<Curve>, tol: f64) -> Result<FixedGaps> {
    let mut fixed: u64 = 0;

    // head-tail them into chains - remove duplicate curves
    let mut chained = fix_chain_curves_list(k, curves.into_iter().map(Some).collect(), tol, true)?;
    let mut chain_start = chained.chain_starts.clone();
    chain_start.push(chained.curves.len());

    let missing = || SolidError::invalid_input(OP_GAPS, "chain index out of range");

    // between each curve within chain
    for j in 1..chain_start.len() {
        let low = *chain_start.get(j - 1).ok_or_else(missing)?;
        let hi = chain_start.get(j).ok_or_else(missing)?.wrapping_sub(1);
        if hi.wrapping_sub(low) < 1 {
            continue;
        }

        // every segment in the current chain
        for join_index in low..hi {
            let crv0 = chained.curves.get(join_index).ok_or_else(missing)?;
            let crv1 = chained.curves.get(join_index + 1).ok_or_else(missing)?;
            let domain0 = crv0.natural_interval();
            let domain1 = crv1.natural_interval();

            // start and end point between curves within loop/chain (as the fork, a gap
            // that cannot be measured is not closed, tangency-labs/smlib 8311f8e)
            let Some(end_pt) = point_or_none(k, crv0, domain0.max())? else {
                continue;
            };
            let Some(start_pt) = point_or_none(k, crv1, domain1.min())? else {
                continue;
            };

            // gap between current segments
            let gap = start_pt.distance(end_pt);

            // when gap is not zero but smaller than dTol
            if gap > EFF_ZERO && gap < tol {
                // set the end point to the start of the next curve
                let crv0 = chained.curves.get_mut(join_index).ok_or_else(missing)?;
                if !move_end_point(crv0, start_pt)? {
                    continue;
                }
                fixed += 1;
            }
        } // end iter every segment in the current chain
    } // end iter every distinct chain

    // check loop end to start gap and close if gap is small
    for j in 1..chain_start.len() {
        let low = *chain_start.get(j - 1).ok_or_else(missing)?;
        let hi = chain_start.get(j).ok_or_else(missing)?.wrapping_sub(1);
        let crv0 = chained.curves.get(low).ok_or_else(missing)?;
        let crv1 = chained.curves.get(hi).ok_or_else(missing)?;
        let domain0 = crv0.natural_interval();
        let domain1 = crv1.natural_interval();

        // start and end points of 'loop' for each chain
        let Some(start_pt) = point_or_none(k, crv0, domain0.min())? else {
            continue;
        };
        let Some(end_pt) = point_or_none(k, crv1, domain1.max())? else {
            continue;
        };

        // end to start gap for current chain
        let gap = start_pt.distance(end_pt);

        // when gap is small yet larger than zero
        if gap > EFF_ZERO && gap < tol {
            // Chain j-1 is closed
            // just as a check, replace end point with start point
            let crv1 = chained.curves.get_mut(hi).ok_or_else(missing)?;
            if !move_end_point(crv1, start_pt)? {
                continue;
            }
            fixed += 1;
        }
    } // end iter every distinct chain looking for closed curves

    // all done
    Ok(FixedGaps { fixed, chained })
}

// ---- FindParameterAtArcLength -----------------------------------------------------------

/// Result of [`Curve::find_parameter_at_arc_length`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArcLengthParameter {
    /// `rdEndParam`: the parameter the arc length away from the start.
    pub end_param: f64,
    /// `*pbResultPastEnd`: the length runs past the end of the curve (the end
    /// parameter is then the curve end, or where the iteration left the curve).
    pub past_end: bool,
    /// False when the Newton iteration stopped moving (upstream returns
    /// `SM_ERR_NOT_CONVERGING` with the last parameter).
    pub converged: bool,
}

impl Curve {
    /// The parameter `arc_length` (negative: backwards) along the curve from
    /// `start_param` (`SmCurve::FindParameterAtArcLength`): whole knot spans are
    /// skipped by their lengths, then Newton's method on the arc length within the
    /// last span (damped when oscillating, at most 100 iterations) finds the
    /// parameter to about `0.01 * (|arc_length| + 1) * SM_EFF_ZERO_SQRT`.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a start parameter more than `SM_EFF_ZERO` (relative) outside
    /// the knots, or below the first knot when stepping backwards (an out of bounds
    /// read upstream), or a curve without knots; those of the lengths and
    /// evaluations of the Newton iteration ([`Curve::length`]: an edge's curve borrowed
    /// from its record is [`SolidError::Unported`], use
    /// [`crate::topology::edge::evaluation_curve`]).
    ///
    /// The length caches are built at the tolerance of the curve's edge when one is
    /// recorded on the curve ([`Curve::owner_zone_tol_3d`]), as upstream reads it
    /// through the owner.
    pub fn find_parameter_at_arc_length(
        &self,
        k: &Kernel,
        start_param: f64,
        arc_length: f64,
    ) -> Result<ArcLengthParameter> {
        self.find_parameter_at_arc_length_with_zone_tol(
            k,
            start_param,
            arc_length,
            self.owner_zone_tol_3d(),
        )
    }

    /// Finds an arc-length parameter using the owning edge's resolved zone
    /// tolerance for each temporary length cache.
    pub(crate) fn find_parameter_at_arc_length_with_zone_tol(
        &self,
        k: &Kernel,
        start_param: f64,
        arc_length: f64,
        owner_zone_tol_3d: Option<f64>,
    ) -> Result<ArcLengthParameter> {
        let mut out = ArcLengthParameter {
            end_param: 0.0,
            past_end: false,
            converged: true,
        };
        let mut start_param = start_param;
        let mut arc_length = arc_length;

        let orig_ivl = self.natural_interval();
        let param_tol = (orig_ivl.length() + 1.0) * EFF_ZERO;

        // Set up the tolerance to produce about 6 decimal places of accuracy.
        let mut tol = (arc_length.abs() + 1.0) * EFF_ZERO_SQRT;

        // Why only 6?  Partly because Length() complains.  So make it tighter, but
        // not enough to get complaints from Length().
        tol *= 0.01;

        // This will be the absolute value of the required arc length.
        // It will decrease as we cover whole spans.
        let mut abs_len_needed = arc_length;
        let mut increasing = true;
        if arc_length < 0.0 {
            increasing = false;
            abs_len_needed = -arc_length;
        }

        // First, find the span containing dStartParam. (As the fork, the statuses of
        // the knots, intervals and span lengths are passed on, tangency-labs/smlib
        // 8311f8e.)
        let with_status = self.knots_with_status(k, None)?;
        with_status.status?;
        let knots = with_status.knots;
        let num_knots = knots.len() as i64;
        let knot = |i: i64| -> Result<f64> {
            usize::try_from(i)
                .ok()
                .and_then(|i| knots.get(i))
                .copied()
                .ok_or(SolidError::invalid_input(OP_ARC, "knot index out of range"))
        };

        // Ruling this out will make later checks easier:
        if start_param < knot(0)? - param_tol || start_param > knot(num_knots - 1)? + param_tol {
            return Err(SolidError::invalid_input(
                OP_ARC,
                "start parameter outside the curve",
            ));
        }

        let mut ik: i64;

        // Callers usually start at one end or the other:
        if increasing && start_param <= knot(0)? {
            ik = 0;
        } else if !increasing && start_param >= knot(num_knots - 1)? {
            ik = num_knots - 2;
        } else {
            ik = 0;
            while knot(ik)? <= start_param && ik < num_knots - 1 {
                ik += 1;
            }
            ik -= 1;
        }

        // Move down spans until the length to dStartParam is within the span
        let span_step: i64 = if increasing { 1 } else { -1 };
        let mut interval = Extent1d::new();
        if increasing {
            set_min_max(&mut interval, start_param, knot(ik + 1)?)?;
        } else {
            set_min_max(&mut interval, knot(ik)?, start_param)?;
        }

        let mut span_len = self.length_with_zone_tol(k, &interval, tol, owner_zone_tol_3d)?;

        while span_len <= abs_len_needed {
            abs_len_needed -= span_len;

            // Check for serendipity
            if abs_len_needed < tol {
                out.end_param = if increasing { knot(ik + 1)? } else { knot(ik)? };
                return Ok(out);
            }

            ik += span_step; // move up or down to the next span

            if ik >= num_knots - 1 || ik < 0 {
                // We've fallen off the end of the curve without reaching len.
                // Return a good status, just a warning if asked for.
                out.end_param = if increasing {
                    knot(num_knots - 1)?
                } else {
                    knot(0)?
                };
                out.past_end = true;
                return Ok(out);
            }

            set_min_max(&mut interval, knot(ik)?, knot(ik + 1)?)?;
            span_len = self.length_with_zone_tol(k, &interval, tol, owner_zone_tol_3d)?;

            // Reset dStartParam to the 'beginning' of the span:
            // bottom if increasing, top otherwise.
            start_param = if increasing { knot(ik)? } else { knot(ik + 1)? };
        }

        // How much remaining, within this span:
        arc_length = if increasing {
            abs_len_needed
        } else {
            -abs_len_needed
        };

        // Prorate the current interval to get a guess for dCurrT.
        let frac = interval.length() * (arc_length / span_len);
        let mut curr_t = start_param + frac;

        // and set dCurrLen according to dCurrT
        if increasing {
            set_min_max(&mut interval, start_param, curr_t)?;
        } else {
            set_min_max(&mut interval, curr_t, start_param)?;
        }
        let mut curr_len = self.length_with_zone_tol(k, &interval, tol, owner_zone_tol_3d)?;
        if !increasing {
            curr_len = -curr_len;
        }

        let mut prev_delta = BIG_DOUBLE;
        let mut off_end_count: u64 = 0;
        let max_iter: u64 = 100; // (really high, for a Newton iteration)

        for iter in 0..max_iter {
            // Newton step: function value to be zeroed is the difference between
            // the arc length to the current parameter value dCurrT and the
            // desired arc length.  Its deriv is just the magnitude of the
            // curve deriv at the current point.
            let fcn_val = curr_len - arc_length;

            if fcn_val.abs() < tol {
                break; // success.
            }

            // exact Tangent values
            let deriv: [Vec3; 2] = eval_n(k, self, curr_t, 1, true, false)?;
            let fcn_deriv = deriv[1].length();
            let mut delta_t = -fcn_val / fcn_deriv;

            // Check for no movement
            if delta_t.abs() < param_tol {
                out.converged = false;
                break;
            }

            // Check for no change in movement
            if (delta_t - prev_delta).abs() < param_tol && iter > 30 {
                out.converged = false;
                break;
            }

            // Apply the step
            let old_t = curr_t;
            curr_t += delta_t;

            // Check for falling off the end (an unsigned count upstream)
            if curr_t < orig_ivl.min() && delta_t < 0.0 {
                off_end_count = off_end_count.wrapping_sub(1);
            } else if curr_t > orig_ivl.max() && delta_t > 0.0 {
                off_end_count = off_end_count.wrapping_add(1);
            }

            // clamp before checking off-end
            curr_t = orig_ivl.clamp_value(curr_t);
            delta_t = curr_t - old_t;

            if off_end_count >= 3 {
                out.past_end = true;
                break; // but still return good status; just a warning
            }

            // Apply damping if oscillating.
            if delta_t.abs() > (0.9999 * prev_delta).abs() && delta_t * prev_delta < 0.0 {
                delta_t /= 2.0;
                curr_t = old_t + delta_t;
                curr_t = orig_ivl.clamp_value(curr_t);
            }
            prev_delta = delta_t;

            // Find the function value at current param: get arc length.
            if increasing {
                set_min_max(&mut interval, start_param, curr_t)?;
            } else {
                set_min_max(&mut interval, curr_t, start_param)?;
            }

            curr_len = self.length_with_zone_tol(k, &interval, tol, owner_zone_tol_3d)?;

            if !increasing {
                curr_len = -curr_len;
            }
        }

        out.end_param = curr_t;
        Ok(out)
    }
}

// ---- SmFindPCExtremaEFO -----------------------------------------------------------------

impl EvalFunction for FindPcExtremaEfo<'_, Curve> {
    /// `SmFindPCExtremaEFO::Evaluate`: `F = (C(t) - P) . C'(t)` and
    /// `F' = C'.C' + (C - P).C''` (the second order term limited to `-0.95 C'.C'`,
    /// negated for [`SolverOperation::Maximize`]), all scaled by the power of ten
    /// picked at the first call to keep `|C'|^2 <= 1e4`; when intersecting, both divided
    /// by `|C'|` where it is below 1 (not upstream, #17735). Found when the gap or its
    /// squared angle from the normal plane is below the squared tolerance. A zero
    /// tangent is a solution on a degenerate curve and `SM_ERR` otherwise; a zero gap
    /// is `SM_ERR` when maximising.
    fn evaluate(&mut self, t: f64, out: &mut FunctionValue, _signal_errors: bool) -> Result<()> {
        let k = self.kernel;
        // init output
        out.f = 0.0;
        out.f_prime = 0.0;
        out.found_answer = false;

        // curve point
        let side = t < self.domain.mid();
        let mut pv: [Vec3; 3] = eval_n(k, self.curve, t, 2, side, true)?; // nonZeroTangent values

        // gap: sV = CurvePoint - TestPoint
        let mut sv = pv[0] - self.test_point;

        // Scale the problem down to a reasonable size (0-10000)
        if self.have_scale {
            let mut scale = 1.0;
            for _kk in 0..self.num_scale {
                scale *= 10.0;
            }
            if scale > 1.0 {
                sv = div(sv, scale);
                pv[0] = div(pv[0], scale);
                pv[1] = div(pv[1], scale);
                pv[2] = div(pv[2], scale);
            }
        } else {
            while pv[1].length_squared() > 1.0e4 {
                if pv[1].x.is_infinite() || pv[1].y.is_infinite() || pv[1].z.is_infinite() {
                    return Err(SolidError::curve(
                        OP_PC,
                        CurveFailure::InfiniteTangent { t },
                    ));
                }
                // (not upstream) an infinite tangent never scales down: see
                // MAX_PC_SCALE_STEPS
                if self.num_scale >= MAX_PC_SCALE_STEPS {
                    return Err(SolidError::invalid_input(
                        OP_PC,
                        "tangent too large to scale (upstream loops forever)",
                    ));
                }
                self.num_scale += 1;
                sv = div(sv, 10.0);
                pv[0] = div(pv[0], 10.0);
                pv[1] = div(pv[1], 10.0);
                pv[2] = div(pv[2], 10.0);
            }
            self.have_scale = true;
        }

        // Check for a zero length tangent.
        // If it's because of a degenerate curve, then we're already at a solution.
        // If not degenerate, we're at a singularity and can't do much.
        let tan_dot_tan = pv[1].dot(pv[1]);
        if tan_dot_tan < EFF_ZERO_SQ {
            // degenerate curve?  Just return that we are perp, where we are.
            if self.curve.is_degenerate(k, EFF_ZERO, None)? {
                out.f = 0.0;
                out.found_answer = true;
                return Ok(());
            }
            return Err(SolidError::curve(OP_PC, CurveFailure::ZeroTangent { t }));
        } // end zero tangent check

        let v_dot_v = sv.dot(sv);
        let v_dot_tan = sv.dot(pv[1]);
        let mut v_dot_2nd = sv.dot(pv[2]);

        // Evaluate Func = (C(t) - P) * C'(t)
        out.f = v_dot_tan;

        // 1st Deriv = C'(t) * C'(t) + (C(t) - P) * C"(t), the second order term
        // limited so that it cannot change the sign of the step. [B52]
        let lim_2d_deriv = 0.95; // Smaller values can slow down convergence.

        // Not upstream (WR-740, #2546): upstream holds F' at 0.05 C'.C' wherever the second
        // order term is below -0.95 C'.C'. Near a centre of curvature the true F' is
        // C'.C' (rho / R) for a point rho from the centre of a circle of radius R, so every
        // step there was (rho / R) / 0.05 of Newton's and the solve ran out of iterations
        // from within about R / 100 of the centre: no closest point on an arc from near its
        // centre. When minimising, where the full F' is positive (a minimum's side of the
        // centre of curvature) and the gap is within the limit's own angle of the normal
        // (|cos(gap, C')| below 0.05), F' is the full one, bounded below by
        // |cos(gap, C')| C'.C' so that a step never moves the foot along the curve farther
        // than the gap is long. Everywhere else it is upstream's: near the answer the step
        // is Newton's, far from it (where a full step would overshoot) upstream's damping.
        let full = tan_dot_tan + v_dot_2nd;
        let limit = 1.0 - lim_2d_deriv;
        let cos_sq = if v_dot_v > 0.0 {
            (v_dot_tan * v_dot_tan) / (v_dot_v * tan_dot_tan)
        } else {
            0.0
        };
        out.f_prime = if self.solver_operation == SolverOperation::Minimize
            && full > 0.0
            && cos_sq < limit * limit
        {
            full.max(cos_sq.sqrt() * tan_dot_tan)
        } else {
            if v_dot_2nd < -tan_dot_tan * lim_2d_deriv {
                v_dot_2nd = -tan_dot_tan * lim_2d_deriv;
            }
            tan_dot_tan + v_dot_2nd
        };

        // Set sign of 1st Derive for maximize
        if self.solver_operation == SolverOperation::Maximize {
            // Let - sign of FofT rule
            out.f_prime = -out.f_prime;
        }

        // (not upstream, #17735) For an intersection, along a tangent shorter than 1, F
        // and F' are divided by |C'|: F is then the gap's component along the unit
        // tangent and the Newton step -F/F' is unchanged. The solve stops at
        // |F| < SM_EFF_ZERO, and F = (C - P) . C' is that component times |C'|: undivided,
        // a short tangent stopped it with the component up to SM_EFF_ZERO / |C'|, so a
        // point on a 0.03 mm cubic was found 2e-11 off it and its intersection within the
        // scaled zero (1.5e-11) reported no point. Divided, the residual stop leaves the
        // component below SM_EFF_ZERO whatever the tangent (|C'| >= 1 already did). The
        // angular test below, unchanged, stops with the gap within SM_EFF_ZERO of square
        // to the tangent, a component below SM_EFF_ZERO times the gap. Neither bounds the
        // distance (a stationary point that is not the point stops it too, as it did
        // before): a stop only ends the iteration, and the intersection keeps the foot
        // only when its distance |C - P| is within the tolerance
        // (`CpGlobalSolver::local_solve_branch`). The tangent is not zero here (the test
        // above).
        if self.intersecting && tan_dot_tan < 1.0 {
            let tan_len = tan_dot_tan.sqrt();
            out.f /= tan_len;
            out.f_prime /= tan_len;
        }

        // Done when GapSize goes to zero
        if v_dot_v < self.squared_tolerance {
            // for SM_SO_MINIMIZE = SUCCESS
            //     SM_SO_MAXIMIZE = FAILURE
            if self.solver_operation == SolverOperation::Maximize {
                return Err(SolidError::curve(
                    OP_PC,
                    CurveFailure::PointOnCurveWhenMaximising { t },
                ));
            }
            out.found_answer = true;
            return Ok(());
        }

        // Find gap/CurveTangent angle from perpendicular squared
        // Test this to see if we satisfy our tolerance
        let gap_tan_angle_from_perp_squared = (v_dot_tan * v_dot_tan) / (v_dot_v * tan_dot_tan);
        if gap_tan_angle_from_perp_squared < self.squared_tolerance {
            out.found_answer = true;
            return Ok(());
        }

        // all done
        Ok(())
    }
}

// ---- SmBestLineFitENFO ------------------------------------------------------------------

/// The (unfinished upstream) best line fit equations (`SmBestLineFitENFO`): at
/// `x = [s1, s2]`, the gap `C1(s1) - C2(s2)` dotted with the unit tangents of both
/// curves, switching to the tangent intersection equations
/// ([`CcTangentIntEnfo::static_evaluate`]) near a tangent intersection.
#[derive(Debug, Clone)]
pub struct BestLineFitEnfo<'a, C1: ?Sized, C2: ?Sized> {
    /// Caller context for virtual evaluations.
    pub kernel: &'a Kernel,

    /// `m_crCurve1`.
    pub curve1: &'a C1,
    /// `m_crCurve2`.
    pub curve2: &'a C2,
    /// `m_cpIntervals` (`None` for `NULL`).
    pub intervals: Option<ExtentNd>,
    /// `m_cpPeriodicities` (`None` for `NULL`).
    pub periodicities: Option<Vec<bool>>,
    /// `m_dSquaredTolerance` (not used).
    pub squared_tolerance: f64,
    /// `m_dSolutionSize`: the gap length of the last evaluation.
    pub solution_size: f64,
    /// `m_dScaledZero`: `SM_EFF_ZERO` scaled by the size of the last points.
    pub scaled_zero: f64,
}

impl<'a, C1: ?Sized, C2: ?Sized> BestLineFitEnfo<'a, C1, C2> {
    /// `SmBestLineFitENFO(crCurve1, crCurve2, dSquaredTolerance)`: no intervals or
    /// periodicities; the base class's solution size `0` and scaled zero `1`.
    #[must_use]
    pub const fn new(
        k: &'a Kernel,
        curve1: &'a C1,
        curve2: &'a C2,
        squared_tolerance: f64,
    ) -> Self {
        Self {
            kernel: k,
            curve1,
            curve2,
            intervals: None,
            periodicities: None,
            squared_tolerance,
            solution_size: 0.0,
            scaled_zero: 1.0,
        }
    }
}

impl<C1: CurveEvaluate + ?Sized, C2: CurveEvaluate + ?Sized> EvalNFunctions
    for BestLineFitEnfo<'_, C1, C2>
{
    fn solution_size(&self) -> f64 {
        self.solution_size
    }

    fn scaled_zero(&self) -> f64 {
        self.scaled_zero
    }

    /// `HasIntervals`: always true.
    fn has_intervals(&self) -> bool {
        true
    }

    /// `SetIntervals`: keeps (copies of) the solver's intervals and periodicities.
    fn set_intervals(
        &mut self,
        intervals: Option<&ExtentNd>,
        periodicities: Option<&[bool]>,
    ) -> Result<()> {
        self.intervals = intervals.cloned();
        self.periodicities = periodicities.map(<[bool]>::to_vec);
        Ok(())
    }

    /// `Evaluate`: `F = [gap . u1, gap . u2]` with the unit tangents `u` (limited
    /// second order terms in the Jacobian), or the tangent intersection equations
    /// when the gap is below `0.1` and the tangents are parallel within 0.01 degree;
    /// `rbFoundAnswer` is always false. `InvalidInput` for `x` or `f` shorter than 2.
    fn evaluate(
        &mut self,
        x: &[f64],
        f: &mut [f64],
        jacobian: Option<&mut Matrix>,
    ) -> Result<bool> {
        let k = self.kernel;
        let (Some(&x0), Some(&x1)) = (x.first(), x.get(1)) else {
            return Err(SolidError::invalid_input(
                OP_LINE_FIT,
                "need two parameters",
            ));
        };
        if f.len() < 2 {
            return Err(SolidError::invalid_input(
                OP_LINE_FIT,
                "need two function values",
            ));
        }

        // init output
        let mut found = false;

        // Evaluate both curves; need two derivatives.
        let pvv1: [Vec3; 3] = eval_n(k, self.curve1, x0, 2, true, true)?; // nonZeroTangent values
        let pvv2: [Vec3; 3] = eval_n(k, self.curve2, x1, 2, true, true)?; // nonZeroTangent values

        // get the gap - save the SolutionSize
        let gap_vec = pvv1[0] - pvv2[0];
        self.solution_size = gap_vec.length();
        self.scaled_zero =
            EFF_ZERO * (1.0 + max2(pvv1[0].max_dimension(), pvv2[0].max_dimension()));

        // Check for nearby tangent intersection cases - this allows
        // large near maxima conditions to 'fall off' the hill. [B98 B115]
        let switch_to_tan_fcns = true;

        let ang_lim_deg = 0.01; // Note: .01 works better than 2.0.
        let dist_lim_sq = 0.01;
        let is_tangent = switch_to_tan_fcns
            && gap_vec.length_squared() < dist_lim_sq
            && pvv1[1].is_parallel_to(pvv2[1], ang_lim_deg);

        // when near a tangent curve/curve intersection
        if is_tangent {
            // switch to the TangentIntersection Solver Objective Functions
            // (status ignored upstream; passed on as the fork, tangency-labs/smlib
            // 8311f8e)
            found = CcTangentIntEnfo::static_evaluate(k, self.curve1, self.curve2, x, f, jacobian)?;
        } else {
            // the usual case, not tangent.
            // Get the unitized derivatives.
            let (c1_t_unit, _) = unitize_tangent(pvv1, -gap_vec);
            let (c2_t_unit, _) = unitize_tangent(pvv2, gap_vec);

            // Function values:
            if let [f0, f1, ..] = f {
                *f0 = gap_vec.dot(c1_t_unit);
                *f1 = gap_vec.dot(c2_t_unit);
            }

            // Derivatives:
            if let Some(j) = jacobian {
                // Deriv of the unitized C_t  ( C_t / || C_t || )
                let c1_t_unit_t = pvv1[1].unitized_derivative(pvv1[2]);
                let c2_t_unit_t = pvv2[1].unitized_derivative(pvv2[2]);

                let term11 = pvv1[1].dot(c1_t_unit);
                let mut term12 = gap_vec.dot(c1_t_unit_t);
                let term21 = -pvv2[1].dot(c2_t_unit);
                let mut term22 = gap_vec.dot(c2_t_unit_t);

                if term12 < -term11 / 2.0 {
                    // note, dTerm11 > 0 always
                    term12 = -term11 / 2.0;
                }
                if term22 > -term21 / 2.0 {
                    // note, dTerm21 < 0 always
                    term22 = -term21 / 2.0;
                }
                j.set_at(0, 0, term11 + term12)?;
                j.set_at(0, 1, -pvv2[1].dot(c1_t_unit))?;
                j.set_at(1, 0, pvv1[1].dot(c2_t_unit))?;
                j.set_at(1, 1, term21 + term22)?;
            }
        } // end not near tangent branch

        Ok(found)
    }
}

// ---- LocalPointSolve --------------------------------------------------------------------

/// The optional inputs of [`Curve::local_point_solve`].
#[derive(Debug, Clone, Copy, Default)]
pub struct PointSolveOptions<'a> {
    /// `cpdOptDistanceTolerance`: largest ray / curve gap for
    /// [`SolverOperation::Rayfire`] and [`SolverOperation::Signed3dDirectedMinimize`]
    /// (required there).
    pub distance_tolerance: Option<f64>,
    /// `cpdOptTargetDistance`: the distance of [`SolverOperation::AtDistance`]
    /// (required there).
    pub target_distance: Option<f64>,
    /// `cpOptVectors`: the ray direction (first entry) of
    /// [`SolverOperation::Rayfire`] and [`SolverOperation::Signed3dDirectedMinimize`]
    /// (required there).
    pub vectors: Option<&'a [Vec3]>,
}

/// A ray solve called without a required option (upstream `NER` on it): the caller's
/// error, with the status the C++ reports.
#[track_caller]
fn missing_ray_option(option: PointSolveOption) -> SolidError {
    SolidError::curve(OP_POINT, CurveFailure::PointSolveOptionMissing { option })
}

impl Curve {
    /// The parameter in `interval` near `guess` solving `op` for `test_point`
    /// (`SmCurve::LocalPointSolve`): the closest / farthest point or a normal foot
    /// ([`SolverOperation::Minimize`], `Maximize`, `Normalize`, `Intersect`) by
    /// Newton's method on [`FindPcExtremaEfo`] (a solution clamped to the interval
    /// counts, except when normalising), a point at the target distance
    /// ([`SolverOperation::AtDistance`], by [`crate::curve::curve::p4::CpDistAtEnfo`]),
    /// or the curve point closest to a ray ([`SolverOperation::Rayfire`],
    /// `Signed3dDirectedMinimize`, through `LocalPropertyAnalysis`). Returns whether
    /// an answer was found and the solution (type, value and first parameter; the
    /// whole `LocalPropertyAnalysis` solution for a ray).
    ///
    /// # Errors
    ///
    /// [`CurveFailure::PointSolveOptionMissing`] (status `SM_ERR_NULL_POINTER`, kind
    /// `InvalidInput`) for a ray without distance tolerance or vectors; `InvalidInput` for an empty vector array or a missing target distance;
    /// `SM_ERR` when the at-distance solver fails; those of the evaluations and of
    /// `LocalPropertyAnalysis`. A failing Newton iteration of the other operations is no
    /// answer, as upstream.
    pub fn local_point_solve<O: Copy>(
        &self,
        k: &Kernel,
        interval: &Extent1d,
        op: SolverOperation,
        test_point: Point3,
        opts: &PointSolveOptions<'_>,
        guess: f64,
    ) -> Result<(bool, Solution<O>)> {
        self.local_point_solve_held(k, interval, op, test_point, opts, guess, false)
    }

    /// [`Curve::local_point_solve`] for a minimisation that decides an intersection
    /// when `intersecting` (not upstream, #17735): the Newton solve's residual stop
    /// (`|F| < SM_EFF_ZERO`) then leaves the gap's component along the tangent below
    /// `SM_EFF_ZERO` ([`FindPcExtremaEfo::intersecting`]). The solve also stops on
    /// [`FindPcExtremaEfo`]'s own tests, a gap below `SM_EFF_ZERO` or within an angle of
    /// `SM_EFF_ZERO` of square to the tangent, which bounds that component only relative to
    /// the gap. Whichever stop ends it, the solution's value is the foot's distance from the
    /// point, which the intersection compares with its tolerance.
    ///
    /// # Errors
    ///
    /// As [`Curve::local_point_solve`].
    pub(crate) fn local_point_solve_held<O: Copy>(
        &self,
        k: &Kernel,
        interval: &Extent1d,
        op: SolverOperation,
        test_point: Point3,
        opts: &PointSolveOptions<'_>,
        guess: f64,
        intersecting: bool,
    ) -> Result<(bool, Solution<O>)> {
        let mut solution = Solution::new();

        // Final solution will be within the interval.
        let guess = interval.clamp_value(guess);

        // for RAYFIRE and SIGNED_MINIMIZE
        if op == SolverOperation::Rayfire || op == SolverOperation::Signed3dDirectedMinimize {
            // check inputs
            let dist_tol = opts
                .distance_tolerance
                .ok_or_else(|| missing_ray_option(PointSolveOption::DistanceTolerance))?;
            let vectors = opts
                .vectors
                .ok_or_else(|| missing_ray_option(PointSolveOption::Vectors))?;
            let vec0 = *vectors
                .first()
                .ok_or(SolidError::invalid_input(OP_POINT, "empty option vectors"))?;

            // locals
            let mut sol = Solution::new();
            let vecs = [test_point, vec0];

            // Find point on curve with closest projection to [testPoint, OptVec[0]] plane
            let found = local_property_analysis(
                k,
                self,
                interval,
                CurvePropertyType::ProjectedPointMinimize,
                guess,
                Some(dist_tol),
                &vecs,
                &mut sol,
            )?;

            // quit when no answer was found
            if !found {
                return Ok((false, solution));
            }

            // get Curve solution point
            let pnt = self.evaluate_point(k, sol.start.param(0)?)?;

            // Project CurvePoint to [testPoint, OptVec[0]] line
            let param = line_closest_point(vecs[0], vecs[1], pnt);
            let pnt_on_vec = vecs[0] + param * vecs[1];

            // when Ray/Curve gap is less than optDistTol
            if pnt_on_vec.distance(pnt) < dist_tol {
                // output the solution
                sol.start.solution_value = param;
                return Ok((true, sol));
            }

            return Ok((false, solution));
        } // end SolverOperation == RAYFIRE or SIGNED_DIRECTED_MINIMIZE check

        if op == SolverOperation::AtDistance {
            let target = opts.target_distance.ok_or(SolidError::invalid_input(
                OP_POINT,
                "missing target distance",
            ))?;
            let mut intervals = ExtentNd::new(4);
            let unbounded = Extent1d::from_raw(-BIG_DOUBLE, BIG_DOUBLE);
            for (i, ivl) in [*interval, unbounded, unbounded, unbounded]
                .into_iter()
                .enumerate()
            {
                if let Some(slot) = intervals.get_mut(i) {
                    *slot = ivl;
                }
            }
            // Otherwise it will stop at ends.
            let periodicities = [false; 4];

            let mut eval = crate::curve::curve::p4::CpDistAtEnfo::new(
                k,
                self,
                test_point,
                target,      // Desired GapSizeSq
                EFF_ZERO_SQ, // currently not used
            );
            let mut ls = LocalSolveNd::new(&mut eval, Some(&intervals), Some(&periodicities));

            let guess_t = [guess, 0.1, 0.1, 0.1];
            let r = match ls.solve_it(&guess_t, EFF_ZERO) {
                Ok(r) => r,
                Err(e) if e.is_unported_or_cancelled() => return Err(e),
                // some sort of error in solver
                Err(e) => {
                    return Err(Failure::new(OP_POINT, CurveFailure::LocalSolveFailed)
                        .caused_by(e)
                        .into_error());
                }
            };
            if r.found {
                let t0 = r.x.first().copied().unwrap_or(0.0);
                solution.solution_type = SolutionType::SingleValue;
                set_param(&mut solution.start, 0, t0)?;
                let eval_pnt = self.evaluate_point(k, t0)?;
                solution.start.solution_value = eval_pnt.distance(test_point);
                return Ok((true, solution));
            }
            return Ok((false, solution));
        } // end eSolverOperation == SM_SO_AT_DISTANCE check

        // For all other operation cases - Use Newton-Raphson to solve
        let mut efo = FindPcExtremaEfo::new(
            k,
            self,
            test_point,
            *interval,
            EFF_ZERO_SQ, // NR iter stops when gapSize or Angle from Perp gets smaller than SM_EFF_ZERO
            op,
        );
        efo.intersecting = intersecting;
        let mut ls = LocalSolve1d::with_interval(*interval, false);
        // acceptable accuracy when Func(t) < tolerance
        let r = match ls.solve_it(&mut efo, guess, 1.0e-8) {
            Ok(r) => r,
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            // some solver error - return without answer
            Err(_) => return Ok((false, solution)),
        };

        // when SolveIt failed to converge
        if !r.found {
            // Don't report failures to converge due to hitting bounds
            // (except for operation = SM_SO_NORMALIZE) because these
            // failures are really successful bounded value solutions
            // reported in dFoundT.
            if ls.termination_reason() != TerminationReason::OutOfBounds
                || op == SolverOperation::Normalize
            {
                // when normalizing must be within bounds
                return Ok((false, solution));
            }
        } // end no solution branch

        // else when SolveIt did converge or found a bounded value
        //   let solutionValue = dist(TestPoint,SolutionPoint)
        // (status ignored upstream; passed on as the fork, tangency-labs/smlib 8311f8e)
        let found_pnt = self.evaluate_point(k, r.t)?;

        solution.solution_type = SolutionType::SingleValue;
        solution.start.solution_value = found_pnt.distance(test_point);
        set_param(&mut solution.start, 0, r.t)?;

        // all done
        Ok((true, solution))
    }
}

// ---- LocalCurveSolve --------------------------------------------------------------------

/// The local curve/curve solve of every operation but
/// [`SolverOperation::Signed3dDirectedMinimize`] (see [`local_curve_solve`]).
fn curve_solve_newton<O: Copy, C1, C2>(
    k: &Kernel,
    this: &C1,
    interval: &Extent1d,
    other: &C2,
    other_interval: &Extent1d,
    op: SolverOperation,
    dist_tol_3d: f64,
    target_distance: Option<f64>,
    opt_vectors: Option<&[Vec3]>,
    guesses: [f64; 2],
    objects: [Option<O>; 2],
) -> Result<(bool, Solution<O>)>
where
    C1: CurveEvaluate + ?Sized,
    C2: CurveEvaluate + ?Sized,
{
    let mut solution = Solution::new();

    let mut intervals = ExtentNd::new(2);
    for (i, ivl) in [*interval, *other_interval].into_iter().enumerate() {
        if let Some(slot) = intervals.get_mut(i) {
            *slot = ivl;
        }
    }

    // Local solvers should not jump across seams:
    let periodicities = [false, false];

    let mut at_dist_sq = 0.0;
    if let Some(d) = target_distance {
        at_dist_sq = d * d;
    }

    let mut eval = FindCcNormalEnfo::new(
        k,
        this,
        other,
        EFF_ZERO * EFF_ZERO,
        at_dist_sq,
        op,
        opt_vectors,
    );
    let mut ls = LocalSolveNd::new(&mut eval, Some(&intervals), Some(&periodicities));

    ls.set_boundary_handler(BoundaryHandler::TotalBoundaryHits, 8);

    if op == SolverOperation::AtDistance {
        ls.set_maximum_iterations(300); // This one tends to have slower convergance
    }
    let r = ls.solve_it(&guesses, dist_tol_3d)?;
    let sol0 = r.x.first().copied().unwrap_or(0.0);
    let sol1 = r.x.get(1).copied().unwrap_or(0.0);

    // Fill in rSolution in any event.  It might be a better solution than
    // the input guesses, and the caller can check for that if desired.
    solution.solution_type = SolutionType::SingleValue;
    solution.num_objects = 2;
    solution.objects[0] = objects[0];
    solution.objects[1] = objects[1];
    let pnt1 = this.evaluate_point(k, sol0)?;
    let pnt2 = other.evaluate_point(k, sol1)?;
    solution.start.solution_value = pnt1.distance(pnt2);

    // Note that for projected operations we only have one vector and that
    // is the normal.  For directed operations the direction vector is also
    // available.
    if matches!(
        op,
        SolverOperation::ProjectedMinimize
            | SolverOperation::ProjectedMaximize
            | SolverOperation::SignedDirectedMinimize
            | SolverOperation::DirectedMinimize
            | SolverOperation::DirectedMaximize
            | SolverOperation::SignedPivotMinimize
    ) {
        let vec = |i: usize| -> Result<Vec3> {
            opt_vectors
                .and_then(|v| v.get(i))
                .copied()
                .ok_or(SolidError::invalid_input(OP_CURVE, "missing option vector"))
        };
        let normal = vec(0)?;
        let mut direction = vec(0)?;
        if matches!(
            op,
            SolverOperation::SignedDirectedMinimize
                | SolverOperation::DirectedMinimize
                | SolverOperation::DirectedMaximize
        ) {
            direction = vec(1)?;
        }

        if op == SolverOperation::SignedPivotMinimize {
            direction = vec(2)?;
            let sol_pt = other.evaluate_point(k, sol1)?;
            solution.start.solution_value =
                proj_signed_angle(pnt1, sol_pt, vec(1)?, normal, direction)?;
        } else {
            solution.start.solution_value =
                proj_dist(k, other, sol1, pnt1, normal, true, direction, op)?;
        }
    }

    let mut found = true;

    // Solution value in this case is not the distance.
    if op == SolverOperation::SurfaceProjectedIntersect {
        let mut obj_fcns = [0.0; 2];
        // (status ignored upstream; Not used by Evaluate; passed on as the fork,
        // tangency-labs/smlib 8311f8e)
        ls.evaluator().evaluate(&r.x, &mut obj_fcns, None)?;
        solution.start.solution_value = obj_fcns[0].abs() + obj_fcns[1].abs();

        // For a legitimate crossing, this should be extremely precise.
        // Check for spurious solutions caused by hitting cache boundaries.
        if solution.start.solution_value > 1000.0 * EFF_ZERO {
            if let Some(ivls) = ls.intervals() {
                let on = |i: usize, v: f64| {
                    ivls.get(i)
                        .is_some_and(|ivl| ivl.is_value_on_boundary(v, EFF_ZERO).0)
                };
                if on(0, sol0) {
                    found = false;
                }
                if on(1, sol1) {
                    found = false;
                }
            }

            if !found {
                return Ok((false, solution));
            }
        }
    }

    set_param(&mut solution.start, 0, sol0)?;
    set_param(&mut solution.start, 1, sol1)?;

    if !r.found {
        // If we terminate because of hitting bounds that
        // is ok in the closest point problem.  We have
        // returned the bound hit in dFoundT.
        // When normalizing must be within bounds.
        // Also for tangency. [Unit tests]
        if ls.termination_reason() != TerminationReason::OutOfBounds
            || matches!(
                op,
                SolverOperation::Normalize
                    | SolverOperation::AtDistance
                    | SolverOperation::ProjectedTangency
                    | SolverOperation::SurfaceProjectedIntersect
            )
        {
            return Ok((false, solution));
        }
    }

    Ok((found, solution))
}

/// The local solution of `op` between `this` over `interval` and `other` over
/// `other_interval` from the parameters `guesses` (`SmCurve::LocalCurveSolve`):
/// Newton's method on the curve/curve normal equations (`SmFindCCNormalENFO`). The
/// solution (filled in even when none is accepted) has both parameters, the objects
/// `objects` and the distance (the projected distance or signed pivot angle for the
/// projected, directed and pivot operations; `|F0| + |F1|` for
/// [`SolverOperation::SurfaceProjectedIntersect`], rejected when large on an
/// interval bound). A solution stopped out of bounds counts except when normalising,
/// at distance, for tangency and the surface projected intersection.
///
/// # Errors
///
/// `InvalidInput` for [`SolverOperation::Signed3dDirectedMinimize`] (see
/// [`Curve::local_curve_solve`]) or a missing option vector; those of the solver
/// and evaluations.
pub fn local_curve_solve<O: Copy, C1, C2>(
    k: &Kernel,
    this: &C1,
    interval: &Extent1d,
    other: &C2,
    other_interval: &Extent1d,
    op: SolverOperation,
    dist_tol_3d: f64,
    target_distance: Option<f64>,
    opt_vectors: Option<&[Vec3]>,
    guesses: [f64; 2],
    objects: [Option<O>; 2],
) -> Result<(bool, Solution<O>)>
where
    C1: CurveEvaluate + ?Sized,
    C2: CurveEvaluate + ?Sized,
{
    if op == SolverOperation::Signed3dDirectedMinimize {
        return Err(SolidError::invalid_input(
            OP_CURVE,
            "the 3D signed directed minimum needs Curve operands",
        ));
    }
    curve_solve_newton(
        k,
        this,
        interval,
        other,
        other_interval,
        op,
        dist_tol_3d,
        target_distance,
        opt_vectors,
        guesses,
        objects,
    )
}

impl Curve {
    /// [`local_curve_solve`] of this curve with `other`, including
    /// [`SolverOperation::Signed3dDirectedMinimize`]: the curves projected along the
    /// first option vector are intersected ([`local_curve_intersect`]) and the
    /// signed 3D distance between the intersection points is the solution value
    /// (signed by the first option vector); when they do not intersect, nearly
    /// coplanar curves are solved as [`SolverOperation::SignedDirectedMinimize`] and
    /// accepted when the gap is within 1 degree of the vector.
    ///
    /// # Errors
    ///
    /// As [`local_curve_solve`]; `InvalidInput` for the 3D signed operation without
    /// option vectors.
    pub fn local_curve_solve<O: Copy>(
        &self,
        k: &Kernel,
        interval: &Extent1d,
        other: &Self,
        other_interval: &Extent1d,
        op: SolverOperation,
        dist_tol_3d: f64,
        target_distance: Option<f64>,
        opt_vectors: Option<&[Vec3]>,
        guesses: [f64; 2],
        objects: [Option<O>; 2],
    ) -> Result<(bool, Solution<O>)> {
        if op != SolverOperation::Signed3dDirectedMinimize {
            return curve_solve_newton(
                k,
                self,
                interval,
                other,
                other_interval,
                op,
                dist_tol_3d,
                target_distance,
                opt_vectors,
                guesses,
                objects,
            );
        }

        // Do intersection and then compute 3D distance
        let mut solution = Solution::new();
        let vectors = opt_vectors.ok_or(SolidError::invalid_input(
            OP_CURVE,
            "missing option vectors",
        ))?;
        let vec0 = *vectors
            .first()
            .ok_or(SolidError::invalid_input(OP_CURVE, "missing option vector"))?;
        let origin = Vec3::new(0.0, 0.0, 0.0);
        let aux_data = Vec3::new(0.0, 0.0, 0.0);
        let proj1 = Self::from(ProjectedCurve::new(
            self.clone(),
            Some(origin),
            Some(vec0),
            Some(aux_data),
            ProjectionType::Parallel,
            OwnsCurve::No,
        )?);
        let proj2 = Self::from(ProjectedCurve::new(
            other.clone(),
            Some(origin),
            Some(vec0),
            Some(aux_data),
            ProjectionType::Parallel,
            OwnsCurve::No,
        )?);

        // find projected curve intersections
        let isect = local_curve_intersect(
            k,
            &proj1,
            interval,
            &proj2,
            other_interval,
            dist_tol_3d,
            guesses[0],
            guesses[1],
        )?;
        let Some(isect) = isect else {
            // Check for coplanarity case and use separate solver

            // curve values at guess parameters
            let pv1: [Vec3; 2] = eval_n(k, self, guesses[0], 1, true, true)?; // nonZeroTangent values
            let pv2: [Vec3; 2] = eval_n(k, other, guesses[1], 1, true, true)?; // nonZeroTangent values

            let norm = pv1[1].cross(vec0);
            if norm.length_squared() < EFF_ZERO_SQ {
                return Ok((false, solution));
            }
            let norm = norm.unitize()?;
            let d = -norm.dot(pv1[0]);
            let dist = norm.dot(pv2[0]) + d;
            let tol = EFF_ZERO_SQRT * (1.0 + pv2[0].max_dimension());
            if dist.abs() > tol {
                return Ok((false, solution));
            }
            if norm.dot(pv2[1]) > EFF_ZERO_SQRT {
                return Ok((false, solution));
            }
            // If make it here then we are likely to have a coplanar curves case
            // use a different solver to fix the problem.
            let vecs = [norm, vec0];
            let (found, sol) = self.local_curve_solve(
                k,
                interval,
                other,
                other_interval,
                SolverOperation::SignedDirectedMinimize,
                dist_tol_3d,
                target_distance,
                Some(&vecs),
                guesses,
                objects,
            )?;
            if !found {
                return Ok((false, solution));
            }

            // Make sure resulting points lie parallel to the vector
            let p1 = self.evaluate_point(k, sol.start.param(0)?)?;
            let p2 = other.evaluate_point(k, sol.start.param(1)?)?;
            let vec = p2 - p1;
            if vec.length_squared() < EFF_ZERO_SQ {
                return Ok((false, solution));
            }
            let mut angle = vec.angle_between(vec0)?;
            if angle > PI / 2.0 {
                angle = PI - angle;
            }
            if angle > 1.0 * PI / 180.0 {
                return Ok((false, solution));
            }

            solution = sol;
            solution.num_objects = 2;
            solution.objects[0] = objects[0];
            solution.objects[1] = objects[1];
            return Ok((true, solution));
        };

        let p1 = self.evaluate_point(k, isect.my_param)?;
        let p2 = other.evaluate_point(k, isect.other_param)?;
        let vec = p2 - p1;
        let mut dist = vec.length();
        if vec.dot(vec0) < 0.0 {
            dist = -dist;
        }
        solution.solution_type = SolutionType::SingleValue;
        solution.num_variables = 2;
        solution.start.solution_value = dist;
        set_param(&mut solution.start, 0, isect.my_param)?;
        set_param(&mut solution.start, 1, isect.other_param)?;
        solution.num_objects = 2;
        solution.objects[0] = objects[0];
        solution.objects[1] = objects[1];
        Ok((true, solution))
    }
}

// ---- LocalCurveIntersect ----------------------------------------------------------------

/// An intersection found by [`local_curve_intersect`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveIntersection {
    /// `rdMyParameter`.
    pub my_param: f64,
    /// `rdOtherParameter`.
    pub other_param: f64,
    /// `rdDeviation`: the distance between the two points.
    pub deviation: f64,
}

/// Distance between `this` at `t1` and `other` at `t2` (positions of nonZeroTangent
/// evaluations) and both evaluations.
fn deviation_at<C1, C2>(
    k: &Kernel,
    this: &C1,
    other: &C2,
    t1: f64,
    t2: f64,
) -> Result<(f64, [Vec3; 2], [Vec3; 2])>
where
    C1: CurveEvaluate + ?Sized,
    C2: CurveEvaluate + ?Sized,
{
    let pv1: [Vec3; 2] = eval_n(k, this, t1, 1, true, true)?; // nonZeroTangent values
    let pv2: [Vec3; 2] = eval_n(k, other, t2, 1, true, true)?; // nonZeroTangent values
    Ok((pv1[0].distance(pv2[0]), pv1, pv2))
}

/// The intersection of `this` over `interval` and `other` over `other_interval`
/// near the guesses (`SmCurve::LocalCurveIntersect`): Newton's method on the
/// curve/curve intersection equations (`SmCCIntersectENFO`, accuracy
/// `SM_EFF_ZERO / 10`, acceptable `2 dist_tol`); a result within `dist_tol` is
/// polished by a second solve from it, then by the closest approach of the curves
/// ([`local_curve_solve`], [`SolverOperation::Minimize`]) and, for nearly parallel
/// tangents, by the tangent intersection equations. `None` when no intersection is
/// found near the guesses.
///
/// # Errors
///
/// Those of the solvers and evaluations.
pub fn local_curve_intersect<C1, C2>(
    k: &Kernel,
    this: &C1,
    interval: &Extent1d,
    other: &C2,
    other_interval: &Extent1d,
    dist_tol: f64,
    my_guess: f64,
    other_guess: f64,
) -> Result<Option<CurveIntersection>>
where
    C1: CurveEvaluate + ?Sized,
    C2: CurveEvaluate + ?Sized,
{
    // locals
    let mut intervals = ExtentNd::new(2);
    for (i, ivl) in [*interval, *other_interval].into_iter().enumerate() {
        if let Some(slot) = intervals.get_mut(i) {
            *slot = ivl;
        }
    }
    // Do not utilize periodicities in local solver -- local solvers
    // should not jump across seams.
    let periodicities = [false, false];

    let guess_t = [my_guess, other_guess];

    // create a newton raphson solver object containing a curve/curve evaluator.
    let mut eval = CcIntersectEnfo::new(k, this, other, EFF_ZERO * EFF_ZERO / 10000.0);
    let mut ls = LocalSolveNd::new(&mut eval, Some(&intervals), Some(&periodicities));

    // Pick up close solutions and then test them
    ls.set_desired_accuracy(EFF_ZERO / 10.0);
    ls.set_boundary_handler(BoundaryHandler::HitsAllBoundaries, 3);

    // look for a newton raphson solution
    let r = ls.solve_it(&guess_t, dist_tol * 2.0)?;
    let mut sol = [
        r.x.first().copied().unwrap_or(0.0),
        r.x.get(1).copied().unwrap_or(0.0),
    ];

    // when a solution was found
    if !r.found {
        // arrive here when no answer is found
        return Ok(None);
    }

    // get solution deviation
    let (mut deviation, mut pv1, mut pv2) = deviation_at(k, this, other, sol[0], sol[1])?;

    // Be happy with a result within the really tight tolerance:
    if deviation < EFF_ZERO_SQ {
        return Ok(Some(CurveIntersection {
            my_param: sol[0],
            other_param: sol[1],
            deviation,
        }));
    }

    // when deviation is within the loose tolerance
    if deviation >= dist_tol {
        return Ok(None);
    }

    // If it's not real tight,
    // see if we can improve it by starting again at the solution.
    // [my_shell_demo iter: 2]
    if deviation > EFF_ZERO {
        // save current solutions
        let old_sol = sol;
        let old_guess = guess_t;

        // However, if the solution didn't move,
        // no sense trying again.
        if !are_same_to_tol(old_sol[0], old_guess[0], EFF_ZERO_SQRT)
            || !are_same_to_tol(old_sol[1], old_guess[1], EFF_ZERO_SQRT)
        {
            // create a second NewtonRaphson Solver
            let mut eval2 = CcIntersectEnfo::new(k, this, other, EFF_ZERO * EFF_ZERO / 10000.0);
            let mut ls2 = LocalSolveNd::new(&mut eval2, Some(&intervals), Some(&periodicities));

            // Pick up close solutions and then test them
            ls2.set_desired_accuracy(EFF_ZERO / 10.0);
            ls2.set_boundary_handler(BoundaryHandler::HitsAllBoundaries, 3);

            // use current solutions as next guesses (the old guess is restored
            // afterwards upstream)
            let guess2 = sol;

            // solve starting at previous solution
            let r2 = ls2.solve_it(&guess2, dist_tol * 2.0)?;
            sol = [
                r2.x.first().copied().unwrap_or(0.0),
                r2.x.get(1).copied().unwrap_or(0.0),
            ];

            // when a better solution was found - store it
            if r2.found {
                // get new deviation
                let (new_deviation, npv1, npv2) = deviation_at(k, this, other, sol[0], sol[1])?;
                pv1 = npv1;
                pv2 = npv2;

                // when new deviation is better than last, store it
                if new_deviation < deviation {
                    deviation = new_deviation;
                } else {
                    // restore old solution
                    sol = old_sol;
                }
            } else {
                // didn't find a solution, restore old one
                sol = old_sol;
            }
        } // end first solution uv not the same as guess uv branch
    } // end deviation larger than zero check

    // If we did not converge to an exact intersection (with nonZero length intervals)
    //  then try finding minimum distance between spans using intersection as start point.
    if deviation > EFF_ZERO
        && !are_same(interval.min(), interval.max())
        && !are_same(other_interval.min(), other_interval.max())
    {
        // Find minimum distance between spans as best possible answer
        let (found, s) = local_curve_solve::<(), C1, C2>(
            k,
            this,
            interval,
            other,
            other_interval,
            SolverOperation::Minimize,
            dist_tol,
            None,
            None,
            sol,
            [None, None],
        )?;

        // when a better solution was found
        if found && s.start.solution_value < deviation {
            deviation = s.start.solution_value;
            sol = [s.start.param(0)?, s.start.param(1)?];
        } // end found a better solution check
    } // end did not converge to an exact solution check

    // If the curves are nearly parallel,
    // an SmCCTangentIntENFO can often do better.
    if deviation > EFF_ZERO && pv1[1].is_parallel_to(pv2[1], 2.0) {
        let mut eval2 = CcTangentIntEnfo::new(k, this, other, EFF_ZERO * EFF_ZERO);
        let periodicities2 = [
            is_periodic(k, this, interval)?,
            is_periodic(k, other, other_interval)?,
        ];

        let guess_t2 = sol;

        let mut ls2 = LocalSolveNd::new(&mut eval2, Some(&intervals), Some(&periodicities2));

        // Pick up close solutions and then test them
        ls2.set_desired_accuracy(EFF_ZERO_SQ); // very tight
                                               // See if answer is better than one we found
        let r2 = ls2.solve_it(&guess_t2, dist_tol * 2.0)?;
        let sol2 = [
            r2.x.first().copied().unwrap_or(0.0),
            r2.x.get(1).copied().unwrap_or(0.0),
        ];
        let (new_deviation, _, _) = deviation_at(k, this, other, sol2[0], sol2[1])?;
        if new_deviation < deviation {
            return Ok(Some(CurveIntersection {
                my_param: sol2[0],
                other_param: sol2[1],
                deviation: new_deviation,
            }));
        }
    } // end tangent processor check

    // store the output and quit
    Ok(Some(CurveIntersection {
        my_param: sol[0],
        other_param: sol[1],
        deviation,
    }))
}

// ---- SmCurvePropertyGS::LocalSolve ------------------------------------------------------

/// A note [`CurvePropertyGs::local_solve_branch_with_note`] returns with its result. It is
/// not a failure of the local solve: the caller decides whether to stop on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CurvePropertyNote {
    /// For an angle property, a corner of the span's pseudo box projects onto the view
    /// origin (within `SM_EFF_ZERO`), where it has no angle: the local solve ends there.
    /// Upstream returns `TRUE` (status `1`) here.
    CornerOnViewAxis,
}

impl CurvePropertyNote {
    /// The error of a caller that stops on this note, as upstream's callers stop on the
    /// status `1` (`SER`): a failure of the local solve whose comparison status
    /// ([`SolidError::sm_status`]) is `1`.
    #[must_use]
    pub fn into_error(self) -> SolidError {
        match self {
            Self::CornerOnViewAxis => Failure::new(OP_GS, CurveFailure::CornerOnViewAxis)
                .with_status(1)
                .into_error(),
        }
    }
}

impl<'t, O: SolutionObject + 't> CurvePropertyGs<'t, O> {
    /// `SmCurvePropertyGS::LocalSolve` on the curve leaf `branch[0]` (always returns
    /// `rbNeedsMoreSubdivision` false): for the angle properties the span is skipped
    /// when no pseudo box corner beats the best answer; the span interval is limited
    /// to the solver's; for the angle and plane intersection properties a span end
    /// at a less than G1 continuity is a candidate solution (a plane intersection
    /// span lying in the plane is a range solution); then `LocalPropertyAnalysis`
    /// from the span middle (and its ends and quarters for the other properties) adds
    /// the solutions found, a span with three exact guesses becoming a range.
    ///
    /// This is the solver's step ([`GlobalSolve::local_solve`]), which stops on the
    /// note of [`CurvePropertyGs::local_solve_branch_with_note`] as upstream's
    /// `SmGlobalSolver::SolveIt` stops on the status `1` (`SER`): the note's error
    /// ([`CurvePropertyNote::into_error`]).
    ///
    /// # Errors
    ///
    /// Those of [`CurvePropertyGs::local_solve_branch_with_note`], and its note's error.
    pub fn local_solve_branch(&mut self, k: &Kernel, branch: &Branch) -> Result<bool> {
        match self.local_solve_branch_with_note(k, branch)? {
            (_, Some(note)) => Err(note.into_error()),
            (needs_more_subdivision, None) => Ok(needs_more_subdivision),
        }
    }

    /// [`CurvePropertyGs::local_solve_branch`] with its note on the success path: for
    /// the angle properties, a pseudo box corner that projects onto the view origin ends
    /// the local solve at once with [`CurvePropertyNote::CornerOnViewAxis`] (upstream
    /// `return TRUE` from the `SmStatus` function), with no more subdivision.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a node without data (status `SM_ERR_NULL_POINTER`) or a
    /// missing branch node, curve, option vector or value or a node that is not a curve
    /// leaf; those of the evaluations, `LocalPropertyAnalysis` and
    /// `sm_FindAdditionalCpGuess`.
    pub fn local_solve_branch_with_note(
        &mut self,
        k: &Kernel,
        branch: &Branch,
    ) -> Result<(bool, Option<CurvePropertyNote>)> {
        // init output
        let needs_more_subdivision = false;

        // locals
        let curve_node_id = branch[0].ok_or(SolidError::invalid_input(OP_GS, "null curve node"))?;
        let tree = self.base.tree(0)?;
        let curve_node = tree.node(curve_node_id)?;
        if matches!(curve_node.data, NodeData::None) {
            return Err(SolidError::curve(OP_GS, CurveFailure::CurveNodeWithoutData));
        }
        let bez_span = *tree.curve_span(curve_node)?;
        let owner = tree.owner_object()?;

        let curve = self
            .curve
            .ok_or(SolidError::invalid_input(OP_GS, "solver without curve"))?;
        let mut ivl = *bez_span.interval();
        let prop = self.property;

        if matches!(
            prop,
            CurvePropertyType::MinimizeAngleToPlane
                | CurvePropertyType::MinimizeDirectedAngle
                | CurvePropertyType::MinimizeCcwAngle
        ) {
            // Check the minimum angle made by the pseudo box to see if
            // we can eliminate having to process this node.
            let corners = bez_span.pseudo_box().corners();
            let (o0, o1, o2) = (self.opt(0)?, self.opt(1)?, self.opt(2)?);
            let mut min_cos_theta = BIG_DOUBLE;
            for c in corners {
                let mut vec = c - o0;
                vec = o1.cross(vec).cross(o1); // Project to view plane
                if vec.length_squared() < EFF_ZERO {
                    // (`return TRUE` from an SmStatus function)
                    return Ok((
                        needs_more_subdivision,
                        Some(CurvePropertyNote::CornerOnViewAxis),
                    ));
                }
                let vec = vec.unitize()?;
                let cos_theta = property_angle(prop, o1, o2, vec)?;

                if cos_theta < min_cos_theta {
                    min_cos_theta = cos_theta;
                }
            }

            // If we have no angles which are small enough than just return
            if (min_cos_theta * min_cos_theta.abs()) + EFF_ZERO_SQRT
                > self.base.best_answer_so_far_sq
            {
                return Ok((needs_more_subdivision, None));
            }
        } // end angle cases

        // Do interval tests and reject if disjoint or adjust
        // if sIvl is not completely inside of m_crInterval
        let mut conts = bez_span.aux.conts;
        if ivl.are_disjoint(&self.interval, 0.0) {
            return Ok((needs_more_subdivision, None));
        }

        if ivl.min() <= self.interval.min() {
            conts[0] = ContinuityType::Discontinuous;
        }

        if ivl.max() >= self.interval.max() {
            conts[1] = ContinuityType::Discontinuous;
        }

        if !ivl.is_contained_by(&self.interval, EFF_ZERO) {
            ivl = ivl.intersect(&self.interval)?;
            // Check for zero length intervals
            if are_same(ivl.max(), ivl.min()) {
                return Ok((needs_more_subdivision, None));
            }
        }

        // locals
        let mut sol: Solution<O> = Solution::new();

        // Process discontinuity points when appropriate
        if matches!(
            prop,
            CurvePropertyType::MinimizeAngleToPlane
                | CurvePropertyType::MinimizeDirectedAngle
                | CurvePropertyType::MinimizeCcwAngle
                | CurvePropertyType::PlaneIntersection
        ) {
            for (jj, cont) in conts.iter().enumerate() {
                let mut param = if jj == 0 { ivl.min() } else { ivl.max() };
                if (*cont as i32) < (ContinuityType::G1 as i32) {
                    let pnt = curve.evaluate_point(k, param)?;

                    // for minimize angle cases
                    if prop != CurvePropertyType::PlaneIntersection {
                        let (o0, o1, o2) = (self.opt(0)?, self.opt(1)?, self.opt(2)?);
                        let mut vec = pnt - o0;
                        vec = o1.cross(vec).cross(o1);

                        // If the point is at the origin of rotation just ignore it
                        if vec.length_squared() > EFF_ZERO_SQ {
                            let vec = vec.unitize()?;
                            let cos_theta = property_angle(prop, o1, o2, vec)?;

                            // save solution
                            sol.solution_type = SolutionType::SingleValue;
                            sol.nodes[0] = branch[0]; // Point node
                            sol.nodes[1] = branch[1]; // Curve node
                            sol.num_objects = 1;
                            sol.objects[0] = owner;
                            sol.num_variables = 1;
                            set_param(&mut sol.start, 0, param)?;
                            sol.start.solution_value = cos_theta;
                            self.add_sorted_solution(&sol, SortKey::ByValueWithParameter, true)?;
                        }
                    } // end minimize angle cases

                    if prop == CurvePropertyType::PlaneIntersection {
                        let abc = self.opt(0)?;
                        let value = self.value()?;
                        let val = pnt.dot(abc) + value;

                        // points within tolerance
                        if val.abs() < self.base.tolerance_3d {
                            sol.solution_type = SolutionType::SingleValue;
                            sol.nodes[0] = branch[0]; // Point node
                            sol.nodes[1] = branch[1]; // Curve node
                            sol.num_objects = 1;
                            sol.objects[0] = owner;
                            sol.num_variables = 1;
                            set_param(&mut sol.start, 0, param)?;
                            sol.start.solution_value = val.abs();

                            if jj == 0 {
                                param = ivl.max();
                                let pnt2 = curve.evaluate_point(k, param)?;
                                let val2 = pnt2.dot(abc) + value;
                                let pnt3 = curve.evaluate_point(k, ivl.evaluate(0.4567))?;
                                let val3 = pnt3.dot(abc) + value;
                                if val2.abs() < self.base.tolerance_3d
                                    && val3.abs() < self.base.tolerance_3d
                                {
                                    sol.solution_type = SolutionType::RangeOfValues;
                                    set_param(&mut sol.end, 0, param)?;
                                    sol.end.solution_value = val.abs();
                                    self.add_sorted_solution(
                                        &sol,
                                        SortKey::ByFirstParameter,
                                        true,
                                    )?;
                                    return Ok((needs_more_subdivision, None));
                                }
                            } // end max interval value value check

                            // inform the public
                            self.add_sorted_solution(&sol, SortKey::ByFirstParameter, true)?;
                        } // end points within tolerance check
                    } // end plane intersection branch
                } // end aeConts[jj] < SM_CT_G1 check
            } // end iter over interval min and max
        } // end discontinuity cases

        let mut guess_t: Vec<f64> = if matches!(
            prop,
            CurvePropertyType::MinimizeAngleToPlane
                | CurvePropertyType::MinimizeDirectedAngle
                | CurvePropertyType::MinimizeCcwAngle
                | CurvePropertyType::ParallelToVector
        ) {
            // There are possible optimizations here that we could use to
            // check the angle of the curve relative to the point.
            vec![ivl.evaluate(0.5)]
        } else {
            // Just take 5 guesses
            vec![
                ivl.evaluate(0.5),
                ivl.evaluate(0.0),
                ivl.evaluate(1.0),
                ivl.evaluate(0.25),
                ivl.evaluate(0.75),
            ]
        };

        let num_original_guess = guess_t.len();

        // If we have three zeroes in this little span, assume it's a zero function
        // and create a Range solution.  [B500. before other fix]
        let mut num_serendipities: u64 = 0;

        // each guess
        let mut ii = 0;
        while ii < guess_t.len() {
            let guess = *guess_t
                .get(ii)
                .ok_or(SolidError::invalid_input(OP_GS, "guess index"))?;
            let this_ii = ii;
            ii += 1;
            let mut found = match local_property_analysis(
                k,
                curve,
                &ivl,
                prop,
                guess,
                self.property_value,
                &self.opt_vectors,
                &mut sol,
            ) {
                Ok(f) => f,
                Err(e) if e.is_unported_or_cancelled() => return Err(e),
                // Had an error just try a different starting point - perhaps some
                // higher derivative was zero.
                Err(_) => continue,
            };

            if found && prop == CurvePropertyType::ParallelToVector {
                // nonZeroTangent values
                let pv: [Vec3; 2] = eval_n(k, curve, sol.start.param(0)?, 1, true, true)?;
                let mut angle = self.opt(0)?.angle_between(pv[1])?;
                if angle > PI / 2.0 {
                    angle = PI - angle;
                }
                if angle > self.base.tolerance_3d {
                    found = false;
                }
            }

            // If we did not find a solution the first time then stop
            if this_ii == 0 && !found {
                return Ok((needs_more_subdivision, None));
            }
            if !found {
                continue; // If fail don't save answer
            }

            // Have a solution.
            // Was the guess point a solution?
            if are_same_to_tol(guess, sol.start.param(0)?, EFF_ZERO) {
                num_serendipities += 1;
            }

            if prop == CurvePropertyType::PlaneIntersection {
                // This returns signed solution values.  AddSortedSolution() requires
                // absolute values.
                sol.start.solution_value = sol.start.solution_value.abs();
                if sol.solution_type == SolutionType::RangeOfValues {
                    sol.end.solution_value = sol.end.solution_value.abs();
                }

                // Also, look for a nearby guess: if nearly tangent,
                // make a circle at the solution and intersect with plane.
                if this_ii < num_original_guess {
                    if let Some(new_guess) =
                        find_additional_cp_guess(k, curve, &ivl, self.opt(0)?, sol.start.param(0)?)?
                    {
                        guess_t.push(new_guess);
                    }
                }
            }

            sol.nodes[0] = branch[0]; // Curve node
            sol.num_variables = 1;
            sol.num_objects = 1;
            sol.objects[0] = owner;

            // Check for a Range solution.
            if num_serendipities >= 3 {
                // Make a Range solution and we're done.
                sol.solution_type = SolutionType::RangeOfValues;
                set_param(&mut sol.start, 0, ivl.min())?;
                set_param(&mut sol.end, 0, ivl.max())?;
                sol.end.solution_value = sol.start.solution_value;
                self.add_sorted_solution(&sol, SortKey::ByFirstParameter, true)?;
                return Ok((needs_more_subdivision, None));
            }

            if matches!(
                prop,
                CurvePropertyType::MinimizeFirstDerivative
                    | CurvePropertyType::MaximizeFirstDerivative
                    | CurvePropertyType::MinimizeRadiusOfCurvature
                    | CurvePropertyType::MaximizeRadiusOfCurvature
                    | CurvePropertyType::MinimizeCcwAngle
                    | CurvePropertyType::MinimizeAngleToPlane
                    | CurvePropertyType::MinimizeDirectedAngle
                    | CurvePropertyType::ProjectedPointMaximize
                    | CurvePropertyType::ProjectedPointMinimize
            ) {
                self.add_sorted_solution(&sol, SortKey::ByValueWithParameter, true)?;
            } else {
                self.add_sorted_solution(&sol, SortKey::ByFirstParameter, true)?;
            }
        } // end iter every guess point

        // all done
        Ok((needs_more_subdivision, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::hermite_curve::HermiteCurve;
    use crate::curve::line::{BoundedLineOptions, Line};
    use crate::error::{SM_ERR, SM_ERR_NULL_POINTER};
    use crate::math::det;

    fn hermite(p1: [f64; 3], d1: [f64; 3], p2: [f64; 3], d2: [f64; 3]) -> Curve {
        Curve::from(HermiteCurve::new(
            Vec3::from_array(p1),
            Vec3::from_array(d1),
            Vec3::from_array(p2),
            Vec3::from_array(d2),
            3,
        ))
    }

    fn line(p: [f64; 3], v: [f64; 3], a: f64, b: f64) -> Curve {
        Curve::from(Line::bounded(
            Vec3::from_array(p),
            Vec3::from_array(v),
            &Extent1d::from_min_max(a, b),
            &BoundedLineOptions::default(),
        ))
    }

    /// The curve property solver's status 1 (upstream `return TRUE`: a pseudo box corner
    /// on the view axis of an angle property) comes back as
    /// [`CurvePropertyNote::CornerOnViewAxis`], and the global solve stops on it with the
    /// status 1 it returned before.
    #[test]
    fn issue_15020_the_corner_note_stops_the_global_solve_with_status_1() {
        use crate::curve::curve::p7::PropertyAnalysisOptions;
        use crate::curve::curve::CurvePropertyType;
        use crate::geom::solution_array::OperandObject;
        let k = Kernel::default();
        // a line along the view direction: every pseudo box corner is on the view axis
        let c = line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], 0.0, 1.0);
        let vecs = [Vec3::ZERO, Vec3::X, Vec3::Z];
        let opts: PropertyAnalysisOptions<'_, OperandObject> = PropertyAnalysisOptions {
            opt_vectors: &vecs,
            ..PropertyAnalysisOptions::default()
        };
        let e = c
            .global_property_analysis(
                &k,
                &Extent1d::from_min_max(0.0, 1.0),
                CurvePropertyType::MinimizeDirectedAngle,
                1e-6,
                &opts,
            )
            .unwrap_err();
        assert_eq!(e.sm_status(), Some(1));
        assert_eq!(e.upstream_status(), Some(1));
        assert_eq!(e.operation(), OP_GS);
        assert_eq!(e.kind(), crate::error::ErrorKind::Failed);
        assert!(e.curve_failure().is_some());
        assert_eq!(
            CurvePropertyNote::CornerOnViewAxis.into_error(),
            e,
            "the solve stops with the note's error"
        );
    }

    #[test]
    fn issue_1776_concave_loop_normal_follows_orientation() {
        let points = [
            [-0.5, -3.5, 0.0],
            [0.5, -3.5, 0.0],
            [0.5, -0.5, 0.0],
            [3.5, -0.5, 0.0],
            [3.5, 0.5, 0.0],
            [0.5, 0.5, 0.0],
            [0.5, 3.5, 0.0],
            [-0.5, 3.5, 0.0],
            [-0.5, 0.5, 0.0],
            [-3.5, 0.5, 0.0],
            [-3.5, -0.5, 0.0],
            [-0.5, -0.5, 0.0],
        ];
        let curves: Vec<_> = points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .map(|(&a, &b)| {
                Curve::from(
                    Line::create_line_segment(3, Vec3::from_array(a), Vec3::from_array(b), None)
                        .unwrap(),
                )
            })
            .collect();
        // A counter-clockwise plus-shaped loop (four reflex corners) about +Z: its
        // area vector is +Z, where upstream's weighted turns give -Z.
        let k = Kernel::default();
        assert_eq!(compute_crvs_normal(&k, &curves).unwrap(), Vec3::Z);

        let reversed: Vec<_> = points
            .iter()
            .rev()
            .zip(points.iter().rev().cycle().skip(1))
            .map(|(&a, &b)| {
                Curve::from(
                    Line::create_line_segment(3, Vec3::from_array(a), Vec3::from_array(b), None)
                        .unwrap(),
                )
            })
            .collect();
        assert_eq!(compute_crvs_normal(&k, &reversed).unwrap(), -Vec3::Z);
    }

    #[test]
    fn normal_of_a_planar_loop() {
        let k = Kernel::default();
        // a counter-clockwise loop in the XY plane made of curved Hermite pieces
        let curves = vec![
            hermite(
                [0.0, 0.0, 0.0],
                [1.0, -0.5, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 0.5, 0.0],
            ),
            hermite(
                [1.0, 0.0, 0.0],
                [0.5, 1.0, 0.0],
                [1.0, 1.0, 0.0],
                [-0.5, 1.0, 0.0],
            ),
            hermite(
                [1.0, 1.0, 0.0],
                [-1.0, 0.5, 0.0],
                [0.0, 1.0, 0.0],
                [-1.0, -0.5, 0.0],
            ),
            hermite(
                [0.0, 1.0, 0.0],
                [-0.5, -1.0, 0.0],
                [0.0, 0.0, 0.0],
                [0.5, -1.0, 0.0],
            ),
        ];
        let n = compute_crvs_normal(&k, &curves).unwrap();
        assert!((n.z - 1.0).abs() < 1e-12, "{n:?}");
    }

    #[test]
    fn area_normal_closes_an_open_chain_and_uses_its_plane() {
        let k = Kernel::default();
        // Two chords enclose a triangle in the YZ plane; the closing chord
        // goes back to the first sample without contributing another fan triangle.
        let curves = [
            line([7.0, 10.0, 20.0], [0.0, 1.0, 0.0], 0.0, 2.0),
            line([7.0, 12.0, 21.0], [0.0, 0.0, 1.0], -1.0, 3.0),
        ];
        assert_eq!(compute_crvs_normal(&k, &curves).unwrap(), Vec3::X);
    }

    #[test]
    fn area_normal_is_invariant_under_start_point_and_chord_subdivision() {
        let k = Kernel::default();
        let points = [
            Vec3::new(1e9, -5.0, -35.0),
            Vec3::new(1e9, 5.0, -35.0),
            Vec3::new(1e9, 5.0, -5.0),
            Vec3::new(1e9, 35.0, -5.0),
            Vec3::new(1e9, 35.0, 5.0),
            Vec3::new(1e9, 5.0, 5.0),
            Vec3::new(1e9, 5.0, 35.0),
            Vec3::new(1e9, -5.0, 35.0),
            Vec3::new(1e9, -5.0, 5.0),
            Vec3::new(1e9, -35.0, 5.0),
            Vec3::new(1e9, -35.0, -5.0),
            Vec3::new(1e9, -5.0, -5.0),
        ];
        for start in 0..points.len() {
            let ordered: Vec<_> = points
                .iter()
                .cycle()
                .skip(start)
                .take(points.len())
                .collect();
            let curves: Vec<_> = ordered
                .iter()
                .zip(ordered.iter().cycle().skip(1))
                .flat_map(|(&&a, &&b)| {
                    let mid = (a + b) * 0.5;
                    [(a, mid), (mid, b)].map(|(p, q)| {
                        Curve::from(Line::create_line_segment(3, p, q, None).unwrap())
                    })
                })
                .collect();
            assert_eq!(compute_crvs_normal(&k, &curves).unwrap(), Vec3::X);
        }
    }

    /// The fork's be45e8d (unchanged to eaa998a) unitizes with `SER`: a zero area (no
    /// curves, or collinear ones) is `SM_ERR_INVALID_INPUT` (upstream makes it
    /// `(0, 0, 1)`); a single line is `SM_ERR`.
    #[test]
    fn area_normal_reports_a_zero_area_and_keeps_the_single_line_error() {
        let k = Kernel::default();
        let a = line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], 0.0, 1.0);
        let b = line([1.0, 0.0, 0.0], [1.0, 0.0, 0.0], 0.0, 1.0);
        let collinear = [a.clone(), b];
        let zero_area = |r: Result<Vec3>| matches!(r, Err(SolidError::InvalidInput { .. }));
        assert!(zero_area(compute_crvs_normal(&k, &[])));
        assert!(zero_area(compute_crvs_normal(&k, &collinear)));
        let single = compute_crvs_normal(&k, &[a]).unwrap_err();
        assert_eq!(
            single.curve_failure(),
            Some(&CurveFailure::SingleLinearCurve)
        );
        assert_eq!(single.upstream_status(), Some(SM_ERR));
        assert!(single.is_status_answer());
    }

    /// Straight B-spline segments through `points` in turn (two control points each, so
    /// their samples are the points themselves).
    fn polyline(points: &[[f64; 3]]) -> Vec<Curve> {
        points
            .windows(2)
            .map(|w| {
                Curve::from(
                    crate::nurbs::b_spline_curve::p1::BSplineCurve::create_line_segment(
                        3,
                        Vec3::from_array(w[0]),
                        Vec3::from_array(w[1]),
                    )
                    .unwrap(),
                )
            })
            .collect()
    }

    /// An open planar chain whose chord-closed area cancels exactly still spans its
    /// plane (#21291 review): the answer is that plane's normal in the sense of the
    /// chain's turns (here -2 about Z, so -Z, upstream's weighted-turns answer), not a
    /// refusal. The same chain in the XZ plane answers its plane's normal, +Y (its turns
    /// are +2 about Y).
    #[test]
    fn open_chain_of_cancelling_area_answers_its_plane() {
        let k = Kernel::default();
        let z = polyline(&[
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [2.0, -2.0, 0.0],
            [2.0, 0.0, 0.0],
        ]);
        assert_eq!(compute_crvs_normal(&k, &z).unwrap(), -Vec3::Z);
        let xz = polyline(&[
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
            [2.0, 0.0, -2.0],
            [2.0, 0.0, 0.0],
        ]);
        assert_eq!(compute_crvs_normal(&k, &xz).unwrap(), Vec3::Y);
        // reversed, the chain turns the other way: +Z
        let back = polyline(&[
            [2.0, 0.0, 0.0],
            [2.0, -2.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 0.0, 0.0],
        ]);
        assert_eq!(compute_crvs_normal(&k, &back).unwrap(), Vec3::Z);
        // a symmetric S (turns +1 and -1, area 0): the sense of its first turn, +Z
        let s = polyline(&[
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [2.0, 1.0, 0.0],
        ]);
        assert_eq!(compute_crvs_normal(&k, &s).unwrap(), Vec3::Z);
    }

    /// The chains above in a tilted plane, offset and scaled (#21291 review): rounding
    /// leaves the chord-closed area of the Z path above `SM_EFF_ZERO` at large scales, but
    /// it is not certainly a direction, so the answer is still the plane's normal (here
    /// in the sense of the turns, as at the origin). The S's rounded points enclose an
    /// area of rounding size whose side decides its sense.
    #[test]
    fn tilted_chains_of_cancelling_area_answer_their_plane_at_every_scale() {
        let k = Kernel::default();
        let u = Vec3::new(det::cos(0.3_f64), det::sin(0.3_f64), 0.0);
        let v = Vec3::new(
            -det::sin(0.3_f64) * det::cos(0.7_f64),
            det::cos(0.3_f64) * det::cos(0.7_f64),
            det::sin(0.7_f64),
        );
        let plane = u.cross(v).unitize().unwrap();
        for scale in [1.0, 10.0, 100.0, 1e3, 1e4] {
            let off = Vec3::new(123.456, 7.89, -3.21) * scale;
            let map = |pts: &[[f64; 2]]| -> Vec<[f64; 3]> {
                pts.iter()
                    .map(|p| (off + (u * p[0] + v * p[1]) * scale).to_array())
                    .collect()
            };
            let z = polyline(&map(&[[0.0, 0.0], [1.0, 1.0], [2.0, -2.0], [2.0, 0.0]]));
            let n = compute_crvs_normal(&k, &z).unwrap();
            assert!((n + plane).length() < 1e-9, "Z path at {scale}: {n:?}");
            // the S's rounded points enclose an area of rounding size, on one side or
            // the other: the answer is the plane's normal on that side
            let s = polyline(&map(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [2.0, 1.0]]));
            let n = compute_crvs_normal(&k, &s).unwrap();
            assert!(
                (n - plane).length() < 1e-9 || (n + plane).length() < 1e-9,
                "S at {scale}: {n:?}"
            );
        }
    }

    /// A chain that does not span a plane, or a closed loop that encloses no area (no
    /// winding, so no sense), still has no normal: `SM_ERR_INVALID_INPUT` with
    /// `(0, 0, 1)` written.
    #[test]
    fn collinear_chain_and_closed_loop_of_no_area_still_refuse() {
        let k = Kernel::default();
        let refused = |curves: &[Curve]| {
            let (n, e) = compute_crvs_normal_written(&k, curves).unwrap();
            n == Vec3::Z && matches!(e, Some(SolidError::InvalidInput { .. }))
        };
        // collinear, with a turn back over itself
        assert!(refused(&polyline(&[
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [3.0, 0.0, 0.0],
            [2.0, 0.0, 0.0]
        ])));
        // collinear and open, out of the XY plane, turning back over itself
        assert!(refused(&polyline(&[
            [0.0, 1.0, 2.0],
            [1.0, 3.0, 5.0],
            [3.0, 7.0, 11.0],
            [2.0, 5.0, 8.0],
        ])));
        // a closed bowtie: its two lobes cancel
        assert!(refused(&polyline(&[
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0],
        ])));
        // a chain spanning a plane below the area bound SM_EFF_ZERO
        assert!(refused(&polyline(&[
            [0.0, 0.0, 0.0],
            [1e-7, 1e-7, 0.0],
            [2e-7, -2e-7, 0.0],
            [2e-7, 0.0, 0.0]
        ])));
        // the collinear chain and the bowtie tilted, offset and scaled: rounding leaves
        // an area above SM_EFF_ZERO, but within its own rounding of zero (#21291 review)
        let u = Vec3::new(0.3, 0.5, 0.7);
        let v = Vec3::new(-0.5, 0.3, 0.1);
        for scale in [1.0, 100.0, 1e3, 1e4] {
            let off = Vec3::new(123.456, 7.89, -3.21) * scale;
            let map = |pts: &[[f64; 2]]| -> Vec<[f64; 3]> {
                pts.iter()
                    .map(|p| (off + (u * p[0] + v * p[1]) * scale).to_array())
                    .collect()
            };
            let collinear = map(&[[0.0, 0.0], [1.0, 0.0], [3.0, 0.0], [2.0, 0.0]]);
            assert!(refused(&polyline(&collinear)), "collinear at {scale}");
            let bowtie = map(&[[0.0, 0.0], [1.0, 1.0], [1.0, 0.0], [0.0, 1.0], [0.0, 0.0]]);
            assert!(refused(&polyline(&bowtie)), "bowtie at {scale}");
        }
    }

    /// An open planar chain whose area is nonzero, but too small for its direction to be
    /// certain to the angular zero, answers its plane's normal on the side of its area,
    /// not of its turns (#21291 review): the Z path with its end raised by `eps` has area
    /// `2 eps` along +Z and turns summing to `eps - 2`.
    #[test]
    fn open_chain_of_small_certain_area_keeps_the_areas_side() {
        let k = Kernel::default();
        for eps in [1e-12, 1e-10, 6e-9, 1e-7] {
            let z = polyline(&[
                [0.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [2.0, -2.0, 0.0],
                [2.0, eps, 0.0],
            ]);
            assert_eq!(compute_crvs_normal(&k, &z).unwrap(), Vec3::Z, "eps {eps}");
        }
    }

    /// A sample that is not finite has no normal: `SM_ERR_INVALID_INPUT` with `(0, 0, 1)`
    /// written, not a NaN normal (#21291 review).
    #[test]
    fn loop_normal_of_a_point_that_is_not_finite_refuses() {
        let k = Kernel::default();
        let seg = |a: [f64; 3], b: [f64; 3]| {
            let d = Vec3::from_array(b) - Vec3::from_array(a);
            line(a, d.to_array(), 0.0, d.length())
        };
        for bad in [f64::NAN, f64::INFINITY] {
            let curves = [
                seg([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                seg([1.0, 0.0, 0.0], [bad, 1.0, 0.0]),
                seg([bad, 1.0, 0.0], [0.0, 0.0, 0.0]),
            ];
            let (n, e) = compute_crvs_normal_written(&k, &curves).unwrap();
            assert_eq!(n, Vec3::Z);
            assert!(matches!(e, Some(SolidError::InvalidInput { .. })), "{e:?}");
        }
    }

    /// A certain area decides even when its fan vectors are nearly parallel (#21291
    /// review): the chain far along X encloses -1.000002e-3 about Z, certain in sense under the
    /// componentwise rounding bound (2e-8) though not under `(N + 8) u S` (1e-2); its
    /// turns point the other way. A long thin closed rectangle (1000 by 1e-5 in 40000
    /// segments) has a certain area 0.02 the same way and answers +Z, as main did.
    #[test]
    fn nearly_parallel_fans_keep_their_certain_area() {
        let k = Kernel::default();
        let far = polyline(&[
            [0.0, 0.0, 0.0],
            [1e6, 0.0, 0.0],
            [1e6 + 1.0, 1.0, 0.0],
            [1e6 + 2.0, -2.0, 0.0],
            [1e6 + 2.0, -1e-9, 0.0],
        ]);
        assert_eq!(compute_crvs_normal(&k, &far).unwrap(), -Vec3::Z);
        let mut pts = Vec::new();
        for i in 0..=20000 {
            pts.push([f64::from(i) * 0.05, 0.0, 0.0]);
        }
        for i in (0..20000).rev() {
            pts.push([f64::from(i) * 0.05, 1e-5, 0.0]);
        }
        pts.push([0.0, 0.0, 0.0]);
        assert_eq!(compute_crvs_normal(&k, &polyline(&pts)).unwrap(), Vec3::Z);
    }

    /// A Z path flattened to within the kernel's angular zero of a line (turns of
    /// 4e-7) has no plane, as a collinear chain has none: `SM_ERR_INVALID_INPUT`. Less
    /// flat (1e-5) it answers the plane in the sense of its turns, -Z.
    #[test]
    fn flat_z_path_within_the_angular_zero_is_collinear() {
        let k = Kernel::default();
        let z = |h: f64| {
            polyline(&[
                [0.0, 0.0, 0.0],
                [1.0, h, 0.0],
                [2.0, -2.0 * h, 0.0],
                [2.0, 0.0, 0.0],
            ])
        };
        let (n, e) = compute_crvs_normal_written(&k, &z(4e-7)).unwrap();
        assert_eq!(n, Vec3::Z);
        assert!(matches!(e, Some(SolidError::InvalidInput { .. })), "{e:?}");
        assert_eq!(compute_crvs_normal(&k, &z(1e-5)).unwrap(), -Vec3::Z);
        // a chain whose widest triangle is flat to 1.5e-6 but which kinks by 1e-3 near
        // its start spans a plane, so it is answered: its area cancels and its turn sum
        // (about 1e-6 of a weight 0.75) is below the angular zero, so its first turn
        // (-1e-3 rad, at the kink) decides, -Z
        let kinked = polyline(&[
            [0.0, 0.0, 0.0],
            [1e-3, 1e-6, 0.0],
            [1.0, -1.5e-6, 0.0],
            [1.5, 1.2485e-6, 0.0],
            [2.0, 0.0, 0.0],
        ]);
        assert_eq!(compute_crvs_normal(&k, &kinked).unwrap(), -Vec3::Z);
    }

    /// Finite samples so large that their sums overflow (1e80 and 1e155 squares, whose
    /// area vector's length or cross products overflow, and a parallelogram at 2^516
    /// whose area 2^497 is finite but whose offsets' lengths overflow) are scaled by a
    /// power of two and answer their normal, +Z (main answered a zero or NaN vector).
    #[test]
    fn loop_normal_of_samples_too_large_is_scaled() {
        let k = Kernel::default();
        for size in [1e80, 1e155] {
            let square = polyline(&[
                [0.0, 0.0, 0.0],
                [size, 0.0, 0.0],
                [size, size, 0.0],
                [0.0, size, 0.0],
                [0.0, 0.0, 0.0],
            ]);
            assert_eq!(
                compute_crvs_normal(&k, &square).unwrap(),
                Vec3::Z,
                "{size:e}"
            );
        }
        let (l, d) = (det::powi(2f64, 516), det::powi(2f64, -20));
        let parallelogram = polyline(&[
            [0.0, 0.0, 0.0],
            [l, 1.0, 0.0],
            [2.0 * l, 2.0 + d, 0.0],
            [l, 1.0 + d, 0.0],
            [0.0, 0.0, 0.0],
        ]);
        assert_eq!(compute_crvs_normal(&k, &parallelogram).unwrap(), Vec3::Z);
    }

    /// The fork's be45e8d evaluates its samples with `SER(EvaluatePoint)`: a curve whose
    /// point evaluation fails (a composite curve: `SmCurve::EvaluatePoint`, `SM_ERR`) makes
    /// the call fail with that status (upstream ignores the status and keeps the previous
    /// point).
    #[test]
    fn area_normal_returns_a_failed_point_evaluation() {
        let k = Kernel::default();
        let segment = line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], 0.0, 1.0);
        let composite = Curve::from(
            crate::curve::composite_curve::CompositeCurve::with_curves(
                3,
                vec![segment],
                false,
                None,
                None,
            )
            .unwrap(),
        );
        let side = line([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 0.0, 1.0);
        let curves = [side, composite];
        assert!(matches!(
            compute_crvs_normal(&k, &curves),
            Err(e) if crate::error::upstream_status(&e) == Some(SM_ERR)
        ));
    }

    #[test]
    fn single_line_has_no_normal() {
        let k = &Kernel::default();
        let l = line([0.0; 3], [1.0, 0.0, 0.0], 0.0, 1.0);
        assert!(compute_crvs_normal(k, &[l]).is_err());
    }

    #[test]
    fn arc_length_on_a_line() {
        let k = Kernel::default();
        let l = line([0.0; 3], [2.0, 0.0, 0.0], 0.0, 5.0);
        let ivl = l.natural_interval();
        let r = l
            .find_parameter_at_arc_length(&k, ivl.min(), 0.25 * ivl.length() * 2.0)
            .unwrap();
        assert!(r.converged);
        assert!(!r.past_end);
        let back = l.find_parameter_at_arc_length(&k, ivl.max(), -1e6).unwrap();
        assert!(back.past_end);
    }

    #[test]
    fn closest_point_on_a_hermite_curve() {
        let k = &Kernel::default();
        let c = hermite(
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [2.0, 0.0, 0.0],
            [1.0, -1.0, 0.0],
        );
        let ivl = c.natural_interval();
        let (found, sol): (bool, Solution<()>) = c
            .local_point_solve(
                k,
                &ivl,
                SolverOperation::Minimize,
                Vec3::new(1.0, 2.0, 0.0),
                &PointSolveOptions::default(),
                0.4,
            )
            .unwrap();
        assert!(found);
        assert!((sol.start.parameters[0] - 0.5).abs() < 1e-8);
    }

    /// A ray solve without its distance tolerance or its vectors is the caller's error, naming
    /// the option left out, with the C++'s `NER` status: never a stage failure (a defect).
    #[test]
    fn a_ray_solve_without_an_option_names_it_and_is_no_defect() {
        let k = &Kernel::default();
        let c = hermite(
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [2.0, 0.0, 0.0],
            [1.0, -1.0, 0.0],
        );
        let ivl = c.natural_interval();
        let dir = [Vec3::new(0.0, -1.0, 0.0)];
        for op in [
            SolverOperation::Rayfire,
            SolverOperation::Signed3dDirectedMinimize,
        ] {
            for (opts, option) in [
                (
                    PointSolveOptions {
                        vectors: Some(&dir),
                        ..PointSolveOptions::default()
                    },
                    PointSolveOption::DistanceTolerance,
                ),
                (
                    PointSolveOptions {
                        distance_tolerance: Some(1e-6),
                        ..PointSolveOptions::default()
                    },
                    PointSolveOption::Vectors,
                ),
            ] {
                let e = c
                    .local_point_solve::<()>(k, &ivl, op, Vec3::new(1.0, 2.0, 0.0), &opts, 0.4)
                    .unwrap_err();
                assert_eq!(
                    e.curve_failure(),
                    Some(&CurveFailure::PointSolveOptionMissing { option }),
                    "{op:?}"
                );
                assert_eq!(e.kind(), crate::error::ErrorKind::InvalidInput, "{op:?}");
                assert!(!e.is_defect(), "{op:?} {option:?}");
                assert_eq!(e.upstream_status(), Some(SM_ERR_NULL_POINTER));
                assert_eq!(e.sm_status(), Some(SM_ERR_NULL_POINTER));
            }
        }
    }

    #[test]
    fn an_intersection_is_decided_by_the_distance_not_by_where_the_search_stops() {
        // #17735: C(t) = (0.1 t (t - 1), 0.1 (1 - (1 - t)^3)) has |C'| < 1, so the search
        // for a point on it divides F by |C'|. Its end C(1) = (0, 0.1) is a stationary
        // point of the distance to P = C(0): the gap there is square to the tangent, so the
        // search can stop at t = 1, 0.1 from P. That stop is no answer: the solution keeps
        // its real distance, and the intersection keeps only points within its tolerance.
        use crate::curve::curve::p1::GlobalPointSolveOptions;
        use crate::geom::global_solver::SolutionRequested;
        use crate::geom::solution_array::OperandObject;
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::nurbs::crv_basic::Curve as NurbCurve;
        use crate::nurbs::math::CPoint;
        let k = &Kernel::default();
        let third = 1.0 / 30.0;
        let pw = [(0.0, 0.0), (-third, 0.1), (-third, 0.1), (0.0, 0.1)]
            .iter()
            .map(|&(x, y)| CPoint::from_point(Vec3::new(x, y, 0.0)))
            .collect();
        let knots = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
        let c = Curve::from(BSplineCurve::from_nurb(
            NurbCurve::new(pw, 3, knots).unwrap(),
            3,
            false,
        ));
        let ivl = c.natural_interval();
        let p = Vec3::ZERO;
        let (found, at_end): (bool, Solution<()>) = c
            .local_point_solve_held(
                k,
                &ivl,
                SolverOperation::Minimize,
                p,
                &PointSolveOptions::default(),
                1.0,
                true,
            )
            .unwrap();
        let t = at_end.start.parameters[0];
        let gap = c.evaluate_point(k, t).unwrap().distance(p);
        assert!(found);
        assert!(
            (at_end.start.solution_value - gap).abs() < 1e-15,
            "{t} {gap}"
        );
        assert!(
            (t - 1.0).abs() < 1e-9 && (gap - 0.1).abs() < 1e-9,
            "{t} {gap}"
        );
        let tol = 1e-9;
        let sols = c
            .global_point_solve(
                k,
                &ivl,
                SolverOperation::Intersect,
                p,
                tol,
                SolutionRequested::All,
                &GlobalPointSolveOptions::<OperandObject>::default(),
            )
            .unwrap();
        assert!(!sols.is_empty());
        for s in sols.as_slice() {
            let t = s.start.parameters[0];
            assert!(c.evaluate_point(k, t).unwrap().distance(p) <= tol, "{t}");
            assert!(t.abs() < 1e-6, "{t}");
        }
    }

    #[test]
    fn point_extrema_rejects_overflowed_hermite_tangent_without_scaling_forever() {
        let k = &Kernel::default();
        let c = hermite(
            [0.0; 3],
            [f64::MAX / 2.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0; 3],
        );
        let domain = c.natural_interval();
        let mut evaluator = FindPcExtremaEfo::new(
            k,
            &c,
            Vec3::ZERO,
            domain,
            EFF_ZERO_SQ,
            SolverOperation::Minimize,
        );
        let mut value = FunctionValue::default();
        let infinite = evaluator.evaluate(0.25, &mut value, true).unwrap_err();
        assert_eq!(
            infinite.curve_failure(),
            Some(&CurveFailure::InfiniteTangent { t: 0.25 })
        );
        assert_eq!(infinite.upstream_status(), Some(1001));
        let (found, _): (bool, Solution<()>) = c
            .local_point_solve(
                k,
                &domain,
                SolverOperation::Minimize,
                Vec3::ZERO,
                &PointSolveOptions::default(),
                0.25,
            )
            .unwrap();
        assert!(!found);
    }

    /// The quarter circle of radius 5 about the origin from (5, 0) to (0, 5), as the
    /// rational quadratic B-spline a round's edge is.
    fn quarter_circle() -> Curve {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::nurbs::crv_basic::Curve as NurbCurve;
        use crate::nurbs::math::CPoint;
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let pw = vec![
            CPoint::new(5.0, 0.0, 0.0, 1.0),
            CPoint::new(5.0 * w, 5.0 * w, 0.0, w),
            CPoint::new(0.0, 5.0, 0.0, 1.0),
        ];
        let nurb = NurbCurve::new(pw, 2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]).unwrap();
        Curve::from(BSplineCurve::from_nurb(nurb, 3, false))
    }

    /// #2546 (WR-740): the closest point of an arc to a point `rho` from its centre is the
    /// arc's point at the point's angle, `5 - rho` away. Newton's derivative there is
    /// `C'.C' rho / 5`; upstream's limit holds it at `0.05 C'.C'` and the solve runs out of
    /// iterations, from within about R / 100 of the centre. Here it is Newton's, and the
    /// point is found down to rho = 1e-6.
    #[test]
    fn issue_2546_closest_point_from_near_an_arcs_centre() {
        let c = quarter_circle();
        let ivl = c.natural_interval();
        let angle = 30f64.to_radians();
        for rho in [1e-2, 1e-4, 1e-6] {
            let p = Vec3::new(rho * det::cos(angle), rho * det::sin(angle), 0.0);
            {
                let k = Kernel::default();
                let (found, sol): (bool, Solution<()>) = c
                    .local_point_solve(
                        &k,
                        &ivl,
                        SolverOperation::Minimize,
                        p,
                        &PointSolveOptions::default(),
                        0.05,
                    )
                    .unwrap();
                assert!(found, "rho {rho}");
                let q = c.evaluate_point(&k, sol.start.parameters[0]).unwrap();
                let at = det::atan2(q.y, q.x);
                // the solve stops where the gap is square to the tangent within EFF_ZERO,
                // an angle of (rho / R) (at - angle) there: the point is as ill-conditioned
                // as R / rho, its distance not
                assert!(
                    (at - angle).abs() * rho <= EFF_ZERO * 5.0,
                    "rho {rho}: angle {at}"
                );
                let want = 5.0 - rho;
                assert!(
                    (sol.start.solution_value - want).abs() < 1e-12,
                    "rho {rho}: {} vs {want}",
                    sol.start.solution_value
                );
            }
        }
    }

    #[test]
    fn crossing_lines_intersect_locally() {
        let k = &Kernel::default();
        let a = hermite([0.0; 3], [1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
        let b = hermite(
            [0.5, -1.0, 0.0],
            [0.0, 2.0, 0.0],
            [0.5, 1.0, 0.0],
            [0.0, 2.0, 0.0],
        );
        let ivl = a.natural_interval();
        let r = local_curve_intersect(k, &a, &ivl, &b, &ivl, 1e-6, 0.4, 0.6)
            .unwrap()
            .unwrap();
        assert!((r.my_param - 0.5).abs() < 1e-9, "{r:?}");
        assert!((r.other_param - 0.5).abs() < 1e-9, "{r:?}");
        assert!(r.deviation < 1e-9);
    }
}
