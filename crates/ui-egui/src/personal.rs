//! The user's own colours over any theme (Preferences › Appearance › Your colours): a base colour
//! that tints the title bar, ribbon, panels and dialogs, how strongly it tints, and the 3D view's
//! background, all with their text kept readable. The base colour can follow the time of day, and
//! the background the active ribbon tab (`prefs::apply` works out the colours each frame).
//!
//! [`set`] keeps them per thread, like the theme; `theme` applies them ([`Tokens::personalised`])
//! wherever the window's colours are made, after the theme file and its options, so they hold
//! for built-in palettes, theme files and tab colours alike. With none set, nothing changes.

use std::cell::Cell;

use egui::Color32;

use crate::theme::Tokens;

/// The user's own colours over a theme. `None`: the theme's.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Personal {
    /// The base colour, and how much of it the surfaces take (0 to 1).
    pub color: Option<Color32>,
    pub intensity: f32,
    /// The 3D view's background gradient, top and bottom, in place of the theme's.
    pub sky: Option<[Color32; 2]>,
}

impl Personal {
    /// Nothing over the theme's own.
    pub fn is_empty(&self) -> bool {
        self.color.is_none() && self.sky.is_none()
    }
}

thread_local! {
    /// The user's own colours on now ([`set`]).
    static CURRENT: Cell<Option<Personal>> = const { Cell::new(None) };
}

/// Use the user's own colours (`prefs::apply`, every frame); empty ones are none. Returns whether
/// that changed anything.
pub fn set(personal: Personal) -> bool {
    let personal = (!personal.is_empty()).then_some(personal);
    CURRENT.replace(personal) != personal
}

/// The user's own colours on now, if any.
pub fn current() -> Option<Personal> {
    CURRENT.get()
}

/// The surfaces the base colour tints, with the ink read on each and how much of the tint it
/// takes at full intensity: the title bar most, panels and dialogs gently.
macro_rules! tinted_surfaces {
    ($t:ident, $tint:expr, $k:expr, [$($fill:ident on $ink:ident * $share:expr),* $(,)?]) => {
        $( $t.$fill = readable_mix($t.$fill, $tint, $k * $share, $t.$ink); )*
    };
}

impl Tokens {
    /// These tokens with the user's own colours over them. Text stays readable: a surface takes
    /// less of the tint where its text would read worse than 4.5:1 (or than it did, if the
    /// theme's own was lower).
    pub fn personalised(mut self, p: Personal) -> Tokens {
        if let Some(tint) = p.color {
            let k = if p.intensity.is_finite() { p.intensity.clamp(0.0, 1.0) } else { 0.0 };
            tinted_surfaces!(self, tint, k, [
                app_bar on app_bar_text * 0.85,
                app_bar_tab_active on app_bar_text * 0.3,
                toolbar on text * 0.3,
                panel on text * 0.3,
                panel_header on text * 0.35,
                timeline on text * 0.3,
                dialog_bg on dialog_label * 0.3,
                dialog_body on dialog_label * 0.25,
                dialog_footer on dialog_label * 0.3,
                overlay on text * 0.3,
            ]);
        }
        if let Some([top, bottom]) = p.sky {
            (self.viewport_top, self.viewport_bottom) = (top, bottom);
            // The window's title bar follows the background's top, its text kept readable: a
            // colour too strong for it is lightened or darkened first, so it still shows.
            self.app_bar = readable_mix(self.app_bar, readable_shade(top, self.app_bar_text), 0.75, self.app_bar_text);
        }
        self
    }
}

/// `base` mixed towards `tint` by `amount`, keeping `base`'s alpha, and backed off until `ink`
/// reads on it at least as well as 4.5:1 (or as on `base`, if that was lower).
fn readable_mix(base: Color32, tint: Color32, amount: f32, ink: Color32) -> Color32 {
    let [r, g, b, a] = base.to_srgba_unmultiplied();
    let opaque = Color32::from_rgb(r, g, b);
    let floor = contrast(ink, opaque).min(4.5) - 1e-6;
    let mut amount = amount.clamp(0.0, 1.0);
    for _ in 0..16 {
        let mixed = opaque.lerp_to_gamma(tint, amount);
        if contrast(ink, mixed) >= floor {
            return Color32::from_rgba_unmultiplied(mixed.r(), mixed.g(), mixed.b(), a);
        }
        amount *= 0.75;
    }
    base
}

/// `color`, its hue kept, lightened (behind dark `ink`) or darkened (behind light `ink`) only as
/// far as `ink` needs to read on it at 4.5:1.
fn readable_shade(color: Color32, ink: Color32) -> Color32 {
    let opaque = Color32::from_rgb(color.r(), color.g(), color.b());
    if contrast(ink, opaque) >= 4.5 {
        return opaque;
    }
    // The middle of the luminance range in contrast terms: darker ink reads on lighter colours.
    let toward = if luminance(ink) < 0.18 { Color32::WHITE } else { Color32::BLACK };
    let (mut low, mut high) = (0.0_f32, 1.0_f32);
    for _ in 0..16 {
        let middle = (low + high) * 0.5;
        if contrast(ink, opaque.lerp_to_gamma(toward, middle)) >= 4.5 {
            high = middle;
        } else {
            low = middle;
        }
    }
    opaque.lerp_to_gamma(toward, high)
}

/// `color` with the lightness of `reference`, its hue kept: mixed towards white or black until
/// its relative luminance matches, so strong and pale colours come out equally strong.
pub fn with_lightness_of(color: Color32, reference: Color32) -> Color32 {
    let target = luminance(reference);
    let own = luminance(color);
    let toward = if own < target { Color32::WHITE } else { Color32::BLACK };
    let (mut low, mut high) = (0.0_f32, 1.0_f32);
    for _ in 0..16 {
        let middle = (low + high) * 0.5;
        if (luminance(color.lerp_to_gamma(toward, middle)) < target) == (own < target) {
            low = middle;
        } else {
            high = middle;
        }
    }
    color.lerp_to_gamma(toward, (low + high) * 0.5)
}

/// The sky's colour through a day, round the clock: the hour each colour is at its fullest, from
/// midnight's indigo through dawn's rose, the morning's and midday's blues, the late afternoon's
/// gold and dusk's coral to twilight's purple. Between two the colour shifts gradually.
const DAYLIGHT: [(f32, [u8; 3]); 8] = [
    (0.0, [38, 46, 104]),
    (5.0, [86, 70, 140]),
    (6.5, [240, 150, 120]),
    (9.0, [120, 190, 240]),
    (13.0, [70, 160, 230]),
    (17.5, [245, 180, 90]),
    (19.5, [220, 100, 120]),
    (21.0, [100, 70, 150]),
];

/// The sky's colour at `hours` past midnight ([`DAYLIGHT`]), for colours that follow the time of
/// day. Any hour is taken round the clock; one that isn't a number is midday.
pub fn daylight(hours: f32) -> Color32 {
    let hours = if hours.is_finite() { hours.rem_euclid(24.0) } else { 12.0 };
    let rgb = |c: [u8; 3]| Color32::from_rgb(c[0], c[1], c[2]);
    let next = DAYLIGHT.iter().position(|(at, _)| *at > hours).unwrap_or(DAYLIGHT.len());
    let (from, from_rgb) = DAYLIGHT.get((next + DAYLIGHT.len() - 1) % DAYLIGHT.len()).copied().unwrap_or((0.0, [0, 0, 0]));
    let (to, to_rgb) = DAYLIGHT.get(next).copied().unwrap_or((24.0, DAYLIGHT[0].1));
    let along = if to > from { (hours - from) / (to - from) } else { 0.0 };
    rgb(from_rgb).lerp_to_gamma(rgb(to_rgb), along.clamp(0.0, 1.0))
}

/// A colour on its way to `target` over `seconds`, channel by channel, from where it was last
/// asked for under `id` (the first time, `target` itself).
pub fn shifting(ctx: &egui::Context, id: &str, target: Color32, seconds: f32) -> Color32 {
    let channel = |i: u8, v: u8| ctx.animate_value_with_time(egui::Id::unique((id, i)), f32::from(v), seconds).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(channel(0, target.r()), channel(1, target.g()), channel(2, target.b()))
}

/// The local time of day, in hours past midnight (0 to 24), for colours that follow it.
pub fn local_hours() -> f32 {
    use chrono::Timelike;
    chrono::Local::now().num_seconds_from_midnight() as f32 / 3600.0
}

/// Relative luminance of an opaque sRGB colour (WCAG).
fn luminance(c: Color32) -> f64 {
    let linear = |v: u8| {
        let v = f64::from(v) / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * linear(c.r()) + 0.7152 * linear(c.g()) + 0.0722 * linear(c.b())
}

/// WCAG contrast of two colours' opaque sRGB.
fn contrast(a: Color32, b: Color32) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_time_of_days_colour_goes_round_the_clock_without_a_jump() {
        assert_eq!(daylight(0.0), daylight(24.0));
        assert_eq!(daylight(f32::NAN), daylight(12.0));
        let step = |a: Color32, b: Color32| a.r().abs_diff(b.r()).max(a.g().abs_diff(b.g())).max(a.b().abs_diff(b.b()));
        for minute in 0..24 * 60 {
            let (now, next) = (minute as f32 / 60.0, (minute + 1) as f32 / 60.0);
            assert!(step(daylight(now), daylight(next)) <= 6, "a jump at {now} h");
        }
    }

    #[test]
    fn a_base_colour_tints_the_surfaces_and_keeps_their_text_readable() {
        for base in [Tokens::light(), Tokens::dark()] {
            for tint in [Color32::from_rgb(255, 0, 0), Color32::from_rgb(0, 0, 255), Color32::from_rgb(255, 255, 0)] {
                let tinted = base.personalised(Personal { color: Some(tint), intensity: 1.0, sky: None });
                assert_ne!(tinted.app_bar, base.app_bar, "the title bar takes the colour");
                for (fill, ink, own) in [(tinted.app_bar, tinted.app_bar_text, base.app_bar), (tinted.panel, tinted.text, base.panel)] {
                    assert!(contrast(ink, fill) >= contrast(ink, own).min(4.5) - 1e-3, "{tint:?} keeps text readable");
                }
            }
            assert_eq!(base.personalised(Personal::default()), base, "no colours of the user's change nothing");
        }
    }

    #[test]
    fn the_title_bar_follows_a_strong_background_colour_and_its_text_stays_readable() {
        let base = Tokens::light();
        let red = Color32::from_rgb(220, 20, 20);
        let shaded = base.personalised(Personal { sky: Some([red, red]), ..Default::default() });
        assert_ne!(shaded.app_bar, base.app_bar);
        assert!(contrast(shaded.app_bar_text, shaded.app_bar) >= contrast(base.app_bar_text, base.app_bar).min(4.5) - 1e-3);
        assert_eq!((shaded.viewport_top, shaded.viewport_bottom), (red, red));
    }

    #[test]
    fn a_colour_given_another_colours_lightness_keeps_its_hue() {
        let pale = Color32::from_rgb(200, 220, 240);
        let even = with_lightness_of(Color32::from_rgb(200, 0, 0), pale);
        assert!((luminance(even) - luminance(pale)).abs() < 0.02);
        assert!(even.r() > even.g() && even.r() > even.b(), "still red");
    }
}
