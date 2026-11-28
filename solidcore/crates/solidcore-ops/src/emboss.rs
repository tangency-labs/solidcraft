//! Where an emboss on a freeform face offsets it (#21347): the pieces of the face a box cut
//! leaves under the profiles, and whether one of them reaches a pole of its surface, where the
//! kernel never certifies an offset.

pub use solidcore::emboss::{patch_outline, picked_pieces};

use solidcore::eval::SurfaceEval;
use solidcore_record::brep::{Brep, Loop};
use solidcore_record::Vec3;

use solidcore::query::api_queries::face_internal_point;

use crate::body::Body;
use crate::error::{Error, Result};
use crate::query::Query;
use crate::session::with_body;

/// The faces of `cut`, a cut of `body`, that are pieces of `body`'s face `face`: those of its
/// kind of surface with a point inside them on it, within the two bodies' tolerance, each with
/// that point.
///
/// # Errors
/// [`Error::Argument`] for a face `body` does not have; the record conversion's refusals and the
/// kernel's, finding a point inside a face of `cut`.
pub fn pieces_of(body: &Brep, face: usize, cut: &Brep) -> Result<Vec<(usize, [f64; 3])>> {
    let kind = std::mem::discriminant(&body.faces.get(face).ok_or(Error::Argument)?.surface);
    let alike: Vec<usize> = (0..cut.faces.len())
        .filter(|&f| {
            cut.faces
                .get(f)
                .is_some_and(|c| std::mem::discriminant(&c.surface) == kind)
        })
        .collect();
    let (cut_body, order) = Body::from_record(cut, true)?;
    let ids = alike
        .iter()
        .map(|&f| crate::order::face(&order, f))
        .collect::<Result<Vec<_>>>()?;
    let inside = with_body("emboss pieces", &cut_body, |k, b| {
        ids.iter()
            .map(|&f| face_internal_point(k, b, f).map(|p| [p.x, p.y, p.z]))
            .collect::<solidcore::Result<Vec<_>>>()
            .map(Ok::<_, Error>)
    })??;
    let query = Query::new(body);
    let accept = body.tolerance.max(cut.tolerance);
    Ok(alike
        .into_iter()
        .zip(inside)
        .filter(|(_, [x, y, z])| query.face_contains(face, Vec3::new(*x, *y, *z), accept))
        .collect())
}

/// For each of `faces` of `record`, the group of them it is joined to edge to edge: the first
/// of `faces` in its group.
#[must_use]
pub fn joined_groups(record: &Brep, faces: &[usize]) -> Vec<usize> {
    let mut group: Vec<usize> = (0..faces.len()).collect();
    let mut changed = true;
    while changed {
        changed = false;
        for edge in &record.edges {
            let on: Vec<usize> = edge
                .uses
                .iter()
                .filter_map(|u| faces.iter().position(|&f| f == u.face as usize))
                .collect();
            let Some(least) = on.iter().filter_map(|&i| group.get(i).copied()).min() else {
                continue;
            };
            for &i in &on {
                if let Some(g) = group.get_mut(i) {
                    if *g != least {
                        *g = least;
                        changed = true;
                    }
                }
            }
        }
    }
    group
}

/// Whether any of `faces` of `record` has a vertex at a pole of its surface, within the
/// vertex's tolerance. Only a pole at a vertex is found (a revolved face carries its pole as
/// one); a pole inside a face is not.
///
/// # Errors
/// [`Error::Argument`] for a face the record does not have; [`Error::Unsupported`] for a
/// surface the kernel cannot evaluate there.
pub fn reaches_pole(record: &Brep, faces: &[usize]) -> Result<bool> {
    let query = Query::new(record);
    for &f in faces {
        let face = record.faces.get(f).ok_or(Error::Argument)?;
        let surface = solidcore_convert::surface(face.parameters())
            .map_err(|_| Error::Unsupported("an emboss face whose surface does not convert"))?;
        let eval = SurfaceEval::new(&surface)
            .map_err(|_| Error::Unsupported("an emboss face whose surface cannot be evaluated"))?;
        let mut vertices = Vec::new();
        for l in &face.loops {
            match l {
                Loop::Edges { edges } => {
                    for e in edges {
                        let edge = record.edges.get(e.edge as usize).ok_or(Error::Argument)?;
                        vertices.extend(edge.vertices);
                    }
                }
                Loop::Vertex { vertex } => vertices.push(*vertex),
            }
        }
        for v in vertices {
            let vertex = record.vertices.get(v as usize).ok_or(Error::Argument)?;
            let [x, y, z] = vertex.point;
            let reach = vertex.tolerance.max(record.tolerance);
            let uv = query
                .face_uv(f, Vec3::new(x, y, z), reach)
                .ok_or(Error::Unsupported(
                    "an emboss face whose vertex has no parameters on it",
                ))?;
            let pole = eval
                .pole_within(uv, reach)
                .map_err(|_| Error::Unsupported("an emboss face whose pole test fails"))?;
            if pole.iter().any(|p| *p) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
