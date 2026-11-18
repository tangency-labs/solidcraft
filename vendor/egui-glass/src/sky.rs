#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_shown_smaller_takes_a_smaller_level() {
        let flat = |x: f32| SkyView::Flat { min: [0.0, 0.0], max: [x, 1.0] };
        assert_eq!(level(&flat(1.0), [1024, 512], [1024, 512]), 0.0);
        assert_eq!(level(&flat(1.0), [1024, 512], [256, 128]), 2.0);
        assert_eq!(level(&flat(0.5), [1024, 512], [2048, 1024]), 0.0, "never below the full picture");
    }

    #[test]
    fn a_panorama_takes_the_level_of_the_sky_one_window_pixel_covers() {
        let round = SkyView::Round { right: [0.5, 0.0, 0.0], up: [0.0, 0.0, 0.5], forward: [0.0, 1.0, 0.0] };
        assert_eq!(level(&round, [2048, 1024], [1600, 1024]), 0.0, "a window taller than the sky's pixels needs the full picture");
        let squeezed = level(&round, [2048, 1024], [1600, 256]);
        assert!((squeezed - (2048.0 / std::f32::consts::TAU / 256.0).log2()).abs() < 1e-5, "{squeezed}");
    }
}
