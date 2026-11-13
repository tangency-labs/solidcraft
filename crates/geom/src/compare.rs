//! Comparing two closed triangle meshes without a boolean of the two: how far each surface strays
//! from the other, and the places where only one of them has material.
//!
//! Each surface is sampled: the corners of its triangles, and inside the larger ones the corners
//! and centres of pieces at most `spacing` across. Every sample measures its distance to the other
//! surface (a [`TriangleTree`] over the other's triangles). Samples further away than the
//! tolerance are the deviating surface. One that lies outside the other mesh bounds material
//! only its own mesh has; one inside it bounds material only the other mesh has. The deviating
//! samples are clustered by distance into regions, and each region's volume is integrated along
//! a grid of parallel lines through its box: on each line, the stretches inside one mesh and
//! outside the other, found from where the line crosses the two surfaces. Nothing here
//! intersects the two surfaces, so bodies that share almost every face (two versions of one
//! part) compare as well as any others.

use crate::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use crate::{Aabb3, Mesh};
use solidcore_record::Vec3;

/// Runs a job for every index below `n`: the jobs are independent, so any order and any number
/// of threads will do. [`serial`] runs them one after another; a caller with threads passes one
/// that spreads them. A job that did not run (it panicked on another thread) runs again on the
/// calling thread.
pub type Each<'a> = &'a (dyn Fn(usize, &(dyn Fn(usize) + Sync)) + Sync);

/// [`Each`] on the calling thread.
pub fn serial(n: usize, job: &(dyn Fn(usize) + Sync)) {
    (0..n).for_each(job);
}

/// How [`compare`] samples and measures.
#[derive(Clone, Copy, Debug)]
pub struct CompareOptions {
    /// Distances up to this are agreement (mm).
    pub tolerance: f64,
    /// The largest size of the pieces a triangle is sampled in (mm); raised when a mesh would
    /// need more than `max_points` samples.
    pub spacing: f64,
    /// Sample points per mesh, about (a mesh of more triangles than two thirds of this is
    /// sampled once per triangle, at its centre and corners, anyway).
    pub max_points: usize,
    /// Lines through a region for its volume, at most.
    pub max_lines: usize,
    /// Regions measured and listed, largest first; the rest are counted in
    /// [`Comparison::unlisted`].
    pub max_regions: usize,
}

impl Default for CompareOptions {
    fn default() -> Self {
        CompareOptions { tolerance: 0.01, spacing: 0.5, max_points: 8_000_000, max_lines: 40_000, max_regions: 50 }
    }
}

/// Which mesh has the material of a region.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// Material of `a` where `b` has none.
    OnlyInA,
    /// Material of `b` where `a` has none.
    OnlyInB,
}

/// How far one mesh's surface lies from the other's, over its samples.
#[derive(Clone, Copy, Debug, Default)]
pub struct Deviation {
    /// The largest distance (mm) and the sample where it was found.
    pub max: f64,
    pub at: Vec3,
    /// The mean distance, weighted by the surface each sample stands for (mm).
    pub mean: f64,
    /// The surface further than the tolerance from the other mesh, and all of it (mm²).
    pub beyond_area: f64,
    pub area: f64,
    /// Sample points measured.
    pub points: usize,
}

/// The lines a region's volume was integrated along.
#[derive(Clone, Copy, Debug, Default)]
pub struct Lines {
    /// The lines run along this axis (0 = X, 1 = Y, 2 = Z): the region box's shortest.
    pub axis: usize,
    /// Distance between neighbouring lines across the axis, along the next two axes in turn
    /// (for Z: X, then Y) (mm).
    pub spacing: [f64; 2],
    pub count: usize,
    /// Lines left out because they crossed a mesh an odd number of times (a gap in it).
    pub skipped: usize,
}

/// A place where only one mesh has material.
#[derive(Clone, Debug)]
pub struct Region {
    pub side: Side,
    /// The volume of the material only one mesh has here, integrated along [`Region::lines`]
    /// (mm³; NaN when it would take too long to measure).
    pub volume: f64,
    /// That volume's centre and box; the deviating surface's when the lines found none.
    pub center: Vec3,
    pub bbox: Aabb3,
    /// The deviating surface of each mesh here (mm²).
    pub area_a: f64,
    pub area_b: f64,
    /// The largest distance between the surfaces here (mm), and where.
    pub max_deviation: f64,
    pub at: Vec3,
    /// The faces (the meshes' `tri_face` ids) the deviating surface lies on, each with its
    /// deviating area here (mm²); [`Comparison::face_area`] has each face's whole area.
    pub faces_a: Vec<(u32, f64)>,
    pub faces_b: Vec<(u32, f64)>,
    pub lines: Lines,
}

/// What [`compare`] found.
#[derive(Clone, Debug, Default)]
pub struct Comparison {
    pub a_to_b: Deviation,
    pub b_to_a: Deviation,
    /// Largest volume first.
    pub regions: Vec<Region>,
    /// Regions found beyond [`CompareOptions::max_regions`] (not measured).
    pub unlisted: usize,
    /// The sample spacing used (mm).
    pub spacing: f64,
    /// The sampled area of each face of `a` and of `b`, by `tri_face` id (mm²).
    pub face_area: [HashMap<u32, f64>; 2],
}

/// The point of triangle `abc` nearest `p` (Ericson, Real-Time Collision Detection, 5.1.5).
pub fn closest_on_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let den = d1 - d3;
        return if den > 0.0 { a + ab * (d1 / den) } else { a };
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let den = d2 - d6;
        return if den > 0.0 { a + ac * (d2 / den) } else { a };
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let den = (d4 - d3) + (d5 - d6);
        return if den > 0.0 { b + (c - b) * ((d4 - d3) / den) } else { b };
    }
    let den = va + vb + vc;
    if den.abs() < 1e-300 {
        return a;
    }
    a + ab * (vb / den) + ac * (vc / den)
}

fn comp(v: Vec3, k: usize) -> f64 {
    match k {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    }
}

/// The point with coordinate `ck` on axis `k`, and `cu`, `cv` on the next two axes in turn.
fn from_axes(k: usize, cu: f64, cv: f64, ck: f64) -> Vec3 {
    match k {
        0 => Vec3::new(ck, cu, cv),
        1 => Vec3::new(cv, ck, cu),
        _ => Vec3::new(cu, cv, ck),
    }
}

fn box_of(t: &[Vec3; 3]) -> Aabb3 {
    let mut b = Aabb3::EMPTY;
    t.iter().for_each(|p| b.add(*p));
    b
}

/// Squared distance from `p` to the box (0 inside it).
fn box_dist2(b: &Aabb3, p: Vec3) -> f64 {
    (b.min - p).max(p - b.max).max(Vec3::ZERO).len2()
}

/// Do the boxes overlap on every axis but `skip` (none when `skip > 2`)?
fn boxes_meet(a: &Aabb3, b: &Aabb3, skip: usize) -> bool {
    (0..3).filter(|&k| k != skip).all(|k| comp(a.min, k) <= comp(b.max, k) && comp(b.min, k) <= comp(a.max, k))
}

/// Triangles in a leaf of [`TriangleTree`], at most (unless they all share a centre).
const LEAF: usize = 4;

/// A bounding-volume hierarchy over a mesh's triangles: the nearest point of the surface, and
/// the triangles a line can cross.
pub struct TriangleTree {
    /// The mesh's triangles with finite corners, in tree order.
    tris: Vec<[Vec3; 3]>,
    /// Each one's index in the mesh.
    ids: Vec<u32>,
    nodes: Vec<Node>,
}

#[derive(Clone, Copy)]
struct Node {
    bbox: Aabb3,
    /// A leaf holds `tris[a..b]`; an inner node has the children `a` and `b`.
    a: u32,
    b: u32,
    leaf: bool,
}

impl TriangleTree {
    pub fn new(mesh: &Mesh) -> TriangleTree {
        let mut tris = Vec::with_capacity(mesh.triangles.len());
        let mut ids = Vec::with_capacity(mesh.triangles.len());
        for (i, t) in mesh.triangles.iter().enumerate() {
            if let (Some(tri), Ok(i)) = (mesh.tri(t), u32::try_from(i))
                && tri.iter().all(|p| p.is_finite())
            {
                tris.push(tri);
                ids.push(i);
            }
        }
        let centres: Vec<Vec3> = tris.iter().map(|t| (t[0] + t[1] + t[2]) / 3.0).collect();
        let mut order: Vec<u32> = (0..u32::try_from(tris.len()).unwrap_or(u32::MAX)).collect();
        let n = order.len();
        let empty = Node { bbox: Aabb3::EMPTY, a: 0, b: 0, leaf: true };
        let mut nodes = vec![empty];
        let mut work = vec![(0usize, 0usize, n)];
        while let Some((ni, lo, hi)) = work.pop() {
            let Some(range) = order.get_mut(lo..hi) else { continue };
            let (mut bbox, mut cb) = (Aabb3::EMPTY, Aabb3::EMPTY);
            for &i in range.iter() {
                if let (Some(t), Some(c)) = (tris.get(i as usize), centres.get(i as usize)) {
                    bbox = bbox.union(&box_of(t));
                    cb.add(*c);
                }
            }
            let size = cb.size();
            let axis = if size.x >= size.y && size.x >= size.z {
                0
            } else if size.y >= size.z {
                1
            } else {
                2
            };
            let node = if hi - lo <= LEAF || comp(size, axis) <= 0.0 {
                Node { bbox, a: lo as u32, b: hi as u32, leaf: true }
            } else {
                let mid = (hi - lo) / 2;
                range.select_nth_unstable_by(mid, |x, y| {
                    let c = |i: &u32| centres.get(*i as usize).map_or(0.0, |c| comp(*c, axis));
                    c(x).total_cmp(&c(y))
                });
                let (left, right) = (nodes.len(), nodes.len() + 1);
                nodes.push(empty);
                nodes.push(empty);
                work.push((left, lo, lo + mid));
                work.push((right, lo + mid, hi));
                Node { bbox, a: left as u32, b: right as u32, leaf: false }
            };
            if let Some(slot) = nodes.get_mut(ni) {
                *slot = node;
            }
        }
        let tris_in_order = order.iter().filter_map(|&i| tris.get(i as usize).copied()).collect();
        let ids_in_order = order.iter().filter_map(|&i| ids.get(i as usize).copied()).collect();
        TriangleTree { tris: tris_in_order, ids: ids_in_order, nodes }
    }

    pub fn len(&self) -> usize {
        self.tris.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    /// The distance from `p` to the nearest point of the surface, and the index in the mesh of
    /// the triangle it lies on (`None` for an empty mesh or a point that isn't finite).
    pub fn nearest(&self, p: Vec3) -> Option<(f64, u32)> {
        if !p.is_finite() || self.tris.is_empty() {
            return None;
        }
        let (mut best2, mut hit) = (f64::INFINITY, None);
        let mut stack: Vec<u32> = Vec::with_capacity(64);
        stack.push(0);
        while let Some(ni) = stack.pop() {
            let Some(node) = self.nodes.get(ni as usize) else { continue };
            if box_dist2(&node.bbox, p) >= best2 {
                continue;
            }
            if node.leaf {
                for k in node.a..node.b {
                    let Some([a, b, c]) = self.tris.get(k as usize).copied() else { continue };
                    let d2 = (closest_on_triangle(p, a, b, c) - p).len2();
                    if d2 < best2 {
                        best2 = d2;
                        hit = self.ids.get(k as usize).copied();
                    }
                }
                continue;
            }
            let d = |i: u32| self.nodes.get(i as usize).map_or(f64::INFINITY, |n| box_dist2(&n.bbox, p));
            let (da, db) = (d(node.a), d(node.b));
            // The nearer child is searched first (pushed last).
            let (near, far, dn, df) = if da <= db { (node.a, node.b, da, db) } else { (node.b, node.a, db, da) };
            if df < best2 {
                stack.push(far);
            }
            if dn < best2 {
                stack.push(near);
            }
        }
        hit.map(|i| (best2.sqrt(), i))
    }

    /// Every triangle (by its place in the tree) whose box overlaps `bx` on the two axes other
    /// than `axis`: the triangles a line along `axis` through `bx` can cross.
    fn column(&self, bx: &Aabb3, axis: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut stack: Vec<u32> = if self.tris.is_empty() { Vec::new() } else { vec![0] };
        while let Some(ni) = stack.pop() {
            let Some(node) = self.nodes.get(ni as usize) else { continue };
            if !boxes_meet(&node.bbox, bx, axis) {
                continue;
            }
            if node.leaf {
                out.extend((node.a..node.b).map(|k| k as usize).filter(|&k| self.tris.get(k).is_some_and(|t| boxes_meet(&box_of(t), bx, axis))));
            } else {
                stack.push(node.a);
                stack.push(node.b);
            }
        }
        out
    }
}

fn longest_edge(t: &[Vec3; 3]) -> f64 {
    t[0].dist(t[1]).max(t[1].dist(t[2])).max(t[2].dist(t[0]))
}

/// Pieces one triangle is sampled in, at most.
const MAX_PIECES: usize = 1 << 22;

/// About how many pieces [`for_each_sample`] cuts the triangle into.
fn pieces_estimate(t: &[Vec3; 3], h: f64) -> f64 {
    let l = longest_edge(t);
    if !(l > h) {
        return 1.0;
    }
    let area = (t[1] - t[0]).cross(t[2] - t[0]).len() * 0.5;
    (4.0 * area / (h * h)).max(2.0 * l / h).clamp(1.0, MAX_PIECES as f64)
}

/// Calls `f` with every sample point of the triangle and the area it stands for. The triangle is
/// halved across its longest side until every piece's sides are at most `h` long; the centre of
/// each piece stands for the piece, its corners (which find what lies along the edges) for none.
fn for_each_sample(t: &[Vec3; 3], h: f64, stack: &mut Vec<[Vec3; 3]>, mut f: impl FnMut(Vec3, f64)) {
    stack.clear();
    stack.push(*t);
    let mut pieces = 0usize;
    while let Some([a, b, c]) = stack.pop() {
        let (lab, lbc, lca) = (a.dist(b), b.dist(c), c.dist(a));
        let l = lab.max(lbc).max(lca);
        pieces += 1;
        if !(l > h) || pieces + stack.len() >= MAX_PIECES {
            for p in [a, b, c] {
                f(p, 0.0);
            }
            f((a + b + c) / 3.0, (b - a).cross(c - a).len() * 0.5);
            continue;
        }
        // Midpoints are computed from the same two corners on both sides of an inner edge, so
        // the pieces either side share them exactly.
        if lab >= lbc && lab >= lca {
            let m = (a + b) * 0.5;
            stack.push([a, m, c]);
            stack.push([m, b, c]);
        } else if lbc >= lca {
            let m = (b + c) * 0.5;
            stack.push([a, b, m]);
            stack.push([a, m, c]);
        } else {
            let m = (c + a) * 0.5;
            stack.push([a, b, m]);
            stack.push([m, b, c]);
        }
    }
}

/// The spacing at which `mesh` needs about `max` sample points (at least `want`; a mesh whose
/// triangles alone need more gets one piece per triangle).
fn spacing_for(mesh: &Mesh, want: f64, max: usize) -> f64 {
    let tris: Vec<[Vec3; 3]> = mesh.triangles.iter().filter_map(|t| mesh.tri(t)).filter(|t| t.iter().all(|p| p.is_finite())).collect();
    // Every piece is sampled at its centre and its corners, each corner once: a triangulated
    // surface has about half as many corners as triangles, so 1.5 samples a piece.
    let fewest = 1.5 * tris.len() as f64;
    let max = max.max(1) as f64;
    let coarser = |h: f64, total: f64| h * (total / max).sqrt() * 1.02;
    let mut h = if want.is_finite() && want > 0.0 { want } else { 1.0 };
    // A spacing known to need too many, to close in on from above.
    let mut over = None;
    // First from the estimate, cheaply; then from the exact count, which the estimate can be
    // a few times under (long thin triangles, the halving's steps).
    for _ in 0..40 {
        let total = tris.iter().map(|t| 1.5 * pieces_estimate(t, h)).sum::<f64>();
        if total <= max || total <= fewest * 1.001 {
            break;
        }
        over = Some(h);
        h = coarser(h, total);
    }
    let mut stack = Vec::new();
    let mut count = |h: f64| {
        let mut pieces = 0usize;
        for t in &tris {
            for_each_sample(t, h, &mut stack, |_, w| pieces += usize::from(w > 0.0));
        }
        1.5 * pieces as f64
    };
    let mut total = count(h);
    for _ in 0..12 {
        if total <= max || total <= fewest * 1.001 {
            break;
        }
        over = Some(h);
        h = coarser(h, total);
        total = count(h);
    }
    // The count falls in steps (each halving doubles a triangle's pieces), so the last step may
    // have gone a step too far: the finest spacing within the budget lies between.
    if let Some(mut lo) = over.filter(|_| total <= max) {
        for _ in 0..10 {
            let mid = (lo * h).sqrt();
            if count(mid) <= max {
                h = mid;
            } else {
                lo = mid;
            }
        }
    }
    h
}

/// Triangles one job samples.
const CHUNK: usize = 1024;

/// A sample further than the tolerance from the other surface.
#[derive(Clone, Copy)]
struct Off {
    p: Vec3,
    d: f64,
    w: f64,
    face: u32,
}

#[derive(Default)]
struct Part {
    max: f64,
    at: Vec3,
    sum: f64,
    weight: f64,
    beyond: f64,
    points: usize,
    off: Vec<Off>,
    face_area: HashMap<u32, f64>,
}

/// Runs `job` for every index below `n` through `each` and returns the results in order.
fn gather<T: Send + Sync>(n: usize, each: Each, job: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let slots: Vec<OnceLock<T>> = (0..n).map(|_| OnceLock::new()).collect();
    each(n, &|i| {
        if let Some(slot) = slots.get(i) {
            let _ = slot.set(job(i));
        }
    });
    slots.into_iter().enumerate().map(|(i, s)| s.into_inner().unwrap_or_else(|| job(i))).collect()
}

/// How far the samples of `from` lie from the surface in `to` (distances capped at `cap`).
fn deviate(from: &Mesh, to: &TriangleTree, h: f64, tol: f64, cap: f64, each: Each) -> Part {
    let n = from.triangles.len();
    let parts = gather(n.div_ceil(CHUNK), each, |k| {
        let mut part = Part::default();
        let mut seen: HashSet<[u64; 3]> = HashSet::new();
        let mut stack = Vec::new();
        for ti in k * CHUNK..((k + 1) * CHUNK).min(n) {
            let Some(t) = from.triangles.get(ti).and_then(|t| from.tri(t)) else { continue };
            if !t.iter().all(|p| p.is_finite()) {
                continue;
            }
            let face = from.tri_face.get(ti).copied().unwrap_or(0);
            for_each_sample(&t, h, &mut stack, |p, w| {
                // A corner is shared by the pieces around it: measured once.
                if w == 0.0 && !seen.insert([p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]) {
                    return;
                }
                let d = to.nearest(p).map_or(cap, |(d, _)| d.min(cap));
                part.points += 1;
                if w > 0.0 {
                    *part.face_area.entry(face).or_default() += w;
                }
                part.sum += w * d;
                part.weight += w;
                if d > part.max {
                    part.max = d;
                    part.at = p;
                }
                if d > tol {
                    part.beyond += w;
                    part.off.push(Off { p, d, w, face });
                }
            });
        }
        part
    });
    let mut all = Part::default();
    for p in parts {
        if p.max > all.max {
            all.max = p.max;
            all.at = p.at;
        }
        all.sum += p.sum;
        all.weight += p.weight;
        all.beyond += p.beyond;
        all.points += p.points;
        all.off.extend(p.off);
        for (f, w) in p.face_area {
            *all.face_area.entry(f).or_default() += w;
        }
    }
    all
}

fn deviation(p: &Part) -> Deviation {
    Deviation {
        max: p.max,
        at: p.at,
        mean: if p.weight > 0.0 { p.sum / p.weight } else { 0.0 },
        beyond_area: p.beyond,
        area: p.weight,
        points: p.points,
    }
}

/// Deviating samples kept for the regions, at most (beyond it, every k-th, standing for k).
const MAX_OFF: usize = 4_000_000;

/// A deviating sample and the side of the material it bounds.
#[derive(Clone, Copy)]
struct Pt {
    off: Off,
    from_a: bool,
    side: Side,
}

/// The deviating samples of `from` with their sides: outside the other mesh, they bound
/// material only `from` has; inside it, material only the other has.
fn sided(mut off: Vec<Off>, other: &Mesh, from_a: bool, each: Each) -> Vec<Pt> {
    if off.len() > MAX_OFF {
        let k = off.len().div_ceil(MAX_OFF);
        off = off.into_iter().step_by(k).map(|o| Off { w: o.w * k as f64, ..o }).collect();
    }
    if off.is_empty() {
        return Vec::new();
    }
    let inside = other.inside_index();
    let off = &off;
    let parts = gather(off.len().div_ceil(CHUNK), each, |k| {
        off.iter()
            .skip(k * CHUNK)
            .take(CHUNK)
            .map(|o| {
                let within = inside.contains(o.p);
                let side = if within == from_a { Side::OnlyInB } else { Side::OnlyInA };
                Pt { off: *o, from_a, side }
            })
            .collect::<Vec<_>>()
    });
    parts.into_iter().flatten().collect()
}

fn find(parent: &mut [usize], mut i: usize) -> usize {
    for _ in 0..parent.len() {
        let Some(&p) = parent.get(i) else { return i };
        if p == i {
            return i;
        }
        let gp = parent.get(p).copied().unwrap_or(p);
        if let Some(slot) = parent.get_mut(i) {
            *slot = gp;
        }
        i = gp;
    }
    i
}

fn union(parent: &mut [usize], a: usize, b: usize) -> bool {
    let (ra, rb) = (find(parent, a), find(parent, b));
    if ra == rb {
        return false;
    }
    if let Some(slot) = parent.get_mut(ra.max(rb)) {
        *slot = ra.min(rb);
    }
    true
}

/// Groups of samples (indices into `pts`), one side each: samples in the same or neighbouring
/// cells of a grid of `cell` join, and so do groups of one side whose boxes, grown by `cell`,
/// overlap (so that no two regions measure the same material).
fn clusters(pts: &[Pt], cell: f64) -> Vec<Vec<usize>> {
    let key = |p: Vec3| ((p.x / cell).floor() as i64, (p.y / cell).floor() as i64, (p.z / cell).floor() as i64);
    let mut cells: HashMap<(bool, i64, i64, i64), usize> = HashMap::new();
    let mut of_point = Vec::with_capacity(pts.len());
    for pt in pts {
        let (x, y, z) = key(pt.off.p);
        let next = cells.len();
        of_point.push(*cells.entry((pt.side == Side::OnlyInA, x, y, z)).or_insert(next));
    }
    let mut parent: Vec<usize> = (0..cells.len()).collect();
    for (&(s, x, y, z), &id) in &cells {
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(&o) = cells.get(&(s, x + dx, y + dy, z + dz)) {
                        union(&mut parent, id, o);
                    }
                }
            }
        }
    }
    let groups_of = |parent: &mut [usize]| -> Vec<usize> { of_point.iter().map(|&c| find(parent, c)).collect() };
    // Join groups of one side whose grown boxes overlap, until none do (sort and sweep along X).
    for _ in 0..16 {
        let groups = groups_of(&mut parent);
        let mut boxes: HashMap<usize, (Side, Aabb3)> = HashMap::new();
        for (pt, g) in pts.iter().zip(&groups) {
            boxes.entry(*g).or_insert((pt.side, Aabb3::EMPTY)).1.add(pt.off.p);
        }
        let mut list: Vec<(usize, Side, Aabb3)> = boxes.into_iter().map(|(g, (s, b))| (g, s, b.inflated(cell))).collect();
        list.sort_by(|a, b| a.2.min.x.total_cmp(&b.2.min.x).then(a.0.cmp(&b.0)));
        let mut joined = false;
        for (k, (ga, sa, ba)) in list.iter().enumerate() {
            for (gb, sb, bb) in list.iter().skip(k + 1) {
                if bb.min.x > ba.max.x {
                    break;
                }
                if sa == sb && boxes_meet(ba, bb, 3) {
                    joined |= union(&mut parent, *ga, *gb);
                }
            }
        }
        if !joined {
            break;
        }
    }
    let mut by: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, g) in groups_of(&mut parent).into_iter().enumerate() {
        by.entry(g).or_default().push(i);
    }
    let mut out: Vec<Vec<usize>> = by.into_values().collect();
    out.sort_by_key(|g| g.first().copied().unwrap_or(0));
    out
}

/// Line and triangle pairs tested for one region, at most; beyond it, its volume is not measured.
const MAX_LINE_WORK: usize = 200_000_000;

/// Where the lines lie in their cells, across the axis (off the middle, so that lines through
/// the edges and corners of a mesh are rare).
const LINE_OFFSET: [f64; 2] = [0.513_72, 0.527_18];

/// The volume of the material only `side`'s mesh has inside `bx`, from lines along the box's
/// shortest axis: (volume, centre, box, lines), or `None` when that would take too much work.
fn region_volume(trees: [&TriangleTree; 2], bx: &Aabb3, side: Side, max_lines: usize) -> Option<(f64, Vec3, Aabb3, Lines)> {
    let size = bx.size();
    let k = if size.x <= size.y && size.x <= size.z {
        0
    } else if size.y <= size.z {
        1
    } else {
        2
    };
    let (ku, kv) = ((k + 1) % 3, (k + 2) % 3);
    let (eu, ev) = (comp(size, ku), comp(size, kv));
    let max_lines = max_lines.max(1);
    let ds = (eu * ev / max_lines as f64).sqrt();
    let count = |e: f64| if ds > 0.0 && ds.is_finite() { (e / ds).ceil().clamp(1.0, max_lines as f64) as usize } else { 1 };
    let (nu, nv) = (count(eu), count(ev));
    let (du, dv) = (eu / nu as f64, ev / nv as f64);
    let (u0, v0, t0, t1) = (comp(bx.min, ku), comp(bx.min, kv), comp(bx.min, k), comp(bx.max, k));
    let line_u = |i: usize| u0 + (i as f64 + LINE_OFFSET[0]) * du;
    let line_v = |j: usize| v0 + (j as f64 + LINE_OFFSET[1]) * dv;
    // The lines whose positions fall in [lo, hi].
    let span = |lo: f64, hi: f64, origin: f64, step: f64, offset: f64, n: usize| -> Option<(usize, usize)> {
        if !(step > 0.0) {
            return Some((0, 0));
        }
        let a = ((lo - origin) / step - offset).ceil().max(0.0);
        let b = ((hi - origin) / step - offset).floor().min(n as f64 - 1.0);
        (a <= b).then_some((a as usize, b as usize))
    };
    // Where each line crosses each mesh: (line, coordinate along the axis, +1 in / -1 out, mesh).
    let mut hits: Vec<(u32, f64, i8, u8)> = Vec::new();
    let mut work = 0usize;
    for (m, tree) in trees.iter().enumerate() {
        for ti in tree.column(bx, k) {
            let Some(t) = tree.tris.get(ti) else { continue };
            let nk = comp((t[1] - t[0]).cross(t[2] - t[0]), k);
            if nk == 0.0 || !nk.is_finite() {
                continue;
            }
            // Lines run towards +axis: they enter where the outward normal points against them.
            let sign: i8 = if nk < 0.0 { 1 } else { -1 };
            let tb = box_of(t);
            let (Some((i0, i1)), Some((j0, j1))) = (
                span(comp(tb.min, ku), comp(tb.max, ku), u0, du, LINE_OFFSET[0], nu),
                span(comp(tb.min, kv), comp(tb.max, kv), v0, dv, LINE_OFFSET[1], nv),
            ) else {
                continue;
            };
            work = work.saturating_add((i1 - i0 + 1).saturating_mul(j1 - j0 + 1));
            if work > MAX_LINE_WORK {
                return None;
            }
            for i in i0..=i1 {
                let pu = line_u(i);
                for j in j0..=j1 {
                    let pv = line_v(j);
                    // Watertight edge tests (Woop, Benthin & Wald, JCGT 2013) in the plane across
                    // the axis: two triangles get exactly opposite values on the edge they share.
                    let at = |p: Vec3| (comp(p, ku) - pu, comp(p, kv) - pv);
                    let ((ax, ay), (bx2, by), (cx, cy)) = (at(t[0]), at(t[1]), at(t[2]));
                    let u = cx * by - cy * bx2;
                    let v = ax * cy - ay * cx;
                    let w = bx2 * ay - by * ax;
                    if (u < 0.0 || v < 0.0 || w < 0.0) && (u > 0.0 || v > 0.0 || w > 0.0) {
                        continue;
                    }
                    let det = u + v + w;
                    if det == 0.0 {
                        continue;
                    }
                    let along = (u * comp(t[0], k) + v * comp(t[1], k) + w * comp(t[2], k)) / det;
                    if let (true, Ok(line)) = (along.is_finite(), u32::try_from(i * nv + j)) {
                        hits.push((line, along, sign, m as u8));
                    }
                }
            }
        }
    }
    hits.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let wanted = |w: [i32; 2]| match side {
        Side::OnlyInA => w[0] > 0 && w[1] <= 0,
        Side::OnlyInB => w[1] > 0 && w[0] <= 0,
    };
    let (mut length, mut moment, mut found, mut skipped) = (0.0, Vec3::ZERO, Aabb3::EMPTY, 0);
    let mut pieces: Vec<(f64, f64)> = Vec::new();
    let mut rest = hits.as_slice();
    while let Some(first) = rest.first() {
        let line = first.0;
        let (this, after) = rest.split_at(rest.iter().take_while(|h| h.0 == line).count());
        rest = after;
        let (pu, pv) = (line_u(line as usize / nv), line_v(line as usize % nv));
        let (mut w, mut last) = ([0i32; 2], [None::<(f64, i8)>; 2]);
        let mut prev = f64::NEG_INFINITY;
        pieces.clear();
        for &(_, along, sign, m) in this {
            let m = usize::from(m.min(1));
            // A line through an edge or corner shared by several triangles crosses there once.
            let eps = 1e-9 * (1.0 + along.abs());
            if last.get(m).copied().flatten().is_some_and(|(t, s)| s == sign && (along - t).abs() <= eps) {
                continue;
            }
            if wanted(w) {
                let (s0, s1) = (prev.max(t0), along.min(t1));
                // Where the two surfaces meet, rounding leaves pieces of no length between them.
                if s1 - s0 > 1e-9 * (1.0 + s1.abs()) {
                    pieces.push((s0, s1));
                }
            }
            if let Some(x) = w.get_mut(m) {
                *x += i32::from(sign);
            }
            if let Some(x) = last.get_mut(m) {
                *x = Some((along, sign));
            }
            prev = along;
        }
        // A line that leaves a mesh more or less often than it enters passed through a gap.
        if w != [0, 0] {
            skipped += 1;
            continue;
        }
        for &(s0, s1) in &pieces {
            let l = s1 - s0;
            length += l;
            moment += from_axes(k, pu, pv, (s0 + s1) * 0.5) * l;
            found.add(from_axes(k, pu - du * 0.5, pv - dv * 0.5, s0));
            found.add(from_axes(k, pu + du * 0.5, pv + dv * 0.5, s1));
        }
    }
    let centre = if length > 0.0 { moment / length } else { bx.center() };
    Some((length * du * dv, centre, found, Lines { axis: k, spacing: [du, dv], count: nu * nv, skipped }))
}

/// Compare the closed meshes `a` and `b` (in one frame): see the module documentation.
pub fn compare(a: &Mesh, b: &Mesh, o: &CompareOptions, each: Each) -> Comparison {
    let tol = if o.tolerance.is_finite() { o.tolerance.max(0.0) } else { 0.01 };
    let max_points = o.max_points.max(1);
    let h = spacing_for(a, o.spacing, max_points).max(spacing_for(b, o.spacing, max_points));
    let cap = a.bounds().union(&b.bounds()).diagonal().max(1.0);
    let Ok([ta, tb]) = <[TriangleTree; 2]>::try_from(gather(2, each, |i| TriangleTree::new(if i == 0 { a } else { b }))) else {
        return Comparison::default();
    };
    let mut pa = deviate(a, &tb, h, tol, cap, each);
    let mut pb = deviate(b, &ta, h, tol, cap, each);
    let (a_to_b, b_to_a) = (deviation(&pa), deviation(&pb));
    let face_area = [std::mem::take(&mut pa.face_area), std::mem::take(&mut pb.face_area)];
    let mut pts = sided(pa.off, b, true, each);
    pts.extend(sided(pb.off, a, false, each));
    // Samples on one surface are at most `h` apart, and the two surfaces at a region's rim come
    // within the tolerance of each other: cells of that size keep a region together.
    let cell = h.max(3.0 * tol).max(1e-6);
    struct Found {
        side: Side,
        area: [f64; 2],
        bbox: Aabb3,
        max: f64,
        at: Vec3,
        faces: [Vec<(u32, f64)>; 2],
        weight: f64,
    }
    let mut found: Vec<Found> = clusters(&pts, cell)
        .iter()
        .filter_map(|g| {
            let first = pts.get(*g.first()?)?;
            let mut f = Found {
                side: first.side,
                area: [0.0; 2],
                bbox: Aabb3::EMPTY,
                max: 0.0,
                at: first.off.p,
                faces: [Vec::new(), Vec::new()],
                weight: 0.0,
            };
            let mut faces: [HashMap<u32, f64>; 2] = Default::default();
            for pt in g.iter().filter_map(|&i| pts.get(i)) {
                let m = usize::from(!pt.from_a);
                if let Some(x) = f.area.get_mut(m) {
                    *x += pt.off.w;
                }
                if let Some(x) = faces.get_mut(m) {
                    *x.entry(pt.off.face).or_default() += pt.off.w;
                }
                f.bbox.add(pt.off.p);
                f.weight += pt.off.w * pt.off.d;
                if pt.off.d > f.max {
                    f.max = pt.off.d;
                    f.at = pt.off.p;
                }
            }
            for (to, from) in f.faces.iter_mut().zip(faces) {
                *to = from.into_iter().collect();
                to.sort_unstable_by_key(|x| x.0);
            }
            Some(f)
        })
        .collect();
    // Which to measure: largest first by the volume the surfaces suggest (area × distance).
    found.sort_by(|x, y| y.weight.total_cmp(&x.weight).then(y.max.total_cmp(&x.max)));
    let unlisted = found.len().saturating_sub(o.max_regions);
    found.truncate(o.max_regions);
    let measured = gather(found.len(), each, |i| {
        let f = found.get(i)?;
        region_volume([&ta, &tb], &f.bbox.inflated(cell), f.side, o.max_lines)
    });
    let mut regions: Vec<Region> = found
        .into_iter()
        .zip(measured)
        .map(|(f, m)| {
            let (volume, center, bbox, lines) = match m {
                Some((v, c, b, l)) if v > 0.0 => (v, c, b, l),
                Some((v, _, _, l)) => (v, f.bbox.center(), f.bbox, l),
                None => (f64::NAN, f.bbox.center(), f.bbox, Lines::default()),
            };
            let [faces_a, faces_b] = f.faces;
            Region {
                side: f.side,
                volume,
                center,
                bbox,
                area_a: f.area[0],
                area_b: f.area[1],
                max_deviation: f.max,
                at: f.at,
                faces_a,
                faces_b,
                lines,
            }
        })
        .collect();
    let key = |r: &Region| if r.volume.is_finite() { r.volume } else { -1.0 };
    regions.sort_by(|x, y| key(y).total_cmp(&key(x)).then(y.max_deviation.total_cmp(&x.max_deviation)));
    Comparison { a_to_b, b_to_a, regions, unlisted, spacing: h, face_area }
}

#[cfg(test)]
#[path = "compare_tests.rs"]
mod tests;
