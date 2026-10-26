//! The pixel grid a picture of a paper rectangle is drawn on (#5624): the one rule the picture
//! renderer and the writers that place its pixels share, so a picture and the lines drawn over it
//! register exactly.
//!
//! - Pixels are `p = 25.4 / dpi` millimetres square.
//! - The grid is anchored at the rectangle's lower-left corner `(x0, y0)` and rounds outward: a
//!   `w × h` mm rectangle is `W = ceil(w / p − 1e-9)` by `H = ceil(h / p − 1e-9)` pixels, so the
//!   picture covers `[x0, x0 + W·p] × [y0, y0 + H·p]` and any overhang is at the top and the right.
//!   (The 1e-9 keeps a side that is a whole number of pixels, such as 25.4 mm at 300 dpi, from
//!   gaining one through rounding.)
//! - Column `i` covers `[x0 + i·p, x0 + (i + 1)·p)`. Rows run top-down, as an image is stored:
//!   row `r` covers `[y0 + (H − r − 1)·p, y0 + (H − r)·p)`.
//! - A pixel is sampled at its centre, or at a quarter and three quarters of the way across it
//!   in each direction when it is sampled 2 × 2.
//! - A whole sheet's grid ([`PixelGrid::sheet`]) is anchored at its top-left corner instead, as an
//!   image of the sheet is drawn, and a picture on the sheet is drawn into its window of that grid
//!   ([`PixelGrid::window`]), so placing it is a copy of whole pixels.

use crate::{Aabb2, Vec2};

/// The longest side a grid may have, in pixels.
pub const MAX_SIDE_PX: u32 = 65_536;

/// The pixel grid of one paper rectangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PixelGrid {
    /// The lower-left corner, paper millimetres.
    pub x0: f64,
    pub y0: f64,
    /// The pixel size, millimetres.
    pub pitch: f64,
    /// Columns and rows.
    pub width: u32,
    pub height: u32,
}

/// Columns `x0..x1` and rows `y0..y1` of a grid, row 0 at the top.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridWindow {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}

/// Why a rectangle has no grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PixelGridError {
    /// A coordinate, a size or the dpi is not finite, or a size or the dpi is not above 0.
    Invalid,
    /// A side would be more than [`MAX_SIDE_PX`] pixels.
    TooLarge { pixels: f64 },
}

impl std::fmt::Display for PixelGridError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid => write!(f, "a picture needs a finite position and a size and dpi above 0"),
            Self::TooLarge { pixels } => write!(f, "a picture side of {pixels:.0} px is over the limit of {MAX_SIDE_PX} px"),
        }
    }
}

impl std::error::Error for PixelGridError {}

impl PixelGrid {
    /// The grid of the `width_mm × height_mm` rectangle whose lower-left corner is `(x0, y0)`, at
    /// `dpi`.
    pub fn new(x0: f64, y0: f64, width_mm: f64, height_mm: f64, dpi: f64) -> Result<PixelGrid, PixelGridError> {
        if ![x0, y0, width_mm, height_mm, dpi].iter().all(|v| v.is_finite()) || width_mm <= 0.0 || height_mm <= 0.0 || dpi <= 0.0 {
            return Err(PixelGridError::Invalid);
        }
        let pitch = 25.4 / dpi;
        let side = |mm: f64| {
            let pixels = (mm / pitch - 1e-9).ceil().max(1.0);
            if pixels.is_finite() && pixels <= f64::from(MAX_SIDE_PX) { Ok(pixels as u32) } else { Err(PixelGridError::TooLarge { pixels }) }
        };
        Ok(PixelGrid { x0, y0, pitch, width: side(width_mm)?, height: side(height_mm)? })
    }

    /// The grid of a whole `size_mm` sheet at `dpi`, anchored at its top-left corner: row 0's top
    /// is the sheet's top edge, and any overhang is at the bottom and the right.
    pub fn sheet(size_mm: Vec2, dpi: f64) -> Result<PixelGrid, PixelGridError> {
        let grid = PixelGrid::new(0.0, 0.0, size_mm.x, size_mm.y, dpi)?;
        Ok(PixelGrid { y0: size_mm.y - f64::from(grid.height) * grid.pitch, ..grid })
    }

    /// The columns and rows of this grid that meet `rect` (paper millimetres) over more than
    /// 1e-9 of a pixel, so a side on a pixel edge takes no sliver of the pixel beyond it; `None`
    /// when they share no pixel or `rect` isn't finite.
    pub fn window(&self, rect: Aabb2) -> Option<GridWindow> {
        if rect.is_empty() || !rect.min.is_finite() || !rect.max.is_finite() {
            return None;
        }
        // Pixel coordinates: columns from the left edge, rows down from the top edge.
        let top = self.y0 + f64::from(self.height) * self.pitch;
        let (columns, rows) = (f64::from(self.width), f64::from(self.height));
        let x0 = ((rect.min.x - self.x0) / self.pitch + 1e-9).floor().max(0.0);
        let x1 = ((rect.max.x - self.x0) / self.pitch - 1e-9).ceil().min(columns);
        let y0 = ((top - rect.max.y) / self.pitch + 1e-9).floor().max(0.0);
        let y1 = ((top - rect.min.y) / self.pitch - 1e-9).ceil().min(rows);
        (x0 < x1 && y0 < y1).then_some(GridWindow { x0: x0 as u32, y0: y0 as u32, x1: x1 as u32, y1: y1 as u32 })
    }

    /// Columns times rows.
    pub fn pixels(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    /// The paper x range of column `i`.
    pub fn column(&self, i: u32) -> (f64, f64) {
        (self.x0 + f64::from(i) * self.pitch, self.x0 + (f64::from(i) + 1.0) * self.pitch)
    }

    /// The paper y range of row `r` (row 0 is the top).
    pub fn row(&self, r: u32) -> (f64, f64) {
        let from_bottom = f64::from(self.height) - f64::from(r);
        (self.y0 + (from_bottom - 1.0) * self.pitch, self.y0 + from_bottom * self.pitch)
    }

    /// The paper rectangle the whole grid covers: `(x0, y0, x1, y1)`.
    pub fn covers(&self) -> (f64, f64, f64, f64) {
        (self.x0, self.y0, self.x0 + f64::from(self.width) * self.pitch, self.y0 + f64::from(self.height) * self.pitch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 100 mm at 300 dpi is 1181.1 pixels, so 1182; a whole number of pixels stays whole.
    #[test]
    fn sides_round_outward_and_whole_sides_stay_whole() {
        let g = PixelGrid::new(10.0, 20.0, 100.0, 50.0, 300.0).unwrap();
        assert_eq!((g.width, g.height), (1182, 591));
        assert!((g.pitch - 25.4 / 300.0).abs() < 1e-15);
        let whole = PixelGrid::new(0.0, 0.0, 25.4, 2.54, 300.0).unwrap();
        assert_eq!((whole.width, whole.height), (300, 30));
        // 25 pixels of 0.1 mm: 2.5 / 0.1 is 25.000000000000004 in floating point.
        assert_eq!(PixelGrid::new(0.0, 0.0, 2.5, 2.5, 254.0).unwrap().width, 25);
        assert_eq!(PixelGrid::new(0.0, 0.0, 0.001, 0.001, 300.0).unwrap().pixels(), 1, "anything at all is one pixel");
    }

    /// The grid sits on the lower-left corner; its overhang is at the top and the right, and row 0
    /// is the top row.
    #[test]
    fn the_grid_is_anchored_lower_left_and_rows_run_top_down() {
        let g = PixelGrid::new(10.0, 20.0, 100.0, 50.0, 300.0).unwrap();
        let (x0, y0, x1, y1) = g.covers();
        assert_eq!((x0, y0), (10.0, 20.0));
        assert!(x1 >= 110.0 && x1 - 110.0 < g.pitch && y1 >= 70.0 && y1 - 70.0 < g.pitch, "{x1} {y1}");
        assert_eq!(g.column(0).0, 10.0);
        assert!((g.column(g.width - 1).1 - x1).abs() < 1e-9);
        let (bottom, top) = g.row(g.height - 1);
        assert!((bottom - 20.0).abs() < 1e-12 && (top - 20.0 - g.pitch).abs() < 1e-12, "the last row is the bottom: {bottom} {top}");
        assert!((g.row(0).1 - y1).abs() < 1e-9, "the first row is the top");
        // Not the paper rectangle's own top: the overhang is there.
        assert!(g.row(0).1 > 70.0);
    }

    #[test]
    fn hostile_rectangles_are_refused() {
        for (x, y, w, h, dpi) in [
            (f64::NAN, 0.0, 1.0, 1.0, 300.0),
            (0.0, f64::INFINITY, 1.0, 1.0, 300.0),
            (0.0, 0.0, 0.0, 1.0, 300.0),
            (0.0, 0.0, 1.0, -1.0, 300.0),
            (0.0, 0.0, 1.0, 1.0, 0.0),
            (0.0, 0.0, 1.0, 1.0, f64::NAN),
        ] {
            assert_eq!(PixelGrid::new(x, y, w, h, dpi), Err(PixelGridError::Invalid), "{x} {y} {w} {h} {dpi}");
        }
        // A0's long side at 1200 dpi fits; at 1500 dpi it does not.
        assert_eq!(PixelGrid::new(0.0, 0.0, 1189.0, 841.0, 1200.0).unwrap().width, 56_174);
        assert!(matches!(PixelGrid::new(0.0, 0.0, 1189.0, 841.0, 1500.0), Err(PixelGridError::TooLarge { .. })));
        assert!(matches!(PixelGrid::new(0.0, 0.0, 1e300, 1.0, 300.0), Err(PixelGridError::TooLarge { .. })));
    }

    /// A sheet's grid starts at its top edge: an A3 landscape sheet at 300 dpi is 4961 × 3508
    /// pixels, its top row's top at 297 mm and its overhang below the sheet's bottom edge.
    #[test]
    fn a_sheets_grid_is_anchored_at_its_top_left_corner() {
        let g = PixelGrid::sheet(Vec2::new(420.0, 297.0), 300.0).unwrap();
        assert_eq!((g.width, g.height), (4961, 3508));
        assert_eq!(g.x0, 0.0);
        assert!((g.row(0).1 - 297.0).abs() < 1e-9, "{:?}", g.row(0));
        let (bottom, _) = g.row(g.height - 1);
        assert!(bottom < 0.0 && bottom > -g.pitch, "the overhang is at the bottom: {bottom}");
        assert_eq!(PixelGrid::sheet(Vec2::new(420.0, f64::NAN), 300.0), Err(PixelGridError::Invalid));
    }

    /// A window takes the pixels a rectangle meets: exactly its own on pixel edges, the partly
    /// covered ones too a little inside them, and none outside the grid.
    #[test]
    fn a_window_takes_the_pixels_a_rectangle_meets() {
        // 1 mm pixels on a 100 × 50 mm sheet, so row r spans y 49 − r to 50 − r.
        let g = PixelGrid::sheet(Vec2::new(100.0, 50.0), 25.4).unwrap();
        let rect = |x0: f64, y0: f64, x1: f64, y1: f64| Aabb2 { min: Vec2::new(x0, y0), max: Vec2::new(x1, y1) };
        let window = |x0, y0, x1, y1| Some(GridWindow { x0, y0, x1, y1 });
        assert_eq!(g.window(rect(40.0, 20.0, 60.0, 30.0)), window(40, 20, 60, 30));
        assert_eq!(g.window(rect(39.6, 20.0, 60.0, 30.4)), window(39, 19, 60, 30));
        // A side a hair past a pixel edge (floating-point noise) takes no sliver.
        assert_eq!(g.window(rect(40.0 - 1e-12, 20.0, 60.0 + 1e-12, 30.0)), window(40, 20, 60, 30));
        // Clipped to the grid; outside it, or with no area, nothing.
        assert_eq!(g.window(rect(-10.0, -10.0, 5.0, 5.0)), window(0, 45, 5, 50));
        assert_eq!(g.window(rect(100.0, 0.0, 120.0, 10.0)), None);
        assert_eq!(g.window(rect(10.0, 10.0, 10.0, 20.0)), None);
        assert_eq!(g.window(Aabb2::EMPTY), None);
        assert_eq!(g.window(rect(f64::NAN, 0.0, 10.0, 10.0)), None);
        assert_eq!(g.window(rect(0.0, 0.0, f64::INFINITY, 10.0)), None);
        // On a 300 dpi grid, a picture's rectangle on whole pixels is exactly its own grid's size.
        let g = PixelGrid::sheet(Vec2::new(420.0, 297.0), 300.0).unwrap();
        let p = g.pitch;
        let (left, top) = (100.0 * p, g.row(200).1);
        let w = g.window(rect(left, top - 591.0 * p, left + 1182.0 * p, top)).unwrap();
        assert_eq!((w.x0, w.x1 - w.x0, w.y0, w.y1 - w.y0), (100, 1182, 200, 591));
    }
}
