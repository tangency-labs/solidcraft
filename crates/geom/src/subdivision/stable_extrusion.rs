//! Quad extrusion without compacting control IDs or changing surviving face slots.
use super::{Cage, EdgeCrease, Result, VertexCrease, bad, key};
use crate::prelude::*;
use solidcore_record::Vec3;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wall {
    pub face: usize,
    pub base: [usize; 2],
    pub cap: [usize; 2],
}

pub struct Extrusion {
    pub cage: Cage,
    /// Old selected vertex to its translated cap control. Interior controls retain their ID.
    pub cap: BTreeMap<usize, usize>,
    /// Appended side faces, with directed base and translated cap edges.
    pub walls: Vec<Wall>,
}

fn edges(face: &[usize]) -> impl Iterator<Item = [usize; 2]> + '_ {
    face.iter().copied().zip(face.iter().copied().cycle().skip(1)).take(face.len()).map(|(a, b)| [a, b])
}

pub(super) fn displaced(cage: &Cage, vertex: usize, delta: Vec3) -> Result<Vec3> {
    let point = cage.point(vertex)? + delta;
    if !point.is_finite() || point.x.abs().max(point.y.abs()).max(point.z.abs()) > 100_000.0 {
        return Err(bad("extrusion coordinates must remain within 100000 mm"));
    }
    Ok(point)
}

fn qualified_quad_source(cage: &Cage, delta: Vec3, influence: bool) -> Result<()> {
    cage.validate()?;
    if cage.faces.iter().any(|face| face.len() != 4) {
        return Err(bad("stable extrusion requires an all-quad evaluator"));
    }
    if !delta.is_finite() || (!influence && delta.len() < 1e-9) {
        return Err(bad("extrusion needs a finite nonzero displacement"));
    }
    Ok(())
}

fn append_walls(
    cage: &mut Cage,
    boundary: impl IntoIterator<Item = [usize; 2]>,
    cap: &BTreeMap<usize, usize>,
    replace_faces: bool,
) -> Result<Vec<Wall>> {
    let mut walls = Vec::new();
    for [a, b] in boundary {
        let u = *cap.get(&a).ok_or_else(|| bad("missing first extrusion cap control"))?;
        let v = *cap.get(&b).ok_or_else(|| bad("missing second extrusion cap control"))?;
        let face = cage.faces.len();
        let vertices = if replace_faces { vec![a, b, v, u] } else { vec![b, a, u, v] };
        cage.faces.push(vertices);
        walls.push(Wall { face, base: [a, b], cap: [u, v] });
    }
    Ok(walls)
}

fn transfer_creases(source: &Cage, cage: &mut Cage, cap: &BTreeMap<usize, usize>) {
    let available: BTreeSet<_> = cage.faces.iter().flat_map(|f| edges(f)).map(|[a, b]| key(a, b)).collect();
    let mut edge_creases = BTreeMap::new();
    for c in &source.edge_creases {
        let [a, b] = c.edge;
        if available.contains(&key(a, b)) {
            edge_creases.insert(key(a, b), c.sharpness);
        }
        if let (Some(&a), Some(&b)) = (cap.get(&a), cap.get(&b))
            && available.contains(&key(a, b))
        {
            edge_creases.insert(key(a, b), c.sharpness);
        }
    }
    cage.edge_creases = edge_creases.into_iter().map(|((a, b), sharpness)| EdgeCrease { edge: [a, b], sharpness }).collect();
    cage.vertex_creases.extend(source.vertex_creases.iter().filter_map(|c| {
        let &vertex = cap.get(&c.vertex)?;
        (vertex != c.vertex).then_some(VertexCrease { vertex, sharpness: c.sharpness })
    }));
    cage.vertex_creases.sort_by_key(|c| c.vertex);
}

/// Move selected quad slots to the cap and append walls. The original control
/// at the foot of each wall retains its ID. Selected interior controls move in
/// place, so no source controls become unused or need compaction.
pub fn faces(source: &Cage, selected: &[usize], delta: Vec3) -> Result<Extrusion> {
    faces_impl(source, selected, delta, false)
}

/// Replay connectivity for a control influence without the authored translation.
pub(super) fn face_influence(source: &Cage, selected: &[usize]) -> Result<Extrusion> {
    faces_impl(source, selected, Vec3::ZERO, true)
}

fn faces_impl(source: &Cage, selected: &[usize], delta: Vec3, influence: bool) -> Result<Extrusion> {
    qualified_quad_source(source, delta, influence)?;
    let unique = source.selected_faces(selected)?;
    let mut boundary = BTreeMap::new();
    let mut vertices = BTreeSet::new();
    for &id in &unique {
        let face = source.faces.get(id).ok_or_else(|| bad("invalid extrusion face"))?;
        vertices.extend(face.iter().copied());
        for [a, b] in edges(face) {
            boundary.entry(key(a, b)).and_modify(|e| *e = None).or_insert(Some([a, b]));
        }
    }
    let rim: BTreeSet<_> = boundary.values().flatten().flat_map(|edge| edge.iter().copied()).collect();
    let shared: BTreeSet<_> = source.faces.iter().enumerate().filter(|(i, _)| !unique.contains(i)).flat_map(|(_, f)| f.iter().copied()).collect();
    if vertices.iter().any(|v| shared.contains(v) && !rim.contains(v)) {
        return Err(bad("extrusion selection branches at a control"));
    }
    let mut cage = source.clone();
    let mut cap = BTreeMap::new();
    for v in vertices {
        let point = displaced(source, v, delta)?;
        let destination = if rim.contains(&v) {
            let index = cage.points.len();
            cage.points.push(point);
            index
        } else {
            *cage.points.get_mut(v).ok_or_else(|| bad("missing interior extrusion control"))? = point;
            v
        };
        cap.insert(v, destination);
    }
    for &id in &unique {
        for v in cage.faces.get_mut(id).ok_or_else(|| bad("missing cap face slot"))? {
            *v = *cap.get(v).ok_or_else(|| bad("missing cap face control"))?;
        }
    }
    let walls = append_walls(&mut cage, boundary.into_values().flatten(), &cap, true)?;
    transfer_creases(source, &mut cage, &cap);
    cage.validate()?;
    Ok(Extrusion { cage, cap, walls })
}

/// Append one quad strip along true free evaluator edges. A logical trim
/// with a hidden neighbor still needs a separate stencil-aware edit.
pub fn boundary(source: &Cage, selected: &[[usize; 2]], delta: Vec3) -> Result<Extrusion> {
    boundary_impl(source, selected, delta, false)
}

pub(super) fn boundary_influence(source: &Cage, selected: &[[usize; 2]]) -> Result<Extrusion> {
    boundary_impl(source, selected, Vec3::ZERO, true)
}

fn boundary_impl(source: &Cage, selected: &[[usize; 2]], delta: Vec3, influence: bool) -> Result<Extrusion> {
    qualified_quad_source(source, delta, influence)?;
    if selected.is_empty() || selected.len() > source.points.len() {
        return Err(bad("select distinct free evaluator edges to extrude"));
    }
    let mut available = BTreeMap::new();
    for [a, b] in source.faces.iter().flat_map(|face| edges(face)) {
        available.entry(key(a, b)).and_modify(|e| *e = None).or_insert(Some([a, b]));
    }
    let unique: BTreeSet<_> = selected.iter().map(|&[a, b]| key(a, b)).collect();
    if unique.len() != selected.len() {
        return Err(bad("duplicate extrusion edge"));
    }
    let rim = unique
        .iter()
        .map(|e| available.get(e).copied().flatten().ok_or_else(|| bad("extrusion requires a true free evaluator edge")))
        .collect::<Result<Vec<_>>>()?;
    let mut cage = source.clone();
    let mut cap = BTreeMap::new();
    for v in rim.iter().flatten().copied().collect::<BTreeSet<_>>() {
        cap.insert(v, cage.points.len());
        cage.points.push(displaced(source, v, delta)?);
    }
    let walls = append_walls(&mut cage, rim, &cap, false)?;
    transfer_creases(source, &mut cage, &cap);
    cage.validate()?;
    Ok(Extrusion { cage, cap, walls })
}
