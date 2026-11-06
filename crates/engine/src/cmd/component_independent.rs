//! Detach one physical placement, copying only the containing shared definitions it needs.
use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::params::bad;
use crate::{Result, Sel, Session};

use super::component_clone::{Scope, copy};

const CMD: &str = "component.make_independent";

pub(super) fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let id = p
        .get("occurrence")
        .and_then(Value::as_u64)
        .or_else(|| p.get("path")?.as_array()?.last()?.as_u64())
        .ok_or_else(|| bad(CMD, "supply an occurrence or its exact path"))?;
    let mut path = super::component::occurrence_path(s, p, id, CMD)?;
    let mut counts = BTreeMap::<u64, usize>::new();
    let records: BTreeMap<_, _> = s.doc.occurrences.iter().map(|o| (o.id, o)).collect();
    for o in &s.doc.occurrences {
        *counts.entry(o.component).or_default() += 1;
    }
    let start = path
        .iter()
        .position(|id| records.get(id).is_some_and(|o| counts.get(&o.component).is_some_and(|n| *n > 1)))
        .ok_or_else(|| bad(CMD, "this placement is already independent"))?;
    let mut scratch = s.scratch();
    let mut notices = Vec::new();
    let mut target = 0;
    for depth in start..path.len() {
        let id = *path.get(depth).ok_or_else(|| bad(CMD, "missing placement"))?;
        let old = scratch.doc.occurrences.iter().find(|o| o.id == id).cloned().ok_or_else(|| bad(CMD, "missing placement"))?;
        let prefix = path.get(..=depth).ok_or_else(|| bad(CMD, "missing placement path"))?.to_vec();
        // Capture view masks only when present. Geometry is never expanded to remap them.
        let masks = masks(&scratch)?;
        let hidden = scratch.visibility.hidden_occurrences.clone();
        let scope = if depth + 1 == path.len() { Scope::DetachTree } else { Scope::DetachDefinition };
        let (component, transform, parent) = (old.component, old.transform, old.parent);
        let result = copy(&mut scratch, component, Some(old), transform, parent, &json!({"occurrence":id,"path":prefix}), scope)?;
        target = result.component;
        for item in path.iter_mut().skip(depth + 1) {
            if let Some(mapped) = result.occurrences.get(item) {
                *item = *mapped;
            }
        }
        let state = scratch.model.state();
        let names = solidcraft_doc::WorldNames::new(&scratch.doc, &state);
        let mut renamed = BTreeMap::new();
        for mask in masks {
            let selected = mask.path.starts_with(&prefix);
            let mut placed = mask.path;
            if selected {
                remap_path(&mut placed, &result.occurrences);
            }
            let name = if selected { result.bodies.get(&mask.source).unwrap_or(&mask.source) } else { &mask.source };
            let component = if selected { result.components.get(&mask.component).copied().unwrap_or(mask.component) } else { mask.component };
            let new = names.name(name, component, &placed);
            renamed.insert(mask.world, new);
        }
        for entries in [&mut scratch.visibility.hidden_bodies, &mut scratch.visibility.locked_bodies] {
            for entry in entries {
                if let Some(new) = renamed.get(entry) {
                    entry.clone_from(new);
                }
            }
        }
        scratch.visibility.hidden_occurrences = hidden;
        for hidden in &mut scratch.visibility.hidden_occurrences {
            if hidden.starts_with(&prefix) {
                remap_path(hidden, &result.occurrences);
            }
        }
        for key in &mut scratch.visibility.hidden_folders {
            let Some((scope, folder)) = key.rsplit_once('/') else { continue };
            let Some(ids) = scope.strip_prefix("o:") else { continue };
            let Some(mut placed) = ids.split('/').map(|id| id.parse::<u64>().ok()).collect::<Option<Vec<_>>>() else { continue };
            if placed.starts_with(&prefix) {
                remap_path(&mut placed, &result.occurrences);
                *key = format!("{}/{folder}", Session::folder_scope(&placed));
            }
        }
        notices.extend(result.notices);
    }
    scratch.active_component = target;
    scratch.active_occurrence_path = Some(path.clone());
    scratch.selection = vec![Sel::Occurrence { path: path.clone() }];
    let visibility = std::mem::take(&mut scratch.visibility);
    s.accept_computed(scratch);
    s.visibility = visibility;
    Ok(json!({"component":target,"occurrence":path.last(),"path":path,"notices":notices}))
}

fn remap_path(path: &mut [u64], ids: &BTreeMap<u64, u64>) {
    for id in path {
        if let Some(new) = ids.get(id) {
            *id = *new;
        }
    }
}

struct Mask {
    world: String,
    source: String,
    component: u64,
    path: Vec<u64>,
}

fn masks(s: &Session) -> Result<Vec<Mask>> {
    if s.visibility.hidden_bodies.is_empty() && s.visibility.locked_bodies.is_empty() {
        return Ok(Vec::new());
    }
    let wanted: std::collections::BTreeSet<_> = s.visibility.hidden_bodies.iter().chain(&s.visibility.locked_bodies).map(String::as_str).collect();
    // A warm view identifies the source definitions without computing new world geometry.
    let warm = s.cached_world_state();
    let relevant: Option<std::collections::BTreeSet<_>> =
        warm.as_ref().map(|state| state.bodies.iter().filter(|b| wanted.contains(b.name.as_str())).map(|b| b.definition_name()).collect());
    let state = s.model.state();
    let names = solidcraft_doc::WorldNames::new(&s.doc, &state);
    let owners = s.doc.model_body_components(&state);
    let mut bodies = BTreeMap::<u64, Vec<_>>::new();
    for (body, owner) in state.bodies.iter().zip(owners) {
        if relevant.as_ref().is_none_or(|names| names.contains(body.name.as_str())) {
            bodies.entry(owner).or_default().push(body);
        }
    }
    let mut children = BTreeMap::<u64, Vec<_>>::new();
    for o in &s.doc.occurrences {
        children.entry(o.parent).or_default().push(o);
    }
    let mut output = Vec::new();
    let mut record = |component, path: &[u64]| {
        for body in bodies.get(&component).into_iter().flatten() {
            let world = names.name(&body.name, component, path);
            if wanted.contains(world.as_str()) {
                output.push(Mask { world, source: body.name.clone(), component, path: path.to_vec() });
            }
        }
    };
    let mut path = Vec::with_capacity(64);
    let mut stack = vec![(0u64, 0usize)];
    let mut visits = 0usize;
    record(0, &path);
    while let Some((component, next)) = stack.last_mut() {
        let Some(o) = children.get(component).and_then(|children| children.get(*next)) else {
            stack.pop();
            path.pop();
            continue;
        };
        *next += 1;
        visits += 1;
        if visits > 100_000 || path.len() >= 64 {
            return Err(bad(CMD, "view masks exceed the placement budget"));
        }
        path.push(o.id);
        record(o.component, &path);
        stack.push((o.component, 0));
    }
    Ok(output)
}
