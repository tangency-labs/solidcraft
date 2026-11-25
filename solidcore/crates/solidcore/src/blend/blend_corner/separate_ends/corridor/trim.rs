//! Enclose the exact endpoints of straight edges on non-cardinal support planes.

use super::{cardinal, coords, expansion_span, lift_component, measured_span, resolved_vertex};
use crate::blend::fillet_section::interval::Ivl;
use crate::curve::curve::{Curve, CurveKind};
use crate::curve::line::Line;
use crate::error::Result;
use crate::geom::certified::next_up;
use crate::geom::expansion;
use crate::geom::extent1d::Extent1d;
use crate::kernel::Kernel;
use crate::math::{Point2, Point3};
use crate::nurbs::math::{gamma, CPoint, NOW};
use crate::surface::plane::Plane;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::face::p2 as face2;
use crate::topology::{edge, vertex, Brep, EdgeId, EdgeuseId, FaceId, VertexId};

pub(super) type PointBounds = [Ivl; 3];
pub(super) type UvEnds = [[Vec<f64>; 2]; 2];

pub(super) fn point_bounds(point: Point3) -> PointBounds {
    coords(point).map(Ivl::from)
}

pub(super) fn subtract(a: PointBounds, b: PointBounds) -> PointBounds {
    let [x, y, z] = a;
    let [u, v, w] = b;
    [x.sub_ulp(u), y.sub_ulp(v), z.sub_ulp(w)]
}

pub(super) fn cross(a: PointBounds, b: PointBounds) -> PointBounds {
    let [x, y, z] = a;
    let [u, v, w] = b;
    [
        y.mul_ulp(w).sub_ulp(z.mul_ulp(v)),
        z.mul_ulp(u).sub_ulp(x.mul_ulp(w)),
        x.mul_ulp(v).sub_ulp(y.mul_ulp(u)),
    ]
}

pub(super) fn dot(a: PointBounds, b: PointBounds) -> Ivl {
    a.into_iter()
        .zip(b)
        .fold(Ivl::from(0.0), |sum, (x, y)| sum.add_ulp(x.mul_ulp(y)))
}

pub(super) fn excludes_zero(value: Ivl) -> bool {
    value.lo > 0.0 || value.hi < 0.0
}

fn encloses_distance(a: PointBounds, b: PointBounds, limit: f64) -> bool {
    if !limit.is_finite() || limit < 0.0 {
        return false;
    }
    // The 1-norm bounds the Euclidean distance from above.
    let distance = subtract(a, b).into_iter().fold(0.0, |sum, range| {
        let component = range.mag();
        if component == 0.0 {
            sum
        } else {
            next_up(sum + component)
        }
    });
    distance <= limit
}

pub(super) fn stored_vertex_in_zone(
    b: &Brep,
    v: VertexId,
    point: PointBounds,
    allowance: f64,
) -> Result<bool> {
    let stored = point_bounds(vertex::point(b, v)?);
    Ok(encloses_distance(
        point,
        stored,
        vertex::tolerance(b, v)?.min(allowance),
    ))
}

struct SupportPlane {
    normal: PointBounds,
    origin: PointBounds,
    exact_normal: [Vec<f64>; 3],
}

fn support_plane(b: &Brep, face: FaceId) -> Result<Option<SupportPlane>> {
    let Some(SurfaceKind::Plane(plane)) = face2::surface(b, face)?.map(Surface::kind) else {
        return Ok(None);
    };
    let frame = plane.position();
    // Cross the actual chart axes. Its field normal may itself be rounded.
    let Some(exact_normal) = expansion::cross(frame.x_axis(), frame.y_axis()) else {
        return Ok(None);
    };
    let [x, y, z] = exact_normal
        .each_ref()
        .map(|terms| expansion_span(terms).map(|span| Ivl::new(span.lo, span.hi)));
    let (Some(x), Some(y), Some(z)) = (x, y, z) else {
        return Ok(None);
    };
    Ok(Some(SupportPlane {
        normal: [x, y, z],
        origin: point_bounds(frame.origin()),
        exact_normal,
    }))
}

/// A rounded solve can straddle an exactly representable vertex. Certify the
/// stored point directly with expansion arithmetic before enclosing a quotient.
fn contains_exactly(plane: &SupportPlane, stored: PointBounds) -> bool {
    plane
        .exact_normal
        .iter()
        .zip(plane.origin)
        .zip(stored)
        .try_fold(Vec::new(), |sum, ((normal, origin), stored)| {
            let at = expansion::scale(normal, stored.lo)?;
            let from = expansion::scale(normal, origin.lo)?;
            Some(expansion::add(&sum, &expansion::sub(&at, &from)))
        })
        .is_some_and(|deviation| expansion::is_zero(&deviation))
}

/// Enclose the intersection of three independent plane charts. Translate the
/// equations about the stored vertex to keep the interval cancellation local.
/// A cardinal incident plane fixes its coordinate exactly, including where the
/// trim ends on one of the corridor's permitted contact planes.
pub(super) fn vertex_bounds(b: &Brep, v: VertexId) -> Result<Option<PointBounds>> {
    if let Some(point) = resolved_vertex(b, v)? {
        return Ok(Some(point_bounds(point)));
    }
    let faces = vertex::faces(b, v)?;
    let [a, b_face, c] = faces.as_slice() else {
        return Ok(None);
    };
    let (Some(a), Some(z), Some(c)) = (
        support_plane(b, *a)?,
        support_plane(b, *b_face)?,
        support_plane(b, *c)?,
    ) else {
        return Ok(None);
    };
    let planes = [a, z, c];
    let [a, z, c] = planes.each_ref().map(|plane| plane.normal);
    let cofactors = [cross(z, c), cross(c, a), cross(a, z)];
    let det = dot(a, cofactors[0]);
    if !excludes_zero(det) {
        return Ok(None);
    }
    let stored = point_bounds(vertex::point(b, v)?);
    if planes.iter().all(|plane| contains_exactly(plane, stored)) {
        return Ok(Some(stored));
    }
    let rhs = planes
        .each_ref()
        .map(|p| dot(p.normal, subtract(p.origin, stored)));
    let mut point = stored;
    let [a, z, c] = cofactors;
    for (value, ((a, z), c)) in point.iter_mut().zip(a.into_iter().zip(z).zip(c)) {
        *value = value.add_ulp(dot([a, z, c], rhs).div_ulp(det));
    }
    for plane in planes {
        let mut axes = plane
            .normal
            .into_iter()
            .enumerate()
            .filter(|(_, n)| *n != Ivl::from(0.0));
        if let Some((axis, normal)) = axes.next() {
            if axes.next().is_none() && excludes_zero(normal) {
                if let Some((value, fixed)) = point.get_mut(axis).zip(plane.origin.get(axis)) {
                    *value = *fixed;
                }
            }
        }
    }
    Ok(encloses_distance(point, stored, vertex::tolerance(b, v)?).then_some(point))
}

/// The plane's affine UV lift with outward interval arithmetic. Unlike an
/// expansion, this also encloses products for subnormal UV error bounds.
fn lift_box(plane: &Plane, [u, v]: [Ivl; 2], point: bool) -> PointBounds {
    if u.lo == u.hi && v.lo == v.hi {
        let lifted = [0, 1, 2].map(|axis| {
            expansion_span(&lift_component(
                plane,
                Point2::new(u.lo, v.lo),
                axis,
                point,
            )?)
            .map(|s| Ivl::new(s.lo, s.hi))
        });
        if let [Some(x), Some(y), Some(z)] = lifted {
            return [x, y, z];
        }
    }
    let frame = plane.position();
    let u = u.mul_ulp(Ivl::from(plane.uv_scale().x));
    let v = v.mul_ulp(Ivl::from(plane.uv_scale().y));
    let mut result = if point {
        point_bounds(frame.origin())
    } else {
        [Ivl::from(0.0); 3]
    };
    for ((axis, x), y) in result
        .iter_mut()
        .zip(coords(frame.x_axis()))
        .zip(coords(frame.y_axis()))
    {
        *axis = axis
            .add_ulp(u.mul_ulp(Ivl::from(x)))
            .add_ulp(v.mul_ulp(Ivl::from(y)));
    }
    result
}

pub(super) fn uv_bounds(plane: &Plane, [first, last]: [[Ivl; 2]; 2]) -> [PointBounds; 2] {
    let endpoints = [lift_box(plane, first, true), lift_box(plane, last, true)];
    if plane.is_out_of_bounds_enabled() {
        return endpoints;
    }
    // The clamp displacement is monotone in each coordinate. Use the total
    // affine UV range to enclose displacement throughout the whole trim.
    let [u0, v0] = first;
    let [u1, v1] = last;
    let [u, v] = [u0.hull(u1), v0.hull(v1)];
    let domain = plane.step_uv_domain();
    let lo = domain.min();
    let hi = domain.max();
    let displacement = |range: Ivl, min: f64, max: f64| {
        let delta = |t: f64| Ivl::from(t.min(max).max(min)).sub_ulp(Ivl::from(t));
        delta(range.lo).hull(delta(range.hi))
    };
    let [du, dv] = [displacement(u, lo.x, hi.x), displacement(v, lo.y, hi.y)];
    let [u, v, w] = lift_box(plane, [du, dv], false);
    endpoints.map(|[x, y, z]| [x.add_ulp(u), y.add_ulp(v), z.add_ulp(w)])
}

fn spline_uv_endpoints(points: &[CPoint]) -> Option<[[Ivl; 2]; 2]> {
    let [a, z] = points else {
        return None;
    };
    [a, z]
        .into_iter()
        .all(|p| (p.w == NOW || p.w == 1.0) && p.x.is_finite() && p.y.is_finite())
        .then(|| [a, z].map(|p| [Ivl::from(p.x), Ivl::from(p.y)]))
}

fn match_endpoints(
    actual: [PointBounds; 2],
    ideal: [PointBounds; 2],
    tolerances: [f64; 2],
) -> bool {
    let [a, z] = actual;
    let [u, v] = ideal;
    let monotone = a
        .into_iter()
        .zip(z)
        .zip(u.into_iter().zip(v))
        .any(|((a, z), (u, v))| (a.hi < z.lo && u.hi < v.lo) || (z.hi < a.lo && v.hi < u.lo));
    let [first, last] = tolerances;
    monotone && encloses_distance(a, u, first) && encloses_distance(z, v, last)
}

pub(super) fn line_endpoints(line: &Line, interval: &Extent1d) -> Option<[PointBounds; 2]> {
    let [first, last] = line.segment_over(interval)?;
    let bounds = |t, (point, error)| {
        line.exact_point_at_parameter(t).map_or_else(
            || {
                coords(point).map(|x| {
                    let span = measured_span(x, error);
                    Ivl::new(span.lo, span.hi)
                })
            },
            point_bounds,
        )
    };
    Some([bounds(interval.min(), first), bounds(interval.max(), last)])
}

/// Enclose each spatial coordinate independently at the selected endpoint.
fn line_end_bounds(line: &Line, parameter: f64) -> Option<PointBounds> {
    let errors = line.point_coordinate_errors(parameter)?;
    let point = line.evaluate_point(parameter);
    if !coords(point).into_iter().all(f64::is_finite) {
        return None;
    }
    let work = coords(point).into_iter().zip(errors).map(|(value, error)| {
        let span = measured_span(value, error);
        Ivl::new(span.lo, span.hi)
    });
    let mut bounds = [Ivl::from(0.0); 3];
    for (out, value) in bounds.iter_mut().zip(work) {
        *out = value;
    }
    if let Some(exact) = line.point_expansions_at_parameter(parameter) {
        for (value, terms) in bounds.iter_mut().zip(exact) {
            let span = expansion_span(&terms)?;
            *value = Ivl::new(span.lo, span.hi);
        }
    }
    for ((value, direction), origin) in bounds
        .iter_mut()
        .zip(coords(line.line_vector()))
        .zip(coords(line.line_point()))
    {
        if direction == 0.0 {
            *value = Ivl::from(origin);
        }
    }
    Some(bounds)
}

/// The lifted ends of an affine stored trim over the actual edge interval.
pub(super) fn lifted_trim(
    b: &Brep,
    eu: EdgeuseId,
    interval: &Extent1d,
    plane: &Plane,
) -> Result<Option<[PointBounds; 2]>> {
    let Some(trim) = eu1::uv_trim_curve(b, eu)?.0 else {
        return Ok(None);
    };
    let uv = match trim.kind() {
        CurveKind::BSpline(spline)
            if spline.dim() == 2
                && spline.degree()? == 1
                && trim.natural_interval() == *interval =>
        {
            spline_uv_endpoints(spline.control_points()?)
        }
        CurveKind::Line(line) if line.dim() == 2 => {
            let [a, z] = [interval.min(), interval.max()]
                .map(|t| line_end_bounds(line, t).map(|[u, v, _]| [u, v]));
            a.zip(z).map(|(a, z)| [a, z])
        }
        _ => None,
    };
    Ok(uv.map(|uv| uv_bounds(plane, uv)))
}

pub(super) fn uv_terms(b: &Brep, eu: EdgeuseId) -> Result<Option<UvEnds>> {
    let Some(e) = eu1::edge(b, eu)? else {
        return Ok(None);
    };
    let Some(curve) = eu1::uv_trim_curve(b, eu)?.0 else {
        return Ok(None);
    };
    let interval = edge::interval(b, e)?;
    terms_for_curve(curve, interval)
}

fn terms_for_curve(curve: &Curve, interval: Extent1d) -> Result<Option<UvEnds>> {
    Ok(match curve.kind() {
        CurveKind::BSpline(spline)
            if spline.dim() == 2
                && spline.degree()? == 1
                && curve.natural_interval() == interval =>
        {
            spline_uv_endpoints(spline.control_points()?)
                .map(|ends| ends.map(|p| p.map(|c| vec![c.lo])))
        }
        CurveKind::Line(line) if line.dim() == 2 => {
            let ends = [interval.min(), interval.max()].map(|t| {
                line.point_expansions_at_parameter(t)
                    .map(|[u, v, _]| [u, v])
            });
            match ends {
                [Some(a), Some(z)] => Some([a, z]),
                _ => None,
            }
        }
        _ => None,
    })
}

/// Form the normal projection and its input scale in the cap's frame.
pub(super) fn relative_origin(
    point: Point3,
    anchor: Point3,
    normal: Point3,
) -> Option<(Vec<f64>, Ivl)> {
    coords(point)
        .into_iter()
        .zip(coords(anchor))
        .zip(coords(normal))
        .try_fold(
            (Vec::new(), Ivl::from(0.0)),
            |(sum, size), ((point, anchor), normal)| {
                let relative = expansion::sub(&[point], &[anchor]);
                let projected = expansion::scale(&relative, normal)?;
                let extent = expansion_span(&projected)?;
                let magnitude = Ivl::new(extent.lo, extent.hi).mag();
                Some((
                    expansion::add(&sum, &projected),
                    size.add_ulp(Ivl::from(magnitude)),
                ))
            },
        )
}

/// Project the affine lift before enclosing either endpoint. UV motion parallel
/// to the boundary then cancels exactly, even along a long trim.
pub(super) fn projected_uv_ends(
    plane: &Plane,
    ends: &UvEnds,
    normal: Point3,
    anchor: Point3,
) -> Option<[(Ivl, Ivl); 2]> {
    let chart = plane.position();
    let (origin, origin_size) = relative_origin(chart.origin(), anchor, normal)?;
    let slopes = [
        expansion::scale(&expansion::dot(normal, chart.x_axis())?, plane.uv_scale().x)?,
        expansion::scale(&expansion::dot(normal, chart.y_axis())?, plane.uv_scale().y)?,
    ];
    let [first, last] = ends;
    let domain = plane.step_uv_domain();
    let limits = [
        (domain.min().x, domain.max().x),
        (domain.min().y, domain.max().y),
    ];
    let unbounded = plane.is_out_of_bounds_enabled();
    if !unbounded
        && first
            .iter()
            .zip(last)
            .all(|(a, z)| !expansion::is_zero(&expansion::sub(a, z)))
        && first
            .iter()
            .zip(last)
            .zip(&slopes)
            .zip(limits)
            .any(|(((a, z), slope), (min, max))| {
                !expansion::is_zero(slope) && !shared_clamp_branch(a, z, min, max)
            })
    {
        // A clamp can bend a trim with two varying coordinates between its
        // endpoints. Keep the full raw enclosure where that projection bends.
        return None;
    }
    let cardinal_axis = cardinal([0.0; 3], coords(normal)).is_some();
    let magnitude = |terms: &[f64]| {
        let span = expansion_span(terms)?;
        Some(Ivl::from(span.lo.abs().max(span.hi.abs())))
    };
    let project = |uv: &[Vec<f64>; 2]| {
        let (value, size) = uv.iter().zip(&slopes).zip(limits).try_fold(
            (origin.clone(), origin_size),
            |(sum, size), ((terms, slope), (min, max))| {
                if !terms.iter().all(|t| t.is_finite()) {
                    return None;
                }
                let clamped = if unbounded {
                    terms.as_slice()
                } else if expansion::sign(&expansion::sub(terms, &[min])) < 0 {
                    std::slice::from_ref(&min)
                } else if expansion::sign(&expansion::sub(terms, &[max])) > 0 {
                    std::slice::from_ref(&max)
                } else {
                    terms.as_slice()
                };
                let contribution = clamped.iter().try_fold(Vec::new(), |sum, &t| {
                    Some(expansion::add(&sum, &expansion::scale(slope, t)?))
                })?;
                Some((
                    expansion::add(&sum, &contribution),
                    size.add_ulp(magnitude(&contribution)?),
                ))
            },
        )?;
        // Two coordinate products and three sums in the affine projection
        // relative to the cap. Its tangent coefficients are already combined.
        let roundoff = if cardinal_axis {
            Ivl::from(0.0)
        } else {
            let bound = size.mul_ulp(Ivl::from(gamma(5))).hi;
            Ivl::new(-bound, bound)
        };
        let span = expansion_span(&value)?;
        Some((Ivl::new(span.lo, span.hi), roundoff))
    };
    Some([project(first)?, project(last)?])
}

fn shared_clamp_branch(a: &[f64], z: &[f64], min: f64, max: f64) -> bool {
    if min == max {
        return true;
    }
    let lower = [a, z].map(|point| expansion::sign(&expansion::sub(point, &[min])));
    let upper = [a, z].map(|point| expansion::sign(&expansion::sub(point, &[max])));
    lower.into_iter().all(|sign| sign <= 0)
        || upper.into_iter().all(|sign| sign >= 0)
        || (lower.into_iter().all(|sign| sign >= 0) && upper.into_iter().all(|sign| sign <= 0))
}

/// Compare the affine restriction of a stored trim to the kernel's
/// deterministic planar drop of its own spatial edge.
pub(super) fn reprojects(
    k: &Kernel,
    b: &Brep,
    eu: EdgeuseId,
    face: FaceId,
    plane: &Plane,
) -> Result<bool> {
    Ok(reprojected_axes(k, b, eu, face, plane)? == [true; 2])
}

fn reprojected_axes(
    k: &Kernel,
    b: &Brep,
    eu: EdgeuseId,
    face: FaceId,
    plane: &Plane,
) -> Result<[bool; 2]> {
    let (Some(e), Some(actual)) = (eu1::edge(b, eu)?, uv_terms(b, eu)?) else {
        return Ok([false; 2]);
    };
    let Some(spatial) = edge::curve(b, e)? else {
        return Ok([false; 2]);
    };
    if !matches!(spatial.kind(), CurveKind::Line(_)) {
        return Ok([false; 2]);
    }
    let interval = edge::interval(b, e)?;
    let domain = face2::uv_domain(b, face)?;
    let generated = plane.drop_curve(
        k,
        &domain,
        spatial,
        &interval,
        edge::tolerance(b, e)?,
        false,
        true,
        None,
    )?;
    let [expected] = generated.uv_curves.as_slice() else {
        return Ok([false; 2]);
    };
    let Some(expected) = terms_for_curve(expected, interval)? else {
        return Ok([false; 2]);
    };
    let mut agrees = [true; 2];
    for (actual, expected) in actual.into_iter().zip(expected) {
        for ((agrees, a), z) in agrees.iter_mut().zip(actual).zip(expected) {
            *agrees &= expansion::is_zero(&expansion::sub(&a, &z));
        }
    }
    Ok(agrees)
}

fn exact_uv_ends(b: &Brep, eu: EdgeuseId) -> Result<Option<[[Option<f64>; 2]; 2]>> {
    let exact = |terms: Vec<f64>| {
        let at = expansion::to_f64(&terms);
        (at.is_finite() && expansion::is_zero(&expansion::sub(&terms, &[at]))).then_some(at)
    };
    Ok(uv_terms(b, eu)?.map(|ends| ends.map(|uv| uv.map(exact))))
}

fn uv_at(b: &Brep, eu: EdgeuseId, vertex: VertexId) -> Result<Option<[Option<f64>; 2]>> {
    let Some(e) = eu1::edge(b, eu)? else {
        return Ok(None);
    };
    let Some([from, to]) = exact_uv_ends(b, eu)? else {
        return Ok(None);
    };
    let (first, last) = edge::start_end_vertices(b, e)?;
    Ok(if first == Some(vertex) {
        Some(from)
    } else if last == Some(vertex) {
        Some(to)
    } else {
        None
    })
}

fn neighbors(b: &Brep, eu: EdgeuseId) -> Result<[Option<EdgeuseId>; 2]> {
    let face = eu1::face(b, eu)?;
    let mut neighbors = [eu1::cw_edgeuse(b, eu)?, eu1::ccw_edgeuse(b, eu)?];
    for neighbor in &mut neighbors {
        if let Some(id) = *neighbor {
            if id == eu || eu1::face(b, id)? != face {
                *neighbor = None;
            }
        }
    }
    Ok(neighbors)
}

/// Identify a rail or cap only through a radial seam at this loop endpoint.
pub(super) fn touches_support(
    b: &Brep,
    eu: EdgeuseId,
    vertex: VertexId,
    matches_support: impl Fn(FaceId) -> bool,
) -> Result<bool> {
    let [previous, next] = neighbors(b, eu)?;
    for occurrence in [Some(eu), previous, next].into_iter().flatten() {
        let (Some(e), Some(radial)) = (eu1::edge(b, occurrence)?, eu1::radial(b, occurrence)?)
        else {
            continue;
        };
        if !eu1::face(b, radial)?.is_some_and(&matches_support) {
            continue;
        }
        let (from, to) = edge::start_end_vertices(b, e)?;
        if from == Some(vertex) || to == Some(vertex) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn verified_iso_seam(
    k: &Kernel,
    b: &Brep,
    face: FaceId,
    plane: &Plane,
    eu: EdgeuseId,
    matches_cap: &impl Fn(FaceId) -> bool,
) -> Result<Option<(usize, f64)>> {
    let Some(radial) = eu1::radial(b, eu)? else {
        return Ok(None);
    };
    if !eu1::face(b, radial)?.is_some_and(matches_cap) {
        return Ok(None);
    }
    let Some(e) = eu1::edge(b, eu)? else {
        return Ok(None);
    };
    let (Some(from), Some(to)) = edge::start_end_vertices(b, e)? else {
        return Ok(None);
    };
    let Some([a, z]) = exact_uv_ends(b, eu)? else {
        return Ok(None);
    };
    let Some((axis, Some(level))) = a
        .into_iter()
        .zip(z)
        .enumerate()
        .find_map(|(i, (a, z))| (a.is_some() && a == z).then_some((i, a)))
    else {
        return Ok(None);
    };
    let neighbors = neighbors(b, eu)?;
    let mut joined = [false; 2];
    for (joined, vertex) in joined.iter_mut().zip([from, to]) {
        for neighbor in neighbors.into_iter().flatten() {
            *joined |= uv_at(b, neighbor, vertex)?.and_then(|p| p.get(axis).copied().flatten())
                == Some(level);
        }
    }
    // The transverse coordinate can agree exactly even when rotating the
    // charts introduces rounding along the seam.
    let proven = joined.into_iter().any(|same| same)
        || reprojected_axes(k, b, eu, face, plane)?.get(axis) == Some(&true);
    Ok(proven.then_some((axis, level)))
}

/// At a shared seam, certify the fixed support parameter with the adjacent
/// trims or by independently projecting the spatial edge onto this face.
pub(super) fn on_iso_seam(
    k: &Kernel,
    b: &Brep,
    face: FaceId,
    plane: &Plane,
    eu: EdgeuseId,
    vertex: VertexId,
    matches_cap: impl Fn(FaceId) -> bool,
) -> Result<bool> {
    if verified_iso_seam(k, b, face, plane, eu, &matches_cap)?.is_some() {
        return Ok(true);
    }
    let Some(at) = uv_at(b, eu, vertex)? else {
        return Ok(false);
    };
    for neighbor in neighbors(b, eu)?.into_iter().flatten() {
        let Some((axis, level)) = verified_iso_seam(k, b, face, plane, neighbor, &matches_cap)?
        else {
            continue;
        };
        if at.get(axis).copied().flatten() == Some(level) && uv_at(b, neighbor, vertex)?.is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Both incident plane charts select a unique straight carrier. Affine trims
/// stay within the existing edge zone of its bounded segment by convexity,
/// once their ordered endpoints agree within the edge and vertex zones.
pub(super) fn bound_affine_trim(
    b: &Brep,
    eu: EdgeuseId,
    e: EdgeId,
    plane: &Plane,
    line: &Line,
    vertices: [VertexId; 2],
    endpoints: [PointBounds; 2],
    allowance: f64,
) -> Result<Option<[PointBounds; 2]>> {
    let faces = edge::faces(b, e, None)?;
    let [a, z] = faces.as_slice() else {
        return Ok(None);
    };
    let (Some(a), Some(z)) = (support_plane(b, *a)?, support_plane(b, *z)?) else {
        return Ok(None);
    };
    if !cross(a.normal, z.normal).into_iter().any(excludes_zero) {
        return Ok(None);
    }
    let [u, v] = vertices;
    let zone = edge::tolerance(b, e)?.min(allowance);
    let tolerances = [
        zone.min(vertex::tolerance(b, u)?),
        zone.min(vertex::tolerance(b, v)?),
    ];
    let interval = edge::interval(b, e)?;
    let Some(xyz) = line_endpoints(line, &interval) else {
        return Ok(None);
    };
    if !match_endpoints(xyz, endpoints, tolerances) {
        return Ok(None);
    }
    Ok(lifted_trim(b, eu, &interval, plane)?
        .filter(|&lifted| match_endpoints(lifted, endpoints, tolerances)))
}
