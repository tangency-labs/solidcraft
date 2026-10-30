//! What the chat's agent may do to the app. Every request its tools send (whatever the backend)
//! passes here before it runs: a short list of methods (run commands, read the design, move the
//! camera, render), with commands sorted into those that run at once, those that wait for the
//! user's Allow or Deny in the panel (anything that reads or writes files or replaces the design),
//! and those that never run for the agent (crash reports, which hold other designs and paths).
//! Plain data and rules, tested without a window.

use serde_json::Value;

/// Changes the agent may make in one turn (requests that run commands; reads don't count); 0 is
/// no limit, the default. The user can always press Stop.
pub const DEFAULT_TURN_LIMIT: u32 = 0;
/// What the limit field shows when the user turns the limit on.
pub const SEEDED_TURN_LIMIT: u32 = 24;
/// The default before the limit became optional; saved preferences still carry it, so it reads
/// as no limit.
pub const OLD_DEFAULT_TURN_LIMIT: u32 = 24;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Runs now. `change`: it runs a command that may change the design (counted per turn).
    Run { change: bool },
    /// Waits for Allow or Deny; the text says what it would do.
    Ask(String),
    /// Never for the agent; the reason goes back to it.
    Refuse(String),
}

/// Methods the bridged MCP server uses; nothing else (clicks, preferences, quitting, the chat
/// itself) is the agent's.
const METHODS: &[&str] = &["engine.commands", "document.inspect", "engine.execute", "engine.batch", "ui.view", "ui.render", "ui.screenshot"];

/// Parameter names that hold a file or folder and nothing else.
const FILE_KEYS: &[&str] = &["file", "files", "dir", "folder", "out", "output", "filename", "target_path"];

/// Parameter names that are a file in some commands and geometry in others (`path`: a sweep's or
/// pattern's curve ids, an occurrence's ids; `source`: a feature or body). They name a file only
/// when the value reads like one, or the command is by its nature about files.
const AMBIGUOUS_KEYS: &[&str] = &["path", "paths", "source"];

/// File extensions that make a bare word a file name.
const FILE_EXTENSIONS: &[&str] = &[
    "solidcraft",
    "json",
    "csv",
    "step",
    "stp",
    "iges",
    "igs",
    "stl",
    "3mf",
    "obj",
    "dxf",
    "svg",
    "png",
    "jpg",
    "jpeg",
    "urdf",
    "txt",
    "py",
    "toml",
];

/// Commands that only read (not counted as changes). Section Analysis changes only the view, but
/// each change is an undo step.
fn reads_only(id: &str) -> bool {
    (id.starts_with("inspect.") && id != "inspect.section")
        || id.starts_with("model.")
        || id.ends_with(".inspect")
        || id.ends_with(".list")
        || matches!(id, "edit.checkpoint" | "render.status" | "render.backends" | "render.settings.get" | "file.slicers" | "engine.describe")
}

/// Commands that read or write a file by their nature: any string they are given in a path
/// parameter is a file, whatever it looks like.
fn is_file_verb(id: &str) -> bool {
    id.starts_with("library.") || ["export", "import", "save", "open", "insert", "load", "write", "recover"].iter().any(|v| id.contains(v))
}

/// Whether `s` reads like a file or folder name: a path separator, `~`, a leading dot, or a known
/// extension. Curve ids (`c1`), edge names (`edge-3`) and the like do not.
fn looks_like_file(s: &str) -> bool {
    if s.is_empty() || s.len() > 4096 {
        return false;
    }
    if s.contains('/') || s.contains('\\') || s.starts_with('~') || s.starts_with('.') {
        return true;
    }
    s.rsplit_once('.').is_some_and(|(stem, ext)| !stem.is_empty() && FILE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// The first file or folder a request's parameters name, for the question and the decision.
/// `id`: the command (empty for methods that are not commands).
fn names_a_file(id: &str, params: &Value) -> Option<String> {
    let o = params.as_object()?;
    let shown = |s: &str| s.chars().take(200).collect::<String>();
    let by_nature = is_file_verb(id);
    let first = |v: &Value, any: bool| -> Option<String> {
        match v {
            Value::String(s) if any && !s.is_empty() => Some(shown(s)),
            Value::String(s) if looks_like_file(s) => Some(shown(s)),
            Value::Array(a) => {
                let names: Vec<&str> = a.iter().filter_map(Value::as_str).filter(|s| (any && !s.is_empty()) || looks_like_file(s)).take(3).collect();
                (!names.is_empty()).then(|| names.join(", "))
            }
            _ => None,
        }
    };
    FILE_KEYS
        .iter()
        .find_map(|k| o.get(*k).and_then(|v| first(v, true)))
        .or_else(|| AMBIGUOUS_KEYS.iter().find_map(|k| o.get(*k).and_then(|v| first(v, by_nature))))
}

/// One command as the agent would run it. `undo_left`: steps this turn may still undo without
/// reaching changes made before it.
pub fn command(id: &str, params: &Value, undo_left: usize) -> Verdict {
    let id = id.trim();
    if id.starts_with("diagnostics.") || id.contains("crash") {
        return Verdict::Refuse(format!("`{id}` is not available in the agent chat: crash reports stay with the user"));
    }
    if id.starts_with("app.") || id.starts_with("prefs.") {
        return Verdict::Refuse(format!("`{id}` is not available in the agent chat"));
    }
    // Files, and commands that replace or reload the whole design.
    let file_command = id.starts_with("file.") || id.starts_with("doc.") || matches!(id, "render.export" | "render.start");
    // The question says which program a send to a slicer would start.
    let with_slicer =
        |question: String| match params.get("slicer").and_then(Value::as_str).filter(|_| id.ends_with("save_mesh") || id.ends_with("send_to_slicer"))
        {
            Some(slicer) => format!("{question}, send to {}", slicer.chars().take(200).collect::<String>()),
            None => question,
        };
    // The model leaves the machine: the user is asked, whatever the agent says about consent.
    if id == "file.open_in_easyprint" {
        return Verdict::Ask(format!("{id}, upload the model to Printables' servers (EasyPrint)"));
    }
    if let Some(p) = names_a_file(id, params) {
        return Verdict::Ask(with_slicer(format!("{id}: {p}")));
    }
    if file_command && !reads_only(id) {
        return Verdict::Ask(with_slicer(id.to_string()));
    }
    // Undo past this turn's first change takes back the user's own work.
    if id == "edit.undo" && undo_left == 0 {
        return Verdict::Ask(format!("{id} (a step from before this message)"));
    }
    Verdict::Run { change: !reads_only(id) }
}

/// One request from the agent's tools: `method` and its `params` as the control channel has them.
pub fn request(method: &str, params: &Value, undo_left: usize) -> Verdict {
    if !METHODS.contains(&method) {
        return Verdict::Refuse(format!("`{method}` is not available in the agent chat"));
    }
    match method {
        "engine.execute" => {
            let id = params.get("command").and_then(Value::as_str).unwrap_or("");
            command(id, params.get("params").unwrap_or(&Value::Null), undo_left)
        }
        "engine.batch" => {
            let Some(steps) = params.get("commands").and_then(Value::as_array) else { return Verdict::Run { change: false } };
            let mut asks = Vec::new();
            let mut change = false;
            // A batch can undo as many steps as it has: count down as it goes.
            let mut undo_left = undo_left;
            for s in steps.iter().take(10_000) {
                let id = s.get("command").or_else(|| s.get("id")).and_then(Value::as_str).unwrap_or("");
                match command(id, s.get("params").unwrap_or(&Value::Null), undo_left) {
                    Verdict::Refuse(e) => return Verdict::Refuse(e),
                    Verdict::Ask(a) => asks.push(a),
                    Verdict::Run { change: c } => {
                        change |= c;
                        if id == "edit.undo" {
                            undo_left = undo_left.saturating_sub(1);
                        } else if c {
                            undo_left += 1;
                        }
                    }
                }
            }
            if asks.is_empty() {
                Verdict::Run { change }
            } else {
                Verdict::Ask(format!("batch: {}", asks.into_iter().take(6).collect::<Vec<_>>().join("; ")))
            }
        }
        // A picture saved to a file names it.
        "ui.screenshot" | "ui.render" => match names_a_file("ui.render.export", params) {
            Some(_) if is_scratch(params) => Verdict::Run { change: false },
            Some(p) => Verdict::Ask(format!("{method}: {p}")),
            None => Verdict::Run { change: false },
        },
        _ => Verdict::Run { change: false },
    }
}

/// What "Always allow" remembers for a request: the command it runs (`batch` for a batch).
pub fn tool_key(method: &str, params: &Value) -> String {
    match method {
        "engine.execute" => params.get("command").and_then(Value::as_str).unwrap_or("").chars().take(120).collect(),
        "engine.batch" => "batch".into(),
        m => m.into(),
    }
}

/// A request as an approval card says it: the command, or a batch's commands.
pub fn describe(method: &str, params: &Value) -> String {
    match method {
        "engine.batch" => {
            let steps = params.get("commands").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
            let ids: Vec<&str> = steps.iter().filter_map(|s| s.get("command").or_else(|| s.get("id")).and_then(Value::as_str)).take(6).collect();
            let more = if steps.len() > ids.len() { format!(" (+{})", steps.len() - ids.len()) } else { String::new() };
            format!("batch: {}{more}", ids.join(", "))
        }
        _ => tool_key(method, params),
    }
}

/// The MCP server saves the pictures it hands back in its own scratch folder, inside its
/// `allow_paths`; those need no question.
fn is_scratch(params: &Value) -> bool {
    let Some(path) = params.get("path").and_then(Value::as_str) else { return false };
    let path = std::path::Path::new(path);
    if path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return false;
    }
    let ok = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("png"));
    ok && params.get("allow_paths").and_then(Value::as_array).is_some_and(|roots| {
        roots
            .iter()
            .filter_map(Value::as_str)
            .map(std::path::Path::new)
            .any(|r| r.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("solidcraft-mcp-")) && path.starts_with(r))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn exec(id: &str, params: Value) -> Verdict {
        request("engine.execute", &json!({"command": id, "params": params}), 5)
    }

    #[test]
    fn modelling_runs_and_counts_reads_dont() {
        assert_eq!(exec("solid.box", json!({"x": 10})), Verdict::Run { change: true });
        assert_eq!(exec("sketch.inspect", json!({"sketch": 1})), Verdict::Run { change: false });
        assert_eq!(exec("inspect.measure", json!({})), Verdict::Run { change: false });
        assert_eq!(request("document.inspect", &json!({}), 0), Verdict::Run { change: false });
        assert_eq!(exec("engine.describe", json!({"command": "solid.box"})), Verdict::Run { change: false });
    }

    #[test]
    fn files_and_replacing_the_design_ask_first() {
        assert!(matches!(exec("file.save", json!({"path": "/tmp/a.solidcraft"})), Verdict::Ask(a) if a.contains("/tmp/a.solidcraft")));
        assert!(matches!(exec("file.new", json!({})), Verdict::Ask(_)));
        assert!(matches!(exec("doc.open", json!({"path": "x.step"})), Verdict::Ask(_)));
        assert!(matches!(exec("file.export", json!({"path": "x.stl"})), Verdict::Ask(_)));
        // Any command given a file, whatever its area.
        assert!(matches!(exec("sketch.import", json!({"file": "a.dxf"})), Verdict::Ask(_)));
        let batch = json!({"commands": [{"command": "solid.box", "params": {}}, {"command": "file.save", "params": {"path": "/etc/x"}}]});
        assert!(matches!(request("engine.batch", &batch, 0), Verdict::Ask(a) if a.contains("/etc/x")));
    }

    /// Saving a mesh or opening it in a slicer asks, and the question names the slicer.
    #[test]
    fn starting_a_slicer_asks_and_names_it() {
        assert!(
            matches!(exec("file.save_mesh", json!({"path": "a.stl", "slicer": "orca"})), Verdict::Ask(a) if a.contains("a.stl") && a.contains("send to orca"))
        );
        assert!(
            matches!(exec("file.send_to_slicer", json!({"slicer": "prusa", "bodies": ["Part"]})), Verdict::Ask(a) if a.ends_with("send to prusa"))
        );
        assert!(
            matches!(exec("file.open_in_easyprint", json!({"bodies": ["Part"], "consent": true})), Verdict::Ask(a) if a.contains("upload the model"))
        );
        assert!(matches!(exec("file.send_to_slicer", json!({})), Verdict::Ask(_)));
    }

    /// `path`, `source` and friends are also geometry (curve ids, edges, occurrence ids): those
    /// are not files and must not stop a sweep or a pattern with a question.
    #[test]
    fn geometry_named_path_is_not_a_file() {
        assert_eq!(exec("solid.sweep", json!({"profile": "S1", "path_sketch": "S2", "path": ["c1", "c2"]})), Verdict::Run { change: true });
        assert_eq!(
            exec("solid.pattern.path", json!({"features": ["F1"], "path_sketch": "S2", "path": ["c1"], "count": 4})),
            Verdict::Run { change: true }
        );
        assert_eq!(exec("construct.plane.along_path", json!({"path": "edge-3", "t": 0.5})), Verdict::Run { change: true });
        assert_eq!(exec("component.move", json!({"component": 2, "path": [4, 7], "translate": [1, 0, 0]})), Verdict::Run { change: true });
        assert_eq!(exec("occurrence.move", json!({"occurrence": 3, "path": [1, 2]})), Verdict::Run { change: true });
        let batch = json!({"commands": [
            {"command": "solid.box", "params": {}},
            {"command": "solid.sweep", "params": {"path": ["c1", "c2"], "path_sketch": "S"}},
        ]});
        assert_eq!(request("engine.batch", &batch, 0), Verdict::Run { change: true });
    }

    /// What does name a file still asks, with or without a separator or an extension, and in
    /// the commands that export, import or save by their nature.
    #[test]
    fn real_file_names_still_ask() {
        for (id, p) in [
            ("sketch.export_dxf", json!({"path": "/tmp/s.dxf"})),
            ("sketch.export_dxf", json!({"path": "s"})),
            ("parameters.export", json!({"path": "params"})),
            ("parameters.import", json!({"path": "p.csv"})),
            ("motion.export", json!({"study": "a", "path": "out.csv"})),
            ("canvas.insert", json!({"file": "pic"})),
            ("library.apply", json!({"path": "lib"})),
            ("solid.box", json!({"filename": "x"})),
            ("solid.box", json!({"path": "../x.step"})),
            ("solid.box", json!({"paths": ["a/b.step"]})),
            ("solid.box", json!({"dir": "out"})),
        ] {
            assert!(matches!(exec(id, p.clone()), Verdict::Ask(_)), "{id} {p}");
        }
    }

    /// Every registered `file.*` and `doc.*` command asks, so a new one cannot slip through by
    /// name (the read-only ones, `reads_only`, are the listed exceptions).
    #[test]
    fn every_registered_file_and_doc_command_asks() {
        let mut n = 0;
        for spec in solidcraft_engine::command_specs() {
            if (spec.id.starts_with("file.") || spec.id.starts_with("doc.")) && !reads_only(spec.id) {
                n += 1;
                assert!(matches!(exec(spec.id, json!({})), Verdict::Ask(_)), "{}", spec.id);
            }
        }
        assert!(n >= 8, "found {n}");
    }

    #[test]
    fn crash_reports_and_app_methods_are_never_the_agents() {
        assert!(matches!(exec("diagnostics.crash_reports", json!({})), Verdict::Refuse(_)));
        assert!(matches!(exec("diagnostics.crash_report", json!({"id": "x"})), Verdict::Refuse(_)));
        let batch = json!({"commands": [{"command": "solid.box"}, {"command": "diagnostics.crash_reports"}]});
        assert!(matches!(request("engine.batch", &batch, 0), Verdict::Refuse(_)));
        for m in ["app.quit", "ui.click", "ui.prefs", "ui.agent_chat", "engine.script", "ui.key", "ui.file"] {
            assert!(matches!(request(m, &json!({}), 0), Verdict::Refuse(_)), "{m}");
        }
    }

    #[test]
    fn undo_asks_only_past_the_turns_own_changes() {
        assert_eq!(request("engine.execute", &json!({"command": "edit.undo"}), 1), Verdict::Run { change: true });
        assert!(matches!(request("engine.execute", &json!({"command": "edit.undo"}), 0), Verdict::Ask(_)));
        let batch = json!({"commands": [{"command": "solid.box"}, {"command": "edit.undo"}, {"command": "edit.undo"}]});
        assert!(matches!(request("engine.batch", &batch, 0), Verdict::Ask(_)));
        assert_eq!(exec("inspect.section", json!({"plane": "XY"})), Verdict::Run { change: true });
        let sectioned = json!({"commands": [{"command": "inspect.section", "params": {"plane": "XY"}}, {"command": "edit.undo"}]});
        assert_eq!(request("engine.batch", &sectioned, 0), Verdict::Run { change: true });
    }

    #[test]
    fn the_mcp_servers_own_pictures_need_no_question() {
        let scratch = json!({"path": "/tmp/solidcraft-mcp-41-a3/shot-1.png", "allow_paths": ["/tmp/solidcraft-mcp-41-a3"]});
        assert_eq!(request("ui.screenshot", &scratch, 0), Verdict::Run { change: false });
        let elsewhere = json!({"path": "/home/u/Desktop/shot.png", "allow_paths": ["/tmp/solidcraft-mcp-41-a3", "/home/u/Desktop"]});
        assert!(matches!(request("ui.screenshot", &elsewhere, 0), Verdict::Ask(_)));
        assert_eq!(request("ui.render", &json!({"width": 400}), 0), Verdict::Run { change: false });
    }
}
