//! JSON-RPC 2.0 framing and the MCP lifecycle / tools / resources / prompts / completion methods.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

use crate::content::Contents;
use crate::schema::{check_arguments, check_prompt_arguments};

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
/// Most suggestions in one `completion/complete` reply (the MCP limit).
const MAX_COMPLETIONS: usize = 100;

/// What `completion/complete` asks about: a prompt by its name, or a resource by its URI or URI
/// template.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reference<'a> {
    Prompt(&'a str),
    Resource(&'a str),
}

/// An app's tools, served by [`Server`].
pub trait Tools {
    /// What every client is told at `initialize`.
    fn instructions(&self) -> String;
    /// Every tool's definition ([`crate::tool`]), as `tools/list` serves it now.
    fn list(&mut self) -> Vec<Value>;
    /// Run tool `name`. The server calls it right after the [`Tools::list`] its arguments were
    /// checked against ([`crate::check_arguments`]), so it runs the tool as that listing defined
    /// it. Failures are tool results with `isError: true` ([`crate::text_result`]); it never
    /// panics on hostile arguments.
    fn call(&mut self, name: &str, args: &Value) -> Value;
    /// The resources `resources/list` serves (MCP `Resource`: uri, name, description, mimeType).
    fn resources(&self) -> Vec<Value> {
        Vec::new()
    }
    /// The resource templates `resources/templates/list` serves (MCP `ResourceTemplate`:
    /// uriTemplate, name, description, mimeType). A URI made from one is read with
    /// [`Tools::read_resource`].
    fn resource_templates(&self) -> Vec<Value> {
        Vec::new()
    }
    /// The contents of resource `uri`: `None` when there is no such resource, `Err` when it could
    /// not be read now.
    fn read_resource(&mut self, uri: &str) -> Option<Result<Contents, String>> {
        let _ = uri;
        None
    }
    /// The prompts `prompts/list` serves (MCP `Prompt`: name, title, description, arguments).
    fn prompts(&self) -> Vec<Value> {
        Vec::new()
    }
    /// Prompt `name` filled in with `args` (MCP `GetPromptResult`: description, messages). The
    /// server calls it only for a listed prompt, with `args` checked against that listing: an
    /// object of strings with every required argument and no other. `Err` is an internal error.
    fn get_prompt(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        let _ = args;
        Err(format!("prompt `{name}` cannot be filled in"))
    }
    /// What argument `argument` of `reference` could be, given what has been typed (`value`).
    /// Asked only of an app that serves prompts or resource templates; a prompt reference is
    /// always a listed prompt. The reply holds the first 100.
    fn complete(&mut self, reference: Reference<'_>, argument: &str, value: &str) -> Vec<String> {
        let _ = (reference, argument, value);
        Vec::new()
    }
}

/// Who the server says it is at `initialize`, and the longest request line it reads.
#[derive(Clone, Copy, Debug)]
pub struct Info {
    pub name: &'static str,
    pub title: &'static str,
    pub version: &'static str,
    /// Longest accepted request line, in bytes; a longer one is refused in bounded space.
    pub max_line: usize,
}

/// An MCP server over the app's [`Tools`].
pub struct Server<T: Tools> {
    info: Info,
    tools: T,
    initialized: bool,
}

fn error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message.into()}})
}

/// One message read from a newline-delimited stream by [`read_message`].
#[derive(Debug, PartialEq, Eq)]
pub enum Incoming {
    /// A line to handle ([`Server::handle_line`]), without its newline.
    Line(String),
    /// A line refused unread (too long, not UTF-8): the error reply to send for it.
    Refused(String),
}

/// Read the next line of newline-delimited JSON-RPC from `input`; `None` once it closes. A line
/// longer than `max_line` is consumed in bounded space, so a client cannot exhaust memory by
/// sending an arbitrarily long request without a newline, and an unterminated last line counts.
pub fn read_message(input: &mut impl BufRead, max_line: usize) -> std::io::Result<Option<Incoming>> {
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
            if line.len().saturating_add(content_len) > max_line {
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
        return Ok(None);
    }
    if oversized {
        return Ok(Some(Incoming::Refused(error(Value::Null, INVALID_REQUEST, "request too large").to_string())));
    }
    Ok(Some(match String::from_utf8(line) {
        Ok(line) => Incoming::Line(line),
        // Not UTF-8: answer with a parse error and keep serving.
        Err(_) => Incoming::Refused(error(Value::Null, PARSE_ERROR, "parse error: input is not UTF-8").to_string()),
    }))
}

/// The JSON-RPC response to request `id`, as [`Server::handle`] sends it.
pub fn response(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

impl<T: Tools> Server<T> {
    pub fn new(info: Info, tools: T) -> Self {
        Self { info, tools, initialized: false }
    }
    pub fn tools(&mut self) -> &mut T {
        &mut self.tools
    }
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Serve newline-delimited JSON-RPC until `input` closes. Only protocol messages go to
    /// `output`; diagnostics belong on stderr. Consume oversized lines in bounded space so a
    /// client cannot exhaust memory by sending an arbitrarily long request without a newline.
    pub fn serve(&mut self, mut input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
        while let Some(incoming) = read_message(&mut input, self.info.max_line)? {
            let reply = match incoming {
                Incoming::Line(line) => self.handle_line(&line),
                Incoming::Refused(reply) => Some(reply),
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
        if line.len() > self.info.max_line {
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
                let Info { name, title, version: server_version, .. } = self.info;
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": self.capabilities(),
                    "serverInfo": {"name": name, "title": title, "version": server_version},
                    "instructions": self.tools.instructions(),
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": self.tools.list()})),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).ok_or((INVALID_PARAMS, "missing tool `name`".to_string()))?;
                let args = match params.get("arguments") {
                    None | Some(Value::Null) => json!({}),
                    Some(a) => a.clone(),
                };
                check_arguments(&self.tools.list(), name, &args).map_err(|e| (INVALID_PARAMS, e))?;
                Ok(self.tools.call(name, &args))
            }
            "resources/list" => Ok(json!({"resources": self.tools.resources()})),
            "resources/templates/list" => Ok(json!({"resourceTemplates": self.tools.resource_templates()})),
            "prompts/list" => Ok(json!({"prompts": self.tools.prompts()})),
            "resources/read" => {
                let uri = params.get("uri").and_then(Value::as_str).unwrap_or("");
                let contents = self
                    .tools
                    .read_resource(uri)
                    .ok_or_else(|| (RESOURCE_NOT_FOUND, format!("unknown resource `{uri}`")))?
                    .map_err(|e| (INTERNAL_ERROR, e))?;
                Ok(json!({"contents": [contents.block(uri)]}))
            }
            "prompts/get" if !self.tools.prompts().is_empty() => {
                let name = params.get("name").and_then(Value::as_str).ok_or((INVALID_PARAMS, "missing prompt `name`".to_string()))?;
                let args = match params.get("arguments") {
                    None | Some(Value::Null) => json!({}),
                    Some(a) => a.clone(),
                };
                check_prompt_arguments(&self.tools.prompts(), name, &args).map_err(|e| (INVALID_PARAMS, e))?;
                self.tools.get_prompt(name, &args).map_err(|e| (INTERNAL_ERROR, e))
            }
            "completion/complete" if self.completes() => {
                let reference = params.get("ref").unwrap_or(&Value::Null);
                let reference = match (reference.get("type").and_then(Value::as_str), reference.get("name"), reference.get("uri")) {
                    (Some("ref/prompt"), Some(Value::String(name)), _) => {
                        if !self.tools.prompts().iter().any(|p| p["name"] == name.as_str()) {
                            return Err((INVALID_PARAMS, format!("unknown prompt `{name}`")));
                        }
                        Reference::Prompt(name)
                    }
                    (Some("ref/resource"), _, Some(Value::String(uri))) => Reference::Resource(uri),
                    _ => {
                        return Err((
                            INVALID_PARAMS,
                            "`ref` must be a prompt's {type: ref/prompt, name} or a resource's {type: ref/resource, uri}".into(),
                        ));
                    }
                };
                let argument = params.get("argument");
                let (Some(argument), Some(value)) =
                    (argument.and_then(|a| a.get("name")).and_then(Value::as_str), argument.and_then(|a| a.get("value")).and_then(Value::as_str))
                else {
                    return Err((INVALID_PARAMS, "`argument` must have a `name` and a `value`, both strings".into()));
                };
                let mut values = self.tools.complete(reference, argument, value);
                let total = values.len();
                values.truncate(MAX_COMPLETIONS);
                Ok(json!({"completion": {"values": values, "total": total, "hasMore": total > MAX_COMPLETIONS}}))
            }
            _ => Err((METHOD_NOT_FOUND, format!("method `{method}` not found"))),
        }
    }

    /// What the server tells a client at `initialize` that it serves: the tools always, and the
    /// rest only when the app has some.
    fn capabilities(&self) -> Value {
        let mut capabilities = json!({"tools": {"listChanged": false}});
        if !self.tools.resources().is_empty() || !self.tools.resource_templates().is_empty() {
            capabilities["resources"] = json!({"listChanged": false});
        }
        if !self.tools.prompts().is_empty() {
            capabilities["prompts"] = json!({"listChanged": false});
        }
        if self.completes() {
            capabilities["completions"] = json!({});
        }
        capabilities
    }

    /// Whether the app has arguments to complete: those of its prompts and resource templates.
    fn completes(&self) -> bool {
        !self.tools.prompts().is_empty() || !self.tools.resource_templates().is_empty()
    }
}
