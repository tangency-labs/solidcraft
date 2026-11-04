//! Exact tensor patches in a smooth regular quad neighborhood.
use super::*;

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
        let net = net.iter().map(|&v| cage.point(v.ok_or_else(|| bad("incomplete regular patch"))?)).collect::<Result<Vec<_>>>()?;
        // Uniform cubic B-spline span to cubic Bezier, applied in each direction.
        let weights = [[1.0, 4.0, 1.0, 0.0], [0.0, 4.0, 2.0, 0.0], [0.0, 2.0, 4.0, 0.0], [0.0, 1.0, 4.0, 1.0]];
        let mut points = Vec::with_capacity(16);
        for v in weights {
            for u in weights {
                let mut point = Vec3::ZERO;
                for j in 0..4 {
                    for i in 0..4 {
                        point += net[j * 4 + i] * (u[i] * v[j] / 36.0);
                    }
                }
                points.push(point);
            }
        }
        out.push(Some(
            NurbsSurface::new([3, 3], [vec![0., 0., 0., 0., 1., 1., 1., 1.], vec![0., 0., 0., 0., 1., 1., 1., 1.]], &points, &[1.0; 16])
                .map_err(|e| bad(&e.to_string()))?,
        ));
    }
    Ok(out)
}
