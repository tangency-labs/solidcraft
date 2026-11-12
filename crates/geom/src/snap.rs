//! Snap candidates on model geometry: the places a pick near the model locks onto — an edge's
//! ends and middle, the centre and quadrants of a circular edge, a planar face's centre, the
//! nearest point along an edge and the point on a face. This is the conventional CAD snap
//! vocabulary, worked out from the display mesh so a hover can ask for it every frame.
//!
//! A caller says which kinds it wants ([`SnapKinds`]) and ranks what comes back with [`rank`],
//! which multiplies each candidate's distance by its kind's [`SnapKind::weight`]: an exact place
//! a little further from the cursor still wins over a point that only slides along an edge.
//! Ranking in screen pixels is the viewport's job; [`rank`] takes whatever the caller measures in.
//!
//! [`edge_candidates`] and [`face_candidates`] answer "the cursor is on this edge or face, what
//! does it offer" — a circular edge offers its centre however far that is from the cursor, which
//! is what makes hovering the rim of a hole snap to its axis. [`candidates_near`] answers the
//! other question, "what lies within this distance of this place", which is what the
//! `model.snap` command is for.

use crate::Mesh;
use crate::prelude::*;
use solidcore_record::Vec3;

/// Which snap kinds a search wants: a bit set of [`SnapKind::bit`].
pub type SnapKinds = u16;

pub const VERTEX: SnapKinds = 1;
pub const CENTER: SnapKinds = 2;
pub const MIDPOINT: SnapKinds = 4;
pub const QUADRANT: SnapKinds = 8;
pub const FACE_CENTER: SnapKinds = 16;
pub const ON_EDGE: SnapKinds = 32;
pub const ON_FACE: SnapKinds = 64;
pub const ORIGIN: SnapKinds = 128;
/// The kinds that lock onto an exact place, rather than sliding along the geometry.
pub const EXACT: SnapKinds = ORIGIN | VERTEX | CENTER | MIDPOINT | QUADRANT | FACE_CENTER;
/// Every kind.
pub const ALL: SnapKinds = EXACT | ON_EDGE | ON_FACE;

/// Most candidates one search returns.
pub const MAX_CANDIDATES: usize = 64;
/// Most mesh edges one broad search looks at: a radius comes from input and is hostile.
const MAX_EDGES: usize = 50_000;
/// Most triangles one face's centre is worked out from.
const MAX_FACE_TRIANGLES: usize = 200_000;
/// Most faces one broad search works centres out for.
const MAX_NEAR_FACES: usize = 32;
/// How far a face's triangle normals may turn and the face still count as planar (cosine).
const PLANAR_COS: f64 = 1e-4;

/// What a snap candidate stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapKind {
    /// The origin of the design, which is nowhere on the model.
    Origin,
    /// An end of an edge (a model vertex).
    Vertex,
    /// The centre of a circular edge.
    Center,
    /// The middle of an edge.
    Midpoint,
    /// A quarter turn around a circular edge from its start.
    Quadrant,
    /// The centre of a planar face, when it lies on the face.
    FaceCenter,
    /// The point of an edge nearest the cursor: slides along it.
    OnEdge,
    /// The point of a face under the cursor: slides over it.
    OnFace,
}

impl SnapKind {
    pub const fn bit(self) -> SnapKinds {
        match self {
            SnapKind::Origin => ORIGIN,
            SnapKind::Vertex => VERTEX,
            SnapKind::Center => CENTER,
            SnapKind::Midpoint => MIDPOINT,
            SnapKind::Quadrant => QUADRANT,
            SnapKind::FaceCenter => FACE_CENTER,
            SnapKind::OnEdge => ON_EDGE,
            SnapKind::OnFace => ON_FACE,
        }
    }

    /// How strongly this kind pulls: [`rank`] multiplies the distance by it, so a vertex a
    /// little further from the cursor still beats a point that only slides along an edge.
    pub const fn weight(self) -> f64 {
        match self {
            // Asked for by name rather than found on the model, so it wins where it is offered.
            SnapKind::Origin => 0.35,
            SnapKind::Vertex => 0.4,
            SnapKind::Center => 0.5,
            SnapKind::Midpoint => 0.6,
            SnapKind::Quadrant => 0.75,
            SnapKind::FaceCenter => 0.85,
            SnapKind::OnEdge => 1.0,
            SnapKind::OnFace => 1.3,
        }
    }

    /// Which tier this kind is judged in: an exact place (0) always beats one that only slides
    /// along the geometry (1).
    ///
    /// A sliding place is the point of the edge or face nearest the cursor, so it sits at no
    /// distance at all and no weighting could demote it. It is the fallback: the caller offers
    /// only the candidates within its snap distance, and an exact place among them wins.
    pub const fn tier(self) -> u8 {
        if EXACT & self.bit() != 0 { 0 } else { 1 }
    }

    /// The id the `model.snap` command and the preferences use.
    pub const fn id(self) -> &'static str {
        match self {
            SnapKind::Origin => "origin",
            SnapKind::Vertex => "vertex",
            SnapKind::Center => "center",
            SnapKind::Midpoint => "midpoint",
            SnapKind::Quadrant => "quadrant",
            SnapKind::FaceCenter => "face_center",
            SnapKind::OnEdge => "on_edge",
            SnapKind::OnFace => "on_face",
        }
    }

    pub fn from_id(id: &str) -> Option<SnapKind> {
        EVERY.iter().copied().find(|k| k.id() == id)
    }

    /// The short label the viewport shows beside the cursor.
    pub const fn label(self) -> &'static str {
        match self {
            SnapKind::Origin => "Org",
            SnapKind::Vertex => "End",
            SnapKind::Center => "Ctr",
            SnapKind::Midpoint => "Mid",
            SnapKind::Quadrant => "Quad",
            SnapKind::FaceCenter => "Face",
            SnapKind::OnEdge => "On",
            SnapKind::OnFace => "",
        }
    }
}

/// Every kind, strongest first.
pub const EVERY: [SnapKind; 8] = [
    SnapKind::Origin,
    SnapKind::Vertex,
    SnapKind::Center,
    SnapKind::Midpoint,
    SnapKind::Quadrant,
    SnapKind::FaceCenter,
    SnapKind::OnEdge,
    SnapKind::OnFace,
];

/// The circle a circular edge lies on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Circle {
    pub center: Vec3,
    /// Unit normal of the circle's plane.
    pub axis: Vec3,
    pub radius: f64,
}

/// One place a pick could lock onto, with where it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct SnapCandidate {
    pub kind: SnapKind,
    pub at: Vec3,
    /// The mesh edge it came from.
    pub edge: Option<usize>,
    /// The B-rep face it came from.
    pub face: Option<u32>,
    /// The circle behind a centre or a quadrant.
    pub circle: Option<Circle>,
}

impl SnapCandidate {
    fn new(kind: SnapKind, at: Vec3) -> SnapCandidate {
        SnapCandidate { kind, at, edge: None, face: None, circle: None }
    }

    fn of_edge(mut self, edge: usize) -> SnapCandidate {
        self.edge = Some(edge);
        self
    }

    fn of_face(mut self, face: u32) -> SnapCandidate {
        self.face = Some(face);
        self
    }

    fn of_circle(mut self, c: Circle) -> SnapCandidate {
        self.circle = Some(c);
        self
    }
}

/// The point halfway along a polyline.
pub fn polyline_mid(pts: &[Vec3]) -> Vec3 {
    let len: f64 = pts.windows(2).map(|w| w[0].dist(w[1])).sum();
    let mut acc = 0.0;
    for w in pts.windows(2) {
        let l = w[0].dist(w[1]);
        if acc + l >= len * 0.5 && l > 0.0 {
            return w[0].lerp(w[1], (len * 0.5 - acc) / l);
        }
        acc += l;
    }
    pts.first().copied().unwrap_or_default()
}

/// A length below which two places on this polyline count as the same one.
pub fn polyline_tol(pts: &[Vec3]) -> f64 {
    let len: f64 = pts.windows(2).map(|w| w[0].dist(w[1])).sum();
    (len * 1e-3).max(1e-6)
}

/// Whether a polyline comes back to where it started.
pub fn closed_polyline(pts: &[Vec3]) -> bool {
    match (pts.first(), pts.last()) {
        (Some(a), Some(b)) => pts.len() > 2 && a.dist(*b) <= polyline_tol(pts),
        _ => false,
    }
}

/// The point of a polyline nearest `p`, and how far away it is.
pub fn nearest_on_polyline(pts: &[Vec3], p: Vec3) -> Option<(Vec3, f64)> {
    let mut best: Option<(Vec3, f64)> = None;
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let ab = b - a;
        let l2 = ab.len2();
        let t = if l2 > 0.0 { ((p - a).dot(ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
        let q = a + ab * t;
        let d = q.dist(p);
        if best.is_none_or(|(_, bd)| d < bd) {
            best = Some((q, d));
        }
    }
    best.or_else(|| pts.first().map(|q| (*q, q.dist(p))))
}

/// Centre, axis and radius of the circle a polyline lies on, if it lies on one.
pub fn circle_of(pts: &[Vec3]) -> Option<Circle> {
    let n = pts.len();
    if n < 4 {
        return None;
    }
    let (a, b, c) = (*pts.first()?, *pts.get(n / 3)?, *pts.get(2 * n / 3)?);
    let (ab, ac) = (b - a, c - a);
    let nn = ab.cross(ac);
    let l2 = nn.len2();
    if l2 < 1e-18 {
        return None;
    }
    let center = a + (nn.cross(ab) * ac.len2() + ac.cross(nn) * ab.len2()) / (2.0 * l2);
    let radius = center.dist(a);
    if !center.is_finite() || !radius.is_finite() || radius > 1e6 || radius <= 0.0 {
        return None;
    }
    let tol = (radius * 2e-3).max(1e-6);
    let axis = nn.normalized()?;
    let on = pts.iter().all(|p| (p.dist(center) - radius).abs() < tol && (*p - center).dot(axis).abs() < tol);
    on.then_some(Circle { center, axis, radius })
}

/// Angle wrapped into (-π, π].
fn wrap_pi(a: f64) -> f64 {
    let t = a.rem_euclid(std::f64::consts::TAU);
    if t > std::f64::consts::PI { t - std::f64::consts::TAU } else { t }
}

/// The quarter-turn points of a circle that lie on `pts`: an arc offers only those it passes.
///
/// Measured from a reference direction the circle itself fixes, so the places stay put wherever
/// the edge happens to start and whichever way it runs. Which quarter turns an arc reaches is
/// decided by the angle it sweeps: its polyline is a chord approximation whose middles cut
/// inside the true curve by far more than any tolerance worth having (a facet spanning
/// [`crate::mesh`]'s display angle sags by about 1.5% of the radius), while its points lie on
/// the curve exactly, so their angles are exact.
fn quadrants(c: &Circle, pts: &[Vec3]) -> Vec<Vec3> {
    let Some(u) = c.axis.any_perp().normalized() else {
        return Vec::new();
    };
    let v = c.axis.cross(u);
    let angle = |p: &Vec3| (*p - c.center).dot(v).atan2((*p - c.center).dot(u));
    let Some(start) = pts.first().map(angle) else {
        return Vec::new();
    };
    // How far round the edge runs, and which way: the wrapped steps add up over the whole
    // polyline, so a full circle sweeps a whole turn and an arc only its own span.
    let sweep: f64 = pts.windows(2).map(|w| wrap_pi(angle(&w[1]) - angle(&w[0]))).sum();
    let (span, dir) = (sweep.abs(), if sweep < 0.0 { -1.0 } else { 1.0 });
    (0..4)
        .map(|k| std::f64::consts::FRAC_PI_2 * f64::from(k))
        .filter(|q| (q - start).mul_add(dir, 0.0).rem_euclid(std::f64::consts::TAU) <= span + 1e-9)
        .map(|q| c.center + (u * q.cos() + v * q.sin()) * c.radius)
        .collect()
}

/// Area-weighted centre and unit normal of a planar face, when the centre lies on the face.
///
/// The centre of a ring or an L falls outside it, and a face that is not planar has none to
/// offer; the circular edge around a hole gives its axis instead (see [`SnapKind::Center`]).
pub fn planar_face_center(mesh: &Mesh, face: u32) -> Option<(Vec3, Vec3)> {
    let tris = || mesh.triangles.iter().zip(&mesh.tri_face).filter(move |(_, f)| **f == face).filter_map(|(t, _)| mesh.tri(t));
    let (mut normal, mut acc, mut area2, mut seen) = (None, Vec3::ZERO, 0.0, 0usize);
    for [a, b, c] in tris() {
        seen += 1;
        if seen > MAX_FACE_TRIANGLES {
            return None;
        }
        let n = (b - a).cross(c - a);
        let w = n.len();
        if w <= 0.0 {
            continue;
        }
        match normal {
            None => normal = n.normalized(),
            // Every triangle of a planar face faces the same way.
            Some(n0) if n0.dot(n) / w < 1.0 - PLANAR_COS => return None,
            Some(_) => {}
        }
        acc += (a + b + c) / 3.0 * w;
        area2 += w;
    }
    let normal = normal?;
    if area2 <= 0.0 {
        return None;
    }
    let center = acc / area2;
    let tol = (area2.sqrt() * 1e-3).max(1e-6);
    let on = tris().any(|[a, b, c]| crate::compare::closest_on_triangle(center, a, b, c).dist(center) <= tol);
    (on && center.is_finite()).then_some((center, normal))
}

/// What edge `edge` of the mesh offers a cursor at `near`, strongest first.
///
/// The centre and quadrants of a circular edge come whatever the distance: hovering the rim of a
/// hole is how its axis is picked.
pub fn edge_candidates(mesh: &Mesh, edge: usize, near: Vec3, kinds: SnapKinds) -> Vec<SnapCandidate> {
    let mut out: Vec<SnapCandidate> = Vec::new();
    let Some(pts) = mesh.edges.get(edge) else { return out };
    if pts.len() < 2 || !mesh.edge_pickable(edge) {
        return out;
    }
    let tol = polyline_tol(pts);
    // Offered strongest first, so a place already taken keeps the stronger kind.
    let push = |c: SnapCandidate, out: &mut Vec<SnapCandidate>| {
        if c.at.is_finite() && out.iter().all(|k| k.at.dist(c.at) > tol) {
            out.push(c);
        }
    };
    if kinds & VERTEX != 0 {
        for v in [pts.first(), pts.last()].into_iter().flatten() {
            push(SnapCandidate::new(SnapKind::Vertex, *v).of_edge(edge), &mut out);
        }
    }
    let circle = circle_of(pts);
    if let Some(c) = circle.filter(|_| kinds & CENTER != 0) {
        push(SnapCandidate::new(SnapKind::Center, c.center).of_edge(edge).of_circle(c), &mut out);
    }
    // A closed circle has no middle to snap to; the middle of an arc is halfway along it.
    if kinds & MIDPOINT != 0 && !closed_polyline(pts) {
        push(SnapCandidate::new(SnapKind::Midpoint, polyline_mid(pts)).of_edge(edge), &mut out);
    }
    if let Some(c) = circle.filter(|_| kinds & QUADRANT != 0) {
        for q in quadrants(&c, pts) {
            push(SnapCandidate::new(SnapKind::Quadrant, q).of_edge(edge).of_circle(c), &mut out);
        }
    }
    if kinds & ON_EDGE != 0
        && let Some((q, _)) = nearest_on_polyline(pts, near)
    {
        push(SnapCandidate::new(SnapKind::OnEdge, q).of_edge(edge), &mut out);
    }
    out
}

/// What face `face` offers for a point `at` on it.
pub fn face_candidates(mesh: &Mesh, face: u32, at: Vec3, kinds: SnapKinds) -> Vec<SnapCandidate> {
    let mut out = Vec::new();
    if kinds & FACE_CENTER != 0
        && let Some((c, _)) = planar_face_center(mesh, face)
    {
        out.push(SnapCandidate::new(SnapKind::FaceCenter, c).of_face(face));
    }
    if kinds & ON_FACE != 0 && at.is_finite() {
        out.push(SnapCandidate::new(SnapKind::OnFace, at).of_face(face));
    }
    out
}

/// Every candidate within `radius` of `near`, strongest first.
///
/// This is the proximity question: the centre of a hole comes back for a place near the centre,
/// not for one on the rim a radius away. [`edge_candidates`] answers the hover question instead.
pub fn candidates_near(mesh: &Mesh, near: Vec3, radius: f64, kinds: SnapKinds) -> Vec<SnapCandidate> {
    let mut out = Vec::new();
    if !near.is_finite() || !radius.is_finite() || radius <= 0.0 {
        return out;
    }
    for (i, pts) in mesh.edges.iter().enumerate().take(MAX_EDGES) {
        // Only an edge that comes near can offer a place within the radius; a circle whose
        // centre is in reach is kept too, since its own points may all lie further out.
        let near_edge = nearest_on_polyline(pts, near).is_some_and(|(_, d)| d <= radius);
        let near_center = circle_of(pts).is_some_and(|c| c.center.dist(near) <= radius);
        if near_edge || near_center {
            out.extend(edge_candidates(mesh, i, near, kinds));
        }
    }
    if kinds & (FACE_CENTER | ON_FACE) != 0 {
        let mut faces: Vec<u32> = Vec::new();
        for (t, f) in mesh.triangles.iter().zip(&mesh.tri_face) {
            if faces.len() >= MAX_NEAR_FACES {
                break;
            }
            if faces.contains(f) {
                continue;
            }
            if let Some([a, b, c]) = mesh.tri(t)
                && crate::compare::closest_on_triangle(near, a, b, c).dist(near) <= radius
            {
                faces.push(*f);
            }
        }
        for f in faces {
            let at = nearest_on_face(mesh, f, near).unwrap_or(near);
            out.extend(face_candidates(mesh, f, at, kinds));
        }
    }
    out.retain(|c| c.at.dist(near) <= radius);
    rank(&mut out, near, (radius * 1e-3).max(1e-9));
    out
}

/// The point of face `face` nearest `p`.
pub fn nearest_on_face(mesh: &Mesh, face: u32, p: Vec3) -> Option<Vec3> {
    mesh.triangles
        .iter()
        .zip(&mesh.tri_face)
        .filter(|(_, f)| **f == face)
        .filter_map(|(t, _)| mesh.tri(t))
        .map(|[a, b, c]| crate::compare::closest_on_triangle(p, a, b, c))
        .min_by(|a, b| a.dist(p).total_cmp(&b.dist(p)))
}

/// Order candidates by how strongly they pull at `near` (strongest first), drop those sitting
/// within `tol` of a stronger place, and cap the list at [`MAX_CANDIDATES`].
///
/// `near` and `tol` are in whatever the caller measures in: millimetres for the engine, screen
/// pixels for the viewport.
pub fn rank(cands: &mut Vec<SnapCandidate>, near: Vec3, tol: f64) {
    rank_by(cands, near, tol, |c| (c.kind, c.at));
}

/// [`rank`] for items that carry a candidate along with something else — which body it came
/// from, where it lands on screen — named by `key` as the kind and the place to judge it by.
pub fn rank_by<T>(items: &mut Vec<T>, near: Vec3, tol: f64, key: impl Fn(&T) -> (SnapKind, Vec3)) {
    // An exact place first (see [`SnapKind::tier`]), then the strongest pull, and right on the
    // cursor every weighted distance is zero, so the kind itself breaks that tie: a lighter
    // weight is a stronger place.
    items.sort_by(|a, b| {
        let ((ka, pa), (kb, pb)) = (key(a), key(b));
        ka.tier()
            .cmp(&kb.tier())
            .then((ka.weight() * pa.dist(near)).total_cmp(&(kb.weight() * pb.dist(near))))
            .then(ka.weight().total_cmp(&kb.weight()))
    });
    let mut kept: Vec<Vec3> = Vec::new();
    items.retain(|it| {
        let (_, p) = key(it);
        if tol > 0.0 && kept.iter().any(|k| k.dist(p) <= tol) {
            return false;
        }
        kept.push(p);
        true
    });
    items.truncate(MAX_CANDIDATES);
}

/// The kinds named by `ids`, or the first id that names none.
pub fn kinds_of<'a>(ids: impl IntoIterator<Item = &'a str>) -> Result<SnapKinds, &'a str> {
    let mut k = 0;
    for id in ids {
        k |= SnapKind::from_id(id).ok_or(id)?.bit();
    }
    Ok(k)
}

#[cfg(test)]
#[path = "snap_tests.rs"]
mod tests;
