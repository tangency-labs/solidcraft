//! A soft glow behind text on see-through panels (Preferences › Appearance › Text glow), so it
//! stays easy to read over whatever shows through: the model, a background pattern or picture.
//! Text lighter than the panel gets a dark glow, darker text a light one. Off by default.
//!
//! At the end of the frame ([`apply`]) each layer's see-through surfaces are found among its own
//! shapes: a filled rectangle at least [`MIN_SURFACE`] in size whose fill is partly transparent,
//! as a theme's Opacity option makes floating panels, windows and popups. Each text shape whose
//! middle lies on one gets faint copies of itself round it, behind it. Opaque surfaces and text
//! anywhere else are left exactly as they are.

use std::cell::Cell;

use egui::{Color32, Stroke};

use crate::theme::Tokens;

/// The smallest surface that counts as a see-through panel, so a translucent hover fill behind a
/// button's label doesn't.
const MIN_SURFACE: egui::Vec2 = egui::vec2(80.0, 40.0);
/// At most this many surfaces are looked for in one layer; text on more is drawn without a glow.
const MAX_SURFACES: usize = 1024;

/// The glow's rings of faint copies, round a point a pixel below the text (light falls from
/// above): (how far out, how many copies, how strong each is at half strength). Overlapping, they
/// build up near the letters and fade outwards like a blur; at full strength each is twice as
/// strong.
const RINGS: [(f32, usize, f32); 4] = [(0.0, 1, 0.2), (1.0, 8, 0.09), (2.0, 12, 0.05), (3.0, 16, 0.025)];

thread_local! {
    /// How strong the glow is ([`set_strength`]), 0 to 1.
    static STRENGTH: Cell<f32> = const { Cell::new(0.0) };
}

/// How strong the glow is (`prefs::apply`, every frame): 0 for none (the default), 0.5 soft, 1
/// strong. Not a number: none.
pub fn set_strength(strength: f32) {
    STRENGTH.set(if strength.is_finite() { strength.clamp(0.0, 1.0) } else { 0.0 });
}

/// After a frame is drawn: the text on its see-through surfaces gets its glow.
pub fn apply(ctx: &egui::Context) {
    let strength = STRENGTH.get();
    if strength <= 0.0 {
        return;
    }
    let surface = luminance(Tokens::get().panel);
    // Every area's layer (floating panels, windows, popups), and the background's.
    let mut layers: Vec<egui::LayerId> = ctx.memory(|m| m.layer_ids().collect());
    if !layers.contains(&egui::LayerId::background()) {
        layers.push(egui::LayerId::background());
    }
    ctx.graphics_mut(|graphics| {
        for layer in layers {
            let Some(list) = graphics.get_mut(layer) else { continue };
            let mut glass = Vec::new();
            for clipped in list.all_entries() {
                see_through(&clipped.shape, clipped.clip_rect, &mut glass);
            }
            if glass.is_empty() {
                continue;
            }
            let count = list.all_entries().len();
            for i in 0..count {
                list.mutate_shape(egui::layers::ShapeIdx(i), |clipped| glow(&mut clipped.shape, &glass, surface, strength));
            }
        }
    });
}

/// The see-through surfaces among `shape`, as far as `clip` shows them, onto `out`.
fn see_through(shape: &egui::Shape, clip: egui::Rect, out: &mut Vec<egui::Rect>) {
    match shape {
        egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| see_through(s, clip, out)),
        egui::Shape::Rect(rect) if rect.fill.a() > 0 && rect.fill.a() < u8::MAX && rect.blur_width <= 0.0 => {
            let shown = rect.rect.intersect(clip);
            if out.len() < MAX_SURFACES && shown.is_finite() && shown.width() >= MIN_SURFACE.x && shown.height() >= MIN_SURFACE.y {
                out.push(shown);
            }
        }
        _ => {}
    }
}

/// Where the glow's copies go and how strong each is, at half strength ([`RINGS`]).
fn copies() -> impl Iterator<Item = (egui::Vec2, f32)> {
    RINGS.into_iter().flat_map(|(radius, count, alpha)| {
        (0..count).map(move |i| {
            let angle = std::f32::consts::TAU * i as f32 / count as f32;
            (egui::vec2(radius * angle.cos(), 1.0 + radius * angle.sin()), alpha)
        })
    })
}

/// `shape`'s text that lies on `glass`, with its glow behind it, dark or light against `surface`
/// (the panel's luminance) as the text is, as strong as `strength` (0 to 1).
fn glow(shape: &mut egui::Shape, glass: &[egui::Rect], surface: f64, strength: f32) {
    match shape {
        egui::Shape::Vec(shapes) => shapes.iter_mut().for_each(|s| glow(s, glass, surface, strength)),
        egui::Shape::Text(text) => {
            let center = text.visual_bounding_rect().center();
            if !glass.iter().any(|rect| rect.contains(center)) {
                return;
            }
            let ink = text
                .override_text_color
                .or_else(|| text.galley.job.sections.first().map(|s| s.format.color))
                .map(|c| if c == Color32::PLACEHOLDER { text.fallback_color } else { c })
                .unwrap_or(text.fallback_color);
            if ink.a() == 0 {
                return;
            }
            // Twice the table's strength at full strength; fainter text, a fainter glow.
            let scale = 2.0 * strength * f32::from(ink.a()) / 255.0;
            let behind = if luminance(ink) > surface { Color32::BLACK } else { Color32::WHITE };
            let mut shapes: Vec<egui::Shape> = copies()
                .map(|(offset, alpha)| {
                    let mut copy = text.clone();
                    copy.pos += offset;
                    copy.override_text_color = Some(behind.gamma_multiply((alpha * scale).min(1.0)));
                    copy.underline = Stroke::NONE;
                    egui::Shape::Text(copy)
                })
                .collect();
            shapes.push(std::mem::replace(shape, egui::Shape::Noop));
            *shape = egui::Shape::Vec(shapes);
        }
        _ => {}
    }
}

/// The relative luminance of a colour's opaque sRGB.
fn luminance(c: Color32) -> f64 {
    let linear = |v: u8| {
        let v = f64::from(v) / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * linear(c.r()) + 0.7152 * linear(c.g()) + 0.0722 * linear(c.b())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(ctx: &egui::Context, at: egui::Pos2, color: Color32) -> egui::Shape {
        let galley = ctx.fonts_mut(|f| f.layout_no_wrap("Extrude".into(), egui::FontId::proportional(13.0), color));
        egui::Shape::galley(at, galley, color)
    }

    #[test]
    fn only_large_partly_transparent_fills_count_as_see_through() {
        let clip = egui::Rect::EVERYTHING;
        let panel = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(300.0, 200.0));
        let mut found = Vec::new();
        for (rect, fill) in [
            (panel, Color32::from_rgba_unmultiplied(200, 220, 240, 170)),
            (panel, Color32::from_rgb(200, 220, 240)),
            (egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(40.0, 20.0)), Color32::from_white_alpha(40)),
        ] {
            see_through(&egui::Shape::rect_filled(rect, 0.0, fill), clip, &mut found);
        }
        assert_eq!(found, vec![panel], "the see-through panel, not the opaque one or a small hover fill");
    }

    #[test]
    fn text_on_glass_glows_dark_or_light_as_its_ink_and_text_elsewhere_does_not() {
        let ctx = crate::fonts::test_context();
        let glass = [egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(200.0, 100.0))];
        let surface = luminance(Color32::from_rgb(200, 220, 240));
        for (ink, behind) in [(Color32::from_rgb(10, 40, 70), Color32::WHITE), (Color32::WHITE, Color32::BLACK)] {
            let mut on = text(&ctx, egui::pos2(20.0, 20.0), ink);
            glow(&mut on, &glass, surface, 0.5);
            let egui::Shape::Vec(shapes) = &on else { panic!("text on glass gets a glow") };
            assert_eq!(shapes.len(), copies().count() + 1, "the copies, then the text itself");
            let egui::Shape::Text(first) = &shapes[0] else { panic!() };
            let shade = first.override_text_color.unwrap();
            assert_eq!((shade.r(), shade.g(), shade.b()), (behind.r(), behind.g(), behind.b()));
        }
        let mut off = text(&ctx, egui::pos2(400.0, 400.0), Color32::BLACK);
        glow(&mut off, &glass, surface, 0.5);
        assert!(matches!(off, egui::Shape::Text(_)), "text off the glass is left alone");
    }
}
