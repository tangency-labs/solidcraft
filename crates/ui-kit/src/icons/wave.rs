//! The RGB wave (an effect of kind `hue-wave`, `effects@0.1`): ribbon icons and button blocks take a
//! hue from their own screen x and the time, so one wave runs across the window however the ribbon
//! is laid out. The host sets [`WaveSet`] each frame from the chosen plugin effect and its settings.

use egui::{Color32, Painter, Rect};

use super::{Finish, Look, Mirror, paint_icon, paint_tinted, resolve};

const WAVE_FLAGS: &str = "sc_rgb_wave";
const WAVE_SATURATION: f32 = 0.65;

/// The wave's shape: how long one colour cycle is on screen and how fast it travels, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wave {
    pub length: f32,
    pub speed: f32,
}

impl Default for Wave {
    fn default() -> Wave {
        Wave { length: 2400.0, speed: 150.0 }
    }
}

impl Wave {
    /// The hue's phase at screen x 0 and time (s), 0 to 1.
    pub fn phase(self, time: f64) -> f32 {
        (time * f64::from(self.speed) / f64::from(self.length.max(1.0))).rem_euclid(1.0) as f32
    }

    /// The wave's colour at screen x (points) and time (s), at `opacity`.
    pub fn tint(self, x: f32, time: f64, opacity: f32) -> Color32 {
        let hue = (x / self.length.max(1.0) - self.phase(time)).rem_euclid(1.0);
        let [r, g, b] = egui::ecolor::Hsva::new(hue, WAVE_SATURATION, 1.0, 1.0).to_srgb();
        Color32::from_rgb(r, g, b).gamma_multiply(opacity.clamp(0.0, 1.0))
    }

    /// The colour of one cycle at palette index `i` of 256, for a texture that repeats the cycle.
    pub fn cycle_colour(i: usize) -> Color32 {
        let [r, g, b] = egui::ecolor::Hsva::new(i as f32 / 256.0, WAVE_SATURATION, 1.0, 1.0).to_srgb();
        Color32::from_rgb(r, g, b)
    }
}

/// What the host draws this frame: `on` while a plugin's effect of this kind is chosen and turned
/// on; the three targets (each a preference); the wave's shape.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WaveSet {
    pub on: bool,
    pub icons: bool,
    pub blocks: bool,
    pub panels: bool,
    pub wave: Wave,
}

/// Set by the host each frame. Asks for about 30 fps while the wave shows.
pub fn set_wave(ctx: &egui::Context, set: WaveSet) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique(WAVE_FLAGS), set));
    if set.on && (set.icons || set.blocks || set.panels) {
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
    }
}

/// The wave set by [`set_wave`] this frame, all off if none was.
pub fn wave_flags(ctx: &egui::Context) -> WaveSet {
    ctx.data(|d| d.get_temp(egui::Id::unique(WAVE_FLAGS))).unwrap_or_default()
}

/// How much of a pixel is accent rather than white or grey, by its saturation.
fn wave_share(pixel: Color32) -> f32 {
    let [r, g, b, _] = pixel.to_srgba_unmultiplied();
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    if max == 0 {
        return 0.0;
    }
    let saturation = f32::from(max - min) / f32::from(max);
    ((saturation - 0.12) / 0.2).clamp(0.0, 1.0)
}

/// A pixel's accent (as its brightness, for a tint to multiply) or the rest of it. Together they
/// make up the pixel; brightness and not luminance, as a blue's luminance is dark whatever its shade.
pub(super) fn wave_part(pixel: Color32, accent: bool) -> Color32 {
    let share = wave_share(pixel);
    if !accent {
        return pixel.gamma_multiply(1.0 - share);
    }
    let [r, g, b, a] = pixel.to_srgba_unmultiplied();
    let v = r.max(g).max(b);
    Color32::from_rgba_unmultiplied(v, v, v, a).gamma_multiply(share)
}

/// A ribbon command icon: [`paint_icon`], or with the wave on, the artwork with the wave in place
/// of its accent.
pub fn paint_ribbon_icon(p: &Painter, r: Rect, name: &str, opacity: f32) {
    let ctx = p.ctx();
    let set = wave_flags(ctx);
    if !(set.on && set.icons) {
        return paint_icon(p, r, name, opacity);
    }
    let resolved = resolve(name);
    let look = Look::themed(ctx.global_style().visuals.dark_mode, resolved.accent, solidcraft_theme::vision());
    let tint = set.wave.tint(r.center().x, ctx.input(|i| i.time), opacity);
    paint_tinted(p, r, resolve(name), look, opacity, Finish::WaveNeutral, Mirror::None, None);
    paint_tinted(p, r, resolved, look, opacity, Finish::WaveAccent, Mirror::None, Some(tint));
}

/// A ribbon button's block in the wave's colour at low alpha (with the `ui_effect_buttons` target).
pub fn paint_ribbon_block(p: &Painter, r: Rect, radius: u8, enabled: bool) {
    let ctx = p.ctx();
    let set = wave_flags(ctx);
    if enabled && set.on && set.blocks {
        p.rect_filled(r, radius, set.wave.tint(r.center().x, ctx.input(|i| i.time), 0.18));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An accent keeps its shading as brightness, two shades stay apart, and white stays neutral.
    #[test]
    fn the_wave_replaces_the_accent_and_keeps_the_detail() {
        let (light, dark) = (Color32::from_rgb(90, 170, 255), Color32::from_rgb(20, 70, 160));
        let v = |c: Color32| wave_part(c, true).to_srgba_unmultiplied()[0];
        assert!(v(light) > 200 && v(light) - v(dark) > 60, "shades apart: {} {}", v(light), v(dark));
        assert_eq!(wave_part(light, false).a(), 0, "an accent pixel has no neutral part");
        let white = Color32::from_rgb(235, 235, 235);
        assert_eq!(wave_part(white, true).a(), 0, "white takes no tint");
        assert_eq!(wave_part(white, false), white);
    }

    /// One colour cycle repeats: the wave's length is the distance to the next same colour.
    #[test]
    fn the_wave_moves_at_its_speed_over_its_length() {
        let wave = Wave { length: 600.0, speed: 150.0 };
        assert_eq!(wave.tint(10.0, 2.0, 1.0), wave.tint(310.0, 0.0, 1.0));
        assert_eq!(wave.tint(10.0, 0.0, 1.0), wave.tint(610.0, 0.0, 1.0));
    }
}
