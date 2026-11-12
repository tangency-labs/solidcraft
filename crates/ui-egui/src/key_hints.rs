//! The strip under the viewport: what to do now, and the keys whatever is in hand answers to.
//!
//! A tool that takes several clicks has more to say than a line of status text can hold, and the
//! status toast says its piece and goes after a few seconds — shorter than drawing a shape takes.
//! The strip stays up for as long as the tool does, names every key, and marks the one that is
//! set, so a key is something to be seen rather than remembered.
//!
//! Each tool says its own piece: a primitive being drawn through [`crate::solid_draw`], a sketch
//! tool through [`crate::tools`]. Nothing in hand draws nothing.

use crate::SolveApp;
use crate::theme::Tokens;

/// One key the thing in hand answers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hint {
    pub key: &'static str,
    pub label: &'static str,
    /// Whether this is what is set now (a choice among several), for the mark.
    pub on: bool,
}

impl Hint {
    pub const fn new(key: &'static str, label: &'static str) -> Hint {
        Hint { key, label, on: false }
    }

    pub const fn set(key: &'static str, label: &'static str, on: bool) -> Hint {
        Hint { key, label, on }
    }
}

/// What to do now and the keys for it, from whatever is in hand.
pub fn current(app: &SolveApp) -> Option<(String, Vec<Hint>)> {
    crate::solid_draw::key_hints(app).or_else(|| crate::tools::key_hints(app))
}

/// Draw the strip, when there is anything to say.
pub fn show(app: &SolveApp, ui: &egui::Ui, viewport: egui::Rect) {
    let Some((line, hints)) = current(app) else { return };
    let t = Tokens::get();
    let mut obstacles = crate::shell::covered_rects(app);
    if let Some(layout) = &app.viewport.layout {
        obstacles.extend([layout.navigation, layout.axis_view]);
    }
    let free = crate::viewport_layout::free_rect(crate::shell::content_rect(app, viewport), &obstacles).shrink(t.space_8);
    if !free.is_finite() || !free.is_positive() || free.width() <= 80.0 {
        return;
    }
    let painter = ui.painter().with_clip_rect(free);
    let font = crate::fonts::medium(t.font_micro);
    let mut chips: Vec<(String, bool)> = Vec::new();
    if !line.is_empty() {
        chips.push((line, false));
    }
    chips.extend(hints.iter().map(|h| (format!("{}  {}", h.key, h.label), h.on)));
    if chips.is_empty() {
        return;
    }
    let laid: Vec<_> = chips.iter().map(|(text, on)| (painter.layout_no_wrap(text.clone(), font.clone(), t.text), *on)).collect();
    let (gap, pad) = (t.space_6, t.space_6);
    let width: f32 = laid.iter().map(|(g, _)| g.size().x + pad * 2.0).sum::<f32>() + gap * (laid.len() as f32 - 1.0);
    let height = laid.first().map_or(0.0, |(g, _)| g.size().y) + pad;
    if width > free.width() || height > free.height() {
        return;
    }
    let left = free.center().x - width / 2.0;
    let top = free.bottom() - height;
    let mut x = left;
    for (galley, on) in laid {
        let r = egui::Rect::from_min_size(egui::pos2(x, top), egui::vec2(galley.size().x + pad * 2.0, height));
        painter.rect_filled(r, 3.0, if on { t.accent_soft } else { t.viewport_top });
        painter.galley(egui::pos2(r.left() + pad, r.top() + pad / 2.0), galley, t.text);
        x += r.width() + gap;
    }
    crate::scenario::publish_rect("key_hints", egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(width, height)));
}

/// A key pressed goes to whatever is in hand first: those letters belong to commands the rest of
/// the time, so only the tool holding them may take them. Says whether it was taken.
pub fn handle_key(app: &mut SolveApp, key: &str) -> bool {
    crate::solid_draw::handle_key(app, key) || crate::tools::handle_key(app, key)
}

#[cfg(test)]
#[path = "key_hints_tests.rs"]
mod tests;
