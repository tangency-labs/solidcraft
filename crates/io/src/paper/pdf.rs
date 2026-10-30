//! A paper scene as a vector PDF (#4890, P1): one page the sheet's size, its lines and fills,
//! geometry and optional-content layers (text follows with P2).
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
//! - **Layers:** nonempty groups share one PDF optional-content group for each
//!   distinct paper object kind. Group order remains the original paint order,
//!   and each kind's PDF layer uses ObjectKind::layer, as in DXF. Empty
//!   groups do not invent layers. Consecutive painted groups of the same kind
//!   share one marked-content run; empty groups do not interrupt it. All present
//!   layers start visible.
//!   Set [`PdfOptions::layers`] to false for the same vector marks without layers.
//! - **Content size:** every append checks the cumulative byte limit before growing the
//!   buffer, including inside a path/arc and the final graphics-state restore. Allocation
//!   failure is an error. The limit is for uncompressed content, not total export memory.

use std::f64::consts::FRAC_PI_2;
use std::io::Write as _;

use flate2::Compression;
use flate2::write::ZlibEncoder;
use pdf_writer::{Date, Filter, Name, Pdf, Rect, Ref, TextStr};
use solidcraft_geom::paper::{Grid, GridDashes, GridGroup, GridItem, GridPath, GridPoint, GridReport, ObjectKind, Piece, Prepared};
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PdfOptions {
    /// The document's title, left out when `None` or empty.
    pub title: Option<String>,
    /// When the PDF was made, in seconds since 1970 (UTC): its creation and modification dates.
    /// Without it the file has no dates.
    pub output_time: Option<u64>,
    /// Include optional-content layers by object kind (default true). False writes
    /// the same vector marks in paint order, without layer objects or markers.
    pub layers: bool,
}

impl Default for PdfOptions {
    fn default() -> Self {
        Self { title: None, output_time: None, layers: true }
    }
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

    let layers = if options.layers { collect_layers(&on.groups)? } else { Vec::new() };
    let content = content_with_limit_and_layers(&on.groups, options.layers.then_some(layers.as_slice()), MAX_DRAWING_BYTES)?;
    let mut deflate = ZlibEncoder::new(Vec::new(), Compression::default());
    let compressed = deflate.write_all(content.as_bytes()).and_then(|()| deflate.finish());
    let compressed = compressed.map_err(|e| IoError::Invalid(format!("the sheet PDF: {e}")))?;

    // Each object's number, used once (pdf-writer panics on a number written twice).
    let (catalog, tree, page_id, contents, info) = (Ref::new(1), Ref::new(2), Ref::new(3), Ref::new(4), Ref::new(5));
    let mut pdf = Pdf::new();
    {
        let mut root = pdf.catalog(catalog);
        root.pages(tree);
        if !layers.is_empty() {
            // PDF 1.5 optional content: one group per used paper object kind.
            // The default configuration orders all layers and starts them ON.
            let mut properties = root.insert(Name(b"OCProperties")).dict();
            properties.insert(Name(b"OCGs")).array().items(layers.iter().map(|l| l.reference));
            let mut default = properties.insert(Name(b"D")).dict();
            default.insert(Name(b"Order")).array().items(layers.iter().map(|l| l.reference));
            default.insert(Name(b"ON")).array().items(layers.iter().map(|l| l.reference));
            default.pair(Name(b"BaseState"), Name(b"ON"));
        }
    }
    pdf.pages(tree).kids([page_id]).count(1);
    let mut writer = pdf.page(page_id);
    writer.parent(tree).media_box(Rect::new(0.0, 0.0, page[0] as f32, page[1] as f32)).contents(contents);
    {
        let mut resources = writer.resources();
        if !layers.is_empty() {
            let mut properties = resources.insert(Name(b"Properties")).dict();
            for layer in &layers {
                properties.pair(Name(layer.tag.as_bytes()), layer.reference);
            }
        }
    }
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
    for layer in &layers {
        let mut group = pdf.indirect(layer.reference).dict();
        group.pair(Name(b"Type"), Name(b"OCG"));
        group.pair(Name(b"Name"), TextStr(layer.kind.layer()));
    }
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

/// One PDF layer for every kind with actual paper marks, in first paint order.
struct Layer {
    kind: ObjectKind,
    reference: Ref,
    tag: String,
}

fn collect_layers(groups: &[GridGroup]) -> Result<Vec<Layer>> {
    let mut layers: Vec<Layer> = Vec::new();
    for group in groups.iter().filter(|group| !group.items.is_empty()) {
        if layers.iter().any(|layer| layer.kind == group.kind) {
            continue;
        }
        layers.try_reserve(1).map_err(|_| IoError::Invalid("the sheet PDF: cannot allocate its layer list".into()))?;
        let index = i32::try_from(layers.len()).ok().and_then(|n| n.checked_add(6)).ok_or_else(|| IoError::Invalid("too many PDF layers".into()))?;
        layers.push(Layer { kind: group.kind, reference: Ref::new(index), tag: format!("L{}", layers.len()) });
    }
    Ok(layers)
}

/// The page's content stream: the scene in millimetres, scaled to points once.
#[cfg(test)]
fn content(groups: &[GridGroup]) -> Result<String> {
    content_with_limit(groups, MAX_DRAWING_BYTES)
}

// The private seam permits smaller test limits, never a higher production cap.
#[cfg(test)]
fn content_with_limit(groups: &[GridGroup], max_bytes: usize) -> Result<String> {
    let layers = collect_layers(groups)?;
    content_with_limit_and_layers(groups, Some(&layers), max_bytes)
}

fn content_with_limit_and_layers(groups: &[GridGroup], layers: Option<&[Layer]>, max_bytes: usize) -> Result<String> {
    let mut w = Content::new(max_bytes)?;
    w.append(&format!("q {PT_PER_MM:.9} 0 0 {PT_PER_MM:.9} 0 0 cm 0 J 0 j 4 M\n"))?;
    let mut active_kind = None;
    for group in groups.iter().filter(|group| !group.items.is_empty()) {
        if let Some(layers) = layers {
            let layer = layers.iter().find(|layer| layer.kind == group.kind).ok_or_else(|| IoError::Invalid("missing paper PDF layer".into()))?;
            if active_kind != Some(group.kind) {
                if active_kind.is_some() {
                    w.append("EMC\n")?;
                }
                w.append("/OC /")?;
                w.append(&layer.tag)?;
                w.append(" BDC\n")?;
                active_kind = Some(group.kind);
            }
        }
        for item in &group.items {
            match item {
                GridItem::Stroke { width, path, dashes } => {
                    if w.width != Some(*width) {
                        w.number(*width)?;
                        w.append(" w\n")?;
                        w.width = Some(*width);
                    }
                    if w.dashes.as_ref() != dashes.as_ref() {
                        w.dashes(dashes.as_ref())?;
                        w.dashes.clone_from(dashes);
                    }
                    w.path(path)?;
                    // A closed path's closing line, then the stroke: `s`.
                    w.append(if path.closed { "s\n" } else { "S\n" })?;
                }
                GridItem::Fill { loops } => {
                    for l in loops {
                        w.path(l)?;
                        w.append("h\n")?;
                    }
                    w.append("f*\n")?;
                }
            }
        }
    }
    if active_kind.is_some() {
        w.append("EMC\n")?;
    }
    w.append("Q\n")?;
    Ok(w.out)
}

// One growth path for every operand/operator. Reserve geometrically, capped at
// the allowance, rather than repeatedly reallocating for individual numbers.
fn append_bounded(out: &mut String, text: &str, max_bytes: usize) -> Result<()> {
    let length = out
        .len()
        .checked_add(text.len())
        .filter(|&n| n <= max_bytes)
        .ok_or_else(|| IoError::Invalid(format!("the PDF's drawing would be over {max_bytes} bytes")))?;
    if length > out.capacity() {
        let capacity = out.capacity().saturating_mul(2).max(length).min(max_bytes);
        out.try_reserve_exact(capacity - out.len())
            .map_err(|_| IoError::Invalid("the sheet PDF: cannot allocate its bounded content buffer".into()))?;
    }
    out.push_str(text);
    Ok(())
}

struct Content {
    out: String,
    max_bytes: usize,
    // Reuse a small buffer for the existing shared integer formatter. Any i64
    // on this grid needs at most 21 bytes, including a minus sign and point.
    number_text: String,
    /// The stroke width and dash pattern last set, so each is written only when it changes.
    width: Option<i64>,
    dashes: Option<GridDashes>,
}

impl Content {
    fn new(max_bytes: usize) -> Result<Self> {
        let mut number_text = String::new();
        number_text.try_reserve_exact(32).map_err(|_| IoError::Invalid("the sheet PDF: cannot allocate its number buffer".into()))?;
        Ok(Self { out: String::new(), max_bytes: max_bytes.min(MAX_DRAWING_BYTES), number_text, width: None, dashes: None })
    }

    fn append(&mut self, text: &str) -> Result<()> {
        append_bounded(&mut self.out, text, self.max_bytes)
    }

    fn number(&mut self, n: i64) -> Result<()> {
        self.number_text.clear();
        push_mm(&mut self.number_text, n, DECIMALS);
        append_bounded(&mut self.out, &self.number_text, self.max_bytes)
    }

    fn point(&mut self, p: GridPoint) -> Result<()> {
        self.number(p.x)?;
        self.append(" ")?;
        self.number(p.y)?;
        self.append(" ")
    }

    /// `p` in steps, rounded onto the grid.
    fn near(&mut self, p: Vec2) -> Result<()> {
        if !p.is_finite() {
            return Err(IoError::Invalid("the sheet PDF: a curve point is not a number".into()));
        }
        self.point(GridPoint { x: p.x.round() as i64, y: p.y.round() as i64 })
    }

    /// A dash pattern `[on off ...] phase d`, or a solid line `[] 0 d`.
    fn dashes(&mut self, dashes: Option<&GridDashes>) -> Result<()> {
        self.append("[")?;
        if let Some(d) = dashes {
            for (i, length) in d.lengths.iter().enumerate() {
                if i > 0 {
                    self.append(" ")?;
                }
                self.number(*length)?;
            }
        }
        self.append("] ")?;
        self.number(dashes.map_or(0, |d| d.phase))?;
        self.append(" d\n")
    }

    /// A path from its start; a closed one's closing line is left to the caller's `h` or `s`.
    fn path(&mut self, path: &GridPath) -> Result<()> {
        self.point(path.start)?;
        self.append("m\n")?;
        let mut at = path.start;
        for piece in &path.pieces {
            match *piece {
                Piece::Line { to } => {
                    self.point(to)?;
                    self.append("l\n")?;
                }
                Piece::Cubic { c1, c2, to } => {
                    self.point(c1)?;
                    self.point(c2)?;
                    self.point(to)?;
                    self.append("c\n")?;
                }
                // A parabola is the cubic with its control points two thirds of the way to `c`.
                Piece::Quadratic { c, to } => {
                    let (a, c, b) = (at.vec(), c.vec(), to.vec());
                    self.near(a + (c - a) * (2.0 / 3.0))?;
                    self.near(b + (c - b) * (2.0 / 3.0))?;
                    self.point(to)?;
                    self.append("c\n")?;
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
                self.point(to)?
            } else {
                self.near(p3)?
            }
            self.append("c\n")?;
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
