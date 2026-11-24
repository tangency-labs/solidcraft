//! `SmCone` part 2: the offset cone, recognition of a cone NURB, the rebuild of the
//! analytic data from an edited NURB, and the ellipse intervals inside a trimmed cone.
//!
//! Upstream: `source/SMLib/src/SmCone.cpp`, `source/SMLib/inc/SmCone.h`;
//! `SmCylinder::IsNurbSurfaceCylinder` from `source/SMLib/src/SmCylinder.cpp`,
//! `source/SMLib/inc/SmCylinder.h` (ported here, next to the cone recogniser whose iso
//! arc test it shares).
//!
//! | Rust                                         | Upstream                                         |
//! |----------------------------------------------|--------------------------------------------------|
//! | [`Cone::create_offset_surface`]              | `SmCone::CreateOffsetSurface`                    |
//! | [`Cone::is_nurb_surface_cone`]               | `SmCone::IsNurbSurfaceCone`                      |
//! | [`Cone::is_nurb_surface_cylinder`]           | `SmCylinder::IsNurbSurfaceCylinder` (`SmCylinder.cpp`) |
//! | [`Cone::rebuild_step_from_nurb_parameters`]  | `SmCone::RebuildSTEPFromNURBParameters`          |
//! | [`ellipse_trim_intervals`]                   | `sm_GetEllipseTrimIntervals` (file static)       |
//! | `is_cone_point_in_trim_domain` (private)     | `sm_IsConePointInTrimDomain` (file static)       |
//! | [`Cone::reparameterize`]                     | `SmSurfOfRevolution::Reparameterize` (inherited; its polar converter rebuild calls the cone's iso curves) |
//! | [`Cone::trim_with_domain`]                  | `SmSurfOfRevolution::TrimWithDomain` (inherited; its conversions and NURB rebuilds call the cone's virtuals) |
//! | [`Cone::base_mut`]                           | `GetGwNurbPointer` (non-`const`, inherited)      |
//!
//! Deviations:
//! - **Inversion answers apart** (WR-662, the SolidCraft fork's tangency-labs/smlib
//!   dfc7cb6): the ellipse-trim classification passes a failed cone inversion on
//!   instead of reading it as exterior.
//! - **Out-params** become return values: the offset is `None` where upstream succeeds
//!   without a surface (both new radii below the tolerance); the recogniser's cone is
//!   `Some` when upstream returns `TRUE`; the trim intervals are returned.
//! - **Statuses.** Calls whose status upstream ignores keep upstream's values
//!   (`SmPoint3d` outputs stay `SM_UNDEF_DOUBLE`, a failed `AngleBetween` is `0`),
//!   except that [`SolidError::Unported`] always propagates.
//! - **Uninitialised reads.** A failed `ConvertUVFromNURBSToSTEP` in
//!   [`Cone::trim_with_domain`] (upstream ignores the status and reads the corner it may
//!   not have written) returns the error. `sm_GetEllipseTrimIntervals` ignores the status of
//!   `smgu_LinePlaneIntersect` and then reads its parameter: a seam line parallel to
//!   the ellipse plane (or of zero length) returns the error here. A failed
//!   `ConvertUVFromNURBSToSTEP` there leaves the default-constructed domain, as
//!   upstream. A STEP inversion of the cone that fails in
//!   the generator solve returns the error (upstream keeps the location it had already
//!   classified); its other failures leave `SM_LT_EXTERIOR`, as upstream.
//! - **Seam lines of the ellipse trim (WR-490).** `sm_GetEllipseTrimIntervals`
//!   evaluates the two seam lines on the given domain's v range, as upstream. Before
//!   v1.0.1 a face trimmed along its axis lost the seam crossing in its trimmed-off part;
//!   since v1.0.1 the trim circles sit at the domain's v limits and the segments are
//!   classified against the domain, so the ellipse keeps its part inside the domain.
//!   Seam lines over the cone's whole height (the retired opt-in) added crossings
//!   outside the domain, which split off zero-length or outside intervals.
//!   See `plan/upstream-issues/WR-490--ellipse-trim-seam-lines-on-trimmed-domain.md`.
//! - **Offset factory.** The apex-truncation branch dereferences the result of
//!   `CreateCanonical` without checking its status (a `NULL` cone crashes, a
//!   non-finite height recurses without bound); here that construction's error is
//!   returned.
//! - **Offset truncation height.** Where one offset radius falls below the tolerance,
//!   upstream takes the kept cone's height as `new_top_radius * height / old_top_radius`
//!   (and the mirror), which holds only for a full apex cone. Here the slope comes from
//!   the radius's span, `height / (old_top_radius - old_bot_radius)`, so a truncated
//!   cone's offset keeps its slope and stays `S + d n` at the same parameters; full apex
//!   cones give the same value.
//! - **NURB.** As in `surface::cone::p1`, the cone is expected to have its NURB
//!   ([`Cone::make_nurb`]); upstream makes a missing one inside `const` queries, which
//!   the port does (and keeps) at the `Surface` level (`Surface::kind_with_nurb`).
//!   `Notify` (attribute copying in the recogniser) has no equivalent.
//! - **Cylinder rebuild (#713, WR-385).** An `SmCylinder` keeps a zero
//!   semi-angle. Its recovered end radii must agree within
//!   `EFF_ZERO_SQRT * (1 + bottom_radius)`;
//!   their mean becomes its radius, and the NURB's control points are moved radially
//!   onto that cylinder about the recorded axis (heights, weights and knots kept).
//!   For coaxial, parallel, untwisted end circles the NURB is then the exact rational
//!   form of the analytic surface. The rebuild's angle checks still allow a far
//!   circle off the axis (below `height * sin(ang_tol deg)`, `ang_tol` = 1e-3
//!   degrees as `is_parallel_to` takes it) or turned about it (`twisted`, below
//!   `ang_tol`); radial scaling keeps those, so the moved net is kept only when
//!   `surface::implicit::lies_on` certifies it on the cylinder (#16982 review).
//!   Radii within `EFF_ZERO * (1 + bottom_radius)` keep upstream's copy of the bottom
//!   radius, but the cylinder is recorded only when `lies_on` certifies the net on it,
//!   as it is (kept bit for bit) or else moved onto it as above (#17228: upstream
//!   recorded it whatever the net, an oblique or twisted one included).
//!   Every cone rebuild also requires the top circle's normal to be parallel to the
//!   centre line, as the bottom's is (not upstream, #16982 review): before, a top
//!   circle tilted about its x axis passed. A larger taper, or a net that cannot be
//!   moved or certified, returns `SM_ERR` before editing the analytic state or the
//!   NURB. The pinned C++ instead assigns a nonzero semi-angle to
//!   cylinders. Ordinary `SmCone` objects retain upstream's radius/angle rules.
//! - **Cylinder recogniser.** [`Cone::is_nurb_surface_cylinder`] returns an error
//!   where a `SER` call returns its status (a truthy result without cylinder) and
//!   `Some` where upstream returns `TRUE` with a cylinder; its NURB is a copy of the
//!   tested one, as upstream. A tested surface without NURB is `SM_ERR_NULL_POINTER`
//!   (upstream copies a null NURB). An inside-out NURB gets `IsNurbSurfaceCone`'s
//!   frame (top-knot circle, reversed Z; WR-496, #1463), where upstream keeps the
//!   bottom-knot circle and reverses the cylinder's Z.
//! - **Cylinder iso arcs from control points.** [`Cone::is_nurb_surface_cylinder`] makes its
//!   end circles from the iso curves' control points (`iso_arc_exact`, The NURBS Book §7.3,
//!   §7.5), not fitted through three samples as `IsArc` does: two fitted end circles of an
//!   extruded arc put its axis off the extrusion's direction by more than rounding (1.4e-13
//!   rad on a 94 mm r 40 wall), so the cylinder was off its own NURBS form and failed the
//!   exact-class proof (`implicit::lies_on`). The cone recogniser and the rebuild keep `IsArc`.
//! - **Recursion.** None of these functions recurses.
//! - **Debug code.** The `SM_DEBUG_CODE` checks and drawing are omitted.

use super::Cone;
use crate::curve::curve::Curve;
use crate::curve::ellipse::{Ellipse, EllipseOptions};
use crate::curve::line::{BoundedLineOptions, Line};
use crate::error::surface::{AnalyticCheck, SurfaceFailure};
use crate::error::{ignore_status, ignored, Failure, Result, SolidError, SM_ERR_NULL_POINTER};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::geom_utility::p1::{
    line_plane_intersect, line_point_distance, shell_sort_double_array,
};
use crate::geom::geom_utility::p2::{circle_plane_intersect, CurveIntersection};
use crate::geom::tol::{approx_tol3d, is_zero};
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::{max2, min2, EFF_ZERO, EFF_ZERO_SQRT, PI};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::{
    BSplineSurface, IsoCurveOptions, LocationType, SurfParamType,
};
use crate::nurbs::nurbs_crv::to_euclid;
use crate::nurbs::nurbs_srf::copy_nurb_surface;
use crate::surface::implicit;

const OP_OFFSET: &str = "surface::cone::p2::Cone::create_offset_surface";
const OP_NURB_CYLINDER: &str = "surface::cone::p2::Cone::is_nurb_surface_cylinder";
const OP_REBUILD: &str = "surface::cone::p2::Cone::rebuild_step_from_nurb_parameters";
const OP_REPARAM: &str = "surface::cone::p2::Cone::reparameterize";
const OP_TRIM: &str = "surface::cone::p2::ellipse_trim_intervals";

/// `ANALYTIC_TOL_SCALE` (`SmConfig.h`).
const ANALYTIC_TOL_SCALE: f64 = 100.0;

/// A circular iso curve found by `IsArc` (or the apex of a cone with a vertex).
#[derive(Debug, Clone, Copy)]
struct IsoArc {
    /// `s*RefFrame`: origin at the centre, X towards the start point.
    frame: Axis2Placement,
    /// `d*Rad`.
    radius: f64,
    /// `d*StartAng`.
    start_ang: f64,
    /// `d*EndAng`.
    end_ang: f64,
}

/// The linear (generator) direction test shared by the recogniser and the rebuild:
/// `SM_SP_V` for a u of degree 1 with two control points, `SM_SP_U` for such a v,
/// else `SM_SP_NEITHER`.
fn linear_iso_param(s: &BSplineSurface) -> SurfParamType {
    if s.degree(SurfParamType::U) == 1 && s.number_control_points(SurfParamType::U) == 2 {
        SurfParamType::V
    } else if s.degree(SurfParamType::V) == 1 && s.number_control_points(SurfParamType::V) == 2 {
        SurfParamType::U
    } else {
        SurfParamType::Neither
    }
}

/// `pIsoCrv->IsArc(5, dScaledZero, ..)` of a plain B-spline iso curve.
fn iso_arc(k: &Kernel, curve: &BSplineCurve, scaled_zero: f64) -> Result<Option<IsoArc>> {
    let arc = crate::curve::curve::p5::is_arc_base(
        k,
        curve,
        |c, tol| c.is_degenerate(k, tol, None),
        5,
        scaled_zero,
    )?;
    Ok(arc.map(|(frame, radius, start_ang, end_ang)| IsoArc {
        frame,
        radius,
        start_ang,
        end_ang,
    }))
}

/// The circle a rational quadratic iso curve is, made from its control points (not
/// upstream, which fits a circle through three samples, `pIsoCrv->IsArc(5, dScaledZero, ..)`).
///
/// Each Bézier segment `Q0, Q1, Q2` (weights `w0, w1, w2`) of a circular arc has equal
/// tangent legs `|Q1 − Q0| = |Q2 − Q1|`, and its middle weight in standard form,
/// `w1 / √(w0 w2)`, is the cosine of the half sweep `α`, the angle between the leg
/// `Q1 − Q0` and the chord `Q2 − Q0` (The NURBS Book, Piegl and Tiller, §7.3 and §7.5). The
/// centre is then `Q0 + r n`, `n` the unit normal into the arc in its plane and
/// `r = |Q1 − Q0| / tan α`: the circle passes through `Q0` exactly, so an error in `r`
/// and the matching one in the centre cancel along the arc instead of growing with
/// `1 / (1 − w1²)` as the centre found from the legs' meeting does. Everything is built from
/// differences of control points, so rows that are translates of each other (an extruded
/// arc) give circles that are translates of each other, and the axis through their centres
/// is the extrusion's direction to rounding, where two independently fitted circles tilt
/// it (#13536).
///
/// The circle is made from the first segment at least half as wide as the widest, a well
/// conditioned one, and
/// the curve is that circle when every segment is an arc of it, end to end: its legs equal and
/// its weight the half sweep's cosine (within `EFF_ZERO_SQRT`), its plane parallel (within
/// `EFF_ZERO_SQRT`), its end points on the circle and its middle point `r / cos α` from the
/// centre, the lengths within `scaled_zero`. (A segment a hair long, an arc's extension, fixes
/// a centre of its own only to the rounding of its points over its sweep, so it is checked on
/// the circle, not compared by its own centre.)
/// The frame's X axis points to the start point and its Z axis about which the curve turns
/// counter-clockwise; the start angle is `0` and the end angle the sweep in degrees, as
/// `IsArc` gives them. `None` for a degenerate curve, a degree other than 2, or a curve that
/// is no circle.
///
/// # Errors
///
/// Those of the degeneracy test and the Bézier decomposition.
fn iso_arc_exact(k: &Kernel, curve: &BSplineCurve, scaled_zero: f64) -> Result<Option<IsoArc>> {
    /// One Bézier segment: its Euclidean control points, half sweep and plane normal.
    struct Segment {
        q0: Point3,
        q1: Point3,
        q2: Point3,
        half: f64,
        normal: Option<Vec3>,
    }
    if curve.is_degenerate(k, scaled_zero, None)? {
        return Ok(None);
    }
    let Some(nurb) = curve.nurb.as_ref() else {
        return Ok(None);
    };
    let mut segments = Vec::new();
    for segment in nurb.decompose_bez()? {
        if segment.degree() != 2 {
            return Ok(None);
        }
        let points = segment.cpts();
        let (Some(&a), Some(&b), Some(&c)) = (points.first(), points.get(1), points.get(2)) else {
            return Ok(None);
        };
        if !(a.w > 0.0 && b.w > 0.0 && c.w > 0.0) {
            return Ok(None);
        }
        let (q0, q1, q2) = (to_euclid(a), to_euclid(b), to_euclid(c));
        let (leg0, leg1, chord) = (q1 - q0, q2 - q1, q2 - q0);
        if (leg0.length() - leg1.length()).abs() > scaled_zero {
            return Ok(None);
        }
        // the legs made unit before their cross product: a short arc's legs are short, and
        // the product of their lengths would fall under the unit vector's absolute floor
        let (Ok(tangent), Ok(next)) = (leg0.unitize(), leg1.unitize()) else {
            return Ok(None);
        };
        // the half sweep, from the leg and the chord
        let half = tangent.cross(chord).length().atan2(tangent.dot(chord));
        let weight = b.w / (a.w * c.w).sqrt();
        if (weight - half.cos()).abs() > EFF_ZERO_SQRT {
            return Ok(None);
        }
        segments.push(Segment {
            q0,
            q1,
            q2,
            half,
            normal: tangent.cross(next).unitize().ok(),
        });
    }
    // The circle is made from a segment of large sweep, well conditioned: a segment a hair
    // long (an arc's extension past its face) fixes its centre only to the rounding of its
    // points over its sweep. The first segment at least half as wide as the widest is taken,
    // so equal sweeps (a whole circle's quarters) take the first. Every segment must then
    // lie on that circle.
    let widest = segments.iter().fold(0.0_f64, |m, s| m.max(s.half));
    let Some(base) = segments.iter().find(|s| s.half >= 0.5 * widest) else {
        return Ok(None);
    };
    let Some(normal) = base.normal else {
        return Ok(None);
    };
    let (leg0, leg1, chord) = (base.q1 - base.q0, base.q2 - base.q1, base.q2 - base.q0);
    let (Ok(tangent), Ok(across)) = (leg0.unitize(), chord.unitize()) else {
        return Ok(None);
    };
    // r = |leg| / tan α = |leg| cos α / sin α, from the leg's direction and the chord's
    let radius = leg0.length() * tangent.dot(across) / tangent.cross(across).length();
    // as `IsArc`: a radius too large for the arc's length makes no accurate circle
    if radius.is_nan() || radius > (leg0.length() + leg1.length()) / EFF_ZERO_SQRT {
        return Ok(None);
    }
    let centre = base.q0 + normal.cross(tangent) * radius;
    let mut sweep = 0.0_f64;
    for s in &segments {
        let on = |p: Point3| ((p - centre).length() - radius).abs() <= scaled_zero;
        // the middle point is where the end tangents meet, r / cos α from the centre
        let corner = ((s.q1 - centre).length() * s.half.cos() - radius).abs() <= scaled_zero;
        let flat = s
            .normal
            .is_none_or(|n| n.dot(normal) >= 1.0 - EFF_ZERO_SQRT);
        if !(on(s.q0) && on(s.q2) && corner && flat) {
            return Ok(None);
        }
        sweep += 2.0 * s.half;
    }
    let Some(first) = segments.first() else {
        return Ok(None);
    };
    // X toward the start point: away from the centre, square to the first segment's tangent
    let Ok(start_tangent) = (first.q1 - first.q0).unitize() else {
        return Ok(None);
    };
    let x_axis = start_tangent.cross(normal);
    // the segments run one way round: each starts where the one before it ends
    if segments.windows(2).any(|w| match w {
        [a, b] => a.q2.distance(b.q0) > scaled_zero,
        _ => false,
    }) {
        return Ok(None);
    }
    let end_ang = sweep.to_degrees();
    if !(end_ang > 0.0 && end_ang <= 360.0 + EFF_ZERO_SQRT) {
        return Ok(None);
    }
    Ok(Some(IsoArc {
        frame: Axis2Placement::new(centre, x_axis, normal.cross(x_axis)),
        radius,
        start_ang: 0.0,
        end_ang: end_ang.min(360.0),
    }))
}

/// The circle of a rational quadratic iso curve recovered from each segment's chord and
/// normalised middle weight `w = cos α` instead of its legs: `r = |Q2 − Q0| / (2 sin α)`, the
/// centre `r cos α` from the chord's midpoint, square to the chord in the circle's plane
/// (The NURBS Book, Piegl and Tiller, §7.3). When the end nets are translates of each other,
/// `axis`, the form's linear direction, gives the plane normal, and the arc's control points
/// must be within `scaled_zero` of the plane square to it through the start of the first
/// segment at least half as wide as the widest (#17462). A very
/// short arc's legs and point angles amplify coordinate rounding past the tolerance; its
/// chord, weight and plane distances do not. The other checks are [`iso_arc_exact`]'s. Used only where [`iso_arc_exact`]'s circles make no
/// cylinder, so a form that was recognised keeps its circles to the last bit.
///
/// # Errors
///
/// Those of the degeneracy test and the Bézier decomposition.
fn iso_arc_from_chord(
    k: &Kernel,
    curve: &BSplineCurve,
    scaled_zero: f64,
    axis: Option<Vec3>,
) -> Result<Option<IsoArc>> {
    /// One Bézier segment: its Euclidean control points, half sweep and plane normal.
    struct Segment {
        q0: Point3,
        q1: Point3,
        q2: Point3,
        half: f64,
        weight: f64,
        normal: Option<Vec3>,
    }
    if curve.is_degenerate(k, scaled_zero, None)? {
        return Ok(None);
    }
    let Some(nurb) = curve.nurb.as_ref() else {
        return Ok(None);
    };
    let mut segments = Vec::new();
    for segment in nurb.decompose_bez()? {
        if segment.degree() != 2 {
            return Ok(None);
        }
        let points = segment.cpts();
        let (Some(&a), Some(&b), Some(&c)) = (points.first(), points.get(1), points.get(2)) else {
            return Ok(None);
        };
        if !(a.w > 0.0 && b.w > 0.0 && c.w > 0.0) {
            return Ok(None);
        }
        let (q0, q1, q2) = (to_euclid(a), to_euclid(b), to_euclid(c));
        let (leg0, leg1, chord) = (q1 - q0, q2 - q1, q2 - q0);
        if (leg0.length() - leg1.length()).abs() > scaled_zero {
            return Ok(None);
        }
        // the legs made unit before their cross product: a short arc's legs are short, and
        // the product of their lengths would fall under the unit vector's absolute floor
        let (Ok(tangent), Ok(next)) = (leg0.unitize(), leg1.unitize()) else {
            return Ok(None);
        };
        // the half sweep, from the leg and the chord
        let half = tangent.cross(chord).length().atan2(tangent.dot(chord));
        let weight = b.w / (a.w * c.w).sqrt();
        if (weight - half.cos()).abs() > EFF_ZERO_SQRT {
            return Ok(None);
        }
        segments.push(Segment {
            q0,
            q1,
            q2,
            half,
            weight,
            normal: tangent.cross(next).unitize().ok(),
        });
    }
    // The circle is made from a segment of large sweep, well conditioned: a segment a hair
    // long (an arc's extension past its face) fixes its centre only to the rounding of its
    // points over its sweep. The first segment at least half as wide as the widest is taken,
    // so equal sweeps (a whole circle's quarters) take the first. Every segment must then
    // lie on that circle.
    let widest = segments.iter().fold(0.0_f64, |m, s| m.max(s.half));
    let Some(base) = segments.iter().find(|s| s.half >= 0.5 * widest) else {
        return Ok(None);
    };
    let Some(plane_normal) = base.normal else {
        return Ok(None);
    };
    let normal = if let Some(axis) = axis {
        // The arc lies on a plane square to the ruling when its control points are within
        // the distance zero of the plane through the start of `base` (the first segment at
        // least half as wide as the widest). Not the angle of the legs'
        // normal to the ruling: that normal carries the rounding of the points over the
        // sweep, 2e-8 rad on a 0.005 degree arc of radius 88 mm (#17462), past an angular
        // zero, where the ruling is the form's own direction to the last bits.
        let off = |p: Point3| (p - base.q0).dot(axis).abs() > scaled_zero;
        if segments.iter().any(|s| off(s.q0) || off(s.q1) || off(s.q2)) {
            return Ok(None);
        }
        if plane_normal.dot(axis) < 0.0 {
            -axis
        } else {
            axis
        }
    } else {
        plane_normal
    };
    let (leg0, leg1, chord) = (base.q1 - base.q0, base.q2 - base.q1, base.q2 - base.q0);
    let Ok(across) = chord.unitize() else {
        return Ok(None);
    };
    let radius = chord.length() / (2.0 * ((1.0 - base.weight) * (1.0 + base.weight)).sqrt());
    // as `IsArc`: a radius too large for the arc's length makes no accurate circle
    if radius.is_nan() || radius > (leg0.length() + leg1.length()) / EFF_ZERO_SQRT {
        return Ok(None);
    }
    let centre = base.q0 + chord * 0.5 + normal.cross(across) * (radius * base.weight);
    let mut sweep = 0.0_f64;
    for s in &segments {
        let on = |p: Point3| ((p - centre).length() - radius).abs() <= scaled_zero;
        // the middle point is where the end tangents meet, r / cos α from the centre
        let corner = ((s.q1 - centre).length() * s.weight - radius).abs() <= scaled_zero;
        let flat = s
            .normal
            .is_none_or(|n| n.dot(normal) >= 1.0 - EFF_ZERO_SQRT);
        if !(on(s.q0) && on(s.q2) && corner && flat) {
            return Ok(None);
        }
        sweep += 2.0 * s.half;
    }
    let Some(first) = segments.first() else {
        return Ok(None);
    };
    // X toward the start point on the recovered circle.
    let Ok(x_axis) = (first.q0 - centre).unitize() else {
        return Ok(None);
    };
    // the segments run one way round: each starts where the one before it ends
    if segments.windows(2).any(|w| match w {
        [a, b] => a.q2.distance(b.q0) > scaled_zero,
        _ => false,
    }) {
        return Ok(None);
    }
    let end_ang = sweep.to_degrees();
    if !(end_ang > 0.0 && end_ang <= 360.0 + EFF_ZERO_SQRT) {
        return Ok(None);
    }
    Ok(Some(IsoArc {
        frame: Axis2Placement::new(centre, x_axis, normal.cross(x_axis)),
        radius,
        start_ang: 0.0,
        end_ang: end_ang.min(360.0),
    }))
}

/// The point at the middle of the natural interval of a degenerate iso curve (status
/// ignored upstream: `SM_UNDEF_DOUBLE` on failure).
fn mid_point(curve: &BSplineCurve) -> Result<Point3> {
    Ok(ignored(curve.evaluate_point(curve.natural_interval().mid()))?.unwrap_or(Vec3::UNDEF))
}

/// The sweep iso curve of `s` at `knot`; the inner `Err` is the error of a construction
/// that failed (upstream `return FALSE`; an unported dependency or a cancellation still
/// propagates as the outer error).
fn iso_curve(
    k: &Kernel,
    s: &BSplineSurface,
    sweep: SurfParamType,
    knot: f64,
) -> Result<std::result::Result<BSplineCurve, SolidError>> {
    match s.create_iso_parametric_curve(k, sweep, knot, IsoCurveOptions::default()) {
        Ok(iso) => Ok(Ok(iso.curve)),
        Err(e @ (SolidError::Unported { .. } | SolidError::Cancelled { .. })) => Err(e),
        Err(e) => Ok(Err(e)),
    }
}

/// What [`end_circles`] finds at the two ends of the sweep.
enum EndCircles {
    /// The top and bottom circles (one of zero radius at a vertex).
    Found(IsoArc, IsoArc),
    /// The end iso curves are not two arcs or an arc and a degenerate curve.
    NotCircular,
    /// The construction of an end iso curve failed with this error (upstream's
    /// `return FALSE`, which the cone tests take for "not a cone").
    IsoFailed(SolidError),
}

/// The top and bottom circles of the sweep iso curves at `top_knot` and `bot_knot`
/// (with a zero-radius circle at a vertex), [`EndCircles::NotCircular`] when they are not
/// two arcs or an arc and a degenerate curve, or the error of an iso curve that could not
/// be made.
fn end_circles(
    k: &Kernel,
    s: &BSplineSurface,
    sweep: SurfParamType,
    top_knot: f64,
    bot_knot: f64,
    scaled_zero: f64,
) -> Result<EndCircles> {
    // make temporary Top and Bot BSPline isoParameter curves - hopefully arc shapes
    // (a failed curve is not a cone)
    let top_iso = match iso_curve(k, s, sweep, top_knot)? {
        Ok(c) => c,
        Err(e) => return Ok(EndCircles::IsoFailed(e)),
    };
    let bot_iso = match iso_curve(k, s, sweep, bot_knot)? {
        Ok(c) => c,
        Err(e) => return Ok(EndCircles::IsoFailed(e)),
    };

    // classify IsoparamCurves
    let top_degenerate = top_iso.is_degenerate(k, scaled_zero, None)?;
    let bot_degenerate = bot_iso.is_degenerate(k, scaled_zero, None)?;
    let top_arc = if top_degenerate {
        None
    } else {
        iso_arc(k, &top_iso, scaled_zero)?
    };
    let bot_arc = if bot_degenerate {
        None
    } else {
        iso_arc(k, &bot_iso, scaled_zero)?
    };

    //  2 Arcs                       = were good, continue
    //  1 Arc and a degenerate curve = cone has a vertex, set zero radius arc parameters
    //  2 degenerate curves          = not a cone, error
    match (top_arc, bot_arc) {
        (Some(top), _) if bot_degenerate => {
            // cone with a vertex
            let point = mid_point(&bot_iso)?;
            let bot = IsoArc {
                frame: Axis2Placement::new(point, top.frame.x_axis(), top.frame.y_axis()),
                radius: 0.0,
                start_ang: top.start_ang,
                end_ang: top.end_ang,
            };
            Ok(EndCircles::Found(top, bot))
        }
        (_, Some(bot)) if top_degenerate => {
            // cone with a vertex
            let point = mid_point(&top_iso)?;
            let top = IsoArc {
                frame: Axis2Placement::new(point, bot.frame.x_axis(), bot.frame.y_axis()),
                radius: 0.0,
                start_ang: bot.start_ang,
                end_ang: bot.end_ang,
            };
            Ok(EndCircles::Found(top, bot))
        }
        (Some(top), Some(bot)) => Ok(EndCircles::Found(top, bot)),
        // not a cone - Top or Bot circular isoParameter curve is degenerate or not an arc
        _ => Ok(EndCircles::NotCircular),
    }
}

/// Top/bot start angles, end angles or x axes vary by more than `ang_tol`.
fn twisted(top: &IsoArc, bot: &IsoArc, ang_tol: f64) -> Result<bool> {
    // (AngleBetween's status is ignored upstream: its angle is then 0)
    let twist_ang = ignored(top.frame.x_axis().angle_between(bot.frame.x_axis()))?.unwrap_or(0.0);
    Ok((top.start_ang - bot.start_ang).abs() > ang_tol
        || (top.end_ang - bot.end_ang).abs() > ang_tol
        || twist_ang.abs() > ang_tol)
}

impl Cone {
    /// Not upstream (SolidCraft's draft of a cylindrical wall): this cylinder turned into the
    /// cone whose radius, the cylinder's at the height `neutral_height` (from the frame's
    /// origin along its axis), changes by `slope` per unit of height. The NURB's control
    /// points move radially, each row by the radius ratio at its height (a rational circle
    /// scaled about its axis stays exact, and the rows run straight up the axis), so the
    /// cone keeps the cylinder's frame, trimmed domain and parameterization: the UV trim
    /// curves of its face stay valid on it. The analytic data is then rebuilt from the NURB
    /// ([`Self::rebuild_step_from_nurb_parameters`]). The result is a cone
    /// ([`Self::is_cylinder_class`] false), whatever the slope.
    ///
    /// # Errors
    ///
    /// `SM_ERR` unless this is a cylinder with a NURB whose radius stays positive and finite
    /// under a finite `slope` and `neutral_height`; those of the control point edits and of
    /// the rebuild.
    pub fn lean_cylinder(&self, k: &Kernel, slope: f64, neutral_height: f64) -> Result<Cone> {
        const OP_LEAN: &str = "surface::cone::p2::Cone::lean_cylinder";
        let canonical = self.canonical();
        if !self.is_cylinder()
            || canonical.radius.partial_cmp(&EFF_ZERO) != Some(core::cmp::Ordering::Greater)
        {
            return Err(SolidError::surface(OP_LEAN, SurfaceFailure::NotACylinder));
        }
        // a NaN or infinite radius law is the caller's, not a cone that crosses its axis
        if !(slope.is_finite() && neutral_height.is_finite()) {
            return Err(SolidError::surface(OP_LEAN, SurfaceFailure::LeanNotFinite));
        }
        let origin = canonical.origin.origin();
        let axis = canonical.origin.x_axis().cross(canonical.origin.y_axis());
        let axis = axis / axis.length();
        let mut cone = self.clone();
        // The result is an `SmCone`, not an `SmCylinder` (the capi's draft gets an `SmCone`
        // from `CopyAndAddAnalytics` on the edited NURB): a kept cylinder flag would make the
        // rebuild refuse the unequal radii (tangency-labs/solidcore#1606).
        cone.cylinder = false;
        let (nu, nv) = (
            cone.rev.base.number_control_points(SurfParamType::U),
            cone.rev.base.number_control_points(SurfParamType::V),
        );
        for i in 0..nu {
            for j in 0..nv {
                let (p, w) = cone.rev.base.control_point(
                    crate::nurbs::b_spline_curve::ControlPointForm::EuclideanRational,
                    i,
                    j,
                )?;
                let d = p - origin;
                let height = d.dot(axis);
                let radius = canonical.radius + slope * (height - neutral_height);
                // finite factors whose product overflows
                if !radius.is_finite() {
                    return Err(SolidError::surface(OP_LEAN, SurfaceFailure::LeanNotFinite));
                }
                if radius.partial_cmp(&EFF_ZERO) != Some(core::cmp::Ordering::Greater) {
                    return Err(SolidError::surface(
                        OP_LEAN,
                        SurfaceFailure::ConeClosesWithinSpan,
                    ));
                }
                let moved =
                    origin + axis * height + (d - axis * height) * (radius / canonical.radius);
                cone.rev.base.set_control_point(
                    crate::nurbs::b_spline_curve::ControlPointForm::EuclideanRational,
                    i,
                    j,
                    moved,
                    w,
                )?;
            }
        }
        cone.rebuild_step_from_nurb_parameters(k)?;
        Ok(cone)
    }

    /// The inherited NURB (`SmBSplineSurface` base object), mutably: an edit of the
    /// NURB (as through upstream's `GetGwNurbPointer`) leaves the analytic data as it
    /// was until [`Self::rebuild_step_from_nurb_parameters`].
    pub fn base_mut(&mut self) -> &mut BSplineSurface {
        &mut self.rev.base
    }

    /// Maps the NURB domain onto `trim_domain` without changing the shape (the
    /// inherited `SmSurfOfRevolution::Reparameterize`): the NURB (made first when
    /// missing, with the cone's iso curves), then the generator's NURB interval, then
    /// the polar converter rebuilt with the cone's iso curves (statuses ignored).
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` when the NURB cannot be made; those of
    /// [`crate::surface::surf_of_revolution::SurfOfRevolution::reparameterize`].
    pub fn reparameterize(&mut self, k: &Kernel, trim_domain: &Extent2d) -> Result<()> {
        // GetOrCreateGwNurbPointer (the virtual MakeNurb with the cone's iso curves)
        let mut made = Ok(());
        if self.rev.base.gw_nurb().is_none() {
            made = self.make_nurb(k);
            ignore_status(made.clone())?;
        }
        if self.rev.base.gw_nurb().is_none() {
            return Err(match made {
                Err(e) => {
                    SolidError::surface_nurb_not_made(OP_REPARAM, SM_ERR_NULL_POINTER, Err(e))
                }
                Ok(()) => Failure::new(
                    OP_REPARAM,
                    SurfaceFailure::Defect {
                        why: "the NURB is missing after make_nurb returned success, which on \
                              success sets the NURB",
                    },
                )
                .with_status(SM_ERR_NULL_POINTER)
                .into_error(),
            });
        }
        let inside_out = self.inside_out;
        self.rev
            .reparameterize_using(k, trim_domain, &|s, k, p, t| {
                Ok(super::p1::cone_iso_curve(s, inside_out, k, p, t, None, false)?.curve)
            })
    }

    /// Trims to the NURB domain `new_nurb_uv_domain` (the inherited
    /// `SmSurfOfRevolution::TrimWithDomain`): its body with the cone's
    /// `ConvertUVFromNURBSToSTEP` (in `UpdateAnalyticalDomain`) and iso curves (in
    /// `MakeNurb`, `Reparameterize` and `RebuildPolarConverter`) as its virtual calls.
    ///
    /// # Errors
    ///
    /// Those of [`crate::surface::surf_of_revolution::SurfOfRevolution::trim_with_domain`]
    /// and of [`Self::convert_uv_from_nurbs_to_step`] (upstream ignores that status and
    /// reads the corner it may not have written).
    pub fn trim_with_domain(&mut self, k: &Kernel, new_nurb_uv_domain: &Extent2d) -> Result<()> {
        let inside_out = self.inside_out;
        self.rev.trim_with_domain_using(
            k,
            new_nurb_uv_domain,
            &|s, k, uv| super::p1::cone_convert_uv_from_nurbs_to_step(s, inside_out, k, uv),
            &|s, k, p, t| Ok(super::p1::cone_iso_curve(s, inside_out, k, p, t, None, false)?.curve),
        )
    }

    /// Rebuilds the polar converter with the cone's iso curves
    /// (`RebuildPolarConverter`, which calls the virtual `CreateIsoParametricCurve`).
    pub(crate) fn rebuild_cone_polar_converter(&mut self, k: &Kernel) -> Result<()> {
        let inside_out = self.inside_out;
        self.rev.rebuild_polar_converter(k, &|s, k, p, t| {
            Ok(super::p1::cone_iso_curve(s, inside_out, k, p, t, None, false)?.curve)
        })
    }

    // ---- CreateOffsetSurface -----------------------------------------------------

    /// The cone truncated at an apex, as [`Self::create_offset_surface`] makes an offset that
    /// shrinks one end below the tolerance: the radius `new_bot_radius` at the bottom and
    /// `new_top_radius` at the top, the bottom's when `collapsed_at_base` (else the top's)
    /// replaced by 0, the ends `off_z_dist` along the axis from this cone's. The kept cone runs
    /// between its apex (where the radius reaches zero) and the surviving end, at this cone's
    /// slope: the radius spans `top_radius - bot_radius` over `height`. Made by
    /// [`Self::create_canonical`] (same `swap_uv` and `inside_out`), `height` this cone's
    /// ([`Self::height`]), its apex exactly on the
    /// axis, its NURB reparameterised to the matching part of this NURB domain (its share of
    /// the span, more than all of it where the cone runs on past this one's end to its apex),
    /// anchored at the end that survives, the generator's NURB interval set to match and the
    /// polar converter rebuilt: every point of the kept cone keeps its parameter, at the
    /// surviving end's radius plus the radii's change.
    ///
    /// # Errors
    ///
    /// Those of [`Self::create_canonical`] (upstream then dereferences a `NULL` cone);
    /// `SM_ERR_NULL_POINTER` without generator.
    fn truncated_at_apex(
        &self,
        k: &Kernel,
        height: f64,
        collapsed_at_base: bool,
        mut new_bot_radius: f64,
        mut new_top_radius: f64,
        off_z_dist: f64,
    ) -> Result<Self> {
        let origin = self.rev.position.origin();
        let x_axis = self.rev.position.x_axis();
        let y_axis = self.rev.position.y_axis();
        let z_axis = self.rev.position.z_axis();
        let nurb_domain = self.rev.base.natural_uv_domain();
        let old_bot_radius = self.bot_radius();
        let old_top_radius = self.top_radius();
        // compute BotOrigin, Height, Bot and Top radii for new truncated cone.
        // The kept cone runs between its new apex (where the offset radius reaches
        // zero) and the surviving end, at the original's slope: the radius spans
        // `old_top_radius - old_bot_radius` over `height`.
        // (The ends are at the STEP domain's heights, not at the origin and `height`
        // above it as upstream takes them: an unbounded cone runs from its apex, and an
        // adjusted one from any height. Upstream placed the kept cone's origin as if the
        // bottom were at the origin, off its apex by the bottom height, so the offset
        // was another cone: the base radius at the wrong height. With the bottom at
        // the origin, `0 + h` is `h` and nothing changes.)
        let bot_height = self.bot_height();
        let (new_height, bot_origin) = if collapsed_at_base {
            let new_height = new_top_radius * height / (old_top_radius - old_bot_radius);
            new_bot_radius = 0.0;
            (
                new_height,
                origin + (bot_height + height + off_z_dist - new_height) * z_axis,
            )
        } else {
            let new_height = new_bot_radius * height / (old_bot_radius - old_top_radius);
            new_top_radius = 0.0;
            (new_height, origin + (bot_height + off_z_dist) * z_axis)
        };

        // New Cone Position - truncated at new apex position
        let new_position = Axis2Placement::new(bot_origin, x_axis, y_axis);

        // Make the new cone - preserving original cone bSwapUV and bInsideOut values
        let mut off_cone = Self::create_canonical(
            k,
            &new_position,
            new_bot_radius,
            new_top_radius,
            new_height,
            self.rev.swap_uv,
            self.inside_out,
        )?;

        // compute the offsetSurf Nurb domain to be a properly trimmed subset of the
        // input Nurb domain: the part the truncated cone stands for. Its span is the
        // kept height's share of this domain's, anchored at the end that survives,
        // so a point of the kept cone keeps its parameter: the offset's rim stays at
        // the rim's parameter and the face's UV trims lift onto it unchanged (the
        // offset of S is S + d n at the same parameters). An inside-out cone's NURB
        // runs the generator backwards (`cone_convert_uv_from_nurbs_to_step`), so the
        // surviving end is the domain's other end there.
        let at_max_end = collapsed_at_base != self.inside_out;
        let tgt_nurb_domain = {
            let ratio = new_height / height;
            if self.rev.swap_uv {
                let lo = nurb_domain.min().x;
                let hi = nurb_domain.max().x;
                let span = hi - lo;
                let (min_x, max_x) = if at_max_end {
                    (hi - ratio * span, hi)
                } else {
                    (lo, lo + ratio * span)
                };
                Extent2d::from_bounds(min_x, nurb_domain.min().y, max_x, nurb_domain.max().y)
            } else {
                let lo = nurb_domain.min().y;
                let hi = nurb_domain.max().y;
                let span = hi - lo;
                let (min_y, max_y) = if at_max_end {
                    (hi - ratio * span, hi)
                } else {
                    (lo, lo + ratio * span)
                };
                Extent2d::from_bounds(nurb_domain.min().x, min_y, nurb_domain.max().x, max_y)
            }
        };

        // update the pOffCone NurbDomain (status ignored)
        ignore_status(off_cone.reparameterize(k, &tgt_nurb_domain))?;

        // adjust the GenCurve Nurb domain to be a properly trimmed subset of the
        // input NurbDomain (status ignored)
        let tgt_gen_nurb_ivl = if self.rev.swap_uv {
            tgt_nurb_domain.u_interval()
        } else {
            tgt_nurb_domain.v_interval()
        };
        let gen = off_cone.rev.gen_curve.as_mut().ok_or_else(|| {
            Failure::new(
                OP_OFFSET,
                SurfaceFailure::Defect {
                    why: "the offset cone has no generator, though create_canonical \
                          built it above with its generator line",
                },
            )
            .with_status(SM_ERR_NULL_POINTER)
            .into_error()
        })?;
        ignore_status(gen.edit_parameterization(k, &tgt_gen_nurb_ivl))?;

        // rebuild the PolarCurve to make sure all is coordinated (status ignored)
        ignore_status(off_cone.rebuild_cone_polar_converter(k))?;

        Ok(off_cone)
    }

    /// Not upstream (#21188): this cone moved `shift` along its axis and run on to its apex
    /// at the end it narrows toward, every point keeping its parameter and its radius
    /// ([`Self::truncated_at_apex`] with the radii as they are). Its NURB domain holds this
    /// one's, and more toward the apex. The offset of a cone moved toward its axis is such a
    /// cone, moved by the offset over the sine of the semi-angle; a face whose edges the
    /// offset's parameters put past its apex lies whole on this one. `None` for a cylinder
    /// (no apex) or radii that do not narrow toward one end.
    ///
    /// # Errors
    ///
    /// Those of [`Self::height`] and [`Self::truncated_at_apex`].
    pub(crate) fn moved_along_axis_to_apex(&self, k: &Kernel, shift: f64) -> Result<Option<Self>> {
        let (bot, top) = (self.bot_radius(), self.top_radius());
        // radii that narrow toward one end, neither below 0
        if self.cylinder
            || !shift.is_finite()
            || !(bot >= 0.0 && top >= 0.0)
            || !(bot < top || top < bot)
        {
            return Ok(None);
        }
        // the apex lies past the narrower end: past the bottom where it is narrower
        let collapsed_at_base = bot < top;
        let height = self.height(k)?;
        self.truncated_at_apex(k, height, collapsed_at_base, bot, top, shift)
            .map(Some)
    }

    /// Not upstream (`SmSurface::OffsetCollapses`, WR-464): whether the offset by
    /// `signed_offset_distance` has both radii below the tolerance, i.e. shrinks the cone
    /// (or cylinder) onto its axis, where [`Self::create_offset_surface`] gives no surface.
    #[must_use]
    pub fn offset_collapses(&self, k: &Kernel, signed_offset_distance: f64, tol: f64) -> bool {
        let tol = approx_tol3d(k, tol);
        let sign =
            if self.rev.swap_uv { -1.0 } else { 1.0 } * if self.inside_out { -1.0 } else { 1.0 };
        let d = (self.semi_angle_deg * PI / 180.0).cos() * sign * signed_offset_distance;
        self.bot_radius() + d < tol && self.top_radius() + d < tol
    }

    /// The cone offset by `signed_offset_distance` along the NURB normal (negated with
    /// [`crate::surface::surf_of_revolution::SurfOfRevolution::swap_uv`] and with
    /// [`Self::inside_out`]) (`CreateOffsetSurface`). The radii change by
    /// `cos(semi) d` and the origin moves by `-sin(semi) d` along the axis:
    ///
    /// * both new radii below the tolerance (`SmApproxTol3d(approx_tol_3d)`): `None`;
    /// * one of them below it: the cone truncated at its new apex, made by
    ///   [`Self::create_canonical`] (same `swap_uv` and `inside_out`), its NURB
    ///   reparameterised to the matching part of this NURB domain (upstream scales the
    ///   domain maximum, not its span), the generator's NURB interval set to match and
    ///   the polar converter rebuilt; the matching part is anchored at the end that
    ///   survives (an inside-out
    ///   cone's NURB runs the generator backwards, so its ends swap), so every point of
    ///   the kept cone keeps its parameter and the face's UV trims lift onto the offset
    ///   unchanged;
    /// * else a copy with the new base radius and origin, every NURB control point
    ///   moved by the offset of its generator row (computed from the next control
    ///   point at the apex) and the generator translated by the offset vector.
    ///
    /// # Errors
    ///
    /// Those of [`Self::height`] and of [`Self::create_canonical`] (upstream then
    /// dereferences a `NULL` cone); `SM_ERR_NULL_POINTER` without generator.
    pub fn create_offset_surface(
        &self,
        k: &Kernel,
        signed_offset_distance: f64,
        approx_tol_3d: f64,
    ) -> Result<Option<Self>> {
        let approx_tol_3d = approx_tol3d(k, approx_tol_3d);

        // When UVs are swapped - reverse orientation of offset
        // When InsideOut       - reverse orientation of offset
        let offset_sign = 1.0
            * (if self.rev.swap_uv { -1.0 } else { 1.0 })
            * (if self.inside_out { -1.0 } else { 1.0 });

        // Cone locals
        let origin = self.rev.position.origin();
        let x_axis = self.rev.position.x_axis();
        let y_axis = self.rev.position.y_axis();
        let z_axis = self.rev.position.z_axis();
        let height = self.height(k)?;

        // offset vector and displacements
        let mut sin = (self.semi_angle_deg * PI / 180.0).sin();
        let mut cos = (self.semi_angle_deg * PI / 180.0).cos();
        let norm = (cos * x_axis - sin * z_axis).unitize_or_z();
        let off_vec = offset_sign * signed_offset_distance * norm;
        let off_z_dist = -sin * offset_sign * signed_offset_distance;
        let off_rad_dist = cos * offset_sign * signed_offset_distance;

        // get new Bot and Top radii from Old and dOffRadDist
        let old_bot_radius = self.bot_radius();
        let old_top_radius = self.top_radius();
        let new_bot_radius = old_bot_radius + off_rad_dist;
        let new_top_radius = old_top_radius + off_rad_dist;

        // If both top and bottom are negative then don't create result
        // (`Self::offset_collapses`: the test of `SmSurface::OffsetCollapses`)
        if new_bot_radius < approx_tol_3d && new_top_radius < approx_tol_3d {
            return Ok(None);
        }

        // only the BotRadius or TopRadius has gone negative: the cone truncated at its new
        // apex
        if new_bot_radius < approx_tol_3d || new_top_radius < approx_tol_3d {
            return self
                .truncated_at_apex(
                    k,
                    height,
                    new_bot_radius < approx_tol_3d,
                    new_bot_radius,
                    new_top_radius,
                    off_z_dist,
                )
                .map(Some);
        }

        // copy the cone
        let mut off_cone = self.clone();

        // modify analytic parameters - new BaseRadius and new Cone Center
        off_cone.base_radius = self.base_radius + off_rad_dist;
        ignore_status(off_cone.rev.position.set_canonical(
            origin + off_z_dist * z_axis,
            x_axis,
            y_axis,
        ))?;

        // modify every Cone Nurb ControlPoint position - preserves the input NURB domain
        if let Some(off_nurb) = off_cone.rev.base.nurb.as_mut() {
            // For every m_pNurb ControlPoint - make inner iteration the GenCurve direction
            let (i_cnt, j_cnt) = if self.rev.swap_uv {
                (off_nurb.m(), off_nurb.n())
            } else {
                (off_nurb.n(), off_nurb.m())
            };
            let cp = |ii: usize, jj: usize| {
                if self.rev.swap_uv {
                    (jj, ii)
                } else {
                    (ii, jj)
                }
            };

            // for every sweep direction control point
            for ii in 0..=i_cnt {
                // get ControlPoint[i][0] Euclidean position
                let (u0, v0) = cp(ii, 0);
                let euclid_0 = to_euclid(off_nurb.cpt(u0, v0)?);

                // get the Cone radius for ControlPoint[i][0] Radius
                let p0_dot_z = (euclid_0.x - origin.x) * z_axis.x
                    + (euclid_0.y - origin.y) * z_axis.y
                    + (euclid_0.z - origin.z) * z_axis.z;
                let center = origin + p0_dot_z * z_axis;
                let vec = Vec3::new(
                    euclid_0.x - center.x,
                    euclid_0.y - center.y,
                    euclid_0.z - center.z,
                );
                let this_radius = self.radius(p0_dot_z);

                // when the radius is zero - need to compute move from other ControlPoint
                let mv = if is_zero(this_radius) {
                    // point is an apex - need to get rotation angle from next control
                    // point in genCurve direction
                    let (u1, v1) = cp(ii, 1);
                    let rotation_euclid = to_euclid(off_nurb.cpt(u1, v1)?);
                    let rotation_vec = Vec3::new(
                        rotation_euclid.x - origin.x,
                        rotation_euclid.y - origin.y,
                        rotation_euclid.z - origin.z,
                    );
                    let this_height = rotation_vec.dot(z_axis);
                    let radial_vec = rotation_vec - this_height * z_axis;
                    let radius = radial_vec.length();
                    let height_radius = self.radius(this_height);
                    // (operator/: a plain division, unlike operator/=)
                    let radial_unit = radial_vec / radius;
                    cos = radial_unit.dot(x_axis);
                    sin = radial_unit.dot(y_axis);

                    radius / height_radius * off_rad_dist * (cos * x_axis + sin * y_axis)
                        + off_z_dist * z_axis
                } else {
                    let scale = (this_radius + off_rad_dist) / this_radius;
                    vec * (scale - 1.0) + off_z_dist * z_axis
                };

                // for every genCurve directed ControlPoint
                for jj in 0..=j_cnt {
                    // apply the move to the controlPoint
                    let (u, v) = cp(ii, jj);
                    let euclid_j = to_euclid(off_nurb.cpt(u, v)?);
                    let cp_j = off_nurb.cpt_mut(u, v)?;
                    cp_j.x = (euclid_j.x + mv.x) * cp_j.w;
                    cp_j.y = (euclid_j.y + mv.y) * cp_j.w;
                    cp_j.z = (euclid_j.z + mv.z) * cp_j.w;
                }
            }
        }

        // translate the NewCone->GenCurve from current center line (status ignored)
        let mut mv = Axis2Placement::IDENTITY;
        mv.translate(off_vec);
        // (the offset cone is a copy of this one: its generator is this cone's)
        let gen = off_cone.rev.gen_curve.as_mut().ok_or_else(|| {
            SolidError::surface(OP_OFFSET, SurfaceFailure::SurfaceWithoutGenerator)
        })?;
        ignore_status(gen.transform(k, &mv, None))?;

        // (The polar converter's circle is rebuilt on the offset cone, as the truncated
        // branch above does: upstream's copy kept the original's circle, off the offset
        // by the offset distance, so conversions through it named the original's
        // parameters.) (status ignored)
        ignore_status(off_cone.rebuild_cone_polar_converter(k))?;

        Ok(Some(off_cone))
    }

    // ---- IsNurbSurfaceCone -------------------------------------------------------

    /// The height (the distance between its end circles' centres) below which
    /// [`Self::is_nurb_surface_cone`] refuses a NURB as a disk, not a cone, and
    /// [`Self::rebuild_step_from_nurb_parameters`] refuses to rebuild one. Named so that the
    /// rotational sweep, which builds no analytic cone lower than this, and these tests use
    /// the same number (tangency-labs/solidcraft#17085). Above it a form can still be refused
    /// by the other tests.
    pub(crate) const MIN_HEIGHT: f64 = EFF_ZERO_SQRT;

    /// The cone `test_surface` is (`IsNurbSurfaceCone`), or `None`: the surface must be
    /// rational, of degree 1 with two control points in one direction and degree 2 in
    /// the other, whose knots have the multiplicities `[3, 2, .., 2, 3]`; its
    /// iso curves at both ends of the linear direction are coaxial circular arcs (or an
    /// arc and a degenerate curve: a vertex) over the same angles, and the iso curve
    /// halfway is a circle of the average radius on the same axis. Tolerances are
    /// `tolerance_scale * SM_EFF_ZERO_SQRT` for angles and
    /// `tolerance_scale * 100 * SM_EFF_ZERO * (1 + |min corner| + |max corner|)` for
    /// distances (upstream default scale `1`). The new bounded cone is reparameterised
    /// to the surface's NURB domain (its NURB is the cone's own, not a copy).
    ///
    /// # Errors
    ///
    /// Those of the arc test (a failed iso curve is not a cone).
    pub fn is_nurb_surface_cone(
        k: &Kernel,
        test_surface: &BSplineSurface,
        tolerance_scale: f64,
    ) -> Result<Option<Self>> {
        Self::is_nurb_surface_cone_on(k, test_surface, tolerance_scale, None)
    }

    /// [`Self::is_nurb_surface_cone`] of the `SmBSplineSurface` part of `this` (not upstream: the
    /// fork's `SmProceduralBlend`): the corner points come from its virtual
    /// `EvaluatePoint`, so a procedural blend's are its definition's. `None` evaluates
    /// `test_surface` itself.
    ///
    /// # Errors
    ///
    /// As [`Self::is_nurb_surface_cone`].
    pub(crate) fn is_nurb_surface_cone_on(
        k: &Kernel,
        test_surface: &BSplineSurface,
        tolerance_scale: f64,
        this: Option<&crate::surface::surface::Surface>,
    ) -> Result<Option<Self>> {
        let corner = |uv| match this {
            Some(s) => s.evaluate_point(k, uv),
            None => test_surface.evaluate_point(uv),
        };
        // low work - not rational, not proper degree, not linear in one direction or other
        let deg_u = test_surface.degree(SurfParamType::U);
        let deg_v = test_surface.degree(SurfParamType::V);
        let uv_domain = test_surface.natural_uv_domain();
        let linear_iso_param = linear_iso_param(test_surface);

        // quit when surface is not properly parameterized for a potential cone
        if deg_u + deg_v != 3
            || !test_surface.is_rational()
            || linear_iso_param == SurfParamType::Neither
        {
            return Ok(None);
        }

        // check for proper knot vector in non-linear direction:
        // [tripleKnot, doubleKnot, doubleKnot, ..., tripleKnot]
        let (_, cir_mults) = test_surface.knots(linear_iso_param, None)?;
        if cir_mults.first() != Some(&3) || cir_mults.last() != Some(&3) {
            return Ok(None);
        }
        let inner = cir_mults.len().saturating_sub(2);
        if cir_mults.iter().skip(1).take(inner).any(|&m| m != 2) {
            return Ok(None);
        }

        // evaluate Nurb min/max domain points (statuses ignored)
        let min_pnt = ignored(corner(uv_domain.min()))?.unwrap_or(Vec3::UNDEF);
        let max_pnt = ignored(corner(uv_domain.max()))?.unwrap_or(Vec3::UNDEF);

        // scale zero value and angle tolerance
        let ang_tol = tolerance_scale * EFF_ZERO_SQRT;
        let scaled_zero = tolerance_scale
            * ANALYTIC_TOL_SCALE
            * EFF_ZERO
            * (1.0 + min_pnt.max_dimension() + max_pnt.max_dimension());

        // branch on linear direction to find circular and linear directions
        let (swap_uv, sweep_iso_param, bot_knot, top_knot) = if deg_u == 1 {
            // Need to swap UV when mapping parameters from analytic domain to nurbs domain
            (true, SurfParamType::U, uv_domain.min().x, uv_domain.max().x)
        } else {
            (
                false,
                SurfParamType::V,
                uv_domain.min().y,
                uv_domain.max().y,
            )
        };

        // classify the top and bottom isoParameter curves
        // (an end iso curve that cannot be made is not a cone either)
        let EndCircles::Found(mut top, bot) = end_circles(
            k,
            test_surface,
            sweep_iso_param,
            top_knot,
            bot_knot,
            scaled_zero,
        )?
        else {
            return Ok(None);
        };

        // check for not a cone - top/bot start angles or top/bot end angles vary
        if twisted(&top, &bot, ang_tol)? {
            return Ok(None);
        }

        // get the cone height = length of axis vector from botCurve Center to TopCurve
        // Center
        let center_top = top.frame.origin();
        let center_bot = bot.frame.origin();
        let mut nurb_z_axis = center_top - center_bot;
        let height = nurb_z_axis.length();

        // not a cone - height of cone is degenerate (cone is a disk)
        if height < Self::MIN_HEIGHT {
            return Ok(None);
        }
        nurb_z_axis = nurb_z_axis.unitize_or_z();

        // check for not a cone - bottom arc's plane is not perpendicular to cone axis
        if !nurb_z_axis.is_parallel_to(bot.frame.z_axis(), ang_tol) {
            return Ok(None);
        }

        // check for not a cone - a middle arc must also be concentric with axis, and in
        // a parallel plane
        let mid_param = (bot_knot + top_knot) / 2.0;
        let Ok(mid_iso) = iso_curve(k, test_surface, sweep_iso_param, mid_param)? else {
            return Ok(None);
        };

        // It must be circular.
        let Some(mid) = iso_arc(k, &mid_iso, scaled_zero)? else {
            return Ok(None);
        };

        // Radius must be halfway between top and bottom.
        if ((bot.radius + top.radius) / 2.0 - mid.radius).abs() > scaled_zero {
            return Ok(None);
        }

        // check for not a cone - middle arc's plane is not perpendicular to cone axis
        if !mid.frame.z_axis().is_parallel_to(nurb_z_axis, ang_tol) {
            return Ok(None);
        }

        // check for not a cone - middle arc's center is not on cone axis
        // (smgu_LinePointDistance always succeeds)
        let (dist_to_axis, _) = line_point_distance(center_top, nurb_z_axis, mid.frame.origin());
        if dist_to_axis > scaled_zero {
            return Ok(None);
        }

        // arrive here when surface has passed all not-a-cone filters

        // By definition: the circle rotation sets the Z axis for the cone when the
        // CenterBot to CenterTop vector opposes this Z axis, bInsideOut = TRUE
        let inside_out = nurb_z_axis.dot(bot.frame.z_axis()) < 0.0;

        // For the x-axis, use the one (top or bottom) at the beginning of the param
        // range in that direction.
        let x_axis = if inside_out {
            top.frame.x_axis()
        } else {
            bot.frame.x_axis()
        };

        // get STEP YAxis from XAxis and StepZAxis
        let step_z_axis = if inside_out {
            -nurb_z_axis
        } else {
            nurb_z_axis
        };
        let y_axis = step_z_axis.cross(x_axis);

        // when top and bot radius values are nearly equal - make them exactly equal
        if (bot.radius - top.radius).abs() < EFF_ZERO * (1.0 + bot.radius) {
            top.radius = bot.radius;
        }

        // build the analytic cone surface
        // bInsideOut toggles the STEP Origin between Top and Bot NurbCircles
        let mut cone = Self::new_bounded(
            if inside_out {
                top.frame.origin()
            } else {
                bot.frame.origin()
            },
            x_axis,
            y_axis,
            if inside_out { top.radius } else { bot.radius },
            if inside_out { bot.radius } else { top.radius },
            top.start_ang,
            top.end_ang,
            height,
            swap_uv,
            inside_out,
            false,
        );

        // reparameterize the NURB so that AnalyticSurface->NurbDomain ==
        // InputSurface->NurbDomain (status ignored)
        ignore_status(cone.reparameterize(k, &test_surface.natural_uv_domain()))?;

        Ok(Some(cone))
    }

    // ---- SmCylinder::IsNurbSurfaceCylinder ------------------------------------------

    /// The cylinder `test_surface` is (`SmCylinder::IsNurbSurfaceCylinder`), or `None`:
    /// the surface must be rational, of degree 1 with at most two control points in one
    /// direction and degree 2 in the other; its iso curves at both ends of the linear
    /// direction are non-degenerate circular arcs over the same angles, with the same
    /// X axis, of equal radii, on parallel planes perpendicular to the line through
    /// their centres (at least `SM_EFF_ZERO` apart). Tolerances are `SM_EFF_ZERO_SQRT`
    /// for angles and `tolerance_scale * SM_EFF_ZERO * 100 * (1 + |min corner| + |max
    /// corner|)` for distances and radii (upstream default scale `1`). The new bounded
    /// cylinder ([`Cone::is_cylinder`]) keeps a copy of the tested NURB as its own, its
    /// polar converter rebuilt from it and its generator reparameterised to the
    /// linear direction's NURB interval. An inside-out NURB gets the frame
    /// [`Self::is_nurb_surface_cone`] gives it: origin the top-knot circle, Z against
    /// the linear direction (WR-496, #1463). Upstream keeps the bottom-knot circle and
    /// Z along the linear direction, which reverses the cylinder's axis.
    ///
    /// # Errors
    ///
    /// Upstream's `SER` statuses (returned as a `TRUE` result without cylinder): those
    /// of the iso curves; those of the degeneracy and arc tests. A surface without NURB
    /// is not a cylinder (`Ok(None)`).
    pub fn is_nurb_surface_cylinder(
        k: &Kernel,
        test_surface: &BSplineSurface,
        tolerance_scale: f64,
    ) -> Result<Option<Self>> {
        if let Some(cylinder) =
            Self::nurb_surface_cylinder(k, test_surface, tolerance_scale, false)?
        {
            return Ok(Some(cylinder));
        }
        Self::nurb_surface_cylinder(k, test_surface, tolerance_scale, true)
    }

    /// [`Self::is_nurb_surface_cylinder`] with its end circles from [`iso_arc_exact`], or with
    /// `from_chord` from [`iso_arc_from_chord`].
    fn nurb_surface_cylinder(
        k: &Kernel,
        test_surface: &BSplineSurface,
        tolerance_scale: f64,
        from_chord: bool,
    ) -> Result<Option<Self>> {
        let deg_u = test_surface.degree(SurfParamType::U);
        let deg_v = test_surface.degree(SurfParamType::V);
        if deg_u + deg_v != 3
            || !test_surface.is_rational()
            || (deg_u == 1 && test_surface.number_control_points(SurfParamType::U) > 2)
            || (deg_v == 1 && test_surface.number_control_points(SurfParamType::V) > 2)
        {
            return Ok(None);
        }

        // (evaluation statuses ignored upstream)
        let uv_domain = test_surface.natural_uv_domain();
        let min_pnt = ignored(test_surface.evaluate_point(uv_domain.min()))?.unwrap_or(Vec3::UNDEF);
        let max_pnt = ignored(test_surface.evaluate_point(uv_domain.max()))?.unwrap_or(Vec3::UNDEF);

        let pt_tol = tolerance_scale
            * EFF_ZERO
            * ANALYTIC_TOL_SCALE
            * (1.0 + min_pnt.max_dimension() + max_pnt.max_dimension());

        let ang_tol = EFF_ZERO_SQRT;
        let (swap_uv, surf_param, bot_knot, top_knot) = if deg_u == 1 {
            // Need to swap UV when mapping parameters from analytic domain to nurbs domain
            (true, SurfParamType::U, uv_domain.min().x, uv_domain.max().x)
        } else {
            (
                false,
                SurfParamType::V,
                uv_domain.min().y,
                uv_domain.max().y,
            )
        };

        // The form's ruling: the start corners of its top and bottom arcs (see below).
        let top_start = if swap_uv {
            Point2::new(uv_domain.max().x, uv_domain.min().y)
        } else {
            Point2::new(uv_domain.min().x, uv_domain.max().y)
        };
        let top_corner = test_surface.evaluate_point(top_start)?;
        let min_corner = test_surface.evaluate_point(uv_domain.min())?;
        let ruling = top_corner - min_corner;
        let top_iso = test_surface
            .create_iso_parametric_curve(k, surf_param, top_knot, IsoCurveOptions::default())?
            .curve;
        if top_iso.is_degenerate(k, pt_tol, None)? {
            return Ok(None);
        }
        let bot_iso = test_surface
            .create_iso_parametric_curve(k, surf_param, bot_knot, IsoCurveOptions::default())?
            .curve;
        if bot_iso.is_degenerate(k, pt_tol, None)? {
            return Ok(None);
        }
        // The arcs' plane normal is the ruling when the end nets are translates of each other:
        // a short arc's normal found from its legs amplifies coordinate rounding. A small twist
        // is allowed by the angular tolerance, and its ruling is not the axis, so a twisted form
        // keeps the normal from the legs.
        let axis = top_iso
            .nurb
            .as_ref()
            .zip(bot_iso.nurb.as_ref())
            .filter(|(top, bot)| {
                top.cpts().iter().zip(bot.cpts()).all(|(top, bot)| {
                    (to_euclid(*top) - to_euclid(*bot) - ruling).length() <= pt_tol
                })
            })
            .and_then(|_| ruling.unitize().ok());
        let arc = |curve: &BSplineCurve| {
            if from_chord {
                iso_arc_from_chord(k, curve, pt_tol, axis)
            } else {
                iso_arc_exact(k, curve, pt_tol)
            }
        };
        let Some(top) = arc(&top_iso)? else {
            return Ok(None);
        };
        let Some(bot) = arc(&bot_iso)? else {
            return Ok(None);
        };

        if (top.start_ang - bot.start_ang).abs() > ang_tol
            || (top.end_ang - bot.end_ang).abs() > ang_tol
        {
            return Ok(None);
        }

        let center_top = top.frame.origin();
        let x_axis = top.frame.x_axis();
        // (status ignored upstream, which then reads the uninitialised angle; the arcs'
        // unit X axes never fail it: 0 here)
        let ang = ignored(x_axis.angle_between(bot.frame.x_axis()))?.unwrap_or(0.0);
        if ang.abs() > ang_tol {
            return Ok(None);
        }

        let center_bot = bot.frame.origin();

        // Not upstream (#16488): the form's own ruling, the difference of the start corners
        // of the top and bottom arcs, which are control points a fixed extrusion apart. The
        // arcs' centres each carry the rounding of a normal found from the legs of a short
        // arc, amplified by its radius over its sweep (1.6e-10 rad on a 0.046 degree arc of
        // radius 201 mm), which tilts the centres' line and the generator it starts, so that
        // it ran 3.3e-8 mm off the form. The ruling is required: a form with none (a
        // degenerate one, or one whose rows are not translates of each other) is no cylinder.
        // The arcs start at angle 0 by `iso_arc_exact`'s construction.
        let height = ruling.length();
        if !height.is_finite() || height < EFF_ZERO || top.start_ang != 0.0 || bot.start_ang != 0.0
        {
            return Ok(None);
        }
        let z_axis = ruling.unitize_or_z();

        // The form must still be a cylinder, as before: the arcs' centres lie along their
        // planes' normal (the rounding the ruling avoids is far under `ang_tol`, which is
        // what a form past it is refused by).
        let centre_line = center_top - center_bot;
        if centre_line.length() < EFF_ZERO
            || !centre_line.is_parallel_to(bot.frame.z_axis(), ang_tol)
        {
            return Ok(None);
        }
        // A top row turned about the axis by up to `ang_tol` (the arcs' X axes are held to it
        // above, in radians) tilts the ruling by that angle times radius over height, so the
        // ruling is held to the normal within that.
        let tilt = z_axis.angle_between(bot.frame.z_axis()).unwrap_or(PI);
        if tilt.min(PI - tilt) > EFF_ZERO_SQRT * (1.0 + bot.radius / height) {
            return Ok(None);
        }

        let inside_out = z_axis.dot(bot.frame.z_axis()) < 0.0;

        // Not upstream (WR-496, #1463): an inside-out NURB's STEP frame points
        // against the NURB's linear direction from its top-knot circle, as
        // IsNurbSurfaceCone builds it; upstream ("Do some correction here") keeps the
        // bottom-knot circle and the NURB's direction, reversing the cylinder's Z
        let flip = inside_out;
        let (centre, corner, z_axis) = if flip {
            (center_top, top_corner, -z_axis)
        } else {
            (center_bot, min_corner, z_axis)
        };

        // The frame the arcs' circles give, as upstream makes it (the centres' line, the
        // bottom or top centre, the top arc's X axis), is kept where the generator it starts
        // runs from the form's start corner to its far corner to within the rounding of the
        // points' size: it is then the form's own frame, to the last bits, and the recovery
        // changes nothing for the cylinders it already placed well. Where it does not (a
        // short arc's normal rounded by its radius over its sweep), the frame is the ruling's:
        // the origin the point of the axis level with the start corner, since the centre's
        // error is along the axis, and X the unit vector from it to the corner, not the first
        // segment's tangent, which a very short first segment rounds.
        let (far, circle_z) = if flip {
            (min_corner, -centre_line.unitize_or_z())
        } else {
            (top_corner, centre_line.unitize_or_z())
        };
        let circles_x = top.frame.x_axis();
        let circles_start = centre + bot.radius * circles_x;
        let circles_far = circles_start + centre_line.length() * circle_z;
        let rounding = f64::EPSILON * 16.0 * (1.0 + corner.max_dimension() + far.max_dimension());
        let (origin, x_axis, z_axis, height) = if (circles_start - corner).length() <= rounding
            && (circles_far - far).length() <= rounding
        {
            (centre, circles_x, circle_z, centre_line.length())
        } else {
            let origin = centre + (corner - centre).dot(z_axis) * z_axis;
            let Ok(x_axis) = (corner - origin).unitize() else {
                return Ok(None);
            };
            (origin, x_axis, z_axis, height)
        };

        let y_axis = z_axis.cross(x_axis);
        // Try to eliminate some numerical inaccurracies (the top radius is not used
        // further)
        let equal_radii = (bot.radius - top.radius).abs() < pt_tol;
        if !equal_radii {
            return Ok(None);
        }
        // Not upstream (#17462, #18415 review): the chord recovery holds the arcs to the
        // circle and to the plane square to the ruling only within `pt_tol`, so the copied
        // net is kept as this cylinder's form only when `implicit::lies_on` certifies it on
        // the cylinder, to the rounding of storing it. That tolerance does not also cover the
        // rounding of the recovered origin and radius or of the Bezier decomposition's knot
        // insertion, so a net off by only those may be refused: conservative, never unsound.
        // Otherwise the surface is no cylinder here and stays a B-spline.
        if from_chord {
            let certificate = implicit::Implicit::Cone {
                origin,
                axis: z_axis,
                radius: bot.radius,
                tan_angle: 0.0,
            };
            if !implicit::lies_on(test_surface, &certificate)? {
                return Ok(None);
            }
        }

        let mut cylinder = Self::new_cylinder_bounded(
            origin,
            x_axis,
            y_axis,
            bot.radius,
            top.start_ang,
            top.end_ang,
            height,
            swap_uv,
            inside_out,
            false,
        );
        // (error recovery: the constructor always makes its generator)

        // the cylinder's NURB is a copy of the tested one
        let nurb = test_surface.gw_nurb().ok_or_else(|| {
            Failure::new(
                OP_NURB_CYLINDER,
                SurfaceFailure::Defect {
                    why: "the tested surface has no NURB, though its degrees summed to 3 \
                          above, which a surface without NURB cannot",
                },
            )
            .with_status(SM_ERR_NULL_POINTER)
            .into_error()
        })?;
        cylinder.rev.base.nurb = Some(copy_nurb_surface(nurb)).into();
        cylinder.rev.base.nurb_is_borrowed = false;

        // (Notify(SM_NO_COPY) copies the test surface's attributes: not modelled)

        // (status ignored upstream)
        ignore_status(cylinder.rebuild_cone_polar_converter(k))?;

        // (status ignored upstream)
        let gen_ivl = Extent1d::from_min_max(bot_knot, top_knot);
        if let Some(gen) = cylinder.rev.gen_curve.as_mut() {
            ignore_status(gen.edit_parameterization(k, &gen_ivl))?;
        }

        Ok(Some(cylinder))
    }

    // ---- RebuildSTEPFromNURBParameters ---------------------------------------------

    /// Not upstream (WR-385): the NURB's control points moved radially about the axis
    /// through `centre` along the unit `z`, from the radius the frustum between the end
    /// circles has at their height (`radii.0` at height 0, `radii.1` at `height`) to
    /// `radius`, as `(i, j, point, weight)` for [`Self::certified_cylinder_net`].
    /// Heights, weights, knots and parameterisation are kept, so a net of coaxial,
    /// parallel, untwisted end circles becomes the exact rational form of the cylinder
    /// of `radius`; any other net is refused by the certificate. Reads only, so the
    /// rebuild calls it before changing any state.
    ///
    /// # Errors
    ///
    /// `SM_ERR` when the frustum's radius at a control point's height is not positive
    /// and finite; those of the control point access.
    fn cylinder_net(
        &self,
        centre: Point3,
        z: Vec3,
        height: f64,
        radii: (f64, f64),
        radius: f64,
    ) -> Result<Vec<(usize, usize, Point3, f64)>> {
        use crate::nurbs::b_spline_curve::ControlPointForm::EuclideanRational;
        let (bot_radius, top_radius) = radii;
        let (nu, nv) = (
            self.rev.base.number_control_points(SurfParamType::U),
            self.rev.base.number_control_points(SurfParamType::V),
        );
        let mut moved = Vec::new();
        for i in 0..nu {
            for j in 0..nv {
                let (p, w) = self.rev.base.control_point(EuclideanRational, i, j)?;
                let d = p - centre;
                let h = d.dot(z);
                let here = bot_radius + (top_radius - bot_radius) * (h / height);
                if !(here > 0.0 && here.is_finite()) {
                    return Err(SolidError::surface(
                        OP_REBUILD,
                        SurfaceFailure::NurbNotAnalytic {
                            check: AnalyticCheck::NetRadiusNotPositive,
                        },
                    ));
                }
                moved.push((i, j, centre + z * h + (d - z * h) * (radius / here), w));
            }
        }
        Ok(moved)
    }

    /// Not upstream (WR-385, #16982 review): a copy of the NURB with the control points
    /// [`Self::cylinder_net`] computed, kept only when [`implicit::lies_on`] certifies it
    /// on the cylinder of `radius` about the axis through `centre` along `z`, up to the
    /// rounding of storing it. Radial scaling makes coaxial, parallel, untwisted end
    /// rows that exact cylinder; end rows turned against each other about the axis
    /// (which `twisted` allows up to `ang_tol`), or a far circle off the axis, leave
    /// sections inside it (a twist `phi` puts the middle section at `radius cos(phi /
    /// 2)`), and are refused rather than recorded as the cylinder.
    ///
    /// # Errors
    ///
    /// `SM_ERR` when the moved net is not certified on the cylinder; those of the
    /// control point access and of the certificate.
    fn certified_cylinder_net(
        &self,
        moved: Vec<(usize, usize, Point3, f64)>,
        centre: Point3,
        z: Vec3,
        radius: f64,
    ) -> Result<BSplineSurface> {
        use crate::nurbs::b_spline_curve::ControlPointForm::EuclideanRational;
        let mut base = self.rev.base.clone();
        for (i, j, p, w) in moved {
            base.set_control_point(EuclideanRational, i, j, p, w)?;
        }
        let cylinder = implicit::Implicit::Cone {
            origin: centre,
            axis: z,
            radius,
            tan_angle: 0.0,
        };
        if !implicit::lies_on(&base, &cylinder)? {
            return Err(SolidError::surface(
                OP_REBUILD,
                SurfaceFailure::NurbNotAnalytic {
                    check: AnalyticCheck::NetOffCylinder,
                },
            ));
        }
        Ok(base)
    }

    /// Not upstream (WR-385, #17228): the rebuild's net on the cylinder of `radius` about
    /// the axis of `frame` (through its origin along its Z): `None` when `keep_if_on` and
    /// the net is certified on it as it is (kept), else the net moved onto it by
    /// [`Self::cylinder_net`] and certified by [`Self::certified_cylinder_net`].
    ///
    /// # Errors
    ///
    /// Those of [`Self::cylinder_net`], [`Self::certified_cylinder_net`] and the
    /// certificate.
    fn net_on_axis(
        &self,
        frame: &Axis2Placement,
        height: f64,
        radii: (f64, f64),
        radius: f64,
        keep_if_on: bool,
    ) -> Result<Option<BSplineSurface>> {
        let (origin, z) = (frame.origin(), frame.z_axis());
        // (#17228) a net the certificate already puts on the cylinder is the upstream
        // result and stays untouched; any other is moved and certified
        if keep_if_on && self.net_on_cylinder(origin, z, radius)? {
            return Ok(None);
        }
        let moved = self.cylinder_net(origin, z, height, radii, radius)?;
        Ok(Some(self.certified_cylinder_net(moved, origin, z, radius)?))
    }

    /// Not upstream (#17228): whether [`implicit::lies_on`] certifies the NURB as it is
    /// on the cylinder of `radius` about the axis through `centre` along `z`.
    ///
    /// # Errors
    ///
    /// Those of the certificate.
    fn net_on_cylinder(&self, centre: Point3, z: Vec3, radius: f64) -> Result<bool> {
        implicit::lies_on(
            &self.rev.base,
            &implicit::Implicit::Cone {
                origin: centre,
                axis: z,
                radius,
                tan_angle: 0.0,
            },
        )
    }

    /// Rebuilds the analytic data from the NURB after an edit
    /// (`RebuildSTEPFromNURBParameters`): the circles at both ends of the linear
    /// direction (tested as [`Self::is_nurb_surface_cone`] does with the scale `1000`,
    /// the corner points evaluated on the cone, without the knot and middle circle
    /// tests) give the semi-angle (kept for equal radii), base radius,
    /// inside-out flag, placement (the end circle's frame), STEP domain and a new
    /// generator line over the NURB's linear interval; the polar converter is rebuilt
    /// before the generator is replaced.
    /// Cylinders require end radii equal within `EFF_ZERO_SQRT * (1 + bottom_radius)`;
    /// they use the mean radius and an exactly zero semi-angle, and the NURB's control
    /// points are moved radially onto that cylinder about the recorded axis, before any
    /// state changes, so for coaxial, parallel end circles the NURB and the analytic form
    /// describe one surface (WR-385, #713; see [`Self::cylinder_net`]). Radii within
    /// `EFF_ZERO * (1 + bottom_radius)` take the bottom radius, and the net is kept
    /// when certified on that cylinder as it is, else moved onto it and certified
    /// (#17228). When neither works about the end circle's normal, the same is tried about
    /// the centre line through both circles' centres, and a net that passes is recorded
    /// with that axis (a rigid move of one end within the angle checks). Not upstream (#16982 review): the top circle's plane must be
    /// perpendicular to the axis too.
    ///
    /// # Errors
    ///
    /// `SM_ERR` when the NURB is no longer a cone, a cylinder's radii differ
    /// beyond the reconstruction tolerance, or a cylinder's net is not certified on
    /// the cylinder recorded; those of the iso curves.
    pub fn rebuild_step_from_nurb_parameters(&mut self, k: &Kernel) -> Result<()> {
        // check m_pNurb - a degree 2 rational surface in at least 1 direction
        let deg_u = self.rev.base.degree(SurfParamType::U);
        let deg_v = self.rev.base.degree(SurfParamType::V);
        let nurb_uv_domain = self.rev.base.natural_uv_domain();
        let linear_iso_param = linear_iso_param(&self.rev.base);
        let linear_ivl = match linear_iso_param {
            SurfParamType::V => nurb_uv_domain.u_interval(),
            SurfParamType::U => nurb_uv_domain.v_interval(),
            _ => Extent1d::new(),
        };

        // no work - modified parameterization
        if deg_u + deg_v != 3
            || !self.rev.base.is_rational()
            || linear_iso_param == SurfParamType::Neither
        {
            // Modified Nurb SmCone: no longer has degree 1 and 2 basis functions
            return Err(SolidError::surface(
                OP_REBUILD,
                SurfaceFailure::NurbNotAnalytic {
                    check: AnalyticCheck::DegreeAndRationality,
                },
            ));
        }

        // evaluate Nurb min/max domain points (the cone's EvaluatePoint; statuses
        // ignored)
        let nurb_min_point =
            ignored(self.evaluate_point(k, nurb_uv_domain.min()))?.unwrap_or(Vec3::UNDEF);
        let nurb_max_point =
            ignored(self.evaluate_point(k, nurb_uv_domain.max()))?.unwrap_or(Vec3::UNDEF);

        // scaled zero value (loose because surf is already labeled analytic)
        let tolerance_scale = 1000.0;
        let ang_tol = tolerance_scale * EFF_ZERO_SQRT;
        let scaled_zero = tolerance_scale
            * ANALYTIC_TOL_SCALE
            * EFF_ZERO
            * (1.0 + nurb_min_point.max_dimension() + nurb_max_point.max_dimension());

        // branch on linear direction to find circular and linear directions
        let (swap_uv, sweep_iso_param, bot_knot, top_knot) = if deg_u == 1 {
            (
                true,
                SurfParamType::U,
                nurb_uv_domain.min().x,
                nurb_uv_domain.max().x,
            )
        } else {
            (
                false,
                SurfParamType::V,
                nurb_uv_domain.min().y,
                nurb_uv_domain.max().y,
            )
        };

        // classify the top and bottom isoParameter curves (SmBSplineSurface's)
        let (mut top, mut bot) = match end_circles(
            k,
            &self.rev.base,
            sweep_iso_param,
            top_knot,
            bot_knot,
            scaled_zero,
        )? {
            EndCircles::Found(top, bot) => (top, bot),
            // Modified Nurb SmCone: no longer has circular top and bottom IsoParamCurves
            EndCircles::NotCircular => {
                return Err(SolidError::surface(
                    OP_REBUILD,
                    SurfaceFailure::NurbNotAnalytic {
                        check: AnalyticCheck::EndsNotCircular,
                    },
                ));
            }
            // (an end iso curve that could not be made: its error is the cause)
            EndCircles::IsoFailed(e) => {
                return Err(Failure::new(OP_REBUILD, SurfaceFailure::CallFailed)
                    .caused_by(e)
                    .into_error());
            }
        };

        // not a cone - top/bot start angles or top/bot end angles vary by more than
        // tolerance
        if twisted(&top, &bot, ang_tol)? {
            return Err(SolidError::surface(
                OP_REBUILD,
                SurfaceFailure::NurbNotAnalytic {
                    check: AnalyticCheck::EndArcsTwisted,
                },
            ));
        }

        // get the cone height = length of axis vector from botCurve Center to TopCurve
        // Center
        let center_top = top.frame.origin();
        let center_bot = bot.frame.origin();
        let mut nurb_z_axis = center_top - center_bot;
        let height = nurb_z_axis.length();

        // not a cone - height of cone is degenerate (cone is a disk)
        if height < Self::MIN_HEIGHT {
            return Err(SolidError::surface(
                OP_REBUILD,
                SurfaceFailure::NurbNotAnalytic {
                    check: AnalyticCheck::HeightDegenerate,
                },
            ));
        }
        nurb_z_axis = nurb_z_axis.unitize_or_z();

        // not a cone - bottom arc's plane is not perpendicular to cone axis
        if !nurb_z_axis.is_parallel_to(bot.frame.z_axis(), ang_tol) {
            return Err(SolidError::surface(
                OP_REBUILD,
                SurfaceFailure::NurbNotAnalytic {
                    check: AnalyticCheck::BottomNotSquareToAxis,
                },
            ));
        }
        // Not upstream (#16982 review): nor the top arc's. `twisted` compares only the
        // x axes and the angles, so a top circle tilted about its x axis passed, and the
        // rebuild recorded a cone (or, for an inside-out one, a placement) the NURB is
        // not. Both ends now meet the one rule above (`ang_tol` in degrees), so each
        // circle's tilt from the centre line is below `ang_tol` degrees (the far circle's
        // from the recorded axis, below `2 ang_tol`)
        if !nurb_z_axis.is_parallel_to(top.frame.z_axis(), ang_tol) {
            return Err(SolidError::surface(
                OP_REBUILD,
                SurfaceFailure::NurbNotAnalytic {
                    check: AnalyticCheck::TopNotSquareToAxis,
                },
            ));
        }

        // when the CenterBot to CenterTop vector opposes the circle Z axis, bInsideOut
        let inside_out = nurb_z_axis.dot(bot.frame.z_axis()) < 0.0;

        // (upstream also computes the STEP Y axis from the top frame's X axis here;
        // it is unused)

        // the fitted end radii (bottom, top) when the NURB's net must lie on the
        // cylinder recorded below, and whether a net already certified on it is kept
        // as it is (the `EFF_ZERO` branch) rather than moved onto it
        let mut net_onto_cylinder = None;

        // when top and bot radius values are nearly equal - make them exactly equal
        if (bot.radius - top.radius).abs() < EFF_ZERO * (1.0 + bot.radius) {
            // Not upstream (#17228): an SmCylinder recorded from this branch is
            // certified as the mean-radius one below is; upstream recorded it
            // whatever the net (an oblique or twisted net within the angle checks)
            if self.cylinder {
                net_onto_cylinder = Some((bot.radius, top.radius, true));
            }
            top.radius = bot.radius;
        } else if self.cylinder {
            // Not upstream (WR-385, tangency-labs/solidcore#713; the fork's 5df5b27):
            // an SmCylinder's radii within SM_EFF_ZERO_SQRT * (1 + r) are its one
            // radius (their mean); further apart, the NURB is no longer a cylinder
            if (bot.radius - top.radius).abs() < EFF_ZERO_SQRT * (1.0 + bot.radius) {
                let mean = 0.5 * (bot.radius + top.radius);
                // the NURB becomes that cylinder too (below), so for coaxial end
                // circles it and the analytic form describe one surface
                net_onto_cylinder = Some((bot.radius, top.radius, false));
                bot.radius = mean;
                top.radius = mean;
            } else {
                return Err(SolidError::surface(
                    OP_REBUILD,
                    SurfaceFailure::NurbNotAnalytic {
                        check: AnalyticCheck::RadiiDiffer,
                    },
                ));
            }
        }

        // Not upstream (WR-385, #17228): the net moved onto the recorded cylinder, about the
        // axis of the placement recorded below so the net and the analytic cylinder
        // share one axis and radius (heights along it run from the placement's end
        // circle to the other one). Computed before any state changes, so a refused net
        // leaves the cone as it was. The placement's axis is its end circle's normal
        // (upstream); when the net is on no cylinder about it, the cylinder about the
        // centre line through both end circles' centres is tried (#17228): a net whose
        // end circles were moved rigidly against each other by a little (within the angle
        // checks) is on that one, up to rounding, and is recorded with it rather than
        // refused
        let mut refit_position = None;
        let moved_net = match net_onto_cylinder {
            Some((bot_radius, top_radius, keep_if_on)) => {
                let (frame, radii) = if inside_out {
                    (&top.frame, (top_radius, bot_radius))
                } else {
                    (&bot.frame, (bot_radius, top_radius))
                };
                let centre_z = if frame.z_axis().dot(nurb_z_axis) < 0.0 {
                    -nurb_z_axis
                } else {
                    nurb_z_axis
                };
                // Both axes square to the centre line (the end circle's x axis is square
                // to its normal, not to the centre line), so the placement's z axis is
                // the centre line itself; the x axis moves by O(t) for a tilt `t`, which
                // shifts the angle origin of the recorded arc by O(t) and the net's
                // distance from the cylinder only by O(t^2)
                let refit_y = centre_z.cross(frame.x_axis());
                let refit = Axis2Placement::new(frame.origin(), refit_y.cross(centre_z), refit_y);
                match self.net_on_axis(frame, height, radii, bot.radius, keep_if_on) {
                    Ok(net) => net,
                    // only a net off the end circle's axis retries on the centre line;
                    // any other failure is not about the axis and stands
                    Err(off)
                        if refit != *frame
                            && matches!(
                                off.surface_failure(),
                                Some(SurfaceFailure::NurbNotAnalytic {
                                    check: AnalyticCheck::NetOffCylinder
                                        | AnalyticCheck::NetRadiusNotPositive
                                })
                            ) =>
                    {
                        match self.net_on_axis(&refit, height, radii, bot.radius, keep_if_on) {
                            Ok(net) => {
                                refit_position = Some(refit);
                                net
                            }
                            Err(_) => return Err(off),
                        }
                    }
                    Err(off) => return Err(off),
                }
            }
            None => None,
        };

        // modify m_dSemiAngleDeg for nonCylinder shapes
        let radius_diff = if inside_out {
            bot.radius - top.radius
        } else {
            top.radius - bot.radius
        };
        if radius_diff.abs() > EFF_ZERO {
            self.semi_angle_deg = radius_diff.atan2(height) * 180.0 / PI;
        }
        // Not upstream (WR-385, #713): an SmCylinder's angle is always 0, so clear any
        // angle a previous rebuild left (its radii are equal here, which skips the
        // update above)
        if self.cylinder {
            self.semi_angle_deg = 0.0;
        }

        // set Analytic parameters
        self.base_radius = if inside_out { top.radius } else { bot.radius };
        self.inside_out = inside_out;
        self.rev.planar_generator = true;
        self.rev.position =
            refit_position.unwrap_or(if inside_out { top.frame } else { bot.frame });
        self.rev.swap_uv = swap_uv;
        if let Some(base) = moved_net {
            self.rev.base = base;
        }
        let _ = self
            .rev
            .anal_uv_domain
            .set_min_max_coords(top.start_ang, 0.0, top.end_ang, height);

        // Make the generator line
        let line_point = self.rev.position.origin() + self.base_radius * self.rev.position.x_axis();
        let line_vec = height * self.rev.position.z_axis()
            + (self.radius(height) - self.base_radius) * self.rev.position.x_axis();
        let length = line_vec.length();
        let mut line = Line::bounded(
            line_point,
            line_vec,
            &Extent1d::from_min_max(0.0, height),
            &BoundedLineOptions {
                scale: length / height,
                dim: 3,
                nurb: None,
                inside_out,
            },
        );

        // set the NurbDomain (status ignored)
        ignore_status(line.edit_parameterization(&linear_ivl))?;

        // Build newCone PolarConverter from Nurb rotation isoParamCurve (status
        // ignored), then save the gencurve
        ignore_status(self.rebuild_cone_polar_converter(k))?;
        self.rev.gen_curve = Some(Curve::from(line));
        Ok(())
    }
}

// ---- sm_GetEllipseTrimIntervals ----------------------------------------------------

/// The location and STEP parameters the cone's `STEPInversion` writes for `point` (its
/// status is ignored upstream). The parameters are only read for a location other
/// than `SM_LT_EXTERIOR`; they are `SM_UNDEF_DOUBLE` where a failure sets that location.
/// Only the answer "off the generator" reads as exterior; any other failure (a failed
/// start-plane transform) is returned, where upstream read some as exterior (WR-662;
/// the SolidCraft fork, tangency-labs/smlib dfc7cb6, in `sm_GetEllipseTrimIntervals`
/// and `sm_IsConePointInTrimDomain`).
fn cone_inversion(
    k: &Kernel,
    cone: &Cone,
    point: Point3,
    tol: f64,
) -> Result<(LocationType, Point2)> {
    match cone.step_inversion(k, point, tol, None) {
        Ok(inv) => Ok((inv.location, inv.uv)),
        // rotated point not on the generator: upstream sets SM_LT_EXTERIOR
        // (by its status, so also a typed failure that replaced the `Upstream`, #13790)
        Err(e)
            if !e.is_unported_or_cancelled()
                && crate::surface::surf_of_revolution::p2::is_off_generator(&e) =>
        {
            Ok((LocationType::Exterior, Point2::UNDEF))
        }
        // the fork (tangency-labs/smlib dfc7cb6, `SER` unless `STEPInversionAnswered`):
        // any other failure is passed on instead of read as SM_LT_EXTERIOR
        Err(e) => Err(e),
    }
}

/// Whether `point` on `cone` lies inside the STEP trim domain `anal_uv_domain`
/// (`sm_IsConePointInTrimDomain`): the cone's STEP inversion ignores the domain and
/// only reports whether the point is inside the whole cone, so the height must be in
/// the domain's v interval and the angle (arbitrary at a pole) in its u interval.
fn is_cone_point_in_trim_domain(
    k: &Kernel,
    cone: &Cone,
    anal_uv_domain: &Extent2d,
    point: Point3,
    tol: f64,
) -> Result<bool> {
    let (loc, step_uv) = cone_inversion(k, cone, point, tol)?;
    if loc == LocationType::Exterior {
        return Ok(false);
    }

    if !anal_uv_domain.v_interval().contains_value(step_uv.y, tol) {
        return Ok(false);
    }

    // the sweep angle is arbitrary at a pole
    Ok(loc == LocationType::Pole
        || anal_uv_domain
            .u_interval()
            .contains_periodic_value(step_uv.x, 360.0, -1.0)
            .contained)
}

/// The segment classification of `sm_GetEllipseTrimIntervals` (`bInside[ii]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Inside {
    /// `FALSE`.
    No,
    /// `TRUE`.
    Yes,
    /// `UNSURE`: a degenerate segment, classified by its neighbours.
    Unsure,
}

/// The intervals (in degrees of the ellipse's STEP domain) of the ellipse
/// `origin + x_radius cos(t) x_axis + y_radius sin(t) y_axis`, known to lie on the
/// cone, that are inside the cone trimmed to its NURB domain `uv_domain`
/// (`sm_GetEllipseTrimIntervals`). The full ellipse is intersected with the trim
/// circles at the v limits of the trim domain (two-point crossings inside the sweep
/// interval only) and the two seam lines (within the larger of the scaled zero and
/// `tol` of their ends, or always when a circle lies in the ellipse plane); the
/// crossing angles, sorted, cut it into segments whose midpoints are classified
/// against the trim domain (a degenerate segment is inside when both neighbours are
/// outside). Every inside segment is intersected periodically with
/// `[start_ang_deg, end_ang_deg]`. Fewer than two crossings give no interval.
///
/// # Errors
///
/// Those of [`Ellipse::new`]; a seam line parallel to the
/// ellipse plane or of zero length (upstream then reads uninitialised values); a STEP
/// inversion failing in the generator solve. A failed NURB to STEP conversion of the
/// domain corners is ignored as upstream does: the default-constructed domain is used.
pub fn ellipse_trim_intervals(
    k: &Kernel,
    ellipse_origin: Point3,
    ellipse_x_axis: Vec3,
    ellipse_y_axis: Vec3,
    ellipse_x_radius: f64,
    ellipse_y_radius: f64,
    ellipse_start_ang_deg: f64,
    ellipse_end_ang_deg: f64,
    cone: &Cone,
    uv_domain: &Extent2d,
    tol: f64,
) -> Result<Vec<Extent1d>> {
    // init output
    let mut ivls = Vec::new();

    // find ellipse/coneTrimBoundary intersections (at most 6)
    let mut xsect_angle: Vec<f64> = Vec::with_capacity(6);

    // ellipse locals
    let ellipse_z = ellipse_x_axis.cross(ellipse_y_axis);
    let ellipse_ivl = Extent1d::from_min_max(ellipse_start_ang_deg, ellipse_end_ang_deg);
    let full_ellipse = Ellipse::new(
        k,
        ellipse_origin,
        ellipse_x_axis,
        ellipse_y_axis,
        Extent1d::from_min_max(0.0, 360.0),
        ellipse_x_radius,
        ellipse_y_radius,
        &EllipseOptions {
            dim: 3,
            ..EllipseOptions::default()
        },
    )?;

    // map given NurbDomain into StepDomain (SmSurface::ConvertDomainFromNURBSToSTEP,
    // status ignored: a failed corner conversion leaves the default-constructed
    // domain, a reversed result is replaced by the NURB domain)
    let mut anal_uv_domain = Extent2d::new();
    if let Some(min_step_uv) = ignored(cone.convert_uv_from_nurbs_to_step(k, uv_domain.min()))? {
        if let Some(max_step_uv) = ignored(cone.convert_uv_from_nurbs_to_step(k, uv_domain.max()))?
        {
            if anal_uv_domain
                .set_min_max_coords(
                    min_step_uv.x,
                    min2(min_step_uv.y, max_step_uv.y),
                    max_step_uv.x,
                    max2(min_step_uv.y, max_step_uv.y),
                )
                .is_err()
            {
                anal_uv_domain = *uv_domain;
            }
        }
    }
    let sweep_ivl = anal_uv_domain.u_interval();

    // cone locals - bottom and top trim circles sit at the v limits of the trim domain,
    //               which may be a subset of the whole cone
    let cone_axis = cone.rev.position.z_axis();
    let bot_height = anal_uv_domain.min().y;
    let top_height = anal_uv_domain.max().y;
    let bot_radius = cone.radius(bot_height);
    let top_radius = cone.radius(top_height);
    let center_bot = cone.rev.position.origin() + bot_height * cone_axis;
    let center_top = cone.rev.position.origin() + top_height * cone_axis;

    // get cone corners [StartLine start, StartLine end, EndLine start EndLine end]
    // (statuses ignored). The seam lines run over the domain's v range, as upstream:
    // since v1.0.1 the trim circles sit at that range's limits, so a seam crossing
    // outside it is not needed (WR-490), and one taken there would only split the
    // ellipse at a point outside the domain.
    let seam_domain = anal_uv_domain;
    let corner = |u: f64, v: f64| -> Result<Point3> {
        Ok(ignored(cone.evaluate_step_point(seam_domain.evaluate(u, v)))?.unwrap_or(Vec3::UNDEF))
    };
    let corners = [
        corner(0.0, 0.0)?,
        corner(0.0, 1.0)?,
        corner(1.0, 0.0)?,
        corner(1.0, 1.0)?,
    ];
    let [corner0, corner1, corner2, corner3] = corners;
    let start_line_vec = corner1 - corner0;
    let end_line_vec = corner3 - corner2;

    // get ellipse/TopCircle(max 2), ellipse/BotCircle(max 2) ellipse/StartSeamLine(max
    // 1), ellipse/EndSeamLine(max 1) intersections (the circle statuses are ignored:
    // their count is 0 then)
    let circle_hits = |radius: f64, center: Point3| -> Result<CurveIntersection> {
        Ok(ignored(circle_plane_intersect(
            tol,
            radius,
            center,
            cone_axis,
            ellipse_origin,
            ellipse_z,
        ))?
        .unwrap_or(CurveIntersection::Points(Vec::new())))
    };
    let mut tsect1 = circle_hits(top_radius, center_top)?;
    let mut tsect2 = circle_hits(bot_radius, center_bot)?;
    let param0 = line_plane_intersect(corner0, start_line_vec, ellipse_origin, ellipse_z)?;
    let param1 = line_plane_intersect(corner2, end_line_vec, ellipse_origin, ellipse_z)?;

    // Check for degenerate solutions, within tol. [B10]
    for tsect in [&mut tsect1, &mut tsect2] {
        if let CurveIntersection::Points(points) = tsect {
            if let [a, b] = points.as_slice() {
                if a.distance(*b) < tol {
                    points.truncate(1);
                }
            }
        }
    }
    let coincident = matches!(tsect1, CurveIntersection::Coincident)
        || matches!(tsect2, CurveIntersection::Coincident);

    // Consider all 4 possible ConeCircle/Ellipse intersections: only two-point
    // intersections, only points in the cone trim boundary (not on its seams)
    for tsect in [&tsect1, &tsect2] {
        let CurveIntersection::Points(points) = tsect else {
            continue;
        };
        if points.len() != 2 {
            continue;
        }
        for &point in points {
            // map XSectPoint to ConeUV point
            let (loc, cone_step_uv) = cone_inversion(k, cone, point, tol)?;
            // skip circle intersections on seams because the line intersections will
            // catch seam intersections
            if loc == LocationType::Interior
                && sweep_ivl
                    .contains_periodic_value(cone_step_uv.x, 360.0, -1.0)
                    .contained
            {
                // add ellipse angleDeg for intersection to in/out angle array
                xsect_angle.push(full_ellipse.step_inversion(k, point, false, 0.0).0);
            }
        }
    }

    // for StartSeamLine/ellipse and EndSeamLine/ellipse xSects
    let scaled_zero = EFF_ZERO * (1.0 + max2(corner0.max_dimension(), corner3.max_dimension()));

    // circle crossings within tol of a seam line were skipped above,
    // so seam line crossings within tol of the line ends must be kept here
    let param_tol = |line_vec: Vec3| {
        let len = line_vec.length();
        if len > EFF_ZERO {
            max2(scaled_zero, tol / len)
        } else {
            scaled_zero
        }
    };
    let start_param_tol = param_tol(start_line_vec);
    let end_param_tol = param_tol(end_line_vec);
    for (line_start, line_vec, param, param_tol) in [
        (corner0, start_line_vec, param0, start_param_tol),
        (corner2, end_line_vec, param1, end_param_tol),
    ] {
        if coincident || (param >= 0.0 - param_tol && param <= 1.0 + param_tol) {
            let point = line_start + param * line_vec;
            xsect_angle.push(full_ellipse.step_inversion(k, point, false, 0.0).0);
        }
    }

    // When there are no intersections the whole curve must lie outside the cone
    // trim domain.
    let cnt = xsect_angle.len();
    if cnt < 2 {
        return Ok(ivls);
    }

    // order the XSectPoints by angle
    shell_sort_double_array(&mut xsect_angle);

    // the start and end angles of segment ii (the last one includes the zero angle:
    // its start is moved down by 360)
    let segment = |ii: usize| -> Result<(f64, f64)> {
        let next = (ii + 1) % cnt;
        let this_ang = *xsect_angle
            .get(ii)
            .ok_or(SolidError::invalid_input(OP_TRIM, "segment index"))?;
        let next_ang = *xsect_angle
            .get(next)
            .ok_or(SolidError::invalid_input(OP_TRIM, "segment index"))?;
        let start = if next == 0 {
            this_ang - 360.0
        } else {
            this_ang
        };
        Ok((start, next_ang))
    };

    // now classify the segment midPoints as in/out
    let scaled_ang_zero = EFF_ZERO * 100.0 * 360.0;
    let mut inside = Vec::with_capacity(cnt);
    for ii in 0..cnt {
        let (start_ang_deg, end_ang_deg) = segment(ii)?;

        // mark degenerate intervals as unknown for now
        if end_ang_deg - start_ang_deg < scaled_ang_zero {
            inside.push(Inside::Unsure);
            continue;
        }

        // classify nonDegenerate interval midPoint
        let mid_ang_deg = (end_ang_deg + start_ang_deg) / 2.0;
        let mid_point = full_ellipse.evaluate_step_point(mid_ang_deg)?;

        // Remember if segment MidPointUV is within cone trim boundaries
        inside.push(
            if is_cone_point_in_trim_domain(k, cone, &anal_uv_domain, mid_point, tol)? {
                Inside::Yes
            } else {
                Inside::No
            },
        );
    }

    // add one interval to output for every inside segment
    for ii in 0..cnt {
        let next = (ii + 1) % cnt;
        let prev = (ii + cnt - 1) % cnt;
        let state = |i: usize| inside.get(i).copied();
        let (start_ang_deg, end_ang_deg) = segment(ii)?;

        // when the segment is inside
        if state(ii) == Some(Inside::Yes)
            || (state(ii) == Some(Inside::Unsure)
                && state(prev) == Some(Inside::No)
                && state(next) == Some(Inside::No))
        {
            // make and trim interval to ellipse interval (AddValue: start can be above
            // end by machine noise)
            let mut untrimmed_ivl = Extent1d::new();
            untrimmed_ivl.add_value(start_ang_deg);
            untrimmed_ivl.add_value(end_ang_deg);
            ivls.extend(ellipse_ivl.intersect_periodic(&untrimmed_ivl, 360.0, false)?);
        }
    }
    Ok(ivls)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::axis2_placement::Axis2Placement;

    fn kernel() -> Kernel {
        Kernel::default()
    }

    /// The r 40 corner of the battery case's plan outline (#13536): a 0.156 rad arc about
    /// (-133.12369073537272, -163.5) extruded along z from -1 to 93, its NURBS form as the
    /// record holds it, which SolidCraft's import recognises. The cylinder recogniser finds the
    /// cylinder on the form at bound 0, with the extrusion's axis. (Their end circles were
    /// fitted through samples: its axis tilted 1.44e-13 rad, 1.35e-11 mm
    /// off the form at the top, so the case wall had no exact class and its shell was
    /// refused.)
    #[test]
    fn an_extruded_short_arc_is_recognised_on_its_form() {
        use crate::nurbs::math::CPoint;
        use crate::nurbs::srf_basic::Surface as Nurb;
        use crate::surface::implicit::{lies_on, AnalyticSupport};
        let k = kernel();
        let w = 0.996_964_674_751_439_2;
        // (x w, y w, w) of each column, at z = -1 and 93
        let columns = [
            (-133.123_690_735_372_72, -203.5, 1.0),
            (-129.605_407_717_687_06, -202.882_311_311_917_87, w),
            (-126.914_177_375_687_37, -203.015_085_016_179_46, 1.0),
        ];
        let net = columns
            .into_iter()
            .map(|(x, y, w)| vec![CPoint::new(x, y, -w, w), CPoint::new(x, y, 93.0 * w, w)])
            .collect();
        let knot = 6.234_728_055_932_869;
        let nurb = Nurb::new(
            net,
            2,
            1,
            vec![0.0, 0.0, 0.0, knot, knot, knot],
            vec![0.0, 0.0, 94.0, 94.0],
        )
        .unwrap();
        let form = BSplineSurface::from_nurb(nurb, false);
        let found = [Cone::is_nurb_surface_cylinder(&k, &form, 1.0).unwrap()];
        for cone in found {
            let surface = crate::surface::surface::Surface::from(cone.expect("a cylinder"));
            let support = AnalyticSupport::of(&surface).expect("an analytic surface");
            assert!(lies_on(&form, &support.implicit()).unwrap(), "{support:?}");
            let (AnalyticSupport::Cylinder { placement, .. }
            | AnalyticSupport::Cone { placement, .. }) = support
            else {
                panic!("{support:?}");
            };
            let axis = placement.z_axis();
            assert!(axis.x.abs() < 1e-15 && axis.y.abs() < 1e-15, "{axis:?}");
        }
    }

    /// #16488: a wall of radius 201 mm over a 0.046 degree arc, extruded 100 mm along x from
    /// x = -95 (a body moved 95 mm along its cylinder's axis), its NURBS form exact to
    /// rounding. The recogniser's frame is made from the form's ruling, so the generator it
    /// starts lies on the form at its start corner: the kernel's check compares the two to
    /// 2.8e-8 mm, and a normal found from the legs of so short an arc is tilted by about
    /// 1.6e-10 rad, 3.3e-8 mm at that radius. The second form has a very short first
    /// segment followed by a wide one: its frame does not depend on the first segment's leg.
    #[test]
    fn an_extruded_very_short_arc_has_its_generator_on_the_form() {
        use crate::nurbs::math::CPoint;
        use crate::nurbs::srf_basic::Surface as Nurb;
        use crate::surface::implicit::{lies_on, AnalyticSupport};
        let k = kernel();
        let (radius, start) = (201.0_f64, 1.234_f64);
        let (cy, cz) = (-107.916_728_995_484_4, 248.198_661_548_332_04);
        let on = |a: f64, r: f64| (cy + r * a.cos(), cz + r * a.sin());
        let short = 0.046_f64.to_radians();
        for sweeps in [vec![short], vec![short, 0.7]] {
            // the arc's points (y, z) and weights: each segment's ends then its middle
            let mut arc = vec![(on(start, radius), 1.0)];
            let mut knots = vec![0.0; 3];
            let mut at = start;
            for sweep in &sweeps {
                let w = (0.5 * sweep).cos();
                arc.push((on(at + 0.5 * sweep, radius / w), w));
                at += sweep;
                arc.push((on(at, radius), 1.0));
                knots.extend([(at - start) * radius; 2]);
            }
            knots.truncate(knots.len() - 2);
            knots.extend([(at - start) * radius; 3]);
            let net = arc
                .into_iter()
                .map(|((y, z), w)| {
                    vec![
                        CPoint::new(-95.0 * w, y * w, z * w, w),
                        CPoint::new(5.0 * w, y * w, z * w, w),
                    ]
                })
                .collect();
            let nurb = Nurb::new(net, 2, 1, knots, vec![0.0, 0.0, 100.0, 100.0]).unwrap();
            let form = BSplineSurface::from_nurb(nurb, false);
            let cone = Cone::is_nurb_surface_cylinder(&k, &form, 1.0)
                .unwrap()
                .expect("a cylinder");
            let surface = crate::surface::surface::Surface::from(cone);
            let support = AnalyticSupport::of(&surface).expect("an analytic surface");
            assert!(lies_on(&form, &support.implicit()).unwrap(), "{support:?}");
            let (AnalyticSupport::Cylinder {
                placement,
                radius: r,
            }
            | AnalyticSupport::Cone {
                placement,
                base_radius: r,
                ..
            }) = support
            else {
                panic!("{support:?}");
            };
            let axis = placement.z_axis();
            assert!(axis.y.abs() < 1e-15 && axis.z.abs() < 1e-15, "{axis:?}");
            // the start corner at x = -95 is on the generator the cylinder starts
            let (y, z) = on(start, radius);
            let from_axis = placement.origin() + r * placement.x_axis() - Vec3::new(-95.0, y, z);
            assert!(from_axis.cross(axis).length() < 1e-11, "{from_axis:?}");
        }
    }

    /// The #17462 wall's NURBS form (see the test below), its middle control points moved
    /// by `nudge` (in both rows, so the rows stay translates).
    fn short_arc_wall(nudge: Vec3) -> BSplineSurface {
        use crate::nurbs::math::CPoint;
        use crate::nurbs::srf_basic::Surface as Nurb;
        let w = 0.999_999_998_985_303_8;
        // (x w, y w, z w, w), as the record holds them
        let row = |p: [[f64; 3]; 3]| -> Vec<CPoint> {
            p.iter()
                .zip([(1.0, Vec3::ZERO), (w, nudge * w), (1.0, Vec3::ZERO)])
                .map(|(&[x, y, z], (w, d))| CPoint::new(x + d.x, y + d.y, z + d.z, w))
                .collect()
        };
        let net = vec![
            row([
                [4.770_516_121_717_348, -29.0, 58.638_980_067_614_24],
                [
                    4.774_450_686_714_966,
                    -28.999_999_970_573_803,
                    58.639_563_676_146_95,
                ],
                [4.778_385_209_010_169, -29.0, 58.640_147_758_206_49],
            ]),
            row([
                [4.770_516_121_717_353, -82.0, 58.638_980_067_614_24],
                [
                    4.774_450_686_714_971,
                    -81.999_999_916_794_9,
                    58.639_563_676_146_95,
                ],
                [4.778_385_209_010_175, -82.0, 58.640_147_758_206_49],
            ]),
        ];
        let knot = 0.007_955_252_114_514_06;
        let nurb = Nurb::new(
            net,
            1,
            2,
            vec![0.0, 0.0, 53.0, 53.0],
            vec![0.0, 0.0, 0.0, knot, knot, knot],
        )
        .unwrap();
        BSplineSurface::from_nurb(nurb, false)
    }

    /// #17462: the wall a linear sweep made from 0.008 mm of a spline, a 0.005 degree arc of
    /// radius 88 mm swept 53 mm along -y, its NURBS form as the app's flat arrays carry it
    /// (rows exact translates). The legs' normal of so short an arc is 2e-8 rad off the
    /// ruling, past the angular zero both recoveries held it to, so the form, which is the
    /// cylinder's own, was refused. It is a cylinder on the form with the ruling's axis.
    #[test]
    fn a_very_short_extruded_arc_is_a_cylinder_whatever_its_legs_normal() {
        use crate::surface::implicit::{lies_on, AnalyticSupport};
        let k = kernel();
        let form = short_arc_wall(Vec3::ZERO);
        let cone = Cone::is_nurb_surface_cylinder(&k, &form, 1.0)
            .unwrap()
            .expect("a cylinder");
        let surface = crate::surface::surface::Surface::from(cone);
        let support = AnalyticSupport::of(&surface).expect("an analytic surface");
        assert!(lies_on(&form, &support.implicit()).unwrap(), "{support:?}");
        let (AnalyticSupport::Cylinder { placement, .. } | AnalyticSupport::Cone { placement, .. }) =
            support
        else {
            panic!("{support:?}");
        };
        let axis = placement.z_axis();
        assert!(axis.x.abs() < 1e-15 && axis.z.abs() < 1e-15, "{axis:?}");
    }

    /// #18415 review: the chord recovery holds the arc's control points to the circle only
    /// within `pt_tol` (1.4e-8 here). The #17462 wall with its middle control points moved
    /// 5e-9 mm off the circle, square to the chord in the arc's plane, passes every one of
    /// those checks, but its net is not the cylinder's: the copy is refused rather than
    /// labelled as that cylinder, and the surface stays a B-spline.
    #[test]
    fn a_short_arc_wall_off_its_circle_by_less_than_the_zero_is_no_cylinder() {
        let k = kernel();
        let chord = Vec3::new(
            4.778_385_209_010_169 - 4.770_516_121_717_348,
            0.0,
            58.640_147_758_206_49 - 58.638_980_067_614_24,
        );
        let across = Vec3::new(-chord.z, 0.0, chord.x).unitize().unwrap();
        let wall = short_arc_wall(across * 5e-9);
        assert!(Cone::is_nurb_surface_cylinder(&k, &wall, 1.0)
            .unwrap()
            .is_none());
    }

    /// #16488: a form whose rows are not exact translates. A unit-radius arc of 1 rad about
    /// the x axis, extruded 1 mm along x, whose top row is turned about the axis by `turn`.
    /// The recogniser holds the arcs' X axes to `EFF_ZERO_SQRT` radians, so a turn of half
    /// that is a cylinder whose axis is the ruling's direction (tilted from x by about the
    /// turn), and a turn of four times it is not a cylinder.
    #[test]
    fn a_top_row_turned_about_the_axis_is_a_cylinder_within_the_angle_tolerance_only() {
        use crate::nurbs::math::CPoint;
        use crate::nurbs::srf_basic::Surface as Nurb;
        use crate::surface::implicit::AnalyticSupport;
        let k = kernel();
        let sweep = 1.0_f64;
        let w = (0.5 * sweep).cos();
        let row = |turn: f64, x: f64| -> Vec<CPoint> {
            [
                (0.0, 1.0, 1.0),
                (0.5 * sweep, 1.0 / w, w),
                (sweep, 1.0, 1.0),
            ]
            .iter()
            .map(|&(a, r, weight): &(f64, f64, f64)| {
                let (y, z) = (r * (a + turn).cos(), r * (a + turn).sin());
                CPoint::new(x * weight, y * weight, z * weight, weight)
            })
            .collect()
        };
        let form = |turn: f64| {
            let (bot, top) = (row(0.0, 0.0), row(turn, 1.0));
            let net = (0..3).map(|i| vec![bot[i], top[i]]).collect();
            let nurb = Nurb::new(
                net,
                2,
                1,
                vec![0.0, 0.0, 0.0, sweep, sweep, sweep],
                vec![0.0, 0.0, 1.0, 1.0],
            )
            .unwrap();
            BSplineSurface::from_nurb(nurb, false)
        };
        let near = Cone::is_nurb_surface_cylinder(&k, &form(0.5 * EFF_ZERO_SQRT), 1.0)
            .unwrap()
            .expect("a cylinder within the angle tolerance");
        let surface = crate::surface::surface::Surface::from(near);
        let support = AnalyticSupport::of(&surface).expect("an analytic surface");
        let (AnalyticSupport::Cylinder { placement, .. } | AnalyticSupport::Cone { placement, .. }) =
            support
        else {
            panic!("{support:?}");
        };
        let tilt = (placement.z_axis() - Vec3::new(1.0, 0.0, 0.0)).length();
        assert!(tilt > 0.0 && tilt < 2.0 * EFF_ZERO_SQRT, "{tilt:e}");
        assert!(
            Cone::is_nurb_surface_cylinder(&k, &form(4.0 * EFF_ZERO_SQRT), 1.0)
                .unwrap()
                .is_none()
        );
    }

    /// A canonical cylinder of radius 2.94 (the goldens' gen0196), its net reaching ±1.2e6
    /// along its axis, recognised at a thousandth of the default tolerance. Its control points
    /// are known to the rounding of coordinates that large (about 2.3e-10), which turns its
    /// middle circle's plane by about 2.5e-11 rad, more than that scaled tolerance (1.75e-11
    /// rad): the circle checks take at least the angle that rounding allows, so the cylinder is
    /// still found (#13536).
    fn canonical(k: &Kernel, bot: f64, top: f64, h: f64, swap: bool, inside: bool) -> Cone {
        Cone::create_canonical(k, &Axis2Placement::IDENTITY, bot, top, h, swap, inside).unwrap()
    }

    /// fork dfc7cb6 (`sm_GetEllipseTrimIntervals` / `sm_IsConePointInTrimDomain`,
    /// `if(!SmSurface::STEPInversionAnswered(eInv)) { SER(eInv) ; }`): a point off the
    /// generator is an answer, read as exterior, and a point on the cone inverts.
    /// Coverage only: the failure passed on (a failed start-plane transform,
    /// `CCWAngleBetween` on a zero projection) is screened out by the transform's own
    /// axis and projection checks for every finite input.
    #[test]
    fn cone_inversion_answers_read_as_locations() {
        let k = kernel();
        let cone = canonical(&k, 2.0, 2.0, 3.0, false, false);
        let off = Point3::new(3.0, 0.0, 1.0);
        assert_eq!(
            cone_inversion(&k, &cone, off, 1e-9).unwrap().0,
            LocationType::Exterior
        );
        let on = Point3::new(0.0, 2.0, 1.0);
        assert_ne!(
            cone_inversion(&k, &cone, on, 1e-9).unwrap().0,
            LocationType::Exterior
        );
    }

    #[test]
    fn recognises_its_own_nurb() {
        let k = kernel();
        for (swap, inside) in [(false, false), (true, false), (false, true), (true, true)] {
            let cone = canonical(&k, 2.0, 1.0, 3.0, swap, inside);
            let found = Cone::is_nurb_surface_cone(&k, cone.rev().base(), 1.0)
                .unwrap()
                .unwrap();
            assert!((found.bot_radius() - cone.bot_radius()).abs() < 1e-9);
            assert!((found.top_radius() - cone.top_radius()).abs() < 1e-9);
            assert_eq!(found.rev().swap_uv(), swap);
        }
    }

    #[test]
    fn trim_with_domain_keeps_the_shape() {
        let k = kernel();
        for (swap, inside) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut cone = canonical(&k, 2.0, 1.0, 3.0, swap, inside);
            let d = cone.rev().base().natural_uv_domain();
            let (lo, hi) = (d.min(), d.max());
            let trim = Extent2d::from_bounds(
                lo.x + 0.25 * (hi.x - lo.x),
                lo.y + 0.125 * (hi.y - lo.y),
                lo.x + 0.75 * (hi.x - lo.x),
                lo.y + 0.5 * (hi.y - lo.y),
            );
            cone.trim_with_domain(&k, &trim).unwrap();
            let nd = cone.rev().base().natural_uv_domain();
            assert!(nd.is_contained_by(&trim, 1e-12) && trim.is_contained_by(&nd, 1e-12));
            let p = cone.rev().base().evaluate_point(trim.max()).unwrap();
            let back = cone.convert_uv_from_nurbs_to_step(&k, trim.max()).unwrap();
            let q = cone.evaluate_step_point(back).unwrap();
            assert!((p - q).length() < 1e-9);
            // the cone through the trimmed corner keeps its radius at that height
            let r = (p.x * p.x + p.y * p.y).sqrt();
            assert!((r - cone.radius(p.z)).abs() < 1e-9);
            // a domain outside the current one is rejected
            assert!(cone.trim_with_domain(&k, &d).is_err());
        }
    }

    /// An end iso curve that cannot be made is told apart from ends that are not circular:
    /// its construction's error comes back for the rebuild to chain as its cause.
    #[test]
    fn end_circles_keep_an_iso_construction_error() {
        let k = kernel();
        let cone = canonical(&k, 2.0, 1.0, 3.0, false, false);
        let base = cone.rev().base();
        let d = base.natural_uv_domain();
        let (lo, hi) = (d.min().y, d.max().y);
        let tol = 1e-9;
        assert!(matches!(
            end_circles(&k, base, SurfParamType::V, hi, lo, tol).unwrap(),
            EndCircles::Found(..)
        ));
        // a knot far outside the domain: the iso curve's construction fails
        let far = hi + 1.0e6 * (hi - lo);
        let EndCircles::IsoFailed(e) =
            end_circles(&k, base, SurfParamType::V, far, lo, tol).unwrap()
        else {
            panic!("an iso curve outside the domain is not made");
        };
        assert!(!e.is_unported_or_cancelled(), "{e}");
        // a flat net's ends are lines: not circular, and no error
        let flat = BSplineSurface::create_bilinear_surface(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
        )
        .unwrap();
        assert!(matches!(
            end_circles(&k, &flat, SurfParamType::V, 1.0, 0.0, tol).unwrap(),
            EndCircles::NotCircular
        ));
    }

    #[test]
    fn rejects_non_cones() {
        let k = kernel();
        let bs = BSplineSurface::create_bilinear_surface(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
        )
        .unwrap();
        assert!(Cone::is_nurb_surface_cone(&k, &bs, 1.0).unwrap().is_none());
    }

    #[test]
    fn rebuild_keeps_the_cone() {
        let k = kernel();
        let mut cone = canonical(&k, 2.0, 1.0, 3.0, false, false);
        let before = cone.clone();
        cone.rebuild_step_from_nurb_parameters(&k).unwrap();
        assert!((cone.base_radius() - before.base_radius()).abs() < 1e-9);
        assert!((cone.semi_angle_deg() - before.semi_angle_deg()).abs() < 1e-7);
    }

    // Edit the actual homogeneous NURB net, as SwapUV/canonicalisation can do,
    // while retaining the object's SmCylinder class and its old analytic state.
    fn edit_last_radius(cone: &mut Cone, factor: f64) {
        let swap = cone.rev().swap_uv();
        let s = cone.base_mut().get_or_create_gw_nurb().unwrap();
        let (n, m) = (s.n(), s.m());
        for i in 0..=n {
            for j in 0..=m {
                if if swap { i == n } else { j == m } {
                    let p = s.cpt_mut(i, j).unwrap();
                    p.x *= factor;
                    p.y *= factor;
                }
            }
        }
    }

    fn edited_cylinder(k: &Kernel, radius: f64, top: f64, swap: bool, inside: bool) -> Cone {
        edited_cylinder_at(k, Vec3::ZERO, radius, top, swap, inside)
    }

    fn edited_cylinder_at(
        k: &Kernel,
        origin: Point3,
        radius: f64,
        top: f64,
        swap: bool,
        inside: bool,
    ) -> Cone {
        let mut c = Cone::new_cylinder_bounded(
            origin,
            Vec3::X,
            Vec3::Y,
            radius,
            0.0,
            360.0,
            3.0,
            swap,
            inside,
            false,
        );
        c.make_nurb(k).unwrap();
        edit_last_radius(&mut c, top / radius);
        c
    }

    #[test]
    fn rebuild_cylinder_roundoff_preserves_zero_semi_angle() {
        let k = kernel();
        // The radii recovered in issue #713, plus both sides of the rounding
        // allowance at different model scales and in both parameter directions.
        for (radius, top) in [(9.0000000000985, 9.0000000000757)].into_iter().chain(
            [0.009, 9.0, 90_000.0]
                .into_iter()
                .flat_map(|r| [-0.5, 0.5].map(|sign| (r, r + sign * EFF_ZERO_SQRT * (1.0 + r)))),
        ) {
            for swap in [false, true] {
                for inside in [false, true] {
                    let mut c = edited_cylinder(&k, radius, top, swap, inside);
                    c.rebuild_step_from_nurb_parameters(&k).unwrap();
                    assert!(c.is_cylinder_class());
                    assert_eq!(c.semi_angle_deg(), 0.0, "{radius} {top} {swap} {inside}");
                    let mean = (radius + top) / 2.0;
                    assert!((c.base_radius() - mean).abs() < 1e-12 * (1.0 + mean));
                    assert_eq!(c.bot_radius(), c.top_radius());
                }
            }
        }
    }

    /// WR-385 (#16982 review): a cylinder rebuilt from near-equal end radii keeps its
    /// NURB on the analytic cylinder. Every control point (weight divided out) lies the
    /// cylinder's radius from its axis, or `sqrt 2` times it for a corner of weight
    /// `sqrt(2)/2`, up to rounding. Before, the net kept the unequal end radii, half the
    /// difference off the analytic surface (2.5e-6 for r 9, about 2.5e-7 (1 + r)).
    ///
    /// The guard is MEASURED, not a bound: the move maps a point at radius `rho` to
    /// `rho * mean / here`, so any gap between the input net and the fitted end
    /// circles (`here`) reaches the output, and neither the net's construction and
    /// edit rounding nor the circle fit's is counted. It is sized like a rounding count
    /// of the move and the measure alone, in the object's frame
    /// (`S = |o| + 1.5 r + h`): per coordinate the read (divide by `w`) 1, `h` 5 (`p - c`
    /// and the dot product), `z h` 1, `d - z h` 1, the scale 4, applying it 1, the
    /// recompose 2, store and read back 2, the non-unit frame axis 4, the measure 10,
    /// `want` and the difference 3: `sqrt 3 gamma(34) S`, rounded outward. The worst
    /// case seen is under a tenth of it. The rebuilt surface is also certified on the
    /// analytic cylinder with `surface::implicit::lies_on`.
    #[test]
    fn rebuild_cylinder_moves_its_net_onto_the_mean_radius() {
        use crate::nurbs::b_spline_curve::ControlPointForm::EuclideanRational;
        use crate::nurbs::math::gamma;
        let k = kernel();
        let height = 3.0;
        let mut worst: f64 = 0.0;
        // (the offset is along the axis: `edit_last_radius` scales x and y about the
        // world origin, so a sideways offset would tilt the edited cylinder)
        for origin in [Vec3::ZERO, Vec3::new(0.0, 0.0, 1000.0)] {
            for (radius, top) in [(9.0000000000985, 9.0000000000757)].into_iter().chain(
                [0.009, 9.0, 90_000.0].into_iter().flat_map(|r| {
                    [-0.5, 0.5].map(|sign| (r, r + sign * EFF_ZERO_SQRT * (1.0 + r)))
                }),
            ) {
                for swap in [false, true] {
                    for inside in [false, true] {
                        let mut c = edited_cylinder_at(&k, origin, radius, top, swap, inside);
                        c.rebuild_step_from_nurb_parameters(&k).unwrap();
                        let r = c.base_radius();
                        let pos = c.rev().position();
                        let (o, z) = (pos.origin(), pos.z_axis());
                        let scale = o.length() + 1.5 * r + height;
                        let bound = 3f64.sqrt() * gamma(34) * scale * (1.0 + 8.0 * f64::EPSILON);
                        let s = c.rev().base();
                        let cylinder = implicit::Implicit::Cone {
                            origin: o,
                            axis: z,
                            radius: r,
                            tan_angle: 0.0,
                        };
                        assert!(
                            implicit::lies_on(s, &cylinder).unwrap(),
                            "{origin:?} {radius} {top} {swap} {inside}: not on the cylinder"
                        );
                        let (nu, nv) = (
                            s.number_control_points(SurfParamType::U),
                            s.number_control_points(SurfParamType::V),
                        );
                        for i in 0..nu {
                            for j in 0..nv {
                                let (p, w) = s.control_point(EuclideanRational, i, j).unwrap();
                                let d = p - o;
                                let dist = (d - z * d.dot(z)).length();
                                let want = if w == 1.0 { r } else { r * 2f64.sqrt() };
                                let off = (dist - want).abs();
                                worst = worst.max(off / bound);
                                assert!(
                                    off <= bound,
                                    "{origin:?} {radius} {top} {swap} {inside} ({i}, {j}): \
                                     {dist} vs {want}, bound {bound}"
                                );
                            }
                        }
                    }
                }
            }
        }
        println!("worst {worst} of the bound");
    }

    /// #16982 review (CodeRabbit): `twisted` lets the end rows turn against each other
    /// about the axis by up to `ang_tol` (1e-3 rad for the x axes), and radial scaling
    /// keeps that turn, so the middle section of a net twisted by `phi` lies at
    /// `r cos(phi / 2)`, inside the cylinder. A cylinder with near-equal radii whose top
    /// row is turned by 1e-4 rad (middle section 1.1e-8 inside at r 9) is now refused,
    /// unchanged, because the moved net fails `lies_on`; before, it rebuilt as the
    /// cylinder. Untwisted, the same radii rebuild.
    /// A cylinder of radius 9 whose top row is scaled to near-equal radius (the
    /// mean-radius branch) and turned by `turn` rad about the axis.
    fn twisted_cylinder(k: &Kernel, turn: f64, swap: bool, inside: bool) -> Cone {
        let (radius, top) = (9.0, 9.0 + 0.5 * EFF_ZERO_SQRT * 10.0);
        let (sin, cos) = turn.sin_cos();
        let mut c = edited_cylinder(k, radius, top, swap, inside);
        // turn the top row about the axis (z), in homogeneous coordinates
        let s = c.base_mut().get_or_create_gw_nurb().unwrap();
        let (n, m) = (s.n(), s.m());
        for i in 0..=n {
            for j in 0..=m {
                if if swap { i == n } else { j == m } {
                    let p = s.cpt_mut(i, j).unwrap();
                    let (x, y) = (p.x, p.y);
                    p.x = x * cos - y * sin;
                    p.y = x * sin + y * cos;
                }
            }
        }
        c
    }

    /// The NURB's control points (weights divided out) and weights, row by row.
    fn net_of(c: &Cone) -> Vec<(Point3, f64)> {
        use crate::nurbs::b_spline_curve::ControlPointForm::EuclideanRational;
        let s = c.rev().base();
        let (nu, nv) = (
            s.number_control_points(SurfParamType::U),
            s.number_control_points(SurfParamType::V),
        );
        (0..nu)
            .flat_map(|i| (0..nv).map(move |j| (i, j)))
            .map(|(i, j)| s.control_point(EuclideanRational, i, j).unwrap())
            .collect()
    }

    #[test]
    fn rebuild_cylinder_refuses_a_twisted_net() {
        let k = kernel();
        for (turn, refused) in [(1e-4_f64, true), (0.0, false)] {
            for swap in [false, true] {
                for inside in [false, true] {
                    let mut c = twisted_cylinder(&k, turn, swap, inside);
                    let before = c.clone();
                    let result = c.rebuild_step_from_nurb_parameters(&k);
                    assert_eq!(result.is_err(), refused, "{turn} {swap} {inside}");
                    if refused {
                        assert_eq!(c.base_radius(), before.base_radius());
                        assert_eq!(c.semi_angle_deg(), before.semi_angle_deg());
                        assert_eq!(c.rev().position(), before.rev().position());
                    }
                }
            }
        }
    }

    /// #16982 review (CodeRabbit): `Surface::reverse` of a cone reverses the NURB and
    /// rebuilds; upstream ignores the rebuild's status, so a refused rebuild left the
    /// reversed NURB under the old analytic data and reported success. Reversing a
    /// twisted cylinder (a net the rebuild refuses) now fails and leaves the surface as
    /// it was, NURB and analytic data alike, in either direction; the untwisted
    /// cylinder still reverses.
    #[test]
    fn reverse_of_an_uncertifiable_cylinder_fails_unchanged() {
        use crate::surface::surface::{Surface, SurfaceKind};
        let k = kernel();
        for (turn, refused) in [(1e-4_f64, true), (0.0, false)] {
            for swap in [false, true] {
                for inside in [false, true] {
                    for param in [SurfParamType::U, SurfParamType::V] {
                        let c = twisted_cylinder(&k, turn, swap, inside);
                        let (net, radius, angle, position) = (
                            net_of(&c),
                            c.base_radius(),
                            c.semi_angle_deg(),
                            *c.rev().position(),
                        );
                        let mut s = Surface::from(c);
                        let result = s.reverse(&k, param);
                        let case = format!("{turn} {swap} {inside} {param:?}");
                        assert_eq!(result.is_err(), refused, "{case}");
                        let SurfaceKind::Cone(after) = s.kind() else {
                            panic!("{case}: not a cone");
                        };
                        if refused {
                            assert_eq!(net_of(after), net, "{case}");
                            assert_eq!(after.base_radius(), radius, "{case}");
                            assert_eq!(after.semi_angle_deg(), angle, "{case}");
                            assert_eq!(*after.rev().position(), position, "{case}");
                        } else {
                            assert_ne!(net_of(after), net, "{case}");
                        }
                    }
                }
            }
        }
    }

    /// #17228: radii within `EFF_ZERO (1 + r)` took the branch that copies the bottom
    /// radius and recorded the cylinder whatever the net. A net of exactly equal radii
    /// whose top row is turned 1e-4 rad (middle section 9 (1 - cos 5e-5) = 1.1e-8
    /// inside), or moved 0.01 sideways on a 1000 long cylinder (5.7e-4 degrees off
    /// the axis, under the 1e-3 degree angle checks), is now refused with the cone,
    /// its net and its analytic data unchanged; before, both rebuilt as the right
    /// cylinder. Neither is on the cylinder about the centre line either (the turn is
    /// about it; the moved top leaves both circles tilted 1e-5 rad from it, `9 t^2 / 2`
    /// = 4.5e-10 off it, far beyond rounding at this size; `surface.cone_p2` `gen0152`,
    /// tilted 1.4e-7 rad at r 2.8 with coordinates near 1e6, is within rounding of its
    /// centre line and rebuilds there). The untouched cylinder still rebuilds, its net kept
    /// bit for bit (certified on the cylinder as it is).
    #[test]
    fn rebuild_cylinder_within_eff_zero_certifies_the_net() {
        let k = kernel();
        // (turn in rad, sideways shift of the top row, refused)
        for (turn, shift, refused) in [(1e-4_f64, 0.0, true), (0.0, 0.01, true), (0.0, 0.0, false)]
        {
            for swap in [false, true] {
                for inside in [false, true] {
                    let mut c = Cone::new_cylinder_bounded(
                        Vec3::ZERO,
                        Vec3::X,
                        Vec3::Y,
                        9.0,
                        0.0,
                        360.0,
                        1000.0,
                        swap,
                        inside,
                        false,
                    );
                    c.make_nurb(&k).unwrap();
                    let (sin, cos) = turn.sin_cos();
                    let s = c.base_mut().get_or_create_gw_nurb().unwrap();
                    let (n, m) = (s.n(), s.m());
                    for i in 0..=n {
                        for j in 0..=m {
                            if if swap { i == n } else { j == m } {
                                // turn about z and shift along x, in homogeneous form
                                let p = s.cpt_mut(i, j).unwrap();
                                let (x, y) = (p.x, p.y);
                                p.x = x * cos - y * sin + shift * p.w;
                                p.y = x * sin + y * cos;
                            }
                        }
                    }
                    let (net, radius, angle, position) = (
                        net_of(&c),
                        c.base_radius(),
                        c.semi_angle_deg(),
                        *c.rev().position(),
                    );
                    let result = c.rebuild_step_from_nurb_parameters(&k);
                    let case = format!("{turn} {shift} {swap} {inside}");
                    assert_eq!(result.is_err(), refused, "{case}: {result:?}");
                    assert_eq!(net_of(&c), net, "{case}");
                    if let Err(e) = &result {
                        assert_eq!(
                            e.surface_failure(),
                            Some(&SurfaceFailure::NurbNotAnalytic {
                                check: AnalyticCheck::NetOffCylinder
                            }),
                            "{case}"
                        );
                        assert_eq!(c.base_radius(), radius, "{case}");
                        assert_eq!(c.semi_angle_deg(), angle, "{case}");
                        assert_eq!(*c.rev().position(), position, "{case}");
                    } else {
                        assert!(c.is_cylinder_class() && c.semi_angle_deg() == 0.0, "{case}");
                    }
                }
            }
        }
    }

    /// #17228 (`surface.cone_p2` `gen0291`): a top row moved rigidly sideways by a
    /// little, 1e-8 on a 1000 long cylinder of radius 9, leaves the net on no cylinder
    /// about the bottom circle's normal (the top circle is 1e-8 off it, far beyond
    /// rounding), but on the cylinder about the centre line through both circles'
    /// centres: tilted `t` = 1e-11 rad from it, each circle is within `9 t^2 / 2` =
    /// 4.5e-22 of it, below rounding. The rebuild records that cylinder, the net kept bit
    /// for bit and certified on it, rather than refusing a cylinder.
    #[test]
    fn rebuild_cylinder_records_the_centre_line_of_a_slightly_moved_top() {
        let k = kernel();
        let shift = 1e-8;
        for swap in [false, true] {
            for inside in [false, true] {
                let mut c = Cone::new_cylinder_bounded(
                    Vec3::ZERO,
                    Vec3::X,
                    Vec3::Y,
                    9.0,
                    0.0,
                    360.0,
                    1000.0,
                    swap,
                    inside,
                    false,
                );
                c.make_nurb(&k).unwrap();
                let s = c.base_mut().get_or_create_gw_nurb().unwrap();
                let (n, m) = (s.n(), s.m());
                let mut moved_z = 0.0;
                for i in 0..=n {
                    for j in 0..=m {
                        if if swap { i == n } else { j == m } {
                            let p = s.cpt_mut(i, j).unwrap();
                            p.x += shift * p.w;
                            moved_z = p.z / p.w;
                        }
                    }
                }
                let net = net_of(&c);
                let case = format!("{swap} {inside}");
                // not on the cylinder about the bottom circle's normal
                let on_normal = implicit::lies_on(
                    &c.rev().base,
                    &implicit::Implicit::Cone {
                        origin: Vec3::ZERO,
                        axis: Vec3::Z,
                        radius: 9.0,
                        tan_angle: 0.0,
                    },
                )
                .unwrap();
                assert!(!on_normal, "{case}");
                c.rebuild_step_from_nurb_parameters(&k).unwrap();
                assert_eq!(net_of(&c), net, "{case}");
                assert!(c.is_cylinder_class() && c.semi_angle_deg() == 0.0, "{case}");
                assert_eq!(c.base_radius(), 9.0, "{case}");
                let pos = *c.rev().position();
                let z = pos.z_axis();
                // along the centre line, from the unmoved circle's centre to the moved
                // one's (an inside-out net may run down, its last row at z = 0): (1e-8, 0,
                // 1000) or (1e-8, 0, -1000), up to its sign
                let tilt = z.x / z.z;
                let want = if moved_z > 500.0 { shift } else { -shift } / 1000.0;
                assert!((tilt - want).abs() < 1e-6 * shift / 1000.0, "{case}: {z:?}");
                assert!(z.y.abs() < 1e-20, "{case}: {z:?}");
                assert!(
                    implicit::lies_on(
                        &c.rev().base,
                        &implicit::Implicit::Cone {
                            origin: pos.origin(),
                            axis: z,
                            radius: 9.0,
                            tan_angle: 0.0,
                        },
                    )
                    .unwrap(),
                    "{case}"
                );
            }
        }
    }

    #[test]
    fn rebuild_cylinder_rejects_a_taper_before_changing_analytic_state() {
        let k = kernel();
        for radius in [0.009, 9.0, 90_000.0] {
            for sign in [-2.0, 2.0] {
                for swap in [false, true] {
                    for inside in [false, true] {
                        let top = radius + sign * EFF_ZERO_SQRT * (1.0 + radius);
                        let mut c = edited_cylinder(&k, radius, top, swap, inside);
                        let before = c.clone();
                        let e = c.rebuild_step_from_nurb_parameters(&k).unwrap_err();
                        assert_eq!(
                            e.surface_failure(),
                            Some(&SurfaceFailure::NurbNotAnalytic {
                                check: AnalyticCheck::RadiiDiffer
                            })
                        );
                        assert_eq!(c.base_radius(), before.base_radius());
                        assert_eq!(c.semi_angle_deg(), before.semi_angle_deg());
                        assert_eq!(c.rev().position(), before.rev().position());
                        assert_eq!(c.rev().step_uv_domain(), before.rev().step_uv_domain());
                    }
                }
            }
        }
    }

    /// #16982 review: a top circle tilted about its own x axis keeps the x axes, the
    /// angles and the centre line of an untilted one, which were all the rebuild
    /// checked, so it rebuilt as a cylinder the NURB is not. The top normal is now
    /// checked against the axis with the rule the bottom one meets (1e-3 degrees): a
    /// tilt of 0.01 degrees is refused with the cone unchanged, 1e-4 degrees passes
    /// those checks.
    ///
    /// #17228: the 1e-4 degree net is then refused by the cylinder certificate, cone
    /// unchanged. Its radii are equal (the `EFF_ZERO` branch, which recorded the
    /// cylinder unchecked), but the tilted top circle is no section of a right
    /// cylinder: tilted by `t` = 1.745e-6 rad about x, its point at 90 degrees is
    /// `(0, 9 cos t, 3 + 9 sin t)`, `9 sin^2 t / 2 = 1.37e-11` inside radius 9. That
    /// point is an interpolated control point (weight 1) of the end row, and radial
    /// scaling leaves it (the fitted top circle has radius 9, so the scale is 1), so
    /// the implicit `x^2 + y^2 - 81` has the Bernstein coefficient
    /// `-81 sin^2 t = -2.47e-10` there, against the certificate's rounding allowance
    /// (`implicit`: `gamma_64` for these degree (2, 1) patches, 7.1e-15, times the
    /// squared size of the patch, a few hundred here: about 1e-12): outside it, so
    /// `lies_on` refuses, as the mean-radius branch already did since #16982.
    #[test]
    fn rebuild_refuses_a_tilted_top_circle() {
        let k = kernel();
        for (tilt_deg, refused) in [(0.01, true), (1e-4, false)] {
            let (sin, cos) = f64::to_radians(tilt_deg).sin_cos();
            for swap in [false, true] {
                for inside in [false, true] {
                    let mut c = edited_cylinder(&k, 9.0, 9.0, swap, inside);
                    // turn the top row (z = 3) about the line through (0, 0, 3)
                    // along x, in homogeneous coordinates
                    let s = c.base_mut().get_or_create_gw_nurb().unwrap();
                    let (n, m) = (s.n(), s.m());
                    for i in 0..=n {
                        for j in 0..=m {
                            if if swap { i == n } else { j == m } {
                                let p = s.cpt_mut(i, j).unwrap();
                                let (y, z) = (p.y, p.z - 3.0 * p.w);
                                p.y = y * cos - z * sin;
                                p.z = 3.0 * p.w + y * sin + z * cos;
                            }
                        }
                    }
                    let before = c.clone();
                    let result = c.rebuild_step_from_nurb_parameters(&k);
                    // the tilted row is the NURB's last; with swap and inside the rebuild
                    // reads it as its bottom or its top arc, so one of the two
                    // square-to-axis checks refuses it; the 1e-4 degree tilt passes
                    // them and is refused by the cylinder certificate (#17228)
                    let e = result.expect_err(&format!("{tilt_deg} {swap} {inside}"));
                    let check = if refused {
                        matches!(
                            e.surface_failure(),
                            Some(&SurfaceFailure::NurbNotAnalytic {
                                check: AnalyticCheck::BottomNotSquareToAxis
                                    | AnalyticCheck::TopNotSquareToAxis
                            })
                        )
                    } else {
                        e.surface_failure()
                            == Some(&SurfaceFailure::NurbNotAnalytic {
                                check: AnalyticCheck::NetOffCylinder,
                            })
                    };
                    assert!(check, "{tilt_deg} {swap} {inside}: {e}");
                    assert_eq!(c.base_radius(), before.base_radius());
                    assert_eq!(c.semi_angle_deg(), before.semi_angle_deg());
                    assert_eq!(c.rev().position(), before.rev().position());
                }
            }
        }
    }

    #[test]
    fn rebuild_shallow_cone_keeps_its_nonzero_angle() {
        let k = kernel();
        for sign in [-0.5, 0.5] {
            let radius = 9.0;
            let top = radius + sign * EFF_ZERO_SQRT * (1.0 + radius);
            let mut c = Cone::new_bounded(
                Vec3::ZERO,
                Vec3::X,
                Vec3::Y,
                radius,
                top,
                0.0,
                360.0,
                3.0,
                false,
                false,
                false,
            );
            c.make_nurb(&k).unwrap();
            assert!(!c.is_cylinder_class());
            c.rebuild_step_from_nurb_parameters(&k).unwrap();
            let expected = (top - radius).atan2(3.0) * 180.0 / PI;
            assert!((c.semi_angle_deg() - expected).abs() < 1e-12);
            assert_ne!(c.semi_angle_deg(), 0.0);
        }
    }

    #[test]
    fn rebuild_cylinder_small_arc_clears_a_stale_semi_angle() {
        let k = kernel();
        for swap in [false, true] {
            for inside in [false, true] {
                let mut c = Cone::new_cylinder_bounded(
                    Vec3::ZERO,
                    Vec3::X,
                    Vec3::Y,
                    9.0,
                    0.0,
                    360.0 / 256.0,
                    3.0,
                    swap,
                    inside,
                    false,
                );
                c.make_nurb(&k).unwrap();
                // An earlier buggy rebuild can leave this angle even after the
                // NURB is restored to an exact cylinder. Equal radii must reset it.
                c.semi_angle_deg = -4.3550699142343645e-10;
                c.rebuild_step_from_nurb_parameters(&k).unwrap();
                assert_eq!(c.semi_angle_deg(), 0.0);
                assert!((c.base_radius() - 9.0).abs() < 1e-8);
                assert_eq!(c.bot_radius(), c.top_radius());
            }
        }
    }

    /// tangency-labs/solidcore#713 (WR-385): a cylinder whose NURB radii differ by
    /// 2.25e-11 (rounding, as after `SwapUV`) gets a semi-angle upstream, which its own
    /// check forbids; here the radii are its one radius.
    #[test]
    fn issue_713_cylinder_rebuild_keeps_zero_semi_angle() {
        let k = kernel();
        let make = |k: &Kernel| {
            let mut c = Cone::create_canonical_cylinder(k, &Axis2Placement::IDENTITY, 9.0).unwrap();
            let linear_u = c.rev.base.degree(SurfParamType::U) == 1;
            let nurb = c.rev.base.nurb.as_mut().unwrap();
            let (n, m) = (nurb.n(), nurb.m());
            for i in 0..=n {
                for j in 0..=m {
                    if (linear_u && i == n) || (!linear_u && j == m) {
                        let p = nurb.cpt_mut(i, j).unwrap();
                        p.x *= 1.0 + 2.5e-12;
                        p.y *= 1.0 + 2.5e-12;
                    }
                }
            }
            c
        };
        let mut fixed = make(&k);
        fixed.rebuild_step_from_nurb_parameters(&k).unwrap();
        assert_eq!(fixed.semi_angle_deg(), 0.0);
        assert!((fixed.base_radius() - 9.0).abs() < 1e-9);
    }

    /// The fork's 5df5b27 order (WR-385): radii within `SM_EFF_ZERO` * (1 + r) are made
    /// equal by upstream's own copy of the bottom radius before the mean is considered,
    /// so the radius is bit-for-bit the bottom (first-row) circle's: the one the
    /// unedited cylinder rebuilds to, since only the last row is edited.
    #[test]
    fn rebuild_cylinder_within_eff_zero_copies_the_bottom_radius() {
        let k = kernel();
        for radius in [0.009, 5.123456789, 9.0, 90_000.0] {
            for factor in [1.0 - 1e-13, 1.0 + 1e-13] {
                for swap in [false, true] {
                    for inside in [false, true] {
                        let mut bottom = edited_cylinder(&k, radius, radius, swap, inside);
                        bottom.rebuild_step_from_nurb_parameters(&k).unwrap();
                        let mut fixed = edited_cylinder(&k, radius, radius * factor, swap, inside);
                        fixed.rebuild_step_from_nurb_parameters(&k).unwrap();
                        let at = format!("{radius} {factor} {swap} {inside}");
                        assert_eq!(fixed.base_radius(), bottom.base_radius(), "{at}");
                        assert_eq!(fixed.bot_radius(), bottom.bot_radius(), "{at}");
                        assert_eq!(fixed.top_radius(), bottom.top_radius(), "{at}");
                        assert_eq!(fixed.bot_radius(), fixed.top_radius(), "{at}");
                        assert_eq!(fixed.semi_angle_deg(), 0.0, "{at}");
                    }
                }
            }
        }
    }

    #[test]
    fn offset_moves_radii() {
        let k = kernel();
        let cone = canonical(&k, 2.0, 1.0, 3.0, false, false);
        let off = cone.create_offset_surface(&k, 0.5, 1e-6).unwrap().unwrap();
        let semi = cone.semi_angle_deg() * PI / 180.0;
        assert!((off.base_radius() - (2.0 + semi.cos() * 0.5)).abs() < 1e-12);
        // both radii below the tolerance: no surface
        assert!(cone
            .create_offset_surface(&k, -10.0, 1e-6)
            .unwrap()
            .is_none());
        // one radius below the tolerance: truncated at the new apex
        let trunc = cone.create_offset_surface(&k, -1.2, 1e-6).unwrap().unwrap();
        assert!(trunc.top_radius().abs() < 1e-12);
    }

    /// The offset of a cone truncated at its new apex is reparameterised to the matching
    /// part of this NURB domain, anchored at the end that survives (an inside-out cone's
    /// NURB runs the generator backwards), so a point of the kept cone keeps its
    /// parameter and the face's UV trims lift onto the offset unchanged
    /// (tangency-labs/solidcraft#11774: the part sat at the domain's other end, the
    /// rim's UV moved, and a shell's edge-use mapping could not pair the rim, so the
    /// shell refused a cone with an apex).
    #[test]
    fn offset_truncation_keeps_the_surviving_ends_parameters() {
        let k = kernel();
        for inside in [false, true] {
            // negated inside out: the shrinking offset is -1.2 there and 1.2 here
            let d = if inside { 1.2 } else { -1.2 };
            // full apex cones: (bot, top) = (0, 2) collapses at the bot end under the
            // offset, (2, 0) at the top; truncated cones (1, 2) and (2, 1) likewise,
            // where the kept height follows the radius's span top - bot, not the
            // surviving radius alone
            for (bot, top) in [(0.0, 2.0), (2.0, 0.0), (1.0, 2.0), (2.0, 1.0)] {
                let cone = canonical(&k, bot, top, 3.0, false, inside);
                let off = cone.create_offset_surface(&k, d, 1e-6).unwrap().unwrap();
                // the radii move by cos(semi) * d along the NURB normal (negated
                // inside out); the kept cone runs from where that radius reaches zero
                // to the surviving end, at the slope (top - bot) / 3
                let semi = cone.semi_angle_deg() * PI / 180.0;
                let moved = (if inside { -1.0 } else { 1.0 }) * d * semi.cos();
                let (new_bot, new_top) = (bot + moved, top + moved);
                let collapsed_bot = new_bot < new_top;
                let (want_bot, want_top, want_height) = if collapsed_bot {
                    (0.0, new_top, 3.0 * new_top / (top - bot))
                } else {
                    (new_bot, 0.0, 3.0 * new_bot / (bot - top))
                };
                assert!(
                    (off.bot_radius() - want_bot).abs() < 1e-9
                        && (off.top_radius() - want_top).abs() < 1e-9,
                    "bot {bot} top {top} inside {inside}: radii {:?} {:?}",
                    off.bot_radius(),
                    off.top_radius()
                );
                assert!(
                    (off.height(&k).unwrap() - want_height).abs() < 1e-9,
                    "bot {bot} top {top} inside {inside}: height {:?}, the radius's span gives {want_height}",
                    off.height(&k)
                );

                // The matching part of this NURB domain, anchored at the surviving end:
                // the bot sits at the domain's minimum and the top at its maximum, and an
                // inside-out cone's NURB runs that generator backwards, so the surviving
                // end is the maximum one when the collapsed end is the bot's, and the
                // minimum one when the collapsed end is the top's, and the other way
                // round inside out.
                let at_max_end = collapsed_bot != inside;
                let dom0 = cone.rev().base().natural_uv_domain();
                let dom = off.rev().base().natural_uv_domain();
                let (lo, hi) = (dom0.min().y, dom0.max().y);
                let span = hi - lo;
                let kept = want_height / 3.0 * span;
                let (want_min, want_max) = if at_max_end {
                    (hi - kept, hi)
                } else {
                    (lo, lo + kept)
                };
                assert!(
                    (dom.min().y - want_min).abs() < 1e-9 && (dom.max().y - want_max).abs() < 1e-9,
                    "bot {bot} top {top} inside {inside}: domain {dom:?}, matching part [{want_min}, {want_max}]"
                );

                // the offset of S is S + d n at the same parameters: sampled points of
                // the kept cone keep their distance from the original
                for t in [0.3, 0.7] {
                    let uv = Point2::new(0.37, want_min + t * (want_max - want_min));
                    let s = cone.evaluate_point(&k, uv).unwrap();
                    let o = off.evaluate_point(&k, uv).unwrap();
                    assert!(
                        (s.distance(o) - d.abs()).abs() < 1e-9,
                        "bot {bot} top {top} inside {inside} at {uv:?}: {}",
                        s.distance(o)
                    );
                }
            }
        }
    }

    /// A cone moved along its axis and run on to its apex (#21188) keeps each point's
    /// parameter and radius: its point at a parameter of this cone is this cone's point there
    /// moved by the shift, one of its radii is 0 (its apex, on the axis; the radius read back
    /// from its placement to its rounding), and its domain holds this cone's and runs on to
    /// the apex, its span grown by the larger radius over the radii's difference (the rulings'
    /// parameter speed kept). A cylinder has no apex.
    #[test]
    fn a_cone_moved_along_its_axis_to_its_apex_keeps_its_parameters() {
        let k = kernel();
        for inside in [false, true] {
            for (bot, top) in [(4.0, 2.0), (2.0, 4.0), (1.0, 2.0)] {
                let cone = canonical(&k, bot, top, 2.0, false, inside);
                let dom0 = cone.rev().base().natural_uv_domain();
                for shift in [-4.2, 1.5] {
                    let at = format!("bot {bot} top {top} inside {inside} shift {shift}");
                    let moved = cone.moved_along_axis_to_apex(&k, shift).unwrap().unwrap();
                    assert!(
                        moved.bot_radius().abs() < 1e-12 || moved.top_radius().abs() < 1e-12,
                        "{at}: radii {} {}",
                        moved.bot_radius(),
                        moved.top_radius()
                    );
                    let dom = moved.rev().base().natural_uv_domain();
                    assert!(
                        dom0.is_contained_by(&dom, 1e-12),
                        "{at}: {dom0:?} in {dom:?}"
                    );
                    let want = dom0.v_interval().length() * f64::max(bot, top) / (bot - top).abs();
                    assert!(
                        (dom.v_interval().length() - want).abs() < 1e-9,
                        "{at}: span {:?}, to the apex {want}",
                        dom.v_interval()
                    );
                    for (s, t) in [(0.37, 0.0), (0.37, 0.5), (0.9, 1.0)] {
                        let uv = dom0.evaluate(s, t);
                        let p = moved.evaluate_point(&k, uv).unwrap();
                        let q = cone.evaluate_point(&k, uv).unwrap() + Point3::new(0.0, 0.0, shift);
                        assert!(p.distance(q) < 1e-9, "{at} at {uv:?}: {p:?} vs {q:?}");
                    }
                }
            }
        }
        let cylinder = canonical(&k, 2.0, 2.0, 3.0, false, false);
        assert!(cylinder
            .moved_along_axis_to_apex(&k, 1.0)
            .unwrap()
            .is_none());
    }

    /// tangency-labs/solidcraft#15165: a truncated cone (bot 1, top 2, height 3) offset
    /// inward by 1.2 collapses at its bot end. Its radii move by `cos(semi) d` with
    /// `tan(semi) = 1 / 3`, so the offset top radius is `2 - 1.2 * 3 / sqrt(10)` =
    /// 0.86158 and, at the original's slope (the radius grows 1 over 3 of height), the
    /// kept cone is `3 * 0.86158 / 1` = 2.58474 high (2.5848 in the issue, from 0.8616).
    /// The old height `0.86158 * 3 / 2` = 1.29237 took the slope from the top radius
    /// alone, as for a full apex cone. The
    /// offset is `S + d n` at the same parameters, with the closed-form unit normal
    /// `(cos(semi) cos(t), cos(semi) sin(t), -sin(semi))` of the identity-placed cone.
    /// The mirror cone (bot 2, top 1) collapses at its top by the same amount.
    #[test]
    fn offset_truncation_of_a_truncated_cone_keeps_its_slope() {
        let k = kernel();
        let (cos_semi, sin_semi) = (3.0 / 10f64.sqrt(), 1.0 / 10f64.sqrt());
        let d = -1.2;
        let kept_radius = 2.0 + d * cos_semi;
        let want_height = 3.0 * kept_radius / 1.0;
        // the issue's figures: 0.8616 to four places, and 3 * 0.8616 = 2.5848 (the
        // unrounded height is 2.58474)
        assert!((kept_radius - 0.8616).abs() < 5e-5, "{kept_radius}");
        assert!((want_height - 2.5848).abs() < 3.0 * 5e-5, "{want_height}");
        for (bot, top) in [(1.0, 2.0), (2.0, 1.0)] {
            let cone = canonical(&k, bot, top, 3.0, false, false);
            let off = cone.create_offset_surface(&k, d, 1e-6).unwrap().unwrap();
            let height = off.height(&k).unwrap();
            assert!(
                (height - want_height).abs() < 1e-12,
                "bot {bot} top {top}: height {height}, the radius's span gives {want_height}"
            );
            let (want_bot, want_top) = if bot < top {
                (0.0, kept_radius)
            } else {
                (kept_radius, 0.0)
            };
            assert!(
                (off.bot_radius() - want_bot).abs() < 1e-12
                    && (off.top_radius() - want_top).abs() < 1e-12,
                "bot {bot} top {top}: radii {} {}",
                off.bot_radius(),
                off.top_radius()
            );

            // S + d n at the same parameters, n the outward unit normal of the cone
            // whose radius grows with z (bot < top) or shrinks (bot > top)
            let tilt = if bot < top { sin_semi } else { -sin_semi };
            let dom = off.rev().base().natural_uv_domain();
            for (su, sv) in [(0.1, 0.2), (0.37, 0.5), (0.8, 0.9), (0.5, 1.0)] {
                let uv = Point2::new(
                    dom.min().x + su * (dom.max().x - dom.min().x),
                    dom.min().y + sv * (dom.max().y - dom.min().y),
                );
                let s = cone.evaluate_point(&k, uv).unwrap();
                let o = off.evaluate_point(&k, uv).unwrap();
                let t = s.y.atan2(s.x);
                let n = Vec3::new(cos_semi * t.cos(), cos_semi * t.sin(), -tilt);
                let want = s + d * n;
                assert!(
                    o.distance(want) < 1e-12,
                    "bot {bot} top {top} at {uv:?}: offset {o:?}, S + d n {want:?}"
                );
            }
        }
    }

    #[test]
    fn offset_with_non_finite_truncation_height_returns() {
        // bottom radius 0 and a negative top radius: the truncation height is h / 0
        let k = kernel();
        let mut cone = Cone::new_bounded(
            Vec3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            0.0,
            -1.0,
            0.0,
            360.0,
            2.0,
            false,
            false,
            false,
        );
        cone.make_nurb(&k).unwrap();
        for d in [0.5, 1e-3, 3.0, -0.5] {
            let _ = cone.create_offset_surface(&k, d, 1e-6);
        }
    }

    #[test]
    fn trim_of_a_cross_section() {
        let k = kernel();
        let cone = canonical(&k, 2.0, 1.0, 3.0, false, false);
        // the circle at half height, a little tilted plane through the axis point
        let r = cone.radius(1.5);
        let ivls = ellipse_trim_intervals(
            &k,
            Vec3::new(0.0, 0.0, 1.5),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            r,
            r,
            0.0,
            360.0,
            &cone,
            &cone.rev().base().natural_uv_domain(),
            1e-6,
        );
        // the circle lies in no seam-line plane crossing: the result is whatever the
        // classification gives, but it must not fail
        assert!(ivls.is_ok());
    }

    /// WR-490: the trim intervals of an ellipse on a cylinder r 10, h 100 trimmed to
    /// v >= 30, and the total length (deg) of the same ellipse's on the natural domain.
    fn ellipse_on_a_trimmed_cylinder() -> (Vec<Extent1d>, f64) {
        // A face already joined to another body: the ellipse of a plane through the axis
        // point (0, 0, 50) that meets the seam (x = 10, y = 0) only at its lowest point,
        // z = 20, in the trimmed-off part. Upstream takes the seam lines from the trimmed
        // domain: at c9c5979 no crossing at all, and the ellipse came back as outside the
        // cylinder; v1.0.1 adds the trim circle at v = 30 (see the test below).
        let k = kernel();
        let cyl = canonical(&k, 10.0, 10.0, 100.0, false, false);
        let d = cyl.rev().base().natural_uv_domain();
        let (lo, hi) = (d.min(), d.max());
        let trim = Extent2d::from_bounds(lo.x, lo.y + 0.3 * (hi.y - lo.y), hi.x, hi.y);
        let step_min = cyl.convert_uv_from_nurbs_to_step(&k, trim.min()).unwrap();
        assert!(step_min.y > 20.0 + 1e-6 && step_min.y < 50.0);
        let x = Vec3::new(1.0, 0.0, -3.0) * (1.0 / 10.0_f64.sqrt());
        let rx = 1000.0_f64.sqrt();
        let lowest = Vec3::new(0.0, 0.0, 50.0) + rx * x;
        assert!((lowest - Vec3::new(10.0, 0.0, 20.0)).length() < 1e-12);
        let ivls = ellipse_trim_intervals(
            &k,
            Vec3::new(0.0, 0.0, 50.0),
            x,
            Vec3::new(0.0, 1.0, 0.0),
            rx,
            10.0,
            0.0,
            360.0,
            &cyl,
            &trim,
            1e-6,
        )
        .unwrap();
        let natural = ellipse_trim_intervals(
            &k,
            Vec3::new(0.0, 0.0, 50.0),
            x,
            Vec3::new(0.0, 1.0, 0.0),
            rx,
            10.0,
            0.0,
            360.0,
            &cyl,
            &d,
            1e-6,
        )
        .unwrap();
        let total = natural.iter().map(Extent1d::length).sum::<f64>();
        (ivls, total)
    }

    /// The part of the ellipse of [`ellipse_on_a_trimmed_cylinder`] inside the trimmed
    /// domain: `z = 50 - 30 cos t >= 30`, so `cos t <= 2/3`.
    fn ellipse_part_above_v30() -> f64 {
        360.0 - 2.0 * (2.0_f64 / 3.0).acos().to_degrees()
    }

    #[test]
    fn seam_crossing_below_a_trimmed_domain_keeps_the_ellipse() {
        // WR-490: the trim circles sit at the trim domain's v limits, so the part of the
        // ellipse inside the trimmed domain, one interval with no zero-length piece at the
        // seam crossing z = 20 below it (the C++ 1.0.1 gives the same: surface.cone_p2
        // case wr490_ellipse_on_cylinder_trimmed_v30)
        let (trimmed, natural) = ellipse_on_a_trimmed_cylinder();
        assert!((natural - 360.0).abs() < 1e-9);
        let total = trimmed.iter().map(Extent1d::length).sum::<f64>();
        assert!(
            (total - ellipse_part_above_v30()).abs() < 1e-9,
            "{trimmed:?}"
        );
        // z = 50 - 30 cos t = 30 at t = acos(2/3): the one interval between those angles
        let edge = (2.0_f64 / 3.0).acos().to_degrees();
        let [only] = trimmed.as_slice() else {
            panic!("one interval expected: {trimmed:?}");
        };
        assert!((only.min() - edge).abs() < 1e-9, "{trimmed:?}");
        assert!((only.max() - (360.0 - edge)).abs() < 1e-9, "{trimmed:?}");
    }

    /// The control points and knots of two NURBs agree bit for bit.
    fn same_nurb(a: &BSplineSurface, b: &BSplineSurface) -> bool {
        match (a.gw_nurb(), b.gw_nurb()) {
            (Some(a), Some(b)) => {
                let rows = |s: &crate::nurbs::srf_basic::Surface| {
                    s.rows()
                        .flat_map(|r| r.iter().map(|p| [p.x, p.y, p.z, p.w]))
                        .collect::<Vec<_>>()
                };
                rows(a) == rows(b) && a.u_knots() == b.u_knots() && a.v_knots() == b.v_knots()
            }
            _ => false,
        }
    }

    /// tangency-labs/solidcore#1433: the cylinder test finds the radius 5 cylinder
    /// about +Z from its NURB form and keeps exactly that NURB.
    #[test]
    fn issue_1433_cylinder_found_in_its_nurb_keeps_it() {
        let k = kernel();
        let cyl = Cone::create_canonical_cylinder(&k, &Axis2Placement::default(), 5.0).unwrap();
        let nurbs = cyl.rev().base().clone();
        let found = Cone::is_nurb_surface_cylinder(&k, &nurbs, 1.0)
            .unwrap()
            .expect("a cylinder");
        assert!(found.is_cylinder());
        assert_eq!(found.semi_angle_deg(), 0.0);
        assert!(
            (found.base_radius() - 5.0).abs() < 1e-12,
            "{}",
            found.base_radius()
        );
        let z = found
            .rev()
            .position()
            .x_axis()
            .cross(found.rev().position().y_axis());
        assert!(z.is_parallel_to(Vec3::new(0.0, 0.0, 1.0), 1e-9), "{z:?}");
        assert!(found.rev().position().origin().x.abs() < 1e-12);
        assert!(found.rev().position().origin().y.abs() < 1e-12);
        assert!(same_nurb(found.rev().base(), &nurbs));
    }

    /// A bounded partial cylinder (either UV order, inside out) is found with its
    /// radius, angles and height; a cone and a tapered cylinder NURB are not cylinders.
    #[test]
    fn cylinder_recognition_partial_and_non_matching() {
        let k = kernel();
        for (swap, inside) in [(false, false), (true, false), (false, true), (true, true)] {
            let frame = Axis2Placement::new(
                Vec3::new(1.0, -2.0, 3.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(0.0, 0.0, 1.0),
            );
            let mut cyl = Cone::new_cylinder_bounded(
                frame.origin(),
                frame.x_axis(),
                frame.y_axis(),
                2.5,
                30.0,
                200.0,
                4.0,
                swap,
                inside,
                false,
            );
            cyl.make_nurb(&k).unwrap();
            let nurbs = cyl.rev().base().clone();
            let found = Cone::is_nurb_surface_cylinder(&k, &nurbs, 1.0)
                .unwrap()
                .expect("a cylinder");
            assert!((found.base_radius() - 2.5).abs() < 1e-12);
            assert!(same_nurb(found.rev().base(), &nurbs));
            let d = found.rev().step_uv_domain();
            assert!((d.max().x - d.min().x - 170.0).abs() < 1e-9, "{d:?}");
            assert!((d.max().y - d.min().y - 4.0).abs() < 1e-9, "{d:?}");
        }
        let cone = canonical(&k, 2.0, 30.0, 3.0, false, false);
        assert!(Cone::is_nurb_surface_cylinder(&k, cone.rev().base(), 1.0)
            .unwrap()
            .is_none());
    }

    /// `lean_cylinder`: a cylinder of radius 5 (frame at (1, 2, 3), heights 0 to 10) leaning at
    /// 0.1 per unit of height, radius 5 at height 4. Same parameters point for point, the radius
    /// the straight law of the height, the cone's own data (base radius, semi-angle) rebuilt.
    #[test]
    fn lean_cylinder_keeps_the_parameters_and_changes_the_radius() {
        let k = kernel();
        let frame = Axis2Placement::new(Point3::new(1.0, 2.0, 3.0), Vec3::X, Vec3::Y);
        let mut cylinder = Cone::create_canonical_cylinder(&k, &frame, 5.0).unwrap();
        let mut domain = Extent2d::default();
        domain.set_min_max_coords(0.0, 0.0, 360.0, 10.0).unwrap();
        cylinder.adjust_step_uv_domain(&k, &domain).unwrap();
        let slope = 0.1;
        let cone = cylinder.lean_cylinder(&k, slope, 4.0).unwrap();
        assert!(!cone.is_cylinder() && !cone.is_cylinder_class());
        assert_eq!(
            cone.rev().base().natural_uv_domain(),
            cylinder.rev().base().natural_uv_domain()
        );
        assert!((cone.canonical().radius - (5.0 - 0.4)).abs() < 1e-9);
        assert!((cone.canonical().semi_angle_deg - slope.atan().to_degrees()).abs() < 1e-9);
        let nurb = cylinder.rev().base().natural_uv_domain();
        for i in 0..=6 {
            for j in 0..=3 {
                let uv = Point2::new(
                    nurb.min().x + (nurb.max().x - nurb.min().x) * f64::from(i) / 6.0,
                    nurb.min().y + (nurb.max().y - nurb.min().y) * f64::from(j) / 3.0,
                );
                let (old, new) = (
                    cylinder.rev().base().evaluate_point(uv).unwrap(),
                    cone.rev().base().evaluate_point(uv).unwrap(),
                );
                let (d_old, d_new) = (
                    old - Point3::new(1.0, 2.0, old.z),
                    new - Point3::new(1.0, 2.0, new.z),
                );
                let height = old.z - 3.0;
                assert!((new.z - old.z).abs() < 1e-9, "{uv:?}");
                assert!(
                    (d_new.length() - (5.0 + slope * (height - 4.0))).abs() < 1e-9,
                    "{uv:?}"
                );
                assert!(d_old.cross(d_new).length() < 1e-9 * 25.0, "{uv:?}");
            }
        }
        // a radius that would go through the axis is refused, and so is a surface that is
        // not a cylinder
        let e = cylinder.lean_cylinder(&k, 2.0, 4.0).unwrap_err();
        assert_eq!(e.sm_status(), Some(crate::error::SM_ERR));
        assert_eq!(
            e.surface_failure(),
            Some(&SurfaceFailure::ConeClosesWithinSpan)
        );
        assert_eq!(e.kind(), crate::error::ErrorKind::Degenerate);
        // a NaN or infinite slope or neutral height, or a finite pair whose radius law
        // overflows, is the caller's: invalid input, not a cone through the axis
        for (s, h) in [
            (f64::NAN, 4.0),
            (slope, f64::NAN),
            (f64::INFINITY, 4.0),
            (slope, f64::NEG_INFINITY),
            (1e308, -1e308),
        ] {
            let e = cylinder.lean_cylinder(&k, s, h).unwrap_err();
            assert_eq!(e.sm_status(), Some(crate::error::SM_ERR), "{s} {h}");
            assert_eq!(
                e.surface_failure(),
                Some(&SurfaceFailure::LeanNotFinite),
                "{s} {h}"
            );
            assert_eq!(e.kind(), crate::error::ErrorKind::InvalidInput, "{s} {h}");
        }
        let e = cone.lean_cylinder(&k, slope, 4.0).unwrap_err();
        assert_eq!(e.sm_status(), Some(crate::error::SM_ERR));
        assert_eq!(e.surface_failure(), Some(&SurfaceFailure::NotACylinder));
        assert_eq!(e.kind(), crate::error::ErrorKind::Unsupported);
    }

    /// Two end arcs of a short cylinder differ by two ulps at one control point. Their
    /// weights still describe the same circle; recovering each radius from point angles
    /// amplifies that rounding enough to refuse the cylinder.
    #[test]
    fn short_cylinder_with_rounded_end_arcs_keeps_its_exact_class() {
        use crate::nurbs::math::CPoint;
        use crate::nurbs::srf_basic::Surface as Nurb;
        use crate::surface::implicit::{lies_on, AnalyticSupport};

        let k = kernel();
        let (radius, half, start) = (300.0, 0.0009_f64, -0.4);
        let weight = half.cos();
        let mut net = Vec::new();
        for y in [0.0, -80.0] {
            let mut row = Vec::new();
            for (i, (angle, w)) in [
                (start, 1.0),
                (start + half, weight),
                (start + 2.0 * half, 1.0),
            ]
            .into_iter()
            .enumerate()
            {
                let mut x = 10.0 + radius * angle.cos() / w;
                let z = 30.0 + radius * angle.sin() / w;
                if y < 0.0 && i == 1 {
                    x = x.next_up().next_up();
                }
                row.push(CPoint::new(x * w, y * w, z * w, w));
            }
            net.push(row);
        }
        let form = BSplineSurface::from_nurb(
            Nurb::new(
                net,
                1,
                2,
                vec![0.0, 0.0, 80.0, 80.0],
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            )
            .unwrap(),
            false,
        );
        let cylinder = Cone::is_nurb_surface_cylinder(&k, &form, 1.0)
            .unwrap()
            .expect("a short cylinder");
        assert!((cylinder.base_radius() - radius).abs() < 1e-6);
        assert_eq!(cylinder.rev().base().gw_nurb(), form.gw_nurb());
        let support =
            AnalyticSupport::of(&crate::surface::surface::Surface::from(cylinder)).unwrap();
        assert!(lies_on(&form, &support.implicit()).unwrap(), "{support:?}");
    }
}
