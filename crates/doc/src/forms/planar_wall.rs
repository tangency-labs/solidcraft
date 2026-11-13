//! Planar Form walls assembled from the exact native sheet and its boundary curves.
use crate::kernel::history::{BodyHistory, Change, EdgeOrigin, EdgeProv, FaceOrigin, FaceProv, History};
use crate::{DocError, Result, kernel};
use solidcore_record::Vec3;
use solidcore_record::brep::{Curve, Shell};
use std::collections::BTreeSet;

const MAX_EDGES: usize = 4096;
const MAX_CONTROLS: usize = 262_144;

fn invalid(why: impl Into<String>) -> DocError {
    DocError::Invalid(why.into())
}

fn planar_normal(body: &kernel::Body) -> Option<Vec3> {
    let record = body.record();
    if !body.is_surface()
        || body.is_mesh()
        || record.edges.len() > MAX_EDGES
        || !matches!(record.shells.as_slice(), [Shell::Faces { wires, .. }] if wires.is_empty())
    {
        return None;
    }
    let mut spline_boundary = false;
    let mut free = 0;
    for edge in &record.edges {
        match edge.uses.len() {
            1 if edge.vertices[0] != edge.vertices[1] => {
                free += 1;
                match edge.parameters() {
                    Curve::Line { .. } => {}
                    Curve::Spline { .. } | Curve::RationalSpline { .. } => spline_boundary = true,
                    _ => return None,
                }
            }
            2 => {}
            _ => return None,
        }
    }
    if !spline_boundary || free < 3 {
        return None;
    }
    let planes = (0..body.face_count()).map(|index| body.planar_face(index)).collect::<Option<Vec<_>>>()?;
    kernel::form_curves::coplanar_normal(planes.into_iter())
}

/// A direct wall for coplanar fitted Forms with spline boundaries. Ordinary
/// analytic and curved-surface Thicken keep their existing construction.
pub fn can_build_planar_wall(body: &kernel::Body) -> bool {
    planar_normal(body).is_some()
}

/// The two cap operands describe opposite sides of the same source body.
/// Keeping them distinct in the history gives both offsets stable names.
pub(crate) fn build(body: &kernel::Body, thickness: f64, symmetric: bool) -> Result<(kernel::Body, History)> {
    let normal = planar_normal(body).ok_or_else(|| invalid("Thicken requires the saved Form's coplanar spline boundary"))?;
    let (offsets, tolerance) =
        kernel::form_curves::wall_offsets(normal, thickness, symmetric).ok_or_else(|| invalid("Thicken requires a finite nonzero thickness"))?;
    let [start, end] = offsets;
    let first = kernel::transform(body, start, Vec3::ZERO, Vec3::Z, 0.0)?;
    let second = kernel::transform(body, end, Vec3::ZERO, Vec3::Z, 0.0)?;
    let mut remaining = MAX_CONTROLS;
    let mut side_edges = Vec::new();
    let mut sides = Vec::new();
    for (index, _) in body.record().edges.iter().enumerate().filter(|(_, edge)| edge.uses.len() == 1) {
        let edge = kernel::edge_geometry(body, &[index])?.into_iter().next().ok_or_else(|| invalid("missing planar Form boundary"))?;
        sides.push(edge.planar_wall_strip(offsets, &mut remaining)?);
        side_edges.push(index);
    }
    let parts: Vec<_> = [&first, &second].into_iter().chain(&sides).collect();
    let wall = kernel::stitch(&parts, tolerance)?;
    if wall.is_surface()
        || wall.shell_count() != 1
        || wall.record().material_regions().len() != 1
        || wall.face_count() != parts.iter().map(|part| part.face_count()).sum::<usize>()
        || !kernel::validity(&wall).ok()
    {
        return Err(invalid("Thicken could not join the planar Form's complete wall into one valid solid"));
    }
    let history = lineage(&wall, body, &parts, &side_edges, symmetric)?;
    Ok((wall, history))
}

fn lineage(wall: &kernel::Body, source: &kernel::Body, parts: &[&kernel::Body], side_edges: &[usize], symmetric: bool) -> Result<History> {
    let missing = || invalid("Thicken did not retain the planar Form's exact face and edge history");
    let raw = wall.history().ok_or_else(missing)?;
    raw.check(wall, parts)?;
    let mut seen = BTreeSet::new();
    let faces = raw
        .faces
        .iter()
        .map(|provenance| {
            let [FaceOrigin::Face { operand, face }] = *provenance.origins.as_slice() else { return Err(missing()) };
            if !seen.insert((operand, face)) {
                return Err(missing());
            }
            let (origin, change) = match operand {
                0 if !symmetric => (FaceOrigin::Face { operand, face }, Change::Kept),
                0 | 1 => (FaceOrigin::Offset { operand, face }, Change::Generated),
                side if face == 0 => (FaceOrigin::Edge { operand: 0, edge: *side_edges.get(side - 2).ok_or_else(missing)? }, Change::Generated),
                _ => return Err(missing()),
            };
            Ok(FaceProv { origins: vec![origin], change })
        })
        .collect::<Result<Vec<_>>>()?;
    let edges = raw
        .edges
        .iter()
        .map(|edge| {
            let mut kept = edge.origins.iter().filter_map(|origin| match *origin {
                EdgeOrigin::Edge { operand: 0, edge } if !symmetric => Some(edge),
                _ => None,
            });
            let origin = if let Some(edge) = kept.next() {
                if kept.next().is_some() {
                    return Err(missing());
                }
                EdgeOrigin::Edge { operand: 0, edge }
            } else {
                let [a, b] = *edge.faces.as_slice() else { return Err(missing()) };
                let face = |index: usize| faces.get(index).and_then(|p| p.origins.first()).copied().ok_or_else(missing);
                EdgeOrigin::meet(face(a)?, face(b)?)
            };
            Ok(EdgeProv { faces: edge.faces.clone(), origins: vec![origin] })
        })
        .collect::<Result<Vec<_>>>()?;
    let history = History { operands: 2, faces, edges };
    history.check(wall, &[source, source])?;
    Ok(history)
}
