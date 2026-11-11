//! Unfold for Printing: a solid of flat faces is laid on one of them and unfolded by hand into a
//! flat pattern that a 3D printer prints and its user folds back up. This module holds what the
//! first two steps decide, and the rules they follow: the face the body lies on (the base face),
//! and which of its edges fold (bends) and which are cut open.
//!
//! The faces and edges are read exactly, from the body's B-rep: every face must be flat, so every
//! edge is a straight line between two faces ([`UnfoldGraph`]). The pattern starts at the base
//! face. An edge set as a bend hangs the face across it on the face it was reached from, and
//! every other edge of that face is then a cut, until one of them is set as a bend in turn: the
//! bends make a tree from the base face, so no ring of bends closes ([`UnfoldLayout`]). A cut is
//! one edge of the body and an edge of each of its two faces in the pattern: the two are found
//! again as a pair by the cut's edge.
//!
//! Faces can be carved out of the pattern into a group: a part of its own that starts at the
//! group's first face. Every edge between a face of the group and a face outside it is a cut,
//! and inside the group the bends make a tree from its first face. A group folds at the inside
//! corners of the body only: its bends turn toward the outer skin, on the film alone.
//!
//! Faces, edges and the base face are kept as points on them in the body's coordinates, as the
//! other commands that pick edges keep them: the body is a copy that no feature changes.

use serde::{Deserialize, Serialize};
use solidcore_record::brep::{Curve, Loop};
use solidcore_record::{Loop2, Plane, Region2, Vec2, Vec3};
use solidcraft_geom::prelude::*;
use solidcraft_kernel::Body;

use crate::{DocError, Result};

/// The most faces a body unfolded for printing may have.
pub const MAX_UNFOLD_FACES: usize = 2000;

fn err<T>(msg: impl Into<String>) -> Result<T> {
    Err(DocError::Invalid(msg.into()))
}

const CURVED: &str = "not supported yet: unfolding a body with curved faces for printing (every face must be flat)";

/// Whether an edge's curve is a straight line: a line, or a polyline or spline whose points all
/// lie on the line through its first and last (such a curve lies on that line all along: a
/// chamfer's edges are splines of two points).
fn straight(curve: &Curve, tol: f64) -> bool {
    let on_line = |pts: &[Vec3]| {
        let (Some(a), Some(b)) = (pts.first(), pts.last()) else { return false };
        pts.len() >= 2 && a.dist(*b) > tol && pts.iter().all(|p| p.dist_to_segment(*a, *b) <= tol)
    };
    match curve {
        Curve::Line { .. } => true,
        Curve::Polyline { points } | Curve::Spline { points, .. } => on_line(&points.iter().map(|p| Vec3::new(p[0], p[1], p[2])).collect::<Vec<_>>()),
        Curve::RationalSpline { homogeneous, .. } => {
            homogeneous.iter().all(|h| h[3] > 1e-12)
                && on_line(&homogeneous.iter().map(|h| Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3])).collect::<Vec<_>>())
        }
        _ => false,
    }
}

/// How far the work on a design's unfolded print has come.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrintUnfoldStep {
    /// The bends and cuts are being set.
    #[default]
    Edges,
    /// Every face hangs on the pattern and the surface model is made.
    Surface,
    /// The surface model is laid flat, to check it against a print bed and split it into parts.
    Flat,
    /// The printable body is made, with its walls and joints.
    Solid,
}

/// The wall and the joints of the print: what every bend and cut gets, and the edges that get
/// something else.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrintJoints {
    /// The wall thickness (a length expression).
    pub wall: String,
    /// What a bend is: `film` (the film alone), `filament` or `press`.
    pub bends: String,
    /// How two walls meet at a cut: `butt`, `mitre`, `filament` or `press`.
    pub cuts: String,
    /// Edges with a joint of their own: a point on the edge (its middle on the body) and
    /// `plain` (a bend's film, a cut's butt or mitre), `filament` or `press`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<(Vec3, String)>,
}

impl Default for PrintJoints {
    fn default() -> Self {
        PrintJoints { wall: "2 mm".into(), bends: "film".into(), cuts: "butt".into(), edges: Vec::new() }
    }
}

/// The bed of the printer the pattern is checked against.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrintBed {
    /// The printer's name, as chosen (empty for a size typed in).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Its bed (mm).
    pub width: f64,
    pub depth: f64,
}

/// What a body measured when it was copied to be unfolded: a change of the body it was copied
/// from shows as a different stamp.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrintUnfoldStamp {
    pub faces: usize,
    pub edges: usize,
    pub volume: f64,
    pub area: f64,
    pub min: Vec3,
    pub max: Vec3,
}

impl PrintUnfoldStamp {
    /// Whether `other` is the same body's stamp, within what a recompute changes.
    pub fn same(&self, other: &PrintUnfoldStamp) -> bool {
        let size = (self.max - self.min).len().max(1e-9);
        let near = |a: f64, b: f64, scale: f64| (a - b).abs() <= 1e-7 * scale.abs().max(1e-9);
        self.faces == other.faces
            && self.edges == other.edges
            && near(self.volume, other.volume, self.volume.abs().max(size.powi(3)))
            && near(self.area, other.area, self.area.abs().max(size * size))
            && self.min.dist(other.min) <= 1e-7 * size
            && self.max.dist(other.max) <= 1e-7 * size
    }
}

/// Where the body being unfolded was copied from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrintUnfoldSource {
    /// The name of the design it was copied from.
    pub design: String,
    /// That design's file, when it had one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The body's name there.
    pub body: String,
    /// The point picked on its base face there (that design's world coordinates, mm).
    pub face: Vec3,
    /// What the body measured there when it was copied.
    pub stamp: PrintUnfoldStamp,
    /// That design while it is open in this run of the app (the engine session's `uid`); not
    /// saved, so 0 in a design opened from a file.
    #[serde(skip)]
    pub session: u64,
}

/// Faces carved out of the pattern into a part of their own: a point inside each face (the
/// body's coordinates, mm), the face the part starts at first.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrintGroup {
    pub faces: Vec<Vec3>,
}

/// A design's unfolded print: the body lying on its base face, and its bends.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrintUnfold {
    /// The body being unfolded (a copy, lying on its base face).
    pub body: String,
    /// A point on the base face (the body's coordinates, mm).
    pub base: Vec3,
    /// A point on each bend edge (its middle), in the order they were set.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bends: Vec<Vec3>,
    /// The groups of faces carved out of the pattern.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<PrintGroup>,
    #[serde(default)]
    pub step: PrintUnfoldStep,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<PrintUnfoldSource>,
    /// The feature that holds the surface model, once [`PrintUnfoldStep::Surface`] made it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<u64>,
    /// A point on each bend the flat pattern is split at (its middle on the body), each
    /// starting a part of its own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub splits: Vec<Vec3>,
    /// The print bed the flat pattern is checked against.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bed: Option<PrintBed>,
    /// The wall and joints chosen for the print.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub joints: Option<PrintJoints>,
    /// The feature that makes the printable body, once [`PrintUnfoldStep::Solid`] made it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solid: Option<u64>,
    /// The feature that holds the printable bodies of the carved-out groups, made with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_solids: Option<u64>,
}

/// How the two faces at an edge meet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EdgeTurn {
    /// Round the outside of the body: a bend there folds by this angle (radians, the angle
    /// between the faces' outward normals), toward the material.
    Outside(f64),
    /// In an inside corner of the body.
    Inside,
    /// In one plane.
    Flat,
}

/// A flat face of the body: its plane (the normal out of the material), its loops as corner
/// points (the outer one first), and its edges (indices into [`UnfoldGraph::edges`]).
#[derive(Clone, Debug)]
pub struct UnfoldFace {
    pub plane: Plane,
    pub loops: Vec<Vec<Vec3>>,
    pub edges: Vec<usize>,
}

impl UnfoldFace {
    pub fn normal(&self) -> Vec3 {
        self.plane.x.cross(self.plane.y)
    }

    fn local(&self, p: Vec3) -> Vec2 {
        let d = p - self.plane.origin;
        Vec2::new(d.dot(self.plane.x), d.dot(self.plane.y))
    }

    /// Whether `p` (a point in the face's plane) lies inside the face: inside its outer loop
    /// and outside its holes (even-odd over all its loops).
    fn contains(&self, p: Vec3) -> bool {
        let q = self.local(p);
        let mut inside = false;
        for lp in &self.loops {
            let n = lp.len();
            for i in 0..n {
                let (Some(a), Some(b)) = (lp.get(i), lp.get((i + 1) % n)) else { continue };
                let (a, b) = (self.local(*a), self.local(*b));
                if (a.y > q.y) != (b.y > q.y) && q.x < a.x + (q.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                    inside = !inside;
                }
            }
        }
        inside
    }

    /// How far `p` is from the face's boundary.
    fn boundary_distance(&self, p: Vec3) -> f64 {
        let mut best = f64::INFINITY;
        for lp in &self.loops {
            let n = lp.len();
            for i in 0..n {
                if let (Some(a), Some(b)) = (lp.get(i), lp.get((i + 1) % n)) {
                    best = best.min(p.dist_to_segment(*a, *b));
                }
            }
        }
        best
    }

    /// A point well inside the face: of the middles of its corners' triangles, the one farthest
    /// from its boundary.
    pub fn inner_point(&self) -> Option<Vec3> {
        let lp = self.loops.first()?;
        let n = lp.len();
        let mut best: Option<(Vec3, f64)> = None;
        for i in 0..n {
            for j in 1..n.saturating_sub(1) {
                let (Some(a), Some(b), Some(c)) = (lp.get(i), lp.get((i + j) % n), lp.get((i + j + 1) % n)) else { continue };
                let p = (*a + *b + *c) * (1.0 / 3.0);
                if !self.contains(p) {
                    continue;
                }
                let d = self.boundary_distance(p);
                if best.is_none_or(|(_, far)| d > far) {
                    best = Some((p, d));
                }
            }
        }
        best.map(|(p, _)| p)
    }

    /// The face as a region in its plane.
    pub fn region(&self) -> Region2 {
        let mut loops: Vec<(f64, Loop2)> = self
            .loops
            .iter()
            .map(|lp| {
                let pts: Vec<Vec2> = lp.iter().map(|p| self.local(*p)).collect();
                let n = pts.len();
                let area: f64 = (0..n).filter_map(|i| Some(pts.get(i)?.cross(*pts.get((i + 1) % n)?))).sum::<f64>() / 2.0;
                (area.abs(), Loop2::polygon(&pts))
            })
            .collect();
        loops.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut it = loops.into_iter().map(|(_, lp)| lp);
        Region2 { outer: it.next().unwrap_or_default(), holes: it.collect() }
    }
}

/// A straight edge of the body between two of its faces.
#[derive(Clone, Copy, Debug)]
pub struct UnfoldEdge {
    pub ends: (Vec3, Vec3),
    pub faces: (usize, usize),
    pub turn: EdgeTurn,
    /// Its index among the body's edges.
    pub index: usize,
}

impl UnfoldEdge {
    pub fn middle(&self) -> Vec3 {
        (self.ends.0 + self.ends.1) * 0.5
    }

    /// The face across the edge from `face`.
    pub fn across(&self, face: usize) -> usize {
        if self.faces.0 == face { self.faces.1 } else { self.faces.0 }
    }
}

/// The flat faces of a body and the edges between them, read from its B-rep.
#[derive(Clone, Debug)]
pub struct UnfoldGraph {
    pub faces: Vec<UnfoldFace>,
    pub edges: Vec<UnfoldEdge>,
    /// How near a picked point lies to what it picks (mm).
    pub near: f64,
}

/// What an edge is in the pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// The pattern folds here.
    Bend,
    /// The pattern is cut open here: an edge of each of its two faces.
    Cut,
    /// Neither of its faces hangs on the pattern yet.
    Open,
}

/// The pattern the bends make from the base face.
#[derive(Clone, Debug)]
pub struct UnfoldLayout {
    pub base: usize,
    /// Per face: the face it hangs on and the bend between them (none: the base face, or a
    /// face not on the pattern yet).
    pub parent: Vec<Option<(usize, usize)>>,
    /// Per face: whether it is on the pattern.
    pub attached: Vec<bool>,
    /// Per edge.
    pub kinds: Vec<EdgeKind>,
    /// Per face: the carved-out group it is in.
    pub group: Vec<Option<usize>>,
    /// The first face of each group: where its part starts.
    pub roots: Vec<usize>,
}

impl UnfoldLayout {
    /// Whether every face is on the pattern, so every edge is a bend or a cut.
    pub fn complete(&self) -> bool {
        self.attached.iter().all(|a| *a)
    }

    pub fn count(&self, kind: EdgeKind) -> usize {
        self.kinds.iter().filter(|k| **k == kind).count()
    }

    /// How many bends lie between `face` and the base face.
    fn depth(&self, face: usize) -> usize {
        let (mut at, mut depth) = (face, 0);
        while let Some(Some((parent, _))) = self.parent.get(at) {
            at = *parent;
            depth += 1;
            if depth > self.parent.len() {
                break;
            }
        }
        depth
    }

    /// Whether `face` hangs on `above`, directly or through other faces.
    fn below(&self, face: usize, above: usize) -> bool {
        let (mut at, mut steps) = (face, 0);
        while let Some(Some((parent, _))) = self.parent.get(at) {
            if *parent == above {
                return true;
            }
            at = *parent;
            steps += 1;
            if steps > self.parent.len() {
                break;
            }
        }
        false
    }
}

/// What setting or clearing a bend did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BendChange {
    /// The edge is a bend now and `face` hangs on the pattern by it.
    Added { face: usize },
    /// The edge is a bend now; `face` hung by another bend before, which is a cut again.
    Moved { face: usize },
    /// The edge is a cut again: these faces are off the pattern.
    Removed { faces: Vec<usize> },
}

impl UnfoldGraph {
    /// The faces and edges of `body`. Refused: a mesh, a body that is not one closed solid, and
    /// any face that is not flat.
    pub fn of(body: &Body) -> Result<UnfoldGraph> {
        if body.is_mesh() {
            return err("not supported yet: unfolding a mesh body for printing (convert it to a solid first)");
        }
        let rec = body.record();
        if rec.faces.is_empty() {
            return err("the body has no faces");
        }
        if rec.faces.len() > MAX_UNFOLD_FACES {
            return err(format!("the body has more than {MAX_UNFOLD_FACES} faces"));
        }
        if (0..rec.faces.len()).any(|i| body.planar_face(i).is_none()) {
            return err(CURVED);
        }
        let point = |v: u32| rec.vertices.get(v as usize).map(|v| Vec3::new(v.point[0], v.point[1], v.point[2]));
        let (mut lo, mut hi) =
            (Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY));
        for v in &rec.vertices {
            let p = Vec3::new(v.point[0], v.point[1], v.point[2]);
            lo = Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
            hi = Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
        }
        let size = (hi - lo).len();
        if !(size.is_finite() && size > 0.0) {
            return err("the body has no size");
        }
        // The edges between two faces, each by its index among the body's edges.
        let mut graph_edge: Vec<Option<usize>> = vec![None; rec.edges.len()];
        let mut edges: Vec<UnfoldEdge> = Vec::new();
        for (index, e) in rec.edges.iter().enumerate() {
            if e.uses.is_empty() {
                continue;
            }
            if !straight(&e.curve, size * 1e-9) {
                return err(CURVED);
            }
            let mut faces: Vec<usize> = e.uses.iter().map(|u| u.face as usize).collect();
            faces.sort_unstable();
            faces.dedup();
            let (&[a, b], 2) = (faces.as_slice(), e.uses.len()) else {
                return err("the body is not a closed solid: an edge of it does not lie between two faces");
            };
            let (Some(p), Some(q)) = (point(e.vertices[0]), point(e.vertices[1])) else {
                return err("the body's edges do not match its corners");
            };
            if let Some(slot) = graph_edge.get_mut(index) {
                *slot = Some(edges.len());
            }
            edges.push(UnfoldEdge { ends: (p, q), faces: (a, b), turn: EdgeTurn::Flat, index });
        }
        let mut faces = Vec::with_capacity(rec.faces.len());
        for (i, f) in rec.faces.iter().enumerate() {
            let Some(plane) = body.planar_face(i) else { return err(CURVED) };
            let (mut loops, mut face_edges) = (Vec::new(), Vec::new());
            for lp in &f.loops {
                let Loop::Edges { edges: run } = lp else { return err(CURVED) };
                let n = run.len();
                if n < 3 {
                    return err(CURVED);
                }
                // Each corner is where an edge of the loop meets the one before it.
                let mut pts = Vec::with_capacity(n);
                for k in 0..n {
                    let (Some(before), Some(here)) = (run.get((k + n - 1) % n), run.get(k)) else { continue };
                    let (Some(p), Some(q)) = (rec.edges.get(before.edge as usize), rec.edges.get(here.edge as usize)) else {
                        return err("the body's faces do not match its edges");
                    };
                    let Some(corner) = q.vertices.iter().find(|v| p.vertices.contains(v)).and_then(|v| point(*v)) else {
                        return err("the outline of a face of the body does not close");
                    };
                    pts.push(corner);
                    if let Some(Some(g)) = graph_edge.get(here.edge as usize) {
                        face_edges.push(*g);
                    }
                }
                loops.push(pts);
            }
            if loops.is_empty() {
                return err("a face of the body has no outline");
            }
            faces.push(UnfoldFace { plane, loops, edges: face_edges });
        }
        let mut graph = UnfoldGraph { faces, edges, near: (size * 2e-3).max(1e-3) };
        for i in 0..graph.edges.len() {
            let turn = graph.turn(i);
            if let Some(e) = graph.edges.get_mut(i) {
                e.turn = turn;
            }
        }
        // One piece: every face is reached from the first across edges.
        let mut seen = vec![false; graph.faces.len()];
        let mut todo = vec![0usize];
        while let Some(f) = todo.pop() {
            if std::mem::replace(seen.get_mut(f).unwrap_or(&mut true), true) {
                continue;
            }
            for e in graph.faces.get(f).map(|f| f.edges.as_slice()).unwrap_or_default() {
                if let Some(e) = graph.edges.get(*e) {
                    todo.push(e.across(f));
                }
            }
        }
        if seen.iter().any(|s| !*s) {
            return err("the body has more than one piece (or a hollow inside): unfold one solid piece at a time");
        }
        Ok(graph)
    }

    /// How the two faces at edge `e` meet: a little into one face from the edge's middle lies
    /// behind the other face's plane round the outside of the body, in front of it in an
    /// inside corner.
    fn turn(&self, e: usize) -> EdgeTurn {
        let Some(edge) = self.edges.get(e) else { return EdgeTurn::Flat };
        let (Some(a), Some(b)) = (self.faces.get(edge.faces.0), self.faces.get(edge.faces.1)) else { return EdgeTurn::Flat };
        let (na, nb) = (a.normal(), b.normal());
        let along = edge.ends.1 - edge.ends.0;
        let Some(d) = along.normalized() else { return EdgeTurn::Flat };
        let Some(side) = na.cross(d).normalized() else { return EdgeTurn::Flat };
        let step = (along.len() * 1e-4).max(1e-7);
        let middle = edge.middle();
        let into = match (a.contains(middle + side * step), a.contains(middle - side * step)) {
            (true, false) => side,
            (false, true) => -side,
            _ => return EdgeTurn::Flat,
        };
        let lean = into.dot(nb);
        if lean < -1e-9 {
            EdgeTurn::Outside(na.dot(nb).clamp(-1.0, 1.0).acos())
        } else if lean > 1e-9 {
            EdgeTurn::Inside
        } else {
            EdgeTurn::Flat
        }
    }

    /// The face `p` lies on.
    pub fn face_at(&self, p: Vec3) -> Option<usize> {
        let mut best: Option<(usize, f64)> = None;
        for (i, f) in self.faces.iter().enumerate() {
            let off = (p - f.plane.origin).dot(f.normal());
            if off.abs() > self.near {
                continue;
            }
            let on = p - f.normal() * off;
            if !(f.contains(on) || f.boundary_distance(on) <= self.near) {
                continue;
            }
            // Inside a face counts before the edge of another.
            let d = if f.contains(on) { off.abs() } else { off.abs() + f.boundary_distance(on) };
            if best.is_none_or(|(_, b)| d < b) {
                best = Some((i, d));
            }
        }
        best.map(|(i, _)| i)
    }

    /// The edge `p` lies on.
    pub fn edge_at(&self, p: Vec3) -> Option<usize> {
        self.edges
            .iter()
            .enumerate()
            .map(|(i, e)| (i, p.dist_to_segment(e.ends.0, e.ends.1)))
            .filter(|(_, d)| *d <= self.near)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// The pattern `bends` (edges) make from face `base`, and from the first face of each of
    /// `groups` (faces carved out into a part of their own). Refused: a bend that hangs on no
    /// face of the pattern, one that closes a ring, and one across the border of a group.
    pub fn layout(&self, base: usize, bends: &[usize], groups: &[Vec<usize>]) -> Result<UnfoldLayout> {
        if base >= self.faces.len() {
            return err("the base face is not a face of the body");
        }
        let mut attached = vec![false; self.faces.len()];
        let mut parent: Vec<Option<(usize, usize)>> = vec![None; self.faces.len()];
        if let Some(a) = attached.get_mut(base) {
            *a = true;
        }
        let mut group: Vec<Option<usize>> = vec![None; self.faces.len()];
        let mut roots = Vec::with_capacity(groups.len());
        for (k, faces) in groups.iter().enumerate() {
            for f in faces {
                match group.get_mut(*f) {
                    Some(slot) if slot.is_none() && *f != base => *slot = Some(k),
                    Some(_) => return err("a face is in two carved-out groups, or the base face is in one"),
                    None => return err("a carved-out face is not a face of the body"),
                }
            }
            let Some(first) = faces.first() else { return err("a carved-out group has no faces") };
            if let Some(a) = attached.get_mut(*first) {
                *a = true;
            }
            roots.push(*first);
        }
        if bends.iter().filter_map(|e| self.edges.get(*e)).any(|e| group.get(e.faces.0) != group.get(e.faces.1)) {
            return err("a bend lies on the border of a carved-out group: that edge is a cut");
        }
        let mut rest: Vec<usize> = bends.to_vec();
        loop {
            let before = rest.len();
            let mut later = Vec::new();
            for e in rest {
                let Some(edge) = self.edges.get(e) else { return err("a bend is not an edge of the body") };
                let on = |f: usize| attached.get(f).copied().unwrap_or(false);
                match (on(edge.faces.0), on(edge.faces.1)) {
                    (true, true) => return err("the bends close a ring: one of its edges must stay a cut"),
                    (false, false) => later.push(e),
                    (first, _) => {
                        let (from, to) = if first { edge.faces } else { (edge.faces.1, edge.faces.0) };
                        if let (Some(a), Some(p)) = (attached.get_mut(to), parent.get_mut(to)) {
                            *a = true;
                            *p = Some((from, e));
                        }
                    }
                }
            }
            rest = later;
            if rest.is_empty() {
                break;
            }
            if rest.len() == before {
                return err("a bend hangs on no face of the pattern");
            }
        }
        let kinds = self
            .edges
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let on = |f: usize| attached.get(f).copied().unwrap_or(false);
                if bends.contains(&i) {
                    EdgeKind::Bend
                } else if on(e.faces.0) || on(e.faces.1) {
                    EdgeKind::Cut
                } else {
                    EdgeKind::Open
                }
            })
            .collect();
        Ok(UnfoldLayout { base, parent, attached, kinds, group, roots })
    }

    /// Click edge `e`: a cut becomes a bend and hangs the face across it on the pattern (a face
    /// that hung by another bend moves to this one, with what hangs on it); a bend becomes a
    /// cut again, and what hung by it is off the pattern.
    pub fn toggle_bend(&self, base: usize, bends: &mut Vec<usize>, groups: &[Vec<usize>], e: usize) -> Result<BendChange> {
        let layout = self.layout(base, bends, groups)?;
        let Some(edge) = self.edges.get(e) else { return err("that is not an edge of the body") };
        if let Some(at) = bends.iter().position(|b| *b == e) {
            // The face that hung by it, and everything hanging on that face.
            let child = if layout.parent.get(edge.faces.0).copied().flatten().is_some_and(|(_, by)| by == e) { edge.faces.0 } else { edge.faces.1 };
            let off: Vec<usize> = (0..self.faces.len()).filter(|f| *f == child || layout.below(*f, child)).collect();
            bends.remove(at);
            bends.retain(|b| !layout.parent.iter().enumerate().any(|(f, p)| off.contains(&f) && p.is_some_and(|(_, by)| by == *b)));
            return Ok(BendChange::Removed { faces: off });
        }
        let (ga, gb) = (layout.group.get(edge.faces.0).copied().flatten(), layout.group.get(edge.faces.1).copied().flatten());
        if ga != gb {
            return err("this edge is the border of a carved-out group: it stays a cut");
        }
        match (edge.turn, ga) {
            (EdgeTurn::Outside(_), None) | (EdgeTurn::Inside, Some(_)) => {}
            (EdgeTurn::Outside(_), Some(_)) => {
                return err("not supported yet: a carved-out group folds at inside corners of the body only (this edge stays a cut)");
            }
            (EdgeTurn::Inside, None) => {
                return err(
                    "this edge can't fold here: its faces meet in an inside corner of the body. Carve its faces out into a group first; a group folds at inside corners",
                );
            }
            (EdgeTurn::Flat, _) => return err("this edge can't fold: its two faces lie in one plane"),
        }
        let on = |f: usize| layout.attached.get(f).copied().unwrap_or(false);
        match (on(edge.faces.0), on(edge.faces.1)) {
            (false, false) => {
                err("neither face at this edge is on the pattern yet: set the bends outward from the base face, one face after the other")
            }
            (true, true) => {
                // The face that moves: the one hanging on the other, else the one farther from
                // the base face (of two as far, the one whose bend was set later).
                let (a, b) = edge.faces;
                let set_at = |f: usize| layout.parent.get(f).copied().flatten().and_then(|(_, by)| bends.iter().position(|x| *x == by));
                let moves = if layout.below(b, a) {
                    b
                } else if layout.below(a, b) {
                    a
                } else {
                    match layout.depth(a).cmp(&layout.depth(b)) {
                        std::cmp::Ordering::Greater => a,
                        std::cmp::Ordering::Less => b,
                        std::cmp::Ordering::Equal => {
                            if set_at(a) > set_at(b) {
                                a
                            } else {
                                b
                            }
                        }
                    }
                };
                let Some((_, old)) = layout.parent.get(moves).copied().flatten() else {
                    return err("this edge would close a ring of bends round the face a part starts at: one edge of a ring stays a cut");
                };
                bends.retain(|x| *x != old);
                bends.push(e);
                Ok(BendChange::Moved { face: moves })
            }
            (first, _) => {
                bends.push(e);
                Ok(BendChange::Added { face: if first { edge.faces.1 } else { edge.faces.0 } })
            }
        }
    }

    /// Every face as a flat sheet of its own, where it lies on the body: the surface model.
    pub fn patches(&self) -> Result<Vec<Body>> {
        self.faces.iter().map(|f| Ok(solidcraft_kernel::patch_region(&f.plane, &f.region())?)).collect()
    }
}

impl PrintUnfold {
    /// The base face and the bends as faces and edges of `graph`.
    pub fn resolve(&self, graph: &UnfoldGraph) -> Result<(usize, Vec<usize>)> {
        let Some(base) = graph.face_at(self.base) else { return err("the base face of the unfolded print is no longer a face of its body") };
        let mut bends = Vec::with_capacity(self.bends.len());
        for p in &self.bends {
            let Some(e) = graph.edge_at(*p) else { return err("a bend of the unfolded print is no longer an edge of its body") };
            if !bends.contains(&e) {
                bends.push(e);
            }
        }
        Ok((base, bends))
    }

    /// The carved-out groups as faces of `graph`.
    pub fn resolve_groups(&self, graph: &UnfoldGraph) -> Result<Vec<Vec<usize>>> {
        let mut out = Vec::with_capacity(self.groups.len());
        for g in &self.groups {
            let mut faces = Vec::with_capacity(g.faces.len());
            for p in &g.faces {
                let Some(f) = graph.face_at(*p) else { return err("a carved-out face of the unfolded print is no longer a face of its body") };
                if !faces.contains(&f) {
                    faces.push(f);
                }
            }
            out.push(faces);
        }
        Ok(out)
    }

    /// The bends the flat pattern is split at, as edges of `graph`: those of `bends` only.
    pub fn resolve_splits(&self, graph: &UnfoldGraph, bends: &[usize]) -> Vec<usize> {
        let mut out = Vec::new();
        for p in &self.splits {
            if let Some(e) = graph.edge_at(*p).filter(|e| bends.contains(e) && !out.contains(e)) {
                out.push(e);
            }
        }
        out
    }
}
