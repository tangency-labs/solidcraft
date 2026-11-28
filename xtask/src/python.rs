//! `cargo xtask python [--cli PATH] [--out DIR]`: the Python client's generated bindings
//! (`python/src/solidcraft/_generated/`, not committed; docs/python-api.md).
//!
//! `cargo xtask python wheel --platform TAG --version VERSION [--cli PATH] [--out DIR]`: a wheel of
//! the package as it stands (generate the bindings first), with `solidcraft-cli` from `--cli` under
//! `solidcraft/bin/` for a platform wheel, or none for `--platform any`. The release workflow runs
//! it on each platform's own released binary (`packaging/python/wheels.sh`), so a wheel carries
//! the bytes `SHA256SUMS.txt` lists, signed where the release is.
//!
//! Two listings, both from the build, are the only input, so the bindings cannot drift from what
//! the program does:
//!
//! - `solidcraft-cli commands --schemas`: every engine command becomes a method under
//!   `Session.cmd` (`d.cmd.solid.box(...)`), which sends MCP's `execute`;
//! - `tools/list` from `solidcraft-cli mcp --headless`: every MCP tool becomes a method of the
//!   session (`d.measure(...)`), so a tool added to the server appears with the next build.
//!
//! Names: ids split on `.`; a segment or parameter that is a Python keyword gets a trailing `_`
//! (`from_`), other characters that are not allowed become `_`. Two names that map to the same
//! identifier stop the generator; nothing is dropped silently. Planned commands are left out.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

/// Python keywords and soft keywords that cannot name a parameter or attribute.
const KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except",
    "finally", "for", "from", "global", "if", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield",
];

/// Names the hand-written `Session` uses; a tool of the same name would hide one.
const SESSION_NAMES: &[&str] = &["tool", "call", "cmd", "changes", "close", "find", "describe", "server_version", "mark"];

/// Named `$defs` the command contracts share, and the alias `_types.py` gives each.
const ALIASES: &[(&str, &str)] = &[
    ("LengthExpr", "Length"),
    ("AngleExpr", "Angle"),
    ("Expr", "Length"),
    ("PointExpr3", "Point3"),
    ("PointExpr2", "Point2"),
    ("Vec3", "Vec3"),
    ("Vec2", "Point2"),
    ("StringList", "Names"),
];

pub fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    if args.first() == Some(&"wheel") {
        return wheel(root, &args[1..]);
    }
    let mut cli: Option<PathBuf> = None;
    let mut out = root.join("python/src/solidcraft/_generated");
    let mut i = 0;
    while let Some(a) = args.get(i) {
        match *a {
            "--cli" => cli = Some(PathBuf::from(args.get(i + 1).ok_or("python: --cli needs a path")?)),
            "--out" => out = PathBuf::from(args.get(i + 1).ok_or("python: --out needs a folder")?),
            other => return Err(format!("python: unknown argument `{other}` (it takes --cli PATH, --out DIR)")),
        }
        i += 2;
    }
    let cli = match cli {
        Some(c) => c,
        None => build_cli(root)?,
    };
    let commands = cli_json(&cli, &["commands", "--schemas"])?;
    let commands = commands.as_array().ok_or("python: `commands --schemas` is not a list")?;
    let (version, tools) = mcp_tools(&cli)?;
    let files = generate(commands, &tools, &version)?;
    std::fs::create_dir_all(&out).map_err(|e| format!("python: {}: {e}", out.display()))?;
    for (name, text) in &files {
        std::fs::write(out.join(name), text).map_err(|e| format!("python: {name}: {e}"))?;
    }
    let runnable = commands.iter().filter(|c| !is_planned(c)).count();
    println!("python: {runnable} commands, {} tools, version {version} → {}", tools.len(), out.display());
    Ok(())
}

/// Builds `solidcraft-cli` with the `ci` profile, as the fast gate does, and returns its path.
pub(crate) fn build_cli(root: &Path) -> Result<PathBuf, String> {
    let mut build = crate::cargo();
    build.args(["build", "-q", "--profile", "ci", "-p", "solidcraft-cli"]);
    crate::run(build, "cargo build --profile ci -p solidcraft-cli")?;
    let target = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| root.join("target"));
    Ok(target.join("ci").join(if cfg!(windows) { "solidcraft-cli.exe" } else { "solidcraft-cli" }))
}

pub(crate) fn cli_json(cli: &Path, args: &[&str]) -> Result<Value, String> {
    let out = Command::new(cli).args(args).output().map_err(|e| format!("python: {}: {e}", cli.display()))?;
    if !out.status.success() {
        return Err(format!("python: solidcraft-cli {} failed:\n{}", args.join(" "), String::from_utf8_lossy(&out.stderr)));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("python: solidcraft-cli {}: {e}", args.join(" ")))
}

/// The server's version and its `tools/list`, from a headless MCP server.
fn mcp_tools(cli: &Path) -> Result<(String, Vec<Value>), String> {
    let mut child = Command::new(cli)
        .args(["mcp", "--headless"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("python: {} mcp: {e}", cli.display()))?;
    let lines = [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "xtask-python", "version": "0"}}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    ];
    {
        let mut stdin = child.stdin.take().ok_or("python: mcp has no stdin")?;
        for l in &lines {
            writeln!(stdin, "{l}").map_err(|e| format!("python: mcp stdin: {e}"))?;
        }
    }
    let stdout = child.stdout.take().ok_or("python: mcp has no stdout")?;
    let (mut version, mut tools) = (None, None);
    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|e| format!("python: mcp stdout: {e}"))?;
        let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
        match msg["id"].as_u64() {
            Some(1) => version = msg["result"]["serverInfo"]["version"].as_str().map(str::to_string),
            Some(2) => tools = msg["result"]["tools"].as_array().cloned(),
            _ => {}
        }
    }
    let _ = child.wait();
    Ok((version.ok_or("python: mcp sent no version")?, tools.ok_or("python: mcp sent no tools/list")?))
}

pub(crate) fn is_planned(c: &Value) -> bool {
    !c["planned"].is_null() && c["planned"] != json!(false)
}

/// A Python identifier for `name`, or an error naming it.
fn ident(name: &str) -> Result<String, String> {
    let mut s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
    if s.is_empty() {
        return Err(format!("python: `{name}` has no usable characters"));
    }
    if s.starts_with(|c: char| c.is_ascii_digit()) {
        s.insert(0, '_');
    }
    if KEYWORDS.contains(&s.as_str()) {
        s.push('_');
    }
    Ok(s)
}

/// A Python string literal (double-quoted, escaped).
fn lit(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// A docstring body: no `"""`, no backslash escapes, lines indented by `pad`.
fn doc(text: &str, pad: &str) -> String {
    let clean = text.replace('\\', "\\\\").replace("\"\"\"", "'''");
    clean.lines().map(|l| if l.trim().is_empty() { String::new() } else { format!("{pad}{}", l.trim_end()) }).collect::<Vec<_>>().join("\n")
}

/// The Python annotation for a JSON Schema, read against `defs`. Deliberately shallow: the
/// engine checks parameters; the annotation is for people and models reading the stubs.
fn annotation(s: &Value, defs: &Value, depth: u32) -> String {
    if depth > 4 {
        return "Any".into();
    }
    if let Some(r) = s["$ref"].as_str() {
        let name = r.rsplit('/').next().unwrap_or_default();
        if let Some((_, alias)) = ALIASES.iter().find(|(def, _)| *def == name) {
            return (*alias).into();
        }
        return annotation(&defs[name], defs, depth + 1);
    }
    if let Some(c) = s.get("const") {
        return format!("Literal[{}]", py_value(c));
    }
    if let Some(e) = s["enum"].as_array() {
        let vals: Vec<String> = e.iter().filter(|v| !v.is_null()).map(py_value).collect();
        return if vals.is_empty() { "None".into() } else { format!("Literal[{}]", vals.join(", ")) };
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(alts) = s[key].as_array() {
            return union(alts.iter().map(|a| annotation(a, defs, depth + 1)));
        }
    }
    match &s["type"] {
        Value::String(t) => typed(t, s, defs, depth),
        Value::Array(ts) => union(ts.iter().filter_map(Value::as_str).map(|t| typed(t, s, defs, depth))),
        _ => "Any".into(),
    }
}

fn typed(t: &str, s: &Value, defs: &Value, depth: u32) -> String {
    match t {
        "string" => "str".into(),
        "number" => "float".into(),
        "integer" => "int".into(),
        "boolean" => "bool".into(),
        "null" => "None".into(),
        "object" => "dict[str, Any]".into(),
        "array" => match s.get("items") {
            Some(items) if items.is_object() => format!("list[{}]", annotation(items, defs, depth + 1)),
            _ => "list[Any]".into(),
        },
        _ => "Any".into(),
    }
}

/// `A | B`, without `None` (every optional parameter defaults to `None` anyway) and without
/// repeats; `Any` when it would say nothing useful.
fn union(parts: impl Iterator<Item = String>) -> String {
    let mut seen: Vec<String> = Vec::new();
    for p in parts {
        for q in p.split(" | ") {
            if q != "None" && !seen.iter().any(|s| s == q) {
                seen.push(q.to_string());
            }
        }
    }
    if seen.iter().any(|s| s == "Any") || seen.len() > 4 {
        return "Any".into();
    }
    if seen.is_empty() { "None".into() } else { seen.join(" | ") }
}

fn py_value(v: &Value) -> String {
    match v {
        Value::String(s) => lit(s),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Null => "None".into(),
        v => v.to_string(),
    }
}

/// One parameter of a generated method.
struct Param {
    key: String,
    name: String,
    ann: String,
    required: bool,
    doc: String,
}

/// The parameters of an input schema, required ones first, in the schema's order otherwise.
fn params(schema: &Value) -> Result<Vec<Param>, String> {
    let Some(props) = schema["properties"].as_object() else { return Ok(Vec::new()) };
    // A top-level anyOf/oneOf is a choice between required sets: then nothing is required alone.
    let alternatives = schema.get("anyOf").is_some() || schema.get("oneOf").is_some();
    let required: BTreeSet<&str> =
        if alternatives { BTreeSet::new() } else { schema["required"].as_array().into_iter().flatten().filter_map(Value::as_str).collect() };
    let mut out = Vec::new();
    let mut names = BTreeSet::new();
    for (key, s) in props {
        let name = ident(key)?;
        if !names.insert(name.clone()) || name == "self" || name == "extra" {
            return Err(format!("python: parameter `{key}` maps to `{name}`, which is taken"));
        }
        let ann = annotation(s, schema.get("$defs").unwrap_or(&Value::Null), 0);
        let doc = s["description"].as_str().unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" ");
        out.push(Param { key: key.clone(), name, ann, required: required.contains(key.as_str()) && s.get("default").is_none(), doc });
    }
    out.sort_by_key(|p| !p.required);
    Ok(out)
}

/// A method: `def name(self, *, a: T, b: U | None = None) -> Any:` with its docstring, sending
/// `send(<id>, {...}, extra)`. A closed schema (`additionalProperties: false`, every contract)
/// takes no `**extra`, so a misspelt keyword fails in Python, where it was written; an open or
/// missing one (a command without a contract yet) passes other keywords through.
fn method(name: &str, id: &str, title: &str, prose: &str, input: &Value, output: &Value, send: &str) -> Result<String, String> {
    let ps = params(input)?;
    let closed = input["additionalProperties"] == json!(false) && input["properties"].is_object();
    let mut sig = vec!["self".to_string()];
    if !ps.is_empty() {
        sig.push("*".into());
    }
    for p in &ps {
        sig.push(if p.required { format!("{}: {}", p.name, p.ann) } else { format!("{}: {} | None = None", p.name, p.ann) });
    }
    if !closed {
        sig.push("**extra: Any".into());
    }
    let mut d = String::new();
    d += title;
    if !prose.is_empty() {
        d += &format!("\n\n{prose}");
    }
    if !ps.is_empty() {
        d += "\n\nArgs:";
        for p in &ps {
            let note = if p.doc.is_empty() { String::new() } else { format!(" {}", p.doc) };
            d += &format!("\n    {}{}{note}", p.name, if p.required { " (required):" } else { ":" });
        }
    }
    if let Some(props) = output["properties"].as_object() {
        let keys: Vec<&str> = props.keys().map(String::as_str).filter(|k| *k != "param_warnings").collect();
        if !keys.is_empty() {
            d += &format!("\n\nReturns: {{{}}}", keys.join(", "));
        }
    }
    let body = ps.iter().map(|p| format!("{}: {}", lit(&p.key), p.name)).collect::<Vec<_>>().join(", ");
    Ok(format!(
        "    def {name}({}) -> Any:\n        \"\"\"{}\n        \"\"\"\n        return {send}({}, {{{body}}}, {})\n",
        sig.join(", "),
        doc(&d, "        ").trim_start(),
        lit(id),
        if closed { "{}" } else { "extra" },
    ))
}

/// A node of the command tree: the command at this id, if any, and the nodes below it.
#[derive(Default)]
struct Node<'a> {
    command: Option<&'a Value>,
    children: BTreeMap<String, Node<'a>>,
}

fn class_name(path: &[String]) -> String {
    if path.is_empty() { "Commands".into() } else { format!("_{}", path.join("_")) }
}

/// The classes of `node` and of everything below it, deepest first.
fn emit_node(node: &Node, path: &mut Vec<String>, out: &mut Vec<String>) -> Result<(), String> {
    let mut idents: BTreeMap<String, String> = BTreeMap::new();
    for seg in node.children.keys() {
        let id = ident(seg)?;
        if let Some(prev) = idents.insert(id.clone(), seg.clone()) {
            return Err(format!("python: `{}` and `{seg}` under `{}` both map to `{id}`", prev, path.join(".")));
        }
    }
    for (seg, child) in &node.children {
        if !child.children.is_empty() {
            path.push(ident(seg)?);
            emit_node(child, path, out)?;
            path.pop();
        }
    }
    let name = class_name(path);
    let dotted = path.join(".");
    let mut text = format!(
        "class {name}(_Node):\n    \"\"\"{}\"\"\"\n\n    def __init__(self, send: Any) -> None:\n        self._send = send\n",
        if dotted.is_empty() {
            "Every SolidCraft command, by id: `d.cmd.solid.box(...)` runs `solid.box`.".to_string()
        } else {
            format!("The `{dotted}.*` commands.")
        },
    );
    for (seg, child) in &node.children {
        if !child.children.is_empty() {
            let mut sub = path.clone();
            sub.push(ident(seg)?);
            text += &format!("        self.{} = {}(send)\n", ident(seg)?, class_name(&sub));
        }
    }
    if let Some(c) = node.command {
        text += "\n";
        text += &command_method("__call__", c)?;
    }
    for (seg, child) in &node.children {
        if child.children.is_empty()
            && let Some(c) = child.command
        {
            text += "\n";
            text += &command_method(&ident(seg)?, c)?;
        }
    }
    out.push(text);
    Ok(())
}

fn command_method(name: &str, c: &Value) -> Result<String, String> {
    let id = c["id"].as_str().unwrap_or_default();
    let title = format!("{} (`{id}`).", c["label"].as_str().unwrap_or(id));
    method(name, id, &title, c["params"].as_str().unwrap_or_default(), &c["input"], &c["output"], "self._send")
}

const HEADER: &str = "# Generated by `cargo xtask python` from the build's command registry and MCP tool list.\n# Do not edit; it is not committed. docs/python-api.md\n";

/// The generated files: `commands.py`, `tools.py`, `__init__.py`.
pub fn generate(commands: &[Value], tools: &[Value], version: &str) -> Result<Vec<(String, String)>, String> {
    let mut root = Node::default();
    let mut index = Vec::new();
    for c in commands.iter().filter(|c| !is_planned(c)) {
        let id = c["id"].as_str().ok_or("python: a command has no id")?;
        let mut node = &mut root;
        for seg in id.split('.') {
            node = node.children.entry(seg.to_string()).or_default();
        }
        node.command = Some(c);
        index.push(format!(
            "    ({}, {}, {}),",
            lit(id),
            lit(c["label"].as_str().unwrap_or_default()),
            lit(c["params"].as_str().unwrap_or_default())
        ));
    }
    let mut classes = Vec::new();
    emit_node(&root, &mut Vec::new(), &mut classes)?;
    let commands_py = format!(
        "{HEADER}from __future__ import annotations\n\nfrom typing import Any, Literal\n\nfrom .._types import Angle, Length, Names, Point2, Point3, Vec3, _Node\n\n\n{}\n\n# (id, label, parameters) of every command, for `solidcraft.find`.\nINDEX: list[tuple[str, str, str]] = [\n{}\n]\n",
        classes.join("\n\n"),
        index.join("\n"),
    );

    let mut tool_methods = Vec::new();
    let mut seen = BTreeSet::new();
    for t in tools {
        let name = t["name"].as_str().ok_or("python: a tool has no name")?;
        let id = ident(name)?;
        if SESSION_NAMES.contains(&id.as_str()) || !seen.insert(id.clone()) {
            return Err(format!("python: tool `{name}` maps to `{id}`, which is taken"));
        }
        let desc = t["description"].as_str().unwrap_or_default();
        let (title, rest) = desc.split_once(". ").map_or((desc.to_string(), ""), |(a, b)| (format!("{a}."), b));
        tool_methods.push(method(&id, name, &format!("MCP tool `{name}`: {title}"), rest, &t["inputSchema"], &Value::Null, "self.tool_call")?);
    }
    let tools_py = format!(
        "{HEADER}from __future__ import annotations\n\nfrom typing import Any, Literal\n\nfrom .._types import Angle, Length, Names, Point2, Point3, Vec3\n\n\nclass Tools:\n    \"\"\"Every tool of SolidCraft's MCP server, as a method of the session.\"\"\"\n\n    def tool_call(self, name: str, args: dict[str, Any], extra: dict[str, Any]) -> Any:\n        raise NotImplementedError\n\n{}\n\nTOOL_NAMES: list[str] = [{}]\n",
        tool_methods.join("\n"),
        tools.iter().filter_map(|t| t["name"].as_str()).map(lit).collect::<Vec<_>>().join(", "),
    );
    let init_py = format!("{HEADER}VERSION = {}\n", lit(version));
    Ok(vec![("commands.py".into(), commands_py), ("tools.py".into(), tools_py), ("__init__.py".into(), init_py)])
}

// ---------------------------------------------------------------- wheels

/// A Cargo version as a PEP 440 version: `0.51.0` stays, `0.51.0-rc.1` is `0.51.0rc1` (`alpha`,
/// `beta` likewise `a`, `b`), and a nightly `0.51.0-nightly.20261127.123.1` is
/// `0.51.0.dev202611271231`, a development release that sorts by date and run.
pub fn pep440(version: &str) -> Result<String, String> {
    // A post release of the Python package alone (`0.51.1.post1`, pypi.yml's source mode): already
    // PEP 440, on top of the release whose CLI it downloads.
    if let Some((base, n)) = version.split_once(".post")
        && !n.is_empty()
        && n.bytes().all(|b| b.is_ascii_digit())
    {
        return pep440(base).map(|b| format!("{b}.post{n}")).map_err(|_| format!("python wheel: `{version}` is not a version like 0.51.0.post1"));
    }
    let (base, pre) = version.split_once('-').unwrap_or((version, ""));
    if base.split('.').count() != 3 || !base.split('.').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())) {
        return Err(format!("python wheel: `{version}` is not a version like 0.51.0"));
    }
    if pre.is_empty() {
        return Ok(base.to_string());
    }
    let parts: Vec<&str> = pre.split('.').collect();
    let digits = |p: &[&str]| -> Option<String> {
        (!p.is_empty() && p.iter().all(|x| !x.is_empty() && x.bytes().all(|b| b.is_ascii_digit()))).then(|| p.concat())
    };
    let (kind, n) = (parts[0], digits(&parts[1..]));
    match (kind, n) {
        ("rc", Some(n)) if parts.len() == 2 => Ok(format!("{base}rc{n}")),
        ("alpha", Some(n)) if parts.len() == 2 => Ok(format!("{base}a{n}")),
        ("beta", Some(n)) if parts.len() == 2 => Ok(format!("{base}b{n}")),
        ("nightly", Some(n)) => Ok(format!("{base}.dev{n}")),
        _ => Err(format!("python wheel: no PEP 440 form for the version suffix `-{pre}`")),
    }
}

/// Standard base64 without padding, URL-safe, as a wheel's RECORD writes hashes.
fn b64url(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18u32, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(T[((n >> shift) & 63) as usize]));
            }
        }
    }
    out
}

/// The files of a wheel, as (path in the wheel, bytes, executable).
type Entries = Vec<(String, Vec<u8>, bool)>;

/// The wheel's files: the package, the binary, and the `.dist-info` metadata (RECORD last).
pub fn wheel_entries(root: &Path, version: &str, platform: &str, cli: Option<(&str, Vec<u8>)>) -> Result<Entries, String> {
    let pkg = root.join("python/src/solidcraft");
    if !pkg.join("_generated/commands.py").exists() {
        return Err("python wheel: the bindings are missing: run `cargo xtask python` first".into());
    }
    let mut files: Entries = Vec::new();
    let mut stack = vec![pkg.clone()];
    while let Some(dir) = stack.pop() {
        let mut entries: Vec<PathBuf> =
            std::fs::read_dir(&dir).map_err(|e| format!("python wheel: {}: {e}", dir.display()))?.filter_map(|e| e.ok().map(|e| e.path())).collect();
        entries.sort();
        for path in entries {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            if path.is_dir() {
                if name != "__pycache__" && name != "bin" {
                    stack.push(path);
                }
            } else if name.ends_with(".py") {
                let rel = path.strip_prefix(root.join("python/src")).map_err(|e| e.to_string())?.to_string_lossy().replace('\\', "/");
                files.push((rel, std::fs::read(&path).map_err(|e| format!("python wheel: {}: {e}", path.display()))?, false));
            }
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let binary = cli.is_some();
    if let Some((name, bytes)) = cli {
        files.push((format!("solidcraft/bin/{name}"), bytes, true));
    }
    let info = format!("solidcraft-{version}.dist-info");
    let mut licences = vec!["LICENSE-MIT", "LICENSE-APACHE", "NOTICE"];
    if binary {
        licences.push("THIRD-PARTY-LICENSES.txt"); // the binary carries third-party code
    }
    let readme = std::fs::read_to_string(root.join("python/README.md")).map_err(|e| format!("python wheel: python/README.md: {e}"))?;
    let mut metadata = format!(
        "Metadata-Version: 2.4\nName: solidcraft\nVersion: {version}\nSummary: Drive SolidCraft, the parametric CAD app, from Python: every command and MCP tool, typed.\n\
         License-Expression: MIT OR Apache-2.0\nRequires-Python: >=3.10\nProject-URL: Source, https://github.com/tangency-labs/solidcraft\n"
    );
    for l in &licences {
        metadata += &format!("License-File: {l}\n");
        let bytes = std::fs::read(root.join(l)).map_err(|e| format!("python wheel: {l}: {e}"))?;
        files.push((format!("{info}/licenses/{l}"), bytes, false));
    }
    metadata += &format!("Description-Content-Type: text/markdown\n\n{readme}");
    files.push((format!("{info}/METADATA"), metadata.into_bytes(), false));
    let wheel_meta = format!(
        "Wheel-Version: 1.0\nGenerator: cargo xtask python wheel\nRoot-Is-Purelib: {}\nTag: py3-none-{platform}\n",
        if binary { "false" } else { "true" }
    );
    files.push((format!("{info}/WHEEL"), wheel_meta.into_bytes(), false));
    files.push((
        format!("{info}/entry_points.txt"),
        b"[console_scripts]\nsolidcraft = solidcraft._cli:main\nsolidcraft-session = solidcraft.__main__:main\n".to_vec(),
        false,
    ));
    let mut record = String::new();
    for (path, bytes, _) in &files {
        use sha2::Digest as _;
        record += &format!("{path},sha256={},{}\n", b64url(&sha2::Sha256::digest(bytes)), bytes.len());
    }
    record += &format!("{info}/RECORD,,\n");
    files.push((format!("{info}/RECORD"), record.into_bytes(), false));
    Ok(files)
}

fn wheel(root: &Path, args: &[&str]) -> Result<(), String> {
    let (mut platform, mut version, mut cli, mut out) = (None, None, None, root.join("dist/python"));
    let mut i = 0;
    while let Some(a) = args.get(i) {
        let value = args.get(i + 1).copied();
        match (*a, value) {
            ("--platform", Some(v)) => platform = Some(v.to_string()),
            ("--version", Some(v)) => version = Some(v.to_string()),
            ("--cli", Some(v)) => cli = Some(PathBuf::from(v)),
            ("--out", Some(v)) => out = PathBuf::from(v),
            (other, _) => {
                return Err(format!(
                    "python wheel: unknown or incomplete argument `{other}` (it takes --platform TAG, --version V, --cli PATH, --out DIR)"
                ));
            }
        }
        i += 2;
    }
    let platform = platform.ok_or("python wheel: --platform is required (a wheel platform tag such as manylinux_2_35_x86_64, or any)")?;
    let version = pep440(&version.ok_or("python wheel: --version is required")?)?;
    if platform.is_empty() || !platform.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Err(format!("python wheel: `{platform}` is not a wheel platform tag"));
    }
    let binary = match (&cli, platform.as_str()) {
        (Some(_), "any") => return Err("python wheel: a wheel for any platform carries no binary; leave out --cli".into()),
        (None, p) if p != "any" => return Err(format!("python wheel: a {p} wheel needs --cli, the release's solidcraft-cli for it")),
        (Some(path), _) => {
            let name = if platform.starts_with("win") { "solidcraft-cli.exe" } else { "solidcraft-cli" };
            Some((name, std::fs::read(path).map_err(|e| format!("python wheel: {}: {e}", path.display()))?))
        }
        (None, _) => None,
    };
    let files = wheel_entries(root, &version, &platform, binary)?;
    std::fs::create_dir_all(&out).map_err(|e| format!("python wheel: {}: {e}", out.display()))?;
    let path = out.join(format!("solidcraft-{version}-py3-none-{platform}.whl"));
    let file = std::fs::File::create(&path).map_err(|e| format!("python wheel: {}: {e}", path.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    for (name, bytes, exec) in &files {
        use std::io::Write as _;
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).unix_permissions(if *exec {
            0o755
        } else {
            0o644
        });
        zip.start_file(name.as_str(), opts).map_err(|e| format!("python wheel: {name}: {e}"))?;
        zip.write_all(bytes).map_err(|e| format!("python wheel: {name}: {e}"))?;
    }
    zip.finish().map_err(|e| format!("python wheel: {}: {e}", path.display()))?;
    println!("python wheel: {} ({} files)", path.display(), files.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_become_identifiers() {
        assert_eq!(ident("from").unwrap(), "from_");
        assert_eq!(ident("import").unwrap(), "import_");
        assert_eq!(ident("two-point").unwrap(), "two_point");
        assert_eq!(ident("3d").unwrap(), "_3d");
        assert!(ident("").is_err());
    }

    #[test]
    fn annotations_stay_readable() {
        let defs = json!({"LengthExpr": {}, "Op": {"enum": ["new", "cut"]}});
        assert_eq!(annotation(&json!({"$ref": "#/$defs/LengthExpr"}), &defs, 0), "Length");
        assert_eq!(annotation(&json!({"$ref": "#/$defs/Op"}), &defs, 0), "Literal[\"new\", \"cut\"]");
        assert_eq!(annotation(&json!({"type": ["string", "null"]}), &defs, 0), "str");
        assert_eq!(annotation(&json!({"type": "array", "items": {"type": "number"}}), &defs, 0), "list[float]");
        assert_eq!(annotation(&json!({"anyOf": [{"type": "number"}, {"type": "string"}, {"type": "null"}]}), &defs, 0), "float | str");
    }

    #[test]
    fn a_command_that_is_also_a_prefix_is_callable() {
        let cmds = vec![
            json!({"id": "sketch.line", "label": "Line", "params": "points", "input": {"type": "object", "additionalProperties": false, "properties": {"points": {"type": "array"}}, "required": ["points"]}, "output": null}),
            json!({"id": "sketch.line.midpoint", "label": "Midpoint Line", "params": "", "input": {"type": "object", "properties": {"from": {"type": "string"}}}, "output": null}),
            json!({"id": "solid.future", "label": "Planned", "params": "", "input": null, "output": null, "planned": "#1"}),
        ];
        let tools = vec![
            json!({"name": "measure", "description": "Measure bodies. More.", "inputSchema": {"type": "object", "properties": {"summary": {"type": "boolean"}}}}),
        ];
        let files = generate(&cmds, &tools, "1.2.3").unwrap();
        let commands = &files[0].1;
        assert!(commands.contains("class _sketch_line(_Node):"), "{commands}");
        assert!(commands.contains("def __call__(self, *, points: list[Any]) -> Any"), "{commands}");
        assert!(commands.contains("def midpoint(self, *, from_: str | None = None, **extra: Any)"), "{commands}");
        assert!(commands.contains("{\"from\": from_}"), "{commands}");
        assert!(!commands.contains("solid.future"), "planned commands are left out");
        assert!(files[1].1.contains("def measure(self, *, summary: bool | None = None, **extra: Any)"), "{}", files[1].1);
        assert!(files[2].1.contains("VERSION = \"1.2.3\""));
    }

    #[test]
    fn cargo_versions_become_pep_440() {
        assert_eq!(pep440("0.51.0").unwrap(), "0.51.0");
        assert_eq!(pep440("0.51.0-rc.2").unwrap(), "0.51.0rc2");
        assert_eq!(pep440("1.0.0-beta.1").unwrap(), "1.0.0b1");
        assert_eq!(pep440("0.51.0-nightly.20261127.123.1").unwrap(), "0.51.0.dev202611271231");
        assert!(pep440("0.51").is_err());
        assert!(pep440("0.51.0-weird").is_err());
        assert!(pep440("0.51.0-rc.x").is_err());
        assert_eq!(pep440("0.51.1.post1").unwrap(), "0.51.1.post1");
        assert!(pep440("0.51.1.post").is_err());
        assert!(pep440("0.51.post1").is_err());
    }

    #[test]
    fn record_hashes_are_unpadded_url_safe_base64() {
        // sha256("") in a wheel RECORD, as pip writes it.
        use sha2::Digest as _;
        assert_eq!(b64url(&sha2::Sha256::digest(b"")), "47DEQpj8HBSa-_TImW-5JCeuQeRkm5NMpJWZG3hSuFU");
    }

    #[test]
    fn a_tool_that_would_hide_a_session_method_stops_the_generator() {
        let tools = vec![json!({"name": "call", "description": "", "inputSchema": {}})];
        assert!(generate(&[], &tools, "0").is_err_and(|e| e.contains("taken")));
    }
}
