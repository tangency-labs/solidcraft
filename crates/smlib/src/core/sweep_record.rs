//! Bodies with faces on procedural sweeps (the screw-swept walls of a thread or a coil, and
//! their offsets) cross the record boundary through solidcore-ops' conversion, which carries
//! each wall's exact definition and the edges on it. The flat arrays ([`super::convert`])
//! carry only a wall's net, so a body would lose its definitions at its first operation.

use std::sync::PoisonError;

use solidcore::topology::brep::p1;
use solidcore::topology::face;
use solidcore_record::brep as b;

use super::Body;
use crate::history::{Order, Piece};
use crate::{Error, Result};

/// Whether `record` has a face on a procedural sweep or an edge on one.
pub(crate) fn record_has_sweeps(record: &b::Brep) -> bool {
    !record.screw_owners.is_empty()
        || record.faces.iter().any(|f| matches!(f.surface, b::Surface::ProceduralSweep(_)))
        || record.edges.iter().any(|e| matches!(&e.curve, b::Curve::OnSurface { surface, .. } if matches!(**surface, b::Surface::ProceduralSweep(_))))
}

/// Whether a face of `body` lies on a procedural sweep.
pub(crate) fn has_sweeps(body: &Body) -> Result<bool> {
    super::with_body("procedural sweep faces", body, |_, brep| {
        for f in p1::faces(brep, None)? {
            if face::p2::surface(brep, f)?.is_some_and(|s| s.procedural_sweep().is_some()) {
                return Ok(true);
            }
        }
        Ok(false)
    })
}

fn ops_error(what: &str, e: solidcore_ops::Error) -> Error {
    Error::Solidcore(format!("{what}: {e}"))
}

/// The kernel body of `record` (one with sweeps), as solidcore-ops' conversion builds it.
pub(crate) fn from_record(record: &b::Brep, trusted: bool) -> Result<Body> {
    let (made, _) = solidcore_ops::Body::from_record(record, trusted).map_err(|e| ops_error("record to B-rep", e))?;
    Ok(Body::new(made.into_kernel()))
}

/// `body` (one with sweeps) as a record in canonical order, through solidcore-ops' conversion of
/// a copy of it, and where each of its faces and edges went (as the flat export's order says:
/// by their place in the body's own face and edge lists, which the copy keeps).
pub(crate) fn to_record(body: &Body) -> Result<(b::Brep, Order)> {
    let copy = super::transform::copy(body)?;
    let kernel = copy.0.into_inner().unwrap_or_else(PoisonError::into_inner);
    let ops = solidcore_ops::Body::from_kernel(kernel);
    let (record, order) = ops.to_record().map_err(|e| ops_error("B-rep to record", e))?;
    let pieces = order.pieces(&record).map_err(|e| Error::Solidcore(format!("B-rep to record: {e}")))?;
    let kernel = ops.into_kernel();
    let (record_faces, record_edges) = (order.record_faces(), order.record_edges());
    let unmapped = || Error::Solidcore("B-rep to record: an entity the record does not list".into());
    let faces = kernel.face_list().iter().map(|f| record_faces.get(f).map(|&i| i as usize)).collect::<Option<Vec<_>>>().ok_or_else(unmapped)?;
    let edge_list = kernel.edge_list();
    let edges = edge_list.iter().map(|e| record_edges.get(e).map(|&i| i as usize)).collect::<Option<Vec<_>>>().ok_or_else(unmapped)?;
    let pieces = pieces
        .into_iter()
        .map(|p| {
            let edge = edge_list.iter().position(|e| *e == p.kernel)?;
            Some(Piece { edge, canonical: p.edge as usize, faces: p.faces.into_iter().map(|f| f as usize).collect() })
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(unmapped)?;
    Ok((record, Order { faces, edges, pieces }))
}
