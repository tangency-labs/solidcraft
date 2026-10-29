//! Measured single-row navigation bar. Commands own all view changes.
use crate::navigation::NavCaps;
use crate::navigation_bar::{self, Canvas, Edge, Idle, Kind, Labels, MenuDisplay, NavigationBar, Position, Slot};
use crate::theme::Tokens;
use crate::ui_kit::{self, Container, Surface};
use crate::{SolveApp, viewport::NavMode};
use egui::{Align2, Rect, pos2, vec2};
use serde_json::json;

pub(crate) struct BarLayout {
    pub rect: Rect,
    pub slots: Vec<(&'static Slot, Rect)>,
    pub overflow: Vec<&'static Slot>,
    pub more: Rect,
    pub labels: bool,
    pub values: bool,
}

/// Drop menu values, then labels, before moving a suffix into More. All rectangles use logical UI points.
pub(crate) fn measure(content: Rect, prefs: &NavigationBar, canvas: impl Into<Canvas>, label_width: impl Fn(&Slot) -> f32) -> BarLayout {
    measure_row(content, prefs, canvas.into(), label_width, false)
}

fn measure_row(content: Rect, prefs: &NavigationBar, canvas: Canvas, label_width: impl Fn(&Slot) -> f32, editing: bool) -> BarLayout {
    use ui_kit::*;
    let metrics = NavigationMetrics::for_size(prefs.size);
    let button = prefs.size.points();
    let inset = NAV_BORDER + NAV_PADDING;
    let mut result = BarLayout { rect: Rect::NOTHING, slots: Vec::new(), overflow: Vec::new(), more: Rect::NOTHING, labels: false, values: true };
    let height = metrics.height();
    let available = content.width() - 2.0 * NAV_EDGE;
    if !content.is_finite() || available < button + 2.0 * inset || content.height() < height + NAV_BOTTOM {
        return result;
    }
    let slots: Vec<_> =
        prefs.slots.iter().take(navigation_bar::MAX_SLOTS).filter_map(|id| navigation_bar::find(id)).filter(|slot| slot.available(canvas)).collect();
    if slots.is_empty() {
        return result;
    }
    let width = |slot: &Slot, labels, values| {
        if slot.kind == Kind::Separator {
            NAV_SEPARATOR
        } else if slot.kind == Kind::Menu {
            metrics.icon
                + if editing { 2.0 * NAV_BORDER } else { 0.0 }
                + NAV_MENU_LEFT
                + NAV_MENU_RIGHT
                + NAV_CHEVRON
                + NAV_MENU_GAP
                + if values && slot.shows_value() && prefs.menu_display(slot.id) == MenuDisplay::IconAndValue {
                    label_width(slot).max(0.0) + NAV_MENU_GAP
                } else {
                    0.0
                }
        } else if labels {
            (metrics.icon + NAV_LABEL_GAP + label_width(slot).max(0.0) + 2.0 * NAV_LABEL_PADDING).max(metrics.button)
        } else {
            metrics.button
        }
    };
    let total = |labels, values| {
        slots.iter().map(|slot| width(slot, labels, values)).sum::<f32>() + NAV_GAP * slots.len().saturating_sub(1) as f32 + 2.0 * inset
    };
    result.labels = prefs.labels == Labels::Labels;
    if total(result.labels, true) > available {
        result.values = false;
    }
    if total(result.labels, result.values) > available {
        result.labels = false;
    }
    let mut visible = slots.len();
    let mut row_width = total(result.labels, result.values);
    if row_width > available {
        visible = 0;
        row_width = metrics.button + 2.0 * inset;
        for slot in &slots {
            let next = row_width + width(slot, false, false) + NAV_GAP;
            if next > available {
                break;
            }
            row_width = next;
            visible += 1;
        }
        result.overflow.extend(slots.iter().skip(visible).copied());
    }
    let left = match prefs.position {
        Position::Centre => content.center().x - row_width / 2.0,
        Position::Left => content.left() + NAV_EDGE,
        Position::Right => content.right() - NAV_EDGE - row_width,
    };
    // The same gap from either edge of the view.
    let top = match prefs.edge {
        navigation_bar::Edge::Bottom => content.bottom() - height - NAV_BOTTOM,
        navigation_bar::Edge::Top => content.top() + NAV_BOTTOM,
    };
    result.rect = Rect::from_min_size(pos2(left, top), vec2(row_width, height));
    let mut x = left + inset;
    for slot in slots.iter().take(visible) {
        let w = width(slot, result.labels, result.values);
        result.slots.push((*slot, Rect::from_min_size(pos2(x, result.rect.top() + inset), vec2(w, metrics.button))));
        x += w + NAV_GAP;
    }
    if !result.overflow.is_empty() {
        result.more = Rect::from_min_size(pos2(x, result.rect.top() + inset), vec2(metrics.button, metrics.button));
    }
    result
}

/// Editing retains every stored catalogue slot, including those hidden by the preview canvas.
pub(crate) fn editor_layout(stage: Rect, prefs: &NavigationBar, label_width: impl Fn(&Slot) -> f32) -> BarLayout {
    let virtual_canvas = Rect::from_min_size(stage.min, vec2(8192.0, ui_kit::NAV_STAGE_HEIGHT));
    let mut bar = measure_row(virtual_canvas, prefs, NavCaps::MODEL.into(), label_width, true);
    if !bar.rect.is_positive() {
        let metrics = ui_kit::NavigationMetrics::for_size(prefs.size);
        bar.rect = Rect::from_min_size(stage.min, vec2(metrics.button + 2.0 * (ui_kit::NAV_PADDING + ui_kit::NAV_BORDER), metrics.height()));
    }
    let x = match prefs.position {
        Position::Left => stage.left() + ui_kit::NAV_BORDER + ui_kit::NAV_STAGE_INSET,
        Position::Centre => stage.center().x - bar.rect.width() / 2.0,
        Position::Right => stage.right() - ui_kit::NAV_BORDER - ui_kit::NAV_STAGE_INSET - bar.rect.width(),
    };
    let delta = pos2(x, stage.center().y - bar.rect.height() / 2.0) - bar.rect.min;
    bar.rect = bar.rect.translate(delta);
    for (_, rect) in &mut bar.slots {
        *rect = rect.translate(delta);
    }
    if bar.more.is_positive() {
        bar.more = bar.more.translate(delta);
    }
    bar
}

pub(crate) fn measured(app: &SolveApp, ctx: &egui::Context, content: Rect) -> BarLayout {
    let t = Tokens::get();
    measure(content, &app.preferences.navigation_bar, navigation_bar::canvas(app), |slot| {
        ctx.fonts_mut(|fonts| fonts.layout_no_wrap(slot_text(app, slot).to_owned(), crate::fonts::medium(ui_kit::NAV_TEXT), t.form_ink).size().x)
    })
}

fn selected(app: &SolveApp, id: &str) -> bool {
    match id {
        "orbit" => app.viewport.nav == Some(NavMode::Orbit),
        "pan" => app.viewport.nav == Some(NavMode::Pan),
        "zoom" => app.viewport.nav == Some(NavMode::Zoom),
        "spin" => app.viewport.spin,
        "projection" => app.perspective_view(),
        "grid" => app.ui.show_grid,
        "ground_shadow" => app.ui.ground_shadow,
        "ambient_occlusion" => app.preferences.ambient_occlusion,
        _ => false,
    }
}

/// A sketch is always drawn orthographic, so the projection controls stay inert until it ends (#2144).
fn projection_locked(app: &SolveApp) -> bool {
    app.session.active_sketch.is_some()
}

/// #6143 will replace this single compatibility path with a registered command.
fn toggle_projection(app: &mut SolveApp) {
    if !projection_locked(app) {
        app.ui.perspective = !app.ui.perspective;
    }
}

fn activate(app: &mut SolveApp, slot: &Slot) {
    match slot.id {
        "look_at" => crate::viewport::look_at_selection(app),
        "projection" => toggle_projection(app),
        _ => {
            if let Some(command) = slot.command {
                let params = match slot.kind {
                    Kind::Tool => json!({"tool": if selected(app, slot.id) { "none" } else { slot.id }}),
                    _ if slot.id == "ambient_occlusion" => json!({"enabled": !app.preferences.ambient_occlusion}),
                    _ => json!({}),
                };
                let _ = app.run(command, params);
            }
        }
    }
}

fn tooltip(app: &SolveApp, response: egui::Response, slot: &Slot, hidden: Option<&str>, along: Rect, edge: Edge) -> egui::Response {
    let label = if let Some(hidden) = hidden {
        app.tr(hidden).to_owned()
    } else if slot.id == "projection" && projection_locked(app) {
        app.tr("nav.perspective.sketch").to_owned()
    } else if let Some(value) = current_value(app, slot) {
        crate::i18n::format("navbar.value_tip", &[("name", app.tr(slot.label)), ("value", value)])
    } else {
        app.tr(slot.label).to_owned()
    };
    let key = slot.command.and_then(|id| crate::keymap::display_for(app, id));
    ui_kit::navigation_tooltip(&response, along, edge, &label, key.as_deref());
    response
}

pub(crate) fn show(app: &mut SolveApp, ui: &mut egui::Ui, rect: Rect) {
    if !app.ui.show_navigation_bar {
        return;
    }
    let Some(layout) = app.viewport.layout.as_ref().filter(|l| l.navigation.is_positive()) else { return };
    let bar = measured(app, ui.ctx(), layout.content);
    if !bar.rect.is_positive() {
        return;
    }
    crate::scenario::publish_rect("nav:outer", bar.rect);
    let menu_id = ui.id().with("navigation_bar_menu");
    let mut open = ui.data(|d| d.get_temp::<String>(menu_id)).unwrap_or_default();
    let active = ui.rect_contains_pointer(bar.rect)
        || ui.rect_contains_pointer(crate::viewport::axis_rect(app, rect))
        || app.viewport.axis_drag.is_some()
        || app.viewport.primary_nav.is_some()
        || app.viewport.orbit_from.is_some()
        || (app.viewport.mouse.is_some() && ui.input(|i| i.pointer.middle_down()))
        || app.viewport.modifier_nav.is_some()
        || app.viewport.spin
        || !open.is_empty();
    let opacity = ui_kit::navigation_opacity(app.preferences.navigation_bar.idle, active);
    let background = ui.interact(bar.rect, ui.id().with("navigation_bar_context"), egui::Sense::click());
    let opacity = if app.preferences.navigation_bar.idle == Idle::Hide {
        ui.ctx().animate_value_with_time(menu_id.with("opacity"), opacity, if active { ui_kit::NAV_FADE_SECONDS } else { 0.0 })
    } else {
        opacity
    };
    let mut anchor = bar.more;
    ui.scope(|ui| {
        ui.set_opacity(opacity);
        for (index, slot, response) in paint_row(app, ui, &bar, &open, None) {
            let bounds = response.rect;
            crate::scenario::publish_target(&format!("nav:slot:{}", slot.id), bounds);
            context_menu(app, &response, Some(index));
            if response.clicked() && slot.kind != Kind::Separator {
                if slot.kind == Kind::Menu {
                    open = if open == slot.id { String::new() } else { slot.id.to_owned() };
                } else {
                    activate(app, slot);
                }
            }
            if slot.id == "display" {
                crate::scenario::publish_handle("nav:view", bounds.center());
            }
            if open == slot.id {
                anchor = bounds;
            }
        }
        if bar.more.is_positive() {
            let response = ui_kit::navigation_button(
                ui,
                bar.more,
                ui_kit::NavigationButton {
                    id: "more",
                    kind: Kind::Menu,
                    icon: "dots",
                    label: app.tr("navbar.more"),
                    selected: open == "more" || bar.overflow.iter().any(|slot| slot.id == open),
                    labels: false,
                    menu: false,
                },
            );
            ui_kit::navigation_tooltip(&response, bar.rect, app.preferences.navigation_bar.edge, app.tr("navbar.more"), None);
            crate::scenario::publish_target("nav:more", response.rect);
            context_menu(app, &response, None);
            if response.clicked() {
                open = if open == "more" { String::new() } else { "more".to_owned() };
            }
        }
    });
    context_menu(app, &background, None);
    if !open.is_empty() && anchor.is_positive() {
        let child_popup_open = egui::Popup::is_any_open(ui.ctx());
        let ctx = ui.ctx().clone();
        // Menus open into the view: up from a bar at the bottom, down from one at the top.
        let (pivot, at) = match app.preferences.navigation_bar.edge {
            Edge::Bottom => (Align2::RIGHT_BOTTOM, anchor.right_top() - vec2(0.0, ui_kit::NAV_POPUP_GAP)),
            Edge::Top => (Align2::RIGHT_TOP, anchor.right_bottom() + vec2(0.0, ui_kit::NAV_POPUP_GAP)),
        };
        let menu = Container::new(&ctx, Surface::Popup).area(menu_id.with("popup")).order(egui::Order::Foreground).pivot(pivot).fixed_pos(at).show(
            &ctx,
            |ui| {
                ui_kit::navigation_menu(ui, if open == "more" { ui_kit::NAV_MORE_WIDTH } else { ui_kit::NAV_POPUP_WIDTH }, |ui| {
                    match open.as_str() {
                        "visual_style" => {
                            if style_menu(app, ui) {
                                open.clear();
                            }
                        }
                        "selection_mode" => {
                            if selection_menu(app, ui) {
                                open.clear();
                            }
                        }
                        "display" => display_menu(app, ui),
                        "more" => overflow_menu(app, ui, &bar.overflow, &mut open),
                        _ => open.clear(),
                    }
                });
            },
        );
        let outside = !child_popup_open
            && ui.input(|i| {
                i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !menu.response.rect.contains(p) && !bar.rect.contains(p))
            });
        if outside || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            open.clear();
        }
    }
    ui.data_mut(|d| d.insert_temp(menu_id, open));
}

pub(crate) fn current_style(app: &SolveApp) -> &str {
    app.tr(match app.ui.visual_style {
        1 => "navbar.style.shaded",
        2 => "navbar.style.wireframe",
        3 => "navbar.style.shaded_with_hidden_edges",
        _ => "navbar.style.shaded_with_edges",
    })
}

/// The setting a menu slot holds, for its button and tooltip; `None` for slots without one.
fn current_value<'a>(app: &'a SolveApp, slot: &Slot) -> Option<&'a str> {
    match slot.id {
        "visual_style" => Some(current_style(app)),
        "selection_mode" => Some(app.tr_label(crate::selection_mode::label(app.ui.pick_mode))),
        _ => None,
    }
}

pub(crate) fn slot_text<'a>(app: &'a SolveApp, slot: &Slot) -> &'a str {
    match current_value(app, slot) {
        Some(value) if app.preferences.navigation_bar.menu_display(slot.id) == MenuDisplay::IconAndValue => value,
        _ => app.tr(slot.label),
    }
}

/// Both viewport and Preferences call this painter. Editing changes interaction and outlines only.
pub(crate) fn paint_row(
    app: &SolveApp,
    ui: &mut egui::Ui,
    bar: &BarLayout,
    open: &str,
    editing: Option<(Option<usize>, NavCaps, bool)>,
) -> Vec<(usize, &'static Slot, egui::Response)> {
    ui_kit::navigation_surface(ui, bar.rect, app.preferences.navigation_bar.size, editing.is_some());
    let mut responses = Vec::new();
    let mut cursor = 0;
    for (slot, bounds) in &bar.slots {
        let Some((index, _)) = app.preferences.navigation_bar.slots.iter().enumerate().skip(cursor).find(|(_, id)| id.as_str() == slot.id) else {
            continue;
        };
        cursor = index + 1;
        let response = ui
            .push_id(index, |ui| {
                let hidden = editing.is_some_and(|(_, caps, _)| !slot.available(caps));
                ui.scope(|ui| {
                    if hidden {
                        ui.set_opacity(ui_kit::NAV_HIDDEN_OPACITY);
                    } else if editing.is_none() && slot.id == "projection" && projection_locked(app) {
                        ui.set_opacity(ui.visuals().disabled_alpha);
                    }
                    if slot.kind == Kind::Separator {
                        ui_kit::navigation_separator(ui, *bounds);
                        ui.interact(
                            *bounds,
                            ui.id().with("separator"),
                            if editing.is_some() { egui::Sense::click_and_drag() } else { egui::Sense::click() },
                        )
                    } else {
                        let button = ui_kit::NavigationButton {
                            id: slot.id,
                            kind: slot.kind,
                            icon: slot.current_icon(&app.ui),
                            label: slot_text(app, slot),
                            selected: if editing.is_some() { false } else { selected(app, slot.id) || open == slot.id },
                            labels: if slot.kind == Kind::Menu {
                                bar.values && slot.shows_value() && app.preferences.navigation_bar.menu_display(slot.id) == MenuDisplay::IconAndValue
                            } else {
                                bar.labels
                            },
                            menu: slot.kind == Kind::Menu,
                        };
                        if editing.is_some() {
                            ui_kit::navigation_button_editable(ui, *bounds, button, true)
                        } else {
                            ui_kit::navigation_button(ui, *bounds, button)
                        }
                    }
                })
                .inner
            })
            .inner;
        let mut hidden = None;
        if let Some((selected, caps, render)) = editing {
            response
                .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), selected == Some(index), app.tr(slot.label)));
            ui_kit::navigation_edit_outline(ui, *bounds, !slot.available(caps), selected == Some(index));
            if !slot.available(caps) {
                hidden = Some(if render { "navbar.custom.hidden_render" } else { "navbar.custom.hidden_design" });
            }
        } else {
            response.widget_info(|| {
                egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), selected(app, slot.id) || open == slot.id, app.tr(slot.label))
            });
        }
        // The settings' preview keeps its tips above, wherever the bar sits in the view.
        let edge = if editing.is_some() { Edge::Bottom } else { app.preferences.navigation_bar.edge };
        responses.push((index, *slot, tooltip(app, response, slot, hidden, bar.rect, edge)));
    }
    responses
}

pub(crate) fn context_menu(app: &mut SolveApp, response: &egui::Response, index: Option<usize>) {
    ui_kit::navigation_context(response, |ui| {
        for id in if index.is_none() { &["customise", "hide", "reset"][..] } else { &[][..] } {
            let key = format!("navbar.custom.{id}");
            let item = ui_kit::NavigationMenuRow::new(id, app.tr(&key), "").show(ui);
            crate::scenario::publish_target(&format!("nav:context:{id}"), item.rect);
            if item.clicked() {
                match *id {
                    "customise" => {
                        let _ = crate::prefs::control(app, &json!({"open":"Interface/Navigation bar"}));
                    }
                    "hide" => {
                        let _ = app.run("view.navigation_bar", json!({"visible":false}));
                    }
                    _ => {
                        app.preferences.navigation_bar = NavigationBar::default();
                        let _ = app.run("view.navigation_bar", json!({"visible":true}));
                    }
                }
                ui.close();
            }
        }
        if let Some(index) = index {
            let item = ui_kit::NavigationMenuRow::new("remove", app.tr("navbar.custom.remove_bar"), "").show(ui);
            crate::scenario::publish_target("nav:context:remove", item.rect);
            if item.clicked() {
                let _ = app.preferences.navigation_bar.remove(index);
                ui.close();
            }
        }
    });
}

fn style_menu(app: &mut SolveApp, ui: &mut egui::Ui) -> bool {
    let mut chosen = false;
    for (index, style) in navigation_bar::VISUAL_STYLES.iter().map(|(style, _)| *style).enumerate() {
        let label = format!("navbar.style.{style}");
        let mut item = ui_kit::NavigationMenuRow::new(style, app.tr(&label), navigation_bar::visual_style_icon(index as u8));
        item.selected = app.ui.visual_style == index as u8;
        let response = item.show(ui);
        crate::scenario::publish_target(&format!("nav:style:{style}"), response.rect);
        if response.clicked() {
            let _ = app.run("view.visual_style", json!({"style":style}));
            chosen = true;
        }
    }
    ui_kit::navigation_menu_separator(ui);
    let mut later = ui_kit::NavigationMenuRow::new("translucent", app.tr("navbar.translucent"), "style_translucent");
    later.enabled = false;
    later.detail = Some(app.tr("navbar.later"));
    later.show(ui);
    chosen
}

/// "What a click picks": the five modes that pick from the model, then sketch geometry. Rows run
/// `select.mode`; true once one is chosen.
fn selection_menu(app: &mut SolveApp, ui: &mut egui::Ui) -> bool {
    use solidcraft_engine::PickMode;
    ui_kit::navigation_menu_heading(ui, app.tr("navbar.selection.heading"));
    let mut chosen = false;
    for &(mode, icon) in navigation_bar::SELECTION_MODE_ICONS {
        if mode == PickMode::Sketch {
            ui_kit::navigation_menu_separator(ui);
        }
        let id = crate::selection_mode::id(mode);
        let mut item = ui_kit::NavigationMenuRow::new(&id, app.tr_label(crate::selection_mode::label(mode)), icon);
        item.selected = app.ui.pick_mode == mode;
        let response = item.show(ui);
        crate::scenario::publish_target(&format!("nav:selection:{id}"), response.rect);
        if response.clicked() {
            crate::selection_mode::set(app, mode);
            chosen = true;
        }
    }
    chosen
}

fn overflow_menu(app: &mut SolveApp, ui: &mut egui::Ui, slots: &[&Slot], open: &mut String) {
    for slot in slots {
        if slot.kind == Kind::Separator {
            let start = ui.next_widget_position();
            ui_kit::navigation_menu_separator(ui);
            let bounds = Rect::from_min_max(start, pos2(start.x + ui.available_width(), ui.next_widget_position().y));
            crate::scenario::publish_target(&format!("nav:slot:{}", slot.id), bounds);
            continue;
        }
        let mut item = ui_kit::NavigationMenuRow::new(slot.id, app.tr(slot.label), slot.current_icon(&app.ui));
        if matches!(slot.kind, Kind::Tool | Kind::Toggle) {
            item.selected = selected(app, slot.id);
        }
        let response = item.show(ui);
        let index = app.preferences.navigation_bar.slots.iter().position(|id| id == slot.id);
        context_menu(app, &response, index);
        crate::scenario::publish_target(&format!("nav:slot:{}", slot.id), response.rect);
        if slot.id == "display" {
            crate::scenario::publish_handle("nav:view", response.rect.center());
        }
        if tooltip(app, response.clone(), slot, None, response.rect, Edge::Bottom).clicked() {
            if slot.kind == Kind::Menu {
                *open = slot.id.to_owned();
            } else {
                activate(app, slot);
                open.clear();
            }
        }
    }
}

fn display_menu(app: &mut SolveApp, ui: &mut egui::Ui) {
    use ui_kit::{NavigationMenuRow, navigation_menu_heading as heading, navigation_menu_separator as separator};
    heading(ui, app.tr("navbar.graphics"));
    let current = crate::prefs::graphics_preset(&app.preferences);
    let detail = format!("ui.graphics_{current}");
    let mut preset = NavigationMenuRow::new("preset", app.tr("navbar.preset"), "");
    preset.detail = Some(app.tr(&detail));
    preset.submenu = true;
    let response = preset.show(ui);
    crate::scenario::publish_target("view:graphics_preset", response.rect);
    Container::new(ui.ctx(), Surface::Popup).menu(&response).frame(ui_kit::navigation_popup_frame()).show(|ui| {
        ui.set_width(ui_kit::NAV_POPUP_WIDTH - 2.0 * (f32::from(ui_kit::NAV_POPUP_PADDING) + ui_kit::NAV_BORDER));
        ui.spacing_mut().item_spacing.y = 0.0;
        for (id, label) in [("performance", "ui.graphics_performance"), ("quality", "ui.graphics_quality")] {
            let mut item = NavigationMenuRow::new(id, app.tr(label), "");
            item.selected = current == id;
            let response = item.show(ui);
            crate::scenario::publish_target(&format!("view:graphics_preset:{id}"), response.rect);
            if response.clicked() {
                let _ = app.run("view.graphics_preset", json!({"preset":id}));
                ui.close();
            }
        }
    });
    separator(ui);
    heading(ui, app.tr("navbar.show"));
    for id in ["grid", "ground_shadow", "ambient_occlusion"] {
        let Some(slot) = navigation_bar::find(id) else { continue };
        let mut item = NavigationMenuRow::new(slot.id, app.tr(slot.label), slot.current_icon(&app.ui));
        item.selected = selected(app, slot.id);
        let response = item.show(ui);
        crate::scenario::publish_target(&format!("view:{id}"), response.rect);
        if response.clicked() {
            activate(app, slot);
        }
    }
    separator(ui);
    heading(ui, app.tr("navbar.projection_heading"));
    for (perspective, key) in [(false, "navbar.orthographic"), (true, "navbar.perspective")] {
        let mut item = NavigationMenuRow::new(key, app.tr(key), "perspective");
        item.selected = app.ui.perspective == perspective;
        item.enabled = !projection_locked(app);
        let response = item.show(ui);
        let response = if projection_locked(app) { response.on_hover_text(app.tr("nav.perspective.sketch")) } else { response };
        crate::scenario::publish_target(if perspective { "view:projection" } else { "view:orthographic" }, response.rect);
        if response.clicked() && app.ui.perspective != perspective {
            toggle_projection(app);
        }
    }
    separator(ui);
    for (id, label, on) in [
        ("auto_ortho_faces", "Perspective while orbiting, ortho on faces", app.ui.auto_ortho_faces),
        ("tool_cursor", "Show tool at cursor", app.ui.tool_cursor),
    ] {
        let mut row = NavigationMenuRow::new(id, crate::i18n::tr_label(label), "select");
        row.selected = on;
        let response = row.show(ui);
        crate::scenario::publish_target(&format!("view:{id}"), response.rect);
        if response.clicked() {
            match id {
                "auto_ortho_faces" => app.ui.auto_ortho_faces = !on,
                _ => app.ui.tool_cursor = !on,
            }
        }
    }
    // The View angles readout under the orientation gizmo (#6032).
    let mut angles = NavigationMenuRow::new("view_angles", app.tr("ui.view_angles"), "select");
    angles.selected = app.preferences.show_view_angles;
    let response = angles.show(ui).on_hover_text(app.tr("ui.view_angles_help"));
    crate::scenario::publish_target("view:view_angles", response.rect);
    if response.clicked() {
        let _ = app.run("view.angle_readout", json!({"enabled": !app.preferences.show_view_angles}));
    }
    separator(ui);
    heading(ui, app.tr("navbar.assistant"));
    let mut cursor = NavigationMenuRow::new("agent_cursor", app.tr("navbar.agent_cursor"), "select");
    cursor.selected = app.ui.agent_cursor.show;
    let response = cursor.show(ui);
    crate::scenario::publish_target("view:agent_cursor", response.rect);
    if response.clicked() {
        app.ui.agent_cursor.show = !app.ui.agent_cursor.show;
    }
    if crate::agent_chat::shown(app) {
        let mut chat = NavigationMenuRow::new("agent_chat", app.tr("navbar.agent_chat"), "agent_chat");
        chat.selected = app.agent_chat.open;
        let response = chat.show(ui);
        crate::scenario::publish_target("view:agent_chat", response.rect);
        if response.clicked() {
            app.agent_chat.open = !app.agent_chat.open;
            app.preferences.agent_chat_minimized &= !app.agent_chat.open;
        }
    }
}
