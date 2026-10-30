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
//!   that isn't written, and streamed into the file, so an A0 sheet at 2400 dpi (8.9 billion
//!   pixels) needs no more memory than an A4 one, and the bytes are the same whatever the band.
//! - **The file** records its dpi (`pHYs`), so it prints at the sheet's size, and is sRGB.

use std::io::Write;
use std::ops::RangeInclusive;

use solidcraft_geom::paper::{self, Grid, GridItem, GridPath, GridReport, Prepared};
use tiny_skia::{Color, FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

use solidcraft_doc::drawing::{DEFAULT_DPI, MAX_DPI, MIN_DPI};

use crate::{IoError, Result};

/// The resolutions a sheet PNG may have, in dots per inch: a sheet's.
pub const DPI: RangeInclusive<f64> = MIN_DPI..=MAX_DPI;
/// The most points a sheet's lines may flatten to in all.
pub const MAX_POINTS: usize = 1 << 24;
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

/// What writing a sheet PNG did: the grid's report, and the image's width and height in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PngReport {
    pub grid: GridReport,
    pub pixels: [u32; 2],
}

/// The scene as a sheet PNG written to `out`.
pub fn paper_png(scene: &Prepared, options: PngOptions, out: impl Write) -> Result<PngReport> {
    write(scene, options, out, BAND_BYTES)
}

fn write(scene: &Prepared, options: PngOptions, out: impl Write, band_bytes: usize) -> Result<PngReport> {
    let PngOptions { dpi, colour } = options;
    if !DPI.contains(&dpi) {
        return Err(IoError::Invalid(format!("a sheet PNG is 25 to 2400 dpi, not {dpi}")));
    }
    let on = scene.on_grid(Grid::NANOMETRE).map_err(|e| IoError::Invalid(e.to_string()))?;
    let px = dpi / 25.4e6; // pixels a nanometre
    let pixels = [on.size.x, on.size.y].map(|n| ((n as f64 * px).round() as u32).max(1));
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
        }
    }
    let png_error = |e: png::EncodingError| IoError::Invalid(format!("the sheet PNG: {e}"));
    let mut encoder = png::Encoder::new(out, pixels[0], pixels[1]);
    let (colour_type, depth) = match colour {
        PngColour::Grey => (png::ColorType::Grayscale, png::BitDepth::Eight),
        PngColour::BlackWhite => (png::ColorType::Grayscale, png::BitDepth::One),
        PngColour::Rgb => (png::ColorType::Rgb, png::BitDepth::Eight),
        PngColour::Transparent => (png::ColorType::Rgba, png::BitDepth::Eight),
    };
    encoder.set_color(colour_type);
    encoder.set_depth(depth);
    encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let per_metre = (dpi / 0.0254).round() as u32;
    encoder.set_pixel_dims(Some(png::PixelDimensions { xppu: per_metre, yppu: per_metre, unit: png::Unit::Meter }));
    let mut header = encoder.write_header().map_err(png_error)?;
    let mut stream = header.stream_writer().map_err(png_error)?;

    let width = pixels[0] as usize;
    let rows = (band_bytes / (width * 4)).clamp(1, pixels[1] as usize) as u32;
    let mut band = Pixmap::new(pixels[0], rows + 2 * MARGIN).ok_or_else(|| IoError::Invalid("the sheet PNG: no room for a band of pixels".into()))?;
    // A hairline reaches at most a pixel past its line: into the margin rows, never a written one.
    let spans: Vec<(f32, f32)> = shapes
        .iter()
        .map(|(Shape::Fill(path) | Shape::EvenOdd(path) | Shape::Thin(path, _))| (path.bounds().top(), path.bounds().bottom()))
        .collect();
    let mut paint = Paint { anti_alias: colour != PngColour::BlackWhite, ..Paint::default() };
    paint.set_color(Color::BLACK);
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
    Ok(PngReport { grid: on.report, pixels })
}

/// What a band draws for one item.
enum Shape {
    /// A stroke's outline, filled non-zero.
    Fill(Path),
    /// A fill's loops, filled even-odd.
    EvenOdd(Path),
    /// A line under a pixel wide, stroked as a hairline.
    Thin(Path, Stroke),
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
        let at = |p: solidcraft_geom::Vec2| ((p.x * self.px) as f32, ((self.height - p.y) * self.px) as f32);
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
mod tests;
