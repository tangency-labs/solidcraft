//! Fillet identification: whether a face could be a fillet, judged by its surface's
//! cross sections and by edges lying on its boundary iso curves.
//!
//! Upstream: `source/SMLib/src/SmFace.cpp`, `source/SMLib/inc/SmFace.h`; co-ported
//! `SmBSplineSurface::TestForFilletSurfaceUOrV` (`SmBSplineSurface.cpp`),
//! `SmBSplineCurve::TestForFilletCrossSection` (`SmBSplineCurve.cpp`),
//! `smgu_CircleFromPointsTangents` (`SmGeomUtility.cpp`) and
//! `SmSurface::FindFilletMinMaxRadii` / `CalculateFilletRadius` (`SmSurface.cpp`).
//!
//! | Rust                                  | Upstream                                         |
//! |---------------------------------------|--------------------------------------------------|
//! | [`identify_fillet`]                   | `SmFace::IdentifyFillet`                         |
//! | [`identify_fillet_u_or_v`]            | `SmFace::IdentifyFilletUOrV`                     |
//! | [`IdentifyFilletOptions`]             | the in-params of `IdentifyFillet`                |
//! | [`FilletIdentification`]              | the out-params of `IdentifyFillet`               |
//! | [`test_for_fillet_surface_u_or_v`]    | `SmBSplineSurface::TestForFilletSurfaceUOrV`     |
//! | [`FilletSurfaceTest`]                 | the out-params of `TestForFilletSurfaceUOrV`     |
//! | [`test_for_fillet_cross_section`]     | `SmBSplineCurve::TestForFilletCrossSection`      |
//! | [`FilletCrossSection`]                | the out-params of `TestForFilletCrossSection`    |
//! | [`circle_from_points_tangents`]       | `smgu_CircleFromPointsTangents`                  |
//! | [`CircleFromPointsTangents`]          | the out-params of `smgu_CircleFromPointsTangents`|
//! | [`find_fillet_min_max_radii`]         | `SmSurface::FindFilletMinMaxRadii`               |
//! | [`calculate_fillet_radius`]           | `SmSurface::CalculateFilletRadius`               |
//!
//! Deviations:
//! - **Parameters.** The inputs of `IdentifyFillet` are [`IdentifyFilletOptions`],
//!   whose `Default` is the pinned caller's choice. The outputs are returned as structs. Outputs upstream leaves
//!   unset (a face that is not a fillet, a direction that is not one) keep the values
//!   of [`FilletIdentification::default`], which are what the only pinned caller
//!   (`SmApiFillets.cpp`'s `sm_IsFilletFace`) initialises them to.
//! - **Uninitialised distance.** When the max rib of `TestForFilletSurfaceUOrV` is
//!   degenerate, upstream compares an uninitialised `dDist2`; the port starts it at `0`
//!   (only reachable when every rib is degenerate).
//! - **Ignored evaluations.** `CalculateFilletRadius` ignores the statuses of its two
//!   `Evaluate1stDerivatives` and would compute from unset values; the port returns the
//!   failure, which its caller (status ignored) sees as the radius `0` the routine sets
//!   first.
//! - **Mutable B-rep.** `IdentifyFillet` is `const` upstream, but `IsTangentEdge` and
//!   `NormalizedEvaluate` may build missing UV trim curves; the port takes `&mut Brep`.
//! - **Null pointers.** A loop edge or primary edge use upstream dereferences while
//!   `NULL` is [`SolidError::InvalidInput`].
//! - No recursion.

use crate::curve::curve::p2::FindCurveDistanceExtrema;
use crate::curve::curve::Curve;
use crate::error::{ignore_status, ignored, Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent1d::Extent1d;
use crate::geom::global_solver::SolverOperation;
use crate::geom::local_solve1d::{EvalFunction, FunctionValue, LocalSolve1d};
use crate::geom::polynomial::solve_quadratic;
use crate::geom::tol::ZONE_TOL_3D;
use crate::geom::vector2d::{Point2, Vec2};
use crate::geom::vector3d::sqrt;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, EFF_ZERO, EFF_ZERO_SQ, EFF_ZERO_SQRT, PI};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::p1::SurfParamType;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::crv_fit::p1::ParamMethod;
use crate::nurbs::knots::Side;
use crate::surface::surface::Surface;
use crate::topology::edgeuse::p1::primary_edgeuse;
use crate::topology::edgeuse::p3::normalized_evaluate;
use crate::topology::{edge, face, loopuse, Brep, EdgeId, FaceId};

const OP: &str = "blend::identify_fillet";

fn null(what: &'static str) -> SolidError {
    SolidError::invalid_input(OP, what)
}

/// `NER(p)`: a `NULL` pointer is `SM_ERR_NULL_POINTER`.
fn ner<T>(v: Option<T>) -> Result<T> {
    v.ok_or_else(|| SolidError::null_pointer(OP))
}

/// Element `i` of an evaluation array.
fn nth(v: &[Vec3], i: usize) -> Result<Vec3> {
    v.get(i).copied().ok_or_else(|| null("short evaluation"))
}

/// The other parameter direction (`SM_SP_U` <-> `SM_SP_V`, anything but `U` gives `U`).
fn other_dir(d: SurfParamType) -> SurfParamType {
    if d == SurfParamType::U {
        SurfParamType::V
    } else {
        SurfParamType::U
    }
}

// ---- smgu_CircleFromPointsTangents ----------------------------------------------------

/// The circle through two points with given tangents
/// ([`circle_from_points_tangents`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CircleFromPointsTangents {
    /// `rbIsCircle`.
    pub is_circle: bool,
    /// `rPlacement`: centre, start direction and the in-plane direction of travel.
    pub placement: Axis2Placement,
    /// `rdRadius`.
    pub radius: f64,
    /// `rdStartAngleDeg` (always `0`).
    pub start_angle_deg: f64,
    /// `rdEndAngleDeg`.
    pub end_angle_deg: f64,
}

/// Whether `p1`/`tan1` and `p2`/`tan2` lie on one circle within `angle_tol_deg` of
/// perpendicular radius-to-tangent at `p2`, and that circle
/// (`smgu_CircleFromPointsTangents`). Outputs other than `is_circle` are only set for a
/// circle (else `Default`).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for the unit vector of the chord-side direction or of
/// the centre-to-`p2` direction of zero length, and from their angle (upstream
/// `SER`).
pub fn circle_from_points_tangents(
    p1: Point3,
    tan1: Vec3,
    p2: Point3,
    tan2: Vec3,
    angle_tol_deg: f64,
) -> Result<CircleFromPointsTangents> {
    let mut out = CircleFromPointsTangents {
        is_circle: false,
        placement: Axis2Placement::default(),
        radius: 0.0,
        start_angle_deg: 0.0,
        end_angle_deg: 0.0,
    };

    let mut norm = tan1.cross(tan2);
    if norm.length_squared() < EFF_ZERO_SQ {
        return Ok(out);
    }
    let vec_p1p2 = p2 - p1;
    let mut line_vec = norm.cross(tan1);
    if line_vec.dot(vec_p1p2) < 0.0 {
        line_vec = -line_vec;
    }
    line_vec = line_vec.unitize()?;
    let p_diff = p1 - p2;

    // Solve equation that says difference between circle center and P2
    // is same as T which is the radius.  The line goes from P1 back through
    // the origin.
    // t^2 = ((P1-T*LV) - P2)^2
    // Set up quadratic equation coefficients.
    let coeff = [
        p_diff.dot(p_diff),           // C
        2.0 * p_diff.dot(line_vec),   // B
        line_vec.dot(line_vec) - 1.0, // A of quadratic equation
    ];

    // Solve for t using quadratic equation.
    let roots = solve_quadratic(&coeff, EFF_ZERO);
    let sol = roots.as_slice();
    let mut t = 0.0;
    match *sol {
        [] => return Ok(out),
        [s0] => t = s0,
        [s0, s1] => t = crate::math::min2(s0, s1),
        _ => {}
    }

    if t.abs() < EFF_ZERO {
        return Ok(out);
    }

    let center = p1 + t * line_vec;

    let c_to_p2 = (p2 - center).unitize()?;

    let ang_rad = c_to_p2.angle_between(tan2)?;
    let ang_deg = ang_rad * 180.0 / PI;
    if (ang_deg - 90.0).abs() > angle_tol_deg {
        return Ok(out); // Doesn't satisfy angle tolerance
    }

    // We have made it - it is a circle - now compute
    // some vectors and angles that define it.
    let x = (-line_vec).unitize_or_z();

    let mut y = norm.cross(x);
    if y.dot(tan1) < 0.0 {
        y = -y;
        norm = -norm;
    }
    let y = y.unitize_or_z();

    ignore_status(out.placement.set_canonical(center, x, y))?;
    let angle_rad = norm.ccw_angle_between(x, c_to_p2)?;
    out.start_angle_deg = 0.0;
    out.end_angle_deg = angle_rad * 180.0 / PI;
    out.radius = t;
    out.is_circle = true;
    Ok(out)
}

// ---- SmBSplineCurve::TestForFilletCrossSection ----------------------------------------

/// The classification of a curve as a fillet cross section
/// ([`test_for_fillet_cross_section`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FilletCrossSection {
    /// `rbIsFilletCrossSection`.
    pub is_fillet_cross_section: bool,
    /// `rlFilletCrossSection`: `0` circular, `1` approximately circular, `2` linear,
    /// `3`/`4`/`5` G1/G2/G3 blend, `10` degenerate; `None` where upstream leaves it
    /// unset.
    pub cross_section: Option<u32>,
    /// `rdDistance`: the chord between the curve's ends; `None` where upstream leaves
    /// it unset (a degenerate curve).
    pub distance: Option<f64>,
    /// `rdRadius`: the arc radius, or that of the circle through the end points and
    /// tangents; `0` without one.
    pub radius: f64,
    /// `rdBlendScale` (always `1`).
    pub blend_scale: f64,
}

/// Whether `c` (a B-spline curve or one of its derived kinds) might be a fillet cross
/// section within `tol3d`: degenerate, a line, an arc, or the G1, G2 or G3 end
/// interpolant (`SmBSplineCurve::TestForFilletCrossSection`).
///
/// # Errors
///
/// Those of the evaluations, of [`circle_from_points_tangents`], of the interpolations
/// and of [`Curve::curve_max_distance_between`] (upstream `SER`).
pub fn test_for_fillet_cross_section(
    k: &Kernel,
    c: &Curve,
    tol3d: f64,
) -> Result<FilletCrossSection> {
    let mut out = FilletCrossSection {
        is_fillet_cross_section: false,
        cross_section: None,
        distance: None,
        radius: 0.0,
        blend_scale: 1.0,
    };

    if c.is_degenerate(k, EFF_ZERO, None)? {
        out.is_fillet_cross_section = true;
        out.cross_section = Some(10);
        return Ok(out);
    } // end is degenerate check

    let ivl = c.natural_interval();
    // (the SmBSplineCurve default: exact tangents)
    let pvvv = c.evaluate(k, ivl.min(), 3, true, false)?;
    let pvvv2 = c.evaluate(k, ivl.max(), 3, true, false)?;

    out.distance = Some(nth(&pvvv, 0)?.distance(nth(&pvvv2, 0)?));

    if c.is_line(k, 10, tol3d)?.is_some() {
        out.is_fillet_cross_section = true;
        out.cross_section = Some(2);
        return Ok(out);
    } // end isLine check

    if let Some((_, radius, _, _)) = c.is_arc(k, 10, tol3d)? {
        out.is_fillet_cross_section = true;
        // (0 circular when rational, 1 approximately circular)
        out.cross_section = Some(u32::from(!c.is_rational()?));
        out.radius = radius;
        return Ok(out);
    }

    let pnts = [nth(&pvvv, 0)?, nth(&pvvv2, 0)?];
    let tangents = [nth(&pvvv, 1)?, nth(&pvvv2, 1)?];
    let higher = [nth(&pvvv, 2)?, nth(&pvvv2, 2)?];

    let crv_angle_tol_deg = 1.0;
    let circle = circle_from_points_tangents(
        pnts[0],
        tangents[0],
        pnts[1],
        tangents[1],
        crv_angle_tol_deg,
    )?;
    if circle.is_circle {
        out.radius = circle.radius;
    }

    // the G1 (degree 3), G2 (degree 5) and G3 (degree 7) end interpolants
    let g3 = [
        nth(&pvvv, 2)?,
        nth(&pvvv2, 2)?,
        nth(&pvvv, 3)?,
        nth(&pvvv2, 3)?,
    ];
    let blends: [(usize, Option<&[Vec3]>, u32); 3] = [
        (3, None, 3),
        (5, Some(&higher[..]), 4),
        (7, Some(&g3[..]), 5),
    ];
    for (degree, more, kind) in blends {
        let blend = Curve::from(BSplineCurve::create_interpolating_curve(
            ParamMethod::Uniform,
            3,
            degree,
            &pnts,
            &tangents,
            more,
            false,
        )?);
        let b_ivl = blend.natural_interval();

        let mid_pnt = blend.evaluate_point(k, b_ivl.evaluate(0.5))?;
        let my_mid_pnt = c.evaluate_point(k, ivl.evaluate(0.5))?;

        if my_mid_pnt.distance(mid_pnt) < tol3d {
            let max_dist = c
                .curve_max_distance_between(k, &ivl, &blend, b_ivl.min(), b_ivl.max(), 10, None)?
                .distance;

            if max_dist < tol3d {
                out.is_fillet_cross_section = true;
                out.cross_section = Some(kind);
                return Ok(out);
            } // end max curve/curve distance check
        } // end midPoint distance check
    }

    Ok(out)
}

// ---- SmBSplineSurface::TestForFilletSurfaceUOrV ---------------------------------------

/// The classification of a surface's ribs in one direction
/// ([`test_for_fillet_surface_u_or_v`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FilletSurfaceTest {
    /// `rbIsFilletSurface`.
    pub is_fillet_surface: bool,
    /// `rlFilletCrossSection` (`9999` unless a fillet surface).
    pub cross_section: u32,
    /// `rlFilletSolverType`: `0` constant radius, `1` constant distance, `2` variable
    /// (`9999` unless a fillet surface).
    pub solver_type: u32,
    /// `rbNormalIsOutward`.
    pub normal_is_outward: bool,
    /// `*pdRadius`: the constant radius, else `-1`.
    pub radius: f64,
    /// `*pdDist`: the constant cross-section distance, else `-1`.
    pub dist: f64,
    /// `*pMinMaxRadii`, when asked for: the radii range of a variable fillet, else
    /// empty ([`Extent1d::new`]).
    pub min_max_radii: Option<Extent1d>,
}

/// Whether the iso curves of constant `which_dir` (the ribs) at the natural domain's
/// ends and at every interior knot all have the same fillet cross-section shape
/// within `tol3d`, and the fillet's solver type, constant radius or distance and
/// normal sense (`SmBSplineSurface::TestForFilletSurfaceUOrV`). `want_min_max_radii`
/// is upstream's non-null `pMinMaxRadii`.
///
/// # Errors
///
/// Those of the iso-curve constructions, of [`test_for_fillet_cross_section`] and of
/// the normal and rib evaluations (upstream `SER`); [`SolidError::Unported`] from the
/// ignored-status radii search.
pub fn test_for_fillet_surface_u_or_v(
    k: &Kernel,
    s: &Surface,
    which_dir: SurfParamType,
    tol3d: f64,
    want_min_max_radii: bool,
) -> Result<FilletSurfaceTest> {
    // init outputs
    let mut out = FilletSurfaceTest {
        is_fillet_surface: false,
        cross_section: 9999,
        solver_type: 9999,
        normal_is_outward: false,
        radius: -1.0,
        dist: -1.0,
        min_max_radii: want_min_max_radii.then(Extent1d::new),
    };

    // locals
    let mut constant_distance = true;
    let mut constant_radius = true;

    // 1. Check that min/max naturalDomain isoParmeter curves have fillet crossSection shapes

    // Construct temporary naturalDomain Iso boundaries for this direction: rib curves.
    let (rib_min, rib_max) = ner(s.create_iso_boundaries(k, which_dir, EFF_ZERO)?)?;

    // skip min isoParameter curves that don't have a known filletCrossSection shape
    let t = test_for_fillet_cross_section(k, &rib_min, tol3d)?;
    if !t.is_fillet_cross_section {
        return Ok(out);
    }
    let cross_section = t.cross_section;
    // (dDist starts at 0: a degenerate rib leaves it)
    let dist = t.distance.unwrap_or(0.0);
    let radius = t.radius;

    // skip max isoParameter curves that don't have a known filletCrossSection shape
    let t = test_for_fillet_cross_section(k, &rib_max, tol3d)?;
    if !t.is_fillet_cross_section {
        return Ok(out);
    }
    // (upstream's dDist2 is uninitialised here when the rib is degenerate)
    let mut dist2 = t.distance.unwrap_or(0.0);
    let mut radius2 = t.radius;

    // skip cases where min and max curves don't have same FilletCrossSection shapes
    if cross_section != t.cross_section {
        return Ok(out);
    }

    // arrive here when min/max naturalDomain isoParameter curves have the same crossSection shapes

    // surface is not a ConstantDistance fillet when min/max curve beg/end Point chord distances vary
    if (dist - dist2).abs() > tol3d {
        constant_distance = false;
    }

    // surface is not a ConstantRadius fillet when min/max curve beg/end Fillet radii vary
    if radius == 0.0 || (radius - radius2).abs() > tol3d {
        constant_radius = false;
    }

    // Test at every knot in rib direction (status ignored).
    let knots = ignored(s.knots_with_nurb(k, which_dir, None))?
        .map(|(knots, _)| knots)
        .unwrap_or_default();

    // for every knot value
    let mut good_cross_sections = true;
    for &knot in knots.get(1..knots.len().saturating_sub(1)).unwrap_or(&[]) {
        // create fillet crossSection direction isoParameter curve
        let rib = s
            .create_iso_parametric_curve(k, which_dir, knot, EFF_ZERO, None, false)?
            .curve;

        // test the internal isoParameter for a fillet CrossSection shape
        let t = test_for_fillet_cross_section(k, &rib, tol3d)?;
        if let Some(d) = t.distance {
            dist2 = d;
        }
        radius2 = t.radius;

        // quit when any crossSection fails to be the same kind of fillet CrossSection shape as the minBoundary
        if !t.is_fillet_cross_section {
            good_cross_sections = false;
            break;
        }
        if t.cross_section != cross_section {
            good_cross_sections = false;
            break;
        }

        // watch for varying distance and radius blends
        if (dist - dist2).abs() > tol3d {
            constant_distance = false;
        }
        if radius == 0.0 || (radius - radius2).abs() > tol3d {
            constant_radius = false;
        }
    } // end iter every internal knot isoParameter curve

    if !good_cross_sections {
        return Ok(out);
    }

    // Here we have a possible fillet - set outputs and return.

    // Set rbNormalIsOutward.
    // For that we need surface normal and filletCrossSection 2nd derivative.
    // The cross-section 2nd derivative points 'inward',
    // so normal is outward if it's in the opposite direction.
    let domain = s.natural_uv_domain_with_nurb(k)?;
    let normal = s.evaluate_normal(k, domain.min(), Side::Left, Side::Left)?;
    let pvv = rib_min.evaluate(k, rib_min.natural_interval().min(), 2, true, false)?;

    out.normal_is_outward = normal.dot(nth(&pvv, 2)?) < 0.0;

    // set other outputs and return
    out.solver_type = if constant_radius {
        0 // Constant Radius
    } else if constant_distance {
        1 // Constant Distance
    } else {
        2 // variable radius or unknown
    };
    out.is_fillet_surface = true;
    out.cross_section = cross_section.unwrap_or(9999);

    if constant_radius {
        out.radius = radius;
    }
    if constant_distance {
        out.dist = dist;
    }

    // Check for variable radius.
    if let (Some(radii), false) = (out.min_max_radii.as_mut(), constant_radius) {
        ignore_status(find_fillet_min_max_radii(k, s, which_dir, radii, EFF_ZERO))?;
    }

    Ok(out)
}

// ---- SmSurface::CalculateFilletRadius / FindFilletMinMaxRadii -------------------------

/// The radius of a fillet surface at `param` of the rail direction `rail_dir`, from the
/// end points and normals of the cross section there, and, with `want_circular`,
/// whether the two normals meet at one centre within `tol` (default
/// `SM_EFF_ZERO_SQRT`) (`SmSurface::CalculateFilletRadius`). Does not check that the
/// surface is a fillet.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a zero normal (upstream `SER`; its radius output is
/// then the `0` it starts with); those of the evaluations (ignored upstream, see the
/// module deviations).
pub fn calculate_fillet_radius(
    k: &Kernel,
    s: &Surface,
    rail_dir: SurfParamType,
    param: f64,
    want_circular: bool,
    tol: Option<f64>,
) -> Result<(f64, Option<bool>)> {
    let mut radius = 0.0;
    let mut is_circular = want_circular.then_some(true);

    let domain = s.natural_uv_domain_with_nurb(k)?;
    let cross_domain = if rail_dir == SurfParamType::U {
        domain.v_interval()
    } else {
        domain.u_interval()
    };

    let (uv1, uv2) = if rail_dir == SurfParamType::U {
        (
            Point2::new(param, cross_domain.min()),
            Point2::new(param, cross_domain.max()),
        )
    } else {
        (
            Point2::new(cross_domain.min(), param),
            Point2::new(cross_domain.max(), param),
        )
    };

    // nonZeroTangent evals
    let (pt1, su1, sv1) = s.evaluate_1st_derivatives(k, uv1, false, false)?;
    let (pt2, su2, sv2) = s.evaluate_1st_derivatives(k, uv2, false, false)?;

    let dist_sq = pt1.distance_squared(pt2);
    if dist_sq < EFF_ZERO_SQ {
        return Ok((radius, is_circular));
    }

    let norm1 = su1.cross(sv1).unitize()?;
    let norm2 = su2.cross(sv2).unitize()?;

    let dot = norm1.dot(norm2);
    // Prevent zero-divide.
    if dot >= 1.0 {
        // Shouldn't happen, but if it did, dDistSq should be 0.
        radius = sqrt(dist_sq);
    } else {
        // Here's the formula:
        let term = dist_sq / (2.0 * (1.0 - dot));
        radius = sqrt(term);
    }

    if want_circular {
        // Check whether the 'center points' are the same.
        let mut cent1 = pt1 + radius * norm1;
        let mut cent2 = pt2 + radius * norm2;
        if !cent1.closer_than(radius, cent2) {
            // Moved farther apart: normals point outward.
            cent1 = pt1 - radius * norm1;
            cent2 = pt2 - radius * norm2;
        }
        let tol = tol.unwrap_or(EFF_ZERO_SQRT);
        is_circular = Some(cent1.closer_than(tol, cent2));
    }

    Ok((radius, is_circular))
}

/// The radius of [`calculate_fillet_radius`] as its ignoring callers see it: the `0`
/// it starts with when it fails.
fn fillet_radius_or_zero(
    k: &Kernel,
    s: &Surface,
    rail_dir: SurfParamType,
    param: f64,
    want_circular: bool,
) -> Result<f64> {
    Ok(ignored(calculate_fillet_radius(
        k,
        s,
        rail_dir,
        param,
        want_circular,
        None,
    ))?
    .map_or(0.0, |(r, _)| r))
}

/// The extrema function at `t` with its status ignored: a failed evaluation leaves the
/// zeros it starts with.
fn sample(efo: &mut FindCurveDistanceExtrema<'_>, t: f64) -> Result<FunctionValue> {
    let mut v = FunctionValue::default();
    ignore_status(efo.evaluate(t, &mut v, true))?;
    Ok(v)
}

/// The range of the radii of a fillet surface whose rails run in `rail_dir`: at both
/// rail ends, at each of 30 steps where the rails' distance is extremal, and at the
/// extrema a local solve finds between steps (`SmSurface::FindFilletMinMaxRadii`).
/// `radii` is reset first and keeps what was added when the routine fails part way.
/// Does not check that the surface is a fillet.
///
/// # Errors
///
/// `SM_ERR_NULL_POINTER` without the rails; those of the rail construction (upstream
/// `SER`); [`SolidError::Unported`] from the ignored evaluations.
pub fn find_fillet_min_max_radii(
    k: &Kernel,
    s: &Surface,
    rail_dir: SurfParamType,
    radii: &mut Extent1d,
    tol: f64,
) -> Result<()> {
    radii.init();

    let rib_dir = other_dir(rail_dir);
    let (rail1, rail2) = ner(s.create_iso_boundaries(k, rib_dir, tol)?)?;

    let domain = rail1.natural_interval();

    let mut param = domain.min();
    radii.add_value(fillet_radius_or_zero(k, s, rail_dir, param, false)?);
    param = domain.max();
    radii.add_value(fillet_radius_or_zero(k, s, rail_dir, param, false)?);

    let mut efo = FindCurveDistanceExtrema::new(k, &rail1, &rail2, domain, tol);

    let mut ls = LocalSolve1d::with_interval(domain, false);

    let num_steps: u32 = 30;
    let denom = f64::from(num_steps);

    let mut prev_t = domain.min();
    let mut prev_f = sample(&mut efo, prev_t)?.f;

    for i in 1..=num_steps {
        let frac = f64::from(i) / denom;
        param = domain.evaluate(frac);

        let v = sample(&mut efo, param)?;
        let f = v.f;
        if v.found_answer {
            radii.add_value(fillet_radius_or_zero(k, s, rail_dir, param, true)?);
        } else if f * prev_f < -EFF_ZERO {
            let guess_param = prev_t - prev_f * (param - prev_t) / (f - prev_f);
            match ls.solve_it(&mut efo, guess_param, tol) {
                Ok(o) => {
                    radii.add_value(fillet_radius_or_zero(k, s, rail_dir, o.t, true)?);
                }
                Err(e) if e.is_unported() => return Err(e),
                Err(_) => {}
            }
        } // end else

        prev_t = param;
        prev_f = f;
    } // end stepping along curve

    Ok(())
}

// ---- SmFace::IdentifyFillet / IdentifyFilletUOrV --------------------------------------

/// The inputs of [`identify_fillet`] (`SmFace::IdentifyFillet`). [`Default`] is the
/// pinned caller's choice (`SmApiFillets.cpp`'s `sm_IsFilletFace`): `SM_ZONE_TOL_3D`,
/// 5 degrees, tangent rails required, non-manifold rails allowed, no min/max radii.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IdentifyFilletOptions {
    /// `d3DTolerance`: the rail and cross-section tolerance.
    pub tol_3d: f64,
    /// `dTangencyTolDeg`: the tangency angle of a rail edge.
    pub tangency_tol_deg: f64,
    /// `bRequireTangent`: only tangent rail edges (non-manifold edges are not checked).
    pub require_tangent: bool,
    /// `bRequireManifold`: only manifold rail edges.
    pub require_manifold: bool,
    /// A non-null `pMinMaxRadii`: also find the radii range of a variable fillet.
    pub want_min_max_radii: bool,
}

impl Default for IdentifyFilletOptions {
    fn default() -> Self {
        Self {
            tol_3d: ZONE_TOL_3D,
            tangency_tol_deg: 5.0,
            require_tangent: true,
            require_manifold: false,
            want_min_max_radii: false,
        }
    }
}

/// What [`identify_fillet`] found (the out-params of `SmFace::IdentifyFillet`).
#[derive(Debug, Clone, PartialEq)]
pub struct FilletIdentification {
    /// `rbIsFillet`.
    pub is_fillet: bool,
    /// `rlFilletCrossSection`: `0` circular, `1` approximately circular, `2` linear,
    /// `3`/`4`/`5` G1/G2/G3 blend, `6` other.
    pub cross_section: u32,
    /// `rlFilletSolverType`: `0` constant radius, `1` constant distance, `2` variable.
    pub solver_type: u32,
    /// `reFilletRailDirection`: the direction the rails run in, or
    /// [`SurfParamType::Both`] when the face looks like a fillet both ways.
    pub rail_direction: SurfParamType,
    /// `rdRadius`: the constant radius, else `-1`.
    pub radius: f64,
    /// `rdDist`: the constant cross-section distance, else `-1`.
    pub dist: f64,
    /// `rRailEdgesMin`: outer-loop edges on the low iso boundary, in loop order.
    pub rail_edges_min: Vec<EdgeId>,
    /// `rRailEdgesMax`: outer-loop edges on the high iso boundary, in loop order.
    pub rail_edges_max: Vec<EdgeId>,
    /// `*pMinMaxRadii`, when asked for: the radii range of a variable fillet, else
    /// empty ([`Extent1d::new`]).
    pub min_max_radii: Option<Extent1d>,
}

impl Default for FilletIdentification {
    /// Not a fillet, with the values `sm_IsFilletFace` gives the outputs upstream leaves
    /// unset.
    fn default() -> Self {
        Self {
            is_fillet: false,
            cross_section: 0,
            solver_type: 0,
            rail_direction: SurfParamType::U,
            radius: 0.0,
            dist: 0.0,
            rail_edges_min: Vec::new(),
            rail_edges_max: Vec::new(),
            min_max_radii: None,
        }
    }
}

/// Whether `f` could be a fillet whose rails run in `rail_dir`
/// (`SmFace::IdentifyFilletUOrV`): its surface is a B-spline surface (or a derived
/// kind) whose two boundary iso curves across `rail_dir` are distinct and each carry
/// at least one outer-loop edge (manifold and/or tangent within `tangency_tol_deg` as
/// asked), the edges' midpoints within ten times their tolerance of the iso curve;
/// then the surface's cross sections classify it. `rail_direction` of the result is
/// left at its default.
///
/// # Errors
///
/// Those of the rail construction, the edge evaluations and drops and of
/// [`test_for_fillet_surface_u_or_v`] (upstream `SER`); [`SolidError::InvalidInput`]
/// for a null loop edge or primary edge use.
pub fn identify_fillet_u_or_v(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    rail_dir: SurfParamType,
    options: &IdentifyFilletOptions,
) -> Result<FilletIdentification> {
    let IdentifyFilletOptions {
        tol_3d: tol3d,
        tangency_tol_deg,
        require_tangent,
        require_manifold,
        want_min_max_radii,
    } = *options;

    // init output
    let mut out = FilletIdentification {
        min_max_radii: want_min_max_radii.then(Extent1d::new),
        ..FilletIdentification::default()
    };

    // locals: get surface pointer
    let Some(surface) = face::p2::surface(b, f)? else {
        return Ok(out);
    };
    // no work - surface is not a BSplineSurface
    if !surface.is_kind_of(BSplineSurface::TYPE)? {
        return Ok(out);
    }
    let surface = surface.clone();

    // We make the assumption that the rail curves are the
    // min and max isoparameter curves of the surface.
    // Get the rail curves, and compare them to every edge that is
    // manifold, and tangent (if indicated by the input flags).

    // This will return the presumed rail curves:
    let rail_iso_dir = other_dir(rail_dir);
    let (min_rail, max_rail) = ner(surface.create_iso_boundaries(k, rail_iso_dir, tol3d)?)?;

    // We will be using these:
    let min_rail_domain = min_rail.natural_interval();
    let max_rail_domain = max_rail.natural_interval();

    // We will be doing DropPoint to these isocurves, from the midpoints
    // of prospective edges.  Get guess parameters for the drop.
    let param_guess_min = min_rail_domain.mid();
    let param_guess_max = max_rail_domain.mid();

    // Also, the rails must be distinct: a fillet cannot be closed in the rib direction.
    // (the evaluation's status is ignored; its point is then unset upstream)
    let rail_pt = ignored(min_rail.evaluate_point(k, param_guess_min))?;
    if let Some(rail_pt) = rail_pt {
        let drop = max_rail.drop_point(
            k,
            &min_rail_domain,
            rail_pt,
            None,
            tol3d,
            Some(param_guess_max),
            SolverOperation::Minimize,
        );
        match drop {
            Ok(d) if d.success && d.distance < tol3d => return Ok(out),
            Err(e) if e.is_unported() => return Err(e),
            _ => {}
        }
    }

    // Loop on all edges -- actually, only those in the outer loop.
    let loops = face::p2::loops(b, f, false, None)?;
    let Some(&outer) = loops.first() else {
        return Ok(out);
    };
    let edges = loopuse::loop_edges(b, outer)?;

    for e in edges {
        let e = e.ok_or_else(|| null("loop edge use without edge"))?;

        // Maybe skip nonManifold edges.
        if require_manifold && !edge::is_manifold(b, e)? {
            continue;
        }

        // Maybe skip nonTangent edges.
        // Note: this Boolean gets tangled with the manifold switch,
        // because a nonmanifold edge is always reported as non-tangent.
        // If the caller does not require Manifold, then tangency should
        // not be checked on a nonmanifold edge.  So, we reject a non-tangent
        // edge only if it is also manifold.
        if require_tangent
            && edge::is_manifold(b, e)?
            && !edge::is_tangent_edge(k, b, e, tangency_tol_deg, false)?
        {
            continue;
        }

        // This Edge passes tests for manifold and tangency.
        // See if it's one of the rail curves.

        // get edge's midPoint
        let prim = primary_edgeuse(b, e)?.ok_or_else(|| null("edge without primary edge use"))?;
        let mid_pnt = normalized_evaluate(k, b, prim, 0.5, false, false, true)?.point;

        // when midPoint drops to MinRail within 10*tolerance this is the minRail Edge
        let ten_tol = edge::tolerance(b, e)? * 10.0;
        let d = min_rail.drop_point(
            k,
            &min_rail.natural_interval(),
            mid_pnt,
            None,
            ten_tol,
            Some(param_guess_min),
            SolverOperation::Minimize,
        )?;
        if d.success && d.distance < ten_tol {
            out.rail_edges_min.push(e);
        }

        // when midPoint drops to MaxRail within 10*tolerance this is the minRail Edge
        let d = max_rail.drop_point(
            k,
            &max_rail.natural_interval(),
            mid_pnt,
            None,
            ten_tol,
            Some(param_guess_max),
            SolverOperation::Minimize,
        )?;
        if d.success && d.distance < ten_tol {
            out.rail_edges_max.push(e);
        }
    } // end iter every face->edge

    // this is a fillet Face if we found rail edges on both isoparam extemes
    if !out.rail_edges_min.is_empty() && !out.rail_edges_max.is_empty() {
        out.is_fillet = true;

        // Fill in rlFilletCrossSection, rlFilletSolverType, rbNormalIsOutward,
        // rdRadius, rdDist, and pMinMaxRadii.
        let t = test_for_fillet_surface_u_or_v(k, &surface, rail_dir, tol3d, want_min_max_radii)?;
        out.cross_section = t.cross_section;
        out.solver_type = t.solver_type;
        out.radius = t.radius;
        out.dist = t.dist;
        out.min_max_radii = t.min_max_radii;

        if !t.is_fillet_surface {
            // This can happen if the cross-section shape is not one of those
            // that SMLib will create: circular, G1,G2,G3, etc.
            out.cross_section = 6; // 'other'
            out.solver_type = 0; // const radius: most likely
        }
    }

    Ok(out)
}

/// Whether `f` could be a fillet (`SmFace::IdentifyFillet`): [`identify_fillet_u_or_v`]
/// in both directions. A face that is a fillet both ways reports
/// [`SurfParamType::Both`] and the values of the direction with the smaller of two
/// positive radii, else of the direction the surface is larger in. The pinned caller
/// passes [`IdentifyFilletOptions::default`].
///
/// # Errors
///
/// Those of [`identify_fillet_u_or_v`] (upstream `SER`); [`SolidError::Unported`] from
/// the ignored size estimate.
pub fn identify_fillet(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    options: &IdentifyFilletOptions,
) -> Result<FilletIdentification> {
    let want_min_max_radii = options.want_min_max_radii;

    // init output
    let mut out = FilletIdentification {
        min_max_radii: want_min_max_radii.then(Extent1d::new),
        ..FilletIdentification::default()
    };

    // Check both directions.
    let u = identify_fillet_u_or_v(k, b, f, SurfParamType::U, options)?;
    let v = identify_fillet_u_or_v(k, b, f, SurfParamType::V, options)?;

    // Sort out what we have.

    if !u.is_fillet && !v.is_fillet {
        return Ok(out);
    }

    // Looks like a fillet in at least one direction.
    out.is_fillet = true;

    let which = if !u.is_fillet {
        // Fillet in v-direction
        out.rail_direction = SurfParamType::V;
        v
    } else if !v.is_fillet {
        // Fillet in u-direction
        out.rail_direction = SurfParamType::U;
        u
    } else {
        // It's reporting fillet in both directions.
        // So: we will just return SM_SP_BOTH, and let the caller figure
        // it out.  It's quite possible that they don't care anyway.
        out.rail_direction = SurfParamType::Both;

        // One thing we can say is, that if it's circular in both directions,
        // the cross-section (rib) direction would have the smaller radius.
        // Other than that, we'll use the relative-sizes heuristic.
        let which_dir = if u.radius > 0.0 && v.radius > 0.0 {
            if u.radius < v.radius {
                SurfParamType::U
            } else {
                SurfParamType::V
            }
        } else {
            // (status ignored; the size is then unset upstream)
            let s = ner(face::p2::surface(b, f)?)?;
            let sizes = ignored(s.approximate_size(k, None))?.unwrap_or(Vec2::new(0.0, 0.0));
            if sizes.x > sizes.y {
                SurfParamType::U
            } else {
                SurfParamType::V
            }
        };
        if which_dir == SurfParamType::U {
            u
        } else {
            v
        }
    };

    out.cross_section = which.cross_section;
    out.solver_type = which.solver_type;
    out.radius = which.radius;
    out.dist = which.dist;
    out.rail_edges_min = which.rail_edges_min;
    out.rail_edges_max = which.rail_edges_max;
    if want_min_max_radii {
        out.min_max_radii = which.min_max_radii;
    }

    // all done
    Ok(out)
}
