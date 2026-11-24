//! A geometry-only room certificate for straight, cardinal separate ends.
//!
//! The selected edge and the support normal rays define a rectangular prism containing
//! either blend sweep. The two rail rectangles are inside their faces, the end rectangles
//! are outside the cap trims, and no other boundary reaches the portion being changed.

mod placed;
mod trim;

use self::placed::Placed;

use self::trim::{cross, dot, excludes_zero, point_bounds, subtract, vertex_bounds, PointBounds};
use crate::blend::fillet_section::interval::Ivl;
use crate::curve::curve::{Curve, CurveKind};
use crate::curve::line::Line;
use crate::error::topology::TopologyFailure;
use crate::error::Result;
use crate::error::SolidError;
use crate::geom::certified::{difference, distance, next_down, next_up};
use crate::geom::expansion;
use crate::geom::extent3d::Extent3d;
use crate::kernel::Kernel;
use crate::math::{Point2, Point3};
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::math::{CPoint, NOW};
use crate::surface::plane::Plane;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::edgeuse::p3::get_or_create_uv_trim_curve;
use crate::topology::face::p2 as face2;
use crate::topology::face::p4 as face4;
use crate::topology::{edge, loopuse, vertex, Brep, EdgeId, EdgeuseId, FaceId, VertexId};

const OP: &str = "blend::blend_corner::separate_ends::corridor";

pub(super) struct Corridor {
    pub(super) at: (VertexId, Point3),
    pub(super) ends: [(VertexId, Point3); 3],
    pub(super) far: [VertexId; 2],
    pub(super) faces: [FaceId; 6],
    pub(super) far_caps: [FaceId; 2],
}

pub(super) enum Clearance {
    Clear(Extent3d),
    Obstructed,
    Unproven,
}

type Coords = [f64; 3];
type Segment = [PointBounds; 2];
type Polygon = Vec<Vec<Segment>>;

#[derive(Clone, Copy)]
struct Span {
    lo: f64,
    hi: f64,
}

impl Span {
    fn between(a: f64, b: f64) -> Self {
        Self {
            lo: a.min(b),
            hi: a.max(b),
        }
    }

    fn overlaps(self, other: Self) -> bool {
        self.lo <= other.hi && self.hi >= other.lo
    }

    fn overlaps_open(self, other: Self) -> bool {
        self.lo < other.hi && self.hi > other.lo
    }

    // The side at the original edge belongs to the adjoining support; a
    // contact at the outer rail can change the blend and is not skipped.
    fn overlaps_from(self, other: Self, origin: f64) -> bool {
        if origin == other.lo {
            self.lo <= other.hi && self.hi > other.lo
        } else {
            self.lo < other.hi && self.hi >= other.lo
        }
    }

    fn middle(self) -> Option<f64> {
        let mid = self.lo.midpoint(self.hi);
        (self.lo < mid && mid < self.hi).then_some(mid)
    }
}

#[derive(Clone, Copy)]
struct Arm {
    along: usize,
    across: usize,
    above: usize,
    origin: Coords,
    spans: [Span; 3],
    rail_faces: [FaceId; 2],
    near_caps: [FaceId; 2],
    far_cap: FaceId,
}

fn coords(point: Point3) -> Coords {
    [point.x, point.y, point.z]
}

fn cardinal(origin: Coords, end: Coords) -> Option<(usize, bool)> {
    if !origin.into_iter().chain(end).all(f64::is_finite) {
        return None;
    }
    let mut varying = origin
        .into_iter()
        .zip(end)
        .enumerate()
        .filter(|(_, (a, b))| a != b);
    let (axis, (a, b)) = varying.next()?;
    varying.next().is_none().then_some((axis, b > a))
}

/// Enclose the end away from the original edge. Keep an exact sum unchanged:
/// a rail exactly at the opening's bounding plane may fit.
fn extended(value: f64, positive: bool, size: f64) -> Option<f64> {
    let delta = if positive { size } else { -size };
    let end = value + delta;
    if !end.is_finite() {
        return None;
    }
    let residual = expansion::sub(&expansion::add(&[value], &[delta]), &[end]);
    if !residual.iter().all(|term| term.is_finite()) {
        return None;
    }
    let outward = match (positive, expansion::sign(&residual)) {
        (true, 1) => next_up(end),
        (false, -1) => next_down(end),
        _ => end,
    };
    (outward.is_finite()
        && if positive {
            outward > value
        } else {
            outward < value
        })
    .then_some(outward)
}

impl Arm {
    fn new(
        at: Point3,
        rim: Point3,
        top: Point3,
        far: Point3,
        size: f64,
        faces: [FaceId; 5],
    ) -> Option<Self> {
        let [wall, floor, cap_wall, cap_opening, far_cap] = faces;
        let origin = coords(at);
        let far = coords(far);
        let (along, _) = cardinal(origin, far)?;
        let (across, across_positive) = cardinal(origin, coords(rim))?;
        let (above, above_positive) = cardinal(origin, coords(top))?;
        if along == across || along == above || across == above {
            return None;
        }
        let mut corner = far;
        *corner.get_mut(across)? = extended(*origin.get(across)?, across_positive, size)?;
        *corner.get_mut(above)? = extended(*origin.get(above)?, above_positive, size)?;
        let [a, b, c] = origin;
        let [x, y, z] = corner;
        Some(Self {
            along,
            across,
            above,
            origin,
            spans: [
                Span::between(a, x),
                Span::between(b, y),
                Span::between(c, z),
            ],
            rail_faces: [wall, floor],
            near_caps: [cap_wall, cap_opening],
            far_cap,
        })
    }

    fn permits_contact(self, face: FaceId) -> bool {
        self.rail_faces.contains(&face) || self.near_caps.contains(&face) || self.far_cap == face
    }

    fn contacts(self, bounds: [Span; 3]) -> Option<bool> {
        let along = *self.spans.get(self.along)?;
        let across = *self.spans.get(self.across)?;
        let above = *self.spans.get(self.above)?;
        let axial = *bounds.get(self.along)?;
        let side = *bounds.get(self.across)?;
        let height = *bounds.get(self.above)?;
        Some(
            // Include the rails' outer boundaries along the edge, where an
            // unrelated face can touch a new rail even without crossing it.
            (axial.overlaps_open(along) && side.overlaps(across) && height.overlaps(above))
                // At a cap an entire unrelated patch could coincide with the new
                // end. Common edges on the original support planes are allowed.
                || (axial.overlaps(along)
                    && side.overlaps_from(across, *self.origin.get(self.across)?)
                    && height.overlaps_from(above, *self.origin.get(self.above)?)),
        )
    }
}

/// Intersect the vertex's three exact cardinal support planes. The stored
/// vertex may differ from the intersection within its existing zone. Copying
/// one coordinate from each plane gives their intersection without rounding.
pub(super) fn resolved_vertex(b: &Brep, v: VertexId) -> Result<Option<Point3>> {
    let mut coordinates = [None; 3];
    for face in vertex::faces(b, v)? {
        let Some(SurfaceKind::Plane(plane)) = face2::surface(b, face)?.map(Surface::kind) else {
            return Ok(None);
        };
        let (origin, normal, _) = plane.is_planar();
        let Some((axis, _)) = cardinal([0.0; 3], coords(normal)) else {
            return Ok(None);
        };
        let (Some(&value), Some(slot)) = (coords(origin).get(axis), coordinates.get_mut(axis))
        else {
            return Ok(None);
        };
        if !value.is_finite() || slot.is_some_and(|old| old != value) {
            return Ok(None);
        }
        *slot = Some(value);
    }
    let [Some(x), Some(y), Some(z)] = coordinates else {
        return Ok(None);
    };
    let intersection = Point3::new(x, y, z);
    let stored = vertex::point(b, v)?;
    let zone = vertex::tolerance(b, v)?;
    let agrees = intersection == stored || distance(intersection, stored).at_most(zone).is_yes();
    Ok(agrees.then_some(intersection))
}

fn along_ends_match([first, last]: [Span; 2], ends: [f64; 2], zones: [f64; 2]) -> bool {
    let [a, z] = ends;
    let earlier = if a < z {
        first.hi < last.lo
    } else {
        first.lo > last.hi
    };
    earlier
        && [first, last]
            .into_iter()
            .zip(ends)
            .zip(zones)
            .all(|((span, at), zone)| {
                zone.is_finite()
                    && zone >= 0.0
                    && (span.lo == at || next_up(at - span.lo) <= zone)
                    && (span.hi == at || next_up(span.hi - at) <= zone)
            })
}

fn measured_span(point: f64, error: f64) -> Span {
    if error == 0.0 {
        return Span::between(point, point);
    }
    Span {
        lo: next_down(point - error),
        hi: next_up(point + error),
    }
}

fn expansion_span(parts: &[f64]) -> Option<Span> {
    let mut terms = parts.iter().copied().filter(|term| *term != 0.0);
    let first = terms.next().unwrap_or(0.0);
    let mut span = Span::between(first, first);
    for term in terms {
        span.lo = next_down(span.lo + term);
        span.hi = next_up(span.hi + term);
    }
    (span.lo.is_finite() && span.hi.is_finite()).then_some(span)
}

/// One lifted coordinate of a UV point or direction, kept as an exact expansion.
fn lift_component(plane: &Plane, uv: Point2, axis: usize, point: bool) -> Option<Vec<f64>> {
    let frame = plane.position();
    let [scale_u, scale_v] = [plane.uv_scale().x, plane.uv_scale().y];
    let u = *coords(frame.x_axis()).get(axis)?;
    let v = *coords(frame.y_axis()).get(axis)?;
    // Product expansions decline a subnormal factor. Multiplication by zero or
    // a sign is exact even at the outward enclosure of a zero UV coordinate.
    let mul = |parameter: f64, scale: f64, direction: f64| {
        if direction == 0.0 || parameter == 0.0 {
            return (parameter.is_finite() && scale.is_finite() && direction.is_finite())
                .then(|| vec![0.0]);
        }
        let scaled = if scale.abs() == 1.0 {
            vec![parameter * scale]
        } else {
            expansion::product(parameter, scale)?
        };
        if direction.abs() == 1.0 {
            Some(scaled.into_iter().map(|term| term * direction).collect())
        } else {
            expansion::scale(&scaled, direction)
        }
    };
    let pu = mul(uv.x, scale_u, u)?;
    let pv = mul(uv.y, scale_v, v)?;
    let sum = expansion::add(&pu, &pv);
    Some(if point {
        expansion::add(&sum, &[*coords(frame.origin()).get(axis)?])
    } else {
        sum
    })
}

/// Plane evaluation may clamp a UV endpoint. Allow that only if the trim's fixed
/// chart coordinate is inside the domain, so clamping moves along the same line.
fn can_clamp(plane: &Plane, [fixed_u, fixed_v]: [Option<f64>; 2]) -> bool {
    let domain = plane.step_uv_domain();
    let lo = domain.min();
    let hi = domain.max();
    fixed_u.is_some_and(|u| lo.x <= u && u <= hi.x)
        || fixed_v.is_some_and(|v| lo.y <= v && v <= hi.y)
}

fn spline_bounds(
    plane: &Plane,
    knot_ends: &[CPoint],
    at: Coords,
    along: usize,
) -> Option<[Span; 2]> {
    let [a, z] = knot_ends else {
        return None;
    };
    if [a, z]
        .into_iter()
        .any(|cp| (cp.w != NOW && cp.w != 1.0) || !(cp.x.is_finite() && cp.y.is_finite()))
    {
        return None;
    }
    let fixed = [(a.x == z.x).then_some(a.x), (a.y == z.y).then_some(a.y)];
    let lift = |uv: Point2| -> Option<Span> {
        let mut axial = None;
        for (axis, coordinate) in at.into_iter().enumerate() {
            let exact = lift_component(plane, uv, axis, true)?;
            if axis == along {
                axial = expansion_span(&exact);
            } else if !expansion::is_zero(&expansion::sub(&exact, &[coordinate])) {
                return None;
            }
        }
        axial
    };
    let enclose = |uv| -> Option<Span> {
        let mut span = lift(uv)?;
        let domain = plane.step_uv_domain();
        if !plane.is_out_of_bounds_enabled() && !domain.contains_point(uv, 0.0) {
            if !can_clamp(plane, fixed) {
                return None;
            }
            let clamped = lift(domain.clamp_point(uv))?;
            span.lo = span.lo.min(clamped.lo);
            span.hi = span.hi.max(clamped.hi);
        }
        Some(span)
    };
    Some([
        enclose(Point2::new(a.x, a.y))?,
        enclose(Point2::new(z.x, z.y))?,
    ])
}

fn uv_line_bounds(
    plane: &Plane,
    line: &Line,
    interval: &crate::geom::extent1d::Extent1d,
    at: Coords,
    along: usize,
) -> Option<[Span; 2]> {
    let p = line.line_point();
    let v = line.line_vector();
    for (axis, coordinate) in at.into_iter().enumerate() {
        if along == axis {
            continue;
        }
        let origin = lift_component(plane, Point2::new(p.x, p.y), axis, true)?;
        let direction = lift_component(plane, Point2::new(v.x, v.y), axis, false)?;
        if !expansion::is_zero(&expansion::sub(&origin, &[coordinate]))
            || !expansion::is_zero(&direction)
        {
            return None;
        }
    }
    let fixed = [(v.x == 0.0).then_some(p.x), (v.y == 0.0).then_some(p.y)];
    let domain = plane.step_uv_domain();
    let lift = |(p, error): (Point3, f64)| -> Option<Span> {
        let u = measured_span(p.x, error);
        let v = measured_span(p.y, error);
        let mut bound = Span {
            lo: f64::INFINITY,
            hi: f64::NEG_INFINITY,
        };
        for uv in [
            Point2::new(u.lo, v.lo),
            Point2::new(u.lo, v.hi),
            Point2::new(u.hi, v.lo),
            Point2::new(u.hi, v.hi),
        ] {
            let mut corner = expansion_span(&lift_component(plane, uv, along, true)?)?;
            if !plane.is_out_of_bounds_enabled() && !domain.contains_point(uv, 0.0) {
                if !can_clamp(plane, fixed) {
                    return None;
                }
                let clamped =
                    expansion_span(&lift_component(plane, domain.clamp_point(uv), along, true)?)?;
                corner.lo = corner.lo.min(clamped.lo);
                corner.hi = corner.hi.max(clamped.hi);
            }
            bound.lo = bound.lo.min(corner.lo);
            bound.hi = bound.hi.max(corner.hi);
        }
        Some(bound)
    };
    let [a, z] = line.segment_over(interval)?;
    Some([lift(a)?, lift(z)?])
}

/// An oriented loop end and the direction of the trim traversing it.
#[derive(Clone, Copy)]
struct LoopEnd {
    vertex: VertexId,
    direction: PointBounds,
}

impl LoopEnd {
    fn joins(self, next: Self) -> bool {
        self.vertex == next.vertex
            && (cross(self.direction, next.direction)
                .into_iter()
                .any(excludes_zero)
                || dot(self.direction, next.direction).lo > 0.0)
    }
}

/// Support trims can coincide with a rail or cap exactly. On other faces the
/// enclosure is used only to exclude the whole face from the sweep.
#[derive(Clone, Copy)]
enum TrimUse {
    Support,
    OtherFace,
}

/// Bind a straight polygon segment to the edge's 3D line and lifted UV trim.
fn segment(
    b: &Brep,
    eu: EdgeuseId,
    plane: &Plane,
    use_: TrimUse,
    placed: Option<&Placed>,
) -> Result<Option<(Segment, LoopEnd, LoopEnd)>> {
    let Some(e) = eu1::edge(b, eu)? else {
        return Ok(None);
    };
    let Some(CurveKind::Line(line)) = edge::curve(b, e)?.map(Curve::kind) else {
        return Ok(None);
    };
    let (Some(u), Some(v)) = edge::start_end_vertices(b, e)? else {
        return Ok(None);
    };
    let (a, z) = if let Some(placed) = placed {
        let (Some(a), Some(z)) = (placed.point(b, u)?, placed.point(b, v)?) else {
            return Ok(None);
        };
        if !placed.binds_trim(b, eu, e, plane, line, [u, v], [a, z])? {
            return Ok(None);
        }
        (point_bounds(a), point_bounds(z))
    } else {
        let (Some(a), Some(z)) = (vertex_bounds(b, u)?, vertex_bounds(b, v)?) else {
            return Ok(None);
        };
        let cardinal_bounds = match (resolved_vertex(b, u)?, resolved_vertex(b, v)?) {
            (Some(a), Some(z)) => cardinal(coords(a), coords(z)),
            _ => None,
        };
        let bound = if let Some((along, _)) = cardinal_bounds {
            binds_cardinal_trim(b, eu, e, plane, line, [u, v], [a, z], along)?
                || (matches!(use_, TrimUse::OtherFace)
                    && trim::binds_affine_trim(
                        b,
                        eu,
                        e,
                        plane,
                        line,
                        [u, v],
                        [a, z],
                        f64::INFINITY,
                    )?)
        } else {
            trim::binds_affine_trim(b, eu, e, plane, line, [u, v], [a, z], f64::INFINITY)?
        };
        if !bound {
            return Ok(None);
        }
        (a, z)
    };
    let (u, v, direction) = match eu1::orientation(b, eu)? {
        OrientType::Same => (u, v, subtract(z, a)),
        OrientType::Opposite => (v, u, subtract(a, z)),
        OrientType::Unknown => return Ok(None),
    };
    Ok(Some((
        [a, z],
        LoopEnd {
            vertex: u,
            direction,
        },
        LoopEnd {
            vertex: v,
            direction,
        },
    )))
}

/// Bind a cardinal trim to its projected carrier, including exact contacts
/// across the two perpendicular rails of the corridor.
fn binds_cardinal_trim(
    b: &Brep,
    eu: EdgeuseId,
    e: EdgeId,
    plane: &Plane,
    line: &Line,
    [u, v]: [VertexId; 2],
    [a, z]: Segment,
    along: usize,
) -> Result<bool> {
    let [a, z] = [a.map(|at| at.lo), z.map(|at| at.lo)];
    let point = coords(line.line_point());
    let direction = coords(line.line_vector());
    let Some((normal, _)) = cardinal([0.0; 3], coords(plane.position().z_axis())) else {
        return Ok(false);
    };
    let trim = eu1::uv_trim_curve(b, eu)?.0;
    for (axis, ((at, p), d)) in a.into_iter().zip(point).zip(direction).enumerate() {
        if axis == along {
            continue;
        }
        if d != 0.0 {
            return Ok(false);
        }
        if at != p {
            // The line may sit within the edge zone above or below this face.
            // Its projection has the same in-plane carrier. Require the
            // stored pcurve to prove that carrier too.
            if axis != normal || !difference(at, p).at_most(edge::tolerance(b, e)?).is_yes() {
                return Ok(false);
            }
        }
    }

    let zones = [vertex::tolerance(b, u)?, vertex::tolerance(b, v)?];
    let (Some(&first), Some(&last)) = (a.get(along), z.get(along)) else {
        return Ok(false);
    };
    let ends = [first, last];
    // The endpoint enclosures keep the analytic line's actual parameter span separate
    // from the ideal carrier intersection. Its two other coordinates are exact.
    let xyz = line
        .segment_over(&edge::interval(b, e)?)
        .and_then(|[(p, dp), (q, dq)]| {
            Some([
                measured_span(*coords(p).get(along)?, dp),
                measured_span(*coords(q).get(along)?, dq),
            ])
        });
    let Some(xyz) = xyz else {
        return Ok(false);
    };
    if !along_ends_match(xyz, ends, zones) {
        return Ok(false);
    }
    let uv = match trim.map(|trim| (trim.kind(), trim.natural_interval())) {
        Some((CurveKind::BSpline(spline), _)) if spline.dim() == 2 && spline.degree()? == 1 => {
            spline_bounds(plane, spline.control_points()?, a, along)
        }
        Some((CurveKind::Line(line), interval)) if line.dim() == 2 => {
            uv_line_bounds(plane, line, &interval, a, along)
        }
        _ => None,
    };
    let Some(uv) = uv else {
        return Ok(false);
    };
    Ok(along_ends_match(uv, ends, zones))
}

/// Enclose the closed trim polygons as in `band_room`. Adjoining carriers
/// intersect at their common vertex or continue along the same line. Extending an interval
/// enclosure past such a corner would mistake a transverse trim that ends at
/// a permitted contact for one that enters the open rail rectangle or prism.
fn segments(
    k: &Kernel,
    b: &Brep,
    face: FaceId,
    plane: &Plane,
    use_: TrimUse,
    placed: Option<&Placed>,
) -> Result<Option<Polygon>> {
    let mut polygon = Vec::new();
    let loops = face4::upward_loopuses(b, face, false, None)?;
    polygon
        .try_reserve_exact(loops.len())
        .map_err(|_| SolidError::topology(OP, TopologyFailure::AllocationFailed))?;
    for lu in loops {
        let mut segments = Vec::new();
        let uses = loopuse::edgeuses(b, lu, None, None)?;
        segments
            .try_reserve_exact(uses.len())
            .map_err(|_| SolidError::topology(OP, TopologyFailure::AllocationFailed))?;
        let mut first = None;
        let mut last: Option<LoopEnd> = None;
        for eu in uses {
            k.check_cancelled(OP)?;
            let Some((segment, start, end)) = segment(b, eu, plane, use_, placed)? else {
                return Ok(None);
            };
            if last.is_some_and(|prev| !prev.joins(start)) {
                return Ok(None);
            }
            first.get_or_insert(start);
            last = Some(end);
            segments.push(segment);
        }
        if !matches!((last, first), (Some(last), Some(first)) if last.joins(first)) {
            return Ok(None);
        }
        polygon.push(segments);
    }
    Ok((!polygon.is_empty()).then_some(polygon))
}

fn planar_polygon(
    k: &Kernel,
    b: &mut Brep,
    face: FaceId,
    use_: TrimUse,
    placed: Option<&Placed>,
) -> Result<Option<Polygon>> {
    if !matches!(
        face2::surface(b, face)?.map(Surface::kind),
        Some(SurfaceKind::Plane(_))
    ) || face2::outer_loops(b, face)?.is_empty()
    {
        return Ok(None);
    }
    for eu in face4::upward_edgeuses(b, face, false, None)? {
        k.check_cancelled(OP)?;
        if eu1::uv_trim_curve(b, eu)?.0.is_none() && !certifiable_lazy_trim(k, b, eu)? {
            return Ok(None);
        }
    }
    let Some(SurfaceKind::Plane(plane)) = face2::surface(b, face)?.map(Surface::kind) else {
        return Ok(None);
    };
    segments(k, b, face, plane, use_, placed)
}

/// Populate a missing planar UV trim on the planning copy. A drop that needs
/// a different 3D range or a larger zone cannot prove the original boundary.
fn certifiable_lazy_trim(k: &Kernel, b: &mut Brep, eu: EdgeuseId) -> Result<bool> {
    let Some(edge) = eu1::edge(b, eu)? else {
        return Ok(false);
    };
    if !matches!(
        edge::curve(b, edge)?.map(Curve::kind),
        Some(CurveKind::Line(_))
    ) {
        return Ok(false);
    }
    let (Some(a), Some(z)) = edge::start_end_vertices(b, edge)? else {
        return Ok(false);
    };
    let interval = edge::interval(b, edge)?;
    let zones = [
        edge::tolerance(b, edge)?,
        vertex::tolerance(b, a)?,
        vertex::tolerance(b, z)?,
    ];
    // A failed drop must also leave the planning copy unchanged, since a
    // different face may still establish a real obstruction.
    b.with_rollback_untouched(
        |b| {
            let drop = get_or_create_uv_trim_curve(k, b, eu, false, false, true)?;
            Ok(drop.status.is_ok()
                && edge::interval(b, edge)? == interval
                && [
                    edge::tolerance(b, edge)?,
                    vertex::tolerance(b, a)?,
                    vertex::tolerance(b, z)?,
                ] == zones)
        },
        |result| matches!(result, Ok(true)),
    )
}

fn segment_bounds([a, b]: Segment) -> [Span; 3] {
    let hull = |a: Ivl, b: Ivl| {
        let bound = a.hull(b);
        Span {
            lo: bound.lo,
            hi: bound.hi,
        }
    };
    let [x, y, z] = a;
    let [u, v, w] = b;
    [hull(x, u), hull(y, v), hull(z, w)]
}

/// Half-open ray parity, after the whole rectangle is proved off the trim.
fn crosses([x, y]: [f64; 2], [a, b]: [Ivl; 2], [u, v]: [Ivl; 2]) -> Option<bool> {
    let above = |span: Ivl| {
        if span.lo > y {
            Some(true)
        } else if span.hi <= y {
            Some(false)
        } else {
            None
        }
    };
    let last_above = above(v)?;
    if above(b)? == last_above {
        return Some(false);
    }
    let side = u
        .sub_ulp(a)
        .mul_ulp(Ivl::from(y).sub_ulp(b))
        .sub_ulp(v.sub_ulp(b).mul_ulp(Ivl::from(x).sub_ulp(a)));
    excludes_zero(side).then_some((side.lo > 0.0) == last_above)
}

fn clear_rectangle(
    path: &Polygon,
    arm: Arm,
    plane: (usize, f64),
    axes: [usize; 2],
    inside: bool,
) -> Option<bool> {
    let [u, v] = axes;
    let (normal, value) = plane;
    let u_span = *arm.spans.get(u)?;
    let v_span = *arm.spans.get(v)?;
    let u_at = *arm.origin.get(u)?;
    let v_at = *arm.origin.get(v)?;
    let mut contained = false;
    let center = [u_span.middle()?, v_span.middle()?];
    for segments in path {
        let bounds = polygon_bounds(segments.iter().copied())?;
        // A closed loop lying outside the rectangle contributes neither a
        // boundary contact nor a winding at any point in the rectangle.
        if !bounds.get(u)?.overlaps(u_span) || !bounds.get(v)?.overlaps(v_span) {
            continue;
        }
        for &edge in segments {
            let [from, to] = edge;
            if *from.get(normal)? != Ivl::from(value) || *to.get(normal)? != Ivl::from(value) {
                return None;
            }
            let box_ = segment_bounds(edge);
            let ub = *box_.get(u)?;
            let vb = *box_.get(v)?;
            // In a rail rectangle allow the caps on the two short sides. In an
            // end rectangle allow the sides on the original support planes.
            let meets = if inside {
                ub.overlaps_open(u_span) && vb.overlaps_from(v_span, v_at)
            } else {
                ub.overlaps_from(u_span, u_at) && vb.overlaps_from(v_span, v_at)
            };
            if meets {
                return Some(false);
            }
            let a = [*from.get(u)?, *from.get(v)?];
            let z = [*to.get(u)?, *to.get(v)?];
            contained ^= crosses(center, a, z)?;
        }
    }
    Some(contained == inside)
}

fn clears_arm(
    k: &Kernel,
    b: &mut Brep,
    arm: Arm,
    far: Point3,
    placed: Option<&Placed>,
) -> Result<Option<bool>> {
    let [wall, floor] = arm.rail_faces;
    let [first_near, second_near] = arm.near_caps;
    let (Some(&side), Some(&height), Some(&near), Some(&far)) = (
        arm.origin.get(arm.across),
        arm.origin.get(arm.above),
        arm.origin.get(arm.along),
        coords(far).get(arm.along),
    ) else {
        return Ok(None);
    };
    for (face, plane, axes, inside) in [
        (wall, (arm.across, side), [arm.along, arm.above], true),
        (floor, (arm.above, height), [arm.along, arm.across], true),
        (
            first_near,
            (arm.along, near),
            [arm.across, arm.above],
            false,
        ),
        (
            second_near,
            (arm.along, near),
            [arm.across, arm.above],
            false,
        ),
        (
            arm.far_cap,
            (arm.along, far),
            [arm.across, arm.above],
            false,
        ),
    ] {
        let Some(path) = planar_polygon(k, b, face, TrimUse::Support, placed)? else {
            return Ok(None);
        };
        match clear_rectangle(&path, arm, plane, axes, inside) {
            Some(true) => {}
            other => return Ok(other),
        }
    }
    Ok(Some(true))
}

fn polygon_bounds(path: impl Iterator<Item = Segment>) -> Option<[Span; 3]> {
    let mut segments = path.map(segment_bounds);
    let mut bounds = segments.next()?;
    for segment in segments {
        for (span, next) in bounds.iter_mut().zip(segment) {
            span.lo = span.lo.min(next.lo);
            span.hi = span.hi.max(next.hi);
        }
    }
    Some(bounds)
}

/// The sweep must fit even at the closest possible end of the rail.
fn bounded_rail_end(start: Point3, end: PointBounds, width: f64) -> Option<Point3> {
    let mut nearest = [0.0; 3];
    for ((out, at), range) in nearest.iter_mut().zip(coords(start)).zip(end) {
        *out = if range == Ivl::from(at) {
            at
        } else if range.lo > at {
            range.lo
        } else if range.hi < at {
            range.hi
        } else {
            return None;
        };
    }
    let origin = coords(start);
    let (axis, positive) = cardinal(origin, nearest)?;
    let bounded = origin
        .get(axis)
        .zip(nearest.get(axis))
        .is_some_and(|(&start, &end)| {
            extended(start, positive, width).is_some_and(|limit| {
                if positive {
                    limit <= end
                } else {
                    limit >= end
                }
            })
        });
    let [x, y, z] = nearest;
    bounded.then_some(Point3::new(x, y, z))
}

/// Checks the two sweeps at their requested widths or the common limiting width.
///
/// This certificate covers cardinal sweeps bounded by planar polygons. Unsupported
/// geometry does not establish clearance or prevent the ordinary End construction.
pub(super) fn check_corridor(
    k: &Kernel,
    b: &mut Brep,
    c: &Corridor,
    [size_a, size_b]: [f64; 2],
) -> Result<Clearance> {
    let (vertex, at) = c.at;
    let placed = if resolved_vertex(b, vertex)? == Some(at) {
        None
    } else {
        let Some(placed) = Placed::read(k, b, c)? else {
            return Ok(Clearance::Unproven);
        };
        Some(placed)
    };
    let placed = placed.as_ref();
    let (at, size_a, size_b) = if let Some(placed) = placed {
        let (Some(at), Some(size_a), Some(size_b)) = (
            placed.point(b, vertex)?,
            placed.width(size_a),
            placed.width(size_b),
        ) else {
            return Ok(Clearance::Unproven);
        };
        (at, size_a, size_b)
    } else {
        (at, size_a, size_b)
    };
    let rail = |(vertex, _), width| -> Result<Option<Point3>> {
        let bound = if let Some(placed) = placed {
            placed.point(b, vertex)?.map(point_bounds)
        } else {
            vertex_bounds(b, vertex)?
        };
        Ok(bound.and_then(|end| bounded_rail_end(at, end, width)))
    };
    let [rim_a, rim_b, top] = c.ends;
    let (Some(rim_a), Some(rim_b), Some(top)) = (
        rail(rim_a, size_a)?,
        rail(rim_b, size_b)?,
        rail(top, size_a.max(size_b))?,
    ) else {
        return Ok(Clearance::Unproven);
    };
    let far = |vertex| {
        if let Some(placed) = placed {
            placed.point(b, vertex)
        } else {
            resolved_vertex(b, vertex)
        }
    };
    let [far_a, far_b] = c.far;
    let (Some(far_a), Some(far_b)) = (far(far_a)?, far(far_b)?) else {
        return Ok(Clearance::Unproven);
    };
    let [wall_a, wall_b, floor_b, opening_b, opening_a, floor_a] = c.faces;
    let [cap_a, cap_b] = c.far_caps;
    let (Some(a), Some(z)) = (
        Arm::new(
            at,
            rim_a,
            top,
            far_a,
            size_a,
            [wall_a, floor_a, wall_b, opening_a, cap_a],
        ),
        Arm::new(
            at,
            rim_b,
            top,
            far_b,
            size_b,
            [wall_b, floor_b, wall_a, opening_b, cap_b],
        ),
    ) else {
        return Ok(Clearance::Unproven);
    };
    let mut clear = true;
    for (arm, end) in [(a, far_a), (z, far_b)] {
        match clears_arm(k, b, arm, end, placed)? {
            Some(true) => {}
            Some(false) => return Ok(Clearance::Obstructed),
            None => clear = false,
        }
    }

    let mut faces = Vec::new();
    faces
        .try_reserve_exact(b.face_list().len())
        .map_err(|_| SolidError::topology(OP, TopologyFailure::AllocationFailed))?;
    faces.extend_from_slice(b.face_list());
    for face in faces {
        k.check_cancelled(OP)?;
        let mut look_a = !a.permits_contact(face);
        let mut look_z = !z.permits_contact(face);
        if !look_a && !look_z {
            continue;
        }
        let level = if placed.is_some() {
            None
        } else {
            plane_level(b, face)?
        };
        if let Some((axis, coordinate)) = level {
            let meets = |span: Option<&Span>| {
                span.is_none_or(|span| span.lo <= coordinate && coordinate <= span.hi)
            };
            look_a &= meets(a.spans.get(axis));
            look_z &= meets(z.spans.get(axis));
        }
        if !look_a && !look_z {
            continue;
        }
        let Some(bounds) = planar_polygon(k, b, face, TrimUse::OtherFace, placed)?
            .and_then(|loops| polygon_bounds(loops.into_iter().flatten()))
        else {
            clear = false;
            continue;
        };
        for (look, arm, opposite_opening) in [(look_a, a, opening_b), (look_z, z, opening_a)] {
            if !look {
                continue;
            }
            let contact = if let Some(placed) = placed {
                placed.foreign_contact(arm, bounds, face == opposite_opening)
            } else {
                arm.contacts(bounds)
            };
            match contact {
                Some(true) => return Ok(Clearance::Obstructed),
                Some(false) => {}
                None => {
                    clear = false;
                }
            }
        }
    }
    if !clear {
        return Ok(Clearance::Unproven);
    }
    let span = if let Some(placed) = placed {
        let (Some(a), Some(z)) = (placed.extent(a), placed.extent(z)) else {
            return Ok(Clearance::Unproven);
        };
        a.union(&z)
    } else {
        Extent3d::from_points(at, far_a).union(&Extent3d::from_point(far_b))
    };
    Ok(Clearance::Clear(span))
}

fn plane_level(b: &Brep, face: FaceId) -> Result<Option<(usize, f64)>> {
    let Some(SurfaceKind::Plane(plane)) = face2::surface(b, face)?.map(Surface::kind) else {
        return Ok(None);
    };
    let (origin, normal, _) = plane.is_planar();
    Ok(cardinal([0.0; 3], coords(normal)).and_then(|(axis, _)| {
        coords(origin)
            .get(axis)
            .copied()
            .filter(|c| c.is_finite())
            .map(|c| (axis, c))
    }))
}
