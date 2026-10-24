//! Display commands: Isolate in layers, and display states. The layers are view state: never saved,
//! not undo steps; the states are saved with the design (see `crate::display_state`).

use serde_json::{Value, json};

use super::CommandSpec;
use solidcraft_doc::display_state::NamedDisplayState;

use super::browser::keys;
use crate::params::{bad, str_};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("display.isolate", "Isolate", isolate).icon("eye").noundo().params(
        "items?: [body names] (all other bodies are hidden), or paths?: [[occurrence ids]] (exact placements and what is under them). A new layer on top of any active one; an empty list does nothing. View state: not saved, not an undo step",
    ),
    CommandSpec::new("display.unisolate", "Exit Isolation", unisolate).icon("eye").noundo().params(
        "none; leaves the top layer: what was shown before it comes back exactly, parts hidden by hand while it was active included. Does nothing when not isolated",
    ),
    CommandSpec::new("display.unisolate_all", "Exit All Isolations", unisolate_all).icon("eye").noundo().params(
        "none; leaves every layer: the view from before the first isolation comes back exactly. Does nothing when not isolated",
    ),
    CommandSpec::new("display.save", "New Display State", save).icon("list").params(
        "name? (replaces the state of that name, keeping its place in the list). Saves what is shown now and how the bodies look (colours, appearances); while isolated it saves the isolated view. The camera is not stored, Named Views do that",
    ),
    CommandSpec::new("display.apply", "Apply Display State", apply).icon("list").noundo().params(
        "state: name. Shows what the state shows (missing bodies are ignored, bodies added since are shown) and the looks it stored. The camera is not moved. View state: not an undo step",
    ),
    CommandSpec::new("display.rename", "Rename Display State", rename).icon("rename").params("state: name, name"),
    CommandSpec::new("display.delete", "Delete Display State", delete).icon("delete").params("state: name"),
    CommandSpec::new("display.list", "List Display States", list).icon("list").noundo().doc_only(),
];

fn isolate(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "display.isolate";
    match (p.get("items"), p.get("paths")) {
        (Some(_), Some(_)) => Err(bad(cmd, "give `items` or `paths`, not both")),
        (Some(_), None) => {
            let items = keys(p, "items", cmd)?;
            if items.is_empty() {
                return Ok(json!({"isolated": 0, "level": s.isolation_depth()}));
            }
            let level = s.isolate_bodies(&items)?;
            Ok(json!({"isolated": items.len(), "level": level}))
        }
        (None, Some(value)) => {
            let list = value.as_array().filter(|a| a.len() <= 10_000).ok_or_else(|| bad(cmd, "`paths` must list at most 10000 occurrence paths"))?;
            if list.is_empty() {
                return Ok(json!({"isolated": 0, "level": s.isolation_depth()}));
            }
            let paths: Vec<Vec<u64>> = list
                .iter()
                .map(|value| {
                    let path = value
                        .as_array()
                        .filter(|a| !a.is_empty() && a.len() <= 64)
                        .and_then(|a| a.iter().map(Value::as_u64).collect::<Option<Vec<_>>>())
                        .ok_or_else(|| bad(cmd, "each path must list 1..64 occurrence ids"))?;
                    s.occurrence_placement(&path).map_err(|e| bad(cmd, e.to_string()))?;
                    Ok(path)
                })
                .collect::<Result<_>>()?;
            let level = s.isolate_paths(&paths)?;
            Ok(json!({"isolated": paths.len(), "level": level}))
        }
        (None, None) => Err(bad(cmd, "`items` or `paths` is required")),
    }
}

fn unisolate(s: &mut Session, _p: &Value) -> Result<Value> {
    let left = s.unisolate();
    Ok(json!({"left": left, "level": s.isolation_depth()}))
}

fn unisolate_all(s: &mut Session, _p: &Value) -> Result<Value> {
    let left = s.unisolate_all();
    Ok(json!({"left": left, "level": s.isolation_depth()}))
}

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
    Ok(json!({ "state": state.name, "level": s.isolation_depth() }))
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
