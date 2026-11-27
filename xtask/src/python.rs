//! `cargo xtask python [--cli PATH] [--out DIR]`: the Python client's generated bindings
//! (`python/src/solidcraft/_generated/`, not committed; docs/python-api.md).
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
fn build_cli(root: &Path) -> Result<PathBuf, String> {
    let mut build = crate::cargo();
    build.args(["build", "-q", "--profile", "ci", "-p", "solidcraft-cli"]);
    crate::run(build, "cargo build --profile ci -p solidcraft-cli")?;
    let target = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| root.join("target"));
    Ok(target.join("ci").join(if cfg!(windows) { "solidcraft-cli.exe" } else { "solidcraft-cli" }))
}

fn cli_json(cli: &Path, args: &[&str]) -> Result<Value, String> {
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

fn is_planned(c: &Value) -> bool {
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
    fn a_tool_that_would_hide_a_session_method_stops_the_generator() {
        let tools = vec![json!({"name": "call", "description": "", "inputSchema": {}})];
        assert!(generate(&[], &tools, "0").is_err_and(|e| e.contains("taken")));
    }
}
