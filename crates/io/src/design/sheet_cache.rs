//! The sheet cache (#3890): each drawing's views' last lines, kept in the design file so a drawing
//! opens showing them. One entry for each drawing, `cache/drawings/<id>.json`, deflated, its id as
//! [`ids::encode`] writes it. Like the model cache it is optional: written only when the file has
//! room, read back only by the build that wrote it (a view's lines come from the projection code as
//! well as the model), and anything wrong with it is a miss, never a failed open. Whether a view's
//! lines are current is not decided here: each view says what it showed, and the engine compares
//! that with the model the design opens with.
//!
//! A shaded view's pictures go beside its entry, one PNG part each,
//! `cache/drawings/<id>/<view>-<picture>.png`, stored as they are (a PNG is deflated already). The entry
//! says what each was drawn from, and the engine decides whether it is current, as for the lines.
use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use solidcore_record::Vec2;
use solidcraft_doc::{ThreadInfo, ids};
use solidcraft_geom::Aabb2;
use solidcraft_geom::paper::{Group, MAX_SIDE_MM, Pictures, Scene};
use solidcraft_geom::pixel_grid::{GridWindow, PixelGrid};

use super::*;

const FORMAT: &str = "solidcraft-sheet-cache/1";
const FOLDER: &str = "cache/drawings/";
/// The most one drawing's entry may hold, expanded.
const MAX_ENTRY: usize = 64 << 20;
/// The most pixels one picture may hold: the sheet page's own cap on a picture.
const MAX_PICTURE_PIXELS: u64 = 16 << 20;
/// The most one picture's PNG may hold.
const MAX_PICTURE_PNG: usize = 16 << 20;

/// One view's last lines, and what they were drawn from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SheetView {
    /// The view's id.
    pub view: u64,
    /// The view as it drew, as a number its build makes: lines of a view whose settings changed
    /// since are not its.
    pub resolved: u64,
    /// What it showed, body by body.
    pub shows: Vec<ShownBody>,
    /// Its lines, centred on the view.
    pub group: Arc<Group>,
    pub boundary: Option<Aabb2>,
    /// The middle of the model's box on the view's plane: with the view's own plane and scale, the
    /// frame its lines were drawn through.
    pub model_centre: Vec2,
    /// What it says beside its lines.
    pub why: Option<String>,
    /// Its shaded pictures' pixels.
    pub pictures: Vec<SheetPixels>,
}

/// A picture's pixels as the file keeps them, and what they were drawn from: which picture of the
/// view, the bodies' looks and the sheet's dpi (with the picture's rectangle, they give its grid),
/// as numbers its build makes, and the pixel tests it cost. Its rows are a PNG part of their own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SheetPixels {
    pub source: u64,
    pub looks: u64,
    pub dpi: f64,
    pub tests: u64,
    /// Its columns and rows.
    pub size: [u32; 2],
    /// Its pixels: straight alpha, sRGB, 8 bits a channel, RGBA, top row first. Saved as a PNG part
    /// of its own; read back only when that part decodes to exactly `size`.
    #[serde(skip)]
    pub rows: Option<Arc<[u8]>>,
}

/// A drawing's entry as its file holds it, unread, and its pictures' PNG parts, by view and picture.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FiledSheet {
    pub entry: Arc<[u8]>,
    pub pictures: BTreeMap<(u64, u64), Arc<[u8]>>,
}

impl From<&[u8]> for FiledSheet {
    /// An entry with no picture parts.
    fn from(entry: &[u8]) -> FiledSheet {
        FiledSheet { entry: Arc::from(entry), pictures: BTreeMap::new() }
    }
}

/// A body a view showed: the occurrences that place it, its definition's body name, whether it is
/// a reference, its geometry's key where it stands, and its cosmetic threads.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShownBody {
    pub path: Vec<u64>,
    pub body: String,
    pub reference: bool,
    pub geometry: u64,
    pub threads: Vec<ThreadInfo>,
}

/// A drawing's lines to save: its views now, or the entry its file held, never read.
pub enum SheetLines {
    Views(Vec<SheetView>),
    Kept(FiledSheet),
}

#[derive(Serialize)]
struct Written<'a> {
    build: String,
    format: &'a str,
    views: &'a [SheetView],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    build: String,
    format: String,
    views: Vec<SheetView>,
}

fn name(drawing: u64) -> String {
    format!("{FOLDER}{}.json", ids::encode(drawing))
}

fn picture_name(drawing: u64, (view, source): (u64, u64)) -> String {
    format!("{FOLDER}{}/{}-{}.png", ids::encode(drawing), ids::encode(view), ids::encode(source))
}

/// Each drawing's entry, while the file has room for it within `budget`, then its pictures, each
/// while there is room. A picture left out is a miss when the entry is read.
pub(super) fn write(writer: &mut PackageWriter, drawings: &[(u64, SheetLines)], budget: usize) -> Result<()> {
    for (drawing, lines) in drawings {
        let bytes = match lines {
            SheetLines::Views(views) if views.is_empty() => continue,
            SheetLines::Views(views) => {
                Cow::Owned(serde_json::to_vec(&Written { build: native_cache::build(), format: FORMAT, views }).map_err(invalid)?)
            }
            SheetLines::Kept(filed) => Cow::Borrowed(&filed.entry[..]),
        };
        if bytes.len() > MAX_ENTRY || !writer.room(1, bytes.len(), budget) {
            continue;
        }
        writer.part(&name(*drawing), &bytes, true)?;
        // Encoded only once the entry is in: a picture without its entry is never read.
        let pictures: Vec<((u64, u64), Cow<[u8]>)> = match lines {
            SheetLines::Views(views) => {
                views.iter().flat_map(|v| v.pictures.iter().filter_map(|p| Some(((v.view, p.source), Cow::Owned(png(p)?))))).collect()
            }
            SheetLines::Kept(filed) => filed.pictures.iter().map(|(key, png)| (*key, Cow::Borrowed(&png[..]))).collect(),
        };
        for (key, png) in pictures {
            if png.len() <= MAX_PICTURE_PNG && writer.room(1, png.len(), budget) {
                writer.part(&picture_name(*drawing, key), &png, false)?;
            }
        }
    }
    Ok(())
}

/// Each drawing's entry the file holds, unread, with its pictures' parts: a name that isn't a
/// drawing's or a picture's, or a part too large or damaged, is left out.
pub(super) fn read<R: Read + Seek>(reader: &mut PackageReader<R>) -> BTreeMap<u64, FiledSheet> {
    let names = reader.part_names();
    let drawing = |name: &str| name.strip_prefix(FOLDER)?.strip_suffix(".json").and_then(ids::decode);
    let mut filed: BTreeMap<u64, FiledSheet> =
        names.iter().filter_map(|name| Some((drawing(name)?, FiledSheet::from(&reader.limited_part(name, MAX_ENTRY).ok()?[..])))).collect();
    let picture = |name: &str| {
        let (drawing, rest) = name.strip_prefix(FOLDER)?.split_once('/')?;
        let (view, source) = rest.strip_suffix(".png")?.split_once('-')?;
        Some((ids::decode(drawing)?, (ids::decode(view)?, ids::decode(source)?)))
    };
    for name in &names {
        let Some((drawing, key)) = picture(name) else { continue };
        if filed.contains_key(&drawing)
            && let Ok(png) = reader.limited_part(name, MAX_PICTURE_PNG)
            && let Some(sheet) = filed.get_mut(&drawing)
        {
            sheet.pictures.insert(key, Arc::from(png));
        }
    }
    filed
}

/// The views of a drawing's entry, when this build wrote it: each that passes a paper scene's checks,
/// with the pictures whose parts decode to their size. None for an entry of another build, or one
/// that doesn't read.
pub fn sheet_views(filed: &FiledSheet) -> Vec<SheetView> {
    let Ok(entry) = serde_json::from_slice::<Stored>(&filed.entry) else {
        return Vec::new();
    };
    let trusted = native_cache::source_trusted(env!("SOLIDCRAFT_SOURCE_COMMIT"), cfg!(any(test, feature = "trust-unknown-source")));
    if entry.format != FORMAT || entry.build != native_cache::build() || !trusted {
        return Vec::new();
    }
    let decoded = |view: u64, mut picture: SheetPixels| {
        picture.rows = Some(rgba(filed.pictures.get(&(view, picture.source))?, picture.size)?);
        Some(picture)
    };
    let mut views: Vec<SheetView> = entry.views.into_iter().filter(SheetView::checks).collect();
    for v in &mut views {
        let view = v.view;
        v.pictures = std::mem::take(&mut v.pictures).into_iter().filter_map(|p| decoded(view, p)).collect();
    }
    views
}

/// A picture's rows as a PNG, through the encoder an export's pictures use; none if it has no
/// rows, or its rows aren't its size.
fn png(picture: &SheetPixels) -> Option<Vec<u8>> {
    let ([width, height], rows) = (picture.size, picture.rows.as_ref()?);
    let grid = PixelGrid { x0: 0.0, y0: 0.0, pitch: 25.4 / picture.dpi, width, height };
    let whole = GridWindow { x0: 0, y0: 0, x1: width, y1: height };
    crate::paper::picture_png(picture.source, &grid, whole, &Rows(rows), MAX_PICTURE_PNG).ok()
}

/// One picture's rows, as an export's writer asks for them.
struct Rows<'a>(&'a [u8]);

impl Pictures for Rows<'_> {
    fn rows(&self, _: u64, grid: &PixelGrid, window: GridWindow) -> std::result::Result<Vec<u8>, String> {
        let row = grid.width as usize * 4;
        let (top, bottom) = (window.y0 as usize * row, window.y1 as usize * row);
        if row * grid.height as usize != self.0.len() || window.x0 != 0 || window.x1 != grid.width {
            return Err("not its size".into());
        }
        self.0.get(top..bottom).map(<[u8]>::to_vec).ok_or_else(|| "past its rows".into())
    }
}

/// A PNG's pixels, when it is 8-bit RGBA of exactly `size`, within [`MAX_PICTURE_PIXELS`]; none
/// for anything else. The decoder is held to that size whatever the header says.
fn rgba(png: &[u8], [width, height]: [u32; 2]) -> Option<Arc<[u8]>> {
    let pixels = u64::from(width) * u64::from(height);
    if pixels == 0 || pixels > MAX_PICTURE_PIXELS {
        return None;
    }
    let bytes = usize::try_from(pixels * 4).ok()?;
    let mut decoder = ::png::Decoder::new_with_limits(Cursor::new(png), ::png::Limits { bytes: bytes + (1 << 20) });
    decoder.set_transformations(::png::Transformations::IDENTITY);
    let mut reader = decoder.read_info().ok()?;
    let info = reader.info();
    if (info.width, info.height, info.color_type, info.bit_depth) != (width, height, ::png::ColorType::Rgba, ::png::BitDepth::Eight) {
        return None;
    }
    let mut rows = vec![0; bytes];
    let frame = reader.next_frame(&mut rows).ok()?;
    (frame.buffer_size() == bytes).then(|| Arc::from(rows))
}

impl SheetView {
    /// Its lines pass the checks of a scene on the largest sheet, and its numbers are finite.
    fn checks(&self) -> bool {
        let finite = |p: Vec2| p.x.is_finite() && p.y.is_finite();
        let boundary = self.boundary.is_none_or(|b| finite(b.min) && finite(b.max));
        let side = Vec2::new(MAX_SIDE_MM, MAX_SIDE_MM);
        finite(self.model_centre) && boundary && Scene::new(side, vec![Group::clone(&self.group)]).validate().is_ok()
    }
}
