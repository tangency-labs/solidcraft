//! The mesh path's shaping of a face by its triangles in 3D ([`super::Purpose::Mesh`]): Delaunay
//! refinement on the surface (Chew, "Guaranteed-quality mesh generation for curved surfaces",
//! 1993; Ruppert 1995), on the face's triangulation in (u, v). Each triangle is judged by its
//! own 3D angles, side lengths and sag, not in (u, v) scaled by the surface's mean speeds, which
//! breaks down where the speeds vary (a sphere towards its poles, a fillet, a torus) or collapse.
//!
//! - Sides are flipped to the surface's Delaunay rule: a side whose two opposite corners' 3D
//!   angles add up past half a turn is flipped, where both new triangles keep their winding in
//!   (u, v) (Lawson's flips, by 3D angles).
//! - A triangle sharper than the angle, longer than [`LONG`] times the side asked, or off the
//!   surface by more than the tolerance gets a point at its circumcentre: the centre of its 3D
//!   circle, put on the surface at its closest point (from the parameters the triangle's own
//!   map from its plane gives it), so the point is chosen in 3D, and (u, v) only locates it.
//!   A point nearer a node than [`CROWDED`] of the triangle's longest side is not added.
//! - A point that would encroach on a boundary side (lie within the ball the side is a diameter
//!   of, or past it) is not added: the side is split instead, which the face cannot do alone,
//!   since the side is an edge's segment its neighbour shares. The sides are returned, to be put
//!   on their edges ([`super::plan_at`]), or, where the face may not split them, kept whole.
//! - The surface's poles are no concern of it: the planner fans each pole's cap on its own
//!   ([`super::pole_caps`]). The fans are kept as they are: never flipped or refined, since in
//!   (u, v) a pole is a side and a fan's middles and maps there are not the surface's. A ring
//!   side encroaches by its lens, not its diametral ball ([`encroaches`]); a ring side a point
//!   would encroach on is split in the face, the fan triangle on it halved at the pole, while the
//!   halves keep the angle, and past that the point moves to the side's off-centre
//!   ([`off_centre`]), just outside its lens.
//!
//! Every loop is bounded: the points added (`budget`, reported as [`ShapeLimit::Points`]), the
//! flips per pass ([`FLIPS_PER_TRIANGLE`] per triangle at the start, [`FLIPS_PER_POINT`] after
//! each point) and the steps of each walk ([`WALK_STEPS`]).

use std::collections::VecDeque;

use crate::hash::BitsMap;
use crate::vec::VecExt as _;
use crate::ShapeLimit;

use solidcore::Vec3;

use super::size::{SizeAt, local};

/// A side is split once it is this many times the side asked (as [`super::cdt`]'s refinement).
const LONG: f64 = 1.5;
/// Flips per triangle the first pass of Lawson flips may make.
const FLIPS_PER_TRIANGLE: usize = 8;
/// Times a ring side may be halved in the face, its halves in turn: the fan's angle at the pole
/// over four at most.
const MAX_RING_SPLITS: u8 = 2;
/// Flips the pass after one new point may make.
const FLIPS_PER_POINT: usize = 256;
/// Steps of the walk to a new point; past them it is lost and not added.
const WALK_STEPS: usize = 4096;

/// What the shaping asks of a face.
pub(crate) struct Rules<'a> {
    /// The smallest angle (radians).
    pub min_angle: f64,
    /// The chord tolerance (mm).
    pub tol: f64,
    /// The face's side (mm).
    pub h: f64,
    /// The caller's size function.
    pub size_at: Option<SizeAt<'a>>,
    /// The most points it may add.
    pub budget: usize,
    /// `None`: a boundary side a point would encroach on is returned to be split. Otherwise the
    /// sides are kept whole, and this is why, where it leaves a triangle short.
    pub whole: Option<ShapeLimit>,
    /// The parameters of a 3D point on or near the surface (within the distance given), from
    /// the parameters given, unwrapped to them: the face's surface's closest point.
    pub project: &'a dyn Fn(Vec3, [f64; 2], f64) -> Option<[f64; 2]>,
}

/// A point is not added nearer a node than this fraction of the longest side of the triangle it
/// is for. The circumcentre of a triangle of the surface's Delaunay triangulation is as far as
/// its circumradius from every node, at least half that side: a point nearer is not its
/// circumcentre (the surface's Delaunay rule is kept only where both new triangles keep their
/// winding in (u, v)), and adding it would make a sliver, as the next point would.
const CROWDED: f64 = 0.25;

/// How the shaping ended.
pub(crate) enum Shaped {
    /// The triangles are shaped as far as the limits let them.
    Done(Vec<ShapeLimit>),
    /// The boundary sides new points would encroach on: each as its two boundary nodes, in loop
    /// order, with the fraction (1/2) where it is to be split.
    Split(Vec<(usize, usize, Vec<f64>)>),
}

/// The point and its parameters on the surface, `None` where it does not evaluate.
pub(crate) type Point<'a> = &'a dyn Fn([f64; 2]) -> Option<Vec3>;

/// A face's triangulation as the shaping works on it.
struct Mesh<'a> {
    uv: &'a mut Vec<[f64; 2]>,
    pos: &'a mut Vec<Vec3>,
    tris: &'a mut Vec<[u32; 3]>,
    /// The triangles on each side, by its two nodes in index order.
    sides: BitsMap<(u32, u32), Vec<u32>>,
    /// The boundary sides, by their two nodes in index order, each as its nodes in loop order.
    fixed: BitsMap<(u32, u32), (u32, u32)>,
    /// Triangles shaping gave up on until they change.
    stuck: Vec<bool>,
    /// The sides of the pole caps' rings, by their two nodes in index order, each with how often
    /// it was halved from a side of the ring as made: kept like the boundary, but split in the
    /// face ([`split_ring`]) at most [`MAX_RING_SPLITS`] times.
    ring: std::collections::BTreeMap<(u32, u32), u8>,
    /// The poles' copies: the fans' corners at the poles.
    poles: &'a std::collections::BTreeSet<u32>,
    /// The triangles of the pole caps' fans: never flipped or refined.
    frozen: Vec<bool>,
}

fn key(a: u32, b: u32) -> (u32, u32) {
    (a.min(b), a.max(b))
}

/// Twice the signed area of the (u, v) triangle `a`, `b`, `c`.
fn orient(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// The angle at `o` between the directions to `a` and `b` (radians; 0 where one is empty).
fn angle(o: Vec3, a: Vec3, b: Vec3) -> f64 {
    let (x, y) = (a - o, b - o);
    x.cross(y).len().atan2(x.dot(y))
}

impl Mesh<'_> {
    /// Whether side `e` is kept: a boundary side or a ring side.
    fn kept(&self, e: (u32, u32)) -> bool {
        self.fixed.contains_key(&e) || self.ring.contains_key(&e)
    }

    fn is_frozen(&self, ti: u32) -> bool {
        self.frozen.get(ti as usize).copied().unwrap_or(false)
    }

    fn node(&self, i: u32) -> Option<(Vec3, [f64; 2])> {
        Some((*self.pos.get(i as usize)?, *self.uv.get(i as usize)?))
    }

    fn corners(&self, t: [u32; 3]) -> Option<[Vec3; 3]> {
        Some([self.node(t[0])?.0, self.node(t[1])?.0, self.node(t[2])?.0])
    }

    fn params(&self, t: [u32; 3]) -> Option<[[f64; 2]; 3]> {
        Some([self.node(t[0])?.1, self.node(t[1])?.1, self.node(t[2])?.1])
    }

    /// Whether two of the triangle's corners are one point (the copies of a pole): such a
    /// triangle has no area and is left out of the face.
    fn collapsed(&self, t: [u32; 3]) -> bool {
        self.corners(t).is_none_or(|[a, b, c]| a == b || b == c || c == a)
    }

    fn link(&mut self, ti: u32) {
        if let Some(&t) = self.tris.get(ti as usize) {
            for k in 0..3 {
                let [a, b, _] = super::rot3(t, k);
                self.sides.entry(key(a, b)).or_default().push(ti);
            }
        }
        if let Some(s) = self.stuck.get_mut(ti as usize) {
            *s = false;
        }
    }

    fn unlink(&mut self, ti: u32) {
        if let Some(&t) = self.tris.get(ti as usize) {
            for k in 0..3 {
                let [a, b, _] = super::rot3(t, k);
                if let Some(ts) = self.sides.get_mut(&key(a, b)) {
                    ts.retain(|x| *x != ti);
                }
            }
        }
    }

    /// Put triangle `t` in slot `ti` (a new slot past the end), linked.
    fn set(&mut self, ti: u32, t: [u32; 3]) {
        if (ti as usize) < self.tris.len() {
            self.unlink(ti);
            if let Some(slot) = self.tris.get_mut(ti as usize) {
                *slot = t;
            }
        } else {
            self.tris.push(t);
            self.stuck.push(false);
            self.frozen.push(false);
        }
        self.link(ti);
    }

    /// The other triangle on side `(a, b)` than `ti`.
    fn across(&self, a: u32, b: u32, ti: u32) -> Option<u32> {
        self.sides.get(&key(a, b))?.iter().copied().find(|&x| x != ti)
    }

    /// Flip the sides in `queue` that break the surface's Delaunay rule, and those that then do,
    /// at most `budget` times. Returns the triangles changed.
    fn flip(&mut self, mut queue: Vec<(u32, u32)>, budget: usize) -> Vec<u32> {
        let mut changed = Vec::new();
        let mut left = budget;
        while let Some(e) = queue.pop() {
            if left == 0 {
                break;
            }
            if self.kept(e) {
                continue;
            }
            let Some(&[t1, t2]) = self.sides.get(&e).map(Vec::as_slice) else { continue };
            if self.is_frozen(t1) || self.is_frozen(t2) {
                continue;
            }
            let (Some(&ta), Some(&tb)) = (self.tris.get(t1 as usize), self.tris.get(t2 as usize)) else { continue };
            // [a, b, c] on one side of a -> b, [b, a, d] on the other.
            let Some([a, b, c]) = (0..3).map(|k| super::rot3(ta, k)).find(|[a, b, _]| key(*a, *b) == e) else { continue };
            let Some(d) = tb.iter().copied().find(|&v| v != a && v != b) else { continue };
            if c == d || self.sides.get(&key(c, d)).is_some_and(|ts| !ts.is_empty()) || self.collapsed(ta) || self.collapsed(tb) {
                continue;
            }
            let (Some((pa, qa)), Some((pb, qb)), Some((pc, qc)), Some((pd, qd))) = (self.node(a), self.node(b), self.node(c), self.node(d)) else { continue };
            // Locally Delaunay on the surface: the corners facing the side see it at less than
            // half a turn together.
            if !(angle(pc, pa, pb) + angle(pd, pb, pa) > std::f64::consts::PI * (1.0 + 1e-12)) {
                continue;
            }
            let (n1, n2) = ([a, d, c], [d, b, c]);
            if !(orient(qa, qd, qc) > 0.0 && orient(qd, qb, qc) > 0.0) || pc == pd {
                continue;
            }
            left -= 1;
            self.set(t1, n1);
            self.set(t2, n2);
            changed.extend([t1, t2]);
            queue.extend([key(a, d), key(d, b), key(b, c), key(c, a)]);
        }
        changed
    }

    /// Where the walk from triangle `from` to (u, v) point `q` ends.
    fn locate(&self, from: u32, q: [f64; 2]) -> Located {
        let mut t = from;
        for _ in 0..WALK_STEPS {
            let (Some(&tri), Some(ps)) = (self.tris.get(t as usize), self.tris.get(t as usize).and_then(|t| self.params(*t))) else {
                return Located::Lost;
            };
            // Each side's signed distance of `q` to its left, in (u, v).
            let mut out: Option<(u32, u32)> = None;
            let mut on: Option<(u32, u32)> = None;
            let size = (0..3)
                .map(|k| {
                    let [p0, p1, _] = super::rot3(ps, k);
                    (p1[0] - p0[0]).hypot(p1[1] - p0[1])
                })
                .fold(0.0, f64::max);
            for k in 0..3 {
                let [a, b, _] = super::rot3(tri, k);
                let [pa, pb, _] = super::rot3(ps, k);
                let len = (pb[0] - pa[0]).hypot(pb[1] - pa[1]);
                if !(len > 0.0) {
                    continue;
                }
                let side = orient(pa, pb, q) / len;
                if side < -1e-12 * size {
                    out = Some((a, b));
                    break;
                }
                if side <= 1e-12 * size {
                    on = Some((a, b));
                }
            }
            match out {
                None => return on.map_or(Located::Inside(t), |(a, b)| Located::OnSide(t, a, b)),
                Some((a, b)) => {
                    if self.kept(key(a, b)) {
                        return Located::Beyond(a, b);
                    }
                    match self.across(a, b, t) {
                        Some(next) => t = next,
                        None => return Located::Beyond(a, b),
                    }
                }
            }
        }
        Located::Lost
    }

    /// Put point `p` (parameters `q`) in triangle `t`, or on its side `(a, b)`; the triangles
    /// made, and the sides facing the new node (to flip).
    fn insert(&mut self, at: Located, q: [f64; 2], p: Vec3) -> Option<(Vec<u32>, Vec<(u32, u32)>)> {
        let n = u32::try_from(self.uv.len()).ok()?;
        match at {
            Located::Inside(ti) => {
                let [a, b, c] = *self.tris.get(ti as usize)?;
                self.uv.push(q);
                self.pos.push(p);
                let (t2, t3) = (u32::try_from(self.tris.len()).ok()?, u32::try_from(self.tris.len() + 1).ok()?);
                self.set(ti, [a, b, n]);
                self.set(t2, [b, c, n]);
                self.set(t3, [c, a, n]);
                Some((vec![ti, t2, t3], vec![key(a, b), key(b, c), key(c, a)]))
            }
            Located::OnSide(ti, x, y) => {
                if self.kept(key(x, y)) {
                    return None;
                }
                let tj = self.across(x, y, ti)?;
                let c = self.tris.get(ti as usize)?.iter().copied().find(|&v| v != x && v != y)?;
                let d = self.tris.get(tj as usize)?.iter().copied().find(|&v| v != x && v != y)?;
                self.uv.push(q);
                self.pos.push(p);
                let (t3, t4) = (u32::try_from(self.tris.len()).ok()?, u32::try_from(self.tris.len() + 1).ok()?);
                // [x, y, c] on one side, [y, x, d] on the other: each in two at the new node.
                self.set(ti, [x, n, c]);
                self.set(t3, [n, y, c]);
                self.set(tj, [y, n, d]);
                self.set(t4, [n, x, d]);
                Some((vec![ti, t3, tj, t4], vec![key(y, c), key(c, x), key(x, d), key(d, y)]))
            }
            Located::Beyond(..) | Located::Lost => None,
        }
    }
}

/// Where a point lies in the face's triangulation.
#[derive(Clone, Copy)]
enum Located {
    Inside(u32),
    /// On the side `(a, b)` of the triangle.
    OnSide(u32, u32, u32),
    /// Past the boundary side `(a, b)`.
    Beyond(u32, u32),
    Lost,
}

/// Shape the face's triangulation `tris` of nodes `uv` (the first `fixed` of them its boundary,
/// in `loops`, at the 3D points `pos` has for them), adding nodes, by `rules` on the surface
/// `point`. `pos` holds every node's 3D point and grows with `uv`.
pub(crate) fn shape(
    uv: &mut Vec<[f64; 2]>,
    pos: &mut Vec<Vec3>,
    tris: &mut Vec<[u32; 3]>,
    loops: &[std::ops::Range<usize>],
    caps: (&[(u32, u32)], std::ops::Range<usize>, &std::collections::BTreeSet<u32>),
    point: Point<'_>,
    rules: &Rules,
) -> Shaped {
    let (ring_sides, fans, poles) = caps;
    let mut fixed: BitsMap<(u32, u32), (u32, u32)> = BitsMap::default();
    for r in loops {
        let n = r.len();
        for k in 0..n {
            let (Ok(a), Ok(b)) = (u32::try_from(r.start + k), u32::try_from(r.start + (k + 1) % n)) else { continue };
            if a != b {
                fixed.entry(key(a, b)).or_insert((a, b));
            }
        }
    }
    let stuck = vec![false; tris.len()];
    let frozen: Vec<bool> = (0..tris.len()).map(|t| fans.contains(&t)).collect();
    let ring: std::collections::BTreeMap<(u32, u32), u8> = ring_sides.iter().map(|&(a, b)| (key(a, b), 0)).collect();
    let mut m = Mesh { uv, pos, tris, sides: BitsMap::default(), fixed, stuck, ring, frozen, poles };
    for ti in 0..m.tris.len() {
        if let Ok(ti) = u32::try_from(ti) {
            m.link(ti);
        }
    }
    let per = FLIPS_PER_TRIANGLE.saturating_mul(m.tris.len().max(1));
    // The surface's Delaunay triangulation of the nodes the (u, v) one was made of.
    let mut all: Vec<(u32, u32)> = m.sides.iter().filter(|(_, ts)| ts.len() == 2).map(|(e, _)| *e).collect();
    all.sort_unstable();
    m.flip(all, per);
    let h_at = |p: Vec3| local(rules.h, rules.size_at, p);
    let floor = if rules.tol.is_finite() && rules.tol > 0.0 { (1e-3 * rules.tol).powi(2) } else { 0.0 };
    let mut limits: Vec<ShapeLimit> = Vec::new();
    // The sides points would encroach on, by their nodes in index order.
    let mut encroached: std::collections::BTreeMap<(u32, u32), (u32, u32)> = std::collections::BTreeMap::new();
    let mut added = 0usize;
    // The triangles (their nodes, sorted) given up on for a ring side they may not split: they
    // stay given up whatever changes round them, so no point is spent on them again.
    let mut dead: std::collections::BTreeSet<[u32; 3]> = std::collections::BTreeSet::new();
    let sorted = |t: [u32; 3]| {
        let mut t = t;
        t.sort_unstable();
        t
    };
    let mut queue: VecDeque<u32> = (0..m.tris.len()).filter_map(|t| u32::try_from(t).ok()).collect();
    while let Some(ti) = queue.pop_front() {
        let Some(&t) = m.tris.get(ti as usize) else { continue };
        if m.stuck.get(ti as usize).copied().unwrap_or(true) || m.is_frozen(ti) || m.collapsed(t) || dead.contains(&sorted(t)) {
            continue;
        }
        let (Some([a, b, c]), Some([qa, qb, qc])) = (m.corners(t), m.params(t)) else { continue };
        // Its angles, sides, and how far its middle and its inner sides' middles lie off the surface.
        let angles = [angle(a, b, c), angle(b, c, a), angle(c, a, b)];
        let (sharpest, at) = angles.iter().copied().enumerate().map(|(k, x)| (x, k)).fold((f64::INFINITY, 0), |best, x| if x.0 < best.0 { x } else { best });
        let centroid = (a + b + c) * (1.0 / 3.0);
        let middle = [(qa[0] + qb[0] + qc[0]) / 3.0, (qa[1] + qb[1] + qc[1]) / 3.0];
        let off = |q: [f64; 2], x: Vec3| point(q).map_or(0.0, |s| s.dist(x));
        let long = [(a, b), (b, c), (c, a)].iter().any(|(x, y)| x.dist(*y) > LONG * h_at((*x + *y) * 0.5));
        let sags = off(middle, centroid) > rules.tol
            || (0..3).any(|k| {
                let [i, j, _] = super::rot3(t, k);
                let ([pi, pj, _], [ui, uj, _]) = (super::rot3([a, b, c], k), super::rot3([qa, qb, qc], k));
                !m.kept(key(i, j)) && off([(ui[0] + uj[0]) * 0.5, (ui[1] + uj[1]) * 0.5], (pi + pj) * 0.5) > rules.tol
            });
        let sharp = sharpest < rules.min_angle;
        if !(sharp || long || sags) {
            continue;
        }
        // A corner of the boundary sharper than the angle, between two of its sides, stays.
        let [i, j, k] = super::rot3(t, at);
        if sharp && !long && !sags && m.kept(key(i, j)) && m.kept(key(k, i)) {
            give_up(&mut m, ti, Some(ShapeLimit::Corner), &mut limits);
            continue;
        }
        let area = 0.5 * (b - a).cross(c - a).len();
        if !(area > floor) {
            give_up(&mut m, ti, sharp.then_some(ShapeLimit::Floor), &mut limits);
            continue;
        }
        if added >= rules.budget {
            note(&mut limits, ShapeLimit::Points);
            break;
        }
        // The 3D circumcentre, and its barycentric coordinates in the triangle's plane.
        let (e1, e2) = (b - a, c - a);
        let w = e1.cross(e2);
        let w2 = w.dot(w);
        if !(w2 > 0.0) {
            give_up(&mut m, ti, sharp.then_some(ShapeLimit::Floor), &mut limits);
            continue;
        }
        let x = (e2.cross(w) * e1.dot(e1) + w.cross(e1) * e2.dot(e2)) * (0.5 / w2);
        let (d11, d12, d22) = (e1.dot(e1), e1.dot(e2), e2.dot(e2));
        let det = d11 * d22 - d12 * d12;
        let (l1, l2) = ((x.dot(e1) * d22 - x.dot(e2) * d12) / det, (x.dot(e2) * d11 - x.dot(e1) * d12) / det);
        let near = [qa[0] + l1 * (qb[0] - qa[0]) + l2 * (qc[0] - qa[0]), qa[1] + l1 * (qb[1] - qa[1]) + l2 * (qc[1] - qa[1])];
        // The circumcentre on the surface: its closest point, from the parameters the
        // triangle's map gives it.
        let centre = a + x;
        let q = if near[0].is_finite() && near[1].is_finite() { (rules.project)(centre, near, x.len() + rules.tol) } else { None };
        let Some(q) = q.filter(|q| q[0].is_finite() && q[1].is_finite()) else {
            give_up(&mut m, ti, None, &mut limits);
            continue;
        };
        let longest = a.dist(b).max(b.dist(c)).max(c.dist(a));
        let mut located = m.locate(ti, q);
        let mut q = q;
        let mut p = match located {
            Located::Inside(_) | Located::OnSide(..) => point(q),
            Located::Beyond(..) | Located::Lost => None,
        };
        let mut near = encroaches(&m, located, p, rules.min_angle);
        if !near.is_empty() && near.iter().all(|e| !m.fixed.contains_key(e)) {
            // Only ring sides: the first is split in the face, where its fan's halves keep the
            // angle; otherwise the point is moved to the side's off-centre, which lies on the
            // edge of its lens and so encroaches on nothing it is for.
            let split = match near.first() {
                Some(&e) if added < rules.budget => split_ring(&mut m, e, point, rules.min_angle, CROWDED * longest),
                Some(_) => Err(ShapeLimit::Points),
                None => Err(ShapeLimit::SidesOnNoEdge),
            };
            if let Ok((made, facing)) = split {
                added += 1;
                let flipped = m.flip(facing, FLIPS_PER_POINT);
                queue.extend(made);
                queue.extend(flipped);
                queue.push_back(ti);
                continue;
            }
            let moved = near.first().and_then(|&e| off_centre(&m, e, rules)).map(|(from, oq)| {
                let at = m.locate(from, oq);
                let op = match at {
                    Located::Inside(_) | Located::OnSide(..) => point(oq),
                    Located::Beyond(..) | Located::Lost => None,
                };
                (at, oq, op)
            });
            match moved {
                Some((at, oq, Some(op))) if encroaches(&m, at, Some(op), rules.min_angle).is_empty() => {
                    (located, q, p) = (at, oq, Some(op));
                    near.clear();
                }
                _ => {
                    dead.insert(sorted(t));
                    give_up(&mut m, ti, Some(ShapeLimit::SidesOnNoEdge), &mut limits);
                    continue;
                }
            }
        }
        if !near.is_empty() {
            let edge_sides: Vec<(u32, u32)> = near.into_iter().filter(|e| m.fixed.contains_key(e)).collect();
            match rules.whole {
                None => {
                    for e in edge_sides {
                        if let Some(&side) = m.fixed.get(&e) {
                            encroached.insert(e, side);
                        }
                    }
                    give_up(&mut m, ti, None, &mut limits);
                }
                Some(why) => give_up(&mut m, ti, Some(why), &mut limits),
            }
            continue;
        }
        let Some(p) = p else {
            give_up(&mut m, ti, None, &mut limits);
            continue;
        };
        // Not a sliver's worth from a node of the triangle it lands in or of its neighbours.
        let landed = match located {
            Located::Inside(tj) | Located::OnSide(tj, ..) => Some(tj),
            Located::Beyond(..) | Located::Lost => None,
        };
        let crowded = landed.and_then(|tj| m.tris.get(tj as usize).copied().map(|tri| (tj, tri))).is_some_and(|(tj, tri)| {
            let mut around: Vec<[u32; 3]> = vec![tri];
            for k in 0..3 {
                let [x, y, _] = super::rot3(tri, k);
                around.extend(m.across(x, y, tj).and_then(|o| m.tris.get(o as usize).copied()));
            }
            around.iter().flatten().filter_map(|&v| m.node(v)).any(|(o, _)| o.dist(p) < CROWDED * longest)
        });
        if crowded {
            dead.insert(sorted(t));
            give_up(&mut m, ti, Some(ShapeLimit::Crowded), &mut limits);
            continue;
        }
        let Some((made, facing)) = m.insert(located, q, p) else {
            give_up(&mut m, ti, None, &mut limits);
            continue;
        };
        added += 1;
        let flipped = m.flip(facing, FLIPS_PER_POINT);
        queue.extend(made);
        queue.extend(flipped);
    }
    if !encroached.is_empty() {
        return Shaped::Split(encroached.into_values().map(|(a, b)| (a as usize, b as usize, vec![0.5])).collect());
    }
    // The triangles still too sharp, by why they stayed.
    Shaped::Done(limits)
}

/// The kept sides point `p` (located `at`) encroaches on: the one it lies past, and those of
/// the triangle it lands in and of that triangle's neighbours (but a pole's fan, which owns its
/// sides) that it lies in the diametral ball of, for a boundary side; for a ring side, in its
/// lens (Shewchuk, "Delaunay refinement mesh generation", 1997, §3.4: the points that see it at
/// more than half a turn less twice `min_angle`), since a ring side is never split there: a point
/// outside the lens on its bisector makes the triangle on it keep `min_angle` at its ends.
fn encroaches(m: &Mesh<'_>, at: Located, p: Option<Vec3>, min_angle: f64) -> Vec<(u32, u32)> {
    let mut near: Vec<(u32, u32)> = Vec::new();
    match at {
        Located::Beyond(x, y) => near.push(key(x, y)),
        Located::Inside(tj) | Located::OnSide(tj, ..) => {
            let (Some(p), Some(&tri)) = (p, m.tris.get(tj as usize)) else { return near };
            let mut around: Vec<u32> = vec![tj];
            for k in 0..3 {
                let [x, y, _] = super::rot3(tri, k);
                around.extend(m.across(x, y, tj));
            }
            for tk in around.into_iter().filter(|&tk| !m.is_frozen(tk)) {
                let Some(&tk) = m.tris.get(tk as usize) else { continue };
                for k in 0..3 {
                    let [x, y, _] = super::rot3(tk, k);
                    let e = key(x, y);
                    if !m.kept(e) || near.contains(&e) {
                        continue;
                    }
                    let (Some((px, _)), Some((py, _))) = (m.node(x), m.node(y)) else { continue };
                    if px == py {
                        continue;
                    }
                    let inside = if m.fixed.contains_key(&e) {
                        p.dist((px + py) * 0.5) < 0.5 * px.dist(py)
                    } else {
                        angle(p, px, py) > std::f64::consts::PI - 2.0 * min_angle
                    };
                    if inside {
                        near.push(e);
                    }
                }
            }
        }
        Located::Lost => {}
    }
    near
}

/// The off-centre of ring side `e` (Üngör, "Off-centers: a new type of Steiner points for
/// computing size-optimal quality-guaranteed Delaunay triangulations", 2004): the point on the
/// side's bisector, toward the face, from which it is seen at half a turn less twice the angle
/// (a hair less): the triangle it makes on the side keeps the angle at both ends, and it lies just
/// outside the side's lens ([`encroaches`]). The triangle to walk from and its parameters, put on
/// the surface at its closest point; `None` where the side has no such triangle.
fn off_centre(m: &Mesh<'_>, e: (u32, u32), rules: &Rules) -> Option<(u32, [f64; 2])> {
    let ts = m.sides.get(&e)?;
    let other = ts.iter().copied().find(|&t| !m.is_frozen(t))?;
    let tri = *m.tris.get(other as usize)?;
    let c = tri.iter().copied().find(|&v| v != e.0 && v != e.1)?;
    let ((px, qx), (py, qy), (pc, qc)) = (m.node(e.0)?, m.node(e.1)?, m.node(c)?);
    let length = px.dist(py);
    let along = (py - px) * (1.0 / length);
    let mid = (px + py) * 0.5;
    let w = pc - mid;
    let across = w - along * w.dot(along);
    let reach = across.len();
    if !(length > 0.0 && reach > 0.0) {
        return None;
    }
    let height = 0.5 * length * (rules.min_angle * (1.0 + 1e-3)).tan();
    let target = mid + across * (height / reach);
    let qm = [0.5 * (qx[0] + qy[0]), 0.5 * (qx[1] + qy[1])];
    let f = height / reach;
    let from = [qm[0] + (qc[0] - qm[0]) * f, qm[1] + (qc[1] - qm[1]) * f];
    let q = (rules.project)(target, from, height + rules.tol)?;
    Some((other, q))
}

/// Split ring side `e` in the face at its middle in (u, v) (the ring is straight there), on the
/// surface: the triangle beyond it in two, and the fan triangle on it in two at the pole. Under
/// the same rules as every point the shaping adds: not nearer a node of the two triangles than
/// `crowd` ([`ShapeLimit::Crowded`]), and only while the halves of the fan's angle at its pole
/// (a pole's copy, checked) keep `min_angle` and the side was halved fewer than
/// [`MAX_RING_SPLITS`] times ([`ShapeLimit::SidesOnNoEdge`]). The triangles made and the sides
/// facing the new node; nothing changed on a refusal.
fn split_ring(m: &mut Mesh<'_>, e: (u32, u32), point: Point<'_>, min_angle: f64, crowd: f64) -> Result<(Vec<u32>, Vec<(u32, u32)>), ShapeLimit> {
    let refuse = ShapeLimit::SidesOnNoEdge;
    let depth = *m.ring.get(&e).ok_or(refuse)?;
    if depth >= MAX_RING_SPLITS {
        return Err(refuse);
    }
    let ts: Vec<u32> = m.sides.get(&e).ok_or(refuse)?.clone();
    let [t1, t2] = ts.as_slice() else { return Err(refuse) };
    let (fan, other) = match (m.is_frozen(*t1), m.is_frozen(*t2)) {
        (true, false) => (*t1, *t2),
        (false, true) => (*t2, *t1),
        _ => return Err(refuse),
    };
    let tri = *m.tris.get(other as usize).ok_or(refuse)?;
    // The side in the other triangle's order.
    let [x, y, c] = (0..3).map(|k| super::rot3(tri, k)).find(|[a, b, _]| key(*a, *b) == e).ok_or(refuse)?;
    let ftri = *m.tris.get(fan as usize).ok_or(refuse)?;
    let apex = ftri.iter().copied().find(|&v| v != x && v != y).ok_or(refuse)?;
    if !m.poles.contains(&apex) {
        return Err(refuse);
    }
    let ((pa, _), (px, qx), (py, qy), (pc, _)) = (m.node(apex).ok_or(refuse)?, m.node(x).ok_or(refuse)?, m.node(y).ok_or(refuse)?, m.node(c).ok_or(refuse)?);
    if !(0.5 * angle(pa, px, py) >= min_angle) {
        return Err(refuse);
    }
    let q = [0.5 * (qx[0] + qy[0]), 0.5 * (qx[1] + qy[1])];
    let p = point(q).ok_or(refuse)?;
    if [pa, px, py, pc].iter().any(|o| o.dist(p) < crowd) {
        return Err(ShapeLimit::Crowded);
    }
    m.ring.remove(&e);
    let Some((made, facing)) = m.insert(Located::OnSide(other, x, y), q, p) else {
        m.ring.insert(e, depth);
        return Err(refuse);
    };
    let n = u32::try_from(m.uv.len().checked_sub(1).ok_or(refuse)?).map_err(|_| refuse)?;
    m.ring.insert(key(x, n), depth + 1);
    m.ring.insert(key(n, y), depth + 1);
    // The fan triangle's two halves stay fans: the insert put them in its slot and the last one.
    if let Some(&last) = made.last() {
        for t in [fan, last] {
            if let Some(f) = m.frozen.get_mut(t as usize) {
                *f = true;
            }
        }
    }
    Ok((made, facing))
}

/// `why`, in `limits` once.
fn note(limits: &mut Vec<ShapeLimit>, why: ShapeLimit) {
    if !limits.contains(&why) {
        limits.push(why);
    }
}

/// Triangle `ti` given up on until it changes, and why, where that leaves it short.
fn give_up(m: &mut Mesh<'_>, ti: u32, why: Option<ShapeLimit>, limits: &mut Vec<ShapeLimit>) {
    if let Some(s) = m.stuck.get_mut(ti as usize) {
        *s = true;
    }
    if let Some(why) = why {
        note(limits, why);
    }
}

/// A triangle's smallest angle in 3D (radians).
pub(crate) fn smallest([a, b, c]: [Vec3; 3]) -> f64 {
    angle(a, b, c).min(angle(b, c, a)).min(angle(c, a, b))
}
