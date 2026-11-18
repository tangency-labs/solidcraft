/// Resolution of the off-screen live backdrop. Visible page content stays full resolution.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum LiveBackdropQuality {
    /// Native resolution, preserving the original rendering quality.
    #[default]
    Full,
    /// Three-quarter width and height (about 56% of the backdrop pixels).
    Balanced,
    /// Half width and height (about 25% of the backdrop pixels); small text is softer.
    Performance,
}

impl LiveBackdropQuality {
    pub(crate) fn scale(self) -> f32 {
        match self {
            Self::Full => 1.0,
            Self::Balanced => 0.75,
            Self::Performance => 0.5,
        }
    }

    pub(crate) fn screen(self, screen: &egui_wgpu::ScreenDescriptor) -> egui_wgpu::ScreenDescriptor {
        egui_wgpu::ScreenDescriptor {
            size_in_pixels: screen.size_in_pixels.map(|size| (size as f32 * self.scale()).ceil() as u32),
            pixels_per_point: screen.pixels_per_point * self.scale(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_scales_physical_resolution_and_pixel_density() {
        let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: [2560, 1440], pixels_per_point: 2.0 };
        for (quality, size, density) in [
            (LiveBackdropQuality::Full, [2560, 1440], 2.0),
            (LiveBackdropQuality::Balanced, [1920, 1080], 1.5),
            (LiveBackdropQuality::Performance, [1280, 720], 1.0),
        ] {
            let scaled = quality.screen(&screen);
            assert_eq!(scaled.size_in_pixels, size);
            assert_eq!(scaled.pixels_per_point, density);
        }
        assert_eq!(LiveBackdropQuality::default(), LiveBackdropQuality::Full);
    }

    #[test]
    fn odd_and_tiny_targets_round_up_without_zero_allocations() {
        for (input, expected) in [([801, 603], [401, 302]), ([1, 1], [1, 1]), ([0, 0], [0, 0])] {
            let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: input, pixels_per_point: 1.25 };
            assert_eq!(LiveBackdropQuality::Performance.screen(&screen).size_in_pixels, expected);
        }
    }
}
