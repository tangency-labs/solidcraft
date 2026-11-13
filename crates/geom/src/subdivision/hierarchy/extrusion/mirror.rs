//! Connected selected caps share one offset wherever they share a control.
use super::*;

pub(super) fn face_offsets(
    cage: &Cage,
    cells: &BTreeMap<usize, Vec<usize>>,
    requested: &[usize],
    symmetry: &Symmetry,
    delta: Vec3,
) -> Result<(Vec3, Vec<Vec3>)> {
    let mut vertices_by_leaf: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    for (&leaf, faces) in cells {
        let vertices = vertices_by_leaf.entry(leaf).or_default();
        for &face in faces {
            vertices.extend(cage.faces.get(face).ok_or_else(|| bad("missing mirrored extrusion cell"))?.iter().copied());
        }
    }
    offsets(&vertices_by_leaf, requested, symmetry, delta)
}

pub(super) fn edge_offsets(edges: &[[usize; 2]], requested: &[usize], symmetry: &Symmetry, delta: Vec3) -> Result<(Vec3, Vec<Vec3>)> {
    let vertices = edges.iter().enumerate().map(|(id, edge)| (id, BTreeSet::from(*edge))).collect();
    offsets(&vertices, requested, symmetry, delta)
}

fn offsets(vertices_by_leaf: &BTreeMap<usize, BTreeSet<usize>>, requested: &[usize], symmetry: &Symmetry, delta: Vec3) -> Result<(Vec3, Vec<Vec3>)> {
    let mut leaves_by_vertex: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (&leaf, vertices) in vertices_by_leaf {
        for &v in vertices {
            leaves_by_vertex.entry(v).or_default().push(leaf);
        }
    }

    let mut leaf_component = BTreeMap::new();
    let mut vertex_component = BTreeMap::new();
    for &start in vertices_by_leaf.keys() {
        if leaf_component.contains_key(&start) {
            continue;
        }
        let mut pending = vec![start];
        while let Some(leaf) = pending.pop() {
            if leaf_component.insert(leaf, start).is_some() {
                continue;
            }
            let vertices = vertices_by_leaf.get(&leaf).ok_or_else(|| bad("missing mirrored extrusion leaf"))?;
            for &v in vertices {
                if vertex_component.insert(v, start).is_none() {
                    pending.extend(leaves_by_vertex.get(&v).ok_or_else(|| bad("missing mirrored extrusion fan"))?.iter().copied());
                }
            }
        }
    }

    let mut displacements = BTreeMap::new();
    for &leaf in requested {
        let &component = leaf_component.get(&leaf).ok_or_else(|| bad("missing selected extrusion component"))?;
        if displacements.contains_key(&component) {
            continue;
        }
        let &vertex = vertices_by_leaf.get(&component).and_then(BTreeSet::first).ok_or_else(|| bad("empty mirrored extrusion component"))?;
        let partner = symmetry.partners.get(vertex).ok_or_else(|| bad("missing extrusion mirror partner"))?;
        let &mirrored = vertex_component.get(partner).ok_or_else(|| bad("incomplete mirrored extrusion region"))?;
        if component == mirrored {
            let tangent = (delta + symmetry.reflect_vector(delta)) * 0.5;
            if tangent.len() < 1e-9 {
                return Err(bad("an extrusion joined at the mirror plane needs displacement along the plane"));
            }
            displacements.insert(component, tangent);
        } else {
            displacements.insert(component, delta);
            displacements.insert(mirrored, symmetry.reflect_vector(delta));
        }
    }
    let offsets = vertices_by_leaf
        .keys()
        .map(|leaf| {
            leaf_component
                .get(leaf)
                .and_then(|component| displacements.get(component))
                .copied()
                .ok_or_else(|| bad("missing mirrored extrusion displacement"))
        })
        .collect::<Result<Vec<_>>>()?;
    let first = *offsets.first().ok_or_else(|| bad("empty mirrored extrusion"))?;
    if offsets.iter().all(|offset| *offset == first) { Ok((first, Vec::new())) } else { Ok((first, offsets)) }
}

pub(super) fn apply_offsets(cage: &Cage, cells: &[Vec<usize>], offsets: &[Vec3], result: &mut Extrusion) -> Result<()> {
    let mut by_vertex = BTreeMap::new();
    for (faces, &delta) in cells.iter().zip(offsets) {
        for &id in faces {
            let face = cage.faces.get(id).ok_or_else(|| bad("missing extrusion source face"))?;
            assign_offset(&mut by_vertex, face, delta)?;
        }
    }
    move_caps(&by_vertex, result)
}

pub(super) fn apply_edge_offsets(edges: &[[usize; 2]], offsets: &[Vec3], result: &mut Extrusion) -> Result<()> {
    let mut by_vertex = BTreeMap::new();
    for (edge, &delta) in edges.iter().zip(offsets) {
        assign_offset(&mut by_vertex, edge, delta)?;
    }
    move_caps(&by_vertex, result)
}

fn assign_offset(by_vertex: &mut BTreeMap<usize, Vec3>, vertices: &[usize], delta: Vec3) -> Result<()> {
    if !delta.is_finite() || delta.len() < 1e-9 {
        return Err(bad("extrusion needs a finite nonzero displacement"));
    }
    for &v in vertices {
        if by_vertex.insert(v, delta).is_some_and(|prior| prior != delta) {
            return Err(bad("connected extrusion regions must agree at shared controls"));
        }
    }
    Ok(())
}

fn move_caps(by_vertex: &BTreeMap<usize, Vec3>, result: &mut Extrusion) -> Result<()> {
    for (&source, &cap) in &result.cap {
        let delta = by_vertex.get(&source).ok_or_else(|| bad("missing extrusion control displacement"))?;
        let point = stable_extrusion::displaced(&result.cage, cap, *delta)?;
        *result.cage.points.get_mut(cap).ok_or_else(|| bad("missing mirrored cap control"))? = point;
    }
    result.cage.validate()
}
