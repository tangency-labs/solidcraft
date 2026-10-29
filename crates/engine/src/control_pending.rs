//! What a client is told about a control request that outlasts its wait (docs/control-protocol.md,
//! #1618): the app's control server and the bridged MCP server both say what the request still
//! does when it finishes: nothing (it only reads), write a file (an export or a save), or change
//! the design.

use serde_json::Value;

/// What a request still running does when it finishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// It only reads (the design, the window or the command list): nothing changes.
    ReadsOnly,
    /// It writes a file (an export, a save, a render to a path); the design stays as it is.
    WritesFile,
    /// It changes the design (or may: anything not known to read or only write a file).
    Changes,
}

impl Effect {
    /// What happens when the request finishes, for a message: "it only reads: nothing changes",
    /// "it was not cancelled and will still write the file when it finishes", …
    pub fn when_done(self) -> &'static str {
        match self {
            Effect::ReadsOnly => "it only reads: nothing changes",
            Effect::WritesFile => "it was not cancelled and will still write the file when it finishes; the design does not change",
            Effect::Changes => "it was not cancelled and will still apply when it finishes",
        }
    }
}

/// Control methods that only read: a timed-out one changes nothing when it finishes.
/// (`ui.screenshot` with a `path`, and `ui.render`, write a file: see [`effect`].)
const READ_METHODS: &[&str] = &[
    "control.pending",
    "control.subscribe",
    "document.inspect",
    "document.revision",
    "engine.commands",
    "ui.at",
    "ui.handles",
    "ui.inspect",
    "ui.selection",
    "ui.sketchToScreen",
    "ui.worldToScreen",
];

/// Commands that write a file and leave the design as it is. `file.save` and `file.save_as`
/// count: they mark the design saved, but change nothing in it.
const FILE_COMMANDS: &[&str] = &["file.export", "file.save", "file.save_as", "file.save_mesh"];

/// Commands that only read the design (inspection, lists, measurements).
pub fn command_reads_only(id: &str) -> bool {
    let id = crate::find_command(id).map_or(id, |c| c.id);
    id.starts_with("inspect.")
        || id.starts_with("model.")
        || id.ends_with(".inspect")
        || id.ends_with(".list")
        || id == "document.revision"
        || matches!(id, "document.inspect" | "engine.commands" | "render.status" | "render.backends" | "render.settings.get" | "file.slicers")
}

/// What the command `id` with `params` does: an export that is given a `path` (`*.export*`, as
/// `sketch.export_dxf` or `parameters.export`) or a save writes a file; without a path an export
/// only answers with its contents.
pub fn command_effect(id: &str, params: &Value) -> Effect {
    let id = crate::find_command(id).map_or(id, |c| c.id);
    let exports = id.split('.').skip(1).any(|part| part.starts_with("export"));
    if FILE_COMMANDS.contains(&id) || (exports && params.get("path").is_some_and(|p| !p.is_null())) {
        Effect::WritesFile
    } else if command_reads_only(id) || exports {
        Effect::ReadsOnly
    } else {
        Effect::Changes
    }
}

/// What the request `method` with `params` does when it finishes. Renders and screenshots
/// change nothing in the design; given a `path` they write it.
pub fn effect(method: &str, params: &Value) -> Effect {
    match method {
        "engine.execute" | "command" => match params.get("command").or_else(|| params.get("id")).and_then(Value::as_str) {
            Some(id) => command_effect(id, params.get("params").unwrap_or(&Value::Null)),
            None => Effect::Changes,
        },
        "ui.render" => Effect::WritesFile,
        "ui.screenshot" if params.get("path").is_some_and(|p| !p.is_null()) => Effect::WritesFile,
        "ui.screenshot" => Effect::ReadsOnly,
        m if READ_METHODS.contains(&m) => Effect::ReadsOnly,
        _ => Effect::Changes,
    }
}

/// Whether the request `method` with `params` only reads.
pub fn reads_only(method: &str, params: &Value) -> bool {
    effect(method, params) == Effect::ReadsOnly
}

/// The request in a message: "`engine.execute` (solid.fillet)", or just "`ui.inspect`".
pub fn describe(method: &str, params: &Value) -> String {
    let command = matches!(method, "engine.execute" | "command")
        .then(|| params.get("command").or_else(|| params.get("id")).and_then(Value::as_str))
        .flatten()
        .filter(|c| c.len() <= 128);
    match command {
        Some(c) => format!("`{method}` ({c})"),
        None => format!("`{}`", method.chars().take(128).collect::<String>()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_are_told_from_changes() {
        for (m, p) in [
            ("ui.inspect", json!({})),
            ("ui.screenshot", json!({})),
            ("document.inspect", json!({"measure": true})),
            ("engine.execute", json!({"command": "sketch.inspect"})),
            ("engine.execute", json!({"command": "inspect.measure"})),
            ("engine.execute", json!({"command": "model.edges", "params": {"body": "Body1"}})),
            ("engine.execute", json!({"command": "parameters.export", "params": {"format": "csv"}})),
        ] {
            assert!(reads_only(m, &p), "{m} {p}");
            assert_eq!(effect(m, &p), Effect::ReadsOnly, "{m} {p}");
        }
        for (m, p) in [
            ("engine.execute", json!({"command": "solid.fillet"})),
            ("engine.execute", json!({})),
            ("engine.batch", json!({"commands": []})),
            ("ui.click", json!({"x": 1, "y": 2})),
            ("app.quit", json!({})),
        ] {
            assert!(!reads_only(m, &p), "{m} {p}");
            assert_eq!(effect(m, &p), Effect::Changes, "{m} {p}");
        }
        assert_eq!(describe("engine.execute", &json!({"command": "solid.fillet"})), "`engine.execute` (solid.fillet)");
        assert_eq!(describe("ui.inspect", &json!({})), "`ui.inspect`");
    }

    /// An export or a save writes a file and leaves the design as it is (#1989: a STEP export
    /// is not said to "still apply").
    #[test]
    fn file_writers_are_told_apart() {
        for (m, p) in [
            ("engine.execute", json!({"command": "file.export", "params": {"path": "/tmp/a.step"}})),
            ("engine.execute", json!({"command": "file.save", "params": {}})),
            ("engine.execute", json!({"command": "file.save_as", "params": {"path": "/tmp/a.solidcraft"}})),
            ("engine.execute", json!({"command": "file.save_mesh", "params": {"path": "/tmp/a.stl"}})),
            ("engine.execute", json!({"command": "sketch.export_dxf", "params": {"path": "/tmp/a.dxf"}})),
            ("ui.render", json!({"path": "/tmp/a.png"})),
            ("ui.screenshot", json!({"path": "/tmp/a.png"})),
        ] {
            assert_eq!(effect(m, &p), Effect::WritesFile, "{m} {p}");
            assert!(!reads_only(m, &p), "{m} {p}");
        }
        assert!(Effect::WritesFile.when_done().contains("will still write the file") && !Effect::WritesFile.when_done().contains("apply"));
        assert!(Effect::Changes.when_done().contains("will still apply"));
        assert!(Effect::ReadsOnly.when_done().contains("nothing changes"));
    }
}
