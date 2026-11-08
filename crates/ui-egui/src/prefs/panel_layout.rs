//! Registered panel arrangements, edited live under the Preferences snapshot.
use egui::{Align2, FontId, Id, Rect, Response, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use serde_json::json;
use solidcraft_engine::panels::{Layout, SectionLocation, Zone, commands};

use crate::panel_runtime::{LayoutPreferenceEntry, LayoutPreferencesView};
use crate::ui_kit::{self, BackingTooltipExt as _, Container, Surface};
use crate::{SolveApp, theme::Tokens};
use std::sync::Arc;

const STATE: &str = "sc_preferences_panel_layout";
const TILE_WIDTH: f32 = 96.0;
const THUMB_HEIGHT: f32 = 62.0;
const TILE_HEIGHT: f32 = 81.0;

#[derive(Clone, Default)]
struct State {
    focused: usize,
    workspace: String,
    source_name: Option<String>,
    popup: Option<Popover>,
    escape: bool,
    view: Option<(crate::panel_runtime::PanelLayoutSnapshot, Arc<LayoutPreferencesView>)>,
    list_focus: Option<Id>,
    focus_name: Option<String>,
}

#[derive(Clone)]
struct Popover {
    origin: Id,
    kind: PopoverKind,
    first: bool,
}

#[derive(Clone)]
enum PopoverKind {
    Save { name: String, chosen: usize },
    Replace { name: String },
}

/// Reserve the first Escape for our popover, before the Preferences footer.
pub(super) fn reserve_escape(ctx: &egui::Context) {
    let key = Id::unique(STATE);
    let origin = ctx.data_mut(|data| data.get_temp_mut_or_default::<State>(key).popup.as_ref().map(|popup| popup.origin));
    let Some(origin) = origin else { return };
    if !egui::Popup::is_id_open(ctx, origin.with("popup")) || !ctx.read_response(origin).is_some_and(|owner| owner.enabled()) {
        closed(ctx);
        return;
    }
    if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        ctx.data_mut(|data| data.get_temp_mut_or_default::<State>(key).escape = true);
    }
}

pub(super) fn closed(ctx: &egui::Context) {
    let key = Id::unique(STATE);
    let origin = ctx.data_mut(|data| {
        let state = data.get_temp_mut_or_default::<State>(key);
        state.escape = false;
        state.popup.take().map(|popup| popup.origin)
    });
    if let Some(origin) = origin {
        egui::Popup::close_id(ctx, origin.with("popup"));
    }
}

pub(super) fn show(app: &mut SolveApp, ui: &mut Ui) {
    if !crate::panel_runtime::preferences_enabled(app) {
        return;
    }
    let key = Id::unique(STATE);
    let mut state = ui.ctx().data(|data| data.get_temp::<State>(key)).unwrap_or_default();
    if !state.view.as_ref().is_some_and(|(snapshot, _)| crate::panel_runtime::panel_layout_matches(app, snapshot)) {
        let Ok(view) = crate::panel_runtime::layout_preferences_view(app) else { return };
        state.view = Some((crate::panel_runtime::panel_layout_snapshot(app), Arc::new(view)));
    }
    let Some(view) = state.view.as_ref().map(|(_, view)| view.clone()) else { return };
    if state.workspace != view.selected_workspace.0 {
        state.workspace.clone_from(&view.selected_workspace.0);
        state.focused = 0;
        state.source_name = None;
        state.popup = None;
    }
    if !state.source_name.as_ref().is_some_and(|name| view.entries.iter().any(|entry| entry.matches && &entry.name == name))
        && let Some(entry) = view.entries.iter().find(|entry| entry.selected)
    {
        state.source_name = Some(entry.name.clone());
    }
    ui_kit::settings_section(ui, "panel_layout", app.tr("ui.panel_layout.panel_layout"), |ui| {
        ui.spacing_mut().item_spacing.y = Tokens::get().space_16;
        workspace_selector(app, ui, &view);
        tiles(app, ui, &view, &mut state);
    });
    if ui.is_visible() {
        popover(app, ui, &view, &mut state);
        ui.ctx().data_mut(|data| data.insert_temp(key, state));
    }
}

fn workspace_selector(app: &mut SolveApp, ui: &mut Ui, view: &LayoutPreferencesView) {
    let t = Tokens::get();
    let mut workspaces: Vec<_> = view.workspaces.iter().collect();
    workspaces.sort_by_key(|workspace| {
        ["modelling", "sketch", "render", "review", "classic"].iter().position(|suffix| workspace.title_key.ends_with(*suffix)).unwrap_or(5)
    });
    let labels: Vec<_> = workspaces.iter().map(|workspace| app.tr(&workspace.title_key).to_owned()).collect();
    let options: Vec<_> = labels.iter().map(String::as_str).collect();
    let current = workspaces.iter().position(|workspace| workspace.id == view.selected_workspace).unwrap_or(0);
    let mut selected = current;
    let mut reset = false;
    let title = app.tr("ui.panel_layout.layout_workspace").to_owned();
    ui_kit::settings_form(ui, 0.0, |ui| {
        ui_kit::sized_row(ui, &title, t.control_height, |ui| {
            crate::ui_kit::settings_search::note_choices(ui, &options);
            ui.horizontal(|ui| {
                let reset_label = app.tr("ui.panel_layout.reset_short");
                let reset_width = ui_kit::ContentWidth::name(ui, reset_label) + t.space_24 + t.space_16;
                let width = (ui.available_width() - reset_width - t.space_12).max(t.control_height);
                ui.allocate_ui_with_layout(vec2(width, t.control_height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.set_min_width(width);
                    let need: f32 = options.iter().map(|name| ui_kit::ContentWidth::name(ui, name) + t.space_16).sum();
                    if need <= width {
                        let ids: Vec<_> = (0..options.len()).map(|index| ui.make_persistent_id(("segment", index))).collect();
                        let mut focus = None;
                        if let Some(at) = ui.memory(|memory| memory.focused()).and_then(|id| ids.iter().position(|item| *item == id)) {
                            ui.memory_mut(|memory| {
                                memory.set_focus_lock_filter(
                                    ids[at],
                                    egui::EventFilter { horizontal_arrows: true, vertical_arrows: true, ..Default::default() },
                                )
                            });
                            let delta = ui.input_mut(|input| {
                                if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft)
                                    || input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)
                                {
                                    -1
                                } else if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight)
                                    || input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)
                                {
                                    1
                                } else {
                                    0
                                }
                            });
                            if delta != 0 {
                                selected = at.saturating_add_signed(delta).min(options.len().saturating_sub(1));
                                focus = Some(selected);
                            }
                        }
                        ui_kit::segmented(ui, &mut selected, &options, ui_kit::Segments { intrinsic: true, focus, ..Default::default() });
                        for id in ids {
                            if ui.ctx().read_response(id).is_some_and(|response| response.clicked()) {
                                ui.memory_mut(|memory| memory.request_focus(id));
                            }
                            ui.ctx().accesskit_node_builder(id, |node| node.set_role(egui::Role::RadioButton));
                        }
                    } else {
                        ui_kit::dropdown(ui, "panel_layout_workspace", &mut selected, &options);
                    }
                });
                let response = reset_button(ui, reset_label, !view.default_matches);
                let workspace = labels.get(current).map(String::as_str).unwrap_or_default();
                let hint = crate::i18n::format("ui.panel_layout.reset_hint", &[("workspace", workspace)]);
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, response.enabled(), &hint));
                crate::scenario::publish_target("prefs:panel_layout:reset", response.rect);
                reset = response.backing_on_hover_text(hint).clicked();
            });
        })
    });
    if selected != current {
        if let Some(workspace) = workspaces.get(selected) {
            run(app, "layout.workspace.select", json!({"workspace":workspace.id}), None);
        }
    } else if reset {
        run(app, "layout.reset", json!({}), None);
    }
}

fn reset_button(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    let t = Tokens::get();
    let width = ui_kit::ContentWidth::name(ui, label) + t.space_24 + t.space_16;
    ui.add_enabled(enabled, |ui: &mut Ui| {
        let (rect, response) = ui.allocate_exact_size(vec2(width, t.control_height), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(rect, t.control_radius, t.hover);
        }
        let ink = if enabled { t.form_ink } else { t.form_dim };
        crate::icons::paint_ink(
            ui.painter(),
            Rect::from_center_size(pos2(rect.left() + t.space_12, rect.center().y), vec2(t.space_14, t.space_14)),
            "undo",
            ink,
        );
        ui.painter().text(pos2(rect.left() + t.space_24, rect.center().y), Align2::LEFT_CENTER, label, FontId::proportional(t.font_label), ink);
        if response.has_focus() {
            ui.painter().rect_stroke(rect, t.control_radius, Stroke::new(1.0, t.form_primary), egui::StrokeKind::Inside);
        }
        response
    })
}

#[derive(Clone, Copy)]
enum Tile<'a> {
    Custom,
    Saved(&'a LayoutPreferenceEntry),
    Save,
}

fn tiles(app: &mut SolveApp, ui: &mut Ui, view: &LayoutPreferencesView, state: &mut State) {
    let t = Tokens::get();
    let custom = !view.entries.iter().any(|entry| entry.matches);
    let mut items = Vec::with_capacity(view.entries.len() + 2);
    if custom {
        items.push(Tile::Custom);
    }
    items.extend(view.entries.iter().map(Tile::Saved));
    items.push(Tile::Save);
    let mut reveal_focus = state.focus_name.is_some();
    if let Some(name) = state.focus_name.take()
        && let Some(index) = items.iter().position(|item| matches!(item, Tile::Saved(entry) if entry.name == name))
    {
        state.focused = index;
    }
    state.focused = state.focused.min(items.len().saturating_sub(1));
    let mut columns = ((ui.available_width() + t.space_10) / (TILE_WIDTH + t.space_10)).floor().max(1.0) as usize;
    if items.len() > columns * 3 {
        columns = ((ui.available_width() - ui.spacing().scroll.allocated_width() + t.space_10) / (TILE_WIDTH + t.space_10)).floor().max(1.0) as usize;
    }
    let rows = items.len().div_ceil(columns);
    let height = (rows.min(3) as f32 * (TILE_HEIGHT + t.space_12) - t.space_12).max(TILE_HEIGHT);
    let label = app.tr("ui.panel_layout.panel_layout").to_owned();
    ui_kit::settings_form(ui, 0.0, |ui| {
        ui_kit::sized_row(ui, &label, height, |ui| {
            let names: Vec<_> = items
                .iter()
                .map(|item| match item {
                    Tile::Custom => app.tr("ui.panel_layout.custom"),
                    Tile::Saved(entry) => entry.name.as_str(),
                    Tile::Save => app.tr("ui.panel_layout.save_as_tile"),
                })
                .collect();
            crate::ui_kit::settings_search::note_choices(ui, &names);
            crate::ui_kit::settings_search::note_choices(
                ui,
                &[app.tr("ui.panel_layout.save"), app.tr("ui.panel_layout.replace"), app.tr("ui.panel_layout.reset")],
            );
            let list_id = ui.make_persistent_id("panel_layout_list");
            state.list_focus = Some(list_id);
            let bounds = Rect::from_min_size(ui.next_widget_position(), vec2(ui.available_width(), height));
            let list = ui.interact(bounds, list_id, Sense::focusable_noninteractive());
            list.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ListBox, ui.is_enabled(), &label));
            let mut activate = false;
            let mut menu = false;
            let previous_focus = state.focused;
            if list.has_focus() && state.popup.is_none() {
                ui.memory_mut(|memory| {
                    memory.set_focus_lock_filter(list_id, egui::EventFilter { horizontal_arrows: true, vertical_arrows: true, ..Default::default() })
                });
                ui.input_mut(|input| {
                    for (key, delta) in [
                        (egui::Key::ArrowLeft, -1),
                        (egui::Key::ArrowRight, 1),
                        (egui::Key::ArrowUp, -(columns as isize)),
                        (egui::Key::ArrowDown, columns as isize),
                    ] {
                        if input.consume_key(egui::Modifiers::NONE, key) {
                            state.focused = state.focused.saturating_add_signed(delta).min(items.len().saturating_sub(1));
                        }
                    }
                    if input.consume_key(egui::Modifiers::NONE, egui::Key::Home) {
                        state.focused = 0;
                    }
                    if input.consume_key(egui::Modifiers::NONE, egui::Key::End) {
                        state.focused = items.len().saturating_sub(1);
                    }
                    activate =
                        input.consume_key(egui::Modifiers::NONE, egui::Key::Enter) || input.consume_key(egui::Modifiers::NONE, egui::Key::Space);
                    menu = input.consume_key(egui::Modifiers::SHIFT, egui::Key::F10) || input.consume_key(egui::Modifiers::NONE, egui::Key::Menu);
                });
            }
            reveal_focus |= state.focused != previous_focus || list.gained_focus();
            ui.scope_builder(UiBuilder::new().layout(egui::Layout::top_down(egui::Align::Min)).accessibility_parent(list_id), |ui| {
                egui::ScrollArea::vertical().id_salt("panel_layout_tiles").max_height(height).auto_shrink([false, true]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(t.space_10, t.space_12);
                    for (row, chunk) in items.chunks(columns).enumerate() {
                        ui.horizontal_top(|ui| {
                            for (column, item) in chunk.iter().enumerate() {
                                let index = row * columns + column;
                                let response = tile(app, ui, *item, view, state, list.has_focus() && state.focused == index);
                                if matches!(item, Tile::Saved(entry) if entry.user)
                                    && index
                                        .checked_sub(1)
                                        .and_then(|at| items.get(at))
                                        .is_some_and(|item| matches!(item, Tile::Saved(entry) if !entry.user))
                                {
                                    if column == 0 {
                                        ui.painter().hline(
                                            response.rect.x_range(),
                                            response.rect.top() - t.space_6,
                                            Stroke::new(1.0, t.control_border),
                                        );
                                    } else {
                                        ui.painter().vline(
                                            response.rect.left() - t.space_10 / 2.0,
                                            response.rect.y_range(),
                                            Stroke::new(1.0, t.control_border),
                                        );
                                    }
                                }
                                if list.has_focus() && state.focused == index {
                                    ui.ctx().accesskit_node_builder(list_id, |node| node.set_active_descendant(response.id.accesskit_id()));
                                    if reveal_focus {
                                        response.scroll_to_me(Some(egui::Align::Center));
                                    }
                                }
                                let clicked = response.clicked() || (activate && state.focused == index);
                                if response.clicked() {
                                    state.focused = index;
                                    list.request_focus();
                                }
                                if clicked {
                                    match item {
                                        Tile::Saved(entry) => {
                                            if run(app, "layout.apply", json!({"name":entry.name}), Some((&entry.name, view))) {
                                                state.source_name = Some(entry.name.clone());
                                            }
                                        }
                                        Tile::Save => open_save(ui.ctx(), state, &response, view),
                                        Tile::Custom => {}
                                    }
                                }
                                if let Tile::Saved(entry) = item
                                    && entry.user
                                {
                                    tile_menu(app, ui, state, &response, entry, menu && state.focused == index);
                                }
                            }
                        });
                    }
                });
            });
        })
    });
}

fn tile(app: &SolveApp, ui: &mut Ui, item: Tile<'_>, view: &LayoutPreferencesView, state: &State, focused: bool) -> Response {
    let t = Tokens::get();
    let (rect, response) = ui.allocate_exact_size(vec2(TILE_WIDTH, TILE_HEIGHT), Sense::CLICK);
    let thumb = Rect::from_min_size(rect.min, vec2(TILE_WIDTH, THUMB_HEIGHT));
    let (name, selected, fill, ink) = match item {
        Tile::Custom => (app.tr("ui.panel_layout.custom"), true, t.pref_note_fill, t.form_ink),
        Tile::Saved(entry) => {
            let selected = entry.matches && state.source_name.as_ref().map_or(entry.selected, |name| name == &entry.name);
            (entry.name.as_str(), selected, if selected { t.selected_bg } else { t.pref_well }, t.form_ink)
        }
        Tile::Save => (app.tr("ui.panel_layout.save_as_tile"), false, t.pref_well, t.form_primary),
    };
    let custom = matches!(item, Tile::Custom);
    let saved = matches!(item, Tile::Saved(_));
    let stroke = Stroke::new(
        if selected {
            2.0
        } else if matches!(item, Tile::Save) {
            1.5
        } else {
            1.0
        },
        if custom {
            t.pref_note
        } else if selected {
            t.form_primary
        } else if response.hovered() {
            t.form_dim
        } else {
            t.control_border
        },
    );
    ui.painter().rect_filled(thumb, 7, fill);
    if saved {
        ui.painter().rect_stroke(thumb, 7, stroke, egui::StrokeKind::Inside);
    } else {
        dashed_border(ui.painter(), thumb.shrink(1.0), stroke);
    }
    match item {
        Tile::Saved(entry) => thumbnail(ui.painter(), thumb.shrink(t.space_6), &entry.layout),
        Tile::Custom => thumbnail(ui.painter(), thumb.shrink(t.space_6), &view.current_layout),
        Tile::Save => {
            crate::icons::paint_ink(ui.painter(), Rect::from_center_size(thumb.center(), vec2(t.space_24, t.space_24)), "plus", t.form_primary)
        }
    }
    if focused {
        ui.painter().rect_stroke(thumb.expand(2.0), 8, Stroke::new(2.0, t.form_primary), egui::StrokeKind::Outside);
    }
    let mut text_rect = Rect::from_min_max(thumb.left_bottom() + vec2(0.0, 5.0), rect.right_bottom());
    if custom {
        ui.painter().circle_filled(pos2(text_rect.left() + 3.5, text_rect.center().y), 3.5, t.pref_note);
        text_rect.min.x += t.space_12;
    }
    let font = if selected || matches!(item, Tile::Save) { crate::fonts::semibold(t.font_body) } else { FontId::proportional(t.font_body) };
    let mut job = egui::text::LayoutJob::single_section(name.to_owned(), egui::TextFormat::simple(font, ink));
    job.wrap.max_width = text_rect.width();
    job.wrap.max_rows = 1;
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    ui.painter().galley(text_rect.min, galley, ink);
    let kind = if matches!(item, Tile::Saved(entry) if entry.user) { app.tr("ui.panel_layout.your_layout") } else { "" };
    let hint = if custom {
        let source = state.source_name.as_deref().unwrap_or(app.tr("ui.panel_layout.current"));
        crate::i18n::format("ui.panel_layout.custom_hint", &[("name", source)])
    } else {
        name.to_owned()
    };
    let accessible = [name, kind, if custom { &hint } else { "" }, if selected { app.tr("ui.panel_layout.selected") } else { "" }]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::ListBoxOption, ui.is_enabled(), selected, &accessible));
    let target = match item {
        Tile::Custom => "custom".to_owned(),
        Tile::Save => "save_as".to_owned(),
        Tile::Saved(entry) => format!("tile:{}", entry.name),
    };
    crate::scenario::publish_target(&format!("prefs:panel_layout:{target}"), rect);
    response.backing_on_hover_text(hint)
}

fn dashed_border(painter: &egui::Painter, rect: Rect, stroke: Stroke) {
    painter.extend(egui::Shape::dashed_line(
        &[rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()],
        stroke,
        5.0,
        4.0,
    ));
}

fn tile_menu(app: &SolveApp, ui: &mut Ui, state: &mut State, tile: &Response, entry: &LayoutPreferenceEntry, keyboard: bool) {
    let t = Tokens::get();
    let at = Rect::from_min_size(tile.rect.min + vec2(TILE_WIDTH - 24.0, 4.0), vec2(20.0, 20.0));
    let id = tile.id.with("menu");
    let popup = ui_kit::Menu::button_with(
        ui,
        |ui, _| {
            let response = ui.interact(at, id, Sense::CLICK);
            if keyboard {
                egui::Popup::open_id(ui.ctx(), egui::Popup::default_response_id(&response));
            }
            ui.painter().rect(at, 5, t.control_fill, Stroke::new(1.0, t.dialog_border), egui::StrokeKind::Inside);
            for x in [-4.0, 0.0, 4.0] {
                ui.painter().circle_filled(at.center() + vec2(x, 0.0), 1.0, t.form_dim);
            }
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), app.tr("ui.panel_layout.replace_current")));
            crate::scenario::publish_target(&format!("prefs:panel_layout:menu:{}", entry.name), at);
            response
        },
        |ui| {
            let action = ui_kit::button(ui, app.tr("ui.panel_layout.replace_current"), ui_kit::Button::Ghost);
            if keyboard {
                action.request_focus();
            }
            if action.clicked() {
                state.popup = Some(Popover { origin: tile.id, kind: PopoverKind::Replace { name: entry.name.clone() }, first: true });
                ui.close();
            }
        },
    );
    if popup.inner.is_some() {
        crate::ui_kit::settings_search::note_key_sink(ui.ctx());
    }
}

fn open_save(ctx: &egui::Context, state: &mut State, tile: &Response, view: &LayoutPreferencesView) {
    let name = (1..=2 * commands::MAX_USER_PRESETS + 1)
        .map(|n| crate::i18n::format("ui.panel_layout.suggested_name", &[("n", &n.to_string())]))
        .find(|name| {
            !view.entries.iter().any(|entry| commands::preset_names_equal(&entry.name, name))
                && !view.builtin_names.iter().any(|builtin| commands::preset_names_equal(builtin, name))
        })
        .unwrap_or_default();
    state.popup = Some(Popover { origin: tile.id, kind: PopoverKind::Save { name, chosen: 0 }, first: true });
    ctx.request_repaint();
}

fn validation<'a>(name: &str, view: &'a LayoutPreferencesView) -> (Option<&'static str>, Option<&'a str>) {
    if name.trim().is_empty() {
        return (Some("ui.panel_layout.validation.empty"), None);
    }
    if commands::preset_name(name).is_err() {
        return (Some("ui.panel_layout.validation.invalid"), None);
    }
    let name = name.trim();
    if view.builtin_names.iter().any(|builtin| commands::preset_names_equal(builtin, name)) {
        return (Some("ui.panel_layout.validation.builtin"), None);
    }
    if let Some(entry) = view.entries.iter().find(|entry| commands::preset_names_equal(&entry.name, name)) {
        return if entry.user {
            (Some("ui.panel_layout.validation.duplicate"), Some(&entry.name))
        } else {
            (Some("ui.panel_layout.validation.builtin"), None)
        };
    }
    (None, None)
}

fn popover(app: &mut SolveApp, ui: &mut Ui, view: &LayoutPreferencesView, state: &mut State) {
    let Some(mut popup) = state.popup.take() else { return };
    if state.escape {
        state.escape = false;
        egui::Popup::close_id(ui.ctx(), popup.origin.with("popup"));
        ui.ctx().memory_mut(|memory| memory.request_focus(state.list_focus.unwrap_or(popup.origin)));
        return;
    }
    let Some(anchor) = ui.ctx().read_response(popup.origin) else { return };
    let popup_id = egui::Popup::default_response_id(&anchor);
    if popup.first {
        egui::Popup::open_id(ui.ctx(), popup_id);
    }
    let first = popup.first;
    popup.first = false;
    let t = Tokens::get();
    let max_width = (ui.ctx().content_rect().width() - t.space_24 * 2.0).max(TILE_WIDTH);
    let width = (t.form_label_width + t.space_24 * 12.0).min(max_width);
    let mut close = false;
    let mut replace = None;
    let mut save = None;
    let mut focus_ids = Vec::new();
    let shown = Container::new(ui.ctx(), Surface::Popup)
        .menu(&anchor)
        .open_memory(None)
        .anchor(anchor.rect)
        .frame(ui_kit::Menu::frame())
        .width(width)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui_kit::scope(ui, |ui| {
                ui_kit::settings_form(ui, 0.0, |ui| {
                    ui.set_width(width);
                    ui.spacing_mut().item_spacing = vec2(t.space_8, t.space_8);
                    crate::ui_kit::settings_search::push_force(ui);
                    crate::ui_kit::settings_search::note_key_sink(ui.ctx());
                    match &mut popup.kind {
                        PopoverKind::Save { name, chosen } => {
                            let users: Vec<_> = view.entries.iter().filter(|entry| entry.user).collect();
                            let full = users.len() >= commands::MAX_USER_PRESETS;
                            let labels: Vec<_> = users.iter().map(|entry| entry.name.as_str()).collect();
                            let full_notice = full.then(|| validation_alert(app, ui, "ui.panel_layout.validation.full", t.pref_note));
                            let mut actions_width = if full {
                                ui_kit::ContentWidth::button(ui, app.tr("ui.panel_layout.replace_short"))
                            } else {
                                ui_kit::ContentWidth::button(ui, app.tr("ui.save")) + t.control_height + t.space_8
                            };
                            if !full && validation(name, view).1.is_some() {
                                actions_width += ui_kit::ContentWidth::button(ui, app.tr("ui.panel_layout.replace_short")) + t.space_8;
                            }
                            let inline = ui.available_width() >= actions_width + t.space_24 * 6.0;
                            let actions = if inline {
                                ui.horizontal_top(|ui| {
                                    let field_width = (ui.available_width() - actions_width - t.space_12).max(t.space_24 * 6.0);
                                    let entered = ui
                                        .allocate_ui_with_layout(
                                            vec2(field_width, t.control_height),
                                            egui::Layout::top_down(egui::Align::Min),
                                            |ui| {
                                                ui.set_width(field_width);
                                                save_field(ui, name, chosen, &labels, first, &mut focus_ids, view)
                                            },
                                        )
                                        .inner;
                                    save_actions(app, ui, name, view, entered, &mut focus_ids)
                                })
                                .inner
                            } else {
                                let entered = save_field(ui, name, chosen, &labels, first, &mut focus_ids, view);
                                ui.horizontal_wrapped(|ui| save_actions(app, ui, name, view, entered, &mut focus_ids)).inner
                            };
                            save = actions.save;
                            replace = actions.replace;
                            close = actions.close;
                            let (message, duplicate) = validation(name, view);
                            let notice = if full {
                                full_notice
                            } else {
                                message
                                    .map(|message| validation_alert(app, ui, message, if duplicate.is_some() { t.pref_note } else { t.form_error }))
                            };
                            if let Some(response) = notice
                                && let Some(field) = focus_ids.first()
                            {
                                ui.ctx().accesskit_node_builder(*field, |node| node.push_described_by(response.id.accesskit_id()));
                            }
                        }
                        PopoverKind::Replace { name } => {
                            let question = crate::i18n::format("ui.panel_layout.confirm_replace", &[("name", name)]);
                            ui.label(egui::RichText::new(question).size(t.font_label).color(t.form_ink));
                            ui.horizontal_wrapped(|ui| {
                                let cancel = ui_kit::button(ui, app.tr("ui.panel_layout.cancel"), ui_kit::Button::Ghost);
                                focus_ids.push(cancel.id);
                                crate::scenario::publish_target("prefs:panel_layout:replace_cancel", cancel.rect);
                                if first {
                                    cancel.request_focus();
                                }
                                close |= cancel.clicked();
                                let response = danger_button(ui, app.tr("ui.panel_layout.replace_short"));
                                focus_ids.push(response.id);
                                crate::scenario::publish_target("prefs:panel_layout:replace_confirm", response.rect);
                                if response.clicked() {
                                    save = Some(name.clone());
                                }
                            });
                        }
                    }
                    trap_focus(ui, &focus_ids);
                    crate::ui_kit::settings_search::pop_force(ui);
                })
            })
        });
    if let Some(shown) = &shown {
        crate::scenario::publish_rect("prefs:panel_layout:popover", shown.response.rect);
        crate::scenario::publish_rect(
            if matches!(popup.kind, PopoverKind::Replace { .. }) { "prefs:panel_layout:replace_popover" } else { "prefs:panel_layout:save_popover" },
            shown.response.rect,
        );
    }
    if let Some(name) = replace {
        popup.kind = PopoverKind::Replace { name };
        popup.first = true;
    }
    if let Some(name) = save {
        let is_replace = matches!(popup.kind, PopoverKind::Replace { .. });
        if run(app, "layout.save", json!({"name":name,"replace":is_replace}), Some((&name, view))) {
            state.focus_name = Some(name.clone());
            state.source_name = Some(name);
            close = true;
        }
    }
    if close {
        egui::Popup::close_id(ui.ctx(), popup_id);
        ui.ctx().memory_mut(|memory| memory.request_focus(state.list_focus.unwrap_or(popup.origin)));
    } else if shown.is_some() && egui::Popup::is_id_open(ui.ctx(), popup_id) {
        state.popup = Some(popup);
    }
}

fn save_field(
    ui: &mut Ui,
    name: &mut String,
    chosen: &mut usize,
    users: &[&str],
    first: bool,
    ids: &mut Vec<Id>,
    view: &LayoutPreferencesView,
) -> bool {
    if users.len() >= commands::MAX_USER_PRESETS {
        let response = ui_kit::dropdown(ui, "panel_layout_replace_name", chosen, users);
        ids.push(response.id);
        if let Some(value) = users.get(*chosen) {
            *name = (*value).to_owned();
        }
        if first {
            response.request_focus();
        }
        return false;
    }
    let t = Tokens::get();
    ui_kit::row(ui, "", |ui| {
        let (enter, response) = ui_kit::text_field_reserving(ui, name, "", t.space_24 * 2.0);
        ids.push(response.id);
        response.widget_info(|| {
            let mut info = egui::WidgetInfo::labeled(egui::Role::TextInput, ui.is_enabled(), crate::i18n::tr("ui.panel_layout.save_as"));
            info.current_text_value = Some(name.clone());
            info
        });
        crate::scenario::publish_target("prefs:panel_layout:name", response.rect);
        let (message, duplicate) = validation(name, view);
        if message.is_some() && duplicate.is_none() {
            ui.painter().rect_stroke(response.rect, t.control_radius, Stroke::new(1.0, t.form_error), egui::StrokeKind::Inside);
        }
        if first {
            response.request_focus();
            if let Some(mut edit) = egui::TextEdit::load_state(ui.ctx(), response.id) {
                edit.cursor
                    .set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(name.chars().count()))));
                edit.store(ui.ctx(), response.id);
            }
        }
        let count = name.trim().chars().count();
        let counter = Rect::from_min_max(pos2(response.rect.right() - t.space_24 * 2.0, response.rect.top()), response.rect.max);
        ui.painter().text(
            counter.center(),
            Align2::CENTER_CENTER,
            format!("{count}/64"),
            FontId::proportional(t.font_caption),
            if count > 64 { t.form_error } else { t.form_dim },
        );
        enter || (response.has_focus() && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)))
    })
}

#[derive(Default)]
struct SaveActions {
    save: Option<String>,
    replace: Option<String>,
    close: bool,
}

fn save_actions(app: &SolveApp, ui: &mut Ui, name: &str, view: &LayoutPreferencesView, entered: bool, ids: &mut Vec<Id>) -> SaveActions {
    let t = Tokens::get();
    let mut actions = SaveActions::default();
    let full = view.entries.iter().filter(|entry| entry.user).count() >= commands::MAX_USER_PRESETS;
    let (message, duplicate) = validation(name, view);
    let valid = message.is_none() && !full;
    if !full {
        let save = ui.add_enabled_ui(valid, |ui| ui_kit::button(ui, app.tr("ui.save"), ui_kit::Button::Primary)).inner;
        if save.enabled() {
            ids.push(save.id);
        }
        crate::scenario::publish_target("prefs:panel_layout:save", save.rect);
        if (save.clicked() || entered) && valid {
            actions.save = Some(name.trim().to_owned());
        }
    }
    if let Some(name) = duplicate {
        let response = danger_button(ui, app.tr("ui.panel_layout.replace_short"));
        ids.push(response.id);
        crate::scenario::publish_target("prefs:panel_layout:replace", response.rect);
        if response.clicked() {
            actions.replace = Some(name.to_owned());
        }
    }
    if !full {
        let (rect, _) = ui.allocate_exact_size(vec2(t.control_height, t.control_height), Sense::hover());
        ui.painter().rect(rect, t.control_radius, t.control_fill, Stroke::new(1.0, t.control_border), egui::StrokeKind::Inside);
        let close = ui_kit::icon_button_at(ui, rect, "close", app.tr("ui.close"));
        crate::scenario::publish_target("prefs:panel_layout:save_close", close.rect);
        ids.push(close.id);
        actions.close = close.clicked();
    }
    actions
}

fn validation_alert(app: &SolveApp, ui: &mut Ui, message: &str, color: egui::Color32) -> Response {
    let t = Tokens::get();
    let response = ui
        .horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(t.space_14, t.space_14), Sense::hover());
            crate::icons::paint_ink(ui.painter(), rect, "status_warning", color);
            ui.label(egui::RichText::new(app.tr(message)).size(t.font_caption).color(color))
        })
        .inner;
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Alert, true, app.tr(message)));
    response
}

fn trap_focus(ui: &Ui, ids: &[Id]) {
    let Some(focused) = ui.memory(|memory| memory.focused()) else { return };
    if let Some(index) = ids.iter().position(|id| *id == focused) {
        ui.memory_mut(|memory| memory.set_focus_lock_filter(focused, egui::EventFilter { tab: true, escape: true, ..Default::default() }));
        let shift = ui.input(|input| input.modifiers.shift);
        if ui.input_mut(|input| input.consume_key(if shift { egui::Modifiers::SHIFT } else { egui::Modifiers::NONE }, egui::Key::Tab)) {
            let next = if shift { index.checked_sub(1).unwrap_or(ids.len().saturating_sub(1)) } else { (index + 1) % ids.len().max(1) };
            if let Some(id) = ids.get(next) {
                ui.memory_mut(|memory| memory.request_focus(*id));
            }
        }
    }
}

fn danger_button(ui: &mut Ui, label: &str) -> Response {
    ui_kit::button(ui, egui::RichText::new(label).size(Tokens::get().font_label), ui_kit::Button::DangerOutlined)
}

fn run(app: &mut SolveApp, command: &str, params: serde_json::Value, applied: Option<(&str, &LayoutPreferencesView)>) -> bool {
    match app.run(command, params) {
        Ok(_) => {
            if let Some((name, view)) = applied {
                let workspace = view
                    .workspaces
                    .iter()
                    .find(|workspace| workspace.id == view.selected_workspace)
                    .map(|workspace| app.tr(&workspace.title_key))
                    .unwrap_or_default();
                app.set_status(crate::i18n::format("ui.panel_layout.applied", &[("name", name), ("workspace", workspace)]), false);
            }
            true
        }
        Err(reason) => {
            app.set_status(reason, true);
            false
        }
    }
}

/// A schematic from saved placement, never a render of hidden panel contents.
fn thumbnail(painter: &egui::Painter, rect: Rect, layout: &Layout) {
    let t = Tokens::get();
    painter.rect(rect, 3, t.figure_accent.gamma_multiply(0.16), Stroke::new(0.7, t.figure_accent.gamma_multiply(0.5)), egui::StrokeKind::Inside);
    let sx = rect.width() / 1000.0;
    let sy = rect.height() / 700.0;
    let shown = |section: &&solidcraft_engine::panels::Section| {
        section.tabs.iter().any(|id| layout.instance(id).is_ok_and(|instance| instance.requested_open))
    };
    let bottom = layout
        .sections
        .iter()
        .filter(shown)
        .filter(|section| section.location.zone() == Zone::Bottom)
        .map(|section| section.size.height)
        .fold(0.0, f32::max)
        .min(300.0)
        * sy;
    for zone in [Zone::Left, Zone::Right] {
        let mut edge = if zone == Zone::Left { rect.left() } else { rect.right() };
        for column in layout.columns.iter().filter(|column| column.zone == zone) {
            let width = (column.width * sx).min(rect.width() * 0.35);
            let x = if zone == Zone::Left { edge } else { edge - width };
            let sections: Vec<_> = layout.sections.iter().filter(shown).filter(|section| section.column.as_ref() == Some(&column.id)).collect();
            if sections.is_empty() {
                continue;
            }
            let weights: f32 = sections.iter().map(|section| if section.collapsed { 0.12 } else { section.weight }).sum::<f32>().max(0.01);
            let mut top = rect.top();
            for section in sections {
                let height = (rect.height() - bottom) * if section.collapsed { 0.12 } else { section.weight } / weights;
                let card = Rect::from_min_size(pos2(x, top), vec2(width, height)).shrink(1.0);
                painter.rect_filled(card, 2.5, t.form_dim.gamma_multiply(if section.collapsed { 0.26 } else { 0.4 }));
                top += height;
            }
            edge += if zone == Zone::Left { width } else { -width };
        }
    }
    let bottoms: Vec<_> = layout.sections.iter().filter(shown).filter(|section| section.location.zone() == Zone::Bottom).collect();
    let total_width = bottoms.iter().map(|section| section.size.width).sum::<f32>().max(1.0);
    let mut left = rect.left();
    for section in bottoms {
        let width = rect.width() * section.size.width / total_width;
        painter.rect_filled(
            Rect::from_min_size(pos2(left, rect.bottom() - bottom), vec2(width, bottom)).shrink(1.0),
            2.5,
            t.form_dim.gamma_multiply(0.4),
        );
        left += width;
    }
    for section in layout.sections.iter().filter(shown) {
        let origin = match &section.location {
            SectionLocation::Floating { origin } | SectionLocation::Window { origin, .. } => *origin,
            _ => continue,
        };
        let width = (section.size.width * sx).min(rect.width() * 0.7);
        let height = (section.size.height * sy).min(rect.height() * 0.75);
        let at = pos2(
            (rect.left() + origin.x * sx).clamp(rect.left(), rect.right() - width),
            (rect.top() + origin.y * sy).clamp(rect.top(), rect.bottom() - height),
        );
        painter.rect_filled(Rect::from_min_size(at, vec2(width, height)), 2.5, t.form_dim.gamma_multiply(0.45));
    }
}
