//! A run of history items folded into one chip: the icon, a name and the count, then, past a
//! rule, the disclosure arrow in a well of its own. The joints, feature folders and long runs of a
//! component's features use it. Open, a group's items show after its chip inside a tray that
//! reaches from the chip to its last item.
use crate::{
    icons,
    theme::{TabAccent, Tokens},
};
use egui::{Painter, Pos2, Rect, Stroke, pos2, vec2};

/// The arrow's square well at the chip's right end.
pub(crate) const GROUP_ARROW_WELL: f32 = 20.0;
/// Chips in the History bar are this tall, and the icon sits in a square this wide.
const CHIP_SIDE: f32 = 28.0;
/// A longer name is cut at this width (the tooltip and the Browser have it whole).
const NAME_MAX: f32 = 96.0;
/// Items past the history marker fade like feature chips do.
const DIM_OPACITY: f32 = 0.42;
/// How far an open group's tray reaches past its chip and its last item.
pub(crate) const HISTORY_TRAY_PAD: f32 = 4.0;
/// How strongly a tray's edge takes the accent.
const TRAY_EDGE: f32 = 0.6;

/// The tray behind an open group in the History bar, from `left` to `right` around chips whose
/// tops are at `chip_top`, `depth` trays deep. An outer tray takes the accent's soft tint and a
/// tray inside one the bar's own colour, so each level stands apart from the one around it;
/// deeper trays hug the chips a little closer.
pub(crate) fn history_tray(left: f32, right: f32, chip_top: f32, depth: usize, accent: TabAccent) -> egui::Shape {
    let t = Tokens::get();
    let inset = (HISTORY_TRAY_PAD - depth as f32).max(1.0);
    let rect = Rect::from_min_max(pos2(left, chip_top - inset), pos2(right.max(left), chip_top + CHIP_SIDE + inset));
    let fill = if depth.is_multiple_of(2) { accent.soft } else { t.timeline };
    let edge = Stroke::new(1.0, accent.color.gamma_multiply(TRAY_EDGE));
    egui::Shape::Rect(egui::epaint::RectShape::new(rect, f32::from(t.radius_sm) + inset, fill, edge, egui::StrokeKind::Inside))
}

/// The arrow between two steps of the History bar's path, from `at` (its left middle). Returns
/// where it ends. Tokens: form_dim.
pub(crate) fn history_crumb_arrow(p: &Painter, at: Pos2) -> f32 {
    let t = Tokens::get();
    let side = t.form_section_icon_size;
    icons::paint_ink(p, Rect::from_center_size(pos2(at.x + side / 2.0, at.y), vec2(side, side)), "chevron_right", t.form_dim);
    at.x + side
}

pub(crate) struct HistoryGroupChip<'a> {
    pub icon: &'a str,
    /// Shown before the count; empty for none.
    pub name: &'a str,
    pub count: usize,
    /// The grouped items are shown one by one after the chip.
    pub open: bool,
    /// Something inside the folded group is selected.
    pub selected: bool,
    /// Something inside is under the pointer elsewhere (in the viewport).
    pub hovered: bool,
    /// Everything inside is past the history marker.
    pub dim: bool,
    pub error: bool,
    pub warning: bool,
    pub accent: TabAccent,
}

impl HistoryGroupChip<'_> {
    fn name_width(&self, p: &Painter) -> f32 {
        if self.name.is_empty() {
            return 0.0;
        }
        let t = Tokens::get();
        let galley = p.layout_no_wrap(self.name.to_owned(), egui::FontId::proportional(t.font_small), t.text);
        galley.size().x.min(NAME_MAX) + t.space_4
    }

    fn count_text(&self, p: &Painter) -> std::sync::Arc<egui::Galley> {
        let t = Tokens::get();
        let ink = if self.name.is_empty() { t.text } else { t.text_dim };
        p.layout_no_wrap(self.count.to_string(), egui::FontId::proportional(t.font_small), ink)
    }

    /// The chip's width; its height is that of a feature chip.
    pub fn width(&self, p: &Painter) -> f32 {
        let t = Tokens::get();
        CHIP_SIDE + t.space_3 + self.name_width(p) + self.count_text(p).size().x + t.space_6 + 1.0 + t.space_3 + GROUP_ARROW_WELL + t.space_4
    }

    pub fn size(&self, p: &Painter) -> egui::Vec2 {
        vec2(self.width(p), CHIP_SIDE)
    }

    /// The arrow's well in a chip drawn at `rect`.
    pub fn well(rect: Rect) -> Rect {
        let t = Tokens::get();
        Rect::from_center_size(pos2(rect.right() - t.space_4 - GROUP_ARROW_WELL / 2.0, rect.center().y), vec2(GROUP_ARROW_WELL, GROUP_ARROW_WELL))
    }

    /// Whether `pointer` is on the arrow: from the rule to the chip's end.
    pub fn on_arrow(rect: Rect, pointer: Option<Pos2>) -> bool {
        pointer.is_some_and(|q| q.x >= Self::well(rect).left() - Tokens::get().space_3)
    }

    /// Paints the chip at `rect`; `pointer` is where the pointer is while it is over the chip. The
    /// arrow's well lights up under the pointer, and the chip takes the accent while its items are
    /// shown one by one or while it holds the selection.
    pub fn paint(&self, painter: &Painter, rect: Rect, pointer: Option<Pos2>) {
        let t = Tokens::get();
        let mut p = painter.clone();
        if self.dim {
            p.multiply_opacity(DIM_OPACITY);
        }
        let on_arrow = Self::on_arrow(rect, pointer);
        let (fill, edge) = if self.error {
            (t.form_error_fill, t.form_error)
        } else if self.open || self.selected {
            (self.accent.soft, self.accent.color)
        } else if self.hovered || (pointer.is_some() && !on_arrow) {
            (t.hover, t.border)
        } else {
            (t.timeline_item, t.border)
        };
        p.rect(rect, t.radius_sm, fill, Stroke::new(1.0, edge), egui::StrokeKind::Inside);
        let square = Rect::from_min_size(rect.min, vec2(CHIP_SIDE, CHIP_SIDE));
        icons::paint_icon(&p, square.shrink(t.space_4), self.icon, f32::from(t.icon.a()) / 255.0);
        let mut x = square.right() - t.space_4 + t.space_3;
        if !self.name.is_empty() {
            let galley = p.layout_no_wrap(self.name.to_owned(), egui::FontId::proportional(t.font_small), t.text);
            let clip = Rect::from_min_max(pos2(x, rect.top()), pos2(x + NAME_MAX, rect.bottom()));
            p.with_clip_rect(clip.intersect(p.clip_rect())).galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, t.text);
            x += self.name_width(&p);
        }
        let count = self.count_text(&p);
        p.galley(pos2(x, rect.center().y - count.size().y / 2.0), count, t.text);
        let well = Self::well(rect);
        let rule = well.left() - t.space_3;
        p.line_segment([pos2(rule, rect.top() + t.space_6), pos2(rule, rect.bottom() - t.space_6)], Stroke::new(1.0, edge));
        let ink = match (self.open, on_arrow) {
            (true, true) => {
                p.rect_filled(well, t.radius_sm, self.accent.color);
                t.on_accent
            }
            (false, true) => {
                p.rect_filled(well, t.radius_sm, t.control_border);
                t.form_ink
            }
            (true, false) => t.form_ink,
            (false, false) => t.form_dim,
        };
        crate::ui_kit::disclosure(&p, well.center(), self.open, ink);
        if self.error || self.warning {
            let side = t.space_10;
            let at = square.right_top() - vec2(side, 0.0);
            icons::paint_icon(&p, Rect::from_min_size(at, vec2(side, side)), if self.error { "error" } else { "warning" }, 1.0);
        }
    }
}
