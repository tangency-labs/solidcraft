//! A Single hole placed by distances from reference edges (#37): up to two straight edges of the
//! face it is drilled into, each with the distance of the hole's centre from that edge's line,
//! measured in the face's plane. The edges are found again by their persistent names, else by
//! their stored points, so the hole follows them when the part changes.

use solidcraft_geom::{Mesh, Vec3};

use crate::{DocError, ModelBody, ModelState, Result};

/// The most reference edges a hole takes.
pub const MAX_HOLE_REFERENCES: usize = 2;

/// A reference edge's line in the face's plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReferenceLine {
    /// A point on the line: the edge's middle, moved into the face's plane.
    pub point: Vec3,
    /// Along the edge (unit).
    pub along: Vec3,
    /// In the plane, square to the edge, toward the side the face is on (unit): the way the
    /// distance grows.
    pub toward: Vec3,
}

impl ReferenceLine {
    /// How far `p` is from the line, toward the face's side (negative on the other side).
    pub fn distance(&self, p: Vec3) -> f64 {
        (p - self.point).dot(self.toward)
    }

    /// The point of the line nearest `p`.
    pub fn foot(&self, p: Vec3) -> Vec3 {
        self.point + self.along * (p - self.point).dot(self.along)
    }
}

/// The face a referenced hole is drilled into (its plane) and its reference edges' lines.
#[derive(Clone, Debug, PartialEq)]
pub struct ReferenceFrame {
    /// The body and the face (its index).
    pub body: String,
    pub face: usize,
    /// A point on the face's plane.
    pub origin: Vec3,
    /// The face's outward unit normal.
    pub normal: Vec3,
    pub lines: Vec<ReferenceLine>,
    /// The reference edges found (their indices in the body's mesh), one per line.
    pub edges: Vec<usize>,
}

impl ReferenceFrame {
    /// `p` moved square onto the face's plane.
    pub fn onto_plane(&self, p: Vec3) -> Vec3 {
        p - self.normal * (p - self.origin).dot(self.normal)
    }

    /// How far `p` is from each line (see [`ReferenceLine::distance`]).
    pub fn distances(&self, p: Vec3) -> Vec<f64> {
        let p = self.onto_plane(p);
        self.lines.iter().map(|l| l.distance(p)).collect()
    }

    /// The hole's centre at `distances` (one per line) from the lines, in the face's plane. With
    /// one line, `near` moves square to it (it keeps its place along the edge); with two, the
    /// centre is where both distances hold.
    pub fn place(&self, near: Vec3, distances: &[f64]) -> Result<Vec3> {
        if let Some((i, _)) = distances.iter().enumerate().find(|(_, d)| !(d.is_finite() && **d >= 0.0 && **d < 1e9)) {
            return Err(DocError::Invalid(format!("reference distance {} must be a length of 0 or more", i + 1)));
        }
        let p = self.onto_plane(near);
        match (self.lines.as_slice(), distances) {
            ([], []) => Ok(p),
            ([a], [d]) => Ok(p + a.toward * (d - a.distance(p))),
            ([a, b], [d1, d2]) => {
                // p + r1 * a.toward + s * e2, with e2 square to a in the plane: a's distance is
                // then d1, and s gives b's.
                let (r1, r2) = (d1 - a.distance(p), d2 - b.distance(p));
                let e2 = self.normal.cross(a.toward);
                let across = b.toward.dot(e2);
                if across.abs() < 1e-6 {
                    return Err(DocError::Invalid("the reference edges are parallel: pick edges that cross".into()));
                }
                Ok(p + a.toward * r1 + e2 * ((r2 - r1 * a.toward.dot(b.toward)) / across))
            }
            _ => Err(DocError::Invalid("a hole takes one distance for each reference edge".into())),
        }
    }
}

/// The reference frame of a hole placed at `at` (on the surface), drilled along `dir` (into the
/// material), from its reference edges: a point on each edge and, when it has one, the edge's
/// persistent name. A fresh pick (`exact`) must lie on its edge; a stored one is found by its
/// name, else the edge nearest its point. The face is the flat face under `at` that faces the
/// drill, else the one the edges border.
pub fn reference_frame(st: &ModelState, at: Vec3, dir: Vec3, edges: &[(Vec3, Option<&str>)], exact: bool) -> Result<ReferenceFrame> {
    if edges.len() > MAX_HOLE_REFERENCES {
        return Err(DocError::Invalid(format!("a hole takes at most {MAX_HOLE_REFERENCES} reference edges")));
    }
    let dir = dir.normalized().ok_or_else(|| DocError::Invalid("hole direction".into()))?;
    if !at.is_finite() || edges.iter().any(|(p, _)| !p.is_finite()) {
        return Err(DocError::Invalid("reference edges need finite points".into()));
    }
    let b = body_at(st, at).ok_or_else(|| DocError::Invalid("no body at the hole for its reference edges".into()))?;
    let m = b.mesh();
    let size = b.body.size().max(1e-9);
    let mut found = Vec::with_capacity(edges.len());
    for (k, (p, name)) in edges.iter().enumerate() {
        found.push(find_edge(b, &m, k, *p, *name, exact)?);
    }
    let (face, origin, normal) = flat_face(b, &m, at, dir, &found).ok_or_else(|| {
        DocError::Invalid("reference edges need the hole on a flat face: no flat face facing the drill at the hole or along its edges".into())
    })?;
    let mut lines = Vec::with_capacity(found.len());
    for (k, (a, z, _)) in found.iter().enumerate() {
        let flat = |q: Vec3| q - normal * (q - origin).dot(normal);
        let (a, z) = (flat(*a), flat(*z));
        let along = (z - a).normalized().ok_or_else(|| DocError::Invalid(format!("reference edge {} is square to the face", k + 1)))?;
        let across = normal.cross(along).normalized().ok_or_else(|| DocError::Invalid(format!("reference edge {} is square to the face", k + 1)))?;
        let mid = (a + z) * 0.5;
        // The side the face is on next to the edge; an edge the face doesn't border (or borders
        // on both sides) takes the side the hole is on.
        let side = face_side(&m, face, mid, across, normal, size).unwrap_or_else(|| if (at - mid).dot(across) < 0.0 { -1.0 } else { 1.0 });
        lines.push(ReferenceLine { point: mid, along, toward: across * side });
    }
    if let [a, b] = lines.as_slice()
        && a.along.cross(b.along).len() < 1e-6
    {
        return Err(DocError::Invalid("the reference edges are parallel: pick edges that cross".into()));
    }
    Ok(ReferenceFrame { body: b.name.clone(), face, origin, normal, lines, edges: found.iter().map(|(_, _, i)| *i).collect() })
}

/// Whether `p` lies on the flat face a reference frame was made for.
pub fn on_reference_face(st: &ModelState, frame: &ReferenceFrame, p: Vec3) -> bool {
    let Some(b) = st.bodies.iter().find(|b| b.name == frame.body) else { return false };
    let m = b.mesh();
    let tol = (b.body.size() * 1e-4).max(1e-6);
    m.triangles
        .iter()
        .zip(&m.tri_face)
        .filter(|(_, f)| **f as usize == frame.face)
        .any(|(t, _)| m.tri(t).is_some_and(|[a, bb, c]| crate::appearance::closest_on_triangle(p, a, bb, c).dist(p) <= tol))
}

/// The persistent names of the edges at `edges`, on the body the hole at `at` is drilled into
/// (empty where an edge has none).
pub fn reference_edge_names(st: &ModelState, at: Vec3, edges: &[Vec3]) -> Vec<String> {
    match body_at(st, at) {
        Some(b) => crate::naming::edge_names_at(b, edges),
        None => Vec::new(),
    }
}

/// The persistent names of a cylinder's reference edges (#6380), on the body it stands on at
/// `base` (its coordinates are numbers when it has reference edges).
pub fn cylinder_edge_names(st: &ModelState, base: &crate::PointExpr, references: &[crate::HoleReference]) -> Vec<String> {
    let [x, y, z] = base.clone().map(|c| c.parse::<f64>().unwrap_or(0.0));
    let edges: Vec<Vec3> = references.iter().map(|r| r.edge.0).collect();
    reference_edge_names(st, Vec3::new(x, y, z), &edges)
}

/// The (non-mesh) body whose surface is nearest `p`.
fn body_at(st: &ModelState, p: Vec3) -> Option<&ModelBody> {
    st.bodies
        .iter()
        .filter(|b| !b.body.is_mesh())
        .filter_map(|b| crate::appearance::nearest_face(b, p).map(|(d, _)| (d, b)))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, b)| b)
}

/// How far `p` is from an edge's polyline.
fn to_polyline(p: Vec3, e: &[Vec3]) -> f64 {
    e.windows(2).map(|w| p.dist_to_segment(w[0], w[1])).fold(f64::INFINITY, f64::min)
}

/// Reference edge `k` (its ends, and its index in the mesh), found by `name`, else by the edge
/// nearest `p`: one that runs through `p` for a fresh pick, within a quarter of the body for a
/// stored one. It must be straight.
fn find_edge(b: &ModelBody, m: &Mesh, k: usize, p: Vec3, name: Option<&str>, exact: bool) -> Result<(Vec3, Vec3, usize)> {
    let size = b.body.size().max(1e-9);
    let by_name = name.filter(|n| !n.is_empty()).and_then(|n| crate::naming::locate_edge(b, n, p).ok()).map(|hit| hit.edge);
    let i = match by_name {
        Some(i) => i,
        None => {
            let reach = if exact { (size * 1e-2).max(1e-3) } else { size * 0.25 };
            m.edges
                .iter()
                .enumerate()
                .map(|(i, e)| (to_polyline(p, e), i))
                .filter(|(d, _)| d.is_finite())
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .filter(|(d, _)| *d <= reach)
                .map(|(_, i)| i)
                .ok_or_else(|| {
                    DocError::Invalid(if exact {
                        format!("reference edge {}: no edge at [{}, {}, {}]", k + 1, p.x, p.y, p.z)
                    } else {
                        format!("reference edge {} no longer exists", k + 1)
                    })
                })?
        }
    };
    let e = m.edges.get(i).ok_or_else(|| DocError::Invalid(format!("reference edge {} no longer exists", k + 1)))?;
    let (Some(a), Some(z)) = (e.first().copied(), e.last().copied()) else {
        return Err(DocError::Invalid(format!("reference edge {} no longer exists", k + 1)));
    };
    let len = a.dist(z);
    let straight = len > size * 1e-9 && e.iter().all(|q| q.dist_to_segment(a, z) <= 1e-5 * (1.0 + len));
    if !straight {
        return Err(DocError::Invalid(format!("reference edge {} is not straight: pick a straight edge", k + 1)));
    }
    Ok((a, z, i))
}

/// The flat face a referenced hole goes into (its index, a point on it and its outward normal):
/// the one under `at` when it faces the drill, else the one the edges border that faces it most.
fn flat_face(b: &ModelBody, m: &Mesh, at: Vec3, dir: Vec3, edges: &[(Vec3, Vec3, usize)]) -> Option<(usize, Vec3, Vec3)> {
    let size = b.body.size().max(1e-9);
    let plane = |f: usize| match crate::kernel::face_surface(&b.body, f) {
        // Outward, so against the drill.
        Some(crate::kernel::FaceSurface::Plane { normal, point }) => {
            let normal: Vec3 = normal.into();
            let n = if normal.dot(dir) > 0.0 { -normal } else { normal };
            (n.dot(dir) < -1e-3).then_some((f, point, n))
        }
        _ => None,
    };
    if let Some((d, f)) = crate::appearance::nearest_face(b, at)
        && d <= (size * 1e-4).max(1e-6)
        && let Some(hit) = plane(f)
    {
        return Some(hit);
    }
    // The faces every edge borders.
    let mut faces: Vec<usize> =
        edges.first().and_then(|(_, _, i)| m.edge_faces.get(*i)).map(|fs| fs.iter().map(|f| *f as usize).collect()).unwrap_or_default();
    for (_, _, i) in edges.iter().skip(1) {
        let theirs = m.edge_faces.get(*i).map(Vec::as_slice).unwrap_or(&[]);
        faces.retain(|f| theirs.iter().any(|g| *g as usize == *f));
    }
    faces.into_iter().filter_map(plane).min_by(|x, y| x.2.dot(dir).total_cmp(&y.2.dot(dir)))
}

/// Which side of an edge (through `mid`, square to it in the plane along `across`) face `face`
/// lies on: +1 or −1, or `None` when both sides or neither are that face.
fn face_side(m: &Mesh, face: usize, mid: Vec3, across: Vec3, normal: Vec3, size: f64) -> Option<f64> {
    let step = (size * 1e-3).max(1e-6);
    let lands = |s: f64| {
        let from = mid + across * (s * step) + normal * step;
        m.raycast(from, -normal).is_some_and(|(t, tri)| t <= 2.0 * step && m.tri_face.get(tri).is_some_and(|f| *f as usize == face))
    };
    match (lands(1.0), lands(-1.0)) {
        (true, false) => Some(1.0),
        (false, true) => Some(-1.0),
        _ => None,
    }
}
