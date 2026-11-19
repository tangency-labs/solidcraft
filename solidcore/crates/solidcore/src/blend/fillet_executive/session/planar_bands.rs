//! Contact of constant-width chamfer bands on a planar support.
//!
//! Upstream: not in SMLib. One construction covers equal and unequal setbacks:
//! the bounded original edges' closest gap, with opposing inward normal witnesses.
//! The triangle inequality excludes an earlier contact, and the normal witnesses
//! attain that bound. Parallel lines retain their exact minimizing interval; other
//! curve minima use the existing global curve solver, including its endpoint solves.
//! This module introduces no station sampling or retry builds.
//!
//! Deviations: endpoint witnesses are retained only at unblended planar caps that
//! contain the inward normal ray. Selected endpoint corners and other cap classes
//! do not establish a constant kept domain and therefore name no size here. Interior
//! witnesses must clear every incident selected planar straight-edge chamfer strip;
//! unsupported moving corners name no size. All clearance products use expansions.

use super::SolverObject;
use crate::blend::fillet_executive::FitKind;
use crate::curve::curve::p9::CurveSolveOptions;
use crate::curve::curve::{Curve, CurveKind};
use crate::error::{Result, SolidError};
use crate::geom::expansion as exact;
use crate::geom::extent1d::Extent1d;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, EFF_ZERO};
use crate::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use crate::nurbs::knots::Side;
use crate::nurbs::math::{CPoint, NOW, NOZ};
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::edgeuse::p1 as eu;
use crate::topology::{edge, Brep, EdgeId, FaceId, VertexId};

const OP: &str = "blend::fillet_executive::session::planar_bands";

pub(super) struct BandContact {
    pub size: f64,
    pub kind: FitKind,
    pub edge: EdgeId,
    pub face: FaceId,
    pub at: Point3,
}

/// A selected neighbor can trim an interior witness at either endpoint. Only a
/// constant planar chamfer has the straight-strip certificate used here.
pub(super) struct CornerBand {
    edge: EdgeId,
    ends: [Option<VertexId>; 2],
    line: Option<(Point3, Vec3, f64)>,
}

struct ContactWitness {
    feet: [Point3; 2],
    widths: [f64; 2],
}

struct Band {
    edge: EdgeId,
    face: FaceId,
    curve: Curve,
    interval: Extent1d,
    plane_normal: Vec3,
    other: Surface,
    inward_sign: f64,
    width: f64,
    offsets: [f64; 2],
}

fn missing(what: &'static str) -> SolidError {
    SolidError::blend_missing(OP, what)
}

fn parallel_error_is_zero(error: f64) -> bool {
    error.is_finite() && error <= EFF_ZERO
}

/// Recognize the extrusion in its stored homogeneous net, including after a record
/// round trip. Equal paired weights and one translation for every pair prove the
/// surface is a constant sweep; no surface fitting or station tests are involved.
fn net_extrusion_axis(surface: &BSplineSurface) -> Result<Option<Vec3>> {
    let (u_count, v_count, points) = surface.control_points()?;
    let sweep_u = surface.degree(SurfParamType::U) == 1 && u_count == 2;
    let sweep_v = surface.degree(SurfParamType::V) == 1 && v_count == 2;
    if !sweep_u && !sweep_v {
        return Ok(None);
    }
    let count = if sweep_u { v_count } else { u_count };
    let mut reference: Option<([Vec<f64>; 3], f64)> = None;
    for i in 0..count {
        let (start, end) = if sweep_u {
            (Some(i), i.checked_add(v_count))
        } else {
            (
                i.checked_mul(2),
                i.checked_mul(2).and_then(|j| j.checked_add(1)),
            )
        };
        let point = |index: Option<usize>| -> Result<CPoint> {
            index
                .and_then(|index| points.get(index))
                .copied()
                .ok_or_else(|| missing("the extrusion control point"))
        };
        let (a, b) = (point(start)?, point(end)?);
        let weight = if a.w == NOW { 1.0 } else { a.w };
        if a.w != b.w || !weight.is_finite() || weight <= 0.0 || a.z == NOZ || b.z == NOZ {
            return Ok(None);
        }
        if ![a.x, a.y, a.z, b.x, b.y, b.z].iter().all(|x| x.is_finite()) {
            return Err(SolidError::invalid_input(
                OP,
                "non-finite extrusion control point",
            ));
        }
        let delta = [
            exact::sub(&[b.x], &[a.x]),
            exact::sub(&[b.y], &[a.y]),
            exact::sub(&[b.z], &[a.z]),
        ];
        if let Some((first, first_weight)) = &reference {
            for (d, r) in delta.iter().zip(first) {
                let left = exact::scale(d, *first_weight)
                    .ok_or_else(|| missing("the exact extrusion translation product"))?;
                let right = exact::scale(r, weight)
                    .ok_or_else(|| missing("the exact extrusion translation product"))?;
                if !exact::is_zero(&exact::sub(&left, &right)) {
                    return Ok(None);
                }
            }
        } else {
            reference = Some((delta, weight));
        }
    }
    let Some(([x, y, z], weight)) = reference else {
        return Ok(None);
    };
    let axis = Vec3::new(
        exact::to_f64(&x) / weight,
        exact::to_f64(&y) / weight,
        exact::to_f64(&z) / weight,
    );
    Ok(Some(axis.unitize()?))
}

fn band(target: &Brep, solver: &SolverObject, rail: usize) -> Result<Option<Band>> {
    let base = solver.base();
    let Some(chamfer) = base.chamfer.as_ref() else {
        return Ok(None);
    };
    let Some(offsets) = solver.constant_offsets()? else {
        return Ok(None);
    };
    let other_rail = match rail {
        0 => 1,
        1 => 0,
        _ => return Err(SolidError::invalid_input(OP, "rail index out of range")),
    };
    let use_ = base.edgeuse(rail)?.ok_or_else(|| missing("the edge use"))?;
    let face = eu::face(target, use_)?.ok_or_else(|| missing("the shared face"))?;
    let own = base
        .surface(rail)?
        .ok_or_else(|| missing("the planar support"))?;
    let SurfaceKind::Plane(plane) = own.surface.base_surface().kind() else {
        return Ok(None);
    };
    let plane_normal = plane.evaluate_normal();
    let other = base
        .surface(other_rail)?
        .ok_or_else(|| missing("the other support"))?
        .surface
        .base_surface();
    match other.kind() {
        SurfaceKind::Plane(_) => {}
        SurfaceKind::SurfOfExtrusion(extrusion) => {
            let axis = extrusion.extrusion_vector().unitize()?;
            if !parallel_error_is_zero(axis.cross(plane_normal).length()) {
                return Ok(None);
            }
        }
        SurfaceKind::BSpline(surface) => {
            let Some(axis) = net_extrusion_axis(surface)? else {
                return Ok(None);
            };
            if !parallel_error_is_zero(axis.cross(plane_normal).length()) {
                return Ok(None);
            }
        }
        _ => return Ok(None),
    }
    let width = chamfer
        .setbacks()
        .get(rail)
        .copied()
        .ok_or_else(|| missing("the chamfer setback"))?;
    let inward_offset = offsets
        .get(other_rail)
        .copied()
        .ok_or_else(|| missing("the other offset"))?;
    if !(width.is_finite() && width > 0.0 && inward_offset.is_finite() && inward_offset != 0.0) {
        return Err(SolidError::invalid_input(OP, "invalid constant band width"));
    }
    let edge = eu::edge(target, use_)?.ok_or_else(|| missing("the blended edge"))?;
    let curve =
        edge::evaluation_curve(target, edge)?.ok_or_else(|| missing("the blended edge's curve"))?;
    Ok(Some(Band {
        edge,
        face,
        curve,
        interval: edge::interval(target, edge)?,
        plane_normal,
        other: other.clone(),
        inward_sign: inward_offset.signum(),
        width,
        offsets,
    }))
}

/// Classify the support definitions before choosing the contact construction.
pub(super) fn supports(
    target: &Brep,
    a: &SolverObject,
    b: &SolverObject,
    rail_a: usize,
    rail_b: usize,
) -> Result<bool> {
    let (Some(a), Some(b)) = (band(target, a, rail_a)?, band(target, b, rail_b)?) else {
        return Ok(false);
    };
    Ok(a.face == b.face)
}

pub(super) fn corner_band(
    target: &Brep,
    solver: &SolverObject,
    shared: FaceId,
) -> Result<Option<CornerBand>> {
    let Some(use_) = solver.base().edgeuse(0)? else {
        return Ok(None);
    };
    let edge = eu::edge(target, use_)?.ok_or_else(|| missing("the selected edge"))?;
    let (start, end) = edge::start_end_vertices(target, edge)?;
    let mut line = None;
    for rail in 0..2 {
        let Some(use_) = solver.base().edgeuse(rail)? else {
            continue;
        };
        if eu::face(target, use_)? != Some(shared) {
            continue;
        }
        if let Some(band) = band(target, solver, rail)? {
            if matches!(band.other.kind(), SurfaceKind::Plane(_)) {
                if let CurveKind::Line(curve) = band.curve.kind() {
                    line = Some((curve.line_point(), curve.line_vector(), band.width));
                }
            }
        }
    }
    Ok(Some(CornerBand {
        edge,
        ends: [start, end],
        line,
    }))
}

fn exact_range() -> SolidError {
    SolidError::invalid_input(OP, "corner clearance outside exact arithmetic range")
}

fn expansion_product(a: &[f64], b: &[f64]) -> Result<Vec<f64>> {
    let mut product = Vec::new();
    for &factor in b {
        product = exact::add(&product, &exact::scale(a, factor).ok_or_else(exact_range)?);
    }
    if !product.iter().all(|x| x.is_finite()) {
        return Err(exact_range());
    }
    Ok(product)
}

/// Infinite-line distance bounds bounded-edge distance from below. The contact is
/// (wb P + wa Q)/(wa+wb); eliminate that denominator against the common scale's
/// denominator. Neither the contact point, scale, nor gap is rounded for this test.
fn outside_corner_strip(witness: &ContactWitness, line: (Point3, Vec3, f64)) -> Result<bool> {
    let (origin, tangent, width) = line;
    let [p, q] = witness.feet;
    let [wa, wb] = witness.widths;
    let p_cross = exact::cross(p, tangent).ok_or_else(exact_range)?;
    let q_cross = exact::cross(q, tangent).ok_or_else(exact_range)?;
    let origin_cross = exact::cross(origin, tangent).ok_or_else(exact_range)?;
    let mut distance = Vec::new();
    for ((a, b), c) in p_cross.iter().zip(&q_cross).zip(&origin_cross) {
        let from_p = exact::scale(&exact::sub(a, c), wb).ok_or_else(exact_range)?;
        let from_q = exact::scale(&exact::sub(b, c), wa).ok_or_else(exact_range)?;
        let component = exact::add(&from_p, &from_q);
        distance = exact::add(&distance, &expansion_product(&component, &component)?);
    }
    let mut gap_squared = Vec::new();
    for (a, b) in [(p.x, q.x), (p.y, q.y), (p.z, q.z)] {
        let delta = exact::sub(&[b], &[a]);
        gap_squared = exact::add(&gap_squared, &expansion_product(&delta, &delta)?);
    }
    let width_squared = exact::product(width, width).ok_or_else(exact_range)?;
    let tangent_squared = exact::dot(tangent, tangent).ok_or_else(exact_range)?;
    let bound = expansion_product(
        &expansion_product(&gap_squared, &width_squared)?,
        &tangent_squared,
    )?;
    let difference = exact::sub(&distance, &bound);
    if !difference.iter().all(|x| x.is_finite()) {
        return Err(exact_range());
    }
    Ok(exact::sign(&difference) > 0)
}

fn inward(k: &Kernel, band: &Band, foot: Point3) -> Result<Vec3> {
    let dropped = band.other.drop_point::<NoObject>(
        k,
        foot,
        &band.other.natural_uv_domain_with_nurb(k)?,
        &DropPointOptions::default(),
    )?;
    if !dropped.success {
        return Err(missing("the foot on the other support"));
    }
    let normal = band
        .other
        .evaluate_normal(k, dropped.result_uv, Side::Left, Side::Left)?
        * band.inward_sign;
    (normal - band.plane_normal * normal.dot(band.plane_normal)).unitize()
}

fn endpoint_cap(
    k: &Kernel,
    target: &Brep,
    band: &Band,
    vertex: VertexId,
    normal: Vec3,
    blended: &[EdgeId],
) -> Result<bool> {
    // A selected adjacent edge places a moving corner, rather than this fixed cap.
    for &selected in blended {
        if selected != band.edge
            && (edge::start_vertex(target, selected)? == Some(vertex)
                || edge::end_vertex(target, selected)? == Some(vertex))
        {
            return Ok(false);
        }
    }
    for candidate in target.edges().ids() {
        if candidate == band.edge
            || (edge::start_vertex(target, candidate)? != Some(vertex)
                && edge::end_vertex(target, candidate)? != Some(vertex))
        {
            continue;
        }
        let faces = edge::faces(target, candidate, None)?;
        if !faces.contains(&band.face) {
            continue;
        }
        let Some(curve) = edge::evaluation_curve(target, candidate)? else {
            continue;
        };
        if !matches!(curve.kind(), CurveKind::Line(_)) {
            continue;
        }
        let interval = edge::interval(target, candidate)?;
        let direction = (curve.evaluate_point(k, interval.max())?
            - curve.evaluate_point(k, interval.min())?)
        .unitize()?;
        if !parallel_error_is_zero(direction.cross(normal).length()) {
            continue;
        }
        for face in faces {
            if face == band.face {
                continue;
            }
            let Some(surface) = crate::topology::face::p2::surface(target, face)? else {
                continue;
            };
            if matches!(surface.kind(), SurfaceKind::Plane(plane)
                if plane.evaluate_normal().dot(normal).abs() <= EFF_ZERO)
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn kept_witness(
    k: &Kernel,
    target: &Brep,
    band: &Band,
    foot: Point3,
    normal: Vec3,
    blended: &[EdgeId],
    corners: &[CornerBand],
    witness: &ContactWitness,
) -> Result<bool> {
    let tolerance = edge::tolerance(target, band.edge)?;
    for (parameter, vertex) in [
        (band.interval.min(), edge::start_vertex(target, band.edge)?),
        (band.interval.max(), edge::end_vertex(target, band.edge)?),
    ] {
        if foot.distance(band.curve.evaluate_point(k, parameter)?) <= tolerance {
            let vertex = vertex.ok_or_else(|| missing("the blended edge's endpoint"))?;
            return endpoint_cap(k, target, band, vertex, normal, blended);
        }
    }
    let ends = [
        edge::start_vertex(target, band.edge)?,
        edge::end_vertex(target, band.edge)?,
    ];
    for neighbor in corners {
        if neighbor.edge == band.edge
            || !ends
                .iter()
                .flatten()
                .any(|v| neighbor.ends.contains(&Some(*v)))
        {
            continue;
        }
        let Some(line) = neighbor.line else {
            return Ok(false);
        };
        if !outside_corner_strip(witness, line)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn closest_feet(k: &Kernel, a: &Band, b: &Band, tolerance: f64) -> Result<Option<[Point3; 2]>> {
    if let (CurveKind::Line(line_a), CurveKind::Line(line_b)) = (a.curve.kind(), b.curve.kind()) {
        let cross = exact::cross(line_a.line_vector(), line_b.line_vector())
            .ok_or_else(|| SolidError::invalid_input(OP, "line directions outside exact range"))?;
        let a0 = a.curve.evaluate_point(k, a.interval.min())?;
        let a1 = a.curve.evaluate_point(k, a.interval.max())?;
        let b0 = b.curve.evaluate_point(k, b.interval.min())?;
        let b1 = b.curve.evaluate_point(k, b.interval.max())?;
        let tangent = (a1 - a0).unitize()?;
        if cross.iter().all(|component| exact::is_zero(component)) {
            // The minimizers form an interval. Its midpoint is a candidate; the
            // retained-witness certificate below must also exclude corner strips.
            let b_start = (b0 - a0).dot(tangent);
            let b_end = (b1 - a0).dot(tangent);
            let a_span = (a1 - a0).dot(tangent);
            let b_span = b_end - b_start;
            if !(a_span.is_finite() && a_span > 0.0 && b_span.is_finite() && b_span != 0.0) {
                return Err(SolidError::invalid_input(
                    OP,
                    "invalid projected line spans",
                ));
            }
            let lo = 0.0_f64.max(b_start.min(b_end));
            let hi = a_span.min(b_start.max(b_end));
            if lo > hi {
                // Disjoint projections cannot have opposing normal witnesses.
                return Ok(None);
            }
            let along = lo + (hi - lo) * 0.5;
            return Ok(Some([
                a.curve
                    .evaluate_point(k, a.interval.evaluate(along / a_span))?,
                b.curve
                    .evaluate_point(k, b.interval.evaluate((along - b_start) / b_span))?,
            ]));
        }
    }
    let solutions = a.curve.global_curve_solve::<NoObject>(
        k,
        &a.interval,
        &b.curve,
        &b.interval,
        SolverOperation::Minimize,
        tolerance,
        &CurveSolveOptions::default(),
        SolutionRequested::Single,
    )?;
    let solution = solutions
        .get(0)
        .ok_or_else(|| missing("the closest band feet"))?;
    Ok(Some([
        a.curve.evaluate_point(k, solution.start.param(0)?)?,
        b.curve.evaluate_point(k, solution.start.param(1)?)?,
    ]))
}

/// Name the normal contact of two constant-width planar bands, including fixed caps.
pub(super) fn contact(
    k: &Kernel,
    target: &mut Brep,
    a: &SolverObject,
    b: &SolverObject,
    rails: [usize; 2],
    blended: &[EdgeId],
    corners: &[CornerBand],
    as_scale: bool,
) -> Result<Option<BandContact>> {
    let [rail_a, rail_b] = rails;
    let (Some(a), Some(b)) = (band(target, a, rail_a)?, band(target, b, rail_b)?) else {
        return Ok(None);
    };
    if a.face != b.face {
        return Ok(None);
    }
    let tolerance = edge::tolerance(target, a.edge)?.max(edge::tolerance(target, b.edge)?);
    let Some([foot_a, foot_b]) = closest_feet(k, &a, &b, tolerance)? else {
        return Ok(None);
    };
    let between = foot_b - foot_a;
    let gap = between.length();
    if !(gap.is_finite() && gap > 0.0) {
        return Ok(None);
    }
    let normal_a = inward(k, &a, foot_a)?;
    let normal_b = inward(k, &b, foot_b)?;
    let direction = between / gap;
    let opposing = direction.dot(normal_a) > 0.0
        && direction.dot(normal_b) < 0.0
        && parallel_error_is_zero(direction.cross(normal_a).length())
        && parallel_error_is_zero(direction.cross(normal_b).length());
    if !opposing {
        return Ok(None);
    }
    let widths = a.width + b.width;
    if !(widths.is_finite() && widths > 0.0) {
        return Err(SolidError::invalid_input(OP, "non-finite total band width"));
    }
    let scale = gap / widths;
    if !(scale.is_finite() && scale > 0.0) {
        return Err(SolidError::invalid_input(
            OP,
            "invalid planar contact scale",
        ));
    }
    let contact_at = foot_a + between * (a.width / widths);
    let witness = ContactWitness {
        feet: [foot_a, foot_b],
        widths: [a.width, b.width],
    };
    if !kept_witness(k, target, &a, foot_a, normal_a, blended, corners, &witness)?
        || !kept_witness(k, target, &b, foot_b, normal_b, blended, corners, &witness)?
    {
        return Ok(None);
    }
    if super::sample_off_face(k, target, a.face, contact_at)? {
        return Ok(None);
    }
    let rho = a
        .offsets
        .first()
        .copied()
        .ok_or_else(|| missing("the first offset"))?
        .abs();
    let equal = !as_scale
        && a.offsets
            .iter()
            .chain(b.offsets.iter())
            .all(|offset| (offset.abs() - rho).abs() <= EFF_ZERO * (1.0 + rho));
    let (size, kind) = if equal {
        (rho * gap / widths, FitKind::Radius)
    } else {
        (
            ((gap - 1000.0 * tolerance) / widths).max(0.0) * (1.0 - crate::nurbs::math::gamma(7)),
            FitKind::Scale,
        )
    };
    if !size.is_finite() {
        return Err(SolidError::invalid_input(
            OP,
            "non-finite planar band contact",
        ));
    }
    Ok(Some(BandContact {
        size,
        kind,
        edge: a.edge,
        face: a.face,
        at: crate::topology::face::p2::calculate_an_internal_point(k, target, a.face)?,
    }))
}
