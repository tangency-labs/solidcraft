//! Text elements for dense cards; see `docs/ui-kit.md` for tokens and usage.
use egui::Ui;
use solidcraft_theme::Tokens;

/// Shorten `text` with an ellipsis in the middle so both ends stay visible (#9081).
pub fn ellipsize_middle(ui: &Ui, text: &str, font: egui::FontId, max_width: f32) -> String {
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
pub fn paint_text(ui: &Ui, rect: egui::Rect, text: &str, font: egui::FontId, ink: egui::Color32, spaced: bool) {
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
    // Optical centring: egui centres the row box, but this font's capitals sit low in it (the
    // ascent exceeds the cap height, and a descent hangs under the baseline), so the row box
    // goes up by the measured offset and the ink reads centred (#22642).
    let rise = cap_rise(ui.painter(), &font);
    let shown = if middle { ellipsize_middle(ui, text, font, rect.width().max(0.0)) } else { text.to_owned() };
    let mut job = egui::text::LayoutJob::single_section(shown, format);
    job.wrap.max_width = rect.width().max(0.0);
    job.wrap.max_rows = 1;
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    // Snap to whole device pixels: a fractional origin blurs every glyph edge.
    let ppp = ui.ctx().pixels_per_point().max(0.5);
    let snap = |v: f32| (v * ppp).round() / ppp;
    let at = egui::pos2(snap(rect.left()), snap(rect.center().y - galley.size().y / 2.0 - rise));
    let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(rect));
    painter.galley(at, galley, ink);
}

/// How far above the geometric middle a single line in `font` must sit so its capitals read
/// centred, in points. Measured from one fixed probe glyph, so it follows the font, its size
/// and the pixels-per-point, and never moves with the text shown (#22642).
pub(crate) fn cap_rise(painter: &egui::Painter, font: &egui::FontId) -> f32 {
    egui::cap_ink_rise(painter, font)
}

/// Where to paint `galley` so its capitals read centred on `centre` (#22642): egui's geometric
/// centre of the row box, lifted by the font's measured offset.
pub(crate) fn centred_in(painter: &egui::Painter, galley: &egui::Galley, centre: egui::Pos2) -> egui::Pos2 {
    centre - galley.size() / 2.0 - egui::vec2(0.0, galley_cap_rise(painter, galley))
}

/// The [`cap_rise`] of the font `galley` is laid out in; zero unless `galley` is one row, whose
/// whole placement is about to be centred (#22642).
pub(crate) fn galley_cap_rise(painter: &egui::Painter, galley: &egui::Galley) -> f32 {
    match (galley.rows.len(), galley.job.sections.first()) {
        (1, Some(section)) => cap_rise(painter, &section.format.font_id),
        _ => 0.0,
    }
}
