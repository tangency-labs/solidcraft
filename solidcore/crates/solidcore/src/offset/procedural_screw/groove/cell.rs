//! Candidate boundary complexes for a finite screw cell.
//!
//! This is only the combinatorial premise, not a geometric certificate. A caller
//! must separately prove common support, branch tubes, winding and material side.
//! Cancelling an internal radial pair is the oriented gluing of two face disks.
//! A connected orientable manifold with one boundary and Euler characteristic one
//! is a disk. No UV endpoint comparison is used to identify its vertices.
//! Deviations: new construction; all walks are bounded by the input use count.

use super::{bad, Result};
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::topology::edgeuse::p1 as uses;
use crate::topology::{edge, loopuse, Brep, EdgeuseId, FaceId, FaceuseId};
use std::collections::{BTreeMap, BTreeSet};

/// A candidate disk boundary in its material region's traversal order.
/// It does not assert anything about the geometry of the member faces.
pub(in crate::offset::procedural_screw) fn disk_boundary(
    body: &Brep,
    faces: &[FaceId],
) -> Result<Vec<EdgeuseId>> {
    let members: BTreeSet<_> = faces.iter().copied().collect();
    if members.is_empty() || members.len() != faces.len() {
        return Err(bad("thread support group is empty or repeats a face"));
    }
    let mut region = None;
    let mut owners = BTreeMap::new();
    let mut edges = BTreeSet::new();
    let mut vertices = BTreeSet::new();
    let mut adjacency: BTreeMap<FaceId, BTreeSet<FaceId>> = BTreeMap::new();
    for &face in faces {
        let fu = material_use(body, face)?;
        let current = uses::faceuse_region(body, fu)?;
        if current.is_none() || region.is_some_and(|r| Some(r) != current) {
            return Err(bad("thread support group crosses material regions"));
        }
        region = current;
        let loops = uses::faceuse_loopuses(body, fu, false, None)?;
        let [lu] = loops.as_slice() else {
            return Err(bad("thread member must have one disk boundary"));
        };
        if body.loopuse(*lu)?.orientation != OrientType::Same {
            return Err(bad("thread member has an interior boundary"));
        }
        for eu in loopuse::edgeuses(body, *lu, None, None)? {
            if !matches!(
                body.edgeuse(eu)?.orientation,
                OrientType::Same | OrientType::Opposite
            ) {
                return Err(bad("thread edge use has no orientation"));
            }
            let e = body
                .edgeuse(eu)?
                .edge
                .ok_or_else(|| bad("thread use has no edge"))?;
            if !edge::is_manifold(body, e)? {
                return Err(bad("thread boundary is not manifold"));
            }
            owners.insert(eu, face);
            edges.insert(e);
            let (a, b) = edge::start_end_vertices(body, e)?;
            vertices.insert(a.ok_or_else(|| bad("thread edge has no start vertex"))?);
            vertices.insert(b.ok_or_else(|| bad("thread edge has no end vertex"))?);
        }
    }
    let mut boundary = BTreeSet::new();
    for (&eu, &face) in &owners {
        let other = uses::radial(body, eu)?.ok_or_else(|| bad("thread edge has no radial pair"))?;
        if let Some(&neighbor) = owners.get(&other) {
            if body.edgeuse(eu)?.orientation == body.edgeuse(other)?.orientation {
                return Err(bad("thread seam orientations do not cancel"));
            }
            adjacency.entry(face).or_default().insert(neighbor);
        } else {
            boundary.insert(eu);
        }
    }
    let first_face = *faces
        .first()
        .ok_or_else(|| bad("thread support group is empty"))?;
    let mut reached = BTreeSet::from([first_face]);
    let mut pending = vec![first_face];
    while let Some(face) = pending.pop() {
        if let Some(neighbors) = adjacency.get(&face) {
            for &neighbor in neighbors {
                if reached.insert(neighbor) {
                    pending.push(neighbor);
                }
            }
        }
    }
    let vertex_faces = vertices
        .len()
        .checked_add(faces.len())
        .ok_or_else(|| bad("thread disk vertex and face count overflowed"))?;
    let edge_one = edges
        .len()
        .checked_add(1)
        .ok_or_else(|| bad("thread disk edge count overflowed"))?;
    if reached != members || vertex_faces != edge_one {
        return Err(bad("thread support group is not a connected disk complex"));
    }
    let start = *boundary
        .first()
        .ok_or_else(|| bad("thread support has no boundary"))?;
    let mut result = Vec::new();
    let mut visited = BTreeSet::new();
    let mut boundary_vertices = BTreeSet::new();
    let mut cursor = start;
    loop {
        if !boundary.contains(&cursor) || !visited.insert(cursor) {
            return Err(bad("thread boundary repeats or leaves its support"));
        }
        let vu = body
            .edgeuse(cursor)?
            .vertexuse
            .ok_or_else(|| bad("thread boundary has no vertex use"))?;
        let vertex = body
            .vertexuse(vu)?
            .vertex
            .ok_or_else(|| bad("thread boundary has no vertex"))?;
        if !boundary_vertices.insert(vertex) {
            return Err(bad("thread boundary pinches at a repeated vertex"));
        }
        result.push(cursor);
        let mut next = body
            .edgeuse(cursor)?
            .ccw
            .ok_or_else(|| bad("thread loop is open"))?;
        let mut found = false;
        for _ in 0..=owners.len() {
            if boundary.contains(&next) {
                found = true;
                break;
            }
            if !owners.contains_key(&next) {
                return Err(bad("thread seam walk leaves its support"));
            }
            let pair = uses::radial(body, next)?.ok_or_else(|| bad("thread seam has no pair"))?;
            if !owners.contains_key(&pair) {
                return Err(bad("thread seam pair leaves its support"));
            }
            next = body
                .edgeuse(pair)?
                .ccw
                .ok_or_else(|| bad("thread seam loop is open"))?;
        }
        if !found {
            return Err(bad("thread seam walk does not reach a boundary"));
        }
        cursor = next;
        if cursor == start {
            break;
        }
    }
    if visited != boundary {
        return Err(bad("thread support has more than one boundary component"));
    }
    Ok(result)
}

fn material_use(body: &Brep, face: FaceId) -> Result<FaceuseId> {
    let primary = body
        .face(face)?
        .faceuse
        .ok_or_else(|| bad("thread face has no use"))?;
    let mate = body
        .faceuse(primary)?
        .mate
        .ok_or_else(|| bad("thread face has no mate"))?;
    let mut material = None;
    for fu in [primary, mate] {
        if !matches!(
            body.faceuse(fu)?.orientation,
            OrientType::Same | OrientType::Opposite
        ) {
            return Err(bad("thread face use has no orientation"));
        }
        let region =
            uses::faceuse_region(body, fu)?.ok_or_else(|| bad("thread face has no region"))?;
        match body.region(region)?.is_void {
            0 if material.is_none() => material = Some(fu),
            1 => {}
            _ => return Err(bad("thread face does not separate material and void")),
        }
    }
    material.ok_or_else(|| bad("thread face bounds no material region"))
}

/// Match the candidate family's boundary to two cylinder and two cap groups.
/// Labels 0/1 are the two cylinder supports; 2/3 are the two cap supports.
pub(super) fn four_chains(
    body: &Brep,
    faces: &[FaceId],
    labels: &BTreeMap<FaceId, usize>,
) -> Result<()> {
    let mut runs = Vec::new();
    for eu in disk_boundary(body, faces)? {
        let opposite = uses::radial(body, eu)?.ok_or_else(|| bad("thread rail has no neighbor"))?;
        let fu =
            uses::faceuse(body, opposite)?.ok_or_else(|| bad("thread rail has no face use"))?;
        let face = body
            .faceuse(fu)?
            .face
            .ok_or_else(|| bad("thread rail has no face"))?;
        let label = *labels
            .get(&face)
            .ok_or_else(|| bad("thread family has an unrecognized boundary support"))?;
        if runs.last() != Some(&label) {
            runs.push(label);
        }
    }
    if runs.first() == runs.last() {
        runs.pop();
    }
    let [a, b, c, d] = runs.as_slice() else {
        return Err(bad("thread family does not have four boundary chains"));
    };
    if [*a, *b, *c, *d].into_iter().collect::<BTreeSet<_>>() != BTreeSet::from([0, 1, 2, 3])
        || (*a < 2) == (*b < 2)
        || (*b < 2) == (*c < 2)
        || (*c < 2) == (*d < 2)
        || (*d < 2) == (*a < 2)
    {
        return Err(bad("thread cylinder and cap chains do not alternate"));
    }
    Ok(())
}
