//! Exact tensor patches in a smooth regular quad neighborhood.
use super::*;

/// Reuse exact original patches and fit the remaining roots on a compact
/// cage when the full next-level reference grid would exceed its budget.
pub(super) fn fit_neighborhood(cage: &Cage, tolerance: f64) -> Result<Option<Fit>> {
    let Some(level) = (2..=6).find(|&level| grid_samples_exceed_budget(cage.faces.len(), (1 << (level + 1)) + 1)) else { return Ok(None) };
    let exact = patches(cage)?;
    let needed: BTreeSet<_> = cage.faces.iter().zip(&exact).filter(|(_, patch)| patch.is_none()).flat_map(|(face, _)| face.iter().copied()).collect();
    if needed.is_empty() {
        return Ok(None);
    }
    // Include every face incident to a target corner. One subdivision step
    // then preserves the entire star of each child corner: an old corner
    // uses its complete fan, an edge point uses both incident parent faces,
    // and a face point uses its parent. New faces around other old corners
    // cannot touch these child corners. Induction retains the target limit
    // grids even if the exterior of the cropped cage acquires a boundary.
    let roots: Vec<_> = cage.faces.iter().enumerate().filter_map(|(id, face)| face.iter().any(|v| needed.contains(v)).then_some(id)).collect();
    if roots.len() == cage.faces.len() {
        return Ok(None);
    }
    let mut neighborhood = cage.clone();
    neighborhood.faces = roots.iter().map(|&id| cage.faces[id].clone()).collect();
    let local = neighborhood.finish_edit()?.fit_root_patches_at(tolerance, level)?;
    if local.patches.len() != roots.len() {
        return Err(bad("compact CAD fit lost its source charts"));
    }
    let mut fitted: BTreeMap<_, _> = roots.into_iter().zip(local.patches).collect();
    // The common symmetric stations preserve reversed fitted seams. Beside
    // an exact patch its boundary is already cubic, so interpolation at the
    // same stations reproduces that same boundary.
    let patches: Vec<_> = exact
        .into_iter()
        .enumerate()
        .map(|(root, patch)| patch.or_else(|| fitted.remove(&root)).ok_or_else(|| bad("missing compact CAD source patch")))
        .collect::<Result<_>>()?;
    Ok(Some(Fit {
        root_basis: None,
        patch_roots: (0..patches.len()).collect(),
        charts: (0..patches.len()).map(PatchChart::whole).collect(),
        patches,
        sampled_deviation: local.sampled_deviation,
        subdivision_level: local.subdivision_level,
    }))
}

pub(super) fn patches(cage: &Cage) -> Result<Vec<Option<NurbsSurface>>> {
    let topology = cage.topology()?;
    let sharp_edges: BTreeSet<_> = cage.edge_creases.iter().filter(|e| e.sharpness > 0.0).map(|e| super::super::key(e.edge[0], e.edge[1])).collect();
    let sharp_vertices: BTreeSet<_> = cage.vertex_creases.iter().filter(|v| v.sharpness > 0.0).map(|v| v.vertex).collect();
    let mut out = Vec::with_capacity(cage.faces.len());
    for (root, face) in cage.faces.iter().enumerate() {
        if face.len() != 4
            || face.iter().any(|&v| {
                sharp_vertices.contains(&v)
                    || topology.incident.get(v).is_none_or(|edges| {
                        edges.len() != 4
                            || edges.iter().any(|&i| {
                                topology.edges.get(i).is_none_or(|e| e.faces.len() != 2 || sharp_edges.contains(&super::super::key(e.a, e.b)))
                            })
                    })
            })
        {
            out.push(None);
            continue;
        }
        let mut net = [None; 16];
        for (&v, (x, y)) in face.iter().zip([(1, 1), (2, 1), (2, 2), (1, 2)]) {
            net[y * 4 + x] = Some(v);
        }
        let mut neighbors = Vec::with_capacity(4);
        let mut regular = true;
        for edge in 0..4 {
            let (a, b) = (face[edge], face[(edge + 1) % 4]);
            let adjacent = topology
                .lookup
                .get(&super::super::key(a, b))
                .and_then(|&id| topology.edges.get(id))
                .and_then(|e| e.faces.iter().copied().find(|&f| f != root));
            let Some(other) = adjacent.and_then(|id| cage.faces.get(id).map(|f| (id, f))) else {
                regular = false;
                break;
            };
            if other.1.len() != 4 {
                regular = false;
                break;
            }
            let Some(at) = (0..4).find(|&i| other.1.get(i) == Some(&b) && other.1.get((i + 1) % 4) == Some(&a)) else {
                regular = false;
                break;
            };
            let (p, q) = (other.1[(at + 2) % 4], other.1[(at + 3) % 4]);
            let (i, j) = match edge {
                0 => (1, 2),
                1 => (7, 11),
                2 => (14, 13),
                _ => (8, 4),
            };
            net[i] = Some(p);
            net[j] = Some(q);
            neighbors.push(other.0);
        }
        if !regular {
            out.push(None);
            continue;
        }
        if neighbors.iter().copied().collect::<BTreeSet<_>>().len() != 4 {
            out.push(None);
            continue;
        }
        for (corner, &vertex) in face.iter().enumerate() {
            let faces: BTreeSet<_> = topology
                .incident
                .get(vertex)
                .into_iter()
                .flatten()
                .filter_map(|&i| topology.edges.get(i))
                .flat_map(|e| e.faces.iter().copied())
                .collect();
            let across = faces.into_iter().find(|&f| f != root && !neighbors.contains(&f));
            let Some(diagonal) = across.and_then(|i| cage.faces.get(i)) else {
                regular = false;
                break;
            };
            if diagonal.len() != 4 {
                regular = false;
                break;
            }
            let Some(at) = diagonal.iter().position(|&v| v == vertex) else {
                regular = false;
                break;
            };
            let index = [0, 3, 15, 12].get(corner).copied().ok_or_else(|| bad("invalid regular corner"))?;
            net[index] = Some(diagonal[(at + 2) % 4]);
        }
        if !regular {
            out.push(None);
            continue;
        }
        let mut controls = [Vec3::ZERO; 16];
        for (point, index) in controls.iter_mut().zip(net) {
            *point = cage.point(index.ok_or_else(|| bad("incomplete regular patch"))?)?;
        }
        let points = solidcore_ops::form_subdivision::regular_bezier(controls);
        out.push(Some(
            NurbsSurface::new([3, 3], [vec![0., 0., 0., 0., 1., 1., 1., 1.], vec![0., 0., 0., 0., 1., 1., 1., 1.]], &points, &[1.0; 16])
                .map_err(|e| bad(&e.to_string()))?,
        ));
    }
    Ok(out)
}
