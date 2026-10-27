//! A paper scene as a vector PDF (#4890, P1): one page the sheet's size, its lines and fills,
//! geometry only (text follows with P2).
//!
//! - **The file** comes from `pdf-writer`: a catalog, one page whose MediaBox is the sheet in
//!   points, its content deflated, and an Info dictionary with the Producer, the Title when there
//!   is one (ISO 32000-1 14.3.3: leave out what isn't known) and dates only from
//!   [`PdfOptions::output_time`], never the clock, so a scene always gives the same bytes.
//! - **Numbers** are the scene on a 10 nm grid ([`Grid::TEN_NANOMETRES`], whose walk gives the
//!   joints, closures and merges SVG and PNG draw), written as millimetres with up to five
//!   decimals (Annex C's precision) inside one `cm` that scales millimetres to points. y is up on
//!   the page as in the scene, so nothing flips.
//! - **Curves:** lines `l` and cubics `c` as they are; a parabola raised to a cubic; an arc's
//!   pieces (each at most a quarter turn) cut into cubics that stay within [`ARC_TOLERANCE_MM`],
//!   their rounding onto the grid included.
//! - **Paint:** strokes at their width `w` (the walk never gives 0, which 8.4.3.2 says is a device
//!   pixel and shouldn't be used), butt caps, miter joins and a miter limit of 4, dashed by `d`;
//!   fills black, even-odd (`f*`). Black on no background, as SVG.

use std::f64::consts::FRAC_PI_2;
use std::fmt::Write as _;
use std::io::Write as _;

use flate2::Compression;
use flate2::write::ZlibEncoder;
use pdf_writer::{Date, Filter, Pdf, Rect, Ref, TextStr};
use solidcraft_geom::paper::{Grid, GridDashes, GridGroup, GridItem, GridPath, GridPoint, GridReport, Piece, Prepared};
use solidcraft_geom::{Seg2, Vec2};

use super::push_mm;
use crate::{IoError, MAX_DRAWING_BYTES, Result};

/// How far an arc's cubics may stray from the arc, rounding onto the grid included.
pub const ARC_TOLERANCE_MM: f64 = 1e-4;
/// Points a millimetre.
const PT_PER_MM: f64 = 72.0 / 25.4;
/// The smallest page side, in points (Annex C).
const MIN_PAGE_PT: f64 = 3.0;
/// The most characters a title may have.
const MAX_TITLE_CHARS: usize = 1000;
/// The last second a PDF date can hold: 9999-12-31 23:59:59 UTC.
const MAX_OUTPUT_TIME: u64 = 253_402_300_799;
/// Decimals of a millimetre on the grid.
const DECIMALS: u32 = 5;

/// What a sheet PDF says beyond the scene.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PdfOptions {
    /// The document's title, left out when `None` or empty.
    pub title: Option<String>,
    /// When the PDF was made, in seconds since 1970 (UTC): its creation and modification dates.
    /// Without it the file has no dates.
    pub output_time: Option<u64>,
}

/// What writing a scene did that the scene didn't say: the grid's report.
pub type PdfReport = GridReport;

/// A scene's PDF document, and what writing it did.
pub fn paper_pdf(scene: &Prepared, options: &PdfOptions) -> Result<(Vec<u8>, PdfReport)> {
    let title = options.title.as_deref().filter(|t| !t.is_empty());
    if title.is_some_and(|t| t.chars().count() > MAX_TITLE_CHARS) {
        return Err(IoError::Invalid(format!("a sheet PDF's title is at most {MAX_TITLE_CHARS} characters")));
    }
    let date = options.output_time.map(date).transpose()?;
    let on = scene.on_grid(Grid::TEN_NANOMETRES).map_err(|e| IoError::Invalid(e.to_string()))?;
    let page = [on.size.x, on.size.y].map(|n| n as f64 / Grid::TEN_NANOMETRES.per_mm() * PT_PER_MM);
    if page.iter().any(|side| *side < MIN_PAGE_PT) {
        return Err(IoError::Invalid(format!("a sheet PDF's page is at least {MIN_PAGE_PT} pt (about 1.06 mm) a side")));
    }

    let content = content(&on.groups)?;
    let mut deflate = ZlibEncoder::new(Vec::new(), Compression::default());
    let compressed = deflate.write_all(content.as_bytes()).and_then(|()| deflate.finish());
    let compressed = compressed.map_err(|e| IoError::Invalid(format!("the sheet PDF: {e}")))?;

    // Each object's number, used once (pdf-writer panics on a number written twice).
    let (catalog, tree, page_id, contents, info) = (Ref::new(1), Ref::new(2), Ref::new(3), Ref::new(4), Ref::new(5));
    let mut pdf = Pdf::new();
    pdf.catalog(catalog).pages(tree);
    pdf.pages(tree).kids([page_id]).count(1);
    let mut writer = pdf.page(page_id);
    writer.parent(tree).media_box(Rect::new(0.0, 0.0, page[0] as f32, page[1] as f32)).contents(contents);
    writer.resources();
    drop(writer);
    pdf.stream(contents, &compressed).filter(Filter::FlateDecode);
    let mut doc = pdf.document_info(info);
    doc.producer(TextStr(concat!("SolidCraft ", env!("CARGO_PKG_VERSION"))));
    if let Some(title) = title {
        doc.title(TextStr(title));
    }
    if let Some(date) = date {
        doc.creation_date(date).modified_date(date);
    }
    drop(doc);
    Ok((pdf.finish(), on.report))
}

/// `secs` since 1970 as a PDF date in UTC, refused past the year 9999.
fn date(secs: u64) -> Result<Date> {
    if secs > MAX_OUTPUT_TIME {
        return Err(IoError::Invalid("a sheet PDF's date is at most the year 9999".into()));
    }
    let (year, month, day, hour, minute, second) = crate::civil(secs);
    let (month, day) = (u8::try_from(month).unwrap_or(1), u8::try_from(day).unwrap_or(1));
    let [hour, minute, second] = [hour, minute, second].map(|v| u8::try_from(v).unwrap_or(0));
    Ok(Date::new(u16::try_from(year).unwrap_or(0)).month(month).day(day).hour(hour).minute(minute).second(second).utc_offset_hour(0))
}

/// The page's content stream: the scene in millimetres, scaled to points once.
fn content(groups: &[GridGroup]) -> Result<String> {
    let mut w = Content { out: String::new(), width: None, dashes: None };
    let _ = writeln!(w.out, "q {PT_PER_MM:.9} 0 0 {PT_PER_MM:.9} 0 0 cm 0 J 0 j 4 M");
    for item in groups.iter().flat_map(|g| &g.items) {
        match item {
            GridItem::Stroke { width, path, dashes } => {
                if w.width != Some(*width) {
                    w.number(*width);
                    w.out.push_str(" w\n");
                    w.width = Some(*width);
                }
                if w.dashes.as_ref() != dashes.as_ref() {
                    w.dashes(dashes.as_ref());
                    w.dashes.clone_from(dashes);
                }
                w.path(path)?;
                // A closed path's closing line, then the stroke: `s`.
                w.out.push_str(if path.closed { "s\n" } else { "S\n" });
            }
            GridItem::Fill { loops } => {
                for l in loops {
                    w.path(l)?;
                    w.out.push_str("h\n");
                }
                w.out.push_str("f*\n");
            }
        }
        if w.out.len() > MAX_DRAWING_BYTES {
            return Err(IoError::Invalid(format!("the PDF's drawing would be over {} MiB", MAX_DRAWING_BYTES >> 20)));
        }
    }
    w.out.push_str("Q\n");
    Ok(w.out)
}

struct Content {
    out: String,
    /// The stroke width and dash pattern last set, so each is written only when it changes.
    width: Option<i64>,
    dashes: Option<GridDashes>,
}

impl Content {
    fn number(&mut self, n: i64) {
        push_mm(&mut self.out, n, DECIMALS);
    }

    fn point(&mut self, p: GridPoint) {
        self.number(p.x);
        self.out.push(' ');
        self.number(p.y);
        self.out.push(' ');
    }

    /// `p` in steps, rounded onto the grid.
    fn near(&mut self, p: Vec2) -> Result<()> {
        if !p.is_finite() {
            return Err(IoError::Invalid("the sheet PDF: a curve point is not a number".into()));
        }
        self.point(GridPoint { x: p.x.round() as i64, y: p.y.round() as i64 });
        Ok(())
    }

    /// A dash pattern `[on off …] phase d`, or a solid line `[] 0 d`.
    fn dashes(&mut self, dashes: Option<&GridDashes>) {
        self.out.push('[');
        if let Some(d) = dashes {
            for (i, length) in d.lengths.iter().enumerate() {
                if i > 0 {
                    self.out.push(' ');
                }
                self.number(*length);
            }
        }
        self.out.push_str("] ");
        self.number(dashes.map_or(0, |d| d.phase));
        self.out.push_str(" d\n");
    }

    /// A path from its start; a closed one's closing line is left to the caller's `h` or `s`.
    fn path(&mut self, path: &GridPath) -> Result<()> {
        self.point(path.start);
        self.out.push_str("m\n");
        let mut at = path.start;
        for piece in &path.pieces {
            match *piece {
                Piece::Line { to } => {
                    self.point(to);
                    self.out.push_str("l\n");
                }
                Piece::Cubic { c1, c2, to } => {
                    self.point(c1);
                    self.point(c2);
                    self.point(to);
                    self.out.push_str("c\n");
                }
                // A parabola is the cubic with its control points two thirds of the way to `c`.
                Piece::Quadratic { c, to } => {
                    let (a, c, b) = (at.vec(), c.vec(), to.vec());
                    self.near(a + (c - a) * (2.0 / 3.0))?;
                    self.near(b + (c - b) * (2.0 / 3.0))?;
                    self.point(to);
                    self.out.push_str("c\n");
                }
                Piece::Arc { .. } => self.arc(piece.seg(at), piece.to())?,
            }
            at = piece.to();
        }
        Ok(())
    }

    /// An arc piece as cubics within [`ARC_TOLERANCE_MM`], ending exactly on `to`.
    fn arc(&mut self, arc: Seg2, to: GridPoint) -> Result<()> {
        let Seg2::Arc { center, radius, start, sweep } = arc else {
            return Err(IoError::Invalid("the sheet PDF: an arc piece isn't an arc".into()));
        };
        let n = cubics_for(radius, sweep).ok_or_else(|| IoError::Invalid("the sheet PDF: an arc needs too many cubics".into()))?;
        let h = sweep / n as f64;
        let k = 4.0 / 3.0 * (h / 4.0).tan() * radius;
        let tangent = |a: f64| Vec2::new(-a.sin(), a.cos());
        for i in 0..n {
            let (a0, a1) = (start + h * i as f64, start + h * (i + 1) as f64);
            let (p0, p3) = (center + Vec2::from_angle(a0) * radius, center + Vec2::from_angle(a1) * radius);
            self.near(p0 + tangent(a0) * k)?;
            self.near(p3 - tangent(a1) * k)?;
            if i + 1 == n {
                self.point(to)
            } else {
                self.near(p3)?
            }
            self.out.push_str("c\n");
        }
        Ok(())
    }
}

/// How many equal cubics keep an arc of `radius` steps and `sweep` (at most a quarter turn) within
/// the tolerance. The 4/3·tan(θ/4) cubic of a θ arc strays at most r·(2/27)·sin⁶(θ/4)/cos²(θ/4)
/// from it, outward; each control point rounded onto the grid moves the curve at most half a
/// step's diagonal more.
fn cubics_for(radius: f64, sweep: f64) -> Option<usize> {
    let tolerance = ARC_TOLERANCE_MM * Grid::TEN_NANOMETRES.per_mm() - std::f64::consts::FRAC_1_SQRT_2;
    let strays = |theta: f64| radius * (2.0 / 27.0) * (theta / 4.0).sin().powi(6) / (theta / 4.0).cos().powi(2);
    (sweep.is_finite() && sweep.abs() <= FRAC_PI_2 + 1e-9 && radius.is_finite())
        .then(|| (1..=64).find(|&n| strays(sweep.abs() / n as f64) <= tolerance))
        .flatten()
}

#[cfg(test)]
#[path = "pdf_tests.rs"]
mod tests;
