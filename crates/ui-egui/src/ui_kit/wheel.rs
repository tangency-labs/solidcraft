//! Wheel input for the kit's scrolled strips. Camera input never reads the wheel here: it goes
//! through the app's navigation handler.

use egui::Ui;

/// Take a predominantly horizontal wheel gesture for a hovered scroll viewport.
/// Leave vertical input for its enclosing page, and leave horizontal input
/// unconsumed when this viewport is already at the requested end.
pub(crate) fn take_horizontal_wheel(ui: &Ui, offset: f32, max_offset: f32) -> f32 {
    ui.input_mut(|input| {
        let delta = input.smooth_scroll_delta;
        if delta.x == 0.0 || delta.x.abs() <= delta.y.abs() {
            return 0.0;
        }
        if (offset - delta.x).clamp(0.0, max_offset) != offset {
            input.smooth_scroll_delta.x = 0.0;
        }
        delta.x
    })
}
