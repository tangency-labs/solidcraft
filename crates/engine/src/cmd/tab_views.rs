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
    MAX_NAME, TabViewActivateParams, TabViewBesideParams, TabViewEdge, TabViewIdParams, TabViewLayoutParams, TabViewMoveParams, TabViewNoParams,
    TabViewOpenParams, TabViewRenameParams, TabViewReply,
};
use crate::schema::tab_windows::{
    TabViewMoveToWindowParams, TabWindowCloseReply, TabWindowIdParams, TabWindowListParams, TabWindowListReply, TabWindowMoveReply, TabWindowTarget,
};
use crate::{Result, Session};

const OPEN_CONTRACT: TypedContract<TabViewOpenParams> = TypedContract::new();
const ID_CONTRACT: TypedContract<TabViewIdParams> = TypedContract::new();
const ACTIVATE_CONTRACT: TypedContract<TabViewActivateParams> = TypedContract::new();
const RENAME_CONTRACT: TypedContract<TabViewRenameParams> = TypedContract::new();
const MOVE_CONTRACT: TypedContract<TabViewMoveParams> = TypedContract::new();
const LAYOUT_CONTRACT: TypedContract<TabViewLayoutParams> = TypedContract::new();
const BESIDE_CONTRACT: TypedContract<TabViewBesideParams> = TypedContract::new();
const NO_PARAMS_CONTRACT: TypedContract<TabViewNoParams> = TypedContract::new();
const MOVE_TO_WINDOW_CONTRACT: TypedContract<TabViewMoveToWindowParams> = TypedContract::new();
const WINDOW_ID_CONTRACT: TypedContract<TabWindowIdParams> = TypedContract::new();
const WINDOW_LIST_CONTRACT: TypedContract<TabWindowListParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("tab.view.open", "Open Another View", crate::checked!("tab.view.open", open))
        .icon("new").noundo().doc_only().key("Ctrl+T").contract(&OPEN_CONTRACT).output(output_schema_for::<TabViewReply>)
        .params("kind?: model|sketch|sheet|render|animation (default: the active view's kind); subject?: the sketch or sheet id; reuse?: bool (default: true with a subject); window?: id (default: last focused) → the request, id: the view now shown; the window opens a sub-tab of the shown design; never edits the design or opens a dialog"),
    CommandSpec::new("tab.view.close", "Close View", crate::checked!("tab.view.close", close))
        .icon("close").noundo().doc_only().contract(&ID_CONTRACT).output(output_schema_for::<TabViewReply>)
        .params("id?: the view (default: the active one) → id; closes it, which Reopen Closed View undoes; the last view of a design never closes (an error); never opens a dialog"),
    CommandSpec::new("tab.view.pop_out", "Open View in New Window", crate::checked!("tab.view.pop_out", pop_out))
        .icon("panel_popout").noundo().doc_only().key("Ctrl+Alt+O").contract(&ID_CONTRACT).output(output_schema_for::<TabViewReply>)
        .params("id?: the view (default: the active view) → id; moves the view to another desktop window of the live design; never opens a dialog"),
    CommandSpec::new("tab.view.move_to_window", "Move View to Window", crate::checked!("tab.view.move_to_window", move_to_window))
        .icon("panel_popout").noundo().doc_only().key("Ctrl+Alt+W").contract(&MOVE_TO_WINDOW_CONTRACT).output(output_schema_for::<TabWindowMoveReply>)
        .params("view: an open view id; window?: id or new (default); existing only: index?: insertion position (0 first); new only: display?: name or index, x?, y?, w?, h?: logical points, level?: views_only → view, window, created, from, closed_source, placement"),
    CommandSpec::new("tab.window.list", "List Windows", crate::checked!("tab.window.list", list_windows))
        .icon("list").noundo().doc_only().read_only().contract(&WINDOW_LIST_CONTRACT).output(output_schema_for::<TabWindowListReply>)
        .params("no parameters → windows of the active design; main window is 0; read-only"),
    CommandSpec::new("tab.window.focus", "Show Window", crate::checked!("tab.window.focus", focus_window))
        .icon("panel_float").noundo().doc_only().contract(&WINDOW_ID_CONTRACT).output(output_schema_for::<TabWindowIdParams>)
        .params("window: a window id, including 0 for the main window → window; brings it forward"),
    CommandSpec::new("tab.window.close", "Close Window", crate::checked!("tab.window.close", close_window))
        .icon("close").noundo().doc_only().contract(&WINDOW_ID_CONTRACT).output(output_schema_for::<TabWindowCloseReply>)
        .params("window: a pop-out id → returned: view ids sent back to the main window, in order"),
    CommandSpec::new("tab.view.reopen", "Reopen Closed View", crate::checked!("tab.view.reopen", reopen))
        .icon("undo").noundo().doc_only().key("Ctrl+Shift+T").contract(&NO_PARAMS_CONTRACT).output(output_schema_for::<TabViewReply>)
        .params("no parameters → id of the view brought back, which is shown again; an error when none was closed"),
    CommandSpec::new("tab.view.activate", "Show View", crate::checked!("tab.view.activate", activate))
        .icon("list").noundo().doc_only().contract(&ACTIVATE_CONTRACT).output(output_schema_for::<TabViewReply>)
        .params("id: the view, or index: its place in tab order (0 first); window?: defaults to the last focused window → id; shows it"),
    CommandSpec::new("tab.view.rename", "Rename View", crate::checked!("tab.view.rename", rename))
        .icon("rename").noundo().doc_only().key("F2").contract(&RENAME_CONTRACT).output(output_schema_for::<TabViewReply>)
        .params("name?: 1 to 64 characters after trimming, no control characters (left out, the window opens the view's name field for typing); id?: the view (default: the active one) → id, name"),
    CommandSpec::new("tab.view.duplicate", "Duplicate View", crate::checked!("tab.view.duplicate", duplicate))
        .icon("copy").noundo().doc_only().contract(&ID_CONTRACT).output(output_schema_for::<TabViewReply>)
        .params("id?: the view (default: the active one) → id of the copy, which is shown; the copy sits next to the original"),
    CommandSpec::new("tab.view.move", "Move View", crate::checked!("tab.view.move", move_view))
        .icon("list").noundo().doc_only().contract(&MOVE_CONTRACT).output(output_schema_for::<TabViewReply>)
        .params("from, to: places in tab order (0 first) → from, to; moves the view among the views of its design"),
    CommandSpec::new("tab.view.layout", "Show Views Together", crate::checked!("tab.view.layout", layout))
        .icon("list").noundo().doc_only().contract(&LAYOUT_CONTRACT).output(output_schema_for::<TabViewReply>)
        .params("layout: single|side_by_side|stacked|large_and_two|grid; window?: id (default: last focused) → layout, shown: how many views the window shows together; fills the layout with the window's Model and Render views in tab order (the active one among them); single shows one view at a time again; never edits the design"),
    CommandSpec::new("tab.view.beside", "Show View Beside", crate::checked!("tab.view.beside", beside))
        .icon("copy").noundo().doc_only().contract(&BESIDE_CONTRACT).output(output_schema_for::<TabViewReply>)
        .params("id?: the open Model or Render view to show (default: a new Model angle); edge?: left|right|top|bottom of the active view (default: right); placement?: requires id and replaces edge, {kind:empty,index}, {kind:replace,target}, or {kind:split,target,edge}; window?: id (default: last focused) → id, edge or placement, shown; allows up to four views, including one Render view"),
    CommandSpec::new("tab.view.list", "List Views", crate::checked!("tab.view.list", list))
        .icon("list").noundo().doc_only().contract(&NO_PARAMS_CONTRACT).output(output_schema_for::<TabViewReply>)
        .params("no parameters → views: [{id, kind, name, subject?, active}] of the shown design in tab order; read-only")];

/// `p` as the contract type `P`; no parameters at all (`null`) is the empty object.
fn parse<P: DeserializeOwned>(cmd: &str, p: &Value) -> Result<P> {
    let p = if p.is_null() { Value::Object(Default::default()) } else { p.clone() };
    serde_json::from_value(p).map_err(|e| bad(cmd, e.to_string()))
}

/// The reply a command sends.
fn reply(cmd: &str, reply: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(reply).map_err(|e| bad(cmd, e.to_string()))
}

fn open(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "tab.view.open";
    let request: TabViewOpenParams = parse(cmd, p)?;
    if request.subject.as_ref().is_some_and(|subject| subject.len() > 256) {
        return Err(bad(cmd, "subject must be at most 256 bytes"));
    }
    let reuse = request.reuse.unwrap_or(request.subject.is_some());
    reply(cmd, TabViewReply { kind: request.kind, subject: request.subject, reuse: Some(reuse), window: request.window, ..Default::default() })
}

fn close(_s: &mut Session, p: &Value) -> Result<Value> {
    let request: TabViewIdParams = parse("tab.view.close", p)?;
    reply("tab.view.close", TabViewReply { id: request.id, ..Default::default() })
}

fn pop_out(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "tab.view.pop_out";
    let request: TabViewIdParams = parse(cmd, p)?;
    reply(cmd, TabViewReply { id: request.id, ..Default::default() })
}

fn move_to_window(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "tab.view.move_to_window";
    if p.get("window").is_some_and(|target| !target.is_string() && !target.is_u64()) {
        return Err(bad(cmd, "window must be a window id or new"));
    }
    let mut request: TabViewMoveToWindowParams = parse(cmd, p)?;
    if let TabWindowTarget::Existing(_) = request.window {
        if request.placement.is_some() && request.index.is_some() {
            return Err(bad(cmd, "placement and index cannot be combined"));
        }
        if ["display", "x", "y", "w", "h", "level"].into_iter().any(|key| p.get(key).is_some()) {
            return Err(bad(cmd, "display, geometry and level require window: new"));
        }
    } else {
        if p.get("index").is_some() || p.get("placement").is_some() {
            return Err(bad(cmd, "index and placement require an existing window id"));
        }
        request.level = Some(request.level.unwrap_or_default());
    }
    if [request.x, request.y].into_iter().flatten().any(|coord| !coord.is_finite())
        || [request.w, request.h].into_iter().flatten().any(|size| !size.is_finite() || size <= 0.0)
    {
        return Err(bad(cmd, "bad_geometry"));
    }
    reply(cmd, TabWindowMoveReply::Requested(request))
}

fn list_windows(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "tab.window.list";
    reply(cmd, TabWindowListReply::Requested(parse(cmd, p)?))
}

fn focus_window(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "tab.window.focus";
    reply(cmd, parse::<TabWindowIdParams>(cmd, p)?)
}

fn close_window(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "tab.window.close";
    let request: TabWindowIdParams = parse(cmd, p)?;
    if request.window == 0 {
        return Err(bad(cmd, "cannot_close_main_window"));
    }
    reply(cmd, TabWindowCloseReply::Requested(request))
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
    reply(cmd, TabViewReply { id: request.id, index: request.index, window: request.window, ..Default::default() })
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

fn layout(_s: &mut Session, p: &Value) -> Result<Value> {
    let request: TabViewLayoutParams = parse("tab.view.layout", p)?;
    reply("tab.view.layout", TabViewReply { layout: Some(request.layout), window: request.window, ..Default::default() })
}

fn beside(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "tab.view.beside";
    let request: TabViewBesideParams = parse(cmd, p)?;
    if request.placement.is_some() && (request.id.is_none() || request.edge.is_some()) {
        return Err(bad(cmd, "placement requires id and takes the place of edge"));
    }
    let edge = request.placement.is_none().then_some(request.edge.unwrap_or(TabViewEdge::Right));
    reply(cmd, TabViewReply { id: request.id, edge, placement: request.placement, window: request.window, ..Default::default() })
}

fn list(_s: &mut Session, p: &Value) -> Result<Value> {
    let _: TabViewNoParams = parse("tab.view.list", p)?;
    reply("tab.view.list", TabViewReply::default())
}
