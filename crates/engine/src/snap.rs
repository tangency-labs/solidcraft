//! Snap points (#5881): the points a hovered edge or face offers as picks, so a point input
//! (Move's From and To, Measure, …) can take a hole's centre or an edge's middle where no vertex
//! sits. A round edge offers its centre (an arc also its ends and middle), a straight or other
//! edge its ends and middle, a cylinder or cone the centres of its ends, a sphere or torus its
//! centre, a vertex itself. A flat face offers its centre and its whole outline at once (#11476):
//! every corner, the middle of every straight edge and the centre of every round edge, holes
//! included, read from the exact faces and edges. The other shapes are Measure's: exact from the
//! kernel where it knows the curve or surface, fitted to the mesh otherwise. World coordinates,
//! as the viewport shows the model.
//!
//! [`snap_points_from`] also says where each point came from ([`SnapSource`]): the face, its
//! outward normal and the edge, so a tool can keep the point by name and turn a frame to the face.

use serde::Serialize;
use solidcore_record::Vec3;
use solidcore_record::brep::{Brep, Curve, Loop};
use solidcraft_geom::UnitVec3;
use solidcraft_geom::brep::eval::CurveEval;
use solidcraft_geom::prelude::*;
use solidcraft_kernel::{Body, EdgeCircle};

use crate::cmd::Shape;
use crate::{Sel, Session};

/// What a snap point is on its item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapKind {
    /// The centre of a circle, an arc, a sphere or a torus, or of one end of a cylinder or cone.
    Centre,
    /// The middle of an edge (along its length).
    Middle,
    /// One end of an edge.
    End,
    /// The centre of a flat face (of its area).
    FaceCentre,
    /// A vertex.
    Vertex,
}

/// A point an item offers as a pick.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SnapPoint {
    pub at: Vec3,
    pub kind: SnapKind,
    /// The body the item belongs to (empty for the origin and sketch geometry).
    pub body: String,
}

/// Where a snap point came from, in world coordinates: what a tool needs to keep the point by
/// name as the part changes (`joint.create`'s `{"edge", "corner", "on"}`) and to turn a frame to
/// the face it lies on. Empty for a vertex and for sketch geometry.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct SnapSource {
    /// A point on the face that offered the point: where the face was picked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub face: Option<Vec3>,
    /// Which way the point's face looks, as a unit vector out of the material: a flat face's
    /// outward normal, or for a cylinder's or cone's end centre the axis pointing out of that end.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normal: Option<Vec3>,
    /// A point inside the edge the point belongs to: the edge a corner starts, the straight edge a
    /// middle halves, the round edge a centre is the centre of, or the picked edge itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge: Option<Vec3>,
}

/// Points of one kind closer than this (relative to the item's size) are one snap point.
const SAME: f64 = 1e-6;

/// The most edges of one face whose points a flat face offers: a face with more (an imported
/// lattice) offers those of its first edges.
const MAX_OUTLINE_EDGES: usize = 4096;

/// A point found on an item, its kind and where it came from.
type Found = (Vec3, SnapKind, SnapSource);

/// The snap points `sel` offers: none for things with no points to offer (a body, a plane).
pub fn snap_points(s: &Session, sel: &Sel) -> Vec<SnapPoint> {
    snap_points_from(s, sel).into_iter().map(|(p, _)| p).collect()
}

/// [`snap_points`], each with where it came from.
pub fn snap_points_from(s: &Session, sel: &Sel) -> Vec<(SnapPoint, SnapSource)> {
    let body = match sel {
        Sel::Edge { body, .. } | Sel::Face { body, .. } | Sel::Vertex { body, .. } => body.clone(),
        _ => String::new(),
    };
    let tagged =
        |points: Vec<(Vec3, SnapKind)>, from: SnapSource| -> Vec<Found> { points.into_iter().map(|(at, kind)| (at, kind, from.clone())).collect() };
    let found = match sel {
        Sel::Vertex { point, .. } => vec![(*point, SnapKind::Vertex, SnapSource::default())],
        Sel::Edge { point, .. } => tagged(edge_points(s, sel), SnapSource { edge: Some(*point), ..SnapSource::default() }),
        Sel::SketchCurve { .. } | Sel::ExternalSketchCurve { .. } => tagged(edge_points(s, sel), SnapSource::default()),
        Sel::Face { .. } => face_points(s, sel),
        _ => Vec::new(),
    };
    let mut out: Vec<(SnapPoint, SnapSource)> = Vec::new();
    for (at, kind, from) in found {
        let tol = SAME * (1.0 + at.len());
        // Only points of one kind are one point: a hole centred on its face is both the face's
        // centre and the hole's, and only the hole's keeps its rim.
        if at.is_finite() && !out.iter().any(|(p, _)| p.kind == kind && p.at.dist(at) <= tol) {
            out.push((SnapPoint { at, kind, body: body.clone() }, from));
        }
    }
    out
}

/// An edge's or sketch curve's points: a whole circle its centre; an arc its centre, ends and
/// middle; anything else its ends and middle.
fn edge_points(s: &Session, sel: &Sel) -> Vec<(Vec3, SnapKind)> {
    let Some((shape, pts, _)) = crate::cmd::shape_of(s, sel) else { return Vec::new() };
    let ends = |v: &mut Vec<(Vec3, SnapKind)>| {
        v.extend(pts.first().map(|p| (*p, SnapKind::End)));
        v.extend(pts.last().map(|p| (*p, SnapKind::End)));
    };
    let mut v = Vec::new();
    match shape {
        Shape::Round { center, axis, radius, .. } => {
            v.push((center, SnapKind::Centre));
            if !shape.full() {
                // The kernel may split one arc into pieces (at a cylinder's seam): the whole arc.
                let arc = whole_arc(s, sel, center, axis, radius).unwrap_or(pts.clone());
                v.extend(arc.first().map(|p| (*p, SnapKind::End)));
                v.extend(arc.last().map(|p| (*p, SnapKind::End)));
                v.extend(arc_middle(center, axis, &arc).map(|p| (p, SnapKind::Middle)));
            }
        }
        Shape::Line { a, b } => v.extend([(a, SnapKind::End), (b, SnapKind::End), (a.lerp(b, 0.5), SnapKind::Middle)]),
        _ => {
            ends(&mut v);
            v.extend(along(&pts, 0.5).map(|p| (p, SnapKind::Middle)));
        }
    }
    v
}

/// The polyline of a picked edge joined with the neighbouring edges that continue it on the
/// same circle, in order: the whole arc when the kernel split it.
fn whole_arc(s: &Session, sel: &Sel, center: Vec3, axis: UnitVec3, radius: f64) -> Option<Vec<Vec3>> {
    let Sel::Edge { body, index, .. } = sel else { return None };
    let st = s.world_state().ok()?;
    let m = st.body(body)?.mesh();
    let tol = (radius * 1e-6).max(1e-9);
    let on = |e: &Vec<Vec3>| e.iter().all(|p| (p.dist(center) - radius).abs() <= tol * 10.0 && (*p - center).dot(axis).abs() <= tol * 10.0);
    let mut left: Vec<usize> =
        m.tangent_chain(*index, 2f64.to_radians()).into_iter().filter(|&i| i != *index && m.edges.get(i).is_some_and(on)).collect();
    let mut arc = m.edges.get(*index).filter(|e| on(e))?.clone();
    // Add pieces at either end while one starts or ends there (capped by the pieces there are).
    while let Some((k, at_end, rev)) = left.iter().enumerate().find_map(|(k, &i)| {
        let e = m.edges.get(i)?;
        let (first, last, a, z) = (*e.first()?, *e.last()?, *arc.first()?, *arc.last()?);
        [(z, first, true, false), (z, last, true, true), (a, last, false, false), (a, first, false, true)]
            .into_iter()
            .find(|(p, q, _, _)| p.dist(*q) <= tol * 10.0)
            .map(|(_, _, at_end, rev)| (k, at_end, rev))
    }) {
        let i = left.swap_remove(k);
        let mut piece = m.edges.get(i)?.clone();
        if rev {
            piece.reverse();
        }
        if at_end {
            arc.extend(piece.into_iter().skip(1));
        } else {
            piece.pop();
            piece.extend(arc);
            arc = piece;
        }
    }
    Some(arc)
}

/// The exact middle of an arc through `pts` (first to last): on the circle, halfway round
/// from one end to the other on the side the polyline runs.
fn arc_middle(center: Vec3, axis: UnitVec3, pts: &[Vec3]) -> Option<Vec3> {
    let (a, z) = (*pts.first()? - center, *pts.last()? - center);
    let near = along(pts, 0.5)? - center;
    let radius = a.len();
    // Along the bisector of the ends, or square to them across a half circle.
    let dir = (a + z).normalized().or_else(|| axis.cross(a).normalized())?;
    let dir = if dir.dot(near) < 0.0 { -dir } else { dir };
    Some(center + dir * radius)
}

/// The point a fraction `f` of the way along a polyline (by length).
fn along(pts: &[Vec3], f: f64) -> Option<Vec3> {
    let total: f64 = pts.windows(2).map(|w| w[0].dist(w[1])).sum();
    let mut left = total * f;
    for w in pts.windows(2) {
        let d = w[0].dist(w[1]);
        if left <= d && d > 0.0 {
            return Some(w[0].lerp(w[1], left / d));
        }
        left -= d;
    }
    pts.last().copied()
}

/// A face's points: a flat face its area's centre and its outline ([`outline_points`]); a
/// cylinder or cone the centres of its two ends on the axis; a sphere or torus its centre.
fn face_points(s: &Session, sel: &Sel) -> Vec<Found> {
    let Sel::Face { body, index, point } = sel else { return Vec::new() };
    let Ok(st) = s.world_state() else { return Vec::new() };
    let Some(b) = st.body(body) else { return Vec::new() };
    let Some((shape, _, tris)) = crate::cmd::shape_of(s, sel) else { return Vec::new() };
    let verts: Vec<Vec3> = tris.iter().flat_map(|t| t.iter().copied()).collect();
    let on = SnapSource { face: Some(*point), ..SnapSource::default() };
    let tagged = |points: Vec<(Vec3, SnapKind)>| -> Vec<Found> { points.into_iter().map(|(at, kind)| (at, kind, on.clone())).collect() };
    match solidcraft_kernel::face_surface(&b.body, *index) {
        Some(solidcraft_kernel::FaceSurface::Sphere { center, .. } | solidcraft_kernel::FaceSurface::Torus { center, .. }) => {
            return tagged(vec![(center, SnapKind::Centre)]);
        }
        Some(solidcraft_kernel::FaceSurface::Cone { apex, axis, .. }) => return ends_out(axis_ends(apex, axis, &verts), axis, &on),
        _ => {}
    }
    match shape {
        Shape::Round { center, axis, .. } => ends_out(axis_ends(center, axis, &verts), axis, &on),
        Shape::Plane { .. } => {
            let flat = SnapSource { normal: b.body.planar_face(*index).and_then(|p| p.normal().normalized()), ..on.clone() };
            let mut v: Vec<Found> = area_centre(&tris).map(|p| vec![(p, SnapKind::FaceCentre, flat.clone())]).unwrap_or_default();
            v.extend(outline_points(&b.body, *index).into_iter().map(|(at, kind, edge)| (at, kind, SnapSource { edge: Some(edge), ..flat.clone() })));
            v
        }
        _ => Vec::new(),
    }
}

/// A flat face's outline from the exact record: every corner (where one edge ends and another
/// begins, but not where a round edge runs on into the next piece of its circle, nor on a closed
/// circle's seam), the middle of every straight edge and the centre of every round edge, holes
/// included. Each comes with a point inside the edge it belongs to, which finds that edge again.
fn outline_points(body: &Body, face: usize) -> Vec<(Vec3, SnapKind, Vec3)> {
    let record = body.record();
    let Some(f) = record.faces.get(face) else { return Vec::new() };
    let tol = (body.size() * 1e-7).max(1e-9) * 100.0;
    let mut out = Vec::new();
    let mut left = MAX_OUTLINE_EDGES;
    for l in &f.loops {
        let Loop::Edges { edges } = l else { continue };
        let edges = &edges[..edges.len().min(left)];
        left -= edges.len();
        let circles: Vec<Option<EdgeCircle>> = edges.iter().map(|oe| solidcraft_kernel::edge_circle(body, oe.edge as usize)).collect();
        for (i, oe) in edges.iter().enumerate() {
            let Some(e) = record.edges.get(oe.edge as usize) else { continue };
            let Some(c) = CurveEval::new(e.parameters()).ok() else { continue };
            let [t0, t1] = e.interval;
            let Some(inside) = c.point((t0 + t1) * 0.5) else { continue };
            match (circles.get(i).and_then(Option::as_ref), &e.curve) {
                (Some(circle), _) => out.push((circle.center, SnapKind::Centre, inside)),
                (None, Curve::Line { .. }) => {
                    if let (Some(a), Some(b)) = (c.point(t0), c.point(t1)) {
                        out.push((a.lerp(b, 0.5), SnapKind::Middle, inside));
                    }
                }
                _ => {}
            }
            // The corner this edge starts from in the loop.
            if e.vertices[0] == e.vertices[1] {
                continue;
            }
            let previous = i.checked_sub(1).unwrap_or(edges.len().saturating_sub(1));
            let runs_on = match (circles.get(previous).and_then(Option::as_ref), circles.get(i).and_then(Option::as_ref)) {
                (Some(a), Some(b)) if previous != i => same_circle(a, b, tol),
                _ => false,
            };
            let start = if oe.forward { e.vertices[0] } else { e.vertices[1] };
            if let (false, Some(p)) = (runs_on, vertex(record, start)) {
                out.push((p, SnapKind::End, inside));
            }
        }
        if left == 0 {
            break;
        }
    }
    out
}

/// Whether two round edges lie on one circle: one circle the kernel split into pieces.
fn same_circle(a: &EdgeCircle, b: &EdgeCircle, tol: f64) -> bool {
    a.center.dist(b.center) <= tol && (a.radius - b.radius).abs() <= tol && a.axis.dot(b.axis).abs() >= 1.0 - 1e-9
}

/// Vertex `i`'s point.
fn vertex(record: &Brep, i: u32) -> Option<Vec3> {
    record.vertices.get(i as usize).map(|v| Vec3::from(v.point)).filter(|p| p.is_finite())
}

/// A round face's two end centres (lower along `axis` first), each with the axis pointing out of
/// its end as the normal: the normal of the face a hole opens onto, or of a pin's end.
fn ends_out(ends: Vec<(Vec3, SnapKind)>, axis: UnitVec3, on: &SnapSource) -> Vec<Found> {
    ends.into_iter().zip([-axis.vec(), axis.vec()]).map(|((at, kind), out)| (at, kind, SnapSource { normal: Some(out), ..on.clone() })).collect()
}

/// The points of the axis through `origin` along `axis` level with the face's two ends.
fn axis_ends(origin: Vec3, axis: UnitVec3, verts: &[Vec3]) -> Vec<(Vec3, SnapKind)> {
    let along = || verts.iter().map(|p| (*p - origin).dot(axis));
    let (lo, hi) = (along().fold(f64::INFINITY, f64::min), along().fold(f64::NEG_INFINITY, f64::max));
    if !(lo.is_finite() && hi.is_finite()) {
        return Vec::new();
    }
    vec![(origin + axis * lo, SnapKind::Centre), (origin + axis * hi, SnapKind::Centre)]
}

/// The centre of the area of some triangles.
fn area_centre(tris: &[[Vec3; 3]]) -> Option<Vec3> {
    let (mut sum, mut area) = (Vec3::ZERO, 0.0);
    for [a, b, c] in tris {
        let w = (*b - *a).cross(*c - *a).len() * 0.5;
        sum += (*a + *b + *c) * (w / 3.0);
        area += w;
    }
    (area > 0.0).then(|| sum * (1.0 / area))
}
