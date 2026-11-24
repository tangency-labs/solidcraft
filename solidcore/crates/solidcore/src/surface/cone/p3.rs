//! Analytic cone/cylinder intersections with planes and other cones.
//!
//! Upstream: `source/SMLib/src/SmCone.cpp`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`Cone::intersect_with_plane`] | `SmCone::IntersectWithPlane` |
//! | [`Cone::intersect_with_cone`] | `SmCone::IntersectWithCone` |
//! | [`Cone::global_surface_intersect_with`] | `SmCone::GlobalSurfaceIntersect` |
//! | `add_ellipse_intervals_trimmed_to_two_cones` | `sm_AddEllipseIntervalsTrimmedToTwoCones` |
//!
//! Deviations: output arrays are returned together and always requested; unused
//! edge/angle arguments of the analytic routines are omitted. Ignored statuses
//! propagate Unported; errors which would leave an uninitialised output propagate.
//! A default `SmPoint2d` is `SM_UNDEF_DOUBLE`, not uninitialised, so an ignored
//! `ConvertUVFromSTEPToNURBS` failure keeps the converter-written U and that V.
//! The general intersection is supplied as a callback (the common `Surface` dispatch
//! passes the base `SmSurface::GlobalSurfaceIntersect`); it receives every original
//! argument. The coincident-cylinder `IntersectSurfaceBoundaries` calls run
//! [`Surface::intersect_surface_boundaries`] on the caller's common surfaces when
//! dispatched through `Surface`, else on bare copies of the cones; an absent
//! tolerance there (UB-123, a native null dereference) is `InvalidInput`.
//! Bounding boxes are those of
//! bare geometry (no face owner), with upstream's zone-tolerance expansion.
//! A generator solve with no solution explicitly writes EXTERIOR upstream:
//! intersections skip that result while the public inversion API still errors.
//! Start-plane transform failures propagate here; upstream initializes EXTERIOR
//! before that transform and skips the point on failure. Parallel boundary-output
//! arrays are checked before deduplication rather than assuming equal lengths.
//! Parallel-cylinder lines span the overlap of both requested domains converted to
//! STEP, not the full cylinders' axial extents, and are tested against both face domains at their
//! middle, not their bottom end, which a trimmed face domain skips (WR-259).
//! Conversion errors propagate; the original tolerance branches are kept.
//! Not upstream: a plane's section of the cone, a circle or an ellipse, touches a side of
//! the plane's domain where it overlaps it by up to the contact tolerance
//! (`within_contact_tol`) and both surfaces are faces'
//! (`Plane::make_new_plane_ellipse_in_contact`, docs/booleans.md §6).
//! WR-510 (#1525): a plane parallel to a cylinder's axis is also tangent when it lies
//! within the zone tolerance of a bounded face's wall along a line (the larger of the
//! cone's and the plane's face tolerances, 0 without a face) and that line lies in the
//! face's sweep, else its crossings are tested; bare surfaces keep upstream's test. "Within"
//! is read as every contact site reads it (`within_contact_tol`): a plane exactly the zone
//! tolerance into the wall, or off a cone's generator, is within it.
//! Where that tangent line is off the face and an end of the face's sweep lies between it
//! and a crossing, the face touches the plane along that end: one tangent line there, not
//! the crossing a little further on (#20390), as an arc that starts on a line a hair inside
//! its circle touches it at its start (`Ellipse::intersect_with_line`). That touch is read
//! with the rim's tolerance, [`contact_tol_3d`], and taken only along an end the requested
//! domain holds whose line meets the plane's domain; else the crossing stays.
//! A plane holding a generator of a cone (a box's face against a cone lying on it) touches
//! it along that generator: one tangent line in closed form, as for the cylinder, instead of
//! a traced curve through the tangency, whose repeated points the check refuses. It holds
//! the generator within a face's zone tolerance; a bare plane that dips into the cone does
//! only while its two crossings lie within `tol` of the generator, the cylinder's own test.
//! WR-787 (#15429): parallel cylinders that both bound faces and overlap past touching by
//! up to [`contact_tol_3d`] (`within_contact_tol`) touch along one line; bare surfaces keep
//! two crossings.
//! No input-driven recursion is introduced.

use super::{Cone, ConeStepInversion};
use crate::curve::curve::{Curve, CurveKind};
use crate::curve::ellipse::{Ellipse, EllipseOptions};
use crate::curve::line::{BoundedLineOptions, CurveLocationType, Line};
use crate::error::surface::SurfaceFailure;
use crate::error::{ignored, Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::extent3d::Extent3d;
use crate::geom::geom_utility::p1::{
    circle_coplanar_circle_intersect_in_contact, line_closest_point, line_plane_intersect,
    parallelogram_cross_vectors, CircleCircle,
};
use crate::geom::geom_utility::p2::segment_segment_intersect;
use crate::geom::tol::{
    approx_tol3d, are_same_to_tol, contact_tol_3d, is_zero, within_contact_tol,
};
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::{sqrt, Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::{max2, min2, EFF_ZERO, EFF_ZERO_SQRT, PI, UNDEF_DOUBLE};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::{BoundingBoxRequest, LocationType};
use crate::surface::plane::p2::PlaneOwner;
use crate::surface::plane::Plane;
use crate::surface::sphere::Sphere;
use crate::surface::surf_of_extrusion::SurfOfExtrusion;
use crate::surface::surf_of_revolution::p3::Revolved;
use crate::surface::surf_of_revolution::SurfOfRevolution;
use crate::surface::surface::p2::{SurfaceIntersection, TsectCurveType, CONE_TYPE, CYLINDER_TYPE};
use crate::surface::surface::{Surface, SurfaceKind};

const OP: &str = "surface::cone::p3";

/// The explicitly typed other surface, retaining analytic virtual dispatch.
#[derive(Clone, Copy)]
pub enum IntersectTarget<'a> {
    /// Another cone or cylinder; cylinders have a different upstream type code.
    Cone(&'a Cone),
    /// A plane.
    Plane(&'a Plane),
    /// A general surface of revolution.
    Revolution(&'a SurfOfRevolution),
    /// A sphere.
    Sphere(&'a Sphere),
    /// A surface of extrusion.
    Extrusion(&'a SurfOfExtrusion),
    /// A surface already represented by the shared enum.
    Surface(&'a Surface),
}

/// Full inputs for the general intersector callback, including edge flags and
/// optional tolerances which the analytic routines do not use.
pub struct GeneralIntersectionRequest<'a> {
    /// Creation context and tolerances.
    pub kernel: &'a Kernel,
    /// This cone (retain its analytic overrides).
    pub cone: &'a Cone,
    /// This cone's requested NURB domain.
    pub domain: &'a Extent2d,
    /// The other surface.
    pub other: IntersectTarget<'a>,
    /// Its requested NURB domain.
    pub other_domain: &'a Extent2d,
    /// Surface boundary participation flags, in operand order.
    pub use_surface_edges: [bool; 2],
    /// Optional approximation tolerance, before context clamping.
    pub approx_tol_3d: Option<f64>,
    /// Optional angular tolerance in radians.
    pub ang_tol_rad: Option<f64>,
}

fn add(out: &mut SurfaceIntersection, curve: Curve, kind: TsectCurveType) {
    out.curves_3d.push(curve);
    out.surface1_uv_curves.push(None);
    out.surface2_uv_curves.push(None);
    out.curve_types.push(kind);
    out.deviations.push(0.0);
}

fn append(out: &mut SurfaceIntersection, more: SurfaceIntersection) {
    out.curves_3d.extend(more.curves_3d);
    out.surface1_uv_curves.extend(more.surface1_uv_curves);
    out.surface2_uv_curves.extend(more.surface2_uv_curves);
    out.curve_types.extend(more.curve_types);
    out.deviations.extend(more.deviations);
}

fn append_boundary_intersections(
    k: &Kernel,
    out: &mut SurfaceIntersection,
    mut second: SurfaceIntersection,
    tol: f64,
) -> Result<()> {
    for result in [&*out, &second] {
        let count = result.curves_3d.len();
        if result.surface1_uv_curves.len() != count
            || result.surface2_uv_curves.len() != count
            || result.curve_types.len() != count
            || result.deviations.len() != count
        {
            return Err(SolidError::surface(
                OP,
                SurfaceFailure::Defect {
                    why: "an intersection result's lists differ in length, though the \
                          intersections push one entry to each list per curve",
                },
            ));
        }
    }
    for (i, first) in out.curves_3d.iter().enumerate() {
        let mut duplicate = None;
        for (j, candidate) in second.curves_3d.iter().enumerate() {
            let coincidence = ignored(super::total_coincidence_checker(
                k,
                first,
                &first.natural_interval(),
                candidate,
                &candidate.natural_interval(),
                tol,
            ))?;
            if coincidence.is_some_and(|c| c.coincident) {
                duplicate = Some(j);
                break;
            }
        }
        if let Some(j) = duplicate {
            second.curves_3d.remove(j);
            second.surface1_uv_curves.remove(j);
            second.surface2_uv_curves.remove(j);
            second.curve_types.remove(j);
            second.deviations.remove(j);
            if let Some(kind) = out.curve_types.get_mut(i) {
                *kind = TsectCurveType::Coincident;
            }
        }
    }
    // The upstream second boundary call appends UV arrays without
    // reversing them; keep that order even though operands swapped.
    append(out, second);
    Ok(())
}

/// `IntersectSurfaceBoundaries` of `this` against `other` (both wrapping cones), as
/// the coincident-cylinder branch requests it: every output array, the boundary
/// curves with no UV curves and type `SM_TC_TANGENT`.
fn boundary_intersections(
    k: &Kernel,
    this: &Surface,
    domain: &Extent2d,
    other: &Surface,
    other_domain: &Extent2d,
    approx_tol: Option<f64>,
) -> Result<SurfaceIntersection> {
    // Upstream passes `*pdOptApproxTol3d` here, dereferencing an absent tolerance
    // (UB-123); the FFI refuses that input.
    let Some(tol) = approx_tol else {
        return Err(SolidError::invalid_input(
            OP,
            "coincident cylinders without an approximation tolerance (UB-123)",
        ));
    };
    let found = this.intersect_surface_boundaries(k, domain, other, other_domain, tol)?;
    let count = found.curves_3d.len();
    Ok(SurfaceIntersection {
        curves_3d: found.curves_3d,
        surface1_uv_curves: vec![None; count],
        surface2_uv_curves: vec![None; count],
        curve_types: vec![TsectCurveType::Tangent; count],
        deviations: found.deviations,
    })
}

fn bbox(k: &Kernel, cone: &Cone, domain: &Extent2d) -> Result<Extent3d> {
    cone.rev()
        .base()
        .calculate_bounding_box(
            k,
            domain,
            &BoundingBoxRequest {
                normal: true,
                expand_by_zone_tol: true,
                ..BoundingBoxRequest::default()
            },
        )?
        .normal
        .ok_or_else(|| {
            SolidError::surface(
                OP,
                SurfaceFailure::Defect {
                    why: "the B-spline bounding box routine, asked for the normal box, \
                          returned without it, though it returns every box it is asked for",
                },
            )
        })
}

fn angle_in_sweep(cone: &Cone, axis: Vec3, direction: Vec3, tol: f64) -> Result<bool> {
    let angle = axis.ccw_angle_between(cone.rev().position().x_axis(), direction)?;
    let sweep = cone.rev().step_uv_domain().u_interval();
    // Snap and compare against the raw start/end angles (`dStartAng`/`dEndAng`), as
    // upstream does; only the periodic mapping uses the normalised sweep
    // (`sSweepIvl`), so a reversed angle range matches the C++ (no curve).
    let start = cone.rev().start_angle_deg();
    let end = cone.rev().end_angle_deg();
    let mut deg = sweep
        .contains_periodic_value(angle * 180.0 / PI, 360.0, 0.0)
        .periodic_value;
    if are_same_to_tol(deg, start, tol) {
        deg = start;
    } else if are_same_to_tol(deg, end, tol) {
        deg = end;
    }
    Ok(deg >= start && deg <= end)
}

/// The direction, from the axis, of the end of the cone's sweep that lies strictly between
/// `tangent` (the direction of the plane's tangent line, off the sweep) and `crossing` (the
/// direction of a crossing line in the sweep), the short way round, and that the requested
/// `domain` holds: the generator along which a face that starts on the lens of a plane a
/// hair inside its wall touches that plane. `None` when neither end does (an end of the
/// surface's sweep that the requested domain stops short of is not an end of the face).
fn sweep_end_on_the_lens(
    k: &Kernel,
    cone: &Cone,
    domain: &Extent2d,
    axis: Vec3,
    tangent: Vec3,
    crossing: Vec3,
) -> Result<Option<Vec3>> {
    let x_axis = cone.rev().position().x_axis();
    let from_x = |direction: Vec3| axis.ccw_angle_between(x_axis, direction);
    // the angle from the tangent line, in (-pi, pi]
    let turn = |angle: f64, tangent_angle: f64| {
        let mut d = (angle - tangent_angle) % (2.0 * PI);
        if d > PI {
            d -= 2.0 * PI;
        } else if d <= -PI {
            d += 2.0 * PI;
        }
        d
    };
    let tangent_angle = from_x(tangent)?;
    let to_crossing = turn(from_x(crossing)?, tangent_angle);
    let step = cone.rev().step_uv_domain();
    for end_deg in [cone.rev().start_angle_deg(), cone.rev().end_angle_deg()] {
        let end = end_deg * PI / 180.0;
        let to_end = turn(end, tangent_angle);
        if to_end * to_crossing > 0.0 && to_end.abs() < to_crossing.abs() {
            // the end as the requested domain sees it: the coordinate of its generator
            // that does not change along the axis
            let low = cone.convert_uv_from_step_to_nurbs(k, Point2::new(end_deg, step.v_min()))?;
            let high = cone.convert_uv_from_step_to_nurbs(k, Point2::new(end_deg, step.v_max()))?;
            let (at, held) = if (low.x - high.x).abs() <= (low.y - high.y).abs() {
                (low.x, domain.u_interval())
            } else {
                (low.y, domain.v_interval())
            };
            if !held.contains_value(at, crate::geom::tol::ScaledZero::scaled_zero(&held)) {
                continue;
            }
            let y_axis = axis.cross(x_axis);
            return Ok(Some(end.cos() * x_axis + end.sin() * y_axis));
        }
    }
    Ok(None)
}

fn add_plane_line(
    out: &mut SurfaceIntersection,
    plane: &Plane,
    domain: &Extent2d,
    start: Point3,
    end: Point3,
    tol: f64,
    kind: TsectCurveType,
) -> Result<()> {
    let (curve, found_interval) = plane.make_new_plane_line(domain, start, end, tol)?;
    if let Some(curve) = curve {
        add(
            out,
            curve,
            if found_interval {
                kind
            } else {
                TsectCurveType::Touching
            },
        );
    }
    Ok(())
}

fn add_plane_ellipses(k: &Kernel, out: &mut SurfaceIntersection, curves: Vec<Curve>) -> Result<()> {
    for curve in curves {
        let kind = if curve.is_degenerate(k, EFF_ZERO, None)? {
            TsectCurveType::Touching
        } else {
            TsectCurveType::Crossing
        };
        add(out, curve, kind);
    }
    Ok(())
}

/// `sm_GetEllipseTrimIntervals` ([`super::p2::ellipse_trim_intervals`]) with the
/// ellipse's angular interval.
fn ellipse_trim_intervals(
    k: &Kernel,
    center: Point3,
    x: Vec3,
    y: Vec3,
    rx: f64,
    ry: f64,
    angles: Extent1d,
    cone: &Cone,
    domain: &Extent2d,
    tol: f64,
) -> Result<Vec<Extent1d>> {
    super::p2::ellipse_trim_intervals(
        k,
        center,
        x,
        y,
        rx,
        ry,
        angles.min(),
        angles.max(),
        cone,
        domain,
        tol,
    )
}

impl Cone {
    /// Analytic intersection with a plane, returning `(needs_general, curves)`.
    /// Hyperbolic/parabolic sections and an off-axis parallel cut of a true cone
    /// request the general intersector. Domains are NURB domains, not STEP domains.
    ///
    /// # Errors
    /// Geometry/conversion failures, or an unfinished ellipse-trimming dependency.
    pub fn intersect_with_plane(
        &self,
        k: &Kernel,
        domain: &Extent2d,
        plane: &Plane,
        other_domain: &Extent2d,
        approx_tol: Option<f64>,
    ) -> Result<(bool, SurfaceIntersection)> {
        self.intersect_with_plane_owned(
            k,
            domain,
            plane,
            PlaneOwner::NoFace,
            other_domain,
            approx_tol,
            [None, None],
        )
    }

    /// [`Cone::intersect_with_plane`] with the plane's owner: its box is expanded by the
    /// tolerance of the plane's face (`GetFace()->GetTolerance()`), else by
    /// `SM_ZONE_TOL_3D`. `zone_tols` are the zone tolerances of the cone's face and of
    /// the plane's face (`None`: no face, 0), read only by the WR-510 tangency test.
    ///
    /// # Errors
    ///
    /// Those of [`Cone::intersect_with_plane`]; [`SolidError::Unported`]
    /// (`SmFace::GetTolerance`) for a plane with a face whose tolerance is not cached.
    pub(crate) fn intersect_with_plane_owned(
        &self,
        k: &Kernel,
        domain: &Extent2d,
        plane: &Plane,
        plane_owner: PlaneOwner,
        other_domain: &Extent2d,
        approx_tol: Option<f64>,
        zone_tols: [Option<f64>; 2],
    ) -> Result<(bool, SurfaceIntersection)> {
        let mut out = SurfaceIntersection::default();
        let tol = approx_tol.map_or(EFF_ZERO_SQRT, |t| approx_tol3d(k, t));
        // a section a hair past a side of the plane's domain touches it where both
        // surfaces are faces' (`Plane::make_new_plane_ellipse_in_contact`); bare surfaces keep
        // upstream's
        let contact = if zone_tols.iter().all(Option::is_some) {
            contact_tol_3d(k)
        } else {
            0.0
        };
        let box1 = bbox(k, self, domain)?;
        // crPlane.CalculateBoundingBox: expanded by its face's tolerance (no face:
        // SM_ZONE_TOL_3D)
        let mut box2 = plane.bounding_boxes(k, other_domain, plane_owner)?.0;
        box2.expand_absolute(tol);
        if box1.are_disjoint(&box2, 0.0) {
            return Ok((false, out));
        }
        let height = self.height(k)?;
        let position = self.rev().position();
        let axis = position.z_axis();
        let step = self.rev().step_uv_domain();
        let bottom = position.origin() + step.v_min() * axis;
        let top = position.origin() + step.v_max() * axis;
        let mut normal = plane.position().z_axis();
        let plane_origin = plane.position().origin();
        let ang_tol = EFF_ZERO * 100.0 * 360.0;
        let mut angle = axis.angle_between(normal)?;
        if angle > PI / 2.0 {
            angle = PI - angle;
            normal = -normal;
        }
        let distance = (bottom - plane_origin).dot(normal);
        let bot_radius = self.bot_radius();
        if PI / 2.0 - angle < ang_tol {
            let ext = self.step_linear_param_extent().0;
            let line_max = ext.length();
            let vec = axis.cross(normal);
            if self.is_cylinder() {
                let scale = height / ext.length();
                if distance.abs() > bot_radius + tol {
                    return Ok((false, out));
                }
                // Upstream: tangent, or the two crossing lines are within tolerance of each
                // other. WR-510 (#1525; tangency-labs/smlib#55, merged as bd20f6e): also
                // tangent within the zone tolerance of a face either surface bounds, so a
                // plane a hair off a face's wall is one line; bare surfaces (zone tolerance
                // 0) keep their two lines. Only when that line lies in the face's sweep: a
                // sweep ending between it and a crossing keeps the crossing.
                let zone_tol = zone_tols[0].unwrap_or(0.0).max(zone_tols[1].unwrap_or(0.0));
                let dir = if distance > 0.0 { -normal } else { normal };
                let line_in_sweep = angle_in_sweep(self, axis, dir, ang_tol)?;
                // (within the zone tolerance as every contact site reads it,
                // `within_contact_tol`)
                let size = bottom.max_dimension().max(plane_origin.max_dimension()) + bot_radius;
                let tangent = distance.abs() >= bot_radius
                    || (distance.abs() > bot_radius - tol
                        && bot_radius * bot_radius - distance * distance < tol * tol)
                    || (zone_tol > 0.0
                        && within_contact_tol(bot_radius - distance.abs(), zone_tol, size)
                        && line_in_sweep);
                // The touch at an end of the sweep (below) is the contact the face's rim
                // reads against the plane's edge, so it takes the rim's tolerance,
                // `contact_tol_3d`, not the faces' zone tolerance, which may have grown
                // past it: between the two the rim crosses, and so does the wall. As at the
                // rim, a wall whose whole circle lies within that tolerance of the plane
                // touches it at no single line and keeps its crossings
                // (`line_coplanar_circle_intersect_distinct`).
                let end_contact = zone_tol > 0.0
                    && within_contact_tol(bot_radius - distance.abs(), contact_tol_3d(k), size)
                    && !within_contact_tol(bot_radius + distance.abs(), contact_tol_3d(k), size);
                if tangent {
                    if line_in_sweep {
                        let start = bottom + bot_radius * dir;
                        let end = start + line_max * scale * axis;
                        add_plane_line(
                            &mut out,
                            plane,
                            other_domain,
                            start,
                            end,
                            tol,
                            TsectCurveType::Tangent,
                        )?;
                    }
                } else {
                    let center = bottom - distance * normal;
                    let val = sqrt(bot_radius * bot_radius - distance * distance);
                    for start in [center + val * vec, center - val * vec] {
                        if angle_in_sweep(self, axis, start - bottom, ang_tol)? {
                            // Not upstream (#20390): a plane in contact with the wall whose
                            // tangent line is off the face stays within the contact
                            // tolerance of the wall from that line to this crossing. An end
                            // of the face's sweep between the two lies on the plane by that
                            // tolerance, so the face touches the plane along it: one tangent
                            // line there, and no crossing a hair further on, as an arc that
                            // starts on a line a hair inside its circle touches it at its
                            // start (`Ellipse::intersect_with_line`, docs/booleans.md §6).
                            // Only with the tangent line off the face (a wall nearly all
                            // the way round holds it, and its start is then no end beside
                            // this crossing), only an end the requested domain holds, and
                            // only where the tangent line meets the plane's own domain: else
                            // the crossing stays, as the one curve of this pair there.
                            let touch = if end_contact && !line_in_sweep {
                                sweep_end_on_the_lens(k, self, domain, axis, dir, start - bottom)?
                            } else {
                                None
                            };
                            let mut touched = false;
                            if let Some(end_dir) = touch {
                                let from = bottom + bot_radius * end_dir;
                                let to = from + line_max * scale * axis;
                                if let (Some(curve), true) =
                                    plane.make_new_plane_line(other_domain, from, to, tol)?
                                {
                                    add(&mut out, curve, TsectCurveType::Tangent);
                                    touched = true;
                                }
                            }
                            if !touched {
                                let end = start + line_max * scale * axis;
                                add_plane_line(
                                    &mut out,
                                    plane,
                                    other_domain,
                                    start,
                                    end,
                                    tol,
                                    TsectCurveType::Crossing,
                                )?;
                            }
                        }
                    }
                }
            } else {
                if distance.abs() > EFF_ZERO {
                    return Ok((true, out));
                }
                for direction in [-vec, vec] {
                    if angle_in_sweep(self, axis, direction, ang_tol)? {
                        let start = bottom + bot_radius * direction;
                        let end = top + self.top_radius() * direction;
                        add_plane_line(
                            &mut out,
                            plane,
                            other_domain,
                            start,
                            end,
                            tol,
                            TsectCurveType::Crossing,
                        )?;
                    }
                }
            }
        } else if angle < ang_tol {
            let Some(mut v) = ignored(line_plane_intersect(
                position.origin(),
                axis,
                plane_origin,
                normal,
            ))?
            else {
                return Ok((true, out));
            };
            let v_domain = step.v_interval();
            if !v_domain.contains_value(v, tol) {
                return Ok((false, out));
            }
            if let Some(t) = approx_tol {
                let t = approx_tol3d(k, t);
                if v < v_domain.min() + t {
                    v = v_domain.min();
                }
                if v > v_domain.max() - t {
                    v = v_domain.max();
                }
            }
            let center = position.origin() + v * axis;
            let radius = self.radius(v).abs();
            let curves = plane.make_new_plane_ellipse_in_contact(
                k,
                other_domain,
                center,
                radius,
                radius,
                position.x_axis(),
                position.y_axis(),
                &step.u_interval(),
                tol,
                contact,
            )?;
            add_plane_ellipses(k, &mut out, curves)?;
        } else {
            let cone_angle = (self.semi_angle_deg() * (PI / 180.0)).abs();
            let plane_angle = PI / 2.0 - angle.abs();
            if !self.is_cylinder() && plane_angle < cone_angle + ang_tol {
                // A plane that holds a whole generator of the cone touches it along that
                // generator: a tangent line, in closed form as the cylinder's above. Only the
                // generator under the normal can lie in the plane: a plane holding a generator
                // and the circle's tangent where it meets the rim is the tangent plane there,
                // whose normal leans from the axis toward that generator. Every other
                // plane steep enough to cut the cone is traced in general, as before.
                //
                // The plane holds the generator when both of its ends are on the plane. Within a
                // face's zone tolerance that is a contact, as for the cylinder (WR-510). A bare
                // surface keeps the cylinder's upstream test. The plane may stand off the cone
                // by up to `tol`. Where it dips in by `e`, it does so by `e / c` within a
                // cross-section, `c` being the cosine of the plane's tilt from the axis. There it
                // cuts a rim of radius at most `r` in a chord whose ends lie `sqrt(2 r e / c)`
                // from the generator. The two crossings are one line only when that is within
                // `tol`.
                let zone_tol = zone_tols[0].unwrap_or(0.0).max(zone_tols[1].unwrap_or(0.0));
                let lean = normal - normal.dot(axis) * axis;
                let tilt = lean.length();
                if tilt > EFF_ZERO {
                    let lean = lean.unitize_or_z();
                    let reach = bot_radius.max(self.top_radius());
                    for d in [lean, -lean] {
                        let start = bottom + bot_radius * d;
                        let end = top + self.top_radius() * d;
                        // How far `p` lies past the plane, away from the axis: above 0 where the
                        // plane cuts into the cone at that end.
                        let side = d.dot(normal).signum();
                        let size = plane_origin
                            .max_dimension()
                            .max(bottom.max_dimension().max(top.max_dimension()) + reach);
                        let held = |p: Point3| {
                            let e = (p - plane_origin).dot(normal) * side;
                            (zone_tol > 0.0 && within_contact_tol(e.abs(), zone_tol, size))
                                || (e <= 0.0 && -e <= tol)
                                || (e > 0.0 && 2.0 * reach * e / tilt <= tol * tol)
                        };
                        if held(start) && held(end) && angle_in_sweep(self, axis, d, ang_tol)? {
                            add_plane_line(
                                &mut out,
                                plane,
                                other_domain,
                                start,
                                end,
                                tol,
                                TsectCurveType::Tangent,
                            )?;
                            return Ok((false, out));
                        }
                    }
                }
                return Ok((true, out));
            }
            let vec = axis.cross(axis.cross(normal)).unitize_or_z();
            let start1 = bottom - bot_radius * vec;
            let end1 = top - self.top_radius() * vec;
            let line1 = end1 - start1;
            let len1 = line1.length();
            let Some(t1) = ignored(line_plane_intersect(start1, line1, plane_origin, normal))?
            else {
                return Ok((true, out));
            };
            let end1 = start1 + t1 * line1;
            let start2 = bottom + bot_radius * vec;
            let end2 = top + self.top_radius() * vec;
            let line2 = end2 - start2;
            let len2 = line2.length();
            let Some(t2) = ignored(line_plane_intersect(start2, line2, plane_origin, normal))?
            else {
                return Ok((true, out));
            };
            // Upstream compares these dimensionless parameters with line lengths.
            if (t1 < 0.0 - len1 && t2 < 0.0 - len2) || (t1 > 1.0 + len1 && t2 > 1.0 + len2) {
                return Ok((false, out));
            }
            let end2 = start2 + t2 * line2;
            let x = (end2 - end1) / 2.0;
            let center = end1 + x;
            let major = x.length();
            if is_zero(major) {
                add(
                    &mut out,
                    Curve::from(BSplineCurve::create_degenerate_curve(3, end1)?),
                    TsectCurveType::Touching,
                );
                return Ok((false, out));
            }
            let x = x.unitize_or_z();
            let y = normal.cross(x).unitize_or_z();
            let minor = if self.is_cylinder() {
                bot_radius
            } else {
                let vec = center - bottom;
                let t = vec.dot(axis);
                let radius = (self.top_radius() - bot_radius) * t / height + bot_radius;
                sqrt(radius * radius - (vec.length_squared() - t * t))
            };
            // The upstream call deliberately uses the natural domain here.
            let intervals = ellipse_trim_intervals(
                k,
                center,
                x,
                y,
                major,
                minor,
                Extent1d::from_min_max(0.0, 360.0),
                self,
                &self.rev().base().natural_uv_domain(),
                tol,
            )?;
            for interval in intervals {
                let curves = plane.make_new_plane_ellipse_in_contact(
                    k,
                    other_domain,
                    center,
                    major,
                    minor,
                    x,
                    y,
                    &interval,
                    tol,
                    contact,
                )?;
                add_plane_ellipses(k, &mut out, curves)?;
            }
        }
        Ok((false, out))
    }

    /// Analytic intersection with another cone (including cylinders).
    /// Returns whether the general intersector is still required and all curves.
    ///
    /// # Errors
    /// Geometry errors, unfinished ellipse trimming or coincident-boundary handling.
    pub fn intersect_with_cone(
        &self,
        k: &Kernel,
        domain: &Extent2d,
        other: &Self,
        other_domain: &Extent2d,
        approx_tol: Option<f64>,
    ) -> Result<(bool, SurfaceIntersection)> {
        self.intersect_with_cone_on(k, domain, other, other_domain, approx_tol, [None, None])
    }

    /// [`Cone::intersect_with_cone`] with the common surfaces wrapping this cone and
    /// `other`, when the caller has them (their owners reach the boundary drops);
    /// bare copies are wrapped otherwise.
    fn intersect_with_cone_on(
        &self,
        k: &Kernel,
        domain: &Extent2d,
        other: &Self,
        other_domain: &Extent2d,
        approx_tol: Option<f64>,
        surfaces: [Option<&Surface>; 2],
    ) -> Result<(bool, SurfaceIntersection)> {
        let mut out = SurfaceIntersection::default();
        let more = self.intersect_with_cone_on_into(
            k,
            domain,
            other,
            other_domain,
            approx_tol,
            surfaces,
            &mut out,
        )?;
        Ok((more, out))
    }

    /// [`Self::intersect_with_cone_on`] into `out` (reset first). On an error `out` keeps what was
    /// appended before it, as upstream leaves its output arrays.
    fn intersect_with_cone_on_into(
        &self,
        k: &Kernel,
        domain: &Extent2d,
        other: &Self,
        other_domain: &Extent2d,
        approx_tol: Option<f64>,
        surfaces: [Option<&Surface>; 2],
        out: &mut SurfaceIntersection,
    ) -> Result<bool> {
        *out = SurfaceIntersection::default();
        if core::ptr::eq(self, other) {
            return Ok(false);
        }
        let box1 = bbox(k, self, domain)?;
        let mut box2 = bbox(k, other, other_domain)?;
        let size = box2.size();
        let tol = approx_tol.map_or_else(
            || EFF_ZERO_SQRT * (1.0 + size.length()),
            |t| approx_tol3d(k, t),
        );
        box2.expand_absolute(tol);
        if box1.are_disjoint(&box2, 0.0) {
            return Ok(false);
        }
        let height = self.height(k)?;
        let axis = self.rev().position().z_axis();
        let origin = self.rev().position().origin();
        let step = self.rev().step_uv_domain();
        let bottom = origin + step.v_min() * axis;
        let top = origin + step.v_max() * axis;
        let other_axis = other.rev().position().z_axis();
        let other_origin = other.rev().position().origin();
        let other_step = other.rev().step_uv_domain();
        let other_bottom = other_origin + other_step.v_min() * other_axis;
        let other_top = other_origin + other_step.v_max() * other_axis;
        let mut angle = axis.angle_between(other_axis)?;
        if angle > PI / 2.0 {
            angle = PI - angle;
        }
        if angle < EFF_ZERO_SQRT {
            let param = line_closest_point(bottom, axis, other_bottom);
            let point = bottom + param * axis;
            let distance = point.distance(other_bottom);
            let min_radius = min2(self.bot_radius(), self.top_radius());
            let max_radius = max2(self.bot_radius(), self.top_radius());
            let other_min_radius = min2(other.bot_radius(), other.top_radius());
            let other_max_radius = max2(other.bot_radius(), other.top_radius());
            if distance - tol > max_radius + other_max_radius
                || max_radius + tol < other_min_radius - distance
                || other_max_radius + tol < min_radius - distance
            {
                return Ok(false);
            }
            let param2 = line_closest_point(bottom, axis, other_top);
            if (param > height + tol && param2 > height + tol)
                || (param < 0.0 - tol && param2 < 0.0 - tol)
            {
                return Ok(false);
            }
            if !self.is_cylinder() || !other.is_cylinder() {
                return Ok(true);
            }
            // Face domains can be shorter than either underlying cylinder, where
            // upstream spans the full cylinders (WR-259). Work relative to
            // `bottom`, as the circle/line construction below does. STEP conversion
            // handles swapped UV and inside-out surfaces; projection also handles
            // the other cylinder's reversed axis.
            let trim = self.convert_domain_from_nurbs_to_step(k, domain)?;
            let other_trim = other.convert_domain_from_nurbs_to_step(k, other_domain)?;
            let mut min = trim.v_min() - step.v_min();
            let mut max = trim.v_max() - step.v_min();
            let other_trim_bottom = other_origin + other_trim.v_min() * other_axis;
            let other_trim_top = other_origin + other_trim.v_max() * other_axis;
            let other_param = line_closest_point(bottom, axis, other_trim_bottom);
            let other_param2 = line_closest_point(bottom, axis, other_trim_top);
            let other_min = min2(other_param, other_param2);
            let other_max = max2(other_param, other_param2);
            if other_min > max + tol || other_max < min - tol {
                return Ok(false);
            }
            if other_min > max - tol {
                min = max;
            }
            if other_max < min + tol {
                max = min;
            }
            if other_min > min && other_min <= max {
                min = other_min;
            }
            if other_max < max && other_max >= min {
                max = other_max;
            }
            if distance <= tol {
                // (the boundary calls take the clamped `*pdOptApproxTol3d`)
                let boundary_tol = approx_tol.map(|t| approx_tol3d(k, t));
                let (own_this, own_other);
                let this_surface = if let Some(s) = surfaces[0] {
                    s
                } else {
                    own_this = Surface::from(self.clone());
                    &own_this
                };
                let other_surface = if let Some(s) = surfaces[1] {
                    s
                } else {
                    own_other = Surface::from(other.clone());
                    &own_other
                };
                if let Some(first) = ignored(boundary_intersections(
                    k,
                    this_surface,
                    domain,
                    other_surface,
                    other_domain,
                    boundary_tol,
                ))? {
                    append(out, first);
                }
                let second = boundary_intersections(
                    k,
                    other_surface,
                    other_domain,
                    this_surface,
                    domain,
                    boundary_tol,
                );
                // Upstream overwrites the first status/needs-more with the second.
                let second = second?;
                append_boundary_intersections(k, out, second, tol)?;
                return Ok(false);
            }
            let start = bottom + param * axis;
            // walls a hair less than tangent touch along one line where both are faces'
            // (WR-787, #15429); bare surfaces keep upstream's two crossings
            let faces = surfaces
                .iter()
                .all(|s| s.is_some_and(|s| s.owner_zone_tol_3d().is_some()));
            let intersection = circle_coplanar_circle_intersect_in_contact(
                axis,
                start,
                self.bot_radius(),
                other_bottom,
                other.bot_radius(),
                tol,
                if faces { contact_tol_3d(k) } else { 0.0 },
            )?;
            let count = intersection.count();
            let points = match intersection {
                CircleCircle::None => Vec::new(),
                CircleCircle::Tangent(p) => vec![p],
                CircleCircle::Crossing(p, q) => vec![p, q],
                CircleCircle::Coincident => return Ok(true),
            };
            // Axes within EFF_ZERO_SQRT are parallel here, but a move along `axis` leaves `other`
            // by that angle times the move, past `zero` (#15727): test on `other`'s generator.
            let other_along = if axis.dot(other_axis) < 0.0 {
                -other_axis
            } else {
                other_axis
            };
            for point in points {
                let line_point = point + (min - param) * axis;
                // Upstream tests the bottom end of the line, which lies outside a face
                // domain trimmed above the surface's own bottom, and skips the whole
                // line (a face of a cylinder cut by an earlier Boolean; upstream defect
                // WR-259). The middle of the clipped line lies within both faces' axial
                // ranges (to the tolerance); the inversions below decide the rest.
                let test_point = line_point + 0.5 * (max - min) * axis;
                let other_test_point = point + ((min - param) + 0.5 * (max - min)) * other_along;
                let zero = EFF_ZERO * (1.0 + line_point.max_dimension());
                let inv = intersection_inversion(self, k, test_point, zero)?;
                let other_inv = intersection_inversion(other, k, other_test_point, zero)?;
                let (Some(inv), Some(other_inv)) = (inv, other_inv) else {
                    continue;
                };
                if inv.location == LocationType::Exterior
                    || other_inv.location == LocationType::Exterior
                {
                    continue;
                }
                // (both statuses ignored upstream)
                let uv = nurbs_uv_status_ignored(self, k, inv.uv)?;
                let other_uv = nurbs_uv_status_ignored(other, k, other_inv.uv)?;
                if !domain.contains_point(uv, EFF_ZERO)
                    || !other_domain.contains_point(other_uv, EFF_ZERO)
                {
                    continue;
                }
                if max - min < tol {
                    add(
                        out,
                        Curve::from(BSplineCurve::create_degenerate_curve(3, line_point)?),
                        TsectCurveType::Touching,
                    );
                } else {
                    let curve = Line::bounded(
                        line_point,
                        axis,
                        &Extent1d::from_min_max(0.0, max - min),
                        &BoundedLineOptions {
                            scale: 1.0,
                            ..BoundedLineOptions::default()
                        },
                    );
                    add(
                        out,
                        Curve::from(curve),
                        if count == 1 {
                            TsectCurveType::Tangent
                        } else {
                            TsectCurveType::Crossing
                        },
                    );
                }
            }
        } else {
            if !self.is_cylinder()
                || !other.is_cylinder()
                || (self.top_radius() - other.top_radius()).abs() > tol
            {
                return Ok(true);
            }
            let points = segment_segment_intersect(bottom, top, other_bottom, other_top, tol)?;
            let [point] = points.as_slice() else {
                return Ok(true);
            };
            let (vec1, vec2) = parallelogram_cross_vectors(
                axis,
                other_axis,
                self.top_radius(),
                other.top_radius(),
            )?;
            let x = axis.cross(other_axis).unitize()?;
            let mut degenerate = Vec::new();
            for (vec, checked) in [(vec1, true), (vec2, false)] {
                let radius = vec.length();
                let y = if checked {
                    vec.unitize()?
                } else {
                    vec.unitize_or_z()
                };
                for y in [y, -y] {
                    let mut frame = Axis2Placement::IDENTITY;
                    frame.set_canonical(*point, x, y)?;
                    add_ellipse_intervals_trimmed_to_two_cones(
                        k,
                        frame.origin(),
                        frame.x_axis(),
                        frame.y_axis(),
                        self.top_radius(),
                        radius,
                        Extent1d::from_min_max(0.0, 180.0),
                        self,
                        domain,
                        other,
                        other_domain,
                        tol,
                        &mut out.curves_3d,
                        &mut degenerate,
                    )?;
                }
            }
            let count = out.curves_3d.len();
            for curve in degenerate {
                let point = curve.evaluate_point(k, curve.natural_interval().min())?;
                let mut unique = true;
                for existing in out.curves_3d.iter().take(count) {
                    if ellipse_contains(k, existing, point)? {
                        unique = false;
                        break;
                    }
                }
                if unique {
                    out.curves_3d.push(curve);
                }
            }
            for _ in &out.curves_3d {
                out.surface1_uv_curves.push(None);
                out.surface2_uv_curves.push(None);
                out.curve_types.push(TsectCurveType::Crossing);
                out.deviations.push(0.0);
            }
        }
        Ok(false)
    }
}

// The native intersection ignores STEPInversion's status, then skips an
// EXTERIOR classification. The one local error below is exactly the generator
// solve's empty-result branch: it explicitly writes EXTERIOR before returning
// SM_ERR. No UV coordinate is fabricated or consumed on that branch.
// Start-plane transform errors still propagate (see the module Deviations), which
// is what the SolidCraft fork does (tangency-labs/smlib dfc7cb6, `SER` unless
// `STEPInversionAnswered`; WR-662).
fn intersection_inversion(
    cone: &Cone,
    k: &Kernel,
    point: Point3,
    tol: f64,
) -> Result<Option<ConeStepInversion>> {
    match cone.step_inversion(k, point, tol, None) {
        Ok(inversion) => Ok(Some(inversion)),
        // by its status, so also a typed failure that replaced the `Upstream` (#13790)
        Err(e) if crate::surface::surf_of_revolution::p2::is_off_generator(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

/// `ConvertUVFromSTEPToNURBS(StepUV, NurbUV)` with its status ignored
/// (`SmCone.cpp:2966-2967`). `NurbUV` is a default `SmPoint2d`, both coordinates
/// `SM_UNDEF_DOUBLE`. A failed polar conversion returns before the height is
/// converted, so `x` holds what the converter had written (`SM_UNDEF_DOUBLE` when
/// nothing) and `y` stays `SM_UNDEF_DOUBLE`; the caller's domain test then decides.
/// Unported and a missing polar curve (a `NULL` dereference upstream) propagate.
fn nurbs_uv_status_ignored(cone: &Cone, k: &Kernel, step_uv: Point2) -> Result<Point2> {
    match cone.convert_uv_from_step_to_nurbs(k, step_uv) {
        Ok(uv) => Ok(uv),
        Err(e) if e.is_unported_or_cancelled() => Err(e),
        Err(e) => {
            let circle = cone.rev().polar_curve().ok_or(e)?;
            let x = cone
                .rev()
                .polar
                .convert_to_nurbs_parameter_or_written(k, circle, step_uv.x, 0.0)?;
            Ok(Point2::new(x.unwrap_or(UNDEF_DOUBLE), UNDEF_DOUBLE))
        }
    }
}

fn ellipse_contains(k: &Kernel, curve: &Curve, point: Point3) -> Result<bool> {
    let CurveKind::Ellipse(ellipse) = curve.kind() else {
        return Err(SolidError::surface(
            OP,
            SurfaceFailure::Defect {
                why: "a curve compared is not an ellipse, though both callers compare only \
                      the ellipse arcs the cone/cone branch built",
            },
        ));
    };
    Ok(ellipse.step_inversion(k, point, true, 0.0).1 != Some(CurveLocationType::Exterior))
}

fn add_ellipse_intervals_trimmed_to_two_cones(
    k: &Kernel,
    center: Point3,
    x: Vec3,
    y: Vec3,
    rx: f64,
    ry: f64,
    angles: Extent1d,
    cone: &Cone,
    domain: &Extent2d,
    other: &Cone,
    other_domain: &Extent2d,
    tol: f64,
    curves: &mut Vec<Curve>,
    degenerate: &mut Vec<Curve>,
) -> Result<()> {
    let zero_ang = EFF_ZERO * 100.0 * (1.0 + 360.0);
    let intervals = ellipse_trim_intervals(k, center, x, y, rx, ry, angles, cone, domain, tol)?;
    let other_intervals =
        ellipse_trim_intervals(k, center, x, y, rx, ry, angles, other, other_domain, tol)?;
    for interval in intervals {
        for other_interval in &other_intervals {
            for intersection in interval.intersect_periodic(other_interval, 360.0, false)? {
                if intersection.length() < zero_ang {
                    let angle = intersection.min();
                    let cos = (angle * PI / 180.0).cos();
                    let sin = (angle * PI / 180.0).sin();
                    let point = center + rx * cos * x + y * sin * ry;
                    let mut unique = true;
                    for existing in degenerate.iter() {
                        let old = existing.evaluate_point(k, existing.natural_interval().min())?;
                        let zero = EFF_ZERO * 100.0 * (1.0 * point.max_dimension());
                        if old.distance(point) < zero {
                            unique = false;
                            break;
                        }
                    }
                    if unique {
                        for existing in curves.iter() {
                            if ellipse_contains(k, existing, point)? {
                                unique = false;
                                break;
                            }
                        }
                    }
                    if unique {
                        degenerate.push(Curve::from(BSplineCurve::create_degenerate_curve(
                            3, point,
                        )?));
                    }
                } else {
                    curves.push(Curve::from(Ellipse::new(
                        k,
                        center,
                        x,
                        y,
                        intersection,
                        rx,
                        ry,
                        &EllipseOptions::default(),
                    )?));
                }
            }
        }
    }
    Ok(())
}

impl Revolved for Cone {
    fn as_surf_of_revolution(&self) -> &SurfOfRevolution {
        self.rev()
    }
    fn convert_domain_from_nurbs_to_step(&self, k: &Kernel, domain: &Extent2d) -> Result<Extent2d> {
        crate::surface::surface::p1::convert_domain_from_nurbs_to_step_with(domain, |uv| {
            self.convert_uv_from_nurbs_to_step(k, uv)
        })
    }
}

impl Cone {
    /// Global analytic dispatch, followed by the supplied general intersector when
    /// the analytic routine requests it. The callback must retain cone dispatch;
    /// converting the cone to its revolution base loses overrides.
    ///
    /// # Errors
    /// Errors from the selected analytic routine or the general callback.
    pub fn global_surface_intersect_with<'a>(
        &'a self,
        k: &'a Kernel,
        domain: &'a Extent2d,
        other: IntersectTarget<'a>,
        other_domain: &'a Extent2d,
        use_surface_edges: [bool; 2],
        approx_tol_3d: Option<f64>,
        ang_tol_rad: Option<f64>,
        general: impl FnOnce(GeneralIntersectionRequest<'a>) -> Result<SurfaceIntersection>,
    ) -> Result<SurfaceIntersection> {
        self.global_surface_intersect_on(
            None,
            k,
            domain,
            other,
            other_domain,
            use_surface_edges,
            approx_tol_3d,
            ang_tol_rad,
            general,
        )
    }

    /// [`Cone::global_surface_intersect_with`] for the common surface `this` wrapping
    /// this cone (`None` wraps a bare copy where the boundary path needs one).
    ///
    /// # Errors
    /// Those of [`Cone::global_surface_intersect_with`].
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn global_surface_intersect_on<'a>(
        &'a self,
        this: Option<&'a Surface>,
        k: &'a Kernel,
        domain: &'a Extent2d,
        other: IntersectTarget<'a>,
        other_domain: &'a Extent2d,
        use_surface_edges: [bool; 2],
        approx_tol_3d: Option<f64>,
        ang_tol_rad: Option<f64>,
        general: impl FnOnce(GeneralIntersectionRequest<'a>) -> Result<SurfaceIntersection>,
    ) -> Result<SurfaceIntersection> {
        let mut out = SurfaceIntersection::default();
        self.global_surface_intersect_on_into(
            this,
            k,
            domain,
            other,
            other_domain,
            use_surface_edges,
            approx_tol_3d,
            ang_tol_rad,
            |req, out| {
                *out = general(req)?;
                Ok(())
            },
            &mut out,
        )?;
        Ok(out)
    }

    /// [`Cone::global_surface_intersect_on`] into `out` (upstream's output arrays). On an
    /// error `out` keeps what the failing special case (or `general`, which fills it)
    /// appended before it, as upstream leaves its output arrays; the general
    /// intersection resets them first.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn global_surface_intersect_on_into<'a, G>(
        &'a self,
        this: Option<&'a Surface>,
        k: &'a Kernel,
        domain: &'a Extent2d,
        other: IntersectTarget<'a>,
        other_domain: &'a Extent2d,
        use_surface_edges: [bool; 2],
        approx_tol_3d: Option<f64>,
        ang_tol_rad: Option<f64>,
        general: G,
        out: &mut SurfaceIntersection,
    ) -> Result<()>
    where
        G: FnOnce(GeneralIntersectionRequest<'a>, &mut SurfaceIntersection) -> Result<()>,
    {
        // (each special case resets the outputs on entry)
        *out = SurfaceIntersection::default();
        let other_surface = match other {
            IntersectTarget::Surface(s) => Some(s),
            _ => None,
        };
        let target = match other {
            IntersectTarget::Surface(s) => match s.kind() {
                SurfaceKind::Plane(p) => IntersectTarget::Plane(p),
                SurfaceKind::SurfOfRevolution(r) => IntersectTarget::Revolution(r),
                SurfaceKind::Cone(c) => IntersectTarget::Cone(c.as_ref()),
                SurfaceKind::Sphere(s) => IntersectTarget::Sphere(s.as_ref()),
                SurfaceKind::SurfOfExtrusion(e) => IntersectTarget::Extrusion(e.as_ref()),
                _ => other,
            },
            _ => other,
        };
        let needs_more = match target {
            IntersectTarget::Plane(p) => {
                // the zone tolerances of this cone's face and of the plane's (WR-510)
                let (more, found) = self.intersect_with_plane_owned(
                    k,
                    domain,
                    p,
                    PlaneOwner::NoFace,
                    other_domain,
                    approx_tol_3d,
                    [
                        this.and_then(Surface::owner_zone_tol_3d),
                        other_surface.and_then(Surface::owner_zone_tol_3d),
                    ],
                )?;
                *out = found;
                more
            }
            IntersectTarget::Cone(c) if matches!(c.type_code(), CONE_TYPE | CYLINDER_TYPE) => self
                .intersect_with_cone_on_into(
                    k,
                    domain,
                    c,
                    other_domain,
                    approx_tol_3d,
                    [this, other_surface],
                    out,
                )?,
            // the other surface's first surface is this one: its UV curves are swapped,
            // also in a partial output
            IntersectTarget::Revolution(r) => {
                let more =
                    r.intersect_with_cone_into(k, other_domain, self, domain, approx_tol_3d, out);
                core::mem::swap(&mut out.surface1_uv_curves, &mut out.surface2_uv_curves);
                more?
            }
            IntersectTarget::Sphere(s) => {
                let more =
                    s.intersect_with_cone_into(k, other_domain, self, domain, approx_tol_3d, out);
                core::mem::swap(&mut out.surface1_uv_curves, &mut out.surface2_uv_curves);
                more?
            }
            IntersectTarget::Extrusion(e) if self.is_cylinder() => {
                let r = e.intersect_with_cylinder(k, self, approx_tol_3d)?;
                *out = SurfaceIntersection {
                    curves_3d: r.curves_3d,
                    surface1_uv_curves: r.surface2_uv_curves,
                    surface2_uv_curves: r.surface1_uv_curves,
                    curve_types: r.curve_types,
                    deviations: r.deviations,
                };
                r.needs_more_intersections
            }
            _ => true,
        };
        if needs_more {
            // DoIntersection resets every native output before its general solve.
            *out = SurfaceIntersection::default();
            general(
                GeneralIntersectionRequest {
                    kernel: k,
                    cone: self,
                    domain,
                    other,
                    other_domain,
                    use_surface_edges,
                    approx_tol_3d,
                    ang_tol_rad,
                },
                out,
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cylinder(k: &Kernel, origin: Vec3, reverse: bool, swap: bool, inside: bool) -> Cone {
        let mut c = Cone::new_cylinder_bounded(
            origin,
            Vec3::X,
            if reverse { -Vec3::Y } else { Vec3::Y },
            5.0,
            0.0,
            360.0,
            12.0,
            swap,
            inside,
            false,
        );
        c.make_nurb(k).unwrap();
        c
    }

    fn axial_domain(k: &Kernel, c: &Cone, min: f64, max: f64) -> Extent2d {
        let step = c.rev().step_uv_domain();
        Extent2d::from_points(
            c.convert_uv_from_step_to_nurbs(k, Point2::new(step.u_min(), min))
                .unwrap(),
            c.convert_uv_from_step_to_nurbs(k, Point2::new(step.u_max(), max))
                .unwrap(),
        )
    }

    /// `count` lines along z from `z_min` to `z_max`, both ends on both cylinders `on`.
    fn assert_axial_lines(
        out: &SurfaceIntersection,
        on: [&Cone; 2],
        count: usize,
        z_min: f64,
        z_max: f64,
    ) {
        assert_eq!(out.curves_3d.len(), count);
        for curve in &out.curves_3d {
            let CurveKind::Line(line) = curve.kind() else {
                panic!("expected an analytic line");
            };
            let interval = line.natural_interval();
            let start = line.evaluate_point(interval.min());
            let end = line.evaluate_point(interval.max());
            assert!(
                (start.z.min(end.z) - z_min).abs() < 1e-10,
                "{start:?} {end:?}"
            );
            assert!(
                (start.z.max(end.z) - z_max).abs() < 1e-10,
                "{start:?} {end:?}"
            );
            assert!((start.x - end.x).abs() < 1e-10);
            assert!((start.y - end.y).abs() < 1e-10);
            for p in [start, end] {
                for c in on {
                    let origin = c.rev().position().origin();
                    let r = (p.x - origin.x).hypot(p.y - origin.y);
                    assert!((r - c.bot_radius()).abs() < 1e-9, "{p:?}");
                }
            }
        }
    }

    #[test]
    fn issue711_overlapping_holes_keep_both_trimmed_intersections() {
        let k = Kernel::default();
        let a = cylinder(&k, Vec3::new(6.0, 0.0, -1.0), false, false, false);
        let b = cylinder(&k, Vec3::new(3.0, 5.196, -1.0), false, false, false);
        let da = axial_domain(&k, &a, 1.0, 11.0);
        let db = b.rev().base().natural_uv_domain();
        for (a, da, b, db) in [(&a, da, &b, db), (&b, db, &a, da)] {
            let (more, out) = a.intersect_with_cone(&k, &da, b, &db, Some(1e-7)).unwrap();
            assert!(!more);
            assert_axial_lines(&out, [a, b], 2, 0.0, 10.0);
            assert_eq!(out.curve_types, vec![TsectCurveType::Crossing; 2]);
        }
    }

    /// tangency-labs/solidcraft#15727: with one axis 5e-12 rad off, the holes of the issue711 test
    /// still cross in both lines, in either order; both were dropped and the cut came back empty.
    #[test]
    fn cylinders_a_hair_off_parallel_keep_both_crossing_lines() {
        let k = Kernel::default();
        let a = cylinder(&k, Vec3::new(6.0, 0.0, -1.0), false, false, false);
        let mut b = Cone::new_cylinder_bounded(
            Vec3::new(3.0, 5.196, -1.0),
            Vec3::new(1.0, 0.0, -5e-12),
            Vec3::Y,
            5.0,
            0.0,
            360.0,
            12.0,
            false,
            false,
            false,
        );
        b.make_nurb(&k).unwrap();
        let da = axial_domain(&k, &a, 1.0, 11.0);
        let db = b.rev().base().natural_uv_domain();
        for (a, da, b, db) in [(&a, da, &b, db), (&b, db, &a, da)] {
            let (more, out) = a.intersect_with_cone(&k, &da, b, &db, Some(1e-7)).unwrap();
            assert!(!more);
            assert_axial_lines(&out, [a, b], 2, 0.0, 10.0);
            assert_eq!(out.curve_types, vec![TsectCurveType::Crossing; 2]);
        }
    }

    #[test]
    fn issue711_axial_trims_respect_both_parameterizations_and_axis_directions() {
        let k = Kernel::default();
        for flags in 0..32 {
            let reverse = flags & 16 != 0;
            let a = cylinder(&k, Vec3::ZERO, false, flags & 1 != 0, flags & 2 != 0);
            let b = cylinder(
                &k,
                Vec3::new(6.0, 0.0, if reverse { 12.0 } else { 0.0 }),
                reverse,
                flags & 4 != 0,
                flags & 8 != 0,
            );
            let da = axial_domain(&k, &a, 1.0, 9.0);
            let db = if reverse {
                axial_domain(&k, &b, 2.0, 9.0)
            } else {
                axial_domain(&k, &b, 3.0, 10.0)
            };
            let (more, out) = a.intersect_with_cone(&k, &da, &b, &db, Some(1e-7)).unwrap();
            assert!(!more, "flags={flags}");
            assert_axial_lines(&out, [&a, &b], 2, 3.0, 9.0);
            assert_eq!(out.curve_types, vec![TsectCurveType::Crossing; 2]);
        }
    }

    #[test]
    fn issue711_upper_trim_shortens_lines_and_tangent_lines() {
        let k = Kernel::default();
        let a = cylinder(&k, Vec3::ZERO, false, false, false);
        for (offset, count, kind) in [
            (6.0, 2, TsectCurveType::Crossing),
            (10.0, 1, TsectCurveType::Tangent),
        ] {
            let b = cylinder(&k, Vec3::new(offset, 0.0, 0.0), false, false, false);
            for (end_a, end_b) in [(4.0, 12.0), (12.0, 4.0)] {
                let da = axial_domain(&k, &a, 0.0, end_a);
                let db = axial_domain(&k, &b, 0.0, end_b);
                let (more, out) = a.intersect_with_cone(&k, &da, &b, &db, Some(1e-7)).unwrap();
                assert!(!more);
                assert_axial_lines(&out, [&a, &b], count, 0.0, 4.0);
                assert_eq!(out.curve_types, vec![kind; count]);
            }
        }
    }

    #[test]
    fn issue711_lines_end_where_either_face_ends() {
        let k = Kernel::default();
        let a = cylinder(&k, Vec3::ZERO, false, false, false);
        let b = cylinder(&k, Vec3::new(6.0, 0.0, 0.0), false, false, false);
        // The full line's bottom (0) and middle (6) are both outside this
        // overlap: the lines are clipped to the faces and tested inside them.
        let da = axial_domain(&k, &a, 1.0, 4.0);
        let db = axial_domain(&k, &b, 2.0, 5.0);
        let (more, out) = a.intersect_with_cone(&k, &da, &b, &db, Some(1e-7)).unwrap();
        assert!(!more);
        assert_axial_lines(&out, [&a, &b], 2, 2.0, 4.0);
        assert_eq!(out.curve_types, vec![TsectCurveType::Crossing; 2]);
        // A line's bottom in both faces does not make it run on past the
        // shorter face's top.
        let da = axial_domain(&k, &a, 0.0, 4.0);
        let db = b.rev().base().natural_uv_domain();
        let (more, out) = a.intersect_with_cone(&k, &da, &b, &db, Some(1e-7)).unwrap();
        assert!(!more);
        assert_axial_lines(&out, [&a, &b], 2, 0.0, 4.0);
        assert_eq!(out.curve_types, vec![TsectCurveType::Crossing; 2]);
    }

    #[test]
    fn issue711_axial_disjoint_and_touching_domains() {
        let k = Kernel::default();
        let a = cylinder(&k, Vec3::ZERO, false, false, false);
        let b = cylinder(&k, Vec3::new(6.0, 0.0, 0.0), false, false, false);
        let da = axial_domain(&k, &a, 1.0, 4.0);
        for (start_b, count) in [(5.0, 0), (4.0, 2)] {
            let db = axial_domain(&k, &b, start_b, 9.0);
            let (more, out) = a.intersect_with_cone(&k, &da, &b, &db, Some(1e-7)).unwrap();
            assert!(!more);
            assert_eq!(out.curves_3d.len(), count);
            assert_eq!(out.curve_types, vec![TsectCurveType::Touching; count]);
            for curve in &out.curves_3d {
                let p = curve
                    .evaluate_point(&k, curve.natural_interval().min())
                    .unwrap();
                assert!((p.z - 4.0).abs() < 1e-10);
            }
        }
    }

    #[test]
    fn issue711_trimmed_cylinder_with_nonzero_step_origin() {
        let k = Kernel::default();
        let mut a = cylinder(&k, Vec3::ZERO, false, false, false);
        let da = axial_domain(&k, &a, 2.0, 10.0);
        a.trim_with_domain(&k, &da).unwrap();
        assert!((a.rev().step_uv_domain().v_min() - 2.0).abs() < 1e-10);
        let b = cylinder(&k, Vec3::new(6.0, 0.0, 0.0), false, false, false);
        let da = axial_domain(&k, &a, 3.0, 9.0);
        let db = axial_domain(&k, &b, 4.0, 11.0);
        let (more, out) = a.intersect_with_cone(&k, &da, &b, &db, Some(1e-7)).unwrap();
        assert!(!more);
        assert_axial_lines(&out, [&a, &b], 2, 4.0, 9.0);
    }

    #[test]
    fn issue711_coaxial_faces_apart_along_the_axis_share_nothing() {
        let k = Kernel::default();
        let a = cylinder(&k, Vec3::ZERO, false, false, false);
        let b = cylinder(&k, Vec3::ZERO, false, false, false);
        let da = axial_domain(&k, &a, 0.0, 4.0);
        let db = axial_domain(&k, &b, 6.0, 10.0);
        let (more, out) = a.intersect_with_cone(&k, &da, &b, &db, Some(1e-7)).unwrap();
        assert!(!more);
        assert!(out.curves_3d.is_empty());
        // Overlapping faces still take the coincident-surface boundary curves,
        // all on the common wall.
        let db = axial_domain(&k, &b, 2.0, 10.0);
        let (_, out) = a.intersect_with_cone(&k, &da, &b, &db, Some(1e-7)).unwrap();
        assert!(!out.curves_3d.is_empty());
        for curve in &out.curves_3d {
            let n = curve.natural_interval();
            for t in [n.min(), 0.5 * (n.min() + n.max()), n.max()] {
                let p = curve.evaluate_point(&k, t).unwrap();
                assert!((p.x.hypot(p.y) - 5.0).abs() < 1e-7, "{p:?}");
            }
        }
    }

    /// A cone whose y axis is zero has no polar converter (upstream dereferences
    /// `NULL`): converting its face domain refuses the intersection.
    #[test]
    fn parallel_cylinders_without_polar_converter_refuse() {
        let k = Kernel::default();
        let mut a = Cone::new_bounded(
            Vec3::ZERO,
            Vec3::X,
            Vec3::ZERO,
            1.5628,
            1.5628,
            0.0,
            360.0,
            1.1078,
            false,
            true,
            false,
        );
        a.make_nurb(&k).unwrap();
        let mut b = Cone::new_bounded(
            Vec3::new(0.0, 0.5539, 0.5539),
            Vec3::X,
            Vec3::Z,
            1.5628,
            1.5628,
            0.0,
            360.0,
            1.1078,
            false,
            false,
            false,
        );
        b.make_nurb(&k).unwrap();
        let da = a.rev().base().natural_uv_domain();
        let db = b.rev().base().natural_uv_domain();
        assert!(a.intersect_with_cone(&k, &da, &b, &db, Some(1e-7)).is_err());
    }

    /// Section of the radius-3 cylinder along z by the plane `x = d`, parallel to its axis,
    /// the plane owned as `owner` and the zone tolerances `zone_tols` (cone, plane).
    fn wall_section(
        k: &Kernel,
        c: &Cone,
        d: f64,
        owner: PlaneOwner,
        zone_tols: [Option<f64>; 2],
    ) -> SurfaceIntersection {
        let wall = axial_domain(k, c, 0.0, 12.0);
        wall_section_within(k, c, d, owner, zone_tols, &wall, -20.0)
    }

    /// [`wall_section`] with the requested part of the wall, `wall`, and the plane's patch
    /// from `y_min` up (to 20, and 20 either way along z).
    fn wall_section_within(
        k: &Kernel,
        c: &Cone,
        d: f64,
        owner: PlaneOwner,
        zone_tols: [Option<f64>; 2],
        wall: &Extent2d,
        y_min: f64,
    ) -> SurfaceIntersection {
        let plane = Plane::from_normal(Vec3::new(d, 0.0, 0.0), Vec3::X);
        // the patch's corners in the plane's own coordinates
        let (origin, u, v) = (
            plane.position().origin(),
            plane.position().x_axis(),
            plane.position().y_axis(),
        );
        let corners = [(y_min, -20.0), (y_min, 20.0), (20.0, -20.0), (20.0, 20.0)]
            .map(|(y, z)| Vec3::new(d, y, z) - origin)
            .map(|p| (p.dot(u), p.dot(v)));
        let low = |pick: fn(&(f64, f64)) -> f64| corners.iter().map(pick).fold(f64::MAX, f64::min);
        let high = |pick: fn(&(f64, f64)) -> f64| corners.iter().map(pick).fold(f64::MIN, f64::max);
        let plane_domain = Extent2d::from_points(
            Point2::new(low(|c| c.0), low(|c| c.1)),
            Point2::new(high(|c| c.0), high(|c| c.1)),
        );
        let (more, out) = c
            .intersect_with_plane_owned(k, wall, &plane, owner, &plane_domain, None, zone_tols)
            .unwrap();
        assert!(!more);
        out
    }

    /// The cone from radius 4 at z = 0 to radius 2 at z = 6 (axis z). Its generator from
    /// (4, 0, 0) to (2, 0, 6) runs along g = (-2, 0, 6); the plane holding it and the rim's
    /// tangent y there has the normal g x y, along (3, 0, 1). That plane touches the cone
    /// along the generator: one tangent line, its ends on the generator's ends, decided in
    /// closed form. The same plane moved 0.5 off the cone, clear of it, still goes to the
    /// general intersection, as every plane steep enough to cut a cone did before.
    #[test]
    fn a_plane_holding_a_cone_generator_touches_it_along_that_line() {
        let k = Kernel::default();
        let mut c = Cone::new_bounded(
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            4.0,
            2.0,
            0.0,
            360.0,
            6.0,
            false,
            false,
            false,
        );
        c.make_nurb(&k).unwrap();
        let normal = Vec3::new(3.0, 0.0, 1.0).unitize_or_z();
        let plane_domain = Extent2d::from_bounds(-20.0, -20.0, 20.0, 20.0);
        let cut = |at: Vec3| {
            let plane = Plane::from_normal(at, normal);
            c.intersect_with_plane_owned(
                &k,
                &axial_domain(&k, &c, 0.0, 6.0),
                &plane,
                PlaneOwner::NoFace,
                &plane_domain,
                None,
                [None, None],
            )
            .unwrap()
        };
        let (more, out) = cut(Vec3::new(4.0, 0.0, 0.0));
        assert!(!more);
        assert_eq!(out.curve_types, vec![TsectCurveType::Tangent]);
        let CurveKind::Line(line) = out.curves_3d[0].kind() else {
            panic!("expected an analytic line");
        };
        let ivl = line.natural_interval();
        let mut ends = [
            line.evaluate_point(ivl.min()),
            line.evaluate_point(ivl.max()),
        ];
        ends.sort_by(|a, b| a.z.total_cmp(&b.z));
        for (p, want) in ends
            .iter()
            .zip([Vec3::new(4.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 6.0)])
        {
            assert!((*p - want).length() < 1e-9, "{p:?} against {want:?}");
        }
        let (more, out) = cut(Vec3::new(4.0, 0.0, 0.0) + 0.5 * normal);
        assert!(more && out.curves_3d.is_empty());
    }

    /// The plane of the test above dipped 5e-8 into the cone (toward its axis), cut at a
    /// tolerance of 1e-7. Within a cross-section it dips in by e / c = 5e-8 * sqrt(10) / 3
    /// (c = 3 / sqrt(10), the cosine of its tilt from the axis), so at z = 0 it cuts the rim of
    /// radius 4 in a chord whose ends lie sqrt(2 * 4 * 5.27e-8) = 6.5e-4 from the generator:
    /// two crossing curves, 6500 times the tolerance apart, are not one tangent line, and the
    /// plane goes to the general intersection. Exactly on the generator it is the tangent line
    /// at that tolerance too. Against a face whose zone tolerance is 1e-7, the dip is within
    /// the zone, and the plane touches the face's wall along the generator, as a plane a hair
    /// off a cylinder's wall does (WR-510).
    #[test]
    fn a_plane_dipped_into_a_cone_past_its_tolerance_crosses_it_instead_of_touching() {
        let k = Kernel::default();
        let mut c = Cone::new_bounded(
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            4.0,
            2.0,
            0.0,
            360.0,
            6.0,
            false,
            false,
            false,
        );
        c.make_nurb(&k).unwrap();
        let normal = Vec3::new(3.0, 0.0, 1.0).unitize_or_z();
        let plane_domain = Extent2d::from_bounds(-20.0, -20.0, 20.0, 20.0);
        let cut = |at: Vec3, zone: Option<f64>| {
            let plane = Plane::from_normal(at, normal);
            c.intersect_with_plane_owned(
                &k,
                &axial_domain(&k, &c, 0.0, 6.0),
                &plane,
                PlaneOwner::NoFace,
                &plane_domain,
                Some(1e-7),
                [zone, zone],
            )
            .unwrap()
        };
        let on = Vec3::new(4.0, 0.0, 0.0);
        let dipped = on - 5e-8 * normal;
        let (more, out) = cut(dipped, None);
        assert!(more && out.curves_3d.is_empty(), "{:?}", out.curve_types);
        for (at, zone) in [(on, None), (dipped, Some(1e-7))] {
            let (more, out) = cut(at, zone);
            assert!(!more);
            assert_eq!(out.curve_types, vec![TsectCurveType::Tangent]);
        }
    }

    /// WR-510 (#1525): a plane parallel to a cylinder's axis, turned a hair off its wall,
    /// dips in by 1e-11, far below `tol` = 1e-6, but its two crossing lines each lie
    /// `sqrt(2 R depth)` = 7.7e-6 from the foot, 1.5e-5 apart, so upstream cuts the wall
    /// in two lines. Within a face's zone tolerance (1e-5) it is one tangent line.
    #[test]
    fn issue1525_plane_within_tolerance_of_wall_is_tangent() {
        let k = Kernel::default();
        let mut c = Cone::new_cylinder_bounded(
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            3.0,
            0.0,
            360.0,
            12.0,
            false,
            false,
            false,
        );
        c.make_nurb(&k).unwrap();
        let hair = 3.0 - 1e-11;
        let bare = [None, None];
        let zone = Some(1e-5);

        // Bare surfaces keep their two crossing lines.
        let out = wall_section(&k, &c, hair, PlaneOwner::NoFace, bare);
        assert_eq!(out.curve_types, vec![TsectCurveType::Crossing; 2]);

        // A face's zone tolerance, on the plane's face or on the cone's, makes the
        // plane one tangent line on the wall.
        for (owner, zone_tols) in [
            (PlaneOwner::Face(zone), [None, zone]),
            (PlaneOwner::NoFace, [zone, None]),
        ] {
            let out = wall_section(&k, &c, hair, owner, zone_tols);
            assert_eq!(
                out.curve_types,
                vec![TsectCurveType::Tangent],
                "{owner:?} {zone_tols:?}"
            );
            let CurveKind::Line(line) = out.curves_3d[0].kind() else {
                panic!("expected an analytic line");
            };
            let p = line.evaluate_point(line.natural_interval().min());
            assert!((p.x - 3.0).abs() < 1e-9 && p.y.abs() < 1e-9, "{p:?}");
        }

        // Clearly cutting the cylinder: two crossing lines; clearly outside: nothing.
        let out = wall_section(&k, &c, 2.0, PlaneOwner::NoFace, bare);
        assert_eq!(out.curve_types, vec![TsectCurveType::Crossing; 2]);
        assert!(wall_section(&k, &c, 4.0, PlaneOwner::NoFace, bare)
            .curves_3d
            .is_empty());
    }

    /// The wall face of the radius-3 cylinder along z swept over `start`..`end` degrees.
    fn wall_swept(k: &Kernel, start: f64, end: f64) -> Cone {
        wall_of_radius(k, 3.0, start, end)
    }

    /// [`wall_swept`] for a cylinder of `radius`.
    fn wall_of_radius(k: &Kernel, radius: f64, start: f64, end: f64) -> Cone {
        let mut c = Cone::new_cylinder_bounded(
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            radius,
            start,
            end,
            12.0,
            false,
            false,
            false,
        );
        c.make_nurb(k).unwrap();
        c
    }

    /// The kinds of a wall section's curves and where each line starts, as (x, y).
    fn section_lines(out: &SurfaceIntersection) -> Vec<(TsectCurveType, f64, f64)> {
        out.curve_types
            .iter()
            .zip(&out.curves_3d)
            .map(|(kind, curve)| {
                let CurveKind::Line(line) = curve.kind() else {
                    panic!("expected an analytic line");
                };
                let p = line.evaluate_point(line.natural_interval().min());
                (*kind, p.x, p.y)
            })
            .collect()
    }

    /// #20390: a plane a hair inside the wall crosses it in two lines and stays within that
    /// hair of it between them. A face that starts between the tangent line and a crossing
    /// has its own end on the plane by the zone tolerance, and touches the plane along that
    /// end: one tangent line there, not the crossing a little further on (the edge of that
    /// end and the crossing were two lines of one contact, 3.4e-5 apart in the issue's
    /// notch, and the boolean failed between them). All in closed form on the radius-3
    /// wall: the plane `x = 3 - depth` crosses it at `±acos(1 - depth / 3)`.
    #[test]
    fn a_wall_that_starts_between_the_tangent_line_and_a_crossing_touches_along_its_start() {
        let k = Kernel::default();
        let zone = Some(1e-5);
        let faces = [
            (PlaneOwner::Face(zone), [None, zone]),
            (PlaneOwner::NoFace, [zone, None]),
        ];
        let at = |deg: f64| (3.0 * deg.to_radians().cos(), 3.0 * deg.to_radians().sin());
        let near = |got: (TsectCurveType, f64, f64), kind: TsectCurveType, (x, y): (f64, f64)| {
            got.0 == kind && (got.1 - x).abs() < 1e-12 && (got.2 - y).abs() < 1e-12
        };

        // the issue's proportions: 1.6e-10 deep (crossings at ±5.9e-4°), the face from
        // 8.6e-5°; and the wall of WR-510's test, 1e-6 deep (±0.0468°), from 0.04°
        for (depth, start) in [(1.6e-10, 8.6e-5), (1e-6, 0.04)] {
            let c = wall_swept(&k, start, 80.0);
            let d = 3.0 - depth;
            let crossing = (1.0 - depth / 3.0_f64).acos().to_degrees();
            assert!(start < crossing, "{depth}: the face starts on the lens");
            for (owner, zone_tols) in faces {
                let lines = section_lines(&wall_section(&k, &c, d, owner, zone_tols));
                assert_eq!(lines.len(), 1, "{depth} {owner:?}: {lines:?}");
                assert!(
                    near(lines[0], TsectCurveType::Tangent, at(start)),
                    "{depth} {owner:?}: {lines:?}"
                );
            }
            // bare surfaces keep the crossing
            let lines = section_lines(&wall_section(&k, &c, d, PlaneOwner::NoFace, [None; 2]));
            assert_eq!(lines.len(), 1, "{depth}: {lines:?}");
            assert!(
                near(
                    lines[0],
                    TsectCurveType::Crossing,
                    (d, (9.0 - d * d).sqrt())
                ),
                "{depth}: {lines:?}"
            );
        }

        // the mirror image: a face that ends on the lens touches along its end
        let c = wall_swept(&k, -80.0, -0.04);
        for (owner, zone_tols) in faces {
            let lines = section_lines(&wall_section(&k, &c, 3.0 - 1e-6, owner, zone_tols));
            assert_eq!(lines.len(), 1, "{owner:?}: {lines:?}");
            assert!(
                near(lines[0], TsectCurveType::Tangent, at(-0.04)),
                "{owner:?}: {lines:?}"
            );
        }

        // not changed: a face that holds the tangent line touches along it; a face that
        // starts past the crossing meets the plane nowhere; and a plane deeper in the wall
        // than the zone tolerance (1e-4: crossings at ±0.468°) keeps its crossing
        for (owner, zone_tols) in faces {
            let c = wall_swept(&k, -0.01, 80.0);
            let lines = section_lines(&wall_section(&k, &c, 3.0 - 1e-6, owner, zone_tols));
            assert_eq!(lines.len(), 1, "{owner:?}: {lines:?}");
            assert!(
                near(lines[0], TsectCurveType::Tangent, (3.0, 0.0)),
                "{owner:?}: {lines:?}"
            );

            let c = wall_swept(&k, 0.05, 80.0);
            let out = wall_section(&k, &c, 3.0 - 1e-6, owner, zone_tols);
            assert!(out.curves_3d.is_empty(), "{owner:?}");

            let c = wall_swept(&k, 0.3, 80.0);
            let d = 3.0 - 1e-4;
            let lines = section_lines(&wall_section(&k, &c, d, owner, zone_tols));
            assert_eq!(lines.len(), 1, "{owner:?}: {lines:?}");
            assert!(
                near(
                    lines[0],
                    TsectCurveType::Crossing,
                    (d, (9.0 - d * d).sqrt())
                ),
                "{owner:?}: {lines:?}"
            );
        }
    }

    /// The touch along a wall's start is taken only where it is the faces' contact; else
    /// the crossing stays, as before. On the wall of
    /// `a_wall_that_starts_between_the_tangent_line_and_a_crossing_touches_along_its_start`
    /// (from 0.04°, the plane 1e-6 deep, its crossing at 0.0468°, `y = 0.00244949`):
    /// - a requested part of the wall that starts at 0.045°, past the surface's own start
    ///   and before the crossing, does not hold that start, so it is no edge of the face;
    /// - a plane patch from `y = 0.00235` holds the crossing but not the start's line, at
    ///   `y = 3 sin 0.04° = 0.00209440`;
    /// - with a face's zone tolerance grown to 1e-4, a plane 5e-5 deep (its crossing at
    ///   0.3308°) is past the contact tolerance the rim reads, 1e-5, so the wall crosses as
    ///   the rim does; 1e-6 deep it still touches.
    #[test]
    fn the_touch_along_a_walls_start_is_only_the_faces_own_contact() {
        let k = Kernel::default();
        let zone = Some(1e-5);
        let faces = [
            (PlaneOwner::Face(zone), [None, zone]),
            (PlaneOwner::NoFace, [zone, None]),
        ];
        let c = wall_swept(&k, 0.04, 80.0);
        let d: f64 = 3.0 - 1e-6;
        let crossing = (d, (9.0 - d * d).sqrt());
        let whole = axial_domain(&k, &c, 0.0, 12.0);
        let is = |got: &[(TsectCurveType, f64, f64)], kind: TsectCurveType, (x, y): (f64, f64)| {
            got.len() == 1
                && got[0].0 == kind
                && (got[0].1 - x).abs() < 1e-12
                && (got[0].2 - y).abs() < 1e-12
        };
        let start = (
            3.0 * 0.04_f64.to_radians().cos(),
            3.0 * 0.04_f64.to_radians().sin(),
        );

        // the requested part of the wall from 0.045°
        let part = Extent2d::from_points(
            c.convert_uv_from_step_to_nurbs(&k, Point2::new(0.045, 0.0))
                .unwrap(),
            c.convert_uv_from_step_to_nurbs(&k, Point2::new(80.0, 12.0))
                .unwrap(),
        );
        for (owner, zone_tols) in faces {
            let lines = section_lines(&wall_section_within(
                &k, &c, d, owner, zone_tols, &part, -20.0,
            ));
            assert!(
                is(&lines, TsectCurveType::Crossing, crossing),
                "{owner:?}: {lines:?}"
            );

            // the plane's patch from y = 0.00235, and one from y = 0.002 that holds the start
            let lines = section_lines(&wall_section_within(
                &k, &c, d, owner, zone_tols, &whole, 0.00235,
            ));
            assert!(
                is(&lines, TsectCurveType::Crossing, crossing),
                "{owner:?}: {lines:?}"
            );
            let lines = section_lines(&wall_section_within(
                &k, &c, d, owner, zone_tols, &whole, 0.002,
            ));
            assert!(
                is(&lines, TsectCurveType::Tangent, start),
                "{owner:?}: {lines:?}"
            );
        }

        // a wall nearly all the way round, from -0.01° to 359.98°, holds the tangent line:
        // the rule's premise fails, whatever the tolerances (here a zone tolerance of 1e-7,
        // under the plane's depth, so the tangent line is not taken either). Its start lies
        // between the tangent line and the crossing at -0.0468°, but that crossing is at
        // the wall's other end; both crossings stay.
        let round = wall_swept(&k, -0.01, 359.98);
        let tight = Some(1e-7);
        let lines = section_lines(&wall_section(
            &k,
            &round,
            d,
            PlaneOwner::NoFace,
            [tight, None],
        ));
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(
            lines
                .iter()
                .all(|l| l.0 == TsectCurveType::Crossing && (l.1 - d).abs() < 1e-12),
            "{lines:?}"
        );
        assert!(
            (lines[0].2 + lines[1].2).abs() < 1e-12
                && (lines[0].2.abs() - crossing.1).abs() < 1e-12,
            "{lines:?}"
        );

        // a wall so small that its whole circle lies within the contact tolerance of the
        // plane (radius 5e-6, the plane through its axis) touches it at no single line: the
        // rim keeps its crossings there, and so does the wall, swept from 10° to 170°, at 90°
        let tiny = wall_of_radius(&k, 5e-6, 10.0, 170.0);
        for (owner, zone_tols) in faces {
            let lines = section_lines(&wall_section(&k, &tiny, 0.0, owner, zone_tols));
            assert!(
                is(&lines, TsectCurveType::Crossing, (0.0, 5e-6)),
                "{owner:?}: {lines:?}"
            );
        }

        // a zone tolerance grown past the contact tolerance
        let grown = Some(1e-4);
        let c = wall_swept(&k, 0.2, 80.0);
        for (owner, zone_tols) in [
            (PlaneOwner::Face(grown), [None, grown]),
            (PlaneOwner::NoFace, [grown, None]),
        ] {
            let d: f64 = 3.0 - 5e-5;
            let lines = section_lines(&wall_section(&k, &c, d, owner, zone_tols));
            assert!(
                is(&lines, TsectCurveType::Crossing, (d, (9.0 - d * d).sqrt())),
                "{owner:?}: {lines:?}"
            );
        }
        let c = wall_swept(&k, 0.04, 80.0);
        for (owner, zone_tols) in [
            (PlaneOwner::Face(grown), [None, grown]),
            (PlaneOwner::NoFace, [grown, None]),
        ] {
            let lines = section_lines(&wall_section(&k, &c, 3.0 - 1e-6, owner, zone_tols));
            assert!(
                is(&lines, TsectCurveType::Tangent, start),
                "{owner:?}: {lines:?}"
            );
        }
    }

    /// WR-510 (#1525): a face swept over 0.04°..0.06° only, cut by the plane `x = 3 - 1e-6`.
    /// The tangent line at 0° is outside the face; the crossing at +0.0468° is inside it,
    /// and bare surfaces keep it. (Where a face's zone tolerance holds the plane in contact
    /// with the wall, the face touches the plane along its start, at 0.04°:
    /// `a_wall_that_starts_between_the_tangent_line_and_a_crossing_touches_along_its_start`.)
    #[test]
    fn issue1525_tangent_line_outside_sweep_keeps_crossing() {
        let k = Kernel::default();
        let mut c = Cone::new_cylinder_bounded(
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            3.0,
            0.04,
            0.06,
            12.0,
            false,
            false,
            false,
        );
        c.make_nurb(&k).unwrap();
        let d = 3.0 - 1e-6;
        let out = wall_section(&k, &c, d, PlaneOwner::NoFace, [None, None]);
        assert_eq!(out.curve_types, vec![TsectCurveType::Crossing]);
        let CurveKind::Line(line) = out.curves_3d[0].kind() else {
            panic!("expected an analytic line");
        };
        let p = line.evaluate_point(line.natural_interval().min());
        let y = (3.0 * 3.0 - d * d).sqrt();
        assert!((p.x - d).abs() < 1e-9 && (p.y - y).abs() < 1e-9, "{p:?}");
    }

    #[test]
    fn general_dispatch_preserves_cone_and_all_parameters() {
        let k = Kernel::default();
        let cone = Cone::empty(false);
        let torus = Surface::from(crate::surface::torus::Torus::empty());
        let domain = Extent2d::from_bounds(1.0, 2.0, 3.0, 4.0);
        let other_domain = Extent2d::from_bounds(-1.0, -2.0, 5.0, 6.0);
        // A torus has no case in the native switch: the general intersector gets every
        // parameter unchanged.
        let error = cone.global_surface_intersect_with(&k, &domain,
            IntersectTarget::Surface(&torus), &other_domain, [false, true], Some(0.0123), Some(0.456),
            |request| {
                assert!(core::ptr::eq(request.kernel, &raw const k));
                assert!(core::ptr::eq(request.cone, &raw const cone));
                assert_eq!(*request.domain, domain);
                assert_eq!(*request.other_domain, other_domain);
                assert_eq!(request.use_surface_edges, [false, true]);
                assert_eq!(request.approx_tol_3d, Some(0.0123));
                assert_eq!(request.ang_tol_rad, Some(0.456));
                assert!(matches!(request.other, IntersectTarget::Surface(s) if core::ptr::eq(s, &raw const torus)));
                Err(SolidError::sm_err("test::general_dispatch"))
            }).unwrap_err();
        assert_eq!(error.operation(), "test::general_dispatch");
    }

    #[test]
    fn cylinder_target_reaches_intersect_with_cone() {
        let k = Kernel::default();
        let cone = Cone::empty(false);
        let cylinder = Cone::empty(true);
        let domain = Extent2d::from_bounds(1.0, 2.0, 3.0, 4.0);
        // v1.0.1 SmCone::GlobalSurfaceIntersect (SmCone.cpp:2376-2377) handles
        // SmCylinder_TYPE with SmCone_TYPE in IntersectWithCone; the empty cylinder
        // fails there, before any general fallback.
        let error = cone
            .global_surface_intersect_with(
                &k,
                &domain,
                IntersectTarget::Cone(&cylinder),
                &domain,
                [true, true],
                None,
                None,
                |_| Err(SolidError::sm_err("test::general_dispatch")),
            )
            .unwrap_err();
        assert_ne!(error.operation(), "test::general_dispatch");
    }

    #[test]
    fn identical_object_needs_no_geometry_or_general_fallback() {
        let k = Kernel::default();
        let cone = Cone::empty(false);
        let domain = Extent2d::new();
        let result = cone
            .global_surface_intersect_with(
                &k,
                &domain,
                IntersectTarget::Cone(&cone),
                &domain,
                [true, false],
                None,
                None,
                |_| panic!("same native object must finish before general fallback"),
            )
            .unwrap();
        assert!(result.curves_3d.is_empty());
    }

    #[test]
    fn boundary_dedup_validates_parallel_arrays() {
        let k = Kernel::default();
        let mut valid = SurfaceIntersection::default();
        add(
            &mut valid,
            Curve::from(BSplineCurve::create_degenerate_curve(3, Vec3::ZERO).unwrap()),
            TsectCurveType::Crossing,
        );
        for first in [false, true] {
            for field in 0..4 {
                for count in [0, 2] {
                    let mut malformed = valid.clone();
                    match field {
                        0 => malformed.surface1_uv_curves.resize(count, None),
                        1 => malformed.surface2_uv_curves.resize(count, None),
                        2 => malformed
                            .curve_types
                            .resize(count, TsectCurveType::Crossing),
                        _ => malformed.deviations.resize(count, 0.0),
                    }
                    let (mut out, second) = if first {
                        (malformed, valid.clone())
                    } else {
                        (valid.clone(), malformed)
                    };
                    let error =
                        append_boundary_intersections(&k, &mut out, second, 1e-7).unwrap_err();
                    assert!(
                        error.operation() == OP
                            && error.failed_routine_status() == Some(crate::error::SM_ERR),
                        "first={first} field={field} count={count}: {error}"
                    );
                    // No removals occurred before the contract failure.
                    assert_eq!(out.curves_3d.len(), 1);
                }
            }
        }
        // Different curve counts between bundles are valid; each bundle only
        // needs its own parallel arrays to agree.
        let mut out = valid;
        append_boundary_intersections(&k, &mut out, SurfaceIntersection::default(), 1e-7).unwrap();
        assert_eq!(out.curves_3d.len(), 1);
        assert_eq!(out.surface1_uv_curves.len(), 1);
        assert_eq!(out.surface2_uv_curves.len(), 1);
        assert_eq!(out.curve_types.len(), 1);
        assert_eq!(out.deviations.len(), 1);
    }
}
