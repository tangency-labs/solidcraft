//! `SmBSplineCurve`, part 3: scale-translate-rotate copies, internal pole and segment
//! line tests, single control point access, plane containment, joining another curve
//! onto this one, subdivision at discontinuities, closing small gaps between curves
//! and the `AssertValid` checks.
//!
//! Upstream: `source/SMLib/src/SmBSplineCurve.cpp`, `source/SMLib/inc/SmBSplineCurve.h`.
//!
//! | Rust                                              | Upstream                               |
//! |---------------------------------------------------|----------------------------------------|
//! | [`BSplineCurve::create_by_scale_trans_rot`]       | `CreateByScaleTransRot`                |
//! | [`BSplineCurve::has_internal_pole`]               | `HasInternalPole`                      |
//! | [`BSplineCurve::is_segment_line`], [`SegmentLine`] | `IsSegmentLine`                       |
//! | [`BSplineCurve::control_point`]                   | `GetControlPoint`                      |
//! | [`BSplineCurve::is_on_plane`], [`OnPlane`]        | `IsOnPlane`                            |
//! | [`BSplineCurve::join_with`]                       | `JoinWith`                             |
//! | [`BSplineCurve::subdivide_at_discontinuity`]      | `SubdivideAtDiscontinuity`             |
//! | [`BSplineCurve::trim_gap`], [`GapFix`]            | `TrimGap`                              |
//! | [`BSplineCurve::assert_valid`]                    | `AssertValid`                          |
//! | [`BSplineCurve::assert_no_periodic_jump`], [`assert_no_periodic_jump_of`] | `AssertNoPeriodicJump` |
//! | `is_zero_speed_tangent_end`, `zero_speed_radius_stretch` (private; fork, WR-668) | `sm_IsZeroSpeedTangentEnd`, `sm_ZeroSpeedRadiusStretch` |
//!
//! The other members of the file belong to `nurbs.b_spline_curve_p1`, `_p2` and
//! `nurbs.b_spline_curve_n_lib`.
//!
//! Deviations:
//! - **Out-params, contexts.** Outputs are return values; `SmContext` arguments (and
//!   the local context of `TrimGap`) only choose where upstream allocates and are
//!   dropped. `IsSegmentLine` and `IsOnPlane` leave some outputs unset on some paths;
//!   those are `None` in [`SegmentLine`] / [`OnPlane`]. `IsSegmentLine` returns an
//!   `SmStatus` through its `SmBoolean` when an evaluation fails (`SER`); the port
//!   returns the error. Its sample count is bounded by [`MAX_SEGMENT_SAMPLES`]
//!   (`InvalidInput` above it, checked only where upstream starts sampling): upstream
//!   evaluates any count, so an absurd one spins for hours.
//! - **Join ends** (`ULONG` 0 = start, 1 = end) are `bool`s (`true` = end), the
//!   `TrimGap` mode (`int` 0, 1, 2) is [`GapFix`] (upstream treats any other value as
//!   0) and its `int` result a `bool`.
//! - **Join in place.** `JoinWith` hands `this` and the other curve to
//!   `CreateByJoining`, which may elevate or flatten them in place; the port moves both
//!   into the segment slice and back, so the other curve sees the same edits.
//! - **Ignored statuses.** Where upstream ignores a status (the `TrimGap` evaluations,
//!   extensions, trims and control point edits, `SubdivideAtDiscontinuity`'s trims,
//!   `AssertValid`'s evaluations and bounding box), the port does too, except that
//!   [`SolidError::Unported`] is always passed on. Ignored evaluations leave
//!   default-constructed `SmVector3d`s ([`Vec3::UNDEF`]), as upstream.
//!   With the opt-in `Kernel::fixes.query_statuses_pass_on` (WR-663) `TrimGap` returns
//!   `false` (gap not fixed) at the first of those that fails, `HasInternalPole` returns
//!   `false` when its knots cannot be read, and `AssertValid` fails tests 5/6 when a
//!   derivative at an interior knot of multiplicity `p` cannot be evaluated (`value`
//!   `SM_UNDEF_DOUBLE`), fails tests 11/12 or 13/14 (`value` `SM_UNDEF_DOUBLE`) when the
//!   bounding box or `GlobalPropertyAnalysis` fails, and fails tests 11/12 (`value`
//!   `SM_UNDEF_DOUBLE`) when a derivative at a first-derivative solution cannot be
//!   evaluated, as the fork (tangency-labs/smlib 8843967).
//! - **Crashes.** Where upstream dereferences a `NULL` (a curve without NURB in
//!   `GetControlPoint` / `IsSegmentLine`, an extension `TrimGap` did not make, an
//!   unclamped knot vector indexing before the control points in `HasInternalPole`)
//!   the port returns `SM_ERR` or `InvalidInput`. `GetControlPoint` of a curve of
//!   dimension 1 is `InvalidInput` (see `GetControlPointsExpert` in `p2`).
//! - **`AssertValid`.** The report list is an [`AssertArray`]; `this` is identified by
//!   the caller's [`ObjectKey`]. Only `SM_LEVEL_GIVEN` changes anything without
//!   `SM_DEBUG_CODE` (it skips the base class checks), so the level is the
//!   `level_given` flag; the walk flag and test requests are unused upstream and
//!   dropped. `MakeNurb` cannot make a NURB for a plain B-spline, so the method takes
//!   `&self`. `SM_ASSERT_SET_VALUE` / `SM_ASSERT_SET_OWNER_PARAM` after a report are
//!   applied to the new report before it is added (the same result: they only touch
//!   a report that was added, and report equality ignores those fields). `WARN`
//!   messages are not ported.
//!   Not upstream (WR-668, the fork's tangency-labs/smlib ecf1963, replayed patch for
//!   patch as 6ca3405 in tangency-labs/smlib#76; the default since its flag was
//!   retired): at a zero-speed tangent end of an edge's curve or
//!   trim, test 3 passes over the coincident first control points and tests 13/14
//!   pass over a radius below the tolerance within the end's growth stretch. The fork
//!   reads the curve's edge (`GetEdge`, `GetEdgeuse()->GetEdge()`); a Rust curve has
//!   none, so the edge checks pass an `EdgeEnds` context in
//!   ([`BSplineCurve::assert_valid_on_edge`]). The fork takes the smallest radius
//!   solution (its loop from cc4289d); the port keeps WR-382's first solution and
//!   takes the first one not passed over.
//!   Not upstream (WR-560 extended): test 13 of a UV trim measured on its surface also
//!   passes over a solution below the tolerance where the trim's image on the surface,
//!   `S(c(t))`, is proved by a rounded-interval bound on its composed coefficients to have a radius of at least
//!   the tolerance over the whole trim, its zero-speed end stretches (WR-668) to at least
//!   half the growth line they stand for at each dyadic piece's near edge, out to a sliver
//!   at each end (one polynomial piece, capped at the first knot) whose UV extent is
//!   certified to be at the trim's own rounding (`stretch_clears_on_surface`)
//!   ([`crate::nurbs::b_spline_curve::trim_image`]; at most 4096 pieces, each halved at
//!   most 64 times, per call, no recursion): the constant metric WR-560 maps the trim by is exact only on a
//!   plane, and near a pole the exact trim of a smooth curve turns sharply in (u, v).
//! - **Back-edges.** `HasInternalPole` calls `IsDegenerate` (`p2`); `IsSegmentLine`,
//!   `IsOnPlane` and `JoinWith` test `IsKindOf(SmLine_TYPE)` (false for a
//!   `BSplineCurve`; a line is a [`crate::curve::line::Line`], whose `JoinWith`
//!   overrides this one) and `JoinWith` measures `SmCurve::ApproximateLength` without
//!   a gap tolerance; `SubdivideAtDiscontinuity` calls `CalculateContinuities`
//!   (`SmCurve::EvaluateContinuity`);
//!   `TrimGap` intersects with `SmCurve::GlobalCurveIntersect` (`curve.curve_p8`,
//!   whose status upstream ignores: a failure finds no intersection here, upstream
//!   keeps the solutions found before it); `AssertValid` calls
//!   `SmCurve::GlobalPropertyAnalysis` (`curve.curve_p7`); its
//!   `SmCurve::AssertValid` is [`crate::curve::curve::p4::assert_valid_base`] for a
//!   curve without owner. Those calls return
//!   [`SolidError::Unported`] until the units are ported. `SmTol::IsDegenerate(this)`
//!   (unit `geom.tol`, which does not port the object dispatch) is computed locally for
//!   a curve without edge: `IsDegenerate` at twice the system zone tolerance, so
//!   `AssertValid` takes the [`Kernel`]; `SmCurve::IsAnalytic` / `IsBounded` are the
//!   base `FALSE` / `TRUE`.

use crate::curve::curve::p2::{Curve, CurvePropertyType};
use crate::curve::curve::p7::{PropertyAnalysis, PropertyAnalysisOptions};
use crate::curve::curve::p8::CurveIntersectOptions;
use crate::error::{ignore_or_pass_on, ignored, Result, SolidError};
use crate::geom::assert_array::{labels, AssertArray, AssertReport, ObjectKey, ReportSpec};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::geom_utility::p1::{
    line_closest_point, line_point_distance, plane_point_distance, point_project_to_plane,
    segment_point_distance,
};
use crate::geom::geom_utility::p2::ContinuityType;
use crate::geom::solution_array::{ObjectKind, SolutionEnd, SolutionObject, SolutionType};
use crate::geom::tol::{
    is_zero_to_tol, tol3d, xsect_tol_3d_from_zones, zone_tol_3d, SmallTopoType, ZONE_TOL_3D,
};
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::{max2, max3, min2, BIG_DOUBLE, EFF_ZERO, EFF_ZERO_SQRT, PI, UNDEF_DOUBLE};
use crate::nurbs::b_spline_curve::analysis::{analysis_key, curve_bounds, BOUND_MARGIN};
use crate::nurbs::b_spline_curve::p1::BSplineCurve;
use crate::nurbs::b_spline_curve::p2::{BoundingBoxRequest, ControlPointForm};
use crate::nurbs::b_spline_curve::trim_image::{radius_clears_on_surface, sliver_within};
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::geom::dist_cpt_cpt;
use crate::nurbs::nurbs_crv::to_euclid;
use crate::surface::surface::Surface;

/// `SmLine_TYPE` (`CURV_BASE_TYPE + 10`).
const LINE_TYPE: i64 = BSplineCurve::TYPE - 33 + 10;

/// Largest sample count [`BSplineCurve::is_segment_line`] accepts (a port bound,
/// far above upstream's callers; upstream evaluates any count).
pub const MAX_SEGMENT_SAMPLES: u64 = 1 << 20;

/// Cosine of `SM_CONTINUITY_ANGLE` (1 degree), as written in `AssertValid`.
const COSINE_TOL: f64 = 0.999_847_695_156_391_3;

const OP_SCALE: &str = "nurbs::b_spline_curve::p3::BSplineCurve::create_by_scale_trans_rot";
const OP_POLE: &str = "nurbs::b_spline_curve::p3::BSplineCurve::has_internal_pole";
const OP_SEG: &str = "nurbs::b_spline_curve::p3::BSplineCurve::is_segment_line";
const OP_CP: &str = "nurbs::b_spline_curve::p3::BSplineCurve::control_point";
const OP_JOIN: &str = "nurbs::b_spline_curve::p3::BSplineCurve::join_with";
const OP_SUBDIV: &str = "nurbs::b_spline_curve::p3::BSplineCurve::subdivide_at_discontinuity";
const OP_GAP: &str = "nurbs::b_spline_curve::p3::BSplineCurve::trim_gap";
const OP_ASSERT: &str = "nurbs::b_spline_curve::p3::BSplineCurve::assert_valid";

/// The outputs of [`BSplineCurve::is_segment_line`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SegmentLine {
    /// The `SmBoolean` result: the segment is a line.
    pub is_line: bool,
    /// The curve start point and chord (`rLinePointArg`, `rLineVectorArg`), when
    /// upstream sets them (also for a short chord whose midpoint coincides with the
    /// ends, which is not reported as a line).
    pub line: Option<(Point3, Vec3)>,
}

/// The outputs of [`BSplineCurve::is_on_plane`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OnPlane {
    /// All curve points are within the tolerance of the plane (`rbIsOnPlaneArg`).
    pub on_plane: bool,
    /// The largest distance of the end, middle and control points to the plane
    /// (`rdMaxDistToPlaneArg`); `None` when a sample point is off the plane (upstream
    /// leaves it unset).
    pub max_dist: Option<f64>,
}

/// Which end points [`BSplineCurve::trim_gap`] moves (`CrvToFix`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GapFix {
    /// `0`: both curves to the midpoint of the extended curves' intersection ends.
    Both = 0,
    /// `1`: both curves to this curve's initial end point.
    ToThis = 1,
    /// `2`: both curves to the other curve's initial end point.
    ToOther = 2,
}

// ---- back-edges -------------------------------------------------------------------

/// `SmCurve::ApproximateLength` of `curve` over `interval` with `samples` samples.
fn approximate_length(
    k: &Kernel,
    curve: &BSplineCurve,
    interval: &Extent1d,
    samples: usize,
) -> Result<f64> {
    Ok(crate::curve::curve::p2::approximate_length(k, curve, interval, samples, false)?.0)
}

/// A curve of [`global_curve_intersect`] in its solutions (`0`: the first curve, `1`:
/// the second, `2` / `3`: their analytic replacements) and the natural interval of
/// the curve (upstream's objects are the curves, whose natural intervals
/// `IdenticalSolutions` reads).
#[derive(Debug, Clone, Copy, PartialEq)]
struct GapCurve(u8, Extent1d);

impl SolutionObject for GapCurve {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Curve
    }

    fn natural_interval(&self) -> Option<Extent1d> {
        Some(self.1)
    }
}

/// `SmCurve::GlobalCurveIntersect` of `curve0` over `ivl0` with `curve1` over `ivl1`
/// at tolerance `tol`: the solutions' types and start ends. The status is ignored
/// upstream: a failed intersection gives no solutions here (upstream keeps those found
/// before the failure); [`SolidError::Unported`] is passed on.
fn global_curve_intersect(
    k: &Kernel,
    curve0: &BSplineCurve,
    ivl0: &Extent1d,
    curve1: &BSplineCurve,
    ivl1: &Extent1d,
    tol: f64,
) -> Result<Vec<(SolutionType, SolutionEnd)>> {
    let c0 = Curve::from(curve0.clone());
    let c1 = Curve::from(curve1.clone());
    let opts = CurveIntersectOptions {
        skip_global_coincidence_check: false,
        handles: [
            Some(GapCurve(0, c0.natural_interval())),
            Some(GapCurve(1, c1.natural_interval())),
        ],
        analytic_handles: [
            Some(GapCurve(2, c0.natural_interval())),
            Some(GapCurve(3, c1.natural_interval())),
        ],
    };
    let found = ignored(c0.global_curve_intersect(k, ivl0, &c1, ivl1, tol, &opts))?;
    Ok(found.map_or_else(Vec::new, |sols| {
        sols.as_slice()
            .iter()
            .map(|s| (s.solution_type, s.start))
            .collect()
    }))
}

/// The property `SmCurve::GlobalPropertyAnalysis` minimizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CurveProperty {
    /// `SM_CP_MINIMIZE_FIRST_DERIVATIVE`.
    MinimizeFirstDerivative,
    /// `SM_CP_MINIMIZE_RADIUS_OF_CURVATURE`.
    MinimizeRadiusOfCurvature,
}

/// The analysed curve in the solutions of [`global_property_analysis`] (unread).
#[derive(Debug, Clone, Copy, PartialEq)]
struct SelfCurve;

impl SolutionObject for SelfCurve {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Curve
    }
}

/// `SmCurve::GlobalPropertyAnalysis` of `curve` over `domain` (no extra arguments) with
/// minimum separation `min_sep`: whether the call returned `SM_SUCCESS` and the
/// solutions' start ends (including any partial solutions found before a failure, as
/// upstream's `rSolutionArray`); [`SolidError::Unported`] and
/// [`SolidError::Cancelled`] are passed on.
///
/// Not upstream (#2568): with `Kernel::memoize_curve_caches`, the result is kept in the
/// kernel's [`AnalysisMemo`](crate::nurbs::b_spline_curve::analysis::AnalysisMemo) by the
/// exact content of its inputs (the curve, the interval, the property, the separation,
/// the kernel's tolerances and fixes), and a curve analysed before gets its result again.
fn global_property_analysis(
    k: &Kernel,
    curve: &BSplineCurve,
    domain: &Extent1d,
    property: CurveProperty,
    min_sep: f64,
) -> Result<(bool, Vec<SolutionEnd>)> {
    let key = if k.memoize_curve_caches {
        analysis_key(
            curve,
            domain.min(),
            domain.max(),
            property as u64,
            min_sep,
            &analysis_settings(k),
        )
    } else {
        None
    };
    if let Some(key) = &key {
        if let Some(hit) = k
            .caches
            .curve_analysis
            .try_borrow()
            .ok()
            .and_then(|memo| memo.get(key))
        {
            return Ok(hit);
        }
    }
    let result = global_property_analysis_run(k, curve, domain, property, min_sep)?;
    if let (Some(key), Ok(mut memo)) = (key, k.caches.curve_analysis.try_borrow_mut()) {
        memo.insert(key, result.clone());
    }
    Ok(result)
}

/// The kernel settings an analysis depends on, for [`analysis_key`]: its tolerances (by
/// their debug text, which writes each double so that it reads back exactly) and its
/// fixes.
fn analysis_settings(k: &Kernel) -> Vec<u64> {
    let mut settings: Vec<u64> = format!("{:?}", k.tolerances)
        .bytes()
        .map(u64::from)
        .collect();
    settings.push(u64::MAX);
    settings.extend(k.fixes.flags().map(|(_, on)| u64::from(on)));
    settings
}

/// [`global_property_analysis`] without the memo.
fn global_property_analysis_run(
    k: &Kernel,
    curve: &BSplineCurve,
    domain: &Extent1d,
    property: CurveProperty,
    min_sep: f64,
) -> Result<(bool, Vec<SolutionEnd>)> {
    let property = match property {
        CurveProperty::MinimizeFirstDerivative => CurvePropertyType::MinimizeFirstDerivative,
        CurveProperty::MinimizeRadiusOfCurvature => CurvePropertyType::MinimizeRadiusOfCurvature,
    };
    let crv = Curve::from(curve.clone());
    let opts: PropertyAnalysisOptions<'_, SelfCurve> = PropertyAnalysisOptions::default();
    let mut out = PropertyAnalysis {
        solutions: crate::geom::solution_array::SolutionArray::new(),
        comb_points: Vec::new(),
        comb_vecs: Vec::new(),
    };
    let ok =
        ignored(crv.global_property_analysis_into(k, domain, property, min_sep, &opts, &mut out))?
            .is_some();
    let sols = out.solutions.as_slice().iter().map(|s| s.start).collect();
    Ok((ok, sols))
}

/// `SmCurve::AssertValid(pAList, eTestLevel, SM_NO_WALK, pTestRequests)` of `curve`
/// (reporting for `key`); a B-spline here has no owner.
fn curve_assert_valid(
    curve: &BSplineCurve,
    list: Option<&mut AssertArray>,
    key: ObjectKey,
) -> Result<bool> {
    crate::curve::curve::p4::assert_valid_base(
        None,
        curve.dim(),
        "SmBSplineCurve",
        list,
        false,
        key,
    )
}

/// `SmTol::IsDegenerate(const SmObject*)` of `curve` (its `SmCurve_TYPE` branch,
/// computed here): `IsDegenerate` at the intersection tolerance of two zone
/// tolerances of the curve, where a curve without edge (a B-spline here has no owner)
/// has the system zone tolerance (`SmTol::GetZoneTol3d(const SmObject*)`).
fn tol_is_degenerate(k: &Kernel, is_degenerate: &dyn Fn(f64) -> Result<bool>) -> Result<bool> {
    let zone = zone_tol_3d(k, SmallTopoType::NormalSize);
    let xsect = xsect_tol_3d_from_zones(k, zone, zone);
    is_degenerate(xsect)
}

/// The virtual methods of a class derived from `SmBSplineCurve` that
/// `SmBSplineCurve::AssertValid` calls on `this`.
pub(crate) struct DerivedOverrides<'a> {
    /// `GetDegree` (`None`: the NURB's degree, the base method).
    pub degree: Option<usize>,
    /// `IsAnalytic`.
    pub analytic: bool,
    /// `IsBounded`.
    pub bounded: bool,
    /// `IsDegenerate(tol)` over the natural interval.
    pub is_degenerate: &'a dyn Fn(f64) -> Result<bool>,
    /// Not upstream: the surface of the face whose UV trim this curve is
    /// (`GetEdgeuse()->GetFace()->GetSurface()`), WR-560; `None` for any other curve.
    pub trim_surface: Option<&'a Surface>,
    /// Not upstream: the edge this curve is the 3D curve or a UV trim of
    /// (`GetEdge()`, `GetEdgeuse()->GetEdge()`), read for WR-668's zero-speed tangent
    /// ends; `None` for a curve on no edge.
    pub edge_ends: Option<&'a EdgeEnds>,
}

/// Not upstream (fork tangency-labs/smlib ecf1963, WR-668, opt-in): what the fork's
/// `sm_IsZeroSpeedTangentEnd` reads from a curve's edge, which a Rust curve does not
/// point back to. The edge check and edge-use check build it from the edge before
/// checking its curve or a UV trim of it ([`DerivedOverrides::edge_ends`]).
#[derive(Debug, Clone)]
pub(crate) struct EdgeEnds {
    /// The edge's interval (`pEdge->GetInterval()`).
    pub interval: Extent1d,
    /// At the edge's start (`[0]`) and end (`[1]`) parameter: whether at least two
    /// faces of the edge's uses have their normals, at their UV trims' points there,
    /// parallel to the first one's within `SM_CONTINUITY_ANGLE` (`false` where an
    /// evaluation fails, as the fork; an [`SolidError::Unported`] is kept and passed on
    /// only when the end is consulted).
    pub faces_tangent: [Result<bool>; 2],
}

/// Whether end `end` (0: the start, 1: the end) of `curve` is a zero-speed tangent
/// end (the fork's `sm_IsZeroSpeedTangentEnd`, WR-668): its two end control points
/// coincide to `scaled_zero` and its second derivative there does not vanish, and
/// the end is a vertex of its edge at which the edge's faces are tangent.
///
/// # Errors
///
/// A [`SolidError::Unported`] of the edge's face evaluations.
fn is_zero_speed_tangent_end(
    curve: &BSplineCurve,
    end: usize,
    scaled_zero: f64,
    edge_ends: Option<&EdgeEnds>,
) -> Result<bool> {
    let Some(cur) = curve.nurb.as_ref() else {
        return Ok(false);
    };
    if cur.n() < 2 {
        return Ok(false);
    }
    let n = cur.n();
    let (i_end, i_next) = if end == 0 { (0, 1) } else { (n, n - 1) };
    let pw = cur.cpts();
    let (Some(&p_end), Some(&p_next)) = (pw.get(i_end), pw.get(i_next)) else {
        return Ok(false);
    };
    let cpt_dist = dist_cpt_cpt(p_end, p_next);
    if !is_zero_to_tol(cpt_dist, scaled_zero) {
        return Ok(false);
    }

    // the second derivative there (`sPV[2].Length() <= dScaledZero` refuses the end, so
    // a NaN length does not)
    let ivl = curve.natural_interval();
    let param = if end == 0 { ivl.min() } else { ivl.max() };
    match curve.evaluate(param, 2, end == 0, false) {
        Ok(pv) => match pv.get(2).map(|d2| d2.length().partial_cmp(&scaled_zero)) {
            Some(Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)) | None => {
                return Ok(false)
            }
            Some(_) => {}
        },
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        Err(_) => return Ok(false),
    }

    // the end is a vertex of the curve's edge
    let Some(edge) = edge_ends else {
        return Ok(false);
    };
    let edge_end = if end == 0 {
        edge.interval.min()
    } else {
        edge.interval.max()
    };
    if (edge_end - param).abs() > EFF_ZERO * (1.0 + ivl.length()) {
        return Ok(false);
    }

    // the edge's faces tangent there
    match edge.faces_tangent.get(end) {
        Some(Ok(t)) => Ok(*t),
        Some(Err(e)) => Err(e.clone()),
        None => Ok(false),
    }
}

/// The stretch from a zero-speed end `end` (0: the start, 1: the end) of `curve`
/// within which its radius of curvature is below `radius_tol` by its growth there,
/// `r = rate |t - t_end|`, `rate = 2 |C''|^3 / |C'' x C'''|` (the fork's
/// `sm_ZeroSpeedRadiusStretch`, WR-668): `radius_tol / rate`. `None` when the radius
/// does not grow so: `C'' x C'''` vanishes at the end, the stretch is not shorter
/// than the curve, or at the stretch's far end the radius is off `radius_tol` by more
/// than half of it.
///
/// # Errors
///
/// A [`SolidError::Unported`] of the evaluations (none for a B-spline).
fn zero_speed_radius_stretch(
    curve: &BSplineCurve,
    end: usize,
    radius_tol: f64,
) -> Result<Option<f64>> {
    let ivl = curve.natural_interval();
    let param = if end == 0 { ivl.min() } else { ivl.max() };
    let pv = match curve.evaluate(param, 3, end == 0, false) {
        Ok(pv) => pv,
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        Err(_) => return Ok(None),
    };
    let (Some(&d2), Some(&d3)) = (pv.get(2), pv.get(3)) else {
        return Ok(None);
    };
    let cross = d2.cross(d3).length();
    let d2_len = d2.length();
    if cross <= EFF_ZERO * d2_len * d2_len {
        return Ok(None);
    }
    let stretch = radius_tol * cross / (2.0 * d2_len * d2_len * d2_len);
    // (`!(stretch < length)`: a NaN stretch is no stretch)
    if stretch.partial_cmp(&ivl.length()) != Some(std::cmp::Ordering::Less) {
        return Ok(None);
    }
    let at_param = if end == 0 {
        param + stretch
    } else {
        param - stretch
    };
    let at = match curve.evaluate(at_param, 2, true, false) {
        Ok(at) => at,
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        Err(_) => return Ok(None),
    };
    let (Some(&a1), Some(&a2)) = (at.get(1), at.get(2)) else {
        return Ok(None);
    };
    let speed = a1.length();
    let at_cross = a1.cross(a2).length();
    if at_cross.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
        return Ok(None);
    }
    if (speed * speed * speed / at_cross - radius_tol).abs() > 0.5 * radius_tol {
        return Ok(None);
    }
    Ok(Some(stretch))
}

/// The most dyadic pieces [`stretch_clears_on_surface`] certifies a zero-speed stretch in:
/// out to `2^-52` of the stretch from its end.
const STRETCH_PIECES: i32 = 52;

/// The UV extent, in units of `ε` times the trim's largest control point, that the sliver
/// [`stretch_clears_on_surface`] leaves to a certified extent bound instead of the radius
/// bound may have: the radius enclosure's coefficients carry absolute widths of a few
/// `ε |P|` (the trim's own rounding), and a piece whose image shrinks to that cannot clear.
const STRETCH_SLIVER_ULPS: f64 = 1024.0;

/// Not upstream: whether the image on `surface` of the trim `trim` has, over the
/// zero-speed stretch of length `stretch` at its end `end` (0: the start, 1: the end of
/// `domain`), a radius of curvature of at least half the growth line WR-668 reads there,
/// `r >= (radius_tol / stretch) |t - t_end| / 2`, at each piece's near edge (below),
/// except within a sliver at the end whose extent is certified at the trim's rounding.
///
/// **The sliver.** It runs from the end out to `cut`, the largest of
/// - `ulp(t_end)` and `stretch 2^-52`: nearer the end, the pieces would round onto the
///   end itself;
/// - the parameter length over which the trim `c` covers a UV length of `L / 4`,
///   `L = STRETCH_SLIVER_ULPS ε |P|_max`, to second order (`|c'| t + |c''| t² / 2 = L / 4`
///   at the end). The radius enclosure's coefficients carry absolute widths of a few
///   `ε |P|`, so a piece whose image is that short cannot clear (MEASURED: on the cubic of
///   `a_genuine_zero_speed_end_clears_its_stretch`, pieces from about 1e-7 of the end
///   inward do not clear even an eighth of the line);
///
/// capped at the first knot from the end, so that the sliver is one polynomial piece (the
/// cap lets a stretch whose first knot lies inside the uncapped cut be certified at all:
/// `a_zero_speed_end_with_a_knot_inside_its_sliver_clears`). Its extent is then
/// certified: [`sliver_within`] bounds its UV diameter from all of its power coefficients
/// in rounded intervals, at most `L`, at the trim's own coordinate rounding; its 3D
/// diameter is bounded by that times an enclosure of the surface's Jacobian over the
/// sliver's UV box (so at most the image of the trim's coordinate rounding). Otherwise (for instance where `ulp(t_end)` binds and the trim moves farther
/// than `L` within it) the stretch is not certified. Within the sliver the radius vanishes
/// with the speed, and no positive tolerance can clear it; that an image at the image of
/// the trim's own rounding holds no corner the trim could represent is ARGUED.
///
/// **The pieces.** From `cut` out, the stretch is cut into the dyadic pieces at
/// `stretch 2^-(j+1)` to `stretch 2^-j` from the end, the last one's near edge at `cut`
/// (at most [`STRETCH_PIECES`] of them). On each, [`radius_clears_on_surface`] must clear
/// half the growth line's value at the piece's near edge (`radius_tol 2^-(j+2)` on a whole
/// dyadic piece; a quarter of the line's value at its far edge). This is a proposed
/// DEFINITION of the defect inside a zero-speed stretch (the maintainer's decision): a
/// radius below that is reported (`a_radius_below_half_the_growth_line_is_reported`: 0.4
/// times the line). A genuine zero-speed end has its radius on the line to first order, so the full line would leave
/// it no margin at the near edges. It is looser than WR-668's own check, which accepts the
/// radius at the stretch's far end within half the tolerance of the line (at least
/// `radius_tol / 2` there), where the outermost piece asks for `radius_tol / 4`.
///
/// `false` at the first piece that does not clear, for a sliver that is not certified,
/// and for a stretch that is not finite and positive or an end that is not finite.
///
/// # Errors
///
/// Those of [`radius_clears_on_surface`] and [`sliver_within`].
fn stretch_clears_on_surface(
    k: &Kernel,
    trim: &BSplineCurve,
    surface: &Surface,
    domain: &Extent1d,
    end: usize,
    stretch: f64,
    radius_tol: f64,
) -> Result<bool> {
    let t_end = if end == 0 { domain.min() } else { domain.max() };
    // the spacing of doubles above |t_end| (`next_up` is past the MSRV)
    let mag = t_end.abs();
    let ulp = f64::from_bits(mag.to_bits().saturating_add(1)) - mag;
    let Ok((points, _)) = trim.control_polygon() else {
        return Ok(false);
    };
    let p_max = points.iter().fold(0.0_f64, |m, p| m.max(p.length()));
    let (speed, accel) = match trim.evaluate(t_end, 2, end == 0, false) {
        Ok(d) => (
            d.get(1).map_or(f64::NAN, |a| a.length()),
            d.get(2).map_or(f64::NAN, |a| a.length()),
        ),
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        Err(_) => return Ok(false),
    };
    let Ok((knots, _)) = trim.knots(None) else {
        return Ok(false);
    };
    // the first knot from the end, inside the domain (the domain's other end if none)
    let knot = if end == 0 {
        knots
            .iter()
            .copied()
            .find(|&x| x > t_end && x < domain.max())
            .unwrap_or_else(|| domain.max())
    } else {
        knots
            .iter()
            .rev()
            .copied()
            .find(|&x| x < t_end && x > domain.min())
            .unwrap_or_else(|| domain.min())
    };
    let cap = (knot - t_end).abs();
    // the positive root of `|c''| t² / 2 + |c'| t = L / 4`, in the form that does not cancel
    let length = STRETCH_SLIVER_ULPS * f64::EPSILON * p_max;
    let quarter = 0.25 * length;
    let sliver = 2.0 * quarter / (speed + (speed * speed + 2.0 * accel * quarter).sqrt());
    let wanted = max3(ulp, stretch * 2f64.powi(-STRETCH_PIECES), sliver);
    // (written to accept: a NaN or infinite stretch, end, sliver or knot certifies nothing)
    let finite = stretch > 0.0
        && stretch.is_finite()
        && ulp.is_finite()
        && sliver.is_finite()
        && wanted.is_finite()
        && cap > 0.0
        && cap.is_finite();
    if !finite {
        return Ok(false);
    }
    let capped = wanted >= cap;
    let cut = if capped { cap } else { wanted };
    // the sliver's inner edge: the knot itself where the cap binds
    let edge = match (capped, end) {
        (true, _) => knot,
        (false, 0) => t_end + cut,
        (false, _) => t_end - cut,
    };
    let (slo, shi) = if end == 0 {
        (t_end, edge)
    } else {
        (edge, t_end)
    };
    if !sliver_within(k, trim, surface, slo, shi, length)? {
        return Ok(false);
    }
    for j in 0..STRETCH_PIECES {
        let far = stretch * 2f64.powi(-j);
        if far <= cut {
            break;
        }
        let dyadic = stretch * 2f64.powi(-(j + 1));
        let last = dyadic <= cut;
        let near = if last { cut } else { dyadic };
        let (near_t, far_t) = match (last, end) {
            (true, 0) => (edge, t_end + far),
            (true, _) => (edge, t_end - far),
            (false, 0) => (t_end + near, t_end + far),
            (false, _) => (t_end - near, t_end - far),
        };
        let (lo, hi) = if end == 0 {
            (near_t, far_t)
        } else {
            (far_t, near_t)
        };
        let tol = radius_tol * (near / stretch) * 0.5;
        if !radius_clears_on_surface(k, trim, surface, lo, hi, tol)? {
            return Ok(false);
        }
        if last {
            break;
        }
    }
    Ok(true)
}

/// The first fundamental form `[E, F, G]` of `surface` at `uv`, its derivatives taken
/// on the given sides (the fork's `sm_FirstFundamentalForm`, as
/// `topology::brep::validation::pointers` has it for WR-406); `None` where the surface
/// cannot be evaluated.
fn first_fundamental_form(
    k: &Kernel,
    surface: &Surface,
    uv: Point2,
    u_from_left: bool,
    v_from_left: bool,
) -> Result<Option<[f64; 3]>> {
    match surface.evaluate_1st_derivatives(k, uv, u_from_left, v_from_left) {
        Ok((_, du, dv)) => Ok(Some([du.dot(du), du.dot(dv), dv.dot(dv)])),
        Err(e) if e.is_unported_or_cancelled() => Err(e),
        Err(_) => Ok(None),
    }
}

/// Whether a first fundamental form is positive definite: no direction of zero length
/// (not a singular point of its surface) (the fork's `sm_IsPositiveDefinite`).
fn is_positive_definite(efg: [f64; 3]) -> bool {
    efg[0] > 0.0 && efg[0] * efg[2] - efg[1] * efg[1] > 0.0
}

/// The map `(u, v) -> (a u + b v, c v)` that measures a UV trim's steps as lengths on
/// its face's surface (the fork's `sm_TrimMetric`, WR-560): `[a, b, c]`, the Cholesky
/// factor `L` of the surface's first fundamental form (`|L d|^2 = d.G.d`) at the
/// trim's middle, or at the centre of the surface's natural domain where the middle is
/// a singular point. `None` for a curve that is no UV trim of a face (`surface` `None`,
/// a dimension other than 2), an unbounded one, or one whose surface has no metric at
/// either point.
fn trim_metric(
    k: &Kernel,
    trim: &BSplineCurve,
    bounded: bool,
    surface: Option<&Surface>,
    domain: &Extent1d,
) -> Result<Option<[f64; 3]>> {
    let Some(surface) = surface.filter(|_| trim.dim == 2) else {
        return Ok(None);
    };
    if !bounded {
        return Ok(None);
    }
    let form_at = |uv: Point2| -> Result<Option<[f64; 3]>> {
        Ok(first_fundamental_form(k, surface, uv, true, true)?
            .filter(|efg| is_positive_definite(*efg)))
    };
    let mut form = match trim.evaluate_point(domain.evaluate(0.5)) {
        Ok(mid) => form_at(Point2::new(mid.x, mid.y))?,
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        Err(_) => None,
    };
    if form.is_none() {
        let centre = match surface.natural_uv_domain_with_nurb(k) {
            Ok(d) => Some(d.evaluate(0.5, 0.5)),
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) => None,
        };
        if let Some(centre) = centre {
            form = form_at(centre)?;
        }
    }
    let Some(efg) = form else {
        return Ok(None);
    };
    let a = efg[0].sqrt();
    let b = efg[1] / a;
    let c = (efg[0] * efg[2] - efg[1] * efg[1]).sqrt() / a;
    Ok(Some([a, b, c]))
}

/// A copy of `trim` with its control points mapped by [`trim_metric`]'s `l` (the
/// fork's `sm_MappedTrim`): linear, so it maps homogeneous control points as it maps
/// cartesian ones. `None` when the copy has no NURB.
fn mapped_trim(trim: &BSplineCurve, l: [f64; 3]) -> Option<BSplineCurve> {
    let mut copy = trim.clone();
    let nurb = copy.nurb.as_mut()?;
    for pw in nurb.cpts_mut() {
        let (x, y) = (pw.x, pw.y);
        pw.x = l[0] * x + l[1] * y;
        pw.y = l[2] * y;
    }
    Some(copy)
}

/// `SmCurve::IsAnalytic` of a B-spline: the base `FALSE` (`SmBSplineCurve` does not
/// override it; see [`crate::curve::curve::Curve::is_analytic`]).
const fn curve_is_analytic(_curve: &BSplineCurve) -> bool {
    false
}

/// `SmCurve::IsBounded` of a B-spline: the base `TRUE` (`SmBSplineCurve` does not
/// override it; see [`crate::curve::curve::Curve::is_bounded`]).
const fn curve_is_bounded(_curve: &BSplineCurve) -> bool {
    true
}

// ---- helpers ----------------------------------------------------------------------

fn invalid(operation: &'static str, reason: &'static str) -> SolidError {
    SolidError::InvalidInput { operation, reason }
}

/// The `SM_ASSERT_*_REPORT` macros of `AssertValid` (`SM_LEVEL_0` tests, so the
/// predicate always runs): `true` when `passed`; otherwise, with a list, a report for
/// test `test_index` (tolerance `tol`, value `val`, then `set` applied) is added
/// unless an equal one is there.
fn report(
    list: &mut Option<&mut AssertArray>,
    key: ObjectKey,
    test_index: u64,
    passed: bool,
    tol: f64,
    val: f64,
    set: impl FnOnce(&mut AssertReport),
) -> Result<bool> {
    if passed {
        if let Some(l) = list.as_deref_mut() {
            l.set_last_add(false);
        }
        return Ok(true);
    }
    if let Some(l) = list.as_deref_mut() {
        let mut r = AssertReport::new(ReportSpec {
            reporter: Some(key),
            test_index,
            test_class: labels::B_SPLINE_CURVE_TYPE,
            owner: Some(key),
            owner_type: labels::B_SPLINE_CURVE_TYPE,
            owner_type_string: Some("SmBSplineCurve"),
            other: None,
            other_type: 0,
            other_type_string: Some("NULL"),
            tol3d: tol3d(tol),
            value: val,
            list_index: 0,
            message: None,
        })?;
        set(&mut r);
        l.add_unique_delete_duplicate(r);
    }
    Ok(false)
}

/// `SM_ASSERT_BOOLEAN_REPORT`: [`report`] without tolerance and value.
fn bool_report(
    list: &mut Option<&mut AssertArray>,
    key: ObjectKey,
    test_index: u64,
    passed: bool,
) -> Result<bool> {
    report(
        list,
        key,
        test_index,
        passed,
        UNDEF_DOUBLE,
        UNDEF_DOUBLE,
        |_| {},
    )
}

impl BSplineCurve {
    /// A copy whose control points are moved along a blend of `start_vec` and
    /// `end_vec` (by their projection on the chord: `start_vec` at the start point,
    /// `end_vec` at the end point) and rotated by `rotation_deg` about the moved chord
    /// (`CreateByScaleTransRot`).
    ///
    /// # Errors
    ///
    /// `SM_ERR` for a closed curve (end points within `SM_EFF_ZERO`) or without a NURB;
    /// `InvalidInput` (`SM_ERR_INVALID_INPUT`) when the moved end points coincide.
    pub fn create_by_scale_trans_rot(
        &self,
        start_vec: Vec3,
        end_vec: Vec3,
        rotation_deg: f64,
    ) -> Result<Self> {
        let ivl = self.natural_interval();
        let start = self.evaluate_point(ivl.min())?;
        let end = self.evaluate_point(ivl.max())?;

        if start.closer_than(EFF_ZERO, end) {
            return Err(SolidError::sm_err(OP_SCALE));
        }
        let mut res = self.clone();

        let start2 = start + start_vec;
        let end2 = end + end_vec;
        let rot_vec = (end2 - start2).unitize()?;

        let mut rot = Axis2Placement::default();
        rot.rotate_about_axis_at_point(rotation_deg * PI / 180.0, start2, rot_vec);

        let form = if res.is_rational()? {
            ControlPointForm::EuclideanRational
        } else {
            ControlPointForm::NonRational
        };

        // Now Let's go through and project each control point down onto
        // the line and use that to compute a transformation.
        let line_vec = end - start;
        for i in 0..res.number_control_points()? {
            let (pnt, weight) = res.control_point(form, i)?;
            // Parameter contains relative stength of start v.s. end.
            let param = line_closest_point(start, line_vec, pnt);
            // dParam may be outside [0, 1]: the effect of the vectors continues.
            let trans_vec = start_vec + param * (end_vec - start_vec);
            let pnt = pnt + trans_vec; // Move point
            let pnt = rot.transform_point(pnt); // Rotate point

            // Now put point back into curve
            res.set_control_point(form, i, pnt, weight)?;
        }
        Ok(res)
    }

    /// Whether some span has `degree + 1` coincident control points (within `tol`,
    /// `SM_ZONE_TOL_3D` when `tol <= 0`), mapping the whole span to one point
    /// (`HasInternalPole`). `false` without a NURB and for a degenerate curve.
    ///
    /// # Errors
    ///
    /// Those of the degeneracy test; `InvalidInput` for an unclamped knot vector
    /// (upstream reads outside the control points).
    pub fn has_internal_pole(&self, k: &Kernel, tol: f64) -> Result<bool> {
        let mut rtn = false;

        // no work - no BSplineCurve representation
        let Some(cur) = self.nurb.as_ref() else {
            return Ok(false);
        };

        // no work - degenerate curve
        if self.is_degenerate(k, tol, None)? {
            return Ok(false);
        }

        let degree = cur.degree();
        let (unique_knots, mults) = if k.fixes.query_statuses_pass_on {
            let Some(km) = ignored(self.knots(None))? else {
                return Ok(false);
            };
            km
        } else {
            self.knots(None)?
        };
        let mut knot_count = 0usize;

        // pick tolerance
        let tolerance = if tol > 0.0 { tol } else { ZONE_TOL_3D };
        let pw = cur.cpts();
        let cpt = |i: usize| {
            pw.get(i)
                .map(|&p| to_euclid(p))
                .ok_or_else(|| invalid(OP_POLE, "control point index out of range"))
        };

        // for every knot span - seek sets of coincident control points
        let mut ii = 0usize;
        while ii + 1 < unique_knots.len() && !rtn {
            knot_count += mults
                .get(ii)
                .copied()
                .ok_or_else(|| invalid(OP_POLE, "missing multiplicity"))?;

            // the 1st control point whose basis function is nonzero on the span
            let cp_index = knot_count
                .checked_sub(1 + degree)
                .ok_or_else(|| invalid(OP_POLE, "knot vector not clamped"))?;
            let pt = cpt(cp_index)?;

            let mut test = true;
            // for the next Degree number of neighbor control points
            let mut jj = 1usize;
            while jj <= degree && test {
                let pt_u = cpt(cp_index + jj)?;
                test &= pt.closer_than(tolerance, pt_u);
                jj += 1;
            }

            // remember when ivl is degenerate
            rtn |= test;
            ii += 1;
        }
        Ok(rtn)
    }

    /// Whether the curve over `interval` is a line, from `sample_count` (at least 3)
    /// samples and the tolerance `tol` (`IsSegmentLine`): every interior sample within
    /// `tol` of the chord, and its derivative's sideways drift over a sample step
    /// within `tol`. A chord shorter than `tol` only checks the middle sample, and is
    /// never a line (the outputs are set when the middle coincides with the ends).
    ///
    /// # Errors
    ///
    /// `SM_ERR` without a NURB or when an evaluation fails; `InvalidInput` for more
    /// than [`MAX_SEGMENT_SAMPLES`] samples.
    pub fn is_segment_line(
        &self,
        interval: &Extent1d,
        sample_count: u64,
        tol: f64,
    ) -> Result<SegmentLine> {
        if self.nurb.is_none() {
            return Err(SolidError::sm_err(OP_SEG));
        }
        let mut sample_count = sample_count;

        // curve end points
        let start_pt = self.evaluate_point(interval.min())?;
        let end_pt = self.evaluate_point(interval.max())?;

        // Used only if the chord length is substantial
        let chord = end_pt - start_pt;
        let mut unit_chord = Vec3::default(); // set if chord sane

        // If the chord is short wrt the tolerance, use only 3 points
        let chord_length = (start_pt - end_pt).length();
        let mut chord_short = false;

        // for endpoints close together (short lines and closed curves)
        if chord_length < tol {
            sample_count = 3;
            chord_short = true;
        } else {
            // unitize the chord (the status only feeds an assert)
            unit_chord = chord.unitize_or_z();
        }

        // When this curve is of type SmLine
        if self.is_kind_of(LINE_TYPE)? {
            // (SE(SM_ERR) only reports for a short chord)
            return Ok(SegmentLine {
                is_line: true,
                line: Some((start_pt, chord)),
            });
        }

        // Use no less than 3 points
        if sample_count < 3 {
            sample_count = 3;
        }
        // (port bound: upstream samples any count, spinning for absurd ones)
        if sample_count > MAX_SEGMENT_SAMPLES {
            return Err(SolidError::invalid_input(
                OP_SEG,
                "sample count above MAX_SEGMENT_SAMPLES",
            ));
        }

        // Evaluating the curve and its derivative in all internal points
        let mut i = 1u64;
        while i + 1 < sample_count {
            let param = interval.evaluate(i as f64 / (sample_count as f64 - 1.0));
            let pt_dir = self.evaluate(param, 1, true, false)?;
            let p0 = pt_dir.first().copied().unwrap_or(Vec3::UNDEF);
            let p1 = pt_dir.get(1).copied().unwrap_or(Vec3::UNDEF);

            // If the segment is short, check if the midpoint coincided with
            // the start/end points, and decide upon the result
            if chord_short {
                let di = (p0 - start_pt).length();
                if di > tol {
                    return Ok(SegmentLine {
                        is_line: false,
                        line: None,
                    });
                }
                // DANGER: line, degenerating to a single point (SE(SM_ERR) only
                // reports)
                return Ok(SegmentLine {
                    is_line: false,
                    line: Some((start_pt, chord)),
                });
            }

            // the chord is long now: check the distance between the curve point
            // and the assumed line
            let (dist, _) = line_point_distance(start_pt, chord, p0);
            if dist > tol {
                return Ok(SegmentLine {
                    is_line: false,
                    line: None,
                });
            }

            let deriv_len = p1.length();

            // we have no other clue wrt the world's resolution than the eps given to us
            if deriv_len > tol {
                let sin_angle = p1.cross(unit_chord).length() / deriv_len;
                let discr = sin_angle.abs() * chord_length / sample_count as f64;
                // the derivative should not cause a large departure from the line
                if discr > tol {
                    return Ok(SegmentLine {
                        is_line: false,
                        line: None,
                    });
                }
            }
            i += 1;
        }

        // If we are here, it should be a line
        Ok(SegmentLine {
            is_line: true,
            line: Some((start_pt, chord)),
        })
    }

    /// Control point `index` in `form` and its weight (`GetControlPoint`): through
    /// [`BSplineCurve::control_points_expert`] with a stride of 4 into `[_, _, 1, 1]`;
    /// a 2D curve gets `z = 0` and the weight from the third slot.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for an index above `n` or a dimension below 2; `SM_ERR` without
    /// a NURB.
    pub fn control_point(&self, form: ControlPointForm, index: usize) -> Result<(Point3, f64)> {
        // Default for non-rational, 2d or 3d.
        let mut tmp = [0.0, 0.0, 1.0, 1.0];
        let vals = self.control_points_expert(form, index, index, 4)?;
        // the slots GetControlPointsExpert writes (the others keep their defaults)
        let written =
            2 + usize::from(self.dim > 2) + usize::from(form != ControlPointForm::NonRational);
        for (slot, v) in tmp.iter_mut().zip(&vals).take(written) {
            *slot = *v;
        }
        let [x, y, z, w] = tmp;
        let mut control_point = Vec3::new(x, y, z);
        let mut weight = w;
        if self.dim == 2 {
            control_point.z = 0.0;
            weight = z;
        }
        if vals.len() < written {
            return Err(invalid(OP_CP, "control point values missing"));
        }
        Ok((control_point, weight))
    }

    /// Whether the curve over `interval` (the natural interval for `None`) lies within
    /// `tol` of the plane through `plane_point` with normal `plane_normal`
    /// (`IsOnPlane`): its start, middle and end points, then (unless it is a line)
    /// the control points of the spans covering the interval.
    ///
    /// # Errors
    ///
    /// `SM_ERR` without a NURB, when an evaluation fails or for a non-unit normal
    /// (`smgu_PlanePointDistance`); `GetPolygon`'s errors.
    pub fn is_on_plane(
        &self,
        plane_point: Point3,
        plane_normal: Vec3,
        tol: f64,
        interval: Option<&Extent1d>,
    ) -> Result<OnPlane> {
        let iv = match interval {
            Some(i) => *i,
            None => self.natural_interval(),
        };

        // get Curve start, mid, end points
        let start_pt = self.evaluate_point(iv.min())?;
        let end_pt = self.evaluate_point(iv.max())?;
        let mid_pt = self.evaluate_point(iv.evaluate(0.5))?;

        // project points to plane
        let proj_start_pt = point_project_to_plane(start_pt, plane_point, plane_normal);
        let proj_end_pt = point_project_to_plane(end_pt, plane_point, plane_normal);
        let proj_mid_pt = point_project_to_plane(mid_pt, plane_point, plane_normal);

        // get start, mid, end point distances to plane
        let st_discr = (start_pt - proj_start_pt).length();
        let end_discr = (end_pt - proj_end_pt).length();
        let mid_discr = (mid_pt - proj_mid_pt).length();

        // low work - points not on plane
        if st_discr > tol || end_discr > tol || mid_discr > tol {
            return Ok(OnPlane {
                on_plane: false,
                max_dist: None,
            });
        }

        // get max of 3 sample point distances - good enough for lines
        let mut max_dist = max3(st_discr, mid_discr, end_discr);

        // for nonLine curves - get max distance to controlPoint
        if !self.is_kind_of(LINE_TYPE)? {
            for pnt in self.polygon(&iv)? {
                let dist = plane_point_distance(plane_point, plane_normal, pnt)?;
                if dist > max_dist {
                    max_dist = dist;
                }
            }
        }

        Ok(OnPlane {
            on_plane: max_dist < tol,
            max_dist: Some(max_dist),
        })
    }

    /// Joins `other` onto this curve (`JoinWith`): this curve's end (`this_at_end`) to
    /// the other's end (`other_at_end`), which must be within `gap_tolerance` (by
    /// default `1000 * SM_EFF_ZERO_SQRT` times the approximate lengths). Two lines that
    /// continue each other become one line segment; otherwise the curves are joined
    /// with [`BSplineCurve::create_by_joining`] (which may change `other` in place).
    ///
    /// # Errors
    ///
    /// `SM_ERR` for ends too far apart or when the join fails or without a NURB;
    /// [`SolidError::Unported`] from `IsLine`'s back-edges.
    pub fn join_with(
        &mut self,
        k: &Kernel,
        this_at_end: bool,
        other: &mut Self,
        other_at_end: bool,
        gap_tolerance: Option<f64>,
    ) -> Result<()> {
        let ivl1 = self.natural_interval();
        let ivl2 = other.natural_interval();
        let end_this = f64::from(u8::from(this_at_end));
        let end_other = f64::from(u8::from(other_at_end));

        // Get a tolerance to use.
        let tol = if let Some(t) = gap_tolerance {
            t
        } else {
            let length =
                approximate_length(k, self, &ivl1, 10)? + approximate_length(k, other, &ivl2, 10)?;
            length * EFF_ZERO_SQRT * 1000.0
        };

        // Make sure that the points of join ends are close enough
        let pnt1 = self.evaluate_point(ivl1.evaluate(end_this))?;
        let pnt2 = other.evaluate_point(ivl2.evaluate(end_other))?;
        if pnt1.distance(pnt2) > tol {
            return Err(SolidError::sm_err(OP_JOIN));
        }

        // First let's handle the simple case of two lines with the same vector.
        if self.is_line(5, tol)?.is_some() && other.is_line(5, tol)?.is_some() {
            // Create a line from the non-joining ends and test the other points.
            let pnt1b = self.evaluate_point(ivl1.evaluate(1.0 - end_this))?;
            let pnt2b = other.evaluate_point(ivl2.evaluate(1.0 - end_other))?;
            let mut can_join_lines = false;
            let (dist, _) = segment_point_distance(pnt2b, pnt1b, pnt1);
            if dist < tol {
                // Only can join continuous lines.
                let (dist, _) = segment_point_distance(pnt2b, pnt1b, pnt2);
                if dist < tol {
                    can_join_lines = true;
                }
            }
            if can_join_lines {
                // (upstream's `SM_CAST_NONNULL_PTR(SmLine, this)` branch extends an
                // `SmLine`'s STEP interval; a `BSplineCurve` is never a line here, lines
                // join through `crate::curve::line::Line::join_with`)
                let new_line = if this_at_end {
                    Self::create_line_segment(self.dim, pnt1b, pnt2b)?
                } else {
                    Self::create_line_segment(self.dim, pnt2b, pnt1b)?
                };
                return self.set_from_gw_nurb(new_line.gw_nurb());
            }
        }

        // Try using nurb-join: the segments in join order and their senses
        let this_curve = std::mem::replace(self, Self::new(self.dim));
        let other_curve = std::mem::replace(other, Self::new(other.dim));
        let (mut curves, orients, this_first) = if this_at_end {
            ([this_curve, other_curve], [true, !other_at_end], true)
        } else {
            ([other_curve, this_curve], [other_at_end, true], false)
        };
        let composite = Self::create_by_joining(&mut curves, Some(&orients));
        let [a, b] = curves;
        let (this_back, other_back) = if this_first { (a, b) } else { (b, a) };
        *self = this_back;
        *other = other_back;
        let composite = match composite {
            Ok(c) => c,
            Err(e @ (SolidError::Unported { .. } | SolidError::Cancelled { .. })) => return Err(e),
            Err(_) => return Err(SolidError::sm_err(OP_JOIN)),
        };
        self.set_from_gw_nurb(composite.gw_nurb())
    }

    /// Copies of the curve split at every interior knot whose continuity is at most
    /// `continuity_to_split` (with the angle tolerance `continuity_angle_tol`), each
    /// trimmed to its piece (`SubdivideAtDiscontinuity`); a single copy when nothing
    /// splits it.
    ///
    /// # Errors
    ///
    /// `SM_ERR` without a NURB; those of `SmCurve::EvaluateContinuity`.
    pub fn subdivide_at_discontinuity(
        &self,
        k: &Kernel,
        continuity_to_split: ContinuityType,
        continuity_angle_tol: f64,
    ) -> Result<Vec<Self>> {
        let mut resulting_curves = Vec::new();

        let (knots, _) = self.knots(None)?; // Unique knot vector
        let (_, continuities) = self.calculate_continuities(k, continuity_angle_tol)?;

        // check state
        if knots.len() != continuities.len() {
            return Err(SolidError::sm_err(OP_SUBDIV));
        }

        // init iter params
        let mut start = *knots
            .first()
            .ok_or_else(|| invalid(OP_SUBDIV, "no knots"))?;

        // for every unique interval (every unique knot but the first)
        for (ii, (&current, &cont)) in knots.iter().zip(&continuities).enumerate().skip(1) {
            // for internal discontinuites or the last segment
            if ii == knots.len() - 1 || (cont as i32) <= (continuity_to_split as i32) {
                // copy the curve
                let mut copy = self.clone();

                // trim it to the interval between discontinuities (may be the whole
                // curve; may snap the interval to existing knots)
                let mut trim = Extent1d::from_min_max(start, current);
                // (status ignored; passed on with the opt-in `query_statuses_pass_on`,
                // tangency-labs/smlib 8843967)
                ignore_or_pass_on(
                    copy.trim(k, &mut trim, TrimMode::Notify),
                    k.fixes.query_statuses_pass_on,
                )?;

                resulting_curves.push(copy);

                // update iter param
                start = current;
            }
        }
        Ok(resulting_curves)
    }

    /// Closes a gap below `d_min` between the closest ends of this curve and `other`
    /// (`TrimGap`): both curves are extended (G1, by 3% of their domain) past those
    /// ends and intersected, and the end control points are moved per `fix` (to the
    /// intersection, or to one curve's end). `false` when the gap is too large or the
    /// extensions do not intersect; `true` when there was no gap or it was closed.
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] when the intersection of the extended curves reaches
    /// an unported unit; `InvalidInput` when an
    /// end point cannot be compared (NaN) or an extension is missing (upstream crashes).
    pub fn trim_gap(
        &mut self,
        k: &Kernel,
        other: &mut Self,
        fix: GapFix,
        d_min: f64,
    ) -> Result<bool> {
        // only close gap if less than dMin
        let d_min_sq = d_min * d_min;

        // with the opt-in fix each failed step leaves the gap unfixed (returns 0), as the
        // fork has it (tangency-labs/smlib 8843967); upstream goes on with unset values
        let soft = k.fixes.query_statuses_pass_on;
        macro_rules! or_unfixed {
            ($r:expr, $default:expr) => {
                match ignored($r)? {
                    Some(v) => v,
                    None if soft => return Ok(false),
                    None => $default,
                }
            };
        }

        // curve 0 start and end points
        let mut extent0 = self.natural_interval();
        let t0size = extent0.max() - extent0.min();
        let s0 = or_unfixed!(self.evaluate_point(extent0.min()), Vec3::UNDEF);
        let e0 = or_unfixed!(self.evaluate_point(extent0.max()), Vec3::UNDEF);

        // curve 1 start and end points
        let mut extent1 = other.natural_interval();
        let t1size = extent1.max() - extent1.min();
        let s1 = or_unfixed!(other.evaluate_point(extent1.min()), Vec3::UNDEF);
        let e1 = or_unfixed!(other.evaluate_point(extent1.max()), Vec3::UNDEF);

        // Extend curves based on closest endpoints by domain * 0.3
        let d_s0s1 = (s0 - s1).length_squared();
        let d_s0e1 = (s0 - e1).length_squared();
        let d_e0s1 = (e0 - s1).length_squared();
        let d_e0e1 = (e0 - e1).length_squared();
        let dmin = min2(min2(d_s0s1, d_s0e1), min2(d_e0s1, d_e0e1));

        // Test if Gap is greater than dMin
        if dmin > d_min_sq {
            return Ok(false); // gap is too large to fix
        }
        if dmin < EFF_ZERO {
            return Ok(true); // curves have no gap
        }

        // figure out which end points to move
        let ext_case = if dmin >= d_s0s1 {
            1
        } else if dmin >= d_s0e1 {
            2
        } else if dmin >= d_e0s1 {
            3
        } else if dmin >= d_e0e1 {
            4
        } else {
            0
        };
        let start0 = ext_case == 1 || ext_case == 2;
        let end0 = ext_case == 3 || ext_case == 4;
        let start1 = ext_case == 1 || ext_case == 3;
        let end1 = ext_case == 2 || ext_case == 4;

        // extend the input curves past the ends with the smallest gaps
        let extend = |c: &Self, t: f64| c.create_extended_curve(t, ContinuityType::G1, true);
        let mut curve0e = None;
        let mut curve1e = None;
        if start0 {
            curve0e = or_unfixed!(extend(self, extent0.min() - 0.03 * t0size), None);
        }
        if end0 {
            curve0e = or_unfixed!(extend(self, extent0.max() + 0.03 * t0size), None);
        }
        if start1 {
            curve1e = or_unfixed!(extend(other, extent1.min() - 0.03 * t1size), None);
        }
        if end1 {
            curve1e = or_unfixed!(extend(other, extent1.max() + 0.03 * t1size), None);
        }
        let (Some(mut curve0e), Some(mut curve1e)) = (curve0e, curve1e) else {
            return Err(invalid(OP_GAP, "curve extension missing"));
        };

        // intersect the extended curves
        let solutions = global_curve_intersect(
            k,
            &curve0e,
            &curve0e.natural_interval(),
            &curve1e,
            &curve1e.natural_interval(),
            dmin * 5.0,
        )?;

        // failure - the extended curves don't intersect
        if solutions.is_empty() {
            return Ok(false);
        }

        // for every intersection
        for (solution_type, start) in &solutions {
            if *solution_type == SolutionType::SingleValue {
                let t0 = start.param(0)?; // 1st curve intersection param
                let t1 = start.param(1)?; // 2nd curve intersection param
                extent0 = curve0e.natural_interval();
                extent1 = curve1e.natural_interval();

                // Trim the domain of the two curves to the intersection
                let tmin0 = if start0 { t0 } else { extent0.min() };
                let tmax0 = if end0 { t0 } else { extent0.max() };
                let tmin1 = if start1 { t1 } else { extent1.min() };
                let tmax1 = if end1 { t1 } else { extent1.max() };

                let mut ext0 = Extent1d::from_min_max(tmin0, tmax0);
                let mut ext1 = Extent1d::from_min_max(tmin1, tmax1);
                or_unfixed!(curve0e.trim(k, &mut ext0, TrimMode::Notify), ());
                or_unfixed!(curve1e.trim(k, &mut ext1, TrimMode::Notify), ());

                // only do this once
                break;
            }
        }

        // Now lets average the two trimmed endpoints
        extent0 = curve0e.natural_interval(); // after trimming
        extent1 = curve1e.natural_interval();

        let mut p0 = Vec3::UNDEF;
        let mut p1 = Vec3::UNDEF;
        if start0 {
            p0 = or_unfixed!(curve0e.evaluate_point(extent0.min()), Vec3::UNDEF);
        }
        if end0 {
            p0 = or_unfixed!(curve0e.evaluate_point(extent0.max()), Vec3::UNDEF);
        }
        if start1 {
            p1 = or_unfixed!(curve1e.evaluate_point(extent1.min()), Vec3::UNDEF);
        }
        if end1 {
            p1 = or_unfixed!(curve1e.evaluate_point(extent1.max()), Vec3::UNDEF);
        }

        // the new common end point
        let mid = match fix {
            GapFix::ToThis => {
                if start0 {
                    s0
                } else {
                    e0
                }
            }
            GapFix::ToOther => {
                if start1 {
                    s1
                } else {
                    e1
                }
            }
            GapFix::Both => 0.5 * (p0 + p1),
        };

        // replace the endpoint of the original curves
        let form = ControlPointForm::NonRational;
        if start0 {
            or_unfixed!(self.set_control_point(form, 0, mid, 1.0), ());
        }
        if end0 {
            let last = self.number_control_points()?.wrapping_sub(1);
            or_unfixed!(self.set_control_point(form, last, mid, 1.0), ());
        }
        if start1 {
            or_unfixed!(other.set_control_point(form, 0, mid, 1.0), ());
        }
        if end1 {
            let last = other.number_control_points()?.wrapping_sub(1);
            or_unfixed!(other.set_control_point(form, last, mid, 1.0), ());
        }

        Ok(true) // success...we have modified the curves
    }

    /// The validity checks of the curve (`AssertValid`), reporting failures for `key`
    /// into `list`: the base curve checks (unless `level_given`), a NURB (test 0), end
    /// knot multiplicities `p + 1` (1), interior ones at most `p` (2), consistent knot
    /// and control point counts (16), coincident end control points (3, 4), and for a
    /// curve neither degenerate nor analytic: zero derivatives (5, 6) and kinks (7, 8)
    /// at interior knots of multiplicity `p`, vanishing first derivatives (11, 12),
    /// tiny radii of curvature (13, 14); then coincident interior control points (15).
    /// `true` when every check passes.
    ///
    /// # Errors
    ///
    /// Past the knot checks those of `SmCurve::GlobalPropertyAnalysis`. `k` gives the system zone tolerance of
    /// `SmTol::IsDegenerate(this)`.
    pub fn assert_valid(
        &self,
        k: &Kernel,
        list: Option<&mut AssertArray>,
        level_given: bool,
        key: ObjectKey,
    ) -> Result<bool> {
        let is_degenerate = |tol: f64| self.is_degenerate(k, tol, None);
        self.assert_valid_with(
            k,
            list,
            level_given,
            key,
            &DerivedOverrides {
                degree: None,
                analytic: curve_is_analytic(self),
                bounded: curve_is_bounded(self),
                is_degenerate: &is_degenerate,
                trim_surface: None,
                edge_ends: None,
            },
        )
    }

    /// [`BSplineCurve::assert_valid`] of a UV trim on `trim_surface`, its face's
    /// surface: the radius of curvature (13) and coincident control point (15) tests
    /// measure the trim on that surface (WR-560).
    ///
    /// # Errors
    ///
    /// As [`BSplineCurve::assert_valid`], and those of the surface's evaluations
    /// ([`SolidError::Unported`] only; a failed evaluation leaves the trim unmapped).
    pub fn assert_valid_on(
        &self,
        k: &Kernel,
        list: Option<&mut AssertArray>,
        level_given: bool,
        key: ObjectKey,
        trim_surface: Option<&Surface>,
    ) -> Result<bool> {
        self.assert_valid_on_edge(k, list, level_given, key, trim_surface, None)
    }

    /// [`BSplineCurve::assert_valid_on`] of the 3D curve or a UV trim of an edge whose
    /// ends are `edge_ends` (read for WR-668's zero-speed tangent ends; `None` for a
    /// curve on no edge).
    ///
    /// # Errors
    ///
    /// As [`BSplineCurve::assert_valid_on`].
    pub(crate) fn assert_valid_on_edge(
        &self,
        k: &Kernel,
        list: Option<&mut AssertArray>,
        level_given: bool,
        key: ObjectKey,
        trim_surface: Option<&Surface>,
        edge_ends: Option<&EdgeEnds>,
    ) -> Result<bool> {
        let is_degenerate = |tol: f64| self.is_degenerate(k, tol, None);
        self.assert_valid_with(
            k,
            list,
            level_given,
            key,
            &DerivedOverrides {
                degree: None,
                analytic: curve_is_analytic(self),
                bounded: curve_is_bounded(self),
                is_degenerate: &is_degenerate,
                trim_surface,
                edge_ends,
            },
        )
    }

    /// [`BSplineCurve::assert_valid`] run for a class derived from `SmBSplineCurve`
    /// (an ellipse, ...): the virtual methods `AssertValid` calls come from `ovr`.
    /// The caller makes a missing NURB first (`MakeNurb` is virtual).
    ///
    /// # Errors
    ///
    /// As [`BSplineCurve::assert_valid`].
    pub(crate) fn assert_valid_with(
        &self,
        k: &Kernel,
        list: Option<&mut AssertArray>,
        level_given: bool,
        key: ObjectKey,
        ovr: &DerivedOverrides<'_>,
    ) -> Result<bool> {
        let mut list = list;
        let mut rtn = true;

        // call the base class AssertValid
        rtn &= if level_given {
            true
        } else {
            curve_assert_valid(self, list.as_deref_mut(), key)?
        };

        // (MakeNurb cannot build a missing NURB for a plain B-spline)

        /*  0 */
        // check for nonNULL m_pNurb pointer
        rtn &= bool_report(&mut list, key, 0, self.nurb.is_some())?;
        let Some(cur) = self.nurb.as_ref() else {
            return Ok(rtn);
        };

        // Check knot vector
        let (knots, mults) = self.knots(None)?;
        let num_knots = knots.len();
        let mult = |i: usize| {
            mults
                .get(i)
                .copied()
                .ok_or_else(|| invalid(OP_ASSERT, "missing multiplicity"))
        };

        /*  1 */
        // check end knots for expected multiplicity count == Degree + 1
        let deg = match ovr.degree {
            Some(d) => d,
            None => self.degree()?,
        };
        let ends_ok = mult(0)? == deg + 1 && mult(num_knots.wrapping_sub(1))? == deg + 1;
        rtn &= bool_report(&mut list, key, 1, ends_ok)?;
        if !rtn {
            return Ok(rtn);
        }

        /*  2 */
        // check internal knots for multiplicty count limit <= Degree
        for idx in 1..num_knots.saturating_sub(1) {
            let ok = mult(idx)? <= deg;
            rtn &= bool_report(&mut list, key, 2, ok)?;
        }
        if !rtn {
            return Ok(rtn);
        }

        /* 16 */
        // check number of knots consistent with number of ControlPoints and degree
        let counts_ok = cur.m() + 1 == cur.n() + 1 + cur.degree() + 1;
        rtn &= bool_report(&mut list, key, 16, counts_ok)?;
        if !rtn {
            return Ok(rtn);
        }

        // check for coincident end control points when the curve is not degenerate
        let degenerate = tol_is_degenerate(k, ovr.is_degenerate)?;
        let pw = cur.cpts();
        let cpt = |i: usize| {
            pw.get(i)
                .copied()
                .ok_or_else(|| invalid(OP_ASSERT, "control point index out of range"))
        };
        if !degenerate {
            // for now - allow endPair ControlPoint coincidence, but output a warning

            // check for start ControlPoint coincidence
            let euclid = cpt(0)?.to_euclid();
            let start_scaled_zero = EFF_ZERO * (1.0 + euclid.max_dimension());
            let start_cpt_dist = dist_cpt_cpt(cpt(0)?, cpt(1)?);

            // not upstream (WR-668): not at a zero-speed tangent end, whose first two
            // control points coincide by construction (the fork's ecf1963)
            if is_zero_to_tol(start_cpt_dist, start_scaled_zero)
                && !is_zero_speed_tangent_end(self, 0, start_scaled_zero, ovr.edge_ends)?
            {
                /*  3 */
                let ok = !is_zero_to_tol(start_cpt_dist, start_scaled_zero);
                rtn &= report(
                    &mut list,
                    key,
                    3,
                    ok,
                    start_scaled_zero,
                    start_cpt_dist,
                    |_| {},
                )?;
            }

            // check for end ControlPoint coincidence
            let i_n = cur.n();
            let euclid = cpt(i_n)?.to_euclid();
            let end_scaled_zero = EFF_ZERO * (1.0 + euclid.max_dimension());
            let end_cpt_dist = dist_cpt_cpt(cpt(i_n.wrapping_sub(1))?, cpt(i_n)?);
            if is_zero_to_tol(end_cpt_dist, end_scaled_zero) {
                // (upstream tests and reports the start pair's values here)
                let ok = !is_zero_to_tol(start_cpt_dist, start_scaled_zero);
                rtn &= report(
                    &mut list,
                    key,
                    4,
                    ok,
                    start_scaled_zero,
                    start_cpt_dist,
                    |_| {},
                )?;
            }
        }

        // Check for kinks at internal knot multiplicities; skip kink and zero
        // internal 1st derivative checks for degenerate and analytic curves.
        let analytic = ovr.analytic;

        // for debug - data at failing tests
        let dim = self.dim;
        if !degenerate && !analytic {
            // for non degenerate curves
            let mut idx = 1usize;
            while idx < num_knots.saturating_sub(1) && rtn {
                let i = idx;
                idx += 1;

                // skip knots with low mults that guarantee C1 or better continuity
                if mult(i)? < deg {
                    continue;
                }

                // Lower interval evaluation (status ignored upstream; fails test 5/6
                // with `query_statuses_pass_on`, tangency-labs/smlib 8843967)
                let param = knots.get(i).copied().unwrap_or(UNDEF_DOUBLE);
                let eval1 = ignored(self.evaluate(param, 1, false, false))?;
                let (v1_len, unit1) = match eval1 {
                    Some(v) => {
                        let dir = v.get(1).copied().unwrap_or(Vec3::UNDEF);
                        (dir.length(), dir.unitize().ok())
                    }
                    None if k.fixes.query_statuses_pass_on => (UNDEF_DOUBLE, None),
                    None => (Vec3::UNDEF.length(), Vec3::UNDEF.unitize().ok()),
                };
                let Some(u1) = unit1 else {
                    let test = if dim == 2 {
                        Some(5)
                    } else if dim == 3 {
                        Some(6)
                    } else {
                        None
                    };
                    if let Some(t) = test {
                        rtn &= report(&mut list, key, t, false, EFF_ZERO, UNDEF_DOUBLE, |r| {
                            r.set_owner_param(Point2::new(param, 0.0));
                            r.set_value(v1_len);
                        })?;
                    }
                    // skip further checks - only need to report the 1st problem
                    continue;
                };

                // upper interval eval
                let eval2 = ignored(self.evaluate(param, 1, true, false))?;
                let (v2_len, unit2) = match eval2 {
                    Some(v) => {
                        let dir = v.get(1).copied().unwrap_or(Vec3::UNDEF);
                        (dir.length(), dir.unitize().ok())
                    }
                    None if k.fixes.query_statuses_pass_on => (UNDEF_DOUBLE, None),
                    None => (Vec3::UNDEF.length(), Vec3::UNDEF.unitize().ok()),
                };
                let Some(u2) = unit2 else {
                    let test = if dim == 2 {
                        Some(5)
                    } else if dim == 3 {
                        Some(6)
                    } else {
                        None
                    };
                    if let Some(t) = test {
                        report(&mut list, key, t, false, EFF_ZERO, UNDEF_DOUBLE, |r| {
                            r.set_owner_param(Point2::new(param, 0.0));
                            r.set_value(v2_len);
                        })?;
                    }
                    rtn = false;
                    // skip further checks - only need to report the 1st problem
                    continue;
                };

                // Check the angle between the unitized 1st derivatives
                let dot = u1.dot(u2);

                // when angle between lower and upper ivl 1st Derivs is too big - not G1
                if dot < COSINE_TOL {
                    let max_sin_ang_rad_param = param;
                    let test = if dim == 2 {
                        Some(7)
                    } else if dim == 3 {
                        Some(8)
                    } else {
                        None
                    };
                    if let Some(t) = test {
                        let ok = dot > COSINE_TOL;
                        report(&mut list, key, t, ok, COSINE_TOL, dot, |r| {
                            r.set_owner_param(Point2::new(max_sin_ang_rad_param, 0.0));
                            r.set_value(dot);
                        })?;
                    }
                    rtn = false;
                    // (upstream continues: only the 1st problem is reported)
                }
            }

            // Check for nonregular curve: vanishing first derivative. Allow zero 1st
            // derivatives at the NaturalParameter ends of the curve.

            // Get a representive size (status ignored upstream; with
            // `query_statuses_pass_on`, an unsized curve fails test 11/12 and skips
            // the derivative and radius analyses, tangency-labs/smlib 8843967)
            let domain = self.natural_interval();
            let bbox_opt = ignored(self.calculate_bounding_box(
                &domain,
                BoundingBoxRequest {
                    normal: true,
                    pseudo: false,
                    polar: false,
                },
            ))?
            .and_then(|b| b.normal);
            let (sized, size_xyz) = if k.fixes.query_statuses_pass_on {
                let ok = bbox_opt.is_some();
                rtn &= report(
                    &mut list,
                    key,
                    if dim == 2 { 11 } else { 12 },
                    ok,
                    UNDEF_DOUBLE,
                    UNDEF_DOUBLE,
                    |_| {},
                )?;
                (ok, bbox_opt.map_or(0.0, |b| b.size().length()))
            } else {
                (true, bbox_opt.unwrap_or_default().size().length())
            };
            let size_t = domain.length();

            // finite curves: proportional to the average curve speed; else absolute
            let bounded = ovr.bounded;
            let tol = if bounded {
                (size_xyz / size_t) / 10000.0
            } else {
                1e-03
            };
            let min_sep = if bounded {
                size_xyz / max2(20., size_t)
            } else {
                5.0
            };

            // Not upstream (#2568): where the curve's speed is bounded above the tolerance
            // the search cannot report, and is not run (`analysis`; off without
            // `Kernel::memoize_curve_caches`, and with `query_statuses_pass_on`, which
            // reports the search's own status)
            let shortcut = k.memoize_curve_caches && !k.fixes.query_statuses_pass_on;
            let bounds = if shortcut { curve_bounds(cur) } else { None };
            let speed_clear = bounds.is_some_and(|b| b.speed >= tol * BOUND_MARGIN);
            let solutions = if sized && speed_clear {
                Vec::new()
            } else if sized {
                let (first_derivs_ok, sols) = global_property_analysis(
                    k,
                    self,
                    &domain,
                    CurveProperty::MinimizeFirstDerivative,
                    min_sep,
                )?;
                if k.fixes.query_statuses_pass_on {
                    rtn &= report(
                        &mut list,
                        key,
                        if dim == 2 { 11 } else { 12 },
                        first_derivs_ok,
                        tol,
                        UNDEF_DOUBLE,
                        |_| {},
                    )?;
                }
                sols
            } else {
                Vec::new()
            };

            // The solution value is not necessarily the length of the first
            // derivative, so evaluate it at the solution parameter.
            for start in &solutions {
                // solution parameter value
                let param = start.param(0)?;

                // skip solutions on the natural boundary ends - they are not a problem
                if domain
                    .is_value_on_boundary(param, EFF_ZERO * (1.0 + domain.length()))
                    .0
                {
                    continue;
                }

                // 1stDeriv magnitude
                let eval = ignored(self.evaluate(param, 1, false, false))?;
                let (evaluated, deriv_length) = match eval {
                    Some(pt_der) => (true, pt_der.get(1).copied().unwrap_or(Vec3::UNDEF).length()),
                    None if k.fixes.query_statuses_pass_on => (false, UNDEF_DOUBLE),
                    None => (true, Vec3::UNDEF.length()),
                };

                // check 1stDeriv > tolerance; with `query_statuses_pass_on` a derivative
                // that cannot be evaluated fails too
                if !evaluated || deriv_length < tol {
                    let min_1st_deriv_param = param;
                    let test = if dim == 2 {
                        Some(11)
                    } else if dim == 3 {
                        Some(12)
                    } else {
                        None
                    };
                    if let Some(t) = test {
                        let ok = evaluated && deriv_length >= tol;
                        rtn &= report(&mut list, key, t, ok, tol, deriv_length, |r| {
                            r.set_owner_param(Point2::new(min_1st_deriv_param, 0.0));
                            r.set_value(deriv_length);
                        })?;
                    }
                    // only need to report one problem
                    break;
                }
            }

            // Not upstream (fork tangency-labs/smlib b4f8c57, WR-560, opt-in): a radius of
            // curvature is a length, and a UV trim's lengths are those of its face's
            // surface. The trim is tested mapped by the surface's first fundamental form
            // at its middle, with the mapped trim's tolerance and solution spacing; a
            // mapped trim that cannot be sized fails test 13 and is not analysed.
            let mut mapped: Option<BSplineCurve> = None;
            let (mut radius_sized, mut radius_tol, mut radius_sep) = (sized, tol, min_sep);
            if sized {
                if let Some(l) = trim_metric(k, self, bounded, ovr.trim_surface, &domain)? {
                    mapped = mapped_trim(self, l);
                    let mapped_box = match mapped.as_ref() {
                        Some(m) => ignored(m.calculate_bounding_box(
                            &domain,
                            BoundingBoxRequest {
                                normal: true,
                                pseudo: false,
                                polar: false,
                            },
                        ))?
                        .and_then(|b| b.normal),
                        None => None,
                    };
                    radius_sized = mapped_box.is_some();
                    rtn &= bool_report(&mut list, key, 13, radius_sized)?;
                    if let Some(mapped_box) = mapped_box {
                        let mapped_size = mapped_box.size().length();
                        radius_tol = (mapped_size / size_t) / 10000.0;
                        radius_sep = mapped_size / max2(20., size_t);
                    }
                }
            }
            let copy_curve = mapped.as_ref().unwrap_or(self);

            // Another type of kink check: vanishing radius of curvature.
            let mut radii = false;
            // Not upstream (#2568): where the radius is bounded above the tolerance and the
            // search's objective evaluates everywhere (a failed evaluation is returned as a
            // radius of 0), the search cannot report and is not run
            let radius_clear = shortcut
                && match mapped.as_ref() {
                    None => bounds,
                    Some(m) => m.nurb.as_ref().and_then(curve_bounds),
                }
                .is_some_and(|b| b.radius_clears(radius_tol));
            let solutions = if radius_sized && radius_clear {
                radii = true;
                Vec::new()
            } else if radius_sized {
                let (radii_ok, sols) = global_property_analysis(
                    k,
                    copy_curve,
                    &domain,
                    CurveProperty::MinimizeRadiusOfCurvature,
                    radius_sep,
                )?;
                if k.fixes.query_statuses_pass_on {
                    radii = radii_ok;
                    rtn &= report(
                        &mut list,
                        key,
                        if dim == 2 { 13 } else { 14 },
                        radii,
                        radius_tol,
                        UNDEF_DOUBLE,
                        |_| {},
                    )?;
                    if radii {
                        sols
                    } else {
                        Vec::new()
                    }
                } else {
                    radii = true;
                    sols
                }
            } else {
                Vec::new()
            };

            // (upstream compares before setting the minimum radius, so the test never
            // fires; here it is read first: WR-382, tangency-labs/solidcore#475)
            //
            // Not upstream (fork tangency-labs/smlib ecf1963, WR-668): at a
            // zero-speed tangent end the radius vanishes as |t - t_end|. A solution below
            // the tolerance within the stretch where that growth puts it below is that
            // growth and is passed over; the first other solution is read instead.
            let mut stretch = [-1.0, -1.0];
            if radii {
                for (end, slot) in stretch.iter_mut().enumerate() {
                    let i = if end == 0 { 0 } else { cur.n() };
                    let end_cpt = cpt(i)?.to_euclid();
                    let scaled_zero = EFF_ZERO * (1.0 + end_cpt.max_dimension());
                    if is_zero_speed_tangent_end(self, end, scaled_zero, ovr.edge_ends)? {
                        if let Some(s) = zero_speed_radius_stretch(copy_curve, end, radius_tol)? {
                            *slot = s;
                        }
                    }
                }
            }
            // Not upstream: a UV trim measured on its surface (WR-560) has a small
            // radius only where its image on the surface has one. The constant metric
            // of `trim_metric` is exact on a plane only: near a singular point of the
            // surface (a sphere's pole) it overstates the steps along the collapsing
            // direction, and the exact trim of a smooth curve passing near the pole
            // turns there through up to half a period over a few times the pole's
            // distance. A solution below the tolerance is passed over only where the
            // image on the surface is proved to have a radius of at least the tolerance
            // over the whole trim, its zero-speed end stretches to at least half the
            // growth line they stand for ([`radius_clears_on_surface`],
            // [`stretch_clears_on_surface`]). The bound covers the whole trim, not only
            // the solution's neighbourhood: the analysis finds solutions on the mapped
            // trim, and a corner that is sharp on the surface can read above the
            // tolerance mapped (where a step in u is shorter on the surface than at the
            // trim's middle) and be no solution at all.
            let on_surface = match (mapped.as_ref(), ovr.trim_surface) {
                (Some(_), Some(surface)) => Some(surface),
                _ => None,
            };
            let mut first = solutions.first();
            if stretch.iter().any(|&s| s >= 0.0) || on_surface.is_some() {
                first = None;
                let mut params = Vec::new();
                for s in &solutions {
                    params.push(s.param(0)?);
                }
                // the whole trim, certified once
                let mut whole_clears: Option<bool> = None;
                for (s, &p) in solutions.iter().zip(&params) {
                    let growth = (p - domain.min()).abs() <= stretch[0]
                        || (p - domain.max()).abs() <= stretch[1];
                    let smooth_on_surface = match on_surface {
                        Some(surface) if s.solution_value < radius_tol && !growth => {
                            if let Some(c) = whole_clears {
                                c
                            } else {
                                let lo = domain.min() + max2(stretch[0], 0.0);
                                let hi = domain.max() - max2(stretch[1], 0.0);
                                let mut c =
                                    radius_clears_on_surface(k, self, surface, lo, hi, radius_tol)?;
                                // the zero-speed stretches: on the surface, at least half
                                // the growth line they stand for
                                for (end, &len) in stretch.iter().enumerate() {
                                    if c && len >= 0.0 {
                                        c = stretch_clears_on_surface(
                                            k, self, surface, &domain, end, len, radius_tol,
                                        )?;
                                    }
                                }
                                whole_clears = Some(c);
                                c
                            }
                        }
                        _ => false,
                    };
                    if !(s.solution_value < radius_tol && (growth || smooth_on_surface)) {
                        first = Some(s);
                        break;
                    }
                }
            }
            let first_radius = first.map_or(BIG_DOUBLE, |s| s.solution_value);
            let min_radius = first_radius;
            if min_radius < radius_tol {
                let min_radius_param = match first {
                    Some(s) => s.param(0)?,
                    None => 0.0,
                };
                let test = if dim == 2 {
                    Some(13)
                } else if dim == 3 {
                    Some(14)
                } else {
                    None
                };
                if let Some(t) = test {
                    let ok = min_radius >= radius_tol;
                    report(&mut list, key, t, ok, radius_tol, min_radius, |r| {
                        r.set_owner_param(Point2::new(min_radius_param, 0.0));
                    })?;
                }
                rtn = false;
            }
        }

        /* 15 */
        // Are Curve Internal ControlPoints Coincident
        let pole_tol = ZONE_TOL_3D / 10.0;
        // Not upstream (fork tangency-labs/smlib 54ca3e0, WR-560): the tolerance is
        // a length, and a UV trim's control points are measured as lengths on its face's
        // surface, as the radius test above.
        let mut pole_curve: Option<BSplineCurve> = None;
        let interval = self.natural_interval();
        if let Some(l) = trim_metric(k, self, ovr.bounded, ovr.trim_surface, &interval)? {
            pole_curve = mapped_trim(self, l);
        }
        let has_internal_pole = pole_curve
            .as_ref()
            .unwrap_or(self)
            .has_internal_pole(k, pole_tol)?;
        rtn &= report(
            &mut list,
            key,
            15,
            !has_internal_pole,
            pole_tol,
            UNDEF_DOUBLE,
            |_| {},
        )?;

        Ok(rtn)
    }

    /// Whether the control polygon of this 2D (UV trim) curve stays clear of a jump
    /// across a periodic boundary of the surface domain `surface_uv_domain`
    /// (`AssertNoPeriodicJump`): `false` when two consecutive control points differ by
    /// at least nine tenths of the domain length in a direction the surface is closed
    /// in (`surface_closed_u` / `surface_closed_v`). A curve that is not 2D, a surface
    /// closed in neither direction and a two-point polygon are `true`.
    ///
    /// # Errors
    ///
    /// `SM_ERR` without a NURB (upstream makes one or dereferences `NULL`; a plain
    /// B-spline cannot make one); those of [`BSplineCurve::polygon`].
    pub fn assert_no_periodic_jump(
        &self,
        surface_closed_u: bool,
        surface_closed_v: bool,
        surface_uv_domain: &Extent2d,
    ) -> Result<bool> {
        // return value
        let mut rtn = true;

        // no work - not 2d, not periodic
        if self.dim() != 2 || (!surface_closed_u && !surface_closed_v) {
            return Ok(true);
        }

        // get curve control points in 2d euclidean space (the NURB is made by the
        // caller, `assert_no_periodic_jump_of`, for the kinds that can make one)
        let ivl = self.natural_interval();
        let polygon = self.polygon(&ivl)?;

        // no work - single pair control point curves are assumed to not jump boundaries
        if polygon.len() == 2 {
            return Ok(true);
        }

        // get slightly shortened surface periods
        let period_u = 0.9 * surface_uv_domain.x_length();
        let period_v = 0.9 * surface_uv_domain.y_length();

        // for every pair of control Points
        for pair in polygon.windows(2) {
            let (Some(a), Some(b)) = (pair.first(), pair.get(1)) else {
                continue;
            };
            let jump_u = (b.x - a.x).abs();
            let jump_v = (b.y - a.y).abs();

            // look for a potential bad U Jump
            if surface_closed_u && jump_u >= period_u {
                rtn = false;
            }
            if surface_closed_v && jump_v >= period_v {
                rtn = false;
            }
        }

        // all done
        Ok(rtn)
    }
}

/// `pUVTrimCurve->AssertNoPeriodicJump(bClosedU, bClosedV, rSurfaceUVDomain)` for a
/// curve of any B-spline kind: [`BSplineCurve::assert_no_periodic_jump`] of its
/// `SmBSplineCurve` part, a line's NURB made on a copy first (`GetPolygon`'s
/// `GetOrCreateGwNurbPointer`; upstream makes it on the curve itself).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a curve that is not an `SmBSplineCurve` (upstream's
/// UV trim curves always are); those of [`BSplineCurve::assert_no_periodic_jump`].
pub fn assert_no_periodic_jump_of(
    curve: &Curve,
    surface_closed_u: bool,
    surface_closed_v: bool,
    surface_uv_domain: &Extent2d,
) -> Result<bool> {
    use crate::curve::curve::p2::CurveKind;
    let base = match curve.kind() {
        CurveKind::BSpline(c) => std::borrow::Cow::Borrowed(c),
        CurveKind::Line(c) => c.base_with_nurb()?,
        CurveKind::Ellipse(c) => std::borrow::Cow::Borrowed(c.base()),
        CurveKind::Parabola(c) => std::borrow::Cow::Borrowed(c.base()),
        CurveKind::Hyperbola(c) => std::borrow::Cow::Borrowed(c.base()),
        _ => {
            return Err(SolidError::invalid_input(
                "nurbs::b_spline_curve::p3::assert_no_periodic_jump",
                "the curve is not an SmBSplineCurve",
            ))
        }
    };
    base.assert_no_periodic_jump(surface_closed_u, surface_closed_v, surface_uv_domain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nurbs::math::{CPoint, NOW};
    use crate::nurbs::nurbs_crv::allocate_nurb_curve;
    use std::f64::consts::PI;

    /// A non-rational 3D curve of degree `p` through `pts` with `knots`.
    fn curve(p: usize, pts: &[[f64; 3]], knots: &[f64]) -> BSplineCurve {
        let n = pts.len() - 1;
        let mut cur = allocate_nurb_curve(n, p, knots.len() - 1).unwrap();
        for (d, s) in cur.cpts_mut().iter_mut().zip(pts) {
            *d = CPoint::new(s[0], s[1], s[2], NOW);
        }
        cur.knots_mut().copy_from_slice(knots);
        BSplineCurve::from_nurb(cur, 3, false)
    }

    fn quad() -> BSplineCurve {
        curve(
            2,
            &[
                [0.0, 0.0, 0.0],
                [1.0, 2.0, 0.0],
                [3.0, 2.0, 0.0],
                [4.0, 0.0, 0.0],
            ],
            &[0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0],
        )
    }

    #[test]
    fn control_point_forms() {
        let c = quad();
        let (p, w) = c.control_point(ControlPointForm::NonRational, 1).unwrap();
        assert_eq!(p, Vec3::new(1.0, 2.0, 0.0));
        assert_eq!(w, 1.0);
        let (p, w) = c
            .control_point(ControlPointForm::EuclideanRational, 3)
            .unwrap();
        assert_eq!(p, Vec3::new(4.0, 0.0, 0.0));
        assert_eq!(w, 1.0);
        assert!(c.control_point(ControlPointForm::NonRational, 9).is_err());
    }

    #[test]
    fn scale_trans_rot_moves_ends() {
        let c = quad();
        let r = c
            .create_by_scale_trans_rot(Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 0.0, 1.0), 0.0)
            .unwrap();
        let (p, _) = r.control_point(ControlPointForm::NonRational, 0).unwrap();
        assert_eq!(p, Vec3::new(0.0, 0.0, 1.0));
        let (p, _) = r.control_point(ControlPointForm::NonRational, 3).unwrap();
        assert_eq!(p, Vec3::new(4.0, 0.0, 1.0));
    }

    #[test]
    fn on_plane_and_off_plane() {
        let c = quad();
        let z = Vec3::new(0.0, 0.0, 1.0);
        let on = c
            .is_on_plane(Vec3::new(0.0, 0.0, 5.0), z, 1e-6, None)
            .unwrap();
        assert!(!on.on_plane);
        assert_eq!(on.max_dist, None);
        // on the plane the SmLine test is reached (false for a plain B-spline)
        let on = c.is_on_plane(Vec3::default(), z, 1e-6, None).unwrap();
        assert!(on.on_plane);
    }

    #[test]
    fn issue_2009_trim_gap_unfixed_without_end_points() {
        // tangency-labs/smlib 8843967 (SmBSplineCurve::TrimGap): an end point that
        // cannot be evaluated leaves the gap unfixed (0); upstream measures the gap
        // between unset points, finds none and reports the curves joined.
        let mut on = Kernel::default();
        on.fixes.query_statuses_pass_on = true;
        let off = Kernel::default();
        let mut a = BSplineCurve::new(3);
        assert!(a.evaluate_point(0.0).is_err());
        let mut b = curve(
            1,
            &[[0.0, 0.0, 0.0], [1.0, 1.0, 0.0]],
            &[0.0, 0.0, 1.0, 1.0],
        );
        assert!(a
            .clone()
            .trim_gap(&off, &mut b.clone(), GapFix::Both, 0.1)
            .unwrap());
        assert!(!a.trim_gap(&on, &mut b, GapFix::Both, 0.1).unwrap());
    }

    #[test]
    fn trim_gap_early_exits() {
        let k = Kernel::default();
        let mut a = curve(
            1,
            &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
            &[0.0, 0.0, 1.0, 1.0],
        );
        let mut b = curve(
            1,
            &[[1.0, 0.0, 0.0], [1.0, 1.0, 0.0]],
            &[0.0, 0.0, 1.0, 1.0],
        );
        assert!(a.trim_gap(&k, &mut b, GapFix::Both, 0.1).unwrap());
        let mut c = curve(
            1,
            &[[3.0, 0.0, 0.0], [4.0, 1.0, 0.0]],
            &[0.0, 0.0, 1.0, 1.0],
        );
        assert!(!a.trim_gap(&k, &mut c, GapFix::Both, 0.1).unwrap());
    }

    #[test]
    fn join_with_gap_too_large() {
        let k = &Kernel::default();
        let mut a = quad();
        let mut b = curve(
            1,
            &[[9.0, 0.0, 0.0], [10.0, 0.0, 0.0]],
            &[0.0, 0.0, 1.0, 1.0],
        );
        assert!(a.join_with(k, true, &mut b, false, Some(0.01)).is_err());
    }

    #[test]
    fn join_with_curves() {
        let k = &Kernel::default();
        // two lines at an angle: joined by CreateByJoining
        let mut a = curve(
            1,
            &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
            &[0.0, 0.0, 1.0, 1.0],
        );
        let mut b = curve(
            1,
            &[[1.0, 0.0, 0.0], [1.0, 1.0, 0.0]],
            &[0.0, 0.0, 1.0, 1.0],
        );
        a.join_with(k, true, &mut b, false, Some(0.01)).unwrap();
        let e = a.ends().unwrap();
        assert_eq!(e.start, Vec3::new(0.0, 0.0, 0.0));
        assert_eq!(e.end, Vec3::new(1.0, 1.0, 0.0));
        // collinear lines reach the SmLine test (false for plain B-splines)
        let mut c = curve(
            1,
            &[[1.0, 1.0, 0.0], [1.0, 2.0, 0.0]],
            &[0.0, 0.0, 1.0, 1.0],
        );
        let r = b.join_with(k, true, &mut c, false, Some(0.01));
        assert!(!matches!(
            r,
            Err(SolidError::Unported {
                unit: "curve.curve_p2",
                ..
            })
        ));
    }

    // C(t) = (width * (t - 1/2), (t - 1/2)^2, 0), with minimum
    // curvature radius width^2 / 2 at t = 1/2 and nonzero speed width.
    fn curvature_test_parabola(dim: u64, width: f64) -> BSplineCurve {
        let mut c = curve(
            2,
            &[
                [-width / 2.0, 0.25, 0.0],
                [0.0, -0.25, 0.0],
                [width / 2.0, 0.25, 0.0],
            ],
            &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        );
        c.dim = dim;
        c
    }

    #[test]
    fn assert_valid_reports_tiny_curvature_radius_in_2d_and_3d() {
        let k = Kernel::default();
        let width = 0.001;
        for (dim, test_index) in [(2, 13), (3, 14)] {
            let c = curvature_test_parabola(dim, width);
            for level_given in [true, false] {
                let mut list = AssertArray::new();
                assert!(!c
                    .assert_valid(&k, Some(&mut list), level_given, ObjectKey(42))
                    .unwrap());
                let reports: Vec<_> = list
                    .reports()
                    .iter()
                    .filter(|r| r.test_index == test_index)
                    .collect();
                assert_eq!(reports.len(), 1);
                let report = reports[0];
                let radius = width * width / 2.0;
                assert!((report.value - radius).abs() <= 1e-9 * radius, "{report:?}");
                assert!((report.owner_param.x - 0.5).abs() <= 1e-9, "{report:?}");
                assert!(report.value < report.tol3d);
                assert_eq!(report.owner_param.y, 0.0);
                // This curve is regular: only the curvature check should fail.
                assert_eq!(list.len(), 1, "{list:?}");
            }
            assert!(!c.assert_valid(&k, None, true, ObjectKey(42)).unwrap());
        }
    }

    #[test]
    fn assert_valid_accepts_regular_curvature_and_linear_empty_analysis() {
        let k = Kernel::default();
        for dim in [2, 3] {
            let c = curvature_test_parabola(dim, 0.02);
            let mut list = AssertArray::new();
            assert!(c
                .assert_valid(&k, Some(&mut list), true, ObjectKey(1))
                .unwrap());
            assert!(list.is_empty());
            let mut line = curve(
                1,
                &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
                &[0.0, 0.0, 1.0, 1.0],
            );
            line.dim = dim;
            assert!(line
                .assert_valid(&k, Some(&mut list), true, ObjectKey(1))
                .unwrap());
            assert!(list.is_empty());
        }
    }

    #[test]
    fn assert_valid_accepts_straight_span_joined_to_regular_bend() {
        let k = Kernel::default();
        // A straight quadratic span followed by a C1 quadratic bend. The radius
        // is infinite on the straight span and at least 1/2 on the bend.
        let c = curve(
            2,
            &[
                [0.0, 0.0, 0.0],
                [0.5, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.5, 0.0, 0.0],
                [2.0, 1.0, 0.0],
            ],
            &[0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
        );
        let mut list = AssertArray::new();
        assert!(
            c.assert_valid(&k, Some(&mut list), true, ObjectKey(1))
                .unwrap(),
            "{list:?}"
        );
        assert!(list.is_empty());
    }

    #[test]
    fn assert_valid_reports_bad_knots() {
        let k = Kernel::default();
        let mut cur = allocate_nurb_curve(2, 1, 4).unwrap();
        for (i, d) in cur.cpts_mut().iter_mut().enumerate() {
            *d = CPoint::new(i as f64, 0.0, 0.0, NOW);
        }
        // start multiplicity 1 (not p + 1 = 2)
        cur.knots_mut().copy_from_slice(&[0.0, 0.5, 0.7, 1.0, 1.0]);
        let c = BSplineCurve::from_nurb(cur, 3, true);
        let mut list = AssertArray::new();
        let ok = c
            .assert_valid(&k, Some(&mut list), true, ObjectKey(7))
            .unwrap();
        assert!(!ok);
        assert_eq!(list.len(), 1);
        assert_eq!(list.get(0).map(|r| r.test_index), Some(1));
        // no NURB: test 0
        let e = BSplineCurve::new(3);
        let mut list = AssertArray::new();
        assert!(!e
            .assert_valid(&k, Some(&mut list), true, ObjectKey(1))
            .unwrap());
        assert_eq!(list.get(0).map(|r| r.test_index), Some(0));
        // the base checks pass (no owner); test 0 still fails
        assert!(!e.assert_valid(&k, None, false, ObjectKey(1)).unwrap());
    }

    /// tangency-labs/solidcore#475 (WR-382): tests 13/14 (an extremely small radius of
    /// curvature) compare the minimum radius before it is read upstream, so they never
    /// fire; here it is read first. A quadratic with a sharp bend: radius 1/h = 1e-3 at its
    /// middle against a tolerance of about h / 20000 = 0.05.
    #[test]
    fn issue_475_min_radius_of_curvature_test() {
        let k = Kernel::default();
        let h = 1000.0;
        let c = curve(
            2,
            &[[-1.0, 0.0, 0.0], [0.0, h, 0.0], [1.0, 0.0, 0.0]],
            &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        );
        let tests = |k: &Kernel| {
            let mut list = AssertArray::new();
            let ok = c
                .assert_valid(k, Some(&mut list), true, ObjectKey(3))
                .unwrap();
            let idx: Vec<_> = (0..list.len())
                .filter_map(|i| list.get(i).map(|r| r.test_index))
                .collect();
            (ok, idx)
        };
        let (ok, idx) = tests(&k);
        assert!(!ok && idx.contains(&14), "{idx:?}");
    }

    #[test]
    fn subdivide_single_span() {
        let k = Kernel::default();
        let c = curve(
            2,
            &[[0.0, 0.0, 0.0], [1.0, 2.0, 0.0], [3.0, 2.0, 0.0]],
            &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        );
        let parts = c
            .subdivide_at_discontinuity(&k, ContinuityType::C0, 1.0)
            .unwrap();
        assert_eq!(parts.len(), 1);
    }

    #[test]
    fn segment_line_sample_count_is_bounded() {
        // fuzz regression (nurbs.b_spline_curve_p3 fuzz00318): i64::MAX samples on a
        // straight curve ran for hours
        let c = curve(
            2,
            &[
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [2.0, 0.0, 0.0],
                [3.0, 0.0, 0.0],
            ],
            &[0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0],
        );
        let ivl = Extent1d::from_min_max(0.0, 1.0);
        assert!(c.is_segment_line(&ivl, 10, 1e-6).unwrap().is_line);
        assert!(c.is_segment_line(&ivl, u64::MAX, 1e-6).is_err());
        assert!(c
            .is_segment_line(&ivl, MAX_SEGMENT_SAMPLES + 1, 1e-6)
            .is_err());
    }

    /// The exact rational trim of a circle of radius `r` about `(cx, cy)` on a plane
    /// spanning `sx` x `sy` over `[0,1]^2`, and that plane: in (u, v) an ellipse
    /// `sy / sx` times as wide as it is long.
    fn circle_trim_on_plane(
        (cx, cy, r): (f64, f64, f64),
        (sx, sy): (f64, f64),
    ) -> (BSplineCurve, Surface) {
        use crate::nurbs::b_spline_curve::p1::{BSplineCurveForm, CanonicalData, KnotType};
        use crate::nurbs::b_spline_surface::BSplineSurface;
        let s = 2.0_f64.sqrt();
        let control: Vec<Vec3> = (0..9_i32)
            .map(|i| {
                let t = f64::from(i) * PI / 4.0;
                let scale = if i % 2 == 1 { s } else { 1.0 };
                Vec3::new(
                    (cx + scale * r * t.cos()) / sx,
                    (cy + scale * r * t.sin()) / sy,
                    0.0,
                )
            })
            .collect();
        let w = 0.5_f64.sqrt();
        let trim = BSplineCurve::create_canonical(&CanonicalData {
            dim: 2,
            degree: 2,
            control_points: &control,
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &[3, 2, 2, 2, 3],
            knots: &[0.0, 0.25, 0.5, 0.75, 1.0],
            knot_type: KnotType::Unspecified,
            weights: Some(&[1.0, w, 1.0, w, 1.0, w, 1.0, w, 1.0]),
            trim: None,
        })
        .unwrap();
        let plane = Surface::from(
            BSplineSurface::create_bilinear_surface(
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(sx, 0.0, 0.0),
                Vec3::new(0.0, sy, 0.0),
                Vec3::new(sx, sy, 0.0),
            )
            .unwrap(),
        );
        (trim, plane)
    }

    /// The failed test indices of `trim`'s `AssertValid` on `surface`.
    fn failed_tests(k: &Kernel, trim: &BSplineCurve, surface: Option<&Surface>) -> Vec<u64> {
        let mut list = AssertArray::new();
        trim.assert_valid_on(k, Some(&mut list), true, ObjectKey(5), surface)
            .unwrap();
        (0..list.len())
            .filter_map(|i| list.get(i).map(|r| r.test_index))
            .collect()
    }

    /// WR-560 (fork tangency-labs/smlib b4f8c57, #891 item 2): the exact trim of a r 1
    /// circle on a plane spanning 600 x 3 is an ellipse 200 times as long as it is wide
    /// in (u, v), of radius 8.3e-6 at its ends against a tolerance of about 6.7e-5; on
    /// the plane it is the circle of radius 1.
    #[test]
    fn issue_891_uv_trim_radius_is_measured_on_its_surface() {
        let k = Kernel::default();
        let (trim, plane) = circle_trim_on_plane((300.0, 1.5, 1.0), (600.0, 3.0));
        let idx = failed_tests(&k, &trim, Some(&plane));
        assert!(!idx.contains(&13), "{idx:?}");
        // a curve that is no face's trim is tested as it is
        let idx = failed_tests(&k, &trim, None);
        assert!(idx.contains(&13), "{idx:?}");
    }

    /// WR-560 (fork tangency-labs/smlib 54ca3e0, #1524): the trim of a r 0.003 hole on a
    /// plane spanning 4500 x 4500 has control points 6.7e-7 apart in (u, v), under the
    /// pole tolerance `SM_ZONE_TOL_3D / 10`; on the plane they are 0.003 apart.
    #[test]
    fn issue_1524_uv_trim_control_points_are_measured_on_its_surface() {
        let k = Kernel::default();
        let (trim, plane) = circle_trim_on_plane((2250.0, 2250.0, 0.003), (4500.0, 4500.0));
        let idx = failed_tests(&k, &trim, Some(&plane));
        assert!(!idx.contains(&15), "{idx:?}");
    }

    /// [`radius_clears_on_surface`] bounds the radius of the trim's image: on a plane
    /// spanning 600 x 3 over the unit square, the quadratic trim whose image has the
    /// control points `(-1, 1) (0, -1) (1, 1)` about the square's centre (a parabola 200
    /// times as narrow in (u, v) as on the plane) has its least radius, `a^2 / 2b = 0.5`,
    /// at its apex. It clears 0.25 over the whole trim and does not clear 1.
    #[test]
    fn a_trim_on_a_plane_clears_the_radius_of_its_image() {
        use crate::nurbs::b_spline_surface::BSplineSurface;
        let k = Kernel::default();
        let plane = Surface::from(
            BSplineSurface::create_bilinear_surface(
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(600.0, 0.0, 0.0),
                Vec3::new(0.0, 3.0, 0.0),
                Vec3::new(600.0, 3.0, 0.0),
            )
            .unwrap(),
        );
        let at = |x: f64, y: f64| [0.5 + x / 600.0, 0.5 + y / 3.0];
        let trim = uv_bezier(&[at(-1.0, 1.0), at(0.0, -1.0), at(1.0, 1.0)]);
        assert!(radius_clears_on_surface(&k, &trim, &plane, 0.0, 1.0, 0.25).unwrap());
        assert!(!radius_clears_on_surface(&k, &trim, &plane, 0.0, 1.0, 1.0).unwrap());
    }

    /// A r 6 sphere about the origin, its poles on z, and its natural UV domain.
    fn sphere() -> (Surface, Extent2d) {
        use crate::surface::sphere::{Sphere, SphereOptions};
        let k = Kernel::default();
        let mut sphere = Sphere::new(
            &k,
            Point3::ZERO,
            Vec3::X,
            Vec3::Y,
            Extent2d::from_bounds(0.0, -90.0, 360.0, 90.0),
            6.0,
            SphereOptions::default(),
        )
        .unwrap();
        sphere.make_nurb(&k).unwrap();
        let surface = Surface::from(sphere);
        let dom = surface.natural_uv_domain_if_built();
        (surface, dom)
    }

    /// A 2D trim of `degree` through the control points `pts` (one Bezier piece).
    fn uv_bezier(pts: &[[f64; 2]]) -> BSplineCurve {
        use crate::nurbs::b_spline_curve::p1::{BSplineCurveForm, CanonicalData, KnotType};
        let control: Vec<Vec3> = pts.iter().map(|p| Vec3::new(p[0], p[1], 0.0)).collect();
        let m = control.len();
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 2,
            degree: m - 1,
            control_points: &control,
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &[m, m],
            knots: &[0.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights: None,
            trim: None,
        })
        .unwrap()
    }

    /// Near a pole: a straight trim along u at fixed v on a r 6 sphere is an arc of a
    /// circle of latitude, whose radius is its distance from the sphere's axis (about 2e-5
    /// 1e-6 of the way from either pole, 6 halfway up). The bound clears half that radius
    /// over the whole trim and does not clear twice it.
    #[test]
    fn a_trim_near_a_sphere_pole_clears_the_radius_of_its_image() {
        let k = Kernel::default();
        let (surface, dom) = sphere();
        let (w, h) = (dom.max().x - dom.min().x, dom.max().y - dom.min().y);
        for f in [1e-6, 0.5, 1.0 - 1e-6] {
            let v = dom.min().y + f * h;
            let (u0, u1) = (dom.min().x + 0.1 * w, dom.min().x + 0.7 * w);
            let trim = uv_bezier(&[[u0, v], [u1, v]]);
            let p = surface
                .evaluate_point(&k, Point2::new(0.5 * (u0 + u1), v))
                .unwrap();
            let radius = (p.x * p.x + p.y * p.y).sqrt();
            let clears = |tol| radius_clears_on_surface(&k, &trim, &surface, 0.0, 1.0, tol);
            assert!(clears(0.5 * radius).unwrap(), "f {f}: {radius}");
            assert!(!clears(2.0 * radius).unwrap(), "f {f}: {radius}");
        }
    }

    /// A real cusp near a pole is still reported: a quadratic trim turning sharply in
    /// (u, v), 1.5 % of the way from the sphere's pole, where the surface is regular. Its
    /// turn, `(-e, h) (0, -h) (e, h)` about its apex with `e = 1e-5` and `h = 2e-3` of the
    /// domain, has radius `e^2 / 2h` in (u, v) and one about as small on the ball, far
    /// below test 13's tolerance: the bound does not clear it and test 13 reports.
    #[test]
    fn a_cusp_near_a_sphere_pole_is_reported() {
        let k = Kernel::default();
        let (surface, dom) = sphere();
        let (w, h) = (dom.max().x - dom.min().x, dom.max().y - dom.min().y);
        let (u, v) = (dom.min().x + 0.5 * w, dom.max().y - 0.015 * h);
        let (e, hh) = (1e-5 * w, 2e-3 * h);
        let trim = uv_bezier(&[[u - e, v + hh], [u, v - hh], [u + e, v + hh]]);
        let tol = 1e-4;
        assert!(!radius_clears_on_surface(&k, &trim, &surface, 0.0, 1.0, tol).unwrap());
        let idx = failed_tests(&k, &trim, Some(&surface));
        assert!(idx.contains(&13), "{idx:?}");
    }

    /// The zero-speed stretch certificate: over a stretch taken as the whole of a trim, a
    /// latitude arc halfway up the r 6 ball (radius 6) clears half the growth line of the
    /// tolerance 1e-3 (at each piece's near edge), and the V cusp near the pole of
    /// [`a_cusp_near_a_sphere_pole_is_reported`] does not, from either end. (A unit test
    /// of the certificate: a whole trim with a zero-speed tangent end on a tangent edge,
    /// a corner inside its stretch and a false-alarm turn elsewhere is not built here.)
    #[test]
    fn a_corner_inside_a_zero_speed_stretch_is_not_cleared() {
        let k = Kernel::default();
        let (surface, dom) = sphere();
        let (w, h) = (dom.max().x - dom.min().x, dom.max().y - dom.min().y);
        let v = dom.min().y + 0.5 * h;
        let arc = uv_bezier(&[[dom.min().x + 0.1 * w, v], [dom.min().x + 0.7 * w, v]]);
        let unit = Extent1d::from_raw(0.0, 1.0);
        for end in [0, 1] {
            assert!(stretch_clears_on_surface(&k, &arc, &surface, &unit, end, 1.0, 1e-3).unwrap());
        }
        let (u, v) = (dom.min().x + 0.5 * w, dom.max().y - 0.015 * h);
        let (e, hh) = (1e-5 * w, 2e-3 * h);
        let cusp = uv_bezier(&[[u - e, v + hh], [u, v - hh], [u + e, v + hh]]);
        for end in [0, 1] {
            assert!(
                !stretch_clears_on_surface(&k, &cusp, &surface, &unit, end, 1.0, 1e-3).unwrap()
            );
        }
    }

    /// The zero-speed stretch certificate on a genuine zero-speed end: the cubic with
    /// `P0 = P1` = (0.2, 0.2), (0.6, 0.2), (0.6, 0.6) on the unit plane (isometric, so its
    /// image's radius is its own) has, near the start, a radius on the growth line to first
    /// order (rate `12 |a|^3 / |a x b|` = 4.8, `a = P2 - P0`, `b = P3 - 3 P2 + 2 P0`). Its
    /// WR-668 stretch for the tolerance 1e-3 (2.1e-4) clears down to the rounding-level
    /// sliver (about 2e-7 of the end). So does the reversed cubic's at its far end, on
    /// the domain [0, 1] and on [1000, 1001], a domain end far from 0 (where pieces cut at
    /// `2^-52` of the stretch would round onto the end).
    #[test]
    fn a_genuine_zero_speed_end_clears_its_stretch() {
        let k = Kernel::default();
        let plane = Surface::from(
            crate::nurbs::b_spline_surface::BSplineSurface::create_bilinear_surface(
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(1.0, 1.0, 0.0),
            )
            .unwrap(),
        );
        let pts = [[0.2, 0.2], [0.2, 0.2], [0.6, 0.2], [0.6, 0.6]];
        let mut back = pts;
        back.reverse();
        let tol = 1e-3;
        for (cubic, end, lo) in [
            (uv_cubic_on(&pts, 0.0), 0, 0.0),
            (uv_cubic_on(&back, 0.0), 1, 0.0),
            (uv_cubic_on(&back, 1000.0), 1, 1000.0),
        ] {
            let stretch = zero_speed_radius_stretch(&cubic, end, tol)
                .unwrap()
                .unwrap();
            assert!((stretch - tol / 4.8).abs() <= 1e-9 * stretch, "{stretch:e}");
            let domain = Extent1d::from_raw(lo, lo + 1.0);
            assert!(
                stretch_clears_on_surface(&k, &cubic, &plane, &domain, end, stretch, tol).unwrap(),
                "end {end} on [{lo}, {}]",
                lo + 1.0
            );
        }
    }

    /// The sliver at a zero-speed end is one polynomial piece. A cubic trim on the unit
    /// plane (knots 0, 1e-4, 1e-3, 2e-3, 3e-3, 1; single interior knots, so C2) whose
    /// first span (0 to 1e-4) has its control points within two ulps of (0.2, 0.2), so
    /// `|c'|` is 0 and `|c''|` tiny at the start, and which then runs out to (0.6, 0.2) and
    /// turns back there sharply but smoothly inside the span 2e-3 to 3e-3 (asserted: its
    /// radius there does not clear 1e-6; about 8e-9 near t = 3.0e-3 by sampling). A sliver
    /// from the end's derivatives alone would run past 3e-3 (asserted) and cover the
    /// turn. The sliver's extent certificate keeps the turn out of the sliver: it certifies
    /// one polynomial piece only, so the sliver ends at the knot 1e-4, the turn is in the
    /// pieces, and the stretch (0.05, taken as given) does not clear. MEASURED by probe:
    /// with the sliver taken uncertified, as at df391d2ef, the stretch clears and this test
    /// fails. The turn's radius is bounded above at one point in rounded intervals.
    #[test]
    fn a_turn_past_a_collapsed_end_span_is_not_cleared() {
        let k = Kernel::default();
        let plane = Surface::from(
            crate::nurbs::b_spline_surface::BSplineSurface::create_bilinear_surface(
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(1.0, 1.0, 0.0),
            )
            .unwrap(),
        );
        let a1 = f64::from_bits(0.2_f64.to_bits() + 1);
        let a2 = f64::from_bits(0.2_f64.to_bits() + 2);
        let trim = curve(
            3,
            &[
                [0.2, 0.2, 0.0],
                [0.2, 0.2, 0.0],
                [a1, 0.2, 0.0],
                [a2, 0.2, 0.0],
                [0.6, 0.2, 0.0],
                [0.6, 0.201, 0.0],
                [0.2, 0.202, 0.0],
                [0.2, 0.6, 0.0],
            ],
            &[
                0.0, 0.0, 0.0, 0.0, 1e-4, 1e-3, 2e-3, 3e-3, 1.0, 1.0, 1.0, 1.0,
            ],
        );
        assert!(!radius_clears_on_surface(&k, &trim, &plane, 2e-3, 3e-3, 1e-6).unwrap());
        // the radius at t0 = 2.9971e-3 (span 2e-3 to 3e-3), bounded above: |c'|^3 / |c' x c''|
        {
            use crate::blend::fillet_section::certificate::{curve_derivs_over, NurbsCurve};
            use crate::blend::fillet_section::interval::Ivl;
            let nc = NurbsCurve::of(&trim).unwrap();
            let d = curve_derivs_over(&nc, Ivl::from(2.9971e-3), 2, Some(6)).unwrap();
            let (d1, d2) = (d[1], d[2]);
            let speed_sq = d1.x * d1.x + d1.y * d1.y;
            let cross = d1.x * d2.y - d1.y * d2.x;
            assert!(cross.lo > 0.0 || cross.hi < 0.0);
            let cross_lo = Ivl::from(cross.lo.abs().min(cross.hi.abs()));
            let radius_hi = (speed_sq * speed_sq.sqrt() / cross_lo).hi;
            assert!(radius_hi <= 1e-6, "{radius_hi:e}");
        }
        let d = trim.evaluate(0.0, 2, true, false).unwrap();
        let (speed, accel) = (d[1].length(), d[2].length());
        assert!(speed == 0.0 && accel > 0.0, "{speed:e} {accel:e}");
        let p_max = (0.6_f64 * 0.6 + 0.201 * 0.201).sqrt();
        let quarter = 0.25 * STRETCH_SLIVER_ULPS * f64::EPSILON * p_max;
        let uncapped = (2.0 * quarter / accel).sqrt();
        assert!(uncapped > 3e-3, "{uncapped:e}");
        let unit = Extent1d::from_raw(0.0, 1.0);
        assert!(!stretch_clears_on_surface(&k, &trim, &plane, &unit, 0, 0.05, 1e-3).unwrap());
    }

    /// A C0 corner at a knot of full multiplicity is never cleared: a quadratic trim on the
    /// r 6 ball halfway up, along the latitude circle and then down a meridian, with the
    /// double knot 0.5 at the corner. The radius bound cuts its pieces at the knots, so it
    /// would see two smooth arcs (MEASURED: without the knot check it clears 1e-3); the knot,
    /// whose two tangents are not certified parallel, refuses it. Test 7 reports the trim;
    /// test 13 finds no solution at such a knot (also on main, MEASURED), so the pass-over
    /// had nothing to hide here, and the knot check keeps it so where test 13 does find one.
    /// The same trim with its double knot on a straight run (a G1 join, the tangents
    /// parallel) is not refused for the knot: the radius bound decides it.
    #[test]
    fn a_c0_corner_at_a_knot_is_not_cleared() {
        let k = Kernel::default();
        let (surface, dom) = sphere();
        let trim = uv_quadratic(
            &[
                lon_lat(&dom, 1.0, 0.3),
                lon_lat(&dom, 1.2, 0.3),
                lon_lat(&dom, 1.4, 0.3),
                lon_lat(&dom, 1.4, 0.1),
                lon_lat(&dom, 1.4, -0.1),
            ],
            &[0.0, 0.5, 1.0],
            &[3, 2, 3],
        );
        assert!(!radius_clears_on_surface(&k, &trim, &surface, 0.0, 1.0, 1e-3).unwrap());
        let idx = failed_tests(&k, &trim, Some(&surface));
        assert!(idx.contains(&7), "{idx:?}");
        // a G1 join: the double knot in the middle of a straight run along the circle
        let join = uv_quadratic(
            &[
                lon_lat(&dom, 1.0, 0.3),
                lon_lat(&dom, 1.1, 0.3),
                lon_lat(&dom, 1.2, 0.3),
                lon_lat(&dom, 1.3, 0.3),
                lon_lat(&dom, 1.4, 0.3),
            ],
            &[0.0, 0.5, 1.0],
            &[3, 2, 3],
        );
        assert!(radius_clears_on_surface(&k, &join, &surface, 0.0, 1.0, 1e-3).unwrap());
        // a near-G1 corner, 1e-8 rad, at a double knot on a straight run of the unit plane:
        // still a corner
        let plane = Surface::from(
            crate::nurbs::b_spline_surface::BSplineSurface::create_bilinear_surface(
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(1.0, 1.0, 0.0),
            )
            .unwrap(),
        );
        let bend: f64 = 1e-8;
        let kinked = uv_quadratic(
            &[
                [0.2, 0.2],
                [0.3, 0.2],
                [0.4, 0.2],
                [0.4 + 0.1 * bend.cos(), 0.2 + 0.1 * bend.sin()],
                [0.4 + 0.2 * bend.cos(), 0.2 + 0.2 * bend.sin()],
            ],
            &[0.0, 0.5, 1.0],
            &[3, 2, 3],
        );
        assert!(!radius_clears_on_surface(&k, &kinked, &plane, 0.0, 1.0, 1e-3).unwrap());
        let straight = uv_quadratic(
            &[[0.2, 0.2], [0.3, 0.2], [0.4, 0.2], [0.5, 0.2], [0.6, 0.2]],
            &[0.0, 0.5, 1.0],
            &[3, 2, 3],
        );
        assert!(radius_clears_on_surface(&k, &straight, &plane, 0.0, 1.0, 1e-3).unwrap());
        // a knot of multiplicity 3 on a quadratic (the curve may jump there): refused
        // (the raw NURB: the canonical constructor would reduce the knot)
        let jump = curve(
            2,
            &[
                [0.2, 0.2, 0.0],
                [0.3, 0.2, 0.0],
                [0.4, 0.2, 0.0],
                [0.4, 0.2, 0.0],
                [0.5, 0.2, 0.0],
                [0.6, 0.2, 0.0],
            ],
            &[0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0],
        );
        assert!(!radius_clears_on_surface(&k, &jump, &plane, 0.0, 1.0, 1e-3).unwrap());
    }

    /// A G1 join whose control points are rounded keeps the pass-over: the straight run
    /// `(0.7, 0.4) + i h (cos a, sin a)`, `i = 0..4`, `h = 1e-4`, on the unit plane as two
    /// quadratic pieces joined at a double knot, each control point rounded on its own, so
    /// that the knot's two legs are parallel only to the points' rounding (up to
    /// `ulp(0.7) / h`, about 1e-12 rad). MEASURED by probe: the cross product's enclosure at
    /// the knot reaches 1.3e-10 to 1.9e-10 of `|a| |b|` for the five directions, beyond
    /// `64 ε` (1.4e-14), so a rule of `64 ε` alone would refuse every one of these
    /// straight runs; the enclosures' widths term admits them (allowance 3.8e-10). The
    /// near-G1 corner of 1e-8 rad in [`a_c0_corner_at_a_knot_is_not_cleared`] is still
    /// refused.
    #[test]
    fn a_straight_run_with_rounded_control_points_clears() {
        let k = Kernel::default();
        let plane = Surface::from(
            crate::nurbs::b_spline_surface::BSplineSurface::create_bilinear_surface(
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(1.0, 1.0, 0.0),
            )
            .unwrap(),
        );
        let h = 1e-4;
        for a in [0.3f64, 0.7, 1.1, 2.0, 2.9] {
            let (sn, cs) = a.sin_cos();
            let at = |i: f64| [0.7 + i * h * cs, 0.4 + i * h * sn];
            let run = uv_quadratic(
                &[at(0.0), at(1.0), at(2.0), at(3.0), at(4.0)],
                &[0.0, 0.5, 1.0],
                &[3, 2, 3],
            );
            assert!(
                radius_clears_on_surface(&k, &run, &plane, 0.0, 1.0, 1e-3).unwrap(),
                "{a}"
            );
        }
    }

    /// The half-line definition reports a real defect: the positive cubic of
    /// [`a_genuine_zero_speed_end_clears_its_stretch`] read against a growth line 2.5 times
    /// as steep as its own (the stretch taken as 0.4 times its WR-668 stretch), so that its
    /// radius is 0.4 times the line, below the half the definition asks, does not clear;
    /// against its own line it does.
    #[test]
    fn a_radius_below_half_the_growth_line_is_reported() {
        let k = Kernel::default();
        let plane = Surface::from(
            crate::nurbs::b_spline_surface::BSplineSurface::create_bilinear_surface(
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(1.0, 1.0, 0.0),
            )
            .unwrap(),
        );
        let cubic = uv_cubic_on(&[[0.2, 0.2], [0.2, 0.2], [0.6, 0.2], [0.6, 0.6]], 0.0);
        let tol = 1e-3;
        let stretch = zero_speed_radius_stretch(&cubic, 0, tol).unwrap().unwrap();
        let unit = Extent1d::from_raw(0.0, 1.0);
        assert!(stretch_clears_on_surface(&k, &cubic, &plane, &unit, 0, stretch, tol).unwrap());
        assert!(
            !stretch_clears_on_surface(&k, &cubic, &plane, &unit, 0, 0.4 * stretch, tol).unwrap()
        );
    }

    /// The cap is what lets a stretch whose first knot lies inside the uncapped sliver be
    /// certified: the cubic of [`a_genuine_zero_speed_end_clears_its_stretch`] with the knot
    /// `a = 0.9` times its sliver length inserted (Boehm: the same curve up to rounding).
    /// With the cap the sliver ends at `a`, inside one span, and the stretch clears. MEASURED
    /// by probe: without the cap the sliver crosses `a`, its extent certificate refuses it,
    /// and the stretch is not certified. The stretch is the plain cubic's: on the knotted
    /// curve WR-668's stretch detection (`zero_speed_radius_stretch`) reads `C'''` off the
    /// 1.8e-7 first span, where rounding swamps it, and finds no stretch, so the production
    /// path never reaches the certificate here (a WR-668 limitation, recorded in the PR).
    #[test]
    fn a_zero_speed_end_with_a_knot_inside_its_sliver_clears() {
        let k = Kernel::default();
        let plane = Surface::from(
            crate::nurbs::b_spline_surface::BSplineSurface::create_bilinear_surface(
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(1.0, 1.0, 0.0),
            )
            .unwrap(),
        );
        let pts = [[0.2, 0.2], [0.2, 0.2], [0.6, 0.2], [0.6, 0.6]];
        let plain = uv_cubic_on(&pts, 0.0);
        let d = plain.evaluate(0.0, 2, true, false).unwrap();
        let accel = d[2].length();
        let p_max = (0.6_f64 * 0.6 + 0.6 * 0.6).sqrt();
        let quarter = 0.25 * STRETCH_SLIVER_ULPS * f64::EPSILON * p_max;
        let sliver = (2.0 * quarter / accel).sqrt();
        let a = 0.9 * sliver;
        let lerp = |x: [f64; 2], y: [f64; 2]| {
            [
                (1.0 - a) * x[0] + a * y[0],
                (1.0 - a) * x[1] + a * y[1],
                0.0,
            ]
        };
        let trim = curve(
            3,
            &[
                [0.2, 0.2, 0.0],
                lerp(pts[0], pts[1]),
                lerp(pts[1], pts[2]),
                lerp(pts[2], pts[3]),
                [0.6, 0.6, 0.0],
            ],
            &[0.0, 0.0, 0.0, 0.0, a, 1.0, 1.0, 1.0, 1.0],
        );
        let tol = 1e-3;
        // the plain cubic's stretch (its third derivative, read off the short first span of
        // the knotted one, is lost to rounding)
        let stretch = zero_speed_radius_stretch(&plain, 0, tol).unwrap().unwrap();
        let unit = Extent1d::from_raw(0.0, 1.0);
        assert!(stretch_clears_on_surface(&k, &trim, &plane, &unit, 0, stretch, tol).unwrap());
    }

    /// Where `ulp(t_end)` binds the sliver, the sliver's extent is not at the trim's
    /// rounding and the stretch is not certified: the cubic of
    /// [`a_genuine_zero_speed_end_clears_its_stretch`] on [2^33, 2^33 + 1], where
    /// `ulp(2^33)` = 1.9e-6 is far above the 2e-7 sliver of the domain [0, 1], moves about
    /// `|c''| ulp² / 2` = 4.3e-12 within it, above `L` = 1.9e-13.
    #[test]
    fn a_stretch_whose_sliver_the_ulp_binds_is_not_cleared() {
        let k = Kernel::default();
        let plane = Surface::from(
            crate::nurbs::b_spline_surface::BSplineSurface::create_bilinear_surface(
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(1.0, 1.0, 0.0),
            )
            .unwrap(),
        );
        let lo = 2f64.powi(33);
        let cubic = uv_cubic_on(&[[0.2, 0.2], [0.2, 0.2], [0.6, 0.2], [0.6, 0.6]], lo);
        let tol = 1e-3;
        let stretch = tol / 4.8;
        let domain = Extent1d::from_raw(lo, lo + 1.0);
        assert!(!stretch_clears_on_surface(&k, &cubic, &plane, &domain, 0, stretch, tol).unwrap());
    }

    /// A cubic 2D trim through the control points `pts` on the domain `[lo, lo + 1]`.
    fn uv_cubic_on(pts: &[[f64; 2]; 4], lo: f64) -> BSplineCurve {
        use crate::nurbs::b_spline_curve::p1::{BSplineCurveForm, CanonicalData, KnotType};
        let control: Vec<Vec3> = pts.iter().map(|p| Vec3::new(p[0], p[1], 0.0)).collect();
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 2,
            degree: 3,
            control_points: &control,
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &[4, 4],
            knots: &[lo, lo + 1.0],
            knot_type: KnotType::Unspecified,
            weights: None,
            trim: None,
        })
        .unwrap()
    }

    /// A quadratic 2D trim through the control points `pts` with the distinct `knots` and
    /// their multiplicities `mults` (ends 3, interior 1).
    fn uv_quadratic(pts: &[[f64; 2]], knots: &[f64], mults: &[usize]) -> BSplineCurve {
        use crate::nurbs::b_spline_curve::p1::{BSplineCurveForm, CanonicalData, KnotType};
        let control: Vec<Vec3> = pts.iter().map(|p| Vec3::new(p[0], p[1], 0.0)).collect();
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 2,
            degree: 2,
            control_points: &control,
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: mults,
            knots,
            knot_type: KnotType::Unspecified,
            weights: None,
            trim: None,
        })
        .unwrap()
    }

    /// The (u, v) of longitude `lon` and latitude `lat` (radians) on [`sphere`]'s domain.
    fn lon_lat(dom: &Extent2d, lon: f64, lat: f64) -> [f64; 2] {
        let (w, h) = (dom.max().x - dom.min().x, dom.max().y - dom.min().y);
        [
            dom.min().x + lon / (2.0 * PI) * w,
            dom.min().y + (lat + 0.5 * PI) / PI * h,
        ]
    }

    /// A trim that comes down a meridian of the r 6 sphere from the equator to 0.01 rad from
    /// the pole and turns there onto the circle of latitude, sweeping 2 rad of longitude (the
    /// control points (lon, lat) `(0, 0) (0, π/2 - 0.05) (0, π/2 - 0.01) (2, π/2 - 0.01)`,
    /// knots 0, 0.9, 1), followed by the control points `extra` (with two of them, knots 0,
    /// 0.9, 0.95, 0.97, 1). Its middle lies on the meridian at a latitude of about 70°, where
    /// a step in u is about 33 times as long as at the turn. So the turn onto the latitude
    /// circle, whose u leg is 2 rad but 0.12 long on the ball, mapped by the metric at the
    /// middle (WR-560), gives test-13 solutions of 3.4e-4 to 7.4e-4 against the tolerance
    /// 1.0e-3 (measured), while its image on the ball has a least radius of about 6.8e-3 (a
    /// closed-form model of the curve, sampled; the test asserts only the verdicts).
    fn turn_onto_a_latitude_circle(dom: &Extent2d, extra: &[[f64; 2]]) -> BSplineCurve {
        let top = 0.5 * PI - 0.01;
        let mut pts = vec![
            lon_lat(dom, 0.0, 0.0),
            lon_lat(dom, 0.0, top - 0.04),
            lon_lat(dom, 0.0, top),
            lon_lat(dom, 2.0, top),
        ];
        pts.extend(extra.iter().map(|&[lon, lat]| lon_lat(dom, lon, lat)));
        if extra.is_empty() {
            uv_quadratic(&pts, &[0.0, 0.9, 1.0], &[3, 1, 3])
        } else {
            uv_quadratic(&pts, &[0.0, 0.9, 0.95, 0.97, 1.0], &[3, 1, 1, 1, 3])
        }
    }

    /// The trim of [`turn_onto_a_latitude_circle`]: the turn that is sharp only in the
    /// mapped (u, v) is passed over, so test 13 does not report. Without the pass-over (on
    /// main) test 13 reports this trim.
    #[test]
    fn a_turn_near_a_sphere_pole_that_is_smooth_on_the_ball_is_passed_over() {
        let k = Kernel::default();
        let (surface, dom) = sphere();
        let trim = turn_onto_a_latitude_circle(&dom, &[]);
        let idx = failed_tests(&k, &trim, Some(&surface));
        assert!(!idx.contains(&13), "{idx:?}");
    }

    /// The same turn, followed along the latitude circle by a corner that is sharp on the
    /// ball: the control points `(2.06, π/2 - 0.01) (2.06, π/2 - 0.31)` and knots 0, 0.9,
    /// 0.95, 0.97, 1 turn back down a meridian over 0.06 rad of longitude, 0.0036 long on
    /// the ball. On the ball the corner's radius is about 5e-6 at t = 0.97; the turn at
    /// t = 0.9 still has a radius of about 3.1e-3 there at its own parameter, which a test
    /// at one point passes. Over `[0.9, 0.95]` the bound clears the tolerance 1e-3; over
    /// `[0.9, 1]`, which holds the corner off the turn's parameter, it does not.
    ///
    /// Test 13 reports the trim. Mapped, the corner reads about 4.2e-3, above the tolerance,
    /// so the analysis finds no solution there; a pass-over certified only out to the
    /// neighbouring solutions passed the turn at 0.9 and so the whole trim. The pass-over is
    /// certified over the whole trim, which the corner fails, so the turn is reported.
    #[test]
    fn a_kink_on_the_ball_off_a_turns_parameter_is_not_cleared() {
        let k = Kernel::default();
        let (surface, dom) = sphere();
        let top = 0.5 * PI - 0.01;
        let trim = turn_onto_a_latitude_circle(&dom, &[[2.06, top], [2.06, top - 0.3]]);
        let clears = |lo, hi| radius_clears_on_surface(&k, &trim, &surface, lo, hi, 1e-3);
        assert!(clears(0.9, 0.95).unwrap());
        assert!(!clears(0.9, 1.0).unwrap());
        let idx = failed_tests(&k, &trim, Some(&surface));
        assert!(idx.contains(&13), "{idx:?}");
    }

    /// WR-668 (fork tangency-labs/smlib ecf1963): `C(t) = (t^2, t^3, 0)` on `[0, 1]`,
    /// `y = x^(3/2)`, the shape of the curve where two surfaces touch at a vertex (a
    /// G2 blend's tangent end): curvature growing as `s^(-1/2)` in the length `s`,
    /// zero speed at `t = 0`, radius of curvature `t (4 + 9 t^2)^(3/2) / 6`, growing as
    /// `4/3 t`. The Bezier `(0,0) (0,0) (1/3,0) (1,1)` with the knot `a = 5e-5`
    /// inserted (Boehm: `Q_i = (1 - a) P_(i-1) + a P_i`), as an interpolated rib has
    /// knots close to its end: the radius minimum the check's analysis finds is at
    /// that knot, `r(a) ~ 6.7e-5`, below the tolerance `|box| / 10000 = 1.4e-4`.
    fn zero_speed_end_curve() -> BSplineCurve {
        let a = 5e-5;
        let p = [
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [1.0 / 3.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
        ];
        let lerp = |x: [f64; 3], y: [f64; 3]| {
            [
                (1.0 - a) * x[0] + a * y[0],
                (1.0 - a) * x[1] + a * y[1],
                0.0,
            ]
        };
        curve(
            3,
            &[
                p[0],
                lerp(p[0], p[1]),
                lerp(p[1], p[2]),
                lerp(p[2], p[3]),
                p[3],
            ],
            &[0.0, 0.0, 0.0, 0.0, a, 1.0, 1.0, 1.0, 1.0],
        )
    }

    /// Both ends of an edge over `interval` with its faces tangent there or not.
    fn ends(interval: (f64, f64), faces_tangent: bool) -> EdgeEnds {
        EdgeEnds {
            interval: Extent1d::from_raw(interval.0, interval.1),
            faces_tangent: [Ok(faces_tangent), Ok(faces_tangent)],
        }
    }

    /// The tests [`BSplineCurve::assert_valid_on_edge`] reports for `c` on `edge`.
    fn edge_failed_tests(k: &Kernel, c: &BSplineCurve, edge: Option<&EdgeEnds>) -> Vec<u64> {
        let mut list = AssertArray::new();
        let ok = c
            .assert_valid_on_edge(k, Some(&mut list), true, ObjectKey(7), None, edge)
            .unwrap();
        let idx: Vec<u64> = (0..list.len())
            .filter_map(|i| list.get(i).map(|r| r.test_index))
            .collect();
        assert_eq!(ok, idx.is_empty(), "{idx:?}");
        idx
    }

    #[test]
    fn wr_668_zero_speed_tangent_end_is_no_kink() {
        let c = zero_speed_end_curve();
        let tangent = ends((0.0, 1.0), true);
        let k = Kernel::default();
        // at a vertex where the edge's faces touch, the coincident first control points
        // (test 3) and, with the minimum radius read, the radius below the tolerance
        // (test 14) are the zero-speed end (upstream reports both)
        assert!(edge_failed_tests(&k, &c, Some(&tangent)).is_empty());
        assert!(edge_failed_tests(&k, &c, Some(&tangent)).is_empty());
        // on no edge, where the faces meet at an angle, or at no end of the edge, it
        // still fails both. Before tangency-labs/solidcore#2574 the 0 test 14 compared
        // came from the objective failing at the zero-speed end; it is now the radius
        // where the search stopped, still below the tolerance (#2669).
        assert_eq!(edge_failed_tests(&k, &c, None), vec![3, 14]);
        let angled = ends((0.0, 1.0), false);
        assert_eq!(edge_failed_tests(&k, &c, Some(&angled)), vec![3, 14]);
        let inner = ends((0.25, 1.0), true);
        assert_eq!(edge_failed_tests(&k, &c, Some(&inner)), vec![3, 14]);
        // an unported face evaluation is passed on when the end is consulted
        let unported = EdgeEnds {
            interval: Extent1d::from_raw(0.0, 1.0),
            faces_tangent: [
                Err(SolidError::Unported {
                    upstream: "SmSurface::EvaluateNormal",
                    unit: "test",
                }),
                Ok(true),
            ],
        };
        assert!(c
            .assert_valid_on_edge(&k, None, true, ObjectKey(7), None, Some(&unported))
            .unwrap_err()
            .is_unported());
    }

    #[test]
    fn wr_668_radius_stretch_is_the_growth_rate() {
        // rate 2 |C''|^3 / |C'' x C'''| = 2 * 8 / 12 = 4/3: the stretch is 3/4 tol
        let c = zero_speed_end_curve();
        let tol = 1e-4;
        let s = zero_speed_radius_stretch(&c, 0, tol).unwrap().unwrap();
        assert!((s - tol * 0.75).abs() <= 1e-12 * s, "{s}");
        // the far end is regular: its radius is far above the tolerance there
        assert_eq!(zero_speed_radius_stretch(&c, 1, tol).unwrap(), None);
        let tangent = ends((0.0, 1.0), true);
        assert!(is_zero_speed_tangent_end(&c, 0, 1e-15, Some(&tangent)).unwrap());
        assert!(!is_zero_speed_tangent_end(&c, 1, 1e-15, Some(&tangent)).unwrap());
    }

    /// [`zero_speed_end_curve`] run backwards, of dimension `dim`: its zero-speed end
    /// is the curve's end (`t = 1`).
    fn zero_speed_at_end_curve(dim: u64) -> BSplineCurve {
        let fwd = zero_speed_end_curve();
        let cur = fwd.nurb.as_ref().unwrap();
        let pts: Vec<[f64; 3]> = cur.cpts().iter().rev().map(|p| [p.x, p.y, p.z]).collect();
        let knots: Vec<f64> = cur.knots().iter().rev().map(|u| 1.0 - u).collect();
        let mut c = curve(3, &pts, &knots);
        c.dim = dim;
        c
    }

    /// A zero-speed tangent end at the curve's end (the fork's `lEnd` 1), as an edge's
    /// 3D curve (test 14) and as a UV trim (test 13): the stretch is measured back from
    /// the domain's max and the radius below the tolerance there is passed over. Test 3
    /// reads only the first pair, and test 4 reports the first pair's values, so with
    /// the minimum radius read only the radius test would fail without WR-668.
    #[test]
    fn wr_668_zero_speed_tangent_end_at_the_curve_end() {
        let tol = 1e-4;
        let tangent = ends((0.0, 1.0), true);
        for (dim, radius_test) in [(3, 14), (2, 13)] {
            let c = zero_speed_at_end_curve(dim);
            let s = zero_speed_radius_stretch(&c, 1, tol).unwrap().unwrap();
            assert!((s - tol * 0.75).abs() <= 1e-9 * s, "{s}");
            assert_eq!(zero_speed_radius_stretch(&c, 0, tol).unwrap(), None);
            assert!(is_zero_speed_tangent_end(&c, 1, 1e-15, Some(&tangent)).unwrap());
            assert!(!is_zero_speed_tangent_end(&c, 0, 1e-15, Some(&tangent)).unwrap());

            let k = Kernel::default();
            assert!(edge_failed_tests(&k, &c, Some(&tangent)).is_empty());
            assert!(edge_failed_tests(&k, &c, Some(&tangent)).is_empty());
            // on no edge, or on an edge that ends past the curve's end, it still fails
            assert_eq!(edge_failed_tests(&k, &c, None), vec![radius_test]);
            let longer = ends((0.0, 1.25), true);
            assert_eq!(edge_failed_tests(&k, &c, Some(&longer)), vec![radius_test]);
        }
    }

    /// Coincident end control points that are no zero-speed end stay a fault at a vertex
    /// where the edge's faces touch: the quartic Bezier `(0,0) (0,0)
    /// (0,0) (1,0) (1,1)` has its second derivative vanish at `t = 0` as well
    /// (`sm_IsZeroSpeedTangentEnd` refuses `|C''| <= dScaledZero`), so tests 3 and 14
    /// report the fault.
    #[test]
    fn wr_668_coincident_points_without_curvature_stay_a_fault() {
        let c = curve(
            4,
            &[
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
            ],
            &[0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        );
        let tangent = ends((0.0, 1.0), true);
        assert!(!is_zero_speed_tangent_end(&c, 0, EFF_ZERO, Some(&tangent)).unwrap());
        let k = Kernel::default();
        let on_edge = edge_failed_tests(&k, &c, Some(&tangent));
        assert!(on_edge.contains(&3), "{on_edge:?}");
        assert_eq!(edge_failed_tests(&k, &c, None), on_edge);
    }

    #[test]
    fn issue_2009_assert_valid_unevaluable_interior_knot_derivs_fail_tests_5_6() {
        // tangency-labs/smlib 8843967 (SmBSplineCurve::AssertValid, tests 5/6, WR-663):
        // two knots 1e-30 apart on a borrowed NURB (not spread by `sm_FixupKnotVector`)
        // count as one double knot in `GetKnots` (tolerance 1e-12), while `sm_BasisDerivs`
        // rejects a span <= 1e-24. Without `query_statuses_pass_on` the failed evaluation
        // leaves `Vec3::UNDEF` (`length = sqrt(3) * SM_UNDEF_DOUBLE`), reporting that
        // length as the fault value; with the flag, test 5 (2D) / 6 (3D) fails with
        // value `SM_UNDEF_DOUBLE`.
        let mut on = Kernel::default();
        on.fixes.query_statuses_pass_on = true;
        let off = Kernel::default();

        let borrowed = |dim: u64, p: usize, pts: &[[f64; 3]], knots: &[f64]| {
            let n = pts.len() - 1;
            let mut cur = allocate_nurb_curve(n, p, knots.len() - 1).unwrap();
            for (d, s) in cur.cpts_mut().iter_mut().zip(pts) {
                *d = CPoint::new(s[0], s[1], s[2], NOW);
            }
            cur.knots_mut().copy_from_slice(knots);
            BSplineCurve::from_nurb(cur, dim, true)
        };

        // Upper interval (`from_left = true`) fails on `[0.0, 1e-30]`.
        let upper_knots = [-1.0, -1.0, -1.0, 0.0, 1e-30, 1.0, 1.0, 1.0];

        // Lower interval (`from_left = false`) fails at the second interior knot:
        // `-1e-12` and `-1e-26` merge into knot 1 (spans ~1.0 and ~1e-12 both evaluate),
        // while `1e-26` x2 forms knot 2 (`|-1e-12 - 1e-26| > 1e-12`) whose lower span
        // `[-1e-26, 1e-26]` has width `2e-26 <= 1e-24`.
        let lower_knots = [
            -1.0, -1.0, -1.0, -1e-12, -1e-26, 1e-26, 1e-26, 1.0, 1.0, 1.0,
        ];

        for (knots, expected_param) in [(&upper_knots[..], 0.0), (&lower_knots[..], 1e-26)] {
            let n_cpts = knots.len() - 3;
            let pts: Vec<[f64; 3]> = (0..n_cpts).map(|i| [i as f64, 0.0, 0.0]).collect();

            for (dim, expected_test) in [(2_u64, 5_u64), (3, 6)] {
                let c = borrowed(dim, 2, &pts, knots);
                let mut list_off = AssertArray::new();
                assert!(!c
                    .assert_valid(&off, Some(&mut list_off), true, ObjectKey(1))
                    .unwrap());
                assert_eq!(list_off.len(), 1, "{list_off:?}");
                let r_off = list_off.get(0).unwrap();
                assert_eq!(r_off.test_index, expected_test);
                assert_eq!(r_off.value, Vec3::UNDEF.length());
                assert_ne!(r_off.value, UNDEF_DOUBLE);

                let mut list_on = AssertArray::new();
                assert!(!c
                    .assert_valid(&on, Some(&mut list_on), true, ObjectKey(1))
                    .unwrap());
                assert_eq!(list_on.len(), 1, "{list_on:?}");
                let r_on = list_on.get(0).unwrap();
                assert_eq!(r_on.test_index, expected_test);
                assert_eq!(r_on.value, UNDEF_DOUBLE);
                assert_eq!(r_on.owner_param, Point2::new(expected_param, 0.0));
            }
        }
    }

    #[test]
    fn issue_2009_assert_valid_failed_property_analyses_fail_tests_11_to_14() {
        // tangency-labs/smlib 8843967 (SmBSplineCurve::AssertValid, tests 11-14, WR-663):
        // a single-span Bezier of degree `GW_MAX_DEGREE` (32) passes the knot, endpoint
        // and pole checks and sizes from its control polygon, then fails both
        // `GlobalPropertyAnalysis` calls in `sm_CreateBezSegments` (`NL_DEG_ERR`). Without
        // `query_statuses_pass_on` both failed analyses are ignored as empty solution sets
        // and the curve passes; with the flag, tests 11 & 13 (2D) or 12 & 14 (3D) fail
        // with value `SM_UNDEF_DOUBLE`.
        let mut on = Kernel::default();
        on.fixes.query_statuses_pass_on = true;
        let off = Kernel::default();

        let deg = crate::nurbs::nurbs_crv::MAX_DEGREE;
        let mut knots = vec![0.0; deg + 1];
        knots.extend(core::iter::repeat_n(1.0, deg + 1));
        let pts: Vec<[f64; 3]> = (0..=deg).map(|i| [i as f64, 0.0, 0.0]).collect();

        for (dim, expected) in [(2_u64, [11_u64, 13]), (3, [12_u64, 14])] {
            let mut cur = allocate_nurb_curve(deg, deg, knots.len() - 1).unwrap();
            for (d, s) in cur.cpts_mut().iter_mut().zip(&pts) {
                *d = CPoint::new(s[0], s[1], s[2], NOW);
            }
            cur.knots_mut().copy_from_slice(&knots);
            let c = BSplineCurve::from_nurb(cur, dim, true);

            let mut list_off = AssertArray::new();
            assert!(c
                .assert_valid(&off, Some(&mut list_off), true, ObjectKey(9))
                .unwrap());
            assert!(list_off.is_empty(), "{list_off:?}");

            let mut list_on = AssertArray::new();
            assert!(!c
                .assert_valid(&on, Some(&mut list_on), true, ObjectKey(9))
                .unwrap());
            let indices: Vec<u64> = list_on.reports().iter().map(|r| r.test_index).collect();
            assert_eq!(indices, expected, "{list_on:?}");
            for r in list_on.reports() {
                assert_eq!(r.value, UNDEF_DOUBLE, "{r:?}");
                assert!(r.tol3d > 0.0, "{r:?}");
            }
        }
    }
}
