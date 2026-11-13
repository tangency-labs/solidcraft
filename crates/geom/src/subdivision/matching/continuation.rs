use super::*;

/// Refine the active cage with its G2 continuation, then retain the original
/// evaluator IDs. The inherited first exterior row must agree with the
/// continuation reconstructed from the fine core.
pub fn refine_g2(cage: &Cage, levels: usize) -> Result<Cage> {
    if levels > super::super::hierarchy::MAX_DEPTH {
        return Err(bad("G2 continuation refinement exceeds its depth budget"));
    }
    let mut core = cage.clone();
    for _ in 0..levels {
        let full = quadratic_extension(&core)?;
        let mut refined = core.refined(1)?;
        if full.faces.len() == core.faces.len() {
            core = refined;
            continue;
        }
        let full_topology = full.topology()?;
        let refined_full = full.refined(1)?;
        let refined_topology = refined_full.topology()?;
        // Subdivision emits old vertices, then edge midpoints, then face centers.
        // Explicitly look up edge IDs in the expanded topology.
        let mut map: Vec<_> = (0..core.points.len()).collect();
        for [a, b] in core.edges()? {
            let edge = *full_topology.lookup.get(&key(a, b)).ok_or_else(|| bad("missing continued edge"))?;
            map.push(full.points.len() + edge);
        }
        let face_start = full.points.len() + full_topology.edges.len();
        map.extend((0..core.faces.len()).map(|i| face_start + i));
        if map.len() != refined.points.len() {
            return Err(bad("invalid G2 refinement index transfer"));
        }
        let full_faces = refined_full.faces.get(..refined.faces.len()).ok_or_else(|| bad("missing continued child faces"))?;
        for (core_face, full_face) in refined.faces.iter().zip(full_faces) {
            if core_face.len() != full_face.len() || core_face.iter().zip(full_face).any(|(&v, w)| map.get(v) != Some(w)) {
                return Err(bad("G2 refinement would change a child face identity"));
            }
        }
        for (point, &id) in refined.points.iter_mut().zip(&map) {
            *point = refined_full.point(id)?;
        }
        let retained: BTreeSet<_> = map.iter().copied().collect();
        let loops = refined.boundary_loops()?;
        let [ring] = loops.as_slice() else { return Err(bad("expected one refined G2 opening")) };
        if ring.len() > 64 {
            return Err(bad("the refined G2 opening exceeds 64 controls"));
        }
        let baseline = quadratic_extension(&refined)?;
        let ghost = baseline.points.get(refined.points.len()..).ok_or_else(|| bad("missing refined continuation row"))?;
        if ghost.len() != ring.len() {
            return Err(bad("the refined opening has an incompatible continuation"));
        }
        for (&vertex, &point) in ring.iter().zip(ghost) {
            let id = *map.get(vertex).ok_or_else(|| bad("invalid refined boundary control"))?;
            let adjacent = refined_topology.incident.get(id).ok_or_else(|| bad("missing refined boundary edges"))?;
            let mut outside = None;
            for &edge in adjacent {
                let edge = refined_topology.edges.get(edge).ok_or_else(|| bad("missing refined continuation edge"))?;
                let neighbor = if edge.a == id { edge.b } else { edge.a };
                if !retained.contains(&neighbor) && outside.replace(neighbor).is_some() {
                    return Err(bad("the refined opening has more than one exterior neighbor"));
                }
            }
            let outside = outside.ok_or_else(|| bad("the refined opening has no exterior neighbor"))?;
            if refined_full.point(outside)?.dist(point) > 1e-8 {
                return Err(bad("the refined stencil cannot preserve this G2 continuation"));
            }
        }
        core = refined;
    }
    Ok(core)
}
