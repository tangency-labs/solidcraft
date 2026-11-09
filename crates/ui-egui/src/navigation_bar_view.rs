//! Measured single-row navigation bar. Commands own all view changes.
use crate::nav_bar::{self, Bar, BarLabels, ContextItem, Menu, MenuEntry, MenuRow, NavAction, SlotView};
use crate::navigation::NavCaps;
use crate::navigation_bar::{self, Labels, MenuDisplay, NavigationBar, Position, Slot};
use crate::ui_kit::{self, NavigationEdge, NavigationKind};
use crate::{SolveApp, viewport::NavMode};
use egui::{Rect, pos2, vec2};
use serde_json::json;
use solidcraft_theme::Tokens;

pub(crate) struct BarLayout {
    pub rect: Rect,
    pub slots: Vec<(&'static Slot, Rect)>,
    pub overflow: Vec<&'static Slot>,
    pub more: Rect,
    pub labels: bool,
    pub values: bool,
}

/// Drop menu values, then labels, before moving a suffix into More. All rectangles use logical UI points.
pub(crate) fn measure(content: Rect, prefs: &NavigationBar, caps: NavCaps, label_width: impl Fn(&Slot) -> f32) -> BarLayout {
    measure_row(content, prefs, caps, label_width, false)
}

fn measure_row(content: Rect, prefs: &NavigationBar, caps: NavCaps, label_width: impl Fn(&Slot) -> f32, editing: bool) -> BarLayout {
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
        prefs.slots.iter().take(navigation_bar::MAX_SLOTS).filter_map(|id| navigation_bar::find(id)).filter(|slot| slot.available(caps)).collect();
    if slots.is_empty() {
        return result;
    }
    let width = |slot: &Slot, labels, values| {
        if slot.kind == NavigationKind::Separator {
            NAV_SEPARATOR
        } else if slot.kind == NavigationKind::Menu {
            metrics.icon
                + if editing { 2.0 * NAV_BORDER } else { 0.0 }
                + NAV_MENU_LEFT
                + NAV_MENU_RIGHT
                + NAV_CHEVRON
                + NAV_MENU_GAP
                + if values && slot.id == "visual_style" && prefs.menu_display(slot.id) == MenuDisplay::IconAndValue {
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
        NavigationEdge::Bottom => content.bottom() - height - NAV_BOTTOM,
        NavigationEdge::Top => content.top() + NAV_BOTTOM,
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
    let mut bar = measure_row(virtual_canvas, prefs, NavCaps::MODEL, label_width, true);
    if !bar.rect.is_positive() {
        let metrics = ui_kit::NavigationMetrics::for_size(prefs.size);
        bar.rect = Rect::from_min_size(stage.min, vec2(metrics.button + 2.0 * (ui_kit::NAV_PADDING + ui_kit::NAV_BORDER), metrics.height()));
    }
    let x = match prefs.position {
        Position::Left => stage.left() + ui_kit::NAV_BORDER + ui_kit::NAV_STAGE_INSET,
        Position::Centre => stage.center().x - bar.rect.width() / 2.0,
        Position::Right => stage.right() - ui_kit::NAV_BORDER - ui_kit::NAV_STAGE_INSET - bar.rect.width(),
    };
    // The same inset from the top or bottom edge as from the side, as in the live view.
    let y = match prefs.edge {
        NavigationEdge::Top => stage.top() + ui_kit::NAV_BORDER + ui_kit::NAV_STAGE_INSET,
        NavigationEdge::Bottom => stage.bottom() - ui_kit::NAV_BORDER - ui_kit::NAV_STAGE_INSET - bar.rect.height(),
    };
    let delta = pos2(x, y) - bar.rect.min;
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
    let label_width = |slot: &Slot| {
        ctx.fonts_mut(|fonts| fonts.layout_no_wrap(slot_text(app, slot).to_owned(), crate::fonts::medium(ui_kit::NAV_TEXT), t.form_ink).size().x)
    };
    let mut bar = measure(content, &app.preferences.navigation_bar, navigation_bar::canvas_caps(app), label_width);
    let Some(_) = app.universal_panel_frame() else {
        return bar;
    };
    if !bar.rect.is_positive() {
        return bar;
    }
    let Ok(region) = crate::shell::host_usable(app) else {
        bar.rect = Rect::NOTHING;
        bar.slots.clear();
        bar.more = Rect::NOTHING;
        return bar;
    };
    let preferred = bar.rect.center();
    let place = |bar: &BarLayout| {
        region.place_footprint(
            solidcraft_engine::panels::Size { width: bar.rect.width(), height: bar.rect.height() },
            solidcraft_engine::panels::Point { x: preferred.x, y: preferred.y },
            t.space_4,
        )
    };
    let mut location = place(&bar);
    if location.is_none() {
        // Keep the existing value/label/More policy when a full row cannot fit.
        // Measure a smaller row, then validate its complete footprint against
        // the contour rather than anchoring it to a fictitious central strip.
        let width = region
            .free
            .iter()
            .filter(|bounds| bounds.size.height >= bar.rect.height() + 2.0 * t.space_4)
            .map(|bounds| (bounds.size.width - 2.0 * t.space_4).max(0.0))
            .max_by(f32::total_cmp)
            .unwrap_or(0.0);
        bar = measure(
            Rect::from_min_size(content.min, vec2(width, content.height())),
            &app.preferences.navigation_bar,
            navigation_bar::canvas_caps(app),
            label_width,
        );
        if bar.rect.is_positive() {
            location = place(&bar);
        }
    }
    if location.is_none() {
        // A minimal More row may span adjacent sweep slabs even when no single
        // free piece is tall enough. Validate that measured footprint directly.
        let metrics = ui_kit::NavigationMetrics::for_size(app.preferences.navigation_bar.size);
        let width = metrics.button + 2.0 * (ui_kit::NAV_BORDER + ui_kit::NAV_PADDING + ui_kit::NAV_EDGE);
        bar = measure(
            Rect::from_min_size(content.min, vec2(width, content.height())),
            &app.preferences.navigation_bar,
            navigation_bar::canvas_caps(app),
            label_width,
        );
        if bar.rect.is_positive() {
            location = place(&bar);
        }
    }
    if let Some(location) = location {
        let delta = crate::panel_runtime::egui_rect(location).min - bar.rect.min;
        bar.rect = bar.rect.translate(delta);
        for (_, rect) in &mut bar.slots {
            *rect = rect.translate(delta);
        }
        if bar.more.is_positive() {
            bar.more = bar.more.translate(delta);
        }
    } else {
        bar.rect = Rect::NOTHING;
        bar.slots.clear();
        bar.more = Rect::NOTHING;
    }
    bar
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
                    NavigationKind::Tool if command == "view.tool" => json!({"tool": if selected(app, slot.id) { "none" } else { slot.id }}),
                    _ if slot.id == "ambient_occlusion" => json!({"enabled": !app.preferences.ambient_occlusion}),
                    _ => json!({}),
                };
                let _ = app.run(command, params);
            }
        }
    }
}

/// The slot's hover text: a hidden slot's reason, the current style, the sketch's lock, or its name.
fn tip(app: &SolveApp, slot: &Slot, hidden: Option<&str>, key: bool) -> String {
    if let Some(hidden) = hidden {
        app.tr(hidden).to_owned()
    } else if slot.id == "visual_style" {
        solidcraft_ui_text::format("navbar.value_tip", &[("name", app.tr(slot.label)), ("value", current_style(app))])
    } else if slot.id == "projection" && projection_locked(app) {
        app.tr("nav.perspective.sketch").to_owned()
    } else if slot.id == "zoom" && key {
        app.tr("navbar.zoom_hold_tip").to_owned()
    } else {
        app.tr(slot.label).to_owned()
    }
}

/// Slot `slot`, at `index` of the stored list, as the bar component draws it.
fn slot_view(app: &SolveApp, bar: &BarLayout, slot: &'static Slot, index: usize, rect: Rect, editing: Option<(NavCaps, bool)>) -> SlotView<'static> {
    let hidden = editing
        .filter(|(caps, _)| !slot.available(*caps))
        .map(|(_, render)| if render { "navbar.custom.hidden_render" } else { "navbar.custom.hidden_design" });
    let key = slot.command.and_then(|id| crate::keymap::display_for(app, id));
    let value = slot.kind == NavigationKind::Menu
        && slot.id == "visual_style"
        && app.preferences.navigation_bar.menu_display(slot.id) == MenuDisplay::IconAndValue;
    SlotView {
        index,
        id: slot.id,
        kind: slot.kind,
        icon: slot.current_icon(app.ui.visual_style),
        text: slot_text(app, slot).to_owned(),
        name: app.tr(slot.label).to_owned(),
        tip: tip(app, slot, hidden, key.is_some()),
        key,
        selected: editing.is_none() && selected(app, slot.id),
        enabled: editing.is_some() || !(slot.id == "projection" && projection_locked(app)),
        hidden: hidden.is_some(),
        show_text: if slot.kind == NavigationKind::Menu { bar.values && value } else { bar.labels },
        rect,
        // The View menu's slot, by the name older scenarios use.
        alias: (slot.id == "display").then_some("nav:view"),
    }
}

/// The measured row's slots, each with its index in the stored list (a slot the list no longer
/// holds is left out).
fn slot_views(app: &SolveApp, bar: &BarLayout, editing: Option<(NavCaps, bool)>) -> Vec<SlotView<'static>> {
    let mut cursor = 0;
    let mut views = Vec::new();
    for (slot, rect) in &bar.slots {
        let Some((index, _)) = app.preferences.navigation_bar.slots.iter().enumerate().skip(cursor).find(|(_, id)| id.as_str() == slot.id) else {
            continue;
        };
        cursor = index + 1;
        views.push(slot_view(app, bar, slot, index, *rect, editing));
    }
    views
}

pub(crate) fn show(app: &mut SolveApp, ui: &mut egui::Ui, rect: Rect) {
    if !app.ui.show_navigation_bar {
        return;
    }
    let Some(layout) = app.viewport.layout.as_ref().filter(|l| l.navigation.is_positive()) else { return };
    let measured = measured(app, ui.ctx(), layout.content);
    if !measured.rect.is_positive() {
        return;
    }
    let active = ui.rect_contains_pointer(crate::viewport::axis_rect(app, rect))
        || app.viewport.axis_drag.is_some()
        || app.viewport.primary_nav.is_some()
        || app.viewport.orbit_from.is_some()
        || (app.viewport.mouse.is_some() && ui.input(|i| i.pointer.middle_down()))
        || app.viewport.modifier_nav.is_some()
        || app.viewport.spin;
    let prefs = &app.preferences.navigation_bar;
    let bar = Bar {
        rect: measured.rect,
        size: prefs.size,
        edge: prefs.edge,
        idle: prefs.idle,
        active,
        slots: slot_views(app, &measured, None),
        overflow: measured
            .overflow
            .iter()
            .filter_map(|slot| Some(slot_view(app, &measured, slot, prefs.slots.iter().position(|id| id == slot.id)?, Rect::NOTHING, None)))
            .collect(),
        more: measured.more,
        labels: BarLabels {
            more: app.tr("navbar.more").to_owned(),
            customise: app.tr("navbar.custom.customise").to_owned(),
            hide: app.tr("navbar.custom.hide").to_owned(),
            reset: app.tr("navbar.custom.reset").to_owned(),
            remove: app.tr("navbar.custom.remove_bar").to_owned(),
        },
    };
    let action = nav_bar::show(ui, &bar, |id| menu(app, id));
    if let Some(action) = action {
        run(app, action);
        // Rows of a menu that stays open show the new state on the next frame, without new input.
        ui.ctx().request_repaint();
    }
}

/// Run what the bar returned.
fn run(app: &mut SolveApp, action: NavAction) {
    match action {
        NavAction::Slot(id) => {
            if let Some(slot) = navigation_bar::find(&id) {
                activate(app, slot);
            }
        }
        NavAction::Context(ContextItem::Customise) => {
            let _ = crate::prefs::control(app, &json!({"open":"Interface/Navigation bar"}));
        }
        NavAction::Context(ContextItem::Hide) => {
            let _ = app.run("view.navigation_bar", json!({"visible":false}));
        }
        NavAction::Context(ContextItem::Reset) => {
            app.preferences.navigation_bar = NavigationBar::default();
            let _ = app.run("view.navigation_bar", json!({"visible":true}));
        }
        NavAction::Context(ContextItem::Remove(index)) => {
            let _ = app.preferences.navigation_bar.remove(index);
        }
        NavAction::Item { menu, item } if menu == "visual_style" => {
            let _ = app.run("view.visual_style", json!({"style":item}));
        }
        NavAction::Item { item, .. } => display_pick(app, &item),
    }
}

pub(crate) fn current_style(app: &SolveApp) -> &str {
    app.tr(match app.ui.visual_style {
        1 => "navbar.style.shaded",
        2 => "navbar.style.wireframe",
        3 => "navbar.style.shaded_with_hidden_edges",
        _ => "navbar.style.shaded_with_edges",
    })
}

pub(crate) fn slot_text<'a>(app: &'a SolveApp, slot: &Slot) -> &'a str {
    if slot.id == "visual_style" && app.preferences.navigation_bar.menu_display(slot.id) == MenuDisplay::IconAndValue {
        current_style(app)
    } else {
        app.tr(slot.label)
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
    let mut views = slot_views(app, bar, editing.map(|(_, caps, render)| (caps, render)));
    for view in &mut views {
        view.selected |= editing.is_none() && open == view.id;
    }
    // The settings' preview keeps its tips above, wherever the bar sits in the view.
    let edge = if editing.is_some() { NavigationEdge::Bottom } else { app.preferences.navigation_bar.edge };
    let responses = nav_bar::paint_slots(ui, bar.rect, app.preferences.navigation_bar.size, edge, &views, editing.map(|(selected, _, _)| selected));
    views.iter().zip(responses).filter_map(|(view, response)| Some((view.index, navigation_bar::find(view.id)?, response))).collect()
}

/// The rows of the menu slot `id` opens (none for an unknown one, which closes it).
fn menu(app: &SolveApp, id: &str) -> Menu {
    match id {
        "visual_style" => style_menu(app),
        "display" => display_menu(app),
        _ => Menu::default(),
    }
}

/// Visual style: the four styles, then Translucent (later); a pick closes the menu.
fn style_menu(app: &SolveApp) -> Menu {
    let mut entries: Vec<MenuEntry> = navigation_bar::VISUAL_STYLES
        .iter()
        .enumerate()
        .map(|(index, (style, _))| {
            let mut row = MenuRow::new(
                *style,
                app.tr(&format!("navbar.style.{style}")),
                navigation_bar::visual_style_icon(index as u8),
                format!("nav:style:{style}"),
            );
            row.selected = app.ui.visual_style == index as u8;
            MenuEntry::Row(row)
        })
        .collect();
    entries.push(MenuEntry::Separator);
    let mut later = MenuRow::new("translucent", app.tr("navbar.translucent"), "style_translucent", "");
    later.enabled = false;
    later.detail = Some(app.tr("navbar.later").to_owned());
    entries.push(MenuEntry::Row(later));
    Menu { entries, close_on_pick: true }
}

/// The View menu: graphics preset, what the view shows, projection, cursor and the assistant.
fn display_menu(app: &SolveApp) -> Menu {
    let row = |id: &str, label: &str, icon: &str, target: &str, selected: bool| {
        let mut row = MenuRow::new(id, label, icon, target);
        row.selected = selected;
        row
    };
    let mut entries = vec![MenuEntry::Heading(app.tr("navbar.graphics").to_owned())];
    let current = crate::prefs::graphics_preset(&app.preferences);
    let mut preset = MenuRow::new("preset", app.tr("navbar.preset"), "", "view:graphics_preset");
    preset.detail = Some(app.tr(&format!("ui.graphics_{current}")).to_owned());
    preset.submenu = [("performance", "ui.graphics_performance"), ("quality", "ui.graphics_quality")]
        .into_iter()
        .map(|(id, label)| row(id, app.tr(label), "", &format!("view:graphics_preset:{id}"), current == id))
        .collect();
    entries.extend([MenuEntry::Row(preset), MenuEntry::Separator, MenuEntry::Heading(app.tr("navbar.show").to_owned())]);
    for id in ["grid", "ground_shadow", "ambient_occlusion"] {
        let Some(slot) = navigation_bar::find(id) else { continue };
        entries.push(MenuEntry::Row(row(
            slot.id,
            app.tr(slot.label),
            slot.current_icon(app.ui.visual_style),
            &format!("view:{id}"),
            selected(app, slot.id),
        )));
    }
    entries.extend([MenuEntry::Separator, MenuEntry::Heading(app.tr("navbar.projection_heading").to_owned())]);
    for (perspective, key) in [(false, "navbar.orthographic"), (true, "navbar.perspective")] {
        let mut item = row(
            key,
            app.tr(key),
            "perspective",
            if perspective { "view:projection" } else { "view:orthographic" },
            app.ui.perspective == perspective,
        );
        item.enabled = !projection_locked(app);
        item.tip = projection_locked(app).then(|| app.tr("nav.perspective.sketch").to_owned());
        entries.push(MenuEntry::Row(item));
    }
    entries.push(MenuEntry::Separator);
    for (id, label, on) in [
        ("auto_ortho_faces", "Perspective while orbiting, ortho on faces", app.ui.auto_ortho_faces),
        ("tool_cursor", "Show tool at cursor", app.ui.tool_cursor),
    ] {
        entries.push(MenuEntry::Row(row(id, solidcraft_ui_text::tr_label(label), "select", &format!("view:{id}"), on)));
    }
    // The View angles readout under the orientation gizmo (#6032).
    let mut angles = row("view_angles", app.tr("ui.view_angles"), "select", "view:view_angles", app.preferences.show_view_angles);
    angles.tip = Some(app.tr("ui.view_angles_help").to_owned());
    entries.extend([MenuEntry::Row(angles), MenuEntry::Separator, MenuEntry::Heading(app.tr("navbar.assistant").to_owned())]);
    entries.push(MenuEntry::Row(row("agent_cursor", app.tr("navbar.agent_cursor"), "select", "view:agent_cursor", app.ui.agent_cursor.show)));
    if crate::agent_chat::shown(app) {
        entries.push(MenuEntry::Row(row("agent_chat", app.tr("navbar.agent_chat"), "agent_chat", "view:agent_chat", app.agent_chat.open)));
    }
    Menu { entries, close_on_pick: false }
}

/// A View menu pick.
fn display_pick(app: &mut SolveApp, item: &str) {
    match item {
        "performance" | "quality" => {
            let _ = app.run("view.graphics_preset", json!({"preset":item}));
        }
        "navbar.orthographic" | "navbar.perspective" => {
            if app.ui.perspective != (item == "navbar.perspective") {
                toggle_projection(app);
            }
        }
        "auto_ortho_faces" => app.ui.auto_ortho_faces = !app.ui.auto_ortho_faces,
        "tool_cursor" => app.ui.tool_cursor = !app.ui.tool_cursor,
        "view_angles" => {
            let _ = app.run("view.angle_readout", json!({"enabled": !app.preferences.show_view_angles}));
        }
        "agent_cursor" => app.ui.agent_cursor.show = !app.ui.agent_cursor.show,
        "agent_chat" => {
            app.agent_chat.open = !app.agent_chat.open;
            app.preferences.agent_chat_minimized &= !app.agent_chat.open;
        }
        id => {
            if let Some(slot) = navigation_bar::find(id) {
                activate(app, slot);
            }
        }
    }
}
