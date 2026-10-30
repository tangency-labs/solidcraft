//! A sheet's paper scene (`geom::paper`) in a file: SVG (A3, #4890) and sheet PNG (A4); DXF and PDF follow.

mod png;
mod svg;

pub use png::{DPI, MAX_PICTURE_DPI, MAX_POINTS, PICTURE_DPI, PngColour, PngOptions, PngReport, paper_png};
pub use svg::{SvgReport, paper_svg};
