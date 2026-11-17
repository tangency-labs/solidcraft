//! An SVG given as a canvas or decal image, drawn into a PNG as it is inserted. The design keeps
//! the picture, not the drawing, so the canvas reopens, draws and exports as any PNG canvas does.
//!
//! resvg draws it without text, system fonts or embedded raster images, so reading an SVG reads
//! no other file. The picture's size is capped before anything is drawn.

use image::ImageEncoder;

/// The longest side of the drawn picture when none is asked for, in pixels.
pub(super) const DEFAULT_SIDE: u32 = 1024;
/// The largest longest side that may be asked for, in pixels.
pub(super) const MAX_SIDE: u32 = 4096;

/// Whether `bytes` look like an SVG file (text starting with a tag, an `<svg` element in its
/// first 4 KiB) or a gzipped one (SVGZ) rather than a PNG or JPEG.
pub(super) fn is_svg(bytes: &[u8]) -> bool {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        return true;
    }
    let head = String::from_utf8_lossy(bytes.get(..bytes.len().min(4096)).unwrap_or_default());
    head.trim_start_matches('\u{feff}').trim_start().starts_with('<') && head.contains("<svg")
}

/// The SVG drawn into a PNG whose longest side is `side` pixels (1 to [`MAX_SIDE`]), its shape
/// kept; transparent where the drawing leaves the page bare.
pub(super) fn rasterize(bytes: &[u8], side: u32) -> Result<Vec<u8>, String> {
    let side = side.clamp(1, MAX_SIDE);
    let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default()).map_err(|e| format!("the SVG cannot be read: {e}"))?;
    let size = tree.size();
    let (w, h) = (f64::from(size.width()), f64::from(size.height()));
    if !(w > 0.0 && h > 0.0 && w.is_finite() && h.is_finite()) {
        return Err("the SVG has no size".into());
    }
    let scale = f64::from(side) / w.max(h);
    let pixels = |x: f64| ((x * scale).round() as u32).clamp(1, MAX_SIDE);
    let (pw, ph) = (pixels(w), pixels(h));
    let mut pixmap = resvg::tiny_skia::Pixmap::new(pw, ph).ok_or_else(|| "the SVG's picture cannot be made".to_string())?;
    let transform = resvg::tiny_skia::Transform::from_scale((f64::from(pw) / w) as f32, (f64::from(ph) / h) as f32);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let mut rgba = Vec::new();
    rgba.try_reserve_exact(pixmap.data().len()).map_err(|_| "not enough memory for the SVG's picture".to_string())?;
    for p in pixmap.pixels() {
        let c = p.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(&rgba, pw, ph, image::ExtendedColorType::Rgba8)
        .map_err(|e| format!("the SVG's picture cannot be written: {e}"))?;
    Ok(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SQUARES: &str = r##"<?xml version="1.0"?>
<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20" viewBox="0 0 40 20">
  <rect x="0" y="0" width="20" height="20" fill="#ff0000"/>
  <rect x="20" y="0" width="20" height="20" fill="#0000ff" fill-opacity="0.5"/>
</svg>"##;

    /// A 2:1 drawing becomes a 2:1 PNG with its longest side as asked, each half its own colour,
    /// the half-transparent one half-transparent.
    #[test]
    fn an_svg_becomes_a_png_of_its_shape_and_colours() {
        assert!(is_svg(SQUARES.as_bytes()));
        assert!(is_svg(b"\xef\xbb\xbf  <svg/>"), "a byte-order mark and spaces before the tag");
        assert!(!is_svg(b"\x89PNG\r\n\x1a\n"));
        assert!(!is_svg(b"<html><body>no drawing</body></html>"));
        let png = rasterize(SQUARES.as_bytes(), 200).unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(img.dimensions(), (200, 100));
        assert_eq!(img.get_pixel(50, 50).0, [255, 0, 0, 255]);
        let blue = img.get_pixel(150, 50).0;
        assert_eq!((blue[0], blue[1], blue[2]), (0, 0, 255));
        assert!((i32::from(blue[3]) - 128).abs() <= 1, "{blue:?}");
    }

    #[test]
    fn hostile_svgs_and_sizes_are_refused_or_capped() {
        assert!(rasterize(b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"0\" height=\"10\"/>", 100).is_err());
        assert!(rasterize(b"not an svg at all", 100).is_err());
        let png = rasterize(SQUARES.as_bytes(), u32::MAX).unwrap();
        assert_eq!(image::load_from_memory(&png).unwrap().width(), MAX_SIDE);
        let png = rasterize(SQUARES.as_bytes(), 0).unwrap();
        assert_eq!(image::load_from_memory(&png).unwrap().width(), 1);
    }
}
