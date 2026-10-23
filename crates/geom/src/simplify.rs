//! Triangle mesh simplification, for drawing a large mesh coarser where it is small on screen
//! (display levels of detail). Display only: nothing measured, exported or picked reads a
//! simplified mesh.
//!
//! Edge collapse ordered by quadric error (Garland and Heckbert, "Surface Simplification Using
//! Quadric Error Metrics", SIGGRAPH 1997): each vertex carries the sum of the squared-distance
//! quadrics of the planes of its faces; the edge whose two ends merge at the point of least
//! summed squared distance goes first. A collapse that would fold a face over (its normal turning
//! by more than about 80°) or join two sheets of the surface (the ends sharing more neighbours
//! than the faces between them) is skipped. Open boundaries carry extra planes across them, so
//! they keep their place.
//!
//! [`levels`] simplifies once, keeping a copy of the mesh each time it reaches the next target
//! triangle count, with its error measured: how far its vertices and the centres of its triangles
//! lie from the original surface, at most.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::Vec3;

/// Most triangles [`levels`] simplifies (more: no levels).
pub const MAX_TRIANGLES: usize = 50_000_000;

/// How much more an open boundary's planes count than a face's.
const BOUNDARY_WEIGHT: f64 = 10.0;
/// A collapse may turn a face's normal by at most this (cosine): about 80°.
const MIN_TURN_COS: f64 = 0.2;

/// A simplified copy of a mesh.
#[derive(Clone, Debug, Default)]
pub struct Level {
    pub positions: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
    /// How far (mm) its vertices and triangle centres lie from the original surface, at most.
    pub error: f64,
}

/// The mesh (`positions`, and `triangles` indexing them; corners at the same position are one
/// vertex) simplified to each of `targets` triangles in turn (largest first), one [`Level`] per
/// target reached. Simplification stops early when no edge can collapse any more; the levels made
/// by then are returned, but none that is not at least a fifth smaller than the one before it.
/// Triangles with an index out of range or a point not finite are left out; an empty or oversized
/// mesh ([`MAX_TRIANGLES`]) has no levels.
pub fn levels(positions: &[Vec3], triangles: &[[u32; 3]], targets: &[usize]) -> Vec<Level> {
    if triangles.is_empty() || triangles.len() > MAX_TRIANGLES {
        return Vec::new();
    }
    let Some(mut m) = Collapser::new(positions, triangles) else { return Vec::new() };
    let original = Surface::new(&m.pos, &m.tris);
    let mut out: Vec<Level> = Vec::new();
    let mut previous = m.live_triangles;
    for &target in targets {
        if target >= previous {
            continue;
        }
        m.collapse_to(target);
        if m.live_triangles * 5 > previous * 4 {
            break;
        }
        previous = m.live_triangles;
        let (mut level, seeds) = m.level();
        // Never less than a coarser level's own error.
        level.error = original.deviation(&level, &seeds).max(out.last().map_or(0.0, |l| l.error));
        out.push(level);
    }
    out
}

/// The original surface, for measuring how far a level strays from it.
struct Surface {
    tris: Vec<[Vec3; 3]>,
    bvh: crate::Bvh,
}

impl Surface {
    fn new(pos: &[[f64; 3]], tris: &[[u32; 3]]) -> Surface {
        let tris: Vec<[Vec3; 3]> = tris.iter().map(|t| t.map(|i| pos.get(i as usize).map_or(Vec3::ZERO, |p| Vec3::new(p[0], p[1], p[2])))).collect();
        let boxes: Vec<crate::Aabb3> = tris
            .iter()
            .map(|t| {
                let mut b = crate::Aabb3::EMPTY;
                t.iter().for_each(|p| b.add(*p));
                b
            })
            .collect();
        Surface { bvh: crate::Bvh::new(&boxes), tris }
    }

    fn to_triangle(&self, p: Vec3, t: u32) -> f64 {
        self.tris.get(t as usize).map_or(f64::INFINITY, |[a, b, c]| crate::compare::closest_on_triangle(p, *a, *b, *c).dist(p))
    }

    /// How far `p` lies from the surface, when that is more than `beyond`; otherwise
    /// `beyond` or less (the search stops at the first triangle within `beyond`). `near`: an
    /// original triangle `p` is likely near (where it came from), so the search starts close.
    fn distance_beyond(&self, p: Vec3, beyond: f64, near: u32) -> f64 {
        let best = std::cell::Cell::new(self.to_triangle(p, near));
        let gap = |b: &crate::Aabb3| {
            let d = |v: f64, lo: f64, hi: f64| (lo - v).max(v - hi).max(0.0);
            Vec3::new(d(p.x, b.min.x, b.max.x), d(p.y, b.min.y, b.max.y), d(p.z, b.min.z, b.max.z)).len()
        };
        self.bvh.visit(|b| best.get() > beyond && gap(b) < best.get(), |t| best.set(best.get().min(self.to_triangle(p, t))));
        best.get()
    }

    /// How far `level`'s vertices and triangle centres lie from the surface, at most (`seeds`:
    /// an original triangle each vertex came from).
    fn deviation(&self, level: &Level, seeds: &[u32]) -> f64 {
        let at = |i: u32| level.positions.get(i as usize).copied().unwrap_or(Vec3::ZERO);
        let seed = |i: u32| seeds.get(i as usize).copied().unwrap_or(0);
        let vertices = level.positions.iter().copied().zip(seeds.iter().copied());
        let centres = level.triangles.iter().map(|t| {
            let [a, b, c] = t.map(at);
            ((a + b + c) * (1.0 / 3.0), seed(t[0]))
        });
        let worst = vertices.chain(centres).fold(0.0, |worst: f64, (p, near)| worst.max(self.distance_beyond(p, worst, near)));
        if worst.is_finite() { worst } else { 0.0 }
    }
}

/// A symmetric 4 × 4 quadric: a a, a b, a c, a d, b b, b c, b d, c c, c d, d d.
#[derive(Clone, Copy, Default)]
struct Quadric([f64; 10]);

impl Quadric {
    /// The squared distance to the plane n · p + d = 0 (`n` unit), times `w`.
    fn plane(n: [f64; 3], d: f64, w: f64) -> Quadric {
        let [a, b, c] = n;
        Quadric([a * a * w, a * b * w, a * c * w, a * d * w, b * b * w, b * c * w, b * d * w, c * c * w, c * d * w, d * d * w])
    }

    fn add(&mut self, o: &Quadric) {
        for (x, y) in self.0.iter_mut().zip(o.0) {
            *x += y;
        }
    }

    fn sum(&self, o: &Quadric) -> Quadric {
        let mut q = *self;
        q.add(o);
        q
    }

    /// The summed squared distance at `p`.
    fn at(&self, p: [f64; 3]) -> f64 {
        let [aa, ab, ac, ad, bb, bc, bd, cc, cd, dd] = self.0;
        let [x, y, z] = p;
        (aa * x * x
            + 2.0 * ab * x * y
            + 2.0 * ac * x * z
            + 2.0 * ad * x
            + bb * y * y
            + 2.0 * bc * y * z
            + 2.0 * bd * y
            + cc * z * z
            + 2.0 * cd * z
            + dd)
            .max(0.0)
    }

    /// The point of least summed squared distance, when it is well defined.
    fn best(&self) -> Option<[f64; 3]> {
        let [aa, ab, ac, ad, bb, bc, bd, cc, cd, _] = self.0;
        let det = aa * (bb * cc - bc * bc) - ab * (ab * cc - bc * ac) + ac * (ab * bc - bb * ac);
        let scale = (aa.abs() + bb.abs() + cc.abs()).powi(3);
        if !(det.abs() > 1e-12 * scale) {
            return None;
        }
        let (r0, r1, r2) = (-ad, -bd, -cd);
        let x = (r0 * (bb * cc - bc * bc) - ab * (r1 * cc - bc * r2) + ac * (r1 * bc - bb * r2)) / det;
        let y = (aa * (r1 * cc - bc * r2) - r0 * (ab * cc - bc * ac) + ac * (ab * r2 - r1 * ac)) / det;
        let z = (aa * (bb * r2 - r1 * bc) - ab * (ab * r2 - r1 * ac) + r0 * (ab * bc - bb * ac)) / det;
        [x, y, z].iter().all(|v| v.is_finite()).then_some([x, y, z])
    }
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    let l = dot(a, a).sqrt();
    (l > 0.0 && l.is_finite()).then(|| [a[0] / l, a[1] / l, a[2] / l])
}

/// A candidate collapse: its cost, its ends, and their versions when it was costed (a later
/// change to either end makes it stale).
#[derive(Clone, Copy)]
struct Candidate {
    cost: f64,
    u: u32,
    v: u32,
    stamps: (u32, u32),
}

impl PartialEq for Candidate {
    fn eq(&self, o: &Self) -> bool {
        self.cmp(o) == Ordering::Equal
    }
}
impl Eq for Candidate {}
impl PartialOrd for Candidate {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Candidate {
    /// The cheapest first (the heap pops the greatest), ties by the ends, so the result never
    /// depends on the heap's order.
    fn cmp(&self, o: &Self) -> Ordering {
        o.cost.total_cmp(&self.cost).then_with(|| (o.u, o.v).cmp(&(self.u, self.v)))
    }
}

/// A quick hasher for welding by position (the keys are coordinates' bits).
#[derive(Default)]
struct BitsHasher(u64);

impl std::hash::Hasher for BitsHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.write_u64(u64::from(*b));
        }
    }
    fn write_u64(&mut self, x: u64) {
        self.0 = (self.0.rotate_left(5) ^ x).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
}

/// The lists a collapse works with, kept between collapses.
#[derive(Default)]
struct Scratch {
    at_u: Vec<u32>,
    at_v: Vec<u32>,
    shared: Vec<u32>,
    nu: Vec<u32>,
    nv: Vec<u32>,
}

impl Scratch {
    fn clear(&mut self) {
        for l in [&mut self.at_u, &mut self.at_v, &mut self.shared, &mut self.nu, &mut self.nv] {
            l.clear();
        }
    }
}

struct Collapser {
    pos: Vec<[f64; 3]>,
    quadric: Vec<Quadric>,
    alive: Vec<bool>,
    stamp: Vec<u32>,
    tris: Vec<[u32; 3]>,
    tri_alive: Vec<bool>,
    /// The triangles at each vertex (dead ones are dropped lazily).
    around: Vec<Vec<u32>>,
    /// An original triangle at each vertex (where it came from), to start a search near it.
    seed: Vec<u32>,
    /// Lists a collapse reuses.
    scratch: Scratch,
    heap: BinaryHeap<Candidate>,
    live_triangles: usize,
}

impl Collapser {
    fn new(positions: &[Vec3], triangles: &[[u32; 3]]) -> Option<Collapser> {
        // Weld corners at the same position.
        let mut index: std::collections::HashMap<[u64; 3], u32, std::hash::BuildHasherDefault<BitsHasher>> =
            std::collections::HashMap::with_capacity_and_hasher(positions.len() / 2, Default::default());
        let mut remap: Vec<Option<u32>> = Vec::with_capacity(positions.len());
        let mut pos: Vec<[f64; 3]> = Vec::new();
        for p in positions {
            if !p.is_finite() {
                remap.push(None);
                continue;
            }
            let next = u32::try_from(pos.len()).ok()?;
            let key = [(p.x + 0.0).to_bits(), (p.y + 0.0).to_bits(), (p.z + 0.0).to_bits()];
            remap.push(Some(*index.entry(key).or_insert_with(|| {
                pos.push([p.x, p.y, p.z]);
                next
            })));
        }
        drop(index);
        let mut tris: Vec<[u32; 3]> = Vec::with_capacity(triangles.len());
        for t in triangles {
            let r = |i: u32| remap.get(i as usize).copied().flatten();
            let (Some(a), Some(b), Some(c)) = (r(t[0]), r(t[1]), r(t[2])) else { continue };
            if a != b && b != c && c != a {
                tris.push([a, b, c]);
            }
        }
        if tris.is_empty() {
            return None;
        }
        let n = pos.len();
        let mut quadric = vec![Quadric::default(); n];
        let mut around: Vec<Vec<u32>> = vec![Vec::new(); n];
        let mut sides: Vec<(u32, u32, u32)> = Vec::with_capacity(tris.len() * 3);
        for (ti, t) in tris.iter().enumerate() {
            let ti = u32::try_from(ti).ok()?;
            let [a, b, c] = t.map(|i| pos.get(i as usize).copied().unwrap_or_default());
            if let Some(nrm) = unit(cross(sub(b, a), sub(c, a))) {
                let q = Quadric::plane(nrm, -dot(nrm, a), 1.0);
                for &i in t {
                    if let Some(x) = quadric.get_mut(i as usize) {
                        x.add(&q);
                    }
                }
            }
            for (k, &i) in t.iter().enumerate() {
                if let Some(list) = around.get_mut(i as usize) {
                    list.push(ti);
                }
                let j = t[(k + 1) % 3];
                sides.push((i.min(j), i.max(j), ti));
            }
        }
        // Open boundaries (sides of one triangle only): a plane across each, weighted, so the
        // boundary keeps its place.
        sides.sort_unstable();
        let mut k = 0;
        while k < sides.len() {
            let (a, b, t) = sides[k];
            let mut e = k + 1;
            while e < sides.len() && sides[e].0 == a && sides[e].1 == b {
                e += 1;
            }
            if e - k == 1
                && let (Some(pa), Some(pb), Some(tri)) = (pos.get(a as usize), pos.get(b as usize), tris.get(t as usize))
            {
                let [p0, p1, p2] = tri.map(|i| pos.get(i as usize).copied().unwrap_or_default());
                let face = cross(sub(p1, p0), sub(p2, p0));
                if let Some(across) = unit(cross(sub(*pb, *pa), face)) {
                    let q = Quadric::plane(across, -dot(across, *pa), BOUNDARY_WEIGHT);
                    for i in [a, b] {
                        if let Some(x) = quadric.get_mut(i as usize) {
                            x.add(&q);
                        }
                    }
                }
            }
            k = e;
        }
        let live_triangles = tris.len();
        let seed: Vec<u32> = around.iter().map(|list| list.first().copied().unwrap_or(0)).collect();
        let mut m = Collapser {
            alive: vec![true; n],
            stamp: vec![0; n],
            tri_alive: vec![true; tris.len()],
            heap: BinaryHeap::with_capacity(sides.len() / 2 + 1),
            pos,
            quadric,
            tris,
            around,
            seed,
            scratch: Scratch::default(),
            live_triangles,
        };
        let mut last = None;
        for &(a, b, _) in &sides {
            if last != Some((a, b)) {
                m.offer(a, b);
                last = Some((a, b));
            }
        }
        Some(m)
    }

    /// Where `u` and `v` would merge, and its cost.
    fn target(&self, u: u32, v: u32) -> Option<([f64; 3], f64)> {
        let (qu, qv) = (self.quadric.get(u as usize)?, self.quadric.get(v as usize)?);
        let q = qu.sum(qv);
        let (pu, pv) = (*self.pos.get(u as usize)?, *self.pos.get(v as usize)?);
        let mid = [(pu[0] + pv[0]) * 0.5, (pu[1] + pv[1]) * 0.5, (pu[2] + pv[2]) * 0.5];
        // The best point when it lies near the edge; else the best of its ends and middle.
        let reach = dot(sub(pu, pv), sub(pu, pv)) * 4.0;
        let best = q.best().filter(|p| dot(sub(*p, mid), sub(*p, mid)) <= reach);
        best.into_iter().chain([pu, pv, mid]).map(|p| (p, q.at(p))).min_by(|a, b| a.1.total_cmp(&b.1))
    }

    fn offer(&mut self, u: u32, v: u32) {
        let Some((_, cost)) = self.target(u, v) else { return };
        let stamps = (self.stamp.get(u as usize).copied().unwrap_or(0), self.stamp.get(v as usize).copied().unwrap_or(0));
        self.heap.push(Candidate { cost, u, v, stamps });
    }

    /// The live vertices sharing a live triangle with `v`.
    fn neighbours(&self, v: u32, out: &mut Vec<u32>) {
        out.clear();
        out.extend(
            self.around
                .get(v as usize)
                .into_iter()
                .flatten()
                .filter(|t| self.tri_alive.get(**t as usize).copied().unwrap_or(false))
                .filter_map(|t| self.tris.get(*t as usize))
                .flat_map(|t| t.iter().copied())
                .filter(|i| *i != v),
        );
        out.sort_unstable();
        out.dedup();
    }

    fn collapse_to(&mut self, target: usize) {
        while self.live_triangles > target {
            let Some(c) = self.heap.pop() else { return };
            let current = |i: u32| self.alive.get(i as usize).copied().unwrap_or(false);
            let stamp = |i: u32| self.stamp.get(i as usize).copied().unwrap_or(u32::MAX);
            if !current(c.u) || !current(c.v) || (stamp(c.u), stamp(c.v)) != c.stamps {
                continue;
            }
            self.collapse(c.u, c.v);
        }
    }

    /// Merge `v` into `u` at their best point, unless that would fold a face over or join two
    /// sheets of the surface.
    fn collapse(&mut self, u: u32, v: u32) {
        let Some((p, _)) = self.target(u, v) else { return };
        let live = |t: &u32| self.tri_alive.get(*t as usize).copied().unwrap_or(false);
        let mut s = std::mem::take(&mut self.scratch);
        s.clear();
        s.at_v.extend(self.around.get(v as usize).into_iter().flatten().copied().filter(live));
        s.at_u.extend(self.around.get(u as usize).into_iter().flatten().copied().filter(live));
        s.shared.extend(s.at_v.iter().copied().filter(|t| self.tris.get(*t as usize).is_some_and(|tri| tri.contains(&u))));
        let done = self.collapse_with(u, v, p, &mut s);
        self.scratch = s;
        if done {
            self.reoffer(u);
        }
    }

    /// [`Collapser::collapse`] with its lists in `s`: whether it collapsed.
    fn collapse_with(&mut self, u: u32, v: u32, p: [f64; 3], s: &mut Scratch) -> bool {
        if s.shared.is_empty() {
            return false;
        }
        // Two sheets meeting only at these ends would be joined.
        self.neighbours(u, &mut s.nu);
        self.neighbours(v, &mut s.nv);
        let common = s.nu.iter().filter(|x| s.nv.binary_search(x).is_ok()).count();
        if common > s.shared.len() {
            return false;
        }
        // No face folds over.
        for &t in s.at_u.iter().chain(&s.at_v).filter(|t| !s.shared.contains(t)) {
            let Some(tri) = self.tris.get(t as usize) else { continue };
            let corners = tri.map(|i| self.pos.get(i as usize).copied().unwrap_or_default());
            let moved = tri.map(|i| if i == u || i == v { p } else { self.pos.get(i as usize).copied().unwrap_or_default() });
            let before = cross(sub(corners[1], corners[0]), sub(corners[2], corners[0]));
            let after = cross(sub(moved[1], moved[0]), sub(moved[2], moved[0]));
            let (lb, la) = (dot(before, before).sqrt(), dot(after, after).sqrt());
            if !(la > 0.0) || (lb > 0.0 && dot(before, after) < MIN_TURN_COS * lb * la) {
                return false;
            }
        }
        for t in &s.shared {
            if let Some(a) = self.tri_alive.get_mut(*t as usize) {
                *a = false;
            }
            self.live_triangles = self.live_triangles.saturating_sub(1);
        }
        let mut list = self.around.get_mut(u as usize).map(std::mem::take).unwrap_or_default();
        let tri_alive = &self.tri_alive;
        list.retain(|t| tri_alive.get(*t as usize).copied().unwrap_or(false));
        for &t in s.at_v.iter().filter(|t| !s.shared.contains(t)) {
            if let Some(tri) = self.tris.get_mut(t as usize) {
                for i in tri.iter_mut() {
                    if *i == v {
                        *i = u;
                    }
                }
                list.push(t);
            }
        }
        if let Some(slot) = self.around.get_mut(u as usize) {
            *slot = list;
        }
        if let Some(slot) = self.around.get_mut(v as usize) {
            *slot = Vec::new();
        }
        let qv = self.quadric.get(v as usize).copied().unwrap_or_default();
        if let Some(q) = self.quadric.get_mut(u as usize) {
            q.add(&qv);
        }
        if let Some(x) = self.pos.get_mut(u as usize) {
            *x = p;
        }
        if let Some(a) = self.alive.get_mut(v as usize) {
            *a = false;
        }
        for i in [u, v] {
            if let Some(stamp) = self.stamp.get_mut(i as usize) {
                *stamp = stamp.wrapping_add(1);
            }
        }
        true
    }

    /// Cost again every edge at `u` (it moved).
    fn reoffer(&mut self, u: u32) {
        let mut around = std::mem::take(&mut self.scratch.nu);
        self.neighbours(u, &mut around);
        for &w in &around {
            self.offer(u.min(w), u.max(w));
        }
        self.scratch.nu = around;
    }

    /// The mesh as it is now, and an original triangle each of its vertices came from.
    fn level(&self) -> (Level, Vec<u32>) {
        let mut index: Vec<Option<u32>> = vec![None; self.pos.len()];
        let mut out = Level::default();
        let mut seeds = Vec::new();
        out.triangles.reserve(self.live_triangles);
        for (t, alive) in self.tris.iter().zip(&self.tri_alive) {
            if !alive {
                continue;
            }
            let mut tri = [0u32; 3];
            for (k, &i) in t.iter().enumerate() {
                let Some(slot) = index.get_mut(i as usize) else { continue };
                tri[k] = *slot.get_or_insert_with(|| {
                    let p = self.pos.get(i as usize).copied().unwrap_or_default();
                    out.positions.push(Vec3::new(p[0], p[1], p[2]));
                    seeds.push(self.seed.get(i as usize).copied().unwrap_or(0));
                    u32::try_from(out.positions.len() - 1).unwrap_or(u32::MAX)
                });
            }
            out.triangles.push(tri);
        }
        (out, seeds)
    }
}

#[cfg(test)]
#[path = "simplify_tests.rs"]
mod tests;
