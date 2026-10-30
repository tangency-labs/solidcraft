//! Preferences › Plugins: packages installed under the per-user data folder.
//!
//! A plugin is a directory of `{data}/plugins/<id>/plugin.json`. The page searches that list.
//! It reads `plugin.json` and records which ids are turned off. It does not load or run anything
//! from the folder. Explicit `plugin.run` calls use the native component host and honour
//! `disabled_plugins`; listing or enabling a package never executes its code.

use std::path::{Path, PathBuf};

use egui::RichText;
use serde::Deserialize;

use crate::SolveApp;

/// How many turned-off ids a preferences file may name.
pub const MAX_DISABLED: usize = 256;
/// `plugin.json` larger than this is refused.
const MAX_MANIFEST_BYTES: u64 = 16 * 1024;

/// One installed plugin, as its manifest describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plugin {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
}

/// A folder under the plugins directory that is not a plugin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skipped {
    pub folder: String,
    pub reason: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    id: String,
    name: String,
    version: String,
    #[serde(default)]
    description: String,
}

/// The plugins directory: `$XDG_DATA_HOME/solidcraft/plugins` (and the platform equivalents of
/// [`solidcraft_engine::recovery::data_dir`]). `None` when that folder cannot be named.
pub fn plugins_dir() -> Option<PathBuf> {
    solidcraft_engine::recovery::data_dir().map(|d| d.join("plugins"))
}

/// An id a folder and a manifest may share: 1–64 characters, lowercase letters, digits and
/// hyphens, not starting or ending with a hyphen.
pub fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    (1..=64).contains(&b.len())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
        && b[0] != b'-'
        && *b.last().unwrap_or(&b'-') != b'-'
}

fn plain(text: &str, max: usize) -> bool {
    !text.is_empty() && text.chars().count() <= max && text.chars().all(|c| !c.is_control())
}

fn check(m: Manifest, folder: &str) -> Result<Plugin, String> {
    if m.id != folder {
        return Err(format!("plugin.json id `{}` does not match the folder `{folder}`", m.id));
    }
    if !valid_id(&m.id) {
        return Err("the id must be 1–64 lowercase letters, digits and hyphens".into());
    }
    let name = m.name.trim();
    if !plain(name, 80) {
        return Err("the name must be 1–80 characters with no line breaks".into());
    }
    let version = m.version.trim();
    if !plain(version, 32) || !version.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+')) {
        return Err("the version must be 1–32 letters, digits, dots, hyphens or plus signs".into());
    }
    let description = m.description.trim();
    if description.chars().count() > 240 || description.chars().any(|c| c.is_control()) {
        return Err("the description must be at most 240 characters with no line breaks".into());
    }
    Ok(Plugin { id: m.id, name: name.to_string(), version: version.to_string(), description: description.to_string() })
}

fn read_plugin(dir: &Path, folder: &str) -> Result<Plugin, String> {
    let path = dir.join("plugin.json");
    let meta = std::fs::metadata(&path).map_err(|_| "no plugin.json".to_string())?;
    if !meta.is_file() || meta.len() > MAX_MANIFEST_BYTES {
        return Err("plugin.json is missing or larger than 16 KiB".into());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("plugin.json could not be read ({e})"))?;
    let parsed = solidcraft_engine::plugin::parse_manifest(text.as_bytes(), folder).map_err(|e| e.to_string())?;
    check(Manifest { id: parsed.id, name: parsed.name, version: parsed.version, description: parsed.description }, folder)
}

/// The plugins in `dir`, and the folders that were not. A missing directory is an empty list.
pub fn scan(dir: &Path) -> (Vec<Plugin>, Vec<Skipped>) {
    let mut plugins = Vec::new();
    let mut skipped = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (plugins, skipped);
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        if !kind.is_dir() {
            continue;
        }
        let Some(folder) = entry.file_name().into_string().ok() else {
            skipped.push(Skipped { folder: String::new(), reason: "the folder name is not text".into() });
            continue;
        };
        if folder.starts_with('.') {
            continue;
        }
        match read_plugin(&entry.path(), &folder) {
            Ok(plugin) => plugins.push(plugin),
            Err(reason) => skipped.push(Skipped { folder, reason }),
        }
    }
    plugins.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    skipped.sort_by(|a, b| a.folder.cmp(&b.folder));
    (plugins, skipped)
}

/// Exact integer controls retain all u64 fuel values, without a float slider conversion.
pub(crate) struct LimitInputs {
    applied: solidcraft_engine::plugin::HostLimits,
    fuel: String,
    memory: String,
}
impl From<solidcraft_engine::plugin::HostLimits> for LimitInputs {
    fn from(applied: solidcraft_engine::plugin::HostLimits) -> Self {
        Self { fuel: applied.max_fuel.to_string(), memory: applied.max_memory_mib.to_string(), applied }
    }
}
pub(crate) fn host_limits(p: &crate::prefs::Prefs) -> solidcraft_engine::plugin::HostLimits {
    solidcraft_engine::plugin::HostLimits { max_fuel: p.plugin_max_fuel, max_memory_mib: p.plugin_max_memory_mib }
}
pub(crate) fn sync_policy(app: &mut SolveApp) {
    app.session.disabled_plugins.clone_from(&app.preferences.disabled_plugins);
    app.session.plugin_limits = host_limits(&app.preferences);
}
fn parsed_limits(input: &LimitInputs) -> Option<solidcraft_engine::plugin::HostLimits> {
    let limits =
        solidcraft_engine::plugin::HostLimits { max_fuel: input.fuel.trim().parse().ok()?, max_memory_mib: input.memory.trim().parse().ok()? };
    limits.validate().ok()?;
    Some(limits)
}
pub(crate) fn resource_pending(app: &SolveApp) -> bool {
    let current = host_limits(&app.preferences);
    app.prefs_window.plugin_limit_inputs.as_ref().is_some_and(|input| {
        input.applied == current && (input.fuel != current.max_fuel.to_string() || input.memory != current.max_memory_mib.to_string())
    })
}
/// Enter, Apply and OK commit both exact integers together. An incomplete edit cannot save a prefix.
pub(crate) fn commit_resource_inputs(app: &mut SolveApp) -> Result<(), String> {
    let Some(input) = app.prefs_window.plugin_limit_inputs.take() else { return Ok(()) };
    // A control client has already changed the preferences: never overwrite that newer policy.
    if input.applied != host_limits(&app.preferences) {
        return Ok(());
    }
    let Some(limits) = parsed_limits(&input) else {
        app.prefs_window.plugin_limit_inputs = Some(input);
        return Err(app.tr("prefs.plugin_limits_invalid").to_string());
    };
    app.preferences.plugin_max_fuel = limits.max_fuel;
    app.preferences.plugin_max_memory_mib = limits.max_memory_mib;
    app.prefs_window.plugin_limit_inputs = Some(limits.into());
    sync_policy(app);
    Ok(())
}
fn resource_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let current = host_limits(&app.preferences);
    let mut input = app.prefs_window.plugin_limit_inputs.take().filter(|v| v.applied == current).unwrap_or_else(|| current.into());
    let (fuel_enter, _) = crate::ui_kit::text_row(ui, app.tr("prefs.plugin_max_fuel"), &mut input.fuel, "");
    let (memory_enter, _) = crate::ui_kit::text_row(ui, app.tr("prefs.plugin_max_memory"), &mut input.memory, "");
    if parsed_limits(&input).is_none() {
        crate::ui_kit::text(ui, app.tr("prefs.plugin_limits_invalid"), crate::ui_kit::Text::Error("plugin_limits"));
    }
    crate::ui_kit::text(ui, app.tr("prefs.plugin_limits_note"), crate::ui_kit::Text::Hint("plugin_limits"));
    app.prefs_window.plugin_limit_inputs = Some(input);
    if (fuel_enter || memory_enter)
        && let Err(error) = commit_resource_inputs(app)
    {
        app.set_status(error, true);
    }
    ui.separator();
}

/// Whether an installed plugin is offered. Unknown ids are on; an id in `disabled_plugins` is off.
pub fn enabled(app: &SolveApp, id: &str) -> bool {
    !app.preferences.disabled_plugins.contains(id)
}

fn matches_query(plugin: &Plugin, query: &str) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return true;
    }
    let query = query.to_lowercase();
    [plugin.name.as_str(), plugin.id.as_str(), plugin.description.as_str(), plugin.version.as_str()]
        .iter()
        .any(|text| text.to_lowercase().contains(&query))
}

/// The Plugins page. Desktop only: the browser build has no per-user plugin folder.
pub fn page(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = crate::theme::Tokens::get();
    if cfg!(target_arch = "wasm32") {
        ui.label(crate::i18n::tr("ui.plugins_desktop"));
        return;
    }
    let Some(dir) = plugins_dir() else {
        ui.label(crate::i18n::tr("ui.plugins_desktop"));
        return;
    };
    let path = dir.display().to_string();
    let searching = crate::prefs_search::querying(ui);
    // The page's own box is for browsing Plugins. The dialog search is the filter while it is active.
    if !searching {
        crate::ui_kit::search_field(ui, ui.id().with("plugins_search"), &mut app.prefs_window.plugin_query, crate::i18n::tr("ui.plugins_search"));
        ui.add_space(8.0);
    }
    if searching {
        crate::prefs_search::begin_section(ui, crate::i18n::tr("ui.plugins"));
    } else {
        ui.add(
            egui::Label::new(RichText::new(crate::i18n::format("ui.plugins_note", &[("path", &path)])).size(t.font_small).color(t.text_dim)).wrap(),
        );
    }
    resource_controls(app, ui);
    ui.add_space(8.0);
    if !searching && crate::ui_kit::button(ui, crate::i18n::tr_label("Show in folder"), crate::ui_kit::Button::Normal).clicked() {
        // `show_in_folder` opens the parent of the path it is given, so the argument is a name
        // inside the plugins directory (it need not exist).
        if let Err(e) = std::fs::create_dir_all(&dir) {
            app.set_status(format!("Could not create the plugins folder: {e}"), true);
        } else if let Some(show) = &app.services.show_in_folder {
            show(&dir.join("plugin.json").display().to_string());
        }
    }
    ui.add_space(8.0);
    let (plugins, skipped) = scan(&dir);
    let query = app.prefs_window.plugin_query.clone();
    if plugins.is_empty() && !searching {
        ui.label(crate::i18n::tr("ui.plugins_empty"));
    }
    let mut shown = 0usize;
    for plugin in plugins.iter().filter(|plugin| searching || matches_query(plugin, &query)) {
        if searching && !crate::prefs_search::texts_hit(ui, &[&plugin.name, &plugin.id, &plugin.description, &plugin.version]) {
            continue;
        }
        shown += 1;
        if searching {
            crate::prefs_search::reveal(ui);
            crate::prefs_search::push_force(ui);
        }
        let mut on = enabled(app, &plugin.id);
        ui.horizontal(|ui| {
            let check = crate::ui_kit::checkbox(ui, "", &mut on);
            crate::scenario::publish_handle(&format!("prefs:plugin:{}", plugin.id), check.rect.center());
            if check.changed() {
                if on {
                    app.preferences.disabled_plugins.remove(&plugin.id);
                } else if app.preferences.disabled_plugins.len() < MAX_DISABLED {
                    app.preferences.disabled_plugins.insert(plugin.id.clone());
                }
            }
            ui.vertical(|ui| {
                ui.label(RichText::new(&plugin.name).strong());
                ui.label(
                    RichText::new(crate::i18n::format("ui.plugins_version", &[("version", &plugin.version)])).size(t.font_small).color(t.text_dim),
                );
                if !plugin.description.is_empty() {
                    ui.label(RichText::new(&plugin.description).size(t.font_small).color(t.text_dim));
                }
            });
        });
        ui.separator();
        if searching {
            crate::prefs_search::pop_force(ui);
        }
    }
    if !searching && !plugins.is_empty() && shown == 0 {
        ui.label(crate::i18n::tr("ui.plugins_none"));
    }
    if !searching && query.trim().is_empty() && !skipped.is_empty() {
        ui.add_space(6.0);
        ui.label(
            RichText::new(crate::i18n::format("ui.plugins_skipped", &[("count", &skipped.len().to_string())])).size(t.font_small).color(t.text_dim),
        );
        for skip in skipped.iter().take(8) {
            let who = if skip.folder.is_empty() { "folder" } else { skip.folder.as_str() };
            ui.label(RichText::new(format!("{who}: {}", skip.reason)).size(t.font_small).color(t.text_dim));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, folder: &str, body: &str) {
        let d = dir.join(folder);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("plugin.json"), body).unwrap();
    }

    #[test]
    fn a_folder_with_plugin_json_is_listed_and_a_bad_one_is_skipped() {
        let dir = std::env::temp_dir().join(format!("solidcraft-plugins-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write(
            &dir,
            "ftc-frc-parts",
            r#"{"id":"ftc-frc-parts","name":"FTC and FRC parts","version":"0.1.0","description":"Commercial robot parts."}"#,
        );
        write(&dir, "broken", r#"{"id":"other","name":"Nope","version":"1"}"#);
        std::fs::create_dir_all(dir.join("notes")).unwrap();
        let (plugins, skipped) = scan(&dir);
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].id, "ftc-frc-parts");
        assert_eq!(plugins[0].name, "FTC and FRC parts");
        assert!(skipped.iter().any(|s| s.folder == "broken" && s.reason.contains("does not match")));
        assert!(skipped.iter().any(|s| s.folder == "notes" && s.reason.contains("no plugin.json")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ids_and_manifests_refuse_what_would_not_fit_on_the_page() {
        assert!(valid_id("a"));
        assert!(valid_id("ftc-frc-parts"));
        assert!(!valid_id(""));
        assert!(!valid_id("-a"));
        assert!(!valid_id("a-"));
        assert!(!valid_id("FTC"));
        assert!(!valid_id(&"a".repeat(65)));
        let bad = Manifest { id: "gear".into(), name: "Gear\nbox".into(), version: "1".into(), description: String::new() };
        assert!(check(bad, "gear").is_err());
        let big = Manifest { id: "gear".into(), name: "Gear".into(), version: "1".into(), description: "x".repeat(241) };
        assert!(check(big, "gear").unwrap_err().contains("240"));
    }

    #[test]
    fn turning_a_plugin_off_is_saved_and_restore_defaults_turns_it_back_on() {
        use serde_json::json;
        let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        assert!(enabled(&app, "ftc-frc-parts"));
        crate::prefs::set(&mut app, &json!({"disabled_plugins": ["ftc-frc-parts"]})).unwrap();
        assert!(!enabled(&app, "ftc-frc-parts"));
        let err = crate::prefs::set(&mut app, &json!({"disabled_plugins": ["Bad Id"]})).unwrap_err();
        assert!(err.contains("disabled_plugins"), "{err}");
        assert!(!enabled(&app, "ftc-frc-parts"), "a refused list leaves the previous choice");
        let mut other = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        other.load_prefs(&app.prefs());
        assert!(!enabled(&other, "ftc-frc-parts"));
        crate::prefs::control(&mut app, &json!({"open": "Plugins", "button": "defaults"})).unwrap();
        assert_eq!(app.prefs_window.section, crate::prefs::PAGE_PLUGINS);
        assert!(enabled(&app, "ftc-frc-parts"));
        assert!(crate::prefs::control(&mut app, &json!({"open": "NoSuch"})).is_err());
    }

    #[test]
    fn resource_ceilings_preserve_defaults_precision_persistence_and_page_transactions() {
        use serde_json::json;
        let mut h = crate::scenario::Harness::new();
        let initial = host_limits(&h.app.preferences);
        assert_eq!(initial, solidcraft_engine::plugin::HostLimits::default());
        let before = h.app.session.doc.to_json();
        let changed = json!({"plugin_max_fuel":u64::MAX,"plugin_max_memory_mib":512});
        let reply = h.call("ui.prefs", changed.clone());
        assert_eq!(reply["ok"], true, "{reply}");
        assert_eq!(h.app.preferences.plugin_max_fuel, u64::MAX);
        assert_eq!(h.app.session.plugin_limits.max_memory_mib, 512);
        for bad in [
            json!({"plugin_max_fuel":0}),
            json!({"plugin_max_fuel":-1}),
            json!({"plugin_max_fuel":1.5}),
            json!({"plugin_max_memory_mib":4097}),
            json!({"plugin_max_memory_mib":0}),
        ] {
            assert_eq!(h.call("ui.prefs", bad)["ok"], false);
            assert_eq!(h.app.preferences.plugin_max_fuel, u64::MAX);
        }
        let mut restored = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert_eq!(host_limits(&restored.preferences), host_limits(&h.app.preferences));
        assert_eq!(restored.session.plugin_limits.max_fuel, u64::MAX);
        h.call("ui.prefs", json!({"open":"Plugins"}));
        h.call("ui.prefs", json!({"plugin_max_memory_mib":1024}));
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert_eq!(h.app.preferences.plugin_max_memory_mib, 512);
        h.call("ui.prefs", json!({"open":"Plugins","button":"defaults"}));
        assert_eq!(host_limits(&h.app.preferences), initial);
        if let Some(input) = &h.app.prefs_window.plugin_limit_inputs {
            assert_eq!(input.applied, initial);
        }
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert_eq!(h.app.preferences.plugin_max_memory_mib, 512);
        assert_eq!(h.app.session.doc.to_json(), before);
        assert!(h.app.session.undo.is_empty());
        let read = crate::prefs::lenient(&json!({"plugin_max_fuel":0,"plugin_max_memory_mib":99999,"language":"en"}));
        assert_eq!(host_limits(&read), initial);
        assert_eq!(read.language, "en");
    }
    #[test]
    fn resource_text_edits_require_complete_valid_confirmation() {
        use serde_json::json;
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"open":"Plugins"}));
        let initial = host_limits(&h.app.preferences);
        h.app.prefs_window.plugin_limit_inputs = Some(LimitInputs { applied: initial, fuel: u64::MAX.to_string(), memory: "512".into() });
        assert!(resource_pending(&h.app));
        assert_eq!(h.app.session.plugin_limits, initial, "typing alone does not authorize a higher ceiling");
        assert_eq!(h.call("ui.prefs", json!({}))["result"]["apply_enabled"], true);
        assert_eq!(h.call("ui.prefs", json!({"button":"apply"}))["ok"], true);
        assert_eq!(h.app.session.plugin_limits.max_fuel, u64::MAX);
        assert_eq!(h.app.session.plugin_limits.max_memory_mib, 512);
        let applied = h.app.session.plugin_limits;
        for text in ["", "0", "18446744073709551616", "1.5"] {
            h.app.prefs_window.plugin_limit_inputs = Some(LimitInputs { applied, fuel: text.into(), memory: "1024".into() });
            assert_eq!(h.call("ui.prefs", json!({"button":"ok"}))["ok"], false);
            assert!(h.app.prefs_window.open);
            assert_eq!(h.app.session.plugin_limits, applied);
        }
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert_eq!(h.app.session.plugin_limits, applied, "Cancel retains the last Apply");
        assert!(h.app.prefs_window.plugin_limit_inputs.is_none());
        h.call("ui.prefs", json!({"open":"Plugins","button":"defaults"}));
        assert_eq!(h.app.session.plugin_limits, initial);
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert_eq!(h.app.session.plugin_limits, applied);
    }
    #[test]
    fn search_matches_the_name_the_id_and_the_description() {
        let plugin = Plugin {
            id: "ftc-frc-parts".into(),
            name: "FTC and FRC parts".into(),
            version: "0.1.0".into(),
            description: "Commercial robot parts.".into(),
        };
        assert!(matches_query(&plugin, ""));
        assert!(matches_query(&plugin, " frc "));
        assert!(matches_query(&plugin, "FTC-FRC"));
        assert!(matches_query(&plugin, "commercial"));
        assert!(matches_query(&plugin, "0.1"));
        assert!(!matches_query(&plugin, "bearing"));
    }
}
