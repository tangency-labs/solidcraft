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
    /// Names no command: the engine's own error goes back to the agent, and nothing runs.
    Unknown(String),
}

/// Methods the bridged MCP server uses; nothing else (clicks, preferences, quitting, the chat
/// itself) is the agent's.
const METHODS: &[&str] = &[
    "engine.commands",
    "document.inspect",
    "document.selection",
    "engine.execute",
    "engine.batch",
    "ui.view",
    "ui.render",
    "ui.screenshot",
    "mcp.options",
];

/// Commands the window answers itself, with no engine entry (`control.rs`, `execute`).
const WINDOW_COMMANDS: &[&str] = &["view.pick"];

/// The registered id `id` names, as the engine resolves it ([`solidcraft_engine::find_command`]:
/// surrounding whitespace ignored, any case, ASCII only), or a command the window answers itself.
/// The gate decides on this id and never on the id as sent, so a request cannot reach a command
/// the gate took for another (`FILE.SAVE` is `file.save`); `None` names nothing that runs.
pub fn resolve(id: &str) -> Option<&'static str> {
    solidcraft_engine::find_command(id)
        .map(|spec| spec.id)
        .or_else(|| solidcraft_engine::normalize_command_id(id).and_then(|id| WINDOW_COMMANDS.iter().find(|w| **w == id).copied()))
}

/// A request's command id, as the control channel reads it: `command`, else `id`.
fn command_of(v: &Value) -> Option<&str> {
    v.get("command").or_else(|| v.get("id")).and_then(Value::as_str)
}

/// Rewrite the commands a request runs to their registered ids ([`resolve`]), in `command`, so
/// what runs after the gate is exactly what the gate decided on. Ids that name nothing are left
/// as they are; the gate refuses them.
pub fn canonicalize(method: &str, params: &mut Value) {
    fn one(step: &mut Value) {
        let Some(id) = command_of(step).and_then(resolve) else { return };
        if let Some(o) = step.as_object_mut() {
            o.remove("id");
            o.insert("command".into(), Value::String(id.into()));
        }
    }
    match method {
        "engine.execute" => one(params),
        "engine.batch" => {
            if let Some(steps) = params.get_mut("commands").and_then(Value::as_array_mut) {
                steps.iter_mut().take(10_000).for_each(one);
            }
        }
        _ => {}
    }
}

/// Whether a request runs a plugin's code anywhere in it: `plugin.run` in any spelling, alone or
/// as any step of a batch. Such a request always waits for the user, whatever the permission mode
/// or an "Always allow" says. (A plugin's own entry, `package.entry`, reaches the gate as
/// `plugin.run` once its package is known (`plugins::normalize_request`); before that it names
/// no command and is refused, so a batch can't inspect a package and run it unasked.)
pub fn runs_plugin(method: &str, params: &Value) -> bool {
    let is_run = |step: &Value| command_of(step).and_then(resolve) == Some("plugin.run");
    match method {
        "engine.execute" => is_run(params),
        "engine.batch" => params.get("commands").and_then(Value::as_array).is_some_and(|steps| steps.iter().take(10_000).any(is_run)),
        _ => false,
    }
}

/// The error for an id that names no command, as the engine words it.
fn unknown(id: &str) -> Verdict {
    let shown: String = id.chars().filter(|c| !c.is_control()).take(120).collect();
    Verdict::Unknown(solidcraft_engine::EngineError::UnknownCommand(shown).to_string())
}

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
    "sdf",
    "txt",
    "py",
    "toml",
];

/// Commands that only read (not counted as changes). Section Analysis changes only the view, but
/// each change is an undo step.
fn reads_only(id: &str) -> bool {
    // The engine finds a command ignoring case, so the plugin readers are matched the same way.
    id.eq_ignore_ascii_case("plugin.inspect")
        || id.eq_ignore_ascii_case("plugin.check")
        || (id.starts_with("inspect.") && id != "inspect.section")
        || id.starts_with("model.")
        || id.ends_with(".inspect")
        || id.ends_with(".list")
        || matches!(
            id,
            "edit.checkpoint"
                | "selection.get"
                | "render.status"
                | "render.backends"
                | "render.settings.get"
                | "file.slicers"
                | "engine.describe"
                | "plugin.check"
                | "plugin.tools"
                | "doc.info"
        )
}

/// Commands that read or write a file by their nature: any string they are given in a path
/// parameter is a file, whatever it looks like.
fn is_file_verb(id: &str) -> bool {
    // Ignoring case, as the engine finds the command: `PLUGIN.CHECK` reads the package too.
    if id.eq_ignore_ascii_case("plugin.inspect") || id.eq_ignore_ascii_case("plugin.check") {
        return true;
    }
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
    // Every rule below compares registered ids: decide on the command the engine will run.
    let Some(id) = resolve(id) else { return unknown(id) };
    if id.starts_with("diagnostics.") || id.contains("crash") {
        return Verdict::Refuse(format!("`{id}` is not available in the agent chat: crash reports stay with the user"));
    }
    if id.starts_with("app.") || id.starts_with("prefs.") {
        return Verdict::Refuse(format!("`{id}` is not available in the agent chat"));
    }
    // External executable code always needs a user decision; the digest is not consent by itself.
    if id == "plugin.run" {
        let shown =
            |key: &str| params.get(key).and_then(Value::as_str).unwrap_or_default().chars().filter(|c| !c.is_control()).take(200).collect::<String>();
        let path = shown("path");
        let entry = shown("entry");
        return Verdict::Ask(solidcraft_ui_text::format("ui.plugin_run_consent", &[("entry", &entry), ("path", &path)]));
    }
    // Files, and commands that replace or reload the whole design.
    // A library write changes files though it names none (only an id).
    let file_command =
        id.starts_with("file.") || id.starts_with("doc.") || matches!(id, "render.export" | "render.start") || crate::library_places::writes(id);
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
    // A library write says which library and entry it changes, as a file command names its file.
    if let Some(what) = crate::library_places::write_names(id, params) {
        return Verdict::Ask(format!("{id}: {what}"));
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
    if !METHODS.contains(&method) && !(method == "ui.help" && params.get("item").is_none_or(Value::is_null)) {
        return Verdict::Refuse(format!("`{method}` is not available in the agent chat"));
    }
    match method {
        "engine.execute" => {
            let id = command_of(params).unwrap_or("");
            command(id, params.get("params").unwrap_or(&Value::Null), undo_left)
        }
        "engine.batch" => {
            let Some(steps) = params.get("commands").and_then(Value::as_array) else { return Verdict::Run { change: false } };
            let mut asks = Vec::new();
            let mut change = false;
            // A batch can undo as many steps as it has: count down as it goes.
            let mut undo_left = undo_left;
            for s in steps.iter().take(10_000) {
                let id = command_of(s).unwrap_or("");
                match command(id, s.get("params").unwrap_or(&Value::Null), undo_left) {
                    Verdict::Refuse(e) => return Verdict::Refuse(e),
                    Verdict::Unknown(e) => return Verdict::Unknown(e),
                    Verdict::Ask(a) => asks.push(a),
                    Verdict::Run { change: c } => {
                        change |= c;
                        if resolve(id) == Some("edit.undo") {
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
        "engine.execute" => {
            let id = command_of(params).unwrap_or("");
            resolve(id).map_or_else(|| id.chars().take(120).collect(), String::from)
        }
        "engine.batch" => "batch".into(),
        m => m.into(),
    }
}

/// What "Always allow" must cover for a request the gate asks about: each command in a batch that
/// asks (by its registered id), or the request's [`tool_key`]. An answer given for one batch never
/// covers another batch's file, design, upload or slicer steps.
pub fn asking_keys(method: &str, params: &Value, undo_left: usize) -> Vec<String> {
    if method != "engine.batch" {
        return vec![tool_key(method, params)];
    }
    let steps = params.get("commands").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    let mut keys: Vec<String> = Vec::new();
    let mut undo_left = undo_left;
    for s in steps.iter().take(10_000) {
        let id = command_of(s).unwrap_or("");
        match command(id, s.get("params").unwrap_or(&Value::Null), undo_left) {
            Verdict::Ask(_) => {
                let key = resolve(id).map_or_else(|| id.chars().take(120).collect(), String::from);
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
            Verdict::Run { change } => {
                if resolve(id) == Some("edit.undo") {
                    undo_left = undo_left.saturating_sub(1);
                } else if change {
                    undo_left += 1;
                }
            }
            Verdict::Refuse(_) | Verdict::Unknown(_) => {}
        }
    }
    if keys.is_empty() { vec![tool_key(method, params)] } else { keys }
}

/// A request as an approval card says it: the command, or a batch's commands.
pub fn describe(method: &str, params: &Value) -> String {
    match method {
        "engine.batch" => {
            let steps = params.get("commands").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
            let ids: Vec<&str> = steps.iter().filter_map(command_of).map(|id| resolve(id).unwrap_or(id)).take(6).collect();
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
        assert_eq!(exec("selection.get", json!({})), Verdict::Run { change: false });
        assert_eq!(request("document.inspect", &json!({}), 0), Verdict::Run { change: false });
        assert_eq!(exec("engine.describe", json!({"command": "solid.box"})), Verdict::Run { change: false });
        assert_eq!(exec("doc.info", json!({})), Verdict::Run { change: false });
    }

    #[test]
    fn files_and_replacing_the_design_ask_first() {
        assert!(matches!(exec("file.save", json!({"path": "/tmp/a.solidcraft"})), Verdict::Ask(a) if a.contains("/tmp/a.solidcraft")));
        assert!(matches!(exec("file.new", json!({})), Verdict::Ask(_)));
        assert!(matches!(exec("doc.open", json!({"path": "x.step"})), Verdict::Ask(_)));
        assert!(matches!(exec("file.export", json!({"path": "x.stl"})), Verdict::Ask(_)));
        // Any command given a file, whatever its area.
        assert!(matches!(exec("sketch.create", json!({"file": "a.dxf"})), Verdict::Ask(_)));
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
        assert_eq!(exec("component.move_bodies", json!({"component": 2, "path": [4, 7], "translate": [1, 0, 0]})), Verdict::Run { change: true });
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

    /// A change to a library place names its folder, so it asks first; listing only reads, and
    /// rescanning every place runs.
    #[test]
    fn library_place_changes_ask() {
        for (id, p) in [
            ("library.places.add", json!({"path": "/srv/lib", "kind": "library"})),
            ("library.places.remove", json!({"path": "/srv/lib"})),
            ("library.places.set", json!({"path": "/srv/lib", "read_only": true})),
            ("library.places.refresh", json!({"path": "/srv/lib"})),
        ] {
            assert!(matches!(exec(id, p.clone()), Verdict::Ask(a) if a.contains("/srv/lib")), "{id} {p}");
        }
        assert_eq!(exec("library.places.list", json!({})), Verdict::Run { change: false });
        assert_eq!(exec("library.places.refresh", json!({})), Verdict::Run { change: true });
    }

    /// A library write changes files though it names only ids, so each asks first, and its
    /// question says what it changes; listing a library's entries only reads.
    #[test]
    fn library_writes_ask() {
        let steel = json!({"kind": "material", "value": {"name": "Steel"}});
        let long = format!("Steel\u{7}{}", "x".repeat(300));
        for (id, p, question) in [
            ("library.create", json!({"place": "/srv/lib", "name": "Shop"}), "library.create: /srv/lib, Shop"),
            ("library.entry.add", json!({"library": "user.shop", "definition": steel}), "library.entry.add: user.shop, Steel"),
            ("library.entry.add", json!({"library": "user.shop", "definition": {}}), "library.entry.add: user.shop"),
            (
                "library.entry.edit",
                json!({"entry": "user.shop/material-steel", "revision": "0", "definition": steel}),
                "library.entry.edit: user.shop/material-steel, Steel",
            ),
            ("library.entry.remove", json!({"entry": "user.shop/material-steel", "revision": "0"}), "library.entry.remove: user.shop/material-steel"),
            (
                "library.entry.copy",
                json!({"entry": "solidcraft.builtin/material-steel", "to": "user.shop"}),
                "library.entry.copy: solidcraft.builtin/material-steel, user.shop",
            ),
        ] {
            assert_eq!(exec(id, p.clone()), Verdict::Ask(question.into()), "{id} {p}");
        }
        // A hostile name is shown without its control characters and cut at 200 characters.
        let Verdict::Ask(question) = exec("library.create", json!({"place": "/srv/lib", "name": long})) else { panic!("a write asks") };
        assert_eq!(question, format!("library.create: /srv/lib, Steel{}", "x".repeat(195)));
        // Nothing named: the command alone, as before.
        assert_eq!(exec("library.entry.remove", json!({})), Verdict::Ask("library.entry.remove".into()));
        assert_eq!(exec("library.entry.list", json!({"library": "user.shop"})), Verdict::Run { change: false });
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
        // How a host page keeps the design (#8517) is not the agent's.
        for m in ["document.export", "document.import", "document.saved"] {
            assert!(matches!(request(m, &json!({}), 0), Verdict::Refuse(_)), "{m}");
        }
    }

    /// The spellings of `id` a client may send: other cases, surrounding whitespace (ASCII and
    /// Unicode), and both together. The engine runs each as `id`.
    fn spellings(id: &str) -> Vec<String> {
        let mixed: String = id.chars().enumerate().map(|(i, c)| if i % 2 == 0 { c.to_ascii_uppercase() } else { c }).collect();
        let upper = id.to_ascii_uppercase();
        vec![
            upper.clone(),
            mixed.clone(),
            format!(" {id} "),
            format!("\t{id}\n"),
            format!("\u{a0}{id}\u{2003}"),
            format!("  {upper}"),
            format!("{mixed}\r\n"),
            format!("\u{3000}{mixed}"),
        ]
    }

    /// Ids that look like `id` but are another string: a Cyrillic or fullwidth letter, a
    /// zero-width or a non-breaking character inside. The engine runs none of them, and the
    /// gate must not let them run either.
    fn look_alikes(id: &str) -> Vec<String> {
        let swap = |from: char, to: char| id.replacen(from, &to.to_string(), 1);
        let mut v = vec![
            swap('e', '\u{0435}'),
            swap('o', '\u{043e}'),
            swap('a', '\u{0430}'),
            swap('i', '\u{0456}'),
            swap('.', '\u{ff0e}'),
            format!("\u{200b}{id}"),
            format!("{id}\u{200b}"),
            id.replacen('.', ".\u{200d}", 1),
            id.replacen('.', "\u{a0}.", 1),
            id.chars().map(|c| if c.is_ascii_lowercase() { char::from_u32(c as u32 - 'a' as u32 + 0xff41).unwrap_or(c) } else { c }).collect(),
            id.to_uppercase().replacen('I', "\u{130}", 1),
            id.replacen('k', "\u{212a}", 1),
        ];
        v.retain(|s| !s.is_ascii());
        v
    }

    fn runs(v: &Verdict) -> bool {
        matches!(v, Verdict::Run { .. })
    }

    /// A protected command spelled in another case, or with whitespace
    /// around it, gets the same verdict as its registered id: it never runs without the question,
    /// and what never runs for the agent still never does. Through `engine.execute` (by `command`
    /// or `id`) and inside a batch.
    #[test]
    fn protected_commands_in_any_spelling_get_the_same_verdict() {
        let protected = [
            ("file.open_in_easyprint", json!({"bodies": ["Body"], "consent": true})),
            ("diagnostics.crash_reports", json!({})),
            ("diagnostics.crash_report", json!({"id": "x"})),
            ("diagnostics.crash_reports.clear", json!({})),
            ("file.new", json!({})),
            ("file.save", json!({"path": "part.sc"})),
            ("file.save", json!({})),
            ("file.save_as", json!({"path": "part.sc"})),
            ("doc.open", json!({"path": "other.sc"})),
            ("file.export", json!({"path": "x.stl"})),
            ("file.send_to_slicer", json!({"slicer": "prusa"})),
            ("render.export", json!({})),
        ];
        let same = |a: &Verdict, b: &Verdict| std::mem::discriminant(a) == std::mem::discriminant(b);
        for (id, params) in &protected {
            let canonical = exec(id, params.clone());
            assert!(!runs(&canonical), "{id} must not run unasked: {canonical:?}");
            for spelled in spellings(id) {
                let by_command = exec(&spelled, params.clone());
                assert!(same(&by_command, &canonical), "{spelled:?}: {by_command:?}, but {id}: {canonical:?}");
                let by_id = request("engine.execute", &json!({"id": spelled, "params": params}), 5);
                assert!(same(&by_id, &canonical), "id {spelled:?}: {by_id:?}");
                let batch = json!({"commands": [{"command": "solid.box", "params": {}}, {"command": spelled, "params": params}]});
                let in_batch = request("engine.batch", &batch, 0);
                assert!(same(&in_batch, &canonical), "batch {spelled:?}: {in_batch:?}");
                let batch = json!({"commands": [{"id": spelled, "params": params}]});
                assert!(!runs(&request("engine.batch", &batch, 0)), "batch id {spelled:?}");
            }
        }
        // Undo past the turn's own changes asks however it is spelled, and an undo in another
        // case counts down the batch's own steps like `edit.undo`.
        for spelled in spellings("edit.undo") {
            assert!(matches!(request("engine.execute", &json!({"command": spelled}), 0), Verdict::Ask(_)), "{spelled:?}");
            let batch = json!({"commands": [{"command": "solid.box"}, {"command": spelled}, {"command": spelled}]});
            assert!(matches!(request("engine.batch", &batch, 0), Verdict::Ask(_)), "{spelled:?}");
        }
        // A read stays a read, and a change stays a change (the Ask mode asks for changes).
        for spelled in spellings("selection.get") {
            assert_eq!(exec(&spelled, json!({})), Verdict::Run { change: false }, "{spelled:?}");
        }
        for spelled in spellings("solid.box") {
            assert_eq!(exec(&spelled, json!({})), Verdict::Run { change: true }, "{spelled:?}");
        }
    }

    /// Look-alike ids name no command: the engine would not run them, and the gate does not let
    /// them run either (they used to pass as unknown modelling commands).
    #[test]
    fn look_alike_ids_never_run() {
        for id in
            ["file.open_in_easyprint", "diagnostics.crash_reports", "file.new", "file.save", "file.save_as", "doc.open", "solid.box", "edit.undo"]
        {
            for alike in look_alikes(id) {
                assert!(solidcraft_engine::find_command(&alike).is_none(), "{alike:?} resolves");
                let v = exec(&alike, json!({"path": "part.sc"}));
                assert!(matches!(v, Verdict::Unknown(_)), "{alike:?}: {v:?}");
                let batch = json!({"commands": [{"command": "solid.box"}, {"command": alike}]});
                assert!(matches!(request("engine.batch", &batch, 0), Verdict::Unknown(_)), "batch {alike:?}");
            }
        }
        assert!(matches!(exec("", json!({})), Verdict::Unknown(_)));
        assert!(matches!(request("engine.execute", &json!({"params": {}}), 0), Verdict::Unknown(_)));
        assert!(matches!(exec("solid.extrud", json!({})), Verdict::Unknown(e) if e.contains("solid.extrude")));
    }

    /// What runs after the gate is the id the gate decided on: `canonicalize` rewrites every
    /// command a request runs to its registered id (and the keys "Always allow" remembers match).
    #[test]
    fn requests_run_the_id_the_gate_decided_on() {
        let mut one = json!({"id": " FILE.Open_In_EasyPrint\u{a0}", "params": {"consent": true}});
        canonicalize("engine.execute", &mut one);
        assert_eq!(one, json!({"command": "file.open_in_easyprint", "params": {"consent": true}}));
        assert_eq!(tool_key("engine.execute", &json!({"command": "FILE.SAVE"})), "file.save");
        let mut batch = json!({"commands": [{"command": "SOLID.BOX"}, {"id": "\tdiagnostics.CRASH_REPORTS"}, {"command": "fil\u{0435}.save"}, {"command": "view.pick"}]});
        canonicalize("engine.batch", &mut batch);
        assert_eq!(
            batch,
            json!({"commands": [{"command": "solid.box"}, {"command": "diagnostics.crash_reports"}, {"command": "fil\u{0435}.save"}, {"command": "view.pick"}]})
        );
        assert_eq!(describe("engine.batch", &json!({"commands": [{"command": "FILE.NEW"}]})), "batch: file.new");
        // The window's own pick has no engine entry and still runs.
        assert_eq!(exec("view.pick", json!({"x": 1, "y": 1})), Verdict::Run { change: true });
    }

    /// "Always allow" for a batch that asks must cover each command that asked, by its own id.
    #[test]
    fn a_batch_that_asks_is_keyed_by_the_commands_that_ask() {
        let batch = json!({"commands": [
            {"command": "solid.box"},
            {"command": "FILE.SAVE", "params": {"path": "a.sc"}},
            {"id": "file.export", "params": {"path": "a.stl"}},
            {"command": "file.save", "params": {"path": "b.sc"}},
        ]});
        assert_eq!(asking_keys("engine.batch", &batch, 0), ["file.save", "file.export"]);
        assert_eq!(asking_keys("engine.batch", &json!({"commands": [{"command": "solid.box"}]}), 0), ["batch"]);
        assert_eq!(asking_keys("engine.execute", &json!({"command": "File.Save"}), 0), ["file.save"]);
    }

    #[test]
    fn external_plugin_code_always_requires_consent() {
        crate::i18n::install();
        for id in ["plugin.run", "PLUGIN.RUN", "Plugin.Run"] {
            let verdict = exec(id, json!({"path":"sample","approval":"from-inspect","entry":"make"}));
            let Verdict::Ask(question) = verdict else { panic!("external plugin needs consent") };
            assert!(question.contains("sample") && question.contains("make"), "{question}");
        }
        assert!(matches!(exec("plugin.inspect", json!({"path":"sample"})), Verdict::Ask(_)));
        for id in ["plugin.check", "PLUGIN.CHECK", "Plugin.Check", "Plugin.Inspect"] {
            assert!(matches!(exec(id, json!({"path":"sample"})), Verdict::Ask(_)), "{id}: checking a package reads its files");
            assert!(reads_only(id) && is_file_verb(id), "{id} reads, and only reads, a file");
        }
    }

    /// A plugin's code runs only after the user's Allow: `plugin.run` in any spelling, alone or
    /// in a batch, is a request that runs a plugin (which no permission mode or "Always allow"
    /// answers for the user); and a plugin entry the gate can't place (a package a batch's own
    /// `plugin.inspect` would register first) never runs.
    #[test]
    fn plugin_code_never_runs_unasked_from_a_batch() {
        crate::i18n::install();
        let run = json!({"path": "sample", "approval": "from-inspect", "entry": "make"});
        for spelled in std::iter::once("plugin.run".to_string()).chain(spellings("plugin.run")) {
            assert!(runs_plugin("engine.execute", &json!({"command": spelled, "params": run})), "{spelled:?}");
            assert!(runs_plugin("engine.execute", &json!({"id": spelled, "params": run})), "{spelled:?}");
            let batch = json!({"commands": [{"command": "solid.box"}, {"command": spelled, "params": run}]});
            assert!(runs_plugin("engine.batch", &batch), "{spelled:?}");
            assert!(matches!(request("engine.batch", &batch, 0), Verdict::Ask(_)), "{spelled:?}");
        }
        assert!(!runs_plugin("engine.batch", &json!({"commands": [{"command": "solid.box"}, {"command": "plugin.inspect"}]})));
        assert!(!runs_plugin("engine.execute", &json!({"command": "solid.box"})));
        // Inspect, then the package's entry in the same batch: the entry is not yet a command the
        // gate can place, so the batch is refused before anything runs.
        let inspect_then_run = json!({"commands": [
            {"command": "plugin.inspect", "params": {"path": "sample"}},
            {"command": "sample.make", "params": {"approval": "from-inspect", "params": {}}},
        ]});
        assert!(matches!(request("engine.batch", &inspect_then_run, 0), Verdict::Unknown(_)));
        assert!(matches!(exec("sample.make", json!({"approval": "x"})), Verdict::Unknown(_)));
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
    fn the_mcp_server_may_read_its_output_options() {
        assert_eq!(request("mcp.options", &json!({}), 0), Verdict::Run { change: false });
    }

    #[test]
    fn the_mcp_server_may_read_build_information_without_running_help_actions() {
        for params in [json!({}), json!({"item": null})] {
            assert_eq!(request("ui.help", &params, 0), Verdict::Run { change: false }, "{params}");
        }
        for item in ["about", "updates", "shortcuts", "report", "crashes", "docs", "messages", "install_cli", ""] {
            assert!(matches!(request("ui.help", &json!({"item": item}), 0), Verdict::Refuse(_)), "{item}");
        }
        for item in [json!(false), json!(0), json!([]), json!({})] {
            assert!(matches!(request("ui.help", &json!({"item": item}), 0), Verdict::Refuse(_)), "{item}");
        }
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
