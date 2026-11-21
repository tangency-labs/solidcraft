use egui::Color32;

/// Visual parameters of a glass surface. All lengths are in logical points.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct GlassStyle {
    /// Corner radius. Use [`f32::INFINITY`] for a capsule.
    pub corner_radius: f32,
    /// Continuous-corner smoothing, 0 = circular arc, 1 = fully smoothed.
    /// 0.6 matches the look of iOS continuous corners. Same model as Figma's
    /// corner smoothing: the transition spans `(1 + smoothing) * radius`.
    pub corner_smoothing: f32,
    /// Backdrop blur radius.
    pub blur: f32,
    /// Maximum displacement of the backdrop at the edge (lens effect).
    pub refraction: f32,
    /// Width of the lens zone measured inwards from the edge.
    pub edge_width: f32,
    /// How far past its own edge the lens reaches for what it shows, 0..=1. At 1 the rim shows
    /// the backdrop up to `refraction` beyond the edge; at 0 it only magnifies what is under it.
    pub reach: f32,
    /// Chromatic aberration inside the lens zone, 0..=1.
    pub chromatic: f32,
    /// Tint mixed over the backdrop; alpha is the mix amount.
    pub tint: Color32,
    /// Backdrop brightness multiplier.
    pub brightness: f32,
    /// Backdrop saturation multiplier.
    pub saturation: f32,
    /// Specular rim/sheen strength, 0..=1.
    pub specular: f32,
    /// Inner border strength, 0..=1.
    pub border: f32,
    /// Drop shadow strength, 0..=1.
    pub shadow: f32,
    /// Drop shadow blur radius.
    pub shadow_radius: f32,
    /// How far the shadow is pushed down.
    pub shadow_offset: f32,
    /// How far the shadow shape grows beyond the glass before it is blurred.
    pub shadow_spread: f32,
}

impl Default for GlassStyle {
    fn default() -> Self {
        Self::regular()
    }
}

impl GlassStyle {
    /// Apple "regular" glass: legible, slightly frosted.
    pub const fn regular() -> Self {
        Self {
            corner_radius: 22.0,
            corner_smoothing: 0.6,
            blur: 10.0,
            refraction: 14.0,
            edge_width: 8.0,
            reach: 1.0,
            chromatic: 0.3,
            tint: Color32::from_rgba_unmultiplied_const(255, 255, 255, 52),
            brightness: 1.06,
            saturation: 1.2,
            specular: 0.5,
            border: 0.3,
            // iOS on a white page: no drop shadow, just a faint wide halo (Photos toolbar).
            shadow: 0.045,
            shadow_radius: 18.0,
            shadow_offset: 3.0,
            shadow_spread: 0.0,
        }
    }

    /// Apple "clear" glass: almost no frost, strong lensing.
    pub const fn clear() -> Self {
        Self {
            blur: 2.0,
            refraction: 22.0,
            edge_width: 34.0,
            chromatic: 0.35,
            tint: Color32::from_rgba_unmultiplied_const(255, 255, 255, 18),
            ..Self::regular()
        }
    }

    /// Flat frosted panel for large surfaces (sidebars, sheets): a plain sheet of
    /// frosted glass. Heavy blur, a whisper of lensing at the edge, no highlights,
    /// no halo; only a faint edge separates it from a white page.
    pub const fn panel() -> Self {
        Self {
            corner_radius: 28.0,
            blur: 36.0,
            refraction: 4.0,
            edge_width: 8.0,
            chromatic: 0.0,
            tint: Color32::from_rgba_unmultiplied_const(255, 255, 255, 80),
            brightness: 0.985, // a hair darker than a white page so the panel still reads on it
            saturation: 1.05,
            specular: 0.08,
            border: 0.15,
            // A tight, faint contact shadow: the iPad sidebar over white content shows a
            // few px of soft darkening at its edge, nothing wide.
            shadow: 0.06,
            shadow_radius: 10.0,
            shadow_offset: 2.0,
            ..Self::regular()
        }
    }

    /// [`Self::panel`] for dark themes.
    pub const fn panel_dark() -> Self {
        Self {
            // iOS dark panels are dimmed content with a faint white lift, not black paint.
            tint: Color32::from_rgba_unmultiplied_const(255, 255, 255, 24),
            brightness: 0.72,
            saturation: 0.9,
            specular: 0.1,
            border: 0.15,
            shadow: 0.0,
            ..Self::panel()
        }
    }

    /// Dark tinted glass for light content.
    pub const fn dark() -> Self {
        Self { tint: Color32::from_rgba_unmultiplied_const(0, 0, 0, 90), brightness: 0.9, specular: 0.5, border: 0.5, ..Self::regular() }
    }

    /// Sets the corner radius in logical points; infinity requests a capsule.
    pub const fn with_corner_radius(mut self, radius: f32) -> Self {
        self.corner_radius = radius;
        self
    }

    /// Rounds the ends to half the surface's shorter dimension.
    pub const fn capsule(self) -> Self {
        self.with_corner_radius(f32::INFINITY)
    }

    /// Sets the overlay colour; alpha controls how much tint is mixed in.
    pub const fn with_tint(mut self, tint: Color32) -> Self {
        self.tint = tint;
        self
    }

    /// True when the tint makes the surface dark enough to need light text.
    pub fn is_dark(&self) -> bool {
        let [r, g, b, a] = self.tint.to_array();
        a >= 60 && (0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32) < 0.5 * a as f32
    }

    /// Variant used while the pointer is over an interactive glass widget.
    pub fn hovered(mut self) -> Self {
        self.brightness *= 1.08;
        self.specular = (self.specular + 0.15).min(1.0);
        self
    }

    /// Variant used while an interactive glass widget is pressed.
    pub fn pressed(mut self) -> Self {
        self.brightness *= 0.9;
        self.tint = self.tint.gamma_multiply(1.3);
        self
    }
}

#[cfg(all(test, feature = "serde"))]
mod tests {
    use super::GlassStyle;

    #[test]
    fn json_round_trip_and_missing_fields_default() {
        let style = GlassStyle::clear().with_corner_radius(7.5);
        let json = serde_json::to_string(&style).unwrap();
        assert_eq!(serde_json::from_str::<GlassStyle>(&json).unwrap(), style);
        let partial: GlassStyle = serde_json::from_str(r#"{"blur": 3.0}"#).unwrap();
        assert_eq!(partial, GlassStyle { blur: 3.0, ..GlassStyle::regular() });
    }
}
