//! SolidCraft's MCP server: its tools, instructions and resources over the shared server
//! (`solidcraft-mcp-server`), with the agent's folders and the output options around each call.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, mpsc};

use serde_json::{Value, json};
use solidcraft_engine::io::vfs::{self, PathScope};
use solidcraft_mcp_server::{Contents, Incoming, Info, Tools, read_message, response};

use crate::backend::{Backend, Remote};
use crate::options::Options;
use crate::tools::{call_tool_with, tool_definitions_with};

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
A call whose error starts `still running:` was not cancelled: don't send it again; `pending` waits for its result.";

/// The design files a call opens or saves (#7181): `open` and `save`, and the commands they run
/// (`doc.open`, `file.save`, `file.save_as`) through `execute` or as `batch` steps. Each reads or
/// writes the file it names through the agent's scope.
fn design_files(name: &str, args: &Value) -> Vec<String> {
    let step = |command: &str, params: &Value| {
        let params = match command {
            "open" | "save" | "doc.open" | "file.save" | "file.save_as" => params,
            "execute" if matches!(params["command"].as_str(), Some("doc.open" | "file.save" | "file.save_as")) => &params["params"],
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
fn side_probe(side: &Mutex<Option<(Remote, Options)>>) -> Option<(Remote, Options)> {
    let side = side.lock().ok()?;
    let (template, options) = side.as_ref()?;
    Some((template.side_channel()?, *options))
}

/// Write one reply line and flush it, whole, whichever thread sends it.
fn write_line(output: &Mutex<impl std::io::Write>, reply: &str) -> std::io::Result<()> {
    let mut out = output.lock().map_err(|_| std::io::Error::other("output lock poisoned"))?;
    out.write_all(format!("{reply}\n").as_bytes())?;
    out.flush()
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
}

impl Agent {
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

impl Tools for Agent {
    fn instructions(&self) -> String {
        format!("{INSTRUCTIONS} Backend: {}.", self.backend.describe())
    }

    fn list(&mut self) -> Vec<Value> {
        self.listed = self.options();
        tool_definitions_with(self.listed)
    }

    fn call(&mut self, name: &str, args: &Value) -> Value {
        let options = self.listed;
        // The open design's folder counts, and goes on counting for the rest of this run when a
        // save-as or open moves the design elsewhere (#7181).
        let open = self.backend.design_path();
        self.scope = self.scope.take().map(|s| s.with_design(open.as_deref()));
        let scope = self.scope.clone();
        let backend = self.backend.as_mut();
        let mut result = vfs::scoped(scope, || call_tool_with(backend, name, args, options));
        // So does the folder of each design file the call opened or saved: the call reached it,
        // so it was one of the agent's folders already. Only paths given in full: a running app
        // resolves a relative one against its own working folder.
        if result["isError"] != true {
            for file in design_files(name, args).iter().filter(|f| std::path::Path::new(f).is_absolute()) {
                self.scope = self.scope.take().map(|s| s.with_design_file(file));
            }
        }
        if let Some(note) = self.backend.notice()
            && let Some(content) = result.get_mut("content").and_then(Value::as_array_mut)
        {
            content.push(json!({"type": "text", "text": note}));
        }
        result
    }

    fn resources(&self) -> Vec<Value> {
        vec![
            json!({"uri": "solidcraft://design", "name": "Active design", "description": "document.inspect with measurements", "mimeType": "application/json"}),
            json!({"uri": "solidcraft://commands", "name": "Command catalog", "description": "every command with its parameter docs", "mimeType": "application/json"}),
        ]
    }

    fn read_resource(&mut self, uri: &str) -> Option<Result<Contents, String>> {
        let read = match uri {
            // The solvecraft:// URIs are what earlier releases listed (compatibility shim,
            // docs/compatibility.md); clients that saved them still read the resource.
            "solidcraft://design" | "solvecraft://design" => self.backend.call("document.inspect", json!({"measure": true})),
            "solidcraft://commands" | "solvecraft://commands" => self.backend.call("engine.commands", json!({})),
            _ => return None,
        };
        Some(read.map(|v| Contents::json(&v)))
    }
}

/// SolidCraft's MCP server over a [`Backend`].
pub struct Server {
    inner: solidcraft_mcp_server::Server<Agent>,
}

impl Server {
    pub fn new(backend: Box<dyn Backend>) -> Self {
        let info = Info { name: "solidcraft", title: "SolidCraft", version: env!("CARGO_PKG_VERSION"), max_line: solidcraft_engine::MAX_LINE };
        let agent = Agent { backend, scope: None, options: Options::default(), app_options: None, listed: Options::default() };
        Self { inner: solidcraft_mcp_server::Server::new(info, agent) }
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
    /// call that arrives while another call is still being handled (a long `batch`) is answered at
    /// once on a connection of its own to the app, as its docs promise, instead of after that call
    /// (#14007). Replies to those calls may then come before the reply to the call in progress;
    /// each carries its request's id.
    pub fn serve(&mut self, input: impl std::io::BufRead + Send, output: impl std::io::Write + Send) -> std::io::Result<()> {
        let output = Mutex::new(output);
        // Messages read and not yet answered by the main loop: counted when the reader queues one,
        // not when the loop takes it, so a `running` read right behind a `batch` the loop has not
        // picked up yet is not queued behind it. A `running` answered apart is never queued, so it
        // never counts.
        let in_flight = AtomicUsize::new(0);
        // How to reach the app apart from the call in progress, and the options its tools run with:
        // known before the first message, and refreshed as each one is taken (the backend may
        // bridge to an app that started since).
        let side: Mutex<Option<(Remote, Options)>> = Mutex::new(self.side());
        let (tx, rx) = mpsc::channel::<Incoming>();
        std::thread::scope(|scope| {
            let (output, in_flight, side) = (&output, &in_flight, &side);
            let reader = scope.spawn(move || -> std::io::Result<()> {
                let mut input = input;
                while let Some(incoming) = read_message(&mut input, solidcraft_engine::MAX_LINE)? {
                    if let Incoming::Line(line) = &incoming
                        && in_flight.load(Ordering::SeqCst) > 0
                        && let Some(id) = running_call(line)
                        && let Some((mut remote, options)) = side_probe(side)
                    {
                        scope.spawn(move || {
                            let result = call_tool_with(&mut remote, "running", &json!({}), options);
                            // The client may be gone; the main loop sees that on its own write.
                            let _ = write_line(output, &response(id, result).to_string());
                        });
                        continue;
                    }
                    in_flight.fetch_add(1, Ordering::SeqCst);
                    if tx.send(incoming).is_err() {
                        break;
                    }
                }
                Ok(())
            });
            let mut written = Ok(());
            for incoming in rx {
                if let Ok(mut s) = side.lock() {
                    *s = self.side();
                }
                let reply = match incoming {
                    Incoming::Line(line) => self.inner.handle_line(&line),
                    Incoming::Refused(reply) => Some(reply),
                };
                let sent = reply.map_or(Ok(()), |reply| write_line(output, &reply));
                in_flight.fetch_sub(1, Ordering::SeqCst);
                if let Err(e) = sent {
                    written = Err(e);
                    break;
                }
            }
            let read = reader.join().unwrap_or_else(|_| Err(std::io::Error::other("the stdin reader stopped")));
            written.and(read)
        })
    }
    /// A fresh bridge to the app for answering `running` apart from the call in progress, with
    /// the options tools run with now; `None` when the backend is not bridged to an app.
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
}
