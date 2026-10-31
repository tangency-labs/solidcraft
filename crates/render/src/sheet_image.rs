//! A drawing's image file on paper (W38e, #5425): a PNG or JPEG decoded once, then stretched over
//! its paper rectangle onto whatever pixel grid a writer asks for (`solidcraft_geom::paper::Pictures`).
//!
//! - Only PNG and JPEG, told by their bytes, and only whole files. The size is read from the
//!   header and refused over [`MAX_IMAGE_PIXELS`] before anything is decoded.
//! - An EXIF orientation is applied, so a photo stands as every viewer shows it. Colour profiles
//!   are not read: the pixels are taken as sRGB, as browsers take an untagged image.
//! - Each pixel of the grid is the average of the image under the part of that pixel inside the
//!   rectangle (a box filter), in linear light with alpha premultiplied: a shrunk logo keeps its
//!   edges and gains no dark fringe. Pixels of the grid wholly outside the rectangle (a picture's
//!   own grid rounds outward) are transparent.

use std::io::Cursor;

use image::{DynamicImage, ImageDecoder, ImageReader};
use solidcraft_geom::Aabb2;
use solidcraft_geom::paper::MAX_IMAGE_PIXELS;
use solidcraft_geom::pixel_grid::{GridWindow, PixelGrid};
use solidcraft_geom::srgb::{decode8, encode8};

use crate::texture_input::is_whole;

#[cfg(test)]
#[path = "sheet_image_tests.rs"]
mod tests;

/// An image file's pixels: straight-alpha sRGB RGBA, top row first.
#[derive(Clone, Debug, PartialEq)]
pub struct SheetImage {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

/// Which texels along one side a pixel of the grid covers: the first, then how much of each, in
/// texels. Empty when it covers none.
struct Span {
    first: usize,
    weights: Vec<f64>,
}

impl SheetImage {
    /// The image in `bytes`, decoded; an error, never a panic, for anything that isn't a whole
    /// PNG or JPEG of at most [`MAX_IMAGE_PIXELS`].
    pub fn decode(bytes: &[u8]) -> Result<SheetImage, String> {
        let Some(format) = image::guess_format(bytes).ok().filter(|f| is_whole(bytes, *f)) else {
            return Err("not a whole PNG or JPEG file".into());
        };
        let decoded = std::panic::catch_unwind(|| {
            let mut decoder = ImageReader::with_format(Cursor::new(bytes), format).into_decoder().map_err(|e| e.to_string())?;
            let (width, height) = decoder.dimensions();
            if u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS {
                return Err(format!("an image of {width} × {height} pixels: at most {} megapixels", MAX_IMAGE_PIXELS / 1_000_000));
            }
            // A 16-bit file takes twice what its pixels do here.
            if decoder.total_bytes() > MAX_IMAGE_PIXELS * 4 {
                return Err(format!("an image of {width} × {height} pixels takes over {} MB to decode", MAX_IMAGE_PIXELS * 4 / 1_000_000));
            }
            let orientation = decoder.orientation().map_err(|e| e.to_string())?;
            let mut image = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
            image.apply_orientation(orientation);
            let rgba = image.into_rgba8();
            Ok(SheetImage { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() })
        });
        decoded.unwrap_or_else(|_| Err("the image decoder failed".into()))
    }

    /// Columns and rows.
    pub fn size(&self) -> [u32; 2] {
        [self.width, self.height]
    }

    /// The image stretched over `rect` (paper millimetres), for `window` of `grid`: straight-alpha
    /// sRGB RGBA, top row first, exactly the window's size.
    pub fn rows(&self, rect: Aabb2, grid: &PixelGrid, window: GridWindow) -> Result<Vec<u8>, String> {
        let size = rect.size();
        if !rect.min.is_finite() || !size.is_finite() || size.x <= 0.0 || size.y <= 0.0 {
            return Err("an image needs a finite rectangle of some size".into());
        }
        if window.x0 >= window.x1 || window.y0 >= window.y1 || window.x1 > grid.width || window.y1 > grid.height {
            return Err("a window outside its grid".into());
        }
        let columns: Vec<Span> = (window.x0..window.x1)
            .map(|i| {
                let (a, b) = grid.column(i);
                span(a - rect.min.x, b - rect.min.x, size.x, self.width)
            })
            .collect();
        // Rows run top-down in both the grid and the image.
        let rows: Vec<Span> = (window.y0..window.y1)
            .map(|r| {
                let (low, high) = grid.row(r);
                span(rect.max.y - high, rect.max.y - low, size.y, self.height)
            })
            .collect();
        let linear: Vec<f64> = (0..=255).map(decode8).collect();
        let mut out = Vec::with_capacity(columns.len() * rows.len() * 4);
        for row in &rows {
            for column in &columns {
                out.extend(self.average(column, row, &linear));
            }
        }
        Ok(out)
    }

    /// The premultiplied, linear-light average of the texels `column` and `row` cover.
    fn average(&self, column: &Span, row: &Span, linear: &[f64]) -> [u8; 4] {
        let total = column.weights.iter().sum::<f64>() * row.weights.iter().sum::<f64>();
        if total <= 0.0 {
            return [0; 4];
        }
        let mut sum = [0.0; 4];
        for (y, wy) in (row.first..).zip(&row.weights) {
            for (x, wx) in (column.first..).zip(&column.weights) {
                let at = (y * self.width as usize + x) * 4;
                let Some(&[r, g, b, a]) = self.rgba.get(at..at + 4) else { continue };
                let (alpha, w) = (f64::from(a) / 255.0, wx * wy);
                for (s, c) in sum.iter_mut().zip([r, g, b]) {
                    *s += linear.get(usize::from(c)).copied().unwrap_or(0.0) * alpha * w;
                }
                sum[3] += alpha * w;
            }
        }
        if sum[3] <= 0.0 {
            return [0; 4];
        }
        let colour = |s: f64| encode8(s / sum[3]);
        [colour(sum[0]), colour(sum[1]), colour(sum[2]), (sum[3] / total * 255.0).round() as u8]
    }
}

/// The texels of a side `texels` long that `[a, b)` covers, where `a` and `b` are millimetres
/// from the start of a side `side` millimetres long.
fn span(a: f64, b: f64, side: f64, texels: u32) -> Span {
    let n = f64::from(texels);
    let (a, b) = ((a / side * n).clamp(0.0, n), (b / side * n).clamp(0.0, n));
    if b - a <= 1e-9 {
        return Span { first: 0, weights: Vec::new() };
    }
    let (first, last) = (a.floor() as usize, b.ceil() as usize);
    let weights = (first..last).map(|t| b.min(t as f64 + 1.0) - a.max(t as f64)).collect();
    Span { first, weights }
}
