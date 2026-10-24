//! Bounded original presentation adjustments, separate from light transport.
//! Raw linear masters never pass through these display-oriented operations.
use super::{ErrorCode, JobError, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Finishing {
    #[serde(default, skip_serializing_if = "super::looks::LookStack::is_empty")]
    pub looks: super::looks::LookStack,
    #[serde(default, skip_serializing_if = "super::photographic::Photographic::is_identity")]
    pub photographic: super::photographic::Photographic,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bloom: Option<super::bloom::Bloom>,
    /// Explicit creative linear RGB gains, not a measured illuminant temperature.
    pub rgb_gain: [f64; 3],
    /// Display-linear contrast about 18% gray, after the selected tone response.
    pub contrast: f64,
    pub saturation: f64,
    /// Elliptical screen-space attenuation; not a physical lens model.
    pub vignette: f64,
}
impl Default for Finishing {
    fn default() -> Self {
        Self {
            looks: Default::default(),
            photographic: Default::default(),
            bloom: None,
            rgb_gain: [1.; 3],
            contrast: 1.,
            saturation: 1.,
            vignette: 0.,
        }
    }
}
impl Finishing {
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }
    pub fn validate(self) -> Result<()> {
        self.looks.validate()?;
        self.photographic.validate()?;
        if let Some(bloom) = self.bloom {
            bloom.validate()?;
        }
        if self.rgb_gain.iter().any(|v| !v.is_finite() || !(0.1..=8.0).contains(v))
            || !self.contrast.is_finite()
            || !(0.25..=4.).contains(&self.contrast)
            || !self.saturation.is_finite()
            || !(0.0..=2.).contains(&self.saturation)
            || !self.vignette.is_finite()
            || !(0.0..=1.).contains(&self.vignette)
        {
            return Err(JobError::new(
                ErrorCode::InvalidRequest,
                "finishing requires RGB gain 0.1..8, contrast 0.25..4, saturation 0..2 and vignette 0..1",
            ));
        }
        Ok(())
    }
    /// Called only with a validated plan and finite tone-mapped RGB. Returns
    /// linear-display values; the output encoder owns final clipping and sRGB.
    pub fn display(self, mut rgb: [f64; 3], position: [f64; 2]) -> [f64; 3] {
        rgb = self.looks.apply(rgb);
        if self.contrast != 1. {
            rgb = rgb.map(|c| (c - 0.18) * self.contrast + 0.18);
        }
        if self.saturation != 1. {
            let y = rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
            rgb = rgb.map(|c| y + (c - y) * self.saturation);
        }
        if self.vignette != 0. {
            let x = 2. * position[0] - 1.;
            let y = 2. * position[1] - 1.;
            let radius = ((x * x + y * y) * 0.5).clamp(0., 1.);
            let attenuation = 1. - self.vignette * radius * radius;
            rgb = rgb.map(|c| c * attenuation);
        }
        rgb
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackdropMode {
    #[default]
    Vertical,
    Horizontal,
    Radial,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Backdrop {
    pub mode: BackdropMode,
    pub first_srgb: [u8; 3],
    pub second_srgb: [u8; 3],
    /// Normalized raster location for the radial origin. No scene units involved.
    pub center: [f64; 2],
    pub radius: f64,
}
impl Default for Backdrop {
    fn default() -> Self {
        Self { mode: BackdropMode::Vertical, first_srgb: [240, 243, 248], second_srgb: [80, 91, 109], center: [0.5, 0.5], radius: 0.75 }
    }
}
impl Backdrop {
    pub fn validate(self) -> Result<()> {
        if self.center.iter().any(|c| !c.is_finite() || !(0.0..=1.).contains(c)) || !self.radius.is_finite() || !(0.05..=4.).contains(&self.radius) {
            return Err(JobError::new(ErrorCode::InvalidRequest, "backdrop center must be in 0..1 and radius in 0.05..4"));
        }
        Ok(())
    }
    /// Gradient interpolation is in linear display light, never encoded sRGB.
    /// This background is not an emitter, environment map, reflection or terrain.
    pub fn color(self, position: [f64; 2]) -> [f64; 3] {
        let t = match self.mode {
            BackdropMode::Vertical => position[1],
            BackdropMode::Horizontal => position[0],
            BackdropMode::Radial => ((position[0] - self.center[0]).powi(2) + (position[1] - self.center[1]).powi(2)).sqrt() / self.radius,
        }
        .clamp(0., 1.);
        let a = self.first_srgb.map(crate::surface::srgb8_to_linear);
        let b = self.second_srgb.map(crate::surface::srgb8_to_linear);
        std::array::from_fn(|i| a[i] * (1. - t) + b[i] * t)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_is_exact_and_unknown_nonfinite_or_extreme_intent_is_refused() {
        let input = [0., 0.18, 1.23456789];
        assert_eq!(Finishing::default().display(input, [0., 1.]), input);
        assert!(serde_json::from_str::<Finishing>(r#"{"blur":1}"#).is_err());
        for v in [f64::NAN, f64::INFINITY, -1., 9.] {
            assert!(Finishing { rgb_gain: [v, 1., 1.], ..Default::default() }.validate().is_err());
        }
        assert!(Finishing { saturation: 2.01, ..Default::default() }.validate().is_err());
        assert!(Finishing { contrast: 0., ..Default::default() }.validate().is_err());
        assert!(Finishing { vignette: 1.1, ..Default::default() }.validate().is_err());
    }
    #[test]
    fn neutral_preserves_gray_and_chroma_zero_is_luminance_not_channel_mean() {
        let neutral = Finishing { saturation: 0., ..Default::default() }.display([1., 0., 0.], [0.5, 0.5]);
        assert_eq!(neutral, [0.2126; 3]);
        assert_eq!(Finishing { contrast: 3., ..Default::default() }.display([0.18; 3], [0.5, 0.5]), [0.18; 3]);
        let v = Finishing { vignette: 0.5, ..Default::default() };
        assert_eq!(v.display([1.; 3], [0.5, 0.5]), [1.; 3]);
        assert_eq!(v.display([1.; 3], [0., 0.]), [0.5; 3]);
    }
    #[test]
    fn gradient_uses_linear_light_and_independent_origin_not_subject_geometry() {
        let g = Backdrop { first_srgb: [0; 3], second_srgb: [255; 3], ..Default::default() };
        assert_eq!(g.color([0., 0.]), [0.; 3]);
        assert_eq!(g.color([1., 1.]), [1.; 3]);
        assert_eq!(g.color([0.1, 0.5]), [0.5; 3]);
        assert_ne!(g.color([0.1, 0.5]), [crate::surface::srgb8_to_linear(128); 3]);
        let radial = Backdrop { mode: BackdropMode::Radial, first_srgb: [255; 3], second_srgb: [0; 3], center: [0.2, 0.4], radius: 0.2 };
        assert_eq!(radial.color([0.2, 0.4]), [1.; 3]);
        assert!(radial.color([0.4, 0.4]).iter().all(|v| v.abs() < 1e-12));
        assert!(Backdrop { radius: 0., ..g }.validate().is_err());
        assert!(Backdrop { center: [f64::NAN, 0.], ..g }.validate().is_err());
    }
}
