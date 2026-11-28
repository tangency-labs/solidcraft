//! Strict planar cap triangulation for reconstructed mesh parts.
//! Keeps the original boundary indices and constrained triangulation face order.
//!
//! Deviations: no upstream counterpart. No recursion.

use spade::handles::FixedVertexHandle;
use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation};
use std::collections::HashMap;

type Cdt = ConstrainedDelaunayTriangulation<Point2<f64>>;

/// Fill closed loops without adding boundary or interior vertices; crossing constraints are refused.
pub(super) fn triangulate(
    boundary: &[[f64; 2]],
    loops: &[std::ops::Range<usize>],
) -> Option<Vec<[u32; 3]>> {
    if boundary.iter().flatten().any(|x| !x.is_finite())
        || loops
            .iter()
            .any(|r| r.start > r.end || r.end > boundary.len() || r.len() < 3)
    {
        return None;
    }
    let mut cdt = Cdt::new();
    let mut index: HashMap<FixedVertexHandle, u32> = HashMap::new();
    let mut handles: Vec<FixedVertexHandle> = Vec::new();
    for (i, &[x, y]) in boundary.iter().enumerate() {
        let h = cdt
            .insert(spade::mitigate_underflow(Point2::new(x, y)))
            .ok()?;
        index.entry(h).or_insert(u32::try_from(i).ok()?);
        handles.push(h);
    }
    let mut sides: Vec<(FixedVertexHandle, FixedVertexHandle)> = Vec::new();
    for range in loops {
        let ring = handles.get(range.clone())?;
        sides.extend(
            ring.iter()
                .copied()
                .zip(ring.iter().copied().cycle().skip(1)),
        );
    }
    let slits = slits(&sides);
    for (a, b) in sides {
        if a == b || cdt.exists_constraint(a, b) {
            continue;
        }
        if !cdt.can_add_constraint(a, b) {
            return None;
        }
        cdt.add_constraint(a, b);
    }
    let region = region(&cdt, &slits);
    let mut triangles = Vec::new();
    for face in cdt.inner_faces() {
        if !region.get(face.index()).copied().unwrap_or(false) {
            continue;
        }
        let mut t = [0u32; 3];
        for (slot, v) in t.iter_mut().zip(face.vertices()) {
            *slot = *index.get(&v.fix())?;
        }
        triangles.push(t);
    }
    Some(triangles)
}

/// Sides run along an even number of times do not separate the inside from the outside.
fn slits(
    sides: &[(FixedVertexHandle, FixedVertexHandle)],
) -> std::collections::HashSet<(FixedVertexHandle, FixedVertexHandle)> {
    let mut runs: HashMap<(FixedVertexHandle, FixedVertexHandle), usize> = HashMap::new();
    for &(a, b) in sides {
        if a != b {
            *runs
                .entry(if a.index() < b.index() {
                    (a, b)
                } else {
                    (b, a)
                })
                .or_insert(0) += 1;
        }
    }
    runs.into_iter()
        .filter(|(_, n)| n % 2 == 0)
        .map(|(e, _)| e)
        .collect()
}

/// Per face (by index): inside the region the constraint loops bound. A flood fill from the
/// outer face that flips parity each time it crosses a constraint edge (holes are out), but for
/// the sides in `slits` (run along both ways, the region on both sides), linear in the number of
/// triangles.
fn region(
    cdt: &Cdt,
    slits: &std::collections::HashSet<(FixedVertexHandle, FixedVertexHandle)>,
) -> Vec<bool> {
    use spade::handles::{FixedFaceHandle, InnerTag};
    let counts = |e: spade::handles::DirectedEdgeHandle<'_, _, _, _, _>| {
        let [a, b] = e.vertices().map(|v| v.fix());
        e.is_constraint_edge()
            && !slits.contains(&if a.index() < b.index() {
                (a, b)
            } else {
                (b, a)
            })
    };
    let mut parity: Vec<Option<bool>> = vec![None; cdt.num_all_faces()];
    let mut todo: Vec<(FixedFaceHandle<InnerTag>, bool)> = Vec::new();
    for e in cdt.convex_hull() {
        // Hull edges have the outer face on one side; the other side is an inner face.
        for side in [e, e.rev()] {
            if let Some(inner) = side.face().as_inner() {
                todo.push((inner.fix(), counts(e)));
            }
        }
    }
    while let Some((h, odd)) = todo.pop() {
        match parity.get_mut(h.index()) {
            Some(slot @ None) => *slot = Some(odd),
            _ => continue,
        }
        for e in cdt.face(h).adjacent_edges() {
            if let Some(next) = e.rev().face().as_inner() {
                todo.push((next.fix(), odd ^ counts(e)));
            }
        }
    }
    parity.into_iter().map(|p| p.unwrap_or(false)).collect()
}
