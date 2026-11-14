//! Install plugin…: pick a plugin `.zip`, review what it asks for, and install it with
//! `plugin.install`. Desktop only; Preferences › Plugins and menus call [`start`]. The command itself never asks anything:
//! this question is the menu's.

use serde_json::{Value, json};
use solidcraft_ui_text::{format, tr};

use crate::{SolveApp, ui_kit};

/// The plugin waiting for the user's answer.
#[derive(Default)]
pub(crate) struct State {
    pub(crate) ask: Option<Ask>,
}

pub(crate) struct Ask {
    pub(crate) path: String,
    /// `plugin.install`'s dry-run reply.
    pub(crate) plugin: Value,
}

/// Whether this build installs plugins: the browser has no plugins folder.
pub fn available() -> bool {
    !cfg!(target_arch = "wasm32")
}

/// Install plugin…: the file dialog for a `.zip`, then the question.
pub fn start(app: &mut SolveApp) {
    if !available() {
        return;
    }
    app.pick_file(tr("plugin_install.zip_kind"), &["zip"], |app, path| {
        if let Some(path) = path {
            review(app, path);
        }
    });
}

/// Checks the `.zip` and asks; a `.zip` that can't be installed is said at once.
pub(crate) fn review(app: &mut SolveApp, path: String) {
    match app.run("plugin.install", json!({"path": path, "dry_run": true})) {
        Ok(plugin) => app.plugin_install.ask = Some(Ask { path, plugin }),
        Err(error) => app.set_status(format("plugin_install.failed", &[("error", &error)]), true),
    }
}

fn text(plugin: &Value, key: &str) -> String {
    plugin.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}
fn list(value: Option<&Value>) -> Vec<String> {
    value.and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default()
}

/// What the plugin asks for, one line each, in plain words.
pub(crate) fn permission_lines(plugin: &Value) -> Vec<String> {
    let permissions = plugin.get("permissions");
    let mut lines: Vec<String> = list(permissions.and_then(|p| p.get("command_groups")))
        .iter()
        .map(|group| match group.as_str() {
            "app" | "doc" | "file" | "plugin" => tr(&std::format!("plugin_install.group_{group}")).to_string(),
            other => other.to_string(),
        })
        .collect();
    for folder in permissions.and_then(|p| p.get("files")).and_then(Value::as_array).into_iter().flatten() {
        let name = folder.get("name").and_then(Value::as_str).unwrap_or_default();
        let write = folder.get("write").and_then(Value::as_bool).unwrap_or(false);
        lines.push(format(if write { "plugin_install.folder_write" } else { "plugin_install.folder_read" }, &[("name", name)]));
    }
    if permissions.and_then(|p| p.get("clock")).and_then(Value::as_bool).unwrap_or(false) {
        lines.push(tr("plugin_install.clock").to_string());
    }
    let commands = list(plugin.get("commands")).len();
    match commands {
        0 => {}
        1 => lines.push(tr("plugin_install.command_one").to_string()),
        n => lines.push(format("plugin_install.commands", &[("count", &n.to_string())])),
    }
    lines
}

/// The question, while a plugin waits for it.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(ask) = app.plugin_install.ask.as_ref() else { return };
    let _language = solidcraft_ui_text::scope(&app.preferences.language);
    let plugin = &ask.plugin;
    let (name, version, summary) = (text(plugin, "name"), text(plugin, "version"), text(plugin, "description"));
    let replaces = plugin.get("replaces").and_then(Value::as_str).map(str::to_string);
    let lines = permission_lines(plugin);
    let title = tr("plugin_install.title").to_string();
    let (install, cancel) = (tr("plugin_install.install").to_string(), tr("plugin_install.cancel").to_string());
    let (closed, action) = ui_kit::Confirm::new(egui::Id::unique("sc_plugin_install"), "import", &title)
        .tone(if replaces.is_some() { ui_kit::Tone::Warning } else { ui_kit::Tone::Info })
        .actions(&install, &cancel, None)
        .show(ctx, true, |ui| {
            ui_kit::text(ui, &std::format!("{name} {version}"), ui_kit::Text::Heading("plugin_install_name"));
            if !summary.is_empty() {
                ui_kit::text(ui, &summary, ui_kit::Text::Report("plugin_install_summary", false));
            }
            ui_kit::text(ui, tr("plugin_install.asks"), ui_kit::Text::Report("plugin_install_asks", false));
            if lines.is_empty() {
                ui_kit::text(ui, tr("plugin_install.asks_nothing"), ui_kit::Text::Hint("plugin_install_nothing"));
            }
            for (i, line) in lines.iter().enumerate() {
                ui.push_id(i, |ui| ui_kit::text(ui, &std::format!("• {line}"), ui_kit::Text::Hint("plugin_install_permission")));
            }
            if let Some(replaces) = &replaces {
                ui_kit::text(ui, &format("plugin_install.replaces", &[("plugin", replaces)]), ui_kit::Text::Report("plugin_install_replaces", true));
            }
        });
    match action {
        ui_kit::Action::Ok => {
            if let Some(ask) = app.plugin_install.ask.take() {
                install_now(app, &ask);
            }
        }
        ui_kit::Action::Cancel => app.plugin_install.ask = None,
        _ if closed => app.plugin_install.ask = None,
        _ => {}
    }
}

/// Installs the reviewed `.zip`; replacing what the question said it replaces.
fn install_now(app: &mut SolveApp, ask: &Ask) {
    let replace = ask.plugin.get("replaces").is_some_and(Value::is_string);
    match app.run("plugin.install", json!({"path": ask.path, "replace": replace})) {
        Ok(plugin) => {
            // The ribbon's Plugins group reads the installed plugins again.
            app.plugin_menu.loaded = false;
            let shown = std::format!("{} {}", text(&plugin, "name"), text(&plugin, "version"));
            app.set_status(format("plugin_install.done", &[("plugin", &shown)]), false);
        }
        Err(error) => app.set_status(format("plugin_install.failed", &[("error", &error)]), true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_plugin_reviews_then_installs_and_says_so() {
        let mut h = crate::scenario::Harness::new();
        let plugins = h.dir.join("plugins");
        h.app.session.plugin_state.plugins_dir = Some(plugins.clone());
        let manifest = json!({"id":"gears","name":"Gears","version":"0.1.0","description":"Spur gears","capabilities":["commands@0.1"],
            "runtime":{"module":"gears.wasm","entries":["make"],"commands":["solid.box","file.export_stl"],
            "permissions":{"command_groups":["file"],"files":[{"name":"exports","write":true}]}}});
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, bytes) in [("gears/plugin.json", serde_json::to_vec(&manifest).unwrap()), ("gears/gears.wasm", b"\0asm".to_vec())] {
            zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut zip, &bytes).unwrap();
        }
        let zip = zip.finish().unwrap().into_inner();
        let path = h.dir.join("gears.zip");
        std::fs::write(&path, zip).unwrap();
        review(&mut h.app, path.to_string_lossy().into_owned());
        let ask = h.app.plugin_install.ask.as_ref().expect("asks first");
        assert!(!plugins.join("gears").exists(), "nothing is written before the answer");
        let lines = permission_lines(&ask.plugin);
        assert_eq!(lines.len(), 3, "{lines:?}");
        let ask = h.app.plugin_install.ask.take().unwrap();
        install_now(&mut h.app, &ask);
        assert!(plugins.join("gears/gears.wasm").exists());
        assert!(h.app.session.plugin_state.packages.contains_key("gears"), "loaded without a restart");
        // A broken .zip is refused in plain words, with no question.
        let bad = h.dir.join("bad.zip");
        std::fs::write(&bad, b"not a zip").unwrap();
        review(&mut h.app, bad.to_string_lossy().into_owned());
        assert!(h.app.plugin_install.ask.is_none());
    }
}
