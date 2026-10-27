//! Recording the exact history of a boolean's result, whichever path made it (see
//! [`crate::ops::boolean`]): for each result face, the operand faces it is a region of; for each
//! result edge, the operand edges it lies along or the operand faces whose meeting it is. The
//! operands are `[a, b]`: the target, then the tool. Paths compose the histories of the steps
//! they take ([`History::over`], [`History::substitute`]); a path that cannot leaves the result
//! without history.

use std::collections::{HashMap, HashSet};

use solidcraft_geom::Vec3;
use truck_modeling as mt;

use crate::Result;
use crate::body::{Body, Solid, edge_order};
use crate::history::{Change, EdgeOrigin, FaceOrigin, FaceProv, History};
use crate::record;

/// A composition step's history, or none (logged) where the lineage can't be expressed.
pub(crate) fn composed(h: Result<History>) -> Option<History> {
    h.map_err(|e| log::debug!("no history: {e}")).ok()
}

/// `h`, the history of an operation over one operand, as over `operands` with that one at
/// `operand`.
pub(crate) fn placed(h: Option<History>, operand: usize, operands: usize) -> Option<History> {
    composed(h?.renumbered(&[Some(operand)], operands))
}

/// The history of `body` as operand `operand` of `operands`: every face and edge kept.
pub(crate) fn kept(body: &Body, operand: usize, operands: usize) -> Option<History> {
    placed(Some(History::identity(body)), operand, operands)
}

/// The history of `result`, made from operands of which operand `k` was made by the operation
/// `inner` describes, with `inner`'s operands in `k`'s place.
pub(crate) fn through(result: &Body, k: usize, inner: Option<&History>) -> Option<History> {
    composed(result.history()?.clone().substituted(k, inner?))
}

/// Operand `operand` (with `counts` giving every operand's faces and edges) as the result, with
/// the history that says so.
pub(crate) fn operand(body: &Body, operand: usize, counts: &[(usize, usize)]) -> Body {
    body.clone().with_checked_history(kept(body, operand, counts.len()), counts)
}

/// The histories of `parts`, each made of shells of `whole` (its lumps), over `[whole]`: their
/// faces and edges are the whole's own (by identity).
pub(crate) fn parts_of(parts: &[Body], whole: &Body) -> Vec<Option<History>> {
    let (faces, edges) = (record::face_index(&whole.solid), record::edge_origins(&whole.solid, 0));
    parts
        .iter()
        .map(|part| {
            let faces = part.solid.face_iter().map(|f| Some(record::from_face(0, *faces.get(&f.id())?, Change::Kept))).collect::<Option<Vec<_>>>()?;
            Some(History { operands: 1, faces, edges: record::edge_provs(part, &edges)? })
        })
        .collect()
}

/// A body with its history over the operation's operands (`None`: not known).
pub(crate) type Traced = (Body, Option<History>);

/// The history over the operation's operands of `result`, made from `x` and `y` (in that order)
/// whose histories over them are given.
pub(crate) fn of_pair(result: &Body, x: &Option<History>, y: &Option<History>) -> Option<History> {
    composed(result.history()?.clone().over(&[x.as_ref()?, y.as_ref()?]))
}

/// Bodies apart from each other, each with its history over the operation's operands (with
/// `counts` faces and edges), as one body of several pieces whose history is theirs side by side.
pub(crate) fn join(pieces: Vec<Traced>, counts: &[(usize, usize)]) -> Result<Option<Body>> {
    if pieces.len() < 2 {
        return Ok(pieces.into_iter().next().map(|(b, h)| b.with_checked_history(h, counts)));
    }
    // The pieces' shells in order: their faces in order, their edges themselves.
    let mut faces = Some(Vec::new());
    let mut edges = HashMap::new();
    for (p, h) in &pieces {
        let (Some(h), Some(all)) = (h, faces.as_mut()) else {
            faces = None;
            break;
        };
        all.extend(h.faces.iter().cloned());
        edges.extend(record::edges_by_identity(p, h));
    }
    if let Some(faces) = faces.as_mut() {
        record::mark_shared_split(faces);
    }
    let joined = crate::ops::join_pieces(pieces.into_iter().map(|(p, _)| p).collect())?;
    Ok(joined.map(|body| record::traced(body, counts, faces, Some(&edges))))
}

/// A truck boolean's history in our terms, in the order of the solid it made: each face a
/// region of one operand face (Kept when that very face), each edge's origins.
pub(crate) type Shapeops = (Vec<FaceProv>, Vec<Vec<EdgeOrigin>>);

/// A truck boolean's history in our terms ([`Shapeops`]). `tool` is the
/// tool as passed (after a cut turned it inside out, which keeps its faces' order but walks each
/// face's edges the other way, so its edge order changes) and `tool_edges` its edges by identity
/// in its own order (before turning).
pub(crate) fn shapeops(h: &truck_shapeops::History, tool: &Solid, tool_edges: Option<&HashMap<mt::EdgeID, Vec<EdgeOrigin>>>) -> Option<Shapeops> {
    let face = |f: truck_shapeops::FaceIndex| FaceOrigin::Face { operand: f.operand, face: f.face };
    let faces =
        h.faces.iter().map(|o| record::from_face(o.source.operand, o.source.face, if o.kept { Change::Kept } else { Change::Modified })).collect();
    let passed: Vec<mt::EdgeID> = edge_order(tool.face_iter()).0.iter().map(|e| e.id()).collect();
    let edges = h
        .edges
        .iter()
        .map(|o| match *o {
            truck_shapeops::EdgeOrigin::Edge { operand: 1, edge } if tool_edges.is_some() => {
                tool_edges.and_then(|t| t.get(passed.get(edge)?)).cloned()
            }
            truck_shapeops::EdgeOrigin::Edge { operand, edge } => Some(vec![EdgeOrigin::Edge { operand, edge }]),
            truck_shapeops::EdgeOrigin::Meet { a, b } => Some(vec![EdgeOrigin::meet(face(a), face(b))]),
        })
        .collect::<Option<Vec<_>>>()?;
    Some((faces, edges))
}

/// Samples along each edge when checking that its curve evaluates.
const EVALUATION_SAMPLES: usize = 32;

/// Whether every edge of `solid` evaluates along its whole range. An intersection curve projects
/// its leader's point onto both surfaces, which fails to converge where a boolean made the edge
/// from an approximation that strayed (#1252). Such a result is no solid to keep: the mesher,
/// healing and measuring all sample the edge and stumble over the gap.
pub(crate) fn edges_evaluate(solid: &Solid) -> bool {
    use truck_meshalgo::prelude::TrySubs;
    use truck_modeling::BoundedCurve;
    let mut checked = HashSet::new();
    solid.boundaries().iter().flat_map(|shell| shell.edge_iter()).filter(|edge| checked.insert(edge.id())).all(|edge| {
        let curve = edge.curve();
        let (t0, t1) = curve.range_tuple();
        (0..=EVALUATION_SAMPLES).all(|k| {
            let t = t0 + (t1 - t0) * k as f64 / EVALUATION_SAMPLES as f64;
            curve.try_subs(t).is_some_and(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite())
        })
    })
}

/// The body made of `solid` healed (keeping faces apart along `keep`), with the history whose
/// faces and edges (in `solid`'s orders) are given before healing, carried through it.
/// Operands with `counts` faces and edges. A result with an edge that does not evaluate is
/// refused (see [`edges_evaluate`]).
pub(crate) fn heal_build(
    solid: Solid,
    size: f64,
    keep: &[Vec<Vec3>],
    faces: Option<Vec<FaceProv>>,
    edges: Option<Vec<Vec<EdgeOrigin>>>,
    counts: &[(usize, usize)],
) -> Result<Body> {
    let (healed, map) = crate::heal::heal_traced(solid, size, keep);
    if !edges_evaluate(&healed) {
        return Err(crate::KernelError::Failed("an edge of the result does not evaluate".into()));
    }
    let carried = faces.zip(edges).zip(map).and_then(|((f, e), m)| m.carry(&f, &e));
    let (faces, edges) = carried.unzip();
    record::build(healed, counts, faces, edges.as_ref())
}

/// The body truck's result `s` makes, healed (see [`heal_build`]), with its history `h`.
pub(crate) fn truck_result(s: Solid, h: Option<Shapeops>, size: f64, keep: &[Vec<Vec3>], counts: &[(usize, usize)]) -> Result<Body> {
    let (faces, edges) = h.unzip();
    heal_build(s, size, keep, faces, edges, counts)
}

/// `body` healed (see [`heal_build`]), its history carried through: `solid` is `body`'s solid
/// or a copy of it (`Body::deep_copy`, which keeps its order).
pub(crate) fn heal_body(body: &Body, solid: Solid, size: f64, keep: &[Vec<Vec3>], counts: &[(usize, usize)]) -> Result<Body> {
    let (faces, edges) = body.history().map(|h| (h.faces.clone(), h.edges.iter().map(|e| e.origins.clone()).collect())).unzip();
    heal_build(solid, size, keep, faces, edges, counts)
}

/// A prism boolean's result `body` with the coplanar pieces its slabs leave merged (nothing
/// else healed: its edges are exact as they are), its history carried through, and what else it carries kept: each face's
/// construction source where every piece merged into it had that same one, its Split Face
/// lines and its regularization protection.
pub(crate) fn heal_prism_result(body: &Body, size: f64, keep: &[Vec<Vec3>], counts: &[(usize, usize)]) -> Result<Body> {
    let (faces, edges): (Option<Vec<FaceProv>>, Option<Vec<Vec<EdgeOrigin>>>) =
        body.history().map(|h| (h.faces.clone(), h.edges.iter().map(|e| e.origins.clone()).collect())).unzip();
    let (healed, map) = crate::heal::merge_faces_traced((*body.solid).clone(), size, keep);
    let sources = match (body.face_sources(), &map) {
        (Some(sources), Some(m)) => Some(
            m.faces
                .iter()
                .map(|under| {
                    let mut each = under.iter().map(|i| sources.get(*i).cloned().flatten());
                    let first = each.next().flatten()?;
                    each.all(|s| s.as_ref() == Some(&first)).then_some(first)
                })
                .collect::<Vec<_>>(),
        ),
        _ => None,
    };
    let carried = faces.zip(edges).zip(map).and_then(|((f, e), m)| m.carry(&f, &e));
    let (faces, edges) = carried.unzip();
    let mut out = record::build(healed, counts, faces, edges.as_ref())?;
    out = out.with_face_sources(sources.map(std::sync::Arc::new)).inherit_regularization_protection(body);
    out.splits = body.splits.clone();
    Ok(out)
}

/// `body` healed as the local modifiers take it (coplanar neighbours as one face, as after a
/// join; faces split on purpose kept apart), with its history over `[body]`.
pub(crate) fn healed(body: &Body) -> Result<Body> {
    if let Some(healed) = PREPARED.with(|slot| slot.try_borrow().ok()?.as_ref().filter(|entry| std::ptr::eq(entry.input, body))?.healed.clone()) {
        return Ok(healed);
    }
    let counts = record::counts(&[body]);
    let healed = heal_body(&operand(body, 0, &counts), body.deep_copy(), body.size(), body.split_keep(), &counts)?;
    PREPARED.with(|slot| {
        if let Ok(mut slot) = slot.try_borrow_mut()
            && let Some(entry) = slot.as_mut().filter(|entry| std::ptr::eq(entry.input, body))
        {
            entry.healed = Some(healed.clone());
        }
    });
    Ok(healed)
}

/// The exact input reference of one synchronous modifier invocation. The pointer
/// is only compared, never dereferenced: `with_prepared` keeps its borrow alive
/// until the scope restores the prior entry. A clone with different metadata is
/// a different reference and cannot accidentally use another input's history.
struct PreparedInput {
    input: *const Body,
    healed: Option<Body>,
}

thread_local! {
    static PREPARED: std::cell::RefCell<Option<PreparedInput>> = const { std::cell::RefCell::new(None) };
}

/// Reuse healing of this unchanged input during a fillet's construction and
/// acceptance checks. Nested operations and escaped panics restore the outer
/// scope; no cached body survives the operation that borrowed its input.
pub(crate) fn with_prepared<T>(input: &Body, run: impl FnOnce() -> T) -> T {
    struct Restore(Option<PreparedInput>);
    impl Drop for Restore {
        fn drop(&mut self) {
            PREPARED.with(|slot| {
                if let Ok(mut slot) = slot.try_borrow_mut() {
                    *slot = self.0.take();
                }
            });
        }
    }
    let previous = PREPARED
        .with(|slot| slot.try_borrow_mut().ok().map(|mut slot| slot.replace(PreparedInput { input: std::ptr::from_ref(input), healed: None })));
    let restore = previous.map(Restore);
    let result = run();
    drop(restore);
    result
}

/// `body` copied as it is, unhealed (`Body::deep_copy`, which keeps its face and edge order),
/// with its history over `[body]`: every face and edge kept.
pub(crate) fn copied(body: &Body) -> Result<Body> {
    let copy = Body::new(body.deep_copy())?;
    Ok(operand(&copy, 0, &record::counts(&[body])))
}

/// The faces and edges of `a`, then of the tool `b` turned inside out as a void (`inner`, a
/// copy of `b`, with `inner_edges` its edges by identity before turning): the history of
/// `a` with `b`'s inside taken out where `b` lies wholly within it.
pub(crate) fn void(
    a: &Body,
    inner_edges: HashMap<mt::EdgeID, Vec<EdgeOrigin>>,
    b_faces: usize,
) -> (Vec<FaceProv>, HashMap<mt::EdgeID, Vec<EdgeOrigin>>) {
    let faces = (0..a.face_count())
        .map(|i| record::from_face(0, i, Change::Kept))
        .chain((0..b_faces).map(|i| record::from_face(1, i, Change::Kept)))
        .collect();
    let mut edges = record::edge_origins(&a.solid, 0);
    edges.extend(inner_edges);
    (faces, edges)
}

#[cfg(test)]
#[path = "boolean_history_tests.rs"]
pub(crate) mod tests;
