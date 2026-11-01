//! Reordering chrome shared by the ribbon and editable navigation bar.
use crate::theme::Tokens;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ReorderPayload<T> {
    pub id: T,
    pub from_menu: bool,
}

pub(crate) fn insertion_marker(painter: &egui::Painter, x: f32, top: f32, bottom: f32) -> egui::Rect {
    insertion_marker_with_ink(painter, x, top, bottom, Tokens::get().accent)
}

pub(crate) fn insertion_marker_with_ink(painter: &egui::Painter, x: f32, top: f32, bottom: f32, ink: egui::Color32) -> egui::Rect {
    let rect = egui::Rect::from_min_max(egui::pos2(x - 1.0, top), egui::pos2(x + 1.0, bottom));
    painter.rect_filled(rect, 1, ink);
    rect
}

/// A gap before the first centre to the right of the pointer; hostile coordinates append.
pub(crate) fn drop_index(x: f32, centres: impl IntoIterator<Item = f32>) -> usize {
    let mut count = 0;
    for centre in centres {
        if x.is_finite() && centre.is_finite() && x < centre {
            return count;
        }
        count += 1;
    }
    count
}
