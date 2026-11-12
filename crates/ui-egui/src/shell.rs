//! Transient shell geometry shared by automation, drawing and viewport occlusion.
use crate::ui_kit::{Container, LayoutMode, SHELL_GAP, Surface, within};
use egui::{Rect, pos2};

use crate::SolveApp;
use solidcraft_theme::Tokens;

/// The viewport's top corners are rounded concentrically with the cards: the card margin plus the
/// card radius.
pub(crate) fn corner_radius() -> f32 {
    Tokens::outset_radius(f32::from(Tokens::get().radius_lg), SHELL_GAP)
}

/// Fill the two concave corners where the application bar flows into the viewport's top edge, so
/// the viewport reads as a rounded surface under the bar.
pub(crate) fn paint_top_corners(painter: &egui::Painter, viewport: Rect, color: egui::Color32) {
    let r = corner_radius().min(viewport.width() / 2.0).min(viewport.height() / 2.0);
    if !(r.is_finite() && r > 0.5) {
        return;
    }
    const STEPS: usize = 32;
    for left in [true, false] {
        let (corner, centre, dir) = if left {
            (viewport.left_top(), pos2(viewport.left() + r, viewport.top() + r), -1.0)
        } else {
            (viewport.right_top(), pos2(viewport.right() - r, viewport.top() + r), 1.0)
        };
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(corner, color);
        for i in 0..=STEPS {
            // From the top tangent point round to the side tangent point.
            let a = std::f32::consts::FRAC_PI_2 * i as f32 / STEPS as f32;
            mesh.colored_vertex(pos2(centre.x + dir * r * a.sin(), centre.y - r * a.cos()), color);
        }
        for i in 0..STEPS as u32 {
            mesh.add_triangle(0, i + 1, i + 2);
        }
        painter.add(egui::Shape::mesh(mesh));
    }
}

/// Shell panels measured after drawing the full-bleed viewport. No design or preferences data.
#[derive(Default)]
pub(crate) struct State {
    /// Per-window Render view state; neither preferences nor document data.
    pub render_focus: bool,
    pub render_browser_open: bool,
    pub cards: Vec<Rect>,
    pub panels: Vec<Rect>,
    /// Actual floating workspace surfaces and their optional canvas reservation.
    pub workspace_panels: std::collections::BTreeMap<&'static str, (Rect, Option<crate::ui_kit::Edge>)>,
    pub browser: Option<Rect>,
    /// The browser set to reveal on hover is out of sight this frame (#8692); its place stays
    /// in `browser`, which the pointer reveals.
    pub browser_hidden: bool,
    pub history: Option<Rect>,
    /// In-memory seam drag width, never serialized as a preference.
    pub history_drag_width: Option<f32>,
    /// Unreserved canvas bounds used by History docking targets.
    pub history_dock_space: Option<Rect>,
    /// Browser/history collapse progress shared by both cards for this frame.
    pub history_folding: Option<[f32; 2]>,
    pub history_fold_motion: Option<[crate::timeline_left::FoldMotion; 2]>,
    pub content: Option<Rect>,
    /// The space the browser and a left History are laid out in this frame.
    pub left_space: Option<Rect>,
    /// The left column an open dialog holds this frame (`dialog_column`).
    pub dialog_column: Option<Rect>,
    /// The design tree over the view while a dialog holds the left column (`dialog_tree`), and
    /// whether it is open (folded otherwise; each dialog starts with it folded).
    pub dialog_tree: Option<Rect>,
    pub dialog_tree_open: bool,
    /// The status bar along the window's bottom edge, while it is shown (#7243).
    pub status_bar: Option<Rect>,
}

/// Register the actual card, so fitting and pointer shielding use the same rectangle.
pub(crate) fn register_panel(app: &mut SolveApp, id: &'static str, rect: Option<Rect>, edge: Option<crate::ui_kit::Edge>) {
    let rect = rect.filter(|_| !crate::render_ui::focused(app) || !id.starts_with("render:") || id == "render:focus:exit");
    if let Some(rect) = rect.filter(|r| r.is_finite() && r.is_positive()) {
        app.shell.workspace_panels.insert(id, (rect, edge));
    } else {
        app.shell.workspace_panels.remove(id);
    }
}

/// Shared workspace-card bounds below the ribbon and above a floating bottom strip.
pub(crate) fn panel_bounds(app: &SolveApp, mut available: Rect) -> Rect {
    for card in &app.shell.cards {
        available.min.y = within(card.bottom(), available.top(), available.bottom());
    }
    if crate::render_workspace::active(app) {
        let bottom = app.shell.cards.iter().map(|r| r.bottom()).fold(available.top() - Tokens::get().space_8, f32::max);
        available.min.y = (bottom + Tokens::get().space_8).min(available.bottom());
    }
    for (rect, edge) in app.shell.workspace_panels.values() {
        if matches!(edge, Some(crate::ui_kit::Edge::Bottom)) {
            available.max.y = within(rect.top(), available.top(), available.bottom());
        }
    }
    available
}

/// One source of occlusion for navigation, picking and overlays.
pub(crate) fn covered_rects(app: &SolveApp) -> Vec<Rect> {
    occluding_rects(app, false, false, true)
}

/// Selection controls follow the projected model, so they must not influence its fit.
pub(crate) fn fit_rects(app: &SolveApp) -> Vec<Rect> {
    occluding_rects(app, true, false, true)
}

/// Actual persistent workspace footprints for the opt-in host. Transient menus
/// remain input shields, without becoming targets for framing or control placement.
pub(crate) fn host_usable(app: &SolveApp) -> solidcraft_engine::panels::PanelResult<crate::panel_runtime::HostUsableRegion> {
    use crate::panel_runtime::{MAX_USABLE_OBSTACLES, egui_rect, logical_rect, resolve_usable_region};
    use solidcraft_engine::panels::{Rule, error};
    let frame = app.universal_panel_frame().ok_or_else(|| error(Rule::Lifecycle, "universal host geometry is unavailable"))?;
    let windows = app.viewport.host_windows.as_ref().ok_or_else(|| error(Rule::CollectionLimit, "persistent window footprint limit"))?;
    let mut obstacles = frame.layout.usable.obstacles.clone();
    for bounds in occluding_rects(app, true, false, false).into_iter().chain(windows.iter().copied()).chain(app.viewport.host_confirm) {
        if !bounds.is_positive() || !bounds.is_finite() || !bounds.intersect(egui_rect(frame.layout.render_domain)).is_positive() {
            continue;
        }
        let bounds = logical_rect(bounds);
        bounds.validate()?;
        if !obstacles.contains(&bounds) {
            if obstacles.len() >= MAX_USABLE_OBSTACLES {
                return Err(error(Rule::CollectionLimit, "effective usable-space obstacle limit"));
            }
            obstacles.push(bounds);
        }
    }
    resolve_usable_region(frame.layout.render_domain, &obstacles)
}

fn occluding_rects(app: &SolveApp, fitting: bool, input: bool, include_confirm: bool) -> Vec<Rect> {
    let mut rects = Vec::new();
    if let Some(frame) = app.universal_panel_frame() {
        let regions = if input { &frame.layout.input } else { &frame.layout.occlusion };
        rects.extend(regions.iter().copied().map(crate::panel_runtime::egui_rect));
        if !fitting {
            rects.extend(frame.menu_bounds.map(crate::panel_runtime::egui_rect));
            rects.extend(frame.owned_overlays.iter().map(|region| crate::panel_runtime::egui_rect(region.bounds)));
        }
    }
    rects.extend(app.shell.workspace_panels.iter().filter(|(id, _)| !fitting || **id != "render:selection").map(|(_, (rect, _))| *rect));
    if app.universal_panel_host_enabled() || crate::render_workspace::active(app) || Container::for_layout(app.ui.layout, Surface::Panel).floating() {
        rects.extend(app.shell.cards.iter().copied());
        rects.extend(app.shell.panels.iter().copied());
        if !app.universal_panel_host_enabled() && crate::agent_chat::shown(app) && app.agent_chat.open && !app.preferences.agent_chat_popped {
            rects.extend(app.agent_chat.panel_rect);
        }
    }
    if !app.universal_panel_host_enabled() {
        rects.extend(app.shell.history.filter(|_| app.ui.show_timeline));
        rects.extend(app.library.shown);
        if app.ui.show_timeline && app.preferences.timeline_left {
            rects.extend(app.shell.history.map(|card| crate::ui_kit::HistorySeam::new(card).pill));
        }
        rects.extend(app.shell.browser.filter(|_| app.ui.show_browser));
    }
    rects.extend(app.viewport.palette_rect.filter(|_| app.universal_panel_host_enabled() || app.session.active_sketch.is_some()));
    // The host's persistent Window ledger supplies real feature/redefine
    // paint. This legacy cache is also written by transient modal warnings.
    if !fitting || !app.universal_panel_host_enabled() {
        rects.extend(app.viewport.dialog_rect.filter(|_| app.universal_panel_host_enabled() || app.dialog.is_some() || app.tree.redefine.is_some()));
    }
    if include_confirm {
        rects.extend(crate::view_confirm::rect(app));
    }
    rects.extend(app.shell.dialog_tree.filter(|_| app.dialog.is_some()));
    rects.extend(app.shell.status_bar.filter(|_| crate::status_bar::shown(app)));
    rects.retain(|r| r.is_finite() && r.is_positive());
    rects
}

pub(crate) fn covered(app: &SolveApp, p: egui::Pos2) -> bool {
    occluding_rects(app, false, true, true).iter().any(|r| r.contains(p))
}

/// Space reserved below viewport overlays by the floating History card. Its rect is set before
/// the viewport is painted, so showing or hiding History takes effect in the same frame. History
/// docked as the left sidebar sits beside the viewport, not below it, so it reserves nothing.
pub(crate) fn viewport_bottom_inset(app: &SolveApp, viewport: Rect) -> f32 {
    if app.universal_panel_host_enabled() {
        return 0.0;
    }
    if app.ui.layout != LayoutMode::Floating || (app.ui.show_timeline && app.preferences.timeline_left) {
        return 0.0;
    }
    app.shell
        .history
        .filter(|r| r.is_finite() && r.width() >= 1.0 && r.height() >= 30.0)
        .map_or(0.0, |history| (viewport.bottom() - history.top() + SHELL_GAP).clamp(0.0, viewport.height().max(0.0)))
}

/// Bottom-anchored overlays share the content area's History clearance without moving the
/// viewport's projection or its left and right anchors.
pub(crate) fn bottom_overlay_rect(app: &SolveApp, mut viewport: Rect) -> Rect {
    viewport.max.y -= viewport_bottom_inset(app, viewport);
    viewport
}

/// The former central panel remains the anchor for controls and docked panels.
pub(crate) fn content_rect(app: &SolveApp, viewport: Rect) -> Rect {
    // The content kept from the last frame can lie outside a viewport that has shrunk or moved
    // since (a resize, a new scale): the intersection is then turned inside out, its min past
    // its max. The viewport stands in for it.
    let content = app.shell.content.unwrap_or(viewport).intersect(viewport);
    if content.is_positive() { content } else { viewport }
}

/// Free anchor space shared by both layouts. Compact edge hosts have already consumed
/// their allocation; content-hugging Browser and floating overlays reserve their measured span.
pub(crate) fn workspace_content(app: &SolveApp, mut content: Rect) -> Rect {
    if let Some(frame) = app.universal_panel_frame() {
        return frame.layout.viewport.map(crate::panel_runtime::egui_rect).map_or(content, |rect| rect.intersect(content));
    }
    if crate::render_workspace::active(app) {
        if crate::render_ui::focused(app) {
            return content;
        }
        let gap = Container::floating_card(Surface::Panel).gap();
        for (rect, edge) in app.shell.workspace_panels.values() {
            match edge {
                Some(crate::ui_kit::Edge::Left) => content.min.x = within(rect.right() + gap, content.left(), content.right()),
                Some(crate::ui_kit::Edge::Right) => content.max.x = within(rect.left() - gap, content.left(), content.right()),
                Some(crate::ui_kit::Edge::Bottom) => content.max.y = within(rect.top() - gap, content.top(), content.bottom()),
                Some(crate::ui_kit::Edge::Top) => content.min.y = within(rect.bottom() + gap, content.top(), content.bottom()),
                None => {}
            }
        }
        return content;
    }
    let gap = Container::for_layout(app.ui.layout, Surface::Panel).gap();
    if let Some(history) = app.shell.history {
        if app.preferences.timeline_left {
            content.min.x = within(history.right() + gap, content.left(), content.right());
        } else {
            content.max.y = within(history.top() - gap, content.top(), content.bottom());
        }
    }
    if let Some(browser) = app.shell.browser {
        content.min.x = within(browser.right() + gap, content.left(), content.right());
    }
    if let Some(column) = app.shell.dialog_column {
        content.min.x = within(column.right() + gap, content.left(), content.right());
    }
    content
}

/// Tool sheets may cover Browser and History, while shell chrome and Assistant stay clear.
/// Assistant's outside-rail exception stays in its owner.
pub(crate) fn tool_drag_rect(app: &SolveApp, placement: Rect) -> Rect {
    let gap = Container::for_layout(app.ui.layout, Surface::Feature).gap();
    let mut bounds = crate::agent_chat::tool_space(app, app.viewport.rect.unwrap_or(placement));
    for card in app.shell.cards.iter().filter(|r| r.is_finite() && r.is_positive()) {
        bounds.min.y = within(card.bottom() + gap, bounds.top(), bounds.bottom());
    }
    bounds
}

/// Initial placement for floating editor windows in the shared-panel workspace.
pub(crate) fn floating_editor_anchor(app: &SolveApp, ctx: &egui::Context) -> Option<(egui::Align2, egui::Pos2)> {
    if !crate::panel_runtime::production_active(app) {
        return None;
    }
    let viewport = crate::agent_chat::tool_space(app, content_rect(app, app.viewport.rect.unwrap_or_else(|| ctx.content_rect())));
    let side = crate::frame::Side::Right;
    Some((egui::Align2::RIGHT_TOP, pos2(side.anchor_x(viewport), panel_top(app, viewport, viewport.bottom(), side))))
}

pub(crate) fn floating_editor_side(app: &SolveApp, use_left: bool) -> crate::frame::Side {
    if use_left && !crate::panel_runtime::production_active(app) { crate::frame::Side::Left } else { crate::frame::Side::Right }
}

pub(crate) fn panel_top(app: &SolveApp, viewport: Rect, bottom: f32, side: crate::frame::Side) -> f32 {
    let content = content_rect(app, viewport);
    if side == crate::frame::Side::Right {
        // Both layouts use the measured compass, rather than a legacy fixed-height gap.
        // Reclaim that space on short views while keeping the panel usable.
        app.viewport
            .layout
            .as_ref()
            .map_or(content.top(), |l| if l.axis_view.is_positive() { l.axis_view.bottom().min(bottom) } else { content.top() })
            .min((bottom - 360.0).max(content.top()))
    } else {
        (viewport.top() + crate::viewport::AXIS_VIEW_CLEARANCE).min((bottom - 360.0).max(viewport.top()))
    }
}

#[cfg(test)]
#[path = "shell_tests.rs"]
mod tests;
