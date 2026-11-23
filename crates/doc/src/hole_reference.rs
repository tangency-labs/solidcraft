//! A Single hole placed by distances from reference edges (#37): up to two straight edges of the
//! face it is drilled into, each with the distance of the hole's centre from that edge's line,
//! measured in the face's plane. The edges are found again by their persistent names, else by
//! their stored points, so the hole follows them when the part changes.

use solidcore_record::Vec3;
use solidcraft_geom::Mesh;
use solidcraft_geom::prelude::*;

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
    let refs: Vec<RefInput> = edges.iter().map(|(p, name)| RefInput::Edge(*p, *name)).collect();
    reference_frame_of(st, at, dir, &refs, exact)
}

/// One reference of a hole: a straight edge (a point on it and its persistent name, when it has
/// one), or a plane or an axis, which measures from the line it makes in the face's plane. A
/// plane's or an axis' `opposite` is the side of that line the hole is on (see
/// [`crate::HoleReference::opposite`]); none takes the side the hole is on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RefInput<'a> {
    Edge(Vec3, Option<&'a str>),
    Plane { point: Vec3, normal: Vec3, opposite: Option<bool> },
    Axis { point: Vec3, dir: Vec3, opposite: Option<bool> },
}

impl RefInput<'_> {
    /// This reference with the side of its line set, for a plane or an axis.
    fn on_side(self, side: Option<bool>) -> Self {
        match self {
            RefInput::Plane { point, normal, .. } => RefInput::Plane { point, normal, opposite: side },
            RefInput::Axis { point, dir, .. } => RefInput::Axis { point, dir, opposite: side },
            edge => edge,
        }
    }
}

/// The `edges` slot of a [`ReferenceFrame`] for a reference that is a plane or an axis.
pub const NO_EDGE: usize = usize::MAX;

/// [`reference_frame`] for edges, planes and axes. A plane's line is where it cuts the face's
/// plane; an axis' line is its projection onto it. The side of such a line is its `opposite`, else
/// the one the hole is on.
pub fn reference_frame_of(st: &ModelState, at: Vec3, dir: Vec3, refs: &[RefInput], exact: bool) -> Result<ReferenceFrame> {
    if refs.len() > MAX_HOLE_REFERENCES {
        return Err(DocError::Invalid(format!("a hole takes at most {MAX_HOLE_REFERENCES} references")));
    }
    let dir = dir.normalized().ok_or_else(|| DocError::Invalid("hole direction".into()))?;
    let finite = |r: &RefInput| match r {
        RefInput::Edge(p, _) => p.is_finite(),
        RefInput::Plane { point, normal, .. } => point.is_finite() && normal.is_finite(),
        RefInput::Axis { point, dir, .. } => point.is_finite() && dir.is_finite(),
    };
    if !at.is_finite() || !refs.iter().all(finite) {
        return Err(DocError::Invalid("references need finite points".into()));
    }
    let points: Vec<Vec3> = refs.iter().filter_map(|r| if let RefInput::Edge(p, _) = r { Some(*p) } else { None }).collect();
    let b = body_for(st, at, &points).ok_or_else(|| DocError::Invalid("no body at the hole for its references".into()))?;
    let m = b.mesh();
    let size = b.body.size().max(1e-9);
    // Each reference's edge, in order (none for a plane or an axis).
    let mut found = Vec::with_capacity(refs.len());
    for (k, r) in refs.iter().enumerate() {
        found.push(match r {
            RefInput::Edge(p, name) => Some(find_edge(b, &m, k, *p, *name, exact)?),
            _ => None,
        });
    }
    let edges: Vec<(Vec3, Vec3, usize)> = found.iter().flatten().copied().collect();
    let (face, origin, normal) = flat_face(b, &m, at, dir, &edges).ok_or_else(|| {
        DocError::Invalid("references need the hole on a flat face: no flat face facing the drill at the hole or along its edges".into())
    })?;
    let mut lines = Vec::with_capacity(refs.len());
    for (k, (r, e)) in refs.iter().zip(&found).enumerate() {
        if let Some((a, z, _)) = e {
            let flat = |q: Vec3| q - normal * (q - origin).dot(normal);
            let (a, z) = (flat(*a), flat(*z));
            let along = (z - a).normalized().ok_or_else(|| DocError::Invalid(format!("reference edge {} is square to the face", k + 1)))?;
            let across =
                normal.cross(along).normalized().ok_or_else(|| DocError::Invalid(format!("reference edge {} is square to the face", k + 1)))?;
            let mid = (a + z) * 0.5;
            // The side the face is on next to the edge; an edge the face doesn't border (or borders
            // on both sides) takes the side the hole is on.
            let side = face_side(&m, face, mid, across, normal, size).unwrap_or_else(|| if (at - mid).dot(across) < 0.0 { -1.0 } else { 1.0 });
            lines.push(ReferenceLine { point: mid, along, toward: across * side });
        } else {
            lines.push(datum_line(r, k, at, origin, normal)?);
        }
    }
    if let [a, b] = lines.as_slice()
        && a.along.cross(b.along).len() < 1e-6
    {
        return Err(DocError::Invalid("the references are parallel: pick references that cross".into()));
    }
    let edge_index = found.iter().map(|e| e.map_or(NO_EDGE, |(_, _, i)| i)).collect();
    Ok(ReferenceFrame { body: b.name.clone(), face, origin, normal, lines, edges: edge_index })
}

/// The line a plane or an axis makes in the face's plane (through `origin`, square to `normal`),
/// on the side its `opposite` says, else on the side of the hole at `at`.
fn datum_line(r: &RefInput, k: usize, at: Vec3, origin: Vec3, normal: Vec3) -> Result<ReferenceLine> {
    let (point, along, opposite) = match r {
        RefInput::Plane { point, normal: n, opposite } => {
            let n = n.normalized().ok_or_else(|| DocError::Invalid(format!("reference {} has no direction", k + 1)))?;
            // Running so that the plane's normal is on its positive side (`normal × along`).
            let u = n.cross(normal);
            let along = u.normalized().ok_or_else(|| DocError::Invalid(format!("reference {} is a plane parallel to the face", k + 1)))?;
            // The point where the face's plane, the plane and the plane square to the line through
            // the hole meet.
            let (d1, d2, d3) = (normal.dot(origin), n.dot(*point), along.dot(at));
            let det = normal.dot(n.cross(along));
            if det.abs() < 1e-9 {
                return Err(DocError::Invalid(format!("reference {} is a plane parallel to the face", k + 1)));
            }
            ((n.cross(along) * d1 + along.cross(normal) * d2 + normal.cross(n) * d3) * (1.0 / det), along, opposite)
        }
        RefInput::Axis { point, dir, opposite } => {
            let d = *dir - normal * dir.dot(normal);
            let along = d.normalized().ok_or_else(|| DocError::Invalid(format!("reference {} is an axis square to the face", k + 1)))?;
            (*point - normal * (*point - origin).dot(normal), along, opposite)
        }
        RefInput::Edge(..) => return Err(DocError::Invalid("an edge is not a datum".into())),
    };
    let across = normal.cross(along).normalized().ok_or_else(|| DocError::Invalid(format!("reference {} is square to the face", k + 1)))?;
    let opposite = opposite.unwrap_or((at - point).dot(across) < 0.0);
    Ok(ReferenceLine { point, along, toward: if opposite { -across } else { across } })
}

/// The geometry of the plane or axis a reference is made from, by name: the origin planes `XY`,
/// `XZ`, `YZ` and axes `X`, `Y`, `Z`, else a construction plane or axis of the timeline.
pub fn datum_input(st: &ModelState, datum: &crate::HoleDatum) -> Result<RefInput<'static>> {
    use crate::construct::ConstructGeom;
    let find = |name: &str| st.construct.iter().find(|c| c.name == name).map(|c| c.geom.clone());
    match datum {
        crate::HoleDatum::Plane(name) => match name.to_ascii_uppercase().as_str() {
            "XY" => Ok(RefInput::Plane { point: Vec3::ZERO, normal: Vec3::Z, opposite: None }),
            "XZ" => Ok(RefInput::Plane { point: Vec3::ZERO, normal: Vec3::Y, opposite: None }),
            "YZ" => Ok(RefInput::Plane { point: Vec3::ZERO, normal: Vec3::X, opposite: None }),
            _ => match find(name) {
                Some(ConstructGeom::Plane(p)) => Ok(RefInput::Plane { point: p.origin, normal: p.normal(), opposite: None }),
                Some(_) => Err(DocError::Invalid(format!("`{name}` is not a plane"))),
                None => Err(DocError::Unknown(format!("plane `{name}` (it must come earlier in the timeline)"))),
            },
        },
        crate::HoleDatum::Axis(name) => match name.to_ascii_uppercase().as_str() {
            "X" => Ok(RefInput::Axis { point: Vec3::ZERO, dir: Vec3::X, opposite: None }),
            "Y" => Ok(RefInput::Axis { point: Vec3::ZERO, dir: Vec3::Y, opposite: None }),
            "Z" => Ok(RefInput::Axis { point: Vec3::ZERO, dir: Vec3::Z, opposite: None }),
            _ => match find(name) {
                Some(ConstructGeom::Axis { origin, dir }) => Ok(RefInput::Axis { point: origin, dir, opposite: None }),
                Some(_) => Err(DocError::Invalid(format!("`{name}` is not an axis"))),
                None => Err(DocError::Unknown(format!("axis `{name}` (it must come earlier in the timeline)"))),
            },
        },
    }
}

/// The references of a hole as [`RefInput`]s, the names being the persistent names of the edges
/// (by reference, empty where a reference has none).
pub fn inputs_of<'a>(st: &ModelState, references: &'a [crate::HoleReference], names: &'a [String]) -> Result<Vec<RefInput<'a>>> {
    references
        .iter()
        .enumerate()
        .map(|(i, r)| match &r.datum {
            Some(d) => datum_input(st, d).map(|input| input.on_side(Some(r.opposite))),
            None => Ok(RefInput::Edge(r.edge.0, names.get(i).map(String::as_str))),
        })
        .collect()
}

/// Sets each plane or axis reference's `opposite` to the side of its line the hole at `at`,
/// drilled along `dir`, is on, so the hole keeps that side when the plane or axis moves past it.
pub fn keep_datum_sides(st: &ModelState, at: Vec3, dir: Vec3, references: &mut [crate::HoleReference], exact: bool) -> Result<()> {
    if references.iter().all(|r| r.datum.is_none()) {
        return Ok(());
    }
    // Every line on its positive side: the hole is opposite where it is on the negative one.
    let positive: Vec<RefInput> = inputs_of(st, references, &[])?.into_iter().map(|r| r.on_side(Some(false))).collect();
    let frame = reference_frame_of(st, at, dir, &positive, exact)?;
    for (r, line) in references.iter_mut().zip(&frame.lines) {
        if r.datum.is_some() {
            r.opposite = (at - line.point).dot(line.toward) < 0.0;
        }
    }
    Ok(())
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
    match body_for(st, at, edges) {
        Some(b) => crate::naming::edge_names_at(b, edges),
        None => Vec::new(),
    }
}

/// The persistent names of a cylinder's reference edges (#6380), on the body it stands on at
/// `base` (its coordinates are numbers when it has reference edges).
pub fn cylinder_edge_names(st: &ModelState, base: &crate::PointExpr, references: &[crate::HoleReference]) -> Vec<String> {
    let [x, y, z] = base.clone().map(|c| c.parse::<f64>().unwrap_or(0.0));
    reference_names(st, Vec3::new(x, y, z), references)
}

/// The persistent names of a hole's references, one for each: those of its edges, empty for a plane
/// or an axis.
pub fn reference_names(st: &ModelState, at: Vec3, references: &[crate::HoleReference]) -> Vec<String> {
    let edges: Vec<Vec3> = references.iter().filter(|r| r.datum.is_none()).map(|r| r.edge.0).collect();
    let mut named = reference_edge_names(st, at, &edges).into_iter();
    references.iter().map(|r| if r.datum.is_some() { String::new() } else { named.next().unwrap_or_default() }).collect()
}

/// The (non-mesh) body whose surface is nearest `p`; of the bodies equally near (parts that touch
/// at `p`), the one the `edges` run along, so a reference edge picked on one part is not looked
/// for on the part beside it.
fn body_for<'a>(st: &'a ModelState, p: Vec3, edges: &[Vec3]) -> Option<&'a ModelBody> {
    let near: Vec<(f64, &ModelBody)> =
        st.bodies.iter().filter(|b| !b.body.is_mesh()).filter_map(|b| crate::appearance::nearest_face(b, p).map(|(d, _)| (d, b))).collect();
    let nearest = near.iter().map(|(d, _)| *d).min_by(f64::total_cmp)?;
    let tied = near.iter().filter(|(d, b)| *d <= nearest + (b.body.size() * 1e-4).max(1e-6)).map(|(_, b)| *b);
    let has_edges = |b: &ModelBody| {
        let reach = (b.body.size() * 1e-2).max(1e-3);
        edges.iter().all(|q| matches!(b.body.nearest_edge(*q, reach), Ok(Some((_, d))) if d <= reach))
    };
    let tied: Vec<&ModelBody> = tied.collect();
    tied.iter().find(|b| has_edges(b)).or(tied.first()).copied()
}

/// How far `p` is from an edge's polyline.
fn to_polyline(p: Vec3, e: &[Vec3]) -> f64 {
    e.windows(2).map(|w| p.dist_to_segment(w[0], w[1])).fold(f64::INFINITY, f64::min)
}

/// Reference edge `k` (its ends, and its index in the mesh), found by `name` (a name that finds
/// no edge fails, asking for the edge to be picked again), else by the edge nearest `p`: one
/// that runs through `p` for a fresh pick, within a quarter of the body for a stored one. It
/// must be straight.
fn find_edge(b: &ModelBody, m: &Mesh, k: usize, p: Vec3, name: Option<&str>, exact: bool) -> Result<(Vec3, Vec3, usize)> {
    let size = b.body.size().max(1e-9);
    let by_name = match name.filter(|n| !n.is_empty()) {
        Some(n) => Some(crate::naming::locate_edge(b, n, p).map_err(|_| DocError::reselect(n))?.edge),
        None => None,
    };
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
