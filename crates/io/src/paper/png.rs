//! A paper scene as a sheet PNG (#4890, A4): the sheet at its size in pixels at a chosen dpi.
//!
//! - **Geometry** is the scene on the nanometre grid ([`Prepared::on_grid`]), the one SVG writes, so
//!   both draw the same joints, closures and merges. Curves are flattened within 1/8 px
//!   ([`paper::flatten`]) and tiny-skia is given lines only: its own flattening stops at 64 lines a
//!   curve, more than a pixel off on a large arc at a high dpi.
//! - **Paint** as the scene says: black, strokes with butt caps and miter joins (limit 4), fills
//!   even-odd. Each line is stroked once, then filled band by band; a line under a pixel wide is
//!   drawn as tiny-skia's hairline instead, its coverage scaled by its width, so it never vanishes.
//! - **Bands:** the sheet is drawn [`BAND_BYTES`] of pixels at a time, with a row above and below
//!   that isn't written, and streamed into the file, so an A0 sheet at 1200 dpi (1.6 billion
//!   pixels) needs no more memory than an A4 one, and the bytes are the same whatever the band.
//! - **Pixels** are the sheet's pixel grid ([`PixelGrid::sheet`]): anchored at its top-left corner
//!   and rounded outward, so nothing is clipped (210 mm at 300 dpi is 2481 px) and a side over
//!   [`MAX_SIDE_PX`] is refused.
//! - **Pictures** (#5624 S5) are drawn where the scene orders them, into their window of that grid,
//!   from a [`Pictures`] provider asked for just the band's rows at the export's dpi, or at
//!   [`MAX_PICTURE_DPI`] with each pixel copied when the export is finer. Black and white dithers
//!   them; lines stay solid.
//! - **The file** records its dpi (`pHYs`), so it prints at the sheet's size, and is sRGB.

use std::io::Write;
use std::ops::RangeInclusive;

use solidcraft_geom::Vec2;
use solidcraft_geom::paper::{self, Grid, GridItem, GridPath, GridReport, Picture, Pictures, Prepared};
use solidcraft_geom::pixel_grid::{GridWindow, MAX_SIDE_PX, PixelGrid, PixelGridError};
use tiny_skia::{Color, FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, PremultipliedColorU8, Stroke, Transform};

use solidcraft_doc::drawing::{DEFAULT_DPI, MAX_DPI, MIN_DPI};

use crate::{IoError, Result};

/// The resolutions a sheet PNG may have, in dots per inch: a sheet's.
pub const DPI: RangeInclusive<f64> = MIN_DPI..=MAX_DPI;
/// The most points a sheet's lines may flatten to in all.
pub const MAX_POINTS: usize = 1 << 24;
/// The most dots per inch a picture is asked for: above it, a coarser grid's pixels are copied.
pub const MAX_PICTURE_DPI: f64 = 1200.0;
/// The resolutions a picture may be embedded at (SVG, PDF), in dots per inch.
pub const PICTURE_DPI: RangeInclusive<f64> = *DPI.start()..=MAX_PICTURE_DPI;
/// The pixels drawn at a time, as RGBA bytes.
const BAND_BYTES: usize = 16 << 20;
/// Rows drawn above and below each band and not written: tiny-skia starts an edge a little
/// differently where a clip cuts it, so no written row sits on a cut and bands change nothing.
const MARGIN: u32 = 1;

/// How a sheet PNG stores its pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PngColour {
    /// 8-bit greyscale on white paper.
    #[default]
    Grey,
    /// One bit a pixel, black or white, every line at least a pixel wide so none drops out.
    BlackWhite,
    /// 24-bit colour on white paper.
    Rgb,
    /// 32-bit colour, the paper transparent.
    Transparent,
}

/// A sheet PNG's resolution and colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PngOptions {
    pub dpi: f64,
    pub colour: PngColour,
}

impl Default for PngOptions {
    fn default() -> Self {
        PngOptions { dpi: DEFAULT_DPI, colour: PngColour::Grey }
    }
}

/// What writing a sheet PNG did: the grid's report, the image's width and height in pixels, and
/// whether a picture was dithered to black and white.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PngReport {
    pub grid: GridReport,
    pub pixels: [u32; 2],
    pub dithered: bool,
}

/// The scene as a sheet PNG written to `out`, its pictures' pixels from `pictures`.
pub fn paper_png(scene: &Prepared, options: PngOptions, pictures: &dyn Pictures, out: impl Write) -> Result<PngReport> {
    write(scene, options, pictures, out, BAND_BYTES)
}

fn write(scene: &Prepared, options: PngOptions, pictures: &dyn Pictures, out: impl Write, band_bytes: usize) -> Result<PngReport> {
    let PngOptions { dpi, colour } = options;
    if !DPI.contains(&dpi) {
        return Err(IoError::Invalid(format!("a sheet PNG is 25 to 2400 dpi, not {dpi}")));
    }
    let grid = sheet_grid(scene.size_mm(), dpi)?;
    // Pictures are asked for at most MAX_PICTURE_DPI: on the grid of dpi / k, whose pixels are each
    // exactly k × k of the sheet's, since both are anchored at the sheet's top-left corner.
    let k = (dpi / MAX_PICTURE_DPI).ceil().max(1.0) as u32;
    let asked = if k == 1 { grid } else { sheet_grid(scene.size_mm(), dpi / f64::from(k))? };
    let on = scene.on_grid(Grid::NANOMETRE).map_err(|e| IoError::Invalid(e.to_string()))?;
    let px = dpi / 25.4e6; // pixels a nanometre
    let pixels = [grid.width, grid.height];
    let mut sheet = Sheet { px, height: on.size.y as f64, tolerance: 0.125 / px, points: 0 };
    let mut shapes = Vec::new();
    for item in on.groups.iter().flat_map(|g| &g.items) {
        match item {
            GridItem::Stroke { width, path } => {
                let mut builder = PathBuilder::new();
                sheet.trace(&mut builder, path)?;
                let width = (*width as f64 * px) as f32;
                let stroke = Stroke {
                    width: if colour == PngColour::BlackWhite { width.max(1.0) } else { width },
                    miter_limit: 4.0,
                    line_cap: LineCap::Butt,
                    line_join: LineJoin::Miter,
                    dash: None,
                };
                // A line under a pixel wide is drawn as tiny-skia's hairline, its coverage scaled by its
                // width: its outline would fall between the rows the rasteriser samples and vanish.
                let thin = colour != PngColour::BlackWhite && width < 1.0;
                shapes.extend(
                    builder.finish().and_then(|line| if thin { Some(Shape::Thin(line, stroke)) } else { line.stroke(&stroke, 1.0).map(Shape::Fill) }),
                );
            }
            GridItem::Fill { loops } => {
                let mut builder = PathBuilder::new();
                for l in loops {
                    sheet.trace(&mut builder, l)?;
                }
                shapes.extend(builder.finish().map(Shape::EvenOdd));
            }
            GridItem::Picture(picture) => shapes.extend(grid.window(picture.rect).map(|window| Shape::Picture(*picture, window))),
        }
    }
    let png_error = |e: png::EncodingError| IoError::Invalid(format!("the sheet PNG: {e}"));
    let format = match colour {
        PngColour::Grey => (png::ColorType::Grayscale, png::BitDepth::Eight),
        PngColour::BlackWhite => (png::ColorType::Grayscale, png::BitDepth::One),
        PngColour::Rgb => (png::ColorType::Rgb, png::BitDepth::Eight),
        PngColour::Transparent => (png::ColorType::Rgba, png::BitDepth::Eight),
    };
    let mut header = encoder(out, pixels, format, dpi).write_header().map_err(png_error)?;
    let mut stream = header.stream_writer().map_err(png_error)?;

    let width = pixels[0] as usize;
    let rows = (band_bytes / (width * 4)).clamp(1, pixels[1] as usize) as u32;
    let mut band = Pixmap::new(pixels[0], rows + 2 * MARGIN).ok_or_else(|| IoError::Invalid("the sheet PNG: no room for a band of pixels".into()))?;
    // A hairline reaches at most a pixel past its line: into the margin rows, never a written one.
    let spans: Vec<(f32, f32)> = shapes
        .iter()
        .map(|shape| match shape {
            Shape::Fill(path) | Shape::EvenOdd(path) | Shape::Thin(path, _) => (path.bounds().top(), path.bounds().bottom()),
            Shape::Picture(_, window) => (window.y0 as f32, window.y1 as f32),
        })
        .collect();
    let mut paint = Paint { anti_alias: colour != PngColour::BlackWhite, ..Paint::default() };
    paint.set_color(Color::BLACK);
    let mut dithered = false;
    let mut row = Vec::with_capacity(width * 4);
    let mut top = 0;
    while top < pixels[1] {
        let n = rows.min(pixels[1] - top);
        band.fill(if colour == PngColour::Transparent { Color::TRANSPARENT } else { Color::WHITE });
        let shift = Transform::from_translate(0.0, MARGIN as f32 - top as f32);
        for (shape, (y0, y1)) in shapes.iter().zip(&spans) {
            if *y1 >= top as f32 - MARGIN as f32 && *y0 <= (top + n + MARGIN) as f32 {
                match shape {
                    Shape::Fill(path) => band.fill_path(path, &paint, FillRule::Winding, shift, None),
                    Shape::EvenOdd(path) => band.fill_path(path, &paint, FillRule::EvenOdd, shift, None),
                    Shape::Thin(line, stroke) => band.stroke_path(line, &paint, stroke, shift, None),
                    Shape::Picture(picture, window) => {
                        let (r0, r1) = (window.y0.max(top), window.y1.min(top + n));
                        if r0 < r1 {
                            let cut = GridWindow { y0: r0, y1: r1, ..*window };
                            dithered |= composite(&mut band, top, picture, cut, pictures, &asked, k, colour)?;
                        }
                    }
                }
            }
        }
        for line in band.pixels().chunks_exact(width).skip(MARGIN as usize).take(n as usize) {
            row.clear();
            match colour {
                PngColour::Grey => row.extend(line.iter().map(|p| luma(p.red(), p.green(), p.blue()))),
                PngColour::BlackWhite => row.extend(line.chunks(8).map(|eight| {
                    // Most significant bit first; 1 is white.
                    eight.iter().enumerate().fold(0u8, |byte, (i, p)| byte | (u8::from(luma(p.red(), p.green(), p.blue()) >= 128) << (7 - i)))
                })),
                PngColour::Rgb => row.extend(line.iter().flat_map(|p| [p.red(), p.green(), p.blue()])),
                PngColour::Transparent => row.extend(line.iter().flat_map(|p| {
                    let c = p.demultiply();
                    [c.red(), c.green(), c.blue(), c.alpha()]
                })),
            }
            stream.write_all(&row).map_err(|e| IoError::Invalid(format!("the sheet PNG: {e}")))?;
        }
        top += n;
    }
    stream.finish().map_err(png_error)?;
    header.finish().map_err(png_error)?;
    Ok(PngReport { grid: on.report, pixels, dithered })
}

/// The whole sheet's pixel grid at `dpi`, its limit named when a side is too long.
fn sheet_grid(size_mm: Vec2, dpi: f64) -> Result<PixelGrid> {
    PixelGrid::sheet(size_mm, dpi).map_err(|e| {
        IoError::Invalid(match e {
            PixelGridError::TooLarge { pixels } => {
                format!("a sheet PNG side of {pixels:.0} px at {dpi} dpi is over the limit of {MAX_SIDE_PX} px")
            }
            PixelGridError::Invalid => format!("the sheet PNG: {e}"),
        })
    })
}

/// A PNG encoder for `pixels` columns and rows of `format` (colour type, bit depth): sRGB, and
/// `dpi` recorded so it prints at its size.
fn encoder<W: Write>(out: W, pixels: [u32; 2], format: (png::ColorType, png::BitDepth), dpi: f64) -> png::Encoder<'static, W> {
    let mut encoder = png::Encoder::new(out, pixels[0], pixels[1]);
    encoder.set_color(format.0);
    encoder.set_depth(format.1);
    encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let per_metre = (dpi / 0.0254).round() as u32;
    encoder.set_pixel_dims(Some(png::PixelDimensions { xppu: per_metre, yppu: per_metre, unit: png::Unit::Meter }));
    encoder
}

/// Picture `source`'s pixels in `window` of `grid`, from `pictures`, refused unless they are
/// exactly the size asked for.
fn ask(pictures: &dyn Pictures, source: u64, grid: &PixelGrid, window: GridWindow) -> Result<Vec<u8>> {
    let rgba = pictures.rows(source, grid, window).map_err(|e| IoError::Invalid(format!("picture {source}: {e}")))?;
    let expected = window.x1.saturating_sub(window.x0) as usize * window.y1.saturating_sub(window.y0) as usize * 4;
    if rgba.len() != expected {
        return Err(IoError::Invalid(format!("picture {source}: {} bytes where {expected} were asked for", rgba.len())));
    }
    Ok(rgba)
}

/// Picture `source`'s pixels in `window` of its own `grid` as a PNG file of their own (RGBA,
/// straight alpha), for SVG and PDF to embed: asked for [`BAND_BYTES`] of rows at a time, and
/// refused once the file passes `max_bytes`.
pub(super) fn picture_png(source: u64, grid: &PixelGrid, window: GridWindow, pictures: &dyn Pictures, max_bytes: usize) -> Result<Vec<u8>> {
    let png_error = |e: png::EncodingError| IoError::Invalid(format!("picture {source}: {e}"));
    let io_error = |e: std::io::Error| IoError::Invalid(format!("picture {source}: {e}"));
    let pixels = [window.x1.saturating_sub(window.x0), window.y1.saturating_sub(window.y0)];
    let mut out = Capped { bytes: Vec::new(), max: max_bytes, refused: false };
    let written = (|| {
        let mut header =
            encoder(&mut out, pixels, (png::ColorType::Rgba, png::BitDepth::Eight), 25.4 / grid.pitch).write_header().map_err(png_error)?;
        let mut stream = header.stream_writer().map_err(png_error)?;
        let rows = (BAND_BYTES / (pixels[0] as usize * 4).max(1)).max(1) as u32;
        let mut top = window.y0;
        while top < window.y1 {
            let band = GridWindow { y0: top, y1: window.y1.min(top.saturating_add(rows)), ..window };
            stream.write_all(&ask(pictures, source, grid, band)?).map_err(io_error)?;
            top = band.y1;
        }
        stream.finish().map_err(png_error)?;
        header.finish().map_err(png_error)
    })();
    match written {
        _ if out.refused => Err(IoError::Invalid(format!("picture {source}: its PNG would be over {max_bytes} bytes"))),
        Ok(()) => Ok(out.bytes),
        Err(e) => Err(e),
    }
}

/// A file in memory that refuses to grow past `max` bytes, and remembers that it did.
struct Capped {
    bytes: Vec<u8>,
    max: usize,
    refused: bool,
}

impl Write for Capped {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.bytes.len() + buf.len() > self.max {
            self.refused = true;
            return Err(std::io::Error::other("over its limit"));
        }
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The 8 × 8 ordered-dither thresholds, 0 to 63, by row and column.
const BAYER: [[u8; 8]; 8] = [
    [0, 32, 8, 40, 2, 34, 10, 42],
    [48, 16, 56, 24, 50, 18, 58, 26],
    [12, 44, 4, 36, 14, 46, 6, 38],
    [60, 28, 52, 20, 62, 30, 54, 22],
    [3, 35, 11, 43, 1, 33, 9, 41],
    [51, 19, 59, 27, 49, 17, 57, 25],
    [15, 47, 7, 39, 13, 45, 5, 37],
    [63, 31, 55, 23, 61, 29, 53, 21],
];

/// Picture `picture`'s pixels in `window` (sheet rows inside the band that starts at row `top`)
/// over what the band holds: source-over in sRGB, its alpha times its opacity. They are asked for
/// on `asked`, the sheet's grid or one `k` times coarser, each of whose pixels covers k × k of the
/// sheet's. In black and white each pixel is dithered at its place on the sheet; true when one was.
#[allow(clippy::too_many_arguments)]
fn composite(
    band: &mut Pixmap,
    top: u32,
    picture: &Picture,
    window: GridWindow,
    pictures: &dyn Pictures,
    asked: &PixelGrid,
    k: u32,
    colour: PngColour,
) -> Result<bool> {
    let source = picture.source;
    let coarse =
        GridWindow { x0: window.x0 / k, y0: window.y0 / k, x1: window.x1.div_ceil(k).min(asked.width), y1: window.y1.div_ceil(k).min(asked.height) };
    let columns = coarse.x1.saturating_sub(coarse.x0) as usize;
    let rgba = ask(pictures, source, asked, coarse)?;
    let width = band.width() as usize;
    let pixels = band.pixels_mut();
    for y in window.y0..window.y1 {
        for x in window.x0..window.x1 {
            let at = ((y / k - coarse.y0) as usize * columns + (x / k - coarse.x0) as usize) * 4;
            let (Some(&[r, g, b, a]), Some(under)) = (rgba.get(at..at + 4), pixels.get_mut((y + MARGIN - top) as usize * width + x as usize)) else {
                continue;
            };
            let alpha = f32::from(a) / 255.0 * picture.opacity as f32;
            let out_a = (255.0 * alpha + f32::from(under.alpha()) * (1.0 - alpha)).round() as u8;
            let channel = |src: u8, dst: u8| ((f32::from(src) * alpha + f32::from(dst) * (1.0 - alpha)).round() as u8).min(out_a);
            let mut out = [channel(r, under.red()), channel(g, under.green()), channel(b, under.blue())];
            if colour == PngColour::BlackWhite {
                // The paper is white and opaque here, so the premultiplied channels are the colour.
                let threshold = BAYER[(y % 8) as usize][(x % 8) as usize] * 4 + 2;
                out = [if luma(out[0], out[1], out[2]) < threshold { 0 } else { 255 }; 3];
            }
            *under = PremultipliedColorU8::from_rgba(out[0], out[1], out[2], out_a).unwrap_or(*under);
        }
    }
    Ok(colour == PngColour::BlackWhite)
}

/// What a band draws for one item.
enum Shape {
    /// A stroke's outline, filled non-zero.
    Fill(Path),
    /// A fill's loops, filled even-odd.
    EvenOdd(Path),
    /// A line under a pixel wide, stroked as a hairline.
    Thin(Path, Stroke),
    /// A picture, and the pixels of the sheet it covers.
    Picture(Picture, GridWindow),
}

/// The luma of an sRGB pixel, Rec. 709's weights in 256ths.
fn luma(r: u8, g: u8, b: u8) -> u8 {
    ((u32::from(r) * 54 + u32::from(g) * 183 + u32::from(b) * 19 + 128) >> 8) as u8
}

/// The sheet in pixels: the grid's nanometres scaled, y flipped to the page's top, curves
/// flattened within the tolerance, the points counted against [`MAX_POINTS`].
struct Sheet {
    /// Pixels a nanometre.
    px: f64,
    /// The sheet's height, in nanometres.
    height: f64,
    /// The flattening tolerance, in nanometres.
    tolerance: f64,
    points: usize,
}

impl Sheet {
    /// `path` onto `builder` in pixels, closed when it is.
    fn trace(&mut self, builder: &mut PathBuilder, path: &GridPath) -> Result<()> {
        let points = paper::flatten(path, self.tolerance).map_err(|e| IoError::Invalid(e.to_string()))?;
        self.points += points.len();
        if self.points > MAX_POINTS {
            return Err(IoError::Invalid(format!("the sheet needs over {MAX_POINTS} points at this resolution")));
        }
        let at = |p: Vec2| ((p.x * self.px) as f32, ((self.height - p.y) * self.px) as f32);
        for (i, p) in points.into_iter().enumerate() {
            let (x, y) = at(p);
            if i == 0 { builder.move_to(x, y) } else { builder.line_to(x, y) }
        }
        if path.closed {
            builder.close();
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "png_tests.rs"]
pub(super) mod tests;
