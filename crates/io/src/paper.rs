//! A sheet's paper scene (`geom::paper`) in a file: SVG (A3, #4890) and sheet PNG (A4); DXF and PDF follow.

mod png;
mod svg;

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

#[cfg(test)]
#[path = "paper_tests.rs"]
mod tests;
