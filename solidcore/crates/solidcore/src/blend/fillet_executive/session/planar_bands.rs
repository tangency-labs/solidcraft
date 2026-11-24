//! Contact of constant-width chamfer bands on a planar support.
//!
//! Upstream: not in SMLib. One construction covers equal and unequal setbacks:
//! the bounded original edges' closest gap, with opposing inward normal witnesses.
//! The triangle inequality excludes an earlier contact, and the normal witnesses
//! attain that bound. Parallel lines retain their exact minimizing interval; other
//! curve minima use the existing global curve solver, including its endpoint solves.
//! Parallel families subtract exact closed corner-strip intervals before choosing a
//! retained contact, preserving the midpoint whenever it clears those intervals.
//! This module introduces no station sampling or retry builds.
//!
//! Deviations: endpoint witnesses are retained only at unblended planar caps that
//! contain the inward normal ray. Selected endpoint corners and other cap classes
//! do not establish a constant kept domain and therefore name no size here. Interior
//! witnesses must clear every incident selected planar straight-edge chamfer strip;
//! unsupported moving corners name no size. All clearance products use expansions.
//! The width event itself is compared exactly but read through the rounding of the feet's
//! own coordinates (`gamma(32)` of their size): bands that meet to that rounding do not
//! overlap, and a gap proved under the widths by more than it does.

mod family;
mod interval;
mod opposing;

use super::SolverObject;
use crate::blend::fillet_executive::FitKind;
use crate::curve::curve::p9::CurveSolveOptions;
use crate::curve::curve::{Curve, CurveKind};
use crate::error::blend::BandInput;
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
    pub overlapping: bool,
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
    other_face: Option<FaceId>,
    curve: Curve,
    interval: Extent1d,
    plane_normal: Vec3,
    other: Surface,
    inward_sign: f64,
    width: f64,
    offsets: [f64; 2],
}

fn missing(input: BandInput) -> SolidError {
    SolidError::blend_band_input_missing(OP, input)
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
                .ok_or_else(|| missing(BandInput::ExtrusionControlPoint))
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
                    .ok_or_else(|| missing(BandInput::ExtrusionTranslationProduct))?;
                let right = exact::scale(r, weight)
                    .ok_or_else(|| missing(BandInput::ExtrusionTranslationProduct))?;
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
    let use_ = base
        .edgeuse(rail)?
        .ok_or_else(|| missing(BandInput::EdgeUse))?;
    let face = eu::face(target, use_)?.ok_or_else(|| missing(BandInput::SharedFace))?;
    let own = base
        .surface(rail)?
        .ok_or_else(|| missing(BandInput::PlanarSupport))?;
    let SurfaceKind::Plane(plane) = own.surface.base_surface().kind() else {
        return Ok(None);
    };
    let plane_normal = plane.evaluate_normal();
    let other = base
        .surface(other_rail)?
        .ok_or_else(|| missing(BandInput::OtherSupport))?
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
        .ok_or_else(|| missing(BandInput::ChamferSetback))?;
    let inward_offset = offsets
        .get(other_rail)
        .copied()
        .ok_or_else(|| missing(BandInput::OtherOffset))?;
    if !(width.is_finite() && width > 0.0 && inward_offset.is_finite() && inward_offset != 0.0) {
        return Err(SolidError::invalid_input(OP, "invalid constant band width"));
    }
    let other_face = match base.edgeuse(other_rail)? {
        Some(other_use) => eu::face(target, other_use)?,
        None => None,
    };
    let edge = eu::edge(target, use_)?.ok_or_else(|| missing(BandInput::BlendedEdge))?;
    let curve = edge::evaluation_curve(target, edge)?
        .ok_or_else(|| missing(BandInput::BlendedEdgeCurve))?;
    Ok(Some(Band {
        edge,
        face,
        other_face,
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

pub(super) use opposing::PreparedBand;

/// Prepare one straight planar chamfer band on `face`, if `solver` has one.
/// Cached once per `(solver, face)` before the pair loop so candidate pruning
/// performs no surface drops or topology queries per pair.
pub(super) fn prepare_straight_band(
    k: &Kernel,
    target: &Brep,
    solver: &SolverObject,
    face: FaceId,
    blended: &[EdgeId],
) -> Result<Option<PreparedBand>> {
    for rail in 0..2 {
        let Some(use_) = solver.base().edgeuse(rail)? else {
            continue;
        };
        if eu::face(target, use_)? != Some(face) {
            continue;
        }
        let Some(b) = band(target, solver, rail)? else {
            return Ok(None);
        };
        return opposing::prepare(k, target, &b, blended);
    }
    Ok(None)
}

/// Select prepared straight-line pairs whose bands can overlap at the requested
/// widths: either exact parallel lines whose infinite-line separation is under
/// the total width, or non-parallel opposing straight lines whose bisector
/// reaches a scale under 1. This necessary condition runs in O(1) without
/// surface drops or corner reconstruction.
pub(super) fn overlap_candidate(a: &PreparedBand, b: &PreparedBand) -> Result<bool> {
    if a.face != b.face {
        return Ok(false);
    }
    let tangent = a.line_vector;
    let parallel = exact::cross(tangent, b.line_vector)
        .ok_or_else(|| SolidError::invalid_input(OP, "line directions outside exact range"))?;
    if !parallel.iter().all(|component| exact::is_zero(component)) {
        return Ok(opposing::overlap_candidate_prepared(a, b));
    }
    let (pa, pb) = (a.line_point, b.line_point);
    let [dx, dy, dz] = [
        interval::sub(&[pb.x], &[pa.x])?,
        interval::sub(&[pb.y], &[pa.y])?,
        interval::sub(&[pb.z], &[pa.z])?,
    ];
    let cross = [
        interval::sub(
            &interval::scale(&dy, tangent.z)?,
            &interval::scale(&dz, tangent.y)?,
        )?,
        interval::sub(
            &interval::scale(&dz, tangent.x)?,
            &interval::scale(&dx, tangent.z)?,
        )?,
        interval::sub(
            &interval::scale(&dx, tangent.y)?,
            &interval::scale(&dy, tangent.x)?,
        )?,
    ];
    let mut distance_numerator = Vec::new();
    for component in cross {
        distance_numerator =
            interval::add(&distance_numerator, &interval::mul(&component, &component)?)?;
    }
    let widths = interval::add(&[a.width], &[b.width])?;
    let tangent_squared = exact::dot(tangent, tangent)
        .ok_or_else(|| SolidError::invalid_input(OP, "line direction outside exact range"))?;
    let bound = interval::mul(&interval::mul(&widths, &widths)?, &tangent_squared)?;
    Ok(exact::sign(&interval::sub(&distance_numerator, &bound)?) < 0)
}

pub(super) fn corner_band(
    target: &Brep,
    solver: &SolverObject,
    shared: FaceId,
) -> Result<Option<CornerBand>> {
    let Some(use_) = solver.base().edgeuse(0)? else {
        return Ok(None);
    };
    let edge = eu::edge(target, use_)?.ok_or_else(|| missing(BandInput::SelectedEdge))?;
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

fn outside_corner_strip(witness: &ContactWitness, line: (Point3, Vec3, f64)) -> Result<bool> {
    family::outside_corner_strip(witness, line)
}

fn inward(k: &Kernel, band: &Band, foot: Point3) -> Result<Vec3> {
    let dropped = band.other.drop_point::<NoObject>(
        k,
        foot,
        &band.other.natural_uv_domain_with_nurb(k)?,
        &DropPointOptions::default(),
    )?;
    if !dropped.success {
        return Err(missing(BandInput::FootOnOtherSupport));
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
            let vertex = vertex.ok_or_else(|| missing(BandInput::BlendedEdgeEndpoint))?;
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

fn normal_contact(
    k: &Kernel,
    a: &Band,
    b: &Band,
    feet: [Point3; 2],
) -> Result<Option<(f64, Vec3, Vec3)>> {
    let [foot_a, foot_b] = feet;
    let between = foot_b - foot_a;
    let gap = between.length();
    if !(gap.is_finite() && gap > 0.0) {
        return Ok(None);
    }
    let normal_a = inward(k, a, foot_a)?;
    let normal_b = inward(k, b, foot_b)?;
    let direction = between / gap;
    let opposing = direction.dot(normal_a) > 0.0
        && direction.dot(normal_b) < 0.0
        && parallel_error_is_zero(direction.cross(normal_a).length())
        && parallel_error_is_zero(direction.cross(normal_b).length());
    if !opposing {
        return Ok(None);
    }
    Ok(Some((gap, normal_a, normal_b)))
}

struct ClosestFeet {
    points: [Point3; 2],
    parallel_family: bool,
}

fn closest_feet(
    k: &Kernel,
    target: &Brep,
    a: &Band,
    b: &Band,
    tolerance: f64,
    blended: &[EdgeId],
    corners: &[CornerBand],
) -> Result<Option<ClosestFeet>> {
    if let (CurveKind::Line(line_a), CurveKind::Line(line_b)) = (a.curve.kind(), b.curve.kind()) {
        let cross = exact::cross(line_a.line_vector(), line_b.line_vector())
            .ok_or_else(|| SolidError::invalid_input(OP, "line directions outside exact range"))?;
        if cross.iter().all(|component| exact::is_zero(component)) {
            return Ok(
                family::feet(k, target, a, b, [line_a, line_b], blended, corners)?.map(|points| {
                    ClosestFeet {
                        points,
                        parallel_family: true,
                    }
                }),
            );
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
        .ok_or_else(|| missing(BandInput::ClosestBandFeet))?;
    Ok(Some(ClosestFeet {
        points: [
            a.curve.evaluate_point(k, solution.start.param(0)?)?,
            b.curve.evaluate_point(k, solution.start.param(1)?)?,
        ],
        parallel_family: false,
    }))
}

/// Whether a gap (its exact square, `gap_squared`) is under `widths` (exactly `exact_widths`)
/// by more than `rounding`: `gap < widths - rounding`. For a rounding below the widths that
/// is `gap^2 - widths^2 < -(2 widths - rounding) rounding`, compared exactly. A rounding
/// that reaches the widths leaves no gap proved under them: at such a size the coordinates
/// do not tell the two bands apart.
fn gap_proved_under(
    gap_squared: &[f64],
    exact_widths: &[f64],
    widths: f64,
    rounding: f64,
) -> Result<bool> {
    if rounding.is_nan() || rounding >= widths {
        return Ok(false);
    }
    let slack = interval::mul(&[2.0 * widths - rounding], &[rounding])?;
    Ok(exact::sign(&interval::add(
        &interval::sub(gap_squared, &interval::mul(exact_widths, exact_widths)?)?,
        &slack,
    )?) < 0)
}

/// Construct the retained normal contact and classify its closed width event.
/// The witness location is retained for the caller's trimmed-face classification.
pub(super) fn retained_contact(
    k: &Kernel,
    target: &Brep,
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
    if let (CurveKind::Line(line_a), CurveKind::Line(line_b)) = (a.curve.kind(), b.curve.kind()) {
        let cross = exact::cross(line_a.line_vector(), line_b.line_vector())
            .ok_or_else(|| SolidError::invalid_input(OP, "line directions outside exact range"))?;
        if !cross.iter().all(|component| exact::is_zero(component)) {
            return opposing::retained_contact(k, target, &a, &b, blended, corners, as_scale);
        }
    }
    let tolerance = edge::tolerance(target, a.edge)?.max(edge::tolerance(target, b.edge)?);
    let Some(closest) = closest_feet(k, target, &a, &b, tolerance, blended, corners)? else {
        return Ok(None);
    };
    let [foot_a, foot_b] = closest.points;
    let Some((gap, normal_a, normal_b)) = normal_contact(k, &a, &b, closest.points)? else {
        return Ok(None);
    };
    let between = foot_b - foot_a;
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
        if closest.parallel_family {
            return Err(SolidError::invalid_input(
                OP,
                "rounded parallel contact lost its exact clearance",
            ));
        }
        return Ok(None);
    }
    let mut gap_squared = Vec::new();
    for (p, q) in [
        (foot_a.x, foot_b.x),
        (foot_a.y, foot_b.y),
        (foot_a.z, foot_b.z),
    ] {
        let delta = interval::sub(&[q], &[p])?;
        gap_squared = interval::add(&gap_squared, &interval::mul(&delta, &delta)?)?;
    }
    let exact_widths = interval::add(&[a.width], &[b.width])?;
    // The feet are the model's own points, each coordinate a few roundings off the designed
    // one: a face meant 2 wide measured 1.9999999999999991 between an edge a cut left at
    // z = 6.000000000000001 and one at 8 (#20450), and two 1 mm bands that exactly meet on it
    // read as overlapping by 9e-16. The bands overlap only when the gap is proved under
    // their widths by more than those roundings can account for, as a contact is read
    // ([`crate::geom::tol::within_contact_tol`]): `gamma(32)` of the largest coordinate
    // involved.
    let size = foot_a.max_dimension().max(foot_b.max_dimension()) + widths;
    let rounding = crate::nurbs::math::gamma(32) * size;
    let overlapping = gap_proved_under(&gap_squared, &exact_widths, widths, rounding)?;
    let rho = a
        .offsets
        .first()
        .copied()
        .ok_or_else(|| missing(BandInput::FirstOffset))?
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
        at: contact_at,
        overlapping,
    }))
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
    let Some(mut contact) = retained_contact(k, target, a, b, rails, blended, corners, as_scale)?
    else {
        return Ok(None);
    };
    if super::sample_off_face(k, target, contact.face, contact.at)? {
        return Ok(None);
    }
    contact.at = crate::topology::face::p2::calculate_an_internal_point(k, target, contact.face)?;
    Ok(Some(contact))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `gap < widths - rounding`, for a gap `g` and two widths of 1.
    fn proved(g: f64, rounding: f64) -> bool {
        let squared = interval::mul(&[g], &[g]).unwrap();
        let widths = interval::add(&[1.0], &[1.0]).unwrap();
        gap_proved_under(&squared, &widths, 2.0, rounding).unwrap()
    }

    /// Two bands overlap when the gap is under their widths by more than the rounding of the
    /// coordinates it was measured on, and not otherwise: an exact meeting, the 9e-16
    /// shortfall of #20450 and a shortfall of exactly the rounding are no overlap; one past
    /// it is. A rounding that reaches the widths (coordinates near 2e15 for widths of 2)
    /// proves no overlap, whatever the gap.
    #[test]
    fn a_gap_is_under_the_widths_only_past_its_rounding() {
        let rounding = crate::nurbs::math::gamma(32) * 10.0;
        assert!(!proved(2.0, rounding));
        assert!(!proved(1.999_999_999_999_999_1, rounding));
        assert!(!proved(2.0 - rounding, rounding));
        assert!(proved(2.0 - 4.0 * rounding, rounding));
        assert!(proved(1.98, rounding));
        // with no rounding the comparison is the exact one
        assert!(!proved(2.0, 0.0));
        assert!(proved(1.999_999_999_999_999_1, 0.0));
        for rounding in [2.0, 2.5, 3.0, 5.0, f64::INFINITY, f64::NAN] {
            for g in [1e-9, 0.5, 1.0, 1.9, 2.0, 3.0] {
                assert!(!proved(g, rounding), "{g} {rounding}");
            }
        }
    }
}
