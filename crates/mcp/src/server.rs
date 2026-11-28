//! SolidCraft's MCP server: its tools, instructions and resources over the shared server
//! (`solidcraft-mcp-server`), with the agent's folders and the output options around each call.

use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Mutex, mpsc};
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use solidcraft_engine::io::vfs::{self, PathScope};
use solidcraft_mcp_server::{Contents, Info, ProgressReporter, Reference, ResourceError, Tools, notification};
#[cfg(not(target_arch = "wasm32"))]
use solidcraft_mcp_server::{Incoming, cancelled_notification, read_message, response};

#[cfg(not(target_arch = "wasm32"))]
use crate::backend::Remote;
use crate::backend::{Backend, DesignState};
use crate::options::Options;
#[cfg(not(target_arch = "wasm32"))]
use crate::tools::call_tool_with;
use crate::tools::{CORE_TOOLS, CallContext, call_tool_with_context, tool_definitions_with};

/// Maximum number of resource URIs a single MCP session may subscribe to simultaneously.
pub const MAX_RESOURCE_SUBSCRIPTIONS: usize = 256;

/// Bound on undrained notifications kept in memory when a caller neither runs [`Server::serve`]
/// nor calls [`Server::take_notifications`].
const MAX_PENDING_NOTIFICATIONS: usize = 1024;

/// How often [`Server::serve`] polls a bridged desktop app for external design changes while idle.
#[cfg(not(target_arch = "wasm32"))]
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Backoff after an idle poll cannot reach the bridged app, avoiding repeated reconnect attempts
/// while the app is not running.
#[cfg(not(target_arch = "wasm32"))]
const FAILED_POLL_BACKOFF: Duration = Duration::from_secs(5);

/// Callback receiving server-to-client JSON-RPC 2.0 notifications as they are emitted.
pub type NotificationSink = Box<dyn FnMut(Value)>;

/// Bounded per-session set of subscribed resource URIs (`resources/subscribe`).
pub(crate) struct ResourceSubscriptions {
    uris: BTreeSet<String>,
    limit: usize,
}

impl ResourceSubscriptions {
    pub(crate) fn new(limit: usize) -> Self {
        Self { uris: BTreeSet::new(), limit }
    }

    pub(crate) fn subscribe(&mut self, uri: &str) -> Result<(), ResourceError> {
        if self.uris.contains(uri) {
            return Ok(());
        }
        if self.uris.len() >= self.limit {
            return Err(ResourceError::InvalidParams(format!("resource subscription limit reached (maximum {})", self.limit)));
        }
        self.uris.insert(uri.to_owned());
        Ok(())
    }

    pub(crate) fn unsubscribe(&mut self, uri: &str) -> bool {
        self.uris.remove(uri)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.uris.is_empty()
    }

    pub(crate) fn uris(&self) -> Vec<String> {
        self.uris.iter().cloned().collect()
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.uris.len()
    }

    #[cfg(test)]
    pub(crate) fn set_limit(&mut self, limit: usize) {
        self.limit = limit;
    }
}

/// Whether `uri` reflects the active design and should receive `notifications/resources/updated`
/// when the design changes. The static command catalog (`solidcraft://commands`) is unaffected by
/// design edits.
fn uri_affected_by_design_change(uri: &str) -> bool {
    uri != "solidcraft://commands"
}

/// What every MCP client is told at `initialize`: how to plan, build and verify a part. It stays
/// version-neutral (no workaround for a quirk of the current kernel or solver: those belong in
/// `skills/solidcraft-modeling/reference.md`, which `skill_reference` serves per release), and
/// `tests::instructions_name_only_real_tools` checks every tool and command id it names.
const INSTRUCTIONS: &str = "SolidCraft is an independent parametric 3D CAD app, and these tools drive it: when its window is \
open the user watches the model appear, otherwise a headless session builds it. When asked to design or build a part, do it \
with these tools straight away. Units are millimetres and degrees in expressions, Z is up.\n\n\
Start: for anything beyond one primitive, call `skill_reference {\"skill\":\"solidcraft-modeling\"}` first (command shapes, \
ids, plane axes, worked examples and the known traps of this version; `file: \"SKILL.md\"` has the workflow). Then \
`inspect_design {\"measure\":false,\"sketches\":false}` to see what is already there.\n\n\
Plan: work the numbers out first (the parameters, the expected volume and bounding box after each stage, the world point you \
will use for each edge or face pick). Then build in a few `batch` calls (all-or-nothing; the reply names the failing step; \
`save`, `measure` and `set_parameter` can be steps). Use `verbose: false`. Name every feature. `list_commands {\"filter\": word}` \
gives ids and one-line parameter docs (never unfiltered: it also takes prefix, panel, compact: true and limit); `describe_command` gives one command's docs and schemas.\n\n\
Routes: a plate, block, boss, standoff or hole is cheapest as a primitive with expression fields (`solid.box`, `solid.cylinder`, \
`solid.hole`, `solid.fillet`, `solid.shell`, patterns and mirror), no sketch. Sketch when the profile is not a box or a circle. \
Fillet outside edges before shelling, build inside features after the shell, then the holes and cuts through walls. Give a blind \
hole or a hole through one wall of a hollow part its depth.\n\n\
Parametric: a size or position that must follow a parameter is an expression in a feature field (`\"L/2 - 10\"`), a sketch \
dimension to `\"origin\"` or an edge, or a pattern spacing; a plain number never follows a parameter. Every sketch must end at \
dof 0 (`sketch.solve` reports it). A pick (`edges: [[x,y,z]]`, `faces: [[x,y,z]]`) is a number that snaps to the nearest edge \
or face and follows it when parameters change.\n\n\
Verify: `measure {}` after each stage and compare volume and bbox with your numbers; `ok: true` alone proves nothing: a cut \
that leaves the volume unchanged has missed, a join that adds a body did not touch. Positions of holes and bosses come from \
`body_topology` (cylinder origin, axis and radius), not from the volume or the centre of mass of a symmetric part. To check \
that the design follows its parameters, `check_parameters {\"set\": {name: value}}` tries the values and puts the design \
back (volume, errors, cylinder axes), or change them in one `batch` with `coalesce_parameters: true` (one rebuild), `measure`, \
then set them back the same way before saving.\n\n\
Fix, don't rebuild: a wrong number in a feature is changed with `timeline.edit` or `parameters.change` on its input \
(`parameters.list {\"filter\":\"feature\"}` names them); `edit.checkpoint` and `edit.restore_checkpoint` go back exactly.\n\n\
The user's current selection comes with every reply; when they say 'this', 'these' or 'the selected', use it.\n\n\
A call whose error starts `still running:` was not cancelled: don't send it again; `pending` waits for its result.";

/// [`INSTRUCTIONS`] for the core tool listing ([`Options::core_tools`]): the same guidance, with
/// commands found by `find_commands` and run by `call_command`, and only tools the listing has.
/// `tests::core_instructions_name_only_listed_tools` checks the names.
const CORE_INSTRUCTIONS: &str = "SolidCraft is an independent parametric 3D CAD app, and these tools drive it: when its window is \
open the user watches the model appear, otherwise a headless session builds it. When asked to design or build a part, do it \
with these tools straight away. Units are millimetres and degrees in expressions, Z is up.\n\n\
Start: for anything beyond one primitive, call `skill_reference {\"skill\":\"solidcraft-modeling\"}` first (command shapes, \
ids, plane axes, worked examples and the known traps of this version; `file: \"SKILL.md\"` has the workflow). Then \
`inspect_design {\"measure\":false,\"sketches\":false}` to see what is already there.\n\n\
Plan: work the numbers out first (the parameters, the expected volume and bounding box after each stage, the world point you \
will use for each edge or face pick). Then build in a few `batch` calls (all-or-nothing; the reply names the failing step; \
`save`, `measure` and `set_parameter` can be steps). Use `verbose: false`. Name every feature. `find_commands {\"query\": words}` \
gives the matching command ids with their parameters; `call_command {\"id\", \"params\"}` runs one, checked against its schema first.\n\n\
Routes: a plate, block, boss, standoff or hole is cheapest as a primitive with expression fields (`solid.box`, `solid.cylinder`, \
`solid.hole`, `solid.fillet`, `solid.shell`, patterns and mirror), no sketch. Sketch when the profile is not a box or a circle. \
Fillet outside edges before shelling, build inside features after the shell, then the holes and cuts through walls. Give a blind \
hole or a hole through one wall of a hollow part its depth.\n\n\
Parametric: a size or position that must follow a parameter is an expression in a feature field (`\"L/2 - 10\"`), a sketch \
dimension to `\"origin\"` or an edge, or a pattern spacing; a plain number never follows a parameter. Every sketch must end at \
dof 0 (`sketch.solve` reports it). A pick (`edges: [[x,y,z]]`, `faces: [[x,y,z]]`) is a number that snaps to the nearest edge \
or face and follows it when parameters change.\n\n\
Verify: `measure {}` after each stage and compare volume and bbox with your numbers; `ok: true` alone proves nothing: a cut \
that leaves the volume unchanged has missed, a join that adds a body did not touch. Positions of holes and bosses come from \
`body_topology` (cylinder origin, axis and radius), not from the volume or the centre of mass of a symmetric part. To check that the design follows its parameters, change them in one `batch` with \
`coalesce_parameters: true` (one rebuild), `measure`, then set them back the same way before saving.\n\n\
Fix, don't rebuild: a wrong number in a feature is changed with `timeline.edit` or `parameters.change` on its input \
(`parameters.list {\"filter\":\"feature\"}` names them); `edit.checkpoint` and `edit.restore_checkpoint` go back exactly.\n\n\
The user's current selection comes with every reply; when they say 'this', 'these' or 'the selected', use it.\n\n\
A call whose error starts `still running:` was not cancelled: don't send it again; `pending` waits for its result.\n\n\
These are the core tools: every other SolidCraft action is a command, found with `find_commands` and run with `call_command` or \
as a `batch` step.";

/// The design files a call opens or saves (#7181): `open` and `save`, and the commands they run
/// (`doc.open`, `file.save`, `file.save_as`) through `execute` or as `batch` steps. Each reads or
/// writes the file it names through the agent's scope.
fn design_files(name: &str, args: &Value) -> Vec<String> {
    let step = |command: &str, params: &Value| {
        let params = match command {
            "open" | "save" | "doc.open" | "file.save" | "file.save_as" => params,
            "execute" if matches!(params["command"].as_str(), Some("doc.open" | "file.save" | "file.save_as")) => &params["params"],
            "call_command" if matches!(params["id"].as_str(), Some("doc.open" | "file.save" | "file.save_as")) => &params["params"],
            _ => return None,
        };
        params["path"].as_str().map(str::to_owned)
    };
    match name {
        "batch" => args["commands"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| step(s["command"].as_str().or_else(|| s["id"].as_str())?, &s["params"]))
            .collect(),
        _ => step(name, args).into_iter().collect(),
    }
}

/// The id of a `tools/call` of `running` (no arguments), the one call answered apart from the
/// call in progress (#14007); `None` for anything else, which waits its turn.
#[cfg(not(target_arch = "wasm32"))]
fn running_call(line: &str) -> Option<Value> {
    let msg: Value = serde_json::from_str(line.trim()).ok()?;
    let id = msg.get("id").filter(|id| !id.is_null())?;
    let params = msg.get("params")?;
    let no_arguments = match params.get("arguments") {
        None | Some(Value::Null) => true,
        Some(Value::Object(a)) => a.is_empty(),
        Some(_) => false,
    };
    (msg["method"] == "tools/call" && params["name"] == "running" && no_arguments).then(|| id.clone())
}

/// A fresh bridge to the app for a `running` call while another call is in progress, with the
/// options tools run with; `None` when the backend is not bridged to an app.
#[cfg(not(target_arch = "wasm32"))]
fn side_probe(side: &Mutex<Option<(Remote, Options)>>) -> Option<(Remote, Options)> {
    let side = side.lock().ok()?;
    let (template, options) = side.as_ref()?;
    Some((template.side_channel()?, *options))
}

/// Cap queued cancellations tracked ahead of execution so hostile input cannot grow memory.
#[cfg(not(target_arch = "wasm32"))]
const MAX_QUEUED_CANCELS: usize = 256;

/// JSON-RPC request `id`s carried by one incoming line (one for a single request object, or every
/// request's id in a JSON-RPC batch array). Notifications and invalid JSON yield none.
#[cfg(not(target_arch = "wasm32"))]
fn request_ids(line: &str) -> Vec<Value> {
    let Ok(msg) = serde_json::from_str::<Value>(line.trim()) else { return Vec::new() };
    let one = |v: &Value| {
        let o = v.as_object()?;
        o.get("method").and_then(Value::as_str)?;
        o.get("id").filter(|id| matches!(id, Value::String(_)) || matches!(id, Value::Number(n) if n.is_i64() || n.is_u64())).cloned()
    };
    match &msg {
        Value::Object(_) => one(&msg).into_iter().collect(),
        Value::Array(items) => items.iter().filter_map(one).collect(),
        _ => Vec::new(),
    }
}

/// Request IDs currently executing or queued in [`Server::serve`], shared with the stdin reader so
/// out-of-band `notifications/cancelled` matches the exact request and ignores mismatched IDs.
#[cfg(not(target_arch = "wasm32"))]
struct FlightState {
    current_ids: Vec<Value>,
    current_cancel: Arc<AtomicBool>,
    queued_ids: Vec<Value>,
    cancelled_queued: Vec<Value>,
}

#[cfg(not(target_arch = "wasm32"))]
impl FlightState {
    fn new() -> Self {
        Self { current_ids: Vec::new(), current_cancel: Arc::new(AtomicBool::new(false)), queued_ids: Vec::new(), cancelled_queued: Vec::new() }
    }

    fn enqueue(&mut self, ids: &[Value]) {
        self.queued_ids.extend(ids.iter().cloned());
    }

    fn start(&mut self, ids: Vec<Value>) -> Arc<AtomicBool> {
        let mut already_cancelled = false;
        for id in &ids {
            if let Some(pos) = self.queued_ids.iter().position(|q| q == id) {
                self.queued_ids.remove(pos);
            }
            if let Some(pos) = self.cancelled_queued.iter().position(|c| c == id) {
                self.cancelled_queued.remove(pos);
                already_cancelled = true;
            }
        }
        let flag = Arc::new(AtomicBool::new(already_cancelled));
        self.current_ids = ids;
        self.current_cancel = Arc::clone(&flag);
        flag
    }

    fn finish(&mut self) {
        self.current_ids.clear();
        self.current_cancel = Arc::new(AtomicBool::new(false));
    }

    /// Record a `notifications/cancelled` for `request_id`. Returns `true` when `request_id`
    /// matches the currently executing request (setting its local cancel flag), so the caller can
    /// also cancel a bridged app call via `Remote::side_channel()`.
    fn cancel(&mut self, request_id: &Value) -> bool {
        if self.current_ids.iter().any(|id| id == request_id) {
            self.current_cancel.store(true, Ordering::SeqCst);
            return true;
        }
        if self.queued_ids.iter().any(|id| id == request_id)
            && !self.cancelled_queued.iter().any(|id| id == request_id)
            && self.cancelled_queued.len() < MAX_QUEUED_CANCELS
        {
            self.cancelled_queued.push(request_id.clone());
        }
        false
    }
}

/// SolidCraft's tools as the shared server serves them: the backend they run on, the files they
/// may reach and the options that shape what they return.
struct Agent {
    backend: Box<dyn Backend>,
    scope: Option<PathScope>,
    /// The options from the command line and environment.
    options: Options,
    /// The options the app's Preferences ask for, once it has answered (`None`: not yet).
    app_options: Option<Options>,
    /// The options of the last listing: a call runs with the tools as the listing it was checked
    /// against defined them.
    listed: Options,
    /// Out-of-band cancellation flag for the currently executing request, if any.
    cancel: Option<Arc<AtomicBool>>,
    /// True once the client sent `notifications/initialized`.
    initialized: bool,
    /// Per-session subscribed resource URIs (`resources/subscribe`).
    subscriptions: ResourceSubscriptions,
    /// Last observed active design identity, revision, and optional structural inventory.
    last_design: Option<DesignState>,
    /// Optional push sink invoked immediately when a server-to-client notification is emitted.
    notification_sink: Option<NotificationSink>,
    /// Bounded queue of notifications awaiting flush in `serve` or `take_notifications`.
    pending_notifications: VecDeque<Value>,
}

impl Agent {
    fn result_says(result: &Value, what: &str) -> bool {
        result["content"].as_array().is_some_and(|c| c.iter().any(|t| t["text"].as_str().is_some_and(|t| t.contains(what))))
    }

    fn result_unavailable(result: &Value) -> bool {
        Self::result_says(result, crate::backend::NOT_RUNNING) || Self::result_says(result, crate::backend::STILL_RUNNING)
    }

    /// Whether this session is actively tracking and emitting resource notifications.
    fn notifications_active(&self) -> bool {
        self.initialized || !self.subscriptions.is_empty() || self.notification_sink.is_some()
    }

    /// Whether `uri` names a resource currently served by this server.
    fn is_known_resource(&self, uri: &str) -> bool {
        self.resources().iter().any(|r| r.get("uri").and_then(Value::as_str) == Some(uri))
    }

    /// Capture the baseline design state without emitting notifications if none is recorded yet.
    fn ensure_design_baseline(&mut self) {
        if self.last_design.is_none() {
            self.last_design = self.backend.design_state();
        }
    }

    /// Emit one server-to-client JSON-RPC notification to the sink (if any) and the drain queue.
    fn emit(&mut self, note: Value) {
        if let Some(sink) = self.notification_sink.as_mut() {
            sink(note.clone());
        }
        if self.pending_notifications.len() >= MAX_PENDING_NOTIFICATIONS {
            self.pending_notifications.pop_front();
        }
        self.pending_notifications.push_back(note);
    }

    /// Compare `current` against the last observed design state, updating it and emitting
    /// `notifications/resources/list_changed` and `notifications/resources/updated` as needed.
    fn observe_design_state(&mut self, current: DesignState) -> usize {
        let Some(previous) = self.last_design.take() else {
            self.last_design = Some(current);
            return 0;
        };
        let same = previous.document_id == current.document_id && previous.revision == current.revision;
        let list_changed = previous.document_id != current.document_id
            || match (&previous.structure, &current.structure) {
                (Some(before), Some(after)) => before != after,
                _ => true,
            };
        self.last_design = Some(current);
        if same {
            return 0;
        }
        let mut emitted = 0;
        if list_changed {
            self.emit(notification("notifications/resources/list_changed", None));
            emitted += 1;
        }
        for uri in self.subscriptions.uris() {
            if uri_affected_by_design_change(&uri) {
                self.emit(notification("notifications/resources/updated", Some(json!({"uri": uri}))));
                emitted += 1;
            }
        }
        emitted
    }

    /// Query the backend for any design revision change that occurred outside an MCP tool call.
    /// Returns `None` when the backend could not report its design state.
    fn poll_external_changes(&mut self) -> Option<usize> {
        let current = self.backend.design_state()?;
        Some(self.observe_design_state(current))
    }

    /// The selection block for a reply, `None` when nothing is selected or the backend cannot say
    /// (no app listening, an app from before `document.selection`).
    fn selection(&mut self, tool: &str, result: &Value) -> Option<String> {
        // `running` and `pending` ask how an earlier request is getting on: they never wait for
        // another question to the app. Neither does a reply that says a request is still running,
        // or one that never reached the app.
        if matches!(tool, "running" | "pending") || Self::result_unavailable(result) {
            return None;
        }
        let reply = self.backend.call("document.selection", json!({})).ok()?;
        reply["selection"].as_str().map(str::to_owned)
    }

    /// The options tools use now: the server's own, with what the app's Preferences turn on. The
    /// app is asked until it answers once (it may start after the server, or be busy), and its
    /// answer then holds for this session, so the tools an agent listed keep their shape.
    fn options(&mut self) -> Options {
        if self.app_options.is_none()
            && let Ok(reply) = self.backend.app_options()
        {
            self.app_options = Some(reply.as_ref().map(Options::from_app).unwrap_or_default());
        }
        self.options.or(self.app_options.unwrap_or_default())
    }
}

/// `--structured`: the result's reply as data, `structuredContent: {"result", "notes"?}`, for a
/// program calling the tools. `result` is the first text block read as JSON (the text itself when
/// it is not JSON: a message); `notes` are the text blocks after it (a notice, the selection). It
/// is built from what every tool already returns, so a new tool is covered without a change here.
/// Keys a result already carries (`panel_error`, `plugin_error`) stay.
pub(crate) fn structure(result: &mut Value) {
    let texts: Vec<&str> =
        result["content"].as_array().into_iter().flatten().filter(|c| c["type"] == "text").filter_map(|c| c["text"].as_str()).collect();
    let Some((first, rest)) = texts.split_first() else { return };
    let reply = serde_json::from_str::<Value>(first).unwrap_or_else(|_| Value::String((*first).to_string()));
    let notes: Vec<Value> = rest.iter().map(|t| Value::String((*t).to_string())).collect();
    let mut data = match result.get_mut("structuredContent").map(Value::take) {
        Some(Value::Object(m)) => m,
        _ => serde_json::Map::new(),
    };
    data.insert("result".into(), reply);
    if !notes.is_empty() {
        data.insert("notes".into(), Value::Array(notes));
    }
    result["structuredContent"] = Value::Object(data);
}

impl Tools for Agent {
    fn instructions(&self) -> String {
        let instructions = if self.options.core_tools { CORE_INSTRUCTIONS } else { INSTRUCTIONS };
        format!("{instructions} Backend: {}.", self.backend.describe())
    }

    fn list(&mut self) -> Vec<Value> {
        self.listed = self.options();
        let mut tools = tool_definitions_with(self.listed);
        if self.listed.core_tools {
            tools.retain(|t| t["name"].as_str().is_some_and(|n| CORE_TOOLS.contains(&n)));
        }
        // Every installed, enabled plugin's entries, read afresh: a plugin turned off or removed
        // since the last listing is gone from this one.
        tools.extend(crate::plugin_tools::definitions(self.backend.as_mut()));
        tools
    }

    fn definition(&mut self, name: &str) -> Option<Value> {
        self.listed = self.options();
        if crate::plugin_tools::is_plugin_tool(name) {
            crate::plugin_tools::definition_for(self.backend.as_mut(), name)
        } else {
            tool_definitions_with(self.listed).into_iter().find(|t| t["name"].as_str() == Some(name))
        }
    }

    fn call(&mut self, name: &str, args: &Value) -> Value {
        self.call_with_progress(name, args, None)
    }

    fn call_with_progress(&mut self, name: &str, args: &Value, progress: Option<&ProgressReporter>) -> Value {
        let track_changes = self.notifications_active() && !matches!(name, "running" | "quit_app");
        if track_changes {
            self.ensure_design_baseline();
        }
        let options = self.listed;
        // The open design's folder counts, and goes on counting for the rest of this run when a
        // save-as or open moves the design elsewhere (#7181).
        let open = self.backend.design_path();
        self.scope = self.scope.take().map(|s| s.with_design(open.as_deref()));
        let scope = self.scope.clone();
        let ctx = CallContext { progress: progress.cloned(), cancel: self.cancel.clone() };
        self.backend.set_progress(progress.cloned());
        self.backend.set_cancel_flag(self.cancel.clone());
        let backend = self.backend.as_mut();
        let mut result = vfs::scoped(scope, || call_tool_with_context(backend, name, args, options, &ctx));
        self.backend.set_progress(None);
        self.backend.set_cancel_flag(None);
        // So does the folder of each design file the call opened or saved: the call reached it,
        // so it was one of the agent's folders already. Only paths given in full: a running app
        // resolves a relative one against its own working folder.
        let dry_run_batch = name == "batch" && args.get("dry_run").and_then(Value::as_bool) == Some(true);
        if result["isError"] != true && !dry_run_batch {
            for file in design_files(name, args).iter().filter(|f| std::path::Path::new(f).is_absolute()) {
                self.scope = self.scope.take().map(|s| s.with_design_file(file));
            }
        }
        if let Some(note) = self.backend.notice()
            && let Some(content) = result.get_mut("content").and_then(Value::as_array_mut)
        {
            content.push(json!({"type": "text", "text": note}));
        }
        // What the user has selected goes with every reply, so "this" and "these" mean something.
        if !ctx.is_cancelled()
            && let Some(block) = self.selection(name, &result)
            && let Some(content) = result.get_mut("content").and_then(Value::as_array_mut)
        {
            content.push(json!({"type": "text", "text": block}));
        }
        if self.options.structured {
            structure(&mut result);
        }
        if track_changes
            && !ctx.is_cancelled()
            && !Self::result_unavailable(&result)
            && let Some(current) = self.backend.design_state()
        {
            self.observe_design_state(current);
        }
        result
    }

    fn resources(&self) -> Vec<Value> {
        vec![
            json!({"uri": "solidcraft://design", "name": "Active design", "description": "document.inspect with measurements", "mimeType": "application/json"}),
            json!({"uri": "solidcraft://commands", "name": "Command catalog", "description": "every command with its parameter docs", "mimeType": "application/json"}),
        ]
    }

    fn resource_templates(&self) -> Vec<Value> {
        crate::resources::templates()
    }

    fn read_resource(&mut self, uri: &str) -> Option<Result<Contents, String>> {
        match uri {
            "solidcraft://design" => Some(self.backend.call("document.inspect", json!({"measure": true})).map(|v| Contents::json(&v))),
            "solidcraft://commands" => Some(self.backend.call("engine.commands", json!({})).map(|v| Contents::json(&v))),
            _ => crate::resources::read_template(self.backend.as_mut(), uri),
        }
    }

    fn prompts(&self) -> Vec<Value> {
        crate::prompts::definitions()
    }

    fn get_prompt(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        crate::prompts::get(name, args)
    }

    fn complete(&mut self, reference: Reference<'_>, argument: &str, value: &str) -> Vec<String> {
        crate::resources::complete(self.backend.as_mut(), reference, argument, value)
    }

    fn resource_subscribe(&self) -> bool {
        true
    }

    fn resource_list_changed(&self) -> bool {
        true
    }

    fn subscribe_resource(&mut self, uri: &str) -> Result<(), ResourceError> {
        if !self.is_known_resource(uri) {
            return Err(ResourceError::NotFound(format!("unknown resource `{uri}`")));
        }
        self.subscriptions.subscribe(uri)?;
        self.ensure_design_baseline();
        Ok(())
    }

    fn unsubscribe_resource(&mut self, uri: &str) -> Result<(), ResourceError> {
        if self.subscriptions.unsubscribe(uri) || self.is_known_resource(uri) {
            return Ok(());
        }
        Err(ResourceError::NotFound(format!("unknown resource `{uri}`")))
    }

    fn on_initialized(&mut self) {
        self.initialized = true;
        self.ensure_design_baseline();
    }

    fn take_notifications(&mut self) -> Vec<Value> {
        self.pending_notifications.drain(..).collect()
    }
}

/// SolidCraft's MCP server over a [`Backend`].
pub struct Server {
    inner: solidcraft_mcp_server::Server<Agent>,
    #[cfg(not(target_arch = "wasm32"))]
    poll_interval: Duration,
    #[cfg(not(target_arch = "wasm32"))]
    last_failed_poll: Option<Instant>,
}

impl Server {
    pub fn new(backend: Box<dyn Backend>) -> Self {
        let info = Info { name: "solidcraft", title: "SolidCraft", version: env!("CARGO_PKG_VERSION"), max_line: solidcraft_engine::MAX_LINE };
        let agent = Agent {
            backend,
            scope: None,
            options: Options::default(),
            app_options: None,
            listed: Options::default(),
            cancel: None,
            initialized: false,
            subscriptions: ResourceSubscriptions::new(MAX_RESOURCE_SUBSCRIPTIONS),
            last_design: None,
            notification_sink: None,
            pending_notifications: VecDeque::new(),
        };
        Self {
            inner: solidcraft_mcp_server::Server::new(info, agent),
            #[cfg(not(target_arch = "wasm32"))]
            poll_interval: DEFAULT_POLL_INTERVAL,
            #[cfg(not(target_arch = "wasm32"))]
            last_failed_poll: None,
        }
    }
    /// Install a thread-safe sink for server-to-client notifications (`notifications/progress`).
    pub fn with_notifier(mut self, emit: Arc<dyn Fn(Value) + Send + Sync>) -> Self {
        self.inner = self.inner.with_notifier(emit);
        self
    }
    /// Confine the files tool calls may reach to `scope` (`None`: anywhere). `solidcraft-cli mcp`
    /// always sets one unless started with `--allow-any-path` (docs/mcp.md).
    pub fn with_path_scope(mut self, scope: Option<PathScope>) -> Self {
        self.inner.tools().scope = scope;
        self
    }
    /// Shape tool output with `options` (default: as always; `solidcraft-cli mcp --compact` or
    /// `SOLIDCRAFT_MCP_COMPACT=1` turn on compact output, docs/mcp.md). The app's Preferences can
    /// turn an option on too ([`Server::options`]).
    pub fn with_options(mut self, options: Options) -> Self {
        self.inner.tools().options = options;
        self
    }
    /// Attach a sink that receives every server-to-client JSON-RPC notification as it is emitted.
    pub fn with_notification_sink(mut self, sink: impl FnMut(Value) + 'static) -> Self {
        self.inner.tools().notification_sink = Some(Box::new(sink));
        self
    }
    /// How often [`Server::serve`] polls a bridged desktop app for external design changes while
    /// idle (default: 500 ms).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }
    /// Drain server-to-client notifications queued since the last drain.
    pub fn take_notifications(&mut self) -> Vec<Value> {
        self.inner.take_notifications()
    }
    /// Observe the backend for external design changes (e.g. user or in-app Assistant edits in a
    /// connected desktop app) and emit any resulting resource notifications. Returns the number of
    /// notifications emitted.
    pub fn poll_changes(&mut self) -> usize {
        self.inner.tools().poll_external_changes().unwrap_or(0)
    }
    /// Resource URIs currently subscribed to in this session, in sorted order.
    pub fn subscribed_resources(&mut self) -> Vec<String> {
        self.inner.tools().subscriptions.uris()
    }
    #[cfg(test)]
    pub(crate) fn set_max_subscriptions(&mut self, limit: usize) {
        self.inner.tools().subscriptions.set_limit(limit);
    }
    /// The options tools use now: the server's own, with what the app's Preferences turn on.
    pub fn options(&mut self) -> Options {
        self.inner.tools().options()
    }
    pub fn backend(&mut self) -> &mut dyn Backend {
        self.inner.tools().backend.as_mut()
    }
    pub fn is_initialized(&self) -> bool {
        self.inner.is_initialized()
    }
    /// Serve newline-delimited JSON-RPC until `input` closes (see the shared server). Requests are
    /// handled one at a time, in order, but `input` is read on a thread of its own, so a `running`
    /// call or `notifications/cancelled` that arrives while another call is still being handled
    /// (a long `batch`) is handled at once instead of waiting behind that call (#14007). Progress
    /// notifications (`notifications/progress`) and side-channel `running` replies go out over the
    /// same writer thread as normal responses so lines never interleave. Native only: it needs the
    /// stdin reader thread and the bridge to the app, which the browser build has neither of.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn serve(&mut self, input: impl std::io::BufRead + Send, output: impl std::io::Write + Send) -> std::io::Result<()> {
        // Messages read and not yet answered by the main loop: counted when the reader queues one,
        // not when the loop takes it, so a `running` read right behind a `batch` the loop has not
        // picked up yet is not queued behind it. A `running` answered apart is never queued, so it
        // never counts.
        let in_flight = AtomicUsize::new(0);
        let flight = Mutex::new(FlightState::new());
        // How to reach the app apart from the call in progress, and the options its tools run with:
        // known before the first message, and refreshed as each one is taken (the backend may
        // bridge to an app that started since).
        let side: Mutex<Option<(Remote, Options)>> = Mutex::new(self.side());
        let (out_tx, out_rx) = mpsc::channel::<String>();
        let notify_tx = Arc::new(Mutex::new(out_tx.clone()));
        self.inner.set_notifier(Some(Arc::new(move |msg: Value| {
            if let Ok(tx) = notify_tx.lock() {
                let _ = tx.send(msg.to_string());
            }
        })));
        std::thread::scope(|outer| {
            let writer = outer.spawn(move || -> std::io::Result<()> {
                let mut output = output;
                for line in out_rx {
                    output.write_all(line.as_bytes())?;
                    output.write_all(b"\n")?;
                    output.flush()?;
                }
                Ok(())
            });
            let read = std::thread::scope(|scope| {
                let (in_flight, flight, side) = (&in_flight, &flight, &side);
                let (tx, rx) = mpsc::channel::<(Incoming, Vec<Value>)>();
                let reader_out = out_tx.clone();
                let reader = scope.spawn(move || -> std::io::Result<()> {
                    let mut input = input;
                    while let Some(incoming) = read_message(&mut input, solidcraft_engine::MAX_LINE)? {
                        if let Incoming::Line(line) = &incoming {
                            if let Some(cancelled) = cancelled_notification(line) {
                                let cancel_remote = flight.lock().ok().is_some_and(|mut f| f.cancel(&cancelled.request_id));
                                if cancel_remote && let Some((mut remote, _)) = side_probe(side) {
                                    scope.spawn(move || {
                                        let _ = remote.cancel_side_channel();
                                    });
                                }
                                continue;
                            }
                            if in_flight.load(Ordering::SeqCst) > 0
                                && let Some(id) = running_call(line)
                                && let Some((mut remote, options)) = side_probe(side)
                            {
                                let side_out = reader_out.clone();
                                scope.spawn(move || {
                                    let result = call_tool_with(&mut remote, "running", &json!({}), options);
                                    // The client may be gone; the main loop sees that on its own write.
                                    let _ = side_out.send(response(id, result).to_string());
                                });
                                continue;
                            }
                        }
                        let ids = match &incoming {
                            Incoming::Line(line) => request_ids(line),
                            Incoming::Refused(_) => Vec::new(),
                        };
                        if let Ok(mut f) = flight.lock() {
                            f.enqueue(&ids);
                        }
                        in_flight.fetch_add(1, Ordering::SeqCst);
                        if tx.send((incoming, ids)).is_err() {
                            break;
                        }
                    }
                    Ok(())
                });
                loop {
                    let (incoming, ids) = if self.should_poll_external() {
                        match rx.recv_timeout(self.poll_interval) {
                            Ok(item) => item,
                            Err(mpsc::RecvTimeoutError::Timeout) => {
                                self.poll_external_in_serve();
                                if !self.flush_notifications(&out_tx) {
                                    break;
                                }
                                continue;
                            }
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        }
                    } else {
                        match rx.recv() {
                            Ok(item) => item,
                            Err(mpsc::RecvError) => break,
                        }
                    };
                    let cancel_flag = flight.lock().ok().map(|mut f| f.start(ids));
                    self.inner.tools().cancel = cancel_flag;
                    if let Ok(mut s) = side.lock() {
                        *s = self.side();
                    }
                    self.last_failed_poll = None;
                    let reply = match incoming {
                        Incoming::Line(line) => self.inner.handle_line(&line),
                        Incoming::Refused(reply) => Some(reply),
                    };
                    self.inner.tools().cancel = None;
                    if let Ok(mut f) = flight.lock() {
                        f.finish();
                    }
                    let sent = reply.is_none_or(|reply| out_tx.send(reply).is_ok()) && self.flush_notifications(&out_tx);
                    in_flight.fetch_sub(1, Ordering::SeqCst);
                    if !sent {
                        break;
                    }
                }
                reader.join().unwrap_or_else(|_| Err(std::io::Error::other("the stdin reader stopped")))
            });
            self.inner.set_notifier(None);
            drop(out_tx);
            let written = writer.join().unwrap_or_else(|_| Err(std::io::Error::other("the stdout writer stopped")));
            written.and(read)
        })
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn should_poll_external(&mut self) -> bool {
        let agent = self.inner.tools();
        !agent.backend.in_process() && agent.notifications_active()
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn poll_external_in_serve(&mut self) {
        let backoff = FAILED_POLL_BACKOFF.min(self.poll_interval.saturating_mul(10));
        if let Some(failed_at) = self.last_failed_poll
            && failed_at.elapsed() < backoff
        {
            return;
        }
        if self.inner.tools().poll_external_changes().is_some() {
            self.last_failed_poll = None;
        } else {
            self.last_failed_poll = Some(Instant::now());
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn flush_notifications(&mut self, out_tx: &mpsc::Sender<String>) -> bool {
        for note in self.inner.take_notifications() {
            if out_tx.send(note.to_string()).is_err() {
                return false;
            }
        }
        true
    }
    /// A fresh bridge to the app for answering `running` apart from the call in progress, with
    /// the options tools run with now; `None` when the backend is not bridged to an app.
    #[cfg(not(target_arch = "wasm32"))]
    fn side(&mut self) -> Option<(Remote, Options)> {
        let options = self.inner.tools().listed;
        self.inner.tools().backend.side_channel().map(|r| (r, options))
    }
    /// One line in, at most one line out (notifications get no reply).
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        self.inner.handle_line(line)
    }
    pub fn handle(&mut self, msg: Value) -> Option<Value> {
        self.inner.handle(msg)
    }
    /// Bind a loopback-only TCP listener on `addr` and serve MCP Streamable HTTP (`POST /mcp`,
    /// `GET /mcp`, `DELETE /mcp`) until `config.shutdown` is triggered.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn serve_http(&mut self, addr: std::net::SocketAddr, config: solidcraft_mcp_server::HttpConfig) -> std::io::Result<()> {
        let config =
            if config.session_id_generator.is_none() { config.with_session_id_generator(solidcraft_control::server::new_token) } else { config };
        self.inner.serve_http(addr, config)
    }
    /// Serve MCP Streamable HTTP on an already-bound loopback `listener` until `config.shutdown`
    /// is triggered. Multiple local MCP clients share this warm session in strict FIFO order.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn serve_http_listener(&mut self, listener: std::net::TcpListener, config: solidcraft_mcp_server::HttpConfig) -> std::io::Result<()> {
        let config =
            if config.session_id_generator.is_none() { config.with_session_id_generator(solidcraft_control::server::new_token) } else { config };
        self.inner.serve_http_listener(listener, config)
    }
}
