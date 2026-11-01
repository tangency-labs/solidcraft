//! A drawing's images on paper (W38e, #5425): each placement on a sheet as a group of the sheet's
//! scene ([`groups`]), each logo in its title block as a picture of the block's group ([`logo_items`],
//! #3875), and the pixels a writer asks for ([`Provider`]).
//!
//! One export makes one provider, which holds only what its sheet uses. It decodes a file the first
//! time a writer asks for it, and once however many placements show it: a writer asks in bands, and
//! a logo can sit twice on a sheet. What it may hold decoded is capped from the files' headers
//! before anything decodes: a small file can claim to hold more pixels than memory has, and each
//! file's own cap doesn't bound a sheet of them.

use std::cell::OnceCell;
use std::collections::{BTreeMap, BTreeSet};

use solidcraft_doc::canvas::base64_decode;
use solidcraft_doc::drawing::Drawing;
use solidcraft_doc::drawing::image::{ImageFile, SheetImage};
use solidcraft_doc::ids;
use solidcraft_geom::paper::{Group, Item, MAX_IMAGE_PIXELS, ObjectKind, Picture, Pictures};
use solidcraft_geom::pixel_grid::{GridWindow, PixelGrid};
use solidcraft_geom::title_block::CellKind;
use solidcraft_geom::{Aabb2, Vec2};
use solidcraft_render::sheet_image;

/// The placements on sheet `sheet` of `d`, by id.
fn on_sheet(d: &Drawing, sheet: u64) -> impl Iterator<Item = &SheetImage> {
    d.sheet_images.iter().map(|place| &**place).filter(move |place| place.sheet == sheet)
}

fn rect(place: &SheetImage) -> Aabb2 {
    let [x0, y0, x1, y1] = place.rect_mm;
    Aabb2 { min: Vec2::new(x0, y0), max: Vec2::new(x1, y1) }
}

/// Each image placed on sheet `sheet` of `d`, by id, as a group of the sheet's scene: its one
/// picture, whose pixels a [`Provider`] has under the placement's id.
pub fn groups(d: &Drawing, sheet: u64) -> Vec<Group> {
    let group = |place: &SheetImage| {
        let picture = Picture { source: place.id, rect: rect(place), opacity: 1.0 };
        Group { object: place.id, kind: ObjectKind::SheetImage, items: vec![Item::Picture(picture)] }
    };
    on_sheet(d, sheet).map(group).collect()
}

/// A logo in a title block cell on one sheet.
struct Logo<'a> {
    /// What its picture names: a number no placement on the sheet has, as a source is opaque.
    source: u64,
    file: &'a ImageFile,
    rect: Aabb2,
}

/// The largest rectangle of `pixels`' shape inside `room`, centred.
fn fitted(room: Aabb2, pixels: [u32; 2]) -> Aabb2 {
    let (size, centre) = (room.size(), room.center());
    let [w, h] = pixels.map(f64::from);
    let scale = (size.x / w).min(size.y / h);
    let half = Vec2::new(w * scale / 2.0, h * scale / 2.0);
    Aabb2 { min: centre - half, max: centre + half }
}

/// The logos of sheet `sheet` of `d`, in its block's cell order. A block that doesn't lay out has
/// none: the scene refuses it.
fn logos(d: &Drawing, sheet: u64) -> Vec<Logo<'_>> {
    let Some(record) = d.sheets.iter().find(|record| record.id == sheet) else { return Vec::new() };
    let (Some(block), Some(border)) = (&record.title_block, &record.border) else { return Vec::new() };
    let Some(layout) = border.layout(&record.sheet).ok().and_then(|on_sheet| block.layout(&on_sheet).ok()) else { return Vec::new() };
    let placements: BTreeSet<u64> = on_sheet(d, sheet).map(|place| place.id).collect();
    let mut sources = (1u64..).filter(|source| !placements.contains(source));
    let logo = |(cell, placed): (&solidcraft_geom::title_block::Cell, &solidcraft_geom::title_block::CellLayout)| {
        let CellKind::Text(text) = &cell.kind else { return None };
        let file = d.images.by_id(text.image?)?;
        Some(Logo { source: sources.next()?, file, rect: fitted(placed.value_box, file.pixels) })
    };
    block.cells.iter().zip(&layout.cells).filter_map(logo).collect()
}

/// Each logo on sheet `sheet` of `d` as a picture, for its block's group, whose pixels a [`Provider`]
/// has under the same source.
pub fn logo_items(d: &Drawing, sheet: u64) -> Vec<Item> {
    logos(d, sheet).iter().map(|logo| Item::Picture(Picture { source: logo.source, rect: logo.rect, opacity: 1.0 })).collect()
}

/// A file's pixels, or why it has none.
type Decoded = Result<sheet_image::SheetImage, String>;

/// The pixels of the images on one sheet, for the writers.
pub struct Provider<'a> {
    /// Each source: the file it shows, where it sits, and the id a refusal names (a placement's, or
    /// a logo's file's).
    places: BTreeMap<u64, (u64, Aabb2, u64)>,
    /// Each file the placements show, by id, and its pixels once a writer has asked for them.
    files: BTreeMap<u64, (&'a ImageFile, OnceCell<Decoded>)>,
}

impl<'a> Provider<'a> {
    /// The provider for sheet `sheet` of `d`, or why its files are too much to hold decoded: more
    /// than [`MAX_IMAGE_PIXELS`] between them, which is 256 MB.
    pub fn new(d: &'a Drawing, sheet: u64) -> Result<Provider<'a>, String> {
        let (mut places, mut files) = (BTreeMap::new(), BTreeMap::new());
        for place in on_sheet(d, sheet) {
            places.insert(place.id, (place.image, rect(place), place.id));
            if let Some(file) = d.images.by_id(place.image) {
                files.entry(file.id).or_insert((file, OnceCell::new()));
            }
        }
        for logo in logos(d, sheet) {
            places.insert(logo.source, (logo.file.id, logo.rect, logo.file.id));
            files.entry(logo.file.id).or_insert((logo.file, OnceCell::new()));
        }
        let total: u64 = files.values().map(|(file, _)| file.pixels.iter().map(|side| u64::from(*side)).product::<u64>()).sum();
        if total > MAX_IMAGE_PIXELS {
            return Err(format!("the sheet's {} image files have {total} pixels between them: at most {MAX_IMAGE_PIXELS}", files.len()));
        }
        Ok(Provider { places, files })
    }
}

fn decode(file: &ImageFile) -> Decoded {
    let bytes = base64_decode(&file.data).ok_or("its file isn't base64")?;
    sheet_image::SheetImage::decode(&bytes)
}

impl Pictures for Provider<'_> {
    fn rows(&self, source: u64, grid: &PixelGrid, window: GridWindow) -> Result<Vec<u8>, String> {
        let place = self.places.get(&source).ok_or_else(|| format!("image {}: it isn't on this sheet", ids::encode(source)))?;
        let named = |why: String| format!("image {}: {why}", ids::encode(place.2));
        let (file, pixels) = self.files.get(&place.0).ok_or_else(|| named("the drawing doesn't have its file".into()))?;
        let image = pixels.get_or_init(|| decode(file)).as_ref().map_err(|why| named(why.clone()))?;
        image.rows(place.1, grid, window).map_err(named)
    }
}

#[cfg(test)]
#[path = "drawing_pictures_tests.rs"]
pub(crate) mod tests;
