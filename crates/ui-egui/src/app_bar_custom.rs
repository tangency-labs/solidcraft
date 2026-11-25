//! App-bar edit mode. The tray, drag previews and fade are transient; only the item list is saved.
use crate::ui_kit::{self, Container, Item, Surface};
use crate::{SolveApp, context_menu};
use egui::{FontId, Id, Pos2, Rect, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};
use ui_kit::app_bar::{self, Control};
use ui_kit::app_bar_items::{self, DragRelease, DropSlot, Layout, Preset, Zone};

fn defaults(app: &SolveApp) -> Layout {
    Layout::default_for(app.toolbar_custom.quick_access.iter().map(String::as_str))
}
fn drag_key() -> Id {
    Id::unique("sc_appbar_tray_drag")
}
fn spring_key() -> Id {
    Id::unique("sc_appbar_tray_spring")
}
#[derive(Clone)]
struct Drag {
    id: String,
    from_bar: bool,
    origin: Pos2,
    return_to: Pos2,
    pointer: Pos2,
    active: bool,
    slot: Option<DropSlot>,
    below: bool,
}
#[derive(Clone)]
struct Spring {
    id: String,
    from: Pos2,
    to: Pos2,
    started: f64,
    from_bar: bool,
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
            rows.push(
                Item::action("ui.appBarEdit", app.tr(key), icon)
                    .on_if(action != "hide" || app_bar_items::can_remove(id))
                    .with(json!({"item":id,"action":action})),
            );
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
pub(crate) fn preset(app: &mut SolveApp, value: Preset) {
    let defaults = defaults(app);
    app.toolbar_custom.app_bar.set_preset(value, &defaults);
}
pub(crate) fn add(app: &mut SolveApp, id: &str) {
    if matches!(id, "separator" | "space") {
        let defaults = defaults(app);
        app.toolbar_custom.app_bar.add_object(id, Zone::End, None, &defaults);
        return;
    }
    if app.toolbar_custom.app_bar.items.iter().any(|p| p.id == id && p.shown) {
        return;
    }
    let defaults = defaults(app);
    app.toolbar_custom.app_bar.move_before(id, Zone::End, None, &defaults);
    app.toolbar_custom.app_bar.set_shown(id, true, &defaults);
}

/// A temporary list paints the actual item at its landing slot, moving neighbours without
/// saving the preview. Outside a card, the source is hidden and the chip follows the pointer.
pub(crate) fn preview(app: &SolveApp, ctx: &egui::Context) -> Layout {
    let mut layout = app.toolbar_custom.app_bar.clone();
    let drag = ctx.data(|d| d.get_temp::<Drag>(drag_key()));
    let spring = ctx.data(|d| d.get_temp::<Spring>(spring_key()));
    let lifted = if app.toolbar_custom.app_bar_customizing {
        drag.as_ref()
            .filter(|d| d.active && d.slot.is_none())
            .map(|d| d.id.clone())
            .or_else(|| spring.as_ref().filter(|s| s.from_bar).map(|s| s.id.clone()))
    } else {
        None
    };
    app_bar::editing(ctx, app.toolbar_custom.app_bar_customizing, lifted);
    app_bar::dragging(ctx, app.toolbar_custom.app_bar_customizing && drag.as_ref().is_some_and(|d| d.active));
    if let Some(drag) = drag.filter(|d| d.active && app.toolbar_custom.app_bar_customizing) {
        let defaults = defaults(app);
        if let Some(slot) = drag.slot {
            if matches!(drag.id.as_str(), "separator" | "space") {
                layout.add_object(&drag.id, slot.zone, slot.before.as_deref(), &defaults);
            } else {
                layout.move_before(&drag.id, slot.zone, slot.before.as_deref(), &defaults);
                layout.set_shown(&drag.id, true, &defaults);
            }
        } else if drag.from_bar && drag.below {
            layout.set_shown(&drag.id, false, &defaults);
        }
    }
    layout
}
fn spring(ctx: &egui::Context, drag: &Drag) {
    let started = ctx.input(|i| i.time);
    ctx.data_mut(|d| {
        d.insert_temp(spring_key(), Spring { id: drag.id.clone(), from: drag.pointer, to: drag.return_to, started, from_bar: drag.from_bar })
    });
    ctx.request_repaint();
}
fn cancel_or_exit(app: &mut SolveApp, ctx: &egui::Context) {
    if app.toolbar_custom.app_bar_customizing && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        let drag = ctx.data_mut(|d| {
            let drag = d.get_temp::<Drag>(drag_key());
            d.remove::<Drag>(drag_key());
            drag
        });
        if let Some(drag) = drag.filter(|d| d.active) {
            spring(ctx, &drag);
            ctx.data_mut(|d| d.insert_temp(Id::unique("appbar_cancelled_press"), true));
        } else {
            app.toolbar_custom.app_bar_customizing = false;
        }
        ctx.request_repaint();
    }
}
pub(crate) fn home_escape(app: &mut SolveApp, ctx: &egui::Context) {
    let menu_open = app.viewport.context_menu.is_some() && matches!(app.menu.target, context_menu::Target::AppBar { .. });
    if menu_open && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        context_menu::close(app);
    } else {
        cancel_or_exit(app, ctx);
    }
}

fn zones(content: Rect, out: &app_bar::BarOut) -> [(Zone, Rect); 2] {
    let [start, end] = out.cards;
    [(Zone::Start, start), (Zone::End, end)].map(|(zone, card)| {
        if let Some(card) = card {
            return (zone, card);
        }
        let mut left = f32::INFINITY;
        let mut right = f32::NEG_INFINITY;
        for item in out.edit_items.iter().filter(|p| !p.locked && p.zone == zone) {
            left = left.min(item.left);
            right = right.max(item.right);
        }
        if !left.is_finite() {
            let edge = if zone == Zone::Start { out.tabs_start } else { out.tabs_end };
            let gap = ui_kit::app_bar_layout::application::GAP;
            left = if zone == Zone::Start { edge - gap } else { edge };
            right = left + gap;
        }
        let mut lo = content.left();
        let mut hi = content.right();
        for fixed in out.edit_items.iter().filter(|p| p.locked) {
            if fixed.right <= left {
                lo = lo.max(fixed.right);
            }
            if fixed.left >= right {
                hi = hi.min(fixed.left);
            }
        }
        (zone, Rect::from_min_max(pos2(left.max(lo), content.top()), pos2(right.min(hi).max(left.max(lo)), content.bottom())))
    })
}

/// Cards use existing gaps only and never reserve layout space.
pub(crate) fn backing(ui: &egui::Ui, content: Rect, out: &app_bar::BarOut, shapes: [egui::layers::ShapeIdx; 2]) {
    let t = solidcraft_theme::Tokens::get();
    for ((zone, rect), shape) in zones(content, out).into_iter().zip(shapes) {
        if out.cards.into_iter().flatten().all(|card| card != rect) {
            continue;
        }
        ui.painter().set(
            shape,
            egui::Shape::Vec(vec![
                egui::Shape::rect_filled(rect, 9, app_bar::edit_palette(t).card),
                egui::Shape::rect_stroke(rect, 9, Stroke::new(1.0, t.border), egui::StrokeKind::Inside),
            ]),
        );
        crate::scenario::publish_rect(if zone == Zone::Start { "appbar:zone:start" } else { "appbar:zone:end" }, rect);
    }
}
pub(crate) fn bar(app: &mut SolveApp, ui: &mut egui::Ui, content: Rect, out: &app_bar::BarOut) {
    if !app.toolbar_custom.app_bar_customizing {
        return;
    }
    if ui.input(|i| i.pointer.primary_pressed()) {
        ui.ctx().data_mut(|d| d.remove::<bool>(Id::unique("appbar_cancelled_press")));
    }
    let mut drag = ui.ctx().data(|d| d.get_temp::<Drag>(drag_key()));
    for item in &out.edit_items {
        let rect = Rect::from_min_max(pos2(item.left, content.top() + 1.0), pos2(item.right, content.bottom() - 1.0));
        if item.locked {
            continue;
        }
        let response = ui.interact(rect, Id::unique(("appbar_edit", &item.id)), Sense::click_and_drag());
        if response.contains_pointer() && !drag.as_ref().is_some_and(|d| d.active) {
            ui.ctx().set_cursor_icon(app_bar::drag_cursor(response.is_pointer_button_down_on()));
        }
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label_icon(app, &item.id).0));
        crate::scenario::publish_target(&format!("appbar:edit:{}", item.id), rect);
        if response.has_focus() && !drag.as_ref().is_some_and(|d| d.active) {
            let version = format!("v{}", crate::update::current_version());
            let visual = control(app, &item.id, &version);
            app_bar::paint_item_focus(ui, &item.id, app_bar::item_rect(rect, visual), app_bar::control_radius(visual));
        }
        if response.has_focus() {
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)) {
                run(app, &json!({"item":item.id,"action":"hide"}));
            }
            for (key, forward) in [(egui::Key::ArrowLeft, false), (egui::Key::ArrowRight, true)] {
                if ui.input_mut(|i| i.consume_key(egui::Modifiers::ALT, key)) {
                    let defaults = defaults(app);
                    app.toolbar_custom.app_bar.move_step(&item.id, forward, &defaults);
                }
            }
        }
        if ui.input(|i| i.pointer.primary_pressed())
            && response.contains_pointer()
            && let Some(p) = ui.input(|i| i.pointer.interact_pos())
        {
            response.request_focus();
            drag = Some(Drag {
                id: item.id.clone(),
                from_bar: true,
                origin: p,
                return_to: rect.center(),
                pointer: p,
                active: false,
                slot: None,
                below: false,
            });
        }
    }
    if let Some(held) = &mut drag {
        if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
            held.pointer = p;
            held.active |= app_bar_items::is_drag(p.x - held.origin.x, p.y - held.origin.y);
        }
        held.below = app_bar_items::drag_out(held.pointer.y, content.bottom());
        // The repeatable tray tile has no saved identity until placement. Exclude its
        // provisional preview object from the slot calculation just like a lifted item.
        let preview_id = if matches!(held.id.as_str(), "separator" | "space") {
            out.edit_items
                .iter()
                .find(|p| p.id.starts_with(&format!("{}:", held.id)) && !app.toolbar_custom.app_bar.items.iter().any(|s| s.id == p.id))
                .map(|p| p.id.as_str())
                .unwrap_or(&held.id)
        } else {
            &held.id
        };
        held.slot = zones(content, out).into_iter().find(|(_, rect)| rect.contains(held.pointer)).and_then(|(zone, _)| {
            // Select by the actual card, including when the tabs have collapsed.
            let centre = if zone == Zone::Start { f32::MAX } else { f32::MIN };
            app_bar_items::drop_slot(held.pointer.x, centre, &out.edit_items, preview_id)
        });
        if held.active
            && held.slot.is_some()
            && let Some(item) = out.edit_items.iter().find(|p| p.id == preview_id)
        {
            let version = format!("v{}", crate::update::current_version());
            let visual = control(app, &held.id, &version);
            let body = app_bar::item_rect(Rect::from_min_max(pos2(item.left, content.top()), pos2(item.right, content.bottom())), visual);
            app_bar::paint_item_outline(ui, preview_id, body, app_bar::control_radius(visual), 1.0);
        }
        let pass = ui.ctx().cumulative_pass_nr();
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            if held.active {
                spring(ui.ctx(), held);
                ui.ctx().data_mut(|d| d.insert_temp(Id::unique("appbar_cancelled_press"), true));
            } else {
                app.toolbar_custom.app_bar_customizing = false;
            }
            ui.ctx().data_mut(|d| d.insert_temp(Id::unique("appbar_consumed_release"), pass));
            drag = None;
        } else if ui.input(|i| i.pointer.primary_released()) {
            if held.active || !held.from_bar {
                ui.ctx().data_mut(|d| d.insert_temp(Id::unique("appbar_consumed_release"), pass));
            }
            if !held.active {
                if !held.from_bar {
                    add(app, &held.id);
                }
            } else {
                match app_bar_items::drag_release(&held.id, held.from_bar, held.below, held.slot.is_some(), false) {
                    DragRelease::Insert => {
                        if let Some(slot) = &held.slot {
                            let defaults = defaults(app);
                            if matches!(held.id.as_str(), "separator" | "space") {
                                app.toolbar_custom.app_bar.add_object(&held.id, slot.zone, slot.before.as_deref(), &defaults);
                            } else {
                                app.toolbar_custom.app_bar.move_before(&held.id, slot.zone, slot.before.as_deref(), &defaults);
                                app.toolbar_custom.app_bar.set_shown(&held.id, true, &defaults);
                            }
                        }
                    }
                    DragRelease::Remove => run(app, &json!({"item":held.id,"action":"hide"})),
                    DragRelease::Return => spring(ui.ctx(), held),
                }
            }
            drag = None;
            ui.ctx().request_repaint();
        } else if !ui.input(|i| i.pointer.primary_down()) {
            drag = None;
        }
    }
    let active = drag.as_ref().is_some_and(|d| d.active);
    app_bar::dragging(ui.ctx(), active);
    if active {
        ui.ctx().set_cursor_icon(app_bar::drag_cursor(true));
    }
    ui.ctx().data_mut(|d| match drag {
        Some(drag) => {
            d.insert_temp(drag_key(), drag);
        }
        None => d.remove::<Drag>(drag_key()),
    });
}
pub(crate) fn menu(app: &mut SolveApp, ui: &egui::Ui, content: Rect, out: &app_bar::BarOut) {
    if ui.input(|i| i.pointer.secondary_clicked())
        && !app_bar::tab_context_click_claimed(ui.ctx())
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
    if let Some(c) = id.strip_prefix("quick:").and_then(solidcraft_engine::find_command) {
        return (app.tr_command(c.id, c.label).into(), c.info(&app.session).icon.into());
    }
    let (key, icon) = match id {
        "documents" => ("parts_library.title", "document_browser"),
        "separator" => ("app_bar.separator", "help"),
        "space" => ("app_bar.space", "help"),
        "file" => ("ui.file", "new"),
        "home" => ("ui.start_page", "home"),
        "save" => ("ui.save", "save"),
        "undo" => ("ui.undo", "undo"),
        "redo" => ("ui.redo", "redo"),
        "assistant" => ("ui.assistant", "assistant"),
        "donate" => ("links.kofi_button", "kofi"),
        "discord" => ("links.discord_button", "discord"),
        "report" => ("ui.report_a_bug", "bug_report"),
        "settings" => ("ui.details_settings", "settings"),
        "help" => ("ui.help", "help"),
        "version" => ("app_bar.version", "help"),
        _ if id.starts_with("separator:") => ("app_bar.separator", "help"),
        _ if id.starts_with("space:") => ("app_bar.space", "help"),
        _ => return (id.into(), "help".into()),
    };
    (app.tr(key).into(), icon.into())
}
fn control<'a>(app: &'a SolveApp, id: &'a str, version: &'a str) -> Control<'a> {
    match id {
        "documents" => Control::Documents,
        "separator" => Control::Separator,
        "space" => Control::Space,
        _ if id.starts_with("separator:") => Control::Separator,
        _ if id.starts_with("space:") => Control::Space,
        "file" => Control::File { label: app.tr("ui.file"), enabled: true, open: false },
        "report" => Control::Report { label: app.tr("ui.report_a_bug"), labelled: true },
        "version" => Control::Version(version),
        "assistant" => Control::Assistant { state: crate::toolbar::assistant_state(app), open: app.agent_chat.open },
        "donate" | "discord" => {
            if let Some(link) = crate::links::by_id(if id == "donate" { "kofi" } else { id }) {
                Control::Pill {
                    data: app_bar::Pill {
                        id: link.id,
                        icon: link.icon,
                        label: if link.named { app.tr(link.button) } else { "" },
                        labelled: link.named,
                        tip: app.tr(link.tip),
                        hide_tip: app.tr("app_bar.hide_tip"),
                        name: app.tr(link.name),
                        hide_name: app.tr(link.hide_name),
                        fill: link.fill,
                        ink: solidcraft_theme::Tokens::get().on_accent,
                    },
                    lift: 0.0,
                    veil: 0.0,
                    focused: false,
                }
            } else {
                Control::Icon { icon: "help", size: 18.0, enabled: true }
            }
        }
        _ => {
            let icon = match id {
                "home" => "home",
                "save" => "save",
                "undo" => "undo",
                "redo" => "redo",
                "settings" => "settings",
                "help" => "help",
                _ => id.strip_prefix("quick:").and_then(solidcraft_engine::find_command).map_or("help", |c| c.info(&app.session).icon),
            };
            if id.starts_with("quick:") { Control::Command { icon, enabled: true } } else { Control::Icon { icon, size: 18.0, enabled: true } }
        }
    }
}
const TILE_WIDTH: f32 = 86.0;
const TILE_HEIGHT: f32 = 72.0;

fn tile(app: &mut SolveApp, ui: &mut egui::Ui, id: &str, version: &str) {
    let t = solidcraft_theme::Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(TILE_WIDTH, TILE_HEIGHT), Sense::hover());
    let response = ui.interact(rect, Id::unique(("appbar_tile", id)), Sense::click_and_drag());
    let cell = Rect::from_min_size(rect.min + vec2(0.0, t.space_6), vec2(TILE_WIDTH, 46.0));
    crate::scenario::publish_target(&format!("appbar:tile:{id}"), rect);
    crate::scenario::publish_rect(&format!("appbar:cell:{id}"), cell);
    let label = label_icon(app, id).0;
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    let drag = ui.ctx().data(|d| d.get_temp::<Drag>(drag_key()));
    let spring = ui.ctx().data(|d| d.get_temp::<Spring>(spring_key()));
    let returning = drag.as_ref().is_some_and(|d| d.active && d.from_bar && d.below && d.id == id);
    let empty = drag.as_ref().is_some_and(|d| d.active && !d.from_bar && d.id == id) || spring.as_ref().is_some_and(|s| !s.from_bar && s.id == id);
    let on_bar = app.toolbar_custom.app_bar.items.iter().any(|p| p.id == id && p.shown) && !returning;
    let placeable = !on_bar && !empty;
    if placeable && response.contains_pointer() && !app_bar::is_dragging(ui.ctx()) {
        ui.ctx().set_cursor_icon(app_bar::drag_cursor(response.is_pointer_button_down_on()));
    }
    app_bar::paint_hover(ui, &format!("tray:{id}"), cell, 9, placeable && response.contains_pointer(), |ui| {
        ui.painter().rect_filled(
            cell,
            9,
            app_bar::edit_item_background(t, control(app, id, version), placeable && response.contains_pointer() && !app_bar::is_dragging(ui.ctx())),
        );
        if !empty {
            if returning {
                ui.painter().rect_stroke(cell.expand(2.0), 11, Stroke::new(4.0, t.accent.gamma_multiply(0.15)), egui::StrokeKind::Outside);
            }
            ui.painter().rect_stroke(
                cell,
                9,
                Stroke::new(
                    1.0,
                    if returning {
                        t.accent
                    } else if id == "space" {
                        t.app_bar_text.gamma_multiply(0.24)
                    } else {
                        t.border
                    },
                ),
                egui::StrokeKind::Inside,
            );
            app_bar::preview_control(ui, cell.shrink(4.0), control(app, id, version), if on_bar { 0.5 } else { 1.0 }, false, false);
            if on_bar {
                let badge = app_bar::tray_badge(ui, cell);
                crate::scenario::publish_rect(&format!("appbar:badge:{id}"), badge);
                ui.interact(badge, ui.scope_id().with(("badge", id)), Sense::hover())
                    .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, app.tr("app_bar.on_bar")));
            }
        }
        if response.has_focus() {
            app_bar::paint_focus(ui.painter(), cell, 9);
        }
        let label_rect = Rect::from_min_max(pos2(cell.left(), cell.bottom() + t.space_4), pos2(cell.right(), rect.bottom()));
        crate::scenario::publish_rect(&format!("appbar:label:{id}"), label_rect);
        ui_kit::paint_text(ui, label_rect, &label, FontId::proportional(t.font_small), t.text, false);
    });
    if ui.input(|i| i.pointer.primary_pressed())
        && response.contains_pointer()
        && !on_bar
        && let Some(p) = ui.input(|i| i.pointer.interact_pos())
    {
        response.request_focus();
        ui.ctx().data_mut(|d| {
            d.insert_temp(
                drag_key(),
                Drag { id: id.into(), from_bar: false, origin: p, return_to: cell.center(), pointer: p, active: false, slot: None, below: true },
            )
        });
    }
    let consumed = ui.ctx().data(|d| d.get_temp::<u64>(Id::unique("appbar_consumed_release"))) == Some(ui.ctx().cumulative_pass_nr());
    let cancelled = ui.ctx().data(|d| d.get_temp::<bool>(Id::unique("appbar_cancelled_press"))).unwrap_or(false);
    if (!consumed && !cancelled && response.clicked())
        || (response.has_focus() && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)))
    {
        add(app, id);
        ui.ctx().request_repaint();
    }
}

fn header(app: &mut SolveApp, ui: &mut egui::Ui, width: f32) -> bool {
    let t = solidcraft_theme::Tokens::get();
    let options = [app.tr("app_bar.default"), app.tr("app_bar.minimal"), app.tr("app_bar.full"), app.tr("app_bar.compact"), app.tr("app_bar.custom")];
    let done_label = app.tr("app_bar.done");
    let done_width =
        ui.painter().layout_no_wrap(done_label.into(), FontId::proportional(t.font_label), t.on_accent).size().x + (t.space_12 + t.space_6) * 2.0;
    let segment_gap = t.space_8;
    let natural_presets: f32 = options
        .iter()
        .map(|label| ui.painter().layout_no_wrap((*label).into(), ui_kit::type_face::semibold(t.font_label), t.text).size().x.ceil() + t.space_16)
        .sum::<f32>()
        + segment_gap;
    let presets_width = natural_presets.min((width - done_width - t.space_24 - 166.0).max(80.0));
    let controls_width = presets_width + t.space_24 + done_width;
    let title_width = (width - controls_width - t.space_24 - t.space_2).max(0.0);
    let title_height = t.font_heading + t.space_4;
    let subtitle = app.tr("app_bar.subtitle");
    let subtitle_height = ui.painter().layout(subtitle.into(), FontId::proportional(t.font_small), t.form_dim, title_width).size().y;
    let height = (title_height + t.space_4 + subtitle_height + t.space_8).max(32.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let title = Rect::from_min_size(rect.min, vec2(title_width, title_height));
    ui_kit::paint_text(ui, title, app.tr("app_bar.title"), FontId::proportional(t.font_heading), t.text, false);
    crate::scenario::publish_rect("appbar:header:title", title);
    let sub = Rect::from_min_max(pos2(rect.left(), title.bottom() + t.space_4), pos2(title.right(), rect.bottom()));
    let mut sub_ui = ui.new_child(egui::UiBuilder::new().max_rect(sub).layout(egui::Layout::top_down(egui::Align::Min)));
    sub_ui.set_clip_rect(sub.intersect(ui.clip_rect()));
    ui_kit::text(&mut sub_ui, subtitle, ui_kit::Text::Note("appbar_subtitle"));
    let done_rect = Rect::from_center_size(pos2(rect.right() - done_width / 2.0, rect.center().y), vec2(done_width, 32.0));
    let mut done_ui =
        ui.new_child(egui::UiBuilder::new().id_salt("appbar_done").max_rect(done_rect).layout(egui::Layout::top_down(egui::Align::Min)));
    let done = app_bar::tray_done(&mut done_ui, done_rect, done_label);
    crate::scenario::publish_target("appbar:done", done.rect);
    let presets_rect =
        Rect::from_center_size(pos2(done_rect.left() - t.space_24 - presets_width / 2.0, rect.center().y), vec2(presets_width, ui_kit::row_height()));
    crate::scenario::publish_rect("appbar:header:presets", presets_rect);
    let values = [Preset::Default, Preset::Minimal, Preset::Full, Preset::Compact, Preset::Custom];
    let matched = app.toolbar_custom.app_bar.preset(&defaults(app));
    let mut selected = values.iter().position(|p| *p == matched).unwrap_or(4);
    // Both groups paint from one derived selection. Explicit states keep exactly one
    // highlight even in the click frame; Custom is an indicator and never changes the list.
    let states = values.map(|p| p == matched);
    let custom_width = options.last().map_or(0.0, |label| {
        ui.painter().layout_no_wrap((*label).into(), ui_kit::type_face::semibold(t.font_label), t.text).size().x.ceil() + t.space_16
    });
    let left = Rect::from_min_max(
        presets_rect.min,
        pos2((presets_rect.right() - custom_width - segment_gap).max(presets_rect.left()), presets_rect.bottom()),
    );
    let right = Rect::from_min_max(pos2(presets_rect.right() - custom_width, presets_rect.top()), presets_rect.max);
    let mut presets_ui =
        ui.new_child(egui::UiBuilder::new().id_salt("appbar_presets").max_rect(left).layout(egui::Layout::top_down(egui::Align::Min)));
    let changed = ui_kit::segmented(
        &mut presets_ui,
        &mut selected,
        options.get(..4).unwrap_or(&[]),
        ui_kit::Segments {
            intrinsic: true,
            states: states.get(..4).unwrap_or(&[]),
            targets: &["appbar:preset:Default", "appbar:preset:Minimal", "appbar:preset:Full", "appbar:preset:Compact"],
            ..Default::default()
        },
    );
    let mut custom_ui =
        ui.new_child(egui::UiBuilder::new().id_salt("appbar_custom_indicator").max_rect(right).layout(egui::Layout::top_down(egui::Align::Min)));
    let mut indicator = if matched == Preset::Custom { 0 } else { usize::MAX };
    ui_kit::segmented(
        &mut custom_ui,
        &mut indicator,
        options.get(4..).unwrap_or(&[]),
        ui_kit::Segments { intrinsic: true, states: states.get(4..).unwrap_or(&[]), targets: &["appbar:preset:Custom"], ..Default::default() },
    );
    if changed && let Some(value) = values.get(selected) {
        preset(app, *value);
    }
    done.clicked()
}

fn tiles_per_row(width: f32) -> usize {
    // Leave room for the six-pixel badge overlap and inward rounding at the column edge.
    ((width - 7.0 + solidcraft_theme::Tokens::get().space_8) / (TILE_WIDTH + solidcraft_theme::Tokens::get().space_8)).floor().max(1.0) as usize
}

fn section_height(width: f32, count: usize) -> f32 {
    let t = solidcraft_theme::Tokens::get();
    let rows = count.div_ceil(tiles_per_row(width));
    (t.control_height + t.space_14 + rows as f32 * TILE_HEIGHT + rows.saturating_sub(1) as f32 * t.space_14).ceil() + 1.0
}

fn section(app: &mut SolveApp, ui: &mut egui::Ui, width: f32, key: &str, ids: &[&str], version: &str) {
    if ids.is_empty() {
        return;
    }
    let t = solidcraft_theme::Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(width, section_height(width, ids.len())), Sense::hover());
    crate::scenario::publish_rect(&format!("appbar:section:{key}"), rect);
    let heading = Rect::from_min_size(rect.min, vec2(width, t.control_height));
    ui_kit::paint_text(ui, heading, app.tr(key), FontId::proportional(t.font_button), t.text, false);
    let per_row = tiles_per_row(width);
    for (i, id) in ids.iter().enumerate() {
        let at = rect.min
            + vec2(
                (i % per_row) as f32 * (TILE_WIDTH + t.space_8),
                t.control_height + t.space_14 + (i / per_row) as f32 * (TILE_HEIGHT + t.space_14),
            );
        // egui quantises allocated widget origins; round inward so a fractional column
        // boundary cannot put its first tile just outside the column's clip rectangle.
        let at = pos2(at.x.ceil(), at.y.ceil());
        let tile_rect = Rect::from_min_size(at, vec2(TILE_WIDTH, TILE_HEIGHT));
        let mut tile_ui = ui.new_child(egui::UiBuilder::new().id_salt(id).max_rect(tile_rect).layout(egui::Layout::top_down(egui::Align::Min)));
        tile_ui.set_clip_rect(rect.intersect(ui.clip_rect()));
        tile(app, &mut tile_ui, id, version);
    }
}

fn body(app: &mut SolveApp, ui: &mut egui::Ui, width: f32, version: &str) {
    let t = solidcraft_theme::Tokens::get();
    let gap = t.space_24 + t.space_2;
    let sections = [
        ("app_bar.commands", &["documents", "home", "save", "undo", "redo", "assistant", "settings", "help"][..]),
        ("app_bar.links", &["report", "donate", "discord", "version"][..]),
        ("app_bar.layout", &["separator", "space"][..]),
    ];
    let quick: Vec<_> =
        app.toolbar_custom.quick_access.iter().filter(|id| solidcraft_engine::find_command(id).is_some()).map(|id| format!("quick:{id}")).collect();
    let quick_ids: Vec<_> = quick.iter().map(String::as_str).collect();
    // At narrow window sizes sections stack; a tile never shrinks below its cell width.
    if width < (TILE_WIDTH + 7.0) * 3.0 + gap * 2.0 {
        for (key, ids) in sections {
            section(app, ui, width, key, ids, version);
            ui.add_space(t.space_14);
        }
        section(app, ui, width, "app_bar.quick", &quick_ids, version);
        return;
    }
    let column_width = (width - gap * 2.0) / 3.0;
    let height = sections
        .iter()
        .map(|(key, ids)| {
            section_height(column_width, ids.len())
                + if *key == "app_bar.layout" && !quick_ids.is_empty() { t.space_14 + section_height(column_width, quick_ids.len()) } else { 0.0 }
        })
        .fold(0.0_f32, f32::max);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    for (i, (key, ids)) in sections.iter().enumerate() {
        let column = Rect::from_min_size(rect.min + vec2(i as f32 * (column_width + gap), 0.0), vec2(column_width, height));
        crate::scenario::publish_rect(&format!("appbar:column:{i}"), column);
        let mut column_ui = ui.new_child(egui::UiBuilder::new().id_salt(key).max_rect(column).layout(egui::Layout::top_down(egui::Align::Min)));
        column_ui.set_clip_rect(column.intersect(ui.clip_rect()));
        section(app, &mut column_ui, column_width, key, ids, version);
        if *key == "app_bar.layout" && !quick_ids.is_empty() {
            column_ui.add_space(t.space_14);
            section(app, &mut column_ui, column_width, "app_bar.quick", &quick_ids, version);
        }
    }
}

fn divider(ui: &mut egui::Ui, width: f32) {
    let t = solidcraft_theme::Tokens::get();
    ui.add_space(t.space_8);
    let (line, _) = ui.allocate_exact_size(vec2(width, 1.0), Sense::hover());
    ui.painter().hline(line.x_range(), line.center().y, Stroke::new(1.0, t.border));
    ui.add_space(t.space_8);
}

fn footer(app: &mut SolveApp, ui: &mut egui::Ui, width: f32) {
    let t = solidcraft_theme::Tokens::get();
    let font = FontId::proportional(t.font_small);
    let measure = |s: &str| ui.painter().layout_no_wrap(s.into(), font.clone(), t.text).size().x;
    let reset_width = measure(app.tr("app_bar.reset"));
    let check_width = measure(app.tr("app_bar.check_hint"));
    let keys_width = measure(app.tr("app_bar.keyboard_hint"));
    let (row, _) = ui.allocate_exact_size(vec2(width, 32.0), Sense::hover());
    let flank = reset_width.max(check_width).min(width / 3.0);
    let reset_rect = Rect::from_min_max(row.min, pos2(row.left() + reset_width.min(flank), row.bottom()));
    let reset = app_bar::tray_link(ui, reset_rect, app.tr("app_bar.reset"));
    crate::scenario::publish_target("appbar:reset", reset.rect);
    if reset.clicked() {
        app.start("ui.appBarReset");
    }
    let keys_rect = Rect::from_center_size(row.center(), vec2(keys_width.min((width - flank * 2.0 - t.space_16).max(0.0)), row.height()));
    let check_rect = Rect::from_min_max(pos2(row.right() - check_width.min(flank), row.top()), row.max);
    ui_kit::paint_text(ui, keys_rect, app.tr("app_bar.keyboard_hint"), font.clone(), t.form_dim, false);
    ui_kit::paint_text(ui, check_rect, app.tr("app_bar.check_hint"), font, t.form_dim, false);
    crate::scenario::publish_rect("appbar:footer:keys", keys_rect);
    crate::scenario::publish_rect("appbar:footer:check", check_rect);
}

/// The early foreground host shields the app before its children process pointer input.
pub(crate) fn shield(app: &SolveApp, ctx: &egui::Context) {
    let fade = ctx.animate_bool_with_time(Id::unique("sc_appbar_edit_fade"), app.toolbar_custom.app_bar_customizing, 0.15);
    if fade <= 0.0 {
        return;
    }
    let screen = ctx.content_rect();
    let below =
        Rect::from_min_max(pos2(screen.left(), screen.top() + app_bar::height(app.toolbar_custom.app_bar_customizing)), screen.right_bottom());
    Container::new(ctx, Surface::InputShield)
        .area(Id::unique("sc_appbar_edit_scrim"))
        .order(egui::Order::Foreground)
        .fixed_pos(below.min)
        .movable(false)
        .interactable(app.toolbar_custom.app_bar_customizing)
        .show(ctx, |ui| {
            let (_, response) = ui.allocate_exact_size(below.size(), Sense::CLICK | Sense::DRAG);
            if response.clicked() {
                response.surrender_focus();
            }
            let t = solidcraft_theme::Tokens::get();
            ui.painter().rect_filled(below, 0, t.overlay.lerp_to_gamma(t.figure_shadow, 0.85).to_opaque().gamma_multiply(0.7 * fade));
        });
}
pub(crate) fn show(app: &mut SolveApp, ctx: &egui::Context) {
    cancel_or_exit(app, ctx);
    if !app.toolbar_custom.app_bar_customizing {
        ctx.data_mut(|d| {
            d.remove::<Drag>(drag_key());
            d.remove::<Spring>(spring_key());
        });
        return;
    }
    let t = solidcraft_theme::Tokens::get();
    let screen = ctx.content_rect();
    let width = 1040.0_f32.min((screen.width() - t.space_12 * 2.0).max(120.0));
    let at = pos2(screen.center().x - width / 2.0, screen.top() + app_bar::height(true) + t.space_12);
    let host = Container::floating_card(Surface::Panel);
    let removing =
        ctx.data(|d| d.get_temp::<Drag>(drag_key())).is_some_and(|d| d.active && d.from_bar && d.below && app_bar_items::can_remove(&d.id));
    let mut done = false;
    let shown = host
        .area(Id::unique("sc_appbar_tray"))
        .order(egui::Order::Foreground)
        .fixed_pos(at)
        .default_size(vec2(width, 0.0))
        .movable(false)
        .fade_in(false)
        .show(ctx, |ui| {
            ui.set_width(width);
            ui.set_opacity(1.0);
            let frame = host.frame(ctx, t.panel, ui_kit::frame::Side::Left).inner_margin(t.space_12);
            let content_width = (width - (t.space_12 + frame.stroke.width) * 2.0).max(TILE_WIDTH);
            let card = Container::show_backing(ui, frame, |ui| {
                ui.set_width(content_width);
                done |= header(app, ui, content_width);
                divider(ui, content_width);
                let version = format!("v{}", crate::update::current_version());
                egui::ScrollArea::vertical().max_height((screen.height() - 250.0).max(80.0)).show(ui, |ui| {
                    // Reserve scrollbar space within the panel rather than letting it widen it.
                    let body_width = ui.available_width().min(content_width);
                    body(app, ui, body_width, &version);
                });
                divider(ui, content_width);
                footer(app, ui, content_width);
                if removing {
                    ui_kit::assistant_badge(ui, app.tr("app_bar.release"), ui_kit::AssistantTone::Working);
                }
            });
            crate::scenario::publish_rect("appbar:tray", card.response.rect);
        });
    // A scrim click may raise its area. Keep the card directly above it on every pass,
    // independently of area creation and click order.
    ctx.set_sublayer(egui::LayerId::new(egui::Order::Foreground, Id::unique("sc_appbar_edit_scrim")), shown.response.layer_id);
    if done {
        app.toolbar_custom.app_bar_customizing = false;
        ctx.request_repaint();
    }
    let drag = ctx.data(|d| d.get_temp::<Drag>(drag_key()));
    let mut returning = ctx.data(|d| d.get_temp::<Spring>(spring_key()));
    let lifted = if let Some(drag) = drag.filter(|d| d.active) {
        if drag.slot.is_none() {
            Some((drag.id, drag.pointer, drag.from_bar && drag.below))
        } else {
            ctx.request_repaint();
            None
        }
    } else if let Some(s) = &returning {
        let progress = ((ctx.input(|i| i.time) - s.started) / 0.15).clamp(0.0, 1.0) as f32;
        let result = Some((s.id.clone(), s.from.lerp(s.to, 1.0 - (1.0 - progress).powi(3)), false));
        if progress >= 1.0 {
            returning = None;
        }
        ctx.request_repaint();
        result
    } else {
        None
    };
    ctx.data_mut(|d| match returning {
        Some(s) => {
            d.insert_temp(spring_key(), s);
        }
        None => d.remove::<Spring>(spring_key()),
    });
    if let Some((id, pointer, outside)) = lifted {
        if app_bar::is_dragging(ctx) {
            ctx.set_cursor_icon(app_bar::drag_cursor(true));
        }
        let warning = id == "file" && outside;
        let version = format!("v{}", crate::update::current_version());
        let visual = control(app, &id, &version);
        let size = app_bar::control_size(ctx, visual);
        Container::new(ctx, Surface::Canvas)
            .area(Id::unique("sc_appbar_lifted"))
            .order(egui::Order::Tooltip)
            .fixed_pos(pointer - size / 2.0)
            .interactable(false)
            .show(ctx, |ui| {
                let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                app_bar::preview_control(ui, rect, visual, 1.0, true, warning);
            });
        if warning {
            let host = Container::new(ctx, Surface::Popup);
            host.area(Id::unique("sc_appbar_file_tip")).order(egui::Order::Tooltip).fixed_pos(pointer + vec2(16.0, 58.0)).interactable(false).show(
                ctx,
                |ui| {
                    Container::show_backing(ui, host.frame(ctx, t.dialog_bg, ui_kit::frame::Side::Left).inner_margin(t.space_8), |ui| {
                        ui.set_max_width(320.0);
                        ui_kit::text(ui, app.tr("app_bar.file_required"), ui_kit::Text::Note("appbar_file_required"));
                    });
                },
            );
        }
    }
}
