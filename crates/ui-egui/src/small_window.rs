//! The forms the main window's parts take in a window smaller than [`FULL_LAYOUT`].
//!
//! In a window at least [`FULL_LAYOUT`] in both directions every part keeps its full form. Below
//! it, each part gives up room by one rule, worked out every frame from the window's size and
//! never written to the preferences:
//!
//! - **Height** ([`bands`]): the bands across the window (the app bar, the ribbon, the history
//!   docked along the bottom) take no larger share of its height than they take in the default
//!   layout at [`FULL_LAYOUT`]. To keep to it they give up, in turn, the ribbon's labels, the
//!   ribbon's panels (folded to its tab row, a tab opens them over the view) and the history.
//! - **Width** ([`hidden_columns`]): the panels docked at the sides take no more than half of the
//!   window's width. The widest column goes first, the left one among equals; a button in the view
//!   shows it again.
//! - **Feature cards** ([`card_at_bottom`]): a card that would take more than half of the view's
//!   width goes along the view's bottom instead, at most half of its height, its body scrolling.
//! - **View navigator** ([`navigator_shown`]): it takes at most a third of the view's shorter
//!   side, or it is not shown.
//!
//! A preference that already asks for less (ribbon labels off, the ribbon folded) is kept.

use crate::SolveApp;
use crate::panel_runtime::{SizedPanel, sized_panels};
use crate::ui_kit::BackingTooltipExt as _;
use crate::ui_kit::{Container, SHELL_GAP, Surface};
use egui::{Rect, Sense, Vec2, vec2};
use solidcraft_theme::Tokens;
use std::collections::BTreeSet;

/// The smallest window in which every part of the main window keeps its full form: the window's
/// minimum size before it could shrink to [`crate::ui_kit::window_chrome::MIN_INNER_SIZE`].
pub(crate) const FULL_LAYOUT: Vec2 = vec2(960.0, 600.0);

/// Which bands across the window's height keep their room.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Bands {
    /// The ribbon's buttons carry their labels.
    pub labels: bool,
    /// The ribbon shows its panels under its tab row; folded, a tab opens them over the view.
    pub panels: bool,
    /// The history docked along the bottom is shown.
    pub history: bool,
}

impl Bands {
    pub(crate) const FULL: Self = Self { labels: true, panels: true, history: true };
    /// From the fullest form to the barest, each giving up one more band's room.
    const LADDER: [Self; 4] = [
        Self::FULL,
        Self { labels: false, panels: true, history: true },
        Self { labels: false, panels: false, history: true },
        Self { labels: false, panels: false, history: false },
    ];
}

impl Default for Bands {
    fn default() -> Self {
        Self::FULL
    }
}

/// The heights of the bands across the window, in points.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BandHeights {
    /// The app bar at the top.
    pub bar: f32,
    /// The ribbon with labels, without them, and folded to its tab row.
    pub labelled: f32,
    pub icons: f32,
    pub tabs: f32,
    /// The history docked along the bottom; `None` when it is elsewhere or closed.
    pub history: Option<f32>,
    /// The gap around the ribbon card and above the bottom edge (none in the docked layout).
    pub gap: f32,
}

impl BandHeights {
    /// What the bands take from the window's height in the form `bands`.
    pub(crate) fn of(&self, bands: Bands) -> f32 {
        let ribbon = if !bands.panels {
            self.tabs
        } else if bands.labels {
            self.labelled
        } else {
            self.icons
        };
        let history = self.history.filter(|_| bands.history).unwrap_or(0.0);
        self.bar + 2.0 * self.gap + ribbon + self.gap + history
    }
}

/// The fullest form of the bands in a window `height` tall: the full form at [`FULL_LAYOUT`]'s
/// height and above, below it the fullest whose bands take at most `share` of the height (the
/// barest when none does). A height that is not a number keeps the full form.
pub(crate) fn bands(height: f32, heights: &BandHeights, share: f32) -> Bands {
    if height.is_nan() || height >= FULL_LAYOUT.y {
        return Bands::FULL;
    }
    let mut ladder = Bands::LADDER.into_iter();
    let barest = Bands::LADDER[Bands::LADDER.len() - 1];
    ladder.find(|bands| heights.of(*bands) <= share * height).unwrap_or(barest)
}

/// The share of the window's height the bands take in `default` heights at [`FULL_LAYOUT`].
pub(crate) fn band_share(default: &BandHeights) -> f32 {
    default.of(Bands::FULL) / FULL_LAYOUT.y
}

/// The side columns to hide in a window `window_width` wide, by their index in `widths` (left
/// columns before right ones): none at [`FULL_LAYOUT`]'s width and above; below it the widest,
/// one by one (the earlier one first among equals, so a task's panel on the right outlasts the
/// Browser), until the rest with a `gap` beside each take at most half of the window's width.
pub(crate) fn hidden_columns(window_width: f32, widths: &[f32], gap: f32) -> Vec<usize> {
    if window_width.is_nan() || window_width >= FULL_LAYOUT.x {
        return Vec::new();
    }
    // A width that is not a number takes all the room there is.
    let mut kept: Vec<(usize, f32)> = widths.iter().map(|w| if w.is_nan() { f32::INFINITY } else { *w }).enumerate().collect();
    let mut hidden = Vec::new();
    let taken = |kept: &[(usize, f32)]| kept.iter().map(|(_, width)| width + gap).sum::<f32>();
    while taken(&kept) > window_width / 2.0 {
        // The first of the widest: `min_by` keeps the first of equals.
        let widest = kept.iter().enumerate().min_by(|(_, (_, a)), (_, (_, b))| b.total_cmp(a)).map(|(at, _)| at);
        let Some(at) = widest else { break };
        hidden.push(kept.remove(at).0);
    }
    hidden.sort_unstable();
    hidden
}

/// Whether a feature card `card_width` wide goes along the bottom of a view `view_width` wide:
/// in a window narrower than [`FULL_LAYOUT`], when at the side it would take more than half of
/// the view's width.
pub(crate) fn card_at_bottom(window_width: f32, view_width: f32, card_width: f32) -> bool {
    window_width < FULL_LAYOUT.x && card_width > view_width / 2.0
}

/// Whether the view navigator, `size` points square, shows in a view of `view` size in a window
/// of `window` size: always in a window at least [`FULL_LAYOUT`]; below it while it takes at
/// most a third of the view's shorter side.
pub(crate) fn navigator_shown(window: Vec2, view: Vec2, size: f32) -> bool {
    let full = window.x >= FULL_LAYOUT.x && window.y >= FULL_LAYOUT.y;
    full || size <= view.min_elem() / 3.0
}

/// What the window's size takes from the main window's parts this frame. Not saved.
#[derive(Debug, Default)]
pub(crate) struct Fit {
    pub bands: Bands,
    /// The panels hidden for the window's size, in the order their buttons show.
    pub hidden: Vec<SizedPanel>,
    /// The hidden panels shown again with their button, until the window no longer hides them.
    pub revealed: BTreeSet<&'static str>,
    /// Where the buttons that show hidden panels again are drawn, while they are.
    pub buttons: Option<Rect>,
}

impl Fit {
    /// Whether the panel with instance `id` is hidden for the window's size and not shown again.
    pub(crate) fn hides(&self, id: &str) -> bool {
        self.hidden.iter().any(|panel| panel.id == id) && !self.revealed.contains(id)
    }

    /// Show a hidden panel again, or hide it again.
    pub(crate) fn toggle(&mut self, id: &'static str) {
        if !self.revealed.remove(id) && self.hidden.iter().any(|panel| panel.id == id) {
            self.revealed.insert(id);
        }
    }
}

/// The bands' heights in the ribbon's three forms, with the History's `history` band and `gap`.
fn band_heights(history: Option<f32>, gap: f32) -> BandHeights {
    BandHeights {
        bar: crate::ui_kit::app_bar::HEIGHT,
        labelled: crate::toolbar::ribbon_height(true),
        icons: crate::toolbar::ribbon_height(false),
        tabs: crate::toolbar::TAB_ROW_HEIGHT + 1.0,
        history,
        gap,
    }
}

/// Work out this frame's [`Fit`] from the window's size. An embedded view (no app bar, no ribbon)
/// keeps every part.
pub(crate) fn update(app: &mut SolveApp, ctx: &egui::Context) {
    if app.embedded || app.embed_editor {
        app.shell.small_window = Fit::default();
        return;
    }
    let window = ctx.content_rect().size();
    let panels = sized_panels(app);
    let share = band_share(&band_heights(Some(crate::panel_runtime::bottom_band_height()), SHELL_GAP));
    let gap = Container::for_layout(app.ui.layout, Surface::Chrome).gap();
    let history = panels.as_ref().and_then(|panels| panels.history);
    let bands = bands(window.y, &band_heights(history.map(|(_, height)| height), gap), share);
    let mut hidden: Vec<SizedPanel> = Vec::new();
    if let Some(panels) = &panels {
        let widths: Vec<f32> = panels.columns.iter().map(|(_, width)| *width).collect();
        let panel_gap = Container::for_layout(app.ui.layout, Surface::Panel).gap();
        let columns = hidden_columns(window.x, &widths, panel_gap).into_iter().filter_map(|at| panels.columns.get(at));
        hidden.extend(columns.flat_map(|(column, _)| column.iter().copied()));
    }
    if let Some((panel, _)) = history.filter(|_| !bands.history) {
        hidden.push(panel);
    }
    let fit = &mut app.shell.small_window;
    fit.bands = bands;
    fit.revealed.retain(|id| hidden.iter().any(|panel| panel.id == *id));
    fit.hidden = hidden;
}

/// Each button's side: as large as a navigation bar button.
const TARGET: f32 = 32.0;

/// The fewest columns, from one up to one per button, that `count` buttons can be laid out in
/// with `fits` accepting the grid's size, and the place `fits` gives it: each column `rows` tall,
/// the last one holding what is left.
fn button_grid<T>(count: usize, pitch: f32, inset: f32, mut fits: impl FnMut(Vec2) -> Option<T>) -> Option<(usize, T)> {
    (1..=count).find_map(|columns| {
        let rows = count.div_ceil(columns);
        let columns = count.div_ceil(rows);
        fits(vec2(columns as f32 * pitch + inset, rows as f32 * pitch + inset)).map(|at| (rows, at))
    })
}

/// One button for each panel the window's size hides, in a column at the view's top left; in a
/// view too short for the column, in as few columns as fit its height. A click shows the panel
/// again where it was docked, a second click hides it again. Where the panels shown again leave
/// no room for the buttons, they go over those panels' corner, so a panel shown again can always
/// be hidden again. The buttons are one of the shell's covered rects, so a click on them never
/// picks or navigates.
pub(crate) fn show_buttons(app: &mut SolveApp, ctx: &egui::Context) {
    app.shell.small_window.buttons = None;
    let count = app.shell.small_window.hidden.len();
    if count == 0 || app.home.open {
        return;
    }
    let t = Tokens::get();
    let pitch = TARGET + t.space_4;
    let Some(content) = app.viewport.layout.as_ref().map(|layout| layout.content).filter(|r| r.is_finite() && r.is_positive()) else { return };
    let Ok(region) = crate::shell::host_usable(app) else { return };
    let corner = content.left_top() + vec2(t.space_4, t.space_4);
    let beside = button_grid(count, pitch, t.space_4, |size| {
        let preferred = corner + size / 2.0;
        let footprint = solidcraft_engine::panels::Size { width: size.x, height: size.y };
        region.place_footprint(footprint, solidcraft_engine::panels::Point { x: preferred.x, y: preferred.y }, t.space_4)
    });
    let placed = beside.map(|(rows, at)| (rows, crate::panel_runtime::egui_rect(at), egui::Order::Middle)).or_else(|| {
        let over = button_grid(count, pitch, t.space_4, |size| Some(Rect::from_min_size(corner, size)).filter(|r| content.contains_rect(*r)));
        over.map(|(rows, rect)| (rows, rect, egui::Order::Foreground))
    });
    let Some((rows, rect, order)) = placed else { return };
    let panels = app.shell.small_window.hidden.clone();
    let container = Container::new(ctx, Surface::Navigation);
    let frame = container.frame(ctx, t.overlay, crate::frame::Side::Left);
    let mut clicked = None;
    let mut painted = false;
    container.area(egui::Id::unique("sc_small_window_panels")).order(order).fixed_pos(rect.min).fade_in(false).show(ctx, |ui| {
        painted = ui.is_visible() && !ui.is_sizing_pass();
        let (outer, _) = ui.allocate_exact_size(rect.size(), Sense::hover());
        Container::paint_frame(ui.painter(), frame, outer.shrink(frame.stroke.width));
        for (index, panel) in panels.iter().enumerate() {
            let (column, row) = (index / rows, index % rows);
            let at = Rect::from_min_size(outer.min + vec2(t.space_4 + column as f32 * pitch, t.space_4 + row as f32 * pitch), vec2(TARGET, TARGET));
            let key = format!("small_window:{}", panel.id);
            let response = ui.interact(at, ui.scope_id().with(&key), Sense::click());
            if app.shell.small_window.revealed.contains(panel.id) {
                ui.painter().rect_filled(at, t.radius_sm, t.selected_bg);
            } else if response.hovered() {
                ui.painter().rect_filled(at, t.radius_sm, t.hover);
            }
            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            crate::icons::paint_icon(ui.painter(), at.shrink(t.space_6), panel.icon, 1.0);
            let label = solidcraft_ui_text::tr(panel.title_key);
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, app.shell.small_window.revealed.contains(panel.id), label));
            if painted {
                crate::scenario::publish_target(&key, at);
            }
            if response.backing_on_hover_text(label).clicked() {
                clicked = Some(panel.id);
            }
        }
    });
    if let Some(id) = clicked {
        app.shell.small_window.toggle(id);
    }
    app.shell.small_window.buttons = painted.then_some(rect);
    if painted {
        crate::scenario::publish_rect("small_window:panels", rect);
    }
}

#[cfg(test)]
#[path = "small_window_tests.rs"]
mod tests;
