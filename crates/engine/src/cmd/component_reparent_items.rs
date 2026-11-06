//! Moving placements, timeline features and bodies into a component in one undo step
//! (`browser.reparent`: the owner of each changes, whatever its kind).
//!
//! What Paste does after Cut. Each kind moves by its own command (`occurrence.reparent`,
//! `component.move_features`, `component.move_bodies`), in that order, on a scratch copy, so what
//! an earlier step carried along is left alone by a later one: a moved component takes its
//! features and bodies with it, moved features take the bodies they made. The result is adopted
//! only when every step succeeds, as one undo step; otherwise nothing changes.

use std::collections::BTreeSet;

use super::move_features::move_features;
use super::reparent::{follow_visibility, reparent};
use super::transfer;
use super::*;
use crate::params::{name_or_id_list, string_list};

/// Most items of each kind in one move.
const MAX_ITEMS: usize = 1000;

/// A placement named by its occurrence id, or by its path from the top of the design.
fn placements(s: &Session, p: &Value, cmd: &str) -> Result<Vec<(u64, Option<Vec<u64>>)>> {
    let list = match p.get("occurrences") {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(a)) if a.len() <= MAX_ITEMS => a,
        Some(_) => return Err(bad(cmd, format!("`occurrences` must list at most {MAX_ITEMS} placements"))),
    };
    let mut out: Vec<(u64, Option<Vec<u64>>)> = Vec::new();
    for entry in list {
        let found = match entry {
            Value::Number(n) => n.as_u64().map(|id| (id, None)),
            Value::Array(a) if !a.is_empty() && a.len() <= 64 => {
                a.iter().map(Value::as_u64).collect::<Option<Vec<_>>>().and_then(|path| path.last().copied().map(|id| (id, Some(path))))
            }
            _ => None,
        };
        let (id, path) = found.ok_or_else(|| bad(cmd, "`occurrences` must list occurrence ids or paths of 1..64 occurrence ids"))?;
        if !s.doc.occurrences.iter().any(|o| o.id == id) {
            return Err(bad(cmd, format!("no occurrence {id}")));
        }
        if !out.iter().any(|(seen, _)| *seen == id) {
            out.push((id, path));
        }
    }
    Ok(out)
}

pub(super) fn reparent_items(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "browser.reparent";
    let dest = if p.get("component").is_some() { component(s, p, "component", cmd)? } else { s.active_component };
    let dest_path = p.get("component_path").cloned();
    let occurrences = placements(s, p, cmd)?;
    let features = name_or_id_list(cmd, p, "features")?;
    let bodies = string_list(cmd, p, "bodies")?;
    if features.len() > MAX_ITEMS || bodies.len() > MAX_ITEMS {
        return Err(bad(cmd, format!("`features` and `bodies` must list at most {MAX_ITEMS} items each")));
    }
    if occurrences.is_empty() && features.is_empty() && bodies.is_empty() {
        return Err(bad(cmd, "give `occurrences`, `features` or `bodies` to move"));
    }
    if s.doc.marker.is_some() {
        return Err(bad(cmd, "finish timeline rollback before moving"));
    }
    let mut feature_ids = BTreeSet::new();
    for k in &features {
        feature_ids.insert(s.doc.find_feature(k).map(|f| f.id).ok_or_else(|| bad(cmd, format!("no feature `{k}`")))?);
    }
    {
        let state = s.model.state();
        if let Some(missing) = bodies.iter().find(|b| state.body(b).is_none()) {
            return Err(bad(cmd, format!("no body `{missing}`")));
        }
    }

    let mut sc = s.scratch();
    sc.active_component = s.active_component;
    sc.active_occurrence_path = s.active_occurrence_path.clone();

    // Components first: each takes everything it holds.
    let mut moved_placements = Vec::new();
    let mut moved_components = Vec::new();
    let mut followed: Vec<(Vec<u64>, Vec<u64>)> = Vec::new();
    for (id, path) in &occurrences {
        let Some(o) = sc.doc.occurrences.iter().find(|o| o.id == *id).cloned() else { continue };
        if o.parent == dest {
            continue;
        }
        let mut params = json!({"occurrence": id, "parent": dest});
        if let Some(path) = path {
            params["path"] = json!(path);
        }
        if let Some(parent_path) = &dest_path {
            params["parent_path"] = parent_path.clone();
        }
        let old_path = occurrence_path(&sc, &params, *id, cmd)?;
        let out = reparent(&mut sc, &params)?;
        let new_path = out.get("path").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).collect::<Vec<_>>());
        followed.extend(new_path.map(|new| (old_path, new)));
        moved_placements.push(*id);
        moved_components.push(o.component);
    }
    // What a moved component holds travels with it.
    let travels = |sc: &Session, component: u64| moved_components.iter().any(|c| sc.doc.inside_component(component, *c));

    // Then features, which take the bodies they made.
    let features_to_move: Vec<u64> =
        feature_ids.iter().copied().filter(|id| sc.doc.feature(*id).is_some_and(|f| f.component != dest && !travels(&sc, f.component))).collect();
    let mut moved_features = Vec::new();
    if !features_to_move.is_empty() {
        let mut params = json!({"features": features_to_move, "component": dest});
        if let Some(path) = &dest_path {
            params["component_path"] = path.clone();
        }
        let out = move_features(&mut sc, &params)?;
        moved_features =
            out.get("moved").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
    }

    // Last the bodies still elsewhere: one a moved component or feature carried is already there.
    let bodies_to_move: Vec<String> = {
        let state = sc.model.state();
        bodies
            .iter()
            .filter(|b| {
                state.body(b).is_some_and(|body| {
                    let at = sc.doc.model_body_component(body);
                    at != dest && !travels(&sc, at)
                })
            })
            .cloned()
            .collect()
    };
    if !bodies_to_move.is_empty() {
        let mut params = json!({"bodies": bodies_to_move, "component": dest});
        if let Some(path) = &dest_path {
            params["component_path"] = path.clone();
        }
        transfer::move_bodies(&mut sc, &params)?;
    }

    if moved_placements.is_empty() && moved_features.is_empty() && bodies_to_move.is_empty() {
        return Err(bad(cmd, "already in that component"));
    }
    // The steps above are this command's one undo step.
    sc.undo.clear();
    s.accept_computed(sc);
    for (old, new) in &followed {
        follow_visibility(s, old, new);
    }
    Ok(json!({"component": dest, "occurrences": moved_placements, "features": moved_features, "bodies": bodies_to_move}))
}
