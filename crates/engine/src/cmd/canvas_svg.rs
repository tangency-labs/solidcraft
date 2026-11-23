//! An SVG given as a canvas or decal image, drawn into a PNG as it is inserted. The design keeps
//! the picture, not the drawing, so the canvas reopens, draws and exports as any PNG canvas does.
//!
//! resvg (all its features off) draws it without text, system fonts, embedded raster images or
//! SVGZ decompression, and an `<image>` that
//! names a file or address draws nothing, so reading an SVG reads no other file (images given
//! inline as `data:` URLs still draw). A compressed SVG (SVGZ) is refused rather than inflated to
//! an unknown size. The picture's size is capped before anything is drawn.

use image::ImageEncoder;

/// The longest side of the drawn picture when none is asked for, in pixels.
pub(super) const DEFAULT_SIDE: u32 = 1024;
/// The largest longest side that may be asked for, in pixels.
pub(super) const MAX_SIDE: u32 = 4096;

/// The two bytes a gzip stream, and so an SVGZ file, starts with.
const GZIP: [u8; 2] = [0x1f, 0x8b];

/// Whether `bytes` look like an SVG file rather than a PNG or JPEG: after a byte-order mark,
/// white space and the prolog (an XML declaration, processing instructions, comments and a
/// DOCTYPE, however long), the root element is `svg` (namespace prefix allowed). A gzipped file
/// counts too, so that [`rasterize`] can refuse it as a compressed SVG.
pub(super) fn is_svg(bytes: &[u8]) -> bool {
    if bytes.starts_with(&GZIP) {
        return true;
    }
    let mut rest = bytes.strip_prefix(b"\xef\xbb\xbf".as_slice()).unwrap_or(bytes);
    loop {
        rest = rest.trim_ascii_start();
        rest = if let Some(r) = rest.strip_prefix(b"<?".as_slice()) {
            after(r, b"?>")
        } else if let Some(r) = rest.strip_prefix(b"<!--".as_slice()) {
            after(r, b"-->")
        } else if let Some(r) = rest.strip_prefix(b"<!".as_slice()) {
            after_declaration(r)
        } else {
            return root_is_svg(rest);
        };
    }
}

/// What follows the first `end` in `bytes`; nothing when there is none.
fn after<'a>(bytes: &'a [u8], end: &[u8]) -> &'a [u8] {
    bytes.windows(end.len()).position(|w| w == end).and_then(|i| bytes.get(i + end.len()..)).unwrap_or_default()
}

/// What follows a `<!…>` declaration such as a DOCTYPE, whose internal subset (`[…]`) and quoted
/// strings may hold `>`; nothing when it never ends.
fn after_declaration(bytes: &[u8]) -> &[u8] {
    let (mut depth, mut quote) = (0usize, None);
    for (i, &b) in bytes.iter().enumerate() {
        match (quote, b) {
            (Some(q), _) if b == q => quote = None,
            (Some(_), _) => {}
            (None, b'"' | b'\'') => quote = Some(b),
            (None, b'[') => depth += 1,
            (None, b']') => depth = depth.saturating_sub(1),
            (None, b'>') if depth == 0 => return bytes.get(i + 1..).unwrap_or_default(),
            _ => {}
        }
    }
    &[]
}

/// Whether `bytes` start with an element named `svg` or `prefix:svg`.
fn root_is_svg(bytes: &[u8]) -> bool {
    let Some(tag) = bytes.strip_prefix(b"<".as_slice()) else { return false };
    let name = tag.split(|b| b.is_ascii_whitespace() || *b == b'>' || *b == b'/').next().unwrap_or_default();
    name == b"svg" || name.ends_with(b":svg")
}

/// The SVG drawn into a PNG whose longest side is `side` pixels (1 to [`MAX_SIDE`]), its shape
/// kept; transparent where the drawing leaves the page bare.
pub(super) fn rasterize(bytes: &[u8], side: u32) -> Result<Vec<u8>, String> {
    if bytes.starts_with(&GZIP) {
        return Err("a compressed SVG (SVGZ) cannot be inserted: save it as a plain SVG".into());
    }
    let side = side.clamp(1, MAX_SIDE);
    // An `<image>` naming a file or address is left out; only inline `data:` images draw.
    let no_files: resvg::usvg::ImageHrefStringResolverFn = Box::new(|_, _| None);
    let mut options = resvg::usvg::Options::default();
    options.image_href_resolver.resolve_string = no_files;
    let tree = resvg::usvg::Tree::from_data(bytes, &options).map_err(|e| format!("the SVG cannot be read: {e}"))?;
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

    /// The root element decides, however long the prolog before it: a comment, declaration or
    /// DOCTYPE mentioning `<svg` does not make another document an SVG.
    #[test]
    fn the_root_element_after_any_prolog_decides() {
        let long = format!("<?xml version=\"1.0\"?>\n<!-- {} -->\n<!DOCTYPE svg [ <!ENTITY a \">\"> ]>\n{SQUARES_ROOT}", "x".repeat(5000));
        assert!(is_svg(long.as_bytes()), "the root comes after more than 4 KiB");
        assert_eq!(image::load_from_memory(&rasterize(long.as_bytes(), 40).unwrap()).unwrap().width(), 40);
        assert!(is_svg(b"<svg:svg xmlns:svg=\"http://www.w3.org/2000/svg\"/>"));
        assert!(!is_svg(b"<!-- <svg> --><html/>"));
        assert!(!is_svg(b"<svgfoo/>"));
        assert!(!is_svg(b"<!-- never closed <svg"));
        assert!(!is_svg(b"\xff\xd8\xff\xe0 <svg"), "a JPEG");
    }

    /// An `<image>` naming a file draws nothing: the file is never read into the picture.
    #[test]
    fn an_image_naming_a_file_draws_nothing() {
        let dir = std::env::temp_dir().join(format!("solidcraft-svg-href-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let red = dir.join("red.svg");
        std::fs::write(
            &red,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="#ff0000"/></svg>"##,
        )
        .unwrap();
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="10" height="10"><image width="10" height="10" xlink:href="{}"/></svg>"#,
            red.display()
        );
        let img = image::load_from_memory(&rasterize(svg.as_bytes(), 10).unwrap()).unwrap().to_rgba8();
        assert_eq!(img.get_pixel(5, 5).0[3], 0, "the referenced file was drawn");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A gzipped SVG is recognised and refused, not inflated.
    #[test]
    fn a_compressed_svg_is_refused() {
        let svgz = [0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert!(is_svg(&svgz));
        assert!(rasterize(&svgz, 100).unwrap_err().contains("SVGZ"));
    }

    const SQUARES_ROOT: &str =
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="#ff0000"/></svg>"##;

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
