//! A geometry-only room certificate for straight, cardinal separate ends.
//!
//! The selected edge and the support normal rays define a rectangular prism containing
//! either blend sweep. The two rail rectangles are inside their faces, the end rectangles
//! are outside the cap trims, and no other boundary reaches the portion being changed.

use crate::curve::curve::{Curve, CurveKind};
use crate::error::Result;
use crate::geom::certified::distance;
use crate::geom::expansion;
use crate::geom::extent3d::Extent3d;
use crate::kernel::Kernel;
use crate::math::Point3;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::face::p2 as face2;
use crate::topology::topology_traverser::face_edges;
use crate::topology::{edge, vertex, Brep, FaceId, VertexId};

const OP: &str = "blend::blend_corner::separate_ends::corridor";

pub(super) struct Corridor {
    pub(super) at: (VertexId, Point3),
    pub(super) ends: [(VertexId, Point3); 3],
    pub(super) far: [VertexId; 2],
    pub(super) faces: [FaceId; 6],
    pub(super) far_caps: [FaceId; 2],
}

type Coords = [f64; 3];
type Segment = [Coords; 2];

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

fn extended(value: f64, positive: bool, size: f64) -> Option<f64> {
    let delta = if positive { size } else { -size };
    let end = value + delta;
    (end.is_finite()
        && end != value
        && expansion::is_zero(&expansion::sub(&expansion::add(&[value], &[delta]), &[end])))
    .then_some(end)
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

/// As in `band_room`, use the segment between the vertices of a straight trim.
/// Resolve them from incident planes so each vertex has a consistent position
/// in both the cap and support faces.
fn segments(k: &Kernel, b: &Brep, face: FaceId) -> Result<Option<Vec<Segment>>> {
    let mut result = Vec::new();
    for e in face_edges(b, face)? {
        k.check_cancelled(OP)?;
        if !matches!(
            edge::curve(b, e)?.map(Curve::kind),
            Some(CurveKind::Line(_))
        ) {
            return Ok(None);
        }
        let (Some(u), Some(v)) = edge::start_end_vertices(b, e)? else {
            return Ok(None);
        };
        let (Some(a), Some(z)) = (resolved_vertex(b, u)?, resolved_vertex(b, v)?) else {
            return Ok(None);
        };
        let [a, z] = [coords(a), coords(z)];
        if cardinal(a, z).is_none() {
            return Ok(None);
        }
        result.push([a, z]);
    }
    Ok((!result.is_empty()).then_some(result))
}

fn planar_polygon(k: &Kernel, b: &Brep, face: FaceId) -> Result<Option<Vec<Segment>>> {
    if !matches!(
        face2::surface(b, face)?.map(Surface::kind),
        Some(SurfaceKind::Plane(_))
    ) || face2::outer_loops(b, face)?.is_empty()
    {
        return Ok(None);
    }
    segments(k, b, face)
}

fn segment_bounds([a, b]: Segment) -> [Span; 3] {
    let [x, y, z] = a;
    let [u, v, w] = b;
    [
        Span::between(x, u),
        Span::between(y, v),
        Span::between(z, w),
    ]
}

/// The parity of a horizontal ray from a point off the trim. This is used
/// only after the entire rectangle has been proved to miss every trim, so
/// it gives the side of the whole connected rectangle.
fn crosses([x, y]: [f64; 2], [a, b]: [f64; 2], [u, v]: [f64; 2]) -> Option<bool> {
    if (b > y) == (v > y) {
        return Some(false);
    }
    (a == u && a != x).then_some(a > x)
}

fn clear_rectangle(
    path: &[Segment],
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
    for &edge in path {
        let [from, to] = edge;
        if *from.get(normal)? != value || *to.get(normal)? != value {
            return Some(false);
        }
        let box_ = segment_bounds(edge);
        let ub = *box_.get(u)?;
        let vb = *box_.get(v)?;
        // In a rail rectangle allow the caps on the two short sides. In an
        // end rectangle allow the sides on the original two support planes.
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
    Some(contained == inside)
}

fn certifies_arm(k: &Kernel, b: &Brep, arm: Arm, far: Point3) -> Result<bool> {
    let [wall, floor] = arm.rail_faces;
    let [first_near, second_near] = arm.near_caps;
    let (Some(&side), Some(&height), Some(&near), Some(&far)) = (
        arm.origin.get(arm.across),
        arm.origin.get(arm.above),
        arm.origin.get(arm.along),
        coords(far).get(arm.along),
    ) else {
        return Ok(false);
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
        let Some(path) = planar_polygon(k, b, face)? else {
            return Ok(false);
        };
        if clear_rectangle(&path, arm, plane, axes, inside) != Some(true) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn polygon_bounds(path: &[Segment]) -> Option<[Span; 3]> {
    let mut segments = path.iter().copied().map(segment_bounds);
    let mut bounds = segments.next()?;
    for segment in segments {
        for (span, next) in bounds.iter_mut().zip(segment) {
            span.lo = span.lo.min(next.lo);
            span.hi = span.hi.max(next.hi);
        }
    }
    Some(bounds)
}

/// Certifies room at a common limiting size and returns the span of the picks.
/// The caller has already qualified its fan and identified a true rim or post
/// length that supplies the upper bound, and checks interactions across pairs.
///
/// This additional certificate is deliberately limited to planar polygons with
/// cardinal trims. The ordinary correctly sized construction remains available
/// for other geometry.
pub(super) fn certified_limit_span(
    k: &Kernel,
    b: &Brep,
    c: &Corridor,
    size: f64,
) -> Result<Option<Extent3d>> {
    let [wall_a, wall_b, floor_b, opening_b, opening_a, floor_a] = c.faces;
    let (vertex, at) = c.at;
    for (vertex, stored) in std::iter::once((vertex, at)).chain(c.ends) {
        // The rails used to bound the size must agree with the planes.
        if resolved_vertex(b, vertex)? != Some(stored) {
            return Ok(None);
        }
    }
    let [(_, rim_a), (_, rim_b), (_, top)] = c.ends;
    let [far_a, far_b] = c.far;
    let [cap_a, cap_b] = c.far_caps;
    let (Some(far_a), Some(far_b)) = (resolved_vertex(b, far_a)?, resolved_vertex(b, far_b)?)
    else {
        return Ok(None);
    };
    let (Some(a), Some(z)) = (
        Arm::new(
            at,
            rim_a,
            top,
            far_a,
            size,
            [wall_a, floor_a, wall_b, opening_a, cap_a],
        ),
        Arm::new(
            at,
            rim_b,
            top,
            far_b,
            size,
            [wall_b, floor_b, wall_a, opening_b, cap_b],
        ),
    ) else {
        return Ok(None);
    };
    if !certifies_arm(k, b, a, far_a)? || !certifies_arm(k, b, z, far_b)? {
        return Ok(None);
    }

    for &face in b.face_list() {
        k.check_cancelled(OP)?;
        if a.permits_contact(face) && z.permits_contact(face) {
            continue;
        }
        let Some(path) = planar_polygon(k, b, face)? else {
            return Ok(None);
        };
        let Some(bounds) = polygon_bounds(&path) else {
            return Ok(None);
        };
        if (!a.permits_contact(face) && a.contacts(bounds) != Some(false))
            || (!z.permits_contact(face) && z.contacts(bounds) != Some(false))
        {
            return Ok(None);
        }
    }
    Ok(Some(
        Extent3d::from_points(at, far_a).union(&Extent3d::from_point(far_b)),
    ))
}
