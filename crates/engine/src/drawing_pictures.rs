//! A drawing's images on paper (W38e, #5425): each placement on a sheet as a group of the sheet's
//! scene ([`groups`]), and the pixels a writer asks for ([`Provider`]).
//!
//! One export makes one provider, which holds only what its sheet uses. It decodes a file the first
//! time a writer asks for it, and once however many placements show it: a writer asks in bands, and
//! a logo can sit twice on a sheet. What it may hold decoded is capped from the files' headers
//! before anything decodes: a small file can claim to hold more pixels than memory has, and each
//! file's own cap doesn't bound a sheet of them.

use std::cell::OnceCell;
use std::collections::BTreeMap;

use solidcraft_doc::canvas::base64_decode;
use solidcraft_doc::drawing::Drawing;
use solidcraft_doc::drawing::image::{ImageFile, SheetImage};
use solidcraft_doc::ids;
use solidcraft_geom::paper::{Group, Item, MAX_IMAGE_PIXELS, ObjectKind, Picture, Pictures};
use solidcraft_geom::pixel_grid::{GridWindow, PixelGrid};
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

/// A file's pixels, or why it has none.
type Decoded = Result<sheet_image::SheetImage, String>;

/// The pixels of the images on one sheet, for the writers.
pub struct Provider<'a> {
    /// Each placement, by id: the file it shows and where it sits.
    places: BTreeMap<u64, (u64, Aabb2)>,
    /// Each file the placements show, by id, and its pixels once a writer has asked for them.
    files: BTreeMap<u64, (&'a ImageFile, OnceCell<Decoded>)>,
}

impl<'a> Provider<'a> {
    /// The provider for sheet `sheet` of `d`, or why its files are too much to hold decoded: more
    /// than [`MAX_IMAGE_PIXELS`] between them, which is 256 MB.
    pub fn new(d: &'a Drawing, sheet: u64) -> Result<Provider<'a>, String> {
        let (mut places, mut files) = (BTreeMap::new(), BTreeMap::new());
        for place in on_sheet(d, sheet) {
            places.insert(place.id, (place.image, rect(place)));
            if let Some(file) = d.images.by_id(place.image) {
                files.entry(file.id).or_insert((file, OnceCell::new()));
            }
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
        let named = |why: String| format!("image {}: {why}", ids::encode(source));
        let place = self.places.get(&source).ok_or_else(|| named("it isn't on this sheet".into()))?;
        let (file, pixels) = self.files.get(&place.0).ok_or_else(|| named("the drawing doesn't have its file".into()))?;
        let image = pixels.get_or_init(|| decode(file)).as_ref().map_err(|why| named(why.clone()))?;
        image.rows(place.1, grid, window).map_err(named)
    }
}

#[cfg(test)]
#[path = "drawing_pictures_tests.rs"]
pub(crate) mod tests;
