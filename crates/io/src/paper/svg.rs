//! A paper scene as SVG (#4890, A3): one user unit a millimetre, the paper's size in mm, and the
//! scene's own curves where SVG has them.
//!
//! - **Numbers** are whole nanometres, written as millimetres with up to six decimals: no exponent, no "-0",
//!   nothing that depends on the locale. They are also the writer's state, so a joint is written
//!   once and a closed path ends exactly on its first point.
//! - **y is flipped** by writing the page height less y, so the file's numbers are page
//!   coordinates, with no transform. After the flip an anticlockwise arc turns the negative way,
//!   so its sweep flag is 0.
//! - **Curves:** lines `L`, circular arcs `A` in pieces of at most a quarter turn (a half turn's
//!   centre is ill-conditioned once its ends are rounded), cubics `C`, parabolas `Q`. Ellipses,
//!   hyperbolas and Béziers above degree 3 aren't written yet (A3b): a scene with one is refused.
//! - **Paint:** strokes `stroke-width`, with butt caps, miter joins and a miter limit of 4 on the
//!   root; fills black, even-odd. One `<path>` per item, so each stays one object in an editor.
//! - **Groups** are `<g data-object="…" data-kind="…">`: object ids repeat and start with a digit,
//!   so they can't be XML ids. No background: the page stays transparent, as cutters want.
//!
//! What the grid can't express is counted, never an error: a width under half a nanometre is
//! written as one, and a piece ending within 10 nanometres of where the path is goes straight on
//! to the next.

use std::fmt::Write;

use solidcraft_geom::paper::{Item, Prepared, arc_pieces};
use solidcraft_geom::{Region2, Seg2, Vec2};

use crate::{IoError, MAX_DRAWING_BYTES, Result};

/// The largest number written, in millimetres either way: far past any scene's 2e5.
const MAX_NUMBER_MM: f64 = 9e9;
/// Pieces ending this many grid steps or fewer from where the path is are merged (1e-5 mm).
const SLIVER_STEPS: i64 = 10;

/// What writing a scene did that the scene didn't say.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SvgReport {
    /// Widths under half a nanometre, written as one nanometre.
    pub widths_snapped: usize,
    /// Pieces too short for the nanometre grid, merged into the next.
    pub pieces_merged: usize,
}

/// A scene's SVG document, and what writing it did.
pub fn paper_svg(scene: &Prepared) -> Result<(Vec<u8>, SvgReport)> {
    let mut w = Writer { out: String::new(), height: nano(scene.size_mm().y)?, report: SvgReport::default(), at: (0, 0), first: (0, 0) };
    let (width, height) = (number(nano(scene.size_mm().x)?), number(w.height));
    w.out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(
        w.out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}mm\" height=\"{height}mm\" viewBox=\"0 0 {width} {height}\" \
         fill=\"none\" stroke=\"#000\" stroke-linecap=\"butt\" stroke-linejoin=\"miter\" stroke-miterlimit=\"4\">"
    );
    for group in scene.groups() {
        let kind = serde_json::to_value(group.kind).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        let _ = writeln!(w.out, "<g data-object=\"{}\" data-kind=\"{kind}\">", group.object);
        for item in &group.items {
            match item {
                Item::Stroke { path, closed, width_mm } => w.stroke(path, *closed, *width_mm)?,
                Item::Fill { region } => w.fill(region)?,
            }
            if w.out.len() > MAX_DRAWING_BYTES {
                return Err(IoError::Invalid(format!("the SVG would be over {} MiB", MAX_DRAWING_BYTES >> 20)));
            }
        }
        w.out.push_str("</g>\n");
    }
    w.out.push_str("</svg>\n");
    Ok((w.out.into_bytes(), w.report))
}

/// A value in millimetres as whole nanometres.
fn nano(v: f64) -> Result<i64> {
    let n = (v * 1e6).round();
    if n.abs() <= MAX_NUMBER_MM * 1e6 { Ok(n as i64) } else { Err(IoError::Invalid("a number too large (or not a number) for the SVG".into())) }
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

/// [`push_number`] as its own text, for the numbers written once per document or arc.
fn number(n: i64) -> String {
    let mut text = String::new();
    push_number(&mut text, n);
    text
}

struct Writer {
    out: String,
    /// The page height, in nanometres: y is written as this less y.
    height: i64,
    report: SvgReport,
    /// Where the path is, and where its subpath started, on the grid.
    at: (i64, i64),
    first: (i64, i64),
}

impl Writer {
    /// A scene point on the page's grid.
    fn grid(&self, p: Vec2) -> Result<(i64, i64)> {
        Ok((nano(p.x)?, self.height - nano(p.y)?))
    }

    fn point(&mut self, (x, y): (i64, i64)) {
        push_number(&mut self.out, x);
        self.out.push(' ');
        push_number(&mut self.out, y);
    }

    /// Starts a subpath at `p`.
    fn move_to(&mut self, p: Vec2) -> Result<()> {
        let at = self.grid(p)?;
        self.out.push('M');
        self.point(at);
        (self.at, self.first) = (at, at);
        Ok(())
    }

    /// Whether a piece ending at `end` is too short to write: it is merged into the next. A path's
    /// last piece (`keep`) has no next, so it is written unless it has no length: an open path
    /// ends where it ends, a closed one on its first point exactly.
    fn merged(&mut self, end: (i64, i64), keep: bool) -> bool {
        if keep {
            return end == self.at;
        }
        let (dx, dy) = (end.0 - self.at.0, end.1 - self.at.1);
        let short = dx.abs() <= SLIVER_STEPS && dy.abs() <= SLIVER_STEPS && dx * dx + dy * dy <= SLIVER_STEPS * SLIVER_STEPS;
        if short {
            self.report.pieces_merged += 1;
        }
        short
    }

    /// One piece, from where the path is: `closing` makes its end the subpath's first point, and
    /// `keep` marks the path's last piece ([`Writer::merged`]).
    fn piece(&mut self, seg: &Seg2, closing: bool, keep: bool) -> Result<()> {
        let end = if closing { self.first } else { self.grid(seg.end())? };
        match *seg {
            Seg2::Line { .. } => {
                if !self.merged(end, keep) {
                    self.out.push('L');
                    self.point(end);
                    self.at = end;
                }
            }
            Seg2::Arc { center, radius, start, sweep } => {
                let r = number(nano(radius)?);
                let flag = u8::from(sweep < 0.0);
                let mut ends = arc_pieces(center, radius, start, sweep).peekable();
                while let Some(p) = ends.next() {
                    let final_piece = ends.peek().is_none();
                    let to = if final_piece { end } else { self.grid(p)? };
                    if !self.merged(to, keep && final_piece) {
                        let _ = write!(self.out, "A{r} {r} 0 0 {flag} ");
                        self.point(to);
                        self.at = to;
                    }
                }
            }
            Seg2::Cubic { p1, p2, .. } => {
                let (c1, c2) = (self.grid(p1)?, self.grid(p2)?);
                self.out.push('C');
                self.point(c1);
                self.out.push(' ');
                self.point(c2);
                self.out.push(' ');
                self.point(end);
                self.at = end;
            }
            Seg2::Conic { apex, w, .. } if (w - 1.0).abs() <= 1e-12 => {
                let c = self.grid(apex)?;
                self.out.push('Q');
                self.point(c);
                self.out.push(' ');
                self.point(end);
                self.at = end;
            }
            Seg2::Conic { .. } | Seg2::Bezier { .. } => {
                return Err(IoError::Invalid("not supported yet: SVG of ellipses, hyperbolas and Béziers above degree 3 (#4890, A3b)".into()));
            }
        }
        Ok(())
    }

    /// A path's pieces after its `M`: a closed one ends exactly on its first point, a line back to
    /// it as `Z`.
    fn pieces(&mut self, path: &[Seg2], closed: bool) -> Result<()> {
        let Some((last, body)) = path.split_last() else { return Ok(()) };
        for seg in body {
            self.piece(seg, false, false)?;
        }
        if !closed {
            return self.piece(last, false, true);
        }
        if !matches!(last, Seg2::Line { .. }) {
            self.piece(last, true, true)?;
        }
        self.out.push('Z');
        Ok(())
    }

    fn stroke(&mut self, path: &[Seg2], closed: bool, width_mm: f64) -> Result<()> {
        let Some(first) = path.first() else { return Ok(()) };
        let mut width = nano(width_mm)?;
        if width == 0 {
            width = 1;
            self.report.widths_snapped += 1;
        }
        self.out.push_str("<path stroke-width=\"");
        push_number(&mut self.out, width);
        self.out.push_str("\" d=\"");
        self.move_to(first.start())?;
        self.pieces(path, closed)?;
        self.out.push_str("\"/>\n");
        Ok(())
    }

    fn fill(&mut self, region: &Region2) -> Result<()> {
        self.out.push_str("<path fill=\"#000\" stroke=\"none\" fill-rule=\"evenodd\" d=\"");
        for l in region.loops() {
            let Some(first) = l.segs.first() else { continue };
            self.move_to(first.start())?;
            self.pieces(&l.segs, true)?;
        }
        self.out.push_str("\"/>\n");
        Ok(())
    }
}

#[cfg(test)]
#[path = "svg_tests.rs"]
mod tests;
