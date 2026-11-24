//! A drawing's images (W38e, #5426): a PNG or JPEG placed on a sheet, such as a logo or a photo.
//!
//! - **A file and its placements are two records.** A logo sits on every sheet: [`Drawing::images`]
//!   holds it once, and each [`SheetImage`] in [`Drawing::sheet_images`] says where one sits. A file
//!   goes with the last thing that uses it ([`Drawing::images_in_use`]): a placement, or a title
//!   block's cell that shows it in place of its text ([`Drawing::set_logo`]).
//! - **The bytes** travel with the drawing, as a part of its container of their own that the
//!   record names (`part`), so the design opens without the original file, and the file's path is
//!   never saved. The drawing holds them as bytes, never text; an older drawing's base64 `data` is
//!   decoded as it opens, and a plain JSON file can't hold them.
//! - **The caps** are checked from the file's bytes and header, before anything decodes its pixels:
//!   a small file can claim to hold more pixels than memory has. The header can lie in the record
//!   too, so reading a drawing compares the two ([`Drawing::check_image_files`]); an edit, which only
//!   ever adds a checked file, doesn't decode them again.
//!
//! The rules, beside the record's: a file is used; a placement names a file the drawing has, and is
//! on one of its sheets, in a rectangle on paper with a size; at most [`MAX_IMAGES`] files of at most
//! [`MAX_IMAGE_BYTES`] and [`MAX_IMAGE_PIXELS`] each, and [`MAX_IMAGES_BYTES`] between them; at most
//! [`MAX_SHEET_IMAGES`] placements; each list by id.

use std::collections::BTreeSet;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use solidcraft_geom::drawing_view::is_paper_coordinate;
use solidcraft_geom::paper::{MAX_IMAGE_PIXELS, MAX_SIDE_MM};
use solidcraft_geom::title_block::{Cell, CellKind};

use super::by_id::Record;
use super::{Drawing, require};
use crate::blob::Blob;
use crate::canvas::{base64_decode, image_info};
use crate::{DocError, Result, ids};

/// The most image files a drawing may have.
pub const MAX_IMAGES: usize = 1_000;
/// The most placements of them a drawing may have.
pub const MAX_SHEET_IMAGES: usize = 1_000;
/// The largest file an image may be.
pub const MAX_IMAGE_BYTES: usize = 4 << 20;
/// The most a drawing's image files may be between them: two of the largest and some, whose 12 MiB
/// of text leaves room under [`super::MAX_DRAWING_BYTES`] for the rest of the record.
pub const MAX_IMAGES_BYTES: usize = 9 << 20;

/// One image file of a drawing, however many places it sits.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(try_from = "ImageRecord")]
pub struct ImageFile {
    pub id: u64,
    /// `png` or `jpeg`, as the file's header says.
    pub format: String,
    /// Its width and height in pixels, as the file's header says.
    pub pixels: [u32; 2],
    /// The file's bytes.
    pub data: Blob,
}

/// An image file's record: its file is the container's part it names (`part`), or, in an older
/// drawing, base64 in `data`. Written, `data` is always empty: the bytes are never text.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImageRecord {
    #[serde(with = "crate::ids")]
    id: u64,
    format: String,
    pixels: [u32; 2],
    #[serde(default)]
    data: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    part: Option<String>,
}

impl TryFrom<ImageRecord> for ImageFile {
    type Error = DocError;
    fn try_from(record: ImageRecord) -> Result<ImageFile> {
        let data = match &record.part {
            Some(name) => {
                crate::blob::part(name).ok_or_else(|| DocError::Invalid(format!("image {}: its file `{name}` is missing", ids::encode(record.id))))?
            }
            None => base64_decode(&record.data)
                .map(Blob::from)
                .ok_or_else(|| DocError::Invalid(format!("image {}: its file isn't base64", ids::encode(record.id))))?,
        };
        Ok(ImageFile { id: record.id, format: record.format, pixels: record.pixels, data })
    }
}

/// Its record, always naming its part (handed to the container being written,
/// [`crate::blob::collecting_parts`]).
impl Serialize for ImageFile {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let part = Some(crate::blob::add_part(&self.data, &self.format));
        ImageRecord { id: self.id, format: self.format.clone(), pixels: self.pixels, data: String::new(), part }.serialize(serializer)
    }
}

/// One place an image file sits, on a sheet of a drawing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SheetImage {
    #[serde(with = "crate::ids")]
    pub id: u64,
    #[serde(with = "crate::ids")]
    pub sheet: u64,
    /// Where it sits, `[x0, y0, x1, y1]` in millimetres from the sheet's lower-left corner.
    pub rect_mm: [f64; 4],
    /// The file it shows, by id.
    #[serde(with = "crate::ids")]
    pub image: u64,
}

/// What a file's bytes and header say it is, when its caps allow it. Nothing is decoded.
fn file_info(bytes: &[u8]) -> Result<(&'static str, [u32; 2])> {
    require(bytes.len() <= MAX_IMAGE_BYTES, || format!("the file is over {} MiB", MAX_IMAGE_BYTES >> 20))?;
    let (format, pixels) = image_info(bytes).ok_or_else(|| DocError::Invalid("the file isn't a PNG or JPEG".into()))?;
    let [w, h] = pixels;
    require(u64::from(w) * u64::from(h) <= MAX_IMAGE_PIXELS, || format!("the file has {w} by {h} pixels, over {MAX_IMAGE_PIXELS}"))?;
    Ok((format, pixels))
}

impl ImageFile {
    /// The image file of a file's `bytes`, when its caps allow it: nothing is decoded.
    pub fn from_file(id: u64, bytes: &[u8]) -> Result<ImageFile> {
        let (format, pixels) = file_info(bytes)?;
        Ok(ImageFile { id, format: format.into(), pixels, data: bytes.into() })
    }
}

impl Record for ImageFile {
    fn id(&self) -> u64 {
        self.id
    }
}

impl Record for SheetImage {
    fn id(&self) -> u64 {
        self.id
    }
}

impl Drawing {
    /// The image files something in the drawing uses: the one list of what keeps a file. Anything new
    /// that shows a file adds its own here.
    pub fn images_in_use(&self) -> BTreeSet<u64> {
        self.sheet_images.iter().map(|placement| placement.image).chain(self.logos()).collect()
    }

    /// The image files the sheets' title blocks show, once for each cell that shows one.
    pub fn logos(&self) -> impl Iterator<Item = u64> + '_ {
        self.sheets.iter().filter_map(|sheet| sheet.title_block.as_ref()).flat_map(|block| block.logos())
    }

    /// The id of the file with `file`'s bytes: one the drawing has, or `file`, added. The same bytes
    /// are the same file, so a logo on every sheet is stored once.
    pub fn keep_file(&mut self, file: ImageFile) -> u64 {
        if let Some(known) = self.images.iter().find(|known| known.data == file.data) {
            return known.id;
        }
        let id = file.id;
        self.images.add([Arc::new(file)]);
        id
    }

    /// Text cell `cell` of the title blocks of `sheets`, or of every sheet whose block has it, shows
    /// `file` in place of its text; with none, its text again. Answers the sheets it set, each once,
    /// and the file's id. Every sheet is checked before any changes, so a refusal changes nothing.
    pub fn set_logo(&mut self, sheets: Option<&[u64]>, cell: &str, file: Option<ImageFile>) -> Result<(Vec<u64>, Option<u64>)> {
        /// The cell named `cell` of `sheet`'s title block.
        fn named<'a>(sheet: &'a super::Sheet, cell: &str) -> Option<&'a Cell> {
            sheet.title_block.as_ref().and_then(|block| block.cells.iter().find(|c| c.name == cell))
        }
        let chosen: Vec<u64> = match sheets {
            Some(sheets) => {
                require(!sheets.is_empty(), || "the list of sheets names no sheet".into())?;
                let mut seen = BTreeSet::new();
                sheets.iter().copied().filter(|id| seen.insert(*id)).collect()
            }
            None => {
                let chosen: Vec<u64> = self.sheets.iter().filter(|sheet| named(sheet, cell).is_some()).map(|sheet| sheet.id).collect();
                require(!chosen.is_empty(), || format!("no sheet's title block has a cell named `{cell}`"))?;
                chosen
            }
        };
        for id in &chosen {
            let sheet =
                self.sheets.iter().find(|sheet| sheet.id == *id).ok_or_else(|| DocError::Invalid(format!("no sheet {}", ids::encode(*id))))?;
            match named(sheet, cell).map(|c| &c.kind) {
                Some(CellKind::Text(_)) => {}
                Some(CellKind::ProjectionSymbol) => return Err(DocError::Invalid(format!("`{cell}` isn't a text cell"))),
                None => return Err(DocError::Invalid(format!("sheet {} has no title block cell named `{cell}`", ids::encode(*id)))),
            }
        }
        let shown = file.map(|file| self.keep_file(file));
        for sheet in self.sheets.iter_mut().filter(|sheet| chosen.contains(&sheet.id)) {
            let found = Arc::make_mut(sheet).title_block.as_mut().and_then(|block| block.cells.iter_mut().find(|c| c.name == cell));
            if let Some(CellKind::Text(text)) = found.map(|c| &mut c.kind) {
                text.image = shown;
            }
        }
        self.drop_unused_images();
        Ok((chosen, shown))
    }

    /// Takes out each image file nothing uses now.
    pub fn drop_unused_images(&mut self) {
        let used = self.images_in_use();
        self.images.retain(|file| used.contains(&file.id));
    }

    pub(super) fn check_images(&self) -> Result<()> {
        // The placements first: one that names no file says so, before its file reads as unused.
        self.sheet_images.check(MAX_SHEET_IMAGES, "placed image", |placement| self.check_sheet_image(placement))?;
        let used = self.images_in_use();
        for logo in self.logos() {
            require(self.images.by_id(logo).is_some(), || {
                format!("a title block shows image {}, which the drawing doesn't have", ids::encode(logo))
            })?;
        }
        self.images.check(MAX_IMAGES, "image", |file| {
            check_image(file)?;
            require(used.contains(&file.id), || "nothing uses it".into())
        })?;
        let total = self.images.iter().fold(0, |total: usize, i| total.saturating_add(i.data.len()));
        require(total <= MAX_IMAGES_BYTES, || format!("a drawing's images are at most {} MiB between them", MAX_IMAGES_BYTES >> 20))
    }

    fn check_sheet_image(&self, i: &SheetImage) -> Result<()> {
        require(self.images.by_id(i.image).is_some(), || format!("it shows image {}, which the drawing doesn't have", ids::encode(i.image)))?;
        require(self.sheets.iter().any(|s| s.id == i.sheet), || format!("it is on sheet {}, which the drawing doesn't have", ids::encode(i.sheet)))?;
        let [x0, y0, x1, y1] = i.rect_mm;
        let on_paper = [x0, y0, x1, y1].into_iter().all(is_paper_coordinate);
        require(on_paper && x0 < x1 && y0 < y1, || format!("its rectangle has a size and is at most {MAX_SIDE_MM} mm on paper"))
    }

    /// Each image's bytes are the file its record says: what reading a drawing checks once, so that
    /// nothing later trusts a size the file doesn't have.
    pub(super) fn check_image_files(&self) -> Result<()> {
        self.images.iter().try_for_each(|i| {
            let (format, pixels) = file_info(&i.data).map_err(|e| DocError::Invalid(format!("image {}: {e}", ids::encode(i.id))))?;
            require(format == i.format && pixels == i.pixels, || format!("image {}: its record and its file disagree", ids::encode(i.id)))
        })
    }
}

/// Where a drawing's record keeps its image files, their placements and its logos when it has any:
/// fields of their own at its top level (also in the drawing reader's record fields), which a release that can't read image parts (0.45.0) sets
/// aside, keeps as they came and writes back, opening the rest of the drawing. Its logo cells show
/// their text there; nothing is lost when it saves.
pub(super) const FILES: &str = "image_files";
pub(super) const PLACED: &str = "placed_images";
pub(super) const LOGOS: &str = "logo_cells";

/// A logo's on-disk placement, separate from the text cell it accompanies.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LogoCell {
    #[serde(with = "crate::ids")]
    pub sheet: u64,
    #[serde(rename = "cell")]
    _cell: String,
    #[serde(with = "crate::ids")]
    pub image: u64,
}

/// Moves a drawing record's image files, placements and logos to the fields of their own above.
pub(super) fn set_aside(v: &mut serde_json::Value) {
    use serde_json::{Value, json};
    let Some(top) = v.as_object_mut() else { return };
    let files = top.remove("images").unwrap_or(Value::Null);
    let placed = top.remove("sheet_images").unwrap_or(Value::Null);
    if files.as_array().is_none_or(Vec::is_empty) {
        return;
    }
    let mut logos = Vec::new();
    for sheet in top.get_mut("sheets").and_then(Value::as_array_mut).into_iter().flatten() {
        let id = sheet.get("id").cloned().unwrap_or(Value::Null);
        for cell in sheet.pointer_mut("/title_block/cells").and_then(Value::as_array_mut).into_iter().flatten() {
            let name = cell.get("name").cloned().unwrap_or(Value::Null);
            if let Some(image) = cell.pointer_mut("/kind/text").and_then(Value::as_object_mut).and_then(|text| text.remove("image")) {
                logos.push(json!({"sheet": id, "cell": name, "image": image}));
            }
        }
    }
    top.insert(FILES.into(), files);
    if placed.as_array().is_some_and(|p| !p.is_empty()) {
        top.insert(PLACED.into(), placed);
    }
    if !logos.is_empty() {
        top.insert(LOGOS.into(), Value::Array(logos));
    }
}

/// Puts a record's set-aside image files, placements and logos back where the drawing reads them.
/// A placement on a sheet the drawing no longer has, or a logo for a cell that is gone or no
/// longer text (an older release changed the drawing around them), is left out, and so is a file
/// nothing then uses: the rest of the drawing reads.
pub(super) fn take_back(v: &mut serde_json::Value) {
    use serde_json::Value;
    let Some(top) = v.as_object_mut() else { return };
    let Some(files) = top.remove(FILES) else { return };
    let placed = top.remove(PLACED);
    let logos = top.remove(LOGOS);
    let sheet_ids: BTreeSet<String> = top
        .get("sheets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|s| s.get("id").and_then(Value::as_str).map(String::from))
        .collect();
    let mut used = BTreeSet::new();
    let placed: Vec<Value> = placed
        .and_then(|p| p.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.get("sheet").and_then(Value::as_str).is_some_and(|s| sheet_ids.contains(s)))
        .inspect(|p| {
            if let Some(image) = p.get("image").and_then(Value::as_str) {
                used.insert(image.to_string());
            }
        })
        .collect();
    for logo in logos.and_then(|l| l.as_array().cloned()).unwrap_or_default() {
        let (Some(sheet), Some(cell), Some(image)) =
            (logo.get("sheet").and_then(Value::as_str), logo.get("cell").and_then(Value::as_str), logo.get("image").and_then(Value::as_str))
        else {
            continue;
        };
        let text = top
            .get_mut("sheets")
            .and_then(Value::as_array_mut)
            .and_then(|sheets| sheets.iter_mut().find(|s| s.get("id").and_then(Value::as_str) == Some(sheet)))
            .and_then(|s| s.pointer_mut("/title_block/cells"))
            .and_then(Value::as_array_mut)
            .and_then(|cells| cells.iter_mut().find(|c| c.get("name").and_then(Value::as_str) == Some(cell)))
            .and_then(|c| c.pointer_mut("/kind/text"))
            .and_then(Value::as_object_mut);
        if let Some(text) = text {
            text.insert("image".into(), Value::String(image.to_string()));
            used.insert(image.to_string());
        }
    }
    let files: Vec<Value> = files
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|f| f.get("id").and_then(Value::as_str).is_some_and(|id| used.contains(id)))
        .collect();
    if !files.is_empty() {
        top.insert("images".into(), Value::Array(files));
    }
    if !placed.is_empty() {
        top.insert("sheet_images".into(), Value::Array(placed));
    }
}

/// A file's own rules, from its record alone: what every edit keeps, without decoding it.
fn check_image(i: &ImageFile) -> Result<()> {
    require(matches!(i.format.as_str(), "png" | "jpeg"), || "it is a PNG or a JPEG".into())?;
    let [w, h] = i.pixels;
    require(w > 0 && h > 0 && u64::from(w) * u64::from(h) <= MAX_IMAGE_PIXELS, || format!("it has 1 to {MAX_IMAGE_PIXELS} pixels"))?;
    require(i.data.len() <= MAX_IMAGE_BYTES, || format!("its file is at most {} MiB", MAX_IMAGE_BYTES >> 20))
}

#[cfg(test)]
#[path = "image_tests.rs"]
mod tests;
