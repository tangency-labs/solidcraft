//! The client side of the Agent Client Protocol (ACP, <https://agentclientprotocol.com>), version 1:
//! JSON-RPC 2.0, one message per line over the agent's stdin/stdout. This file only builds and
//! reads messages; the process lives in `process.rs` and the state in `mod.rs`, so everything
//! here is plain data and tested without an agent.
//!
//! What SolidCraft says it can do (`clientCapabilities`): no file system and no terminal (the agent
//! reaches the design through SolidCraft's MCP server instead), and terminal login, so an agent
//! that needs one describes a command for the user to run rather than asking for a key.

use serde::Serialize;
use serde_json::{Value, json};

/// One advertised value, with an optional human-readable group label.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigChoice {
    pub value: String,
    pub name: String,
    pub description: String,
    pub group: Option<String>,
}
/// A supported ACP select option; identifiers and categories belong to the agent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigOption {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub current_value: String,
    pub options: Vec<ConfigChoice>,
}

/// Parse ACP select options as a bounded authoritative snapshot.
#[derive(Debug, PartialEq, Eq)]
pub enum ConfigParse {
    Absent,
    Invalid,
    Snapshot(Vec<ConfigOption>),
}
pub fn config_options(v: &Value) -> ConfigParse {
    let Some(raw_list) = v.get("configOptions") else {
        return ConfigParse::Absent;
    };
    let Some(list) = raw_list.as_array() else {
        return ConfigParse::Invalid;
    };
    let mut out = Vec::new();
    let mut ids = std::collections::HashSet::new();
    for x in list.iter().take(64) {
        match x.get("type").and_then(Value::as_str) {
            Some("select") => {}
            Some(_) => continue,
            None => return ConfigParse::Invalid,
        }
        let Some(id) = x.get("id").and_then(Value::as_str).filter(|s| !s.is_empty() && s.len() <= 256) else {
            return ConfigParse::Invalid;
        };
        let Some(current_value) = x.get("currentValue").and_then(Value::as_str).filter(|s| s.len() <= 256) else {
            return ConfigParse::Invalid;
        };
        let Some(raw) = x.get("options").and_then(Value::as_array) else {
            return ConfigParse::Invalid;
        };
        if !ids.insert(id.to_string()) {
            continue;
        }
        let mut choices = Vec::new();
        let mut values = std::collections::HashSet::new();
        'items: for item in raw.iter().take(128) {
            if choices.len() >= 128 {
                break;
            }
            let group =
                item.get("name").and_then(Value::as_str).or_else(|| item.get("group").and_then(Value::as_str)).map(|s| s.chars().take(256).collect());
            if let Some(cs) = item.get("options").and_then(Value::as_array) {
                for c in cs.iter().take(128) {
                    if choices.len() >= 128 {
                        break 'items;
                    }
                    let Some(value) = c.get("value").and_then(Value::as_str).filter(|s| !s.is_empty() && s.len() <= 256) else {
                        continue;
                    };
                    if !values.insert(value.to_string()) {
                        continue;
                    }
                    choices.push(ConfigChoice {
                        value: value.into(),
                        name: c.get("name").and_then(Value::as_str).unwrap_or(value).chars().take(256).collect(),
                        description: c.get("description").and_then(Value::as_str).unwrap_or("").chars().take(1024).collect(),
                        group: group.clone(),
                    });
                }
            } else {
                let Some(value) = item.get("value").and_then(Value::as_str).filter(|s| !s.is_empty() && s.len() <= 256) else {
                    continue;
                };
                if values.insert(value.to_string()) {
                    choices.push(ConfigChoice {
                        value: value.into(),
                        name: item.get("name").and_then(Value::as_str).unwrap_or(value).chars().take(256).collect(),
                        description: item.get("description").and_then(Value::as_str).unwrap_or("").chars().take(1024).collect(),
                        group: None,
                    });
                }
            }
        }
        if choices.is_empty() {
            return ConfigParse::Invalid;
        }
        out.push(ConfigOption {
            id: id.into(),
            name: x.get("name").and_then(Value::as_str).unwrap_or(id).chars().take(256).collect(),
            description: x.get("description").and_then(Value::as_str).unwrap_or("").chars().take(1024).collect(),
            category: x.get("category").and_then(Value::as_str).unwrap_or("").chars().take(128).collect(),
            current_value: current_value.into(),
            options: choices,
        });
    }
    ConfigParse::Snapshot(out)
}

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

/// Whether a permission question is about a tool of the chat's own MCP server, whose calls
/// SolidCraft's gate checks. The agent writes both values and ACP has nothing better to go on, so
/// the test is strict: the whole title is the tool's name as the Claude adapter forms it
/// (`mcp__solidcraft-app__batch`), one word with no room for a command line, a path or other free
/// text, and the kind is `other` (the kind an MCP tool has) or not given. Any other title goes
/// through the user's mode and remembered answers: agents that word their MCP tools differently,
/// and a user's own MCP server, which the gate doesn't see.
pub fn is_bridge_tool(title: &str, kind: Option<&str>) -> bool {
    let tool = title.strip_prefix("mcp__").and_then(|t| t.strip_prefix(BRIDGE_NAME)).and_then(|t| t.strip_prefix("__"));
    let named = tool.is_some_and(|t| !t.is_empty() && t.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'));
    named && matches!(kind, None | Some("other"))
}

/// The file or folder a permission question would read, when it asks for one of Claude Code's own
/// read tools ([`CLAUDE_ASK_TOOLS`]) as the Claude adapter forms such a question: `Read <path>` of
/// kind `read` with `rawInput.file_path`; `Find …` of kind `search` (Glob) with `rawInput.path`;
/// `grep …` of kind `search` with `rawInput.path` (or `file_path`). The path must be absolute; a
/// Glob pattern or Grep glob must be relative with no `..` (it could reach out of the folder
/// otherwise); an MCP tool's question (`_meta.claudeCode.mcpServer`) never counts. Anything else is
/// `None`, and the question goes to the user as before.
pub fn read_path(call: &Value) -> Option<std::path::PathBuf> {
    if call.pointer("/_meta/claudeCode/mcpServer").is_some() {
        return None;
    }
    let title = call.get("title")?.as_str()?;
    let kind = call.get("kind")?.as_str()?;
    let input = call.get("rawInput")?;
    let text = |key: &str| input.get(key).and_then(Value::as_str);
    // A pattern, or any of its `{a,b}` alternatives, that starts at a root or home, names a drive, or
    // steps up a folder could match outside the folder it is given.
    let rooted = |s: &str| s.starts_with(['/', '\\', '~']);
    let inside = |glob: &str| {
        !glob.is_empty()
            && !glob.contains(':')
            && !rooted(glob)
            && !glob.split(['{', ',']).any(rooted)
            && !glob.split(['/', '\\', '{', ',', '}']).any(|part| part == "..")
    };
    let path = match kind {
        "read" if title.starts_with("Read ") => text("file_path")?,
        "search" if title.starts_with("Find") => {
            if !inside(text("pattern")?) {
                return None;
            }
            text("path")?
        }
        "search" if title.starts_with("grep") => {
            if text("glob").is_some_and(|glob| !inside(glob)) {
                return None;
            }
            text("path").or_else(|| text("file_path"))?
        }
        _ => return None,
    };
    let path = std::path::PathBuf::from(path);
    path.is_absolute().then_some(path)
}

/// The file a permission question would write, when it asks for one of Claude Code's own write
/// tools ([`CLAUDE_PLAN_TOOLS`]) as the Claude adapter forms such a question: `Write <path>` or
/// `Edit <path>` of kind `edit` with a `rawInput.file_path`. An MCP tool's question
/// (`_meta.claudeCode.mcpServer`) never counts. Anything else is `None`. The path is kept even
/// when it isn't absolute, so the chat declines such a write instead of leaving it to the mode; a
/// WSL path the host doesn't read as absolute (`/mnt/c/...` on Windows) is taken back to the
/// drive it names ([`host_path`]).
pub fn write_path(call: &Value) -> Option<std::path::PathBuf> {
    if call.pointer("/_meta/claudeCode/mcpServer").is_some() || call.get("kind")?.as_str()? != "edit" {
        return None;
    }
    let title = call.get("title")?.as_str()?;
    if !CLAUDE_PLAN_TOOLS.iter().any(|tool| title.strip_prefix(tool).is_some_and(|rest| rest.starts_with(' '))) {
        return None;
    }
    let path = std::path::PathBuf::from(call.pointer("/rawInput/file_path")?.as_str()?);
    if path.is_absolute() {
        return Some(path);
    }
    Some(host_path(&path).unwrap_or(path))
}

/// The Windows drive path a WSL path under the default mount root names, the reverse of
/// [`wsl_path`]: `/mnt/c/Users/u` is `C:\Users\u`. Anything else is `None`.
pub fn host_path(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let text = path.to_str()?.strip_prefix("/mnt/")?;
    let mut chars = text.chars();
    let drive = chars.next().filter(char::is_ascii_alphabetic)?;
    let rest = chars.as_str();
    let rest = match rest.strip_prefix('/') {
        Some(rest) => rest,
        None if rest.is_empty() => rest,
        None => return None,
    };
    Some(format!("{}:\\{}", drive.to_ascii_uppercase(), rest.replace('/', "\\")).into())
}

/// The most of a plan a question shows, in characters.
pub const MAX_PLAN: usize = 20_000;

/// The plan a question asks the user to approve: Claude Code puts the text of its plan file in
/// `rawInput.plan` of its "Approve Plan" question (kind `switch_mode`). Blank is `None`; a plan
/// longer than [`MAX_PLAN`] is cut there.
pub fn plan_text(call: &Value) -> Option<String> {
    if call.get("kind").and_then(Value::as_str) != Some("switch_mode") {
        return None;
    }
    let plan = call.pointer("/rawInput/plan")?.as_str()?.trim();
    if plan.is_empty() {
        return None;
    }
    let mut text: String = plan.chars().take(MAX_PLAN).collect();
    if text.len() < plan.len() {
        text.push('…');
    }
    Some(text)
}

/// The MCP server the agent starts to reach the design: `solidcraft-cli mcp --connect` bridged to
/// this app's control port. The server finds the port's token in the token file the app wrote
/// (docs/control-protocol.md), so the token never travels through the agent, and it keeps its
/// folder limits (docs/mcp.md): only folders the user explicitly allowed in `allow_paths`, never
/// `--allow-any-path`.
#[derive(Clone, Debug, PartialEq)]
pub struct McpBridge {
    pub cli: std::path::PathBuf,
    pub port: u16,
    pub allow_paths: Vec<std::path::PathBuf>,
}

fn absolute_path(path: std::path::PathBuf) -> Option<std::path::PathBuf> {
    if path.is_absolute() { Some(path) } else { std::env::current_dir().ok().map(|cwd| cwd.join(path)) }
}

impl McpBridge {
    pub fn to_json(&self) -> Value {
        let control_dir = solidcraft_engine::control_auth::APP
            .token_path(self.port)
            .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
            .and_then(absolute_path);
        let mut args = vec!["mcp".to_string(), "--connect".to_string(), format!("127.0.0.1:{}", self.port)];
        for dir in &self.allow_paths {
            let text = dir.to_string_lossy();
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                args.push("--allow-path".to_string());
                args.push(trimmed.to_string());
            }
        }
        json!({
            "name": BRIDGE_NAME,
            "command": self.cli.to_string_lossy(),
            "args": args,
            "env": bridge_env(std::env::var_os("XDG_DATA_HOME"), control_dir),
        })
    }
    /// The same bridge for an agent inside WSL: the CLI as WSL sees it. It still runs on the
    /// Windows side (WSL starts Windows programs itself), so the port and `--allow-path` folders
    /// stay as Windows paths.
    pub fn in_wsl(&self) -> McpBridge {
        McpBridge { cli: wsl_path(&self.cli), port: self.port, allow_paths: self.allow_paths.clone() }
    }
}

/// The agent runs inside WSL: its program is `wsl` or `wsl.exe` (any case, with or without a
/// folder). Such an agent can't use Windows paths, so the session hands it [`wsl_path`]s (#5546).
pub fn runs_in_wsl(argv: &[String]) -> bool {
    argv.first().is_some_and(|program| {
        let name = program.rsplit(['\\', '/']).next().unwrap_or(program);
        name.eq_ignore_ascii_case("wsl") || name.eq_ignore_ascii_case("wsl.exe")
    })
}

/// A Windows drive path as WSL shows it under its default mount root: `C:\Users\u` is
/// `/mnt/c/Users/u`. Anything else (a UNC share, a path already in that form) is kept.
pub fn wsl_path(path: &std::path::Path) -> std::path::PathBuf {
    let text = path.to_string_lossy();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    let mut chars = text.chars();
    match (chars.next(), chars.next(), chars.next()) {
        (Some(drive), Some(':'), Some('\\' | '/') | None) if drive.is_ascii_alphabetic() => {
            let rest: String = chars.map(|c| if c == '\\' { '/' } else { c }).collect();
            format!("/mnt/{}/{rest}", drive.to_ascii_lowercase()).into()
        }
        _ => path.to_path_buf(),
    }
}

/// Pass the app's effective token folder directly to its MCP child. XDG_DATA_HOME is also
/// forwarded for other SolidCraft data lookups, but SOLIDCRAFT_CONTROL_DIR takes precedence in
/// control_auth and therefore keeps authentication aligned with the exact token_path for this port.
fn bridge_env(xdg_data_home: Option<std::ffi::OsString>, control_dir: Option<std::path::PathBuf>) -> Value {
    let mut env = Vec::with_capacity(2);
    if let Some(value) = xdg_data_home.as_deref().and_then(|value| value.to_str()) {
        env.push(json!({"name": "XDG_DATA_HOME", "value": value}));
    }
    if let Some(value) = control_dir.as_deref().and_then(|value| value.to_str()) {
        env.push(json!({"name": "SOLIDCRAFT_CONTROL_DIR", "value": value}));
    }
    Value::Array(env)
}

/// What every session is told about where it is. An agent with no route for it in
/// [`session_new_params`] doesn't get it.
pub const SYSTEM_NOTE: &str = "You are running inside SolidCraft, a parametric 3D CAD application, as the user's assistant. \
The open design is reachable through the `solidcraft-app` MCP server: use its tools (list_commands, batch, execute, inspect_design, \
measure, screenshot, set_parameter, undo, skill_reference) to read and change the design the user is looking at, and verify with \
measure or inspect_design. Before building anything beyond one primitive, read `skill_reference` for `solidcraft-modeling` (its \
SKILL.md has the workflow, its reference.md the command shapes and this version's traps), work the numbers out first, and after \
each stage compare `measure` with them. Prefer one `batch` over many small calls. Saving, opening or exporting files waits for the user's approval in \
the chat, and each message allows a limited number of changes. Don't use shell tools and don't write files: work through the SolidCraft \
tools. Read a file or folder with your own read tools only when the user attached it by path; to read another, ask the user to attach it. \
On a drawing, make section views and their section lines with `drawing.view.section`, and dimensions with \
`drawing.dimension.add` or `drawing.dimension.from_model`, never with free sketch lines or text notes: those do not move with their \
view, cannot be selected as section lines or dimensions, and do not update the model. If a drawing object has no command, say so \
and explain the limitation instead of drawing an imitation. Keep answers short.";

/// The most of the user's custom instructions (#8220) a session gets, in characters: the
/// Preferences field takes no more, and a longer text from an edited settings file is cut here.
pub const INSTRUCTIONS_LIMIT: usize = 4000;

/// [`SYSTEM_NOTE`] with the user's custom instructions (#8220) after it. Blank instructions leave
/// the note as it is.
pub fn system_note(instructions: &str) -> String {
    let text: String = instructions.trim().chars().take(INSTRUCTIONS_LIMIT).collect();
    let text = text.trim_end();
    if text.is_empty() {
        return SYSTEM_NOTE.to_string();
    }
    format!("{SYSTEM_NOTE}\n\nThe user's custom instructions, set in SolidCraft's preferences:\n{text}")
}

/// The Claude Code adapter's own tools that reach the computer rather than the design: turned off
/// for the session (`_meta.claudeCode.options.disallowedTools`), so it works through SolidCraft's.
/// Its read tools ([`CLAUDE_ASK_TOOLS`]) and write tools ([`CLAUDE_PLAN_TOOLS`]) are not among
/// them when the session is for the adapter the chat recognises ([`is_claude_adapter`]): they ask
/// instead, and it writes its plan with the write tools.
pub const CLAUDE_DISALLOWED_TOOLS: &[&str] = &["Bash", "BashOutput", "KillShell", "MultiEdit", "NotebookEdit", "WebFetch", "WebSearch", "Task"];

/// The Claude Code adapter's tools that write a file. In plan mode it writes its plan to a file in
/// its plan folder ([`PLANS_FOLDER`]) and reads it back into the question that asks the user to
/// approve the plan (`rawInput.plan`, [`plan_text`]); without them the question comes with no plan
/// (#19571). They are on only where the read tools are ([`CLAUDE_ASK_TOOLS`]), and every call asks
/// (a `permissions.ask` rule in the session's settings): the chat allows a write of a plan file by
/// itself ([`write_path`]) and declines any other.
pub const CLAUDE_PLAN_TOOLS: &[&str] = &["Write", "Edit"];

/// Claude Code's plan folder (its `plansDirectory` setting), inside the agent's working folder,
/// which is SolidCraft's own: not the user's `~/.claude/plans`.
pub const PLANS_FOLDER: &str = "plans";

/// The Claude Code adapter's settings for a session (`_meta.claudeCode.options.settings`): every
/// call of its write and read tools asks the chat, and plans go in [`PLANS_FOLDER`].
fn claude_settings() -> Value {
    let ask: Vec<&str> = CLAUDE_PLAN_TOOLS.iter().chain(CLAUDE_ASK_TOOLS).copied().collect();
    json!({"permissions": {"ask": ask}, "plansDirectory": PLANS_FOLDER})
}

/// The Claude Code adapter's read tools. They are on only in a session for the adapter the chat
/// recognises by its command ([`is_claude_adapter`], decided when the agent starts), and there
/// every call asks (a `permissions.ask` rule in the session's settings), so the question reaches
/// the panel: the chat allows it by itself for a path the user attached by path to this
/// conversation ([`read_path`]) and declines any other, whatever the mode. Any other session keeps
/// them off, so an adapter started some other way (a wrapper script, `node …/dist/index.js`)
/// reads nothing.
pub const CLAUDE_ASK_TOOLS: &[&str] = &["Read", "Glob", "Grep"];

/// The Claude Code adapter's built-in tools the session has (`_meta.claudeCode.options.tools`):
/// only these, so a tool a later Claude Code adds (or one the list above doesn't name, such as its
/// schedulers and message senders) stays off (#19554). `ExitPlanMode` is how the agent leaves the
/// plan mode the panel's mode picker offers; it only asks. The read tools ([`CLAUDE_ASK_TOOLS`])
/// and write tools ([`CLAUDE_PLAN_TOOLS`]) join them only in a session for the adapter the chat
/// recognises. [`CLAUDE_DISALLOWED_TOOLS`]
/// stays as a second guard.
pub const CLAUDE_TOOLS: &[&str] = &["ExitPlanMode"];

/// The `session/new` parameters. `init` is the agent's `initialize` reply: agents read the system
/// note from different places (#9811), and the standard field may only go to an agent offering it.
/// `note` is the system note ([`system_note`]). `claude`: the agent was recognised as Claude Code's
/// adapter when it started ([`is_claude_adapter`]), so its read and write tools are on, behind the
/// ask rule.
pub fn session_new_params(cwd: &std::path::Path, bridge: Option<&McpBridge>, init: &Value, note: &str, claude: bool) -> Value {
    let servers: Vec<Value> = bridge.map(McpBridge::to_json).into_iter().collect();
    let (tools, off): (Vec<&str>, Vec<&str>) = if claude {
        (CLAUDE_TOOLS.iter().chain(CLAUDE_ASK_TOOLS).chain(CLAUDE_PLAN_TOOLS).copied().collect(), CLAUDE_DISALLOWED_TOOLS.to_vec())
    } else {
        (CLAUDE_TOOLS.to_vec(), CLAUDE_DISALLOWED_TOOLS.iter().chain(CLAUDE_ASK_TOOLS).chain(CLAUDE_PLAN_TOOLS).copied().collect())
    };
    let mut meta = json!({
        // The Claude adapter's own key, for its versions without the standard field.
        "systemPrompt": {"append": note},
        // Hosts may opt a session out of "bypass permissions" (the Claude adapter reads it):
        // every tool call the agent asks about reaches the panel.
        // Its own shell and web tools are off: the agent works through SolidCraft's tools. Its
        // read and write tools ask, so the chat sees every read and write (it may write only its
        // plan), or are off too when the adapter wasn't recognised. Only the MCP servers given here
        // (`strictMcpConfig`): not the user's claude.ai connectors, nor the servers in their own
        // Claude Code configuration (#19554).
        "claudeCode": {"options": {
            "allowDangerouslySkipPermissions": false,
            "disallowedTools": off,
            "tools": tools,
            "settings": claude_settings(),
            "strictMcpConfig": true,
        }},
    });
    if takes_antigravity_note(init)
        && let Some(meta) = meta.as_object_mut()
    {
        meta.insert("antigravity".into(), json!({"systemPrompt": {"append": note}}));
    }
    let mut params = json!({
        "cwd": cwd.to_string_lossy(),
        "mcpServers": servers,
        "_meta": meta,
    });
    if takes_standard_note(init)
        && let Some(params) = params.as_object_mut()
    {
        params.insert("systemPrompt".into(), json!({"mode": "append", "content": [{"type": "text", "text": note}]}));
    }
    params
}

/// The agent is Claude Code's ACP adapter, the one agent whose own tools SolidCraft turns off or
/// limits: its program (or the package `npx` starts) is `claude-agent-acp` or the older
/// `claude-code-acp`.
pub fn is_claude_adapter(argv: &[String]) -> bool {
    argv.iter().any(|arg| {
        let name = arg.rsplit(['/', '\\']).next().unwrap_or(arg);
        name.starts_with("claude-agent-acp") || name.starts_with("claude-code-acp")
    })
}

/// Claude Code's own tool calls in the running turn, by id: which of its tools each one is (the
/// adapter names it in `_meta.claudeCode.toolName` on the call's updates) and which asked first.
/// With it a question about one of Claude Code's read or write tools is known as such whatever
/// command started the adapter, and a read or write that completes without having asked is caught:
/// the ask rule ([`CLAUDE_ASK_TOOLS`], [`CLAUDE_PLAN_TOOLS`]) is what lets the chat check each one,
/// and an adapter that stopped honouring it must not go unnoticed.
#[derive(Debug, Default)]
pub struct ClaudeCalls {
    tools: std::collections::HashMap<String, String>,
    asked: std::collections::HashSet<String>,
}

impl ClaudeCalls {
    /// More calls than any turn makes; past it, nothing new is recorded and nothing is caught.
    const MAX: usize = 4096;

    /// An update of call `id` (`tool`: the Claude tool it names, `status`: its new status).
    /// `Some(tool)` the first time one of Claude Code's read or write tools completes without having
    /// asked.
    pub fn update(&mut self, id: &str, tool: Option<&str>, status: Option<ToolStatus>) -> Option<String> {
        if let Some(tool) = tool
            && (self.tools.len() < Self::MAX || self.tools.contains_key(id))
        {
            self.tools.insert(id.to_string(), tool.to_string());
        }
        let unasked = status == Some(ToolStatus::Completed)
            && self.asked.len() < Self::MAX
            && !self.asked.contains(id)
            && self.tools.get(id).is_some_and(|tool| CLAUDE_ASK_TOOLS.iter().chain(CLAUDE_PLAN_TOOLS).any(|t| t == tool));
        if unasked { self.tools.remove(id) } else { None }
    }

    /// A question about call `id` (`tool`: the Claude tool the question itself names): the Claude
    /// tool it is about, from the question or from the call's updates.
    pub fn question(&mut self, id: Option<&str>, tool: Option<&str>) -> Option<String> {
        if let Some(id) = id
            && self.asked.len() < Self::MAX
        {
            self.asked.insert(id.to_string());
        }
        tool.map(str::to_string).or_else(|| id.and_then(|id| self.tools.get(id)).cloned())
    }

    /// A new turn: the last one's calls are done.
    pub fn clear(&mut self) {
        self.tools.clear();
        self.asked.clear();
    }
}

/// The agent offers ACP's `systemPrompt` field on its sessions.
fn takes_standard_note(init: &Value) -> bool {
    init.pointer("/agentCapabilities/sessionCapabilities/systemPrompt").is_some_and(|offer| !offer.is_null())
}

/// Antigravity offers its own key under `_meta` and doesn't read the Claude adapter's.
fn takes_antigravity_note(init: &Value) -> bool {
    init.pointer("/agentCapabilities/_meta/antigravity/systemPrompt/append").and_then(Value::as_bool).unwrap_or(false)
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

/// The agent's models in a `session/new` or `session/load` reply (`models`: `currentModelId` and
/// `availableModels: [{modelId, name, description?}]`): the current one and each model. `None`
/// when the agent reports none.
pub fn models(session: &Value) -> Option<(String, Vec<ModelInfo>)> {
    let m = session.get("models")?;
    let current = m.get("currentModelId").and_then(Value::as_str).unwrap_or("").to_string();
    let available: Vec<ModelInfo> = m
        .get("availableModels")?
        .as_array()?
        .iter()
        .take(200)
        .filter_map(|x| {
            let id = x.get("modelId").or_else(|| x.get("id"))?.as_str()?.to_string();
            let name = x.get("name").and_then(Value::as_str).filter(|n| !n.is_empty()).unwrap_or(&id).to_string();
            let description = x.get("description").and_then(Value::as_str).unwrap_or("").to_string();
            Some(ModelInfo { id, name, description })
        })
        .collect();
    (!available.is_empty()).then_some((current, available))
}

/// `session/set_model` parameters.
pub fn set_model_params(session: &str, model: &str) -> Value {
    json!({"sessionId": session, "modelId": model})
}
pub fn set_config_option_params(session: &str, id: &str, value: &str) -> Value {
    json!({"sessionId": session, "configId": id, "value": value})
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
                            let mut command = meta.and_then(|t| Some((t.get("command")?.as_str()?.to_string(), strings(t.get("args")))));
                            let mut args = strings(m.get("args"));
                            // Vibe's Python launcher can advertise its own path as an argument.
                            // The configured ACP command already starts Vibe and accepts --setup.
                            if id == "vibe-setup" && init.pointer("/agentInfo/name").and_then(Value::as_str) == Some("@mistralai/mistral-vibe") {
                                args = vec!["--setup".into()];
                                command = None;
                            }
                            AuthKind::Terminal { args, command }
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
    Reply {
        id: u64,
        result: Result<Value, (i64, String)>,
    },
    /// Text the agent streams (`thought`: its reasoning, shown dimmed).
    Text {
        thought: bool,
        text: String,
    },
    /// A new tool call, or news about one (`title`/`status` when they changed).
    Tool {
        id: String,
        title: Option<String>,
        kind: Option<String>,
        status: Option<ToolStatus>,
        input: Option<Value>,
        output: Option<String>,
        /// Which of Claude Code's own tools it is (`_meta.claudeCode.toolName`), when the Claude
        /// adapter says.
        claude_tool: Option<String>,
    },
    /// The agent's plan: each entry with its status.
    Plan {
        session: String,
        entries: Vec<(String, String)>,
    },
    /// The agent asks before running a tool call; answered with [`permission_reply`].
    Permission {
        rpc: Value,
        title: String,
        /// The tool call's kind (`execute`, `edit`, `other`, …), as the agent gives it.
        kind: Option<String>,
        options: Vec<PermissionOption>,
        /// The path it would read, when it is one of Claude Code's own read tools ([`read_path`]).
        reads: Option<std::path::PathBuf>,
        /// The tool call it asks about (`toolCall.toolCallId`).
        call: Option<String>,
        /// Which of Claude Code's own tools it is, when the question says
        /// (`toolCall._meta.claudeCode.toolName`; the adapter gives it only for some).
        claude_tool: Option<String>,
        /// The file it would write, when it is one of Claude Code's own write tools ([`write_path`]).
        writes: Option<std::path::PathBuf>,
        /// The plan it asks the user to approve ([`plan_text`]).
        plan: Option<String>,
    },
    /// The agent says whether it is logged in (an extension both adapters send).
    AuthStatus(String),
    /// The agent switched to another of its modes.
    ModeChanged(String),
    /// The agent switched to another model.
    ModelChanged(String),
    ConfigOptions {
        session: String,
        options: Vec<ConfigOption>,
    },
    /// A request we don't serve (file system, terminal, …): answer "method not found".
    Unsupported {
        rpc: Value,
        method: String,
    },
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
            let kind = call.and_then(|c| c.get("kind")).and_then(Value::as_str).map(str::to_string);
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
            let reads = call.and_then(read_path);
            let text = |pointer: &str| call.and_then(|c| c.pointer(pointer)).and_then(Value::as_str).map(str::to_string);
            let (claude_tool, call_id) = (text("/_meta/claudeCode/toolName"), text("/toolCallId"));
            let writes = call.and_then(write_path);
            let plan = call.and_then(plan_text);
            Some(Incoming::Permission { rpc: id.clone(), title, kind, options, reads, writes, plan, call: call_id, claude_tool })
        }
        (Some(m), Some(id)) => Some(Incoming::Unsupported { rpc: id.clone(), method: m.to_string() }),
        (Some("session/update"), None) => Some(update(params.get("update")?, params.get("sessionId").and_then(Value::as_str).unwrap_or(""))),
        (Some("_auth/status_update"), None) => {
            params.get("authStatus").and_then(|s| s.get("label")).and_then(Value::as_str).map(|l| Incoming::AuthStatus(l.to_string()))
        }
        _ => Some(Incoming::Ignored),
    }
}

fn update(u: &Value, session: &str) -> Incoming {
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
                // ACP's `rawInput`: the arguments the tool was called with.
                input: u.get("rawInput").filter(|v| v.is_object()).cloned(),
                output: tool_output(u),
                claude_tool: u.pointer("/_meta/claudeCode/toolName").and_then(Value::as_str).map(str::to_string),
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
        Some("config_option_update") => match config_options(u) {
            ConfigParse::Snapshot(options) if !session.is_empty() => Incoming::ConfigOptions { session: session.into(), options },
            _ => Incoming::Ignored,
        },
        Some("plan") if !session.is_empty() => Incoming::Plan {
            session: session.into(),
            entries: u
                .get("entries")
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
        },
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
        assert!(is_bridge_tool(&format!("mcp__{BRIDGE_NAME}__batch"), Some("other")));
    }

    /// #20731: the note sends drawing work to the real commands and says to explain a limitation
    /// rather than imitate a section line or a dimension with free lines or text.
    #[test]
    fn the_note_names_the_drawing_commands_and_forbids_imitations() {
        for name in ["drawing.view.section", "drawing.dimension.add", "drawing.dimension.from_model"] {
            assert!(SYSTEM_NOTE.contains(name), "{name}");
        }
        assert!(SYSTEM_NOTE.contains("never with free sketch lines or text notes"));
        assert!(SYSTEM_NOTE.contains("explain the limitation instead of drawing an imitation"));
    }

    /// Only the whole name of a tool of the chat's own server counts, whatever else a title holds.
    #[test]
    fn only_the_bridges_exact_tool_names_are_its_tools() {
        for tool in ["batch", "inspect_design", "set_parameter"] {
            assert!(is_bridge_tool(&format!("mcp__solidcraft-app__{tool}"), None), "{tool}");
            assert!(is_bridge_tool(&format!("mcp__solidcraft-app__{tool}"), Some("other")), "{tool}");
        }
        for title in [
            "",
            "solidcraft",
            "SolidCraft",
            "run solidcraft-cli; ls",
            "solidcraft-cli --version",
            "Edit ~/.local/share/solidcraft/agent-chat/work/notes.txt",
            // A user's own MCP server (docs/mcp.md), which SolidCraft's gate doesn't see.
            "mcp__solidcraft__batch",
            "mcp__solidcraft-live__batch",
            "mcp__other__solidcraft-app",
            "mcp__solidcraft-app__",
            "mcp__solidcraft-app",
            "solidcraft-app__batch",
            "solidcraft-app: batch",
            "Tool: solidcraft-app/batch",
            "mcp.solidcraft-app.batch",
            " mcp__solidcraft-app__batch",
            "mcp__solidcraft-app__batch ",
            "mcp__solidcraft-app__batch; ls",
            "mcp__solidcraft-app__batch && ls",
            "mcp__solidcraft-app__batch\nls",
            "mcp__solidcraft-app__../../tool",
            "mcp__SOLIDCRAFT-APP__batch",
            "MCP__solidcraft-app__batch",
            "mcp__solidcraft-app__Batch",
        ] {
            assert!(!is_bridge_tool(title, None), "{title:?}");
        }
        for kind in ["execute", "edit", "delete", "move", "read", "search", "fetch", "think", ""] {
            assert!(!is_bridge_tool("mcp__solidcraft-app__batch", Some(kind)), "{kind:?}");
        }
    }

    #[test]
    fn the_session_gets_the_bridged_mcp_server_without_a_token_or_any_path() {
        let b = McpBridge { cli: "/opt/solidcraft/solidcraft-cli".into(), port: 41234, allow_paths: Vec::new() };
        let p = session_new_params(std::path::Path::new("/home/u/designs"), Some(&b), &json!({}), SYSTEM_NOTE, true);
        let s = &p["mcpServers"][0];
        assert_eq!(s["name"], BRIDGE_NAME);
        assert_eq!(s["args"], json!(["mcp", "--connect", "127.0.0.1:41234"]));
        assert!(s.get("token").is_none(), "{s}");
        assert!(s.get("path").is_none(), "{s}");
        assert!(s.get("allow-any-path").is_none(), "{s}");
        // The env carries folders only, never a token or a token file.
        for e in s["env"].as_array().into_iter().flatten() {
            assert!(matches!(e["name"].as_str(), Some("XDG_DATA_HOME" | "SOLIDCRAFT_CONTROL_DIR")), "{e}");
            assert!(!e["value"].as_str().unwrap_or_default().ends_with(".token"), "{e}");
        }
        let text = p.to_string();
        assert!(!text.contains("allow-any-path") && !text.contains("TOKEN"), "{text}");
        assert!(p["_meta"]["claudeCode"]["options"]["disallowedTools"].as_array().is_some_and(|t| t.iter().any(|x| x == "Bash")));
        assert_eq!(session_new_params(std::path::Path::new("/tmp"), None, &json!({}), SYSTEM_NOTE, true)["mcpServers"], json!([]));
    }

    #[test]
    fn the_bridged_mcp_server_forwards_configured_allow_paths() {
        let b = McpBridge {
            cli: "/opt/solidcraft/solidcraft-cli".into(),
            port: 41234,
            allow_paths: vec![" /work/cad ".into(), "".into(), "/work/shared".into()],
        };
        let p = session_new_params(std::path::Path::new("/home/u/designs"), Some(&b), &json!({}), SYSTEM_NOTE, true);
        assert_eq!(
            p["mcpServers"][0]["args"],
            json!(["mcp", "--connect", "127.0.0.1:41234", "--allow-path", "/work/cad", "--allow-path", "/work/shared"])
        );
        assert!(!p.to_string().contains("allow-any-path"));
    }

    /// #19554: Claude Code gets only the chat's MCP server, none of the user's claude.ai connectors
    /// or own servers (`strictMcpConfig`), and only the built-in tools the chat names (`tools`),
    /// none of which the disallowed list turns off: its read tools only when the adapter was
    /// recognised (#16458).
    #[test]
    fn claude_code_gets_only_the_chats_server_and_named_tools() {
        let b = McpBridge { cli: "/opt/solidcraft/solidcraft-cli".into(), port: 41234, allow_paths: Vec::new() };
        for (bridge, claude) in [(Some(&b), true), (None, true), (None, false)] {
            let p = session_new_params(std::path::Path::new("/tmp"), bridge, &json!({}), SYSTEM_NOTE, claude);
            let options = &p["_meta"]["claudeCode"]["options"];
            assert_eq!(options["strictMcpConfig"], json!(true), "{p}");
            let tools = if claude { json!(["ExitPlanMode", "Read", "Glob", "Grep", "Write", "Edit"]) } else { json!(["ExitPlanMode"]) };
            assert_eq!(options["tools"], tools, "{p}");
            assert!(CLAUDE_TOOLS.iter().all(|t| !CLAUDE_DISALLOWED_TOOLS.contains(t)));
            assert_eq!(p["mcpServers"].as_array().map(Vec::len), Some(usize::from(bridge.is_some())));
        }
    }

    /// #9811: the note goes by the routes an agent's `initialize` reply offers, and the standard
    /// field never goes to an agent that doesn't offer it, as the protocol asks.
    #[test]
    fn the_system_note_takes_every_route_the_agent_offers() {
        // With the user's custom instructions (#8220), which go wherever the note goes.
        let note = system_note("Sketch on XZ with explicit plane vectors.");
        let params = |init: Value| session_new_params(std::path::Path::new("/tmp"), None, &init, &note, true);
        let standard = json!({"mode": "append", "content": [{"type": "text", "text": note}]});

        let offers_standard = params(json!({"agentCapabilities": {"sessionCapabilities": {"systemPrompt": {}}}}));
        assert_eq!(offers_standard["systemPrompt"], standard);
        assert!(offers_standard["_meta"].get("antigravity").is_none());

        let offers_antigravity = params(json!({"agentCapabilities": {"_meta": {"antigravity": {"systemPrompt": {"append": true}}}}}));
        assert_eq!(offers_antigravity["_meta"]["antigravity"]["systemPrompt"]["append"], note);
        assert!(offers_antigravity.get("systemPrompt").is_none());

        for none in [
            json!({}),
            json!({"agentCapabilities": {"sessionCapabilities": {"systemPrompt": null}}}),
            json!({"agentCapabilities": {"_meta": {"antigravity": {"systemPrompt": {"append": false}}}}}),
        ] {
            let p = params(none);
            assert!(p.get("systemPrompt").is_none(), "{p}");
            assert!(p["_meta"].get("antigravity").is_none(), "{p}");
        }

        // Every session keeps the key older Claude adapters read, and the tools they turn off.
        for init in [json!({}), json!({"agentCapabilities": {"sessionCapabilities": {"systemPrompt": {}}}})] {
            let p = params(init);
            assert_eq!(p["_meta"]["systemPrompt"]["append"], note);
            assert!(p["_meta"]["claudeCode"]["options"]["disallowedTools"].is_array());
        }
    }

    /// #8220: the user's custom instructions follow the note, trimmed and cut at the limit; blank
    /// ones leave the note as it is.
    #[test]
    fn custom_instructions_follow_the_note_trimmed_and_capped() {
        assert_eq!(system_note(""), SYSTEM_NOTE);
        assert_eq!(system_note(" \n\t "), SYSTEM_NOTE);
        let note = system_note("\n  Use M3 screws.\nPrint flat side down.  \n");
        assert!(note.starts_with(SYSTEM_NOTE), "{note}");
        assert!(note.ends_with("preferences:\nUse M3 screws.\nPrint flat side down."), "{note}");
        let note = system_note(&"ż".repeat(INSTRUCTIONS_LIMIT + 10));
        assert_eq!(note.chars().filter(|c| *c == 'ż').count(), INSTRUCTIONS_LIMIT);
    }

    /// #5546: an agent started through `wsl.exe` gets the session's paths as WSL shows them.
    #[test]
    fn an_agent_inside_wsl_gets_wsl_paths() {
        let argv = |line: &str| split_command(line);
        assert!(runs_in_wsl(&argv("wsl.exe -d Ubuntu -e bash -lc claude-agent-acp")));
        assert!(runs_in_wsl(&argv("WSL claude-agent-acp")));
        assert!(runs_in_wsl(&argv(r#""C:\Windows\System32\wsl.exe" -e claude-agent-acp"#)));
        assert!(!runs_in_wsl(&argv("claude-agent-acp")));
        assert!(!runs_in_wsl(&argv("wslconfig /l")));
        assert!(!runs_in_wsl(&[]));

        let wsl = |p: &str| wsl_path(std::path::Path::new(p)).to_string_lossy().into_owned();
        assert_eq!(wsl(r"C:\Users\u\AppData\Local\SolidCraft\agent-chat\work"), "/mnt/c/Users/u/AppData/Local/SolidCraft/agent-chat/work");
        assert_eq!(wsl(r"\\?\D:\Program Files\SolidCraft\solidcraft-cli.exe"), "/mnt/d/Program Files/SolidCraft/solidcraft-cli.exe");
        assert_eq!(wsl("E:/designs"), "/mnt/e/designs");
        assert_eq!(wsl("C:"), "/mnt/c/");
        assert_eq!(wsl("/home/u/designs"), "/home/u/designs");
        assert_eq!(wsl(r"\\server\share\x"), r"\\server\share\x");

        let b =
            McpBridge { cli: r"C:\Program Files\SolidCraft\solidcraft-cli.exe".into(), port: 41234, allow_paths: vec![r"D:\Projects\CAD".into()] };
        let p = session_new_params(&wsl_path(std::path::Path::new(r"C:\Users\u\work")), Some(&b.in_wsl()), &json!({}), SYSTEM_NOTE, true);
        assert_eq!(p["cwd"], "/mnt/c/Users/u/work");
        assert_eq!(p["mcpServers"][0]["command"], "/mnt/c/Program Files/SolidCraft/solidcraft-cli.exe");
        assert_eq!(p["mcpServers"][0]["args"], json!(["mcp", "--connect", "127.0.0.1:41234", "--allow-path", r"D:\Projects\CAD"]));
    }

    #[test]
    fn absolute_path_preserves_absolute_paths() {
        let path = std::path::PathBuf::from("/tmp/solidcraft/control");
        assert_eq!(absolute_path(path.clone()), Some(path));
    }

    #[test]
    fn absolute_path_resolves_relative_paths() {
        let path = std::path::PathBuf::from("solidcraft/control");
        let result = absolute_path(path).unwrap();

        assert!(result.is_absolute());
        assert!(result.ends_with("solidcraft/control"));
    }

    #[test]
    fn bridge_environment_passes_data_root_and_effective_control_folder_without_secrets() {
        let env = bridge_env(Some(std::ffi::OsString::from("/flatpak/data")), Some(std::path::PathBuf::from("/flatpak/data/solidcraft/control")));
        assert_eq!(
            env,
            json!([
                {"name": "XDG_DATA_HOME", "value": "/flatpak/data"},
                {"name": "SOLIDCRAFT_CONTROL_DIR", "value": "/flatpak/data/solidcraft/control"},
            ])
        );
        assert_eq!(bridge_env(None, None), json!([]));
        assert!(env.to_string().find("token").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn bridge_environment_omits_non_utf8_paths_and_preserves_valid_entries() {
        use std::os::unix::ffi::OsStringExt;

        let invalid = std::ffi::OsString::from_vec(vec![b'/', 0xff]);

        assert_eq!(
            bridge_env(Some(invalid.clone()), Some("/valid/control".into())),
            json!([{"name": "SOLIDCRAFT_CONTROL_DIR", "value": "/valid/control"}])
        );

        assert_eq!(bridge_env(Some("/valid/data".into()), Some(invalid.into())), json!([{"name": "XDG_DATA_HOME", "value": "/valid/data"}]));
    }

    #[test]
    fn vibe_terminal_setup_uses_the_configured_acp_launcher() {
        let mut init = json!({
            "agentInfo": {"name": "@mistralai/mistral-vibe"},
            "authMethods": [{
                "id": "vibe-setup", "type": "terminal", "name": "Register your API Key",
                "args": [r"C:\Program Files\Vibe\vibe-acp", "--setup"],
                "_meta": {"terminal-auth": {"command": "python.exe", "args": [r"C:\Program Files\Vibe\vibe-acp", "--setup"]}}
            }]
        });
        let methods = auth_methods(&init);
        assert_eq!(methods[0].kind, AuthKind::Terminal { args: vec!["--setup".into()], command: None });

        init["agentInfo"]["name"] = json!("another-agent");
        let methods = auth_methods(&init);
        let AuthKind::Terminal { args, command } = &methods[0].kind else { panic!("terminal method expected") };
        assert_eq!(args, &[r"C:\Program Files\Vibe\vibe-acp", "--setup"]);
        assert!(command.is_some());
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
            Some(Incoming::Tool {
                id: "t1".into(),
                title: None,
                kind: None,
                status: Some(ToolStatus::Completed),
                input: None,
                output: Some("ok".into()),
                claude_tool: None,
            })
        );
        let perm = r#"{"jsonrpc":"2.0","id":7,"method":"session/request_permission","params":{"sessionId":"s","toolCall":{"toolCallId":"t1","title":"batch"},"options":[{"optionId":"allow","name":"Allow","kind":"allow_once"},{"optionId":"reject","name":"Reject","kind":"reject_once"}]}}"#;
        let Some(Incoming::Permission { rpc, title, kind, options, reads, writes, plan, .. }) = parse(perm) else { panic!() };
        assert_eq!((title.as_str(), options.len(), options[1].kind.as_str()), ("batch", 2, "reject_once"));
        assert_eq!((kind, reads, writes, plan), (None, None, None, None));
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
        let Incoming::Tool { output: Some(o), .. } = update(&u, "s") else { panic!() };
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
        // An agent without models, or with an empty list, reports none.
        assert!(models(&json!({"sessionId": "s"})).is_none());
        assert!(models(&json!({"models": {"currentModelId": "x", "availableModels": []}})).is_none());
        let changed = r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"current_model_update","currentModelId":"haiku"}}}"#;
        assert_eq!(parse(changed), Some(Incoming::ModelChanged("haiku".into())));
        assert_eq!(set_model_params("s", "opus"), json!({"sessionId": "s", "modelId": "opus"}));
        assert_eq!(set_config_option_params("s1", "vendor.model", "smart"), json!({"sessionId":"s1","configId":"vendor.model","value":"smart"}));
    }

    #[test]
    fn config_options_parse_flat_and_grouped_selects_and_ignore_unsupported() {
        let v = json!({"configOptions":[
            {"id":"arbitrary","name":"Model","category":"model","type":"select","currentValue":"a","options":[{"value":"a","name":"A"}]},
            {"id":"effort","category":"thought_level","type":"select","currentValue":"high","options":[{"group":"Reasoning","options":[{"value":"high","name":"High"}]}]},
            {"id":"bad","type":"boolean"}]});
        let ConfigParse::Snapshot(options) = config_options(&v) else { panic!() };
        assert_eq!(options.len(), 2);
        assert_eq!(options[0].id, "arbitrary");
        assert_eq!(options[1].options[0].group.as_deref(), Some("Reasoning"));
        assert!(matches!(
            config_options(&json!({"configOptions":[{"type":"select","id":"broken","currentValue":"x","options":[]}]})),
            ConfigParse::Invalid
        ));
        assert_eq!(config_options(&json!({"configOptions":[]})), ConfigParse::Snapshot(vec![]));
        let notification = json!({"sessionUpdate":"config_option_update","configOptions":[]});
        assert_eq!(update(&notification, "s"), Incoming::ConfigOptions { session: "s".into(), options: vec![] });
    }

    #[test]
    fn config_snapshots_bound_choices_and_preserve_unknown_categories() {
        let choices: Vec<_> = (0..300).map(|n| json!({"value":n.to_string(),"name":"Choice"})).collect();
        let option = json!({"id":"custom","name":"Custom","type":"select","currentValue":"0","options":[
            {"group":"internal-id","name":"Readable group","options":choices},
            {"value":"extra","name":"Extra"}
        ]});
        let ConfigParse::Snapshot(options) = config_options(&json!({"configOptions":[option.clone(),option]})) else { panic!() };
        assert_eq!(options.len(), 1, "duplicate IDs do not produce duplicate controls");
        assert_eq!(options[0].options.len(), 128, "the choice cap spans groups and flat entries");
        assert_eq!(options[0].options[0].group.as_deref(), Some("Readable group"));
        assert_eq!(options[0].category, "", "categories are optional metadata");
        assert_eq!(config_options(&json!({})), ConfigParse::Absent);
        assert_eq!(config_options(&json!({"configOptions":null})), ConfigParse::Invalid);
        assert_eq!(config_options(&json!({"configOptions":[{"type":"select","id":"broken"}]})), ConfigParse::Invalid);
    }

    /// In a session for the adapter the chat recognises, Claude Code's read tools are on and ask
    /// before every call, as do its write tools, for its plan; its shell and web tools stay off.
    #[test]
    fn claudes_read_tools_ask_and_its_other_tools_are_off() {
        let p = session_new_params(std::path::Path::new("/tmp"), None, &json!({}), SYSTEM_NOTE, true);
        let options = &p["_meta"]["claudeCode"]["options"];
        let off: Vec<&str> = options["disallowedTools"].as_array().map(|t| t.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        for tool in ["Bash", "MultiEdit", "NotebookEdit", "WebFetch", "WebSearch", "Task"] {
            assert!(off.contains(&tool), "{tool} off: {off:?}");
        }
        for tool in CLAUDE_ASK_TOOLS.iter().chain(CLAUDE_PLAN_TOOLS) {
            assert!(!off.contains(tool), "{tool} on: {off:?}");
        }
        assert_eq!(options["settings"]["permissions"]["ask"], json!(["Write", "Edit", "Read", "Glob", "Grep"]));
        assert_eq!(options["settings"]["plansDirectory"], "plans");
        assert!(options.get("resume").is_none());
        assert!(SYSTEM_NOTE.contains("attached it by path"), "{SYSTEM_NOTE}");
        let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(is_claude_adapter(&argv(&["/opt/homebrew/bin/claude-agent-acp"])));
        assert!(is_claude_adapter(&argv(&["npx", "-y", "@zed-industries/claude-code-acp"])));
        assert!(is_claude_adapter(&argv(&["C:\\npm\\claude-agent-acp.cmd"])));
        assert!(!is_claude_adapter(&argv(&["codex-acp"])) && !is_claude_adapter(&argv(&["opencode", "acp"])));
    }

    /// An agent whose command doesn't name the Claude adapter (a wrapper script, `node
    /// …/dist/index.js`) keeps Claude Code's read tools off with its other tools, so the adapter
    /// started that way reads nothing, whatever the mode (#16458 review).
    #[test]
    fn an_adapter_started_another_way_gets_no_read_tools() {
        let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        for command in
            [&["node", "/opt/claude/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js"][..], &["/home/u/bin/claude-wrapper.sh"]]
        {
            assert!(!is_claude_adapter(&argv(command)), "{command:?}");
        }
        let p = session_new_params(std::path::Path::new("/tmp"), None, &json!({}), SYSTEM_NOTE, false);
        let options = &p["_meta"]["claudeCode"]["options"];
        let off: Vec<&str> = options["disallowedTools"].as_array().map(|t| t.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        for tool in CLAUDE_ASK_TOOLS.iter().chain(CLAUDE_PLAN_TOOLS).chain(CLAUDE_DISALLOWED_TOOLS) {
            assert!(off.contains(tool), "{tool} off: {off:?}");
        }
        assert_eq!(options["allowDangerouslySkipPermissions"], json!(false));
    }

    /// The adapter names its own tool on a call's updates, and a question names the call: so the
    /// chat knows a question about Claude Code's Read whatever started the adapter, and a read
    /// that completes without having asked is caught, once (#16458 review).
    #[test]
    fn claude_calls_name_questions_and_catch_a_read_that_never_asked() {
        let tool = json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "s", "update": {
            "sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Read /x", "kind": "read", "status": "pending",
            "_meta": {"claudeCode": {"toolName": "Read"}}}}});
        assert!(matches!(parse(&tool.to_string()), Some(Incoming::Tool { claude_tool: Some(t), .. }) if t == "Read"));
        let perm = json!({"jsonrpc": "2.0", "id": 4, "method": "session/request_permission", "params": {"sessionId": "s",
            "toolCall": {"toolCallId": "t1", "title": "Read /x", "kind": "read"}, "options": []}});
        assert!(matches!(parse(&perm.to_string()), Some(Incoming::Permission { call: Some(c), claude_tool: None, .. }) if c == "t1"));

        let mut calls = ClaudeCalls::default();
        assert_eq!(calls.update("t1", Some("Read"), Some(ToolStatus::Pending)), None);
        assert_eq!(calls.question(Some("t1"), None).as_deref(), Some("Read"), "named by its call");
        assert_eq!(calls.update("t1", None, Some(ToolStatus::Completed)), None, "it asked");
        assert_eq!(calls.question(Some("t9"), Some("Glob")).as_deref(), Some("Glob"), "named by the question");
        assert_eq!(calls.question(Some("t8"), None), None, "another agent's call");

        assert_eq!(calls.update("t2", Some("Grep"), Some(ToolStatus::Pending)), None);
        assert_eq!(calls.update("t2", None, Some(ToolStatus::Completed)).as_deref(), Some("Grep"), "it never asked");
        assert_eq!(calls.update("t2", None, Some(ToolStatus::Completed)), None, "caught once");
        assert_eq!(calls.update("t3", Some("mcp__solidcraft-app__batch"), Some(ToolStatus::Completed)), None, "not a read tool");
        assert_eq!(calls.update("t4", Some("Read"), Some(ToolStatus::Failed)), None, "a failed read read nothing");
        assert_eq!(calls.update("t5", None, Some(ToolStatus::Completed)), None, "another agent's call");
        assert_eq!(calls.update("t6", Some("Write"), Some(ToolStatus::Completed)).as_deref(), Some("Write"), "a write that never asked");
        calls.clear();
        assert_eq!(calls.question(Some("t1"), None), None, "a new turn");
    }

    /// Only the Claude adapter's own Write and Edit questions give a file to check; the plan comes
    /// only from its "Approve Plan" question, cut at [`MAX_PLAN`].
    #[test]
    fn claudes_writes_and_plan_are_read_from_its_questions() {
        let f = "/w/plans/plan-a.md";
        let write = |v: Value| write_path(&v);
        assert_eq!(write(json!({"title": format!("Write {f}"), "kind": "edit", "rawInput": {"file_path": f, "content": "# Plan"}})), Some(f.into()));
        assert_eq!(write(json!({"title": "Edit plans/plan-a.md", "kind": "edit", "rawInput": {"file_path": f}})), Some(f.into()));
        // A path that isn't absolute is kept, so the chat declines it; WSL's form goes back to its drive.
        assert_eq!(
            write(json!({"title": "Write plans/plan-a.md", "kind": "edit", "rawInput": {"file_path": "plans/plan-a.md"}})),
            Some("plans/plan-a.md".into())
        );
        let wsl = "/mnt/c/Users/u/plans/a.md";
        let host = if std::path::Path::new(wsl).is_absolute() { wsl.into() } else { std::path::PathBuf::from(r"C:\Users\u\plans\a.md") };
        assert_eq!(write(json!({"title": format!("Write {wsl}"), "kind": "edit", "rawInput": {"file_path": wsl}})), Some(host));
        assert_eq!(host_path(std::path::Path::new(wsl)), Some(r"C:\Users\u\plans\a.md".into()));
        assert_eq!(host_path(std::path::Path::new("/mnt/d")), Some(r"D:\".into()));
        for other in ["/mnt/cd/x", "/mnt/1/x", "/home/u/a.md", "plans/a.md"] {
            assert_eq!(host_path(std::path::Path::new(other)), None, "{other}");
        }
        for call in [
            json!({"title": format!("Write {f}"), "kind": "read", "rawInput": {"file_path": f}}),
            json!({"title": format!("Writer {f}"), "kind": "edit", "rawInput": {"file_path": f}}),
            json!({"title": format!("MultiEdit {f}"), "kind": "edit", "rawInput": {"file_path": f}}),
            json!({"title": format!("Write {f}"), "kind": "edit", "rawInput": {"path": f}}),
            json!({"title": format!("Write {f}"), "kind": "edit", "rawInput": {"file_path": f}, "_meta": {"claudeCode": {"mcpServer": {"name": "x"}}}}),
        ] {
            assert_eq!(write(call.clone()), None, "{call}");
        }
        let plan = |v: Value| plan_text(&v);
        let approve = |p: Value| json!({"title": "Ready to code?", "kind": "switch_mode", "rawInput": {"plan": p, "planFilePath": f}});
        assert_eq!(plan(approve(json!("\n# Plan\n\n1. A block.\n"))).as_deref(), Some("# Plan\n\n1. A block."));
        assert_eq!(plan(approve(json!("  "))), None);
        assert_eq!(plan(approve(json!(7))), None);
        assert_eq!(plan(json!({"title": "batch", "kind": "other", "rawInput": {"plan": "x"}})), None);
        let long = plan(approve(json!("é".repeat(MAX_PLAN + 5)))).unwrap_or_default();
        assert_eq!((long.chars().count(), long.ends_with('…')), (MAX_PLAN + 1, true));
        let perm = json!({
            "jsonrpc": "2.0", "id": 9, "method": "session/request_permission",
            "params": {"sessionId": "s", "toolCall": {"toolCallId": "t", "title": "Ready to code?", "kind": "switch_mode", "rawInput": {"plan": "# Plan"}}, "options": []},
        });
        assert!(matches!(parse(&perm.to_string()), Some(Incoming::Permission { plan: Some(p), writes: None, .. }) if p == "# Plan"));
    }

    /// Only the Claude adapter's own Read, Glob and Grep questions, with an absolute path and a
    /// pattern that can't reach out of it, give a path to check; anything else goes to the user.
    #[test]
    fn read_questions_give_their_path_only_in_the_adapters_own_form() {
        let dir = std::env::temp_dir().join("photos");
        let file = dir.join("a.jpg");
        let (d, f) = (dir.to_string_lossy().to_string(), file.to_string_lossy().to_string());
        let read = |v: Value| read_path(&v);
        assert_eq!(read(json!({"title": format!("Read {f}"), "kind": "read", "rawInput": {"file_path": f}})), Some(file.clone()));
        assert_eq!(
            read(json!({"title": format!("Find `{d}` `*.jpg`"), "kind": "search", "rawInput": {"path": d, "pattern": "**/*.jpg"}})),
            Some(dir.clone())
        );
        assert_eq!(
            read(json!({"title": "grep \"x\"", "kind": "search", "rawInput": {"path": d, "pattern": "x", "glob": "*.md"}})),
            Some(dir.clone())
        );
        assert_eq!(
            read(json!({"title": "grep \"x\"", "kind": "search", "rawInput": {"file_path": f, "pattern": "../x"}})),
            Some(file.clone()),
            "a regex is no path"
        );
        for refused in [
            json!({"title": "Read a.jpg", "kind": "read", "rawInput": {"file_path": "a.jpg"}}),
            json!({"title": "Find", "kind": "search", "rawInput": {"path": d, "pattern": "../*"}}),
            json!({"title": "Find", "kind": "search", "rawInput": {"path": d, "pattern": "x/../../*"}}),
            json!({"title": "Find", "kind": "search", "rawInput": {"path": d, "pattern": "/etc/*"}}),
            json!({"title": "Find", "kind": "search", "rawInput": {"path": d, "pattern": "{x,/etc/*}"}}),
            json!({"title": "Find", "kind": "search", "rawInput": {"path": d, "pattern": "~/.ssh/*"}}),
            json!({"title": "Find", "kind": "search", "rawInput": {"path": d, "pattern": "C:/x"}}),
            json!({"title": "Find", "kind": "search", "rawInput": {"pattern": "*"}}),
            json!({"title": "grep x", "kind": "search", "rawInput": {"pattern": "x"}}),
            json!({"title": "grep x", "kind": "search", "rawInput": {"path": d, "pattern": "x", "glob": "../**"}}),
            json!({"title": format!("Read {f}"), "kind": "edit", "rawInput": {"file_path": f}}),
            json!({"title": format!("Run cat {f}"), "kind": "read", "rawInput": {"file_path": f}}),
            json!({"title": format!("Read {f}"), "kind": "read", "rawInput": {"file_path": f}, "_meta": {"claudeCode": {"mcpServer": {"name": "x"}}}}),
            json!({"title": format!("Read {f}"), "kind": "read"}),
            json!({"title": format!("Read {f}"), "rawInput": {"file_path": f}}),
        ] {
            assert_eq!(read(refused.clone()), None, "{refused}");
        }
        let perm = json!({
            "jsonrpc": "2.0", "id": 9, "method": "session/request_permission",
            "params": {"sessionId": "s", "toolCall": {"toolCallId": "t", "title": format!("Read {f}"), "kind": "read", "rawInput": {"file_path": f}}, "options": []},
        });
        assert!(matches!(parse(&perm.to_string()), Some(Incoming::Permission { reads: Some(p), .. }) if p == file));
    }
}
