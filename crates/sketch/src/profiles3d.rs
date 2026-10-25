//! Planar 3D profile detection (#741).
//!
//! With 3D Sketch on, drawn curves (wires) may leave the sketch plane. When such curves close a
//! loop that lies in a plane — possibly together with ordinary in-plane lines, which then lie
//! along the intersection of that plane with the sketch plane — the loop is a profile in its own
//! plane: a patch or extrude of it builds a tilted face (the sides of a pyramid, for one).
//!
//! The sketch's own 2D detection (`profiles`) only reads in-plane curves, so these loops take no
//! part in it. This module lifts the in-plane lines into space, adds the wire segments, finds the
//! planes the points span, and runs the ordinary 2D face tracing on the edges lying in each plane
//! (projected into it), so holes nest exactly as they do in the sketch plane. A profile bounded
//! entirely by in-plane curves is left out: it is already one of the 2D profiles.
//!
//! Wire points are taken in the sketch's component frame (the frame of the `plane` passed in);
//! linked World-space wires take no part.

use std::collections::{HashMap, HashSet};

use solidcraft_geom::{Plane, Region2, Vec2, Vec3};

use crate::model::{Curve, CurveKind, Sketch};
use crate::profiles::find_profiles;
use crate::wire::WireSpace;

/// A closed loop of sketch curves lying in a plane of its own, with the ids of the curves
/// bounding it. The region is in the plane's own coordinates (`plane.origin + u·plane.x +
/// v·plane.y`); the centroid is in the frame `plane` was given in.
#[derive(Clone, Debug)]
pub struct Profile3D {
    pub plane: Plane,
    pub region: Region2,
    /// Curve ids of the outer loop (a wire that contributes several segments is listed once).
    pub outer_curves: Vec<String>,
    /// Curve ids of each hole loop.
    pub hole_curves: Vec<Vec<String>>,
    pub area: f64,
    pub centroid: Vec3,
}

/// End points within this (mm) are one node; plane coincidence is tested to it too.
const MERGE_TOL: f64 = 1e-5;
/// Triple enumeration is O(n³): more nodes than this give up rather than stall evaluation.
const MAX_NODES: usize = 64;
/// Total lifted segments allowed.
const MAX_EDGES: usize = 2_000;
/// Distinct planes considered.
const MAX_PLANES: usize = 256;
/// A face smaller than this (mm²) is round-off, not a profile.
const MIN_AREA: f64 = 1e-9;

/// A lifted segment: node indices and the curve it came from.
struct Edge3 {
    a: usize,
    b: usize,
    id: String,
    from_wire: bool,
}

/// Node set with end points merged within `MERGE_TOL` (a uniform grid, one node per cell).
struct Nodes {
    pts: Vec<Vec3>,
    cells: HashMap<(i64, i64, i64), Vec<usize>>,
}

impl Nodes {
    fn new() -> Nodes {
        Nodes { pts: Vec::new(), cells: HashMap::new() }
    }
    fn cell(v: f64) -> i64 {
        (v / MERGE_TOL).floor() as i64
    }
    fn add(&mut self, p: Vec3) -> usize {
        let key = (Self::cell(p.x), Self::cell(p.y), Self::cell(p.z));
        for dx in [-1, 0, 1] {
            for dy in [-1, 0, 1] {
                for dz in [-1, 0, 1] {
                    if let Some(ids) = self.cells.get(&(key.0 + dx, key.1 + dy, key.2 + dz))
                        && let Some(&near) = ids.iter().find(|&&i| self.pts[i].is_finite() && p.dist(self.pts[i]) <= MERGE_TOL)
                    {
                        return near;
                    }
                }
            }
        }
        self.cells.entry(key).or_default().push(self.pts.len());
        self.pts.push(p);
        self.pts.len() - 1
    }
}

/// A candidate plane: unit normal, signed offset (`n·p` for its points), and one point on it.
struct Candidate {
    n: Vec3,
    off: f64,
    anchor: Vec3,
}

/// The plane with its normal's sign made canonical (the first dominant component positive), so
/// the same plane found from different triples compares equal.
fn canonical(n: Vec3, off: f64) -> (Vec3, f64) {
    let flip = n.z < -1e-12 || (n.z.abs() <= 1e-12 && (n.y < -1e-12 || (n.y.abs() <= 1e-12 && n.x < 0.0)));
    if flip { (-n, -off) } else { (n, off) }
}

/// Planar closed loops the 3D sketch curves take part in, in planes of their own.
///
/// `plane` is the sketch's plane in the frame the caller works in; drawn wires (`WireSpace::Component`)
/// are read in the same frame. Returns nothing unless the sketch has 3D Sketch on and wires to
/// work with, so it is cheap to call for every sketch on every evaluation.
pub fn find_planar_3d_profiles(sk: &Sketch, plane: Plane) -> Vec<Profile3D> {
    if !sk.view.three_d || sk.wires.is_empty() {
        return Vec::new();
    }
    // Lift the in-plane lines into space and add the wire segments. Only lines can lie on a
    // plane other than the sketch's own (curves in two planes lie on their intersection).
    let mut nodes = Nodes::new();
    let mut edges: Vec<Edge3> = Vec::new();
    let mut plane_line_ids: HashSet<String> = HashSet::new();
    for c in &sk.curves {
        if c.construction || c.centerline || c.infinite {
            continue;
        }
        if let CurveKind::Line { a, b } = c.kind
            && let (Some(u), Some(v)) = (sk.point(a), sk.point(b))
        {
            let lift = |p: Vec2| plane.origin + plane.x * p.x + plane.y * p.y;
            let (a, b) = (lift(u), lift(v));
            if a.dist(b) > MERGE_TOL && edges.len() < MAX_EDGES {
                edges.push(Edge3 { a: nodes.add(a), b: nodes.add(b), id: c.id.clone(), from_wire: false });
                plane_line_ids.insert(c.id.clone());
            }
        }
    }
    let mut wire_ids: HashSet<String> = HashSet::new();
    for w in &sk.wires {
        if w.space != WireSpace::Component {
            continue;
        }
        wire_ids.insert(w.id.clone());
        for pair in w.pts.windows(2) {
            if pair[0].dist(pair[1]) > MERGE_TOL && edges.len() < MAX_EDGES {
                edges.push(Edge3 { a: nodes.add(pair[0]), b: nodes.add(pair[1]), id: w.id.clone(), from_wire: true });
            }
        }
    }
    if edges.is_empty() || nodes.pts.len() < 3 || nodes.pts.len() > MAX_NODES {
        return Vec::new();
    }
    // The planes the nodes span, deduplicated. A plane needs at least three edges on it, one of
    // them from a wire, to bound anything this module should report. The sketch plane itself is a
    // candidate too (flat 3D loops take part there); profiles bounded only by in-plane curves are
    // dropped below, so nothing is listed twice.
    let mut candidates: Vec<Candidate> = Vec::new();
    for i in 0..nodes.pts.len() {
        for j in i + 1..nodes.pts.len() {
            for k in j + 1..nodes.pts.len() {
                let (a, b, c) = (nodes.pts[i], nodes.pts[j], nodes.pts[k]);
                let (ab, ac) = (b - a, c - a);
                let n = ab.cross(ac);
                let len = n.len();
                if len < 1e-12 || len < 1e-9 * ab.len().max(1e-300) * ac.len().max(1e-300) {
                    continue;
                }
                let (n, off) = canonical(n / len, n.dot(a) / len);
                let known = candidates.iter().any(|p| (p.n.dot(n) - 1.0).abs() < 1e-9 && (p.off - off).abs() < 1e-6);
                if known {
                    continue;
                }
                candidates.push(Candidate { n, off, anchor: a });
                if candidates.len() >= MAX_PLANES {
                    break;
                }
            }
            if candidates.len() >= MAX_PLANES {
                break;
            }
        }
        if candidates.len() >= MAX_PLANES {
            break;
        }
    }
    candidates.sort_by(|x, y| x.n.x.total_cmp(&y.n.x).then(x.n.y.total_cmp(&y.n.y)).then(x.n.z.total_cmp(&y.n.z)).then(x.off.total_cmp(&y.off)));

    let mut out: Vec<Profile3D> = Vec::new();
    let mut seen: HashSet<(i64, i64, i64, i64, String)> = HashSet::new();
    for cand in &candidates {
        let on = |p: Vec3| (cand.n.dot(p) - cand.off).abs() <= MERGE_TOL;
        let edges_on: Vec<&Edge3> = edges.iter().filter(|e| on(nodes.pts[e.a]) && on(nodes.pts[e.b])).collect();
        if edges_on.len() < 3 || !edges_on.iter().any(|e| e.from_wire) {
            continue;
        }
        // Project the plane's edges into it and let the ordinary 2D tracing close and nest them.
        let basis = Plane::from_normal(cand.anchor, cand.n);
        let Some(basis) = basis else { continue };
        let mut temp = Sketch::new();
        let mut at: HashMap<usize, usize> = HashMap::new();
        let mut bad = false;
        for e in &edges_on {
            for end in [e.a, e.b] {
                if at.contains_key(&end) {
                    continue;
                }
                let p = nodes.pts[end];
                let q = Vec2 { x: (p - basis.origin).dot(basis.x), y: (p - basis.origin).dot(basis.y) };
                match temp.add_point(q, None) {
                    Ok(i) => {
                        at.insert(end, i);
                    }
                    Err(_) => {
                        bad = true;
                        break;
                    }
                }
            }
            if bad {
                break;
            }
            let (Some(&a), Some(&b)) = (at.get(&e.a), at.get(&e.b)) else {
                bad = true;
                break;
            };
            if a == b {
                continue;
            }
            temp.curves.push(Curve {
                id: e.id.clone(),
                kind: CurveKind::Line { a, b },
                construction: false,
                reversed: false,
                link: None,
                centerline: false,
                fixed: false,
                infinite: false,
            });
        }
        if bad {
            continue;
        }
        for p in find_profiles(&temp) {
            if p.area < MIN_AREA {
                continue;
            }
            let outer = dedupe(&p.outer_curves);
            let holes: Vec<Vec<String>> = p.hole_curves.iter().map(|h| dedupe(h)).collect();
            // A loop bounded only by in-plane curves is already one of the sketch's 2D profiles.
            if outer.iter().chain(holes.iter().flatten()).all(|id| plane_line_ids.contains(id)) {
                continue;
            }
            let key = (
                (cand.n.x * 1e6).round() as i64,
                (cand.n.y * 1e6).round() as i64,
                (cand.n.z * 1e6).round() as i64,
                (cand.off * 1e4).round() as i64,
                {
                    let mut ids: Vec<&str> = outer.iter().map(String::as_str).chain(holes.iter().flatten().map(String::as_str)).collect();
                    ids.sort_unstable();
                    ids.join("|")
                },
            );
            if !seen.insert(key) {
                continue;
            }
            out.push(Profile3D {
                plane: basis,
                region: p.region,
                outer_curves: outer,
                hole_curves: holes,
                area: p.area,
                centroid: basis.origin + basis.x * p.centroid.x + basis.y * p.centroid.y,
            });
        }
    }
    out
}

/// The list with repeats dropped, order kept (a wire of several segments bounds one loop edge).
fn dedupe(ids: &[String]) -> Vec<String> {
    let mut seen: HashSet<&str> = HashSet::new();
    ids.iter().filter(|id| seen.insert(id.as_str())).cloned().collect()
}

#[cfg(test)]
#[path = "profiles3d_tests.rs"]
mod tests;
