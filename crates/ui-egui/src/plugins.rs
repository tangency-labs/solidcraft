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
use crate::ui_kit::BackingTooltipExt as _;

/// Route namespaced entries through the existing approval gate and dedicated transaction worker.
pub(crate) fn normalize_request(session: &solidcraft_engine::Session, method: &str, params: &mut serde_json::Value) -> Result<(), String> {
    fn normalize(session: &solidcraft_engine::Session, step: &mut serde_json::Value) -> Result<(), String> {
        let Some(command) = step.get("command").or_else(|| step.get("id")).and_then(serde_json::Value::as_str) else { return Ok(()) };
        let inputs = step.get("params").unwrap_or(&serde_json::Value::Null);
        let Some(resolved) = solidcraft_engine::plugin::resolve_call(session, command, inputs).map_err(|e| e.to_string())? else { return Ok(()) };
        if let Some(step) = step.as_object_mut() {
            step.remove("id");
            step.insert("command".into(), "plugin.run".into());
            step.insert("params".into(), resolved);
        }
        Ok(())
    }
    match method {
        "engine.execute" | "command" => normalize(session, params),
        "engine.batch" | "engine.script" => {
            solidcraft_engine::script_steps(params).map_err(|e| e.to_string())?;
            let steps = if params.is_array() { params.as_array_mut() } else { params.get_mut("commands").and_then(serde_json::Value::as_array_mut) };
            if let Some(steps) = steps {
                for step in steps.iter_mut() {
                    normalize(session, step)?;
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

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
    let dir = if cfg!(target_arch = "wasm32") { None } else { plugins_dir() };
    page_in_dir(app, ui, dir.as_deref());
}

fn page_in_dir(app: &mut SolveApp, ui: &mut egui::Ui, dir: Option<&Path>) {
    crate::ui_kit::settings_page(ui, |ui| {
        crate::ui_kit::settings_heading(ui, solidcraft_ui_text::tr("ui.plugins"), solidcraft_ui_text::tr("prefs.plugins_intro"));
        let Some(dir) = dir else {
            ui.label(solidcraft_ui_text::tr("ui.plugins_desktop"));
            return;
        };
        // Install plugin…: pick a .zip, review what it asks for, install it.
        let install = crate::ui_kit::icon_label_button(ui, solidcraft_ui_text::tr("plugin_install.menu"), "import", true, false);
        crate::scenario::publish_target("prefs:plugins:install", install.rect);
        if install.clicked() {
            crate::plugin_install::start(app);
        }
        ui.add_space(8.0);
        resource_controls(app, ui);
        let t = solidcraft_theme::Tokens::get();
        let path = dir.display().to_string();
        let searching = crate::ui_kit::settings_search::querying(ui);
        // The page's own box is for browsing Plugins. The dialog search is the filter while it is active.
        if !searching {
            crate::ui_kit::search_field(
                ui,
                ui.scope_id().with("plugins_search"),
                &mut app.prefs_window.plugin_query,
                solidcraft_ui_text::tr("ui.plugins_search"),
            );
            ui.add_space(8.0);
        }
        let (plugins, skipped) = scan(dir);
        let query = app.prefs_window.plugin_query.clone();
        crate::ui_kit::settings_list_section(
            ui,
            "plugins_installed",
            solidcraft_ui_text::tr("prefs.plugins_installed"),
            Some(&plugins.len().to_string()),
            |ui| {
                if plugins.is_empty() && (!searching || crate::ui_kit::settings_search::shows_all(ui)) {
                    crate::ui_kit::text(ui, solidcraft_ui_text::tr("ui.plugins_empty"), crate::ui_kit::Text::Hint("plugins_empty"));
                }
                ui.spacing_mut().item_spacing.y = t.space_8;
                let mut shown = 0usize;
                for plugin in plugins.iter().filter(|plugin| searching || matches_query(plugin, &query)) {
                    if searching
                        && !crate::ui_kit::settings_search::shows_all(ui)
                        && !crate::ui_kit::settings_search::texts_hit(ui, &[&plugin.name, &plugin.id, &plugin.description, &plugin.version])
                    {
                        continue;
                    }
                    shown += 1;
                    if searching {
                        crate::ui_kit::settings_search::reveal(ui);
                        crate::ui_kit::settings_search::push_force(ui);
                    }
                    plugin_card(app, ui, dir, plugin);
                    if searching {
                        crate::ui_kit::settings_search::pop_force(ui);
                    }
                }
                if !searching && !plugins.is_empty() && shown == 0 {
                    ui.label(solidcraft_ui_text::tr("ui.plugins_none"));
                }
            },
        );
        if !searching && query.trim().is_empty() && !skipped.is_empty() {
            ui.add_space(6.0);
            ui.label(
                RichText::new(solidcraft_ui_text::format("ui.plugins_skipped", &[("count", &skipped.len().to_string())]))
                    .size(t.font_small)
                    .color(t.text_dim),
            );
            for skip in skipped.iter().take(8) {
                let who = if skip.folder.is_empty() { "folder" } else { skip.folder.as_str() };
                ui.label(RichText::new(format!("{who}: {}", skip.reason)).size(t.font_small).color(t.text_dim));
            }
        }
        if !searching {
            ui.horizontal(|ui| {
                let button = crate::ui_kit::icon_label_button(ui, solidcraft_ui_text::tr("prefs.plugins_show_folder"), "folder", true, false)
                    .backing_on_hover_text(solidcraft_ui_text::format("ui.plugins_note", &[("path", &path)]));
                crate::scenario::publish_target("prefs:plugins:folder", button.rect);
                if button.clicked() {
                    if let Err(e) = std::fs::create_dir_all(dir) {
                        app.set_status(format!("Could not create the plugins folder: {e}"), true);
                    } else if let Some(show) = &app.services.show_in_folder {
                        // The service opens the parent of the supplied file, even if it is absent.
                        show(&dir.join("plugin.json").display().to_string());
                    }
                }
            });
        }
    });
}

fn plugin_card(app: &mut SolveApp, ui: &mut egui::Ui, dir: &Path, plugin: &Plugin) {
    use crate::ui_kit;
    let mut on = enabled(app, &plugin.id);
    let was_on = on;
    let labels = ui_kit::PluginCardLabels {
        show_in_folder: solidcraft_ui_text::tr_label("Show in folder"),
        enabled: solidcraft_ui_text::tr("prefs.plugins_enabled"),
    };
    let card = ui
        .push_id(&plugin.id, |ui| ui_kit::settings_plugin_card(ui, &plugin.id, &plugin.name, &plugin.version, &plugin.description, &mut on, &labels))
        .inner;
    let response = card.response;
    let (folder, check) = card.inner;
    crate::scenario::publish_target(&format!("prefs:plugin:{}:folder", plugin.id), folder.rect);
    crate::scenario::publish_target(&format!("prefs:plugin:{}", plugin.id), check.rect);
    if folder.clicked()
        && let Some(show) = &app.services.show_in_folder
    {
        show(&dir.join(&plugin.id).join("plugin.json").display().to_string());
    }
    if was_on != on {
        if on {
            app.preferences.disabled_plugins.remove(&plugin.id);
        } else if app.preferences.disabled_plugins.len() < MAX_DISABLED {
            app.preferences.disabled_plugins.insert(plugin.id.clone());
        }
    }
    crate::scenario::publish_rect(&format!("prefs:plugin:{}:card", plugin.id), response.rect);
    let texts = [&plugin.name, &plugin.id, &plugin.description, &plugin.version];
    if let Some(text) = texts.iter().find(|text| crate::ui_kit::settings_search::texts_hit(ui, &[*text])) {
        crate::ui_kit::settings_search::remember(ui, text, response.rect);
    }
    for text in texts {
        crate::ui_kit::settings_search::follow(ui, text, response.rect);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page_frame(app: &mut SolveApp, ctx: &egui::Context, dir: &Path, events: Vec<egui::Event>) {
        crate::scenario::clear_handles();
        let mut output = crate::test_support::frame(
            ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(700.0, 800.0))),
                events,
                focused: true,
                ..Default::default()
            },
            |ui| crate::ui_kit::scope(ui, |ui| page_in_dir(app, ui, Some(dir))),
        );
        output.textures_delta.clear();
    }

    fn click_page(app: &mut SolveApp, ctx: &egui::Context, dir: &Path, handle: &str) {
        let at = crate::scenario::handle_at(handle).unwrap();
        page_frame(app, ctx, dir, crate::test_support::pointer(at, true));
        page_frame(app, ctx, dir, crate::test_support::pointer(at, false));
    }

    #[test]
    fn plugins_production_composition_publishes_page_heading_and_section_geometry() {
        let previous = solidcraft_theme::is_dark();
        for dark in [false, true] {
            solidcraft_theme::set_dark(dark);
            let mut h = crate::scenario::Harness::new();
            let dir = h.dir.join("page-geometry");
            let ctx = crate::fonts::test_context();
            for _ in 0..3 {
                page_frame(&mut h.app, &ctx, &dir, vec![]);
            }
            // The injected path goes through the production composition, including its
            // heading and settings_page scope; the fixture adds only the window's form style.
            let heading = crate::scenario::rect_of("prefs:page_heading").unwrap();
            let section = crate::scenario::rect_of("prefs:section:plugins_installed").unwrap();
            let body = crate::scenario::rect_of("prefs:section:plugins_installed:body").unwrap();
            let t = solidcraft_theme::Tokens::get();
            assert!(heading.height() >= t.space_24 && heading.bottom() < section.top());
            assert!((section.left() - heading.left()).abs() <= 1.0 && (section.right() - heading.right()).abs() <= 1.0);
            assert!((section.height() - (t.control_height + t.space_2)).abs() <= 1.0);
            assert!((body.top() - section.bottom() - t.space_10).abs() <= 1.0);
            assert!(!dir.exists(), "production layout does not create the plugin folder");
        }
        solidcraft_theme::set_dark(previous);
    }

    #[test]
    fn plugin_card_actions_share_a_row_at_normal_width_and_stay_inside_narrow_cards() {
        let previous = solidcraft_theme::is_dark();
        for dark in [false, true] {
            solidcraft_theme::set_dark(dark);
            for language in ["en", "de"] {
                let _language = solidcraft_ui_text::scope(language);
                for width in [646.0, 200.0] {
                    let mut h = crate::scenario::Harness::new();
                    let dir = h.dir.join("plugin-action-layout");
                    write(&dir, "public-one", r#"{"id":"public-one","name":"Public one","version":"1.2.3","description":"Public fixture."}"#);
                    let ctx = crate::fonts::test_context();
                    let mut clip = egui::Rect::NOTHING;
                    let mut painted = Vec::new();
                    for _ in 0..3 {
                        crate::scenario::clear_handles();
                        let mut output = crate::test_support::frame(
                            &ctx,
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width + 16.0, 800.0))),
                                ..Default::default()
                            },
                            |ui| {
                                ui.set_width(width);
                                clip = ui.clip_rect();
                                crate::ui_kit::scope(ui, |ui| page_in_dir(&mut h.app, ui, Some(&dir)));
                            },
                        );
                        output.textures_delta.clear();
                        painted = output.shapes;
                    }
                    let card = crate::scenario::rect_of("prefs:plugin:public-one:card").unwrap();
                    let folder = crate::scenario::rect_of("prefs:plugin:public-one:folder").unwrap();
                    let checkbox = crate::scenario::rect_of("prefs:plugin:public-one").unwrap();
                    assert!(card.width() <= width + 1.0, "{language}, {width}: card stays within the page");
                    for action in [folder, checkbox] {
                        assert!(card.expand(1.0).contains_rect(action), "{language}, {width}: action stays inside its card");
                        assert!(clip.contains_rect(action), "{language}, {width}: action is not clipped");
                    }
                    if width == 646.0 {
                        assert!((folder.center().y - checkbox.center().y).abs() <= 1.0, "{language}: normal-width actions share one row");
                        assert!(folder.right() <= checkbox.left(), "actions do not overlap");
                        assert!((card.height() - 62.0).abs() <= 1.0, "36 pt content plus 12 pt padding and 1 pt border on each side");
                        assert_eq!(folder.height(), 24.0);
                        assert_eq!(checkbox.size(), egui::vec2(16.0, 16.0));
                        let t = solidcraft_theme::Tokens::get();
                        let icon = crate::scenario::rect_of("prefs:plugin:public-one:icon").unwrap();
                        assert_eq!(icon.size(), egui::vec2(36.0, 36.0));
                        assert!((icon.left() - card.left() - 13.0).abs() <= 1.0, "12 pt padding inside the border");
                        for control in [icon, folder, checkbox] {
                            assert!((control.center().y - card.center().y).abs() <= 1.0, "all card content is vertically centred");
                        }
                        assert!((checkbox.right() - card.right() + 13.0).abs() <= 1.0, "right padding matches left");
                        let mut quiet = true;
                        let mut semibold_title = false;
                        for shape in &painted {
                            crate::test_support::each_shape(&shape.shape, &mut |shape| match shape {
                                egui::Shape::Rect(rect) if rect.rect == folder && rect.fill != egui::Color32::TRANSPARENT => quiet = false,
                                egui::Shape::Text(text) if text.galley.job.text == "Public one" => {
                                    semibold_title = text
                                        .galley
                                        .job
                                        .sections
                                        .iter()
                                        .all(|section| section.format.font_id == crate::ui_kit::type_face::semibold(t.font_button));
                                }
                                _ => {}
                            });
                        }
                        assert!(quiet, "Show in folder has no button background");
                        assert!(semibold_title, "the title uses the actual 13 pt semibold face");
                    }
                    assert!(enabled(&h.app, "public-one"), "layout does not change plugin state");
                }
            }
        }
        solidcraft_theme::set_dark(previous);
    }

    #[test]
    fn plugin_cards_open_their_own_folder_and_cancel_restores_enabled_state() {
        use std::{cell::RefCell, rc::Rc};
        let previous = solidcraft_theme::is_dark();
        for dark in [false, true] {
            solidcraft_theme::set_dark(dark);
            let mut h = crate::scenario::Harness::new();
            let dir = h.dir.join("public-plugins");
            write(&dir, "public-one", r#"{"id":"public-one","name":"Public one","version":"1.2.3","description":"Public fixture."}"#);
            write(&dir, "public-two", r#"{"id":"public-two","name":"Public two","version":"2","description":"Another fixture."}"#);
            let opened = Rc::new(RefCell::new(Vec::new()));
            let seen = opened.clone();
            h.app.services.show_in_folder = Some(Box::new(move |path| seen.borrow_mut().push(PathBuf::from(path))));
            crate::prefs::control(&mut h.app, &serde_json::json!({"open":"Plugins"})).unwrap();
            let ctx = crate::fonts::test_context();
            for _ in 0..3 {
                page_frame(&mut h.app, &ctx, &dir, vec![]);
            }
            assert!(opened.borrow().is_empty(), "rendering never opens a folder");
            click_page(&mut h.app, &ctx, &dir, "prefs:plugin:public-two:folder");
            assert_eq!(*opened.borrow(), [dir.join("public-two/plugin.json")]);
            click_page(&mut h.app, &ctx, &dir, "prefs:plugin:public-one");
            assert!(!enabled(&h.app, "public-one") && enabled(&h.app, "public-two"));
            crate::prefs::control(&mut h.app, &serde_json::json!({"button":"defaults"})).unwrap();
            assert!(enabled(&h.app, "public-one"));
            page_frame(&mut h.app, &ctx, &dir, vec![]);
            click_page(&mut h.app, &ctx, &dir, "prefs:plugin:public-one");
            crate::prefs::control(&mut h.app, &serde_json::json!({"button":"cancel"})).unwrap();
            assert!(enabled(&h.app, "public-one") && enabled(&h.app, "public-two"));
        }
        solidcraft_theme::set_dark(previous);
    }

    #[test]
    fn show_plugins_folder_creates_only_the_requested_fixture_root() {
        use std::{cell::RefCell, rc::Rc};
        let mut h = crate::scenario::Harness::new();
        let dir = h.dir.join("not-created-yet");
        let opened = Rc::new(RefCell::new(Vec::new()));
        let seen = opened.clone();
        h.app.services.show_in_folder = Some(Box::new(move |path| seen.borrow_mut().push(PathBuf::from(path))));
        let ctx = crate::fonts::test_context();
        for _ in 0..3 {
            page_frame(&mut h.app, &ctx, &dir, vec![]);
        }
        assert!(!dir.exists() && opened.borrow().is_empty());
        click_page(&mut h.app, &ctx, &dir, "prefs:plugins:folder");
        assert!(dir.is_dir());
        assert_eq!(*opened.borrow(), [dir.join("plugin.json")]);
        assert!(!dir.join("plugin.json").exists(), "the reveal service needs only a sentinel path");
    }

    #[test]
    fn plugin_metadata_search_owns_one_navigation_target_and_reveals_the_real_card() {
        let mut h = crate::scenario::Harness::new();
        let dir = h.dir.join("public-plugin-search");
        write(&dir, "public-one", r#"{"id":"public-one","name":"Public one","version":"1.2.3","description":"Unique description."}"#);
        let ctx = crate::fonts::test_context();
        for query in ["Public one", "public-one", "1.2.3", "Unique description"] {
            for _ in 0..3 {
                crate::scenario::clear_handles();
                let mut output = crate::test_support::frame(&ctx, egui::RawInput::default(), |ui| {
                    ui.set_width(646.0);
                    let _search = crate::ui_kit::settings_search::begin(ui, query, "prefs");
                    crate::ui_kit::settings_search::begin_page(ui, crate::prefs::PAGE_PLUGINS, "Extend > Plugins");
                    crate::ui_kit::scope(ui, |ui| page_in_dir(&mut h.app, ui, Some(&dir)));
                    crate::ui_kit::settings_search::store_nav(ui);
                });
                output.textures_delta.clear();
            }
            let (_, _, hits) = crate::ui_kit::settings_search::nav(&ctx);
            assert_eq!(hits.len(), 1, "{query}: one real plugin result");
            assert_eq!(hits[0].page, crate::prefs::PAGE_PLUGINS);
            crate::ui_kit::settings_search::set_reveal(&ctx, Some(crate::ui_kit::settings_search::reveal_for(hits[0].page, &hits[0].label, "prefs")));
            page_frame(&mut h.app, &ctx, &dir, vec![]);
            let card = crate::scenario::rect_of("prefs:plugin:public-one:card").unwrap();
            assert!(crate::scenario::handle_at("prefs:revealed").unwrap().distance(card.center()) <= 1.0);
            assert!(enabled(&h.app, "public-one"), "searching and revealing do not toggle the plugin");
        }
    }

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
        assert_eq!(app.prefs_window.shell.section, crate::prefs::PAGE_PLUGINS);
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
            assert!(h.app.prefs_window.shell.open);
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
    fn resource_drafts_survive_preferences_navigation() {
        use serde_json::json;
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"open":"Plugins"}));
        let initial = host_limits(&h.app.preferences);
        h.app.prefs_window.plugin_limit_inputs = Some(LimitInputs { applied: initial, fuel: "90000000".into(), memory: "512".into() });
        assert_eq!(h.call("ui.prefs", json!({"open":"General"}))["ok"], true);
        let draft = h.app.prefs_window.plugin_limit_inputs.as_ref().unwrap();
        assert_eq!((&draft.fuel, &draft.memory), (&"90000000".to_string(), &"512".to_string()));
        assert_eq!(h.call("ui.prefs", json!({"button":"apply"}))["ok"], true);
        assert_eq!(h.app.session.plugin_limits.max_fuel, 90000000);
        assert_eq!(h.app.session.plugin_limits.max_memory_mib, 512);
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
