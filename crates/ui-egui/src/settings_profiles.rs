//! Settings profiles (#6237): the input and interface settings kept together under a name, such
//! as "Laptop" and "Large monitor", and switched at once. A profile holds the preferences of the
//! Interface, Navigation and Sketch pages and the language, the keyboard shortcuts, the toolbar
//! and the S box favourites, and the imported theme it uses. Machine-specific settings stay out:
//! the 3D mouse, graphics, recent files and the AI pages.
//!
//! A profile is kept, exported and imported as one `solidcraft-profile/1` document. Every
//! document is checked by [`sanitize`] first: unknown keys are dropped, sizes are capped, and a
//! preference this version can't read is left out. Applying one sets each preference through
//! [`prefs::set`], so it is checked again and has the same effects as a change in Preferences;
//! one that is refused there (an interface font this computer lacks) keeps its current value.
//! Saved theme option maps are restored intact, retaining stale entries for fallback notices.
//!
//! The `settings.profile.*` commands reach the profiles; the engine only checks their
//! parameters, and [`command`] does the rest after `SolveApp::run`.

use serde_json::{Map, Value, json};
use solidcraft_engine::{MAX_PROFILE_NAME, profile_name};

use crate::{SolveApp, prefs, theme_files, ui_kit};

/// The format tag of a profile document.
pub const FORMAT: &str = "solidcraft-profile/1";
/// Most profiles kept.
pub const MAX_PROFILES: usize = 32;
/// Most bytes read from a profile file.
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
/// Most bytes in one checked profile document.
const MAX_PROFILE_BYTES: usize = 1024 * 1024;
const MAX_SHORTCUTS: usize = 2000;
const MAX_FAVOURITES: usize = 64;
/// Most imported themes one profile carries (the one it uses).
const MAX_THEMES: usize = 4;
/// Most characters in a command id or a key name.
const MAX_ID: usize = 200;

/// The Preferences pages whose settings a profile holds.
const PAGES: [usize; 3] = [prefs::PAGE_INTERFACE, prefs::PAGE_NAVIGATION, prefs::PAGE_SKETCH];
/// Settings a profile holds from other pages.
const ALSO: [&str; 1] = ["language"];

/// Settings of those pages that belong to the computer, not the person: the 3D mouse (its
/// calibration, driver and axes) and the cursor flag older releases saved.
fn machine_specific(key: &str) -> bool {
    key.starts_with("space_mouse_") || key == "orbit_cursor"
}

/// The preferences a profile holds, by their stored key.
pub fn keys() -> impl Iterator<Item = &'static str> {
    prefs::FIELD_PAGES
        .iter()
        .filter(|(key, page)| ALSO.contains(key) || page.is_some_and(|page| PAGES.contains(&page)))
        .map(|(key, _)| *key)
        .filter(|key| !machine_specific(key))
}

/// The kept profiles, which one was applied or saved last, and the Preferences section's state.
#[derive(Default)]
pub struct Profiles {
    /// Checked `solidcraft-profile/1` documents, in the order they were added.
    pub list: Vec<Value>,
    /// The profile applied or saved last, while it exists.
    pub active: Option<String>,
    /// The name typed in Preferences.
    draft: String,
    /// The last action's outcome in Preferences.
    message: Option<Outcome>,
}

/// What a profile action in Preferences reports. Kept untranslated: applying a profile can
/// change the language, and the report is shown in the new one.
enum Outcome {
    /// It worked: the message id.
    Done(&'static str),
    /// A profile applied with this many settings refused.
    Skipped(usize),
    Failed(String),
}

impl Profiles {
    /// What the app keeps between runs.
    pub fn prefs(&self) -> Value {
        json!({ "active": self.active, "profiles": self.list })
    }

    /// Back from [`Profiles::prefs`]: profiles that fail the checks, and repeated names, are dropped.
    pub fn load(&mut self, v: &Value) {
        self.list.clear();
        for doc in v.get("profiles").and_then(Value::as_array).into_iter().flatten().take(MAX_PROFILES) {
            match sanitize(doc) {
                Ok(doc) if self.find(name_of(&doc)).is_none() => self.list.push(doc),
                Ok(_) => {}
                Err(e) => log::warn!("settings profiles: one is dropped: {e}"),
            }
        }
        self.active = v.get("active").and_then(Value::as_str).filter(|name| self.find(name).is_some()).map(str::to_string);
    }

    /// The profile names, in order.
    pub fn names(&self) -> Vec<String> {
        self.list.iter().map(|doc| name_of(doc).to_string()).collect()
    }

    fn find(&self, name: &str) -> Option<usize> {
        self.list.iter().position(|doc| name_of(doc) == name)
    }

    fn get(&self, name: &str) -> Result<&Value, String> {
        self.find(name).and_then(|i| self.list.get(i)).ok_or_else(|| format!("no settings profile `{name}`"))
    }

    /// `name`, or the first of `name 2`, `name 3`… no profile has yet.
    fn free_name(&self, name: &str) -> String {
        if self.find(name).is_none() {
            return name.to_string();
        }
        (2..=MAX_PROFILES + 1)
            .map(|n| {
                let suffix = format!(" {n}");
                let base: String = name.chars().take(MAX_PROFILE_NAME - suffix.chars().count()).collect();
                format!("{}{suffix}", base.trim_end())
            })
            .find(|candidate| self.find(candidate).is_none())
            .unwrap_or_else(|| name.to_string())
    }
}

fn name_of(doc: &Value) -> &str {
    doc.get("name").and_then(Value::as_str).unwrap_or_default()
}

fn set_name(doc: &mut Value, name: &str) {
    if let Some(o) = doc.as_object_mut() {
        o.insert("name".into(), json!(name));
    }
}

/// The current settings as a profile called `name`.
pub fn capture(app: &SolveApp, name: &str) -> Value {
    let all = serde_json::to_value(&app.preferences).unwrap_or_default();
    let preferences: Map<String, Value> = keys().filter_map(|key| Some((key.to_string(), all.get(key)?.clone()))).collect();
    let themes: Vec<Value> = app.preferences.custom_themes.iter().filter(|t| t.id == app.preferences.theme).map(theme_document).collect();
    json!({
        "format": FORMAT,
        "name": name,
        "preferences": preferences,
        "shortcuts": app.keymap.prefs(),
        "favourites": app.sbox.pinned,
        "toolbar": app.toolbar_custom.prefs(),
        "themes": themes,
    })
}

/// An imported theme as a theme file that also keeps the id the profile's `theme` names.
fn theme_document(theme: &theme_files::ThemeFile) -> Value {
    let mut v = serde_json::to_value(theme).unwrap_or_default();
    if let Some(o) = v.as_object_mut() {
        o.insert("format".into(), json!(theme_files::FORMAT));
    }
    v
}

/// A theme id as a profile may name one: lower-case letters, digits and dashes.
fn theme_id(id: &str) -> Option<&str> {
    (!id.is_empty() && id.len() <= MAX_PROFILE_NAME && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')).then_some(id)
}

/// A profile document checked and cut down to what this version keeps: the format and a usable
/// name are required; unknown keys, preferences this version can't read, and themes that fail
/// their checks are dropped; lists and texts are capped.
pub fn sanitize(v: &Value) -> Result<Value, String> {
    let o = v.as_object().ok_or("not a SolidCraft settings profile")?;
    if o.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err(format!("not a SolidCraft settings profile: `format` must be \"{FORMAT}\""));
    }
    let name = o
        .get("name")
        .and_then(Value::as_str)
        .and_then(profile_name)
        .ok_or_else(|| format!("`name` must be 1 to {MAX_PROFILE_NAME} characters, without control characters"))?;
    let mut preferences = Map::new();
    if let Some(src) = o.get("preferences") {
        let src = src.as_object().ok_or("`preferences` must be an object")?;
        let defaults = serde_json::to_value(prefs::Prefs::default()).map_err(|e| e.to_string())?;
        for key in keys() {
            let Some(value) = src.get(key) else { continue };
            let mut probe = defaults.clone();
            if let Some(p) = probe.as_object_mut() {
                p.insert(key.to_string(), value.clone());
            }
            if serde_json::from_value::<prefs::Prefs>(probe).is_ok() {
                preferences.insert(key.to_string(), value.clone());
            } else {
                log::warn!("settings profile `{name}`: `{key}` is not understood by this version; left out");
            }
        }
    }
    let text = |v: &Value| v.as_str().filter(|s| !s.is_empty() && s.chars().count() <= MAX_ID).map(str::to_string);
    let shortcuts: Map<String, Value> = o
        .get("shortcuts")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(id, _)| id.chars().count() <= MAX_ID)
        .filter_map(|(id, keys)| Some((id.clone(), shortcut_keys(keys)?)))
        .take(MAX_SHORTCUTS)
        .collect();
    let favourites: Vec<String> = o.get("favourites").and_then(Value::as_array).into_iter().flatten().filter_map(text).take(MAX_FAVOURITES).collect();
    let mut toolbar = crate::toolbar_custom::ToolbarCustom::default();
    if let Some(t) = o.get("toolbar") {
        toolbar.load(t);
    }
    let mut themes = Vec::new();
    for t in o.get("themes").and_then(Value::as_array).into_iter().flatten().take(MAX_THEMES) {
        match theme_files::from_value(t) {
            Ok(mut theme) => {
                if let Some(id) = t.get("id").and_then(Value::as_str).and_then(theme_id) {
                    theme.id = id.to_string();
                }
                themes.push(theme_document(&theme));
            }
            Err(e) => log::warn!("settings profile `{name}`: a theme is left out: {e}"),
        }
    }
    let doc = json!({
        "format": FORMAT,
        "name": name,
        "preferences": preferences,
        "shortcuts": shortcuts,
        "favourites": favourites,
        "toolbar": toolbar.prefs(),
        "themes": themes,
    });
    if doc.to_string().len() > MAX_PROFILE_BYTES {
        return Err(format!("a settings profile is at most {} KiB", MAX_PROFILE_BYTES / 1024));
    }
    Ok(doc)
}

/// A command's keys in a profile, as the keymap saves them (`""`, one key or a list of them), or
/// `None` when they can't be read or a key name is too long.
fn shortcut_keys(v: &Value) -> Option<Value> {
    let keys = crate::keymap::saved_keys(v)?;
    keys.iter().all(|k| k.chars().count() <= MAX_ID).then(|| crate::keymap::keys_value(&keys))
}

/// Apply a checked profile: its themes, each preference (through [`prefs::set`], except saved option maps), the
/// shortcuts, the favourites and the toolbar. Returns the preferences that were refused.
fn apply(app: &mut SolveApp, doc: &Value) -> Vec<String> {
    let mut theme_ids = std::collections::BTreeMap::new();
    for t in doc.get("themes").and_then(Value::as_array).into_iter().flatten() {
        let Ok(mut theme) = theme_files::from_value(t) else { continue };
        let wanted = t.get("id").and_then(Value::as_str).and_then(theme_id).unwrap_or(&theme.id).to_string();
        theme.id = wanted.clone();
        if let Ok(id) = theme_files::add(&mut app.preferences.custom_themes, theme) {
            theme_ids.insert(wanted, id);
        }
    }
    // Resolve imported identities before migrating retired built-in cards. Migrate the
    // theme and its option maps together: applying `theme` first and then replacing the
    // saved maps would otherwise discard the old card's selected contrast.
    let mut preferences = doc.get("preferences").and_then(Value::as_object).cloned().unwrap_or_default();
    if let Some(id) = preferences.get("theme").and_then(Value::as_str).and_then(|id| theme_ids.get(id)).cloned() {
        preferences.insert("theme".into(), json!(id));
    }
    if let Some(options) = preferences.get("theme_options").and_then(Value::as_object) {
        let remapped: Map<String, Value> = options.iter().map(|(id, values)| (theme_ids.get(id).unwrap_or(id).clone(), values.clone())).collect();
        preferences.insert("theme_options".into(), Value::Object(remapped));
    }
    let options = preferences
        .get("theme_options")
        .map(|value| serde_json::from_value::<std::collections::BTreeMap<String, std::collections::BTreeMap<String, Value>>>(value.clone()))
        .transpose();
    // Leave malformed raw fields intact for the existing per-key skipped reporting.
    // In particular, a bad option map must not become an empty map that clears saved values.
    if let Ok(options) = options {
        let theme = preferences.get("theme").and_then(Value::as_str).map(str::to_owned);
        let legacy = theme.as_deref().and_then(theme_files::legacy_gruvbox).is_some();
        let mut migrated = prefs::Prefs {
            theme: theme.clone().unwrap_or_else(|| "dark".into()),
            theme_options: options.unwrap_or_else(|| app.preferences.theme_options.clone()),
            ..Default::default()
        };
        prefs::migrate_gruvbox(&mut migrated);
        if theme.is_some() {
            preferences.insert("theme".into(), json!(migrated.theme));
        }
        if preferences.contains_key("theme_options") || legacy {
            preferences.insert("theme_options".into(), json!(migrated.theme_options));
        }
    }
    let mut skipped = Vec::new();
    for (key, value) in &preferences {
        if key == "theme_options" {
            // Restoring saved state keeps stale entries just like loading preferences. The
            // control channel's strict merge is for new edits, not profile restoration.
            match serde_json::from_value::<std::collections::BTreeMap<String, std::collections::BTreeMap<String, Value>>>(value.clone()) {
                Ok(options) => {
                    app.preferences.theme_options = options;
                }
                Err(_) => skipped.push(key.clone()),
            }
            continue;
        }
        if let Err(e) = prefs::set(app, &json!({ key: value })) {
            log::warn!("settings profile: `{key}` keeps its current value: {e}");
            skipped.push(key.clone());
        }
    }
    if let Some(s) = doc.get("shortcuts") {
        app.keymap.load(s);
    }
    if let Some(f) = doc.get("favourites").and_then(Value::as_array) {
        app.sbox.pinned = f.iter().filter_map(|x| x.as_str().map(str::to_string)).collect();
    }
    if let Some(t) = doc.get("toolbar") {
        app.toolbar_custom.load(t);
    }
    skipped
}

/// A profile file read and checked, refusing one too large to be a profile.
fn read_file(path: &str) -> Result<Value, String> {
    let size = std::fs::metadata(path).map_err(|e| format!("{path}: {e}"))?.len();
    if size > MAX_FILE_BYTES {
        return Err(format!("{path}: a settings profile file is at most {} MiB", MAX_FILE_BYTES / 1024 / 1024));
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("{path}: not a settings profile: {e}"))
}

/// Add a checked profile under its name, or `name` when given, made unique; never switches to it.
fn add(app: &mut SolveApp, doc: &Value, name: Option<&str>) -> Result<String, String> {
    let mut doc = sanitize(doc)?;
    if app.settings_profiles.list.len() >= MAX_PROFILES {
        return Err(format!("at most {MAX_PROFILES} settings profiles; delete one first"));
    }
    let name = app.settings_profiles.free_name(name.unwrap_or(name_of(&doc)));
    set_name(&mut doc, &name);
    app.settings_profiles.list.push(doc);
    Ok(name)
}

/// The rest of a `settings.profile.*` command, after the engine checked its parameters (`v`, its echo).
pub fn command(app: &mut SolveApp, id: &str, v: &Value) -> Result<Value, String> {
    let name = v.get("name").and_then(Value::as_str).unwrap_or_default();
    match id {
        "settings.profile.list" => Ok(json!({ "profiles": app.settings_profiles.names(), "active": app.settings_profiles.active })),
        "settings.profile.save" => {
            let doc = capture(app, name);
            match app.settings_profiles.find(name) {
                Some(i) => {
                    if let Some(old) = app.settings_profiles.list.get_mut(i) {
                        *old = doc;
                    }
                }
                None if app.settings_profiles.list.len() >= MAX_PROFILES => {
                    return Err(format!("at most {MAX_PROFILES} settings profiles; delete one first"));
                }
                None => app.settings_profiles.list.push(doc),
            }
            app.settings_profiles.active = Some(name.to_string());
            Ok(json!({ "name": name }))
        }
        "settings.profile.apply" => {
            let doc = app.settings_profiles.get(name)?.clone();
            let skipped = apply(app, &doc);
            app.settings_profiles.active = Some(name.to_string());
            let applied = doc.get("preferences").and_then(Value::as_object).map_or(0, |p| p.len().saturating_sub(skipped.len()));
            Ok(json!({ "name": name, "applied": applied, "skipped": skipped }))
        }
        "settings.profile.rename" => {
            let to = v.get("to").and_then(Value::as_str).unwrap_or_default();
            let i = app.settings_profiles.find(name).ok_or_else(|| format!("no settings profile `{name}`"))?;
            if to != name && app.settings_profiles.find(to).is_some() {
                return Err(format!("another settings profile is called `{to}`"));
            }
            if let Some(doc) = app.settings_profiles.list.get_mut(i) {
                set_name(doc, to);
            }
            if app.settings_profiles.active.as_deref() == Some(name) {
                app.settings_profiles.active = Some(to.to_string());
            }
            Ok(json!({ "name": to }))
        }
        "settings.profile.delete" => {
            let i = app.settings_profiles.find(name).ok_or_else(|| format!("no settings profile `{name}`"))?;
            app.settings_profiles.list.remove(i);
            if app.settings_profiles.active.as_deref() == Some(name) {
                app.settings_profiles.active = None;
            }
            Ok(json!({ "deleted": name }))
        }
        "settings.profile.export" => {
            let doc = app.settings_profiles.get(name)?.clone();
            if let Some(path) = v.get("path").and_then(Value::as_str) {
                let text = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
                std::fs::write(path, text).map_err(|e| format!("{path}: {e}"))?;
                return Ok(json!({ "profile": doc, "path": path }));
            }
            Ok(json!({ "profile": doc }))
        }
        "settings.profile.import" => {
            let doc = match v.get("path").and_then(Value::as_str) {
                Some(path) => read_file(path)?,
                None => v.get("profile").cloned().unwrap_or_default(),
            };
            let name = add(app, &doc, v.get("name").and_then(Value::as_str))?;
            Ok(json!({ "name": name }))
        }
        _ => Err(format!("`{id}` is not a settings profile command")),
    }
}

/// Run a profile command from Preferences and show its outcome there.
fn act(app: &mut SolveApp, id: &str, params: Value, done: &'static str) {
    let result = app.run(id, params);
    app.settings_profiles.message = Some(match result {
        Ok(v) => match v.get("skipped").and_then(Value::as_array).map_or(0, Vec::len) {
            0 => Outcome::Done(done),
            skipped => Outcome::Skipped(skipped),
        },
        Err(e) => Outcome::Failed(e),
    });
}

/// The Settings profiles section of the General page: the active profile (choosing one applies
/// it), a name to save the current settings under, and Rename, Delete, Export… and Import….
pub(crate) fn section(app: &mut SolveApp, ui: &mut egui::Ui, labels: f32) {
    let t = crate::theme::Tokens::get();
    if app.settings_profiles.draft.is_empty()
        && let Some(active) = &app.settings_profiles.active
    {
        app.settings_profiles.draft = active.clone();
    }
    ui_kit::settings_section(ui, "profiles", app.tr("prefs.profiles"), |ui| {
        ui_kit::text(ui, app.tr("prefs.profiles_note"), ui_kit::Text::Hint("profiles"));
        ui_kit::settings_form(ui, labels, |ui| {
            let names = app.settings_profiles.names();
            let current = app.settings_profiles.active.clone().unwrap_or_else(|| crate::i18n::tr_label("None").to_string());
            let mut chosen = None;
            ui_kit::row(ui, app.tr("prefs.profile_active"), |ui| {
                ui_kit::settings_control_width(ui, t.form_card_width - t.space_4, |ui| {
                    ui_kit::dropdown_menu(ui, "pref_profile", &current, |ui| {
                        for name in &names {
                            if ui_kit::list_picker(ui, name, name, app.settings_profiles.active.as_ref() == Some(name)).clicked() {
                                chosen = Some(name.clone());
                            }
                        }
                    });
                });
            });
            if let Some(name) = chosen {
                app.settings_profiles.draft = name.clone();
                act(app, "settings.profile.apply", json!({ "name": name }), "prefs.profile_applied");
            }
            if let Some(active) = app.settings_profiles.active.clone()
                && app.settings_profiles.get(&active).is_ok_and(|doc| *doc != capture(app, &active))
            {
                ui_kit::text(ui, app.tr("prefs.profile_changed"), ui_kit::Text::Note("profile_changed"));
            }
            ui_kit::row(ui, crate::i18n::tr_label("Name"), |ui| {
                let mut draft = std::mem::take(&mut app.settings_profiles.draft);
                let (enter, _) = ui_kit::text_field(ui, &mut draft, app.tr("prefs.profile_name_hint"));
                app.settings_profiles.draft = draft;
                if enter {
                    let name = app.settings_profiles.draft.clone();
                    act(app, "settings.profile.save", json!({ "name": name }), "prefs.profile_saved");
                }
            });
        });
        let draft = app.settings_profiles.draft.trim().to_string();
        let active = app.settings_profiles.active.clone();
        ui.horizontal_wrapped(|ui| {
            if ui_kit::button(ui, app.tr("prefs.profile_save"), ui_kit::Button::Enabled(profile_name(&draft).is_some())).clicked() {
                act(app, "settings.profile.save", json!({ "name": draft }), "prefs.profile_saved");
            }
            let renamable = active.as_ref().is_some_and(|a| *a != draft) && profile_name(&draft).is_some();
            if ui_kit::button(ui, app.tr("prefs.profile_rename"), ui_kit::Button::Enabled(renamable)).clicked()
                && let Some(name) = &active
            {
                act(app, "settings.profile.rename", json!({ "name": name, "to": draft }), "prefs.profile_renamed");
            }
            if ui_kit::button(ui, crate::i18n::tr_label("Delete"), ui_kit::Button::Enabled(active.is_some())).clicked()
                && let Some(name) = &active
            {
                act(app, "settings.profile.delete", json!({ "name": name }), "prefs.profile_deleted");
                app.settings_profiles.draft.clear();
            }
            if ui_kit::button(ui, app.tr("prefs.profile_export"), ui_kit::Button::Enabled(active.is_some())).clicked()
                && let Some(name) = active.clone()
            {
                app.pick_save(&format!("{name}.solidcraft-profile.json"), &["json"], move |app, path| {
                    if let Some(path) = path {
                        act(app, "settings.profile.export", json!({ "name": name, "path": path }), "prefs.profile_exported");
                    }
                });
            }
            if ui_kit::button(ui, crate::i18n::tr_label("Import…"), ui_kit::Button::Normal).clicked() {
                app.pick_open_for(&["json"], |app, path| {
                    if let Some(path) = path {
                        act(app, "settings.profile.import", json!({ "path": path }), "prefs.profile_imported");
                    }
                });
            }
        });
        let locale = crate::i18n::Locale::for_language(&app.preferences.language);
        match &app.settings_profiles.message {
            Some(Outcome::Done(id)) => {
                ui_kit::text(ui, locale.text(id), ui_kit::Text::Note("profile_message"));
            }
            Some(Outcome::Skipped(n)) => {
                let text = locale.format("prefs.profile_skipped", &[("n", &n.to_string())]);
                ui_kit::text(ui, &text, ui_kit::Text::Error("profile_message"));
            }
            Some(Outcome::Failed(e)) => {
                ui_kit::text(ui, e, ui_kit::Text::Error("profile_message"));
            }
            None => {}
        }
    });
}
