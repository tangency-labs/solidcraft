//! Display state commands: save what is shown (and how it looks) under a name, apply, rename,
//! delete and list them. They are saved with the design (see `crate::display_state`).

use serde_json::{Value, json};

use super::CommandSpec;
use solidcraft_doc::display_state::NamedDisplayState;

use crate::params::{bad, str_};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("display.save", "New Display State", save).icon("list").params(
        "name? (replaces the state of that name, keeping its place in the list). Saves what is shown now and how the bodies look (colours, appearances); the camera is not stored, Named Views do that",
    ),
    CommandSpec::new("display.apply", "Apply Display State", apply).icon("list").noundo().params(
        "state: name. Shows what the state shows (missing bodies are ignored, bodies added since are shown) and the looks it stored. The camera is not moved. View state: not an undo step",
    ),
    CommandSpec::new("display.rename", "Rename Display State", rename).icon("rename").params("state: name, name"),
    CommandSpec::new("display.delete", "Delete Display State", delete).icon("delete").params("state: name"),
    CommandSpec::new("display.list", "List Display States", list).icon("list").noundo().doc_only(),
];

fn state_index(s: &Session, p: &Value, cmd: &str) -> Result<usize> {
    let k = str_(p, "state").ok_or_else(|| bad(cmd, "`state` must be a display state name"))?;
    s.doc.display_states.iter().position(|v| v.name == k).ok_or_else(|| bad(cmd, format!("no display state `{k}`")))
}

fn save(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "display.save";
    let n = s.doc.display_states.len();
    let name = match str_(p, "name").map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) if n.len() <= 128 => n.to_string(),
        Some(_) => return Err(bad(cmd, "`name` is too long")),
        None => (n + 1..).map(|k| format!("Display State{k}")).find(|x| !s.doc.display_states.iter().any(|v| v.name == *x)).unwrap_or_default(),
    };
    if n >= 1000 && !s.doc.display_states.iter().any(|v| v.name == name) {
        return Err(bad(cmd, "too many display states"));
    }
    let snapshot = s.capture_visibility();
    let looks = s.doc.appearances.clone();
    let d = s.doc_mut();
    // Updating a state keeps its place in the list.
    let state = NamedDisplayState { name: name.clone(), snapshot, appearances: Some(looks) };
    match d.display_states.iter().position(|x| x.name == name) {
        Some(i) => d.display_states[i] = state,
        None => d.display_states.push(state),
    }
    Ok(json!({ "state": name }))
}

fn apply(s: &mut Session, p: &Value) -> Result<Value> {
    let i = state_index(s, p, "display.apply")?;
    let state = s.doc.display_states[i].clone();
    s.apply_visibility(&state.snapshot);
    if let Some(looks) = state.appearances.as_ref().filter(|a| **a != s.doc.appearances) {
        s.doc_mut().appearances = looks.clone();
    }
    Ok(json!({ "state": state.name }))
}

fn rename(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "display.rename";
    let i = state_index(s, p, cmd)?;
    let name =
        str_(p, "name").map(str::trim).filter(|n| !n.is_empty() && n.len() <= 128).ok_or_else(|| bad(cmd, "`name` must be 1…128 characters"))?;
    if s.doc.display_states.iter().enumerate().any(|(j, v)| j != i && v.name == name) {
        return Err(bad(cmd, format!("another display state is called `{name}`")));
    }
    if let Some(v) = s.doc_mut().display_states.get_mut(i) {
        v.name = name.to_string();
    }
    Ok(json!({ "state": name }))
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let i = state_index(s, p, "display.delete")?;
    let v = s.doc_mut().display_states.remove(i);
    Ok(json!({ "deleted": v.name }))
}

fn list(s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(json!({ "states": s.doc.display_states }))
}
