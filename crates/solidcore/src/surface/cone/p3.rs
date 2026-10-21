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
//! The general intersection is supplied as a callback until the common Surface intersection
//! adapters retain cone virtual methods. It receives every original
//! argument. Sphere dispatch awaits the sphere type. Bounding boxes are those of
//! bare geometry (no face owner), with upstream's zone-tolerance expansion.
//! A generator solve with no solution explicitly writes EXTERIOR upstream:
//! intersections skip that result while the public inversion API still errors.
//! Start-plane transform failures propagate here; upstream initializes EXTERIOR
//! before that transform and skips the point on failure. Parallel boundary-output
//! arrays are checked before deduplication rather than assuming equal lengths.
//! No input-driven recursion is introduced.

use super::{Cone, ConeStepInversion};
use crate::curve::curve::{Curve, CurveKind};
use crate::curve::ellipse::{Ellipse, EllipseOptions};
use crate::curve::line::{BoundedLineOptions, CurveLocationType, Line};
use crate::error::{ignored, Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::extent3d::Extent3d;
use crate::geom::geom_utility::p1::{
    circle_coplanar_circle_intersect, line_closest_point, line_plane_intersect,
    parallelogram_cross_vectors, CircleCircle,
};
use crate::geom::geom_utility::p2::segment_segment_intersect;
use crate::geom::tol::{approx_tol3d, are_same_to_tol, is_zero};
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::{sqrt, Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::{max2, min2, EFF_ZERO, EFF_ZERO_SQRT, PI, UNDEF_DOUBLE};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::{BoundingBoxRequest, LocationType};
use crate::surface::plane::Plane;
use crate::surface::surf_of_extrusion::SurfOfExtrusion;
use crate::surface::surf_of_revolution::p3::Revolved;
use crate::surface::surf_of_revolution::SurfOfRevolution;
use crate::surface::surface::p2::{SurfaceIntersection, TsectCurveType, CONE_TYPE};
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
            return Err(SolidError::sm_err(OP));
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

fn boundary_intersections(
    _k: &Kernel,
    _cone: &Cone,
    _domain: &Extent2d,
    _other: &Cone,
    _other_domain: &Extent2d,
    _approx_tol: Option<f64>,
) -> Result<SurfaceIntersection> {
    // SurfaceKind retains cones; the shared boundary path still needs its
    // analytic iso/evaluation adapters. Keep this integration with its owner.
    // UNPORTED(surface.surface_p2): common cone boundary dispatch adapter
    Err(SolidError::unported(
        "SmCone common boundary dispatch adapter",
        "surface.surface_p2",
    ))
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
        .ok_or_else(|| SolidError::sm_err(OP))
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
        let mut out = SurfaceIntersection::default();
        let tol = approx_tol.map_or(EFF_ZERO_SQRT, |t| approx_tol3d(k, t));
        let box1 = bbox(k, self, domain)?;
        let mut box2 = plane
            .base_with_nurb()?
            .calculate_bounding_box(
                k,
                other_domain,
                &BoundingBoxRequest {
                    normal: true,
                    expand_by_zone_tol: true,
                    ..BoundingBoxRequest::default()
                },
            )?
            .normal
            .ok_or_else(|| SolidError::sm_err(OP))?;
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
                if distance.abs() >= bot_radius
                    || (distance.abs() > bot_radius - tol
                        && bot_radius * bot_radius - distance * distance < tol * tol)
                {
                    let dir = if distance > 0.0 { -normal } else { normal };
                    if angle_in_sweep(self, axis, dir, ang_tol)? {
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
            let curves = plane.make_new_plane_ellipse(
                k,
                other_domain,
                center,
                radius,
                radius,
                position.x_axis(),
                position.y_axis(),
                &step.u_interval(),
                tol,
            )?;
            add_plane_ellipses(k, &mut out, curves)?;
        } else {
            let cone_angle = (self.semi_angle_deg() * (PI / 180.0)).abs();
            let plane_angle = PI / 2.0 - angle.abs();
            if !self.is_cylinder() && plane_angle < cone_angle + ang_tol {
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
                let curves = plane.make_new_plane_ellipse(
                    k,
                    other_domain,
                    center,
                    major,
                    minor,
                    x,
                    y,
                    &interval,
                    tol,
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
        let mut out = SurfaceIntersection::default();
        if core::ptr::eq(self, other) {
            return Ok((false, out));
        }
        let box1 = bbox(k, self, domain)?;
        let mut box2 = bbox(k, other, other_domain)?;
        let size = box2.size();
        let tol = approx_tol.map_or(EFF_ZERO_SQRT * (1.0 + size.length()), |t| {
            approx_tol3d(k, t)
        });
        box2.expand_absolute(tol);
        if box1.are_disjoint(&box2, 0.0) {
            return Ok((false, out));
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
                return Ok((false, out));
            }
            let param2 = line_closest_point(bottom, axis, other_top);
            if (param > height + tol && param2 > height + tol)
                || (param < 0.0 - tol && param2 < 0.0 - tol)
            {
                return Ok((false, out));
            }
            if !self.is_cylinder() || !other.is_cylinder() {
                return Ok((true, out));
            }
            let mut min = 0.0;
            let mut max = height;
            let other_min = min2(param, param2);
            let other_max = max2(param, param2);
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
                if let Some(first) = ignored(boundary_intersections(
                    k,
                    self,
                    domain,
                    other,
                    other_domain,
                    approx_tol,
                ))? {
                    append(&mut out, first);
                }
                let second =
                    boundary_intersections(k, other, other_domain, self, domain, approx_tol);
                // Upstream overwrites the first status/needs-more with the second.
                let second = second?;
                append_boundary_intersections(k, &mut out, second, tol)?;
                return Ok((false, out));
            }
            let start = bottom + param * axis;
            let intersection = circle_coplanar_circle_intersect(
                axis,
                start,
                self.bot_radius(),
                other_bottom,
                other.bot_radius(),
                tol,
            )?;
            let count = intersection.count();
            let points = match intersection {
                CircleCircle::None => Vec::new(),
                CircleCircle::Tangent(p) => vec![p],
                CircleCircle::Crossing(p, q) => vec![p, q],
                CircleCircle::Coincident => return Ok((true, out)),
            };
            for point in points {
                let line_point = point + (min - param) * axis;
                let zero = EFF_ZERO * (1.0 + line_point.max_dimension());
                let inv = intersection_inversion(self, k, line_point, zero)?;
                let other_inv = intersection_inversion(other, k, line_point, zero)?;
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
                        &mut out,
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
                        &mut out,
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
                return Ok((true, out));
            }
            let points = segment_segment_intersect(bottom, top, other_bottom, other_top, tol)?;
            let [point] = points.as_slice() else {
                return Ok((true, out));
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
        Ok((false, out))
    }
}

// The native intersection ignores STEPInversion's status, then skips an
// EXTERIOR classification. The one local error below is exactly the generator
// solve's empty-result branch: it explicitly writes EXTERIOR before returning
// SM_ERR. No UV coordinate is fabricated or consumed on that branch.
// Start-plane transform errors still propagate (see the module Deviations).
fn intersection_inversion(
    cone: &Cone,
    k: &Kernel,
    point: Point3,
    tol: f64,
) -> Result<Option<ConeStepInversion>> {
    match cone.step_inversion(k, point, tol, None) {
        Ok(inversion) => Ok(Some(inversion)),
        Err(SolidError::Upstream {
            operation: "surface::surf_of_revolution::p2::SurfOfRevolution::step_inversion",
            code: crate::error::SM_ERR,
            ..
        }) => Ok(None),
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
        Err(e) if e.is_unported() => Err(e),
        Err(e) => {
            let circle = cone.rev().polar_curve.as_ref().ok_or(e)?;
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
        return Err(SolidError::sm_err(OP));
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
        let target = match other {
            IntersectTarget::Surface(s) => match s.kind() {
                SurfaceKind::Plane(p) => IntersectTarget::Plane(p),
                SurfaceKind::SurfOfRevolution(r) => IntersectTarget::Revolution(r),
                SurfaceKind::Cone(c) => IntersectTarget::Cone(c.as_ref()),
                _ => other,
            },
            _ => other,
        };
        let (needs_more, out) = match target {
            IntersectTarget::Plane(p) => {
                self.intersect_with_plane(k, domain, p, other_domain, approx_tol_3d)?
            }
            IntersectTarget::Cone(c) if c.type_code() == CONE_TYPE => {
                self.intersect_with_cone(k, domain, c, other_domain, approx_tol_3d)?
            }
            IntersectTarget::Revolution(r) => {
                let (more, mut out) =
                    r.intersect_with_cone(k, other_domain, self, domain, approx_tol_3d)?;
                core::mem::swap(&mut out.surface1_uv_curves, &mut out.surface2_uv_curves);
                (more, out)
            }
            IntersectTarget::Extrusion(e) if self.is_cylinder() => {
                let r = e.intersect_with_cylinder(k, self, approx_tol_3d)?;
                (
                    r.needs_more_intersections,
                    SurfaceIntersection {
                        curves_3d: r.curves_3d,
                        surface1_uv_curves: r.surface2_uv_curves,
                        surface2_uv_curves: r.surface1_uv_curves,
                        curve_types: r.curve_types,
                        deviations: r.deviations,
                    },
                )
            }
            _ => (true, SurfaceIntersection::default()),
        };
        if needs_more {
            // DoIntersection resets every native output before its general solve.
            return general(GeneralIntersectionRequest {
                kernel: k,
                cone: self,
                domain,
                other,
                other_domain,
                use_surface_edges,
                approx_tol_3d,
                ang_tol_rad,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn general_dispatch_preserves_cone_and_all_parameters() {
        let k = Kernel::default();
        let cone = Cone::empty(false);
        let cylinder = Cone::empty(true);
        let domain = Extent2d::from_bounds(1.0, 2.0, 3.0, 4.0);
        let other_domain = Extent2d::from_bounds(-1.0, -2.0, 5.0, 6.0);
        // SmCylinder_TYPE deliberately does not match SmCone_TYPE in the native
        // switch, even though its object derives from SmCone.
        let error = cone.global_surface_intersect_with(&k, &domain,
            IntersectTarget::Cone(&cylinder), &other_domain, [false, true], Some(0.0123), Some(0.456),
            |request| {
                assert!(core::ptr::eq(request.kernel, &raw const k));
                assert!(core::ptr::eq(request.cone, &raw const cone));
                assert_eq!(*request.domain, domain);
                assert_eq!(*request.other_domain, other_domain);
                assert_eq!(request.use_surface_edges, [false, true]);
                assert_eq!(request.approx_tol_3d, Some(0.0123));
                assert_eq!(request.ang_tol_rad, Some(0.456));
                assert!(matches!(request.other, IntersectTarget::Cone(c) if core::ptr::eq(c, &raw const cylinder)));
                Err(SolidError::sm_err("test::general_dispatch"))
            }).unwrap_err();
        assert_eq!(error.operation(), "test::general_dispatch");
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
                        matches!(
                            error,
                            SolidError::Upstream {
                                operation: OP,
                                code: crate::error::SM_ERR,
                                ..
                            }
                        ),
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
