//! Quad extrusion without compacting control IDs or changing surviving face slots.
use super::{Cage, EdgeCrease, Result, VertexCrease, bad, key};
use solidcore_ops::form_cage::{self as kernel, ExtrusionSelection};
use solidcore_record::Vec3;
use std::collections::{BTreeMap, BTreeSet};

pub use solidcore_ops::form_cage::ExtrusionWall as Wall;

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
    kernel::displaced(cage.point(vertex)?, delta).map_err(|error| bad(&error.to_string()))
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
    extrude(source, ExtrusionSelection::Faces(selected), delta, false)
}

/// Replay connectivity for a control influence without the authored translation.
pub(super) fn face_influence(source: &Cage, selected: &[usize]) -> Result<Extrusion> {
    extrude(source, ExtrusionSelection::Faces(selected), Vec3::ZERO, true)
}

/// Append one quad strip along true free evaluator edges. A logical trim
/// with a hidden neighbor still needs a separate stencil-aware edit.
pub fn boundary(source: &Cage, selected: &[[usize; 2]], delta: Vec3) -> Result<Extrusion> {
    extrude(source, ExtrusionSelection::Boundary(selected), delta, false)
}

pub(super) fn boundary_influence(source: &Cage, selected: &[[usize; 2]]) -> Result<Extrusion> {
    extrude(source, ExtrusionSelection::Boundary(selected), Vec3::ZERO, true)
}

fn extrude(source: &Cage, selection: ExtrusionSelection<'_>, delta: Vec3, influence: bool) -> Result<Extrusion> {
    source.validate()?;
    let result = kernel::extrude(&source.points, &source.faces, selection, delta, influence).map_err(|error| bad(&error.to_string()))?;
    let mut cage =
        Cage { points: result.cage.points, faces: result.cage.faces, edge_creases: Vec::new(), vertex_creases: source.vertex_creases.clone() };
    transfer_creases(source, &mut cage, &result.cap);
    cage.validate()?;
    Ok(Extrusion { cage, cap: result.cap, walls: result.walls })
}
