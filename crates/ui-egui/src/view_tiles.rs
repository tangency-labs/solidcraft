//! The window showing several views of the design at once: the area split by the design's
//! [`Tiles`], each Model view drawn by the one viewport code with its own camera and analyses.
//!
//! The focused view is the *ground*: it fills the window behind the ribbon and every panel, with no
//! frame and no header, so the app feels the same as with one view. The others are *cards*: laid
//! out below the ribbon, each with a frame, a shadow, a header and a compass of its own, so they
//! stand off the ground and a header is never hidden. A card and the ground never overlap, so the
//! order they are drawn in does not matter to the picture.
//!
//! The focused view is the design's shown view (what the ribbon, the browser and the commands act
//! on); the cards stand in for it while they are drawn ([`crate::documents::with_view_look`]), so
//! a tile needs no code of its own for orbiting, picking or analysing. A press inside a view makes
//! it the focused one. Every view reaches the GPU as a tile of its own
//! ([`solidcraft_viewport_egui::TileCallback`]).

use egui::{CornerRadius, Rect, Sense, Stroke, StrokeKind, UiBuilder, pos2, vec2};
use solidcraft_theme::Tokens;
use solidcraft_views::{Axis, Rect as TileRect};

use crate::SolveApp;

/// The space between neighbouring views.
const GUTTER: f32 = 12.0;
/// The space between the cards and the edge of the free workspace.
const MARGIN: f32 = 8.0;
/// Height of a card's header.
const HEADER: f32 = 28.0;
/// Corner radius of a card.
const CARD_RADIUS: u8 = 8;

/// The window shows more than one view of the design.
pub(crate) fn shown(app: &SolveApp) -> bool {
    app.docs.views.is_tiled()
}

fn to_tile(rect: Rect) -> TileRect {
    TileRect { x: rect.min.x, y: rect.min.y, w: rect.width(), h: rect.height() }
}

fn from_tile(rect: TileRect) -> Rect {
    Rect::from_min_size(pos2(rect.x, rect.y), vec2(rect.w, rect.h))
}

/// The tile's slot on the GPU: the view's own id.
fn slot(view: u64) -> u32 {
    u32::try_from(view).unwrap_or(u32::MAX)
}

/// The focused view's rectangle: its cell, run out to the window's edge on every side where the
/// cell meets the free workspace (so it lies behind the ribbon and the panels), and to the middle
/// of the gutter on the sides that meet a card.
fn ground_rect(cell: Rect, free: Rect, area: Rect) -> Rect {
    let edge = |c: f32, f: f32, a: f32, toward: f32| if (c - f).abs() < 1.0 { a } else { c + toward };
    let half = GUTTER / 2.0;
    Rect::from_min_max(
        pos2(edge(cell.min.x, free.min.x, area.min.x, -half), edge(cell.min.y, free.min.y, area.min.y, -half)),
        pos2(edge(cell.max.x, free.max.x, area.max.x, half), edge(cell.max.y, free.max.y, area.max.y, half)),
    )
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

pub(crate) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let area = ui.max_rect();
    let Some(tiles) = app.docs.views.tiles().cloned() else { return };
    let focus = app.docs.views.active().id;
    let free = crate::shell::workspace_content(app, crate::shell::content_rect(app, area)).shrink(MARGIN);
    let free = if free.is_positive() { free } else { area };
    let mut places: Vec<(u64, Rect)> = tiles.rects(to_tile(free), GUTTER).into_iter().map(|(id, r)| (id, from_tile(r))).collect();
    // The focused view is drawn last: what it leaves in the app (the viewport's rectangle, its
    // projection for the panels) is what the rest of the frame reads.
    places.sort_by_key(|(id, _)| *id == focus);
    let press = ui.input(|i| i.pointer.any_pressed().then(|| i.pointer.interact_pos()).flatten()).filter(|p| !crate::shell::covered(app, *p));
    let mut pressed = None;
    let mut cards = Vec::new();
    let mut ground = area;
    for (id, cell) in places {
        let is_ground = id == focus;
        let rect = if is_ground { ground_rect(cell, free, area) } else { cell };
        if press.is_some_and(|p| rect.contains(p)) {
            pressed = Some(id);
        }
        let body = if is_ground { rect } else { Rect::from_min_max(rect.min + vec2(0.0, HEADER), rect.max) };
        let mut tile = ui.new_child(UiBuilder::new().id_salt(("view_tile", id)).max_rect(body));
        tile.set_clip_rect(ui.clip_rect().intersect(body));
        app.viewport.tile = Some(slot(id));
        if is_ground {
            crate::viewport::show(app, &mut tile);
            ground = rect;
        } else {
            crate::documents::with_view_look(app, id, |app| crate::viewport::show(app, &mut tile));
            cards.push((id, body));
        }
        app.viewport.tile = None;
    }
    // Drawn over the views: the shadow and frame of each card, and the focused view's name.
    let mut chosen = None;
    for (id, body) in &cards {
        let card = Rect::from_min_max(body.min - vec2(0.0, HEADER), body.max);
        if let Some(action) = card_chrome(app, ui, *id, card) {
            chosen = Some((*id, action));
        }
    }
    ground_chip(app, ui, focus, ground, free);
    for splitter in tiles.splitters(to_tile(free), GUTTER) {
        splitter_handle(app, ui, &splitter);
    }
    // The shown view's rectangle is the one the panels and the host's controls read.
    app.viewport.rect = Some(ground);
    app.viewport.cards = cards;
    match chosen {
        Some((id, CardAction::Maximise)) => {
            crate::documents::focus_view(app, id);
            crate::documents::view_layout(app, solidcraft_views::Preset::Single);
        }
        Some((id, CardAction::Close)) => {
            crate::documents::view_close(app, id);
        }
        None => {
            if let Some(id) = pressed.filter(|id| *id != focus) {
                crate::documents::focus_view(app, id);
            }
        }
    }
}

/// What a card's header buttons ask for.
enum CardAction {
    /// Show this view alone, in the whole window.
    Maximise,
    Close,
}

/// The controls of the cards (the compass), each acting on its own view. The focused view's are
/// the host's, drawn after this.
pub(crate) fn card_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let ground = app.viewport.rect;
    let cards = std::mem::take(&mut app.viewport.cards);
    let bar = std::mem::replace(&mut app.ui.show_navigation_bar, false);
    for (id, rect) in &cards {
        let mut controls = ui.new_child(UiBuilder::new().id_salt(("card_controls", id)).max_rect(*rect));
        controls.set_clip_rect(ui.clip_rect().intersect(*rect));
        app.viewport.rect = Some(*rect);
        crate::documents::with_view_look(app, *id, |app| crate::viewport::show_card_controls(app, &mut controls, *rect));
    }
    app.ui.show_navigation_bar = bar;
    app.viewport.rect = ground;
    app.viewport.cards = cards;
}

/// A card lifted off the focused view: a soft shadow around it, its frame, and a header with the
/// view's name that stays clear of the panels floating over it.
fn card_chrome(app: &SolveApp, ui: &mut egui::Ui, id: u64, card: Rect) -> Option<CardAction> {
    let t = Tokens::get();
    let radius = CornerRadius::same(CARD_RADIUS);
    let shadow = egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: t.app_bar.gamma_multiply(0.30) }.as_shape(card, radius);
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
    let top = Rect::from_min_size(card.min, vec2(card.width(), HEADER));
    painter.rect_filled(top, CornerRadius { nw: CARD_RADIUS, ne: CARD_RADIUS, sw: 0, se: 0 }, t.panel_header);
    let name = app.docs.views.get(id).map_or("", |view| view.name.as_str());
    let label = clear_of_panels(app, Rect::from_min_size(top.left_top() + vec2(10.0, 0.0), vec2(120.0, HEADER)));
    painter.text(label.left_center(), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(t.font_body), t.text);
    painter.rect_stroke(card.shrink(0.5), radius, Stroke::new(1.0, t.border), StrokeKind::Inside);
    let mut action = None;
    let mut button = |slot: f32, kind: CardAction| {
        let rect = Rect::from_center_size(top.right_center() - vec2(16.0 + slot * 24.0, 0.0), vec2(22.0, 22.0));
        let resp = ui.interact(rect, ui.scope_id().with(("card_button", id, slot as i32)), Sense::click());
        let tip = match kind {
            CardAction::Maximise => solidcraft_ui_text::tr_label("Maximise this view"),
            CardAction::Close => solidcraft_ui_text::tr_label("Close view"),
        };
        let resp = resp.on_hover_text(tip);
        if resp.hovered() {
            painter.rect_filled(rect, CornerRadius::same(5), t.accent_soft);
        }
        let ink = Stroke::new(1.6, if resp.hovered() { t.text } else { t.text_dim });
        let c = rect.center();
        match kind {
            CardAction::Maximise => {
                painter.rect_stroke(Rect::from_center_size(c, vec2(10.0, 10.0)), CornerRadius::same(2), ink, StrokeKind::Inside);
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
    action
}

/// The focused view has no frame and no header: it is the ground the others lie on. Its name sits
/// in a small chip with the accent, clear of the ribbon and the panels.
fn ground_chip(app: &SolveApp, ui: &egui::Ui, id: u64, ground: Rect, free: Rect) {
    let t = Tokens::get();
    let name = app.docs.views.get(id).map_or("", |view| view.name.as_str());
    let painter = ui.painter();
    let galley = painter.layout_no_wrap(name.to_owned(), egui::FontId::proportional(t.font_body), t.text);
    let size = vec2(galley.size().x + 32.0, 26.0);
    let at = pos2(ground.left().max(free.left()), free.top());
    let chip = clear_of_panels(app, Rect::from_min_size(at, size));
    painter.rect_filled(chip, CornerRadius::same(13), t.panel_header);
    painter.rect_stroke(chip, CornerRadius::same(13), Stroke::new(1.0, t.border), StrokeKind::Inside);
    painter.circle_filled(chip.left_center() + vec2(12.0, 0.0), 3.5, t.accent);
    let text_at = chip.left_center() + vec2(22.0, -galley.size().y / 2.0);
    painter.galley(text_at, galley, t.text);
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

    #[test]
    fn the_ground_runs_to_the_window_edge_and_stops_in_the_gutter() {
        let area = Rect::from_min_max(pos2(0.0, 0.0), pos2(1000.0, 600.0));
        let free = area.shrink(MARGIN);
        // The left of two cells: it meets the free area on three sides.
        let left = Rect::from_min_max(free.min, pos2(500.0 - GUTTER / 2.0, free.max.y));
        assert_eq!(ground_rect(left, free, area), Rect::from_min_max(area.min, pos2(500.0, area.max.y)));
        // A cell in the middle of the area keeps to its cell, grown by half a gutter.
        let middle = Rect::from_min_max(pos2(100.0, 100.0), pos2(200.0, 200.0));
        assert_eq!(ground_rect(middle, free, area), middle.expand(GUTTER / 2.0));
    }
}
