//! Where something pasted or moved into a component goes: placed by one placement of that
//! component, so it sits where its original is in the world. A component placed more than once is
//! one definition: what goes in it goes in every instance, placed by the one named (`parent_path`,
//! `component_path`), else its only placement, else the active one, and in the same place in the
//! others.

use super::*;

/// The `parent_path` parameter: the placement of the parent to place by (`[]`: the design).
pub(super) fn parent_path(p: &Value, cmd: &str) -> Result<Option<Vec<u64>>> {
    let Some(value) = p.get("parent_path") else { return Ok(None) };
    value
        .as_array()
        .filter(|a| a.len() <= 64)
        .and_then(|a| a.iter().map(Value::as_u64).collect::<Option<Vec<_>>>())
        .map(Some)
        .ok_or_else(|| bad(cmd, "`parent_path` must list 0..64 occurrence ids"))
}

/// The transform a copy of the placement at `path` takes in `parent` to sit where the placement
/// is in the world, then moved by `translate` (in the world). The parent is placed by
/// [`placement_of`]: in its other instances the copy sits in the same place within them.
pub(super) fn in_place(s: &Session, path: &[u64], parent: u64, parent_path: Option<&[u64]>, translate: Vec3, cmd: &str) -> Result<Mat> {
    let doc = s.placed_document();
    let component =
        path.last().and_then(|id| doc.occurrences.iter().find(|o| o.id == *id)).map(|o| o.component).ok_or_else(|| bad(cmd, "no placement"))?;
    let world = doc.path_transform(component, path).ok_or_else(|| bad(cmd, "that placement is gone"))?;
    let (_, frame) = placement_of(s, &doc, parent, parent_path, "parent_path", cmd)?;
    let inverse = solidcraft_doc::mat_inverse(&frame).ok_or_else(|| bad(cmd, "the parent's frame is not invertible"))?;
    Ok(solidcraft_doc::mat_mul(&inverse, &solidcraft_doc::mat_mul(&translation(translate), &world)))
}

/// The placement of `component` a move or paste is placed by (the design is the empty path): its
/// path and world frame. `path` when given (it must lead to `component`), else its only
/// placement, else the active placement when that is one of it. `key` names the parameter that
/// gives `path`, for the error when none of them settles it.
pub(in crate::cmd) fn placement_of(
    s: &Session,
    doc: &solidcraft_doc::Document,
    component: u64,
    path: Option<&[u64]>,
    key: &str,
    cmd: &str,
) -> Result<(Vec<u64>, Mat)> {
    let name = || doc.components.iter().find(|c| c.id == component).map(|c| c.name.clone()).unwrap_or_else(|| "that component".into());
    if let Some(path) = path {
        let frame = doc.path_transform(component, path).ok_or_else(|| bad(cmd, format!("that path is not a placement of {}", name())))?;
        return Ok((path.to_vec(), frame));
    }
    let all: Vec<(u64, Vec<u64>, Mat)> = doc.placements().into_iter().filter(|(c, _, _)| *c == component).collect();
    match all.as_slice() {
        [(_, path, frame)] => Ok((path.clone(), *frame)),
        [] => Err(bad(cmd, "that component is not placed in the design")),
        _ => all
            .iter()
            .find(|(_, path, _)| s.active_occurrence_path.as_deref() == Some(path.as_slice()))
            .map(|(_, path, frame)| (path.clone(), *frame))
            .ok_or_else(|| bad(cmd, format!("{} is placed more than once: activate the placement to work in, or give it as `{key}`", name()))),
    }
}
