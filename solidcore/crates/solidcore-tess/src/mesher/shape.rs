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
//!   circle, taken to (u, v) by the triangle's own map from its plane (its corners' (u, v) at
//!   the circle centre's barycentric coordinates), then put on the surface there.
//! - A point that would encroach on a boundary side (lie within the ball the side is a diameter
//!   of, or past it) is not added: the side is split instead, which the face cannot do alone,
//!   since the side is an edge's segment its neighbour shares. The sides are returned, to be put
//!   on their edges ([`super::plan_at`]), or, where the face may not split them, kept whole.
//! - A point within half a side of a pole of the surface is not added: towards a pole the
//!   parameters collapse, and refinement would crowd there without end.

use std::collections::VecDeque;

use crate::hash::BitsMap;
use crate::vec::VecExt as _;
use crate::ShapeLimit;

use solidcore::Vec3;

use super::size::{SizeAt, local};

/// A side is split once it is this many times the side asked (as [`super::cdt`]'s refinement).
const LONG: f64 = 1.5;
/// Flips per triangle a pass of Lawson flips may make.
const FLIPS_PER_TRIANGLE: usize = 8;
/// A point this many sides asked from a pole is too near it ([`ShapeLimit::Pole`]).
const POLE_GUARD: f64 = 0.5;
/// Steps of the walk to a new point, per triangle.
const WALK: usize = 4;

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
    /// The surface's poles on the face (their 3D points).
    pub poles: &'a [Vec3],
}

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
            if self.fixed.contains_key(&e) {
                continue;
            }
            let Some(&[t1, t2]) = self.sides.get(&e).map(Vec::as_slice) else { continue };
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
        for _ in 0..self.tris.len().saturating_mul(WALK).max(16) {
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
                    if self.fixed.contains_key(&key(a, b)) {
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
                if self.fixed.contains_key(&key(x, y)) {
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
    point: Point<'_>,
    rules: &Rules,
) -> Shaped {
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
    let mut m = Mesh { uv, pos, tris, sides: BitsMap::default(), fixed, stuck };
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
    let mut queue: VecDeque<u32> = (0..m.tris.len()).filter_map(|t| u32::try_from(t).ok()).collect();
    while let Some(ti) = queue.pop_front() {
        let Some(&t) = m.tris.get(ti as usize) else { continue };
        if m.stuck.get(ti as usize).copied().unwrap_or(true) || m.collapsed(t) {
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
                !m.fixed.contains_key(&key(i, j)) && off([(ui[0] + uj[0]) * 0.5, (ui[1] + uj[1]) * 0.5], (pi + pj) * 0.5) > rules.tol
            });
        let sharp = sharpest < rules.min_angle;
        if !(sharp || long || sags) {
            continue;
        }
        // A corner of the boundary sharper than the angle, between two of its sides, stays.
        let [i, j, k] = super::rot3(t, at);
        if sharp && !long && !sags && m.fixed.contains_key(&key(i, j)) && m.fixed.contains_key(&key(k, i)) {
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
        let q = [qa[0] + l1 * (qb[0] - qa[0]) + l2 * (qc[0] - qa[0]), qa[1] + l1 * (qb[1] - qa[1]) + l2 * (qc[1] - qa[1])];
        if !(q[0].is_finite() && q[1].is_finite()) {
            give_up(&mut m, ti, None, &mut limits);
            continue;
        }
        let located = m.locate(ti, q);
        let p = match located {
            Located::Inside(_) | Located::OnSide(..) => point(q),
            Located::Beyond(..) | Located::Lost => None,
        };
        // The boundary sides it encroaches on: the one it lies past, and those of the triangle
        // it lands in and of that triangle's neighbours whose diametral ball holds it.
        let mut near: Vec<(u32, u32)> = Vec::new();
        match located {
            Located::Beyond(x, y) => near.push(key(x, y)),
            Located::Inside(tj) | Located::OnSide(tj, ..) => {
                if let (Some(p), Some(&tri)) = (p, m.tris.get(tj as usize)) {
                    let mut around: Vec<u32> = vec![tj];
                    for k in 0..3 {
                        let [x, y, _] = super::rot3(tri, k);
                        around.extend(m.across(x, y, tj));
                    }
                    for tk in around {
                        let Some(&tk) = m.tris.get(tk as usize) else { continue };
                        for k in 0..3 {
                            let [x, y, _] = super::rot3(tk, k);
                            let e = key(x, y);
                            if !m.fixed.contains_key(&e) || near.contains(&e) {
                                continue;
                            }
                            let (Some((px, _)), Some((py, _))) = (m.node(x), m.node(y)) else { continue };
                            if px != py && p.dist((px + py) * 0.5) < 0.5 * px.dist(py) {
                                near.push(e);
                            }
                        }
                    }
                }
            }
            Located::Lost => {}
        }
        if !near.is_empty() {
            match rules.whole {
                None => {
                    for e in near {
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
        if rules.poles.iter().any(|pole| p.dist(*pole) < POLE_GUARD * h_at(p)) {
            give_up(&mut m, ti, Some(ShapeLimit::Pole), &mut limits);
            continue;
        }
        let Some((made, facing)) = m.insert(located, q, p) else {
            give_up(&mut m, ti, None, &mut limits);
            continue;
        };
        added += 1;
        let flipped = m.flip(facing, per);
        queue.extend(made);
        queue.extend(flipped);
    }
    if !encroached.is_empty() {
        return Shaped::Split(encroached.into_values().map(|(a, b)| (a as usize, b as usize, vec![0.5])).collect());
    }
    // The triangles still too sharp, by why they stayed.
    Shaped::Done(limits)
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
