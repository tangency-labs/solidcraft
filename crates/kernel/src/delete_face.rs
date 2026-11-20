//! Delete Face with healing: faces are removed and their planar neighbours extend to close the
//! gap, as when a fillet, chamfer, boss or hole is taken off a part.
//!
//! The body is rebuilt from its own schema 4 record. Faces away from the gap are kept as they
//! are. Each neighbouring face (planar) keeps its plane and its edges that don't touch the gap;
//! a loop that ran entirely along deleted faces is dropped for a hole's rim or a boss's
//! footprint, or restored at the moved corners for a face enclosed by rounds. A corner on the
//! gap moves to where its planes meet the neighbours across the gap (a fillet's tangent points
//! go to the sharp corner), and the stretch of loop that bordered deleted faces closes with a
//! straight edge between the moved corners. A corner left on one neighbour only, with nowhere
//! to move (the apex of a V notch or groove), drops out of that edge. Where the gap is left
//! between neighbours on one plane, that plane extends across it: the region is filled and the
//! faces on the plane become one. Anything else (curved neighbours, a gap the neighbours cannot
//! close) is refused, naming the faces and the way out.

use solidcraft_geom::prelude::*;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use solidcore_ops::cap::{is_face_on_plane, is_point_on_plane};
use solidcore_record::Vec3;
use solidcore_record::brep::{Brep, Curve, Frame, Loop, Surface};
use solidcraft_geom::Aabb3;

use crate::assemble::{AFace, AUse, Assembly};
use crate::body::Body;
use crate::history::{Change, FaceProv, faces_of};
use crate::offset::{Surf, surf_of};
use crate::{KernelError, Result, guard};

fn fail(m: impl Into<String>) -> KernelError {
    KernelError::Invalid(format!("Delete Face: {}", m.into()))
}

type Key = (i64, i64, i64);

/// A short name for the kind of surface face `f` lies on, for refusal messages that tell the
/// user what a face is and how to get past the refusal.
fn surface_kind(rec: &solidcore_record::brep::Brep, f: usize) -> &'static str {
    match surf_of(rec, f) {
        Some(Surf::Plane { .. }) => "a plane",
        Some(Surf::Cylinder { .. }) => "a cylinder",
        Some(Surf::Sphere { .. }) => "a sphere",
        Some(Surf::Cone { .. }) => "a cone",
        Some(Surf::Torus { .. }) => "a torus (as a fillet or round often is)",
        None => "a free-form (spline) face",
    }
}

/// The lowest curved neighbour `f` of a chosen face `by` that `takes` does not take, as `(f, by)`.
fn curved_neighbour(rec: &Brep, del: &BTreeSet<usize>, takes: impl Fn(usize) -> bool) -> Option<(usize, usize)> {
    let mut found: BTreeSet<(usize, usize)> = BTreeSet::new();
    for &d in del {
        let Some(face) = rec.faces.get(d) else { continue };
        for l in &face.loops {
            let Loop::Edges { edges } = l else { continue };
            for oe in edges {
                let Some(e) = rec.edges.get(oe.edge as usize) else { continue };
                for f in faces_of(e) {
                    if !del.contains(&f) && !matches!(surf_of(rec, f), Some(Surf::Plane { .. })) && !takes(f) {
                        found.insert((f, d));
                    }
                }
            }
        }
    }
    found.into_iter().next()
}

/// The refusal for a gap that a curved neighbour `f` of face `by` cannot close.
fn curved_refusal(rec: &Brep, by: usize, f: usize) -> KernelError {
    fail(format!(
        "face {by} ({}), borders face {f}, {}: include face {f} in the selection if it should go too, or take off the feature that made face {f} in the timeline instead; only planar neighbours can extend to close the gap",
        surface_kind(rec, by),
        surface_kind(rec, f),
    ))
}

/// Key of a point for sharing vertices and edges between rebuilt faces.
fn key(p: Vec3, q: f64) -> Key {
    let r = |x: f64| (x / q).round() as i64;
    (r(p.x), r(p.y), r(p.z))
}

/// The vertices and straight edges the rebuilt faces share, by where they are.
struct Shared {
    q: f64,
    vertices: HashMap<Key, u32>,
    lines: HashMap<(Key, Key), u32>,
    arcs: HashMap<(Key, Key, Key), u32>,
}

impl Shared {
    fn vertex(&mut self, asm: &mut Assembly, p: Vec3) -> Result<u32> {
        let k = key(p, self.q);
        if let Some(v) = self.vertices.get(&k) {
            return Ok(*v);
        }
        let v = asm.vertex(p)?;
        self.vertices.insert(k, v);
        Ok(v)
    }

    /// The straight edge from `a` to `b`, made once and used both ways.
    fn line(&mut self, asm: &mut Assembly, a: Vec3, b: Vec3) -> Result<AUse> {
        let (ka, kb) = (key(a, self.q), key(b, self.q));
        if let Some(e) = self.lines.get(&(ka, kb)) {
            return Ok(AUse { edge: *e, forward: true, given: None });
        }
        if let Some(e) = self.lines.get(&(kb, ka)) {
            return Ok(AUse { edge: *e, forward: false, given: None });
        }
        let (va, vb) = (self.vertex(asm, a)?, self.vertex(asm, b)?);
        let e = asm.line(va, vb)?;
        self.lines.insert((ka, kb), e);
        Ok(AUse { edge: e, forward: true, given: None })
    }

    /// The arc from `a` to `b` about `c` normal to `axis`, on `out`'s side of the chord (where the
    /// deleted face's material lay), made once and used both ways.
    fn arc(&mut self, asm: &mut Assembly, a: Vec3, b: Vec3, c: Vec3, axis: Vec3, out: Vec3) -> Result<AUse> {
        let (ka, kb, kc) = (key(a, self.q), key(b, self.q), key(c, self.q));
        if let Some(e) = self.arcs.get(&(ka, kb, kc)) {
            return Ok(AUse { edge: *e, forward: true, given: None });
        }
        if let Some(e) = self.arcs.get(&(kb, ka, kc)) {
            return Ok(AUse { edge: *e, forward: false, given: None });
        }
        let arc = solidcore_ops::cylinder_circle::arc_toward(arr(a), arr(b), arr(c), arr(axis), arr(out), self.q * 10.0)
            .ok_or_else(|| fail("an arc with no side to go round"))?;
        let (va, vb) = (self.vertex(asm, a)?, self.vertex(asm, b)?);
        let frame = Frame { origin: arr(c), x_axis: arc.x_axis, y_axis: arc.y_axis };
        let e = asm.edge(Curve::Circle { frame, radius: arc.radius }, [0.0, arc.sweep], va, vb)?;
        self.arcs.insert((ka, kb, kc), e);
        Ok(AUse { edge: e, forward: true, given: None })
    }
}

/// A cylinder neighbour that closes round over the gap: axis point, unit axis and radius.
type Wrap = solidcore_ops::cylinder_circle::Cylinder;

/// The circle where plane `n·x = d` cuts a cylinder of `wraps` square to it, if `a` and `b`
/// both lie on it: its centre and axis.
fn wrap_circle(wraps: &BTreeMap<usize, Wrap>, n: Vec3, d: f64, a: Vec3, b: Vec3, tol: f64) -> Option<(Vec3, Vec3)> {
    wraps.values().find_map(|w| Some((Vec3::from(w.plane_circle(arr(n), d, arr(a), arr(b), tol)?), Vec3::from(w.axis))))
}

fn arr(p: Vec3) -> [f64; 3] {
    [p.x, p.y, p.z]
}

/// The corners along a gap stretch, closed to `a`: straight edges between them, with corners
/// that coincide or lie in line with their neighbours dropped (two rounds meeting in a mitre
/// leave the sharp corner between them).
fn close(pts: Vec<Vec3>, a: Vec3, q: f64) -> Vec<(Vec3, Vec3)> {
    let mut c: Vec<Vec3> = Vec::new();
    for p in pts.into_iter().chain(std::iter::once(a)) {
        if c.last().is_some_and(|l| l.dist(p) <= q * 10.0) {
            continue;
        }
        if let [.., x, y] = c[..]
            && (y - x).cross(p - y).len() <= q * 10.0 * (p - x).len()
            && (y - x).dot(p - y) > 0.0
        {
            c.pop();
        }
        c.push(p);
    }
    c.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))).collect()
}

/// The edges closing a gap stretch through `pts` back to `a` on a plane neighbour: one arc on a
/// wrapping cylinder's circle (its ends stay), else straight edges ([`close`]).
#[allow(clippy::too_many_arguments)]
fn closing(
    shared: &mut Shared,
    asm: &mut Assembly,
    pts: Vec<Vec3>,
    a: Vec3,
    plane: Option<(Vec3, f64)>,
    out: Option<Vec3>,
    wraps: &BTreeMap<usize, Wrap>,
) -> Result<Vec<AUse>> {
    let q = shared.q;
    if let (Some((n, d)), Some(out), Some(&first)) = (plane, out, pts.first())
        && key(first, q) != key(a, q)
        && let Some((c, ax)) = wrap_circle(wraps, n, d, first, a, q * 100.0)
    {
        return Ok(vec![shared.arc(asm, first, a, c, ax, out)?]);
    }
    close(pts, a, q).into_iter().map(|(x, y)| shared.line(asm, x, y)).collect()
}

/// Whether the use's edge has a corner that moves.
fn corner_moves(asm: &Assembly, moved: &HashMap<u32, Vec3>, u: &AUse) -> bool {
    asm.ends(u).is_some_and(|(a, b)| moved.contains_key(&a) || moved.contains_key(&b))
}

/// Where vertex `v` is after the corners move.
fn position(asm: &Assembly, moved: &HashMap<u32, Vec3>, v: u32) -> Result<Vec3> {
    moved.get(&v).copied().or_else(|| asm.point(v)).ok_or_else(|| fail("vertex"))
}

/// Whether face `g` lies on `plane` within `tol`: its own plane points the same way and every
/// corner of its loops is on `plane` ([`is_face_on_plane`]).
fn face_on_plane(asm: &Assembly, planes: &HashMap<usize, (Vec3, f64)>, g: usize, plane: (Vec3, f64), tol: f64) -> bool {
    let (Some(own), Some(face)) = (planes.get(&g), asm.faces.get(g).and_then(Option::as_ref)) else { return false };
    let corners: Vec<[f64; 3]> = face.loops.iter().flatten().filter_map(|u| asm.ends(u)).filter_map(|(a, _)| asm.point(a)).map(arr).collect();
    is_face_on_plane(arr(own.0), &corners, arr(plane.0), plane.1, tol)
}

/// Face `i`, or the face it was merged into.
fn surviving(alias: &HashMap<usize, usize>, mut i: usize) -> usize {
    while let Some(j) = alias.get(&i) {
        i = *j;
    }
    i
}

/// Loop `a` and loop `b` joined along the edge `e` that each uses once (in opposite
/// directions): `a` from after `e` round to before it, then `b` likewise. `None` when `e` is
/// not used exactly once in each, both run it the same way, or the joined loop would use an
/// edge twice.
fn splice(a: &[AUse], b: &[AUse], e: u32) -> Option<Vec<AUse>> {
    let once = |l: &[AUse]| if l.iter().filter(|u| u.edge == e).count() == 1 { l.iter().position(|u| u.edge == e) } else { None };
    let (i, j) = (once(a)?, once(b)?);
    if a.get(i)?.forward == b.get(j)?.forward {
        return None;
    }
    let out: Vec<AUse> = a.iter().skip(i + 1).chain(a.iter().take(i)).chain(b.iter().skip(j + 1)).chain(b.iter().take(j)).cloned().collect();
    let edges: HashSet<u32> = out.iter().map(|u| u.edge).collect();
    (!out.is_empty() && edges.len() == out.len()).then_some(out)
}

/// Close what the rebuilt faces leave open where the gap lies between neighbours on one plane
/// (a V notch through a wall, a V groove across a top). Only the closing edges the deletion made
/// count (`opened`, each with the deleted face it stands in for); a sheet's own free boundary
/// stays as it is. Those used by one face only make loops; a loop on a neighbour's plane is that
/// plane extended across the gap, a new face on it merged with the faces of that plane it
/// borders. A loop on no neighbour's plane cannot close (one face of a box: its sides are
/// parallel) and is refused, naming the deleted face (`fallback` when none is known).
fn fill_on_planes(
    asm: &mut Assembly,
    planes: &HashMap<usize, (Vec3, f64)>,
    tol: f64,
    history: &mut Vec<(usize, FaceProv)>,
    opened: &HashMap<u32, usize>,
    fallback: usize,
) -> Result<()> {
    let mut uses: HashMap<u32, Vec<(usize, AUse)>> = HashMap::new();
    for (fi, f) in asm.faces.iter().enumerate() {
        for u in f.iter().flat_map(|f| &f.loops).flatten() {
            if opened.contains_key(&u.edge) {
                uses.entry(u.edge).or_default().push((fi, u.clone()));
            }
        }
    }
    let cannot = |e: u32| {
        let f = opened.get(&e).copied().unwrap_or(fallback);
        fail(format!("the gap left by face {f} cannot close: no neighbouring plane extends across it"))
    };
    // The other side of each edge used once, by where it starts, with the face that uses it.
    let mut open: Vec<u32> = uses.iter().filter(|(_, us)| us.len() == 1).map(|(e, _)| *e).collect();
    open.sort_unstable();
    let mut next: HashMap<u32, (usize, AUse)> = HashMap::new();
    for e in &open {
        let Some((fi, u)) = uses.get(e).and_then(|us| us.first()).cloned() else { continue };
        let back = AUse { edge: *e, forward: !u.forward, given: None };
        let (a, _) = asm.ends(&back).ok_or_else(|| fail("edge"))?;
        if next.insert(a, (fi, back)).is_some() {
            return Err(cannot(*e));
        }
    }
    // Faces merged away, to the face they became part of.
    let mut alias: HashMap<usize, usize> = HashMap::new();
    let mut done: HashSet<u32> = HashSet::new();
    for e in open {
        if done.contains(&e) {
            continue;
        }
        let Some(first) = next.values().find(|(_, u)| u.edge == e).map(|(_, u)| u.clone()) else { continue };
        let (start, _) = asm.ends(&first).ok_or_else(|| fail("edge"))?;
        // Walk the loop, with the face on the other side of each edge. A chain that does not
        // close (it runs into a sheet's free boundary) is left open.
        let (mut lp, mut beside): (Vec<AUse>, Vec<usize>) = (Vec::new(), Vec::new());
        let mut at = start;
        let closed = loop {
            let Some((fi, u)) = next.get(&at).cloned() else { break false };
            if !done.insert(u.edge) {
                return Err(cannot(e));
            }
            at = asm.ends(&u).ok_or_else(|| fail("edge"))?.1;
            lp.push(u);
            beside.push(surviving(&alias, fi));
            if at == start {
                break true;
            }
        };
        if !closed {
            continue;
        }
        // The neighbour plane the loop lies on: every corner on it, and every curved edge on a
        // face of that plane.
        let on = |pl: (Vec3, f64)| {
            lp.iter().zip(&beside).all(|(u, fi)| {
                let corners_on = asm
                    .ends(u)
                    .is_some_and(|(a, b)| [a, b].iter().all(|v| asm.point(*v).is_some_and(|p| is_point_on_plane(arr(p), arr(pl.0), pl.1, tol))));
                let straight = matches!(asm.edges.get(u.edge as usize).map(|x| &x.curve), Some(Curve::Line { .. }));
                corners_on && (straight || face_on_plane(asm, planes, *fi, pl, tol))
            })
        };
        let Some(owner) = beside.iter().copied().find(|fi| planes.get(fi).is_some_and(|pl| on(*pl))) else {
            return Err(cannot(e));
        };
        let plane = *planes.get(&owner).ok_or_else(|| fail("plane"))?;
        let template = asm.faces.get(owner).and_then(Option::as_ref).ok_or_else(|| fail("face"))?.clone();
        // Merge the new face with the faces of that plane it borders, one shared edge at a time;
        // where two outer loops cannot be joined the new face stays a face of its own.
        let mut merged = AFace { loops: vec![lp], domain: None, ..template };
        let mut into: Option<usize> = None;
        let mut origins = Vec::new();
        loop {
            let found = beside.iter().copied().find_map(|g| {
                if Some(g) == into || !face_on_plane(asm, planes, g, plane, tol) {
                    return None;
                }
                let face = asm.faces.get(g).and_then(Option::as_ref)?;
                let (m, o) = (merged.loops.first()?, face.loops.first()?);
                let e = m.iter().map(|u| u.edge).find(|e| o.iter().any(|x| x.edge == *e))?;
                Some((g, splice(m, o, e)?))
            });
            let Some((g, mut joined)) = found else { break };
            let mut face = asm.faces.get_mut(g).and_then(Option::take).ok_or_else(|| fail("face"))?;
            // A face on another surface (or NURBS form) keeps no pcurves: they are about its own
            // parameters, not the merged face's, and are made again on the merged face's surface.
            if face.surface != merged.surface || face.nurbs != merged.nurbs {
                let theirs: HashSet<u32> = face.loops.iter().flatten().map(|u| u.edge).collect();
                for u in joined.iter_mut().chain(face.loops.iter_mut().skip(1).flatten()) {
                    if theirs.contains(&u.edge) {
                        u.given = None;
                    }
                }
            }
            let holes: Vec<Vec<AUse>> = merged.loops.drain(1..).chain(face.loops.into_iter().skip(1)).collect();
            merged.loops = std::iter::once(joined).chain(holes).collect();
            let prov = history.iter().position(|(fi, _)| *fi == g).map(|k| history.remove(k).1);
            match into {
                None => into = Some(g),
                Some(k) => {
                    alias.insert(g, k);
                }
            }
            origins.extend(prov.into_iter().flat_map(|p| p.origins));
        }
        match into {
            Some(g) => {
                if let Some(slot) = asm.faces.get_mut(g) {
                    *slot = Some(merged);
                }
                history.push((g, FaceProv { origins, change: Change::Modified }));
            }
            None => {
                asm.face(merged);
                history.push((asm.faces.len() - 1, crate::record::from_face(0, owner, Change::Modified)));
            }
        }
    }
    Ok(())
}

/// Delete faces (indices in the body's face order, as in its mesh's `tri_face`) and heal the
/// gap by extending the neighbouring planes. The result's history is over `[body]`.
pub fn delete_faces(body: &Body, faces: &[usize]) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "delete_faces");
    body.require_brep("Delete Face")?;
    let rec = crate::native::brep_record(body)?;
    let n = rec.faces.len();
    if faces.is_empty() {
        return Err(fail("no faces given"));
    }
    let del: BTreeSet<usize> = faces.iter().copied().collect();
    if let Some(bad) = del.iter().find(|f| **f >= n) {
        return Err(fail(format!("there is no face {bad} (the body has {n})")));
    }
    if del.len() >= n {
        return Err(fail("that would delete every face"));
    }
    let size = body.size().max(1e-9);
    // Classified before any healing: a curved neighbour the healer cannot take goes to fillet
    // removal. A record the healer cannot assemble takes none, so its error stands only when
    // every neighbour is planar.
    let gap = guard("delete face", || Gap::new(&rec, &del, size));
    let takes = |f: usize| gap.as_ref().is_ok_and(|g| g.takes(f));
    if let Some((f, by)) = curved_neighbour(&rec, &del, takes) {
        let chosen: Vec<usize> = del.iter().copied().collect();
        return crate::offset_backend::delete_fillet_faces(body, &chosen).map_err(|e| {
            log::debug!(target: "smlib", "delete face: fillet removal refused too: {e}");
            curved_refusal(&rec, by, f)
        });
    }
    guard("delete face", || heal(gap?, &rec, &del, size, body))
}

/// The planar healer: the body rebuilt with the deleted faces gone and their neighbours closing
/// the gap, over the neighbours [`Gap::new`] classified.
fn heal(gap: Gap, rec: &Brep, del: &BTreeSet<usize>, size: f64, body: &Body) -> Result<Body> {
    let n = rec.faces.len();
    let Gap { mut asm, efaces, vfaces, neighbours, planes, kept_whole, wraps, .. } = gap;
    let gap_edge = |e: u32| on_gap(&efaces, del, e);
    let outward = |f: usize| match surf_of(rec, f) {
        Some(Surf::Plane { n, .. }) => Some(n),
        _ => None,
    };
    // The surface a wrap face takes: its cylinder's frame starts at the kept seam.
    let mut seams: HashMap<usize, Surface> = HashMap::new();
    // The deleted faces' vertices.
    let gap_vertices: BTreeSet<u32> = vfaces.iter().filter(|(_, fs)| fs.iter().any(|f| del.contains(f))).map(|(v, _)| *v).collect();
    // Corners across the gap: three neighbour planes meeting near the deleted faces.
    let gap: Aabb3 = gap_vertices.iter().filter_map(|v| asm.point(*v)).collect();
    let near_gap = gap.inflated(gap.diagonal().max(size * 1e-3));
    let on = |p: Vec3, f: usize| planes.get(&f).is_some_and(|(n, d)| (n.dot(p) - d).abs() <= size * 1e-6);
    let nb: Vec<(Vec3, f64)> = neighbours.iter().filter_map(|f| planes.get(f).copied()).collect();
    let mut corners: Vec<Vec3> = Vec::new();
    for (i, a) in nb.iter().enumerate() {
        for (j, b) in nb.iter().enumerate().skip(i + 1) {
            for c in nb.iter().skip(j + 1) {
                let den = a.0.dot(b.0.cross(c.0));
                if den.abs() < 1e-9 {
                    continue;
                }
                let x = (b.0.cross(c.0) * a.1 + c.0.cross(a.0) * b.1 + a.0.cross(b.0) * c.1) / den;
                if x.is_finite() && near_gap.contains(x) {
                    corners.push(x);
                }
            }
        }
    }
    // Where each vertex on the gap goes: the nearest corner on all of its remaining planes.
    let mut moved: HashMap<u32, Vec3> = HashMap::new();
    // Corners on a single neighbour with no corner to go to (a V notch's apex): the
    // neighbour's gap stretch closes straight past them.
    let mut dropped: HashSet<u32> = HashSet::new();
    // The plane of a face that keeps a gap vertex: a neighbour's, or (a face meeting the gap
    // only at a corner) its own where it is planar.
    let kept_plane = |f: usize| -> Option<(Vec3, f64)> {
        planes.get(&f).copied().or_else(|| match surf_of(rec, f) {
            Some(Surf::Plane { n, d }) => Some((n, d)),
            _ => None,
        })
    };
    for v in &gap_vertices {
        let Some(fs) = vfaces.get(v) else { continue };
        let keep: Vec<usize> = fs.iter().copied().filter(|x| !del.contains(x)).collect();
        if keep.is_empty() {
            continue;
        }
        if keep.len() >= 3 {
            // Three or more remaining faces hold the vertex where they meet, unless their
            // planes all run through one line (a ridge whose round is deleted at its end):
            // then it slides along that line to the corner the gap closes at, as a vertex of
            // two faces does.
            let Some(kp) = keep.iter().map(|f| kept_plane(*f)).collect::<Option<Vec<_>>>() else { continue };
            let pinned = kp
                .iter()
                .enumerate()
                .any(|(i, a)| kp.iter().enumerate().skip(i + 1).any(|(j, b)| kp.iter().skip(j + 1).any(|c| a.0.dot(b.0.cross(c.0)).abs() >= 1e-9)));
            if pinned {
                continue;
            }
            let p = asm.point(*v).ok_or_else(|| fail("vertex"))?;
            let on_all = |c: Vec3| kp.iter().all(|(n, d)| (n.dot(c) - d).abs() <= size * 1e-6);
            if let Some(c) = corners.iter().filter(|c| on_all(**c)).min_by(|a, b| a.dist(p).total_cmp(&b.dist(p))) {
                moved.insert(*v, *c);
            }
            continue;
        }
        // A corner of a loop that goes whole goes with it.
        if keep.iter().all(|x| kept_whole.contains(x)) {
            continue;
        }
        // A corner on a cylinder that closes round stays where it is (the arcs meet there).
        if keep.iter().any(|x| wraps.contains_key(x)) && keep.iter().all(|x| planes.contains_key(x) || wraps.contains_key(x)) {
            continue;
        }
        if keep.iter().any(|x| !planes.contains_key(x)) {
            let f = fs.iter().find(|x| del.contains(x)).copied().unwrap_or(0);
            return Err(fail(format!("a corner of face {f} touches a face that cannot move")));
        }
        let p = asm.point(*v).ok_or_else(|| fail("vertex"))?;
        let best = corners.iter().filter(|c| keep.iter().all(|x| on(**c, *x))).min_by(|a, b| a.dist(p).total_cmp(&b.dist(p)));
        if let Some(c) = best {
            moved.insert(*v, *c);
        } else if keep.len() == 1 {
            dropped.insert(*v);
        }
    }
    let q = size * 1e-7;
    let mut shared = Shared { q, vertices: HashMap::new(), lines: HashMap::new(), arcs: HashMap::new() };
    // The deleted face each closing edge stands in for, to name it if the gap cannot close.
    let mut closed_by: HashMap<u32, usize> = HashMap::new();
    for (i, v) in asm.vertices.iter().enumerate() {
        if !moved.contains_key(&(i as u32)) {
            shared.vertices.insert(key(Vec3::from(v.point), q), i as u32);
        }
    }
    let mut history: Vec<(usize, FaceProv)> = Vec::new();
    let mut rebuilt: Vec<(usize, AFace)> = Vec::new();
    for fi in 0..n {
        if del.contains(&fi) {
            continue;
        }
        let f = asm.faces.get(fi).and_then(Option::as_ref).ok_or_else(|| fail("face"))?.clone();
        if !neighbours.contains(&fi) {
            // Untouched, unless a corner moves: a plane meeting the gap only at a corner sliding along
            // its edge (a ridge) follows it when every moved corner stays on it; no other face can.
            if f.loops.iter().flatten().any(|u| corner_moves(&asm, &moved, u)) {
                let follows = match surf_of(rec, fi) {
                    Some(Surf::Plane { n, d }) => f
                        .loops
                        .iter()
                        .flatten()
                        .filter_map(|u| asm.ends(u))
                        .flat_map(|(a, b)| [a, b])
                        .filter_map(|v| moved.get(&v))
                        .all(|c| (n.dot(*c) - d).abs() <= size * 1e-6),
                    _ => false,
                };
                if !follows {
                    return Err(fail(format!("face {fi} meets the gap only at a corner; it cannot follow")));
                }
            } else {
                history.push((fi, crate::record::from_face(0, fi, Change::Kept)));
                continue;
            }
        }
        let mut loops: Vec<Vec<AUse>> = Vec::new();
        for l in &f.loops {
            let gaps = l.iter().filter(|u| gap_edge(u.edge)).count();
            if gaps == 0 && !l.iter().any(|u| corner_moves(&asm, &moved, u)) {
                loops.push(l.clone());
                continue;
            }
            if let Some(w) = wraps.get(&fi).copied()
                && gaps == 2
            {
                // A cylinder closing round: the gap's first ruling gives way to two arcs.
                let on: Vec<usize> = (0..l.len()).filter(|k| l.get(*k).is_some_and(|u| gap_edge(u.edge))).collect();
                let (Some(&i), Some(&j)) = (on.first(), on.get(1)) else { return Err(fail("edge")) };
                let (Some(ui), Some(uj)) = (l.get(i), l.get(j)) else { return Err(fail("edge")) };
                let (vp, vq) = asm.ends(ui).ok_or_else(|| fail("edge"))?;
                let (vr, vs) = asm.ends(uj).ok_or_else(|| fail("edge"))?;
                let pt = |v: u32| asm.point(v).ok_or_else(|| fail("vertex"));
                let (p, pq, r, ps) = (pt(vp)?, pt(vq)?, pt(vr)?, pt(vs)?);
                let (ax, centre) = (Vec3::from(w.axis), |x: Vec3| Vec3::from(w.centre_at(arr(x))));
                let (c1, c2) = (centre(p), centre(r));
                let (x_axis, y_axis) = w.frame_at(arr(r));
                seams.insert(fi, Surface::Cylinder { frame: Frame { origin: w.origin, x_axis, y_axis }, radius: w.radius });
                let by = efaces.get(&ui.edge).and_then(|fs| fs.iter().find(|x| del.contains(x)).copied());
                let out = by.and_then(outward).ok_or_else(|| fail(format!("face {fi}: the gap over it is not flat")))?;
                let top = shared.arc(&mut asm, p, ps, c1, ax, out)?;
                let bottom = shared.arc(&mut asm, r, pq, c2, ax, out)?;
                // Every use's pcurve is made again on the face's new form (the record's
                // pcurves are on its old, narrower one).
                let mut edges = Vec::with_capacity(l.len() + 2);
                for (k, u) in l.iter().enumerate() {
                    if k == i {
                        edges.push(top.clone());
                        edges.push(AUse { edge: uj.edge, forward: !uj.forward, given: None });
                        edges.push(bottom.clone());
                    } else {
                        edges.push(AUse { given: None, ..u.clone() });
                    }
                }
                if let Some(d) = by {
                    closed_by.entry(top.edge).or_insert(d);
                    closed_by.entry(bottom.edge).or_insert(d);
                }
                loops.push(edges);
                continue;
            }
            if gaps == l.len() {
                // A hole or boss footprint disappears with its walls. A face ringed by rounds does
                // not: its boundary moves back to the remaining planes' intersections (#1557).
                if !l.iter().any(|u| corner_moves(&asm, &moved, u)) {
                    continue;
                }
                let mut boundary = Vec::new();
                for u in l {
                    let (a, b) = asm.ends(u).ok_or_else(|| fail("edge"))?;
                    let (Some(pa), Some(pb)) = (moved.get(&a).copied(), moved.get(&b).copied()) else {
                        return Err(fail(format!("face {fi} has a closed gap boundary that cannot be restored completely")));
                    };
                    if pa.dist(pb) > q * 10.0 {
                        let restored = shared.line(&mut asm, pa, pb)?;
                        asm.piece(restored.edge, u.edge);
                        boundary.push(restored);
                    }
                }
                if !boundary.is_empty() {
                    loops.push(boundary);
                }
                continue;
            }
            // Start after a gap stretch so stretches don't wrap around.
            let m = l.len();
            let is_gap = |k: usize| l.get(k).is_some_and(|u| gap_edge(u.edge));
            let start = (0..m).find(|i| is_gap((i + m - 1) % m) && !is_gap(*i)).unwrap_or(0);
            let mut edges: Vec<AUse> = Vec::new();
            let mut pending: Option<Vec<Vec3>> = None;
            let mut pending_by: Option<usize> = None;
            for k in 0..m {
                let Some(u) = l.get((start + k) % m) else { continue };
                let (va, vb) = asm.ends(u).ok_or_else(|| fail("edge"))?;
                let (a, b) = (position(&asm, &moved, va)?, position(&asm, &moved, vb)?);
                if gap_edge(u.edge) {
                    let stretch = pending.get_or_insert_with(Vec::new);
                    stretch.extend([(va, a), (vb, b)].into_iter().filter(|(v, _)| !dropped.contains(v)).map(|(_, p)| p));
                    pending_by = pending_by.or_else(|| efaces.get(&u.edge).and_then(|fs| fs.iter().find(|x| del.contains(x)).copied()));
                    continue;
                }
                if let Some(p) = pending.take() {
                    let by = pending_by.take();
                    for line in closing(&mut shared, &mut asm, p, a, planes.get(&fi).copied(), by.and_then(outward), &wraps)? {
                        if let Some(d) = by {
                            closed_by.entry(line.edge).or_insert(d);
                        }
                        edges.push(line);
                    }
                }
                if !corner_moves(&asm, &moved, u) {
                    edges.push(u.clone());
                } else if matches!(asm.edges.get(u.edge as usize).map(|e| &e.curve), Some(Curve::Line { .. })) {
                    if a.dist(b) > q * 10.0 {
                        let stretched = shared.line(&mut asm, a, b)?;
                        asm.piece(stretched.edge, u.edge);
                        edges.push(stretched);
                    }
                } else {
                    return Err(fail(format!("face {fi} has a curved edge that would have to stretch")));
                }
            }
            // A gap stretch at the end closes back to the first edge.
            if let (Some(p), Some(first)) = (pending, edges.first()) {
                let (v0, _) = asm.ends(first).ok_or_else(|| fail("edge"))?;
                let a = asm.point(v0).ok_or_else(|| fail("vertex"))?;
                for line in closing(&mut shared, &mut asm, p, a, planes.get(&fi).copied(), pending_by.and_then(outward), &wraps)? {
                    if let Some(d) = pending_by {
                        closed_by.entry(line.edge).or_insert(d);
                    }
                    edges.push(line);
                }
            }
            if edges.is_empty() {
                return Err(fail(format!("face {fi} would lose its boundary")));
            }
            loops.push(edges);
        }
        if loops.is_empty() {
            return Err(fail(format!("face {fi} would lose its boundary")));
        }
        // The face keeps its plane: every moved corner lies on it.
        // A curved neighbour only lost whole loops: its parameter box stands.
        let domain = if kept_whole.contains(&fi) { f.domain } else { None };
        let (nurbs, surface) = if wraps.contains_key(&fi) {
            let seam = seams.get(&fi).cloned().ok_or_else(|| fail(format!("face {fi}: its seam is not known")))?;
            (None, seam)
        } else {
            (f.nurbs.clone(), f.surface.clone())
        };
        rebuilt.push((fi, AFace { loops, domain, nurbs, surface, ..f }));
        history.push((fi, crate::record::from_face(0, fi, Change::Modified)));
    }
    // A wrap face's kept circles go on its new frame's form: each is relabelled as the arc
    // between its ends through its middle, and the pcurves about it are made again.
    let rims: HashMap<u32, Wrap> = rebuilt
        .iter()
        .filter_map(|(fi, f)| Some((wraps.get(fi).copied()?, f)))
        .flat_map(|(w, f)| f.loops.iter().flatten().map(move |u| (u.edge, w)))
        .filter(|(e, _)| asm.edges.get(*e as usize).is_some_and(|e| e.source.is_some() && matches!(e.curve, Curve::Circle { .. })))
        .collect();
    for (e, w) in &rims {
        let edge = asm.edges.get(*e as usize).ok_or_else(|| fail("edge"))?;
        let [va, vb] = edge.vertices;
        let (a, b) = (asm.point(va).ok_or_else(|| fail("vertex"))?, asm.point(vb).ok_or_else(|| fail("vertex"))?);
        let [t0, t1] = edge.interval;
        let mid = solidcraft_geom::brep::eval::CurveEval::new(edge.nurbs.as_ref().unwrap_or(&edge.curve))
            .ok()
            .and_then(|ev| ev.point(0.5 * (t0 + t1)))
            .ok_or_else(|| fail("a kept circle that cannot be evaluated"))?;
        let centre = w.centre_at(arr(a));
        let arc = solidcore_ops::cylinder_circle::circle_arc_through(arr(a), arr(mid), arr(b), centre, w.axis, q * 10.0)
            .ok_or_else(|| fail("a kept circle with no side to go round"))?;
        if let Some(edge) = asm.edges.get_mut(*e as usize) {
            edge.curve = Curve::Circle { frame: Frame { origin: centre, x_axis: arc.x_axis, y_axis: arc.y_axis }, radius: arc.radius };
            edge.nurbs = None;
            edge.interval = [0.0, arc.sweep];
        }
    }
    for u in rebuilt
        .iter_mut()
        .flat_map(|(_, f)| f.loops.iter_mut().flatten())
        .chain(asm.faces.iter_mut().flatten().flat_map(|f| f.loops.iter_mut().flatten()))
    {
        if rims.contains_key(&u.edge) {
            u.given = None;
        }
    }
    for &fi in del {
        if let Some(slot) = asm.faces.get_mut(fi) {
            *slot = None;
        }
    }
    for (fi, face) in rebuilt {
        if let Some(slot) = asm.faces.get_mut(fi) {
            *slot = Some(face);
        }
    }
    let fallback = del.first().copied().unwrap_or(0);
    fill_on_planes(&mut asm, &planes, size * 1e-6, &mut history, &closed_by, fallback)?;
    history.sort_by_key(|(fi, _)| *fi);
    let finished = asm.finish().map_err(|e| fail(format!("the neighbouring faces do not close the gap ({e})")))?;
    crate::assemble::body_with_history(finished, &asm, body, history.into_iter().map(|(_, h)| h).collect())
}

fn on_gap(efaces: &HashMap<u32, Vec<usize>>, del: &BTreeSet<usize>, e: u32) -> bool {
    efaces.get(&e).is_some_and(|fs| fs.iter().any(|x| del.contains(x)))
}

/// The neighbours of the chosen faces, classified before the healer runs: planes extend across
/// the gap, a cylinder closes round over it (`wraps`), a curved neighbour only loses whole loops
/// (`kept_whole`). Any other curved neighbour is left out, and fillet removal takes it.
struct Gap {
    asm: Assembly,
    efaces: HashMap<u32, Vec<usize>>,
    vfaces: HashMap<u32, Vec<usize>>,
    neighbours: BTreeSet<usize>,
    planes: HashMap<usize, (Vec3, f64)>,
    kept_whole: HashSet<usize>,
    wraps: BTreeMap<usize, Wrap>,
    /// Curved neighbours at a corner the healer would refuse (a corner it cannot move shared with
    /// faces that are not all kept whole, or all planes or wraps).
    mixed: HashSet<usize>,
}

impl Gap {
    fn new(rec: &Brep, del: &BTreeSet<usize>, size: f64) -> Result<Gap> {
        let asm = Assembly::from_record(rec)?;
        // Edge and vertex → faces.
        let mut efaces: HashMap<u32, Vec<usize>> = HashMap::new();
        let mut vfaces: HashMap<u32, Vec<usize>> = HashMap::new();
        for (i, f) in asm.faces.iter().enumerate() {
            for u in f.iter().flat_map(|f| &f.loops).flatten() {
                let l = efaces.entry(u.edge).or_default();
                if !l.contains(&i) {
                    l.push(i);
                }
                let (a, b) = asm.ends(u).ok_or_else(|| fail("edge"))?;
                for v in [a, b] {
                    let l = vfaces.entry(v).or_default();
                    if !l.contains(&i) {
                        l.push(i);
                    }
                }
            }
        }
        let gap_edge = |e: u32| on_gap(&efaces, del, e);
        // Neighbours: faces sharing an edge with a deleted face.
        let mut neighbours: BTreeSet<usize> = BTreeSet::new();
        for fs in efaces.values() {
            if fs.iter().any(|f| del.contains(f)) {
                neighbours.extend(fs.iter().filter(|f| !del.contains(f)));
            }
        }
        if neighbours.is_empty() {
            return Err(fail("the faces don't border the rest of the body"));
        }
        let ruling = |u: &AUse, w: &Wrap, tol: f64| {
            asm.edges.get(u.edge as usize).is_some_and(|e| match &e.curve {
                Curve::Line { start, end } => w.is_ruling(arr(Vec3::from(*start)), arr(Vec3::from(*end)), tol),
                _ => false,
            })
        };
        // Whether edge `e` is the cylinder's circle at height `h`.
        let on_circle = |e: u32, w: &Wrap, h: f64, tol: f64| {
            asm.edges.get(e as usize).is_some_and(|edge| match &edge.curve {
                Curve::Circle { frame, radius } => w.is_circle_at(frame.origin, frame.x_axis, frame.y_axis, *radius, h, tol),
                _ => false,
            })
        };
        let wraps_round = |f: usize, w: &Wrap| {
            let tol = size * 1e-6;
            let Some(face) = asm.faces.get(f).and_then(Option::as_ref) else { return false };
            let mut ends: Option<(f64, f64)> = None;
            for l in &face.loops {
                let on: Vec<usize> = (0..l.len()).filter(|k| l.get(*k).is_some_and(|u| gap_edge(u.edge))).collect();
                match on[..] {
                    [] => continue,
                    [i, j] if j - i != 1 && !(i == 0 && j + 1 == l.len()) => {}
                    _ => return false,
                }
                let (Some(ui), Some(uj)) = (on.first().and_then(|k| l.get(*k)), on.get(1).and_then(|k| l.get(*k))) else { return false };
                if !(ruling(ui, w, tol) && ruling(uj, w, tol)) || ends.is_some() {
                    return false;
                }
                let Some((a, b)) = asm.ends(ui).and_then(|(a, b)| Some((asm.point(a)?, asm.point(b)?))) else { return false };
                let Some((r, s)) = asm.ends(uj).and_then(|(r, s)| Some((asm.point(r)?, asm.point(s)?))) else { return false };
                // One deleted plane borders both rulings, and the rulings end on the same circles.
                let borders =
                    |u: &AUse| -> BTreeSet<usize> { efaces.get(&u.edge).into_iter().flatten().copied().filter(|x| del.contains(x)).collect() };
                let shared: Vec<usize> = borders(ui).intersection(&borders(uj)).copied().collect();
                let [by] = shared[..] else { return false };
                if !matches!(surf_of(rec, by), Some(Surf::Plane { .. })) || !w.same_circle(arr(a), arr(s), tol) || !w.same_circle(arr(r), arr(b), tol)
                {
                    return false;
                }
                let (h1, h2) = (w.height(arr(a)), w.height(arr(b)));
                // Every other edge of the loop runs along one of those two circles (no tiny
                // edge or step between the rulings).
                if !l.iter().filter(|u| !gap_edge(u.edge)).all(|u| on_circle(u.edge, w, h1, tol) || on_circle(u.edge, w, h2, tol)) {
                    return false;
                }
                ends = Some((h1.min(h2), h1.max(h2)));
            }
            // The cylinder ends at those circles: none of its vertices lies past them (a flat
            // pocketed into the middle of a shaft, its end walls inside the solid, is not one).
            let Some((lo, hi)) = ends else { return false };
            face.loops.iter().flatten().all(|u| {
                asm.ends(u)
                    .is_some_and(|(a, b)| [a, b].iter().all(|v| asm.point(*v).is_some_and(|p| (lo - tol..=hi + tol).contains(&w.height(arr(p))))))
            })
        };
        let loses_whole_loops = |f: usize| {
            asm.faces
                .get(f)
                .and_then(Option::as_ref)
                .is_some_and(|face| face.loops.iter().all(|l| l.iter().all(|u| gap_edge(u.edge)) || !l.iter().any(|u| gap_edge(u.edge))))
        };
        let mut planes: HashMap<usize, (Vec3, f64)> = HashMap::new();
        let mut kept_whole: HashSet<usize> = HashSet::new();
        let mut wraps: BTreeMap<usize, Wrap> = BTreeMap::new();
        for &f in &neighbours {
            match surf_of(rec, f) {
                Some(Surf::Plane { n, d }) => {
                    planes.insert(f, (n, d));
                }
                Some(Surf::Cylinder { o, a, r, .. }) if wraps_round(f, &Wrap { origin: arr(o), axis: arr(a), radius: r }) => {
                    wraps.insert(f, Wrap { origin: arr(o), axis: arr(a), radius: r });
                }
                _ if loses_whole_loops(f) => {
                    kept_whole.insert(f);
                }
                _ => {}
            }
        }
        let mut mixed: HashSet<usize> = HashSet::new();
        for fs in vfaces.values().filter(|fs| fs.iter().any(|f| del.contains(f))) {
            let keep: Vec<usize> = fs.iter().copied().filter(|x| !del.contains(x)).collect();
            if keep.is_empty() || keep.len() >= 3 {
                continue;
            }
            let whole = keep.iter().all(|x| kept_whole.contains(x));
            let planar = keep.iter().all(|x| planes.contains_key(x) || wraps.contains_key(x));
            if !whole && !planar {
                mixed.extend(keep.iter().copied().filter(|x| kept_whole.contains(x) || wraps.contains_key(x)));
            }
        }
        Ok(Gap { asm, efaces, vfaces, neighbours, planes, kept_whole, wraps, mixed })
    }

    /// Whether the planar healer takes curved neighbour `f`: a cylinder that closes round over the
    /// gap, or one that only loses whole loops, unless it shares a corner the healer cannot move.
    fn takes(&self, f: usize) -> bool {
        (self.wraps.contains_key(&f) || self.kept_whole.contains(&f)) && !self.mixed.contains(&f)
    }
}

#[cfg(test)]
mod tests {
    use solidcore_record::Vec3;
    use solidcraft_geom::MeshGeom;

    use super::delete_faces;
    use crate::{BoolOp, HalfSpace, boolean, box_solid, chamfer, convex_polyhedron, cylinder, fillet};

    fn volume(b: &crate::Body) -> f64 {
        b.mesh(&crate::MeshTolerance::chord(1e-3)).unwrap().measure().volume
    }

    /// The faces of `b` with every triangle corner passing `keep`.
    fn faces_where(b: &crate::Body, keep: impl Fn(Vec3) -> bool) -> Vec<usize> {
        let m = b.mesh(&crate::MeshTolerance::chord(1e-3)).unwrap();
        let n = m.tri_face.iter().max().map(|x| *x as usize + 1).unwrap_or(0);
        (0..n)
            .filter(|f| {
                m.triangles.iter().zip(&m.tri_face).filter(|(_, x)| **x as usize == *f).all(|(t, _)| m.tri(t).unwrap().iter().all(|p| keep(*p)))
            })
            .collect()
    }

    #[test]
    fn issue1557_delete_closed_fillet_ring_keeps_the_floor() {
        let b = box_solid(Vec3::ZERO, Vec3::new(25.0, 25.0, 10.0)).unwrap();
        let picks = [Vec3::new(12.5, 0.0, 0.0), Vec3::new(25.0, 12.5, 0.0), Vec3::new(12.5, 25.0, 0.0), Vec3::new(0.0, 12.5, 0.0)];
        let rounded = fillet(&b, &picks, 2.0).unwrap();
        let record = crate::brep_record(&rounded).unwrap();
        let rounds: Vec<usize> =
            (0..record.faces.len()).filter(|i| !matches!(crate::offset::surf_of(&record, *i), Some(crate::offset::Surf::Plane { .. }))).collect();
        assert_eq!(rounds.len(), 4);
        let restored = delete_faces(&rounded, &rounds).unwrap();
        assert_eq!(restored.face_count(), 6);
        assert_eq!(restored.unmeshed_faces(), 0);
        assert!((volume(&restored) - 6250.0).abs() < 1e-6);
        assert_eq!(restored.classify_point(Vec3::new(0.1, 0.1, 0.1)).unwrap(), crate::PointClass::Inside);
    }

    #[test]
    fn delete_fillet_chamfer_hole_and_boss_faces() {
        let sharp = 24000.0;
        let b = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 20.0)).unwrap();
        // A fillet on a box edge: deleting it gives the sharp box back, exactly.
        let f = fillet(&b, &[Vec3::new(20.0, 30.0, 20.0)], 5.0).unwrap();
        assert!(volume(&f) < sharp - 1.0);
        let fi = faces_where(&f, |p| p.y > 25.0 - 1e-6 && p.z > 15.0 - 1e-6 && (p.y < 30.0 - 1e-9 || p.z < 20.0 - 1e-9));
        assert_eq!(fi.len(), 1, "{fi:?}");
        let back = delete_faces(&f, &fi).unwrap();
        assert!((volume(&back) - sharp).abs() < 1e-9 * sharp, "{}", volume(&back));
        assert_eq!(back.face_count(), 6);
        // A chamfer likewise.
        let c = chamfer(&b, &[Vec3::new(20.0, 30.0, 20.0)], 4.0).unwrap();
        let ci = faces_where(&c, |p| p.y > 26.0 - 1e-6 && p.z > 16.0 - 1e-6 && (p.y < 30.0 - 1e-9 || p.z < 20.0 - 1e-9));
        let back = delete_faces(&c, &ci).unwrap();
        assert!((volume(&back) - sharp).abs() < 1e-9 * sharp, "{}", volume(&back));
        // A through hole: deleting its wall fills it.
        let plate = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 10.0)).unwrap();
        let pin = cylinder(Vec3::new(20.0, 15.0, -1.0), Vec3::Z, 4.0, 12.0).unwrap();
        let holed = boolean(&plate, &pin, BoolOp::Cut).unwrap().unwrap();
        let wall = faces_where(&holed, |p| ((p.x - 20.0).hypot(p.y - 15.0) - 4.0).abs() < 1e-2);
        assert!(!wall.is_empty());
        let filled = delete_faces(&holed, &wall).unwrap();
        assert!((volume(&filled) - 12000.0).abs() < 1e-9 * 12000.0, "{}", volume(&filled));
        // A blind hole: its wall and floor.
        let drill = cylinder(Vec3::new(20.0, 15.0, 4.0), Vec3::Z, 3.0, 7.0).unwrap();
        let blind = boolean(&plate, &drill, BoolOp::Cut).unwrap().unwrap();
        let hole = faces_where(&blind, |p| (p.x - 20.0).hypot(p.y - 15.0) < 3.0 + 1e-3 && p.z > 4.0 - 1e-6);
        let filled = delete_faces(&blind, &hole).unwrap();
        assert!((volume(&filled) - 12000.0).abs() < 1e-9 * 12000.0, "{}", volume(&filled));
        // A boss on the plate: deleting its wall and top takes it off.
        let boss = cylinder(Vec3::new(10.0, 10.0, 10.0), Vec3::Z, 3.0, 5.0).unwrap();
        let bossed = boolean(&plate, &boss, BoolOp::Union).unwrap().unwrap();
        let bf = faces_where(&bossed, |p| p.z > 10.0 - 1e-9 && (p.x - 10.0).hypot(p.y - 10.0) < 3.0 + 1e-6);
        let off = delete_faces(&bossed, &bf).unwrap();
        assert!((volume(&off) - 12000.0).abs() < 1e-9 * 12000.0, "{}", volume(&off));
        // A curved neighbour is refused, naming both faces' kinds and the way out.
        let rounded = fillet(&b, &[Vec3::new(20.0, 30.0, 20.0)], 5.0).unwrap();
        let round = faces_where(&rounded, |p| p.y > 25.0 - 1e-6 && p.z > 15.0 - 1e-6 && (p.y < 30.0 - 1e-9 || p.z < 20.0 - 1e-9));
        let top = faces_where(&rounded, |p| (p.z - 20.0).abs() < 1e-9 && (p.y - 30.0).hypot(p.x - 20.0) > 5.0 + 1e-6);
        let top = top.first().copied().unwrap();
        let why = delete_faces(&rounded, &[top]).unwrap_err().to_string();
        assert!(why.contains(&format!("face {top} (a plane), borders face {}, a cylinder", round[0])), "{why}");
        assert!(why.contains("the timeline"), "{why}");

        // Errors name what is wrong.
        assert!(delete_faces(&plate, &[99]).is_err());
        assert!(delete_faces(&plate, &[0, 1, 2, 3, 4, 5]).is_err());
        // One face of a box: the sides cannot close it.
        let top = faces_where(&plate, |p| (p.z - 10.0).abs() < 1e-9);
        let refused = delete_faces(&plate, &top).unwrap_err().to_string();
        assert!(refused.contains("cannot close: no neighbouring plane extends across it"), "{refused}");
        // The refusal names the face deleted, not face 0.
        let bottom = faces_where(&plate, |p| p.z.abs() < 1e-9);
        assert_eq!(bottom.len(), 1);
        assert_ne!(bottom[0], 0);
        let refused = delete_faces(&plate, &bottom).unwrap_err().to_string();
        assert!(refused.contains(&format!("the gap left by face {} cannot close", bottom[0])), "{refused}");
    }

    /// A half-space `n · x <= d` with `n` made unit.
    fn half(n: Vec3, d: f64) -> crate::HalfSpace {
        let len = (n.x * n.x + n.y * n.y + n.z * n.z).sqrt();
        crate::HalfSpace { n: n / len, d: d / len }
    }

    /// #14257, #15775: two faces of a V notch or groove whose neighbours across the gap lie on
    /// one plane (the wall either side of the notch, the top either side of the groove) left an
    /// open shell. That plane now extends across the gap and the faces on it become one.
    #[test]
    fn deleting_a_v_notch_or_groove_extends_the_plane_across_it() {
        let block = box_solid(Vec3::ZERO, Vec3::new(40.0, 20.0, 10.0)).unwrap();
        // A V notch through the front wall: mouth x 15..25 at y = 0, apex at (20, 5).
        let wedge = crate::convex_polyhedron(&[
            half(Vec3::new(0.0, 0.0, -1.0), 1.0),
            half(Vec3::new(0.0, 0.0, 1.0), 11.0),
            half(Vec3::new(0.0, -1.0, 0.0), 1.0),
            half(Vec3::new(-1.0, 1.0, 0.0), -15.0),
            half(Vec3::new(1.0, 1.0, 0.0), 25.0),
        ])
        .unwrap();
        let notched = boolean(&block, &wedge, BoolOp::Cut).unwrap().unwrap();
        assert!((volume(&notched) - 7750.0).abs() < 1e-6);
        let sides = faces_where(&notched, |p| ((p.x - 20.0).abs() + p.y - 5.0).abs() < 1e-3);
        assert_eq!(sides.len(), 2, "{sides:?}");
        let back = delete_faces(&notched, &sides).unwrap();
        assert!((volume(&back) - 8000.0).abs() < 1e-6, "{}", volume(&back));
        assert_eq!(back.face_count(), 6);
        assert_eq!(back.unmeshed_faces(), 0);
        assert_eq!(back.classify_point(Vec3::new(20.0, 1.0, 5.0)).unwrap(), crate::PointClass::Inside);
        // A V groove along x across the top: mouth y 5..15 at z = 10, apex at z = 5.
        let cutter = crate::convex_polyhedron(&[
            half(Vec3::new(-1.0, 0.0, 0.0), 1.0),
            half(Vec3::new(1.0, 0.0, 0.0), 41.0),
            half(Vec3::new(0.0, 0.0, 1.0), 11.0),
            half(Vec3::new(0.0, -1.0, -1.0), -15.0),
            half(Vec3::new(0.0, 1.0, -1.0), 5.0),
        ])
        .unwrap();
        let grooved = boolean(&block, &cutter, BoolOp::Cut).unwrap().unwrap();
        assert!((volume(&grooved) - 7000.0).abs() < 1e-6);
        let walls = faces_where(&grooved, |p| (p.y + p.z - 15.0).abs() < 1e-3 || (p.y - p.z - 5.0).abs() < 1e-3);
        assert_eq!(walls.len(), 2, "{walls:?}");
        let back = delete_faces(&grooved, &walls).unwrap();
        assert!((volume(&back) - 8000.0).abs() < 1e-6, "{}", volume(&back));
        assert_eq!(back.face_count(), 6);
        assert_eq!(back.classify_point(Vec3::new(20.0, 10.0, 9.0)).unwrap(), crate::PointClass::Inside);
    }

    /// #16864 review: faces merged into the filled face keep no pcurves from their own surface
    /// frames. Two notched halves, the right one turned half a turn about y: their front faces
    /// lie on one plane, either side of the notch, with opposite u axes.
    #[test]
    fn filling_across_faces_on_two_surfaces_remakes_their_pcurves() {
        let wedge = crate::convex_polyhedron(&[
            half(Vec3::new(0.0, 0.0, -1.0), 1.0),
            half(Vec3::new(0.0, 0.0, 1.0), 11.0),
            half(Vec3::new(0.0, -1.0, 0.0), 1.0),
            half(Vec3::new(-1.0, 1.0, 0.0), -15.0),
            half(Vec3::new(1.0, 1.0, 0.0), 25.0),
        ])
        .unwrap();
        let left = box_solid(Vec3::ZERO, Vec3::new(20.0, 20.0, 10.0)).unwrap();
        let right = box_solid(Vec3::new(20.0, 0.0, 0.0), Vec3::new(40.0, 20.0, 10.0)).unwrap();
        let right = crate::transform(&right, Vec3::ZERO, Vec3::new(30.0, 10.0, 5.0), Vec3::new(0.0, 1.0, 0.0), std::f64::consts::PI).unwrap();
        let left = boolean(&left, &wedge, BoolOp::Cut).unwrap().unwrap();
        let right = boolean(&right, &wedge, BoolOp::Cut).unwrap().unwrap();
        let notched = boolean(&left, &right, BoolOp::Union).unwrap().unwrap();
        let fronts = faces_where(&notched, |p| p.y.abs() < 1e-9);
        let surfaces: Vec<_> = fronts.iter().map(|f| notched.record().faces[*f].surface.clone()).collect();
        assert_eq!(fronts.len(), 2, "{fronts:?}");
        assert_ne!(surfaces[0], surfaces[1], "the fronts must be on two surfaces for this test");
        let sides = faces_where(&notched, |p| ((p.x - 20.0).abs() + p.y - 5.0).abs() < 1e-3);
        assert_eq!(sides.len(), 2, "{sides:?}");
        let back = delete_faces(&notched, &sides).unwrap();
        assert!((volume(&back) - 8000.0).abs() < 1e-6, "{}", volume(&back));
        assert_eq!(faces_where(&back, |p| p.y.abs() < 1e-9).len(), 1);
        solidcore_ops::record_check::agreement(back.record()).unwrap();
    }

    /// #16864 review: a sheet's own free boundary is not part of the gap. An open box (no top)
    /// with a chamfered bottom edge: deleting the chamfer heals the corner and leaves the rim open.
    #[test]
    fn deleting_on_a_sheet_leaves_its_free_boundary_open() {
        let plate = box_solid(Vec3::ZERO, Vec3::new(40.0, 20.0, 10.0)).unwrap();
        let cham = chamfer(&plate, &[Vec3::new(20.0, 0.0, 0.0)], 2.0).unwrap();
        let top = faces_where(&cham, |p| (p.z - 10.0).abs() < 1e-9);
        assert_eq!(top.len(), 1);
        let open_box = crate::remove_faces(&cham, &top).unwrap();
        let bevel = faces_where(&open_box, |p| (p.y + p.z - 2.0).abs() < 1e-3);
        assert_eq!(bevel.len(), 1, "{bevel:?}");
        let healed = delete_faces(&open_box, &bevel).unwrap();
        assert_eq!(healed.face_count(), 5);
        assert!(faces_where(&healed, |p| (p.z - 10.0).abs() < 1e-9).is_empty());
        assert_eq!(faces_where(&healed, |p| p.y.abs() < 1e-9).len(), 1);
    }

    /// #16864 review: two V grooves either side of one strip of the top. The first fill merges
    /// the strip into a face; the second must find that face, not the strip's old slot.
    #[test]
    fn two_grooves_beside_one_strip_both_fill() {
        let block = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 10.0)).unwrap();
        let groove = |left: f64, right: f64| {
            crate::convex_polyhedron(&[
                half(Vec3::new(-1.0, 0.0, 0.0), 1.0),
                half(Vec3::new(1.0, 0.0, 0.0), 41.0),
                half(Vec3::new(0.0, 0.0, 1.0), 11.0),
                half(Vec3::new(0.0, -1.0, -1.0), -left),
                half(Vec3::new(0.0, 1.0, -1.0), right),
            ])
            .unwrap()
        };
        let one = boolean(&block, &groove(14.0, 2.0), BoolOp::Cut).unwrap().unwrap();
        let two = boolean(&one, &groove(28.0, 16.0), BoolOp::Cut).unwrap().unwrap();
        assert!((volume(&two) - 10720.0).abs() < 1e-6, "{}", volume(&two));
        // Each wall by its own plane: a top strip has every corner on one wall plane or another.
        let walls: Vec<usize> = [(1.0, 14.0), (-1.0, 2.0), (1.0, 28.0), (-1.0, 16.0)]
            .iter()
            .flat_map(|(s, d)| faces_where(&two, |p| (p.y + s * p.z - d).abs() < 1e-3))
            .collect();
        assert_eq!(walls.len(), 4, "{walls:?}");
        let back = delete_faces(&two, &walls).unwrap();
        assert!((volume(&back) - 12000.0).abs() < 1e-6, "{}", volume(&back));
        assert_eq!(back.face_count(), 6);
        assert_eq!(back.classify_point(Vec3::new(20.0, 8.0, 9.0)).unwrap(), crate::PointClass::Inside);
        assert_eq!(back.classify_point(Vec3::new(20.0, 22.0, 9.0)).unwrap(), crate::PointClass::Inside);
    }

    /// A round whose end lies on a ridge (T-219): a wedge, its top sloping down from the ridge
    /// x = 0, z = 26 to x = -37, with a rib on its wall x = 0 (x 0..20, y -22.5..-17.5, up to the
    /// ridge's height), the inside corner between the wall and the rib rounded r2. The round's
    /// top end on the wall, (0, -24.5, 26), lies on the rib's top, the wall and the wedge's
    /// slope, three planes through the ridge: it slides along the ridge to the sharp corner at
    /// y = -22.5 with the slope's edges. It stayed, and the rib's top came back with an edge
    /// along the ridge out to -24.5 and back (SmFace test #34).
    ///
    /// Closed form: the wedge 37 x 26 / 2 over y -40..10 (24050) and the rib 20 x 5 x 26 (2600).
    #[test]
    fn delete_a_round_ending_on_a_ridge() {
        let slope = Vec3::new(-26.0, 0.0, 37.0);
        let wedge = convex_polyhedron(&[
            HalfSpace { n: Vec3::new(1.0, 0.0, 0.0), d: 0.0 },
            HalfSpace { n: Vec3::new(0.0, 0.0, -1.0), d: 0.0 },
            HalfSpace { n: Vec3::new(0.0, -1.0, 0.0), d: 40.0 },
            HalfSpace { n: Vec3::new(0.0, 1.0, 0.0), d: 10.0 },
            HalfSpace { n: slope, d: 37.0 * 26.0 },
        ])
        .unwrap();
        let rib = box_solid(Vec3::new(0.0, -22.5, 0.0), Vec3::new(20.0, -17.5, 26.0)).unwrap();
        let ribbed = boolean(&wedge, &rib, BoolOp::Union).unwrap().unwrap();
        let rounded = fillet(&ribbed, &[Vec3::new(0.0, -22.5, 13.0)], 2.0).unwrap();
        let round = faces_where(&rounded, |p| ((p.x - 2.0).hypot(p.y + 24.5) - 2.0).abs() < 1e-2);
        assert_eq!(round.len(), 1, "{round:?}");
        let back = delete_faces(&rounded, &round).unwrap();
        assert!(crate::failing_check(&back).unwrap().is_none(), "{:?}", crate::failing_check(&back));
        let want = 37.0 * 26.0 / 2.0 * 50.0 + 20.0 * 5.0 * 26.0;
        assert!((volume(&back) - want).abs() < 1e-9 * want, "{} against {want}", volume(&back));
    }

    /// A round beside a curved neighbour goes to fillet removal: the sharp boss comes back.
    #[test]
    fn deleting_a_round_beside_a_cylinder_gives_the_sharp_boss() {
        let plate = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 5.0)).unwrap();
        let boss = cylinder(Vec3::new(20.0, 15.0, 5.0), Vec3::Z, 5.0, 8.0).unwrap();
        let bossed = boolean(&plate, &boss, BoolOp::Union).unwrap().unwrap();
        let sharp = 6000.0 + 200.0 * std::f64::consts::PI;
        let round_at = |b: &crate::Body| {
            let record = crate::brep_record(b).unwrap();
            (0..record.faces.len())
                .filter(|i| {
                    !matches!(crate::offset::surf_of(&record, *i), Some(crate::offset::Surf::Plane { .. } | crate::offset::Surf::Cylinder { .. }))
                })
                .collect::<Vec<usize>>()
        };
        for edge in [Vec3::new(25.0, 15.0, 5.0), Vec3::new(25.0, 15.0, 13.0)] {
            let rounded = fillet(&bossed, &[edge], 1.0).unwrap();
            let round = round_at(&rounded);
            assert_eq!(round.len(), 1, "{edge:?}: {round:?}");
            let back = delete_faces(&rounded, &round).unwrap();
            // Exact, not the meshed volume: a 1e-3 chord loses ~0.06 on the Ø10 boss.
            let v = back.volume_at(1e-3).unwrap();
            assert!((v - sharp).abs() < 1e-6 * sharp, "{edge:?}: {v}");
            assert_eq!(back.face_count(), 8, "{edge:?}");
            assert_eq!(back.unmeshed_faces(), 0);
        }
    }

    #[test]
    fn deleting_a_round_on_a_small_boss_gives_the_sharp_boss() {
        let plate = box_solid(Vec3::ZERO, Vec3::new(0.4, 0.3, 0.05)).unwrap();
        let boss = cylinder(Vec3::new(0.2, 0.15, 0.05), Vec3::Z, 0.05, 0.08).unwrap();
        let bossed = boolean(&plate, &boss, BoolOp::Union).unwrap().unwrap();
        let sharp = 0.006 + 0.0002 * std::f64::consts::PI;
        let rounded = fillet(&bossed, &[Vec3::new(0.25, 0.15, 0.05)], 0.01).unwrap();
        let record = crate::brep_record(&rounded).unwrap();
        let round: Vec<usize> = (0..record.faces.len())
            .filter(|i| {
                !matches!(crate::offset::surf_of(&record, *i), Some(crate::offset::Surf::Plane { .. } | crate::offset::Surf::Cylinder { .. }))
            })
            .collect();
        assert_eq!(round.len(), 1, "{round:?}");
        let back = delete_faces(&rounded, &round).unwrap();
        let v = back.volume_at(1e-9).unwrap();
        assert!((v - sharp).abs() < 1e-6 * sharp, "{v}");
        // The plate's faces 2·(0.4·0.3) + 2·(0.4·0.05) + 2·(0.3·0.05) = 0.31, and the boss's
        // lateral 2π·0.05·0.08 = 0.008π.
        let area = crate::measure(&back).unwrap().area;
        let whole_area = 0.31 + 0.008 * std::f64::consts::PI;
        assert!((area - whole_area).abs() < 1e-6 * whole_area, "{area} vs {whole_area}");
        assert_eq!(back.face_count(), 8);
    }

    /// A hole's chamfer (a cone between the top and the wall) is removed: the plain hole returns.
    #[test]
    fn deleting_a_hole_chamfer_gives_the_plain_hole() {
        let plate = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 5.0)).unwrap();
        let pin = cylinder(Vec3::new(20.0, 15.0, -1.0), Vec3::Z, 3.0, 7.0).unwrap();
        let holed = boolean(&plate, &pin, BoolOp::Cut).unwrap().unwrap();
        let plain = holed.volume_at(1e-3).unwrap();
        let chamfered = chamfer(&holed, &[Vec3::new(23.0, 15.0, 5.0)], 1.0).unwrap();
        let record = crate::brep_record(&chamfered).unwrap();
        let cone: Vec<usize> =
            (0..record.faces.len()).filter(|i| matches!(crate::offset::surf_of(&record, *i), Some(crate::offset::Surf::Cone { .. }))).collect();
        assert_eq!(cone.len(), 1, "{cone:?}");
        let back = delete_faces(&chamfered, &cone).unwrap();
        let v = back.volume_at(1e-3).unwrap();
        assert!((v - plain).abs() < 1e-6 * plain, "{v} vs {plain}");
        assert_eq!(back.face_count(), holed.face_count());
        assert_eq!(back.unmeshed_faces(), 0);
    }

    /// A cross hole's wall in a rod gives the whole rod back, 4000π.
    #[test]
    fn deleting_a_cross_hole_wall_gives_the_whole_rod() {
        let rod = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 40.0).unwrap();
        let pin = cylinder(Vec3::new(-11.0, 0.0, 20.0), Vec3::X, 2.0, 22.0).unwrap();
        let holed = boolean(&rod, &pin, BoolOp::Cut).unwrap().unwrap();
        let wall = faces_where(&holed, |p| (p.y.hypot(p.z - 20.0) - 2.0).abs() < 1e-2);
        assert!(!wall.is_empty());
        let whole = 4000.0 * std::f64::consts::PI;
        let back = delete_faces(&holed, &wall).unwrap();
        let v = back.volume_at(1e-3).unwrap();
        assert!((v - whole).abs() < 1e-6 * whole, "{v} vs {whole}");
        assert_eq!(back.face_count(), 3);
    }

    /// A flat on a shaft gives the round shaft back, 4000π.
    #[test]
    fn deleting_a_flat_on_a_shaft_gives_the_round_shaft() {
        let shaft = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 40.0).unwrap();
        let cut = box_solid(Vec3::new(8.0, -11.0, -1.0), Vec3::new(13.0, 11.0, 41.0)).unwrap();
        let flatted = boolean(&shaft, &cut, BoolOp::Cut).unwrap().unwrap();
        let flat = faces_where(&flatted, |p| (p.x - 8.0).abs() < 1e-6);
        assert_eq!(flat.len(), 1, "{flat:?}");
        let whole = 4000.0 * std::f64::consts::PI;
        let back = delete_faces(&flatted, &flat).unwrap();
        let v = back.volume_at(1e-3).unwrap();
        assert!((v - whole).abs() < 1e-6 * whole, "{v} vs {whole}");
        // Lateral 2π·10·40 and two caps 2π·10²: 800π + 200π.
        let area = crate::measure(&back).unwrap().area;
        let whole_area = 1000.0 * std::f64::consts::PI;
        assert!((area - whole_area).abs() < 1e-6 * whole_area, "{area} vs {whole_area}");
        assert_eq!(back.face_count(), 3);
        sound(&back);
    }

    /// A flat on a boss gives the round boss back, 6000 + 200π (the plate's top closes over its foot).
    #[test]
    fn deleting_a_flat_on_a_boss_gives_the_round_boss() {
        let plate = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 5.0)).unwrap();
        let boss = cylinder(Vec3::new(20.0, 15.0, 5.0), Vec3::Z, 5.0, 8.0).unwrap();
        let bossed = boolean(&plate, &boss, BoolOp::Union).unwrap().unwrap();
        let cut = box_solid(Vec3::new(23.0, 9.0, 5.0), Vec3::new(26.0, 21.0, 14.0)).unwrap();
        let flatted = boolean(&bossed, &cut, BoolOp::Cut).unwrap().unwrap();
        let flat = faces_where(&flatted, |p| (p.x - 23.0).abs() < 1e-6);
        assert_eq!(flat.len(), 1, "{flat:?}");
        let whole = 6000.0 + 200.0 * std::f64::consts::PI;
        let back = delete_faces(&flatted, &flat).unwrap();
        let v = back.volume_at(1e-3).unwrap();
        assert!((v - whole).abs() < 1e-6 * whole, "{v} vs {whole}");
        // The plate's faces 2·(40·30) + 2·(40·5) + 2·(30·5) = 3100 (its top keeps the boss's
        // footprint as the boss's top), and the boss's lateral 2π·5·8 = 80π.
        let area = crate::measure(&back).unwrap().area;
        let whole_area = 3100.0 + 80.0 * std::f64::consts::PI;
        assert!((area - whole_area).abs() < 1e-6 * whole_area, "{area} vs {whole_area}");
        assert_eq!(back.face_count(), 8);
        assert_eq!(back.unmeshed_faces(), 0);
        sound(&back);
    }

    /// A cross hole's rim chamfer on a rod is refused: it lies between two cylinders, and its curve is no circle.
    #[test]
    fn deleting_a_cross_hole_chamfer_is_refused() {
        let rod = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 40.0).unwrap();
        let pin = cylinder(Vec3::new(-11.0, 0.0, 20.0), Vec3::X, 2.0, 22.0).unwrap();
        let holed = boolean(&rod, &pin, BoolOp::Cut).unwrap().unwrap();
        let x = (100.0f64 - 4.0).sqrt();
        let chamfered = chamfer(&holed, &[Vec3::new(x, 2.0, 20.0)], 0.5).unwrap();
        let record = crate::brep_record(&chamfered).unwrap();
        let new: Vec<usize> = (0..chamfered.face_count())
            .filter(|i| {
                !matches!(crate::offset::surf_of(&record, *i), Some(crate::offset::Surf::Plane { .. } | crate::offset::Surf::Cylinder { .. }))
            })
            .collect();
        assert_eq!(new.len(), 1, "{new:?}");
        assert!(delete_faces(&chamfered, &new).is_err());
    }

    /// The record's pcurves agree with its edges and the body checks clean.
    fn sound(b: &crate::Body) {
        let record = crate::brep_record(b).unwrap();
        solidcore_ops::record_check::agreement(&record).unwrap();
        let problems = b.validity();
        assert!(problems.is_empty(), "{problems:?}");
    }

    /// A flat past the axis (x = −3 on a shaft Ø20) gives the round shaft back, 4000π.
    #[test]
    fn deleting_a_flat_past_the_axis_gives_the_round_shaft() {
        let shaft = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 40.0).unwrap();
        let cut = box_solid(Vec3::new(-3.0, -11.0, -1.0), Vec3::new(11.0, 11.0, 41.0)).unwrap();
        let flatted = boolean(&shaft, &cut, BoolOp::Cut).unwrap().unwrap();
        let flat = faces_where(&flatted, |p| (p.x + 3.0).abs() < 1e-6);
        assert_eq!(flat.len(), 1, "{flat:?}");
        let whole = 4000.0 * std::f64::consts::PI;
        let back = delete_faces(&flatted, &flat).unwrap();
        let v = back.volume_at(1e-3).unwrap();
        assert!((v - whole).abs() < 1e-6 * whole, "{v} vs {whole}");
        // The same round shaft as the flat on it: 800π lateral + 200π caps.
        let area = crate::measure(&back).unwrap().area;
        let whole_area = 1000.0 * std::f64::consts::PI;
        assert!((area - whole_area).abs() < 1e-6 * whole_area, "{area} vs {whole_area}");
        assert_eq!(back.face_count(), 3);
        sound(&back);
    }

    /// A pocket flat (z 10 to 30, with end walls) alone is refused: its end walls would close inside the solid.
    #[test]
    fn deleting_a_pocket_flat_alone_is_refused() {
        let shaft = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 40.0).unwrap();
        let cut = box_solid(Vec3::new(8.0, -11.0, 10.0), Vec3::new(11.0, 11.0, 30.0)).unwrap();
        let pocketed = boolean(&shaft, &cut, BoolOp::Cut).unwrap().unwrap();
        let flat = faces_where(&pocketed, |p| (p.x - 8.0).abs() < 1e-6);
        assert_eq!(flat.len(), 1, "{flat:?}");
        assert!(delete_faces(&pocketed, &flat).is_err());
    }

    /// A flat away from the cylinder's seam (x = −8) is refused cleanly for now (closing it round is backlog).
    #[test]
    fn deleting_a_flat_away_from_the_seam_is_refused() {
        let shaft = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 40.0).unwrap();
        let cut = box_solid(Vec3::new(-11.0, -11.0, -1.0), Vec3::new(-8.0, 11.0, 41.0)).unwrap();
        let flatted = boolean(&shaft, &cut, BoolOp::Cut).unwrap().unwrap();
        let flat = faces_where(&flatted, |p| (p.x + 8.0).abs() < 1e-6);
        assert_eq!(flat.len(), 1, "{flat:?}");
        assert!(delete_faces(&flatted, &flat).is_err());
    }

    /// The faces a round or chamfer on the x = 8 shaft flat adds: not the lateral (radius 10), the caps or the flat.
    fn shaft_rims(b: &crate::Body) -> Vec<usize> {
        let record = crate::brep_record(b).unwrap();
        (0..record.faces.len())
            .filter(|i| match crate::offset::surf_of(&record, *i) {
                Some(crate::offset::Surf::Plane { n, .. }) => n.x.abs() < 1.0 - 1e-9 && n.z.abs() < 1.0 - 1e-9,
                Some(crate::offset::Surf::Cylinder { r, .. }) => (r - 10.0).abs() > 1e-9,
                _ => true,
            })
            .collect()
    }

    /// `delete_faces` gives fillet removal's result, or its curved-neighbour refusal, never a healer error.
    fn is_fillet_removal(b: &crate::Body, faces: &[usize]) {
        let direct = crate::offset_backend::delete_fillet_faces(b, faces).ok();
        match (delete_faces(b, faces), direct) {
            (Ok(back), Some(direct)) => {
                let (v, w) = (back.volume_at(1e-3).unwrap(), direct.volume_at(1e-3).unwrap());
                assert!((v - w).abs() < 1e-6 * w.abs().max(1.0), "{v} vs {w}");
                assert_eq!(back.face_count(), direct.face_count());
            }
            // Only the fillet-removal dispatch gives the curved-neighbour refusal; the healer's errors never do.
            (Err(e), None) => {
                let e = e.to_string();
                assert!(e.contains("borders face") && e.contains("in the timeline"), "{e}");
            }
            (got, direct) => panic!("{:?} vs fillet removal {}", got.map(|b| b.face_count()), direct.is_some()),
        }
    }

    /// Two rounds on the shaft flat, both picked: no one deleted plane borders both rulings, so the wrap is not taken.
    #[test]
    fn two_rounds_on_a_shaft_flat_go_to_fillet_removal() {
        let shaft = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 40.0).unwrap();
        let cut = box_solid(Vec3::new(8.0, -11.0, -1.0), Vec3::new(13.0, 11.0, 41.0)).unwrap();
        let flatted = boolean(&shaft, &cut, BoolOp::Cut).unwrap().unwrap();
        let rounded = fillet(&flatted, &[Vec3::new(8.0, 6.0, 20.0), Vec3::new(8.0, -6.0, 20.0)], 1.0).unwrap();
        let rounds = shaft_rims(&rounded);
        assert_eq!(rounds.len(), 2, "{rounds:?}");
        is_fillet_removal(&rounded, &rounds);
    }

    /// Two chamfers on the shaft flat, both picked: each borders one ruling only, so the wrap is not taken.
    #[test]
    fn two_chamfers_on_a_shaft_flat_go_to_fillet_removal() {
        let shaft = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 40.0).unwrap();
        let cut = box_solid(Vec3::new(8.0, -11.0, -1.0), Vec3::new(13.0, 11.0, 41.0)).unwrap();
        let flatted = boolean(&shaft, &cut, BoolOp::Cut).unwrap().unwrap();
        let chamfered = chamfer(&flatted, &[Vec3::new(8.0, 6.0, 20.0), Vec3::new(8.0, -6.0, 20.0)], 0.5).unwrap();
        let chamfers = shaft_rims(&chamfered);
        assert_eq!(chamfers.len(), 2, "{chamfers:?}");
        is_fillet_removal(&chamfered, &chamfers);
    }
}
