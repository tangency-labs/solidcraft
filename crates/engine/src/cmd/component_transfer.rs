//! Atomic timeline ownership changes, preserving the selected world geometry.
use super::*;
use solidcraft_doc::BodyName;
use std::collections::{BTreeMap, BTreeSet};

type UniqueFrames = BTreeMap<u64, Option<(Vec<u64>, Mat)>>;

fn unique_frames(s: &Session, cmd: &str) -> Result<UniqueFrames> {
    let doc = s.design_document();
    let mut children = BTreeMap::<u64, Vec<&Occurrence>>::new();
    for occurrence in &doc.occurrences {
        children.entry(occurrence.parent).or_default().push(occurrence);
    }
    let mut frames = BTreeMap::from([(0, Some((Vec::new(), solidcraft_doc::IDENTITY)))]);
    let mut stack = vec![(0, Vec::new(), solidcraft_doc::IDENTITY)];
    let mut work = 0usize;
    while let Some((component, path, matrix)) = stack.pop() {
        for occurrence in children.get(&component).into_iter().flatten() {
            work += 1;
            if path.len() >= 64 || work > 100_000 {
                return Err(bad(cmd, "transfer assembly exceeds 64 levels or 100000 placements"));
            }
            let mut child_path = path.clone();
            child_path.push(occurrence.id);
            let frame = solidcraft_doc::mat_mul(&matrix, &occurrence.transform);
            frames.entry(occurrence.component).and_modify(|entry| *entry = None).or_insert_with(|| Some((child_path.clone(), frame)));
            stack.push((occurrence.component, child_path, frame));
        }
    }
    Ok(frames)
}

fn body_names(s: &Session, p: &Value, cmd: &str, allow_boundary: bool) -> Result<Vec<String>> {
    if p.get("bodies").and_then(Value::as_array).is_none_or(|names| names.iter().any(|name| !name.is_string())) {
        return Err(bad(cmd, "bodies must be an array of body names"));
    }
    let names = string_list(p, "bodies");
    if names.is_empty() || names.len() > 1000 || names.iter().collect::<BTreeSet<_>>().len() != names.len() {
        return Err(bad(cmd, "bodies must list 1..1000 distinct existing definition bodies"));
    }
    if s.doc.marker.is_some() && !allow_boundary {
        return Err(bad(cmd, "finish timeline rollback before transferring bodies"));
    }
    let state = s.model.state();
    for name in &names {
        if state.body(name).is_none() {
            return Err(bad(cmd, format!("no body `{name}`")));
        }
    }
    Ok(names)
}

fn transfer(s: &mut Session, name: &str, to: u64, frames: &UniqueFrames, cmd: &str) -> Result<u64> {
    let state = s.model.state();
    let body = state.body(name).ok_or_else(|| bad(cmd, "body is missing"))?;
    let from = s.doc.model_body_component(body);
    let doc = s.design_document();
    let unique = |component| {
        frames.get(&component).and_then(Option::as_ref).ok_or_else(|| {
            bad(cmd, "body transfer requires one source and destination placement; shared instances cannot preserve a unique world pose")
        })
    };
    // A body of a component placed more than once goes by its name at the first placement,
    // where the name is its own: it moves from there.
    let first =
        frames.get(&from).is_some_and(Option::is_none).then(|| doc.first_path(from)).and_then(|path| Some((doc.path_transform(from, &path)?, path)));
    let first = first.map(|(frame, path)| (path, frame));
    let source = match &first {
        Some(placed) => placed,
        None => unique(from)?,
    };
    let destination = unique(to)?;
    let old = doc.model_body_offset(body).map(|offset| solidcraft_doc::mat_mul(&source.1, offset)).unwrap_or(source.1);
    let inv = solidcraft_doc::mat_inverse(&destination.1).ok_or_else(|| bad(cmd, "destination frame is not invertible"))?;
    let transform = solidcraft_doc::mat_mul(&inv, &old);
    if !solidcraft_doc::valid_transfer_matrix(&transform) {
        return Err(bad(cmd, "body transfer exceeds supported rigid coordinate range"));
    }
    let inherited = (!s.doc.appearances.bodies.contains_key(name)).then(|| s.doc.appearances.components.get(&from).cloned()).flatten();
    let paths: BTreeSet<u64> = source.0.iter().chain(&destination.0).copied().collect();
    let uncaptured = s.uncaptured_moves();
    let captures: Vec<_> = paths.into_iter().filter_map(|id| uncaptured.get(&id).map(|transform| (id, *transform))).collect();
    drop(uncaptured);
    if captures.iter().any(|(_, transform)| !solidcraft_doc::valid_transfer_matrix(transform)) {
        return Err(bad(cmd, "pending transfer position is not a supported rigid transform"));
    }
    drop(doc);
    for (id, transform) in captures {
        if let Some(occurrence) = s.doc_mut().occurrences.iter_mut().find(|occurrence| occurrence.id == id) {
            occurrence.transform = transform;
        }
        s.pending_moves.remove(&id);
    }
    if let Some(look) = inherited {
        s.doc_mut().appearances.bodies.insert(name.to_string(), look);
    }
    let id = s.doc_mut().add_feature(FeatureKind::BodyTransfer { bodies: vec![BodyName::from(name)], from, to, transform }, None)?;
    if let Some(feature) = s.doc_mut().feature_mut(id) {
        feature.component = to;
    }
    Ok(id)
}

fn finish(s: &mut Session, scratch: &mut Session, features: &[u64], cmd: &str) -> Result<()> {
    scratch.refresh();
    for id in features {
        if let Some(error) = scratch.model.result(*id).and_then(|result| result.error.as_ref()) {
            return Err(bad(cmd, error));
        }
    }
    s.accept_computed(std::mem::take(scratch));
    Ok(())
}

pub(super) fn from_bodies(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.from_bodies";
    let bodies = body_names(s, p, cmd, false)?;
    let mut scratch = s.scratch();
    let parent = s.active_component;
    let mut made = Vec::new();
    let mut targets = Vec::new();
    for name in &bodies {
        let component = scratch.doc_mut().add_component(Some(name), parent)?;
        targets.push(component);
    }
    let frames = unique_frames(&scratch, cmd)?;
    let mut features = Vec::new();
    for (name, component) in bodies.into_iter().zip(targets) {
        let feature = transfer(&mut scratch, &name, component, &frames, cmd)?;
        features.push(feature);
        made.push(json!({"component":component,"body":name,"feature":feature}));
    }
    finish(s, &mut scratch, &features, cmd)?;
    Ok(json!({"components":made}))
}

/// A placement path parameter (`component_path`: the target's placement to place by,
/// `from_path`: the source's placement to move from).
fn path_param(p: &Value, key: &str, cmd: &str) -> Result<Option<Vec<u64>>> {
    let Some(value) = p.get(key) else { return Ok(None) };
    value
        .as_array()
        .filter(|a| a.len() <= 64)
        .and_then(|a| a.iter().map(Value::as_u64).collect::<Option<Vec<_>>>())
        .map(Some)
        .ok_or_else(|| bad(cmd, format!("`{key}` must list 0..64 occurrence ids")))
}

pub(super) fn move_bodies(s: &mut Session, p: &Value) -> Result<Value> {
    move_at_boundary(s, p, false)
}

/// A solid constructor can attach its result immediately after the new feature,
/// including inside the scratch timeline used by Edit Feature.
pub(super) fn move_result(s: &mut Session, p: &Value) -> Result<Value> {
    move_at_boundary(s, p, true)
}

fn move_at_boundary(s: &mut Session, p: &Value, allow_boundary: bool) -> Result<Value> {
    let cmd = "component.move_bodies";
    let to = super::component(s, p, "component", cmd)?;
    let bodies = body_names(s, p, cmd, allow_boundary)?;
    let mut scratch = s.scratch();
    let mut frames = unique_frames(&scratch, cmd)?;
    // The target is placed by `component_path`, else its only placement, else the active one: a
    // component placed more than once gets the bodies in every instance, placed by that one.
    let given = path_param(p, "component_path", cmd)?;
    if given.is_some() || frames.get(&to).is_some_and(Option::is_none) {
        let placed = super::placing::placement_of(&scratch, &scratch.design_document(), to, given.as_deref(), "component_path", cmd)?;
        frames.insert(to, Some(placed));
    }
    // The bodies move from the placement named (`from_path`: one of several where their component
    // is placed), as that placement shows them.
    if let Some(path) = path_param(p, "from_path", cmd)? {
        let placed = scratch.design_document();
        let component = path
            .last()
            .and_then(|id| placed.occurrences.iter().find(|o| o.id == *id))
            .map(|o| o.component)
            .ok_or_else(|| bad(cmd, "`from_path` is no placement"))?;
        let frame = placed.path_transform(component, &path).ok_or_else(|| bad(cmd, "`from_path` is no placement"))?;
        drop(placed);
        frames.insert(component, Some((path, frame)));
    }
    let mut features = Vec::new();
    for name in &bodies {
        features.push(transfer(&mut scratch, name, to, &frames, cmd)?);
    }
    finish(s, &mut scratch, &features, cmd)?;
    Ok(json!({"bodies":bodies,"component":to,"features":features}))
}

#[cfg(test)]
#[path = "component_transfer_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
