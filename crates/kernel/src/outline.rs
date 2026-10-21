//! Union of bodies touching back to back over part of a planar face, imprinted along the
//! outline of the contact itself.
//!
//! [`crate::coplanar::imprint_union`] splits each body's face along the other's sides, extended
//! to whole planes and cylinders. Where a side's surface runs on into the contact (the inner
//! wall of a lip swept round a corner, whose straight part is tangent to its round part), the
//! two bodies' pieces don't match and the faces can't be glued. Here each face in the plane is
//! divided only by the other body's edges in the plane, as far as they run across it: both
//! bodies' faces in the plane are lines and circular arcs, every edge is cut where the other's
//! edges cross or touch it, the other's edges lying inside a face are copied into it, and the
//! face is rebuilt as the regions those edges bound. The common regions are then the same faces
//! in both bodies, and [`crate::coplanar::glue`] takes them away.

use std::collections::HashMap;
use std::f64::consts::TAU;

use mt::{BoundedCurve, ParametricCurve};
use solidcraft_geom::{Vec2, Vec3};
use truck_modeling as mt;

use crate::body::{Body, from_p3};
use crate::history::History;
use crate::record::Rebuild;
use crate::{KernelError, Result, guard, record};

/// Most edges of a body's faces in the plane the outline imprint takes on.
const MAX_EDGES: usize = 2000;

/// A frame in the plane, `x × y` its normal.
#[derive(Clone, Copy)]
struct Frame {
    o: Vec3,
    x: Vec3,
    y: Vec3,
}

impl Frame {
    /// The plane `n · p = d` (`n` a unit normal).
    fn new(n: Vec3, d: f64) -> Frame {
        let x = n.any_perp();
        Frame { o: n * d, x, y: n.cross(x) }
    }

    fn normal(&self) -> Vec3 {
        self.x.cross(self.y)
    }

    /// The same plane seen from its other side.
    fn flipped(&self) -> Frame {
        Frame { o: self.o, x: self.x, y: -self.y }
    }

    fn local(&self, p: Vec3) -> Vec2 {
        let v = p - self.o;
        Vec2::new(v.dot(self.x), v.dot(self.y))
    }

    fn world(&self, q: Vec2) -> Vec3 {
        self.o + self.x * q.x + self.y * q.y
    }
}

/// A line or circular arc in the plane, in the direction of the edge it is.
#[derive(Clone, Copy, Debug)]
enum Piece {
    Line {
        a: Vec2,
        b: Vec2,
    },
    /// From the angle `t0` about `c`, turning by `sweep` (counter-clockwise when positive).
    Arc {
        c: Vec2,
        r: f64,
        t0: f64,
        sweep: f64,
    },
}

/// A fixed direction for counting crossings, clear of the axes and of round angles.
const RAY: (f64, f64) = (0.951_056_516_295_153_6, 0.309_016_994_374_947_45);

impl Piece {
    fn at(&self, s: f64) -> Vec2 {
        match *self {
            Piece::Line { a, b } => a.lerp(b, s),
            Piece::Arc { c, r, t0, sweep } => c + Vec2::from_angle(t0 + sweep * s) * r,
        }
    }

    fn len(&self) -> f64 {
        match *self {
            Piece::Line { a, b } => a.dist(b),
            Piece::Arc { r, sweep, .. } => r * sweep.abs(),
        }
    }

    /// Where the angle `t` lies along the arc (0 at its start, 1 at its end), if it does.
    fn arc_fraction(t0: f64, sweep: f64, t: f64) -> Option<f64> {
        let turned = if sweep >= 0.0 { (t - t0).rem_euclid(TAU) } else { (t0 - t).rem_euclid(TAU) };
        let span = sweep.abs();
        if turned <= span {
            Some(turned / span)
        } else if span >= TAU - 1e-12 {
            Some(0.0)
        } else {
            None
        }
    }

    /// Distance from `p` to the piece.
    fn dist(&self, p: Vec2) -> f64 {
        match *self {
            Piece::Line { a, b } => {
                let d = b - a;
                let l2 = d.len2();
                let s = if l2 > 0.0 { ((p - a).dot(d) / l2).clamp(0.0, 1.0) } else { 0.0 };
                p.dist(a + d * s)
            }
            Piece::Arc { c, r, t0, sweep } => match Self::arc_fraction(t0, sweep, (p - c).angle()) {
                Some(_) => ((p - c).len() - r).abs(),
                None => p.dist(self.at(0.0)).min(p.dist(self.at(1.0))),
            },
        }
    }

    /// How many times the ray from `p` along [`RAY`] crosses the piece.
    fn crossings(&self, p: Vec2) -> usize {
        let dir = Vec2::new(RAY.0, RAY.1);
        match *self {
            Piece::Line { a, b } => {
                let e = b - a;
                let den = dir.cross(e);
                if den.abs() < 1e-300 {
                    return 0;
                }
                let w = a - p;
                let t = w.cross(e) / den;
                let s = w.cross(dir) / den;
                usize::from(t > 0.0 && (0.0..1.0).contains(&s))
            }
            Piece::Arc { c, r, t0, sweep } => {
                // p + t·dir on the circle: t² + 2t(w·dir) + |w|² − r² = 0, w = p − c.
                let w = p - c;
                let bq = w.dot(dir);
                let disc = bq * bq - (w.len2() - r * r);
                if !(disc > 0.0) {
                    return 0;
                }
                let root = disc.sqrt();
                [-bq - root, -bq + root]
                    .into_iter()
                    .filter(|t| *t > 0.0)
                    .filter(|t| Self::arc_fraction(t0, sweep, (p + dir * *t - c).angle()).is_some_and(|f| f < 1.0 || sweep.abs() >= TAU - 1e-12))
                    .count()
            }
        }
    }

    /// The unit direction from the piece's start (`end` false) or end (`end` true) toward a
    /// point a little along it: the direction it leaves or enters its vertex, with its bend.
    fn leaving(&self, end: bool) -> Option<Vec2> {
        let len = self.len();
        let s = if len > 0.0 { (1e-3 / len).min(0.25) } else { 0.25 };
        let (v, q) = if end { (self.at(1.0), self.at(1.0 - s)) } else { (self.at(0.0), self.at(s)) };
        (q - v).normalized()
    }
}

/// The edge `e` (in its own direction) as a line or arc in the frame's plane; `None` when it is
/// off the plane or neither.
fn piece_of(e: &mt::Edge, fr: &Frame, tol: f64) -> Option<Piece> {
    let c = e.oriented_curve();
    let (t0, t1) = c.range_tuple();
    let n = fr.normal();
    let pts: Vec<Vec3> = (0..=8).map(|k| from_p3(c.subs(t0 + (t1 - t0) * k as f64 / 8.0))).collect();
    if pts.iter().any(|p| (*p - fr.o).dot(n).abs() > tol) {
        return None;
    }
    let q: Vec<Vec2> = pts.iter().map(|p| fr.local(*p)).collect();
    let (&a, &b, &m) = (q.first()?, q.last()?, q.get(4)?);
    let closed = a.dist(b) <= tol;
    if !closed {
        let line = Piece::Line { a, b };
        if q.iter().all(|p| line.dist(*p) <= tol) {
            return Some(line);
        }
    }
    // The circle through three of the points.
    let (p1, p2, p3) = if closed { (a, *q.get(3)?, *q.get(6)?) } else { (a, m, b) };
    let (u, v) = (p2 - p1, p3 - p1);
    let den = 2.0 * u.cross(v);
    if den.abs() < 1e-300 {
        return None;
    }
    let centre = p1 + Vec2::new(v.y * u.len2() - u.y * v.len2(), u.x * v.len2() - v.x * u.len2()) * (1.0 / den);
    let r = (a - centre).len();
    if !(r > tol) || q.iter().any(|p| ((*p - centre).len() - r).abs() > tol) {
        return None;
    }
    let start = (a - centre).angle();
    let ccw = (a - centre).cross(*q.get(1)? - centre) > 0.0;
    let sweep = if closed {
        if ccw { TAU } else { -TAU }
    } else if ccw {
        ((b - centre).angle() - start).rem_euclid(TAU)
    } else {
        -((start - (b - centre).angle()).rem_euclid(TAU))
    };
    // The samples run along it in order (a rational arc is not even in angle).
    let mut last = -1e-9;
    for p in q.iter().skip(1).take(7) {
        let f = Piece::arc_fraction(start, sweep, (*p - centre).angle())?;
        if f < last {
            return None;
        }
        last = f;
    }
    Some(Piece::Arc { c: centre, r, t0: start, sweep })
}

/// Where two pieces cross or touch (each point within `tol` of both).
fn meetings(p: &Piece, q: &Piece, tol: f64) -> Vec<Vec2> {
    let mut out: Vec<Vec2> = Vec::new();
    let line_circle = |a: Vec2, b: Vec2, c: Vec2, r: f64, out: &mut Vec<Vec2>| {
        let Some(dir) = (b - a).normalized() else { return };
        let foot = a + dir * (c - a).dot(dir);
        let h = (c - foot).len();
        if h > r + tol {
            return;
        }
        if (h - r).abs() <= tol {
            out.push(foot);
        } else {
            let k = (r * r - h * h).max(0.0).sqrt();
            out.extend([foot - dir * k, foot + dir * k]);
        }
    };
    match (*p, *q) {
        (Piece::Line { a, b }, Piece::Line { a: c, b: d }) => {
            let (e, f) = (b - a, d - c);
            let den = e.cross(f);
            if den.abs() > 1e-12 * e.len() * f.len() {
                let s = (c - a).cross(f) / den;
                out.push(a + e * s);
            }
        }
        (Piece::Line { a, b }, Piece::Arc { c, r, .. }) | (Piece::Arc { c, r, .. }, Piece::Line { a, b }) => line_circle(a, b, c, r, &mut out),
        (Piece::Arc { c: c1, r: r1, .. }, Piece::Arc { c: c2, r: r2, .. }) => {
            let d = c1.dist(c2);
            if d > tol && d <= r1 + r2 + tol && d + tol >= (r1 - r2).abs() {
                let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
                let h = (r1 * r1 - a * a).max(0.0).sqrt();
                let u = (c2 - c1) * (1.0 / d);
                let base = c1 + u * a;
                if h <= tol {
                    out.push(base);
                } else {
                    out.extend([base + u.perp() * h, base - u.perp() * h]);
                }
            }
        }
    }
    // The ends of either lying on the other (a T, or where overlapping pieces part).
    for (x, y) in [(p, q), (q, p)] {
        for end in [x.at(0.0), x.at(1.0)] {
            if y.dist(end) <= tol {
                out.push(end);
            }
        }
    }
    out.retain(|m| p.dist(*m) <= tol && q.dist(*m) <= tol);
    out
}

/// The edges of `body`'s faces in the plane of `fr` that face along its normal, once each, in
/// their absolute direction, with their pieces. `None` when one is neither a line nor an arc.
fn plane_edges(body: &Body, faces: &[usize], fr: &Frame, tol: f64) -> Option<Vec<(mt::Edge, Piece)>> {
    let fs: Vec<&mt::Face> = body.solid.face_iter().collect();
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for f in faces.iter().filter_map(|i| fs.get(*i)) {
        for e in f.edge_iter() {
            if !seen.insert(e.id()) {
                continue;
            }
            let abs = if e.orientation() { e.clone() } else { e.inverse() };
            out.push((abs.clone(), piece_of(&abs, fr, tol)?));
            if out.len() > MAX_EDGES {
                return None;
            }
        }
    }
    Some(out)
}

/// Points where the edges of `own` are to be cut so that every piece of them lies inside,
/// outside or along the edges of `other` (in world coordinates).
fn cuts(own: &[(mt::Edge, Piece)], other: &[(mt::Edge, Piece)], fr: &Frame, tol: f64) -> Vec<Vec3> {
    let mut out = Vec::new();
    for (_, p) in own {
        for (_, q) in other {
            for m in meetings(p, q, tol) {
                if m.dist(p.at(0.0)) > tol && m.dist(p.at(1.0)) > tol {
                    out.push(fr.world(m));
                }
            }
        }
    }
    out
}

/// One edge use in a face being rebuilt: the edge as the face uses it, its piece in that
/// direction, its ends, and its reverse use when it is a copy used both ways.
struct Use {
    edge: mt::Edge,
    piece: Piece,
    from: mt::VertexID,
    to: mt::VertexID,
    twin: Option<usize>,
}

/// Does the point lie inside the region bounded by `pieces` (closed loops, holes included)?
fn inside(pieces: &[Piece], p: Vec2) -> bool {
    pieces.iter().map(|x| x.crossings(p)).sum::<usize>() % 2 == 1
}

/// The loops of uses, each next use the one leaving the vertex furthest clockwise from the way
/// the last one came in: each loop runs round one region with it on the left.
fn loops(uses: &[Use]) -> Option<Vec<Vec<usize>>> {
    let mut out_of: HashMap<mt::VertexID, Vec<usize>> = HashMap::new();
    for (i, u) in uses.iter().enumerate() {
        out_of.entry(u.from).or_default().push(i);
    }
    let mut used = vec![false; uses.len()];
    let mut out = Vec::new();
    for start in 0..uses.len() {
        if used.get(start).copied().unwrap_or(true) {
            continue;
        }
        let mut lp = vec![start];
        if let Some(x) = used.get_mut(start) {
            *x = true;
        }
        let mut cur = start;
        loop {
            let u = uses.get(cur)?;
            let back = u.piece.leaving(true)?;
            let next = out_of
                .get(&u.to)?
                .iter()
                .copied()
                .filter(|k| Some(*k) != u.twin)
                .filter_map(|k| {
                    let d = uses.get(k)?.piece.leaving(false)?;
                    // Counter-clockwise from the way back, in (0, 2π].
                    let turn = back.cross(d).atan2(back.dot(d));
                    Some((k, if turn <= 0.0 { turn + TAU } else { turn }))
                })
                .max_by(|x, y| x.1.total_cmp(&y.1))
                .map(|(k, _)| k)?;
            if next == start {
                break;
            }
            if used.get(next).copied().unwrap_or(true) || lp.len() > uses.len() {
                return None;
            }
            if let Some(x) = used.get_mut(next) {
                *x = true;
            }
            lp.push(next);
            cur = next;
        }
        out.push(lp);
    }
    Some(out)
}

/// The signed area a loop bounds (positive counter-clockwise).
fn area(uses: &[Use], lp: &[usize]) -> f64 {
    let pts: Vec<Vec2> = lp.iter().filter_map(|i| uses.get(*i)).flat_map(|u| (0..16).map(move |k| u.piece.at(k as f64 / 16.0))).collect();
    (0..pts.len()).map(|i| pts.get(i).zip(pts.get((i + 1) % pts.len())).map(|(a, b)| a.cross(*b)).unwrap_or(0.0)).sum::<f64>() * 0.5
}

/// `body` with its faces `faces` (in the plane of `fr`, facing along its normal) divided by those
/// edges of `other` that run inside them: each such edge is copied into the face, and the face
/// is rebuilt as the regions its own edges and the copies bound. History over `[body]`. `None`
/// when a face can't be rebuilt so.
fn divided(body: &Body, faces: &[usize], other: &[(mt::Edge, Piece)], fr: &Frame, tol: f64) -> Result<Option<Body>> {
    let fs: Vec<&mt::Face> = body.solid.face_iter().collect();
    // Each face's own edge uses, in the direction it runs them (counter-clockwise about the
    // frame's normal round its outside).
    let mut own: Vec<(usize, Vec<(mt::Edge, Piece)>)> = Vec::new();
    for &fi in faces {
        let Some(f) = fs.get(fi) else { continue };
        let mut es = Vec::new();
        for w in f.boundaries() {
            for e in w.edge_iter() {
                let Some(p) = piece_of(e, fr, tol) else { return Ok(None) };
                es.push((e.clone(), p));
            }
        }
        own.push((fi, es));
    }
    let all_own: Vec<Piece> = own.iter().flat_map(|(_, es)| es.iter().map(|(_, p)| *p)).collect();
    // The vertices the copies end on: the faces' own where they meet, else new ones shared.
    let mut pool: Vec<mt::Vertex> = own.iter().flat_map(|(_, es)| es.iter().flat_map(|(e, _)| [e.front().clone(), e.back().clone()])).collect();
    let mut vertex = |p: Vec3| -> mt::Vertex {
        if let Some(v) = pool.iter().find(|v| from_p3(v.point()).dist(p) <= tol) {
            return v.clone();
        }
        let v = mt::Vertex::new(crate::body::p3(p));
        pool.push(v.clone());
        v
    };
    let mut copies: HashMap<usize, Vec<(mt::Edge, Piece)>> = HashMap::new();
    for (e, p) in other {
        // Along the faces' own edges: nothing to add.
        if [0.25, 0.5, 0.75].iter().all(|s| all_own.iter().any(|o| o.dist(p.at(*s)) <= tol * 10.0)) {
            continue;
        }
        let mid = p.at(0.5);
        let Some((fi, _)) = own.iter().find(|(_, es)| inside(&es.iter().map(|(_, x)| *x).collect::<Vec<_>>(), mid)) else { continue };
        let (v0, v1) = (vertex(from_p3(e.front().point())), vertex(from_p3(e.back().point())));
        // On the face's own vertices, which lie within `tol` of the edge's ends.
        let copy = mt::Edge::new_unchecked(&v0, &v1, e.curve());
        let Some(cp) = piece_of(&copy, fr, tol) else { return Ok(None) };
        copies.entry(*fi).or_default().push((copy, cp));
    }
    if copies.is_empty() {
        return Ok(None);
    }
    let mut rebuilt: HashMap<usize, Vec<mt::Face>> = HashMap::new();
    for (fi, es) in &own {
        let Some(cs) = copies.get(fi) else { continue };
        let Some(f) = fs.get(*fi) else { continue };
        let mut uses: Vec<Use> =
            es.iter().map(|(e, p)| Use { edge: e.clone(), piece: *p, from: e.front().id(), to: e.back().id(), twin: None }).collect();
        for (e, p) in cs {
            let k = uses.len();
            let back = e.inverse();
            let Some(bp) = piece_of(&back, fr, tol) else { return Ok(None) };
            uses.push(Use { edge: e.clone(), piece: *p, from: e.front().id(), to: e.back().id(), twin: Some(k + 1) });
            uses.push(Use { from: back.front().id(), to: back.back().id(), edge: back, piece: bp, twin: Some(k) });
        }
        let Some(lps) = loops(&uses) else { return Ok(None) };
        let (outer, holes): (Vec<(Vec<usize>, f64)>, Vec<(Vec<usize>, f64)>) = lps
            .into_iter()
            .map(|lp| {
                let a = area(&uses, &lp);
                (lp, a)
            })
            .partition(|(_, a)| *a > 0.0);
        let pieces_of = |lp: &[usize]| -> Vec<Piece> { lp.iter().filter_map(|i| uses.get(*i)).map(|u| u.piece).collect() };
        let mut groups: Vec<(Vec<usize>, f64, Vec<Vec<usize>>)> = outer.into_iter().map(|(lp, a)| (lp, a, Vec::new())).collect();
        for (hole, _) in holes {
            let Some(p) = hole.first().and_then(|i| uses.get(*i)).map(|u| u.piece.at(0.5)) else { return Ok(None) };
            let Some(g) = groups.iter_mut().filter(|(lp, _, _)| inside(&pieces_of(lp), p)).min_by(|x, y| x.1.total_cmp(&y.1)) else {
                return Ok(None);
            };
            g.2.push(hole);
        }
        let wire = |lp: &[usize]| -> mt::Wire { lp.iter().filter_map(|i| uses.get(*i)).map(|u| u.edge.clone()).collect::<Vec<_>>().into() };
        let mut made = Vec::new();
        for (lp, _, hs) in &groups {
            let wires: Vec<mt::Wire> = std::iter::once(wire(lp)).chain(hs.iter().map(|h| wire(h))).collect();
            // The loops run round the face as its normal sees them; a face turned against its
            // surface stores them the other way.
            let nf = if f.orientation() {
                mt::Face::try_new(wires, f.surface())
            } else {
                mt::Face::try_new(wires.iter().map(|w| w.inverse()).collect(), f.surface()).map(|mut x| {
                    x.invert();
                    x
                })
            };
            made.push(nf.map_err(|x| KernelError::Failed(format!("rebuilding a face of the contact: {x}")))?);
        }
        rebuilt.insert(*fi, made);
    }
    let mut rec = Rebuild::default();
    let mut index = 0;
    let mut shells = Vec::new();
    for sh in body.solid.boundaries() {
        let mut faces = Vec::new();
        for f in sh.face_iter() {
            match rebuilt.get(&index) {
                Some(made) => {
                    for nf in made {
                        rec.whole(nf, f);
                        faces.push(nf.clone());
                    }
                }
                None => {
                    rec.whole(f, f);
                    faces.push(f.clone());
                }
            }
            index += 1;
        }
        shells.push(mt::Shell::from(faces));
    }
    let Ok(solid) = mt::Solid::try_new(shells) else { return Ok(None) };
    let split = Body {
        solid: std::sync::Arc::new(solid),
        size: 0.0,
        boundary_tol: body.boundary_tol,
        volume: Some(std::sync::Arc::new(std::sync::Mutex::new(None))),
        coarse_missing: None,
        mesh: None,
        color: body.color,
        paint: None,
        splits: body.splits.clone(),
        face_sources: None,
        history: None,
        regularization_protected: true,
        display_of: None,
        meshes: None,
        phased: false,
        tolerances: None,
    };
    Ok(Some(record::rebuilt_body(split, &[&body.solid], &rec)))
}

/// Union of bodies touching back to back over part of a planar face whose edges there are lines
/// and circular arcs (a lip swept along a rim, flush with a round side): imprinted on each other
/// along the outline of the contact and glued. `None` when that does not give a result
/// `accept`ed.
pub(crate) fn outline_union(a: &Body, b: &Body, accept: &dyn Fn(&Body) -> bool) -> Option<Body> {
    let size = a.size().max(b.size());
    let tol = (size * 1e-7).max(1e-9) * 10.0;
    let back: Vec<(Vec3, f64)> = crate::coplanar::coincident_planes(a, b).into_iter().filter(|(_, _, same)| !*same).map(|(n, d, _)| (n, d)).collect();
    if back.is_empty() {
        return None;
    }
    let run = || -> Result<Option<Body>> {
        let (mut a2, mut ha): (Body, Option<History>) = (a.clone(), crate::boolean_record::kept(a, 0, 1));
        let (mut b2, mut hb): (Body, Option<History>) = (b.clone(), crate::boolean_record::kept(b, 0, 1));
        for (n, d) in &back {
            // a's faces face along n in the plane; b's the other way.
            let fa = Frame::new(*n, *d);
            let fb = fa.flipped();
            let in_a = |x: &Body| crate::coplanar::faces_in(x, *n, *d, tol);
            let in_b = |x: &Body| crate::coplanar::faces_in(x, -*n, -*d, tol);
            let (Some(ea), Some(eb)) = (plane_edges(&a2, &in_a(&a2), &fa, tol), plane_edges(&b2, &in_b(&b2), &fa, tol)) else { return Ok(None) };
            let a3 = crate::splitface::split_edges_at(&a2, &cuts(&ea, &eb, &fa, tol))?;
            ha = crate::boolean_record::through(&a3, 0, ha.as_ref());
            let b3 = crate::splitface::split_edges_at(&b2, &cuts(&eb, &ea, &fa, tol))?;
            hb = crate::boolean_record::through(&b3, 0, hb.as_ref());
            // Each side's edges as the other side's frame sees them.
            let (Some(eb), Some(ea)) = (plane_edges(&b3, &in_b(&b3), &fa, tol), plane_edges(&a3, &in_a(&a3), &fb, tol)) else { return Ok(None) };
            let a4 = divided(&a3, &in_a(&a3), &eb, &fa, tol)?.unwrap_or(a3);
            let b4 = divided(&b3, &in_b(&b3), &ea, &fb, tol)?.unwrap_or(b3);
            ha = crate::boolean_record::through(&a4, 0, ha.as_ref());
            hb = crate::boolean_record::through(&b4, 0, hb.as_ref());
            (a2, b2) = (a4, b4);
        }
        let Some(r) = crate::coplanar::glue(&a2, &b2) else { return Ok(None) };
        let r = r?;
        let (ha, hb) = (crate::boolean_record::placed(ha, 0, 2), crate::boolean_record::placed(hb, 1, 2));
        let h = crate::boolean_record::of_pair(&r, &ha, &hb);
        Ok(Some(r.with_checked_history(h, &record::counts(&[a, b]))))
    };
    match guard("outline union", run) {
        Ok(Some(r)) if accept(&r) => Some(r),
        _ => None,
    }
}
