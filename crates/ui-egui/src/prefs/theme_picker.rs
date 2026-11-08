//! Appearance's theme browser. Only the Day/Night assignments are saved preferences.
use super::*;
use ui_kit::{ThemeGlyph as Glyph, ThemeSegment};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Classification {
    pub dark: bool,
    pub high: bool,
}

/// Interim traits until themes declare tone and contrast in a later file-contract change.
/// Use the resolved thumbnail ground and primary ink. 20:1 reserves High contrast for
/// near-maximal text/background contrast: four shipped palettes measure 21:1, while
/// every other shipped palette is at most 16.006:1. No theme ids are special-cased.
pub(super) fn theme_traits(t: &Tokens) -> Classification {
    let linear = solidcraft_color::srgb::decode8;
    let luminance = 0.2126 * linear(t.dialog_footer.r()) + 0.7152 * linear(t.dialog_footer.g()) + 0.0722 * linear(t.dialog_footer.b());
    Classification { dark: luminance < 0.5, high: crate::theme::contrast_ratio(t.form_ink, t.dialog_footer) >= 20.0 }
}

pub(super) fn palette(p: &Prefs, id: &str) -> Tokens {
    match id {
        "light" => Tokens::light(),
        "dark" => Tokens::dark(),
        _ => crate::theme_files::find(id, &p.custom_themes).map(|f| resolved_theme(p, f)).unwrap_or_else(Tokens::dark),
    }
}

/// No recursion through `system`, even for hand-edited or removed theme ids.
pub(super) fn slot_id(p: &Prefs, night: bool) -> &str {
    let (id, fallback) = if night { (&p.theme_night, "dark") } else { (&p.theme_day, "light") };
    if matches!(id.as_str(), "light" | "dark") || crate::theme_files::find(id, &p.custom_themes).is_some() { id } else { fallback }
}

pub(super) fn active_id(p: &Prefs, system_dark: bool) -> &str {
    if p.theme == "system" { slot_id(p, system_dark) } else { &p.theme }
}

#[derive(Clone, Default, PartialEq)]
pub(super) struct State {
    pub tone: Option<bool>,
    pub high: Option<bool>,
    pub imported: bool,
    pub grid: bool,
    pub night: bool,
}

#[derive(Clone)]
pub(super) struct Entry {
    id: String,
    name: String,
    palette: Tokens,
    class: Classification,
    imported: bool,
    options: bool,
}

impl State {
    fn matches(&self, entry: &Entry, skip: &str) -> bool {
        (skip == "tone" || self.tone.is_none_or(|dark| dark == entry.class.dark))
            && (skip == "contrast" || self.high.is_none_or(|high| high == entry.class.high))
            && (skip == "imported" || !self.imported || entry.imported)
    }

    pub(super) fn count(&self, entries: &[Entry], group: &str, value: bool) -> usize {
        entries
            .iter()
            .filter(|e| {
                self.matches(e, group)
                    && match group {
                        "tone" => e.class.dark == value,
                        "contrast" => e.class.high == value,
                        _ => e.imported,
                    }
            })
            .count()
    }

    pub(super) fn aim(&mut self, night: bool) {
        self.night = night;
        self.tone = Some(night);
    }
}

pub(super) fn entries(app: &SolveApp) -> Vec<Entry> {
    theme_choices(app)
        .into_iter()
        .map(|(id, name)| {
            let palette = palette(&app.preferences, &id);
            let class = theme_traits(&palette);
            let imported = app.preferences.custom_themes.iter().any(|f| f.id == id);
            let options = crate::theme_files::find(&id, &app.preferences.custom_themes)
                .is_some_and(|f| !crate::theme_options::offered(f, crate::theme_options::PROVIDERS).is_empty());
            Entry { id, name, palette, class, imported, options }
        })
        .collect()
}

pub(super) fn switch_mode(app: &mut SolveApp, follow: bool, dark: bool) {
    if follow == (app.preferences.theme == "system") {
        return;
    }
    stop_following(app);
    if follow {
        let id = app.preferences.theme.clone();
        let night = theme_traits(&palette(&app.preferences, &id)).dark;
        fill_slot(&mut app.preferences, night, &id);
        app.preferences.theme = "system".into();
    } else {
        let id = active_id(&app.preferences, dark).to_owned();
        select_theme(app, &id);
    }
}

pub(super) fn fill_slot(p: &mut Prefs, night: bool, id: &str) {
    if night {
        p.theme_night = id.to_owned();
    } else {
        p.theme_day = id.to_owned();
    }
}

pub(super) fn choose(app: &mut SolveApp, night: bool, id: &str) {
    if app.preferences.theme == "system" {
        fill_slot(&mut app.preferences, night, id);
    } else {
        stop_following(app);
        select_theme(app, id);
    }
}

pub(super) fn row(app: &mut SolveApp, ui: &mut egui::Ui) {
    if crate::ui_kit::settings_search::suppress_chrome(ui) {
        row_contents(app, ui);
        return;
    }
    // The page scroller's host can be wider than its clipped content well. Give the
    // picker its real visible width, and never propagate a child's horizontal growth.
    let start = ui.next_widget_position();
    let width = ui.available_width().min((ui.clip_rect().right() - start.x).max(0.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("theme_picker_content")
            .max_rect(egui::Rect::from_min_size(start, egui::vec2(width, ui.available_height())))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.set_width(width);
    row_contents(app, &mut child);
    ui.allocate_exact_size(egui::vec2(width, child.min_size().y), egui::Sense::hover());
}

fn row_contents(app: &mut SolveApp, ui: &mut egui::Ui) {
    let key = ui.make_persistent_id("theme_picker_state");
    let mut state = ui.data(|d| d.get_temp::<State>(key)).unwrap_or_default();
    let search = crate::ui_kit::settings_search::suppress_chrome(ui);
    let mut action = None;
    let mut action_theme = None;
    let t = Tokens::get();
    let dark = system_dark(app, ui.ctx()).unwrap_or_else(|| app.prefs_window.system_theme_dark.unwrap_or(app.ui.dark));
    let label = app.tr("prefs.picker.import");
    let import_width = ui_kit::ContentWidth::name(ui, label) + t.space_14 + t.space_6 + t.space_10 * 2.0;
    let (mode, import_rect) = ui_kit::theme_toolbar(
        ui,
        "mode",
        import_width,
        t.space_24 * 19.0 + t.space_4,
        |ui| {
            let mode = ui_kit::theme_segments(
                ui,
                "mode",
                &[
                    ThemeSegment::new(app.tr("prefs.picker.one"), Glyph::Monitor, None),
                    ThemeSegment::new(app.tr("prefs.picker.follow"), Glyph::Contrast, None),
                ],
                Some(usize::from(app.preferences.theme == "system")),
                false,
            );
            if app.preferences.theme == "system" {
                ui_kit::settings_inline_note(ui, app.tr("prefs.picker.hint"));
            }
            mode
        },
        |ui| {
            let button = ui_kit::theme_import_button(ui, label).backing_on_hover_text(app.tr("prefs.theme_import_hint"));
            crate::scenario::publish_target("prefs:theme_import", button.rect);
            if button.clicked() {
                action = Some("import");
            }
            #[cfg(not(target_arch = "wasm32"))]
            ui_kit::Menu::context(&button, |ui| {
                ui_kit::Menu::show(ui, t.form_card_width, |ui| {
                    if ui_kit::MenuItem::new("theme_paste", app.tr("prefs.theme_paste"), "paste").show(ui).clicked() {
                        action = Some("paste");
                        ui.close();
                    }
                });
            });
            button.rect
        },
    );
    app.prefs_window.import_tile = Some(import_rect);
    if let Some(index) = mode {
        switch_mode(app, index == 1, dark);
        state.tone = if index == 1 { Some(state.night) } else { None };
    }
    let system = app.preferences.theme == "system";
    if system {
        let width = (ui.available_width() - t.space_10) / 2.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = t.space_10;
            for night in [false, true] {
                let id = slot_id(&app.preferences, night);
                let name = theme_choices(app).into_iter().find(|(i, _)| i == id).map(|(_, n)| n).unwrap_or_default();
                let title = app.tr(if night { "prefs.picker.night" } else { "prefs.picker.day" });
                let aimed = state.night == night;
                let response = ui_kit::theme_slot(
                    ui,
                    &ui_kit::ThemeSlot {
                        title,
                        name: &name,
                        aimed,
                        night,
                        width,
                        hint: app.tr(if aimed { "prefs.picker.pick" } else { "prefs.picker.change" }),
                        sample: ui_kit::ThemeSample::from_tokens(&palette(&app.preferences, id)),
                    },
                );
                crate::scenario::publish_target(if night { "prefs:theme_slot:night" } else { "prefs:theme_slot:day" }, response.rect);
                if response.clicked() {
                    state.aim(night);
                }
            }
        });
    }
    let entries = entries(app);
    let (filters, view) = ui_kit::theme_toolbar(
        ui,
        "filters",
        (t.space_24 + t.space_10) * 2.0,
        0.0,
        |ui| {
            let mut tone = state.tone;
            let mut high = state.high;
            let mut imported = state.imported;
            if let Some(index) = ui_kit::theme_segments(
                ui,
                "tone",
                &[
                    ThemeSegment::new(app.tr("ui.light"), Glyph::Sun, Some(state.count(&entries, "tone", false))),
                    ThemeSegment::new(app.tr("ui.dark"), Glyph::Moon, Some(state.count(&entries, "tone", true))),
                ],
                state.tone.map(usize::from),
                false,
            ) {
                let value = Some(index == 1);
                tone = if state.tone == value { None } else { value };
            }
            if let Some(index) = ui_kit::theme_segments(
                ui,
                "contrast",
                &[
                    ThemeSegment::new(app.tr("prefs.picker.standard"), Glyph::None, Some(state.count(&entries, "contrast", false))),
                    ThemeSegment::new(app.tr("prefs.picker.high"), Glyph::Contrast, Some(state.count(&entries, "contrast", true))),
                ],
                state.high.map(usize::from),
                false,
            ) {
                let value = Some(index == 1);
                high = if state.high == value { None } else { value };
            }
            if ui_kit::theme_segments(
                ui,
                "imported",
                &[ThemeSegment::new(app.tr("prefs.picker.imported"), Glyph::None, Some(state.count(&entries, "imported", true)))],
                state.imported.then_some(0),
                false,
            )
            .is_some()
            {
                imported = !state.imported;
            }
            (tone, high, imported)
        },
        |ui| {
            ui_kit::theme_segments(
                ui,
                "view",
                &[
                    ThemeSegment::new(app.tr("prefs.picker.carousel"), Glyph::Carousel, None),
                    ThemeSegment::new(app.tr("prefs.picker.grid"), Glyph::Grid, None),
                ],
                Some(usize::from(state.grid)),
                true,
            )
        },
    );
    (state.tone, state.high, state.imported) = filters;
    if let Some(index) = view {
        state.grid = index == 1;
    }
    let visible: Vec<_> = entries.iter().filter(|e| search || state.matches(e, "")).collect();
    let following = !app.prefs_window.follow.path(&app.preferences.theme_follow).is_empty();
    let reveal = if system { slot_id(&app.preferences, state.night) } else { &app.preferences.theme };
    let selected_index = visible.iter().position(|e| e.id == reveal);
    let identity = format!("{reveal}:{:?}:{:?}:{}:{}", state.tone, state.high, state.imported, state.grid);
    let reveal_key = ui.make_persistent_id("theme_picker_reveal");
    let changed = ui.data(|d| d.get_temp::<String>(reveal_key)).as_ref() != Some(&identity);
    let mut chosen = None;
    let mut card = |ui: &mut egui::Ui, index: usize, width: f32| {
        let Some(e) = visible.get(index) else { return };
        let source = app.tr(if e.imported { "prefs.imported_file" } else { "prefs.built_in" });
        let note = if e.class.high { solidcraft_ui_text::format("prefs.picker.high_note", &[("source", source)]) } else { source.to_owned() };
        let day = system && slot_id(&app.preferences, false) == e.id;
        let night = system && slot_id(&app.preferences, true) == e.id;
        let selected = !following && if system { day || night } else { app.preferences.theme == e.id };
        let response = ui_kit::theme_card(
            ui,
            &ui_kit::ThemeCard {
                name: &e.name,
                note: &note,
                selected,
                day,
                night,
                dark: e.class.dark,
                high: e.class.high,
                options: e.options.then(|| app.tr("prefs.theme_options_badge")),
                sample: ui_kit::ThemeSample::from_tokens(&e.palette),
            },
            width,
        );
        let clipped = response.rect.intersect(ui.clip_rect());
        if clipped.is_positive() {
            crate::scenario::publish_target(&format!("prefs:theme_tile:{}", e.id), clipped);
            crate::scenario::publish_target(&format!("prefs:theme:{}", e.id), clipped);
            if e.options {
                crate::scenario::publish_rect(&format!("prefs:theme_options_badge:{}", e.id), clipped);
            }
            if selected && !system {
                crate::scenario::publish_target("prefs:theme", clipped);
            }
        }
        if response.clicked() {
            chosen = Some(e.id.clone());
        }
        ui_kit::Menu::context(&response, |ui| {
            ui_kit::Menu::show(ui, t.form_card_width, |ui| {
                if ui_kit::MenuItem::new("theme_export", app.tr_label("Export…"), "export").show(ui).clicked() {
                    action = Some("export");
                    action_theme = Some(e.id.clone());
                    ui.close();
                }
                if e.imported {
                    let mut remove = ui_kit::MenuItem::new("theme_remove", app.tr_label("Remove"), "delete");
                    remove.danger = true;
                    if remove.show(ui).clicked() {
                        action = Some("remove");
                        action_theme = Some(e.id.clone());
                        ui.close();
                    }
                }
            });
        });
    };
    if search {
        for index in 0..visible.len() {
            card(ui, index, t.space_24 * 5.0);
        }
    } else if visible.is_empty() {
        ui_kit::theme_empty(ui, app.tr("prefs.picker.empty"), app.tr("prefs.picker.clear"), || {
            state.tone = None;
            state.high = None;
            state.imported = false;
        });
    } else {
        ui_kit::theme_browser(ui, "appearance_theme_picker", visible.len(), state.grid, changed.then_some(selected_index).flatten(), &mut card);
    }
    ui.data_mut(|d| {
        d.insert_temp(key, state.clone());
        d.insert_temp(reveal_key, identity);
    });
    if !search {
        app.prefs_window.theme_picker_night = state.night;
    }
    if let Some(id) = chosen {
        choose(app, state.night, &id);
    }
    super::theme_action(app, ui, action, action_theme);
}
