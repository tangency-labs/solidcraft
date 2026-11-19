//! App-bar edit mode. The tray, drag previews and fade are transient; only the item list is saved.
use crate::ui_kit::{self, Container, Item, Surface};
use crate::{SolveApp, context_menu};
use egui::{Align2, FontId, Id, Pos2, Rect, Sense, Stroke, pos2, vec2};
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
    if app.toolbar_custom.app_bar.items.iter().any(|p| p.id == id && p.shown) {
        return;
    }
    let defaults = defaults(app);
    app.toolbar_custom.app_bar.move_before(id, Zone::End, None, &defaults);
    app.toolbar_custom.app_bar.set_shown(id, true, &defaults);
}

/// Reserve the lifted item's measured width in a temporary layout. The shared painter leaves
/// that control empty, making a real slot and moving neighbours without saving the preview.
pub(crate) fn preview(app: &SolveApp, ctx: &egui::Context) -> Layout {
    let mut layout = app.toolbar_custom.app_bar.clone();
    let drag = ctx.data(|d| d.get_temp::<Drag>(drag_key()));
    let spring = ctx.data(|d| d.get_temp::<Spring>(spring_key()));
    let lifted = if app.toolbar_custom.app_bar_customizing {
        drag.as_ref().filter(|d| d.active).map(|d| d.id.clone()).or_else(|| spring.as_ref().filter(|s| s.from_bar).map(|s| s.id.clone()))
    } else {
        None
    };
    app_bar::editing(ctx, app.toolbar_custom.app_bar_customizing, lifted);
    if let Some(drag) = drag.filter(|d| d.active && app.toolbar_custom.app_bar_customizing) {
        let defaults = defaults(app);
        if let Some(slot) = drag.slot {
            layout.move_before(&drag.id, slot.zone, slot.before.as_deref(), &defaults);
            layout.set_shown(&drag.id, true, &defaults);
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
fn dashed(p: &egui::Painter, r: Rect, ink: egui::Color32) {
    p.extend(egui::Shape::dashed_line(
        &[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()],
        Stroke::new(1.0, ink),
        4.0,
        3.0,
    ));
}

pub(crate) fn bar(app: &mut SolveApp, ui: &mut egui::Ui, content: Rect, out: &app_bar::BarOut) {
    if !app.toolbar_custom.app_bar_customizing {
        return;
    }
    let t = solidcraft_theme::Tokens::get();
    if ui.input(|i| i.pointer.primary_pressed()) {
        ui.ctx().data_mut(|d| d.remove::<bool>(Id::unique("appbar_cancelled_press")));
    }
    let mut drag = ui.ctx().data(|d| d.get_temp::<Drag>(drag_key()));
    for item in &out.edit_items {
        let rect = Rect::from_min_max(pos2(item.left, content.top() + 1.0), pos2(item.right, content.bottom() - 1.0));
        if item.locked {
            ui.painter().rect_filled(rect, 0, t.app_bar.gamma_multiply(0.45));
            continue;
        }
        let response = ui.interact(rect, Id::unique(("appbar_edit", &item.id)), Sense::click_and_drag());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label_icon(app, &item.id).0));
        crate::scenario::publish_target(&format!("appbar:edit:{}", item.id), rect);
        dashed(ui.painter(), rect.shrink(1.0), t.app_bar_text.gamma_multiply(0.5));
        if response.has_focus() {
            ui.painter().rect_stroke(rect, t.radius_sm, Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
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
    ui.painter().hline(content.x_range(), content.bottom() - 1.0, Stroke::new(1.0, t.accent));
    for (end, left, right) in [(false, content.left(), out.tabs_start), (true, out.tabs_end, content.right())] {
        if right > left {
            let rect = Rect::from_min_max(pos2(left, content.top()), pos2(right, content.bottom()));
            ui.painter().rect_filled(rect, 0, t.accent.gamma_multiply(0.055));
            ui.ctx().data_mut(|d| d.insert_temp(Id::unique(("appbar_zone", end)), rect));
        }
    }
    if let Some(held) = &mut drag {
        if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
            held.pointer = p;
            held.active |= app_bar_items::is_drag(p.x - held.origin.x, p.y - held.origin.y);
        }
        held.below = app_bar_items::drag_out(held.pointer.y, content.bottom());
        held.slot = content
            .contains(held.pointer)
            .then(|| app_bar_items::drop_slot(held.pointer.x, (out.tabs_start + out.tabs_end) / 2.0, &out.edit_items, &held.id))
            .flatten();
        if held.active
            && let Some(slot) = &held.slot
        {
            let gap = out
                .edit_items
                .iter()
                .find(|p| p.id == held.id)
                .map_or(Rect::from_min_size(pos2(slot.x, content.top() + 2.0), vec2(32.0, content.height() - 4.0)), |p| {
                    Rect::from_min_max(pos2(p.left, content.top() + 2.0), pos2(p.right, content.bottom() - 2.0))
                });
            dashed(ui.painter(), gap, t.accent);
            ui_kit::insertion_marker(ui.painter(), gap.left(), gap.top(), gap.bottom());
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
            if held.active {
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
                            app.toolbar_custom.app_bar.move_before(&held.id, slot.zone, slot.before.as_deref(), &defaults);
                            app.toolbar_custom.app_bar.set_shown(&held.id, true, &defaults);
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
    ui.ctx().data_mut(|d| match drag {
        Some(drag) => {
            d.insert_temp(drag_key(), drag);
        }
        None => d.remove::<Drag>(drag_key()),
    });
}
pub(crate) fn menu(app: &mut SolveApp, ui: &egui::Ui, content: Rect, out: &app_bar::BarOut) {
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
    if let Some(c) = id.strip_prefix("quick:").and_then(solidcraft_engine::find_command) {
        return (app.tr_label(c.label).into(), c.info(&app.session).icon.into());
    }
    let (key, icon) = match id {
        "file" => ("ui.file", "new"),
        "home" => ("ui.start_page", "home"),
        "save" => ("ui.save", "save"),
        "undo" => ("ui.undo", "undo"),
        "redo" => ("ui.redo", "redo"),
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
fn control<'a>(app: &'a SolveApp, id: &'a str, version: &'a str) -> Control<'a> {
    match id {
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
fn tile(app: &mut SolveApp, ui: &mut egui::Ui, id: &str, version: &str) {
    let t = solidcraft_theme::Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(82.0, 76.0), Sense::hover());
    let response = ui.interact(rect, Id::unique(("appbar_tile", id)), Sense::click_and_drag());
    let cell = Rect::from_min_size(rect.min, vec2(74.0, 46.0));
    crate::scenario::publish_target(&format!("appbar:tile:{id}"), rect);
    let label = label_icon(app, id).0;
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    let drag = ui.ctx().data(|d| d.get_temp::<Drag>(drag_key()));
    let spring = ui.ctx().data(|d| d.get_temp::<Spring>(spring_key()));
    let returning = drag.as_ref().is_some_and(|d| d.active && d.from_bar && d.below && d.id == id);
    let empty = drag.as_ref().is_some_and(|d| d.active && !d.from_bar && d.id == id) || spring.as_ref().is_some_and(|s| !s.from_bar && s.id == id);
    let on_bar = app.toolbar_custom.app_bar.items.iter().any(|p| p.id == id && p.shown) && !returning;
    ui.painter().rect_filled(cell, t.radius_sm, t.app_bar);
    if empty {
        dashed(ui.painter(), cell.shrink(1.0), t.border);
    } else {
        if returning {
            ui.painter().rect_stroke(cell.expand(2.0), t.radius_sm, Stroke::new(4.0, t.accent.gamma_multiply(0.15)), egui::StrokeKind::Outside);
        }
        ui.painter().rect_stroke(
            cell,
            t.radius_sm,
            Stroke::new(1.0, if returning || response.has_focus() { t.accent } else { t.border }),
            egui::StrokeKind::Inside,
        );
        app_bar::preview_control(ui, cell.shrink(4.0), control(app, id, version), if on_bar { 0.5 } else { 1.0 }, false, false);
        if on_bar || id == "file" {
            let badge = Rect::from_center_size(cell.right_top() + vec2(-7.0, 7.0), vec2(14.0, 14.0));
            ui.painter().circle_filled(badge.center(), 7.0, t.accent);
            ui_kit::icons::paint_ink(ui.painter(), badge.shrink(2.0), if id == "file" { "lock" } else { "check" }, t.on_accent);
            ui.interact(badge, ui.scope_id().with(("badge", id)), Sense::hover()).widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::Image, true, app.tr(if id == "file" { "app_bar.locked" } else { "app_bar.on_bar" }))
            });
        }
    }
    ui_kit::paint_text(
        ui,
        Rect::from_min_max(pos2(rect.left(), cell.bottom() + t.space_4), rect.right_bottom()),
        &label,
        FontId::proportional(t.font_small),
        t.text,
        false,
    );
    if ui.input(|i| i.pointer.primary_pressed())
        && response.contains_pointer()
        && !on_bar
        && id != "file"
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

/// The early foreground host shields the app before its children process pointer input.
pub(crate) fn shield(app: &SolveApp, ctx: &egui::Context) {
    let fade = ctx.animate_bool_with_time(Id::unique("sc_appbar_edit_fade"), app.toolbar_custom.app_bar_customizing, 0.15);
    if fade <= 0.0 {
        return;
    }
    let screen = ctx.content_rect();
    let below = Rect::from_min_max(pos2(screen.left(), screen.top() + app_bar::HEIGHT), screen.right_bottom());
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
            for (end, key) in [(false, "ui.start"), (true, "ui.end")] {
                if let Some(rect) = ctx.data(|d| d.get_temp::<Rect>(Id::unique(("appbar_zone", end)))) {
                    let p = ui.painter().with_clip_rect(screen);
                    let at = pos2(rect.center().x, below.top());
                    p.rect_filled(Rect::from_center_size(at, vec2(48.0, 16.0)), t.radius_sm, t.app_bar);
                    p.text(at, Align2::CENTER_CENTER, app.tr(key).to_uppercase(), FontId::proportional(t.font_small), t.accent.gamma_multiply(fade));
                }
            }
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
    let at = pos2(screen.center().x - width / 2.0, screen.top() + app_bar::HEIGHT + t.space_12);
    let host = Container::new(ctx, Surface::Window);
    let removing =
        ctx.data(|d| d.get_temp::<Drag>(drag_key())).is_some_and(|d| d.active && d.from_bar && d.below && app_bar_items::can_remove(&d.id));
    let mut done = false;
    host.area(Id::unique("sc_appbar_tray")).order(egui::Order::Foreground).fixed_pos(at).movable(false).show(ctx, |ui| {
        let frame = host.frame(ctx, t.dialog_bg, ui_kit::frame::Side::Left).inner_margin(t.space_12);
        let card = Container::show_backing(ui, frame, |ui| {
            ui.set_width((width - t.space_12 * 2.0).max(0.0));
            ui.horizontal_wrapped(|ui| {
                ui.vertical(|ui| {
                    ui.set_width((width - 490.0).max(120.0));
                    ui_kit::text(ui, app.tr("app_bar.title"), ui_kit::Text::Heading("appbar_tray"));
                    ui_kit::text(ui, app.tr("app_bar.subtitle"), ui_kit::Text::Note("appbar_subtitle"));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    let response = ui_kit::button(ui, app.tr("app_bar.done"), ui_kit::Button::Primary);
                    crate::scenario::publish_target("appbar:done", response.rect);
                    done |= response.clicked();
                    let mut selected = ctx.data(|d| d.get_temp::<usize>(Id::unique("appbar_preset"))).unwrap_or(0);
                    let options = [app.tr("app_bar.default"), app.tr("app_bar.minimal"), app.tr("app_bar.full"), app.tr("app_bar.compact")];
                    if ui_kit::segmented(
                        ui,
                        &mut selected,
                        &options,
                        ui_kit::Segments {
                            intrinsic: true,
                            targets: &["appbar:preset:Default", "appbar:preset:Minimal", "appbar:preset:Full", "appbar:preset:Compact"],
                            ..Default::default()
                        },
                    ) {
                        if let Some(value) = [Preset::Default, Preset::Minimal, Preset::Full, Preset::Compact].get(selected) {
                            preset(app, *value);
                        }
                        ctx.data_mut(|d| d.insert_temp(Id::unique("appbar_preset"), selected));
                    }
                });
            });
            ui.separator();
            let version = format!("v{}", crate::update::current_version());
            egui::ScrollArea::vertical().max_height((screen.height() - 260.0).max(80.0)).show(ui, |ui| {
                ui.columns(3, |columns| {
                    for (column, (key, ids)) in columns.iter_mut().zip([
                        ("app_bar.commands", &["file", "home", "save", "undo", "redo"][..]),
                        ("app_bar.status", &["assistant", "report", "settings", "help"][..]),
                        ("app_bar.community", &["donate", "discord", "version"][..]),
                    ]) {
                        ui_kit::text(column, app.tr(key), ui_kit::Text::Heading(key));
                        column.horizontal_wrapped(|ui| {
                            for id in ids {
                                ui.push_id(id, |ui| tile(app, ui, id, &version));
                            }
                        });
                    }
                });
                let quick: Vec<_> = app
                    .toolbar_custom
                    .quick_access
                    .iter()
                    .filter(|id| solidcraft_engine::find_command(id).is_some())
                    .map(|id| format!("quick:{id}"))
                    .collect();
                if !quick.is_empty() {
                    ui_kit::text(ui, app.tr("app_bar.quick"), ui_kit::Text::Heading("appbar_quick"));
                    ui.horizontal_wrapped(|ui| {
                        for id in &quick {
                            ui.push_id(id, |ui| tile(app, ui, id, &version));
                        }
                    });
                }
            });
            ui.separator();
            let reset = ui_kit::button(ui, app.tr("app_bar.reset"), ui_kit::Button::Ghost);
            crate::scenario::publish_target("appbar:reset", reset.rect);
            if reset.clicked() {
                app.start("ui.appBarReset");
            }
            ui_kit::text(ui, app.tr("app_bar.keyboard_hint"), ui_kit::Text::Note("appbar_keys"));
            ui_kit::text(ui, app.tr("app_bar.check_hint"), ui_kit::Text::Note("appbar_check_hint"));
            if removing {
                ui_kit::assistant_badge(ui, app.tr("app_bar.release"), ui_kit::AssistantTone::Working);
            }
        });
        crate::scenario::publish_rect("appbar:tray", card.response.rect);
        if removing {
            dashed(ui.painter(), card.response.rect.expand(2.0), t.accent);
        }
    });
    if done {
        app.toolbar_custom.app_bar_customizing = false;
        ctx.request_repaint();
    }
    let drag = ctx.data(|d| d.get_temp::<Drag>(drag_key()));
    let mut returning = ctx.data(|d| d.get_temp::<Spring>(spring_key()));
    let lifted = if let Some(drag) = drag.filter(|d| d.active) {
        Some((drag.id, drag.pointer, drag.from_bar && drag.below))
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
