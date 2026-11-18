#[cfg(test)]
mod tests {
    use super::*;

    fn luma(p: &Picture, x: u32, y: u32) -> f32 {
        let i = ((y * p.size[0] + x) * 4) as usize;
        (f32::from(p.rgba[i]) + f32::from(p.rgba[i + 1]) + f32::from(p.rgba[i + 2])) / 765.0
    }

    #[test]
    fn the_galaxy_without_a_picture_is_a_dark_panorama_with_stars() {
        let sky = galaxy(512);
        assert_eq!(sky.size, [512, 256]);
        assert_eq!(sky.rgba.len(), 512 * 256 * 4);
        assert!(is_panorama(sky.size));
        let all: Vec<f32> = (0..256).flat_map(|y| (0..512).map(move |x| (x, y))).map(|(x, y)| luma(&sky, x, y)).collect();
        let mean = all.iter().sum::<f32>() / all.len() as f32;
        assert!((0.01..0.3).contains(&mean), "a night sky: {mean}");
        assert!(all.iter().any(|&l| l > 0.8), "stars are bright");
        assert!(sky.rgba.chunks(4).all(|px| px[3] == 255), "opaque");
    }

    #[test]
    fn the_galaxy_is_the_same_every_time_and_wraps_without_a_seam() {
        let (a, b) = (galaxy(512), galaxy(512));
        assert!(a.rgba == b.rgba);
        let seam: f32 = (0..256).map(|y| (luma(&a, 0, y) - luma(&a, 511, y)).abs()).sum::<f32>() / 256.0;
        let inside: f32 = (0..256).map(|y| (luma(&a, 255, y) - luma(&a, 256, y)).abs()).sum::<f32>() / 256.0;
        assert!(seam < inside * 2.0 + 0.02, "seam {seam}, inside {inside}");
    }

    #[test]
    fn a_picture_about_twice_as_wide_as_tall_is_a_panorama() {
        assert!(is_panorama([4096, 2048]) && is_panorama([2000, 1000]));
        assert!(!is_panorama([1920, 1080]) && !is_panorama([1000, 1000]));
    }

    #[test]
    fn a_flat_picture_covers_the_window_from_its_centre() {
        let wide = cover_uv(egui::vec2(1000.0, 500.0), egui::vec2(500.0, 500.0));
        assert_eq!((wide.min.y, wide.max.y), (0.0, 1.0));
        assert!((wide.min.x - 0.25).abs() < 1e-6 && (wide.max.x - 0.75).abs() < 1e-6);
        let tall = cover_uv(egui::vec2(500.0, 500.0), egui::vec2(1000.0, 500.0));
        assert_eq!((tall.min.x, tall.max.x), (0.0, 1.0));
        assert!((tall.min.y - 0.25).abs() < 1e-6 && (tall.max.y - 0.75).abs() < 1e-6);
    }

    #[test]
    fn a_picture_is_decoded_and_shrunk_to_what_the_gpu_takes() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(64, 32, image::Rgba([10, 20, 30, 255])).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let whole = decode(png.get_ref(), 1024).unwrap();
        assert_eq!((whole.size, &whole.rgba[..4]), ([64, 32], &[10, 20, 30, 255][..]));
        assert_eq!(decode(png.get_ref(), 16).unwrap().size, [16, 8]);
        assert!(decode(b"not a picture", 1024).is_err());
    }
}
