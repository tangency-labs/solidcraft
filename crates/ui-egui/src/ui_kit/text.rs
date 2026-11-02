//! Text elements for dense cards; see `docs/ui-kit.md` for tokens and usage.
use crate::theme::Tokens;
use egui::Ui;

/// Shorten `text` with an ellipsis in the middle so both ends stay visible (#9081).
pub(crate) fn ellipsize_middle(ui: &Ui, text: &str, font: egui::FontId, max_width: f32) -> String {
    if !(max_width.is_finite() && max_width > 0.0) {
        return String::new();
    }
    let width = |s: &str| ui.fonts_mut(|fonts| fonts.layout_no_wrap(s.to_owned(), font.clone(), ui.visuals().text_color()).size().x);
    if width(text) <= max_width + 0.5 {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.len() < 4 {
        return "…".into();
    }
    let mut best = "…".to_string();
    let mut lo = 1usize;
    let mut hi = chars.len() / 2;
    while lo <= hi {
        let keep = (lo + hi) / 2;
        let left: String = chars.iter().take(keep).collect();
        let right: String = chars.iter().skip(chars.len() - keep).collect();
        let candidate = format!("{left}…{right}");
        if width(&candidate) <= max_width + 0.5 {
            best = candidate;
            lo = keep + 1;
        } else if keep == 0 {
            break;
        } else {
            hi = keep - 1;
        }
    }
    best
}

/// Paint one pixel-aligned text run with the requested interface weight.
/// Clipped, device-pixel-snapped weighted text. Tokens: `font_section_tracking; caller supplies font/ink tokens`.
pub(super) fn paint_text(ui: &Ui, rect: egui::Rect, text: &str, font: egui::FontId, ink: egui::Color32, spaced: bool) {
    paint_text_aligned(ui, rect, text, font, ink, spaced, false);
}

/// Like [`paint_text`], but with `middle` an ellipsis sits in the middle so a path's file name
/// stays visible (#9081).
pub(super) fn paint_text_aligned(ui: &Ui, rect: egui::Rect, text: &str, font: egui::FontId, ink: egui::Color32, spaced: bool, middle: bool) {
    let size = font.size;
    let mut format = egui::TextFormat::simple(font.clone(), ink);
    if spaced {
        format.extra_letter_spacing = size * Tokens::get().font_section_tracking;
    }
    let shown = if middle { ellipsize_middle(ui, text, font, rect.width().max(0.0)) } else { text.to_owned() };
    let mut job = egui::text::LayoutJob::single_section(shown, format);
    job.wrap.max_width = rect.width().max(0.0);
    job.wrap.max_rows = 1;
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    // Snap to whole device pixels: a fractional origin blurs every glyph edge.
    let ppp = ui.ctx().pixels_per_point().max(0.5);
    let snap = |v: f32| (v * ppp).round() / ppp;
    let at = egui::pos2(snap(rect.left()), snap(rect.center().y - galley.size().y / 2.0));
    let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(rect));
    painter.galley(at, galley, ink);
}
