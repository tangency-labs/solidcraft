//! Tab groups: open designs gathered under a named, coloured label in the window's tab strip. The
//! engine holds one design and no tabs, so, as with `tab.view.*`, each command checks its
//! parameters and echoes them, and the window makes the change (`ui-egui`'s `documents`).
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::CommandSpec;
use crate::params::bad;
use crate::schema::TypedContract;
use crate::schema::output_schema_for;
use crate::schema::tab_groups::{
    MAX_NAME, TabGroupAddParams, TabGroupCreateParams, TabGroupIdParams, TabGroupNoParams, TabGroupReply, TabGroupTabParams, TabGroupUpdateParams,
};
use crate::{Result, Session};

const CREATE_CONTRACT: TypedContract<TabGroupCreateParams> = TypedContract::new();
const ADD_CONTRACT: TypedContract<TabGroupAddParams> = TypedContract::new();
const TAB_CONTRACT: TypedContract<TabGroupTabParams> = TypedContract::new();
const UPDATE_CONTRACT: TypedContract<TabGroupUpdateParams> = TypedContract::new();
const ID_CONTRACT: TypedContract<TabGroupIdParams> = TypedContract::new();
const NO_PARAMS_CONTRACT: TypedContract<TabGroupNoParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("tab.group.create", "New Tab Group", crate::checked!("tab.group.create", create))
        .icon("new").noundo().doc_only().contract(&CREATE_CONTRACT).output(output_schema_for::<TabGroupReply>)
        .params("tab?: the design's tab, 0 first (default: the one shown); name?: at most 64 characters, no control characters (default: none); colour?: blue|green|orange|yellow|purple|cyan|pink (default: the first unused) → group: the new group's id; the design leaves any other group; never edits the design or opens a dialog"),
    CommandSpec::new("tab.group.add", "Add to Tab Group", crate::checked!("tab.group.add", add))
        .icon("list").noundo().doc_only().contract(&ADD_CONTRACT).output(output_schema_for::<TabGroupReply>)
        .params("group: the group's id; tab?: the design's tab (default: the one shown) → group; the design moves next to the group's other designs and leaves any other group"),
    CommandSpec::new("tab.group.remove", "Remove from Tab Group", crate::checked!("tab.group.remove", remove))
        .icon("close").noundo().doc_only().contract(&TAB_CONTRACT).output(output_schema_for::<TabGroupReply>)
        .params("tab?: the design's tab (default: the one shown) → group it left; it moves out past the group's other designs; a group left with no design goes"),
    CommandSpec::new("tab.group.update", "Edit Tab Group", crate::checked!("tab.group.update", update))
        .icon("rename").noundo().doc_only().contract(&UPDATE_CONTRACT).output(output_schema_for::<TabGroupReply>)
        .params("group: the group's id; name?: at most 64 characters, empty for none; colour?: blue|green|orange|yellow|purple|cyan|pink; collapsed?: true folds the group's designs away to its label, false shows them (the design on screen keeps its group open) → group, name, colour, collapsed"),
    CommandSpec::new("tab.group.ungroup", "Ungroup Tabs", crate::checked!("tab.group.ungroup", ungroup))
        .icon("close").noundo().doc_only().contract(&ID_CONTRACT).output(output_schema_for::<TabGroupReply>)
        .params("group: the group's id → group; the group goes and its designs stay open where they are"),
    CommandSpec::new("tab.group.list", "List Tab Groups", crate::checked!("tab.group.list", list))
        .icon("list").noundo().doc_only().contract(&NO_PARAMS_CONTRACT).output(output_schema_for::<TabGroupReply>)
        .params("no parameters → groups: [{id, name, colour, collapsed, tabs}] in the order they were made, tabs 0 first; read-only")];

/// `p` as the contract type `P`; no parameters at all (`null`) is the empty object.
fn parse<P: DeserializeOwned>(cmd: &str, p: &Value) -> Result<P> {
    let p = if p.is_null() { Value::Object(Default::default()) } else { p.clone() };
    serde_json::from_value(p).map_err(|e| bad(cmd, e.to_string()))
}

/// The reply a command sends.
fn reply(cmd: &str, reply: TabGroupReply) -> Result<Value> {
    serde_json::to_value(reply).map_err(|e| bad(cmd, e.to_string()))
}

/// `name` trimmed, or a refusal when it is too long or holds a control character.
fn checked_name(cmd: &str, name: Option<&str>) -> Result<Option<String>> {
    match name.map(str::trim) {
        Some(name) if name.chars().count() > MAX_NAME || name.chars().any(char::is_control) => {
            Err(bad(cmd, format!("name must be at most {MAX_NAME} characters, without control characters")))
        }
        name => Ok(name.map(str::to_owned)),
    }
}

fn create(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "tab.group.create";
    let request: TabGroupCreateParams = parse(cmd, p)?;
    let name = checked_name(cmd, request.name.as_deref())?;
    reply(cmd, TabGroupReply { tab: request.tab, name, colour: request.colour, ..Default::default() })
}

fn add(_s: &mut Session, p: &Value) -> Result<Value> {
    let request: TabGroupAddParams = parse("tab.group.add", p)?;
    reply("tab.group.add", TabGroupReply { group: Some(request.group), tab: request.tab, ..Default::default() })
}

fn remove(_s: &mut Session, p: &Value) -> Result<Value> {
    let request: TabGroupTabParams = parse("tab.group.remove", p)?;
    reply("tab.group.remove", TabGroupReply { tab: request.tab, ..Default::default() })
}

fn update(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "tab.group.update";
    let request: TabGroupUpdateParams = parse(cmd, p)?;
    let name = checked_name(cmd, request.name.as_deref())?;
    reply(cmd, TabGroupReply { group: Some(request.group), name, colour: request.colour, collapsed: request.collapsed, ..Default::default() })
}

fn ungroup(_s: &mut Session, p: &Value) -> Result<Value> {
    let request: TabGroupIdParams = parse("tab.group.ungroup", p)?;
    reply("tab.group.ungroup", TabGroupReply { group: Some(request.group), ..Default::default() })
}

fn list(_s: &mut Session, p: &Value) -> Result<Value> {
    let _: TabGroupNoParams = parse("tab.group.list", p)?;
    reply("tab.group.list", TabGroupReply::default())
}
