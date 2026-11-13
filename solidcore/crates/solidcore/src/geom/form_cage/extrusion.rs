//! Stable quad extrusion with explicit cap controls and oriented side faces.

use super::{bad, finite, CageGeometry, MAX_FACES, MAX_POINTS};
use crate::{Result, Vec3};
use std::collections::{BTreeMap, BTreeSet};

/// The appended wall face and its directed source and cap edges.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Wall {
    /// Face index in the result.
    pub face: usize,
    /// Directed edge on the source cage.
    pub base: [usize; 2],
    /// Directed edge on the cap.
    pub cap: [usize; 2],
}

/// Result of a cage extrusion, retaining source identity.
pub struct Extrusion {
    /// New controls and face cycles.
    pub cage: CageGeometry,
    /// Selected source controls mapped to their cap controls.
    pub cap: BTreeMap<usize, usize>,
    /// Appended side faces in result order.
    pub walls: Vec<Wall>,
}

/// The part of a cage to extrude.
#[derive(Clone, Copy)]
pub enum Selection<'a> {
    /// Move these face slots onto the cap.
    Faces(&'a [usize]),
    /// Add a strip to these true free edges.
    Boundary(&'a [[usize; 2]]),
}

/// Translate one control within the editable Form coordinate budget.
pub fn displaced(point: Vec3, delta: Vec3) -> Result<Vec3> {
    let point = point + delta;
    if !finite(point) || point.x.abs().max(point.y.abs()).max(point.z.abs()) > 100_000.0 {
        return Err(bad("extrusion coordinates must remain within 100000 mm"));
    }
    Ok(point)
}

fn key([a, b]: [usize; 2]) -> (usize, usize) {
    (a.min(b), a.max(b))
}

fn edges(face: &[usize]) -> impl Iterator<Item = [usize; 2]> + '_ {
    face.iter()
        .copied()
        .zip(face.iter().copied().cycle().skip(1))
        .take(face.len())
        .map(|(a, b)| [a, b])
}

fn boundary_of<'a>(
    faces: impl Iterator<Item = &'a Vec<usize>>,
) -> BTreeMap<(usize, usize), Option<[usize; 2]>> {
    let mut boundary = BTreeMap::new();
    for edge in faces.flat_map(|face| edges(face)) {
        boundary
            .entry(key(edge))
            .and_modify(|e| *e = None)
            .or_insert(Some(edge));
    }
    boundary
}

/// Extrude an oriented manifold quad cage without compacting control IDs. A zero
/// displacement is accepted only for replaying the topology of an influence.
pub fn extrude(
    points: &[Vec3],
    faces: &[Vec<usize>],
    selected: Selection<'_>,
    delta: Vec3,
    influence: bool,
) -> Result<Extrusion> {
    if points.is_empty() || faces.is_empty() || points.len() > MAX_POINTS || faces.len() > MAX_FACES
    {
        return Err(bad("editable cage budget exceeded"));
    }
    if faces
        .iter()
        .any(|face| face.len() != 4 || face.iter().any(|&v| v >= points.len()))
    {
        return Err(bad("stable extrusion requires an all-quad evaluator"));
    }
    if !finite(delta) || (!influence && delta.length() < 1e-9) {
        return Err(bad("extrusion needs a finite nonzero displacement"));
    }
    let (selected_faces, boundary, vertices) = match selected {
        Selection::Faces(ids) => {
            if ids.is_empty() || ids.len() > faces.len() {
                return Err(bad("select distinct existing faces"));
            }
            let selected: BTreeSet<_> = ids.iter().copied().collect();
            if selected.len() != ids.len() {
                return Err(bad("select distinct existing faces"));
            }
            let region = selected
                .iter()
                .map(|&id| {
                    faces
                        .get(id)
                        .ok_or_else(|| bad("select distinct existing faces"))
                })
                .collect::<Result<Vec<_>>>()?;
            let vertices = region
                .iter()
                .flat_map(|f| f.iter().copied())
                .collect::<BTreeSet<_>>();
            let boundary = boundary_of(region.into_iter())
                .into_values()
                .flatten()
                .collect::<Vec<_>>();
            (selected, boundary, vertices)
        }
        Selection::Boundary(ids) => {
            if ids.is_empty() || ids.len() > points.len() {
                return Err(bad("select distinct free evaluator edges to extrude"));
            }
            let unique: BTreeSet<_> = ids.iter().copied().map(key).collect();
            if unique.len() != ids.len() {
                return Err(bad("duplicate extrusion edge"));
            }
            let available = boundary_of(faces.iter());
            let boundary = unique
                .iter()
                .map(|e| {
                    available
                        .get(e)
                        .copied()
                        .flatten()
                        .ok_or_else(|| bad("extrusion requires a true free evaluator edge"))
                })
                .collect::<Result<Vec<_>>>()?;
            let vertices = boundary.iter().flatten().copied().collect();
            (BTreeSet::new(), boundary, vertices)
        }
    };
    let rim: BTreeSet<_> = boundary.iter().flatten().copied().collect();
    if points.len() + rim.len() > MAX_POINTS || faces.len() + boundary.len() > MAX_FACES {
        return Err(bad("editable cage budget exceeded"));
    }
    if !selected_faces.is_empty()
        && faces
            .iter()
            .enumerate()
            .filter(|(i, _)| !selected_faces.contains(i))
            .flat_map(|(_, f)| f.iter())
            .any(|v| vertices.contains(v) && !rim.contains(v))
    {
        return Err(bad("extrusion selection branches at a control"));
    }
    let mut cage = CageGeometry {
        points: points.to_vec(),
        faces: faces.to_vec(),
    };
    let mut cap = BTreeMap::new();
    for v in vertices {
        let point = displaced(*points.get(v).ok_or_else(|| bad("invalid vertex"))?, delta)?;
        let destination = if rim.contains(&v) {
            let index = cage.points.len();
            cage.points.push(point);
            index
        } else {
            *cage
                .points
                .get_mut(v)
                .ok_or_else(|| bad("missing interior extrusion control"))? = point;
            v
        };
        cap.insert(v, destination);
    }
    for id in selected_faces {
        for v in cage
            .faces
            .get_mut(id)
            .ok_or_else(|| bad("missing cap face slot"))?
        {
            *v = *cap.get(v).ok_or_else(|| bad("missing cap face control"))?;
        }
    }
    let mut walls = Vec::new();
    for [a, b] in boundary {
        let u = *cap
            .get(&a)
            .ok_or_else(|| bad("missing first extrusion cap control"))?;
        let v = *cap
            .get(&b)
            .ok_or_else(|| bad("missing second extrusion cap control"))?;
        let face = cage.faces.len();
        let vertices = match selected {
            Selection::Faces(_) => vec![a, b, v, u],
            Selection::Boundary(_) => vec![b, a, u, v],
        };
        cage.faces.push(vertices);
        walls.push(Wall {
            face,
            base: [a, b],
            cap: [u, v],
        });
    }
    Ok(Extrusion { cage, cap, walls })
}
