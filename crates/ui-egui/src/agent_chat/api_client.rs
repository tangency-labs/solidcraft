//! The API backend (desktop only): a model behind an OpenAI-compatible chat completions endpoint
//! (Ollama, LM Studio, OpenAI, any URL the user gives). SolidCraft runs the tool loop itself, with
//! the same tools the ACP agents get: it starts the same MCP server (`solidcraft-cli mcp
//! --connect` to the chat's own endpoint) and is its client, so every tool call passes the same
//! gate, limits and folder checks. The key, when there is one, comes from the system keychain for
//! this connection only.
//!
//! The loop runs on a thread of its own: a model request can take a while.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use serde_json::{Value, json};

use super::acp::ModelInfo;
use super::backend::{ChatBackend, Event, HttpFn, HttpRequest, PromptPart, StartContext, ToolStatus};
use super::process::{AgentProcess, FromAgent, ProcessHandle};

/// Model requests for one message, at most (each answer with tool calls is one).
pub const MAX_REQUESTS_PER_TURN: usize = 24;
/// Longest tool result handed back to the model.
const MAX_TOOL_TEXT: usize = 20_000;
/// Messages kept in the conversation (the system note stays).
const MAX_HISTORY: usize = 240;
/// How long one model request may take.
const REQUEST_TIMEOUT_S: u64 = 300;
/// How long one tool call may take (it may wait for the user's Allow).
const TOOL_TIMEOUT: Duration = Duration::from_secs(330);
/// How often a waiting request or tool call looks at the cancel flag.
const CANCEL_POLL: Duration = Duration::from_millis(40);

/// Tools the model is not offered.
const HIDDEN_TOOLS: &[&str] = &["crash_reports"];

pub struct ApiConfig {
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub key: Option<String>,
    pub http: HttpFn,
}

enum Cmd {
    Prompt(Vec<PromptPart>),
    NewChat,
    SetModel(String),
}

pub struct ApiBackend {
    cmds: Option<Sender<Cmd>>,
    events: Receiver<Event>,
    cancel: Arc<AtomicBool>,
    mcp: Option<ProcessHandle>,
}

impl ApiBackend {
    pub fn start(cfg: ApiConfig, ctx: StartContext) -> Result<ApiBackend, String> {
        if cfg.key.is_some() && !super::agents::key_safe_for(&cfg.base_url) {
            return Err(crate::i18n::tr("ui.ai_key_needs_https").to_string());
        }
        // The same MCP server an ACP agent would get.
        let mcp = match &ctx.bridge {
            Some(b) => {
                let argv = vec![b.cli.to_string_lossy().into_owned(), "mcp".into(), "--connect".into(), format!("127.0.0.1:{}", b.port)];
                Some(AgentProcess::spawn(&argv, &ctx.cwd, &["SOLIDCRAFT_CONTROL_TOKEN"], || {})?)
            }
            None => None,
        };
        let handle = mcp.as_ref().map(AgentProcess::handle);
        let (cmd_tx, cmd_rx) = channel::<Cmd>();
        let (ev_tx, events) = channel::<Event>();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::from(ctx.wake);
        std::thread::spawn(move || {
            let mut w = Worker {
                cfg,
                mcp,
                next: 0,
                tools: Vec::new(),
                history: Vec::new(),
                events: ev_tx,
                wake,
                cancel: stop,
                no_reasoning: Default::default(),
            };
            w.run(cmd_rx);
        });
        Ok(ApiBackend { cmds: Some(cmd_tx), events, cancel, mcp: handle })
    }
}

impl ChatBackend for ApiBackend {
    fn poll(&mut self) -> Vec<Event> {
        self.events.try_iter().take(512).collect()
    }
    fn prompt(&mut self, parts: &[PromptPart]) {
        self.cancel.store(false, Ordering::Relaxed);
        if let Some(c) = &self.cmds {
            let _ = c.send(Cmd::Prompt(parts.to_vec()));
        }
    }
    fn cancel(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
    fn set_model(&mut self, model: &str) {
        if let Some(c) = &self.cmds {
            let _ = c.send(Cmd::SetModel(model.to_string()));
        }
    }
    fn new_session(&mut self) {
        // A running turn is cancelled; the worker resets the history once it has ended.
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(c) = &self.cmds {
            let _ = c.send(Cmd::NewChat);
        }
    }
    fn answer_permission(&mut self, _key: u64, _option: Option<&str>) {}
    fn authenticate(&mut self, _method: &str) {}
    fn pid(&self) -> Option<u32> {
        self.mcp.as_ref().and_then(ProcessHandle::pid)
    }
    fn shutdown(&mut self, wait: bool) {
        self.cancel.store(true, Ordering::Relaxed);
        self.cmds = None;
        if let Some(h) = self.mcp.take() {
            h.shutdown(wait);
        }
    }
}

impl Drop for ApiBackend {
    fn drop(&mut self) {
        self.shutdown(false);
    }
}

struct Worker {
    cfg: ApiConfig,
    mcp: Option<AgentProcess>,
    next: u64,
    /// The MCP tools as OpenAI function tools.
    tools: Vec<Value>,
    history: Vec<Value>,
    events: Sender<Event>,
    wake: Arc<dyn Fn() + Send + Sync>,
    cancel: Arc<AtomicBool>,
    /// Set once the endpoint refused tools together with a reasoning effort: later requests ask for none.
    no_reasoning: std::cell::Cell<bool>,
}

impl Worker {
    fn emit(&self, e: Event) {
        let _ = self.events.send(e);
        (self.wake)();
    }

    fn run(&mut self, cmds: Receiver<Cmd>) {
        if let Err(e) = self.start_mcp() {
            self.emit(Event::Error(e));
        }
        self.emit(Event::ToolsAvailable(!self.tools.is_empty()));
        self.history.push(json!({"role": "system", "content": super::acp::SYSTEM_NOTE}));
        self.emit(Event::Ready { agent: format!("{} · {}", self.cfg.name, self.cfg.model), images: true });
        // The endpoint's models for the picker; an endpoint that does not list them shows none.
        if let Ok(list) = model_list(&self.cfg.http, &self.cfg.base_url, self.cfg.key.as_deref()) {
            let mut available = list;
            if !self.cfg.model.is_empty() && !available.iter().any(|m| m.id == self.cfg.model) {
                available.insert(0, ModelInfo { id: self.cfg.model.clone(), name: self.cfg.model.clone(), description: String::new() });
            }
            if !available.is_empty() {
                self.emit(Event::Models { current: self.cfg.model.clone(), available });
            }
        }
        for cmd in cmds {
            match cmd {
                Cmd::Prompt(parts) => self.turn(parts),
                Cmd::SetModel(m) => self.cfg.model = m,
                Cmd::NewChat => {
                    self.history.truncate(1);
                    self.cancel.store(false, Ordering::Relaxed);
                    self.emit(Event::Ready { agent: format!("{} · {}", self.cfg.name, self.cfg.model), images: true });
                }
            }
        }
        if let Some(mut m) = self.mcp.take() {
            m.shutdown(false);
        }
    }

    /// One MCP request to the server; its `result`.
    fn mcp_call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let Some(m) = self.mcp.as_ref() else { return Err("no SolidCraft tools in this session".into()) };
        self.next += 1;
        let id = self.next;
        m.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let deadline = std::time::Instant::now() + TOOL_TIMEOUT;
        loop {
            // Wait in short slices so Stop and Send now do not wait for a slow tool: its late
            // answer is skipped later (its id no longer matches).
            if self.cancel.load(Ordering::Relaxed) {
                return Err("cancelled by the user".into());
            }
            let left = deadline.saturating_duration_since(std::time::Instant::now()).min(CANCEL_POLL);
            match m.from_agent.recv_timeout(left) {
                Ok(FromAgent::Line(l)) => {
                    let Ok(v) = serde_json::from_str::<Value>(l.trim()) else { continue };
                    if v.get("id").and_then(Value::as_u64) != Some(id) {
                        continue;
                    }
                    if let Some(e) = v.get("error") {
                        return Err(e.get("message").and_then(Value::as_str).unwrap_or("error").to_string());
                    }
                    return Ok(v.get("result").cloned().unwrap_or(Value::Null));
                }
                Ok(FromAgent::Exited(_)) => return Err("the SolidCraft tools stopped".into()),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) if std::time::Instant::now() < deadline => {}
                Err(_) => return Err("the SolidCraft tools did not answer".into()),
            }
        }
    }

    fn start_mcp(&mut self) -> Result<(), String> {
        if self.mcp.is_none() {
            return Ok(());
        }
        self.mcp_call(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "solidcraft-agent-chat", "version": env!("CARGO_PKG_VERSION")}}),
        )?;
        if let Some(m) = &self.mcp {
            m.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        }
        let list = self.mcp_call("tools/list", json!({}))?;
        self.tools = openai_tools(&list);
        Ok(())
    }

    fn turn(&mut self, parts: Vec<PromptPart>) {
        self.history.push(user_message(&parts));
        // Keep the system note; drop the oldest messages, never leaving a reply first.
        while self.history.len() > MAX_HISTORY {
            self.history.remove(1);
        }
        while self.history.len() > 2 && self.history.get(1).is_some_and(|m| m["role"] != "user") {
            self.history.remove(1);
        }
        for _ in 0..MAX_REQUESTS_PER_TURN {
            // The history as of here is complete (every tool call answered): a cancelled request
            // goes back to it, so no half-finished round is left to confuse the next message.
            let sound = self.history.len();
            if self.cancel.load(Ordering::Relaxed) {
                self.emit(Event::TurnEnded { reason: "cancelled".into() });
                return;
            }
            let reply = match self.request() {
                Ok(_) if self.cancel.load(Ordering::Relaxed) => {
                    self.history.truncate(sound);
                    self.emit(Event::TurnEnded { reason: "cancelled".into() });
                    return;
                }
                Ok(r) => r,
                Err(_) if self.cancel.load(Ordering::Relaxed) => {
                    self.history.truncate(sound);
                    self.emit(Event::TurnEnded { reason: "cancelled".into() });
                    return;
                }
                Err(e) => {
                    self.emit(Event::Error(e));
                    self.emit(Event::TurnEnded { reason: "error".into() });
                    return;
                }
            };
            let msg = reply.pointer("/choices/0/message").cloned().unwrap_or(Value::Null);
            if let Some(text) = msg.get("content").and_then(Value::as_str).filter(|t| !t.is_empty()) {
                self.emit(Event::Text { thought: false, text: text.to_string() });
            }
            let calls: Vec<Value> = msg.get("tool_calls").and_then(Value::as_array).cloned().unwrap_or_default();
            let mut kept = json!({"role": "assistant", "content": msg.get("content").cloned().unwrap_or(Value::Null)});
            if !calls.is_empty() {
                kept["tool_calls"] = Value::Array(calls.clone());
            }
            self.history.push(kept);
            if calls.is_empty() {
                self.emit(Event::TurnEnded { reason: String::new() });
                return;
            }
            for (i, c) in calls.iter().enumerate().take(32) {
                let id = c.get("id").and_then(Value::as_str).map_or_else(|| format!("call-{i}"), str::to_string);
                let name = c.pointer("/function/name").and_then(Value::as_str).unwrap_or("").to_string();
                let parsed = tool_arguments(c.pointer("/function/arguments"));
                let args = parsed.clone().unwrap_or_else(|_| json!({}));
                let title = tool_title(&name, &args);
                let tool_id = format!("{id}-{}", self.next);
                self.emit(Event::Tool {
                    id: tool_id.clone(),
                    title: Some(title),
                    kind: Some(String::new()),
                    status: Some(ToolStatus::InProgress),
                    output: None,
                });
                let (text, failed) = if self.cancel.load(Ordering::Relaxed) {
                    ("cancelled by the user".to_string(), true)
                } else if let Err(e) = &parsed {
                    // A call whose arguments do not parse is answered, never run with `{}`.
                    (format!("Not run: {e}"), true)
                } else if HIDDEN_TOOLS.contains(&name.as_str())
                    || !self.tools.iter().any(|t| t.pointer("/function/name").and_then(Value::as_str) == Some(name.as_str()))
                {
                    (format!("unknown tool `{name}`"), true)
                } else {
                    match self.mcp_call("tools/call", json!({"name": name, "arguments": args})) {
                        Ok(r) => tool_text(&r),
                        Err(e) => (e, true),
                    }
                };
                let shown: String = text.chars().take(2000).collect();
                self.emit(Event::Tool {
                    id: tool_id,
                    title: None,
                    kind: None,
                    status: Some(if failed { ToolStatus::Failed } else { ToolStatus::Completed }),
                    output: Some(shown),
                });
                self.history.push(json!({"role": "tool", "tool_call_id": id, "content": text}));
            }
        }
        self.emit(Event::Text { thought: false, text: format!("\n({})", crate::i18n::tr("ui.ai_request_limit")) });
        self.emit(Event::TurnEnded { reason: "max_turn_requests".into() });
    }

    fn request(&self) -> Result<Value, String> {
        let mut body = json!({"model": self.cfg.model, "messages": self.history});
        if !self.tools.is_empty() {
            body["tools"] = Value::Array(self.tools.clone());
            if self.no_reasoning.get() {
                body["reasoning_effort"] = json!("none");
            }
        }
        let (status, v) = self.post(&body)?;
        // Some OpenAI models refuse function tools on this endpoint unless the reasoning effort is
        // "none" (HTTP 400 naming `reasoning_effort`): ask again once with it, and keep it for the
        // next requests.
        let refused = status == 400 && !self.tools.is_empty() && !self.no_reasoning.get() && http_error(status, &v).contains("reasoning_effort");
        if refused {
            self.no_reasoning.set(true);
            body["reasoning_effort"] = json!("none");
            let (status, v) = self.post(&body)?;
            return if status == 200 { Ok(v) } else { Err(http_error(status, &v)) };
        }
        if status != 200 {
            return Err(http_error(status, &v));
        }
        Ok(v)
    }

    fn post(&self, body: &Value) -> Result<(u16, Value), String> {
        let mut headers = vec![("Content-Type".to_string(), "application/json".to_string())];
        if let Some(k) = &self.cfg.key {
            headers.push(("Authorization".into(), format!("Bearer {k}")));
        }
        let req = HttpRequest {
            method: "POST",
            url: format!("{}/chat/completions", self.cfg.base_url),
            headers,
            body: Some(body.to_string().into_bytes()),
            timeout_s: REQUEST_TIMEOUT_S,
        };
        // The request runs on a thread of its own so that Stop does not wait for the model: a
        // cancelled one is abandoned (the host's own timeout ends it) and its answer dropped.
        let (tx, rx) = channel();
        let http = self.cfg.http.clone();
        std::thread::spawn(move || {
            let _ = tx.send(http(&req));
        });
        let (status, bytes) = loop {
            if self.cancel.load(Ordering::Relaxed) {
                return Err("cancelled by the user".into());
            }
            match rx.recv_timeout(CANCEL_POLL) {
                Ok(r) => break r?,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => return Err("the request ended without an answer".into()),
            }
        };
        Ok((status, serde_json::from_slice(&bytes).unwrap_or(Value::Null)))
    }
}

/// An endpoint's refusal as people read it.
pub fn http_error(status: u16, body: &Value) -> String {
    let detail =
        body.pointer("/error/message").or_else(|| body.get("error")).and_then(Value::as_str).unwrap_or("").chars().take(300).collect::<String>();
    let what = match status {
        401 | 403 => crate::i18n::tr("ui.ai_http_key"),
        404 => crate::i18n::tr("ui.ai_http_not_found"),
        429 => crate::i18n::tr("ui.ai_http_busy"),
        _ => "",
    };
    format!(
        "HTTP {status}{}{}",
        if what.is_empty() { String::new() } else { format!(": {what}") },
        if detail.is_empty() { String::new() } else { format!(" ({detail})") }
    )
}

/// A tool call's arguments as an object: the JSON text a model sends (or an object, for servers
/// that send one), or why they cannot be used. Missing or empty arguments are an empty object.
fn tool_arguments(raw: Option<&Value>) -> Result<Value, String> {
    let text = match raw {
        None | Some(Value::Null) => return Ok(json!({})),
        Some(v @ Value::Object(_)) => return Ok(v.clone()),
        Some(Value::String(s)) => s.trim(),
        Some(_) => return Err("the tool arguments must be a JSON object".into()),
    };
    if text.is_empty() {
        return Ok(json!({}));
    }
    match serde_json::from_str::<Value>(text) {
        Ok(v @ Value::Object(_)) => Ok(v),
        Ok(_) => Err("the tool arguments must be a JSON object".into()),
        Err(e) => Err(format!("the tool arguments were not valid JSON ({e}); send the call again with a JSON object")),
    }
}

/// The message for what the user typed (and the view, as a data URL).
fn user_message(parts: &[PromptPart]) -> Value {
    let images = parts.iter().any(|p| matches!(p, PromptPart::Image { .. }));
    if !images {
        let text: Vec<&str> = parts.iter().filter_map(|p| if let PromptPart::Text(t) = p { Some(t.as_str()) } else { None }).collect();
        return json!({"role": "user", "content": text.join("\n")});
    }
    let content: Vec<Value> = parts
        .iter()
        .filter_map(|p| match p {
            PromptPart::Text(t) => Some(json!({"type": "text", "text": t})),
            PromptPart::Image { mime, base64 } => Some(json!({"type": "image_url", "image_url": {"url": format!("data:{mime};base64,{base64}")}})),
            // This endpoint takes no documents (the chat doesn't send any); the message names the file.
            PromptPart::Document { .. } => None,
        })
        .collect();
    json!({"role": "user", "content": content})
}

/// MCP `tools/list` as OpenAI function tools (the hidden ones left out).
pub fn openai_tools(list: &Value) -> Vec<Value> {
    list.get("tools")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .take(128)
                .filter_map(|t| {
                    let name = t.get("name")?.as_str()?;
                    if HIDDEN_TOOLS.contains(&name) {
                        return None;
                    }
                    let params = t.get("inputSchema").cloned().filter(Value::is_object).unwrap_or(json!({"type": "object", "properties": {}}));
                    Some(json!({"type": "function", "function": {
                        "name": name,
                        "description": t.get("description").and_then(Value::as_str).unwrap_or(""),
                        "parameters": params,
                    }}))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A tool call's result as text for the model, and whether it failed.
fn tool_text(r: &Value) -> (String, bool) {
    let failed = r.get("isError").and_then(Value::as_bool).unwrap_or(false);
    let parts: Vec<String> = r
        .get("content")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .take(16)
                .map(|c| match c.get("type").and_then(Value::as_str) {
                    Some("text") => c.get("text").and_then(Value::as_str).unwrap_or("").to_string(),
                    Some("image") => "[a picture of the view]".to_string(),
                    _ => String::new(),
                })
                .collect()
        })
        .unwrap_or_default();
    let mut text = parts.join("\n");
    if text.len() > MAX_TOOL_TEXT {
        let mut cut = MAX_TOOL_TEXT;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        text.push_str("\n…(cut)");
    }
    (text, failed)
}

/// What a tool call card says: the tool and, for `execute` and `batch`, the commands.
fn tool_title(name: &str, args: &Value) -> String {
    match name {
        "execute" => format!("execute: {}", args.get("command").and_then(Value::as_str).unwrap_or("?")),
        "batch" => {
            let ids: Vec<&str> = args
                .get("commands")
                .and_then(Value::as_array)
                .map(|c| c.iter().filter_map(|s| s.get("command").and_then(Value::as_str)).take(4).collect())
                .unwrap_or_default();
            format!("batch: {}", ids.join(", "))
        }
        _ => name.to_string(),
    }
}

/// `GET {base_url}/models`: the model ids, in the order the endpoint lists them.
fn model_ids(http: &HttpFn, base_url: &str, headers: Vec<(String, String)>) -> Result<Vec<String>, String> {
    let req = HttpRequest { method: "GET", url: format!("{base_url}/models"), headers, body: None, timeout_s: 15 };
    let (status, bytes) = http(&req)?;
    let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if status != 200 {
        return Err(http_error(status, &v));
    }
    Ok(v.get("data")
        .and_then(Value::as_array)
        .map(|a| a.iter().take(500).filter_map(|m| m.get("id").and_then(Value::as_str).map(String::from)).collect())
        .unwrap_or_default())
}

/// The endpoint's models for the picker.
pub fn model_list(http: &HttpFn, base_url: &str, key: Option<&str>) -> Result<Vec<ModelInfo>, String> {
    let mut headers = Vec::new();
    if let Some(k) = key {
        if !super::agents::key_safe_for(base_url) {
            return Err(crate::i18n::tr("ui.ai_key_needs_https").to_string());
        }
        headers.push(("Authorization".to_string(), format!("Bearer {k}")));
    }
    Ok(model_ids(http, base_url, headers)?.into_iter().map(|id| ModelInfo { name: id.clone(), id, description: String::new() }).collect())
}

/// Ask an endpoint for its models (Test connection): how many, and whether `model` is one.
pub fn test_endpoint(http: &HttpFn, base_url: &str, key: Option<&str>, model: &str) -> Result<String, String> {
    let mut headers = Vec::new();
    if let Some(k) = key {
        if !super::agents::key_safe_for(base_url) {
            return Err(crate::i18n::tr("ui.ai_key_needs_https").to_string());
        }
        headers.push(("Authorization".to_string(), format!("Bearer {k}")));
    }
    let ids = model_ids(http, base_url, headers)?;
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    let found = model.is_empty() || ids.contains(&model);
    Ok(crate::i18n::format(if found { "ui.ai_test_ok" } else { "ui.ai_test_no_model" }, &[("count", &ids.len().to_string()), ("model", model)]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_tools_become_function_tools_without_crash_reports() {
        let list = json!({"tools": [
            {"name": "batch", "description": "Run commands", "inputSchema": {"type": "object", "properties": {"commands": {"type": "array"}}}},
            {"name": "crash_reports", "description": "x", "inputSchema": {"type": "object"}},
            {"name": "measure"},
        ]});
        let t = openai_tools(&list);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0]["function"]["name"], "batch");
        assert_eq!(t[1]["function"]["parameters"]["type"], "object");
    }

    #[test]
    fn pictures_go_as_data_urls_and_text_alone_as_a_string() {
        assert_eq!(user_message(&[PromptPart::Text("hi".into())])["content"], "hi");
        let m = user_message(&[PromptPart::Text("hi".into()), PromptPart::Image { mime: "image/png".into(), base64: "AA==".into() }]);
        assert_eq!(m["content"][1]["image_url"]["url"], "data:image/png;base64,AA==");
    }

    /// A scripted endpoint: one answer with a tool call, then a plain answer. The loop runs the
    /// tool (here without SolidCraft tools: an unknown tool fails) and reports the turn's end.
    #[test]
    fn the_tool_loop_runs_calls_and_ends_the_turn() {
        let answers = std::sync::Mutex::new(vec![
            json!({"choices": [{"message": {"role": "assistant", "content": "Done."}}]}),
            json!({"choices": [{"message": {"role": "assistant", "content": "Measuring.", "tool_calls": [{"id": "c1", "type": "function", "function": {"name": "measure", "arguments": "{}"}}]}}]}),
        ]);
        let seen = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let log = seen.clone();
        let http: HttpFn = Arc::new(move |r: &HttpRequest| {
            if r.method == "GET" {
                return Ok((200, b"{\"data\":[]}".to_vec()));
            }
            log.lock().unwrap().push(serde_json::from_slice(r.body.as_deref().unwrap_or(b"null")).unwrap());
            let a = answers.lock().unwrap().pop().unwrap_or(json!({}));
            Ok((200, a.to_string().into_bytes()))
        });
        let cfg = ApiConfig { name: "Test".into(), base_url: "http://localhost:1/v1".into(), model: "m".into(), key: None, http };
        let ctx = StartContext { cwd: std::env::temp_dir(), bridge: None, tools: None, wake: Box::new(|| {}) };
        let mut b = ApiBackend::start(cfg, ctx).unwrap();
        b.prompt(&[PromptPart::Text("measure it".into())]);
        let mut events = Vec::new();
        let start = std::time::Instant::now();
        while !events.iter().any(|e| matches!(e, Event::TurnEnded { .. })) && start.elapsed() < Duration::from_secs(10) {
            events.extend(b.poll());
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(events.first(), Some(&Event::ToolsAvailable(false)));
        assert!(events.iter().any(|e| matches!(e, Event::Ready { .. })), "{events:?}");
        assert!(events.iter().any(|e| matches!(e, Event::Tool { status: Some(ToolStatus::Failed), .. })), "{events:?}");
        assert!(events.iter().any(|e| matches!(e, Event::Text { text, .. } if text == "Done.")), "{events:?}");
        assert!(matches!(events.last(), Some(Event::TurnEnded { reason }) if reason.is_empty()), "{events:?}");
        // The second request carried the tool's answer.
        let sent = seen.lock().unwrap();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[1]["messages"].as_array().unwrap().last().unwrap()["role"], "tool");
        b.shutdown(true);
    }

    /// Cancel while a model request is in flight ends the turn at once (the request is
    /// abandoned, its late answer dropped), leaves no half-finished round in the history and
    /// the next message goes through.
    #[test]
    fn cancelling_a_request_ends_the_turn_and_the_next_message_still_works() {
        let (release_tx, release_rx) = channel::<()>();
        let release_rx = std::sync::Mutex::new(release_rx);
        let seen = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let log = seen.clone();
        let http: HttpFn = Arc::new(move |r: &HttpRequest| {
            if r.method == "GET" {
                return Ok((200, b"{\"data\":[]}".to_vec()));
            }
            let n = {
                let mut l = log.lock().unwrap();
                l.push(serde_json::from_slice(r.body.as_deref().unwrap_or(b"null")).unwrap());
                l.len()
            };
            if n == 1 {
                // The first request hangs until the test lets it go, then answers late.
                let _ = release_rx.lock().unwrap().recv_timeout(Duration::from_secs(20));
                return Ok((200, json!({"choices": [{"message": {"role": "assistant", "content": "Too late."}}]}).to_string().into_bytes()));
            }
            Ok((200, json!({"choices": [{"message": {"role": "assistant", "content": "Steered."}}]}).to_string().into_bytes()))
        });
        let cfg = ApiConfig { name: "Test".into(), base_url: "http://localhost:1/v1".into(), model: "m".into(), key: None, http };
        let ctx = StartContext { cwd: std::env::temp_dir(), bridge: None, tools: None, wake: Box::new(|| {}) };
        let mut b = ApiBackend::start(cfg, ctx).unwrap();
        let wait = |b: &mut ApiBackend, events: &mut Vec<Event>| {
            let start = std::time::Instant::now();
            while !events.iter().any(|e| matches!(e, Event::TurnEnded { .. })) && start.elapsed() < Duration::from_secs(10) {
                events.extend(b.poll());
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        b.prompt(&[PromptPart::Text("long job".into())]);
        let start = std::time::Instant::now();
        while seen.lock().unwrap().is_empty() && start.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(10));
        }
        b.cancel();
        let mut events = Vec::new();
        wait(&mut b, &mut events);
        assert!(start.elapsed() < Duration::from_secs(8), "the turn ended while the request still hung");
        assert!(matches!(events.last(), Some(Event::TurnEnded { reason }) if reason == "cancelled"), "{events:?}");
        assert!(!events.iter().any(|e| matches!(e, Event::Text { .. } | Event::Error(_))), "{events:?}");
        // The next message is answered, and the late answer of the first never shows up.
        b.prompt(&[PromptPart::Text("[interrupted] do this instead".into())]);
        let mut events = Vec::new();
        wait(&mut b, &mut events);
        let _ = release_tx.send(());
        assert!(events.iter().any(|e| matches!(e, Event::Text { text, .. } if text == "Steered.")), "{events:?}");
        assert!(matches!(events.last(), Some(Event::TurnEnded { reason }) if reason.is_empty()), "{events:?}");
        let sent = seen.lock().unwrap();
        let roles: Vec<&str> = sent[1]["messages"].as_array().unwrap().iter().map(|m| m["role"].as_str().unwrap_or("")).collect();
        assert_eq!(roles, ["system", "user", "user"], "no reply or tool call of the cancelled round is left");
        b.shutdown(true);
    }

    /// New chat resets the model's history to the system note: the next request carries nothing
    /// of the conversation before it.
    #[test]
    fn a_new_chat_resets_the_history_to_the_system_note() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let log = seen.clone();
        let http: HttpFn = Arc::new(move |r: &HttpRequest| {
            if r.method == "GET" {
                return Ok((200, b"{\"data\":[]}".to_vec()));
            }
            log.lock().unwrap().push(serde_json::from_slice(r.body.as_deref().unwrap_or(b"null")).unwrap());
            Ok((200, json!({"choices": [{"message": {"role": "assistant", "content": "ok"}}]}).to_string().into_bytes()))
        });
        let cfg = ApiConfig { name: "Test".into(), base_url: "http://localhost:1/v1".into(), model: "m".into(), key: None, http };
        let ctx = StartContext { cwd: std::env::temp_dir(), bridge: None, tools: None, wake: Box::new(|| {}) };
        let mut b = ApiBackend::start(cfg, ctx).unwrap();
        let run = |b: &mut ApiBackend, f: &dyn Fn(&mut ApiBackend), want: usize| {
            f(b);
            let mut events = Vec::new();
            let start = std::time::Instant::now();
            while events.iter().filter(|e| matches!(e, Event::TurnEnded { .. } | Event::Ready { .. })).count() < want
                && start.elapsed() < Duration::from_secs(10)
            {
                events.extend(b.poll());
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        run(&mut b, &|b| b.prompt(&[PromptPart::Text("one".into())]), 2);
        run(&mut b, &|b| b.new_session(), 1);
        run(&mut b, &|b| b.prompt(&[PromptPart::Text("two".into())]), 1);
        let sent = seen.lock().unwrap();
        let roles: Vec<&str> = sent[1]["messages"].as_array().unwrap().iter().map(|m| m["role"].as_str().unwrap_or("")).collect();
        assert_eq!(roles, ["system", "user"]);
        assert_eq!(sent[1]["messages"][1]["content"], "two");
        b.shutdown(true);
    }

    /// The endpoint's `/models` fills the picker (with the current model first if it is not
    /// listed), and choosing one changes the model of the next request.
    #[test]
    fn the_endpoint_lists_models_and_a_choice_changes_the_next_request() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let log = seen.clone();
        let http: HttpFn = Arc::new(move |r: &HttpRequest| {
            if r.method == "GET" {
                assert!(r.url.ends_with("/models"));
                return Ok((200, json!({"data": [{"id": "big"}, {"id": "small"}]}).to_string().into_bytes()));
            }
            log.lock().unwrap().push(serde_json::from_slice(r.body.as_deref().unwrap_or(b"null")).unwrap());
            Ok((200, json!({"choices": [{"message": {"role": "assistant", "content": "ok"}}]}).to_string().into_bytes()))
        });
        let cfg = ApiConfig { name: "Test".into(), base_url: "http://localhost:1/v1".into(), model: "local".into(), key: None, http };
        let ctx = StartContext { cwd: std::env::temp_dir(), bridge: None, tools: None, wake: Box::new(|| {}) };
        let mut b = ApiBackend::start(cfg, ctx).unwrap();
        let mut events = Vec::new();
        let start = std::time::Instant::now();
        while !events.iter().any(|e| matches!(e, Event::Models { .. })) && start.elapsed() < Duration::from_secs(10) {
            events.extend(b.poll());
            std::thread::sleep(Duration::from_millis(10));
        }
        let Some(Event::Models { current, available }) = events.iter().find(|e| matches!(e, Event::Models { .. })) else { panic!("{events:?}") };
        assert_eq!(current, "local");
        assert_eq!(available.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["local", "big", "small"]);
        b.set_model("big");
        b.prompt(&[PromptPart::Text("hi".into())]);
        let start = std::time::Instant::now();
        while seen.lock().unwrap().is_empty() && start.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(seen.lock().unwrap()[0]["model"], "big");
        b.shutdown(true);
    }

    #[test]
    fn tool_arguments_are_an_object_or_an_error() {
        assert_eq!(tool_arguments(None), Ok(json!({})));
        assert_eq!(tool_arguments(Some(&json!(""))), Ok(json!({})));
        assert_eq!(tool_arguments(Some(&json!("{\"a\": 1}"))), Ok(json!({"a": 1})));
        assert_eq!(tool_arguments(Some(&json!({"a": 1}))), Ok(json!({"a": 1})));
        for bad in [json!("{command: solid.box"), json!("[1]"), json!("7"), json!(7), json!([1])] {
            assert!(tool_arguments(Some(&bad)).is_err(), "{bad}");
        }
    }

    /// A call with arguments that are not JSON is answered with the reason, not run with `{}`.
    #[test]
    fn a_call_with_broken_arguments_is_not_run() {
        let answers = std::sync::Mutex::new(vec![
            json!({"choices": [{"message": {"role": "assistant", "content": "Sorry."}}]}),
            json!({"choices": [{"message": {"role": "assistant", "content": "", "tool_calls": [{"id": "c1", "type": "function", "function": {"name": "batch", "arguments": "{commands: [oops"}}]}}]}),
        ]);
        let seen = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let log = seen.clone();
        let http: HttpFn = Arc::new(move |r: &HttpRequest| {
            if r.method == "GET" {
                return Ok((200, b"{\"data\":[]}".to_vec()));
            }
            log.lock().unwrap().push(serde_json::from_slice(r.body.as_deref().unwrap_or(b"null")).unwrap());
            Ok((200, answers.lock().unwrap().pop().unwrap_or(json!({})).to_string().into_bytes()))
        });
        let cfg = ApiConfig { name: "Test".into(), base_url: "http://localhost:1/v1".into(), model: "m".into(), key: None, http };
        let ctx = StartContext { cwd: std::env::temp_dir(), bridge: None, tools: None, wake: Box::new(|| {}) };
        let mut b = ApiBackend::start(cfg, ctx).unwrap();
        b.prompt(&[PromptPart::Text("go".into())]);
        let mut events = Vec::new();
        let start = std::time::Instant::now();
        while !events.iter().any(|e| matches!(e, Event::TurnEnded { .. })) && start.elapsed() < Duration::from_secs(10) {
            events.extend(b.poll());
            std::thread::sleep(Duration::from_millis(10));
        }
        let failed = events.iter().find_map(|e| match e {
            Event::Tool { status: Some(ToolStatus::Failed), output: Some(o), .. } => Some(o.clone()),
            _ => None,
        });
        assert!(failed.is_some_and(|o| o.contains("Not run") && o.contains("not valid JSON")), "{events:?}");
        // The model was told, in the next request.
        let sent = seen.lock().unwrap();
        let last = sent[1]["messages"].as_array().unwrap().last().unwrap().clone();
        assert!(last["content"].as_str().unwrap_or("").contains("not valid JSON"), "{last}");
        b.shutdown(true);
    }

    #[test]
    fn keys_never_go_over_plain_http_to_another_computer() {
        let http: HttpFn = Arc::new(|_: &HttpRequest| Ok((200, b"{}".to_vec())));
        assert!(test_endpoint(&http, "http://10.0.0.2:11434/v1", Some("k"), "m").is_err());
        let cfg = ApiConfig { name: "x".into(), base_url: "http://10.0.0.2/v1".into(), model: "m".into(), key: Some("k".into()), http };
        let ctx = StartContext { cwd: std::env::temp_dir(), bridge: None, tools: None, wake: Box::new(|| {}) };
        assert!(ApiBackend::start(cfg, ctx).is_err());
    }

    /// A worker with one function tool whose endpoint answers each POST with the next scripted
    /// (status, body); the requests it received are returned too.
    fn scripted_worker(answers: Vec<(u16, Value)>) -> (Worker, Arc<std::sync::Mutex<Vec<Value>>>) {
        let answers = std::sync::Mutex::new(answers.into_iter().rev().collect::<Vec<_>>());
        let seen = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let log = seen.clone();
        let http: HttpFn = Arc::new(move |r: &HttpRequest| {
            log.lock().unwrap().push(serde_json::from_slice(r.body.as_deref().unwrap()).unwrap());
            let (status, v) = answers.lock().unwrap().pop().expect("no request beyond the script");
            Ok((status, v.to_string().into_bytes()))
        });
        let cfg = ApiConfig { name: "Test".into(), base_url: "http://localhost:1/v1".into(), model: "m".into(), key: Some("sk-secret".into()), http };
        let (events, _rx) = channel();
        let w = Worker {
            cfg,
            mcp: None,
            next: 0,
            tools: vec![json!({"type": "function", "function": {"name": "measure"}})],
            history: vec![json!({"role": "user", "content": "hi"})],
            events,
            wake: Arc::new(|| {}),
            cancel: Arc::new(AtomicBool::new(false)),
            no_reasoning: Default::default(),
        };
        (w, seen)
    }

    fn refusal() -> (u16, Value) {
        (400, json!({"error": {"message": "Function tools with reasoning_effort are not supported for gpt-6.1-sol in /v1/chat/completions."}}))
    }

    /// Newer OpenAI models answer 400 to function tools without `reasoning_effort: none`: the
    /// request is replayed once with it, and the next ones carry it at once.
    #[test]
    fn a_refusal_of_tools_with_reasoning_effort_is_replayed_once_with_none() {
        let ok = json!({"choices": [{"message": {"role": "assistant", "content": "Done."}}]});
        let (w, seen) = scripted_worker(vec![refusal(), (200, ok.clone()), (200, ok.clone())]);
        assert_eq!(w.request().unwrap(), ok);
        assert_eq!(w.request().unwrap(), ok);
        let sent = seen.lock().unwrap();
        assert_eq!(sent.len(), 3);
        assert!(sent[0].get("reasoning_effort").is_none());
        assert_eq!(sent[1]["reasoning_effort"], "none");
        assert_eq!(sent[2]["reasoning_effort"], "none");
    }

    /// No loop: a second refusal is the error, after exactly two requests; the message shows the
    /// endpoint's words but never the key; an unrelated 400 is not replayed at all.
    #[test]
    fn the_replay_happens_once_and_only_for_that_refusal() {
        let (w, seen) = scripted_worker(vec![refusal(), refusal()]);
        let e = w.request().unwrap_err();
        assert!(e.starts_with("HTTP 400") && e.contains("reasoning_effort") && !e.contains("sk-secret"), "{e}");
        assert_eq!(seen.lock().unwrap().len(), 2);
        let (w, seen) = scripted_worker(vec![(400, json!({"error": {"message": "context length exceeded"}}))]);
        assert!(w.request().unwrap_err().contains("context length"));
        assert_eq!(seen.lock().unwrap().len(), 1);
    }
}
