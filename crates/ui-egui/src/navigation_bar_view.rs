//! Measured single-row navigation bar. Commands own all view changes.
use crate::nav_bar::{self, Bar, BarLabels, ContextItem, Menu, MenuEntry, MenuRow, NavAction, SlotView};
use crate::navigation::NavCaps;
use crate::navigation_bar::{self, MenuDisplay, NavigationBar, Slot};
use crate::ui_kit::nav_bar_layout::{BarLayout, measure};
use crate::ui_kit::{self, NavigationEdge, NavigationKind};
use crate::{SolveApp, viewport::NavMode};
use egui::{Rect, vec2};
use serde_json::json;
use solidcraft_theme::Tokens;

/// What the current canvas lets the camera do: a flat sheet page or a finished render cannot
/// orbit or pick, the Render workspace has its own caps, and the model has the full set.
pub(crate) fn canvas_caps(app: &SolveApp) -> NavCaps {
    if crate::dialogs_sheet::flat_active(app) || crate::render_workspace::result_active(app) {
        NavCaps { orbit: false, pick: false }
    } else if crate::render_workspace::active(app) {
        NavCaps::RENDER
    } else {
        NavCaps::MODEL
    }
}

pub(crate) fn measured(app: &SolveApp, ctx: &egui::Context, content: Rect) -> BarLayout {
    let t = Tokens::get();
    let label_width = |slot: &Slot| {
        ctx.fonts_mut(|fonts| {
            fonts.layout_no_wrap(slot_text(app, slot).to_owned(), crate::ui_kit::type_face::medium(ui_kit::NAV_TEXT), t.form_ink).size().x
        })
    };
    let caps = canvas_caps(app);
    let shown = |slot: &Slot| caps.shows(slot);
    let mut bar = measure(content, &app.preferences.navigation_bar, shown, label_width);
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
        bar = measure(Rect::from_min_size(content.min, vec2(width, content.height())), &app.preferences.navigation_bar, shown, label_width);
        if bar.rect.is_positive() {
            location = place(&bar);
        }
    }
    if location.is_none() {
        // A minimal More row may span adjacent sweep slabs even when no single
        // free piece is tall enough. Validate that measured footprint directly.
        let metrics = ui_kit::NavigationMetrics::for_size(app.preferences.navigation_bar.size);
        let width = metrics.button + 2.0 * (ui_kit::NAV_BORDER + ui_kit::NAV_PADDING + ui_kit::NAV_EDGE);
        bar = measure(Rect::from_min_size(content.min, vec2(width, content.height())), &app.preferences.navigation_bar, shown, label_width);
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
        .filter(|(caps, _)| !slot.available(caps.orbit, caps.pick))
        .map(|(_, render)| if render { "navbar.custom.hidden_render" } else { "navbar.custom.hidden_design" });
    let key = slot.command.and_then(|id| crate::keymap::display_for(app, id));
    let value = slot.kind == NavigationKind::Menu && app.preferences.navigation_bar.menu_display(slot.id) == MenuDisplay::IconAndValue;
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
    let mut entries: Vec<MenuEntry> = crate::navigation_slots::VISUAL_STYLES
        .iter()
        .enumerate()
        .map(|(index, (style, _))| {
            let mut row = MenuRow::new(
                *style,
                app.tr(&format!("navbar.style.{style}")),
                crate::navigation_slots::visual_style_icon(index as u8),
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
        }
        id => {
            if let Some(slot) = navigation_bar::find(id) {
                activate(app, slot);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navigation_bar::*;
    use crate::navigation_slots::CATALOGUE;
    use serde_json::json;

    #[test]
    fn default_slots_and_canvas_availability_preserve_navigation() {
        crate::navigation_slots::install();
        let bar = NavigationBar::default();
        assert_eq!(bar.slots, ["orbit", "pan", "zoom", "separator", "fit", "look_at", "home", "separator", "visual_style", "display"]);
        let flat = NavCaps { orbit: false, pick: false };
        for caps in [NavCaps::MODEL, NavCaps::RENDER, flat] {
            let shown: Vec<_> = CATALOGUE.iter().filter(|slot| slot.available(caps.orbit, caps.pick)).map(|slot| slot.id).collect();
            for id in ["orbit", "spin", "look_at", "projection", "visual_style"] {
                assert_eq!(shown.contains(&id), caps.orbit && (caps.pick || !matches!(id, "look_at" | "visual_style")), "{caps:?}: {id}");
            }
            for id in ["pan", "zoom", "fit", "home", "display", "grid", "ground_shadow", "ambient_occlusion"] {
                assert!(shown.contains(&id), "{caps:?}: {id}");
            }
        }
        let mut app = crate::SolveApp::new(Default::default(), Default::default());
        app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        let caps = canvas_caps(&app);
        assert!(find("visual_style").unwrap().available(caps.orbit, caps.pick), "the previous bar kept style available in a sketch");
    }

    #[test]
    fn navigation_preview_and_viewport_share_the_row_painter() {
        let ctx = crate::fonts::test_context();
        let app = crate::SolveApp::new(Default::default(), Default::default());
        let content = Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 200.0));
        let bar = measure(content, &app.preferences.navigation_bar, |slot| NavCaps::MODEL.shows(slot), |_| 0.0);
        let paint = |editing| {
            crate::test_support::frame(&ctx, egui::RawInput::default(), |ui| {
                crate::navigation_bar_view::paint_row(&app, ui, &bar, "", editing);
            })
        };
        let live = paint(None);
        let preview = paint(Some((None, NavCaps::MODEL, false)));
        assert_eq!(live.shapes.len(), preview.shapes.len());
        for (live, preview) in live.shapes.iter().zip(&preview.shapes) {
            assert_eq!(live.clip_rect, preview.clip_rect);
            if let (egui::Shape::Rect(live), egui::Shape::Rect(preview)) = (&live.shape, &preview.shape)
                && live.fill != preview.fill
            {
                assert_eq!(live.fill, solidcraft_theme::Tokens::get().dialog_body.gamma_multiply(0.97));
                assert_eq!(preview.fill, solidcraft_theme::Tokens::get().dialog_body);
                let mut opaque = live.clone();
                opaque.fill = preview.fill;
                assert_eq!(&opaque, preview, "only the board's opaque editing surface differs");
            } else if let (egui::Shape::Mesh(live), egui::Shape::Mesh(preview)) = (&live.shape, &preview.shape)
                && live != preview
            {
                let first = live.vertices.first().unwrap().pos;
                assert!(bar.slots.iter().any(|(slot, rect)| slot.kind == NavigationKind::Menu && rect.contains(first)));
                let delta = preview.vertices.first().unwrap().pos - first;
                assert_eq!(delta.y, 0.0);
                assert_eq!(delta.x.abs(), crate::ui_kit::NAV_BORDER, "editing menus include their CSS transparent border");
                let mut shifted = live.as_ref().clone();
                for vertex in &mut shifted.vertices {
                    vertex.pos += delta;
                }
                assert_eq!(&shifted, preview.as_ref(), "menu padding changes no artwork, colour, UVs or other geometry");
            } else {
                assert_eq!(live, preview, "slot painting is identical in both callers");
            }
        }
    }

    #[test]
    fn navigation_profile_export_import_preserves_menu_options_and_legacy_profiles() {
        let mut source = crate::SolveApp::new(Default::default(), Default::default());
        source.preferences.navigation_bar.slots = vec!["visual_style".into()];
        source.preferences.navigation_bar.slot_options.insert("visual_style".into(), SlotOptions { shows: MenuDisplay::IconAndValue });
        let profile = crate::toolbar_custom::export(&source);
        let mut dest = crate::SolveApp::new(Default::default(), Default::default());
        crate::toolbar_custom::import(&mut dest, &profile).unwrap();
        assert_eq!(source.preferences.navigation_bar, dest.preferences.navigation_bar);
        let before = dest.preferences.navigation_bar.clone();
        crate::toolbar_custom::import(&mut dest, &json!({"format":"solidcraft-shortcuts/1"})).unwrap();
        assert_eq!(before, dest.preferences.navigation_bar);
    }

    #[test]
    fn navigation_context_menus_customise_hide_reset_and_remove() {
        for action in ["customise", "hide", "reset", "remove"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.app.preferences.navigation_bar.slots = vec!["pan".into(), "fit".into()];
            h.frames(4);
            if action == "remove" {
                h.step(&json!({"click":{"handle":"nav:slot:pan"},"button":"right"})).unwrap();
            } else {
                let bar = crate::scenario::rect_of("nav:outer").unwrap();
                h.step(&json!({"click":[bar.left()+1.0,bar.top()+1.0],"button":"right"})).unwrap();
            }
            h.frames(3);
            for entry in ["customise", "hide", "reset", "remove"] {
                assert_eq!(
                    crate::scenario::handle_at(&format!("nav:context:{entry}")).is_some(),
                    if action == "remove" { entry == "remove" } else { entry != "remove" },
                    "{entry} follows the board's bar/slot split"
                );
            }
            crate::test_support::click(&mut h, &format!("nav:context:{action}"));
            match action {
                "customise" => {
                    assert!(h.app.prefs_window.shell.open);
                    assert_eq!(h.app.prefs_window.shell.section, crate::prefs::PAGE_LAYOUT);
                    assert!(crate::scenario::rect_of("prefs:navbar:preview").is_some());
                }
                "hide" => assert!(!h.app.ui.show_navigation_bar),
                "reset" => assert_eq!(h.app.preferences.navigation_bar, NavigationBar::default()),
                _ => assert_eq!(h.app.preferences.navigation_bar.slots, ["fit"]),
            }
        }
    }

    #[test]
    fn the_view_menu_slot_is_named_nav_view_on_the_bar_and_in_more() {
        let mut h = crate::scenario::Harness::new();
        h.app.ui.show_browser = false;
        h.app.ui.show_timeline = false;
        h.call("ui.prefs", json!({"navigation_bar":{"slots":["display","fit","home"]}}));
        h.settle();
        let on_bar = solidcraft_ui_registry::handle_at("nav:slot:display").expect("the View slot is on the bar");
        assert_eq!(solidcraft_ui_registry::handle_at("nav:view"), Some(on_bar));
        // Many slots before it in a narrow view fold the View slot into More.
        h.call(
            "ui.prefs",
            json!({"navigation_bar":{"slots":["pan","fit","home","visual_style","grid","ground_shadow","ambient_occlusion","projection","spin","orbit","zoom","display"]}}),
        );
        h.call("ui.resize", json!({"width":300,"height":600}));
        h.settle();
        assert!(solidcraft_ui_registry::handle_at("nav:slot:display").is_none() && solidcraft_ui_registry::handle_at("nav:view").is_none(), "folded");
        h.step(&json!({"click":{"handle":"nav:more"}})).unwrap();
        let in_more = solidcraft_ui_registry::handle_at("nav:slot:display").expect("More lists the View slot");
        assert_eq!(solidcraft_ui_registry::handle_at("nav:view"), Some(in_more));
    }

    #[test]
    fn catalogue_slot_descriptions_explain_slots_without_settings_jargon() {
        crate::i18n::install();
        for slot in CATALOGUE {
            for language in ["en", "ja", "zh"] {
                let description = solidcraft_ui_text::Locale::for_language(language).text(slot.description);
                assert!(!description.trim().is_empty() && description != slot.description, "{language}: {} needs a description", slot.id);
                assert!(description.ends_with(['.', '。']), "{language}: {} must be a sentence", slot.id);
                let lower = description.to_lowercase();
                assert!(!lower.contains("icon only") && !lower.contains("icon and value"), "{language}: {} describes settings controls", slot.id);
                // The owner's exact Grid/Shadow copy uses "Shows" as a visibility verb,
                // rather than naming the inspector's Shows setting.
                let words = if language == "en" && matches!(slot.id, "grid" | "ground_shadow") {
                    description.strip_prefix("Shows or hides ").expect("the specified visibility description")
                } else {
                    description
                };
                assert!(
                    !words.split(|c: char| !c.is_alphabetic()).any(|word| word.eq_ignore_ascii_case("shows")),
                    "{language}: {} names the Shows setting",
                    slot.id
                );
            }
        }
        assert!(find("future_slot").is_none(), "unknown slots have no catalogue description");
    }
}
