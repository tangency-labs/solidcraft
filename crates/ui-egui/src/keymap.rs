//! Keyboard shortcuts: every command's default key comes from its `CommandSpec`; the user can
//! rebind or clear any of them (Preferences › Keyboard Shortcuts). A new key that another command
//! already uses is reported and only taken over on request; keys the app itself handles (Esc, S,
//! V, Delete, Ctrl+Z…) can't be bound. Changes are kept between runs.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use solidcraft_engine::command_specs;

use crate::SolveApp;
use crate::ui_kit;

/// Repeats the last tool or dialog started (`SolveApp::repeat_last`, #3045).
pub const REPEAT_KEY: &str = "Shift+R";

/// Keys the application handles before any command shortcut.
pub const RESERVED: &[&str] = &[
    REPEAT_KEY,
    "Escape",
    "S",
    "V",
    "F2",
    "F6",
    "Delete",
    "Backspace",
    "Ctrl+Z",
    "Ctrl+Y",
    "Ctrl+S",
    "Ctrl+N",
    "Ctrl+O",
    "Ctrl+Comma",
    "Ctrl+Shift+A",
    "Ctrl+C",
    "Ctrl+X",
    "Ctrl+V",
    // View keys (`navigation::view_key`).
    "Left",
    "Right",
    "Up",
    "Down",
    "Shift+Left",
    "Shift+Right",
    "Shift+Up",
    "Shift+Down",
    "Ctrl+Left",
    "Ctrl+Right",
    "Ctrl+Up",
    "Ctrl+Down",
    "Ctrl+8",
];

#[derive(Default)]
pub struct Keymap {
    /// Command id → key (`""`: no key). Only the changed ones.
    pub custom: BTreeMap<String, String>,
    /// Asks for the Keyboard Shortcuts page of Preferences (File and Help menus).
    pub open: bool,
    /// Opened from Help: a list to read, not to edit.
    pub read_only: bool,
    filter: String,
    /// The command waiting for a key press.
    capturing: Option<String>,
    /// A key that another command uses: (command, key, the other commands).
    conflict: Option<(String, String, Vec<String>)>,
    message: Option<String>,
}

impl Keymap {
    pub fn prefs(&self) -> Value {
        json!(self.custom)
    }
    pub fn load(&mut self, v: &Value) {
        self.custom =
            v.as_object().map(|o| o.iter().take(2000).filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect()).unwrap_or_default();
    }
}

/// A key press as a shortcut name: `E`, `Shift+E`, `Ctrl+Alt+K`, `F5`, `Delete`.
pub fn key_name(key: egui::Key, mods: egui::Modifiers) -> String {
    let mut s = String::new();
    if mods.command || mods.ctrl {
        s.push_str("Ctrl+");
    }
    if mods.alt {
        s.push_str("Alt+");
    }
    if mods.shift {
        s.push_str("Shift+");
    }
    s.push_str(key.name());
    s
}

/// The key a command runs from now.
pub fn effective(app: &SolveApp, id: &str) -> Option<String> {
    match app.keymap.custom.get(id) {
        Some(k) if k.is_empty() => None,
        Some(k) => Some(k.clone()),
        None => solidcraft_engine::find_command(id).and_then(|c| c.shortcut).map(str::to_string),
    }
}

/// Commands bound to `key` now.
pub fn commands_for(app: &SolveApp, key: &str) -> Vec<&'static str> {
    command_specs().into_iter().filter(|c| effective(app, c.id).as_deref() == Some(key)).map(|c| c.id).collect()
}

/// The enabled command a key press runs, if any.
pub fn command_for(app: &SolveApp, key: &str) -> Option<&'static str> {
    commands_for(app, key).into_iter().find(|id| solidcraft_engine::find_command(id).is_some_and(|c| c.info(&app.session).enabled))
}

/// Run a shortcut key: its enabled command; outside a sketch, a sketch tool's key opens Create
/// Sketch (on the selected plane or planar face, else on the plane picked next) and the tool
/// starts in the new sketch.
pub fn press(app: &mut SolveApp, key: &str) {
    if let Some(id) = command_for(app, key) {
        app.start(app.preferences.sketch_variants.resolve(id));
    } else if let Some(id) = sketch_tool_for(app, key) {
        app.start("sketch.create");
        if let Some(d) = app.dialog.as_mut().filter(|d| matches!(d.kind, crate::dialogs::Kind::Sketch)) {
            d.then_start = Some(app.preferences.sketch_variants.resolve(id));
        }
    }
}

/// Outside a sketch: the SKETCH-tab command bound to `key` that a new sketch would start. Not
/// Normal/Construction (a mode of the drawing in progress) nor the Finish Sketch panel.
fn sketch_tool_for(app: &SolveApp, key: &str) -> Option<&'static str> {
    if app.session.active_sketch.is_some() {
        return None;
    }
    commands_for(app, key).into_iter().find(|id| {
        solidcraft_engine::find_command(id).is_some_and(|c| c.tab == "SKETCH" && c.panel != "FINISH SKETCH" && c.id != "sketch.construction")
    })
}

/// Bind `key` to a command (`""` clears it). Refused for reserved keys, and when other commands
/// use the key unless `replace` (they lose it).
pub fn bind(app: &mut SolveApp, id: &str, key: &str, replace: bool) -> Result<(), String> {
    let spec = solidcraft_engine::find_command(id).ok_or_else(|| format!("no command `{id}`"))?;
    if RESERVED.contains(&key) {
        return Err(format!("{key} is used by SolidCraft itself"));
    }
    if !key.is_empty() {
        let others: Vec<&'static str> = commands_for(app, key).into_iter().filter(|o| *o != id).collect();
        if !others.is_empty() && !replace {
            let names: Vec<&str> = others.iter().filter_map(|o| solidcraft_engine::find_command(o).map(|c| c.label)).collect();
            return Err(format!("{key} is already used by {}", names.join(", ")));
        }
        for o in others {
            app.keymap.custom.insert(o.to_string(), String::new());
        }
    }
    if spec.shortcut.unwrap_or("") == key {
        app.keymap.custom.remove(id);
    } else {
        app.keymap.custom.insert(id.to_string(), key.to_string());
    }
    Ok(())
}

pub fn reset(app: &mut SolveApp, id: Option<&str>) {
    match id {
        Some(id) => drop(app.keymap.custom.remove(id)),
        None => app.keymap.custom.clear(),
    }
}

/// The Keyboard Shortcuts page of Preferences: search, rebind (click, then press the key), clear,
/// reset; a key another command uses is reported and taken over only on request.
pub fn page(app: &mut SolveApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    // A key press while waiting for one binds it (Esc cancels).
    if let Some(id) = app.keymap.capturing.clone() {
        let press = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
                _ => None,
            })
        });
        if let Some((k, m)) = press {
            app.keymap.capturing = None;
            if k != egui::Key::Escape {
                let name = key_name(k, m);
                match bind(app, &id, &name, false) {
                    Ok(()) => app.keymap.message = None,
                    Err(e) if e.contains("already used") => {
                        app.keymap.conflict = Some((id.clone(), name.clone(), commands_for(app, &name).iter().map(|s| s.to_string()).collect()));
                        app.keymap.message = Some(e);
                    }
                    Err(e) => app.keymap.message = Some(e),
                }
            }
        }
    }
    let mut action: Option<(String, &str)> = None;
    ui_kit::scope(ui, |ui| {
        let actions = [
            ui_kit::SearchAction { label: app.tr_label("Reset All"), icon: None, visible: !app.keymap.read_only, tooltip: None },
            ui_kit::SearchAction { label: app.tr_label("Import…"), icon: Some("import_tray"), visible: !app.keymap.read_only, tooltip: None },
            ui_kit::SearchAction {
                label: app.tr_label("Export…"),
                icon: Some("export_tray"),
                visible: true,
                tooltip: Some(app.tr_label("Keys, S box favourites and the toolbar, as a file")),
            },
        ];
        let (filter, toolbar_action) = ui_kit::search_actions_row(
            ui,
            egui::Id::new("sc_keymap_filter"),
            &mut app.keymap.filter,
            crate::i18n::tr_label("Filter commands or keys"),
            &actions,
        );
        crate::scenario::publish_target("keymap:filter", filter.rect);
        if let Some(index) = toolbar_action {
            action = Some((
                String::new(),
                match index {
                    0 => "reset_all",
                    1 => "import",
                    _ => "export",
                },
            ));
        }
        if let Some(m) = &app.keymap.message {
            ui_kit::text(ui, m, ui_kit::Text::Report("keymap_message", true));
        }
        if let Some((id, key, _)) = app.keymap.conflict.clone() {
            ui.horizontal_wrapped(|ui| {
                let replace = ui_kit::button(ui, crate::i18n::format("keymap.use_anyway", &[("key", &key)]), ui_kit::Button::Normal);
                crate::scenario::publish_target("keymap:replace", replace.rect);
                if replace.clicked() {
                    action = Some((format!("{id}\n{key}"), "replace"));
                }
                if ui_kit::button(ui, app.tr_label("Cancel"), ui_kit::Button::Normal).clicked() {
                    action = Some((String::new(), "cancel"));
                }
            });
        }
        ui_kit::text(ui, app.tr_label("Select Other: hold the left button still on the model (or right-click › Select Other…) to list everything under the cursor; click again on the same spot to select the next thing there. Double-click the middle button to go Home (Preferences › Navigation)."), ui_kit::Text::Note("navigation"));
        let q = app.keymap.filter.to_lowercase();
        let commands = command_specs();
        let t = crate::theme::Tokens::get();
        let key_width = commands
            .iter()
            .map(|c| {
                let key = effective(app, c.id).unwrap_or_default();
                ui_kit::ContentWidth::button(ui, &key)
            })
            .fold(ui_kit::ContentWidth::button(ui, app.tr_label("press a key…")), f32::max)
            .max(ui_kit::ContentWidth::label(ui, app.tr_label("Keyboard Shortcuts")) + t.space_8);
        let action_width = if app.keymap.read_only {
            t.control_height
        } else {
            ui_kit::ContentWidth::button(ui, app.tr_label("Clear")) + ui_kit::ContentWidth::button(ui, app.tr_label("Reset")) + t.space_4
        };
        ui_kit::settings_scroll(ui, "sc_keymap_scroll", |ui| {
            let columns = [
                ui_kit::Column::new("Command", 1.5).flex().min_content_width(t.form_label_width),
                ui_kit::Column::new(crate::i18n::tr("keymap.command_id"), 1.8).flex().min_content_width(t.form_label_width),
                ui_kit::Column::new("Keyboard Shortcuts", 1.0).flex().min_content_width(key_width),
                ui_kit::Column::new("", 0.0).icon_only().min_content_width(action_width),
            ];
            ui_kit::table(ui, "keymap", &columns, crate::i18n::tr("keymap.empty"), |ui, table| {
                for c in commands {
                    let key = effective(app, c.id).unwrap_or_default();
                    let translated_label = app.tr_label(c.label);
                    if !q.is_empty() && ![c.label, c.id, key.as_str(), translated_label].iter().any(|s| s.to_lowercase().contains(&q)) {
                        continue;
                    }
                    table.row(ui, c.id, app.keymap.capturing.as_deref() == Some(c.id), |row| {
                        row.cell(|ui| {
                            ui_kit::text(ui, translated_label, ui_kit::Text::Cell);
                        });
                        row.cell(|ui| {
                            ui_kit::text(ui, c.id, ui_kit::Text::Cell);
                        });
                        let capturing = app.keymap.capturing.as_deref() == Some(c.id);
                        let changed = app.keymap.custom.contains_key(c.id);
                        let text = if capturing {
                            app.tr_label("press a key…").to_string()
                        } else if key.is_empty() {
                            "—".to_string()
                        } else {
                            key.clone()
                        };
                        row.cell(|ui| {
                            if app.keymap.read_only {
                                ui_kit::text(ui, &text, ui_kit::Text::Cell);
                            } else {
                                let capture = if key.is_empty() && !capturing {
                                    let (rect, _) = ui.allocate_exact_size(egui::vec2(t.control_height, t.control_height), egui::Sense::hover());
                                    ui_kit::quiet_icon_button_at(ui, rect, "plus", app.tr_label("Click, then press the new key"))
                                } else {
                                    ui_kit::button(ui, &text, ui_kit::Button::Normal).on_hover_text(app.tr_label("Click, then press the new key"))
                                };
                                crate::scenario::publish_target(&format!("keymap:capture:{}", c.id), capture.rect);
                                if capture.clicked() {
                                    action = Some((c.id.to_string(), "capture"));
                                }
                            }
                        });
                        row.cell(|ui| {
                            if !app.keymap.read_only {
                                ui.horizontal(|ui| {
                                    let clear = ui_kit::button(ui, app.tr_label("Clear"), ui_kit::Button::Enabled(!key.is_empty()));
                                    crate::scenario::publish_target(&format!("keymap:clear:{}", c.id), clear.rect);
                                    if clear.clicked() {
                                        action = Some((c.id.to_string(), "clear"));
                                    }
                                    let reset = ui_kit::button(ui, app.tr_label("Reset"), ui_kit::Button::Enabled(changed));
                                    crate::scenario::publish_target(&format!("keymap:reset:{}", c.id), reset.rect);
                                    if reset.clicked() {
                                        action = Some((c.id.to_string(), "reset"));
                                    }
                                });
                            }
                        });
                    });
                }
            });
        });
    });
    match action {
        Some((id, "capture")) => {
            app.keymap.capturing = Some(id);
            app.keymap.conflict = None;
            app.keymap.message = None;
        }
        Some((id, "clear")) => drop(bind(app, &id, "", false)),
        Some((id, "reset")) => reset(app, Some(&id)),
        Some((_, "reset_all")) => reset(app, None),
        Some((_, "export")) => {
            let text = crate::toolbar_custom::export(app).to_string();
            app.pick_save("SolidCraft shortcuts.json", &["json"], move |app, path| {
                if let Some(path) = path {
                    app.keymap.message = std::fs::write(&path, text).err().map(|e| format!("export: {e}"));
                }
            });
        }
        Some((_, "import")) => {
            app.pick_open(|app, path| {
                let Some(path) = path else { return };
                let r = std::fs::read_to_string(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string()))
                    .and_then(|v| crate::toolbar_custom::import(app, &v));
                app.keymap.message = r.err().map(|e| format!("import: {e}"));
            });
        }
        Some((pair, "replace")) => {
            if let Some((id, key)) = pair.split_once('\n') {
                let _ = bind(app, id, key, true);
            }
            app.keymap.conflict = None;
            app.keymap.message = None;
        }
        Some((_, "cancel")) => {
            app.keymap.conflict = None;
            app.keymap.message = None;
        }
        _ => {}
    }
}

/// Stop waiting for a key (the dialog closed).
pub fn stop_capture(app: &mut SolveApp) {
    app.keymap.capturing = None;
    app.keymap.conflict = None;
    app.keymap.message = None;
}

/// Is the page waiting for a key (so the key must not also run a command)?
pub fn capturing(app: &SolveApp) -> bool {
    app.prefs_window.open && app.prefs_window.section == crate::prefs::PAGE_SHORTCUTS && app.keymap.capturing.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> SolveApp {
        SolveApp::new(solidcraft_engine::Session::default(), Default::default())
    }

    #[test]
    fn rebinding_detects_conflicts_and_resets() {
        let mut a = app();
        assert_eq!(effective(&a, "solid.extrude").as_deref(), Some("E"));
        // Shift+E is free.
        bind(&mut a, "solid.extrude", "Shift+E", false).unwrap();
        assert_eq!(commands_for(&a, "Shift+E"), vec!["solid.extrude"]);
        assert!(commands_for(&a, "E").is_empty());
        // M belongs to Move/Copy: refused, then taken over on request.
        let e = bind(&mut a, "solid.revolve", "M", false).unwrap_err();
        assert!(e.contains("already used by Move/Copy"), "{e}");
        bind(&mut a, "solid.revolve", "M", true).unwrap();
        assert_eq!(commands_for(&a, "M"), vec!["solid.revolve"]);
        assert_eq!(effective(&a, "solid.move"), None);
        assert!(bind(&mut a, "solid.revolve", "Ctrl+Z", false).is_err(), "reserved");
        // Kept between runs.
        let mut b = app();
        b.keymap.load(&a.keymap.prefs());
        assert_eq!(effective(&b, "solid.revolve").as_deref(), Some("M"));
        reset(&mut b, None);
        assert_eq!(effective(&b, "solid.move").as_deref(), Some("M"));
        assert_eq!(key_name(egui::Key::K, egui::Modifiers { ctrl: true, shift: true, ..Default::default() }), "Ctrl+Shift+K");
    }

    /// #3045: Shift+R starts the last tool again (here a sketch's Line after Escape put it
    /// down), R alone is still the rectangle; the menus show the key on Repeat, and no command
    /// can take it.
    #[test]
    fn shift_r_repeats_the_last_tool() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.frames(2);
        h.step(&json!({"start": "sketch.line"})).unwrap();
        h.step(&json!({"key": "Escape"})).unwrap();
        h.frames(1);
        assert!(h.app.tool.is_none());
        h.step(&json!({"key": "R", "shift": true})).unwrap();
        h.frames(1);
        assert_eq!(h.app.tool.as_ref().map(|t| t.cmd.to_string()).as_deref(), Some("sketch.line"), "Shift+R starts Line again");
        h.step(&json!({"key": "Escape"})).unwrap();
        h.step(&json!({"key": "R"})).unwrap();
        h.frames(1);
        let rectangle = h.app.tool.as_ref().map(|t| t.cmd.to_string()).unwrap_or_default();
        assert!(rectangle.starts_with("sketch.rectangle"), "R alone: {rectangle}");
        let repeat = crate::context_menu::radial_items(&h.app).into_iter().find(|i| i.id == "ui.repeat").unwrap();
        assert_eq!(repeat.shortcut, REPEAT_KEY);
        let mut a = app();
        assert!(bind(&mut a, "solid.extrude", REPEAT_KEY, true).is_err(), "Shift+R is the app's");
    }
}

#[cfg(test)]
#[path = "sketch_shortcut_tests.rs"]
mod sketch_shortcut_tests;

#[cfg(test)]
mod card_tests {
    use super::*;
    use crate::scenario::{self, Harness};

    #[test]
    fn shortcut_card_captures_clears_resets_and_esc_cancels_capture_only() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.keymap.filter = "solid.extrude".into();
        h.call("ui.prefs", json!({"open":"Keyboard Shortcuts"}));
        for (action, expected) in [("capture", "Shift+E"), ("clear", ""), ("reset", "E")] {
            h.step(&json!({"click":{"handle":format!("keymap:{action}:solid.extrude")}})).unwrap();
            if action == "capture" {
                h.step(&json!({"key":"E","shift":true})).unwrap();
            }
            assert_eq!(effective(&h.app, "solid.extrude").unwrap_or_default(), expected);
        }
        h.step(&json!({"click":{"handle":"keymap:capture:solid.extrude"}})).unwrap();
        h.step(&json!({"key":"Escape"})).unwrap();
        assert!(h.app.prefs_window.open && h.app.keymap.capturing.is_none());
        h.step(&json!({"key":"Escape"})).unwrap();
        assert!(!h.app.prefs_window.open);
    }

    #[test]
    fn help_shortcuts_card_keeps_rebinding_read_only() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.keymap.filter = "solid.extrude".into();
        h.call("ui.help", json!({"item":"shortcuts"}));
        assert!(h.app.keymap.read_only);
        assert!(scenario::handle_at("keymap:filter").is_some());
        for action in ["capture", "clear", "reset"] {
            assert!(scenario::handle_at(&format!("keymap:{action}:solid.extrude")).is_none());
        }
        assert_eq!(effective(&h.app, "solid.extrude").as_deref(), Some("E"));
    }
}
