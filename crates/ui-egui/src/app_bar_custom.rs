//! Application-bar editing. Every entry point edits `ToolbarCustom.app_bar` through the kit model.

use crate::ui_kit::{self, BackingTooltipExt as _, Item};
use crate::{SolveApp, context_menu};
use egui::{Pos2, Rect};
use serde_json::{Value, json};
use ui_kit::app_bar_items::{self, Layout, Preset, Zone};

fn defaults(app: &SolveApp) -> Layout {
    Layout::default_for(app.toolbar_custom.quick_access.iter().map(String::as_str))
}

pub(crate) fn items(app: &SolveApp, id: Option<&str>) -> Vec<Item> {
    let mut rows = Vec::new();
    if let Some(id) = id.filter(|id| app.toolbar_custom.app_bar.editable(id)) {
        for (action, key, icon) in [
            ("hide", "ui.hide", "close"),
            ("start", "app_bar.move_start", "undo"),
            ("end", "app_bar.move_end", "redo"),
            ("left", "app_bar.move_left", "undo"),
            ("right", "app_bar.move_right", "redo"),
        ] {
            rows.push(Item::action("ui.appBarEdit", app.tr(key), icon).with(json!({"item":id,"action":action})));
        }
        rows.push(Item::sep());
    }
    rows.push(Item::action("ui.appBarCustomize", app.tr("cmd.ui.appBarCustomize"), "settings"));
    if id.is_none() {
        rows.push(Item::action("ui.appBarReset", app.tr("cmd.ui.appBarReset"), "restore"));
    }
    rows
}

pub(crate) fn run(app: &mut SolveApp, p: &Value) {
    let id = p.get("item").and_then(Value::as_str).unwrap_or("");
    let defaults = defaults(app);
    let layout = &mut app.toolbar_custom.app_bar;
    match p.get("action").and_then(Value::as_str).unwrap_or("") {
        "hide" => {
            layout.set_shown(id, false, &defaults);
        }
        "show" => {
            layout.set_shown(id, true, &defaults);
        }
        "start" => {
            layout.move_before(id, Zone::Start, None, &defaults);
        }
        "end" => {
            layout.move_before(id, Zone::End, None, &defaults);
        }
        "left" => {
            layout.move_step(id, false, &defaults);
        }
        "right" => {
            layout.move_step(id, true, &defaults);
        }
        _ => {}
    }
}

#[derive(Clone)]
struct BarDrag {
    id: String,
    origin: Pos2,
    active: bool,
    cancelled: bool,
}

/// Observe the kit's geometry without covering the bar children's click targets.
/// Returns true through release/cancellation so the original item's click cannot also run.
pub(crate) fn bar(app: &mut SolveApp, ui: &mut egui::Ui, content: Rect, out: &ui_kit::app_bar::BarOut) -> bool {
    let key = egui::Id::unique("sc_appbar_edit_drag");
    let mut drag = ui.ctx().data(|d| d.get_temp::<BarDrag>(key));
    let pointer = ui.input(|i| i.pointer.interact_pos());
    if let Some(p) = pointer
        && ui.input(|i| i.pointer.primary_pressed())
        && ui.rect_contains_pointer(content)
    {
        drag = out.edit_items.iter().find(|item| !item.locked && p.x >= item.left && p.x <= item.right).map(|item| BarDrag {
            id: item.id.clone(),
            origin: p,
            active: false,
            cancelled: false,
        });
    }
    let mut consumed = false;
    if let Some(held) = &mut drag {
        if let Some(p) = pointer {
            held.active |= app_bar_items::is_drag(p.x - held.origin.x, p.y - held.origin.y);
        }
        held.cancelled |= ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        consumed = held.active || held.cancelled;
        if held.active
            && !held.cancelled
            && let Some(p) = pointer.filter(|p| content.contains(*p))
            && let Some(slot) = app_bar_items::drop_slot(p.x, (out.tabs_start + out.tabs_end) / 2.0, &out.edit_items, &held.id)
        {
            ui_kit::insertion_marker(ui.painter(), slot.x, content.top() + 6.0, content.bottom() - 6.0);
            if ui.input(|i| i.pointer.primary_released()) {
                let defaults = defaults(app);
                app.toolbar_custom.app_bar.move_before(&held.id, slot.zone, slot.before.as_deref(), &defaults);
            }
        }
        if ui.input(|i| i.pointer.primary_released() || !i.pointer.primary_down()) {
            drag = None;
        }
    }
    ui.ctx().data_mut(|d| match drag {
        Some(drag) => {
            d.insert_temp(key, drag);
        }
        None => d.remove::<BarDrag>(key),
    });
    consumed
}

/// Tabs publish their actual occupied rectangles after drawing; their unused flexible space
/// is bare bar, and offers Reset as well as Customize.
pub(crate) fn menu(app: &mut SolveApp, ui: &egui::Ui, content: Rect, out: &ui_kit::app_bar::BarOut) {
    if ui.input(|i| i.pointer.secondary_clicked())
        && ui.rect_contains_pointer(content)
        && let Some(p) = ui.input(|i| i.pointer.interact_pos())
    {
        let id = out
            .edit_items
            .iter()
            .find(|item| p.x >= item.left && p.x <= item.right)
            .filter(|item| item.id != "tabs" || solidcraft_ui_registry::rects("tab:").iter().any(|(_, rect)| rect.contains(p)))
            .map(|item| item.id.clone());
        context_menu::open_for(app, p, context_menu::Target::AppBar { id });
    }
}

fn label_icon(app: &SolveApp, id: &str) -> (String, String) {
    if let Some(command) = id.strip_prefix("quick:").and_then(solidcraft_engine::find_command) {
        return (app.tr_label(command.label).into(), command.info(&app.session).icon.into());
    }
    let (key, icon) = match id {
        "home" => ("ui.start_page", "home"),
        "save" => ("ui.save", "save"),
        "undo" => ("ui.undo", "undo"),
        "redo" => ("ui.redo", "redo"),
        "search" => ("cmd.ui.palette", "zoom"),
        "assistant" => ("cmd.ui.assistant", "assistant"),
        "donate" => ("links.kofi_button", "kofi"),
        "discord" => ("links.discord_button", "discord"),
        "report" => ("ui.report_a_bug", "bug_report"),
        "settings" => ("ui.preferences", "settings"),
        "help" => ("ui.help", "help"),
        "version" => ("app_bar.version", "help"),
        _ => return (id.into(), "help".into()),
    };
    (app.tr(key).into(), icon.into())
}

pub(crate) fn preset(app: &mut SolveApp, preset: Preset) {
    let defaults = defaults(app);
    app.toolbar_custom.app_bar.set_preset(preset, &defaults);
}

#[derive(Clone)]
struct RowDrag(String);

/// Home owns Escape too. Close this foreground UI before Home can consume the same key.
pub(crate) fn home_escape(app: &mut SolveApp, ctx: &egui::Context) {
    let menu_open = app.viewport.context_menu.is_some() && matches!(app.menu.target, context_menu::Target::AppBar { .. });
    if (menu_open || app.toolbar_custom.app_bar_customizing) && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        if menu_open {
            context_menu::close(app);
        } else {
            app.toolbar_custom.app_bar_customizing = false;
        }
    }
}

pub(crate) fn window(app: &mut SolveApp, ctx: &egui::Context) {
    if !app.toolbar_custom.app_bar_customizing {
        return;
    }
    let defaults = defaults(app);
    let snapshot = app.toolbar_custom.app_bar.clone();
    let mut selected_preset = None;
    let mut reset = false;
    let mut edit = None;
    let mut drop = None;
    let t = solidcraft_theme::Tokens::get();
    let origin =
        egui::pos2((ctx.content_rect().center().x - t.form_card_width).max(ctx.content_rect().left()), ctx.content_rect().top() + t.space_24 * 2.0);
    let (closed, action) = ui_kit::Window::new(egui::Id::unique("sc_appbar_customize"), "settings", app.tr("cmd.ui.appBarCustomize"))
        .placed(egui::Align2::LEFT_TOP, origin)
        .cards(2.0)
        .actions("", app.tr("ui.close"), None)
        .show(ctx, true, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (value, key) in [
                    (Preset::Default, "app_bar.default"),
                    (Preset::Minimal, "app_bar.minimal"),
                    (Preset::Full, "app_bar.full"),
                    (Preset::Compact, "app_bar.compact"),
                ] {
                    let response = ui_kit::button(ui, app.tr(key), ui_kit::Button::Normal);
                    crate::scenario::publish_target(&format!("appbar:preset:{value:?}"), response.rect);
                    if response.clicked() {
                        selected_preset = Some(value);
                    }
                }
            });
            egui::ScrollArea::vertical().max_height((ctx.content_rect().height() * 0.6).max(80.0)).show(ui, |ui| {
                for (zone, hidden, key) in [(Zone::Start, false, "ui.start"), (Zone::End, false, "ui.end"), (Zone::Start, true, "app_bar.hidden")] {
                    ui_kit::section(ui, key, app.tr(key), true, |ui| {
                        for item in
                            snapshot.items.iter().filter(|p| snapshot.editable(&p.id) && if hidden { !p.shown } else { p.shown && p.zone == zone })
                        {
                            ui.push_id(&item.id, |ui| {
                                let row = ui
                                    .horizontal(|ui| {
                                        let handle = ui_kit::button(ui, "≡", ui_kit::Button::Normal)
                                            .interact(egui::Sense::drag())
                                            .backing_on_hover_text(app.tr("app_bar.drag"));
                                        crate::scenario::publish_target(&format!("appbar:drag:{}", item.id), handle.rect);
                                        if handle.drag_started() {
                                            egui::DragAndDrop::set_payload(ctx, RowDrag(item.id.clone()));
                                        }
                                        let (label, icon) = label_icon(app, &item.id);
                                        let mut shown = item.shown;
                                        let check = ui_kit::checkbox_icon(ui, &label, &icon, &mut shown);
                                        crate::scenario::publish_target(&format!("appbar:shown:{}", item.id), check.rect);
                                        if check.changed() {
                                            edit = Some(json!({"item":item.id,"action":if shown {"show"} else {"hide"}}));
                                        }
                                        for (label, action) in [("↑", "left"), ("↓", "right")] {
                                            if ui_kit::button(ui, label, ui_kit::Button::Normal)
                                                .backing_on_hover_text(app.tr(if action == "left" { "app_bar.move_up" } else { "app_bar.move_down" }))
                                                .clicked()
                                            {
                                                edit = Some(json!({"item":item.id,"action":action}));
                                            }
                                        }
                                        let destination = if item.zone == Zone::Start { "end" } else { "start" };
                                        if ui_kit::button(
                                            ui,
                                            app.tr(if item.zone == Zone::Start { "app_bar.move_end" } else { "app_bar.move_start" }),
                                            ui_kit::Button::Normal,
                                        )
                                        .clicked()
                                        {
                                            edit = Some(json!({"item":item.id,"action":destination}));
                                        }
                                    })
                                    .response;
                                if let Some(payload) = egui::DragAndDrop::payload::<RowDrag>(ctx)
                                    && ui.rect_contains_pointer(row.rect)
                                {
                                    ui_kit::insertion_marker(ui.painter(), row.rect.left(), row.rect.top(), row.rect.bottom());
                                    if ui.input(|i| i.pointer.primary_released()) {
                                        drop = Some((payload.0.clone(), item.zone, Some(item.id.clone()), hidden));
                                    }
                                }
                            });
                        }
                        // A full-width target also makes empty zones reachable.
                        let target = ui.allocate_response(
                            egui::vec2(ui.available_width(), solidcraft_theme::Tokens::get().control_height),
                            egui::Sense::hover(),
                        );
                        crate::scenario::publish_target(&format!("appbar:zone:{key}"), target.rect);
                        if let Some(payload) = egui::DragAndDrop::payload::<RowDrag>(ctx)
                            && target.contains_pointer()
                        {
                            ui_kit::insertion_marker(ui.painter(), target.rect.left(), target.rect.top(), target.rect.bottom());
                            if ui.input(|i| i.pointer.primary_released()) {
                                drop = Some((payload.0.clone(), zone, None, hidden));
                            }
                        }
                    });
                }
            });
            if ui_kit::button(ui, app.tr("cmd.ui.appBarReset"), ui_kit::Button::Normal).clicked() {
                reset = true;
            }
        });
    if let Some(preset_value) = selected_preset {
        preset(app, preset_value);
        ctx.request_repaint();
    }
    if let Some(edit) = edit {
        run(app, &edit);
        ctx.request_repaint();
    }
    if let Some((id, zone, before, hidden)) = drop {
        let layout = &mut app.toolbar_custom.app_bar;
        if !hidden {
            layout.move_before(&id, zone, before.as_deref(), &defaults);
        }
        layout.set_shown(&id, !hidden, &defaults);
        egui::DragAndDrop::clear_payload(ctx);
        ctx.request_repaint();
    }
    if reset {
        app.start("ui.appBarReset");
        ctx.request_repaint();
    }
    app.toolbar_custom.app_bar_customizing = !closed && action != ui_kit::Action::Cancel;
}
