//! The fillet intersector (`SmFilletIntersector`): the advanced surface intersector
//! tracing the intersection of the two offset surfaces of a fillet solver (the fillet
//! centre line), stepping through the solver's own Newton solves and flushing the
//! traced points into the centre line, the two rail curves, their UV curves and the
//! fillet surface.
//!
//! Upstream: `source/SMLib/src/SmFilletIntersector.cpp`,
//! `source/SMLib/inc/SmFilletIntersector.h`.
//!
//! | Rust                                                    | Upstream                                        |
//! |---------------------------------------------------------|-------------------------------------------------|
//! | [`FilletIntersector`], [`FilletIntersector::new`]       | `SmFilletIntersector`, its constructor          |
//! | `Drop` (implicit)                                       | `~SmFilletIntersector`                          |
//! | [`FilletIntersector::compute_point_values`]             | `ComputePointValues`                            |
//! | [`FilletIntersector::compute_point_values_delta`]       | `ComputePointValuesDelta`                       |
//! | [`FilletIntersector::test_span_accuracy`]               | `TestSpanAccuracy`                              |
//! | [`FilletIntersector::compute_next_point`]               | `ComputeNextPoint`                              |
//! | [`flush_crv`]                                           | `sm_FlushCrv` (static)                          |
//! | [`find_guess_mid_tsect_pnt`]                            | `sm_FindGuessMidTsectPnt` (static)              |
//! | [`FilletIntersector::flush_curve`]                      | `FlushCurve`                                    |
//! | [`FilletIntersector::evaluate_law_point`]               | `EvaluateLawPoint`                              |
//! | [`FilletIntersector::do_point_intersection`], [`FilletIntersector::do_law_intersection`], [`FilletDispatch`] | `DoPointIntersection`, `DoLawIntersection` (inherited) run on `*this` |
//! | [`FilletIntersector::curr_fillet_geom`], [`FilletIntersector::curr_fillet_touch_boundary`], [`FilletIntersector::is_curr_fillet_touch_boundary`], [`FilletIntersector::self_intersect`] | `GetCurrFilletGeom`, `GetCurrFilletTouchBoundary`, `IsCurrFilletTouchBoundary`, `GetSelfIntersect` |
//! | [`FilletIntersector::set_self_intersect`]               | `SetSelfIntersect`                              |
//! | [`FilletIntersector::is_kind_of`], [`TYPE`]             | `IsKindOf`, `SmFilletIntersector_TYPE`          |
//! | [`FilletSolverOps`], [`StepSolve`] (CO-PORTED interface) | the `SmFilletSolver` virtuals and members these methods call |
//! | `FilletSolverOps for ConstantRadiusAssistedFs`          | delegates to the `SmFilletSolver` base of unit `blend.fillet_solver_p1`: `ComputeSurfaceValues`, `ComputeUVVectors`, `SetSurfacesToZeroOffset`, `ReloadSurfaceOffsets`, `SetStatus`, `GetLastFilletGeom`, `GetSelfIntersectionHandler`, `GetFilletSurfaceGenerator`, `m_pFSG->CreateSurface` |
//! | [`FilStatus`] (re-export of `blend.fillet_geom_p1`'s)   | `SmFilStatus`                                   |
//! | [`HandlingAlgorithmType`] (CO-PORTED)                   | `SmHandlingAlgorithmType`                       |
//! | `one_sided_tangents` (private)                          | `sm_OneSidedTangents` (the fork's 6eecf04)      |
//!
//! Deviations:
//! - **Solver.** Upstream holds the solver by reference (`m_rFilletSolver`); here every
//!   method that uses it takes it as an argument (`solver: &mut S`), so a solver can
//!   own the surfaces it hands the intersector. `GetFilletSolver` is dropped. The
//!   solver is any [`FilletSolverOps`]: the trait gathers the `SmFilletSolver`
//!   virtuals and members (and the `SmFilletGeom` and self-intersection handler calls
//!   reached through it) that this file calls. The methods of the base
//!   `SmFilletSolver` (unit `blend.fillet_solver_p1`) are required; the defaults of
//!   the others return [`SolidError::Unported`], tagged with the unit owning each
//!   upstream symbol, so the solver units implement them as they are ported. The
//!   implementation for the co-ported [`ConstantRadiusAssistedFs`] delegates to the
//!   base [`FilletSolver`] (`m_dSavedOffsets` is the base's, uninitialised upstream,
//!   starts at `0`). The fillet geometry pointer (`SmFilletGeom*`, unit
//!   `blend.fillet_geom_p1`) is the solver's handle type
//!   [`FilletSolverOps::FilletGeom`] (a [`FilletGeomId`] of the executive's pseudo
//!   B-rep for that solver); `m_pCurrFilletGeom` is an `Option` of it. The writes
//!   `FlushCurve` makes to the geometry, its rails and their mate edge uses go to
//!   the pseudo B-rep ([`FilletBrep`]) its caller passes (upstream reaches it through
//!   the geometry pointer).
//! - **Virtual overrides.** `ComputeNextPoint`, `FlushCurve`, `ComputePointValues`,
//!   `ComputePointValuesDelta`, `EvaluateLawPoint` and `TestSpanAccuracy` are
//!   inherent methods here that take the solver; [`FilletDispatch`] implements the
//!   base drivers' hook trait (`intersect::surface_intersector::p4::IntersectorVirtuals`)
//!   with them, so [`FilletIntersector::do_point_intersection`] and
//!   [`FilletIntersector::do_law_intersection`] run the base drivers on the overrides.
//!   A driver run directly on a [`FilletIntersector`]'s `base` calls the basic
//!   intersector's versions.
//! - **Surfaces.** Upstream every caller builds `SmFilletIntersector` over the
//!   solver's own `SmOffsetSurface`s, so `SetSurfacesToZeroOffset` changes the
//!   intersector's `m_cpSurface` too (the span tests and `ComputeNextPoint` rely on
//!   it). Here the intersector borrows its two [`Surface`]s; zeroing the solver's
//!   offsets does not change them. A solver must therefore hand the intersector the
//!   surfaces it means to be evaluated (or a solver-backed surface accessor must be
//!   added); the difftest builds both sides over plain base surfaces, so the
//!   offset-surface configuration is not compared here.
//! - **Context.** `m_cpContext = rFilletSolver.GetContext()` is dropped (no contexts).
//! - **Points.** As in the base intersector (`intersect.surface_intersector_p1..p4`),
//!   the points of `ComputeNextPoint` and `TestSpanAccuracy` are arena ids
//!   ([`TsectId`]); `ComputePointValues`, `ComputePointValuesDelta` take the points by
//!   reference. `FlushCurve`'s new mid-span points are owned locally (upstream
//!   allocates and deletes them).
//! - **Outputs.** Out-params become return values ([`NextPoint`], [`SpanAccuracy`],
//!   the curve parameter and UV points of `EvaluateLawPoint`). `FlushCurve` appends
//!   to the `rCurveTypes` and `rDeviations` of a [`TsectCurves`] (its curve arrays
//!   are unused upstream too).
//! - **Statuses.** `StepSolve`'s status is turned into "no answer" as upstream; its
//!   clip and boundary flags (an out-param of [`FilletSolverOps::step_solve`]) keep
//!   whatever it set before failing, and its in/out step size is honoured.
//!   Statuses upstream ignores are ignored, except [`SolidError::Unported`], which
//!   always propagates.
//! - **Curves on surfaces.** Span tests map Hermite UV curves through [`CrvOnSurf`]
//!   on an exact evaluation copy of the concrete [`Surface`]. Plane and offset
//!   overrides are retained; a surface of revolution evaluates as its B-spline
//!   base, as upstream.
//! - **Dead code.** Step 11 of `FlushCurve` (the self-intersection blending) is
//!   guarded by the constant `bTempTrySelfIntHandler = FALSE` and never runs; it is
//!   not ported.
//! - **Skin between stations.** (not upstream, #15256) Upstream accepts a span on its
//!   rails alone, so a skin of balls' touch circles is unbounded between stations.
//!   Where the solver's surface is the exact touch circles of balls whose radius varies
//!   ([`FilletSolverOps::skins_varying_balls`]), `FlushCurve` holds the built skin to the
//!   tolerance over every span (`skin_splits`): nine samples of the span's middle
//!   iso-curve against the solved middle section's envelope steer, and a span they pass
//!   is kept only on a proved bound over its whole rectangle of the skin against the
//!   interpolated ball family `R` through the solved sections about it (`skin_bound`,
//!   Bernstein enclosures with their rounding), not against the round's true balls; a span
//!   over the tolerance, or not proved, takes its middle section as a station, and the
//!   skin is built again. That `R` is the round's true family between its solved sections
//!   is argued, not bounded, and the kept span is not marked certified: a peak or kink of
//!   the radius law strictly inside a span, narrower than about a quarter of it, is not
//!   seen (radius corners are found only at stations). The certified forms are #15657 (the
//!   round as its definition) and #16446 (interval Newton on the section equations). The geometry builder of steps 5b to 10 is
//!   `build_flush_geometry`, step 5 `equalise_spans` and step 5a `solve_mid_point`; the
//!   pieces a round whose law has corners keeps are handed to the solver once, for the
//!   final skin ([`FilletSolverOps::keep_law_corner_pieces`]).
//! - **Loops.** `FlushCurve`'s span equalisation stops once the knot count exceeds
//!   three times its start, as upstream; each pass at most doubles it. The skin's
//!   stations are added in at most `SKIN_ROUNDS` rounds and to at most
//!   `SKIN_STATION_FACTOR` times the stations the curve starts with; past either the
//!   round is refused. No recursion.
//! - **Not ported.** `Dump` and the `SM_DEBUG_CODE` drawing blocks; `SM_ASSERT`s.

mod skin_bound;

use crate::blend::fillet_custom_solver::{offset_evaluate_1st, ConstantRadiusAssistedFs};
use crate::blend::fillet_geom::p1::{set_rail_curve, FilletBrep, FilletGeomId};
use crate::blend::fillet_solver::p1::{compute_uv_vectors, FilletSolver};
use crate::blend::fillet_solver::p3::StepSolveOutcome;
use crate::blend::fillet_standard_solver::p1::FilletLaw;
use crate::blend::law_corners::LawCornerPieces;
use crate::curve::crv_on_surf::CrvOnSurf;
use crate::curve::curve::p3::PointSolveOptions;
use crate::curve::curve::Curve;
use crate::curve::hermite_curve::HermiteCurve;
use crate::error::blend::BlendFailure;
use crate::error::Failure;
use crate::error::{Result, SolidError};
use crate::geom::certified::above;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::geom_utility::p1::line_closest_point;
use crate::geom::global_solver::SolverOperation;
use crate::geom::solution_array::SolutionObject;
use crate::geom::vector2d::{Point2, Vec2};
use crate::geom::vector3d::{sqrt, Point3, Vec3};
use crate::intersect::adv_surface_intersector::AdvSurfaceIntersector;
use crate::intersect::surface_intersector::p1::{
    IntersectionPointType, SpanAccuracy, SrfIndex, SurfaceIntersector, TsectId, TsectPnt,
};
use crate::intersect::surface_intersector::p2::TsectCurves;
use crate::intersect::surface_intersector::p3::NextPoint;
use crate::intersect::surface_intersector::p4::{
    do_law_intersection_with, do_point_intersection_with, IntersectionCurve, IntersectorVirtuals,
    LawIntersectionOptions, PointIntersectionOptions,
};
use crate::kernel::Kernel;
use crate::math::{max2, max4, min2, EFF_ZERO, EFF_ZERO_SQ};
use crate::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use crate::nurbs::math::gamma;
use crate::offset::construction::bernstein::Binomials;
use crate::surface::surface::p2::{drop_vectors, TsectCurveType};
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::{edge, EdgeId};
use skin_bound::{Ball, BallFamily};

const OP_VALUES: &str = "blend::fillet_intersector::FilletIntersector::compute_point_values";
const OP_DELTA: &str = "blend::fillet_intersector::FilletIntersector::compute_point_values_delta";
const OP_SPAN: &str = "blend::fillet_intersector::FilletIntersector::test_span_accuracy";
const OP_NEXT: &str = "blend::fillet_intersector::FilletIntersector::compute_next_point";
const OP_FLUSH: &str = "blend::fillet_intersector::FilletIntersector::flush_curve";

/// `SOLV_BASE_TYPE`.
const SOLV_BASE_TYPE: i64 = 19000;

/// `SmFilletIntersector_TYPE` (`SOLV_BASE_TYPE + 52`).
pub const TYPE: i64 = SOLV_BASE_TYPE + 52;

/// `NUM_TEST_POINTS` of the span tests.
const NUM_TEST_POINTS: usize = 5;

/// `dParamsToTest` of the span tests.
const PARAMS_TO_TEST: [f64; NUM_TEST_POINTS] = [0.5, 0.17, 0.83, 0.33, 0.67];

/// The two surfaces, in upstream index order.
const SRFS: [SrfIndex; 2] = [SrfIndex::First, SrfIndex::Second];

/// The most rounds in which `FlushCurve` adds stations where a skin of balls' touch
/// circles is farther than the tolerance from them (#15256). Between stations the skin
/// is a cubic interpolant, whose distance from the balls falls about sixteenfold each
/// time a span is halved: the farthest measured, 10.2 times the tolerance, needs one
/// round, and six reach a skin `16^6` times the tolerance off.
const SKIN_ROUNDS: usize = 6;

/// The most stations a curve's skin may take, as a multiple of the stations it starts
/// with (#15256): each round at most doubles them, and the span equalisation after it
/// at most triples them again.
const SKIN_STATION_FACTOR: usize = 64;

/// A span the skin check splits (#15256): its index, the parameter of its new station and
/// the solver's section there.
type SkinSplit = (usize, f64, TsectPnt);

/// What the skin check finds (#15256): the spans it splits, the largest distance it
/// measured (infinite where a span's section could not be placed on the skin), and where it
/// last computed a value that is not finite (a distance, or the balls' rate or speed).
#[derive(Debug, Default)]
struct SkinCheck {
    splits: Vec<SkinSplit>,
    worst: f64,
    not_finite_at: Option<f64>,
}

/// The shares of the skin's section where `FlushCurve` measures it against the balls
/// (#15256): both rails and seven points between.
const SKIN_SHARES: [f64; 9] = [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0];

/// What steps 5b to 10 of `FlushCurve` build from its stations: the fillet geometry
/// they go to, the centre line, the rails and their UV curves, the fillet surface and
/// the pieces a round whose law has corners keeps.
struct FlushGeometry<G> {
    fillet_geom: G,
    center_line: BSplineCurve,
    curve1: BSplineCurve,
    curve2: BSplineCurve,
    uv1: BSplineCurve,
    uv2: BSplineCurve,
    fillet_surface: Option<Surface>,
    kept: Option<LawCornerPieces>,
}

/// The balls' envelope to first order at a section (#15256): the ball (centre `c`,
/// radius `r`) and, where its centre moves, the unit direction `a` of the centre line
/// and the cosine and sine of the angle between `a` and the envelope's normal along the
/// touch circle, `cos = -r' / |c'|`. The envelope of the balls `|x - c - c' t| = r + r' t`
/// is the cone tangent to the true envelope along the whole touch circle.
#[derive(Debug, Clone, Copy)]
struct EnvelopeCone {
    c: Point3,
    r: f64,
    axis: Option<(Vec3, f64, f64)>,
}

impl EnvelopeCone {
    /// The cone at the section with centre `cent` moving at `cent_d`, and rail points
    /// `p1`, `p2` moving at `d1`, `d2`, all along the trace: `r` is the mean distance from
    /// the centre to the rail points and `r'` its rate, read off the rails as
    /// `CircularCrossSectionFsg::create_surface_from_curves_of_balls` reads it.
    ///
    /// # Errors
    ///
    /// [`BlendFailure::BallsNest`] where the radius changes at least as fast as the centre
    /// moves (`|r'| >= |c'|`): each ball holds the next, so the balls have no envelope there;
    /// [`BlendFailure::SkinMeasureNotFinite`] where a radius, the rate or the speed is not
    /// finite. `s` is the section's parameter along the trace, for the record.
    fn at_section(
        s: f64,
        cent: Point3,
        cent_d: Vec3,
        [(p1, d1), (p2, d2)]: [(Point3, Vec3); 2],
    ) -> Result<Self> {
        let v1 = p1 - cent;
        let v2 = p2 - cent;
        let r1 = v1.length();
        let r2 = v2.length();
        let r = 0.5 * (r1 + r2);
        let speed = cent_d.length();
        // (in the section's own frame: the speed against the radius, each radius against
        // the centre's size)
        let size = 1.0 + cent.x.abs().max(cent.y.abs()).max(cent.z.abs());
        if speed <= EFF_ZERO * (1.0 + r) || r1 <= EFF_ZERO * size || r2 <= EFF_ZERO * size {
            return Ok(Self {
                c: cent,
                r,
                axis: None,
            });
        }
        let rate = 0.5 * (v1.dot(d1 - cent_d) / r1 + v2.dot(d2 - cent_d) / r2);
        if !(r1.is_finite() && r2.is_finite() && speed.is_finite() && rate.is_finite()) {
            return Err(SolidError::blend(
                OP_FLUSH,
                BlendFailure::SkinMeasureNotFinite { s },
            ));
        }
        let cos = -rate / speed;
        let sin_sq = 1.0 - cos * cos;
        // (refused within the rounding of `sin^2`: each of the rate's terms
        // `v.(d - c') / |v|` is at most `|d - c'|` and carries `gamma_16` of it, the speed
        // and the quotient `gamma_8` of `cos`, and `1 - cos^2` `gamma_2`; NaN refuses too)
        let rate_err = gamma(16) * ((d1 - cent_d).length() + (d2 - cent_d).length());
        let cos_err = rate_err / speed + gamma(8) * cos.abs();
        let sin_sq_err = cos_err * (2.0 * cos.abs() + cos_err) + gamma(2);
        if !above(sin_sq, sin_sq_err) {
            return Err(SolidError::blend(
                OP_FLUSH,
                BlendFailure::BallsNest {
                    s,
                    rate: rate.abs(),
                    speed,
                },
            ));
        }
        Ok(Self {
            c: cent,
            r,
            axis: Some((cent_d / speed, cos, sqrt(sin_sq))),
        })
    }

    /// The point of the section's touch circle (centre `c + r cos a`, radius `r sin`,
    /// square to `a`) nearest `x`'s direction about its axis: `x` projected onto the
    /// circle. Without a moving centre, `x` projected onto the ball.
    fn touch_circle_point_near(&self, x: Point3) -> Point3 {
        // the circle's centre and radius, and the plane `x` is dropped into
        let (o, radius, axis) = match self.axis {
            Some((a, cos, sin)) => (self.c + a * (self.r * cos), self.r * sin, Some(a)),
            None => (self.c, self.r, None),
        };
        let q = x - o;
        let w = axis.map_or(q, |a| q - a * q.dot(a));
        let len = w.length();
        if len > EFF_ZERO {
            o + w * (radius / len)
        } else {
            x
        }
    }

    /// The signed distance from `x` to the cone (positive outside the balls), in closed
    /// form: `min_t |x - c - c' t| - r - r' t = rho sin + h cos - r`, `h` the height of
    /// `x` along the centre line from `c` and `rho` its distance from the line; without
    /// a moving centre, the distance to the ball. It is the distance to the balls'
    /// envelope on the touch circle, and departs from it only to second order in the
    /// distance from the circle along the trace.
    fn distance(&self, x: Point3) -> f64 {
        let p = x - self.c;
        match self.axis {
            Some((a, cos, sin)) => {
                let h = p.dot(a);
                let rho = (p - a * h).length();
                rho * sin + h * cos - self.r
            }
            None => p.length() - self.r,
        }
    }
}

/// The rails' points on the faces at a section and their rates along the trace, with the
/// solver's offsets at zero.
///
/// # Errors
///
/// Those of the rails' evaluations.
fn section_rails<S: FilletSolverOps>(
    solver: &mut S,
    k: &Kernel,
    p: &TsectPnt,
) -> Result<[(Point3, Vec3); 2]> {
    let mut rails = [(Point3::ZERO, Vec3::ZERO); 2];
    for ((rail, s), slot) in SRFS.into_iter().enumerate().zip(rails.iter_mut()) {
        let uv_d = p.uv_deriv(s);
        let (pnt, du, dv) = solver.rail_surface_evaluate_1st_derivatives(k, rail, p.uv_pos(s))?;
        *slot = (pnt, du * uv_d.x + dv * uv_d.y);
    }
    Ok(rails)
}

/// A section's ball (#15256): its centre `c`, the mean distance from it to the rails' points
/// (as [`EnvelopeCone::at_section`] reads a section's radius), at the skin's parameter `at`.
fn section_ball(c: Point3, [(p1, _), (p2, _)]: [(Point3, Vec3); 2], at: f64) -> Ball {
    Ball {
        c,
        r: 0.5 * ((p1 - c).length() + (p2 - c).length()),
        at,
    }
}

// ---- co-ported enums ------------------------------------------------------------------------

/// The filleting status `SmFilStatus` is unit `blend.fillet_geom_p1`'s
/// [`FilStatus`] (one type per class).
pub use crate::blend::fillet_geom::p1::FilStatus;

/// What a self-intersection handler does with self-intersecting fillets
/// (`SmHandlingAlgorithmType`, CO-PORTED from unit `blend.fillet_solver_p1`);
/// discriminants are the upstream values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HandlingAlgorithmType {
    /// `SM_HA_UNKNOWN`.
    Unknown = 0,
    /// `SM_HA_SMOOTHING`: smooth out the self intersections.
    Smoothing = 1,
    /// `SM_HA_BLENDING`: insert a blending patch.
    Blending = 2,
}

// ---- the solver interface -------------------------------------------------------------------

/// The flags `SmFilletSolver::StepSolve` returns besides the next point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StepSolve {
    /// `rbFoundAnswer`: the Newton solve found the next point.
    pub found_answer: bool,
    /// `rbClipped`: the step was shortened to keep the point in both domains.
    pub clipped: bool,
    /// `rbBoundaryHit`: the step leaves a domain.
    pub boundary_hit: bool,
}

/// The fillet solver as `SmFilletIntersector` uses it (`SmFilletSolver& m_rFilletSolver`
/// and the objects reached through it): an interface CO-PORTED for units
/// `blend.fillet_solver_p1` to `_p4`, `blend.fillet_geom_p1` and the fillet surface
/// generators. Every default method returns [`SolidError::Unported`] tagged with the
/// unit owning the upstream symbol; a solver overrides what is ported.
pub trait FilletSolverOps {
    /// The fillet geometry handle (`SmFilletGeom*`, unit `blend.fillet_geom_p1`).
    type FilletGeom: Clone + core::fmt::Debug;

    /// `ComputePointValues(aUVValues, crUVDomain1, crUVDomain2, dCurveTraceDirection,
    /// rTsectPnt, pOptPreviousPnt)`: the point, derivatives and type of the
    /// intersection point at the rail UV points.
    ///
    /// # Errors
    ///
    /// The solver's failure. A non-virtual `SmFilletSolver` member (unit
    /// `blend.fillet_solver_p4`), so there is no default: a solver over the
    /// `SmFilletSolver` base implements it with [`FilletSolver::compute_point_values`].
    #[allow(clippy::too_many_arguments)]
    fn compute_point_values(
        &mut self,
        k: &Kernel,
        uv_values: [Point2; 2],
        uv_domain1: &Extent2d,
        uv_domain2: &Extent2d,
        curve_trace_direction: f64,
        point: &mut TsectPnt,
        previous: Option<&TsectPnt>,
    ) -> Result<()>;

    /// `ComputeSurfaceValues(aUVValues, rTsectPnt)`: the surface and curve point and
    /// derivative values at the rail UV points.
    ///
    /// # Errors
    ///
    /// The solver's failure.
    fn compute_surface_values(
        &mut self,
        k: &Kernel,
        uv_values: [Point2; 2],
        point: &mut TsectPnt,
    ) -> Result<()>;

    /// `ComputeUVVectors(rTsectPnt)` (non-virtual): the UV tangents of the point
    /// ([`crate::blend::fillet_solver::p1::compute_uv_vectors`]).
    ///
    /// # Errors
    ///
    /// The solver's failure.
    fn compute_uv_vectors(&mut self, k: &Kernel, point: &mut TsectPnt) -> Result<()>;

    /// `StepSolve(rTsectPnt, rdStep, crUVDomain1, crUVDomain2, rbFoundAnswer,
    /// rbClipped, rbBoundaryHit, rNextTSP)`: the UV points of the next point, written
    /// to `next`. `step_size` is in/out (`double& rdStep`): a clipped step is
    /// rescaled to the point found. The flags are written to `flags` as upstream
    /// writes them, so they keep what was set before a failure.
    ///
    /// # Errors
    ///
    /// The solver's failure (`FilletSolver::step_solve`, unit `blend.fillet_solver_p3`).
    #[allow(clippy::too_many_arguments)]
    fn step_solve(
        &mut self,
        k: &Kernel,
        point: &TsectPnt,
        step_size: &mut f64,
        uv_domain1: &Extent2d,
        uv_domain2: &Extent2d,
        flags: &mut StepSolve,
        next: &mut TsectPnt,
    ) -> Result<()>;

    /// `PointOnPlaneSolve(crPlaneOrig, crPlaneNormal, crUVDomain1, crUVDomain2, crUV1,
    /// crUV2, rbFoundSolution, rTsectPnt)`: the fillet point on the plane near the
    /// guesses, written to `point`; the result is `rbFoundSolution`.
    ///
    /// # Errors
    ///
    /// The solver's failure. A virtual: each implementer dispatches it on its own
    /// class (the base `SmFilletSolver::PointOnPlaneSolve` is
    /// [`crate::blend::fillet_solver::p1::FilletSolver::point_on_plane_solve`]).
    #[allow(clippy::too_many_arguments)]
    fn point_on_plane_solve(
        &mut self,
        k: &Kernel,
        plane_origin: Point3,
        plane_normal: Vec3,
        uv_domain1: &Extent2d,
        uv_domain2: &Extent2d,
        uv1: Point2,
        uv2: Point2,
        point: &mut TsectPnt,
    ) -> Result<bool>;

    /// `SetSurfacesToZeroOffset()`: saves the offsets of the solver's offset surfaces
    /// and sets them to zero (`void` upstream).
    ///
    /// # Errors
    ///
    /// The solver's failure.
    fn set_surfaces_to_zero_offset(&mut self) -> Result<()>;

    /// `ReloadSurfaceOffsets()`: restores the saved offsets (`void` upstream).
    ///
    /// # Errors
    ///
    /// The solver's failure.
    fn reload_surface_offsets(&mut self) -> Result<()>;

    /// `OffsetRadiiCanChange()`: whether the solver's offsets change along the trace
    /// (a virtual: `TRUE` for the `SmFilletSolver` base and the variable radius solver,
    /// `FALSE` for the constant radius solvers). Read only with
    /// `fixes.fillet_variable_span_follows_fillet`.
    ///
    /// # Errors
    ///
    /// The solver's failure.
    fn offset_radii_can_change(&self) -> Result<bool>;

    /// `GetRadiusCornerParameters(rParams)` (virtual, the fork's 6eecf04): the parameters
    /// on the filleted edge where the radius has a corner, ascending. None by default.
    ///
    /// # Errors
    ///
    /// The solver's failure.
    fn radius_corner_parameters(&mut self) -> Result<Vec<f64>> {
        Ok(Vec::new())
    }

    /// `IsAtRadiusCorner(crTsectPnt)` (virtual, the fork's 6eecf04): whether the point lies
    /// where the radius has a corner. `false` by default.
    fn is_at_radius_corner(&mut self, _point: &TsectPnt) -> bool {
        false
    }

    /// `GetRadiusParameter(crTsectPnt, rdParam)` (virtual, the fork's 6eecf04): the point's
    /// edge parameter, `None` where the solver has no radius corners (the default).
    fn radius_parameter(&mut self, _point: &TsectPnt) -> Option<f64> {
        None
    }

    /// `GetSurface(rail)->Evaluate1stDerivatives(uv, TRUE, TRUE, ..)`: the point and
    /// first partials of the rail's offset surface.
    ///
    /// # Errors
    ///
    /// The evaluation's failure.
    fn rail_surface_evaluate_1st_derivatives(
        &mut self,
        k: &Kernel,
        rail: usize,
        uv: Point2,
    ) -> Result<(Point3, Vec3, Vec3)>;

    /// `SetStatus(eStatus)`.
    ///
    /// # Errors
    ///
    /// The solver's failure.
    fn set_status(&mut self, status: FilStatus) -> Result<()>;

    /// `GetLastFilletGeom()`.
    ///
    /// # Errors
    ///
    /// The solver's failure (no fillet geometry).
    fn last_fillet_geom(&self) -> Result<Self::FilletGeom>;

    /// `GetSelfIntersectionHandler()` and its `GetAlgorithmType()`: `None` without a
    /// handler.
    ///
    /// # Errors
    ///
    /// The solver's failure (no executive to hold a handler);
    /// [`SolidError::Unported`] until the executive is ported.
    fn self_intersection_handler(&self) -> Result<Option<HandlingAlgorithmType>>;

    /// The handler's `SmoothOutIntersection(pFilletGeom, r3DCurves, rUVCurves1,
    /// rUVCurves2)`: joins the new curves to the fillet geometry's previous section
    /// (stored in `pseudo`).
    ///
    /// # Errors
    ///
    /// The handler's failure.
    fn smooth_out_intersection(
        &mut self,
        k: &Kernel,
        pseudo: &mut FilletBrep,
        geom: &Self::FilletGeom,
        curves_3d: &mut Vec<BSplineCurve>,
        uv_curves1: &mut Vec<BSplineCurve>,
        uv_curves2: &mut Vec<BSplineCurve>,
    ) -> Result<()>;

    /// Whether the solver has a fillet surface generator (`m_pFSG != NULL`).
    ///
    /// # Errors
    ///
    /// The solver's failure.
    fn has_surface_generator(&self) -> Result<bool>;

    /// `m_pFSG->CreateSurface(*this, pCenterLine, pCurve1, pCurve2, sPnts, pSur[0],
    /// pUV1, pSur[1], pUV2, pFilletSurface)`: the fillet surface (the generator may
    /// change the curves in place), and the pieces a round whose law has corners keeps
    /// (the fork's `AddLawCornerPieces`), which `FlushCurve` hands to
    /// [`Self::keep_law_corner_pieces`] once its stations are final.
    ///
    /// # Errors
    ///
    /// The generator's failure; [`SolidError::Unported`] for a generator of an
    /// unported unit.
    #[allow(clippy::too_many_arguments)]
    fn create_fillet_surface(
        &mut self,
        k: &Kernel,
        center_line: &mut BSplineCurve,
        curve1: &mut BSplineCurve,
        curve2: &mut BSplineCurve,
        points: &[TsectPnt],
        uv1: &mut BSplineCurve,
        uv2: &mut BSplineCurve,
    ) -> Result<(Option<Surface>, Option<LawCornerPieces>)>;

    /// `rFilletSolver.AddLawCornerPieces(..)` (the fork's 7babe17): keeps the pieces of
    /// the round [`Self::create_fillet_surface`] built.
    ///
    /// # Errors
    ///
    /// The solver's failure.
    fn keep_law_corner_pieces(&mut self, pieces: LawCornerPieces) -> Result<()>;

    /// Whether the fillet surface is the exact touch circles of balls whose radius varies
    /// along the round, skinned between the stations (#15256): a circular generator with
    /// exact, unmirrored arcs, for a solver whose ball changes its radius
    /// ([`FilletSolverType::ball_radius_varies`]). `FlushCurve` then holds the skin, not
    /// only the rails, to the tolerance.
    ///
    /// # Errors
    ///
    /// The solver's failure.
    fn skins_varying_balls(&self) -> Result<bool>;

    /// `pFilletGeom->SetFilletSurface(pFilletSurface)` (after deleting the old one); the
    /// geometry is stored in `pseudo`.
    ///
    /// # Errors
    ///
    /// The geometry lookup's.
    fn set_fillet_surface(
        &mut self,
        pseudo: &mut FilletBrep,
        geom: &Self::FilletGeom,
        surface: Option<Surface>,
    ) -> Result<()>;

    /// `pFilletGeom->m_pCenterLineCurve = pCenterLine` (after deleting the old one).
    ///
    /// # Errors
    ///
    /// The geometry lookup's.
    fn set_center_line_curve(
        &mut self,
        pseudo: &mut FilletBrep,
        geom: &Self::FilletGeom,
        center_line: BSplineCurve,
    ) -> Result<()>;

    /// `pRail = pFilletGeom->GetRail(rail); pRail->SetCurve(curve, TRUE);
    /// curve->SetOwner(pRail); pRail->SetInterval(curve->GetNaturalInterval())`.
    ///
    /// # Errors
    ///
    /// The geometry lookup's; [`SolidError::InvalidInput`] for a geometry without
    /// that rail (upstream dereferences `NULL`).
    fn set_rail_curve(
        &mut self,
        pseudo: &mut FilletBrep,
        geom: &Self::FilletGeom,
        rail: usize,
        curve: BSplineCurve,
    ) -> Result<()>;

    /// `pRail->GetPrimaryEdgeuse()->GetMate()->SetUVCurve(uv, TRUE);
    /// uv->SetOwner(mate)` for the rail `rail` of the geometry.
    ///
    /// # Errors
    ///
    /// The geometry lookup's; [`SolidError::InvalidInput`] for a missing rail,
    /// primary edge use or mate (upstream dereferences `NULL`).
    fn set_rail_mate_uv_curve(
        &mut self,
        pseudo: &mut FilletBrep,
        geom: &Self::FilletGeom,
        rail: usize,
        uv_curve: BSplineCurve,
    ) -> Result<()>;
}

/// The `SmFilletSolver` members `SmFilletIntersector` reaches through the
/// co-ported constant-radius solver, delegated to the base ([`FilletSolver`], unit
/// `blend.fillet_solver_p1`). The fillet geometry handle is a [`FilletGeomId`] of the
/// pseudo B-rep the geometry writes go to (`blend.fillet_geom_p1`).
impl FilletSolverOps for ConstantRadiusAssistedFs {
    type FilletGeom = FilletGeomId;

    fn compute_point_values(
        &mut self,
        k: &Kernel,
        uv_values: [Point2; 2],
        uv_domain1: &Extent2d,
        uv_domain2: &Extent2d,
        curve_trace_direction: f64,
        point: &mut TsectPnt,
        previous: Option<&TsectPnt>,
    ) -> Result<()> {
        // non-virtual in the base (unit blend.fillet_solver_p4); it dispatches on this
        // solver's virtuals (SetupOffsetValues, ..), so the solver itself is passed
        FilletSolver::compute_point_values(
            self,
            k,
            uv_values,
            uv_domain1,
            uv_domain2,
            curve_trace_direction,
            point,
            previous,
        )
    }

    fn compute_surface_values(
        &mut self,
        k: &Kernel,
        uv_values: [Point2; 2],
        point: &mut TsectPnt,
    ) -> Result<()> {
        // non-virtual in the base; it calls the virtual SetupOffsetValues, which
        // SmConstantRadiusFS overrides (blend.fillet_standard_solver_p1)
        FilletSolver::compute_surface_values(k, self, uv_values, point)
    }

    fn set_surfaces_to_zero_offset(&mut self) -> Result<()> {
        self.solver.set_surfaces_to_zero_offset()
    }

    fn reload_surface_offsets(&mut self) -> Result<()> {
        self.solver.reload_surface_offsets()
    }

    fn offset_radii_can_change(&self) -> Result<bool> {
        // SmConstantRadiusFS::OffsetRadiiCanChange
        Ok(ConstantRadiusAssistedFs::offset_radii_can_change(self))
    }

    fn compute_uv_vectors(&mut self, _k: &Kernel, point: &mut TsectPnt) -> Result<()> {
        // SmFilletSolver::ComputeUVVectors reads only the intersection point
        compute_uv_vectors(point)
    }

    /// `SmConstantRadiusAssistedFS::PointOnPlaneSolve` (unit
    /// `blend.fillet_standard_solver_p2`). The intersector holds no B-rep: a solver with
    /// edge uses fails in `LoadInitialValues` ([`SolidError::InvalidInput`]).
    fn point_on_plane_solve(
        &mut self,
        k: &Kernel,
        plane_origin: Point3,
        plane_normal: Vec3,
        uv_domain1: &Extent2d,
        uv_domain2: &Extent2d,
        uv1: Point2,
        uv2: Point2,
        point: &mut TsectPnt,
    ) -> Result<bool> {
        crate::blend::fillet_standard_solver::p2::point_on_plane_solve(
            self,
            k,
            None,
            plane_origin,
            plane_normal,
            uv_domain1,
            uv_domain2,
            uv1,
            uv2,
            point,
        )
    }

    fn step_solve(
        &mut self,
        k: &Kernel,
        point: &TsectPnt,
        step_size: &mut f64,
        uv_domain1: &Extent2d,
        uv_domain2: &Extent2d,
        flags: &mut StepSolve,
        next: &mut TsectPnt,
    ) -> Result<()> {
        // non-virtual in the base; it dispatches on this solver's virtuals
        // (GetSolverType, LoadInitialValues, ...), so the solver itself is passed. The
        // flags are copied back on an error too: upstream sets them in place.
        let mut out = StepSolveOutcome::default();
        let r = FilletSolver::step_solve(
            self, k, None, point, step_size, uv_domain1, uv_domain2, next, &mut out,
        );
        flags.found_answer = out.found_good_point;
        flags.clipped = out.clipped;
        flags.boundary_hit = out.boundary_hit;
        r
    }

    fn set_status(&mut self, status: FilStatus) -> Result<()> {
        self.solver.set_status(status);
        Ok(())
    }

    fn last_fillet_geom(&self) -> Result<FilletGeomId> {
        self.solver.last_fillet_geom()
    }

    fn self_intersection_handler(&self) -> Result<Option<HandlingAlgorithmType>> {
        // the handler is the executive's member: the base reports the missing
        // executive (a NULL dereference upstream)
        self.solver.self_intersection_handler()
    }

    fn smooth_out_intersection(
        &mut self,
        k: &Kernel,
        pseudo: &mut FilletBrep,
        geom: &FilletGeomId,
        curves_3d: &mut Vec<BSplineCurve>,
        uv_curves1: &mut Vec<BSplineCurve>,
        uv_curves2: &mut Vec<BSplineCurve>,
    ) -> Result<()> {
        // FlushCurve calls this only after `self_intersection_handler` found the
        // executive's smoothing handler (without an executive that read has already
        // failed: a NULL dereference upstream), and the only smoothing handler is
        // SmMakeCurveBlendSIH: run its SmoothOutIntersection on this solver
        let handler = self.solver.executive_curve_blend_sih.ok_or_else(|| {
            SolidError::invalid_input(OP_FLUSH, "no smoothing self-intersection handler")
        })?;
        handler.smooth_out_intersection(self, k, pseudo, *geom, curves_3d, uv_curves1, uv_curves2)
    }

    fn has_surface_generator(&self) -> Result<bool> {
        // GetFilletSurfaceGenerator() != NULL
        Ok(self.solver.fillet_surface_generator().is_some())
    }

    fn create_fillet_surface(
        &mut self,
        k: &Kernel,
        center_line: &mut BSplineCurve,
        curve1: &mut BSplineCurve,
        curve2: &mut BSplineCurve,
        points: &[TsectPnt],
        uv1: &mut BSplineCurve,
        uv2: &mut BSplineCurve,
    ) -> Result<(Option<Surface>, Option<LawCornerPieces>)> {
        // the virtual m_pFSG->CreateSurface(*this, pCenterLine, pCurve1, pCurve2,
        // sPnts, pSur[0], pUV1, pSur[1], pUV2, ..): the base's generator (SM_ERR for
        // the base class); it reads the solver's tolerance and GetSolverType()
        // (SM_FS_CONST_RADIUS_ASSISTED); pSur[i] = GetSurface(i), passed as copies
        let approx_tol_3d = self.solver.this_approx_tol_3d();
        let sur0 = Surface::from(self.surface(0)?.surface.clone());
        let sur1 = Surface::from(self.surface(1)?.surface.clone());
        let generator = self.solver.fillet_surface_generator_mut().ok_or_else(|| {
            SolidError::blend_missing(
                "blend::fillet_intersector::create_fillet_surface",
                "the solver's surface generator",
            )
        })?;
        generator
            .create_surface_replacing_uv(
                k,
                approx_tol_3d,
                crate::blend::fillet_solver::p1::FilletSolverType::ConstRadiusAssisted,
                Some(center_line),
                curve1,
                curve2,
                points,
                [&sur0, &sur1],
                [uv1, uv2],
                None,
            )
            .map(|(surface, kept)| (Some(surface), kept))
    }

    fn keep_law_corner_pieces(&mut self, pieces: LawCornerPieces) -> Result<()> {
        self.solver.law_corner_pieces.push(pieces);
        Ok(())
    }

    fn skins_varying_balls(&self) -> Result<bool> {
        // a constant radius solver's ball keeps its radius
        Ok(false)
    }

    fn rail_surface_evaluate_1st_derivatives(
        &mut self,
        k: &Kernel,
        rail: usize,
        uv: Point2,
    ) -> Result<(Point3, Vec3, Vec3)> {
        offset_evaluate_1st(k, self.surface_mut(rail)?, uv)
    }

    fn set_fillet_surface(
        &mut self,
        pseudo: &mut FilletBrep,
        geom: &FilletGeomId,
        surface: Option<Surface>,
    ) -> Result<()> {
        pseudo.geom_mut(*geom)?.set_fillet_surface(surface);
        Ok(())
    }

    fn set_center_line_curve(
        &mut self,
        pseudo: &mut FilletBrep,
        geom: &FilletGeomId,
        center_line: BSplineCurve,
    ) -> Result<()> {
        pseudo.geom_mut(*geom)?.center_line_curve = Some(Curve::from(center_line));
        Ok(())
    }

    fn set_rail_curve(
        &mut self,
        pseudo: &mut FilletBrep,
        geom: &FilletGeomId,
        rail: usize,
        curve: BSplineCurve,
    ) -> Result<()> {
        let rail_edge = geom_rail(pseudo, *geom, rail)?;
        let interval = curve.natural_interval();
        // SetCurve(curve, TRUE): the old curve is deleted and the change notification
        // clears the edge uses' UV trim curves and cached gaps; SetOwner is identity
        set_rail_curve(pseudo.brep_mut(), rail_edge, Curve::from(curve))?;
        edge::set_interval(pseudo.brep_mut(), rail_edge, &interval)
    }

    fn set_rail_mate_uv_curve(
        &mut self,
        pseudo: &mut FilletBrep,
        geom: &FilletGeomId,
        rail: usize,
        uv_curve: BSplineCurve,
    ) -> Result<()> {
        let rail_edge = geom_rail(pseudo, *geom, rail)?;
        let prim = eu1::primary_edgeuse(pseudo.brep(), rail_edge)?
            .ok_or_else(|| SolidError::invalid_input(OP_FLUSH, "rail without edge use"))?;
        let mate = eu1::mate(pseudo.brep(), prim)?
            .ok_or_else(|| SolidError::invalid_input(OP_FLUSH, "rail edge use without mate"))?;
        pseudo.set_edgeuse_uv_curve(mate, Some(Curve::from(uv_curve)), true, true)
    }
}

/// The solver members `SmMakeCurveBlendSIH::SmoothOutIntersection` reaches when this
/// solver is the intersector's own (no target B-rep: the rail faces' surfaces cannot
/// be read, [`SolidError::InvalidInput`] where upstream reads them).
impl crate::blend::fillet_solver::curve_blend_sih::SmoothingSolver for ConstantRadiusAssistedFs {
    fn original_face_has_surface(&self, _face: crate::topology::FaceId) -> Result<bool> {
        Err(SolidError::invalid_input(
            OP_FLUSH,
            "no target B-rep for the rail faces",
        ))
    }

    fn setup_offset_radius(&mut self, tsect: &TsectPnt) -> Result<f64> {
        let mut radius = 0.0;
        crate::blend::fillet_solver::p1::FilletSolverOps::setup_offset_values(
            self,
            tsect,
            Some(&mut radius),
        )?;
        Ok(radius)
    }

    fn base_natural_uv_domain(&self, rail: usize) -> Result<Extent2d> {
        Ok(own_offset_surface(&self.solver, rail)?
            .base_surface()
            // (kernel-free: a lazy analytic answers its NURB-less base's empty domain)
            .natural_uv_domain_if_built())
    }

    fn rail_surface(&self, rail: usize) -> Result<Surface> {
        Ok(Surface::from(
            own_offset_surface(&self.solver, rail)?.surface.clone(),
        ))
    }

    fn this_approx_tol_3d(&self) -> f64 {
        self.solver.approx_tol_3d
    }
}

/// `GetSurface(rail)` (an `SmOffsetSurface*`; upstream dereferences it unchecked).
fn own_offset_surface(
    solver: &FilletSolver,
    rail: usize,
) -> Result<&crate::blend::fillet_custom_solver::OffsetSurface> {
    solver
        .surface(rail)?
        .map(|slot| &slot.surface)
        .ok_or_else(|| SolidError::invalid_input(OP_FLUSH, "no offset surface"))
}

/// `pFilletGeom->GetRail(rail)`, dereferenced (`NULL` is [`SolidError::InvalidInput`]).
fn geom_rail(pseudo: &FilletBrep, geom: FilletGeomId, rail: usize) -> Result<EdgeId> {
    pseudo
        .geom(geom)?
        .rail(rail)
        .ok_or_else(|| SolidError::invalid_input(OP_FLUSH, "fillet geometry without rail"))
}

// ---- helpers --------------------------------------------------------------------------------

/// A status upstream ignores: `Unported` still propagates.
fn keep_unported<T>(r: Result<T>) -> Result<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.is_unported_or_cancelled() => Err(e),
        Err(_) => Ok(None),
    }
}

/// `SmCrvOnSurf(uv_curve, *surface, domain, 0, NULL)`.
fn crv_on_surf(
    k: &Kernel,
    uv_curve: HermiteCurve,
    surface: &Surface,
    domain: &Extent2d,
) -> Result<CrvOnSurf> {
    CrvOnSurf::new(
        k,
        Curve::from(uv_curve),
        surface.clone_for_evaluation()?,
        Some(*domain),
        0,
    )
}

/// `sMap.Evaluate(t, 1, TRUE, aPV)`: the position and first derivative.
fn eval1(k: &Kernel, map: &CrvOnSurf, t: f64) -> Result<[Vec3; 2]> {
    let v = map.evaluate(k, t, 1, true, true)?;
    let p = v.first().copied().unwrap_or(Vec3::UNDEF);
    let d = v.get(1).copied().unwrap_or(Vec3::UNDEF);
    Ok([p, d])
}

/// The two Hermite UV span curves `(uv(i), uv'(i) * step) -> (uv(i+1), uv'(i+1) * step)`.
fn uv_span_curves(point: &TsectPnt, next: &TsectPnt, step_size: f64) -> [HermiteCurve; 2] {
    SRFS.map(|s| {
        HermiteCurve::from_2d(
            point.uv_pos(s),
            point.uv_deriv(s) * step_size,
            next.uv_pos(s),
            next.uv_deriv(s) * step_size,
            2,
        )
    })
}

/// A rejected span: a singular point becomes a crossing point.
fn demote_singularity(p: &mut TsectPnt) {
    if p.point_type == IntersectionPointType::Singularity {
        p.point_type = IntersectionPointType::Crossing;
    }
}

/// `sm_FlushCrv`: the piecewise cubic, C0 B-spline of `cntrl_poly` over the unique
/// `knots` (end multiplicity 4, interior 3), appended to `curves`; nothing when there
/// are no knots.
///
/// # Errors
///
/// Those of `SmBSplineCurve::CreateCanonical` (bad dimension, a control point count
/// that does not match the knots).
pub fn flush_crv(
    dimension: u64,
    cntrl_poly: &[Point3],
    knots: &[f64],
    curves: &mut Vec<BSplineCurve>,
) -> Result<()> {
    // check input
    if knots.is_empty() {
        return Ok(());
    }

    // locals for BSplineCurve
    let degree = 3;
    let mut knot_mult = vec![0usize; knots.len()];
    if let Some(first) = knot_mult.first_mut() {
        *first = 4;
    }
    if let Some(last) = knot_mult.last_mut() {
        *last = 4;
    }
    let n = knot_mult.len();
    for m in knot_mult.iter_mut().take(n.saturating_sub(1)).skip(1) {
        *m = 3;
    }

    // build the BSplineCurve
    let new_bsp = BSplineCurve::create_canonical(&CanonicalData {
        dim: dimension,
        degree,
        control_points: cntrl_poly,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &knot_mult,
        knots,
        knot_type: KnotType::Unspecified,
        weights: None,
        trim: None,
    })?;

    // set output
    curves.push(new_bsp);
    Ok(())
}

/// `sm_FindGuessMidTsectPnt`: prorates `curr` and `next` at fraction `f0` into
/// `mid` (curve point, tangent direction with prorated length, UV points and tangents,
/// curve parameter; the deviation is the larger of the two).
pub fn find_guess_mid_tsect_pnt(curr: &TsectPnt, next: &TsectPnt, f0: f64, mid: &mut TsectPnt) {
    let f1 = 1.0 - f0;

    // position:
    *mid.crv_pos_mut() = f1 * curr.crv_pos() + f0 * next.crv_pos();

    // 1st deriv:
    *mid.crv_deriv_mut() = f1 * curr.crv_deriv() + f0 * next.crv_deriv();
    let len = f1 * curr.crv_deriv().length() + f0 * next.crv_deriv().length();
    // status ignored: a zero vector becomes (0, 0, 1)
    *mid.crv_deriv_mut() = mid.crv_deriv().unitize_or_z();
    *mid.crv_deriv_mut() = mid.crv_deriv() * len;

    // Surface uv points:
    for s in SRFS {
        *mid.uv_pos_mut(s) = f1 * curr.uv_pos(s) + f0 * next.uv_pos(s);
        *mid.uv_deriv_mut(s) = f1 * curr.uv_deriv(s) + f0 * next.uv_deriv(s);
    }

    // curve param:
    mid.curve_parameter = f1 * curr.curve_parameter + f0 * next.curve_parameter;

    // deviation: be pessimistic, better safe than sorry.
    mid.deviation = max2(curr.deviation, next.deviation);
}

// ---- the intersector ------------------------------------------------------------------------

/// The fillet intersector (`SmFilletIntersector`): an advanced surface intersector
/// over the (extended) domains of the two offset surfaces, driven by a fillet solver
/// ([`FilletSolverOps`]) passed to each method.
#[derive(Debug, Clone)]
pub struct FilletIntersector<'s, 't, O, G> {
    /// The `SmAdvSurfaceIntersector` base.
    pub base: AdvSurfaceIntersector<'s, 't, O>,
    /// `m_pCurrFilletGeom`: the explicit fillet geometry `FlushCurve` fills (else the
    /// solver's last one).
    curr_fillet_geom: Option<G>,
    /// `m_bCurrFilletTouchBoundary`: the last flushed curve ended on a domain boundary.
    curr_fillet_touch_boundary: bool,
    /// `m_bSelfIntersect` (no longer used upstream).
    self_intersect: bool,
}

impl<'s, 't, O: SolutionObject + 't, G: Clone> FilletIntersector<'s, 't, O, G> {
    /// The intersector of `surface1` over `uv_domain1` and `surface2` over
    /// `uv_domain2` with the fillet geometry `curr_fillet_geom` (upstream default
    /// `NULL`): the advanced intersector built with `bFromFilletIntersector = TRUE`
    /// (the domains may exceed the natural ones), without the boundary point on curve
    /// test (`m_bDoBoundaryPointOnCurveTest = FALSE`).
    ///
    /// # Errors
    ///
    /// Those of [`AdvSurfaceIntersector::new`].
    pub fn new(
        k: &'s Kernel,
        surface1: &'s Surface,
        uv_domain1: &Extent2d,
        surface2: &'s Surface,
        uv_domain2: &Extent2d,
        curr_fillet_geom: Option<G>,
    ) -> Result<Self> {
        let mut base =
            AdvSurfaceIntersector::new(k, surface1, uv_domain1, surface2, uv_domain2, true)?;
        // Will not try to extend boundary intersection point to catch
        // an end point which is also on the boundary. i.e. deactivate this feature
        base.base.do_boundary_point_on_curve_test = false;
        Ok(Self {
            base,
            curr_fillet_geom,
            curr_fillet_touch_boundary: false,
            self_intersect: false,
        })
    }

    /// `GetCurrFilletGeom`.
    #[must_use]
    pub const fn curr_fillet_geom(&self) -> Option<&G> {
        self.curr_fillet_geom.as_ref()
    }

    /// `GetCurrFilletTouchBoundary`.
    #[must_use]
    pub const fn curr_fillet_touch_boundary(&self) -> bool {
        self.curr_fillet_touch_boundary
    }

    /// `IsCurrFilletTouchBoundary`.
    #[must_use]
    pub const fn is_curr_fillet_touch_boundary(&self) -> bool {
        self.curr_fillet_touch_boundary
    }

    /// `GetSelfIntersect`.
    #[must_use]
    pub const fn self_intersect(&self) -> bool {
        self.self_intersect
    }

    /// `SetSelfIntersect`.
    pub fn set_self_intersect(&mut self, is_self_intersect: bool) {
        self.self_intersect = is_self_intersect;
    }

    /// `IsKindOf`: `SmFilletIntersector_TYPE` or a type the advanced intersector is.
    #[must_use]
    pub const fn is_kind_of(&self, t: i64) -> bool {
        t == TYPE || self.base.is_kind_of(t)
    }

    /// `ComputePointValues`: the solver's point values at `uv_values` over the two
    /// domains and the trace direction (`step_size` is unused upstream).
    ///
    /// # Errors
    ///
    /// `SM_ERR` when the solver fails; [`SolidError::Unported`] propagates.
    pub fn compute_point_values<S: FilletSolverOps<FilletGeom = G>>(
        &self,
        solver: &mut S,
        k: &Kernel,
        uv_values: [Point2; 2],
        point: &mut TsectPnt,
        previous: Option<&TsectPnt>,
        _step_size: Option<f64>,
    ) -> Result<()> {
        let si = &self.base.base;
        // pass the call along to the solver
        match solver.compute_point_values(
            k,
            uv_values,
            &si.uv_domains[0],
            &si.uv_domains[1],
            si.curve_trace_direction,
            point,
            previous,
        ) {
            Ok(()) => Ok(()),
            Err(e) if e.is_unported_or_cancelled() => Err(e),
            Err(_) => Err(SolidError::blend_failed(
                OP_VALUES,
                "the section's values could not be computed",
            )),
        }
    }

    /// `ComputePointValuesDelta`: the surface and curve values of `point` at
    /// `uv_values` and of `delta` at `uv_values_delta`; `point`'s curve tangent becomes
    /// the unit chord to `delta` (oriented with the curve parameters), its type a
    /// crossing, and its UV tangents are computed.
    ///
    /// # Errors
    ///
    /// The solver's; `SM_ERR` for a chord shorter than `SM_EFF_ZERO`.
    pub fn compute_point_values_delta<S: FilletSolverOps<FilletGeom = G>>(
        &self,
        solver: &mut S,
        k: &Kernel,
        uv_values: [Point2; 2],
        uv_values_delta: [Point2; 2],
        point: &mut TsectPnt,
        delta: &mut TsectPnt,
    ) -> Result<()> {
        // given m_pSurface[0,1] UV params - compute Surface pt, Crv pos, and deriv values
        solver.compute_surface_values(k, uv_values, point)?;
        solver.compute_surface_values(k, uv_values_delta, delta)?;

        // get curve[param deltaParam] chord (tangent approximation)
        let mut diff = delta.crv_pos() - point.crv_pos();

        // when needed - negate the chord to match the parameter progression
        if delta.curve_parameter < point.curve_parameter {
            diff = -diff;
        }

        // skip zero length chords
        if diff.length() < EFF_ZERO {
            return Err(SolidError::blend_failed(
                OP_DELTA,
                "the chord has zero length",
            ));
        }

        // unitize and save the chord
        *point.crv_deriv_mut() = diff.unitize()?;

        // set output
        point.point_type = IntersectionPointType::Crossing;

        solver.compute_uv_vectors(k, point)?;

        // all done
        Ok(())
    }

    /// `TestSpanAccuracy`: rejects the span from `point` to `next` when the tangent
    /// turns more than `this_ang_tol_rad` or (on the base surfaces, offsets set to
    /// zero) the Hermite UV spans mapped onto the surfaces stray from the Hermite 3D
    /// spans by more than the angle-adjusted tolerance (both tests only for a step
    /// longer than `100 * SM_EFF_ZERO`). With `mid`, a mid point farther than 100
    /// times the tolerance from the spans also rejects. A rejected span turns a
    /// singular `next` into a crossing point. The deviation is never set (`None`).
    ///
    /// # Errors
    ///
    /// `SM_ERR` for a zero `next` tangent; `SM_ERR_INVALID_INPUT` for a zero `point`
    /// tangent; those of the evaluations.
    pub fn test_span_accuracy<S: FilletSolverOps<FilletGeom = G>>(
        &mut self,
        solver: &mut S,
        k: &Kernel,
        point: TsectId,
        next: TsectId,
        mid: Option<TsectId>,
    ) -> Result<SpanAccuracy> {
        let si = &self.base.base;
        let tsect = si.point(point)?.clone();
        let mut next_tsp = si.point(next)?.clone();
        let mid_pnt = match mid {
            Some(m) => Some(si.point(m)?.clone()),
            None => None,
        };

        let step_size = next_tsp.curve_parameter - tsect.curve_parameter;

        if next_tsp.crv_deriv().length_squared() < EFF_ZERO_SQ {
            return Err(SolidError::blend_failed(
                OP_SPAN,
                "the next section's curve derivative is zero",
            ));
        }

        let angle_found_rad = tsect.crv_deriv().angle_between(next_tsp.crv_deriv())?;
        // First do a quick angular test to see if we can reject this span
        if angle_found_rad > si.this_ang_tol_rad && step_size > EFF_ZERO * 100.0 {
            demote_singularity(&mut next_tsp);
            *self.base.base.point_mut(next)? = next_tsp;
            return Ok(SpanAccuracy {
                found_good_point: false,
                deviation_found: None,
                angle_found_rad,
            });
        }

        let [uv_crv0, uv_crv1] = uv_span_curves(&tsect, &next_tsp, step_size);

        // First set surfaces back to the base surfaces if needed - this
        // makes the offsets zero.
        solver.set_surfaces_to_zero_offset()?;

        let s0 = si.surface(SrfIndex::First);
        let s1 = si.surface(SrfIndex::Second);
        let map0 = crv_on_surf(k, uv_crv0, s0, &si.uv_domains[0])?;
        let map1 = crv_on_surf(k, uv_crv1, s1, &si.uv_domains[1])?;

        let ivl0 = map0.natural_interval();
        let [p00, d00] = eval1(k, &map0, ivl0.min())?;
        let [p01, d01] = eval1(k, &map0, ivl0.max())?;
        let crv_3d0 = HermiteCurve::new(p00, d00, p01, d01, 3);
        let ivl1 = map1.natural_interval();
        let [p10, d10] = eval1(k, &map1, ivl1.min())?;
        let [p11, d11] = eval1(k, &map1, ivl1.max())?;
        let crv_3d1 = HermiteCurve::new(p10, d10, p11, d11, 3);

        // Utilize the angle between the normals to adjust the tolerance.
        let min_tan_angle = min2(
            tsect.tangent_plane_angle_rad,
            next_tsp.tangent_plane_angle_rad,
        );
        let mut adjusted_tolerance = 2.0 * si.this_approx_tol_3d * (min_tan_angle / 2.0).sin();
        // Don't let the adjusted tolerance get to be less then 1/10th of the actual
        // tolerance.
        adjusted_tolerance = max2(adjusted_tolerance, si.this_approx_tol_3d / 1.0);
        // Don't let points get farther away than the tolerance either
        adjusted_tolerance = min2(si.this_approx_tol_3d, adjusted_tolerance);

        let tol_sq = adjusted_tolerance * adjusted_tolerance;
        if let Some(mid_pnt) = &mid_pnt {
            let test_mid1 = s0.evaluate_point(k, mid_pnt.uv_pos(SrfIndex::First))?;
            let test_mid2 = s1.evaluate_point(k, mid_pnt.uv_pos(SrfIndex::Second))?;
            let c0 = Curve::from(crv_3d0);
            let c1 = Curve::from(crv_3d1);

            let (found, sol) = c0.local_point_solve::<()>(
                k,
                &c0.natural_interval(),
                SolverOperation::Minimize,
                test_mid1,
                &PointSolveOptions::default(),
                0.5,
            )?;
            let t0 = if found { sol.start.param(0)? } else { 0.5 };
            let crv_pnt0 = crv_3d0.evaluate_point(t0);

            let (found, sol) = c1.local_point_solve::<()>(
                k,
                &c1.natural_interval(),
                SolverOperation::Minimize,
                test_mid2,
                &PointSolveOptions::default(),
                0.5,
            )?;
            let t1 = if found { sol.start.param(0)? } else { 0.5 };
            let crv_pnt1 = crv_3d1.evaluate_point(t1);

            let mut big_tol_sq = adjusted_tolerance * 100.0;
            big_tol_sq = big_tol_sq * big_tol_sq;
            let dist1 = crv_pnt0.distance_squared(test_mid1);
            let dist2 = crv_pnt1.distance_squared(test_mid2);
            if dist1 > big_tol_sq || dist2 > big_tol_sq {
                solver.reload_surface_offsets()?;
                return Ok(SpanAccuracy {
                    found_good_point: false,
                    deviation_found: None,
                    angle_found_rad,
                });
            }
        }

        for &param in &PARAMS_TO_TEST {
            let uv3d0 = uv_crv0.evaluate_point(param);
            let uv3d1 = uv_crv1.evaluate_point(param);
            let uv0 = si.uv_domains[0].clamp_point(Point2::new(uv3d0.x, uv3d0.y));
            let uv1 = si.uv_domains[1].clamp_point(Point2::new(uv3d1.x, uv3d1.y));
            let pnt0 = s0.evaluate_point(k, uv0)?;
            let pnt1 = s1.evaluate_point(k, uv1)?;
            let crv_pnt0 = crv_3d0.evaluate_point(param);
            let crv_pnt1 = crv_3d1.evaluate_point(param);
            let dist_sq = pnt0.distance_squared(crv_pnt0);
            let dist_sq2 = pnt1.distance_squared(crv_pnt1);
            let dist_sq = max2(dist_sq, dist_sq2);
            if dist_sq > tol_sq && step_size > EFF_ZERO * 100.0 {
                demote_singularity(&mut next_tsp);
                *self.base.base.point_mut(next)? = next_tsp;
                solver.reload_surface_offsets()?;
                return Ok(SpanAccuracy {
                    found_good_point: false,
                    deviation_found: None,
                    angle_found_rad,
                });
            }
        }

        solver.reload_surface_offsets()?;
        Ok(SpanAccuracy {
            found_good_point: true,
            deviation_found: None,
            angle_found_rad,
        })
    }

    /// `ComputeNextPoint`: steps from the crossing point `prev` by `step_size` with
    /// the solver's `StepSolve`, evaluates the new point into `next`, and accepts it
    /// unless it is not a crossing, its tangent is zero or turns too much (on a step
    /// over `1000 * SM_EFF_ZERO` with distinct points), it lies behind `prev`, or the
    /// Hermite UV spans mapped onto the base surfaces stray from the Hermite 3D spans
    /// by more than `this_approx_tol_3d` (ten times that when the derivatives were
    /// recomputed after a through-point clip). A point that is not a crossing has no
    /// next point.
    ///
    /// # Errors
    ///
    /// Those of the angle and derivative computations and the evaluations; a `next`
    /// that is `prev` is [`SolidError::InvalidInput`] (two aliases of one point)
    /// when that point is a crossing (any other type returns no point, as upstream).
    pub fn compute_next_point<S: FilletSolverOps<FilletGeom = G>>(
        &mut self,
        solver: &mut S,
        k: &Kernel,
        prev: TsectId,
        step_size: f64,
        next: TsectId,
    ) -> Result<NextPoint> {
        if prev == next {
            // Upstream may alias `rTsectPnt` and `rNextTSP`: a point that is not a
            // crossing returns at once with every flag false, before either is
            // touched. A crossing would be written while it is read; refuse it.
            if self.base.base.point(prev)?.point_type != IntersectionPointType::Crossing {
                return Ok(NextPoint {
                    found_good_point: false,
                    deviation_found: None,
                    angle_found_rad: None,
                    clipped: false,
                    boundary_hit: false,
                });
            }
            return Err(SolidError::invalid_input(
                OP_NEXT,
                "the next point is the previous point",
            ));
        }
        let mut tsect = self.base.base.point(prev)?.clone();
        let mut next_tsp = self.base.base.point(next)?.clone();
        let out = self.compute_next_point_on(solver, k, &mut tsect, step_size, &mut next_tsp);
        *self.base.base.point_mut(prev)? = tsect;
        *self.base.base.point_mut(next)? = next_tsp;
        out
    }

    /// The fork's 6eecf04 in `ComputeNextPoint`: where the step from `tsect` to `next_tsp`
    /// (by `step_size`, found) crosses a corner of the radius law and `tsect` is not at
    /// one, secant steps on the step size (at most 8) until the point lands on the
    /// corner; then `next_tsp` is that point and `step_size` its step. Where it does not
    /// land, the full step is solved again (`StepSolve` leaves the offsets at its last
    /// point) and its flags replace `step`'s. Only the first corner crossed is tried.
    ///
    /// # Errors
    ///
    /// Those of `StepSolve`.
    fn land_on_radius_corner<S: FilletSolverOps<FilletGeom = G>>(
        &self,
        solver: &mut S,
        k: &Kernel,
        tsect: &TsectPnt,
        step_size: &mut f64,
        step: &mut StepSolve,
        next_tsp: &mut TsectPnt,
    ) -> Result<()> {
        let si = &self.base.base;
        let (Some(from_param), Some(to_param)) = (
            solver.radius_parameter(tsect),
            solver.radius_parameter(next_tsp),
        ) else {
            return Ok(());
        };
        let Ok(corners) = solver.radius_corner_parameters() else {
            return Ok(());
        };
        for &corner in &corners {
            if solver.is_at_radius_corner(tsect)
                || (corner - from_param) * (corner - to_param) >= 0.0
            {
                continue;
            }

            // secant steps on the step size until the point lands on the corner
            let (mut step0, mut param0) = (0.0, from_param);
            let (mut step1, mut param1) = (*step_size, to_param);
            let mut on_corner = TsectPnt::new();
            let mut landed = false;
            for _ in 0..8 {
                if landed || param1 == param0 {
                    break;
                }
                let mut d_step = step0 + (step1 - step0) * (corner - param0) / (param1 - param0);
                let mut flags = StepSolve::default();
                solver.step_solve(
                    k,
                    tsect,
                    &mut d_step,
                    &si.uv_domains[0],
                    &si.uv_domains[1],
                    &mut flags,
                    &mut on_corner,
                )?;
                let param = if flags.found_answer {
                    solver.radius_parameter(&on_corner)
                } else {
                    None
                };
                let Some(param) = param else {
                    break;
                };
                step0 = step1;
                param0 = param1;
                step1 = d_step;
                param1 = param;
                landed = solver.is_at_radius_corner(&on_corner);
            }
            if landed {
                *next_tsp = on_corner;
                *step_size = step1;
            } else {
                // back to the full step (StepSolve leaves the offsets at its last point)
                solver.step_solve(
                    k,
                    tsect,
                    step_size,
                    &si.uv_domains[0],
                    &si.uv_domains[1],
                    step,
                    next_tsp,
                )?;
            }
            break;
        }
        Ok(())
    }

    /// The body of [`Self::compute_next_point`] on detached points.
    #[allow(clippy::too_many_lines)]
    fn compute_next_point_on<S: FilletSolverOps<FilletGeom = G>>(
        &self,
        solver: &mut S,
        k: &Kernel,
        tsect: &mut TsectPnt,
        step_size: f64,
        next_tsp: &mut TsectPnt,
    ) -> Result<NextPoint> {
        let si = &self.base.base;
        // init output
        let mut out = NextPoint {
            found_good_point: false,
            deviation_found: None,
            angle_found_rad: None,
            clipped: false,
            boundary_hit: false,
        };

        // Bottom line, SM_IP_CROSSING is the only case that can have a next point.
        if tsect.point_type != IntersectionPointType::Crossing {
            return Ok(out);
        }

        // `dStepSize` is passed to StepSolve by reference: a clipped step comes back
        // rescaled and every later use below sees the new value.
        let mut step_size = step_size;

        // SolidCraft (the fork's 6eecf04): where the radius has a corner the curves turn. A
        // step from a point at a corner starts along the tangents after it
        // (ComputePointValues averages the two sides).
        if solver.is_at_radius_corner(tsect) {
            let mut after = tsect.clone();
            if one_sided_tangents(solver, k, &si.uv_domains, 1.0, &mut after)? {
                take_tangents(tsect, &after);
            }
        }

        // solve for the next SmTsectPt - save step output values
        let mut step = StepSolve::default();
        if keep_unported(solver.step_solve(
            k,
            tsect,
            &mut step_size,
            &si.uv_domains[0],
            &si.uv_domains[1],
            &mut step,
            next_tsp,
        ))?
        .is_none()
        {
            step.found_answer = false;
        }

        // SolidCraft (the fork's 6eecf04): a step across a radius corner is shortened to
        // end at the corner (a span across it can't follow the turn, and the trace would
        // creep up to it in tiny steps).
        if step.found_answer {
            self.land_on_radius_corner(solver, k, tsect, &mut step_size, &mut step, next_tsp)?;
        }

        // Set return values
        out.clipped = step.clipped;
        out.boundary_hit = step.boundary_hit;

        // quit when NR iteration can't find a next step value
        if !step.found_answer {
            return Ok(out);
        }

        // compute rNextTSP point values -
        let uvs = [
            next_tsp.uv_pos(SrfIndex::First),
            next_tsp.uv_pos(SrfIndex::Second),
        ];
        let stat =
            self.compute_point_values(solver, k, uvs, next_tsp, Some(tsect), Some(step_size));

        // stop criteria: fail to compute derivs when exiting a surface boundary - trace is done
        if keep_unported(stat)?.is_none() {
            out.boundary_hit = true; // causes termination of current tracing
            return Ok(out);
        }

        // stop criteria: next point is a tangent or coincident point - trace is done
        if next_tsp.point_type == IntersectionPointType::TangentPoint
            || next_tsp.point_type == IntersectionPointType::Coincidence
        {
            return Ok(out);
        }

        // SolidCraft (the fork's 6eecf04): a step that ends at a radius corner arrives
        // along the tangents before it
        if solver.is_at_radius_corner(next_tsp) {
            let mut before = next_tsp.clone();
            if one_sided_tangents(solver, k, &si.uv_domains, -1.0, &mut before)? {
                take_tangents(next_tsp, &before);
            }
        }

        // stop criteria: next point stepsize is zero
        if next_tsp.crv_deriv().length_squared() < EFF_ZERO_SQ {
            return Ok(out);
        }

        // (not upstream, WR-503) a point far short of the
        // step is another root of the equations, not the next point of this trace
        if !step.clipped && tsect.crv_pos().distance(next_tsp.crv_pos()) < 0.5 * step_size.abs() {
            demote_singularity(next_tsp);
            return Ok(out);
        }

        // get change in 3dCurve angle for this step
        let angle_found_rad = tsect.crv_deriv().angle_between(next_tsp.crv_deriv())?;
        out.angle_found_rad = Some(angle_found_rad);

        // when the change in tangent angle is too large -
        //   reject span over a large enough stepSize,
        //   accept span to on small steps to stop endless recursion.
        if angle_found_rad > si.this_ang_tol_rad {
            // (upstream: an empty check of nearly 180 degree turns)

            // if step size is still large enough to split
            if step_size > EFF_ZERO * 1000.0 {
                // failing point when sample pts are distinct - stop
                let tol = EFF_ZERO * 1000.0 * (1.0 + next_tsp.crv_pos().max_dimension());
                if tsect.crv_pos().distance(next_tsp.crv_pos()) > tol {
                    demote_singularity(next_tsp);
                    return Ok(out);
                }
            }
        }

        // Also make sure that the point is going in the same direction
        let line_param = line_closest_point(
            Vec3::from(tsect.uv_pos(SrfIndex::First)),
            Vec3::from(tsect.uv_deriv(SrfIndex::First)),
            Vec3::from(next_tsp.uv_pos(SrfIndex::First)),
        );
        if line_param < 0.0 {
            demote_singularity(next_tsp);
            return Ok(out);
        }

        // set next SmTsectPt parameter values = lastParameter + dStepSize
        next_tsp.curve_parameter = tsect.curve_parameter + step_size;

        // But first - set deriviatves with finite differences
        //   when m_bClippedByThroughPoint == TRUE
        //   and the magnitude of either uv derivative has changed substantially
        let mut adjust_derivatives = false;
        if si.clipped_by_through_point {
            for which_surf in SRFS {
                let start_uv_len = tsect.uv_deriv(which_surf).length();
                let end_uv_len = next_tsp.uv_deriv(which_surf).length();

                if start_uv_len > 3.0 * end_uv_len || end_uv_len > 3.0 * start_uv_len {
                    // Compute derivatives by making a small step
                    let mut small_step = TsectPnt::new();
                    let mut d_step = 1.0e-5;
                    let mut small_flags = StepSolve::default();
                    solver.step_solve(
                        k,
                        tsect,
                        &mut d_step,
                        &si.uv_domains[0],
                        &si.uv_domains[1],
                        &mut small_flags,
                        &mut small_step,
                    )?;
                    let found_good = small_flags.found_answer;

                    if found_good {
                        // Got uv's, fill in the rest.
                        let uv_array = [
                            next_tsp.uv_pos(SrfIndex::First),
                            next_tsp.uv_pos(SrfIndex::Second),
                        ];
                        if keep_unported(self.compute_point_values(
                            solver,
                            k,
                            uv_array,
                            &mut small_step,
                            None,
                            None,
                        ))?
                        .is_none()
                        {
                            break; // If either doesn't work, forget this idea.
                        }

                        // Set the deriv of the 3d curve using finite difference.
                        let diff = small_step.crv_pos() - tsect.crv_pos();
                        *tsect.crv_deriv_mut() = diff / d_step;
                        // SER(Unitize()): a zero vector is left as (0, 0, 1)
                        let unit = tsect.crv_deriv().unitize();
                        *tsect.crv_deriv_mut() = tsect.crv_deriv().unitize_or_z();
                        unit?;

                        // Set the derivs of the uv curves on both surfaces.
                        for j_srf in SRFS {
                            let dropped = drop_vectors(
                                small_step.srf_du(j_srf),
                                small_step.srf_dv(j_srf),
                                &[tsect.crv_deriv()],
                            )?;
                            let new_deriv = dropped.first().copied().unwrap_or(Vec2::UNDEF);

                            // Set it only if shorter.  (?)
                            let orig_len = tsect.uv_deriv(j_srf).length();
                            if new_deriv.length() < orig_len {
                                *tsect.uv_deriv_mut(j_srf) = new_deriv;
                            }
                        }
                        adjust_derivatives = true; // Remember for later.
                    }

                    // If we did this for the first surf,
                    // no need to do it again for the second.
                    break;
                }
            }
        }

        // create the Hermite UVTrimCurves
        let [uv_crv0, uv_crv1] = uv_span_curves(tsect, next_tsp, step_size);

        // Set OffsetSurface offset distance to zero to get baseSurface evaluations.
        solver.set_surfaces_to_zero_offset()?;

        // use UVTrimCurves on baseSurfaces to define 3d railCurves for this step
        let s0 = si.surface(SrfIndex::First);
        let s1 = si.surface(SrfIndex::Second);
        let map0 = crv_on_surf(k, uv_crv0, s0, &si.uv_domains[0])?;
        let map1 = crv_on_surf(k, uv_crv1, s1, &si.uv_domains[1])?;

        let crv_3d0 = rail_hermite(k, &map0, adjust_derivatives)?;
        let crv_3d1 = rail_hermite(k, &map1, adjust_derivatives)?;

        let mut adjusted_tolerance = si.this_approx_tol_3d;
        if adjust_derivatives {
            adjusted_tolerance *= 10.0;
        }
        let tol_sq = adjusted_tolerance * adjusted_tolerance;

        // for every test point
        let mut max_dist_sq = 0.0;
        for &param in &PARAMS_TO_TEST {
            // get UVPnts from UVTrimCurves clamped to surf domain
            let uv3d0 = uv_crv0.evaluate_point(param);
            let uv3d1 = uv_crv1.evaluate_point(param);
            let uv0 = si.uv_domains[0].clamp_point(Point2::new(uv3d0.x, uv3d0.y));
            let uv1 = si.uv_domains[1].clamp_point(Point2::new(uv3d1.x, uv3d1.y));

            // evaluate the associated surface points
            let pnt0 = s0.evaluate_point(k, uv0)?;
            let pnt1 = s1.evaluate_point(k, uv1)?;

            // evaluate the associated 3d RailCurve points
            let crv_pnt0 = crv_3d0.evaluate_point(param);
            let crv_pnt1 = crv_3d1.evaluate_point(param);

            // get distance between 3dCurve(param) points and Surf(UVTrimCurve(param)) points
            let dist_sq = pnt0.distance_squared(crv_pnt0);
            let dist_sq2 = pnt1.distance_squared(crv_pnt1);
            let dist_sq = max2(dist_sq, dist_sq2);

            // when max dDistSq value is too large on a long enough step - reject this span
            if dist_sq > tol_sq && step_size > EFF_ZERO * 100.0 {
                demote_singularity(next_tsp);
                solver.reload_surface_offsets()?;
                return Ok(out);
            }

            // save the max deviation
            if dist_sq > max_dist_sq {
                max_dist_sq = dist_sq;
            }
        }

        // (not upstream, `fixes.fillet_variable_span_follows_fillet`, WR-646; the fork's
        // tangency-labs/smlib@4438859) where the fillet's offsets change along the trace
        // (a variable radius), the rails between two points must follow the fillet, not
        // only lie on the faces: on a plane every uv curve does, so the test above holds
        // for any span, the step grows to the angle tolerance, and the Hermite rails cut
        // across the radius law between the points. The fillet is solved at the test
        // points (a step of that share of the span from the start), and the span is
        // rejected where a solution's contacts lie farther than the tolerance from the
        // rails.
        if k.fixes.fillet_variable_span_follows_fillet
            && solver.offset_radii_can_change()?
            && step_size > EFF_ZERO * 100.0
        {
            let rails = [&crv_3d0, &crv_3d1];
            for &param in &PARAMS_TO_TEST {
                let mut part_step = step_size * param;
                let mut part_flags = StepSolve::default();
                let mut part = TsectPnt::new();
                solver.reload_surface_offsets()?;
                solver.step_solve(
                    k,
                    tsect,
                    &mut part_step,
                    &si.uv_domains[0],
                    &si.uv_domains[1],
                    &mut part_flags,
                    &mut part,
                )?;
                solver.set_surfaces_to_zero_offset()?;
                if !part_flags.found_answer || part_flags.clipped {
                    continue; // no solution inside the span there: the rails' own test stands
                }
                let contact0 = s0.evaluate_point(
                    k,
                    si.uv_domains[0].clamp_point(part.uv_pos(SrfIndex::First)),
                )?;
                let contact1 = s1.evaluate_point(
                    k,
                    si.uv_domains[1].clamp_point(part.uv_pos(SrfIndex::Second)),
                )?;
                for (rail, contact) in rails.into_iter().zip([contact0, contact1]) {
                    let rail_curve = Curve::from(*rail);
                    let (on, sol) = rail_curve.local_point_solve::<()>(
                        k,
                        &rail_curve.natural_interval(),
                        SolverOperation::Minimize,
                        contact,
                        &PointSolveOptions::default(),
                        param,
                    )?;
                    let t = if on { sol.start.param(0)? } else { param };
                    let on_rail = rail.evaluate_point(t);
                    if on_rail.distance_squared(contact) > tol_sq {
                        solver.reload_surface_offsets()?;
                        out.found_good_point = false;
                        return Ok(out);
                    }
                }
            }
        }

        // set output and return FoundGoodPoint == TRUE
        out.deviation_found = Some(sqrt(max_dist_sq));
        out.found_good_point = true; // passed test
        solver.reload_surface_offsets()?;
        Ok(out)
    }

    /// `FlushCurve`: takes the curve points (clearing the list), notes whether the
    /// last one is on a domain boundary ([`Self::curr_fillet_touch_boundary`]), and
    /// with at least two points builds the centre line, the rail UV curves and the
    /// rail 3D curves (one cubic Bézier span per point span, spans first equalised to
    /// within a factor 2.8 by solver mid points, a tiny last span merged), appends the
    /// curve type and deviation to `out`, lets the self-intersection handler smooth
    /// them, builds the fillet surface and stores everything in the current (else the
    /// solver's last) fillet geometry.
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` without curve points; `SM_ERR` when a mid point has no
    /// solution or the handler or surface generator fails (after setting the solver
    /// status); those of the solver and the curve construction.
    #[allow(clippy::too_many_lines)]
    pub fn flush_curve<S: FilletSolverOps<FilletGeom = G>>(
        &mut self,
        solver: &mut S,
        k: &Kernel,
        pseudo: &mut FilletBrep,
        out: &mut TsectCurves,
    ) -> Result<()> {
        // 1. offset surface sample points
        let ids = self.base.base.curve_points().to_vec();
        let mut pnts = ids
            .iter()
            .map(|&id| self.base.base.point(id).cloned())
            .collect::<Result<Vec<_>>>()?;

        // 2. Remember if the end of current fillet touched a boundary on
        //    either surface UVDomain
        let last_tsect_pnt = pnts
            .last()
            .ok_or_else(|| SolidError::blend_missing(OP_FLUSH, "the last section point"))?;
        self.curr_fillet_touch_boundary = false;

        // for both surfaces
        for (s, dom) in SRFS.into_iter().zip(self.base.base.uv_domains.iter()) {
            // get tolerance proportional to max UV coordinate value
            let size = max4(
                dom.min().x.abs(),
                dom.min().y.abs(),
                dom.max().x.abs(),
                dom.max().y.abs(),
            );
            let tol = EFF_ZERO * (1.0 + size) * 100.0;

            // remember if lastTsectPnt is on this surface's (extended) uv domain boundary
            if dom.is_point_on_boundary(last_tsect_pnt.uv_pos(s), tol).0 {
                self.curr_fillet_touch_boundary = true;
                break;
            }
        }

        // Clear out curve point list that's been copied into sPnts
        self.base.base.curve_points.clear();

        // no work - not enough xsect points to flush a curve
        if pnts.len() < 2 {
            return Ok(());
        }

        // new curve locals
        let mut knots: Vec<f64> = Vec::new();

        // 3. If the final span is tiny, remove it by consolidating it with
        // the previous span.
        if pnts.len() > 2 {
            let n = pnts.len();
            let lt = pnts.get(n - 1).map_or(0.0, |p| p.curve_parameter);
            let nlt = pnts.get(n - 2).map_or(0.0, |p| p.curve_parameter);
            let nnlt = pnts.get(n - 3).map_or(0.0, |p| p.curve_parameter);
            let ratio = (nlt - nnlt) / (lt - nlt);
            if ratio > 10.0 {
                // If the ratio is too big remove the next to last point
                if let Some(last) = pnts.pop() {
                    if let Some(slot) = pnts.last_mut() {
                        *slot = last;
                    }
                }
            }
        }

        // 4. Build array of knots
        for p in &pnts {
            knots.push(p.curve_parameter);
        }

        // 5. add knots so that each span is similiarly sized
        self.equalise_spans(solver, k, &mut pnts, &mut knots)?;

        // 5c. (not upstream, #15256) where the skin is the touch circles of balls whose
        //     radius varies, hold the skin, not only the rails, to the tolerance: steps
        //     5b to 10 build the curves and the skin, then each span's middle section is
        //     solved and the skin measured against it (`skin_splits`); a span whose skin
        //     is farther than the tolerance takes that section as a station, the spans are
        //     equalised again and everything is built again, at most `SKIN_ROUNDS` times.
        let skin_held = Self::holds_skin(solver)?;
        let station_cap = pnts.len().saturating_mul(SKIN_STATION_FACTOR);
        let (deviations, curve_types) = (out.deviations.len(), out.curve_types.len());
        let mut round = 0;
        let built = loop {
            k.check_cancelled(OP_FLUSH)?;
            out.deviations.truncate(deviations);
            out.curve_types.truncate(curve_types);
            let built = self.build_flush_geometry(solver, k, pseudo, &pnts, &knots, out)?;
            // (a round whose law has corners, built wholly from the pieces' definitions, is
            // their certified nets joined, not a skin of the stations; where some pieces are
            // skins, those are measured and the definitions passed over, `skin_splits`)
            let defined = built.kept.as_ref().is_some_and(|kept| {
                !kept.pieces.is_empty()
                    && kept
                        .pieces
                        .iter()
                        .all(|p| matches!(p.kind(), SurfaceKind::ProceduralBlend(_)))
            });
            let Some(skin) = built
                .fillet_surface
                .as_ref()
                .filter(|_| skin_held && !defined)
            else {
                break built;
            };
            // (the pieces of a round whose law has corners each skin their own stations:
            // the joined surface averages the pieces' rows at each corner, where the session
            // builds the corner from the pieces)
            let skins: Vec<&Surface> = match built.kept.as_ref() {
                Some(kept) => kept.pieces.iter().collect(),
                None => vec![skin],
            };
            let SkinCheck {
                splits,
                worst,
                not_finite_at,
            } = self.skin_splits(solver, k, &pnts, &skins)?;
            if splits.is_empty() {
                break built;
            }
            if round == SKIN_ROUNDS || pnts.len().saturating_add(splits.len()) > station_cap {
                let failure = match not_finite_at {
                    Some(s) => BlendFailure::SkinMeasureNotFinite { s },
                    None => BlendFailure::SkinPastStationCap {
                        asked: self.base.base.this_approx_tol_3d,
                        reached: worst,
                        stations: pnts.len(),
                        rounds: round,
                    },
                };
                return Err(SolidError::blend(OP_FLUSH, failure));
            }
            // each split's section after the station it follows
            let mut splits = splits.into_iter().peekable();
            let mut merged = Vec::new();
            let mut merged_knots = Vec::new();
            for (i, (p, knot)) in pnts.into_iter().zip(knots).enumerate() {
                merged.push(p);
                merged_knots.push(knot);
                if let Some((_, mid_knot, mid)) = splits.next_if(|(j, ..)| *j == i) {
                    merged.push(mid);
                    merged_knots.push(mid_knot);
                }
            }
            pnts = merged;
            knots = merged_knots;
            self.equalise_spans(solver, k, &mut pnts, &mut knots)?;
            round += 1;
        };
        let FlushGeometry {
            fillet_geom,
            center_line,
            curve1,
            curve2,
            uv1,
            uv2,
            fillet_surface,
            kept,
        } = built;

        // the fork's `rFilletSolver.AddLawCornerPieces` (7babe17), once for the final skin
        if let Some(kept) = kept {
            solver.keep_law_corner_pieces(kept)?;
        }

        // 11. Correct self-intersecting fillet-surface: never runs upstream
        //     (bTempTrySelfIntHandler is a FALSE constant).

        // 12. Save Fillet Surface and rail curve data
        solver.set_fillet_surface(pseudo, &fillet_geom, fillet_surface)?;
        solver.set_center_line_curve(pseudo, &fillet_geom, center_line)?;
        solver.set_rail_curve(pseudo, &fillet_geom, 0, curve1)?;
        solver.set_rail_mate_uv_curve(pseudo, &fillet_geom, 0, uv1)?;
        solver.set_rail_curve(pseudo, &fillet_geom, 1, curve2)?;
        solver.set_rail_mate_uv_curve(pseudo, &fillet_geom, 1, uv2)?;

        // restore FilletSolver offset distances in offset Surfaces stored in the FilletSolver
        solver.reload_surface_offsets()?;

        // all done
        Ok(())
    }

    /// Whether `FlushCurve` holds the skin to the tolerance between stations (#15256):
    /// the solver's surface is the exact touch circles of balls whose radius varies
    /// ([`FilletSolverOps::skins_varying_balls`]) and no smoothing handler changes the
    /// curves (it edits the previous piece in place, so it runs once).
    ///
    /// # Errors
    ///
    /// The solver's.
    fn holds_skin<S: FilletSolverOps<FilletGeom = G>>(solver: &S) -> Result<bool> {
        Ok(solver.skins_varying_balls()?
            && solver.self_intersection_handler()? != Some(HandlingAlgorithmType::Smoothing))
    }

    /// The spans of the skins (built on the stations `pnts`: one skin, or the pieces of
    /// a round whose law has corners, together over the stations' parameters) farther
    /// than the tolerance from the balls in their middle (#15256), each with the
    /// solver's section there ([`Self::solve_mid_point`], the section the span
    /// equalisation adds) and that section's parameter on the skin. The parameter is
    /// where the skin passes the section's touch circle (the circle's point nearest the
    /// skin's middle iso-curve, projected onto the skin), so the section, added as a
    /// station there, keeps the skin's own parametrisation: at the middle knot it would
    /// sit where the stations' Hermite centre line puts it, up to 2e-2 along the trace
    /// from where the skin does, and bend the skin through its neighbours. The skin's
    /// iso-curve at that parameter is measured at [`SKIN_SHARES`] against
    /// [`EnvelopeCone`], the envelope of the section's ball family to first order, by its
    /// closed-form distance.
    ///
    /// The samples only steer (they split a span, never keep one). A span they pass is
    /// kept only where [`skin_bound::span_bound`] proves the skin over the span's whole
    /// rectangle within the tolerance of the interpolated ball family `R` through the solved
    /// balls about it (its stations, its middle and, past a station where the round goes on
    /// smoothly, the neighbouring span's middle), and split otherwise
    /// (`docs/definitions-and-certificates.md` §7.6: the decision is on Bernstein
    /// enclosures, with their rounding, not on samples). The bound is against `R`, not the
    /// round's true balls: that `R` is the true family between the solved sections, which
    /// are the solver's converged Newton solutions placed by converged projections, is
    /// ARGUED, and the kept span is not marked certified. A peak or kink of the radius law
    /// strictly inside a span, narrower than about a quarter of it, is not seen
    /// ([`FilletSolverOps::is_at_radius_corner`] is asked only at stations). The certified
    /// forms: the round as its definition with a certified net (#15657), interval Newton on
    /// the section equations (#16446). Only
    /// B-spline skins are measured: a definition's net is certified against its definition,
    /// and the spans a definition piece holds are passed over; any other kind is a port
    /// invariant. Leaves the solver's offsets loaded.
    ///
    /// Returns the splits, the largest distance measured (infinite where a span's section
    /// could not be placed on the skin, so the span was split unmeasured) and where a value
    /// it computed was not finite. A distance, or a cone, that is not finite splits its span
    /// as a span off its balls does: it proves nothing either way, and a local failure of an
    /// evaluation goes away with the station. `FlushCurve` refuses only if one is still not
    /// finite at the cap.
    ///
    /// Where the section does not project into its span (the skin passes it more than half
    /// a span from where its stations put it, or the projection does not converge), the span
    /// is split at its middle knot with that section, as [`Self::equalise_spans`] places a
    /// station: the span needs stations either way, so the check is total and the case
    /// converges or reaches the cap.
    ///
    /// # Errors
    ///
    /// [`BlendFailure::StationDidNotConverge`] where no middle section is found from the
    /// interpolated guess nor from either end station's (with the last solve's error as its
    /// cause, where one failed); a cancellation or port failure of the plane solve as it is;
    /// [`SolidError::PortInvariant`] for skins that do not run over the stations' parameters,
    /// which they are built from, or of a kind the generator does not build; a cancellation
    /// (polled for each span); [`BlendFailure::BallsNest`] from
    /// [`EnvelopeCone::at_section`]; those of the rails' evaluations and the projection.
    fn skin_splits<S: FilletSolverOps<FilletGeom = G>>(
        &self,
        solver: &mut S,
        k: &Kernel,
        pnts: &[TsectPnt],
        skins: &[&Surface],
    ) -> Result<SkinCheck> {
        let tol = self.base.base.this_approx_tol_3d;
        let (Some(first), Some(last)) = (pnts.first(), pnts.last()) else {
            solver.reload_surface_offsets()?;
            return Ok(SkinCheck::default());
        };
        // the B-spline skins and their domains, which together run over the stations'
        // parameters (one skin, or the pieces of a round whose law has corners)
        let size = 1.0 + first.curve_parameter.abs().max(last.curve_parameter.abs());
        let near = |a: f64, b: f64| (a - b).abs() <= EFF_ZERO * size;
        // (a piece that is a round's definition is its certified net: it is passed over,
        // and the spans it holds with it; any other kind is not what the generator builds)
        let mut nets = Vec::new();
        let mut defined = false;
        for skin in skins {
            match skin.kind() {
                SurfaceKind::BSpline(net) => nets.push((*skin, net.natural_uv_domain())),
                SurfaceKind::ProceduralBlend(_) => defined = true,
                _ => {
                    return Err(SolidError::port_invariant(
                        OP_FLUSH,
                        "a variable round's skin is neither a B-spline nor its definition",
                    ));
                }
            }
        }
        let joined = nets.windows(2).all(|w| match w {
            [(_, a), (_, b)] => near(a.max().x, b.min().x),
            _ => true,
        });
        let (Some((_, d0)), Some((_, d1))) = (nets.first(), nets.last()) else {
            solver.reload_surface_offsets()?;
            return Ok(SkinCheck::default());
        };
        if !defined
            && (!joined
                || !near(d0.min().x, first.curve_parameter)
                || !near(d1.max().x, last.curve_parameter))
        {
            // (the skin is built from these stations: a defect of its construction)
            return Err(SolidError::port_invariant(
                OP_FLUSH,
                "the skin does not run over its stations' parameters",
            ));
        }

        // each span's middle section, with the offsets loaded
        solver.reload_surface_offsets()?;
        let mut mids = Vec::new();
        for (j, pair) in pnts.windows(2).enumerate() {
            k.check_cancelled(OP_FLUSH)?;
            let [pj, pj1] = pair else {
                continue;
            };
            let frac = 0.5;
            let knot = (1.0 - frac) * pj.curve_parameter + frac * pj1.curve_parameter;
            // the B-spline skin over the span (a span's middle is never a piece's end: the
            // pieces end at stations); where a definition piece holds it, the span is passed
            // over
            let Some(net) = nets
                .iter()
                .position(|(_, d)| d.min().x < knot && knot < d.max().x)
            else {
                if defined {
                    continue;
                }
                return Err(SolidError::port_invariant(
                    OP_FLUSH,
                    "no skin runs over a span's middle",
                ));
            };
            // from the interpolated guess, then from either end station's, retried after a
            // failed solve too (#15034 rule 5: a solve that does not converge is a defect,
            // never "no section"); a cancellation or a port failure is not retried
            let mut mid = None;
            let mut failed = None;
            for start in [None, Some(pj), Some(pj1)] {
                match self.solve_mid_point_from(solver, k, pj, pj1, frac, start) {
                    Ok(Some(found)) => {
                        mid = Some(found);
                        break;
                    }
                    Ok(None) => {}
                    Err(e) if e.is_port_failure() || matches!(e, SolidError::Cancelled { .. }) => {
                        return Err(e);
                    }
                    Err(e) => failed = Some(e),
                }
            }
            let Some(mid) = mid else {
                // (the plane solve reports neither its residual, its last step nor its
                // iterations, only whether it found the section)
                let failure = Failure::new(
                    OP_FLUSH,
                    BlendFailure::StationDidNotConverge {
                        s: knot,
                        residual: None,
                        tolerance: self.base.base.this_approx_tol_3d,
                        step: None,
                        iterations: None,
                    },
                );
                return Err(match failed {
                    Some(e) => failure.caused_by(e),
                    None => failure,
                }
                .into_error());
            };
            mids.push((j, knot, net, mid));
        }

        // its rail points and derivatives on the faces, and the skin against its cone
        solver.set_surfaces_to_zero_offset()?;
        let mut splits = Vec::new();
        let mut worst = 0.0_f64;
        let mut not_finite_at = None;
        let binomials = Binomials::new();
        // the stations' balls (their rails on the faces), whether each is a corner of the
        // radius law, each measured span's middle ball and the spans the samples pass
        let mut station_balls: Vec<Option<Ball>> = Vec::new();
        let mut corner = Vec::new();
        for p in pnts {
            let rails = section_rails(solver, k, p)?;
            let ball = section_ball(p.crv_pos(), rails, p.curve_parameter);
            // (the family is not carried past a corner of the radius law, nor past a station
            // where the radius runs to zero, where the radius turns as `|r|`; a corner strictly
            // inside a span is not seen here, #16446)
            corner.push(solver.is_at_radius_corner(p) || !above(ball.r, tol));
            station_balls.push(Some(ball));
        }
        let mut mid_balls: Vec<Option<Ball>> = vec![None; pnts.len()];
        let mut pending = Vec::new();
        for (j, knot, net, mid) in mids {
            k.check_cancelled(OP_FLUSH)?;
            let Some(&(skin, domain)) = nets.get(net) else {
                return Err(SolidError::port_invariant(
                    OP_FLUSH,
                    "no skin runs over a span's middle",
                ));
            };
            let rails = section_rails(solver, k, &mid)?;
            // (a cone that is not finite leaves the span unmeasured: it is split at its knot,
            // as below, and refused only if the values stay not finite to the cap)
            let cone = match EnvelopeCone::at_section(knot, mid.crv_pos(), mid.crv_deriv(), rails) {
                Err(e)
                    if matches!(
                        e.blend_failure(),
                        Some(BlendFailure::SkinMeasureNotFinite { .. })
                    ) =>
                {
                    not_finite_at = Some(knot);
                    splits.push((j, knot, mid));
                    continue;
                }
                cone => cone?,
            };
            // the section's parameter on the skin: where the skin passes its touch
            // circle's middle, so the skin is measured there and, added as a station at
            // that parameter, the section keeps the skin's own parametrisation (the
            // middle knot is where the stations' centre line, not the skin, puts it)
            let v_mid = 0.5 * (domain.min().y + domain.max().y);
            let near_mid = skin.evaluate_point(k, Point2::new(knot, v_mid))?;
            let on_circle = cone.touch_circle_point_near(near_mid);
            let (lo, hi) = (
                pnts.get(j).map_or(knot, |p| p.curve_parameter),
                pnts.get(j + 1).map_or(knot, |p| p.curve_parameter),
            );
            let solved = skin.local_point_solve::<()>(
                k,
                &domain,
                SolverOperation::Minimize,
                on_circle,
                Point2::new(knot, v_mid),
            )?;
            let at = solved.solution.start.param(0)?;
            // (written to accept: a section off its span, or none found, splits the span
            // at its middle knot unmeasured)
            let in_span = solved.found && lo < at && at < hi;
            if !in_span {
                splits.push((j, knot, mid));
                worst = f64::INFINITY;
                continue;
            }
            let mut held = true;
            for share in SKIN_SHARES {
                let v = domain.min().y + share * (domain.max().y - domain.min().y);
                let off = cone
                    .distance(skin.evaluate_point(k, Point2::new(at, v))?)
                    .abs();
                // (a distance that is not finite proves nothing about the skin either way:
                // the span is split, as a span off its balls is, and the round is refused
                // only if a value stays not finite to the cap)
                if !off.is_finite() {
                    not_finite_at = Some(at);
                    held = false;
                    continue;
                }
                worst = worst.max(off);
                held &= off <= tol;
            }
            // the samples only steer: a span they pass is kept on its proved bound against the
            // interpolated family R over the whole span (below), and split where it is over
            // the tolerance or not proved
            if let Some(slot) = mid_balls.get_mut(j) {
                *slot = Some(section_ball(mid.crv_pos(), rails, at));
            }
            if held {
                let mut mid = mid;
                mid.curve_parameter = at;
                pending.push((j, at, mid, skin, domain));
                continue;
            }
            if !held {
                let mut mid = mid;
                mid.curve_parameter = at;
                splits.push((j, at, mid));
            }
        }
        // each span the samples pass, against the closed-form family of balls through the
        // solved ones about it (`skin_bound`): its stations, its middle and, past a station
        // where the round goes on smoothly, the next span's middle
        for (j, at, mid, skin, domain) in pending {
            k.check_cancelled(OP_FLUSH)?;
            let ball = |i: usize| station_balls.get(i).copied().flatten();
            let mut nodes = Vec::new();
            if j > 0 && !corner.get(j).copied().unwrap_or(true) {
                nodes.extend(mid_balls.get(j - 1).copied().flatten());
            }
            nodes.extend(ball(j));
            nodes.extend(mid_balls.get(j).copied().flatten());
            nodes.extend(ball(j + 1));
            if !corner.get(j + 1).copied().unwrap_or(true) {
                nodes.extend(mid_balls.get(j + 1).copied().flatten());
            }
            let (lo, hi) = (
                pnts.get(j).map_or(f64::NAN, |p| p.curve_parameter),
                pnts.get(j + 1).map_or(f64::NAN, |p| p.curve_parameter),
            );
            let bound = match BallFamily::through(&nodes, at, [lo, hi]) {
                Some(family) => skin_bound::span_bound(
                    k,
                    skin,
                    &family,
                    [lo, hi],
                    [domain.min().y, domain.max().y],
                    tol,
                    &binomials,
                )?,
                None => f64::INFINITY,
            };
            worst = worst.max(bound);
            // (NaN is not within the tolerance)
            if bound.is_nan() || above(bound, tol) {
                splits.push((j, at, mid));
            }
        }
        // (in span order, as the stations are merged)
        splits.sort_by_key(|(j, ..)| *j);
        solver.reload_surface_offsets()?;
        Ok(SkinCheck {
            splits,
            worst,
            not_finite_at,
        })
    }

    /// Step 5 of `FlushCurve`: splits every span more than `2.8` times the size of a
    /// neighbour at its middle, with the solver's section there
    /// ([`Self::solve_mid_point`]), until no span is or the knots have tripled.
    ///
    /// # Errors
    ///
    /// Those of [`Self::solve_mid_point`]; `InvalidInput` for more knots than points.
    fn equalise_spans<S: FilletSolverOps<FilletGeom = G>>(
        &self,
        solver: &mut S,
        k: &Kernel,
        pnts: &mut Vec<TsectPnt>,
        knots: &mut Vec<f64>,
    ) -> Result<()> {
        let mut done = false;
        let maximum_delta_factor = 2.8;
        let orig_num_knots = knots.len();

        // continue to iter until spans are uniformly sized
        while !done {
            done = true;

            // end condition: if knot count is more than tripled
            if knots.len() as f64 > 3.0 * orig_num_knots as f64 {
                break;
            }

            // iter every span
            let mut j = 0usize;
            while j + 1 < knots.len() {
                let kj = knots.get(j).copied().unwrap_or(0.0);
                let kj1 = knots.get(j + 1).copied().unwrap_or(0.0);

                // see if this span should be split
                let mut split = false;
                let span_size = kj1 - kj;

                if j > 0 {
                    // Look at size of previous span
                    let last_span_size = kj - knots.get(j - 1).copied().unwrap_or(0.0);
                    if span_size / last_span_size > maximum_delta_factor {
                        split = true;
                    }
                }

                if j < knots.len() - 2 {
                    // Look at size of next span
                    let next_span_size = knots.get(j + 2).copied().unwrap_or(0.0) - kj1;
                    if span_size / next_span_size > maximum_delta_factor {
                        split = true;
                    }
                }

                if !split {
                    j += 1;
                    continue;
                }

                // Split the span at the midpoint. [Fillet reg 2:229]
                let frac = 0.5;

                // add a mid-span knot value to knot array
                done = false;
                let split_knot = (1.0 - frac) * kj + frac * kj1;
                knots.insert(j + 1, split_knot);

                // 5a. Find 3D intersection point for new knot value
                let (pj, pj1) = match (pnts.get(j), pnts.get(j + 1)) {
                    (Some(a), Some(b)) => (a.clone(), b.clone()),
                    _ => {
                        return Err(SolidError::invalid_input(
                            OP_FLUSH,
                            "more knots than curve points",
                        ))
                    }
                };
                let mid_pnt = self.solve_mid_point(solver, k, &pj, &pj1, frac)?;

                // add the completed mid-span point to the sPnts array
                pnts.insert(j + 1, mid_pnt);
                j += 2;
            }
        }
        Ok(())
    }

    /// Step 5a of `FlushCurve`: the solver's section at share `frac` of the span from
    /// `pj` to `pj1`, on the plane through the interpolated point square to its
    /// interpolated tangent, with its rail values.
    ///
    /// # Errors
    ///
    /// `SM_ERR` when the plane has no solution; those of the solver.
    fn solve_mid_point<S: FilletSolverOps<FilletGeom = G>>(
        &self,
        solver: &mut S,
        k: &Kernel,
        pj: &TsectPnt,
        pj1: &TsectPnt,
        frac: f64,
    ) -> Result<TsectPnt> {
        self.solve_mid_point_from(solver, k, pj, pj1, frac, None)?
            .ok_or_else(|| SolidError::blend_failed(OP_FLUSH, "no mid-point section was found"))
    }

    /// [`Self::solve_mid_point`], its plane solve started from `start`'s surface
    /// parameters instead of the interpolated guess's (not upstream, #15256: the skin
    /// check retries from each end station), and `None` where the plane has no solution.
    ///
    /// # Errors
    ///
    /// Those of the solver.
    fn solve_mid_point_from<S: FilletSolverOps<FilletGeom = G>>(
        &self,
        solver: &mut S,
        k: &Kernel,
        pj: &TsectPnt,
        pj1: &TsectPnt,
        frac: f64,
        start: Option<&TsectPnt>,
    ) -> Result<Option<TsectPnt>> {
        let uv_domains = self.base.base.uv_domains;
        let curve_trace_direction = self.base.base.curve_trace_direction;
        let mut mid_pnt = TsectPnt::new();
        find_guess_mid_tsect_pnt(pj, pj1, frac, &mut mid_pnt);

        let plane_orig = mid_pnt.crv_pos();
        let guess = start.unwrap_or(&mid_pnt);
        let uv1 = guess.uv_pos(SrfIndex::First);
        let uv2 = guess.uv_pos(SrfIndex::Second);
        let plane_normal = mid_pnt.crv_deriv().unitize()?;

        // find filletPoint on given plane satisfying geometry
        //   requirements implemented in derived SmFilletSolver class
        let found_solution = solver.point_on_plane_solve(
            k,
            plane_orig,
            plane_normal,
            &uv_domains[0],
            &uv_domains[1],
            uv1,
            uv2,
            &mut mid_pnt,
        )?;
        if !found_solution {
            return Ok(None);
        }

        // save the rail UVCurve points for the new mid-span point
        let uvs = [
            mid_pnt.uv_pos(SrfIndex::First),
            mid_pnt.uv_pos(SrfIndex::Second),
        ];

        // get the rail 3DCurve sample values
        solver.compute_point_values(
            k,
            uvs,
            &uv_domains[0],
            &uv_domains[1],
            curve_trace_direction,
            &mut mid_pnt,
            Some(pj),
        )?;

        Ok(Some(mid_pnt))
    }

    /// Steps 5b to 10 of `FlushCurve` over the stations `pnts` at `knots`: the centre
    /// line, the rails and their UV curves, smoothed by a smoothing self-intersection
    /// handler, and the fillet surface (with the pieces a round whose law has corners
    /// keeps); the curve's type and deviation are appended to `out`. Leaves the solver's
    /// offsets at zero.
    ///
    /// # Errors
    ///
    /// As `FlushCurve`'s steps.
    #[allow(clippy::too_many_lines)]
    fn build_flush_geometry<S: FilletSolverOps<FilletGeom = G>>(
        &self,
        solver: &mut S,
        k: &Kernel,
        pseudo: &mut FilletBrep,
        pnts: &[TsectPnt],
        knots: &[f64],
        out: &mut TsectCurves,
    ) -> Result<FlushGeometry<G>> {
        let uv_domains = self.base.base.uv_domains;
        let mut ctrl_pts_3d: Vec<Point3> = Vec::new();
        let mut uv_ctrl_pts: [Vec<Point3>; 2] = [Vec::new(), Vec::new()];
        let mut rail_ctrl_pts_3d: [Vec<Point3>; 2] = [Vec::new(), Vec::new()];

        let mut curve_type = TsectCurveType::Crossing;
        let mut max_deviation = 0.0;
        let mut max_angle = 0.0;

        // 5b. SolidCraft (the fork's 6eecf04): where the fillet radius has a corner, the
        //     center curve and rails turn. A point there carries the average of the
        //     tangents of the two sides; the span that ends there takes the tangents from
        //     before the corner (`incoming`), the span that starts there those from after
        //     it, so the curves keep the turn.
        let mut incoming: Vec<Option<TsectPnt>> = Vec::new();
        let mut pnts = pnts.to_vec();
        let count = pnts.len();
        for (kk, slot) in pnts.iter_mut().enumerate() {
            let mut point_in = None;
            if kk > 0 && kk + 1 < count && solver.is_at_radius_corner(slot) {
                let mut point_out = slot.clone();
                let mut before = slot.clone();
                let b_in = one_sided_tangents(solver, k, &uv_domains, -1.0, &mut before)?;
                let b_out = one_sided_tangents(solver, k, &uv_domains, 1.0, &mut point_out)?;
                if b_in && b_out {
                    point_in = Some(before);
                    take_tangents(slot, &point_out);
                }
            }
            incoming.push(point_in);
        }

        // set the stored fillet-surface OffsetDistances to 0.0
        solver.set_surfaces_to_zero_offset()?;

        // 6. for every offset-surface xsect curve sample point span
        // build center-line, rail UVCurves, and rail 3DCurves control-point arrays
        // all with the same parameterization
        for (i, pair) in pnts.windows(2).enumerate() {
            let [tsp_last, tsp_curr] = pair else {
                continue;
            };
            // the end tangents of this span (the incoming ones at a radius corner)
            let tsp_end = incoming
                .get(i + 1)
                .and_then(Option::as_ref)
                .unwrap_or(tsp_curr);

            // save max angle between offset surfaces along xsect curve sample points
            max_angle = max2(max_angle, tsp_curr.tangent_plane_angle_rad);

            // If we have a tangent curve set that curve type
            if i == 0 && tsp_curr.point_type == IntersectionPointType::TangentCurve {
                curve_type = TsectCurveType::Tangent;
            }

            // Collect maximum deviation
            if tsp_last.deviation > max_deviation {
                max_deviation = tsp_last.deviation;
            }
            if tsp_curr.deviation > max_deviation {
                max_deviation = tsp_curr.deviation;
            }

            // let dScale = span parameter length
            let scale = tsp_curr.curve_parameter - tsp_last.curve_parameter;

            // for the CenterLine
            let p1 = tsp_last.crv_pos();
            let p4 = tsp_curr.crv_pos();
            let d1 = tsp_last.crv_deriv() * scale;
            let d2 = tsp_end.crv_deriv() * scale;

            // Hermite to Bezier
            let p2 = p1 + d1 / 3.0;
            let p3 = p4 - d2 / 3.0;

            // for the first span - add the start control point to the s3DCtrlPts array
            if ctrl_pts_3d.is_empty() {
                ctrl_pts_3d.push(p1);
            }

            // now add the span's last 3 control points to the s3DCtrlPts array
            ctrl_pts_3d.push(p2);
            ctrl_pts_3d.push(p3);
            ctrl_pts_3d.push(p4);

            // for both surface rail curves - build the rail curves
            for (rail, s) in SRFS.into_iter().enumerate() {
                let uv1 = tsp_last.uv_pos(s);
                let uv4 = tsp_curr.uv_pos(s);
                let der1 = tsp_last.uv_deriv(s) * scale;
                let der2 = tsp_end.uv_deriv(s) * scale;

                // Convert from Hermite to Bezier.
                let uv2 = uv1 + der1 / 3.0;
                let uv3 = uv4 - der2 / 3.0;

                let (Some(uv_pts), Some(pts_3d)) =
                    (uv_ctrl_pts.get_mut(rail), rail_ctrl_pts_3d.get_mut(rail))
                else {
                    continue;
                };

                // for the first span - add the start control point
                if uv_pts.is_empty() {
                    uv_pts.push(Vec3::from(uv1));
                }

                // now add the span's last 3 control points
                uv_pts.push(Vec3::from(uv2));
                uv_pts.push(Vec3::from(uv3));
                uv_pts.push(Vec3::from(uv4));

                // Now lift the point and derivatives of UV curves into 3D on the zero offset surface.
                let (pnt, du, dv) =
                    solver.rail_surface_evaluate_1st_derivatives(k, rail, tsp_last.uv_pos(s))?;
                let p13d = pnt;
                let d13d = du * der1.x + dv * der1.y;

                let (pnt, du, dv) =
                    solver.rail_surface_evaluate_1st_derivatives(k, rail, tsp_curr.uv_pos(s))?;
                let p43d = pnt;
                let d23d = du * der2.x + dv * der2.y;

                // Convert from Hermite to Bezier.
                let p23d = p13d + d13d / 3.0;
                let p33d = p43d - d23d / 3.0;
                if pts_3d.is_empty() {
                    pts_3d.push(p13d);
                }

                pts_3d.push(p23d);
                pts_3d.push(p33d);
                pts_3d.push(p43d);
            }
        }

        // 7. convert control-point/knot Arrays into SmBsplineCurves
        let mut curves_3d: Vec<BSplineCurve> = Vec::new();
        let mut surface1_uv_curves: Vec<BSplineCurve> = Vec::new();
        let mut surface2_uv_curves: Vec<BSplineCurve> = Vec::new();
        let [uv_ctrl_pts0, uv_ctrl_pts1] = &uv_ctrl_pts;
        let [rail_pts0, rail_pts1] = &rail_ctrl_pts_3d;
        flush_crv(3, &ctrl_pts_3d, knots, &mut curves_3d)?;
        flush_crv(2, uv_ctrl_pts0, knots, &mut surface1_uv_curves)?;
        flush_crv(2, uv_ctrl_pts1, knots, &mut surface2_uv_curves)?;
        flush_crv(3, rail_pts0, knots, &mut curves_3d)?;
        flush_crv(3, rail_pts1, knots, &mut curves_3d)?;
        out.deviations.push(max_deviation);
        out.curve_types.push(curve_type);

        // 8. Select target pFilletGeom to Modify
        let fillet_geom = match &self.curr_fillet_geom {
            Some(g) => g.clone(),
            None => solver.last_fillet_geom()?,
        };

        // 9. when self-intersection checking/correcting is on
        if let Some(algorithm) = solver.self_intersection_handler()? {
            if algorithm == HandlingAlgorithmType::Smoothing {
                let smoothed = solver.smooth_out_intersection(
                    k,
                    pseudo,
                    &fillet_geom,
                    &mut curves_3d,
                    &mut surface1_uv_curves,
                    &mut surface2_uv_curves,
                );
                if keep_unported(smoothed)?.is_none() {
                    solver.set_status(FilStatus::SelfIntHandlingFailure)?;
                    return Err(SolidError::blend_failed(
                        OP_FLUSH,
                        "the self-intersecting fillet could not be smoothed",
                    ));
                }
            }
        }

        // get (the potentially modified) rail curves, their UVTrimCurves, and the Centerline
        let mut uv1 = surface1_uv_curves
            .pop()
            .ok_or_else(|| SolidError::blend_missing(OP_FLUSH, "the first rail's UV curve"))?;
        let mut uv2 = surface2_uv_curves
            .pop()
            .ok_or_else(|| SolidError::blend_missing(OP_FLUSH, "the second rail's UV curve"))?;
        let mut curve2 = curves_3d
            .pop()
            .ok_or_else(|| SolidError::blend_missing(OP_FLUSH, "the second rail curve"))?;
        let mut curve1 = curves_3d
            .pop()
            .ok_or_else(|| SolidError::blend_missing(OP_FLUSH, "the first rail curve"))?;
        let mut center_line = curves_3d
            .pop()
            .ok_or_else(|| SolidError::blend_missing(OP_FLUSH, "the center line"))?;

        // 10. Generate the fillet surface from the rail curves and centerline
        let mut fillet_surface = None;
        let mut kept = None;
        if solver.has_surface_generator()? {
            let created = solver.create_fillet_surface(
                k,
                &mut center_line,
                &mut curve1,
                &mut curve2,
                &pnts,
                &mut uv1,
                &mut uv2,
            );
            let Some((s, pieces)) = keep_unported(created)? else {
                solver.set_status(FilStatus::SurfSkinningFailure)?;
                return Err(SolidError::blend_failed(
                    OP_FLUSH,
                    "the fillet surface could not be skinned",
                ));
            };
            fillet_surface = s;
            kept = pieces;
        }

        Ok(FlushGeometry {
            fillet_geom,
            center_line,
            curve1,
            curve2,
            uv1,
            uv2,
            fillet_surface,
            kept,
        })
    }

    /// `EvaluateLawPoint`: the solver's point on the plane through `plane_origin`
    /// with normal `plane_normal` near `uv0`, `uv1`: its curve parameter (user double
    /// 0) and UV points. The law arguments are unused upstream; whether a solution was
    /// found is not checked (a default point's values are returned).
    ///
    /// # Errors
    ///
    /// The solver's.
    #[allow(clippy::too_many_arguments)]
    pub fn evaluate_law_point<S: FilletSolverOps<FilletGeom = G>, L: ?Sized>(
        &self,
        solver: &mut S,
        k: &Kernel,
        _parameter: f64,
        uv0: Point2,
        uv1: Point2,
        _law: &L,
        _law_orient: bool,
        _point_curve_interval: &Extent1d,
        _surface_orientations: [f64; 2],
        plane_origin: Point3,
        plane_normal: Vec3,
    ) -> Result<(f64, [Point2; 2])> {
        let si = &self.base.base;
        let mut tmp_pnt = TsectPnt::new();

        // find filletPoint on given plane
        let _found_solution = solver.point_on_plane_solve(
            k,
            plane_origin,
            plane_normal,
            &si.uv_domains[0],
            &si.uv_domains[1],
            uv0,
            uv1,
            &mut tmp_pnt,
        )?;

        // set output
        let uvs = [
            tmp_pnt.uv_pos(SrfIndex::First),
            tmp_pnt.uv_pos(SrfIndex::Second),
        ];
        let curve_param = tmp_pnt.user_doubles.first().copied().unwrap_or(0.0);
        Ok((curve_param, uvs))
    }

    /// `DoPointIntersection` (the `SmSurfaceIntersector` method, not overridden) run
    /// on this intersector: the base driver of `intersect::surface_intersector::p4`
    /// with its virtual calls dispatched to this class's overrides ([`FilletDispatch`]):
    /// `ComputePointValues` and `ComputeNextPoint` step through `solver`, and
    /// `FlushCurve` stores the traced curves in the current (else the solver's last)
    /// fillet geometry of `pseudo`.
    ///
    /// # Errors
    ///
    /// As the base `DoPointIntersection`, with the errors of the overrides.
    pub fn do_point_intersection<S: FilletSolverOps<FilletGeom = G>>(
        &mut self,
        solver: &mut S,
        pseudo: &mut FilletBrep,
        k: &Kernel,
        points: &[TsectId],
        opts: &PointIntersectionOptions,
    ) -> Result<IntersectionCurve> {
        do_point_intersection_with(
            &mut FilletDispatch {
                intersector: self,
                solver,
                pseudo,
            },
            k,
            points,
            opts,
        )
    }

    /// `DoLawIntersection` (the `SmSurfaceIntersector` method, not overridden) run on
    /// this intersector: the base driver of `intersect::surface_intersector::p4` with
    /// its virtual calls dispatched to this class's overrides ([`FilletDispatch`]):
    /// `EvaluateLawPoint` solves the solver's point on each section plane (the law is
    /// not evaluated), `ComputePointValuesDelta` and `TestSpanAccuracy` evaluate
    /// through `solver`, and `FlushCurve` stores the traced curve in the current (else
    /// the solver's last) fillet geometry of `pseudo`.
    ///
    /// # Errors
    ///
    /// As the base `DoLawIntersection`, with the errors of the overrides.
    pub fn do_law_intersection<S: FilletSolverOps<FilletGeom = G>>(
        &mut self,
        solver: &mut S,
        pseudo: &mut FilletBrep,
        k: &Kernel,
        points: &[TsectId],
        opts: &LawIntersectionOptions<'_>,
        law: &mut FilletLaw,
    ) -> Result<IntersectionCurve> {
        do_law_intersection_with(
            &mut FilletDispatch {
                intersector: self,
                solver,
                pseudo,
            },
            k,
            points,
            opts,
            law,
        )
    }
}

/// `SmFilletIntersector` seen through its `SmSurfaceIntersector` virtuals: the fillet
/// intersector, the solver its overrides take as an argument and the pseudo B-rep
/// `FlushCurve` writes the fillet geometry into. The base drivers
/// (`DoPointIntersection`, `TraceCurve`, `TestPoint`) run on it as upstream runs them
/// on `*this`.
#[derive(Debug)]
pub struct FilletDispatch<'a, 's, 't, O, G, S> {
    /// The fillet intersector.
    pub intersector: &'a mut FilletIntersector<'s, 't, O, G>,
    /// `m_rFilletSolver`.
    pub solver: &'a mut S,
    /// The pseudo B-rep holding the solver's fillet geometries.
    pub pseudo: &'a mut FilletBrep,
}

impl<'s, 't, O, G, S> IntersectorVirtuals<'s> for FilletDispatch<'_, 's, 't, O, G, S>
where
    O: SolutionObject + 't,
    G: Clone,
    S: FilletSolverOps<FilletGeom = G>,
{
    fn intersector(&self) -> &SurfaceIntersector<'s> {
        &self.intersector.base.base
    }

    fn intersector_mut(&mut self) -> &mut SurfaceIntersector<'s> {
        &mut self.intersector.base.base
    }

    /// [`FilletIntersector::compute_point_values`].
    fn compute_point_values(
        &mut self,
        k: &Kernel,
        uv_values: [Point2; 2],
        point: &mut TsectPnt,
        previous: Option<&TsectPnt>,
        step_size: Option<f64>,
    ) -> Result<()> {
        self.intersector
            .compute_point_values(self.solver, k, uv_values, point, previous, step_size)
    }

    /// [`FilletIntersector::compute_next_point`].
    fn compute_next_point(
        &mut self,
        k: &Kernel,
        prev: TsectId,
        step_size: f64,
        next: TsectId,
    ) -> Result<NextPoint> {
        self.intersector
            .compute_next_point(self.solver, k, prev, step_size, next)
    }

    /// [`FilletIntersector::flush_curve`].
    fn flush_curve(&mut self, k: &Kernel, out: &mut TsectCurves) -> Result<()> {
        self.intersector
            .flush_curve(self.solver, k, self.pseudo, out)
    }

    /// [`FilletIntersector::evaluate_law_point`] (the law arguments are unused).
    fn evaluate_law_point(
        &mut self,
        k: &Kernel,
        parameter: f64,
        uv0: Point2,
        uv1: Point2,
        law: &mut FilletLaw,
        law_orient: bool,
        point_curve_interval: &Extent1d,
        surface_orientations: [f64; 2],
        plane_origin: Point3,
        plane_normal: Vec3,
    ) -> Result<(Option<f64>, [Point2; 2])> {
        let (curve_param, uvs) = self.intersector.evaluate_law_point(
            self.solver,
            k,
            parameter,
            uv0,
            uv1,
            &*law,
            law_orient,
            point_curve_interval,
            surface_orientations,
            plane_origin,
            plane_normal,
        )?;
        Ok((Some(curve_param), uvs))
    }

    /// [`FilletIntersector::compute_point_values_delta`].
    fn compute_point_values_delta(
        &mut self,
        k: &Kernel,
        uv_values: [Point2; 2],
        uv_values_delta: [Point2; 2],
        point: &mut TsectPnt,
        delta: &mut TsectPnt,
    ) -> Result<()> {
        self.intersector.compute_point_values_delta(
            self.solver,
            k,
            uv_values,
            uv_values_delta,
            point,
            delta,
        )
    }

    /// [`FilletIntersector::test_span_accuracy`].
    fn test_span_accuracy(
        &mut self,
        k: &Kernel,
        point: TsectId,
        next: TsectId,
        mid: Option<TsectId>,
    ) -> Result<SpanAccuracy> {
        self.intersector
            .test_span_accuracy(self.solver, k, point, next, mid)
    }
}

/// The 3D Hermite rail span of a UV span on its surface: the end points and
/// derivatives of the mapped curve; with `adjust`, the derivatives taken at the
/// interior parameters `0.001` and `0.999` instead.
fn rail_hermite(k: &Kernel, map: &CrvOnSurf, adjust: bool) -> Result<HermiteCurve> {
    let ivl = map.natural_interval();
    let [p0, mut d0] = eval1(k, map, ivl.min())?;
    let [p1, mut d1] = eval1(k, map, ivl.max())?;

    // If we had to find the uv curve derivs by divided differences (above),
    // set 3d curve derivs from nearby internal points.
    if adjust {
        let [_, da] = eval1(k, map, ivl.evaluate(0.001))?;
        let [_, db] = eval1(k, map, ivl.evaluate(0.999))?;
        d0 = da;
        d1 = db;
    }

    Ok(HermiteCurve::new(p0, d0, p1, d1, 3))
}

/// `sm_OneSidedTangents` (the fork's 6eecf04, not upstream): `point`'s tangents from one
/// side only, from a small step (`1e-5`, as `ComputePointValues` steps) backward
/// (`side < 0`) or forward along the trace: the direction to the stepped point's centre
/// (the middle of its two contacts on the solver's offset surfaces, evaluated before
/// anything resets the offsets), then `ComputeUVVectors`. Where the radius has a corner
/// the curves turn, and `ComputePointValues`' central difference averages the two sides.
/// `false` (and `point` unchanged) when the step finds no point or a point on top of this
/// one.
///
/// The contacts are read with `Evaluate1stDerivatives` (the fork: `EvaluatePoint`), the one
/// evaluation of the offset surfaces the intersector's solver interface offers.
///
/// # Errors
///
/// Those of `StepSolve`, the evaluations, `Unitize` and `ComputeUVVectors`.
fn one_sided_tangents<S: FilletSolverOps>(
    solver: &mut S,
    k: &Kernel,
    uv_domains: &[Extent2d; 2],
    side: f64,
    point: &mut TsectPnt,
) -> Result<bool> {
    let mut d_step = if side < 0.0 { -1.0e-5 } else { 1.0e-5 };
    let mut stepped = TsectPnt::new();
    let mut flags = StepSolve::default();
    solver.step_solve(
        k,
        point,
        &mut d_step,
        &uv_domains[0],
        &uv_domains[1],
        &mut flags,
        &mut stepped,
    )?;
    if !flags.found_answer {
        return Ok(false);
    }

    // the stepped point on the center curve, evaluated before anything resets the offsets
    let (p0, _, _) =
        solver.rail_surface_evaluate_1st_derivatives(k, 0, stepped.uv_pos(SrfIndex::First))?;
    let (p1, _, _) =
        solver.rail_surface_evaluate_1st_derivatives(k, 1, stepped.uv_pos(SrfIndex::Second))?;
    let mut diff = (p0 + p1) / 2.0 - point.crv_pos();
    if side < 0.0 {
        diff = -diff;
    }
    if diff.length_squared() < EFF_ZERO_SQ {
        return Ok(false);
    }
    *point.crv_deriv_mut() = diff.unitize()?;
    solver.compute_uv_vectors(k, point)?;
    Ok(true)
}

/// The fork's copy of a point's curve and uv tangents (`CrvDeriv()`, `UVDeriv(0)`,
/// `UVDeriv(1)`) from `from` into `to`.
fn take_tangents(to: &mut TsectPnt, from: &TsectPnt) {
    *to.crv_deriv_mut() = from.crv_deriv();
    for s in SRFS {
        *to.uv_deriv_mut(s) = from.uv_deriv(s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blend::fillet_custom_solver::{OffsetSurface, SurfaceId};
    use crate::intersect::adv_surface_intersector::NoSurfaceHandle;
    use crate::nurbs::b_spline_surface::BSplineSurface;
    use crate::surface::offset_surface::SS_UNKNOWN;

    fn bilinear(p: [[f64; 3]; 4]) -> Surface {
        let [a, b, c, d] = p.map(Vec3::from);
        Surface::from(BSplineSurface::create_bilinear_surface(a, b, c, d).unwrap())
    }

    fn solver(a: &Surface, b: &Surface) -> ConstantRadiusAssistedFs {
        let k = Kernel::default();
        ConstantRadiusAssistedFs::from_parts(
            0.001,
            0.5,
            0.03,
            [1.0, 2.0],
            [
                Some(
                    OffsetSurface::new(&k, 1.0, a.clone(), SurfaceId::Free(0), SS_UNKNOWN).unwrap(),
                ),
                Some(
                    OffsetSurface::new(&k, -2.0, b.clone(), SurfaceId::Free(1), SS_UNKNOWN)
                        .unwrap(),
                ),
            ],
            [1.0, -1.0],
            [None, None],
        )
    }

    fn pnt(uv0: [f64; 2], uv1: [f64; 2], d: [f64; 3], t: f64) -> TsectPnt {
        let mut p = TsectPnt::new();
        p.point_type = IntersectionPointType::Crossing;
        *p.uv_pos_mut(SrfIndex::First) = Point2::new(uv0[0], uv0[1]);
        *p.uv_pos_mut(SrfIndex::Second) = Point2::new(uv1[0], uv1[1]);
        *p.uv_deriv_mut(SrfIndex::First) = Vec2::new(1.0, 0.0);
        *p.uv_deriv_mut(SrfIndex::Second) = Vec2::new(1.0, 0.0);
        *p.crv_pos_mut() = Vec3::new(uv0[0], 0.0, 0.0);
        *p.crv_deriv_mut() = Vec3::from(d);
        p.curve_parameter = t;
        p
    }

    fn surfaces() -> (Surface, Surface) {
        (
            bilinear([
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [1.0, 1.0, 0.0],
            ]),
            bilinear([
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                [1.0, 0.0, 0.0],
                [1.0, 0.0, 1.0],
            ]),
        )
    }

    #[test]
    fn construction_and_flags() {
        let k = &Kernel::default();
        let (a, b) = surfaces();
        let d = Extent2d::from_bounds(-1.0, -1.0, 2.0, 2.0);
        let mut fi: FilletIntersector<'_, '_, NoSurfaceHandle, ()> =
            FilletIntersector::new(k, &a, &d, &b, &d, None).unwrap();
        assert!(!fi.base.base.do_boundary_point_on_curve_test);
        // the fillet flag keeps domains beyond the natural ones
        assert_eq!(fi.base.base.uv_domains[0], d);
        assert!(fi.is_kind_of(TYPE));
        assert!(!fi.is_kind_of(7));
        assert!(!fi.self_intersect());
        fi.set_self_intersect(true);
        assert!(fi.self_intersect());
        assert!(!fi.is_curr_fillet_touch_boundary());
        assert!(fi.curr_fillet_geom().is_none());
    }

    #[test]
    fn span_rejects_large_turn_and_keeps_offsets() {
        let k = &Kernel::default();
        let (a, b) = surfaces();
        let d = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let mut s = solver(&a, &b);
        let mut fi: FilletIntersector<'_, '_, NoSurfaceHandle, FilletGeomId> =
            FilletIntersector::new(k, &a, &d, &b, &d, None).unwrap();
        let p = fi
            .base
            .base
            .alloc_point(pnt([0.1, 0.0], [0.1, 0.0], [1.0, 0.0, 0.0], 0.0));
        let mut n = pnt([0.2, 0.0], [0.2, 0.0], [0.0, 1.0, 0.0], 0.1);
        n.point_type = IntersectionPointType::Singularity;
        let n = fi.base.base.alloc_point(n);
        let r = fi
            .test_span_accuracy(&mut s, &Kernel::default(), p, n, None)
            .unwrap();
        assert!(!r.found_good_point);
        assert!((r.angle_found_rad - core::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert_eq!(
            fi.base.base.point(n).unwrap().point_type,
            IntersectionPointType::Crossing
        );
        // a zero next tangent is SM_ERR: a typed blend failure with the status of the bare
        // `Upstream` it replaced (#13790)
        let z = fi
            .base
            .base
            .alloc_point(pnt([0.2, 0.0], [0.2, 0.0], [0.0, 0.0, 0.0], 0.1));
        let e = fi
            .test_span_accuracy(&mut s, &Kernel::default(), p, z, None)
            .unwrap_err();
        assert_eq!(
            e,
            SolidError::blend_failed(OP_SPAN, "the next section's curve derivative is zero")
        );
        assert_eq!(e.sm_status(), Some(crate::error::SM_ERR));
        assert_eq!(e.upstream_status(), Some(crate::error::SM_ERR));
        assert_eq!(s.surface(0).unwrap().offset_distance(), 1.0);
    }

    #[test]
    fn span_accepts_straight_line_on_planes() {
        let k = &Kernel::default();
        let (a, b) = surfaces();
        let d = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let mut s = solver(&a, &b);
        let mut fi: FilletIntersector<'_, '_, NoSurfaceHandle, FilletGeomId> =
            FilletIntersector::new(k, &a, &d, &b, &d, None).unwrap();
        let p = fi
            .base
            .base
            .alloc_point(pnt([0.1, 0.0], [0.1, 0.0], [1.0, 0.0, 0.0], 0.0));
        let n = fi
            .base
            .base
            .alloc_point(pnt([0.2, 0.0], [0.2, 0.0], [1.0, 0.0, 0.0], 0.1));
        let m = fi
            .base
            .base
            .alloc_point(pnt([0.15, 0.0], [0.15, 0.0], [1.0, 0.0, 0.0], 0.05));
        let r = fi
            .test_span_accuracy(&mut s, &Kernel::default(), p, n, Some(m))
            .unwrap();
        assert!(r.found_good_point);
        assert!(r.deviation_found.is_none());
        // offsets restored
        assert_eq!(s.surface(0).unwrap().offset_distance(), 1.0);
        assert_eq!(s.surface(1).unwrap().offset_distance(), -2.0);
        assert_eq!(s.solver.saved_offsets, [1.0, -2.0]);
    }

    #[test]
    fn next_point_needs_a_crossing() {
        let k = &Kernel::default();
        let (a, b) = surfaces();
        let d = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let mut s = solver(&a, &b);
        let mut fi: FilletIntersector<'_, '_, NoSurfaceHandle, FilletGeomId> =
            FilletIntersector::new(k, &a, &d, &b, &d, None).unwrap();
        let mut t = pnt([0.1, 0.0], [0.1, 0.0], [1.0, 0.0, 0.0], 0.0);
        t.point_type = IntersectionPointType::TangentCurve;
        let p = fi.base.base.alloc_point(t);
        let n = fi.base.base.alloc_point(TsectPnt::new());
        let r = fi
            .compute_next_point(&mut s, &Kernel::default(), p, 0.1, n)
            .unwrap();
        assert!(!r.found_good_point && !r.clipped && !r.boundary_hit);
        // aliased non-crossing: no point, as upstream
        let r = fi
            .compute_next_point(&mut s, &Kernel::default(), p, 0.1, p)
            .unwrap();
        assert!(!r.found_good_point && !r.clipped && !r.boundary_hit);
        // a crossing reaches StepSolve, whose Newton solve (through the
        // SmConstantRadiusFS::LoadInitialValues override, blend.fillet_standard_solver_p2)
        // finds no point on these surfaces
        let c = fi
            .base
            .base
            .alloc_point(pnt([0.1, 0.0], [0.1, 0.0], [1.0, 0.0, 0.0], 0.0));
        let r = fi
            .compute_next_point(&mut s, &Kernel::default(), c, 0.1, n)
            .unwrap();
        assert!(
            !r.found_good_point && !r.clipped && !r.boundary_hit,
            "{r:?}"
        );
    }

    /// A solver whose `StepSolve` clips the step to `clip` (rescaling it, as
    /// upstream's i/o `rdStep`) and optionally fails after setting `rbClipped`.
    struct ClipSolver {
        clip: f64,
        fail: bool,
        /// `Some(l)`: the step is not clipped, keeps its size and lands `l` along the tangent.
        unclipped_landing: Option<f64>,
    }

    impl FilletSolverOps for ClipSolver {
        type FilletGeom = ();

        fn point_on_plane_solve(
            &mut self,
            _k: &Kernel,
            _plane_origin: Point3,
            _plane_normal: Vec3,
            _uv_domain1: &Extent2d,
            _uv_domain2: &Extent2d,
            _uv1: Point2,
            _uv2: Point2,
            _point: &mut TsectPnt,
        ) -> Result<bool> {
            Err(SolidError::sm_err("test"))
        }

        fn compute_point_values(
            &mut self,
            _k: &Kernel,
            uv_values: [Point2; 2],
            _uv_domain1: &Extent2d,
            _uv_domain2: &Extent2d,
            _curve_trace_direction: f64,
            point: &mut TsectPnt,
            _previous: Option<&TsectPnt>,
        ) -> Result<()> {
            point.point_type = IntersectionPointType::Crossing;
            *point.uv_pos_mut(SrfIndex::First) = uv_values[0];
            *point.uv_pos_mut(SrfIndex::Second) = uv_values[1];
            *point.uv_deriv_mut(SrfIndex::First) = Vec2::new(1.0, 0.0);
            *point.uv_deriv_mut(SrfIndex::Second) = Vec2::new(1.0, 0.0);
            *point.crv_pos_mut() = Vec3::new(uv_values[0].x, 0.0, 0.0);
            *point.crv_deriv_mut() = Vec3::new(1.0, 0.0, 0.0);
            Ok(())
        }

        fn step_solve(
            &mut self,
            _k: &Kernel,
            point: &TsectPnt,
            step_size: &mut f64,
            _uv_domain1: &Extent2d,
            _uv_domain2: &Extent2d,
            flags: &mut StepSolve,
            next: &mut TsectPnt,
        ) -> Result<()> {
            *flags = StepSolve::default();
            flags.clipped = self.unclipped_landing.is_none();
            if self.fail {
                return Err(SolidError::sm_err("test"));
            }
            if let Some(landing) = self.unclipped_landing {
                for s in SRFS {
                    *next.uv_pos_mut(s) = point.uv_pos(s) + point.uv_deriv(s) * landing;
                }
                flags.found_answer = true;
                return Ok(());
            }
            *step_size = self.clip;
            for s in SRFS {
                *next.uv_pos_mut(s) = point.uv_pos(s) + point.uv_deriv(s) * self.clip;
            }
            flags.found_answer = true;
            Ok(())
        }

        fn compute_surface_values(
            &mut self,
            _k: &Kernel,
            _uv_values: [Point2; 2],
            _point: &mut TsectPnt,
        ) -> Result<()> {
            Ok(())
        }

        fn compute_uv_vectors(&mut self, _k: &Kernel, _point: &mut TsectPnt) -> Result<()> {
            Ok(())
        }

        fn set_surfaces_to_zero_offset(&mut self) -> Result<()> {
            Ok(())
        }

        fn reload_surface_offsets(&mut self) -> Result<()> {
            Ok(())
        }

        fn offset_radii_can_change(&self) -> Result<bool> {
            Ok(false)
        }

        fn rail_surface_evaluate_1st_derivatives(
            &mut self,
            _k: &Kernel,
            _rail: usize,
            _uv: Point2,
        ) -> Result<(Point3, Vec3, Vec3)> {
            Ok((Vec3::ZERO, Vec3::ZERO, Vec3::ZERO))
        }

        fn set_status(&mut self, _status: FilStatus) -> Result<()> {
            Ok(())
        }

        fn last_fillet_geom(&self) -> Result<()> {
            Ok(())
        }

        fn self_intersection_handler(&self) -> Result<Option<HandlingAlgorithmType>> {
            Ok(None)
        }

        fn smooth_out_intersection(
            &mut self,
            _k: &Kernel,
            _pseudo: &mut FilletBrep,
            _geom: &(),
            _curves_3d: &mut Vec<BSplineCurve>,
            _uv_curves1: &mut Vec<BSplineCurve>,
            _uv_curves2: &mut Vec<BSplineCurve>,
        ) -> Result<()> {
            Ok(())
        }

        fn has_surface_generator(&self) -> Result<bool> {
            Ok(false)
        }

        fn create_fillet_surface(
            &mut self,
            _k: &Kernel,
            _center_line: &mut BSplineCurve,
            _curve1: &mut BSplineCurve,
            _curve2: &mut BSplineCurve,
            _points: &[TsectPnt],
            _uv1: &mut BSplineCurve,
            _uv2: &mut BSplineCurve,
        ) -> Result<(Option<Surface>, Option<LawCornerPieces>)> {
            Ok((None, None))
        }

        fn keep_law_corner_pieces(&mut self, _pieces: LawCornerPieces) -> Result<()> {
            Ok(())
        }

        fn skins_varying_balls(&self) -> Result<bool> {
            Ok(false)
        }

        fn set_fillet_surface(
            &mut self,
            _pseudo: &mut FilletBrep,
            _geom: &(),
            _surface: Option<Surface>,
        ) -> Result<()> {
            Ok(())
        }

        fn set_center_line_curve(
            &mut self,
            _pseudo: &mut FilletBrep,
            _geom: &(),
            _center_line: BSplineCurve,
        ) -> Result<()> {
            Ok(())
        }

        fn set_rail_curve(
            &mut self,
            _pseudo: &mut FilletBrep,
            _geom: &(),
            _rail: usize,
            _curve: BSplineCurve,
        ) -> Result<()> {
            Ok(())
        }

        fn set_rail_mate_uv_curve(
            &mut self,
            _pseudo: &mut FilletBrep,
            _geom: &(),
            _rail: usize,
            _uv_curve: BSplineCurve,
        ) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn next_point_uses_the_step_size_step_solve_returns() {
        let k = &Kernel::default();
        let (a, b) = surfaces();
        let d = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let mut fi: FilletIntersector<'_, '_, NoSurfaceHandle, ()> =
            FilletIntersector::new(k, &a, &d, &b, &d, None).unwrap();
        let p = fi
            .base
            .base
            .alloc_point(pnt([0.1, 0.0], [0.1, 0.0], [1.0, 0.0, 0.0], 0.0));
        let n = fi.base.base.alloc_point(TsectPnt::new());
        let mut s = ClipSolver {
            clip: 0.2,
            fail: false,
            unclipped_landing: None,
        };
        let r = fi
            .compute_next_point(&mut s, &Kernel::default(), p, 0.5, n)
            .unwrap();
        assert!(r.found_good_point && r.clipped && !r.boundary_hit);
        // rNextTSP.m_dCurveParameter = rTsectPnt.m_dCurveParameter + (clipped) dStepSize
        assert_eq!(fi.base.base.point(n).unwrap().curve_parameter, 0.2);

        // a failing StepSolve keeps the flags it set
        let mut s = ClipSolver {
            clip: 0.2,
            fail: true,
            unclipped_landing: None,
        };
        let r = fi
            .compute_next_point(&mut s, &Kernel::default(), p, 0.5, n)
            .unwrap();
        assert!(!r.found_good_point && r.clipped);
    }

    #[test]
    fn wr503_next_point_rejects_unclipped_short_step() {
        let k = &Kernel::default();
        let (a, b) = surfaces();
        let d = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let mut fi: FilletIntersector<'_, '_, NoSurfaceHandle, ()> =
            FilletIntersector::new(k, &a, &d, &b, &d, None).unwrap();
        let p = fi
            .base
            .base
            .alloc_point(pnt([0.1, 0.0], [0.1, 0.0], [1.0, 0.0, 0.0], 0.0));
        let n = fi.base.base.alloc_point(TsectPnt::new());

        // an unclipped step of 0.5 that lands a fifth of the step away is another root of the
        // equations, not the next point of this trace
        let mut s = ClipSolver {
            clip: 0.2,
            fail: false,
            unclipped_landing: Some(0.1),
        };
        let r = fi.compute_next_point(&mut s, k, p, 0.5, n).unwrap();
        assert!(!r.found_good_point && !r.clipped && !r.boundary_hit);

        // an unclipped step landing at nine tenths of the step is accepted
        let mut s = ClipSolver {
            clip: 0.2,
            fail: false,
            unclipped_landing: Some(0.45),
        };
        let r = fi.compute_next_point(&mut s, k, p, 0.5, n).unwrap();
        assert!(r.found_good_point && !r.clipped && !r.boundary_hit);
        assert_eq!(fi.base.base.point(n).unwrap().curve_parameter, 0.5);

        // a clipped step is accepted however short it is
        let mut s = ClipSolver {
            clip: 0.05,
            fail: false,
            unclipped_landing: None,
        };
        let r = fi.compute_next_point(&mut s, k, p, 0.5, n).unwrap();
        assert!(r.found_good_point && r.clipped);
        assert_eq!(fi.base.base.point(n).unwrap().curve_parameter, 0.05);
    }

    #[test]
    fn next_point_aliased_non_crossing_is_no_point() {
        let k = &Kernel::default();
        let (a, b) = surfaces();
        let d = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let mut fi: FilletIntersector<'_, '_, NoSurfaceHandle, ()> =
            FilletIntersector::new(k, &a, &d, &b, &d, None).unwrap();
        let mut q = pnt([0.1, 0.0], [0.1, 0.0], [1.0, 0.0, 0.0], 0.0);
        q.point_type = IntersectionPointType::TangentPoint;
        let p = fi.base.base.alloc_point(q);
        let mut s = ClipSolver {
            clip: 0.2,
            fail: false,
            unclipped_landing: None,
        };
        let r = fi
            .compute_next_point(&mut s, &Kernel::default(), p, 0.5, p)
            .unwrap();
        assert!(!r.found_good_point && !r.clipped && !r.boundary_hit);
        // an aliased crossing is still refused
        let c = fi
            .base
            .base
            .alloc_point(pnt([0.1, 0.0], [0.1, 0.0], [1.0, 0.0, 0.0], 0.0));
        assert!(fi
            .compute_next_point(&mut s, &Kernel::default(), c, 0.5, c)
            .is_err());
    }

    #[test]
    fn flush_curve_short_lists() {
        let k = &Kernel::default();
        let (a, b) = surfaces();
        let d = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let mut s = solver(&a, &b);
        let mut fi: FilletIntersector<'_, '_, NoSurfaceHandle, FilletGeomId> =
            FilletIntersector::new(k, &a, &d, &b, &d, None).unwrap();
        let mut out = TsectCurves::default();
        // no points: SM_ERR_NULL_POINTER
        assert!(fi
            .flush_curve(&mut s, &Kernel::default(), &mut FilletBrep::new(), &mut out)
            .is_err());
        // one point on the boundary of the first domain
        let p = fi
            .base
            .base
            .alloc_point(pnt([1.0, 0.5], [0.5, 0.5], [1.0, 0.0, 0.0], 0.0));
        fi.base.base.add_point_to_curve(p).unwrap();
        fi.flush_curve(&mut s, &Kernel::default(), &mut FilletBrep::new(), &mut out)
            .unwrap();
        assert!(fi.curr_fillet_touch_boundary());
        assert!(fi.base.base.curve_points().is_empty());
        assert!(out.curve_types.is_empty());
        // two points evaluate the offset rails, then reach the solver's
        // self-intersection handler, which is the missing executive's member
        // (a NULL dereference upstream)
        let p = fi
            .base
            .base
            .alloc_point(pnt([0.1, 0.5], [0.5, 0.5], [1.0, 0.0, 0.0], 0.0));
        let q = fi
            .base
            .base
            .alloc_point(pnt([0.2, 0.5], [0.5, 0.5], [1.0, 0.0, 0.0], 0.1));
        fi.base.base.add_point_to_curve(p).unwrap();
        fi.base.base.add_point_to_curve(q).unwrap();
        let e = fi
            .flush_curve(&mut s, &Kernel::default(), &mut FilletBrep::new(), &mut out)
            .unwrap_err();
        assert!(matches!(e, SolidError::InvalidInput { .. }), "{e:?}");
        assert!(!fi.curr_fillet_touch_boundary());
    }

    #[test]
    fn base_members_delegate_to_the_solver_base() {
        use crate::blend::fillet_solver::p1::FilletExecutiveRef;
        let (a, b) = surfaces();
        let mut s = solver(&a, &b);
        let k = Kernel::default();

        s.set_status(FilStatus::SurfSkinningFailure).unwrap();
        assert_eq!(s.solver.status(), FilStatus::SurfSkinningFailure);
        s.set_status(FilStatus::Processed).unwrap();
        assert_eq!(s.solver.status(), FilStatus::Processed);

        assert!(!s.has_surface_generator().unwrap());
        // the base constructor's geometry lives in the pseudo B-rep
        assert!(matches!(
            s.last_fillet_geom().unwrap_err(),
            SolidError::InvalidInput { .. }
        ));
        let mut pseudo = FilletBrep::new();
        let g = s
            .solver
            .add_constructor_fillet_geom(
                &k,
                &mut pseudo,
                crate::blend::fillet_geom::p1::FilletSolverId(0),
            )
            .unwrap();
        assert_eq!(s.last_fillet_geom().unwrap(), g);
        // its offset surfaces are copies of the solver's (set before the geometry here)
        assert!(pseudo
            .geom(g)
            .unwrap()
            .off_surfs
            .iter()
            .all(Option::is_some));

        // FlushCurve's writes go to the geometry in the pseudo B-rep
        let mut curves = Vec::new();
        flush_crv(
            3,
            &[
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(2.0, 0.0, 0.0),
                Point3::new(3.0, 0.0, 0.0),
            ],
            &[0.0, 1.0],
            &mut curves,
        )
        .unwrap();
        let line = curves.pop().unwrap();
        s.set_center_line_curve(&mut pseudo, &g, line.clone())
            .unwrap();
        assert!(pseudo.geom(g).unwrap().center_line_curve().is_some());
        s.set_fillet_surface(&mut pseudo, &g, None).unwrap();
        assert!(pseudo
            .geom(g)
            .unwrap()
            .fillet_surface(None)
            .unwrap()
            .is_none());
        // a geometry without rails: upstream dereferences NULL
        assert!(matches!(
            s.set_rail_curve(&mut pseudo, &g, 0, line.clone())
                .unwrap_err(),
            SolidError::InvalidInput { .. }
        ));
        assert!(matches!(
            s.set_rail_mate_uv_curve(&mut pseudo, &g, 1, line)
                .unwrap_err(),
            SolidError::InvalidInput { .. }
        ));

        // the handler is the executive's member
        assert!(matches!(
            s.self_intersection_handler().unwrap_err(),
            SolidError::InvalidInput { .. }
        ));
        s.solver.set_fillet_executive(Some(FilletExecutiveRef));
        assert_eq!(s.self_intersection_handler().unwrap(), None);
        s.solver.executive_self_intersection_handler =
            Some(crate::blend::fillet_solver::p1::MakeSurfaceBlendSih::new(1.0).algorithm_type());
        assert_eq!(
            s.self_intersection_handler().unwrap(),
            Some(HandlingAlgorithmType::Blending)
        );

        // ComputeSurfaceValues runs the SmConstantRadiusFS::SetupOffsetValues override
        // (blend.fillet_standard_solver_p1): radius times orientation on both rails
        s.solver.surfaces[1]
            .as_mut()
            .unwrap()
            .surface
            .set_offset_distance(0.0);
        let mut p = TsectPnt::new();
        s.compute_surface_values(&k, [Point2::new(0.5, 0.5); 2], &mut p)
            .expect("ported constant-radius surface values");
        assert_eq!(s.surface(0).unwrap().offset_distance(), 1.0);
        assert_eq!(s.surface(1).unwrap().offset_distance(), -2.0);
        assert_eq!(p.srf_pos(SrfIndex::First), Vec3::new(0.5, 0.5, 1.0));
        assert_eq!(p.srf_pos(SrfIndex::Second), Vec3::new(0.5, -2.0, 0.5));
        assert_eq!(p.srf_norm(SrfIndex::First), Vec3::new(0.0, 0.0, 1.0));
        assert_eq!(p.srf_norm(SrfIndex::Second), Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(p.crv_pos(), p.srf_pos(SrfIndex::First));
        assert_eq!(
            crate::blend::fillet_solver::p1::FilletSolverOps::solver_type(&s).unwrap(),
            crate::blend::fillet_solver::p1::FilletSolverType::ConstRadiusAssisted
        );

        // StepSolve is the base's (unit blend.fillet_solver_p3) dispatched on this
        // solver: a clipped step finds no boundary point here and goes on to the Newton
        // solve (through the SmConstantRadiusFS::LoadInitialValues override,
        // blend.fillet_standard_solver_p2), which finds no point; the clip it recorded
        // first is kept
        let d = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let cur = pnt([0.9, 0.5], [0.9, 0.5], [1.0, 0.0, 0.0], 0.0);
        let mut step = 0.5;
        let mut flags = StepSolve::default();
        let mut next = TsectPnt::new();
        s.step_solve(&k, &cur, &mut step, &d, &d, &mut flags, &mut next)
            .unwrap();
        assert!(
            flags.clipped && !flags.found_answer && !flags.boundary_hit,
            "{flags:?}"
        );
    }

    #[test]
    fn mid_point_and_flush_crv() {
        let a = pnt([0.0, 0.0], [1.0, 1.0], [2.0, 0.0, 0.0], 0.0);
        let mut b = pnt([1.0, 2.0], [3.0, 1.0], [0.0, 4.0, 0.0], 1.0);
        b.deviation = 0.25;
        let mut m = TsectPnt::new();
        find_guess_mid_tsect_pnt(&a, &b, 0.25, &mut m);
        assert_eq!(m.curve_parameter, 0.25);
        assert_eq!(m.deviation, 0.25);
        assert!((m.crv_deriv().length() - 2.5).abs() < 1e-12);
        let mut z = TsectPnt::new();
        let a0 = pnt([0.0, 0.0], [1.0, 1.0], [0.0, 0.0, 0.0], 0.0);
        find_guess_mid_tsect_pnt(&a0, &a0, 0.5, &mut z);
        assert_eq!(z.crv_deriv(), Vec3::new(0.0, 0.0, 0.0));

        let mut out = Vec::new();
        flush_crv(3, &[], &[], &mut out).unwrap();
        assert!(out.is_empty());
        let poly: Vec<Point3> = (0..7).map(|i| Vec3::new(f64::from(i), 0.0, 0.0)).collect();
        flush_crv(3, &poly, &[0.0, 1.0, 2.0], &mut out).unwrap();
        assert_eq!(out.len(), 1);
        assert!(flush_crv(3, &poly, &[0.0, 1.0], &mut out).is_err());
    }

    /// A solver for the skin check (#15256): balls centred on the x axis at `x = t`, radius
    /// `r0 + rate t`, with rails at `(t, 0, r)` and `(t, r, 0)`; both surfaces' parameters
    /// are `(t, v)`. Its plane solve finds no section `fail_first` times (or always), then
    /// the one on the plane; `skin` is the round's surface it builds.
    struct SkinMock {
        r0: f64,
        rate: f64,
        fail_first: usize,
        err_first: usize,
        always_fail: bool,
        calls: usize,
        skin: Option<Surface>,
    }

    impl SkinMock {
        fn new(r0: f64, rate: f64, skin: Option<Surface>) -> Self {
            Self {
                r0,
                rate,
                fail_first: 0,
                err_first: 0,
                always_fail: false,
                calls: 0,
                skin,
            }
        }
    }

    impl FilletSolverOps for SkinMock {
        type FilletGeom = ();

        fn point_on_plane_solve(
            &mut self,
            _k: &Kernel,
            plane_origin: Point3,
            _plane_normal: Vec3,
            _uv_domain1: &Extent2d,
            _uv_domain2: &Extent2d,
            _uv1: Point2,
            _uv2: Point2,
            point: &mut TsectPnt,
        ) -> Result<bool> {
            self.calls += 1;
            if self.calls <= self.err_first {
                return Err(SolidError::sm_err("test"));
            }
            if self.always_fail || self.calls <= self.fail_first {
                return Ok(false);
            }
            *point.uv_pos_mut(SrfIndex::First) = Point2::new(plane_origin.x, 0.5);
            *point.uv_pos_mut(SrfIndex::Second) = Point2::new(plane_origin.x, 0.5);
            Ok(true)
        }

        fn compute_point_values(
            &mut self,
            _k: &Kernel,
            uv_values: [Point2; 2],
            _uv_domain1: &Extent2d,
            _uv_domain2: &Extent2d,
            _curve_trace_direction: f64,
            point: &mut TsectPnt,
            _previous: Option<&TsectPnt>,
        ) -> Result<()> {
            point.point_type = IntersectionPointType::Crossing;
            *point.uv_pos_mut(SrfIndex::First) = uv_values[0];
            *point.uv_pos_mut(SrfIndex::Second) = uv_values[1];
            *point.uv_deriv_mut(SrfIndex::First) = Vec2::new(1.0, 0.0);
            *point.uv_deriv_mut(SrfIndex::Second) = Vec2::new(1.0, 0.0);
            *point.crv_pos_mut() = Vec3::new(uv_values[0].x, 0.0, 0.0);
            *point.crv_deriv_mut() = Vec3::new(1.0, 0.0, 0.0);
            Ok(())
        }

        fn step_solve(
            &mut self,
            _k: &Kernel,
            _point: &TsectPnt,
            _step_size: &mut f64,
            _uv_domain1: &Extent2d,
            _uv_domain2: &Extent2d,
            _flags: &mut StepSolve,
            _next: &mut TsectPnt,
        ) -> Result<()> {
            Err(SolidError::sm_err("test"))
        }

        fn compute_surface_values(
            &mut self,
            _k: &Kernel,
            _uv_values: [Point2; 2],
            _point: &mut TsectPnt,
        ) -> Result<()> {
            Ok(())
        }

        fn compute_uv_vectors(&mut self, _k: &Kernel, _point: &mut TsectPnt) -> Result<()> {
            Ok(())
        }

        fn set_surfaces_to_zero_offset(&mut self) -> Result<()> {
            Ok(())
        }

        fn reload_surface_offsets(&mut self) -> Result<()> {
            Ok(())
        }

        fn offset_radii_can_change(&self) -> Result<bool> {
            Ok(true)
        }

        fn rail_surface_evaluate_1st_derivatives(
            &mut self,
            _k: &Kernel,
            rail: usize,
            uv: Point2,
        ) -> Result<(Point3, Vec3, Vec3)> {
            let t = uv.x;
            let r = self.r0 + self.rate * t;
            Ok(if rail == 0 {
                (
                    Vec3::new(t, 0.0, r),
                    Vec3::new(1.0, 0.0, self.rate),
                    Vec3::ZERO,
                )
            } else {
                (
                    Vec3::new(t, r, 0.0),
                    Vec3::new(1.0, self.rate, 0.0),
                    Vec3::ZERO,
                )
            })
        }

        fn set_status(&mut self, _status: FilStatus) -> Result<()> {
            Ok(())
        }

        fn last_fillet_geom(&self) -> Result<()> {
            Ok(())
        }

        fn self_intersection_handler(&self) -> Result<Option<HandlingAlgorithmType>> {
            Ok(None)
        }

        fn smooth_out_intersection(
            &mut self,
            _k: &Kernel,
            _pseudo: &mut FilletBrep,
            _geom: &(),
            _curves_3d: &mut Vec<BSplineCurve>,
            _uv_curves1: &mut Vec<BSplineCurve>,
            _uv_curves2: &mut Vec<BSplineCurve>,
        ) -> Result<()> {
            Ok(())
        }

        fn has_surface_generator(&self) -> Result<bool> {
            Ok(true)
        }

        fn create_fillet_surface(
            &mut self,
            _k: &Kernel,
            _center_line: &mut BSplineCurve,
            _curve1: &mut BSplineCurve,
            _curve2: &mut BSplineCurve,
            _points: &[TsectPnt],
            _uv1: &mut BSplineCurve,
            _uv2: &mut BSplineCurve,
        ) -> Result<(Option<Surface>, Option<LawCornerPieces>)> {
            Ok((self.skin.clone(), None))
        }

        fn keep_law_corner_pieces(&mut self, _pieces: LawCornerPieces) -> Result<()> {
            Ok(())
        }

        fn skins_varying_balls(&self) -> Result<bool> {
            Ok(true)
        }

        fn set_fillet_surface(
            &mut self,
            _pseudo: &mut FilletBrep,
            _geom: &(),
            _surface: Option<Surface>,
        ) -> Result<()> {
            Ok(())
        }

        fn set_center_line_curve(
            &mut self,
            _pseudo: &mut FilletBrep,
            _geom: &(),
            _center_line: BSplineCurve,
        ) -> Result<()> {
            Ok(())
        }

        fn set_rail_curve(
            &mut self,
            _pseudo: &mut FilletBrep,
            _geom: &(),
            _rail: usize,
            _curve: BSplineCurve,
        ) -> Result<()> {
            Ok(())
        }

        fn set_rail_mate_uv_curve(
            &mut self,
            _pseudo: &mut FilletBrep,
            _geom: &(),
            _rail: usize,
            _uv_curve: BSplineCurve,
        ) -> Result<()> {
            Ok(())
        }
    }

    /// The flat chord between the rails of radius `r0` balls on the x axis, from `x = x0` to
    /// `x0 + 1` over the parameter `u` in [0, 1]: `r0 (1 - 1/sqrt 2)` off the balls in its
    /// middle.
    fn chord_skin(r0: f64, x0: f64) -> Surface {
        bilinear([
            [x0, 0.0, r0],
            [x0 + 1.0, 0.0, r0],
            [x0, r0, 0.0],
            [x0 + 1.0, r0, 0.0],
        ])
    }

    /// Stations at the parameters `ts` on the mock's balls.
    fn skin_stations(ts: &[f64]) -> Vec<TsectPnt> {
        ts.iter()
            .map(|&t| pnt([t, 0.5], [t, 0.5], [1.0, 0.0, 0.0], t))
            .collect()
    }

    fn skin_check(mock: &mut SkinMock, ts: &[f64], skin: &Surface) -> Result<SkinCheck> {
        let k = &Kernel::default();
        let (a, b) = surfaces();
        let d = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let fi: FilletIntersector<'_, '_, NoSurfaceHandle, ()> =
            FilletIntersector::new(k, &a, &d, &b, &d, None).unwrap();
        fi.skin_splits(mock, k, &skin_stations(ts), &[skin])
    }

    /// `FlushCurve` over three stations on the mock's balls, whose skin is the mock's.
    fn flush_with(mock: SkinMock) -> Result<()> {
        flush_over(mock, &[0.0, 0.5, 1.0])
    }

    /// `FlushCurve` over stations at `ts` on the mock's balls, whose skin is the mock's.
    fn flush_over(mut mock: SkinMock, ts: &[f64]) -> Result<()> {
        let k = &Kernel::default();
        let (a, b) = surfaces();
        let d = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let mut fi: FilletIntersector<'_, '_, NoSurfaceHandle, ()> =
            FilletIntersector::new(k, &a, &d, &b, &d, None).unwrap();
        for p in skin_stations(ts) {
            let id = fi.base.base.alloc_point(p);
            fi.base.base.add_point_to_curve(id).unwrap();
        }
        let mut out = TsectCurves::default();
        fi.flush_curve(&mut mock, k, &mut FilletBrep::new(), &mut out)
    }

    #[test]
    fn the_skin_check_measures_the_worst_distance_and_splits_where_it_is_past() {
        let skin = chord_skin(1.0, 0.0);
        let mut mock = SkinMock::new(1.0, 0.0, None);
        let SkinCheck {
            splits,
            worst,
            not_finite_at,
        } = skin_check(&mut mock, &[0.0, 1.0], &skin).unwrap();
        assert_eq!((splits.len(), not_finite_at), (1, None));
        let (j, at, _) = &splits[0];
        assert_eq!(*j, 0);
        assert!((at - 0.5).abs() < 1e-9, "{at}");
        assert!(
            (worst - (1.0 - core::f64::consts::FRAC_1_SQRT_2)).abs() < 1e-9,
            "{worst}"
        );
    }

    /// Before, a section that did not project into its span refused the round ("a span's
    /// middle section is not on its skin"); now the span takes that section at its middle
    /// knot, unmeasured, as the span equalisation places a station.
    #[test]
    fn a_section_off_its_skin_splits_the_span_at_its_knot() {
        let skin = chord_skin(1.0, 5.0);
        let mut mock = SkinMock::new(1.0, 0.0, None);
        let SkinCheck { splits, worst, .. } = skin_check(&mut mock, &[0.0, 1.0], &skin).unwrap();
        assert_eq!(splits.len(), 1);
        let (j, at, mid) = &splits[0];
        assert_eq!((*j, *at), (0, 0.5));
        assert_eq!(mid.curve_parameter, 0.5);
        assert!(worst.is_infinite());
    }

    #[test]
    fn balls_whose_radius_outruns_their_centres_are_refused_as_nesting() {
        let skin = chord_skin(1.0, 0.0);
        let mut mock = SkinMock::new(1.0, 2.0, None);
        let e = skin_check(&mut mock, &[0.0, 1.0], &skin).unwrap_err();
        assert_eq!(
            e.blend_failure(),
            Some(&BlendFailure::BallsNest {
                s: 0.5,
                rate: 2.0,
                speed: 1.0
            }),
            "{e:?}"
        );
        assert_eq!(e.kind(), crate::error::ErrorKind::BallsFold);
        assert!(e.is_refusal());
    }

    /// A value that is not finite proves nothing about the skin: the span is split at its
    /// knot (as before, when a NaN distance split the span), and the round is refused as a
    /// defect only if it stays not finite to the cap.
    #[test]
    fn a_skin_measure_that_is_not_finite_splits_and_is_a_defect_at_the_cap() {
        let skin = chord_skin(1.0, 0.0);
        let mut mock = SkinMock::new(f64::NAN, 0.0, None);
        let SkinCheck {
            splits,
            not_finite_at,
            ..
        } = skin_check(&mut mock, &[0.0, 1.0], &skin).unwrap();
        assert_eq!(not_finite_at, Some(0.5));
        assert_eq!((splits.len(), splits[0].1), (1, 0.5));
        let e = flush_with(SkinMock::new(f64::NAN, 0.0, Some(skin))).unwrap_err();
        assert!(
            matches!(
                e.blend_failure(),
                Some(BlendFailure::SkinMeasureNotFinite { .. })
            ),
            "{e:?}"
        );
        assert!(e.is_defect());
    }

    #[test]
    fn a_skin_that_does_not_run_over_its_stations_is_a_port_invariant() {
        let skin = chord_skin(1.0, 0.0);
        let mut mock = SkinMock::new(1.0, 0.0, None);
        let e = skin_check(&mut mock, &[0.0, 2.0], &skin).unwrap_err();
        assert!(
            matches!(
                e,
                SolidError::PortInvariant {
                    reason: "the skin does not run over its stations' parameters",
                    ..
                }
            ),
            "{e:?}"
        );
    }

    #[test]
    fn a_middle_section_is_retried_from_both_end_stations_then_refused_as_not_converging() {
        let skin = chord_skin(1.0, 0.0);
        // found on the third start: the check goes on
        let mut mock = SkinMock::new(1.0, 0.0, None);
        mock.fail_first = 2;
        let c = skin_check(&mut mock, &[0.0, 1.0], &skin).unwrap();
        assert_eq!((mock.calls, c.splits.len()), (3, 1));
        // a failed solve is retried too
        let mut mock = SkinMock::new(1.0, 0.0, None);
        mock.err_first = 2;
        let c = skin_check(&mut mock, &[0.0, 1.0], &skin).unwrap();
        assert_eq!((mock.calls, c.splits.len()), (3, 1));
        // found from none of the three: a defect with its station
        let mut mock = SkinMock::new(1.0, 0.0, None);
        mock.always_fail = true;
        let e = skin_check(&mut mock, &[0.0, 1.0], &skin).unwrap_err();
        assert_eq!(mock.calls, 3);
        match e.blend_failure() {
            Some(&BlendFailure::StationDidNotConverge {
                s,
                residual,
                step,
                iterations,
                ..
            }) => {
                // (the plane solve reports only whether it found the section)
                assert_eq!((s, residual, step, iterations), (0.5, None, None, None));
            }
            other => panic!("{other:?}"),
        }
        assert!(e.is_defect());
    }

    /// A skin that stays off its balls whatever the stations (the mock builds the same flat
    /// chord every round) reaches the cap and is refused with the numbers.
    #[test]
    fn a_skin_still_off_its_balls_at_the_cap_is_refused_with_its_numbers() {
        let e = flush_with(SkinMock::new(1.0, 0.0, Some(chord_skin(1.0, 0.0)))).unwrap_err();
        let tol = {
            let k = &Kernel::default();
            let (a, b) = surfaces();
            let d = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
            let fi: FilletIntersector<'_, '_, NoSurfaceHandle, ()> =
                FilletIntersector::new(k, &a, &d, &b, &d, None).unwrap();
            fi.base.base.this_approx_tol_3d
        };
        match e.blend_failure() {
            Some(&BlendFailure::SkinPastStationCap {
                asked,
                reached,
                stations,
                rounds,
            }) => {
                assert_eq!(asked, tol);
                assert!(
                    (reached - (1.0 - core::f64::consts::FRAC_1_SQRT_2)).abs() < 1e-9,
                    "{reached}"
                );
                assert_eq!(rounds, SKIN_ROUNDS, "{e}");
                assert!(stations > 3, "{e}");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(e.kind(), crate::error::ErrorKind::NotWithinTolerance);
    }

    /// The tolerance of the mock's intersector.
    fn mock_tol() -> f64 {
        let k = &Kernel::default();
        let (a, b) = surfaces();
        let d = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let fi: FilletIntersector<'_, '_, NoSurfaceHandle, ()> =
            FilletIntersector::new(k, &a, &d, &b, &d, None).unwrap();
        fi.base.base.this_approx_tol_3d
    }

    /// A skin on its balls at both stations and at the span's middle, where the samples
    /// measure it, but bulged twenty times the tolerance off them in between (closed form,
    /// `skin_bound::tests::bulged_tube`): the samples alone kept it (no split, the largest
    /// distance measured 0); the proved bound over the whole span splits it, and its bound
    /// covers the bulge's peak.
    #[test]
    fn a_skin_off_its_balls_between_the_samples_is_split_on_its_bound() {
        let tol = mock_tol();
        let peak = 20.0 * tol;
        let bump = peak * 12.0 * 3.0_f64.sqrt();
        let skin = skin_bound::tests::bulged_tube(1.0, bump);
        let mut mock = SkinMock::new(1.0, 0.0, None);
        let SkinCheck {
            splits,
            worst,
            not_finite_at,
        } = skin_check(&mut mock, &[0.0, 1.0], &skin).unwrap();
        assert_eq!((splits.len(), not_finite_at), (1, None), "{worst}");
        let (j, at, _) = &splits[0];
        assert_eq!(*j, 0);
        assert!((at - 0.5).abs() < 1e-9, "{at}");
        assert!(worst >= peak, "{worst} < {peak}");
        // the samples on the middle iso-curve see the skin on its balls
        let k = &Kernel::default();
        let cone = EnvelopeCone {
            c: Point3::new(0.5, 0.0, 0.0),
            r: 1.0,
            axis: None,
        };
        for share in SKIN_SHARES {
            let off = cone.distance(skin.evaluate_point(k, Point2::new(0.5, share)).unwrap());
            assert!(off.abs() < 1e-12, "{share}: {off}");
        }
    }

    /// The same skin on its balls everywhere is kept: its proved bound is rounding only.
    #[test]
    fn a_skin_on_its_balls_is_kept_on_its_bound() {
        let skin = skin_bound::tests::bulged_tube(1.0, 0.0);
        let mut mock = SkinMock::new(1.0, 0.0, None);
        let c = skin_check(&mut mock, &[0.0, 1.0], &skin).unwrap();
        assert!(c.splits.is_empty(), "{:?}", c.worst);
        assert!(c.worst < 1e-12, "{}", c.worst);
    }

    /// Through `FlushCurve`: the bulged skin (the mock builds it whatever the stations) is
    /// refused at the cap; with the samples alone the round was kept after one round, off
    /// its balls by twenty times the tolerance.
    #[test]
    fn a_skin_off_its_balls_between_the_samples_is_refused_at_the_cap() {
        let tol = mock_tol();
        let bump = 20.0 * tol * 12.0 * 3.0_f64.sqrt();
        let skin = skin_bound::tests::bulged_tube(1.0, bump);
        let e = flush_over(SkinMock::new(1.0, 0.0, Some(skin)), &[0.0, 1.0]).unwrap_err();
        match e.blend_failure() {
            Some(&BlendFailure::SkinPastStationCap { asked, reached, .. }) => {
                assert_eq!(asked, tol);
                assert!(reached > tol, "{reached}");
            }
            other => panic!("{other:?}"),
        }
    }

    /// A cancellation reaches the skin check's rounds.
    #[test]
    fn the_skin_check_polls_cancellation() {
        let skin = chord_skin(1.0, 0.0);
        let token = crate::kernel::CancelToken::new();
        token.cancel();
        let mut k = Kernel::default();
        k.set_cancel_token(token);
        let k = &k;
        let (a, b) = surfaces();
        let d = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let fi: FilletIntersector<'_, '_, NoSurfaceHandle, ()> =
            FilletIntersector::new(k, &a, &d, &b, &d, None).unwrap();
        let mut mock = SkinMock::new(1.0, 0.0, None);
        let e = fi
            .skin_splits(&mut mock, k, &skin_stations(&[0.0, 1.0]), &[&skin])
            .unwrap_err();
        assert!(matches!(e, SolidError::Cancelled { .. }), "{e:?}");
    }

    /// A skin of a kind the generator does not build is a port invariant, not a pass.
    #[test]
    fn a_skin_neither_b_spline_nor_definition_is_a_port_invariant() {
        let plane = Surface::from(crate::surface::plane::Plane::new(
            Point3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
        ));
        let mut mock = SkinMock::new(1.0, 0.0, None);
        let e = skin_check(&mut mock, &[0.0, 1.0], &plane).unwrap_err();
        assert!(matches!(e, SolidError::PortInvariant { .. }), "{e:?}");
    }
}
