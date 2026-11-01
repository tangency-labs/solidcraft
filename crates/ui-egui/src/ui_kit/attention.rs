//! One "needs attention" mark: something waits for the user (the agent chat's questions today;
//! other things that wait can use it too). A small count, or a plain dot, on the top-right
//! corner of what it belongs to.
use crate::theme::Tokens;
use egui::{Align2, Painter, Pos2, Rect, vec2};

/// Where the mark sits on `target` (a button's or icon's rect): `space_3` in from its top-right
/// corner. Tokens: space_3.
pub(crate) fn attention_at(target: Rect) -> Pos2 {
    let t = Tokens::get();
    target.right_top() + vec2(-t.space_3, t.space_3)
}

/// The mark on `target`: a count when there is one (at most 9 shown), else a plain dot.
/// Tokens: warning (the mark), panel (the count's ink), font_badge.
pub(crate) fn attention_mark(painter: &Painter, target: Rect, count: Option<usize>) {
    let t = Tokens::get();
    let at = attention_at(target);
    match count {
        Some(n) => {
            painter.circle_filled(at, 6.5, t.warning);
            painter.text(at, Align2::CENTER_CENTER, n.min(9).to_string(), egui::FontId::proportional(t.font_badge), t.panel);
        }
        None => {
            painter.circle_filled(at, 4.0, t.warning);
            painter.circle_stroke(at, 4.0, egui::Stroke::new(1.0, t.panel));
        }
    }
}
