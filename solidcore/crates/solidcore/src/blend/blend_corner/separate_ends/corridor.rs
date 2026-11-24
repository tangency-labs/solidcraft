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
use crate::math::{Point2, Point3, Vec3};
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::math::{CPoint, NOW};
use crate::surface::plane::Plane;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::brep::p5::initial_stitch_tolerance;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::edgeuse::p3::get_or_create_uv_trim_curve;
use crate::topology::face::p2 as face2;
use crate::topology::face::p4 as face4;
use crate::topology::{
    edge, loopuse, vertex, Brep, EdgeId, EdgeuseId, FaceId, LoopuseId, VertexId,
};

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

struct Polygon {
    loops: Vec<Vec<BoundarySegment>>,
    lifted: Option<[Span; 3]>,
}

struct BoundarySegment {
    nominal: Segment,
    lifted: Option<Segment>,
}

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

    fn extent(self, placed: Option<&Placed>) -> Option<Extent3d> {
        if let Some(placed) = placed {
            return placed.extent(self);
        }
        bounds_extent(self.spans.map(|span| Ivl::new(span.lo, span.hi)))
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

fn bounds_extent([x, y, z]: PointBounds) -> Option<Extent3d> {
    [x, y, z].into_iter().all(Ivl::is_finite).then(|| {
        Extent3d::from_points(Point3::new(x.lo, y.lo, z.lo), Point3::new(x.hi, y.hi, z.hi))
    })
}

fn with_zone(extent: &Extent3d, zone: f64) -> Option<Extent3d> {
    if !extent.is_bounded() || !zone.is_finite() || zone < 0.0 {
        return None;
    }
    let margin = Ivl::new(-zone, zone);
    let [x, y, z] = coords(extent.min());
    let [u, v, w] = coords(extent.max());
    bounds_extent([(x, u), (y, v), (z, w)].map(|(lo, hi)| Ivl::new(lo, hi).add_ulp(margin)))
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
    Support { arm: Arm, face: FaceId },
    OtherFace,
}

struct TrimSegment {
    nominal: Segment,
    start: LoopEnd,
    end: LoopEnd,
    lifted: Option<Segment>,
}

/// Bind a straight polygon segment to the edge's 3D line and lifted UV trim.
fn segment(
    k: &Kernel,
    b: &Brep,
    eu: EdgeuseId,
    plane: &Plane,
    use_: TrimUse,
    placed: Option<&Placed>,
) -> Result<Option<TrimSegment>> {
    let Some(e) = eu1::edge(b, eu)? else {
        return Ok(None);
    };
    let Some(CurveKind::Line(line)) = edge::curve(b, e)?.map(Curve::kind) else {
        return Ok(None);
    };
    let (Some(u), Some(v)) = edge::start_end_vertices(b, e)? else {
        return Ok(None);
    };
    let (a, z, lifted) = if let Some(placed) = placed {
        let (Some(a), Some(z)) = (placed.point(b, u)?, placed.point(b, v)?) else {
            return Ok(None);
        };
        let Some(lifted) = placed.bound_trim(b, eu, e, plane, line, [u, v], [a, z])? else {
            return Ok(None);
        };
        let lifted = match use_ {
            TrimUse::Support { arm, face } => {
                let Some(lifted) = placed.support_trim(
                    k,
                    b,
                    arm,
                    (face, eu, e, line),
                    [(u, a), (v, z)],
                    lifted,
                )?
                else {
                    return Ok(None);
                };
                Some(lifted)
            }
            // The placed foreign check already budgets both zones.
            TrimUse::OtherFace => None,
        };
        (point_bounds(a), point_bounds(z), lifted)
    } else {
        let (Some(a), Some(z)) = (vertex_bounds(b, u)?, vertex_bounds(b, v)?) else {
            return Ok(None);
        };
        let cardinal_bounds = match (resolved_vertex(b, u)?, resolved_vertex(b, v)?) {
            (Some(a), Some(z)) => cardinal(coords(a), coords(z)),
            _ => None,
        };
        let other_face = matches!(use_, TrimUse::OtherFace);
        let exact = if let Some((along, _)) = cardinal_bounds {
            binds_cardinal_trim(b, eu, e, plane, line, [u, v], [a, z], along)?
        } else {
            false
        };
        let lifted = if exact && !other_face {
            None
        } else if exact {
            let Some(bounds) = trim::lifted_trim(b, eu, &edge::interval(b, e)?, plane)? else {
                return Ok(None);
            };
            Some(bounds)
        } else if other_face || cardinal_bounds.is_none() {
            let Some(bounds) =
                trim::bound_affine_trim(b, eu, e, plane, line, [u, v], [a, z], f64::INFINITY)?
            else {
                return Ok(None);
            };
            Some(bounds)
        } else {
            return Ok(None);
        };
        (a, z, lifted)
    };
    let (u, v, direction) = match eu1::orientation(b, eu)? {
        OrientType::Same => (u, v, subtract(z, a)),
        OrientType::Opposite => (v, u, subtract(a, z)),
        OrientType::Unknown => return Ok(None),
    };
    let lifted = if let (TrimUse::OtherFace, Some([from, to])) = (use_, lifted) {
        // Close only the arithmetic difference where the certified trim and
        // its independent carrier intersection describe the same point.
        let snap = |[fx, fy, fz]: PointBounds, [x, y, z]: PointBounds, roundoff| {
            [(fx, x), (fy, y), (fz, z)].map(|(actual, nominal)| {
                if nominal.lo == nominal.hi
                    && enclosed_difference(actual, nominal).is_some_and(|gap| gap.mag() <= roundoff)
                {
                    nominal
                } else {
                    actual
                }
            })
        };
        Some(
            line_roundoff(line, &edge::interval(b, e)?).map_or([from, to], |roundoff| {
                [snap(from, a, roundoff), snap(to, z, roundoff)]
            }),
        )
    } else {
        lifted
    };
    Ok(Some(TrimSegment {
        nominal: [a, z],
        start: LoopEnd {
            vertex: u,
            direction,
        },
        end: LoopEnd {
            vertex: v,
            direction,
        },
        lifted,
    }))
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
    loops: &[LoopuseId],
    plane: &Plane,
    use_: TrimUse,
    placed: Option<&Placed>,
) -> Result<Option<Polygon>> {
    let mut polygon = Vec::new();
    let mut lifted = None;
    polygon
        .try_reserve_exact(loops.len())
        .map_err(|_| SolidError::topology(OP, TopologyFailure::AllocationFailed))?;
    for &lu in loops {
        let mut segments = Vec::new();
        let uses = loopuse::edgeuses(b, lu, None, None)?;
        segments
            .try_reserve_exact(uses.len())
            .map_err(|_| SolidError::topology(OP, TopologyFailure::AllocationFailed))?;
        let mut first = None;
        let mut last: Option<LoopEnd> = None;
        for eu in uses {
            k.check_cancelled(OP)?;
            let Some(TrimSegment {
                nominal,
                start,
                end,
                lifted: actual,
            }) = segment(k, b, eu, plane, use_, placed)?
            else {
                return Ok(None);
            };
            if last.is_some_and(|prev| !prev.joins(start)) {
                return Ok(None);
            }
            first.get_or_insert(start);
            last = Some(end);
            segments.push(BoundarySegment {
                nominal,
                lifted: actual,
            });
            if let Some(actual) = actual {
                let bounds = segment_bounds(actual);
                lifted = Some(lifted.map_or(bounds, |other| merge_spans(other, bounds)));
            }
        }
        if !matches!((last, first), (Some(last), Some(first)) if last.joins(first)) {
            return Ok(None);
        }
        polygon.push(segments);
    }
    Ok((!polygon.is_empty()).then_some(Polygon {
        loops: polygon,
        lifted,
    }))
}

/// A distant inner trim loop cannot change the winding within the corridor.
fn distant_loop(
    k: &Kernel,
    b: &Brep,
    lu: LoopuseId,
    plane: &Plane,
    envelope: &Extent3d,
) -> Result<bool> {
    let mut total: Option<Extent3d> = None;
    for eu in loopuse::edgeuses(b, lu, None, None)? {
        k.check_cancelled(OP)?;
        let (Some(edge), (Some(uv), _)) = (eu1::edge(b, eu)?, eu1::uv_trim_curve(b, eu)?) else {
            return Ok(false);
        };
        let Some(curve) = edge::curve(b, edge)? else {
            return Ok(false);
        };
        let (Some(spatial), Some(param)) = (
            curve.certified_bounding_box(k, &edge::interval(b, edge)?)?,
            uv.certified_bounding_box(k, &uv.natural_interval())?,
        ) else {
            return Ok(false);
        };
        let [x, y] = [
            Ivl::new(param.min().x, param.max().x),
            Ivl::new(param.min().y, param.max().y),
        ];
        let [lifted, _] = trim::uv_bounds(plane, [[x, y]; 2]);
        let Some(lifted) = bounds_extent(lifted) else {
            return Ok(false);
        };
        let zone = edge::tolerance(b, edge)?.max(b.tolerance());
        let Some(bounds) = with_zone(&spatial.union(&lifted), zone) else {
            return Ok(false);
        };
        total = Some(total.map_or(bounds, |other| other.union(&bounds)));
    }
    Ok(total.is_some_and(|bounds| bounds.are_disjoint(envelope, 0.0)))
}

fn planar_polygon(
    k: &Kernel,
    b: &mut Brep,
    face: FaceId,
    use_: TrimUse,
    placed: Option<&Placed>,
    envelope: &Extent3d,
) -> Result<Option<Polygon>> {
    let Some(SurfaceKind::Plane(plane)) = face2::surface(b, face)?.map(Surface::kind) else {
        return Ok(None);
    };
    let outer = face2::outer_loops(b, face)?;
    if outer.is_empty() {
        return Ok(None);
    }
    let loops = face4::upward_loopuses(b, face, false, None)?;
    let mut nearby = Vec::new();
    nearby
        .try_reserve_exact(loops.len())
        .map_err(|_| SolidError::topology(OP, TopologyFailure::AllocationFailed))?;
    for lu in loops {
        if loopuse::loop_(b, lu)?.is_some_and(|lp| !outer.contains(&lp))
            && distant_loop(k, b, lu, plane, envelope)?
        {
            continue;
        }
        nearby.push(lu);
    }
    for &lu in &nearby {
        for eu in loopuse::edgeuses(b, lu, None, None)? {
            k.check_cancelled(OP)?;
            if eu1::uv_trim_curve(b, eu)?.0.is_none() && !certifiable_lazy_trim(k, b, eu)? {
                return Ok(None);
            }
        }
    }
    let Some(SurfaceKind::Plane(plane)) = face2::surface(b, face)?.map(Surface::kind) else {
        return Ok(None);
    };
    segments(k, b, &nearby, plane, use_, placed)
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
    let side = line_side([x, y], [[a, b], [u, v]]);
    excludes_zero(side).then_some((side.lo > 0.0) == last_above)
}

fn line_side([x, y]: [f64; 2], [[a, b], [u, v]]: [[Ivl; 2]; 2]) -> Ivl {
    u.sub_ulp(a)
        .mul_ulp(Ivl::from(y).sub_ulp(b))
        .sub_ulp(v.sub_ulp(b).mul_ulp(Ivl::from(x).sub_ulp(a)))
}

/// Test the segment after its box passes the half-open rectangle filter.
fn segment_contacts_rectangle(
    segment @ [a, z]: [[Ivl; 2]; 2],
    spans @ [u, v]: [Span; 2],
    origins: [Option<f64>; 2],
) -> Option<bool> {
    let admitted = |point: [Ivl; 2]| {
        point
            .into_iter()
            .zip(spans)
            .zip(origins)
            .all(|((p, s), at)| {
                (p.lo > s.lo || (at == Some(s.hi) && p.lo == s.lo))
                    && (p.hi < s.hi || (at == Some(s.lo) && p.hi == s.hi))
            })
    };
    if segment.into_iter().any(admitted) {
        return Some(true);
    }
    let corners = [[u.lo, v.lo], [u.lo, v.hi], [u.hi, v.lo], [u.hi, v.hi]];
    let sides = corners.map(|p| line_side(p, segment));
    let positive = |s: &Ivl| s.lo > 0.0;
    let negative = |s: &Ivl| s.hi < 0.0;
    if sides.iter().all(positive) || sides.iter().all(negative) {
        return Some(false);
    }
    let exact_line =
        |[x, y]: [Ivl; 2]| (x.lo == x.hi && y.lo == y.hi).then_some(Vec3::new(x.lo, y.lo, 1.0));
    let reaches_corner = corners.into_iter().zip(sides).any(|(p, side)| {
        let [x, y] = p;
        admitted(p.map(Ivl::from))
            && a.into_iter()
                .zip(z)
                .zip(p)
                .all(|((a, z), p)| a.hi.min(z.hi) <= p && a.lo.max(z.lo) >= p)
            && (side == Ivl::from(0.0)
                || (side.lo <= 0.0
                    && side.hi >= 0.0
                    && exact_line(a).zip(exact_line(z)).is_some_and(|(a, z)| {
                        expansion::det3(a, z, Vec3::new(x, y, 1.0))
                            .is_some_and(|terms| expansion::is_zero(&terms))
                    })))
    });
    // The rectangle axes and the segment normal separate two disjoint
    // convex sets. Strict overlap on every axis proves interior contact.
    let overlaps_interior = a
        .into_iter()
        .zip(z)
        .zip(spans)
        .all(|((a, z), span)| a.hi.min(z.hi) < span.hi && a.lo.max(z.lo) > span.lo);
    (reaches_corner
        || (overlaps_interior && sides.iter().any(positive) && sides.iter().any(negative)))
    .then_some(true)
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
    let rectangle = [u_span, v_span];
    let open_sides = [(!inside).then_some(u_at), Some(v_at)];
    let can_contact = |edge| {
        let bounds = segment_bounds(edge);
        let ub = *bounds.get(u)?;
        let vb = *bounds.get(v)?;
        // Allow both caps on a rail rectangle. On an end rectangle
        // allow the sides on the original support planes.
        Some(if inside {
            ub.overlaps_open(u_span) && vb.overlaps_from(v_span, v_at)
        } else {
            ub.overlaps_from(u_span, u_at) && vb.overlaps_from(v_span, v_at)
        })
    };
    let project = |at: PointBounds| Some([*at.get(u)?, *at.get(v)?]);
    for segments in &path.loops {
        for segment in segments {
            if let Some([a, z]) = segment.lifted {
                let [first, last] = segment.nominal;
                // Enclose the whole motion from the carrier to the lifted trim.
                // A possible crossing leaves the support's winding unproved.
                let hull = |[x, y, z]: PointBounds, [a, b, c]: PointBounds| {
                    [x.hull(a), y.hull(b), z.hull(c)]
                };
                let [a, z] = [hull(first, a), hull(last, z)];
                if can_contact([a, z])?
                    && segment_contacts_rectangle(
                        [project(a)?, project(z)?],
                        rectangle,
                        open_sides,
                    )?
                {
                    return None;
                }
            }
        }
        let bounds = polygon_bounds(segments.iter().map(|segment| segment.nominal))?;
        // A closed loop lying outside the rectangle contributes neither a
        // boundary contact nor a winding at any point in the rectangle.
        if !bounds.get(u)?.overlaps(u_span) || !bounds.get(v)?.overlaps(v_span) {
            continue;
        }
        for segment in segments {
            let edge = segment.nominal;
            let [from, to] = edge;
            if *from.get(normal)? != Ivl::from(value) || *to.get(normal)? != Ivl::from(value) {
                return None;
            }
            let (a, z) = (project(from)?, project(to)?);
            if can_contact(edge)? && segment_contacts_rectangle([a, z], rectangle, open_sides)? {
                return Some(false);
            }
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
    extent: &Extent3d,
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
        let Some(path) =
            planar_polygon(k, b, face, TrimUse::Support { arm, face }, placed, extent)?
        else {
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
    let bounds = segments.next()?;
    Some(segments.fold(bounds, merge_spans))
}

fn merge_spans(mut first: [Span; 3], second: [Span; 3]) -> [Span; 3] {
    for (span, next) in first.iter_mut().zip(second) {
        span.lo = span.lo.min(next.lo);
        span.hi = span.hi.max(next.hi);
    }
    first
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

fn enclosed_difference(a: Ivl, z: Ivl) -> Option<Ivl> {
    let low = expansion_span(&expansion::sub(&[a.lo], &[z.hi]))?;
    let high = expansion_span(&expansion::sub(&[a.hi], &[z.lo]))?;
    Some(Ivl::new(low.lo, high.hi))
}

fn line_roundoff(line: &Line, interval: &crate::geom::extent1d::Extent1d) -> Option<f64> {
    let first = line.point_error(interval.min())?;
    let last = line.point_error(interval.max())?;
    let bound = Ivl::from(first).add_ulp(Ivl::from(last)).hi;
    bound.is_finite().then_some(bound)
}

/// At least one bound trim must reach the independent far plane. A near end
/// before the corner still covers it. A shorter trim agrees with the rounded
/// rail only if it has the same recorded span at both ends.
fn reconciles_rail_ends(spatial: [Ivl; 2], trims: [[Ivl; 2]; 2], ideal: Ivl) -> bool {
    let [near, far] = spatial;
    let independent = |[start, end]: [Ivl; 2]| {
        let reaches = start.hi <= 0.0 && end.lo >= ideal.hi;
        let encloses_shared_ends = start.lo <= near.lo.min(0.0)
            && start.hi >= near.hi.max(0.0)
            && end.lo <= far.lo.min(ideal.lo)
            && end.hi >= far.hi.max(ideal.hi);
        reaches || encloses_shared_ends
    };
    trims.into_iter().any(independent)
        && trims
            .into_iter()
            .all(|trim| trim == spatial || independent(trim))
}

/// Reach from the corner, limited by the bounded 3D edge and both lifted trims.
pub(super) fn rail_reach(
    k: &Kernel,
    b: &mut Brep,
    rail: EdgeId,
    start: Point3,
    end: Point3,
) -> Result<Option<Ivl>> {
    let faces = edge::faces(b, rail, None)?;
    let &[first_face, last_face] = faces.as_slice() else {
        return Ok(None);
    };
    let occurrence = |face| -> Result<Option<EdgeuseId>> {
        for eu in face4::upward_edgeuses(b, face, false, None)? {
            if eu1::edge(b, eu)? == Some(rail) {
                return Ok(Some(eu));
            }
        }
        Ok(None)
    };
    let (Some(a), Some(z)) = (occurrence(first_face)?, occurrence(last_face)?) else {
        return Ok(None);
    };
    for eu in [a, z] {
        if eu1::uv_trim_curve(b, eu)?.0.is_none() && !certifiable_lazy_trim(k, b, eu)? {
            return Ok(None);
        }
    }
    let Some(CurveKind::Line(line)) = edge::curve(b, rail)?.map(Curve::kind) else {
        return Ok(None);
    };
    let interval = edge::interval(b, rail)?;
    let lift = |eu| -> Result<Option<Segment>> {
        let Some(face) = eu1::face(b, eu)? else {
            return Ok(None);
        };
        let Some(SurfaceKind::Plane(plane)) = face2::surface(b, face)?.map(Surface::kind) else {
            return Ok(None);
        };
        trim::lifted_trim(b, eu, &interval, plane)
    };
    let endpoints = [trim::line_endpoints(line, &interval), lift(a)?, lift(z)?];
    let [Some(spatial), Some(a), Some(z)] = endpoints else {
        return Ok(None);
    };
    let mut direction = line.line_vector();
    let from = point_bounds(start);
    let points_toward = dot(subtract(point_bounds(end), from), point_bounds(direction));
    if !excludes_zero(points_toward) {
        return Ok(None);
    }
    if points_toward.hi < 0.0 {
        direction = -direction;
    }
    let axis = cardinal([0.0; 3], coords(direction));
    let normal = point_bounds(direction);
    let length = dot(normal, normal).sqrt();
    let along = |point: PointBounds| {
        if let Some((axis, positive)) = axis {
            let p = *point.get(axis)?;
            let at = *from.get(axis)?;
            if positive {
                enclosed_difference(p, at)
            } else {
                enclosed_difference(at, p)
            }
        } else {
            Some(dot(subtract(point, from), normal).div_ulp(length))
        }
    };
    let ordered = |[first, last]: Segment| {
        let (first, last) = (along(first)?, along(last)?);
        if first.hi < last.lo {
            Some([first, last])
        } else {
            (last.hi < first.lo).then_some([last, first])
        }
    };
    let [Some(spatial), Some(a), Some(z)] = [spatial, a, z].map(ordered) else {
        return Ok(None);
    };
    let reach = [spatial, a, z]
        .into_iter()
        .try_fold(None, |common, [near, far]| {
            let span = enclosed_difference(far, near)?;
            let reach = Ivl::new(span.lo.min(far.lo), span.hi.min(far.hi));
            let common = common.map_or(reach, |other: Ivl| {
                Ivl::new(other.lo.min(reach.lo), other.hi.min(reach.hi))
            });
            (common.is_finite() && common.lo > 0.0).then_some(Some(common))
        });
    let mut reach = reach.flatten();
    if let (Some(actual), Some(_), Some(ideal), Some(roundoff)) = (
        reach,
        axis,
        along(point_bounds(end)),
        line_roundoff(line, &interval),
    ) {
        // Reconcile a glued support with the transverse plane intersections
        // only when the independent trim enclosures agree with that support.
        // An independently shortened trim retains its own bound.
        if ideal.lo == ideal.hi
            && enclosed_difference(actual, ideal).is_some_and(|gap| gap.mag() <= roundoff)
            && reconciles_rail_ends(spatial, [a, z], ideal)
        {
            reach = Some(ideal);
        }
    }
    Ok(reach)
}

/// Largest physical width up to `maximum` whose padded placed sweep fits the rail.
pub(super) fn placed_rail_limit(
    k: &Kernel,
    b: &Brep,
    corridor: &Corridor,
    end: VertexId,
    maximum: f64,
) -> Result<Option<f64>> {
    let (start, world) = corridor.at;
    if resolved_vertex(b, start)? == Some(world) {
        return Ok(Some(maximum));
    }
    let Some(placed) = Placed::read(k, b, corridor)? else {
        return Ok(None);
    };
    let (Some(start), Some(end)) = (placed.point(b, start)?, placed.point(b, end)?) else {
        return Ok(None);
    };
    let Some((axis, positive)) = cardinal(coords(start), coords(end)) else {
        return Ok(None);
    };
    let mut end_bounds = point_bounds(end);
    let (Some(&start_level), Some(end_level), Some(margin)) = (
        coords(start).get(axis),
        end_bounds.get_mut(axis),
        placed.foreign_margin(),
    ) else {
        return Ok(None);
    };
    // A foreign boundary at the stopping level also has a zone. Leave room
    // for its enclosure beyond the already padded sweep, with strict clearance.
    let zone = end_level.add_ulp(Ivl::new(-margin, margin));
    let usable_end = if positive {
        next_down(zone.lo)
    } else {
        next_up(zone.hi)
    };
    if !usable_end.is_finite()
        || (if positive {
            usable_end <= start_level
        } else {
            usable_end >= start_level
        })
    {
        return Ok(None);
    }
    *end_level = Ivl::from(usable_end);
    // Leave the new rail outside the initial stitch that follows construction.
    // This additional clearance applies to the suggested width, not to the
    // obstruction test for an unrelated face.
    let stitch = Ivl::from(initial_stitch_tolerance(b.tolerance(), 0.0));
    let fits = |size| {
        let with_stitch = Ivl::from(size).add_ulp(stitch);
        placed
            .width(with_stitch.hi)
            .is_some_and(|width| bounded_rail_end(start, end_bounds, width).is_some())
    };
    if fits(maximum) {
        return Ok(Some(maximum));
    }
    let (mut lo, mut hi) = (0, maximum.to_bits());
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if fits(f64::from_bits(mid)) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Ok((lo != 0).then(|| f64::from_bits(lo)))
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
    let (Some(a_extent), Some(z_extent)) = (a.extent(placed), z.extent(placed)) else {
        return Ok(Clearance::Unproven);
    };
    let envelope = a_extent.union(&z_extent);
    let mut clear = true;
    for (arm, end, extent) in [(a, far_a, &a_extent), (z, far_b, &z_extent)] {
        match clears_arm(k, b, arm, end, placed, extent)? {
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
        let Some(surface) = face2::surface(b, face)? else {
            clear = false;
            continue;
        };
        if !matches!(surface.kind(), SurfaceKind::Plane(_)) {
            let zone = face2::tolerance(b, face)?.max(b.tolerance());
            let bounds = surface.certified_bounding_box(k, &face2::uv_domain(b, face)?)?;
            if !bounds
                .and_then(|bounds| with_zone(&bounds, zone))
                .is_some_and(|bounds| {
                    (!look_a || bounds.are_disjoint(&a_extent, 0.0))
                        && (!look_z || bounds.are_disjoint(&z_extent, 0.0))
                })
            {
                clear = false;
            }
            continue;
        }
        let Some(path) = planar_polygon(k, b, face, TrimUse::OtherFace, placed, &envelope)? else {
            clear = false;
            continue;
        };
        let Some(bounds) = polygon_bounds(
            path.loops
                .into_iter()
                .flatten()
                .map(|segment| segment.nominal),
        ) else {
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
                Some(false) => {
                    if path
                        .lifted
                        .is_some_and(|bounds| arm.contacts(bounds) != Some(false))
                    {
                        clear = false;
                    }
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_oblique_trim_needs_more_than_box_overlap_to_obstruct() {
        let test = |segment: [[f64; 2]; 2]| {
            segment_contacts_rectangle(
                segment.map(|end| end.map(Ivl::from)),
                [Span::between(0.0, 1.0); 2],
                [Some(0.0); 2],
            )
        };
        for (mut segment, expected) in [
            ([[0.5, 2.0], [2.0, 0.5]], false),
            ([[0.5, 1.5], [1.5, 0.5]], true),
        ] {
            assert_eq!(test(segment), Some(expected));
            segment.reverse();
            assert_eq!(test(segment), Some(expected));
        }
        assert_eq!(test([[-1.0, 0.5], [2.0, 0.5]]), Some(true));
        assert_eq!(
            segment_contacts_rectangle(
                [
                    [Ivl::from(0.5), Ivl::new(1.25, 2.0)],
                    [Ivl::from(1.5), Ivl::from(0.5)]
                ],
                [Span::between(0.0, 1.0); 2],
                [Some(0.0); 2],
            ),
            None
        );
    }
    #[test]
    fn an_oblique_carrier_that_misses_the_cap_cannot_hide_a_lifted_trim_entering_it() {
        let arm = Arm::new(
            Point3::ZERO,
            Point3::X,
            Point3::Y,
            Point3::Z,
            1.0,
            [FaceId::from_index(0); 5],
        )
        .unwrap();
        let point = |x, y| point_bounds(Point3::new(x, y, 0.0));
        let delta = 1e-9;
        let a = point(0.5, 1.5 + delta);
        let z = point(1.5 + delta, 0.5);
        let far = point(2.0, 2.0);
        let cap = |lifted| Polygon {
            loops: vec![vec![
                BoundarySegment {
                    nominal: [a, z],
                    lifted,
                },
                BoundarySegment {
                    nominal: [z, far],
                    lifted: None,
                },
                BoundarySegment {
                    nominal: [far, a],
                    lifted: None,
                },
            ]],
            lifted: None,
        };
        assert_eq!(
            clear_rectangle(&cap(None), arm, (2, 0.0), [0, 1], false),
            Some(true)
        );
        let lifted = Some([point(0.5, 1.5 - delta), point(1.5 - delta, 0.5)]);
        assert_eq!(
            clear_rectangle(&cap(lifted), arm, (2, 0.0), [0, 1], false),
            None
        );
    }
    #[test]
    fn a_glued_rail_can_start_just_before_the_corner() {
        let near = Ivl::from(-3.552713678800501e-15);
        let ideal = Ivl::from(5.0);
        let rounded = [near, Ivl::from(4.999999999999998)];
        let at_plane = [near, ideal];
        let enclosed = [
            Ivl::new(-5.329070518200751e-15, 1.7763568394002505e-15),
            Ivl::new(4.9999999999999964, 5.0000000000000036),
        ];
        assert!(reconciles_rail_ends(rounded, [at_plane, enclosed], ideal));
        assert!(reconciles_rail_ends(rounded, [at_plane, rounded], ideal));
        assert!(!reconciles_rail_ends(at_plane, [at_plane, rounded], ideal));

        let zero = Ivl::from(0.0);
        let bits = 5.0_f64.to_bits();
        let spatial = [zero, Ivl::from(f64::from_bits(bits - 4))];
        let might_reach = [zero, Ivl::new(f64::from_bits(bits - 2), 5.0)];
        assert!(!reconciles_rail_ends(
            spatial,
            [spatial, might_reach],
            ideal
        ));
    }
}
