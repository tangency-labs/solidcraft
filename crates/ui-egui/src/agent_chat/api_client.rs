//! The API backend (desktop only): a model behind an endpoint SolidCraft talks to itself, either
//! OpenAI-compatible (Ollama, LM Studio, OpenAI, any URL the user gives) or Anthropic-compatible
//! (the Anthropic Messages API, or an endpoint that speaks it). SolidCraft runs the tool loop
//! itself, with the same tools the ACP agents get: it starts the same MCP server (`solidcraft-cli
//! mcp --connect` to the chat's own endpoint) and is its client, so every tool call passes the
//! same gate, limits and folder checks. The key, when there is one, comes from the system keychain
//! for this connection only.
//!
//! The loop runs on a thread of its own: a model request can take a while.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use serde_json::{Map, Value, json};

use super::acp::ModelInfo;
use super::agents::Protocol;
use super::backend::{ChatBackend, Event, HttpFn, HttpRequest, PromptPart, StartContext, ToolStatus};
use super::process::{AgentProcess, FromAgent, ProcessHandle};

/// Longest tool result handed back to the model.
const MAX_TOOL_TEXT: usize = 20_000;
/// Messages kept in the conversation (the system note stays).
const MAX_HISTORY: usize = 240;
/// Pictures of the view kept in the conversation. The whole history goes with every request
/// (up to `MAX_REQUESTS_PER_TURN` per message), so older views are swapped for a short note:
/// otherwise each attached view makes every later request bigger, until the endpoint refuses it.
const MAX_HISTORY_IMAGES: usize = 2;
/// What an older view becomes in the history.
const DROPPED_VIEW_NOTE: &str = "[an earlier picture of the view, removed to keep the request small]";
/// What an earlier answer that was only thinking says once its thinking is left out (#9417).
const DROPPED_THOUGHT_NOTE: &str = "[earlier reasoning, left out]";
/// How long one tool call may take (it may wait for the user's Allow, declined after four
/// minutes). It does not depend on the endpoint's request timeout: a tool call is SolidCraft's own
/// work and the user's answer, never a wait for the model, so a longer request timeout does not
/// need a longer tool timeout.
const TOOL_TIMEOUT: Duration = Duration::from_secs(330);
/// How often a waiting request or tool call looks at the cancel flag.
const CANCEL_POLL: Duration = Duration::from_millis(40);
/// The Anthropic Messages API requires `max_tokens`; it caps one answer, which is plenty for a
/// chat reply and for the tool calls in it.
const ANTHROPIC_MAX_TOKENS: u64 = 8192;
/// The API version sent to Anthropic-compatible endpoints. It is the version every such endpoint
/// knows: DeepSeek ignores it, Qwen does not require it, and the Claude API still accepts it.
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// The connection test's own timeout: it asks for a token or two, not a whole answer.
const TEST_TIMEOUT_S: u64 = 60;

/// Tools the model is not offered.
const HIDDEN_TOOLS: &[&str] = &["crash_reports"];

/// Chat counter for OpenCode Go session IDs, next to the wall clock below.
static GO_SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Counter for synthesized Gemini call ids; unique across rounds so a later
/// answer maps back to the right function name (see `go_call_names`).
static GO_GEMINI_CALL_COUNTER: AtomicU64 = AtomicU64::new(1);

/// A stable session ID for OpenCode Go's routing and prompt cache; one per chat,
/// not per request. Unique per chat on this machine; the gateway only needs an
/// opaque stable string.
fn new_session_id() -> String {
    let n = GO_SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("solidcraft-{t:x}-{n:x}-{:x}", std::process::id())
}

/// Whether `base_url` is an OpenCode Go endpoint: model requests to all of them
/// carry `x-opencode-session` (the `zen/go` gateway and the per-model
/// `inference/go/...` endpoints alike). The model list and connection test are
/// plain GETs and send none.
fn is_opencode_go(base_url: &str) -> bool {
    let u = base_url.trim().to_ascii_lowercase();
    u.starts_with("https://opencode.ai/") && u.contains("/go")
}

/// Which wire protocol one OpenCode Go model speaks. Go exposes one endpoint per
/// model family (see https://opencode.ai/docs/go/#endpoints); the preset stays a
/// single base URL and each model is routed to its own shape.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum GoProtocol {
    Chat,
    Responses,
    Messages,
    Gemini,
}

/// The protocol `model` speaks on a Go endpoint, by family (prefix table in
/// match order: `minimax-m3.1-flash` must precede `minimax-`). Anything
/// unknown stays on chat completions; a mismatch at request time falls through
/// to the next protocol (see `should_try_next_go`), so new models keep working.
/// Adding a model is one prefix in the right row.
const GO_PROTOCOL_TABLE: &[(&[&str], GoProtocol)] = &[
    (&["gemini-"], GoProtocol::Gemini),
    (&["grok-", "gpt-", "muse-spark-"], GoProtocol::Responses),
    (&["minimax-m3.1-flash"], GoProtocol::Chat),
    (&["minimax-", "claude-", "qwen"], GoProtocol::Messages),
];

fn go_protocol(model: &str) -> GoProtocol {
    let m = model.trim().to_ascii_lowercase();
    for (prefixes, protocol) in GO_PROTOCOL_TABLE {
        if prefixes.iter().any(|p| m.starts_with(p)) {
            return *protocol;
        }
    }
    GoProtocol::Chat
}

/// The Go gateway root every per-model endpoint hangs under.
const GO_BASE: &str = "https://opencode.ai/inference/go";

/// Most tool calls the loop answers from one reply: every call kept here gets
/// its own `tool` message on the next turn.
const GO_MAX_TOOL_CALLS: usize = 32;

/// The documented Go endpoint for a model's protocol family.
fn go_url(model: &str, protocol: GoProtocol) -> String {
    match protocol {
        GoProtocol::Gemini => {
            let m = model.trim().to_ascii_lowercase();
            format!("{GO_BASE}/google/v1beta/models/{m}:generateContent")
        }
        GoProtocol::Responses => format!("{GO_BASE}/openai/v1/responses"),
        GoProtocol::Messages => format!("{GO_BASE}/anthropic/v1/messages"),
        GoProtocol::Chat => format!("{GO_BASE}/openai/v1/chat/completions"),
    }
}

/// The gateway's refusal wording, lowercased, from either error shape.
fn go_error_detail(body: &Value) -> String {
    body.pointer("/error/message").or_else(|| body.get("error")).and_then(Value::as_str).unwrap_or("").to_ascii_lowercase()
}

/// Whether another Go protocol may succeed after this refusal: the gateway
/// naming a protocol mismatch, or a per-path credential refusal while the same
/// key works on the sibling paths.
fn should_try_next_go(status: u16, body: &Value) -> bool {
    let detail = go_error_detail(body);
    if detail.contains("does not support this protocol") {
        return true;
    }
    if status == 401 {
        return detail.contains("credential");
    }
    false
}

/// The shared `(name, description, params)` of one OpenAI function tool, if it has one.
fn go_fn_parts(t: &Value) -> Option<(&Value, &str, Value)> {
    let f = t.get("function")?;
    let params = f.get("parameters").cloned().unwrap_or(json!({"type": "object"}));
    Some((f.get("name")?, f.get("description").and_then(Value::as_str).unwrap_or(""), params))
}

/// One tool in an Anthropic/Responses wrapper: `params_key` is `input_schema`
/// (Messages) or `parameters` (Responses, with `"type": "function"`).
fn go_shaped_tool(name: &Value, desc: &str, params: Value, params_key: &str, type_tag: Option<&str>) -> Value {
    let mut m = Map::new();
    if let Some(t) = type_tag {
        m.insert("type".to_string(), Value::String(t.to_string()));
    }
    m.insert("name".to_string(), name.clone());
    m.insert("description".to_string(), Value::String(desc.to_string()));
    m.insert(params_key.to_string(), params);
    Value::Object(m)
}

/// OpenAI function tools as Anthropic tools for Go's Messages endpoints.
fn go_anthropic_tools(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|t| {
            let (name, desc, params) = go_fn_parts(t)?;
            Some(go_shaped_tool(name, desc, params, "input_schema", None))
        })
        .collect()
}

/// OpenAI function tools as Responses API tools for Go's Responses endpoints.
fn go_responses_tools(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|t| {
            let (name, desc, params) = go_fn_parts(t)?;
            Some(go_shaped_tool(name, desc, params, "parameters", Some("function")))
        })
        .collect()
}

/// One JSON Schema node reduced to what Gemini's Schema proto accepts: `type`
/// (a single string), `description`, `enum`, `items`, object-only `properties`
/// and string-only `required`. Everything else is dropped because the endpoint
/// rejects unknown names instead of ignoring them.
fn go_gemini_property(schema: &Value) -> Value {
    let Some(o) = schema.as_object() else { return json!({"type": "string"}) };
    let mut out = Map::new();
    let mut kind = Value::String("string".to_string());
    if let Some(t) = o.get("type").and_then(Value::as_str) {
        kind = Value::String(t.to_string());
    }
    out.insert("type".to_string(), kind);
    if let Some(d) = o.get("description").and_then(Value::as_str) {
        out.insert("description".to_string(), Value::String(d.to_string()));
    }
    if let Some(e) = o.get("enum").and_then(Value::as_array) {
        out.insert("enum".to_string(), Value::Array(e.clone()));
    }
    if let Some(items) = o.get("items") {
        out.insert("items".to_string(), go_gemini_property(items));
    }
    if let Some(props) = o.get("properties").and_then(Value::as_object) {
        out.insert("properties".to_string(), Value::Object(props.iter().map(|(k, v)| (k.clone(), go_gemini_property(v))).collect()));
    } else if out.get("type").and_then(Value::as_str) == Some("object") {
        out.insert("properties".to_string(), Value::Object(Map::new()));
    }
    if let Some(req) = o.get("required").and_then(Value::as_array)
        && !req.is_empty()
        && req.iter().all(|r| r.is_string())
    {
        out.insert("required".to_string(), Value::Array(req.clone()));
    }
    Value::Object(out)
}

/// A tool's parameters reduced to a Gemini object schema (see `go_gemini_property`).
fn go_gemini_schema(schema: &Value) -> Value {
    let mut root = go_gemini_property(schema);
    root["type"] = Value::String("object".to_string());
    if root.get("properties").is_none() {
        root["properties"] = Value::Object(Map::new());
    }
    root
}

/// OpenAI function tools as Gemini function declarations.
fn go_gemini_tools(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|t| {
            let (name, desc, params) = go_fn_parts(t)?;
            Some(json!({
                "name": name,
                "description": desc,
                "parameters": go_gemini_schema(&params),
            }))
        })
        .collect()
}

/// Append `item` to the last message with `role` (creating one), so roles keep
/// alternating: Anthropic/Messages merges into `content`, Gemini into `parts`.
/// One turn answers each tool call with its own OpenAI `tool` message here.
fn go_push_item(msgs: &mut Vec<Value>, role: &str, key: &str, item: Value) {
    if let Some(last) = msgs.last_mut() {
        let same = last.get("role").and_then(Value::as_str) == Some(role);
        if same && let Some(items) = last.get_mut(key).and_then(Value::as_array_mut) {
            items.push(item);
            return;
        }
    }
    let mut m = Map::new();
    m.insert("role".to_string(), Value::String(role.to_string()));
    m.insert(key.to_string(), Value::Array(vec![item]));
    msgs.push(Value::Object(m));
}

/// Append `block` to `msgs`, merging into the last message when it already has
/// `role` (Anthropic and Gemini both require alternating roles, and one turn
/// answers each tool call with its own OpenAI `tool` message here).
fn go_push_block(msgs: &mut Vec<Value>, role: &str, block: Value) {
    go_push_item(msgs, role, "content", block);
}

/// A `data:<mime>;base64,<data>` URL as its parts.
fn go_data_url(url: &str) -> Option<(&str, &str)> {
    let rest = url.strip_prefix("data:")?;
    let (mime, data) = rest.split_once(";base64,")?;
    if mime.is_empty() || data.is_empty() {
        return None;
    }
    Some((mime, data))
}

/// One OpenAI content part's text, if it has any.
fn openai_part_text(p: &Value) -> Option<&str> {
    p.get("text").and_then(Value::as_str)
}

/// One OpenAI content part's image URL, if it is an image part.
fn openai_part_image_url(p: &Value) -> Option<&str> {
    p.pointer("/image_url/url").and_then(Value::as_str)
}

/// One OpenAI tool call's id and function name.
fn openai_tool_call(c: &Value) -> (&str, &str) {
    (c.get("id").and_then(Value::as_str).unwrap_or(""), c.pointer("/function/name").and_then(Value::as_str).unwrap_or(""))
}

/// OpenAI-style history as Anthropic messages; the system note is dropped (the
/// caller sends its own) and tool answers merge so roles alternate.
fn go_anthropic_messages(history: &[Value]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for m in history {
        let role = m.get("role").and_then(Value::as_str).unwrap_or("");
        match role {
            "user" => match m.get("content") {
                Some(Value::String(t)) => go_push_block(&mut out, "user", json!({"type": "text", "text": t})),
                Some(Value::Array(parts)) => {
                    for p in parts {
                        if let Some(t) = openai_part_text(p) {
                            go_push_block(&mut out, "user", json!({"type": "text", "text": t}));
                        } else if let Some(url) = openai_part_image_url(p)
                            && let Some((mime, data)) = go_data_url(url)
                        {
                            go_push_block(&mut out, "user", json!({"type": "image", "source": {"type": "base64", "media_type": mime, "data": data}}));
                        }
                    }
                }
                _ => {}
            },
            "assistant" => {
                if let Some(t) = m.get("content").and_then(Value::as_str).filter(|t| !t.is_empty()) {
                    go_push_block(&mut out, "assistant", json!({"type": "text", "text": t}));
                }
                if let Some(calls) = m.get("tool_calls").and_then(Value::as_array) {
                    for c in calls {
                        let (id, name) = openai_tool_call(c);
                        let input = tool_arguments(c.pointer("/function/arguments")).unwrap_or_else(|_| json!({}));
                        go_push_block(&mut out, "assistant", json!({"type": "tool_use", "id": id, "name": name, "input": input}));
                    }
                }
            }
            "tool" => {
                let id = m.get("tool_call_id").and_then(Value::as_str).unwrap_or("");
                let text = m.get("content").and_then(Value::as_str).unwrap_or("");
                go_push_block(&mut out, "user", json!({"type": "tool_result", "tool_use_id": id, "content": text}));
            }
            _ => {}
        }
    }
    out
}

/// OpenAI-style history as Responses API input items, with the system note
/// first (no history clone: the note is pushed, not prepended as a message).
/// Assistant history takes `output_text` parts: `input_text` is rejected there.
fn go_responses_input(system: &str, history: &[Value]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    out.push(json!({"type": "message", "role": "system", "content": [{"type": "input_text", "text": system}]}));
    for m in history {
        let role = m.get("role").and_then(Value::as_str).unwrap_or("");
        match role {
            "system" => {
                if let Some(t) = m.get("content").and_then(Value::as_str) {
                    out.push(json!({"type": "message", "role": "system", "content": [{"type": "input_text", "text": t}]}));
                }
            }
            "user" => match m.get("content") {
                Some(Value::String(t)) => {
                    out.push(json!({"type": "message", "role": "user", "content": [{"type": "input_text", "text": t}]}));
                }
                Some(Value::Array(parts)) => {
                    let content: Vec<Value> = parts
                        .iter()
                        .filter_map(|p| {
                            if let Some(t) = openai_part_text(p) {
                                Some(json!({"type": "input_text", "text": t}))
                            } else {
                                openai_part_image_url(p).map(|url| json!({"type": "input_image", "image_url": url}))
                            }
                        })
                        .collect();
                    if !content.is_empty() {
                        out.push(json!({"type": "message", "role": "user", "content": content}));
                    }
                }
                _ => {}
            },
            "assistant" => {
                if let Some(t) = m.get("content").and_then(Value::as_str).filter(|t| !t.is_empty()) {
                    out.push(json!({"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": t}]}));
                }
                if let Some(calls) = m.get("tool_calls").and_then(Value::as_array) {
                    for c in calls {
                        let (id, name) = openai_tool_call(c);
                        let args = match c.pointer("/function/arguments") {
                            Some(Value::String(s)) if !s.trim().is_empty() => s.clone(),
                            Some(v @ Value::Object(_)) => v.to_string(),
                            _ => "{}".to_string(),
                        };
                        out.push(json!({
                            "type": "function_call",
                            "call_id": id,
                            "name": name,
                            "arguments": args,
                        }));
                    }
                }
            }
            "tool" => {
                out.push(json!({
                    "type": "function_call_output",
                    "call_id": m.get("tool_call_id").and_then(Value::as_str).unwrap_or(""),
                    "output": m.get("content").and_then(Value::as_str).unwrap_or(""),
                }));
            }
            _ => {}
        }
    }
    out
}

/// Tool call ids back to their names, so Gemini answers can name the function
/// they answer (its history items carry only the id).
fn go_call_names(history: &[Value]) -> std::collections::HashMap<String, String> {
    let mut names = std::collections::HashMap::new();
    for m in history {
        if let Some(calls) = m.get("tool_calls").and_then(Value::as_array) {
            for c in calls {
                if let (Some(id), Some(name)) = (c.get("id").and_then(Value::as_str), c.pointer("/function/name").and_then(Value::as_str)) {
                    names.insert(id.to_string(), name.to_string());
                }
            }
        }
    }
    names
}

fn go_push_part(contents: &mut Vec<Value>, role: &str, part: Value) {
    go_push_item(contents, role, "parts", part);
}

/// OpenAI-style history as Gemini contents (the system note is dropped; the
/// caller sends its own instruction). `sigs` carries the thought signatures of
/// earlier function-call parts, by kept call id: Gemini 3 rejects a tool round
/// whose first function call lacks the signature it sent.
fn go_gemini_contents(history: &[Value], sigs: &std::collections::HashMap<String, String>) -> Vec<Value> {
    // Tool answers name their function via this map; turns without tool
    // results skip the extra history pass entirely (the common case).
    let names = if history.iter().any(|m| m.get("role").and_then(Value::as_str) == Some("tool")) {
        go_call_names(history)
    } else {
        std::collections::HashMap::new()
    };
    let mut out: Vec<Value> = Vec::new();
    for m in history {
        let role = m.get("role").and_then(Value::as_str).unwrap_or("");
        match role {
            "user" => match m.get("content") {
                Some(Value::String(t)) => go_push_part(&mut out, "user", json!({"text": t})),
                Some(Value::Array(parts)) => {
                    for p in parts {
                        if let Some(t) = openai_part_text(p) {
                            go_push_part(&mut out, "user", json!({"text": t}));
                        } else if let Some(url) = openai_part_image_url(p)
                            && let Some((mime, data)) = go_data_url(url)
                        {
                            go_push_part(&mut out, "user", json!({"inlineData": {"mimeType": mime, "data": data}}));
                        }
                    }
                }
                _ => {}
            },
            "assistant" => {
                if let Some(t) = m.get("content").and_then(Value::as_str).filter(|t| !t.is_empty()) {
                    go_push_part(&mut out, "model", json!({"text": t}));
                }
                if let Some(calls) = m.get("tool_calls").and_then(Value::as_array) {
                    for c in calls {
                        let (id, name) = openai_tool_call(c);
                        let args = tool_arguments(c.pointer("/function/arguments")).unwrap_or_else(|_| json!({}));
                        let call = json!({"name": name, "args": args});
                        go_push_part(
                            &mut out,
                            "model",
                            if let Some(sig) = sigs.get(id) {
                                json!({"functionCall": call, "thoughtSignature": sig})
                            } else {
                                json!({"functionCall": call})
                            },
                        );
                    }
                }
            }
            "tool" => {
                let id = m.get("tool_call_id").and_then(Value::as_str).unwrap_or("");
                let name = names.get(id).map(String::as_str).unwrap_or("tool");
                let text = m.get("content").and_then(Value::as_str).unwrap_or("");
                go_push_part(&mut out, "user", json!({"functionResponse": {"name": name, "response": {"output": text}}}));
            }
            _ => {}
        }
    }
    out
}

/// The tool loop's Reply from shown text and tool calls; the kept assistant
/// message stays OpenAI-shaped so the loop stores one history shape for Go.
fn go_reply_from_parts(text: Vec<(bool, String)>, calls: Vec<Call>, kept_calls: Vec<Value>) -> Reply {
    let joined: String = text.iter().map(|(_, t)| t.as_str()).collect();
    let mut assistant = json!({"role": "assistant", "content": if joined.is_empty() { Value::Null } else { Value::String(joined) }});
    if !kept_calls.is_empty() {
        assistant["tool_calls"] = Value::Array(kept_calls);
    }
    Reply { text, calls, assistant }
}

/// Shared text + capped tool-call accumulator for the three Go reply parsers.
/// The kept assistant message stays OpenAI-shaped: one history shape for all Go.
#[derive(Default)]
struct GoCallAccum {
    text: Vec<(bool, String)>,
    calls: Vec<Call>,
    kept_calls: Vec<Value>,
}

impl GoCallAccum {
    fn push_text(&mut self, text: &str) {
        self.text.push((false, text.to_string()));
    }

    fn full(&self) -> bool {
        self.calls.len() >= GO_MAX_TOOL_CALLS
    }

    fn push_call(&mut self, id: String, name: String, arguments: Result<Value, String>, raw_args: String) {
        self.calls.push(Call { id: id.clone(), name: name.clone(), arguments: arguments.clone() });
        self.kept_calls.push(json!({"id": id, "type": "function", "function": {"name": name, "arguments": raw_args}}));
    }

    fn into_reply(self) -> Reply {
        go_reply_from_parts(self.text, self.calls, self.kept_calls)
    }
}

/// A Responses API answer as the tool loop's Reply; the kept assistant message
/// is OpenAI-shaped so the loop stores one history shape for all of Go.
fn go_responses_reply(body: &Value) -> Reply {
    let mut acc = GoCallAccum::default();
    if let Some(items) = body.get("output").and_then(Value::as_array) {
        for item in items {
            match item.get("type").and_then(Value::as_str) {
                Some("message") => {
                    if let Some(parts) = item.get("content").and_then(Value::as_array) {
                        // One text entry per message, as on the Chat path: parts of a
                        // single answer join instead of emitting one event each.
                        let joined: String = parts.iter().filter_map(|p| p.get("text").and_then(Value::as_str)).collect();
                        if !joined.is_empty() {
                            acc.push_text(&joined);
                        }
                    }
                }
                Some("function_call") if !acc.full() => {
                    let id = item.get("call_id").and_then(Value::as_str).map_or_else(|| format!("call-{}", acc.calls.len()), str::to_string);
                    let name = item.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                    let raw_args = match item.get("arguments") {
                        Some(Value::String(s)) => s.clone(),
                        Some(v) => v.to_string(),
                        None => "{}".to_string(),
                    };
                    let arguments = tool_arguments(item.get("arguments"));
                    acc.push_call(id, name, arguments, raw_args);
                }
                _ => {}
            }
        }
    }
    acc.into_reply()
}

/// An Anthropic messages answer as the tool loop's Reply (history stays OpenAI-shaped).
fn go_messages_reply(body: &Value) -> Reply {
    let mut acc = GoCallAccum::default();
    if let Some(blocks) = body.get("content").and_then(Value::as_array) {
        for b in blocks {
            match b.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(t) = b.get("text").and_then(Value::as_str).filter(|t| !t.trim().is_empty()) {
                        acc.push_text(t);
                    }
                }
                Some("tool_use") if !acc.full() => {
                    let id = b.get("id").and_then(Value::as_str).map_or_else(|| format!("toolu-{}", acc.calls.len()), str::to_string);
                    let name = b.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                    let raw_args = b.get("input").map(|i| i.to_string()).unwrap_or_else(|| "{}".to_string());
                    let arguments = tool_arguments(b.get("input"));
                    acc.push_call(id, name, arguments, raw_args);
                }
                _ => {}
            }
        }
    }
    acc.into_reply()
}

/// A Gemini answer as the tool loop's Reply (history stays OpenAI-shaped).
fn go_gemini_reply(body: &Value) -> Reply {
    let mut acc = GoCallAccum::default();
    if let Some(parts) = body.pointer("/candidates/0/content/parts").and_then(Value::as_array) {
        for p in parts.iter() {
            if let Some(t) = p.get("text").and_then(Value::as_str).filter(|t| !t.is_empty()) {
                acc.push_text(t);
            } else if let Some(call) = p.get("functionCall") {
                if acc.full() {
                    continue;
                }
                let name = call.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                let raw_args = call.get("args").map(|a| a.to_string()).unwrap_or_else(|| "{}".to_string());
                let arguments = tool_arguments(call.get("args"));
                // Unique across rounds: `go_call_names` maps the whole history, so a
                // repeated `call-0` would answer an old result with the newest name.
                let id = format!("go-{}", GO_GEMINI_CALL_COUNTER.fetch_add(1, Ordering::Relaxed));
                acc.push_call(id, name, arguments, raw_args);
            }
        }
    }
    acc.into_reply()
}

/// A Gemini answer's thought signatures, in function-call part order, to replay
/// on the next request (Gemini 3 requires them for tool rounds).
fn go_gemini_signatures(body: &Value) -> Vec<Option<String>> {
    body.pointer("/candidates/0/content/parts")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter(|p| p.get("functionCall").is_some())
                .map(|p| p.get("thoughtSignature").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

pub struct ApiConfig {
    pub name: String,
    pub protocol: Protocol,
    pub base_url: String,
    pub model: String,
    pub key: Option<String>,
    pub http: HttpFn,
    /// How long one model request may take, in seconds (the endpoint's setting).
    pub request_timeout_s: u64,
}

enum Cmd {
    Prompt(Vec<PromptPart>),
    NewChat,
    SetModel(String),
    SetRequestTimeout(u64),
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
        // One stable session ID per chat for OpenCode Go's routing and prompt cache.
        let session_id = new_session_id();
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
                tool_names: Vec::new(),
                history: Vec::new(),
                session_id,
                gemini_sigs: Default::default(),
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
    fn set_request_timeout(&mut self, secs: u64) {
        if let Some(c) = &self.cmds {
            let _ = c.send(Cmd::SetRequestTimeout(secs));
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
    /// The MCP tools in the endpoint's own shape (function tools, or `input_schema` tools).
    tools: Vec<Value>,
    /// The tools' names, whichever shape `tools` has.
    tool_names: Vec<String>,
    /// The conversation in the endpoint's shape, without the system note (sent separately).
    history: Vec<Value>,
    /// OpenCode Go's stable session ID for this chat (one per chat, not per request).
    session_id: String,
    /// Thought signatures of Gemini function-call parts, by kept call id, for the
    /// next tool request. Stays outside the history so other protocols never see it.
    gemini_sigs: std::cell::RefCell<std::collections::HashMap<String, String>>,
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
        self.emit(Event::Ready { agent: format!("{} · {}", self.cfg.name, self.cfg.model), images: true });
        // The endpoint's models for the picker; an endpoint that does not list them shows none.
        if let Ok(list) = self.model_list() {
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
                Cmd::SetRequestTimeout(secs) => self.cfg.request_timeout_s = secs,
                Cmd::NewChat => {
                    self.history.clear();
                    // A new conversation gets a fresh Go session ID for routing and caching.
                    self.session_id = new_session_id();
                    self.gemini_sigs.borrow_mut().clear();
                    self.cancel.store(false, Ordering::Relaxed);
                    self.emit(Event::Ready { agent: format!("{} · {}", self.cfg.name, self.cfg.model), images: true });
                }
            }
        }
        if let Some(mut m) = self.mcp.take() {
            m.shutdown(false);
        }
    }

    /// The endpoint's models, in the shape its protocol lists them.
    fn model_list(&self) -> Result<Vec<ModelInfo>, String> {
        match self.cfg.protocol {
            Protocol::OpenAi => model_list(&self.cfg.http, &self.cfg.base_url, self.cfg.key.as_deref()),
            Protocol::Anthropic => anthropic_model_list(&self.cfg.http, &self.cfg.base_url, self.cfg.key.as_deref()),
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
        self.tools = match self.cfg.protocol {
            Protocol::OpenAi => openai_tools(&list),
            Protocol::Anthropic => anthropic_tools(&list),
        };
        self.tool_names = tool_names(&list);
        Ok(())
    }

    fn turn(&mut self, parts: Vec<PromptPart>) {
        self.history.push(user_message(self.cfg.protocol, &parts));
        bound_history(&mut self.history, self.cfg.protocol);
        // No cap on requests: Stop sets `cancel`, checked at the top of every round.
        loop {
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
            for (thought, text) in reply.text {
                self.emit(Event::Text { thought, text });
            }
            self.history.push(reply.assistant);
            if reply.calls.is_empty() {
                self.emit(Event::TurnEnded { reason: String::new() });
                return;
            }
            // Anthropic takes every result of a round in one user message; OpenAI takes one
            // `tool` message per call.
            let mut results = Vec::new();
            for c in reply.calls.iter().take(32) {
                let args = c.arguments.clone().unwrap_or_else(|_| json!({}));
                let title = tool_title(&c.name, &args);
                let tool_id = format!("{}-{}", c.id, self.next);
                self.emit(Event::Tool {
                    id: tool_id.clone(),
                    title: Some(title),
                    kind: Some(String::new()),
                    status: Some(ToolStatus::InProgress),
                    input: c.arguments.as_ref().ok().filter(|a| a.is_object()).cloned(),
                    output: None,
                });
                let (text, failed) = if self.cancel.load(Ordering::Relaxed) {
                    ("cancelled by the user".to_string(), true)
                } else if let Err(e) = &c.arguments {
                    // A call whose arguments do not parse is answered, never run with `{}`.
                    (format!("Not run: {e}"), true)
                } else if !self.tool_names.iter().any(|n| n == &c.name) {
                    (format!("unknown tool `{}`", c.name), true)
                } else {
                    match self.mcp_call("tools/call", json!({"name": c.name, "arguments": args})) {
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
                    input: None,
                    output: Some(shown),
                });
                match self.cfg.protocol {
                    Protocol::OpenAi => self.history.push(json!({"role": "tool", "tool_call_id": c.id, "content": text})),
                    Protocol::Anthropic => {
                        results.push(json!({"type": "tool_result", "tool_use_id": c.id, "content": text, "is_error": failed}));
                    }
                }
            }
            if self.cfg.protocol == Protocol::Anthropic && !results.is_empty() {
                self.history.push(json!({"role": "user", "content": results}));
            }
        }
    }

    fn request(&self) -> Result<Reply, String> {
        // OpenCode Go serves one protocol per model family behind a single base URL.
        if self.cfg.protocol == Protocol::OpenAi && is_opencode_go(&self.cfg.base_url) {
            return self.go_request();
        }
        let mut body = self.body();
        let (status, v) = self.post(&body)?;
        // Some OpenAI models refuse function tools on this endpoint unless the reasoning effort is
        // "none" (HTTP 400 naming `reasoning_effort`): ask again once with it, and keep it for the
        // next requests. Anthropic has no such parameter.
        let refused = self.cfg.protocol == Protocol::OpenAi
            && status == 400
            && !self.tools.is_empty()
            && !self.no_reasoning.get()
            && http_error(status, &v).contains("reasoning_effort");
        if refused {
            self.no_reasoning.set(true);
            body["reasoning_effort"] = json!("none");
            let (status, v) = self.post(&body)?;
            return if status == 200 { Ok(reply(self.cfg.protocol, &v)) } else { Err(http_error(status, &v)) };
        }
        if status != 200 {
            return Err(http_error(status, &v));
        }
        Ok(reply(self.cfg.protocol, &v))
    }

    /// One Go model on its own protocol, falling through to the next protocol when
    /// the gateway names a mismatch (or a per-path credential refusal). A model the
    /// map does not know starts on chat completions, so new models keep working.
    fn go_request(&self) -> Result<Reply, String> {
        let mapped = go_protocol(&self.cfg.model);
        let mut order = vec![mapped];
        for p in [GoProtocol::Chat, GoProtocol::Responses, GoProtocol::Messages, GoProtocol::Gemini] {
            if p != mapped {
                order.push(p);
            }
        }
        let mut first_err: Option<String> = None;
        for protocol in order {
            let mut body = self.go_body(protocol);
            if protocol == GoProtocol::Chat && !self.tools.is_empty() && self.no_reasoning.get() {
                body["reasoning_effort"] = json!("none");
            }
            let (status, v) = self.go_post(protocol, &body)?;
            if protocol == GoProtocol::Chat {
                // As on plain OpenAI endpoints: ask again once with reasoning effort
                // "none" when the refusal names it.
                let refused =
                    status == 400 && !self.tools.is_empty() && !self.no_reasoning.get() && http_error(status, &v).contains("reasoning_effort");
                if refused {
                    self.no_reasoning.set(true);
                    body["reasoning_effort"] = json!("none");
                    let (status, v) = self.go_post(protocol, &body)?;
                    if status == 200 {
                        return Ok(reply(self.cfg.protocol, &v));
                    }
                    let e = http_error(status, &v);
                    if first_err.is_none() {
                        first_err = Some(e.clone());
                    }
                    if !should_try_next_go(status, &v) {
                        return Err(e);
                    }
                    continue;
                }
            }
            if status == 200 {
                if protocol == GoProtocol::Chat {
                    return Ok(reply(self.cfg.protocol, &v));
                }
                let made = Self::go_reply(protocol, &v);
                if protocol == GoProtocol::Gemini {
                    // Remember thought signatures for the next tool request in this turn.
                    let mut sigs = self.gemini_sigs.borrow_mut();
                    for (call, sig) in made.calls.iter().zip(go_gemini_signatures(&v)) {
                        if let Some(s) = sig {
                            sigs.insert(call.id.clone(), s);
                        }
                    }
                }
                return Ok(made);
            }
            let e = http_error(status, &v);
            if first_err.is_none() {
                first_err = Some(e.clone());
            }
            if !should_try_next_go(status, &v) {
                return Err(e);
            }
        }
        Err(first_err.unwrap_or_else(|| "the request ended without an answer".to_string()))
    }

    /// The `tools` value for one Go protocol, or nothing when the loop has no
    /// tools: one place instead of an `is_empty` check in every arm below.
    fn go_tools(&self, protocol: GoProtocol) -> Option<Value> {
        if self.tools.is_empty() {
            return None;
        }
        Some(match protocol {
            GoProtocol::Responses => Value::Array(go_responses_tools(&self.tools)),
            GoProtocol::Messages => Value::Array(go_anthropic_tools(&self.tools)),
            GoProtocol::Gemini => json!([{"functionDeclarations": go_gemini_tools(&self.tools)}]),
            GoProtocol::Chat => Value::Array(self.tools.clone()),
        })
    }

    /// The request body for one Go protocol from the OpenAI-shaped history. Chat
    /// reuses the endpoint body; the others convert it (each carries the system
    /// note the way its protocol takes it).
    fn go_body(&self, protocol: GoProtocol) -> Value {
        match protocol {
            GoProtocol::Chat => self.body(),
            GoProtocol::Responses => {
                let mut body = json!({"model": self.cfg.model, "input": go_responses_input(super::acp::SYSTEM_NOTE, &self.history)});
                if let Some(tools) = self.go_tools(protocol) {
                    body["tools"] = tools;
                }
                body
            }
            GoProtocol::Messages => {
                let mut body = json!({
                    "model": self.cfg.model,
                    "max_tokens": ANTHROPIC_MAX_TOKENS,
                    "system": super::acp::SYSTEM_NOTE,
                    "messages": go_anthropic_messages(&self.history),
                });
                if let Some(tools) = self.go_tools(protocol) {
                    body["tools"] = tools;
                }
                body
            }
            GoProtocol::Gemini => {
                let mut body = json!({"contents": go_gemini_contents(&self.history, &self.gemini_sigs.borrow())});
                body["systemInstruction"] = json!({"parts": [{"text": super::acp::SYSTEM_NOTE}]});
                if let Some(tools) = self.go_tools(protocol) {
                    body["tools"] = tools;
                }
                body
            }
        }
    }

    /// A non-chat Go answer as the tool loop's Reply.
    fn go_reply(protocol: GoProtocol, body: &Value) -> Reply {
        match protocol {
            GoProtocol::Chat => reply(Protocol::OpenAi, body),
            GoProtocol::Responses => go_responses_reply(body),
            GoProtocol::Messages => go_messages_reply(body),
            GoProtocol::Gemini => go_gemini_reply(body),
        }
    }

    /// The request body in the endpoint's shape: the system note is a message for OpenAI and a
    /// top-level field for Anthropic.
    fn body(&self) -> Value {
        match self.cfg.protocol {
            Protocol::OpenAi => {
                let mut messages = Vec::with_capacity(self.history.len() + 1);
                messages.push(json!({"role": "system", "content": super::acp::SYSTEM_NOTE}));
                messages.extend(self.history.iter().cloned());
                let mut body = json!({"model": self.cfg.model, "messages": messages});
                if !self.tools.is_empty() {
                    body["tools"] = Value::Array(self.tools.clone());
                    if self.no_reasoning.get() {
                        body["reasoning_effort"] = json!("none");
                    }
                }
                body
            }
            Protocol::Anthropic => {
                let mut body = json!({
                    "model": self.cfg.model,
                    "max_tokens": ANTHROPIC_MAX_TOKENS,
                    "system": super::acp::SYSTEM_NOTE,
                    "messages": &self.history,
                });
                if !self.tools.is_empty() {
                    body["tools"] = Value::Array(self.tools.clone());
                }
                body
            }
        }
    }

    fn post_to(&self, url: String, headers: Vec<(String, String)>, body: &Value) -> Result<(u16, Value), String> {
        let req = HttpRequest { method: "POST", url, headers, body: Some(body.to_string().into_bytes()), timeout_s: self.limit_s() };
        self.send(req)
    }

    fn post(&self, body: &Value) -> Result<(u16, Value), String> {
        let mut headers = vec![("Content-Type".to_string(), "application/json".to_string())];
        let url = match self.cfg.protocol {
            Protocol::OpenAi => format!("{}/chat/completions", self.cfg.base_url),
            Protocol::Anthropic => {
                headers.push(("anthropic-version".to_string(), ANTHROPIC_VERSION.to_string()));
                endpoint(&self.cfg.base_url, "messages")
            }
        };
        if let Some(k) = &self.cfg.key {
            match self.cfg.protocol {
                Protocol::OpenAi => headers.push(("Authorization".into(), format!("Bearer {k}"))),
                Protocol::Anthropic => headers.push(("x-api-key".into(), k.clone())),
            }
        }
        self.post_to(url, headers, body)
    }

    /// POST for one Go protocol attempt: its own URL, always the session header,
    /// and the Bearer key (Go speaks Bearer on every path, unlike `x-api-key`).
    fn go_post(&self, protocol: GoProtocol, body: &Value) -> Result<(u16, Value), String> {
        let mut headers = vec![("Content-Type".to_string(), "application/json".to_string())];
        headers.push(("x-opencode-session".to_string(), self.session_id.clone()));
        if protocol == GoProtocol::Messages {
            headers.push(("anthropic-version".to_string(), ANTHROPIC_VERSION.to_string()));
        }
        if let Some(k) = &self.cfg.key {
            headers.push(("Authorization".into(), format!("Bearer {k}")));
        }
        self.post_to(go_url(&self.cfg.model, protocol), headers, body)
    }

    /// The request runs on a thread of its own so that Stop does not wait for the model: a
    /// cancelled one is abandoned (the host's own timeout ends it) and its answer dropped.
    fn send(&self, req: HttpRequest) -> Result<(u16, Value), String> {
        let (tx, rx) = channel();
        let http = self.cfg.http.clone();
        let started = std::time::Instant::now();
        std::thread::spawn(move || {
            let _ = tx.send(http(&req));
        });
        // The host ends the request at the limit; a few seconds more is the backstop for one
        // that does not.
        let backstop = Duration::from_secs(self.limit_s().saturating_add(5));
        let (status, bytes) = loop {
            if self.cancel.load(Ordering::Relaxed) {
                return Err("cancelled by the user".into());
            }
            match rx.recv_timeout(CANCEL_POLL) {
                Ok(r) => break r.map_err(|e| request_error(e, started.elapsed(), self.limit_s()))?,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) if started.elapsed() < backstop => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => return Err(timed_out(self.limit_s())),
                Err(_) => return Err("the request ended without an answer".into()),
            }
        };
        Ok((status, serde_json::from_slice(&bytes).unwrap_or(Value::Null)))
    }
}

impl Worker {
    /// The endpoint's request timeout, in range.
    fn limit_s(&self) -> u64 {
        self.cfg.request_timeout_s.clamp(super::agents::MIN_REQUEST_TIMEOUT_S, super::agents::MAX_REQUEST_TIMEOUT_S)
    }
}

/// What the chat says when a request ran out of time: the limit, and where to raise it.
pub fn timed_out(limit_s: u64) -> String {
    crate::i18n::format("ui.ai_request_timed_out", &[("seconds", &limit_s.to_string())])
}

/// A failed request as people read it: one that ran into the endpoint's request timeout (the
/// host says so, or it failed at the limit) says the limit and where to raise it.
pub fn request_error(error: String, elapsed: Duration, limit_s: u64) -> String {
    if error.starts_with("timeout: global") || elapsed >= Duration::from_secs(limit_s) { timed_out(limit_s) } else { error }
}

/// An endpoint's refusal as people read it.
pub fn http_error(status: u16, body: &Value) -> String {
    let detail =
        body.pointer("/error/message").or_else(|| body.get("error")).and_then(Value::as_str).unwrap_or("").chars().take(300).collect::<String>();
    let what = match status {
        401 | 403 => crate::i18n::tr("ui.ai_http_key"),
        404 => crate::i18n::tr("ui.ai_http_not_found"),
        429 => crate::i18n::tr("ui.ai_http_busy"),
        413 => crate::i18n::tr("ui.ai_http_too_large"),
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

/// The message for what the user typed (and the view, as the endpoint's own content blocks).
fn user_message(protocol: Protocol, parts: &[PromptPart]) -> Value {
    let images = parts.iter().any(|p| matches!(p, PromptPart::Image { .. }));
    if !images {
        let text: Vec<&str> = parts.iter().filter_map(|p| if let PromptPart::Text(t) = p { Some(t.as_str()) } else { None }).collect();
        return json!({"role": "user", "content": text.join("\n")});
    }
    let content: Vec<Value> = parts
        .iter()
        .filter_map(|p| match (protocol, p) {
            (_, PromptPart::Text(t)) => Some(json!({"type": "text", "text": t})),
            (Protocol::OpenAi, PromptPart::Image { mime, base64 }) => {
                Some(json!({"type": "image_url", "image_url": {"url": format!("data:{mime};base64,{base64}")}}))
            }
            (Protocol::Anthropic, PromptPart::Image { mime, base64 }) => {
                Some(json!({"type": "image", "source": {"type": "base64", "media_type": mime, "data": base64}}))
            }
            // These endpoints take no documents (the chat doesn't send any); the message names the file.
            (_, PromptPart::Document { .. }) => None,
        })
        .collect();
    json!({"role": "user", "content": content})
}

/// Whether a history message starts a turn: a user message that is not only tool results.
fn starts_a_turn(message: &Value) -> bool {
    message["role"] == "user" && !message["content"].as_array().is_some_and(|blocks| blocks.iter().any(|b| b["type"] == "tool_result"))
}

/// One tool call the endpoint asked for.
#[derive(Debug)]
struct Call {
    id: String,
    name: String,
    /// The arguments as an object, or why they cannot be used.
    arguments: Result<Value, String>,
}

/// An answer from the endpoint, in the protocol's shape.
#[derive(Debug)]
struct Reply {
    /// Text to show: `(thought, text)` in the order it came.
    text: Vec<(bool, String)>,
    calls: Vec<Call>,
    /// The assistant message to keep in the history, in the endpoint's own shape. Tool calls are
    /// capped to the ones the loop answers, so every call kept here gets a result.
    assistant: Value,
}

/// The answer in the endpoint's shape.
fn reply(protocol: Protocol, body: &Value) -> Reply {
    match protocol {
        Protocol::OpenAi => openai_reply(body),
        Protocol::Anthropic => anthropic_reply(body),
    }
}

/// A `reasoning_details` array safe to replay: streaming-only `index` members
/// are dropped (gateways reject them), thinking content is kept.
fn sanitize_reasoning_details(details: Option<&Value>) -> Option<Value> {
    let items = details?.as_array()?;
    if items.iter().all(|d| matches!(d, Value::Object(_)) && d.get("index").is_none()) {
        return Some(Value::Array(items.clone()));
    }
    let clean: Vec<Value> = items
        .iter()
        .filter_map(|d| {
            let mut c = d.as_object()?.clone();
            c.remove("index");
            Some(Value::Object(c))
        })
        .collect();
    Some(Value::Array(clean))
}

fn openai_reply(body: &Value) -> Reply {
    let message = body.pointer("/choices/0/message").cloned().unwrap_or(Value::Null);
    let text = message.get("content").and_then(Value::as_str).filter(|t| !t.is_empty()).map(|t| vec![(false, t.to_string())]).unwrap_or_default();
    // No more than the loop answers: every call kept here must get a `tool` message.
    let raw: Vec<Value> = message.get("tool_calls").and_then(Value::as_array).map(|a| a.iter().take(32).cloned().collect()).unwrap_or_default();
    let calls = raw
        .iter()
        .enumerate()
        .map(|(i, c)| Call {
            id: c.get("id").and_then(Value::as_str).map_or_else(|| format!("call-{i}"), str::to_string),
            name: c.pointer("/function/name").and_then(Value::as_str).unwrap_or("").to_string(),
            arguments: tool_arguments(c.pointer("/function/arguments")),
        })
        .collect();
    let mut assistant = json!({"role": "assistant", "content": message.get("content").cloned().unwrap_or(Value::Null)});
    // Thinking models (Kimi K3 and kin) need their reasoning back on the next
    // tool request: replay exactly what the endpoint sent, nothing invented.
    for key in ["reasoning_content", "reasoning", "refusal"] {
        if let Some(v) = message.get(key)
            && !v.is_null()
        {
            assistant[key] = v.clone();
        }
    }
    if let Some(clean) = sanitize_reasoning_details(message.get("reasoning_details")) {
        assistant["reasoning_details"] = clean;
    }
    if !raw.is_empty() {
        assistant["tool_calls"] = Value::Array(raw);
    }
    Reply { text, calls, assistant }
}

fn anthropic_reply(body: &Value) -> Reply {
    let content = body.get("content").cloned().unwrap_or(Value::Null);
    let mut text = Vec::new();
    let mut calls = Vec::new();
    let assistant = match &content {
        Value::Array(blocks) => {
            // Keep the answer as it came (thinking blocks and signatures included), but leave out
            // tool calls beyond the loop's cap so every `tool_use` kept gets its `tool_result`.
            let mut kept = Vec::new();
            for (i, block) in blocks.iter().take(128).enumerate() {
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(t) = block.get("text").and_then(Value::as_str).filter(|t| !t.trim().is_empty()) {
                            text.push((false, t.to_string()));
                        }
                        kept.push(block.clone());
                    }
                    Some("thinking") => {
                        let thought = block.get("thinking").or_else(|| block.get("text"));
                        if let Some(t) = thought.and_then(Value::as_str).filter(|t| !t.trim().is_empty()) {
                            text.push((true, t.to_string()));
                        }
                        kept.push(block.clone());
                    }
                    Some("tool_use") if calls.len() < 32 => {
                        calls.push(Call {
                            id: block.get("id").and_then(Value::as_str).map_or_else(|| format!("toolu-{i}"), str::to_string),
                            name: block.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                            arguments: match block.get("input") {
                                None | Some(Value::Null) => Ok(json!({})),
                                Some(v @ Value::Object(_)) => Ok(v.clone()),
                                Some(_) => Err("the tool arguments must be a JSON object".into()),
                            },
                        });
                        kept.push(block.clone());
                    }
                    Some("tool_use") => {}
                    _ => kept.push(block.clone()),
                }
            }
            json!({"role": "assistant", "content": kept})
        }
        // An endpoint that answers with plain text instead of content blocks.
        Value::String(s) => {
            if !s.trim().is_empty() {
                text.push((false, s.clone()));
            }
            json!({"role": "assistant", "content": content.clone()})
        }
        _ => json!({"role": "assistant", "content": content.clone()}),
    };
    Reply { text, calls, assistant }
}

/// The history bounded before a new turn's first request: older pictures of the view swapped for
/// a note, the oldest messages dropped, and the conversation starting at a user message.
///
/// Claude signs each thinking block against everything before it, so once an earlier picture
/// or message goes, the earlier answers' thinking no longer matches and the request is refused
/// ("Invalid `signature` in `thinking` block", #9417). Those answers are from finished turns,
/// whose thinking the API leaves out of the model's context anyway: it goes too.
fn bound_history(history: &mut Vec<Value>, protocol: Protocol) {
    let mut changed = drop_old_views(history, MAX_HISTORY_IMAGES);
    while history.len() > MAX_HISTORY {
        history.remove(0);
        changed = true;
    }
    while history.first().is_some_and(|m| !starts_a_turn(m)) {
        history.remove(0);
        changed = true;
    }
    if changed && protocol == Protocol::Anthropic {
        drop_earlier_thinking(history);
    }
}

/// Takes the `thinking` and `redacted_thinking` blocks out of the assistant messages; an answer
/// that held nothing else keeps a short note, as an answer can't be empty.
fn drop_earlier_thinking(history: &mut [Value]) {
    for msg in history.iter_mut().filter(|m| m.get("role").and_then(Value::as_str) == Some("assistant")) {
        let Some(content) = msg.get_mut("content").and_then(Value::as_array_mut) else { continue };
        let before = content.len();
        content.retain(|b| !matches!(b.get("type").and_then(Value::as_str), Some("thinking" | "redacted_thinking")));
        if content.is_empty() && before > 0 {
            content.push(json!({"type": "text", "text": DROPPED_THOUGHT_NOTE}));
        }
    }
}

/// Keeps the newest `keep` pictures of the view in the history and swaps every older one for a
/// short note, so the model still knows a view was shown. Text is never touched. Pictures come
/// as OpenAI `image_url` parts or Anthropic `image` blocks, depending on the endpoint. Whether
/// any picture was swapped.
fn drop_old_views(history: &mut [Value], keep: usize) -> bool {
    let mut seen = 0;
    let mut swapped = false;
    for msg in history.iter_mut().rev() {
        let Some(content) = msg.get_mut("content").and_then(Value::as_array_mut) else { continue };
        for part in content.iter_mut().rev() {
            if !matches!(part.get("type").and_then(Value::as_str), Some("image_url" | "image")) {
                continue;
            }
            seen += 1;
            if seen > keep {
                *part = json!({"type": "text", "text": DROPPED_VIEW_NOTE});
                swapped = true;
            }
        }
    }
    swapped
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

/// MCP `tools/list` as Anthropic tools (the hidden ones left out).
pub fn anthropic_tools(list: &Value) -> Vec<Value> {
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
                    let schema = t.get("inputSchema").cloned().filter(Value::is_object).unwrap_or(json!({"type": "object", "properties": {}}));
                    Some(json!({
                        "name": name,
                        "description": t.get("description").and_then(Value::as_str).unwrap_or(""),
                        "input_schema": schema,
                    }))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The offered tools' names, whichever shape the endpoint takes.
fn tool_names(list: &Value) -> Vec<String> {
    list.get("tools")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .take(128)
                .filter_map(|t| {
                    let name = t.get("name")?.as_str()?;
                    (!HIDDEN_TOOLS.contains(&name)).then(|| name.to_string())
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `{base}/v1/{path}`, whether the user's base URL ends in `/v1` or in the endpoint root the
/// provider's documentation gives (Anthropic's `https://api.anthropic.com`, DeepSeek's
/// `https://api.deepseek.com/anthropic`).
fn endpoint(base_url: &str, path: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.to_ascii_lowercase().ends_with("/v1") { format!("{base}/{path}") } else { format!("{base}/v1/{path}") }
}

/// Headers for an Anthropic-compatible request: the version, and the key when there is one (which
/// only goes over HTTPS or to this computer).
fn anthropic_headers(base_url: &str, key: Option<&str>) -> Result<Vec<(String, String)>, String> {
    let mut headers = vec![("anthropic-version".to_string(), ANTHROPIC_VERSION.to_string())];
    if let Some(k) = key {
        if !super::agents::key_safe_for(base_url) {
            return Err(crate::i18n::tr("ui.ai_key_needs_https").to_string());
        }
        headers.push(("x-api-key".to_string(), k.to_string()));
    }
    Ok(headers)
}

/// The endpoint's models for the picker. The Messages API has no models endpoint at every
/// compatible provider (Qwen's has none), so a failure here simply leaves the picker empty.
pub fn anthropic_model_list(http: &HttpFn, base_url: &str, key: Option<&str>) -> Result<Vec<ModelInfo>, String> {
    let req = HttpRequest { method: "GET", url: endpoint(base_url, "models"), headers: anthropic_headers(base_url, key)?, body: None, timeout_s: 15 };
    let (status, bytes) = http(&req)?;
    let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if status != 200 {
        return Err(http_error(status, &v));
    }
    Ok(v.get("data")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .take(500)
                .filter_map(|m| {
                    let id = m.get("id").and_then(Value::as_str)?.to_string();
                    let name = m.get("display_name").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| id.clone());
                    Some(ModelInfo { id, name, description: String::new() })
                })
                .collect()
        })
        .unwrap_or_default())
}

/// Test an Anthropic-compatible endpoint: send the smallest message there is, so the check covers
/// the path, the key and the model even where there is no models endpoint to ask.
pub fn test_anthropic_endpoint(http: &HttpFn, base_url: &str, key: Option<&str>, model: &str) -> Result<String, String> {
    let model = model.trim();
    if model.is_empty() {
        return Err(crate::i18n::tr("ui.ai_need_model").to_string());
    }
    let mut headers = anthropic_headers(base_url, key)?;
    headers.push(("Content-Type".to_string(), "application/json".to_string()));
    let body = json!({"model": model, "max_tokens": 16, "messages": [{"role": "user", "content": "Hello"}]});
    let req = HttpRequest {
        method: "POST",
        url: endpoint(base_url, "messages"),
        headers,
        body: Some(body.to_string().into_bytes()),
        timeout_s: TEST_TIMEOUT_S,
    };
    let (status, bytes) = http(&req)?;
    let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if status != 200 {
        return Err(http_error(status, &v));
    }
    Ok(crate::i18n::format("ui.ai_test_answered", &[("model", model)]))
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
        assert_eq!(user_message(Protocol::OpenAi, &[PromptPart::Text("hi".into())])["content"], "hi");
        let m =
            user_message(Protocol::OpenAi, &[PromptPart::Text("hi".into()), PromptPart::Image { mime: "image/png".into(), base64: "AA==".into() }]);
        assert_eq!(m["content"][1]["image_url"]["url"], "data:image/png;base64,AA==");
        let a = user_message(
            Protocol::Anthropic,
            &[PromptPart::Text("hi".into()), PromptPart::Image { mime: "image/png".into(), base64: "AA==".into() }],
        );
        assert_eq!(a["content"][1]["type"], "image");
        assert_eq!(a["content"][1]["source"]["media_type"], "image/png");
        assert_eq!(a["content"][1]["source"]["data"], "AA==");
    }

    fn view(n: u8) -> Value {
        user_message(
            Protocol::OpenAi,
            &[PromptPart::Text(format!("look {n}")), PromptPart::Image { mime: "image/png".into(), base64: format!("V{n}") }],
        )
    }

    fn pictures(history: &[Value]) -> Vec<String> {
        history
            .iter()
            .filter_map(|m| m["content"].as_array())
            .flatten()
            .filter_map(|p| p.pointer("/image_url/url").and_then(Value::as_str).map(str::to_string))
            .collect()
    }

    #[test]
    fn only_the_newest_views_stay_in_the_history_and_older_ones_become_a_note() {
        let mut h = vec![json!({"role": "system", "content": "note"}), view(1), json!({"role": "assistant", "content": "ok"}), view(2)];
        drop_old_views(&mut h, 2);
        assert_eq!(pictures(&h).len(), 2, "two views fit, nothing is dropped");

        h.push(json!({"role": "user", "content": "plain text"}));
        h.push(view(3));
        drop_old_views(&mut h, 2);
        assert_eq!(pictures(&h), vec!["data:image/png;base64,V2", "data:image/png;base64,V3"]);
        // The oldest view is a note now; the words sent with it, and every other message, stay.
        assert_eq!(h[1]["content"][0]["text"], "look 1");
        assert_eq!(h[1]["content"][1], json!({"type": "text", "text": DROPPED_VIEW_NOTE}));
        assert_eq!(h[0]["content"], "note");
        assert_eq!(h[4]["content"], "plain text");
    }

    #[test]
    fn a_long_chat_with_a_view_per_message_does_not_grow_without_end() {
        let mut h = vec![json!({"role": "system", "content": "note"})];
        let big = "A".repeat(200_000);
        for _ in 0..40 {
            h.push(user_message(
                Protocol::OpenAi,
                &[PromptPart::Text("again".into()), PromptPart::Image { mime: "image/png".into(), base64: big.clone() }],
            ));
            drop_old_views(&mut h, MAX_HISTORY_IMAGES);
            h.push(json!({"role": "assistant", "content": "done"}));
        }
        let size = json!({"messages": h}).to_string().len();
        assert_eq!(pictures(&h).len(), MAX_HISTORY_IMAGES);
        assert!(size < (MAX_HISTORY_IMAGES + 1) * big.len(), "request body is {size} bytes");
    }

    /// #9417: Claude signs each thinking block against everything before it, so once an older
    /// view becomes a note, the earlier answers' thinking goes; the current answer's (a tool
    /// round still running) is never touched, as a turn's rounds don't bound the history again.
    /// Untouched history keeps its thinking, and OpenAI's history is not read for it at all.
    #[test]
    fn a_dropped_view_takes_the_earlier_answers_thinking_with_it() {
        let view = |n: u8| {
            user_message(
                Protocol::Anthropic,
                &[PromptPart::Text(format!("look {n}")), PromptPart::Image { mime: "image/png".into(), base64: format!("V{n}") }],
            )
        };
        let answer = |n: u8| {
            json!({"role": "assistant", "content": [
                {"type": "thinking", "thinking": format!("thought {n}"), "signature": format!("s{n}")},
                {"type": "redacted_thinking", "data": "r"},
                {"type": "text", "text": format!("answer {n}")},
            ]})
        };
        let thoughts = |h: &[Value]| {
            h.iter()
                .filter_map(|m| m["content"].as_array())
                .flatten()
                .filter(|b| matches!(b["type"].as_str(), Some("thinking" | "redacted_thinking")))
                .count()
        };
        // Two views fit: nothing changes, the thinking stays as signed.
        let mut h = vec![view(1), answer(1), view(2)];
        bound_history(&mut h, Protocol::Anthropic);
        assert_eq!(thoughts(&h), 2);
        // A third view swaps the first for a note: the answers before it lose their thinking.
        h.push(answer(2));
        h.push(view(3));
        bound_history(&mut h, Protocol::Anthropic);
        assert_eq!(thoughts(&h), 0, "{h:?}");
        assert_eq!(h[1]["content"], json!([{"type": "text", "text": "answer 1"}]));
        assert_eq!(h[3]["content"], json!([{"type": "text", "text": "answer 2"}]));
        // An answer that was only thinking keeps a note: an answer can't be empty.
        let mut only =
            vec![view(1), json!({"role": "assistant", "content": [{"type": "thinking", "thinking": "t", "signature": "s"}]}), view(2), view(3)];
        bound_history(&mut only, Protocol::Anthropic);
        assert_eq!(only[1]["content"], json!([{"type": "text", "text": DROPPED_THOUGHT_NOTE}]));
        // The oldest messages trimmed away count as a change too.
        let mut long: Vec<Value> =
            (0..MAX_HISTORY + 2).map(|i| if i % 2 == 0 { json!({"role": "user", "content": "go"}) } else { answer(1) }).collect();
        bound_history(&mut long, Protocol::Anthropic);
        assert!(long.len() <= MAX_HISTORY && thoughts(&long) == 0);
        // OpenAI keeps its answers as they are.
        let mut openai = vec![view(1), answer(1), view(2), view(3)];
        bound_history(&mut openai, Protocol::OpenAi);
        assert_eq!(thoughts(&openai), 2);
    }

    #[test]
    fn anthropic_views_are_dropped_the_same_way() {
        let view = |n: u8| {
            user_message(
                Protocol::Anthropic,
                &[PromptPart::Text(format!("look {n}")), PromptPart::Image { mime: "image/png".into(), base64: format!("V{n}") }],
            )
        };
        let mut h = vec![view(1), json!({"role": "assistant", "content": "ok"}), view(2), view(3)];
        drop_old_views(&mut h, 2);
        let kept: Vec<&str> =
            h.iter().filter_map(|m| m["content"].as_array()).flatten().filter_map(|p| p.pointer("/source/data").and_then(Value::as_str)).collect();
        assert_eq!(kept, vec!["V2", "V3"]);
        assert_eq!(h[0]["content"][0]["text"], "look 1");
        assert_eq!(h[0]["content"][1], json!({"type": "text", "text": DROPPED_VIEW_NOTE}));
    }

    #[test]
    fn a_too_large_request_says_so() {
        let e = http_error(413, &json!({"error": {"message": "Request body is too large."}}));
        assert!(e.starts_with("HTTP 413: "), "{e}");
        assert!(e.ends_with("(Request body is too large.)"), "{e}");
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
        let cfg = ApiConfig {
            name: "Test".into(),
            protocol: Protocol::OpenAi,
            base_url: "http://localhost:1/v1".into(),
            model: "m".into(),
            key: None,
            http,
            request_timeout_s: 300,
        };
        let ctx = StartContext { cwd: std::env::temp_dir(), bridge: None, tools: None, wake: Box::new(|| {}) };
        let mut b = ApiBackend::start(cfg, ctx).unwrap();
        b.prompt(&[PromptPart::Text("measure it".into())]);
        let events = wait_for_turn(&mut b);
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
        let cfg = ApiConfig {
            name: "Test".into(),
            protocol: Protocol::OpenAi,
            base_url: "http://localhost:1/v1".into(),
            model: "m".into(),
            key: None,
            http,
            request_timeout_s: 300,
        };
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
        wait_for_seen(&seen);
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
        let cfg = ApiConfig {
            name: "Test".into(),
            protocol: Protocol::OpenAi,
            base_url: "http://localhost:1/v1".into(),
            model: "m".into(),
            key: None,
            http,
            request_timeout_s: 300,
        };
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
        let cfg = ApiConfig {
            name: "Test".into(),
            protocol: Protocol::OpenAi,
            base_url: "http://localhost:1/v1".into(),
            model: "local".into(),
            key: None,
            http,
            request_timeout_s: 300,
        };
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
        wait_for_seen(&seen);
        assert_eq!(seen.lock().unwrap()[0]["model"], "big");
        b.shutdown(true);
    }

    /// Go models route by family; the preset's base stays one URL for all of them.
    /// Covers every model the public list returned, so each picks its protocol
    /// at once instead of burning fallback requests on the first turn.
    #[test]
    fn go_models_route_to_their_protocol() {
        for model in [
            "deepseek-v4.1-flash",
            "deepseek-v4-pro",
            "glm-5.1",
            "glm-5.2",
            "glm-5.3",
            "glm-5.3-flash",
            "hy3",
            "hy4-preview",
            "kimi-k2.6",
            "kimi-k2.7-code",
            "kimi-k3",
            "kimi-k3.1-pro",
            "longcat-2.0",
            "mimo-v2.5",
            "mimo-v2.5-pro",
            "mimo-v2.6-flash",
            "mimo-v2.6-pro",
            // minimax-m3.1-flash-free is chat/completions per the Go docs, not messages.
            "minimax-m3.1-flash",
            "minimax-m3.1-flash-free",
            "omen-alpha",
            "step-5-preview",
            "beam-free",
            "something-new",
        ] {
            assert_eq!(go_protocol(model), GoProtocol::Chat, "{model}");
        }
        for model in ["grok-4.6", "grok-4.7", "gpt-6-luna", "gpt-5.6-luna", "muse-spark-1.3-contributor", "muse-spark-1.2-contributor"] {
            assert_eq!(go_protocol(model), GoProtocol::Responses, "{model}");
        }
        for model in [
            "claude-haiku-5-5",
            "minimax-m2.5",
            "minimax-m2.7",
            "minimax-m3",
            "qwen3.6-plus",
            "qwen3.7-max",
            "qwen3.7-plus",
            "qwen3.8-max",
            "qwen3.8-flash",
        ] {
            assert_eq!(go_protocol(model), GoProtocol::Messages, "{model}");
        }
        assert_eq!(go_protocol("gemini-4-argon-contributor"), GoProtocol::Gemini);
        let gemini = go_url("gemini-4-argon-contributor", GoProtocol::Gemini);
        assert!(gemini.starts_with("https://opencode.ai/inference/go/") && gemini.contains("gemini-4-argon-contributor"), "{gemini}");
        assert_eq!(go_url("muse-spark-1.3-contributor", GoProtocol::Responses), "https://opencode.ai/inference/go/openai/v1/responses");
    }

    fn go_cfg(model: &str, http: HttpFn) -> ApiConfig {
        ApiConfig {
            name: "Test".into(),
            protocol: Protocol::OpenAi,
            base_url: "https://opencode.ai/inference/go/v1".into(),
            model: model.into(),
            key: None,
            http,
            request_timeout_s: 300,
        }
    }

    fn go_ctx() -> StartContext {
        StartContext { cwd: std::env::temp_dir(), bridge: None, tools: None, wake: Box::new(|| {}) }
    }

    /// Wait until a scripted endpoint saw a request (ten seconds at most).
    fn wait_for_seen<T>(seen: &std::sync::Mutex<Vec<T>>) {
        for _ in 0..1000 {
            if !seen.lock().unwrap().is_empty() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Poll a scripted backend until its turn ends (ten seconds at most).
    fn wait_for_turn(b: &mut ApiBackend) -> Vec<Event> {
        let mut events = Vec::new();
        let start = std::time::Instant::now();
        while !events.iter().any(|e| matches!(e, Event::TurnEnded { .. })) && start.elapsed() < Duration::from_secs(10) {
            events.extend(b.poll());
            std::thread::sleep(Duration::from_millis(10));
        }
        events
    }

    /// Every Go protocol carries the session header; other endpoints see none.
    #[test]
    fn go_session_header_goes_on_every_protocol() {
        for model in ["mimo-v2.6-flash", "muse-spark-1.3-contributor", "claude-haiku-5-5", "gemini-4-argon-contributor"] {
            let seen = Arc::new(std::sync::Mutex::new(Vec::<HttpRequest>::new()));
            let log = seen.clone();
            let http: HttpFn = Arc::new(move |r: &HttpRequest| {
                if r.method == "GET" {
                    return Ok((200, b"{\"data\":[]}".to_vec()));
                }
                log.lock().unwrap().push(r.clone());
                Ok((200, json!({"choices": [{"message": {"role": "assistant", "content": "ok"}}]}).to_string().into_bytes()))
            });
            let mut b = ApiBackend::start(go_cfg(model, http), go_ctx()).unwrap();
            b.prompt(&[PromptPart::Text("hi".into())]);
            wait_for_seen(&seen);
            let reqs = seen.lock().unwrap();
            assert!(!reqs.is_empty(), "{model}: no request sent");
            assert!(
                reqs.iter().all(|r| r.headers.iter().any(|(name, _)| name.eq_ignore_ascii_case("x-opencode-session"))),
                "{model}: every request needs x-opencode-session"
            );
            // Go speaks Bearer on every path, and Messages takes the version header too.
            assert!(reqs.iter().all(|r| r.headers.iter().any(|(n, v)| n == "Content-Type" && v == "application/json")));
            drop(reqs);
            b.shutdown(true);
        }
        // A non-Go endpoint with a Go-looking model name still sends no session header.
        let seen = Arc::new(std::sync::Mutex::new(Vec::<HttpRequest>::new()));
        let log = seen.clone();
        let http: HttpFn = Arc::new(move |r: &HttpRequest| {
            if r.method == "GET" {
                return Ok((200, b"{\"data\":[]}".to_vec()));
            }
            log.lock().unwrap().push(r.clone());
            Ok((200, json!({"choices": [{"message": {"role": "assistant", "content": "ok"}}]}).to_string().into_bytes()))
        });
        let cfg = ApiConfig {
            name: "Test".into(),
            protocol: Protocol::OpenAi,
            base_url: "https://api.openai.com/v1".into(),
            model: "muse-spark-1.3-contributor".into(),
            key: None,
            http,
            request_timeout_s: 300,
        };
        let mut b = ApiBackend::start(cfg, go_ctx()).unwrap();
        b.prompt(&[PromptPart::Text("hi".into())]);
        wait_for_seen(&seen);
        let reqs = seen.lock().unwrap();
        assert!(reqs.iter().all(|r| r.headers.iter().all(|(name, _)| !name.eq_ignore_ascii_case("x-opencode-session"))));
        drop(reqs);
        b.shutdown(true);
    }

    /// The session ID is stable within a chat and fresh after New Chat.
    #[test]
    fn go_session_is_stable_within_a_chat() {
        let sessions = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let log = sessions.clone();
        let http: HttpFn = Arc::new(move |r: &HttpRequest| {
            if r.method == "GET" {
                return Ok((200, b"{\"data\":[]}".to_vec()));
            }
            let id = r.headers.iter().find(|(name, _)| name.eq_ignore_ascii_case("x-opencode-session")).map(|(_, v)| v.clone()).unwrap_or_default();
            log.lock().unwrap().push(id);
            Ok((200, json!({"choices": [{"message": {"role": "assistant", "content": "ok"}}]}).to_string().into_bytes()))
        });
        let mut b = ApiBackend::start(go_cfg("mimo-v2.6-flash", http), go_ctx()).unwrap();
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
        run(&mut b, &|b| b.prompt(&[PromptPart::Text("still one".into())]), 1);
        run(&mut b, &|b| b.new_session(), 1);
        run(&mut b, &|b| b.prompt(&[PromptPart::Text("two".into())]), 1);
        let ids = sessions.lock().unwrap();
        assert_eq!(ids.len(), 3);
        assert!(!ids[0].is_empty(), "every model request needs x-opencode-session");
        assert_eq!(ids[0], ids[1], "same conversation keeps its session ID");
        assert_ne!(ids[1], ids[2], "New Chat gets a fresh session ID");
        drop(ids);
        b.shutdown(true);
    }

    /// Non-chat Go answers parse into the loop's Reply with OpenAI-shaped history.
    #[test]
    fn go_replies_parse_into_openai_shaped_history() {
        let r = go_responses_reply(&json!({"output": [
            {"type": "reasoning", "summary": []},
            {"type": "message", "content": [{"type": "output_text", "text": "Mea"}, {"type": "output_text", "text": "suring."}]},
            {"type": "function_call", "call_id": "c1", "name": "measure", "arguments": "{}"},
        ]}));
        assert_eq!(r.text, [(false, "Measuring.".to_string())]);
        assert_eq!(r.calls.len(), 1);
        assert_eq!(r.calls[0].name, "measure");
        assert_eq!(r.assistant["tool_calls"][0]["id"], "c1");
        let m = go_messages_reply(&json!({"content": [
            {"type": "text", "text": "Hi."},
            {"type": "tool_use", "id": "t1", "name": "batch", "input": {"commands": []}},
        ]}));
        assert_eq!(m.text, [(false, "Hi.".to_string())]);
        assert_eq!(m.calls[0].id, "t1");
        assert_eq!(m.calls[0].arguments.as_ref().unwrap(), &json!({"commands": []}));
        assert_eq!(m.assistant["tool_calls"][0]["function"]["arguments"], "{\"commands\":[]}");
        let g = go_gemini_reply(&json!({"candidates": [{"content": {"parts": [
            {"text": "Done."},
            {"functionCall": {"name": "measure", "args": {}}},
        ]}}]}));
        assert_eq!(g.text, [(false, "Done.".to_string())]);
        assert_eq!(g.calls[0].name, "measure");
        assert_eq!(g.assistant["role"], "assistant");
        // Empty answers end the turn instead of breaking it.
        for (protocol, v) in [
            (GoProtocol::Responses, json!({"output": []})),
            (GoProtocol::Messages, json!({"content": []})),
            (GoProtocol::Gemini, json!({"candidates": []})),
        ] {
            let n = Worker::go_reply(protocol, &v);
            assert!(n.text.is_empty() && n.calls.is_empty(), "{v}");
            assert!(n.assistant.get("tool_calls").is_none(), "{v}");
        }
    }

    /// Gemini's thought signature survives the round trip: the next request
    /// replays exactly the signature the answer carried (Gemini 3 rejects the
    /// tool round without it).
    #[test]
    fn go_gemini_signatures_replay_on_the_next_request() {
        let answer = json!({"candidates": [{"content": {"parts": [
            {"functionCall": {"name": "measure", "args": {}}, "thoughtSignature": "sig-1"},
        ]}}]});
        let reply = go_gemini_reply(&answer);
        assert_eq!(reply.calls.len(), 1);
        let mut sigs = std::collections::HashMap::new();
        for (call, sig) in reply.calls.iter().zip(go_gemini_signatures(&answer)) {
            if let Some(s) = sig {
                sigs.insert(call.id.clone(), s);
            }
        }
        let contents = go_gemini_contents(&[reply.assistant], &sigs);
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0]["role"], "model");
        assert_eq!(contents[0]["parts"][0]["functionCall"]["name"], "measure");
        assert_eq!(contents[0]["parts"][0]["thoughtSignature"], "sig-1");
    }

    /// Two Gemini tool rounds never share a call id, so an old tool result
    /// keeps the name of the function it answered.
    #[test]
    fn go_gemini_call_ids_are_unique_across_rounds() {
        let first = go_gemini_reply(&json!({"candidates": [{"content": {"parts": [
            {"functionCall": {"name": "measure", "args": {}}},
        ]}}]}));
        let second = go_gemini_reply(&json!({"candidates": [{"content": {"parts": [
            {"functionCall": {"name": "batch", "args": {}}},
        ]}}]}));
        assert_ne!(first.calls[0].id, second.calls[0].id, "reused ids answer old results with the newest name");
        let history = vec![
            first.assistant.clone(),
            json!({"role": "tool", "tool_call_id": first.calls[0].id, "content": "1mm"}),
            second.assistant.clone(),
            json!({"role": "tool", "tool_call_id": second.calls[0].id, "content": "ok"}),
        ];
        let contents = go_gemini_contents(&history, &std::collections::HashMap::new());
        assert_eq!(contents[1]["parts"][0]["functionResponse"]["name"], "measure");
        assert_eq!(contents[3]["parts"][0]["functionResponse"]["name"], "batch");
    }

    /// A Chat answer keeps the thinking fields the endpoint sent, so the next
    /// tool request replays them (Kimi K3 needs its reasoning back); only the
    /// streaming-only `index` members are dropped.
    #[test]
    fn openai_replies_keep_reasoning_fields_for_replay() {
        let r = openai_reply(&json!({"choices": [{"message": {
            "role": "assistant",
            "content": "Measuring.",
            "reasoning_content": "pick the tool",
            "reasoning_details": [{"type": "reasoning.text", "text": "pick", "index": 0}],
            "refusal": null,
            "tool_calls": [{"id": "c1", "type": "function", "function": {"name": "measure", "arguments": "{}"}}],
        }}]}));
        assert_eq!(r.assistant["reasoning_content"], "pick the tool");
        assert_eq!(r.assistant["reasoning_details"][0]["text"], "pick");
        assert!(r.assistant["reasoning_details"][0].get("index").is_none());
        assert!(r.assistant.get("refusal").is_none(), "null fields stay out");
        assert_eq!(r.calls.len(), 1);
    }

    /// The model list and connection test are plain GETs: no session header.
    /// Conversation (POST) requests carry it instead (see the session tests).
    #[test]
    fn go_model_list_sends_no_session_header() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::<HttpRequest>::new()));
        let log = seen.clone();
        let http: HttpFn = Arc::new(move |r: &HttpRequest| {
            log.lock().unwrap().push(r.clone());
            Ok((200, b"{\"data\":[{\"id\":\"kimi-k3\"}]}".to_vec()))
        });
        let listed = model_list(&http, "https://opencode.ai/inference/go/v1", None).unwrap();
        assert_eq!(listed.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["kimi-k3"]);
        let reqs = seen.lock().unwrap();
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].method, "GET");
        assert_eq!(reqs[0].url, "https://opencode.ai/inference/go/v1/models");
        assert!(reqs[0].headers.iter().all(|(name, _)| !name.eq_ignore_ascii_case("x-opencode-session")));
        drop(reqs);
    }

    /// OpenAI-shaped history converts per Go protocol: system handling, merged
    /// tool answers, output_text for Responses, id-to-name for Gemini.
    #[test]
    fn go_history_converts_for_each_protocol() {
        let history = vec![
            json!({"role": "user", "content": "measure it"}),
            json!({"role": "assistant", "content": "On it.", "tool_calls": [
                {"id": "c1", "type": "function", "function": {"name": "measure", "arguments": "{}"}},
                {"id": "c2", "type": "function", "function": {"name": "batch", "arguments": "{\"commands\":[]}"}},
            ]}),
            json!({"role": "tool", "tool_call_id": "c1", "content": "1mm"}),
            json!({"role": "tool", "tool_call_id": "c2", "content": "ok"}),
        ];
        let messages = go_anthropic_messages(&history);
        let roles: Vec<&str> = messages.iter().map(|m| m["role"].as_str().unwrap_or("")).collect();
        assert_eq!(roles, ["user", "assistant", "user"], "{messages:?}");
        assert_eq!(messages[2]["content"].as_array().unwrap().len(), 2, "both tool answers in one message");
        assert_eq!(messages[1]["content"][1]["type"], "tool_use");
        let input = go_responses_input("sys", &history);
        assert_eq!(input[0]["type"], "message");
        assert!(input.iter().any(|i| i["type"] == "function_call"), "{input:?}");
        assert!(input.iter().any(|i| i["type"] == "function_call_output" && i["output"] == "1mm"), "{input:?}");
        assert!(
            input.iter().any(|i| i["role"] == "assistant"
                && i.get("content").and_then(Value::as_array).is_some_and(|c| c.iter().any(|p| p["type"] == "output_text"))),
            "{input:?}"
        );
        let contents = go_gemini_contents(&history, &std::collections::HashMap::new());
        let groles: Vec<&str> = contents.iter().map(|m| m["role"].as_str().unwrap_or("")).collect();
        assert_eq!(groles, ["user", "model", "user"], "{contents:?}");
        assert_eq!(contents[2]["parts"][0]["functionResponse"]["name"], "measure");
        // Each Go protocol posts its own URL and body shape.
        for (model, want_url) in [
            ("muse-spark-1.3-contributor", "https://opencode.ai/inference/go/openai/v1/responses"),
            ("claude-haiku-5-5", "https://opencode.ai/inference/go/anthropic/v1/messages"),
            ("gemini-4-argon-contributor", "https://opencode.ai/inference/go/google/v1beta/models/gemini-4-argon-contributor:generateContent"),
            ("mimo-v2.6-flash", "https://opencode.ai/inference/go/openai/v1/chat/completions"),
        ] {
            let seen = Arc::new(std::sync::Mutex::new(Vec::<HttpRequest>::new()));
            let log = seen.clone();
            let http: HttpFn = Arc::new(move |r: &HttpRequest| {
                if r.method == "GET" {
                    return Ok((200, b"{\"data\":[]}".to_vec()));
                }
                log.lock().unwrap().push(r.clone());
                Ok((200, json!({"choices": [{"message": {"role": "assistant", "content": "ok"}}]}).to_string().into_bytes()))
            });
            let mut b = ApiBackend::start(go_cfg(model, http), go_ctx()).unwrap();
            b.prompt(&[PromptPart::Text("hi".into())]);
            wait_for_seen(&seen);
            let reqs = seen.lock().unwrap();
            assert_eq!(reqs[0].url, want_url, "{model}");
            let body: Value = serde_json::from_slice(reqs[0].body.as_deref().unwrap_or(b"null")).unwrap();
            assert!(body.get("tools").is_none(), "{model}: no MCP tools in this session, none sent");
            drop(reqs);
            b.shutdown(true);
        }
    }

    /// A Go model the map does not know falls through protocols on the gateway's
    /// mismatch wording; other refusals stop at once, on one request.
    #[test]
    fn go_unknown_models_fall_through_protocols() {
        let urls = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let log = urls.clone();
        let http: HttpFn = Arc::new(move |r: &HttpRequest| {
            if r.method == "GET" {
                return Ok((200, b"{\"data\":[]}".to_vec()));
            }
            log.lock().unwrap().push(r.url.clone());
            if r.url.ends_with("/chat/completions") {
                return Ok((
                    400,
                    json!({"error": {"message": "Model something-new does not support this protocol. Use /responses."}}).to_string().into_bytes(),
                ));
            }
            Ok((200, json!({"output": [{"type": "message", "content": [{"type": "output_text", "text": "Hi."}]}]}).to_string().into_bytes()))
        });
        let mut b = ApiBackend::start(go_cfg("something-new", http), go_ctx()).unwrap();
        b.prompt(&[PromptPart::Text("hi".into())]);
        let events = wait_for_turn(&mut b);
        assert!(events.iter().any(|e| matches!(e, Event::Text { text, .. } if text == "Hi.")), "{events:?}");
        let tried = urls.lock().unwrap();
        assert_eq!(tried.len(), 2, "{tried:?}");
        assert!(tried[0].ends_with("/chat/completions") && tried[1].ends_with("/responses"), "{tried:?}");
        drop(tried);
        b.shutdown(true);
        // Other refusals never cascade: one request, then the error.
        let seen = Arc::new(std::sync::Mutex::new(0_usize));
        let count = seen.clone();
        let http: HttpFn = Arc::new(move |r: &HttpRequest| {
            if r.method == "GET" {
                return Ok((200, b"{\"data\":[]}".to_vec()));
            }
            *count.lock().unwrap() += 1;
            Ok((400, json!({"error": {"message": "context length exceeded"}}).to_string().into_bytes()))
        });
        let mut b = ApiBackend::start(go_cfg("mimo-v2.6-flash", http), go_ctx()).unwrap();
        b.prompt(&[PromptPart::Text("hi".into())]);
        let events = wait_for_turn(&mut b);
        assert_eq!(*seen.lock().unwrap(), 1);
        assert!(events.iter().any(|e| matches!(e, Event::Error(_))), "{events:?}");
        b.shutdown(true);
    }

    /// A Go per-path credential refusal falls through; a non-Go 401 never does.
    #[test]
    fn go_credential_refusals_fall_through() {
        let urls = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let log = urls.clone();
        let http: HttpFn = Arc::new(move |r: &HttpRequest| {
            if r.method == "GET" {
                return Ok((200, b"{\"data\":[]}".to_vec()));
            }
            log.lock().unwrap().push(r.url.clone());
            if r.url.ends_with("/messages") {
                return Ok((401, json!({"error": {"message": "Upstream request failed: Go requires a credential"}}).to_string().into_bytes()));
            }
            Ok((200, json!({"choices": [{"message": {"role": "assistant", "content": "Hi."}}]}).to_string().into_bytes()))
        });
        let mut b = ApiBackend::start(go_cfg("qwen3.8-max", http), go_ctx()).unwrap();
        b.prompt(&[PromptPart::Text("hi".into())]);
        let events = wait_for_turn(&mut b);
        assert!(events.iter().any(|e| matches!(e, Event::Text { text, .. } if text == "Hi.")), "{events:?}");
        let tried = urls.lock().unwrap();
        assert!(tried.len() >= 2 && tried[0].ends_with("/messages"), "{tried:?}");
        drop(tried);
        b.shutdown(true);
        // A non-Go 401 never cascades: one request, then the error.
        let seen = Arc::new(std::sync::Mutex::new(0_usize));
        let count = seen.clone();
        let http: HttpFn = Arc::new(move |r: &HttpRequest| {
            if r.method == "GET" {
                return Ok((200, b"{\"data\":[]}".to_vec()));
            }
            *count.lock().unwrap() += 1;
            Ok((401, json!({"error": {"message": "bad key"}}).to_string().into_bytes()))
        });
        let cfg = ApiConfig {
            name: "Test".into(),
            protocol: Protocol::OpenAi,
            base_url: "http://localhost:1/v1".into(),
            model: "m".into(),
            key: None,
            http,
            request_timeout_s: 300,
        };
        let mut b = ApiBackend::start(cfg, go_ctx()).unwrap();
        b.prompt(&[PromptPart::Text("hi".into())]);
        let events = wait_for_turn(&mut b);
        assert_eq!(*seen.lock().unwrap(), 1);
        assert!(events.iter().any(|e| matches!(e, Event::Error(_))), "{events:?}");
        b.shutdown(true);
    }

    /// Gemini takes a minimal schema subset: unknown keywords are dropped,
    /// non-object nodes become plain strings, parameters stay objects.
    #[test]
    fn go_gemini_schemas_are_reduced_to_the_supported_subset() {
        let schema = json!({
            "type": "object",
            "additionalProperties": false,
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "properties": {
                "commands": {"type": "array", "additionalProperties": true, "items": {"type": "string", "default": "x"}},
                "filter": {"description": "q"},
                "bad": [1, 2],
            },
            "required": ["commands"],
        });
        let clean = go_gemini_schema(&schema);
        assert_eq!(clean["type"], "object");
        assert!(clean.get("additionalProperties").is_none() && clean.get("$schema").is_none(), "{clean}");
        assert_eq!(clean.pointer("/properties/commands/type"), Some(&json!("array")));
        assert!(clean.pointer("/properties/commands/additionalProperties").is_none(), "{clean}");
        assert_eq!(clean.pointer("/properties/commands/items"), Some(&json!({"type": "string"})), "{clean}");
        assert_eq!(clean.pointer("/properties/filter/type"), Some(&json!("string")));
        assert_eq!(clean.pointer("/properties/bad/type"), Some(&json!("string")));
        assert_eq!(clean["required"], json!(["commands"]));
        let tools = go_gemini_tools(&[json!({"type": "function", "function": {"name": "batch", "parameters": schema}})]);
        assert!(tools[0].pointer("/parameters/additionalProperties").is_none(), "{tools:?}");
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
        let cfg = ApiConfig {
            name: "Test".into(),
            protocol: Protocol::OpenAi,
            base_url: "http://localhost:1/v1".into(),
            model: "m".into(),
            key: None,
            http,
            request_timeout_s: 300,
        };
        let ctx = StartContext { cwd: std::env::temp_dir(), bridge: None, tools: None, wake: Box::new(|| {}) };
        let mut b = ApiBackend::start(cfg, ctx).unwrap();
        b.prompt(&[PromptPart::Text("go".into())]);
        let events = wait_for_turn(&mut b);
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
        let cfg = ApiConfig {
            name: "x".into(),
            protocol: Protocol::OpenAi,
            base_url: "http://10.0.0.2/v1".into(),
            model: "m".into(),
            key: Some("k".into()),
            http,
            request_timeout_s: 300,
        };
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
        let cfg = ApiConfig {
            name: "Test".into(),
            protocol: Protocol::OpenAi,
            base_url: "http://localhost:1/v1".into(),
            model: "m".into(),
            key: Some("sk-secret".into()),
            http,
            request_timeout_s: 300,
        };
        let (events, _rx) = channel();
        let w = Worker {
            cfg,
            mcp: None,
            next: 0,
            tools: vec![json!({"type": "function", "function": {"name": "measure"}})],
            tool_names: vec!["measure".into()],
            history: vec![json!({"role": "user", "content": "hi"})],
            session_id: "test-session".to_string(),
            gemini_sigs: Default::default(),
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
        assert_eq!(w.request().unwrap().assistant, ok["choices"][0]["message"]);
        assert_eq!(w.request().unwrap().assistant, ok["choices"][0]["message"]);
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

    #[test]
    fn mcp_tools_become_anthropic_tools_without_crash_reports() {
        let list = json!({"tools": [
            {"name": "batch", "description": "Run commands", "inputSchema": {"type": "object", "properties": {"commands": {"type": "array"}}}},
            {"name": "crash_reports", "description": "x", "inputSchema": {"type": "object"}},
            {"name": "measure"},
        ]});
        let t = anthropic_tools(&list);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0]["name"], "batch");
        assert_eq!(t[0]["input_schema"]["properties"]["commands"]["type"], "array");
        assert_eq!(t[1]["input_schema"]["type"], "object");
        assert_eq!(tool_names(&list), ["batch", "measure"]);
    }

    #[test]
    fn anthropic_answers_read_text_thought_and_tool_calls() {
        let r = anthropic_reply(&json!({"content": [
            {"type": "thinking", "thinking": "checking", "signature": "s"},
            {"type": "text", "text": "Measuring."},
            {"type": "tool_use", "id": "t1", "name": "measure", "input": {"body": "b"}},
        ]}));
        assert_eq!(r.text, [(true, "checking".to_string()), (false, "Measuring.".to_string())]);
        assert_eq!(r.calls.len(), 1);
        assert_eq!(r.calls[0].id, "t1");
        assert_eq!(r.calls[0].name, "measure");
        assert_eq!(r.calls[0].arguments, Ok(json!({"body": "b"})));
        assert_eq!(r.assistant["role"], "assistant");
        assert_eq!(r.assistant["content"][2]["type"], "tool_use");
        let broken = anthropic_reply(&json!({"content": [{"type": "tool_use", "name": "measure", "input": "no"}]}));
        assert!(broken.calls[0].arguments.is_err());
        // Calls beyond the loop's cap are left out of the answer too: each kept one is answered.
        let many: Vec<Value> = (0..40).map(|i| json!({"type": "tool_use", "id": format!("t{i}"), "name": "measure", "input": {}})).collect();
        let capped = anthropic_reply(&json!({"content": many}));
        assert_eq!(capped.calls.len(), 32);
        assert_eq!(capped.assistant["content"].as_array().unwrap().len(), 32);
    }

    #[test]
    fn anthropic_urls_take_the_base_with_or_without_v1() {
        assert_eq!(endpoint("https://api.anthropic.com", "messages"), "https://api.anthropic.com/v1/messages");
        assert_eq!(endpoint("https://api.anthropic.com/v1", "messages"), "https://api.anthropic.com/v1/messages");
        assert_eq!(endpoint("https://api.anthropic.com/v1/", "messages"), "https://api.anthropic.com/v1/messages");
        assert_eq!(endpoint("https://api.deepseek.com/anthropic/", "models"), "https://api.deepseek.com/anthropic/v1/models");
        assert_eq!(
            endpoint("https://dashscope.aliyuncs.com/apps/anthropic", "messages"),
            "https://dashscope.aliyuncs.com/apps/anthropic/v1/messages"
        );
    }

    /// A scripted Anthropic endpoint: one answer with a tool call, then a plain answer. The loop
    /// runs the tool (here without SolidCraft tools: an unknown tool fails) and reports the end.
    #[test]
    fn the_anthropic_tool_loop_runs_calls_and_ends_the_turn() {
        let answers = std::sync::Mutex::new(vec![
            json!({"content": [{"type": "text", "text": "Done."}]}),
            json!({"content": [
                {"type": "text", "text": "Measuring."},
                {"type": "tool_use", "id": "t1", "name": "measure", "input": {}},
            ]}),
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
        let cfg = ApiConfig {
            name: "Test".into(),
            protocol: Protocol::Anthropic,
            base_url: "http://localhost:1".into(),
            model: "m".into(),
            key: Some("sk-secret".into()),
            http,
            request_timeout_s: 300,
        };
        let ctx = StartContext { cwd: std::env::temp_dir(), bridge: None, tools: None, wake: Box::new(|| {}) };
        let mut b = ApiBackend::start(cfg, ctx).unwrap();
        b.prompt(&[PromptPart::Text("measure it".into())]);
        let events = wait_for_turn(&mut b);
        assert_eq!(events.first(), Some(&Event::ToolsAvailable(false)));
        assert!(events.iter().any(|e| matches!(e, Event::Ready { .. })), "{events:?}");
        assert!(events.iter().any(|e| matches!(e, Event::Tool { status: Some(ToolStatus::Failed), .. })), "{events:?}");
        assert!(events.iter().any(|e| matches!(e, Event::Text { text, .. } if text == "Done.")), "{events:?}");
        assert!(matches!(events.last(), Some(Event::TurnEnded { reason }) if reason.is_empty()), "{events:?}");
        let sent = seen.lock().unwrap();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0]["system"], super::super::acp::SYSTEM_NOTE);
        assert_eq!(sent[0]["max_tokens"], ANTHROPIC_MAX_TOKENS);
        assert_eq!(sent[1]["messages"].as_array().unwrap().last().unwrap()["content"][0]["type"], "tool_result");
        assert_eq!(sent[1]["messages"].as_array().unwrap().last().unwrap()["content"][0]["tool_use_id"], "t1");
        b.shutdown(true);
    }

    #[test]
    fn the_anthropic_test_sends_one_small_message_and_checks_the_key_rule() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::<HttpRequest>::new()));
        let log = seen.clone();
        let http: HttpFn = Arc::new(move |r: &HttpRequest| {
            log.lock().unwrap().push(r.clone());
            Ok((200, json!({"content": [{"type": "text", "text": "Hi"}]}).to_string().into_bytes()))
        });
        let message = test_anthropic_endpoint(&http, "https://api.anthropic.com/v1", Some("sk-k"), "claude-opus-5-5").unwrap();
        assert!(message.contains("claude-opus-5-5"), "{message}");
        let sent = seen.lock().unwrap();
        assert_eq!(sent[0].url, "https://api.anthropic.com/v1/messages");
        assert!(sent[0].headers.iter().any(|(k, v)| k == "anthropic-version" && v == ANTHROPIC_VERSION));
        assert!(sent[0].headers.iter().any(|(k, v)| k == "x-api-key" && v == "sk-k"));
        let body: Value = serde_json::from_slice(sent[0].body.as_deref().unwrap()).unwrap();
        assert_eq!(body["max_tokens"], 16);
        assert_eq!(body["messages"][0]["content"], "Hello");
        drop(sent);
        assert!(test_anthropic_endpoint(&http, "http://10.0.0.2/anthropic", Some("k"), "m").is_err());
        assert!(test_anthropic_endpoint(&http, "https://api.anthropic.com", None, "").is_err());
    }
}
