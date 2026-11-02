//! JSON-RPC 2.0 framing and the MCP lifecycle / tools / resources methods.

use std::io::{BufRead, Write};

use serde_json::{Value, json};
/// Longest accepted request line, shared with the control server.
use solidcraft_engine::MAX_LINE;

use solidcraft_engine::io::vfs::{self, PathScope};

use crate::backend::Backend;
use crate::tools::{call_tool, check_arguments, tool_definitions};

/// The newest protocol revision we speak; offered when the client asks for one we don't know.
pub const PROTOCOL_VERSION: &str = "2025-06-18";
pub const SUPPORTED_VERSIONS: &[&str] = &[PROTOCOL_VERSION, "2025-03-26", "2024-11-05"];

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const INTERNAL_ERROR: i64 = -32603;
const RESOURCE_NOT_FOUND: i64 = -32002;

/// Most messages in one JSON-RPC batch.
const MAX_BATCH: usize = 1000;

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
gives ids and one-line parameter docs (never unfiltered); `describe_command` gives one command's docs and schemas.\n\n\
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

pub struct Server {
    backend: Box<dyn Backend>,
    initialized: bool,
    scope: Option<PathScope>,
}

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

fn response(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}
fn error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message.into()}})
}

impl Server {
    pub fn new(backend: Box<dyn Backend>) -> Self {
        Self { backend, initialized: false, scope: None }
    }
    /// Confine the files tool calls may reach to `scope` (`None`: anywhere). `solidcraft-cli mcp`
    /// always sets one unless started with `--allow-any-path` (docs/mcp.md).
    pub fn with_path_scope(mut self, scope: Option<PathScope>) -> Self {
        self.scope = scope;
        self
    }
    pub fn backend(&mut self) -> &mut dyn Backend {
        self.backend.as_mut()
    }
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Serve newline-delimited JSON-RPC until `input` closes. Only protocol messages go to
    /// `output`; diagnostics belong on stderr. Consume oversized lines in bounded space so an
    /// agent cannot exhaust memory by sending an arbitrarily long request without a newline.
    pub fn serve(&mut self, mut input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
        loop {
            let mut line = Vec::new();
            let mut oversized = false;
            let mut received = false;
            loop {
                let chunk = input.fill_buf()?;
                if chunk.is_empty() {
                    break;
                }
                let count = chunk.iter().position(|&byte| byte == b'\n').map_or(chunk.len(), |i| i + 1);
                let newline = chunk[count - 1] == b'\n';
                let content_len = count - usize::from(newline);
                if !oversized {
                    if line.len().saturating_add(content_len) > MAX_LINE {
                        oversized = true;
                        line.clear();
                    } else {
                        line.extend_from_slice(&chunk[..content_len]);
                    }
                }
                input.consume(count);
                received = true;
                if newline {
                    break;
                }
            }
            if !received {
                break;
            }
            let reply = if oversized {
                Some(error(Value::Null, INVALID_REQUEST, "request too large").to_string())
            } else {
                match String::from_utf8(line) {
                    Ok(line) => self.handle_line(&line),
                    // Not UTF-8: answer with a parse error and keep serving.
                    Err(_) => Some(error(Value::Null, PARSE_ERROR, "parse error: input is not UTF-8").to_string()),
                }
            };
            if let Some(reply) = reply {
                output.write_all(reply.as_bytes())?;
                output.write_all(b"\n")?;
                output.flush()?;
            }
        }
        Ok(())
    }

    /// One line in, at most one line out (notifications get no reply).
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        // Check the raw input before trimming: a huge whitespace prefix is still a huge line.
        if line.len() > MAX_LINE {
            return Some(error(Value::Null, INVALID_REQUEST, "request too large").to_string());
        }
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        let reply = match serde_json::from_str::<Value>(line) {
            Ok(Value::Array(batch)) if batch.is_empty() => Some(error(Value::Null, INVALID_REQUEST, "empty batch")),
            Ok(Value::Array(batch)) if batch.len() > MAX_BATCH => Some(error(Value::Null, INVALID_REQUEST, "batch too large")),
            Ok(Value::Array(batch)) => {
                let replies: Vec<Value> = batch.into_iter().filter_map(|m| self.handle(m)).collect();
                (!replies.is_empty()).then_some(Value::Array(replies))
            }
            Ok(msg) => self.handle(msg),
            Err(e) => Some(error(Value::Null, PARSE_ERROR, format!("parse error: {e}"))),
        };
        reply.map(|r| r.to_string())
    }

    pub fn handle(&mut self, msg: Value) -> Option<Value> {
        let Value::Object(o) = &msg else { return Some(error(Value::Null, INVALID_REQUEST, "message must be an object")) };
        let id = o.get("id").cloned();
        let Some(method) = o.get("method").and_then(Value::as_str) else {
            // A response to a request of ours (we send none) or junk.
            if o.contains_key("result") || o.contains_key("error") {
                return None;
            }
            return Some(error(id.unwrap_or(Value::Null), INVALID_REQUEST, "missing `method`"));
        };
        let params = o.get("params").cloned().unwrap_or(Value::Null);
        let Some(id) = id else {
            if method == "notifications/initialized" {
                self.initialized = true;
            }
            return None;
        };
        Some(match self.request(method, &params) {
            Ok(r) => response(id, r),
            Err((code, m)) => error(id, code, m),
        })
    }

    fn request(&mut self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        match method {
            "initialize" => {
                let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL_VERSION);
                let version = if SUPPORTED_VERSIONS.contains(&asked) { asked } else { PROTOCOL_VERSION };
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {"listChanged": false}, "resources": {"listChanged": false}},
                    "serverInfo": {"name": "solidcraft", "title": "SolidCraft", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": format!("{INSTRUCTIONS} Backend: {}.", self.backend.describe()),
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tool_definitions()})),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).ok_or((INVALID_PARAMS, "missing tool `name`".to_string()))?;
                let args = match params.get("arguments") {
                    None | Some(Value::Null) => json!({}),
                    Some(a) => a.clone(),
                };
                check_arguments(name, &args).map_err(|e| (INVALID_PARAMS, e))?;
                // The open design's folder counts, and goes on counting for the rest of this run
                // when a save-as or open moves the design elsewhere (#7181).
                let open = self.backend.design_path();
                self.scope = self.scope.take().map(|s| s.with_design(open.as_deref()));
                let scope = self.scope.clone();
                let backend = self.backend.as_mut();
                let mut result = vfs::scoped(scope, || call_tool(backend, name, &args));
                // So does the folder of each design file the call opened or saved: the call reached
                // it, so it was one of the agent's folders already. Only paths given in full: a
                // running app resolves a relative one against its own working folder.
                if result["isError"] != true {
                    for file in design_files(name, &args).iter().filter(|f| std::path::Path::new(f).is_absolute()) {
                        self.scope = self.scope.take().map(|s| s.with_design_file(file));
                    }
                }
                if let Some(note) = backend.notice()
                    && let Some(content) = result.get_mut("content").and_then(Value::as_array_mut)
                {
                    content.push(json!({"type": "text", "text": note}));
                }
                Ok(result)
            }
            "resources/list" => Ok(json!({"resources": [
                {"uri": "solidcraft://design", "name": "Active design", "description": "document.inspect with measurements", "mimeType": "application/json"},
                {"uri": "solidcraft://commands", "name": "Command catalog", "description": "every command with its parameter docs", "mimeType": "application/json"},
            ]})),
            "resources/templates/list" => Ok(json!({"resourceTemplates": []})),
            "prompts/list" => Ok(json!({"prompts": []})),
            "resources/read" => {
                let uri = params.get("uri").and_then(Value::as_str).unwrap_or("");
                let v = match uri {
                    // The solvecraft:// URIs are what earlier releases listed (compatibility shim,
                    // docs/compatibility.md); clients that saved them still read the resource.
                    "solidcraft://design" | "solvecraft://design" => self.backend.call("document.inspect", json!({"measure": true})),
                    "solidcraft://commands" | "solvecraft://commands" => self.backend.call("engine.commands", json!({})),
                    _ => return Err((RESOURCE_NOT_FOUND, format!("unknown resource `{uri}`"))),
                }
                .map_err(|e| (INTERNAL_ERROR, e))?;
                Ok(json!({"contents": [{"uri": uri, "mimeType": "application/json", "text": v.to_string()}]}))
            }
            _ => Err((METHOD_NOT_FOUND, format!("method `{method}` not found"))),
        }
    }
}
