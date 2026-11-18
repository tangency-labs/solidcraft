//! A card the user resizes by its edges, as the Parameters and Configurations cards are: the free
//! side edge sets its width, the bottom edge its body height, the corner both, and a double-click
//! on an edge returns that size to automatic. The chosen sizes are kept in egui's persisted memory,
//! so the card opens at them next time.

/// One resizable card: where its chosen width and body height are kept, and the prefix of the
/// names its edges publish (`<handle>:resize:side`, `:bottom`, `:corner`).
pub(crate) struct CardSize {
    pub width: &'static str,
    pub height: &'static str,
    pub handle: &'static str,
}

impl CardSize {
    /// The outer width the user dragged the card to; `None` while it is automatic.
    pub fn width(&self, ctx: &egui::Context) -> Option<f32> {
        ctx.data_mut(|m| m.get_persisted(egui::Id::unique(self.width))).filter(|w: &f32| w.is_finite())
    }

    /// The body height the user dragged the card to; `None` while it is automatic.
    pub fn height(&self, ctx: &egui::Context) -> Option<f32> {
        ctx.data_mut(|m| m.get_persisted(egui::Id::unique(self.height))).filter(|h: &f32| h.is_finite())
    }

    /// The card's resize edges, read inside its window after the body is drawn.
    pub fn edges(&self, ui: &mut egui::Ui, side: crate::frame::Side, scrollbar: Option<egui::Rect>) -> Resize {
        edges(ui, self.handle, side, scrollbar)
    }

    /// Keep what the edges asked for: `card` is the card's outer rectangle and `body` its body
    /// height on this frame; both stay within `viewport`.
    pub fn apply(&self, ctx: &egui::Context, resize: Resize, card: Option<egui::Rect>, body: Option<f32>, viewport: egui::Rect) {
        let (width_id, height_id) = (egui::Id::unique(self.width), egui::Id::unique(self.height));
        match resize {
            Resize::None => {}
            Resize::Reset(axes) => ctx.data_mut(|m| {
                if axes.width {
                    m.remove::<f32>(width_id);
                }
                if axes.height {
                    m.remove::<f32>(height_id);
                }
            }),
            Resize::By(axes, delta) => {
                let t = solidcraft_theme::Tokens::get();
                if axes.width
                    && let Some(card) = card
                {
                    let w = (card.width() + delta.x).clamp(crate::frame::DIALOG_MIN_W, viewport.width().max(crate::frame::DIALOG_MIN_W));
                    if w.is_finite() {
                        ctx.data_mut(|m| m.insert_persisted(width_id, w));
                    }
                }
                if axes.height
                    && let Some(body) = body
                {
                    let h = (body + delta.y).clamp(t.control_height * 2.0, viewport.height().max(t.control_height * 2.0));
                    if h.is_finite() {
                        ctx.data_mut(|m| m.insert_persisted(height_id, h));
                    }
                }
            }
        }
    }
}

/// Which of the card's two sizes a resize edge changes: the side edge its width, the bottom edge
/// its body height, the corner both. The other axis keeps its automatic size.
#[derive(Clone, Copy)]
pub(crate) struct Axes {
    width: bool,
    height: bool,
}

/// What the card's resize edges asked for this frame.
pub(crate) enum Resize {
    None,
    /// Grow the card by this much on these axes: x widens it, y lengthens its body.
    By(Axes, egui::Vec2),
    /// A double-click on an edge returns these axes to their automatic size.
    Reset(Axes),
}

/// Drag areas on the card's free side edge (the one away from its dock side), its bottom edge
/// and the corner between them. The header stays the title drag area. When the free edge is the
/// right one and the body scrolls, `scrollbar` is the body viewport whose right edge carries the
/// scrollbar: the side grip skips that span so scrollbar drags still scroll (#13422 review).
fn edges(ui: &mut egui::Ui, handle: &str, side: crate::frame::Side, scrollbar: Option<egui::Rect>) -> Resize {
    let t = solidcraft_theme::Tokens::get();
    let card = ui.min_rect();
    let grip = t.space_6;
    if !card.is_finite() || card.width() < grip * 4.0 || card.height() < t.form_header_height + grip * 2.0 {
        return Resize::None;
    }
    // A card docked right is held by its right edge, so it grows from its left edge.
    let free_left = side == crate::frame::Side::Right;
    let x_edge = if free_left { card.left()..=card.left() + grip } else { card.right() - grip..=card.right() };
    let corner_x = if free_left { card.left()..=card.left() + grip * 2.0 } else { card.right() - grip * 2.0..=card.right() };
    // The scrollbar sits on the body's right edge, so only a right-hand grip can cover it.
    let side_top = match scrollbar {
        Some(body) if !free_left && body.is_finite() => body.bottom().max(card.top() + t.form_header_height),
        _ => card.top() + t.form_header_height,
    };
    let mut edges = vec![(
        "bottom",
        egui::Rect::from_x_y_ranges(card.left() + grip * 2.0..=card.right() - grip * 2.0, card.bottom() - grip..=card.bottom()),
        egui::CursorIcon::ResizeVertical,
        Axes { width: false, height: true },
    )];
    if side_top < card.bottom() - grip * 2.0 {
        edges.push((
            "side",
            egui::Rect::from_x_y_ranges(x_edge, side_top..=card.bottom() - grip * 2.0),
            egui::CursorIcon::ResizeHorizontal,
            Axes { width: true, height: false },
        ));
    }
    edges.push((
        "corner",
        egui::Rect::from_x_y_ranges(corner_x, card.bottom() - grip * 2.0..=card.bottom()),
        if free_left { egui::CursorIcon::ResizeNeSw } else { egui::CursorIcon::ResizeNwSe },
        Axes { width: true, height: true },
    ));
    let mut out = Resize::None;
    for (name, rect, cursor, axes) in edges {
        let response = ui.interact(rect, ui.make_persistent_id((handle, "card_resize", name)), egui::Sense::click_and_drag()).on_hover_cursor(cursor);
        crate::scenario::publish_target(&format!("{handle}:resize:{name}"), rect);
        if response.dragged() {
            ui.ctx().set_cursor_icon(cursor);
        }
        paint_grip(ui, name, rect, free_left, response.hovered() || response.dragged());
        if response.double_clicked() {
            out = Resize::Reset(axes);
        } else if response.dragged() {
            let delta = response.drag_delta();
            let delta =
                egui::vec2(if axes.width { if free_left { -delta.x } else { delta.x } } else { 0.0 }, if axes.height { delta.y } else { 0.0 });
            if delta.is_finite() && delta != egui::Vec2::ZERO {
                out = Resize::By(axes, delta);
            }
        }
    }
    out
}

/// A visible mark on the side edge and the corner, so the card shows where it can be dragged to
/// a new size (#18846): two short lines across the middle of the side edge and three diagonal
/// lines in the corner, drawn stronger while the pointer is on them. The bottom edge, like the
/// header, is found by its cursor.
fn paint_grip(ui: &egui::Ui, name: &str, rect: egui::Rect, free_left: bool, active: bool) {
    let t = solidcraft_theme::Tokens::get();
    let color = if active { t.form_primary } else { t.form_dim.gamma_multiply(0.6) };
    let stroke = egui::Stroke::new(1.0, color);
    let painter = ui.painter();
    match name {
        "side" => {
            let half = (t.space_6 * 2.0).min(rect.height() * 0.5);
            let y = rect.center().y;
            for dx in [-1.5, 1.5] {
                painter.vline(rect.center().x + dx, y - half..=y + half, stroke);
            }
        }
        "corner" => {
            // The corner's outer point is the card's bottom corner on the free side.
            let corner = if free_left { rect.left_bottom() } else { rect.right_bottom() };
            let toward_x = if free_left { 1.0 } else { -1.0 };
            let inset = 2.0;
            let tip = corner + egui::vec2(inset * toward_x, -inset);
            for step in 1..=3 {
                let d = step as f32 * t.space_6 * 0.5;
                painter.line_segment([tip + egui::vec2(d * toward_x, 0.0), tip + egui::vec2(0.0, -d)], stroke);
            }
        }
        _ => {}
    }
}
