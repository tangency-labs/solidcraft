//! A sheet's paper scene (`geom::paper`) in a file: SVG (A3, #4890), sheet PNG (A4), PDF (P1) and
//! DXF.

use std::fmt::Write;

use solidcraft_geom::paper::Grid;

use crate::{IoError, Result};

mod dxf;
mod flatten;
mod pdf;
mod png;
mod svg;

pub use dxf::{DxfReport, paper_dxf};
pub use pdf::{PdfOptions, PdfPictures, PdfReport, paper_pdf, paper_pdf_pages, paper_pdf_pages_with};
pub use png::{DPI, MAX_PICTURE_DPI, MAX_POINTS, PICTURE_DPI, PngColour, PngOptions, PngReport, paper_png};
pub use svg::{SvgReport, paper_svg, paper_svg_on};

/// A coloured paper a sheet can be exported on, besides the default white (#10679). It belongs to
/// the export: a sheet holds no colour, and nothing here is saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaperColour {
    /// The deep end of the blueprint in the macOS installer art (`make_background.py`).
    Blueprint,
    /// The pigment's listed value.
    PrussianBlue,
}

impl PaperColour {
    /// What is drawn on either paper: lines, dimensions and text. The installer art's line colour.
    pub const INK: [u8; 3] = [0xE2, 0xEE, 0xFC];

    /// The paper, as sRGB.
    pub const fn rgb(self) -> [u8; 3] {
        match self {
            PaperColour::Blueprint => [0x12, 0x3A, 0x76],
            PaperColour::PrussianBlue => [0x00, 0x31, 0x53],
        }
    }
}

/// Grid steps written as millimetres onto `out`, on a grid of 10^`decimals` steps a millimetre:
/// the integer part, then up to `decimals` decimals without trailing zeros. No exponent, no "-0",
/// nothing that depends on the locale.
fn push_mm(out: &mut String, n: i64, decimals: u32) {
    let unit = 10_u64.pow(decimals);
    let (sign, n) = if n < 0 { ("-", n.unsigned_abs()) } else { ("", n.unsigned_abs()) };
    let (whole, mut part, mut digits) = (n / unit, n % unit, decimals as usize);
    let _ = write!(out, "{sign}{whole}");
    if part != 0 {
        while part % 10 == 0 {
            (part, digits) = (part / 10, digits - 1);
        }
        let _ = write!(out, ".{part:0digits$}");
    }
}

/// A writer's numbers: millimetres on a grid of 10^`decimals` steps a millimetre, written with up
/// to that many decimals. One value, so a writer's grid and its number text can't disagree.
#[derive(Clone, Copy)]
struct Decimals(u32);

/// Whole nanometres: SVG's numbers, and the grid PNG draws on.
const NANOMETRES: Decimals = Decimals(6);
/// 10 nm: PDF's numbers, Annex C's five decimals.
const TEN_NANOMETRES: Decimals = Decimals(5);

impl Decimals {
    /// Steps a millimetre.
    const fn per_mm(self) -> f64 {
        10_u64.pow(self.0) as f64
    }

    /// The grid the writer puts a scene on.
    fn grid(self) -> Result<Grid> {
        Grid::new(self.per_mm()).ok_or_else(|| IoError::Invalid(format!("a paper writer has no grid of {} decimals", self.0)))
    }

    /// Grid steps written as millimetres onto `out` ([`push_mm`]).
    fn push(self, out: &mut String, n: i64) {
        push_mm(out, n, self.0);
    }
}

/// Every grid a paper writer puts a scene on, finest first: for a check that a mark survives every
/// writer (the engine's hatch dots).
pub fn writer_grids() -> Result<[Grid; 2]> {
    Ok([NANOMETRES.grid()?, TEN_NANOMETRES.grid()?])
}

#[cfg(test)]
#[path = "paper_tests.rs"]
mod tests;
