//! Faces narrower than the tolerance (slivers) left out, their sides made one. Some writers leave a
//! face a fraction of a µm wide where two faces meet at a corner: a few µm of a fillet's torus
//! past the line its neighbour ends on, bounded by two long edges that lie within the tolerance of
//! each other and short edges that do too. Such a face has no area in its own parameters (its
//! pcurves meet), so no record holds it. It is the line its two long edges both are: the face is
//! left out, the short edges' vertices made one (each short edge with them), and its two long
//! edges made one, so the faces either side of it meet along it. A face narrower than a loose
//! tolerance may be one the part has (a chamfer a few µm wide on a tiny part), so the body is read
//! with its slivers first, and only the slivers that read names as refused are left out
//! ([`Assembly::finish`]).

use crate::vec::Vec3Geom as _;
use std::collections::{HashMap, HashSet};

use crate::kernel;
use solidcore_record::Vec3;

use super::{Assembly, Bound, R, index};

/// The most pieces a certified length bound may halve an edge into, asked whether it is no
/// longer than the tolerance ([`Assembly::edge_length_within`]): a sliver's short edge is at most
/// the tolerance and the enclosure of the spans as written already settles that, or the enclosure
/// 64 halvings on does (the kernel sums it every 64). A curve that settles neither is not proved
/// short, and its face is kept, so the bound costs this many pieces at most. An edge whose chord
/// lies within its own rounding carries no upper end at all (a degenerate curve), and asking it
/// for a bracket narrowed to the tolerance would halve to the kernel's own cap of 65536 pieces
/// without ever settling, on every such edge of every such file.
const SHORT_EDGE_PIECES: usize = 64;

/// Union-find root of `v` in `parent` (a vertex not in it is its own).
fn root(parent: &HashMap<u32, u32>, mut v: u32) -> u32 {
    while let Some(&p) = parent.get(&v) {
        if p == v {
            break;
        }
        v = p;
    }
    v
}

/// What collapsing one sliver face does.
struct Collapse {
    face: usize,
    /// The short edges, left out wherever they are used.
    short: Vec<u32>,
    /// Each vertex merged into another, to its class's representative.
    merged: HashMap<u32, u32>,
    /// The long edge kept, the one made it, and whether that one runs against it.
    kept: u32,
    gone: u32,
    against: bool,
    /// A proved bound on how far apart the two long edges lie (mm), at most `tol`: the kept
    /// one's tolerance covers it.
    apart: f64,
}

impl Assembly<'_> {
    /// Whether any face is a sliver ([`Self::sliver`]).
    pub(crate) fn has_slivers(&self) -> R<bool> {
        for fi in 0..self.faces.len() {
            if self.sliver(fi)?.is_some() {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Leave out each sliver face of `shells` (above) whose file face is in `named` (one a read
    /// refused), returning the shells with the faces renumbered and how many were left out. A
    /// sliver not named reads as it is, and is kept.
    pub(crate) fn collapse_slivers(&mut self, shells: Vec<Vec<(u32, bool)>>, named: &[u64]) -> R<(Vec<Vec<(u32, bool)>>, usize)> {
        let mut gone: HashSet<usize> = HashSet::new();
        for fi in 0..self.faces.len() {
            if !self.faces.get(fi).is_some_and(|f| named.contains(&f.origin)) {
                continue;
            }
            if let Some(c) = self.sliver(fi)? {
                self.apply(&c);
                gone.insert(c.face);
            }
        }
        if gone.is_empty() {
            return Ok((shells, 0));
        }
        let mut renumber = vec![None; self.faces.len()];
        let mut next = 0usize;
        for (fi, slot) in renumber.iter_mut().enumerate() {
            if !gone.contains(&fi) {
                *slot = Some(index(next)?);
                next += 1;
            }
        }
        let mut at = 0usize;
        self.faces.retain(|_| {
            at += 1;
            !gone.contains(&(at - 1))
        });
        let shells = shells
            .into_iter()
            .map(|s| s.into_iter().filter_map(|(f, fwd)| renumber.get(f as usize).copied().flatten().map(|n| (n, fwd))).collect())
            .collect();
        Ok((shells, gone.len()))
    }

    /// How face `fi` collapses, where it is a sliver: one loop of edges, two of them long and
    /// used by one other face each (not the same one), lying within the tolerance of each other, and the rest no
    /// longer than the tolerance, their vertices made one leaving the long two between the same
    /// two vertices; and where that leaves every other loop with an edge and no other edge
    /// running from a vertex to itself.
    fn sliver(&self, fi: usize) -> R<Option<Collapse>> {
        let Some(face) = self.faces.get(fi) else { return Ok(None) };
        let [Bound::Edges(es)] = face.loops.as_slice() else { return Ok(None) };
        if es.len() < 2 {
            return Ok(None);
        }
        let tol = self.tol;
        let mut short = Vec::new();
        let mut long = Vec::new();
        for &(e, forward) in es {
            if self.edge_length_within(e, tol)? {
                short.push(e);
            } else {
                long.push((e, forward));
            }
        }
        let [(a, _), (b, _)] = long.as_slice() else { return Ok(None) };
        let (a, b) = (*a, *b);
        if a == b {
            return Ok(None);
        }
        // Each long edge bounds one other face.
        let mut uses: HashMap<u32, usize> = HashMap::new();
        for f in &self.faces {
            for l in &f.loops {
                if let Bound::Edges(es) = l {
                    for (e, _) in es {
                        *uses.entry(*e).or_default() += 1;
                    }
                }
            }
        }
        if uses.get(&a) != Some(&2) || uses.get(&b) != Some(&2) {
            return Ok(None);
        }
        // ... and not the same one, which would then run along one edge twice.
        let bounding = |e: u32| {
            self.faces
                .iter()
                .enumerate()
                .position(|(gi, g)| gi != fi && g.loops.iter().any(|l| matches!(l, Bound::Edges(es) if es.iter().any(|u| u.0 == e))))
        };
        if bounding(a) == bounding(b) {
            return Ok(None);
        }
        // The short edges' vertices, one class each.
        let mut parent: HashMap<u32, u32> = HashMap::new();
        for &e in &short {
            let [p, q] = self.edges.get(e as usize).ok_or("edge reference")?.vertices;
            let (rp, rq) = (root(&parent, p), root(&parent, q));
            if rp != rq {
                let (lo, hi) = (rp.min(rq), rp.max(rq));
                parent.insert(hi, lo);
                parent.entry(lo).or_insert(lo);
            }
        }
        let merged: HashMap<u32, u32> = parent.keys().map(|&v| (v, root(&parent, v))).filter(|(v, r)| v != r).collect();
        let class = |v: u32| merged.get(&v).copied().unwrap_or(v);
        let (ea, eb) = (self.edges.get(a as usize).ok_or("edge reference")?, self.edges.get(b as usize).ok_or("edge reference")?);
        // The long two run between the same two vertices, once those are made one, and not
        // round to where they start.
        let (pa, pb) = ([class(ea.vertices[0]), class(ea.vertices[1])], [class(eb.vertices[0]), class(eb.vertices[1])]);
        if pa[0] == pa[1] || !((pa == pb) || (pa == [pb[1], pb[0]])) {
            return Ok(None);
        }
        // No other edge is made to run from a vertex to itself, and no other loop is left
        // without an edge.
        let short_set: HashSet<u32> = short.iter().copied().collect();
        for (e, x) in self.edges.iter().enumerate() {
            let e = index(e)?;
            if !short_set.contains(&e) && x.vertices[0] != x.vertices[1] && class(x.vertices[0]) == class(x.vertices[1]) && uses.contains_key(&e) {
                return Ok(None);
            }
        }
        for (gi, g) in self.faces.iter().enumerate() {
            if gi == fi {
                continue;
            }
            for l in &g.loops {
                if let Bound::Edges(es) = l
                    && es.iter().all(|(e, _)| short_set.contains(e))
                {
                    return Ok(None);
                }
            }
        }
        // The two long edges lie within the tolerance of each other, both ways: the kernel
        // encloses every point of each over its whole span. Where it cannot prove that, the
        // face is kept.
        let Some(apart) = self.edges_together(a, b, tol)? else { return Ok(None) };
        let against = pa[0] != pb[0];
        Ok(Some(Collapse { face: fi, short, merged, kept: a, gone: b, against, apart }))
    }

    /// Whether edge `e` is no longer than `tol`: its ends' gap, and a certified upper bound on
    /// its length over its whole span ([`kernel::curve_length_at_most`], never a sampled path),
    /// settled rather than narrowed to `tol`, and within [`SHORT_EDGE_PIECES`] pieces: an edge
    /// far longer than the tolerance is proved more instead of being halved towards a width it
    /// can never reach, and a degenerate one is not proved short rather than halved to the cap.
    pub(crate) fn edge_length_within(&self, e: u32, tol: f64) -> R<bool> {
        let edge = self.edges.get(e as usize).ok_or("edge reference")?;
        let [p, q] = edge.vertices;
        let (vp, vq) = (self.vertices.get(p as usize).ok_or("vertex reference")?, self.vertices.get(q as usize).ok_or("vertex reference")?);
        if Vec3::from(vp.point).dist(Vec3::from(vq.point)) > tol {
            return Ok(false);
        }
        // An edge the kernel cannot settle is not proved short, and the face is kept.
        Ok(kernel::curve_length_at_most(&edge.curve, edge.interval, tol, SHORT_EDGE_PIECES)?.unwrap_or(false))
    }

    /// Whether the two long edges lie within `tol` of each other, both ways, and so how far
    /// apart they are at most: the kernel's whole-span enclosure ([`kernel::curves_within`]),
    /// never samples. `None` where it cannot prove that, and the face is not collapsed.
    pub(crate) fn edges_together(&self, a: u32, b: u32, tol: f64) -> R<Option<f64>> {
        let (x, y) = (self.edges.get(a as usize).ok_or("edge reference")?, self.edges.get(b as usize).ok_or("edge reference")?);
        Ok(match kernel::curves_within(&x.curve, x.interval, &y.curve, y.interval, tol)? {
            Some(true) => Some(tol),
            _ => None,
        })
    }

    /// Collapse a sliver ([`Self::sliver`]): its vertices made one, its short edges left out of
    /// every loop, its long edges made one, and its loop left empty (the face is taken out by
    /// the caller).
    fn apply(&mut self, c: &Collapse) {
        // Each merged vertex's tolerance covers its representative's, and how far apart they lie.
        for (&v, &r) in &c.merged {
            let (Some(x), Some(y)) = (self.vertices.get(v as usize), self.vertices.get(r as usize)) else { continue };
            let reach = x.tolerance + Vec3::from(x.point).dist(Vec3::from(y.point));
            if let Some(y) = self.vertices.get_mut(r as usize) {
                y.tolerance = y.tolerance.max(reach);
            }
        }
        let class = |v: u32| c.merged.get(&v).copied().unwrap_or(v);
        for edge in &mut self.edges {
            edge.vertices = [class(edge.vertices[0]), class(edge.vertices[1])];
        }
        let short: HashSet<u32> = c.short.iter().copied().collect();
        for face in &mut self.faces {
            for l in &mut face.loops {
                match l {
                    Bound::Edges(es) => {
                        es.retain(|(e, _)| !short.contains(e));
                        for u in es.iter_mut() {
                            if u.0 == c.gone {
                                *u = (c.kept, u.1 != c.against);
                            }
                        }
                    }
                    Bound::Vertex(v) => *v = class(*v),
                }
            }
        }
        if let Some(face) = self.faces.get_mut(c.face) {
            face.loops.clear();
        }
        let gone_tolerance = self.edges.get(c.gone as usize).map_or(0.0, |e| e.tolerance);
        if let Some(kept) = self.edges.get_mut(c.kept as usize) {
            kept.tolerance = kept.tolerance.max(gone_tolerance).max(c.apart);
        }
    }
}
