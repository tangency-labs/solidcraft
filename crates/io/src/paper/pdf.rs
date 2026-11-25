//! A paper scene as a vector PDF (#4890, P1): one page the sheet's size, its lines, fills and text,
//! and optional-content layers; several scenes as the pages of one document ([`paper_pdf_pages`]),
//! each its sheet's size, sharing one set of layers. Given their pixels ([`paper_pdf_pages_with`]),
//! a sheet's pictures (shaded views, images) are drawn too.
//!
//! - **The file** comes from `pdf-writer`: a catalog, one page whose MediaBox is the sheet in
//!   points, its content deflated, and an Info dictionary with the Producer, the Title when there
//!   is one (ISO 32000-1 14.3.3: leave out what isn't known) and dates only from
//!   [`PdfOptions::output_time`], never the clock, so a scene always gives the same bytes.
//! - **Numbers** are the scene on a 10 nm grid ([`PER_MM`] steps a millimetre, whose walk gives the
//!   joints, closures and merges SVG and PNG draw), written as millimetres with up to five
//!   decimals (Annex C's precision) inside one `cm` that scales millimetres to points. y is up on
//!   the page as in the scene, so nothing flips.
//! - **Curves:** lines `l` and cubics `c` as they are; a parabola raised to a cubic; an arc's
//!   pieces (each about a quarter turn, never a half) cut into cubics that stay within [`ARC_TOLERANCE_MM`],
//!   their rounding onto the grid included.
//! - **Paint:** strokes at their width `w` (the walk never gives 0, which 8.4.3.2 says is a device
//!   pixel and shouldn't be used), butt caps, miter joins and a miter limit of 4, dashed by `d`;
//!   fills black, even-odd (`f*`). Black on no background, as SVG.
//! - **Text** is real text by default ([`PdfText::Text`], `text.rs`): each run is set with `Tj` in an
//!   embedded Type 3 font made of its glyph outlines, with a `ToUnicode` map, so it looks the same,
//!   never reflows, and a viewer selects, searches and extracts it. With [`PdfText::Outlines`] it is
//!   its glyph outlines, filled nonzero (`f`) as SVG fills them, and carries no text.
//! - **Pictures** are image XObjects: their own pixel grid ([`Picture::grid`]) at the resolution the
//!   caller gives, cropped to the pixels that show, RGB deflated with their alpha as a soft mask
//!   (left out when every pixel is opaque), placed on the lines' grid so they register, an opacity
//!   below 1 as an ExtGState's `ca`, and cut by their clip (`W n`). Without pixels
//!   ([`paper_pdf_pages`]) they are passed over and the page shows the rest.
//! - **Printing:** with [`PdfOptions::actual_size`] the catalog's viewer preferences ask for
//!   `/PrintScaling /None`, so a viewer that honours them prints the sheet at its true size.
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

use std::collections::BTreeMap;
use std::f64::consts::FRAC_PI_2;
use std::io::Write as _;

use flate2::Compression;
use flate2::write::ZlibEncoder;
use pdf_writer::{Date, Filter, Name, Pdf, Rect, Ref, TextStr};
use solidcore_record::{Seg2, Vec2};
use solidcraft_geom::Aabb2;
use solidcraft_geom::paper::{
    Clip, Grid, GridDashes, GridGroup, GridItem, GridPath, GridPoint, GridReport, Group, Item, ObjectKind, Picture, Pictures, Piece, Prepared,
};
use solidcraft_geom::pixel_grid::{GridWindow, PixelGrid};
use solidcraft_geom::prelude::*;

use self::text::{PageFonts, RunText, TextBook};
use super::TEN_NANOMETRES;
use super::flatten::{seg, vec};
use super::png::{BAND_BYTES, PICTURE_DPI, ask};
use crate::{IoError, MAX_DRAWING_BYTES, Result};

mod text;

/// The control points of the cubic that is the parabola from `a` to `b` with control point `c`:
/// two thirds of the way from each end to `c`. The one place this is worked out, for the lines and
/// for glyph outlines (`text.rs`).
fn quadratic_controls(a: Vec2, c: Vec2, b: Vec2) -> (Vec2, Vec2) {
    (a + (c - a) * (2.0 / 3.0), b + (c - b) * (2.0 / 3.0))
}

/// How far an arc's cubics may stray from the arc, rounding onto the grid included.
pub const ARC_TOLERANCE_MM: f64 = 1e-4;
/// Points a millimetre.
const PT_PER_MM: f64 = 72.0 / 25.4;
/// The smallest page side, in points (Annex C).
const MIN_PAGE_PT: f64 = 3.0;
/// The most pages a sheet PDF may have: a drawing's sheets.
const MAX_PAGES: usize = solidcraft_doc::drawing::MAX_SHEETS;
/// The most characters a title may have.
const MAX_TITLE_CHARS: usize = 1000;
/// The last second a PDF date can hold: 9999-12-31 23:59:59 UTC.
const MAX_OUTPUT_TIME: u64 = 253_402_300_799;
/// Steps a millimetre on the grid: 10 nm steps, five decimals (Annex C's precision).
const PER_MM: f64 = TEN_NANOMETRES.per_mm();

/// The grid the scene is written on.
fn grid() -> Result<Grid> {
    TEN_NANOMETRES.grid()
}

/// A point in millimetres on the lines' grid.
fn grid_point(p: Vec2) -> Result<GridPoint> {
    grid()?.point(p).map_err(|e| IoError::Invalid(e.to_string()))
}

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
    /// Ask a viewer to print at actual size, never shrunk to fit (`/PrintScaling /None`; default
    /// false).
    pub actual_size: bool,
    /// How text is written (default real text).
    pub text: PdfText,
}

/// How a sheet PDF carries its text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PdfText {
    /// Real text in an embedded font, with a `ToUnicode` map: it looks as the outlines do and can be
    /// selected, searched and copied.
    #[default]
    Text,
    /// Glyph outlines only, as before: the file holds no text.
    Outlines,
}

impl Default for PdfOptions {
    fn default() -> Self {
        Self { title: None, output_time: None, layers: true, actual_size: false, text: PdfText::Text }
    }
}

/// What writing a scene did that the scene didn't say: the grid's report.
pub type PdfReport = GridReport;

/// A scene's PDF document, and what writing it did: [`paper_pdf_pages`] with one page.
pub fn paper_pdf(scene: &Prepared, options: &PdfOptions) -> Result<(Vec<u8>, PdfReport)> {
    let (bytes, mut reports) = paper_pdf_pages(std::slice::from_ref(scene), options)?;
    let report = reports.pop().ok_or_else(|| IoError::Invalid("the sheet PDF has no page".into()))?;
    Ok((bytes, report))
}

/// Where a PDF's pictures get their pixels: one source for each page, in page order, and the
/// resolution they are embedded at, in dots per inch ([`PICTURE_DPI`]).
#[derive(Clone, Copy)]
pub struct PdfPictures<'a> {
    pub pages: &'a [&'a dyn Pictures],
    pub dpi: f64,
}

/// Scenes as the pages of one PDF document, in order, each page its scene's size, and what
/// writing each page did. The pages share one allowance of content, [`MAX_DRAWING_BYTES`]. Their
/// pictures are passed over: [`paper_pdf_pages_with`] draws them.
pub fn paper_pdf_pages(scenes: &[Prepared], options: &PdfOptions) -> Result<(Vec<u8>, Vec<PdfReport>)> {
    paper_pdf_pages_with(scenes, options, None)
}

/// [`paper_pdf_pages`], each page's pictures drawn from its own source in `pictures`. The pictures'
/// deflated pixels count against the same allowance as the pages' content.
pub fn paper_pdf_pages_with(scenes: &[Prepared], options: &PdfOptions, pictures: Option<PdfPictures<'_>>) -> Result<(Vec<u8>, Vec<PdfReport>)> {
    let title = options.title.as_deref().filter(|t| !t.is_empty());
    if title.is_some_and(|t| t.chars().count() > MAX_TITLE_CHARS) {
        return Err(IoError::Invalid(format!("a sheet PDF's title is at most {MAX_TITLE_CHARS} characters")));
    }
    if !(1..=MAX_PAGES).contains(&scenes.len()) {
        return Err(IoError::Invalid(format!("a sheet PDF has 1 to {MAX_PAGES} pages")));
    }
    if let Some(pictures) = pictures {
        if pictures.pages.len() != scenes.len() {
            return Err(IoError::Invalid("a sheet PDF needs one picture source for each page".into()));
        }
        if !PICTURE_DPI.contains(&pictures.dpi) {
            return Err(IoError::Invalid(format!("a PDF's pictures are 25 to 1200 dpi, not {}", pictures.dpi)));
        }
    }
    let draws_pictures = pictures.is_some();
    let date = options.output_time.map(date).transpose()?;
    let grid = grid()?;
    // The pages' sizes from the scenes, as the grid will make them. Each page is put on the grid
    // only when it is written, so one page's grid is held at a time.
    let mut sizes = Vec::with_capacity(scenes.len());
    for scene in scenes {
        let size = grid.point(scene.size_mm()).map_err(|e| IoError::Invalid(e.to_string()))?;
        let page = [size.x, size.y].map(|n| n as f64 / PER_MM * PT_PER_MM);
        if page.iter().any(|side| *side < MIN_PAGE_PT) {
            return Err(IoError::Invalid(format!("a sheet PDF's page is at least {MIN_PAGE_PT} pt (about 1.06 mm) a side")));
        }
        sizes.push(page);
    }

    // Object numbers in the order the objects are written: the catalog, the page tree, each page
    // and its content, the Info dictionary, then the layers. One page keeps 1 to 5 and layers
    // from 6.
    let mut refs = Refs(1);
    let (catalog, tree) = (refs.next()?, refs.next()?);
    let mut pages = Vec::with_capacity(sizes.len());
    for _ in &sizes {
        pages.push((refs.next()?, refs.next()?));
    }
    let info = refs.next()?;
    let painted_kinds = scenes.iter().flat_map(|scene| scene.groups()).filter(|group| painted(group, draws_pictures)).map(|group| group.kind);
    let layers = if options.layers { collect_layers(painted_kinds, &mut refs)? } else { Vec::new() };

    let mut pdf = Pdf::new();
    {
        let mut root = pdf.catalog(catalog);
        root.pages(tree);
        if options.actual_size {
            // A drawing prints at its true size: a viewer that honours this starts its print
            // dialog at actual size, not shrunk to fit (ISO 32000-1, 12.2, table 150).
            root.viewer_preferences().pair(Name(b"PrintScaling"), Name(b"None"));
        }
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
    let count = i32::try_from(pages.len()).map_err(|_| IoError::Invalid("too many PDF pages".into()))?;
    pdf.pages(tree).kids(pages.iter().map(|(page, _)| *page)).count(count);
    let (mut used, mut reports) = (0usize, Vec::with_capacity(scenes.len()));
    let mut book = TextBook::default();
    for (index, ((scene, page), (page_id, contents))) in scenes.iter().zip(sizes).zip(pages).enumerate() {
        // This page alone on the grid, written, then dropped. The grid keeps one group for each of
        // the scene's, in order, so the scene says which of them are painted.
        let on = scene.on_grid(grid).map_err(|e| IoError::Invalid(e.to_string()))?;
        if on.groups.len() != scene.groups().len() {
            return Err(IoError::Invalid("the sheet PDF: a page's groups changed on the grid".into()));
        }
        let groups = scene.groups().iter().zip(&on.groups).filter(|(group, _)| painted(group, draws_pictures)).map(|(_, on)| on);
        let source = pictures.and_then(|p| p.pages.get(index).map(|page| (*page, p.dpi)));
        let mut page_fonts = PageFonts::new();
        let mut runs = BTreeMap::new();
        if options.text == PdfText::Text {
            let painted_groups = scene.groups().iter().filter(|group| painted(group, draws_pictures));
            for (gi, group) in painted_groups.enumerate() {
                for (ii, item) in group.items.iter().enumerate() {
                    let Item::Text(run) = item else { continue };
                    let font = scene.font(run).ok_or_else(|| IoError::Invalid("the sheet PDF: a run's font is not in the scene".into()))?;
                    runs.insert((gi, ii), book.set(&mut refs, font, run, &mut page_fonts)?);
                }
            }
        }
        let (content, images) =
            page_content(groups, options.layers.then_some(layers.as_slice()), source, Some(&runs), &mut refs, MAX_DRAWING_BYTES - used)?;
        used += content.len() + images.iter().map(Image::bytes).sum::<usize>();
        let mut deflate = ZlibEncoder::new(Vec::new(), Compression::default());
        let compressed = deflate.write_all(content.as_bytes()).and_then(|()| deflate.finish());
        let compressed = compressed.map_err(|e| IoError::Invalid(format!("the sheet PDF: {e}")))?;
        let mut writer = pdf.page(page_id);
        writer.parent(tree).media_box(Rect::new(0.0, 0.0, page[0] as f32, page[1] as f32)).contents(contents);
        {
            // This page's layers: the kinds it paints.
            let mut resources = writer.resources();
            let mut on_page =
                layers.iter().filter(|layer| scene.groups().iter().any(|g| painted(g, draws_pictures) && g.kind == layer.kind)).peekable();
            if on_page.peek().is_some() {
                let mut properties = resources.insert(Name(b"Properties")).dict();
                for layer in on_page {
                    properties.pair(Name(layer.tag.as_bytes()), layer.reference);
                }
            }
            if !page_fonts.is_empty() {
                let mut fonts = resources.fonts();
                for at in page_fonts.keys() {
                    if let Some((name, reference)) = book.font(*at) {
                        fonts.pair(Name(name.as_bytes()), reference);
                    }
                }
            }
            if !images.is_empty() {
                let mut objects = resources.x_objects();
                for image in &images {
                    objects.pair(Name(image.name.as_bytes()), image.reference);
                }
            }
            if images.iter().any(|image| image.opacity.is_some()) {
                let mut states = resources.ext_g_states();
                for image in images.iter().filter(|image| image.opacity.is_some()) {
                    states.pair(Name(image.state_name.as_bytes()), image.state);
                }
            }
        }
        drop(writer);
        pdf.stream(contents, &compressed).filter(Filter::FlateDecode);
        for image in images {
            let mut x = pdf.image_xobject(image.reference, &image.rgb);
            x.filter(Filter::FlateDecode);
            x.width(image.size[0]).height(image.size[1]).bits_per_component(8);
            x.color_space().device_rgb();
            if image.alpha.is_some() {
                x.s_mask(image.mask);
            }
            drop(x);
            if let Some(alpha) = &image.alpha {
                let mut mask = pdf.image_xobject(image.mask, alpha);
                mask.filter(Filter::FlateDecode);
                mask.width(image.size[0]).height(image.size[1]).bits_per_component(8);
                mask.color_space().device_gray();
            }
            if let Some(opacity) = image.opacity {
                pdf.ext_graphics(image.state).non_stroking_alpha(opacity);
            }
        }
        reports.push(on.report);
    }
    let mut doc = pdf.document_info(info);
    doc.producer(TextStr(concat!("SolidCraft ", env!("CARGO_PKG_VERSION"))));
    if let Some(title) = title {
        doc.title(TextStr(title));
    }
    if let Some(date) = date {
        doc.creation_date(date).modified_date(date);
    }
    drop(doc);
    book.write(&mut pdf, &mut refs)?;
    for layer in &layers {
        let mut group = pdf.indirect(layer.reference).dict();
        group.pair(Name(b"Type"), Name(b"OCG"));
        group.pair(Name(b"Name"), TextStr(layer.kind.layer()));
    }
    Ok((pdf.finish(), reports))
}

/// The next object numbers of one document, each handed out once (pdf-writer panics on a number
/// written twice).
struct Refs(i32);

impl Refs {
    fn next(&mut self) -> Result<Ref> {
        let next = self.0.checked_add(1).ok_or_else(|| IoError::Invalid("too many PDF objects".into()))?;
        Ok(Ref::new(std::mem::replace(&mut self.0, next)))
    }
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

/// The layers of the painted groups' `kinds` (every page's, in page order): one a kind, numbered
/// from `refs` and tagged `L0`, `L1`… as the kinds are first painted, so a kind is the same layer on
/// every page.
fn collect_layers(kinds: impl IntoIterator<Item = ObjectKind>, refs: &mut Refs) -> Result<Vec<Layer>> {
    let mut layers: Vec<Layer> = Vec::new();
    for kind in kinds {
        if layers.iter().any(|layer| layer.kind == kind) {
            continue;
        }
        layers.try_reserve(1).map_err(|_| IoError::Invalid("the sheet PDF: cannot allocate its layer list".into()))?;
        layers.push(Layer { kind, reference: refs.next()?, tag: format!("L{}", layers.len()) });
    }
    Ok(layers)
}

/// Whether `group` puts any mark on this PDF's page: a stroke, a fill or text, or a picture when
/// the PDF `draws_pictures`. A group with none invents no layer and doesn't interrupt a run. Asked
/// of the prepared scene, whose items each stay one on the grid.
fn painted(group: &Group, draws_pictures: bool) -> bool {
    group.items.iter().any(|item| match item {
        Item::Stroke { .. } | Item::Fill { .. } | Item::Text(_) => true,
        Item::Picture(_) => draws_pictures,
    })
}

/// A picture drawn on a page, as its image XObject (and soft mask) and ExtGState will be written.
struct Image {
    name: String,
    reference: Ref,
    mask: Ref,
    state: Ref,
    state_name: String,
    size: [i32; 2],
    /// The pixels' RGB, deflated.
    rgb: Vec<u8>,
    /// Their alpha, deflated: none when every pixel is opaque.
    alpha: Option<Vec<u8>>,
    opacity: Option<f32>,
}

impl Image {
    fn bytes(&self) -> usize {
        self.rgb.len() + self.alpha.as_ref().map_or(0, Vec::len)
    }
}

/// The page's content stream: the scene in millimetres, scaled to points once.
#[cfg(test)]
fn content(groups: &[GridGroup]) -> Result<String> {
    content_with_limit(groups, MAX_DRAWING_BYTES)
}

// The private seam permits smaller test limits, never a higher production cap. The tests give it
// painted groups only.
#[cfg(test)]
fn content_with_limit(groups: &[GridGroup], max_bytes: usize) -> Result<String> {
    // Numbered as a one-page document's, from 6.
    let mut refs = Refs(6);
    let layers = collect_layers(groups.iter().map(|group| group.kind), &mut refs)?;
    Ok(page_content(groups, Some(&layers), None, None, &mut refs, max_bytes)?.0)
}

/// [`page_content`] without pictures, for the tests.
#[cfg(test)]
fn content_with_limit_and_layers<'a>(groups: impl IntoIterator<Item = &'a GridGroup>, layers: Option<&[Layer]>, max_bytes: usize) -> Result<String> {
    Ok(page_content(groups, layers, None, None, &mut Refs(6), max_bytes)?.0)
}

/// The content of a page's painted groups ([`painted`]), in paint order, and the pictures it
/// draws from `pictures`, numbered from `refs`. Without `pictures` a picture is passed over.
fn page_content<'a>(
    groups: impl IntoIterator<Item = &'a GridGroup>,
    layers: Option<&[Layer]>,
    pictures: Option<(&dyn Pictures, f64)>,
    runs: Option<&BTreeMap<(usize, usize), RunText>>,
    refs: &mut Refs,
    max_bytes: usize,
) -> Result<(String, Vec<Image>)> {
    let mut w = Content::new(max_bytes)?;
    let mut images: Vec<Image> = Vec::new();
    w.append(&format!("q {PT_PER_MM:.9} 0 0 {PT_PER_MM:.9} 0 0 cm 0 J 0 j 4 M\n"))?;
    let mut active_kind = None;
    for (gi, group) in groups.into_iter().enumerate() {
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
                GridItem::Text { item, loops } => {
                    let set = runs.and_then(|runs| runs.get(&(gi, *item)));
                    if set.is_none_or(|set| set.outlines) {
                        for l in loops {
                            w.path(l)?;
                            w.append("h\n")?;
                        }
                        w.append("f\n")?;
                    }
                    if let Some(set) = set {
                        w.append(&set.content)?;
                    }
                }
                GridItem::Picture { picture, clip, visible } => {
                    let Some((source, dpi)) = pictures else { continue };
                    let room = w.max_bytes.saturating_sub(w.out.len()).saturating_sub(images.iter().map(Image::bytes).sum());
                    if let Some(image) = w.picture(picture, clip.as_ref(), *visible, dpi, source, images.len(), refs, room)? {
                        images.try_reserve(1).map_err(|_| IoError::Invalid("the sheet PDF: cannot allocate its picture list".into()))?;
                        images.push(image);
                    }
                }
            }
        }
    }
    if active_kind.is_some() {
        w.append("EMC\n")?;
    }
    w.append("Q\n")?;
    if w.out.len() + images.iter().map(Image::bytes).sum::<usize>() > w.max_bytes {
        return Err(IoError::Invalid(format!("the PDF's drawing would be over {} bytes", w.max_bytes)));
    }
    Ok((w.out, images))
}

/// A picture's pixels for the columns and rows of `window`, split into deflated RGB and alpha,
/// asked for [`BAND_BYTES`] at a time and refused once together they pass `max_bytes`. The alpha
/// is none when every pixel is opaque.
fn picture_pixels(
    source: u64,
    grid: &PixelGrid,
    window: GridWindow,
    pictures: &dyn Pictures,
    max_bytes: usize,
) -> Result<(Vec<u8>, Option<Vec<u8>>)> {
    let failed = |e: std::io::Error| IoError::Invalid(format!("picture {source}: {e}"));
    let width = window.x1.saturating_sub(window.x0) as usize;
    let rows = (BAND_BYTES / (width * 4).max(1)).max(1) as u32;
    let (mut rgb, mut alpha) = (ZlibEncoder::new(Vec::new(), Compression::default()), ZlibEncoder::new(Vec::new(), Compression::default()));
    let mut opaque = true;
    let mut top = window.y0;
    while top < window.y1 {
        let band = GridWindow { y0: top, y1: window.y1.min(top.saturating_add(rows)), ..window };
        let rgba = ask(pictures, source, grid, band)?;
        let mut colour = Vec::new();
        let mut cover = Vec::new();
        colour.try_reserve_exact(rgba.len() / 4 * 3).map_err(|_| IoError::Invalid(format!("picture {source}: cannot allocate its pixels")))?;
        cover.try_reserve_exact(rgba.len() / 4).map_err(|_| IoError::Invalid(format!("picture {source}: cannot allocate its pixels")))?;
        for pixel in rgba.as_chunks::<4>().0 {
            colour.extend_from_slice(&pixel[..3]);
            cover.push(pixel[3]);
            opaque &= pixel[3] == u8::MAX;
        }
        rgb.write_all(&colour).map_err(failed)?;
        alpha.write_all(&cover).map_err(failed)?;
        if rgb.get_ref().len() + alpha.get_ref().len() > max_bytes {
            return Err(IoError::Invalid(format!("picture {source}: the PDF's drawing would be over {max_bytes} bytes")));
        }
        top = band.y1;
    }
    let (rgb, alpha) = (rgb.finish().map_err(failed)?, alpha.finish().map_err(failed)?);
    if rgb.len() + if opaque { 0 } else { alpha.len() } > max_bytes {
        return Err(IoError::Invalid(format!("picture {source}: the PDF's drawing would be over {max_bytes} bytes")));
    }
    Ok((rgb, (!opaque).then_some(alpha)))
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
        TEN_NANOMETRES.push(&mut self.number_text, n);
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

    /// Picture `picture` drawn as image `index`: the pixels of its grid at `dpi` that meet `area`,
    /// inside its `clip`, numbered from `refs`. None, with nothing written, when no pixel does.
    #[allow(clippy::too_many_arguments)]
    fn picture(
        &mut self,
        picture: &Picture,
        clip: Option<&Clip>,
        area: Aabb2,
        dpi: f64,
        pictures: &dyn Pictures,
        index: usize,
        refs: &mut Refs,
        room: usize,
    ) -> Result<Option<Image>> {
        let source = picture.source;
        let grid = picture.grid(dpi).map_err(|e| IoError::Invalid(format!("picture {source}: {e}")))?;
        let Some(window) = grid.window(area) else {
            return Ok(None);
        };
        let size = [window.x1.saturating_sub(window.x0), window.y1.saturating_sub(window.y0)];
        let size = size.map(|n| i32::try_from(n).unwrap_or(i32::MAX));
        // The window's outer pixel edges, on the lines' grid, as SVG places them.
        let on_lines = |p: Vec2| grid_point(p);
        let low = on_lines(Vec2::new(grid.column(window.x0).0, grid.row(window.y1 - 1).0))?;
        let high = on_lines(Vec2::new(grid.column(window.x1 - 1).1, grid.row(window.y0).1))?;
        let (rgb, alpha) = picture_pixels(source, &grid, window, pictures, room)?;
        let opacity = (picture.opacity < 1.0).then_some(picture.opacity as f32);
        let image = Image {
            name: format!("Im{index}"),
            reference: refs.next()?,
            mask: refs.next()?,
            state: refs.next()?,
            state_name: format!("GS{index}"),
            size,
            rgb,
            alpha,
            opacity,
        };
        self.append("q\n")?;
        if let Some(clip) = clip {
            self.clip(clip)?;
        }
        if opacity.is_some() {
            self.append("/")?;
            self.append(&image.state_name)?;
            self.append(" gs\n")?;
        }
        self.number(high.x - low.x)?;
        self.append(" 0 0 ")?;
        self.number(high.y - low.y)?;
        self.append(" ")?;
        self.point(low)?;
        self.append("cm\n/")?;
        self.append(&image.name)?;
        self.append(" Do\nQ\n")?;
        Ok(Some(image))
    }

    /// `clip` as the clipping path: `re` for a rectangle, two half-turn arcs for a circle, then `W n`.
    fn clip(&mut self, clip: &Clip) -> Result<()> {
        match *clip {
            Clip::Rect { min, max } => {
                let (low, high) = (grid_point(min)?, grid_point(max)?);
                self.point(low)?;
                self.number(high.x - low.x)?;
                self.append(" ")?;
                self.number(high.y - low.y)?;
                self.append(" re\n")?;
            }
            Clip::Circle { center, radius } => {
                let at = grid_point(center)?;
                let r = grid()?.steps(radius).map_err(|e| IoError::Invalid(e.to_string()))?;
                let centre = Vec2::new(at.x as f64, at.y as f64);
                let (east, west) = (GridPoint { x: at.x + r, y: at.y }, GridPoint { x: at.x - r, y: at.y });
                self.point(east)?;
                self.append("m\n")?;
                let half = |start: f64| Seg2::Arc { center: centre, radius: r as f64, start, sweep: std::f64::consts::PI };
                self.arc(half(0.0), west)?;
                self.arc(half(std::f64::consts::PI), east)?;
                self.append("h\n")?;
            }
        }
        self.append("W n\n")
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
                    let (c1, c2) = quadratic_controls(vec(at), vec(c), vec(to));
                    self.near(c1)?;
                    self.near(c2)?;
                    self.point(to)?;
                    self.append("c\n")?;
                }
                Piece::Arc { .. } => self.arc(seg(piece, at), piece.to())?,
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

/// How many equal cubics keep an arc of `radius` steps and `sweep` (at most a half turn, see
/// [`Piece::Arc`]) within the tolerance, each spanning at most a quarter turn. The 4/3·tan(θ/4)
/// cubic of a θ arc strays at most r·(2/27)·sin⁶(θ/4)/cos²(θ/4) from it, outward; each control
/// point rounded onto the grid moves the curve at most half a step's diagonal more.
fn cubics_for(radius: f64, sweep: f64) -> Option<usize> {
    let tolerance = ARC_TOLERANCE_MM * PER_MM - std::f64::consts::FRAC_1_SQRT_2;
    let strays = |theta: f64| radius * (2.0 / 27.0) * (theta / 4.0).sin().powi(6) / (theta / 4.0).cos().powi(2);
    // A quarter turn or less starts at one cubic, as it always has, so those arcs write the same bytes.
    let least = (sweep.abs() / FRAC_PI_2 - 1e-9).ceil().max(1.0);
    (sweep.is_finite() && sweep.abs() <= std::f64::consts::PI + 1e-9 && radius.is_finite())
        .then(|| (least as usize..=128).find(|&n| strays(sweep.abs() / n as f64) <= tolerance))
        .flatten()
}

#[cfg(test)]
#[path = "pdf_tests.rs"]
mod tests;
