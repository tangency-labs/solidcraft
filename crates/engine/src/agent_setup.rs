//! `solidcraft-cli mcp setup` and Help > Connect AI assistants: add a `solidcraft` MCP server entry
//! to the AI clients installed on this machine, using the `solidcraft-cli` path given.
//!
//! Only the `solidcraft` entries change; every other byte (other servers, other settings, comments,
//! key order) stays as it was. Updates replace the file atomically, keeping the previous version
//! as `<name>.bak`. A file that cannot be read as JSON (with comments) or TOML is
//! left alone, and so is an entry the user changed (its command is not a SolidCraft CLI, or its
//! arguments differ); an entry that only has an old path to the CLI is updated.
//! A client counts as installed when its configuration folder exists; no client program is run.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// The address `--live` bridges to, the one docs/mcp.md starts the app on.
const LIVE_ADDR: &str = "127.0.0.1:7878";

/// Where the clients keep their configuration, read from the environment so tests can point it at a
/// temporary folder.
pub struct Env {
    pub home: Option<PathBuf>,
    pub appdata: Option<PathBuf>,
    pub local_appdata: Option<PathBuf>,
    pub xdg_config: Option<PathBuf>,
    pub codex_home: Option<PathBuf>,
}

impl Env {
    pub fn from_process() -> Env {
        let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let home = var("HOME").or_else(|| var("USERPROFILE"));
        Env { home, appdata: var("APPDATA"), local_appdata: var("LOCALAPPDATA"), xdg_config: var("XDG_CONFIG_HOME"), codex_home: var("CODEX_HOME") }
    }

    /// `$XDG_CONFIG_HOME`, else `~/.config`.
    fn config_home(&self) -> Option<PathBuf> {
        self.xdg_config.clone().or_else(|| self.home.as_ref().map(|h| h.join(".config")))
    }

    fn in_home(&self, rel: &str) -> Option<PathBuf> {
        self.home.as_ref().map(|h| h.join(rel))
    }
}

/// How a client spells a server entry and where its list is.
#[derive(Clone, Copy, PartialEq)]
enum Shape {
    /// `"mcpServers": {name: {"command": exe, "args": [...]}}` (Claude Desktop, Cursor, Gemini CLI, Windsurf)
    McpServers,
    /// Claude Code's `~/.claude.json`: the same with `"type": "stdio"`.
    ClaudeCode,
    /// GitHub Copilot CLI: the same with `"type": "local"` and `"tools": ["*"]`.
    Copilot,
    /// VS Code's user `mcp.json`: `"servers": {name: {"type": "stdio", ...}}`.
    VsCode,
    /// Zed's `settings.json`: `"context_servers": {name: {"command": exe, "args": [...]}}`.
    Zed,
    /// OpenCode: `"mcp": {name: {"type": "local", "command": [exe, ...], "enabled": true}}`.
    OpenCode,
    /// Codex: `[mcp_servers.name]` with `command` and `args`, in `config.toml`.
    Codex,
}

impl Shape {
    fn list_key(self) -> &'static str {
        match self {
            Shape::McpServers | Shape::ClaudeCode | Shape::Copilot => "mcpServers",
            Shape::VsCode => "servers",
            Shape::Zed => "context_servers",
            Shape::OpenCode => "mcp",
            Shape::Codex => "mcp_servers",
        }
    }

    fn entry(self, exe: &str, args: &[&str]) -> Value {
        match self {
            Shape::McpServers | Shape::Zed | Shape::Codex => json!({"command": exe, "args": args}),
            Shape::ClaudeCode | Shape::VsCode => json!({"type": "stdio", "command": exe, "args": args}),
            Shape::Copilot => json!({"type": "local", "command": exe, "args": args, "tools": ["*"]}),
            Shape::OpenCode => {
                let mut cmd = vec![exe];
                cmd.extend_from_slice(args);
                json!({"type": "local", "command": cmd, "enabled": true})
            }
        }
    }
}

struct Target {
    client: &'static str,
    file: PathBuf,
    shape: Shape,
}

/// The names `--client` takes.
pub const CLIENTS: &[&str] = &["opencode", "claude-desktop", "claude-code", "cursor", "codex", "gemini", "copilot", "windsurf", "zed", "vscode"];

/// The server entries to write: `solidcraft`, and `solidcraft-live` with `--live`.
fn servers(live: bool) -> Vec<(&'static str, Vec<&'static str>)> {
    let mut v = vec![("solidcraft", vec!["mcp"])];
    if live {
        v.push(("solidcraft-live", vec!["mcp", "--connect", LIVE_ADDR]));
    }
    v
}

/// The running executable's absolute path, without the `\\?\` prefix `canonicalize` adds on
/// Windows (some clients mishandle it).
pub fn current_exe() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find this executable: {e}"))?;
    Ok(plain_path(&exe.canonicalize().unwrap_or(exe)))
}

/// A path as text, without the `\\?\` prefix of a canonicalized Windows path.
pub fn plain_path(p: &Path) -> String {
    let s = p.to_string_lossy().into_owned();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => rest.to_owned(),
        _ => s,
    }
}

/// Claude Desktop's configuration folders: the regular one, and the Microsoft Store (MSIX)
/// package's redirected copy on Windows.
fn claude_desktop_dirs(env: &Env) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(a) = &env.appdata {
        dirs.push(a.join("Claude"));
    }
    if cfg!(target_os = "macos")
        && let Some(d) = env.in_home("Library/Application Support/Claude")
    {
        dirs.push(d);
    }
    if cfg!(target_os = "linux")
        && let Some(c) = env.config_home()
    {
        dirs.push(c.join("Claude"));
    }
    if let Some(l) = &env.local_appdata
        && let Ok(rd) = std::fs::read_dir(l.join("Packages"))
    {
        let mut pk: Vec<PathBuf> = rd
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("Claude_"))
            .map(|e| e.path().join("LocalCache/Roaming/Claude"))
            .collect();
        pk.sort();
        dirs.extend(pk);
    }
    dirs
}

/// The folder VS Code keeps its user settings in.
fn vscode_dir(env: &Env) -> Option<PathBuf> {
    if cfg!(windows) {
        env.appdata.as_ref().map(|a| a.join("Code/User"))
    } else if cfg!(target_os = "macos") {
        env.in_home("Library/Application Support/Code/User")
    } else {
        env.config_home().map(|c| c.join("Code/User"))
    }
}

/// The clients found on this machine: those whose configuration folder exists.
fn detect(env: &Env, only: Option<&str>) -> Vec<Target> {
    let want = |c: &str| only.is_none_or(|o| o == c);
    let mut v = Vec::new();
    fn add(v: &mut Vec<Target>, client: &'static str, dir: Option<PathBuf>, file: &str, shape: Shape) {
        if let Some(dir) = dir.filter(|d| d.is_dir()) {
            v.push(Target { client, file: dir.join(file), shape });
        }
    }
    if want("opencode") {
        let dir = env.config_home().map(|c| c.join("opencode"));
        // opencode.jsonc is read too (comments allowed); keep to the one the user has.
        let file = if dir.as_ref().is_some_and(|d| !d.join("opencode.json").exists() && d.join("opencode.jsonc").exists()) {
            "opencode.jsonc"
        } else {
            "opencode.json"
        };
        add(&mut v, "opencode", dir, file, Shape::OpenCode);
    }
    if want("claude-desktop") {
        for dir in claude_desktop_dirs(env) {
            add(&mut v, "claude-desktop", Some(dir), "claude_desktop_config.json", Shape::McpServers);
        }
    }
    if want("claude-code")
        && let Some(h) = &env.home
        && (h.join(".claude.json").is_file() || h.join(".claude").is_dir())
    {
        v.push(Target { client: "claude-code", file: h.join(".claude.json"), shape: Shape::ClaudeCode });
    }
    if want("cursor") {
        add(&mut v, "cursor", env.in_home(".cursor"), "mcp.json", Shape::McpServers);
    }
    if want("codex") {
        add(&mut v, "codex", env.codex_home.clone().or_else(|| env.in_home(".codex")), "config.toml", Shape::Codex);
    }
    if want("gemini") {
        add(&mut v, "gemini", env.in_home(".gemini"), "settings.json", Shape::McpServers);
    }
    if want("copilot") {
        add(&mut v, "copilot", env.in_home(".copilot"), "mcp-config.json", Shape::Copilot);
    }
    if want("windsurf") {
        add(&mut v, "windsurf", env.in_home(".codeium/windsurf"), "mcp_config.json", Shape::McpServers);
    }
    if want("zed") {
        let dir = if cfg!(windows) { env.appdata.as_ref().map(|a| a.join("Zed")) } else { env.config_home().map(|c| c.join("zed")) };
        add(&mut v, "zed", dir, "settings.json", Shape::Zed);
    }
    if want("vscode") {
        add(&mut v, "vscode", vscode_dir(env), "mcp.json", Shape::VsCode);
    }
    v
}

// ---------------------------------------------------------------------------------------------
// What an existing entry is.

/// What setup does about one server entry.
#[derive(Debug, PartialEq)]
enum Verdict {
    /// No entry: add it.
    Add,
    /// The entry is what setup would write.
    Same,
    /// The entry is ours with an older path to the CLI: update it.
    Stale,
    /// Something the user wrote or changed: leave it.
    Custom,
}

/// Whether `command` runs a SolidCraft CLI (under this name or the pre-rename one).
fn is_solidcraft_cli(command: &str) -> bool {
    let name = command.rsplit(['/', '\\']).next().unwrap_or(command).to_ascii_lowercase();
    let stem = name.strip_suffix(".exe").unwrap_or(&name);
    stem == "solidcraft-cli" || stem == "solvecraft-cli"
}

/// The command and arguments of an entry, whether it spells them as `command` + `args` or as
/// OpenCode's one `command` array.
fn command_and_args(entry: &Value) -> Option<(String, Vec<String>)> {
    let strings = |v: &Value| v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect::<Vec<_>>());
    match entry.get("command")? {
        Value::String(c) => Some((c.clone(), entry.get("args").and_then(strings).unwrap_or_default())),
        Value::Array(_) => {
            let mut all = strings(entry.get("command")?)?;
            (!all.is_empty()).then(|| (all.remove(0), all))
        }
        _ => None,
    }
}

fn judge(existing: Option<&Value>, exe: &str, args: &[&str]) -> Verdict {
    let Some(existing) = existing else { return Verdict::Add };
    let Some((cmd, got)) = command_and_args(existing) else { return Verdict::Custom };
    if got != args {
        return Verdict::Custom;
    }
    if cmd == exe {
        Verdict::Same
    } else if is_solidcraft_cli(&cmd) {
        Verdict::Stale
    } else {
        Verdict::Custom
    }
}

/// `existing` with its command replaced by `exe`, every other field kept.
fn with_command(existing: &Value, exe: &str) -> Value {
    let mut v = existing.clone();
    match v.get_mut("command") {
        Some(Value::Array(a)) if !a.is_empty() => a[0] = json!(exe),
        Some(c) => *c = json!(exe),
        None => {}
    }
    v
}

// ---------------------------------------------------------------------------------------------
// Editing JSON (with comments and trailing commas, as VS Code, Zed and OpenCode accept) in place.

/// One member of an object in the text: its key and the byte range of its value.
struct Member {
    key: String,
    key_at: usize,
    val: (usize, usize),
}

/// An object in the text: the offsets of its braces and its members.
struct Obj {
    open: usize,
    close: usize,
    members: Vec<Member>,
}

fn skip_ws(b: &[u8], mut i: usize) -> Result<usize, String> {
    loop {
        match b.get(i) {
            Some(c) if c.is_ascii_whitespace() => i += 1,
            Some(b'/') if b.get(i + 1) == Some(&b'/') => {
                while b.get(i).is_some_and(|&c| c != b'\n') {
                    i += 1;
                }
            }
            Some(b'/') if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while !(b.get(i) == Some(&b'*') && b.get(i + 1) == Some(&b'/')) {
                    if i >= b.len() {
                        return Err("an unterminated comment".into());
                    }
                    i += 1;
                }
                i += 2;
            }
            _ => return Ok(i),
        }
    }
}

fn scan_string(b: &[u8], mut i: usize) -> Result<usize, String> {
    i += 1;
    while let Some(&c) = b.get(i) {
        match c {
            b'\\' => i += 2,
            b'"' => return Ok(i + 1),
            _ => i += 1,
        }
    }
    Err("an unterminated string".into())
}

/// The end of the value starting at `i`.
fn scan_value(b: &[u8], i: usize, depth: usize) -> Result<usize, String> {
    if depth > 64 {
        return Err("nesting too deep".into());
    }
    match b.get(i) {
        Some(b'"') => scan_string(b, i),
        Some(b'{') => Ok(scan_object(b, i, depth)?.close + 1),
        Some(b'[') => {
            let mut j = skip_ws(b, i + 1)?;
            loop {
                match b.get(j) {
                    Some(b']') => return Ok(j + 1),
                    Some(b',') => j = skip_ws(b, j + 1)?,
                    Some(_) => j = skip_ws(b, scan_value(b, j, depth + 1)?)?,
                    None => return Err("an unterminated array".into()),
                }
            }
        }
        Some(c) if !matches!(c, b',' | b'}' | b']' | b':') => {
            let mut j = i;
            while b.get(j).is_some_and(|c| !c.is_ascii_whitespace() && !matches!(c, b',' | b'}' | b']' | b'/')) {
                j += 1;
            }
            Ok(j)
        }
        _ => Err(format!("a value was expected at byte {i}")),
    }
}

fn scan_object(b: &[u8], open: usize, depth: usize) -> Result<Obj, String> {
    let mut members = Vec::new();
    let mut i = skip_ws(b, open + 1)?;
    loop {
        match b.get(i) {
            Some(b'}') => return Ok(Obj { open, close: i, members }),
            Some(b',') => i = skip_ws(b, i + 1)?,
            Some(b'"') => {
                let end = scan_string(b, i)?;
                let key: String = serde_json::from_slice(b.get(i..end).unwrap_or_default()).map_err(|e| e.to_string())?;
                let colon = skip_ws(b, end)?;
                if b.get(colon) != Some(&b':') {
                    return Err(format!("a `:` was expected at byte {colon}"));
                }
                let vs = skip_ws(b, colon + 1)?;
                let ve = scan_value(b, vs, depth + 1)?;
                members.push(Member { key, key_at: i, val: (vs, ve) });
                i = skip_ws(b, ve)?;
            }
            _ => return Err(format!("a key or `}}` was expected at byte {i}")),
        }
    }
}

/// The top-level object of a JSON document, with the byte after it checked to hold nothing else.
fn root_object(text: &str) -> Result<Obj, String> {
    let b = text.as_bytes();
    let i = skip_ws(b, 0)?;
    if b.get(i) != Some(&b'{') {
        return Err("the top level is not a JSON object".into());
    }
    let obj = scan_object(b, i, 0)?;
    if skip_ws(b, obj.close + 1)? != b.len() {
        return Err("text after the top-level object".into());
    }
    Ok(obj)
}

/// The indentation of the line holding byte `at`.
fn line_indent(text: &str, at: usize) -> String {
    let start = text.get(..at).and_then(|t| t.rfind('\n')).map_or(0, |n| n + 1);
    text.get(start..).unwrap_or_default().chars().take_while(|c| *c == ' ' || *c == '\t').collect()
}

/// `value` pretty-printed, its lines after the first indented by `indent`.
fn pretty_at(value: &Value, indent: &str) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default().replace('\n', &format!("\n{indent}"))
}

/// Insert `"key": value` as the last member of `obj`.
fn insert_member(text: &mut String, obj: &Obj, key: &str, value: &Value) {
    let closing_indent = line_indent(text, obj.open);
    let member_indent = match obj.members.first() {
        Some(m) => line_indent(text, m.key_at),
        None => format!("{closing_indent}  "),
    };
    let k = serde_json::to_string(key).unwrap_or_default();
    let member = format!("{k}: {}", pretty_at(value, &member_indent));
    match obj.members.last() {
        Some(last) => text.insert_str(last.val.1, &format!(",\n{member_indent}{member}")),
        None => {
            let inner = format!("\n{member_indent}{member}\n{closing_indent}");
            text.replace_range(obj.open + 1..obj.close, &inner);
        }
    }
}

/// Delete member `i` of `obj` from the text, with its comma.
fn remove_member(text: &mut String, obj: &Obj, i: usize) {
    let Some(m) = obj.members.get(i) else { return };
    if let Some(next) = obj.members.get(i + 1) {
        text.replace_range(m.key_at..next.key_at, "");
    } else if i > 0
        && let Some(prev) = obj.members.get(i - 1)
    {
        text.replace_range(prev.val.1..m.val.1, "");
    } else {
        text.replace_range(obj.open + 1..obj.close, "");
    }
}

/// What happened to one server entry.
#[derive(Debug, PartialEq)]
enum Change {
    Added,
    Updated,
    Unchanged,
    /// Taken out (a SolidCraft entry of ours).
    Removed,
    /// Not touched, because the user's own entry is there.
    Custom,
}

/// What a run does with the entries.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Add,
    /// Take out the entries setup wrote (the user's own are left).
    Remove,
}

/// Set the entries in the JSON text, in the object under the top-level `list_key`.
fn edit_json(text: &mut String, shape: Shape, exe: &str, live: bool, mode: Mode) -> Result<Vec<(&'static str, Change)>, String> {
    let mut out = Vec::new();
    let key = shape.list_key();
    if text.trim().is_empty() {
        *text = "{}\n".into();
    }
    let root = root_object(text)?;
    // The list object: found, or created holding nothing yet.
    let list_open = match root.members.iter().find(|m| m.key == key) {
        Some(m) if text.as_bytes().get(m.val.0) == Some(&b'{') => m.val.0,
        Some(_) => return Err(format!("`{key}` is not a JSON object")),
        None if mode == Mode::Remove => return Ok(Vec::new()),
        None => {
            insert_member(text, &root, key, &json!({}));
            let root = root_object(text)?;
            root.members.iter().find(|m| m.key == key).map_or(0, |m| m.val.0)
        }
    };
    for (name, args) in servers(live) {
        let new = shape.entry(exe, &args);
        let obj = scan_object(text.as_bytes(), list_open, 0)?;
        let existing = obj.members.iter().find(|m| m.key == name);
        let parsed = existing.and_then(|m| serde_json::from_str::<Value>(text.get(m.val.0..m.val.1).unwrap_or_default()).ok());
        // An entry that is not even valid JSON (a comment inside it) is the user's.
        let verdict = match (existing, &parsed) {
            (Some(_), None) => Verdict::Custom,
            _ => judge(parsed.as_ref(), exe, &args),
        };
        let change = match (verdict, existing) {
            (Verdict::Add, _) if mode == Mode::Remove => continue,
            (Verdict::Same | Verdict::Stale, Some(_)) if mode == Mode::Remove => {
                let at = obj.members.iter().position(|m| m.key == name).unwrap_or(0);
                remove_member(text, &obj, at);
                Change::Removed
            }
            (Verdict::Add, _) => {
                insert_member(text, &obj, name, &new);
                Change::Added
            }
            (Verdict::Stale, Some(m)) => {
                let updated = with_command(parsed.as_ref().unwrap_or(&new), exe);
                let indent = line_indent(text, m.val.0);
                text.replace_range(m.val.0..m.val.1, &pretty_at(&updated, &indent));
                Change::Updated
            }
            (Verdict::Same, _) => Change::Unchanged,
            _ => Change::Custom,
        };
        out.push((name, change));
    }
    if !text.ends_with('\n') {
        text.push('\n');
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Editing Codex's TOML in place.

fn edit_toml(text: &mut String, exe: &str, live: bool, mode: Mode) -> Result<Vec<(&'static str, Change)>, String> {
    use toml_edit::{Array, DocumentMut, Item, Table, value};
    let mut doc: DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| e.to_string())?;
    let mut out = Vec::new();
    if doc.get("mcp_servers").is_some_and(|i| !i.is_table()) {
        return Err("`mcp_servers` is not a table".into());
    }
    for (name, args) in servers(live) {
        let existing = doc
            .get("mcp_servers")
            .and_then(|t| t.get(name))
            .map(|i| i.as_table().map(toml_to_json).or_else(|| i.as_inline_table().map(|t| toml_to_json(&t.clone().into_table()))));
        let existing = match existing {
            Some(Some(v)) => Some(v),
            Some(None) => {
                out.push((name, Change::Custom));
                continue;
            }
            None => None,
        };
        let change = match judge(existing.as_ref(), exe, &args) {
            Verdict::Add if mode == Mode::Remove => continue,
            Verdict::Same | Verdict::Stale if mode == Mode::Remove => {
                if let Some(t) = doc.get_mut("mcp_servers").and_then(Item::as_table_mut) {
                    t.remove(name);
                }
                Change::Removed
            }
            Verdict::Add => {
                let mut t = Table::new();
                t["command"] = value(exe);
                let mut a = Array::new();
                for x in &args {
                    a.push(*x);
                }
                t["args"] = value(a);
                let servers = doc.entry("mcp_servers").or_insert_with(|| {
                    let mut t = Table::new();
                    t.set_implicit(true);
                    Item::Table(t)
                });
                if let Some(s) = servers.as_table_mut() {
                    s.insert(name, Item::Table(t));
                }
                Change::Added
            }
            Verdict::Stale => {
                if let Some(entry) = doc.get_mut("mcp_servers").and_then(|t| t.get_mut(name)) {
                    entry["command"] = value(exe);
                }
                Change::Updated
            }
            Verdict::Same => Change::Unchanged,
            Verdict::Custom => Change::Custom,
        };
        out.push((name, change));
    }
    *text = doc.to_string();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    Ok(out)
}

/// The `command` and `args` of a TOML table, as JSON for `judge`.
fn toml_to_json(t: &toml_edit::Table) -> Value {
    let command = t.get("command").and_then(|c| c.as_str()).map(|c| json!(c));
    let args = t.get("args").and_then(|a| a.as_array()).map(|a| Value::Array(a.iter().filter_map(|x| x.as_str().map(|s| json!(s))).collect()));
    let mut m = serde_json::Map::new();
    if let Some(c) = command {
        m.insert("command".into(), c);
    }
    if let Some(a) = args {
        m.insert("args".into(), a);
    }
    Value::Object(m)
}

// ---------------------------------------------------------------------------------------------

/// One client's outcome, a line of the report.
fn configure_file(t: &Target, exe: &str, live: bool, dry_run: bool, mode: Mode) -> Result<(String, Vec<(&'static str, Change)>), String> {
    let fail = |e: String| format!("{}: {e}; left unchanged", t.file.display());
    let existing = read_config(&t.file).map_err(|e| format!("{}: {e}", t.file.display()))?;
    let mut text = existing.clone().unwrap_or_default();
    let changes =
        if t.shape == Shape::Codex { edit_toml(&mut text, exe, live, mode) } else { edit_json(&mut text, t.shape, exe, live, mode) }.map_err(fail)?;
    let said: Vec<String> = changes
        .iter()
        .map(|(name, c)| match c {
            Change::Added => format!("added {name}"),
            Change::Updated => format!("updated the path of {name}"),
            Change::Unchanged => format!("{name} already set up"),
            Change::Removed => format!("removed {name}"),
            Change::Custom => format!("{name} left as you have it (it is not a plain SolidCraft entry)"),
        })
        .collect();
    let said = said.join(", ");
    let changed = existing.as_deref() != Some(text.as_str());
    let done = |m: String| Ok((m, changes));
    if dry_run {
        return done(if changed { format!("would write {} ({said}):\n{text}", t.file.display()) } else { format!("no change needed ({said})") });
    }
    if !changed {
        return done(said);
    }
    write_config(&t.file, existing.as_deref(), &text).map_err(|e| format!("{}: {e}", t.file.display()))?;
    done(format!("{said} ({})", t.file.display()))
}

fn read_config(path: &Path) -> std::io::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Refuse a stale edit before writing. Other clients do not share a lock with SolidCraft, so
/// this narrows the race with their writes; it cannot exclude a write after the comparison.
fn write_config(path: &Path, expected: Option<&str>, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if read_config(path)?.as_deref() != expected {
        return Err(std::io::Error::other("configuration changed during setup; left unchanged, try again"));
    }
    solidcraft_io::safe_write::write_atomic(path, text.as_bytes(), true)
}

/// Set up every detected client (or `--client NAME`; flags `--dry-run`, `--live`) for the
/// `solidcraft-cli` at `exe`. Returns the result lines, one per client file, and whether every
/// client succeeded.
pub fn setup(args: &[String], env: &Env, exe: &str) -> Result<(Vec<String>, bool), String> {
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let live = args.iter().any(|a| a == "--live");
    let mut only = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dry-run" | "--live" => {}
            "--client" => {
                let c = it.next().ok_or("mcp setup: --client wants a name")?;
                if !CLIENTS.contains(&c.as_str()) {
                    return Err(format!("mcp setup: unknown client `{c}`; one of {}", CLIENTS.join(", ")));
                }
                only = Some(c.as_str());
            }
            other => return Err(format!("mcp setup: unknown option `{other}`")),
        }
    }
    let mut lines = Vec::new();
    let mut ok = true;
    for t in detect(env, only) {
        match configure_file(&t, exe, live, dry_run, Mode::Add) {
            Ok((m, _)) => lines.push(format!("{}: {m}", t.client)),
            Err(e) => {
                ok = false;
                lines.push(format!("{}: failed: {e}", t.client));
            }
        }
    }
    if lines.is_empty() {
        lines.push(match only {
            Some(c) => format!("{c}: not found on this machine"),
            None => format!("no supported client found ({}); see docs/mcp.md for manual setup", CLIENTS.join(", ")),
        });
    }
    Ok((lines, ok))
}

/// What a quiet run did: the clients that got a new entry, and the ones that took one out.
#[derive(Debug, Default, PartialEq)]
pub struct Quiet {
    pub added: Vec<&'static str>,
    pub removed: Vec<&'static str>,
}

/// Make `solidcraft-cli` at `exe` available to every installed client (`Mode::Add`), or take
/// the entries setup wrote out of them again (`remove`). Quiet and best effort, for the app: a
/// client that fails is skipped. The rules for the user's own entries are the same as `setup`'s.
pub fn quiet(env: &Env, exe: &str, remove: bool) -> Quiet {
    let mode = if remove { Mode::Remove } else { Mode::Add };
    let mut q = Quiet::default();
    for t in detect(env, None) {
        // Adding writes `solidcraft` only (plain `mcp` finds the app by itself); removing also
        // takes out a `solidcraft-live` that is ours.
        let Ok((_, changes)) = configure_file(&t, exe, remove, false, mode) else { continue };
        if changes.iter().any(|(n, c)| *n == "solidcraft" && *c == Change::Added) {
            q.added.push(t.client);
        }
        if changes.iter().any(|(_, c)| *c == Change::Removed) {
            q.removed.push(t.client);
        }
    }
    q
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str = "/opt/solidcraft/solidcraft-cli";

    fn edit(text: &str, shape: Shape) -> (String, Vec<(&'static str, Change)>) {
        let mut t = text.to_owned();
        let c = edit_json(&mut t, shape, EXE, false, Mode::Add).unwrap();
        (t, c)
    }

    #[test]
    fn adds_to_an_empty_file_and_keeps_everything_else_byte_for_byte() {
        let (t, c) = edit("", Shape::McpServers);
        assert_eq!(c, [("solidcraft", Change::Added)]);
        let v: Value = serde_json::from_str(&t).unwrap();
        assert_eq!(v["mcpServers"]["solidcraft"], json!({"command": EXE, "args": ["mcp"]}));

        let src = "{\n    \"zeta\": 1, // keep\n    \"mcpServers\": {\n        \"other\": {\"command\": \"o\"}\n    },\n    \"alpha\": [1,2]\n}\n";
        let (t, _) = edit(src, Shape::McpServers);
        // Only the new member was inserted: the original text is intact around it.
        assert!(
            t.starts_with("{\n    \"zeta\": 1, // keep\n    \"mcpServers\": {\n        \"other\": {\"command\": \"o\"},\n        \"solidcraft\": {"),
            "{t}"
        );
        assert!(t.ends_with("    },\n    \"alpha\": [1,2]\n}\n"), "{t}");
        let v: Value = serde_json::from_str(t.replace("// keep", "").as_str()).unwrap();
        assert_eq!(v["mcpServers"]["other"], json!({"command": "o"}));
    }

    #[test]
    fn a_file_with_comments_and_a_missing_list_gets_the_list() {
        let (t, _) = edit("// my settings\n{\n  \"theme\": \"x\", /* c */\n}\n", Shape::Zed);
        assert!(t.contains("// my settings") && t.contains("/* c */"), "{t}");
        assert!(t.contains("\"context_servers\""), "{t}");
    }

    #[test]
    fn stale_path_is_updated_and_custom_entries_are_not() {
        let old = r#"{"mcpServers": {"solidcraft": {"command": "C:\\Old\\solidcraft-cli.exe", "args": ["mcp"], "env": {"A": "1"}}}}"#;
        let (t, c) = edit(old, Shape::McpServers);
        assert_eq!(c, [("solidcraft", Change::Updated)]);
        let v: Value = serde_json::from_str(&t).unwrap();
        assert_eq!(v["mcpServers"]["solidcraft"]["command"], EXE);
        assert_eq!(v["mcpServers"]["solidcraft"]["env"], json!({"A": "1"}), "other fields stay");

        // The user added an argument: theirs.
        let custom = r#"{"mcpServers": {"solidcraft": {"command": "/x/solidcraft-cli", "args": ["mcp", "--allow-any-path"]}}}"#;
        let (t, c) = edit(custom, Shape::McpServers);
        assert_eq!(c, [("solidcraft", Change::Custom)]);
        assert_eq!(t.trim_end(), custom);
        // Another program under that name: theirs.
        let other = r#"{"mcpServers": {"solidcraft": {"command": "node", "args": ["mcp"]}}}"#;
        assert_eq!(edit(other, Shape::McpServers).1, [("solidcraft", Change::Custom)]);
        // Already right: the text is untouched.
        let same = format!(r#"{{"mcpServers": {{"solidcraft": {{"command": "{EXE}", "args": ["mcp"]}}}}}}"#);
        let (t, c) = edit(&same, Shape::McpServers);
        assert_eq!((t.trim_end(), c), (same.as_str(), vec![("solidcraft", Change::Unchanged)]));
    }

    #[test]
    fn opencode_entries_use_one_command_array() {
        let (t, _) = edit("{\"mcp\": {}}", Shape::OpenCode);
        let v: Value = serde_json::from_str(&t).unwrap();
        assert_eq!(v["mcp"]["solidcraft"], json!({"type": "local", "command": [EXE, "mcp"], "enabled": true}));
        let stale = r#"{"mcp": {"solidcraft": {"type": "local", "command": ["/old/solidcraft-cli", "mcp"], "enabled": false}}}"#;
        let (t, c) = edit(stale, Shape::OpenCode);
        assert_eq!(c, [("solidcraft", Change::Updated)]);
        let v: Value = serde_json::from_str(&t).unwrap();
        assert_eq!(v["mcp"]["solidcraft"]["command"], json!([EXE, "mcp"]));
        assert_eq!(v["mcp"]["solidcraft"]["enabled"], false);
    }

    #[test]
    fn broken_json_is_an_error() {
        for bad in ["[1]", "{\"a\": ", "{\"a\": 1} x", "{\"mcpServers\": 3}"] {
            assert!(edit_json(&mut bad.to_owned(), Shape::McpServers, EXE, false, Mode::Add).is_err(), "{bad}");
        }
    }

    #[test]
    fn codex_toml_keeps_comments_and_other_tables() {
        let src = "# my codex\nmodel = \"x\"\n\n[mcp_servers.other]\ncommand = \"o\" # mine\n";
        let mut t = src.to_owned();
        let c = edit_toml(&mut t, EXE, true, Mode::Add).unwrap();
        assert_eq!(c, [("solidcraft", Change::Added), ("solidcraft-live", Change::Added)]);
        assert!(t.starts_with(src), "{t}");
        assert!(t.contains("[mcp_servers.solidcraft]") && t.contains(&format!("command = \"{EXE}\"")), "{t}");
        // Stale path, then a user-changed entry.
        let mut stale = "[mcp_servers.solidcraft]\ncommand = \"/old/solidcraft-cli\"\nargs = [\"mcp\"]\nstartup_timeout_sec = 30\n".to_owned();
        assert_eq!(edit_toml(&mut stale, EXE, false, Mode::Add).unwrap(), [("solidcraft", Change::Updated)]);
        assert!(stale.contains("startup_timeout_sec = 30") && stale.contains(EXE), "{stale}");
        let mut custom = "[mcp_servers.solidcraft]\ncommand = \"/x/solidcraft-cli\"\nargs = [\"mcp\", \"--headless\"]\n".to_owned();
        let before = custom.clone();
        assert_eq!(edit_toml(&mut custom, EXE, false, Mode::Add).unwrap(), [("solidcraft", Change::Custom)]);
        assert_eq!(custom, before);
        // An empty file works too.
        let mut empty = String::new();
        edit_toml(&mut empty, EXE, false, Mode::Add).unwrap();
        assert!(empty.contains("[mcp_servers.solidcraft]"), "{empty}");
    }

    #[test]
    fn removing_takes_out_only_our_entries_and_keeps_the_rest() {
        let src = "{\n  \"mcpServers\": {\n    \"a\": {\"command\": \"x\"},\n    \"solidcraft\": {\"command\": \"/old/solidcraft-cli\", \"args\": [\"mcp\"]},\n    \"solidcraft-live\": {\"command\": \"/old/solidcraft-cli\", \"args\": [\"mcp\", \"--connect\", \"127.0.0.1:7878\"]}\n  },\n  \"k\": 1\n}\n";
        let mut t = src.to_owned();
        let c = edit_json(&mut t, Shape::McpServers, EXE, true, Mode::Remove).unwrap();
        assert_eq!(c, [("solidcraft", Change::Removed), ("solidcraft-live", Change::Removed)]);
        let v: Value = serde_json::from_str(&t).unwrap();
        assert_eq!(v, json!({"mcpServers": {"a": {"command": "x"}}, "k": 1}));
        // The user's own entry stays.
        let mut mine = r#"{"mcpServers": {"solidcraft": {"command": "node", "args": ["mcp"]}}}"#.to_owned();
        let before = mine.clone();
        assert_eq!(edit_json(&mut mine, Shape::McpServers, EXE, true, Mode::Remove).unwrap(), [("solidcraft", Change::Custom)]);
        assert_eq!(mine.trim_end(), before);
        // Nothing there: nothing happens, no list is created.
        let mut none = "{\"k\": 1}".to_owned();
        assert!(edit_json(&mut none, Shape::McpServers, EXE, true, Mode::Remove).unwrap().is_empty());
        assert_eq!(none.trim_end(), "{\"k\": 1}");
        // TOML.
        let mut toml =
            "[mcp_servers.other]\ncommand = \"o\"\n\n[mcp_servers.solidcraft]\ncommand = \"/old/solidcraft-cli\"\nargs = [\"mcp\"]\n".to_owned();
        assert_eq!(edit_toml(&mut toml, EXE, false, Mode::Remove).unwrap(), [("solidcraft", Change::Removed)]);
        assert!(toml.contains("[mcp_servers.other]") && !toml.contains("solidcraft"), "{toml}");
    }

    fn config_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("solidcraft-agent-config-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn configuration_backup_tracks_the_file_before_each_change() {
        let dir = config_dir("backup");
        let target = Target { client: "claude-code", file: dir.join("config.json"), shape: Shape::ClaudeCode };
        let backup = dir.join("config.json.bak");
        let original = "{\n  \"personal_setting\": true\n}\n";
        std::fs::write(&target.file, original).unwrap();
        configure_file(&target, EXE, false, false, Mode::Add).unwrap();
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), original);
        let changed_by_client = std::fs::read_to_string(&target.file).unwrap().replace("true", "false");
        std::fs::write(&target.file, &changed_by_client).unwrap();
        configure_file(&target, "/new/solidcraft-cli", false, false, Mode::Add).unwrap();
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), changed_by_client);
        let current = std::fs::read_to_string(&target.file).unwrap();
        assert!(current.contains("false") && current.contains("/new/solidcraft-cli"));
        // A no-op or dry run must leave both the file and its backup alone.
        configure_file(&target, "/new/solidcraft-cli", false, false, Mode::Add).unwrap();
        configure_file(&target, "/future/solidcraft-cli", false, true, Mode::Add).unwrap();
        assert_eq!(std::fs::read_to_string(&target.file).unwrap(), current);
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), changed_by_client);
        configure_file(&target, "/new/solidcraft-cli", false, false, Mode::Remove).unwrap();
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), current);
        assert!(!std::fs::read_to_string(&target.file).unwrap().contains("solidcraft"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn configuration_is_unchanged_when_its_backup_cannot_be_replaced() {
        let dir = config_dir("backup-failure");
        let target = Target { client: "codex", file: dir.join("config.toml"), shape: Shape::Codex };
        let original = "# personal setting\nmodel = \"custom\"\n";
        std::fs::write(&target.file, original).unwrap();
        std::fs::create_dir(dir.join("config.toml.bak")).unwrap();
        assert!(configure_file(&target, EXE, false, false, Mode::Add).is_err());
        assert_eq!(std::fs::read_to_string(&target.file).unwrap(), original);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2, "no temporary files remain");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn configuration_refuses_an_edit_based_on_an_outdated_read() {
        let dir = config_dir("concurrent");
        let path = dir.join("config.json");
        let backup = dir.join("config.json.bak");
        std::fs::write(&backup, "previous backup").unwrap();
        for (expected, current) in [(Some("old"), Some("new")), (Some("old"), None), (None, Some("new"))] {
            if let Some(text) = current {
                std::fs::write(&path, text).unwrap();
            } else {
                std::fs::remove_file(&path).unwrap();
            }
            let error = write_config(&path, expected, "stale edit").unwrap_err();
            assert!(error.to_string().contains("configuration changed"), "{error}");
            assert_eq!(read_config(&path).unwrap().as_deref(), current);
            assert_eq!(std::fs::read_to_string(&backup).unwrap(), "previous backup");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn configuration_replaces_a_complete_file_and_keeps_links_and_permissions() {
        use std::io::Read;
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = config_dir("atomic");
        let real = dir.join("real.json");
        let target = Target { client: "claude-code", file: dir.join("config.json"), shape: Shape::ClaudeCode };
        let original = "{\"personal_setting\": true}\n";
        std::fs::write(&real, original).unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
        symlink("real.json", &target.file).unwrap();
        let mut reader = std::fs::File::open(&target.file).unwrap();
        configure_file(&target, EXE, false, false, Mode::Add).unwrap();
        let mut old = String::new();
        reader.read_to_string(&mut old).unwrap();
        assert_eq!(old, original, "an existing reader sees the complete old file");
        assert!(std::fs::read_to_string(&target.file).unwrap().contains(EXE));
        assert!(std::fs::symlink_metadata(&target.file).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::metadata(&real).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::read_to_string(dir.join("real.json.bak")).unwrap(), original);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn quiet_adds_only_the_plain_entry_and_names_new_clients_once() {
        let h = std::env::temp_dir().join(format!("solidcraft-quiet-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&h);
        std::fs::create_dir_all(h.join(".cursor")).unwrap();
        std::fs::create_dir_all(h.join(".gemini")).unwrap();
        let env = Env { home: Some(h.clone()), appdata: None, local_appdata: None, xdg_config: Some(h.join("xdg")), codex_home: None };
        let q = quiet(&env, EXE, false);
        assert_eq!(q.added, ["cursor", "gemini"]);
        let t = std::fs::read_to_string(h.join(".cursor/mcp.json")).unwrap();
        assert!(t.contains("\"solidcraft\"") && !t.contains("solidcraft-live"), "{t}");
        // A second run changes nothing and names nobody.
        assert_eq!(quiet(&env, EXE, false), Quiet::default());
        // An old path is refreshed without being called new.
        std::fs::write(h.join(".cursor/mcp.json"), t.replace(EXE, "/gone/solidcraft-cli")).unwrap();
        assert_eq!(quiet(&env, EXE, false), Quiet::default());
        assert!(std::fs::read_to_string(h.join(".cursor/mcp.json")).unwrap().contains(EXE));
        // Turning it off takes the entries out again.
        assert_eq!(quiet(&env, EXE, true).removed, ["cursor", "gemini"]);
        assert!(!std::fs::read_to_string(h.join(".cursor/mcp.json")).unwrap().contains("solidcraft"));
        let _ = std::fs::remove_dir_all(&h);
    }
}
