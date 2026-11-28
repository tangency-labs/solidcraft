//! Shared nested card painting; the owning section supplies layout and input targets.
use egui::{CornerRadius, Frame, Rect, Shape, Stroke, StrokeKind};
use solidcraft_theme::Tokens;

pub struct CardSurface(pub Tokens);

impl CardSurface {
    pub fn frame(&self) -> Frame {
        Frame::NONE.corner_radius(self.0.radius_lg).fill(self.0.card_fill).stroke(self.0.container_border(self.0.dialog_border))
    }

    pub fn band_fill(&self) -> egui::Color32 {
        self.0.card_fill.lerp_to_gamma(self.0.form_primary, 0.06)
    }

    pub fn band(&self, rect: Rect, header: bool) -> Shape {
        let radius = if header { CornerRadius { nw: self.0.radius_lg, ne: self.0.radius_lg, sw: 0, se: 0 } } else { CornerRadius::ZERO };
        Shape::rect_filled(rect, radius, self.band_fill())
    }

    pub fn rule_stroke(&self) -> Stroke {
        self.0.tertiary_border(self.0.dialog_rule)
    }

    /// Split a hairline around a selected cell block, preserving its uninterrupted fill.
    pub fn rule(&self, row: Rect, selection: Rect) -> Shape {
        let y = row.top();
        let line = |left, right| Shape::line_segment([egui::pos2(left, y), egui::pos2(right, y)], self.rule_stroke());
        if !selection.is_positive() || y <= selection.top() || y >= selection.bottom() {
            return line(row.left(), row.right());
        }
        let sides = [(row.left(), selection.left().min(row.right())), (selection.right().max(row.left()), row.right())];
        Shape::Vec(sides.into_iter().filter(|(left, right)| left < right).map(|(left, right)| line(left, right)).collect())
    }

    pub fn selection_fill(&self) -> egui::Color32 {
        self.0.selected_bg.blend(self.0.form_primary.gamma_multiply(0.15))
    }

    pub fn selection(&self, rect: Rect, hovered: bool) -> Shape {
        let fill = self.selection_fill();
        Shape::rect_filled(rect, self.0.radius_md, if hovered { fill.lerp_to_gamma(self.0.form_primary, 0.08) } else { fill })
    }

    pub fn cell_selection(&self, rect: Rect) -> Shape {
        Shape::Vec(vec![
            Shape::rect_filled(rect, self.0.control_radius, self.0.selected_bg),
            Shape::rect_filled(rect, self.0.control_radius, self.0.form_primary.gamma_multiply(0.15)),
        ])
    }

    pub fn focus(&self, rect: Rect, radius: u8) -> Shape {
        Shape::rect_stroke(rect, radius, Stroke::new(1.5, self.0.form_primary), StrokeKind::Inside)
    }
}
