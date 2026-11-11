//! Unfold for Printing, laid flat: every face of the pattern turned about its bends onto the
//! plane of the base face ([`FlatLayout`]), the pattern split into parts at the bends chosen for
//! it, each part beside the last, and the smallest rectangle round each part, to tell whether it
//! fits a printer's bed ([`MinRect`]).
//!
//! A face hangs on its parent by a bend: turned about that edge until its normal is its
//! parent's, it lies in its parent's plane on the other side of the edge, so the whole pattern
//! lies in the base face's plane with every outer face the same way up. A bend that is split
//! starts a part of its own there: the face across it is laid out as if it had folded, then its
//! part is moved clear of the others. The two faces at a cut, and at a split, each keep the edge:
//! the pair is found again by the edge's index.

use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::print_unfold::{UnfoldGraph, UnfoldLayout};
use crate::{DocError, IDENTITY, Mat, Result, apply_point, mat_mul, rigid};

/// The gap left between two parts of a split pattern (mm).
pub const PART_GAP: f64 = 10.0;

/// The smallest rectangle round a set of points in a plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MinRect {
    /// Its longer and its shorter side (mm).
    pub long: f64,
    pub short: f64,
    /// Its corners, in order round it.
    pub corners: [Vec2; 4],
}

/// The convex hull of `pts`, counter-clockwise (Andrew's monotone chain).
fn hull(mut pts: Vec<Vec2>) -> Vec<Vec2> {
    pts.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    pts.dedup_by(|a, b| (a.x - b.x).abs() < 1e-12 && (a.y - b.y).abs() < 1e-12);
    if pts.len() < 3 {
        return pts;
    }
    let turn = |o: Vec2, a: Vec2, b: Vec2| (a - o).cross(b - o);
    let mut out: Vec<Vec2> = Vec::with_capacity(pts.len() * 2);
    for pass in 0..2 {
        let start = out.len();
        let run: Box<dyn Iterator<Item = &Vec2>> = if pass == 0 { Box::new(pts.iter()) } else { Box::new(pts.iter().rev()) };
        for p in run {
            while out.len() >= start + 2 {
                let (Some(a), Some(b)) = (out.get(out.len() - 2), out.last()) else { break };
                if turn(*a, *b, *p) > 1e-12 {
                    break;
                }
                out.pop();
            }
            out.push(*p);
        }
        out.pop();
    }
    out
}

/// The rectangles round `pts` that lie along an edge of their hull: the smallest rectangle
/// round a set of points always lies along one, so these are all that need trying.
fn rects(pts: &[Vec2]) -> Vec<MinRect> {
    let h = hull(pts.to_vec());
    let n = h.len();
    let mut out = Vec::with_capacity(n.max(1));
    for i in 0..n.max(1) {
        // Along hull edge i (a single point or two: along x).
        let along = match (h.get(i), h.get((i + 1) % n.max(1))) {
            (Some(a), Some(b)) => (*b - *a).normalized().unwrap_or(Vec2::new(1.0, 0.0)),
            _ => Vec2::new(1.0, 0.0),
        };
        let across = Vec2::new(-along.y, along.x);
        let (mut u0, mut u1, mut v0, mut v1) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
        for p in &h {
            let (u, v) = (p.dot(along), p.dot(across));
            (u0, u1, v0, v1) = (u0.min(u), u1.max(u), v0.min(v), v1.max(v));
        }
        if !(u1 >= u0 && v1 >= v0) {
            continue;
        }
        let at = |u: f64, v: f64| along * u + across * v;
        let (w, d) = (u1 - u0, v1 - v0);
        out.push(MinRect { long: w.max(d), short: w.min(d), corners: [at(u0, v0), at(u1, v0), at(u1, v1), at(u0, v1)] });
    }
    out
}

impl MinRect {
    /// The rectangle of the least area round `pts`; none for no points.
    pub fn of(pts: &[Vec2]) -> Option<MinRect> {
        rects(pts).into_iter().min_by(|a, b| (a.long * a.short).total_cmp(&(b.long * b.short)))
    }

    /// A rectangle round `pts` that fits a bed `width` by `depth` either way round, when one
    /// does: the smallest rectangle is not always the one that fits (a long thin bed).
    pub fn fitting(pts: &[Vec2], width: f64, depth: f64) -> Option<MinRect> {
        let (long, short) = (width.max(depth), width.min(depth));
        rects(pts)
            .into_iter()
            .filter(|r| r.long <= long + 1e-9 && r.short <= short + 1e-9)
            .min_by(|a, b| (a.long * a.short).total_cmp(&(b.long * b.short)))
    }
}

/// A part of the flat pattern: faces that hang together by bends that are not split.
#[derive(Clone, Debug)]
pub struct FlatPart {
    /// The face the part starts at: the base face, or the face across a split bend.
    pub root: usize,
    pub faces: Vec<usize>,
    /// The smallest rectangle round it.
    pub rect: Option<MinRect>,
    /// Pairs of its faces that lie on each other in the flat.
    pub overlaps: Vec<(usize, usize)>,
}

/// The pattern laid flat.
#[derive(Clone, Debug)]
pub struct FlatLayout {
    /// Per face: the move that takes it from the body onto the flat.
    pub moves: Vec<Mat>,
    /// Per face: the part it is in.
    pub part: Vec<usize>,
    pub parts: Vec<FlatPart>,
}

/// Whether segments `a` and `b` cross at a point inside both (touching ends do not count).
fn cross(a: (Vec2, Vec2), b: (Vec2, Vec2), tol: f64) -> bool {
    let side = |p: Vec2, q: Vec2, r: Vec2| (q - p).cross(r - p);
    let (d1, d2) = (side(a.0, a.1, b.0), side(a.0, a.1, b.1));
    let (d3, d4) = (side(b.0, b.1, a.0), side(b.0, b.1, a.1));
    let scale = (a.1 - a.0).len().max((b.1 - b.0).len()).max(1e-9);
    let t = tol * scale;
    ((d1 > t && d2 < -t) || (d1 < -t && d2 > t)) && ((d3 > t && d4 < -t) || (d3 < -t && d4 > t))
}

fn inside(loops: &[Vec<Vec2>], q: Vec2) -> bool {
    let mut inside = false;
    for lp in loops {
        let n = lp.len();
        for i in 0..n {
            let (Some(a), Some(b)) = (lp.get(i), lp.get((i + 1) % n)) else { continue };
            if (a.y > q.y) != (b.y > q.y) && q.x < a.x + (q.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                inside = !inside;
            }
        }
    }
    inside
}

fn boundary_distance(loops: &[Vec<Vec2>], q: Vec2) -> f64 {
    let mut best = f64::INFINITY;
    for lp in loops {
        let n = lp.len();
        for i in 0..n {
            let (Some(a), Some(b)) = (lp.get(i), lp.get((i + 1) % n)) else { continue };
            let ab = *b - *a;
            let t = if ab.dot(ab) > 0.0 { ((q - *a).dot(ab) / ab.dot(ab)).clamp(0.0, 1.0) } else { 0.0 };
            best = best.min((q - (*a + ab * t)).len());
        }
    }
    best
}

/// Whether two flat faces lie on each other: an edge of one crosses an edge of the other, or a
/// corner or an edge's middle of one lies well inside the other. Faces that only share an edge
/// (two faces at a bend) do not.
fn overlap(a: &[Vec<Vec2>], b: &[Vec<Vec2>], near: f64) -> bool {
    let segs = |loops: &[Vec<Vec2>]| -> Vec<(Vec2, Vec2)> {
        loops.iter().flat_map(|lp| (0..lp.len()).filter_map(|i| Some((*lp.get(i)?, *lp.get((i + 1) % lp.len())?))).collect::<Vec<_>>()).collect()
    };
    let (sa, sb) = (segs(a), segs(b));
    if sa.iter().any(|x| sb.iter().any(|y| cross(*x, *y, 1e-9))) {
        return true;
    }
    let deep = |pts: &[(Vec2, Vec2)], loops: &[Vec<Vec2>]| {
        pts.iter().flat_map(|(p, q)| [*p, (*p + *q) * 0.5]).any(|p| inside(loops, p) && boundary_distance(loops, p) > near)
    };
    deep(&sa, b) || deep(&sb, a)
}

impl UnfoldGraph {
    /// The pattern `layout` laid flat on the base face's plane, split at the bends `splits`
    /// (edges that are bends of the layout), each part [`PART_GAP`] beside the last.
    pub fn flat(&self, layout: &UnfoldLayout, splits: &[usize]) -> Result<FlatLayout> {
        if !layout.complete() {
            return Err(DocError::Invalid("the pattern can't be laid flat before every face hangs on it".into()));
        }
        let n = self.faces.len();
        // Faces in the order they hang: a face after the face it hangs on.
        let mut order = vec![layout.base];
        let mut at = 0;
        while let Some(&f) = order.get(at) {
            at += 1;
            for (g, p) in layout.parent.iter().enumerate() {
                if p.is_some_and(|(parent, _)| parent == f) && !order.contains(&g) {
                    order.push(g);
                }
            }
        }
        if order.len() != n {
            return Err(DocError::Invalid("the pattern's faces do not all hang from the base face".into()));
        }
        let mut moves = vec![IDENTITY; n];
        let mut part = vec![0usize; n];
        let mut roots = vec![layout.base];
        for &f in order.iter().skip(1) {
            let Some((parent, e)) = layout.parent.get(f).copied().flatten() else { continue };
            let (Some(edge), Some(face), Some(on)) = (self.edges.get(e), self.faces.get(f), self.faces.get(parent)) else { continue };
            let (from, to) = (face.normal(), on.normal());
            let Some(axis) = (edge.ends.1 - edge.ends.0).normalized() else { continue };
            let angle = axis.dot(from.cross(to)).atan2(from.dot(to));
            let turn = rigid(Vec3::ZERO, edge.ends.0, axis, angle);
            let (Some(above), Some(slot)) = (moves.get(parent).copied(), moves.get_mut(f)) else { continue };
            *slot = mat_mul(&above, &turn);
            let own = if splits.contains(&e) {
                roots.push(f);
                roots.len() - 1
            } else {
                part.get(parent).copied().unwrap_or(0)
            };
            if let Some(p) = part.get_mut(f) {
                *p = own;
            }
        }
        let flat_loops = |f: usize, moves: &[Mat]| -> Vec<Vec<Vec2>> {
            let (Some(face), Some(m)) = (self.faces.get(f), moves.get(f)) else { return Vec::new() };
            face.loops.iter().map(|lp| lp.iter().map(|p| apply_point(m, *p)).map(|p| Vec2::new(p.x, p.y)).collect()).collect()
        };
        // Each part after the first beside the one before it, their lower edges level.
        let bounds = |faces: &[usize], moves: &[Mat]| -> Option<(Vec2, Vec2)> {
            let pts: Vec<Vec2> = faces.iter().flat_map(|f| flat_loops(*f, moves)).flatten().collect();
            let first = *pts.first()?;
            Some(pts.iter().fold((first, first), |(lo, hi), p| (Vec2::new(lo.x.min(p.x), lo.y.min(p.y)), Vec2::new(hi.x.max(p.x), hi.y.max(p.y)))))
        };
        let members = |k: usize| -> Vec<usize> { (0..n).filter(|f| part.get(*f) == Some(&k)).collect() };
        let mut right: Option<(f64, f64)> = None;
        for k in 0..roots.len() {
            let faces = members(k);
            let Some((lo, hi)) = bounds(&faces, &moves) else { continue };
            if let Some((x, y)) = right {
                let shift = rigid(Vec3::new(x + PART_GAP - lo.x, y - lo.y, 0.0), Vec3::ZERO, Vec3::Z, 0.0);
                for f in &faces {
                    if let Some(m) = moves.get_mut(*f) {
                        *m = mat_mul(&shift, m);
                    }
                }
                right = Some((x + PART_GAP + (hi.x - lo.x), y));
            } else {
                right = Some((hi.x, lo.y));
            }
        }
        let parts = roots
            .iter()
            .enumerate()
            .map(|(k, root)| {
                let faces = members(k);
                let loops: Vec<Vec<Vec<Vec2>>> = faces.iter().map(|f| flat_loops(*f, &moves)).collect();
                let pts: Vec<Vec2> = loops.iter().flatten().flatten().copied().collect();
                let mut overlaps = Vec::new();
                for i in 0..faces.len() {
                    for j in i + 1..faces.len() {
                        if let (Some(a), Some(b), Some(fa), Some(fb)) = (loops.get(i), loops.get(j), faces.get(i), faces.get(j))
                            && overlap(a, b, self.near)
                        {
                            overlaps.push((*fa, *fb));
                        }
                    }
                }
                FlatPart { root: *root, faces, rect: MinRect::of(&pts), overlaps }
            })
            .collect();
        Ok(FlatLayout { moves, part, parts })
    }
}

impl FlatLayout {
    /// Every face as a flat sheet of its own where the layout puts it: the surface model, flat.
    pub fn patches(&self, graph: &UnfoldGraph) -> Result<Vec<solidcraft_kernel::Body>> {
        graph
            .faces
            .iter()
            .zip(&self.moves)
            .map(|(f, m)| {
                let plane = solidcore_record::Plane {
                    origin: apply_point(m, f.plane.origin),
                    x: crate::apply_vector(m, f.plane.x),
                    y: crate::apply_vector(m, f.plane.y),
                };
                Ok(solidcraft_kernel::patch_region(&plane, &f.region())?)
            })
            .collect()
    }

    /// The corners of the faces of part `k`, flat.
    pub fn points(&self, graph: &UnfoldGraph, k: usize) -> Vec<Vec2> {
        let Some(part) = self.parts.get(k) else { return Vec::new() };
        part.faces
            .iter()
            .filter_map(|f| Some((graph.faces.get(*f)?, self.moves.get(*f)?)))
            .flat_map(|(face, m)| face.loops.iter().flatten().map(|p| apply_point(m, *p)).map(|p| Vec2::new(p.x, p.y)).collect::<Vec<_>>())
            .collect()
    }
}
