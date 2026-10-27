//! A sheet's paper scene (`geom::paper`) in a file: SVG (A3, #4890), sheet PNG (A4) and PDF (P1);
//! DXF follows.

use std::fmt::Write;

mod pdf;
mod png;
mod svg;

pub use pdf::{PdfOptions, PdfReport, paper_pdf};
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

#[cfg(test)]
#[path = "paper_tests.rs"]
mod tests;
