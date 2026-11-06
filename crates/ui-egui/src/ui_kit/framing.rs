//! Shared aspect choices and camera composition guides; no document or command authority.
use crate::{scenario, theme::Tokens};
use egui::{Rect, Stroke, Ui, pos2, vec2};

pub(crate) struct AspectTile<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub aspect: f64,
    pub selected: bool,
}

/// Three-column compact illustrated choices, using the same control surface as other choices.
pub(crate) fn aspect_grid(ui: &mut Ui, tiles: &[AspectTile<'_>]) -> Option<usize> {
    let t = Tokens::get();
    let accent = crate::theme::TabAccent::current(ui.ctx());
    let mut clicked = None;
    for (row, tiles) in tiles.chunks(3).enumerate() {
        ui.columns(3, |columns| {
            for (column, tile) in tiles.iter().enumerate() {
                let Some(ui) = columns.get_mut(column) else { continue };
                let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), t.control_height * 2.0 + t.space_8), egui::Sense::click());
                let fill = if tile.selected {
                    accent.soft
                } else if response.hovered() {
                    t.hover
                } else {
                    t.control_fill
                };
                let ink = if tile.selected { accent.ink } else { t.form_ink };
                ui.painter().rect_filled(rect, t.control_radius, fill);
                ui.painter().rect_stroke(
                    rect,
                    t.control_radius,
                    t.control_border_stroke(if tile.selected { accent.ink } else { t.control_border }),
                    egui::StrokeKind::Inside,
                );
                let available = vec2((rect.width() - t.space_16).max(0.0), t.control_height);
                let aspect = if tile.aspect.is_finite() && tile.aspect > 0.0 { tile.aspect as f32 } else { 1.0 };
                let width = available.x.min(available.y * aspect);
                let size = vec2(width, width / aspect);
                let glyph = Rect::from_center_size(pos2(rect.center().x, rect.top() + t.space_8 + available.y / 2.0), size);
                ui.painter().rect_stroke(glyph, 0, Stroke::new(1.0, ink), egui::StrokeKind::Inside);
                ui.painter().text(
                    pos2(rect.center().x, rect.bottom() - t.space_8),
                    egui::Align2::CENTER_BOTTOM,
                    tile.label,
                    egui::FontId::proportional(t.font_small),
                    ink,
                );
                response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), tile.selected, tile.label));
                scenario::publish_target(tile.id, rect);
                if response.clicked() {
                    clicked = Some(row * 3 + column);
                }
            }
        });
    }
    clicked
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct FrameGuides {
    pub thirds: bool,
    pub safe_area: bool,
    pub centre_mark: bool,
}

/// Pure viewport decoration; callers keep this out of image renderers and result canvases.
pub(crate) fn frame_guides(ui: &Ui, viewport: Rect, frame: Rect, guides: FrameGuides) {
    if !viewport.is_finite() || !frame.is_finite() || !viewport.is_positive() || !frame.is_positive() {
        return;
    }
    let t = Tokens::get();
    let painter = ui.painter().with_clip_rect(viewport);
    let mask = t.app_bar.gamma_multiply(0.45);
    for outside in [
        Rect::from_min_max(viewport.min, pos2(viewport.right(), frame.top())),
        Rect::from_min_max(pos2(viewport.left(), frame.bottom()), viewport.max),
        Rect::from_min_max(pos2(viewport.left(), frame.top()), frame.left_bottom()),
        Rect::from_min_max(frame.right_top(), pos2(viewport.right(), frame.bottom())),
    ] {
        if outside.is_positive() {
            painter.rect_filled(outside, 0, mask);
        }
    }
    let stroke = Stroke::new(1.0, t.form_ink.gamma_multiply(0.65));
    painter.rect_stroke(frame, 0, stroke, egui::StrokeKind::Inside);
    let guide = Stroke::new(1.0, t.form_ink.gamma_multiply(0.3));
    if guides.thirds {
        for fraction in [1.0 / 3.0, 2.0 / 3.0] {
            painter.vline(frame.left() + frame.width() * fraction, frame.y_range(), guide);
            painter.hline(frame.x_range(), frame.top() + frame.height() * fraction, guide);
        }
    }
    if guides.safe_area {
        let safe = frame.shrink2(frame.size() * 0.05);
        for [a, b] in [
            [safe.left_top(), safe.right_top()],
            [safe.right_top(), safe.right_bottom()],
            [safe.right_bottom(), safe.left_bottom()],
            [safe.left_bottom(), safe.left_top()],
        ] {
            let dash = t.space_4.max(a.distance(b) / 256.0);
            painter.extend(egui::Shape::dashed_line(&[a, b], guide, dash, dash));
        }
    }
    if guides.centre_mark {
        let c = frame.center();
        painter.line_segment([c - vec2(t.space_8, 0.0), c + vec2(t.space_8, 0.0)], stroke);
        painter.line_segment([c - vec2(0.0, t.space_8), c + vec2(0.0, t.space_8)], stroke);
    }
}
