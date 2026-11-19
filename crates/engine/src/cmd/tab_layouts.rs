//! Validate requests for the host's reusable, application-wide view layouts.
use serde_json::Value;

use super::CommandSpec;
use crate::params::bad;
use crate::schema::tab_layouts::{TabLayoutListReply, TabLayoutNameParams, TabLayoutRenameParams};
use crate::schema::tab_views::TabViewNoParams;
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const LIST_CONTRACT: TypedContract<TabViewNoParams> = TypedContract::new();
const NAME_CONTRACT: TypedContract<TabLayoutNameParams> = TypedContract::new();
const RENAME_CONTRACT: TypedContract<TabLayoutRenameParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("tab.layout.apply", "Apply View Layout", crate::checked!("tab.layout.apply", apply))
        .icon("list")
        .noundo()
        .doc_only()
        .contract(&NAME_CONTRACT)
        .output(output_schema_for::<TabLayoutNameParams>)
        .params("name: a saved view layout → name; arranges the current design's views in the saved window setup"),
    CommandSpec::new("tab.layout.list", "List Saved View Layouts", crate::checked!("tab.layout.list", list))
        .icon("list")
        .noundo()
        .read_only()
        .contract(&LIST_CONTRACT)
        .output(output_schema_for::<TabLayoutListReply>)
        .params("no parameters → layouts: names of the app's saved view layouts; read-only"),
    CommandSpec::new("tab.layout.remove", "Remove Saved View Layout", crate::checked!("tab.layout.remove", remove))
        .icon("close")
        .noundo()
        .contract(&NAME_CONTRACT)
        .output(output_schema_for::<TabLayoutNameParams>)
        .params("name: a saved view layout → name; removes it from the app's saved layouts"),
    CommandSpec::new("tab.layout.rename", "Rename Saved View Layout", crate::checked!("tab.layout.rename", rename))
        .icon("rename")
        .noundo()
        .contract(&RENAME_CONTRACT)
        .output(output_schema_for::<TabLayoutRenameParams>)
        .params("name: a saved view layout; new_name: 1 to 64 characters after trimming, no control characters → name, new_name"),
    CommandSpec::new("tab.layout.save", "Save View Layout", crate::checked!("tab.layout.save", save))
        .icon("save")
        .noundo()
        .doc_only()
        .contract(&NAME_CONTRACT)
        .output(output_schema_for::<TabLayoutNameParams>)
        .params("name: 1 to 64 characters after trimming, no control characters → name; saves the current window setup for reuse with any design"),
];

fn apply(_s: &mut Session, p: &Value) -> Result<Value> {
    named("tab.layout.apply", p)
}

fn list(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "tab.layout.list";
    let p = if p.is_null() { Value::Object(Default::default()) } else { p.clone() };
    let _: TabViewNoParams = serde_json::from_value(p).map_err(|e| bad(cmd, e.to_string()))?;
    reply(cmd, TabLayoutListReply::default())
}

fn remove(_s: &mut Session, p: &Value) -> Result<Value> {
    named("tab.layout.remove", p)
}

fn rename(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "tab.layout.rename";
    let request: TabLayoutRenameParams = serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    reply(cmd, TabLayoutRenameParams { name: name(cmd, "name", &request.name)?, new_name: name(cmd, "new_name", &request.new_name)? })
}

fn save(_s: &mut Session, p: &Value) -> Result<Value> {
    named("tab.layout.save", p)
}

fn named(cmd: &str, p: &Value) -> Result<Value> {
    let request: TabLayoutNameParams = serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    reply(cmd, TabLayoutNameParams { name: name(cmd, "name", &request.name)? })
}

fn name(cmd: &str, key: &str, value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > solidcraft_views::MAX_NAME || value.chars().any(char::is_control) {
        return Err(bad(cmd, format!("{key} must be 1 to {} characters, without control characters", solidcraft_views::MAX_NAME)));
    }
    Ok(value.to_owned())
}

fn reply(cmd: &str, reply: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(reply).map_err(|e| bad(cmd, e.to_string()))
}
