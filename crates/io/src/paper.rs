//! A sheet's paper scene (`geom::paper`) in a file: SVG (A3, #4890), sheet PNG (A4) and PDF (P1);
//! DXF follows.

use std::fmt::Write;

mod pdf;
mod png;
mod svg;

pub use pdf::{PdfOptions, PdfReport, paper_pdf, paper_pdf_pages};
pub use png::{DPI, MAX_POINTS, PngColour, PngOptions, PngReport, paper_png};
pub use svg::{SvgReport, paper_svg};

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
