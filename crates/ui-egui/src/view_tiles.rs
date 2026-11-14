//! The window showing several views of the design at once: the area split by the design's
//! [`Tiles`], each Model view drawn by the one viewport code with its own camera and analyses.
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

pub(crate) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let area = ui.max_rect();
    let Some(tiles) = app.docs.views.tiles().cloned() else { return };
    let focus = app.docs.views.active().id;
    // Below the ribbon and above the bottom strip: they float over the ground, but a card is never put under them.
    let free = crate::shell::bottom_readout_rect(app, crate::shell::panel_bounds(app, crate::shell::workspace_content(app, crate::shell::content_rect(app, area)))).shrink(MARGIN);
    let free = if free.is_positive() { free } else { area };
    let mut places: Vec<(u64, Rect)> = tiles.rects(to_tile(free), GUTTER).into_iter().map(|(id, r)| (id, from_tile(r))).collect();
    // The focused view is drawn last: what it leaves in the app (the viewport's rectangle, its
    // projection for the panels) is what the rest of the frame reads.
    places.sort_by_key(|(id, _)| *id == focus);
    // The cells with no view in them ask what to show instead of drawing one.
    let empties: Vec<Rect> = places.iter().filter(|(id, _)| *id == solidcraft_views::EMPTY).map(|(_, cell)| *cell).collect();
    places.retain(|(id, _)| *id != solidcraft_views::EMPTY);
    let press = ui.input(|i| i.pointer.any_pressed().then(|| i.pointer.interact_pos()).flatten()).filter(|p| !crate::shell::covered(app, *p));
    let mut pressed = None;
    let mut cards = Vec::new();
    let mut ground = free;
    // Set aside under everything the cards draw: the ground's picture fills the whole area.
    let layer = ui.layer_id();
    let slots: Vec<_> = (0..BACKDROP_SLOTS).map(|_| ui.painter().add(egui::Shape::Noop)).collect();
    for (id, cell) in places {
        let is_ground = id == focus;
        let frame = frame_of(cell);
        // A press on a header button is the button's: focusing the card first would take its buttons away before the click ends.
        let on_button = press.is_some_and(|p| !is_ground && (0..3).any(|slot| button_rect(cell, slot as f32).contains(p)));
        if !on_button && press.is_some_and(|p| if is_ground { frame.contains(p) } else { cell.contains(p) }) {
            pressed = Some(id);
        }
        let mut tile = ui.new_child(UiBuilder::new().id_salt(("view_tile", id)).max_rect(frame));
        tile.set_clip_rect(ui.clip_rect().intersect(if is_ground { area } else { frame }));
        app.viewport.tile = Some(slot(id));
        app.viewport.card = !is_ground;
        if is_ground {
            app.viewport.bleed = Some(area);
            app.viewport.window = Some(free);
            crate::viewport::show(app, &mut tile);
            if let Some(range) = app.viewport.backdrop.take() {
                lay_under(ui.ctx(), layer, &slots, range);
            }
            ground = frame;
        } else {
            crate::documents::with_view_look(app, id, |app| crate::viewport::show(app, &mut tile));
            cards.push((id, frame));
        }
        app.viewport.tile = None;
        app.viewport.card = false;
        app.viewport.window = None;
    }
    // Drawn over the views: the shadow and frame of each card, and the focused view's name.
    let mut chosen = None;
    for (id, body) in &cards {
        let card = Rect::from_min_max(body.min - vec2(0.0, HEADER), body.max);
        if let Some(action) = card_chrome(app, ui, *id, card) {
            chosen = Some((*id, action));
        }
    }
    let mut picked = None;
    for (ordinal, cell) in empties.iter().enumerate() {
        if let Some(action) = empty_picker(app, ui, ordinal, *cell) {
            picked = Some((ordinal, action));
        }
    }
    ground_chip(app, ui, focus, ground);
    for splitter in tiles.splitters(to_tile(free), GUTTER) {
        splitter_handle(app, ui, &splitter);
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
        None => {
            if let Some(id) = pressed.filter(|id| *id != focus) {
                crate::documents::focus_view(app, id);
            }
        }
    }
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
/// Model views that are open and not shown, and a button that closes the cell.
fn empty_picker(app: &SolveApp, ui: &mut egui::Ui, ordinal: usize, cell: Rect) -> Option<EmptyAction> {
    use solidcraft_viewport::StandardView;
    let t = Tokens::get();
    let painter = ui.painter().clone();
    painter.rect_filled(cell, CornerRadius::same(CARD_RADIUS), t.panel_header.gamma_multiply(0.7));
    let edge = [cell.left_top(), cell.right_top(), cell.right_bottom(), cell.left_bottom(), cell.left_top()];
    painter.extend(egui::Shape::dashed_line(&edge, Stroke::new(1.5, t.border), 6.0, 4.0));
    let mut action = None;
    let width = (cell.width() - 48.0).clamp(120.0, 300.0);
    let left = cell.center().x - width / 2.0;
    let open: Vec<(u64, String)> = app
        .docs
        .views
        .views()
        .iter()
        .filter(|view| view.kind == solidcraft_views::ViewKind::Model && !app.docs.views.tiles().is_some_and(|tiles| tiles.contains(view.id)))
        .take(4)
        .map(|view| (view.id, view.name.clone()))
        .collect();
    let height = 22.0 + 2.0 * 34.0 + if open.is_empty() { 0.0 } else { 6.0 + open.len() as f32 * 34.0 };
    let mut y = (cell.center().y - height / 2.0).max(cell.top() + 8.0);
    let title = solidcraft_ui_text::tr_label("Show a view here");
    painter.text(pos2(cell.center().x, y + 8.0), egui::Align2::CENTER_CENTER, title, egui::FontId::proportional(t.font_body), t.text);
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
    for (i, angle) in [StandardView::Front, StandardView::Right, StandardView::Top, StandardView::Iso].into_iter().enumerate() {
        let (col, row) = (i % 2, i / 2);
        let rect = Rect::from_min_size(pos2(left + col as f32 * (half + 6.0), y + row as f32 * 34.0), vec2(half, 28.0));
        let label = solidcraft_ui_text::tr_label(["Front", "Right", "Top", "Iso"][i]);
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
    let close = Rect::from_center_size(cell.right_top() + vec2(-18.0, 18.0), vec2(22.0, 22.0));
    let resp = ui.interact(close, ui.scope_id().with(("empty_cell_close", ordinal)), Sense::click()).on_hover_text(solidcraft_ui_text::tr_label("Close this cell"));
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
    let shadow = egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: egui::Color32::from_black_alpha(70) }.as_shape(card, radius);
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
    // The picture is square: the ground's colour fills what the rounded corners leave out.
    let area = ui.max_rect();
    let backdrop = |y: f32| {
        let k = ((y - area.top()) / area.height().max(1.0)).clamp(0.0, 1.0);
        t.viewport_top.lerp_to_gamma(t.viewport_bottom, k)
    };
    corner_mask(&painter, card.left_bottom(), [1.0, -1.0], backdrop(card.bottom()));
    corner_mask(&painter, card.right_bottom(), [-1.0, -1.0], backdrop(card.bottom()));
    painter.rect_stroke(card.shrink(0.5), radius, Stroke::new(1.0, t.border), StrokeKind::Inside);
    let mut action = None;
    let mut button = |slot: f32, kind: CardAction| {
        let rect = button_rect(card, slot);
        let resp = ui.interact(rect, ui.scope_id().with(("card_button", id, slot as i32)), Sense::click());
        let tip = match kind {
            CardAction::Maximise => solidcraft_ui_text::tr_label("Maximise this view"),
            CardAction::Remove => solidcraft_ui_text::tr_label("Remove from this window"),
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
    action
}

/// What a card's square picture leaves outside its rounded corner, at the card's `corner` (the
/// direction to the card's inside given by `inward`), filled with `color`.
fn corner_mask(painter: &egui::Painter, corner: egui::Pos2, inward: [f32; 2], color: egui::Color32) {
    let r = f32::from(CARD_RADIUS);
    let centre = corner + vec2(inward[0] * r, inward[1] * r);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(corner, color);
    const STEPS: u32 = 8;
    for i in 0..=STEPS {
        let a = std::f32::consts::FRAC_PI_2 * i as f32 / STEPS as f32;
        mesh.colored_vertex(centre + vec2(-inward[0] * r * a.cos(), -inward[1] * r * a.sin()), color);
    }
    for i in 1..=STEPS {
        mesh.add_triangle(0, i, i + 1);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// The focused view has no frame and no header: it is the ground the others lie on. Its name sits
/// in a pill with the accent in the band above its frame, clear of the ribbon and the panels.
fn ground_chip(app: &SolveApp, ui: &egui::Ui, id: u64, ground: Rect) {
    let t = Tokens::get();
    let name = app.docs.views.get(id).map_or("", |view| view.name.as_str());
    let painter = ui.painter();
    let galley = painter.layout_no_wrap(name.to_owned(), egui::FontId::proportional(t.font_body), t.text);
    let size = vec2(galley.size().x + 34.0, 26.0);
    let at = pos2(ground.left() + MARGIN, ground.top() - HEADER + (HEADER - size.y) / 2.0);
    let chip = clear_of_panels(app, Rect::from_min_size(at, size));
    let radius = CornerRadius::same(13);
    painter.add(egui::Shadow { offset: [0, 2], blur: 8, spread: 0, color: egui::Color32::from_black_alpha(60) }.as_shape(chip, radius));
    painter.rect_filled(chip, radius, t.panel_header);
    painter.rect_stroke(chip, radius, Stroke::new(1.0, t.border), StrokeKind::Inside);
    painter.circle_filled(chip.left_center() + vec2(13.0, 0.0), 3.5, t.accent);
    painter.galley(chip.left_center() + vec2(24.0, -galley.size().y / 2.0), galley, t.text);
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
    fn a_view_keeps_its_frame_whether_it_is_the_ground_or_a_card() {
        let cell = Rect::from_min_max(pos2(10.0, 40.0), pos2(500.0, 600.0));
        let frame = frame_of(cell);
        // The same rectangle for both roles: a header's height below the cell's top, the rest of the cell.
        assert_eq!(frame, Rect::from_min_max(pos2(10.0, 40.0 + HEADER), pos2(500.0, 600.0)));
    }
}
