//! Undo/redo, the timeline, deletion and selection.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use solidcraft_doc::{Feature, FeatureKind};

use super::CommandSpec;
use crate::params::{bad, bool_, name_or_id_values, num, str_};
use crate::{EngineError, Result, Sel, Session, Snapshot};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("edit.undo", "Undo", undo).icon("undo").key("Ctrl+Z").noundo().doc_only(),
    CommandSpec::new("edit.redo", "Redo", redo).icon("redo").key("Ctrl+Y").noundo().doc_only(),
    CommandSpec::new("edit.repeat", "Repeat Last Command", crate::checked!("edit.repeat", repeat))
        .icon("redo")
        .key("Shift+R")
        .noundo()
        .doc_only()
        .contract(&REPEAT_CONTRACT)
        .output(crate::schema::output_schema_for::<crate::schema::edit::RepeatResult>)
        .params("Starts the last tool or dialog started in the app again, with its defaults (a toolbar click, not a replay of its values). Only the app has a tool to start: called here it is refused, and changes nothing"),
    CommandSpec::new("edit.checkpoint", "Checkpoint", checkpoint).icon("checkpoint")
        .noundo()
        .doc_only()
        .params("→ {checkpoint: id}: remember the design, active sketch and undo history (restore with edit.restore_checkpoint; the last 16 are kept, the oldest dropped first)"),
    CommandSpec::new("edit.restore_checkpoint", "Restore Checkpoint", restore_checkpoint).icon("restore")
        .noundo()
        .doc_only()
        .params("checkpoint: id; forget?: bool (true = drop it without going back). Goes back to it, undo history included. Restoring or forgetting uses it and every later checkpoint up: take a new one to go back again"),
    CommandSpec::new("timeline.compute_all", "Compute All", compute_all).at("SOLID", "MODIFY").icon("compute").noundo(),
    CommandSpec::new("timeline.play", "Play History", crate::checked!("timeline.play", play))
        .at("INSPECT", "HISTORY")
        .icon("timeline_play")
        .noundo()
        .doc_only()
        .enabled(super::not_in_sketch)
        .contract(&PLAY_CONTRACT)
        .output(crate::schema::output_schema_for::<crate::schema::edit::TimelinePlayResult>)
        .params("action?: toggle|play|stop (default toggle) → action: validated display request; the connected UI plays from the marker to the end with a half-second pause per rebuilt step. Headless callers only validate the request; no dialog, document edit or animation clock is created"),
    CommandSpec::new("timeline.delete", "Delete", delete)
        .at("SOLID", "MODIFY")
        .icon("delete")
        .key("Delete")
        .params("feature: id|name | features: [id|name] | groups: [timeline folder ids] (a folder goes with all its features, joints and nested folders; one undo step; a sketch takes its dependent features with it) → deleted: [feature ids]; joints?: [joint ids]; groups?: [folder ids deleted]; failing?: [{feature, error}] (later features that worked and now fail)"),
    CommandSpec::new("timeline.rename", "Rename", rename).icon("rename").params("feature: id|name, name (references to it follow)"),
    CommandSpec::new("timeline.group", "New Feature Folder", group)
        .icon("folder")
        .doc_only()
        .params("name?; features?: [id|name]; joints?: [id|name]; groups?: [folder ids]; parent?: folder id (features and joints leave any other timeline folder)"),
    CommandSpec::new("timeline.ungroup", "Remove Feature Folders", ungroup).icon("folder")
        .doc_only()
        .params("group: id | groups: [ids] (features and joints remain in the timeline)"),
    CommandSpec::new("timeline.rename_group", "Rename Feature Folder", rename_group).icon("rename").doc_only().params("group: id, name (renames the folder holding features and joints)"),
    CommandSpec::new("timeline.move_to_group", "Move Features to Folder", move_to_group).icon("folder")
        .doc_only()
        .params("features?: [id|name], joints?: [id|name], groups?: [ids], group?: id (omit or null to move to the timeline root)"),
    CommandSpec::new("timeline.move_many", "Move Timeline Items", move_many).icon("reorder").params(
        "features?: [id|name], groups?: [folder ids], group?: folder id, position: insertion slot, active?: bool (true = the moved contents end up before the history marker, false = behind it; other features keep their state); atomically changes folder membership and moves all selected contents as one block",
    ),
    CommandSpec::new("timeline.rollback", "Move Timeline Marker", rollback).icon("rollback").params("position: number of features to keep active, or \"end\" (omit = end)"),
    CommandSpec::new("timeline.roll_to", "Roll History Marker", roll_to).icon("rollback")
        .params("position: number of features to keep active, or \"end\" (omit = end) | feature: id|name (the marker goes right after it)"),
    CommandSpec::new("timeline.reorder", "Reorder Feature", reorder).icon("reorder")
        .params("feature: id|name, position: its new index, active?: bool (true = it ends up before the history marker, false = behind it; other features keep their state); refused when a feature that worked would fail, unless allow_errors?: bool (default false): then made, and broken: [{feature, error}] lists the features it broke"),
    CommandSpec::new("timeline.reorder_many", "Reorder Features", reorder_many).icon("reorder").params(
        "features: [id|name], position: insertion slot in the current history; selected features keep their relative order; active?: bool (as timeline.reorder's); refused when a feature that worked would fail, unless allow_errors?: bool (default false): then made, and broken: [{feature, error}] lists the features it broke",
    ),
    CommandSpec::new("timeline.redefine", "Edit Feature", redefine).icon("edit")
        .params("feature: id|name, command: the feature's command id, params: its parameters (the feature is rebuilt in place) → feature, recomputed, restored, rolled_back: bool (the feature is after the history marker, so the edit is not built yet; `note` then says so), warning?"),
    CommandSpec::new("timeline.dependents", "Feature Dependents", dependents).icon("graph")
        .noundo()
        .params("feature: id|name → features deleted with it, and features that would fail without it"),
    CommandSpec::new("timeline.suppress", "Suppress Feature", suppress).icon("suppress").params(
        "feature: id|name | features: [id|name] | groups: [timeline folder ids] (every feature and joint in those folders and their nested \
         folders; one undo step), suppressed?: bool (default toggles: features and joints are all \
         suppressed unless all of them already are, then all are unsuppressed) → feature: id (or features: [ids]; groups: [ids] and joints: [ids] when groups are given), suppressed",
    ),
    CommandSpec::new("timeline.edit", "Edit Feature", edit_feature).icon("edit").params(
        "feature: id|name, set: {fields to change, merged into the feature's definition as timeline.inspect returns it, \
         e.g. {\"extent\": {\"distance\": \"30\"}} (extrude), {\"angle\": \"90 deg\"} (revolve); \
         a dotted key names a nested field ({\"plane.distance\": \"8 mm\"} = {\"plane\": {\"distance\": \"8 mm\"}}); \
         expressions are text, or a number in mm or degrees} → recomputed: features rebuilt, restored: features whose earlier result for the same inputs was reused",
    ),
    CommandSpec::new("timeline.inspect", "Inspect Feature", inspect_feature).icon("inspect").noundo().params(
        "group: folder id → group, name, parent, features, joints; or feature: id|name → type, definition (the fields timeline.edit's set changes, with their current values), \
         inputs: [{label, param, expression, unit}] (named inputs parameters.change can also set), error, warning",
    ),
    CommandSpec::new("select.set", "Select", select_set).icon("select")
        .noundo()
        .params("items: [{type: body|edge|face|feature|sketch_curve|sketch_point|profile, …}], add?: bool; face/edge: body plus point [x,y,z] in world mm or index (both also accepted); a point uses Measure's pick tolerance, an index supplies a point on the item; invalid partial picks leave the selection unchanged"),
    CommandSpec::new("select.filter", "Selection filter", select_filter).icon("select").noundo().doc_only()
        .params("mode: body|face|component → validated UI pick granularity; headless echo only, no document edits or undo"),
    CommandSpec::new("select.clear", "Clear Selection", select_clear).icon("select").noundo(),
    CommandSpec::new("select.same_look", "Select Same Look", select_same_look).icon("select").noundo().doc_only()
        .params("body?: world body name, face?: face index; defaults to the current body/face selection → selects objects using the resolved appearance"),
];

const PLAY_CONTRACT: crate::schema::TypedContract<crate::schema::edit::TimelinePlayParams> = crate::schema::TypedContract::new();

fn play(_s: &mut Session, p: &Value) -> Result<Value> {
    let request: crate::schema::edit::TimelinePlayParams = serde_json::from_value(p.clone()).map_err(|e| bad("timeline.play", e.to_string()))?;
    serde_json::to_value(crate::schema::edit::TimelinePlayResult { action: request.action }).map_err(|e| EngineError::Other(e.to_string()))
}

fn undo(s: &mut Session, _p: &Value) -> Result<Value> {
    let Some(prev) = s.undo.pop() else { return Err(EngineError::Other("nothing to undo".into())) };
    let now = go_to(s, prev);
    let label = now.label.clone();
    s.redo.push(now);
    Ok(json!({"undone": label}))
}

fn redo(s: &mut Session, _p: &Value) -> Result<Value> {
    let Some(next) = s.redo.pop() else { return Err(EngineError::Other("nothing to redo".into())) };
    let now = go_to(s, next);
    let label = now.label.clone();
    s.undo.push(now);
    Ok(json!({"redone": label}))
}

const REPEAT_CONTRACT: crate::schema::TypedContract<crate::schema::edit::RepeatParams> = crate::schema::TypedContract::new();

/// Repeat Last Command (#7077) starts a tool, which only the app has: it starts the last one in
/// `SolveApp::start` and never reaches this. A script runs the command it wants itself.
fn repeat(_s: &mut Session, _p: &Value) -> Result<Value> {
    Err(EngineError::Other(
        "edit.repeat starts the last tool or dialog again in the app (its key, the canvas menu or command search); there is no tool to start here: run that command itself".into(),
    ))
}

/// Put the session at `snap`; returns where it was, under the same label.
fn go_to(s: &mut Session, snap: Snapshot) -> Snapshot {
    let was = Snapshot {
        label: snap.label,
        doc: s.doc.clone(),
        active_sketch: s.active_sketch,
        pending_moves: s.pending_moves.clone(),
        visibility: s.visibility.clone(),
    };
    let same = std::sync::Arc::ptr_eq(&s.doc, &snap.doc);
    // Undoing a hide (or a show) leaves the document alone: the model needs no evaluating.
    let display_only = same && s.visibility != snap.visibility;
    s.doc = snap.doc;
    s.active_sketch = snap.active_sketch;
    s.pending_moves = snap.pending_moves;
    s.visibility = snap.visibility;
    s.selection.clear();
    // A new document is evaluated by `execute`.
    if same {
        if display_only {
            s.refresh_display();
        } else {
            s.refresh();
        }
    }
    was
}

fn checkpoint(s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(json!({"checkpoint": s.checkpoint()}))
}

fn restore_checkpoint(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "edit.restore_checkpoint";
    let id = p.get("checkpoint").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "`checkpoint` must be an id from edit.checkpoint"))?;
    if bool_(p, "forget") == Some(true) {
        s.drop_checkpoint(id);
        return Ok(json!({"checkpoint": id, "restored": false}));
    }
    s.restore_checkpoint(id)?;
    Ok(json!({"checkpoint": id, "restored": true}))
}

fn compute_all(s: &mut Session, _p: &Value) -> Result<Value> {
    s.model = solidcraft_doc::Model::new();
    s.refresh();
    let errors = s.model.results.iter().filter(|r| r.error.is_some()).count();
    Ok(json!({"features": s.model.results.len(), "errors": errors}))
}

pub(super) fn feature_id(s: &Session, v: Option<&Value>, cmd: &str) -> Result<u64> {
    match v {
        Some(Value::Number(n)) => n.as_u64().and_then(|id| s.doc.feature(id)).map(|f| f.id).ok_or_else(|| bad(cmd, format!("no feature `{n}`"))),
        Some(Value::String(name)) => s.doc.resolve_feature_name(name).map(|f| f.id).map_err(|e| bad(cmd, e.to_string())),
        _ => Err(bad(cmd, "`feature` must be an id or name")),
    }
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "timeline.delete";
    // Resolve every key against the same timeline. A cascade or a repeated key must never
    // redirect a later delete to another feature that happens to match after the first one.
    let keys = name_or_id_values(cmd, p, "features")?;
    let mut ids = keys.iter().chain(p.get("feature")).map(|v| feature_id(s, Some(v), cmd)).collect::<Result<Vec<_>>>()?;
    let groups = group_ids(s, p, false, cmd)?;
    if ids.is_empty() && groups.is_empty() {
        return Err(bad(cmd, "`features` must list features, or `groups` timeline folders"));
    }
    // A folder goes with its features and nested folders, its features in timeline order.
    let (folders, contents) = group_tree(&s.doc, &groups);
    let joints = group_joint_ids(&s.doc, &groups);
    ids.extend(s.doc.features.iter().filter(|f| contents.contains(&f.id)).map(|f| f.id));
    // Features that work now; any of them failing after the delete is reported (a later
    // feature that names a deleted body, #267).
    let worked: std::collections::BTreeSet<u64> = s.model.results.iter().filter(|r| r.error.is_none() && !r.skipped).map(|r| r.id).collect();
    super::joints::delete_joints(s.doc_mut(), &joints);
    let mut gone = Vec::new();
    for id in ids {
        if s.doc.feature(id).is_some() {
            gone.extend(s.doc_mut().delete_feature(id)?);
        }
    }
    if !folders.is_empty() {
        s.doc_mut().timeline_groups.retain(|group| !folders.contains(&group.id));
    }
    if s.active_sketch.is_some_and(|a| gone.contains(&a)) {
        s.active_sketch = None;
    }
    // An inserted file's feature takes its components with it.
    if s.active_component != 0 && !s.doc.components.iter().any(|c| c.id == s.active_component) {
        s.active_component = 0;
    }
    s.selection.clear();
    s.refresh();
    let failing: Vec<Value> = s
        .model
        .results
        .iter()
        .filter(|r| worked.contains(&r.id))
        .filter_map(|r| r.error.as_ref().map(|e| json!({"feature": r.name, "error": e})))
        .collect();
    let mut out = json!({"deleted": gone});
    if !joints.is_empty() {
        out["joints"] = json!(joints);
    }
    if !folders.is_empty() {
        out["groups"] = json!(folders);
    }
    if !failing.is_empty() {
        out["failing"] = json!(failing);
    }
    Ok(out)
}

fn rename(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "timeline.rename";
    let id = feature_id(s, p.get("feature"), cmd)?;
    let name =
        str_(p, "name").map(str::trim).filter(|n| !n.is_empty() && n.len() <= 128).ok_or_else(|| bad(cmd, "`name` must be 1…128 characters"))?;
    let old = s.doc.feature(id).map(|f| f.name.clone()).ok_or_else(|| bad(cmd, "feature"))?;
    if s.doc.features.iter().any(|f| f.name == name && f.id != id) {
        return Err(bad(cmd, format!("another feature is called `{name}`")));
    }
    if s.doc.feature(id).is_some_and(|f| matches!(f.kind, FeatureKind::ConstructionPlane { .. })) {
        super::features::check_plane_name(name, cmd)?;
    }
    if name == old {
        return Ok(json!({"feature": id, "name": name}));
    }
    let names = BTreeMap::from([(old.clone(), name.to_string())]);
    let sketches = BTreeMap::new();
    let doc = s.doc_mut();
    if let Some(f) = doc.feature_mut(id) {
        f.name = name.to_string();
    }
    for f in doc.features.iter_mut() {
        f.kind.remap_refs(&sketches, &names);
    }
    doc.rename_feature_keys(&old, name);
    // The hidden list follows in `Session::refresh_from` (undo and redo too).
    for sel in &mut s.selection {
        if let Sel::Plane { name: n } = sel
            && *n == old
        {
            *n = name.to_string();
        }
    }
    Ok(json!({"feature": id, "name": name}))
}

fn timeline_group_id(s: &Session, p: &Value, cmd: &str) -> Result<u64> {
    timeline_group_id_field(s, p, "group", cmd)
}

fn timeline_group_id_field(s: &Session, p: &Value, field: &str, cmd: &str) -> Result<u64> {
    let id = p.get(field).and_then(Value::as_u64).ok_or_else(|| bad(cmd, format!("`{field}` must be a timeline folder id")))?;
    if s.doc.timeline_groups.iter().any(|group| group.id == id) { Ok(id) } else { Err(bad(cmd, format!("no timeline folder {id}"))) }
}

fn group_features(s: &Session, p: &Value, required: bool, cmd: &str) -> Result<Vec<u64>> {
    let values = match p.get("features") {
        Some(Value::Array(values)) => values,
        None if !required => return Ok(Vec::new()),
        _ => return Err(bad(cmd, "`features` must be a list of feature ids or names")),
    };
    if values.is_empty() && required {
        return Err(bad(cmd, "`features` must list at least one feature"));
    }
    if values.len() > 10_000 {
        return Err(bad(cmd, "too many features"));
    }
    let mut ids = Vec::new();
    for value in values {
        let id = feature_id(s, Some(value), cmd)?;
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

fn group_joints(s: &Session, p: &Value, cmd: &str) -> Result<Vec<u64>> {
    let values = match p.get("joints") {
        None => return Ok(Vec::new()),
        Some(Value::Array(values)) if values.len() <= 10_000 => values,
        _ => return Err(bad(cmd, "`joints` must list at most 10000 joint ids or names")),
    };
    let mut ids = std::collections::BTreeSet::new();
    for value in values {
        ids.insert(super::joints::joint_id(s, Some(value), cmd)?);
    }
    Ok(ids.into_iter().collect())
}

fn group_joint_ids(doc: &solidcraft_doc::Document, roots: &[u64]) -> Vec<u64> {
    let folders = group_tree(doc, roots).0;
    let ids: std::collections::BTreeSet<u64> =
        doc.timeline_groups.iter().filter(|group| folders.contains(&group.id)).flat_map(|group| group.joints.iter().copied()).collect();
    doc.assembly.joints.iter().filter(|joint| ids.contains(&joint.id)).map(|joint| joint.id).collect()
}

fn group_ids(s: &Session, p: &Value, required: bool, cmd: &str) -> Result<Vec<u64>> {
    let values = match p.get("groups") {
        Some(Value::Array(values)) => values,
        None if !required => return Ok(Vec::new()),
        None => return Err(bad(cmd, "`groups` must be a list of timeline folder ids")),
        _ => return Err(bad(cmd, "`groups` must be a list of timeline folder ids")),
    };
    if values.is_empty() && required {
        return Err(bad(cmd, "`groups` must list at least one folder"));
    }
    if values.len() > 10_000 {
        return Err(bad(cmd, "too many folders"));
    }
    let mut ids = Vec::new();
    for value in values {
        let id = value.as_u64().ok_or_else(|| bad(cmd, "`groups` must contain folder ids"))?;
        if !s.doc.timeline_groups.iter().any(|group| group.id == id) {
            return Err(bad(cmd, format!("no timeline folder {id}")));
        }
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

fn group_move_would_cycle(s: &Session, target: u64, sources: &[u64]) -> bool {
    let sources: std::collections::BTreeSet<u64> = sources.iter().copied().collect();
    let parents: std::collections::BTreeMap<u64, Option<u64>> = s.doc.timeline_groups.iter().map(|group| (group.id, group.parent)).collect();
    let mut current = Some(target);
    let mut steps = 0usize;
    while let Some(id) = current {
        if sources.contains(&id) {
            return true;
        }
        steps += 1;
        if steps > s.doc.timeline_groups.len() {
            return true;
        }
        current = parents.get(&id).copied().flatten();
    }
    false
}

fn group_feature_ids(doc: &solidcraft_doc::Document, roots: &[u64]) -> std::collections::BTreeSet<u64> {
    group_tree(doc, roots).1
}

/// The timeline folders `roots` and every folder nested in them, and the features they hold.
fn group_tree(doc: &solidcraft_doc::Document, roots: &[u64]) -> (std::collections::BTreeSet<u64>, std::collections::BTreeSet<u64>) {
    let by_id: std::collections::BTreeMap<u64, &solidcraft_doc::TimelineGroup> = doc.timeline_groups.iter().map(|group| (group.id, group)).collect();
    let mut children = std::collections::BTreeMap::<u64, Vec<u64>>::new();
    for group in &doc.timeline_groups {
        if let Some(parent) = group.parent {
            children.entry(parent).or_default().push(group.id);
        }
    }
    let mut pending = roots.to_vec();
    let mut visited = std::collections::BTreeSet::new();
    let mut features = std::collections::BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        if let Some(group) = by_id.get(&id) {
            features.extend(group.features.iter().copied());
            pending.extend(children.get(&id).into_iter().flatten().copied());
        }
    }
    (visited, features)
}

fn sort_group_features(doc: &mut solidcraft_doc::Document) {
    let positions: std::collections::BTreeMap<u64, usize> = doc.features.iter().enumerate().map(|(index, feature)| (feature.id, index)).collect();
    for group in &mut doc.timeline_groups {
        group.features.sort_by_key(|feature| positions.get(feature).copied().unwrap_or(usize::MAX));
    }
}

fn require_contiguous_groups(doc: &solidcraft_doc::Document, cmd: &str) -> Result<()> {
    let Some(id) = solidcraft_doc::non_contiguous_timeline_group(&doc.features, &doc.timeline_groups) else { return Ok(()) };
    let name = doc.timeline_groups.iter().find(|group| group.id == id).map(|group| group.name.as_str()).unwrap_or("unknown");
    Err(bad(cmd, format!("timeline folder `{name}` must contain one contiguous run of features")))
}

fn group(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "timeline.group";
    let mut features = group_features(s, p, false, cmd)?;
    let mut joints = group_joints(s, p, cmd)?;
    let groups = group_ids(s, p, false, cmd)?;
    let name = match str_(p, "name").map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) if name.len() <= 128 => name.to_string(),
        Some(_) => return Err(bad(cmd, "`name` must be 1…128 characters")),
        None => format!("Folder{}", s.doc.timeline_groups.len() + 1),
    };
    let id = s.doc_mut().allocate_id()?;
    let parent = match p.get("parent") {
        Some(Value::Null) | None => None,
        Some(_) => Some(timeline_group_id_field(s, p, "parent", cmd)?),
    };
    if let Some(parent) = parent
        && group_move_would_cycle(s, parent, &groups)
    {
        return Err(bad(cmd, "a new folder cannot contain an ancestor of its parent"));
    }
    let nested = group_feature_ids(&s.doc, &groups);
    features.retain(|feature| !nested.contains(feature));
    let nested_joints = group_joint_ids(&s.doc, &groups);
    joints.retain(|joint| !nested_joints.contains(joint));
    let mut next = (*s.doc).clone();
    for group in &mut next.timeline_groups {
        group.joints.retain(|joint| !joints.contains(joint));
        group.features.retain(|feature| !features.contains(feature));
        if groups.contains(&group.id) {
            group.parent = Some(id);
        }
    }
    next.timeline_groups.push(solidcraft_doc::TimelineGroup { id, name: name.clone(), parent, features, joints });
    sort_group_features(&mut next);
    require_contiguous_groups(&next, cmd)?;
    *s.doc_mut() = next;
    Ok(json!({"group": id, "name": name, "parent": parent, "groups": groups}))
}

fn ungroup(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "timeline.ungroup";
    let ids = match p.get("groups") {
        Some(Value::Array(values)) if !values.is_empty() && values.len() <= 10_000 => {
            let mut ids = Vec::new();
            for value in values {
                let id = value.as_u64().ok_or_else(|| bad(cmd, "`groups` must contain timeline folder ids"))?;
                if !s.doc.timeline_groups.iter().any(|group| group.id == id) {
                    return Err(bad(cmd, format!("no timeline folder {id}")));
                }
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            ids
        }
        Some(Value::Array(_)) => return Err(bad(cmd, "`groups` must list 1…10000 timeline folder ids")),
        Some(_) => return Err(bad(cmd, "`groups` must be a list of timeline folder ids")),
        None => vec![timeline_group_id(s, p, cmd)?],
    };
    let parents: std::collections::BTreeMap<u64, Option<u64>> =
        s.doc.timeline_groups.iter().filter(|group| ids.contains(&group.id)).map(|group| (group.id, group.parent)).collect();
    let surviving_parent = |mut parent: Option<u64>| {
        let mut steps = 0usize;
        while let Some(id) = parent.filter(|id| ids.contains(id)) {
            parent = parents.get(&id).copied().flatten();
            steps += 1;
            if steps > parents.len() {
                return None;
            }
        }
        parent
    };
    let mut next = (*s.doc).clone();
    let moved_features: Vec<(Option<u64>, Vec<u64>, Vec<u64>)> = next
        .timeline_groups
        .iter()
        .filter(|group| ids.contains(&group.id))
        .map(|group| (surviving_parent(group.parent), group.features.clone(), group.joints.clone()))
        .collect();
    for group in &mut next.timeline_groups {
        if ids.contains(&group.id) {
            continue;
        }
        group.parent = surviving_parent(group.parent);
        for (_, features, joints) in moved_features.iter().filter(|(parent, _, _)| *parent == Some(group.id)) {
            group.joints.extend(joints.iter().copied());
            group.features.extend(features.iter().copied());
        }
    }
    next.timeline_groups.retain(|group| !ids.contains(&group.id));
    sort_group_features(&mut next);
    require_contiguous_groups(&next, cmd)?;
    *s.doc_mut() = next;
    Ok(json!({"groups": ids}))
}

fn rename_group(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "timeline.rename_group";
    let id = timeline_group_id(s, p, cmd)?;
    let name = str_(p, "name")
        .map(str::trim)
        .filter(|name| !name.is_empty() && name.len() <= 128)
        .ok_or_else(|| bad(cmd, "`name` must be 1…128 characters"))?;
    if let Some(group) = s.doc_mut().timeline_groups.iter_mut().find(|group| group.id == id) {
        group.name = name.to_string();
    }
    Ok(json!({"group": id, "name": name}))
}

fn move_to_group(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "timeline.move_to_group";
    let features = group_features(s, p, false, cmd)?;
    let joints = group_joints(s, p, cmd)?;
    let groups = group_ids(s, p, false, cmd)?;
    if features.is_empty() && joints.is_empty() && groups.is_empty() {
        return Err(bad(cmd, "list at least one feature, joint or folder"));
    }
    let target = match p.get("group") {
        Some(Value::Null) | None => None,
        Some(_) => Some(timeline_group_id(s, p, cmd)?),
    };
    if let Some(target) = target
        && group_move_would_cycle(s, target, &groups)
    {
        return Err(bad(cmd, "a folder cannot be moved into itself or one of its descendants"));
    }
    let mut next = (*s.doc).clone();
    for group in &mut next.timeline_groups {
        group.joints.retain(|joint| !joints.contains(joint));
        group.features.retain(|feature| !features.contains(feature));
        if Some(group.id) == target {
            group.joints.extend(joints.iter().copied());
            group.features.extend(features.iter().copied());
        }
    }
    for group in &mut next.timeline_groups {
        if groups.contains(&group.id) {
            group.parent = target;
        }
    }
    sort_group_features(&mut next);
    require_contiguous_groups(&next, cmd)?;
    *s.doc_mut() = next;
    Ok(json!({"features": features, "joints": joints, "groups": groups, "group": target}))
}

fn move_many(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "timeline.move_many";
    let mut features = group_features(s, p, false, cmd)?;
    let groups = group_ids(s, p, false, cmd)?;
    if features.is_empty() && groups.is_empty() {
        return Err(bad(cmd, "list at least one feature or folder"));
    }
    let target = match p.get("group") {
        Some(Value::Null) | None => None,
        Some(_) => Some(timeline_group_id(s, p, cmd)?),
    };
    if let Some(target) = target
        && group_move_would_cycle(s, target, &groups)
    {
        return Err(bad(cmd, "a folder cannot be moved into itself or one of its descendants"));
    }
    let nested = group_feature_ids(&s.doc, &groups);
    features.retain(|feature| !nested.contains(feature));
    let moving: Vec<u64> =
        s.doc.features.iter().filter(|feature| nested.contains(&feature.id) || features.contains(&feature.id)).map(|feature| feature.id).collect();
    let mut next = (*s.doc).clone();
    for group in &mut next.timeline_groups {
        group.features.retain(|feature| !features.contains(feature));
        if Some(group.id) == target {
            group.features.extend(features.iter().copied());
        }
        if groups.contains(&group.id) {
            group.parent = target;
        }
    }
    sort_group_features(&mut next);
    let slot = num(p, "position").map(|x| x.max(0.0) as usize).ok_or_else(|| bad(cmd, "`position` must be a number"))?;
    let active = active_param(p, cmd)?;
    let (mut next, ordered, _, checked) = reorder_features(s, next, &moving, slot, false, cmd)?;
    let marker = next.marker;
    place_marker(s, &mut next, &moving, active, cmd)?;
    let checked = checked.filter(|_| next.marker == marker);
    if next != *s.doc {
        if let Some(model) = checked {
            s.model = model;
        }
        *s.doc_mut() = next;
    }
    Ok(json!({"features": ordered, "groups": groups, "group": target, "position": slot}))
}

fn rollback(s: &mut Session, p: &Value) -> Result<Value> {
    move_marker(s, p, "timeline.rollback")
}

fn roll_to(s: &mut Session, p: &Value) -> Result<Value> {
    move_marker(s, p, "timeline.roll_to")
}

/// `timeline.rollback` and `timeline.roll_to`, refusing in the name of the one asked for (#4712).
fn move_marker(s: &mut Session, p: &Value, cmd: &str) -> Result<Value> {
    let n = s.doc.features.len();
    let pos = match p.get("feature") {
        Some(f) => {
            let id = feature_id(s, Some(f), cmd)?;
            s.doc.feature_index(id).map(|i| i + 1)
        }
        // A number of features, or "end" (as omitting it); anything else is refused rather than
        // read as "end".
        None => match p.get("position") {
            None | Some(Value::Null) => None,
            Some(Value::String(t)) if t.trim().eq_ignore_ascii_case("end") => None,
            Some(v) => {
                let x = v.as_f64().or_else(|| v.as_str().and_then(|t| t.trim().parse::<f64>().ok())).and_then(crate::params::finite);
                let why = "`position` must be a number of features to keep active, or \"end\"";
                let x = x.ok_or_else(|| bad(cmd, why))?;
                // A negative position was taken as 0, rolling the whole timeline back: an agent
                // meaning "one step back" by -1 lost every body without a word (#4712).
                if x < 0.0 {
                    return Err(bad(cmd, format!("`position` {x} is negative: it counts the features to keep active, 0 to {n} (or \"end\")")));
                }
                Some(x as usize)
            }
        },
    };
    s.doc_mut().marker = match pos {
        Some(k) if k < n => Some(k),
        _ => None,
    };
    s.selection.clear();
    Ok(json!({"marker": s.doc.marker.unwrap_or(n), "features": n}))
}

/// The features named by `feature: id|name` and/or `features: [id|name]` (or one id|name), in
/// the order given and without repeats. Each must exist.
fn feature_ids(s: &Session, p: &Value, cmd: &str) -> Result<Vec<u64>> {
    let mut given: Vec<&Value> = Vec::new();
    if let Some(f) = p.get("feature").filter(|v| !v.is_null()) {
        given.push(f);
    }
    match p.get("features") {
        None | Some(Value::Null) => {}
        Some(Value::Array(a)) if a.len() <= 10_000 => given.extend(a),
        Some(Value::Array(_)) => return Err(bad(cmd, "too many features")),
        Some(one @ (Value::Number(_) | Value::String(_))) => given.push(one),
        Some(_) => return Err(bad(cmd, "`features` must be a list of feature ids or names")),
    }
    if given.is_empty() {
        return Err(bad(cmd, "`feature` (id or name) or `features` (a list of them) must name a feature"));
    }
    let mut ids = Vec::new();
    for v in given {
        let id = feature_id(s, Some(v), cmd)?;
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

/// The names the features from the first of `ids` to be suppressed on made, for those that made a
/// body numbered by the counter (`BodyN`). Counted bodies take the next free number each time the
/// design is computed, so once a feature that made one is suppressed, every counted body after it
/// would move down one (`Body2` becoming `Body1`) and break what names it, such as a Move of
/// `Body2` (#4302). Stored as their names, they stay as they were, as when a counted body is
/// renamed (#1109). The suppressed feature keeps its own too: the counter passes the names of
/// suppressed features by, so a body made meanwhile does not take `Body1` and lose it again when
/// the feature is back.
fn counted_names_from(s: &Session, ids: &[u64]) -> Vec<(u64, Vec<String>)> {
    let made = |f: &Feature| -> Vec<String> {
        s.model.result(f.id).map(|r| r.state.bodies.iter().filter(|b| b.feature == f.id).map(|b| b.name.clone()).collect()).unwrap_or_default()
    };
    let makes_counted = |f: &Feature| (0..made(f).len()).any(|i| super::browser::counted(f, i));
    let first = s.doc.features.iter().position(|f| ids.contains(&f.id) && !f.suppressed && makes_counted(f));
    let Some(at) = first else { return Vec::new() };
    s.doc.features.iter().skip(at).filter(|f| makes_counted(f)).map(|f| (f.id, made(f))).collect()
}

fn suppress(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "timeline.suppress";
    let groups = group_ids(s, p, false, cmd)?;
    let named = |key: &str| p.get(key).is_some_and(|v| !v.is_null());
    let mut ids = if groups.is_empty() || named("feature") || named("features") { feature_ids(s, p, cmd)? } else { Vec::new() };
    let contents = group_feature_ids(&s.doc, &groups);
    let more: Vec<u64> = s.doc.features.iter().map(|f| f.id).filter(|id| contents.contains(id) && !ids.contains(id)).collect();
    ids.extend(more);
    let joints = group_joint_ids(&s.doc, &groups);
    if ids.is_empty() && joints.is_empty() {
        return Err(bad(cmd, "the timeline folders hold no features or joints"));
    }
    let want = match bool_(p, "suppressed") {
        Some(b) => b,
        None if p.get("suppressed").is_some_and(|v| !v.is_null()) => return Err(bad(cmd, "`suppressed` must be true or false")),
        // Toggle: one feature flips; several are all suppressed unless all already are.
        None => {
            !(ids.iter().all(|id| s.doc.feature(*id).is_some_and(|f| f.suppressed))
                && joints.iter().all(|id| s.doc.assembly.joints.iter().any(|joint| joint.id == *id && joint.suppressed)))
        }
    };
    let keep = if want { counted_names_from(s, &ids) } else { Vec::new() };
    // One command, so one undo step however many features it changes.
    let doc = s.doc_mut();
    for (id, made) in &keep {
        if let Some(f) = doc.feature_mut(*id) {
            f.pin_body_names(made);
        }
    }
    for id in &ids {
        let f = doc.feature_mut(*id).ok_or_else(|| bad(cmd, format!("no feature {id}")))?;
        f.suppressed = want;
    }
    super::joints::suppress_joints(s.doc_mut(), &joints, want);
    match ids.as_slice() {
        [id] if groups.is_empty() && p.get("features").is_none_or(Value::is_null) => Ok(json!({"feature": id, "suppressed": want})),
        _ if groups.is_empty() => Ok(json!({"features": ids, "suppressed": want})),
        _ => Ok(json!({"features": ids, "joints": joints, "groups": groups, "suppressed": want})),
    }
}

fn merge(dst: &mut Value, src: &Value, depth: usize) {
    if depth > 16 {
        return;
    }
    match (dst, src) {
        (Value::Object(d), Value::Object(s)) => {
            for (k, v) in s {
                match d.get_mut(k) {
                    Some(slot) if slot.is_object() && v.is_object() => merge(slot, v, depth + 1),
                    _ => {
                        d.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (d, s) => *d = s.clone(),
    }
}

/// One step of a path into a feature's JSON definition.
#[derive(Clone)]
enum Step {
    Key(String),
    Index(usize),
}

fn path_text(path: &[Step]) -> String {
    let mut t = String::new();
    for s in path {
        match s {
            Step::Key(k) if t.is_empty() => t.push_str(k),
            Step::Key(k) => t.push_str(&format!(".{k}")),
            Step::Index(i) => t.push_str(&format!("[{i}]")),
        }
    }
    t
}

fn at_path<'a>(v: &'a Value, path: &[Step]) -> Option<&'a Value> {
    path.iter().try_fold(v, |v, s| match s {
        Step::Key(k) => v.get(k),
        Step::Index(i) => v.get(i),
    })
}

/// Does the stored feature read the field at `path` of `merged` (a feature's JSON with the edit
/// merged in)? serde skips a field it doesn't know whatever its value, but no field type takes a
/// flag, a text and nested lists alike.
fn reads_field(merged: &Value, path: &[Step]) -> bool {
    let Some((last, parent)) = path.split_last() else { return true };
    [json!(true), json!("\u{1}"), json!([[[]]])].into_iter().any(|probe| {
        let mut v = merged.clone();
        let slot = parent.iter().try_fold(&mut v, |v, s| match s {
            Step::Key(k) => v.get_mut(k),
            Step::Index(i) => v.get_mut(*i),
        });
        match (slot, last) {
            (Some(Value::Object(o)), Step::Key(k)) => {
                o.insert(k.clone(), probe);
            }
            (Some(Value::Array(a)), Step::Index(i)) => match a.get_mut(*i) {
                Some(x) => *x = probe,
                None => return false,
            },
            _ => return false,
        }
        serde_json::from_value::<Feature>(v).is_err()
    })
}

/// The first field of `set` that the feature's definition doesn't have. A field the definition
/// shows before or after the edit is known; one it leaves out (an unset option, a value at its
/// default) is known when the stored feature reads it. Setting an absent field to `null`
/// changes nothing either way and passes.
fn unknown_field(
    set: &Value,
    before: Option<&Value>,
    after: Option<&Value>,
    merged: &Value,
    path: &mut Vec<Step>,
    depth: usize,
) -> Option<Vec<Step>> {
    if depth > 16 {
        return None;
    }
    let check = |step: Step, v: &Value, path: &mut Vec<Step>| {
        let one = [step];
        let (b, a) = (before.and_then(|x| at_path(x, &one)), after.and_then(|x| at_path(x, &one)));
        let [step] = one;
        path.push(step);
        let found = if b.is_none() && a.is_none() && !v.is_null() && !reads_field(merged, path) {
            Some(path.clone())
        } else {
            unknown_field(v, b, a, merged, path, depth + 1)
        };
        path.pop();
        found
    };
    match set {
        Value::Object(o) => o.iter().find_map(|(k, v)| check(Step::Key(k.clone()), v, path)),
        Value::Array(items) => items.iter().enumerate().take(10_000).find_map(|(i, v)| check(Step::Index(i), v, path)),
        _ => None,
    }
}

/// "unknown field `offset`; ConstructionPlane has `plane.base.name`, `plane.distance`…": the
/// editable fields where it was given, as paths (each one timeline.edit's `set` takes as a key).
fn unknown_field_message(field: &[Step], def: &Value, kind: &FeatureKind) -> String {
    let type_name = kind.type_name();
    let heavy = heavy_fields(kind);
    let skip = |k: &str| NOT_EDITABLE.contains(&k) || heavy.iter().any(|(h, _)| *h == k);
    fn leaves(v: &Value, prefix: &str, skip: Option<&dyn Fn(&str) -> bool>, depth: usize, out: &mut Vec<String>) {
        match v {
            Value::Object(o) if depth < 4 && !o.is_empty() => {
                for (k, x) in o {
                    if skip.is_some_and(|skip| skip(k)) {
                        continue;
                    }
                    let at = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                    leaves(x, &at, None, depth + 1, out);
                }
            }
            _ if !prefix.is_empty() => out.push(format!("`{prefix}`")),
            _ => {}
        }
    }
    let parent = field.split_last().map(|(_, p)| p).unwrap_or_default();
    let mut known = Vec::new();
    if let Some(at) = at_path(def, parent) {
        let top: Option<&dyn Fn(&str) -> bool> = if parent.is_empty() { Some(&skip) } else { None };
        leaves(at, &path_text(parent), top, 0, &mut known);
    }
    const SHOWN: usize = 40;
    let more = known.len().saturating_sub(SHOWN);
    known.truncate(SHOWN);
    let mut list = known.join(", ");
    if more > 0 {
        list.push_str(&format!(" and {more} more"));
    }
    let field = path_text(field);
    if list.is_empty() {
        return format!("unknown field `{field}`; timeline.inspect lists {type_name}'s fields");
    }
    format!("unknown field `{field}`; {type_name} has {list} (timeline.inspect shows them with their values)")
}

/// `set` with dotted keys (`"plane.distance"`, as timeline.edit's unknown-field message lists the
/// fields) expanded into the nested form (`{"plane": {"distance": …}}`), merged with any nested
/// form given next to them. A key the definition itself has at that level stays as it is. A field
/// given twice (`plane.distance` and `plane: {distance}`) is refused.
fn expand_dotted(set: &Value, def: Option<&Value>, cmd: &str, depth: usize) -> Result<Value> {
    if depth > 16 {
        return Ok(set.clone());
    }
    match set {
        Value::Object(o) => {
            let mut nested = serde_json::Map::new();
            for (k, v) in o {
                let parts: Vec<&str> = k.split('.').collect();
                let literal = parts.len() == 1 || parts.len() > 16 || parts.iter().any(|x| x.is_empty()) || def.and_then(|d| d.get(k)).is_some();
                let parts = if literal { vec![k.as_str()] } else { parts };
                insert_field(&mut nested, &parts, v.clone(), k, cmd)?;
            }
            let mut out = serde_json::Map::new();
            for (k, v) in nested {
                let d = def.and_then(|d| d.get(&k));
                out.insert(k, expand_dotted(&v, d, cmd, depth + 1)?);
            }
            Ok(Value::Object(out))
        }
        Value::Array(a) => a
            .iter()
            .enumerate()
            .map(|(i, v)| expand_dotted(v, def.and_then(|d| d.get(i)), cmd, depth + 1))
            .collect::<Result<Vec<_>>>()
            .map(Value::Array),
        _ => Ok(set.clone()),
    }
}

/// Put `v` at `parts` in `out`, creating the objects on the way; `key` is the field as given.
fn insert_field(out: &mut serde_json::Map<String, Value>, parts: &[&str], v: Value, key: &str, cmd: &str) -> Result<()> {
    let twice = || bad(cmd, format!("`{key}` is given twice (as a dotted key and in the nested form)"));
    let Some((last, parents)) = parts.split_last() else { return Ok(()) };
    let mut at = out;
    for part in parents {
        let slot = at.entry(part.to_string()).or_insert_with(|| Value::Object(serde_json::Map::new()));
        at = slot.as_object_mut().ok_or_else(twice)?;
    }
    match (at.get_mut(*last), v) {
        (None, v) => {
            at.insert(last.to_string(), v);
        }
        (Some(Value::Object(slot)), Value::Object(more)) => {
            for (k, x) in more {
                insert_field(slot, &[k.as_str()], x, key, cmd)?;
            }
        }
        (Some(_), _) => return Err(twice()),
    }
    Ok(())
}

fn at_path_mut<'a>(v: &'a mut Value, path: &[Step]) -> Option<&'a mut Value> {
    path.iter().try_fold(v, |v, s| match s {
        Step::Key(k) => v.get_mut(k),
        Step::Index(i) => v.get_mut(*i),
    })
}

/// Paths of the finite numbers in `set` where the definition (`before`) has no number.
fn number_leaves(set: &Value, before: &Value, path: &mut Vec<Step>, out: &mut Vec<Vec<Step>>, depth: usize) {
    if depth > 16 || out.len() >= 1_000 {
        return;
    }
    match set {
        Value::Object(o) => {
            for (k, v) in o {
                path.push(Step::Key(k.clone()));
                number_leaves(v, before, path, out, depth + 1);
                path.pop();
            }
        }
        Value::Array(a) => {
            for (i, v) in a.iter().enumerate().take(10_000) {
                path.push(Step::Index(i));
                number_leaves(v, before, path, out, depth + 1);
                path.pop();
            }
        }
        Value::Number(n) if crate::params::number_text(n).is_some() && !matches!(at_path(before, path), Some(Value::Number(_))) => {
            out.push(path.clone());
        }
        _ => {}
    }
}

/// Finite numbers given for expression fields become the expression's text (`8` → `"8"`, mm or
/// degrees by context), as the feature commands take them (`params::expr`). Which fields are
/// expressions is what the feature's own walk says: each number is put in as a marker text, and
/// the markers the feature then shows among its expressions are expression fields. Numbers
/// anywhere else stay numbers.
fn numbers_as_expressions(mut set: Value, before: &Value) -> Value {
    let mut found = Vec::new();
    number_leaves(&set, before, &mut Vec::new(), &mut found, 0);
    if found.is_empty() {
        return set;
    }
    let marker = |i: usize| format!("\u{1}number {i}");
    // The markers among the feature's expressions with the numbers at `which` marked, or None
    // when the feature doesn't read that way.
    let expressions = |which: &[usize]| -> Option<std::collections::BTreeSet<String>> {
        let mut marked = set.clone();
        for &i in which {
            if let Some(slot) = found.get(i).and_then(|path| at_path_mut(&mut marked, path)) {
                *slot = Value::String(marker(i));
            }
        }
        let mut v = before.clone();
        merge(&mut v, &marked, 0);
        let f: Feature = serde_json::from_value(v).ok()?;
        Some(f.kind.expressions().into_iter().filter(|e| e.starts_with('\u{1}')).map(str::to_string).collect())
    };
    let all: Vec<usize> = (0..found.len()).collect();
    let hits: Vec<usize> = match expressions(&all) {
        Some(e) => all.into_iter().filter(|i| e.contains(&marker(*i))).collect(),
        // Some of them are numbers after all: one at a time.
        None => all.into_iter().take(64).filter(|i| expressions(&[*i]).is_some_and(|e| e.contains(&marker(*i)))).collect(),
    };
    for i in hits {
        if let Some(slot) = found.get(i).and_then(|path| at_path_mut(&mut set, path))
            && let Value::Number(n) = slot
            && let Some(text) = crate::params::number_text(n)
        {
            *slot = Value::String(text);
        }
    }
    set
}

/// `profiles` (also a loft section's) given as the feature commands take it (`"all"`,
/// `[index]`, `[{curves}]`, `[{point}]`...) becomes the stored form; the stored form passes.
fn command_profiles(set: &Value, cmd: &str) -> Result<Value> {
    let conv = |v: &mut Value| -> Result<()> {
        if let Some(pv) = v.get("profiles").filter(|x| x.get("type").is_none()) {
            let sel = super::features::profiles(&json!({"profiles": pv}), cmd)?;
            let sv = serde_json::to_value(sel).map_err(|e| EngineError::Other(e.to_string()))?;
            if let Some(o) = v.as_object_mut() {
                o.insert("profiles".into(), sv);
            }
        }
        Ok(())
    };
    let mut set = set.clone();
    conv(&mut set)?;
    if let Some(secs) = set.get_mut("sections").and_then(Value::as_array_mut) {
        for sec in secs.iter_mut().take(10_000) {
            conv(sec)?;
        }
    }
    Ok(set)
}

/// Fields of a stored feature left out of its definition: bookkeeping timeline.edit recomputes, and
/// the name and suppression, which timeline.rename and timeline.suppress change.
const NOT_EDITABLE: &[&str] = &["id", "type", "name", "suppressed", "param_names", "edge_refs", "edge_names", "face_names"];

/// timeline.edit keeps accepting names for pattern and mirror seeds and for construction planes
/// (#1134): each name in the merged definition resolves to the feature's id, refused when it
/// finds no feature, as the commands that take names refuse it.
fn resolve_feature_names(v: &mut Value, doc: &solidcraft_doc::Document, cmd: &str) -> Result<()> {
    match v {
        Value::Object(o) => {
            if o.get("type").and_then(Value::as_str) == Some("construction") {
                // Only a construction plane's name becomes an id: a revolve axis with the same
                // shape names a construction axis, which is still keyed by name.
                if let Some(name) = o.get("name").and_then(Value::as_str)
                    && let Some(f) = doc.find_feature(name).filter(|f| matches!(f.kind, FeatureKind::ConstructionPlane { .. }))
                {
                    o.remove("name");
                    o.insert("feature".into(), json!(f.id));
                }
                return Ok(());
            }
            for (_, x) in o.iter_mut() {
                resolve_feature_names(x, doc, cmd)?;
            }
            if let Some(Value::Array(seeds)) = o.get_mut("features") {
                for s in seeds.iter_mut() {
                    if let Value::String(name) = s {
                        let name = name.clone();
                        let id = doc.seed_feature(&name).ok_or_else(|| bad(cmd, format!("no feature `{name}`")))?;
                        *s = json!(id.id);
                    }
                }
            }
            Ok(())
        }
        Value::Array(a) => a.iter_mut().try_for_each(|x| resolve_feature_names(x, doc, cmd)),
        _ => Ok(()),
    }
}

/// Large payloads that timeline.edit cannot change, left out of a feature's definition: (field,
/// what it holds).
fn heavy_fields(kind: &FeatureKind) -> &'static [(&'static str, &'static str)] {
    match kind {
        FeatureKind::Sketch { .. } => &[("sketch", "the sketch geometry (sketch.inspect; edit it with the sketch commands)")],
        FeatureKind::Import { .. } => &[("step", "the imported STEP text"), ("components", "the imported product tree")],
        FeatureKind::MeshImport { .. } => &[("meshes", "the imported meshes")],
        _ => &[],
    }
}

fn inspect_feature(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "timeline.inspect";
    if p.get("group").is_some() {
        let id = timeline_group_id(s, p, cmd)?;
        let group = s.doc.timeline_groups.iter().find(|group| group.id == id).ok_or_else(|| bad(cmd, "folder"))?;
        return Ok(json!({"group": id, "name": group.name, "parent": group.parent, "features": group.features, "joints": group.joints}));
    }
    let id = feature_id(s, p.get("feature"), cmd)?;
    let f = s.doc.feature(id).ok_or_else(|| bad(cmd, "feature"))?;
    let mut def = serde_json::to_value(f).map_err(|e| EngineError::Other(e.to_string()))?;
    let mut omitted = Vec::new();
    if let Some(o) = def.as_object_mut() {
        for k in NOT_EDITABLE {
            o.remove(*k);
        }
        for (k, why) in heavy_fields(&f.kind) {
            if o.remove(*k).is_some() {
                omitted.push(json!({"field": k, "what": why}));
            }
        }
    }
    let inputs: Vec<Value> = f
        .kind
        .inputs()
        .into_iter()
        .enumerate()
        .map(|(i, (label, e, k))| {
            json!({"label": label, "param": f.param_names.get(i).filter(|n| !n.is_empty()), "expression": e, "unit": k.default_unit()})
        })
        .collect();
    let marker = s.doc.marker.unwrap_or(usize::MAX);
    let rolled_back = s.doc.features.iter().position(|x| x.id == id).is_some_and(|i| i >= marker);
    let r = s.model.result(id);
    Ok(json!({
        "id": f.id, "name": f.name, "type": f.kind.type_name(), "suppressed": f.suppressed, "rolled_back": rolled_back,
        "definition": def, "omitted": omitted, "inputs": inputs,
        "error": r.and_then(|r| r.error.clone()), "warning": r.and_then(|r| r.warning.clone()),
        "boolean_diagnostics": r.map(|r| &r.boolean_diagnostics),
    }))
}

fn edit_feature(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "timeline.edit";
    let id = feature_id(s, p.get("feature"), cmd)?;
    let set = p.get("set").filter(|v| v.is_object()).ok_or_else(|| bad(cmd, "`set` must be an object of fields"))?;
    let f = s.doc.feature(id).cloned().ok_or_else(|| bad(cmd, "feature"))?;
    let before = serde_json::to_value(&f).map_err(|e| EngineError::Other(e.to_string()))?;
    let set = expand_dotted(set, Some(&before), cmd, 0)?;
    let set = &numbers_as_expressions(command_profiles(&set, cmd)?, &before);
    let mut v = before.clone();
    merge(&mut v, set, 0);
    resolve_feature_names(&mut v, &s.doc, cmd)?;
    let mut nf: Feature = serde_json::from_value(v.clone()).map_err(|e| bad(cmd, format!("invalid feature: {e}")))?;
    nf.id = f.id;
    if let (FeatureKind::Extrude { extent: old, .. }, FeatureKind::Extrude { extent: new, .. }) = (&f.kind, &mut nf.kind)
        && old.direction != solidcraft_doc::Direction::Symmetric
        && new.direction == solidcraft_doc::Direction::Symmetric
        && set.get("extent").and_then(|e| e.get("convention")).is_none()
    {
        new.convention = solidcraft_doc::ExtrudeConvention::TotalDistance;
    }
    if std::mem::discriminant(&nf.kind) != std::mem::discriminant(&f.kind) {
        return Err(bad(cmd, "the feature type cannot change"));
    }
    // serde drops fields the definition doesn't have without a word: refuse them, or a typo
    // would report success and change nothing.
    let after = serde_json::to_value(&nf).map_err(|e| EngineError::Other(e.to_string()))?;
    if let Some(field) = unknown_field(set, Some(&before), Some(&after), &v, &mut Vec::new(), 0) {
        return Err(bad(cmd, unknown_field_message(&field, &before, &f.kind)));
    }
    if let FeatureKind::BodyTransfer { bodies, from, to, transform } = &nf.kind {
        solidcraft_doc::validate_body_transfer(&s.doc, &solidcraft_doc::plain(bodies), *from, *to, transform)?;
        nf.component = *to;
    }
    if let (FeatureKind::Sketch { sketch: a, .. }, FeatureKind::Sketch { sketch: b, .. }) = (&nf.kind, &f.kind)
        && a != b
    {
        return Err(bad(cmd, "edit sketch geometry with the sketch commands"));
    }
    // New edges for a blend: remember where they sit now (in the model before the feature).
    if let (
        FeatureKind::Fillet { edges: a, body, .. } | FeatureKind::Chamfer { edges: a, body, .. },
        FeatureKind::Fillet { edges: b, .. } | FeatureKind::Chamfer { edges: b, .. },
    ) = (&nf.kind, &f.kind)
        && a != b
    {
        let before = s.model.state_before(id);
        nf.edge_refs = solidcraft_doc::edge_refs(&before, body, &solidcraft_doc::plain(a));
        nf.edge_names = solidcraft_doc::edge_names_for(&before, body, &solidcraft_doc::plain(a));
    }
    // New or reordered flange edges: their names now, so each edge's own angle and height stay
    // with the edge they were given for.
    if let (FeatureKind::SheetFlange { edges: a, .. }, FeatureKind::SheetFlange { edges: b, .. }) = (&nf.kind, &f.kind) {
        let points = |e: &[solidcraft_doc::FlangeEdge]| e.iter().map(|x| x.point).collect::<Vec<_>>();
        if points(a) != points(b) {
            nf.edge_names = solidcraft_doc::sheet::sheet_edge_names(&s.model.state_before(id), &solidcraft_doc::plain(&points(a)));
        }
    }
    // New reference edges for a hole (#37): the names of the edges there now.
    if let (FeatureKind::Hole { references: a, position, .. }, FeatureKind::Hole { references: b, .. }) = (&nf.kind, &f.kind) {
        let edges = |r: &[solidcraft_doc::HoleReference]| r.iter().map(|x| x.edge.0).collect::<Vec<_>>();
        if edges(a) != edges(b) {
            nf.edge_names = solidcraft_doc::hole_reference::reference_edge_names(&s.model.state_before(id), position.0, &edges(a));
        }
    }
    // New reference edges for a cylinder (#6380): likewise.
    if let (FeatureKind::Cylinder { references: a, base, .. }, FeatureKind::Cylinder { references: b, .. }) = (&nf.kind, &f.kind) {
        let edges = |r: &[solidcraft_doc::HoleReference]| r.iter().map(|x| x.edge.0).collect::<Vec<_>>();
        if edges(a) != edges(b) {
            nf.edge_names = solidcraft_doc::hole_reference::cylinder_edge_names(&s.model.state_before(id), base, a);
        }
    }
    // New bend edges for a conversion: the names of the body's edges there now.
    if let (FeatureKind::SheetConvert { bends: a, body, .. }, FeatureKind::SheetConvert { bends: b, body: was, .. }) = (&nf.kind, &f.kind)
        && (a != b || body != was)
    {
        nf.edge_names = solidcraft_doc::sheet::bend_edge_names(&s.model.state_before(id), body, &solidcraft_doc::plain(a));
    }
    // Profiles picked by index or point are kept by their curves, as when the feature was made.
    if nf.kind.clone().profile_sels_mut() != f.kind.clone().profile_sels_mut() {
        super::features::stable_profiles(s, &mut nf.kind, false)?;
    }
    // New face points: their faces' names now.
    if nf.kind.face_points() != f.kind.face_points() {
        nf.face_names = solidcraft_doc::face_point_names(&s.doc, &s.model.state_before(id), &nf.kind, nf.component);
    }
    if let Some(slot) = s.doc_mut().feature_mut(id) {
        *slot = nf;
    }
    s.refresh();
    if let Some(e) = s.model.result(id).and_then(|r| r.error.clone()) {
        return Err(EngineError::Other(e));
    }
    Ok(json!({"feature": id, "recomputed": s.model.last_recomputed, "restored": s.model.last_restored}))
}

/// Complete a partial script pick without changing the stored selection type or reinterpreting
/// an existing index-and-point pair. Unlike Measure, selection keeps the exact face/edge index:
/// it must not merge a cylinder's patches or a circular rim's arcs into a different item.
fn selection_item(s: &Session, value: Value) -> Result<Sel> {
    use solidcraft_geom::Vec3;

    let kind = value.get("type").and_then(Value::as_str);
    if !matches!(kind, Some("face" | "edge")) || (value.get("index").is_some() && value.get("point").is_some()) {
        return serde_json::from_value(value).map_err(|e| bad("select.set", e.to_string()));
    }
    let face = kind == Some("face");
    #[derive(serde::Deserialize)]
    struct Pick {
        body: String,
        index: Option<usize>,
        point: Option<Vec3>,
    }
    let Pick { body, index, point } = serde_json::from_value(value).map_err(|e| bad("select.set", e.to_string()))?;
    let state = s.world_state();
    let b = state.body(&body).ok_or_else(|| bad("select.set", format!("no body `{body}`")))?;
    let missing = || bad("select.set", format!("no {} of `{body}` at the given index or point", if face { "face" } else { "edge" }));
    let (index, point) = match (index, point) {
        (Some(index), None) => {
            let point = if face {
                solidcraft_doc::naming::face_spot(b, index).map(|spot| spot.point)
            } else {
                let mesh = b.mesh();
                mesh.edges.get(index).and_then(|edge| match edge.as_slice() {
                    [a, b] => Some(a.lerp(*b, 0.5)),
                    // Use a sample on the curve, not the midpoint of a chord inside it.
                    points if points.len() > 2 => points.get(points.len() / 2).copied(),
                    _ => None,
                })
            };
            (index, point.filter(|p| p.is_finite()).ok_or_else(missing)?)
        }
        (None, Some(point)) if point.is_finite() => {
            let index = if face {
                solidcraft_doc::appearance::face_index_at(b, point)
            } else {
                let mesh = b.mesh();
                mesh.edges
                    .iter()
                    .enumerate()
                    .filter_map(|(index, edge)| {
                        let distance =
                            edge.windows(2).filter_map(|w| Some(point.dist_to_segment(*w.first()?, *w.get(1)?))).fold(f64::INFINITY, f64::min);
                        (distance <= solidcraft_doc::appearance::pick_tolerance(b)).then_some((index, distance))
                    })
                    .min_by(|(_, a), (_, b)| a.total_cmp(b))
                    .map(|(index, _)| index)
            };
            (index.ok_or_else(missing)?, point)
        }
        _ => return Err(bad("select.set", "a face or edge needs an index or a finite point")),
    };
    Ok(if face { Sel::Face { body, index, point } } else { Sel::Edge { body, index, point } })
}

fn select_set(s: &mut Session, p: &Value) -> Result<Value> {
    let items = p.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
    if items.len() > 10_000 {
        return Err(bad("select.set", "too many items"));
    }
    let mut sel: Vec<Sel> = Vec::new();
    for it in items {
        let one = selection_item(s, it)?;
        match &one {
            Sel::Occurrence { path } => {
                s.occurrence_bodies(path, true).map_err(|e| bad("select.set", e.to_string()))?;
            }
            // A selection may not name a body that isn't there: left selected it would delete
            // nothing, or the next body to take that name (#9737).
            other => {
                if let Some(name) = other.body_name()
                    && s.world_state().body(name).is_none()
                {
                    return Err(bad("select.set", format!("no body `{name}`")));
                }
            }
        }
        sel.push(one);
    }
    if !bool_(p, "add").unwrap_or(false) {
        s.selection.clear();
    }
    for x in sel {
        if !s.selection.contains(&x) {
            s.selection.push(x);
        }
    }
    s.refresh_display();
    Ok(json!({"selected": s.selection.len()}))
}

fn select_filter(_s: &mut Session, p: &Value) -> Result<Value> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Request {
        mode: String,
    }
    let request: Request = serde_json::from_value(p.clone()).map_err(|e| bad("select.filter", e.to_string()))?;
    if !matches!(request.mode.as_str(), "body" | "face" | "component") {
        return Err(bad("select.filter", "mode must be body, face or component"));
    }
    Ok(json!({"mode": request.mode}))
}

fn select_same_look(s: &mut Session, p: &Value) -> Result<Value> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Request {
        body: Option<String>,
        face: Option<usize>,
    }
    let request: Request = serde_json::from_value(p.clone()).map_err(|e| bad("select.same_look", e.to_string()))?;
    let selected = s.selection.last();
    let name = request
        .body
        .as_deref()
        .or(match selected {
            Some(Sel::Body { name }) => Some(name.as_str()),
            Some(Sel::Face { body, .. }) => Some(body.as_str()),
            _ => None,
        })
        .ok_or_else(|| bad("select.same_look", "select an object or face"))?;
    let state = s.world_state();
    let body = state.body(name).ok_or_else(|| bad("select.same_look", "unknown object"))?;
    let face = request.face.or(match selected {
        Some(Sel::Face { body, index, .. }) if request.body.is_none() && body == name => Some(*index),
        _ => None,
    });
    if let Some(face) = face
        && face >= body.body.face_count()
    {
        return Err(bad("select.same_look", "unknown face"));
    }
    let look = face.map_or_else(|| s.doc.model_display_look(body), |face| s.doc.model_face_look(body, face));
    let items: Vec<_> =
        state.bodies.iter().filter(|b| s.doc.model_used_looks(b).contains(&look)).take(10_001).map(|b| Sel::Body { name: b.name.clone() }).collect();
    if items.len() > 10_000 {
        return Err(bad("select.same_look", "too many objects"));
    }
    select_set(s, &json!({"items":items}))
}

fn select_clear(s: &mut Session, _p: &Value) -> Result<Value> {
    s.selection.clear();
    s.refresh_display();
    Ok(json!({"selected": 0}))
}

/// Features that worked in `before` and fail in `after`: (name, error).
fn newly_broken(before: &solidcraft_doc::Model, after: &solidcraft_doc::Model) -> Vec<(String, String)> {
    after
        .results
        .iter()
        .filter_map(|r| {
            let e = r.error.clone()?;
            let was_ok = before.result(r.id).is_some_and(|b| b.error.is_none() && !b.skipped);
            was_ok.then(|| (r.name.clone(), e))
        })
        .collect()
}

/// [`newly_broken`] over the whole history, from `before` (the model of `before_doc`) to `after`
/// (the model of `after_doc`). With the history rolled back, the features after the marker are
/// evaluated too, on copies of the models (which share their cache): a delete made in the
/// rolled-back history that breaks them says so before it is made, rather than leaving error marks
/// that only show at the end of the timeline (#3471). A change that leaves them the same inputs
/// needs no such evaluation ([`only_constraints_changed`]).
pub(super) fn newly_broken_in_history(
    before_doc: &solidcraft_doc::Document,
    before: &solidcraft_doc::Model,
    after_doc: &solidcraft_doc::Document,
    after: &solidcraft_doc::Model,
) -> Vec<(String, String)> {
    if (before_doc.marker.is_none() && after_doc.marker.is_none()) || only_constraints_changed(before_doc, before, after_doc, after) {
        return newly_broken(before, after);
    }
    let whole = |doc: &solidcraft_doc::Document, model: &solidcraft_doc::Model| {
        let mut doc = doc.clone();
        doc.marker = None;
        // A copy keeps nothing on its own: it keeps what the model keeps, so a delete checked
        // ahead finds this build there when it runs (#9917).
        let mut copy = model.clone();
        copy.keep(model.kept());
        copy.evaluate(&doc);
        copy
    };
    // Only a feature that fails after the change can be newly broken: when none does, the
    // history before it needn't be built again (#9917: each build ran every boolean after the
    // marker, half a minute on a large design).
    let after = whole(after_doc, after);
    if after.results.iter().all(|r| r.error.is_none()) {
        return Vec::new();
    }
    newly_broken(&whole(before_doc, before), &after)
}

/// Whether `after_doc` differs from `before_doc` only in sketch constraints and dimensions (and
/// model parameters nothing uses any more), every sketch whose constraints changed solving to
/// the same geometry, profiles and ends as before. The features after the marker are then built
/// from the same inputs, so evaluating the whole history twice would find nothing new (#7253):
/// deleting one dimension of a sketch with a long rolled-back history after it took minutes.
fn only_constraints_changed(
    before_doc: &solidcraft_doc::Document,
    before: &solidcraft_doc::Model,
    after_doc: &solidcraft_doc::Document,
    after: &solidcraft_doc::Model,
) -> bool {
    let without_constraints = |doc: &solidcraft_doc::Document| {
        let mut doc = doc.clone();
        for f in &mut doc.features {
            if let FeatureKind::Sketch { sketch, .. } = &mut f.kind {
                sketch.constraints.clear();
            }
        }
        doc
    };
    let (mut b, a) = (without_constraints(before_doc), without_constraints(after_doc));
    if b.params.iter().any(|p| !a.params.contains(p) && (!p.model || !after_doc.param_users(&p.name).is_empty())) {
        return false;
    }
    b.params.retain(|p| a.params.contains(p));
    if b != a {
        return false;
    }
    // What a later feature reads of a sketch: the solved geometry, its plane, profiles and ends.
    let solved = |model: &solidcraft_doc::Model, id: u64| {
        let r = model.result(id).filter(|r| !r.skipped && r.error.is_none())?;
        let s = r.state.sketch(id)?;
        let mut sketch = s.sketch.clone();
        sketch.constraints.clear();
        Some((s.plane, sketch, serde_json::to_value(&s.profiles).ok()?, s.ends.clone(), s.report.ok()))
    };
    before_doc.features.iter().zip(&after_doc.features).all(|(fb, fa)| match (&fb.kind, &fa.kind) {
        (FeatureKind::Sketch { sketch: sb, .. }, FeatureKind::Sketch { sketch: sa, .. }) if sb.constraints != sa.constraints => {
            solved(before, fb.id).is_some_and(|x| solved(after, fa.id).is_some_and(|y| x == y))
        }
        _ => true,
    })
}

fn reorder(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "timeline.reorder";
    let id = feature_id(s, p.get("feature"), cmd)?;
    let n = s.doc.features.len();
    let to = num(p, "position").map(|x| x.max(0.0) as usize).ok_or_else(|| bad(cmd, "`position` must be a number"))?.min(n.saturating_sub(1));
    let from = s.doc.feature_index(id).ok_or_else(|| bad(cmd, "feature"))?;
    let active = active_param(p, cmd)?;
    if from == to && active.is_none() {
        return Ok(json!({"feature": id, "position": to}));
    }
    let mut next = (*s.doc).clone();
    let f = next.features.remove(from);
    let name = f.name.clone();
    next.features.insert(to, f);
    sort_group_features(&mut next);
    require_contiguous_groups(&next, cmd)?;
    // Everything that evaluated before must still evaluate.
    let mut after = s.model.clone();
    after.evaluate(&next);
    let broken = newly_broken(&s.model, &after);
    // `allow_errors`: made anyway, with what it broke said (#5962).
    let allow = allow_errors(p, cmd)?;
    if !broken.is_empty() && !allow {
        let list: Vec<String> = broken.iter().map(|(n, e)| format!("{n} ({e})")).collect();
        return Err(bad(cmd, format!("moving {name} there breaks {}", list.join("; "))));
    }
    let marker = next.marker;
    place_marker(s, &mut next, &[id], active, cmd)?;
    if next != *s.doc {
        // The check evaluated the new order: keep its results, so setting the design doesn't
        // evaluate the moved part of the timeline a second time (#6186). Placing the marker
        // elsewhere makes them stale.
        if next.marker == marker {
            s.model = after;
        }
        *s.doc_mut() = next;
    }
    let mut out = json!({"feature": id, "position": to});
    if allow {
        out["broken"] = broken_json(&broken);
    }
    Ok(out)
}

/// The optional `allow_errors` of a move: make it even when it breaks features that worked.
fn allow_errors(p: &Value, cmd: &str) -> Result<bool> {
    match p.get("allow_errors") {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(allow)) => Ok(*allow),
        Some(_) => Err(bad(cmd, "`allow_errors` must be true or false")),
    }
}

/// The features a move broke, as `[{feature, error}]`.
fn broken_json(broken: &[(String, String)]) -> Value {
    broken.iter().map(|(feature, error)| json!({"feature": feature, "error": error})).collect()
}

/// The optional `active` of a move: whether the moved features end up before the history marker.
fn active_param(p: &Value, cmd: &str) -> Result<Option<bool>> {
    match p.get("active") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(active)) => Ok(Some(*active)),
        Some(_) => Err(bad(cmd, "`active` must be true or false")),
    }
}

/// With `active` given, put the history marker of the reordered `next` so the `moving` features
/// are active (before it) or rolled back (behind it) while every other feature keeps its state, in
/// the same step as the move. Refused when `position` puts them on the other side of it.
fn place_marker(s: &Session, next: &mut solidcraft_doc::Document, moving: &[u64], active: Option<bool>, cmd: &str) -> Result<()> {
    let Some(active) = active else { return Ok(()) };
    let n = s.doc.features.len();
    let was_active: std::collections::BTreeSet<u64> = s.doc.features.iter().take(s.doc.marker.unwrap_or(n)).map(|feature| feature.id).collect();
    let stays_active = |id: u64| if moving.contains(&id) { active } else { was_active.contains(&id) };
    let count = next.features.iter().filter(|feature| stays_active(feature.id)).count();
    if !next.features.iter().take(count).all(|feature| stays_active(feature.id)) {
        let side = if active { "before" } else { "behind" };
        return Err(bad(cmd, format!("`position` is not {side} the history marker, as `active: {active}` asks")));
    }
    next.marker = (count < next.features.len()).then_some(count);
    Ok(())
}

fn reorder_many(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "timeline.reorder_many";
    let ids = group_features(s, p, true, cmd)?;
    let slot = num(p, "position").map(|x| x.max(0.0) as usize).ok_or_else(|| bad(cmd, "`position` must be a number"))?;
    let active = active_param(p, cmd)?;
    let allow = allow_errors(p, cmd)?;
    let (mut next, ordered, broken, checked) = reorder_features(s, (*s.doc).clone(), &ids, slot, allow, cmd)?;
    let marker = next.marker;
    place_marker(s, &mut next, &ordered, active, cmd)?;
    // The checked results stand only when placing the marker left the design as they were made for.
    let checked = checked.filter(|_| next.marker == marker);
    if next != *s.doc {
        if let Some(model) = checked {
            s.model = model;
        }
        *s.doc_mut() = next;
    }
    let mut out = json!({"features": ordered, "position": slot.min(s.doc.features.len())});
    if allow {
        out["broken"] = broken_json(&broken);
    }
    Ok(out)
}

/// The reordered design, the moved features in order, and the model the move was checked on
/// (none when the order didn't change), for the caller to keep (#6186).
fn reorder_features(
    s: &Session,
    mut next: solidcraft_doc::Document,
    ids: &[u64],
    slot: usize,
    allow_errors: bool,
    cmd: &str,
) -> Result<(solidcraft_doc::Document, Vec<u64>, Vec<(String, String)>, Option<solidcraft_doc::Model>)> {
    let n = next.features.len();
    let slot = slot.min(n);
    let selected: std::collections::BTreeSet<u64> = ids.iter().copied().collect();
    let ordered: Vec<u64> = next.features.iter().filter(|feature| selected.contains(&feature.id)).map(|feature| feature.id).collect();
    let before_slot = next.features.iter().take(slot).filter(|feature| selected.contains(&feature.id)).count();
    let insert_at = slot.saturating_sub(before_slot).min(n.saturating_sub(ordered.len()));

    let mut moving = Vec::new();
    next.features.retain(|feature| {
        if selected.contains(&feature.id) {
            moving.push(feature.clone());
            false
        } else {
            true
        }
    });
    let unchanged = s.doc.features.iter().map(|feature| feature.id).eq(next.features[..insert_at]
        .iter()
        .chain(moving.iter())
        .chain(next.features[insert_at..].iter())
        .map(|feature| feature.id));
    next.features.splice(insert_at..insert_at, moving);
    sort_group_features(&mut next);
    require_contiguous_groups(&next, cmd)?;
    if unchanged {
        return Ok((next, ordered, Vec::new(), None));
    }
    // From the session's own results: the features before the first one moved are kept, not
    // evaluated again from an empty model (#6186).
    let mut after = s.model.clone();
    after.evaluate(&next);
    let broken = newly_broken(&s.model, &after);
    if !broken.is_empty() && !allow_errors {
        let list: Vec<String> = broken.iter().map(|(name, error)| format!("{name} ({error})")).collect();
        return Err(bad(cmd, format!("moving the selected features there breaks {}", list.join("; "))));
    }
    Ok((next, ordered, broken, Some(after)))
}

fn input_keys(kind: &FeatureKind) -> Vec<(String, solidcraft_doc::expr::Kind)> {
    let mut keys: Vec<_> = kind.inputs().into_iter().map(|(label, _, unit)| (label.to_string(), unit)).collect();
    if let FeatureKind::Fillet { style, .. } = kind {
        if let Some((key, _)) = keys.first_mut() {
            *key = "fillet.radius".into();
        }
        match style {
            solidcraft_doc::FilletStyle::Variable { mid, .. } => {
                for ((key, _), (position, _)) in keys.iter_mut().skip(2).zip(mid) {
                    *key = format!("fillet.mid.{}", position.to_bits());
                }
            }
            solidcraft_doc::FilletStyle::Spline { .. } => {
                for (i, (key, _)) in keys.iter_mut().enumerate().skip(1) {
                    *key = format!("fillet.control.{i}");
                }
            }
            solidcraft_doc::FilletStyle::Constant | solidcraft_doc::FilletStyle::Chord | solidcraft_doc::FilletStyle::Curvature => {}
        }
    }
    keys
}

fn redefine(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "timeline.redefine";
    let id = feature_id(s, p.get("feature"), cmd)?;
    let cid = str_(p, "command").ok_or_else(|| bad(cmd, "`command` must name the feature's command"))?;
    let spec = super::find_command(cid).ok_or_else(|| bad(cmd, format!("no command `{cid}`")))?;
    // Only modeling constructors may run here. In particular, a scratch session does not
    // isolate filesystem writes performed by export commands.
    let constructor = super::features::COMMANDS
        .iter()
        .chain(super::features_more::COMMANDS)
        .chain(super::construct::COMMANDS)
        .chain(super::surface::COMMANDS)
        .chain(super::surface_boundary::COMMANDS)
        .any(|candidate| candidate.id == spec.id)
        || matches!(
            spec.id,
            "sheet.flange"
                | "sheet.hem"
                | "sheet.perforate"
                | "sheet.fold"
                | "sheet.path_flange"
                | "sheet.unfold"
                | "sheet.refold"
                | "sheet.convert"
                | "plastic.boss"
                | "plastic.lip"
                | "plastic.snap_fit"
                | "plastic.rest"
                | "frame.member"
        );
    if !constructor {
        return Err(bad(cmd, format!("`{cid}` does not make a feature")));
    }
    let mut params = p.get("params").cloned().unwrap_or_else(|| json!({}));
    let idx = s.doc.feature_index(id).ok_or_else(|| bad(cmd, "feature"))?;
    let old = s.doc.feature(id).cloned().ok_or_else(|| bad(cmd, "feature"))?;
    if let FeatureKind::Extrude { extent, .. } | FeatureKind::SurfaceExtrude { extent, .. } = &old.kind
        && matches!(cid, "solid.extrude" | "surface.extrude")
        && let Some(parameters) = params.as_object_mut()
    {
        let convention = if cid == "surface.extrude" || extent.direction == solidcraft_doc::Direction::Symmetric {
            extent.convention
        } else {
            solidcraft_doc::ExtrudeConvention::TotalDistance
        };
        parameters.entry("convention").or_insert_with(|| json!(convention));
    }
    if let FeatureKind::Revolve { convention, .. } = &old.kind
        && cid == "solid.revolve"
        && let Some(parameters) = params.as_object_mut()
    {
        parameters.entry("convention").or_insert_with(|| json!(convention));
    }
    // Members keep their names through an edit, unless it gives them another name.
    if let FeatureKind::FrameMember { names, body_name, .. } = &old.kind
        && cid == "frame.member"
        && let Some(parameters) = params.as_object_mut()
        && parameters.get("body_name").and_then(Value::as_str) == body_name.as_deref()
    {
        parameters.entry("names").or_insert_with(|| json!(names));
    }
    if !old.participants.is_empty()
        && let Some(parameters) = params.as_object_mut()
    {
        parameters.entry("participants").or_insert_with(|| json!(old.participants));
    }
    // Run the constructor on a scratch session: failed edits must not disturb sketch mode,
    // selections, component authoring state or the original feature's definition.
    let mut trial = s.scratch();
    // What the trial computes is kept for the session's own evaluation at the end: the cache may
    // hold only the last few states of a large design, which would be computed twice.
    let kept = s.model.kept();
    let shared = kept.clone().unwrap_or_default();
    trial.model.keep(Some(shared.clone()));
    trial.active_component = old.component;
    if let FeatureKind::Emboss { face: Some(face), .. } = &old.kind {
        trial.active_occurrence_path = (!face.author_path.is_empty()).then(|| face.author_path.clone());
    }
    trial.active_sketch = None;
    trial.selection = s.selection.clone();
    // Keep the original identity as a suppressed placeholder. The constructor and
    // any ownership boundary it adds go AFTER this slot, so discarding its temporary
    // feature leaves auxiliary history after the rewritten original.
    if let Some(slot) = trial.doc_mut().feature_mut(id) {
        slot.suppressed = true;
    }
    trial.doc_mut().marker = Some(idx + 1);
    trial.refresh();
    let r = spec.run_nested(&mut trial, &params)?;
    let made = r.get("feature").and_then(Value::as_u64).ok_or_else(|| bad(cmd, format!("`{cid}` did not make a feature")))?;
    let mut new = trial.doc.feature(made).cloned().ok_or_else(|| bad(cmd, format!("`{cid}` did not make a feature")))?;
    if let (FeatureKind::Emboss { face: Some(before), .. }, FeatureKind::Emboss { face: Some(after), .. }) = (&old.kind, &mut new.kind)
        && before.body == after.body
        && before.path == after.path
        && before.author_path == after.author_path
        && before.at.dist(after.at.0) < 1e-8
    {
        *after = before.clone();
    }
    // A boundary surface's loop of sketch curves and its curve network are one command's two
    // forms: an edit may turn one into the other.
    let boundary = |k: &FeatureKind| matches!(k, FeatureKind::SurfaceBoundary { .. } | FeatureKind::SurfaceNetwork { .. });
    if std::mem::discriminant(&new.kind) != std::mem::discriminant(&old.kind) && !(boundary(&new.kind) && boundary(&old.kind)) {
        return Err(bad(cmd, format!("{} is not a {} feature", old.name, cid)));
    }
    // The trial still holds the old feature, so a plane offset from its own name was made there,
    // and once swapped in it would be built on itself (#3567).
    if new.kind.names_feature(old.id, &old.name) {
        return Err(bad(cmd, format!("{} can't be built on itself: pick another plane or face", old.name)));
    }
    // Optional inputs can move other inputs' positions (e.g. Distance 2 goes before Taper).
    // Keep each existing model parameter attached to its semantic input, rather than its
    // old index. Constructor-assigned names for new inputs cannot reuse the old dimensions.
    let mut old_inputs: Vec<_> = input_keys(&old.kind).into_iter().zip(&old.param_names).collect();
    let param_names: Vec<String> = input_keys(&new.kind)
        .iter()
        .enumerate()
        .map(|(i, (label, kind))| {
            if let Some(k) = old_inputs.iter().position(|((old_label, old_kind), _)| old_label == label && old_kind == kind) {
                old_inputs.remove(k).1.clone()
            } else {
                new.param_names.get(i).cloned().unwrap_or_default()
            }
        })
        .collect();
    trial.doc_mut().features.retain(|f| f.id != made);
    let added = trial.doc.features.len().saturating_sub(s.doc.features.len());
    trial.doc_mut().marker = s.doc.marker.map(|marker| if marker > idx { marker.saturating_add(added) } else { marker });
    if made == s.doc.next_id && trial.doc.next_id == made.saturating_add(1) {
        trial.doc_mut().next_id = s.doc.next_id;
    }
    if let Some(slot) = trial.doc_mut().feature_mut(id) {
        slot.suppressed = old.suppressed;
        slot.kind = new.kind;
        slot.edge_refs = new.edge_refs;
        slot.edge_names = new.edge_names;
        slot.face_names = new.face_names;
        slot.param_names = param_names;
        if params.get("participants").is_some() {
            slot.participants = new.participants;
        }
    }
    // New optional inputs need model parameter names, while existing dimensions keep theirs.
    trial.doc_mut().name_feature_inputs(id);
    trial.refresh();
    if let Some(e) = trial.model.result(id).and_then(|r| r.error.clone()) {
        return Err(EngineError::Other(e));
    }
    // What this edit rebuilt: the trial computed the edited feature and the features after it. The
    // session's evaluation below only takes those results from what the trial kept, so its own
    // counters read as "nothing was rebuilt" (#5694).
    let rebuilt = trial.model.last_recomputed;
    let reused = trial.model.last_restored;
    let original_ids: std::collections::BTreeSet<u64> = s.doc.features.iter().map(|feature| feature.id).collect();
    let added_ids: std::collections::BTreeSet<u64> =
        trial.doc.features.iter().filter(|feature| !original_ids.contains(&feature.id)).map(|feature| feature.id).collect();
    let adds_transfer =
        trial.doc.features.iter().any(|feature| added_ids.contains(&feature.id) && matches!(feature.kind, FeatureKind::BodyTransfer { .. }));
    let baseline = if adds_transfer && s.doc.marker.is_some() {
        let mut baseline = s.scratch();
        baseline.doc_mut().marker = None;
        baseline.refresh();
        Some(baseline)
    } else {
        None
    };
    let proof = if (adds_transfer && trial.doc.marker.is_some()) || added_ids.iter().any(|id| trial.model.result(*id).is_none()) {
        let mut proof = trial.scratch();
        proof.doc_mut().marker = None;
        proof.refresh();
        Some(proof)
    } else {
        None
    };
    let validated = proof.as_ref().map(|proof| &proof.model).unwrap_or(&trial.model);
    let original_model = baseline.as_ref().map(|baseline| &baseline.model).unwrap_or(&s.model);
    for feature in &trial.doc.features {
        let auxiliary = added_ids.contains(&feature.id);
        let protected_transfer = matches!(feature.kind, FeatureKind::BodyTransfer { .. })
            && original_model.result(feature.id).is_some_and(|result| !result.skipped && result.error.is_none());
        if (auxiliary || protected_transfer)
            && let Some(error) = validated.result(feature.id).and_then(|result| result.error.as_ref())
        {
            return Err(bad(cmd, format!("{}: {error}", feature.name)));
        }
    }
    s.pending_moves = trial.pending_moves;
    s.doc = trial.doc;
    s.model.keep(Some(shared));
    s.refresh();
    s.model.keep(kept);
    if let Some(e) = s.model.result(id).and_then(|r| r.error.clone()) {
        return Err(EngineError::Other(e));
    }
    // A feature behind the history marker is not built, so the edit changes nothing in the model
    // yet: the reply says so, or the edit would look as if it had silently failed (#8100). Its
    // warning is the one the constructor met building it in the trial, at its place.
    let rolled_back = s.doc.marker.is_some_and(|marker| s.doc.feature_index(id).is_some_and(|index| index >= marker));
    let warning =
        if rolled_back { r.get("warning").and_then(Value::as_str).map(str::to_string) } else { s.model.result(id).and_then(|r| r.warning.clone()) };
    // Report the trial's rebuild, plus anything the session still had to compute itself (a kept
    // result its budget dropped); `s.model`'s own counters would only report the reuse (#5694).
    let mut reply = json!({"feature": id, "recomputed": rebuilt + s.model.last_recomputed, "restored": reused, "rolled_back": rolled_back});
    if let Some(warning) = warning {
        reply["warning"] = json!(warning);
    }
    if rolled_back {
        let name = s.doc.feature(id).map(|f| f.name.clone()).unwrap_or_default();
        reply["note"] = json!(format!("{name} is after the history marker, so it is not built: move the marker past it to build it"));
    }
    Ok(reply)
}

fn dependents(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "timeline.dependents";
    let id = feature_id(s, p.get("feature"), cmd)?;
    let mut next = (*s.doc).clone();
    let gone = next.delete_feature(id)?;
    // On a copy of the session's model, which shares its evaluation cache (#2678): the features
    // before this one are reused rather than recomputed, and the delete that usually follows
    // finds the ones after it already computed.
    let mut after = s.model.clone();
    after.evaluate(&next);
    let names = |ids: &[u64]| -> Vec<String> { ids.iter().filter(|x| **x != id).filter_map(|x| s.doc.feature(*x).map(|f| f.name.clone())).collect() };
    let broken: Vec<String> = newly_broken_in_history(&s.doc, &s.model, &next, &after).into_iter().map(|(n, _)| n).collect();
    Ok(json!({"deleted_with_it": names(&gone), "would_fail": broken}))
}

#[cfg(test)]
#[path = "edit_tests.rs"]
mod tests;
