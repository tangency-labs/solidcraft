//! Preferences › Plugins: packages installed under the per-user data folder.
//!
//! A plugin is a directory of `{data}/plugins/<id>/plugin.json`. The page searches that list.
//! It reads `plugin.json` and records which ids are turned off. It does not load or run anything
//! from the folder. Running a plugin (the wasm host in docs/plugins.md) is a later step; it will
//! honour `disabled_plugins`.

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
    let manifest: Manifest = serde_json::from_str(&text).map_err(|e| format!("plugin.json is not a plugin manifest ({e})"))?;
    check(manifest, folder)
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
        crate::ui_kit::settings_heading(ui, crate::i18n::tr("ui.plugins"), crate::i18n::tr("prefs.plugins_intro"));
        let Some(dir) = dir else {
            ui.label(crate::i18n::tr("ui.plugins_desktop"));
            return;
        };
        let t = crate::theme::Tokens::get();
        let path = dir.display().to_string();
        let searching = crate::prefs_search::querying(ui);
        // The page's own box is for browsing Plugins. The dialog search is the filter while it is active.
        if !searching {
            crate::ui_kit::search_field(ui, ui.id().with("plugins_search"), &mut app.prefs_window.plugin_query, crate::i18n::tr("ui.plugins_search"));
            ui.add_space(8.0);
        }
        let (plugins, skipped) = scan(dir);
        let query = app.prefs_window.plugin_query.clone();
        crate::ui_kit::settings_section_count(
            ui,
            "plugins_installed",
            crate::i18n::tr("prefs.plugins_installed"),
            Some(&plugins.len().to_string()),
            |ui| {
                if plugins.is_empty() && (!searching || crate::prefs_search::shows_all(ui)) {
                    crate::ui_kit::text(ui, crate::i18n::tr("ui.plugins_empty"), crate::ui_kit::Text::Hint("plugins_empty"));
                }
                let mut shown = 0usize;
                for plugin in plugins.iter().filter(|plugin| searching || matches_query(plugin, &query)) {
                    if searching
                        && !crate::prefs_search::shows_all(ui)
                        && !crate::prefs_search::texts_hit(ui, &[&plugin.name, &plugin.id, &plugin.description, &plugin.version])
                    {
                        continue;
                    }
                    shown += 1;
                    if searching {
                        crate::prefs_search::reveal(ui);
                        crate::prefs_search::push_force(ui);
                    }
                    plugin_card(app, ui, dir, plugin);
                    if searching {
                        crate::prefs_search::pop_force(ui);
                    }
                }
                if !searching && !plugins.is_empty() && shown == 0 {
                    ui.label(crate::i18n::tr("ui.plugins_none"));
                }
            },
        );
        if !searching && query.trim().is_empty() && !skipped.is_empty() {
            ui.add_space(6.0);
            ui.label(
                RichText::new(crate::i18n::format("ui.plugins_skipped", &[("count", &skipped.len().to_string())]))
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
                let button = crate::ui_kit::button(ui, crate::i18n::tr("prefs.plugins_show_folder"), crate::ui_kit::Button::Normal);
                crate::scenario::publish_target("prefs:plugins:folder", button.rect);
                if button.clicked() {
                    if let Err(e) = std::fs::create_dir_all(dir) {
                        app.set_status(format!("Could not create the plugins folder: {e}"), true);
                    } else if let Some(show) = &app.services.show_in_folder {
                        // The service opens the parent of the supplied file, even if it is absent.
                        show(&dir.join("plugin.json").display().to_string());
                    }
                }
                crate::ui_kit::help_icon(ui, &crate::i18n::format("ui.plugins_note", &[("path", &path)]));
            });
        }
    });
}

fn plugin_card(app: &mut SolveApp, ui: &mut egui::Ui, dir: &Path, plugin: &Plugin) {
    use crate::ui_kit;
    let t = crate::theme::Tokens::get();
    let response = ui
        .push_id(&plugin.id, |ui| {
            ui_kit::settings_card(ui, |ui| {
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    let action_width =
                        (ui_kit::ContentWidth::button(ui, crate::i18n::tr_label("Show in folder")) + t.control_height + t.space_8).ceil();
                    let beside = ui.available_width() >= action_width + t.form_label_width * 2.0 + t.space_14;
                    let text_width = if beside { (ui.available_width() - action_width - t.space_14).floor() } else { ui.available_width() };
                    let text = |ui: &mut egui::Ui| {
                        ui.allocate_ui_with_layout(egui::vec2(text_width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.set_width(text_width);
                            ui.spacing_mut().item_spacing.y = t.space_3;
                            ui.horizontal_wrapped(|ui| {
                                ui.add(egui::Label::new(RichText::new(&plugin.name).strong().size(t.font_label).color(t.form_ink)).wrap());
                                ui.label(RichText::new(&plugin.version).size(t.font_small).color(t.form_dim));
                            });
                            if !plugin.description.is_empty() {
                                ui.add(egui::Label::new(RichText::new(&plugin.description).size(t.font_small).color(t.form_dim)).wrap());
                            }
                        });
                    };
                    let mut actions = |ui: &mut egui::Ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(action_width.min(ui.available_width()), t.control_height),
                            egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true),
                            |ui| {
                                ui.spacing_mut().item_spacing.x = t.space_8;
                                let button = ui_kit::button(ui, crate::i18n::tr_label("Show in folder"), ui_kit::Button::Small);
                                crate::scenario::publish_target(&format!("prefs:plugin:{}:folder", plugin.id), button.rect);
                                if button.clicked()
                                    && let Some(show) = &app.services.show_in_folder
                                {
                                    show(&dir.join(&plugin.id).join("plugin.json").display().to_string());
                                }
                                let mut on = enabled(app, &plugin.id);
                                let check = ui_kit::checkbox(ui, "", &mut on).on_hover_text(crate::i18n::tr("prefs.plugins_enabled"));
                                check.widget_info(|| {
                                    egui::WidgetInfo::selected(
                                        egui::WidgetType::Checkbox,
                                        ui.is_enabled(),
                                        on,
                                        format!("{}: {}", plugin.name, crate::i18n::tr("prefs.plugins_enabled")),
                                    )
                                });
                                crate::scenario::publish_target(&format!("prefs:plugin:{}", plugin.id), check.rect);
                                if check.changed() {
                                    if on {
                                        app.preferences.disabled_plugins.remove(&plugin.id);
                                    } else if app.preferences.disabled_plugins.len() < MAX_DISABLED {
                                        app.preferences.disabled_plugins.insert(plugin.id.clone());
                                    }
                                }
                            },
                        );
                    };
                    if beside {
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = t.space_14;
                            text(ui);
                            actions(ui);
                        });
                    } else {
                        text(ui);
                        actions(ui);
                    }
                });
            })
        })
        .inner
        .response;
    crate::scenario::publish_rect(&format!("prefs:plugin:{}:card", plugin.id), response.rect);
    let texts = [&plugin.name, &plugin.id, &plugin.description, &plugin.version];
    if let Some(text) = texts.iter().find(|text| crate::prefs_search::texts_hit(ui, &[*text])) {
        crate::prefs_search::remember(ui, text, response.rect);
    }
    for text in texts {
        crate::prefs_search::follow(ui, text, response.rect);
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
        let previous = crate::theme::is_dark();
        for dark in [false, true] {
            crate::theme::set_dark(dark);
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
            let t = crate::theme::Tokens::get();
            assert!(heading.height() >= t.space_24 && heading.bottom() < section.top());
            assert!((section.left() - heading.left()).abs() <= 1.0 && (section.right() - heading.right()).abs() <= 1.0);
            assert!((section.height() - (t.control_height + t.space_2)).abs() <= 1.0);
            assert!((body.top() - section.bottom() - t.space_10).abs() <= 1.0);
            assert!(!dir.exists(), "production layout does not create the plugin folder");
        }
        crate::theme::set_dark(previous);
    }

    #[test]
    fn plugin_card_actions_share_a_row_at_normal_width_and_stay_inside_narrow_cards() {
        let previous = crate::theme::is_dark();
        for dark in [false, true] {
            crate::theme::set_dark(dark);
            for language in ["en", "de"] {
                let _language = crate::i18n::scope(language);
                for width in [646.0, 200.0] {
                    let mut h = crate::scenario::Harness::new();
                    let dir = h.dir.join("plugin-action-layout");
                    write(&dir, "public-one", r#"{"id":"public-one","name":"Public one","version":"1.2.3","description":"Public fixture."}"#);
                    let ctx = crate::fonts::test_context();
                    let mut clip = egui::Rect::NOTHING;
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
                    }
                    assert!(enabled(&h.app, "public-one"), "layout does not change plugin state");
                }
            }
        }
        crate::theme::set_dark(previous);
    }

    #[test]
    fn plugin_cards_open_their_own_folder_and_cancel_restores_enabled_state() {
        use std::{cell::RefCell, rc::Rc};
        let previous = crate::theme::is_dark();
        for dark in [false, true] {
            crate::theme::set_dark(dark);
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
        crate::theme::set_dark(previous);
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
                    let _search = crate::prefs_search::begin(ui, query);
                    crate::prefs_search::begin_page(ui, crate::prefs::PAGE_PLUGINS, "Extend > Plugins");
                    crate::ui_kit::scope(ui, |ui| page_in_dir(&mut h.app, ui, Some(&dir)));
                    crate::prefs_search::store_nav(ui);
                });
                output.textures_delta.clear();
            }
            let (_, _, hits) = crate::prefs_search::nav(&ctx);
            assert_eq!(hits.len(), 1, "{query}: one real plugin result");
            assert_eq!(hits[0].page, crate::prefs::PAGE_PLUGINS);
            crate::prefs_search::set_reveal(&ctx, Some(crate::prefs_search::reveal_for(hits[0].page, &hits[0].label)));
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
        assert_eq!(app.prefs_window.section, crate::prefs::PAGE_PLUGINS);
        assert!(enabled(&app, "ftc-frc-parts"));
        assert!(crate::prefs::control(&mut app, &json!({"open": "NoSuch"})).is_err());
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
