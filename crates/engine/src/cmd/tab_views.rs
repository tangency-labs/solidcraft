//! The views of a design (Model, Sketch, Sheet, Render, Animation) as sub-tabs of its tab. The
//! engine holds one design and no tabs, so, as with `view.next_tab`, each command checks its
//! parameters and echoes them, and the window makes the change (`ui-egui`'s `documents`).
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::CommandSpec;
use crate::params::bad;
use crate::schema::TypedContract;
use crate::schema::output_schema_for;
use crate::schema::tab_views::{
    MAX_NAME, TabViewActivateParams, TabViewIdParams, TabViewMoveParams, TabViewNoParams, TabViewOpenParams, TabViewRenameParams, TabViewReply,
};
use crate::{Result, Session};

const OPEN_CONTRACT: TypedContract<TabViewOpenParams> = TypedContract::new();
const ID_CONTRACT: TypedContract<TabViewIdParams> = TypedContract::new();
const ACTIVATE_CONTRACT: TypedContract<TabViewActivateParams> = TypedContract::new();
const RENAME_CONTRACT: TypedContract<TabViewRenameParams> = TypedContract::new();
const MOVE_CONTRACT: TypedContract<TabViewMoveParams> = TypedContract::new();
const NO_PARAMS_CONTRACT: TypedContract<TabViewNoParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("tab.view.open", "Open Another View", crate::checked!("tab.view.open", open))
        .icon("new").noundo().doc_only().key("Ctrl+T").contract(&OPEN_CONTRACT).output(output_schema_for::<TabViewReply>)
        .params("kind?: model|sketch|sheet|render|animation (default: the active view's kind); subject?: the sketch or sheet id; reuse?: bool (default: true with a subject) → the request, id: the view now shown; the window opens a sub-tab of the shown design; never edits the design or opens a dialog"),
    CommandSpec::new("tab.view.close", "Close View", crate::checked!("tab.view.close", close))
        .icon("close").noundo().doc_only().contract(&ID_CONTRACT).output(output_schema_for::<TabViewReply>)
        .params("id?: the view (default: the active one) → id; closes it, which Reopen Closed View undoes; the last view of a design never closes (an error); never opens a dialog"),
    CommandSpec::new("tab.view.reopen", "Reopen Closed View", crate::checked!("tab.view.reopen", reopen))
        .icon("undo").noundo().doc_only().key("Ctrl+Shift+T").contract(&NO_PARAMS_CONTRACT).output(output_schema_for::<TabViewReply>)
        .params("no parameters → id of the view brought back, which is shown again; an error when none was closed"),
    CommandSpec::new("tab.view.activate", "Show View", crate::checked!("tab.view.activate", activate))
        .icon("list").noundo().doc_only().contract(&ACTIVATE_CONTRACT).output(output_schema_for::<TabViewReply>)
        .params("id: the view, or index: its place in tab order (0 first) → id; shows it"),
    CommandSpec::new("tab.view.rename", "Rename View", crate::checked!("tab.view.rename", rename))
        .icon("rename").noundo().doc_only().key("F2").contract(&RENAME_CONTRACT).output(output_schema_for::<TabViewReply>)
        .params("name?: 1 to 64 characters after trimming, no control characters (left out, the window opens the view's name field for typing); id?: the view (default: the active one) → id, name"),
    CommandSpec::new("tab.view.duplicate", "Duplicate View", crate::checked!("tab.view.duplicate", duplicate))
        .icon("copy").noundo().doc_only().contract(&ID_CONTRACT).output(output_schema_for::<TabViewReply>)
        .params("id?: the view (default: the active one) → id of the copy, which is shown; the copy sits next to the original"),
    CommandSpec::new("tab.view.move", "Move View", crate::checked!("tab.view.move", move_view))
        .icon("list").noundo().doc_only().contract(&MOVE_CONTRACT).output(output_schema_for::<TabViewReply>)
        .params("from, to: places in tab order (0 first) → from, to; moves the view among the views of its design"),
    CommandSpec::new("tab.view.list", "List Views", crate::checked!("tab.view.list", list))
        .icon("list").noundo().doc_only().contract(&NO_PARAMS_CONTRACT).output(output_schema_for::<TabViewReply>)
        .params("no parameters → views: [{id, kind, name, subject?, active}] of the shown design in tab order; read-only")];

/// `p` as the contract type `P`; no parameters at all (`null`) is the empty object.
fn parse<P: DeserializeOwned>(cmd: &str, p: &Value) -> Result<P> {
    let p = if p.is_null() { Value::Object(Default::default()) } else { p.clone() };
    serde_json::from_value(p).map_err(|e| bad(cmd, e.to_string()))
}

/// The reply a command sends.
fn reply(cmd: &str, reply: TabViewReply) -> Result<Value> {
    serde_json::to_value(reply).map_err(|e| bad(cmd, e.to_string()))
}

fn open(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "tab.view.open";
    let request: TabViewOpenParams = parse(cmd, p)?;
    if request.subject.as_ref().is_some_and(|subject| subject.len() > 256) {
        return Err(bad(cmd, "subject must be at most 256 bytes"));
    }
    let reuse = request.reuse.unwrap_or(request.subject.is_some());
    reply(cmd, TabViewReply { kind: request.kind, subject: request.subject, reuse: Some(reuse), ..Default::default() })
}

fn close(_s: &mut Session, p: &Value) -> Result<Value> {
    let request: TabViewIdParams = parse("tab.view.close", p)?;
    reply("tab.view.close", TabViewReply { id: request.id, ..Default::default() })
}

fn reopen(_s: &mut Session, p: &Value) -> Result<Value> {
    let _: TabViewNoParams = parse("tab.view.reopen", p)?;
    reply("tab.view.reopen", TabViewReply::default())
}

fn activate(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "tab.view.activate";
    let request: TabViewActivateParams = parse(cmd, p)?;
    if request.id.is_some() == request.index.is_some() {
        return Err(bad(cmd, "give either id or index"));
    }
    reply(cmd, TabViewReply { id: request.id, index: request.index, ..Default::default() })
}

fn rename(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "tab.view.rename";
    let request: TabViewRenameParams = parse(cmd, p)?;
    let name = match request.name.as_deref().map(str::trim) {
        None => None,
        Some(name) if name.is_empty() || name.chars().count() > MAX_NAME || name.chars().any(char::is_control) => {
            return Err(bad(cmd, format!("name must be 1 to {MAX_NAME} characters, without control characters")));
        }
        Some(name) => Some(name.to_owned()),
    };
    reply(cmd, TabViewReply { id: request.id, name, ..Default::default() })
}

fn duplicate(_s: &mut Session, p: &Value) -> Result<Value> {
    let request: TabViewIdParams = parse("tab.view.duplicate", p)?;
    reply("tab.view.duplicate", TabViewReply { id: request.id, ..Default::default() })
}

fn move_view(_s: &mut Session, p: &Value) -> Result<Value> {
    let request: TabViewMoveParams = parse("tab.view.move", p)?;
    reply("tab.view.move", TabViewReply { from: Some(request.from), to: Some(request.to), ..Default::default() })
}

fn list(_s: &mut Session, p: &Value) -> Result<Value> {
    let _: TabViewNoParams = parse("tab.view.list", p)?;
    reply("tab.view.list", TabViewReply::default())
}
