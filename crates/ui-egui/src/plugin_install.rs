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
    pub(crate) grants: solidcraft_engine::plugin::Grants,
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
    if let Err(error) =
        crate::plugins::start_package(app, "plugin.install", json!({"path": path, "dry_run": true}), crate::plugins::PackageAction::Describe { path })
    {
        app.set_status(format("plugin_install.failed", &[("error", &error)]), true);
    }
}

/// A checked package from the install command, with initially unfilled host folder paths.
pub(crate) fn described(app: &mut SolveApp, path: String, plugin: Value) {
    let permissions = plugin.get("permissions");
    let grants = solidcraft_engine::plugin::Grants {
        package_hash: None,
        command_groups: list(permissions.and_then(|p| p.get("command_groups"))).into_iter().collect(),
        folders: permissions
            .and_then(|p| p.get("files"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|folder| solidcraft_engine::plugin::FileGrant {
                name: text(folder, "name"),
                folder: Default::default(),
                read: folder.get("read").and_then(Value::as_bool).unwrap_or(false),
                write: folder.get("write").and_then(Value::as_bool).unwrap_or(false),
            })
            .collect(),
    };
    app.plugin_install.ask = Some(Ask { path, plugin, grants });
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
            "app" | "doc" | "file" | "plugin" => tr(&std::format!("prefs.plugin_group_{group}")).to_string(),
            other => other.to_string(),
        })
        .collect();
    for folder in permissions.and_then(|p| p.get("files")).and_then(Value::as_array).into_iter().flatten() {
        let name = folder.get("name").and_then(Value::as_str).unwrap_or_default();
        let read = folder.get("read").and_then(Value::as_bool).unwrap_or(false);
        let write = folder.get("write").and_then(Value::as_bool).unwrap_or(false);
        let access = tr(match (read, write) {
            (true, true) => "prefs.plugin_folder_read_write",
            (false, true) => "prefs.plugin_folder_write",
            _ => "prefs.plugin_folder_read",
        });
        lines.push(format("prefs.plugin_folder", &[("name", name), ("access", access)]));
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
    let Some(mut ask) = app.plugin_install.ask.take() else { return };
    let _language = solidcraft_ui_text::scope(&app.preferences.language);
    let plugin = &ask.plugin;
    let (name, version, summary) = (text(plugin, "name"), text(plugin, "version"), text(plugin, "description"));
    let replaces = plugin.get("replaces").and_then(Value::as_str).map(str::to_string);
    let lines = permission_lines(plugin);
    let title = tr("plugin_install.title").to_string();
    let (install, cancel) = (tr("plugin_install.install").to_string(), tr("plugin_install.cancel").to_string());
    let valid = crate::plugins::valid_grants(&ask.grants) && !crate::plugins::busy(app);
    let (closed, action) = ui_kit::Confirm::new(egui::Id::unique("sc_plugin_install"), "import", &title)
        .tone(if replaces.is_some() { ui_kit::Tone::Warning } else { ui_kit::Tone::Info })
        .actions(&install, &cancel, None)
        .show(ctx, valid, |ui| {
            egui::ScrollArea::vertical().max_height(ctx.content_rect().height() * 0.55).show(ui, |ui| {
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
                for grant in &mut ask.grants.folders {
                    let access = tr(match (grant.read, grant.write) {
                        (true, true) => "prefs.plugin_folder_read_write",
                        (false, true) => "prefs.plugin_folder_write",
                        _ => "prefs.plugin_folder_read",
                    });
                    let label = format("prefs.plugin_folder", &[("name", &grant.name), ("access", access)]);
                    let mut path = grant.folder.display().to_string();
                    ui_kit::text_row(ui, &label, &mut path, tr("prefs.plugin_folder_path"));
                    grant.folder = path.into();
                }
                if !ask.grants.folders.is_empty() {
                    ui_kit::text(ui, tr("prefs.plugin_folder_note"), ui_kit::Text::Hint("plugin_folder"));
                }
                let entries: Vec<solidcraft_engine::plugin::Entry> =
                    plugin.get("entry_points").cloned().and_then(|value| serde_json::from_value(value).ok()).unwrap_or_default();
                if !entries.is_empty() {
                    ui_kit::text(ui, tr("prefs.plugin_entries"), ui_kit::Text::Report("plugin_entries", false));
                }
                for entry in &entries {
                    ui_kit::text(ui, &entry.name, ui_kit::Text::Hint("plugin_entry"));
                    for slot in &entry.slots {
                        let (key, name) = match slot {
                            solidcraft_engine::plugin::UiSlot::Menu { name } => {
                                let name = match crate::plugin_slots::menu_name(name) {
                                    Some("File") => tr("ui.file"),
                                    Some("Help") => tr("ui.help"),
                                    _ => tr("ui.plugins"),
                                };
                                ("prefs.plugin_slot_menu", name.to_string())
                            }
                            solidcraft_engine::plugin::UiSlot::Ribbon { name } => (
                                "prefs.plugin_slot_ribbon",
                                crate::toolbar::ribbon_label(app, crate::plugin_slots::ribbon_name(name).unwrap_or("MODEL")),
                            ),
                        };
                        ui_kit::text(ui, &format(key, &[("name", &name)]), ui_kit::Text::Hint("plugin_entry_slot"));
                    }
                    if entry.slots.is_empty() {
                        ui_kit::text(ui, tr("prefs.plugin_slot_command"), ui_kit::Text::Hint("plugin_entry_command"));
                    }
                }
                if let Some(replaces) = &replaces {
                    ui_kit::text(
                        ui,
                        &format("plugin_install.replaces", &[("plugin", replaces)]),
                        ui_kit::Text::Report("plugin_install_replaces", true),
                    );
                }
            });
        });
    match action {
        ui_kit::Action::Ok => install_now(app, &ask),
        ui_kit::Action::Cancel => {}
        _ if closed => {}
        _ => app.plugin_install.ask = Some(ask),
    }
}

/// Installs the reviewed `.zip` with only the permissions and host folders shown in the question.
fn install_now(app: &mut SolveApp, ask: &Ask) {
    let replace = ask.plugin.get("replaces").is_some_and(Value::is_string);
    let folders: Vec<Value> = ask
        .grants
        .folders
        .iter()
        .map(|grant| {
            json!({
                "name":grant.name,"path":grant.folder,"read":grant.read,"write":grant.write
            })
        })
        .collect();
    if let Err(error) = crate::plugins::start_package(
        app,
        "plugin.install",
        json!({"path": ask.path, "approval": text(&ask.plugin, "approval"), "archive": text(&ask.plugin, "archive"), "replace": replace, "approve":{"command_groups":ask.grants.command_groups,"folders":folders}}),
        crate::plugins::PackageAction::Install,
    ) {
        app.set_status(format("plugin_install.failed", &[("error", &error)]), true);
    }
}

pub(crate) fn installed(app: &mut SolveApp, plugin: &Value) {
    let shown = std::format!("{} {}", text(plugin, "name"), text(plugin, "version"));
    app.set_status(format("plugin_install.done", &[("plugin", &shown)]), false);
}
#[cfg(test)]
mod tests {
    use super::*;

    fn capture_confirm(app: &mut SolveApp) {
        for (size, filename) in [(egui::vec2(1280.0, 720.0), "plugin-install-confirm.png"), (egui::vec2(640.0, 480.0), "plugin-install-small.png")] {
            let ctx = crate::fonts::test_context();
            let mut textures = std::collections::HashMap::new();
            for frame in 0..6 {
                let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                let mut output =
                    ctx.run_ui(egui::RawInput { screen_rect: Some(bounds), time: Some(frame as f64 / 60.0), ..Default::default() }, |ui| {
                        crate::ui_kit::scope(ui, |ui| show(app, ui.ctx()));
                    });
                crate::ui_kit::test_textures_update(&mut textures, &output.textures_delta);
                output.textures_delta.clear();
                if frame == 5 {
                    let text = crate::test_support::text_shapes(&output).map(|shape| shape.galley.text().to_owned()).collect::<Vec<_>>().join("\n");
                    assert!(text.contains("Install") && text.contains("exports (write)"), "{text}");
                    if size.x > 1000.0 {
                        assert!(text.contains("Menu: Plugins") && text.contains("Ribbon: Plugins"), "{text}");
                    }
                    if let Ok(directory) = std::env::var("SOLIDCRAFT_PLUGINS_EVIDENCE") {
                        let image = crate::ui_kit::test_raster(&ctx, output.shapes, output.pixels_per_point, bounds, &textures);
                        std::fs::create_dir_all(&directory).unwrap();
                        image.save(std::path::Path::new(&directory).join(filename)).unwrap();
                    }
                }
            }
        }
    }

    #[test]
    fn plugin_install_permissions_use_plain_words_and_exact_folder_access() {
        // Each test runs in its own process under nextest: load the catalogs, or `tr` returns ids.
        crate::i18n::install();
        let plugin = json!({"permissions":{"command_groups":["app","doc","file","plugin"],"files":[
            {"name":"source","read":true,"write":false},
            {"name":"output","read":false,"write":true},
            {"name":"project","read":true,"write":true}
        ]}});
        let lines = permission_lines(&plugin);
        assert_eq!(
            lines,
            [
                "Change application settings and use application diagnostics.",
                "Open, recover or switch designs.",
                "Read or write files within the approved folders.",
                "Inspect, install, remove or run other plugins.",
                "source (read)",
                "output (write)",
                "project (read and write)"
            ]
        );
    }

    #[test]
    fn install_plugin_reviews_then_installs_and_says_so() {
        let mut h = crate::scenario::Harness::new();
        let plugins = h.dir.join("plugins");
        h.app.session.plugin_state.plugins_dir = Some(plugins.clone());
        let manifest = json!({"id":"gears","name":"Gears","version":"0.1.0","description":"Spur gears","capabilities":["commands@0.1"],
            "runtime":{"module":"gears.wasm","entries":[{"name":"make","slots":[{"kind":"menu","name":"Manufacturing"},{"kind":"ribbon","name":"Manufacturing"}]}],"commands":["solid.box","file.export_stl"],
            "permissions":{"command_groups":["file"],"files":[{"name":"exports","write":true}]}}});
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, bytes) in [("gears/plugin.json", serde_json::to_vec(&manifest).unwrap()), ("gears/gears.wasm", b"\0asm".to_vec())] {
            zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut zip, &bytes).unwrap();
        }
        let zip = zip.finish().unwrap().into_inner();
        let path = h.dir.join("gears.zip");
        std::fs::write(&path, &zip).unwrap();
        review(&mut h.app, path.to_string_lossy().into_owned());
        crate::plugins::tick(&mut h.app, &h.ctx, true);
        let ask = h.app.plugin_install.ask.as_ref().expect("asks first");
        assert!(!plugins.join("gears").exists(), "nothing is written before the answer");
        let lines = permission_lines(&ask.plugin);
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(!crate::plugins::valid_grants(&ask.grants));
        // The screenshot shows a folder a person would pick; the install below uses the test's own folder.
        h.app.plugin_install.ask.as_mut().unwrap().grants.folders[0].folder = std::path::PathBuf::from("/home/maker/Documents/Gear exports");
        capture_confirm(&mut h.app);
        h.app.plugin_install.ask.as_mut().unwrap().grants.folders[0].folder = h.dir.clone();
        let ask = h.app.plugin_install.ask.take().unwrap();
        let reviewed = ask.plugin["approval"].as_str().unwrap();
        assert!(!reviewed.is_empty());
        let mut swapped = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, bytes) in [("gears/plugin.json", serde_json::to_vec(&manifest).unwrap()), ("gears/gears.wasm", b"changed component".to_vec())] {
            swapped.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut swapped, &bytes).unwrap();
        }
        std::fs::write(&path, swapped.finish().unwrap().into_inner()).unwrap();
        install_now(&mut h.app, &ask);
        crate::plugins::tick(&mut h.app, &h.ctx, true);
        assert!(!plugins.exists(), "the UI submits the reviewed hash and refuses a swapped archive");
        assert!(h.app.session.plugin_state.grants.is_empty());
        std::fs::write(&path, zip).unwrap();
        install_now(&mut h.app, &ask);
        crate::plugins::tick(&mut h.app, &h.ctx, true);
        assert!(plugins.join("gears/gears.wasm").exists());
        assert!(h.app.session.plugin_state.packages.contains_key("gears"), "loaded without a restart");
        let grants = &h.app.preferences.plugin_grants["gears"];
        assert!(grants.command_groups.contains("file"));
        assert_eq!(grants.folders[0].folder, h.dir);
        // A broken .zip is refused in plain words, with no question.
        let bad = h.dir.join("bad.zip");
        std::fs::write(&bad, b"not a zip").unwrap();
        review(&mut h.app, bad.to_string_lossy().into_owned());
        crate::plugins::tick(&mut h.app, &h.ctx, true);
        assert!(h.app.plugin_install.ask.is_none());
    }
}
