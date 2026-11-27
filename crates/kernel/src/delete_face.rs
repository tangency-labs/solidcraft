//! Delete Face with healing extends the neighbours of the selected faces to close the gap.
//! SolidCore closes a ring groove across both circular rims and trims intersecting sphere
//! and plane extensions to their common circle.
//!
//! For a planar gap the body is rebuilt from its own schema 4 record. Faces away from the gap
//! are kept as they are. Each neighbouring face keeps its plane and its edges outside the gap.
//! A loop that ran entirely along deleted faces is dropped for a hole's rim or a boss's
//! footprint, or restored at the moved corners for a face enclosed by rounds. A corner on the
//! gap moves to where its planes meet the neighbours across the gap (a fillet's tangent points
//! go to the sharp corner), and the stretch of loop that bordered deleted faces closes with a
//! straight edge between the moved corners. A corner left on one neighbour only, with nowhere
//! to move (the apex of a V notch or groove), drops out of that edge. Where the gap is left
//! between neighbours on one plane, that plane extends across it: the region is filled and the
//! faces on the plane become one. If the planes cannot close the gap, a chain of curved fillet
//! faces can also be taken off together where the kernel can restore their sharp edges.

use solidcraft_geom::prelude::*;
use std::collections::{BTreeSet, HashMap, HashSet};

use solidcore_ops::cap::{is_face_on_plane, is_point_on_plane};
use solidcore_record::Vec3;
use solidcore_record::brep::Curve;
use solidcraft_geom::Aabb3;

use crate::assemble::{AFace, AUse, Assembly};
use crate::body::Body;
use crate::history::{Change, FaceProv};
use crate::offset::{Surf, surf_of};
use crate::{KernelError, Result, guard};

fn fail(m: impl Into<String>) -> KernelError {
    KernelError::Invalid(format!("Delete Face: {}", m.into()))
}

/// Extend the supports of a recognized feature. Carry the original face and edge names into the repaired body.
fn heal_from_extensions(body: &Body, selected: &BTreeSet<usize>, neighbours: &BTreeSet<usize>, round_junction: bool) -> Result<Option<Body>> {
    let Some(form) = body.ops_body() else { return Ok(None) };
    let (operand, order) = &*form;
    let picks: Vec<_> = selected.iter().copied().collect();
    drop(solidcore_ops::session::take_failure_detail());
    let heal = || -> solidcore_ops::Result<_> {
        let removed = if round_junction {
            solidcore_ops::trim::remove_round_junction(operand, order, &picks)?
        } else {
            solidcore_ops::trim::remove_feature_with_crossing_patches(operand, order, &picks)?
        };
        let Some((healed, history)) = removed else {
            return Ok(None);
        };
        Ok(Some((healed.to_record()?.0, history)))
    };
    let result = match solidcraft_smlib::stop::token() {
        Some(token) => solidcore_ops::cancel::within(&token, heal),
        None => heal(),
    }
    .map_err(|e| match e {
        solidcore_ops::Error::Internal(message) => KernelError::Internal(format!("Delete Face: {message}")),
        other => {
            let reason = solidcore_ops::session::take_failure_detail().unwrap_or_else(|| other.to_string());
            KernelError::Failed(format!("Delete Face: {reason}"))
        }
    })?;
    let Some((record, history)) = result else { return Ok(None) };
    let history = crate::record::from_ops_edit(&record, history, neighbours);
    Ok(Some(Body::from_record(record)?.with_history_counts(history, &crate::record::counts(&[body]))?))
}

/// A ball between three round strips is tangent along each whole boundary when
/// its centre is on each cylinder axis and its radius and outward side agree.
fn ball_end(rec: &solidcore_record::brep::Brep, face: usize, adjacent: &impl Fn(usize) -> BTreeSet<usize>) -> bool {
    let Some(Surf::Sphere { c, r, convex }) = surf_of(rec, face) else { return false };
    let around = adjacent(face);
    around.len() == 3
        && around.into_iter().all(|side| {
            matches!(surf_of(rec, side), Some(Surf::Cylinder { o, a, r: sr, convex: outward })
            if outward == convex && (sr - r).abs() <= rec.tolerance && (c - o).cross(a).len() <= rec.tolerance)
        })
}

/// Removing a straight round growing into rolling-ball corners also removes its end
/// patches. The other strips at each corner remain supports and extend to meet.
fn round_junction_faces(
    rec: &solidcore_record::brep::Brep,
    selected: &BTreeSet<usize>,
    adjacent: &impl Fn(usize) -> BTreeSet<usize>,
) -> Option<BTreeSet<usize>> {
    let mut removed = selected.clone();
    let mut has_corner = false;
    for &face in selected {
        if ball_end(rec, face, adjacent) {
            has_corner = true;
            continue;
        }
        let Some(Surf::Cylinder { o, a, r, .. }) = surf_of(rec, face) else { return None };
        let around = adjacent(face);
        if around.len() != 4 {
            return None;
        }
        let supports = around
            .iter()
            .filter(|&&f| {
                matches!(surf_of(rec, f), Some(Surf::Plane { n, d })
            if n.dot(a).abs() * r <= rec.tolerance && ((n.dot(o) - d).abs() - r).abs() <= rec.tolerance)
            })
            .count();
        if supports != 2 {
            return None;
        }
        for end in around {
            if ball_end(rec, end, adjacent) {
                removed.insert(end);
                has_corner = true;
            } else if !matches!(surf_of(rec, end), Some(Surf::Plane { .. })) {
                return None;
            }
        }
    }
    has_corner.then_some(removed)
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

/// Whether the use's edge has a corner that moves.
fn corner_moves(asm: &Assembly, moved: &HashMap<u32, Vec3>, u: &AUse) -> bool {
    asm.ends(u).is_some_and(|(a, b)| moved.contains_key(&a) || moved.contains_key(&b))
}

/// Where vertex `v` is after the corners move.
fn position(asm: &Assembly, moved: &HashMap<u32, Vec3>, v: u32) -> Result<Vec3> {
    moved.get(&v).copied().or_else(|| asm.point(v)).ok_or_else(|| fail("vertex"))
}

fn arr(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
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

/// Remove a groove between two turned faces, with its history over the original body. The
/// kernel decides whether the rims bound a band and records the identity of the surfaces it joins.
fn heal_ring_groove(body: &Body, selected: &BTreeSet<usize>, neighbours: &BTreeSet<usize>) -> Result<Option<Body>> {
    let Some(form) = body.ops_body() else { return Ok(None) };
    let (operand, order) = &*form;
    let picks: Vec<_> = selected.iter().copied().collect();
    let Some((healed, history)) = solidcore_ops::trim::remove_ring_groove(operand, order, &picks).map_err(|e| fail(e.to_string()))? else {
        return Ok(None);
    };
    let (record, _) = healed.to_record().map_err(|e| fail(e.to_string()))?;
    let history = crate::record::from_ops_edit(&record, history, neighbours);
    Ok(Some(Body::from_record(record)?.with_history_counts(history, &crate::record::counts(&[body]))?))
}

/// Delete faces (indices in the body's face order, as in its mesh's `tri_face`) and heal the
/// gap by extending the neighbours. The result's history is over `[body]`.
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
    let planar = guard("delete face", || {
        let mut asm = Assembly::from_record(&rec)?;
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
        let gap_edge = |e: u32| efaces.get(&e).is_some_and(|fs| fs.iter().any(|x| del.contains(x)));
        // Faces sharing an edge with the selection.
        let mut neighbours: BTreeSet<usize> = BTreeSet::new();
        for fs in efaces.values() {
            if fs.iter().any(|f| del.contains(f)) {
                neighbours.extend(fs.iter().filter(|f| !del.contains(f)));
            }
        }
        if neighbours.is_empty() {
            return Err(fail("the faces don't border the rest of the body"));
        }
        if neighbours.len() == 2
            && neighbours.iter().all(|&f| !matches!(surf_of(&rec, f), Some(Surf::Plane { .. })))
            && let Some(groove) = heal_ring_groove(body, &del, &neighbours).transpose()
        {
            // Carry the groove result, including any refusal, past the planar fallback.
            return Ok(groove);
        }
        let adjacent = |face| {
            crate::offset::face_edges(&rec, face)
                .into_iter()
                .filter_map(|edge| efaces.get(&edge))
                .flatten()
                .copied()
                .filter(|&f| f != face)
                .collect::<BTreeSet<_>>()
        };
        if let Some(removed) = round_junction_faces(&rec, &del, &adjacent) {
            let supports = removed.iter().flat_map(|&face| adjacent(face)).filter(|f| !removed.contains(f)).collect();
            if let Some(healed) = heal_from_extensions(body, &removed, &supports, true).transpose() {
                return Ok(healed);
            }
        }
        let mut planes: HashMap<usize, (Vec3, f64)> = HashMap::new();
        for &f in &neighbours {
            match surf_of(&rec, f) {
                Some(Surf::Plane { n, d }) => {
                    planes.insert(f, (n, d));
                }
                other => {
                    if matches!(other, Some(Surf::Sphere { .. }))
                        && neighbours.len() == 2
                        && neighbours.iter().any(|&g| matches!(surf_of(&rec, g), Some(Surf::Plane { .. })))
                        && let Some(healed) = heal_from_extensions(body, &del, &neighbours, false).transpose()
                    {
                        return Ok(healed);
                    }
                    let by = efaces
                        .values()
                        .find(|fs| fs.contains(&f) && fs.iter().any(|x| del.contains(x)))
                        .and_then(|fs| fs.iter().find(|x| del.contains(x)).copied())
                        .unwrap_or(0);
                    let reason = if matches!(surf_of(&rec, by), Some(Surf::Plane { .. })) {
                        "healing this selection requires planar neighbours to close the gap. If a feature created the selected face, edit that feature in the timeline".to_owned()
                    } else {
                        format!(
                            "to remove a tangent chain of rounds, select all the round faces. Include face {f} if it should go too, or take off the feature that made it in the timeline"
                        )
                    };
                    return Err(fail(format!("face {by} ({}), borders face {f}, {}: {reason}", surface_kind(&rec, by), surface_kind(&rec, f))));
                }
            }
        }
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
            planes.get(&f).copied().or_else(|| match surf_of(&rec, f) {
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
                let pinned = kp.iter().enumerate().any(|(i, a)| {
                    kp.iter().enumerate().skip(i + 1).any(|(j, b)| kp.iter().skip(j + 1).any(|c| a.0.dot(b.0.cross(c.0)).abs() >= 1e-9))
                });
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
        let mut shared = Shared { q, vertices: HashMap::new(), lines: HashMap::new() };
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
                // Untouched, unless one of its corners moves: a plane meeting the gap only at a
                // corner that slides along its edge (a ridge) follows it with its straight edges
                // when every moved corner stays on it; any other face cannot.
                if f.loops.iter().flatten().any(|u| corner_moves(&asm, &moved, u)) {
                    let follows = match surf_of(&rec, fi) {
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
                if gaps == l.len() {
                    // A hole or boss footprint disappears when its walls are removed. A face
                    // surrounded by rounds does not: its whole boundary moves back to the
                    // intersections of the remaining planes (#1557), its restored edges shared
                    // with the adjacent rebuilt faces.
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
                // The moved corners along a gap stretch.
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
                        for (x, y) in close(p, a, q) {
                            let line = shared.line(&mut asm, x, y)?;
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
                    for (x, y) in close(p, a, q) {
                        let line = shared.line(&mut asm, x, y)?;
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
            rebuilt.push((fi, AFace { loops, domain: None, ..f }));
            history.push((fi, crate::record::from_face(0, fi, Change::Modified)));
        }
        for &fi in &del {
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
        let finished = asm.finish().map_err(|e| {
            log::debug!("delete_faces assembly finish failed: {e}");
            fail("the neighbouring faces do not close the gap")
        })?;
        crate::assemble::body_with_history(finished, &asm, body, history.into_iter().map(|(_, h)| h).collect()).map(Ok)
    });
    match planar {
        Err(e @ KernelError::Invalid(_)) if del.len() >= 2 && del.iter().all(|&f| !matches!(surf_of(&rec, f), Some(Surf::Plane { .. }))) => {
            delete_fillet_chain(body, &del)?.ok_or(e)
        }
        outcome => outcome?,
    }
}

/// Retain the identity of every surviving face, including supports that ran on to the sharp
/// edges. An edge that used to bound the removed chain is now the meeting of its supports.
fn delete_fillet_chain(body: &Body, selected: &BTreeSet<usize>) -> Result<Option<Body>> {
    let Some(form) = body.ops_body() else { return Ok(None) };
    let faces = selected.iter().copied().collect::<Vec<_>>();
    let edit = || solidcore_ops::blend::delete_fillet_chain(&form.0, &form.1, &faces);
    let removed = match solidcraft_smlib::stop::token() {
        Some(token) => solidcore_ops::cancel::within(&token, edit),
        None => edit(),
    }
    .map_err(|e| match e {
        solidcore_ops::Error::Internal(why) => KernelError::Internal(format!("Delete Face: {why}")),
        e => KernelError::Failed(format!("Delete Face: {e}")),
    })?;
    let Some(removed) = removed else { return Ok(None) };
    let borders = crate::history::edge_faces(body.record());
    let at_gap = |fs: &[usize]| fs.iter().any(|f| selected.contains(f));
    let modified: HashSet<_> = borders.iter().filter(|fs| at_gap(fs)).flatten().copied().collect();
    let faces = removed
        .face_sources
        .into_iter()
        .map(|f| crate::record::from_face(0, f, if modified.contains(&f) { Change::Modified } else { Change::Kept }))
        .collect();
    let edges: Vec<_> =
        removed.edge_sources.into_iter().map(|e| crate::record::pieces_of(0, e.filter(|i| borders.get(*i).is_some_and(|fs| !at_gap(fs))))).collect();
    let healed = Body::from_smlib_record(removed.record)?;
    let history = crate::record::of_faces(healed.record(), 1, faces, &edges);
    Ok(Some(healed.with_history(history, &[body])?))
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
    fn delete_split_rim_keeps_both_supports_and_records_their_sharp_meeting() {
        use crate::history::{Change, EdgeOrigin, FaceOrigin};
        use std::collections::BTreeSet;

        let ball = crate::sphere(Vec3::ZERO, 5.0).unwrap();
        let upper = box_solid(Vec3::new(-6.0, -6.0, 0.0), Vec3::new(6.0, 6.0, 6.0)).unwrap();
        let half = boolean(&ball, &upper, BoolOp::Intersect).unwrap().unwrap();
        let rim = crate::tangent_chain(&half, &[Vec3::new(0.0, 5.0, 0.0)]).unwrap();
        let rounded = fillet(&half, &rim, 1.0).unwrap();
        let names = &rounded.history().unwrap().faces;
        let rounds: Vec<_> =
            names.iter().enumerate().filter(|(_, f)| f.origins.iter().any(|o| matches!(o, FaceOrigin::Edge { .. }))).map(|(i, _)| i).collect();
        assert_eq!(rounds.len(), 2);
        // A body imported without a feature tree still carries both original support faces.
        let input = crate::Body::from_record(rounded.record().clone()).unwrap();
        let supports: BTreeSet<_> = (0..input.face_count()).filter(|i| !rounds.contains(i)).collect();
        let healed = delete_faces(&input, &[rounds[1], rounds[0], rounds[1]]).unwrap();
        let history = healed.history().unwrap();
        let kept: BTreeSet<_> = history
            .faces
            .iter()
            .map(|f| {
                assert_eq!(f.change, Change::Modified);
                let [FaceOrigin::Face { operand: 0, face }] = f.origins[..] else { panic!("lost a rim support: {f:?}") };
                face
            })
            .collect();
        assert_eq!(kept, supports);
        assert!(history.edges.iter().any(|e| matches!(e.origins.as_slice(), [EdgeOrigin::Meet { .. }])));
    }

    fn step_roundtrip(body: &crate::Body) -> crate::Body {
        let step = crate::step_export(&[body], "SolidCraft").unwrap();
        crate::step_import(&step).unwrap().bodies.remove(0).body
    }

    fn picked_faces(body: &crate::Body, samples: &[Vec3]) -> Vec<usize> {
        body.classify_points_with_normals(samples, None)
            .unwrap()
            .into_iter()
            .map(|hit| match hit {
                crate::points::ClassifiedPoint::On { face, .. } => face,
                other => panic!("expected a point on the face, got {other:?}"),
            })
            .collect()
    }

    #[test]
    fn delete_one_round_or_its_ball_corner_on_a_rounded_housing() {
        use solidcraft_geom::Vec3Geom;

        let origin = Vec3::new(11.0, -7.0, 4.0);
        let size = Vec3::new(36.0, 29.0, 24.0);
        let radius = 3.0;
        let block = box_solid(origin, origin + size).unwrap();
        let edges: Vec<_> = block.edges(0.001).unwrap().into_iter().map(|edge| edge.mid).collect();
        let rounded = fillet(&block, &edges, radius).unwrap();
        let input = crate::Body::from_record(rounded.record().clone()).unwrap();
        let strip_at = origin + Vec3::new(0.0, size.y / 2.0, 0.0);
        let remaining: Vec<_> = edges.into_iter().filter(|p| p.dist(strip_at) > 0.001).collect();
        assert_eq!(remaining.len(), 11);
        let control = fillet(&block, &remaining, radius).unwrap();

        let d = radius * (1.0 - std::f64::consts::FRAC_1_SQRT_2);
        let strip = picked_faces(&input, &[strip_at + Vec3::new(d, 0.0, d)]);
        let healed = delete_faces(&input, &strip).unwrap();
        assert_eq!(healed.face_count(), control.face_count());
        assert!((healed.volume_at(1e-8).unwrap() - control.volume_at(1e-8).unwrap()).abs() < 1e-4);
        assert!(healed.edges(0.001).unwrap().iter().any(|edge| edge.mid.dist(strip_at) < 0.001));

        let d = radius * (1.0 - 1.0 / 3.0_f64.sqrt());
        let ball = picked_faces(&input, &[origin + Vec3::new(d, d, d)]);
        let healed = delete_faces(&input, &ball).unwrap();
        assert_eq!(healed.face_count() + 1, input.face_count());
        // The corner gains an octant of three intersecting cylinders less the ball octant.
        let gained = radius.powi(3) * (2.0 - 2.0_f64.sqrt() - std::f64::consts::PI / 6.0);
        assert!((healed.volume_at(1e-8).unwrap() - input.volume_at(1e-8).unwrap() - gained).abs() < 1e-4);
    }

    #[test]
    fn deleting_a_planar_cylinder_cap_explains_the_healing_limit() {
        let body = step_roundtrip(&cylinder(Vec3::ZERO, Vec3::Z, 10.0, 20.0).unwrap());
        let cap = picked_faces(&body, &[Vec3::new(0.0, 0.0, 20.0)])[0];
        let wall = picked_faces(&body, &[Vec3::new(6.0, 8.0, 10.0)])[0];

        let Err(crate::KernelError::Invalid(why)) = delete_faces(&body, &[cap]) else {
            panic!("expected Delete Face to refuse healing a planar cylinder cap");
        };
        assert!(why.contains(&format!("face {cap} (a plane), borders face {wall}, a cylinder")), "{why}");
        assert!(why.contains("healing this selection requires planar neighbours"), "{why}");
        assert!(!why.contains("tangent chain"), "{why}");
    }

    #[test]
    fn delete_step_ball_root_grows_its_spherical_support_back_to_the_plate() {
        let plate = cylinder(Vec3::new(0.0, 0.0, -2.0), Vec3::Z, 8.0, 2.0).unwrap();
        let ball = crate::sphere(Vec3::new(0.0, 0.0, 1.0), 3.0).unwrap();
        let sharp = boolean(&plate, &ball, BoolOp::Union).unwrap().unwrap();
        let rim = crate::tangent_chain(&sharp, &[Vec3::new(0.0, 8.0_f64.sqrt(), 0.0)]).unwrap();
        let input = step_roundtrip(&fillet(&sharp, &rim, 1.0).unwrap());

        // The R1 round is centred at radius 4, height 1. At half height it covers this sphere point.
        let xy = (4.0 - 3.0_f64.sqrt() / 2.0) * std::f64::consts::FRAC_1_SQRT_2;
        let rounds = picked_faces(&input, &[Vec3::new(xy, xy, 0.5), Vec3::new(-xy, -xy, 0.5)]);
        assert_ne!(rounds[0], rounds[1]);
        let sphere_point = Vec3::new(0.0, 35.0_f64.sqrt() / 2.0, 0.5);
        assert_eq!(input.classify_point(sphere_point).unwrap(), crate::PointClass::Inside);
        let healed = delete_faces(&input, &rounds).unwrap();
        assert_eq!(healed.classify_point(sphere_point).unwrap(), crate::PointClass::OnBoundary);

        // The plate contributes 128π volume and 152π exposed area. Its R3 spherical cap
        // stands 4 high, adding 80π/3 volume and 24π area.
        let measure = crate::measure(&healed).unwrap();
        let volume = 464.0 * std::f64::consts::PI / 3.0;
        let area = 176.0 * std::f64::consts::PI;
        assert!((measure.volume - volume).abs() <= 1e-8 * volume, "{}", measure.volume);
        assert!((measure.area - area).abs() <= 1e-8 * area, "{}", measure.area);
    }

    #[test]
    fn delete_open_rim_of_step_block_keeps_the_unselected_bore() {
        use crate::history::{Change, FaceOrigin};

        let block = box_solid(Vec3::ZERO, Vec3::new(20.0, 12.0, 8.0)).unwrap();
        let corners = fillet(&block, &[Vec3::new(20.0, 0.0, 4.0), Vec3::new(20.0, 12.0, 4.0)], 3.0).unwrap();
        let drill = cylinder(Vec3::new(10.0, 6.0, -1.0), Vec3::Z, 1.25, 10.0).unwrap();
        let drilled = boolean(&corners, &drill, BoolOp::Cut).unwrap().unwrap();
        let rim = crate::tangent_chain(&drilled, &[Vec3::new(10.0, 0.0, 8.0)]).unwrap();
        let rounded = fillet(&drilled, &rim, 1.0).unwrap();
        let input = step_roundtrip(&rounded);

        // Halfway around the R1 section, on each straight and around each R3 corner.
        // The turning section's centre is at radius 3 - 1 = 2 from the corner axis.
        let d = std::f64::consts::FRAC_1_SQRT_2;
        let turn = d * (2.0 + d);
        let top = 7.0 + d;
        let samples = [
            Vec3::new(10.0, 1.0 - d, top),
            Vec3::new(17.0 + turn, 3.0 - turn, top),
            Vec3::new(19.0 + d, 6.0, top),
            Vec3::new(17.0 + turn, 9.0 + turn, top),
            Vec3::new(10.0, 11.0 + d, top),
        ];
        let rounded_faces = picked_faces(&input, &samples);
        assert_eq!(rounded_faces.iter().collect::<std::collections::BTreeSet<_>>().len(), 5);
        let bore = [Vec3::new(10.0 + d * 1.25, 6.0 + d * 1.25, 4.0)];
        let bore_input = picked_faces(&input, &bore)[0];
        let healed = delete_faces(&input, &rounded_faces).unwrap();
        let bore_output = picked_faces(&healed, &bore)[0];
        let source = &healed.history().unwrap().faces[bore_output];
        assert_eq!(source.origins, [FaceOrigin::Face { operand: 0, face: bore_input }]);
        assert_eq!(source.change, Change::Kept);
        assert_eq!(healed.classify_point(Vec3::new(10.0, 6.0, 4.0)).unwrap(), crate::PointClass::Outside);
        assert_eq!(healed.classify_point(Vec3::new(10.0, 0.12, 7.88)).unwrap(), crate::PointClass::Inside);
    }

    #[test]
    fn deleting_ring_groove_faces_restores_the_shaft_and_tracks_both_supports() {
        use crate::history::{Change, FaceOrigin};

        let core = cylinder(Vec3::ZERO, Vec3::Z, 4.0, 20.0).unwrap();
        let collar = |z| cylinder(Vec3::new(0.0, 0.0, z), Vec3::Z, 5.0, 8.0).unwrap();
        let lower = boolean(&core, &collar(0.0), BoolOp::Union).unwrap().unwrap();
        let shaft = boolean(&lower, &collar(12.0), BoolOp::Union).unwrap().unwrap();
        let face_at = |r, z| shaft.nearest_face(Vec3::new(0.0, r, z)).unwrap().1;
        let selected = [face_at(4.5, 8.0), face_at(4.0, 10.0), face_at(4.5, 12.0)];
        let supports = [face_at(5.0, 4.0), face_at(5.0, 16.0)];

        let closed = delete_faces(&shaft, &selected).unwrap();
        assert_eq!(closed.face_count(), 3);
        assert!((closed.volume_at(1e-8).unwrap() - 500.0 * std::f64::consts::PI).abs() < 1e-6);
        let hist = closed.history().unwrap();
        let wall = closed.nearest_face(Vec3::new(0.0, 5.0, 10.0)).unwrap().1;
        assert_eq!(hist.faces[wall].change, Change::Merged);
        for support in supports {
            assert!(hist.faces[wall].origins.contains(&FaceOrigin::Face { operand: 0, face: support }));
        }
        for z in [0.0, 20.0] {
            let cap = closed.nearest_face(Vec3::new(0.0, 0.0, z)).unwrap().1;
            assert_eq!(hist.faces[cap].change, Change::Kept);
            assert_eq!(hist.faces[cap].origins, [FaceOrigin::Face { operand: 0, face: face_at(0.0, z) }]);
        }
    }

    #[test]
    fn deleting_a_round_between_curved_supports_restores_the_sharp_rim() {
        use crate::history::{EdgeOrigin, FaceOrigin};

        let post = cylinder(Vec3::ZERO, Vec3::Z, 4.0, 6.0).unwrap();
        let ball = crate::sphere(Vec3::new(0.0, 0.0, 3.0), 5.0).unwrap();
        let upper = box_solid(Vec3::new(-6.0, -6.0, 6.0), Vec3::new(6.0, 6.0, 12.0)).unwrap();
        let cap = boolean(&ball, &upper, BoolOp::Intersect).unwrap().unwrap();
        // Rotating the spherical cap splits its common rim with the post at the two seams.
        let cap = crate::transform(&cap, Vec3::ZERO, Vec3::ZERO, Vec3::Z, 1.5).unwrap();
        let sharp = boolean(&post, &cap, BoolOp::Union).unwrap().unwrap();
        let rim = crate::tangent_chain(&sharp, &[Vec3::new(0.0, 4.0, 6.0)]).unwrap();
        let rounded = fillet(&sharp, &rim, 1.0).unwrap();
        let rounds: Vec<_> = rounded
            .history()
            .unwrap()
            .faces
            .iter()
            .enumerate()
            .filter(|(_, f)| f.origins.iter().any(|o| matches!(o, FaceOrigin::Edge { .. })))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(rounds.len(), 2);

        let input = crate::Body::from_record(rounded.record().clone()).unwrap();
        let healed = delete_faces(&input, &rounds).unwrap();
        // A fillet-chain removal records a new sharp edge between the two surviving supports.
        let edges = &healed.history().unwrap().edges;
        assert!(edges.iter().any(|e| matches!(e.origins.as_slice(), [EdgeOrigin::Meet { .. }])));
        assert_eq!(healed.face_count(), sharp.face_count());
        // The R4 by 6 post contributes 96π and the R5 cap of height 2 contributes 52π/3.
        let expected = 340.0 * std::f64::consts::PI / 3.0;
        let actual = crate::measure_mass(&healed).unwrap().volume;
        assert!((actual - expected).abs() < 1e-7, "restored volume {actual}, expected {expected}");
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

    #[test]
    fn deleting_a_bevel_keeps_the_ball_and_plate_face_origins() {
        use crate::history::{Change, FaceOrigin};
        use solidcore_ops::boolean::{BooleanOp, boolean as join};

        let plate = solidcore_ops::make_cylinder([0.0, 0.0, -2.0], 8.0, 2.0).unwrap();
        let ball = solidcore_ops::make_sphere([0.0, 0.0, 1.0], 3.0).unwrap();
        let bevel = solidcore_ops::make_cone([0.0; 3], 4.0, 3.0, 1.0).unwrap();
        let sharp = join(plate, ball, BooleanOp::Union).unwrap().unwrap();
        let beveled = join(sharp, bevel, BooleanOp::Union).unwrap().unwrap();
        let part = crate::Body::from_record(beveled.to_record().unwrap().0).unwrap();
        let offset = Vec3::new(11.0, -6.0, 9.0);
        let original = crate::transform(&part, offset, Vec3::ZERO, Vec3::Y, std::f64::consts::FRAC_PI_2).unwrap();
        let placed = |p: Vec3| offset + Vec3::new(p.z, p.y, -p.x);
        let (_, picked) = original.nearest_face(placed(Vec3::new(0.0, 3.5, 0.5))).unwrap();
        let healed = delete_faces(&original, &[picked]).unwrap();
        let history = healed.history().unwrap();
        assert_eq!(healed.face_count(), 4);
        for (point, change) in [
            (Vec3::new(6.0, 0.0, 0.0), Change::Modified),
            (Vec3::new(0.0, 5.0_f64.sqrt(), 3.0), Change::Modified),
            (Vec3::new(1.0, 0.0, -2.0), Change::Kept),
            (Vec3::new(8.0, 0.0, -1.0), Change::Kept),
        ] {
            let (_, parent) = original.nearest_face(placed(point)).unwrap();
            let (_, face) = healed.nearest_face(placed(point)).unwrap();
            assert_eq!(history.faces[face].origins, [FaceOrigin::Face { operand: 0, face: parent }]);
            assert_eq!(history.faces[face].change, change);
        }
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

    /// #20671: deleting an unsupported face (e.g. one face of a box or tube wall whose neighbours
    /// can't close the gap) reports a usable error without dumping raw internal B-rep rebuild text.
    #[test]
    fn unclosable_gap_refusal_omits_internal_brep_text() {
        let outer = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 20.0).unwrap();
        let inner = cylinder(Vec3::new(0.0, 0.0, -1.0), Vec3::Z, 6.0, 22.0).unwrap();
        let tube = boolean(&outer, &inner, BoolOp::Cut).unwrap().unwrap();
        let wall = faces_where(&tube, |p| (p.x.hypot(p.y) - 10.0).abs() < 1e-2);
        let err = delete_faces(&tube, &wall).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("the neighbouring faces do not close the gap"), "{msg}");
        assert!(!msg.contains("rebuilding the B-rep"), "{msg}");
        assert!(!msg.contains("outer loop"), "{msg}");
    }
}
