//! `SmBSplineSurface` with NLib, part 2: corner blends, skinned surfaces, trimming to a
//! domain and evaluation with zero-derivative corrections.
//!
//! Upstream: `source/SMLib/src/SmBSplineSurfaceNLib.cpp`, `source/SMLib/inc/SmBSplineSurface.h`.
//!
//! | Rust                                          | Upstream                                  |
//! |-----------------------------------------------|-------------------------------------------|
//! | [`BSplineSurface::create_corner_blend`], [`OrientType`], [`BlendNeighbour`] | `CreateCornerBlend`, `SmOrientType`, its `SmSurface*` |
//! | [`BSplineSurface::create_corner_blend_on_faces`] | `CreateCornerBlend` with the fork's `lFaceSides` (WR-613) |
//! | [`BSplineSurface::create_skinned_surface`], [`SkinnedSurfaceOptions`] | `CreateSkinnedSurface` |
//! | [`BSplineSurface::trim_with_domain`], [`BSplineSurface::trim_with_domain_with`], [`TrimOptions`] | `TrimWithDomain` |
//! | [`BSplineSurface::evaluate`], [`EvaluateFlags`] | `Evaluate`                              |
//!
//! The other functions of the file belong to `nurbs.b_spline_surface_n_lib_p1` and
//! later units.
//!
//! Deviations:
//! - **Memory and contexts.** `SmNLibStackHandler`, `NL_STACKS` and `SmContext`
//!   arguments are dropped; new surfaces are return values (upstream out-params).
//!   `Notify(SM_NO_PRE_EDIT / SM_NO_POST_EDIT)` in `TrimWithDomain` is omitted (a
//!   surface here has no owner to notify).
//! - **Curve and surface types.** `CreateCornerBlend` takes [`BSplineCurve`] 3D curves
//!   and [`BlendNeighbour`] neighbour surfaces (upstream `SmCurve*` / `SmSurface*`;
//!   `sm_CreateNonRationalCurve` fails with `NER` for any other curve type). A
//!   neighbour is the caller's surface of any kind, evaluated through the virtual
//!   `SmSurface` methods (`DropCurve`, `EvaluateNormal`, `EvaluateNormalSection`,
//!   `Evaluate`, `GetNaturalUVDomain`). `NULL` UV curves and surfaces are `None`. Where upstream indexes past the end of the orientation, UV curve or
//!   surface arrays, or dereferences a `NULL` UV curve or surface (n-sided blends,
//!   bilinear blends with more than four curves), the port returns `InvalidInput`.
//! - **`CreateSkinnedSurface`**'s many arguments are the fields of
//!   [`SkinnedSurfaceOptions`]. Upstream casts away the constness of the rails and
//!   reparametrises them when both are given (`SyncronizeKnotsOfCurves`); the rails
//!   are `&mut` here and are changed the same way. No cross sections is `InvalidInput`
//!   (upstream calls NLib with a highest index of `-1`). The NLib skinning routines
//!   change the cross sections, derivative surfaces and rails of the retries in place,
//!   as upstream.
//! - **`Evaluate`** returns the derivative matrix `D[i][j]` (`i <= u`, `j <= v`
//!   derivatives) instead of writing the caller's array; entries upstream leaves
//!   untouched (the lower half with `bOnlyUpperHalf`) are zero. The unwritten lower
//!   half is 0 here and `SM_UNDEF` upstream (a fresh caller's `SmVector3d` array);
//!   unreachable today (no caller reads it). Its flags are an
//!   [`EvaluateFlags`] value; the internal `bDoZeroSampling` argument is private.
//!   `SmSurfaceEval` retains the matrix written before a zero-tangent correction
//!   failure, because upstream accessors ignore that failure and read the output.
//! - **Tangent fields** are [`TangentField`]s of `SmCurve` views of the B-spline
//!   curves (the same data, not a copy-constructed curve).
//! - **Tangent-field approximation.** `SmTangentField::ApproximateCurve` is the
//!   inherited `SmCrvOnSurf::ApproximateCurve`, which on the new field's empty cache
//!   runs `SmCurve::ApproximateCurve` with the field as the virtual curve.
//!   [`crate::curve::crv_on_surf::CrvOnSurf`] and `Curve::approximate_curve_base` take
//!   a [`crate::curve::curve::p2::Curve`], which has no tangent-field kind, so that
//!   body (and `sm_TestSpanAccuracy`) is repeated here over the field, same order and
//!   bounds (`approximate_tangent_field`). (`SmSurface::EvaluateNormalSection`
//!   is `surface::surface::p3`'s, `SmSurface::EvaluateNormal`
//!   is `surface::surface::p7`'s, `Evaluate1stDerivatives` `surface::surface::p1`'s.) Upstream ignores the status of
//!   most of these calls; an unported one still stops the port.
//! - **Not upstream: `TrimWithDomain` knot snapping** (tangency-labs/solidcore#472), only
//!   with [`TrimOptions::snap_roundoff_to_knots`] ([`BSplineSurface::trim_with_domain`]
//!   keeps upstream's behaviour). Upstream moves a trim boundary that lies within
//!   `SM_EFF_ZERO_SQRT * (1 + |t|)` *inside* a knot away from it by ten times that, so a
//!   boundary one rounding error inside a knot leaves a sliver span (2.1e-4 on a fillet
//!   round at u = 20) where a boundary on the knot leaves none. With the option a
//!   boundary within `SM_EFF_ZERO_PARAM` of a knot is snapped onto it, and the
//!   requested domain passed to `sm_CheckTrimBoundaries` takes the knot too, so the
//!   `[B306]` widening does not run for that rounding error.
//! - Status values upstream ignores stay ignored (`EditParameterization` before
//!   `MakeCurvesCompatible`, the curve evaluations of the n-sided blend, `Scale`,
//!   `GetKnots`, `N_CrvElevateDegree`, `SetMinMax`).
//! - **Not upstream: a Coons corner patch tangent to its sides** (WR-547, the fork's
//!   tangency-labs/smlib@cae8618): the four-sided
//!   `CreateCornerBlend` (not bilinear) replaces its Coons patch by
//!   `make_coons_patch_tangent_to_sides` (`sm_MakeCoonsPatchTangentToSides` and its
//!   helpers `sm_CoonsSideNormal`, `sm_CoonsSideAngle`, `sm_RefineToSpans` and
//!   `sm_ProjectCoonsSideRows`, all private). Its refinement is bounded (8 to 64 spans).
//!   `sm_CoonsSideAngle` makes the patch normal unit by its own length, not `Unitize`
//!   (tangency-labs/smlib@701da4b, same option). Not the fork's (WR-613):
//!   `sm_ProjectCoonsSideRows` moves a point next to a face side and a blend side into
//!   the face side's plane, not onto the two planes' mean, as the Coons corner twist
//!   there is the blend side's (`create_coons_srf`'s face sides).

use crate::curve::curve::p1::{SpanAccuracy, MAX_APPROX_SPANS};
use crate::curve::curve::p2::Curve as SmCurve;
use crate::curve::curve::p2::{
    add_to_curve, flush_crv, pop_stack, push_point, stack_look, CurveEvaluate, CurveKind,
};
use crate::curve::curve::p3::PointSolveOptions;
use crate::curve::curve::p4::is_degenerate_base;
use crate::curve::hermite_curve::HermiteCurve;
use crate::curve::tangent_field::{CurveEnd as TangentFieldEnd, TangentDirType, TangentField};
use crate::error::nurbs::BSplineFailure;
use crate::error::{ignored, Result, SolidError, SM_ERR_NULL_POINTER};
use crate::geom::certified::{next_down, next_up};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::{Extent2d, SS_UMAX, SS_UMIN, SS_VMIN};
use crate::geom::extent3d::Extent3d;
use crate::geom::global_solver::SolverOperation;
use crate::geom::tol::ANG_TOL_DEG;
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::acos;
use crate::geom::vector3d::Point3;
use crate::geom::vector3d::Vec3;
use crate::math::{
    max2, min2, EFF_ZERO, EFF_ZERO_PARAM, EFF_ZERO_SQ, EFF_ZERO_SQRT, PI, UNDEF_DOUBLE,
};
use crate::nurbs::b_spline_curve::{
    BSplineCurve, BSplineCurveForm, BoundingBoxRequest, CanonicalData, ControlPointForm, KnotType,
};
use crate::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm, SurfParamType};
use crate::nurbs::crv_basic::Boundary;
use crate::nurbs::crv_basic::Curve;
use crate::nurbs::crv_tool::p2::KnotMap;
use crate::nurbs::geom::Rectangle;
use crate::nurbs::index::at;
use crate::nurbs::knots::Side;
use crate::nurbs::math::{gamma, srf_power_basis_eval_derivs, MAX_DERIVATIVE, UNIT};
use crate::nurbs::srf_adv::p1::{create_skin_spine, SkinDerivs, SkinSpineOptions, SpineAlignment};
use crate::nurbs::srf_adv::p5::{
    create_skin_srf_boundary_continuity_mapped, BoundaryContinuity, SkinBoundaryOptions,
};
use crate::nurbs::srf_basic::{Direction, Surface};
use crate::nurbs::srf_tool::p1::extract_patch;
use crate::surface::surface::Surface as SmSurface;

use super::p1::{
    create_nlib_curve, create_nlib_surface, create_non_rational_curve_with_context, make_knots,
    COONS_FACE_BOTTOM, COONS_FACE_LEFT, COONS_FACE_RIGHT, COONS_FACE_TOP,
};
use crate::intersect::surface_drop_lib::p3::DropSolveContext;

/// `SM_ZONE_TOL_3D`.
const ZONE_TOL_3D: f64 = 1.0e-5;

const OP_CORNER: &str = "nurbs::b_spline_surface_n_lib::p2::BSplineSurface::create_corner_blend";
const OP_SKIN: &str = "nurbs::b_spline_surface_n_lib::p2::BSplineSurface::create_skinned_surface";
const OP_TRIM: &str = "nurbs::b_spline_surface_n_lib::p2::BSplineSurface::trim_with_domain";
const OP_EVAL: &str = "nurbs::b_spline_surface_n_lib::p2::BSplineSurface::evaluate";

// ---- types ----------------------------------------------------------------------------

/// Orientation of a boundary curve relative to the curve order (`SmOrientType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum OrientType {
    /// `SM_OT_UNKNOWN` (0).
    #[default]
    Unknown,
    /// `SM_OT_SAME` (1): parametrised in the direction of the curve order.
    Same,
    /// `SM_OT_OPPOSITE` (2): parametrised against it.
    Opposite,
}

/// A neighbour surface of [`BSplineSurface::create_corner_blend`] (upstream's
/// `SmSurface*`).
#[derive(Debug, Clone, Copy)]
pub struct BlendNeighbour<'a> {
    /// The surface itself, with its kind and owner: `sm_CreateNonRationalCurve`
    /// drops rational curves onto it through the virtual `SmSurface::DropCurve`, and
    /// the cross-tangent construction evaluates it through the virtual
    /// `SmSurface` evaluators.
    pub surface: &'a SmSurface,
}

/// Options of [`BSplineSurface::trim_with_domain_with`]. `Default` is upstream's
/// `TrimWithDomain`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TrimOptions {
    /// Not upstream (tangency-labs/solidcore#472): a trim boundary at most
    /// `SM_EFF_ZERO_PARAM` inside a knot is snapped onto the knot instead of being
    /// moved `10 * SM_EFF_ZERO_SQRT * (1 + |t|)` away from it, which leaves a sliver
    /// span. Off by default (upstream).
    pub snap_roundoff_to_knots: bool,
}

/// The flags of [`BSplineSurface::evaluate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EvaluateFlags {
    /// Span picked at a u knot (`bUFromLeft`: `TRUE` is [`Side::Left`], the span
    /// starting at the knot).
    pub u_side: Side,
    /// Span picked at a v knot (`bVFromLeft`).
    pub v_side: Side,
    /// Compute only `D[i][j]` with `i + j <= highest_u_deriv` (`bOnlyUpperHalf`; the
    /// two orders must then be equal).
    pub only_upper_half: bool,
    /// Replace zero first derivatives by short, properly directed vectors
    /// (`bNonZeroTangents`, upstream default `TRUE`).
    pub non_zero_tangents: bool,
}

impl Default for EvaluateFlags {
    /// From the left in both directions, full matrix, non-zero tangents.
    fn default() -> Self {
        Self {
            u_side: Side::Left,
            v_side: Side::Left,
            only_upper_half: false,
            non_zero_tangents: true,
        }
    }
}

/// The optional inputs of [`BSplineSurface::create_skinned_surface`].
#[derive(Debug)]
pub struct SkinnedSurfaceOptions<'a> {
    /// The cross sections have the same degree and are synchronized (`bSyncronized`).
    pub synchronized: bool,
    /// [`SurfParamType::U`]: the cross sections become u isoparameter curves (NLib
    /// `NL_UDIR`); anything else `NL_VDIR` (`eCrossSectionSurfParam`).
    pub cross_section_surf_param: SurfParamType,
    /// `0` interpolates the cross sections, else approximates within this tolerance
    /// (`dThisApproxTol3d`).
    pub approx_tol: f64,
    /// Rail along the minimum parameter side (`pRail1`).
    pub rail1: Option<&'a mut BSplineCurve>,
    /// Rail along the maximum parameter side (`pRail2`).
    pub rail2: Option<&'a mut BSplineCurve>,
    /// Use `rail1` as spine curve, ignoring `rail2` (`bRail1UsedAsSpineCurve`).
    pub rail1_used_as_spine_curve: bool,
    /// Rail parameters of the cross section / rail intersections
    /// (`pIntersectionsWithRails`): `[0..k]` on `rail1`, `[k..2k]` on `rail2`.
    pub intersections_with_rails: Option<&'a [f64]>,
    /// Cross-boundary derivative surfaces at the start and end of the skin
    /// (`pDerivSurf`; `None` is a `NULL` array).
    pub deriv_surfs: Option<[Option<&'a BSplineSurface>; 2]>,
    /// Degree in the sweep direction (`lDegree`, upstream default 3).
    pub degree: usize,
    /// Not upstream: the skinning parameter of each cross section, increasing from 0 to 1,
    /// used where there are no rails (with rails, `intersections_with_rails` governs);
    /// `None` takes the chord-length parameters, as upstream.
    pub section_params: Option<&'a [f64]>,
}

impl Default for SkinnedSurfaceOptions<'_> {
    fn default() -> Self {
        Self {
            synchronized: false,
            cross_section_surf_param: SurfParamType::U,
            approx_tol: 0.0,
            rail1: None,
            rail2: None,
            rail1_used_as_spine_curve: false,
            intersections_with_rails: None,
            deriv_surfs: None,
            degree: 3,
            section_params: None,
        }
    }
}

// ---- tangent-field callees --------------------------------------------------------------

/// `curve` as an `SmCurve` (the same object upstream: the NURB and flags as they are,
/// without the copy constructor's knot fix-up).
fn as_sm_curve(curve: &BSplineCurve) -> SmCurve {
    SmCurve::from(BSplineCurve {
        dim: curve.dim,
        nurb: curve.nurb.clone(),
        nurb_is_borrowed: curve.nurb_is_borrowed,
        out_of_bounds_enabled: curve.out_of_bounds_enabled,
        knot_type: curve.knot_type,
        form: curve.form,
    })
}

/// The operation path of [`approximate_tangent_field`].
const OP_APPROX_FIELD: &str = "nurbs::b_spline_surface_n_lib::p2::approximate_tangent_field";

/// Why a surface evaluation that comes back short is the kernel's fault: `evaluate` returns
/// one row per u order and one entry per v order asked for.
const SHORT_SURFACE_EVALUATION: &str = "the surface evaluation returned fewer derivatives than \
     the orders it was asked for, and it returns one per order asked";

/// A tangent field seen as an `SmCurve` (its virtual `Evaluate`, `EvaluatePoint`,
/// `GetNaturalInterval` and `GetDim` as `SmCurve::ApproximateCurve` reaches them).
struct FieldCurve<'f, 'a>(&'f TangentField<'a, SmSurface>);

impl CurveEvaluate for FieldCurve<'_, '_> {
    fn dim(&self) -> u64 {
        // SmCrvOnSurf: always 3D
        3
    }

    fn natural_interval(&self) -> Extent1d {
        self.0.natural_interval()
    }

    fn evaluate(
        &self,
        k: &crate::kernel::Kernel,
        t: f64,
        num_derivs: usize,
        _from_left: bool,
        _non_zero_tangents: bool,
    ) -> Result<Vec<Vec3>> {
        self.0.evaluate(k, t, num_derivs)
    }

    fn evaluate_point(&self, k: &crate::kernel::Kernel, t: f64) -> Result<Point3> {
        self.0.evaluate_point(k, t)
    }
}

/// `Evaluate(t, pv, 1, bFromLeft, bNonZeroTangents)` into a position/tangent pair.
fn field_eval2(
    k: &crate::kernel::Kernel,
    curve: &FieldCurve<'_, '_>,
    t: f64,
    from_left: bool,
) -> Result<[Vec3; 2]> {
    let v = curve.evaluate(k, t, 1, from_left, true)?;
    let mut out = [Vec3::UNDEF; 2];
    for (d, s) in out.iter_mut().zip(v) {
        *d = s;
    }
    Ok(out)
}

/// Maximum adaptive Hermite subdivisions in one approximation, as
/// `curve::curve::p1`'s `SmCurve::ApproximateCurve` (see its deviations).
const MAX_APPROXIMATION_SUBDIVISIONS: usize = 8192;

/// `sm_TestSpanAccuracy` on a tangent field (the body of
/// [`crate::curve::curve::p1::test_span_accuracy`], whose curve is the field).
fn field_span_accuracy(
    k: &crate::kernel::Kernel,
    curve: &FieldCurve<'_, '_>,
    last_t: f64,
    last_pv: [Point3; 2],
    curr_t: f64,
    curr_pv: [Point3; 2],
    approx_tol: f64,
    match_parameterization: bool,
) -> Result<SpanAccuracy> {
    // init output
    let mut out = SpanAccuracy {
        passes: false,
        max_dist: 0.0,
    };

    // locals
    let mut force_pass = false;
    let this_span = curr_t - last_t;
    let t_scale = 1.0 + curr_t.abs();
    let ivl = curve.natural_interval();
    let min_step = 1500.0;
    let mut num_test_points: u32 = 3;

    // when span is getting very small - just pass it
    if this_span < EFF_ZERO * t_scale {
        force_pass = true;
    }

    // Don't let the interval get too small
    if this_span < (ivl.length()) / min_step {
        force_pass = true;
    }

    // hermite cuve between interval end points
    let hermite = HermiteCurve::new(
        last_pv[0],
        last_pv[1] * this_span,
        curr_pv[0],
        curr_pv[1] * this_span,
        3,
    );
    let hermite_curve = SmCurve::from(hermite);

    if force_pass {
        // get a better error value if we're going to keep this step.
        num_test_points = 7;
    }

    for ii in 0..num_test_points {
        let normalized_param = (f64::from(ii) + 1.0) / (f64::from(num_test_points) + 1.0);
        let crv_param = last_t + normalized_param * this_span;

        // Evaluate the curve.  Then, if we're matching parameterization,
        // evaluate the Hermite curve at the same (relative) parameter,
        // else find the closest point on the Hermite curve.
        let crv_pnt = curve.evaluate_point(k, crv_param)?;

        let approx_dist = if match_parameterization {
            // Approx dist = DistanceBetween(Curve(param), Hermite(param))
            let herm_pnt = hermite.evaluate_point(normalized_param);
            herm_pnt.distance(crv_pnt)
        } else {
            // Approx dist = DistanceBetween(Curve(param), NearestPointOnHermite)
            let hermite_domain = Extent1d::from_min_max(0.0, 1.0);
            let r = hermite_curve.local_point_solve::<()>(
                k,
                &hermite_domain,
                SolverOperation::Minimize,
                crv_pnt,
                &PointSolveOptions::default(),
                normalized_param,
            );
            // save nearest distance (when drop point failed - load a big distance to
            // quit testing)
            match ignored(r)? {
                Some((true, sol)) => sol.start.solution_value,
                _ => 2.0 * approx_tol,
            }
        };

        // When ApproxDist is too big
        if approx_dist > approx_tol && !force_pass {
            // set output and quit testing
            out.passes = false;
            return Ok(out);
        }
        out.max_dist = max2(out.max_dist, approx_dist);
    }

    // when bForcePass == TRUE
    out.passes = true;
    Ok(out)
}

/// `SmCrvOnSurf::ApproximateCurve(ctx, knots, ang_tol, max_gap, curve, TRUE, FALSE,
/// FALSE)` on a new tangent field (inherited): its `GetOrCreateBSApproxPointer` with
/// the given breaks and `bMatchParam = FALSE` on an empty cache runs
/// `SmCurve::ApproximateCurve(ctx, breaks, tol, gap, approx, TRUE, FALSE, FALSE)` and
/// copies the result. The body is that of
/// [`crate::curve::curve::p1`]'s `Curve::approximate_curve_base` with the field as the
/// virtual curve (a tangent field is no B-spline, so neither the copy shortcut nor
/// the "input curve is better" copy applies).
fn approximate_tangent_field(
    k: &crate::kernel::Kernel,
    field: &TangentField<'_, SmSurface>,
    breaks: &[f64],
    approx_tol_3d: f64,
) -> Result<BSplineCurve> {
    let curve = FieldCurve(field);
    let create_analytics = true;
    let match_parameterization = false;

    // need 2 breakpoints to define the curve interval
    let [first, .., last] = *breaks else {
        return Err(SolidError::b_spline(
            OP_APPROX_FIELD,
            BSplineFailure::TooFewBreakpoints {
                given: breaks.len(),
            },
        ));
    };

    // low work - degenerate curves
    if is_degenerate_base(k, &curve, EFF_ZERO, None)? {
        let pnt = curve.evaluate_point(k, first)?;
        let degenerate = BSplineCurve::create_degenerate_curve(curve.dim(), pnt)?;
        let mut c = SmCurve::from(degenerate);
        let ivl = Extent1d::from_min_max(first, last);
        // (status ignored upstream; passed on as SmCurve::ApproximateCurve in the fork,
        // tangency-labs/smlib 8311f8e)
        c.edit_parameterization(k, &ivl)?;
        let CurveKind::BSpline(bs) = c.into_kind() else {
            return Err(SolidError::invalid_input(
                OP_APPROX_FIELD,
                "the approximation is not an SmBSplineCurve",
            ));
        };
        return Ok(bs.copy());
    }

    // locals
    let mut stack: Vec<f64> = Vec::with_capacity(256 * 4 * 2);
    let mut ctrl_poly: Vec<Point3> = Vec::with_capacity(256);
    let mut knots: Vec<f64> = Vec::with_capacity(256);
    let mut subdivisions = 0;

    // for every interval defined by input BreakParams array
    for w in breaks.windows(2) {
        let (Some(&a), Some(&b)) = (w.first(), w.get(1)) else {
            continue;
        };
        let mut last_t = a;
        let mut curr_t = b;

        // interval start position and tangent (Last)
        let mut last_pv = field_eval2(k, &curve, last_t, true)?; // NonZeroTangent values
        {
            // walk to a nonZero Tangent evaluation
            let mut step = 0.0001;
            while last_pv[1].length_squared() < EFF_ZERO_SQ * 100000.0 {
                let tmp = last_pv[0];
                last_pv = field_eval2(k, &curve, last_t + step * (curr_t - last_t), true)?;
                last_pv[0] = tmp;
                step *= 2.0;
                if step > 0.5 {
                    break;
                }
            }
        }

        // interval end position and tangent (Curr)
        let mut curr_pv = field_eval2(k, &curve, curr_t, false)?; // NonZeroTangent values
        {
            let mut step = 0.0001;
            while curr_pv[1].length_squared() < EFF_ZERO_SQ * 100000.0 {
                let tmp = curr_pv[0];
                curr_pv = field_eval2(k, &curve, curr_t - step * (curr_t - last_t), true)?;
                curr_pv[0] = tmp;
                step *= 2.0;
                if step > 0.5 {
                    break;
                }
            }
        }

        // Place Curr Point [param, posXYZ, tanXYZ] on sStack
        push_point(curr_t, curr_pv, &mut stack);

        // For each span subdivide until our approximating curve is accurate enough.
        while !stack.is_empty() {
            // get last point's param, pos, and tan from stack - leave stack unmodified
            (curr_t, curr_pv) = stack_look(&stack)?;

            // Test if HermiteSpan is within ApproxTol of thisCurve interval
            let acc = field_span_accuracy(
                k,
                &curve,
                last_t,
                last_pv,
                curr_t,
                curr_pv,
                approx_tol_3d,
                match_parameterization,
            )?;

            // When Span passes test (or If step size gets real small just accept it)
            if acc.passes || (last_t - curr_t).abs() < EFF_ZERO {
                // (not upstream) bound the growing curve: see MAX_APPROX_SPANS
                if knots.len() > MAX_APPROX_SPANS {
                    return Err(SolidError::invalid_input(
                        OP_APPROX_FIELD,
                        "approximation needs more than MAX_APPROX_SPANS spans",
                    ));
                }

                // Append Hermite interval to end of growing piecewise cubic bezier
                add_to_curve(last_pv, curr_pv, last_t, curr_t, &mut ctrl_poly, &mut knots)?;

                // for next iteration - move Curr param, pos, and tan to Last
                last_t = curr_t;
                last_pv = curr_pv;

                // for next iteration - pop stack
                pop_stack(&mut stack)?;
            } else {
                if subdivisions >= MAX_APPROXIMATION_SUBDIVISIONS {
                    return Err(SolidError::b_spline(
                        OP_APPROX_FIELD,
                        BSplineFailure::SubdivisionCap {
                            limit: MAX_APPROXIMATION_SUBDIVISIONS,
                        },
                    ));
                }
                subdivisions += 1;

                // get param mid point and associated curve pos and tan
                let mid_t = (last_t + curr_t) / 2.0;
                if !mid_t.is_finite() || mid_t == last_t || mid_t == curr_t {
                    return Err(SolidError::b_spline(
                        OP_APPROX_FIELD,
                        BSplineFailure::IntervalNotSplittable {
                            lo: last_t,
                            hi: curr_t,
                        },
                    ));
                }
                let mid_pv = field_eval2(k, &curve, mid_t, true)?; // NonZeroTangent values
                curr_t = mid_t;
                curr_pv = mid_pv;

                // push midPoint onto stach
                push_point(curr_t, curr_pv, &mut stack);
            }
        }
    }

    // create pBSC curve from control polygon
    let bsc = flush_crv(approx_tol_3d, &mut ctrl_poly, &mut knots, create_analytics)?.ok_or(
        SolidError::invalid_input(OP_APPROX_FIELD, "no approximating curve built"),
    )?;

    // pBSCurve->Copy(crContext, rpNewBSplineCurve)
    Ok(bsc.copy())
}

/// `SmTangentField::Create(ctx, tol, boundary, uv, surface, start, t0, SAME, end, t1,
/// SAME)` followed by its `ApproximateCurve` over the knots of `boundary`: the
/// cross-tangent curve of `boundary` on `surface`.
fn tangent_field_curve(
    k: &crate::kernel::Kernel,
    tol: f64,
    boundary: &BSplineCurve,
    uv_curve: &BSplineCurve,
    surface: &SmSurface,
    (start, start_param): (&BSplineCurve, f64),
    (end, end_param): (&BSplineCurve, f64),
    ang_tol: f64,
) -> Result<BSplineCurve> {
    let base_3d = as_sm_curve(boundary);
    let base_uv = as_sm_curve(uv_curve);
    let start_crv = as_sm_curve(start);
    let end_crv = as_sm_curve(end);
    let field = TangentField::create(
        k,
        tol,
        &base_3d,
        &base_uv,
        surface,
        TangentFieldEnd {
            curve: &start_crv,
            param: start_param,
            opposite: false,
        },
        TangentFieldEnd {
            curve: &end_crv,
            param: end_param,
            opposite: false,
        },
    )?;

    // build an approximate piecewise hermite curve to the crossTangent curve
    let knots = unique_knots(boundary);
    approximate_tangent_field(k, &field, &knots, ang_tol)
}

/// `new SmTangentField(uv, surface, SM_INTERPOLATE_DIR, start, end)` followed by its
/// `ApproximateCurve(knots, ang_tol, ..)`.
fn interpolated_tangent_field_curve(
    k: &crate::kernel::Kernel,
    uv_curve: &BSplineCurve,
    surface: &SmSurface,
    start: Vec3,
    end: Vec3,
    knots: &[f64],
    ang_tol: f64,
) -> Result<BSplineCurve> {
    let base_uv = as_sm_curve(uv_curve);
    let field = TangentField::new(
        k,
        &base_uv,
        surface,
        TangentDirType::Interpolate,
        start,
        end,
        None,
    )?;
    approximate_tangent_field(k, &field, knots, ang_tol)
}

/// `SmSurface::EvaluateNormal(uv, TRUE, TRUE, normal)`.
fn evaluate_normal(k: &crate::kernel::Kernel, surface: &SmSurface, uv: Point2) -> Result<Vec3> {
    surface.evaluate_normal(k, uv, Side::Left, Side::Left)
}

/// `SmSurface::EvaluateNormalSection(uv, TRUE, TRUE, direction, ..)`: the cross
/// tangent (the unit projection of `direction` into the tangent plane).
fn evaluate_normal_section(
    k: &crate::kernel::Kernel,
    surface: &SmSurface,
    uv: Point2,
    dir: Vec3,
) -> Result<Vec3> {
    Ok(surface
        .evaluate_normal_section(k, uv, Side::Left, Side::Left, dir)?
        .tangent)
}

/// `SmSurface::Evaluate1stDerivatives(uv, TRUE, TRUE, ..)`: `(Su, Sv)`.
fn evaluate_1st_derivatives(
    k: &crate::kernel::Kernel,
    surface: &SmSurface,
    uv: Point2,
) -> Result<(Vec3, Vec3)> {
    let (_, du, dv) = crate::surface::surface::p1::evaluate_1st_derivatives(
        k,
        surface,
        uv,
        Side::Left,
        Side::Left,
    )?;
    Ok((du, dv))
}

/// `SmCurve::LocalCurveIntersect(domain, other, other_domain, tol, guess, other_guess,
/// ..)`: whether an intersection was found and its parameter on `rail` (the status is
/// ignored upstream: a failure is no intersection; `Unported` is propagated).
fn local_curve_intersect(
    k: &crate::kernel::Kernel,
    rail: &BSplineCurve,
    domain: &Extent1d,
    other: &BSplineCurve,
    other_domain: &Extent1d,
    tol: f64,
    guess: f64,
    other_guess: f64,
) -> Result<(bool, f64)> {
    let r = ignored(crate::curve::curve::p3::local_curve_intersect(
        k,
        rail,
        domain,
        other,
        other_domain,
        tol,
        guess,
        other_guess,
    ))?;
    Ok(match r.flatten() {
        Some(i) => (true, i.my_param),
        None => (false, UNDEF_DOUBLE),
    })
}

// ---- helpers --------------------------------------------------------------------------

/// Runs `f` on the two curves `curves[a]` and `curves[b]` as one slice (upstream's
/// array of pointers to them).
fn with_pair<R>(
    curves: &mut [BSplineCurve],
    a: usize,
    b: usize,
    f: impl FnOnce(&mut [BSplineCurve]) -> R,
) -> Result<R> {
    let oob = || SolidError::invalid_input(OP_CORNER, "curve index out of range");
    let first = core::mem::take(curves.get_mut(a).ok_or_else(oob)?);
    let second = core::mem::take(curves.get_mut(b).ok_or_else(oob)?);
    let mut pair = [first, second];
    let r = f(&mut pair);
    let [first, second] = pair;
    *curves.get_mut(a).ok_or_else(oob)? = first;
    *curves.get_mut(b).ok_or_else(oob)? = second;
    Ok(r)
}

/// `rail.GetKnots(knots)`: the distinct knots (empty on failure; upstream ignores the
/// status).
fn unique_knots(curve: &BSplineCurve) -> Vec<f64> {
    curve.knots(None).map(|k| k.0).unwrap_or_default()
}

impl BSplineSurface {
    // ---- CreateCornerBlend ----------------------------------------------------------

    /// Blend surfaces filling the region bounded by `curves_3d` (`CreateCornerBlend`).
    ///
    /// Curve order is counterclockwise starting with the left curve (u minimum):
    /// left, bottom, right, top for four curves; `curve_orients[i]` tells whether
    /// curve `i` runs in that direction ([`OrientType::Same`]). Rational curves are
    /// first approximated within `approx_tol / 2` by non-rational ones (with their UV
    /// curves dropped onto the surfaces).
    ///
    /// With four curves the result is one Coons patch
    /// ([`BSplineSurface::create_coons_patch`]): bilinear with
    /// `create_bilinear_patch` (UV curves and surfaces are then optional), else
    /// bicubic matching the cross tangents of the `neighbours` surfaces along their
    /// `uv_curves` within `tangency_tol_radians` (one degenerate curve without surface
    /// gives a triangular patch). More curves give an n-sided patch
    /// ([`BSplineSurface::create_n_sided_patch`]) matching the neighbours' cross
    /// tangents.
    ///
    /// # Errors
    ///
    /// `SM_ERR` for fewer than four curves, a non-degenerate curve without surface,
    /// corner gaps above `approx_tol` and the failures of the called routines;
    /// `SM_ERR_NULL_POINTER` when a G1 blend misses too many UV curves or surfaces;
    /// `InvalidInput` for input arrays shorter than `curves_3d` or missing UV curves or
    /// surfaces where upstream dereferences them; [`SolidError::Unported`] from the
    /// neighbour surfaces' evaluators.
    pub fn create_corner_blend(
        k: &crate::kernel::Kernel,
        curves_3d: &[BSplineCurve],
        curve_orients: &[OrientType],
        uv_curves: &[Option<BSplineCurve>],
        neighbours: &[Option<BlendNeighbour<'_>>],
        approx_tol: f64,
        tangency_tol_radians: f64,
        create_bilinear_patch: bool,
    ) -> Result<Vec<Self>> {
        Self::create_corner_blend_on_faces(
            k,
            curves_3d,
            curve_orients,
            uv_curves,
            neighbours,
            approx_tol,
            tangency_tol_radians,
            create_bilinear_patch,
            0,
        )
    }

    /// [`BSplineSurface::create_corner_blend`] with the fork's `lFaceSides` (WR-613,
    /// tangency-labs/smlib@7fd9fb0): bit `i` of `face_sides` marks `curves_3d[i]` as
    /// lying on a face of the body. A four-sided bicubic patch passes the marks on to
    /// [`BSplineSurface::create_coons_patch_on_faces`] in its left, right, bottom, top
    /// order; other patches ignore them. `0` is upstream's `CreateCornerBlend`.
    ///
    /// # Errors
    ///
    /// As [`BSplineSurface::create_corner_blend`].
    pub fn create_corner_blend_on_faces(
        k: &crate::kernel::Kernel,
        curves_3d: &[BSplineCurve],
        curve_orients: &[OrientType],
        uv_curves: &[Option<BSplineCurve>],
        neighbours: &[Option<BlendNeighbour<'_>>],
        approx_tol: f64,
        tangency_tol_radians: f64,
        create_bilinear_patch: bool,
        face_sides: u32,
    ) -> Result<Vec<Self>> {
        let refs: Vec<Option<&BSplineCurve>> = curves_3d.iter().map(Some).collect();
        Self::create_corner_blend_in::<crate::surface::surface::p4::NoObject>(
            k,
            &refs,
            curve_orients,
            uv_curves,
            neighbours,
            approx_tol,
            tangency_tol_radians,
            create_bilinear_patch,
            None,
            face_sides,
        )
    }

    /// [`BSplineSurface::create_corner_blend`] over general 3D [`SmCurve`]s
    /// (`const SmTArray<SmCurve *>&`, downcast per curve via `SM_CAST_PTR(SmBSplineCurve,
    /// pCurve)` in loop order) whose rational-curve drops onto the neighbour surfaces
    /// run under the caller's face context: `context` builds the surface caches and
    /// `handles[i]` names `neighbours[i]` in them.
    ///
    /// # Errors
    ///
    /// As [`BSplineSurface::create_corner_blend`], `SM_ERR_NULL_POINTER` when a reached
    /// 3D curve is not `SmBSplineCurve`-derived, and those of the context.
    pub fn create_corner_blend_with_context<O: crate::geom::solution_array::SolutionObject>(
        k: &crate::kernel::Kernel,
        curves_3d: &[SmCurve],
        curve_orients: &[OrientType],
        uv_curves: &[Option<BSplineCurve>],
        neighbours: &[Option<BlendNeighbour<'_>>],
        approx_tol: f64,
        tangency_tol_radians: f64,
        create_bilinear_patch: bool,
        context: Option<(
            &mut dyn crate::surface::surface::p1::SurfaceSolveContext<O>,
            &[Option<O>],
        )>,
    ) -> Result<Vec<Self>> {
        let refs: Vec<Option<&BSplineCurve>> = curves_3d
            .iter()
            .map(|c| match c.kind() {
                CurveKind::BSpline(b) => Some(b),
                CurveKind::Line(l) => Some(&l.base),
                CurveKind::Ellipse(e) => Some(e.base()),
                CurveKind::Hyperbola(h) => Some(h.base()),
                CurveKind::Parabola(p) => Some(p.base()),
                _ => None,
            })
            .collect();
        Self::create_corner_blend_in(
            k,
            &refs,
            curve_orients,
            uv_curves,
            neighbours,
            approx_tol,
            tangency_tol_radians,
            create_bilinear_patch,
            context,
            0,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn create_corner_blend_in<O: crate::geom::solution_array::SolutionObject>(
        k: &crate::kernel::Kernel,
        curves_3d: &[Option<&BSplineCurve>],
        curve_orients: &[OrientType],
        uv_curves: &[Option<BSplineCurve>],
        neighbours: &[Option<BlendNeighbour<'_>>],
        approx_tol: f64,
        tangency_tol_radians: f64,
        create_bilinear_patch: bool,
        mut context: Option<(
            &mut dyn crate::surface::surface::p1::SurfaceSolveContext<O>,
            &[Option<O>],
        )>,
        face_sides: u32,
    ) -> Result<Vec<Self>> {
        const OP: &str = OP_CORNER;

        // check state - must have 4 or more edges
        if curves_3d.len() < 4 {
            return Err(SolidError::b_spline(
                OP,
                BSplineFailure::TooFewBoundaryCurves {
                    given: curves_3d.len(),
                },
            ));
        }
        let curve_cnt = curves_3d.len();
        let surfaces: Vec<Option<&SmSurface>> =
            neighbours.iter().map(|n| n.map(|n| n.surface)).collect();
        let surfaces = surfaces.as_slice();
        if uv_curves.len() < curve_cnt || surfaces.len() < curve_cnt {
            return Err(SolidError::invalid_input(
                OP,
                "fewer UV curves or surfaces than curves",
            ));
        }

        // check state - when bCreateBilinearPatch == FALSE, UVCurves and surfaces are
        // required for every curve except when the 4th curve in a 4 sided patch is
        // degenerate
        if !create_bilinear_patch {
            let mut null_count = 0usize;

            // for every boundary curve
            for (uv, srf) in uv_curves.iter().zip(surfaces).take(curve_cnt) {
                // count the associated NULL UVTrimCurve and Surface inputs
                if uv.is_none() || srf.is_none() {
                    null_count += 1;
                }

                // Check inputs for triangle, 4 sided and NSided cases
                if ((curve_cnt == 4 && null_count > 1) || (curve_cnt > 4 && null_count > 0))
                    && (uv.is_none() || srf.is_none())
                {
                    return Err(SolidError::b_spline(
                        OP,
                        BSplineFailure::BoundaryWithoutSurface,
                    ));
                }
            }
        }

        // convert all input edges from NURBs to NUBs
        let mut boundaries: Vec<BSplineCurve> = Vec::new();
        let mut uv_crvs: Vec<Option<BSplineCurve>> = Vec::new();
        for (i, ((curve, uv), srf)) in curves_3d.iter().zip(uv_curves).zip(neighbours).enumerate() {
            // sm_CreateNonRationalCurve: SmBSplineCurve *pNurb = SM_CAST_PTR(SmBSplineCurve, pCurve); NER(pNurb);
            let curve = (*curve)
                .ok_or_else(|| SolidError::b_spline(OP, BSplineFailure::BoundaryCurveMissing))?;
            // copy NUBs and their UVTrimCurves, approx NURBs and drop them to make their
            // UVTrimCurves
            let base = srf.map(|n| n.surface);
            let ctx = context.as_mut().map(|(solver, handles)| DropSolveContext {
                solver: &mut **solver,
                handle: handles.get(i).copied().flatten(),
            });
            let (boundary, ps_curve) = create_non_rational_curve_with_context(
                k,
                curve,
                uv.as_ref(),
                base,
                approx_tol / 2.0,
                ctx,
            )?;
            boundaries.push(boundary); // cleaned up 3d boundary curves
            uv_crvs.push(ps_curve); // associated UVTrimCurves
        }

        // If we have 4 curves then we need to reverse opposite ones and make them
        // compatible
        if boundaries.len() == 4 {
            // Not upstream (WR-613): the given 3D curves, as the body's edges run along
            // them, that the face-sided patch's guard reads its seams along
            let originals: Vec<SmCurve> = if face_sides != 0 && !create_bilinear_patch {
                // (every curve is given: the boundaries above were built from them)
                curves_3d
                    .iter()
                    .flatten()
                    .copied()
                    .map(|c| SmCurve::from(c.clone()))
                    .collect()
            } else {
                Vec::new()
            };
            return corner_blend_4(
                k,
                boundaries,
                &originals,
                uv_crvs,
                curve_orients,
                surfaces,
                approx_tol,
                tangency_tol_radians,
                create_bilinear_patch,
                face_sides,
            );
        }

        corner_blend_n(
            k,
            boundaries,
            uv_crvs,
            curve_orients,
            surfaces,
            approx_tol,
            tangency_tol_radians,
        )
    }

    // ---- CreateSkinnedSurface -------------------------------------------------------

    /// The surface skinned through `cross_sections`, optionally along one or two rails
    /// and with cross-boundary derivative surfaces at its start and end
    /// (`CreateSkinnedSurface`).
    ///
    /// With `rail1_used_as_spine_curve` the cross sections are aligned along `rail1`
    /// as spine (`N_CreateSkinSpine`); otherwise they are skinned with
    /// `N_CreateSkinSrfBoundaryContinuity`, retried without the knot vector and then
    /// without the derivative surfaces when it fails. Without derivative surfaces the
    /// degree is lowered to `cross sections - 1` when needed. Two rails are
    /// synchronized first (and changed in place).
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` for a spine without `rail1` or intersection parameters;
    /// `SM_ERR` for rails without intersection parameters, curves or surfaces without
    /// NURB, or when the skinning fails; `InvalidInput` for no cross sections or too
    /// few intersection parameters; [`SolidError::Unported`] when two rails change
    /// their domains (`SmCurve::LocalCurveIntersect`).
    pub fn create_skinned_surface(
        k: &crate::kernel::Kernel,
        cross_sections: &[BSplineCurve],
        opts: SkinnedSurfaceOptions<'_>,
    ) -> Result<Self> {
        Self::create_skinned_surface_mapped(k, cross_sections, opts, false).map(|(s, _)| s)
    }

    /// [`BSplineSurface::create_skinned_surface`], also saying, when `want_maps`, where each
    /// cross section's knots went in the surface's knots across the sections ([`KnotMap`],
    /// one per section; `None` for a section whose map a step broke, and for every section on
    /// the spine path, which this does not trace). The surface is the one made without.
    ///
    /// # Errors
    ///
    /// Those of [`BSplineSurface::create_skinned_surface`].
    pub(crate) fn create_skinned_surface_mapped(
        k: &crate::kernel::Kernel,
        cross_sections: &[BSplineCurve],
        opts: SkinnedSurfaceOptions<'_>,
        want_maps: bool,
    ) -> Result<(Self, Option<Vec<Option<KnotMap>>>)> {
        const OP: &str = OP_SKIN;
        let SkinnedSurfaceOptions {
            synchronized,
            cross_section_surf_param,
            approx_tol,
            mut rail1,
            mut rail2,
            rail1_used_as_spine_curve,
            intersections_with_rails,
            deriv_surfs,
            degree,
            section_params,
        } = opts;
        let num_sections = cross_sections.len();

        // check input - when using a spine, spine curve and IntersectionsWithRails
        // should be given
        if rail1_used_as_spine_curve {
            if rail1.is_none() {
                return Err(SolidError::b_spline(OP, BSplineFailure::SpineRailMissing));
            }
            if intersections_with_rails.is_none() {
                return Err(SolidError::b_spline(
                    OP,
                    BSplineFailure::RailIntersectionsMissing,
                ));
            }
        }

        if (rail1.is_some() || rail2.is_some()) && intersections_with_rails.is_none() {
            // the same condition, with upstream's other status here
            return Err(SolidError::b_spline_with_status(
                OP,
                BSplineFailure::RailIntersectionsMissing,
                crate::error::SM_ERR,
            ));
        }
        if num_sections == 0 {
            return Err(SolidError::invalid_input(OP, "no cross sections"));
        }

        // We'll need a local non-const copy of this:
        let mut int_params: Option<Vec<f64>> = intersections_with_rails.map(<[f64]>::to_vec);

        // When given two rails - synchronize the knot vectors
        if let (Some(r1), Some(r2)) = (rail1.as_deref_mut(), rail2.as_deref_mut()) {
            // Note, this will reparameterize the curves, which will render the
            // intersection parameters invalid. We'll have to correct that.
            let domain1_before = r1.natural_interval();
            let domain2_before = r2.natural_interval();

            let mut rails = [core::mem::take(r1), core::mem::take(r2)];
            let sync = BSplineCurve::synchronize_knots_of_curves(&mut rails, EFF_ZERO * 100.0);
            let [a, b] = rails;
            *r1 = a;
            *r2 = b;
            sync?;

            // See about adjusting the intersection parameters.
            let domain1_after = r1.natural_interval();
            let domain2_after = r2.natural_interval();

            if let Some(params) = int_params.as_mut() {
                let tol = 0.001
                    * crate::curve::curve::p2::approximate_length(
                        k,
                        &*r1,
                        &domain1_after,
                        6,
                        false,
                    )?
                    .0;

                // Check rail 1.
                if (domain1_after.min() - domain1_before.min()).abs() > EFF_ZERO
                    || (domain1_after.max() - domain1_before.max()).abs() > EFF_ZERO
                {
                    for (ii, sect_crv) in cross_sections.iter().enumerate() {
                        // Map the parameter from the old domain to the new one.
                        let fraction = domain1_before
                            .inversion(at(params, ii, OP)?)
                            .parameter
                            .unwrap_or(UNDEF_DOUBLE);
                        let new_param = domain1_after.evaluate(fraction);

                        // reintersect, to be safe
                        let cross_domain = sect_crv.natural_interval();
                        let (found, t_rail) = local_curve_intersect(
                            k,
                            r1,
                            &domain1_after,
                            sect_crv,
                            &cross_domain,
                            tol,
                            new_param,
                            cross_domain.min(),
                        )?;
                        let slot = params.get_mut(ii).ok_or_else(|| {
                            SolidError::invalid_input(OP, "too few intersection parameters")
                        })?;
                        *slot = if found { t_rail } else { new_param };
                    }
                }

                // Check rail 2.
                if (domain2_after.min() - domain2_before.min()).abs() > EFF_ZERO
                    || (domain2_after.max() - domain2_before.max()).abs() > EFF_ZERO
                {
                    for (ii, sect_crv) in cross_sections.iter().enumerate() {
                        // Map the parameter from the old domain to the new one.
                        let fraction = domain2_before
                            .inversion(at(params, num_sections + ii, OP)?)
                            .parameter
                            .unwrap_or(UNDEF_DOUBLE);
                        let new_param = domain2_after.evaluate(fraction);

                        let cross_domain = sect_crv.natural_interval();
                        let (found, t_rail) = local_curve_intersect(
                            k,
                            r2,
                            &domain2_after,
                            sect_crv,
                            &cross_domain,
                            tol,
                            new_param,
                            cross_domain.max(),
                        )?;
                        let slot = params.get_mut(num_sections + ii).ok_or_else(|| {
                            SolidError::invalid_input(OP, "too few intersection parameters")
                        })?;
                        *slot = if found { t_rail } else { new_param };
                    }
                }
            }
        } // end synchronizing knot vectors, if given both rails

        // load rails into rails array and save knot vector
        let mut rails: [Option<Curve>; 2] = [None, None];
        let mut knots: Vec<f64> = Vec::new();
        if let Some(r2) = rail2.as_deref() {
            knots = unique_knots(r2);
            rails[1] = Some(create_nlib_curve(r2)?);
        }
        if let Some(r1) = rail1.as_deref() {
            knots = unique_knots(r1);
            rails[0] = Some(create_nlib_curve(r1)?);
        }

        // If there are only a couple of profile curves, the degree of the rails must be
        // high enough to fit the constraints, in the sweep direction.  [B542]
        if num_sections <= 3 {
            let mut num_deriv_surfs = 0;
            if let Some(ds) = &deriv_surfs {
                num_deriv_surfs = ds.iter().filter(|d| d.is_some()).count();
            }
            let num_constraints = num_sections + num_deriv_surfs;
            for rail in rails.iter_mut().flatten() {
                let deg = rail.degree();
                if deg + 1 < num_constraints {
                    let increment = num_constraints - 1 - deg;
                    // status ignored upstream
                    if let Ok(elevated) = rail.elevate_degree(increment) {
                        *rail = elevated;
                    }
                }
            }
        }

        // iter every crossSection curve building the cross sections array
        let mut curves: Vec<Curve> = cross_sections
            .iter()
            .map(create_nlib_curve)
            .collect::<Result<_>>()?;
        // (the maps: each section's knots as given to the skinning, to themselves)
        let mut maps: Option<Vec<Option<KnotMap>>> = want_maps.then(|| {
            curves
                .iter()
                .map(|c| Some(KnotMap::identity(c.knots())))
                .collect()
        });

        // load start and end skin surface Cross-boundary derivatives
        let mut dir_array: Option<[Option<Surface>; 2]> = None;
        if let Some(ds) = &deriv_surfs {
            let mut nl_derivs: [Option<Surface>; 2] = [None, None];
            for (slot, d) in nl_derivs.iter_mut().zip(ds) {
                if let Some(d) = d {
                    *slot = Some(create_nlib_surface(d)?);
                }
            }
            if ds.iter().any(Option::is_some) {
                dir_array = Some(nl_derivs);
            }
        }

        // NLib locals
        let num_cross_curves = num_sections;
        let nl_dir = if cross_section_surf_param == SurfParamType::U {
            Direction::U
        } else {
            Direction::V
        };
        let has_rails = rail1.is_some() || rail2.is_some();
        let mut nl_deg = degree;

        // If there are no additional constraints, then use the lowest degree that will
        // interpolate the given number of cross curves in a single span. [5/28/10]
        if dir_array.is_none() && degree >= num_cross_curves {
            nl_deg = num_cross_curves - 1;
        }

        // when pIntersectionsWithRails is given
        let mut nl_knots: Option<Vec<f64>> = None;
        if let Some(params) = &int_params {
            if !knots.is_empty() {
                // use the rail knot vector
                if let Some(r) = &rails[0] {
                    nl_knots = Some(r.knots().to_vec());
                } else if let Some(r) = &rails[1] {
                    nl_knots = Some(r.knots().to_vec());
                }
            } else if dir_array.is_some() {
                // use a regularly spaced knot vector (only used with cross-boundary
                // derivatives, below)
                let mut kv = vec![0.0; num_cross_curves + nl_deg + 1];
                make_knots(params, num_cross_curves - 1, nl_deg, &mut kv)?;
                nl_knots = Some(kv);
            }
        }

        // when cross-derivatives are not specified - don't specify a knot vector
        if dir_array.is_none() {
            nl_knots = None;
        }

        let nl_surf = if rail1_used_as_spine_curve {
            // sub = Subdivision level
            let sub = if num_cross_curves < 9 { 5 } else { 2 };

            // evaluate rail curve start and use the tangent to build a orthonormal
            // vector set
            let (Some(r1), Some(spine), Some(params)) =
                (rail1.as_deref(), rails[0].as_ref(), int_params.as_deref())
            else {
                return Err(SolidError::b_spline_with_status(
                    OP,
                    BSplineFailure::Defect {
                        why: "the spine path lost the spine rail or the rail intersections, \
                              which the input check requires with a spine and nothing since \
                              removes",
                    },
                    SM_ERR_NULL_POINTER,
                ));
            };
            let pv = r1.evaluate(r1.natural_interval().min(), 1, true, false)?;
            let tangent = pv.get(1).copied().ok_or_else(|| {
                SolidError::b_spline(
                    OP,
                    BSplineFailure::Defect {
                        why: "the spine rail's evaluation with one derivative returned no \
                              derivative, and it returns one row per order asked",
                    },
                )
            })?;
            let [_, _, z_axis] = tangent.make_unit_ortho_vectors(None)?;
            // (the spine path makes the sections compatible its own way: not traced)
            if let Some(m) = maps.as_mut() {
                for x in m.iter_mut() {
                    *x = None;
                }
            }
            create_skin_spine(
                &mut curves,
                spine,
                params,
                &SkinSpineOptions {
                    make_compatible: !synchronized,
                    align: Some(SpineAlignment {
                        z0: z_axis,
                        subdiv: sub,
                    }),
                    derivs: if nl_deg < 2 {
                        SkinDerivs::None
                    } else {
                        SkinDerivs::Ends
                    },
                    degree: nl_deg,
                    dir: nl_dir,
                },
            )
            .map_err(SolidError::nurbs_routine(OP))?
        } else {
            // skin crossSections with optional rail curves and start/end
            // cross-derivative constraints
            let has_dir_array = dir_array.is_some();
            let mut sk = SkinBoundaryOptions {
                der: dir_array,
                cmp: synchronized,
                cnt: [BoundaryContinuity::G1, BoundaryContinuity::G1],
                bnd: [Boundary::Bottom, Boundary::Bottom],
                rail: if has_rails { Some(rails) } else { None },
                deg: nl_deg,
                es: approx_tol,
                dir: nl_dir,
                // Not upstream: without rails, the caller's section parameters (if any).
                ts: if has_rails {
                    int_params
                } else {
                    int_params.or_else(|| section_params.map(<[f64]>::to_vec))
                },
                kns: nl_knots,
            };
            match create_skin_srf_boundary_continuity_mapped(
                &mut curves,
                &mut sk,
                maps.as_deref_mut(),
            ) {
                Ok(s) => s,
                Err(
                    e @ (SolidError::Unported { .. }
                    | SolidError::Cancelled { .. }
                    | SolidError::OutOfFuel { .. }),
                ) => return Err(e),
                Err(_) => {
                    // If not successful try without knots.
                    sk.kns = None;
                    match create_skin_srf_boundary_continuity_mapped(
                        &mut curves,
                        &mut sk,
                        maps.as_deref_mut(),
                    ) {
                        Ok(s) => s,
                        Err(
                            e @ (SolidError::Unported { .. }
                            | SolidError::Cancelled { .. }
                            | SolidError::OutOfFuel { .. }),
                        ) => return Err(e),
                        Err(e) => {
                            // It failed with derivative fields try it without them
                            if has_dir_array {
                                sk.der = None;
                                create_skin_srf_boundary_continuity_mapped(
                                    &mut curves,
                                    &mut sk,
                                    maps.as_deref_mut(),
                                )
                                .map_err(SolidError::nurbs_routine(OP))?
                            } else {
                                // its not working - signal an error, with the fit's
                                return Err(SolidError::nurbs_routine(OP)(e));
                            }
                        }
                    }
                }
            }
        };

        // set output
        Ok((Self::from_gw_nurb(&nl_surf), maps))
    }

    // ---- TrimWithDomain -------------------------------------------------------------

    /// Trims the surface to `trim_domain` (`TrimWithDomain`): the domain is snapped
    /// to knots (or moved away from them) so that no span shorter than about
    /// `10 * SM_EFF_ZERO_SQRT` is created, never shrunk into the requested domain,
    /// and the patch over it is extracted (`N_SrfExtractPatch`) and becomes the NURB.
    /// `trim_domain` receives the snapped domain.
    ///
    /// # Errors
    ///
    /// `SM_ERR` when `trim_domain` is not inside the natural domain or the extraction
    /// fails; `InvalidInput` when the snapped u boundaries are too close;
    /// `SM_ERR_NULL_POINTER` without NURB (once a trim is needed).
    pub fn trim_with_domain(&mut self, trim_domain: &mut Extent2d) -> Result<()> {
        self.trim_with_domain_with(trim_domain, TrimOptions::default())
    }

    /// [`BSplineSurface::trim_with_domain`] with [`TrimOptions`]; the default options
    /// are upstream's `TrimWithDomain`.
    ///
    /// # Errors
    ///
    /// Those of [`BSplineSurface::trim_with_domain`].
    pub fn trim_with_domain_with(
        &mut self,
        trim_domain: &mut Extent2d,
        options: TrimOptions,
    ) -> Result<()> {
        const OP: &str = OP_TRIM;
        let snap = options.snap_roundoff_to_knots;
        // get current natural boundary
        let natural_domain = self.natural_uv_domain();

        // check input - trim domain not contained within natural domain
        if !trim_domain.is_contained_by(&natural_domain, EFF_ZERO_PARAM) {
            return Err(SolidError::b_spline(OP, BSplineFailure::TrimDomainOutside));
        }

        // no work - natural domain is same size as trim domain
        if natural_domain.is_contained_by(trim_domain, EFF_ZERO_PARAM) {
            return Ok(());
        }

        // get trim domain corner points
        let mut ul = trim_domain.min().x;
        let mut ur = trim_domain.max().x;
        let mut vl = trim_domain.min().y;
        let mut vr = trim_domain.max().y;

        // Not upstream (#472): the requested domain as sm_CheckTrimBoundaries sees it,
        // with any boundary snapped onto a knot by `snap_roundoff_to_knots`.
        let (mut req_ul, mut req_ur, mut req_vl, mut req_vr) = (ul, ur, vl, vr);

        // don't build too small spans in u direction.
        {
            // scale tolerances to domain size
            let tol_min = EFF_ZERO_SQRT * (1.0 + ul.abs());
            let tol_max = EFF_ZERO_SQRT * (1.0 + ur.abs());

            let mut idx_min: Option<usize> = None;
            let mut idx_max: Option<usize> = None;

            // get current U knots
            let knots: Vec<f64> = self
                .knots(SurfParamType::U, None)
                .map(|k| k.0)
                .unwrap_or_default();
            let last = knots.len().saturating_sub(1);

            // for every U knot - looking to snap trim boundaries
            for (i, &knot) in knots.iter().enumerate() {
                // when lower trim boundary is close to knot
                if (knot - ul).abs() < tol_min {
                    // when trim boundary is slightly larger than knot - set trim
                    // boundary to knot (or, not upstream, a rounding error smaller)
                    if ul >= knot || i == 0 || (snap && knot - ul <= EFF_ZERO_PARAM) {
                        if snap && ul < knot {
                            req_ul = knot;
                        }
                        ul = knot;
                        idx_min = Some(i);
                    } else {
                        // Set UL to either the previous knot or slightly less than
                        // current knot
                        ul = knot - 10.0 * tol_min;
                        idx_min = Some(i);
                        let prev = at(&knots, i - 1, OP)?;
                        if ul < prev {
                            ul = prev;
                            idx_min = Some(i - 1);
                        }
                    }
                }

                // when upper trim boundary is close to knot
                if (knot - ur).abs() < tol_max {
                    if ur <= knot || i == last || (snap && ur - knot <= EFF_ZERO_PARAM) {
                        if snap && ur > knot {
                            req_ur = knot;
                        }
                        ur = knot;
                        idx_max = Some(i);
                    } else {
                        ur = knot + 10.0 * tol_max;
                        let next = at(&knots, i + 1, OP)?;
                        if ur > next {
                            ur = next;
                            idx_max = Some(i + 1);
                        }
                    }
                }
            }

            // Make sure we're not shrinking the input domain. [B306]
            (ul, ur) = super::p1::check_trim_boundaries(
                &knots,
                ul,
                idx_min,
                ur,
                idx_max,
                &Extent2d::from_bounds(req_ul, req_vl, req_ur, req_vr),
                SurfParamType::U,
            )?;

            // no work - snapped trim boundaries are within tolerance of one another
            if (ur - ul).abs() < tol_min + tol_max {
                // Error here because trim points are too close together
                return Err(SolidError::invalid_input(
                    OP,
                    "trim boundaries too close together",
                ));
            }
        }

        // don't build too small spans in V direction.
        {
            let tol_min = EFF_ZERO_SQRT * (1.0 + vl.abs());
            let tol_max = EFF_ZERO_SQRT * (1.0 + vr.abs());

            let mut idx_min: Option<usize> = None;
            let mut idx_max: Option<usize> = None;

            let knots: Vec<f64> = self
                .knots(SurfParamType::V, None)
                .map(|k| k.0)
                .unwrap_or_default();
            let last = knots.len().saturating_sub(1);
            for (i, &knot) in knots.iter().enumerate() {
                // snap lower V Trim boundaries as appropriate
                if (knot - vl).abs() < tol_min {
                    if vl >= knot || i == 0 || (snap && knot - vl <= EFF_ZERO_PARAM) {
                        if snap && vl < knot {
                            req_vl = knot;
                        }
                        vl = knot;
                        idx_min = Some(i);
                    } else {
                        vl = knot - 10.0 * tol_min;
                        idx_min = Some(i);
                        let prev = at(&knots, i - 1, OP)?;
                        if vl < prev {
                            vl = prev;
                            idx_min = Some(i - 1);
                        }
                    }
                }

                // snap upper V Trim Boundaries as appropriate
                if (knot - vr).abs() < tol_max {
                    if vr <= knot || i == last || (snap && vr - knot <= EFF_ZERO_PARAM) {
                        if snap && vr > knot {
                            req_vr = knot;
                        }
                        vr = knot;
                        idx_max = Some(i);
                    } else {
                        vr = knot + 10.0 * tol_max;
                        idx_max = Some(i);
                        let next = at(&knots, i + 1, OP)?;
                        if vr > next {
                            vr = next;
                            idx_max = Some(i + 1);
                        }
                    }
                }
            }

            // Make sure we're not shrinking the input domain. [B306]
            (vl, vr) = super::p1::check_trim_boundaries(
                &knots,
                vl,
                idx_min,
                vr,
                idx_max,
                &Extent2d::from_bounds(req_ul, req_vl, req_ur, req_vr),
                SurfParamType::V,
            )?;
        }

        // place snapped Trim Boundary values into input TrimDomain (status ignored)
        let _ = trim_domain.set_min_max(Point2::new(ul, vl), Point2::new(ur, vr));

        // local surface pointer
        let sur_p = self.nurb.as_ref().ok_or_else(|| {
            SolidError::b_spline_with_status(OP, BSplineFailure::NoNurbsForm, SM_ERR_NULL_POINTER)
        })?;

        // extract patch defined by trim boundaries from surP into sur1
        let sur1 = extract_patch(sur_p, Rectangle::new(ul, ur, vl, vr))
            .map_err(SolidError::nurbs_routine(OP))?;

        // build SMLib surface from NLib sur1 and swap the NURBs
        self.nurb = Self::from_gw_nurb(&sur1).nurb;
        Ok(())
    }

    // ---- Evaluate -------------------------------------------------------------------

    /// The point and partial derivatives at `uv` (`Evaluate`).
    ///
    /// Returns `D[i][j] = d^(i+j) S / du^i dv^j` for `i <= highest_u_deriv`,
    /// `j <= highest_v_deriv` ([`BSplineSurface::evaluate_simple`] semantics: `uv` is
    /// clamped to the natural domain unless out-of-bounds evaluation is enabled or the
    /// surface is in power-basis form). With `flags.non_zero_tangents` a first
    /// derivative shorter than `10 * SM_EFF_ZERO * (1 + max|P|)` is replaced: from the
    /// second derivatives (`Suv`, else `Suu` / `Svv`), or else by the direction of a
    /// good first derivative at a nearby point (stepping `1e-5 .. 0.1` towards the
    /// middle of the domain and diagonally), scaled to `1.1` times that limit.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for `only_upper_half` with different orders or an order above
    /// `NL_MAXDER` (4); `SM_ERR` without NURB, when NLib fails, or when no
    /// replacement for a zero first derivative is found.
    pub fn evaluate(
        &self,
        uv: Point2,
        highest_u_deriv: usize,
        highest_v_deriv: usize,
        flags: EvaluateFlags,
    ) -> Result<Vec<Vec<Vec3>>> {
        self.evaluate_impl(uv, highest_u_deriv, highest_v_deriv, flags, true, None)
    }

    /// Preserve the output written before zero-tangent correction reports failure.
    /// `SmSurfaceEval` accessors deliberately read that output after a failed eval.
    pub(crate) fn evaluate_with_partial_output(
        &self,
        uv: Point2,
        flags: EvaluateFlags,
    ) -> (Result<()>, Vec<Vec<Vec3>>) {
        let mut partial = Vec::new();
        match self.evaluate_impl(uv, 2, 2, flags, true, Some(&mut partial)) {
            Ok(values) => (Ok(()), values),
            Err(error) => (Err(error), partial),
        }
    }

    /// `Evaluate` for a caller that ignores its status (as `SmSurface::IsPeriodic`
    /// does): the derivative array as upstream leaves it in `aDerivatives`, also when
    /// the zero-tangent repair fails after the NLib evaluation wrote it (the raw or
    /// partly repaired derivatives); `None` when the call fails before writing it (bad
    /// orders, no NURB), leaving the caller's array as it was.
    ///
    /// # Errors
    ///
    /// `SM_ERR` when NLib fails: upstream's array then holds whatever NLib wrote
    /// before failing, which the port does not reproduce.
    pub fn evaluate_ignoring_status(
        &self,
        uv: Point2,
        highest_u_deriv: usize,
        highest_v_deriv: usize,
        flags: EvaluateFlags,
    ) -> Result<Option<Vec<Vec<Vec3>>>> {
        // the checks `Evaluate` fails on before anything is written
        if (flags.only_upper_half && highest_u_deriv != highest_v_deriv)
            || highest_u_deriv > MAX_DERIVATIVE
            || highest_v_deriv > MAX_DERIVATIVE
            || self.nurb.is_none()
        {
            return Ok(None);
        }
        let mut written = Vec::new();
        match self.evaluate_impl(
            uv,
            highest_u_deriv,
            highest_v_deriv,
            flags,
            true,
            Some(&mut written),
        ) {
            Ok(d) => Ok(Some(d)),
            Err(_) if !written.is_empty() => Ok(Some(written)),
            Err(e) => Err(e),
        }
    }

    /// `Evaluate` with its internal `bDoZeroSampling` argument.
    fn evaluate_impl(
        &self,
        cr_uv: Point2,
        highest_u_deriv: usize,
        highest_v_deriv: usize,
        flags: EvaluateFlags,
        do_zero_sampling: bool,
        failed_output: Option<&mut Vec<Vec<Vec3>>>,
    ) -> Result<Vec<Vec<Vec3>>> {
        const OP: &str = OP_EVAL;
        let EvaluateFlags {
            u_side,
            v_side,
            only_upper_half,
            non_zero_tangents,
        } = flags;

        // check input - bOnlyUpperHalf can be TRUE only if deriv requests are equal.
        if only_upper_half && highest_u_deriv != highest_v_deriv {
            return Err(SolidError::invalid_input(
                OP,
                "only upper half with different orders",
            ));
        }
        if highest_u_deriv > MAX_DERIVATIVE || highest_v_deriv > MAX_DERIVATIVE {
            return Err(SolidError::invalid_input(
                OP,
                "derivative order above NL_MAXDER",
            ));
        }

        // When appropriate, clamp input parameters to surface domain
        let mut s_uv = cr_uv;
        let natural_uv_domain = self.natural_uv_domain();
        if self.form != BSplineSurfaceForm::Polynomial
            && !natural_uv_domain.contains_point(s_uv, 0.0)
            && !self.is_out_of_bounds_enabled()
        {
            s_uv = natural_uv_domain.clamp_point(s_uv);
        }

        // Setup for NLib call
        let (uflg, vflg) = (u_side, v_side);

        let sur = self
            .nurb
            .as_ref()
            .ok_or_else(|| SolidError::b_spline(OP, BSplineFailure::NoNurbsForm))?;
        let mut d = if self.form == BSplineSurfaceForm::Polynomial {
            srf_power_basis_eval_derivs(
                &sur.cpts(),
                sur.p(),
                sur.q(),
                s_uv.x,
                s_uv.y,
                only_upper_half,
                highest_u_deriv,
                highest_v_deriv,
            )
        } else {
            sur.derivs(
                s_uv.x,
                s_uv.y,
                uflg,
                vflg,
                only_upper_half,
                highest_u_deriv,
                highest_v_deriv,
            )
        }
        .map_err(SolidError::nurbs_routine(OP))?;

        // The zero-tangent repair below works on the matrix NLib wrote; upstream's
        // caller array keeps it (raw or partly repaired) whatever the repair's status,
        // including a failed recursive re-evaluation.
        let status = (|| -> Result<()> {
            let d = &mut d;
            // If we don't have to check for zero first derivatives, we're done.
            if !non_zero_tangents {
                return Ok(());
            }
            if highest_u_deriv < 1 && highest_v_deriv < 1 {
                return Ok(());
            }

            // flat index `k` of the upstream array: D[k / (nv + 1)][k % (nv + 1)]
            let nv1 = highest_v_deriv + 1;
            let at = |d: &[Vec<Vec3>], k: usize| -> Result<Vec3> {
                d.get(k / nv1)
                    .and_then(|row| row.get(k % nv1))
                    .copied()
                    .ok_or_else(|| {
                        SolidError::b_spline(
                            OP,
                            BSplineFailure::Defect {
                                why: SHORT_SURFACE_EVALUATION,
                            },
                        )
                    })
            };
            let set = |d: &mut [Vec<Vec3>], k: usize, v: Vec3| -> Result<()> {
                let slot = d
                    .get_mut(k / nv1)
                    .and_then(|row| row.get_mut(k % nv1))
                    .ok_or_else(|| {
                        SolidError::b_spline(
                            OP,
                            BSplineFailure::Defect {
                                why: SHORT_SURFACE_EVALUATION,
                            },
                        )
                    })?;
                *slot = v;
                Ok(())
            };

            // position and tangents
            let orig_pnt = at(d, 0)?;
            let orig_du = if highest_u_deriv >= 1 {
                at(d, nv1)?
            } else {
                at(d, 1)?
            };
            let orig_dv = if highest_v_deriv >= 1 {
                at(d, 1)?
            } else {
                at(d, nv1)?
            };

            // tangent sizes
            let orig_du_len = orig_du.length();
            let orig_dv_len = orig_dv.length();

            let deriv_lim = crate::surface::surface::p2::derivative_limit(orig_pnt);

            // test
            let zero_du = orig_du_len < deriv_lim;
            let zero_dv = orig_dv_len < deriv_lim;

            // no work - no zero tangents to fix
            if !zero_du && !zero_dv {
                return Ok(());
            }

            // when not enough derivatives were asked for - recurse to this function
            if highest_u_deriv < 2 || highest_v_deriv < 2 {
                let re = self.evaluate_impl(
                    cr_uv,
                    2,
                    2,
                    EvaluateFlags {
                        u_side,
                        v_side,
                        only_upper_half: true,
                        non_zero_tangents: true,
                    },
                    true,
                    None,
                )?;
                let re_at = |i: usize, j: usize| -> Result<Vec3> {
                    re.get(i)
                        .and_then(|row| row.get(j))
                        .copied()
                        .ok_or_else(|| {
                            SolidError::b_spline(
                                OP,
                                BSplineFailure::Defect {
                                    why: SHORT_SURFACE_EVALUATION,
                                },
                            )
                        })
                };

                // save the kluged tangent values (aReDer[3] = Su, aReDer[1] = Sv)
                if highest_u_deriv >= 1 {
                    set(d, nv1, re_at(1, 0)?)?;
                }
                if highest_v_deriv >= 1 {
                    set(d, 1, re_at(0, 1)?)?;
                }
                return Ok(());
            }

            // Ok, now we have at least two derivatives.
            let lnv = highest_v_deriv;
            let mut su = at(d, lnv + 1)?;
            let mut sv = at(d, 1)?;
            let ok = super::p1::check_zero_surface_derivs(
                s_uv,
                &natural_uv_domain,
                &mut su,
                &mut sv,
                at(d, 2 * lnv + 2)?,
                at(d, lnv + 2)?,
                at(d, 2)?,
                deriv_lim,
            );
            set(d, lnv + 1, su)?;
            set(d, 1, sv)?;

            // all done?
            if ok {
                return Ok(());
            }

            // Try finding a nonZeroTangent value with a more brute-force method: step
            // towards the center of the domain.
            let step_dir_u = if s_uv.x > natural_uv_domain.u_interval().mid() {
                -1.0
            } else {
                1.0
            };
            let step_dir_v = if s_uv.y > natural_uv_domain.v_interval().mid() {
                -1.0
            } else {
                1.0
            };

            let mut step = ZONE_TOL_3D / 10.0; // works well in practice.

            // for larger and larger step off distances
            while do_zero_sampling && step < 0.1 {
                // increase the step off distance
                step *= 10.0;

                // for this step size: try 5 directions; last 4 are diagonals.
                // Make the first step go in the most likely direction: direction of good
                // deriv, towards center of surface.
                let first = if orig_du_len < orig_dv_len {
                    // step in v
                    Point2::new(s_uv.x, s_uv.y + step_dir_v * step)
                } else {
                    // step in u
                    Point2::new(s_uv.x + step_dir_u * step, s_uv.y)
                };
                let test_uv = [
                    first,
                    Point2::new(s_uv.x + step, s_uv.y + step),
                    Point2::new(s_uv.x - step, s_uv.y - step),
                    Point2::new(s_uv.x - step, s_uv.y + step),
                    Point2::new(s_uv.x + step, s_uv.y - step),
                ];

                // for all 5 sample directions at this step size
                for t_uv in test_uv {
                    let t_uv = natural_uv_domain.clamp_point(t_uv);
                    // just take what's naturally there; one derivative is enough.
                    let Ok(re) = self.evaluate_impl(
                        t_uv,
                        1,
                        1,
                        EvaluateFlags {
                            u_side,
                            v_side,
                            only_upper_half: true,
                            non_zero_tangents: false,
                        },
                        false,
                        None,
                    ) else {
                        continue;
                    };
                    // aReDer[2] = Su, aReDer[1] = Sv
                    let mut re_su =
                        re.get(1).and_then(|r| r.first()).copied().ok_or_else(|| {
                            SolidError::b_spline(
                                OP,
                                BSplineFailure::Defect {
                                    why: SHORT_SURFACE_EVALUATION,
                                },
                            )
                        })?;
                    let mut re_sv =
                        re.first().and_then(|r| r.get(1)).copied().ok_or_else(|| {
                            SolidError::b_spline(
                                OP,
                                BSplineFailure::Defect {
                                    why: SHORT_SURFACE_EVALUATION,
                                },
                            )
                        })?;

                    // Save the nearby tangent values, if they're good enough.
                    if re_su.length() >= deriv_lim && re_sv.length() >= deriv_lim {
                        // Set magnitudes of degenerate derivatives to 1.1 * dDerivLim.
                        if zero_du {
                            re_su = re_su.unitize_or_z();
                            re_su *= 1.1 * deriv_lim;
                            set(d, highest_v_deriv + 1, re_su)?;
                        }
                        if zero_dv {
                            re_sv = re_sv.unitize_or_z();
                            re_sv *= 1.1 * deriv_lim;
                            set(d, 1, re_sv)?;
                        }

                        // all done
                        return Ok(());
                    }
                }
            }

            // If we make it to here we are unable to get both of the first derivatives to
            // a non-zero size.
            Err(SolidError::b_spline(OP, BSplineFailure::TangentsVanish))
        })();
        match status {
            Ok(()) => Ok(d),
            Err(e) => {
                // C++ has already written the matrix to the caller's array.
                if let Some(output) = failed_output {
                    *output = d;
                }
                Err(e)
            }
        }
    }
}

// ---- CreateCornerBlend: four curves -----------------------------------------------------

const LEFT: usize = 0;
const BOT: usize = 1;
const RIGHT: usize = 2;
const TOP: usize = 3;

/// The four-sided branch of `CreateCornerBlend`; `face_sides` is the fork's
/// `lFaceSides` (WR-613), bit `i` for curve `i`.
fn corner_blend_4(
    k: &crate::kernel::Kernel,
    mut boundaries: Vec<BSplineCurve>,
    originals: &[SmCurve],
    mut uv_crvs: Vec<Option<BSplineCurve>>,
    curve_orients: &[OrientType],
    surfaces: &[Option<&SmSurface>],
    approx_tol: f64,
    tangency_tol_radians: f64,
    create_bilinear_patch: bool,
    face_sides: u32,
) -> Result<Vec<BSplineSurface>> {
    const OP: &str = OP_CORNER;
    let oob = || SolidError::invalid_input(OP, "curve index out of range");

    // for every boundary - set orientations
    for ii in 0..4 {
        // make bot & right orientation = Same, top & left orientation = Opposite
        let orient = at(curve_orients, ii, OP)?;
        if ((ii == TOP || ii == LEFT) && orient == OrientType::Same)
            || ((ii == BOT || ii == RIGHT) && orient != OrientType::Same)
        {
            let b = boundaries.get_mut(ii).ok_or_else(oob)?;
            let ivl = b.natural_interval();
            b.reverse_parameterization(&ivl)?;
            if let Some(uv) = uv_crvs.get_mut(ii).and_then(Option::as_mut) {
                let ivl = uv.natural_interval();
                uv.reverse_parameterization(&ivl)?;
            }
        }
    }

    // make left/right curve pair compatible (union knot vectors), normalizing the
    // domains first (MakeCurvesCompatible normalizes them anyway)
    let unit_ivl = Extent1d::from_min_max(0.0, 1.0);
    // There's a hard-coded knot tolerance everywhere of 1e-8 ... use that, increased a
    // bit for safety.   [T1000C, bd, 21 Dec 05]
    let knot_tol = 2.0e-8;
    with_pair(&mut boundaries, LEFT, RIGHT, |pair| {
        for c in pair.iter_mut() {
            let _ = c.edit_parameterization(&unit_ivl);
        }
        BSplineCurve::make_curves_compatible(pair, knot_tol)
    })??;

    // make top/bot curve pair compatible (union knot vectors)
    with_pair(&mut boundaries, BOT, TOP, |pair| {
        BSplineCurve::make_curves_compatible(pair, knot_tol)
    })??;

    let mut tans: Vec<BSplineCurve> = Vec::new();

    // when working with crossTangents (making g1 patches)
    if !create_bilinear_patch {
        // make sure all UVCurves have same parameter range as 3DCurve
        for (b, uv) in boundaries.iter().zip(uv_crvs.iter_mut()) {
            if let Some(uv) = uv {
                uv.edit_parameterization(&b.natural_interval())?;
            }
        }

        // for every boundary curve - Create Tangent Fields
        let mut tan_curves: Vec<BSplineCurve> = Vec::with_capacity(4);
        for ii in 0..4 {
            let (start_idx, end_idx) = if ii % 2 == 1 {
                (LEFT, RIGHT)
            } else {
                (BOT, TOP)
            };
            let start_curve = boundaries.get(start_idx).ok_or_else(oob)?;
            let end_curve = boundaries.get(end_idx).ok_or_else(oob)?;
            let normalized = if ii >= 2 { 1.0 } else { 0.0 };
            let start_param = start_curve.natural_interval().evaluate(normalized);
            let end_param = end_curve.natural_interval().evaluate(normalized);

            let boundary_curve = boundaries.get(ii).ok_or_else(oob)?;

            let tan_curve = if let Some(srf) = at(surfaces, ii, OP)? {
                // build temporary crossTangent curve function and an approximate
                // piecewise hermite curve to it
                let uv = uv_crvs
                    .get(ii)
                    .and_then(Option::as_ref)
                    .ok_or_else(|| SolidError::invalid_input(OP, "missing UV curve"))?;
                tangent_field_curve(
                    k,
                    2.0 * approx_tol,
                    boundary_curve,
                    uv,
                    srf,
                    (start_curve, start_param),
                    (end_curve, end_param),
                    tangency_tol_radians,
                )?
            } else {
                // a degenerate curve which does not 'lie' on a surface
                if !boundary_curve.is_degenerate(k, EFF_ZERO, None)? {
                    return Err(SolidError::b_spline(
                        OP,
                        BSplineFailure::FreeBoundaryNotDegenerate,
                    ));
                }

                // get neighbor curve endPoint tangents (SmCurve::Evaluate default:
                // non-zero tangents)
                let pv1 = start_curve.evaluate(start_param, 1, true, true)?;
                let pv2 = end_curve.evaluate(end_param, 1, true, true)?;
                let t1 = at(&pv1, 1, OP)?;
                let t2 = at(&pv2, 1, OP)?;

                // Create degree 3 crossTangent Vector linear interpolation of
                // crossTangent values
                let cntrl_poly = [t1, t1, t2, t2];
                let mut tan = BSplineCurve::create_canonical(&CanonicalData {
                    dim: 3,
                    degree: 3,
                    control_points: &cntrl_poly,
                    form: BSplineCurveForm::Unspecified,
                    knot_multiplicities: &[4, 4],
                    knots: &[0.0, 1.0],
                    knot_type: KnotType::Unspecified,
                    weights: None,
                    trim: None,
                })?;
                let trim_ivl = boundary_curve.natural_interval();
                tan.edit_parameterization(&trim_ivl)?;
                tan
            };

            // add crossTangent function to ordered array
            tan_curves.push(tan_curve);
        }

        // create [left, right, bot, top] ordered crossTangent curve array
        let mut tc: Vec<Option<BSplineCurve>> = tan_curves.into_iter().map(Some).collect();
        for idx in [LEFT, RIGHT, BOT, TOP] {
            let c = tc.get_mut(idx).and_then(Option::take).ok_or_else(oob)?;
            tans.push(c);
        }
    }

    // let tol = SM_ZONE_TOL_3D * boundaryCurve->BoundingBox maxDimension
    let mut bbox = Extent3d::new();
    for b in &boundaries {
        let ivl = b.natural_interval();
        let boxes = b.calculate_bounding_box(
            &ivl,
            BoundingBoxRequest {
                normal: true,
                pseudo: false,
                polar: false,
            },
        )?;
        let cbox = boxes.normal.unwrap_or_default();
        bbox = bbox.union(&cbox);
    }
    let tol = ZONE_TOL_3D * bbox.size().max_dimension();

    // create [left, right, bot, top] ordered boundary curve array
    let mut bc: Vec<Option<BSplineCurve>> = boundaries.into_iter().map(Some).collect();
    let mut bounds: Vec<BSplineCurve> = Vec::with_capacity(4);
    for idx in [LEFT, RIGHT, BOT, TOP] {
        let c = bc.get_mut(idx).and_then(Option::take).ok_or_else(oob)?;
        bounds.push(c);
    }

    // build coons patch and save result
    // Not upstream (WR-613; tangency-labs/smlib@7fd9fb0): the face sides in bounds'
    // order (left, right, bottom, top)
    let mut coons_face_sides = 0;
    if face_sides & (1 << LEFT) != 0 {
        coons_face_sides |= COONS_FACE_LEFT;
    }
    if face_sides & (1 << RIGHT) != 0 {
        coons_face_sides |= COONS_FACE_RIGHT;
    }
    if face_sides & (1 << BOT) != 0 {
        coons_face_sides |= COONS_FACE_BOTTOM;
    }
    if face_sides & (1 << TOP) != 0 {
        coons_face_sides |= COONS_FACE_TOP;
    }
    // the face-sided patch's sides, before the mean-twist patch consumes them. Only where
    // every side has a surface: the guard proves each side's seam with that side's
    // surface, and a side without one (a triangular patch's) would be left unheld
    let surfaced = (0..4).all(|ii| matches!(surfaces.get(ii), Some(Some(_))));
    let face_sided = (coons_face_sides != 0 && !create_bilinear_patch && surfaced)
        .then(|| (bounds.clone(), tans.clone()));
    let coons = BSplineSurface::create_coons_patch_on_faces(
        tol,
        create_bilinear_patch,
        &mut bounds,
        &mut tans,
        0,
    )?;
    if create_bilinear_patch {
        return Ok(vec![coons]);
    }

    // Not upstream (WR-547; tangency-labs/smlib@cae8618): the patch tangent to its sides'
    // surfaces
    let tangent =
        make_coons_patch_tangent_to_sides(k, &coons, &uv_crvs, surfaces, tangency_tol_radians)?;

    // Not upstream (WR-613): a patch that meets its face sides exactly, where every side
    // is proved within the largest angle the patch that ships otherwise is proved to
    // reach, so the largest angle over the sides is no larger
    let bar = tangent.unwrap_or(coons);
    let Some(face_sided) = face_sided else {
        return Ok(vec![bar]);
    };
    let sides = CoonsSides {
        curves: originals,
        uv_curves: &uv_crvs,
        surfaces,
        face_sides,
    };
    // the seam check's tangent angle (`make_coons_patch_tangent_to_sides`' target)
    let target = (ANG_TOL_DEG * (PI / 180.0)).min(tangency_tol_radians);
    let patch = face_sided_or_bar(k, tol, face_sided, coons_face_sides, &sides, bar, target)?;
    Ok(vec![patch])
}

/// Not the fork's (WR-613): the face-sided corner patch ([`face_sided_patch`]) built from
/// `face_sided` (the sides and cross tangents before the mean-twist patch consumed them,
/// with `coons_face_sides` in [`BSplineSurface::create_coons_patch_on_faces`]' bits) where
/// a candidate is taken, else `bar` (the patch that ships otherwise) bit for bit.
///
/// The attempt is an addition to the shipped patch, so it never fails a corner the shipped
/// patch builds, save for work given up ([`keep_bar_on_error`]): an error anywhere in it (building the face-sided patch, refining or
/// grading a candidate, projecting its rows, reading or proving a seam) keeps `bar`, as
/// no candidate taken would ([`keep_bar_on_error`]).
///
/// # Errors
///
/// The statuses [`SolidError::is_unported_or_cancelled`] passes on (an unported call, a
/// cancellation, face trims unavailable, a spent certification budget) from the attempt.
fn face_sided_or_bar(
    k: &crate::kernel::Kernel,
    tol: f64,
    (mut face_bounds, mut face_tans): (Vec<BSplineCurve>, Vec<BSplineCurve>),
    coons_face_sides: u32,
    sides: &CoonsSides<'_, '_>,
    bar: BSplineSurface,
    target: f64,
) -> Result<BSplineSurface> {
    let attempt = (|| -> Result<Option<BSplineSurface>> {
        let twisted = BSplineSurface::create_coons_patch_on_faces(
            tol,
            false,
            &mut face_bounds,
            &mut face_tans,
            coons_face_sides,
        )?;
        face_sided_patch(k, &twisted, &bar, sides, target)
    })();
    Ok(keep_bar_on_error(attempt)?.unwrap_or(bar))
}

/// Not the fork's (WR-613): the face-sided attempt's outcome ([`face_sided_or_bar`]), with
/// every error other than those [`SolidError::is_unported_or_cancelled`] passes on (an
/// unported call, a cancellation, face trims unavailable, a spent certification budget)
/// read as no candidate taken, so the caller keeps the patch that ships otherwise. Those
/// are passed on: work given up never becomes a result.
fn keep_bar_on_error(attempt: Result<Option<BSplineSurface>>) -> Result<Option<BSplineSurface>> {
    match attempt {
        Err(e) if e.is_unported_or_cancelled() => Err(e),
        Err(_) => Ok(None),
        Ok(taken) => Ok(taken),
    }
}

// ---- the fork's Coons patch tangency (tangency-labs/smlib@cae8618) ----------------------

/// The fork's `sm_CoonsSideNormal`: the unit normal of `surface` along a Coons patch's
/// side, at patch parameter `s` along that side (`patch_ivl`: the side's interval in
/// the patch's parameters), found through the side's UV curve.
fn coons_side_normal(
    k: &crate::kernel::Kernel,
    uv_curve: &BSplineCurve,
    surface: &SmSurface,
    patch_ivl: &Extent1d,
    s: f64,
) -> Result<Vec3> {
    let ivl = uv_curve.natural_interval();
    let t = ivl.evaluate((s - patch_ivl.min()) / patch_ivl.length());
    let uv = uv_curve.evaluate_point(t)?;
    evaluate_normal(k, surface, Point2::new(uv.x, uv.y))
}

/// The unit normal of `surface` at its point nearest `point`, a point of a Coons patch's
/// side, searched from the side's UV curve at patch parameter `s` (as
/// [`coons_side_normal`]).
///
/// Not the fork, which takes the normal at the UV curve's point itself. A side's 3D
/// curve and its UV curve's image need not coincide: the loop curves of the generated
/// corner cases leave their neighbours by up to a tenth of the patch's size (analytic064:
/// 428 off a torus of minor radius 1223 at mid-side), so the UV curve's point is another
/// place on the neighbour, and its normal differs from the neighbour's at the side by up
/// to the gap times the neighbour's curvature (0.2 radians there). G1 with the
/// neighbour is the angle at the side's own point, so that is what is measured.
///
/// Gauss-Newton on `|S(uv) - point|^2` from the UV curve's point: each step solves the
/// normal equations of the first derivatives, and the search stops when a step moves the
/// surface point by at most `EFF_ZERO` of the point's size. `None` where the search
/// finds no nearest point: an evaluation fails, the derivatives are (near) parallel, a
/// step is not finite, or 64 steps do not settle; the station is then left out rather
/// than measured against a normal at another point.
fn coons_side_normal_nearest(
    k: &crate::kernel::Kernel,
    uv_curve: &BSplineCurve,
    surface: &SmSurface,
    patch_ivl: &Extent1d,
    s: f64,
    point: Point3,
) -> Result<Option<Vec3>> {
    let ivl = uv_curve.natural_interval();
    let t = ivl.evaluate((s - patch_ivl.min()) / patch_ivl.length());
    let start = uv_curve.evaluate_point(t)?;
    let mut uv = Point2::new(start.x, start.y);
    let size = point.length().max(1.0);
    for _ in 0..64 {
        let Ok((at, su, sv)) = crate::surface::surface::p1::evaluate_1st_derivatives(
            k,
            surface,
            uv,
            Side::Left,
            Side::Left,
        ) else {
            return Ok(None);
        };
        let r = point - at;
        let (a11, a12, a22) = (su.dot(su), su.dot(sv), sv.dot(sv));
        let (b1, b2) = (su.dot(r), sv.dot(r));
        let det = a11 * a22 - a12 * a12;
        if det.is_nan() || det <= EFF_ZERO_SQ * a11 * a22 {
            return Ok(None);
        }
        let du = (a22 * b1 - a12 * b2) / det;
        let dv = (a11 * b2 - a12 * b1) / det;
        let next = Point2::new(uv.x + du, uv.y + dv);
        if !(next.x.is_finite() && next.y.is_finite()) {
            return Ok(None);
        }
        uv = next;
        if (su * du + sv * dv).length() <= EFF_ZERO * size {
            return evaluate_normal(k, surface, uv).map(Some);
        }
    }
    Ok(None)
}

/// Side `ii`'s UV curve and surface, `None` when either is missing.
fn coons_side<'a>(
    uv_curves: &'a [Option<BSplineCurve>],
    surfaces: &[Option<&'a SmSurface>],
    ii: usize,
) -> Option<(&'a BSplineCurve, &'a SmSurface)> {
    let uv = uv_curves.get(ii)?.as_ref()?;
    let srf = (*surfaces.get(ii)?)?;
    Some((uv, srf))
}

/// The angle [`coons_side_angle`] computed at one station and its rounding bound
/// (`coons_station_error`), `None` where the angle is not certified: the patch has no
/// normal there, the derivatives no rounding bound, the neighbour no nearest point, or
/// the bound is `PI / 2` or more (the angle lies in `[0, PI / 2]`, so such a bound
/// encloses nothing).
type StationAngle = Option<(f64, f64)>;

/// An interval `[lo, hi]` known to hold an exact angle.
type Enclosure = (f64, f64);

/// An enclosure `[lo, hi]` of a station's exact angle: its computed angle less and plus
/// its bound, each rounded outwards by a step (a correctly rounded sum is within half a
/// step of the exact one).
fn station_enclosure((angle, err): (f64, f64)) -> Enclosure {
    (next_down(angle - err), next_up(angle + err))
}

/// How a candidate's side compares with a copy's side, over the stations the copy
/// certifies ([`compare_side`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SideVerdict {
    /// Shown no worse: the candidate's largest angle is at most the copy's plus the
    /// resolution, by the enclosures, and not certainly above the copy's.
    NoWorse,
    /// Certainly above the copy's largest angle, but by at most the resolution: a veto
    /// unless the candidate's largest angle, at the stations the held copy certifies, is
    /// certainly lower by more than the resolution ([`SideAngle::no_worse_than`]).
    Rises,
    /// The candidate leaves out a station the copy certifies: no worse is not shown there.
    Uncertified,
    /// The candidate's largest angle is certainly larger than the copy's, and not shown
    /// within the resolution of it.
    Worse,
    /// Neither: the enclosures are too wide to show the candidate no worse.
    Undecided,
}

/// Compares one side of a candidate `cand` with the same side of a copy `held`, both
/// measured at the same stations, over the stations `held` certifies: the largest angle
/// of each over them lies in `[max lo, max hi]` of its enclosures
/// ([`station_enclosure`]). Where the candidate's lower end is above the copy's upper end
/// it is certainly worse: [`SideVerdict::Rises`] if its upper end is still at most the
/// copy's lower end plus `resolution`, else [`SideVerdict::Worse`]. Otherwise it is shown
/// no worse where its upper end is at most the copy's lower end plus `resolution`; an
/// undecided comparison does not count as no worse. A side `held` certifies nowhere
/// constrains nothing. Comparisons are written so that a NaN decides against the
/// candidate.
fn compare_side(cand: &[StationAngle], held: &[StationAngle], resolution: f64) -> SideVerdict {
    let (c, h) = match side_maxima(cand, held) {
        Err(v) => return v,
        Ok(None) => return SideVerdict::NoWorse,
        Ok(Some(m)) => m,
    };
    let within = c.1 <= next_down(h.0 + resolution);
    if c.0.is_nan() || h.1.is_nan() {
        SideVerdict::Worse
    } else if c.0 > h.1 {
        if within {
            SideVerdict::Rises
        } else {
            SideVerdict::Worse
        }
    } else if within {
        SideVerdict::NoWorse
    } else {
        SideVerdict::Undecided
    }
}

/// The largest angle of one side of `cand` and of `held` over the stations `held`
/// certifies, each as `[max lo, max hi]` of the stations' enclosures
/// ([`station_enclosure`]): `Ok(None)` where `held` certifies none, and
/// [`SideVerdict::Uncertified`] where `cand` lacks one of them.
fn side_maxima(
    cand: &[StationAngle],
    held: &[StationAngle],
) -> Result<Option<(Enclosure, Enclosure)>, SideVerdict> {
    if cand.len() != held.len() {
        return Err(SideVerdict::Uncertified);
    }
    let mut c = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut h = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut any = false;
    for (sc, sh) in cand.iter().zip(held) {
        let Some(sh) = *sh else { continue };
        let Some(sc) = *sc else {
            return Err(SideVerdict::Uncertified);
        };
        let (cl, ch) = station_enclosure(sc);
        let (hl, hh) = station_enclosure(sh);
        c = (c.0.max(cl), c.1.max(ch));
        h = (h.0.max(hl), h.1.max(hh));
        any = true;
    }
    Ok(any.then_some((c, h)))
}

/// What [`coons_side_angle`] measured, per side (left, bottom, right, top): `None` for a
/// side with no surface, else the station angles at the side's common stations
/// ([`coons_common_stations`]), the same stations for every candidate.
#[derive(Debug, Clone, PartialEq)]
struct SideAngle {
    /// Each side's station angles.
    sides: [Option<Vec<StationAngle>>; 4],
}

impl SideAngle {
    /// The largest computed angle over every certified station; `None` when there is
    /// none (the loop's stop test, as the fork's, on the computed values).
    fn sup(&self) -> Option<f64> {
        let mut sup: Option<f64> = None;
        for (angle, _) in self.sides.iter().flatten().flatten().flatten() {
            sup = Some(match sup {
                Some(s) if s >= *angle => s,
                _ => *angle,
            });
        }
        sup
    }

    /// The verdict of each side of `self` against `held` (`NoWorse` where `held` has no
    /// such side; `Uncertified` where `self` lacks a side `held` has).
    fn verdicts(&self, held: &SideAngle, resolution: f64) -> [SideVerdict; 4] {
        let mut out = [SideVerdict::NoWorse; 4];
        for (v, (c, h)) in out.iter_mut().zip(self.sides.iter().zip(&held.sides)) {
            *v = match (c, h) {
                (_, None) => SideVerdict::NoWorse,
                (None, Some(h)) if h.iter().all(Option::is_none) => SideVerdict::NoWorse,
                (None, Some(_)) => SideVerdict::Uncertified,
                (Some(c), Some(h)) => compare_side(c, h, resolution),
            };
        }
        out
    }

    /// Whether `self` is shown no worse than `held`: every side is
    /// [`SideVerdict::NoWorse`] or [`SideVerdict::Rises`] (certainly above, but within the
    /// resolution), and where any rises, the largest angle at the stations `held`
    /// certifies is certainly lower than `held`'s there by more than the resolution
    /// ([`Self::lowers_sup`]). A side may
    /// give up at most the resolution only in exchange for a certainly smaller seam:
    /// `blend_crossing`'s crease corner (bare patch 12 and 29 degrees off along two sides,
    /// 0 along the third; its refined copies 0.3 degrees along the two and 0.4 along the
    /// third). Without that exchange a certain rise vetoes, however small: analytic064's
    /// refined copies rise by 6e-5 radians on two sides and lower nothing.
    fn no_worse_than(&self, held: &SideAngle, resolution: f64) -> bool {
        let verdicts = self.verdicts(held, resolution);
        if verdicts
            .iter()
            .any(|v| !matches!(v, SideVerdict::NoWorse | SideVerdict::Rises))
        {
            return false;
        }
        !verdicts.contains(&SideVerdict::Rises) || self.lowers_sup(held, resolution)
    }

    /// Whether the largest angle of `self` over the stations `held` certifies, on every
    /// side both measure, is certainly below `held`'s there by more than `resolution`:
    /// the largest upper end of `self` at most the largest lower end of `held` less
    /// `resolution`, by the enclosures ([`side_maxima`]).
    fn lowers_sup(&self, held: &SideAngle, resolution: f64) -> bool {
        let mut c_hi = f64::NEG_INFINITY;
        let mut h_lo = f64::NEG_INFINITY;
        for (c, h) in self.sides.iter().zip(&held.sides) {
            let (Some(c), Some(h)) = (c, h) else { continue };
            match side_maxima(c, h) {
                Ok(Some((c, h))) => {
                    c_hi = c_hi.max(c.1);
                    h_lo = h_lo.max(h.0);
                }
                Ok(None) => {}
                Err(_) => return false,
            }
        }
        h_lo.is_finite() && c_hi <= next_down(h_lo - resolution)
    }
}

/// Whether [`make_coons_patch_tangent_to_sides`] takes a candidate measured as `cand`:
/// it is shown no worse ([`SideAngle::no_worse_than`], at [`COONS_RESOLUTION`]) than
/// every copy held so far, the bare patch first. Against every one, not only the last
/// taken: each tie may let a copy rise by up to the resolution, and a chain of them
/// would drift without bound from the bare patch.
fn coons_takes(held: &[SideAngle], cand: &SideAngle) -> bool {
    held.iter().all(|h| cand.no_worse_than(h, COONS_RESOLUTION))
}

/// A station's angle and rounding bound as [`coons_side_angle`] certifies it: `None`
/// unless the bound is below `PI / 2`. The exact angle lies in `[0, PI / 2]`, so a bound
/// of `PI / 2` or more encloses every angle there is and bounds nothing.
fn certified_station(angle: f64, err: f64) -> StationAngle {
    (err < std::f64::consts::FRAC_PI_2).then_some((angle, err))
}

/// One station of a side angle measurement ([`CoonsCandidateLog`]): what
/// `coons_side_angle` computed there.
#[derive(Debug, Clone, PartialEq)]
pub struct CoonsStationLog {
    /// The side: 0 left (u min), 1 bottom (v min), 2 right (u max), 3 top (v max).
    pub side: usize,
    /// The patch parameter of the station along the side.
    pub s: f64,
    /// The station's patch parameters.
    pub uv: Point2,
    /// The candidate's computed first derivatives at `uv` (without the zero-tangent
    /// repair).
    pub du: Vec3,
    /// See `du`.
    pub dv: Vec3,
    /// Their rounding bounds `(eu, ev)` (`coons_derivative_errors`), `None` where there
    /// is none.
    pub derivative_errors: Option<(f64, f64)>,
    /// The point of the unrefined patch at `uv` (the side point every candidate shares)
    /// and the neighbour's computed unit normal nearest it; `None` where the station is
    /// skipped before they are evaluated, or the neighbour has no nearest point (`angle`
    /// is then `None`).
    pub neighbour: Option<(Point3, Vec3)>,
    /// The computed angle (radians), `None` where the patch has no normal (the
    /// derivatives (near) parallel or vanishing) or the neighbour no nearest point, and
    /// the station is skipped.
    pub angle: Option<f64>,
    /// The rounding bound of `angle` (`coons_station_error`), `PI` where there is none.
    /// The station is certified, and counts, only where this is below `PI / 2`.
    pub err: f64,
}

/// One candidate of [`CoonsTangencyLog`]: the patch and its side angle measurement.
#[derive(Debug, Clone)]
pub struct CoonsCandidateLog {
    /// The refinement's spans each way; `0` for the unrefined patch.
    pub spans: usize,
    /// The candidate patch.
    pub patch: BSplineSurface,
    /// Every station measured, in order: each side's common stations, the same for
    /// every candidate.
    pub stations: Vec<CoonsStationLog>,
}

/// The record of one `make_coons_patch_tangent_to_sides` run, kept while
/// [`crate::kernel::Kernel::coons_tangency_log`] is on: its target, every candidate it
/// measured (the unrefined patch first) and the one it returned. A diagnostic for tests
/// that replay the choice; keeping it changes no result.
#[derive(Debug, Clone)]
pub struct CoonsTangencyLog {
    /// The angle (radians) below which refinement stops.
    pub target: f64,
    /// The resolution of the comparisons: a candidate is shown no worse along a side
    /// where its largest angle's upper end is at most a held copy's lower end plus this.
    pub resolution: f64,
    /// The candidates measured, in order (`candidates[0]` the unrefined patch).
    pub candidates: Vec<CoonsCandidateLog>,
    /// The index of the candidate returned; `None` when the unrefined patch is kept (or
    /// the patch is rational and nothing was measured).
    pub chosen: Option<usize>,
}

/// The rounding data the angle bound needs at one station: `(eu, ev)`, bounds on the
/// absolute error of the first derivatives in u and v computed at `uv` (without the
/// zero-tangent repair), against the exact derivatives of the patch at the same `uv`.
///
/// The derivative is `sum_ij N'_i(u) N_j(v) P_ij` (the patch is non-rational, not in
/// power-basis form), from the Cox-de Boor triangle and its first-derivative row
/// (`basis_derivs`, algorithm A2.3). Inside the span `[t_a, t_a+1]` that holds `u`, every
/// knot difference the triangle forms, `u - t` and `t - u`, is non-negative and is one
/// rounded subtraction of two floats, so within `unit` of its value relatively; each
/// level adds and divides non-negative terms, so the basis values `N_i` are within
/// `gamma(6 p)` relatively. The derivative row is `p (N_(i-1, p-1) / d - N_(i, p-1) / d')`
/// with knot differences `d`, `d'` that span the support of a basis function non-zero
/// at `u`, so at least the span's length `h`: its error is within `gamma(6 p + 4)` of
/// `p (N_(i-1, p-1) / d + N_(i, p-1) / d')`, which sums over `i` to at most `2 p / h`. The
/// tensor sum adds `(p + 1)(q + 1)` products of control points of the span, each at most
/// `M` (the largest `|x| + |y| + |z|` among them). So
/// `|fl(du) - du| <= gamma(K) (2 p / h_u) M` with `K = 8 (p + q + 2) + 2 (p + 1)(q + 1)`
/// (an overcount), and likewise `dv` with `2 q / h_v`. At a knot `u` lies in two spans and
/// the evaluation takes one: `h` is the shorter and `M` is over both.
///
/// Not a global bound: the shortest knot gap and largest control point of the whole patch
/// overstated the error of every station away from them, and the knot differences are
/// relatively accurate, not absolutely within `u T` (`T` the largest knot), as an earlier
/// form of this bound took them (with a factor `2 + 2 T / h_u + 2 T / h_v`). On a graded
/// knot vector the two together overstated it some 1e5-fold, and the bound swallowed whole
/// sides' angles (WR-713: `vase_rev_corner_blend_3`'s stations read `1.5` radians of
/// rounding where the derivatives are good to `1e-10` of their size).
fn coons_derivative_errors(nurb: &Surface, uv: Point2) -> Option<(f64, f64)> {
    let (p, q) = (nurb.p(), nurb.q());
    // the shortest span whose closed interval holds `t`, and the first and last control
    // point index of the spans that do
    let local = |knots: &[f64], deg: usize, t: f64| -> Option<(f64, usize, usize)> {
        let mut h = f64::INFINITY;
        let (mut lo, mut hi) = (usize::MAX, 0_usize);
        for (a, w) in knots.windows(2).enumerate() {
            if let [t0, t1] = w {
                if t1 > t0 && *t0 <= t && t <= *t1 {
                    h = h.min(t1 - t0);
                    lo = lo.min(a.checked_sub(deg)?);
                    hi = hi.max(a);
                }
            }
        }
        (h.is_finite() && h > 0.0).then_some((h, lo, hi))
    };
    let (hu, ilo, ihi) = local(nurb.u_knots(), p, uv.x)?;
    let (hv, jlo, jhi) = local(nurb.v_knots(), q, uv.y)?;
    let mut m = 0.0_f64;
    for i in ilo..=ihi {
        for j in jlo..=jhi {
            let c = nurb.cpt(i, j).ok()?;
            m = m.max(c.x.abs() + c.y.abs() + c.z.abs());
        }
    }
    let g = gamma(8 * (p + q + 2) + 2 * (p + 1) * (q + 1));
    let (pf, qf) = (p as f64, q as f64);
    let eu = g * (2.0 * pf / hu) * m;
    let ev = g * (2.0 * qf / hv) * m;
    (eu.is_finite() && ev.is_finite()).then_some((eu, ev))
}

/// The rounding bound of one station's angle: `du`, `dv` the computed derivatives (each
/// within `eu`, `ev` of exact), `normal` the computed unit patch normal, `side_normal` the
/// computed side normal (taken as exact: the same vector in every candidate) and `angle`
/// the computed angle.
///
/// * the cross product: `|fl(du x dv) - du_x x dv_x| <= eu |dv| + (|du| + eu) ev + 2 gamma(2)
///   (|du| + eu)(|dv| + ev)`; over `|du_x x dv_x|`, at least the computed length less that,
///   this is the sine of the direction error, which is at most `PI / 2` times it;
/// * unitizing adds `gamma(2)` to the direction;
/// * the cosine `|n . m|` of unit `n`, `m` is within `gamma(3) |n||m| + ||n||m| - 1|` of
///   the computed one, with `|n| = 1 +- gamma(6)` and `|m|` within `gamma(3)` of its
///   computed length;
/// * `acos` moves a cosine error `e` by at most `2 e / sin` where `sin^2 >= 8 e`, else by
///   at most `PI sqrt(e)` (`acos` is 1/2-Holder with constant `PI / sqrt 2`); the library
///   `acos` is taken within 2 ulp.
fn coons_station_error(
    du: Vec3,
    dv: Vec3,
    (eu, ev): (f64, f64),
    side_normal: Vec3,
    cos: f64,
    angle: f64,
) -> f64 {
    let (a, b) = (du.length() + eu, dv.length() + ev);
    let cross_err = eu * dv.length() + a * ev + 2.0 * gamma(2) * a * b;
    let cross_len = du.cross(dv).length() * (1.0 - gamma(3)) - cross_err;
    if cross_len.is_nan() || cross_len <= 0.0 || cross_err >= cross_len {
        return PI;
    }
    let dir = 0.5 * PI * (cross_err / cross_len) + gamma(2);
    let m_len = side_normal.length();
    let m_dev = (m_len - 1.0).abs() + gamma(3) * m_len;
    let n_dev = gamma(6);
    let e_cos = gamma(3) * (1.0 + n_dev) * (1.0 + m_dev) + n_dev + m_dev + n_dev * m_dev;
    let sin_sq = 1.0 - cos * cos;
    let from_cos = if sin_sq >= 8.0 * e_cos {
        2.0 * e_cos / sin_sq.sqrt()
    } else {
        PI * e_cos.sqrt()
    };
    let total = dir + from_cos + 4.0 * UNIT * angle;
    if total.is_finite() {
        total
    } else {
        PI
    }
}

/// The stations [`coons_side_angle`] measures a side at, along `along` (the side's
/// interval in direction `dir`): four a knot span, so a miss between control points is
/// seen, and 33 evenly spaced where that would be fewer.
///
/// Not the fork, which spaces `4 * spans + 1` stations evenly: on a graded knot vector
/// (an n-sided patch's spans halve towards its corners, down to `1/2048` of the side)
/// even spacing steps over the short spans, and a refined copy whose projected rows bend
/// the patch inside them read as tangent as the bare patch (WR-713: analytic034's sides
/// leave the cylinder by 26 degrees inside the last span, against 20 at the corner).
///
/// Nor does the fork add [`coons_corner_stations`] in the first and last gaps, here at
/// each end whose corner `corners` (start, end) marks as one the loop curves fix.
fn coons_stations(
    knots: Option<&[f64]>,
    fallback_spans: usize,
    along: &Extent1d,
    corners: [bool; 2],
) -> Vec<f64> {
    let mut stations = coons_span_stations(knots, fallback_spans, along);
    let near_corners = coons_corner_stations(&stations, corners);
    stations.extend(near_corners);
    stations
}

/// Whether each end (start, end) of side `ii` is a corner whose normal the loop curves
/// fix, for [`coons_corner_stations`]: not where the side it meets there has no surface.
/// `create_corner_blend` refuses a side without a surface unless its curve is degenerate,
/// so such a side is a single point and both its ends are a collapsed corner (the apex of
/// a three-sided corner), where the patch normal has no value to approach.
fn coons_fixed_corners(surfaces: &[Option<&SmSurface>], ii: usize) -> [bool; 2] {
    let has = |side: usize| surfaces.get(side).copied().flatten().is_some();
    let (start, end) = if ii == BOT || ii == TOP {
        (LEFT, RIGHT)
    } else {
        (BOT, TOP)
    };
    [has(start), has(end)]
}

/// The stations every candidate of [`make_coons_patch_tangent_to_sides`] is measured at
/// along a side of `patch` in direction `dir`: the union, ascending and without repeats,
/// of the [`coons_stations`] of the patch's knot vector and of each refinement's
/// ([`refinement_knots`], 8, 16, 32 and 64 spans), with corner stations at the ends
/// `corners` (start, end) marks ([`coons_fixed_corners`]).
///
/// Not the fork, which measures each copy at its own stations. Copies compared at
/// different stations compare different maxima: a coarse copy's peak between its
/// stations is unseen where a finer copy's is seen, and the coarse copy reads better
/// for being looked at less (review of WR-713). At common stations a candidate's
/// largest angle and a held copy's are maxima over the same points of the side.
fn coons_common_stations(
    patch: &BSplineSurface,
    dir: SurfParamType,
    along: &Extent1d,
    corners: [bool; 2],
) -> Vec<f64> {
    let fallback = patch
        .number_control_points(dir)
        .saturating_sub(patch.degree(dir));
    let knots = patch.gw_nurb().map(|nurb| {
        if dir == SurfParamType::U {
            nurb.u_knots().to_vec()
        } else {
            nurb.v_knots().to_vec()
        }
    });
    let mut all = coons_stations(knots.as_deref(), fallback, along, corners);
    if let Some(knots) = &knots {
        for spans in COONS_REFINEMENTS {
            let refined = refinement_knots(knots, spans);
            all.extend(coons_stations(Some(&refined), fallback, along, corners));
        }
    }
    all.retain(|s| s.is_finite());
    all.sort_by(f64::total_cmp);
    all.dedup();
    all
}

/// The refinements [`make_coons_patch_tangent_to_sides`] tries, in spans each way.
const COONS_REFINEMENTS: [usize; 4] = [8, 16, 32, 64];

/// The resolution (radians) of [`make_coons_patch_tangent_to_sides`]'s comparisons:
/// `SM_CONTINUITY_ANGLE`, the angle within which SMLib calls a seam tangent. A candidate
/// is no worse than a held copy along a side where its largest angle is certainly at
/// most the copy's plus this. Fixed, not the caller's tangency tolerance (which may be
/// zero), so that ties are decided the same for every caller.
const COONS_RESOLUTION: f64 = ANG_TOL_DEG * (PI / 180.0);

/// `2^(-j/4)` for `j` in `0..4`, rounded to nearest: the fractions of an octave
/// [`coons_corner_stations`] steps by.
const QUARTER_OCTAVES: [f64; 4] = [
    1.0,
    0.840_896_415_253_714_5,
    std::f64::consts::FRAC_1_SQRT_2,
    0.594_603_557_501_360_5,
];

/// Stations towards both corners of a side: in the first gap `[s0, s1]` of `stations`
/// (ascending), `s0 + (s1 - s0) 2^(-e/4)` for `e` in `9..=64` (four an octave, from just
/// under a quarter of the gap down to `2^-16` of it), and likewise from the last
/// station back into the last gap.
///
/// Not the fork. A side's corner normal is set by the loop curves, so every candidate
/// reads the same angle there, and next to the corner the patch normal turns from it
/// faster than anywhere else on the side. A copy whose projected rows twist the corner's
/// first span can rise above the corner's angle well inside the span's first station
/// (WR-713: analytic064's refined copies leave the torus by 0.51721 radians `4.2e-6`
/// along a side whose first span is `4.9e-4` long, above the corner's 0.51715), and that
/// rise decides between candidates whose maxima are otherwise the corner's.
///
/// Only at the ends `fixed` (start, end) marks ([`coons_fixed_corners`]). At a collapsed
/// corner the premise fails: the loop fixes no normal there (the patch's own is
/// undefined at the apex), and next to it the patch normal is the cross product of a
/// derivative shrinking to zero, so the stations there read rounding, not tangency. In
/// the L-shaped box's three-sided corner (`fork_cae8618_l_box_corner_patch`) they lie
/// within `1e-5` of the apex with bounds of `0.2` to `1.5` radians on every candidate,
/// and with them no refined copy could be shown no worse than the bare patch, which
/// leaves its neighbours by 38.5 degrees where the 64-span copy is within 0.4.
///
/// The trade-off: at a collapsed end every corner station goes, from `2^-2.25` of the
/// gap down to `2^-16`, not only those within `1e-5` of the apex that read rounding. The
/// stations nearest the apex are then the common span stations, the closest a quarter
/// of the 64-span refinement's end span (`1/256` of a uniformly knotted side), and a
/// rise inside that last gap is unseen. That is accepted because the stations' premise,
/// a corner normal the loop fixes, is absent at an apex. A side whose corner meets a
/// side with a surface keeps its corner stations there, as before.
fn coons_corner_stations(stations: &[f64], fixed: [bool; 2]) -> Vec<f64> {
    let mut corners = Vec::new();
    let (Some(&s0), Some(&s1)) = (stations.first(), stations.get(1)) else {
        return corners;
    };
    let n = stations.len();
    let (Some(&t0), Some(&t1)) = (stations.get(n - 1), stations.get(n.wrapping_sub(2))) else {
        return corners;
    };
    let [at_start, at_end] = fixed;
    for e in 9..=64_usize {
        let Some(&q) = QUARTER_OCTAVES.get(e % 4) else {
            continue;
        };
        // 2^-(e / 4) is exact, and so the product's only rounding is the one of `q`
        let f = q * 0.5_f64.powi(i32::try_from(e / 4).unwrap_or(0));
        if at_start {
            corners.push(s0 + (s1 - s0) * f);
        }
        if at_end {
            corners.push(t0 - (t0 - t1) * f);
        }
    }
    corners
}

/// The stations [`coons_stations`] spaces by the knot spans (without the corner ones);
/// with no knot vector, `4 * fallback_spans + 1` (33 at least) evenly spaced.
fn coons_span_stations(knots: Option<&[f64]>, fallback_spans: usize, along: &Extent1d) -> Vec<f64> {
    let even = |count: usize| -> Vec<f64> {
        (0..count)
            .map(|kk| along.evaluate(kk as f64 / (count - 1) as f64))
            .collect()
    };
    let Some(knots) = knots else {
        return even((4 * fallback_spans + 1).max(33));
    };
    let mut breaks: Vec<f64> = vec![along.min()];
    for &t in knots {
        if t > along.min() && t < along.max() && breaks.last().is_some_and(|b| t > *b) {
            breaks.push(t);
        }
    }
    breaks.push(along.max());
    let spans = breaks.len() - 1;
    if 4 * spans + 1 < 33 {
        return even(33);
    }
    let mut stations = Vec::new();
    for w in breaks.windows(2) {
        if let [a, b] = w {
            for j in 0..4 {
                stations.push(a + (b - a) * (f64::from(j) / 4.0));
            }
        }
    }
    stations.push(along.max());
    stations
}

/// One station of a side ([`CoonsSideFrames`]): its parameter along the side and, where
/// there is one, the unrefined patch's side point and the neighbour's unit normal
/// nearest it.
type CoonsSideFrame = (f64, Option<(Point3, Vec3)>);

/// What every candidate of [`make_coons_patch_tangent_to_sides`] is measured against,
/// read once from the unrefined patch: per side (left, bottom, right, top), `None` for a
/// side with no UV curve or surface, else a frame at each common station
/// ([`coons_common_stations`]). The side point and normal do not depend on the
/// candidate (refinement and the projection of the side rows leave the sides in
/// place), so each station's side normal is the same vector for every candidate.
struct CoonsSideFrames {
    /// Each side's frames, in station order.
    sides: [Option<Vec<CoonsSideFrame>>; 4],
}

/// The station `s` of side `ii` as a point of the parameter domain `su` x `sv`.
fn coons_station_uv(ii: usize, s: f64, su: &Extent1d, sv: &Extent1d) -> Point2 {
    match ii {
        LEFT => Point2::new(su.min(), s),
        BOT => Point2::new(s, sv.min()),
        RIGHT => Point2::new(su.max(), s),
        _ => Point2::new(s, sv.max()),
    }
}

/// The evaluation flags [`coons_side_angle`] and [`coons_side_frames`] read a station
/// with: the side of each knot that lies inside the domain, and the derivatives as
/// computed, without `Evaluate1stDerivatives`'s repair of a short one (a stand-in from
/// the second derivatives, which no rounding bound covers).
fn coons_station_flags(uv: Point2, su: &Extent1d, sv: &Extent1d) -> EvaluateFlags {
    let side = |below_max: bool| if below_max { Side::Left } else { Side::Right };
    EvaluateFlags {
        u_side: side(uv.x < su.max()),
        v_side: side(uv.y < sv.max()),
        only_upper_half: true,
        non_zero_tangents: false,
    }
}

/// The [`CoonsSideFrames`] of the unrefined `patch`: at each common station the patch's
/// side point and the neighbour's normal nearest it ([`coons_side_normal_nearest`], not
/// the fork's normal at the UV curve's point). A station where the neighbour has no
/// nearest point, or its normal fails to evaluate, has no frame and certifies nothing.
/// Corner stations go at the corners the loop fixes ([`coons_fixed_corners`]).
fn coons_side_frames(
    k: &crate::kernel::Kernel,
    patch: &BSplineSurface,
    uv_curves: &[Option<BSplineCurve>],
    surfaces: &[Option<&SmSurface>],
) -> Result<CoonsSideFrames> {
    coons_side_frames_at(k, patch, uv_curves, surfaces, |ii| {
        coons_fixed_corners(surfaces, ii)
    })
}

/// [`coons_side_frames`] with corner stations at the ends `corners(side)` (start, end)
/// marks.
fn coons_side_frames_at(
    k: &crate::kernel::Kernel,
    patch: &BSplineSurface,
    uv_curves: &[Option<BSplineCurve>],
    surfaces: &[Option<&SmSurface>],
    corners: impl Fn(usize) -> [bool; 2],
) -> Result<CoonsSideFrames> {
    let mut sides: [Option<Vec<CoonsSideFrame>>; 4] = [None, None, None, None];
    let domain = patch.natural_uv_domain();
    let su = Extent1d::from_min_max(domain.min.x, domain.max.x);
    let sv = Extent1d::from_min_max(domain.min.y, domain.max.y);
    for (ii, slot) in sides.iter_mut().enumerate() {
        let Some((uv_curve, surface)) = coons_side(uv_curves, surfaces, ii) else {
            continue;
        };
        let along_u = ii == BOT || ii == TOP;
        let along = if along_u { &su } else { &sv };
        let dir = if along_u {
            SurfParamType::U
        } else {
            SurfParamType::V
        };
        let mut frames = Vec::new();
        for s in coons_common_stations(patch, dir, along, corners(ii)) {
            let uv = coons_station_uv(ii, s, &su, &sv);
            // only the point is used: no tangent repair, which can fail where a
            // derivative of the patch vanishes though the station is valid
            let side_point = patch
                .evaluate(uv, 1, 1, coons_station_flags(uv, &su, &sv))?
                .first()
                .and_then(|row| row.first())
                .copied()
                .ok_or_else(|| {
                    SolidError::b_spline(
                        OP_CORNER,
                        BSplineFailure::Defect {
                            why: SHORT_SURFACE_EVALUATION,
                        },
                    )
                })?;
            let normal = ignored(coons_side_normal_nearest(
                k, uv_curve, surface, along, s, side_point,
            ))?
            .flatten();
            frames.push((s, normal.map(|n| (side_point, n))));
        }
        *slot = Some(frames);
    }
    Ok(CoonsSideFrames { sides })
}

/// The fork's `sm_CoonsSideAngle`: the angles (radians) between a Coons patch's normal
/// and the normals of the surfaces along its sides. Sides are ordered left (u min),
/// bottom (v min), right (u max), top (v max); a side with no surface is skipped, and
/// so is a station where the patch has no normal (a degenerate side's point).
///
/// Not the fork, which returns the largest angle at its own `4 * spans + 1` stations:
/// the result is every station's angle with its rounding bound
/// ([`coons_station_error`]), at the common stations of `frames`, the same for every
/// candidate, against the side normals read once from the unrefined patch
/// ([`coons_side_frames`]). [`make_coons_patch_tangent_to_sides`] compares candidates
/// station by station. G1 is undefined where the normal is: a station is certified only
/// where its bound is below `PI / 2` (the angle lies in `[0, PI / 2]`, so a wider bound
/// encloses nothing); elsewhere, and where there is no frame, it is `None`.
fn coons_side_angle(
    patch: &BSplineSurface,
    frames: &CoonsSideFrames,
    mut log: Option<&mut Vec<CoonsStationLog>>,
) -> Result<SideAngle> {
    let mut per_side: [Option<Vec<StationAngle>>; 4] = [None, None, None, None];
    // the derivative bound holds for the B-spline basis of a non-rational patch
    let nurb = patch
        .gw_nurb()
        .filter(|_| !patch.is_rational() && patch.form() != BSplineSurfaceForm::Polynomial);
    let domain = patch.natural_uv_domain();
    let su = Extent1d::from_min_max(domain.min.x, domain.max.x);
    let sv = Extent1d::from_min_max(domain.min.y, domain.max.y);
    for (ii, (slot, side_frames)) in per_side.iter_mut().zip(&frames.sides).enumerate() {
        let Some(side_frames) = side_frames else {
            continue;
        };
        let mut angles = Vec::new();
        for &(s, frame) in side_frames {
            let uv = coons_station_uv(ii, s, &su, &sv);
            // next to a collapsed corner the station is then skipped or left out
            let d = patch.evaluate(uv, 1, 1, coons_station_flags(uv, &su, &sv))?;
            let deriv = |i: usize, j: usize| -> Result<Vec3> {
                d.get(i).and_then(|row| row.get(j)).copied().ok_or_else(|| {
                    SolidError::b_spline(
                        OP_CORNER,
                        BSplineFailure::Defect {
                            why: SHORT_SURFACE_EVALUATION,
                        },
                    )
                })
            };
            let (du, dv) = (deriv(1, 0)?, deriv(0, 1)?);
            let deriv_err = nurb.and_then(|n| coons_derivative_errors(n, uv));
            let mut normal = du.cross(dv);
            let scale = du.length() * dv.length();
            // No normal where the derivatives are (near) parallel, measured against their
            // own size; where there is one it is made unit by its own length (the fork's
            // tangency-labs/smlib@701da4b): `unitize`'s absolute floor, 1e-12, refused the
            // normal of a station near a collapsing side of a patch 0.01 across.
            let normal_length = normal.length();
            let frame =
                frame.filter(|_| scale >= EFF_ZERO_SQ && normal_length > EFF_ZERO_SQRT * scale);
            let Some((side_point, side_normal)) = frame else {
                if let Some(log) = log.as_deref_mut() {
                    log.push(CoonsStationLog {
                        side: ii,
                        s,
                        uv,
                        du,
                        dv,
                        derivative_errors: deriv_err,
                        neighbour: None,
                        angle: None,
                        err: PI,
                    });
                }
                angles.push(None);
                continue;
            };
            normal /= normal_length;
            let cos = normal.dot(side_normal).abs();
            let angle = acos(if cos < 1.0 { cos } else { 1.0 });
            let err = match deriv_err {
                Some(e) => coons_station_error(du, dv, e, side_normal, cos, angle),
                None => PI,
            };
            let err = if err >= PI { PI } else { err };
            if let Some(log) = log.as_deref_mut() {
                log.push(CoonsStationLog {
                    side: ii,
                    s,
                    uv,
                    du,
                    dv,
                    derivative_errors: deriv_err,
                    neighbour: Some((side_point, side_normal)),
                    angle: Some(angle),
                    err,
                });
            }
            // G1 is undefined where the patch normal is: a station whose angle is not
            // certified (no derivative bound, or a bound of `PI / 2` or more, as where the
            // cross product is within its rounding of zero next to a collapsed corner) is
            // left out, neither a tie nor a win
            angles.push(certified_station(angle, err));
        }
        *slot = Some(angles);
    }
    Ok(SideAngle { sides: per_side })
}

/// The fork's `sm_RefineToSpans`: knots inserted in `param`'s direction until no span
/// is longer than `1 / spans` of the domain (the surface's shape is unchanged).
fn refine_to_spans(surface: &mut BSplineSurface, param: SurfParamType, spans: usize) -> Result<()> {
    const OP: &str = OP_CORNER;
    let (knots, _) = surface.knots(param, None)?;
    if knots.len() < 2 {
        return Err(SolidError::b_spline(OP, BSplineFailure::PatchWithoutSpan));
    }
    let new_knots = refinement_new_knots(&knots, spans);
    if !new_knots.is_empty() {
        surface.insert_knots(param, &new_knots)?;
    }
    Ok(())
}

/// The knots [`refine_to_spans`] inserts into `knots` (ascending): each span longer
/// than `1 / spans` of the whole cut into equal pieces no longer than that.
fn refinement_new_knots(knots: &[f64], spans: usize) -> Vec<f64> {
    let (Some(&first), Some(&last)) = (knots.first(), knots.last()) else {
        return Vec::new();
    };
    let max_span = (last - first) / spans as f64;
    let mut new_knots = Vec::new();
    for pair in knots.windows(2) {
        let [lo, hi] = pair else {
            continue;
        };
        let length = hi - lo;
        let pieces = (length / max_span - 1.0e-9).ceil() as usize;
        for kk in 1..pieces {
            new_knots.push(lo + length * kk as f64 / pieces as f64);
        }
    }
    new_knots
}

/// The knot vector [`refine_to_spans`] leaves: `knots` with the
/// [`refinement_new_knots`] merged in (the stations only need its distinct values).
fn refinement_knots(knots: &[f64], spans: usize) -> Vec<f64> {
    let mut all = knots.to_vec();
    all.extend(refinement_new_knots(knots, spans));
    all.retain(|t| t.is_finite());
    all.sort_by(f64::total_cmp);
    all
}

/// The fork's `sm_ProjectCoonsSideRows`: each control point next to a side of a Coons
/// patch moved, by the least distance, into the tangent plane of that side's surface at
/// the point's Greville station along the side; a point next to two sides (at a
/// corner) onto the mean of their planes. Needs at least 5 control points each way, so
/// that no point is next to two opposite sides. Every plane is read from the unmoved
/// net before any point is written.
///
/// Not the fork's (WR-613): `face_sides` marks the sides on a face of the body (bit
/// `i` for side `i`: left, bottom, right, top), as `CreateCornerBlend`'s `lFaceSides`.
/// A point next to exactly one marked side and one unmarked side goes into the marked
/// side's plane alone, as the Coons corner twist there is the unmarked side's: the patch
/// keeps its tangency to the body's face exactly and the blend side takes the miss. On
/// the mean plane the point stood off the face's tangent plane by half the two planes'
/// separation there, so a patch along a plane face crossed it.
fn project_coons_side_rows(
    k: &crate::kernel::Kernel,
    patch: &mut BSplineSurface,
    uv_curves: &[Option<BSplineCurve>],
    surfaces: &[Option<&SmSurface>],
    face_sides: u32,
) -> Result<()> {
    const OP: &str = OP_CORNER;
    let nu = patch.number_control_points(SurfParamType::U);
    let nv = patch.number_control_points(SurfParamType::V);
    let pu = patch.degree(SurfParamType::U);
    let pv = patch.degree(SurfParamType::V);
    if nu < 5 || nv < 5 || pu < 1 || pv < 1 {
        return Err(SolidError::b_spline(
            OP,
            BSplineFailure::PatchTooCoarse {
                u_count: nu,
                v_count: nv,
                u_degree: pu,
                v_degree: pv,
            },
        ));
    }
    let nurb = patch
        .gw_nurb()
        .ok_or_else(|| SolidError::b_spline(OP, BSplineFailure::NoNurbsForm))?;
    let (ku, kv) = (nurb.u_knots().to_vec(), nurb.v_knots().to_vec());
    // (counts that agree give each direction at least degree + 6 knots)
    let (true, Some(&ku_first), Some(&ku_last), Some(&kv_first), Some(&kv_last)) = (
        ku.len() == nu + pu + 1 && kv.len() == nv + pv + 1,
        ku.first(),
        ku.last(),
        kv.first(),
        kv.last(),
    ) else {
        return Err(SolidError::b_spline(
            OP,
            BSplineFailure::Defect {
                why: "the corner patch's knot, control point and degree counts do not agree, \
                      and the NURBS form the patch holds is built only with counts that do",
            },
        ));
    };
    let su = Extent1d::from_min_max(ku_first, ku_last);
    let sv = Extent1d::from_min_max(kv_first, kv_last);

    // the Greville station of control point `index` in one direction
    let greville = |knots: &[f64], degree: usize, index: usize| -> Result<f64> {
        let mut sum = 0.0;
        for kk in 1..=degree {
            sum += at(knots, index + kk, OP)?;
        }
        Ok(sum / degree as f64)
    };

    let mut moved: Vec<(usize, usize, Point3)> = Vec::new();
    for i in 1..nu - 1 {
        for j in 1..nv - 1 {
            // the sides this control point is next to: (side, anchor u, anchor v)
            let mut sides: Vec<(usize, usize, usize)> = Vec::with_capacity(2);
            if i == 1 {
                sides.push((LEFT, 0, j));
            }
            if i == nu - 2 {
                sides.push((RIGHT, nu - 1, j));
            }
            if j == 1 {
                sides.push((BOT, i, 0));
            }
            if j == nv - 2 && sides.len() < 2 {
                sides.push((TOP, i, nv - 1));
            }
            if sides.is_empty() {
                continue;
            }

            let (mut pnt, _) = patch.control_point(ControlPointForm::NonRational, i, j)?;

            // each side's plane: unit normal, the signed distance of pnt from it, and
            // whether the side is on a face of the body
            let mut planes: Vec<(Vec3, f64, bool)> = Vec::with_capacity(2);
            for &(side, anchor_u, anchor_v) in &sides {
                let Some((uv_curve, surface)) = coons_side(uv_curves, surfaces, side) else {
                    continue;
                };
                let along_u = side == BOT || side == TOP;
                let s = if along_u {
                    greville(&ku, pu, i)?
                } else {
                    greville(&kv, pv, j)?
                };
                let (anchor, _) =
                    patch.control_point(ControlPointForm::NonRational, anchor_u, anchor_v)?;
                let normal =
                    coons_side_normal(k, uv_curve, surface, if along_u { &su } else { &sv }, s)?;
                let on_face = u32::try_from(side)
                    .ok()
                    .and_then(|side| 1u32.checked_shl(side))
                    .is_some_and(|bit| face_sides & bit != 0);
                planes.push((normal, normal.dot(pnt - anchor), on_face));
            }
            match planes.as_slice() {
                [] => continue,
                // next to one side, or to a face side and a blend side: into the (face)
                // side's plane
                [(normal, dist, _)]
                | [(normal, dist, true), (_, _, false), ..]
                | [(_, _, false), (normal, dist, true), ..] => pnt -= *normal * *dist,
                [(normal0, dist0, _), (normal1, dist1, _), ..] => {
                    // next to two sides at a corner: both planes are near the corner's
                    // tangent plane, their line far off; onto their mean plane, which
                    // leaves each a second-order miss
                    let mean = if normal0.dot(*normal1) < 0.0 {
                        *normal0 - *normal1
                    } else {
                        *normal0 + *normal1
                    };
                    let Ok(mean) = mean.unitize() else {
                        continue;
                    };
                    let anchor0 = pnt - *normal0 * *dist0;
                    let anchor1 = pnt - *normal1 * *dist1;
                    let dist = 0.5 * (mean.dot(pnt - anchor0) + mean.dot(pnt - anchor1));
                    pnt -= mean * dist;
                }
            }
            moved.push((i, j, pnt));
        }
    }

    // write the moved points once every plane was read from the unmoved net
    for (i, j, pnt) in moved {
        patch.set_control_point(ControlPointForm::NonRational, i, j, pnt, 1.0)?;
    }
    Ok(())
}

/// The fork's `sm_MakeCoonsPatchTangentToSides`: a Coons corner patch tangent to the
/// surfaces along its sides, to within `SM_CONTINUITY_ANGLE` (or `tangency_tol_radians`
/// where smaller). A bicubic Coons patch meets its cross-boundary derivative curves
/// only where their twists agree at the corners; where two rounds of different
/// curvature meet, `sm_CreateCoonsSrf` takes the mean twist and the patch leaves both
/// rounds at an angle. A copy of the patch is refined to 8, 16, 32 and 64 spans each
/// way and the control points next to each side moved into the side's tangent planes,
/// until every side reads within tolerance. Returns the last copy taken, `None` when
/// `patch` is rational, within tolerance already, or no copy is taken.
///
/// Not the fork: the fork takes a copy when its largest angle (over all sides, each
/// copy at its own stations) is strictly smaller than the held one's. Refinement leaves
/// the corner normals where they are (they are set by the shared loop curves), so where
/// the largest angle sits at a corner every copy reads the same angle up to rounding,
/// and the fork's choice was decided by about `1e-14` of it (WR-713: analytic028 kept
/// the bare bicubic patch). Here every candidate is measured at the same stations
/// ([`coons_common_stations`]) with a rounding bound at each ([`coons_side_angle`]), and
/// is taken only where it is shown no worse, side by side, than the bare patch and
/// every copy taken before it ([`SideAngle::no_worse_than`]): on each side its largest
/// angle's enclosure must lie below the held copy's lower end plus [`COONS_RESOLUTION`],
/// over the stations the held copy certifies. So
/// - a tie within the resolution goes to the finer copy, the refinement's intent;
/// - a side certainly worse vetoes the copy even where the largest angle over all sides
///   (at the stations the held copy certifies) improves (WR-713: `vase_rev_corner_blend_3`'s bottom side, 1.3896 radians on the
///   bare patch against 1.5708 on every refined copy), save one exchange: a side
///   certainly worse by at most the resolution does not veto a copy whose largest angle
///   at the stations the held copy certifies is certainly lower by more than the
///   resolution (`blend_crossing`'s
///   crease corner: 29 degrees off on the bare patch, 0.4 on its refined copies, which
///   give up 0.4 degrees along a side the bare patch fits exactly);
/// - a comparison the bounds leave undecided, or a station the held copy certifies and
///   the candidate does not, does not take the copy (WR-713: `Endcap_corner_blend_3`,
///   whose sides read to a radian next to its collapsed corners);
/// - comparing with every held copy, not only the last, keeps a chain of ties from
///   drifting: the copy returned is within the resolution of the bare patch on every
///   side.
///
/// G1 is undefined where the normal is, so only certified stations count, and no corner
/// stations are placed next to a collapsed corner ([`coons_corner_stations`]). The choice
/// depends only on the computed values, so it is deterministic.
fn make_coons_patch_tangent_to_sides(
    k: &crate::kernel::Kernel,
    patch: &BSplineSurface,
    uv_curves: &[Option<BSplineCurve>],
    surfaces: &[Option<&SmSurface>],
    tangency_tol_radians: f64,
) -> Result<Option<BSplineSurface>> {
    if patch.is_rational() {
        return Ok(None);
    }
    // a seam SMLib calls tangent (SmEdgeuse::IsTangentSector) is within
    // SM_CONTINUITY_ANGLE
    let mut target = ANG_TOL_DEG * (PI / 180.0);
    if tangency_tol_radians < target {
        target = tangency_tol_radians;
    }
    let logging = k
        .coons_tangency_log
        .try_borrow()
        .is_ok_and(|log| log.is_some());
    let mut candidates: Vec<CoonsCandidateLog> = Vec::new();
    let mut chosen = None;
    let record = |spans: usize, attempt: &BSplineSurface, stations| CoonsCandidateLog {
        spans,
        patch: attempt.clone(),
        stations,
    };
    let frames = coons_side_frames(k, patch, uv_curves, surfaces)?;
    let mut stations = Vec::new();
    let bare = coons_side_angle(patch, &frames, logging.then_some(&mut stations))?;
    if logging {
        candidates.push(record(0, patch, std::mem::take(&mut stations)));
    }
    // the bare patch and every copy taken, each a bar the next candidate must clear
    let mut held: Vec<SideAngle> = vec![bare];
    let mut best = None;
    for spans in COONS_REFINEMENTS {
        let above = held
            .last()
            .and_then(SideAngle::sup)
            .is_some_and(|sup| sup > target);
        if !above {
            break;
        }
        let mut attempt = patch.clone();
        refine_to_spans(&mut attempt, SurfParamType::U, spans)?;
        refine_to_spans(&mut attempt, SurfParamType::V, spans)?;
        project_coons_side_rows(k, &mut attempt, uv_curves, surfaces, 0)?;
        let angle = coons_side_angle(&attempt, &frames, logging.then_some(&mut stations))?;
        if logging {
            candidates.push(record(spans, &attempt, std::mem::take(&mut stations)));
        }
        if coons_takes(&held, &angle) {
            held.push(angle);
            best = Some(attempt);
            chosen = logging.then(|| candidates.len().saturating_sub(1));
        }
    }
    if logging {
        if let Ok(mut log) = k.coons_tangency_log.try_borrow_mut() {
            if let Some(log) = log.as_mut() {
                log.push(CoonsTangencyLog {
                    target,
                    resolution: COONS_RESOLUTION,
                    candidates,
                    chosen,
                });
            }
        }
    }
    Ok(best)
}

/// A Coons corner patch's sides (left, bottom, right, top): each side's 3D curve, UV curve
/// and surface, and which sides lie on faces of the body (bit `i` for side `i`; WR-613).
struct CoonsSides<'c, 's> {
    /// Each side's 3D curve as `CreateCornerBlend` was given it: the curve the body's edge
    /// at that side runs along (not its non-rational approximation the patch interpolates).
    curves: &'c [SmCurve],
    /// Each side's UV curve on its surface.
    uv_curves: &'c [Option<BSplineCurve>],
    /// Each side's surface.
    surfaces: &'c [Option<&'s SmSurface>],
    /// The sides on faces of the body.
    face_sides: u32,
}

/// The most times [`face_sided_patch`] halves a candidate's first knot span toward a
/// corner between a face side and a blend side ([`grade_corner_spans`]). With the blend
/// side's twist at that corner the blend side's extra angle sits in that span, so each
/// halving confines it to half the side length; three take a 64-span patch's corner span
/// to 1/512 of the side.
const MAX_CORNER_GRADE: u32 = 3;

/// Not the fork's (WR-613): the most piece bounds one side's certification
/// ([`coons_side_within`]) computes: 128 per starting piece ([`WITHIN_PIECES`]). A side
/// whose normals turn with its neighbour's is bounded in long pieces by the along-seam
/// bound (fact 4 of [`crate::query::seam_bound`]); the budget is for sides that only close
/// in short pieces, such as near a collapsed corner. Past it the side is not certified, so
/// the candidate is refused and the shipped patch kept: the budget bounds the work, never
/// what is accepted.
///
/// [`WITHIN_PIECES`]: crate::query::seam_bound::WITHIN_PIECES
const CERTIFY_PIECES: usize = 2048;

/// Not the fork's (WR-613): a corner patch that meets every face side exactly (`twisted`,
/// built with the blend side's twist at each corner between a face side and a blend side)
/// and is proved, on every side with a surface, within the largest angle `bar` is proved
/// to reach ([`seam_limit`], capped at `target`); `None` when no candidate is, and the
/// caller keeps `bar`. `bar` is the patch that ships otherwise: the mean-twist patch as
/// WR-547 leaves it.
///
/// Candidates, in order: `twisted` as built; then `twisted` refined to 8, 16, 32 and 64
/// spans each way, each with 0 to [`MAX_CORNER_GRADE`] halvings of the first span along
/// each blend side toward each face-side/blend-side corner, its side rows projected with
/// the face sides ([`project_coons_side_rows`]). The first that passes the measured screen
/// ([`read_within`]: each side's seam reading, [`coons_side_seams`], through
/// [`seam_normal_angle`], the seam check's own function, stations and metric) and the
/// proof ([`certified_within`]: on every side with a surface, its largest normal angle over
/// the whole side, not only at the stations, is proved at most the limit,
/// [`seam_angle_within`]) is taken. At most 1 + 4 x 4 candidates (the twisted patch, then
/// four refinements by four gradings): bounded work.
///
/// The rule is on the largest angle over all sides, so the error is not moved: a side of
/// the candidate may read higher than `bar`'s same side, but never above what `bar` is
/// proved to reach on its worst side.
///
/// [`seam_angle_within`]: crate::query::seam_bound::seam_angle_within
/// [`seam_normal_angle`]: crate::query::continuity::seam_normal_angle
fn face_sided_patch(
    k: &crate::kernel::Kernel,
    twisted: &BSplineSurface,
    bar: &BSplineSurface,
    sides: &CoonsSides<'_, '_>,
    target: f64,
) -> Result<Option<BSplineSurface>> {
    let CoonsSides {
        uv_curves,
        surfaces,
        face_sides,
        ..
    } = *sides;
    if twisted.is_rational() || bar.is_rational() {
        return Ok(None);
    }
    let bar_lower = coons_side_lower_bounds(k, bar, sides)?;
    let surfaced = std::array::from_fn(|i| matches!(surfaces.get(i), Some(Some(_))));
    let Some(limit) = seam_limit(target, &bar_lower, surfaced) else {
        return Ok(None);
    };
    let taken = |cand: &BSplineSurface| -> Result<bool> {
        let cand_seams = coons_side_seams(k, cand, sides, limit)?;
        if !read_within(&cand_seams, limit) {
            return Ok(false);
        }
        certified_within(k, cand, &cand_seams, limit, sides, CERTIFY_PIECES)
    };
    if taken(twisted)? {
        return Ok(Some(twisted.clone()));
    }
    for spans in COONS_REFINEMENTS {
        for grade in 0..=MAX_CORNER_GRADE {
            let mut attempt = twisted.clone();
            refine_to_spans(&mut attempt, SurfParamType::U, spans)?;
            refine_to_spans(&mut attempt, SurfParamType::V, spans)?;
            grade_corner_spans(&mut attempt, face_sides, grade)?;
            project_coons_side_rows(k, &mut attempt, uv_curves, surfaces, face_sides)?;
            if taken(&attempt)? {
                return Ok(Some(attempt));
            }
        }
    }
    Ok(None)
}

/// Not the fork's (WR-613): the bound a candidate corner patch is held to on every side
/// ([`face_sided_patch`]): `target` (the angle the seam check calls tangent), or less where
/// the patch that ships otherwise is proved better than that on all its sides: the largest
/// of its proved lower bounds (`bar_lower`) over the sides with a surface. That patch's
/// largest angle along its sides is at least that bound, so a candidate proved within it
/// on every side has no larger largest angle (the error is not moved from one side to
/// another past what ships). `None` where the bound is not positive (nothing proved of
/// that patch, or a NaN).
/// `surfaced[i]`: side `i` has a surface.
fn seam_limit(target: f64, bar_lower: &[Option<f64>; 4], surfaced: [bool; 4]) -> Option<f64> {
    let mut most = 0.0_f64;
    for (lower, on) in bar_lower.iter().zip(surfaced) {
        if let (Some(lower), true) = (*lower, on) {
            most = most.max(lower);
        }
    }
    let limit = target.min(most);
    (limit > 0.0).then_some(limit)
}

/// A side's seam reading in [`coons_side_seams`]: no surface on that side, or the reading.
#[derive(Clone, Copy, Debug, PartialEq)]
enum SideSeam {
    /// The side has no surface: nothing to meet.
    Free,
    /// [`seam_normal_angle`](crate::query::continuity::seam_normal_angle) along the side;
    /// `None` where it reads nothing (too few stations, or an angle not finite).
    Read(Option<f64>),
}

/// Not the fork's (WR-613): each side's seam reading of `patch` (left, bottom, right,
/// top): [`seam_normal_angle`](crate::query::continuity::seam_normal_angle) between the
/// patch and the side's surface along the side's 3D curve as given ([`CoonsSides::curves`],
/// the curve the body's edge runs along) over its natural interval, both in their surfaces'
/// own senses (the reading folds the senses out). Along the non-rational approximation the
/// patch interpolates instead, the reading misses a rise the body's edge shows (#18640:
/// `corner3x2` read 5.92e-3 on both patches where the body's edge reads main's 3.836e-3
/// and the candidate's 3.2e-7 above it). A
/// reading that fails with an error other than an unported call or a cancellation reads
/// nothing.
///
/// The sides after the first whose reading [`read_within`] refuses at `limit` are not
/// read (left [`SideSeam::Free`]): the screen's answer is already no, and each reading is
/// a nearest-point solve at every station.
fn coons_side_seams(
    k: &crate::kernel::Kernel,
    patch: &BSplineSurface,
    sides: &CoonsSides<'_, '_>,
    limit: f64,
) -> Result<[SideSeam; 4]> {
    let patch = SmSurface::from(patch.clone());
    let mut out = [SideSeam::Free; 4];
    for (i, slot) in out.iter_mut().enumerate() {
        let (Some(Some(surface)), Some(curve)) = (sides.surfaces.get(i), sides.curves.get(i))
        else {
            continue;
        };
        let read = crate::query::continuity::seam_normal_angle(
            k,
            curve,
            curve.natural_interval(),
            [(&patch, 1.0), (surface, 1.0)],
        );
        let seam = SideSeam::Read(match read {
            Ok(angle) => angle,
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) => None,
        });
        *slot = seam;
        if !read_within(
            &[seam, SideSeam::Free, SideSeam::Free, SideSeam::Free],
            limit,
        ) {
            break;
        }
    }
    Ok(out)
}

/// Not the fork's (WR-613): whether `cand` reads every side with a surface at or below
/// `limit` ([`coons_side_seams`]). A reading is the angle at a station, so a reading above
/// `limit` (or none, or a NaN) refuses `cand` without trying a proof: no upper bound of
/// the largest angle could then be at most `limit`.
fn read_within(cand: &[SideSeam; 4], limit: f64) -> bool {
    cand.iter().all(|c| match c {
        SideSeam::Free => true,
        SideSeam::Read(Some(c)) => *c <= limit,
        SideSeam::Read(None) => false,
    })
}

/// Not the fork's (WR-613): whether the largest normal angle between `patch` and side
/// `i`'s surface along side `i`'s 3D curve over its natural interval is proved at most
/// `limit` ([`seam_angle_within`], at most `budget` piece bounds: [`CERTIFY_PIECES`] in
/// [`face_sided_patch`]); `false` on a free side, a side whose curve is no B-spline, or
/// where it is not proved within the budget.
///
/// [`seam_angle_within`]: crate::query::seam_bound::seam_angle_within
fn coons_side_within(
    k: &crate::kernel::Kernel,
    patch: &SmSurface,
    sides: &CoonsSides<'_, '_>,
    i: usize,
    limit: f64,
    budget: usize,
) -> Result<bool> {
    let (Some(Some(surface)), Some(curve)) = (sides.surfaces.get(i), sides.curves.get(i)) else {
        return Ok(false);
    };
    let Some(bs) = crate::query::seam_bound::curve_b_spline(curve) else {
        return Ok(false);
    };
    crate::query::seam_bound::seam_angle_within(
        k,
        bs,
        curve.natural_interval(),
        [patch, surface],
        limit,
        budget,
    )
}

/// Not the fork's (WR-613): on each side with a surface, a proved lower bound of the
/// largest normal angle between `patch` and the side's surface along the side's 3D curve
/// over its natural interval ([`seam_angle_lower_bound`]); `None` on a free side, a side
/// whose curve is no B-spline, or where nothing is proved.
///
/// [`seam_angle_lower_bound`]: crate::query::seam_bound::seam_angle_lower_bound
fn coons_side_lower_bounds(
    k: &crate::kernel::Kernel,
    patch: &BSplineSurface,
    sides: &CoonsSides<'_, '_>,
) -> Result<[Option<f64>; 4]> {
    let patch = SmSurface::from(patch.clone());
    let mut out = [None; 4];
    for (i, slot) in out.iter_mut().enumerate() {
        let (Some(Some(surface)), Some(curve)) = (sides.surfaces.get(i), sides.curves.get(i))
        else {
            continue;
        };
        let Some(bs) = crate::query::seam_bound::curve_b_spline(curve) else {
            continue;
        };
        *slot = crate::query::seam_bound::seam_angle_lower_bound(
            k,
            bs,
            curve.natural_interval(),
            [&patch, surface],
        )?;
    }
    Ok(out)
}

/// Not the fork's (WR-613): whether `cand` is proved within `limit` ([`seam_limit`]) on
/// every side with a surface: its proved upper bound of the largest normal angle there
/// over the whole side ([`coons_side_within`], at most `budget` piece bounds a side) is
/// at most `limit`. Where none is proved on such a side, the budget included, `cand` is
/// refused. The side reading highest (`cand_seams`) is
/// tried first, the likeliest to fail.
fn certified_within(
    k: &crate::kernel::Kernel,
    cand: &BSplineSurface,
    cand_seams: &[SideSeam; 4],
    limit: f64,
    sides: &CoonsSides<'_, '_>,
    budget: usize,
) -> Result<bool> {
    let mut order: Vec<(usize, f64)> = Vec::new();
    for (i, seam) in cand_seams.iter().enumerate() {
        if !matches!(sides.surfaces.get(i), Some(Some(_))) {
            continue;
        }
        match seam {
            SideSeam::Read(Some(r)) if *r <= limit => order.push((i, *r)),
            _ => return Ok(false),
        }
    }
    order.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let patch = SmSurface::from(cand.clone());
    for (i, _) in order {
        if !coons_side_within(k, &patch, sides, i, limit, budget)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Not the fork's (WR-613): knots inserted in `surface` that halve its first knot span
/// toward each corner between a face side and a blend side, `grade` times, in the
/// direction the blend side runs (the first span at that end becomes `1 / 2^grade` of
/// what it was). Sides are left, bottom, right, top; `face_sides` has bit `i` for side
/// `i`. The surface's shape is unchanged.
fn grade_corner_spans(surface: &mut BSplineSurface, face_sides: u32, grade: u32) -> Result<()> {
    if grade == 0 {
        return Ok(());
    }
    let on_face = |side: usize| face_sides & (1 << side) != 0;
    // the knots halving the first span at one end of a direction, `grade` times
    let halvings = |param: SurfParamType, at_min: bool| -> Result<Vec<f64>> {
        let (knots, _) = surface.knots(param, None)?;
        let ends = if at_min {
            knots.first().zip(knots.get(1))
        } else {
            knots
                .last()
                .zip(knots.len().checked_sub(2).and_then(|i| knots.get(i)))
        };
        let Some((&end, &next)) = ends else {
            return Err(SolidError::b_spline(
                OP_CORNER,
                BSplineFailure::PatchWithoutSpan,
            ));
        };
        let mut out = Vec::new();
        let mut piece = next - end;
        for _ in 0..grade {
            piece *= 0.5;
            out.push(end + piece);
        }
        Ok(out)
    };
    let (mut in_u, mut in_v) = (Vec::new(), Vec::new());
    // each corner: (its side at constant u, its side at constant v)
    for (side_u, side_v) in [(LEFT, BOT), (RIGHT, BOT), (RIGHT, TOP), (LEFT, TOP)] {
        match (on_face(side_u), on_face(side_v)) {
            // the blend side runs along u: halve the u span at the face side's end
            (true, false) => in_u.extend(halvings(SurfParamType::U, side_u == LEFT)?),
            // the blend side runs along v: halve the v span at the face side's end
            (false, true) => in_v.extend(halvings(SurfParamType::V, side_v == BOT)?),
            _ => {}
        }
    }
    for (param, mut knots) in [(SurfParamType::U, in_u), (SurfParamType::V, in_v)] {
        knots.sort_by(f64::total_cmp);
        knots.dedup();
        if !knots.is_empty() {
            surface.insert_knots(param, &knots)?;
        }
    }
    Ok(())
}

// ---- CreateCornerBlend: n curves --------------------------------------------------------

/// The n-sided branch of `CreateCornerBlend`.
fn corner_blend_n(
    k: &crate::kernel::Kernel,
    mut boundaries: Vec<BSplineCurve>,
    mut uv_crvs: Vec<Option<BSplineCurve>>,
    curve_orients: &[OrientType],
    surfaces: &[Option<&SmSurface>],
    approx_tol: f64,
    tangency_tol_radians: f64,
) -> Result<Vec<BSplineSurface>> {
    const OP: &str = OP_CORNER;
    let missing = || SolidError::invalid_input(OP, "missing UV curve or surface");
    let oob = || SolidError::invalid_input(OP, "curve index out of range");
    let mut negate_curr_normal = false;
    let mut negate_tan_curves: Option<bool> = None; // UNSURE

    // for every boundary - make all curves SM_OT_SAME
    for (ii, &orient) in curve_orients.iter().enumerate() {
        if orient == OrientType::Opposite {
            let b = boundaries.get_mut(ii).ok_or_else(oob)?;
            let ivl1 = b.natural_interval();
            b.reverse_parameterization(&ivl1)?;

            if let Some(uv) = uv_crvs.get_mut(ii).and_then(Option::as_mut) {
                let ivl2 = uv.natural_interval();
                uv.reverse_parameterization(&ivl2)?;
            }
        }
    }

    // Now Create CrossTangent Curves
    let cnt = boundaries.len();
    let mut tan_curves: Vec<BSplineCurve> = Vec::with_capacity(cnt);
    for ii in 0..cnt {
        let prev_i = if ii == 0 { cnt - 1 } else { ii - 1 };
        let next_i = if ii == cnt - 1 { 0 } else { ii + 1 };

        let curr = boundaries.get(ii).ok_or_else(oob)?;
        let prev = boundaries.get(prev_i).ok_or_else(oob)?;
        let next = boundaries.get(next_i).ok_or_else(oob)?;

        let uv_of = |i: usize| uv_crvs.get(i).and_then(Option::as_ref).ok_or_else(missing);
        let srf_of = |i: usize| -> Result<&SmSurface> { at(surfaces, i, OP)?.ok_or_else(missing) };
        let curr_uv = uv_of(ii)?;
        let prev_uv = uv_of(prev_i)?;
        let next_uv = uv_of(next_i)?;
        let curr_srf = srf_of(ii)?;
        let prev_srf = srf_of(prev_i)?;
        let next_srf = srf_of(next_i)?;

        let prev_end_param = prev_uv.natural_interval().max();
        let curr_start_param = curr_uv.natural_interval().min();
        let curr_end_param = curr_uv.natural_interval().max();
        let next_start_param = next_uv.natural_interval().min();

        // 3d Boundary end Curve Points and tangents (statuses ignored upstream)
        let undef = || vec![Vec3::UNDEF; 2];
        let prev_end_pv = prev
            .evaluate(prev_end_param, 1, true, false)
            .unwrap_or_else(|_| undef());
        let curr_start_pv = curr
            .evaluate(curr_start_param, 1, true, false)
            .unwrap_or_else(|_| undef());
        let curr_end_pv = curr
            .evaluate(curr_end_param, 1, true, false)
            .unwrap_or_else(|_| undef());
        let next_start_pv = next
            .evaluate(next_start_param, 1, true, false)
            .unwrap_or_else(|_| undef());

        // 2d Boundary end UV Points
        let uv2 = |c: &BSplineCurve, t: f64| {
            let p = c.evaluate_point(t).unwrap_or(Vec3::UNDEF);
            Point2::new(p.x, p.y)
        };
        let prev_end_uv = uv2(prev_uv, prev_end_param);
        let curr_start_uv = uv2(curr_uv, curr_start_param);
        let curr_end_uv = uv2(curr_uv, curr_end_param);
        let next_start_uv = uv2(next_uv, next_start_param);

        // Surface Normals at boundary endPoints
        let mut prev_end_normal = evaluate_normal(k, prev_srf, prev_end_uv)?;
        let mut curr_start_normal = evaluate_normal(k, curr_srf, curr_start_uv)?;
        let mut curr_end_normal = evaluate_normal(k, curr_srf, curr_end_uv)?;
        let mut next_start_normal = evaluate_normal(k, next_srf, next_start_uv)?;

        // compare the neighbor and current surface normal directions
        let prev_dot_curr_normal = prev_end_normal.dot(curr_start_normal);
        let next_dot_curr_normal = next_start_normal.dot(curr_end_normal);

        // remember the neighbor to curr normal relationships
        let prev_normal_same = prev_dot_curr_normal > 0.0;
        let next_normal_same = next_dot_curr_normal > 0.0;

        // flip surface normals as needed
        if (negate_curr_normal && prev_normal_same) || (!negate_curr_normal && !prev_normal_same) {
            prev_end_normal = -prev_end_normal;
        }
        if negate_curr_normal {
            curr_start_normal = -curr_start_normal;
        }
        if negate_curr_normal {
            curr_end_normal = -curr_end_normal;
        }
        if (negate_curr_normal && next_normal_same) || (!negate_curr_normal && !next_normal_same) {
            next_start_normal = -next_start_normal;
        }

        // get ready for next iteration
        negate_curr_normal =
            (negate_curr_normal && next_normal_same) || (!negate_curr_normal && !next_normal_same);

        let pv1 = |pv: &[Vec3]| pv.get(1).copied().unwrap_or(Vec3::UNDEF);
        let pv0 = |pv: &[Vec3]| pv.first().copied().unwrap_or(Vec3::UNDEF);

        // Let CrossVec be the direction out of the new patch
        let prev_end_cross_vec = prev_end_normal.cross(pv1(&prev_end_pv));
        let curr_start_cross_vec = curr_start_normal.cross(pv1(&curr_start_pv));
        let curr_end_cross_vec = curr_end_normal.cross(pv1(&curr_end_pv));
        let next_start_cross_vec = next_start_normal.cross(pv1(&next_start_pv));

        // see if the first assumption about surface normals can be corrected by a
        // boundary surface case
        if negate_tan_curves.is_none() {
            // classify the ends of the curve against the surface UVDomain
            let curr_uv_domain = curr_srf.natural_uv_domain_with_nurb(k)?;
            let tol_uv = curr_uv_domain.size() / 1000.0;
            let tol = min2(tol_uv.x, tol_uv.y);
            let start_bndries = curr_uv_domain.point_boundaries(curr_start_uv, tol);
            let end_bndries = curr_uv_domain.point_boundaries(curr_end_uv, tol);
            let bndries = start_bndries & end_bndries;

            // when pCurr starts and stop on a common boundary - check orientation
            if bndries != 0 {
                // Surface 1st derivs at CurveStart
                let (srf_du, srf_dv) = evaluate_1st_derivatives(k, curr_srf, curr_start_uv)?;

                // pick the appropriate vector and make it point out of the patch
                let outward = if bndries & SS_UMIN != 0 {
                    srf_du
                } else if bndries & SS_VMIN != 0 {
                    srf_dv
                } else if bndries & SS_UMAX != 0 {
                    -srf_du
                } else {
                    -srf_dv
                };

                let cross_vec_dot_outward = curr_start_cross_vec.dot(outward);
                negate_tan_curves = Some(cross_vec_dot_outward < 0.0);
            }
        }

        // Surface CrossTangents at boundary endPoints
        let prev_end_cross_tang =
            evaluate_normal_section(k, prev_srf, prev_end_uv, prev_end_cross_vec)?;
        let curr_start_cross_tang =
            evaluate_normal_section(k, curr_srf, curr_start_uv, curr_start_cross_vec)?;
        let curr_end_cross_tang =
            evaluate_normal_section(k, curr_srf, curr_end_uv, curr_end_cross_vec)?;
        let next_start_cross_tang =
            evaluate_normal_section(k, next_srf, next_start_uv, next_start_cross_vec)?;

        // check boundary curve gap sizes
        let dist_prev = pv0(&prev_end_pv).distance(pv0(&curr_start_pv));
        let dist_next = pv0(&curr_end_pv).distance(pv0(&next_start_pv));
        if dist_prev > approx_tol || dist_next > approx_tol {
            return Err(SolidError::b_spline(
                OP,
                BSplineFailure::EndsApart {
                    distance: max2(dist_prev, dist_next),
                    tolerance: approx_tol,
                },
            )); // Ends don't match
        }

        // get the desired tangentField endTangent vecs
        let start_tf = (prev_end_cross_tang + curr_start_cross_tang) / 2.0;
        let end_tf = (curr_end_cross_tang + next_start_cross_tang) / 2.0;

        // use pCurr knot vector and build a curve approximation to the tangent field
        let knots = unique_knots(curr);
        let tan_curve = interpolated_tangent_field_curve(
            k,
            curr_uv,
            curr_srf,
            start_tf,
            end_tf,
            &knots,
            tangency_tol_radians,
        )?;
        tan_curves.push(tan_curve);
    }

    // When we've figured out that the crossTangent vectors need to be negated
    if negate_tan_curves == Some(true) {
        for tan in &mut tan_curves {
            let _ = tan.scale(-1.0);
        }
    }

    // pass the call along to create the NSidedPatch
    let mut ders: Vec<Option<BSplineCurve>> = tan_curves.into_iter().map(Some).collect();
    BSplineSurface::create_n_sided_patch(&mut boundaries, &mut ders, tangency_tol_radians, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nurbs::math::{CPoint, NOW};

    /// A bilinear-ish patch over `[0, 1]^2` of degree `(p, q)`.
    fn plane(p: usize, q: usize, z: impl Fn(usize, usize) -> f64) -> BSplineSurface {
        let n = p;
        let m = q;
        let net: Vec<Vec<CPoint>> = (0..=n)
            .map(|i| {
                (0..=m)
                    .map(|j| CPoint::new(i as f64 / n as f64, j as f64 / m as f64, z(i, j), NOW))
                    .collect()
            })
            .collect();
        let mut u = vec![0.0; p + 1];
        u.extend(vec![1.0; p + 1]);
        let mut v = vec![0.0; q + 1];
        v.extend(vec![1.0; q + 1]);
        let s = Surface::new(net, p, q, u, v).unwrap();
        BSplineSurface::from_nurb(s, false)
    }

    #[test]
    fn evaluate_matches_simple_without_zero_tangents() {
        let s = plane(2, 2, |i, j| (i * j) as f64 * 0.3);
        let uv = Point2::new(0.3, 0.6);
        let a = s.evaluate(uv, 2, 2, EvaluateFlags::default()).unwrap();
        let b = s.evaluate_simple(uv, 2, 2, true, true, false).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn evaluate_rejects_bad_orders() {
        let s = plane(1, 1, |_, _| 0.0);
        let uv = Point2::new(0.5, 0.5);
        let flags = EvaluateFlags {
            only_upper_half: true,
            ..EvaluateFlags::default()
        };
        assert!(matches!(
            s.evaluate(uv, 1, 2, flags),
            Err(SolidError::InvalidInput { .. })
        ));
        assert!(matches!(
            s.evaluate(uv, 5, 0, EvaluateFlags::default()),
            Err(SolidError::InvalidInput { .. })
        ));
    }

    #[test]
    fn evaluate_fixes_degenerate_side() {
        // the u = 0 row collapses to a point: Sv = 0 there
        let mut s = plane(2, 2, |_, _| 0.0);
        if let Some(sur) = s.nurb.as_mut() {
            for j in 0..=2 {
                *sur.cpt_mut(0, j).unwrap() = CPoint::new(0.0, 0.5, 0.0, NOW);
            }
        }
        let uv = Point2::new(0.0, 0.5);
        let d = s.evaluate(uv, 1, 1, EvaluateFlags::default()).unwrap();
        assert!(d[0][1].length() > 0.0);
        let raw = s.evaluate_simple(uv, 1, 1, true, true, false).unwrap();
        assert_eq!(raw[0][1].length(), 0.0);
    }

    #[test]
    fn trim_with_domain_extracts_patch() {
        let mut s = plane(2, 3, |i, j| (i + j) as f64 * 0.1);
        let mut dom = Extent2d::from_bounds(0.25, 0.1, 0.75, 0.9);
        s.trim_with_domain(&mut dom).unwrap();
        let nd = s.natural_uv_domain();
        assert!((nd.min().x - 0.25).abs() < 1e-12);
        assert!((nd.max().y - 0.9).abs() < 1e-12);
        // outside the domain fails
        let mut bad = Extent2d::from_bounds(-1.0, 0.0, 0.5, 0.5);
        let e = s.trim_with_domain(&mut bad).unwrap_err();
        assert_b_spline(&e, BSplineFailure::TrimDomainOutside, crate::error::SM_ERR);
    }

    /// A cubic-by-linear surface with u knots `u` (clamped) and v knots `v`, its
    /// control points on z = 0.
    fn net_with_knots(u: &[f64], v: &[f64]) -> BSplineSurface {
        let (p, q) = (3, 1);
        let nu = u.len() - p - 1;
        let nv = v.len() - q - 1;
        let net: Vec<Vec<CPoint>> = (0..nu)
            .map(|i| {
                (0..nv)
                    .map(|j| CPoint::new(i as f64, j as f64, 0.0, NOW))
                    .collect()
            })
            .collect();
        let s = Surface::new(net, p, q, u.to_vec(), v.to_vec()).unwrap();
        BSplineSurface::from_nurb(s, false)
    }

    fn u_knots(s: &BSplineSurface) -> Vec<f64> {
        s.knots(SurfParamType::U, None).unwrap().0
    }

    fn v_knots(s: &BSplineSurface) -> Vec<f64> {
        s.knots(SurfParamType::V, None).unwrap().0
    }

    /// tangency-labs/solidcore#472: the round's u knots and face domains of the
    /// (+x,-y) and (-x,+y) rounds of a drafted box (their domains differ by one ULP).
    #[test]
    fn trim_one_ulp_inside_a_knot_leaves_a_sliver_upstream_and_none_with_snapping() {
        let k = 20.152507010543033_f64;
        let (k2, end) = (60.45751479, 80.61001657);
        let u = [0.0, 0.0, 0.0, 0.0, k, k, k, k2, k2, k2, end, end, end, end];
        let v = [0.0, 0.0, 0.5, 1.0, 1.0];
        let below = f64::from_bits(k.to_bits() - 1);
        assert!(below < k && k - below < 4e-15);
        let ur = 60.601175783809218;

        // on the knot: no sliver
        let mut s = net_with_knots(&u, &v);
        let mut dom = Extent2d::from_bounds(k, 0.0, ur, 1.0);
        s.trim_with_domain(&mut dom).unwrap();
        assert_eq!(u_knots(&s), vec![k, k2, ur]);
        assert_eq!(dom.min().x, k);

        // one ULP inside the knot: upstream moves the boundary 10 * 1e-6 * (1 + u) away
        let mut s = net_with_knots(&u, &v);
        let mut dom = Extent2d::from_bounds(below, 0.0, ur, 1.0);
        s.trim_with_domain(&mut dom).unwrap();
        let sliver = k - 10.0 * EFF_ZERO_SQRT * (1.0 + below.abs());
        assert_eq!(u_knots(&s), vec![sliver, k, k2, ur]);
        assert!((k - sliver - 2.115e-4).abs() < 1e-6);

        // with snapping: the boundary goes onto the knot, as for the face on it
        let snap = TrimOptions {
            snap_roundoff_to_knots: true,
        };
        let mut s = net_with_knots(&u, &v);
        let mut dom = Extent2d::from_bounds(below, 0.0, ur, 1.0);
        s.trim_with_domain_with(&mut dom, snap).unwrap();
        assert_eq!(u_knots(&s), vec![k, k2, ur]);
        assert_eq!(dom.min().x, k);
        assert_eq!(s.natural_uv_domain().min().x, k);
    }

    #[test]
    fn snapping_covers_upper_and_v_boundaries() {
        let u = [0.0, 0.0, 0.0, 0.0, 2.0, 2.0, 2.0, 5.0, 5.0, 5.0, 5.0];
        let v = [0.0, 0.0, 0.5, 1.0, 1.0];
        let snap = TrimOptions {
            snap_roundoff_to_knots: true,
        };
        let above = 2.0 + 1e-12;
        let v_above = 0.5 + 1e-13;

        // upstream: an upper boundary just outside a knot is moved further out
        let mut s = net_with_knots(&u, &v);
        let mut dom = Extent2d::from_bounds(0.5, 0.0, above, v_above);
        s.trim_with_domain(&mut dom).unwrap();
        assert_eq!(dom.max().x, 2.0 + 10.0 * EFF_ZERO_SQRT * (1.0 + above));
        assert_eq!(dom.max().y, 0.5 + 10.0 * EFF_ZERO_SQRT * (1.0 + v_above));
        assert_eq!(u_knots(&s).len(), 3);

        // with snapping both go onto their knots
        let mut s = net_with_knots(&u, &v);
        let mut dom = Extent2d::from_bounds(0.5, 0.0, above, v_above);
        s.trim_with_domain_with(&mut dom, snap).unwrap();
        assert_eq!((dom.max().x, dom.max().y), (2.0, 0.5));
        assert_eq!(u_knots(&s), vec![0.5, 2.0]);
        assert_eq!(v_knots(&s), vec![0.0, 0.5]);

        // lower v boundary just inside a knot
        let mut s = net_with_knots(&u, &v);
        let mut dom = Extent2d::from_bounds(0.5, 0.5 - 1e-13, 4.0, 1.0);
        s.trim_with_domain_with(&mut dom, snap).unwrap();
        assert_eq!(dom.min().y, 0.5);
        assert_eq!(v_knots(&s), vec![0.5, 1.0]);
    }

    #[test]
    fn snapping_leaves_boundaries_beyond_eff_zero_param_to_upstream() {
        let u = [0.0, 0.0, 0.0, 0.0, 2.0, 2.0, 2.0, 5.0, 5.0, 5.0, 5.0];
        let v = [0.0, 0.0, 1.0, 1.0];
        let snap = TrimOptions {
            snap_roundoff_to_knots: true,
        };
        for ul in [2.0 - 3e-8, 2.0 - 1e-6, 2.0 + 1e-9, 1.0] {
            let mut a = net_with_knots(&u, &v);
            let mut b = a.clone();
            let mut da = Extent2d::from_bounds(ul, 0.0, 4.0, 1.0);
            let mut db = da;
            a.trim_with_domain(&mut da).unwrap();
            b.trim_with_domain_with(&mut db, snap).unwrap();
            assert_eq!(da, db, "ul = {ul}");
            assert_eq!(u_knots(&a), u_knots(&b), "ul = {ul}");
        }
    }

    #[test]
    fn corner_blend_needs_four_curves() {
        let k = &crate::Kernel::default();
        let r = BSplineSurface::create_corner_blend(k, &[], &[], &[], &[], 1e-3, 0.1, true);
        assert_b_spline(
            &r.unwrap_err(),
            BSplineFailure::TooFewBoundaryCurves { given: 0 },
            crate::error::SM_ERR,
        );
    }

    /// `e` is the B-spline failure `cause` with `status`, of the class the cause names.
    fn assert_b_spline(e: &SolidError, cause: BSplineFailure, status: i32) {
        assert_eq!(
            e.nurbs_failure(),
            Some(&crate::error::nurbs::NurbsFailure::BSpline(cause)),
            "{e:?}"
        );
        assert_eq!(e.upstream_status(), Some(status), "{e:?}");
        assert_eq!(e.kind(), cause.kind(), "{e:?}");
    }

    /// WR-613: a corner control point next to a side on a face of the body and a blend
    /// side goes into the face side's tangent plane, so the patch keeps its tangency to
    /// the face exactly; next to two sides of one kind it goes onto their mean plane.
    #[test]
    fn wr_613_corner_point_goes_into_the_face_sides_plane() {
        use crate::geom::axis2_placement::Axis2Placement;
        use crate::surface::plane::Plane;

        let k = crate::Kernel::default();
        // a bicubic patch over [0, 1]^2, 5 x 5 control points, off both side planes
        let net: Vec<Vec<CPoint>> = (0..5)
            .map(|i| {
                (0..5)
                    .map(|j| {
                        let z = 0.01 * f64::from((i + 1) * (j + 2));
                        CPoint::new(f64::from(i) / 4.0, f64::from(j) / 4.0, z, NOW)
                    })
                    .collect()
            })
            .collect();
        let knots = vec![0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0];
        let patch = BSplineSurface::from_nurb(
            Surface::new(net, 3, 3, knots.clone(), knots).unwrap(),
            false,
        );
        // left side (u = 0) on the plane z = 0, bottom side (v = 0) on a plane tilted
        // about x, normal (0, -sin a, cos a)
        let a = 0.3_f64;
        let flat = SmSurface::from(Plane::create_canonical(&Axis2Placement::default()).unwrap());
        let tilted = SmSurface::from(
            Plane::create_canonical(&Axis2Placement::new(
                Point3::ZERO,
                Vec3::X,
                Vec3::new(0.0, a.cos(), a.sin()),
            ))
            .unwrap(),
        );
        let n_bot = Vec3::new(0.0, -a.sin(), a.cos());
        let uv = |x: f64| {
            BSplineCurve::create_line_segment(2, Point3::new(x, 0.0, 0.0), Point3::new(x, 1.0, 0.0))
                .unwrap()
        };
        let uv_curves = [Some(uv(0.0)), Some(uv(0.0)), None, None];
        let surfaces = [Some(&flat), Some(&tilted), None, None];

        let corner = |face_sides: u32| -> Point3 {
            let mut p = patch.clone();
            project_coons_side_rows(&k, &mut p, &uv_curves, &surfaces, face_sides).unwrap();
            p.control_point(ControlPointForm::NonRational, 1, 1)
                .unwrap()
                .0
        };
        let anchor = |i, j| {
            patch
                .control_point(ControlPointForm::NonRational, i, j)
                .unwrap()
                .0
        };
        let off_left = |p: Point3| (p - anchor(0, 1)).dot(Vec3::Z);
        let off_bot = |p: Point3| (p - anchor(1, 0)).dot(n_bot);

        // left on a face, bottom a blend side: in the left side's plane
        let left = corner(1 << LEFT);
        assert!(off_left(left).abs() <= 1e-15, "{}", off_left(left));
        assert!(off_bot(left).abs() > 1e-3, "{}", off_bot(left));
        // bottom on a face, left a blend side: in the bottom side's plane
        let bot = corner(1 << BOT);
        assert!(off_bot(bot).abs() <= 1e-15, "{}", off_bot(bot));
        assert!(off_left(bot).abs() > 1e-3, "{}", off_left(bot));
        // no face side, or both: the mean plane, off each side's plane (upstream's)
        let mean = corner(0);
        assert_eq!(corner((1 << LEFT) | (1 << BOT)), mean);
        assert!(off_left(mean).abs() > 1e-3, "{}", off_left(mean));
        assert!(off_bot(mean).abs() > 1e-3, "{}", off_bot(mean));
        // a point next to one side only goes into that side's plane either way
        for face_sides in [0, 1 << LEFT, 1 << BOT] {
            let mut p = patch.clone();
            project_coons_side_rows(&k, &mut p, &uv_curves, &surfaces, face_sides).unwrap();
            let q = p
                .control_point(ControlPointForm::NonRational, 1, 2)
                .unwrap()
                .0;
            assert!((q - anchor(0, 2)).dot(Vec3::Z).abs() <= 1e-15);
        }

        // WR-613's zero-sum transfer at the corner point. With `d_f`, `d_b` its signed
        // distances from the face side's and the blend side's planes and `c` the cosine
        // between their normals: the mean move keeps the gap `d_f - d_b` exactly (it moves
        // along the bisector, as far from each plane), and the face move changes it by
        // `(1 - c) d_f` exactly (it moves along the face normal: `d_b` drops by `c d_f`).
        // So the blend side's miss after the face move is at most both sides' misses on
        // the mean plane plus `(1 - c) |d_f|`: what the face side gains, the blend side
        // loses, to within `1 - cos` of the angle between the sides.
        let p = anchor(1, 1);
        let (d_f, d_b) = (off_left(p), off_bot(p));
        let c = Vec3::Z.dot(n_bot);
        // each distance is a dot product of a unit normal with a difference of points of
        // size at most 2 (error at most γ_3 of 2 + 2u), and each identity sums four of
        // them plus each move's own rounding (a product and a difference per coordinate):
        // 64 ε bounds the lot
        let rnd = 64.0 * f64::EPSILON;
        let gap = d_f - d_b;
        let mean_gap = off_left(mean) - off_bot(mean);
        assert!((mean_gap - gap).abs() <= rnd, "{mean_gap} vs {gap}");
        let face_gap = off_left(left) - off_bot(left);
        assert!(
            (face_gap - (gap - (1.0 - c) * d_f)).abs() <= rnd,
            "{face_gap} vs {gap} - (1 - {c}) {d_f}"
        );
        let rise = off_bot(left).abs() - off_bot(mean).abs();
        assert!(
            rise <= off_left(mean).abs() + (1.0 - c) * d_f.abs() + rnd,
            "the blend side's miss rose by {rise}, more than the face side's {} gave up",
            off_left(mean).abs()
        );
        // and the transfer is real: the face side's whole miss went to the blend side
        assert!(rise > 0.5 * off_left(mean).abs(), "{rise}");
    }

    /// WR-613: `grade_corner_spans` halves the first knot span toward each corner between
    /// a face side and a blend side, in the direction the blend side runs, `grade` times,
    /// and leaves the surface's shape alone.
    #[test]
    fn wr_613_corner_spans_are_graded_along_the_blend_side() {
        let net: Vec<Vec<CPoint>> = (0..5)
            .map(|i| {
                (0..5)
                    .map(|j| {
                        let z = 0.01 * f64::from((i + 1) * (j + 2));
                        CPoint::new(f64::from(i) / 4.0, f64::from(j) / 4.0, z, NOW)
                    })
                    .collect()
            })
            .collect();
        let knots = vec![0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0];
        let patch = BSplineSurface::from_nurb(
            Surface::new(net, 3, 3, knots.clone(), knots).unwrap(),
            false,
        );
        let graded = |face_sides: u32, grade: u32| {
            let mut p = patch.clone();
            grade_corner_spans(&mut p, face_sides, grade).unwrap();
            let u = p.knots(SurfParamType::U, None).unwrap().0;
            let v = p.knots(SurfParamType::V, None).unwrap().0;
            (p, u, v)
        };
        // left a face side, bottom and top blend sides (running along u): u's first span
        // halved twice
        let (p, u, v) = graded(1 << LEFT, 2);
        assert_eq!(u, [0.0, 0.125, 0.25, 0.5, 1.0]);
        assert_eq!(v, [0.0, 0.5, 1.0]);
        let k = crate::Kernel::default();
        let at = |q: &BSplineSurface, s: f64, t: f64| {
            crate::surface::surface::p1::evaluate_1st_derivatives(
                &k,
                q,
                Point2::new(s, t),
                Side::Left,
                Side::Left,
            )
            .unwrap()
            .0
        };
        for (s, t) in [(0.1, 0.2), (0.6, 0.9), (0.01, 0.99)] {
            let (a, b) = (at(&patch, s, t), at(&p, s, t));
            // knot insertion only re-expresses the surface: rounding on coordinates < 1
            assert!(a.distance(b) <= 1e-14, "{a:?} {b:?}");
        }
        // top a face side, left and right blend sides (running along v): v's last span
        // halved three times
        let (_, u, v) = graded(1 << TOP, 3);
        assert_eq!(u, [0.0, 0.5, 1.0]);
        assert_eq!(v, [0.0, 0.5, 0.75, 0.875, 0.9375, 1.0]);
        // no face side, every side one, or grade 0: no knot
        for (face_sides, grade) in [(0, 3), (0b1111, 3), (1 << LEFT, 0)] {
            let (_, u, v) = graded(face_sides, grade);
            assert_eq!((u.len(), v.len()), (3, 3));
        }
        // a face side and its blend sides at both ends: both ends graded, each once
        let (_, u, v) = graded((1 << BOT) | (1 << TOP), 1);
        assert_eq!(u, [0.0, 0.5, 1.0]);
        assert_eq!(v, [0.0, 0.25, 0.5, 0.75, 1.0]);
    }

    /// WR-613: the limit a candidate is held to is the largest proved lower bound of the
    /// shipped patch's angle over its sides with a surface, capped at the target
    /// ([`seam_limit`]; none where nothing is proved, or a NaN); the screen
    /// ([`read_within`]) passes a candidate only where every side with a surface reads at
    /// or below that limit, so one side may rise while the largest angle does not, and a
    /// side that reads nothing, or a NaN, refuses it.
    #[test]
    fn wr_613_face_sided_patch_is_held_to_the_shipped_patchs_largest_angle() {
        use SideSeam::{Free, Read};
        let target = 8.7e-3;
        let surfaced = [true, true, true, false];
        // the shipped patch's proved lower bounds: its largest angle is at least 6.2e-3,
        // so a candidate within that on every side has no larger largest angle
        let bar = [Some(3.3e-3), Some(4.9e-3), Some(6.2e-3), None];
        assert_eq!(seam_limit(target, &bar, surfaced), Some(6.2e-3));
        // a free side's bound does not count; the target caps the limit
        assert_eq!(
            seam_limit(target, &[Some(1e-3), None, None, Some(9e-3)], surfaced),
            Some(1e-3)
        );
        assert_eq!(
            seam_limit(target, &[Some(9e-3), None, None, None], surfaced),
            Some(target)
        );
        // nothing proved of the shipped patch (or a NaN): no limit, nothing is taken
        assert_eq!(seam_limit(target, &[None; 4], surfaced), None);
        assert_eq!(
            seam_limit(target, &[Some(f64::NAN), None, None, None], surfaced),
            None
        );
        // the screen: every side with a surface read at or below the limit; a side may
        // rise (here the blend side, to the limit) while the largest angle falls
        let limit = 6.2e-3;
        assert!(read_within(
            &[
                Read(Some(5.9e-3)),
                Read(Some(6.2e-3)),
                Read(Some(1e-4)),
                Free
            ],
            limit
        ));
        assert!(!read_within(
            &[
                Read(Some(5.9e-3)),
                Read(Some(6.3e-3)),
                Read(Some(1e-4)),
                Free
            ],
            limit
        ));
        assert!(!read_within(
            &[Read(None), Read(Some(1e-3)), Read(Some(1e-4)), Free],
            limit
        ));
        assert!(!read_within(
            &[Read(Some(f64::NAN)), Read(Some(1e-3)), Free, Free],
            limit
        ));
    }

    /// A planar triangle as a cubic patch at `at`, its top side (v = 1) collapsed to the
    /// apex, in the plane `z = 0.3 x + 0.2 y` (relative to `at`), so its normal is
    /// computed with rounding; with the plane as neighbour on its other three sides (UV
    /// lines across it), as `CreateCornerBlend` pads a three-sided corner.
    fn far_triangle(at: Vec3) -> (BSplineSurface, SmSurface, Vec<Option<BSplineCurve>>) {
        let plane_z = |x: f64, y: f64| 0.3 * x + 0.2 * y;
        let net: Vec<Vec<CPoint>> = (0..=3)
            .map(|i| {
                (0..=3)
                    .map(|j| {
                        let (a, b) = (f64::from(i) / 3.0, f64::from(j) / 3.0);
                        let (x, y) = (0.5 * b + a * (1.0 - b), b);
                        CPoint::new(at.x + x, at.y + y, at.z + plane_z(x, y), NOW)
                    })
                    .collect()
            })
            .collect();
        let knots = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
        let patch = BSplineSurface::from_nurb(
            Surface::new(net, 3, 3, knots.clone(), knots).unwrap(),
            false,
        );
        let net: Vec<Vec<CPoint>> = [-1.0, 2.0]
            .iter()
            .map(|&x| {
                [-1.0, 2.0]
                    .iter()
                    .map(|&y| CPoint::new(at.x + x, at.y + y, at.z + plane_z(x, y), NOW))
                    .collect()
            })
            .collect();
        let lin = vec![0.0, 0.0, 1.0, 1.0];
        let plane =
            BSplineSurface::from_nurb(Surface::new(net, 1, 1, lin.clone(), lin).unwrap(), false);
        let uv = BSplineCurve::create_line_segment(
            2,
            Point3::new(0.2, 0.2, 0.0),
            Point3::new(0.8, 0.8, 0.0),
        )
        .unwrap();
        let uvs = vec![Some(uv.clone()), Some(uv.clone()), Some(uv), None];
        (patch, SmSurface::from(plane), uvs)
    }

    /// A candidate whose certification would need more piece bounds than its budget is
    /// refused (WR-613): the budget only bounds the work. The triangle lies in its
    /// neighbour's plane, so along a line in that plane the largest angle is proved 0 with
    /// the kernel's budget, [`CERTIFY_PIECES`]; with fewer piece bounds than the
    /// certification has starting pieces nothing is proved and the same candidate is not
    /// taken ([`certified_within`] with that budget).
    #[test]
    fn wr_613_a_candidate_past_the_certification_budget_is_refused() {
        use SideSeam::{Free, Read};
        let k = &crate::Kernel::default();
        let (patch, plane, uvs) = far_triangle(Vec3::new(0.0, 0.0, 0.0));
        let plane_z = |x: f64, y: f64| 0.3 * x + 0.2 * y;
        let line = BSplineCurve::create_line_segment(
            3,
            Point3::new(0.3, 0.2, plane_z(0.3, 0.2)),
            Point3::new(0.6, 0.4, plane_z(0.6, 0.4)),
        )
        .unwrap();
        let curves = vec![SmCurve::from(line.clone()); 4];
        let surfaces = [Some(&plane), None, None, None];
        let sides = CoonsSides {
            curves: &curves,
            uv_curves: &uvs,
            surfaces: &surfaces,
            face_sides: 0,
        };
        let seams = [Read(Some(0.0)), Free, Free, Free];
        let limit = 1.0e-3;
        assert!(certified_within(k, &patch, &seams, limit, &sides, CERTIFY_PIECES).unwrap());
        for budget in [0, crate::query::seam_bound::WITHIN_PIECES - 1] {
            assert!(
                !certified_within(k, &patch, &seams, limit, &sides, budget).unwrap(),
                "budget {budget}"
            );
        }
    }

    /// The face-sided attempt never fails a corner the shipped patch builds (WR-613): an
    /// error inside it (here building the face-sided patch from three sides, as
    /// [`BSplineSurface::create_coons_patch_on_faces`] refuses) returns the shipped patch
    /// bit for bit, while the kernel's unported and cancelled statuses are passed on and a
    /// taken candidate is returned.
    #[test]
    fn wr_613_an_error_in_the_face_sided_attempt_keeps_the_shipped_patch() {
        let k = &crate::Kernel::default();
        let (bar, plane, uvs) = far_triangle(Vec3::new(0.0, 0.0, 0.0));
        let side = BSplineCurve::create_line_segment(
            3,
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.3),
        )
        .unwrap();
        let curves = vec![SmCurve::from(side.clone()); 4];
        let surfaces = [Some(&plane); 4];
        let sides = CoonsSides {
            curves: &curves,
            uv_curves: &uvs,
            surfaces: &surfaces,
            face_sides: 1 << LEFT,
        };
        // the attempt does fail: three sides are no Coons patch
        let three = vec![side.clone(); 3];
        assert!(BSplineSurface::create_coons_patch_on_faces(
            1e-6,
            false,
            &mut three.clone(),
            &mut Vec::new(),
            COONS_FACE_LEFT,
        )
        .is_err());
        let kept = face_sided_or_bar(
            k,
            1e-6,
            (three, Vec::new()),
            COONS_FACE_LEFT,
            &sides,
            bar.clone(),
            1e-3,
        )
        .expect("an error in the attempt keeps the shipped patch");
        assert_eq!(
            kept.gw_nurb(),
            bar.gw_nurb(),
            "the shipped patch, bit for bit"
        );
        assert_eq!(kept.is_rational(), bar.is_rational());

        // the mapping itself: any other error keeps the bar; unported and cancelled are
        // passed on; a taken candidate is returned
        let other = Err(SolidError::invalid_input("test", "a failed attempt"));
        assert!(matches!(keep_bar_on_error(other), Ok(None)));
        let unported = Err(SolidError::unported("sm_Test", "test.unit"));
        assert!(matches!(
            keep_bar_on_error(unported),
            Err(SolidError::Unported { .. })
        ));
        let cancelled = Err(SolidError::Cancelled { operation: "test" });
        assert!(matches!(
            keep_bar_on_error(cancelled),
            Err(SolidError::Cancelled { .. } | SolidError::OutOfFuel { .. })
        ));
        let taken = keep_bar_on_error(Ok(Some(bar.clone()))).unwrap();
        assert_eq!(
            taken.map(|t| t.gw_nurb().cloned()),
            Some(bar.gw_nurb().cloned())
        );
    }

    /// Near a collapsed corner far from the origin the patch normal is lost in rounding:
    /// the stations whose cross product is within its rounding bound of zero are left out
    /// (bound `PI`), so they neither decide nor tie; every other station is kept with its
    /// angle and bound. Measured with corner stations at both ends of every side, so that
    /// stations next to the apex are measured (the kernel places none there,
    /// [`coons_corner_stations`]).
    #[test]
    fn coons_side_angle_leaves_out_a_collapsed_corner() {
        let k = &crate::Kernel::default();
        let (patch, plane, uvs) = far_triangle(Vec3::new(1e8, 1e8, 1e8));
        let surfaces = [Some(&plane), Some(&plane), Some(&plane), None];
        let frames = coons_side_frames_at(k, &patch, &uvs, &surfaces, |_| [true, true]).unwrap();
        let mut log = Vec::new();
        let angle = coons_side_angle(&patch, &frames, Some(&mut log)).unwrap();
        assert!(
            angle.sides[TOP].is_none(),
            "the collapsed side has no surface"
        );
        for ii in [LEFT, RIGHT] {
            let st: Vec<&CoonsStationLog> = log.iter().filter(|s| s.side == ii).collect();
            // the apex itself has no certified normal (its `du` is rounding of size `1e-8`
            // here, so it may get past the scale test); next to it the bound is PI too
            let apex = st
                .iter()
                .find(|s| s.s == 1.0)
                .expect("a station at the apex");
            assert_eq!(apex.err, PI, "side {ii}: a certified normal at the apex");
            assert!(
                st.iter()
                    .any(|s| s.angle.is_some() && s.err == PI && s.s > 0.999),
                "side {ii}: no station next to the apex left out"
            );
            let side = angle.sides[ii].as_ref().expect("the side has a surface");
            assert_eq!(side.len(), st.len(), "side {ii}: one angle a station");
            for (a, s) in side.iter().zip(&st) {
                let expect = s.angle.and_then(|angle| certified_station(angle, s.err));
                assert_eq!(*a, expect, "side {ii} at {}", s.s);
            }
            assert!(
                side.iter().flatten().count() > 20,
                "side {ii}: few certified stations"
            );
        }
    }

    /// Corner stations go only at a corner the loop fixes: the triangle's top side has no
    /// surface (a collapsed side), so its left and right sides get corner stations at
    /// their bottom ends and none towards the apex, where only the span stations remain;
    /// its bottom side, between two fixed corners, gets them at both ends.
    #[test]
    fn no_corner_stations_at_a_collapsed_corner() {
        let k = &crate::Kernel::default();
        let (patch, plane, uvs) = far_triangle(Vec3::new(0.0, 0.0, 0.0));
        let surfaces = [Some(&plane), Some(&plane), Some(&plane), None];
        for ii in [LEFT, RIGHT] {
            assert_eq!(coons_fixed_corners(&surfaces, ii), [true, false]);
        }
        assert_eq!(coons_fixed_corners(&surfaces, BOT), [true, true]);
        let along = Extent1d::from_min_max(0.0, 1.0);
        let spans: Vec<f64> = (0..=1024).map(|i| f64::from(i) / 1024.0).collect();
        let on_span = |s: f64| spans.contains(&s);
        let frames = coons_side_frames(k, &patch, &uvs, &surfaces).unwrap();
        for ii in [LEFT, RIGHT, BOT] {
            let stations: Vec<f64> = frames.sides[ii]
                .as_ref()
                .unwrap()
                .iter()
                .map(|f| f.0)
                .collect();
            let all = coons_common_stations(&patch, SurfParamType::V, &along, [true, true]);
            // near the start the corner stations are there on every side
            assert!(stations.iter().any(|&s| s > 0.0 && s < 1e-5), "side {ii}");
            let near_end = stations.iter().filter(|&&s| s > 1.0 - 1.0 / 1024.0);
            if ii == BOT {
                assert_eq!(stations, all, "side {ii}");
            } else {
                // towards the apex only the span stations: the end itself
                assert!(near_end.copied().all(on_span), "side {ii}");
                assert!(stations.iter().all(|s| all.contains(s)), "side {ii}");
                assert!(stations.len() < all.len(), "side {ii}");
            }
        }
    }

    /// A patch with no certified station keeps the unrefined patch: at `1e13` from the
    /// origin every station's rounding bound is `PI`, and no refined copy can be shown
    /// no worse than it.
    #[test]
    fn a_patch_with_no_certified_station_is_kept() {
        let k = &crate::Kernel::default();
        let (patch, plane, uvs) = far_triangle(Vec3::new(1e13, 1e13, 1e13));
        let surfaces = [Some(&plane), Some(&plane), Some(&plane), None];
        let frames = coons_side_frames(k, &patch, &uvs, &surfaces).unwrap();
        let angle = coons_side_angle(&patch, &frames, None).unwrap();
        assert!(angle.sides.iter().flatten().flatten().all(Option::is_none));
        assert_eq!(angle.sup(), None);
        let kept = make_coons_patch_tangent_to_sides(k, &patch, &uvs, &surfaces, 0.0).unwrap();
        assert!(kept.is_none(), "a refined copy replaced the patch");
    }

    /// One side of station angles, every station certified.
    fn side_of(angles: &[(f64, f64)]) -> Vec<StationAngle> {
        angles.iter().map(|&a| Some(a)).collect()
    }

    /// A [`SideAngle`] with one station a side.
    fn one_station(sides: [Option<(f64, f64)>; 4]) -> SideAngle {
        SideAngle {
            sides: sides.map(|s| s.map(|a| side_of(&[a]))),
        }
    }

    /// M4 (review of WR-713): a bound of `PI / 2` or more bounds nothing (the exact angle
    /// lies in `[0, PI / 2]`), so such a station is not certified; the old rule kept every
    /// bound below `PI`.
    #[test]
    fn a_bound_of_a_right_angle_certifies_nothing() {
        let half = std::f64::consts::FRAC_PI_2;
        assert_eq!(certified_station(0.3, half), None);
        assert_eq!(certified_station(0.3, 2.0), None);
        assert_eq!(certified_station(0.3, f64::NAN), None);
        let below = next_down(half);
        assert_eq!(certified_station(0.3, below), Some((0.3, below)));
    }

    /// M5 (review of WR-713): every candidate is measured at the same stations, which
    /// hold every station of the bare patch and of each refinement; a side measured at
    /// other stations does not compare.
    #[test]
    fn candidates_are_measured_at_common_stations() {
        let (patch, _, _) = far_triangle(Vec3::new(0.0, 0.0, 0.0));
        let along = Extent1d::from_min_max(0.0, 1.0);
        let common = coons_common_stations(&patch, SurfParamType::U, &along, [true, true]);
        assert!(common.windows(2).all(|w| w[0] < w[1]), "not ascending");
        let knots = [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
        let mut levels = vec![knots.to_vec()];
        levels.extend(COONS_REFINEMENTS.map(|spans| refinement_knots(&knots, spans)));
        for level in &levels {
            for s in coons_stations(Some(level), 1, &along, [true, true]) {
                assert!(common.contains(&s), "station {s} missing");
            }
        }
        // a peak the held copy sees only at a station the candidate is not measured at
        // is not compared away
        let held = [Some((0.2, 1e-9)), Some((0.8, 1e-9)), Some((0.2, 1e-9))];
        assert_eq!(
            compare_side(&[Some((0.1, 1e-9)), Some((0.1, 1e-9))], &held, 0.0),
            SideVerdict::Uncertified
        );
        let cand = [Some((0.1, 1e-9)), Some((0.9, 1e-9)), Some((0.1, 1e-9))];
        assert_eq!(compare_side(&cand, &held, 0.0), SideVerdict::Worse);
    }

    /// M3 (review of WR-713): a candidate that loses a station the held copy certifies is
    /// not shown no worse there, whatever its other stations read; a station only the
    /// candidate certifies constrains nothing.
    #[test]
    fn a_lost_certification_is_not_better() {
        let held = [Some((0.5, 1e-9)), Some((0.9, 1e-9))];
        assert_eq!(
            compare_side(&[Some((0.1, 1e-9)), None], &held, COONS_RESOLUTION),
            SideVerdict::Uncertified
        );
        let held_angle = SideAngle {
            sides: [Some(side_of(&[(0.5, 1e-9)])), None, None, None],
        };
        let lost = SideAngle {
            sides: [Some(vec![None]), None, None, None],
        };
        assert!(!lost.no_worse_than(&held_angle, COONS_RESOLUTION));
        let gained = SideAngle {
            sides: [Some(vec![None]), None, None, None],
        };
        assert!(held_angle.no_worse_than(&gained, COONS_RESOLUTION));
    }

    /// M2 (review of WR-713): a side certainly worse vetoes a candidate even where its
    /// largest angle over all sides improves (0.9 down to 0.5).
    #[test]
    fn a_worse_side_vetoes_a_better_maximum() {
        let held = one_station([Some((0.9, 1e-6)), Some((0.1, 1e-6)), None, None]);
        let cand = one_station([Some((0.5, 1e-6)), Some((0.4, 1e-6)), None, None]);
        assert!(cand.sup() < held.sup());
        assert_eq!(
            cand.verdicts(&held, COONS_RESOLUTION),
            [
                SideVerdict::NoWorse,
                SideVerdict::Worse,
                SideVerdict::NoWorse,
                SideVerdict::NoWorse
            ]
        );
        assert!(!coons_takes(&[held], &cand));
    }

    /// M1 (review of WR-713): each candidate must be no worse than every copy held, not
    /// only the last: two rises of 0.004 radians, each a tie with the copy before (within
    /// both bounds of 0.002 and within the resolution), would carry the chosen copy
    /// 0.008 above the bare patch, which is certainly worse than it.
    #[test]
    fn a_chain_of_ties_does_not_drift() {
        let bare = one_station([None, Some((1.000, 2e-3)), None, None]);
        let first = one_station([None, Some((1.004, 2e-3)), None, None]);
        let second = one_station([None, Some((1.008, 2e-3)), None, None]);
        assert!(coons_takes(std::slice::from_ref(&bare), &first));
        assert!(second.no_worse_than(&first, COONS_RESOLUTION));
        assert!(!coons_takes(&[bare, first], &second));
    }

    /// B2 (review of WR-713): a comparison the bounds leave undecided does not take the
    /// candidate. `Endcap_corner_blend_3`'s left side reads 1.566 within 1.03 on the bare
    /// patch: a refined copy reading 1.2 within 1.0 there is not shown better, and its
    /// bottom side (1.1353 against 1.0752, each within 0.0034) is certainly worse.
    #[test]
    fn an_undecided_side_is_not_no_worse() {
        let bare = one_station([Some((1.566, 1.03)), Some((1.0752, 0.0034)), None, None]);
        let refined = one_station([Some((1.2, 1.0)), Some((1.0752, 0.0034)), None, None]);
        assert_eq!(
            refined.verdicts(&bare, COONS_RESOLUTION)[LEFT],
            SideVerdict::Undecided
        );
        assert!(!coons_takes(std::slice::from_ref(&bare), &refined));
        let worse = one_station([Some((1.2, 1.0)), Some((1.1353, 0.0034)), None, None]);
        assert_eq!(
            worse.verdicts(&bare, COONS_RESOLUTION)[BOT],
            SideVerdict::Worse
        );
        // a tie within the resolution, every side decided, takes the finer copy
        let tie = one_station([None, Some((1.0752 + 1e-3, 0.0034)), None, None]);
        let bare_bot = one_station([None, Some((1.0752, 0.0034)), None, None]);
        assert!(coons_takes(&[bare_bot], &tie));
    }

    /// A rise certainly above the held copy but within the resolution does not veto a
    /// copy whose largest angle is certainly lower by more than the resolution:
    /// `blend_crossing`'s crease corner (the solidcore-ops test
    /// `a_round_ending_at_a_crease_is_not_refused_over_rounds_it_never_reaches`), whose
    /// bare patch reads 0 along its right side and 12 and 29 degrees along the others,
    /// against 0.40, 0.32 and 0.31 degrees on the 64-span copy. A rise past the
    /// resolution still vetoes, and so does a rise within it that lowers no seam
    /// (analytic064's torus corner: every refined copy 6e-5 radians above the bare patch
    /// on two sides, equal on the others).
    #[test]
    fn a_certain_rise_within_the_resolution_trades_only_for_a_lower_seam() {
        let bare = one_station([
            Some((0.215_204, 5.01e-7)),
            Some((0.499_652, 1.19e-7)),
            Some((0.0, 1.20e-6)),
            None,
        ]);
        let finer = one_station([
            Some((0.005_665, 2.51e-6)),
            Some((0.005_362, 1.20e-7)),
            Some((0.006_924, 5.53e-6)),
            None,
        ]);
        let (_, finer_hi) = station_enclosure((0.006_924, 5.53e-6));
        let (_, bare_hi) = station_enclosure((0.0, 1.20e-6));
        assert!(
            finer_hi - 2.0 * 5.53e-6 > bare_hi,
            "the rise is not certain"
        );
        assert_eq!(
            finer.verdicts(&bare, COONS_RESOLUTION),
            [
                SideVerdict::NoWorse,
                SideVerdict::NoWorse,
                SideVerdict::Rises,
                SideVerdict::NoWorse
            ]
        );
        assert!(finer.lowers_sup(&bare, COONS_RESOLUTION));
        assert!(coons_takes(std::slice::from_ref(&bare), &finer));
        // the same rise without the lower seam: the bare patch's other sides as low
        let flat = one_station([
            Some((0.005_665, 2.51e-6)),
            Some((0.005_362, 1.20e-7)),
            Some((0.0, 1.20e-6)),
            None,
        ]);
        assert!(!finer.lowers_sup(&flat, COONS_RESOLUTION));
        assert!(!coons_takes(std::slice::from_ref(&flat), &finer));
        // analytic064: two sides certainly 6e-5 above, the others equal, no lower seam
        let torus = one_station([
            Some((0.517_153_939_750_907, 6.3e-10)),
            Some((0.517_153_939_750_907, 6.3e-10)),
            Some((0.517_153_939_749_756, 5.0e-10)),
            Some((0.517_153_939_749_756, 5.0e-10)),
        ]);
        let refined = one_station([
            Some((0.517_214_025_819_062, 6.3e-10)),
            Some((0.517_153_939_750_907, 6.3e-10)),
            Some((0.517_214_025_817_678, 5.0e-10)),
            Some((0.517_153_939_749_756, 5.0e-10)),
        ]);
        assert_eq!(
            refined.verdicts(&torus, COONS_RESOLUTION),
            [
                SideVerdict::Rises,
                SideVerdict::NoWorse,
                SideVerdict::Rises,
                SideVerdict::NoWorse
            ]
        );
        assert!(!coons_takes(&[torus], &refined));
        let past = one_station([
            Some((0.005_665, 2.51e-6)),
            Some((0.005_362, 1.20e-7)),
            Some((COONS_RESOLUTION + 1e-4, 5.53e-6)),
            None,
        ]);
        assert_eq!(
            past.verdicts(&bare, COONS_RESOLUTION)[RIGHT],
            SideVerdict::Worse
        );
        assert!(!coons_takes(&[bare], &past));
    }

    /// `lowers_sup` compares the candidate's upper end with the held copy's lower end
    /// less the resolution, rounded down a step: a candidate is certainly lower by more
    /// than the resolution only then. Swapping either end, or dropping the step, trades
    /// a rise for a seam that is not certainly lower.
    #[test]
    fn lowers_sup_compares_the_candidate_upper_end_with_the_held_lower_end() {
        let res = COONS_RESOLUTION;
        let held_station = (0.5, 0.1);
        let held = one_station([Some(held_station), None, None, None]);
        let (h_lo, h_hi) = station_enclosure(held_station);
        let edge = h_lo - res;
        // the candidate's enclosure straddles `h_lo - res`, below `h_hi - res`: by the
        // held copy's upper end, or by the candidate's lower end, it would trade
        let straddle = (edge, 0.05);
        let (c_lo, c_hi) = station_enclosure(straddle);
        assert!(c_lo < edge && edge < c_hi && c_hi < h_hi - res);
        assert!(!one_station([Some(straddle), None, None, None]).lowers_sup(&held, res));
        // an exact candidate whose upper end is `edge` itself: not below it by a step
        let at_edge = (next_down(edge), 0.0);
        assert_eq!(station_enclosure(at_edge).1, edge);
        assert!(!one_station([Some(at_edge), None, None, None]).lowers_sup(&held, res));
        // a step lower: certainly lower by more than the resolution
        let below = (next_down(next_down(edge)), 0.0);
        assert_eq!(station_enclosure(below).1, next_down(edge));
        assert!(one_station([Some(below), None, None, None]).lowers_sup(&held, res));
    }

    /// A tie of the largest angles hides a side read precisely: `vase_rev_corner_blend_3`'s
    /// refined copies tie the bare patch within the radian that the left and right sides'
    /// bounds leave, while their bottom side is certainly ten degrees worse.
    #[test]
    fn a_tie_does_not_hide_a_worse_side() {
        let bare = one_station([
            Some((0.0059, 1.38)),
            Some((1.3896, 2.6e-5)),
            Some((1.5705, 1.41)),
            None,
        ]);
        let finer = one_station([
            Some((0.1565, 1.16)),
            Some((1.570_779, 3.3e-5)),
            Some((1.570_780, 1.01)),
            None,
        ]);
        assert_eq!(
            finer.verdicts(&bare, COONS_RESOLUTION)[BOT],
            SideVerdict::Worse
        );
        assert!(!coons_takes(std::slice::from_ref(&bare), &finer));
    }

    #[test]
    fn skinned_surface_needs_sections() {
        let k = &crate::Kernel::default();
        let r = BSplineSurface::create_skinned_surface(k, &[], SkinnedSurfaceOptions::default());
        assert!(matches!(r, Err(SolidError::InvalidInput { .. })));
        // a spine asked for without the rail
        let spine = SkinnedSurfaceOptions {
            rail1_used_as_spine_curve: true,
            ..SkinnedSurfaceOptions::default()
        };
        let r = BSplineSurface::create_skinned_surface(k, &[], spine);
        assert_b_spline(
            &r.unwrap_err(),
            BSplineFailure::SpineRailMissing,
            SM_ERR_NULL_POINTER,
        );
        // a spine rail without the intersections: upstream's NER, 1006
        let mut rail =
            BSplineCurve::create_line_segment(3, Point3::ZERO, Point3::new(1.0, 0.0, 0.0)).unwrap();
        let spine = SkinnedSurfaceOptions {
            rail1: Some(&mut rail),
            rail1_used_as_spine_curve: true,
            ..SkinnedSurfaceOptions::default()
        };
        let r = BSplineSurface::create_skinned_surface(k, &[], spine);
        assert_b_spline(
            &r.unwrap_err(),
            BSplineFailure::RailIntersectionsMissing,
            SM_ERR_NULL_POINTER,
        );
        // a rail without the intersections and no spine: the same cause, upstream's 1001
        let mut rail =
            BSplineCurve::create_line_segment(3, Point3::ZERO, Point3::new(1.0, 0.0, 0.0)).unwrap();
        let rails = SkinnedSurfaceOptions {
            rail1: Some(&mut rail),
            ..SkinnedSurfaceOptions::default()
        };
        let r = BSplineSurface::create_skinned_surface(k, &[], rails);
        assert_b_spline(
            &r.unwrap_err(),
            BSplineFailure::RailIntersectionsMissing,
            crate::error::SM_ERR,
        );
    }
}
