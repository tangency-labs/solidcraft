//! Transient shell geometry shared by automation, drawing and viewport occlusion.
use crate::ui_kit::{Container, Surface, within};
use egui::{Rect, pos2};

use crate::SolveApp;
use solidcraft_theme::Tokens;

/// Shell panels measured after drawing the full-bleed viewport. No design or preferences data.
#[derive(Default)]
pub(crate) struct State {
    /// Per-window Render view state; neither preferences nor document data.
    pub render_focus: bool,
    pub cards: Vec<Rect>,
    /// Actual workspace overlays above the panel host.
    pub workspace_panels: std::collections::BTreeMap<&'static str, Rect>,
    pub browser: Option<Rect>,
    pub history: Option<Rect>,
    pub content: Option<Rect>,
    /// The status bar along the window's bottom edge, while it is shown (#7243).
    pub status_bar: Option<Rect>,
}

/// Register the actual card, so fitting and pointer shielding use the same rectangle.
pub(crate) fn register_panel(app: &mut SolveApp, id: &'static str, rect: Option<Rect>) {
    let rect = rect.filter(|_| !crate::render_ui::focused(app) || !id.starts_with("render:") || id == "render:focus:exit");
    if let Some(rect) = rect.filter(|r| r.is_finite() && r.is_positive()) {
        app.shell.workspace_panels.insert(id, rect);
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
    available
}

/// One source of occlusion for navigation, picking and overlays.
pub(crate) fn covered_rects(app: &SolveApp) -> Vec<Rect> {
    occluding_rects(app, false, false, true)
}

/// Actual persistent workspace footprints for the shared host. Transient menus
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
    rects.extend(app.shell.workspace_panels.iter().filter(|(id, _)| !fitting || **id != "render:selection").map(|(_, rect)| *rect));
    rects.extend(crate::drawing_workspace::covered_rects(app));
    rects.extend(app.shell.cards.iter().copied());
    rects.extend(app.viewport.palette_rect);
    // Persistent feature windows come from the host's window ledger. This cache also
    // receives transient warnings, so include it only when checking input shields.
    if !fitting {
        rects.extend(app.viewport.dialog_rect);
    }
    if include_confirm {
        rects.extend(crate::view_confirm::rect(app));
    }
    rects.extend(app.shell.status_bar.filter(|_| crate::status_bar::shown(app)));
    rects.retain(|r| r.is_finite() && r.is_positive());
    rects
}

pub(crate) fn covered(app: &SolveApp, p: egui::Pos2) -> bool {
    occluding_rects(app, false, true, true).iter().any(|r| r.contains(p))
}

/// The full workspace remains the anchor for independent controls.
pub(crate) fn content_rect(app: &SolveApp, viewport: Rect) -> Rect {
    // The content kept from the last frame can lie outside a viewport that has shrunk or moved
    // since (a resize, a new scale): the intersection is then turned inside out, its min past
    // its max. The viewport stands in for it.
    let content = app.shell.content.unwrap_or(viewport).intersect(viewport);
    if content.is_positive() { content } else { viewport }
}

/// The shared host supplies the free workspace bounds when a layout is available.
pub(crate) fn workspace_content(app: &SolveApp, content: Rect) -> Rect {
    let mut content = app
        .universal_panel_frame()
        .and_then(|frame| frame.layout.viewport)
        .map_or(content, |rect| crate::panel_runtime::egui_rect(rect).intersect(content));
    // The drawing's properties panel takes its side of the sheet's room.
    if let Some(settings) = crate::drawing_workspace::settings_rect(app) {
        let gap = Container::for_layout(app.ui.layout, Surface::Panel).gap();
        if app.preferences.drawing_panel_side == "left" {
            content.min.x = (settings.right() + gap).clamp(content.left(), content.right());
        } else {
            content.max.x = (settings.left() - gap).clamp(content.left(), content.right());
        }
    }
    content
}

/// Keep bottom readouts above the current shared-host bottom panels.
pub(crate) fn bottom_readout_rect(app: &SolveApp, mut viewport: Rect) -> Rect {
    use solidcraft_engine::panels::Zone;
    let Some(frame) = app.universal_panel_frame() else { return viewport };
    let gap = Tokens::get().space_8;
    for section in &frame.layout.sections {
        let bounds = crate::panel_runtime::egui_rect(section.outer);
        let is_bottom_history = section.zone == Zone::Floating && section.instance.0 == "history" && bounds.center().y >= viewport.center().y;
        if (section.zone == Zone::Bottom || is_bottom_history) && bounds.intersect(viewport).is_positive() {
            viewport.max.y = within(bounds.top() - gap, viewport.top(), viewport.bottom());
        }
    }
    viewport
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

/// Keep a newly opened editor beside the visible Sketch Options panel.
pub(crate) fn editor_space(app: &SolveApp, mut viewport: Rect) -> Rect {
    if let Some(palette) = app.viewport.palette_rect
        && palette.intersects(viewport)
        && palette.center().x >= viewport.center().x
    {
        let gap = Container::for_layout(app.ui.layout, Surface::Feature).gap();
        let right = palette.left() - gap;
        if right > viewport.center().x {
            viewport.max.x = viewport.right().min(right);
        }
    }
    viewport
}

/// Initial placement for floating editor windows in the shared-panel workspace.
pub(crate) fn floating_editor_anchor(app: &SolveApp, ctx: &egui::Context) -> (egui::Align2, egui::Pos2) {
    let viewport = crate::agent_chat::tool_space(app, content_rect(app, app.viewport.rect.unwrap_or_else(|| ctx.content_rect())));
    let viewport = editor_space(app, viewport);
    let side = crate::frame::Side::Right;
    (egui::Align2::RIGHT_TOP, pos2(side.anchor_x(viewport), panel_top(app, viewport, viewport.bottom(), side)))
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
