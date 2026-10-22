//! Composing the exact history of a multi-step operation (a boolean through several steps, a
//! local modifier on a healed body): each step's history over its own operands is placed among
//! the operation's operands ([`placed`], [`kept`]) and substituted into the next
//! ([`through`], [`of_pair`], [`crate::record::after`]). The operands of a boolean are `[a, b]`:
//! the target, then the tool. A step that cannot report its history leaves the result without
//! one.

use crate::Result;
use crate::body::Body;
use crate::history::{BodyHistory, History};

/// A composition step's history, or none (logged) where the lineage can't be expressed.
pub(crate) fn composed<E: std::fmt::Display>(h: std::result::Result<History, E>) -> Option<History> {
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

/// A body with its history over the operation's operands (`None`: not known).
pub(crate) type Traced = (Body, Option<History>);

/// The history over the operation's operands of `result`, made from `x` and `y` (in that order)
/// whose histories over them are given.
pub(crate) fn of_pair(result: &Body, x: &Option<History>, y: &Option<History>) -> Option<History> {
    composed(result.history()?.clone().over(&[x.as_ref()?, y.as_ref()?]))
}

/// `body` healed as the local modifiers take it (faces on one surface as one face, as after a
/// join; faces split on purpose kept apart), with its history over `[body]`
/// ([`crate::heal::heal`]), each face keeping the construction source its faces shared and the
/// body its regularization protection.
pub(crate) fn healed(body: &Body) -> Result<Body> {
    let healed = crate::heal::heal(body, body.split_keep())?;
    let sources = healed.history().map(|h| std::sync::Arc::new(h.face_sources(&[body])));
    Ok(healed.with_face_sources(sources).inherit_regularization_protection(body))
}

#[cfg(test)]
#[path = "boolean_history_tests.rs"]
pub(crate) mod tests;
