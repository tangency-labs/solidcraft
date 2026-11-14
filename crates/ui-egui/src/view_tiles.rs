//! The window showing several views of the design at once: the area split by the design's
//! [`Tiles`], each Model view drawn by the one viewport code with its own camera and analyses, a
//! header on each, a ring on the focused one and splitters to drag between them.
//!
//! The focused view is the design's shown view (what the ribbon, the browser and the commands act
//! on); the others stand in for it while they are drawn ([`crate::documents::with_view_look`]), so
//! a tile needs no code of its own for orbiting, picking or analysing. A press inside a tile makes
//! it the focused one. Every tile reaches the GPU as a tile of its own
//! ([`solidcraft_viewport_egui::TileCallback`]), so they do not draw over one another.

use egui::{CornerRadius, Rect, Sense, Stroke, StrokeKind, UiBuilder, pos2, vec2};
use solidcraft_theme::Tokens;
use solidcraft_views::{Axis, Rect as TileRect};

use crate::SolveApp;

/// The space between neighbouring views.
const GUTTER: f32 = 6.0;
/// Height of a view's header.
const HEADER: f32 = 28.0;

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

pub(crate) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let area = ui.max_rect();
    let Some(tiles) = app.docs.views.tiles().cloned() else { return };
    let focus = app.docs.views.active().id;
    let mut places = tiles.rects(to_tile(area), GUTTER);
    // The focused view is drawn last: what it leaves in the app (the viewport's rectangle, its
    // projection for the panels) is what the rest of the frame reads.
    places.sort_by_key(|(id, _)| *id == focus);
    let press = ui.input(|i| i.pointer.any_pressed().then(|| i.pointer.interact_pos()).flatten()).filter(|p| !crate::shell::covered(app, *p));
    let mut pressed = None;
    for (id, place) in places {
        let rect = from_tile(place);
        if press.is_some_and(|p| rect.contains(p)) {
            pressed = Some(id);
        }
        let body = Rect::from_min_max(rect.min + vec2(0.0, HEADER), rect.max);
        let mut tile = ui.new_child(UiBuilder::new().id_salt(("view_tile", id)).max_rect(body));
        tile.set_clip_rect(ui.clip_rect().intersect(body));
        app.viewport.tile = Some(slot(id));
        if id == focus {
            crate::viewport::show(app, &mut tile);
        } else {
            crate::documents::with_view_look(app, id, |app| crate::viewport::show(app, &mut tile));
        }
        app.viewport.tile = None;
        header(app, ui, id, rect, id == focus);
    }
    for splitter in tiles.splitters(to_tile(area), GUTTER) {
        splitter_handle(app, ui, &splitter);
    }
    // Panels anchor to the whole area, not to the tile that was drawn last.
    app.viewport.rect = Some(area);
    if let Some(id) = pressed.filter(|id| *id != focus) {
        crate::documents::focus_view(app, id);
    }
}

/// A view's name over its tile, and the ring that says which one commands go to.
fn header(app: &SolveApp, ui: &egui::Ui, id: u64, rect: Rect, focused: bool) {
    let t = Tokens::get();
    let painter = ui.painter().with_clip_rect(rect);
    let top = Rect::from_min_size(rect.min, vec2(rect.width(), HEADER));
    painter.rect_filled(top, CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }, t.panel_header);
    let name = app.docs.views.get(id).map_or("", |view| view.name.as_str());
    let ink = if focused { t.text } else { t.text_dim };
    painter.text(top.left_center() + vec2(10.0, 0.0), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(t.font_body), ink);
    let ring = if focused { Stroke::new(2.0, t.accent) } else { Stroke::new(1.0, t.border) };
    painter.rect_stroke(rect.shrink(ring.width / 2.0), CornerRadius::same(6), ring, StrokeKind::Inside);
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
