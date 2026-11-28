//! What a click on a rim or a surface picks: the pieces of a circular edge, or of a surface a seam
//! splits, found over the display mesh the viewport shows, so hover, click, box select and the
//! selection an agent is told about agree (picking, not modelling: see `xtask/src/mesh_decisions.rs`).

use solidcore_record::Vec3;

use crate::Sel;
use crate::geom::prelude::*;

/// The point halfway along a polyline, by length (the first point of one with no length).
pub fn polyline_mid(pts: &[Vec3]) -> Vec3 {
    let len: f64 = pts.windows(2).map(|w| w[0].dist(w[1])).sum();
    let mut acc = 0.0;
    for w in pts.windows(2) {
        let l = w[0].dist(w[1]);
        if acc + l >= len * 0.5 && l > 0.0 {
            return w[0].lerp(w[1], (len * 0.5 - acc) / l);
        }
        acc += l;
    }
    pts.first().copied().unwrap_or_default()
}

/// The edges of body `b` that are one edge to the user with its edge `edge`, sorted, `edge` among
/// them: the pieces of a whole circle ([`MeshGeom::circular_rim`]) and those a curved surface's
/// seam ending on an edge splits it into, as a ball's seam splits a slot's arc
/// ([`crate::doc::ModelBody::whole_edge`], from the exact record). Only those `m`,
/// a mesh of `b` (whose edges are the record's), has. Hover, click and box select all take an
/// edge's pieces from here, so they agree.
pub fn edge_pieces(b: &crate::doc::ModelBody, m: &crate::geom::Mesh, edge: usize) -> Vec<usize> {
    let mut pieces = m.circular_rim(edge);
    pieces.extend(b.whole_edge(edge));
    pieces.sort_unstable();
    pieces.dedup();
    pieces.retain(|&i| m.edges.get(i).is_some());
    pieces
}

/// Expand a visible rim or surface into the kernel pieces a command needs. The original
/// face pick keeps its point (used by tools such as Hole and construction geometry).
pub fn grouped_selection(s: &crate::Session, sel: &Sel) -> Vec<Sel> {
    grouped_selection_in(&s.world_state().unwrap_or_default(), sel)
}

/// [`grouped_selection`] in the world `st`: the highlight, at every frame, asks it of the world
/// shown, which a joint drag drawn by moving its bodies keeps to the one it began on (asking the
/// session would place the drag's frame, every moved body again, #6325).
pub fn grouped_selection_in(st: &crate::doc::ModelState, sel: &Sel) -> Vec<Sel> {
    match sel {
        Sel::Edge { body, index, .. } => {
            if let Some(b) = st.body(body) {
                let m = b.mesh();
                // The picked piece first, then the others in order, as a face pick keeps its
                // own: Measure takes the first piece of an edge a click selected.
                let mut pieces = edge_pieces(b, &m, *index);
                pieces.sort_by_key(|i| usize::from(i != index));
                return pieces
                    .into_iter()
                    .filter_map(|i| m.edges.get(i).map(|e| Sel::Edge { body: body.clone(), index: i, point: polyline_mid(e) }))
                    .collect();
            }
        }
        Sel::Face { body, index, .. } => {
            if let Some(b) = st.body(body) {
                let m = b.mesh();
                let mut patch = m.surface_patch(*index);
                patch.sort_by_key(|i| usize::from(i != index));
                return patch
                    .into_iter()
                    .filter_map(|i| {
                        if i == *index {
                            return Some(sel.clone());
                        }
                        m.triangles.iter().zip(&m.tri_face).find_map(|(t, f)| {
                            if *f as usize != i {
                                return None;
                            }
                            let [a, b, c] = m.tri(t)?;
                            Some(Sel::Face { body: body.clone(), index: i, point: (a + b + c) / 3.0 })
                        })
                    })
                    .collect();
            }
        }
        _ => {}
    }
    vec![sel.clone()]
}
