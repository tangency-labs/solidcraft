//! Display state commands: save what is shown (and how it looks) under a name, apply, rename,
//! delete and list them. They are saved with the design (see `crate::display_state`).
//!
//! Isolating (`browser.visibility` with `isolate`) stacks layers; `display.unisolate` and
//! `display.unisolate_all` leave them. The layers are view state, never saved with the design, and
//! go through undo with the visibility they belong to (see `crate::display_state`).
//!
//! Applying is an undo step like hiding or showing (`browser.visibility`): what is hidden is part
//! of the session's undo snapshot, so one undo puts back the visibility and the looks together.

use serde_json::Value;

use super::CommandSpec;
use solidcraft_doc::display_state::NamedDisplayState;

use crate::params::{bad, str_};
use crate::schema::display::{
    DisplayApplied, DisplayApplyParams, DisplayDeleteParams, DisplayDeleted, DisplayListParams, DisplayListResult, DisplayRenameParams,
    DisplayRenamed, DisplaySaveParams, DisplaySaved, DisplayUnisolateAllParams, DisplayUnisolateParams, DisplayUnisolated, DisplayUnisolatedAll,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const SAVE_CONTRACT: TypedContract<DisplaySaveParams> = TypedContract::new();
const APPLY_CONTRACT: TypedContract<DisplayApplyParams> = TypedContract::new();
const RENAME_CONTRACT: TypedContract<DisplayRenameParams> = TypedContract::new();
const DELETE_CONTRACT: TypedContract<DisplayDeleteParams> = TypedContract::new();
const LIST_CONTRACT: TypedContract<DisplayListParams> = TypedContract::new();
const UNISOLATE_CONTRACT: TypedContract<DisplayUnisolateParams> = TypedContract::new();
const UNISOLATE_ALL_CONTRACT: TypedContract<DisplayUnisolateAllParams> = TypedContract::new();

fn reply(cmd: &str, reply: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(reply).map_err(|e| bad(cmd, e.to_string()))
}

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("display.unisolate", "Exit Isolation", crate::checked!("display.unisolate", unisolate))
        .icon("eye")
        .contract(&UNISOLATE_CONTRACT)
        .output(output_schema_for::<DisplayUnisolated>)
        .params(
        "none; leaves the top isolation layer: what was shown before it comes back exactly, parts hidden by hand while it was active included. Does nothing when not isolated. Undone like browser.visibility, in one step",
    ),
    CommandSpec::new("display.unisolate_all", "Exit All Isolations", crate::checked!("display.unisolate_all", unisolate_all))
        .icon("eye")
        .contract(&UNISOLATE_ALL_CONTRACT)
        .output(output_schema_for::<DisplayUnisolatedAll>)
        .params(
        "none; leaves every isolation layer: the view from before the first isolation comes back exactly. Does nothing when not isolated. Undone like browser.visibility, in one step",
    ),
    CommandSpec::new("display.save", "New Display State", crate::checked!("display.save", save))
        .icon("list")
        .contract(&SAVE_CONTRACT)
        .output(output_schema_for::<DisplaySaved>)
        .params(
        "name? (replaces the state of that name, keeping its place in the list). Saves what is shown now and how the bodies look (colours, appearances), the isolated view included while isolated; the camera is not stored, Named Views do that",
    ),
    CommandSpec::new("display.apply", "Apply Display State", crate::checked!("display.apply", apply))
        .icon("list")
        .contract(&APPLY_CONTRACT)
        .output(output_schema_for::<DisplayApplied>)
        .params(
        "state: name. Shows what the state shows (missing bodies are ignored, bodies added since are shown) and the looks it stored. The camera is not moved. Saved with the design and undone like browser.visibility, in one step",
    ),
    CommandSpec::new("display.rename", "Rename Display State", crate::checked!("display.rename", rename))
        .icon("rename")
        .contract(&RENAME_CONTRACT)
        .output(output_schema_for::<DisplayRenamed>)
        .params("state: name, name"),
    CommandSpec::new("display.delete", "Delete Display State", crate::checked!("display.delete", delete))
        .icon("delete")
        .contract(&DELETE_CONTRACT)
        .output(output_schema_for::<DisplayDeleted>)
        .params("state: name"),
    CommandSpec::new("display.list", "List Display States", crate::checked!("display.list", list))
        .icon("list")
        .noundo()
        .contract(&LIST_CONTRACT)
        .output(output_schema_for::<DisplayListResult>)
        .doc_only(),
];

fn unisolate(s: &mut Session, _p: &Value) -> Result<Value> {
    let left = s.unisolate();
    reply("display.unisolate", DisplayUnisolated { left, level: s.isolation_depth(), param_warnings: Vec::new() })
}

fn unisolate_all(s: &mut Session, _p: &Value) -> Result<Value> {
    let left = s.unisolate_all();
    reply("display.unisolate_all", DisplayUnisolatedAll { left, level: s.isolation_depth(), param_warnings: Vec::new() })
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
    reply(cmd, DisplaySaved { state: name, param_warnings: Vec::new() })
}

fn apply(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "display.apply";
    let i = state_index(s, p, cmd)?;
    let state = s.doc.display_states[i].clone();
    // The visibility goes where `browser.visibility` puts it (the session's own lists), and the
    // looks into the design: the command's undo step covers both.
    s.apply_visibility(&state.snapshot);
    if let Some(looks) = state.appearances.as_ref().filter(|a| **a != s.doc.appearances) {
        s.doc_mut().appearances = looks.clone();
    }
    reply(cmd, DisplayApplied { state: state.name, param_warnings: Vec::new() })
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
    reply(cmd, DisplayRenamed { state: name.to_string(), param_warnings: Vec::new() })
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "display.delete";
    let i = state_index(s, p, cmd)?;
    let v = s.doc_mut().display_states.remove(i);
    reply(cmd, DisplayDeleted { deleted: v.name, param_warnings: Vec::new() })
}

fn list(s: &mut Session, _p: &Value) -> Result<Value> {
    reply("display.list", DisplayListResult { states: s.doc.display_states.clone(), param_warnings: Vec::new() })
}
