//! Exact face and edge history: for each face and edge of an operation's result, what it came
//! from among the operation's operands, recorded by the code that built it (never sampled).
//!
//! Indices are positions in a body's record (canonical face and edge order, the order of
//! [`Body::edges`] and of a mesh's edges). Operand numbers are positions in
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

use crate::body::{Body, FaceSource};
use crate::{KernelError, Result};
use solidcraft_geom::brep::{Brep, part::Part};

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
        let edges = edge_faces(body.record())
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

/// The faces each edge separates, in the record's edge order: each face it bounds, once, in the
/// order of the edge's uses.
pub(crate) fn edge_faces(record: &Brep) -> Vec<Vec<usize>> {
    record
        .edges
        .iter()
        .map(|e| {
            let mut faces: Vec<usize> = Vec::with_capacity(e.uses.len());
            for u in &e.uses {
                if !faces.contains(&(u.face as usize)) {
                    faces.push(u.face as usize);
                }
            }
            faces
        })
        .collect()
}

/// The history of a part cut from, or a compound joined of, records numbered one after another
/// (`counts`: each operand's faces and edges, in order): every face and edge of the part is its
/// operand's own, kept.
pub(crate) fn of_part(part: &Part, counts: &[(usize, usize)]) -> History {
    // Operand and local index of an item numbered across the operands one after another.
    let locate = |mut i: usize, pick: fn(&(usize, usize)) -> usize| {
        for (operand, c) in counts.iter().enumerate() {
            if i < pick(c) {
                return (operand, i);
            }
            i -= pick(c);
        }
        (counts.len(), i)
    };
    let faces = part
        .faces
        .iter()
        .map(|&f| {
            let (operand, face) = locate(f as usize, |c| c.0);
            FaceProv { origins: vec![FaceOrigin::Face { operand, face }], change: Change::Kept }
        })
        .collect();
    let edges = part
        .edges
        .iter()
        .zip(edge_faces(&part.brep))
        .map(|(&e, faces)| {
            let (operand, edge) = locate(e as usize, |c| c.1);
            EdgeProv { faces, origins: vec![EdgeOrigin::Edge { operand, edge }] }
        })
        .collect();
    History { operands: counts.len(), faces, edges }
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
