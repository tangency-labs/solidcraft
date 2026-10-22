//! Recording the exact history of the bodies operations build on the schema 4 record: for each
//! result face, what it is made from (operand faces it is the whole of or a piece of, profile
//! segments, caps, operand edges a blend replaces); for each result edge, the operand edges it is
//! or is a piece of, and otherwise the meeting of the two faces it separates. Indices are the
//! records' canonical ones, the result's and each operand's. Nothing is sampled: the building
//! code says what it built from what. Profile segments are numbered by [`crate::numbered`].

use std::collections::BTreeSet;

use solidcraft_geom::brep::Brep;

use crate::body::Body;
use crate::history::{Change, EdgeOrigin, EdgeProv, FaceOrigin, FaceProv, History, edge_faces};

/// A face made from all of profile segments `numbers` (in any order, repeats allowed).
pub(crate) fn profile<'a>(numbers: impl IntoIterator<Item = &'a usize>) -> FaceProv {
    let numbers: BTreeSet<usize> = numbers.into_iter().copied().collect();
    generated(numbers.into_iter().map(|segment| FaceOrigin::Profile { segment }).collect())
}

/// A face of operand `operand` and how this one relates to it.
pub(crate) fn from_face(operand: usize, face: usize, change: Change) -> FaceProv {
    FaceProv { origins: vec![FaceOrigin::Face { operand, face }], change }
}

/// Every face of the one operand (with `count` faces) but `except`, kept as it is, in order:
/// the faces a builder carries over unchanged when it rebuilds a body around one face.
pub(crate) fn kept_except(count: usize, except: usize) -> impl Iterator<Item = FaceProv> {
    (0..count).filter(move |i| *i != except).map(|i| from_face(0, i, Change::Kept))
}

/// A start (`end == false`) or end cap.
pub(crate) fn cap(end: bool) -> FaceProv {
    generated(vec![FaceOrigin::Cap { end }])
}

/// A face new to the operation, made from `origins`.
pub(crate) fn generated(origins: Vec<FaceOrigin>) -> FaceProv {
    FaceProv { origins, change: Change::Generated }
}

pub(crate) use solidcraft_geom::brep::history::mark_shared_split;

/// The origins of a result edge that is (a piece of, or the moved) edges `edges` of operand
/// `operand`.
pub(crate) fn pieces_of(operand: usize, edges: impl IntoIterator<Item = usize>) -> Vec<EdgeOrigin> {
    let edges: BTreeSet<usize> = edges.into_iter().collect();
    edges.into_iter().map(|edge| EdgeOrigin::Edge { operand, edge }).collect()
}

/// The history over `operands` operands of `result`, whose faces (in its face order) come from
/// `faces` (an operand face shared by several result faces is marked Split), and whose edges come
/// from `edges` (by result edge index) where the building code knows them: an operand edge it
/// kept or re-made ([`pieces_of`]). Every other edge separates two faces that met nowhere before,
/// so it comes from their meeting ([`meeting`]).
pub(crate) fn of_faces(result: &Brep, operands: usize, mut faces: Vec<FaceProv>, edges: &[Vec<EdgeOrigin>]) -> History {
    mark_shared_split(&mut faces);
    let edges = edge_faces(result)
        .into_iter()
        .enumerate()
        .map(|(i, between)| {
            let origins = match edges.get(i) {
                Some(known) if !known.is_empty() => known.clone(),
                _ => meeting(&between, &faces),
            };
            EdgeProv { faces: between, origins }
        })
        .collect();
    History { operands, faces, edges }
}

/// `body`, made from operands with these face and edge `counts`, with the history [`of_faces`]
/// gives (none where the faces could not be followed, `None`, or the history does not describe
/// the body: an operation whose history could not be recorded still gives its result).
pub(crate) fn attach(body: Body, counts: &[(usize, usize)], faces: Option<Vec<FaceProv>>, edges: &[Vec<EdgeOrigin>]) -> Body {
    let history = faces.map(|faces| of_faces(body.record(), counts.len(), faces, edges));
    body.with_checked_history(history, counts)
}

/// The face and edge counts of `bodies`, for [`traced`].
pub(crate) fn counts(bodies: &[&Body]) -> Vec<(usize, usize)> {
    bodies.iter().map(|b| (b.face_count(), b.edge_count())).collect()
}

/// `step`, made from `prev` (the step's one operand), with its history carried back to what
/// `prev` was made from: the operation's operands, with `counts` faces and edges. A multi-step
/// operation starts from its operand with [`crate::boolean_record::operand`] (or healed, see
/// [`crate::boolean_record::healed`]) and takes each step's result through this.
pub(crate) fn after(mut step: Body, prev: &Body, counts: &[(usize, usize)]) -> Body {
    let own = step.history.take().map(std::sync::Arc::unwrap_or_clone);
    let h = own.zip(prev.history()).and_then(|(h, inner)| crate::boolean_record::composed(h.substituted(0, inner)));
    step.with_checked_history(h, counts)
}

/// The origins of a new edge between the result faces `between` (whose provenance is in
/// `faces`): the meeting of the two faces (each side all of its face's origins, one or
/// several), when each comes from something and no origin is under both; else none (a seam,
/// an edge between pieces of one face, a face new to the operation).
pub(crate) fn meeting(between: &[usize], faces: &[FaceProv]) -> Vec<EdgeOrigin> {
    let side = |f: &usize| faces.get(*f).and_then(|p| crate::history::Side::new(p.origins.iter().copied()));
    match between {
        [f, g] => match (side(f), side(g)) {
            (Some(a), Some(b)) if a.origins().iter().all(|o| !b.origins().contains(o)) => vec![EdgeOrigin::meet(a, b)],
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

#[cfg(test)]
#[path = "record_tests.rs"]
mod tests;
