//! Keyboard shortcuts: every command's default key comes from its `CommandSpec`; the user can
//! rebind or clear any of them (Preferences › Keyboard Shortcuts). A new key that another command
//! already uses is reported and only taken over on request; keys the app itself handles (Esc, S,
//! V, Delete, Ctrl+Z…) can't be bound. Changes are kept between runs.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use solidcraft_engine::{CommandSpec, command_specs};

use crate::SolveApp;
use crate::ui_kit;

/// Keys the application handles before any command shortcut.
pub const RESERVED: &[&str] = &[
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
    "Ctrl+Shift+S",
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
    recorder: Recorder,
    /// A key that another command uses: (command, key, the other commands).
    conflict: Option<(String, String, Vec<String>)>,
    message: Option<String>,
}

/// A chord is one ordinary key plus modifier state, as in the platform key-event model.
/// The one-second settling period is an editor affordance, not a shortcut activation delay.
const RECORD_SETTLE_SECONDS: f64 = 1.0;

#[derive(Default)]
struct Recorder {
    key: Option<egui::Key>,
    modifiers: egui::Modifiers,
    changed_at: f64,
}

enum Recording {
    Waiting,
    Cancelled,
    MultipleKeys,
    Complete(String),
}

impl Recorder {
    fn add_modifiers(&mut self, modifiers: egui::Modifiers, now: f64) {
        if self.key.is_none() {
            // Before the ordinary key, preview only what is currently held. A modifier
            // pressed and released while choosing a chord must not become part of it.
            self.modifiers = modifiers;
        } else {
            let combined = self.modifiers | modifiers;
            if combined != self.modifiers {
                self.modifiers = combined;
                self.changed_at = now;
            }
        }
    }

    fn preview(&self) -> Option<String> {
        match self.key {
            Some(key) => Some(format!("{}…", display(&key_name(key, self.modifiers)))),
            None if !self.modifiers.is_none() => Some(format!("{}…", display(&modifier_prefix(self.modifiers, None)))),
            None => None,
        }
    }

    fn hint(&self, input: &egui::InputState) -> &'static str {
        if self.key.is_none() {
            if self.modifiers.is_none() { "ui.shortcut_capture_press" } else { "ui.shortcut_capture_add_key" }
        } else if !input.keys_down.is_empty() || !input.modifiers.is_none() {
            "ui.shortcut_capture_release"
        } else {
            "ui.shortcut_capture_settling"
        }
    }

    fn update(&mut self, input: &egui::InputState) -> Recording {
        // Cancel on an actual focus loss. The control channel can legitimately open the
        // recorder while the window is already in the background.
        if input.events.iter().any(|event| matches!(event, egui::Event::WindowFocused(false))) {
            return Recording::Cancelled;
        }
        for event in &input.events {
            match event {
                egui::Event::ModifiersChanged(modifiers) => self.add_modifiers(*modifiers, input.time),
                // egui also emits physical left/right modifier key events. They carry
                // modifier state but must never occupy the chord's ordinary-key slot.
                egui::Event::Key {
                    key:
                        egui::Key::ShiftLeft
                        | egui::Key::ShiftRight
                        | egui::Key::ControlLeft
                        | egui::Key::ControlRight
                        | egui::Key::AltLeft
                        | egui::Key::AltRight
                        | egui::Key::SuperLeft
                        | egui::Key::SuperRight,
                    modifiers,
                    ..
                } => self.add_modifiers(*modifiers, input.time),
                egui::Event::Key { key, pressed, repeat, modifiers, .. } => {
                    if *pressed && *key == egui::Key::Escape {
                        return Recording::Cancelled;
                    }
                    if *pressed && !repeat {
                        if self.key.is_some_and(|previous| previous != *key) {
                            return Recording::MultipleKeys;
                        }
                        if self.key.is_none() {
                            self.key = Some(*key);
                            self.modifiers = *modifiers;
                            self.changed_at = input.time;
                        }
                    }
                    self.add_modifiers(*modifiers, input.time);
                }
                _ => {}
            }
        }
        self.add_modifiers(input.modifiers, input.time);
        if let Some(key) = self.key
            && input.keys_down.is_empty()
            && input.modifiers.is_none()
            && input.time - self.changed_at >= RECORD_SETTLE_SECONDS
        {
            return Recording::Complete(key_name(key, self.modifiers));
        }
        Recording::Waiting
    }
}

impl Keymap {
    pub fn prefs(&self) -> Value {
        json!(self.custom)
    }
    pub fn load(&mut self, v: &Value) {
        self.custom =
            v.as_object().map(|o| o.iter().take(2000).filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect()).unwrap_or_default();
        // A shortcut that named a modifier key twice ("Shift+ShiftLeft", from before the shift
        // keys were named after themselves) is the modifier itself (#4582).
        for key in self.custom.values_mut() {
            if let Some(named) = named_modifier_key(key) {
                *key = named;
            }
        }
    }
}

/// A key press as a shortcut name: `E`, `Shift+E`, `Ctrl+Alt+K`, `F5`, `Delete`.
///
/// A modifier key is the shortcut itself, and its two sides are the same shortcut (the app's
/// modifiers collapse them), so a bound Shift key is named `Shift`, not `Shift+ShiftLeft`
/// (#4582).
pub fn key_name(key: egui::Key, mods: egui::Modifiers) -> String {
    let modifier = Modifier::of(key);
    let mut s = modifier_prefix(mods, modifier);
    s.push_str(modifier.map_or_else(|| key.name(), |m| m.name()));
    s
}

/// The modifiers in a shortcut name, before the key. `modifier` is the modifier the pressed key
/// is, when it is one: a modifier key is the shortcut itself, so it is not named twice (#4582).
fn modifier_prefix(mods: egui::Modifiers, modifier: Option<Modifier>) -> String {
    let mut s = String::new();
    if (mods.command || mods.ctrl) && modifier != Some(Modifier::Ctrl) {
        s.push_str("Ctrl+");
    }
    // Keep the historical Ctrl/Cmd alias for existing bindings, but don't discard a
    // physical Control pressed together with macOS Command.
    if mods.mac_cmd && mods.ctrl && modifier != Some(Modifier::Ctrl) {
        s.push_str("Cmd+");
    }
    if mods.alt && modifier != Some(Modifier::Alt) {
        s.push_str("Alt+");
    }
    if mods.shift && modifier != Some(Modifier::Shift) {
        s.push_str("Shift+");
    }
    s
}

/// The modifier a key is, when it is one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Modifier {
    Ctrl,
    Alt,
    Shift,
}

impl Modifier {
    /// The modifier a key is: the left and right keys are the same one.
    fn of(key: egui::Key) -> Option<Self> {
        match key {
            egui::Key::ShiftLeft | egui::Key::ShiftRight => Some(Modifier::Shift),
            egui::Key::ControlLeft | egui::Key::ControlRight => Some(Modifier::Ctrl),
            egui::Key::AltLeft | egui::Key::AltRight => Some(Modifier::Alt),
            // The Super key is the app's Ctrl (the Command key on macOS).
            egui::Key::SuperLeft | egui::Key::SuperRight => Some(Modifier::Ctrl),
            _ => None,
        }
    }

    /// The modifier a key is called in a shortcut name.
    fn of_name(name: &str) -> Option<Self> {
        match name {
            "ShiftLeft" | "ShiftRight" => Some(Modifier::Shift),
            "ControlLeft" | "ControlRight" | "SuperLeft" | "SuperRight" => Some(Modifier::Ctrl),
            "AltLeft" | "AltRight" => Some(Modifier::Alt),
            _ => None,
        }
    }

    /// The modifier's name in a shortcut.
    fn name(self) -> &'static str {
        match self {
            Modifier::Ctrl => "Ctrl",
            Modifier::Alt => "Alt",
            Modifier::Shift => "Shift",
        }
    }
}

/// A shortcut name that ends in a modifier key, with that key named after its modifier:
/// `Shift+ShiftLeft` is `Shift` and `Ctrl+ShiftLeft` is `Ctrl+Shift` (#4582).
fn named_modifier_key(key: &str) -> Option<String> {
    let mut parts: Vec<&str> = key.split('+').collect();
    let last = parts.pop()?;
    let modifier = Modifier::of_name(last)?;
    let name = modifier.name();
    // The modifier named twice ("Shift+Shift") is the modifier alone.
    if parts.last() == Some(&name) {
        parts.pop();
    }
    parts.push(name);
    Some(parts.join("+"))
}

/// The key a command runs from now.
pub fn effective(app: &SolveApp, id: &str) -> Option<String> {
    match app.keymap.custom.get(id) {
        Some(k) if k.is_empty() => None,
        Some(k) => Some(k.clone()),
        None => solidcraft_engine::find_command(id).and_then(|c| c.shortcut).map(str::to_string),
    }
}

/// Use the spec already available when walking the command list.
fn effective_for_spec<'a>(app: &'a SolveApp, command: &CommandSpec) -> Option<&'a str> {
    app.keymap.custom.get(command.id).map(String::as_str).or(command.shortcut).filter(|key| !key.is_empty())
}

/// Presentation only: stored Ctrl bindings retain their historical primary-modifier alias.
/// A recorded macOS Control+Command chord includes both names explicitly.
pub fn display(key: &str) -> String {
    display_for_platform(key, cfg!(target_os = "macos"))
}

fn display_for_platform(key: &str, mac: bool) -> String {
    let both = key.starts_with("Ctrl+Cmd+");
    key.split('+')
        .map(|part| match (mac, part) {
            (true, "Ctrl") if !both => "Cmd",
            (true, "Alt") => "Option",
            _ => part,
        })
        .collect::<Vec<_>>()
        .join("+")
}

pub fn display_for(app: &SolveApp, id: &str) -> Option<String> {
    effective(app, id).map(|key| display(&key))
}

/// Commands bound to `key` now.
pub fn commands_for(app: &SolveApp, key: &str) -> Vec<&'static str> {
    command_specs().into_iter().filter(|c| effective(app, c.id).as_deref() == Some(key)).map(|c| c.id).collect()
}

/// Where a command's shortcut works: in a sketch (the SKETCH tab's tools), in the model (the
/// other tabs), or everywhere (commands on no tab: undo, views, files).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Scope {
    Render,
    Sketch,
    Model,
    Everywhere,
}

fn scope(id: &str) -> Scope {
    if id.starts_with("render.") || matches!(id, "appearance.paint" | "appearance.pick_up") {
        return Scope::Render;
    }
    match solidcraft_engine::find_command(id).map(|c| c.tab) {
        Some("SKETCH") => Scope::Sketch,
        Some(tab) if tab.starts_with("RENDER") => Scope::Render,
        Some("") | None => Scope::Everywhere,
        Some(_) => Scope::Model,
    }
}

/// Whether two commands can't share a key: a sketch tool and a model command can (#4163), each
/// running where it belongs; anything else can't.
fn clash(a: &str, b: &str) -> bool {
    let (x, y) = (scope(a), scope(b));
    x == y || x == Scope::Everywhere || y == Scope::Everywhere
}

/// The enabled command a key press runs, if any: one of the mode the view is in (a sketch tool
/// while a sketch is open, a model command otherwise) before any other.
pub fn command_for(app: &SolveApp, key: &str) -> Option<&'static str> {
    let here = if crate::render_workspace::active(app) {
        Scope::Render
    } else if app.session.active_sketch.is_some() {
        Scope::Sketch
    } else {
        Scope::Model
    };
    let enabled: Vec<&'static str> = commands_for(app, key)
        .into_iter()
        .filter(|id| {
            (scope(id) != Scope::Render || here == Scope::Render)
                && (here != Scope::Render || scope(id) != Scope::Sketch)
                && solidcraft_engine::find_command(id).is_some_and(|c| crate::render_workspace::interactive_command_info(app, c).enabled)
        })
        .collect();
    enabled.iter().find(|id| scope(id) == here).or(enabled.first()).copied()
}

/// Run a shortcut key: its enabled command; outside a sketch, a sketch tool's key opens Create
/// Sketch (on the selected plane or planar face, else on the plane picked next) and the tool
/// starts in the new sketch.
pub fn press(app: &mut SolveApp, key: &str, repeat: bool) {
    if let Some(id) = command_for(app, key) {
        // Pressed again while a tool of the same family is out, with Preferences › Sketch asking
        // for it (#2459): before its first point, the family's next mode (wrapping round); once a
        // point is placed, the tool keeps it. A held key's repeats change nothing.
        if let Some((family, current, started)) = app.tool.as_ref().filter(|_| app.preferences.shortcut_cycles_modes).and_then(|t| {
            let f = crate::sketch_families::family(t.cmd).filter(|f| f.commands.contains(&id))?;
            Some((f, t.cmd, !t.pts.is_empty() || !t.picks.is_empty()))
        }) {
            if !repeat && !started {
                let at = family.commands.iter().position(|c| *c == current).unwrap_or(0);
                if let Some(next) = family.commands.get((at + 1) % family.commands.len()) {
                    app.start(next);
                }
            }
            return;
        }
        app.start(variant_for_key(app, id));
    } else if let Some(id) = sketch_tool_for(app, key) {
        let then = variant_for_key(app, id);
        app.start("sketch.create");
        if let Some(d) = app.dialog.as_mut().filter(|d| matches!(d.kind, crate::dialogs::Kind::Sketch)) {
            d.then_start = Some(then);
        }
    }
}

/// What a family's key starts: the variant used last (#4308), unless that variant has a key of
/// its own; then the family's key starts the command it is bound to, and the other key the
/// variant.
fn variant_for_key(app: &SolveApp, id: &'static str) -> &'static str {
    let chosen = app.preferences.sketch_variants.resolve(id);
    if chosen != id && effective(app, chosen).is_some() { id } else { chosen }
}

/// Outside a sketch: the SKETCH-tab command bound to `key` that a new sketch would start. Not
/// Normal/Construction (a mode of the drawing in progress) nor the Finish Sketch panel.
fn sketch_tool_for(app: &SolveApp, key: &str) -> Option<&'static str> {
    if app.session.active_sketch.is_some() || crate::render_workspace::active(app) {
        return None;
    }
    commands_for(app, key).into_iter().find(|id| {
        solidcraft_engine::find_command(id).is_some_and(|c| c.tab == "SKETCH" && c.panel != "FINISH SKETCH" && c.id != "sketch.construction")
    })
}

/// Bind `key` to a command (`""` clears it). Refused for reserved keys, and when other commands
/// use the key unless `replace` (they lose it). A sketch tool and a model command may share a
/// key: each runs in its own mode.
pub fn bind(app: &mut SolveApp, id: &str, key: &str, replace: bool) -> Result<(), String> {
    let spec = solidcraft_engine::find_command(id).ok_or_else(|| format!("no command `{id}`"))?;
    if RESERVED.contains(&key) {
        return Err(format!("{key} is used by SolidCraft itself"));
    }
    if !key.is_empty() {
        let others: Vec<&'static str> = commands_for(app, key).into_iter().filter(|o| *o != id && clash(o, id)).collect();
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
    let filtering = crate::prefs_search::querying(ui);
    // Keep a pending chord until it has settled and all keys are released. Modifier-only
    // input never starts the timer, so modifiers can be assembled at the user's own pace.
    // A Preferences search is using the keyboard; don't swallow those keys to finish a capture.
    if !filtering && let Some(id) = app.keymap.capturing.clone() {
        match ctx.input(|i| app.keymap.recorder.update(i)) {
            Recording::Waiting => {
                if app.keymap.recorder.key.is_some() {
                    let remaining = ctx.input(|i| RECORD_SETTLE_SECONDS - (i.time - app.keymap.recorder.changed_at));
                    if remaining > 0.0
                        && let Ok(delay) = std::time::Duration::try_from_secs_f64(remaining)
                    {
                        ctx.request_repaint_after(delay);
                    }
                }
            }
            Recording::Cancelled => {
                app.keymap.capturing = None;
                // Escape cancels recording first, without also closing Preferences.
                ctx.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Key { key: egui::Key::Escape, .. })));
            }
            Recording::MultipleKeys => {
                app.keymap.capturing = None;
                app.keymap.message = Some(app.tr("ui.shortcut_multiple_keys").to_string());
            }
            Recording::Complete(name) => {
                app.keymap.capturing = None;
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
        // The recorder owns these events. In particular, Enter/Space must not activate
        // the focused binding button again or type into the filter while still pending.
        ctx.input_mut(|i| {
            i.events.retain(|event| !matches!(event, egui::Event::Key { .. } | egui::Event::Text(_) | egui::Event::Paste(_)));
        });
    }
    let mut action: Option<(String, &str)> = None;
    ui_kit::scope(ui, |ui| {
        if !filtering {
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
            crate::scenario::publish_handle("shortcut:filter", filter.rect.center());
            if filter.clicked() {
                action = Some((String::new(), "cancel"));
            }
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
                    let replace = ui_kit::button(ui, crate::i18n::format("keymap.use_anyway", &[("key", &display(&key))]), ui_kit::Button::Normal);
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
        }
        if !filtering && !app.keymap.read_only {
            ui_kit::text(ui, app.tr("ui.shortcut_capture_hint"), ui_kit::Text::Note("shortcut_capture_help"));
            if app.keymap.capturing.is_some() {
                let hint = ctx.input(|input| app.keymap.recorder.hint(input));
                ui_kit::text(ui, app.tr(hint), ui_kit::Text::Hint("shortcut_capture_state"));
            }
        }
        let q = if filtering { app.prefs_window.filter.trim().to_lowercase() } else { app.keymap.filter.to_lowercase() };
        let commands: Vec<_> = command_specs()
            .into_iter()
            .filter_map(|c| {
                let key = effective_for_spec(app, c).unwrap_or_default();
                let label = app.tr_label(c.label);
                let hit = if filtering {
                    crate::prefs_search::matches_text(&q, &[c.label, c.id, key, label])
                } else {
                    q.is_empty() || [c.label, c.id, key, label].iter().any(|s| s.to_lowercase().contains(&q))
                };
                hit.then_some((c, key, label))
            })
            .collect();
        if filtering && commands.is_empty() {
            return;
        }
        let count = commands.len();
        let shortcut_title = app.tr("prefs.shortcuts").to_string();
        let paint = |ui: &mut egui::Ui| {
            let t = crate::theme::Tokens::get();
            let key_width = commands
                .iter()
                .map(|(_, key, _)| ui_kit::ContentWidth::button(ui, &display(key)))
                .chain(app.keymap.recorder.preview().filter(|_| app.keymap.capturing.is_some()).map(|text| ui_kit::ContentWidth::button(ui, &text)))
                .fold(ui_kit::ContentWidth::button(ui, app.tr_label("press a key…")), f32::max)
                .max(ui_kit::ContentWidth::label(ui, app.tr_label("Keyboard Shortcuts")) + t.space_8);
            let action_width = if app.keymap.read_only {
                t.control_height
            } else {
                ui_kit::ContentWidth::button(ui, app.tr_label("Clear")) + ui_kit::ContentWidth::button(ui, app.tr_label("Reset")) + t.space_4
            };
            let columns = [
                ui_kit::Column::new("Command", 1.5).flex().min_content_width(t.form_label_width),
                ui_kit::Column::new(crate::i18n::tr("keymap.command_id"), 1.8).flex().min_content_width(t.form_label_width),
                ui_kit::Column::new("Keyboard Shortcuts", 1.0).flex().min_content_width(key_width),
                ui_kit::Column::new("", 0.0).icon_only().min_content_width(action_width),
            ];
            ui_kit::table(ui, "keymap", &columns, crate::i18n::tr("keymap.empty"), |ui, table| {
                for (c, key, translated_label) in commands {
                    table.row_visible(ui, c.id, app.keymap.capturing.as_deref() == Some(c.id), false, |row| {
                        row.cell(|ui| {
                            ui_kit::text(ui, translated_label, ui_kit::Text::Cell);
                        });
                        row.cell(|ui| {
                            ui_kit::text(ui, c.id, ui_kit::Text::Cell);
                        });
                        let capturing = app.keymap.capturing.as_deref() == Some(c.id);
                        let changed = app.keymap.custom.contains_key(c.id);
                        let text = if capturing {
                            app.keymap.recorder.preview().unwrap_or_else(|| app.tr_label("press a key…").to_string())
                        } else if key.is_empty() {
                            "—".to_string()
                        } else {
                            display(key)
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
                                if ui.is_rect_visible(capture.rect) {
                                    crate::scenario::publish_handle(&format!("shortcut:{}", c.id), capture.rect.center());
                                }
                                if capture.clicked() {
                                    capture.surrender_focus();
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
        };
        if filtering {
            ui_kit::search_fold(ui, &shortcut_title, count, paint);
        } else {
            ui_kit::settings_scroll(ui, "sc_keymap_scroll", paint);
        }
    });
    crate::scenario::publish_count("shortcut_recording", f64::from(app.keymap.capturing.is_some()));
    if action.is_some() {
        app.keymap.capturing = None;
        app.keymap.recorder = Recorder::default();
    }
    match action {
        Some((id, "capture")) => {
            app.keymap.capturing = Some(id);
            app.keymap.recorder = Recorder::default();
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
                    // Through `vfs`, like every other save: the web app hands it out as a download.
                    app.keymap.message = solidcraft_engine::io::vfs::write(&path, text.as_bytes()).err().map(|e| format!("export: {e}"));
                }
            });
        }
        Some((_, "import")) => {
            app.pick_open(|app, path| {
                let Some(path) = path else { return };
                // Through `vfs`, like every other import: in the web app a picked file is there.
                let r = solidcraft_engine::io::vfs::read_to_string(&path, crate::toolbar_custom::MAX_IMPORT_BYTES)
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
    app.keymap.recorder = Recorder::default();
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

    fn recorder_frame(a: &mut SolveApp, ctx: &egui::Context, time: f64, events: Vec<egui::Event>) {
        let raw = egui::RawInput { time: Some(time), events, ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| page(a, ui));
        out.textures_delta.clear();
    }

    #[test]
    fn recorder_waits_for_late_modifiers_instead_of_binding_the_first_key_down() {
        let mut a = app();
        let ctx = egui::Context::default();
        a.keymap.capturing = Some("solid.chamfer".into());
        recorder_frame(
            &mut a,
            &ctx,
            0.0,
            vec![egui::Event::Key { key: egui::Key::M, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }],
        );
        assert!(a.keymap.capturing.is_some(), "the first key-down must remain a preview");
        assert_eq!(effective(&a, "solid.chamfer"), None);
    }

    fn event(key: egui::Key, pressed: bool, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }
    }

    #[test]
    fn control_input_preserves_primary_alias_and_explicit_physical_modifiers() {
        for (step, ctrl, mac_cmd) in [
            (json!({"key":"F12","pressed":true,"cmd":true}), true, false),
            (json!({"key":"F12","pressed":true,"cmd":true,"ctrl":false}), false, true),
            (json!({"key":"F12","pressed":true,"cmd":true,"ctrl":true}), true, true),
            (json!({"key":"F12","pressed":true,"ctrl":true}), true, false),
        ] {
            let mut h = crate::scenario::Harness::new();
            h.step(&step).unwrap();
            let modifiers = h.ctx.input(|i| i.modifiers);
            assert_eq!(modifiers.ctrl, ctrl, "{step}");
            assert_eq!(modifiers.mac_cmd, mac_cmd, "{step}");
            assert!(modifiers.command, "{step}");
        }
    }

    #[test]
    fn recorder_previews_each_modifier_and_explains_the_next_step() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"open":"Keyboard Shortcuts"}));
        h.step(&json!({"click":{"handle":"shortcut:filter"}})).unwrap();
        h.call("ui.text", json!({"text":"solid.chamfer"}));
        h.step(&json!({"click":{"handle":"shortcut:solid.chamfer"}})).unwrap();
        h.call("ui.key", json!({"key":"ShiftLeft","pressed":true,"shift":true}));
        assert!(h.widget("Shift+…").is_some(), "show Shift immediately, before any ordinary key");
        assert!(h.widget("Add one ordinary key. Escape cancels.").is_some());
        h.call("ui.key", json!({"key":"SuperLeft","pressed":true,"shift":true,"cmd":true,"ctrl":false}));
        let preview = if cfg!(target_os = "macos") { "Cmd+Shift+…" } else { "Ctrl+Shift+…" };
        assert!(h.widget(preview).is_some(), "show each newly held modifier");
        h.call("ui.key", json!({"key":"SuperLeft","pressed":false,"shift":true}));
        assert!(h.widget("Shift+…").is_some(), "remove released modifiers before an ordinary key");
        h.call("ui.key", json!({"key":"ShiftLeft","pressed":false}));
        assert!(h.widget("press a key…").is_some());
        assert_eq!(effective(&h.app, "solid.chamfer"), None);
        h.call("ui.key", json!({"key":"ShiftRight","pressed":true,"shift":true}));
        h.call("ui.key", json!({"key":"C","pressed":true,"shift":true}));
        assert!(h.widget("Shift+C…").is_some());
        assert!(h.widget("Release all keys to confirm. Escape cancels.").is_some());
        h.call("ui.key", json!({"key":"C","pressed":false,"shift":true}));
        h.call("ui.key", json!({"key":"ShiftRight","pressed":false}));
        assert!(h.widget("Confirming shortcut… Escape cancels.").is_some());
        assert!(h.widget("Shift+C…").is_some(), "retain the proposed chord until confirmed");
        assert_eq!(effective(&h.app, "solid.chamfer"), None);
        h.frames(65);
        assert_eq!(effective(&h.app, "solid.chamfer").as_deref(), Some("Shift+C"));
        assert!(h.widget("Shift+C").is_some());
    }

    #[test]
    fn physical_modifier_keys_are_not_the_ordinary_key_in_a_chord() {
        use egui::Key;
        for (key, mods, expected) in [
            (Key::ShiftLeft, egui::Modifiers::SHIFT, "Shift+C"),
            (Key::ShiftRight, egui::Modifiers::SHIFT, "Shift+C"),
            (Key::ControlLeft, egui::Modifiers::CTRL, "Ctrl+C"),
            (Key::ControlRight, egui::Modifiers::CTRL, "Ctrl+C"),
            (Key::AltLeft, egui::Modifiers::ALT, "Alt+C"),
            (Key::AltRight, egui::Modifiers::ALT, "Alt+C"),
            (Key::SuperLeft, egui::Modifiers::MAC_CMD | egui::Modifiers::COMMAND, "Ctrl+C"),
            (Key::SuperRight, egui::Modifiers::MAC_CMD | egui::Modifiers::COMMAND, "Ctrl+C"),
        ] {
            let ctx = egui::Context::default();
            let mut recorder = Recorder::default();
            for (time, events, complete) in [
                (0.0, vec![egui::Event::ModifiersChanged(mods), event(key, true, mods)], false),
                (5.0, vec![], false),
                (5.1, vec![event(Key::C, true, mods)], false),
                (5.2, vec![event(Key::C, false, mods)], false),
                (5.3, vec![event(key, false, egui::Modifiers::NONE), egui::Event::ModifiersChanged(egui::Modifiers::NONE)], false),
                (6.2, vec![], true),
            ] {
                let raw = egui::RawInput { time: Some(time), events, ..Default::default() };
                let mut out = ctx.run_ui(raw, |ui| {
                    let result = ui.input(|input| recorder.update(input));
                    if complete {
                        assert!(matches!(result, Recording::Complete(ref binding) if binding == expected), "{key:?}");
                    } else {
                        assert!(matches!(result, Recording::Waiting), "{key:?} at {time}");
                    }
                });
                out.textures_delta.clear();
                if time <= 5.0 {
                    assert!(recorder.key.is_none(), "{key:?} must not be recorded as an ordinary key");
                }
            }
        }
    }

    #[test]
    fn recorder_accumulates_late_modifiers_and_settles_for_one_second() {
        let mut a = app();
        let ctx = egui::Context::default();
        a.keymap.capturing = Some("sketch.mirror".into());
        recorder_frame(&mut a, &ctx, 0.0, vec![event(egui::Key::M, true, egui::Modifiers::NONE)]);
        recorder_frame(&mut a, &ctx, 0.4, vec![egui::Event::ModifiersChanged(egui::Modifiers::SHIFT)]);
        let mods = egui::Modifiers { shift: true, mac_cmd: true, command: true, ..Default::default() };
        recorder_frame(&mut a, &ctx, 0.8, vec![egui::Event::ModifiersChanged(mods)]);
        recorder_frame(&mut a, &ctx, 1.0, vec![event(egui::Key::M, false, mods), egui::Event::ModifiersChanged(egui::Modifiers::NONE)]);
        recorder_frame(&mut a, &ctx, 1.79, vec![]);
        assert!(a.keymap.capturing.is_some());
        assert_eq!(effective(&a, "sketch.mirror"), None);
        recorder_frame(&mut a, &ctx, 1.81, vec![]);
        assert!(a.keymap.capturing.is_none());
        assert_eq!(effective(&a, "sketch.mirror").as_deref(), Some("Ctrl+Shift+M"));
        let mut reloaded = app();
        reloaded.keymap.load(&a.keymap.prefs());
        assert_eq!(effective(&reloaded, "sketch.mirror"), effective(&a, "sketch.mirror"));
    }

    #[test]
    fn recorder_waits_indefinitely_for_modifier_only_input_and_for_release() {
        let mut a = app();
        let ctx = egui::Context::default();
        a.keymap.capturing = Some("sketch.mirror".into());
        let mods = egui::Modifiers { shift: true, ctrl: true, alt: true, mac_cmd: true, command: true };
        recorder_frame(&mut a, &ctx, 0.0, vec![egui::Event::ModifiersChanged(mods)]);
        recorder_frame(&mut a, &ctx, 10.0, vec![]);
        assert!(a.keymap.capturing.is_some());
        assert_eq!(effective(&a, "sketch.mirror"), None);
        recorder_frame(&mut a, &ctx, 12.0, vec![event(egui::Key::M, true, mods)]);
        let mut repeat = event(egui::Key::M, true, mods);
        if let egui::Event::Key { repeat, .. } = &mut repeat {
            *repeat = true;
        }
        recorder_frame(&mut a, &ctx, 14.0, vec![repeat]);
        assert!(a.keymap.capturing.is_some(), "held keys must not confirm");
        recorder_frame(&mut a, &ctx, 14.1, vec![event(egui::Key::M, false, mods)]);
        assert!(a.keymap.capturing.is_some(), "held modifiers must not confirm");
        recorder_frame(&mut a, &ctx, 14.2, vec![egui::Event::ModifiersChanged(egui::Modifiers::NONE)]);
        assert_eq!(effective(&a, "sketch.mirror").as_deref(), Some("Ctrl+Cmd+Alt+Shift+M"));
    }

    #[test]
    fn recorder_rejects_two_ordinary_keys_and_escape_preserves_the_old_binding() {
        for second in [egui::Key::N, egui::Key::Escape] {
            let mut a = app();
            let ctx = egui::Context::default();
            a.keymap.capturing = Some("solid.extrude".into());
            recorder_frame(&mut a, &ctx, 0.0, vec![event(egui::Key::M, true, egui::Modifiers::SHIFT)]);
            recorder_frame(&mut a, &ctx, 0.5, vec![event(second, true, egui::Modifiers::SHIFT)]);
            assert!(a.keymap.capturing.is_none());
            assert_eq!(effective(&a, "solid.extrude").as_deref(), Some("E"));
            assert_eq!(a.keymap.message.is_some(), second != egui::Key::Escape);
        }
    }

    #[test]
    fn recorder_conflicts_only_after_settling_and_plain_keys_still_work() {
        let mut a = app();
        let ctx = egui::Context::default();
        a.keymap.capturing = Some("solid.chamfer".into());
        recorder_frame(&mut a, &ctx, 0.0, vec![event(egui::Key::M, true, egui::Modifiers::NONE), event(egui::Key::M, false, egui::Modifiers::NONE)]);
        assert!(a.keymap.conflict.is_none());
        recorder_frame(&mut a, &ctx, 1.1, vec![]);
        assert_eq!(a.keymap.conflict.as_ref().map(|(_, key, _)| key.as_str()), Some("M"));
        assert_eq!(effective(&a, "solid.move").as_deref(), Some("M"));
        assert_eq!(effective(&a, "solid.chamfer"), None);
        stop_capture(&mut a);
        a.keymap.capturing = Some("solid.extrude".into());
        recorder_frame(&mut a, &ctx, 2.0, vec![event(egui::Key::K, true, egui::Modifiers::NONE), event(egui::Key::K, false, egui::Modifiers::NONE)]);
        recorder_frame(&mut a, &ctx, 3.1, vec![]);
        assert_eq!(effective(&a, "solid.extrude").as_deref(), Some("K"));
    }

    #[test]
    fn recorder_cancels_on_focus_loss_and_dialog_close() {
        let mut a = app();
        let ctx = egui::Context::default();
        a.keymap.capturing = Some("solid.extrude".into());
        recorder_frame(&mut a, &ctx, 0.0, vec![event(egui::Key::K, true, egui::Modifiers::SHIFT)]);
        let raw = egui::RawInput { time: Some(0.5), focused: false, events: vec![egui::Event::WindowFocused(false)], ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| page(&mut a, ui));
        out.textures_delta.clear();
        assert!(a.keymap.capturing.is_none());
        assert_eq!(effective(&a, "solid.extrude").as_deref(), Some("E"));
        stop_capture(&mut a);
        assert!(a.keymap.recorder.key.is_none());
    }

    #[test]
    fn recording_a_shortcut_does_not_type_into_preferences_search() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_SHORTCUTS);
        h.app.keymap.capturing = Some("solid.extrude".into());
        h.frames(1);
        h.step(&json!({"text": "z"})).expect("key while recording");
        assert!(h.app.prefs_window.filter.is_empty(), "a shortcut being recorded keeps the key");
    }

    #[test]
    fn escape_cancels_only_recording_and_no_command_fires_during_capture() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"open": "Keyboard Shortcuts"}));
        h.app.keymap.capturing = Some("solid.chamfer".into());
        h.call("ui.key", json!({"key": "C", "pressed": true}));
        h.frames(2);
        assert!(h.app.tool.is_none() && h.app.dialog.is_none());
        assert!(h.app.session.active_sketch.is_none(), "C must not start a sketch while recording");
        h.call("ui.key", json!({"key": "Escape"}));
        h.frames(3);
        assert!(h.app.prefs_window.open, "Escape should cancel the recorder, not close Preferences");
        assert!(h.app.keymap.capturing.is_none());
        assert_eq!(effective(&h.app, "solid.chamfer"), None);
    }

    #[test]
    fn a_background_control_channel_recording_waits_for_an_actual_focus_loss() {
        let mut a = app();
        let ctx = egui::Context::default();
        a.keymap.capturing = Some("sketch.mirror".into());
        let raw =
            egui::RawInput { time: Some(0.0), focused: false, events: vec![event(egui::Key::M, true, egui::Modifiers::SHIFT)], ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| page(&mut a, ui));
        out.textures_delta.clear();
        assert!(a.keymap.capturing.is_some());
        assert_eq!(effective(&a, "sketch.mirror"), None);
    }

    #[test]
    fn enter_and_space_are_recorded_without_reactivating_the_focused_button() {
        for key in ["Enter", "Space"] {
            let mut h = crate::scenario::Harness::new();
            h.call("ui.prefs", json!({"open": "Keyboard Shortcuts"}));
            h.step(&json!({"click":{"handle":"shortcut:filter"}})).unwrap();
            h.call("ui.text", json!({"text":"sketch.mirror"}));
            h.step(&json!({"click":{"handle":"shortcut:sketch.mirror"}})).unwrap();
            h.call("ui.key", json!({"key":key}));
            assert!(h.app.keymap.capturing.is_some(), "{key} should still be pending");
            h.frames(65);
            assert_eq!(effective(&h.app, "sketch.mirror").as_deref(), Some(key));
            assert!(h.app.prefs_window.open);
        }
    }

    #[test]
    fn chord_display_and_legacy_primary_modifier_bindings_are_compatible() {
        assert_eq!(display_for_platform("Ctrl+Alt+Shift+M", true), "Cmd+Option+Shift+M");
        assert_eq!(display_for_platform("Ctrl+Cmd+Alt+Shift+M", true), "Ctrl+Cmd+Option+Shift+M");
        assert_eq!(display_for_platform("Ctrl+Alt+Shift+M", false), "Ctrl+Alt+Shift+M");
        let mut a = app();
        a.keymap.load(&json!({"solid.chamfer":"Shift+C", "solid.extrude":"Ctrl+Alt+E"}));
        assert_eq!(effective(&a, "solid.chamfer").as_deref(), Some("Shift+C"));
        let mods = egui::Modifiers { mac_cmd: true, command: true, alt: true, ..Default::default() };
        assert_eq!(commands_for(&a, &key_name(egui::Key::E, mods)), vec!["solid.extrude"]);
        let extra = egui::Modifiers { shift: true, ..mods };
        assert!(commands_for(&a, &key_name(egui::Key::E, extra)).is_empty());
        let both = egui::Modifiers { ctrl: true, ..extra };
        bind(&mut a, "sketch.mirror", &key_name(egui::Key::M, both), false).unwrap();
        assert_eq!(commands_for(&a, &key_name(egui::Key::M, both)), vec!["sketch.mirror"]);
        assert!(commands_for(&a, &key_name(egui::Key::M, extra)).is_empty());
    }

    #[test]
    fn shortcut_page_only_builds_visible_rows_and_reveals_filtered_rows() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width": 1200, "height": 600}));
        h.call("ui.prefs", json!({"open": "Keyboard Shortcuts"}));
        h.frames(3);
        let commands = command_specs();
        let first = commands.first().unwrap().id;
        let last = commands.last().unwrap().id;
        let handle = |id| format!("keymap:capture:{id}");
        assert!(crate::scenario::handle_at(&handle(first)).is_some());
        assert!(crate::scenario::handle_at(&handle(last)).is_none(), "off-screen cells should not be built");

        h.app.keymap.filter = last.to_string();
        h.frames(3);
        assert!(crate::scenario::handle_at(&handle(last)).is_some(), "filtering brings the last command into view");
        assert!(crate::scenario::handle_at(&handle(first)).is_none());
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

    /// #4163: one key for a model command and a sketch tool. Binding it to both is allowed (a
    /// second model command still clashes); outside a sketch it runs the model command, in a
    /// sketch the tool.
    #[test]
    fn a_sketch_tool_and_a_model_command_share_a_key() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        bind(&mut h.app, "sketch.create", "Shift+S", false).unwrap();
        bind(&mut h.app, "sketch.point", "Shift+S", false).unwrap();
        assert_eq!(effective(&h.app, "sketch.create").as_deref(), Some("Shift+S"), "the model command keeps it");
        let e = bind(&mut h.app, "solid.revolve", "Shift+S", false).unwrap_err();
        assert!(e.contains("already used by"), "{e}");
        assert!(bind(&mut h.app, "sketch.line", "Shift+S", false).is_err(), "two sketch tools still clash");
        assert_eq!(command_for(&h.app, "Shift+S"), Some("sketch.create"));
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.frames(2);
        assert_eq!(command_for(&h.app, "Shift+S"), Some("sketch.point"), "in a sketch the tool runs");
        h.step(&json!({"key": "S", "shift": true})).unwrap();
        h.frames(1);
        assert_eq!(h.app.tool.as_ref().map(|t| t.cmd.to_string()).as_deref(), Some("sketch.point"));
        assert!(h.app.dialog.is_none(), "no second Create Sketch");
    }

    /// A command on no tab (undo, views) can't share its key with anything.
    #[test]
    fn commands_used_everywhere_share_no_key() {
        let mut a = app();
        let everywhere = command_specs().into_iter().find(|c| c.tab.is_empty()).unwrap();
        bind(&mut a, everywhere.id, "Shift+Q", false).unwrap();
        assert!(bind(&mut a, "sketch.point", "Shift+Q", false).is_err(), "{}", everywhere.id);
        assert!(bind(&mut a, "solid.revolve", "Shift+Q", false).is_err(), "{}", everywhere.id);
        // And the other way round.
        bind(&mut a, "sketch.point", "Shift+W", false).unwrap();
        assert!(bind(&mut a, everywhere.id, "Shift+W", false).is_err(), "{}", everywhere.id);
    }

    /// #4582: the Keyboard Shortcuts page named a bound Shift key "Shift+ShiftLeft": the modifier
    /// twice, and the left and right keys as two different shortcuts. A modifier key is the
    /// shortcut itself, and both sides are the same one.
    #[test]
    fn a_bound_modifier_key_is_named_after_itself() {
        let shift = egui::Modifiers { shift: true, ..Default::default() };
        assert_eq!(key_name(egui::Key::ShiftLeft, shift), "Shift");
        assert_eq!(key_name(egui::Key::ShiftRight, shift), "Shift", "both shift keys are the same shortcut");
        assert_eq!(key_name(egui::Key::ControlLeft, egui::Modifiers { ctrl: true, ..Default::default() }), "Ctrl");
        assert_eq!(key_name(egui::Key::AltLeft, egui::Modifiers { alt: true, ..Default::default() }), "Alt");
        // The other keys are named as before.
        assert_eq!(key_name(egui::Key::K, egui::Modifiers { ctrl: true, shift: true, ..Default::default() }), "Ctrl+Shift+K");
        assert_eq!(key_name(egui::Key::E, shift), "Shift+E");
        // A shortcut bound before the change keeps working.
        let mut a = app();
        a.keymap.load(&json!({"solid.extrude": "Shift+ShiftLeft"}));
        assert_eq!(effective(&a, "solid.extrude").as_deref(), Some("Shift"));
        // The bound key runs its command.
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.frames(2);
        bind(&mut h.app, "sketch.line", "Shift", true).unwrap();
        press(&mut h.app, "Shift", false);
        assert_eq!(h.app.tool.as_ref().map(|t| t.cmd.to_string()).as_deref(), Some("sketch.line"), "the bound Shift key starts its command");
    }

    /// #3045: Shift+R starts the last tool again (here a sketch's Line after Escape put it
    /// down), R alone is still the rectangle; the menus show the key on Repeat.
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
        assert_eq!(repeat.shortcut, display("Shift+R"));
    }

    /// #7077: Repeat Last Command is the command `edit.repeat`: Keyboard Shortcuts lists it and
    /// can give it another key (the old one then does nothing, and the menus show the new one),
    /// another command takes its key only on request, and command search finds and starts it.
    /// Typing Shift+R into a field is text, not Repeat.
    #[test]
    fn repeat_last_command_is_a_command_with_a_key_that_can_change() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.frames(2);
        assert_eq!(effective(&h.app, "edit.repeat").as_deref(), Some("Shift+R"));
        assert_eq!(commands_for(&h.app, "Shift+R"), ["edit.repeat"], "no other default uses Shift+R");
        assert!(bind(&mut h.app, "solid.extrude", "Shift+R", false).unwrap_err().contains("Repeat Last Command"));
        // Nothing was started yet: Repeat says so and starts nothing.
        h.step(&json!({"key": "R", "shift": true})).unwrap();
        assert!(h.app.dialog.is_none() && h.app.tool.is_none());
        assert_eq!(h.app.status.as_ref().map(|s| s.0.as_str()), Some(h.app.tr("ui.repeat_nothing")));
        h.step(&json!({"start": "solid.box"})).unwrap();
        h.frames(2);
        assert!(h.app.dialog.is_some(), "Box opens its dialog");
        crate::dialogs::cancel(&mut h.app);
        h.frames(1);
        assert!(h.app.dialog.is_none());
        // Another key for Repeat: Shift+R is free again, Alt+Shift+R starts Box's dialog again.
        bind(&mut h.app, "edit.repeat", "Alt+Shift+R", false).unwrap();
        h.step(&json!({"key": "R", "shift": true})).unwrap();
        h.frames(1);
        assert!(h.app.dialog.is_none(), "the old key no longer repeats");
        h.step(&json!({"key": "R", "shift": true, "alt": true})).unwrap();
        h.frames(2);
        assert_eq!(h.app.dialog.as_ref().and_then(|d| d.command()), Some("solid.box"), "the new key repeats");
        let repeat = crate::context_menu::radial_items(&h.app).into_iter().find(|i| i.id == "ui.repeat").unwrap();
        assert_eq!(repeat.shortcut, display("Alt+Shift+R"));
        crate::dialogs::cancel(&mut h.app);
        reset(&mut h.app, Some("edit.repeat"));
        // Command search lists it, and choosing it starts the last command again.
        assert_eq!(crate::shortcut_box::search(&h.app, "repeat last").first().map(|r| r.0), Some("edit.repeat"));
        h.call("ui.palette", json!({"text": "repeat last"}));
        h.frames(3);
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        assert!(!h.app.ui.palette_open);
        assert_eq!(h.app.dialog.as_ref().and_then(|d| d.command()), Some("solid.box"), "chosen in command search");
        assert_eq!(h.app.last_command.as_ref().map(|c| c.0.as_str()), Some("solid.box"), "Repeat is never what is repeated");
        crate::dialogs::cancel(&mut h.app);
        h.frames(1);
        // While command search's field takes the keyboard, Shift+R is a letter typed there.
        h.call("ui.palette", json!({}));
        h.frames(3);
        h.step(&json!({"key": "R", "shift": true})).unwrap();
        h.frames(1);
        assert!(h.app.ui.palette_open && h.app.dialog.is_none(), "typing in a field never repeats");
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
    fn live_chord_column_fits_all_modifiers() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.keymap.filter = "sketch.mirror".into();
        h.call("ui.prefs", json!({"open":"Keyboard Shortcuts"}));
        h.step(&json!({"click":{"handle":"keymap:capture:sketch.mirror"}})).unwrap();
        h.step(&json!({"key":"M","pressed":true,"shift":true,"cmd":true,"ctrl":true,"alt":true})).unwrap();
        let preview = h.app.keymap.recorder.preview().unwrap();
        let cell = scenario::rect_of("dialog:table:keymap:sketch.mirror:cell:Keyboard Shortcuts").unwrap();
        let ctx = crate::fonts::test_context();
        let mut needed = 0.0;
        let mut out = ctx.run_ui(Default::default(), |ui| {
            ui_kit::scope(ui, |ui| needed = ui_kit::ContentWidth::button(ui, &preview));
        });
        out.textures_delta.clear();
        assert!(cell.width() >= needed, "preview {preview}: cell {} < {needed}", cell.width());
    }

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
                assert_eq!(effective(&h.app, "solid.extrude").as_deref(), Some("E"), "retain the binding during the grace period");
                h.step(&json!({"key":"ShiftLeft","pressed":false})).unwrap();
                h.frames(65);
            }
            assert_eq!(effective(&h.app, "solid.extrude").unwrap_or_default(), expected);
        }
        h.step(&json!({"click":{"handle":"keymap:capture:solid.extrude"}})).unwrap();
        h.step(&json!({"key":"Escape"})).unwrap();
        assert!(h.app.prefs_window.open && h.app.keymap.capturing.is_none());
        h.step(&json!({"key":"Escape"})).unwrap();
        assert!(!h.app.prefs_window.open);
    }

    /// Export… writes the shortcuts file through `vfs`, like the other saves, so the web app hands
    /// it out as a download (#7192). On the disk it is the same file; a folder scope active while
    /// it is written refuses a path outside it, which shows the write goes through `vfs`.
    #[test]
    fn shortcut_export_writes_through_vfs() {
        use solidcraft_engine::io::vfs;
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.prefs", json!({"open":"Keyboard Shortcuts"}));
        h.frames(2);
        let (root, out) = (h.dir.join("root"), h.dir.join("out"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        for (path, inside) in [(root.join("keys.json"), true), (out.join("keys.json"), false)] {
            let answer = path.to_string_lossy().to_string();
            h.app.services.pick_save = Some(Box::new(move |_, _| Box::pin(std::future::ready(Some(answer.clone())))));
            vfs::scoped(Some(vfs::PathScope::new([root.clone()])), || {
                h.step(&json!({"widget":"Export…"})).unwrap();
                h.frames(4);
            });
            let message = h.app.keymap.message.clone();
            if inside {
                assert_eq!(message, None);
                let file: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
                assert_eq!(file, crate::toolbar_custom::export(&h.app));
            } else {
                assert!(message.as_deref().is_some_and(|m| m.contains("outside the folders this agent may use")), "{message:?}");
                assert!(!path.exists(), "nothing written outside");
            }
        }
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

    /// Import… reads the shortcuts file through `vfs`, like the other imports: in the web app a
    /// picked file is kept there, where `std::fs` can't read it (#7643). On the disk it is the same
    /// file; a folder scope active while it is read refuses a path outside it, which shows the
    /// read goes through `vfs`.
    #[test]
    fn shortcut_import_reads_through_vfs() {
        use solidcraft_engine::io::vfs;
        let mut h = Harness::new();
        h.app.home.open = false;
        bind(&mut h.app, "solid.extrude", "Shift+E", true).unwrap();
        let text = crate::toolbar_custom::export(&h.app).to_string();
        reset(&mut h.app, None);
        h.call("ui.prefs", json!({"open":"Keyboard Shortcuts"}));
        h.frames(2);
        let (root, out) = (h.dir.join("root"), h.dir.join("out"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        for (path, inside) in [(out.join("keys.json"), false), (root.join("keys.json"), true)] {
            std::fs::write(&path, &text).unwrap();
            let answer = path.to_string_lossy().to_string();
            h.app.services.pick_open = Some(Box::new(move |_| Box::pin(std::future::ready(Some(answer.clone())))));
            vfs::scoped(Some(vfs::PathScope::new([root.clone()])), || {
                h.step(&json!({"widget":"Import…"})).unwrap();
                h.frames(4);
            });
            let message = h.app.keymap.message.clone();
            if inside {
                assert_eq!(message, None);
                assert_eq!(effective(&h.app, "solid.extrude").as_deref(), Some("Shift+E"));
            } else {
                assert!(message.as_deref().is_some_and(|m| m.contains("outside the folders this agent may use")), "{message:?}");
                assert_eq!(effective(&h.app, "solid.extrude").as_deref(), Some("E"), "nothing imported");
            }
        }
    }
}
