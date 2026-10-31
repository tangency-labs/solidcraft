//! A paper scene as SVG (#4890, A3): one user unit a millimetre, the paper's size in mm, and the
//! scene's own curves where SVG has them.
//!
//! - **Numbers** are the scene on the nanometre grid ([`Prepared::on_grid`], whose rules give the
//!   joints, closures and merges), written as millimetres with up to six decimals: no exponent, no
//!   "-0", nothing that depends on the locale.
//! - **y is flipped** by writing the page height less y, exact on the grid, so the file's numbers
//!   are page coordinates, with no transform. After the flip an anticlockwise arc turns the
//!   negative way, so its sweep flag is 0.
//! - **Curves:** lines `L`, circular arcs `A` in the grid's pieces of at most a quarter turn (a half
//!   turn's centre is ill-conditioned once its ends are rounded), cubics `C`, parabolas `Q`.
//! - **Paint:** strokes `stroke-width`, with butt caps, miter joins and a miter limit of 4 on the
//!   root; fills black, even-odd. Text is its glyph outlines, one `<path>` a run, filled black
//!   nonzero (SVG's default), so it looks the same without the font and never reflows. One `<path>`
//!   per item, so each stays one object in an editor.
//! - **Groups** are `<g data-object="…" data-kind="…">`: object ids repeat and start with a digit,
//!   so they can't be XML ids. No background: the page stays transparent, as cutters want.

use std::fmt::Write;

use solidcraft_geom::paper::{FillRule, Grid, GridPath, GridPoint, GridReport, Paint, Piece, Prepared};

use crate::{IoError, MAX_DRAWING_BYTES, Result};

/// What writing a scene did that the scene didn't say: the grid's report.
pub type SvgReport = GridReport;

/// A scene's SVG document, and what writing it did.
pub fn paper_svg(scene: &Prepared) -> Result<(Vec<u8>, SvgReport)> {
    let on = scene.on_grid(Grid::NANOMETRE).map_err(|e| IoError::Invalid(e.to_string()))?;
    let mut w = Writer { out: String::new(), height: on.size.y };
    let (width, height) = (number(on.size.x), number(on.size.y));
    w.out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(
        w.out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}mm\" height=\"{height}mm\" viewBox=\"0 0 {width} {height}\" \
         fill=\"none\" stroke=\"#000\" stroke-linecap=\"butt\" stroke-linejoin=\"miter\" stroke-miterlimit=\"4\">"
    );
    for group in &on.groups {
        let kind = serde_json::to_value(group.kind).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        let _ = writeln!(w.out, "<g data-object=\"{}\" data-kind=\"{kind}\">", group.object);
        for item in &group.items {
            match item.paint() {
                Paint::Stroke { width, path } => {
                    w.out.push_str("<path stroke-width=\"");
                    push_number(&mut w.out, width);
                    w.out.push_str("\" d=\"");
                    w.path(path);
                }
                Paint::Fill { loops, rule } => {
                    // Nonzero is SVG's default.
                    let rule = if rule == FillRule::EvenOdd { " fill-rule=\"evenodd\"" } else { "" };
                    let _ = write!(w.out, "<path fill=\"#000\" stroke=\"none\"{rule} d=\"");
                    loops.iter().for_each(|l| w.path(l));
                }
            }
            w.out.push_str("\"/>\n");
            if w.out.len() > MAX_DRAWING_BYTES {
                return Err(IoError::Invalid(format!("the SVG would be over {} MiB", MAX_DRAWING_BYTES >> 20)));
            }
        }
        w.out.push_str("</g>\n");
    }
    w.out.push_str("</svg>\n");
    Ok((w.out.into_bytes(), on.report))
}

/// Nanometres written as millimetres onto `out`: the integer part, then up to six decimals
/// without trailing zeros.
fn push_number(out: &mut String, n: i64) {
    let (sign, n) = if n < 0 { ("-", n.unsigned_abs()) } else { ("", n.unsigned_abs()) };
    let (whole, mut part, mut digits) = (n / 1_000_000, n % 1_000_000, 6);
    let _ = write!(out, "{sign}{whole}");
    if part != 0 {
        while part % 10 == 0 {
            (part, digits) = (part / 10, digits - 1);
        }
        let _ = write!(out, ".{part:0digits$}");
    }
}

/// [`push_number`] as its own text, for the numbers written once per document.
fn number(n: i64) -> String {
    let mut text = String::new();
    push_number(&mut text, n);
    text
}

struct Writer {
    out: String,
    /// The page height, in nanometres: y is written as this less y.
    height: i64,
}

impl Writer {
    fn point(&mut self, p: GridPoint) {
        push_number(&mut self.out, p.x);
        self.out.push(' ');
        push_number(&mut self.out, self.height - p.y);
    }

    /// A path's `d`: a closed one ends with `Z`, which draws its closing line.
    fn path(&mut self, path: &GridPath) {
        self.out.push('M');
        self.point(path.start);
        for piece in &path.pieces {
            match *piece {
                Piece::Line { to } => {
                    self.out.push('L');
                    self.point(to);
                }
                Piece::Arc { radius, clockwise, to } => {
                    self.out.push('A');
                    push_number(&mut self.out, radius);
                    self.out.push(' ');
                    push_number(&mut self.out, radius);
                    let _ = write!(self.out, " 0 0 {} ", u8::from(clockwise));
                    self.point(to);
                }
                Piece::Cubic { c1, c2, to } => {
                    self.out.push('C');
                    self.point(c1);
                    self.out.push(' ');
                    self.point(c2);
                    self.out.push(' ');
                    self.point(to);
                }
                Piece::Quadratic { c, to } => {
                    self.out.push('Q');
                    self.point(c);
                    self.out.push(' ');
                    self.point(to);
                }
            }
        }
        if path.closed {
            self.out.push('Z');
        }
    }
}

#[cfg(test)]
#[path = "svg_tests.rs"]
mod tests;
