//! A mesh's triangles and edges listed per face, built in one pass over each, from the mesh's
//! public tables (`tri_face`, `edge_faces`, `seams`, `sharp_edges`).
//!
//! Lighting a face or a surface of many faces (an imported surface split into hundreds of
//! pieces) used to scan every triangle of the body once per face, and every edge once per face
//! to join the pieces: O(faces × triangles). With this index the work follows the triangles and
//! edges of the faces lit. Lists keep the mesh's order, so what is drawn from them is the same,
//! in the same order, as from the scans they replace. The viewport keeps one per lit mesh.
//!
//! It lives here, beside its only user, rather than in `geom`: it is a drawing aid built from
//! what `Mesh` already exposes.

use std::borrow::Cow;
use std::collections::HashSet;

use solidcraft_engine::geom::Mesh;

/// Face `f`'s triangles and edges, as slices of two flat lists.
///
/// Face numbers come from the mesh and are not trusted: one far beyond the mesh's size is not
/// given a slot (that would allocate for every number below it); it is looked up by a scan.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct FaceIndex {
    /// `tris[tri_start[f]..tri_start[f + 1]]`: face `f`'s triangles, ascending.
    tri_start: Vec<usize>,
    tris: Vec<u32>,
    /// `edges[edge_start[f]..edge_start[f + 1]]`: the edges bounding face `f`, ascending, once each.
    edge_start: Vec<usize>,
    edges: Vec<u32>,
    /// Some face number is beyond the slots: those faces are found by scanning the mesh.
    overflow: bool,
}

/// `start` and `items` (a CSR list) from `(slot, item)` pairs in item order.
fn csr(slots: usize, pairs: impl Iterator<Item = (usize, u32)> + Clone) -> (Vec<usize>, Vec<u32>) {
    let mut start = vec![0usize; slots + 1];
    for (f, _) in pairs.clone() {
        if let Some(c) = start.get_mut(f + 1) {
            *c += 1;
        }
    }
    let mut sum = 0;
    for s in &mut start {
        sum += *s;
        *s = sum;
    }
    let mut fill = start.clone();
    let mut items = vec![0u32; start.last().copied().unwrap_or(0)];
    for (f, item) in pairs {
        if let Some(at) = fill.get_mut(f)
            && let Some(slot) = items.get_mut(*at)
        {
            *slot = item;
            *at += 1;
        }
    }
    (start, items)
}

impl FaceIndex {
    pub(crate) fn new(m: &Mesh) -> FaceIndex {
        // As many slots as the mesh has face references: enough for every honest face number.
        let refs = m.tri_face.len() + m.edge_faces.iter().map(Vec::len).sum::<usize>();
        let highest = m.tri_face.iter().chain(m.edge_faces.iter().flatten()).map(|&f| f as usize).max();
        let slots = highest.map_or(0, |h| (h + 1).min(refs + 1));
        let overflow = highest.is_some_and(|h| h >= slots);
        let tri_pairs = m.tri_face.iter().take(m.triangles.len()).enumerate().filter_map(|(t, &f)| Some((f as usize, u32::try_from(t).ok()?)));
        let (tri_start, tris) = csr(slots, tri_pairs);
        // An edge listing one face twice is listed once for it.
        let edge_pairs = m.edge_faces.iter().enumerate().flat_map(|(e, fs)| {
            let e = u32::try_from(e).ok();
            fs.iter().enumerate().filter(move |(i, f)| !fs[..*i].contains(f)).filter_map(move |(_, &f)| Some((f as usize, e?)))
        });
        let (edge_start, edges) = csr(slots, edge_pairs);
        FaceIndex { tri_start, tris, edge_start, edges, overflow }
    }

    fn slot<'a>(start: &[usize], items: &'a [u32], face: usize) -> Option<&'a [u32]> {
        let (&a, &b) = (start.get(face)?, start.get(face + 1)?);
        items.get(a..b)
    }

    /// Face `face`'s triangles (indices into `m.triangles`), in mesh order.
    pub(crate) fn triangles(&self, m: &Mesh, face: usize) -> Cow<'_, [u32]> {
        match Self::slot(&self.tri_start, &self.tris, face) {
            Some(s) => Cow::Borrowed(s),
            None if self.overflow => Cow::Owned(
                m.tri_face
                    .iter()
                    .take(m.triangles.len())
                    .enumerate()
                    .filter(|(_, f)| **f as usize == face)
                    .filter_map(|(t, _)| u32::try_from(t).ok())
                    .collect(),
            ),
            None => Cow::Borrowed(&[]),
        }
    }

    /// The edges bounding face `face`, ascending: `Mesh::face_edges`.
    pub(crate) fn edges(&self, m: &Mesh, face: usize) -> Cow<'_, [u32]> {
        match Self::slot(&self.edge_start, &self.edges, face) {
            Some(s) => Cow::Borrowed(s),
            None if self.overflow => Cow::Owned(
                m.edge_faces
                    .iter()
                    .enumerate()
                    .filter(|(_, fs)| fs.iter().any(|f| *f as usize == face))
                    .filter_map(|(e, _)| u32::try_from(e).ok())
                    .collect(),
            ),
            None => Cow::Borrowed(&[]),
        }
    }

    /// `Mesh::surface_patch` of `m` (the mesh this index was built from), through the index.
    pub(crate) fn surface_patch(&self, m: &Mesh, face: usize) -> Vec<usize> {
        let mut patch = vec![face];
        let mut seen: HashSet<usize> = HashSet::from([face]);
        let mut todo = vec![face];
        while let Some(f) = todo.pop() {
            for &ei in self.edges(m, f).iter() {
                let ei = ei as usize;
                if !m.seams.get(ei).copied().unwrap_or(false) || m.sharp_edges.get(ei).copied().unwrap_or(false) {
                    continue;
                }
                for &next in m.edge_faces.get(ei).into_iter().flatten() {
                    let next = next as usize;
                    if seen.insert(next) {
                        patch.push(next);
                        todo.push(next);
                    }
                }
            }
        }
        patch.sort_unstable();
        patch
    }

    /// The edges bounding any of `faces`, ascending, once each.
    pub(crate) fn edges_of(&self, m: &Mesh, faces: &[usize]) -> Vec<usize> {
        let mut out: Vec<usize> = faces.iter().flat_map(|&f| self.edges(m, f).iter().map(|&e| e as usize).collect::<Vec<_>>()).collect();
        out.sort_unstable();
        out.dedup();
        out
    }
}
