//! Exact face and edge history: for each face and edge of an operation's result, what it came
//! from among the operation's operands, recorded by the code that built it (never sampled).
//!
//! Indices are positions in a body's face order (`face_iter`) and edge order (`unique_edges`,
//! the order of [`Body::edges`] and of a tessellation's edges). Operand numbers are positions in
//! the operation's own operand list, which each operation documents. A body made by an
//! operation carries its history (see [`Body::history`]); [`History::substitute`] composes an
//! operation with one that made one of its operands.
//!
//! This is what persistent names and face attachments stand on. The document composes each
//! feature's histories into face names (`F5:side:l1`, `F7:blend:…`, `F4:top@F6.2`, see
//! `solidcraft_doc::history_names`); a name is exact only when every operation on the way
//! reported its history. A flat face with an exact name takes a durable sketch attachment by that
//! name (`FaceCertificate::Lineage`, docs/face-attachments.md), so an operation that records its
//! history here gives its faces attachments with no attachment code of its own. An operation
//! that can't should leave its result without one (`Body::without_history`), never guess.

pub use solidcraft_geom::brep::history::{Change, EdgeOrigin, EdgeProv, FaceOrigin, FaceProv, History, Side};

use crate::body::{Body, FaceSource, Solid};
use crate::{KernelError, Result};

impl From<solidcraft_geom::brep::history::Failed> for KernelError {
    fn from(e: solidcraft_geom::brep::history::Failed) -> KernelError {
        KernelError::Failed(e.to_string())
    }
}

/// What a [`History`] needs a kernel body for: the history types belong to
/// `solidcraft_geom::brep::history`, so a backend can report them without this crate.
pub trait BodyHistory {
    /// The history of an operation that keeps every face and edge of its one operand in place
    /// (a rigid move, a rebuild that keeps the topology's order).
    fn identity(body: &Body) -> History;
    /// Check that this history describes `result` made from `operands`: one entry per face and
    /// edge, and every origin within its operand's faces and edges.
    fn check(&self, result: &Body, operands: &[&Body]) -> Result<()>;
    /// The result's face sources, carried from its operands' through this history: a face
    /// kept, modified or split from one face keeps that face's source; a merged face keeps the
    /// source its origins share, if they share one; a generated face has none.
    fn face_sources(&self, operands: &[&Body]) -> Vec<Option<FaceSource>>;
}

impl BodyHistory for History {
    /// The history of an operation that keeps every face and edge of its one operand in place
    /// (a rigid move, a rebuild that keeps the topology's order).
    fn identity(body: &Body) -> History {
        let faces =
            (0..body.face_count()).map(|face| FaceProv { origins: vec![FaceOrigin::Face { operand: 0, face }], change: Change::Kept }).collect();
        let edges = edge_faces(&body.solid)
            .into_iter()
            .enumerate()
            .map(|(edge, faces)| EdgeProv { faces, origins: vec![EdgeOrigin::Edge { operand: 0, edge }] })
            .collect();
        History { operands: 1, faces, edges }
    }

    /// Check that this history describes `result` made from `operands`: one entry per face and
    /// edge, and every origin within its operand's faces and edges.
    fn check(&self, result: &Body, operands: &[&Body]) -> Result<()> {
        let counts: Vec<(usize, usize)> = operands.iter().map(|b| (b.face_count(), b.edge_count())).collect();
        Ok(self.check_counts((result.face_count(), result.edge_count()), &counts)?)
    }

    /// The result's face sources, carried from its operands' through this history: a face
    /// kept, modified or split from one face keeps that face's source; a merged face keeps the
    /// source its origins share, if they share one; a generated face has none.
    fn face_sources(&self, operands: &[&Body]) -> Vec<Option<FaceSource>> {
        let source = |o: &FaceOrigin| match *o {
            FaceOrigin::Face { operand, face } => operands.get(operand).and_then(|b| b.face_sources()).and_then(|s| s.get(face)).cloned().flatten(),
            _ => None,
        };
        self.faces
            .iter()
            .map(|f| {
                if f.change == Change::Generated {
                    return None;
                }
                let mut sources = f.origins.iter().map(source);
                let first = sources.next().flatten()?;
                sources.all(|s| s.as_ref() == Some(&first)).then_some(first)
            })
            .collect()
    }
}

/// The faces each edge separates, in the body's edge order.
pub(crate) fn edge_faces(solid: &Solid) -> Vec<Vec<usize>> {
    crate::body::edge_order(solid.face_iter()).1
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
