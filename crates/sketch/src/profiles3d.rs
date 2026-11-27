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

use std::collections::{BTreeMap, HashMap, HashSet};

use solidcore_record::{Plane, Region2, Vec2, Vec3};
use solidcraft_geom::prelude::*;

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
/// Total lifted segments allowed: the most the 2D tracing run on each plane's edges takes, so a
/// long loop is never cut short here and left with a gap (#19464).
const MAX_EDGES: usize = crate::profiles::MAX_PROFILE_EDGES;
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
                let (a, b) = (nodes.add(a), nodes.add(b));
                edges.push(Edge3 { a, b, id: c.id.clone(), from_wire: false });
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
                let (a, b) = (nodes.add(pair[0]), nodes.add(pair[1]));
                edges.push(Edge3 { a, b, id: w.id.clone(), from_wire: true });
            }
        }
    }
    if edges.is_empty() {
        return Vec::new();
    }
    // The planes the nodes span, deduplicated. A plane needs at least three edges on it, one of
    // them from a wire, to bound anything this module should report. The sketch plane itself is a
    // candidate too (flat 3D loops take part there); profiles bounded only by in-plane curves are
    // dropped below, so nothing is listed twice.
    let mut candidates: Vec<Candidate> = Vec::new();
    add_cycle_planes(&edges, &nodes.pts, &mut candidates);
    if candidates.is_empty() {
        return Vec::new();
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
                spline_handle_of: None,
                spline_control_polygon_of: None,
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

/// Union-find root with path halving; an index outside `parent` is its own root.
fn find_root(parent: &mut [usize], mut i: usize) -> usize {
    while let Some(&p) = parent.get(i) {
        if p == i {
            break;
        }
        let gp = parent.get(p).copied().unwrap_or(p);
        if let Some(slot) = parent.get_mut(i) {
            *slot = gp;
        }
        i = gp;
    }
    i
}

/// Adds the plane through three points if it is new and there is room.
fn push_plane(a: Vec3, b: Vec3, c: Vec3, candidates: &mut Vec<Candidate>) {
    if candidates.len() >= MAX_PLANES {
        return;
    }
    let (ab, ac) = (b - a, c - a);
    let n = ab.cross(ac);
    let len = n.len();
    if len < 1e-12 || len < 1e-9 * ab.len().max(1e-300) * ac.len().max(1e-300) {
        return;
    }
    let (n, off) = canonical(n / len, n.dot(a) / len);
    if candidates.iter().any(|p| (p.n.dot(n) - 1.0).abs() < 1e-9 && (p.off - off).abs() < 1e-6) {
        return;
    }
    candidates.push(Candidate { n, off, anchor: a });
}

/// The plane through the first point, the one farthest from it, and the one farthest from their
/// line: the plane a planar point set lies in, found from the points themselves.
fn add_spanning_plane(ids: &[usize], pts: &[Vec3], candidates: &mut Vec<Candidate>) {
    let p = |i: usize| pts.get(i).copied();
    let Some(a) = ids.first().and_then(|&i| p(i)) else { return };
    let dist = |i: usize, f: &dyn Fn(Vec3) -> f64| p(i).map_or(0.0, f);
    let Some(b) = ids.iter().copied().max_by(|&i, &j| dist(i, &|q| q.dist(a)).total_cmp(&dist(j, &|q| q.dist(a)))).and_then(p) else { return };
    let chord = b - a;
    let Some(c) =
        ids.iter().copied().max_by(|&i, &j| dist(i, &|q| chord.cross(q - a).len()).total_cmp(&dist(j, &|q| chord.cross(q - a).len()))).and_then(p)
    else {
        return;
    };
    push_plane(a, b, c, candidates);
}

/// Candidate planes from the graph's cycles. A closed loop only uses edges that lie on a cycle,
/// so edges that do not (a wire hanging off the loop, a branch leading elsewhere) are dropped
/// first: they cannot bound anything, and however many there are they cannot crowd a loop's plane
/// out. Each remaining connected piece gives the plane its points span (a planar loop of any
/// length lies wholly in it), and each corner gives the plane through it and its two neighbours
/// (a loop lies in the plane of any of its corners, and a plane holding only part of a wire, one
/// tooth of a zigzag, is found the same way). Nothing is thinned, so the work is linear in the
/// edges plus the pairs of cycle edges meeting at a node.
fn add_cycle_planes(edges: &[Edge3], pts: &[Vec3], candidates: &mut Vec<Candidate>) {
    let n = pts.len();
    let mut adj: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n];
    for (i, e) in edges.iter().enumerate() {
        if e.a == e.b {
            continue;
        }
        if let Some(l) = adj.get_mut(e.a) {
            l.push((e.b, i));
        }
        if let Some(l) = adj.get_mut(e.b) {
            l.push((e.a, i));
        }
    }
    let bridge = bridges(&adj, edges.len());
    // Neighbours over cycle edges only, and the pieces they connect.
    let mut near: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut parent: Vec<usize> = (0..n).collect();
    for (i, e) in edges.iter().enumerate() {
        if e.a == e.b || bridge.get(i).copied().unwrap_or(true) {
            continue;
        }
        if let Some(l) = near.get_mut(e.a) {
            l.push(e.b);
        }
        if let Some(l) = near.get_mut(e.b) {
            l.push(e.a);
        }
        let (ra, rb) = (find_root(&mut parent, e.a), find_root(&mut parent, e.b));
        if let Some(slot) = parent.get_mut(ra) {
            *slot = rb;
        }
    }
    let mut pieces: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for v in 0..n {
        if near.get(v).is_some_and(|l| !l.is_empty()) {
            pieces.entry(find_root(&mut parent, v)).or_default().push(v);
        }
    }
    for ids in pieces.values() {
        add_spanning_plane(ids, pts, candidates);
    }
    for (v, l) in near.iter_mut().enumerate() {
        l.sort_unstable();
        l.dedup();
        let Some(&c) = pts.get(v) else { continue };
        for (i, &x) in l.iter().enumerate() {
            for &y in l.iter().skip(i + 1) {
                if candidates.len() >= MAX_PLANES {
                    return;
                }
                if let (Some(&a), Some(&b)) = (pts.get(x), pts.get(y)) {
                    // A nearly straight corner gives a plane that is mostly round-off.
                    let (u, w) = (a - c, b - c);
                    if u.cross(w).len() > 1e-4 * u.len() * w.len() {
                        push_plane(c, a, b, candidates);
                    }
                }
            }
        }
    }
}

/// Which edges are bridges (on no cycle): an iterative depth-first search with low-links, so a
/// long chain cannot overflow the stack. Parallel edges are told apart by their index.
fn bridges(adj: &[Vec<(usize, usize)>], edge_count: usize) -> Vec<bool> {
    let n = adj.len();
    let mut bridge = vec![false; edge_count];
    let mut disc = vec![usize::MAX; n];
    let mut low = vec![0usize; n];
    let mut clock = 0usize;
    for root in 0..n {
        if disc.get(root).copied() != Some(usize::MAX) {
            continue;
        }
        // (node, edge it was reached by, next neighbour to look at)
        let mut stack: Vec<(usize, usize, usize)> = vec![(root, usize::MAX, 0)];
        if let Some(d) = disc.get_mut(root) {
            *d = clock;
        }
        if let Some(l) = low.get_mut(root) {
            *l = clock;
        }
        clock += 1;
        while let Some(&(v, via, k)) = stack.last() {
            if let Some(&(w, ei)) = adj.get(v).and_then(|l| l.get(k)) {
                if let Some(top) = stack.last_mut() {
                    top.2 += 1;
                }
                if ei == via {
                    continue;
                }
                match disc.get(w).copied() {
                    Some(usize::MAX) => {
                        if let Some(d) = disc.get_mut(w) {
                            *d = clock;
                        }
                        if let Some(l) = low.get_mut(w) {
                            *l = clock;
                        }
                        clock += 1;
                        stack.push((w, ei, 0));
                    }
                    Some(dw) => {
                        if let Some(l) = low.get_mut(v) {
                            *l = (*l).min(dw);
                        }
                    }
                    None => {}
                }
            } else {
                stack.pop();
                if let Some(&(p, _, _)) = stack.last() {
                    let lv = low.get(v).copied().unwrap_or(0);
                    if let Some(l) = low.get_mut(p) {
                        *l = (*l).min(lv);
                    }
                    if lv > disc.get(p).copied().unwrap_or(0)
                        && let Some(b) = bridge.get_mut(via)
                    {
                        *b = true;
                    }
                }
            }
        }
    }
    bridge
}

/// The list with repeats dropped, order kept (a wire of several segments bounds one loop edge).
fn dedupe(ids: &[String]) -> Vec<String> {
    let mut seen: HashSet<&str> = HashSet::new();
    ids.iter().filter(|id| seen.insert(id.as_str())).cloned().collect()
}

#[cfg(test)]
#[path = "profiles3d_tests.rs"]
mod tests;
