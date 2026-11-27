//! The window showing several views of the design at once: the area split by the design's
//! [`Tiles`]. Each workspace paints its own canvas in the view's fixed frame.
//!
//! The focused view is the *ground*: its picture fills the window behind the ribbon, the panels and
//! the other views, with no frame and no header, so the app feels the same as with one view. The
//! others are *cards*: laid out below the ribbon, each with a frame, a shadow, a header and a
//! compass of its own, so they stand off the ground and a header is never hidden.
//!
//! Every view has one frame, its cell below a header band, and frames the model there whether it is
//! the ground or a card: a change of focus changes only how the view is drawn, never where its
//! content lies or how large it is ([`crate::viewport::ViewportState::bleed`] fills the rest).
//!
//! The focused view is the design's shown view (what the ribbon, the browser and the commands act
//! on); the cards stand in for it while they are drawn ([`crate::documents::with_view_look`]), so
//! a tile needs no code of its own for orbiting, picking or analysing. A press inside a view makes
//! it the focused one. Every view reaches the GPU as a tile of its own
//! ([`solidcraft_viewport_egui::TileCallback`]).

use egui::{CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, UiBuilder, pos2, vec2};
use solidcraft_theme::Tokens;
use solidcraft_views::{Axis, Edge, Placement, Rect as TileRect, Tiles, ViewKind, ViewSet};

use crate::SolveApp;
use crate::ui_kit::BackingTooltipExt as _;

/// The space between neighbouring views.
const GUTTER: f32 = 12.0;
/// The space between the cards and the edge of the free workspace.
const MARGIN: f32 = 8.0;
/// Height of a card's header.
pub(crate) const HEADER: f32 = 28.0;
/// Corner radius of a card.
const CARD_RADIUS: u8 = 8;

/// The window shows more than one view of the design.
pub(crate) fn shown(app: &SolveApp) -> bool {
    app.docs.views.is_tiled()
}

/// Scope workspace-specific canvas and navigation behaviour to an unfocused tile.
fn with_card_kind<T>(app: &mut SolveApp, id: u64, draw: impl FnOnce(&mut SolveApp) -> T) -> T {
    match app.docs.views.get(id).map(|view| view.kind) {
        Some(ViewKind::Render) => crate::render_workspace::with_tile_camera(app, draw),
        Some(ViewKind::Model) if crate::render_workspace::active(app) || crate::drawing_workspace::active(app) => {
            let tab = std::mem::replace(&mut app.ui.tab, "MODEL".into());
            let result = draw(app);
            app.ui.tab = tab;
            result
        }
        _ => draw(app),
    }
}

fn with_card_camera(app: &mut SolveApp, id: u64, draw: impl FnOnce(&mut SolveApp)) {
    let before = app.docs.views.get(id).and_then(|view| view.spec).map(|spec| spec.camera);
    crate::documents::with_view_look(app, id, |app| with_card_kind(app, id, draw));
    if let Some(after) = app.docs.views.get(id).and_then(|view| view.spec).map(|spec| spec.camera) {
        crate::documents::link_visible_cameras(app, id, after, before);
    }
}

pub(crate) fn reset_cameras(app: &mut SolveApp) {
    let active = app.docs.views.active().id;
    let home = |app: &mut SolveApp| {
        app.frame_view(true);
        if let Some(animation) = app.cam_anim.take() {
            app.cam = animation.to;
        }
    };
    let ids: Vec<_> =
        app.docs.views.views().iter().filter(|view| matches!(view.kind, ViewKind::Model | ViewKind::Render)).map(|view| view.id).collect();
    for id in ids {
        if id == active {
            home(app);
        } else {
            crate::documents::with_view_look(app, id, |app| with_card_kind(app, id, home));
        }
    }
    crate::documents::keep_look(app);
    crate::documents::align_linked_cameras(app);
}

fn draw_canvas(app: &mut SolveApp, ui: &mut egui::Ui, kind: ViewKind) {
    match kind {
        ViewKind::Render => {
            crate::dim_view::set_model_drawn(Vec::new());
            crate::render_workspace::show(app, ui);
        }
        ViewKind::Sheet => {
            crate::dim_view::set_model_drawn(Vec::new());
            let id = app.viewport.compass_view.unwrap_or_else(|| app.docs.views.active().id);
            crate::drawing_sheet_page::show_view(app, ui, id);
        }
        _ => crate::viewport::show(app, ui),
    }
}

fn to_tile(rect: Rect) -> TileRect {
    TileRect { x: rect.min.x, y: rect.min.y, w: rect.width(), h: rect.height() }
}

fn from_tile(rect: TileRect) -> Rect {
    Rect::from_min_size(pos2(rect.x, rect.y), vec2(rect.w, rect.h))
}

/// The tile's slot on the GPU: the view's own id.
fn slot(view: u64) -> u64 {
    view
}

/// A view's frame: where its camera frames the model, the same whether the view is the ground or a
/// card, so a change of focus never moves what is drawn.
fn frame_of(cell: Rect) -> Rect {
    Rect::from_min_max(cell.min + vec2(0.0, HEADER), cell.max)
}

/// `rect` moved right until no panel stands over it, so a name or a chip stays readable.
fn clear_of_panels(app: &SolveApp, mut rect: Rect) -> Rect {
    let covers = crate::shell::covered_rects(app);
    for _ in 0..4 {
        let Some(cover) = covers.iter().find(|c| c.intersects(rect)) else { break };
        rect = rect.translate(vec2(cover.right() + MARGIN - rect.left(), 0.0));
    }
    rect
}

/// How many shapes of the ground's picture are laid under the cards.
const BACKDROP_SLOTS: usize = 6;

/// Moves the shapes the ground painted in `from` to the `slots` set aside before the cards, so the
/// cards lie over the picture although the ground is processed last.
fn lay_under(ctx: &egui::Context, layer: egui::LayerId, slots: &[egui::layers::ShapeIdx], from: std::ops::Range<usize>) {
    ctx.graphics_mut(|g| {
        let list = g.entry(layer);
        let moved: Vec<_> = list.all_entries().skip(from.start).take(from.len().min(slots.len())).cloned().collect();
        for (i, entry) in moved.into_iter().enumerate() {
            list.set(slots[i], entry.clip_rect, entry.shape);
            list.reset_shape(egui::layers::ShapeIdx(from.start + i));
        }
    });
}

fn layout_bounds(app: &SolveApp, area: Rect) -> (Rect, Rect) {
    // Below the ribbon and above the bottom strip: they float over the ground, but a card is never put under them.
    let ground = crate::shell::bottom_readout_rect(
        app,
        crate::shell::panel_bounds(app, crate::shell::workspace_content(app, crate::shell::content_rect(app, area))),
    );
    let cells = ground.shrink(MARGIN);
    (if cells.is_positive() { cells } else { area }, if ground.is_positive() { ground } else { area })
}

/// The transient layout while a view is held. Removing the cell collapses its branch,
/// while the real layout stays intact until a drop succeeds.
fn without_lifted(views: &ViewSet, view: u64) -> Option<ViewSet> {
    let tiles = views.tiles()?;
    let neighbor = tiles.views().into_iter().find(|id| *id != view && *id != solidcraft_views::EMPTY)?;
    if !tiles.contains(view) {
        return None;
    }
    let mut next = views.clone();
    if next.active().id == view {
        next.activate(neighbor);
    }
    next.unshow(view).then_some(next)
}

fn visible_views(app: &SolveApp, ctx: &egui::Context) -> Option<ViewSet> {
    app.docs
        .views
        .views()
        .iter()
        .find(|view| crate::view_window_drag::unanchored(app, ctx, view.id))
        .and_then(|view| without_lifted(&app.docs.views, view.id))
}

/// Paint and handle the temporary ground with its own view state. The source view
/// remains focused in the document, so Escape restores its layout and camera.
pub(crate) fn with_visible_ground<T>(app: &mut SolveApp, ctx: &egui::Context, draw: impl FnOnce(&mut SolveApp) -> T) -> T {
    let Some(visible) = visible_views(app, ctx) else { return draw(app) };
    let ground = visible.active().id;
    let previous = app.docs.views.active().id;
    crate::documents::with_view_look(app, ground, |app| {
        app.docs.views.activate(ground);
        let result = with_card_kind(app, ground, draw);
        app.docs.views.activate(previous);
        result
    })
}

pub(crate) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    with_visible_ground(app, &ctx, |app| show_views(app, ui));
}

fn show_views(app: &mut SolveApp, ui: &mut egui::Ui) {
    crate::documents::sync_linked_active(app);
    let area = ui.max_rect();
    let visible = visible_views(app, ui.ctx());
    let views = visible.as_ref().unwrap_or(&app.docs.views);
    let tiles = views.tiles().cloned().unwrap_or(Tiles::Leaf(views.active().id));
    let focus = app.docs.views.active().id;
    let headers = crate::view_windows::child_options(app).card_headers;
    // Tile chrome (headers, the focused view's name) only tells views apart: with one view, none.
    let several = crate::ui_kit::disambiguates(tiles.views().len());
    let render_ground = app.docs.views.active().kind == ViewKind::Render;
    let (free, _) = layout_bounds(app, area);
    let mut places: Vec<(u64, Rect)> = tiles.rects(to_tile(free), GUTTER).into_iter().map(|(id, r)| (id, from_tile(r))).collect();
    // The focused view is drawn last: what it leaves in the app (the viewport's rectangle, its
    // projection for the panels) is what the rest of the frame reads.
    places.sort_by_key(|(id, _)| *id == focus);
    // The cells with no view in them ask what to show instead of drawing one.
    let empties: Vec<Rect> = places.iter().filter(|(id, _)| *id == solidcraft_views::EMPTY).map(|(_, cell)| *cell).collect();
    places.retain(|(id, _)| *id != solidcraft_views::EMPTY);
    // A foreground popover (such as a card's display menu) owns presses over the canvas.
    let press = ui
        .input(|i| i.pointer.any_pressed().then(|| i.pointer.interact_pos()).flatten())
        .filter(|p| app.docs.view_placement.is_none() && ui.is_enabled() && ui.rect_contains_pointer(area) && !crate::shell::covered(app, *p));
    app.viewport.pending_tile_focus = None;
    let mut cards = Vec::new();
    let mut ground = free;
    // Set aside under everything the cards draw: the ground's picture fills the whole area.
    let layer = ui.layer_id();
    if render_ground {
        let (top, middle, bottom) = crate::render_workspace::tile_background(app);
        solidcraft_viewport_egui::paint_background(ui.painter(), area, top, middle, bottom);
    } else if crate::view_window_drag::unanchored(app, ui.ctx(), focus) {
        let t = Tokens::get();
        solidcraft_viewport_egui::paint_background(ui.painter(), area, t.viewport_top, t.viewport_middle, t.viewport_bottom);
    }
    let slots: Vec<_> = (0..BACKDROP_SLOTS).map(|_| ui.painter().add(egui::Shape::Noop)).collect();
    for (id, cell) in places {
        let is_ground = id == focus;
        let frame = frame_of(cell);
        if crate::view_window_drag::unanchored(app, ui.ctx(), id) {
            if is_ground {
                ground = frame;
            }
            continue;
        }
        let Some(kind) = app.docs.views.get(id).map(|view| view.kind) else { continue };
        // A press on a header button is the button's: focusing the card first would take its buttons away before the click ends.
        let header_slots = 4;
        let on_button = press.is_some_and(|p| {
            !is_ground
                && card_header_visible(ui, cell, id, headers)
                && (card_title_rect(cell).contains(p) || (0..header_slots).any(|slot| button_rect(cell, slot as f32).contains(p)))
        });
        let mut tile = ui.new_child(UiBuilder::new().id_salt(("view_tile", app.session.uid, id)).max_rect(frame));
        tile.set_clip_rect(ui.clip_rect().intersect(if is_ground && !render_ground { area } else { frame }));
        app.viewport.tile = Some(slot(id));
        app.viewport.card = !is_ground;
        if is_ground {
            app.viewport.bleed = (!render_ground).then_some(area);
            app.viewport.window = Some(free);
            with_card_kind(app, id, |app| draw_canvas(app, &mut tile, kind));
            crate::documents::sync_linked_active(app);
            if let Some(range) = app.viewport.backdrop.take() {
                lay_under(ui.ctx(), layer, &slots, range);
            }
            ground = frame;
        } else {
            let paint_start = ui.ctx().graphics(|graphics| graphics.get(layer).map_or(egui::layers::ShapeIdx(0), |list| list.next_idx()));
            with_card_camera(app, id, |app| {
                app.step_view_animation(ui.ctx());
                draw_canvas(app, &mut tile, kind);
                let controls = app.viewport.layout.as_ref().filter(|layout| layout.viewport == frame);
                if !on_button
                    && press.is_some_and(|p| cell.contains(p) && !controls.is_some_and(|l| l.axis_view.contains(p) || l.navigation.contains(p)))
                {
                    app.viewport.pending_tile_focus = Some(id);
                }
            });
            let radius = if card_header_visible(ui, cell, id, headers) {
                CornerRadius { nw: 0, ne: 0, sw: CARD_RADIUS, se: CARD_RADIUS }
            } else {
                CornerRadius::same(CARD_RADIUS)
            };
            solidcraft_viewport_egui::clip_card(tile.painter(), paint_start, frame, radius);
            app.viewport.backdrop = None;
            cards.push((id, frame));
        }
        app.viewport.tile = None;
        app.viewport.card = false;
        app.viewport.window = None;
    }
    // Drawn over the views: the shadow and frame of each card, and the focused view's name.
    let clean = crate::view_windows::is_clean(app);
    let mut chosen = None;
    for (id, body) in cards.iter().filter(|_| !clean) {
        let card = Rect::from_min_max(body.min - vec2(0.0, HEADER), body.max);
        if let Some(action) = card_chrome(app, ui, *id, card, if several { headers } else { solidcraft_views::CardHeaders::Hidden }) {
            chosen = Some((*id, action));
        }
    }
    let mut picked = None;
    for (ordinal, cell) in empties.iter().enumerate().filter(|_| !clean) {
        if let Some(action) = empty_picker(app, ui, ordinal, *cell) {
            picked = Some((ordinal, action));
        }
    }
    if !clean {
        if several && !crate::view_window_drag::unanchored(app, ui.ctx(), focus) {
            ground_chip(app, ui, focus, ground);
        }
        if visible.is_none() {
            for splitter in tiles.splitters(to_tile(free), GUTTER) {
                splitter_handle(app, ui, &splitter);
            }
        }
    }
    // The shown view's rectangle is the one the panels and the host's controls read.
    app.viewport.rect = Some(ground);
    app.viewport.cards = cards;
    match picked {
        Some((ordinal, EmptyAction::Angle(angle))) => {
            crate::documents::view_fill_angle(app, ordinal, angle);
        }
        Some((ordinal, EmptyAction::Open(id))) => {
            crate::documents::view_fill_open(app, ordinal, id);
        }
        Some((ordinal, EmptyAction::Close)) => {
            crate::documents::view_close_empty(app, ordinal);
        }
        None => {}
    }
    match chosen {
        Some((id, CardAction::Maximise)) => {
            // This view alone in the window; the others stay open, reached from the bar.
            crate::documents::focus_view(app, id);
            crate::documents::view_layout(app, solidcraft_views::Preset::Single);
        }
        Some((id, CardAction::Remove)) => {
            crate::documents::view_unshow(app, id);
        }
        Some((id, CardAction::Close)) => {
            crate::documents::view_close(app, id);
        }
        None => {}
    }
}

/// Apply a canvas press after all controls for this frame have run with the old focus.
pub(crate) fn focus_pressed(app: &mut SolveApp) -> bool {
    let Some(id) = app.viewport.pending_tile_focus.take() else { return false };
    let Some((_, rect)) = app.viewport.cards.iter().find(|(view, _)| *view == id).copied() else { return false };
    if !crate::documents::focus_view(app, id) {
        return false;
    }
    app.viewport.rect = Some(rect);
    true
}

pub(crate) fn cancel_placement_on_escape(app: &mut SolveApp, ctx: &egui::Context) -> bool {
    ctx.input(|i| i.key_pressed(egui::Key::Escape)) && app.docs.view_placement.take().is_some()
}

/// A candidate shared by pointer, keyboard and the explicit Place in layout controls.
#[derive(Clone)]
pub(crate) struct DockTarget {
    pub placement: Placement,
    pub zone: Rect,
    button_zone: Option<Rect>,
    pub rect: Rect,
    pub neighbors: Vec<Rect>,
    pub displaced: Option<(u64, Rect)>,
    pub focused: bool,
    pub label: String,
    pub blocked: bool,
}

impl DockTarget {
    pub(crate) fn parked_zone(&self) -> Option<Rect> {
        self.button_zone
    }
}

const EDGE_FRACTION: f32 = 0.28;
const ATTRACTION: f32 = 24.0;
const HYSTERESIS: f32 = 6.0;
const MIN_CELL: egui::Vec2 = vec2(200.0, 140.0);

/// Targets are ordered by view, then left, right, top, bottom and middle, then empty cells.
pub(crate) fn dock_targets(app: &SolveApp, view: u64) -> Vec<DockTarget> {
    let Some(view) = app.docs.views.get(view) else { return Vec::new() };
    let Some(area) = app.shell.content.filter(|r| r.is_finite() && r.is_positive()) else { return Vec::new() };
    let (cells, ground) = layout_bounds(app, area);
    zones_for_kind(&app.docs.views, view.id, view.kind, cells, ground, &crate::shell::covered_rects(app))
}

pub(crate) fn incoming_dock_targets(app: &SolveApp, view: &solidcraft_views::DesignView, covers: &[Rect]) -> Vec<DockTarget> {
    let Some(area) = app.shell.content.filter(|r| r.is_finite() && r.is_positive()) else { return Vec::new() };
    let (cells, ground) = layout_bounds(app, area);
    let visible = without_lifted(&app.docs.views, view.id);
    zones_for_kind(visible.as_ref().unwrap_or(&app.docs.views), view.id, view.kind, cells, ground, covers)
}

/// Keep an entered target until the pointer is six points beyond it. A value under one
/// is attraction from a nearby zone; it never commits a drop on its own.
pub(crate) fn dock_at(targets: &[DockTarget], pointer: Pos2, previous: Option<Placement>) -> Option<(&DockTarget, f32)> {
    if !pointer.is_finite() {
        return None;
    }
    if let Some(target) = targets
        .iter()
        .find(|target| Some(target.placement) == previous && target.zone.expand(HYSTERESIS).contains(pointer))
        .or_else(|| targets.iter().find(|target| target.zone.contains(pointer)))
    {
        return Some((target, 1.0));
    }
    targets
        .iter()
        .filter(|target| !target.blocked)
        .map(|target| (target, target.zone.distance_to_pos(pointer)))
        .filter(|(_, distance)| *distance < ATTRACTION)
        .min_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(target, distance)| (target, 1.0 - distance / ATTRACTION))
}

#[cfg(test)]
fn zones(views: &ViewSet, view: u64, area: Rect, ground: Rect, covers: &[Rect]) -> Vec<DockTarget> {
    views.get(view).map_or_else(Vec::new, |view| zones_for_kind(views, view.id, view.kind, area, ground, covers))
}

fn zones_for_kind(views: &ViewSet, view: u64, kind: solidcraft_views::ViewKind, area: Rect, ground: Rect, covers: &[Rect]) -> Vec<DockTarget> {
    let active = views.active().id;
    let single = Tiles::Leaf(active);
    let tiled = views.tiles().filter(|_| views.is_tiled());
    let tiles = tiled.unwrap_or(&single);
    let shown = tiles.contains(view);
    let mut zones = Vec::new();
    let mut add = |placement: Placement, zone: Rect| {
        let button_zone = target_area(zone, covers);
        let Some(next) = views.preview_incoming_placement(view, kind, placement) else { return };
        let cells = next.rects(to_tile(area), GUTTER);
        let cell = |view| cells.iter().find(|(id, _)| *id == view).map(|(_, rect)| from_tile(*rect));
        let Some(rect) = cell(view) else { return };
        let rect = if tiled.is_none() && matches!(placement, Placement::Replace(_)) { ground } else { rect };
        let fits = |rect: Rect| rect.width() >= MIN_CELL.x && rect.height() >= MIN_CELL.y;
        let blocked = matches!(placement, Placement::Split { target, .. } if !fits(rect) || !cell(target).is_some_and(fits));
        let label = dock_label(views, placement, shown, blocked);
        let displaced = match placement {
            Placement::Replace(target) if shown => cell(target).map(|rect| (target, rect)),
            _ => None,
        };
        let neighbors = cells.iter().filter(|(id, _)| *id != view).map(|(_, rect)| from_tile(*rect)).collect();
        let focused = active == view || !next.contains(active);
        zones.push(DockTarget { placement, zone, button_zone, rect, neighbors, displaced, focused, label, blocked });
    };
    let mut empty_cells = Vec::new();
    let zone_extent = if tiled.is_none() { ground } else { area };
    for (id, rect) in tiles.rects(to_tile(zone_extent), GUTTER) {
        let cell = from_tile(rect);
        if id == solidcraft_views::EMPTY {
            empty_cells.push(cell);
            continue;
        }
        let middle = cell.shrink2(cell.size() * EDGE_FRACTION);
        for (edge, rect) in [
            (Edge::Left, Rect::from_min_max(cell.min, pos2(middle.left(), cell.bottom()))),
            (Edge::Right, Rect::from_min_max(pos2(middle.right(), cell.top()), cell.max)),
            (Edge::Top, Rect::from_min_max(pos2(middle.left(), cell.top()), middle.right_top())),
            (Edge::Bottom, Rect::from_min_max(middle.left_bottom(), pos2(middle.right(), cell.bottom()))),
        ] {
            add(Placement::Split { target: id, edge }, rect);
        }
        add(Placement::Replace(id), middle);
    }
    for (ordinal, cell) in empty_cells.into_iter().enumerate() {
        add(Placement::Empty(ordinal), cell);
    }
    zones
}

fn dock_label(views: &ViewSet, placement: Placement, shown: bool, blocked: bool) -> String {
    use solidcraft_ui_text::{format, tr};
    if blocked {
        return tr("multi_window.drag_small").to_owned();
    }
    let (target, key) = match placement {
        Placement::Empty(_) => return tr("multi_window.drag_fill").to_owned(),
        Placement::Replace(target) => (target, if shown { "multi_window.drag_swap" } else { "multi_window.drag_replace" }),
        Placement::Split { target, edge } => (
            target,
            match edge {
                Edge::Left => "multi_window.drag_left",
                Edge::Right => "multi_window.drag_right",
                Edge::Top => "multi_window.drag_top",
                Edge::Bottom => "multi_window.drag_bottom",
            },
        ),
    };
    format(key, &[("view", views.get(target).map_or("", |view| view.name.as_str()))])
}

pub(crate) fn commit_dock(app: &mut SolveApp, view: u64, placement: Placement) -> Result<(), String> {
    use solidcraft_engine::schema::tab_views::TabViewPlacement;
    let window = app.view_windows.current_window();
    app.run("tab.view.beside", serde_json::json!({ "id": view, "window": window, "placement": TabViewPlacement::from(placement) })).map(|_| ())
}

/// Commit the same collapsed source layout shown during a drag. A refused drop
/// restores the original layout through the normal view-state transition.
pub(crate) fn commit_lifted_dock(app: &mut SolveApp, view: u64, placement: Placement) -> Result<(), String> {
    crate::documents::keep_look(app);
    let Some(visible) = without_lifted(&app.docs.views, view) else { return commit_dock(app, view, placement) };
    let original = app.docs.views.clone();
    crate::documents::change_views(app, |views| *views = visible);
    let result = commit_dock(app, view, placement);
    if result.is_err() {
        crate::documents::change_views(app, |views| *views = original);
    }
    result
}

/// Paint the view menu's click-to-place controls. Dragging uses the same targets.
pub(crate) fn placement_overlay(app: &mut SolveApp, ctx: &egui::Context) {
    crate::view_window_drag::record_canvas(app, ctx);
    let Some((tab, view)) = app.docs.view_placement else { return };
    let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    if escape || tab != app.docs.active_tab() {
        app.docs.view_placement = None;
        return;
    }
    let mut chosen = None;
    let mut buttons = 0;
    let container = crate::ui_kit::Container::new(ctx, crate::ui_kit::Surface::Navigation);
    for (i, zone) in dock_targets(app, view).into_iter().filter(|target| !target.blocked).enumerate() {
        let Some(button) = zone.button_zone.map(|rect| rect.shrink(2.0)) else { continue };
        if button.width() < 58.0 || button.height() < 20.0 {
            continue;
        }
        buttons += 1;
        let size = button.size().min(vec2(132.0, 30.0));
        let anchor = egui::Id::unique(("view_placement", ctx.viewport_id(), tab, view, i));
        let label = placement_label(zone.placement);
        container
            .area(anchor)
            .order(egui::Order::Foreground)
            .fixed_pos(button.center() - size / 2.0)
            .default_size(size)
            .movable(false)
            .fade_in(false)
            .show(ctx, |ui| {
                let rect = Rect::from_min_size(ui.next_widget_position(), size);
                let response =
                    crate::ui_kit::button_at(ui, "view_placement_zone", rect, solidcraft_ui_text::tr(label), crate::ui_kit::Button::Normal);
                if response.hovered() || response.has_focus() {
                    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::unique("view_tile_preview")));
                    paint_preview(&painter, zone.rect);
                }
                if response.clicked() {
                    chosen = Some(zone.placement);
                }
            });
    }
    if let Some(placement) = chosen {
        app.docs.view_placement = None;
        let _ = commit_dock(app, view, placement);
    } else if buttons == 0 || ctx.input(|i| i.pointer.secondary_clicked()) {
        app.docs.view_placement = None;
    }
}

fn placement_label(placement: Placement) -> &'static str {
    match placement {
        Placement::Empty(_) => "design_view.place_empty",
        Placement::Replace(_) => "design_view.place_center",
        Placement::Split { edge: Edge::Left, .. } => "design_view.place_left",
        Placement::Split { edge: Edge::Right, .. } => "design_view.place_right",
        Placement::Split { edge: Edge::Top, .. } => "design_view.place_top",
        Placement::Split { edge: Edge::Bottom, .. } => "design_view.place_bottom",
    }
}

fn paint_preview(painter: &egui::Painter, rect: Rect) {
    let tokens = Tokens::get();
    painter.rect_filled(rect, CARD_RADIUS, tokens.accent_soft.gamma_multiply(0.58));
    painter.rect_stroke(rect, CARD_RADIUS, Stroke::new(2.0, tokens.accent), StrokeKind::Inside);
}

fn safe_region(rect: Rect, covers: &[Rect]) -> Option<crate::panel_runtime::HostUsableRegion> {
    use crate::panel_runtime::{logical_rect, resolve_usable_region};
    if !rect.is_positive() {
        return None;
    }
    let covers: Vec<_> = covers.iter().copied().map(|cover| cover.intersect(rect)).filter(Rect::is_positive).map(logical_rect).collect();
    resolve_usable_region(logical_rect(rect), &covers).ok()
}

fn target_area(cell: Rect, covers: &[Rect]) -> Option<Rect> {
    if covers.iter().all(|cover| !cover.intersect(cell).is_positive()) {
        cell.is_positive().then_some(cell)
    } else {
        safe_region(cell, covers)?.largest_rect().map(crate::panel_runtime::egui_rect)
    }
}

fn unobstructed(rect: Rect, covers: &[Rect], aspect: f32) -> Option<Rect> {
    use solidcraft_engine::panels::Point;
    let center = rect.center();
    safe_region(rect, covers)?.fit_rect_with_clearance(aspect, Point { x: center.x, y: center.y }, MARGIN).map(crate::panel_runtime::egui_rect)
}

/// Where the header button in `slot` (0 the rightmost) of the card `card` lies.
fn button_rect(card: Rect, slot: f32) -> Rect {
    Rect::from_center_size(pos2(card.right() - 16.0 - slot * 24.0, card.top() + HEADER / 2.0), vec2(22.0, 22.0))
}

/// What an empty cell's picker asks for.
enum EmptyAction {
    /// A new view looking from a standard angle.
    Angle(solidcraft_viewport::StandardView),
    /// A view that is open already.
    Open(u64),
    /// Close the cell.
    Close,
}

/// An empty cell: a dashed frame that asks what to show there. The standard angles, then the
/// open views that can join the layout, and a button that closes the cell.
fn empty_picker(app: &SolveApp, ui: &mut egui::Ui, ordinal: usize, cell: Rect) -> Option<EmptyAction> {
    use solidcraft_viewport::StandardView;
    let t = Tokens::get();
    let painter = ui.painter().clone();
    painter.rect_filled(cell, CornerRadius::same(CARD_RADIUS), t.panel_header.gamma_multiply(0.7));
    let edge = [cell.left_top(), cell.right_top(), cell.right_bottom(), cell.left_bottom(), cell.left_top()];
    painter.extend(egui::Shape::dashed_line(&edge, Stroke::new(1.5, t.border), 6.0, 4.0));
    let mut action = None;
    let open: Vec<(u64, &str)> =
        app.docs.views.views().iter().filter(|view| app.docs.views.can_show_beside(view.id)).map(|view| (view.id, view.name.as_str())).collect();
    let height = 22.0 + 2.0 * 34.0 + if open.is_empty() { 0.0 } else { 6.0 + open.len().min(4) as f32 * 34.0 };
    let width = (cell.width() - 48.0).clamp(120.0, 300.0);
    let title = solidcraft_ui_text::tr_label("Show a view here");
    let covers = crate::shell::covered_rects(app);
    let ideal = Rect::from_center_size(cell.center(), vec2(width, height));
    let close = Rect::from_center_size(cell.right_top() + vec2(-18.0, 18.0), vec2(22.0, 22.0));
    let angles = [(StandardView::Front, "Front"), (StandardView::Right, "Right"), (StandardView::Top, "Top"), (StandardView::Iso, "Iso")];
    let room = if cell.contains_rect(ideal) && covers.iter().all(|cover| !cover.intersects(ideal)) {
        Some(ideal)
    } else {
        unobstructed(cell, &covers, width / height).filter(|room| room.width() >= width && room.height() >= height)
    };
    let Some(room) = room.filter(|_| open.len() <= 4 && covers.iter().all(|cover| !cover.intersects(close))) else {
        let button = unobstructed(cell, &covers, 4.0)?;
        let button = Rect::from_center_size(button.center(), button.size().min(vec2(160.0, 30.0)));
        let mut child = ui.new_child(UiBuilder::new().id_salt(("empty_cell_menu", ordinal)).max_rect(button));
        let mut chosen = None;
        crate::ui_kit::Menu::button_with(
            &mut child,
            |ui, _| {
                let rect = Rect::from_min_size(ui.next_widget_position(), button.size());
                crate::ui_kit::button_at(ui, "empty_cell_open", rect, title, crate::ui_kit::Button::Normal)
            },
            |ui| {
                for (angle, label) in angles {
                    if crate::ui_kit::menu::button(ui, solidcraft_ui_text::tr_label(label), "").clicked() {
                        chosen = Some(EmptyAction::Angle(angle));
                        ui.close();
                    }
                }
                if !open.is_empty() {
                    ui.separator();
                    egui::ScrollArea::vertical().max_height(210.0).show(ui, |ui| {
                        for (id, name) in &open {
                            if crate::ui_kit::menu::button(ui, name, "").clicked() {
                                chosen = Some(EmptyAction::Open(*id));
                                ui.close();
                            }
                        }
                    });
                }
                ui.separator();
                if crate::ui_kit::menu::button(ui, solidcraft_ui_text::tr_label("Close this cell"), "").clicked() {
                    chosen = Some(EmptyAction::Close);
                    ui.close();
                }
            },
        );
        return chosen;
    };
    let left = room.center().x - width / 2.0;
    let mut y = room.center().y - height / 2.0;
    painter.text(pos2(room.center().x, y + 8.0), egui::Align2::CENTER_CENTER, title, egui::FontId::proportional(t.font_body), t.text);
    y += 22.0;
    let button = |ui: &mut egui::Ui, rect: Rect, key: (usize, u64), label: &str| -> bool {
        let resp = ui.interact(rect, ui.scope_id().with(("empty_cell", ordinal, key)), Sense::click());
        let fill = if resp.hovered() { t.accent_soft } else { t.panel_header };
        ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
        ui.painter().rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, t.border), StrokeKind::Inside);
        ui.painter().text(rect.left_center() + vec2(10.0, 0.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(t.font_body), t.text);
        resp.clicked()
    };
    let half = (width - 6.0) / 2.0;
    for (i, (angle, label)) in angles.into_iter().enumerate() {
        let (col, row) = (i % 2, i / 2);
        let rect = Rect::from_min_size(pos2(left + col as f32 * (half + 6.0), y + row as f32 * 34.0), vec2(half, 28.0));
        let label = solidcraft_ui_text::tr_label(label);
        if button(ui, rect, (0, i as u64), label) {
            action = Some(EmptyAction::Angle(angle));
        }
    }
    y += 2.0 * 34.0 + 6.0;
    for (id, name) in &open {
        let rect = Rect::from_min_size(pos2(left, y), vec2(width, 28.0));
        if button(ui, rect, (1, *id), name) {
            action = Some(EmptyAction::Open(*id));
        }
        y += 34.0;
    }
    let resp = ui
        .interact(close, ui.scope_id().with(("empty_cell_close", ordinal)), Sense::click())
        .backing_on_hover_text(solidcraft_ui_text::tr_label("Close this cell"));
    let ink = Stroke::new(1.6, if resp.hovered() { t.text } else { t.text_dim });
    let c = close.center();
    painter.line_segment([c + vec2(-4.5, -4.5), c + vec2(4.5, 4.5)], ink);
    painter.line_segment([c + vec2(-4.5, 4.5), c + vec2(4.5, -4.5)], ink);
    if resp.clicked() {
        action = Some(EmptyAction::Close);
    }
    action
}

/// What a card's header buttons ask for.
enum CardAction {
    /// Keep only this view in the window: the others stay open, reached from the bar.
    Maximise,
    /// Stop showing this view in the window; it stays open, reached from the bar.
    Remove,
    Close,
}

/// The controls of the cards, each acting on its own view. The focused view's are
/// the host's, drawn after this.
pub(crate) fn card_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    with_visible_ground(app, &ctx, |app| visible_card_controls(app, ui));
}

fn visible_card_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let ground = app.viewport.rect;
    let cards = std::mem::take(&mut app.viewport.cards);
    for (id, rect) in &cards {
        let mut controls = controls_ui(ui, app.session.uid, *id, *rect);
        app.viewport.rect = Some(*rect);
        app.viewport.card = true;
        with_card_camera(app, *id, |app| {
            if app.docs.views.get(*id).is_some_and(|view| view.kind == ViewKind::Sheet) {
                return;
            }
            if crate::render_workspace::active(app) {
                crate::render_workspace::finish_result_navigation(app, &mut controls, *rect);
            }
            crate::viewport::show_card_controls(app, &mut controls, *rect);
            if app.pending_fit.is_some() {
                app.resolve_fit(ui.ctx());
            }
        });
        app.viewport.card = false;
    }
    app.viewport.rect = ground;
    app.viewport.cards = cards;
}

/// Use the same widget identities with or without focus, so a gesture or menu follows its owner.
pub(crate) fn controls_ui(ui: &mut egui::Ui, document: u64, id: u64, rect: Rect) -> egui::Ui {
    let mut child = ui.new_child(UiBuilder::new().id_salt(("view_controls", document, id)).max_rect(rect));
    child.set_clip_rect(ui.clip_rect().intersect(rect));
    child
}

/// Paint the opaque header shared by workspace cards and their drag preview. The caller
/// owns buttons and icon placement and supplies the title's x coordinate.
pub(crate) fn paint_card_header(painter: &egui::Painter, rect: Rect, radius: u8, name: &str, name_x: f32) {
    let t = Tokens::get();
    painter.rect_filled(rect, CornerRadius { nw: radius, ne: radius, sw: 0, se: 0 }, t.panel_header);
    painter.with_clip_rect(rect).text(
        pos2(name_x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        name,
        crate::ui_kit::type_face::semibold(t.font_body),
        t.text,
    );
}

/// The card shadow, also used when a drag settles into a layout cell.
pub(crate) fn card_shadow(card: Rect, radius: CornerRadius) -> egui::epaint::RectShape {
    egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: egui::Color32::from_black_alpha(70) }.as_shape(card, radius)
}

fn card_header_visible(ui: &egui::Ui, card: Rect, id: u64, headers: solidcraft_views::CardHeaders) -> bool {
    use solidcraft_views::CardHeaders;
    match headers {
        CardHeaders::Always => true,
        CardHeaders::Hidden => false,
        CardHeaders::Hover => {
            let scope = ui.scope_id();
            ui.rect_contains_pointer(card)
                || ui.memory(|memory| {
                    memory.has_focus(scope.with(("card_view_menu", id))) || (0..3).any(|slot| memory.has_focus(scope.with(("card_button", id, slot))))
                })
        }
    }
}

fn sketch_header_name(app: &SolveApp, id: u64) -> Option<String> {
    let view = app.docs.views.get(id)?;
    view.editing.as_ref()?;
    let name = crate::sketch_view::name(app, id)?;
    Some(solidcraft_ui_text::format("sketch_mode.view_name", &[("view", &view.name), ("sketch", &name)]))
}

/// A card lifted off the focused view: its soft shadow, frame, and optional header.
fn card_chrome(app: &mut SolveApp, ui: &mut egui::Ui, id: u64, cell: Rect, headers: solidcraft_views::CardHeaders) -> Option<CardAction> {
    let visible = card_header_visible(ui, cell, id, headers);
    // View framing is independent of whether the title and controls are drawn.
    let card = if visible { cell } else { frame_of(cell) };
    let t = Tokens::get();
    let radius = CornerRadius::same(CARD_RADIUS);
    let shadow = card_shadow(card, radius);
    let out = card.expand(32.0).intersect(ui.clip_rect());
    // The shadow only outside the card, so the view inside is not dimmed.
    for strip in [
        Rect::from_min_max(out.min, pos2(card.left(), out.max.y)),
        Rect::from_min_max(pos2(card.right(), out.min.y), out.max),
        Rect::from_min_max(pos2(card.left(), out.min.y), pos2(card.right(), card.top())),
        Rect::from_min_max(pos2(card.left(), card.bottom()), pos2(card.right(), out.max.y)),
    ] {
        ui.painter().with_clip_rect(strip).add(shadow.clone());
    }
    let painter = ui.painter().with_clip_rect(card);
    if visible {
        let top = Rect::from_min_size(cell.min, vec2(cell.width(), HEADER));
        let name = app.docs.views.get(id).map_or("", |view| view.name.as_str());
        let label = clear_of_panels(app, Rect::from_min_size(top.left_top() + vec2(10.0, 0.0), vec2(120.0, HEADER)));
        if let Some((paper, state)) = crate::drawing_sheet_page::view_badge(app, id, ui.ctx()) {
            paint_card_header(&painter, top, CARD_RADIUS, "", label.left());
            let right = button_rect(cell, 3.0).left();
            let metadata = Rect::from_min_max(pos2(label.left(), top.top()), pos2(right.max(label.left()), top.bottom()));
            let used = crate::app_bar::paint_view_chips(&painter, metadata, &[&paper, &state]);
            let title = Rect::from_min_max(top.min, pos2((right - used).max(label.left()), top.bottom()));
            painter.with_clip_rect(title).text(
                pos2(label.left(), top.center().y),
                egui::Align2::LEFT_CENTER,
                name,
                crate::ui_kit::type_face::semibold(t.font_body),
                t.text,
            );
        } else {
            paint_card_header(&painter, top, CARD_RADIUS, name, label.left());
        }
        if view_title_drag(app, ui, id, card_title_rect(cell)) {
            app.viewport.pending_tile_focus = Some(id);
        }
    }
    painter.rect_stroke(card.shrink(0.5), radius, Stroke::new(1.0, t.border), StrokeKind::Inside);
    if headers == solidcraft_views::CardHeaders::Hidden {
        return None;
    }
    // Hover mode leaves the controls reachable with Tab. Focus reveals the header.
    let mut painter = ui.painter().with_clip_rect(cell);
    if !visible {
        painter.set_invisible();
    }
    let mut action = None;
    let mut button = |slot: f32, kind: CardAction| {
        let rect = button_rect(cell, slot);
        let resp = ui.interact(rect, ui.scope_id().with(("card_button", id, slot as i32)), Sense::click());
        let tip = match kind {
            CardAction::Maximise => solidcraft_ui_text::tr_label("Maximise this view"),
            CardAction::Remove => solidcraft_ui_text::tr_label("Remove from this window"),
            CardAction::Close => solidcraft_ui_text::tr_label("Close view"),
        };
        let resp = resp.backing_on_hover_text(tip);
        if resp.hovered() {
            painter.rect_filled(rect, CornerRadius::same(5), t.accent_soft);
        }
        let ink = Stroke::new(1.6, if resp.hovered() { t.text } else { t.text_dim });
        let c = rect.center();
        match kind {
            CardAction::Maximise => {
                // Four corners, as in the design: a view to fill the window.
                for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                    let corner = c + vec2(sx * 5.0, sy * 5.0);
                    painter.line_segment([corner + vec2(-sx * 3.5, 0.0), corner], ink);
                    painter.line_segment([corner, corner + vec2(0.0, -sy * 3.5)], ink);
                }
            }
            CardAction::Remove => {
                painter.line_segment([c + vec2(-4.5, 0.0), c + vec2(4.5, 0.0)], ink);
            }
            CardAction::Close => {
                painter.line_segment([c + vec2(-4.5, -4.5), c + vec2(4.5, 4.5)], ink);
                painter.line_segment([c + vec2(-4.5, 4.5), c + vec2(4.5, -4.5)], ink);
            }
        }
        if resp.clicked() {
            action = Some(kind);
        }
    };
    button(0.0, CardAction::Close);
    button(1.0, CardAction::Maximise);
    button(2.0, CardAction::Remove);
    let rect = button_rect(cell, 3.0);
    let response = ui.interact(rect, ui.scope_id().with(("card_view_menu", id)), Sense::click());
    let tip = app.tr("view_menu.open");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    let open = response.clicked() || response.has_focus() && ui.input(|i| i.modifiers.shift && i.key_pressed(egui::Key::F10));
    let response = response.backing_on_hover_text(tip);
    if response.hovered() || response.has_focus() {
        painter.rect_filled(rect, CornerRadius::same(5), t.accent_soft);
    }
    painter.text(rect.center(), egui::Align2::CENTER_CENTER, "⋯", egui::FontId::proportional(t.font_body), t.text_dim);
    if open && let Some(index) = crate::view_windows::strip_views(app).iter().position(|(_, view)| view.id == id) {
        crate::app_bar::open_card_view_menu(ui.ctx(), crate::documents::tab_strip_id(ui.ctx().viewport_id()), index, &response);
    }
    action
}

/// The focused view's name in a pill in the band above its frame, clear of the ribbon and the panels.
fn ground_chip(app: &mut SolveApp, ui: &egui::Ui, id: u64, ground: Rect) {
    let t = Tokens::get();
    let name = app.docs.views.get(id).map_or("", |view| view.name.as_str());
    let painter = ui.painter();
    let galley = painter.layout_no_wrap(name.to_owned(), egui::FontId::proportional(t.font_body), t.text);
    let badges = crate::drawing_sheet_page::view_badge(app, id, ui.ctx());
    let badge_width = badges.as_ref().map_or(0.0, |(paper, state)| {
        [paper, state]
            .iter()
            .map(|label| {
                painter.layout_no_wrap((*label).clone(), crate::ui_kit::type_face::semibold(t.font_small), t.text).size().x + t.space_12 + t.space_6
            })
            .sum::<f32>()
    });
    let size = vec2((galley.size().x + 34.0 + badge_width).min((ground.width() - 2.0 * MARGIN).max(0.0)), 26.0);
    let at = pos2(ground.left() + MARGIN, ground.top() - HEADER + (HEADER - size.y) / 2.0);
    let chip = clear_of_panels(app, Rect::from_min_size(at, size));
    let painter = &painter.with_clip_rect(chip);
    let radius = CornerRadius::same(13);
    painter.add(egui::Shadow { offset: [0, 2], blur: 8, spread: 0, color: egui::Color32::from_black_alpha(60) }.as_shape(chip, radius));
    painter.rect_filled(chip, radius, t.panel_header);
    painter.rect_stroke(chip, radius, Stroke::new(1.0, t.border), StrokeKind::Inside);
    painter.circle_filled(chip.left_center() + vec2(13.0, 0.0), 3.5, t.accent);
    painter.galley(chip.left_center() + vec2(24.0, -galley.size().y / 2.0), galley, t.text);
    if let Some((paper, state)) = badges {
        crate::app_bar::paint_view_chips(
            painter,
            Rect::from_min_max(pos2(chip.right() - badge_width, chip.top()), chip.right_bottom()),
            &[&paper, &state],
        );
    }
    view_title_drag(app, ui, id, chip);
}

fn card_title_rect(cell: Rect) -> Rect {
    Rect::from_min_max(cell.min, pos2(button_rect(cell, 3.0).left().max(cell.left()), cell.top() + HEADER))
}

fn view_title_drag(app: &mut SolveApp, ui: &egui::Ui, id: u64, title: Rect) -> bool {
    crate::scenario::publish_rect(&format!("view:title:{id}"), title);
    let response = ui.interact(title, ui.scope_id().with(("view_title_drag", id)), Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::Grab);
    if let Some((_, state)) = crate::drawing_sheet_page::view_badge(app, id, ui.ctx())
        && state == app.tr("sheet_views.fit")
    {
        let name = app.docs.views.get(id).map_or("", |view| view.name.as_str());
        let label = solidcraft_ui_text::format("sheet_views.fitted", &[("name", name)]);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, ui.is_enabled(), label.as_str()));
    }
    if response.drag_started_by(egui::PointerButton::Primary) {
        let origin = Rect::from_min_size(title.min, vec2(title.width().min(140.0), title.height()));
        crate::view_window_drag::start_header(app, ui.ctx(), id, origin);
    }
    if let Some(name) = sketch_header_name(app, id) {
        let name = crate::sketch_view::owner_label(app, id)
            .map_or(name.clone(), |owner| solidcraft_ui_text::format("sketch_mode.view_name", &[("view", &name), ("sketch", &owner)]));
        let name = solidcraft_ui_text::format("sketch_mode.announcement", &[("view", &name)]);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name.as_str()));
    }
    response.clicked()
}

/// The gutter between two sides of a split: drag it to move the split.
fn splitter_handle(app: &mut SolveApp, ui: &mut egui::Ui, splitter: &solidcraft_views::Splitter) {
    let t = Tokens::get();
    let rect = from_tile(splitter.rect);
    let hit = rect.expand(2.0);
    let response = ui.interact(hit, ui.scope_id().with(("view_splitter", splitter.id)), Sense::drag());
    let icon = match splitter.axis {
        Axis::Row => egui::CursorIcon::ResizeHorizontal,
        Axis::Column => egui::CursorIcon::ResizeVertical,
    };
    let response = response.on_hover_cursor(icon);
    if response.dragged()
        && let Some(p) = response.interact_pointer_pos()
    {
        let span = splitter.span;
        let ratio = match splitter.axis {
            Axis::Row => (p.x - span.x) / span.w.max(1.0),
            Axis::Column => (p.y - span.y) / span.h.max(1.0),
        };
        app.docs.views.set_split(splitter.id, ratio);
    }
    if response.hovered() || response.dragged() {
        ui.painter().rect_filled(rect, CornerRadius::same(2), t.accent_soft);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn native_window_picture_slots_are_disjoint_from_saved_view_ids_and_other_windows() {
        let ids = [(1_u64 << 40) + 1, (1_u64 << 40) + 2, 1_u64 << 53];
        let mut main = crate::viewport::ViewportState::default();
        for id in ids {
            main.tile = Some(slot(id));
            assert_eq!(main.scene_slot(), Some(id));
        }
        let mut first = crate::viewport::ViewportState::default();
        let mut second = crate::viewport::ViewportState::default();
        first.isolate_window_scenes();
        second.isolate_window_scenes();
        let mut expected = Vec::new();
        for tile in std::iter::once(None).chain(ids.into_iter().map(Some)) {
            first.tile = tile;
            second.tile = tile;
            let first_slot = first.scene_slot().unwrap();
            let second_slot = second.scene_slot().unwrap();
            assert!(first_slot >= 1_u64 << 63 && second_slot >= 1_u64 << 63);
            assert_ne!(first_slot, second_slot, "the same view id in different native hosts has separate pictures");
            assert_eq!(first.scene_slot(), Some(first_slot), "a host keeps its slot across frames");
            assert!(!ids.contains(&first_slot) && !ids.contains(&second_slot));
            expected.push(first_slot);
        }
        let mut retained: Vec<_> = first.window_scene_slots().collect();
        retained.sort_unstable();
        expected.sort_unstable();
        assert_eq!(retained, expected, "closing a host can retire precisely its own pictures");
    }

    #[test]
    fn saved_view_ids_above_32_bits_keep_distinct_gpu_picture_slots() {
        let ids = [(1_u64 << 31) + 1, (1_u64 << 40) + 1, (1_u64 << 40) + 2, 1_u64 << 53];
        let saved = json!({
            "active": ids[0],
            "views": ids.iter().enumerate().map(|(index, id)| json!({"id": id, "kind": "model", "name": format!("Model {index}")})).collect::<Vec<_>>()
        });
        let mut views = solidcraft_views::ViewSet::from_json(&saved).unwrap();
        assert_eq!(views.set_layout(solidcraft_views::Preset::Grid), 4);
        let slots: Vec<_> = views.tiles().unwrap().views().into_iter().map(slot).collect();
        assert_eq!(slots, ids, "picture callbacks keep the complete saved view identity");
        assert_eq!(slots.iter().copied().collect::<std::collections::HashSet<_>>().len(), 4);
        assert!(!slots.contains(&0), "the ordinary root viewport has its own slot");
    }

    #[test]
    fn model_and_sheet_sketches_draw_no_frame_or_editing_pill_over_the_view() {
        for sheet in [false, true] {
            let mut app = SolveApp::new(Default::default(), Default::default());
            if sheet {
                app.run("solid.box", json!({"length":40,"width":30,"height":5})).unwrap();
                let drawing = app.run("drawing.create", json!({})).unwrap();
                crate::workspace::enter(&mut app, "drawing").unwrap();
                app.run("drawing.sketch.create", json!({"owner":drawing["sheets"][0]["id"]})).unwrap();
            } else {
                app.run("sketch.create", json!({"plane":"XY"})).unwrap();
            }
            let owner = app.session.editing_view().expect("a hosted sketch has an owning tile");
            assert_eq!(app.docs.views.active().id, owner);
            assert!(sketch_header_name(&app, owner).is_some(), "the view hosts the sketch");
            let ground = Rect::from_min_size(pos2(80.0, 130.0), vec2(600.0, 360.0));
            let ctx = crate::fonts::test_context();
            let mut output =
                ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 700.0))), ..Default::default() }, |ui| {
                    ground_chip(&mut app, ui, owner, ground);
                });
            let t = Tokens::get();
            // Overlay removed by owner decision: no green frame round the view while it sketches.
            assert!(
                !output.shapes.iter().any(|clipped| matches!(&clipped.shape, egui::Shape::Rect(rect) if rect.stroke.color == t.sketch_accent)),
                "{sheet:?}: no green sketch frame is drawn round the view"
            );
            // Overlay removed by owner decision: no "Editing" pill or green sketch-name box over the view.
            assert!(
                !output.shapes.iter().any(|clipped| matches!(&clipped.shape, egui::Shape::Rect(rect) if rect.fill == t.sketch_soft)),
                "{sheet:?}: no green sketch box is drawn over the view"
            );
            assert!(
                !output.shapes.iter().any(|clipped| matches!(&clipped.shape, egui::Shape::Text(text) if text.galley.text() == "Editing" || text.galley.text().contains(" · "))),
                "{sheet:?}: no Editing state or sketch breadcrumb is drawn over the view"
            );
            output.textures_delta.clear();
        }
    }

    #[test]
    fn lifting_a_view_collapses_its_cell_without_changing_the_saved_layout() {
        let mut views = ViewSet::new();
        let first = views.active().id;
        let second = views.open(ViewKind::Model, None, false);
        views.set_layout_padded(solidcraft_views::Preset::SideBySide);
        let original = views.to_json();
        let visible = without_lifted(&views, second).unwrap();
        assert_eq!(visible.active().id, first);
        assert!(visible.tiles().is_none());
        assert!(visible.get(second).is_some(), "the tab stays open while its cell is lifted");
        assert_eq!(views.to_json(), original, "cancel has the complete original layout");
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 700.0));
        let targets = zones_for_kind(&visible, second, ViewKind::Model, area, area, &[]);
        let target = targets.iter().find(|target| target.placement == Placement::Split { target: first, edge: Edge::Right }).unwrap();
        let placed = visible.preview_placement(second, target.placement).unwrap();
        let actual = placed.rects(to_tile(area), GUTTER).into_iter().find(|(id, _)| *id == second).unwrap().1;
        assert_eq!(target.rect, from_tile(actual));
        assert!(target.displaced.is_none());
    }

    #[test]
    fn dropping_a_lifted_active_view_keeps_its_latest_camera() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let first = h.app.docs.views.active().id;
        crate::documents::view_layout(&mut h.app, solidcraft_views::Preset::SideBySide);
        let dragged = h.app.docs.views.views().iter().find(|view| view.id != first).unwrap().id;
        crate::documents::focus_view(&mut h.app, dragged);
        let old_camera = h.app.cam;
        h.app.cam.yaw += 0.4;
        h.app.cam.distance *= 1.3;
        let latest = h.app.cam;
        assert_eq!(h.app.docs.views.get(dragged).unwrap().spec.unwrap().camera, old_camera);
        commit_lifted_dock(&mut h.app, dragged, Placement::Split { target: first, edge: Edge::Right }).unwrap();
        assert_eq!(h.app.docs.views.get(dragged).unwrap().spec.unwrap().camera, latest);
        crate::documents::focus_view(&mut h.app, dragged);
        assert_eq!(h.app.cam, latest);
    }

    #[test]
    fn lifting_a_grid_view_keeps_its_siblings_and_collapses_the_split_branch() {
        let mut views = ViewSet::new();
        let first = views.active().id;
        let second = views.open(ViewKind::Model, None, false);
        let third = views.open(ViewKind::Model, None, false);
        let fourth = views.open(ViewKind::Model, None, false);
        views.set_layout_padded(solidcraft_views::Preset::Grid);
        let visible = without_lifted(&views, fourth).unwrap();
        let area = TileRect { x: 0.0, y: 0.0, w: 1000.0, h: 700.0 };
        let cells = visible.tiles().unwrap().rects(area, GUTTER);
        assert_eq!(cells.len(), 3);
        assert_eq!(visible.tiles().unwrap().views(), vec![first, second, third]);
        let sibling = cells.iter().find(|(id, _)| *id == third).unwrap().1;
        assert_eq!(sibling.w, area.w, "the neighboring view fills the vacated row");
        assert!(views.tiles().unwrap().contains(fourth));
    }

    #[test]
    fn escape_dismisses_view_placement() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let view = h.app.docs.views.active().id;
        let copy = crate::documents::view_duplicate(&mut h.app, view).unwrap();
        let tab = h.app.docs.active_tab();
        h.app.docs.view_placement = Some((tab, copy));
        h.call("ui.key", json!({"key":"Escape"}));
        assert!(h.app.docs.view_placement.is_none());
    }

    #[test]
    fn a_dock_preview_uses_the_committed_cell_and_rounds_odd_splits() {
        let mut views = ViewSet::new();
        let ground = views.active().id;
        let dragged = views.open(ViewKind::Model, None, false);
        assert!(views.activate(ground));
        let area = Rect::from_min_size(pos2(8.0, 40.0), vec2(625.0, 419.0));
        let full_ground = area.expand(MARGIN);
        let targets = zones(&views, dragged, area, full_ground, &[]);
        for (edge, expected) in [
            (Edge::Left, Rect::from_min_size(area.min, vec2(306.0, 419.0))),
            (Edge::Right, Rect::from_min_size(pos2(326.0, 40.0), vec2(307.0, 419.0))),
            (Edge::Top, Rect::from_min_size(area.min, vec2(625.0, 203.0))),
            (Edge::Bottom, Rect::from_min_size(pos2(8.0, 255.0), vec2(625.0, 204.0))),
        ] {
            let placement = Placement::Split { target: ground, edge };
            let target = targets.iter().find(|target| target.placement == placement).unwrap();
            assert_eq!(target.rect, expected);
            assert!(!target.blocked);
            let mut committed = views.clone();
            assert!(committed.place_tile(dragged, placement));
            let tiled = committed.tiles().unwrap().rects(to_tile(area), GUTTER);
            assert_eq!(tiled.into_iter().find(|(id, _)| *id == dragged).map(|(_, rect)| from_tile(rect)), Some(target.rect));
        }
        let middle = targets.iter().find(|target| target.placement == Placement::Replace(ground)).unwrap();
        assert_eq!(middle.rect, full_ground);
        assert!(middle.displaced.is_none());
        assert!(middle.focused);
    }

    #[test]
    fn dock_at_holds_a_zone_through_jitter_and_nearby_targets_do_not_commit() {
        let mut views = ViewSet::new();
        let ground = views.active().id;
        let dragged = views.open(ViewKind::Model, None, false);
        assert!(views.activate(ground));
        let area = Rect::from_min_size(pos2(100.0, 40.0), vec2(600.0, 400.0));
        let targets = zones(&views, dragged, area, area, &[]);
        let left = Placement::Split { target: ground, edge: Edge::Left };
        let border = area.left() + area.width() * 0.28;
        let near_border = |offset| pos2(border + offset, area.center().y);
        assert_eq!(dock_at(&targets, near_border(5.0), Some(left)).map(|(target, weight)| (target.placement, weight)), Some((left, 1.0)));
        assert_eq!(
            dock_at(&targets, near_border(7.0), Some(left)).map(|(target, weight)| (target.placement, weight)),
            Some((Placement::Replace(ground), 1.0))
        );
        let outside = |offset| pos2(area.left() - offset, area.center().y);
        assert_eq!(dock_at(&targets, outside(12.0), None).map(|(target, weight)| (target.placement, weight)), Some((left, 0.5)));
        assert!(dock_at(&targets, outside(24.0), None).is_none());
    }

    #[test]
    fn too_small_splits_are_refused_without_removing_the_center_target() {
        let mut views = ViewSet::new();
        let ground = views.active().id;
        let dragged = views.open(ViewKind::Model, None, false);
        assert!(views.activate(ground));
        let area = Rect::from_min_size(pos2(0.0, 40.0), vec2(411.0, 291.0));
        let targets = zones(&views, dragged, area, area, &[]);
        assert_eq!(targets.iter().filter(|target| target.blocked).count(), 4);
        let (center, strength) = dock_at(&targets, area.center(), None).unwrap();
        assert_eq!(center.placement, Placement::Replace(ground));
        assert_eq!(strength, 1.0);
        assert!(!center.blocked);
    }

    #[test]
    fn a_swap_moves_the_replaced_view_into_the_dragged_views_previous_cell() {
        let mut views = ViewSet::new();
        let ground = views.active().id;
        let dragged = views.open(ViewKind::Model, None, false);
        assert!(views.activate(ground));
        views.set_layout_padded(solidcraft_views::Preset::Grid);
        let area = Rect::from_min_size(pos2(8.0, 40.0), vec2(900.0, 700.0));
        let cells = views.tiles().unwrap().rects(to_tile(area), GUTTER);
        let cell = |target| cells.iter().find(|(id, _)| *id == target).map(|(_, rect)| from_tile(*rect)).unwrap();
        let targets = zones(&views, dragged, area, area, &[]);
        let middle = targets.iter().find(|target| target.placement == Placement::Replace(ground)).unwrap();
        assert_eq!(middle.rect, cell(ground));
        assert_eq!(middle.displaced, Some((ground, cell(dragged))));
        let from_ground = zones(&views, ground, area, area, &[]);
        let swap_card = from_ground.iter().find(|target| target.placement == Placement::Replace(dragged)).unwrap();
        assert_eq!(swap_card.rect, cell(dragged));
        assert_eq!(swap_card.displaced, Some((dragged, cell(ground))));
        let empty = targets.last().unwrap();
        assert_eq!(empty.placement, Placement::Empty(1));
        assert!(!empty.blocked);
        assert_eq!(empty.rect, from_tile(cells.last().unwrap().1));
    }

    #[test]
    fn tiled_navigation_controls_only_change_their_own_view() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.ui.show_browser = false;
        h.app.ui.show_timeline = false;
        h.app.ui.show_navigation_bar = true;
        h.app.ui.perspective = false;
        h.app.ui.auto_ortho_faces = false;
        h.app.ui.visual_style = 1;
        h.app.preferences.navigation_bar.slots = vec!["projection".into(), "visual_style".into()];
        let ground = h.app.docs.views.active().id;
        assert_eq!(crate::documents::view_layout(&mut h.app, solidcraft_views::Preset::SideBySide), 2);
        h.frames(4);
        let (card, rect) = h.app.viewport.cards[0];
        let ctx = h.ctx.clone();
        let bar = crate::documents::with_view_look(&mut h.app, card, |app| crate::navigation_bar_view::measured(app, &ctx, rect));
        assert!(bar.rect.is_positive() && rect.contains_rect(bar.rect));
        let click_slot = |h: &mut crate::scenario::Harness, id: &str| {
            let at = bar.slots.iter().find(|(slot, _)| slot.id == id).unwrap().1.center();
            h.step(&json!({"click":[at.x,at.y]})).unwrap();
        };
        click_slot(&mut h, "projection");
        assert_eq!(h.app.docs.views.active().id, ground);
        assert!(!h.app.perspective_view());
        assert!(crate::documents::with_view_look(&mut h.app, card, |app| app.perspective_view()));

        click_slot(&mut h, "visual_style");
        crate::test_support::click(&mut h, "nav:style:wireframe");
        assert_eq!(h.app.docs.views.active().id, ground);
        assert_eq!(h.app.ui.visual_style, 1);
        assert!(crate::documents::focus_view(&mut h.app, card));
        assert!(h.app.perspective_view());
        assert_eq!(h.app.ui.visual_style, 2);
        assert!(crate::documents::focus_view(&mut h.app, ground));
        assert!(!h.app.perspective_view());
        assert_eq!(h.app.ui.visual_style, 1);
    }

    #[test]
    fn tiled_compass_drag_stays_with_its_view_after_crossing_the_gutter() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.ui.show_browser = false;
        h.app.ui.show_timeline = false;
        h.app.preferences.navigator_style = crate::prefs::NavigatorStyle::Axes;
        h.app.preferences.orbit_cursor_wrap = false;
        let ground = h.app.docs.views.active().id;
        assert_eq!(crate::documents::view_layout(&mut h.app, solidcraft_views::Preset::SideBySide), 2);
        h.frames(4);
        let (card, rect) = h.app.viewport.cards[0];
        let ground_rect = h.app.viewport.rect.unwrap();
        let ctx = h.ctx.clone();
        let (axis, before_card) = crate::documents::with_view_look(&mut h.app, card, |app| {
            app.viewport.rect = Some(rect);
            app.viewport.card = true;
            crate::viewport::refresh_layout(app, &ctx);
            let axis = crate::viewport::axis_rect(app, rect);
            app.viewport.rect = Some(ground_rect);
            app.viewport.card = false;
            (axis, app.cam)
        });
        assert!(axis.is_positive() && rect.contains_rect(axis));
        let at = axis.center();
        let target = pos2(ground_rect.center().x, at.y + 20.0);
        let ground_cam = h.app.cam;
        h.call("ui.drag", json!({"x0":at.x,"y0":at.y,"x1":target.x,"y1":target.y,"steps":12}));
        assert_eq!(h.app.docs.views.active().id, ground);
        assert_eq!(h.app.cam, ground_cam);
        crate::documents::with_view_look(&mut h.app, card, |app| {
            assert_ne!(app.cam, before_card);
            assert!(app.viewport.axis_drag.is_none());
        });
    }

    #[test]
    fn render_ground_keeps_its_canvas_beside_a_new_sketch_card() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let model = h.app.docs.views.active().id;
        let render = crate::documents::view_open(&mut h.app, ViewKind::Render, None, false);
        assert!(crate::documents::view_close(&mut h.app, model));
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        let owner = h.app.session.sketching_view.unwrap();
        assert_ne!(owner, render);
        assert_eq!(h.app.docs.views.active().id, render);
        assert!(shown(&h.app));
        h.frame();
        assert_eq!(h.app.docs.views.active().id, render);
        assert_eq!(h.app.docs.views.get(render).unwrap().kind, ViewKind::Render);
        // #19655: the focused view picks the workspace and ribbon (workspace_follows_view, owner-approved),
        // so the Render view keeps its own ribbon and the Model card's sketch does not bring the SKETCH tab.
        assert!(crate::workspace::render_tab(&h.app.ui.tab), "the focused Render view owns the ribbon: {}", h.app.ui.tab);
        let card = h.app.viewport.cards.iter().find(|(id, _)| *id == owner).unwrap().1;
        let render_frame = crate::scenario::rect_of("render:product:frame").expect("the ground painted the Render canvas");
        assert!(!card.contains(render_frame.center()), "the Render canvas remains the ground, not the Model card");
    }

    #[test]
    fn tiled_display_modes_queue_separate_gpu_scenes() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
        h.app.viewport.gpu = Some(crate::gpu::GpuTarget {
            format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm,
            depth: None,
            samples: 1,
            tess: false,
            oit: false,
            edges: false,
        });
        h.app.ui.visual_style = 1;
        assert_eq!(crate::documents::view_layout(&mut h.app, solidcraft_views::Preset::SideBySide), 2);
        h.frames(4);
        let card = h.app.viewport.cards[0].0;
        crate::documents::with_view_look(&mut h.app, card, |app| {
            app.run("view.visual_style", json!({"style":"wireframe"})).unwrap();
        });
        h.frame();
        // egui prepares the GPU callbacks after the UI pass. Both source scenes must still be
        // present, since each renderer callback consumes its pending upload.
        let ground_slot = h.app.viewport.slot.clone();
        let ground = ground_slot.lock().unwrap().take().unwrap().merged();
        assert!(!ground.tris.is_empty());
        crate::documents::with_view_look(&mut h.app, card, |app| {
            assert!(!std::sync::Arc::ptr_eq(&ground_slot, &app.viewport.slot));
            let wire = app.viewport.slot.lock().unwrap().take().unwrap().merged();
            assert!(wire.tris.is_empty());
            assert!(!wire.lines.is_empty());
        });
    }

    #[test]
    fn a_view_keeps_its_frame_whether_it_is_the_ground_or_a_card() {
        let cell = Rect::from_min_max(pos2(10.0, 40.0), pos2(500.0, 600.0));
        let frame = frame_of(cell);
        // The same rectangle for both roles: a header's height below the cell's top, the rest of the cell.
        assert_eq!(frame, Rect::from_min_max(pos2(10.0, 40.0 + HEADER), pos2(500.0, 600.0)));
    }

    #[test]
    fn placement_controls_find_room_beside_a_floating_panel() {
        let cell = Rect::from_min_max(pos2(0.0, 0.0), pos2(400.0, 200.0));
        let browser = Rect::from_min_max(pos2(24.0, 4.0), pos2(230.0, 197.0));
        let controls = unobstructed(cell, &[browser], 4.0).unwrap();
        assert!(cell.contains_rect(controls));
        assert!(!browser.intersects(controls));
        assert!(controls.width() > 120.0, "{controls:?}");
    }
}
