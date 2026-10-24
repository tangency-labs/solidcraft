//! Snap points (#5881): the points a hovered edge or face offers as picks, so a point input
//! (Move's From and To, Measure, …) can take a hole's centre or an edge's middle where no vertex
//! sits. A round edge offers its centre (an arc also its ends and middle), a straight or other
//! edge its ends and middle, a flat face its centre, a cylinder or cone the centres of its ends,
//! a sphere or torus its centre, a vertex itself. The shapes are Measure's: exact from the
//! kernel where it knows the curve or surface, fitted to the mesh otherwise. World coordinates,
//! as the viewport shows the model.

use serde::Serialize;
use solidcraft_geom::{UnitVec3, Vec3};

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

/// Points closer than this (relative to the item's size) are one snap point.
const SAME: f64 = 1e-6;

/// The snap points `sel` offers: none for things with no points to offer (a body, a plane).
pub fn snap_points(s: &Session, sel: &Sel) -> Vec<SnapPoint> {
    let body = match sel {
        Sel::Edge { body, .. } | Sel::Face { body, .. } | Sel::Vertex { body, .. } => body.clone(),
        _ => String::new(),
    };
    let found = match sel {
        Sel::Vertex { point, .. } => vec![(*point, SnapKind::Vertex)],
        Sel::Edge { .. } | Sel::SketchCurve { .. } | Sel::ExternalSketchCurve { .. } => edge_points(s, sel),
        Sel::Face { .. } => face_points(s, sel),
        _ => Vec::new(),
    };
    let mut out: Vec<SnapPoint> = Vec::new();
    for (at, kind) in found {
        let tol = SAME * (1.0 + at.len());
        if at.is_finite() && !out.iter().any(|p| p.at.dist(at) <= tol) {
            out.push(SnapPoint { at, kind, body: body.clone() });
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
    let st = s.world_state();
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

/// A face's points: a flat face its area's centre; a cylinder or cone the centres of its two
/// ends on the axis; a sphere or torus its centre.
fn face_points(s: &Session, sel: &Sel) -> Vec<(Vec3, SnapKind)> {
    let Sel::Face { body, index, .. } = sel else { return Vec::new() };
    let st = s.world_state();
    let Some(b) = st.body(body) else { return Vec::new() };
    let Some((shape, _, tris)) = crate::cmd::shape_of(s, sel) else { return Vec::new() };
    let verts: Vec<Vec3> = tris.iter().flat_map(|t| t.iter().copied()).collect();
    match solidcraft_kernel::face_surface(&b.body, *index) {
        Some(solidcraft_kernel::FaceSurface::Sphere { center, .. } | solidcraft_kernel::FaceSurface::Torus { center, .. }) => {
            return vec![(center, SnapKind::Centre)];
        }
        Some(solidcraft_kernel::FaceSurface::Cone { apex, axis, .. }) => return axis_ends(apex, axis, &verts),
        _ => {}
    }
    match shape {
        Shape::Round { center, axis, .. } => axis_ends(center, axis, &verts),
        Shape::Plane { .. } => area_centre(&tris).map(|p| vec![(p, SnapKind::FaceCentre)]).unwrap_or_default(),
        _ => Vec::new(),
    }
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
