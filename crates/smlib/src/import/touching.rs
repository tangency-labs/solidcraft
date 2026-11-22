//! Solids touching each other, given back to SMLib as SMLib makes them.
//!
//! SMLib keeps solids that touch each other along an edge or at a vertex as one non-manifold
//! boundary: the edge or vertex is one entity that faces of both solids use, and the empty space
//! round them is one runtime shell. The record keeps one copy of such an edge and vertex per
//! solid, so that each record shell is a closed manifold boundary of its own (`crate::export`,
//! `split_shared`). Sent back as they are, the copies are distinct coincident entities in one
//! body. SMLib's boolean then cannot relate a third body's one edge or vertex there to both of
//! them (its topology intersector's mates are one to one: "another entity is already related to
//! the other one", #14419), and its check refuses a face that gets two of them ("Face has
//! Coincident Vertices").
//!
//! [`shared`] finds those copies and glues them back for a boolean's operand: vertices of
//! different closed shells at the very same point become one vertex, and two edges of different
//! shells on the same curve and interval between the same glued vertices become one edge with
//! the uses of both, in radial order round it. Only exact copies are glued: the split copies a
//! point and a curve and never moves them. Entities that coincide only within a tolerance are left
//! as they are. Three or more solids round one edge would need the angles of their wedges to order
//! them, which this does not compute: such a record is not glued at all. Nor is a record whose
//! solids touch only at vertices: it is glued only where two solids share an edge.
//!
//! Round an edge two solids touch along, each copy's two uses bound one wedge of material. The
//! record lists an edge's uses in radial order, right-hand about the edge's direction, and SMLib's
//! radial walk enters each use at its `upper` side and leaves into the next sector from the other
//! side. That sector is material where the walk enters on the natural normal's side (`upper`) and
//! that normal points out of the material (its shell's `forward`), or neither: it leaves on the
//! side away from the material's outward normal. So each copy's uses are ordered [into the
//! material, out of it]. The
//! copies run the same way along the same curve, so that is the same turning sense for both, and
//! the glued edge's cycle is one copy's pair, then the other's: material, space, material, space.

use std::collections::BTreeMap;

use solidcore_record::brep as b;

use super::{R, Unsupported};

/// A record whose touching solids share their edges and vertices again, with the pairs of shells
/// that now touch: on their empty side they are one runtime shell in SMLib.
pub(super) struct Glued {
    pub(super) brep: b::Brep,
    pub(super) joined: Vec<(u32, u32)>,
}

fn idx(i: usize) -> R<u32> {
    u32::try_from(i).map_err(|_| Unsupported::Invalid("index"))
}

/// Whether two vertices of `brep` lie at the very same point: the cheap test that comes before
/// [`shared`], which a record with no copies fails.
pub(super) fn may_share(brep: &b::Brep) -> bool {
    let mut points: Vec<[u64; 3]> = brep.vertices.iter().map(|v| v.point.map(f64::to_bits)).collect();
    points.sort_unstable();
    points.windows(2).any(|w| matches!(w, [p, q] if p == q))
}

/// `brep` with the copies of what its solids touch along glued back (see the module's notes);
/// `None` when no two of its solids share a copied edge.
pub(super) fn shared(brep: &b::Brep) -> R<Option<Glued>> {
    // The shell each face is in, and whether the face's natural normal points out of the
    // material there.
    let mut face_shell: Vec<Option<(u32, bool)>> = vec![None; brep.faces.len()];
    for (s, shell) in brep.shells.iter().enumerate() {
        let b::Shell::Faces { faces, .. } = shell else { continue };
        let s = idx(s)?;
        for f in faces {
            if let Some(slot) = face_shell.get_mut(f.face as usize) {
                *slot = Some((s, f.forward));
            }
        }
    }
    // The empty region round each closed shell of material: listed by exactly two regions, one
    // of them material.
    let material = |r: usize| r > 0 && brep.regions.get(r).is_some_and(|x| !x.void);
    let mut listed: Vec<Vec<usize>> = vec![Vec::new(); brep.shells.len()];
    for (r, region) in brep.regions.iter().enumerate() {
        for &s in &region.shells {
            if let Some(l) = listed.get_mut(s as usize) {
                l.push(r);
            }
        }
    }
    let space: Vec<Option<usize>> = listed
        .iter()
        .map(|rs| match rs.as_slice() {
            [p, q] if material(*p) != material(*q) => Some(if material(*p) { *q } else { *p }),
            _ => None,
        })
        .collect();
    let closed = |s: u32| space.get(s as usize).copied().flatten().is_some();

    // The one closed shell of material each edge is in: two uses, both on faces of that shell.
    let shell_of_face = |f: u32| face_shell.get(f as usize).copied().flatten().map(|(s, _)| s);
    let edge_shell: Vec<Option<u32>> = brep
        .edges
        .iter()
        .map(|e| match e.uses.as_slice() {
            [p, q] => shell_of_face(p.face).filter(|&s| shell_of_face(q.face) == Some(s) && closed(s)),
            _ => None,
        })
        .collect();
    // Likewise for each vertex, through its edges and vertex loops: `Some(None)` where it is not
    // in exactly one such shell.
    let mut vertex_shell: Vec<Option<Option<u32>>> = vec![None; brep.vertices.len()];
    let mut see = |v: u32, s: Option<u32>| {
        if let Some(slot) = vertex_shell.get_mut(v as usize) {
            *slot = Some(match *slot {
                None => s,
                Some(seen) if seen == s => seen,
                Some(_) => None,
            });
        }
    };
    for (e, edge) in brep.edges.iter().enumerate() {
        let s = edge_shell.get(e).copied().flatten();
        for &v in &edge.vertices {
            see(v, s);
        }
    }
    for (f, face) in brep.faces.iter().enumerate() {
        let s = shell_of_face(idx(f)?).filter(|&s| closed(s));
        for l in &face.loops {
            if let b::Loop::Vertex { vertex } = l {
                see(*vertex, s);
            }
        }
    }

    // Vertices at the very same point, each of another shell, whose shells share their empty
    // region: one vertex.
    let mut by_point: BTreeMap<[u64; 3], Vec<u32>> = BTreeMap::new();
    for (v, vertex) in brep.vertices.iter().enumerate() {
        if let Some(Some(Some(_))) = vertex_shell.get(v) {
            by_point.entry(vertex.point.map(f64::to_bits)).or_default().push(idx(v)?);
        }
    }
    let vertex_in = |v: u32| vertex_shell.get(v as usize).copied().flatten().flatten();
    let mut vertex_rep: Vec<u32> = (0..brep.vertices.len()).map(idx).collect::<R<_>>()?;
    let mut joined: Vec<(u32, u32)> = Vec::new();
    for group in by_point.values().filter(|g| g.len() > 1) {
        let shells: Vec<u32> = group.iter().filter_map(|&v| vertex_in(v)).collect();
        let mut distinct = shells.clone();
        distinct.sort_unstable();
        distinct.dedup();
        let around = |s: u32| space.get(s as usize).copied().flatten();
        let first = shells.first().copied();
        if distinct.len() != group.len() || !shells.iter().all(|&s| around(s) == first.and_then(around)) {
            continue;
        }
        let (Some(&rep), Some(rep_shell)) = (group.first(), first) else { continue };
        for (&v, &s) in group.iter().zip(&shells).skip(1) {
            if let Some(slot) = vertex_rep.get_mut(v as usize) {
                *slot = rep;
            }
            if !joined.contains(&(rep_shell, s)) {
                joined.push((rep_shell, s));
            }
        }
    }
    if joined.is_empty() {
        return Ok(None);
    }
    let glued_vertex = |v: u32| vertex_rep.get(v as usize).copied().ok_or(Unsupported::Invalid("edge vertex"));

    // Edges of different shells between the same glued vertices, on the same curve and interval:
    // one edge.
    let mut in_group = vec![false; brep.vertices.len()];
    for (v, &rep) in vertex_rep.iter().enumerate() {
        if rep as usize != v {
            for at in [v, rep as usize] {
                if let Some(slot) = in_group.get_mut(at) {
                    *slot = true;
                }
            }
        }
    }
    let glued_end = |v: u32| in_group.get(v as usize).copied().unwrap_or(false);
    let mut between: BTreeMap<[u32; 2], Vec<u32>> = BTreeMap::new();
    for (e, edge) in brep.edges.iter().enumerate() {
        let [p, q] = edge.vertices;
        if edge_shell.get(e).copied().flatten().is_some() && glued_end(p) && glued_end(q) {
            between.entry([glued_vertex(p)?, glued_vertex(q)?]).or_default().push(idx(e)?);
        }
    }
    let edge_at = |e: u32| brep.edges.get(e as usize).ok_or(Unsupported::Invalid("edge"));
    let same_copy = |x: &b::Edge, y: &b::Edge| x.curve == y.curve && x.nurbs == y.nurbs && x.interval == y.interval;
    let mut pairs: Vec<(u32, u32)> = Vec::new();
    for edges in between.values() {
        let mut classes: Vec<Vec<u32>> = Vec::new();
        for &e in edges {
            let edge = edge_at(e)?;
            let mut found = false;
            for class in &mut classes {
                if let Some(&first) = class.first()
                    && same_copy(edge_at(first)?, edge)
                {
                    class.push(e);
                    found = true;
                    break;
                }
            }
            if !found {
                classes.push(vec![e]);
            }
        }
        for class in classes {
            match class.as_slice() {
                [_] => {}
                [p, q] if edge_shell.get(*p as usize) != edge_shell.get(*q as usize) => pairs.push((*p, *q)),
                // three or more solids round one edge, or two copies in one shell
                _ => return Ok(None),
            }
        }
    }
    // Solids that touch only at vertices are left as the record keeps them: only an edge two
    // solids share is glued (with its vertices and any others the same solids share).
    if pairs.is_empty() {
        return Ok(None);
    }

    // Each copy's uses ordered [into the material, out of it] (see the module's notes).
    let after = |u: &b::EdgeUseRef| -> R<bool> {
        let (_, forward) = face_shell.get(u.face as usize).copied().flatten().ok_or(Unsupported::Invalid("edge use"))?;
        Ok(forward == u.upper)
    };
    let wedge = |e: u32| -> R<Option<[b::EdgeUseRef; 2]>> {
        let [p, q] = edge_at(e)?.uses.as_slice() else { return Ok(None) };
        Ok(match (after(p)?, after(q)?) {
            (true, false) => Some([*p, *q]),
            (false, true) => Some([*q, *p]),
            _ => None,
        })
    };
    let mut edge_rep: Vec<u32> = (0..brep.edges.len()).map(idx).collect::<R<_>>()?;
    let mut merged_uses: BTreeMap<u32, Vec<b::EdgeUseRef>> = BTreeMap::new();
    for &(p, q) in &pairs {
        let (Some(wp), Some(wq)) = (wedge(p)?, wedge(q)?) else { return Ok(None) };
        merged_uses.insert(p, wp.into_iter().chain(wq).collect());
        if let Some(slot) = edge_rep.get_mut(q as usize) {
            *slot = p;
        }
    }

    // The record with each glued group as its first member, the others removed and everything
    // renumbered; each kept vertex and edge takes the largest tolerance of its group.
    let renumber = |rep: &[u32]| -> R<Vec<Option<u32>>> {
        let mut next = 0u32;
        rep.iter()
            .enumerate()
            .map(|(i, &r)| {
                if r as usize == i {
                    let n = next;
                    next = next.checked_add(1).ok_or(Unsupported::Invalid("index"))?;
                    Ok(Some(n))
                } else {
                    Ok(None)
                }
            })
            .collect()
    };
    let vertex_new = renumber(&vertex_rep)?;
    let edge_new = renumber(&edge_rep)?;
    let new_vertex =
        |v: u32| vertex_rep.get(v as usize).and_then(|&r| vertex_new.get(r as usize).copied().flatten()).ok_or(Unsupported::Invalid("vertex"));
    let new_edge = |e: u32| edge_rep.get(e as usize).and_then(|&r| edge_new.get(r as usize).copied().flatten()).ok_or(Unsupported::Invalid("edge"));

    let mut vertices: Vec<b::Vertex> = Vec::new();
    for (v, vertex) in brep.vertices.iter().enumerate() {
        let v = idx(v)?;
        match vertices.get_mut(new_vertex(v)? as usize) {
            Some(kept) => kept.tolerance = kept.tolerance.max(vertex.tolerance),
            None => vertices.push(vertex.clone()),
        }
    }
    let mut edges: Vec<b::Edge> = Vec::new();
    for (e, edge) in brep.edges.iter().enumerate() {
        let e = idx(e)?;
        match edges.get_mut(new_edge(e)? as usize) {
            Some(kept) => kept.tolerance = kept.tolerance.max(edge.tolerance),
            None => {
                let mut kept = edge.clone();
                let [p, q] = edge.vertices;
                kept.vertices = [new_vertex(p)?, new_vertex(q)?];
                if let Some(uses) = merged_uses.get(&e) {
                    kept.uses.clone_from(uses);
                }
                edges.push(kept);
            }
        }
    }
    let mut faces = brep.faces.clone();
    for face in &mut faces {
        for l in &mut face.loops {
            match l {
                b::Loop::Edges { edges } => {
                    for u in edges {
                        u.edge = new_edge(u.edge)?;
                    }
                }
                b::Loop::Vertex { vertex } => *vertex = new_vertex(*vertex)?,
            }
        }
    }
    let glued = b::Brep {
        version: brep.version,
        tolerance: brep.tolerance,
        regions: brep.regions.clone(),
        shells: brep.shells.clone(),
        faces,
        edges,
        vertices,
    };
    Ok(Some(Glued { brep: glued, joined }))
}
