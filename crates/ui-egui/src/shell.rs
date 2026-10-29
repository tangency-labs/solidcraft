//! Transient shell geometry shared by automation, drawing and viewport occlusion.
use crate::ui_kit::{Container, Surface};
use egui::{Rect, pos2, vec2};
use serde::{Deserialize, Serialize};

use crate::{SolveApp, theme::Tokens, toolbar};

/// Saved workspace layout; automation may override it for a session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LayoutMode {
    #[default]
    Floating,
    #[serde(rename = "compact", alias = "docked")]
    Docked,
}

impl LayoutMode {
    pub(crate) fn publish(self, ctx: &egui::Context) {
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("sc_container_layout"), self));
    }

    pub(crate) fn current(ctx: &egui::Context) -> Self {
        ctx.data(|data| data.get_temp(egui::Id::new("sc_container_layout"))).unwrap_or_default()
    }
}

/// The gap from the viewport edge to the floating cards and between the cards.
pub(crate) const SHELL_GAP: f32 = 7.0;

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

#[derive(Clone, Copy, Debug)]
pub(crate) struct Cards {
    pub workspace: Rect,
    pub ribbon: Rect,
    pub bottom: f32,
}

impl Cards {
    pub fn new(available: Rect, mode: LayoutMode, ribbon_height: f32) -> Self {
        let margin = Container::for_layout(mode, Surface::Chrome).gap();
        let gap = margin;
        let left = available.left() + margin;
        let top = available.top() + margin;
        let right = (available.right() - margin).max(left);
        let workspace = Rect::from_min_size(
            pos2(left, top),
            vec2(
                toolbar::WORKSPACE_WIDTH.min((right - left).max(0.0)),
                ribbon_height - if !Container::for_layout(mode, Surface::Chrome).floating() { 1.0 } else { 0.0 },
            ),
        );
        let ribbon = Rect::from_min_max(pos2((workspace.right() + gap).min(right), top), pos2(right, top + ribbon_height));
        Self { workspace, ribbon, bottom: ribbon.bottom() + margin }
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
        available.min.y = card.bottom().clamp(available.top(), available.bottom());
    }
    if crate::render_workspace::active(app) {
        let bottom = app.shell.cards.iter().map(|r| r.bottom()).fold(available.top() - Tokens::get().space_8, f32::max);
        available.min.y = (bottom + Tokens::get().space_8).min(available.bottom());
    }
    for (rect, edge) in app.shell.workspace_panels.values() {
        if matches!(edge, Some(crate::ui_kit::Edge::Bottom)) {
            available.max.y = rect.top().clamp(available.top(), available.bottom());
        }
    }
    available
}

/// One source of occlusion for navigation, picking and overlays.
pub(crate) fn covered_rects(app: &SolveApp) -> Vec<Rect> {
    occluding_rects(app, false)
}

/// Selection controls follow the projected model, so they must not influence its fit.
pub(crate) fn fit_rects(app: &SolveApp) -> Vec<Rect> {
    occluding_rects(app, true)
}

fn occluding_rects(app: &SolveApp, fitting: bool) -> Vec<Rect> {
    let mut rects = Vec::new();
    rects.extend(app.shell.workspace_panels.iter().filter(|(id, _)| !fitting || **id != "render:selection").map(|(_, (rect, _))| *rect));
    if crate::render_workspace::active(app) || Container::for_layout(app.ui.layout, Surface::Panel).floating() {
        rects.extend(app.shell.cards.iter().copied());
        rects.extend(app.shell.panels.iter().copied());
        if crate::agent_chat::shown(app) && app.agent_chat.open && !app.preferences.agent_chat_popped {
            rects.extend(app.agent_chat.panel_rect);
        }
    }
    rects.extend(app.shell.history.filter(|_| app.ui.show_timeline));
    if app.ui.show_timeline && app.preferences.timeline_left {
        rects.extend(app.shell.history.map(|card| crate::ui_kit::HistorySeam::new(card).pill));
    }
    rects.extend(app.shell.browser.filter(|_| app.ui.show_browser));
    rects.extend(app.viewport.palette_rect.filter(|_| app.session.active_sketch.is_some()));
    rects.extend(app.viewport.dialog_rect.filter(|_| app.dialog.is_some() || app.tree.redefine.is_some()));
    rects.extend(crate::view_confirm::rect(app));
    rects.extend(app.shell.status_bar.filter(|_| crate::status_bar::shown(app)));
    rects.retain(|r| r.is_finite() && r.is_positive());
    rects
}

pub(crate) fn covered(app: &SolveApp, p: egui::Pos2) -> bool {
    covered_rects(app).iter().any(|r| r.contains(p))
}

/// Space reserved below viewport overlays by the floating History card. Its rect is set before
/// the viewport is painted, so showing or hiding History takes effect in the same frame. History
/// docked as the left sidebar sits beside the viewport, not below it, so it reserves nothing.
pub(crate) fn viewport_bottom_inset(app: &SolveApp, viewport: Rect) -> f32 {
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
    app.shell.content.unwrap_or(viewport).intersect(viewport)
}

/// Free anchor space shared by both layouts. Compact edge hosts have already consumed
/// their allocation; content-hugging Browser and floating overlays reserve their measured span.
pub(crate) fn workspace_content(app: &SolveApp, mut content: Rect) -> Rect {
    if crate::render_workspace::active(app) {
        if crate::render_ui::focused(app) {
            return content;
        }
        let gap = Container::floating_card(Surface::Panel).gap();
        for (rect, edge) in app.shell.workspace_panels.values() {
            match edge {
                Some(crate::ui_kit::Edge::Left) => content.min.x = (rect.right() + gap).clamp(content.left(), content.right()),
                Some(crate::ui_kit::Edge::Right) => content.max.x = (rect.left() - gap).clamp(content.left(), content.right()),
                Some(crate::ui_kit::Edge::Bottom) => content.max.y = (rect.top() - gap).clamp(content.top(), content.bottom()),
                Some(crate::ui_kit::Edge::Top) => content.min.y = (rect.bottom() + gap).clamp(content.top(), content.bottom()),
                None => {}
            }
        }
        return content;
    }
    let gap = Container::for_layout(app.ui.layout, Surface::Panel).gap();
    if let Some(history) = app.shell.history {
        if app.preferences.timeline_left {
            content.min.x = (history.right() + gap).clamp(content.left(), content.right());
        } else {
            content.max.y = (history.top() - gap).clamp(content.top(), content.bottom());
        }
    }
    if let Some(browser) = app.shell.browser {
        content.min.x = (browser.right() + gap).clamp(content.left(), content.right());
    }
    if let Some(column) = app.shell.dialog_column {
        content.min.x = (column.right() + gap).clamp(content.left(), content.right());
    }
    content
}

/// Tool sheets may cover Browser and History, while shell chrome and Assistant stay clear.
/// Assistant's outside-rail exception stays in its owner.
pub(crate) fn tool_drag_rect(app: &SolveApp, placement: Rect) -> Rect {
    let gap = Container::for_layout(app.ui.layout, Surface::Feature).gap();
    let mut bounds = crate::agent_chat::tool_space(app, app.viewport.rect.unwrap_or(placement));
    for card in app.shell.cards.iter().filter(|r| r.is_finite() && r.is_positive()) {
        bounds.min.y = (card.bottom() + gap).clamp(bounds.top(), bounds.bottom());
    }
    bounds
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
