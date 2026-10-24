//! The client side of the Agent Client Protocol (ACP, <https://agentclientprotocol.com>), version 1:
//! JSON-RPC 2.0, one message per line over the agent's stdin/stdout. This file only builds and
//! reads messages; the process lives in `process.rs` and the state in `mod.rs`, so everything
//! here is plain data and tested without an agent.
//!
//! What SolidCraft says it can do (`clientCapabilities`): no file system and no terminal (the agent
//! reaches the design through SolidCraft's MCP server instead), and terminal login, so an agent
//! that needs one describes a command for the user to run rather than asking for a key.

use serde_json::{Value, json};

/// The protocol version we speak.
pub const PROTOCOL_VERSION: u64 = 1;

/// JSON-RPC "method not found", our answer to every agent request we don't serve.
pub const METHOD_NOT_FOUND: i64 = -32601;
/// The error code ACP agents use for "authentication required".
pub const AUTH_REQUIRED: i64 = -32000;

/// One request or notification to send.
pub fn request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}
pub fn notification(method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "method": method, "params": params})
}
pub fn reply(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}
pub fn reply_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

pub fn initialize_params(version: &str) -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "clientCapabilities": {
            "fs": {"readTextFile": false, "writeTextFile": false},
            "terminal": false,
            "auth": {"terminal": true},
            "_meta": {"terminal-auth": true},
        },
        "clientInfo": {"name": "solidcraft", "title": "SolidCraft", "version": version},
    })
}

/// The name the chat gives its MCP server (#2592): not `solidcraft`, the name docs/mcp.md suggests
/// for a user's own setup, so an agent that merges its own MCP servers with the session's (Codex
/// keeps its own entry on a clash) can't shadow the bridge with a headless session.
pub const BRIDGE_NAME: &str = "solidcraft-app";

/// The MCP server the agent starts to reach the design: `solidcraft-cli mcp --connect` bridged to
/// this app's control port. The server finds the port's token in the token file the app wrote
/// (docs/control-protocol.md), so the token never travels through the agent, and it keeps its
/// default folder limits (docs/mcp.md): no `--allow-any-path`.
#[derive(Clone, Debug, PartialEq)]
pub struct McpBridge {
    pub cli: std::path::PathBuf,
    pub port: u16,
}

impl McpBridge {
    pub fn to_json(&self) -> Value {
        json!({
            "name": BRIDGE_NAME,
            "command": self.cli.to_string_lossy(),
            "args": ["mcp", "--connect", format!("127.0.0.1:{}", self.port)],
            "env": [],
        })
    }
}

/// What every session is told about where it is. Agents that don't read `_meta` ignore it.
pub const SYSTEM_NOTE: &str = "You are running inside SolidCraft, a parametric 3D CAD application, as the user's assistant. \
The open design is reachable through the `solidcraft-app` MCP server: use its tools (list_commands, batch, execute, inspect_design, \
measure, screenshot, set_parameter, undo) to read and change the design the user is looking at, and verify with measure or \
inspect_design. Prefer one `batch` over many small calls. Saving, opening or exporting files waits for the user's approval in \
the chat, and each message allows a limited number of changes. Don't use shell or file tools: work through the SolidCraft tools. \
Keep answers short.";

/// The Claude Code adapter's own tools that reach the computer rather than the design: turned off
/// for the session (`_meta.claudeCode.options.disallowedTools`), so it works through SolidCraft's.
pub const CLAUDE_DISALLOWED_TOOLS: &[&str] =
    &["Bash", "BashOutput", "KillShell", "Read", "Write", "Edit", "MultiEdit", "NotebookEdit", "Glob", "Grep", "WebFetch", "WebSearch", "Task"];

pub fn session_new_params(cwd: &std::path::Path, bridge: Option<&McpBridge>) -> Value {
    let servers: Vec<Value> = bridge.map(McpBridge::to_json).into_iter().collect();
    json!({
        "cwd": cwd.to_string_lossy(),
        "mcpServers": servers,
        "_meta": {
            "systemPrompt": {"append": SYSTEM_NOTE},
            // Hosts may opt a session out of "bypass permissions" (the Claude adapter reads it):
            // every tool call the agent asks about reaches the panel.
            // Its own shell and file tools are off: the agent works through SolidCraft's tools.
            "claudeCode": {"options": {"allowDangerouslySkipPermissions": false, "disallowedTools": CLAUDE_DISALLOWED_TOOLS}},
        },
    })
}

pub fn prompt_params(session: &str, parts: &[super::backend::PromptPart]) -> Value {
    json!({"sessionId": session, "prompt": super::backend::content_blocks(parts)})
}

/// The agent's modes in a `session/new` reply: the current one and each `(id, name)`.
pub fn modes(session: &Value) -> Option<(String, Vec<(String, String)>)> {
    let m = session.get("modes")?;
    let current = m.get("currentModeId").and_then(Value::as_str).unwrap_or("").to_string();
    let available: Vec<(String, String)> = m
        .get("availableModes")?
        .as_array()?
        .iter()
        .take(16)
        .filter_map(|x| {
            let id = x.get("id")?.as_str()?.to_string();
            let name = x.get("name").and_then(Value::as_str).unwrap_or(&id).to_string();
            Some((id, name))
        })
        .collect();
    (!available.is_empty()).then_some((current, available))
}

/// One model an agent offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub description: String,
}

/// The advertised select option that controls the session model.
pub fn model_config(session: &Value) -> Option<&Value> {
    session.get("configOptions")?.as_array()?.iter().find(|c| (c["category"] == "model" || c["id"] == "model") && c["type"] == "select")
}

/// The current model and available models, from legacy models or config options.
pub fn models(session: &Value) -> Option<(String, Vec<ModelInfo>)> {
    let (current, list, legacy) = if let Some(m) = model_config(session) {
        (m.get("currentValue"), m.get("options"), false)
    } else {
        let m = session.get("models")?;
        (m.get("currentModelId"), m.get("availableModels"), true)
    };
    let current = current.and_then(Value::as_str).unwrap_or("").to_string();
    let available = list
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|x| x.get("options").and_then(Value::as_array).map_or_else(|| std::slice::from_ref(x), Vec::as_slice))
        .take(200)
        .filter_map(|x| {
            let id = if legacy { x.get("modelId").or_else(|| x.get("id")) } else { x.get("value") }?.as_str()?.to_string();
            let name = x.get("name").and_then(Value::as_str).filter(|n| !n.is_empty()).unwrap_or(&id).to_string();
            let description = x.get("description").and_then(Value::as_str).unwrap_or("").to_string();
            Some(ModelInfo { id, name, description })
        })
        .collect();
    Some((current, available))
}

/// `session/set_model` parameters.
pub fn set_model_params(session: &str, model: &str) -> Value {
    json!({"sessionId": session, "modelId": model})
}

/// Whether the agent takes embedded documents in a message
/// (`agentCapabilities.promptCapabilities.embeddedContext`: it reads `resource` blocks).
pub fn takes_documents(init: &Value) -> bool {
    init.pointer("/agentCapabilities/promptCapabilities/embeddedContext").and_then(Value::as_bool).unwrap_or(false)
}

/// Whether the agent takes pictures in a message (`agentCapabilities.promptCapabilities.image`).
pub fn takes_images(init: &Value) -> bool {
    init.pointer("/agentCapabilities/promptCapabilities/image").and_then(Value::as_bool).unwrap_or(false)
}

/// One way to log in that the agent offers (`authMethods` in its `initialize` reply).
#[derive(Clone, Debug, PartialEq)]
pub struct AuthMethod {
    pub id: String,
    pub name: String,
    pub description: String,
    pub kind: AuthKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AuthKind {
    /// The agent signs in by itself when asked (`authenticate`), e.g. by opening a browser.
    Agent,
    /// The user runs the agent's own login in a terminal: the agent's command plus `args`.
    Terminal { args: Vec<String>, command: Option<(String, Vec<String>)> },
    /// The agent reads a key from an environment variable the user sets themselves.
    EnvVar { vars: Vec<String> },
}

pub fn auth_methods(init: &Value) -> Vec<AuthMethod> {
    let strings = |v: Option<&Value>| -> Vec<String> {
        v.and_then(Value::as_array).map(|a| a.iter().take(64).filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
    };
    init.get("authMethods")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .take(32)
                .filter_map(|m| {
                    let s = |k: &str| m.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                    let id = s("id");
                    if id.is_empty() {
                        return None;
                    }
                    let kind = match m.get("type").and_then(Value::as_str) {
                        Some("terminal") => {
                            let meta = m.get("_meta").and_then(|x| x.get("terminal-auth"));
                            let command = meta.and_then(|t| Some((t.get("command")?.as_str()?.to_string(), strings(t.get("args")))));
                            AuthKind::Terminal { args: strings(m.get("args")), command }
                        }
                        Some("env_var") => AuthKind::EnvVar {
                            vars: m
                                .get("vars")
                                .and_then(Value::as_array)
                                .map(|v| v.iter().take(16).filter_map(|x| x.get("name").and_then(Value::as_str).map(str::to_string)).collect())
                                .unwrap_or_default(),
                        },
                        // A key the agent would take from the client: SolidCraft never handles
                        // keys, so this becomes "set it in your environment" instead.
                        _ if m.get("_meta").and_then(|x| x.get("api-key")).is_some() => {
                            let provider = m
                                .get("_meta")
                                .and_then(|x| x.get("api-key"))
                                .and_then(|k| k.get("provider"))
                                .and_then(Value::as_str)
                                .unwrap_or("agent");
                            AuthKind::EnvVar { vars: vec![format!("{}_API_KEY", provider.to_ascii_uppercase())] }
                        }
                        _ => AuthKind::Agent,
                    };
                    Some(AuthMethod { id, name: s("name"), description: s("description"), kind })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The agent's name and version from its `initialize` reply.
pub fn agent_info(init: &Value) -> String {
    let i = init.get("agentInfo");
    let s = |k: &str| i.and_then(|i| i.get(k)).and_then(Value::as_str);
    match (s("title").or(s("name")), s("version")) {
        (Some(n), Some(v)) => format!("{n} {v}"),
        (Some(n), None) => n.to_string(),
        _ => String::new(),
    }
}

/// A tool call's state as the agent reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

impl ToolStatus {
    pub fn parse(s: &str) -> Option<ToolStatus> {
        Some(match s {
            "pending" => ToolStatus::Pending,
            "in_progress" => ToolStatus::InProgress,
            "completed" => ToolStatus::Completed,
            "failed" => ToolStatus::Failed,
            _ => return None,
        })
    }
}

/// One choice of a permission request.
#[derive(Clone, Debug, PartialEq)]
pub struct PermissionOption {
    pub id: String,
    pub name: String,
    /// `allow_once`, `allow_always`, `reject_once` or `reject_always`.
    pub kind: String,
}

/// What a message from the agent means for the panel.
#[derive(Clone, Debug, PartialEq)]
pub enum Incoming {
    /// The reply to one of our requests.
    Reply { id: u64, result: Result<Value, (i64, String)> },
    /// Text the agent streams (`thought`: its reasoning, shown dimmed).
    Text { thought: bool, text: String },
    /// A new tool call, or news about one (`title`/`status` when they changed).
    Tool { id: String, title: Option<String>, kind: Option<String>, status: Option<ToolStatus>, output: Option<String> },
    /// The agent's plan: each entry with its status.
    Plan(Vec<(String, String)>),
    /// The agent asks before running a tool call; answered with [`permission_reply`].
    Permission { rpc: Value, title: String, options: Vec<PermissionOption> },
    /// The agent says whether it is logged in (an extension both adapters send).
    AuthStatus(String),
    /// The agent switched to another of its modes.
    ModeChanged(String),
    /// The agent switched to another model.
    ModelChanged(String),
    /// A request we don't serve (file system, terminal, …): answer "method not found".
    Unsupported { rpc: Value, method: String },
    /// Anything else (mode and command lists, usage): nothing to show.
    Ignored,
}

/// The longest text kept from one tool call's output.
const MAX_OUTPUT: usize = 2000;

fn content_text(c: &Value) -> Option<String> {
    // A content block, or a tool call content item wrapping one (`{type: "content", content}`).
    let block = if c.get("type").and_then(Value::as_str) == Some("content") { c.get("content")? } else { c };
    match block.get("type").and_then(Value::as_str) {
        Some("text") => block.get("text").and_then(Value::as_str).map(str::to_string),
        Some("image") => Some("[image]".into()),
        Some("resource_link") => block.get("uri").and_then(Value::as_str).map(|u| format!("[{u}]")),
        Some("diff") => block.get("path").and_then(Value::as_str).map(|p| format!("[diff {p}]")),
        Some("terminal") => Some("[terminal]".into()),
        _ => None,
    }
}

fn tool_output(u: &Value) -> Option<String> {
    let parts: Vec<String> = u.get("content")?.as_array()?.iter().take(16).filter_map(content_text).collect();
    if parts.is_empty() {
        return None;
    }
    let mut s = parts.join("\n");
    if s.len() > MAX_OUTPUT {
        let mut cut = MAX_OUTPUT;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push('…');
    }
    Some(s)
}

/// Read one line from the agent.
pub fn parse(line: &str) -> Option<Incoming> {
    let msg: Value = serde_json::from_str(line.trim()).ok()?;
    let method = msg.get("method").and_then(Value::as_str);
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    match (method, msg.get("id")) {
        (None, Some(id)) => {
            let id = id.as_u64()?;
            let result = match msg.get("error") {
                Some(e) => {
                    Err((e.get("code").and_then(Value::as_i64).unwrap_or(0), e.get("message").and_then(Value::as_str).unwrap_or("error").to_string()))
                }
                None => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
            };
            Some(Incoming::Reply { id, result })
        }
        (Some("session/request_permission"), Some(id)) => {
            let call = params.get("toolCall");
            let title = call.and_then(|c| c.get("title")).and_then(Value::as_str).unwrap_or("").to_string();
            let options = params
                .get("options")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .take(8)
                        .filter_map(|o| {
                            Some(PermissionOption {
                                id: o.get("optionId")?.as_str()?.to_string(),
                                name: o.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                                kind: o.get("kind").and_then(Value::as_str).unwrap_or("").to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(Incoming::Permission { rpc: id.clone(), title, options })
        }
        (Some(m), Some(id)) => Some(Incoming::Unsupported { rpc: id.clone(), method: m.to_string() }),
        (Some("session/update"), None) => Some(update(params.get("update")?)),
        (Some("_auth/status_update"), None) => {
            params.get("authStatus").and_then(|s| s.get("label")).and_then(Value::as_str).map(|l| Incoming::AuthStatus(l.to_string()))
        }
        _ => Some(Incoming::Ignored),
    }
}

fn update(u: &Value) -> Incoming {
    let s = |k: &str| u.get(k).and_then(Value::as_str).map(str::to_string);
    match u.get("sessionUpdate").and_then(Value::as_str) {
        Some(kind @ ("agent_message_chunk" | "agent_thought_chunk")) => match u.get("content").and_then(content_text) {
            Some(text) => Incoming::Text { thought: kind == "agent_thought_chunk", text },
            None => Incoming::Ignored,
        },
        Some("tool_call" | "tool_call_update") => match s("toolCallId") {
            Some(id) => Incoming::Tool {
                id,
                title: s("title"),
                kind: s("kind"),
                status: u.get("status").and_then(Value::as_str).and_then(ToolStatus::parse),
                output: tool_output(u),
            },
            None => Incoming::Ignored,
        },
        Some("current_mode_update") => match s("currentModeId").or_else(|| s("modeId")) {
            Some(m) => Incoming::ModeChanged(m),
            None => Incoming::Ignored,
        },
        Some("current_model_update") => match s("currentModelId").or_else(|| s("modelId")) {
            Some(m) => Incoming::ModelChanged(m),
            None => Incoming::Ignored,
        },
        Some("plan") => Incoming::Plan(
            u.get("entries")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .take(64)
                        .map(|e| {
                            let g = |k: &str| e.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                            (g("content"), g("status"))
                        })
                        .collect()
                })
                .unwrap_or_default(),
        ),
        _ => Incoming::Ignored,
    }
}

/// The answer to a permission request: the option picked, or `None` for "cancelled".
pub fn permission_reply(rpc: Value, option: Option<&str>) -> Value {
    let outcome = match option {
        Some(o) => json!({"outcome": "selected", "optionId": o}),
        None => json!({"outcome": "cancelled"}),
    };
    reply(rpc, json!({"outcome": outcome}))
}

/// Split a command line into the program and its arguments: whitespace separates, double or
/// single quotes group, a backslash before a quote keeps it. No shell runs it.
pub fn split_command(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (_, '\\') if matches!(chars.peek(), Some('"') | Some('\'')) => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
                started = true;
            }
            (None, '"' | '\'') => {
                quote = Some(c);
                started = true;
            }
            (Some(q), _) if c == q => quote = None,
            (None, c) if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            _ => {
                cur.push(c);
                started = true;
            }
        }
    }
    if started {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documents_are_taken_only_when_the_agent_says_so() {
        let yes = json!({"agentCapabilities": {"promptCapabilities": {"image": true, "embeddedContext": true}}});
        let no = json!({"agentCapabilities": {"promptCapabilities": {"image": true}}});
        assert!(takes_documents(&yes) && takes_images(&yes));
        assert!(!takes_documents(&no) && !takes_documents(&json!({})));
    }

    #[test]
    fn commands_split_like_a_shell_without_one() {
        assert_eq!(split_command("gemini --acp"), ["gemini", "--acp"]);
        assert_eq!(split_command(r#"  "C:\Program Files\x\agent.exe"  --flag 'a b' "#), [r"C:\Program Files\x\agent.exe", "--flag", "a b"]);
        assert_eq!(split_command(r#"a "" b"#), ["a", "", "b"]);
        assert!(split_command("   ").is_empty());
    }

    /// #2592: the bridge is not called `solidcraft`, the name a user's own MCP entry takes from
    /// docs/mcp.md, and the note tells the agent the name it really has.
    #[test]
    fn the_bridge_has_a_name_of_its_own_and_the_note_uses_it() {
        assert_ne!(BRIDGE_NAME, "solidcraft");
        assert!(SYSTEM_NOTE.contains(&format!("`{BRIDGE_NAME}` MCP server")), "{SYSTEM_NOTE}");
        assert!(!SYSTEM_NOTE.contains("`solidcraft` MCP server"));
        // The panel still recognises the bridge's tools as SolidCraft's (agents name them after
        // their server, e.g. `mcp__solidcraft-app__batch`).
        assert!(BRIDGE_NAME.contains("solidcraft"));
    }

    #[test]
    fn the_session_gets_the_bridged_mcp_server_without_a_token_or_any_path() {
        let b = McpBridge { cli: "/opt/solidcraft/solidcraft-cli".into(), port: 41234 };
        let p = session_new_params(std::path::Path::new("/home/u/designs"), Some(&b));
        let s = &p["mcpServers"][0];
        assert_eq!(s["name"], BRIDGE_NAME);
        assert_eq!(s["args"], json!(["mcp", "--connect", "127.0.0.1:41234"]));
        let text = p.to_string();
        assert!(!text.contains("allow-any-path") && !text.contains("TOKEN") && !text.contains("token"), "{text}");
        assert!(p["_meta"]["claudeCode"]["options"]["disallowedTools"].as_array().is_some_and(|t| t.iter().any(|x| x == "Bash")));
        assert_eq!(session_new_params(std::path::Path::new("/tmp"), None)["mcpServers"], json!([]));
    }

    #[test]
    fn auth_methods_of_both_adapters_are_read() {
        // As the Claude adapter offers them to a client with terminal login.
        let claude = json!({"authMethods": [{"description": "Use Claude subscription ", "name": "Claude Subscription", "id": "claude-ai-login",
            "type": "terminal", "args": ["--cli", "auth", "login", "--claudeai"],
            "_meta": {"terminal-auth": {"command": "/usr/bin/node", "args": ["/x/dist/index.js", "--cli", "auth", "login", "--claudeai"], "label": "Claude Login"}}}]});
        let m = auth_methods(&claude);
        assert_eq!(m.len(), 1);
        let AuthKind::Terminal { args, command } = &m[0].kind else { panic!("{m:?}") };
        assert_eq!(args.last().map(String::as_str), Some("--claudeai"));
        assert_eq!(command.as_ref().map(|c| c.0.as_str()), Some("/usr/bin/node"));
        // As the Codex adapter offers them.
        let codex = json!({"authMethods": [{"id": "chatgpt", "name": "Login with ChatGPT"},
            {"type": "env_var", "id": "openai-api-key", "name": "Use OPENAI_API_KEY", "vars": [{"name": "OPENAI_API_KEY"}]}]});
        let m = auth_methods(&codex);
        assert_eq!(m[0].kind, AuthKind::Agent);
        // Codex 2.x offers its key login as a method the client would feed: shown as a variable.
        let codex2 = json!({"authMethods": [{"id": "api-key", "name": "API Key", "_meta": {"api-key": {"provider": "openai"}}}]});
        assert_eq!(auth_methods(&codex2)[0].kind, AuthKind::EnvVar { vars: vec!["OPENAI_API_KEY".into()] });
        assert_eq!(m[1].kind, AuthKind::EnvVar { vars: vec!["OPENAI_API_KEY".into()] });
    }

    #[test]
    fn updates_permissions_and_replies_are_told_apart() {
        let text = r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Hi"}}}}"#;
        assert_eq!(parse(text), Some(Incoming::Text { thought: false, text: "Hi".into() }));
        let tool = r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"tool_call_update","toolCallId":"t1","status":"completed","content":[{"type":"content","content":{"type":"text","text":"ok"}}]}}}"#;
        assert_eq!(
            parse(tool),
            Some(Incoming::Tool { id: "t1".into(), title: None, kind: None, status: Some(ToolStatus::Completed), output: Some("ok".into()) })
        );
        let perm = r#"{"jsonrpc":"2.0","id":7,"method":"session/request_permission","params":{"sessionId":"s","toolCall":{"toolCallId":"t1","title":"batch"},"options":[{"optionId":"allow","name":"Allow","kind":"allow_once"},{"optionId":"reject","name":"Reject","kind":"reject_once"}]}}"#;
        let Some(Incoming::Permission { rpc, title, options }) = parse(perm) else { panic!() };
        assert_eq!((title.as_str(), options.len(), options[1].kind.as_str()), ("batch", 2, "reject_once"));
        assert_eq!(permission_reply(rpc.clone(), Some("allow"))["result"]["outcome"], json!({"outcome": "selected", "optionId": "allow"}));
        assert_eq!(permission_reply(rpc, None)["result"]["outcome"]["outcome"], "cancelled");
        let fs = r#"{"jsonrpc":"2.0","id":8,"method":"fs/read_text_file","params":{}}"#;
        assert!(matches!(parse(fs), Some(Incoming::Unsupported { .. })));
        let err = r#"{"jsonrpc":"2.0","id":3,"error":{"code":-32000,"message":"Authentication required"}}"#;
        assert_eq!(parse(err), Some(Incoming::Reply { id: 3, result: Err((AUTH_REQUIRED, "Authentication required".into())) }));
        assert_eq!(parse("not json"), None);
        let mode = r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"current_mode_update","currentModeId":"plan"}}}"#;
        assert_eq!(parse(mode), Some(Incoming::ModeChanged("plan".into())));
        let new = json!({"sessionId": "s", "modes": {"currentModeId": "default", "availableModes": [{"id": "default", "name": "Default"}, {"id": "acceptEdits", "name": "Accept Edits"}]}});
        assert_eq!(modes(&new), Some(("default".into(), vec![("default".into(), "Default".into()), ("acceptEdits".into(), "Accept Edits".into())])));
    }

    #[test]
    fn long_tool_output_is_cut_on_a_character_boundary() {
        let long = "é".repeat(3000);
        let u = json!({"sessionUpdate": "tool_call", "toolCallId": "t", "content": [{"type": "content", "content": {"type": "text", "text": long}}]});
        let Incoming::Tool { output: Some(o), .. } = update(&u) else { panic!() };
        assert!(o.len() <= MAX_OUTPUT + 3 && o.ends_with('…'));
    }

    #[test]
    fn the_models_of_a_session_reply_are_read_like_its_modes() {
        let v = json!({"sessionId": "s", "models": {"currentModelId": "sonnet", "availableModels": [
            {"modelId": "opus", "name": "Opus", "description": "Most capable"},
            {"modelId": "sonnet", "name": "Sonnet"},
            {"modelId": "haiku"},
            {"name": "no id"}]}});
        let (current, list) = models(&v).unwrap();
        assert_eq!(current, "sonnet");
        let names: Vec<_> = list.iter().map(|m| (m.id.as_str(), m.name.as_str(), m.description.as_str())).collect();
        assert_eq!(names, [("opus", "Opus", "Most capable"), ("sonnet", "Sonnet", ""), ("haiku", "haiku", "")]);
        // Preserve a reported current model even when the available list is empty.
        assert!(models(&json!({"sessionId": "s"})).is_none());
        assert_eq!(models(&json!({"models": {"currentModelId": "x", "availableModels": []}})).unwrap().0, "x");
        assert_eq!(models(&json!({"models": {"currentModelId": "haiku"}})).unwrap().0, "haiku");
        let changed = r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"current_model_update","currentModelId":"haiku"}}}"#;
        assert_eq!(parse(changed), Some(Incoming::ModelChanged("haiku".into())));
        assert_eq!(set_model_params("s", "opus"), json!({"sessionId": "s", "modelId": "opus"}));
    }
}
