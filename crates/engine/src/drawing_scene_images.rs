//! The pixels of a sheet's images for the sheet page (#18413): each image placed on the sheet shown
//! and each logo in its title block, made once for its file, its rectangle and the sheet's dpi, and
//! handed on as a [`SheetPicture`] as a shaded view's picture is. As a view's picture is drawn, the
//! one long piece, the file's decode, is [`super::begin`]'s job beside the window, and [`super::step`]
//! makes its rows a band at a time within its budget. One image is made at a time, so one decoded
//! file is held, and let go once its rows are made. The rows are the export's own (the same
//! [`SheetImage::rows`]), and the sheet is held to the export's caps: its files' pixels together from
//! their headers ([`Provider::new`]), before anything decodes. What the page holds of one sheet's
//! images is capped together too ([`SHEET_IMAGE_PIXELS`]).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use solidcraft_doc::drawing::Drawing;
use solidcraft_doc::drawing::image::ImageFile;
use solidcraft_doc::ids;
use solidcraft_geom::Aabb2;
use solidcraft_geom::paper::Picture;
use solidcraft_geom::pixel_grid::{GridWindow, PixelGrid};
use solidcraft_render::sheet_image::SheetImage;

use super::{CMD, REQUEST, SheetPicture, WINDOW_PICTURE_PIXELS, dpi_now, no_pixels, page_grid};
use crate::Session;
use crate::drawing_pictures::{Provider, decode, placed};

/// The most pixels the page draws one sheet's images with between them: four of the largest
/// picture, 256 MB of rows.
const SHEET_IMAGE_PIXELS: u64 = 4 * WINDOW_PICTURE_PIXELS;

/// About the pixels a band makes: some milliseconds' work, so a step stays within its budget.
const BAND_PIXELS: u64 = 1 << 16;

/// Each image's pixels on the sheet shown, by source.
pub(super) type Images = BTreeMap<u64, ImagePixels>;

/// What an image's pixels are made from: its file, its rectangle and the sheet's dpi. The same
/// three are the same pixels.
#[derive(Clone, Debug)]
pub(super) struct ImageFrom {
    file: Arc<ImageFile>,
    rect: Aabb2,
    dpi: Option<f64>,
}

impl ImageFrom {
    fn same(&self, other: &ImageFrom) -> bool {
        Arc::ptr_eq(&self.file, &other.file) && self.rect == other.rect && self.dpi == other.dpi
    }
}

/// One image's pixels on the page, or why it has none, and what they were made from.
#[derive(Clone, Debug)]
pub(super) struct ImagePixels {
    from: ImageFrom,
    grid: PixelGrid,
    rows: Result<Arc<[u8]>, String>,
}

/// One image of the sheet shown as the page wants it now: its grid, or the grid it has none on and
/// why.
struct Wanted {
    source: u64,
    /// The id its refusals name.
    named: u64,
    from: ImageFrom,
    grid: Result<PixelGrid, (PixelGrid, String)>,
}

/// The image whose rows [`step`] is making: its decoded file, and its rows so far.
pub(super) struct Imaging {
    source: u64,
    from: ImageFrom,
    grid: PixelGrid,
    image: SheetImage,
    rows: Vec<u8>,
}

impl Imaging {
    fn is_for(&self, w: &Wanted) -> bool {
        self.source == w.source && self.from.same(&w.from)
    }
}

/// Each image on sheet `sheet` of `d` as the page wants it at `dpi`, in [`placed`]'s order, but one
/// whose source is a view picture's (only a hostile file's placement can be): the page tells
/// pictures apart by source.
fn wanted(d: &Drawing, sheet: u64, dpi: Option<f64>) -> Vec<Wanted> {
    if d.images.is_empty() {
        return Vec::new();
    }
    let views = crate::drawing_picture::sources_on(d, sheet);
    // The export's refusal of the whole sheet: its files' pixels together, from their headers.
    let refused = Provider::new(d, sheet).err();
    let mut total = 0u64;
    let mut out = Vec::new();
    for place in placed(d, sheet) {
        let Some(file) = d.images.index_of(place.file).and_then(|at| d.images.get(at)) else { continue };
        if views.contains(&place.source) {
            continue;
        }
        let picture = Picture { source: place.source, rect: place.rect, opacity: 1.0 };
        let none = |why: String| (no_pixels(&picture), why);
        let named = |why: String| none(format!("image {}: {why}", ids::encode(place.named)));
        let grid = match &refused {
            Some(why) => Err(none(why.clone())),
            None => page_grid(&picture, dpi).map_err(named).and_then(|grid| {
                total = total.saturating_add(grid.pixels());
                if total > SHEET_IMAGE_PIXELS {
                    return Err(named(format!("the sheet's images are over {SHEET_IMAGE_PIXELS} pixels on the page together")));
                }
                Ok(grid)
            }),
        };
        let from = ImageFrom { file: file.clone(), rect: place.rect, dpi };
        out.push(Wanted { source: place.source, named: place.named, from, grid });
    }
    out
}

/// The images of the sheet shown that have a grid and no pixels made for what they show now.
fn missing(s: &Session) -> Vec<Wanted> {
    let (Ok(d), Some(sheet)) = (s.drawing(CMD), s.drawing_scenes.sheet) else { return Vec::new() };
    let images = &s.drawing_scenes.images;
    let made = |w: &Wanted| images.get(&w.source).is_some_and(|p| p.from.same(&w.from));
    wanted(d, sheet, dpi_now(s)).into_iter().filter(|w| w.grid.is_ok() && !made(w)).collect()
}

/// Whether an image of the sheet shown waits for its pixels.
pub(super) fn waiting(s: &Session) -> bool {
    !missing(s).is_empty()
}

/// The decode of the first image waiting, beside the window.
pub(super) struct ImageJob {
    request: u64,
    source: u64,
    named: u64,
    from: ImageFrom,
    grid: PixelGrid,
}

/// An image job's decoded file, or why it won't decode.
pub(super) struct ImageDone {
    job: ImageJob,
    decoded: Result<SheetImage, String>,
}

/// Asks for the decode of the first image waiting: the latest request from now on. None when none
/// waits, or it is the one being made (its bands are [`step`]'s).
pub(super) fn job(s: &mut Session) -> Option<ImageJob> {
    let first = missing(s).into_iter().next()?;
    if s.drawing_scenes.imaging.as_ref().is_some_and(|i| i.is_for(&first)) {
        return None;
    }
    let grid = first.grid.ok()?;
    let request = REQUEST.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    s.drawing_scenes.requested = Some(request);
    Some(ImageJob { request, source: first.source, named: first.named, from: first.from, grid })
}

impl ImageJob {
    /// Decodes its file, on any thread.
    pub(super) fn run(self) -> ImageDone {
        let decoded = decode(&self.from.file);
        ImageDone { job: self, decoded }
    }
}

/// Takes an image job's decode when it is the latest request and its image still waits, so [`step`]
/// makes its rows; one that won't decode keeps why, so it isn't tried again until its file, its
/// rectangle or the dpi changes. Says whether it was taken.
pub(super) fn finish(s: &mut Session, done: ImageDone) -> bool {
    let ImageDone { job, decoded } = done;
    if s.drawing_scenes.requested != Some(job.request) {
        return false;
    }
    s.drawing_scenes.requested = None;
    if !missing(s).iter().any(|w| w.source == job.source && w.from.same(&job.from)) {
        return false;
    }
    let ImageJob { source, named, from, grid, .. } = job;
    let cache = &mut s.drawing_scenes;
    match decoded {
        Ok(image) => cache.imaging = Some(Imaging { source, from, grid, image, rows: Vec::new() }),
        Err(why) => {
            cache.imaging = None;
            let rows = Err(format!("image {}: {why}", ids::encode(named)));
            cache.images.insert(source, ImagePixels { from, grid, rows });
            cache.serial = cache.serial.wrapping_add(1);
        }
    }
    true
}

/// Makes bands of the first image waiting until `spent` says so, its decode done here first when no
/// job has done it, and says whether one still waits. An image being made for anything else is
/// dropped. Its rows are handed on only once whole.
pub(super) fn step(s: &mut Session, spent: &mut dyn FnMut() -> bool) -> bool {
    let Some(first) = missing(s).into_iter().next() else {
        s.drawing_scenes.imaging = None;
        return false;
    };
    if !s.drawing_scenes.imaging.as_ref().is_some_and(|i| i.is_for(&first)) {
        s.drawing_scenes.imaging = None;
        if let Some(job) = job(s) {
            finish(s, job.run());
        }
    }
    let cache = &mut s.drawing_scenes;
    let Some(imaging) = cache.imaging.as_mut() else { return waiting(s) };
    let (width, height) = (imaging.grid.width, imaging.grid.height);
    let band = u32::try_from((BAND_PIXELS / u64::from(width).max(1)).max(1)).unwrap_or(u32::MAX);
    let made = loop {
        let y0 = u32::try_from(imaging.rows.len() / 4 / (width.max(1) as usize)).unwrap_or(u32::MAX);
        if y0 >= height {
            break Ok(());
        }
        let y1 = y0.saturating_add(band).min(height);
        match imaging.image.rows(imaging.from.rect, &imaging.grid, GridWindow { x0: 0, y0, x1: width, y1 }) {
            Ok(rows) => imaging.rows.extend(rows),
            Err(why) => break Err(why),
        }
        if y1 < height && spent() {
            return true;
        }
    };
    let Some(Imaging { source, from, grid, rows, .. }) = cache.imaging.take() else { return false };
    let rows = made.map(|()| Arc::from(rows));
    cache.images.insert(source, ImagePixels { from, grid, rows });
    cache.serial = cache.serial.wrapping_add(1);
    waiting(s)
}

/// The page's pictures of the images of the sheet shown, and whether one waits for its pixels: not
/// while a deferred drawing holds (`held`), as nothing is drawn then. The pixels of images no longer
/// shown are let go.
pub(super) fn pictures(s: &mut Session, d: &Drawing, held: bool) -> (Vec<SheetPicture>, bool) {
    let Some(sheet) = s.drawing_scenes.sheet else { return (Vec::new(), false) };
    let now = wanted(d, sheet, dpi_now(s));
    let shown: BTreeSet<u64> = now.iter().map(|w| w.source).collect();
    let images = &mut s.drawing_scenes.images;
    images.retain(|source, _| shown.contains(source));
    let (mut pictures, mut waiting) = (Vec::with_capacity(now.len()), false);
    for w in now {
        let kept = images.get(&w.source);
        let picture = match w.grid {
            Err((grid, why)) => SheetPicture { source: w.source, view: 0, grid, rows: None, stale: false, why: Some(why) },
            Ok(grid) => {
                let current = kept.is_some_and(|p| p.from.same(&w.from));
                waiting |= !current && !held;
                let (grid, rows, why) = match kept {
                    Some(p) => (p.grid, p.rows.as_ref().ok().cloned(), p.rows.as_ref().err().cloned()),
                    None => (grid, None, None),
                };
                SheetPicture { source: w.source, view: 0, grid, rows, stale: !current, why }
            }
        };
        pictures.push(picture);
    }
    (pictures, waiting)
}
