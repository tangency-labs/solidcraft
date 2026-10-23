//! Versions and branches kept inside the design file.
//!
//! `version.create` freezes the open workspace. `version.branch` starts another workspace from
//! a version. `version.switch` opens a workspace, keeping the one that was open. `version.restore`
//! replaces the open workspace with a version. `version.graph` only describes the graph.

use std::path::Path;

use serde_json::{Value, json};
use solidcraft_doc::versions::linked_file_version;

use super::CommandSpec;
use crate::params::{bad, str_};
use crate::{EngineError, Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("version.graph", "Versions", graph)
        .icon("checkpoint")
        .noundo()
        .doc_only()
        .params("→ current workspace, workspaces (id, name, from, open) and versions (id, name, parent, workspace)"),
    CommandSpec::new("version.create", "Create Version", create).icon("checkpoint").doc_only().params(
        "name: the version's name. The open workspace stays as it is. A linked design records the version that file is on, in this snapshot only",
    ),
    CommandSpec::new("version.branch", "New Branch", branch)
        .icon("checkpoint")
        .doc_only()
        .params("name: the workspace's name; version?: a version to start from (omit to copy the open workspace as it is now). The workspace that was open is kept, and the new one is opened"),
    CommandSpec::new("version.switch", "Open Workspace", switch)
        .icon("checkpoint")
        .doc_only()
        .params("workspace: id. The workspace that was open is kept, and this one is opened"),
    CommandSpec::new("version.restore", "Restore Version", restore)
        .icon("checkpoint")
        .doc_only()
        .params("version: id. Replaces the open workspace with that version. The version stays in the graph"),
    CommandSpec::new("version.view", "View Version", view)
        .icon("checkpoint")
        .noundo()
        .doc_only()
        .params("version: id. Shows that version in the window. The open workspace is kept and is what a save writes"),
    CommandSpec::new("version.return", "Back to Workspace", return_from)
        .icon("checkpoint")
        .noundo()
        .doc_only()
        .params("Closes a version that was only being viewed and shows the workspace again"),
    CommandSpec::new("version.rename", "Rename", rename).icon("checkpoint").doc_only().params(
        "name; version?: id, or workspace?: id. The other names in the graph stay unique",
    ),
    CommandSpec::new("version.delete", "Delete", delete).icon("checkpoint").doc_only().params(
        "version?: id, or workspace?: id. Main, and the workspace that is open, cannot be deleted. A version's children start from the version before it",
    ),
    CommandSpec::new("version.changes", "Changes", changes).icon("checkpoint").noundo().doc_only().params(
        "version?: id, or workspace?: id → added, removed, changed and conflicting parameters and features",
    ),
    CommandSpec::new("version.merge", "Merge into here", merge)
        .icon("checkpoint")
        .doc_only()
        .params("workspace: id; take?: paths to take from that workspace; keep?: paths to keep from the open one. A conflict with no choice is refused"),
];

fn graph(s: &mut Session, _p: &Value) -> Result<Value> {
    let versions = &s.doc.versions;
    Ok(json!({
        "current": versions.current,
        "workspaces": versions.workspaces.iter().map(|workspace| json!({
            "id": workspace.id,
            "name": workspace.name,
            "from": workspace.from,
            "open": workspace.id == versions.current,
        })).collect::<Vec<_>>(),
        "versions": versions.versions.iter().map(|version| json!({
            "id": version.id,
            "name": version.name,
            "parent": version.parent,
            "workspace": version.workspace,
        })).collect::<Vec<_>>(),
    }))
}

fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let name = required(p, "name")?;
    let path = s.path.clone();
    let id = s.doc_mut().create_version(name, |link| read_link(&path, link)).map_err(into_engine)?;
    Ok(json!({ "version": id }))
}

fn branch(s: &mut Session, p: &Value) -> Result<Value> {
    let name = required(p, "name")?;
    let path = s.path.clone();
    let id = if let Some(version) = str_(p, "version").filter(|text| !text.trim().is_empty()) {
        s.doc_mut().branch(name, version).map_err(into_engine)?
    } else {
        s.doc_mut().branch_from_here(name, |link| read_link(&path, link)).map_err(into_engine)?
    };
    leave(s);
    Ok(json!({ "workspace": id }))
}

fn switch(s: &mut Session, p: &Value) -> Result<Value> {
    let id = required(p, "workspace")?;
    s.doc_mut().switch_workspace(id).map_err(into_engine)?;
    leave(s);
    Ok(json!({ "workspace": id }))
}

fn rename(s: &mut Session, p: &Value) -> Result<Value> {
    let name = required(p, "name")?;
    let id = one_of(p)?;
    s.doc_mut().rename_node(id, name).map_err(into_engine)?;
    Ok(json!({ "id": id, "name": name }))
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(id) = str_(p, "version").filter(|text| !text.trim().is_empty()) {
        s.doc_mut().delete_version(id).map_err(into_engine)?;
        return Ok(json!({ "version": id }));
    }
    let id = required(p, "workspace")?;
    s.doc_mut().delete_workspace(id).map_err(into_engine)?;
    Ok(json!({ "workspace": id }))
}

fn changes(s: &mut Session, p: &Value) -> Result<Value> {
    let listed = if let Some(id) = str_(p, "version").filter(|text| !text.trim().is_empty()) {
        s.doc.changes_for_version(id).map_err(into_engine)?
    } else {
        s.doc.changes_for_workspace(required(p, "workspace")?).map_err(into_engine)?
    };
    Ok(serde_json::to_value(&listed).map_err(|error| EngineError::Other(error.to_string()))?)
}

fn chosen(p: &Value, key: &str, theirs: bool) -> Vec<(String, bool)> {
    p.get(key).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(|path| (path.to_string(), theirs)).collect()
}

fn merge(s: &mut Session, p: &Value) -> Result<Value> {
    let id = required(p, "workspace")?;
    let mut choices = chosen(p, "take", true);
    choices.extend(chosen(p, "keep", false));
    let refs: Vec<(&str, bool)> = choices.iter().map(|(path, take)| (path.as_str(), *take)).collect();
    let kept = s.doc.clone();
    let outcome = s.doc_mut().merge_workspace(id, &refs).map_err(into_engine)?;
    if outcome == solidcraft_doc::versions::MergeOutcome::Already {
        s.doc = kept;
    } else {
        leave(s);
    }
    let result = match outcome {
        solidcraft_doc::versions::MergeOutcome::BroughtIn => "brought",
        solidcraft_doc::versions::MergeOutcome::Merged => "merged",
        solidcraft_doc::versions::MergeOutcome::Already => "already",
    };
    Ok(json!({ "workspace": id, "result": result }))
}

fn one_of<'a>(p: &'a Value) -> Result<&'a str> {
    str_(p, "version")
        .filter(|text| !text.trim().is_empty())
        .or_else(|| str_(p, "workspace").filter(|text| !text.trim().is_empty()))
        .ok_or_else(|| bad("version", "`version` or `workspace` is required"))
}

fn restore(s: &mut Session, p: &Value) -> Result<Value> {
    let id = required(p, "version")?;
    s.doc_mut().restore_version(id).map_err(into_engine)?;
    leave(s);
    Ok(json!({ "version": id }))
}

fn view(s: &mut Session, p: &Value) -> Result<Value> {
    let id = required(p, "version")?;
    let parked = s.version_view.as_ref().map(|view| view.parked.clone()).unwrap_or_else(|| s.doc.clone());
    let name = parked
        .versions
        .versions
        .iter()
        .find(|version| version.id == id)
        .map(|version| version.name.clone())
        .ok_or_else(|| bad("version.view", format!("no version {id}")))?;
    let mut shown = (*parked).clone();
    shown.restore_version(id).map_err(into_engine)?;
    s.doc = std::sync::Arc::new(shown);
    s.version_view = Some(crate::VersionView { version: id.to_string(), name, parked });
    leave(s);
    Ok(json!({ "version": id, "viewing": true }))
}

fn return_from(s: &mut Session, _p: &Value) -> Result<Value> {
    let Some(view) = s.version_view.take() else { return Ok(json!({ "viewing": false })) };
    s.doc = view.parked;
    leave(s);
    Ok(json!({ "viewing": false }))
}

fn required<'a>(p: &'a Value, key: &str) -> Result<&'a str> {
    str_(p, key).filter(|text| !text.trim().is_empty()).ok_or_else(|| bad("version", format!("`{key}` is required")))
}

fn into_engine(error: solidcraft_doc::DocError) -> EngineError {
    EngineError::Other(error.to_string())
}

fn leave(s: &mut Session) {
    s.active_sketch = None;
    s.selection.clear();
}

/// The version recorded for a linked design, when its file can be read.
fn read_link(design: &Option<String>, link: &str) -> Option<String> {
    let path = resolve(design.as_deref(), link);
    let bytes = std::fs::read(&path).ok()?;
    linked_file_version(&bytes)
}

fn resolve(design: Option<&str>, link: &str) -> std::path::PathBuf {
    let path = Path::new(link);
    match design.and_then(|file| Path::new(file).parent()) {
        Some(dir) if path.is_relative() => dir.join(path),
        _ => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        s.execute(id, &p).unwrap_or_else(|error| panic!("{id}: {error}"))
    }

    #[test]
    fn create_branch_switch_and_undo() {
        let mut s = Session::default();
        s.doc_mut().name = "First".into();
        let first = run(&mut s, "version.create", json!({"name": "Start"}))["version"].as_str().unwrap().to_string();
        s.doc_mut().name = "Second".into();
        run(&mut s, "version.branch", json!({"name": "Side", "version": first}));
        assert_eq!(s.doc.name, "First");
        assert_eq!(s.doc.versions.workspaces.len(), 2);
        run(&mut s, "version.switch", json!({"workspace": "main"}));
        assert_eq!(s.doc.name, "Second");
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc.versions.current, s.doc.versions.workspaces.iter().find(|w| w.name == "Side").unwrap().id);
        let graph = run(&mut s, "version.graph", json!({}));
        assert_eq!(graph["versions"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn viewing_a_version_leaves_the_workspace() {
        let mut s = Session::default();
        s.doc_mut().name = "First".into();
        let first = run(&mut s, "version.create", json!({"name": "Start"}))["version"].as_str().unwrap().to_string();
        s.doc_mut().name = "Second".into();
        run(&mut s, "version.view", json!({"version": first}));
        assert_eq!(s.doc.name, "First");
        assert_eq!(s.version_view.as_ref().unwrap().parked.name, "Second");
        assert!(s.execute("document.units", &json!({"units": "in"})).is_err(), "a viewed version is not edited");
        assert_eq!(s.design().name, "Second", "a save would write the workspace");
        run(&mut s, "version.return", json!({}));
        assert!(s.version_view.is_none());
        assert_eq!(s.doc.name, "Second");
        assert_eq!(s.doc.units, "mm");
    }
}
